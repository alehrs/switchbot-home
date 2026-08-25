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
}
