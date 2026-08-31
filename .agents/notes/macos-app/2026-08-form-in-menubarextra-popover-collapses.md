---
name: form-in-menubarextra-popover-collapses
description: In the MenuBarExtra(.window) popover a `Form`/`ScrollView` collapses the screen, and `.navigationTitle` renders a misplaced bar — pushed screens must mirror DeviceDetailView
metadata:
  type: bugfix
---

`MenuBarExtra(.window)` sizes its popover to the **ideal size** of its
content, and a pushed `NavigationStack` destination doesn't get a real
title bar. Two ways a screen breaks there:

1. A SwiftUI `Form` — or a bare `ScrollView` — has an ill-defined ideal
   height, so a screen built around one renders **collapsed**: near-zero
   height, no visible fields or buttons.
2. `.navigationTitle(...)` on a pushed destination renders a **misplaced
   bar** — content ends up shoved into a corner of an oversized popover
   (user report: "input boxes far bottom-right, can't scroll").

`DeviceEditView` hit both, in that order (blank → corner-shoved).

**Rule for this app:** a pushed popover screen mirrors `DeviceDetailView`
exactly — a plain `VStack(alignment: .leading, spacing: 12)`, an
**in-content title** (`Text(...).font(.title2.bold())`, *never*
`.navigationTitle`), `.padding(16)`, `.frame(width: 460)` (a constant
width so the popover doesn't jump when navigating detail ↔ edit), a
trailing `Spacer(minLength: 0)` so short content pins to the top. The
system back button still appears (it comes from the `navigationDestination`
push, not from `.navigationTitle`). `Form` is fine only in the `Settings`
scene (`SettingsView`), a real window.

**Regression guard:** `DeviceEditViewLayoutTests` renders the view with
`ImageRenderer` and asserts `image.size` is over 200pt tall and 400pt
wide — catches a collapse (144pt) without pixel-comparison flakiness.
(`ImageRenderer` renders `TextField(.roundedBorder)` as a solid block —
an AppKit-control limitation — so assert on size, never pixels.
`ImageRenderer.proposedSize` also does *not* force the canvas: the view
still returns its own ideal size, so it can't reproduce the
corner-shoved case — that needed a live build.)

See [[lsuielement-window-focus]] for the other class of
`MenuBarExtra`/`LSUIElement` popover gotcha in this codebase.
