//! `cargo xtask gaps`: what of the pinned client's visible text this client never shows. Every web module that names a
//! message of the string catalog (`<intl>.t.KEY` or `<intl>.t["KEY"]`) is a piece of interface; a message whose key
//! and English text appear nowhere in our Rust sources is something we do not draw. The report groups the missing
//! messages by module, with the CSS class names the module imports as a hint of what the component is, so whole
//! missing features (a popout, a menu, a status line) show up without anyone clicking through the app.
//!
//! Output: `design/gaps/gaps.md` (ranked by missing messages) and `design/gaps/gaps.tsv` (one line per missing message:
//! module, key, text, class hints). Both are local, like the rest of `design/`.

use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// A module naming more catalog keys than this is a locale catalog, not interface.
const CATALOG_KEYS: usize = 1500;

pub fn run(repo: &Path) -> Result<()> {
    let snapshot = repo.join("design").join("snapshot");
    let strings: BTreeMap<String, Value> = serde_json::from_str(
        &fs::read_to_string(snapshot.join("dumps/STRINGS/en-US.json"))
            .context("reading the en-US string catalog")?,
    )?;
    let texts: HashMap<&str, Text> = strings
        .iter()
        .map(|(key, value)| (key.as_str(), Text::of(value)))
        .collect();

    let corpus = our_sources(repo)?;
    let covered = |key: &str, text: &Text| -> bool {
        corpus.contains(&format!("\"{key}\"")) || text.found_in(&corpus)
    };

    let modules_dir = snapshot.join("modules");
    let mut css: HashMap<String, Vec<String>> = HashMap::new();
    let mut uses: BTreeMap<String, (BTreeSet<String>, Vec<String>)> = BTreeMap::new();
    for entry in fs::read_dir(&modules_dir)? {
        let path = entry?.path();
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(classes) = css_classes(&source) {
            css.insert(id.clone(), classes);
            continue;
        }
        let keys = keys_named(&source, &texts);
        if keys.is_empty() || keys.len() > CATALOG_KEYS {
            continue;
        }
        uses.insert(id, (keys, imports(&source)));
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut seen_missing: BTreeSet<String> = BTreeSet::new();
    let mut total_keys: BTreeSet<String> = BTreeSet::new();
    for (module, (keys, imported)) in &uses {
        let hints: Vec<String> = imported
            .iter()
            .filter_map(|id| css.get(id))
            .flatten()
            .take(8)
            .cloned()
            .collect();
        let mut missing = Vec::new();
        for key in keys {
            total_keys.insert(key.clone());
            let text = &texts[key.as_str()];
            if text.display.trim().is_empty() || covered(key, text) {
                continue;
            }
            seen_missing.insert(key.clone());
            missing.push((key.clone(), text.display.clone()));
        }
        if !missing.is_empty() {
            rows.push(Row {
                module: module.clone(),
                total: keys.len(),
                missing,
                hints,
            });
        }
    }
    rows.sort_by(|a, b| {
        b.missing
            .len()
            .cmp(&a.missing.len())
            .then(a.module.cmp(&b.module))
    });

    let out = repo.join("design").join("gaps");
    fs::create_dir_all(&out)?;
    let mut md = String::new();
    let _ = writeln!(
        md,
        "# Gaps against the pinned client\n\nInterface messages named by modules: {}; not found in our sources: {} \
         ({:.1}% covered). Modules with something missing: {}.\n",
        total_keys.len(),
        seen_missing.len(),
        100.0 * (total_keys.len() - seen_missing.len()) as f64 / total_keys.len().max(1) as f64,
        rows.len()
    );
    let mut tsv = String::from("module\tkey\ttext\thints\n");
    for row in &rows {
        let _ = writeln!(
            md,
            "## {} ({} of {} missing)\n\nhints: {}\n",
            row.module,
            row.missing.len(),
            row.total,
            row.hints.join(", ")
        );
        for (key, text) in &row.missing {
            let _ = writeln!(md, "- `{key}` {}", one_line(text));
            let _ = writeln!(
                tsv,
                "{}\t{key}\t{}\t{}",
                row.module,
                one_line(text),
                row.hints.join(" ")
            );
        }
        md.push('\n');
    }
    fs::write(out.join("gaps.md"), md)?;
    fs::write(out.join("gaps.tsv"), tsv)?;
    println!(
        "{} interface messages, {} missing ({:.1}% covered), {} modules with gaps; report in design/gaps/",
        total_keys.len(),
        seen_missing.len(),
        100.0 * (total_keys.len() - seen_missing.len()) as f64 / total_keys.len().max(1) as f64,
        rows.len()
    );
    Ok(())
}

struct Row {
    module: String,
    total: usize,
    missing: Vec<(String, String)>,
    hints: Vec<String>,
}

/// A catalog message reduced to what a search of our sources can find.
struct Text {
    /// The message as one line, parameters shown as `{name}`.
    display: String,
    /// The literal pieces between parameters.
    pieces: Vec<String>,
}

impl Text {
    fn of(value: &Value) -> Self {
        let mut display = String::new();
        let mut pieces = Vec::new();
        let mut current = String::new();
        fn walk(
            value: &Value,
            display: &mut String,
            pieces: &mut Vec<String>,
            current: &mut String,
        ) {
            match value {
                Value::String(text) => {
                    display.push_str(text);
                    current.push_str(text);
                }
                Value::Array(parts) => {
                    // `[kind, name, ...]` is a parameter or a tag; anything else is a list of parts.
                    if let (Some(Value::Number(_)), Some(Value::String(name))) =
                        (parts.first(), parts.get(1))
                    {
                        let _ = write!(display, "{{{name}}}");
                        if !current.trim().is_empty() {
                            pieces.push(std::mem::take(current));
                        } else {
                            current.clear();
                        }
                        // A plural or select carries its branches after the name; their text is optional.
                        return;
                    }
                    for part in parts {
                        walk(part, display, pieces, current);
                    }
                }
                _ => {}
            }
        }
        walk(value, &mut display, &mut pieces, &mut current);
        if !current.trim().is_empty() {
            pieces.push(current);
        }
        Self { display, pieces }
    }

    /// The whole text for a plain message; for one with parameters, its longest literal piece when that is long
    /// enough to mean something.
    fn found_in(&self, corpus: &str) -> bool {
        if self.pieces.len() <= 1 && !self.display.contains('{') {
            let text = escape(&self.display);
            return text.len() >= 2 && corpus.contains(&text);
        }
        match self
            .pieces
            .iter()
            .map(|piece| piece.trim())
            .max_by_key(|piece| piece.len())
        {
            Some(longest) if longest.len() >= 6 => corpus.contains(&escape(longest)),
            // Too little text to tell (`{count} {unit}`): counted as present.
            _ => true,
        }
    }
}

/// How the text sits inside a Rust string literal.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\t'], " ")
}

