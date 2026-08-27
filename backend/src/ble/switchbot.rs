//! Parses SwitchBot Meter / Meter Plus / Outdoor Meter BLE advertisements.
//!
//! Format reference: OpenWonderLabs' official BLE API docs
//! (SwitchBotAPI-BLE, devicetypes/meter.md, "New Broadcast Message") for the
//! byte layout, and pySwitchbot's device type table for the Meter Plus
//! device type byte (0x69/0x49) and the Outdoor Meter ('w'/'W'), neither of
//! which is in the official doc.
//!
//! The base Meter and Meter Plus carry the 3-byte temperature/humidity
//! payload in their service data (bytes 3..6). The Outdoor Meter's service
//! data stops after the battery byte; it moves the same 3-byte payload into
//! its manufacturer data (bytes 8..11, after a 6-byte MAC and a 2-byte
//! header). Matches pySwitchbot's `process_wosensorth`.
//!
//! A meter seen from too far to answer a `SCAN_REQ` never delivers its
//! service data at all (that rides in the `SCAN_RSP`), only the `ADV_IND`
//! with the same manufacturer-data payload. `parse_meter_manufacturer_data`
//! recovers a reading (minus battery, which lives only in service data)
//! from that alone.

use std::sync::LazyLock;

use uuid::Uuid;

/// Device type bytes (bits 6:0 of service data byte 0) that share this
/// temperature/humidity/battery layout: the base MeterTH in its two
/// broadcast modes ('T'/'t'), Meter Plus in its two broadcast modes
/// ('I'/'i'), and the Outdoor Meter ('w'/'W'). Meter Plus was confirmed
/// against a real device (type 0x69, 'i'); the codes come from pySwitchbot's
/// device type table (used by Home Assistant), not OpenWonderLabs' meter.md.
const METER_DEVICE_TYPES: [u8; 6] = [0x54, 0x74, 0x49, 0x69, 0x77, 0x57]; // 'T' 't' 'I' 'i' 'w' 'W'

/// SwitchBot's Bluetooth SIG company identifier. Advertisement manufacturer
/// data is keyed by this; `btleplug` strips the 2-byte company ID, so the
/// value seen here starts at the 6-byte MAC address.
pub const SWITCHBOT_COMPANY_ID: u16 = 0x0969;

/// Service data UUIDs SwitchBot devices are known to advertise under: the
/// legacy 128-bit vendor UUID, and the Bluetooth SIG-assigned 16-bit 0xFD3D
/// used by newer firmware.
static SWITCHBOT_SERVICE_UUIDS: LazyLock<[Uuid; 2]> = LazyLock::new(|| {
    [
        Uuid::parse_str("cba20d00-224d-11e6-9fb8-0002a5d5c51b").unwrap(),
        Uuid::parse_str("0000fd3d-0000-1000-8000-00805f9b34fb").unwrap(),
    ]
});

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedReading {
    pub temperature: f64,
    pub humidity: f64,
    /// `None` for a reading recovered from manufacturer data alone — the
    /// battery byte only ever appears in service data.
    pub battery: Option<i64>,
}

pub fn is_switchbot_service_uuid(uuid: &Uuid) -> bool {
    SWITCHBOT_SERVICE_UUIDS.contains(uuid)
}

/// Parses a Meter / Meter Plus / Outdoor Meter reading from its service data
/// (and, for the Outdoor Meter, its manufacturer data). Returns `None` when
/// the advertisement is too short, reports a non-meter device type, or
/// carries no usable temperature/humidity payload.
pub fn parse_meter_advertisement(
    service_data: &[u8],
    manufacturer_data: Option<&[u8]>,
) -> Option<ParsedReading> {
    let &[byte0, _byte1, byte2, ..] = service_data else {
        return None;
    };

    let device_type = byte0 & 0x7F;
    if !METER_DEVICE_TYPES.contains(&device_type) {
        return None;
    }

    let battery = i64::from(byte2 & 0x7F);
    let (temperature, humidity) =
        decode_temperature_humidity(temperature_humidity_bytes(service_data, manufacturer_data)?);

    // A meter briefly broadcasts an all-zero payload while booting or
    // pairing; pySwitchbot discards it and so do we, to avoid storing a
    // spurious 0.0 °C / 0 % reading.
    if temperature == 0.0 && humidity == 0.0 && battery == 0 {
        return None;
    }

    Some(ParsedReading {
        temperature,
        humidity,
        battery: Some(battery),
    })
}

