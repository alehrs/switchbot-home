import SwiftUI
import XCTest

@testable import SwitchBotHome

/// `DeviceEditView` is pushed onto the `MenuBarExtra(.window)` popover's
/// `NavigationStack`, which sizes itself to the content's *ideal* size.
/// A SwiftUI `Form` has an ill-defined ideal height in that context and
/// collapsed the whole screen (no fields, no Save button visible). This
/// renders the view at its ideal size and asserts it actually has one —
/// the same failure mode already seen with `ScrollView` in
/// `PopoverContentView`.
@MainActor
final class DeviceEditViewLayoutTests: XCTestCase {
    func testEditScreenHasARealRenderedSize() throws {
        let store = AppStore()
        let settings = AppSettings(
            defaults: UserDefaults(suiteName: "DeviceEditViewLayoutTests")!
        )

        let view = NavigationStack {
            DeviceEditView(deviceID: "dev_TEST")
        }
        .environment(store)
        .environment(settings)

        let image = try XCTUnwrap(
            ImageRenderer(content: view).nsImage,
            "ImageRenderer produced no image"
        )
        let attachment = XCTAttachment(image: image)
        attachment.lifetime = .keepAlways
        attachment.name = "device-edit-rendered"
        add(attachment)

        // Title + two labelled text fields + a hint line + a Save button
        // is comfortably over 200pt tall and 300pt wide. The Form version
        // rendered at 340×144.
        XCTAssertGreaterThan(image.size.height, 200, "edit screen collapsed to \(image.size)")
        XCTAssertGreaterThan(image.size.width, 300, "edit screen collapsed to \(image.size)")
    }
}
