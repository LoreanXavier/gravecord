//! Making a WebView2 view waits for it with a nested message loop (`webview2_com::wait_with_pump`). On the app's
//! thread that loop runs gpui's foreground tasks while the app is mid-update, and the first one that updates an entity
//! panics with "RefCell already borrowed". So the view is only ever built on the surface's own thread, in `run`.

use std::path::{Path, PathBuf};

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

const BUILDERS: [&str; 4] = [
    "WebViewBuilder",
    "build_as_child",
    ".build(&",
    "wait_with_pump",
];

#[test]
fn web_views_are_built_only_on_the_surface_thread() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let mut files = Vec::new();
    rust_files(&root.join("app/src"), &mut files);
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 200, "the scan found {} files", files.len());
    for path in &files {
        if path.ends_with("dc-webhost/src/lib.rs")
            || path.components().any(|part| part.as_os_str() == "tests")
        {
            continue;
        }
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        assert!(
            !code.contains("WebViewBuilder")
                && !code.contains("build_as_child")
                && !code.contains("wait_with_pump"),
            "{} builds a web view outside dc-webhost's surface thread",
            path.display()
        );
    }

    let text = std::fs::read_to_string(host).unwrap().replace("\r\n", "\n");
    let start = text.find("\nfn run(").expect("the surface thread's `run`");
    let end = start + 1 + text[start + 1..].find("\n}\n").expect("the end of `run`");
    let (before, rest) = text.split_at(start);
    let (_, after) = rest.split_at(end - start);
    assert!(
        text[start..end].contains("build_as_child"),
        "`run` builds the view"
    );
    for outside in [before, after] {
        let code = outside.split("#[cfg(test)]").next().unwrap_or("");
        for builder in &BUILDERS[1..] {
            assert!(
                !code.contains(builder),
                "`{builder}` outside the surface thread's `run`"
            );
        }
    }
    // `run` is only ever started on a thread of its own.
    let calls: Vec<_> = text
        .match_indices("run(\n")
        .map(|(at, _)| at)
        .filter(|&at| !text[..at].ends_with("fn "))
        .collect();
    assert!(!calls.is_empty());
    for at in calls {
        let lead = &text[at.saturating_sub(400)..at];
        assert!(
            lead.contains("std::thread::Builder::new()") && lead.contains(".spawn("),
            "`run` is called off a spawned thread"
        );
    }
}
