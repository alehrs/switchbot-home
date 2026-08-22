import SwiftUI

struct OfflineBannerView: View {
    var since: Date

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "wifi.slash")
            Text("Backend unreachable since \(since.formatted(date: .omitted, time: .shortened))")
        }
        .font(.caption)
        .foregroundStyle(.white)
        .padding(.vertical, 6)
        .padding(.horizontal, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.red.opacity(0.85))
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }
}
