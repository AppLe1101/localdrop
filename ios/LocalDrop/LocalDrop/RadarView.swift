//
//  RadarView.swift
//  LocalDrop
//
//  Created by Ablochko on 12.11.2026.
//

import SwiftUI

struct RadarLayout {
    var points: [CGPoint] = []
    var radii: [CGFloat] = []
    var scale: CGFloat = 1

    init(count n: Int) {
        guard n > 0 else { return }
        radii = n <= 6 ? [0.27] : n <= 15 ? [0.22, 0.38] : [0.18, 0.30, 0.41]
        let sum = radii.reduce(0, +)
        var counts = radii.map { Int((CGFloat(n) * $0 / sum).rounded(.down)) }
        var rest = n - counts.reduce(0, +)
        var i = radii.count - 1
        while rest > 0 {
            counts[i] += 1; rest -= 1
            i = (i - 1 + radii.count) % radii.count
        }
        for (ri, r) in radii.enumerated() {
            let c = counts[ri]
            guard c > 0 else { continue }
            let off = ri % 2 == 1 ? Double.pi / Double(c) : 0
            for k in 0..<c {
                let a = -Double.pi / 2 + off + Double(k) * 2 * Double.pi / Double(c)
                points.append(CGPoint(x: 0.5 + r * CGFloat(cos(a)), y: 0.5 + r * CGFloat(sin(a))))
            }
        }
        scale = n <= 5 ? 1 : n <= 9 ? 0.85 : n <= 15 ? 0.68 : n <= 22 ? 0.55 : 0.46
    }
}

struct PulseRings: View {
    @Environment(\.accessibilityReduceMotion) private var reduce
    @State private var go = false

    var body: some View {
        ZStack {
            if !reduce {
                ForEach(0..<3, id: \.self) { i in
                    Circle()
                        .stroke(Color.accentColor.opacity(0.5), lineWidth: 1.5)
                        .scaleEffect(go ? 1 : 0.12)
                        .opacity(go ? 0 : 0.5)
                        .animation(.easeOut(duration: 4.2).repeatForever(autoreverses: false)
                            .delay(Double(i) * 1.4), value: go)
                }
            }
        }
        .onAppear { go = true }
    }
}

struct RadarView<Cell: View>: View {
    let peers: [FfiPeer]
    let myName: String
    var hidden = 0
    var onShowAll: () -> Void = {}
    @ViewBuilder let cell: (FfiPeer, RadarLayout) -> Cell
    @Environment(\.accessibilityReduceMotion) private var reduce
    @State private var sweep = false

    var body: some View {
        GeometryReader { geo in
            let side = min(geo.size.width, geo.size.height)
            let lay = RadarLayout(count: peers.count)
            let cx = geo.size.width / 2
            let cy = geo.size.height / 2
            ZStack {
                Circle().stroke(Color.accentColor.opacity(0.30), lineWidth: 1)
                    .background(Circle().fill(Color.accentColor.opacity(0.07)))
                    .frame(width: side * 0.94, height: side * 0.94).position(x: cx, y: cy)
                ForEach(Array(lay.radii.enumerated()), id: \.offset) { _, r in
                    Circle().stroke(Color.accentColor.opacity(0.24), lineWidth: 1)
                        .frame(width: side * r * 2, height: side * r * 2).position(x: cx, y: cy)
                }
                PulseRings().frame(width: side * 0.94, height: side * 0.94).position(x: cx, y: cy)
                if !reduce {
                    Circle()
                        .fill(AngularGradient(colors: [.clear, Color.brand.opacity(0.0), Color.brand.opacity(0.32)], center: .center))
                        .frame(width: side * 0.94, height: side * 0.94)
                        .rotationEffect(.degrees(sweep ? 360 : 0))
                        .position(x: cx, y: cy)
                        .allowsHitTesting(false)
                }
                AvatarView(name: myName, size: 62, me: true)
                    .background(Circle().fill(Color.accentColor.opacity(0.22)).frame(width: 82, height: 82))
                    .position(x: cx, y: cy)

                ForEach(Array(peers.enumerated()), id: \.element.id) { i, p in
                    if i < lay.points.count {
                        cell(p, lay)
                            .position(x: cx + (lay.points[i].x - 0.5) * side,
                                      y: cy + (lay.points[i].y - 0.5) * side)
                            .transition(.scale.combined(with: .opacity))
                    }
                }
            }
            .animation(.spring(duration: 0.6), value: peers.map(\.id))
            .overlay(alignment: .bottom) {
                if hidden > 0 {
                    Button("Ещё \(hidden): показать все", action: onShowAll)
                        .buttonStyle(.borderedProminent).controlSize(.small).padding(.bottom, 4)
                }
            }
        }
        .onAppear {
            guard !reduce else { return }
            withAnimation(.linear(duration: 6).repeatForever(autoreverses: false)) { sweep = true }
        }
    }
}
