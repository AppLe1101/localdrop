//
//  PasteCircle.swift
//  LocalDrop
//
//  Created by Ablochko on 18.11.2026.
//

import SwiftUI
import UIKit
import UniformTypeIdentifiers

final class PasteHost: UIView {
    var onPaste: (([NSItemProvider]) -> Void)?
    private var control: UIPasteControl?

    override init(frame: CGRect) {
        super.init(frame: frame)
        pasteConfiguration = UIPasteConfiguration(
            acceptableTypeIdentifiers: [UTType.image.identifier, UTType.plainText.identifier])
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    override func paste(itemProviders: [NSItemProvider]) {
        onPaste?(itemProviders)
    }

    func install(size: CGFloat, fill: UIColor?) {
        control?.removeFromSuperview()
        let cfg = UIPasteControl.Configuration()
        cfg.displayMode = .iconOnly
        cfg.cornerStyle = .fixed
        cfg.cornerRadius = size / 2
        cfg.baseBackgroundColor = fill ?? .clear
        cfg.baseForegroundColor = fill == nil ? .label : .white
        let c = UIPasteControl(configuration: cfg)
        c.target = self
        c.translatesAutoresizingMaskIntoConstraints = false
        addSubview(c)
        NSLayoutConstraint.activate([
            c.leadingAnchor.constraint(equalTo: leadingAnchor),
            c.trailingAnchor.constraint(equalTo: trailingAnchor),
            c.topAnchor.constraint(equalTo: topAnchor),
            c.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        control = c
    }
}

struct PasteCircle: UIViewRepresentable {
    var size: CGFloat = 46
    var fill: UIColor? = nil
    var onPaste: ([NSItemProvider]) -> Void

    func makeUIView(context: Context) -> PasteHost {
        let v = PasteHost()
        v.install(size: size, fill: fill)
        v.onPaste = onPaste
        return v
    }

    func updateUIView(_ v: PasteHost, context: Context) {
        v.onPaste = onPaste
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: PasteHost, context: Context) -> CGSize? {
        CGSize(width: size, height: size)
    }
}
