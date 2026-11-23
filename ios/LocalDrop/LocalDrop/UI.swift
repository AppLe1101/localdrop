//
//  UI.swift
//  LocalDrop
//
//  Created by Ablochko on 12.11.2026.
//

import SwiftUI
import UIKit

// MARK: Палитра

extension Color {
    static let brand = Color(red: 0.04, green: 0.52, blue: 1.0)

    static func dyn(_ light: UIColor, _ dark: UIColor) -> Color {
        Color(UIColor { $0.userInterfaceStyle == .dark ? dark : light })
    }
    static let surfaceStroke = dyn(UIColor(white: 0, alpha: 0.09), UIColor(white: 1, alpha: 0.14))
    static let bgTop = dyn(UIColor(white: 0.99, alpha: 1), UIColor(red: 0.105, green: 0.11, blue: 0.125, alpha: 1))
    static let bgBottom = dyn(UIColor(red: 0.90, green: 0.91, blue: 0.93, alpha: 1), UIColor(red: 0.02, green: 0.02, blue: 0.03, alpha: 1))
    static let surface = dyn(UIColor(white: 1, alpha: 0.96), UIColor(red: 0.14, green: 0.15, blue: 0.17, alpha: 0.96))
}

// MARK: Haptics

enum Haptics {
    private static var on: Bool { UserDefaults.standard.object(forKey: "haptics") as? Bool ?? true }
    static func success() { if on { UINotificationFeedbackGenerator().notificationOccurred(.success) } }
    static func warning() { if on { UINotificationFeedbackGenerator().notificationOccurred(.warning) } }
    static func error() { if on { UINotificationFeedbackGenerator().notificationOccurred(.error) } }
    static func tick() { if on { UISelectionFeedbackGenerator().selectionChanged() } }
}

// MARK: Стекло (Liquid Glass на iOS 26, материалы на старых версиях)

extension View {
    @ViewBuilder
    func glassShape(_ shape: AnyShape, tint: Color? = nil, interactive: Bool = false, solid: Bool = false) -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26, *) {
            self.modifier(NativeGlass(shape: shape, tint: tint, interactive: interactive))
        } else {
            legacy(shape, tint: tint, solid: solid)
        }
        #else
        legacy(shape, tint: tint, solid: solid)
        #endif
    }

    private func legacy(_ shape: AnyShape, tint: Color?, solid: Bool) -> some View {
        self
            .background(.regularMaterial, in: shape)
            .background((tint ?? .clear).opacity(solid ? 1 : 0.18), in: shape)
            .overlay { shape.stroke(Color.surfaceStroke, lineWidth: 1).padding(0.5) }
            .shadow(color: .black.opacity(0.14), radius: 14, y: 6)
    }

    func glass(_ radius: CGFloat = 24, tint: Color? = nil, interactive: Bool = false) -> some View {
        glassShape(AnyShape(RoundedRectangle(cornerRadius: radius, style: .continuous)), tint: tint, interactive: interactive)
    }
    func glassCapsule(tint: Color? = nil, interactive: Bool = false, solid: Bool = false) -> some View {
        glassShape(AnyShape(Capsule()), tint: tint, interactive: interactive, solid: solid)
    }
    func glassCircle(tint: Color? = nil, interactive: Bool = false) -> some View {
        glassShape(AnyShape(Circle()), tint: tint, interactive: interactive)
    }

    /// Плотная карточка без стекла (для списков и вложенных блоков)
    func surfaceCard(_ radius: CGFloat = 16) -> some View {
        let s = RoundedRectangle(cornerRadius: radius, style: .continuous)
        return self.background(Color.surface, in: s).overlay(s.strokeBorder(Color.surfaceStroke, lineWidth: 1))
    }

    /// Идентификатор для «перетекания» стеклянных элементов (iOS 26)
    @ViewBuilder
    func glassID<ID: Hashable & Sendable>(_ id: ID, in ns: Namespace.ID) -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26, *) { self.glassEffectID(id, in: ns) } else { self }
        #else
        self
        #endif
    }

    @ViewBuilder
    func minimizeTabBar() -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26, *) { self.tabBarMinimizeBehavior(.onScrollDown) } else { self }
        #else
        self
        #endif
    }
}

#if compiler(>=6.2)
@available(iOS 26, *)
private struct NativeGlass: ViewModifier {
    let shape: AnyShape
    let tint: Color?
    let interactive: Bool
    func body(content: Content) -> some View {
        var g = Glass.regular
        if let tint { g = g.tint(tint) }
        if interactive { g = g.interactive() }
        return content.glassEffect(g, in: shape)
    }
}
#endif

/// Контейнер, в котором стеклянные элементы сливаются и перетекают друг в друга
struct GlassGroup<Content: View>: View {
    var spacing: CGFloat = 12
    @ViewBuilder var content: Content
    var body: some View {
        #if compiler(>=6.2)
        if #available(iOS 26, *) { GlassEffectContainer(spacing: spacing) { content } } else { content }
        #else
        content
        #endif
    }
}

