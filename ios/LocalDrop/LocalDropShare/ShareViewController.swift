//
//  ShareViewController.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import UIKit
import UniformTypeIdentifiers

final class ShareViewController: UIViewController {
    private let group = "group.ablochko.LocalDropApp"
    private let label = UILabel()
    private let spinner = UIActivityIndicatorView(style: .large)
    private let button = UIButton(type: .system)

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        label.text = "Подготовка…"
        label.numberOfLines = 0
        label.textAlignment = .center
        label.font = .preferredFont(forTextStyle: .headline)
        button.setTitle("Готово", for: .normal)
        button.isHidden = true
        button.addTarget(self, action: #selector(done), for: .touchUpInside)

        let stack = UIStackView(arrangedSubviews: [spinner, label, button])
        stack.axis = .vertical
        stack.spacing = 16
        stack.alignment = .center
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            stack.leadingAnchor.constraint(greaterThanOrEqualTo: view.leadingAnchor, constant: 24),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: view.trailingAnchor, constant: -24),
        ])
        spinner.startAnimating()
        Task { await run() }
    }

    @objc private func done() { extensionContext?.completeRequest(returningItems: nil) }

    private func finish(_ message: String) {
        spinner.stopAnimating()
        label.text = message
        button.isHidden = false
    }

    private func run() async {
        let fm = FileManager.default
        guard let root = fm.containerURL(forSecurityApplicationGroupIdentifier: group) else {
            finish("Не настроена общая папка приложения (App Group)")
            return
        }
        let id = UUID().uuidString
        let tmp = root.appendingPathComponent("Tmp/\(id)")
        try? fm.createDirectory(at: tmp, withIntermediateDirectories: true)

        let items = (extensionContext?.inputItems as? [NSExtensionItem]) ?? []
        var count = 0
        for p in items.flatMap({ $0.attachments ?? [] }) {
            if await save(p, to: tmp) { count += 1 }
        }
        guard count > 0 else {
            try? fm.removeItem(at: tmp)
            finish("Нечего отправлять")
            return
        }
        let inbox = root.appendingPathComponent("Inbox")
        try? fm.createDirectory(at: inbox, withIntermediateDirectories: true)
        try? fm.moveItem(at: tmp, to: inbox.appendingPathComponent(id))
        finish("Сохранено: \(count)\nОткройте LocalDrop и выберите устройство")
    }

    private func save(_ p: NSItemProvider, to dir: URL) async -> Bool {
        func has(_ t: UTType) -> Bool { p.hasItemConformingToTypeIdentifier(t.identifier) }
        let fileish = has(.fileURL) || has(.image) || has(.movie) || has(.pdf)

        if !fileish, has(.url) {
            if let u = try? await p.loadItem(forTypeIdentifier: UTType.url.identifier) as? URL {
                return writeText(u.absoluteString, to: dir)
            }
            return false
        }
        if !fileish, has(.plainText) {
            if let s = try? await p.loadItem(forTypeIdentifier: UTType.plainText.identifier) as? String {
                return writeText(s, to: dir)
            }
            return false
        }
        let ti = p.registeredTypeIdentifiers.first { UTType($0)?.conforms(to: .item) == true }
            ?? UTType.item.identifier
        return await withCheckedContinuation { cont in
            p.loadFileRepresentation(forTypeIdentifier: ti) { url, _ in
                guard let url else { cont.resume(returning: false); return }
                let sub = dir.appendingPathComponent(UUID().uuidString)
                do {
                    try FileManager.default.createDirectory(at: sub, withIntermediateDirectories: true)
                    try FileManager.default.copyItem(at: url, to: sub.appendingPathComponent(url.lastPathComponent))
                    cont.resume(returning: true)
                } catch {
                    cont.resume(returning: false)
                }
            }
        }
    }

    private func writeText(_ s: String, to dir: URL) -> Bool {
        guard !s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return false }
        return (try? s.write(to: dir.appendingPathComponent("text-\(UUID().uuidString).ldtext"),
                             atomically: true, encoding: .utf8)) != nil
    }
}
