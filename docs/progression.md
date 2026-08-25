# Progression Log

This is an append-only development log for `switchbot-home`.

- **Every agent session must read this file in full before doing anything
  else.**
- After completing meaningful work, append a new entry at the **bottom**
  with today's date. Never rewrite, reorder, or delete previous entries.

Entry format:

```
## YYYY-MM-DD — Short title
- What was done.
- Decisions made (and why, if not obvious).
- Next steps.
```

---

## 2026-08-21 — Repository scaffolded (docs only)
- Initialized git repo at `switchbot-home` (renamed from the placeholder
  `home-temp`).
- Added `docs/specs/architecture.md` capturing the single-backend
  (BLE collector + REST API) architecture decision.
- Added `CLAUDE.md` with agent workflow rules and `README.md`.
- No code written yet.
- Next: begin the Rust backend implementation per
  `docs/specs/architecture.md`.

## 2026-08-21 — Backend implemented (BLE collector + REST API)
- Installed the Rust toolchain (rustup, stable 1.98.0) — the machine had
  none.
- Created a Cargo workspace with a single `backend` crate.
- Researched the official SwitchBot Meter BLE service-data format
  (OpenWonderLabs/SwitchBotAPI-BLE) instead of depending on the
  `switchbot` crate (crates.io v0.1.2, 0% documented, geared toward
  controllable devices, not meters). Hand-rolled the parser in
  `backend/src/ble/switchbot.rs`, unit-tested against known byte layouts.
- Decision: **no DB-abstraction layer**. Original ask was to keep a future
  Postgres migration "easy", but after weighing the `async-trait` +
  `Storage` trait approach, the user chose to drop it and go directly
  against SQLite (`backend/src/storage.rs`, no trait/adapter split) for
  simplicity. Postgres migration is no longer a design goal — see
  `docs/specs/architecture.md` §6.
- Decision: device registration is auto-discovery (any device seen over
  BLE is upserted with `label = NULL`), with a per-device `blacklisted`
  flag (settable via `PUT /devices/{device_id}`) to make the collector
  ignore unwanted devices — no manual allowlist step.
- Renamed the identity column/field from `mac_address` to `device_id`
  after discovering that on macOS, CoreBluetooth masks the real BLE MAC
  behind a stable-but-local UUID (privacy feature) — the column will hold
  different kinds of values on the Mac (dev) vs. the eventual Linux
  homelab host (prod). See the "Platform caveat" note in
  `docs/specs/architecture.md` §3.
- Enabled SQLite WAL journal mode (non-memory databases only) so the BLE
  writer and API reads don't lock each other out.
- Implemented: `backend/src/domain/` (Device, Reading, NewReading),
  `backend/src/storage.rs` (SqliteStorage, sqlx), `backend/src/ble/`
  (scanner using `btleplug` + the hand-rolled parser), `backend/src/api/`
  (axum router: devices + readings endpoints), `backend/src/main.rs`
  (wiring: BLE scan task + HTTP server share one `Arc<SqliteStorage>`).
- Tests: 16 passing (`cargo test`) — parser unit tests with known byte
  sequences, storage tests against a real in-memory SQLite database, API
  handler tests via `tower::ServiceExt::oneshot`. `cargo clippy
  --all-targets` clean, `cargo fmt` applied.
- Not yet done / explicitly deferred: retention/downsampling, API
  authentication, homelab deployment packaging (systemd/container), and
  actual verification against a real Meter Plus device over BLE (the user
  will test this on their Mac — Bluetooth permission for the terminal app
  must be granted manually in System Settings → Privacy & Security →
  Bluetooth first).
- Next: user to run `cargo run` on the Mac with a Meter Plus powered on
  nearby, granting Bluetooth permission to the terminal if prompted, and
  confirm real readings show up via `GET /readings/latest`. If the parser
  doesn't match the real advertisement bytes, add `RUST_LOG=debug` to see
  the raw `switchbot advertisement received` log lines and adjust
  `backend/src/ble/switchbot.rs` accordingly. After that: macOS menu-bar
  client.

## 2026-08-21 — Code review pass on the backend (fixes applied)
- Ran `coding-review` against the freshly-implemented backend. Found and
  fixed:
  1. `SqliteStorage::set_device_label` did a SELECT-then-UPDATE outside a
     transaction — two concurrent `PUT /devices/{id}` calls on the same
     device could lose one update. Fixed by wrapping it in a
     `BEGIN IMMEDIATE` transaction (SQLite deferred transactions don't
     take the write lock until the first write, so a plain `begin()`
     would not have closed the race).
  2. `label`/`room` could never be cleared back to null once set (`None`
     always meant "leave unchanged", and JSON `null` is indistinguishable
     from an omitted field). Fixed with a tri-state convention: omit the
     field to leave unchanged, send `""` to clear, send a non-empty
     string to set. Documented on `UpdateDeviceRequest` in
     `backend/src/api/devices.rs`.
  3. Considered narrowing the `sqlx` feature flags from `"macros"` to
     `"derive"` (only `#[derive(FromRow)]` looked necessary) — reverted
     after `cargo build` failed: `sqlx::migrate!` also requires
     `"macros"`. Kept as-is; verified by compiling, not assumed.
- Added regression tests for both fixes: `clearing_a_label_sets_it_to_null_without_touching_room`
  (storage) and `sending_an_empty_label_clears_it` (API). 18 tests passing,
  clippy clean.
