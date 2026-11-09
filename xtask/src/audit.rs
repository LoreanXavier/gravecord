//! `cargo xtask audit`: checks `FEATURES.md` against the rules in the project plan section 8.4 and Appendix A.
//!
//! A `done` row names its tests as `<file>::<path to the test fn>` (for example
//! `app/src/ui/links.rs::tests::discord_hosts_are_trusted`), several separated by commas. The audit runs `cargo test`
//! once and requires every named test to have passed in that run.

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
            for reference in references(row.test) {
                if test_id(reference).is_none() {
                    errors.push(format!(
                        "F-{:04} names {reference}, which is not <file>.rs::<test path>",
                        row.id
                    ));
                }
            }
        }
    }
    errors
}

fn references(cell: &str) -> impl Iterator<Item = &str> {
    cell.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

/// `app/src/ui/links.rs::tests::x` becomes `ui::links::tests::x`, the name libtest prints. A crate root (`lib.rs`,
/// `main.rs`) adds nothing to the path; `mod.rs` stands for its directory.
fn test_id(reference: &str) -> Option<String> {
    let (file, path) = reference.split_once(".rs::")?;
    let (_, inside) = file.split_once("/src/")?;
    let mut parts: Vec<&str> = inside.split('/').collect();
    let last = parts.pop()?;
    if !matches!(last, "lib" | "main" | "mod") {
        parts.push(last);
    }
    parts.push(path);
    Some(parts.join("::"))
}

/// Names of the tests that passed in one `cargo test` run over the workspace.
fn passing_tests(repo: &Path) -> Result<BTreeSet<String>> {
    let output = Command::new("cargo")
        .args([
            "test",
            "--workspace",
            "--exclude",
            "xtask",
            "--no-fail-fast",
        ])
        .current_dir(repo)
        .output()
        .context("running cargo test")?;
    verified_passing(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
    )
}

fn verified_passing(success: bool, report: &str) -> Result<BTreeSet<String>> {
    if !success {
        bail!("workspace tests failed; run cargo test --workspace to inspect the failure");
    }
    Ok(parse_passing(report))
}

fn parse_passing(report: &str) -> BTreeSet<String> {
    report
        .lines()
        .filter_map(|line| line.strip_prefix("test ")?.strip_suffix(" ... ok"))
        .map(str::to_owned)
        .collect()
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
    if let Value::Object(keybinds) = read("KEYBINDS.md")?
        && let Some(actions) = keybinds.get("actions").and_then(Value::as_array)
    {
        sources.extend(
            actions
                .iter()
                .filter_map(|action| action[0].as_str())
                .map(|name| format!("KEYBINDS:{name}")),
        );
    }
    if let Value::Object(menus) = read("CONTEXT_MENUS.md")? {
        sources.extend(menus.keys().map(|name| format!("CONTEXT_MENUS:{name}")));
    }
    if let Value::Object(settings) = read("SETTINGS.md")?
        && let Some(ids) = settings.get("ids").and_then(Value::as_array)
    {
        sources.extend(
            ids.iter()
                .filter_map(Value::as_str)
                .map(|name| format!("SETTINGS:{name}")),
        );
    }
    if let Value::Object(sounds) = read("SOUNDS.md")? {
        sources.extend(sounds.keys().map(|id| format!("SOUNDS:{id}")));
    }
    if let Value::Object(embeds) = read("EMBEDS.md")?
        && let Some(types) = embeds.get("types").and_then(Value::as_array)
    {
        sources.extend(
            types
                .iter()
                .filter_map(Value::as_str)
                .map(|name| format!("EMBEDS:{name}")),
        );
    }
    for entry in fs::read_dir(dumps.join("PROTOS"))? {
        let name = entry?.file_name().to_string_lossy().replace(".json", "");
        sources.push(format!("PROTOS:{name}"));
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

    let done_rows: Vec<&Row<'_>> = current.iter().filter(|row| row.status == "done").collect();
    if !done_rows.is_empty() {
        let passing = passing_tests(repo)?;
        for row in &done_rows {
            for reference in references(row.test) {
                if let Some(id) = test_id(reference)
                    && !passing.contains(&id)
                {
                    errors.push(format!("F-{:04}: {id} did not pass", row.id));
                }
            }
        }
    }
    let done = done_rows.len();
    println!("{} rows, {done} done", current.len());
    let missing: Vec<&str> = REQUIRED_DUMPS
        .into_iter()
        .filter(|name| {
            !dumps.join(format!("{name}.md")).exists()
                && !dumps.join(name).exists()
                && !dumps
                    .parent()
                    .is_some_and(|snapshot| snapshot.join(name).exists())
        })
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

    #[test]
    fn a_failed_test_run_cannot_certify_other_passing_tests() {
        let report = "test already_done ... ok\ntest regression ... FAILED\n";
        assert!(verified_passing(false, report).is_err());
        assert!(
            verified_passing(true, "test already_done ... ok\n")
                .unwrap()
                .contains("already_done")
        );
    }

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
    fn refuses_a_test_reference_without_a_file() {
        let errors = check_rows(&rows("| F-0001 | a | b | X:1 | done | t::x | |\n").unwrap());
        assert!(errors.iter().any(|e| e.contains("not <file>.rs")));
    }

    #[test]
    fn maps_files_to_libtest_names() {
        assert_eq!(
            test_id("app/src/ui/links.rs::tests::a").as_deref(),
            Some("ui::links::tests::a")
        );
        assert_eq!(
            test_id("crates/dc-proto/src/lib.rs::tests::b").as_deref(),
            Some("tests::b")
        );
        assert_eq!(
            test_id("app/src/ui/settings/mod.rs::tests::c").as_deref(),
            Some("ui::settings::tests::c")
        );
        let passing = parse_passing("test ui::links::tests::a ... ok\ntest x ... FAILED\n");
        assert!(passing.contains("ui::links::tests::a") && !passing.contains("x"));
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
