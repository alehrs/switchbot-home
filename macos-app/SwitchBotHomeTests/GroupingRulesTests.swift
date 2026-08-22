import XCTest
@testable import SwitchBotHome

final class GroupingRulesTests: XCTestCase {
    private func snapshot(
        id: String,
        room: String?,
        firstSeenAt: Date,
        blacklisted: Bool = false
    ) -> DeviceSnapshot {
        DeviceSnapshot(
            device: Device(
                deviceID: id,
                label: nil,
                room: room,
                blacklisted: blacklisted,
                firstSeenAt: firstSeenAt,
                lastSeenAt: firstSeenAt
            ),
            rank: 1,
            latestTemperature: nil,
            latestHumidity: nil,
            latestBattery: nil,
            latestRecordedAt: nil,
            temperatureAverage1h: nil,
            humidityAverage1h: nil,
            temperatureTrend: .insufficientData,
            humidityTrend: .insufficientData
        )
    }

    func testNilEmptyAndWhitespaceRoomsAllMapToUngrouped() {
        let now = Date()
        let snapshots = [
            snapshot(id: "a", room: nil, firstSeenAt: now),
            snapshot(id: "b", room: "", firstSeenAt: now),
            snapshot(id: "c", room: "   ", firstSeenAt: now),
        ]

        let sections = GroupingRules.sections(for: snapshots)

        XCTAssertEqual(sections.count, 1)
        XCTAssertEqual(sections[0].title, GroupingRules.ungroupedTitle)
        XCTAssertEqual(sections[0].devices.count, 3)
    }

    func testUngroupedSectionAlwaysSortsLast() {
        let now = Date()
        let snapshots = [
            snapshot(id: "a", room: nil, firstSeenAt: now),
            snapshot(id: "b", room: "Zzz Room", firstSeenAt: now),
            snapshot(id: "c", room: "Aaa Room", firstSeenAt: now),
        ]

        let sections = GroupingRules.sections(for: snapshots)

        XCTAssertEqual(sections.map(\.title), ["Aaa Room", "Zzz Room", GroupingRules.ungroupedTitle])
    }

    func testWithinGroupOrderMatchesFirstSeenAt() {
        let now = Date()
        let snapshots = [
            snapshot(id: "later", room: "Cucina", firstSeenAt: now.addingTimeInterval(10)),
            snapshot(id: "earlier", room: "Cucina", firstSeenAt: now),
        ]

        let sections = GroupingRules.sections(for: snapshots)

        XCTAssertEqual(sections[0].devices.map(\.id), ["earlier", "later"])
    }

    func testBlacklistedDevicesAreNeverShown() {
        let now = Date()
        let snapshots = [
            snapshot(id: "visible", room: "Cucina", firstSeenAt: now),
            snapshot(id: "hidden", room: "Cucina", firstSeenAt: now, blacklisted: true),
        ]

        let sections = GroupingRules.sections(for: snapshots)

        XCTAssertEqual(sections[0].devices.map(\.id), ["visible"])
    }
}
