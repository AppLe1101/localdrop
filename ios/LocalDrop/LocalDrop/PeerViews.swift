//
//  PeerViews.swift
//  LocalDrop
//
//  Created by Ablochko on 12.11.2026.
//

import SwiftUI

func peerHue(_ s: String) -> Double {
    var h = 7
    for u in s.unicodeScalars { h = (h &* 31 &+ Int(u.value)) % 360 }
    return Double(h) / 360.0
}

func activeTransfer(for peer: FfiPeer, in items: [TransferItem]) -> TransferItem? {
    items.first { $0.isSend && $0.peerName == peer.name && ($0.status == .waiting || $0.status == .active) }
}

private func fraction(_ t: TransferItem?) -> Double {
    guard let t, t.status == .active, t.size > 0 else { return 0 }
    return Double(t.done) / Double(t.size)
}

struct PeerCircle: View {
    let peer: FfiPeer
    var transfer: TransferItem? = nil
    var selecting = false
    var selected = false
    var scale: CGFloat = 1
    var showName = true
    var done = false

    var body: some View {
        let d = 64 * scale
        let f = fraction(transfer)
        let waiting = transfer != nil && f == 0
        VStack(spacing: 5) {
            ZStack {
                AvatarView(name: peer.name, size: d)
                    .scaleEffect(selected ? 1.08 : 1)
                    .shadow(color: Color.brand.opacity(transfer != nil || selected ? 0.55 : 0), radius: 14)
                if transfer != nil { ProgressRing(fraction: f, waiting: waiting, size: d + 14) }
            }
            .frame(width: d + 16, height: d + 16)
            .overlay(alignment: .topTrailing) {
                if selecting {
                    Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                        .font(.system(size: max(16, 22 * scale)))
                        .foregroundStyle(selected ? Color.brand : .secondary)
                        .background(.background, in: Circle())
                        .symbolEffect(.bounce, value: selected)
                }
            }
            .overlay(alignment: .bottomTrailing) {
                if done {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: max(16, 24 * scale)))
                        .foregroundStyle(.white, .green)
                        .background(.background, in: Circle())
                        .symbolEffect(.bounce, value: done)
                        .transition(.scale(scale: 0.2).combined(with: .opacity))
                }
            }
            .opacity(selecting && !selected ? 0.6 : 1)

            if showName {
                Text(peer.name).font(.footnote.weight(.medium)).lineLimit(1).frame(maxWidth: 96)
                    .padding(.horizontal, 9).padding(.vertical, 3)
                    .glassCapsule()
                Text(transfer == nil ? " " : (waiting ? "Ждём ответа…" : "\(Int(f * 100))%"))
                    .font(.caption2.weight(.semibold)).monospacedDigit()
                    .foregroundStyle(Color.brand)
                    .contentTransition(.numericText())
                    .animation(.snappy, value: Int(f * 100))
            }
        }
        .animation(.spring(duration: 0.4, bounce: 0.45), value: selected)
        .animation(.spring(duration: 0.4, bounce: 0.5), value: done)
        .contentShape(Rectangle())
    }
}

struct PeerRowView: View {
    let peer: FfiPeer
    var transfer: TransferItem? = nil
    var selecting = false
    var selected = false

    var body: some View {
        let f = fraction(transfer)
        HStack(spacing: 12) {
            if selecting {
                Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                    .font(.title3).foregroundStyle(selected ? Color.accentColor : .secondary)
            }
            AvatarView(name: peer.name, size: 42)
            VStack(alignment: .leading, spacing: 2) {
                Text(peer.name).font(.body.weight(.medium)).lineLimit(1)
                Text(transfer == nil
                     ? (peer.addrs.first?.split(separator: ":").first.map(String.init) ?? "")
                     : (f == 0 ? "Ждём ответа…" : "\(Int(f * 100))%"))
                    .font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            if transfer != nil { ProgressRing(fraction: f, waiting: f == 0, size: 26) }
            else if !selecting { Image(systemName: "chevron.right").font(.footnote).foregroundStyle(.tertiary) }
        }
        .padding(.horizontal, 16).padding(.vertical, 10)
        .contentShape(Rectangle())
    }
}
