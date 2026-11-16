# Painted-host repros

Base: upstream `main` at `f31d5ab8e`. Apply and run from a checkout of it:

```sh
git apply painted-host-repros.patch
```

In a fresh worktree the `exact-js` build needs rolldown: `bun install`, or set
`EXACT_ROLLDOWN=<checkout>/node_modules/.bin/rolldown`.

All tests run headless on macOS (CPU painter).

## A canvas whose arguments alone change is not painted again (reproduced)

A commit that changes only a `canvas surface=`'s arguments moves no node, so the presenter is not dirty and no frame
is painted until something else changes.

```sh
cargo test -p exact-painted --lib a_canvas_whose_arguments_alone_change_is_painted_again
```

- Test: `host/painted/src/surfaces/capture_tests.rs`. A button press changes `ink(shade)` from 40 to 41; the canvas
  draws red at 40 and blue otherwise.
- Expected: after the press `p.dirty()` is true and the next frame shows blue at (20, 20).
- Actual:

```
panicked at host/painted/src/surfaces/capture_tests.rs:417:5:
the canvas owes a drawing
```

- Fork fix: with `15897f15b`'s three source files applied (`host.rs`, `paint/damage.rs`, `presenter.rs`), the test
  passes: `test result: ok. 1 passed`. The fix sets `dirty` when `canvas_owed` after a commit, and marks a redrawn
  canvas as flow damage so a partial paint includes it.

## `native.available` is true for an app with no native module (reproduced)

When the painted host composes any package, `serve_packages` calls `handle_modules`, which installs a hosted handler
on the native slot (`presenter/requests.rs`). exact-js sets `hosted` from that slot (`js/src/lib.rs`), and `door.rs`
answers `available` from `native.is_some() || hosted`. Its own comment says `available` is false "when the app links
none". So an app that gates on `native?.available` sends calls that fail:

- `native.call`: "the app's module answers no native.call" (painted installs no `host_call`)
- `native.later`: `NotSupportedError` (`modules_in` serves no extension but the packages')

Two tests, one for each side:

```sh
cargo test -p exact-painted --lib a_package_handler_does_not_stand_for_the_apps_own_module
cargo test -p exact-js --test it a_hosts_package_handler_is_not_the_apps_module
```

**Painted** (`host/painted/src/desktop_tests.rs`): an app with no module, plus one package (`global-shortcuts`).

- Expected: the slot has no hosted app module.
- Actual:

```
panicked at host/painted/src/desktop_tests.rs:371:5:
the slot holds a hosted app module, so native.available is true, and the app's own call answers {"error":"NotSupportedError"}
```

**exact-js** (`js/tests/it/native_later.rs`): the `storage` fixture, with no `NativeModule`. Its slot gets a hosted
handler that refuses with `NotSupportedError`, as painted's does. `work("native")` and `work("later")` return
`"no native module"` when `!native?.available`.

- Expected: `"no native module"`.
- Actual:

```
panicked at js/tests/it/native_later.rs:234:5:
assertion `left == right` failed
  left: "the app's module answers no native.call"
 right: "no native module"
```

## Dark scheme: the window ground and popovers stay light (reproduced, both parts)

**Ground.** `color-scheme="dark"` on the root, with the system set to light, leaves the frame's ground white. Painting
`-exact-background` on the root makes it black.

```sh
cargo test -p exact-painted --lib a_dark_root_scheme_darkens_the_windows_ground
```

- Test: `host/painted/src/presenter/status_bar_tests.rs`. It samples (300, 200) on a 400x300 `main` that holds one
  `text`.
- Expected: dark in both cases.
- Actual:

```
panicked at host/painted/src/presenter/status_bar_tests.rs:717:5:
the ground under a dark root is [255, 255, 255] (painting -exact-background: [0, 0, 0])
```

**Popover.** A `popover="auto"` under a dark root that paints `-exact-background` draws light. It draws dark only
when it sets `color-scheme="dark"` itself. This test runs in the `neutral_alert` binary because the desktop (neutral)
look is process-wide, and under that look popovers paint into the page's frame. In the lib tests, without that look
the tap is refused, and `serve_overlays` moves the popover off the frame.

```sh
cargo test -p exact-painted --test neutral_alert a_popover_under_a_dark_root_draws_dark
```

- Test: `host/painted/tests/neutral_alert/popover.rs`. It samples inside the open popover, once with the popover's
  own `color-scheme="dark"` (the control) and once without.
- Expected: dark both times.
- Actual:

```
panicked at host/painted/tests/neutral_alert/popover.rs:128:5:
inheriting the root's dark scheme: [255, 255, 255] (saying it: [30, 30, 30])
```

## A GIF, JPEG or WebP in `image` draws nothing on Linux (reproduced on Fedora 44)

GIF and WebP headers are read as `Header::jpeg` (`image/jpeg.rs` `inspect_animated`), and `decode_rows` sends every
such header to `jpeg_decode::decode`. On Linux that function is a stub that always returns `Refusal::DecodeFailed`:

```rust
/// No platform decoder on this target.
#[cfg(not(any(target_os = "android", windows, target_os = "macos")))]
pub(super) fn decode<R: Read + Seek>(...) -> Result<Pixmap, Refusal> {
    Err(Refusal::DecodeFailed)
}
```

So on Linux this affects JPEGs as well as GIFs and WebPs. macOS uses ImageIO, Windows uses WIC, and Android uses
`AImageDecoder`. The test below needs no cfg gate. It passes on macOS and fails on Linux:

```sh
cargo test -p exact-painted --lib a_gif_decodes_on_every_painted_host
```

- Test: `host/painted/src/image/decode_tests.rs`. It decodes `apps/motion-gallery/assets/spin.gif` and
  `apps/shared-elements/assets/alpine-water.jpg`.
- macOS: `ok`. Linux (Fedora 44, x86_64):

```
panicked at host/painted/src/image/decode_tests.rs:717:9:
apps/motion-gallery/assets/spin.gif: Err(DecodeFailed)
```