/// Parses a reading from a meter's manufacturer data alone, for meters seen
/// from too far to complete the `SCAN_REQ`/`SCAN_RSP` exchange their service
/// data rides in. The temperature/humidity payload sits at bytes 8..11 —
/// after the 6-byte MAC and a 2-byte header — exactly as in the Outdoor
/// Meter's manufacturer data.
///
/// Manufacturer data carries no device-type byte, so this cannot confirm
/// the sender is a meter rather than another SwitchBot device sharing
/// company ID `0x0969`. A payload that doesn't decode to a physically
/// plausible reading is rejected as the only available guard.
pub fn parse_meter_manufacturer_data(manufacturer_data: &[u8]) -> Option<ParsedReading> {
    let &[_, _, _, _, _, _, _, _, b8, b9, b10, ..] = manufacturer_data else {
        return None;
    };

    let (temperature, humidity) = decode_temperature_humidity([b8, b9, b10]);

    if temperature == 0.0 && humidity == 0.0 {
        return None;
    }
    if !(-40.0..=80.0).contains(&temperature) || !(0.0..=100.0).contains(&humidity) {
        return None;
    }

    Some(ParsedReading {
        temperature,
        humidity,
        battery: None,
    })
}

/// Decodes the 3-byte `[decimal, sign+integer, humidity]` temperature/
/// humidity payload every meter model shares, wherever it was found.
fn decode_temperature_humidity([decimal, sign_and_integer, humidity_byte]: [u8; 3]) -> (f64, f64) {
    let sign = if sign_and_integer & 0x80 == 0 {
        -1.0
    } else {
        1.0
    };
    let temperature =
        sign * (f64::from(sign_and_integer & 0x7F) + f64::from(decimal & 0x0F) / 10.0);
    let humidity = f64::from(humidity_byte & 0x7F);
    (temperature, humidity)
}

/// The 3 bytes that encode temperature and humidity. The base Meter and
/// Meter Plus carry them in service data bytes 3..6; the Outdoor Meter's
/// service data stops after the battery byte, so they come from manufacturer
/// data bytes 8..11 instead. Service data wins when present, keeping the
/// hardware-confirmed Meter Plus path untouched.
fn temperature_humidity_bytes(
    service_data: &[u8],
    manufacturer_data: Option<&[u8]>,
) -> Option<[u8; 3]> {
    if let &[_, _, _, b3, b4, b5, ..] = service_data {
        return Some([b3, b4, b5]);
    }
    if let Some(&[_, _, _, _, _, _, _, _, b8, b9, b10, ..]) = manufacturer_data {
        return Some([b8, b9, b10]);
    }
    None
}

