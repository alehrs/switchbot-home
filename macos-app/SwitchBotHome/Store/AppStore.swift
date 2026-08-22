import Foundation
import Observation

enum ConnectionState: Equatable {
    case connecting
    case online
    case offline(since: Date)
}

/// The single source of truth for what the UI shows. Owns the current
/// snapshots, derives room sections from them, and tracks connectivity.
/// Mutated only from `PollingService`; read by every view.
@MainActor
@Observable
final class AppStore {
    private(set) var snapshots: [DeviceSnapshot]
    private(set) var connectionState: ConnectionState = .connecting
    private(set) var lastRefreshedAt: Date?

    var sections: [(title: String, devices: [DeviceSnapshot])] {
        GroupingRules.sections(for: snapshots)
    }

    init() {
        snapshots = LocalCache.load()
    }

    /// Rebuilds every snapshot from a fresh device roster + each device's
    /// trailing-hour history. Called by the slow poll cycle.
    ///
    /// A slow cycle's own history fetch reflects a moment slightly before
    /// the cycle finishes (it can take a few seconds to fan out across
    /// devices), so a rebuilt snapshot's "latest" value could be older
    /// than one a fast cycle already applied in the meantime. Keeping
    /// whichever of the two is actually newer avoids the displayed value
    /// visibly regressing right after every slow refresh.
    func applyFullRefresh(devices: [Device], historiesByDeviceID: [String: [Reading]]) {
        let ranks = DeviceNumbering.ranks(for: devices)
        let previousByID = Dictionary(uniqueKeysWithValues: snapshots.map { ($0.id, $0) })
        snapshots = devices.map { device in
            var snapshot = buildSnapshot(device: device, rank: ranks[device.deviceID] ?? 0, readings: historiesByDeviceID[device.deviceID] ?? [])
            if let previous = previousByID[device.deviceID], isNewer(previous.latestRecordedAt, than: snapshot.latestRecordedAt) {
                snapshot.latestTemperature = previous.latestTemperature
                snapshot.latestHumidity = previous.latestHumidity
                snapshot.latestBattery = previous.latestBattery
                snapshot.latestRecordedAt = previous.latestRecordedAt
            }
            return snapshot
        }
        LocalCache.save(snapshots)
    }

    private func isNewer(_ candidate: Date?, than reference: Date?) -> Bool {
        guard let candidate else { return false }
        guard let reference else { return true }
        return candidate > reference
    }

    /// Updates just the "current value" fields from `/readings/latest`.
    /// Never adds/removes devices — that only happens on a full refresh.
    /// Called by the fast poll cycle.
    func applyFastUpdate(_ readings: [Reading]) {
        var byID = Dictionary(uniqueKeysWithValues: snapshots.map { ($0.id, $0) })
        for reading in readings {
            guard var snapshot = byID[reading.deviceID] else { continue }
            snapshot.latestTemperature = reading.temperature
            snapshot.latestHumidity = reading.humidity
            snapshot.latestBattery = reading.battery
            snapshot.latestRecordedAt = reading.recordedAt
            byID[reading.deviceID] = snapshot
        }
        snapshots = snapshots.map { byID[$0.id] ?? $0 }
        LocalCache.save(snapshots)
    }

    func markOnline() {
        connectionState = .online
        lastRefreshedAt = Date()
    }

    func markOffline() {
        if case .offline = connectionState { return }
        connectionState = .offline(since: Date())
    }

    private func buildSnapshot(device: Device, rank: Int, readings: [Reading]) -> DeviceSnapshot {
        let sorted = readings.sorted { $0.recordedAt < $1.recordedAt }
        let latest = sorted.last
        return DeviceSnapshot(
            device: device,
            rank: rank,
            latestTemperature: latest?.temperature,
            latestHumidity: latest?.humidity,
            latestBattery: latest?.battery,
            latestRecordedAt: latest?.recordedAt,
            temperatureAverage1h: average(sorted.map(\.temperature)),
            humidityAverage1h: average(sorted.map(\.humidity)),
            temperatureTrend: TrendCalculator.calculate(
                readings: sorted,
                keyPath: \.temperature,
                deadband: TrendCalculator.temperatureDeadband
            ),
            humidityTrend: TrendCalculator.calculate(
                readings: sorted,
                keyPath: \.humidity,
                deadband: TrendCalculator.humidityDeadband
            )
        )
    }

    private func average(_ values: [Double]) -> Double? {
        guard !values.isEmpty else { return nil }
        return values.reduce(0, +) / Double(values.count)
    }
}
