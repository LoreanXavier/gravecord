//! The client data a browser builds for the relying party: the system API hashes these exact bytes and the server
//! checks `type`, the challenge and the origin in them.

use crate::b64;
use serde_json::json;

/// `GLOBAL_ENV.WEBAUTHN_ORIGIN` of the pinned build: the relying party id.
pub const RP_ID: &str = "discord.com";
/// The origin of the page the official client signs for.
pub const ORIGIN: &str = "https://discord.com";

/// `{"type", "challenge", "origin", "crossOrigin"}` in the order Chromium writes them; the challenge is the
/// base64url of its bytes, as the options carried it.
pub fn client_data_json(kind: &str, challenge: &[u8]) -> String {
    json!({
        "type": kind,
        "challenge": b64::encode(challenge),
        "origin": ORIGIN,
        "crossOrigin": false,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_data_is_what_a_browser_writes() {
        assert_eq!(
            client_data_json("webauthn.get", b"foobar"),
            r#"{"type":"webauthn.get","challenge":"Zm9vYmFy","origin":"https://discord.com","crossOrigin":false}"#
        );
        assert_eq!(
            client_data_json("webauthn.create", &[0xfb, 0xff]),
            r#"{"type":"webauthn.create","challenge":"-_8","origin":"https://discord.com","crossOrigin":false}"#
        );
    }
}
