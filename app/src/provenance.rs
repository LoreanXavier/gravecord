//! Which source this build was made from: the commit hash and whether the tree had changes, stamped by `build.rs`
//! (`cargo xtask package` passes its checked values; other builds ask git). Shown next to the version in the
//! settings sidebar's footer, in the copied version text, and in every `panic.log` entry.
//!
//! The stamp names the commit a build says it came from. It is not a proof: only a build made from that commit
//! yourself can show that a file matches it.

/// The full commit hash, or `unknown` outside a git checkout.
pub const COMMIT: &str = env!("GRAVECORD_COMMIT");

/// Whether the build's tree differed from [`COMMIT`] (or was packaged with `--allow-dirty`).
pub fn dirty() -> bool {
    env!("GRAVECORD_DIRTY") == "1"
}

/// The short form shown to people: the first seven characters, with `-dirty` after them when [`dirty`].
pub fn short() -> String {
    label(COMMIT, dirty())
}

fn label(commit: &str, dirty: bool) -> String {
    let short = commit.get(..7).unwrap_or(commit);
    if dirty {
        format!("{short}-dirty")
    } else {
        short.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_is_seven_characters_and_says_dirty() {
        assert_eq!(
            label("0123456789abcdef0123456789abcdef01234567", false),
            "0123456"
        );
        assert_eq!(
            label("0123456789abcdef0123456789abcdef01234567", true),
            "0123456-dirty"
        );
        assert_eq!(label("unknown", true), "unknown-dirty");
        assert!(COMMIT == "unknown" || COMMIT.len() == 40, "{COMMIT}");
        assert!(short().starts_with(COMMIT.get(..7).unwrap_or(COMMIT)));
    }
}
