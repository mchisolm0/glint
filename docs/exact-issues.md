# Draft issues for expo/exact

From building Glint (a CleanShot-style screenshot and recording app) on Exact `a62c78c2e` (2026-11-14), on macOS 27
and on Fedora 44 / GNOME 50 (Wayland). Not filed yet. Each issue links code only as a reference: the fork branch is
what made Glint work, not a request to merge it.

Fork: `mchisolm0/exact`, branch `glint` (link once pushed). Glint: `mchisolm0/glint` (link once pushed).

---

## 1. Canvas 2D `drawImage` can't draw an `app:/` file

### What happened
Glint's editor draws a screenshot the app saved in `app:/data` onto a canvas, adds marks, and `captureCanvas`es the
result. `drawImage("app:/data/captures/x.png", …)` draws nothing and logs nothing: an image handle is an asset or an
http(s) URL only (`Canvas2DImage.load` on Apple, `host.rs`'s `take_canvas_image_requests` on the painted host). An
`image` node shows the same path fine, and `captureCanvas` writes to `app:/`, so the annotate-a-file loop is one
read short.

### Fix we used
One line per host: the canvas's byte reader opens `app:/` paths the way an `image` node does (`AppFiles.open` on Apple,
`picker::open` on the painted host). Branch `glint`, commit `1a7cc5590`.

### Also
- A record in a surface's arguments arrives in `draw` as a positional array (`["arrow", "#ff3b30", 6, …]`), not the
  object the shape's TypeScript type suggests. Saying so in the canvas docs, or passing objects, would save a
  debugging session.
