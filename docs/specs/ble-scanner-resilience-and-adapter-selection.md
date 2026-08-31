# BLE scanner: resilience, adapter selection, adapter-independent identity

Status: Implemented — `backend/src/ble/scanner.rs`,
`backend/src/ble/adapter_power.rs`, `backend/src/config.rs`,
`backend/migrations/0003_strip_adapter_prefix.sql`
Last updated: 2026-08-31

## Why

Diagnosed live on the homelab (k3s) after a TP-Link UB500 long-range USB
Bluetooth adapter was added alongside the existing one:

1. **The scanner died silently.** `scanner::run` was
   `while let Some(event) = events.next().await { … }`. It stops two
   ways, both silent: the stream **ends** (`None`) on a full adapter
   reset (`bluetoothd` restart, dongle re-plugged), *or* it **hangs** —
   `.next()` never resolves, the stream never closes — when the dongle
   wedges (see §Stall recovery). Either way `run` never regained control;
   the API kept serving stale rows. Caused ~26 h silent outages, twice.

2. **The adapter was not selectable.** `run` took
   `manager.adapters().into_iter().next()` with no override. With two
   adapters the choice is not stable across restarts — one restart bound
   the long-range dongle, the next could bind the short-range one.

3. **Adapter switching created duplicate devices.** `device_id` is
   `PeripheralId::to_string()`, which on BlueZ is `hciN/dev_AA_BB_…`. The
   same physical meter seen through a different adapter became a new
   `devices` row (and a new reading series). The homelab DB ended up with
   6 rows for 3 meters.

## Design

### 1. Self-healing scan loop

`scanner::run` is now a supervisor that never returns:

```
run(storage, reading_interval, adapter_name):
    state = ScannerState::default()          # caches, kept across reconnects
    backoff = 1s
    loop:
        started = now
        result = scan_session(&storage, reading_interval, adapter_name, &mut state)
        warn!(...)                            # stream ended, or errored
        if started.elapsed() >= 30s: backoff = 1s   # the session was healthy
        sleep(backoff)
        backoff = min(backoff * 2, 60s)
```

- `scan_session` is the former `run` body: create manager → select
  adapter → `start_scan` → consume `adapter.events()`. The event wait is
  `tokio::time::timeout(EVENT_TIMEOUT = 120 s)` — `Ok(None)` (stream
  ended) → return `Ok(())`; elapsed (silence) → return
  `Err(BleError::Stalled)` (see §Stall recovery). The per-reading
  `resolve_mac_address` D-Bus call is `timeout`-guarded too (10 s).
- `ScannerState` bundles the four per-device caches that used to be
  `run` locals (`last_stored`, `mac_known`, `switchbot_mfr_data`,
  `service_reading_seen`). Owned by `run`, borrowed `&mut` by
  `scan_session`, so a reconnect does **not** re-resolve every MAC or
  emit a burst of unthrottled readings.
- Infinite retry even with genuinely no adapter (dev Mac, no hardware /
  no permission): cost is one `WARN` per ≤60 s, and plugging a dongle in
  later then just works with no restart — strictly better than the old
  silent give-up. `main.rs` no longer wraps the call in
  `if let Err(...)` (run returns `()`).

### 2. Adapter selection — `BLE_ADAPTER`

- `Config.ble_adapter: Option<String>` from `BLE_ADAPTER`
  (empty string treated as unset).
- `select_adapter(manager, wanted)`:
  - `None` → `adapters().next()` (unchanged default).
  - `Some(name)` → first adapter whose `Central::adapter_info()` string
    (`"<hciN> (<modalias>)"`) **contains** `name`, so `name` can be an
    `hciN` name or a modalias fragment. **The modalias is not always the
    dongle's real USB id** — on the homelab it comes through as the
    generic root hub (`v1D6Bp0246`), not the UB500's `2357:0604` — so the
    production configmap pins `BLE_ADAPTER=hci1`. Check the startup log.
  - No match → `warn!` every connected adapter's info string (so a
    misconfigured value is obvious in the logs), then return
    `BleError::AdapterNotFound` — which the supervisor treats as
    retryable (the dongle may be plugged in later).
  - >1 match → use the first, `warn!` about the ambiguity.
- The chosen adapter's info string is logged at `info!` on every
  `BLE scan started`.

### 2a. Stall recovery — `ble/adapter_power.rs`