struct PillButtonStyle: ButtonStyle {
    var prominent = false
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.subheadline.weight(.semibold))
            .padding(.horizontal, 16).padding(.vertical, 10)
            .foregroundStyle(prominent ? Color.white : Color.primary)
            .glassCapsule(tint: prominent ? .brand : nil, interactive: true, solid: prominent)
            .scaleEffect(configuration.isPressed ? 0.95 : 1)
            .animation(.spring(duration: 0.25, bounce: 0.4), value: configuration.isPressed)
    }
}

// MARK: Фон с медленно плывущим свечением

struct AppBackground: View {
    var body: some View {
        LinearGradient(colors: [.bgTop, .bgBottom], startPoint: .top, endPoint: .bottom)
            .ignoresSafeArea()
    }
}

// MARK: Аватар, кольцо, поиск

/// Единственное место, где рисуется аватар (позже сюда добавятся фото)
struct AvatarView: View {
    let name: String
    var size: CGFloat = 56
    var me = false

    var body: some View {
        let h = peerHue(name)
        let colors: [Color] = me
            ? [Color(red: 0.31, green: 0.66, blue: 1), Color(red: 0.03, green: 0.38, blue: 0.95)]
            : [Color(hue: h, saturation: 0.62, brightness: 0.92), Color(hue: h, saturation: 0.85, brightness: 0.60)]
        Text(String(name.prefix(1)).uppercased())
            .font(.system(size: size * 0.42, weight: .semibold, design: .rounded))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(LinearGradient(colors: colors, startPoint: .topLeading, endPoint: .bottomTrailing), in: Circle())
            .overlay(Circle().strokeBorder(.white.opacity(0.4), lineWidth: 1))
            .shadow(color: colors[1].opacity(0.45), radius: size * 0.12, y: size * 0.06)
    }
}

struct ProgressRing: View {
    let fraction: Double
    let waiting: Bool
    let size: CGFloat
    @State private var spin = false

    var body: some View {
        ZStack {
            Circle().stroke(Color.brand.opacity(0.22), lineWidth: 4)
            Circle()
                .trim(from: 0, to: waiting ? 0.25 : max(0.02, fraction))
                .stroke(Color.brand, style: StrokeStyle(lineWidth: 4, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .rotationEffect(.degrees(waiting && spin ? 360 : 0))
                .animation(waiting ? .linear(duration: 1).repeatForever(autoreverses: false) : .easeOut(duration: 0.2), value: spin)
                .animation(.easeOut(duration: 0.2), value: fraction)
        }
        .frame(width: size, height: size)
        .onAppear { spin = true }
    }
}

struct SearchField: View {
    @Binding var text: String
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField("Найти устройство", text: $text).textInputAutocapitalization(.never).autocorrectionDisabled()
            if !text.isEmpty {
                Button { text = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
        .glassCapsule()
    }
}

// MARK: Баннер

struct BannerView: View {
    let b: Banner
    let onClose: () -> Void

    private var style: (String, Color) {
        switch b.kind {
        case .success: ("checkmark.circle.fill", .green)
        case .warning: ("exclamationmark.triangle.fill", .orange)
        case .error: ("xmark.octagon.fill", .red)
        case .info: ("info.circle.fill", .brand)
        }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: style.0).font(.title3).foregroundStyle(style.1)
                .symbolEffect(.bounce, value: b.id)
            VStack(alignment: .leading, spacing: 2) {
                Text(b.count > 1 ? "\(b.title): \(b.count)" : b.title)
                    .font(.subheadline.weight(.semibold))
                    .contentTransition(.numericText())
                if let t = b.text { Text(t).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                if let c = b.copyText {
                    Button("Копировать") { UIPasteboard.general.string = c; onClose() }.font(.caption.weight(.medium))
                }
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(20, tint: style.1.opacity(0.22))
        .padding(.horizontal)
        .onTapGesture(perform: onClose)
    }
}

// MARK: Цифры кода: появляются по очереди

struct CodeDigits: View {
    let code: String
    var size: CGFloat = 38
    @State private var shown = 0

    var body: some View {
        HStack(spacing: 2) {
            ForEach(Array(code.enumerated()), id: \.offset) { i, ch in
                Text(String(ch))
                    .font(.system(size: size, weight: .bold, design: .rounded)).monospacedDigit()
                    .foregroundStyle(Color.brand)
                    .opacity(i < shown ? 1 : 0)
                    .scaleEffect(i < shown ? 1 : 0.4)
                    .offset(y: i < shown ? 0 : 10)
            }
        }
        .task {
            for i in 1...max(code.count, 1) {
                withAnimation(.spring(duration: 0.35, bounce: 0.5)) { shown = i }
                try? await Task.sleep(for: .milliseconds(70))
            }
        }
    }
}
