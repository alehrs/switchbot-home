import Foundation

enum APIError: Error, LocalizedError {
    case invalidBaseURL(String)
    case transport(Error)
    case httpStatus(Int)
    case decoding(Error)

    var errorDescription: String? {
        switch self {
        case .invalidBaseURL(let value):
            return "Invalid API URL: \(value)"
        case .transport(let error):
            return "Network error: \(error.localizedDescription)"
        case .httpStatus(let code):
            return "Server returned HTTP \(code)"
        case .decoding(let error):
            return "Couldn't parse the server's response: \(error.localizedDescription)"
        }
    }
}

/// Whatever can hand `APIClient` a base URL string. `AppSettings`
/// conforms for normal use; `SettingsView` also constructs an ad-hoc
/// `StaticBaseURL` to test-connect an not-yet-saved draft URL without
/// mutating the real settings.
protocol BaseURLProviding {
    var apiBaseURL: String { get }
}

struct StaticBaseURL: BaseURLProviding {
    let apiBaseURL: String
}

/// Talks only to the backend's REST API — no BLE, no SwitchBot-specific
/// logic here. Reads the base URL from `baseURLProvider` on every call,
/// not once at init, so a settings change takes effect immediately.
struct APIClient {
    var baseURLProvider: BaseURLProviding
    var session: URLSession = .shared

    private var settings: BaseURLProviding { baseURLProvider }

    func fetchDevices() async throws -> [Device] {
        try await get([Device].self, path: "/devices")
    }

    func fetchLatestReadings() async throws -> [Reading] {
        try await get([Reading].self, path: "/readings/latest")
    }

    func fetchReadings(deviceID: String, from: Date, to: Date) async throws -> [Reading] {
        var components = try urlComponents(path: "/devices/\(Self.pathEncoded(deviceID))/readings")
        let formatter = ISO8601DateFormatter()
        components.queryItems = [
            URLQueryItem(name: "from", value: formatter.string(from: from)),
            URLQueryItem(name: "to", value: formatter.string(from: to)),
        ]
        guard let url = components.url else {
            throw APIError.invalidBaseURL(settings.apiBaseURL)
        }
        return try await get([Reading].self, url: url)
    }

    // MARK: - Plumbing

    /// `device_id` is opaque and, on Linux, btleplug formats it as
    /// `hci0/dev_XX_XX_XX_XX_XX_XX` — a literal `/`. Left un-encoded,
    /// that turns one path segment into two and the server 404s (its
    /// route has a fixed segment count). `.urlPathAllowed` alone doesn't
    /// help here since `/` is itself a legal *path* character; this
    /// segment needs it encoded, not treated as a separator.
    static func pathEncoded(_ deviceID: String) -> String {
        let allowedInASegment = CharacterSet.urlPathAllowed.subtracting(CharacterSet(charactersIn: "/"))
        return deviceID.addingPercentEncoding(withAllowedCharacters: allowedInASegment) ?? deviceID
    }

    private func urlComponents(path: String) throws -> URLComponents {
        guard var components = URLComponents(string: settings.apiBaseURL) else {
            throw APIError.invalidBaseURL(settings.apiBaseURL)
        }
        components.path = path
        return components
    }

    private func get<T: Decodable>(_ type: T.Type, path: String) async throws -> T {
        guard let url = try urlComponents(path: path).url else {
            throw APIError.invalidBaseURL(settings.apiBaseURL)
        }
        return try await get(type, url: url)
    }

    private func get<T: Decodable>(_ type: T.Type, url: URL) async throws -> T {
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(from: url)
        } catch {
            throw APIError.transport(error)
        }

        if let httpResponse = response as? HTTPURLResponse, !(200..<300).contains(httpResponse.statusCode) {
            throw APIError.httpStatus(httpResponse.statusCode)
        }

        do {
            return try BackendCoding.decoder.decode(T.self, from: data)
        } catch {
            throw APIError.decoding(error)
        }
    }
}
