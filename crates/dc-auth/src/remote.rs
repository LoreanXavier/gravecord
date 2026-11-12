//! QR login: the remote auth gateway and the key handshake, from `useAuthWebsocket` and its key helper in module 785161.
//!
//! The protocol (`wss://remote-auth-gateway.discord.gg/?v=2`, JSON text frames):
//! `hello` -> we send `init` with our RSA-2048 public key (SPKI, base64) -> `nonce_proof` carries a nonce encrypted to
//! that key, which we decrypt (RSA-OAEP, SHA-256) and send back base64url -> `pending_remote_init` carries the key's
//! fingerprint (SHA-256 of the SPKI, base64url), which must equal ours, and the QR code shows it -> `pending_ticket`
//! carries the scanning user, encrypted -> `pending_login` carries a ticket we exchange at `REMOTE_AUTH_LOGIN` for the
//! token, encrypted to our key. `cancel` starts a new session at once and any failure starts one after a backoff; the
//! panel shows no message for either, only the loading cubes again.
//!
//! `yapfast-discord/src/remote_auth.rs` was read for the protocol and not copied (GPL).
//!
//! Nothing in this module logs a fingerprint, a ticket, a payload, a nonce or a token.

use crate::api::AuthApi;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use dc_proto::secrets::Token;
use futures_util::{SinkExt, StreamExt};
use rsa::pkcs8::EncodePublicKey;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, mpsc};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tracing::{debug, warn};

/// `REMOTE_AUTH_ENDPOINT` of the pinned page, plus the `?v=2` the client appends.
pub const ENDPOINT: &str = "wss://remote-auth-gateway.discord.gg/?v=2";
/// The socket is opened from the web app's page, so that is its origin.
pub const ORIGIN: &str = "https://discord.com";
const BACKOFF_START: Duration = Duration::from_millis(1500);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("key generation failed")]
    Key,
    #[error("not valid base64")]
    Base64,
    #[error("could not decrypt")]
    Decrypt,
    #[error("not an encoded user record")]
    UserRecord,
}

/// One session's RSA-2048 key pair. Dropped with the session, and zeroed when dropped.
pub struct KeyPair {
    private: RsaPrivateKey,
    spki: Vec<u8>,
}

impl KeyPair {
    pub fn generate() -> Result<Self, RemoteError> {
        let private =
            RsaPrivateKey::new(&mut rsa_rand::OsRng, 2048).map_err(|_| RemoteError::Key)?;
        Ok(Self::from_private(private))
    }

    pub fn from_private(private: RsaPrivateKey) -> Self {
        let spki = RsaPublicKey::from(&private)
            .to_public_key_der()
            .map(|der| der.as_bytes().to_vec())
            .unwrap_or_default();
        Self { private, spki }
    }

    /// `serializePublicKey`: the SPKI, standard base64 with padding.
    pub fn encoded_public_key(&self) -> String {
        STANDARD.encode(&self.spki)
    }

    /// `publicKeyFingerprint`: SHA-256 of the SPKI, base64url without padding.
    pub fn fingerprint(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(&self.spki))
    }

    fn decrypt(&self, encoded: &str) -> Result<Vec<u8>, RemoteError> {
        let cipher = STANDARD.decode(encoded).map_err(|_| RemoteError::Base64)?;
        self.private
            .decrypt(Oaep::new::<Sha256>(), &cipher)
            .map_err(|_| RemoteError::Decrypt)
    }

    /// `decryptNonce`: the proof of possession, base64url without padding.
    pub fn nonce_proof(&self, encrypted_nonce: &str) -> Result<String, RemoteError> {
        Ok(URL_SAFE_NO_PAD.encode(self.decrypt(encrypted_nonce)?))
    }

    /// `decryptEncodedCiphertext` for the scanning user.
    pub fn user(&self, encrypted_payload: &str) -> Result<RemoteUser, RemoteError> {
        let plain = String::from_utf8(self.decrypt(encrypted_payload)?)
            .map_err(|_| RemoteError::UserRecord)?;
        parse_user_record(&plain)
    }

    /// `decryptEncodedCiphertext` for the account token.
    pub fn token(&self, encrypted_token: &str) -> Result<Token, RemoteError> {
        let plain =
            String::from_utf8(self.decrypt(encrypted_token)?).map_err(|_| RemoteError::Decrypt)?;
        if plain.is_empty() {
            return Err(RemoteError::Decrypt);
        }
        Ok(Token::new(plain))
    }
}

