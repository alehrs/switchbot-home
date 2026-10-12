use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};

use btleplug::api::{BDAddr, Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use chrono::{DateTime, Duration, Utc};
use futures::StreamExt;
use tracing::{debug, info, warn};

use crate::ble::switchbot;
use crate::domain::NewReading;
use crate::health::ScannerHealth;
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
/// Consecutive attempts failing with a lost system-bus connection before the
/// btleplug `Manager` is rebuilt. See `ManagerSlot`.
const MANAGER_RECREATE_AFTER: u32 = 5;

/// Owns the btleplug `Manager` for the scanner's whole lifetime.
///
/// On BlueZ, `Manager::new()` opens a system-bus connection whose driver task
/// btleplug spawns and then discards the handle of, so dropping a `Manager`
/// does **not** close it. Creating one per scan attempt therefore leaked one
/// connection per retry until dbus-daemon refused the process (256 per UID),
/// after which nothing could recover short of a restart. Create it once and
/// reuse it across every retry instead.
///
/// It is rebuilt only when its connection looks dead — see `connection_lost`
/// — and only after `MANAGER_RECREATE_AFTER` such failures in a row, because
/// rebuilding a *live* connection is itself the leak. Rebuilding a dead one
/// is not: its driver task has ended, so the old fd is already closed.
struct ManagerSlot<M> {
    manager: Option<M>,
    lost_connection_streak: u32,
}

impl<M> ManagerSlot<M> {
    fn new() -> Self {
        Self {
            manager: None,
            lost_connection_streak: 0,
        }
    }

    /// The current `Manager`, created on first use. A failed creation leaves
    /// nothing behind (btleplug spawns the driver only after connecting), so
    /// retrying it is leak-free.
    async fn get_or_create<F, Fut>(&mut self, create: F) -> Result<&M, BleError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<M, btleplug::Error>>,
    {
        if self.manager.is_none() {
            self.manager = Some(create().await?);
        }
        Ok(self.manager.as_ref().expect("manager was just created"))
    }

    /// Records how an attempt ended. `connection_lost` is whether the failure
    /// points at the system-bus connection rather than at the adapter.
    fn record_attempt(&mut self, connection_lost: bool) {
        // Without a manager there is nothing to rebuild, and a failed
        // creation must not pre-load the streak of the next one.
        if !connection_lost || self.manager.is_none() {
            self.lost_connection_streak = 0;
            return;
        }
        self.lost_connection_streak += 1;
        if self.lost_connection_streak >= MANAGER_RECREATE_AFTER {
            warn!(
                failures = self.lost_connection_streak,
                "system D-Bus connection appears dead; recreating the BLE manager"
            );
            self.manager = None;
            self.lost_connection_streak = 0;
        }
    }
}

/// Whether a failed attempt says the `Manager`'s own system-bus connection is
/// dead, as opposed to the adapter being missing, stalled or refusing a
/// request — none of which a new connection would fix.
fn connection_lost(err: &BleError) -> bool {
    match err {
        BleError::Btleplug(err) => btleplug_connection_lost(err),
        _ => false,
    }
}

/// btleplug's BlueZ backend reports every D-Bus failure as
/// `Error::Other(BluetoothError::DbusError(_))`, so the D-Bus error name is
/// the only thing left to tell them apart. Linux only: CoreBluetooth has no
/// system-bus connection to leak, so there it is never "lost".
#[cfg(target_os = "linux")]
fn btleplug_connection_lost(err: &btleplug::Error) -> bool {
    use bluez_async::BluetoothError;

    let btleplug::Error::Other(source) = err else {
        return false;
    };
    match source.downcast_ref::<BluetoothError>() {
        Some(BluetoothError::DbusError(err)) => is_local_dbus_failure(err.name()),
        _ => false,
    }
}

#[cfg(not(target_os = "linux"))]
fn btleplug_connection_lost(_err: &btleplug::Error) -> bool {
    false
}

/// D-Bus error names that a *dead local connection* produces. Observed by
/// killing dbus-daemon under a live btleplug `Manager`: the first calls time
/// out (`Timeout`), then every call fails locally with `Failed` ("Failed to
/// send message"). `Disconnected` is libdbus's own name for it, not observed.
/// A reply from a live bus or from `bluetoothd` — `ServiceUnknown` when
/// `bluetoothd` is down, `NoReply`/`Timeout` when it hangs, `org.bluez.Error.*`
/// — proves the connection works, so those must not trigger a rebuild.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_local_dbus_failure(name: Option<&str>) -> bool {
    matches!(
        name,
        Some("org.freedesktop.DBus.Error.Failed" | "org.freedesktop.DBus.Error.Disconnected")
    )
}

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
    health: Arc<ScannerHealth>,
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
    let mut power_cycler = None;
    let mut manager_slot = ManagerSlot::new();
    loop {
        if needs_power_cycle {
            // An HCI reset is what actually revives a wedged dongle; a
            // plain StartDiscovery on the next attempt would not.
            super::adapter_power::power_cycle(&mut power_cycler, adapter_name.as_deref()).await;
        }

        let started = Instant::now();
        let outcome = match manager_slot.get_or_create(Manager::new).await {
            Ok(manager) => {
                scan_session(
                    manager,
                    &storage,
                    &health,
                    reading_interval,
                    adapter_name.as_deref(),
                    &mut state,
                )
                .await
            }
            Err(err) => Err(err),
        };
        manager_slot.record_attempt(outcome.as_ref().err().is_some_and(connection_lost));
        health.record_session_ended(outcome.as_ref().err().map(ToString::to_string));

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
    manager: &Manager,
    storage: &SqliteStorage,
    health: &ScannerHealth,
    reading_interval: Option<Duration>,
    adapter_name: Option<&str>,
    state: &mut ScannerState,
) -> Result<(), BleError> {
    let adapter = select_adapter(manager, adapter_name).await?;

    adapter.start_scan(ScanFilter::default()).await?;
    let info = adapter.adapter_info().await.unwrap_or_default();
    info!(adapter = %info, "BLE scan started");
    health.record_scan_started();

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
        health.record_event();

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

        if store_reading(storage, &device_id, mac_address.as_deref(), parsed).await {
            health.record_reading_stored();
        }
        state.last_stored.insert(device_id, Utc::now());
    }
}

