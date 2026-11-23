//
//  RootView.swift
//  LocalDrop
//
//  Created by Ablochko on 09.11.2026.
//

import SwiftUI

struct RootView: View {
    @AppStorage("onboarded") private var onboarded = false
    @AppStorage("theme") private var theme = "system"
    @StateObject private var model = LocalDropModel()
    @Environment(\.scenePhase) private var phase
    @State private var tab = 0

    var body: some View {
        TabView(selection: $tab) {
            ContentView().tabItem { Label("Радар", systemImage: "dot.radiowaves.left.and.right") }.tag(0)
            DevicesView().tabItem { Label("Устройства", systemImage: "laptopcomputer.and.iphone") }.tag(1)
            HistoryView().tabItem { Label("История", systemImage: "clock") }.tag(2)
            SettingsView().tabItem { Label("Настройки", systemImage: "gearshape") }.tag(3)
        }
        .tint(.brand)
        .minimizeTabBar()
        .environmentObject(model)
        .task { if onboarded {model.start()} }
        .fullScreenCover(isPresented: Binding(get:{!onboarded}, set: { _ in })) {
            OnboardingView {onboarded = true; model.start()}.interactiveDismissDisabled()
        }
        .preferredColorScheme(theme == "light" ? .light : theme == "dark" ? .dark : nil)
        .onChange(of: phase) { _, new in
            if new == .active {
                model.refreshDiscovery()
                model.importInbox()
            }
        }
        .onChange(of: model.inboxArrived) { _, _ in tab = 0 }
        .sheet(item: Binding(get: { model.incoming.first }, set: { _ in })) { req in
            IncomingSheet(req: req) { model.respond(req, $0) }
                .interactiveDismissDisabled()
        }
        .alert("Возможная подмена устройства", isPresented: Binding(
            get: { model.conflictPeer != nil }, set: { if !$0 { model.conflictPeer = nil } })
        ) {
            Button("Отмена", role: .cancel) {}
            Button("Всё равно отправить", role: .destructive) {
                if let p = model.conflictPeer { model.confirmConflict(p) }
            }
        } message: {
            Text("Устройство с таким именем раньше имело другой отпечаток. Это может быть переустановка приложения, а может быть подмена.")
        }
        .overlay(alignment: .top) {
            if let b = model.banner {
                BannerView(b: b) { model.banner = nil }
                    .task(id: b.id) {
                        try? await Task.sleep(for: .seconds(b.copyText == nil ? 4 : 10))
                        if model.banner?.id == b.id { model.banner = nil }
                    }
                    .transition(.move(edge: .top).combined(with: .scale(scale: 0.92)).combined(with: .opacity))
            }
        }
        .animation(.spring(duration: 0.45, bounce: 0.35), value: model.banner?.id)
    }
}
