//
//  Inbox.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import Foundation

enum Inbox {
    static let group = "group.ablochko.LocalDropApp"

    private static var root: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group)
    }

    static func take() -> (files: [URL], text: String?) {
        guard let base = root else { return ([], nil) }
        let fm = FileManager.default
        let inbox = base.appendingPathComponent("Inbox")
        let staged = base.appendingPathComponent("Staged")
        try? fm.createDirectory(at: staged, withIntermediateDirectories: true)

        var files: [URL] = []
        var text: String?
        for d in (try? fm.contentsOfDirectory(at: inbox, includingPropertiesForKeys: nil)) ?? [] {
            let dest = staged.appendingPathComponent(d.lastPathComponent)
            guard (try? fm.moveItem(at: d, to: dest)) != nil else { continue }
            guard let e = fm.enumerator(at: dest, includingPropertiesForKeys: [.isRegularFileKey],
                                        options: [.skipsHiddenFiles]) else { continue }
            for case let f as URL in e {
                guard (try? f.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else { continue }
                if f.pathExtension == "ldtext" { text = try? String(contentsOf: f, encoding: .utf8) }
                else { files.append(f) }
            }
        }
        return (files, text)
    }

    static func cleanup() {
        guard let base = root else { return }
        let fm = FileManager.default
        for sub in ["Staged", "Tmp", "Inbox"] {
            let dir = base.appendingPathComponent(sub)
            for u in (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.creationDateKey])) ?? [] {
                let c = (try? u.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? Date()
                if Date().timeIntervalSince(c) > 86_400 { try? fm.removeItem(at: u) }
            }
        }
    }
}
