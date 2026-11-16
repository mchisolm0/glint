# Draft issues for expo/exact

Found while building [Glint](https://github.com/mchisolm0/glint), a screenshot and recording app, on macOS 27 and
Fedora 44 / GNOME 50. Every repro runs against upstream `main` at `f31d5ab8e`; they live in
[`repros/`](https://github.com/mchisolm0/glint/tree/main/repros). Fixes we used are on
[`mchisolm0/exact`, branch `glint`](https://github.com/mchisolm0/exact/tree/glint). Exact takes no pull requests, and
CONTRIBUTING asks for a fix as a diff or in words in the issue, so each links its commit. Not filed yet.

Seven issues, one question (8), and one comment on an existing issue (9).

---

## 1. Canvas `drawImage` draws nothing for an `app:/` path (tested diff)

**Host:** Web, macOS

`ctx.drawImage("app:/data/red.png", …)` draws nothing and logs nothing, while an `image` node shows the same file.
Canvas image handles load assets and http(s) URLs only.

- **Repro:** [`repros/1-canvas-app-image`](https://github.com/mchisolm0/glint/tree/main/repros/1-canvas-app-image).
  Write a red PNG with `captureCanvas`, then draw it: the canvas keeps its blue background (`#0000ff`), the `image`
  node is red.
- **Fix:** [`1a7cc5590`](https://github.com/mchisolm0/exact/commit/1a7cc5590) opens `app:/` paths in Apple's and the
  painted host's canvas image loaders. The web host still fails: `host/web/canvas2d-glue.js` loads handles through
  `exact.assetURL`, not `appURL`.
- **Also seen in the same repro:** a record argument reaches `draw` as a positional array (`["app:/data/red.png"]`),
  not `{ path }`; a state-bound `bitmap-width` fails the JS build ("a dynamic canvas bitmap size or setting is not in
  the JS target").

---

## 2. A Rust source can't put a file over about 4.5 MB into `app:/` storage

**Host:** macOS (the storage path is shared)

A Rust source that writes files (screen capture, recordings) can't get them into `app:/data`. In #230 the supported
route for Rust app files was `Answer::Later(exact_data::storage::request(...))`, but that route carries the bytes as
a JSON array under a 16 MiB cap (`runner/src/request.rs:15`), so a PNG over about 4.5 MB is refused and a recording
is out of reach:

| Request | Result |
| --- | --- |
| Rust `fs.writeFile` of 5 MB | `storage request exceeds its byte limit` |
| Rust `fs.copyFile`, native path to `app:/data` | `portable storage needs an app:/ path` (native paths are Windows only, `data/host/src/native.rs:531`) |
| TS `copyFile` or `rename`, granted native path to `app:/data` | `filesystem: cannot mix app and native paths` |

- **Repro:** [`repros/2-native-file-to-app-storage`](https://github.com/mchisolm0/glint/tree/main/repros/2-native-file-to-app-storage), headless with `exact agent macos`.
- **Workaround:** `readFile` then `writeFile` through the JS heap.
- **Ask:** `copyFile` from a `doc:` file to `app:/` already has the host copy the bytes, past the 16 MiB result
  (`docs/reference.md`, the `doc:` table). The same for a native path the app was granted, or a copy request for Rust
  sources, would cover it.

---

## 3. `cargo check` in `data/` fails on macOS when the data crate links Swift

**Host:** macOS

If a data crate links a Swift library that uses Concurrency (as the `screencapturekit` crate does), `cargo check
--workspace` in `data/`, which rust-analyzer runs, fails. The web shell's build script links the data crate at
Rust's default deployment target (11.0), so dyld looks for `@rpath/libswift_Concurrency.dylib` and finds none.
Exact's own verbs set 14.0 and pass. `docs/reference.md` says `cargo` and rust-analyzer work in `data/`.

- **Repro:** [`repros/3-swift-linked-data-crate`](https://github.com/mchisolm0/glint/tree/main/repros/3-swift-linked-data-crate):
  `bun exact.mjs web-build`, then `cd data && cargo check --workspace`.
- **Workaround:** `MACOSX_DEPLOYMENT_TARGET = "14.0"` in `.cargo/config.toml` `[env]`. The next `web-build` or
  `update` rewrites the app's copy, so it only sticks in the Exact checkout's
  ([`afcc738ee`](https://github.com/mchisolm0/exact/commit/afcc738ee)).

---

## 4. Painted host: a canvas whose arguments alone change isn't repainted (tested diff)

**Host:** Linux, Windows, Android (the painted host)

A commit that changes only a `canvas surface=`'s arguments moves no node, so no frame is painted until something
else changes. Glint's arrow tool showed nothing while dragging, only on release. The Apple host already schedules a
frame on `canvas_owed`.

- **Repro:** `cargo test -p exact-painted --lib a_canvas_whose_arguments_alone_change_is_painted_again`, from
  [`repros/6-painted-host`](https://github.com/mchisolm0/glint/tree/main/repros/6-painted-host)'s patch. Fails with
  "the canvas owes a drawing".
- **Fix:** [`15897f15b`](https://github.com/mchisolm0/exact/commit/15897f15b) (the test passes with it): a commit sets
  `dirty` when a canvas is owed, and a redrawn canvas counts as damage so a partial paint includes it.

---

## 5. Painted host: `native.available` is true for an app with no native module

**Host:** Linux, Windows, Android (the painted host)

When the painted host serves any package, `handle_modules` installs a hosted handler on the native slot, and
`door.rs` answers `available` from it. An app with no module of its own that checks `available` then sends calls
that fail: `call` answers "the app's module answers no native.call", `later` answers `NotSupportedError`. The docs
say `available` is false when the app links no module.

- **Repro:** two tests in [`repros/6-painted-host`](https://github.com/mchisolm0/glint/tree/main/repros/6-painted-host)'s
  patch: `cargo test -p exact-painted --lib a_package_handler_does_not_stand_for_the_apps_own_module` and
  `cargo test -p exact-js --test it a_hosts_package_handler_is_not_the_apps_module`.

---

## 6. Painted host: under a dark `color-scheme` root, popovers and the window ground stay light

**Host:** Linux, Windows, Android (the painted host)

With the system light and `color-scheme="dark"` on the root:

- **A `popover`** under that root draws white unless it sets `color-scheme="dark"` itself.
- **The window ground** stays white (`[255, 255, 255]`) while the text turns light, so the page reads as light text
  on white until the root paints `-exact-background`. #10 calls the ground under a root with no background intended
  ("the page is the platform's background"); the question is whether that background should follow the root's
  scheme.

- **Repro:** from [`repros/6-painted-host`](https://github.com/mchisolm0/glint/tree/main/repros/6-painted-host)'s
  patch: `cargo test -p exact-painted --test neutral_alert a_popover_under_a_dark_root_draws_dark` and
  `cargo test -p exact-painted --lib a_dark_root_scheme_darkens_the_windows_ground`.

---

## 7. Linux: `image` draws nothing for a JPEG or GIF

**Host:** Linux

Every image but PNG goes to `jpeg_decode::decode`, which on Linux is a stub marked "No platform decoder on this
target" (`host/painted/src/image/jpeg.rs:458`) and returns `DecodeFailed`. macOS uses ImageIO, Windows WIC, Android
`AImageDecoder`. So on Linux a photo or a GIF recording shows nothing.

- **Repro:** `cargo test -p exact-painted --lib a_gif_decodes_on_every_painted_host`, from
  [`repros/6-painted-host`](https://github.com/mchisolm0/glint/tree/main/repros/6-painted-host)'s patch. Passes on
  macOS; on Fedora 44 it fails with `apps/motion-gallery/assets/spin.gif: Err(DecodeFailed)`.
- **Ask:** a Rust decoder for JPEG and GIF on Linux. JPEG encoding there already uses `jpeg-encoder` because Linux has
  no system encoder (`rules/DEFERRED.md`, #504). WebP matters less to us.

---

## 8. Question: is a Linux desktop host wanted?

Exact has no Discussions, and CONTRIBUTING lists a missing capability as issue material, so this goes as an issue
asking, not proposing a merge.

> **Host:** Linux
>
> `rules/DEFERRED.md` keeps `@exact/global-shortcuts`, `@exact/login-item` and `@exact/dock` off Linux "until a
> desktop host". Is a desktop host (a normal window on GNOME or KDE) wanted before 1.0, and is there a shape you'd
> want it in?
>
> For reference, Glint runs on GNOME 50 Wayland with these on our fork:
>
> - [`1bb260aa9`](https://github.com/mchisolm0/exact/commit/1bb260aa9): winit + softbuffer as one more output for
>   the event loop, Wayland and X11, `dlopen`ed so the binary still links only libc.
>   [`2257f942b`](https://github.com/mchisolm0/exact/commit/2257f942b) makes the Wayland app id the app's id, so GNOME
>   finds the `.desktop` file.
> - [`7a57be573`](https://github.com/mchisolm0/exact/commit/7a57be573): global shortcuts through the GlobalShortcuts
>   portal, and the clipboard through `wl-copy`/`xclip`.
> - [`bbf91a41a`](https://github.com/mchisolm0/exact/commit/bbf91a41a): `keepRunning`. winit can't hide a window on
>   Wayland, so closing destroys it, and a second launch hands its activation token to the running one.

---

## 9. Comment on #505 (still capture of the desktop)

> A third host's data point, from [Glint](https://github.com/mchisolm0/glint) on Fedora 44 / GNOME 50 (Wayland). A
> Wayland app can't read the screen directly. Glint asks the Screenshot portal for a still, and the ScreenCast portal
> for recording and scrolling capture, feeding `pipewiresrc` into GStreamer. GNOME asks about the still once (a
> permission under Settings, Privacy and Security, Screenshots), which meets "no per-capture prompt"; a recording's
> picker is skipped after the first by the portal's restore token. The code is
> [`capture/src/linux.rs`](https://github.com/mchisolm0/glint/blob/main/capture/src/linux.rs).
>
> On "the result as a file or image handle, not bytes through the data source": our capture writes a PNG natively and
> hits the same wall getting it into `app:/` (#N, issue 2 above).
