//! The main thread's stack. Windows gives an executable's main thread 1 MiB unless the linker is told otherwise, and
//! gpui lays out and paints a window's whole element tree on that thread: an unoptimized build ran out of it drawing
//! the Voice & Video settings page (`0xc000041d`, no panic). 8 MiB, Linux's default, leaves room.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    } else if target.contains("windows-gnu") {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608");
    }
}
