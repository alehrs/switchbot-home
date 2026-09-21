---
date: 2026-08-28
updated: 2026-09-21
type: decision
tags: [ble, btleplug, bluez, scanner, adapter, device-id, resilience, realtek, autosuspend]
files:
  - backend/src/ble/scanner.rs
  - backend/src/ble/adapter_power.rs
  - backend/src/config.rs
  - backend/migrations/0003_strip_adapter_prefix.sql
---

# btleplug BlueZ scanner: the stream can hang (not just end); the RTL8761 dongle stalls; PeripheralId carries the hciN prefix

**Context.** A TP-Link UB500 was added to the homelab. The collector then
stored nothing for ~26 h at a time, with no error logged, more than once.

## The scanner stops in two different ways — and only one ends the stream

`adapter.events()` on btleplug's BlueZ backend is a D-Bus signal stream.

1. **It ends** (`.next()` → `None`) on a full adapter reset — `bluetoothd`
   restart, dongle re-plugged / `hciN` renumbered. The old
   `while let Some(event) = events.next().await` loop then exited, `run`
   returned `Ok(())`, the spawned task finished. Silent.
2. **It hangs** (`.next()` never resolves, stream never closes) when the
   dongle *wedges* — see below. This is what actually happened on the
   homelab: last log line was a normal `reading stored`, then nothing for
   2 days, no error. A supervisor that only reacts to `scan_session`
   *returning* never regains control here.

*(The 2026-08-28 version of this note claimed only case 1. Case 2 is the
common one in practice.)*

**Fix.** `scanner::run` is a supervisor loop around `scan_session`, with
exponential backoff (1 s → 60 s; reset after a ≥30 s session). Per-device
caches live in a `ScannerState` owned by `run` so a reconnect doesn't
re-resolve every MAC or dump a burst of unthrottled readings.
- Case 1: `scan_session` returns `Ok(())` → reconnect.
- Case 2: the event loop wraps `events.next()` in
  `tokio::time::timeout(EVENT_TIMEOUT = 120 s)`; total silence that long
  → `Err(BleError::Stalled)`. The supervisor then **power-cycles the
  adapter** (`backend/src/ble/adapter_power.rs`: BlueZ `Adapter1.Powered`
  false→true via `bluez-async` — same version btleplug pins,
  `cfg(target_os = "linux")` only) before retrying. A plain
  `StartDiscovery` on the next attempt does **not** revive a wedged
  dongle; only the power toggle (an HCI reset) does — proven by hand
  (`bluetoothctl scan on` found 0 devices, `power off/on` then found 26).
- The per-reading `resolve_mac_address` D-Bus call is also
  `timeout`-guarded (10 s) so it can't wedge the loop.

## The RTL8761 dongle stalls (root cause of the 2-day outage)

The UB500's Realtek RTL8761 stops delivering LE advertisements while
`hciconfig` still shows `UP RUNNING` and BlueZ still shows
`Discovering: yes`. Tells: `hciconfig hciX` `RX bytes` / `events`
counters **frozen**; `bluetoothctl scan on` finds nothing; `INQUIRY`
flag stuck.

Most likely a bad **resume from USB autosuspend**: the dongle had
`power/control=auto`, `autosuspend_delay_ms=2000`, `btusb
enable_autosuspend=Y`, and `active_duration` ≈ 40 % of wall-clock.
Host-side fix (removes the trigger, needs root):
`echo 'options btusb enable_autosuspend=0' > /etc/modprobe.d/btusb.conf`
then reload `btusb`. Documented in README Troubleshooting.

Recovery that works without root: `bluetoothctl power off; power on`.

## Adapter selection

`manager.adapters().into_iter().next()` is **not stable** with >1 adapter
(BlueZ object order isn't guaranteed). Added `BLE_ADAPTER`: substring
match (`info.contains(wanted)`) against `Central::adapter_info()`, which
is `"<hciN> (<modalias>)"`. So the value can be an `hciN` name **or** a
modalias fragment. Unset → first-found. Chosen adapter logged at startup;
no match → logs all connected adapters then errors (retryable).

**Gotcha — the modalias is not always the dongle's real USB id.** On the
homelab, `adapter_info()` for the UB500 is `"hci1 (usb:v1D6Bp0246d0552)"`
— `1d6b:0246` is the generic Linux Foundation root hub, **not** the
UB500's own `2357:0604`. So `BLE_ADAPTER=v2357p0604` would not match
there; the production configmap pins `BLE_ADAPTER=hci1`. Check the actual
startup log line before choosing a value.

**Update 2026-09-21 — do not create one power-cycle D-Bus session per
recovery.** The deployed watchdog created a `bluez-async::BluetoothSession`,
spawned its dispatch future, and aborted it after each `Powered` off/on toggle.
After the collector stalled, the pod accumulated 280 open socket FDs and BlueZ
started returning `The maximum number of active connections for UID 0 has been
reached`; recovery could no longer run. `PowerCycler` now keeps one session and
dispatch task for the scanner lifetime. The obvious short-lived-session approach
is wrong here: `BluetoothSession::new` has already spawned its D-Bus driver, so
aborting the wrapper task merely detaches that driver instead of closing the
system-bus connection.

The same outage showed that a physical UB500 may change from `hci1` to `hci2`.
Its BlueZ modalias is the parent hub, so `BLE_ADAPTER=usb:2357:0604` reads the
actual `PRODUCT=2357/604/...` from `/sys/class/bluetooth/hciN/device/uevent`
instead; this survives the name change.

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
