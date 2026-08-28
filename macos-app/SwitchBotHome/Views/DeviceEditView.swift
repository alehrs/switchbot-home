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

    var body: some View {
        Form {
            Section {
                TextField("Label", text: $label, prompt: Text("e.g. Camera da letto"))
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(save)

                HStack {
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
                        .frame(width: 28)
                    }
                }

                Text("Leave a field empty to clear it.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)

                if let saveError {
                    Text(saveError)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }

            Section {
                HStack {
                    Button("Save", action: save)
                        .keyboardShortcut(.defaultAction)
                        .disabled(isSaving)
                    if isSaving {
                        ProgressView().controlSize(.small)
                    }
                }
            }
        }
        .padding(20)
        .frame(width: 340)
        .navigationTitle("Edit")
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
