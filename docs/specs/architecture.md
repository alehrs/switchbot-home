# Architecture

Status: Backend and macOS app implemented (v1); web client not started
Last updated: 2026-08-21

## 1. Overview & Goals

`switchbot-home` monitors temperature and humidity around the house using
SwitchBot Meter Plus sensors, self-hosted end to end (no SwitchBot cloud
account or Hub involved). Goals:

- Near-real-time temperature/humidity per labeled room, viewable from a
  macOS menu bar.
- A historical record of readings the user actually owns, stored on their
  own homelab server.
- Low maintenance: one process to run, one small embedded database, no
  external service dependency.

## 2. Repository Structure

```
switchbot-home/
├── Cargo.toml     # workspace
├── backend/       # Rust: BLE collector + REST API + SQLite storage (implemented)
├── macos-app/     # Swift/SwiftUI: menu-bar client (v1) — implemented
├── web-app/       # future: browser client — not started
└── docs/
    ├── progression.md
    └── specs/
```

## 3. Hardware Context

- Sensors: SwitchBot Meter Plus and SwitchBot Outdoor Meter (Indoor/Outdoor
  Meter). They broadcast temperature, humidity, and battery level in BLE
  advertisements every few seconds — no pairing or bonding required to read
  them. Format reference: [OpenWonderLabs' official BLE API docs](https://github.com/OpenWonderLabs/SwitchBotAPI-BLE/blob/latest/devicetypes/meter.md).
  Implemented in `backend/src/ble/switchbot.rs`. **Gaps in the official
  doc**:
  - It only lists device type byte 0x54/0x74 ('T'/'t') for the base Meter.
    A real Meter Plus was confirmed to broadcast device type 0x69 ('i'),
    matching [pySwitchbot's](https://github.com/sblibs/pySwitchbot) device
    type table (which maps both 'T'/'t' and 'I'/'i' to the same parser).
    The Outdoor Meter uses 'w'/'W'. All are in `METER_DEVICE_TYPES`.
  - The base Meter and Meter Plus carry the 3-byte temperature/humidity
    payload in their **service data** (bytes 3..6). The Outdoor Meter's
    service data stops after the battery byte and moves that same payload
    into its **manufacturer data** (bytes 8..11, after a 6-byte MAC and a
    2-byte header). `btleplug` delivers service data and manufacturer data
    as separate advertisement events, so the collector caches the latest
    SwitchBot (company ID 0x0969) manufacturer data per device and pairs
    the two up. Matches pySwitchbot's `process_wosensorth`.
- There is **no SwitchBot Hub** in this setup, and the **SwitchBot cloud
  API is entirely out of scope**: it only exposes current status and
  webhooks, not the on-device historical log, and it requires a Hub to
  bridge BLE devices to the cloud in the first place.
- The homelab server is physically central to all sensors in the house, so
  a **single BLE collector process** is sufficient — no per-room relay
  hardware is needed.
- **Platform caveat learned during implementation**: on macOS, CoreBluetooth
  masks a peripheral's real BLE MAC address behind an OS/app-local UUID for
  privacy. This UUID is stable across scans on the same Mac, so it still
  works as a device identifier during Mac-based development — but it will
  **not** match the real MAC addresses that BlueZ exposes on the eventual
  Linux homelab deployment. Devices will need to be re-labeled once the
  backend moves from a Mac (dev) to the homelab (Linux) host. For this
  reason the schema and API use the neutral term `device_id`, not
  `mac_address`, as the primary key.
- **`devices.mac_address` (added later, nullable)**: the collector also
  best-effort records the real hardware MAC as a separate column, via
  `btleplug`'s `Peripheral::properties().address`. On the BlueZ/Linux
  homelab this is the genuine MAC. `btleplug`'s CoreBluetooth backend
  always reports `BDAddr::default()` (all-zero) instead of a real address
  — Apple never exposes it to apps — so the collector detects that
  placeholder and stores `NULL` rather than a fake, identical-across-
  devices value. **Exception**: SwitchBot meters embed their real MAC in
  the first 6 bytes of their manufacturer data, which CoreBluetooth does
  *not* mask, so the collector falls back to that (`resolve_mac_address`
  in `backend/src/ble/scanner.rs`) and can record a genuine MAC even on
  macOS. This column is informational only; `device_id` remains the
  identity/primary key for the reason above.

## 4. System Architecture

### 4.1 Diagram

```
 [Meter Plus]  [Meter Plus]  [Meter Plus]
      |             |             |
      \_____________|_____________/
              BLE advertisements
                    |
                    v
        +--------------------------+
        |   backend (homelab)      |
        |  - BLE collector         |
        |  - REST API              |
        |  - SQLite storage        |
        +--------------------------+
              ^              ^
              | HTTP         | HTTP
              |              |
      [macOS menu-bar app]  [web app (future)]
```

### 4.2 Single unified backend process

The backend is **one Rust process** that is both the BLE collector and the
REST API server — not two separate services. This is a settled decision:
running collector and API together avoids inter-process coordination for
what is a single-machine, single-tenant homelab deployment, and keeps
operations to "run one binary."

### 4.3 Backend responsibilities

- Continuously scan for BLE advertisements and parse SwitchBot Meter Plus
  payloads (temperature, humidity, battery).
- Persist readings as a time series.
- Maintain a device registry, auto-creating an entry the first time a
  device is seen over BLE, with a user-assigned label, room (e.g. "cucina",
  "studio"), and a blacklist flag to make the collector ignore a device.
- Expose a REST API for clients to list/relabel devices and query latest
  and historical readings.
- Apply a retention/downsampling strategy so storage doesn't grow
  unbounded given readings arrive every few seconds (policy: see Open
  Questions).

### 4.4 Data model (implemented)

```
devices
  device_id      TEXT PRIMARY KEY   -- BLE peripheral identifier (see platform caveat above)
  label          TEXT               -- NULL until the user names it
  room           TEXT
  blacklisted    INTEGER NOT NULL DEFAULT 0
  first_seen_at  TEXT NOT NULL
  last_seen_at   TEXT NOT NULL

readings
  id            INTEGER PRIMARY KEY AUTOINCREMENT
  device_id     TEXT NOT NULL REFERENCES devices(device_id)
  temperature   REAL NOT NULL
  humidity      REAL NOT NULL
  battery       INTEGER
  recorded_at   TEXT NOT NULL
```

See `backend/migrations/0001_init.sql`.

### 4.5 REST API surface (implemented)

```
GET  /devices                          list all devices (including blacklisted)
PUT  /devices/{device_id}               partial update: label, room, blacklisted
GET  /devices/{device_id}/latest        latest reading for one device (404 if none)
GET  /devices/{device_id}/readings      historical readings (?from=&to=, default: last 24h)
GET  /readings/latest                   latest reading per device, excluding blacklisted ones
```

Device registration is automatic: any device seen over BLE is upserted into
`devices` with `label = NULL`. There is no separate `POST /devices` — a
device only exists once the collector has actually seen it broadcast.

### 4.6 Throttling and retention (implemented)

BLE advertisements arrive every few seconds per sensor, which would
otherwise mean a database write on every one of them. Two independent,
optional env vars address this — no downsampling/aggregation, just fewer
raw rows in the first place plus a cutoff for old ones:

- `READING_INTERVAL_SECONDS`: the BLE scanner (`backend/src/ble/scanner.rs`)
  keeps an in-memory `device_id → last stored timestamp` map and drops an
  advertisement if less than the configured interval has passed since the
  last one it actually stored *for that device*. Per-device, not global —
  one noisy sensor doesn't throttle the others. Not persisted across
  restarts (accepted: the cost is at most one extra reading right after a
  restart).
- `RETENTION_DAYS`: a background task (`backend/src/retention.rs`), spawned
  only when this is set, runs `DELETE FROM readings WHERE recorded_at <
  now - N days` once at startup and then hourly for as long as the process
  runs.

Both default to "off" (no throttling, no deletion) when unset. See
`README.md` for the exact env var names and defaults.

## 5. Client Applications

### 5.1 macOS menu-bar app (v1 — build first)

Implemented in Swift/SwiftUI — see
[`macos-app.md`](macos-app.md) for the full design. Supersedes the
Rust/`tray-icon`+`muda` plan originally sketched below in §6: the actual
requirements (room-grouped sections, colored trend badges, a settings
form) need real UI composition that a flat native `NSMenu` can't express
well, and Rust's cross-platform/reuse advantages don't apply to a
single-user, Mac-only utility.

- Menu bar icon only (no numeric value — see `macos-app.md` §11 for why);
  a `.window`-style `MenuBarExtra` popover lists all non-blacklisted
  devices grouped by room, each with current temperature/humidity, a 1h
  average, and a temperature trend indicator.
- Talks **only** to the backend's REST API — no direct BLE access, no
  SwitchBot-specific logic on the client.
- Configurable API base URL (Settings window), polled via REST (no
  push/streaming exists on the backend).

### 5.2 Web app (future — out of scope for now)

Not started. Mentioned here only so the backend API is designed to be
client-agnostic: any future web frontend consumes the same REST API as the
macOS app, with no macOS-specific behavior baked into the backend.

## 6. Tech Stack

| Concern              | Crate/Tool        | Rationale                                                             |
|-----------------------|--------------------|------------------------------------------------------------------------|
| BLE central            | `btleplug`         | Cross-platform (macOS/Linux/Windows) async BLE scanning               |
| SwitchBot ad parsing    | hand-rolled (`backend/src/ble/switchbot.rs`) | The only `switchbot` crate on crates.io (0.1.2) is 0% documented and geared toward controllable devices (Bot/Curtain), not meters; the official byte format is small and well-documented enough to parse directly |
| HTTP API               | `axum`             | Async, pairs naturally with `tokio` and `btleplug`                    |
| Storage                | SQLite (`sqlx`)    | Zero-ops embedded DB, fits a single-process homelab deployment. **Decision: SQLite-only, no DB abstraction layer** — Postgres migration is explicitly not a design goal; keeping the storage code simple and directly coupled to SQLite was chosen over the extra indirection of a swappable storage trait |
| macOS menu bar         | Swift/SwiftUI (`MenuBarExtra`, `.window` style) | **Superseded the original `tray-icon`+`muda` Rust plan** — the actual UI needs (grouped sections, colored trend badges, a settings form) require real UI composition that a flat `NSMenu` can't express; see `macos-app.md` §2 |

## 7. Open Questions

- No aggregation/downsampling (e.g. hourly rollups) — `RETENTION_DAYS`
  only deletes old raw rows, it doesn't summarize them first. Also,
  `readings_in_range` has no cap on how wide a range can be requested (it
  only defaults to 24h when `from`/`to` are omitted).
- REST API authentication: none (LAN-trust only) vs. a simple token.
- ~~Deployment mechanism: systemd unit vs. container.~~ Resolved: both,
  chosen at install time — see [`installation-and-deployment.md`](installation-and-deployment.md).
  Designed, not yet implemented; the container/k3s path in particular has
  an unverified assumption (BLE access from inside a container) flagged
  in that doc's §7/§10.
- Web app scope and timeline.
- Backfilling on-device history (the Meter Plus stores ~68 days locally;
  the official app can download it, ours currently can't) — research-only
  so far, see [`ble-history-reverse-engineering.md`](ble-history-reverse-engineering.md).
  The protocol for this isn't in SwitchBot's official docs and hasn't
  been reverse-engineered by the community as far as could be found, so
  this would need original reverse-engineering work before it's buildable.
