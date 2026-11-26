//! Experiments (Vencord `src/plugins/experiments`, GPL-3.0-or-later): the official client's Experiments page, which it
//! shows to developers only (the "Staff Only" section of module 158935, `usePredicate: isDeveloper`). The page lists
//! every experiment the client registers (`experiments_table.rs`, from the pinned modules), overridden ones first,
//! then the newest; a search keeps the ones whose text matches the most words (module 96919); each one opens to its
//! current assignment and an override of its bucket (module 271478), which the client keeps across restarts. Not part
//! of the pinned client.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// `kind` of an experiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    User,
    Guild,
}

/// `system`: the legacy experiment store (module 343446) or the apex one (module 50258).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum System {
    Legacy,
    Apex,
}

impl System {
    pub(crate) fn word(self) -> &'static str {
        match self {
            System::Legacy => "legacy",
            System::Apex => "apex",
        }
    }
}

/// One registered experiment, as the page's rows read it (modules 32523 and 386976).
#[derive(Debug)]
pub(crate) struct Definition {
    pub name: &'static str,
    pub title: &'static str,
    pub kind: Kind,
    pub system: System,
    /// The override's options: the bucket or variant and its label.
    pub buckets: &'static [(i64, &'static str)],
}

pub(crate) use super::experiments_table::DEFINITIONS;

pub(crate) fn definition(name: &str) -> Option<&'static Definition> {
    DEFINITIONS
        .iter()
        .find(|definition| definition.name == name)
}

/// The overrides set on the page, by experiment name: a legacy experiment's bucket (`EXPERIMENT_OVERRIDE_BUCKET`) or
/// an apex one's variant (`APEX_EXPERIMENT_OVERRIDE_CREATE`).
pub(crate) type Overrides = BTreeMap<String, i64>;

/// `{"<name>": <bucket>, ...}`; anything else in the stored value is left out.
pub(crate) fn overrides_from(value: &Value) -> Overrides {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, bucket)| Some((name.clone(), bucket.as_i64()?)))
        .collect()
}

pub(crate) fn overrides_value(overrides: &Overrides) -> Value {
    Value::Object(
        overrides
            .iter()
            .map(|(name, bucket)| (name.clone(), json!(bucket)))
            .collect::<Map<_, _>>(),
    )
}

/// Module 96919 `s`: the `YYYY-M` an experiment's name starts with.
fn date_of(name: &str) -> Option<&str> {
    let bytes = name.as_bytes();
    if bytes.len() < 6 || !bytes[..4].iter().all(u8::is_ascii_digit) || bytes[4] != b'-' {
        return None;
    }
    let month = bytes[5..]
        .iter()
        .take(2)
        .take_while(|b| b.is_ascii_digit())
        .count();
    (month > 0).then(|| &name[..5 + month])
}

