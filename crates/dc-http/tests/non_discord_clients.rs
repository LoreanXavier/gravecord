//! A static check over the shipped sources (`app/src`, `crates/*/src`, without their test modules): every HTTP client
//! that can reach a host outside Discord is built from `dc_http::plain`, and the Discord identity (the pinned user
//! agent, the fixed header set, the client information header) is attached only where requests go to Discord.
//!
//! The pinned client itself chooses per request: its API routes go to `api_base`, and every other request it makes
//! (`fetch_public*`, `post_public_json`, `put_upload*`) asks `public_http`, which hands a non-Discord host the plain
//! client. That choice is tested on the wire in `dc-http`'s unit tests; here the source makes sure no other request of
//! the pinned client skips it.

use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/dc-http sits two levels under the workspace")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(items) = std::fs::read_dir(dir) else {
        return;
    };
    for item in items.flatten() {
        let path = item.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "tests") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && path.file_name().is_some_and(|name| name != "tests.rs")
        {
            out.push(path);
        }
    }
}

/// Every shipped source file, as `(path relative to the workspace with forward slashes, text without its test module)`.
fn shipped_sources() -> Vec<(String, String)> {
    let root = workspace();
    let mut files = Vec::new();
    rust_files(&root.join("app").join("src"), &mut files);
    let crates = std::fs::read_dir(root.join("crates")).expect("the crates folder");
    for item in crates.flatten() {
        rust_files(&item.path().join("src"), &mut files);
    }
    assert!(files.len() > 100, "the sources were found: {}", files.len());
    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            let shipped = match text.find("#[cfg(test)]\nmod tests") {
                Some(at) => text[..at].to_owned(),
                None => text,
            };
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (relative, shipped)
        })
        .collect()
}

/// Ways of making an HTTP client other than `dc_http::plain::builder()`.
const CLIENT_CONSTRUCTORS: &[&str] = &[
    "reqwest::Client::builder(",
    "reqwest::Client::new(",
    "reqwest::ClientBuilder",
    "reqwest::get(",
    "reqwest::blocking",
    "Client::builder()",
];

/// The files allowed to build a client another way, and why each only talks to Discord.
const CLIENT_ALLOWED: &[(&str, &str)] = &[
    (
        "crates/dc-http/src/lib.rs",
        "the pinned Discord pool; non-Discord requests are handed the plain client by public_http",
    ),
    ("crates/dc-http/src/plain.rs", "the plain client itself"),
    (
        "crates/dc-http/src/webhook.rs",
        "posts to pin::API_BASE only",
    ),
    (
        "crates/dc-auth/src/api.rs",
        "the login client, on pin::API_BASE only",
    ),
];

/// The Discord identity: the pinned user agent, the fixed header set and the client information header.
const IDENTITY: &[&str] = &[
    "pin::USER_AGENT",
    "fixed_headers(",
    "super_properties_header(",
    "x-discord-timezone",
    "x-discord-locale",
];

/// The files allowed to use it, and where their requests go.
const IDENTITY_ALLOWED: &[(&str, &str)] = &[
    ("crates/dc-proto/src/pin.rs", "defines it"),
    (
        "crates/dc-proto/src/identity.rs",
        "builds the client information",
    ),
    (
        "crates/dc-http/src/lib.rs",
        "the pinned Discord pool and its API routes",
    ),
    (
        "crates/dc-http/src/plain.rs",
        "derives the plain user agent from it, without its discord/ and Electron/ tokens",
    ),
    (
        "crates/dc-auth/src/api.rs",
        "login requests to pin::API_BASE",
    ),
    (
        "crates/dc-gateway/src/lib.rs",
        "the gateway, gateway.discord.gg",
    ),
    (
        "crates/dc-voice/src/session.rs",
        "the voice servers, *.discord.media",
    ),
    (
        "app/src/ui/plugin_hooks/more_commands.rs",
        "reads the browser name out of it as message text; sends no request with it",
    ),
];

#[test]
fn only_discord_bound_code_builds_a_client_of_its_own() {
    let mut found = Vec::new();
    for (path, text) in shipped_sources() {
        if CLIENT_ALLOWED.iter().any(|(allowed, _)| *allowed == path) {
            continue;
        }
        for pattern in CLIENT_CONSTRUCTORS {
            if text.contains(pattern) {
                found.push(format!("{path}: {pattern}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "build these clients from dc_http::plain::builder(), or add the file to CLIENT_ALLOWED with the Discord \
         host it talks to:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_discord_identity_is_attached_only_to_discord_requests() {
    let mut found = Vec::new();
    for (path, text) in shipped_sources() {
        if IDENTITY_ALLOWED.iter().any(|(allowed, _)| *allowed == path) {
            continue;
        }
        for pattern in IDENTITY {
            if text.contains(pattern) {
                found.push(format!("{path}: {pattern}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "the Discord identity is used outside Discord-bound code:\n{}",
        found.join("\n")
    );
}

/// The name of the function each `self.http()` of the pinned client sits in: only the API routes (which go to
/// `api_base`) and `public_http` (which checks the host) may take the pinned pool.
#[test]
fn the_pinned_pool_is_taken_only_by_api_routes_and_the_host_check() {
    const MAY_TAKE: &[&str] = &[
        "send",
        "get_as_bearer",
        "post_form",
        "post_files",
        "public_http",
    ];
    let root = workspace().join("crates").join("dc-http").join("src");
    let mut found = Vec::new();
    for name in [
        "lib.rs",
        "upload.rs",
        "webhook.rs",
        "spotify.rs",
        "plain.rs",
    ] {
        let text = std::fs::read_to_string(root.join(name))
            .unwrap()
            .replace("\r\n", "\n");
        let shipped = text
            .find("#[cfg(test)]\nmod tests")
            .map_or(text.as_str(), |at| &text[..at]);
        let mut current = String::new();
        for line in shipped.lines() {
            let trimmed = line.trim_start();
            for prefix in [
                "pub async fn ",
                "async fn ",
                "pub fn ",
                "fn ",
                "pub(crate) fn ",
            ] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    current = rest
                        .split(['(', '<'])
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                }
            }
            // `self.http()` on one line, or `self` then `.http()` on the next as rustfmt splits a chain.
            let takes = line.contains("self.http()") || trimmed == ".http()";
            if takes && !MAY_TAKE.contains(&current.as_str()) {
                found.push(format!("{name}: fn {current}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a request outside the API routes takes the pinned pool without the host check (use public_http):\n{}",
        found.join("\n")
    );
}
