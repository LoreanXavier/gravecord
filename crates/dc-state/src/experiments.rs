//! The account's experiment assignments (`ApexExperimentStore`, module 50258), as READY's `apex_experiments` hands them
//! over: `assignments` keyed by unit type (`"1"` user, `"2"` installation), then by unit id, each a list of
//! `[hashedName, variantId, flags, revision, trackedVariantId, config]`. An experiment is looked up by the murmur3 (x86,
//! 32 bit, seed 0) hash of its name, `r().v3(name)` in the bundle.
//!
//! `getConfig` (module 400845) returns the experiment's default when the account has no assignment, or when the
//! assignment only serves as eligibility for a dependent experiment (`flags & 8`); otherwise it is the variation the
//! assignment names. `GuildExperiments` evaluates READY's legacy `guild_experiments` per guild (module 343446).

use serde_json::Value;
use std::collections::HashMap;

/// `ApexExperimentStore.getHash`: murmur3 x86 32 bit with seed 0 over the name's UTF-8 bytes (the names are ASCII).
pub fn hash(name: &str) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    let data = name.as_bytes();
    let mut h: u32 = 0;
    let (blocks, tail) = data.as_chunks::<4>();
    for block in blocks {
        let mut k = u32::from_le_bytes(*block);
        k = k.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
        h ^= k;
        h = h.rotate_left(13).wrapping_mul(5).wrapping_add(0xe654_6b64);
    }
    if !tail.is_empty() {
        let mut k: u32 = 0;
        for (index, byte) in tail.iter().enumerate() {
            k ^= u32::from(*byte) << (8 * index);
        }
        k = k.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
        h ^= k;
    }
    h ^= data.len() as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

/// `ExperimentFlags.UseAsEligibility`: the assignment only gates a dependent experiment.
const USE_AS_ELIGIBILITY: u64 = 8;

/// One account's user-kind assignments: hashed name to `(variantId, flags)`; and the legacy user experiments of
/// READY's `experiments` (`ExperimentStore`, `[hash, revision, bucket, override, population, ...]`): hashed name to
/// bucket.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assignments(HashMap<u32, (i64, u64)>, HashMap<u32, i64>);

impl Assignments {
    /// From READY's `apex_experiments` for this user. Anything else in it is ignored.
    pub fn from_ready(apex_experiments: &Value, user_id: &str) -> Self {
        let list = apex_experiments["assignments"]["1"][user_id]["assignments"].as_array();
        Self(
            list.into_iter()
                .flatten()
                .filter_map(|entry| {
                    let entry = entry.as_array()?;
                    Some((
                        u32::try_from(entry.first()?.as_u64()?).ok()?,
                        (entry.get(1)?.as_i64()?, entry.get(2)?.as_u64().unwrap_or(0)),
                    ))
                })
                .collect(),
            HashMap::new(),
        )
    }

    /// Adds READY's legacy user experiments (`experiments`): each `[hash, revision, bucket, ...]`.
    pub fn with_legacy(mut self, experiments: &Value) -> Self {
        self.1 = experiments
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let entry = entry.as_array()?;
                Some((
                    u32::try_from(entry.first()?.as_u64()?).ok()?,
                    entry.get(2)?.as_i64()?,
                ))
            })
            .collect();
        self
    }

    /// The bucket of a legacy user experiment (`getUserExperimentBucket`), `None` without an assignment (the
    /// experiment's `defaultConfig`).
    pub fn legacy_bucket(&self, name: &str) -> Option<i64> {
        self.1.get(&hash(name)).copied()
    }

    /// The variation the account is in, or `None` when `getConfig` would return the experiment's default.
    pub fn variant(&self, name: &str) -> Option<i64> {
        self.0
            .get(&hash(name))
            .filter(|(_, flags)| flags & USE_AS_ELIGIBILITY == 0)
            .map(|(variant, _)| *variant)
    }

    /// Puts the account in a variation by hand, as the client's bucket override does (harness only).
    pub fn force(&mut self, name: &str, variant: i64) {
        self.0.insert(hash(name), (variant, 0));
    }

    /// Puts the account in a legacy experiment's bucket by hand (harness only).
    pub fn force_legacy(&mut self, name: &str, bucket: i64) {
        self.1.insert(hash(name), bucket);
    }
}

/// `ExperimentBuckets.NOT_ELIGIBLE` and `.CONTROL` (module 688151).
pub const NOT_ELIGIBLE: i64 = -1;
pub const CONTROL: i64 = 0;

