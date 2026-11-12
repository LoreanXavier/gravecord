//! WebAuthn for the Windows build: the second factor "Authenticate with a passkey or security key" of the login and
//! the registration of a security key or passkey in Settings. The official client hands the `publicKey` options the
//! API sent to `navigator.credentials.get` / `.create` and posts back the JSON of the credential (module 293731 `J`
//! and `v`); this crate does the same with the system's own prompt (`webauthn.dll`) in the browser's place.
//!
//! `get_assertion` and `create_credential` block until the person answers the system prompt: call them from a thread
//! of their own. They are never called from tests or capture builds (the caller's stub answers `Cancelled`).

mod b64;
mod client_data;
mod cose;
mod options;
mod response;
#[cfg(windows)]
mod win;

pub use b64::{decode as base64url_decode, encode as base64url_encode};
pub use client_data::{ORIGIN, RP_ID, client_data_json};
pub use options::{AssertionRequest, CreationRequest, Credential, Selection};
pub use response::{RawAssertion, RawAttestation, assertion_json, creation_json};

/// How a call ends without a credential.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// The person closed the prompt, the key was not touched in time, or no key answered (`NotAllowedError`).
    #[error("cancelled")]
    Cancelled,
    /// This system has no usable WebAuthn API.
    #[error("WebAuthn is not available")]
    Unavailable,
    /// The options were not what the API sends.
    #[error("invalid options: {0}")]
    Invalid(String),
    /// Any other failure of the system call, by its HRESULT.
    #[error("WebAuthn failed ({0:#010x})")]
    Failed(u32),
}

/// `navigator.credentials.get` over the system prompt: `options` is the challenge document of the API
/// (`{"publicKey": {...}}`), `window` the handle of the window the prompt belongs to. The answer is the credential
/// JSON the API takes back.
pub fn get_assertion(window: isize, options: &str) -> Result<String, Error> {
    let request = AssertionRequest::parse(options)?;
    let client = client_data_json("webauthn.get", &request.challenge);
    #[cfg(windows)]
    {
        let raw = win::get_assertion(window, &request, client.as_bytes())?;
        Ok(assertion_json(&raw, &client))
    }
    #[cfg(not(windows))]
    {
        let _ = (window, client);
        Err(Error::Unavailable)
    }
}

/// `navigator.credentials.create` over the system prompt, for a security key or passkey.
pub fn create_credential(window: isize, options: &str) -> Result<String, Error> {
    let request = CreationRequest::parse(options)?;
    let client = client_data_json("webauthn.create", &request.challenge);
    #[cfg(windows)]
    {
        let raw = win::create_credential(window, &request, client.as_bytes())?;
        Ok(creation_json(&raw, &client))
    }
    #[cfg(not(windows))]
    {
        let _ = (window, client);
        Err(Error::Unavailable)
    }
}

/// Whether this system can run the calls at all (the API version the prompt needs).
pub fn is_available() -> bool {
    #[cfg(windows)]
    {
        win::api_version() >= win::MIN_API_VERSION
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Options that do not parse end the call before the system is asked anything, so no prompt can open.
    #[test]
    fn broken_options_never_reach_the_system() {
        assert!(matches!(get_assertion(0, "x"), Err(Error::Invalid(_))));
        assert!(matches!(create_credential(0, "{}"), Err(Error::Invalid(_))));
    }

    /// A query that shows nothing: this system's API version (the prompt itself is never opened by a test).
    #[cfg(windows)]
    #[test]
    fn the_system_reports_an_api_version() {
        assert!(win::api_version() >= 1);
        assert_eq!(is_available(), win::api_version() >= win::MIN_API_VERSION);
    }
}
