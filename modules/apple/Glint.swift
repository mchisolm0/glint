// Glint's native module on the Mac (docs/design.md, "Swift module ops"): the pick overlay, the quick-access
// thumbnails, pins, the recording HUD, and the file work (adopt, copy, reveal, open) that should not pass bytes
// through JavaScript. Every entry arrives on the main thread; file moves go to `files`.
import AppKit
import Foundation

final class Glint: ExactModule {
    private var picker: Picker?
    private lazy var quick = QuickStack(glint: self)
    private var pins: [PinWindow] = []
    private var hud: RecordingHUD?
    /// The latest thing a panel or the HUD asked the app to do; `requests` answers it.
    private var latest = (seq: 0, kind: "", path: "")
    private let files = DispatchQueue(label: "dev.mchisolm0.glint.files", qos: .userInitiated)

    override func later(_ request: [String: Any], reply: ExactReply) {
        let op = request["op"] as? String ?? ""
        let path = request["path"] as? String ?? ""
        // Under the agent (the app's tests, on the person's own Mac) nothing touches the real desktop: pick answers
        // a fixed stand-in and the ops that show or copy answer ok. adopt and requests still do their work.
        if context.agent {
            switch op {
            case "pick": return reply.send(["picked": true, "x": 100, "y": 100, "width": 400, "height": 300, "window": 0])
            case "quick", "pin", "hud", "copy", "reveal", "open": return reply.send(["ok": true])
            default: break
            }
        }
        do {
            switch op {
            case "pick": pick(request["mode"] as? String ?? "area", reply)
            case "adopt": try adopt(from: request["from"] as? String ?? "", to: request["to"] as? String ?? "", reply)
            case "quick":
                let url = try file(path)
                quick.show(url, path: path, kind: kind(request, url)) { error in
                    if let error { reply.fail(error) } else { reply.send(["ok": true]) }
                }
            case "pin":
                try pin(try file(path))
                reply.send(["ok": true])
            case "hud":
                if request["hide"] as? Bool == true {
                    hud?.close()
                    hud = nil
                } else {
                    let number = { (key: String) in CGFloat((request[key] as? NSNumber)?.doubleValue ?? 0) }
                    hud?.close()
                    hud = RecordingHUD(area: CGRect(x: number("x"), y: number("y"), width: number("width"), height: number("height"))) { [weak self] in
                        self?.post("stopRecording", path: "")
                    }
                }
                reply.send(["ok": true])
            case "copy":
                let url = try file(path)
                try Files.copy(url, kind: kind(request, url))
                reply.send(["ok": true])
            case "reveal":
                NSWorkspace.shared.activateFileViewerSelecting([try file(path)])
                reply.send(["ok": true])
            case "open":
                NSWorkspace.shared.open(try file(path))
                reply.send(["ok": true])
            default:
                reply.fail("unknown op \(op)")
            }
        } catch {
            reply.fail("\(op): \(error)")
        }
    }

    override func call(_ request: [String: Any]) throws -> [String: Any] {
        guard request["op"] as? String == "requests" else { throw ExactNativeRefusal("unknown call \(request["op"] ?? "")") }
        return ["seq": latest.seq, "kind": latest.kind, "path": latest.path]
    }

    override func destroy() {
        picker?.cancel()
        quick.closeAll()
        pins.forEach { $0.close() }
        pins = []
        hud?.close()
        hud = nil
    }

    /// Record a request from a panel or the HUD and wake every answer watching `glint.requests`.
    func post(_ kind: String, path: String) {
        latest = (latest.seq + 1, kind, path)
        context.changed("glint.requests")
    }

    /// Show `url` as a pin. Throws when it is not an image.
    func pin(_ url: URL) throws {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { throw ExactNativeRefusal("not an image: \(url.path)") }
        let pin = PinWindow(image: image, url: url, cascade: pins.count) { [weak self] closed in
            self?.pins.removeAll { $0 === closed }
        }
        pins.append(pin)
    }

    /// A request's path as a file URL: `app:/data/...` under the session's data directory, or an absolute native path.
    func file(_ path: String) throws -> URL {
        let prefix = "app:/data/"
        if path.hasPrefix(prefix) {
            let root = context.data.standardizedFileURL
            let url = root.appendingPathComponent(String(path.dropFirst(prefix.count))).standardizedFileURL
            guard url.path.hasPrefix(root.path + "/") else { throw ExactNativeRefusal("\(path) is outside app:/data") }
            return url
        }
        if path.hasPrefix("/") { return URL(fileURLWithPath: path).standardizedFileURL }
        throw ExactNativeRefusal("not a file path: \(path)")
    }