/// Extracts the real BLE MAC the Outdoor Meter embeds in the first 6 bytes
/// of its manufacturer data (natural order, MSB first). Unlike the
/// link-layer address, CoreBluetooth does not mask this, so it yields a real
/// MAC even on macOS. Returns `None` for an all-zero / too-short payload.
pub fn mac_from_manufacturer_data(manufacturer_data: &[u8]) -> Option<[u8; 6]> {
    let mac: [u8; 6] = manufacturer_data.get(..6)?.try_into().ok()?;
    (mac != [0u8; 6]).then_some(mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_positive_temperature_normal_mode_reading() {
        // device_type=0x54 (normal), battery=90, decimal=5, sign+integer=23 (0x97), humidity=67 (0x43)
        let data = [0x54, 0x00, 0x5A, 0x05, 0x97, 0x43];

        let reading = parse_meter_advertisement(&data, None).expect("should parse");

        assert_eq!(reading.temperature, 23.5);
        assert_eq!(reading.humidity, 67.0);
        assert_eq!(reading.battery, Some(90));
    }

    #[test]
    fn parses_a_negative_temperature_add_mode_reading() {
        // device_type=0x74 (add mode), battery=15, decimal=3, subzero+integer=5 (0x05), humidity=40 (0x28)
        let data = [0x74, 0x00, 0x0F, 0x03, 0x05, 0x28];

        let reading = parse_meter_advertisement(&data, None).expect("should parse");

        assert_eq!(reading.temperature, -5.3);
        assert_eq!(reading.humidity, 40.0);
        assert_eq!(reading.battery, Some(15));
    }

    #[test]
    fn parses_a_real_meter_plus_advertisement() {
        // Captured from a real Meter Plus: device_type=0x69 ('i'), not
        // documented in the official meter.md (which only lists 'T'/'t').
        let data = [0x69, 0x00, 0x52, 0x08, 0x97, 0x41];

        let reading = parse_meter_advertisement(&data, None).expect("should parse");

        assert_eq!(reading.temperature, 23.8);
        assert_eq!(reading.humidity, 65.0);
        assert_eq!(reading.battery, Some(82));
    }

    #[test]
    fn parses_an_outdoor_meter_from_service_and_manufacturer_data() {
        // Outdoor Meter ('w'): 3-byte service data (device type + battery),
        // temperature/humidity in manufacturer data bytes 8..11. Fixture from
        // pySwitchbot's test_woiosensor_passive_and_active.
        let service_data = [0x77, 0x00, 0xE4];
        let manufacturer_data = [
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0xE0, 0x0F, 0x06, 0x98, 0x35, 0x00,
        ];

        let reading = parse_meter_advertisement(&service_data, Some(&manufacturer_data))
            .expect("should parse");

        assert_eq!(reading.temperature, 24.6);
        assert_eq!(reading.humidity, 53.0);
        assert_eq!(reading.battery, Some(100));
    }

    #[test]
    fn parses_a_meter_plus_reading_from_manufacturer_data_alone() {
        // Real ADV_IND manufacturer data (company ID 0x0969 stripped) from a
        // homelab Meter Plus, captured with btmon: 6-byte MAC, 2-byte header,
        // then the temperature/humidity payload at bytes 8..11. No service
        // data reached the adapter, so there is no battery reading.
        let manufacturer_data = [
            0xD2, 0x2E, 0x81, 0x06, 0x5C, 0x61, 0x6C, 0x0F, 0x02, 0x9C, 0x37,
        ];

        let reading = parse_meter_manufacturer_data(&manufacturer_data).expect("should parse");

        assert_eq!(reading.temperature, 28.2);
        assert_eq!(reading.humidity, 55.0);
        assert_eq!(reading.battery, None);
    }

    #[test]
    fn ignores_manufacturer_data_too_short_for_a_reading() {
        let manufacturer_data = [0xE4, 0x90, 0x00, 0xC6, 0x52, 0x41, 0x09, 0x0E];

        assert_eq!(parse_meter_manufacturer_data(&manufacturer_data), None);
    }

    #[test]
    fn ignores_manufacturer_data_that_decodes_to_an_implausible_reading() {
        // Bytes 8..11 decode to humidity 0x7F & 0x7F = 127 %, out of range —
        // the guard against another SwitchBot device sharing company 0x0969.
        let manufacturer_data = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x00, 0x00, 0x00, 0x9C, 0x7F,
        ];

        assert_eq!(parse_meter_manufacturer_data(&manufacturer_data), None);
    }

    #[test]
    fn ignores_an_all_zero_manufacturer_payload() {
        let manufacturer_data = [
            0xD2, 0x2E, 0x81, 0x06, 0x5C, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        assert_eq!(parse_meter_manufacturer_data(&manufacturer_data), None);
    }

    #[test]
    fn ignores_an_outdoor_meter_advertisement_without_manufacturer_data() {
        // The short service data alone carries no temperature/humidity.
        let service_data = [0x77, 0x00, 0xE4];

        assert_eq!(parse_meter_advertisement(&service_data, None), None);
    }

    #[test]
    fn ignores_an_all_zero_boot_payload() {
        let data = [0x54, 0x00, 0x00, 0x00, 0x00, 0x00];

        assert_eq!(parse_meter_advertisement(&data, None), None);
    }

    #[test]
    fn ignores_non_meter_device_types() {
        // 0x48 = 'H' = SwitchBot Bot, not a meter.
        let data = [0x48, 0x00, 0x5A, 0x05, 0x97, 0x43];

        assert_eq!(parse_meter_advertisement(&data, None), None);
    }

    #[test]
    fn ignores_short_payloads_with_no_manufacturer_data() {
        let data = [0x54, 0x00, 0x5A, 0x05, 0x97];

        assert_eq!(parse_meter_advertisement(&data, None), None);
    }

    #[test]
    fn extracts_the_mac_the_outdoor_meter_embeds_in_manufacturer_data() {
        let manufacturer_data = [
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0xE0, 0x0F, 0x06, 0x98, 0x35, 0x00,
        ];

        assert_eq!(
            mac_from_manufacturer_data(&manufacturer_data),
            Some([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF])
        );
        assert_eq!(mac_from_manufacturer_data(&[0x00; 12]), None);
        assert_eq!(mac_from_manufacturer_data(&[0x01, 0x02]), None);
    }

    #[test]
    fn recognizes_both_known_switchbot_service_uuids() {
        let legacy = Uuid::parse_str("cba20d00-224d-11e6-9fb8-0002a5d5c51b").unwrap();
        let fd3d = Uuid::parse_str("0000fd3d-0000-1000-8000-00805f9b34fb").unwrap();
        let unrelated = Uuid::parse_str("0000180d-0000-1000-8000-00805f9b34fb").unwrap();

        assert!(is_switchbot_service_uuid(&legacy));
        assert!(is_switchbot_service_uuid(&fd3d));
        assert!(!is_switchbot_service_uuid(&unrelated));
    }
}
