import XCTest
@testable import SwitchBotHome

final class DayRangeTests: XCTestCase {
    private var utc: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        return calendar
    }

    func testContainingReturnsMidnightToMidnight() {
        let noon = utc.date(from: DateComponents(year: 2026, month: 8, day: 21, hour: 12))!

        let range = DayRange.containing(noon, calendar: utc)

        XCTAssertEqual(range.start, utc.date(from: DateComponents(year: 2026, month: 8, day: 21)))
        XCTAssertEqual(range.end, utc.date(from: DateComponents(year: 2026, month: 8, day: 22)))
    }

    func testAdvancedByOneMovesToTheNextDay() {
        let range = DayRange.containing(
            utc.date(from: DateComponents(year: 2026, month: 8, day: 21, hour: 12))!,
            calendar: utc
        )

        let next = range.advanced(by: 1, calendar: utc)

        XCTAssertEqual(next.start, utc.date(from: DateComponents(year: 2026, month: 8, day: 22)))
    }

    func testAdvancedByNegativeOneMovesToThePreviousDay() {
        let range = DayRange.containing(
            utc.date(from: DateComponents(year: 2026, month: 8, day: 21, hour: 12))!,
            calendar: utc
        )

        let previous = range.advanced(by: -1, calendar: utc)

        XCTAssertEqual(previous.start, utc.date(from: DateComponents(year: 2026, month: 8, day: 20)))
    }

    func testAdvancingCrossesAMonthBoundaryCorrectly() {
        let range = DayRange.containing(
            utc.date(from: DateComponents(year: 2026, month: 8, day: 31, hour: 12))!,
            calendar: utc
        )

        let next = range.advanced(by: 1, calendar: utc)

        XCTAssertEqual(next.start, utc.date(from: DateComponents(year: 2026, month: 9, day: 1)))
    }

    func testIsTodayReflectsTheRealCurrentDay() {
        let today = DayRange.containing(Date())
        let yesterday = today.advanced(by: -1)

        XCTAssertTrue(today.isToday)
        XCTAssertFalse(yesterday.isToday)
    }
}
