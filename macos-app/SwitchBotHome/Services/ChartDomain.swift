import Foundation

/// Computes a stable Y-axis domain for a chart from its actual data,
/// instead of leaving Swift Charts to infer one automatically.
///
/// Automatic domains are recomputed from whatever marks are currently on
/// screen — including the point/annotation a `chartXSelection` hover adds
/// — which can visibly shift the axis scale on every hover, most
/// noticeably for a value like humidity whose real range is often narrow.
/// A domain derived once from the full day's readings doesn't move when a
/// selection is added or removed.
enum ChartDomain {
    static func range(
        for values: [Double],
        minPadding: Double = 1.0,
        paddingFraction: Double = 0.1,
        fallback: ClosedRange<Double> = 0...30
    ) -> ClosedRange<Double> {
        guard let minValue = values.min(), let maxValue = values.max() else {
            return fallback
        }
        // `max(minPadding, ...)` also covers a flat/single-value series
        // (maxValue - minValue == 0) — it just falls back to minPadding,
        // giving that single value visible headroom instead of a
        // zero-width domain.
        let padding = max(minPadding, (maxValue - minValue) * paddingFraction)
        return (minValue - padding)...(maxValue + padding)
    }
}
