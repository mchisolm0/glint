// While recording: a thin click-through frame around the recorded area and a small control beside it with a
// pulsing red dot, the elapsed time and Stop. A scrolling capture says "Scroll" and has Done instead; the frame
// lets the scrolling through to the app under it. Neither window can be shared, and Rust excludes Glint's pid too.
import AppKit

final class RecordingHUD {
    private var frame: GlintPanel?
    private let control: GlintPanel
    private let time = NSTextField(labelWithString: "0:00")
    private var timer: Timer?

    /// `area` is in global top-left points; an empty one means the whole display under the pointer, unframed.
    init(area: CGRect, scrolling: Bool = false, stop: @escaping () -> Void) {
        let screen = Desktop.pointerScreen
        let local = area.width > 0 && area.height > 0 ? Desktop.flip(area) : screen.frame
        let display = NSScreen.screens.first { $0.frame.contains(CGPoint(x: local.midX, y: local.midY)) } ?? screen
        let visible = display.visibleFrame
        if area.width > 0, area.height > 0, local.width < display.frame.width - 2 || local.height < display.frame.height - 2 {
            let window = GlintPanel(frame: local.insetBy(dx: -4, dy: -4), level: .statusBar)
            window.ignoresMouseEvents = true
            window.hasShadow = false
            window.contentView = Outline(frame: CGRect(origin: .zero, size: window.frame.size))
            window.present(window.frame, duration: 0.15)
            frame = window
        }

        let size = CGSize(width: 156, height: 36)
        // Below the area, else above it, else inside its bottom edge (a full-display recording).
        var y = local.minY - 12 - size.height
        if y < visible.minY + 4 { y = local.maxY + 12 }
        if y + size.height > visible.maxY - 4 { y = max(local.minY, visible.minY) + 24 }
        let x = min(max(local.midX - size.width / 2, visible.minX + 8), visible.maxX - size.width - 8)
        control = GlintPanel(frame: CGRect(origin: CGPoint(x: x, y: y), size: size), level: .statusBar)
        control.isMovableByWindowBackground = true
        let root = NSView(frame: CGRect(origin: .zero, size: size))
        root.addSubview(Chrome.backdrop(root.bounds, radius: size.height / 2))

        let dot = NSView(frame: CGRect(x: 14, y: (size.height - 10) / 2, width: 10, height: 10))
        dot.wantsLayer = true
        dot.layer?.backgroundColor = NSColor.systemRed.cgColor
        dot.layer?.cornerRadius = 5
        let pulse = CABasicAnimation(keyPath: "opacity")
        pulse.fromValue = 1
        pulse.toValue = 0.3
        pulse.duration = 0.8
        pulse.autoreverses = true
        pulse.repeatCount = .infinity
        dot.layer?.add(pulse, forKey: "pulse")
        root.addSubview(dot)

        if scrolling { time.stringValue = "Scroll" }
        time.font = .monospacedDigitSystemFont(ofSize: 13, weight: .medium)
        time.textColor = .white
        time.frame = CGRect(x: 32, y: (size.height - 17) / 2, width: 52, height: 17)
        root.addSubview(time)

        var button: ChromeButton!
        button = ChromeButton(symbol: scrolling ? "checkmark" : "stop.fill", title: scrolling ? "Done" : "Stop",
                              help: scrolling ? "Finish the scrolling capture" : "Stop recording", style: .pill, size: 26) {
            button.isEnabled = false
            stop()
        }
        button.frame.origin = CGPoint(x: size.width - button.frame.width - 5, y: (size.height - 26) / 2)
        root.addSubview(button)
        control.contentView = root
        control.present(control.frame, from: CGVector(dx: 0, dy: -8), duration: 0.2)

        guard !scrolling else { return }
        let started = Date()
        let timer = Timer(timeInterval: 0.25, repeats: true) { [weak time] _ in
            time?.stringValue = Chrome.clock(Date().timeIntervalSince(started))
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    func close() {
        timer?.invalidate()
        timer = nil
        frame?.dismiss(duration: 0.12)
        frame = nil
        control.dismiss(duration: 0.12)
    }
}

/// The frame: a white hairline with a dark halo, so it reads on any background.
private final class Outline: NSView {
    override func draw(_ dirtyRect: NSRect) {
        let rect = bounds.insetBy(dx: 2.5, dy: 2.5)
        NSColor.black.withAlphaComponent(0.35).setStroke()
        let halo = NSBezierPath(rect: rect)
        halo.lineWidth = 3
        halo.stroke()
        NSColor.white.withAlphaComponent(0.9).setStroke()
        let line = NSBezierPath(rect: rect)
        line.lineWidth = 1
        line.stroke()
    }
}