/// The user a phone approved the login for, as far as the handshake tells: enough for "Check your phone!".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteUser {
    pub id: String,
    pub discriminator: String,
    pub avatar: Option<String>,
    pub username: String,
}

impl RemoteUser {
    /// `getUserTag`: the name, with `#0000` kept only for legacy discriminators.
    pub fn tag(&self) -> String {
        if self.discriminator == "0" {
            self.username.clone()
        } else {
            format!("{}#{:0>4}", self.username, self.discriminator)
        }
    }
}

/// `^(\d+):(\d{1,4}):([a-zA-Z0-9_]+):(.*)$`, avatar `0` meaning none.
pub fn parse_user_record(text: &str) -> Result<RemoteUser, RemoteError> {
    let mut parts = text.splitn(4, ':');
    let (Some(id), Some(discriminator), Some(avatar), Some(username)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(RemoteError::UserRecord);
    };
    let digits = |value: &str| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(id)
        || !digits(discriminator)
        || discriminator.len() > 4
        || avatar.is_empty()
        || !avatar
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(RemoteError::UserRecord);
    }
    Ok(RemoteUser {
        id: id.to_owned(),
        discriminator: discriminator.to_owned(),
        avatar: (avatar != "0").then(|| avatar.to_owned()),
        username: username.to_owned(),
    })
}

/// What the QR panel shows (the `step` of the official component).
#[derive(Clone, Debug)]
pub enum RemoteEvent {
    /// `INITIALIZING`: connecting, or between sessions.
    Initializing,
    /// `PENDING_REMOTE_INIT`: the code to scan.
    Code { fingerprint: String },
    /// `PENDING_TICKET`: a phone scanned it, the user has to approve.
    Scanned(RemoteUser),
    /// `PENDING_LOGIN`: approved, the token is being fetched.
    Finishing,
    /// Done. The session ends here.
    Token(Token),
}

pub struct Config {
    pub url: String,
    pub origin: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: ENDPOINT.to_owned(),
            origin: ORIGIN.to_owned(),
        }
    }
}

/// A running remote auth loop. Dropping it closes the socket.
pub struct RemoteAuth {
    restart: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl RemoteAuth {
    pub fn spawn(
        api: Arc<AuthApi>,
        config: Config,
        events: mpsc::UnboundedSender<RemoteEvent>,
    ) -> Self {
        // Dependencies pull in more than one rustls backend, so it has to be chosen. Fails harmlessly if already set.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let restart = Arc::new(Notify::new());
        let task = tokio::spawn(run(api, config, events, restart.clone()));
        Self { restart, task }
    }

    /// "Not me, start over": drops the session and begins a new one at once.
    pub fn restart(&self) {
        self.restart.notify_one();
    }
}

impl Drop for RemoteAuth {
    fn drop(&mut self) {
        self.task.abort();
    }
}

enum Ended {
    Finished,
    /// A failure: a new session after the backoff (`p` in module 785161).
    Retry,
    /// The phone cancelled: a new session at once (`cancel` calls the restart `h` directly, with no backoff).
    Restart,
}

async fn run(
    api: Arc<AuthApi>,
    config: Config,
    events: mpsc::UnboundedSender<RemoteEvent>,
    restart: Arc<Notify>,
) {
    let mut fails = 0u32;
    loop {
        let _ = events.send(RemoteEvent::Initializing);
        let mut reached_code = false;
        let ended = tokio::select! {
            ended = session(&api, &config, &events, &mut reached_code) => ended,
            () = restart.notified() => {
                fails = 0;
                continue;
            }
        };
        match ended {
            Ended::Finished => return,
            Ended::Restart => fails = 0,
            Ended::Retry => {
                // A session that got as far as showing a code was healthy: its end starts the backoff afresh.
                fails = if reached_code { 1 } else { fails + 1 };
                let delay = (BACKOFF_START * 2u32.saturating_pow(fails - 1)).min(BACKOFF_MAX);
                let _ = events.send(RemoteEvent::Initializing);
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    () = restart.notified() => fails = 0,
                }
            }
        }
    }
}

