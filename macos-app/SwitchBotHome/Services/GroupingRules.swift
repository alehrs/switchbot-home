import Foundation

/// Groups device snapshots by room for display. Blacklisted devices are
/// filtered out entirely (they still count toward `DeviceNumbering`'s
/// ranks, they just never render).
enum GroupingRules {
    static let ungroupedTitle = "Ungrouped"

    static func sections(for snapshots: [DeviceSnapshot]) -> [(title: String, devices: [DeviceSnapshot])] {
        let visible = snapshots.filter { !$0.device.blacklisted }

        let grouped = Dictionary(grouping: visible) { snapshot -> String in
            let trimmed = snapshot.room?.trimmingCharacters(in: .whitespaces) ?? ""
            return trimmed.isEmpty ? ungroupedTitle : trimmed
        }

        let sortedTitles = grouped.keys.sorted { lhs, rhs in
            if lhs == ungroupedTitle { return false }
            if rhs == ungroupedTitle { return true }
            return lhs.localizedStandardCompare(rhs) == .orderedAscending
        }

        return sortedTitles.map { title in
            let devices = (grouped[title] ?? []).sorted { $0.device.firstSeenAt < $1.device.firstSeenAt }
            return (title: title, devices: devices)
        }
    }
}
