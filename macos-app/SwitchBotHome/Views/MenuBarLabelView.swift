import SwiftUI

/// The always-visible menu bar content: icon only, no numeric value.
/// With devices spread across different rooms there's no single
/// meaningful number to show collapsed (averaging across rooms is
/// meaningless; picking one "primary" device needs a pinning feature
/// nobody asked for) — the icon just opens the popover with everything.
struct MenuBarLabelView: View {
    var connectionState: ConnectionState

    var body: some View {
        Image(systemName: "thermometer.medium")
            .foregroundStyle(tint)
    }

    private var tint: Color {
        if case .offline = connectionState {
            return .orange
        }
        return .primary
    }
}
