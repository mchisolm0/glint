// A capture pinned above every window at its point size. Drag it by its content, scroll or pinch to resize,
// Option+scroll for opacity, Escape or the hover X to close, right click for more.
import AppKit

final class PinWindow: GlintPanel {
    private let url: URL
    /// The image's point size on the display it opened on: pixels over the backing scale.
    private let natural: CGSize
    private let view: PinView
    private var onClose: ((PinWindow) -> Void)?

    init(image: CGImage, url: URL, cascade: Int, onClose: @escaping (PinWindow) -> Void) {
        self.url = url
        self.onClose = onClose
        let screen = Desktop.pointerScreen, visible = screen.visibleFrame
        natural = CGSize(width: CGFloat(image.width) / screen.backingScaleFactor, height: CGFloat(image.height) / screen.backingScaleFactor)
        let fit = min(1, visible.width * 0.8 / natural.width, visible.height * 0.8 / natural.height)
        let size = CGSize(width: (natural.width * fit).rounded(), height: (natural.height * fit).rounded())
        let step = CGFloat(cascade % 8) * 24
        let frame = CGRect(x: visible.midX - size.width / 2 + step, y: visible.midY - size.height / 2 - step, width: size.width, height: size.height)
        view = PinView(frame: CGRect(origin: .zero, size: size), image: image)
        super.init(frame: frame, level: .floating)
        acceptsKey = true
        becomesKeyOnlyIfNeeded = false
        contentView = view
        view.pin = self
        present(frame, from: CGVector(dx: 0, dy: -6), duration: 0.16)
    }

    override func close() {
        onClose?(self)
        onClose = nil
        super.close()
    }

    override func cancelOperation(_ sender: Any?) { unpin() }

    @objc func unpin() { dismiss(duration: 0.12) }

    @objc func copyImage() {
        do { try Files.copy(url, kind: url.pathExtension.lowercased()) } catch { NSSound.beep() }
    }

    @objc func revealImage() { NSWorkspace.shared.activateFileViewerSelecting([url]) }

    @objc func actualSize() { zoom(to: natural, around: CGPoint(x: frame.midX, y: frame.midY)) }

    /// Scale by `factor`, keeping the point under the pointer still.
    func zoom(by factor: CGFloat, around point: CGPoint) {
        zoom(to: CGSize(width: frame.width * factor, height: frame.height * factor), around: point)
    }

    private func zoom(to wanted: CGSize, around point: CGPoint) {
        let old = frame
        let aspect = natural.width / natural.height
        // Between 48 points on the short side and six times the point size.
        var width = min(max(wanted.width, 48 * max(aspect, 1)), natural.width * 6)
        if width / aspect < 48 { width = 48 * aspect }
        let size = CGSize(width: width.rounded(), height: (width / aspect).rounded())
        let fx = (point.x - old.minX) / old.width, fy = (point.y - old.minY) / old.height
        setFrame(CGRect(x: point.x - fx * size.width, y: point.y - fy * size.height, width: size.width, height: size.height), display: true)
        invalidateShadow()
        view.flash("\(Int((size.width / natural.width * 100).rounded()))%")
    }

    func fade(by delta: CGFloat) {
        alphaValue = min(max(alphaValue + delta, 0.2), 1)
        view.flash("\(Int((alphaValue * 100).rounded()))% opacity")
    }
}

/// The pinned image, its hover close button and a passing readout of the zoom or opacity.
private final class PinView: NSView {
    weak var pin: PinWindow?
    private let picture = CALayer()
    private let readout = NSTextField(labelWithString: "")
    private var close: ChromeButton!
    private var fading: DispatchWorkItem?

    init(frame: CGRect, image: CGImage) {
        super.init(frame: frame)
        autoresizingMask = [.width, .height]
        wantsLayer = true
        picture.contents = image
        picture.contentsGravity = .resize
        picture.minificationFilter = .trilinear
        picture.frame = bounds
        picture.autoresizingMask = [.layerWidthSizable, .layerHeightSizable]
        picture.actions = ["bounds": NSNull(), "position": NSNull()]
        picture.borderWidth = 0.5
        picture.borderColor = NSColor.white.withAlphaComponent(0.12).cgColor
        layer?.addSublayer(picture)
        close = ChromeButton(symbol: "xmark", help: "Close", size: 22) { [weak self] in self?.pin?.unpin() }
        close.frame.origin = CGPoint(x: 8, y: frame.height - 30)
        close.autoresizingMask = [.maxXMargin, .minYMargin]
        close.alphaValue = 0
        addSubview(close)
        readout.font = .monospacedDigitSystemFont(ofSize: 11, weight: .semibold)
        readout.textColor = .white
        readout.alignment = .center
        readout.wantsLayer = true
        readout.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.7).cgColor
        readout.layer?.cornerRadius = 6
        readout.alphaValue = 0
        addSubview(readout)
    }

    required init?(coder: NSCoder) { fatalError("not from a nib") }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.activeAlways, .mouseEnteredAndExited, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) { show(close, true) }
    override func mouseExited(with event: NSEvent) { show(close, false) }

    override func mouseDown(with event: NSEvent) {
        window?.makeKey()
        window?.performDrag(with: event)
    }

    override func scrollWheel(with event: NSEvent) {
        guard let pin else { return }
        let delta = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * 10
        if event.modifierFlags.contains(.option) { pin.fade(by: delta * 0.004) }
        else { pin.zoom(by: exp(delta * 0.006), around: NSEvent.mouseLocation) }
    }

    override func magnify(with event: NSEvent) { pin?.zoom(by: 1 + event.magnification, around: NSEvent.mouseLocation) }

    override func keyDown(with event: NSEvent) {
        if event.modifierFlags.contains(.command), event.charactersIgnoringModifiers == "c" { pin?.copyImage() }
        else if event.keyCode == 53 { pin?.unpin() }
        else { super.keyDown(with: event) }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = NSMenu()
        for (title, action) in [("Copy", #selector(PinWindow.copyImage)), ("Show in Finder", #selector(PinWindow.revealImage)), ("Actual Size", #selector(PinWindow.actualSize))] {
            menu.addItem(withTitle: title, action: action, keyEquivalent: "").target = pin
        }
        menu.addItem(.separator())
        menu.addItem(withTitle: "Close", action: #selector(PinWindow.unpin), keyEquivalent: "").target = pin
        return menu
    }

    /// Show `text` centred for a moment.
    func flash(_ text: String) {
        readout.stringValue = text
        readout.sizeToFit()
        let size = CGSize(width: readout.frame.width + 16, height: readout.frame.height + 6)
        readout.frame = CGRect(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2, width: size.width, height: size.height)
        readout.alphaValue = 1
        fading?.cancel()
        let item = DispatchWorkItem { [weak self] in self.map { $0.show($0.readout, false) } }
        fading = item
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8, execute: item)
    }

    private func show(_ view: NSView, _ on: Bool) {
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.15
            view.animator().alphaValue = on ? 1 : 0
        }
    }
}
