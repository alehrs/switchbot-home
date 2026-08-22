import Foundation

/// A calendar day's `[start, end)` boundaries — midnight to the following
/// midnight, in the user's local calendar. Used to query a day's worth of
/// readings (`GET /devices/{id}/readings?from=&to=`) and to fix a chart's
/// x-axis domain so different days render on a consistent scale.
///
/// Deliberately calendar-based (`Calendar.date(byAdding:.day...)`), not
/// raw 86,400-second arithmetic, so navigating across a Daylight Saving
/// Time transition still lands on the correct local midnight.
struct DayRange: Equatable {
    let start: Date
    let end: Date

    static func containing(_ date: Date, calendar: Calendar = .current) -> DayRange {
        let start = calendar.startOfDay(for: date)
        let end = calendar.date(byAdding: .day, value: 1, to: start) ?? start
        return DayRange(start: start, end: end)
    }

    func advanced(by days: Int, calendar: Calendar = .current) -> DayRange {
        let newStart = calendar.date(byAdding: .day, value: days, to: start) ?? start
        return DayRange.containing(newStart, calendar: calendar)
    }

    var isToday: Bool {
        Calendar.current.isDateInToday(start)
    }
}
