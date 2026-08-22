import SwiftUI

struct DeviceRowView: View {
    var snapshot: DeviceSnapshot

    /// "as of HH:mm" shows once data is older than roughly two fast poll
    /// cycles (30s each) — new enough that a normal cycle wouldn't trip
    /// it, old enough to flag a real stall.
    private static let staleThreshold: TimeInterval = 60

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(snapshot.displayName)
                    .font(.headline)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer()
                batteryIndicator
                if isStale, let text = staleText {
                    Text(text)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                // Hints that the row opens a day-chart detail screen —
                // there's no other visual affordance once wrapped in a
                // plain-styled NavigationLink (RoomSectionView.swift).
                Image(systemName: "chevron.right")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            HStack(alignment: .top, spacing: 16) {
                // Icons make the unit unambiguous at a glance — "67%" on
                // its own could be read as anything; a thermometer vs. a
                // droplet next to it can't be.
                metricColumn(
                    icon: "thermometer",
                    value: snapshot.latestTemperature,
                    valueSuffix: "°C",
                    average: snapshot.temperatureAverage1h,
                    averageSuffix: "°C",
                    trend: snapshot.temperatureTrend,
                    trendSuffix: "°"
                )
                metricColumn(
                    icon: "drop.fill",
                    value: snapshot.latestHumidity,
                    valueSuffix: "%",
                    average: snapshot.humidityAverage1h,
                    averageSuffix: "%",
                    trend: snapshot.humidityTrend,
                    trendSuffix: "%"
                )
            }
        }
        .padding(.vertical, 6)
        .contentShape(Rectangle())
    }

    private var isStale: Bool {
        guard let recordedAt = snapshot.latestRecordedAt else { return false }
        return Date().timeIntervalSince(recordedAt) > Self.staleThreshold
    }

    private var staleText: String? {
        guard let recordedAt = snapshot.latestRecordedAt else { return nil }
        return "as of \(recordedAt.formatted(date: .omitted, time: .shortened))"
    }

    @ViewBuilder
    private var batteryIndicator: some View {
        if let battery = snapshot.latestBattery {
            HStack(spacing: 3) {
                Image(systemName: batteryIcon(for: battery))
                    .font(.caption2)
                Text(Formatting.number(Double(battery), decimals: 0, suffix: "%"))
                    .font(.caption2)
            }
            .foregroundStyle(battery < 20 ? .red : .secondary)
        }
    }

    /// Steps through SF Symbols' battery-level glyphs to match the
    /// reported percentage, rather than always showing a generic icon.
    private func batteryIcon(for percent: Int) -> String {
        switch percent {
        case 88...: return "battery.100"
        case 63..<88: return "battery.75"
        case 38..<63: return "battery.50"
        case 13..<38: return "battery.25"
        default: return "battery.0"
        }
    }

    @ViewBuilder
    private func metricColumn(
        icon: String,
        value: Double?,
        valueSuffix: String,
        average: Double?,
        averageSuffix: String,
        trend: Trend,
        trendSuffix: String
    ) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                Image(systemName: icon)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(value.map { Formatting.number($0, suffix: valueSuffix) } ?? "—")
                    .font(.title3)
            }
            HStack(spacing: 6) {
                if let average {
                    Text("avg " + Formatting.number(average, suffix: averageSuffix) + " (1h)")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                TrendIndicatorView(trend: trend, unitSuffix: trendSuffix)
            }
        }
    }
}
