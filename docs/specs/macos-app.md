# macOS menu-bar app (SwitchBotHome)

Status: Implemented (v1) — see `macos-app/`
Last updated: 2026-08-21

## 1. Overview

A Swift/SwiftUI menu-bar app that reads from the backend's REST API (see
`architecture.md` §4.5) and shows temperature/humidity per device, grouped
by room, with a 1-hour rolling average and a trend indicator (rising/
falling arrow + signed delta) for both temperature and humidity. It talks
**only** to the REST API — no BLE access, no
SwitchBot-specific logic on the client, matching `architecture.md` §5.1.

## 2. Tech stack decision: Swift/SwiftUI, not Rust

`architecture.md` originally planned a Rust client on `tray-icon`+`muda`
(+optional `egui`/`egui_plot`). That fit a flat native menu; it doesn't fit
this app's actual requirements: room-grouped sections, colored trend
badges, an offline banner, and a settings form — real UI composition, not
a list of plain menu items. `NSMenu` (what `tray-icon`/`muda` wrap) can't
express that layout without dropping into raw AppKit `NSView` menu items
from Rust, at which point none of Rust's advantages (code reuse with the
backend, cross-platform) apply anyway for a single-user, Mac-only utility.
SwiftUI's `MenuBarExtra` gives the grouped/colored layout declaratively,
plus Dark Mode/Dynamic Type correctness for free.

**Decision: Swift/SwiftUI.** This supersedes `architecture.md` §5.1/§6.

## 3. App shape

- **Xcode project** at `macos-app/SwitchBotHome.xcodeproj` (needed for a
  proper `.app` bundle with `Info.plist`/entitlements/code signing — a
  bare SwiftPM package doesn't give this without extra manual plumbing).
- `LSUIElement = true` → menu-bar-only, no Dock icon, no app-switcher entry.
- `MenuBarExtra` scene, **`.window` style** (a borderless popover filled
  with arbitrary SwiftUI), not `.menu` — required for section headers,
  colored badges, and the offline banner.
- **Minimum deployment target: macOS 14 (Sonoma)** — the design uses
  `@Observable` (Observation framework) and `@Environment(\.openSettings)`
  for the settings window, both macOS 14+. No reason to target older;
  nothing here needs to run pre-14, and the dev machine is on 26.6.2.

## 4. File layout

```
macos-app/
├── SwitchBotHome.xcodeproj
└── SwitchBotHome/
    ├── SwitchBotHomeApp.swift        # App entry: MenuBarExtra + Settings scenes
    ├── Info.plist                    # LSUIElement, NSAppTransportSecurity, NSLocalNetworkUsageDescription
    ├── SwitchBotHome.entitlements    # App Sandbox on + com.apple.security.network.client
    ├── Models/
    │   ├── Device.swift              # Codable mirror of backend Device
    │   ├── Reading.swift             # Codable mirror of backend Reading
    │   └── DeviceSnapshot.swift      # presentation model: displayName, room, rank, latest values, avg1h, trend
    ├── Networking/
    │   └── APIClient.swift           # URLSession wrapper; base URL read from AppSettings per-request
    ├── Services/
    │   ├── PollingService.swift        # fast/slow poll cycles (§6)
    │   ├── DeviceNumbering.swift       # pure: [Device] -> [device_id: rank] (§7)
    │   ├── GroupingRules.swift         # pure: [DeviceSnapshot] -> [(room, [DeviceSnapshot])] (§8)
    │   ├── TrendCalculator.swift       # pure: [Reading] -> Trend (§9)
    │   ├── DayRange.swift              # pure: calendar-day [start, end) boundaries (§9a)
    │   ├── ReadingsDownsampler.swift   # pure: caps chart point count (§9a)
    │   └── LocalCache.swift            # last-known snapshots -> Application Support JSON
    ├── Store/
    │   └── AppStore.swift            # @Observable: snapshots, sections, connectionState
    ├── Views/
    │   ├── MenuBarLabelView.swift
    │   ├── PopoverContentView.swift    # NavigationStack root: list <-> device detail
    │   ├── RoomSectionView.swift
    │   ├── DeviceRowView.swift
    │   ├── DeviceDetailView.swift      # day charts + day navigation (§9a)
    │   ├── TrendIndicatorView.swift
    │   └── OfflineBannerView.swift
    └── Settings/
        ├── SettingsView.swift        # base-URL field + validation
        └── AppSettings.swift         # UserDefaults-backed
└── SwitchBotHomeTests/
    ├── DeviceNumberingTests.swift
    ├── GroupingRulesTests.swift
    ├── TrendCalculatorTests.swift
    ├── DayRangeTests.swift
    ├── ReadingsDownsamplerTests.swift
    └── ModelDecodingTests.swift
```

## 5. Backend contract this app relies on

```
GET  /devices                          -> [Device]
PUT  /devices/{device_id}               -> Device
GET  /devices/{device_id}/readings      -> [Reading]   (?from=&to=)
GET  /readings/latest                   -> [Reading]   (excludes blacklisted)
```
`Device`: `{ device_id, label: string|null, room: string|null, blacklisted, first_seen_at, last_seen_at }`.
`Reading`: `{ id, device_id, temperature (°C), humidity (%), battery: number|null, recorded_at }`.

`device_id` is an opaque string — on macOS (dev) it's a CoreBluetooth
per-app UUID, not a real MAC address (see `architecture.md` §3's platform
caveat); the client must never parse or attach meaning to it beyond
identity. `GET /devices/{id}/latest` exists on the backend but is unused
by this client — `/readings/latest` already covers "current value for
every device" in one call.

## 6. Polling

No push/streaming exists on the backend — REST polling only.

- **Fast cycle, every 30s**: `GET /readings/latest`. Updates every
  snapshot's current temperature/humidity/battery.
- **Slow cycle, every 3 minutes** (plus once on popover open if the last
  refresh is >10s old): `GET /devices` to refresh labels/rooms/blacklist,
  then a `TaskGroup` fanning out one `GET
  /devices/{id}/readings?from=<now-60m>&to=<now>` per device concurrently
  — there's no batch history endpoint. Device counts are small (a house's
  worth of sensors), so no concurrency cap is needed.
