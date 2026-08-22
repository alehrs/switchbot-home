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
