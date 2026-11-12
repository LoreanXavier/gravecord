//! Multi-Factor Authentication: the Enable Authenticator App dialog (module 333737) with its secret, QR code and
//! code field, the backup codes (module 549404 and the `by` panel of 158935), and the requests of module 98207.
//! Nothing here keeps a secret longer than the dialog that shows it.

use super::account_dialogs::Call;
use crate::net::Request;
use serde_json::{Value, json};

/// The key the answers of these requests come back under.
pub(in crate::ui) const MFA_KEY: &str = "settings-mfa";
/// The security keys list, a plain GET.
pub(in crate::ui) const CREDENTIALS_KEY: &str = "settings-mfa-credentials";
/// Where the backup codes of the account (and what regenerating them needs) are kept: the official `MFABackupCodesStore`.
pub(in crate::ui) const CODES_KEY: &str = "mfa-backup-codes";
/// Present while the SMS phone number shows in full (the panel's "Reveal").
pub(in crate::ui) const REVEAL_KEY: &str = "mfa-sms-reveal";

/// `_.nhx.MFA_SMS` (module 652215): the user flag of SMS authentication.
pub(in crate::ui) const SMS_FLAG: u64 = 16;

/// What a dialog of this flow keeps while it is open: the secret it shows, the password that asked for the emailed
/// key (for "Resend it"), the key typed, and the nonces of the key request.
#[derive(Default)]
pub(in crate::ui) struct Draft {
    pub secret: String,
    pub password: String,
    pub key: String,
    pub view_nonce: String,
    pub regenerate_nonce: String,
    pub codes: Vec<(String, bool)>,
    /// The question before the "2FA is activated" dialog closes without a download is showing.
    pub skip_ask: bool,
    /// The "Verification email resent" line is showing.
    pub resent: bool,
    /// Security key registration (module 245329): the ticket and challenge of the start request, the credential the
    /// system prompt returned, whether that prompt is open, and the sentence the register step shows after a failure.
    pub ticket: String,
    pub challenge: String,
    pub credential: String,
    pub waiting: bool,
    pub error: Option<String>,
    /// The key a rename or a removal is about, and its name now.
    pub key_id: String,
    pub key_name: String,
}

impl Draft {
    /// `MFA_SEND_VERIFICATION_KEY`: `{nonce, regenerate_nonce}` of the answer.
    pub fn set_nonces(&mut self, answer: &Value) {
        self.view_nonce = answer["nonce"].as_str().unwrap_or_default().to_owned();
        self.regenerate_nonce = answer["regenerate_nonce"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
    }
}

/// Whether the backup codes were downloaded or the question about skipping them was confirmed since the app started:
/// the official `MFABackupCodesStore.hasSeenBackupPrompt`, a store that is not persisted.
static SEEN_PROMPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `MFA_SEEN_BACKUP_CODE_PROMPT`.
pub(in crate::ui) fn mark_prompt_seen() {
    SEEN_PROMPT.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Capture only: the next run of the dialog starts unseen again.
#[cfg(feature = "capture")]
pub(in crate::ui) fn forget_prompt_seen() {
    SEEN_PROMPT.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub(in crate::ui) fn prompt_seen() -> bool {
    SEEN_PROMPT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The removal question of the Authenticator App (`bn` in module 158935): `bt.k("totp")` of module 705147 is true
/// while the account has no security key, and then `xlA7Tl` (the warning about support never writing first) is the
/// text; with a security key it is the short `EA4ZEk`.
pub(in crate::ui) fn remove_text(security_keys: usize) -> &'static str {
    if security_keys == 0 {
        "Heads up, removing your Authenticator App means your account will no longer require Multi-Factor Authentication to sign in. If you have not reached out to Discord support for help with removing your Authenticator App and someone reached out to you instead, back out and report them. Discord support will never contact you first in a direct message."
    } else {
        "Are you sure you want to remove your Authenticator App?"
    }
}

/// The text of the alert that asks before the "2FA is activated" dialog closes without a download (`WJFPHJ`, module
/// 549404 `handleCloseModal`).
pub(in crate::ui) const SKIP_BACKUP_TEXT: &str = "Are you sure you don't want to enable SMS authentication or download your backup codes? Without them, you are at risk of losing your account permanently if you lose access to your authenticator app!";

/// `tn` of module 464477: 20 random bytes, base32 without padding, lower case, in groups of four.
pub(in crate::ui) fn new_secret() -> String {
    let mut bytes = [0u8; 20];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..4]);
    group_by_four(&base32(&bytes))
}

fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn group_by_four(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(4)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `cC` / `d` of module 464477: separators go, letters are upper case.
pub(in crate::ui) fn normalize(secret: &str) -> String {
    secret
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '.' | '_' | '-'))
        .collect::<String>()
        .to_uppercase()
}