- **Offline handling**: a failed cycle never clears existing data. It sets
  `connectionState = .offline(since:)`; the popover shows an
  `OfflineBannerView`, and rows show "as of HH:mm" once stale. The next
  successful cycle clears the banner automatically — no user action.
- **Sleep/wake**: poll via a `Task { while true { poll(); sleep } }` loop,
  plus an immediate out-of-cycle refresh on
  `NSWorkspace.didWakeNotification` so data isn't stale for a full cycle
  after the Mac wakes.

## 7. "Unknown device #N" naming

Sort the **full** `GET /devices` roster (including blacklisted) ascending
by `first_seen_at`; 1-based index = that device's permanent rank. Show
"Unknown device #{rank}" only when `label == nil`.

**No local persistence needed.** `first_seen_at` is set once at
auto-discovery and never rewritten (`backend/src/storage.rs`'s
upsert-on-first-sighting never touches it again) — it's always "now" at
insert time, so a later-discovered device can only ever sort after every
earlier one, never in the middle. Re-sorting on every fetch reproduces the
same order forever.

Rank must come from the **full roster**, not just currently-unlabeled
devices: numbering only the unlabeled subset densely would mean labeling
device #2 silently renumbers #3 to #2 — exactly the shuffling to avoid.
With full-roster ranking, labeling #2 just removes it from display,
leaving "#1, #3" — a gap, not a shuffle. Blacklisting/un-blacklisting a
device doesn't affect its rank either (rank uses the full roster; only
*rendering* excludes blacklisted devices, per §8).

## 8. Room grouping

- Group key: `room` trimmed of whitespace; `nil`/empty → `"Ungrouped"`. No
  case-folding — whatever the user typed via `PUT /devices/{id}` displays
  verbatim, so rooms are never silently merged or split.
