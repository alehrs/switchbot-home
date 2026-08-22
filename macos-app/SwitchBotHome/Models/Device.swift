import Foundation

/// Mirrors the backend's `Device` JSON shape (`GET /devices`,
/// `PUT /devices/{device_id}`). `deviceID` is an opaque identifier — on
/// macOS it's a CoreBluetooth per-app UUID, not a real BLE MAC address —
/// never parse it for meaning.
struct Device: Codable, Identifiable, Equatable {
    var deviceID: String
    var label: String?
    var room: String?
    var blacklisted: Bool
    var firstSeenAt: Date
    var lastSeenAt: Date

    var id: String { deviceID }

    enum CodingKeys: String, CodingKey {
        case deviceID = "device_id"
        case label
        case room
        case blacklisted
        case firstSeenAt = "first_seen_at"
        case lastSeenAt = "last_seen_at"
    }
}
