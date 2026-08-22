import SwiftUI

struct RoomSectionView: View {
    var title: String
    var devices: [DeviceSnapshot]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(.subheadline.bold())
                .foregroundStyle(.secondary)
                .padding(.top, 10)
                .padding(.bottom, 2)

            ForEach(devices) { device in
                NavigationLink(value: device.id) {
                    DeviceRowView(snapshot: device)
                }
                .buttonStyle(.plain)
                if device.id != devices.last?.id {
                    Divider()
                }
            }
        }
    }
}
