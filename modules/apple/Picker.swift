// The pick overlay: one borderless window per display over a dimmed desktop. Area mode draws a dragged area with
// full-screen guides and its size; window mode highlights the window under the pointer. Space toggles the two;
// Escape or a right click cancels. The overlays are non-activating panels, so picking never brings Glint's main
// window forward over what the person is capturing.
import AppKit

/// What a pick answers, in global top-left points; `window` is 0 unless a window was picked.
struct Pick {
    var rect: CGRect
    var window: CGWindowID = 0
}

/// One pick across every display. `start` calls its completion once, with nil when cancelled.
final class Picker {
    enum Mode { case area, window }
    /// Another app's on-screen window, its frame in global top-left points.
    struct Target { let id: CGWindowID, frame: CGRect, owner: String }

    private(set) var mode: Mode
    /// In `record` a click without a drag picks the whole display.
    let record: Bool
    /// Front to back, as the window server lists them.
    private(set) var targets: [Target] = []
    private(set) var hovered: Target?
    private var overlays: [PickOverlay] = []
    private var done: ((Pick?) -> Void)?

    init(mode: String) {
        record = mode == "record"
        self.mode = mode == "window" ? .window : .area
    }

    func start(_ done: @escaping (Pick?) -> Void) {
        self.done = done
        targets = Self.windows()
        let mouse = NSEvent.mouseLocation
        for screen in NSScreen.screens {
            let overlay = PickOverlay(screen: screen, picker: self)
            overlays.append(overlay)
            overlay.alphaValue = 0
            overlay.orderFrontRegardless()
            if NSMouseInRect(mouse, screen.frame, false) { overlay.makeKey() }
        }
        NSCursor.crosshair.set()
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.12
            overlays.forEach { $0.animator().alphaValue = 1 }
        }
        moved(to: mouse)
    }

    func cancel() { finish(nil) }

    func finish(_ pick: Pick?) {
        guard let done else { return }
        self.done = nil
        overlays.forEach { $0.orderOut(nil) }
        overlays = []
        done(pick)
    }

    func toggle() {
        mode = mode == .area ? .window : .area
        if mode == .window { targets = Self.windows() }
        overlays.forEach { $0.view.reset() }
        moved(to: NSEvent.mouseLocation)
    }

    /// The pointer moved to `global` (AppKit points): find the window under it and redraw every display.
    func moved(to global: CGPoint) {
        let point = Desktop.flip(global)
        hovered = mode == .window ? targets.first { $0.frame.contains(point) } : nil
        overlays.forEach { $0.view.refresh(global) }
    }

    /// On-screen windows of other apps at the normal level, front to back.
    private static func windows() -> [Target] {
        let own = getpid()
        let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
        return list.compactMap { info in
            guard (info[kCGWindowLayer as String] as? Int) == 0,
                  (info[kCGWindowOwnerPID as String] as? pid_t) != own,
                  (info[kCGWindowAlpha as String] as? Double ?? 1) > 0,
                  let id = info[kCGWindowNumber as String] as? CGWindowID,
                  let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                  let frame = CGRect(dictionaryRepresentation: bounds),
                  frame.width >= 40, frame.height >= 40 else { return nil }
            return Target(id: id, frame: frame, owner: info[kCGWindowOwnerName as String] as? String ?? "")
        }
    }
}

/// A borderless panel refuses key status by default, and the overlay needs it for Escape and Space. Being
/// non-activating, it takes keys without activating Glint, so Glint's other windows stay where they are.
private final class PickOverlay: NSPanel {
    let display: NSScreen
    let view: PickView
    override var canBecomeKey: Bool { true }

    init(screen: NSScreen, picker: Picker) {
        display = screen
        view = PickView(frame: CGRect(origin: .zero, size: screen.frame.size), screen: screen, picker: picker)
        super.init(contentRect: screen.frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        level = .screenSaver
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        isReleasedWhenClosed = false
        hidesOnDeactivate = false
        acceptsMouseMovedEvents = true
        sharingType = .none
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        appearance = NSAppearance(named: .darkAqua)
        contentView = view
        setFrame(screen.frame, display: false)
    }
}

/// One display's overlay, drawn with layers so a pointer move redraws only what moved.
private final class PickView: NSView {
    private unowned let picker: Picker
    private let screen: NSScreen
    private var start: CGPoint?
    private var area: CGRect?
    private let dim = CAShapeLayer(), edge = CAShapeLayer(), guides = CAShapeLayer()
    private let size = CATextLayer()

    init(frame: CGRect, screen: NSScreen, picker: Picker) {
        self.screen = screen
        self.picker = picker
        super.init(frame: frame)
        wantsLayer = true
        guard let root = layer else { return }
        let scale = screen.backingScaleFactor
        dim.fillRule = .evenOdd
        dim.fillColor = NSColor.black.withAlphaComponent(0.32).cgColor
        guides.strokeColor = NSColor.white.withAlphaComponent(0.55).cgColor
        guides.lineWidth = 1
        guides.lineDashPattern = [4, 3]
        edge.lineWidth = 1
        size.backgroundColor = NSColor.black.withAlphaComponent(0.72).cgColor
        size.cornerRadius = 5
        size.alignmentMode = .center
        size.isHidden = true
        [dim, edge, guides, size].forEach { root.addSublayer($0); $0.contentsScale = scale; $0.actions = Self.still }
        root.actions = Self.still
    }

    required init?(coder: NSCoder) { fatalError("not from a nib") }

