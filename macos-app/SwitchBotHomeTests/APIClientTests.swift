import XCTest
@testable import SwitchBotHome

final class APIClientTests: XCTestCase {
    func testPathEncodedEscapesSlashesInLinuxStyleDeviceIDs() {
        let encoded = APIClient.pathEncoded("hci0/dev_D2_2E_81_06_5C_61")

        XCTAssertEqual(encoded, "hci0%2Fdev_D2_2E_81_06_5C_61")
    }

    func testPathEncodedRoundTripsBackToTheOriginalID() {
        let original = "hci0/dev_D2_2E_81_06_5C_61"

        let decoded = APIClient.pathEncoded(original).removingPercentEncoding

        XCTAssertEqual(decoded, original)
    }

    func testPathEncodedLeavesAMacOSStyleUUIDUnchanged() {
        let uuid = "574AD03B-4384-2307-708E-08D01FD8174D"

        XCTAssertEqual(APIClient.pathEncoded(uuid), uuid)
    }

    /// Regression test for a real bug: assigning an already-percent-encoded
    /// path string to `URLComponents.path` re-encodes it (`%2F` becomes
    /// `%252F`), because `.path`'s setter treats its input as the
    /// *decoded* logical value. That silently turned every readings-history
    /// request for a Linux-style device ID into a request for a
    /// nonexistent device — no 404, just an always-empty 200 response,
    /// confirmed against the live homelab backend. `pathEncoded` alone
    /// (tested above) can't catch this: the bug is in what happens to its
    /// *output* one step later, in `urlComponents`.
    func testFetchReadingsURLDoesNotDoubleEncodeADeviceIDSlash() throws {
        let client = APIClient(baseURLProvider: StaticBaseURL(apiBaseURL: "http://192.168.1.5:8090"))
        let encodedID = APIClient.pathEncoded("hci0/dev_D2_2E_81_06_5C_61")

        let components = try client.urlComponents(path: "/devices/\(encodedID)/readings")

        XCTAssertEqual(components.percentEncodedPath, "/devices/hci0%2Fdev_D2_2E_81_06_5C_61/readings")
    }
}
