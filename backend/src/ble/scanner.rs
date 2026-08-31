use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};

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
    #[error("no Bluetooth adapter matched BLE_ADAPTER={0:?}")]
    AdapterNotFound(String),
    #[error("no BLE advertisement for {0:?}; the adapter appears to have stalled")]
    Stalled(StdDuration),
    #[error(transparent)]
    Btleplug(#[from] btleplug::Error),
}

/// Caches kept across scan-session reconnects (see `run`). Persisting
/// them means a transient adapter drop doesn't re-resolve every device's
/// MAC or emit a burst of unthrottled readings the moment scanning
/// resumes. In-memory only — a full process restart still starts fresh,
/// which at worst stores one extra reading per device.
#[derive(Default)]
struct ScannerState {
    /// `device_id` → when a reading was last actually stored for it.
    last_stored: HashMap<String, DateTime<Utc>>,
    /// `device_id`s whose real MAC has already been resolved. A failed or
    /// placeholder lookup is deliberately *not* recorded here, so it's
    /// retried on the device's next throttle-surviving reading.
    mac_known: HashSet<String>,
    /// `device_id` → latest SwitchBot manufacturer data. `btleplug` splits
    /// one advertising PDU into separate service-data and manufacturer-data
    /// events; this reunites them (the Outdoor Meter's temperature/humidity
    /// and every meter's real MAC live in the manufacturer data).
    switchbot_mfr_data: HashMap<String, Vec<u8>>,
    /// `device_id` → when service data last yielded a reading. While recent,
    /// the device's manufacturer-data-only advertisements are skipped as a
    /// lower-quality duplicate (service data also carries battery); once
    /// stale — the meter is too far to answer a `SCAN_REQ` — the
    /// manufacturer-data path takes over.
    service_reading_seen: HashMap<String, DateTime<Utc>>,
}

/// First and maximum delay between scan-session (re)connect attempts.
const INITIAL_BACKOFF: StdDuration = StdDuration::from_secs(1);
const MAX_BACKOFF: StdDuration = StdDuration::from_secs(60);
/// A session that ran at least this long before ending counts as "was
/// healthy", so the backoff resets instead of creeping up over a series of
/// unrelated, widely-spaced adapter blips.
const HEALTHY_SESSION: StdDuration = StdDuration::from_secs(30);
/// If no advertisement event arrives for this long, treat the adapter as
/// stalled. The meters alone broadcast every few seconds, and btleplug
/// emits an event for every nearby BLE device's adverts too, so total
/// silence this long means the adapter has stopped delivering — not a
/// quiet moment.
const EVENT_TIMEOUT: StdDuration = StdDuration::from_secs(120);
/// Cap on the per-device MAC-resolution D-Bus round-trip.
const MAC_LOOKUP_TIMEOUT: StdDuration = StdDuration::from_secs(10);

/// Scans for BLE advertisements forever, storing every SwitchBot
/// Meter/Meter Plus reading it recognizes.
///
/// A single `scan_session` call is not enough: btleplug's BlueZ event
/// stream can end silently (adapter reset, `bluetoothd` restart) *or* go
/// silent without ending at all (a wedged Realtek dongle keeps the stream
/// open but delivers nothing). This supervises it — on a clean stream
/// end it reconnects; on a stall (`EVENT_TIMEOUT` of event silence) it
/// power-cycles the adapter first — with exponential backoff, and never
/// returns.
///
/// `reading_interval` throttles stored readings *per device*.
/// `adapter_name`, when set, selects which adapter to scan (see
/// `adapter_matches`); unset picks the first adapter found.
pub async fn run(
    storage: Arc<SqliteStorage>,
    reading_interval: Option<Duration>,
    adapter_name: Option<String>,
) {
    let mut state = ScannerState::default();
    let mut backoff = INITIAL_BACKOFF;
    // Set when a stall is seen, cleared only once a session runs long
    // enough to have definitely worked. While set, every retry is
    // preceded by an adapter power-cycle — so a `set_powered(true)` that
    // didn't take gets tried again rather than leaving the adapter off.
    let mut needs_power_cycle = false;
    loop {
        if needs_power_cycle {
            // An HCI reset is what actually revives a wedged dongle; a
            // plain StartDiscovery on the next attempt would not.
            super::adapter_power::power_cycle(adapter_name.as_deref()).await;
        }

        let started = Instant::now();
        let outcome = scan_session(
            &storage,
            reading_interval,
            adapter_name.as_deref(),
            &mut state,
        )
        .await;

        let stalled = matches!(outcome, Err(BleError::Stalled(_)));
        let clean = ran_clean(started.elapsed(), stalled);

        match outcome {
            Ok(()) => warn!("BLE event stream ended; reconnecting"),
            Err(err @ BleError::Stalled(_)) => {
                warn!(error = %err, "recovering the Bluetooth adapter");
                needs_power_cycle = true;
            }
            Err(err) => warn!(error = %err, "BLE scan session failed; retrying"),
        }

        if clean {
            backoff = INITIAL_BACKOFF;
            needs_power_cycle = false;
        }
        tokio::time::sleep(backoff).await;
        backoff = next_backoff(backoff);
    }
}

