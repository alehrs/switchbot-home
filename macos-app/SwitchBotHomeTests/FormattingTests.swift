import XCTest
@testable import SwitchBotHome

/// Regression coverage for a real bug: `String(format: "%.1f\(suffix)",
/// value)` silently drops a "%" suffix (and logs a console error on every
/// call) because printf-style format strings treat "%" as a specifier
/// prefix. These tests fail loudly if that pattern is ever reintroduced.
final class FormattingTests: XCTestCase {
    func testAPercentSuffixIsIncludedInTheOutput() {
        XCTAssertEqual(Formatting.number(67.0, suffix: "%"), "67.0%")
    }

    func testADegreeSuffixIsIncludedInTheOutput() {
        XCTAssertEqual(Formatting.number(23.0, suffix: "°C"), "23.0°C")
    }

    func testCustomDecimalCount() {
        XCTAssertEqual(Formatting.number(67.0, decimals: 0, suffix: "%"), "67%")
    }

    func testNoSuffix() {
        XCTAssertEqual(Formatting.number(1.26, suffix: ""), "1.3")
    }

    func testRoundsToTheRequestedDecimalPlaces() {
        XCTAssertEqual(Formatting.number(23.456, suffix: "°C"), "23.5°C")
    }
}
