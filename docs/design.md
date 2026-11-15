# Glint's design

Three layers, so everything but the platform's windows is shared:

| Layer | Where | Shared with Linux |
|---|---|---|
| View, editor, library, settings | `app.contract`, `app.ts` | yes, once Exact's Linux host has a desktop window |
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
| `startRecording` | `x, y, width, height, kind, cursor, clicks, audio, microphone` | `Recording`. `width == 0` records the display under the pointer. `kind` is `"mp4"` or `"gif"` |
| `stopRecording` | none | `Staged` of the finished file |

Glint's own windows (overlays, panels, pins, HUD, main window) are excluded from every capture.

## Swift module ops (`native.later` unless marked `call`)

| op | Request | Reply |
|---|---|---|
| `pick` | `mode`: `"area"`, `"window"` or `"record"` | `{picked, x, y, width, height, window}`. Space toggles area and window. A window pick sets `window` (CGWindowID) and its frame; otherwise `window` is 0. In `record`, a click without a drag picks the whole display under it. Escape or a right click: `picked: false` |
| `adopt` | `from` (native path), `to` (`app:/data/...`) | `{path}`; moves the file and calls `wrote` |
| `quick` | `path` (`app:/`), `kind` | `{ok}`; shows the quick-access panel |
| `pin` | `path` | `{ok}`; a floating, always-on-top image |
| `hud` | `x, y, width, height` or `hide: true` | `{ok}`; the recording frame and its Stop button |
| `copy` | `path`, `kind` | `{ok}`; image data for a PNG, a file URL for anything |
| `reveal`, `open` | `path` | `{ok}`; Finder, or the default app |
| `requests` (`call`) | none | `{seq, kind, path}`: the latest thing a panel or the HUD asked the app to do (`annotate`, `stopRecording`). The module calls `changed("glint.requests")` after each |
