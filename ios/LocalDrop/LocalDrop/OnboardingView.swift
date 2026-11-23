//
//  OnboardingView.swift
//  LocalDrop
//
//  Created by Ablochko on 21.11.2026.
//

import SwiftUI

private struct OnbPage: View {
    let icon: String, title: String, text: String
    var body: some View {
        VStack(spacing: 22) {
            Spacer()
            Image(systemName: icon)
                .font(.system(size: 54, weight: .medium))
                .foregroundStyle(.white)
                .frame(width: 118, height: 118)
                .background(LinearGradient(colors: [Color(red: 0.31, green: 0.66, blue: 1), Color.brand],
                                           startPoint: .topLeading, endPoint: .bottomTrailing), in: Circle())
                .shadow(color: Color.brand.opacity(0.45), radius: 24, y: 10)
                .symbolEffect(.pulse, options: .repeating)
            Text(title).font(.title.weight(.bold)).multilineTextAlignment(.center)
            Text(text).font(.body).foregroundStyle(.secondary).multilineTextAlignment(.center)
            Spacer(); Spacer()
        }
        .padding(.horizontal, 32)
    }
}

struct OnboardingView: View {
    let finish: () -> Void
    @State private var page = 0

    var body: some View {
        ZStack {
            AppBackground()
            VStack(spacing: 8) {
                TabView(selection: $page) {
                    OnbPage(icon: "dot.radiowaves.left.and.right",
                            title: "Файлы между вашими устройствами",
                            text: "Отправляйте файлы, папки и текст на соседние устройства в одной сети. Без облака, аккаунтов и интернета.").tag(0)
                    OnbPage(icon: "checkmark.shield",
                            title: "Безопасно",
                            text: "Соединение шифруется. Для нового устройства на обоих экранах показывается одинаковый код: сверьте его перед приёмом.").tag(1)
                    OnbPage(icon: "wifi",
                            title: "Нужен доступ к локальной сети",
                            text: "Сейчас iOS спросит разрешение. Без него LocalDrop не увидит другие устройства. Приём работает, пока приложение открыто.").tag(2)
                }
                .tabViewStyle(.page(indexDisplayMode: .always))

                Button {
                    if page < 2 { withAnimation(.spring(duration: 0.4)) { page += 1 } } else { finish() }
                } label: {
                    Text(page < 2 ? "Далее" : "Разрешить и начать").frame(maxWidth: .infinity)
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .padding(.horizontal, 24).padding(.bottom, 12)
            }
        }
    }
}
