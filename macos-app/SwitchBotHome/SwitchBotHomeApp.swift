import SwiftUI

@main
struct SwitchBotHomeApp: App {
    @State private var settings: AppSettings
    @State private var store: AppStore
    private let pollingService: PollingService

    init() {
        let settings = AppSettings()
        let store = AppStore()
        _settings = State(initialValue: settings)
        _store = State(initialValue: store)

        let pollingService = PollingService(store: store, client: APIClient(baseURLProvider: settings))
        self.pollingService = pollingService
        // Runs for the whole app lifetime, independent of whether the
        // popover is ever opened, so the menu bar icon's offline state
        // (and the popover, whenever it IS opened) stay current.
        pollingService.start()
    }

    var body: some Scene {
        MenuBarExtra {
            PopoverContentView(onAppearRefresh: { [pollingService] in
                pollingService.refreshIfStale()
            })
            .environment(store)
            .environment(settings)
        } label: {
            MenuBarLabelView(connectionState: store.connectionState)
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView()
                .environment(settings)
        }
    }
}
