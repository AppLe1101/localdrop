//
//  LocalDropModel.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import SwiftUI
import UIKit
internal import Combine
import UniformTypeIdentifiers
import Network

struct IncomingRequest: Identifiable {
    let id: UInt64
    let sender: String
    let knownName: String?
    let name: String
    let size: UInt64
    let isText: Bool
    let preview: String?
    let code: String
    let fpShort: String
    let expires: Date
    let total: TimeInterval
}

struct TransferItem: Identifiable {
    enum Status { case waiting, active, done, rejected, failed, cancelled }
    let id: UInt64
    var name: String
    var size: UInt64 = 0
    var done: UInt64 = 0
    var status: Status = .waiting
    var isSend: Bool
    var peerName: String = ""
    var code: String?
    var path: String?
    var error: String?
    var batch: UUID?
}

struct Banner: Identifiable {
    enum Kind { case info, success, warning, error }
    var id = UUID()
    var kind: Kind = .info
    var title: String
    var text: String?
    var copyText: String?
    var key: String?
    var count = 1
}

func errorText(_ e: Error) -> String {
    if case LdError.Failed(let msg) = e { return msg }
    return "\(e)"
}

enum ReceiveMode: String, CaseIterable {
    case everyone, trustedOnly, off
    var title: String {
        switch self {
        case .everyone: "Все"
        case .trustedOnly: "Только доверенные"
        case .off: "Выключено"
        }
    }
    var hint: String {
        switch self {
        case .everyone: "Неизвестные устройства потребуют вашего подтверждения"
        case .trustedOnly: "Остальные устройства получают отказ автоматически"
        case .off: "Ничего не принимать и скрыться из списков. Отправлять можно"
        }
    }
    var ffi: FfiVisibility {
        switch self {
        case .everyone: .everyone
        case .trustedOnly: .trustedOnly
        case .off: .off
        }
    }
}

@MainActor
final class LocalDropModel: ObservableObject, EventListener {
    @Published var peers: [FfiPeer] = []
    @Published var incoming: [IncomingRequest] = []
    @Published var transfers: [TransferItem] = [] {
        didSet {
            let keep = UserDefaults.standard.object(forKey: "keepAwake") as? Bool ?? true
            UIApplication.shared.isIdleTimerDisabled = keep && transfers.contains { $0.status == .active || $0.status == .waiting }
        }
    }
    @Published var banner: Banner?
    @Published var conflictPeer: FfiPeer?
    @Published var myName = UserDefaults.standard.string(forKey: "deviceName") ?? UIDevice.current.name
    @Published var fpShort = ""
    @Published var history: [HistoryRecord] = []
    @Published var devices: [FfiDevice] = []
    @Published var mode: ReceiveMode = .everyone
    
    @Published var staged: [URL] = []
    @Published var stagedText: String?
    @Published var multi = false
    @Published var selected: Set<String> = []
    @Published var inboxArrived = 0
    @Published var onLAN = true
    @Published var netAccess: NetAccess = .unknown
    @Published var justSent: Set<String> = []

    private var core: LocalDrop?
    private let discovery = Discovery()
    private var scoped: [UInt64: [URL]] = [:]
    private var conflicts: [UInt64: (peer: FfiPeer, urls: [URL])] = [:]
    private let store = HistoryStore()
    private var texts: [UInt64: String] = [:]
    private var port = 0
    private var fpFull = ""
    private let monitor = NWPathMonitor()
    private var probe = LocalNetProbe()

