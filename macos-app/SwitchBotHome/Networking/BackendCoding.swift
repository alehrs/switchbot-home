import Foundation

/// JSON coding shared by anything that reads/writes the backend's
/// `Device`/`Reading` shapes: the backend (Rust `chrono`) serializes
/// timestamps as RFC3339 with fractional seconds (e.g.
/// `"2026-08-21T10:37:08.973140Z"`), which Foundation's built-in
/// `.iso8601` decoding strategy does NOT parse (it only handles whole
/// seconds). Falls back to whole-seconds ISO 8601 for safety.
enum BackendCoding {
    private static let fractionalSecondsFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    private static let wholeSecondsFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        return formatter
    }()

    static var decoder: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let container = try decoder.singleValueContainer()
            let string = try container.decode(String.self)
            if let date = fractionalSecondsFormatter.date(from: string) {
                return date
            }
            if let date = wholeSecondsFormatter.date(from: string) {
                return date
            }
            throw DecodingError.dataCorruptedError(
                in: container,
                debugDescription: "Expected an ISO 8601 date string, got \(string)"
            )
        }
        return decoder
    }

    static var encoder: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .custom { date, encoder in
            var container = encoder.singleValueContainer()
            try container.encode(fractionalSecondsFormatter.string(from: date))
        }
        return encoder
    }
}