/// What a guild experiment's filters read of a guild (module 343446 `L`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuildFacts<'a> {
    pub id: u64,
    pub member_count: Option<u64>,
    pub features: &'a [String],
    pub hub_type: Option<i64>,
    pub has_vanity_url: bool,
    /// Unix milliseconds now, for `guild_age_range_days`.
    pub now_ms: u64,
}

/// One filter of a population (`ee` of module 343446), keyed by the hash of its name.
#[derive(Clone, Debug, PartialEq)]
enum Filter {
    Ids(Vec<u64>),
    IdRange(Option<u64>, Option<u64>),
    AgeDays(Option<u64>, Option<u64>),
    MemberCount(Option<u64>, Option<u64>),
    Feature(Vec<String>),
    HubType(Vec<i64>),
    Vanity(bool),
    InRangeByHash(String, u64),
    /// A filter this build does not know: `ee` maps it to null, which passes.
    Unknown,
}

/// `O` of module 343446: a value within `[min, max]`, either side open when unset.
fn within(value: u64, min: Option<u64>, max: Option<u64>) -> bool {
    !min.is_some_and(|min| value < min) && !max.is_some_and(|max| value > max)
}

fn number(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

impl Filter {
    fn parse(entry: &Value) -> Self {
        let (Some(kind), Some(fields)) = (entry[0].as_u64(), entry[1].as_array()) else {
            return Self::Unknown;
        };
        let fields: Vec<(u64, &Value)> = fields
            .iter()
            .filter_map(|field| Some((field[0].as_u64()?, &field[1])))
            .collect();
        let field = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| *key == u64::from(hash(name)))
                .map(|(_, value)| *value)
        };
        let range = || {
            (
                field("min_id").and_then(number),
                field("max_id").and_then(number),
            )
        };
        let first = || fields.first().map(|(_, value)| *value);
        let strings = |value: Option<&Value>| -> Vec<String> {
            value
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        };
        let kind = u32::try_from(kind).unwrap_or(0);
        if kind == hash("guild_ids") {
            Self::Ids(
                field("guild_ids")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(number)
                    .collect(),
            )
        } else if kind == hash("guild_id_range") {
            let (min, max) = range();
            Self::IdRange(min, max)
        } else if kind == hash("guild_age_range_days") {
            let (min, max) = range();
            Self::AgeDays(min, max)
        } else if kind == hash("guild_member_count_range") {
            let (min, max) = range();
            Self::MemberCount(min, max)
        } else if kind == hash("guild_has_feature") {
            Self::Feature(strings(first()))
        } else if kind == hash("guild_hub_types") {
            Self::HubType(
                first()
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_i64)
                    .collect(),
            )
        } else if kind == hash("guild_has_vanity_url") {
            Self::Vanity(first().and_then(Value::as_bool).unwrap_or(false))
        } else if kind == hash("guild_in_range_by_hash") {
            Self::InRangeByHash(
                field("hash_key")
                    .map(|key| match key {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default(),
                field("target").and_then(number).unwrap_or(0),
            )
        } else {
            Self::Unknown
        }
    }

    fn passes(&self, guild: &GuildFacts) -> bool {
        match self {
            Self::Ids(ids) => ids.contains(&guild.id),
            Self::IdRange(min, max) => within(guild.id, *min, *max),
            Self::AgeDays(min, max) => {
                let made = (guild.id >> 22) + 1_420_070_400_000;
                within(guild.now_ms.saturating_sub(made) / 86_400_000, *min, *max)
            }
            Self::MemberCount(min, max) => guild
                .member_count
                .is_some_and(|count| within(count, *min, *max)),
            Self::Feature(features) => features
                .iter()
                .any(|feature| guild.features.contains(feature)),
            Self::HubType(types) => guild.hub_type.is_some_and(|kind| types.contains(&kind)),
            Self::Vanity(wanted) => *wanted == guild.has_vanity_url,
            Self::InRangeByHash(key, target) => {
                u64::from(hash(&format!("{key}:{}", guild.id))) % 10_000 < *target
            }
            Self::Unknown => true,
        }
    }
}

/// A population (`et`): buckets with their hash positions, and the filters a guild must pass.
#[derive(Clone, Debug, Default, PartialEq)]
struct Population {
    buckets: Vec<(i64, Vec<(u64, u64)>)>,
    filters: Vec<Filter>,
}

