use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use btleplug::api::{BDAddr, Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use chrono::{DateTime, Duration, Utc};
use futures::StreamExt;
use tracing::{debug, info, warn};

use crate::ble::switchbot;
use crate::domain::NewReading;
use crate::storage::SqliteStorage;

#[derive(Debug, thiserror::Error)]
pub enum BleError {
    #[error("no Bluetooth adapter found on this machine")]
    NoAdapter,
    #[error(transparent)]
    Btleplug(#[from] btleplug::Error),
}

/// Scans for BLE advertisements indefinitely, storing every SwitchBot
/// Meter/Meter Plus reading it recognizes. Runs until the adapter's event
/// stream ends, which in practice means until the process exits.
///
/// `reading_interval` throttles how often a reading is stored *per
/// device*: a `Some(interval)` drops advertisements that arrive less than
/// `interval` after the last one actually stored for that device. `None`
/// stores every advertisement that parses successfully — Meter Plus
/// broadcasts every few seconds, so this can add up fast.
pub async fn run(
    storage: Arc<SqliteStorage>,
    reading_interval: Option<Duration>,
) -> Result<(), BleError> {
    let manager = Manager::new().await?;
    let adapter = manager
        .adapters()
        .await?
        .into_iter()
        .next()
        .ok_or(BleError::NoAdapter)?;

    adapter.start_scan(ScanFilter::default()).await?;
    info!("BLE scan started");

    // In-memory only: resets on restart, which just means the first
    // reading after a restart is never throttled. Not persisted, since
    // that would mean a DB round-trip per advertisement just to check the
    // throttle — defeating half the point of throttling in the first
    // place.
    let mut last_stored: HashMap<String, DateTime<Utc>> = HashMap::new();
    // Devices for which a real MAC has already been fetched this process
    // lifetime — separate from `last_stored` so a failed/placeholder
    // lookup (e.g. a transient BlueZ D-Bus error, or macOS/CoreBluetooth's
    // permanent placeholder) gets retried on the device's next
    // throttle-surviving reading instead of being silently forgone for
    // good.
    let mut mac_known: HashSet<String> = HashSet::new();
    // Latest SwitchBot manufacturer data seen per device. `btleplug` splits
    // a single advertising PDU into separate service-data and
    // manufacturer-data events, so this cache reunites them: the Outdoor
    // Meter's temperature/humidity (and every meter's real MAC) live in the
    // manufacturer data, not the service data.
    let mut switchbot_mfr_data: HashMap<String, Vec<u8>> = HashMap::new();
    // When each device's service data last yielded a reading. While that is
    // recent, the device's manufacturer-data-only advertisements are skipped
    // as a lower-quality duplicate (service data also carries battery); once
    // it goes stale — the meter is too far to answer a `SCAN_REQ` any more —
    // the manufacturer-data path takes over.
    let mut service_reading_seen: HashMap<String, DateTime<Utc>> = HashMap::new();

    let mut events = adapter.events().await?;
    while let Some(event) = events.next().await {
        // A single advertising PDU arrives as separate service-data and
        // manufacturer-data events. Both can yield a reading; the common
        // throttle/store tail below runs once on whichever produced one.
        let (id, device_id, parsed, mfr_data) = match &event {
            CentralEvent::ManufacturerDataAdvertisement {
                id,
                manufacturer_data,
            } => {
                let Some(data) = manufacturer_data.get(&switchbot::SWITCHBOT_COMPANY_ID) else {
                    continue;
                };
                let device_id = id.to_string();
                switchbot_mfr_data.insert(device_id.clone(), data.clone());

                if recently_seen(
                    &service_reading_seen,
                    &device_id,
                    Duration::seconds(SERVICE_DATA_PREFERENCE_SECS),
                ) {
                    continue;
                }
                let Some(parsed) = switchbot::parse_meter_manufacturer_data(data) else {
                    continue;
                };
                debug!(device = %id, "switchbot reading recovered from manufacturer data");
                (id.clone(), device_id, parsed, Some(data.clone()))
            }
            CentralEvent::ServiceDataAdvertisement { id, service_data } => {
                let Some(data) = service_data.iter().find_map(|(uuid, data)| {
                    switchbot::is_switchbot_service_uuid(uuid).then_some(data)
                }) else {
                    continue;
                };
                debug!(device = %id, raw = ?data, "switchbot advertisement received");

                let device_id = id.to_string();
                let mfr_data = switchbot_mfr_data.get(&device_id).cloned();

                let Some(parsed) = switchbot::parse_meter_advertisement(data, mfr_data.as_deref())
                else {
                    debug!(device = %id, "advertisement did not match a known meter format");
                    continue;
                };
                service_reading_seen.insert(device_id.clone(), Utc::now());
                (id.clone(), device_id, parsed, mfr_data)
            }
            _ => continue,
        };

        if is_throttled(&last_stored, &device_id, reading_interval) {
            debug!(device = %device_id, "skipping reading: within the throttle interval");
            continue;
        }

        // Skip the lookup once a real MAC is already known for this device —
        // it doesn't change, and a property lookup is unnecessary overhead
        // on every throttle-surviving reading otherwise.
        let mac_address = if mac_known.contains(&device_id) {
            None
        } else {
            resolve_mac_address(&adapter, &id, mfr_data.as_deref()).await
        };
        if mac_address.is_some() {
            mac_known.insert(device_id.clone());
        }

        store_reading(&storage, &device_id, mac_address.as_deref(), parsed).await;
        last_stored.insert(device_id, Utc::now());
    }

    Ok(())
}

/// How long (seconds) a device's service-data reading keeps its
/// manufacturer-data-only advertisements suppressed. One hour: long enough
/// to ride out a meter that only answers a `SCAN_REQ` occasionally, short
/// enough that a meter which has genuinely stopped delivering service data
/// falls back well within a day.
const SERVICE_DATA_PREFERENCE_SECS: i64 = 60 * 60;

fn recently_seen(seen: &HashMap<String, DateTime<Utc>>, device_id: &str, window: Duration) -> bool {
    seen.get(device_id)
        .is_some_and(|at| Utc::now() - *at < window)
}

/// Resolves a device's real MAC, preferring the link-layer address
/// (`btleplug`'s peripheral properties — the genuine MAC on BlueZ) and
/// falling back to the one SwitchBot meters embed in their manufacturer
/// data. The fallback is what makes a real MAC available on macOS, where
/// CoreBluetooth never exposes the link-layer address.
async fn resolve_mac_address(
    adapter: &Adapter,
    id: &PeripheralId,
    manufacturer_data: Option<&[u8]>,
) -> Option<String> {
    if let Some(mac) = fetch_mac_address(adapter, id).await {
        return Some(mac);
    }
    let mac = switchbot::mac_from_manufacturer_data(manufacturer_data?)?;
    Some(BDAddr::from(mac).to_string())
}

fn is_throttled(
    last_stored: &HashMap<String, DateTime<Utc>>,
    device_id: &str,
    reading_interval: Option<Duration>,
) -> bool {
    let Some(interval) = reading_interval else {
        return false;
    };
    match last_stored.get(device_id) {
        Some(last) => Utc::now() - *last < interval,
        None => false,
    }
}

/// Looks up the real BLE MAC via the adapter's peripheral properties.
/// Returns `None` on BlueZ if the lookup fails, and always on macOS/
/// CoreBluetooth, which never exposes the real hardware address to apps
/// (it reports the all-zero placeholder `BDAddr::default()` instead) — see
/// `docs/specs/architecture.md` §3.
async fn fetch_mac_address(adapter: &Adapter, id: &PeripheralId) -> Option<String> {
    let peripheral = adapter.peripheral(id).await.ok()?;
    let properties = peripheral.properties().await.ok()??;
    (properties.address != BDAddr::default()).then(|| properties.address.to_string())
}

async fn store_reading(
    storage: &SqliteStorage,
    device_id: &str,
    mac_address: Option<&str>,
    parsed: switchbot::ParsedReading,
) {
    let now = Utc::now();
    let device = match storage
        .upsert_device_seen(device_id, mac_address, now)
        .await
    {
        Ok(device) => device,
        Err(err) => {
            warn!(device = device_id, error = %err, "failed to record device sighting");
            return;
        }
    };
    let Some(_device) = device else {
        debug!(
            device = device_id,
            "skipping reading for blacklisted device"
        );
        return;
    };

    let reading = NewReading {
        device_id: device_id.to_string(),
        temperature: parsed.temperature,
        humidity: parsed.humidity,
        battery: parsed.battery,
        recorded_at: now,
    };
    if let Err(err) = storage.insert_reading(&reading).await {
        warn!(device = device_id, error = %err, "failed to store reading");
    } else {
        info!(
            device = device_id,
            temperature = parsed.temperature,
            humidity = parsed.humidity,
            "reading stored"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_interval_never_throttles() {
        let mut last_stored = HashMap::new();
        last_stored.insert("AA:BB".to_string(), Utc::now());

        assert!(!is_throttled(&last_stored, "AA:BB", None));
    }

    #[test]
    fn a_device_seen_for_the_first_time_is_never_throttled() {
        let last_stored = HashMap::new();

        assert!(!is_throttled(
            &last_stored,
            "AA:BB",
            Some(Duration::seconds(30))
        ));
    }

    #[test]
    fn a_device_stored_within_the_interval_is_throttled() {
        let mut last_stored = HashMap::new();
        last_stored.insert("AA:BB".to_string(), Utc::now());

        assert!(is_throttled(
            &last_stored,
            "AA:BB",
            Some(Duration::seconds(30))
        ));
    }

    #[test]
    fn a_device_stored_before_the_interval_elapsed_is_not_throttled() {
        let mut last_stored = HashMap::new();
        last_stored.insert("AA:BB".to_string(), Utc::now() - Duration::seconds(31));

        assert!(!is_throttled(
            &last_stored,
            "AA:BB",
            Some(Duration::seconds(30))
        ));
    }

    #[test]
    fn throttling_one_device_does_not_affect_another() {
        let mut last_stored = HashMap::new();
        last_stored.insert("AA:BB".to_string(), Utc::now());

        assert!(!is_throttled(
            &last_stored,
            "CC:DD",
            Some(Duration::seconds(30))
        ));
    }

    #[test]
    fn a_device_with_no_service_reading_is_not_recently_seen() {
        let seen = HashMap::new();

        assert!(!recently_seen(&seen, "AA:BB", Duration::seconds(3600)));
    }

    #[test]
    fn a_recent_service_reading_suppresses_the_manufacturer_path() {
        let mut seen = HashMap::new();
        seen.insert("AA:BB".to_string(), Utc::now());

        assert!(recently_seen(&seen, "AA:BB", Duration::seconds(3600)));
    }

    #[test]
    fn a_stale_service_reading_no_longer_suppresses_the_manufacturer_path() {
        let mut seen = HashMap::new();
        seen.insert("AA:BB".to_string(), Utc::now() - Duration::seconds(3601));

        assert!(!recently_seen(&seen, "AA:BB", Duration::seconds(3600)));
    }
}