    private func kind(_ request: [String: Any], _ url: URL) -> String {
        request["kind"] as? String ?? url.pathExtension.lowercased()
    }

    private func pick(_ mode: String, _ reply: ExactReply) {
        guard picker == nil else { return reply.fail("pick: already picking") }
        let none: [String: Any] = ["picked": false, "x": 0, "y": 0, "width": 0, "height": 0, "window": 0]
        let picker = Picker(mode: mode)
        self.picker = picker
        picker.start { [weak self] pick in
            self?.picker = nil
            guard let pick else { return reply.send(none) }
            // The overlay must be off screen before Rust captures, or it is in the picture.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                reply.send(["picked": true, "x": Double(pick.rect.minX), "y": Double(pick.rect.minY),
                            "width": Double(pick.rect.width), "height": Double(pick.rect.height), "window": Int(pick.window)])
            }
        }
    }

    private func adopt(from: String, to: String, _ reply: ExactReply) throws {
        guard from.hasPrefix("/") else { throw ExactNativeRefusal("from is not a native path: \(from)") }
        let source = URL(fileURLWithPath: from), target = try file(to)
        let folder = to.lastIndex(of: "/").map { String(to[..<$0]) } ?? "app:/data"
        files.async { [context] in
            do {
                try Files.move(source, to: target)
                context.wrote(folder)
                reply.send(["path": to])
            } catch {
                reply.fail("adopt: \(error.localizedDescription)")
            }
        }
    }
}

/// File work shared by the ops and the panels.
enum Files {
    /// Rename `source` to `target`, replacing it; across volumes, copy and delete.
    static func move(_ source: URL, to target: URL) throws {
        let manager = FileManager.default
        try manager.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        if manager.fileExists(atPath: target.path) { try manager.removeItem(at: target) }
        if rename(source.path, target.path) == 0 { return }
        guard errno == EXDEV else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        try manager.copyItem(at: source, to: target)
        try manager.removeItem(at: source)
    }

    /// The pasteboard holds the file's URL, and for a PNG its image data too.
    static func copy(_ url: URL, kind: String) throws {
        let item = NSPasteboardItem()
        if kind == "png" { item.setData(try Data(contentsOf: url), forType: .png) }
        item.setString(url.absoluteString, forType: .fileURL)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.writeObjects([item])
    }
}

/// AppKit's global space has its origin at the primary display's bottom-left; CoreGraphics' (and every reply's)
/// at its top-left. Flipping is its own inverse.
enum Desktop {
    private static var height: CGFloat { NSScreen.screens.first?.frame.height ?? 0 }
    static func flip(_ rect: CGRect) -> CGRect { CGRect(x: rect.minX, y: height - rect.maxY, width: rect.width, height: rect.height) }
    static func flip(_ point: CGPoint) -> CGPoint { CGPoint(x: point.x, y: height - point.y) }
    /// The display under the pointer.
    static var pointerScreen: NSScreen {
        let mouse = NSEvent.mouseLocation
        return NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) } ?? NSScreen.main ?? NSScreen.screens[0]
    }
}

extension NSScreen {
    var displayID: CGDirectDisplayID { deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? CGDirectDisplayID ?? 0 }
}

/// The base of every floating Glint window: borderless, non-activating, dark, on every Space, and kept out of
/// screen sharing. Only windows that set `acceptsKey` (pins) take keyboard focus, and then without activating.
class GlintPanel: NSPanel {
    var acceptsKey = false
    override var canBecomeKey: Bool { acceptsKey }
    override var canBecomeMain: Bool { false }

