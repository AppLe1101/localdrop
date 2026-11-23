//
//  LocalNetProbe.swift
//  LocalDrop
//
//  Created by Ablochko on 15.11.2026.
//

import Network

enum NetAccess { case unknown, granted, denied }

final class LocalNetProbe {
    private var browser: NWBrowser?
    var onChange: ((NetAccess) -> Void)?

    func start() {
        browser?.cancel()
        let b = NWBrowser(for: .bonjour(type: "_localdrop._udp", domain: "local."), using: .udp)
        b.stateUpdateHandler = { [weak self] state in
            switch state {
            case .ready:
                self?.onChange?(.granted)
            case .waiting(let e), .failed(let e):
                if case .dns(let code) = e, code == -65570 { self?.onChange?(.denied) }
            default: break
            }
        }
        b.start(queue: .main)
        browser = b
    }

    func stop() { browser?.cancel(); browser = nil }
}
