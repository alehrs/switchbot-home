import AppKit
import Foundation

/// Runs the two independent poll cadences described in
/// `docs/specs/macos-app.md` §6: a fast cycle for current values, and a
/// slow cycle (fanning out per-device history requests — there's no batch
/// history endpoint) for the 1h average/trend. A failed cycle marks the
/// store offline but never clears existing data.
///
/// `@MainActor`: `lastFullRefreshAt` is written from the slow cycle and
/// read from `refreshIfStale` (called from SwiftUI's `.onAppear` and the
/// wake-notification handler, both on the main thread) — isolating the
/// whole class avoids an unsynchronized cross-thread var access. The
/// network awaits inside still suspend properly, so this doesn't block
/// the main thread; polling every 30s/3min is not performance-sensitive
/// enough to need a background executor.
@MainActor
final class PollingService {
    private static let fastInterval: Duration = .seconds(30)
    private static let slowInterval: Duration = .seconds(180)
    private static let historyWindow: TimeInterval = 60 * 60
    // `nonisolated`: used as a default parameter value below, which is
    // evaluated in a nonisolated context regardless of the class's own
    // isolation — safe here since it's just an immutable constant.
    private nonisolated static let staleThreshold: TimeInterval = 10

    private let store: AppStore
    private let client: APIClient

    private var fastTask: Task<Void, Never>?
    private var slowTask: Task<Void, Never>?
    private var wakeObserver: NSObjectProtocol?
    private var lastFullRefreshAt: Date?

    init(store: AppStore, client: APIClient) {
        self.store = store
        self.client = client
    }

    func start() {
        guard fastTask == nil else { return }

        fastTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.runFastCycle()
                try? await Task.sleep(for: Self.fastInterval)
            }
        }
        slowTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.runSlowCycle()
                try? await Task.sleep(for: Self.slowInterval)
            }
        }

        wakeObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.refreshIfStale(staleAfter: 0)
            }
        }
    }

    func stop() {
        fastTask?.cancel()
        slowTask?.cancel()
        if let wakeObserver {
            NSWorkspace.shared.notificationCenter.removeObserver(wakeObserver)
        }
    }

    /// Called when the popover opens: refreshes immediately unless the
    /// last full refresh was very recent, so opening it repeatedly in
    /// quick succession doesn't hammer the backend.
    func refreshIfStale(staleAfter: TimeInterval = staleThreshold) {
        if let last = lastFullRefreshAt, Date().timeIntervalSince(last) < staleAfter {
            return
        }
        Task { await runFastCycle() }
        Task { await runSlowCycle() }
    }

    private func runFastCycle() async {
        do {
            let readings = try await client.fetchLatestReadings()
            store.applyFastUpdate(readings)
            store.markOnline()
        } catch {
            store.markOffline()
        }
    }

    private func runSlowCycle() async {
        do {
            let devices = try await client.fetchDevices()
            let now = Date()
            let windowStart = now.addingTimeInterval(-Self.historyWindow)

            // Non-throwing: one device's history fetch failing (a
            // transient blip, a device that just dropped off BLE range)
            // must not discard every other device's successfully-fetched
            // history for this cycle, nor mark the whole app offline —
            // that failed device just falls back to "insufficient data"
            // for this cycle instead. Connectivity to the backend overall
            // is judged by the `fetchDevices()` call above, not by this.
            var histories: [String: [Reading]] = [:]
            await withTaskGroup(of: (String, [Reading]).self) { group in
                for device in devices where !device.blacklisted {
                    group.addTask {
                        do {
                            let readings = try await self.client.fetchReadings(deviceID: device.deviceID, from: windowStart, to: now)
                            return (device.deviceID, readings)
                        } catch {
                            return (device.deviceID, [])
                        }
                    }
                }
                for await (deviceID, readings) in group {
                    histories[deviceID] = readings
                }
            }

            store.applyFullRefresh(devices: devices, historiesByDeviceID: histories)
            store.markOnline()
            lastFullRefreshAt = Date()
        } catch {
            store.markOffline()
        }
    }
}