- Section order: real room names via `localizedStandardCompare` (locale-
  and numeral-aware); `"Ungrouped"` always **last**, regardless of where
  it'd alphabetically fall.
- Within a section: sort by `first_seen_at` ascending — the same key used
  for numbering (§7), one ordering rule everywhere.
- Only `blacklisted == false` devices render in any section (mirrors
  `/readings/latest`'s own exclusion); blacklisted devices still count for
  rank (§7), they just never show a row.

## 9. Trend indicator (temperature and humidity)

```
baseline = mean(value) over readings in [now-60m, now-55m]
recent   = mean(value) over readings in [now-5m,  now]
delta    = recent - baseline
```

The same edge-averaged two-point delta is computed independently for both
`temperature` and `humidity` (each device's 1h reading window already
carries both fields, so this is one shared, keypath-parameterized
calculation, not duplicated logic). Chosen over a single-sample delta
(would flicker on ordinary sensor jitter, since unthrottled readings can
arrive every few seconds) and over a linear-regression slope (wrong unit:
the user asked for a plain delta value like a stock-app indicator, not a
rate; also more code for little extra robustness here, since averaging
each edge already smooths noise). Reuses the same 1h-window fetch already
needed for the average — no extra API call, and no extra request for
humidity since it's the same payload.

- Temperature deadband: `|delta| < 0.2°C` → **flat**. Humidity deadband:
  `|delta| < 2%` → **flat** (humidity naturally swings more than
  temperature; a 0.2 threshold would flicker constantly). Both deadbands
  exist for the same reason: stop the arrow flipping direction on noise
  near zero.
- `delta >= deadband` → `arrow.up.forward`, **orange/red** tint,
  `"+X.X°"` / `"+X.X%"`.
- `delta <= -deadband` → `arrow.down.forward`, **blue** tint, `"−X.X°"` /
  `"−X.X%"`.
- **Deliberately not** the literal Trade Republic green=up/red=down: that
  mapping encodes "up is good," which doesn't hold for temperature or
  humidity. Warm (rising) / cool (falling) tints keep the requested
  *style* — small arrow, colored signed delta, no percentage-of-baseline
  math — without a false value judgment, and using the same two colors
  for both metrics keeps the visual language consistent and easy to
  learn. This should stay a code comment near the color logic so it isn't
  "corrected" back to green/red by someone assuming it's a mistake.
- Fallback for devices with sparse/new history (applies to both metrics
  independently): fewer than 5-minute-wide edges but ≥2 readings → raw
  first-vs-last delta over what exists; 0–1 readings → a neutral
  "insufficient data" dash, never a misleading `"0.0"`.
- `TrendCalculator.swift` exposes one function taking a keypath (`\.temperature`
  or `\.humidity`) plus its deadband, so the temperature and humidity rows
  share one tested implementation instead of two near-duplicate ones.
- Each value in `DeviceRowView` is preceded by a small icon (`thermometer`
  for temperature, `drop.fill` for humidity) — added after a user report
  that a bare `"67%"` isn't obviously humidity at a glance the way `"23°C"`
  is obviously temperature.

## 9a. Device detail: day charts (added after initial v1)

Tapping a device row (a trailing chevron hints it's tappable) pushes a
day-chart screen for that device, with two line charts (temperature,
humidity) and prev/next day navigation ("next" disabled once on today).

- **Navigation: `NavigationStack` inside the same popover, not a separate
  window.** A `WindowGroup` was considered — more room, standard window
  chrome — but `LSUIElement` (menu-bar-only) apps have unreliable
  activation/focus behavior for auxiliary windows (no Dock icon to anchor
  window management to). Pushing a wider screen onto the existing
  `MenuBarExtra(.window)` popover's `NavigationStack` avoids that
  entirely — SwiftUI resizes the popover to whatever screen is currently
  pushed, and there's no second window lifecycle to manage. `DeviceRowView`
  is wrapped in `NavigationLink(value: device.id)` in `RoomSectionView.swift`.
- **Day boundaries**: `DayRange` (`Services/DayRange.swift`) computes
  `[start, end)` as local calendar midnight to the following local
  midnight, using `Calendar.date(byAdding: .day, ...)` rather than raw
  86,400-second arithmetic, so navigating across a Daylight Saving Time
  transition still lands on the correct wall-clock midnight. For "today,"
  `end` is simply the *next* midnight, not `now` — the query naturally
  returns nothing past the current moment since no future readings exist,
  so the chart's x-axis can stay fixed to the full day (`chartXScale`)
  for every day, today included, rather than needing special-case clamping.
- **Fetching**: reuses `GET /devices/{id}/readings?from=&to=` (§5) with
  `from`/`to` set to the selected `DayRange` — no backend change needed.
- **Point count safeguard**: a full unthrottled day (no
  `READING_INTERVAL_SECONDS` set — the backend's own default) can reach
  tens of thousands of readings; even a partial day already hit 562 in
  manual testing. `ReadingsDownsampler` (`Services/ReadingsDownsampler.swift`)
  caps what actually reaches the chart at ~300 points via time-bucketed
  averaging (not naive every-Nth-point sampling, which can alias away or
  exaggerate short spikes depending on where the stride lands).
- **Stale-response guard**: switching days quickly cancels the in-flight
  fetch for the old day (`.task(id: dayRange)`), but Swift's task
  cancellation is cooperative — a request that happens to finish network
  I/O right as a newer one starts isn't guaranteed to be pre-empted before
  it resumes and writes its result. `DeviceDetailView.load()` captures the
  day range a given call is actually for and only commits its result if
  that range still matches the currently-selected one, closing the race
  unconditionally rather than relying on cancellation timing.

## 10. Settings: configurable API base URL

- A SwiftUI `Settings` scene, opened via `openSettings` from a
  "Settings…" row in the popover. `TextField` for the base URL (default
  prefilled `http://localhost:3000`, matching the backend's documented
  default), a "Test Connection" button (`GET /devices` against the
  entered value) showing inline success/failure before the value is
  treated as applied. Validated via `URLComponents` (scheme must be
  http/https, host non-empty) — reject silently-broken input.
- Persisted in `UserDefaults` via `AppSettings` (`@Observable`);
  `APIClient` reads the current value per request, so a change takes
  effect immediately, no app restart.

### Networking entitlements/ATS

The backend has no TLS, and the base URL is always loopback or a LAN IP,
never a public host:

- `Info.plist` → `NSAppTransportSecurity` → `NSAllowsLocalNetworking =
  true` — the key built for exactly this (plain HTTP to loopback/private
  IP ranges/`.local` hosts), not the blanket `NSAllowsArbitraryLoads`.
- `Info.plist` → **`NSLocalNetworkUsageDescription`** (a short string).
  Required on modern macOS for the separate "Local Network" privacy
  permission that gates outbound LAN connections — omitting it risks the
  OS failing the connection silently instead of prompting, which would
  look like a bug rather than a missing permission.
- Keep **App Sandbox ON**; add `com.apple.security.network.client` to the
  entitlements file — the only capability actually needed. No reason to
  disable sandboxing for a one-line entitlement.
- Settings help text should mention macOS may show a one-time "allow
  local network access" prompt — expected, not an error state.

## 11. Other considerations

- **Menu bar collapsed state**: icon only (e.g. `thermometer.medium` SF
  Symbol), no numeric value; tinted orange when offline. With multiple
  devices across rooms there's no single meaningful number to show
  collapsed — averaging across rooms is meaningless, and picking a
  "primary" device needs a pinning feature nobody asked for. A "pin a
  favorite device's value into the menu bar" is a reasonable future
  enhancement, not core scope.
- **Instant launch UI**: `LocalCache` writes the last-known snapshots as
  JSON under `~/Library/Application Support/SwitchBotHome/` after every
  successful fast cycle. On launch this loads synchronously (marked
  visually as cached/stale) so the popover is never empty before the
  first network round-trip completes.
- **Explicitly out of scope for now**: launch-at-login (`SMAppService`,
  trivial to add later behind a settings toggle) and notifications/alerts.
  In-app editing of a device's **label and room** is implemented (§13);
  toggling `blacklisted` is still a manual `backend/api.http`/curl step.

## 12. Testing strategy

**Unit tests** (pure logic, no backend/network needed):
- `DeviceNumberingTests` — mixed labeled/unlabeled ranking; labeling a
  mid-ranked device leaves a gap, doesn't renumber later devices; a newly
  discovered device always gets the highest rank; ranks survive blacklist
  toggles.
- `GroupingRulesTests` — nil/empty/whitespace room → "Ungrouped"; that
  section always sorts last; within-group order matches `first_seen_at`;
  blacklisted devices never appear.
- `TrendCalculatorTests` — rising/falling synthetic series produce the
  right sign; a noisy-but-flat series stays within the deadband (no
  flicker); sparse-data and 0/1-reading fallbacks behave as specified.
- `ModelDecodingTests` — decode fixture JSON matching the real `Device`/
  `Reading` shapes, including `label`/`room`/`battery` all `null`.
- `DayRangeTests` — midnight-to-midnight boundaries; advancing forward/
  backward a day; crossing a month boundary; `isToday` reflects the real
  current day.
- `ReadingsDownsamplerTests` — under-the-limit arrays pass through
  unchanged; over-the-limit arrays reduce to the cap; buckets are actually
  averaged, not just sampled; chronological order is preserved.

**Manual verification** (needs `cargo run -p backend` live):
- Empty backend renders a sane empty state, not a crash.
- Real devices show correctly grouped/labeled; a null-labeled device shows
  "Unknown device #N" with the right, stable rank.
- Kill the backend mid-run → offline banner appears, rows show "as of
  HH:mm"; restart it → automatic recovery next cycle, no relaunch.
- Tap a device row → day chart appears for today; navigate to previous
  days and confirm both charts update; confirm "next day" is disabled on
  today; navigate rapidly back-to-back and confirm the displayed day never
  shows another day's data.
- Quit/relaunch the app while the backend is down → cached data renders
  immediately, marked stale.
- Label a previously-"Unknown device #2" via `backend/api.http`, refresh →
  it disappears from the unnamed list, its siblings keep their numbers.
- Tap a device → Edit → set a label and room → Save: the row updates and
  re-groups immediately (no wait for the slow cycle); clearing the label
  field and saving reverts it to "Unknown device #N"; saving with the
  backend down shows an inline error and doesn't crash.
- Sleep/wake the Mac → refresh fires immediately on wake.
- Settings: wrong URL → "Test Connection" fails inline; correct LAN
  IP/port → app switches over live, no restart, no ATS-blocked-connection
  errors in Console.app.
- Visual check in Light and Dark appearance; offline vs. normal menu bar
  icon tint is distinguishable at a glance.

## 13. Device editing (label + room)

Tapping a device row opens `DeviceDetailView` (§9a); an **Edit** button in
that screen's header pushes `DeviceEditView` onto the same popover
`NavigationStack` (no separate window, for the §9a reason). It has a
`Label` and a `Room` field plus a menu to reuse an existing room name
(rooms group verbatim per §8, so this reduces accidental "cucina" vs
"Cucina" splits). Save issues `PUT /devices/{device_id}` via a new
`APIClient.updateDevice(...)`, then `AppStore.applyDeviceUpdate(_:)`
patches the matching snapshot's `Device` in place so the list re-groups
and re-labels immediately without waiting for the slow poll cycle.

Both fields are always sent as strings: an emptied field is sent as `""`,
which the backend maps to `NULL` — so the screen never deals with the
backend's omit-vs-clear tri-state itself. `blacklisted` is always sent as
omitted (`nil`) from this screen.

## 14. Open questions (deferred, not blocking)

- Should a future per-device detail view use `GET /devices/{id}/latest`
  (currently unused) for anything, or is `/readings/latest` always enough?
- Launch-at-login and notifications (§11) — worth doing at some point, not
  designed here.
- Toggling `blacklisted` from the app (the `updateDevice` API method
  already carries the parameter; only the UI toggle is missing).