A Realtek RTL8761 dongle (the UB500) periodically stops delivering LE
advertisements while still reporting `UP` / `Discovering` — most likely a
bad resume from USB autosuspend. `hciconfig` RX counters freeze;
`bluetoothctl scan on` finds nothing. Only an HCI reset revives it —
`StartDiscovery` alone does not (verified by hand: `scan on` → 0 devices,
`power off/on` → 26).

- `scan_session`'s `EVENT_TIMEOUT` (120 s of event silence) →
  `Err(BleError::Stalled)`.
- The supervisor, on `Stalled` only, calls
  `adapter_power::power_cycle(BLE_ADAPTER)` before the backoff sleep: a
  BlueZ `Adapter1.Powered` false→true toggle via `bluez-async` (the same
  `0.8` btleplug already pins; a separate short-lived `BluetoothSession`).
  `cfg(target_os = "linux")` — a no-op stub elsewhere (CoreBluetooth has
  no adapter power control).
- When `BLE_ADAPTER` is set but no adapter matches, `power_cycle` does
  **not** fall back to some other adapter — cycling the wrong dongle is
  worse than nothing.
- Host-side prevention (removes the trigger): disable USB autosuspend for
  BT adapters (`options btusb enable_autosuspend=0`). See README
  Troubleshooting.

### 3. Adapter-independent `device_id`

- `strip_adapter_prefix(id)` — returns the part after the first `/`
  (`hciN/dev_AA_BB_… → dev_AA_BB_…`); ids without a `/` (macOS
  CoreBluetooth UUIDs, or already-stripped ids) pass through. Applied
  wherever the scanner derives the persisted `device_id`; the raw
  `PeripheralId` is still used for btleplug peripheral lookups
  (`resolve_mac_address`).
- No `domain` / `storage` / API change — `device_id` is still the `TEXT`
  primary key, just without the prefix. On BlueZ it now equals the
  `mac_address` column (underscores vs. colons).

### Migration `0003_strip_adapter_prefix.sql`

Rewrites existing rows to the stripped form, merging duplicates. Runs
once via `sqlx::migrate!`. Statement order keeps the
`readings.device_id → devices.device_id` FK satisfied throughout (it is
enforced by default):

1. `INSERT … SELECT substr(device_id, instr(device_id,'/')+1) … GROUP BY target
   ON CONFLICT(device_id) DO UPDATE …` — create/merge the stripped rows.
   Merge rule: `MIN(first_seen_at)`, `MAX(last_seen_at)`,
   `COALESCE`/`MAX` for the rest. (The real meters are unlabeled, so the
   merge is trivial; the rule covers the general case.)
2. `UPDATE readings SET device_id = <stripped> WHERE has a '/'`.
3. `DELETE FROM devices WHERE has a '/'`.

`MIN`/`MAX` over the ISO-8601 UTC text timestamps is correct (lexical ==
chronological for that fixed format). Verified against a copy of the real
homelab DB and a synthetic 6-row/3-pair DB: 3 devices out, prefixes
gone, reading counts preserved, `PRAGMA foreign_key_check` clean.

Forward-only: once `0003` has run, the image can't be rolled back to one
that predates it (sqlx rejects a DB whose applied-migration list is ahead
of the binary). Same as every prior migration; the homelab deploys
forward.

## Testing

Pure-function unit tests in `scanner.rs` (`strip_adapter_prefix`,
`adapter_matches`, `next_backoff`). The supervisor loop, the stall
watchdog + power-cycle, and the migration merge are verified manually
(no async-BLE or migration-state harness exists — consistent with how
0002 was covered). The `bluez-async` power-cycle code is Linux-only, so
it is compile-checked in a `rust:1-slim` + `libdbus-1-dev` container, not
by the macOS `cargo` runs. End-to-end: on the homelab, let the dongle
stall (or `bluetoothctl power off` it) and confirm the logs show
`... adapter appears to have stalled` → `power-cycling the Bluetooth
adapter` → `BLE scan started` → readings resume, no pod restart.

## Operational

k3s configmap: `BLE_ADAPTER: "hci1"` and `READING_INTERVAL_SECONDS: "30"`
(both set as of 2026-08-31). `BLE_ADAPTER` uses the `hciN` name, not a
modalias fragment — see §2. Host: disable USB autosuspend for BT adapters
(`/etc/modprobe.d/btusb.conf` → `options btusb enable_autosuspend=0`) so
the dongle stops wedging in the first place.
