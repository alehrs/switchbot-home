---
date: 2026-08-25
type: bugfix
tags: [urlcomponents, percent-encoding, swift, api-client]
files:
  - macos-app/SwitchBotHome/Networking/APIClient.swift
---

# `URLComponents.path =` re-encodes an already-percent-encoded string

**Context.** `a7a870f` fixed 404s on readings-history requests for Linux-style
device IDs (`hci0/dev_XX...`) by percent-encoding the `/` before building the
path. The day chart still showed "No data for this day" afterward, and manual
`curl` replication of the exact same request always succeeded — the bug had
to be in what the app itself sent, not the query.

**What.** `urlComponents(path:)` built the path string with the device ID
already percent-encoded, then assigned it via `components.path = path`. Fixed
by using `components.percentEncodedPath = path` instead.

**Why.** `URLComponents.path`'s setter treats its input as the *decoded*
logical value and percent-encodes it for you — including the `%` in an
already-encoded `%2F`, turning it into `%252F`. The server still decodes this
once, sees the *literal text* `%2F` as part of the device_id, and finds no
matching row: a silent 200 with an empty array, not a 404. This is much
sneakier than the original bug and explains why nothing looked broken from
the server's error logs. `percentEncodedPath` is the setter that takes
already-encoded input and uses it verbatim.

**Rejected alternatives.** None — once the double-encoding was confirmed with
a two-line Swift repro (`components.path = "...%2F..."` vs
`components.percentEncodedPath = "...%2F..."`, printing `.url!`), the fix was
immediate.

**Gotcha.** Any time a path segment needs manual percent-encoding before being
handed to `URLComponents`, it MUST go through `.percentEncodedPath`, never
`.path`. A unit test on the encoding helper alone (`pathEncoded`, added in
`a7a870f`) does not catch this — the bug is one step further down, in how the
helper's *output* gets consumed. Test the actual constructed URL/path, not
just the encoding function.
