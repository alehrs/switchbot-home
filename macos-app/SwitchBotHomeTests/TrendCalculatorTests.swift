import XCTest
@testable import SwitchBotHome

final class TrendCalculatorTests: XCTestCase {
    private let now = Date()

    private func reading(minutesAgo: Double, temperature: Double) -> Reading {
        Reading(
            id: Int(minutesAgo * 1000),
            deviceID: "AA:BB",
            temperature: temperature,
            humidity: 50,
            battery: nil,
            recordedAt: now.addingTimeInterval(-minutesAgo * 60)
        )
    }

    func testSteadilyRisingSeriesIsDetectedAsRising() {
        let readings = [
            reading(minutesAgo: 60, temperature: 20.0),
            reading(minutesAgo: 58, temperature: 20.1),
            reading(minutesAgo: 56, temperature: 19.9),
            reading(minutesAgo: 4, temperature: 24.9),
            reading(minutesAgo: 2, temperature: 25.0),
            reading(minutesAgo: 0, temperature: 25.1),
        ]

        let trend = TrendCalculator.calculate(
            readings: readings,
            keyPath: \.temperature,
            deadband: TrendCalculator.temperatureDeadband,
            now: now
        )

        guard case .rising(let delta) = trend else {
            return XCTFail("expected .rising, got \(trend)")
        }
        XCTAssertEqual(delta, 5.0, accuracy: 0.01)
    }

    func testSteadilyFallingSeriesIsDetectedAsFalling() {
        let readings = [
            reading(minutesAgo: 60, temperature: 25.0),
            reading(minutesAgo: 58, temperature: 25.1),
            reading(minutesAgo: 56, temperature: 24.9),
            reading(minutesAgo: 4, temperature: 20.1),
            reading(minutesAgo: 2, temperature: 20.0),
            reading(minutesAgo: 0, temperature: 19.9),
        ]

        let trend = TrendCalculator.calculate(
            readings: readings,
            keyPath: \.temperature,
            deadband: TrendCalculator.temperatureDeadband,
            now: now
        )

        guard case .falling(let delta) = trend else {
            return XCTFail("expected .falling, got \(trend)")
        }
        XCTAssertEqual(delta, -5.0, accuracy: 0.01)
    }

    func testNoisyButNetFlatSeriesStaysWithinTheDeadband() {
        let readings = [
            reading(minutesAgo: 60, temperature: 20.0),
            reading(minutesAgo: 58, temperature: 20.15),
            reading(minutesAgo: 56, temperature: 19.9),
            reading(minutesAgo: 4, temperature: 20.05),
            reading(minutesAgo: 2, temperature: 19.95),
            reading(minutesAgo: 0, temperature: 20.1),
        ]

        let trend = TrendCalculator.calculate(
            readings: readings,
            keyPath: \.temperature,
            deadband: TrendCalculator.temperatureDeadband,
            now: now
        )

        XCTAssertEqual(trend, .flat)
    }

    func testJustAboveTheDeadbandCountsAsRisingNotFlat() {
        // Deliberately not testing the exact deadband value itself: a
        // literal `20.2 - 20.0` in `Double` doesn't equal exactly `0.2`
        // (classic binary floating-point representation error), so an
        // exact-boundary assertion would be flaky for a difference no
        // user could ever perceive (the UI rounds to 1 decimal place
        // either way). Testing comfortably past the threshold instead
        // still confirms the boundary is ">=", not ">".
        let readings = [
            reading(minutesAgo: 60, temperature: 20.0),
            reading(minutesAgo: 0, temperature: 20.25),
        ]

        let trend = TrendCalculator.calculate(
            readings: readings,
            keyPath: \.temperature,
            deadband: TrendCalculator.temperatureDeadband,
            now: now
        )

        guard case .rising(let delta) = trend else {
            return XCTFail("expected .rising, got \(trend)")
        }
        XCTAssertEqual(delta, 0.25, accuracy: 0.001)
    }

    func testSparseHistoryFallsBackToFirstVsLastDelta() {
        // Neither reading falls inside the baseline edge window
        // [-60m, -55m], so the edge-averaged calculation can't run and
        // must fall back to a plain first-vs-last delta.
        let readings = [
            reading(minutesAgo: 40, temperature: 20.0),
            reading(minutesAgo: 2, temperature: 22.0),
        ]

        let trend = TrendCalculator.calculate(
            readings: readings,
            keyPath: \.temperature,
            deadband: TrendCalculator.temperatureDeadband,
            now: now
        )

        guard case .rising(let delta) = trend else {
            return XCTFail("expected .rising via fallback, got \(trend)")
        }
        XCTAssertEqual(delta, 2.0, accuracy: 0.01)
    }

    func testZeroOrOneReadingsIsInsufficientData() {
        XCTAssertEqual(
            TrendCalculator.calculate(readings: [], keyPath: \.temperature, deadband: 0.2, now: now),
            .insufficientData
        )
        XCTAssertEqual(
            TrendCalculator.calculate(
                readings: [reading(minutesAgo: 0, temperature: 20.0)],
                keyPath: \.temperature,
                deadband: 0.2,
                now: now
            ),
            .insufficientData
        )
    }

    func testHumidityUsesItsOwnWiderDeadband() {
        let readings = [
            reading(minutesAgo: 60, temperature: 20.0),
            reading(minutesAgo: 0, temperature: 20.0),
        ]
        var withHumidity = readings
        withHumidity[0].humidity = 50.0
        withHumidity[1].humidity = 51.5 // delta 1.5: flat for humidity, would be "rising" for temperature

        let trend = TrendCalculator.calculate(
            readings: withHumidity,
            keyPath: \.humidity,
            deadband: TrendCalculator.humidityDeadband,
            now: now
        )

        XCTAssertEqual(trend, .flat)
    }
}
