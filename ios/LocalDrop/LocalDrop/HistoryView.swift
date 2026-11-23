//
//  HistoryView.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import SwiftUI

private enum GKind { case batch, burst, single }
private struct HGroup: Identifiable { let id: String; var items: [HistoryRecord]; var kind: GKind }

private func ru(_ n: Int, _ one: String, _ few: String, _ many: String) -> String {
    let a = n % 10, b = n % 100
    let w = a == 1 && b != 11 ? one : ((2...4).contains(a) && !(12...14).contains(b) ? few : many)
    return "\(n) \(w)"
}

private func makeGroups(_ rows: [HistoryRecord]) -> [HGroup] {
    var out: [HGroup] = []
    var byBatch: [UUID: Int] = [:]
    var burst: Int?
    for r in rows {
        if let b = r.batch {
            if let i = byBatch[b] { out[i].items.append(r) }
            else { byBatch[b] = out.count; out.append(HGroup(id: "b" + b.uuidString, items: [r], kind: .batch)) }
            burst = nil
            continue
        }
        if let i = burst, let last = out[i].items.last, last.isSend == r.isSend,
           last.peer == r.peer, last.date.timeIntervalSince(r.date) < 90 {
            out[i].items.append(r)
            out[i].kind = .burst
        } else {
            burst = out.count
            out.append(HGroup(id: "s" + r.id.uuidString, items: [r], kind: .single))
        }
    }
    return out
}

struct HistoryView: View {
    @EnvironmentObject private var model: LocalDropModel
    @State private var filter = 0
    @State private var range: ClosedRange<Date>?
    @State private var showCal = false
    @State private var open: Set<String> = []
    @State private var confirmClear = false

    private let cal = Calendar.current

    private var shown: [HistoryRecord] {
        model.history.filter { r in
            guard filter == 0 || (filter == 1) == r.isSend else { return false }
            guard let rg = range else { return true }
            let day = cal.startOfDay(for: r.date)
            return day >= cal.startOfDay(for: rg.lowerBound) && day <= cal.startOfDay(for: rg.upperBound)
        }
    }

    private var days: [(Date, [HGroup])] {
        let groups = makeGroups(shown)
        let dict = Dictionary(grouping: groups) { cal.startOfDay(for: $0.items[0].date) }
        return dict.keys.sorted(by: >).map { ($0, dict[$0]!) }
    }

    private var marks: Set<Date> { Set(model.history.map { cal.startOfDay(for: $0.date) }) }

    private func dayTitle(_ d: Date) -> String {
        if cal.isDateInToday(d) { return "Сегодня" }
        if cal.isDateInYesterday(d) { return "Вчера" }
        return d.formatted(date: .long, time: .omitted)
    }

    private var rangeLabel: String {
        guard let r = range else { return "Все даты" }
        let a = r.lowerBound.formatted(.dateTime.day().month(.abbreviated))
        let b = r.upperBound.formatted(.dateTime.day().month(.abbreviated))
        return cal.isDate(r.lowerBound, inSameDayAs: r.upperBound) ? a : "\(a) – \(b)"
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Picker("Фильтр", selection: $filter) {
                        Text("Все").tag(0); Text("Отправленные").tag(1); Text("Полученные").tag(2)
                    }
                    .pickerStyle(.segmented)
                    HStack {
                        Button { showCal = true } label: { Label(rangeLabel, systemImage: "calendar") }
                            .buttonStyle(.bordered)
                            .tint(range == nil ? .secondary : .accentColor)
                        if range != nil { Button("Сбросить") { range = nil }.font(.footnote) }
                        Spacer()
                    }
                }
                .listRowBackground(Color.surface)

                if shown.isEmpty {
                    ContentUnavailableView(range == nil ? "История пуста" : "За этот период ничего нет",
                                           systemImage: "clock",
                                           description: Text("Здесь появятся ваши передачи"))
                        .listRowBackground(Color.clear)
                }

