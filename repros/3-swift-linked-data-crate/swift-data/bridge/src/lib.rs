#[cfg(target_os = "macos")]
extern "C" {
    fn swift_answer() -> isize;
}

/// 42, from Swift on a Mac.
pub fn answer() -> isize {
    #[cfg(target_os = "macos")]
    return unsafe { swift_answer() };
    #[cfg(not(target_os = "macos"))]
    42
}
