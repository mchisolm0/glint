# Draft issues for expo/exact

Findings from building Glint on Exact `a62c78c2e` (2026-11-14), macOS 27, Xcode 27.2. Not filed yet.

---

## 1. A Rust source has no good path to get a file it wrote into app storage

### What happened

Glint's Rust data source captures the screen with [xcap](https://crates.io/crates/xcap) and gets PNG bytes back. Getting those bytes into `app:/data`, so an `image` can show them, hit two walls on macOS:

- **Storage requests from Rust accept only `app:/` paths.** `fs.copyFile` from `/private/tmp/glint/x.png` answers `portable storage needs an app:/ path` (`data/host/src/native.rs:125`). `native_filesystem` allows native paths on Windows only.
- **Binary writes are a JSON array of numbers.** `fs.writeFile`'s `bytes` is `[137, 80, 78, …]` (`data/host/src/native.rs`, `bytes()`), under the 16 MiB `MAX_HOST_WORK_BYTES`. A 2.2 MB Retina screenshot becomes about 8 MB of JSON. A busy 5K screen or a photo goes over the cap.

The TypeScript side doesn't help either: `storage.fs.copyFile(native, "app:/…")` answers `cannot mix app and native paths` (`ibex2/src/stdlib/app_fs_unix.rs:374`), even with `fs.read /private/tmp/glint` and `fs.write app:/data/captures` both granted. Only a chosen `doc:/` file may be copied into the app (`copies_into_app`).

### Workaround

Rust writes the PNG to a staging folder and answers with its path. `app.ts` then runs `writeFile(appPath, await readFile(stagedPath))` and removes the staged file. That costs an extra source, an extra Contract mutation, and a trip through the JavaScript heap.

### Ask

One of these:

- Let `fs.copyFile` and `fs.rename` move a granted native path into `app:/`, the way `doc:/` already can.
- Take `bytes` as base64, or as a raw buffer beside the JSON, so a Rust source can write a large binary file directly.

Any Rust source that wraps a native library producing files would hit the same wall: renders, exports, recordings.

---

## 2. Field report: a screenshot tool on Exact

### What we built

Glint: ⌃⌥⇧⌘3 captures the screen and ⌃⌥⇧⌘4 drags out an area. A menu bar item does the same. Captures are listed, previewed and copied to the clipboard. One Contract file, a Rust data crate, about 50 lines of `app.ts`, and one Swift module. Runs on macOS; `exact test macos` passes.

### What Exact covered

- `@exact/global-shortcuts` and `-exact-menu="status"`: worked the first time, from the `packages/dock/test/fixture` example.
- Contract composing `app.ts` with a Rust crate (`RUST_SOURCES`, `TypeScriptSources`).
- `@exact/clipboard` writing an `app:/` PNG.
- A native module for what Contract can't express.

### What it needed beyond the box

- **Screen capture.** There is no package for it. A Rust crate (xcap) behind a Rust data source worked unchanged inside the app. Notes:
  - On macOS, xcap uses `CGWindowListCreateImage`, which Apple deprecated in favor of ScreenCaptureKit. It still works on macOS 27.
  - On Linux, xcap needs system libraries (`wayland-client`, `pipewire`, `dbus`) plus `libclang` for bindgen. On a Fedora 44 box, the build stops at `wayland-sys`. Exact's Linux host avoids system libraries entirely.
- **An overlay to pick an area.** A dimmed, borderless window on every display at screen-saver level, with a drag rectangle. Contract has no way to make one, so it is a Swift `ExactModule` (`modules/apple/AreaPicker.swift`). exact-studio does the same for its pickers.
- **Linux as a desktop.** The Linux host paints to DRM/KMS or headless, not to a Wayland or X11 window. `@exact/global-shortcuts` and status items are unsupported there. So a desktop utility can't run on a GNOME or KDE session yet. Windows has the same gap for notification-area icons (README, "Not yet").

Related: #503 (a screenshot annotation tool on Windows that wants PrtSc).

### Ask

Is a `@exact/screen-capture` package in scope (a display, an area, or a window, saved as an `app:/` PNG, with ScreenCaptureKit on Apple)? What about a host-drawn selection overlay, so the next capture or recording app needs no Swift? Glint is here if either is useful as a fixture: <link once pushed>.

---

## 3. (minor) `exact lock` can't settle a lock after an app crate's dependencies change

After `glint-data` dropped two dependencies, `exact mac` stopped with `cannot update the lock file … --locked`. Running `exact lock` then failed:

```
failed to select a version for `bitflags`
  zbus 6.3.0 (via ashpd 0.14, via xcap 0.9.10) needs ^2.13.2
  exact2's lock pins 2.13.1 (via drm 0.15, exact-linux)
```

Deleting the app's `Cargo.lock` and running `exact lock` again resolved it (518 packages). The first `lock` had resolved the same graph without trouble.
