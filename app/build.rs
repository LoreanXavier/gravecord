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
    provenance();
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    } else if target.contains("windows-gnu") {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608");
    }
}

/// The commit this build comes from and whether the tree had changes, as `GRAVECORD_COMMIT` (the full hash, or
/// `unknown` outside a git checkout) and `GRAVECORD_DIRTY` (`1` or `0`). `cargo xtask package` sets both through
/// `GRAVECORD_BUILD_COMMIT` and `GRAVECORD_BUILD_DIRTY` after its own checks; any other build asks git.
fn provenance() {
    println!("cargo:rerun-if-env-changed=GRAVECORD_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=GRAVECORD_BUILD_DIRTY");
    let (commit, dirty) = match std::env::var("GRAVECORD_BUILD_COMMIT") {
        Ok(commit) if !commit.trim().is_empty() => (
            commit.trim().to_owned(),
            std::env::var("GRAVECORD_BUILD_DIRTY").as_deref() != Ok("0"),
        ),
        _ => from_git(),
    };
    println!("cargo:rustc-env=GRAVECORD_COMMIT={commit}");
    println!("cargo:rustc-env=GRAVECORD_DIRTY={}", u8::from(dirty));
}

fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The checkout's HEAD and whether any tracked or untracked (not ignored) file differs from it. The script runs
/// again when HEAD, the branch or a source file changes (staging alone changes neither the commit nor the tree).
fn from_git() -> (String, bool) {
    let Some(commit) = git(&["rev-parse", "HEAD"]).filter(|hash| hash.len() == 40) else {
        return ("unknown".to_owned(), true);
    };
    for path in ["HEAD", "packed-refs"] {
        if let Some(file) = git(&["rev-parse", "--path-format=absolute", "--git-path", path]) {
            println!("cargo:rerun-if-changed={file}");
        }
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(file) = git(&["rev-parse", "--path-format=absolute", "--git-path", &branch])
    {
        println!("cargo:rerun-if-changed={file}");
    }
    for source in [
        "src",
        "build.rs",
        "Cargo.toml",
        "../crates",
        "../Cargo.toml",
        "../Cargo.lock",
    ] {
        println!("cargo:rerun-if-changed={source}");
    }
    // No optional locks: a status that refreshed the index would change no source but still touch the checkout.
    let dirty = git(&["--no-optional-locks", "status", "--porcelain"])
        .is_none_or(|changes| !changes.is_empty());
    (commit, dirty)
}