impl Population {
    fn parse(entry: &Value) -> Self {
        Self {
            buckets: entry[0]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|bucket| {
                    Some((
                        bucket[0].as_i64()?,
                        bucket[1]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|range| Some((range["s"].as_u64()?, range["e"].as_u64()?)))
                            .collect(),
                    ))
                })
                .collect(),
            filters: entry[1]
                .as_array()
                .into_iter()
                .flatten()
                .map(Filter::parse)
                .collect(),
        }
    }
}

/// `er`: the first population whose filters pass decides: the bucket whose positions hold the hash, else control;
/// not eligible there ends the search with none.
fn bucket_of(populations: &[Population], guild: &GuildFacts, position: u64) -> Option<i64> {
    for population in populations {
        if !population.filters.iter().all(|filter| filter.passes(guild)) {
            continue;
        }
        let bucket = population
            .buckets
            .iter()
            .find(|(_, ranges)| {
                ranges
                    .iter()
                    .any(|(start, end)| position >= *start && position < *end)
            })
            .map_or(CONTROL, |(bucket, _)| *bucket);
        return (bucket != NOT_ELIGIBLE).then_some(bucket);
    }
    None
}

/// One guild experiment of READY's `guild_experiments` (`ei`): `[hash, hashKey, revision, populations, overrides,
/// overridesFormatted, holdoutName, holdoutControlBucket, aaMode, triggerDebugging]`.
#[derive(Clone, Debug, Default, PartialEq)]
struct GuildExperiment {
    hash_key: Option<String>,
    populations: Vec<Population>,
    /// `{b, k}`: the bucket of each listed guild.
    overrides: HashMap<u64, i64>,
    overrides_formatted: Vec<Vec<Population>>,
    holdout: Option<(String, i64)>,
    aa_mode: bool,
}

/// `ExperimentStore`'s guild experiments (module 343446), evaluated per guild as `getGuildExperimentDescriptor` does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuildExperiments(HashMap<u32, GuildExperiment>);