/// Every `.rs` file of the app and the crates, except the string catalogs themselves (`dc-intl` holds every message).
fn our_sources(repo: &Path) -> Result<String> {
    let mut corpus = String::new();
    let mut stack: Vec<PathBuf> = vec![repo.join("app").join("src"), repo.join("crates")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "dc-intl" || name == "target" {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                corpus.push_str(&fs::read_to_string(&path).unwrap_or_default());
                corpus.push('\n');
            }
        }
    }
    Ok(corpus)
}

/// The catalog keys a module names as `.t.KEY` or `.t["KEY"]`.
fn keys_named(source: &str, texts: &HashMap<&str, Text>) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let bytes = source.as_bytes();
    let mut at = 0;
    while let Some(found) = source[at..].find(".t") {
        let start = at + found + 2;
        at = start;
        let candidate = match bytes.get(start) {
            Some(b'.') => source
                .get(start + 1..start + 7)
                .filter(|key| {
                    key.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
                })
                .filter(|_| {
                    bytes
                        .get(start + 7)
                        .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b == b'$'))
                }),
            Some(b'[')
                if bytes.get(start + 1) == Some(&b'"') && bytes.get(start + 8) == Some(&b'"') =>
            {
                source.get(start + 2..start + 8)
            }
            _ => None,
        };
        if let Some(key) = candidate
            && texts.contains_key(key)
        {
            keys.insert(key.to_owned());
        }
    }
    keys
}

/// A CSS module (`e.exports={a:"name_hash",...}`): its class names without the hash.
fn css_classes(source: &str) -> Option<Vec<String>> {
    let body = source.split_once("exports={")?.1;
    if source.len() > 20_000 || !body.contains(':') {
        return None;
    }
    let mut classes = Vec::new();
    for part in body.split(',') {
        let value = part
            .split_once(':')?
            .1
            .trim()
            .trim_end_matches(['}', ')'])
            .trim();
        let value = value.strip_prefix('"')?.split('"').next()?;
        let base = value.split(' ').next().unwrap_or(value);
        let name = base.rsplit_once('_').map_or(base, |(name, _)| name);
        if !name.is_empty() && !classes.iter().any(|c| c == name) {
            classes.push(name.to_owned());
        }
    }
    (!classes.is_empty()).then_some(classes)
}

/// The module ids a module requires (`n(123456)`), in order.
fn imports(source: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes = source.as_bytes();
    for (index, _) in source.match_indices('(') {
        if index == 0 || !bytes[index - 1].is_ascii_alphabetic() {
            continue;
        }
        if index >= 2 && (bytes[index - 2].is_ascii_alphanumeric() || bytes[index - 2] == b'_') {
            continue;
        }
        let digits: String = source[index + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.len() >= 3
            && source[index + 1 + digits.len()..].starts_with(')')
            && !ids.contains(&digits)
        {
            ids.push(digits);
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_reduce_to_searchable_text() {
        let plain = Text::of(&serde_json::json!(["Watch Stream"]));
        assert!(plain.found_in("\"Watch Stream\""));
        assert!(!plain.found_in("Watch"));
        let size = Text::of(&serde_json::json!([
            [1, "width"],
            "x",
            [1, "height"],
            " (",
            [1, "fileSize"],
            ")"
        ]));
        assert!(size.found_in(""), "too little literal text to tell");
        let sharing = Text::of(&serde_json::json!([
            "Sharing their screen with ",
            [1, "name"]
        ]));
        assert!(sharing.found_in("\"Sharing their screen with \""));
        assert!(!sharing.found_in("Sharing"));
    }

    #[test]
    fn keys_and_css_maps_are_read() {
        let mut texts = HashMap::new();
        texts.insert("abcdEF", Text::of(&serde_json::json!(["A"])));
        texts.insert("+Kakw+", Text::of(&serde_json::json!(["B"])));
        let keys = keys_named(
            r#"x.intl.string(x.t.abcdEF),y.t["+Kakw+"],z.t.abcdEFG"#,
            &texts,
        );
        assert_eq!(keys.into_iter().collect::<Vec<_>>(), ["+Kakw+", "abcdEF"]);
        let classes = css_classes(
            r#"47299(c){c.exports={jj:"gifFavoriteButton_cT9zhv",wH:"selected_cT9zhv"}}"#,
        )
        .unwrap();
        assert_eq!(classes, ["gifFavoriteButton", "selected"]);
        assert_eq!(
            imports("var a=n(47299),b=n(12);x(123456)"),
            ["47299", "123456"]
        );
    }
}