/// Picks the adapter to scan. With `wanted` set, the first adapter whose
/// `Central::adapter_info` string contains it (an `hciN` name or a USB
/// modalias fragment), or whose Linux USB product matches it; without, the
/// first adapter found.
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
/// should be used for `BLE_ADAPTER`. `hciN` names and BlueZ modalias
/// fragments remain supported. `usb:vvvv:pppp` reads the adapter's USB
/// product from sysfs, so it remains stable even when BlueZ exposes the
/// parent USB hub's modalias or renumbers the HCI device.
pub(crate) fn adapter_matches(info: &str, wanted: &str) -> bool {
    info.contains(wanted)
        || adapter_usb_product(info)
            .as_deref()
            .is_some_and(|product| usb_product_matches(product, wanted))
}

fn adapter_usb_product(info: &str) -> Option<String> {
    let adapter = info.split_whitespace().next()?;
    let uevent =
        std::fs::read_to_string(format!("/sys/class/bluetooth/{adapter}/device/uevent")).ok()?;
    uevent
        .lines()
        .find_map(|line| line.strip_prefix("PRODUCT=").map(str::to_owned))
}

fn usb_product_matches(product: &str, wanted: &str) -> bool {
    let Some(wanted) = wanted.strip_prefix("usb:") else {
        return false;
    };
    let Some((vendor, product_id)) = wanted.split_once(':') else {
        return false;
    };
    let Some((actual_vendor, rest)) = product.split_once('/') else {
        return false;
    };
    let Some((actual_product, _)) = rest.split_once('/') else {
        return false;
    };
    match (
        (parse_usb_hex(actual_vendor), parse_usb_hex(actual_product)),
        (parse_usb_hex(vendor), parse_usb_hex(product_id)),
    ) {
        ((Some(actual_vendor), Some(actual_product)), (Some(vendor), Some(product_id))) => {
            actual_vendor == vendor && actual_product == product_id
        }
        _ => false,
    }
}

