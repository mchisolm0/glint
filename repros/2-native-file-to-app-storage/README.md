# Repro 2: a file written to a native path can't get into `app:/` storage

Tested on upstream `expo/exact` main `f31d5ab8e` (macOS host, Apple Silicon). All three claims reproduce.

Two apps made with `exact new`:
- `copy-into-app/` (TypeScript): writes `/private/tmp/exact-repro2/shot.png`, then tries `copyFile` and `rename` into `app:/data`. Grants: `fs.read`/`fs.write` on that folder and on `app:/data`.
- `rust-source/` (`exact new --rust`): a Rust source sends `fs.copyFile` (native to `app:/data`), and `fs.writeFile` of 1 MB and 5 MB as `"bytes"`.

## Steps

```sh
export EXACT=/path/to/exact BUN=/path/to/bun-1.4.2
cd copy-into-app && bun exact.mjs test macos
bun exact.mjs agent macos --storage s1 --reset-storage "clock data" "state probe"
cd ../rust-source && bun exact.mjs test macos
bun exact.mjs agent macos --storage s1 --reset-storage "clock data" "state copied" "state small" "state big"
```

Both run headless (no window interaction).

## Expected vs actual

| Step | Expected | Actual |
| --- | --- | --- |
| TS `copyFile(native, "app:/data/shot.png")`, both granted | ok | `failed`: `filesystem: cannot mix app and native paths` |
| TS `rename(native, "app:/data/shot.png")` | ok | same error |
| Rust `fs.copyFile` native to `app:/data` | ok | `portable storage needs an app:/ path` |
| Rust `fs.writeFile` 5 MB (`bytes` as a JSON array) | ok | `storage request exceeds its byte limit` |

Controls that pass: TS native `writeFile`/`readFile`, native to native `copyFile`, `readFile` then `writeFile` into `app:/data` (the workaround); Rust 1 MB `writeFile`.

## Observed output

```
copy-into-app$ bun exact.mjs agent macos --storage s1 --reset-storage "clock data" "state probe"
lines ["writeFile native: ok","readFile native: ok","writeFile app:/data: ok",
  "copyFile native -> app:/data: FAILED code=failed message=filesystem: cannot mix app and native paths",
  "rename native -> app:/data: FAILED code=failed message=filesystem: cannot mix app and native paths",
  "copyFile native -> native: ok","readFile + writeFile (workaround): ok"]

rust-source$ bun exact.mjs agent macos --storage s1 --reset-storage "clock data" "state copied" "state small" "state big"
text "FAILED portable storage needs an app:/ path"
text "ok"
text ""        # resource big failed: storage request exceeds its byte limit
```

## Source pointers (f31d5ab8e)

- Claim 1: `vendor/ibex/crates/ibex2/src/stdlib/fs.rs:1037` sends any request with an `app:` side to `AppDirectories::run`, whose `parse` refuses the other, native side (`app_fs_unix.rs:372-374`, `app_fs_windows.rs:154`). The only cross-namespace copy allowed is `doc:` to `app:` (`fs.rs:1212-1215`).
- Claim 2: `data/host/src/native.rs:124-125` and `:612-613` refuse a non-`app:/` path unless `native_filesystem`, which is `cfg!(windows) && …` (`native.rs:531-537`). `:161-162` also refuses an `app:/` source with a native destination.
- Claim 3: bytes are decoded from a JSON array of numbers (`native.rs:29-44`); the request is capped at `MAX_HOST_WORK_BYTES = 16 << 20` (`runner/src/request.rs:15`, checked in `data/host/src/lib.rs:145-146`). A PNG's bytes average about 3.6 JSON characters each, so roughly 4.5 MB of PNG fits in one request.