async fn session(
    api: &AuthApi,
    config: &Config,
    events: &mpsc::UnboundedSender<RemoteEvent>,
    reached_code: &mut bool,
) -> Ended {
    // Key generation is CPU work: off the async threads, so heartbeats of other tasks are not starved.
    let Ok(Ok(keys)) = tokio::task::spawn_blocking(KeyPair::generate).await else {
        warn!("remote auth: key generation failed");
        return Ended::Retry;
    };
    let Ok(mut request) = config.url.as_str().into_client_request() else {
        return Ended::Retry;
    };
    if let Ok(origin) = HeaderValue::from_str(&config.origin) {
        request.headers_mut().insert("origin", origin);
    }
    let Ok((mut socket, _)) = connect_async(request).await else {
        debug!("remote auth: could not connect");
        return Ended::Retry;
    };

    let mut heartbeat: Option<tokio::time::Interval> = None;
    let mut first_beat: Option<std::pin::Pin<Box<tokio::time::Sleep>>> = None;
    let mut acked = true;
    loop {
        tokio::select! {
            () = async { first_beat.as_mut().unwrap().await }, if first_beat.is_some() => {
                first_beat = None;
                let interval = heartbeat.as_mut().expect("armed with the interval");
                interval.reset();
                if !beat(&mut socket, &mut acked).await {
                    return Ended::Retry;
                }
            }
            () = async { heartbeat.as_mut().unwrap().tick().await; }, if heartbeat.is_some() && first_beat.is_none() => {
                if !beat(&mut socket, &mut acked).await {
                    debug!("remote auth: heartbeat timeout");
                    return Ended::Retry;
                }
            }
            frame = socket.next() => {
                let Some(Ok(frame)) = frame else {
                    return Ended::Retry;
                };
                let Message::Text(text) = frame else {
                    if matches!(frame, Message::Close(_)) {
                        return Ended::Retry;
                    }
                    continue;
                };
                let Ok(message) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                match message["op"].as_str().unwrap_or_default() {
                    "hello" => {
                        let interval = Duration::from_millis(message["heartbeat_interval"].as_u64().unwrap_or(41_250).max(10));
                        // The first beat comes after a random part of the interval, as the page does.
                        let wait = interval.mul_f64(rand::random::<f64>());
                        first_beat = Some(Box::pin(tokio::time::sleep(wait)));
                        heartbeat = Some(tokio::time::interval(interval));
                        let init = json!({"op": "init", "encoded_public_key": keys.encoded_public_key()});
                        if socket.send(Message::text(init.to_string())).await.is_err() {
                            return Ended::Retry;
                        }
                    }
                    "heartbeat_ack" => acked = true,
                    "nonce_proof" => {
                        let Ok(proof) = keys.nonce_proof(message["encrypted_nonce"].as_str().unwrap_or_default()) else {
                            return Ended::Retry;
                        };
                        let reply = json!({"op": "nonce_proof", "nonce": proof});
                        if socket.send(Message::text(reply.to_string())).await.is_err() {
                            return Ended::Retry;
                        }
                    }
                    "pending_remote_init" => {
                        let fingerprint = message["fingerprint"].as_str().unwrap_or_default();
                        if fingerprint != keys.fingerprint() {
                            warn!("remote auth: the fingerprint did not match our key");
                            return Ended::Retry;
                        }
                        *reached_code = true;
                        let _ = events.send(RemoteEvent::Code { fingerprint: fingerprint.to_owned() });
                    }
                    "pending_ticket" => {
                        let payload = message["encrypted_user_payload"].as_str().unwrap_or_default();
                        let Ok(user) = keys.user(payload) else {
                            return Ended::Retry;
                        };
                        let _ = events.send(RemoteEvent::Scanned(user));
                    }
                    "pending_login" => {
                        let Some(ticket) = message["ticket"].as_str() else {
                            return Ended::Retry;
                        };
                        let _ = events.send(RemoteEvent::Finishing);
                        let token = match api.remote_auth_login(ticket).await {
                            Ok(encrypted) => keys.token(&encrypted).ok(),
                            Err(_) => None,
                        };
                        return match token {
                            Some(token) => {
                                let _ = events.send(RemoteEvent::Token(token));
                                let _ = socket.close(None).await;
                                Ended::Finished
                            }
                            None => Ended::Retry,
                        };
                    }
                    "cancel" => return Ended::Restart,
                    _ => {}
                }
            }
        }
    }
}

