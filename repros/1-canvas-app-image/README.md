# Canvas 2D `drawImage` can't draw an `app:/` file

`ctx.drawImage("app:/data/red.png", …)` draws nothing and Exact logs nothing, while an `image` node shows the same file.

Tested on upstream `expo/exact` main `f31d5ab8e` (macOS 27, Bun 1.4.2). Reproduces on the **web** and **macOS** hosts.

## The app (`canvas-app-image/`)

Tapping **Write** runs `captureCanvas('red', 'app:/data/red.png')` on a red canvas, then mounts, left to right:
the red source canvas, a canvas (blue background) whose `draw` calls `drawImage` on that path, and an `image` node
with the same path. `check.mjs` reads the colour at the centre of each square in a screenshot.

## Run

```sh
cd canvas-app-image
export EXACT=/path/to/exact            # an Exact checkout
bun exact.mjs agent web   --storage s1 "tap write" "clock settle" logs "screenshot .exact/web.png"
bun exact.mjs agent macos --storage s1 "tap write" "clock settle" logs "screenshot .exact/macos.png"
bun check.mjs .exact/web.png
```

## Expected vs actual

| square | expected | actual (web and macOS) |
| --- | --- | --- |
| red canvas | `#ff0000` | `#ff0000` |
| `drawImage` canvas | `#ff0000` | `#0000ff` (its background: nothing drawn) |
| `image` node | `#ff0000` | `#ff0000` |

Logs (macOS; web is the same plus Chrome's own `net::ERR_UNKNOWN_URL_SCHEME`):

```
t=0 console: captured app:/data/red.png 200x200 image/png
t=0 console: show app:/data/red.png causes=mount
t=0 console: show app:/data/red.png causes=image
```

The second draw (`causes=image`) shows the host finished the load as failed; nothing says why.
Screenshots: `shots/web-upstream.png`, `shots/macos-upstream.png`.

## Fix on the fork

`mchisolm0/exact` branch `glint` (HEAD `15897f15b`, contains `1a7cc5590`) fixes macOS: the `drawImage` square is
`#ff0000` (`shots/macos-fork-1a7cc5590.png`). It does not touch the web host, where the square stays `#0000ff`:
`host/web/canvas2d-glue.js` loads image handles through `exact.assetURL`, not the picker glue's `appURL` that `image`
nodes use. The painted host (`host.rs`) was not run.

## Also

- **Records arrive positional.** The `record(w)` surface takes a `Written` (`{ path }`); `draw` receives
  `["app:/data/red.png"]` on both hosts (`record arg ["app:/data/red.png"]` in the logs).
- **State-bound `bitmap-width` is refused for the JS target.** `cd bitmap-check && bun exact.mjs agent web tree`
  fails with `app.contract: a dynamic canvas bitmap size or setting is not in the JS target`. With `bitmap-width=10`
  it builds and runs.