fn next_backoff(current: StdDuration) -> StdDuration {
    (current * 2).min(MAX_BACKOFF)
}

/// Whether a finished scan session counts as a clean run: it lasted long
/// enough to have definitely been scanning, and it did not end in a
/// stall. A stall is reported only after `EVENT_TIMEOUT` (> `HEALTHY_SESSION`),
/// so without the `!stalled` guard a stalled session would look "healthy"
/// and clear the recovery flag before the power-cycle ever ran.
fn ran_clean(elapsed: StdDuration, stalled: bool) -> bool {
    elapsed >= HEALTHY_SESSION && !stalled
}

/// One scan attempt: pick the adapter, start scanning, and consume its
/// advertisement stream until it ends or errors. Returns `Ok(())` when
/// the stream simply ends (the common case on an adapter reset).
async fn scan_session(
    storage: &SqliteStorage,
    reading_interval: Option<Duration>,
    adapter_name: Option<&str>,
    state: &mut ScannerState,
) -> Result<(), BleError> {
    let manager = Manager::new().await?;
    let adapter = select_adapter(&manager, adapter_name).await?;

    adapter.start_scan(ScanFilter::default()).await?;
    let info = adapter.adapter_info().await.unwrap_or_default();
    info!(adapter = %info, "BLE scan started");

    let mut events = adapter.events().await?;
    loop {
        // A wedged dongle keeps this stream open but stops delivering, so
        // a plain `.next().await` would park here forever. Bound the wait:
        // a total silence past `EVENT_TIMEOUT` means the adapter stalled.
        let event = match tokio::time::timeout(EVENT_TIMEOUT, events.next()).await {
            Ok(Some(event)) => event,
            Ok(None) => return Ok(()),
            Err(_) => return Err(BleError::Stalled(EVENT_TIMEOUT)),
        };

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
                let device_id = strip_adapter_prefix(&id.to_string()).to_string();
                state
                    .switchbot_mfr_data
                    .insert(device_id.clone(), data.clone());

                if recently_seen(
                    &state.service_reading_seen,
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

                let device_id = strip_adapter_prefix(&id.to_string()).to_string();
                let mfr_data = state.switchbot_mfr_data.get(&device_id).cloned();

                let Some(parsed) = switchbot::parse_meter_advertisement(data, mfr_data.as_deref())
                else {
                    debug!(device = %id, "advertisement did not match a known meter format");
                    continue;
                };
                state
                    .service_reading_seen
                    .insert(device_id.clone(), Utc::now());
                (id.clone(), device_id, parsed, mfr_data)
            }
            _ => continue,
        };

        if is_throttled(&state.last_stored, &device_id, reading_interval) {
            debug!(device = %device_id, "skipping reading: within the throttle interval");
            continue;
        }

        // Skip the lookup once a real MAC is already known for this device —
        // it doesn't change, and a property lookup is unnecessary overhead
        // on every throttle-surviving reading otherwise. The lookup is a
        // D-Bus round-trip, so cap it: a sick adapter must not wedge the
        // whole loop here (it just means no MAC this reading, retried next).
        let mac_address = if state.mac_known.contains(&device_id) {
            None
        } else {
            tokio::time::timeout(
                MAC_LOOKUP_TIMEOUT,
                resolve_mac_address(&adapter, &id, mfr_data.as_deref()),
            )
            .await
            .unwrap_or_else(|_| {
                warn!(device = %device_id, "MAC lookup timed out");
                None
            })
        };
        if mac_address.is_some() {
            state.mac_known.insert(device_id.clone());
        }

        store_reading(storage, &device_id, mac_address.as_deref(), parsed).await;
        state.last_stored.insert(device_id, Utc::now());
    }
}

