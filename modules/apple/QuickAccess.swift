// The quick-access thumbnails after a capture: floating panels at the bottom-left of the pointer's display, newest
// at the bottom, older ones stacked above. Hovering shows Copy, Save, Annotate and Pin (PNG only), Reveal and a
// close X; dragging the thumbnail drags the file out. Each leaves by itself after a few seconds unless hovered.
import AppKit
import AVFoundation
import UniformTypeIdentifiers

/// What a thumbnail's controls ask for.
enum QuickAction { case copy, save, annotate, pin, reveal, open, close }

/// Every quick-access panel of a session, and what their buttons do.
final class QuickStack {
    private weak var glint: Glint?
    /// Newest first: index 0 sits at the bottom.
    private var panels: [QuickPanel] = []
    private static let most = 5, margin: CGFloat = 16, gap: CGFloat = 10

    init(glint: Glint) { self.glint = glint }

    /// Load `url`'s thumbnail off the main thread and show its panel; `done` gets an error message or nil.
    func show(_ url: URL, path: String, kind: String, done: @escaping (String?) -> Void) {
        Thumbnail.load(url, kind: kind) { [weak self] image, badge in
            guard let self else { return done("the session ended") }
            guard let image else { return done("no thumbnail for \(url.lastPathComponent)") }
            let screen = Desktop.pointerScreen
            let panel = QuickPanel(image: image, badge: badge, url: url, path: path, kind: kind, screen: screen, stack: self)
            self.panels.insert(panel, at: 0)
            self.panels.dropFirst(Self.most).forEach { self.dismiss($0) }
            self.layout()
            done(nil)
        }
    }

    func closeAll() {
        panels.forEach { $0.close() }
        panels = []
    }

    func dismiss(_ panel: QuickPanel) {
        guard let index = panels.firstIndex(where: { $0 === panel }) else { return }
        panels.remove(at: index)
        panel.leave()
        layout()
    }

    func perform(_ action: QuickAction, on panel: QuickPanel) {
        let url = panel.url
        switch action {
        case .copy:
            do {
                try Files.copy(url, kind: panel.kind)
                panel.confirm("Copied") { [weak self] in self?.dismiss(panel) }
            } catch { NSSound.beep() }
        case .save: save(panel)
        case .annotate:
            glint?.post("annotate", path: panel.path)
            dismiss(panel)
        case .pin:
            do { try glint?.pin(url) } catch { NSSound.beep() }
            dismiss(panel)
        case .reveal:
            NSWorkspace.shared.activateFileViewerSelecting([url])
            dismiss(panel)
        case .open:
            NSWorkspace.shared.open(url)
            dismiss(panel)
        case .close: dismiss(panel)
        }
    }

    /// Ask where to save a copy; the panel stays while the sheet is open.
    private func save(_ panel: QuickPanel) {
        let url = panel.url
        let sheet = NSSavePanel()
        sheet.nameFieldStringValue = url.lastPathComponent
        sheet.allowedContentTypes = UTType(filenameExtension: url.pathExtension).map { [$0] } ?? []
        sheet.canCreateDirectories = true
        panel.busy = true
        NSApp.activate(ignoringOtherApps: true)
        sheet.begin { [weak self, weak panel] response in
            guard let panel else { return }
            panel.busy = false
            guard response == .OK, let target = sheet.url else { return panel.scheduleDismissal() }
            DispatchQueue.global(qos: .userInitiated).async {
                let manager = FileManager.default
                let saved = Result {
                    if manager.fileExists(atPath: target.path) { try manager.removeItem(at: target) }
                    try manager.copyItem(at: url, to: target)
                }
                DispatchQueue.main.async {
                    if case .failure = saved { NSSound.beep() }
                    self?.dismiss(panel)
                }
            }
        }
    }

    /// Each display's panels stacked upward from its bottom-left corner, newest lowest.
    private func layout() {
        var next: [CGDirectDisplayID: CGFloat] = [:]
        for panel in panels {
            let visible = panel.stackArea
            let y = next[panel.display] ?? visible.minY + Self.margin
            panel.place(at: CGPoint(x: visible.minX + Self.margin, y: y))
            next[panel.display] = y + panel.frame.height + Self.gap
        }
    }
}

/// One capture's floating thumbnail.
final class QuickPanel: GlintPanel {
    let url: URL, path: String, kind: String
    let display: CGDirectDisplayID
    /// The display's visible frame when the panel appeared.
    let stackArea: CGRect
    /// A save sheet or a drag is in flight: never dismiss meanwhile.
    var busy = false { didSet { if busy { dismissal?.cancel() } } }
    private weak var stack: QuickStack?
    private let controls: QuickControls
    private var shown = false, hovered = false
    private var dismissal: DispatchWorkItem?
    private static let life: TimeInterval = 6, inset: CGFloat = 5

