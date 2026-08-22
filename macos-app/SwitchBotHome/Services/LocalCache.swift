import Foundation

/// Persists the last-known snapshots to disk so the popover isn't empty
/// before the first poll cycle completes on launch. Best-effort: read/
/// write failures are swallowed (a cache miss just means an empty popover
/// until the first successful poll, not a crash).
enum LocalCache {
    private static var fileURL: URL? {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return nil
        }
        let directory = appSupport.appendingPathComponent("SwitchBotHome", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory.appendingPathComponent("last-snapshot.json")
    }

    static func save(_ snapshots: [DeviceSnapshot]) {
        guard let fileURL else { return }
        guard let data = try? JSONEncoder().encode(snapshots) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }

    static func load() -> [DeviceSnapshot] {
        guard let fileURL, let data = try? Data(contentsOf: fileURL) else { return [] }
        return (try? JSONDecoder().decode([DeviceSnapshot].self, from: data)) ?? []
    }
}