    init(frame: CGRect, level: NSWindow.Level) {
        super.init(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        self.level = level
        isFloatingPanel = true
        hidesOnDeactivate = false
        becomesKeyOnlyIfNeeded = true
        isReleasedWhenClosed = false
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        sharingType = .none
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        appearance = NSAppearance(named: .darkAqua)
        animationBehavior = .none
    }

    /// Fade in from `offset` to `frame`.
    func present(_ frame: CGRect, from offset: CGVector = .zero, duration: TimeInterval = 0.2) {
        setFrame(frame.offsetBy(dx: offset.dx, dy: offset.dy), display: false)
        alphaValue = 0
        orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { context in
            context.duration = duration
            context.timingFunction = CAMediaTimingFunction(name: .easeOut)
            animator().setFrame(frame, display: true)
            animator().alphaValue = 1
        }
    }

    /// Fade out toward `offset`, then close.
    func dismiss(toward offset: CGVector = .zero, duration: TimeInterval = 0.18) {
        let target = frame.offsetBy(dx: offset.dx, dy: offset.dy)
        NSAnimationContext.runAnimationGroup({ context in
            context.duration = duration
            context.timingFunction = CAMediaTimingFunction(name: .easeIn)
            animator().setFrame(target, display: true)
            animator().alphaValue = 0
        }, completionHandler: { [weak self] in self?.close() })
    }
}

/// Glint's floating surfaces: a HUD-material backdrop with rounded corners.
enum Chrome {
    static func backdrop(_ frame: CGRect, radius: CGFloat) -> NSVisualEffectView {
        let view = NSVisualEffectView(frame: frame)
        view.material = .hudWindow
        view.blendingMode = .behindWindow
        view.state = .active
        view.autoresizingMask = [.width, .height]
        // A mask image, unlike a layer's corner radius, also clips the blur and shapes the window's shadow.
        let edge = radius * 2 + 1
        let mask = NSImage(size: CGSize(width: edge, height: edge), flipped: false) { rect in
            NSColor.black.setFill()
            NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
            return true
        }
        mask.capInsets = NSEdgeInsets(top: radius, left: radius, bottom: radius, right: radius)
        mask.resizingMode = .stretch
        view.maskImage = mask
        return view
    }

    static func symbol(_ name: String, size: CGFloat = 12, weight: NSFont.Weight = .semibold) -> NSImage? {
        NSImage(systemSymbolName: name, accessibilityDescription: nil)?
            .withSymbolConfiguration(.init(pointSize: size, weight: weight))
    }

    /// `m:ss`, or `h:mm:ss` from an hour.
    static func clock(_ seconds: Double) -> String {
        let s = Int(seconds.isFinite ? seconds.rounded() : 0)
        return s >= 3600 ? String(format: "%d:%02d:%02d", s / 3600, s / 60 % 60, s % 60) : String(format: "%d:%02d", s / 60, s % 60)
    }
}

/// A borderless button for floating chrome: an SF Symbol in a dark circle (`icon`) or a light capsule with a
/// title (`pill`). It takes the first click on a window that is not key, brightens under the pointer and runs
/// `action`.
final class ChromeButton: NSButton {
    enum Style { case icon, pill }
    private let style: Style
    private let press: () -> Void

    init(symbol: String? = nil, title: String? = nil, help: String, style: Style = .icon, size: CGFloat = 24, action: @escaping () -> Void) {
        self.style = style
        press = action
        let font = NSFont.systemFont(ofSize: 12, weight: .semibold)
        let width = title.map { ($0 as NSString).size(withAttributes: [.font: font]).width + 26 } ?? size
        super.init(frame: CGRect(x: 0, y: 0, width: max(width, size), height: size))
        isBordered = false
        setButtonType(.momentaryChange)
        focusRingType = .none
        toolTip = help
        setAccessibilityLabel(help)
        if let symbol {
            image = Chrome.symbol(symbol, size: style == .icon ? size * 0.46 : 11)
            imagePosition = title == nil ? .imageOnly : .imageLeading
            imageScaling = .scaleNone
        }
        let color: NSColor = style == .icon ? .white : .black
        contentTintColor = color
        attributedTitle = NSAttributedString(string: title ?? "", attributes: [.font: font, .foregroundColor: color])
        wantsLayer = true
        layer?.cornerRadius = size / 2
        if style == .icon {
            layer?.borderWidth = 0.5
            layer?.borderColor = NSColor.white.withAlphaComponent(0.18).cgColor
        }
        lit(false)
        target = self
        self.action = #selector(fire)
    }

    required init?(coder: NSCoder) { fatalError("not from a nib") }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.activeAlways, .mouseEnteredAndExited, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) { lit(true) }
    override func mouseExited(with event: NSEvent) { lit(false) }

    private func lit(_ on: Bool) {
        layer?.backgroundColor = style == .icon
            ? NSColor.black.withAlphaComponent(on ? 0.8 : 0.55).cgColor
            : NSColor.white.withAlphaComponent(on ? 1 : 0.86).cgColor
    }

    @objc private func fire() { press() }
}

let exactModule: ExactModule.Type = Glint.self