    init(image: NSImage, badge: String?, url: URL, path: String, kind: String, screen: NSScreen, stack: QuickStack) {
        self.url = url
        self.path = path
        self.kind = kind
        self.stack = stack
        display = screen.displayID
        stackArea = screen.visibleFrame
        let thumb = Self.fit(image.size)
        let bounds = CGRect(x: 0, y: 0, width: thumb.width + Self.inset * 2, height: thumb.height + Self.inset * 2)
        let imageFrame = bounds.insetBy(dx: Self.inset, dy: Self.inset)
        controls = QuickControls(frame: imageFrame, image: image, url: url, still: kind == "png")
        super.init(frame: bounds, level: .statusBar)

        let root = HoverView(frame: bounds)
        root.addSubview(Chrome.backdrop(bounds, radius: 12))
        let picture = NSImageView(frame: imageFrame)
        picture.image = image
        picture.imageScaling = .scaleProportionallyUpOrDown
        picture.animates = true
        picture.wantsLayer = true
        picture.layer?.cornerRadius = 8
        picture.layer?.masksToBounds = true
        picture.layer?.borderWidth = 0.5
        picture.layer?.borderColor = NSColor.white.withAlphaComponent(0.14).cgColor
        root.addSubview(picture)
        if let badge { root.addSubview(Self.badge(badge, in: imageFrame)) }
        root.addSubview(controls)
        contentView = root

        controls.act = { [weak self] action in
            guard let self else { return }
            self.stack?.perform(action, on: self)
        }
        controls.dragging = { [weak self] on in
            guard let self else { return }
            self.busy = on
            if !on && !self.hovered { self.scheduleDismissal() }
        }
        controls.dropped = { [weak self] in
            guard let self else { return }
            self.stack?.dismiss(self)
        }
        root.hover = { [weak self] inside in self?.hover(inside) }
    }

    /// The thumbnail's size: at most 220 × 150 points, never so thin the controls do not fit.
    private static func fit(_ size: CGSize) -> CGSize {
        let aspect = size.height > 0 ? size.width / size.height : 1.5
        var width: CGFloat = 220, height = width / aspect
        if height > 150 { height = 150; width = height * aspect }
        return CGSize(width: max(width, 120).rounded(), height: max(height, 72).rounded())
    }

    private static func badge(_ text: String, in frame: CGRect) -> NSView {
        let label = NSTextField(labelWithString: text)
        label.font = .monospacedDigitSystemFont(ofSize: 10, weight: .semibold)
        label.textColor = .white
        label.sizeToFit()
        let pill = NSView(frame: CGRect(x: frame.minX + 6, y: frame.minY + 6, width: label.frame.width + 12, height: 18))
        pill.wantsLayer = true
        pill.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.6).cgColor
        pill.layer?.cornerRadius = 9
        label.frame.origin = CGPoint(x: 6, y: (18 - label.frame.height) / 2)
        pill.addSubview(label)
        return pill
    }

    /// Slide in from the left the first time, glide to a new place in the stack after that.
    func place(at origin: CGPoint) {
        let target = CGRect(origin: origin, size: frame.size)
        guard shown else {
            shown = true
            present(target, from: CGVector(dx: -(target.width + 24), dy: 0), duration: 0.28)
            return scheduleDismissal()
        }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.22
            context.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            animator().setFrame(target, display: true)
        }
    }

    func leave() {
        dismissal?.cancel()
        controls.act = { _ in }
        dismiss(toward: CGVector(dx: -(frame.width + 24), dy: 0), duration: 0.22)
    }

    func scheduleDismissal() {
        dismissal?.cancel()
        guard !hovered, !busy else { return }
        let item = DispatchWorkItem { [weak self] in
            guard let self, !self.hovered, !self.busy else { return }
            self.stack?.dismiss(self)
        }
        dismissal = item
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.life, execute: item)
    }

    /// Say `text` in place of the controls for a moment, then `then`.
    func confirm(_ text: String, then: @escaping () -> Void) {
        busy = true
        controls.confirm(text)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6, execute: then)
    }

    private func hover(_ inside: Bool) {
        hovered = inside
        controls.reveal(inside)
        if inside { dismissal?.cancel() } else { scheduleDismissal() }
    }
}

/// Reports the pointer entering and leaving, whether or not the window is key.
final class HoverView: NSView {
    var hover: (Bool) -> Void = { _ in }
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.activeAlways, .mouseEnteredAndExited, .inVisibleRect], owner: self))
    }
    override func mouseEntered(with event: NSEvent) { hover(true) }
    override func mouseExited(with event: NSEvent) { hover(false) }
}

/// The hover layer over a thumbnail: a dark wash with the buttons, and the drag source for the file.
private final class QuickControls: NSView, NSDraggingSource {
    var act: (QuickAction) -> Void = { _ in }
    var dragging: (Bool) -> Void = { _ in }
    var dropped: () -> Void = {}
    private let image: NSImage, url: URL, still: Bool
    private var down: CGPoint?
    private let note = NSTextField(labelWithString: "")

