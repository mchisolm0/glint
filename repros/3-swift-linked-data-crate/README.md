# A data crate that links Swift breaks plain `cargo` in the app's workspace on macOS

A web shell's build script links the app's data crate to bake it. Under plain `cargo` it links at Rust's default
macOS deployment target (11.0), so Swift Concurrency comes from `@rpath/libswift_Concurrency.dylib` and dyld
can't find it. Exact's own verbs set 14.0 (`HOST_MACOS`, `withHostDeployment`) and pass.

Tested on upstream `f31d5ab8e20537be3e43f8a88f93416c9e169aa4`, macOS 27.0.1, Xcode 27.2, rustc 1.99.1 (the pinned toolchain).

## The app

`swift-data/` is `exact new swift-data --rust` plus:

- `bridge/`: one Swift function that starts a `Task` (so it needs libswift_Concurrency), compiled by `build.rs` with
  `swiftc -static` and linked as screencapturekit links its bridge.
- `data/Cargo.toml`: `bridge` as a non-wasm dependency, as Glint's data crate takes its capture crate.
- `data/src/lib.rs`: the `greeting` source calls `bridge::answer()`.

## Steps

```sh
cd swift-data
export EXACT=/path/to/exact PATH=<bun 1.4.2>/bin:$PATH
bun exact.mjs web-build          # writes .exact/shells/ and .cargo/config.toml; passes
cd data
cargo check --workspace          # what rust-analyzer runs; fails
```

`cargo check -p swift-data-web` fails the same way. `cargo test` in `data/` alone passes, since only the web shell's
build script links the data crate for the host.

## Expected

`cargo check --workspace` passes in `data/`: docs/reference.md ("Rust data sources") says `cargo test`,
`cargo clippy` and rust-analyzer work there.

## Actual

```
error: failed to run custom build command for `swift-data-web v0.1.0 (.../swift-data/.exact/shells/web)`

Caused by:
  process didn't exit successfully: `.../target/debug/build/swift-data-web-99ab7c29f820d7f7/build-script-build` (signal: 6, SIGABRT: process abort signal)
  --- stderr
  dyld[29329]: Library not loaded: @rpath/libswift_Concurrency.dylib
    Referenced from: <D36EB21C-...> .../target/debug/build/swift-data-web-99ab7c29f820d7f7/build-script-build
    Reason: tried: '.../target/debug/deps/libswift_Concurrency.dylib' (no such file), '~/.rustup/toolchains/1.99.1-aarch64-apple-darwin/lib/...' (no such file), ... '/usr/lib/libswift_Concurrency.dylib' (no such file, not in dyld cache)
```

The two build scripts, by `otool`:

| Built by | `minos` | Concurrency from |
|---|---|---|
| `cargo check` (`target/debug/build/swift-data-web-*`) | 11.0 | `@rpath/libswift_Concurrency.dylib` |
| `bun exact.mjs web --wasm` (`target/host-dev/build/swift-data-web-*`) | 14.0 | `/usr/lib/swift/libswift_Concurrency.dylib` |

`web-build`, `web` and `web --wasm` all pass. `web-build` and the JS dev loop never link the data crate for the host.

## Workaround

With `MACOSX_DEPLOYMENT_TARGET = "14.0"` in the app's `.cargo/config.toml` `[env]` (or set in the shell),
`cargo check --workspace` passes, and the build script links `/usr/lib/swift/libswift_Concurrency.dylib` at 14.0.

The app's copy doesn't last: the next verb (`web-build`, `update`) rewrites `.cargo/config.toml` from the checkout's
own (`scripts/app-shells.mjs`) and drops the line. It sticks only in the Exact checkout's `.cargo/config.toml`,
which is where Glint's fork put it (`afcc738ee`). A fix upstream could add it there.
