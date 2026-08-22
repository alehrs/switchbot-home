# Reverse-engineering on-device history download (research spec)

Status: Research only — not started, not scheduled. Revisit and decide
later whether to invest time in it.
Last updated: 2026-08-21

## 1. Goal

The Meter Plus stores roughly 68 days of temperature/humidity history
on-device, and the official SwitchBot app can download that log over BLE
(visible in-app as scrollable history predating when the app was ever
connected to the device). Our backend currently only captures data going
forward, by passively listening to live BLE advertisements while it runs
(`backend/src/ble/scanner.rs`) — it has no way to backfill anything from
before it started, or to fill a gap after downtime (server off, Bluetooth
adapter issue, etc.).

This doc scopes what it would take to replicate the "download on-device
history" feature in our own backend, so that decision can be made later
with the real cost in view instead of guessing.

## 2. Why this isn't a small task: the protocol isn't documented

Checked before writing this doc, not assumed:

- OpenWonderLabs' official [BLE API docs](https://github.com/OpenWonderLabs/SwitchBotAPI-BLE/blob/latest/devicetypes/meter.md)
  (already used for the live-advertisement parser, `backend/src/ble/switchbot.rs`)
  document a connect-based command channel (service UUID
  `cba20d00-224d-11e6-9fb8-0002a5d5c51b`, RX characteristic
  `cba20002-...`, TX characteristic `cba20003-...`, request/response
  framing with a `0x57` magic byte + command header + payload) — but the
  only documented commands are **current-state** reads: `0x02` (basic
  info: battery, firmware version), `0x0F`/`0x14` (hardware version),
  `0x0F`/`0x30` (set °C/°F display), `0x0F`/`0x31` (read the *current*
  temperature/humidity, one point in time). Nothing in the official doc
  requests a historical log.
- Searched for prior community reverse-engineering of exactly this
  feature — [pySwitchbot](https://github.com/sblibs/pySwitchbot) (used by
  Home Assistant), ioBroker's switchbot-ble adapter, Theengs Decoder, and
  a detailed [Hacker News thread](https://news.ycombinator.com/item?id=31988259)
  from someone who bought Meter Plus units specifically to poke at the
  protocol — all of these cover live advertisement parsing (what we
  already do) and, for controllable devices (Bot, Curtain, Lock), the
  connect-based command channel for *actions*. None document or mention a
  history-download command for the Meter/Meter Plus.
- Conclusion: this is very likely a proprietary extension of the command
  protocol that only the official app knows, not something we can
  implement against existing documentation or borrow from an existing
  open-source library. It has to be reverse-engineered from scratch.

## 3. Proposed reverse-engineering approach

Capture the real traffic between the official app and a real Meter Plus
while the app performs a history sync/export, then decode it by hand.

### 3.1 Capture options

- **Android** (easiest): enable "Bluetooth HCI snoop log" in Developer
  Options, use the SwitchBot app to sync/export a device's history, then
  pull `btsnoop_hci.log` off the phone (`adb bugreport` or the file
  directly, depending on Android version) and open it in Wireshark, which
  has native Bluetooth GATT dissectors. No extra hardware needed.
- **iOS/Mac**: no built-in HCI snoop like Android. Two options:
  - Apple's **PacketLogger** (part of "Additional Tools for Xcode",
    downloaded separately from the Apple Developer site) can capture BLE
    HCI traffic for an accessory connected through a Mac's own Bluetooth
    radio — usable if the phone/app side of the sync isn't required to be
    an iPhone specifically, or if there's a way to drive the sync from a
    Mac.
  - A dedicated BLE sniffer (e.g., Nordic's **nRF Sniffer for Bluetooth
    LE** + an nRF52840 dongle, free tooling) captures over-the-air
    packets independent of which phone runs the app — more setup, but
    works regardless of platform and doesn't depend on Apple's tooling
    behaving a particular way.
- Given the SwitchBot app used earlier in this project was on iOS (per
  the screenshots shared during initial scoping), the sniffer-dongle route
  is probably the more reliable option unless an Android device is also
  available to make the HCI snoop route possible.

### 3.2 Decoding

- Filter the capture down to GATT writes/notifications on the known
  communication service/characteristics (§2) around the moment the app's
  UI shows it fetching history.
- The outer request/response envelope (`0x57` magic byte, command byte,
  status byte) is already documented — the unknown part is specifically
  the command byte(s) that mean "send history" and the payload format of
  the response (how a bulk log of many timestamped readings gets framed
  and likely paginated across multiple BLE writes/notifications, since a
  single characteristic write/notify is capped at ~20 bytes MTU per the
  documented format).
- Watch specifically for: how each log entry encodes its timestamp
  (an absolute clock read from the device, or an offset counted backward
  from "now" at download time — this matters a lot for correctly mapping
  entries to real UTC times) and whether the device's own clock is
  synced/queried as part of the exchange.

## 4. What implementing it would need, once decoded

Sketch only — not a real plan until §3 has actually produced a decoded
protocol to build against:

- The backend would need to add a **connect-based** BLE interaction
  alongside the existing passive-scan-only design — `btleplug` (already a
  dependency) supports connecting, discovering services/characteristics,
  writing, and subscribing to notifications, so this doesn't require a
  new crate, just new code (a `backend/src/ble/history.rs` sketch).
- **Not continuous**: unlike advertisement scanning, this needs an active
  GATT connection per device, which briefly takes the radio's attention
  and has some battery cost on a coin-cell-powered sensor — this should
  be an on-demand action (a manual trigger, e.g. a new endpoint like
  `POST /devices/{id}/sync-history`), not something run automatically on
  a schedule.
- **Deduplication**: backfilled entries must not create duplicate rows
  for periods the live scanner already covered — needs a dedup rule
  (e.g., skip an incoming historical entry if a reading already exists
  for that device within some small time tolerance).
- **Timestamp correctness**: depends entirely on what §3.2 finds about
  how the device encodes time in its log — this is the single biggest
  unknown affecting whether backfilled data would even be trustworthy.

## 5. Risks / open questions

- **Might be encrypted or require pairing/bonding.** The official docs
  discuss encrypted command exchange mainly in the context of actuators
  (Bot, Lock) that need authorization to perform actions; it's unknown
  whether the Meter Plus's command channel requires any handshake at all,
  or whether history-download specifically adds one even if other meter
  commands don't.
- **Might not be stable across firmware/hardware revisions.** A protocol
  decoded from one unit's firmware isn't guaranteed to match every Meter
  Plus in the wild.
- **Effort is genuinely unknown up front.** Could be a few hours if the
  command turns out to be a simple extension of the already-documented
  framing, or could stall indefinitely if it's encrypted or oddly
  paginated. This is why §6 recommends a timeboxed spike, not a
  committed implementation plan.
- Reverse-engineering an undocumented part of a vendor's app protocol for
  personal interoperability is the same kind of activity the existing
  open-source SwitchBot community tooling already does openly (pySwitchbot,
  Home Assistant, ioBroker) — noted here for completeness, not as a
  blocker.

## 6. Recommendation

Don't commit to full implementation up front. If/when this gets prioritized:
run a **timeboxed spike** (capture + attempt to decode, a few hours) before
deciding whether to build the actual backend feature — the value (backfilling
gaps predating continuous backend operation, or after downtime) is real but
secondary to the backend's own continuous live-scanning, which already covers
everything going forward. Not worth open-ended reverse-engineering effort
unless the spike shows the protocol is simple enough to be worth finishing.
