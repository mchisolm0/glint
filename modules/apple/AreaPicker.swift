// Glint's native module on the Mac: the area overlay. app.ts asks `native.later({op: "pickArea"})`; a dimmed
// window covers every display, a drag draws the area, and the reply is that area in the desktop's global points
// with the origin top-left (what xcap and CoreGraphics use), or `picked: false` when Escape or a right click cancels.
import AppKit
import Foundation

final class Glint: ExactModule {
    private var picker: AreaPicker?

    override func later(_ request: [String: Any], reply: ExactReply) {
        guard request["op"] as? String == "pickArea" else { return reply.fail("unknown op") }
        DispatchQueue.main.async {
            guard self.picker == nil else { return reply.fail("an area is already being picked") }
            let picker = AreaPicker()
            self.picker = picker
            picker.pick { rect in
                self.picker = nil
                guard let rect else {
                    return reply.send(["picked": false, "x": 0, "y": 0, "width": 0, "height": 0])
                }
                // The overlay must be off screen before Rust captures, or it is in the picture.
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                    reply.send(["picked": true, "x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height])
                }
            }
        }
    }
}

/// One borderless overlay per display. Returns the area in global top-left points, or nil when cancelled.
final class AreaPicker {
    private var windows: [NSWindow] = []

    func pick(_ done: @escaping (CGRect?) -> Void) {
        // AppKit's global space has its origin at the primary display's bottom-left; CoreGraphics' at its top-left.
        let primaryHeight = NSScreen.screens.first?.frame.height ?? 0
        let finish: (CGRect?) -> Void = { [weak self] local in
            self?.windows.forEach { $0.orderOut(nil) }
            self?.windows = []
            done(local.map { CGRect(x: $0.minX, y: primaryHeight - $0.maxY, width: $0.width, height: $0.height) })
        }
        let mouse = NSEvent.mouseLocation
        for screen in NSScreen.screens {
            let window = OverlayWindow(contentRect: screen.frame, styleMask: .borderless, backing: .buffered, defer: false)
            window.level = .screenSaver
            window.isOpaque = false
            window.backgroundColor = .clear
            window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            let view = SelectionView(frame: CGRect(origin: .zero, size: screen.frame.size))
            // The view reports in its own coordinates; the screen's origin makes them global.
            view.finish = { rect in finish(rect.map { $0.offsetBy(dx: screen.frame.minX, dy: screen.frame.minY) }) }
            window.contentView = view
            window.orderFrontRegardless()
            if NSMouseInRect(mouse, screen.frame, false) { window.makeKey() }
            windows.append(window)
        }
        NSApp.activate(ignoringOtherApps: true)
    }
}

/// A borderless window refuses key status by default, and the overlay needs it for Escape.
private final class OverlayWindow: NSWindow {
    override var canBecomeKey: Bool { true }
}

/// Dims its display and draws the dragged area clear, with its size beside it.
private final class SelectionView: NSView {
    var finish: (CGRect?) -> Void = { _ in }
    private var start: CGPoint?
    private var area: CGRect?

    override var acceptsFirstResponder: Bool { true }
    override func viewDidMoveToWindow() { window?.makeFirstResponder(self) }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .crosshair) }

    override func mouseDown(with event: NSEvent) {
        start = convert(event.locationInWindow, from: nil)
        area = nil
    }

    override func mouseDragged(with event: NSEvent) {
        guard let start else { return }
        let p = convert(event.locationInWindow, from: nil)
        area = CGRect(x: min(start.x, p.x), y: min(start.y, p.y), width: abs(p.x - start.x), height: abs(p.y - start.y))
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        guard let area, area.width >= 4, area.height >= 4 else { start = nil; self.area = nil; needsDisplay = true; return }
        finish(area.integral)
    }

    override func rightMouseDown(with event: NSEvent) { finish(nil) }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { finish(nil) } else { super.keyDown(with: event) }
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.black.withAlphaComponent(0.35).setFill()
        bounds.fill()
        guard let area else { return }
        NSColor.clear.setFill()
        area.fill(using: .copy)
        NSColor.white.setStroke()
        let edge = NSBezierPath(rect: area.insetBy(dx: 0.5, dy: 0.5))
        edge.lineWidth = 1
        edge.stroke()
        let label = "\(Int(area.width)) × \(Int(area.height))" as NSString
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium),
            .foregroundColor: NSColor.white,
            .backgroundColor: NSColor.black.withAlphaComponent(0.6),
        ]
        label.draw(at: CGPoint(x: area.minX, y: max(area.minY - 16, 2)), withAttributes: attributes)
    }
}

let exactModule: ExactModule.Type = Glint.self
