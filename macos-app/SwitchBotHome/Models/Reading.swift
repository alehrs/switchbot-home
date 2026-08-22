import Foundation

/// Mirrors the backend's `Reading` JSON shape (`GET /readings/latest`,
/// `GET /devices/{device_id}/readings`).
struct Reading: Codable, Identifiable, Equatable {
    var id: Int
    var deviceID: String
    var temperature: Double
    var humidity: Double
    var battery: Int?
    var recordedAt: Date

    enum CodingKeys: String, CodingKey {
        case id
        case deviceID = "device_id"
        case temperature
        case humidity
        case battery
        case recordedAt = "recorded_at"
    }
}