- No other findings survived review (reviewed error handling, SQL
  injection surface — all queries are parameterized, migration schema,
  and the BLE scan loop's error handling).

## 2026-08-21 — README rewrite, .http file, real-hardware bug fix
- Rewrote `README.md`: project description, repo structure, build/test/run
  commands, env vars, BLE permission note, REST API summary. Removed
  "homelab"/"self-hosted" framing per user request.
- Added `backend/api.http` (VS Code REST Client / JetBrains HTTP Client
  format) with ready-to-run requests for every endpoint, including the
  label-clear and blacklist flows.
- **Bug found via real hardware**: the user ran `cargo run -p backend`
  against an actual Meter Plus and saw every advertisement rejected with
  "advertisement did not match a known meter format"
  (`raw=[105, 0, 82, 8, 151, 65]`, i.e. device type byte `0x69`/'i').
  OpenWonderLabs' official meter.md only documents 'T'/'t' (0x54/0x74) for
  the base Meter — it does not mention Meter Plus using a different type
  byte at all. Cross-checked against
  [pySwitchbot](https://github.com/sblibs/pySwitchbot) (used by Home
  Assistant), which maps 'I'/'i' (0x49/0x69) to the same meter parser as
  'T'/'t'. Decoded the user's raw bytes by hand against our existing
  layout (battery=82, temp=23.8°C, humidity=65%) — matched the
  screenshot's ballpark readings, confirming the rest of the byte layout
  was already correct and only the device-type allowlist was missing.
  Fixed in `backend/src/ble/switchbot.rs`
  (`METER_DEVICE_TYPES = [0x54, 0x74, 0x49, 0x69]`), added a regression
  test using the real captured bytes. 19 tests passing, clippy clean.
  Documented the gap in `docs/specs/architecture.md` §3.
- Next: user to restart `cargo run -p backend` (the running process was
  built before this fix) and confirm readings now show up via `GET
  /readings/latest`.

## 2026-08-21 — Reading throttling and retention (READING_INTERVAL_SECONDS, RETENTION_DAYS)
- Motivation: with no throttling, every Meter Plus advertisement (every
  few seconds) was being stored as a separate row — "spam" per the user.
- Added `Config.reading_interval_secs` / `Config.retention_days`
  (`backend/src/config.rs`), both `Option<u64>` read from
  `READING_INTERVAL_SECONDS` / `RETENTION_DAYS`. Unset = current behavior
  (no throttling, no deletion). Set-but-invalid (non-numeric) panics at
  startup with a clear message rather than silently falling back to
  "unset" — a silent fallback on a typo would be a confusing footgun.
- Throttling: `backend/src/ble/scanner.rs` keeps an in-memory
  `device_id -> last stored timestamp` `HashMap` and skips storing (but
  still logs) an advertisement if less than the configured interval has
  passed since the last one actually stored *for that device* — per
  device, not global. Deliberately not persisted across restarts (a DB
  round-trip per advertisement just to check the throttle would defeat
  much of the point).
- Retention: new `backend/src/retention.rs` — a background task, spawned
  only when `RETENTION_DAYS` is set, that deletes readings older than the
  cutoff once at startup and then hourly (`tokio::time::interval`, whose
  first tick fires immediately). New `SqliteStorage::delete_readings_before`
  in `backend/src/storage.rs`.
- Documented both env vars in `README.md` and resolved the corresponding
  "Open Question" in `docs/specs/architecture.md` §4.6 (now describes the
  implemented behavior instead of an open policy question).
- Tests: 25 passing — 5 new pure-function tests for the throttle decision
  (`ble::scanner::tests`), 1 new storage test for
  `delete_readings_before`. Clippy clean, fmt applied.
- Not done: no aggregation/downsampling before deletion (raw rows are
  just dropped, not summarized first) — still an open question.

## 2026-08-21 — macOS app design (docs/specs/macos-app.md), no code yet
- User asked to design (not implement) the macOS menu-bar client. Used a
  Plan-agent pass plus my own review to produce the design; wrote it up in
  the new `docs/specs/macos-app.md`.
- **Pivot from the original plan**: `architecture.md` had planned a Rust
  client on `tray-icon`+`muda`(+`egui`). Decided on Swift/SwiftUI instead
  (`MenuBarExtra`, `.window` style) — the actual requirements (room-
  grouped sections, colored trend badges, a settings form) need real UI
  composition that a flat `NSMenu` can't express without dropping into
  raw AppKit from Rust, at which point Rust's cross-platform/reuse
  advantages don't apply to a single-user Mac-only utility anyway.
  Updated `architecture.md` §2/§5.1/§6 to match.
- Key design decisions captured in `macos-app.md`: "Unknown device #N"
  numbering uses a permanent rank from the full device roster sorted by
  `first_seen_at` (proven stable with no local persistence needed, since
  that timestamp is set once and never rewritten); room grouping with
  "Ungrouped" always sorted last; a temperature trend indicator using an
  edge-averaged two-point delta (mean of last 5min vs. mean of the first
  5min of the trailing hour) with a 0.2°C deadband, deliberately using
  warm/cool tints instead of Trade Republic's literal green/red (up isn't
  "good" for temperature); humidity gets a plain average, no trend arrow;
  configurable API base URL via a Settings window backed by `UserDefaults`,
  with the two macOS-specific gotchas for LAN HTTP calls
  (`NSAllowsLocalNetworking` + `NSLocalNetworkUsageDescription` in
  `Info.plist`, App Sandbox kept on with the network-client entitlement).
- Explicitly deferred: launch-at-login, notifications, in-app device
  editing (label/room/blacklist stay a manual `backend/api.http` workflow
  for now).
- Next: implement the design — create the Xcode project at
  `macos-app/SwitchBotHome.xcodeproj` per `macos-app.md` §4, starting with
  the pure-logic pieces (`DeviceNumbering`, `GroupingRules`,
  `TrendCalculator`) and their unit tests before the networking/UI layers.

