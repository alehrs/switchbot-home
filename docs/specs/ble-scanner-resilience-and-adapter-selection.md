# BLE scanner: resilience, adapter selection, adapter-independent identity

Status: Implemented — `backend/src/ble/scanner.rs`, `backend/src/config.rs`,
`backend/migrations/0003_strip_adapter_prefix.sql`
Last updated: 2026-08-28

## Why

Diagnosed live on the homelab (k3s) after a TP-Link UB500 long-range USB
Bluetooth adapter was added alongside the existing one:

1. **The scanner died silently.** `scanner::run` was
   `while let Some(event) = events.next().await { … }`. When BlueZ ends
   the advertisement stream — which it does *without an error* on any
   adapter reset (USB re-enumeration, `bluetoothd` restart, dongle
   re-plugged) — the loop just exited, `run` returned `Ok(())`, and the
   spawned task finished. No error, no restart; the API kept serving
   stale data. This caused a ~26 h silent outage the moment the second
   dongle was plugged in.

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
  adapter → `start_scan` → consume `adapter.events()` until it ends or
  errors. Returns `Ok(())` on a clean stream end (the common case).
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
    **contains** `name`. `adapter_info()` returns e.g.
    `"hci1 (usb:v2357p0604d0002…)"`, so `name` can be an `hciN` name or a
    USB modalias fragment (`v2357p0604`) — the latter survives `hciN`
    renumbering across reboots and is the recommended form.
  - No match → `warn!` every connected adapter's info string (so a
    misconfigured value is obvious in the logs), then return
    `BleError::AdapterNotFound` — which the supervisor treats as
    retryable (the dongle may be plugged in later).
  - >1 match → use the first, `warn!` about the ambiguity.
- The chosen adapter's info string is logged at `info!` on every
  `BLE scan started`.

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
`adapter_matches`, `next_backoff`). The supervisor loop and the migration
merge are verified manually (no async-BLE or migration-state harness
exists — consistent with how 0002 was covered). End-to-end: on the
homelab, unplug/replug the dongle and confirm the logs show
`BLE event stream ended; reconnecting` → `BLE scan started` → readings
resume within ~1 min with no pod restart.

## Operational

Set in the k3s configmap: `BLE_ADAPTER: "v2357p0604"` (the UB500's USB
modalias fragment) and, finally, `READING_INTERVAL_SECONDS` (still unset
in production — every `ADV_IND` is stored otherwise).
