use std::collections::HashMap;
use std::sync::Arc;

use btleplug::api::{Central, CentralEvent, Manager as _, ScanFilter};
use btleplug::platform::Manager;
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

    let mut events = adapter.events().await?;
    while let Some(event) = events.next().await {
        let CentralEvent::ServiceDataAdvertisement { id, service_data } = event else {
            continue;
        };

        for (uuid, data) in &service_data {
            if !switchbot::is_switchbot_service_uuid(uuid) {
                continue;
            }
            debug!(device = %id, raw = ?data, "switchbot advertisement received");

            let Some(parsed) = switchbot::parse_meter_advertisement(data) else {
                debug!(device = %id, "advertisement did not match a known meter format");
                continue;
            };

            let device_id = id.to_string();
            if is_throttled(&last_stored, &device_id, reading_interval) {
                debug!(device = %device_id, "skipping reading: within the throttle interval");
                continue;
            }

            store_reading(&storage, &device_id, parsed).await;
            last_stored.insert(device_id, Utc::now());
        }
    }

    Ok(())
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

async fn store_reading(storage: &SqliteStorage, device_id: &str, parsed: switchbot::ParsedReading) {
    let now = Utc::now();
    let device = match storage.upsert_device_seen(device_id, now).await {
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
        battery: Some(parsed.battery),
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
}
