import Foundation

/// Caps how many points a chart ever has to render. A full unthrottled
/// day of Meter Plus readings (broadcasts every few seconds, no
/// `READING_INTERVAL_SECONDS` set — the backend's own default) can reach
/// tens of thousands of rows; even a partial day already hits hundreds
/// (562 observed for ~2.5 hours in manual testing). Charting all of them
/// is a real performance/memory risk, not a hypothetical one.
///
/// Reduces to time-ordered buckets, each collapsed to its average
/// temperature/humidity — smoother and more representative of the actual
/// trend than naively keeping every Nth point (which can alias away or
/// exaggerate short spikes depending on where the stride happens to land).
enum ReadingsDownsampler {
    static let defaultMaxPoints = 300

    static func downsample(_ readings: [Reading], maxPoints: Int = defaultMaxPoints) -> [Reading] {
        guard readings.count > maxPoints, maxPoints > 0 else { return readings }

        let bucketSize = Int(ceil(Double(readings.count) / Double(maxPoints)))
        var result: [Reading] = []
        result.reserveCapacity(maxPoints)

        var index = 0
        while index < readings.count {
            let end = min(index + bucketSize, readings.count)
            result.append(averaged(readings[index..<end]))
            index = end
        }
        return result
    }

    private static func averaged(_ bucket: ArraySlice<Reading>) -> Reading {
        let count = Double(bucket.count)
        let averageTemperature = bucket.reduce(0) { $0 + $1.temperature } / count
        let averageHumidity = bucket.reduce(0) { $0 + $1.humidity } / count
        // The middle reading of the bucket stands in for id/deviceID/
        // battery/timestamp — only temperature/humidity are averaged, the
        // rest just need a representative value for that time slot.
        let representative = bucket[bucket.index(bucket.startIndex, offsetBy: bucket.count / 2)]

        return Reading(
            id: representative.id,
            deviceID: representative.deviceID,
            temperature: averageTemperature,
            humidity: averageHumidity,
            battery: representative.battery,
            recordedAt: representative.recordedAt
        )
    }
}