    func start() {
        guard core == nil else { return }
        do {
            let fm = FileManager.default
            let support = try fm.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("localdrop")
            let docs = fm.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("LocalDrop")

            let c = try LocalDrop(dataDir: support.path, downloadDir: docs.path, deviceName: myName)
            core = c
            fpShort = c.fingerprintShort()
            c.start(listener: self)
            
            port = Int(c.port())
            fpFull = c.fingerprintHex()
            history = store.load()
            mode = ReceiveMode(rawValue: UserDefaults.standard.string(forKey: "mode") ?? "") ?? .everyone
            c.setVisibility(v: mode.ffi)
            refreshDevices()

            discovery.onFound = { [weak self] p in self?.found(p) }
            discovery.onLost = { [weak self] id in self?.lost(id) }
            if mode != .off {
                discovery.start(name: myName, port: port, fp: fpFull)
            } else {
                discovery.startBrowsing(fp: fpFull)
            }
            probe.onChange = { [weak self] a in Task { @MainActor in self?.netAccess = a } }
            probe.start()
            Inbox.cleanup()
            importInbox()
            watchNetwork()
        } catch {
            banner = Banner(title: "Не удалось запустить", text: errorText(error))
        }
    }
    
    // MARK: Diagnostic
    func diagnostics() -> String {
        var sys = utsname(); uname(&sys)
        let machine = withUnsafePointer(to: &sys.machine) {
            $0.withMemoryRebound(to: CChar.self, capacity: 1) {
                String(cString: $0)
            }
        }
        let info = Bundle.main.infoDictionary
        let v = info?["CFBundleShortVersionString"] as? String ?? "?"
        let b = info?["CFBundleVersion"] as? String ?? "?"
        return """
            LocalDrop \(v) (\(b)), протокол v\(protocolVersion())
                iOS \(UIDevice.current.systemVersion), \(machine)
                Отпечаток: \(fpShort)
                Режим приёма: \(mode.rawValue)
                Доступ к локальной сети: \(netAccess)
                Wi-Fi: \(onLAN ? "да" : "нет")
                Устройств рядом: \(peers.count)
                Записей в истории: \(history.count) 
            """
    }

    // MARK: Devicse
    private func found(_ p: FfiPeer) {
        core?.peerFound(peer: p)
        peers.removeAll { $0.id == p.id }
        peers.append(p)
        peers.sort { $0.name.localizedCompare($1.name) == .orderedAscending }
    }

    private func lost(_ id: String) {
        core?.peerLost(id: id)
        peers.removeAll { $0.id == id }
        selected.remove(id)
    }
    
    func refreshDiscovery() {
        guard core != nil else { return }
        probe.start()
        for p in peers { core?.peerLost(id: p.id) }
        peers = []
        discovery.restart(name: myName, port: port, fp: fpFull, publish: mode != .off)
    }

    // MARK: Sending
    func send(urls: [URL], to peer: FfiPeer, allowConflict: Bool = false, batch: UUID? = nil) {
        guard let core else { return }
        let opened = urls.filter { $0.startAccessingSecurityScopedResource() }
        do {
            let id = try core.sendFiles(peerId: peer.id, paths: urls.map(\.path), allowConflict: allowConflict)
            scoped[id] = opened
            conflicts[id] = (peer, urls)
            let title = urls.count == 1 ? urls[0].lastPathComponent : "Элементов: \(urls.count)"
            transfers.insert(TransferItem(id: id, name: title, isSend: true, peerName: peer.name, batch: batch), at: 0)
        } catch {
            opened.forEach { $0.stopAccessingSecurityScopedResource() }
            banner = Banner(title: "Не удалось отправить", text: errorText(error))
        }
    }

    func sendText(_ text: String, to peer: FfiPeer, batch: UUID? = nil) {
        guard let core else { return }
        do {
            let id = try core.sendText(peerId: peer.id, text: text, allowConflict: false)
            transfers.insert(TransferItem(id: id, name: "Текст", isSend: true, peerName: peer.name, batch: batch), at: 0)
        } catch {
            banner = Banner(title: "Не удалось отправить", text: errorText(error))
        }
    }

    func confirmConflict(_ peer: FfiPeer) {
        guard let (_, urls) = conflicts.values.first(where: { $0.peer.id == peer.id }) else { return }
        send(urls: urls, to: peer, allowConflict: true)
    }

