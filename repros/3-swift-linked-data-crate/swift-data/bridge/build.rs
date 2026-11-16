//! Compiles bridge.swift into a static library and links it with the Swift runtime, as screencapturekit does.
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=bridge.swift");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = std::env::var("OUT_DIR").unwrap();
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        other => other,
    }
    .to_string();
    let status = Command::new("xcrun")
        .args(["swiftc", "-parse-as-library", "-emit-library", "-static", "-module-name", "Bridge"])
        .args(["-target", &format!("{arch}-apple-macosx12.3"), "bridge.swift", "-o"])
        .arg(format!("{out}/libBridge.a"))
        .status()
        .unwrap();
    assert!(status.success(), "swiftc failed");
    let sdk = Command::new("xcrun").args(["--sdk", "macosx", "--show-sdk-path"]).output().unwrap();
    let sdk = String::from_utf8(sdk.stdout).unwrap();
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static=Bridge");
    println!("cargo:rustc-link-search=native={}/usr/lib/swift", sdk.trim());
    // The toolchain's back-deployment libraries (swiftCompatibility56 and the rest), as screencapturekit adds.
    let swiftc = Command::new("xcrun").args(["--find", "swiftc"]).output().unwrap();
    let bin = std::path::PathBuf::from(String::from_utf8(swiftc.stdout).unwrap().trim());
    println!("cargo:rustc-link-search=native={}", bin.join("../../lib/swift/macosx").display());
}