- `bitmap-width` bound to state is refused for the JS target ("a dynamic canvas bitmap size … is not in the JS
  target"). Glint exports through a second canvas sized `pixels / devicePixelRatio`, mounted only while saving.

---

## 2. A Rust source can't get a file it wrote into app storage

### What happened
Glint's Rust sources capture with ScreenCaptureKit and the desktop portal, which write files. Getting them into
`app:/data`:
- Storage requests from Rust take `app:/` paths only (`portable storage needs an app:/ path`); native paths work on
  Windows only.
- Binary writes are a JSON array of numbers, under 16 MiB per request. A Retina PNG is ~8 MB of JSON; a recording is
  far over.
- From TypeScript, `copyFile(native, "app:/…")` answers `cannot mix app and native paths`, even with both granted.

### Workaround
Rust stages the file in `/tmp/glint`; on the Mac a Swift module renames it into `context.data` and calls `wrote`;
elsewhere `app.ts` reads and writes it through the JS heap.

### Ask
Let `copyFile`/`rename` move a granted native path into `app:/`, or give Rust sources a raw-bytes or move request.

---

## 3. A data crate that links Swift breaks the web shell's build on macOS

### What happened
`glint-capture` uses the `screencapturekit` crate, which links a Swift bridge. The web shell's build script links
`glint-data` for the host to bake, at Rust's default macOS deployment target (11.0). There Swift Concurrency comes
through `@rpath`, which a build script has none of: `dyld: Library not loaded: @rpath/libswift_Concurrency.dylib`.
`exact mac` sets 14.0 and is fine. `update --without web` is refused, since the web host is the dev loop's.

### Fix we used
`MACOSX_DEPLOYMENT_TARGET = "14.0"` in `.cargo/config.toml`'s `[env]` (commit `afcc738ee`).

---

## 4. Linux as a desktop: a window, global shortcuts, the clipboard

### What happened
The Linux host draws to DRM/KMS, VNC or nothing, so a desktop utility can't run on GNOME or KDE (`QUEUE.md`, "Linux
host": Wayland/X11 windows). `@exact/global-shortcuts` and `@exact/clipboard` list Linux as unsupported.

### What we tried, as a reference
- **A window** (commit `1bb260aa9`, ~720 lines in `host/linux/src/display/window.rs`): winit 0.30 + softbuffer, the
  pair the Windows host uses, as one more `Output` for `event_loop::run`. Wayland and X11, client-side decorations on
  GNOME, xkbcommon keymaps (a German layout and a dead key tested), wheel, fractional scale, resize, close. Frames
  wait for the compositor's frame callback as a page flip paces KMS. Libraries are `dlopen`ed, so the binary still
  links only libc. Tested in a private headless `gnome-shell` with frames read through mutter's ScreenCast.
- **Global shortcuts** (commit `7a57be573`): the GlobalShortcuts portal over the host's existing D-Bus code. GNOME
  needs a `.desktop` file for the app id, or it refuses ("An app id is required"); the Linux build could install one.
- **Clipboard:** `wl-copy`, `xclip` under X11 only.

### Still missing for desktop apps
app.json's window size on Linux, tray/status items, input methods checked with IBus, drag and drop, notifications,
multiple windows, layer-shell overlays, theme following the system, accessibility, a `.desktop` file and icon, GPU
presentation.

### Found on the way
- `ime_target()` becomes `None` when the mouse is released over a focused field, so an input method stays off there
  (the shared presenter; affects Windows too).
- A package can't depend on crates of its own, so the portal service lives in `exact_painted`.

---

## 5. Field report: what a screenshot tool needed beyond the box

Glint: Hyper+3 screen, +4 area or window, +5 record, +6 scrolling capture, +2 text (OCR); a quick-access thumbnail, pins, an annotation
editor (arrow, box, ellipse, line, pen, highlighter, text, pixelate, steps, spotlight, crop, background), MP4 and
GIF recording. One Contract file, ~300 lines of `app.ts`, a Rust capture crate, a Swift module for the Mac's windows.

- **Worked out of the box:** `@exact/global-shortcuts`, the status item, `@exact/clipboard`, `@exact/text-detection`,
  `@exact/login-item`, Canvas 2D and `captureCanvas`, `task … key=` for reacting to a native module's `changed`.
- **Screen capture:** no package. A Rust crate behind Rust sources: ScreenCaptureKit on the Mac (`screencapturekit`
  crate; macOS 26's screenshot API for window shadows) and the Screenshot/ScreenCast portals on Linux, with
  GStreamer for encoding. A `@exact/screen-capture` package (display, area, window to an `app:/` PNG; a recording to
  an `app:/` MP4) would cover most of it.
- **Windows Contract can't make:** a dimmed picker over every display, a non-activating floating thumbnail, pins, a
  recording frame. All Swift. A host-drawn overlay or panel primitive would let the next capture app skip it.
- **Types:** shapes aren't exported by name from `app.contract.d.ts`; Glint derives `Mark` from `Args<'saveEdit'>`.
- **`exactViewport().devicePixelRatio`** must be declared `option<number>`, or the bake refuses the shape.
- **`test macos`** drives a 420 × 900 viewport, not app.json's window size, so a desktop layout's drags leave the
  viewport until `--size 1100x760` is passed. A test file can't say its own size.
- **`exact lock`** hit the same bitflags conflict twice after dependency changes; deleting `Cargo.lock` and locking
  again fixed it both times.

Related: #503 (a screenshot annotation tool on Windows).

---

## 6. Running Glint end to end on GNOME 50 (Wayland)

From a private headless GNOME 50 session on Fedora 44 (`glint` branch `0bcaaed72` plus the fix below). Screen, area
and window capture, the clipboard, the editor and save, MP4 and GIF recording with the ScreenCast restore token, and
the global shortcuts all work. What got in the way:

- **`native.available` is true on Linux with no app module.** The painted host installs a hosted handler for its
  packages (`presenter/requests.rs` `handle_modules`), and `door.rs` answers `available` from `state.hosted`, so app
  code that gates its own module on `available` sends it calls that fail (`later`: `NotSupportedError`; `call`: "the
  app's module answers no native.call"). Glint's file moves silently went nowhere. Glint now asks the module once with
  a `call`. `available` meaning "a module answers the app's calls" would match the docs.
- **The window's app id was the binary's name** (`glint`), so GNOME matched no desktop entry (generic icon, no app
  name). Fixed on the branch: the Wayland app id and X11 class are the app's id (`host/linux/src/display/window.rs`).
  The title is still the binary's stem (`glint`), not `app.name`, and the desktop entry has no `Icon=`.
- **Dark mode:** `userInterfaceStyle` is Apple and web only (documented). `color-scheme="dark"` on the root turns the
  text light but leaves the window's ground white until the root paints `-exact-background`, and a `popover` under
  that root still draws light until it says `color-scheme="dark"` itself.
- **Media:** an `image` of a GIF draws nothing and `video` is a blank box (no decoder on the painted host). A recording
  app on Linux shows no preview.
- **Packages on Linux:** `@exact/clipboard` copies PNGs and text only (no file, so no way to copy an MP4);
  `@exact/text-detection` has no Linux implementation (Glint's call answered `NotAllowedError`, which reads like a
  grant problem rather than a missing platform).
- **GlobalShortcuts:** GNOME's "Add Keyboard Shortcuts" dialog lists each shortcut by its id (`area`, `record`); a
  description field would read better there.
- **SF symbols:** `symbol:sf/*` is blank on Linux and the build says so clearly. Declaring `icon.*` tokens with
  Material Symbols SVGs in `app.forma` fixed every one, including a computed `` `symbol:icon.${t}` ``.
