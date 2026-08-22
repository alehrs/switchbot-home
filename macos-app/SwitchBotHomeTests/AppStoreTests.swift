import XCTest
@testable import SwitchBotHome

@MainActor
final class AppStoreTests: XCTestCase {
    private func device(id: String, firstSeenAt: Date) -> Device {
        Device(deviceID: id, label: nil, room: nil, blacklisted: false, firstSeenAt: firstSeenAt, lastSeenAt: firstSeenAt)
    }

    private func reading(deviceID: String, temperature: Double, recordedAt: Date) -> Reading {
        Reading(id: 1, deviceID: deviceID, temperature: temperature, humidity: 50, battery: nil, recordedAt: recordedAt)
    }

    func testFullRefreshDoesNotRegressALatestValueAlreadyUpdatedByAFasterCycle() {
        let store = AppStore()
        let now = Date()
        let device = device(id: "AA:BB", firstSeenAt: now)

        // The slow cycle "started" its history fetch a bit in the past...
        store.applyFullRefresh(
            devices: [device],
            historiesByDeviceID: ["AA:BB": [reading(deviceID: "AA:BB", temperature: 20.0, recordedAt: now.addingTimeInterval(-5))]]
        )
        // ...but a fast cycle already delivered a newer value while it was in flight.
        store.applyFastUpdate([reading(deviceID: "AA:BB", temperature: 21.0, recordedAt: now)])

        // A second slow refresh completes with the SAME (now stale) history
        // it originally fetched — it must not overwrite the newer fast value.
        store.applyFullRefresh(
            devices: [device],
            historiesByDeviceID: ["AA:BB": [reading(deviceID: "AA:BB", temperature: 20.0, recordedAt: now.addingTimeInterval(-5))]]
        )

        XCTAssertEqual(store.snapshots.first?.latestTemperature, 21.0)
    }

    func testFullRefreshDoesApplyAGenuinelyNewerValue() {
        let store = AppStore()
        let now = Date()
        let device = device(id: "AA:BB", firstSeenAt: now)

        store.applyFullRefresh(
            devices: [device],
            historiesByDeviceID: ["AA:BB": [reading(deviceID: "AA:BB", temperature: 20.0, recordedAt: now)]]
        )
        store.applyFullRefresh(
            devices: [device],
            historiesByDeviceID: ["AA:BB": [reading(deviceID: "AA:BB", temperature: 22.0, recordedAt: now.addingTimeInterval(60))]]
        )

        XCTAssertEqual(store.snapshots.first?.latestTemperature, 22.0)
    }
}
