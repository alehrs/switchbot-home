import XCTest
@testable import SwitchBotHome

/// `Trend`'s `Codable` conformance is hand-written (not synthesized) so
/// `LocalCache` can persist it — exactly the kind of code where a typo in
/// `CodingKeys`/`Kind` would silently break the "instant launch UI"
/// feature (docs/specs/macos-app.md §8) without any other test noticing.
final class TrendCodableTests: XCTestCase {
    private func roundTrip(_ trend: Trend) throws -> Trend {
        let data = try JSONEncoder().encode(trend)
        return try JSONDecoder().decode(Trend.self, from: data)
    }

    func testRisingRoundTrips() throws {
        XCTAssertEqual(try roundTrip(.rising(delta: 1.5)), .rising(delta: 1.5))
    }

    func testFallingRoundTrips() throws {
        XCTAssertEqual(try roundTrip(.falling(delta: -2.5)), .falling(delta: -2.5))
    }

    func testFlatRoundTrips() throws {
        XCTAssertEqual(try roundTrip(.flat), .flat)
    }

    func testInsufficientDataRoundTrips() throws {
        XCTAssertEqual(try roundTrip(.insufficientData), .insufficientData)
    }

    func testADeviceSnapshotWithEveryFieldRoundTrips() throws {
        let snapshot = DeviceSnapshot(
            device: Device(
                deviceID: "AA:BB",
                label: "Cucina",
                room: "Cucina",
                blacklisted: false,
                firstSeenAt: Date(timeIntervalSince1970: 1_000),
                lastSeenAt: Date(timeIntervalSince1970: 2_000)
            ),
            rank: 1,
            latestTemperature: 23.5,
            latestHumidity: 67.0,
            latestBattery: 90,
            latestRecordedAt: Date(timeIntervalSince1970: 2_000),
            temperatureAverage1h: 23.0,
            humidityAverage1h: 65.0,
            temperatureTrend: .rising(delta: 0.5),
            humidityTrend: .flat
        )

        let data = try JSONEncoder().encode(snapshot)
        let decoded = try JSONDecoder().decode(DeviceSnapshot.self, from: data)

        XCTAssertEqual(decoded, snapshot)
    }
}
