//! The main thread's stack. Windows gives an executable's main thread 1 MiB unless the linker is told otherwise, and
//! gpui lays out and paints a window's whole element tree on that thread: an unoptimized build ran out of it drawing
//! the Voice & Video settings page (`0xc000041d`, no panic). 8 MiB, Linux's default, leaves room.

fn main() {
    println!("cargo:rerun-if-changed=assets/branding/gravecord/app/gravecord.ico");
    println!("cargo:rerun-if-changed=app.rc");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::copy(
        "assets/branding/gravecord/app/gravecord.ico",
        out.join("gravecord.ico"),
    )
    .unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("app.rc", embed_resource::ParamsIncludeDirs([out.as_path()]))
            .manifest_optional()
            .unwrap();
    }
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    } else if target.contains("windows-gnu") {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608");
    }
}
