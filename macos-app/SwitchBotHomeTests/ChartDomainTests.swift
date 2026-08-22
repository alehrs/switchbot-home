import XCTest
@testable import SwitchBotHome

final class ChartDomainTests: XCTestCase {
    func testExpandsAroundTheDataRangeWithPadding() {
        let domain = ChartDomain.range(for: [10, 20])

        // (20-10)*0.1 = 1, which is also minPadding here, so padding = 1.
        XCTAssertEqual(domain.lowerBound, 9, accuracy: 0.001)
        XCTAssertEqual(domain.upperBound, 21, accuracy: 0.001)
    }

    func testHandlesANegativeMinimumTemperature() {
        let domain = ChartDomain.range(for: [-5.0, 10.0])

        XCTAssertLessThan(domain.lowerBound, -5.0)
        XCTAssertGreaterThan(domain.upperBound, 10.0)
    }

    func testHandlesValuesAboveFortyDegrees() {
        let domain = ChartDomain.range(for: [38.0, 45.0])

        XCTAssertGreaterThanOrEqual(domain.upperBound, 45.0)
    }

    func testAFlatSingleValueSeriesStillGetsVisibleHeadroom() {
        let domain = ChartDomain.range(for: [23.0], minPadding: 1.0)

        XCTAssertEqual(domain.lowerBound, 22.0, accuracy: 0.001)
        XCTAssertEqual(domain.upperBound, 24.0, accuracy: 0.001)
    }

    func testEmptyInputFallsBackToTheProvidedDefault() {
        let domain = ChartDomain.range(for: [], fallback: -10...10)

        XCTAssertEqual(domain, -10...10)
    }
}
