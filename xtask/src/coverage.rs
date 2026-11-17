//! Two checks of `cargo xtask audit` that keep gaps from being found by chance (docs/parity-system.md):
//!
//! * **Data coverage** (`coverage/<area>.tsv`, written by `python tools/coverage.py record`): every key path real
//!   answers carry is `read` by the code (its last key appears in app/ or crates/), or `ignored` with a reason. An
//!   `unknown` path, a `read` path whose key left the code, or an `ignored` one without a reason fails.
//! * **The ledger ratchet** (`coverage/ledger-cited.txt`): the official UI modules the code cites (see
//!   `tools/ledger.py`) may only grow. A module that was cited and no longer is fails, so a port is not undone unseen.
//!   New citations are added to the file by `cargo xtask audit --update-ledger`.

use anyhow::Result;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Every string literal and `pub field:` name in the Rust sources of app/ and crates/.
fn code_keys(repo: &Path) -> Result<BTreeSet<String>> {
    let mut keys = BTreeSet::new();
    let mut sources = String::new();
    for base in ["app", "crates"] {
        collect(&repo.join(base), &mut sources)?;
    }
    let bytes = sources.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'"' && end > start {
                keys.insert(sources[start..end].to_owned());
            }
            index = end.max(start);
        } else {
            index += 1;
        }
    }
    for line in sources.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("pub ")
            && let Some((name, _)) = rest.split_once(':')
            && !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            keys.insert(name.to_owned());
        }
    }
    Ok(keys)
}

fn collect(folder: &Path, into: &mut String) -> Result<()> {
    if !folder.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect(&path, into)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            into.push_str(&fs::read_to_string(&path).unwrap_or_default());
            into.push('\n');
        }
    }
    Ok(())
}

/// The last key of a path (`a.b[].c` is `c`), or none for a map of ids.
fn leaf(path: &str) -> Option<&str> {
    let last = path.rsplit('.').next()?.trim_end_matches("[]");
    (last != "{id}").then_some(last)
}

/// The failures of the coverage tables against the code's keys.
pub fn coverage_errors(tables: &[(String, String)], keys: &BTreeSet<String>) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, text) in tables {
        for line in text.lines().skip(1) {
            let mut cells = line.split('\t');
            let (Some(path), _, Some(status)) = (cells.next(), cells.next(), cells.next()) else {
                continue;
            };
            let reason = cells.next().unwrap_or("").trim();
            match status {
                "unknown" => errors.push(format!("{name}: {path} is not read and has no reason")),
                "ignored" if reason.is_empty() => {
                    errors.push(format!("{name}: {path} is ignored without a reason"))
                }
                "read" => {
                    if let Some(key) = leaf(path)
                        && !keys.contains(key)
                    {
                        errors.push(format!(
                            "{name}: {path} was read, but `{key}` left the code"
                        ));
                    }
                }
                "ignored" => {}
                other => errors.push(format!("{name}: {path} has an unknown status `{other}`")),
            }
        }
    }
    errors
}

/// Module ids the code cites: 4 to 6 digit numbers standing alone in the sources.
fn cited_modules(repo: &Path) -> Result<BTreeSet<String>> {
    let mut sources = String::new();
    for base in ["app", "crates", "xtask"] {
        collect(&repo.join(base), &mut sources)?;
    }
    let mut cited = BTreeSet::new();
    let bytes = sources.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit()
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric())
        {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            let after = bytes.get(index).copied().unwrap_or(b' ');
            if (4..=6).contains(&(index - start)) && !after.is_ascii_alphanumeric() {
                cited.insert(sources[start..index].to_owned());
            }
        } else {
            index += 1;
        }
    }
    Ok(cited)
}

/// The audit's coverage and ledger failures. With `update`, new citations are added to the ratchet file.
pub fn errors(repo: &Path, update: bool) -> Result<Vec<String>> {
    let folder = repo.join("coverage");
    let mut tables = Vec::new();
    if folder.exists() {
        for entry in fs::read_dir(&folder)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "tsv")
                && !path.ends_with("ledger-triage.tsv")
            {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                tables.push((name, fs::read_to_string(&path)?));
            }
        }
    }
    let mut errors = coverage_errors(&tables, &code_keys(repo)?);

    let ratchet = folder.join("ledger-cited.txt");
    let modules = repo.join("design").join("snapshot").join("modules");
    if modules.exists() {
        let cited = cited_modules(repo)?;
        // Only ids that are modules of the pinned build count.
        let ui: BTreeSet<String> = cited
            .iter()
            .filter(|id| modules.join(format!("{id}.js")).exists())
            .cloned()
            .collect();
        let before: BTreeSet<String> = fs::read_to_string(&ratchet)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect();
        for gone in before.difference(&ui) {
            errors.push(format!("ledger: module {gone} was cited and no longer is"));
        }
        let new = ui.difference(&before).count();
        if update || !ratchet.exists() {
            let mut text = String::from(
                "# Official modules the code cites (cargo xtask audit --update-ledger). The list may only grow.\n",
            );
            for id in &ui {
                text.push_str(id);
                text.push('\n');
            }
            fs::create_dir_all(&folder)?;
            fs::write(&ratchet, text)?;
        } else if new > 0 {
            println!("ledger: {new} newly cited modules (record them with --update-ledger)");
        }
        println!("ledger: {} official modules cited", ui.len());
        let triage = fs::read_to_string(folder.join("ledger-triage.tsv")).unwrap_or_default();
        let (triage_errors, backlog) = triage_errors(repo, &modules, &ui, &triage)?;
        errors.extend(triage_errors);
        println!(
            "ledger: backlog {} missing, {} partial (coverage/ledger-triage.tsv)",
            backlog.0, backlog.1
        );
    }
    Ok(errors)
}

