import Foundation

/// Assigns each device a permanent display rank, used for "Unknown device
/// #N" when it has no label.
///
/// Ranks come from the FULL roster (including blacklisted devices) sorted
/// ascending by `firstSeenAt`, not just the currently-unlabeled subset.
/// This is what keeps a device's number stable forever: `firstSeenAt` is
/// set once at auto-discovery and never rewritten by the backend, so it's
/// always "now" at insert time — a later-discovered device can only ever
/// sort after every earlier one, never in the middle. Ranking over the
/// full roster (rather than renumbering only the unlabeled devices
/// densely) means labeling a device just removes it from the "Unknown"
/// list, leaving a gap (#1, #3) instead of shuffling its neighbor (#3)
/// down to (#2).
enum DeviceNumbering {
    static func ranks(for devices: [Device]) -> [String: Int] {
        let ordered = devices.sorted { $0.firstSeenAt < $1.firstSeenAt }
        var ranks: [String: Int] = [:]
        for (index, device) in ordered.enumerated() {
            ranks[device.deviceID] = index + 1
        }
        return ranks
    }
}
