//
//  HistoryStore.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import Foundation

struct HistoryRecord: Codable, Identifiable {
    var id = UUID()
    var date = Date()
    var isSend: Bool
    var name: String
    var size: UInt64
    var peer: String
    var status: String // "done" | "rejected" | "failed"
    var relPath: String?
    var error: String?
    var text: String?
    var batch: UUID?
}

final class HistoryStore {
    private let url: URL
    private let keep = 500

    init() {
        let dir = (try? FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                                appropriateFor: nil, create: true))?
            .appendingPathComponent("localdrop") ?? URL(fileURLWithPath: NSTemporaryDirectory())
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        url = dir.appendingPathComponent("history.json")
    }

    func load() -> [HistoryRecord] {
        guard let data = try? Data(contentsOf: url) else { return [] }
        return (try? JSONDecoder().decode([HistoryRecord].self, from: data)) ?? []
    }

    func save(_ items: [HistoryRecord]) {
        if let data = try? JSONEncoder().encode(Array(items.prefix(keep))) {
            try? data.write(to: url, options: .atomic)
        }
    }
}

func documentsURL() -> URL {
    FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
}

func relativeToDocuments(_ path: String) -> String? {
    let base = documentsURL().path + "/"
    return path.hasPrefix(base) ? String(path.dropFirst(base.count)) : nil
}