/// Whether a module renders elements and shows a message: one row of the ledger (`tools/ledger.py`).
fn is_ui_module(source: &str) -> bool {
    if !source.contains(".jsx") {
        return false;
    }
    let bytes = source.as_bytes();
    let key = |at: usize| {
        bytes.get(at..at + 6).is_some_and(|key| {
            key.iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
        })
    };
    source
        .match_indices(".t[\"")
        .any(|(at, _)| key(at + 4) && bytes.get(at + 10) == Some(&b'"'))
        || source.match_indices(".t.").any(|(at, _)| {
            key(at + 3)
                && bytes[at + 3..at + 9].iter().all(u8::is_ascii_alphanumeric)
                && !bytes.get(at + 9).is_some_and(u8::is_ascii_alphanumeric)
        })
}

/// Every UI module of the pinned build is cited by the code or has a row in the triage table: `ported` (with the
/// files that port it, which must exist), `partial` or `missing` (the backlog), or `not-applicable` (with its reason).
/// A UI module in neither fails, so nothing the official client can draw is left unlooked at. Returns the failures and
/// the backlog's (missing, partial) counts.
fn triage_errors(
    repo: &Path,
    modules: &Path,
    cited: &BTreeSet<String>,
    triage: &str,
) -> Result<(Vec<String>, (usize, usize))> {
    let mut errors = Vec::new();
    let mut rows = std::collections::BTreeMap::new();
    for line in triage.lines().skip(1) {
        let mut cells = line.split('\t');
        let (Some(module), Some(class)) = (cells.next(), cells.next()) else {
            continue;
        };
        let place = cells.next().unwrap_or("").trim();
        let note = cells.next().unwrap_or("").trim();
        match class {
            "ported" => {
                for file in place
                    .split([',', ';', ' '])
                    .map(str::trim)
                    .filter(|f| f.ends_with(".rs"))
                {
                    if !repo.join(file).exists() {
                        errors.push(format!(
                            "ledger-triage: {module} names {file}, which does not exist"
                        ));
                    }
                }
            }
            "not-applicable" if note.is_empty() => errors.push(format!(
                "ledger-triage: {module} is not-applicable without a reason"
            )),
            "partial" | "missing" | "not-applicable" => {}
            other => errors.push(format!(
                "ledger-triage: {module} has an unknown class `{other}`"
            )),
        }
        rows.insert(module.to_owned(), class.to_owned());
    }
    let mut backlog = (0, 0);
    for entry in fs::read_dir(modules)? {
        let path = entry?.path();
        let Some(id) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if cited.contains(&id) {
            continue;
        }
        match rows.get(&id).map(String::as_str) {
            Some("missing") => backlog.0 += 1,
            Some("partial") => backlog.1 += 1,
            Some(_) => {}
            None => {
                let source = fs::read_to_string(&path).unwrap_or_default();
                if is_ui_module(&source) {
                    errors.push(format!(
                        "ledger: UI module {id} is neither cited nor triaged"
                    ));
                }
            }
        }
    }
    Ok((errors, backlog))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_paths_vanished_keys_and_reasonless_ignores_fail() {
        let keys: BTreeSet<String> = ["bio", "pronouns"].into_iter().map(str::to_owned).collect();
        let table = "path\tanswers\tstatus\treason\n\
                     user_profile.bio\t9/9\tread\t\n\
                     user_profile.pronouns\t3/9\tread\t\n\
                     user_profile.accent_color\t2/9\tread\t\n\
                     guild_badges\t9/9\tignored\tempty in every answer, drawn by 851767 when present\n\
                     widgets[].updated_at\t4/9\tignored\t\n\
                     connected_accounts[].metadata.total_karma\t1/9\tunknown\t\n\
                     wishlist_settings.{id}\t1/9\tread\t\n";
        let errors = coverage_errors(&[("profile.tsv".into(), table.into())], &keys);
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(errors[0].contains("accent_color"));
        assert!(errors[1].contains("updated_at"));
        assert!(errors[2].contains("total_karma"));
    }

    #[test]
    fn ui_modules_render_elements_and_show_messages() {
        assert!(is_ui_module(
            r#"(0,r.jsx)("div",{children:o.intl.string(o.t["0ZB/XE"])})"#
        ));
        assert!(is_ui_module(
            r#"(0,r.jsx)(A,{title:s.intl.string(s.t.P2JAEc)})"#
        ));
        assert!(!is_ui_module(r#"(0,r.jsx)("div",{})"#));
        assert!(!is_ui_module(r#"let x=s.t["0ZB/XE"]"#));
    }

    #[test]
    fn a_leaf_is_the_last_key_and_maps_of_ids_have_none() {
        assert_eq!(leaf("a.b[].c"), Some("c"));
        assert_eq!(leaf("widgets[]"), Some("widgets"));
        assert_eq!(leaf("wishlist_settings.{id}"), None);
    }
}
