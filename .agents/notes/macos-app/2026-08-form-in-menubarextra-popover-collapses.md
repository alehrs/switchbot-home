---
name: form-in-menubarextra-popover-collapses
description: A SwiftUI `Form` (or bare `ScrollView`) inside the MenuBarExtra(.window) popover has no ideal height and collapses the screen to ~nothing
metadata:
  type: bugfix
---

`MenuBarExtra(.window)` sizes its popover to the **ideal size** of its
content. A SwiftUI `Form` — and a bare `ScrollView` — has an ill-defined
ideal height, so a screen built around one renders collapsed: near-zero
height, no visible fields or buttons.

Seen twice:
- `PopoverContentView`'s device list `ScrollView` → fixed with an
  explicit `.frame(minHeight:idealHeight:maxHeight:)`.
- `DeviceEditView` used `Form { Section { … } }` → the whole Edit screen
  rendered blank (user report: "no input boxes, no Save button").
  `ImageRenderer(content:)` reproduced it exactly — 340×144 for a screen
  that should be ~340×240.

**Rule for this app:** every popover screen is a plain
`VStack(alignment: .leading, spacing:)` with `.frame(width: 340)` and
intrinsically-sized children (`TextField`, `Text`, `Button` all have a
real intrinsic height; a `VStack` of them has a real ideal height).
`Form` is fine only in the `Settings` scene (`SettingsView`), which is a
real window, not the borderless popover. Match `PopoverContentView` /
`DeviceDetailView`.

**Regression guard:** `DeviceEditViewLayoutTests` renders the view with
`ImageRenderer` and asserts `image.size` is over 200×300 — catches a
collapse without pixel-comparison flakiness. (`ImageRenderer` renders
`TextField(.roundedBorder)` as a solid block — an AppKit-control
limitation — so assert on size, never pixels.)

See [[lsuielement-window-focus]] for the other class of
`MenuBarExtra`/`LSUIElement` popover gotcha in this codebase.
