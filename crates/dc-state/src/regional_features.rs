//! RegionalFeatureConfigStore, module 454409: the features READY says are age gated where the account connects from
//! (`CONNECTION_OPEN`'s `regionalFeatureConfig.age_gated_features`, a bit set of module 687123), with the account's
//! age verification status (`ageVerificationStatus` of the current user, module 155718 `Tk`) that module 36149 reads
//! next to it.

use serde_json::Value;

/// Module 687123: `AGE_GATED_SPACES`.
pub const AGE_GATED_SPACES: u64 = 1;
/// Module 155718 `Tk`: `VERIFIED_ADULT` and `INFERRED_ADULT`.
const VERIFIED_ADULT: u64 = 3;
const INFERRED_ADULT: u64 = 4;

#[derive(Debug, Default)]
pub struct RegionalFeatures {
    /// `null` when READY carried no `regional_feature_config`.
    age_gated_features: Option<u64>,
    age_verification_status: Option<u64>,
}

impl RegionalFeatures {
    /// `isFeatureAgeGated`: false before READY or without a config.
    pub fn is_feature_age_gated(&self, feature: u64) -> bool {
        self.age_gated_features
            .is_some_and(|features| features & feature != 0)
    }

    /// Module 36149 `$8`: the account is a verified or inferred adult.
    pub fn is_adult(&self) -> bool {
        matches!(
            self.age_verification_status,
            Some(VERIFIED_ADULT | INFERRED_ADULT)
        )
    }

    /// Module 597306 `r`: age gated spaces apply here and the account is not known to be an adult (`u5`).
    pub fn age_gated_spaces(&self) -> bool {
        self.is_feature_age_gated(AGE_GATED_SPACES) && !self.is_adult()
    }

    pub fn dispatch(&mut self, event: &str, data: &Value) {
        match event {
            // `CONNECTION_OPEN`: the config is replaced, or cleared when READY has none.
            "READY" => {
                self.age_gated_features =
                    data["regional_feature_config"]["age_gated_features"].as_u64();
                self.age_verification_status = data["user"]["age_verification_status"].as_u64();
            }
            // `CURRENT_USER_UPDATE`: the account's own record.
            "USER_UPDATE" => {
                if let Some(status) = data["age_verification_status"].as_u64() {
                    self.age_verification_status = Some(status);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RegionalFeatures;
    use serde_json::json;

    #[test]
    fn spaces_are_gated_where_the_bit_is_set_for_an_account_not_known_adult() {
        let mut store = RegionalFeatures::default();
        assert!(!store.age_gated_spaces());
        store.dispatch(
            "READY",
            &json!({"regional_feature_config": {"age_gated_features": 687}, "user": {"age_verification_status": 1}}),
        );
        assert!(store.age_gated_spaces());
        store.dispatch("USER_UPDATE", &json!({"age_verification_status": 4}));
        assert!(!store.age_gated_spaces());
        store.dispatch(
            "READY",
            &json!({"regional_feature_config": {"age_gated_features": 686}, "user": {}}),
        );
        assert!(!store.age_gated_spaces());
        store.dispatch("READY", &json!({"user": {"age_verification_status": 1}}));
        assert!(!store.age_gated_spaces());
    }
}