/// Picks the adapter to scan. With `wanted` set, the first adapter whose
/// `Central::adapter_info` string contains it (an `hciN` name or a USB
/// modalias fragment both work); without, the first adapter found.
async fn select_adapter(manager: &Manager, wanted: Option<&str>) -> Result<Adapter, BleError> {
    let adapters = manager.adapters().await?;
    let Some(wanted) = wanted else {
        return adapters.into_iter().next().ok_or(BleError::NoAdapter);
    };

    let mut infos: Vec<(Adapter, String)> = Vec::with_capacity(adapters.len());
    for adapter in adapters {
        let info = adapter.adapter_info().await.unwrap_or_default();
        infos.push((adapter, info));
    }

    let match_count = infos
        .iter()
        .filter(|(_, info)| adapter_matches(info, wanted))
        .count();
    if match_count == 0 {
        // Surface what *is* connected so a wrong BLE_ADAPTER is obvious
        // from the logs rather than a bare "not found".
        for (_, info) in &infos {
            warn!(adapter = %info, "available BLE adapter (none matched BLE_ADAPTER)");
        }
        return Err(BleError::AdapterNotFound(wanted.to_string()));
    }
    if match_count > 1 {
        warn!(
            matches = match_count,
            "BLE_ADAPTER matched multiple adapters; using the first"
        );
    }
    Ok(infos
        .into_iter()
        .find(|(_, info)| adapter_matches(info, wanted))
        .map(|(adapter, _)| adapter)
        .expect("match_count > 0 guarantees a match"))
}

/// Whether an adapter whose `Central::adapter_info` string is `info`
/// should be used for the configured `BLE_ADAPTER` value `wanted`. A
/// plain substring test: `info` looks like `"hci1 (usb:v2357p0604d…)"`,
/// so `wanted` can be an `hciN` name or a USB modalias fragment (the
/// latter survives `hciN` renumbering across reboots).
fn adapter_matches(info: &str, wanted: &str) -> bool {
    info.contains(wanted)
}

/// btleplug's BlueZ `PeripheralId` stringifies as `hciN/dev_AA_BB_…`, so
/// the same physical meter seen through a different adapter would become a
/// new device row. Strip the `hciN/` prefix (there is exactly one `/`) so
/// identity is adapter-independent. A macOS `PeripheralId` is a bare UUID
/// with no `/`, and an already-stripped `dev_…` id, both pass through
/// unchanged. Matches migration `0003`'s `instr(device_id, '/')`.
fn strip_adapter_prefix(id: &str) -> &str {
    id.split_once('/').map(|(_, tail)| tail).unwrap_or(id)
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

    #[test]
    fn strip_adapter_prefix_removes_the_bluez_hci_segment() {
        assert_eq!(
            strip_adapter_prefix("hci0/dev_D2_2E_81_06_5C_61"),
            "dev_D2_2E_81_06_5C_61"
        );
        assert_eq!(
            strip_adapter_prefix("hci1/dev_D2_2E_81_06_5C_61"),
            "dev_D2_2E_81_06_5C_61"
        );
    }

    #[test]
    fn strip_adapter_prefix_leaves_ids_without_a_slash_untouched() {
        // macOS: a bare CoreBluetooth UUID.
        assert_eq!(
            strip_adapter_prefix("574AD03B-4384-2307-708E-08D01FD8174D"),
            "574AD03B-4384-2307-708E-08D01FD8174D"
        );
        // Already stripped.
        assert_eq!(
            strip_adapter_prefix("dev_D2_2E_81_06_5C_61"),
            "dev_D2_2E_81_06_5C_61"
        );
    }

    #[test]
    fn adapter_matches_on_hci_name_or_modalias_fragment() {
        let info = "hci1 (usb:v2357p0604d0002dc00dsc01dp01ic00isc00ip00in00)";
        assert!(adapter_matches(info, "hci1"));
        assert!(adapter_matches(info, "v2357p0604"));
        assert!(!adapter_matches("hci0 (usb:v0BDAp8771d0200)", "hci1"));
    }

    #[test]
    fn next_backoff_doubles_and_caps_at_the_maximum() {
        assert_eq!(
            next_backoff(StdDuration::from_secs(1)),
            StdDuration::from_secs(2)
        );
        assert_eq!(next_backoff(StdDuration::from_secs(32)), MAX_BACKOFF);
        assert_eq!(next_backoff(MAX_BACKOFF), MAX_BACKOFF);
    }

    #[test]
    fn a_stalled_session_is_never_treated_as_a_clean_run() {
        // Even though it lasted well past HEALTHY_SESSION.
        assert!(!ran_clean(EVENT_TIMEOUT, true));
    }

    #[test]
    fn a_long_non_stalled_session_is_a_clean_run_but_a_short_one_is_not() {
        assert!(ran_clean(HEALTHY_SESSION, false));
        assert!(!ran_clean(
            HEALTHY_SESSION - StdDuration::from_secs(1),
            false
        ));
    }
}
