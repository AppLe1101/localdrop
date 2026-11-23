//
//  DeviceView.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import SwiftUI

struct DevicesView: View {
    @EnvironmentObject private var model: LocalDropModel
    @State private var renaming: FfiDevice?
    @State private var draft = ""
    @State private var forgetting: FfiDevice?

    private func online(_ d: FfiDevice) -> Bool { model.peers.contains { $0.fp == d.fingerprint } }

    var body: some View {
        NavigationStack {
            List {
                if model.devices.isEmpty {
                    ContentUnavailableView("Пока нет устройств", systemImage: "laptopcomputer.and.iphone",
                        description: Text("Устройство появится здесь после первой передачи"))
                } else {
                    Section(footer: Text("Доверенные устройства могут отправлять вам файлы без подтверждения.")) {
                        ForEach(model.devices, id: \.fingerprint) { d in
                            HStack(spacing: 12) {
                                ZStack {
                                    Circle().fill(Color(hue: peerHue(d.name), saturation: 0.6, brightness: 0.85))
                                    Text(String(d.name.prefix(1)).uppercased()).bold().foregroundStyle(.white)
                                }
                                .frame(width: 40, height: 40)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(d.name)
                                    HStack(spacing: 4) {
                                        Circle().fill(online(d) ? .green : .gray.opacity(0.5)).frame(width: 7, height: 7)
                                        Text(online(d) ? "В сети" : "Не в сети").font(.caption).foregroundStyle(.secondary)
                                    }
                                }
                                Spacer()
                                Toggle("Доверять", isOn: Binding(get: { d.trusted }, set: { model.setTrusted(d, $0) }))
                                    .labelsHidden()
                            }
                            .swipeActions {
                                Button("Забыть", role: .destructive) { forgetting = d }
                                Button("Имя") { draft = d.name; renaming = d }.tint(.blue)
                            }
                        }
                    }
                    .listRowBackground(Color.surface)
                }
            }
            .navigationTitle("Устройства")
            .scrollContentBackground(.hidden)
            .background(AppBackground())
        }
        .onAppear { model.refreshDevices() }
        .alert("Переименовать", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Имя", text: $draft)
            Button("Отмена", role: .cancel) {}
            Button("Сохранить") { if let d = renaming { model.rename(d, to: draft) } }
        }
        .confirmationDialog("Забыть устройство? При следующей передаче оно снова потребует подтверждения.",
                            isPresented: Binding(get: { forgetting != nil }, set: { if !$0 { forgetting = nil } }),
                            titleVisibility: .visible) {
            Button("Забыть", role: .destructive) { if let d = forgetting { model.forget(d) } }
        }
    }
}