/// Module 96919 `R3`: overridden experiments first, then the newer `YYYY-M` first, then by title.
pub(crate) fn sorted(overrides: &Overrides) -> Vec<&'static Definition> {
    let mut list: Vec<&'static Definition> = DEFINITIONS.iter().collect();
    list.sort_by(|a, b| {
        let (over_a, over_b) = (
            overrides.contains_key(a.name),
            overrides.contains_key(b.name),
        );
        over_b
            .cmp(&over_a)
            .then_with(|| match (date_of(a.name), date_of(b.name)) {
                (Some(left), Some(right)) => right.cmp(left),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    list
}

/// The words an experiment's row is searched in: every string of the official row's object (its id, system, kind,
/// name, title and its options' labels and short labels).
fn texts(definition: &Definition) -> Vec<String> {
    let mut out = vec![
        definition.name.to_owned(),
        definition.system.word().to_owned(),
        match definition.kind {
            Kind::User => "user".to_owned(),
            Kind::Guild => "guild".to_owned(),
        },
        definition.title.to_owned(),
    ];
    for (bucket, label) in definition.buckets {
        out.push((*label).to_owned());
        out.push(short_label(definition.system, *bucket));
    }
    out
}

/// The option's short label: `getExperimentBucketName` of a legacy bucket, `Variant N` of an apex one.
pub(crate) fn short_label(system: System, bucket: i64) -> String {
    match (system, bucket) {
        (System::Apex, _) => format!("Variant {bucket}"),
        (System::Legacy, 0) => "Control".to_owned(),
        (System::Legacy, -1) => "Not Eligible".to_owned(),
        (System::Legacy, _) => format!("Treatment {bucket}"),
    }
}

/// Module 96919 `oC`: the experiments that match a word of the query (any of their texts contains it, without regard
/// to case), the ones matching the most words first; the whole list for an empty query.
pub(crate) fn search(list: Vec<&'static Definition>, query: &str) -> Vec<&'static Definition> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return list;
    }
    let mut groups: BTreeMap<usize, Vec<&'static Definition>> = BTreeMap::new();
    for definition in list {
        let texts: Vec<String> = texts(definition)
            .iter()
            .map(|text| text.to_lowercase())
            .collect();
        let count = words
            .iter()
            .filter(|word| texts.iter().any(|text| text.contains(word.as_str())))
            .count();
        if count > 0 {
            groups.entry(count).or_default().push(definition);
        }
    }
    groups.into_values().rev().flatten().collect()
}

/// The guild row's description (module 158935 `Sh`): `N guilds in bucket B`, by bucket, joined with commas.
pub(crate) fn guild_summary(buckets: &[i64]) -> String {
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for bucket in buckets {
        *counts.entry(*bucket).or_default() += 1;
    }
    counts
        .iter()
        .map(|(bucket, count)| format!("{count} guilds in bucket {bucket}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_both_systems_and_their_options() {
        let badge = definition("2026-03_voice_badge").unwrap();
        assert_eq!(badge.kind, Kind::Guild);
        assert_eq!(badge.system, System::Legacy);
        assert_eq!(badge.buckets[0], (-1, "Not Eligible"));
        assert_eq!(badge.buckets[1], (0, "Treatment 0: Control"));
        assert!(
            DEFINITIONS
                .iter()
                .filter(|d| d.system == System::Apex)
                .count()
                > 200
        );
        assert!(DEFINITIONS.iter().all(|d| !d.buckets.is_empty()));
    }

    #[test]
    fn overridden_and_newer_come_first() {
        let mut overrides = Overrides::new();
        assert!(date_of(sorted(&overrides)[0].name).is_some());
        overrides.insert("2021-02_staff_qc_debugging".to_owned(), 1);
        let list = sorted(&overrides);
        assert_eq!(list[0].name, "2021-02_staff_qc_debugging");
        let dates: Vec<&str> = list[1..].iter().filter_map(|d| date_of(d.name)).collect();
        assert!(dates.windows(2).all(|pair| pair[0] >= pair[1]));
        assert_eq!(date_of("2026-3_x"), Some("2026-3"));
        assert_eq!(date_of("x"), None);
    }

    #[test]
    fn search_ranks_by_matched_words() {
        let list = search(sorted(&Overrides::new()), "voice badge");
        assert_eq!(list[0].name, "2026-03_voice_badge");
        assert!(list.iter().any(|d| d.name == "2022-07_voice_in_threads"));
        assert!(search(sorted(&Overrides::new()), "zzzz-nothing").is_empty());
        assert_eq!(
            search(sorted(&Overrides::new()), "  ").len(),
            DEFINITIONS.len()
        );
    }

    #[test]
    fn overrides_round_trip_and_summaries_count() {
        let mut overrides = Overrides::new();
        overrides.insert("a".to_owned(), 2);
        assert_eq!(overrides_from(&overrides_value(&overrides)), overrides);
        assert!(overrides_from(&json!({"a": "x"})).is_empty());
        assert_eq!(
            guild_summary(&[1, -1, 1]),
            "1 guilds in bucket -1, 2 guilds in bucket 1"
        );
        assert_eq!(short_label(System::Legacy, 0), "Control");
        assert_eq!(short_label(System::Apex, 3), "Variant 3");
    }
}
