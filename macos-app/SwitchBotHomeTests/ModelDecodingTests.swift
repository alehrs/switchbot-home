import XCTest
@testable import SwitchBotHome

final class ModelDecodingTests: XCTestCase {
    func testDecodingADeviceWithAllOptionalFieldsNull() throws {
        let json = """
        {
            "device_id": "574AD03B-4384-2307-708E-08D01FD8174D",
            "label": null,
            "room": null,
            "blacklisted": false,
            "first_seen_at": "2026-08-21T10:37:08.973140Z",
            "last_seen_at": "2026-08-21T10:40:00Z"
        }
        """.data(using: .utf8)!

        let device = try BackendCoding.decoder.decode(Device.self, from: json)

        XCTAssertEqual(device.deviceID, "574AD03B-4384-2307-708E-08D01FD8174D")
        XCTAssertNil(device.label)
        XCTAssertNil(device.room)
        XCTAssertFalse(device.blacklisted)
    }

    func testDecodingADeviceWithLabelAndRoomSet() throws {
        let json = """
        {
            "device_id": "AA:BB",
            "label": "Cucina",
            "room": "Cucina",
            "blacklisted": false,
            "first_seen_at": "2026-08-21T10:37:08.973140Z",
            "last_seen_at": "2026-08-21T10:40:00.000000Z"
        }
        """.data(using: .utf8)!

        let device = try BackendCoding.decoder.decode(Device.self, from: json)

        XCTAssertEqual(device.label, "Cucina")
        XCTAssertEqual(device.room, "Cucina")
    }

    func testDecodingAReadingWithNullBattery() throws {
        let json = """
        {
            "id": 1,
            "device_id": "AA:BB",
            "temperature": 23.5,
            "humidity": 67.0,
            "battery": null,
            "recorded_at": "2026-08-21T10:37:08.973140Z"
        }
        """.data(using: .utf8)!

        let reading = try BackendCoding.decoder.decode(Reading.self, from: json)

        XCTAssertEqual(reading.temperature, 23.5)
        XCTAssertEqual(reading.humidity, 67.0)
        XCTAssertNil(reading.battery)
    }

    func testDecodingAnArrayOfReadings() throws {
        let json = """
        [
            {"id": 1, "device_id": "AA:BB", "temperature": 23.5, "humidity": 67.0, "battery": 90, "recorded_at": "2026-08-21T10:37:08.973140Z"},
            {"id": 2, "device_id": "AA:BB", "temperature": 23.6, "humidity": 66.0, "battery": 90, "recorded_at": "2026-08-21T10:38:08.973140Z"}
        ]
        """.data(using: .utf8)!

        let readings = try BackendCoding.decoder.decode([Reading].self, from: json)

        XCTAssertEqual(readings.count, 2)
        XCTAssertEqual(readings[1].battery, 90)
    }
}
