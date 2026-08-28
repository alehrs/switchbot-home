import AppKit
import SwiftUI

struct PopoverContentView: View {
    @Environment(AppStore.self) private var store
    @Environment(\.openSettings) private var openSettings

    var onAppearRefresh: () -> Void

    var body: some View {
        // No explicit `path:` binding — nothing outside this view needs
        // to observe or manipulate navigation state, so NavigationStack
        // can manage its own push/pop (including the standard back
        // button) internally.
        NavigationStack {
            listContent
                .navigationDestination(for: String.self) { deviceID in
                    // Back navigation is the NavigationStack's own
                    // standard system back button — a second, custom
                    // "Back" button inside DeviceDetailView was redundant
                    // (and confusing: two back buttons on one screen) and
                    // has been removed.
                    DeviceDetailView(deviceID: deviceID)
                }
                .navigationDestination(for: DeviceEditRoute.self) { route in
                    DeviceEditView(deviceID: route.deviceID)
                }
        }
        .onAppear(perform: onAppearRefresh)
    }

    private var listContent: some View {
        VStack(alignment: .leading, spacing: 8) {
            if case .offline(let since) = store.connectionState {
                OfflineBannerView(since: since)
            }

            if store.sections.isEmpty {
                Text("No devices yet")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.vertical, 24)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(store.sections, id: \.title) { section in
                            RoomSectionView(title: section.title, devices: section.devices)
                        }
                    }
                }
                // A ScrollView with only a maxHeight has no well-defined
                // ideal height, so a `.window`-style MenuBarExtra popover
                // (which sizes itself to its content's ideal size) can
                // render it almost collapsed — hence the reported "can't
                // even see the first device without scrolling" bug.
                // Setting idealHeight removes that ambiguity at the root
                // instead of just bounding it; minHeight forces room for
                // a few rows before scrolling kicks in even if content is
                // shorter. ~280pt comfortably fits 3 rows even in the
                // worst case of 3 separate room sections (each adding its
                // own header).
                .frame(minHeight: 280, idealHeight: 280, maxHeight: 460)
            }

            Divider()

            HStack {
                if let lastRefreshedAt = store.lastRefreshedAt {
                    Text("Updated \(lastRefreshedAt.formatted(date: .omitted, time: .shortened))")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Settings…") {
                    // As an `LSUIElement` accessory app, this app is never
                    // the frontmost app on its own — without activating it
                    // first, the Settings window opens behind whatever
                    // app currently has focus instead of on top.
                    NSApp.activate()
                    openSettings()
                }
                Button("Quit") {
                    NSApplication.shared.terminate(nil)
                }
            }
        }
        .padding(12)
        .frame(width: 340)
    }
}
