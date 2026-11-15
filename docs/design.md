# Glint's design

Three layers, so everything but the platform's windows is shared:

| Layer | Where | Shared with Linux |
|---|---|---|
| View, editor, library, settings | `app.contract`, `app.ts` | yes: the main window is the whole UI on Linux |
| Capture and recording | `capture/` (`glint-capture`), `data/src/lib.rs` | yes: one interface, a ScreenCaptureKit back end and an xdg-desktop-portal one |
| Overlays and floating windows | `modules/apple/*.swift` | no: the Linux host draws its own |

## A capture's path

1. A shortcut, the menu bar item or a button sends a Rust source (`captureScreen`, `captureArea`, `captureWindow`,
   `stopRecording`). Rust writes the file to a staging folder (`/private/tmp/glint` on macOS, `/tmp/glint` on Linux)
   and answers `Staged`.
2. `app.ts` `keepCapture` moves it into `app:/data/captures`: on the Mac the Swift module's `adopt` renames the file
   (no bytes through JavaScript); elsewhere it reads and writes it.
3. The view lists it, copies it, and the Swift module shows the quick-access panel.

## Names in `app:/data/captures`

- `glint-<taken>-<w>x<h>.png`, `.mp4` or `.gif`: the original. For video, `glint-<taken>-<w>x<h>-<ms>.mp4`.
- `<original stem>.mosaic.png`: a 1/16-size copy of a PNG, which the editor scales up for its pixelate tool.
- `<original stem>.edit.json`: the editor's shapes, so an annotation stays editable.
- `<original stem>.edit.png`: the annotated render. Copy, drag and pin use it when it exists.

## Rust sources (`RUST_SOURCES`)

All answer `Answer::Now`. Points are the desktop's global points, origin top-left (CoreGraphics); pixels are
points × the display's scale.

| Source | Args | Answers |
|---|---|---|
| `captureScreen` | none | `Staged` of the display under the pointer |
| `captureArea` | `x, y, width, height` | `Staged`, clipped to the display holding the area's centre |
| `captureWindow` | `id` (CGWindowID) | `Staged`, the window alone with its shadow on transparency |
| `startRecording` | `x, y, width, height, kind, cursor, clicks, audio, microphone` | `Recording`. `width == 0` records the display under the pointer. `kind` is `"mp4"`, `"gif"` or `"scroll"` |
| `stopRecording` | none | `Staged` of the finished file; a `"scroll"` one is a PNG with its mosaic |

A scrolling capture is a recording whose frames go to a stitcher (`capture/src/scroll.rs`) instead of the GIF
encoder: each frame is matched to the last by row hashes, rows fixed at the top and bottom (a toolbar, a sticky
header) are kept once, and the band between them grows the picture as the person scrolls. Frames are first cut
to the first frame's opaque part: GNOME casts a window as a display-sized frame with the window at the top left.
| `platform` | none | `{os}`: Rust's `std::env::consts::OS` (`"macos"`, `"linux"`), which hides what a platform lacks |

Glint's own windows (overlays, panels, pins, HUD, main window) are excluded from every capture.

## Swift module ops (`native.later` unless marked `call`)

