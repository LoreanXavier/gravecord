//! `cargo xtask audit`: checks `FEATURES.md` against the rules in the project plan section 8.4 and Appendix A.
//!
//! Not checked yet: that a `done` row's test passed in the last `cargo test` report. No row is `done`, and the check
//! fails loudly if one appears, so it cannot be skipped silently.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

const REQUIRED_DUMPS: [&str; 12] = [
    "GATEWAY",
    "VOICE",
    "SETTINGS",
    "EMBEDS",
    "CONTEXT_MENUS",
    "KEYBINDS",
    "EXPERIMENTS",
    "STRINGS",
    "CONSTANTS",
    "PROTOS",
    "TRAFFIC",
    "SOUNDS",
];

#[derive(Debug)]
struct Row<'a> {
    id: u32,
    source: &'a str,
    status: &'a str,
    test: &'a str,
}

fn parse_row(line: &str) -> Result<Row<'_>> {
    let cells: Vec<&str> = line.split('|').map(str::trim).collect();
    if cells.len() < 9 {
        bail!("row has too few cells: {line}");
    }
    let id = cells[1].strip_prefix("F-").context("id has no F- prefix")?;
    Ok(Row {
        id: id.parse().with_context(|| format!("bad id in {line}"))?,
        source: cells[4],
        status: cells[5],
        test: cells[6],
    })
}

fn rows(text: &str) -> Result<Vec<Row<'_>>> {
    text.lines()
        .filter(|line| line.starts_with("| F-"))
        .map(parse_row)
        .collect()
}

fn check_rows(rows: &[Row<'_>]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut last = 0;
    for row in rows {
        if row.id <= last {
            errors.push(format!("F-{:04} is not monotonic", row.id));
        }
        last = row.id;
        let known = matches!(row.status, "todo" | "wip" | "done" | "blocked-substitute")
            || row.status.starts_with("retired(");
        if !known {
            errors.push(format!("F-{:04} has unknown status {}", row.id, row.status));
        }
        if matches!(row.status, "done" | "blocked-substitute") && row.test.is_empty() {
            errors.push(format!("F-{:04} is {} without a test", row.id, row.status));
        }
        if row.status == "done" {
            errors.push(format!(
                "F-{:04} is done but test results are not verified yet",
                row.id
            ));
        }
    }
    errors
}

/// Ids in the committed file that the working file lost. Rows are never deleted.
fn removed_ids(committed: &[Row<'_>], current: &[Row<'_>]) -> Vec<u32> {
    let current: BTreeSet<u32> = current.iter().map(|row| row.id).collect();
    committed
        .iter()
        .map(|row| row.id)
        .filter(|id| !current.contains(id))
        .collect()
}

fn dump_sources(dumps: &Path) -> Result<Vec<String>> {
    let read = |name: &str| -> Result<Value> {
        serde_json::from_slice(
            &fs::read(dumps.join(name)).with_context(|| format!("reading {name}"))?,
        )
        .with_context(|| format!("parsing {name}"))
    };
    let mut sources = Vec::new();
    if let Value::Object(nodes) = read("dispatcher-nodes.json")? {
        sources.extend(nodes.keys().map(|name| format!("STORES:{}", name.trim())));
    }
    if let Value::Array(names) = read("actions.live.json")? {
        sources.extend(
            names
                .iter()
                .filter_map(Value::as_str)
                .map(|name| format!("ACTION_TYPES:{name}")),
        );
    }
    if let Value::Object(routes) = read("endpoints.live.json")? {
        sources.extend(routes.keys().map(|name| format!("ENDPOINTS:{name}")));
    }
    Ok(sources)
}

pub fn run(repo: &Path) -> Result<()> {
    let text = fs::read_to_string(repo.join("FEATURES.md"))?;
    let current = rows(&text)?;
    let mut errors = check_rows(&current);

    let committed = Command::new("git")
        .args(["show", "HEAD:FEATURES.md"])
        .current_dir(repo)
        .output()?;
    if committed.status.success() {
        let committed = String::from_utf8_lossy(&committed.stdout);
        for id in removed_ids(&rows(&committed)?, &current) {
            errors.push(format!("F-{id:04} was removed"));
        }
    }

    let dumps = repo.join("design").join("snapshot").join("dumps");
    let referenced: BTreeSet<&str> = current.iter().map(|row| row.source).collect();
    for source in dump_sources(&dumps)? {
        if !referenced.contains(source.as_str()) {
            errors.push(format!("{source} has no row"));
        }
    }

    let done = current.iter().filter(|row| row.status == "done").count();
    println!("{} rows, {done} done", current.len());
    let missing: Vec<&str> = REQUIRED_DUMPS
        .into_iter()
        .filter(|name| !dumps.join(format!("{name}.md")).exists() && !dumps.join(name).exists())
        .collect();
    if !missing.is_empty() {
        println!("dumps not produced yet: {}", missing.join(" "));
    }
    if !errors.is_empty() {
        for error in errors.iter().take(40) {
            eprintln!("{error}");
        }
        bail!("audit failed with {} errors", errors.len());
    }
    println!("audit passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str =
        "| F-0001 | a | b | X:1 | todo | | |\n| F-0002 | a | c | X:2 | wip | | note |\n";

    #[test]
    fn accepts_a_valid_inventory() {
        assert!(check_rows(&rows(GOOD).unwrap()).is_empty());
    }

    #[test]
    fn refuses_done_without_a_test() {
        let errors = check_rows(&rows("| F-0001 | a | b | X:1 | done | | |\n").unwrap());
        assert!(errors.iter().any(|e| e.contains("without a test")));
    }

    #[test]
    fn refuses_done_until_test_results_are_verified() {
        let errors = check_rows(&rows("| F-0001 | a | b | X:1 | done | t::x | |\n").unwrap());
        assert!(errors.iter().any(|e| e.contains("not verified")));
    }

    #[test]
    fn refuses_unknown_status_and_reused_ids() {
        let errors = check_rows(
            &rows("| F-0002 | a | b | X:1 | maybe | | |\n| F-0002 | a | b | X:2 | todo | | |\n")
                .unwrap(),
        );
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn accepts_retired_rows() {
        assert!(
            check_rows(&rows("| F-0001 | a | b | X:1 | retired(667580) | | |\n").unwrap())
                .is_empty()
        );
    }

    #[test]
    fn finds_removed_rows() {
        let committed = rows(GOOD).unwrap();
        let current = rows("| F-0002 | a | c | X:2 | wip | | note |\n").unwrap();
        assert_eq!(removed_ids(&committed, &current), [1]);
    }
}