/// JavaScript's `encodeURI`: everything but letters, digits and `-_.!~*'();/?:@&=+$,#` is percent-encoded.
fn encode_uri(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'();/?:@&=+$,#".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `V3` of module 464477: what the QR code carries.
pub(in crate::ui) fn otpauth_url(email: &str, secret: &str) -> String {
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer=Discord",
        encode_uri("Discord"),
        encode_uri(email),
        normalize(secret)
    )
}

/// `enable` (module 98207): the six digits and the secret the dialog showed.
pub(in crate::ui) fn totp_enable_call(code: &str, secret: &str) -> Call {
    Call {
        method: "POST",
        route: "/users/@me/mfa/totp/enable",
        body: Some(json!({"code": code, "secret": normalize(secret)})),
    }
}

/// `disable`.
pub(in crate::ui) fn totp_disable_call() -> Call {
    Call {
        method: "POST",
        route: "/users/@me/mfa/totp/disable",
        body: None,
    }
}

/// `disableSMS`: the password confirms it.
pub(in crate::ui) fn sms_disable_call(password: &str) -> Call {
    Call {
        method: "POST",
        route: "/users/@me/mfa/sms/disable",
        body: Some(json!({"password": password})),
    }
}

/// `sendMFABackupCodesVerificationKeyEmail`: the password asks for the emailed key.
pub(in crate::ui) fn backup_key_call(password: &str) -> Call {
    Call {
        method: "POST",
        route: "/auth/verify/view-backup-codes-challenge",
        body: Some(json!({"password": password})),
    }
}

/// `confirmViewBackupCodes(key, regenerate)`: the emailed key and the nonce of the matching purpose (the answer of
/// the key request carries `nonce` for viewing and `regenerate_nonce` for a new set).
pub(in crate::ui) fn backup_codes_call(key: &str, nonce: &str, regenerate: bool) -> Call {
    Call {
        method: "POST",
        route: "/users/@me/mfa/codes-verification",
        body: Some(json!({"key": key, "nonce": nonce, "regenerate": regenerate})),
    }
}

/// The same requests as the panel sends them on its own (no dialog open): the answers come back under `MFA_KEY`.
pub(in crate::ui) fn panel_request(call: Call) -> Request {
    Request::Rest {
        key: Some(MFA_KEY.to_owned()),
        method: call.method.parse().unwrap_or(reqwest::Method::POST),
        route: call.route.to_owned(),
        body: call.body,
    }
}

/// `enableSMS`.
pub(in crate::ui) fn sms_enable_call() -> Call {
    Call {
        method: "POST",
        route: "/users/@me/mfa/sms/enable",
        body: None,
    }
}

/// The security keys list (`MFA_WEBAUTHN_CREDENTIALS`).
pub(in crate::ui) fn credentials() -> Request {
    Request::Fetch {
        key: CREDENTIALS_KEY.to_owned(),
        route: "/users/@me/mfa/webauthn/credentials".to_owned(),
    }
}

/// The answer keys of the requests about one security key made outside a dialog (a removal that needs no question).
#[cfg_attr(
    not(feature = "capture"),
    expect(
        dead_code,
        reason = "read by the route of the per-key menu's answers, which is not wired yet"
    )
)]
pub(in crate::ui) const KEY_PREFIX: &str = "settings-mfa-key:";

const CREDENTIALS_ROUTE: &str = "/users/@me/mfa/webauthn/credentials";

/// `startRegisterWebAuthnCredential` (module 917136): an empty POST answers `{ticket, challenge}`, the challenge a
/// JSON string of `publicKey` creation options.
pub(in crate::ui) fn key_start_request(key: &str) -> Request {
    Request::Rest {
        key: Some(key.to_owned()),
        method: reqwest::Method::POST,
        route: CREDENTIALS_ROUTE.to_owned(),
        body: Some(json!({})),
    }
}

/// `AF` (module 917136): the key's name, the ticket of the start request and the credential JSON as a string.
pub(in crate::ui) fn key_register_body(name: &str, ticket: &str, credential: &str) -> Value {
    json!({"name": name, "ticket": ticket, "credential": credential})
}

