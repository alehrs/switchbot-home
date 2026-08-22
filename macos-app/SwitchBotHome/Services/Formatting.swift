import Foundation

/// Formats a numeric value with a decimal count and a unit suffix.
///
/// Building a format string by directly interpolating the suffix (e.g.
/// `String(format: "%.1f\(suffix)", value)`) is broken whenever the
/// suffix itself contains a literal "%" (exactly the case for humidity):
/// printf-style format strings treat "%" as the start of a new
/// specifier, so a trailing bare "%" is parsed as a malformed spec — the
/// runtime silently drops it (the value renders without its unit) and
/// logs a "does not match expected" warning on every call. Formatting
/// the number alone and appending the suffix as plain string
/// concatenation avoids ever feeding the suffix through the format
/// parser in the first place.
enum Formatting {
    static func number(_ value: Double, decimals: Int = 1, suffix: String) -> String {
        String(format: "%.\(decimals)f", value) + suffix
    }
}