`app.ts` uses the module only once a `requests` call answers: on Linux `native.available` is true (the host takes its
packages' calls) though no module answers the app's.

| op | Request | Reply |
|---|---|---|
| `pick` | `mode`: `"area"`, `"window"` or `"record"` | `{picked, x, y, width, height, window}`. Space toggles area and window. A window pick sets `window` (CGWindowID) and its frame; otherwise `window` is 0. In `record`, a click without a drag picks the whole display under it. Escape or a right click: `picked: false` |
| `adopt` | `from` (native path), `to` (`app:/data/...`) | `{path}`; moves the file and calls `wrote` |
| `quick` | `path` (`app:/`), `kind` | `{ok}`; shows the quick-access panel |
| `pin` | `path` | `{ok}`; a floating, always-on-top image |
| `hud` | `x, y, width, height, kind` or `hide: true` | `{ok}`; the recording frame and its Stop button, or Done when `kind` is `"scroll"` |
| `copy` | `path`, `kind` | `{ok}`; image data for a PNG, a file URL for anything |
| `reveal`, `open` | `path` | `{ok}`; Finder, or the default app |
| `requests` (`call`) | none | `{seq, kind, path}`: the latest thing a panel or the HUD asked the app to do (`annotate`, `stopRecording`). The module calls `changed("glint.requests")` after each |

## Linux notes

Capture goes through xdg-desktop-portal (`capture/src/linux.rs`), so it runs on GNOME and KDE under Wayland. Wayland
shows no app the pointer or other windows, so the desktop's own pickers stand in for Glint's overlay:

| Source | On Linux |
|---|---|
| `captureScreen` | The whole desktop, every display, from the Screenshot portal without a picker. The first one asks the person to allow screenshots (GNOME: "Allow Apps to Take Screenshots?"); a refusal is kept and every later call fails until it is changed in Settings, Privacy and Security |
| `captureArea` | An empty rect opens the desktop's screenshot UI and answers what the person picked there (area, window or screen). A non-empty rect (from a future overlay) takes the desktop and crops to it, in the picture's pixels |
| `captureWindow` | The same screenshot UI; `id` is unused. The person picks Window in it |
| `startRecording` | A ScreenCast portal session. The first recording shows the desktop's chooser (a display, or a window when no area is given); its restore token is kept in the staging folder (`/tmp/glint/screencast.token`), so later ones start without it until a reboot clears `/tmp`. An area is cropped from the chosen display. `clicks` is ignored |
| `startRecording` `"scroll"` | Always shows the chooser (no restore token), since each one is likely a different window; pick the window to scroll. Start it with the shortcut from that window: GNOME gives focus back to it when the chooser closes, where the Scroll button leaves Glint's window on top. Frames come at up to 30 fps |
| `stopRecording` | Ends the `gst-launch-1.0` child that encodes the PipeWire stream: EOS for MP4, so mp4mux writes its index |

- A cancelled picker answers an error ending "was cancelled".
- GNOME's screenshot UI also copies an interactive pick to the clipboard and posts a "Screenshot captured" notice.
- Glint's own windows are not excluded: the portal has no way to leave an app out.
- Recording needs GStreamer with its PipeWire plugin and an H.264 encoder (openh264enc or x264enc); MP4 frames are
  the display's logical size, so a HiDPI display records at 1x. System audio (`@DEFAULT_MONITOR@`) and the microphone
  (`@DEFAULT_SOURCE@`) come through `pulsesrc`, which PipeWire's Pulse server answers, mixed into AAC.
- A GIF reads raw RGBA frames from the child's stdout into the shared encoder (`capture/src/gif.rs`) at up to
  15 fps, so its timing matches the Mac's: a still screen is one frame.

### What the Linux build leaves out

The view reads `platform` and hides what Linux has no way to do yet:

- The menu bar item, the floating thumbnail, Pin, the recording HUD, Show in Finder and click highlights: GNOME has no
  tray without an extension, and these are the Mac's panels. The main window is the UI.
- Text capture: `@exact/text-detection` has no Linux implementation. The Text button is gone and its shortcut says so.
- A recording's preview and Copy: the Linux host plays no video and decodes no GIF, and `@exact/clipboard` there copies
  PNGs and text only. A recording shows a line saying so.

The window's Wayland app id is the app's id, so GNOME matches it to `dev.mchisolm0.glint.desktop`. The build writes
that file beside the binary; the GlobalShortcuts portal refuses an app id with no desktop entry ("An app id is
required"), so install it once (README). Under the agent (`EXACT_AGENT=1`, the app's tests) nobody answers the
desktop's picker, so an empty area is the fixed 400x300 one the Mac's module picks there.