pub(in crate::ui) fn key_register_request(
    key: &str,
    name: &str,
    ticket: &str,
    credential: &str,
) -> Request {
    Request::Rest {
        key: Some(key.to_owned()),
        method: reqwest::Method::POST,
        route: CREDENTIALS_ROUTE.to_owned(),
        body: Some(key_register_body(name, ticket, credential)),
    }
}

/// `MFA_WEBAUTHN_CREDENTIAL(id)`.
pub(in crate::ui) fn key_route(id: &str) -> String {
    format!("{CREDENTIALS_ROUTE}/{id}")
}

/// `c6` (module 917136): `PATCH` with the new name.
pub(in crate::ui) fn key_rename_request(key: &str, id: &str, name: &str) -> Request {
    Request::Rest {
        key: Some(key.to_owned()),
        method: reqwest::Method::PATCH,
        route: key_route(id),
        body: Some(json!({"name": name})),
    }
}

/// `fR` (module 917136): `DELETE`.
pub(in crate::ui) fn key_delete_request(key: &str, id: &str) -> Request {
    Request::Rest {
        key: Some(key.to_owned()),
        method: reqwest::Method::DELETE,
        route: key_route(id),
        body: None,
    }
}

/// The text the register steps show when the prompt or the request fails (`xSCvBf`, and `fEptJP` for the name step).
pub(in crate::ui) const KEY_PROMPT_FAILED: &str = "An error occurred. Please try again.";
pub(in crate::ui) const KEY_REGISTER_FAILED: &str = "An error occurred, please try again.";
/// `IMKkcn`: what the question before the last passkey goes says.
pub(in crate::ui) const LAST_KEY_TEXT: &str = "Heads up, removing your Passkey means your account will no longer require Multi-Factor Authentication to sign in. If you have not reached out to Discord support for help with removing your Passkey and someone reached out to you instead, back out and report them. Discord support will never contact you first in a direct message.";
/// The name the new key starts with (`I/sJtJ`).
pub(in crate::ui) const KEY_DEFAULT_NAME: &str = "My Key";

/// Module 705147 `k("webauthn")`: removing the last passkey of an account without an authenticator app also ends
/// two-factor authentication, so that removal asks first.
pub(in crate::ui) fn last_key_needs_question(keys: usize, totp: bool) -> bool {
    keys == 1 && !totp
}

/// The credential id the name step's request names: `id` of a list entry.
pub(in crate::ui) fn key_id_of(entry: &Value) -> String {
    entry["id"].as_str().unwrap_or_default().to_owned()
}

/// `bc` of the panel: `abcdefgh` shows as `abcd-efgh`.
pub(in crate::ui) fn code_text(code: &str) -> String {
    let split = code.char_indices().nth(4).map_or(code.len(), |(at, _)| at);
    format!("{}-{}", &code[..split], &code[split..])
}

/// The text of `discord_backup_codes.txt` (module 549404 `getDownloadFileContents`, and `bE`'s `* code (used)`).
pub(in crate::ui) fn backup_file(email: &str, codes: &[(String, bool)]) -> String {
    let lines: Vec<String> = codes
        .iter()
        .map(|(code, used)| {
            format!(
                "* {}{}",
                code_text(code),
                if *used { " (used)" } else { "" }
            )
        })
        .collect();
    format!(
        "These are your Discord backup codes for account {email}.  Keep them safe!\r\n\r\n{}",
        lines.join("\r\n")
    )
}

