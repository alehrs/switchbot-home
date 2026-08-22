import Foundation
import Observation

/// The backend's base URL, persisted in `UserDefaults` and read fresh by
/// `APIClient` on every request — a change here takes effect immediately,
/// no app restart needed.
@Observable
final class AppSettings: BaseURLProviding {
    private static let apiBaseURLKey = "apiBaseURL"
    private static let defaultBaseURL = "http://localhost:3000"

    private let defaults: UserDefaults

    var apiBaseURL: String {
        didSet {
            defaults.set(apiBaseURL, forKey: Self.apiBaseURLKey)
        }
    }

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        self.apiBaseURL = defaults.string(forKey: Self.apiBaseURLKey) ?? Self.defaultBaseURL
    }

    /// Rejects anything that isn't a well-formed http/https URL with a
    /// host, so a typo can't silently persist as "valid."
    static func validate(_ candidate: String) -> Result<Void, ValidationError> {
        guard let components = URLComponents(string: candidate) else {
            return .failure(.malformed)
        }
        guard let scheme = components.scheme, scheme == "http" || scheme == "https" else {
            return .failure(.unsupportedScheme)
        }
        guard let host = components.host, !host.isEmpty else {
            return .failure(.missingHost)
        }
        return .success(())
    }

    enum ValidationError: Error, LocalizedError {
        case malformed
        case unsupportedScheme
        case missingHost

        var errorDescription: String? {
            switch self {
            case .malformed:
                return "That doesn't look like a valid URL."
            case .unsupportedScheme:
                return "The URL must start with http:// or https://."
            case .missingHost:
                return "The URL needs a host, e.g. http://192.168.1.10:3000."
            }
        }
    }
}