fn parse_usb_hex(value: &str) -> Option<u16> {
    u16::from_str_radix(value, 16).ok()
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

/// Returns whether a reading was actually stored (not blacklisted, no error).
async fn store_reading(
    storage: &SqliteStorage,
    device_id: &str,
    mac_address: Option<&str>,
    parsed: switchbot::ParsedReading,
) -> bool {
    let now = Utc::now();
    let device = match storage
        .upsert_device_seen(device_id, mac_address, now)
        .await
    {
        Ok(device) => device,
        Err(err) => {
            warn!(device = device_id, error = %err, "failed to record device sighting");
            return false;
        }
    };
    let Some(_device) = device else {
        debug!(
            device = device_id,
            "skipping reading for blacklisted device"
        );
        return false;
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
        false
    } else {
        info!(
            device = device_id,
            temperature = parsed.temperature,
            humidity = parsed.humidity,
            "reading stored"
        );
        true
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
    fn usb_product_selector_matches_the_stable_vendor_and_product_ids() {
        assert!(usb_product_matches("2357/604/200", "usb:2357:0604"));
        assert!(usb_product_matches("2357/604/200", "usb:2357:604"));
        assert!(!usb_product_matches("13d3/3416/200", "usb:2357:0604"));
        assert!(!usb_product_matches("2357/604/200", "hci2"));
        assert!(!usb_product_matches("not-a-product", "usb:2357:0604"));
        assert!(!usb_product_matches(
            "nope/also-nope/200",
            "usb:bad:also-bad"
        ));
    }

    /// Drives `slot` through `attempts` failed attempts the way `run` does,
    /// returning how many `Manager`s were created.
    async fn count_creations(
        slot: &mut ManagerSlot<()>,
        attempts: usize,
        failure: impl Fn() -> BleError,
    ) -> u32 {
        let created = std::sync::atomic::AtomicU32::new(0);
        for _ in 0..attempts {
            slot.get_or_create(|| async {
                created.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap();
            slot.record_attempt(connection_lost(&failure()));
        }
        created.into_inner()
    }

    #[tokio::test]
    async fn failed_retries_reuse_one_manager_instead_of_creating_one_per_attempt() {
        // The original leak: a non-matching BLE_ADAPTER retried forever.
        let mut slot = ManagerSlot::new();
        let created = count_creations(&mut slot, 300, || {
            BleError::AdapterNotFound("hci2".to_string())
        })
        .await;

        assert_eq!(created, 1);
    }

    #[tokio::test]
    async fn stalls_do_not_recreate_the_manager() {
        let mut slot = ManagerSlot::new();
        let created = count_creations(&mut slot, 300, || BleError::Stalled(EVENT_TIMEOUT)).await;

        assert_eq!(created, 1);
    }

    #[test]
    fn only_a_dead_local_connection_counts_as_a_lost_connection() {
        assert!(is_local_dbus_failure(Some(
            "org.freedesktop.DBus.Error.Failed"
        )));
        assert!(is_local_dbus_failure(Some(
            "org.freedesktop.DBus.Error.Disconnected"
        )));
        // Replies from a live bus / bluetoothd: the connection works.
        assert!(!is_local_dbus_failure(Some(
            "org.freedesktop.DBus.Error.NoReply"
        )));
        assert!(!is_local_dbus_failure(Some(
            "org.freedesktop.DBus.Error.Timeout"
        )));
        assert!(!is_local_dbus_failure(Some(
            "org.freedesktop.DBus.Error.ServiceUnknown"
        )));
        assert!(!is_local_dbus_failure(Some("org.bluez.Error.NotReady")));
        assert!(!is_local_dbus_failure(None));
    }

    #[test]
    fn the_manager_is_rebuilt_only_after_enough_consecutive_lost_connections() {
        let mut slot = ManagerSlot::<()>::new();
        slot.manager = Some(());

        for _ in 0..MANAGER_RECREATE_AFTER - 1 {
            slot.record_attempt(true);
        }
        assert!(slot.manager.is_some());

        slot.record_attempt(true);
        assert!(slot.manager.is_none());
        assert_eq!(slot.lost_connection_streak, 0);
    }

    #[test]
    fn an_attempt_that_does_not_lose_the_connection_resets_the_streak() {
        let mut slot = ManagerSlot::<()>::new();
        slot.manager = Some(());

        for _ in 0..MANAGER_RECREATE_AFTER - 1 {
            slot.record_attempt(true);
        }
        slot.record_attempt(false);
        slot.record_attempt(true);

        assert!(slot.manager.is_some());
    }

    #[test]
    fn failed_creations_do_not_pre_load_the_streak_of_the_next_manager() {
        let mut slot = ManagerSlot::<()>::new();
        for _ in 0..MANAGER_RECREATE_AFTER * 2 {
            slot.record_attempt(true);
        }

        slot.manager = Some(());
        slot.record_attempt(true);

        assert!(slot.manager.is_some());
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
