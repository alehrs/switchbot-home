import SwiftUI

/// A small arrow + signed delta, styled after stock-app trend badges
/// (e.g. Trade Republic) but with deliberately different colors: orange
/// for rising, blue for falling, instead of the literal green=up/red=down
/// convention. Green/red encodes "up is good," which doesn't hold for
/// temperature or humidity — do not "fix" this back to green/red.
struct TrendIndicatorView: View {
    var trend: Trend
    var unitSuffix: String

    private static let minusSign = "\u{2212}"

    var body: some View {
        HStack(spacing: 2) {
            Image(systemName: iconName)
            Text(text)
        }
        .font(.caption)
        .foregroundStyle(color)
    }

    private var iconName: String {
        switch trend {
        case .rising: return "arrow.up.forward"
        case .falling: return "arrow.down.forward"
        case .flat: return "minus"
        case .insufficientData: return "questionmark"
        }
    }

    private var color: Color {
        switch trend {
        case .rising: return .orange
        case .falling: return .blue
        case .flat, .insufficientData: return .secondary
        }
    }

    private var text: String {
        switch trend {
        case .rising(let delta):
            return "+" + Formatting.number(delta, suffix: unitSuffix)
        case .falling(let delta):
            return Self.minusSign + Formatting.number(abs(delta), suffix: unitSuffix)
        case .flat:
            return Formatting.number(0, suffix: unitSuffix)
        case .insufficientData:
            return "—"
        }
    }
}
