//
//  Discovery.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import Foundation
import Darwin

/// Device detection through system Bonjour (without entitlement for multicast).
/// The format is compatible with the desktop version: service _localdrop._udp, TXT: name, ver, fp.
/// Works on the main thread: NetService requires a run loop.
final class Discovery: NSObject, NetServiceBrowserDelegate, NetServiceDelegate {
    private let type = "_localdrop._udp."
    private var browser = NetServiceBrowser()
    private var published: NetService?
    private var found: [String: NetService] = [:]
    private var myFp = ""

    var onFound: ((FfiPeer) -> Void)?
    var onLost: ((String) -> Void)?

    func start(name: String, port: Int, fp: String) {
        myFp = fp
        publish(name: name, port: port, fp: fp)
        browser.delegate = self
        browser.searchForServices(ofType: type, inDomain: "local.")
    }

    func publish(name: String, port: Int, fp: String) {
        published?.stop()
        let svc = NetService(domain: "local.", type: type, name: "ld-" + String(fp.prefix(16)), port: Int32(port))
        let txt: [String: Data] = [
            "name": Data(name.utf8),
            "ver": Data(String(protocolVersion()).utf8),
            "fp": Data(fp.utf8),
        ]
        svc.setTXTRecord(NetService.data(fromTXTRecord: txt))
        svc.publish()
        published = svc
    }
    
    func unpublish() {
        published?.stop()
        published = nil
    }

    func stop() {
        published?.stop()
        browser.stop()
    }
    
    func startBrowsing(fp: String) {
        myFp = fp
        browser.delegate = self
        browser.searchForServices(ofType: type, inDomain: "local.")
    }
    
    func restart(name: String, port: Int, fp: String, publish: Bool) {
        browser.stop()
        browser.delegate = nil
        found.removeAll()
        published?.stop()
        published = nil
        myFp = fp
        if publish { self.publish(name: name, port: port, fp: fp) }
        browser = NetServiceBrowser()
        browser.delegate = self
        browser.searchForServices(ofType: type, inDomain: "local.")
    }

    // MARK: NetServiceBrowserDelegate
    func netServiceBrowser(_ browser: NetServiceBrowser, didFind service: NetService, moreComing: Bool) {
        if service.name == "ld-" + String(myFp.prefix(16)) { return }
        found[service.name] = service
        service.delegate = self
        service.resolve(withTimeout: 5)
    }

    func netServiceBrowser(_ browser: NetServiceBrowser, didRemove service: NetService, moreComing: Bool) {
        found[service.name] = nil
        onLost?(service.name)
    }

    // MARK: NetServiceDelegate
    func netServiceDidResolveAddress(_ sender: NetService) {
        guard let data = sender.txtRecordData() else { return }
        let txt = NetService.dictionary(fromTXTRecord: data)
        func str(_ key: String) -> String? { txt[key].flatMap { String(data: $0, encoding: .utf8) } }

        let fp = str("fp")
        if fp == myFp { return }

        var addrs: [String] = []
        for a in sender.addresses ?? [] {
            a.withUnsafeBytes { raw in
                guard let base = raw.baseAddress else { return }
                let sa = base.assumingMemoryBound(to: sockaddr.self)
                guard Int32(sa.pointee.sa_family) == AF_INET else { return }
                var sin = base.assumingMemoryBound(to: sockaddr_in.self).pointee
                var buf = [CChar](repeating: 0, count: Int(INET_ADDRSTRLEN))
                inet_ntop(AF_INET, &sin.sin_addr, &buf, socklen_t(INET_ADDRSTRLEN))
                let ip = String(cString: buf)
                if !ip.hasPrefix("169.254.") { addrs.append("\(ip):\(sender.port)") }
            }
        }
        guard !addrs.isEmpty else { return }

        onFound?(FfiPeer(
            id: sender.name,
            name: str("name") ?? sender.name,
            addrs: addrs,
            fp: fp,
            ver: UInt16(str("ver") ?? "1") ?? 1
        ))
    }

    func netService(_ sender: NetService, didNotResolve errorDict: [String: NSNumber]) {
        found[sender.name] = nil
    }
}