/// Sends a heartbeat, or reports that the last one was never acknowledged.
async fn beat<S>(socket: &mut S, acked: &mut bool) -> bool
where
    S: SinkExt<Message> + Unpin,
{
    if !*acked {
        return false;
    }
    *acked = false;
    socket
        .send(Message::text(json!({"op": "heartbeat"}).to_string()))
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> KeyPair {
        KeyPair::generate().unwrap()
    }

    fn encrypt(keys: &KeyPair, plain: &[u8]) -> String {
        let public = RsaPublicKey::from(&keys.private);
        let cipher = public
            .encrypt(&mut rsa_rand::OsRng, Oaep::new::<Sha256>(), plain)
            .unwrap();
        STANDARD.encode(cipher)
    }

    #[test]
    fn the_fingerprint_is_the_spki_digest_in_base64url() {
        let keys = pair();
        let der = STANDARD.decode(keys.encoded_public_key()).unwrap();
        assert_eq!(
            keys.fingerprint(),
            URL_SAFE_NO_PAD.encode(Sha256::digest(&der))
        );
        assert_eq!(keys.fingerprint().len(), 43);
        assert!(!keys.fingerprint().contains(['+', '/', '=']));
        // An RSA-2048 SPKI is 294 bytes.
        assert_eq!(der.len(), 294);
    }

    #[test]
    fn nonce_proof_round_trips_through_the_key() {
        let keys = pair();
        let nonce = [7u8; 32];
        let proof = keys.nonce_proof(&encrypt(&keys, &nonce)).unwrap();
        assert_eq!(proof, URL_SAFE_NO_PAD.encode(nonce));
        assert!(keys.nonce_proof("not base64!").is_err());
        assert!(keys.nonce_proof(&STANDARD.encode([1u8; 256])).is_err());
    }

    #[test]
    fn the_scanning_user_and_the_token_decrypt() {
        let keys = pair();
        let user = keys
            .user(&encrypt(&keys, b"123456789012345678:0:abcdef_0:Test:User"))
            .unwrap();
        assert_eq!(user.id, "123456789012345678");
        assert_eq!(user.avatar.as_deref(), Some("abcdef_0"));
        assert_eq!(user.username, "Test:User");
        assert_eq!(user.tag(), "Test:User");
        assert_eq!(
            keys.token(&encrypt(&keys, b"TEST.TOKEN.VALUE"))
                .unwrap()
                .expose(),
            "TEST.TOKEN.VALUE"
        );
    }

    #[test]
    fn user_records_follow_the_official_pattern() {
        let none = parse_user_record("1:0:0:name").unwrap();
        assert_eq!(none.avatar, None);
        let legacy = parse_user_record("1:42:0:name").unwrap();
        assert_eq!(legacy.tag(), "name#0042");
        assert!(parse_user_record("x:0:0:name").is_err());
        assert!(parse_user_record("1:12345:0:name").is_err());
        assert!(parse_user_record("1:0:bad hash:name").is_err());
        assert!(parse_user_record("1:0:0").is_err());
    }
}
