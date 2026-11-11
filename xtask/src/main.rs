mod asar;
mod audit;
mod client;
mod headless;
mod layout;
mod package;
mod parity;
mod perf;
mod report;
mod snapshot;

use anyhow::{Result, bail};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .parent()
        .expect("xtask lives inside the repository")
        .to_owned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["snapshot", "pin", rest @ ..] => snapshot::pin(&repo_root(), rest),
        ["client", rest @ ..] => client::run(&repo_root(), rest),
        ["audit"] => audit::run(&repo_root()),
        ["report"] => report::run(&repo_root()),
        ["parity", rest @ ..] => parity::run(&repo_root(), rest),
        ["package"] => package::run(&repo_root()),
        ["perf", rest @ ..] => perf::run(&repo_root(), rest),
        ["headless", rest @ ..] => headless::run(&repo_root(), rest),
        [] => bail!("usage: cargo xtask <command>"),
        other => bail!("not implemented yet: {}", other.join(" ")),
    }
}
