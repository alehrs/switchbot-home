import SwiftUI
import XCTest

@testable import SwitchBotHome

/// `DeviceEditView` is pushed onto the `MenuBarExtra(.window)` popover's
/// `NavigationStack`. A `Form` (and a bare `ScrollView`) has no ideal
/// height there and collapsed the screen; `.navigationTitle` renders a
/// misplaced bar. See
/// `.agents/notes/macos-app/2026-08-form-in-menubarextra-popover-collapses.md`.
@MainActor
final class DeviceEditViewLayoutTests: XCTestCase {
    func testEditScreenHasARealRenderedSize() throws {
        let store = AppStore()
        let settings = AppSettings(
            defaults: UserDefaults(suiteName: "DeviceEditViewLayoutTests")!
        )

        let view = NavigationStack { DeviceEditView(deviceID: "dev_TEST") }
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

        // Title + two labelled fields + a hint + a Save button: comfortably
        // over 200pt tall, and it takes the full ~460pt width. The Form
        // version rendered at 340×144.
        XCTAssertGreaterThan(image.size.height, 200, "edit screen collapsed to \(image.size)")
        XCTAssertGreaterThan(image.size.width, 400, "edit screen too narrow: \(image.size)")
    }
}
