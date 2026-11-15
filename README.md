# Glint

A screenshot tool built on [Exact](https://github.com/expo/exact): global shortcuts, a menu bar item, and capture done by a Rust crate ([xcap](https://crates.io/crates/xcap)) so the capture code is the same on every desktop.

Spike status: works on macOS. Linux is blocked on Exact's Linux host (see [docs/exact-issues.md](docs/exact-issues.md)).

| Shortcut | Does |
|---|---|
| ⌃⌥⌘3 | Capture the screen |
| ⌃⌥⌘4 | Drag out an area (Escape cancels) |

Each capture is saved, listed, selected and copied to the clipboard.

## Where things live

| Piece | File | Platforms |
|---|---|---|
| View, menu bar item, shortcuts | `app.contract` | Contract |
| Capture (pixels to PNG) | `capture/` (`glint-capture`, wraps xcap) | macOS, Linux, Windows |
| Capture sources | `data/src/lib.rs` | native hosts |
| List, keep, copy, delete | `app.ts` | every host |
| Area overlay | `modules/apple/AreaPicker.swift` | macOS |

A capture runs in three steps: Rust takes the picture and stages a PNG in `/tmp/glint`, `app.ts` moves it into `app:/data/captures`, then the view lists it and copies it.

## Run

Needs a checkout of `expo/exact` at `../exact`, Bun 1.4.2, and Xcode's `clang` and `swift` first on `PATH`.

```sh
bun ../exact/scripts/exact.mjs setup   # once
bun exact.mjs mac --run
bun exact.mjs test macos
```

The first capture asks for Screen Recording permission.
