//! LocationMetadataStore, module 121780: the country READY reports for the connection (`CONNECTION_OPEN`'s
//! `countryCode`, READY's `country_code`) and the minimum age `SET_LOCATION_METADATA` hands over.
//!
//! The official store keeps the country as the entry of its country list (`XF` of module 956956, found by `alpha2`),
//! and falls back to the United States entry (`rE`) when the code names no entry. A missing code leaves the stored
//! country as it was. Here the entry is kept as its `alpha2`; the country list lives in the app, so a code is an entry
//! when it is two letters, and anything else takes the fallback.

use serde_json::Value;

/// `rE` of module 956956: the default entry, "United States".
pub const DEFAULT_COUNTRY: &str = "US";

#[derive(Debug, Default)]
pub struct LocationMetadataStore {
    country_code: Option<String>,
    minimum_age: Option<u64>,
}

/// `o` of module 121780: true when the event carried a code (the store then changes).
fn country_of(code: &Value) -> Option<String> {
    if code.is_null() {
        return None;
    }
    let alpha2 = code
        .as_str()
        .filter(|code| code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase);
    Some(alpha2.unwrap_or_else(|| DEFAULT_COUNTRY.to_owned()))
}

impl LocationMetadataStore {
    /// `getCountryCode`: the `alpha2` of the connection's country, `None` before any READY carried one.
    pub fn get_country_code(&self) -> Option<&str> {
        self.country_code.as_deref()
    }

    /// `getMinimumAge`.
    pub fn get_minimum_age(&self) -> Option<u64> {
        self.minimum_age
    }

    pub fn dispatch(&mut self, event: &str, data: &Value) {
        match event {
            "READY" => {
                if let Some(country) = country_of(&data["country_code"]) {
                    self.country_code = Some(country);
                }
            }
            "SET_LOCATION_METADATA" => {
                let code = if data["countryCode"].is_null() {
                    &data["country_code"]
                } else {
                    &data["countryCode"]
                };
                if let Some(country) = country_of(code) {
                    self.country_code = Some(country);
                }
                let age = if data["minimumAge"].is_null() {
                    &data["minimum_age"]
                } else {
                    &data["minimumAge"]
                };
                self.minimum_age = age.as_u64();
            }
            "LOGOUT" => self.minimum_age = None,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn ready_carries_the_country_and_a_missing_code_keeps_it() {
        let mut stores = crate::Stores::default();
        assert_eq!(stores.location_metadata.get_country_code(), None);
        stores.dispatch("READY", &json!({"country_code": "GB"}));
        assert_eq!(stores.location_metadata.get_country_code(), Some("GB"));
        // A READY without the field (`countryCode` undefined) changes nothing.
        stores.dispatch("READY", &json!({}));
        assert_eq!(stores.location_metadata.get_country_code(), Some("GB"));
        stores.dispatch("READY", &json!({"country_code": "au"}));
        assert_eq!(stores.location_metadata.get_country_code(), Some("AU"));
        // A code that names no country falls back to the United States entry.
        stores.dispatch("READY", &json!({"country_code": "nowhere"}));
        assert_eq!(stores.location_metadata.get_country_code(), Some("US"));
    }

    #[test]
    fn location_metadata_sets_the_minimum_age_until_logout() {
        let mut stores = crate::Stores::default();
        stores.dispatch(
            "SET_LOCATION_METADATA",
            &json!({"countryCode": "BR", "minimumAge": 16}),
        );
        assert_eq!(stores.location_metadata.get_country_code(), Some("BR"));
        assert_eq!(stores.location_metadata.get_minimum_age(), Some(16));
        stores.dispatch("SET_LOCATION_METADATA", &json!({"minimumAge": null}));
        assert_eq!(stores.location_metadata.get_minimum_age(), None);
        assert_eq!(stores.location_metadata.get_country_code(), Some("BR"));
        stores.dispatch("SET_LOCATION_METADATA", &json!({"minimumAge": 13}));
        stores.dispatch("LOGOUT", &json!({}));
        assert_eq!(stores.location_metadata.get_minimum_age(), None);
        assert_eq!(stores.location_metadata.get_country_code(), Some("BR"));
    }
}
