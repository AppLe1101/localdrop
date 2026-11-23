//
//  SettingsView.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import SwiftUI

private let privacyURL = URL(string: "https://ablochko.com/info/privacy.html")!
private let issuesURL = URL(string: "https://github.com/AppLe1101/localdrop/issues/new")!
private let webpageURL = URL(string: "https://ablochko.com/products/LocalDrop")!

struct SettingsView: View {
    @EnvironmentObject private var model: LocalDropModel
    @AppStorage("theme") private var theme = "system"
    @AppStorage("haptics") private var haptics = true
    @AppStorage("keepAwake") private var keepAwake = true
    @AppStorage("onboarded") private var onboarded = true
    @State private var name = ""
    @State private var confirmClear = false
    @State private var copied = false

    private var version: String {
        let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?"
        let b = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "?"
        return "\(v) (\(b))"
    }

    private var net: (String, String, Color) {
        if model.netAccess == .denied { return ("Нет доступа к локальной сети", "network.slash", .red) }
        if !model.onLAN { return ("Нет подключения к Wi-Fi", "wifi.slash", .orange) }
        if model.netAccess == .granted { return ("Сеть доступна", "checkmark.circle.fill", .green) }
        return ("Проверка сети…", "ellipsis.circle", .secondary)
    }

    private func openIOSSettings() {
        if let u = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(u) }
    }

    private func openFiles() {
        let path = documentsURL().appendingPathComponent("LocalDrop").path
        if let enc = path.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed),
           let u = URL(string: "shareddocuments://" + enc) {
            UIApplication.shared.open(u)
        }
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Это устройство") {
                    HStack {
                        TextField("Имя устройства", text: $name).submitLabel(.done)
                            .onSubmit { model.setName(name) }
                        if name.trimmingCharacters(in: .whitespaces) != model.myName {
                            Button("Сохранить") { model.setName(name) }.buttonStyle(.borderless)
                        }
                    }
                    LabeledContent("Отпечаток", value: model.fpShort).textSelection(.enabled)
                }
                .listRowBackground(Color.surface)

                Section {
                    Picker("Принимать от", selection: Binding(get: { model.mode }, set: { model.setMode($0) })) {
                        ForEach(ReceiveMode.allCases, id: \.self) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.inline).labelsHidden()
                } header: { Text("Кто может отправлять вам файлы") } footer: { Text(model.mode.hint) }
                .listRowBackground(Color.surface)

                Section("Сеть") {
                    Label { Text(net.0) } icon: { Image(systemName: net.1).foregroundStyle(net.2) }
                    if model.netAccess == .denied {
                        Button("Открыть настройки iOS", action: openIOSSettings)
                    }
                    LabeledContent("Устройств рядом", value: "\(model.peers.count)")
                }
                .listRowBackground(Color.surface)

                Section {
                    Button { openFiles() } label: { Label("Открыть папку в «Файлах»", systemImage: "folder") }
                } header: { Text("Принятые файлы") } footer: {
                    Text("Файлы лежат в «Файлы» → На iPhone → LocalDrop. Приём работает, пока приложение открыто: iOS приостанавливает его в фоне.")
                }
                .listRowBackground(Color.surface)

                Section("Оформление и поведение") {
                    Picker("Тема", selection: $theme) {
                        Text("Системная").tag("system"); Text("Светлая").tag("light"); Text("Тёмная").tag("dark")
                    }
                    .pickerStyle(.segmented)
                    Toggle("Вибрация", isOn: $haptics)
                    Toggle("Не гасить экран во время передачи", isOn: $keepAwake)
                }
                .listRowBackground(Color.surface)

                Section {
                    Button("Очистить историю", role: .destructive) { confirmClear = true }
                }
                .listRowBackground(Color.surface)

                Section("Помощь") {
                    Link(destination: issuesURL) { Label("Сообщить о проблеме", systemImage: "exclamationmark.bubble") }
                    Button {
                        UIPasteboard.general.string = model.diagnostics()
                        copied = true
                        Haptics.success()
                    } label: {
                        Label(copied ? "Диагностика скопирована" : "Скопировать диагностику", systemImage: "doc.on.doc")
                    }
                    Button { onboarded = false } label: { Label("Показать приветствие", systemImage: "hand.wave") }
                    Link(destination: privacyURL) { Label("Политика конфиденциальности", systemImage: "hand.raised") }
                    Link(destination: webpageURL) { Label("Страница приложения", systemImage: "globe") }
                }
                .listRowBackground(Color.surface)

                Section("О приложении") {
                    LabeledContent("Версия", value: version)
                    LabeledContent("Протокол", value: "v\(protocolVersion())")
                }
                .listRowBackground(Color.surface)
            }
            .scrollContentBackground(.hidden)
            .background(AppBackground())
            .navigationTitle("Настройки")
            .onAppear { name = model.myName; copied = false }
            .confirmationDialog("Удалить всю историю?", isPresented: $confirmClear, titleVisibility: .visible) {
                Button("Очистить", role: .destructive) { model.clearHistory() }
            }
        }
    }
}
