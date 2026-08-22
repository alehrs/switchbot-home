//! Parses SwitchBot Meter / Meter Plus BLE advertisement service data.
//!
//! Format reference: OpenWonderLabs' official BLE API docs
//! (SwitchBotAPI-BLE, devicetypes/meter.md, "New Broadcast Message") for the
//! byte layout, and pySwitchbot's device type table for the Meter Plus
//! device type byte (0x69/0x49), which isn't in the official doc.
//! Meter and Meter Plus share the same 6-byte service data layout.

use std::sync::LazyLock;

use uuid::Uuid;

/// Device type bytes (bits 6:0 of service data byte 0) that use this same
/// 6-byte layout: the base MeterTH in its two broadcast modes ('T'/'t'),
/// and Meter Plus in its two broadcast modes ('I'/'i'). Confirmed against
/// a real Meter Plus device, whose type byte is 0x69 ('i') — not
/// documented in OpenWonderLabs' official meter.md, but consistent with
/// pySwitchbot's device type table (used by Home Assistant), which maps
/// both 'T'/'t' and 'I'/'i' to the same parser.
const METER_DEVICE_TYPES: [u8; 4] = [0x54, 0x74, 0x49, 0x69]; // 'T' 't' 'I' 'i'

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
    pub battery: i64,
}

pub fn is_switchbot_service_uuid(uuid: &Uuid) -> bool {
    SWITCHBOT_SERVICE_UUIDS.contains(uuid)
}

/// Parses a Meter/Meter Plus service data payload, or returns `None` if the
/// data is too short or reports a device type other than a meter.
pub fn parse_meter_advertisement(service_data: &[u8]) -> Option<ParsedReading> {
    let &[byte0, _byte1, byte2, byte3, byte4, byte5, ..] = service_data else {
        return None;
    };

    let device_type = byte0 & 0x7F;
    if !METER_DEVICE_TYPES.contains(&device_type) {
        return None;
    }

    let battery = i64::from(byte2 & 0x7F);
    let temperature_decimal = f64::from(byte3 & 0x0F);
    let temperature_sign = if byte4 & 0x80 != 0 { 1.0 } else { -1.0 };
    let temperature_integer = f64::from(byte4 & 0x7F);
    let temperature = temperature_sign * (temperature_integer + temperature_decimal / 10.0);
    let humidity = f64::from(byte5 & 0x7F);

    Some(ParsedReading {
        temperature,
        humidity,
        battery,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_positive_temperature_normal_mode_reading() {
        // device_type=0x54 (normal), battery=90, decimal=5, sign+integer=23 (0x97), humidity=67 (0x43)
        let data = [0x54, 0x00, 0x5A, 0x05, 0x97, 0x43];

        let reading = parse_meter_advertisement(&data).expect("should parse");

        assert_eq!(reading.temperature, 23.5);
        assert_eq!(reading.humidity, 67.0);
        assert_eq!(reading.battery, 90);
    }

    #[test]
    fn parses_a_negative_temperature_add_mode_reading() {
        // device_type=0x74 (add mode), battery=15, decimal=3, subzero+integer=5 (0x05), humidity=40 (0x28)
        let data = [0x74, 0x00, 0x0F, 0x03, 0x05, 0x28];

        let reading = parse_meter_advertisement(&data).expect("should parse");

        assert_eq!(reading.temperature, -5.3);
        assert_eq!(reading.humidity, 40.0);
        assert_eq!(reading.battery, 15);
    }

    #[test]
    fn parses_a_real_meter_plus_advertisement() {
        // Captured from a real Meter Plus: device_type=0x69 ('i'), not
        // documented in the official meter.md (which only lists 'T'/'t').
        let data = [0x69, 0x00, 0x52, 0x08, 0x97, 0x41];

        let reading = parse_meter_advertisement(&data).expect("should parse");

        assert_eq!(reading.temperature, 23.8);
        assert_eq!(reading.humidity, 65.0);
        assert_eq!(reading.battery, 82);
    }

    #[test]
    fn ignores_non_meter_device_types() {
        // 0x48 = 'H' = SwitchBot Bot, not a meter.
        let data = [0x48, 0x00, 0x5A, 0x05, 0x97, 0x43];

        assert_eq!(parse_meter_advertisement(&data), None);
    }

    #[test]
    fn ignores_payloads_shorter_than_six_bytes() {
        let data = [0x54, 0x00, 0x5A, 0x05, 0x97];

        assert_eq!(parse_meter_advertisement(&data), None);
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
