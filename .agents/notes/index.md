# Project notes index

## ble
- [Some SwitchBot meters put temp/humidity (and the real MAC) in manufacturer data, not service data](ble/2026-08-meter-manufacturer-data.md) — decision, switchbot, manufacturer-data, btleplug — 2026-08-27
- [btleplug BlueZ scanner: the stream can hang (not just end); the RTL8761 dongle stalls; PeripheralId carries the hciN prefix](ble/2026-08-scanner-resilience.md) — decision, btleplug, bluez, scanner, adapter, realtek, autosuspend — 2026-08-28, upd 2026-08-31

## macos-app
- [`URLComponents.path =` re-encodes an already-percent-encoded string](macos-app/2026-08-urlcomponents-path-double-encoding.md) — bugfix, urlcomponents, percent-encoding — 2026-08-25
- [LSUIElement apps don't auto-activate when showing a new window](macos-app/2026-08-lsuielement-window-focus.md) — bugfix, activation-policy, window-focus — 2026-08-26
- [A `Form`/`ScrollView` in the MenuBarExtra popover has no ideal height and collapses the screen](macos-app/2026-08-form-in-menubarextra-popover-collapses.md) — bugfix, swiftui, menubarextra, layout — 2026-08-31
