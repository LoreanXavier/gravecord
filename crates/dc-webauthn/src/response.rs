//! The credential JSON the official client posts back: the field set and order of module 293731 `J` (assertion) and
//! `v` (attestation), every binary field base64url without padding, an absent one left out (`undefined` in
//! `JSON.stringify`).

use crate::{b64, cose};
use serde_json::{Map, Value, json};

/// What the system API returned for an assertion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawAssertion {
    pub credential_id: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub signature: Vec<u8>,
    pub user_handle: Option<Vec<u8>>,
    /// `WEBAUTHN_CTAP_TRANSPORT_*` of the transport used, 0 when unknown.
    pub transport: u32,
}

/// What the system API returned for a new credential.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawAttestation {
    pub credential_id: Vec<u8>,
    pub attestation_object: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub transport: u32,
}

/// `authenticatorAttachment`: a built-in authenticator is "platform", any other transport "cross-platform".
fn attachment(transport: u32) -> Value {
    match transport {
        0 => Value::Null,
        t if t & 16 != 0 => json!("platform"),
        _ => json!("cross-platform"),
    }
}

/// `getTransports()`.
fn transport_names(transport: u32) -> Vec<&'static str> {
    [
        (1, "usb"),
        (2, "nfc"),
        (4, "ble"),
        (32, "hybrid"),
        (16, "internal"),
    ]
    .into_iter()
    .filter(|(bit, _)| transport & bit != 0)
    .map(|(_, name)| name)
    .collect()
}

/// `JSON.stringify` of the assertion credential.
pub fn assertion_json(raw: &RawAssertion, client_data: &str) -> String {
    let id = b64::encode(&raw.credential_id);
    let mut response = Map::new();
    response.insert(
        "authenticatorData".to_owned(),
        json!(b64::encode(&raw.authenticator_data)),
    );
    response.insert(
        "clientDataJSON".to_owned(),
        json!(b64::encode(client_data.as_bytes())),
    );
    response.insert("signature".to_owned(), json!(b64::encode(&raw.signature)));
    if let Some(handle) = &raw.user_handle
        && !handle.is_empty()
    {
        response.insert("userHandle".to_owned(), json!(b64::encode(handle)));
    }
    json!({
        "authenticatorAttachment": attachment(raw.transport),
        "clientExtensionResults": {},
        "id": id,
        "rawId": id,
        "response": Value::Object(response),
        "type": "public-key",
    })
    .to_string()
}

/// `JSON.stringify` of the attestation credential.
pub fn creation_json(raw: &RawAttestation, client_data: &str) -> String {
    let id = b64::encode(&raw.credential_id);
    let mut response = Map::new();
    response.insert(
        "attestationObject".to_owned(),
        json!(b64::encode(&raw.attestation_object)),
    );
    response.insert(
        "authenticatorData".to_owned(),
        json!(b64::encode(&raw.authenticator_data)),
    );
    response.insert(
        "clientDataJSON".to_owned(),
        json!(b64::encode(client_data.as_bytes())),
    );
    if let Some(spki) = cose::public_key_spki(&raw.authenticator_data) {
        response.insert("publicKey".to_owned(), json!(b64::encode(&spki)));
    }
    if let Some(algorithm) = cose::algorithm(&raw.authenticator_data) {
        response.insert("publicKeyAlgorithm".to_owned(), json!(algorithm));
    }
    response.insert(
        "transports".to_owned(),
        json!(transport_names(raw.transport)),
    );
    json!({
        "authenticatorAttachment": attachment(raw.transport),
        "clientExtensionResults": {},
        "id": id,
        "rawId": id,
        "response": Value::Object(response),
        "type": "public-key",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_assertion_has_the_fields_and_order_of_the_bundle() {
        let raw = RawAssertion {
            credential_id: b"foo".to_vec(),
            authenticator_data: b"ad".to_vec(),
            signature: b"sig".to_vec(),
            user_handle: Some(b"u".to_vec()),
            transport: 1,
        };
        assert_eq!(
            assertion_json(&raw, "{}"),
            r#"{"authenticatorAttachment":"cross-platform","clientExtensionResults":{},"id":"Zm9v","rawId":"Zm9v","response":{"authenticatorData":"YWQ","clientDataJSON":"e30","signature":"c2ln","userHandle":"dQ"},"type":"public-key"}"#
        );
    }

    #[test]
    fn a_missing_user_handle_is_left_out_and_an_unknown_transport_is_null() {
        let raw = RawAssertion {
            credential_id: vec![1],
            ..RawAssertion::default()
        };
        let text = assertion_json(&raw, "{}");
        assert!(!text.contains("userHandle"));
        assert!(text.starts_with(r#"{"authenticatorAttachment":null,"#));
        let empty = RawAssertion {
            user_handle: Some(Vec::new()),
            ..raw
        };
        assert!(!assertion_json(&empty, "{}").contains("userHandle"));
    }

    #[test]
    fn a_built_in_authenticator_is_a_platform_one() {
        let raw = RawAssertion {
            transport: 16,
            ..RawAssertion::default()
        };
        assert!(assertion_json(&raw, "{}").contains(r#""authenticatorAttachment":"platform""#));
    }

    #[test]
    fn a_creation_carries_the_key_the_algorithm_and_the_transports() {
        let mut authenticator_data = vec![0u8; 32];
        authenticator_data.push(0x41);
        authenticator_data.extend([0, 0, 0, 1]);
        authenticator_data.extend([0u8; 16]);
        authenticator_data.extend([0, 1, 5]);
        authenticator_data.extend([0xa4, 0x01, 0x01, 0x03, 0x27, 0x20, 0x06, 0x21, 0x58, 0x20]);
        authenticator_data.extend([7u8; 32]);
        let raw = RawAttestation {
            credential_id: vec![5],
            attestation_object: b"ao".to_vec(),
            authenticator_data,
            transport: 1 | 32,
        };
        let value: Value = serde_json::from_str(&creation_json(&raw, "{}")).unwrap();
        assert_eq!(value["id"], "BQ");
        assert_eq!(value["type"], "public-key");
        assert_eq!(value["authenticatorAttachment"], "cross-platform");
        let response = &value["response"];
        assert_eq!(response["attestationObject"], "YW8");
        assert_eq!(response["publicKeyAlgorithm"], -8);
        assert_eq!(response["transports"], json!(["usb", "hybrid"]));
        assert!(
            response["publicKey"]
                .as_str()
                .unwrap()
                .starts_with("MCowBQYDK2Vw")
        );
        let keys: Vec<&str> = response
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "attestationObject",
                "authenticatorData",
                "clientDataJSON",
                "publicKey",
                "publicKeyAlgorithm",
                "transports"
            ]
        );
    }

    #[test]
    fn a_key_the_browser_cannot_describe_has_no_public_key_field() {
        let raw = RawAttestation {
            credential_id: vec![5],
            authenticator_data: vec![0u8; 37],
            ..RawAttestation::default()
        };
        let value: Value = serde_json::from_str(&creation_json(&raw, "{}")).unwrap();
        assert!(value["response"].get("publicKey").is_none());
        assert!(value["response"].get("publicKeyAlgorithm").is_none());
        assert_eq!(value["response"]["transports"], json!([]));
    }
}