impl GuildExperiments {
    pub fn from_ready(list: &Value) -> Self {
        let populations = |value: &Value| -> Vec<Population> {
            value
                .as_array()
                .into_iter()
                .flatten()
                .map(Population::parse)
                .collect()
        };
        Self(
            list.as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    let key = u32::try_from(entry[0].as_u64()?).ok()?;
                    let mut overrides = HashMap::new();
                    for item in entry[4].as_array().into_iter().flatten() {
                        let Some(bucket) = item["b"].as_i64() else {
                            continue;
                        };
                        for guild in item["k"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(number)
                        {
                            overrides.insert(guild, bucket);
                        }
                    }
                    Some((
                        key,
                        GuildExperiment {
                            hash_key: entry[1].as_str().map(str::to_owned),
                            populations: populations(&entry[3]),
                            overrides,
                            overrides_formatted: entry[5]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(populations)
                                .collect(),
                            holdout: entry[6].as_str().map(str::to_owned).zip(entry[7].as_i64()),
                            aa_mode: entry[8].as_i64() == Some(1),
                        },
                    ))
                })
                .collect(),
        )
    }

    /// `getGuildExperimentBucket`: the guild's bucket, `None` when it is not eligible (the experiment's default
    /// config). An `aaMode` experiment reads as control: its config stays the default whatever the bucket.
    pub fn bucket(&self, name: &str, guild: &GuildFacts) -> Option<i64> {
        self.descriptor(name, guild, 0)
    }

    fn descriptor(&self, name: &str, guild: &GuildFacts, depth: u8) -> Option<i64> {
        let experiment = self.0.get(&hash(name))?;
        if let Some(bucket) = experiment.overrides.get(&guild.id) {
            return (*bucket != NOT_ELIGIBLE).then_some(*bucket);
        }
        let key = experiment.hash_key.as_deref().unwrap_or(name);
        let position = u64::from(hash(&format!("{key}:{}", guild.id))) % 10_000;
        for populations in &experiment.overrides_formatted {
            if let Some(bucket) = bucket_of(populations, guild, position) {
                return Some(bucket);
            }
        }
        let bucket = bucket_of(&experiment.populations, guild, position)?;
        // A guild in its holdout's control bucket is held out of this experiment.
        if let Some((holdout, control)) = &experiment.holdout
            && holdout != name
            && depth < 4
            && self.descriptor(holdout, guild, depth + 1) == Some(*control)
        {
            return None;
        }
        Some(if experiment.aa_mode { CONTROL } else { bucket })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `2026-04_voice_user_duration` as the recorded READY carries it: guilds up to an id and with at most 200
    /// members, buckets by position, and an override population for staff servers.
    #[test]
    fn guild_experiments_read_as_get_guild_experiment_descriptor() {
        let list = json!([[4294382796u64, null, 0, [[[[-1, [{"e": 10000, "s": 2000}]], [1, [{"e": 200, "s": 100},
            {"e": 2000, "s": 1100}]]], [[2404720969u64, [[3399957344u64, null], [1238858341u64,
            "1497272116444135424"]]], [2918402255u64, [[3399957344u64, 0], [1238858341u64, 200]]]]]], [],
            [[[[[1, [{"e": 10000, "s": 0}]]], [[1604612045u64, [[1183251248u64, ["INTERNAL_EMPLOYEE_ONLY"]]]]]]]],
            null, null, 0, 1]]);
        let store = GuildExperiments::from_ready(&list);
        let name = "2026-04_voice_user_duration";
        let facts = |id: u64, members: u64, features: &[String]| -> Option<i64> {
            store.bucket(
                name,
                &GuildFacts {
                    id,
                    member_count: Some(members),
                    features,
                    ..GuildFacts::default()
                },
            )
        };
        // Position 1471: bucket 1.
        assert_eq!(facts(1_454_000_000_000_000_000, 50, &[]), Some(1));
        // Too many members: no population passes.
        assert_eq!(facts(1_454_000_000_000_000_000, 500, &[]), None);
        // Position 5267: the not-eligible range.
        assert_eq!(facts(1_300_000_000_000_000_000, 50, &[]), None);
        // A newer guild than the range allows (the fixture guild, position 4795).
        assert_eq!(facts(1_568_692_564_637_524_109, 3, &[]), None);
        // The staff override population wins first.
        let staff = [String::from("INTERNAL_EMPLOYEE_ONLY")];
        assert_eq!(facts(1_568_692_564_637_524_109, 3, &staff), Some(1));
        assert_eq!(
            store.bucket("2026-03_voice_badge", &GuildFacts::default()),
            None
        );
    }

    #[test]
    fn names_hash_as_the_bundle_does() {
        // Seen in the recorded READY (TRAFFIC/identify.json): this experiment's assignment is under this hash.
        assert_eq!(hash("2026-09-halloween-holiday"), 3_337_073_076);
        assert_eq!(hash(""), 0);
        // Reference vectors of murmur3 x86_32, seed 0.
        assert_eq!(hash("a"), 0x3c25_69b2);
        assert_eq!(hash("abcd"), 0x43ed_676a);
        assert_eq!(hash("Hello, world!"), 0xc036_3e43);
    }

    #[test]
    fn legacy_user_experiments_are_read_by_bucket() {
        // The recorded READY's `experiments` rows: `[hash, revision, bucket, override, population, ...]`.
        let legacy = json!([
            [3_013_881_204u32, 5, 11, -1, 1, 3421, 0, 0, null, null, null],
            [
                hash("2021-12_inferno_spam_redaction"),
                0,
                1,
                -1,
                2,
                9531,
                0,
                0,
                null,
                null,
                null
            ]
        ]);
        let assignments = Assignments::default().with_legacy(&legacy);
        assert_eq!(hash("2021-12_inferno_spam_redaction"), 4_179_344_527);
        assert_eq!(
            assignments.legacy_bucket("2021-12_inferno_spam_redaction"),
            Some(1)
        );
        assert_eq!(assignments.legacy_bucket("never-assigned"), None);
    }

    #[test]
    fn a_variation_is_read_from_ready_and_eligibility_flags_hide_it() {
        let name = "2026-09-halloween-holiday";
        let ready = json!({"assignments": {"1": {"42": {"assignments": [
            [hash(name), 1, 0, 1],
            [hash("another-one"), 2, 8, 3],
            [7, 0, 0, 1, 1]
        ]}}}});
        let assignments = Assignments::from_ready(&ready, "42");
        assert_eq!(assignments.variant(name), Some(1));
        assert_eq!(assignments.variant("another-one"), None);
        assert_eq!(assignments.variant("never-assigned"), None);
        assert_eq!(
            Assignments::from_ready(&ready, "43").variant(name),
            None,
            "another user's list is not ours"
        );
        assert_eq!(
            Assignments::from_ready(&json!(null), "42").variant(name),
            None
        );
    }
}
