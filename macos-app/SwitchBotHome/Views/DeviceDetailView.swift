import Charts
import SwiftUI

/// A day's temperature/humidity charts for one device, with day-to-day
/// navigation. Pushed onto `PopoverContentView`'s `NavigationStack` — a
/// separate `WindowGroup` was considered instead, but menu-bar-only
/// (`LSUIElement`) apps have unreliable window activation/focus behavior
/// for auxiliary windows, so staying inside the same popover (which
/// resizes to fit whichever screen is pushed) is more robust.
struct DeviceDetailView: View {
    var deviceID: String

    @Environment(AppStore.self) private var store
    @Environment(AppSettings.self) private var settings

    @State private var dayRange = DayRange.containing(Date())
    @State private var readings: [Reading] = []
    @State private var isLoading = false
    @State private var loadError: String?

    /// Shared between both charts (`.chartXSelection`, bound to the same
    /// state in both `metricChart` calls) so hovering over either one
    /// shows a synchronized crosshair/tooltip on both — moving the mouse
    /// over the temperature chart also highlights the humidity value at
    /// that same moment, and vice versa.
    @State private var selectedTime: Date?

    private var displayName: String {
        store.snapshots.first { $0.id == deviceID }?.displayName ?? deviceID
    }

    /// Both charts plot the same underlying `readings`, so one lookup
    /// serves both — no need to search separately per chart.
    private var selectedReading: Reading? {
        guard let selectedTime else { return nil }
        return readings.min {
            abs($0.recordedAt.timeIntervalSince(selectedTime)) < abs($1.recordedAt.timeIntervalSince(selectedTime))
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(displayName)
                .font(.title2.bold())

            dayNavigator

            content
        }
        .padding(16)
        .frame(width: 460)
        .task(id: dayRange) {
            await load()
        }
    }

    private var dayNavigator: some View {
        HStack {
            Button {
                dayRange = dayRange.advanced(by: -1)
            } label: {
                Image(systemName: "chevron.left")
            }
            Spacer()
            Text(dayRange.start.formatted(date: .abbreviated, time: .omitted))
                .font(.subheadline.bold())
            Spacer()
            Button {
                dayRange = dayRange.advanced(by: 1)
            } label: {
                Image(systemName: "chevron.right")
            }
            .disabled(dayRange.isToday)
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private var content: some View {
        if isLoading {
            placeholder { ProgressView() }
        } else if let loadError {
            placeholder { Text(loadError).foregroundStyle(.red).multilineTextAlignment(.center) }
        } else if readings.isEmpty {
            placeholder { Text("No data for this day").foregroundStyle(.secondary) }
        } else {
            metricChart(
                title: "Temperature",
                suffix: "°C",
                keyPath: \.temperature,
                color: .orange,
                // Derived from the day's actual readings (not a fixed
                // range) so it stays correct for sub-zero days and hot
                // days above 40°C alike, but computed once from the data
                // rather than left to Swift Charts' automatic domain —
                // see `yDomain` doc comment on why that matters.
                yDomain: ChartDomain.range(for: readings.map(\.temperature))
            )
            metricChart(
                title: "Humidity",
                suffix: "%",
                keyPath: \.humidity,
                color: .blue,
                // Relative humidity is always 0-100% by definition, so
                // this is fixed rather than data-derived.
                yDomain: 0...100
            )
        }
    }

    @ViewBuilder
    private func placeholder<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        content()
            .frame(maxWidth: .infinity, minHeight: 300, alignment: .center)
    }

    private func load() async {
        // `.task(id: dayRange)` cancels an in-flight load when the day
        // changes, but cancellation is cooperative: if this call's network
        // request happens to finish right as a newer one starts, nothing
        // stops it from resuming and writing its (now-stale, wrong-day)
        // result after the newer load has already begun. Capturing the
        // day this call is actually for and checking it's still current
        // before committing anything closes that race unconditionally,
        // regardless of cancellation timing.
        let requestedRange = dayRange

        isLoading = true
        loadError = nil
        defer {
            if requestedRange == dayRange {
                isLoading = false
            }
        }

        let client = APIClient(baseURLProvider: settings)
        do {
            let fetched = try await client
                .fetchReadings(deviceID: deviceID, from: requestedRange.start, to: requestedRange.end)
                .sorted { $0.recordedAt < $1.recordedAt }
            guard requestedRange == dayRange else { return }
            readings = ReadingsDownsampler.downsample(fetched)
        } catch {
            guard requestedRange == dayRange else { return }
            loadError = error.localizedDescription
            readings = []
        }
    }

    @ViewBuilder
    private func metricChart(
        title: String,
        suffix: String,
        keyPath: KeyPath<Reading, Double>,
        color: Color,
        yDomain: ClosedRange<Double>
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("\(title) (\(suffix))")
                .font(.caption)
                .foregroundStyle(.secondary)
            Chart {
                ForEach(readings) { reading in
                    LineMark(
                        x: .value("Time", reading.recordedAt),
                        y: .value(title, reading[keyPath: keyPath])
                    )
                    .foregroundStyle(color)
                }

                if let selectedReading {
                    RuleMark(x: .value("Selected time", selectedReading.recordedAt))
                        .foregroundStyle(.secondary.opacity(0.3))
                    PointMark(
                        x: .value("Time", selectedReading.recordedAt),
                        y: .value(title, selectedReading[keyPath: keyPath])
                    )
                    .foregroundStyle(color)
                    .symbolSize(60)
                    .annotation(position: .top) {
                        Text(Formatting.number(selectedReading[keyPath: keyPath], suffix: suffix))
                            .font(.caption2.bold())
                            .padding(.horizontal, 5)
                            .padding(.vertical, 2)
                            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 4))
                    }
                }
            }
            // Fixed to the full calendar day (not just the data's own
            // range) so every day renders on the same scale — for today,
            // the line simply stops partway across instead of stretching
            // to fill the chart.
            .chartXScale(domain: dayRange.start...dayRange.end)
            // Precomputed from the day's data (or fixed at 0-100 for
            // humidity), not left automatic — an automatic domain gets
            // recomputed from whatever's currently on screen, including
            // the selection point/annotation `chartXSelection` adds,
            // which visibly shifted the axis scale on every hover before
            // this fix (most noticeable for humidity's narrower range).
            .chartYScale(domain: yDomain)
            .chartXAxis {
                AxisMarks(values: .stride(by: .hour, count: 4)) { _ in
                    AxisGridLine()
                    AxisValueLabel(format: .dateTime.hour())
                }
            }
            // The title above already says e.g. "Humidity (%)", but that
            // reads as a chart caption, not as attached to the actual
            // numbers — putting the unit directly on each Y-axis tick
            // (e.g. "65%" instead of a bare "65") removes any doubt while
            // looking at the plotted values themselves.
            .chartYAxis {
                AxisMarks { value in
                    AxisGridLine()
                    AxisValueLabel {
                        if let numericValue = value.as(Double.self) {
                            Text(Formatting.number(numericValue, decimals: 0, suffix: suffix))
                        }
                    }
                }
            }
            // Shared across both charts (same `$selectedTime` passed to
            // both calls) so hovering either one moves a synchronized
            // crosshair/tooltip on both.
            .chartXSelection(value: $selectedTime)
            .frame(height: 140)
        }
    }
}
