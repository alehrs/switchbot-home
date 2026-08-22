import Foundation

/// Everything a `DeviceRowView` needs to render one device: identity,
/// display name, latest values, 1h averages, and trends. Built by
/// `AppStore` from raw `Device`/`Reading` API responses.
struct DeviceSnapshot: Identifiable, Equatable, Codable {
    var device: Device
    var rank: Int
    var latestTemperature: Double?
    var latestHumidity: Double?
    var latestBattery: Int?
    var latestRecordedAt: Date?
    var temperatureAverage1h: Double?
    var humidityAverage1h: Double?
    var temperatureTrend: Trend
    var humidityTrend: Trend

    var id: String { device.deviceID }

    var room: String? { device.room }

    /// The device's label, or "Unknown device #N" using its permanent
    /// rank (see `DeviceNumbering`) if it has none.
    var displayName: String {
        if let label = device.label, !label.trimmingCharacters(in: .whitespaces).isEmpty {
            return label
        }
        return "Unknown device #\(rank)"
    }
}
