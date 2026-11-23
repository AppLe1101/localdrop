//
//  IncomingSheet.swift
//  LocalDrop
//
//  Created by Ablochko on 12.11.2026.
//

import SwiftUI

struct IncomingSheet: View {
    let req: IncomingRequest
    let answer: (FfiDecision) -> Void

    var body: some View {
        ScrollView {
            VStack(spacing: 18) {
                AvatarView(name: req.knownName ?? req.sender, size: 76)
                VStack(spacing: 4) {
                    Text(req.knownName ?? req.sender).font(.title3.weight(.semibold))
                    Text(req.isText ? "хочет отправить вам текст" : "хочет отправить вам файлы").foregroundStyle(.secondary)
                }

                Group {
                    if req.isText {
                        Text(req.preview ?? "").frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        VStack(spacing: 2) {
                            Text(req.name).font(.headline).multilineTextAlignment(.center)
                            Text(ByteCountFormatter.string(fromByteCount: Int64(req.size), countStyle: .file))
                                .font(.subheadline).foregroundStyle(.secondary)
                        }
                        .frame(maxWidth: .infinity)
                    }
                }
                .padding(14)
                .surfaceCard(16)

                VStack(spacing: 6) {
                    if req.knownName == nil {
                        Label("Новое устройство", systemImage: "exclamationmark.triangle.fill")
                            .font(.footnote.weight(.medium)).foregroundStyle(.orange)
                    }
                    CodeDigits(code: req.code).font(.system(size: 38, weight: .bold, design: .rounded))
                        .monospacedDigit().foregroundStyle(.tint)
                    Text("Сверьте код с экраном отправителя\nОтпечаток: \(req.fpShort)")
                        .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity).padding(16)
                .surfaceCard(20)

                TimelineView(.animation(minimumInterval: 0.25)) { ctx in
                    let left = max(0, req.expires.timeIntervalSince(ctx.date))
                    VStack(spacing: 4) {
                        ProgressView(value: left, total: max(req.total, 1)).tint(left < 10 ? .red : .accentColor)
                        Text("Автоотклонение через \(Int(ceil(left))) с").font(.caption).foregroundStyle(.secondary)
                    }
                }

                VStack(spacing: 10) {
                    Button { answer(.acceptOnce) } label: { Text("Принять").frame(maxWidth: .infinity) }
                        .buttonStyle(PillButtonStyle(prominent: true))
                    Button { answer(.acceptAndTrust) } label: { Text("Принять и доверять").frame(maxWidth: .infinity) }
                        .buttonStyle(PillButtonStyle())
                    Button { answer(.reject) } label: { Text("Отклонить").frame(maxWidth: .infinity).foregroundStyle(.red) }
                        .buttonStyle(PillButtonStyle())
                }
                .controlSize(.large)
            }
            .padding(24)
        }
        .presentationDetents([.medium, .large])
        .onAppear { Haptics.warning() }
    }
}