    // MARK: Incoming
    func respond(_ req: IncomingRequest, _ decision: FfiDecision) {
        incoming.removeAll { $0.id == req.id }
        if core?.respond(id: req.id, decision: decision) == false {
            banner = Banner(title: "Запрос уже неактуален")
        }
    }
    
    // MARK: Cancel
    func cancel(_ id: UInt64) {
        if core?.cancel(id: id) == false { notify("Отмнять уже поздно", kind: .info) }
    }
    func cancelPeer(_ p: FfiPeer) {
        transfers.filter { $0.isSend && $0.peerName == p.name && ($0.status == .waiting || $0.status == .active) }.forEach { cancel($0.id) }
    }

    // MARK: Core events
    nonisolated func onEvent(event: FfiEvent) {
        Task { @MainActor in self.handle(event) }
    }

    private func update(_ id: UInt64, _ change: (inout TransferItem) -> Void) {
        if let i = transfers.firstIndex(where: { $0.id == id }) { change(&transfers[i]) }
    }

    private func release(_ id: UInt64) {
        scoped.removeValue(forKey: id)?.forEach { $0.stopAccessingSecurityScopedResource() }
    }

    private func handle(_ event: FfiEvent) {
        switch event {
        case let .incoming(id, sender, kind, name, _, size, preview, fpShort, code, knownName, timeoutSecs):
            Haptics.warning()
            incoming.append(IncomingRequest(
                id: id, sender: sender, knownName: knownName, name: name, size: size,
                isText: { if case .text = kind { return true } else { return false } }(),
                preview: preview, code: code, fpShort: fpShort,
                expires: Date().addingTimeInterval(TimeInterval(timeoutSecs)),
                total: TimeInterval(timeoutSecs)
            ))

        case let .peerCode(id, code, _, trusted):
            update(id) { $0.code = trusted ? nil : code }

        case let .started(id, direction, name, size, peerName):
            incoming.removeAll { $0.id == id }
            let send: Bool = { if case .send = direction { return true } else { return false } }()
            if transfers.contains(where: { $0.id == id }) {
                update(id) { $0.name = name; $0.size = size; $0.status = .active; $0.peerName = peerName }
            } else {
                transfers.insert(TransferItem(id: id, name: name, size: size, status: .active, isSend: send, peerName: peerName), at: 0)
            }

        case let .progress(id, done, total):
            update(id) { $0.done = done; $0.size = total; $0.status = .active; $0.code = nil }

        case let .textReceived(id, text):
            texts[id] = text
            banner = Banner(title: "Текст получен", text: String(text.prefix(120)), copyText: text)
            notify("Текст получен",String(text.prefix(120)), kind: .success, copy: text)

        case let .finished(id, path):
            update(id) { $0.status = .done; $0.path = path; $0.code = nil }
            release(id)
            conflicts[id] = nil
            if let t = transfers.first(where: { $0.id == id }) {
                let text = texts.removeValue(forKey: id)
                log(HistoryRecord(isSend: t.isSend, name: t.name, size: t.size, peer: t.peerName, status: "done",
                                  relPath: path.flatMap(relativeToDocuments), text: text, batch: t.batch))
                if text == nil {
                    Haptics.success()
                    notify(t.isSend ? "Отправлено" : "Принято", t.name, kind: .success, key: t.isSend ? "send" : "recv")
                }
                if t.isSend {
                    withAnimation { _ = justSent.insert(t.peerName) }
                    Task {
                        try? await Task.sleep(for: .seconds(1.8))
                        withAnimation { _ = justSent.remove(t.peerName) }
                    }
                }
            }
            refreshDevices()

        case let .rejected(id):
            if let req = incoming.first(where: { $0.id == id }), !transfers.contains(where: { $0.id == id }) {
                log(HistoryRecord(isSend: false, name: req.isText ? "Текст" : req.name, size: req.size,
                                  peer: req.knownName ?? req.sender, status: "rejected"))
            } else if let t = transfers.first(where: { $0.id == id }) {
                log(HistoryRecord(isSend: t.isSend, name: t.name, size: t.size, peer: t.peerName, status: "rejected", batch: t.batch))
                notify("Отклонено", "\(t.peerName) не принял «\(t.name)»", kind: .warning, key: "rej")
            }
            incoming.removeAll { $0.id == id }
            update(id) { $0.status = .rejected; $0.code = nil }
            release(id)
            conflicts[id] = nil

        case let .failed(id, error):
            if let t = transfers.first(where: { $0.id == id }) {
                log(HistoryRecord(isSend: t.isSend, name: t.name, size: t.size, peer: t.peerName,
                                  status: "failed", error: error, batch: t.batch))
                Haptics.error()
                notify("Не удалось передать", error, kind: .error, key: "fail")
            }
            incoming.removeAll { $0.id == id }
            update(id) { $0.status = .failed; $0.error = error; $0.code = nil }
            release(id)
            conflicts[id] = nil

        case let .nameConflict(id):
            release(id)
            transfers.removeAll { $0.id == id }
            conflictPeer = conflicts[id]?.peer
        case let .cancelled(id):
            if let t = transfers.first(where: {$0.id == id}) {
                log(HistoryRecord(isSend: t.isSend, name: t.name, size: t.size, peer: t.peerName, status: "cancelled", batch: t.batch))
                notify(t.isSend ? "Отправка отменена" : "Отправитель отменил отправку", t.name, kind: .warning, key: "cancel")
            } else if let req = incoming.first(where: {$0.id == id}) {
                log(HistoryRecord(isSend: false, name: req.isText ? "Текст" : req.name, size: req.size, peer: req.knownName ?? req.sender, status: "cancelled"))
            }
            incoming.removeAll {$0.id == id}
            update(id) { $0.status = .cancelled; $0.code = nil }
            release(id)
            conflicts[id] = nil
        }
    }
    
