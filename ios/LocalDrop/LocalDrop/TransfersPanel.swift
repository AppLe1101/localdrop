//
//  TransfersPanel.swift
//  LocalDrop
//
//  Created by Ablochko on 12.11.2026.
//

import SwiftUI

private struct TGroup: Identifiable {
    let id: String
    var items: [TransferItem]
    var isBatch: Bool
}

private func makeGroups(_ items: [TransferItem]) -> [TGroup] {
    var out: [TGroup] = []
    var index: [UUID: Int] = [:]
    for t in items {
        if let b = t.batch {
            if let i = index[b] { out[i].items.append(t) }
            else { index[b] = out.count; out.append(TGroup(id: b.uuidString, items: [t], isBatch: true)) }
        } else {
            out.append(TGroup(id: "t\(t.id)", items: [t], isBatch: false))
        }
    }
    return out
}

struct TransfersPanel: View {
    @EnvironmentObject private var model: LocalDropModel
    let close: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Передачи").font(.headline)
                Spacer()
                Button("Очистить завершённые") { model.clearFinished() }.font(.footnote)
                Button { close() } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                    .padding(.leading, 6)
            }
            .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 4)

            if model.transfers.isEmpty {
                Text("Пока ничего не передавалось").font(.footnote).foregroundStyle(.secondary).padding(24)
            } else {
                List {
                    ForEach(makeGroups(model.transfers)) { g in
                        if g.isBatch && g.items.count > 1 {
                            DisclosureGroup { ForEach(g.items) { TransferRow(item: $0, showName: false) } } label: { GroupHeader(items: g.items) }
                        } else {
                            TransferRow(item: g.items[0], showName: true)
                        }
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .frame(maxHeight: 300)
            }
        }
        .glass(24)
    }
}

private struct GroupHeader: View {
    let items: [TransferItem]
    var body: some View {
        let done = items.filter { $0.status == .done }.count
        let bad = items.filter { $0.status == .failed || $0.status == .rejected || $0.status == .cancelled }.count
        let size = items.reduce(UInt64(0)) { $0 + $1.size }
        let got = items.reduce(UInt64(0)) { $0 + $1.done }
        let running = items.contains { $0.status == .active || $0.status == .waiting }
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Image(systemName: "paperplane.circle.fill").foregroundStyle(.tint)
                Text(Set(items.map(\.name)).count == 1 ? items[0].name : "Передач: \(items.count)").lineLimit(1)
                Spacer()
                Text("\(items.count) уст.").font(.caption).foregroundStyle(.secondary)
            }
            if running && size > 0 { ProgressView(value: Double(got), total: Double(size)) }
            HStack(spacing: 10) {
                if done > 0 { Text("✓ \(done)").foregroundStyle(.green) }
                if bad > 0 { Text("✕ \(bad)").foregroundStyle(.orange) }
            }
            .font(.caption)
        }
    }
}

struct TransferRow: View {
    @EnvironmentObject private var model: LocalDropModel
    let item: TransferItem
    var showName = true

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Image(systemName: item.isSend ? "arrow.up.circle.fill" : "arrow.down.circle.fill").foregroundStyle(.tint)
                Text(showName ? item.name : item.peerName).lineLimit(1)
                Spacer()
                Text(label).font(.caption).foregroundStyle(color)
                if item.isSend && (item.status == .waiting || item.status == .active) {
                    Button { model.cancel(item.id) } label: {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary).buttonStyle(.borderless)
                    }
                }
            }
            if showName { Text("\(item.isSend ? "Кому" : "От кого"): \(item.peerName)").font(.caption).foregroundStyle(.secondary) }
            if item.status == .active, item.size > 0 { ProgressView(value: Double(item.done), total: Double(item.size)) }
            if let e = item.error { Text(e).font(.caption).foregroundStyle(.red) }
        }
    }

    private var label: String {
        switch item.status {
        case .waiting: "Ожидание ответа…"
        case .active: item.size > 0 ? "\(item.done * 100 / item.size)%" : ""
        case .done: "Готово"
        case .rejected: "Отклонено"
        case .failed: "Ошибка"
        case .cancelled: "Отменено"
        }
    }
    private var color: Color {
        switch item.status {
        case .done: .green
        case .rejected: .orange
        case .failed: .red
        case .cancelled: .orange
        default: .secondary
        }
    }
}

struct ViewSwitch: View {
    @Binding var value: String
    @Namespace private var ns
    private let items: [(tag: String, icon: String, label: String)] = [
        ("radar", "dot.radiowaves.left.and.right", "Радар"),
        ("grid", "circle.grid.2x2", "Таблица"),
        ("list", "list.bullet", "Список"),
    ]

    var body: some View {
        HStack(spacing: 0) {
            ForEach(items, id: \.tag) { it in
                let on = value == it.tag
                Button {
                    Haptics.tick()
                    withAnimation(.spring(duration: 0.35, bounce: 0.3)) { value = it.tag }
                } label: {
                    Image(systemName: it.icon)
                        .font(.system(size: 15, weight: .medium))
                        .frame(width: 40, height: 32)
                        .foregroundStyle(on ? Color.white : Color.primary)
                        .background {
                            if on { Capsule().fill(Color.brand).matchedGeometryEffect(id: "sel", in: ns) }
                        }
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(it.label)
            }
        }
        .padding(2)
        .modifier(LegacyCapsule())
    }
}

private struct LegacyCapsule: ViewModifier {
    func body(content: Content) -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26, *) { content } else { content.background(.regularMaterial, in: Capsule()) }
        #else
        content.background(.regularMaterial, in: Capsule())
        #endif
    }
}
