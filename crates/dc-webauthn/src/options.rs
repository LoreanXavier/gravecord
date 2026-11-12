//! The request options of the API (`PublicKeyCredentialRequestOptionsJSON` and `...CreationOptionsJSON`), read as
//! `PublicKeyCredential.parseRequestOptionsFromJSON` and `parseCreationOptionsFromJSON` read them (module 293731).

use crate::{Error, b64, client_data::RP_ID};
use serde_json::Value;

/// The browser's default when the options carry no timeout.
const DEFAULT_TIMEOUT_MS: u32 = 120_000;

/// A credential the relying party names (`allowCredentials` / `excludeCredentials`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub id: Vec<u8>,
    /// `WEBAUTHN_CTAP_TRANSPORT_*` bits of the `transports` listed; 0 means any.
    pub transports: u32,
}

/// `userVerification` as the system API takes it (`WEBAUTHN_USER_VERIFICATION_REQUIREMENT_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Any = 0,
    Required = 1,
    Preferred = 2,
    Discouraged = 3,
}

impl Selection {
    fn parse(text: Option<&str>) -> Self {
        match text {
            Some("required") => Self::Required,
            Some("discouraged") => Self::Discouraged,
            // "preferred" is the default of the specification.
            _ => Self::Preferred,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionRequest {
    pub rp_id: String,
    pub challenge: Vec<u8>,
    pub timeout_ms: u32,
    pub allow: Vec<Credential>,
    pub user_verification: Selection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreationRequest {
    pub rp_id: String,
    pub rp_name: String,
    pub user_id: Vec<u8>,
    pub user_name: String,
    pub user_display_name: String,
    pub challenge: Vec<u8>,
    pub timeout_ms: u32,
    /// The COSE algorithm numbers in the order the relying party prefers them.
    pub algorithms: Vec<i32>,
    pub exclude: Vec<Credential>,
    /// 0 any, 1 platform, 2 cross-platform (`WEBAUTHN_AUTHENTICATOR_ATTACHMENT_*`).
    pub attachment: u32,
    pub resident_key: bool,
    pub user_verification: Selection,
    /// `WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_*`.
    pub attestation: u32,
}

fn invalid(what: &str) -> Error {
    Error::Invalid(what.to_owned())
}

/// The document is `{"publicKey": {...}}` as the API sends it; the inner object alone is accepted too.
fn public_key(document: &str) -> Result<Value, Error> {
    let root: Value =
        serde_json::from_str(document).map_err(|error| Error::Invalid(error.to_string()))?;
    let inner = match root.get("publicKey") {
        Some(inner) => inner.clone(),
        None => root,
    };
    if inner.is_object() {
        Ok(inner)
    } else {
        Err(invalid("no publicKey object"))
    }
}

fn bytes(value: &Value, name: &str) -> Result<Vec<u8>, Error> {
    value
        .as_str()
        .and_then(b64::decode)
        .ok_or_else(|| Error::Invalid(format!("{name} is not base64url")))
}

fn timeout(options: &Value) -> u32 {
    options["timeout"]
        .as_u64()
        .map_or(DEFAULT_TIMEOUT_MS, |ms| {
            ms.clamp(1, u64::from(u32::MAX)) as u32
        })
}

fn rp_id(value: &Value) -> String {
    value
        .as_str()
        .filter(|id| !id.is_empty())
        .unwrap_or(RP_ID)
        .to_owned()
}

/// `WEBAUTHN_CTAP_TRANSPORT_USB` 1, `NFC` 2, `BLE` 4, `INTERNAL` 16, `HYBRID` 32.
fn transports(value: &Value) -> u32 {
    value.as_array().map_or(0, |list| {
        list.iter()
            .filter_map(Value::as_str)
            .map(|name| match name {
                "usb" => 1,
                "nfc" => 2,
                "ble" => 4,
                "internal" => 16,
                "hybrid" => 32,
                _ => 0,
            })
            .fold(0, |all, bit| all | bit)
    })
}

fn credentials(value: &Value) -> Result<Vec<Credential>, Error> {
    let Some(list) = value.as_array() else {
        return Ok(Vec::new());
    };
    list.iter()
        .map(|entry| {
            Ok(Credential {
                id: bytes(&entry["id"], "credential id")?,
                transports: transports(&entry["transports"]),
            })
        })
        .collect()
}

impl AssertionRequest {
    pub fn parse(document: &str) -> Result<Self, Error> {
        let options = public_key(document)?;
        Ok(Self {
            rp_id: rp_id(&options["rpId"]),
            challenge: bytes(&options["challenge"], "challenge")?,
            timeout_ms: timeout(&options),
            allow: credentials(&options["allowCredentials"])?,
            user_verification: Selection::parse(options["userVerification"].as_str()),
        })
    }
}

impl CreationRequest {
    pub fn parse(document: &str) -> Result<Self, Error> {
        let options = public_key(document)?;
        let selection = &options["authenticatorSelection"];
        let mut algorithms: Vec<i32> = options["pubKeyCredParams"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter(|entry| {
                        entry["type"]
                            .as_str()
                            .is_none_or(|kind| kind == "public-key")
                    })
                    .filter_map(|entry| entry["alg"].as_i64())
                    .filter_map(|alg| i32::try_from(alg).ok())
                    .collect()
            })
            .unwrap_or_default();
        if algorithms.is_empty() {
            // The specification's defaults when the list is empty: ES256 then RS256.
            algorithms = vec![-7, -257];
        }
        let user = &options["user"];
        Ok(Self {
            rp_id: rp_id(&options["rp"]["id"]),
            rp_name: options["rp"]["name"].as_str().unwrap_or(RP_ID).to_owned(),
            user_id: bytes(&user["id"], "user id")?,
            user_name: user["name"].as_str().unwrap_or_default().to_owned(),
            user_display_name: user["displayName"]
                .as_str()
                .or_else(|| user["name"].as_str())
                .unwrap_or_default()
                .to_owned(),
            challenge: bytes(&options["challenge"], "challenge")?,
            timeout_ms: timeout(&options),
            algorithms,
            exclude: credentials(&options["excludeCredentials"])?,
            attachment: match selection["authenticatorAttachment"].as_str() {
                Some("platform") => 1,
                Some("cross-platform") => 2,
                _ => 0,
            },
            resident_key: selection["residentKey"].as_str() == Some("required")
                || selection["requireResidentKey"].as_bool() == Some(true),
            user_verification: Selection::parse(selection["userVerification"].as_str()),
            attestation: match options["attestation"].as_str() {
                Some("none") | None => 1,
                Some("indirect") => 2,
                Some("direct") => 3,
                _ => 0,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_read_like_parse_request_options_from_json() {
        let request = AssertionRequest::parse(
            r#"{"publicKey":{"challenge":"Zm9vYmFy","timeout":60000,"rpId":"discord.com",
            "allowCredentials":[{"type":"public-key","id":"Zm9v","transports":["usb","internal"]}],
            "userVerification":"required"}}"#,
        )
        .unwrap();
        assert_eq!(request.challenge, b"foobar");
        assert_eq!(request.rp_id, "discord.com");
        assert_eq!(request.timeout_ms, 60_000);
        assert_eq!(
            request.allow,
            vec![Credential {
                id: b"foo".to_vec(),
                transports: 1 | 16
            }]
        );
        assert_eq!(request.user_verification, Selection::Required);
    }

    #[test]
    fn defaults_follow_the_specification() {
        let request = AssertionRequest::parse(r#"{"publicKey":{"challenge":"Zg"}}"#).unwrap();
        assert_eq!(request.rp_id, "discord.com");
        assert_eq!(request.timeout_ms, 120_000);
        assert!(request.allow.is_empty());
        assert_eq!(request.user_verification, Selection::Preferred);
    }

    #[test]
    fn broken_options_are_refused() {
        assert!(AssertionRequest::parse("not json").is_err());
        assert!(AssertionRequest::parse(r#"{"publicKey":{}}"#).is_err());
        assert!(AssertionRequest::parse(r#"{"publicKey":{"challenge":"a b"}}"#).is_err());
        assert!(AssertionRequest::parse("[]").is_err());
        assert!(
            AssertionRequest::parse(
                r#"{"publicKey":{"challenge":"Zg","allowCredentials":[{"id":"%%"}]}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn a_creation_is_read_with_its_selection() {
        let request = CreationRequest::parse(
            r#"{"publicKey":{"rp":{"id":"discord.com","name":"Discord"},
            "user":{"id":"AQID","name":"me","displayName":"Me"},"challenge":"Zm9v",
            "pubKeyCredParams":[{"type":"public-key","alg":-7},{"type":"public-key","alg":-257},{"type":"x","alg":5}],
            "excludeCredentials":[{"type":"public-key","id":"Zg"}],
            "authenticatorSelection":{"residentKey":"required","userVerification":"preferred"},
            "attestation":"direct"}}"#,
        )
        .unwrap();
        assert_eq!(request.user_id, vec![1, 2, 3]);
        assert_eq!(request.algorithms, vec![-7, -257]);
        assert_eq!(request.exclude.len(), 1);
        assert!(request.resident_key);
        assert_eq!(request.attestation, 3);
        assert_eq!(request.attachment, 0);
        assert_eq!(request.rp_name, "Discord");
    }

    #[test]
    fn an_empty_algorithm_list_gets_the_defaults() {
        let request = CreationRequest::parse(
            r#"{"publicKey":{"user":{"id":"AQ","name":"a"},"challenge":"Zg"}}"#,
        )
        .unwrap();
        assert_eq!(request.algorithms, vec![-7, -257]);
        assert_eq!(request.attestation, 1);
        assert_eq!(request.user_display_name, "a");
    }
}
