# switchbot-home

A single Rust backend that scans SwitchBot Meter Plus BLE advertisements
(collector) and serves a REST API (for a macOS menu-bar client first, a web
app later). No SwitchBot Hub or cloud account is involved.

## Documentation

- `docs/specs/` — all design/spec docs, starting with
  [`docs/specs/architecture.md`](docs/specs/architecture.md).
- [`docs/progression.md`](docs/progression.md) — running development log.

## Required workflow for agents

1. Read `docs/progression.md` in full at the start of every session, before
   taking any action.
2. After completing meaningful work, append a new dated entry to
   `docs/progression.md`. It is append-only — never rewrite or delete prior
   entries.
3. Any new spec or design doc goes under `docs/specs/` (kebab-case
   filenames), not scattered elsewhere.
4. Keep this file lean — pointers and rules only. Substantive design
   content belongs in `docs/specs/`.

## Build & Run

```
cargo build            # whole workspace
cargo test              # unit + storage + API tests (no hardware needed)
cargo run -p backend    # starts the BLE scanner + HTTP API on :3000
```

BLE scanning needs a Bluetooth adapter and, on macOS, a one-time permission
grant: System Settings → Privacy & Security → Bluetooth → add your terminal
app. Without it `btleplug` finds no devices. `RUST_LOG=debug cargo run -p
backend` logs every raw SwitchBot advertisement seen, useful for checking
the parser in `backend/src/ble/switchbot.rs` against a real device.

`macos-app/` and `web-app/` don't exist yet.