## 2026-08-21 — macOS app implemented (SwitchBotHome v1)
- Amendment to the design first: trend indicators now apply to **both**
  temperature and humidity (user caught this after approving the spec —
  originally only temperature had one). Humidity uses its own wider
  deadband (2% vs. 0.2°C) since it naturally swings more; both share the
  same `TrendCalculator` implementation via a `KeyPath<Reading, Double>`
  parameter instead of duplicating the algorithm. Updated
  `docs/specs/macos-app.md` §9 accordingly.
- Installed `xcodegen` (Homebrew) to generate `SwitchBotHome.xcodeproj`
  declaratively from `macos-app/Project.yml` — no Xcode GUI was used.
  Swift 5 language mode (not Swift 6 strict concurrency) chosen
  deliberately: avoids fighting the strict concurrency checker's
  Sendable-conformance demands for a personal utility app; `AppStore` and
  `PollingService` are still explicitly `@MainActor` where it matters for
  real correctness (see review fixes below).
- Implemented every file from `macos-app.md` §4's layout: `Device`/
  `Reading`/`DeviceSnapshot` models, `BackendCoding` (handles the
  backend's fractional-second RFC3339 timestamps, which Foundation's
  built-in `.iso8601` strategy can't parse), `APIClient` (built against a
  `BaseURLProviding` protocol rather than the concrete `AppSettings`
  class, so `SettingsView`'s "Test Connection" can probe an unsaved draft
  URL without mutating real settings), `PollingService` (fast 30s /
  slow 3min cycles), `AppStore`, all six views, `AppSettings`/
  `SettingsView`, `Info.plist`/entitlements with the ATS/local-network
  keys from the design.
- Validation: `xcodebuild build`/`test` clean (0 warnings), then ran a
  live smoke test — started the real backend (already had genuine data
  from earlier manual testing: a device labeled "Studio"), launched the
  built `.app`, and confirmed via `lsof` two live TCP connections to
  `localhost:3000` — i.e. the fast+slow poll cycles actually run and the
  App Sandbox + `network.client` entitlement + ATS config work end-to-end
  for a real connection. Could **not** visually confirm the popover/menu
  bar rendering — Screen Recording permission isn't granted to this
  session and can't be self-granted; that part still needs the user's own
  eyes.
- `coding-review` pass found and fixed 4 issues before calling this done:
  1. **Real data race**: `PollingService.lastFullRefreshAt` was a plain
     var written from a background `Task` and read from the main thread
     (`.onAppear`, wake notification) with no synchronization. Fixed by
     making `PollingService` itself `@MainActor`.
  2. **Real bug**: `runSlowCycle`'s per-device history fan-out used
     `withThrowingTaskGroup` — one device's fetch failing cancelled every
     sibling fetch and marked the *entire app* offline. Changed to
     non-throwing `withTaskGroup` with per-task error handling: a failed
     device just gets no history this cycle (falls back to "insufficient
     data"), and offline status now reflects only the initial
     `GET /devices` call.
  3. **Test gap**: `Trend`'s hand-written `Codable` (needed for
     `LocalCache`'s on-disk persistence) had zero coverage. Added
     `TrendCodableTests.swift`.
  4. **Minor correctness**: a slow refresh could momentarily regress a
     device's displayed "latest" value behind what a more-recent fast
     cycle had already shown (the slow cycle's own history fetch reflects
     a moment slightly before it finishes). `AppStore.applyFullRefresh`
     now keeps whichever of the two is actually newer. Added
     `AppStoreTests.swift`.
  - Noted but not changed: `AppStore()` unconditionally loads/saves the
    real `~/Library/Application Support/SwitchBotHome/last-snapshot.json`
    on every init, including from unit tests — not fully hermetic, but
    doesn't cause incorrect test results today (each test's first
    `applyFullRefresh` call fully overwrites the loaded state before any
    assertion). A future cleanup could inject the cache location.
- Final state: 26 unit tests passing, 0 build warnings.
- Fixed `.gitignore`: `*.sqlite` didn't match the `-shm`/`-wal` WAL
  companion files the backend's WAL mode always creates alongside the
  real `.sqlite` file — changed to `*.sqlite*`. (Did not touch the actual
  database files at the repo root — they're the user's real dev data.)
- Not done / left for later: visual/interactive verification of the
  actual UI (needs the user, per above), launch-at-login, notifications,
  in-app device editing (still `backend/api.http`), and opening the
  project in Xcode has not been tried (only command-line `xcodebuild`).

## 2026-08-21 — README macOS instructions; popover sizing bug fix
- Added a "macOS app: build, run, test" section to `README.md`: XcodeGen
  regeneration, opening/running in Xcode (no Apple Developer account
  needed — Xcode signs "to run locally"), where to find the menu-bar icon
  (no Dock icon), how to change the API URL from Settings, and the
  command-line `xcodebuild build`/`test` invocations.
