import XCTest
@testable import SwitchBotHome

final class DeviceNumberingTests: XCTestCase {
    private func device(
        id: String,
        firstSeenAt: Date,
        label: String? = nil,
        blacklisted: Bool = false
    ) -> Device {
        Device(
            deviceID: id,
            label: label,
            room: nil,
            blacklisted: blacklisted,
            firstSeenAt: firstSeenAt,
            lastSeenAt: firstSeenAt
        )
    }

    func testRanksAreAssignedByDiscoveryOrder() {
        let now = Date()
        let devices = [
            device(id: "c", firstSeenAt: now.addingTimeInterval(20)),
            device(id: "a", firstSeenAt: now),
            device(id: "b", firstSeenAt: now.addingTimeInterval(10)),
        ]

        let ranks = DeviceNumbering.ranks(for: devices)

        XCTAssertEqual(ranks["a"], 1)
        XCTAssertEqual(ranks["b"], 2)
        XCTAssertEqual(ranks["c"], 3)
    }

    func testLabelingAMidRankedDeviceLeavesAGapRatherThanRenumbering() {
        let now = Date()
        let devices = [
            device(id: "a", firstSeenAt: now),
            device(id: "b", firstSeenAt: now.addingTimeInterval(10), label: "Cucina"),
            device(id: "c", firstSeenAt: now.addingTimeInterval(20)),
        ]

        let ranks = DeviceNumbering.ranks(for: devices)

        // "b" (rank 2) now has a label, but "c" must still be rank 3, not
        // renumbered down to 2.
        XCTAssertEqual(ranks["a"], 1)
        XCTAssertEqual(ranks["b"], 2)
        XCTAssertEqual(ranks["c"], 3)
    }

    func testANewlyDiscoveredDeviceAlwaysGetsTheHighestRank() {
        let now = Date()
        var devices = [
            device(id: "a", firstSeenAt: now),
            device(id: "b", firstSeenAt: now.addingTimeInterval(10)),
        ]
        devices.append(device(id: "new", firstSeenAt: now.addingTimeInterval(20)))

        let ranks = DeviceNumbering.ranks(for: devices)

        XCTAssertEqual(ranks["new"], 3)
    }

    func testRanksSurviveBlacklistToggling() {
        let now = Date()
        let devices = [
            device(id: "a", firstSeenAt: now),
            device(id: "b", firstSeenAt: now.addingTimeInterval(10), blacklisted: true),
        ]

        let ranks = DeviceNumbering.ranks(for: devices)

        XCTAssertEqual(ranks["a"], 1)
        XCTAssertEqual(ranks["b"], 2)
    }
}
