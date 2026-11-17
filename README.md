# Glint

A screenshot and screen-recording tool built on [Exact](https://github.com/expo/exact): global shortcuts, a menu bar
item, an annotation editor, and capture done in Rust (`glint-capture`) behind one interface, with ScreenCaptureKit
on the Mac and the desktop portal on Linux.

| Hyper (⌃⌥⇧⌘) + | Does |
|---|---|
| 3 | Capture the screen under the pointer |
| 4 | Drag out an area; Space switches to picking a window; Escape cancels |
| 5 | Record an area or a display (a click picks the display); again to stop |
| 2 | Capture the window that has focus |
| 1 | Scrolling capture: pick an area, scroll it, then Done (or Hyper+1 again) |
| T | Capture text (OCR) to the clipboard |

After a capture: it is copied, and a thumbnail floats bottom-left (copy, save, annotate, pin, drag it out). Every
screenshot opens in the editor, no mode to enter: A arrow, R box, O ellipse, L line, P pen, H highlighter, T text,
B pixelate, N numbered steps, S spotlight, C crop, and a background. It saves at the picture's own pixels and stays
editable; Copy saves unsaved marks first, and so does moving to another capture. Recordings are MP4 (system audio and the
microphone optional) or GIF.

## Where things live

| Piece | File | Platforms |
|---|---|---|
| View, editor, library, settings | `app.contract`, `shapes.contract`, `app.ts` | every host |
| Capture and recording | `capture/` (`glint-capture`), `data/src/lib.rs` | macOS, Linux |
| Overlays and floating panels | `modules/apple/*.swift` | macOS |

[docs/design.md](docs/design.md) has the interfaces between them.

## Run

Glint needs the `glint-next` branch of [mchisolm0/exact](https://github.com/mchisolm0/exact/tree/glint-next) (a Canvas
2D image can name an `app:/` file, host binaries target macOS 14, and a Linux desktop window keeps a chosen folder on
GNOME), checked out beside it at `../exact-glint` (`git worktree add ../exact-glint glint-next` in an Exact checkout).
Bun 1.4.2, and Xcode's `clang` and `swift` first on `PATH`.

```sh
export EXACT=../exact-glint
bun $EXACT/scripts/exact.mjs setup   # once
bun exact.mjs mac --run
bun exact.mjs test macos --size 1100x760   # the editor needs a window's width, not the default phone size
```

The first capture asks for Screen Recording permission.

### Linux (GNOME 48 or later, Wayland)

Needs `wl-clipboard`, `xdg-desktop-portal-gnome`, and GStreamer with its PipeWire plugin and `openh264enc` or
`x264enc`. The window is the UI; docs/design.md, "Linux notes", lists what the Mac has that Linux lacks.

```sh
bun exact.mjs linux
scripts/install-linux.sh   # ~/.local/opt/glint, an app menu entry with the icon, and autostart at login
```

The first launch asks to add the shortcuts (change them in Settings, Apps, Glint), the first capture to allow
screenshots, and the first recording which display to share.