- User opened the app in Xcode themselves (first real interactive use)
  and reported the popover renders too small — can't see the first
  device without scrolling. Root cause:
  `PopoverContentView.swift`'s `ScrollView` only had `.frame(maxHeight:
  420)` (a ceiling, no floor); a `MenuBarExtra(.window)`-style popover
  sizes itself from the content's ideal size, and a `ScrollView` with no
  minimum has an ill-defined ideal height, so it rendered nearly
  collapsed. Fixed with `.frame(minHeight: 280, idealHeight: 280,
  maxHeight: 460)` — the explicit `idealHeight` removes the ambiguity at
  the root rather than just bounding it with min/max.
- `coding-review` sanity-checked the 280pt choice against the actual
  `DeviceRowView`/`RoomSectionView` layout (row ≈70pt, section header
  ≈26pt): worst case (3 devices in 3 separate rooms) ≈288pt, comfortably
  covered since minHeight is a floor, not a ceiling; common case (3
  devices, 1 room) ≈238pt, leaving some intentional blank space — an
  accepted tradeoff of a fixed minHeight, not a defect. No other similar
  ambiguous-height risk found nearby (`OfflineBannerView`, the empty-state
  branch, and the row/section views all have well-defined intrinsic
  heights).
- Still unverified: the actual rendered popover size/layout, since this
  session still has no Screen Recording permission to check visually —
  the user needs to confirm in their own Xcode run.
- Build: 0 warnings. (View-only change; the existing 26 unit tests don't
  cover this file — it's UI layout, not pure logic, per the project's
  established test-coverage split — so none were added for this fix.)

## 2026-08-21 — Metric icons + day-chart drill-down (DeviceDetailView)
- User feedback: (1) a bare `"67%"` isn't obviously humidity the way
  `"23°C"` is obviously temperature — fixed by adding a `thermometer`/
  `drop.fill` icon next to each value in `DeviceRowView.swift`, plus a
  trailing chevron hinting the row is tappable. (2) Wanted simple daily
  charts per device with day-to-day navigation, explicitly "from midnight
  to now, not a rolling last-24h."
- New `DeviceDetailView.swift`: two Swift Charts (temperature, humidity)
  for a selected calendar day, prev/next day buttons ("next" disabled on
  today). Reuses the existing `GET /devices/{id}/readings?from=&to=`
  endpoint — no backend change.
- Navigation: `NavigationStack` pushed inside the *same* `MenuBarExtra`
  popover (`PopoverContentView.swift`/`RoomSectionView.swift`), not a
  separate `WindowGroup` — decided against a separate window because
  `LSUIElement` (menu-bar-only, no Dock icon) apps have unreliable
  activation/focus behavior for auxiliary windows. The popover just
  resizes to whatever screen is currently pushed.
- New pure `Services/DayRange.swift` (calendar-based day boundaries —
  correct across DST transitions, unlike raw 86,400s arithmetic) with
  `DayRangeTests.swift`.
- `coding-review` pass found and fixed 3 issues:
  1. **Real, currently-evidenced risk**: manual curl testing against the
     live backend returned 562 readings for a *partial* day with default
     (unthrottled) settings — a full day could be tens of thousands.
     Charting all of them unbounded was a real performance/memory risk,
     not speculative. Added `Services/ReadingsDownsampler.swift`
     (time-bucketed averaging, capped at 300 points) with
     `ReadingsDownsamplerTests.swift`.
  2. **Real race condition**: rapid day-to-day navigation could show the
     wrong day's data — `.task(id: dayRange)` cancellation is cooperative
     and doesn't guarantee an old request's already-resumed continuation
     can't write its (stale) result after a newer one has started. Fixed
     by capturing the requested `DayRange` in `load()` and only committing
     `readings`/`loadError`/`isLoading` if it still matches the current
     selection — closes the race regardless of cancellation timing.
  3. **Preventive hardening, not currently reachable**: the "Back" button's
     `path.removeLast()` would crash on an empty `NavigationPath`. No
     current call site can trigger it, but guarded it (`if !path.isEmpty`)
     anyway given the cost of being wrong is a full crash.
- Verified the day-range query against the real backend via curl (correct
  time-ordered results for today's boundary). Did **not** do a fresh
  interactive app launch this time — a backend process was already
  running, possibly the user's own active testing session, and launching
  a second app instance against it risked interfering. So the actual
  SwiftUI navigation transition (does tapping a row visually push the
  detail screen, does the popover resize smoothly, do the charts render
  sensibly) is still unverified by direct observation.
- Final state: 36 unit tests passing (10 new), 0 build warnings.

## 2026-08-21 — Chart Y-axis unit labels
- User feedback after the day-chart feature: the humidity chart's y-axis
  numbers (e.g. "65") had no unit — the "Humidity (%)" caption above the
  chart wasn't enough to remove ambiguity from the actual plotted values.
  Added a custom `.chartYAxis` in `DeviceDetailView.metricChart` so every
  tick shows the unit directly (e.g. "65%", "23°C"), reusing the same
  `suffix` string already passed in for the chart title. Rounds to the
  nearest whole number for axis ticks (vs. 1-decimal precision for the
  actual current-value/average text elsewhere) since gridline labels
  don't need that precision.
- No new pure logic (this is Chart axis configuration, not testable
  business logic) — build succeeds, all 36 existing tests still pass
  unaffected.

## 2026-08-21 — Real bug: humidity "%" silently dropped + console error spam
- User report: still no "%" showing on the main row values, plus a spam
  of runtime errors: `String(format:locale:arguments:): ... Format
  '%.0f%' does not match expected '%lf'`.
- **Root cause**: `String(format: "%.1f\(suffix)", value)` — when `suffix`
  is `"%"`, the resulting format string is literally `"%.1f%"`. In a
  printf-style format string, `%` is the escape character introducing a
  specifier, so that trailing `%` is parsed as the *start* of a second,
  incomplete specifier rather than a literal character — Foundation logs
  the mismatch and silently drops it from the output. This was present
  from the very first `DeviceRowView` implementation (the value and
  average text), not just introduced by the day-chart axis labels added
  earlier this session — it was never actually exercised/observed until
  now because nobody had looked closely at the exact rendered string
  before. The axis-label version added today hit the same bug and
  explains the "spam" (it's called once per Y-axis tick, so it fires far
  more often than the two per-row occurrences).
- **Fix**: found all 3 occurrences
  (`DeviceRowView.swift` value + average text, `DeviceDetailView.swift`
  Y-axis label) via `grep -rn 'String(format:' SwitchBotHome/`, and rather
  than patching each inline, extracted a shared `Services/Formatting.swift`
  (`Formatting.number(_:decimals:suffix:)`) that formats the number alone
  and appends the suffix via plain string concatenation — never feeding
  the suffix through the format parser at all, closing the entire bug
  class rather than just the 3 known instances. Also routed
  `TrendIndicatorView.swift` through it for consistency (it wasn't
  actually buggy — its suffix was already appended outside the format
  string — but having one formatting path everywhere means this bug can't
  quietly reappear in a 4th location later).
- Added `FormattingTests.swift` with a test asserting a `"%"` suffix
  survives in the output — a regression test that fails loudly if this
  exact pattern is ever reintroduced anywhere that calls the shared
  helper.
- Final state: 41 unit tests passing (5 new), 0 build warnings. Confirmed
  via the new tests (not visually, still no Screen Recording permission)
  that `Formatting.number(67.0, suffix: "%")` now produces exactly
  `"67.0%"`.

## 2026-08-21 — Coordinated chart tooltip + battery indicator
- User asks: (1) a synchronized tooltip/crosshair across both day charts
  when hovering either one, (2) show battery % with a dedicated icon in
  the real-time row values (not just in the day-chart detail).
- Chart sync: added `DeviceDetailView`'s `@State private var
  selectedTime: Date?`, bound to `.chartXSelection(value:)` on **both**
  `metricChart` calls. Since both charts write/read the same shared
  state, moving the mouse over either one updates it for both — no manual
  gesture/overlay code needed, this is Swift Charts' built-in linked-
  selection mechanism (available since the macOS 14 deployment target).
  Added a `RuleMark` (vertical guide) + `PointMark` + value annotation at
  the selected time on each chart. `selectedReading` is computed once at
  the `DeviceDetailView` level (nearest reading to `selectedTime`) since
  both charts plot the same underlying `readings` array — no need to
  search twice.
- Battery: `DeviceRowView`'s header row now shows a battery percentage
  next to a matching SF Symbol (`battery.0`/`.25`/`.50`/`.75`/`.100`,
  picked by threshold), tinted red under 20%. Routed through the already-
  fixed `Formatting.number` helper (not a fresh `String(format:)` call)
  so this addition can't reintroduce the "%" bug from the previous entry.
- Self-review caught one thing before it shipped: adding a third
  always-visible badge (battery) to the header row — on top of the
  already-conditional stale-time text and the tappable-hint chevron —
  made it more likely a long device label would overflow the popover's
  fixed 340pt width and wrap awkwardly. Added `.lineLimit(1)` +
  `.truncationMode(.tail)` to the label so it truncates with an ellipsis
  instead.
- Build: 0 warnings. Tests: 41 passing, unchanged (no new pure logic —
  chart interaction and icon selection are view-layer code, consistent
  with the project's existing test-coverage split). Still unverified
  visually (no Screen Recording permission in this session) — the user
  needs to confirm hovering actually links the two charts and that the
  battery badge doesn't crowd the row in practice.

## 2026-08-21 — Fixed Y-axis jitter on chart hover
- User report, right after trying the coordinated tooltip: the humidity
  chart's Y-axis scale visibly jumps while hovering. Root cause: neither
  chart had an explicit `.chartYScale(domain:)`, so Swift Charts computed
  the domain automatically from whatever marks were currently rendered —
  including the selection `PointMark`/annotation `chartXSelection` adds
  on hover. Recomputing "nice" bounds to make room for that annotation
  shifts the domain, and the shift is far more visible on humidity, whose
  real range is often narrow (e.g. 55-70%), than on temperature.
- Fix: added `Services/ChartDomain.swift`, a pure function that derives a
  padded `ClosedRange<Double>` from a data array once (10% padding, floor
  of 1.0 unit, so a flat/single-value series still gets visible headroom
  instead of a zero-width domain). Humidity's chart now uses a **fixed
  `0...100`** domain per the user's explicit request (it's a percentage
  by definition, not something to derive from a single day's data).
  Temperature's chart uses `ChartDomain.range(for: readings.map(\.temperature))`
  — still fully data-driven (correct for sub-zero days and days over
  40°C, as asked), but computed once from the day's readings rather than
  left automatic, so it no longer moves when a selection is added or
  removed.
- Added `ChartDomainTests.swift`: negative minimum, values above 40,
  a flat single-value series, and the empty-input fallback.
- Final state: 46 unit tests passing (5 new), 0 build warnings.

## 2026-08-21 — Removed the redundant custom "Back" button
- User report: opening the day-chart screen showed two back buttons — the
  standard system one `NavigationStack` already provides automatically at
  the top, and a second custom "Back"-labeled one added earlier in
  `DeviceDetailView` (added defensively, before it was confirmed the
  system one would actually render inside a borderless `.window`-style
  popover — it does).
- Removed `DeviceDetailView`'s `onBack` parameter and its `backButton`
  view entirely; `PopoverContentView`'s `navigationDestination` no longer
  passes an `onBack` closure. Since nothing outside `PopoverContentView`
  needs to observe or manipulate navigation state anymore, also dropped
  the explicit `@State private var path = NavigationPath()` /
  `NavigationStack(path: $path)` in favor of a plain `NavigationStack { }`
  that manages its own push/pop internally — removes now-pointless
  complexity along with the button, not just the button itself.
- Verified no other file references the removed `onBack`/`backButton`/
  `path` names. Build: 0 warnings. Tests: 46 passing, unaffected (this
  was pure view code with no logic to test).

## 2026-08-21 — Research spec: on-device history reverse-engineering
- User question: the Meter Plus stores ~68 days of history on-device, and
  the official app can download it over BLE — could our backend do the
  same to backfill the database?
- Researched before answering: SwitchBot's official BLE API docs (already
  used for the live-advertisement parser) only document current-state
  commands (basic info, hardware version, current temp/humidity) — no
  history-download command. Searched pySwitchbot, Home Assistant, ioBroker,
  Theengs, and a detailed Hacker News reverse-engineering thread — none
  document or mention this specific feature. Conclusion: it's a
  proprietary, undocumented protocol extension that would need original
  reverse-engineering (BLE traffic capture + manual decoding), not
  something implementable against existing docs or libraries.
- Wrote `docs/specs/ble-history-reverse-engineering.md` at the user's
  request, to evaluate later — not scheduled, no code written. Covers:
  why it's not documented/pre-solved, capture approach (Android HCI snoop
  log vs. iOS/Mac PacketLogger vs. a dedicated BLE sniffer dongle), what
  the backend would need if the protocol were decoded (a new connect-based
  BLE path alongside the existing passive-scan-only design, on-demand not
  continuous, dedup against already-scanned data, timestamp-encoding as
  the biggest unknown), and open risks (possible encryption/pairing
  requirement, protocol might not be stable across firmware versions,
  effort is genuinely unknown up front). Recommends a timeboxed spike
  before committing to full implementation, given the live scanner already
  covers everything going forward — this would only backfill gaps from
  before the backend existed or during downtime.
- Added a pointer to it from `docs/specs/architecture.md` §7 (Open
  Questions).
- Documentation only — no code, no tests, no build changes this entry.

## 2026-08-21 — Spec: installation & deployment (one-liner + CI/CD)
- User needs: a single-command install for both the backend and the
  macOS app, distributable to "anyone with repo access"; backend must
  support both systemd and k3s; separately, an automated CI/CD pipeline
  that deploys to the user's own homelab on every push to `main`.
- Asked clarifying questions before designing (repo visibility; build-
  from-source vs. prebuilt binaries/app; how CI reaches a homelab with no
  exposed ports). Answers: repo is **public**; **build from source** for
  both backend and macOS app (backend: avoids a release pipeline; macOS:
  avoids the Gatekeeper "unverified developer" block entirely, since a
  locally-built `.app` never gets the `com.apple.quarantine` attribute
  that triggers it); CI reaches the homelab via a **self-hosted GitHub
  Actions runner** (outbound-only from the homelab, no inbound ports);
  the automated CI/CD path targets **k3s** specifically (the one-liner
  installer still supports systemd as a separate, manually-chosen option
  for anyone else).
- **Real risk flagged and confirmed with the user before designing
  around it**: BLE from inside a container needs `hostNetwork: true` +
  the host's D-Bus socket bind-mounted + elevated container capabilities
  (Linux/BlueZ, not something Docker/k8s support out of the box). User
  confirmed their k3s node has a reachable Bluetooth adapter and is fine
  with the non-standard pod config this requires.
- Wrote `docs/specs/installation-and-deployment.md`: two install scripts
  (`scripts/install-backend.sh`, `scripts/install-macos-app.sh`, curl-
  piped-to-bash, both build from source), a `Makefile` as a convenience
  wrapper for people who already cloned (not a second implementation), a
  systemd unit + k3s manifests under `deploy/`, and a GitHub Actions
  workflow that builds a Docker image and imports it directly into the
  homelab's k3s containerd — **no container registry needed at all**,
  since the self-hosted runner already has local `docker`/`kubectl`
  access to the cluster it's deploying to.
- Explicitly marked as unverified rather than asserted as fact: whether
  the runtime image needs a system `libdbus` package (believed not,
  since `btleplug`'s Linux backend goes through `bluer`/`zbus`, a pure-
  Rust D-Bus client — but never actually tested), and the whole BLE-in-
  container approach generally, since nothing in this project has run
  the backend inside a container yet — only as a plain process during
  development. Also flagged: Xcode Command Line Tools' first-run GUI
  prompt can't be scripted around on a genuinely fresh Mac.
- Resolved `docs/specs/architecture.md` §7's "deployment mechanism:
  systemd vs. container" open question with a pointer to the new doc.
- Documentation only — no code, scripts, Dockerfile, or CI workflow
  written yet. Next: implement `scripts/install-backend.sh` first (the
  systemd path, since it doesn't depend on the still-unverified BLE-in-
  container question) and validate the k3s/BLE assumptions before
  building that path out.

## 2026-08-21 — Implemented installation & deployment
- Confirmed the GitHub repo path with the user (`alehrs/switchbot-home`)
  and that the repo genuinely has **zero commits and no remote yet** —
  nothing has been pushed in this whole project so far, consistent with
  never having been asked to commit.
- Implemented everything from `docs/specs/installation-and-deployment.md`:
  `scripts/install-backend.sh`, `scripts/install-macos-app.sh`,
  `Makefile`, `deploy/systemd/switchbot-home-backend.service`,
  `deploy/docker/Dockerfile`, `deploy/k3s/*.yaml`,
  `.github/workflows/deploy-backend.yml`, and a new README "Install"
  section.
- Validated what could actually be validated in this environment (Docker
  and kubectl happened to be installed here, even though no k3s cluster
  is reachable — that part is still genuinely deferred to the user's
  homelab as agreed):
  - **Both scripts pass `shellcheck` clean.**
  - **The Dockerfile actually builds and runs** (`docker build` +
    `docker run` on this machine) — this caught a real bug: the spec
    doc's guess that `bluer`'s D-Bus client was pure-Rust `zbus` (no
    system dependency) was **wrong**. The build failed outright without
    `libdbus-1-dev`; fixed by adding it (plus the runtime-only
    `libdbus-1-3`) to the Dockerfile, and corrected the spec doc from
    this real evidence instead of leaving the wrong guess in place. Also
    confirmed, from a real (expected) runtime error with no D-Bus socket
    mounted, the exact socket path the binary looks for:
    `/run/dbus/system_bus_socket` (not `/var/run/dbus/...` as originally
    guessed) — the k3s Deployment's volume mount already matches this.
    `GET /devices` also confirmed working inside the container.
  - **Found and fixed a real bug in the k3s manifests**: `kubectl apply
    -f deploy/k3s/` applies files in alphabetical order, and
    `configmap.yaml`/`deployment.yaml` sort before `namespace.yaml` —
    every one of those would have failed with "namespace not found" on a
    truly fresh cluster. Fixed by renaming all five manifests with
    numeric prefixes (`00-namespace.yaml` … `40-service.yaml`) so
    alphabetical order matches the real dependency order; updated every
    doc/script reference to the old filenames accordingly.
  - **Ran the actual `install-macos-app.sh` script end-to-end for real**
    (not just read it): since the real repo has no commits yet, cloned a
    throwaway copy into `/tmp`, gave *that* copy a local commit (not the
    real repo — no commit was made in the actual project), pointed the
    script's `SWITCHBOT_HOME_REPO_URL`/`SWITCHBOT_HOME_WORKDIR` overrides
    at it, and let the unmodified real script run: it cloned, ran
    `xcodegen generate`, built a Release configuration via `xcodebuild`,
    and installed the result to `/Applications/SwitchBotHome.app`, which
    launched successfully. Quit that instance afterward (left the
    installed `.app` itself in place) without touching the user's own
    already-running Xcode-launched instance.
  - **Not tested**: `install-backend.sh`'s systemd path (`useradd`,
    `setcap`, `systemctl`) — no Linux host was available in this
    environment, exactly the gap already flagged in the spec doc. k3s
    itself also untested for the same reason the user already named:
    that happens on the real homelab.
- Updated `docs/specs/installation-and-deployment.md` throughout to
  reflect what was actually confirmed vs. still assumed, rather than
  leaving the original guesses in place now that better evidence exists.
- No commit or push made — the user has never asked for one at any point
  in this project, and nothing changed that expectation here either.

## 2026-08-25 — Project logo (README + macOS app icon)
- User added `logo.png` (1254x1254) to the repo root and asked for it to
  become the project logo, used in the README and as the macOS app icon.
- **Found and fixed a real defect in the source file before using it**:
  `logo.png` was RGB with no alpha channel (`sips -g hasAlpha` → `no`) —
  it's an app-icon-style render (rounded squircle, glow effects) sitting
  on an *opaque black* canvas rather than a transparent one. Used as-is,
  it would show a black square around the icon in the README (against
  GitHub's white background) and a black-cornered square in the Dock/
  Finder (macOS does not auto-mask flat PNG-based `AppIcon.appiconset`
  images — whatever's in the corners renders as-is).
  - Fixed by un-premultiplying the image against black: for each pixel,
    `alpha = max(R, G, B)`, then `color = color * 255 / alpha` — the
    standard technique for recovering alpha from a glow-style render
    that was composited onto pure black. A naive version amplified
    faint pixel-level noise in the near-black corners into visible
    colored speckle (dividing by a very small alpha blows up any noise
    at that pixel); fixed with a smoothstep noise gate (alpha forced to
    0 below a luminance threshold, eased ramp above it) before the
    unpremultiply. Verified visually (viewed the output) before using it
    anywhere — clean transparent corners, glow preserved, no speckle, no
    black fringe. Required installing `pillow`/`numpy` via `pip3
    --user` (not present on this machine); no other dependency changes.
  - Replaced the root `logo.png` with this fixed (alpha-corrected, same
    1254x1254, same visual design) version rather than keeping the
    original alongside it — this file *is* "the logo" per the ask, and
    the original would only be a footgun (correct-looking until embedded
    somewhere with a non-black background).
- README: added a centered `<img src="logo.png" width="160">` above the
  title.
- macOS app icon: generated a full 10-image `AppIcon.appiconset`
  (16/32/128/256/512, @1x and @2x, `idiom: mac`) from the fixed PNG via
  `sips -z` (confirmed `sips` preserves the alpha channel through
  resize), under the new `macos-app/SwitchBotHome/Assets.xcassets/`.
  Wired it in via `ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon` in
  `macos-app/Project.yml` (xcodegen picks up `.xcassets` folders under
  `sources` automatically — no separate resources entry needed).
  Regenerated the Xcode project (`xcodegen generate`) and validated for
  real: `xcodebuild build` succeeded and the built `.app`'s
  `Contents/Resources/AppIcon.icns` exists with `Info.plist`'s
  `CFBundleIconName` set to `AppIcon`; `xcodebuild test` still passes all
  49 tests. Note: the app is `LSUIElement` (menu-bar-only, no Dock icon
  in normal use), so this icon is mainly visible in Finder/About/
  Spotlight/the Xcode run-destination picker, not the Dock — still worth
  having correct.
- No commit made — consistent with the project's standing practice of
  only committing when the user explicitly asks.

## 2026-08-25 — Diagnosed "no history data" on the real homelab deploy
- User reported: after deploying the backend to the homelab (k3s), the
  macOS app shows a device with a live realtime value, but its day-chart
  history appears empty.
- SSH'd into the homelab (`multivac` alias per `~/.zshrc`) to check live
  state directly rather than guess. `kubectl` needs `KUBECONFIG=/home/
  multivac/.kube/config` (the default `/etc/rancher/k3s/k3s.yaml` is
  root-only — same fact already recorded for CI in the 2026-08-25
  KUBECONFIG entry above). Pod running, config has no
  `READING_INTERVAL_SECONDS`/`RETENTION_DAYS` set (both commented out, so
  no throttling/pruning in play).
  - `GET /devices` → exactly one device,
    `hci0/dev_D2_2E_81_06_5C_61` (confirms the Linux-style slash-in-ID
    case the `a7a870f` percent-encoding fix targets).
  - `GET /readings/latest` → one reading, `id: 1`, `recorded_at:
    2026-08-25T14:14:11Z`.
  - `GET /devices/{id}/readings?from=...&to=...` with the id properly
    percent-encoded (`%2F`) → correctly returns that same single reading;
    the same request with a literal unencoded `/` correctly 404s. **The
    `a7a870f` fix itself is confirmed working end-to-end against the real
    homelab** — that's not the bug here.
  - Root cause of the sparse data: only **one** reading has ever been
    stored, ~35 minutes before checking, and none since — consistent with
    the range issue already flagged in the 2026-08-25 BLE findings entries
    above (bluez works, but nothing was reliably in range of the
    homelab's adapter). Not a new bug, just the same open item showing up
    in the app now that there's a live deployment to look at.
  - **Found and fixed a real, currently-reachable UI bug on top of
    that**: `DeviceDetailView.metricChart` only drew a `LineMark` per
    reading — a line needs two points to render a visible segment, so a
    day with exactly one reading (exactly this device's situation)
    rendered a completely blank chart, indistinguishable from the
    explicit "No data for this day" empty state one row above it in the
    same view, even though real data was being fetched. Fixed by adding a
    `PointMark` alongside the `LineMark` for every reading (not just the
    hover-selected one) — makes single/sparse-data days visible as dots,
    and gives every other day per-sample dots on the line too as a
    side benefit.
  - Verified: `xcodebuild build`/`test` clean (49 tests, no new test
    added — this is Chart rendering, not testable pure logic, consistent
    with the project's established split), rebuilt Release and reinstalled
    to `/Applications/SwitchBotHome.app` (relaunched, replacing the build
    from the icon work earlier today) so the user can check the real
    device's history against the live homelab backend.
- Next: user to confirm the single point now shows in the day chart, and
  separately, physically bring a Meter Plus within range of the homelab's
  Bluetooth adapter for a sustained period to get continuous history (the
  range question is still open and isn't something software can fix).

## 2026-08-25 — Real bug found and fixed: readings-history requests silently double-encoded
- User reported the day chart still showed "No data for this day" after
  the previous entry's fixes, on the real homelab-connected app. Manual
  `curl` replication of the exact same query (device ID, day range)
  against the live backend kept succeeding, which didn't match — a sign
  the bug was client-side, in exactly what the app itself sent, not
  reasoning about the request.
- **Investigation approach**: rather than keep guessing from macOS's
  opaque CFNetwork network-summary logs (which show byte counts/status
  but not the actual path), added a temporary `tracing::info!` line to
  `backend/src/api/readings.rs` logging the received `device_id`/`from`/
  `to`. Asked the user before deploying it — this meant briefly replacing
  the live homelab backend's running image with a debug build — user
  approved. Built the debug image by rsyncing the local source to the
  homelab and building there (`docker build` against
  `deploy/docker/Dockerfile`), rather than pushing to the repo/triggering
  real CI/CD for a throwaway diagnostic. Watched the live pod logs
  (`kubectl logs -f`) while the app's own background polling
  (`PollingService.runSlowCycle`, which independently fetches each
  device's trailing-1h history for the trend indicator — a second,
  automatic caller of the same endpoint the day-chart uses) made its next
  request.
- **Root cause found**: the request logged as
  `device_id=hci0%2Fdev_D2_2E_81_06_5C_61` — still percent-encoded after
  axum's `Path` extractor (which decodes once), meaning the wire value was
  double-encoded (`%252F`). `APIClient.urlComponents(path:)` was building
  the path string with the device ID already percent-encoded (via the
  `a7a870f` fix's `pathEncoded`), then assigning it to
  `URLComponents.path`. Confirmed via a minimal standalone Swift repro:
  `URLComponents.path`'s setter treats its input as the *decoded* logical
  value and re-encodes it — `%2F` becomes `%252F`. `curl`/manually-typed
  URLs never go through this code path, which is why every manual
  replication kept succeeding while the real app kept failing. This bug
  has been present since `a7a870f` — every readings-history request the
  app has ever made for a slash-containing (Linux-style) device ID has
  silently returned zero rows (200 OK, not 404) since that fix landed.
  The original `pathEncoded` unit tests (in `a7a870f`) only tested that
  helper in isolation and couldn't have caught this — the bug is one step
  further down, in how its output gets used.
- **Fix**: `APIClient.swift` — changed `components.path = path` to
  `components.percentEncodedPath = path` (which takes its input as
  already-encoded and uses it verbatim). Widened `urlComponents(path:)`
  from `private` to internal (still not `public`) so
  `APIClientTests.swift` can exercise it directly via `@testable import`.
  Added `testFetchReadingsURLDoesNotDoubleEncodeADeviceIDSlash`, which
  builds the actual production URL for a slash-containing device ID and
  asserts the encoded path survives unchanged — a test on the real
  fetchReadings-URL-construction path, not just `pathEncoded` alone,
  so this exact regression can't reappear silently again.
- Verified: `swift` repro script confirmed the `.path` vs
  `.percentEncodedPath` behavior directly (not just reasoned about) before
  writing the fix. `xcodebuild build`/`test` clean, 50 tests passing (1
  new). Rebuilt Release, reinstalled to `/Applications/SwitchBotHome.app`,
  relaunched.
- **Homelab cleanup**: restored the deployment to the exact image tag it
  ran before the debug swap
  (`registry.local:5000/switchbot-home-backend:2ba10eeaf970822ba3fadc4c14407cd12e14bfee`,
  confirmed via `kubectl rollout status`), deleted the local `debug-readings`
  image tag and the `/tmp/switchbot-debug-src` rsync copy on the homelab
  node, and reverted the temporary `tracing::info!` line from
  `backend/src/api/readings.rs` — none of the diagnostic scaffolding was
  kept. No commit or push made.
- Next: user to confirm the day chart now shows the device's reading(s).
  This also transitively fixes `PollingService`'s trend indicator for
  this device (it was silently getting "insufficient data" for the same
  reason), so that's worth a glance too.
