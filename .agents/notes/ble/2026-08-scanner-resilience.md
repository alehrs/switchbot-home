---
date: 2026-08-28
type: decision
tags: [ble, btleplug, bluez, scanner, adapter, device-id, resilience]
files:
  - backend/src/ble/scanner.rs
  - backend/src/config.rs
  - backend/migrations/0003_strip_adapter_prefix.sql
---

# btleplug BlueZ: the event stream ends silently on adapter reset; PeripheralId carries the hciN prefix

**Context.** A TP-Link UB500 was added to the homelab next to the existing
dongle. The collector then stored nothing for ~26 h with no error logged.

## The event stream dies without an error

`adapter.events()` on btleplug's BlueZ backend is a D-Bus signal stream.
When the adapter is reset — USB re-enumeration, `bluetoothd` restart,
dongle re-plugged, `hciN` renumbered — the stream simply **ends**
(`.next()` → `None`). The old `while let Some(event) = events.next().await`
loop then exited, `run` returned `Ok(())`, and the spawned task finished.
No error, no panic, no restart. The HTTP API kept serving stale rows, so
from the outside it looked alive.

**Fix.** `scanner::run` is now a supervisor loop around `scan_session`
(the former body). Any return — clean stream end *or* error — is logged
and retried with exponential backoff (1 s → 60 s cap; reset to 1 s after
a session that lasted ≥30 s). It never returns; `main.rs` dropped the
`if let Err(...)` wrapper. The per-device caches live in a `ScannerState`
owned by `run` so a reconnect doesn't re-resolve every MAC or dump a
burst of unthrottled readings.

## Adapter selection

`manager.adapters().into_iter().next()` is **not stable** with >1 adapter
(BlueZ object order isn't guaranteed). Added `BLE_ADAPTER`: substring
match against `Central::adapter_info()`, which returns
`"hci1 (usb:v2357p0604d0002…)"`. So the value can be `hci1` **or** a USB
modalias fragment like `v2357p0604` — prefer the modalias, it survives
`hciN` renumbering across reboots. Unset → first-found (old behaviour).
Chosen adapter is logged at startup; no match → logs all connected
adapters then errors (retryable — dongle may appear later).

## device_id carried the adapter name

BlueZ `PeripheralId::to_string()` is `hciN/dev_AA_BB_CC_DD_EE_FF`. Since
that string *was* the `device_id` PK, the same meter via a different
adapter became a new device row + new reading series (homelab DB: 6 rows
for 3 meters). Fix: `strip_adapter_prefix` takes the part after the first
`/` before persisting (raw `PeripheralId` still used for
`adapter.peripheral(id)` calls). macOS ids are bare UUIDs (no `/`) and
are untouched. Migration `0003` rewrites + merges existing prefixed rows;
statement order (insert stripped → repoint readings → delete prefixed)
keeps the `readings → devices` FK valid the whole way.

**Gotcha.** `rustfmt` / `cargo fmt` follows `mod` declarations
recursively. Running `rustfmt backend/src/main.rs` reformats the *whole*
crate reachable from it, including the pre-existing fmt deviations in
`storage.rs` / `api/mod.rs` that earlier sessions deliberately left
alone. Format the leaf files you touched directly, or `git checkout` the
collateral.

See [[2026-08-meter-manufacturer-data]] for the SCAN_RSP / range finding
that motivated the long-range dongle in the first place.