                ForEach(days, id: \.0) { day, groups in
                    Section(dayTitle(day)) {
                        ForEach(groups) { g in
                            if g.items.count == 1 {
                                HistoryRow(r: g.items[0])
                                    .swipeActions { Button("Удалить", role: .destructive) { model.deleteRecords([g.items[0].id]) } }
                            } else {
                                DisclosureGroup(isExpanded: Binding(
                                    get: { open.contains(g.id) },
                                    set: { if $0 { open.insert(g.id) } else { open.remove(g.id) } })
                                ) {
                                    ForEach(g.items) { r in
                                        HistoryRow(r: r, byPeer: g.kind == .batch)
                                            .swipeActions { Button("Удалить", role: .destructive) { model.deleteRecords([r.id]) } }
                                    }
                                } label: { GroupHeader(g: g) }
                                .swipeActions { Button("Удалить", role: .destructive) { model.deleteRecords(g.items.map(\.id)) } }
                            }
                        }
                    }
                    .listRowBackground(Color.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(AppBackground())
            .navigationTitle("История")
            .toolbar {
                if !model.history.isEmpty { Button("Очистить") { confirmClear = true } }
            }
            .confirmationDialog("Удалить всю историю? Сами файлы останутся.", isPresented: $confirmClear,
                                titleVisibility: .visible) {
                Button("Очистить", role: .destructive) { model.clearHistory() }
            }
            .sheet(isPresented: $showCal) {
                RangeCalendar(range: $range, marks: marks).presentationDetents([.medium])
            }
        }
    }
}

private struct GroupHeader: View {
    let g: HGroup
    var body: some View {
        let it = g.items
        let done = it.filter { $0.status == "done" }.count
        let rej = it.filter { $0.status == "rejected" || $0.status == "cancelled" }.count
        let bad = it.filter { $0.status == "failed" }.count
        let peers = Array(Set(it.map(\.peer))).sorted()
        let names = Set(it.map { $0.text != nil ? "Текст" : $0.name })
        let who = it[0].isSend ? "Кому" : "От кого"
        let title = g.kind == .batch
            ? (names.count == 1 ? names.first! : "Передач: \(it.count)")
            : ru(it.count, "файл", "файла", "файлов")
        let sub = g.kind == .batch
            ? "\(who): \(peers.prefix(3).joined(separator: ", "))\(peers.count > 3 ? " и ещё \(peers.count - 3)" : "") · \(ru(peers.count, "устройство", "устройства", "устройств"))"
            : "\(who): \(peers.first ?? "")"
        HStack(spacing: 12) {
            Image(systemName: it[0].isSend ? "arrow.up.circle.fill" : "arrow.down.circle.fill")
                .font(.title2).foregroundStyle(.tint)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).lineLimit(1)
                Text("\(sub) · \(it[0].date.formatted(date: .omitted, time: .shortened))")
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            HStack(spacing: 6) {
                if done > 0 { Text("✓ \(done)").foregroundStyle(.green) }
                if rej > 0 { Text("✕ \(rej)").foregroundStyle(.orange) }
                if bad > 0 { Text("! \(bad)").foregroundStyle(.red) }
            }
            .font(.caption)
        }
    }
}

struct HistoryRow: View {
    let r: HistoryRecord
    var byPeer = false

    private var fileURL: URL? {
        guard let rel = r.relPath else { return nil }
        let u = documentsURL().appendingPathComponent(rel)
        return FileManager.default.fileExists(atPath: u.path) ? u : nil
    }

    private var label: String {
        r.text.map { "Текст: " + $0.replacingOccurrences(of: "\n", with: " ").prefix(50) } ?? r.name
    }

    var body: some View {
        HStack(spacing: 12) {
            if !byPeer {
                Image(systemName: r.isSend ? "arrow.up.circle.fill" : "arrow.down.circle.fill")
                    .font(.title2).foregroundStyle(r.status == "done" ? Color.accentColor : .secondary)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(byPeer ? r.peer : label).lineLimit(1)
                Text((byPeer ? label : "\(r.isSend ? "Кому" : "От кого"): \(r.peer)") +
                     " · " + r.date.formatted(date: .omitted, time: .shortened))
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                if let e = r.error { Text(e).font(.caption).foregroundStyle(.red) }
            }
            Spacer()
            switch r.status {
            case "rejected": Text("Отклонено").font(.caption).foregroundStyle(.orange)
            case "failed": Text("Ошибка").font(.caption).foregroundStyle(.red)
            case "cancelled": Text("Отменено").font(.caption).foregroundStyle(.orange)
            default:
                if let t = r.text {
                    Button { UIPasteboard.general.string = t } label: { Image(systemName: "doc.on.doc") }
                        .buttonStyle(.borderless)
                } else if let u = fileURL {
                    ShareLink(item: u) { Image(systemName: "square.and.arrow.up") }.buttonStyle(.borderless)
                }
            }
        }
    }
}

