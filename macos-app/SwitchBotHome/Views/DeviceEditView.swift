import AppKit
import SwiftUI

/// Navigation value for pushing `DeviceEditView`. A dedicated type (not a
/// bare `String`, which `PopoverContentView` already uses for the device
/// detail screen) keeps the two `navigationDestination`s from colliding.
struct DeviceEditRoute: Hashable {
    let deviceID: String
}

/// Edits one device's label and room. Pushed onto
/// `PopoverContentView`'s `NavigationStack` (not a separate window — see
/// `DeviceDetailView`'s note on why menu-bar-only apps stay inside the
/// popover). Saving `PUT`s to the backend and optimistically updates the
/// store; the standard back button returns to the detail screen.
struct DeviceEditView: View {
    var deviceID: String

    @Environment(AppStore.self) private var store
    @Environment(AppSettings.self) private var settings
    @Environment(\.dismiss) private var dismiss

    @State private var label = ""
    @State private var room = ""
    @State private var isSaving = false
    @State private var saveError: String?

    private var currentDevice: Device? {
        store.snapshots.first { $0.id == deviceID }?.device
    }

    /// Existing room names across all devices, offered as a menu so the
    /// user can reuse one instead of retyping — `GroupingRules` groups
    /// rooms verbatim, so "cucina" and "Cucina" would otherwise split.
    private var knownRooms: [String] {
        let rooms = store.snapshots.compactMap { snapshot -> String? in
            let trimmed = snapshot.device.room?.trimmingCharacters(in: .whitespaces) ?? ""
            return trimmed.isEmpty ? nil : trimmed
        }
        return Set(rooms).sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }

    private var deviceName: String {
        store.snapshots.first { $0.id == deviceID }?.displayName ?? "Device"
    }

    // Mirrors `DeviceDetailView`'s shape exactly — an in-content title
    // (no `.navigationTitle`, which renders a misplaced bar inside the
    // borderless `MenuBarExtra(.window)` popover), `.padding(16)`,
    // `.frame(width: 460)` so the popover width doesn't jump when
    // navigating detail ↔ edit, and a trailing `Spacer` so short content
    // pins to the top instead of drifting into a corner.
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Edit \(deviceName)")
                .font(.title2.bold())

            labelledField("Label", text: $label, prompt: "e.g. Camera da letto")

            VStack(alignment: .leading, spacing: 4) {
                Text("Room")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                HStack(spacing: 6) {
                    TextField("Room", text: $room, prompt: Text("e.g. Piano terra"))
                        .textFieldStyle(.roundedBorder)
                        .onSubmit(save)

                    if !knownRooms.isEmpty {
                        Menu {
                            ForEach(knownRooms, id: \.self) { name in
                                Button(name) { room = name }
                            }
                        } label: {
                            Image(systemName: "list.bullet")
                        }
                        .menuStyle(.borderlessButton)
                        .fixedSize()
                    }
                }
            }

            Text("Leave a field empty to clear it.")
                .font(.caption2)
                .foregroundStyle(.secondary)

            if let saveError {
                Text(saveError)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }

            HStack(spacing: 8) {
                if isSaving {
                    ProgressView().controlSize(.small)
                }
                Spacer()
                Button("Save", action: save)
                    .keyboardShortcut(.defaultAction)
                    .disabled(isSaving)
            }

            Spacer(minLength: 0)
        }
        .padding(16)
        .frame(width: 460, alignment: .topLeading)
        .onAppear {
            // A `MenuBarExtra(.window)` popover in an `LSUIElement` app
            // doesn't reliably give its text fields keyboard focus unless
            // the app is frontmost — same activation-policy quirk as the
            // Settings window.
            NSApp.activate()
            if let device = currentDevice {
                label = device.label ?? ""
                room = device.room ?? ""
            }
        }
    }

    @ViewBuilder
    private func labelledField(
        _ title: String,
        text: Binding<String>,
        prompt: String
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
            TextField(title, text: text, prompt: Text(prompt))
                .textFieldStyle(.roundedBorder)
                .onSubmit(save)
        }
    }

    private func save() {
        guard !isSaving else { return }
        isSaving = true
        saveError = nil

        // Always send both fields as strings: an emptied field becomes
        // "", which the backend maps to NULL — the "clear it" case — so
        // this screen never needs the omit-vs-clear tri-state itself.
        let newLabel = label.trimmingCharacters(in: .whitespacesAndNewlines)
        let newRoom = room.trimmingCharacters(in: .whitespacesAndNewlines)

        Task {
            defer { isSaving = false }
            let client = APIClient(baseURLProvider: settings)
            do {
                let updated = try await client.updateDevice(
                    deviceID: deviceID,
                    label: newLabel,
                    room: newRoom,
                    blacklisted: nil
                )
                store.applyDeviceUpdate(updated)
                dismiss()
            } catch {
                saveError = error.localizedDescription
            }
        }
    }
}
