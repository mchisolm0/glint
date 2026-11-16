# Draft issues for expo/exact

Found while building [Glint](https://github.com/mchisolm0/glint), a screenshot and recording app, on macOS 27 and
Fedora 44 / GNOME 50. Every repro runs against upstream `main` at `f31d5ab8e`; they live in
[`repros/`](https://github.com/mchisolm0/glint/tree/main/repros). Fixes we used are on
[`mchisolm0/exact`, branch `glint`](https://github.com/mchisolm0/exact/tree/glint), as references, not merge requests.
Not filed yet.

---

## 1. Canvas `drawImage` draws nothing for an `app:/` path

`ctx.drawImage("app:/data/red.png", …)` draws nothing and logs nothing, while an `image` node shows the same file.
Canvas image handles load assets and http(s) URLs only.

- **Repro:** [`repros/1-canvas-app-image`](https://github.com/mchisolm0/glint/tree/main/repros/1-canvas-app-image).
  Write a red PNG with `captureCanvas`, then draw it: the canvas keeps its blue background (`#0000ff`), the `image`
  node is red. Web and macOS.
- **Fix we used:** `1a7cc5590` opens `app:/` paths in Apple's and the painted host's canvas image loaders. The web
  host still fails: `host/web/canvas2d-glue.js` loads handles through `exact.assetURL`, not `appURL`.
- **Also seen in the same repro:** a record argument reaches `draw` as a positional array (`["app:/data/red.png"]`),
  not `{ path }`; a state-bound `bitmap-width` fails the JS build ("a dynamic canvas bitmap size or setting is not in
  the JS target").

---

## 2. No way to move a native file into `app:/` storage

A Rust source that writes files (screen capture, recordings) can't get them into `app:/data`:

| Request | Result |
| --- | --- |
| TS `copyFile` or `rename`, granted native path to `app:/data` | `filesystem: cannot mix app and native paths` |
| Rust `fs.copyFile`, native path to `app:/data` | `portable storage needs an app:/ path` (native paths are Windows only, `data/host/src/native.rs:531`) |
| Rust `fs.writeFile` of 5 MB | `storage request exceeds its byte limit`: bytes travel as a JSON array under a 16 MiB cap (`runner/src/request.rs:15`), about 4.5 MB of PNG |

- **Repro:** [`repros/2-native-file-to-app-storage`](https://github.com/mchisolm0/glint/tree/main/repros/2-native-file-to-app-storage), headless with `exact agent macos`.
- **Workaround:** `readFile` then `writeFile` through the JS heap.
- **Ask:** let `copyFile`/`rename` move a granted native path into `app:/`, or give Rust sources a move request.

---

## 3. `cargo check` in `data/` fails on macOS when the data crate links Swift

If a data crate links a Swift library that uses Concurrency (as the `screencapturekit` crate does), `cargo check
--workspace` in `data/`, which rust-analyzer runs, fails. The web shell's build script links the data crate at
Rust's default deployment target (11.0), so dyld looks for `@rpath/libswift_Concurrency.dylib` and finds none.
Exact's own verbs set 14.0 and pass. `docs/reference.md` says `cargo` and rust-analyzer work in `data/`.

- **Repro:** [`repros/3-swift-linked-data-crate`](https://github.com/mchisolm0/glint/tree/main/repros/3-swift-linked-data-crate):
  `bun exact.mjs web-build`, then `cd data && cargo check --workspace`.
- **Workaround:** `MACOSX_DEPLOYMENT_TARGET = "14.0"` in `.cargo/config.toml` `[env]`. The next `web-build` or
  `update` rewrites the app's copy, so it only sticks in the Exact checkout's (`afcc738ee`).

---

## 4. Painted host: a canvas whose arguments alone change isn't repainted

A commit that changes only a `canvas surface=`'s arguments moves no node, so no frame is painted until something
else changes. Glint's arrow tool showed nothing while dragging, only on release. The Apple host already schedules a
frame on `canvas_owed`.

- **Repro:** `cargo test -p exact-painted --lib a_canvas_whose_arguments_alone_change_is_painted_again`, from
  [`repros/6-painted-host`](https://github.com/mchisolm0/glint/tree/main/repros/6-painted-host)'s patch. Fails with
  "the canvas owes a drawing".
- **Fix we used:** `15897f15b` (the test passes with it): a commit sets `dirty` when a canvas is owed, and a redrawn
  canvas counts as damage so a partial paint includes it.

---

## 5. Painted host: `native.available` is true for an app with no native module

When the painted host serves any package, `handle_modules` installs a hosted handler on the native slot, and
`door.rs` answers `available` from it. An app with no module of its own that checks `available` then sends calls
that fail: `call` answers "the app's module answers no native.call", `later` answers `NotSupportedError`. The docs
say `available` is false when the app links no module.

- **Repro:** two tests in [`repros/6-painted-host`](https://github.com/mchisolm0/glint/tree/main/repros/6-painted-host)'s
  patch: `cargo test -p exact-painted --lib a_package_handler_does_not_stand_for_the_apps_own_module` and
  `cargo test -p exact-js --test it a_hosts_package_handler_is_not_the_apps_module`.

---

## 6. Painted host: a dark `color-scheme` leaves the window ground and popovers light

With the system light, `color-scheme="dark"` on the root turns text light but the window's ground stays white
(`[255, 255, 255]`) until the root paints `-exact-background`. A `popover` under that root also draws white unless
it sets `color-scheme="dark"` itself.

- **Repro:** from [`repros/6-painted-host`](https://github.com/mchisolm0/glint/tree/main/repros/6-painted-host)'s
  patch: `cargo test -p exact-painted --lib a_dark_root_scheme_darkens_the_windows_ground` and
  `cargo test -p exact-painted --test neutral_alert a_popover_under_a_dark_root_draws_dark`.

---

## 7. Linux: `image` draws nothing for a GIF, JPEG or WebP

On Linux the painted host has no decoder for these: GIF and WebP headers go to the JPEG path, and on Linux
`jpeg_decode::decode` (`host/painted/src/image/jpeg.rs:459`) always returns `DecodeFailed`. macOS uses ImageIO,
Windows WIC, Android `AImageDecoder`. Only PNG draws, so a recording app on Linux can't preview a GIF.

- **Repro:** `cargo test -p exact-painted --lib a_gif_decodes_on_every_painted_host`, from
  [`repros/6-painted-host`](https://github.com/mchisolm0/glint/tree/main/repros/6-painted-host)'s patch. Passes on
  macOS; on Fedora 44 it fails with `apps/motion-gallery/assets/spin.gif: Err(DecodeFailed)`.

---

## 8. Linux as a desktop target

The Linux host draws to DRM/KMS, VNC or nothing, so a desktop app can't open a window on GNOME or KDE, and
`@exact/global-shortcuts` and `@exact/clipboard` don't support Linux. Glint runs on GNOME 50 with these on the fork:

- **Window:** `1bb260aa9`, winit + softbuffer (as on Windows) as one more output for the event loop; Wayland and X11,
  `dlopen`ed so the binary still links only libc. `2257f942b` makes the Wayland app id the app's id, so GNOME finds
  the `.desktop` file.
- **Global shortcuts and clipboard:** `7a57be573`, the GlobalShortcuts portal over the host's D-Bus code, and
  `wl-copy`/`xclip`. GNOME refuses shortcuts without a `.desktop` file for the app id, and its dialog lists each
  shortcut by id (`area`, `record`); a description would read better.
- **`keepRunning`:** `bbf91a41a`. Closing destroys the window (winit can't hide one on Wayland), and a second launch
  hands its activation token to the running one over a socket.

Still missing: the window title (it's the binary's name, not `app.name`), `Icon=` in the desktop entry, following
the system's dark mode, copying a file (`@exact/clipboard` copies PNGs and text only), and `@exact/text-detection`
on Linux. Its call answers `NotAllowedError`, which reads like a missing grant rather than a missing platform.

---

## 9. Field report: a screenshot app on Exact

What Glint needed beyond the box. Related: #503.

- **Worked as is:** `@exact/global-shortcuts`, the status item, `@exact/clipboard`, `@exact/text-detection`,
  `@exact/login-item`, Canvas 2D with `captureCanvas`.
- **Screen capture:** no package, so a Rust crate does it (ScreenCaptureKit on the Mac, the desktop portals and
  GStreamer on Linux). A package that captures a display, area or window to an `app:/` PNG, and records to an
  `app:/` MP4, would cover most of it.
- **Windows Contract can't make:** a dimmed picker over every display, a floating thumbnail that doesn't take focus,
  pinned images, a recording frame. All Swift today.
- **Smaller things:** shapes aren't exported by name from `app.contract.d.ts`; `exactViewport().devicePixelRatio`
  must be declared `option<number>`; `test macos` uses a 420 × 900 viewport, not app.json's window size, and a test
  file can't set its own.