    /// No implicit animations: layers follow the pointer exactly.
    private static let still: [String: CAAction] = ["position": NSNull(), "bounds": NSNull(), "path": NSNull(), "contents": NSNull(), "hidden": NSNull(), "contentsRect": NSNull(), "string": NSNull()]

    override var acceptsFirstResponder: Bool { true }
    override func viewDidMoveToWindow() { window?.makeFirstResponder(self) }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .crosshair) }
    override func cursorUpdate(with event: NSEvent) { NSCursor.crosshair.set() }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.activeAlways, .mouseMoved, .mouseEnteredAndExited, .cursorUpdate, .inVisibleRect], owner: self))
    }

    func reset() {
        start = nil
        area = nil
    }

    // Cursor rects apply only in the active app, and the overlay never activates Glint.
    override func mouseMoved(with event: NSEvent) {
        NSCursor.crosshair.set()
        picker.moved(to: NSEvent.mouseLocation)
    }
    override func mouseExited(with event: NSEvent) { picker.moved(to: NSEvent.mouseLocation) }

    override func mouseDown(with event: NSEvent) {
        start = convert(event.locationInWindow, from: nil)
        area = nil
    }

    override func mouseDragged(with event: NSEvent) {
        guard let start else { return }
        let p = clamp(convert(event.locationInWindow, from: nil))
        // A drag in window mode means the person wants an area after all.
        if picker.mode == .window {
            guard hypot(p.x - start.x, p.y - start.y) > 4 else { return }
            picker.toggle()
            self.start = start
        }
        area = CGRect(x: min(start.x, p.x), y: min(start.y, p.y), width: abs(p.x - start.x), height: abs(p.y - start.y)).integral
        picker.moved(to: NSEvent.mouseLocation)
    }

    override func mouseUp(with event: NSEvent) {
        defer { start = nil }
        guard start != nil else { return }
        if picker.mode == .window {
            if let target = picker.hovered { picker.finish(Pick(rect: target.frame, window: target.id)) }
            return
        }
        if let area, area.width >= 4, area.height >= 4 {
            return picker.finish(Pick(rect: Desktop.flip(area.offsetBy(dx: screen.frame.minX, dy: screen.frame.minY))))
        }
        if picker.record { return picker.finish(Pick(rect: CGDisplayBounds(screen.displayID))) }
        area = nil
        picker.moved(to: NSEvent.mouseLocation)
    }

    override func rightMouseDown(with event: NSEvent) { picker.cancel() }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 53: picker.cancel()
        case 49 where start == nil: picker.toggle()
        default: super.keyDown(with: event)
        }
    }

    private func clamp(_ p: CGPoint) -> CGPoint {
        CGPoint(x: min(max(p.x, 0), bounds.width), y: min(max(p.y, 0), bounds.height))
    }

    /// Redraw for the pointer at `global` (AppKit points), which may be on another display.
    func refresh(_ global: CGPoint) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        let local = CGPoint(x: global.x - screen.frame.minX, y: global.y - screen.frame.minY)
        let here = bounds.contains(local) ? local : nil
        let shape = CGMutablePath()
        shape.addRect(bounds)
        edge.path = nil
        size.isHidden = true
        if picker.mode == .window {
            guides.path = nil
            if let target = picker.hovered {
                let frame = Desktop.flip(target.frame).offsetBy(dx: -screen.frame.minX, dy: -screen.frame.minY)
                let hole = CGPath(roundedRect: frame, cornerWidth: 10, cornerHeight: 10, transform: nil)
                shape.addPath(hole)
                edge.path = hole
                edge.lineWidth = 2
                edge.strokeColor = NSColor.controlAccentColor.cgColor
                edge.fillColor = NSColor.controlAccentColor.withAlphaComponent(0.16).cgColor
                if here != nil {
                    let name = target.owner.isEmpty ? "" : target.owner + "  "
                    place(size, text: "\(name)\(Int(frame.width)) × \(Int(frame.height))", near: frame)
                }
            }
            dim.path = shape
            return
        }
        if let area {
            shape.addRect(area)
            edge.path = CGPath(rect: area.insetBy(dx: -0.5, dy: -0.5), transform: nil)
            edge.lineWidth = 1
            edge.strokeColor = NSColor.white.cgColor
            edge.fillColor = nil
            place(size, text: "\(Int(area.width)) × \(Int(area.height))", near: area)
        }
        dim.path = shape
        guard let here else {
            guides.path = nil
            return
        }
        let x = floor(here.x) + 0.5, y = floor(here.y) + 0.5
        let lines = CGMutablePath()
        lines.move(to: CGPoint(x: x, y: 0))
        lines.addLine(to: CGPoint(x: x, y: bounds.height))
        lines.move(to: CGPoint(x: 0, y: y))
        lines.addLine(to: CGPoint(x: bounds.width, y: y))
        guides.path = lines
    }

    /// `label` below `rect`, or inside its bottom when the display ends there.
    private func place(_ label: CATextLayer, text: String, near rect: CGRect) {
        let s = measure(label, text)
        var y = rect.minY - s.height - 6
        if y < 4 { y = rect.minY + 6 }
        let x = min(max(rect.minX, 4), bounds.width - s.width - 4)
        label.frame = CGRect(origin: CGPoint(x: x, y: y), size: s)
        label.isHidden = false
    }

    private func measure(_ label: CATextLayer, _ text: String) -> CGSize {
        let string = NSAttributedString(string: text, attributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium),
            .foregroundColor: NSColor.white,
        ])
        label.string = string
        let s = string.size()
        // CATextLayer draws from the top; padding the frame and the baseline offset come out even at 11 pt.
        return CGSize(width: ceil(s.width) + 14, height: ceil(s.height) + 6)
    }
}
