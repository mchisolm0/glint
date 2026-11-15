//! ScreenCaptureKit's Swift bridge needs the Swift runtime. Below macOS 12 the SDK names it
//! `@rpath/libswift_Concurrency.dylib`, and a bare `cargo test` targets macOS 11, so this crate's own tests
//! and examples get `/usr/lib/swift` on their rpath. Exact's builds set MACOSX_DEPLOYMENT_TARGET=14.0 and
//! link the absolute path, so the app needs nothing.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // A link arg reaches only this package's own binaries (tests, examples), never a crate using it.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
