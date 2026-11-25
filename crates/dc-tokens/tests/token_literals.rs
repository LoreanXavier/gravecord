//! Every `"--name"` literal in the product code that looks like a design token resolves in all four palettes.
//! An unknown token is a debug assertion and a transparent colour in release, so it has to be caught here.

use dc_tokens::Theme;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const THEMES: [Theme; 4] = [Theme::Dark, Theme::Light, Theme::Darker, Theme::Midnight];

/// Literals of a token family that are not looked up as tokens, never passed to `Theme::color`.
const NOT_TOKENS: &[&str] = &[
    // A command line flag of the capture crawl (`harness/crawl.rs`), not the `--neutral-*` colour family.
    "--neutral",
    // The embed title colour MessageLogger recolours (`plugin_hooks/message_logger.rs`); the app's embeds do not read it.
    "--embed-title",
];

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// The `"--name"` literals of one file's code, its test module and whole-line comments left out.
fn literals(text: &str) -> Vec<String> {
    let code = text.split("#[cfg(test)]").next().unwrap_or("");
    let mut found = Vec::new();
    for line in code.lines() {
        let line = line.trim_start();
        if line.starts_with("//") {
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("\"--") {
            let tail = &rest[at + 1..];
            let end = tail
                .find(|c: char| {
                    !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
                })
                .unwrap_or(tail.len());
            if tail[end..].starts_with('"') && end > 2 {
                found.push(tail[..end].to_owned());
            }
            rest = &rest[at + 1 + end..];
        }
    }
    found
}

fn family(name: &str) -> &str {
    name.trim_start_matches('-').split('-').next().unwrap_or("")
}

#[test]
fn literal_scan_finds_quoted_names_only() {
    let text = "color(\"--text-default\");\n    // color(\"--in-a-comment\")\nlet x = \"--capture\"; \"--x{y}\"\n#[cfg(test)]\nfn t() { color(\"--in-a-test\") }";
    assert_eq!(literals(text), ["--text-default", "--capture"]);
}

#[test]
fn every_token_literal_of_the_product_resolves_in_every_palette() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&root.join("app/src"), &mut files);
    rust_files(&root.join("crates"), &mut files);
    files.retain(|path| !path.ends_with("dc-tokens/src/generated.rs"));
    assert!(files.len() > 200, "the scan found {} files", files.len());

    // The families are the palette's own first segments, so a new family is checked without a list to keep up.
    // Command line flags ("--capture") share the literal shape and are skipped by family.
    let families = palette_families();
    assert!(families.contains("background") && families.contains("text"));

    let mut checked = 0;
    let mut unknown = BTreeSet::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        for name in literals(&text) {
            if NOT_TOKENS.contains(&name.as_str()) || !families.contains(family(&name)) {
                continue;
            }
            checked += 1;
            for theme in THEMES {
                if !theme.has(&name) {
                    unknown.insert(format!("{name} ({theme:?}) in {}", path.display()));
                }
            }
        }
    }
    assert!(checked > 2000, "the scan checked {checked} literals");
    assert!(
        unknown.is_empty(),
        "unknown design tokens:\n{}",
        unknown.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// First segments of every palette name, read from the generated tables.
fn palette_families() -> BTreeSet<String> {
    let generated = include_str!("../src/generated.rs");
    let mut families = BTreeSet::new();
    let mut rest = generated;
    while let Some(at) = rest.find("\"--") {
        let tail = &rest[at + 1..];
        let end = tail.find('"').unwrap_or(tail.len());
        let name = &tail[..end];
        if THEMES.iter().all(|theme| theme.has(name)) {
            families.insert(family(name).to_owned());
        }
        rest = &tail[end..];
    }
    families
}

#[test]
#[cfg(not(debug_assertions))]
fn an_unknown_token_is_transparent_in_release() {
    assert_eq!(
        Theme::Dark.color("--no-such-token"),
        dc_tokens::UNKNOWN_TOKEN
    );
    assert_eq!(
        Theme::Light.color("--no-such-token"),
        dc_tokens::UNKNOWN_TOKEN
    );
}

/// The button colours built with `format!("--control-{family}-{part}-{state}")` (`modal.rs`, `voice_invite.rs`,
/// `clips/widgets.rs`, `settings/connection_card.rs`, `settings/account_dialogs.rs` and the like), which the literal
/// scan cannot see.
#[test]
fn every_built_control_token_resolves_in_every_palette() {
    let mut unknown = Vec::new();
    for family in [
        "primary",
        "secondary",
        "connected",
        "critical-primary",
        "critical-secondary",
    ] {
        for part in ["background", "border", "text", "icon"] {
            for state in ["default", "hover", "active"] {
                let name = format!("--control-{family}-{part}-{state}");
                for theme in THEMES {
                    if !theme.has(&name) {
                        unknown.push(format!("{name} ({theme:?})"));
                    }
                }
            }
        }
    }
    assert!(unknown.is_empty(), "unknown built tokens: {unknown:?}");
}

#[test]
fn a_known_token_resolves() {
    assert_ne!(
        Theme::Dark.color("--text-default"),
        dc_tokens::UNKNOWN_TOKEN
    );
}
