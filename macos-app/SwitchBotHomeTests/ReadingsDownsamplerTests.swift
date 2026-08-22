import XCTest
@testable import SwitchBotHome

final class ReadingsDownsamplerTests: XCTestCase {
    private func reading(_ id: Int, temperature: Double, minutesFromEpoch: Int) -> Reading {
        Reading(
            id: id,
            deviceID: "AA:BB",
            temperature: temperature,
            humidity: 50,
            battery: nil,
            recordedAt: Date(timeIntervalSince1970: Double(minutesFromEpoch * 60))
        )
    }

    func testArraysAtOrUnderTheLimitPassThroughUnchanged() {
        let readings = (0..<10).map { reading($0, temperature: Double($0), minutesFromEpoch: $0) }

        XCTAssertEqual(ReadingsDownsampler.downsample(readings, maxPoints: 10), readings)
        XCTAssertEqual(ReadingsDownsampler.downsample(readings, maxPoints: 20), readings)
    }

    func testArraysOverTheLimitAreReducedToAtMostMaxPoints() {
        let readings = (0..<1000).map { reading($0, temperature: Double($0), minutesFromEpoch: $0) }

        let result = ReadingsDownsampler.downsample(readings, maxPoints: 100)

        XCTAssertLessThanOrEqual(result.count, 100)
        XCTAssertGreaterThan(result.count, 0)
    }

    func testBucketsAreAveragedNotJustSampled() {
        // 4 readings, maxPoints 2 -> two buckets of 2: [10,20] and [30,40].
        let readings = [
            reading(1, temperature: 10, minutesFromEpoch: 0),
            reading(2, temperature: 20, minutesFromEpoch: 1),
            reading(3, temperature: 30, minutesFromEpoch: 2),
            reading(4, temperature: 40, minutesFromEpoch: 3),
        ]

        let result = ReadingsDownsampler.downsample(readings, maxPoints: 2)

        XCTAssertEqual(result.count, 2)
        XCTAssertEqual(result[0].temperature, 15.0, accuracy: 0.001)
        XCTAssertEqual(result[1].temperature, 35.0, accuracy: 0.001)
    }

    func testResultStaysInChronologicalOrder() {
        let readings = (0..<500).map { reading($0, temperature: Double($0), minutesFromEpoch: $0) }

        let result = ReadingsDownsampler.downsample(readings, maxPoints: 50)

        let timestamps = result.map(\.recordedAt)
        XCTAssertEqual(timestamps, timestamps.sorted())
    }

    func testEmptyInputStaysEmpty() {
        XCTAssertEqual(ReadingsDownsampler.downsample([], maxPoints: 100), [])
    }
}
