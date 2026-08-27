---
date: 2026-08-27
type: decision
tags: [ble, switchbot, meter, manufacturer-data, btleplug, mac-address]
files:
  - backend/src/ble/switchbot.rs
  - backend/src/ble/scanner.rs
---

# Some SwitchBot meters put temp/humidity (and the real MAC) in manufacturer data, not service data

**Context.** Adding the Outdoor Meter (Indoor/Outdoor Meter, device type
`w`/`0x77`). Its advertisements were all rejected with "advertisement did not
match a known meter format" (`raw=[119, 192, 228]`).

**What.** The base Meter and Meter Plus (`T`/`t`/`I`/`i`) carry the 3-byte
temperature/humidity payload in BLE **service data** bytes 3..6. The Outdoor
Meter's service data is only 3 bytes (device type + battery); the payload moves
to **manufacturer data** bytes 8..11 (after a 6-byte MAC + 2-byte header), keyed
by SwitchBot company ID `0x0969`. `parse_meter_advertisement` takes both buffers
and prefers service data when it's long enough. Meter Pro (`4`) / Meter Pro CO2
(`5`) work the same way, with CO2 at manufacturer bytes 13..15 — mirrors
pySwitchbot `process_wosensorth` / `process_wosensorth_c`.

**Why.** `btleplug` delivers one advertising PDU as *separate*
`CentralEvent::ServiceDataAdvertisement` and `ManufacturerDataAdvertisement`
events. The obvious approach — read manufacturer data straight off the
service-data event — is impossible: it isn't there. The scanner keeps a
per-device `switchbot_mfr_data` cache filled from the manufacturer-data event
and read by the service-data handler. The first advert after startup can lose
the ordering race and is skipped; the next broadcast (~seconds) has both.

**Rejected alternatives.** Calling `peripheral.properties()` on every reading to
get a consistent snapshot of both — rejected as an unnecessary per-reading async
lookup (same reasoning that keeps the MAC lookup once-per-device).

**Gotcha.** The first 6 bytes of a SwitchBot meter's manufacturer data are its
real MAC in natural order (MSB first). CoreBluetooth does **not** mask this the
way it masks the link-layer address (`properties().address` is all-zero on
macOS), so `resolve_mac_address` falls back to it and `devices.mac_address` can
be non-null even on a Mac. If a meter model ever shows a wrong MAC, check the
byte order against node-switchbot's `extractMacFromManufacturerData` first.