    // MARK: Advanced
    func refreshDevices() {
        devices = (core?.devices() ?? []).sorted {
            ($0.trusted ? 0 : 1, $0.name.lowercased()) < ($1.trusted ? 0 : 1, $1.name.lowercased())
        }
    }

    func setTrusted(_ d: FfiDevice, _ on: Bool) {
        _ = try? core?.setTrusted(fingerprint: d.fingerprint, trusted: on)
        refreshDevices()
    }

    func rename(_ d: FfiDevice, to name: String) {
        let n = name.trimmingCharacters(in: .whitespaces)
        guard !n.isEmpty else { return }
        _ = try? core?.renameDevice(fingerprint: d.fingerprint, name: n)
        refreshDevices()
    }

    func forget(_ d: FfiDevice) {
        _ = try? core?.forgetDevice(prefix: d.fingerprint)
        refreshDevices()
    }

    func setMode(_ m: ReceiveMode) {
        mode = m
        UserDefaults.standard.set(m.rawValue, forKey: "mode")
        core?.setVisibility(v: m.ffi)
        if m == .off { discovery.unpublish() } else { discovery.publish(name: myName, port: port, fp: fpFull) }
    }

    func setName(_ name: String) {
        let n = name.trimmingCharacters(in: .whitespaces)
        guard !n.isEmpty, n.count <= 40 else { return }
        myName = n
        UserDefaults.standard.set(n, forKey: "deviceName")
        core?.setDeviceName(name: n)
        if mode != .off { discovery.publish(name: n, port: port, fp: fpFull) }
    }

    func deleteRecord(_ id: UUID) {
        history.removeAll { $0.id == id }
        store.save(history)
    }
    
    func deleteRecords(_ ids: [UUID]) {
        let s = Set(ids)
        history.removeAll { s.contains($0.id) }
        store.save(history)
    }

    func clearHistory() {
        history = []
        store.save(history)
    }

    private func log(_ r: HistoryRecord) {
        history.insert(r, at: 0)
        store.save(history)
    }
    
    var hasStaged: Bool { !staged.isEmpty || stagedText != nil }

