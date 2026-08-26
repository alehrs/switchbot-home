---
name: lsuielement-window-focus
description: LSUIElement/accessory apps don't auto-activate when showing a new window (e.g. openSettings()) — it opens behind other apps and doesn't focus
metadata:
  type: project
---

`macos-app/SwitchBotHome/Info.plist` sets `LSUIElement = true` (menu-bar
only, no Dock icon). This makes the app run with AppKit's `.accessory`
activation policy, which has a consequence that isn't obvious from
SwiftUI's `Settings` scene / `openSettings()` API alone: **showing a new
window never implicitly activates the app**. The window is created and
can even become key, but the *app* stays in the background, so the window
visually renders behind whatever app currently has focus.

**Fix pattern** (applied to the Settings window in
`Views/PopoverContentView.swift` + `Settings/SettingsView.swift`):
1. Call `NSApp.activate()` immediately before triggering the window (e.g.
   before `openSettings()`). Use the no-arg `activate()`, not
   `activate(ignoringOtherApps:)` — the latter is deprecated as of macOS
   14 (this project's `MACOSX_DEPLOYMENT_TARGET`), and the no-arg form
   already behaves as `ignoringOtherApps: true`.
2. If the window needs to reliably surface on whichever macOS Space the
   user is *currently* on (not the Space it was first created on — SwiftUI
   `Settings` reuses one window instance for the app's lifetime), also
   grab the underlying `NSWindow` (SwiftUI doesn't expose it directly; use
   an `NSViewRepresentable` "WindowAccessor" bridge) and set
   `collectionBehavior.insert(.moveToActiveSpace)` +
   `makeKeyAndOrderFront(nil)` on it.

**Why:** user reported the Settings window always opened behind
already-open apps, regardless of which desktop/Space they were on.

**How to apply:** any future window shown from this menu-bar-only app
(not just Settings) needs the same `NSApp.activate()` treatment before it
will reliably come to the front — this is a property of the app's
activation policy, not of the specific window/API used to show it.

See [[urlcomponents-path-double-encoding]] for another macOS-app-specific
platform gotcha in this codebase.
