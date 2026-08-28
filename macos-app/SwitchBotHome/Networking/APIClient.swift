import Foundation

enum APIError: Error, LocalizedError {
    case invalidBaseURL(String)
    case transport(Error)
    case httpStatus(Int)
    case decoding(Error)
    case encoding(Error)

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
        case .encoding(let error):
            return "Couldn't build the request: \(error.localizedDescription)"
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

    /// `PUT /devices/{device_id}`. Per the backend contract, each field is
    /// tri-state: pass `nil` to leave it unchanged, `""` to clear it to
    /// null, a non-empty string to set it. Returns the updated device;
    /// throws `APIError.httpStatus(404)` if the id is unknown.
    func updateDevice(
        deviceID: String,
        label: String?,
        room: String?,
        blacklisted: Bool?
    ) async throws -> Device {
        let request = try makeUpdateRequest(
            deviceID: deviceID,
            body: UpdateDeviceBody(label: label, room: room, blacklisted: blacklisted)
        )
        return try await send(Device.self, request: request)
    }

    // MARK: - Plumbing

    /// Only `label`/`room`/`blacklisted` that are non-nil are encoded —
    /// `JSONEncoder` omits nil optionals, which is exactly the backend's
    /// "omit the field to leave it unchanged" semantics. Keys already
    /// match the backend's snake-case-free names, so no `CodingKeys`.
    struct UpdateDeviceBody: Encodable {
        let label: String?
        let room: String?
        let blacklisted: Bool?
    }

    /// Built via `urlComponents(path:)` + `pathEncoded` for the same
    /// reason `fetchReadings` is: a Linux-style `device_id` contains a
    /// literal `/` that must stay one path segment and must not get
    /// double-encoded (`%2F` → `%252F`).
    func makeUpdateRequest(deviceID: String, body: UpdateDeviceBody) throws -> URLRequest {
        guard let url = try urlComponents(path: "/devices/\(Self.pathEncoded(deviceID))").url else {
            throw APIError.invalidBaseURL(settings.apiBaseURL)
        }
        var request = URLRequest(url: url)
        request.httpMethod = "PUT"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        do {
            request.httpBody = try BackendCoding.encoder.encode(body)
        } catch {
            throw APIError.encoding(error)
        }
        return request
    }

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

    /// `path` arrives already percent-encoded where it matters (the `/`
    /// inside a Linux-style device ID via `pathEncoded` above). Assigning
    /// to `.path` would re-encode it — `.path`'s setter treats its input
    /// as the *decoded* logical value and escapes it again, turning our
    /// `%2F` into `%252F` (confirmed against a real double-encoded 200-
    /// with-zero-rows response from the live backend, not just reasoned
    /// about). `.percentEncodedPath` takes the string as already-encoded
    /// and uses it verbatim.
    func urlComponents(path: String) throws -> URLComponents {
        guard var components = URLComponents(string: settings.apiBaseURL) else {
            throw APIError.invalidBaseURL(settings.apiBaseURL)
        }
        components.percentEncodedPath = path
        return components
    }

    private func get<T: Decodable>(_ type: T.Type, path: String) async throws -> T {
        guard let url = try urlComponents(path: path).url else {
            throw APIError.invalidBaseURL(settings.apiBaseURL)
        }
        return try await get(type, url: url)
    }

    private func get<T: Decodable>(_ type: T.Type, url: URL) async throws -> T {
        // A bare URL is a GET; funnel it through the same send/decode path
        // as everything else so the status-check and error-wrapping logic
        // lives in one place.
        try await send(type, request: URLRequest(url: url))
    }

    private func send<T: Decodable>(_ type: T.Type, request: URLRequest) async throws -> T {
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
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
