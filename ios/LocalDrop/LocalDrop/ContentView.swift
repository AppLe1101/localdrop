//
//  ContentView.swift
//  LocalDrop
//
//  Created by Ablochko on 08.11.2026.
//

import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @Namespace private var ns
    @EnvironmentObject private var model: LocalDropModel
    @AppStorage("peerView") private var peerView = "radar"
    @State private var importing = false
    @State private var importTarget: FfiPeer?
    @State private var multiPending = false
    @State private var query = ""
    @State private var stagedOpen = false
    @State private var codesOpen = true
    @State private var showTransfers = false
    @State private var composing = false
    @State private var draft = ""

    private var visible: [FfiPeer] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return q.isEmpty ? model.peers : model.peers.filter { $0.name.lowercased().contains(q) }
    }
    private var awaiting: [TransferItem] { model.transfers.filter { $0.code != nil } }
    private var activeCount: Int { model.transfers.filter { $0.status == .waiting || $0.status == .active }.count }
    private var allSelected: Bool { !visible.isEmpty && visible.allSatisfy { model.selected.contains($0.id) } }

    var body: some View {
        NavigationStack {
            ZStack {
                AppBackground()
                mainContent
            }
            .overlay(alignment: .bottom) { dock }
            .navigationTitle("LocalDrop")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    ViewSwitch(value: $peerView)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Text(model.myName).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item, .folder],
                      allowsMultipleSelection: true) { result in
            if case .success(let urls) = result, !urls.isEmpty {
                if multiPending { model.sendFiles(urls, to: model.selectedPeers) }
                else if let p = importTarget { model.send(urls: urls, to: p) }
                else { model.stage(urls: urls) }
            }
            multiPending = false
            importTarget = nil
        }
        .alert("Текст для отправки", isPresented: $composing) {
            TextField("Введите текст", text: $draft)
            Button("Отмена", role: .cancel) {}
            Button("Далее") { if !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { model.stage(text: draft) } }
        } message: { Text("Затем выберите устройство") }
        .onChange(of: model.staged.count) { _, n in if n < 2 { stagedOpen = false } }
    }

    // MARK: Content
    @ViewBuilder private var mainContent: some View {
        if model.netAccess == .denied {
            ContentUnavailableView {
                Label("Нет доступа к локальной сети", systemImage: "network.slash")
            } description: {
                Text("Без этого разрешения LocalDrop не видит другие устройства. Откройте настройки -> Приложения -> LocalDrop и включите «Локальная сеть»")
            } actions: {
                Button("Открыть настройки") {
                    if let u = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(u) }
                }
                .buttonStyle(.borderedProminent)
            }
        } else if !model.onLAN && model.peers.isEmpty {
            ContentUnavailableView {
                Label("Нет подключения к Wi-Fi", systemImage: "wifi.slash")
            } description: {
                Text("Подключитесь к той же сети, что и ваши устройства")
            } actions: {
                Button("Открыть настройки") {
                    if let u = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(u) }
                }
            }
        } else {
            switch peerView {
            case "grid": gridView
            case "list": listView
            default: radar
            }
        }
    }

    private var radar: some View {
        
        VStack(spacing: 8) {
            if model.peers.count > 6 { SearchField(text: $query).padding(.horizontal) }
            RadarView(peers: Array(visible.prefix(30)), myName: model.myName,
                      hidden: max(0, visible.count - 30), onShowAll: { peerView = "grid" }) { p, lay in
                cell(p) {
                    PeerCircle(peer: p, transfer: activeTransfer(for: p, in: model.transfers),
                               selecting: model.multi, selected: model.selected.contains(p.id),
                               scale: lay.scale, showName: lay.scale >= 0.6, done: model.justSent.contains(p.name))
                }
            }
            .overlay(alignment: .bottom) {
                if model.peers.isEmpty {
                    Text("Ищем устройства рядом…\nЗапустите LocalDrop на другом устройстве в этой же сети")
                        .font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.center).padding(.bottom, 8)
                } else if visible.isEmpty {
                    Text("Ничего не найдено").font(.footnote).foregroundStyle(.secondary).padding(.bottom, 8)
                }
            }
        }
        .padding(.bottom, 84)
    }

    private var gridView: some View {
        ScrollView {
            VStack(spacing: 16) {
                if model.peers.count > 6 { SearchField(text: $query) }
                if visible.isEmpty { Text("Ничего не найдено").foregroundStyle(.secondary).padding(.top, 40) }
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 100), spacing: 8)], spacing: 14) {
                    ForEach(visible, id: \.id) { p in
                        cell(p) {
                            PeerCircle(peer: p, transfer: activeTransfer(for: p, in: model.transfers),
                                       selecting: model.multi, selected: model.selected.contains(p.id), done: model.justSent.contains(p.name))
                            
                        }
                    }
                }
            }
            .padding(.horizontal).padding(.top, 8)
        }
        .contentMargins(.bottom, 160, for: .scrollContent)
    }

    private var listView: some View {
        ScrollView {
            VStack(spacing: 12) {
                if model.peers.count > 6 { SearchField(text: $query) }
                if visible.isEmpty { Text("Ничего не найдено").foregroundStyle(.secondary).padding(.top, 40) }
                LazyVStack(spacing: 0) {
                    ForEach(Array(visible.enumerated()), id: \.element.id) { i, p in
                        if i > 0 { Divider().padding(.leading, 70) }
                        cell(p) {
                            PeerRowView(peer: p, transfer: activeTransfer(for: p, in: model.transfers),
                                        selecting: model.multi, selected: model.selected.contains(p.id))
                        }
                    }
                }
                .glass(20)
            }
            .padding(.horizontal).padding(.top, 8)
        }
        .contentMargins(.bottom, 160, for: .scrollContent)
    }

    private func cell<C: View>(_ p: FfiPeer, @ViewBuilder _ label: () -> C) -> some View {
        Button { tap(p) } label: { label() }
            .buttonStyle(.plain)
            .sensoryFeedback(.impact(weight: .light), trigger: model.selected)
            .contextMenu {
                if activeTransfer(for: p, in: model.transfers) != nil {
                    Button("Отменить отправку", systemImage: "xmark.circle", role: .destructive) { model.cancelPeer(p) }
                }
                Button("Отправить текст из буфера", systemImage: "doc.on.clipboard") {
                    if let s = UIPasteboard.general.string { model.sendText(s, to: p) }
                }
            }
    }

    private func tap(_ peer: FfiPeer) {
        Haptics.tick()
        if model.multi { model.toggle(peer) }
        else if model.hasStaged { model.sendStaged(to: [peer]) }
        else { importTarget = peer; importing = true }
    }

    private func sendSelected() {
        if model.hasStaged { model.sendStaged(to: model.selectedPeers) }
        else { multiPending = true; importing = true }
    }

    // MARK: dock
    private var dock: some View {
        VStack(spacing: 10) {
            if !awaiting.isEmpty { codesPanel.transition(.move(edge: .bottom).combined(with: .opacity).combined(with: .scale(scale: 0.95))) }
            if showTransfers {
                TransfersPanel { showTransfers = false }
                    .transition(.move(edge: .bottom).combined(with: .opacity).combined(with: .scale(scale: 0.95)))
            }
            if model.hasStaged {
                stagedPanel.transition(.move(edge: .bottom).combined(with: .opacity).combined(with: .scale(scale: 0.95)))
            }
            actionRow
        }
        .padding(.horizontal, 14).padding(.bottom, 8)
        .animation(.spring(duration: 0.4, bounce: 0.3), value: showTransfers)
        .animation(.spring(duration: 0.4, bounce: 0.3), value: model.hasStaged)
        .animation(.spring(duration: 0.4, bounce: 0.3), value: awaiting.count)
        .animation(.spring(duration: 0.45, bounce: 0.25), value: model.multi)
    }

    private var codesPanel: some View {
        VStack(spacing: 6) {
            Button { withAnimation { codesOpen.toggle() } } label: {
                HStack {
                    Text("Ждём подтверждения: \(awaiting.count)").font(.footnote.weight(.semibold))
                    Spacer()
                    Image(systemName: "chevron.down").font(.caption)
                        .rotationEffect(.degrees(codesOpen ? 0 : -90))
                }
            }
            .buttonStyle(.plain)
            if codesOpen {
                ScrollView {
                    VStack(spacing: 6) {
                        ForEach(awaiting) { t in
                            HStack {
                                Text(t.peerName).font(.subheadline).lineLimit(1)
                                Spacer()
                                Text(t.code ?? "").font(.system(.title3, design: .rounded).weight(.bold))
                                    .monospacedDigit().foregroundStyle(.tint)
                                Button {model.cancel(t.id)} label: {
                                    Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }
                .frame(maxHeight: 120)
                Text("Код должен совпадать с кодом на экране получателя")
                    .font(.caption2).foregroundStyle(.secondary)
            }
        }
        .padding(12)
        .glass(20)
    }

    private var stagedPanel: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: model.stagedText != nil ? "text.alignleft" : (model.staged.count > 1 ? "doc.on.doc" : "doc"))
                    .foregroundStyle(.tint)
                Text(model.stagedLabel).font(.footnote.weight(.medium)).lineLimit(1)
                Spacer()
                if model.staged.count > 1 {
                    Button { withAnimation { stagedOpen.toggle() } } label: {
                        Image(systemName: "chevron.up").rotationEffect(.degrees(stagedOpen ? 180 : 0))
                    }
                    .accessibilityLabel("Список файлов")
                }
                Button { model.clearStaged() } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                    .accessibilityLabel("Очистить")
            }
            .padding(10)

            if stagedOpen && model.staged.count > 1 {
                ScrollView {
                    VStack(spacing: 0) {
                        ForEach(model.staged, id: \.self) { u in
                            HStack(spacing: 8) {
                                Image(systemName: "doc").foregroundStyle(.secondary)
                                Text(u.lastPathComponent).font(.footnote).lineLimit(1)
                                Spacer()
                                Button { model.removeStaged(u) } label: {
                                    Image(systemName: "xmark").font(.caption).foregroundStyle(.secondary)
                                }
                            }
                            .padding(.horizontal, 12).padding(.vertical, 7)
                        }
                    }
                }
                .frame(maxHeight: 160)
            }
        }
        .glass(22, tint: Color.brand.opacity(0.25))
    }

    private var actionRow: some View {
        GlassGroup(spacing: 14) {
            HStack(spacing: 10) {
                if model.multi {
                    Button(allSelected ? "Снять все" : "Выбрать все") { model.selectAll(!allSelected, among: visible) }
                        .buttonStyle(PillButtonStyle()).glassID("all", in: ns)
                    Spacer()
                    Button("Отмена") { model.exitMulti() }
                        .buttonStyle(PillButtonStyle()).glassID("cancel", in: ns)
                    Button { sendSelected() } label: {
                        Label("\(model.selected.isEmpty ? "" : "(\(model.selected.count))")", systemImage: "paperplane.fill")
                    }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(model.selected.isEmpty)
                    .glassID("send", in: ns)
                } else {
                    dockIcon("plus", "Добавить файлы", id: "add") { importTarget = nil; importing = true }
                    PasteCircle(fill: .tintColor) { model.paste($0) }
                        .frame(width: 46, height: 46)
                        .glassCircle(interactive: true)
                        .accessibilityLabel("Вставить")
                    dockIcon("text.alignleft", "Текст", id: "text") { draft = ""; composing = true }
                    if model.peers.count > 1 { dockIcon("person.2", "Выбрать несколько", id: "multi") { model.multi = true } }
                    Spacer()
                    Button { withAnimation(.spring(duration: 0.4, bounce: 0.3)) { showTransfers.toggle() } } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "arrow.up.arrow.down")
                                .symbolEffect(.pulse, isActive: activeCount > 0)
                            if activeCount > 0 {
                                Text("\(activeCount)").font(.caption.weight(.bold)).monospacedDigit()
                                    .contentTransition(.numericText())
                            }
                        }
                        .padding(.horizontal, 16).frame(height: 46)
                        .foregroundStyle(showTransfers ? Color.white : Color.primary)
                        .glassCapsule(tint: showTransfers ? .brand : nil, interactive: true, solid: showTransfers)
                    }
                    .buttonStyle(.plain)
                    .glassID("transfers", in: ns)
                    .accessibilityLabel("Передачи")
                }
            }
        }
    }

    private func dockIcon(_ name: String, _ label: String, id: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: name).font(.system(size: 18, weight: .medium))
                .frame(width: 46, height: 46)
                .glassCircle(interactive: true)
        }
        .buttonStyle(.plain)
        .glassID(id, in: ns)
        .sensoryFeedback(.selection, trigger: model.multi)
        .accessibilityLabel(label)
    }
}
