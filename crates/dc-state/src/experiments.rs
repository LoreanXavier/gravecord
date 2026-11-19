//! The account's experiment assignments (`ApexExperimentStore`, module 50258), as READY's `apex_experiments` hands them
//! over: `assignments` keyed by unit type (`"1"` user, `"2"` installation), then by unit id, each a list of
//! `[hashedName, variantId, flags, revision, trackedVariantId, config]`. An experiment is looked up by the murmur3 (x86,
//! 32 bit, seed 0) hash of its name, `r().v3(name)` in the bundle.
//!
//! `getConfig` (module 400845) returns the experiment's default when the account has no assignment, or when the
//! assignment only serves as eligibility for a dependent experiment (`flags & 8`); otherwise it is the variation the
//! assignment names. Only user-kind experiments are read here.

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