/// The codes of an answer: `[{code, consumed}]`.
pub(in crate::ui) fn codes_of(answer: &Value) -> Vec<(String, bool)> {
    answer["backup_codes"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|entry| {
                    Some((
                        entry["code"].as_str()?.to_owned(),
                        entry["consumed"].as_bool().unwrap_or(false),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_32_base32_characters_in_eight_groups() {
        let secret = new_secret();
        assert_eq!(secret.len(), 39);
        assert_eq!(secret.split(' ').count(), 8);
        assert!(secret.split(' ').all(|group| group.len() == 4));
        assert!(
            secret
                .chars()
                .all(|c| c == ' ' || c.is_ascii_lowercase() || ('2'..='7').contains(&c))
        );
        assert_ne!(secret, new_secret());
    }

    #[test]
    fn base32_matches_the_rfc_vectors() {
        assert_eq!(base32(b"foobar"), "mzxw6ytboi");
        assert_eq!(base32(b"fo"), "mzxq");
    }

    #[test]
    fn the_qr_carries_the_otpauth_url_of_the_bundle() {
        assert_eq!(
            otpauth_url("me@example.com", "abcd efgh ijkl"),
            "otpauth://totp/Discord:me@example.com?secret=ABCDEFGHIJKL&issuer=Discord"
        );
        assert_eq!(encode_uri("a b"), "a%20b");
    }

    #[test]
    fn the_requests_have_the_shapes_of_module_98207() {
        let call = totp_enable_call("123456", "abcd efgh");
        assert_eq!(
            (call.method, call.route),
            ("POST", "/users/@me/mfa/totp/enable")
        );
        assert_eq!(
            call.body,
            Some(json!({"code": "123456", "secret": "ABCDEFGH"}))
        );
        let call = backup_codes_call("k", "n", true);
        assert_eq!(call.route, "/users/@me/mfa/codes-verification");
        assert_eq!(
            call.body,
            Some(json!({"key": "k", "nonce": "n", "regenerate": true}))
        );
        let call = backup_key_call("pw-placeholder");
        assert_eq!(call.route, "/auth/verify/view-backup-codes-challenge");
        assert_eq!(call.body, Some(json!({"password": "pw-placeholder"})));
        let call = sms_disable_call("pw-placeholder");
        assert_eq!(call.route, "/users/@me/mfa/sms/disable");
        assert_eq!(call.body, Some(json!({"password": "pw-placeholder"})));
        assert_eq!(totp_disable_call().body, None);
        assert_eq!(sms_enable_call().route, "/users/@me/mfa/sms/enable");
        let Request::Rest { key, route, .. } = panel_request(sms_enable_call()) else {
            panic!("rest")
        };
        assert_eq!(
            (key.as_deref(), route.as_str()),
            (Some(MFA_KEY), "/users/@me/mfa/sms/enable")
        );
    }

    #[test]
    fn backup_codes_read_and_save_like_the_bundle() {
        assert_eq!(code_text("abcd1234"), "abcd-1234");
        let answer = json!({"backup_codes": [{"code": "abcd1234", "consumed": false}, {"code": "wxyz9876", "consumed": true}]});
        let codes = codes_of(&answer);
        assert_eq!(codes.len(), 2);
        assert_eq!(
            backup_file("me@example.com", &codes),
            "These are your Discord backup codes for account me@example.com.  Keep them safe!\r\n\r\n* abcd-1234\r\n* wxyz-9876 (used)"
        );
    }

    #[test]
    fn the_security_key_requests_have_the_shapes_of_module_917136() {
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = key_start_request("k")
        else {
            panic!("rest")
        };
        assert_eq!(key.as_deref(), Some("k"));
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/users/@me/mfa/webauthn/credentials");
        assert_eq!(body, Some(json!({})));
        let Request::Rest { body, route, .. } =
            key_register_request("k", "My Key", "t", "{\"id\":1}")
        else {
            panic!("rest")
        };
        assert_eq!(route, "/users/@me/mfa/webauthn/credentials");
        // The credential travels as the JSON text the prompt produced, not as an object.
        assert_eq!(
            body,
            Some(json!({"name": "My Key", "ticket": "t", "credential": "{\"id\":1}"}))
        );
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = key_rename_request("k", "123", "New")
        else {
            panic!("rest")
        };
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/users/@me/mfa/webauthn/credentials/123");
        assert_eq!(body, Some(json!({"name": "New"})));
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = key_delete_request("k", "123")
        else {
            panic!("rest")
        };
        assert_eq!(
            (method, route.as_str(), body),
            (
                reqwest::Method::DELETE,
                "/users/@me/mfa/webauthn/credentials/123",
                None
            )
        );
    }

    #[test]
    fn only_the_last_passkey_of_an_account_without_an_app_asks_first() {
        assert!(last_key_needs_question(1, false));
        assert!(!last_key_needs_question(1, true));
        assert!(!last_key_needs_question(2, false));
        assert!(!last_key_needs_question(0, false));
    }

    #[test]
    fn the_removal_text_depends_on_the_security_keys() {
        assert!(remove_text(0).starts_with("Heads up, removing your Authenticator App"));
        assert_eq!(
            remove_text(1),
            "Are you sure you want to remove your Authenticator App?"
        );
        assert_eq!(remove_text(0), remove_text(0));
        assert_ne!(remove_text(0), remove_text(2));
    }
}
