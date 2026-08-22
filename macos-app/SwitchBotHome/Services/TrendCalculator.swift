import Foundation

enum Trend: Equatable, Codable {
    case rising(delta: Double)
    case falling(delta: Double)
    case flat
    case insufficientData

    private enum CodingKeys: String, CodingKey {
        case kind, delta
    }

    private enum Kind: String, Codable {
        case rising, falling, flat, insufficientData
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(Kind.self, forKey: .kind) {
        case .rising:
            self = .rising(delta: try container.decode(Double.self, forKey: .delta))
        case .falling:
            self = .falling(delta: try container.decode(Double.self, forKey: .delta))
        case .flat:
            self = .flat
        case .insufficientData:
            self = .insufficientData
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .rising(let delta):
            try container.encode(Kind.rising, forKey: .kind)
            try container.encode(delta, forKey: .delta)
        case .falling(let delta):
            try container.encode(Kind.falling, forKey: .kind)
            try container.encode(delta, forKey: .delta)
        case .flat:
            try container.encode(Kind.flat, forKey: .kind)
        case .insufficientData:
            try container.encode(Kind.insufficientData, forKey: .kind)
        }
    }
}

/// Computes a rising/falling trend for one field of a device's trailing
/// hour of readings (temperature or humidity — same algorithm, applied
/// via keypath so the two rows share one tested implementation).
///
/// Deliberately an edge-averaged two-point delta (mean of the last ~5min
/// vs. mean of the first ~5min of the trailing hour), not a single-sample
/// delta (flickers on ordinary sensor jitter when readings arrive every
/// few seconds) and not a regression slope (that's a rate, not the plain
/// signed delta value this UI shows).
enum TrendCalculator {
    /// °C. Roughly matches the Meter Plus's 0.1°C reporting granularity.
    static let temperatureDeadband = 0.2
    /// Percentage points. Wider than the temperature deadband because
    /// humidity naturally swings more; 0.2 would flicker constantly.
    static let humidityDeadband = 2.0

    private static let hour: TimeInterval = 60 * 60
    private static let edgeWidth: TimeInterval = 5 * 60

    static func calculate(
        readings: [Reading],
        keyPath: KeyPath<Reading, Double>,
        deadband: Double,
        now: Date = Date()
    ) -> Trend {
        let sorted = readings.sorted { $0.recordedAt < $1.recordedAt }
        guard sorted.count >= 2 else { return .insufficientData }

        let windowStart = now.addingTimeInterval(-hour)
        let baselineWindowEnd = windowStart.addingTimeInterval(edgeWidth)
        let recentWindowStart = now.addingTimeInterval(-edgeWidth)

        let baseline = sorted.filter { $0.recordedAt >= windowStart && $0.recordedAt <= baselineWindowEnd }
        let recent = sorted.filter { $0.recordedAt >= recentWindowStart && $0.recordedAt <= now }

        let delta: Double
        if let baselineMean = mean(baseline, keyPath: keyPath), let recentMean = mean(recent, keyPath: keyPath) {
            delta = recentMean - baselineMean
        } else {
            // Not enough history to fill both 5-minute edges (e.g. a
            // recently-discovered device): fall back to a plain
            // first-vs-last delta over whatever readings exist.
            delta = sorted.last![keyPath: keyPath] - sorted.first![keyPath: keyPath]
        }

        if abs(delta) < deadband {
            return .flat
        }
        return delta > 0 ? .rising(delta: delta) : .falling(delta: delta)
    }

    private static func mean(_ readings: [Reading], keyPath: KeyPath<Reading, Double>) -> Double? {
        guard !readings.isEmpty else { return nil }
        return readings.reduce(0) { $0 + $1[keyPath: keyPath] } / Double(readings.count)
    }
}