    var stagedLabel: String {
        if let t = stagedText {
            let s = t.trimmingCharacters(in: .whitespacesAndNewlines)
            return "Текст: «\(s.prefix(30))\(s.count > 30 ? "…" : "")»"
        }
        guard let first = staged.first else { return "" }
        return staged.count == 1 ? first.lastPathComponent : "\(staged.count) объектов: \(first.lastPathComponent)…"
    }

    var selectedPeers: [FfiPeer] { peers.filter { selected.contains($0.id) } }

    func stage(urls: [URL]) {
        stagedText = nil
        for u in urls where !staged.contains(u) { staged.append(u) }
    }

    func stage(text: String) {
        staged = []
        stagedText = text
    }

    func clearStaged() {
        staged = []
        stagedText = nil
    }

    func toggle(_ p: FfiPeer) {
        if selected.contains(p.id) { selected.remove(p.id) } else { selected.insert(p.id) }
    }

    func selectAll(_ on: Bool, among list: [FfiPeer]? = nil) {
        let ids = (list ?? peers).map(\.id)
        if on { selected.formUnion(ids) } else { selected.subtract(ids) }
    }
    
    func exitMulti() {
        multi = false
        selected = []
    }

    func sendStaged(to targets: [FfiPeer]) {
        let text = stagedText, urls = staged
        let batch = targets.count > 1 ? UUID() : nil
        clearStaged()
        exitMulti()
        for p in targets {
            if let text { sendText(text, to: p, batch: batch) } else { send(urls: urls, to: p, batch: batch) }
        }
    }

    func sendFiles(_ urls: [URL], to targets: [FfiPeer]) {
        let batch = targets.count > 1 ? UUID() : nil
        exitMulti()
        for p in targets { send(urls: urls, to: p, batch: batch) }
    }

    func paste(_ providers: [NSItemProvider]) {
        for p in providers {
            if let ti = p.registeredTypeIdentifiers.first(where: { UTType($0)?.conforms(to: .image) == true }) {
                let ext = UTType(ti)?.preferredFilenameExtension ?? "png"
                p.loadDataRepresentation(forTypeIdentifier: ti) { data, _ in
                    guard let data else { return }
                    let name = "Buffer-\(Int(Date().timeIntervalSince1970))-\(UUID().uuidString.prefix(4)).\(ext)"
                    let url = FileManager.default.temporaryDirectory.appendingPathComponent(name)
                    guard (try? data.write(to: url)) != nil else { return }
                    Task { @MainActor in self.stage(urls: [url]) }
                }
            } else if p.canLoadObject(ofClass: String.self) {
                _ = p.loadObject(ofClass: String.self) { s, _ in
                    guard let s, !s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
                    Task { @MainActor in self.stage(text: s) }
                }
            }
        }
    }

    func importInbox() {
        let r = Inbox.take()
        if !r.files.isEmpty { stage(urls: r.files) }
        else if let t = r.text, !t.isEmpty { stage(text: t) }
        else { return }
        inboxArrived += 1
    }
    
    func notify(_ title: String, _ text: String? = nil, kind: Banner.Kind = .info, key: String? = nil, copy: String? = nil) {
        if let key, var b = banner, b.key == key {
            b.count += 1
            b.id = UUID()
            b.text = text
            banner = b
            return
        }
        banner = Banner(kind: kind, title: title, text: text, copyText: copy, key: key)
    }

    private func watchNetwork() {
        monitor.pathUpdateHandler = { [weak self] path in
            let lan = path.status == .satisfied &&
                (path.usesInterfaceType(.wifi) || path.usesInterfaceType(.wiredEthernet) || !path.usesInterfaceType(.cellular))
            Task { @MainActor in
                guard let self else { return }
                let back = lan && !self.onLAN
                self.onLAN = lan
                if back { self.refreshDiscovery() }
            }
        }
        monitor.start(queue: .global(qos: .utility))
    }

    func removeStaged(_ u: URL) { staged.removeAll { $0 == u } }

    func clearFinished() {
        transfers.removeAll { $0.status == .done || $0.status == .rejected || $0.status == .failed }
    }
}