    init(frame: CGRect, image: NSImage, url: URL, still: Bool) {
        self.image = image
        self.url = url
        self.still = still
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = NSColor.black.withAlphaComponent(0.42).cgColor
        alphaValue = 0
        build()
    }

    required init?(coder: NSCoder) { fatalError("not from a nib") }

    private func build() {
        let w = bounds.width, h = bounds.height, pad: CGFloat = 6, icon: CGFloat = 22
        let send = { [weak self] (action: QuickAction) in self?.act(action) }
        let button = { (symbol: String, help: String, action: QuickAction, origin: CGPoint) in
            let b = ChromeButton(symbol: symbol, help: help, size: icon) { send(action) }
            b.frame.origin = origin
            self.addSubview(b)
        }
        button("xmark", "Close", .close, CGPoint(x: pad, y: h - pad - icon))
        button("folder", "Show in Finder", .reveal, CGPoint(x: w - pad - icon, y: pad))
        if still {
            button("pencil", "Annotate", .annotate, CGPoint(x: w - pad - icon, y: h - pad - icon))
            button("pin", "Pin to screen", .pin, CGPoint(x: w - pad - icon * 2 - 4, y: h - pad - icon))
        }
        let copy = ChromeButton(title: "Copy", help: "Copy", style: .pill, size: 24) { send(.copy) }
        let save = ChromeButton(title: "Save", help: "Save as…", style: .pill, size: 24) { send(.save) }
        let width = max(copy.frame.width, save.frame.width, 70)
        copy.frame.size.width = width
        save.frame.size.width = width
        if h >= 96 {
            copy.frame.origin = CGPoint(x: (w - width) / 2, y: h / 2 + 3)
            save.frame.origin = CGPoint(x: (w - width) / 2, y: h / 2 - 27)
        } else {
            copy.frame.origin = CGPoint(x: w / 2 - width - 3, y: (h - 24) / 2)
            save.frame.origin = CGPoint(x: w / 2 + 3, y: (h - 24) / 2)
        }
        addSubview(copy)
        addSubview(save)
        note.font = .systemFont(ofSize: 13, weight: .semibold)
        note.textColor = .white
        note.alignment = .center
        note.isHidden = true
        note.frame = CGRect(x: 0, y: (h - 18) / 2, width: w, height: 18)
        addSubview(note)
    }

    func reveal(_ on: Bool) {
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.15
            animator().alphaValue = on ? 1 : 0
        }
    }

    func confirm(_ text: String) {
        subviews.forEach { $0.isHidden = $0 !== note }
        note.stringValue = text
        note.isHidden = false
        reveal(true)
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) { down = event.locationInWindow }

    override func mouseDragged(with event: NSEvent) {
        guard let start = down else { return }
        let p = event.locationInWindow
        guard hypot(p.x - start.x, p.y - start.y) > 3 else { return }
        down = nil
        let item = NSDraggingItem(pasteboardWriter: url as NSURL)
        item.setDraggingFrame(bounds, contents: image)
        dragging(true)
        beginDraggingSession(with: [item], event: event, source: self)
    }

    override func mouseUp(with event: NSEvent) {
        defer { down = nil }
        guard down != nil, event.clickCount == 2 else { return }
        act(still ? .annotate : .open)
    }

    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        context == .outsideApplication ? .copy : []
    }

    func draggingSession(_ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) {
        dragging(false)
        if operation != [] { dropped() }
    }
}

/// A capture's thumbnail and its corner badge (a video's length, "GIF"), made off the main thread.
private enum Thumbnail {
    static func load(_ url: URL, kind: String, done: @escaping (NSImage?, String?) -> Void) {
        let answer = { (image: NSImage?, badge: String?) in DispatchQueue.main.async { done(image, badge) } }
        switch kind {
        case "mp4", "mov":
            Task {
                let asset = AVURLAsset(url: url)
                let generator = AVAssetImageGenerator(asset: asset)
                generator.appliesPreferredTrackTransform = true
                generator.maximumSize = CGSize(width: 640, height: 640)
                let frame = try? await generator.image(at: CMTime(value: 0, timescale: 600)).image
                let seconds = (try? await asset.load(.duration)).map(CMTimeGetSeconds) ?? 0
                answer(frame.map { NSImage(cgImage: $0, size: CGSize(width: $0.width, height: $0.height)) }, "▶ " + Chrome.clock(seconds))
            }
        case "gif":
            DispatchQueue.global(qos: .userInitiated).async { answer(NSImage(contentsOf: url), "GIF") }
        default:
            DispatchQueue.global(qos: .userInitiated).async {
                let options: [CFString: Any] = [
                    kCGImageSourceCreateThumbnailFromImageAlways: true,
                    kCGImageSourceCreateThumbnailWithTransform: true,
                    kCGImageSourceThumbnailMaxPixelSize: 640,
                ]
                guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
                      let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return answer(nil, nil) }
                answer(NSImage(cgImage: image, size: CGSize(width: image.width, height: image.height)), nil)
            }
        }
    }
}
