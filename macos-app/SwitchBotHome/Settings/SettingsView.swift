import AppKit
import SwiftUI

struct SettingsView: View {
    @Environment(AppSettings.self) private var settings

    @State private var draftURL: String = ""
    @State private var validationMessage: String?
    @State private var testResult: TestResult?
    @State private var isTesting = false

    private enum TestResult {
        case success
        case failure(String)
    }

    var body: some View {
        Form {
            Section {
                TextField("API Base URL", text: $draftURL, prompt: Text("http://192.168.1.10:3000"))
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(save)

                if let validationMessage {
                    Text(validationMessage)
                        .font(.caption)
                        .foregroundStyle(.red)
                }

                HStack {
                    Button("Test Connection") {
                        Task { await testConnection() }
                    }
                    .disabled(isTesting)

                    if isTesting {
                        ProgressView().controlSize(.small)
                    } else if let testResult {
                        testResultView(testResult)
                    }
                }

                Text("If macOS asks to allow local network access, click Allow — the app needs it to reach your backend on your home network.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            Section {
                Button("Save") {
                    save()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 380)
        .onAppear {
            draftURL = settings.apiBaseURL
        }
        // SwiftUI's `Settings` scene reuses a single window instance
        // across opens, and by default a window stays pinned to whatever
        // Space it was first created on. `.moveToActiveSpace` makes it
        // follow the user to whichever Space is currently active instead,
        // so "Settings…" always surfaces it there rather than switching
        // (or failing to switch) the user to some other Space.
        .background(WindowAccessor { window in
            window.collectionBehavior.insert(.moveToActiveSpace)
            window.makeKeyAndOrderFront(nil)
        })
    }

    @ViewBuilder
    private func testResultView(_ result: TestResult) -> some View {
        switch result {
        case .success:
            Label("Connected", systemImage: "checkmark.circle.fill")
                .foregroundStyle(.green)
        case .failure(let message):
            Label(message, systemImage: "xmark.circle.fill")
                .foregroundStyle(.red)
        }
    }

    private func save() {
        switch AppSettings.validate(draftURL) {
        case .success:
            settings.apiBaseURL = draftURL
            validationMessage = nil
        case .failure(let error):
            validationMessage = error.errorDescription
        }
    }

    private func testConnection() async {
        testResult = nil
        switch AppSettings.validate(draftURL) {
        case .failure(let error):
            testResult = .failure(error.errorDescription ?? "Invalid URL")
            return
        case .success:
            break
        }

        isTesting = true
        defer { isTesting = false }

        let client = APIClient(baseURLProvider: StaticBaseURL(apiBaseURL: draftURL))
        do {
            _ = try await client.fetchDevices()
            testResult = .success
        } catch {
            testResult = .failure(error.localizedDescription)
        }
    }
}

/// Bridges to the `NSWindow` hosting this SwiftUI view, since SwiftUI's
/// `Settings` scene doesn't otherwise expose it for the AppKit-level
/// adjustments in `.background(WindowAccessor { ... })` above.
private struct WindowAccessor: NSViewRepresentable {
    let onResolve: (NSWindow) -> Void

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        resolveWindow(for: view, attemptsRemaining: 5)
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {}

    // The view isn't attached to a window yet on this same run loop turn
    // — defer to the next one, by which point SwiftUI has usually
    // inserted it into the window's view hierarchy. `makeNSView` only
    // runs once for the lifetime of the (single, reused) Settings
    // window, so a one-shot attempt that loses this race would silently
    // and permanently skip `onResolve` for the rest of the app's run;
    // retrying a few more run loop turns makes that failure mode
    // vanishingly unlikely instead.
    private func resolveWindow(for view: NSView, attemptsRemaining: Int) {
        DispatchQueue.main.async {
            if let window = view.window {
                onResolve(window)
            } else if attemptsRemaining > 0 {
                resolveWindow(for: view, attemptsRemaining: attemptsRemaining - 1)
            }
        }
    }
}
