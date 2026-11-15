// The pick overlay: one borderless window per display over a dimmed desktop. Area mode draws a dragged area with
// full-screen guides, its size and a pixel loupe; window mode highlights the window under the pointer. Space
// toggles the two; Escape or a right click cancels.
import AppKit
import ScreenCaptureKit

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
        NSApp.activate(ignoringOtherApps: true)
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.12
            overlays.forEach { $0.animator().alphaValue = 1 }
        }
        moved(to: mouse)
        Snapshot.take { [weak self] display, image in
            self?.overlays.first { $0.display.displayID == display }?.view.snapshot = image
        }
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

/// A borderless window refuses key status by default, and the overlay needs it for Escape and Space.
private final class PickOverlay: NSWindow {
    let display: NSScreen
    let view: PickView
    override var canBecomeKey: Bool { true }

    init(screen: NSScreen, picker: Picker) {
        display = screen
        view = PickView(frame: CGRect(origin: .zero, size: screen.frame.size), screen: screen, picker: picker)
        super.init(contentRect: screen.frame, styleMask: .borderless, backing: .buffered, defer: false)
        level = .screenSaver
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        isReleasedWhenClosed = false
        sharingType = .none
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        appearance = NSAppearance(named: .darkAqua)
        contentView = view
        setFrame(screen.frame, display: false)
    }
}

/// One display's overlay, drawn with layers so a pointer move redraws only what moved.
private final class PickView: NSView {
    /// This display's pixels without Glint's windows, for the loupe; nil until ScreenCaptureKit answers.
    var snapshot: CGImage? { didSet { refresh(NSEvent.mouseLocation) } }
    private unowned let picker: Picker
    private let screen: NSScreen
    private var start: CGPoint?
    private var area: CGRect?
    private let dim = CAShapeLayer(), edge = CAShapeLayer(), guides = CAShapeLayer()
    private let loupe = CALayer(), zoomed = CALayer(), center = CALayer()
    private let readout = CATextLayer(), size = CATextLayer()
    private static let pixels = 15, zoom: CGFloat = 8

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
        let side = CGFloat(Self.pixels) * Self.zoom
        loupe.frame = CGRect(x: 0, y: 0, width: side, height: side)
        loupe.shadowOpacity = 0.45
        loupe.shadowRadius = 8
        loupe.shadowOffset = CGSize(width: 0, height: -2)
        loupe.shadowPath = CGPath(roundedRect: loupe.bounds, cornerWidth: 10, cornerHeight: 10, transform: nil)
        zoomed.frame = loupe.bounds
        zoomed.cornerRadius = 10
        zoomed.masksToBounds = true
        zoomed.magnificationFilter = .nearest
        zoomed.backgroundColor = NSColor.black.cgColor
        zoomed.borderWidth = 1
        zoomed.borderColor = NSColor.white.withAlphaComponent(0.6).cgColor
        center.frame = CGRect(x: (side - Self.zoom) / 2 - 0.5, y: (side - Self.zoom) / 2 - 0.5, width: Self.zoom + 1, height: Self.zoom + 1)
        center.borderWidth = 1
        center.borderColor = NSColor.white.cgColor
        loupe.addSublayer(zoomed)
        loupe.addSublayer(center)
        loupe.isHidden = true
        for label in [readout, size] {
            label.contentsScale = scale
            label.backgroundColor = NSColor.black.withAlphaComponent(0.72).cgColor
            label.cornerRadius = 5
            label.alignmentMode = .center
            label.isHidden = true
        }
        [dim, edge, guides, loupe, readout, size].forEach { root.addSublayer($0); $0.contentsScale = scale }
        root.actions = Self.still
        [dim, edge, guides, loupe, zoomed, center, readout, size].forEach { $0.actions = Self.still }
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

    override func mouseMoved(with event: NSEvent) { picker.moved(to: NSEvent.mouseLocation) }
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
            loupe.isHidden = true
            readout.isHidden = true
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
            loupe.isHidden = true
            readout.isHidden = true
            return
        }
        let x = floor(here.x) + 0.5, y = floor(here.y) + 0.5
        let lines = CGMutablePath()
        lines.move(to: CGPoint(x: x, y: 0))
        lines.addLine(to: CGPoint(x: x, y: bounds.height))
        lines.move(to: CGPoint(x: 0, y: y))
        lines.addLine(to: CGPoint(x: bounds.width, y: y))
        guides.path = lines
        magnify(at: here)
    }

    /// The loupe and its readout beside the pointer, flipped away from the display's edges.
    private func magnify(at p: CGPoint) {
        let side = loupe.bounds.width, gap: CGFloat = 22
        let px = Int(p.x * screen.backingScaleFactor), py = Int((bounds.height - p.y) * screen.backingScaleFactor)
        var text = "\(Int(p.x)), \(Int(bounds.height - p.y))"
        if let snapshot {
            let half = Self.pixels / 2
            zoomed.contents = snapshot.cropping(to: CGRect(x: px - half, y: py - half, width: Self.pixels, height: Self.pixels))
            if let hex = Self.hex(snapshot, x: px, y: py) { text += "   " + hex }
        }
        let labelSize = measure(readout, text)
        var origin = CGPoint(x: p.x + gap, y: p.y - gap - side)
        if origin.x + side > bounds.width { origin.x = p.x - gap - side }
        if origin.y - labelSize.height - 6 < 0 { origin.y = p.y + gap + labelSize.height + 6 }
        loupe.isHidden = snapshot == nil
        loupe.frame.origin = origin
        let below = snapshot == nil ? CGPoint(x: p.x + gap, y: p.y - gap - labelSize.height) : CGPoint(x: origin.x + (side - labelSize.width) / 2, y: origin.y - labelSize.height - 6)
        readout.frame = CGRect(origin: below, size: labelSize)
        readout.isHidden = false
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

    /// The pixel's colour as `#RRGGBB`, read from the snapshot (top-left pixel coordinates).
    private static func hex(_ image: CGImage, x: Int, y: Int) -> String? {
        guard x >= 0, y >= 0, x < image.width, y < image.height,
              let pixel = image.cropping(to: CGRect(x: x, y: y, width: 1, height: 1)),
              let space = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        var rgba = [UInt8](repeating: 0, count: 4)
        let drawn = rgba.withUnsafeMutableBytes { bytes -> Bool in
            guard let context = CGContext(data: bytes.baseAddress, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                                          space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            context.draw(pixel, in: CGRect(x: 0, y: 0, width: 1, height: 1))
            return true
        }
        return drawn ? String(format: "#%02X%02X%02X", rgba[0], rgba[1], rgba[2]) : nil
    }
}

/// Each display's pixels once, without Glint's windows, for the loupe. Without screen recording permission it
/// asks nothing and the overlay simply has no loupe.
private enum Snapshot {
    static func take(_ each: @escaping (CGDirectDisplayID, CGImage) -> Void) {
        guard CGPreflightScreenCaptureAccess() else { return }
        SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: true) { content, _ in
            guard let content else { return }
            let own = content.applications.filter { $0.processID == getpid() }
            for display in content.displays {
                let filter = SCContentFilter(display: display, excludingApplications: own, exceptingWindows: [])
                let config = SCStreamConfiguration()
                let scale = CGFloat(filter.pointPixelScale)
                config.width = Int(CGFloat(display.width) * scale)
                config.height = Int(CGFloat(display.height) * scale)
                config.showsCursor = false
                let id = display.displayID
                SCScreenshotManager.captureImage(contentFilter: filter, configuration: config) { image, _ in
                    guard let image else { return }
                    DispatchQueue.main.async { each(id, image) }
                }
            }
        }
    }
}