struct RangeCalendar: View {
    @Binding var range: ClosedRange<Date>?
    let marks: Set<Date>
    @Environment(\.dismiss) private var dismiss
    @State private var month = Date()
    @State private var anchor: Date?

    private var cal: Calendar { var c = Calendar.current; c.firstWeekday = 2; return c }
    private var today: Date { cal.startOfDay(for: Date()) }

    private var first: Date { cal.date(from: cal.dateComponents([.year, .month], from: month))! }
    private var cells: [Date?] {
        let lead = (cal.component(.weekday, from: first) - cal.firstWeekday + 7) % 7
        let n = cal.range(of: .day, in: .month, for: first)!.count
        return Array(repeating: nil, count: lead) + (0..<n).map { cal.date(byAdding: .day, value: $0, to: first) }
    }

    private func pick(_ d: Date) {
        if let a = anchor { range = min(a, d)...max(a, d); anchor = nil }
        else { anchor = d; range = d...d }
    }
    private func preset(_ back: Int, _ to: Int = 0) {
        range = cal.date(byAdding: .day, value: -back, to: today)!...cal.date(byAdding: .day, value: -to, to: today)!
        anchor = nil
    }

    var body: some View {
        VStack(spacing: 12) {
            HStack(spacing: 8) {
                Button("Сегодня") { preset(0) }
                Button("Вчера") { preset(1, 1) }
                Button("7 дней") { preset(6) }
                Button("30 дней") { preset(29) }
            }
            .buttonStyle(.bordered).font(.footnote)

            HStack {
                Button { month = cal.date(byAdding: .month, value: -1, to: month)! } label: { Image(systemName: "chevron.left") }
                Spacer()
                Text(first.formatted(.dateTime.month(.wide).year())).font(.headline)
                Spacer()
                Button { month = cal.date(byAdding: .month, value: 1, to: month)! } label: { Image(systemName: "chevron.right") }
            }
            .padding(.horizontal)

            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 0), count: 7), spacing: 4) {
                ForEach(["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"], id: \.self) {
                    Text($0).font(.caption2).foregroundStyle(.secondary)
                }
                ForEach(Array(cells.enumerated()), id: \.offset) { _, d in
                    if let d { dayCell(d) } else { Color.clear.frame(height: 38) }
                }
            }
            .padding(.horizontal, 8)

            HStack {
                Button("Все даты") { range = nil; anchor = nil; dismiss() }
                Spacer()
                Button("Готово") { dismiss() }.buttonStyle(.borderedProminent)
            }
            .padding(.horizontal)
            Spacer(minLength: 0)
        }
        .padding(.top, 16)
        .onAppear { if let r = range { month = r.upperBound } }
    }

    private func dayCell(_ d: Date) -> some View {
        let inR = range.map { d >= cal.startOfDay(for: $0.lowerBound) && d <= cal.startOfDay(for: $0.upperBound) } ?? false
        let edge = range.map { cal.isDate(d, inSameDayAs: $0.lowerBound) || cal.isDate(d, inSameDayAs: $0.upperBound) } ?? false
        return Button { pick(d) } label: {
            VStack(spacing: 2) {
                Text("\(cal.component(.day, from: d))")
                    .font(.system(size: 15, weight: cal.isDateInToday(d) ? .bold : .regular).monospacedDigit())
                Circle().fill(marks.contains(d) ? (edge ? Color.white : Color.accentColor) : .clear).frame(width: 4, height: 4)
            }
            .frame(maxWidth: .infinity, minHeight: 38)
            .foregroundStyle(edge ? Color.white : .primary)
            .background(edge ? Color.accentColor : (inR ? Color.accentColor.opacity(0.18) : .clear), in: Capsule())
        }
        .buttonStyle(.plain)
    }
}
