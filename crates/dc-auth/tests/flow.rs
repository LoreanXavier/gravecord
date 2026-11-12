//! Every login flow against a fake Discord: a local HTTP server for the REST routes and a local WebSocket server for the
//! remote auth gateway. Test values only; nothing leaves the machine.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use dc_auth::flow::{Command, Config, Event, spawn};
use dc_auth::remote::{self, RemoteEvent};
use dc_auth::{CaptchaAnswer, Failure, MfaKind, MfaMethod};
use dc_proto::identity::Identity;
use futures_util::{SinkExt, StreamExt};
use rsa::pkcs8::DecodePublicKey;
use rsa::{Oaep, RsaPublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use zeroize::Zeroizing;

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Value,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

type Handler = Arc<dyn Fn(&Seen) -> (u16, Value) + Send + Sync>;

struct Rest {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Rest {
    fn requests_to(&self, path: &str) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == path)
            .cloned()
            .collect()
    }
}

async fn rest_server(handler: impl Fn(&Seen) -> (u16, Value) + Send + Sync + 'static) -> Rest {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler: Handler = Arc::new(handler);
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                loop {
                    // Read one request: headers, then the declared body.
                    let (head, rest) = loop {
                        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                            let rest = buffer.split_off(at + 4);
                            break (String::from_utf8_lossy(&buffer).into_owned(), rest);
                        }
                        let mut chunk = [0u8; 4096];
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                        }
                    };
                    let mut lines = head.split("\r\n");
                    let mut first = lines.next().unwrap().split(' ');
                    let method = first.next().unwrap().to_owned();
                    let path = first.next().unwrap().split('?').next().unwrap().to_owned();
                    let headers: Vec<(String, String)> = lines
                        .filter_map(|line| line.split_once(": "))
                        .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
                        .collect();
                    let length: usize = headers
                        .iter()
                        .find(|(k, _)| k == "content-length")
                        .and_then(|(_, v)| v.parse().ok())
                        .unwrap_or(0);
                    let mut body = rest;
                    while body.len() < length {
                        let mut chunk = [0u8; 4096];
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => body.extend_from_slice(&chunk[..n]),
                        }
                    }
                    buffer = body.split_off(length);
                    let seen = Seen {
                        method,
                        path,
                        headers,
                        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                    };
                    let (status, answer) = handler(&seen);
                    log.lock().unwrap().push(seen);
                    let text = answer.to_string();
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{text}",
                        text.len()
                    );
                    if stream.write_all(reply.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Rest { base, seen }
}

fn config(rest: &Rest, ws: Option<String>) -> Config {
    Config {
        api_base: rest.base.clone(),
        remote: remote::Config {
            url: ws.unwrap_or_else(|| "ws://127.0.0.1:9/".to_owned()),
            origin: remote::ORIGIN.to_owned(),
        },
        identity: Identity::new(),
    }
}

async fn next(events: &mut UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(20), events.recv())
        .await
        .expect("an event in time")
        .expect("the controller is alive")
}

/// The next event that is not `LoggingIn`.
async fn outcome(events: &mut UnboundedReceiver<Event>) -> Event {
    loop {
        let event = next(events).await;
        if !matches!(event, Event::LoggingIn) {
            return event;
        }
    }
}

fn login(commands: &UnboundedSender<Command>, undelete: bool) {
    commands
        .send(Command::Login {
            login: "tester@example.invalid".into(),
            password: Zeroizing::new("test-value-only".into()),
            undelete,
        })
        .unwrap();
}

fn default_routes(seen: &Seen) -> (u16, Value) {
    match seen.path.as_str() {
        "/experiments" => (200, json!({"fingerprint": "123.test-fingerprint"})),
        _ => (404, json!({"message": "404: Not Found", "code": 0})),
    }
}

#[tokio::test]
async fn password_login_gives_the_token_and_sends_the_pinned_headers() {
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/login" => (200, json!({"user_id": "1", "token": "TEST.PASSWORD.TOKEN"})),
        _ => default_routes(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    assert!(matches!(next(&mut events).await, Event::LoggingIn));
    let Event::Success(token) = next(&mut events).await else {
        panic!("no token")
    };
    assert_eq!(token.expose(), "TEST.PASSWORD.TOKEN");

    let request = &rest.requests_to("/auth/login")[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.body["login"], "tester@example.invalid");
    assert_eq!(request.body["password"], "test-value-only");
    assert_eq!(request.body["undelete"], false);
    assert!(request.body["login_source"].is_null());
    // Pinned identity, the fingerprint of the logged-out client, and no authorization at all.
    assert!(request.header("x-super-properties").is_some());
    assert_eq!(
        request.header("x-fingerprint"),
        Some("123.test-fingerprint")
    );
    assert!(request.header("authorization").is_none());
    assert!(request.header("user-agent").unwrap().contains("discord/"));
}

#[tokio::test]
async fn invalid_login_comes_back_under_the_fields() {
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/login" => (
            400,
            json!({"code": 50035, "message": "Invalid Form Body", "errors": {
                "login": {"_errors": [{"code": "INVALID_LOGIN", "message": "Login or password is invalid."}]},
                "password": {"_errors": [{"code": "INVALID_LOGIN", "message": "Login or password is invalid."}]}}}),
        ),
        _ => default_routes(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    let Event::Fields(errors) = outcome(&mut events).await else {
        panic!("no field errors")
    };
    assert_eq!(errors.field("login"), Some("Login or password is invalid."));
    assert_eq!(
        errors.field("password"),
        Some("Login or password is invalid.")
    );
}

#[tokio::test]
async fn rate_limit_and_dead_network_leave_the_form_quiet() {
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/login" => (
            429,
            json!({"message": "You are being rate limited.", "retry_after": 2.5, "global": false}),
        ),
        _ => default_routes(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    let Event::Quiet(Failure::RateLimited { retry_after }) = outcome(&mut events).await else {
        panic!("not a rate limit")
    };
    assert_eq!(retry_after, 2.5);

    let dead = Rest {
        base: "http://127.0.0.1:9".into(),
        seen: Default::default(),
    };
    let (commands, mut events) = spawn(config(&dead, None)).unwrap();
    login(&commands, false);
    assert!(matches!(
        outcome(&mut events).await,
        Event::Quiet(Failure::Network)
    ));
}

fn mfa_login(seen: &Seen) -> (u16, Value) {
    match seen.path.as_str() {
        "/auth/login" => (
            200,
            json!({"mfa": true, "ticket": "TEST-TICKET", "totp": true, "sms": true, "backup": true,
                   "webauthn": null, "user_id": "1", "login_instance_id": "TEST-INSTANCE"}),
        ),
        "/auth/mfa/totp" if seen.body["code"] == "123456" => {
            (200, json!({"token": "TEST.MFA.TOKEN"}))
        }
        "/auth/mfa/totp" => (
            400,
            json!({"code": 60008, "message": "Invalid two-factor code"}),
        ),
        "/auth/mfa/sms/send" => (200, json!({"phone": "+** *** *** ** 34"})),
        "/auth/mfa/sms" => (200, json!({"token": "TEST.SMS.TOKEN"})),
        "/auth/mfa/backup" => (200, json!({"token": "TEST.BACKUP.TOKEN"})),
        _ => default_routes(seen),
    }
}

#[tokio::test]
async fn mfa_totp_with_a_wrong_then_the_right_code() {
    let rest = rest_server(mfa_login).await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    let Event::Mfa(challenge) = outcome(&mut events).await else {
        panic!("no mfa step")
    };
    assert_eq!(challenge.ticket, "TEST-TICKET");
    assert_eq!(
        challenge.methods,
        vec![MfaMethod::Totp, MfaMethod::Backup, MfaMethod::Sms]
    );

    commands
        .send(Command::Mfa {
            kind: MfaKind::Totp,
            code: "000000".into(),
        })
        .unwrap();
    let Event::MfaFailed(message) = outcome(&mut events).await else {
        panic!("not refused")
    };
    assert_eq!(message, "Invalid two-factor code");

    commands
        .send(Command::Mfa {
            kind: MfaKind::Totp,
            code: "123456".into(),
        })
        .unwrap();
    let Event::Success(token) = outcome(&mut events).await else {
        panic!("no token")
    };
    assert_eq!(token.expose(), "TEST.MFA.TOKEN");

    let sent = rest.requests_to("/auth/mfa/totp");
    assert_eq!(sent[1].body["ticket"], "TEST-TICKET");
    assert_eq!(sent[1].body["login_instance_id"], "TEST-INSTANCE");
    assert!(sent[1].header("authorization").is_none());
}

#[tokio::test]
async fn mfa_sms_and_backup_code() {
    let rest = rest_server(mfa_login).await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    assert!(matches!(outcome(&mut events).await, Event::Mfa(_)));
    commands.send(Command::SendSms).unwrap();
    let Event::SmsSent { phone } = outcome(&mut events).await else {
        panic!("no sms")
    };
    assert_eq!(phone, "+** *** *** ** 34");
    assert_eq!(
        rest.requests_to("/auth/mfa/sms/send")[0].body["ticket"],
        "TEST-TICKET"
    );
    commands
        .send(Command::Mfa {
            kind: MfaKind::Sms,
            code: "654321".into(),
        })
        .unwrap();
    let Event::Success(token) = outcome(&mut events).await else {
        panic!("no token")
    };
    assert_eq!(token.expose(), "TEST.SMS.TOKEN");

    // A reset forgets the ticket: a code after it goes nowhere.
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    assert!(matches!(outcome(&mut events).await, Event::Mfa(_)));
    commands.send(Command::Reset).unwrap();
    commands
        .send(Command::Mfa {
            kind: MfaKind::Backup,
            code: "abcd1234".into(),
        })
        .unwrap();
    commands
        .send(Command::Forgot { login: "x".into() })
        .unwrap();
    // The forgot answer (a 404 here) is the next event: the backup code produced none.
    assert!(matches!(
        outcome(&mut events).await,
        Event::Quiet(_) | Event::ForgotFields(_)
    ));
    assert!(rest.requests_to("/auth/mfa/backup").is_empty());
}

#[tokio::test]
async fn captcha_is_resent_with_its_answer_headers() {
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/login" if seen.header("x-captcha-key").is_some() => {
            (200, json!({"user_id": "1", "token": "TEST.CAPTCHA.TOKEN"}))
        }
        "/auth/login" => (
            400,
            json!({"captcha_key": ["captcha-required"], "captcha_service": "hcaptcha",
                   "captcha_sitekey": "TEST-SITEKEY", "captcha_session_id": "TEST-SESSION",
                   "captcha_rqdata": "TEST-RQDATA", "captcha_rqtoken": "TEST-RQTOKEN"}),
        ),
        _ => default_routes(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    let Event::Captcha(challenge) = outcome(&mut events).await else {
        panic!("no captcha")
    };
    assert_eq!(challenge.service, "hcaptcha");
    assert_eq!(challenge.sitekey, "TEST-SITEKEY");
    assert_eq!(challenge.rqdata.as_deref(), Some("TEST-RQDATA"));

    commands
        .send(Command::CaptchaSolved(CaptchaAnswer {
            key: "TEST-ANSWER".into(),
            rqtoken: Some("TEST-RQTOKEN".into()),
            session_id: Some("TEST-SESSION".into()),
        }))
        .unwrap();
    let Event::Success(token) = outcome(&mut events).await else {
        panic!("no token")
    };
    assert_eq!(token.expose(), "TEST.CAPTCHA.TOKEN");
    let second = &rest.requests_to("/auth/login")[1];
    assert_eq!(second.header("x-captcha-key"), Some("TEST-ANSWER"));
    assert_eq!(second.header("x-captcha-rqtoken"), Some("TEST-RQTOKEN"));
    assert_eq!(second.header("x-captcha-session-id"), Some("TEST-SESSION"));
    // The same credentials went again.
    assert_eq!(second.body["login"], "tester@example.invalid");
}

#[tokio::test]
async fn disabled_and_scheduled_accounts_and_restore() {
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/login" if seen.body["undelete"] == true => {
            (200, json!({"user_id": "1", "token": "TEST.RESTORED"}))
        }
        "/auth/login" if seen.body["login"] == "gone@example.invalid" => {
            (400, json!({"code": 20011, "message": "scheduled"}))
        }
        "/auth/login" => (400, json!({"code": 20013, "message": "disabled"})),
        _ => default_routes(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    assert!(matches!(outcome(&mut events).await, Event::AccountDisabled));
    commands
        .send(Command::Login {
            login: "gone@example.invalid".into(),
            password: Zeroizing::new("x".into()),
            undelete: false,
        })
        .unwrap();
    assert!(matches!(
        outcome(&mut events).await,
        Event::ScheduledForDeletion
    ));
    // "Restore Account" resends the typed login with undelete.
    commands.send(Command::RestoreAccount).unwrap();
    let Event::Success(token) = outcome(&mut events).await else {
        panic!("not restored")
    };
    assert_eq!(token.expose(), "TEST.RESTORED");
    assert_eq!(
        rest.requests_to("/auth/login")[2].body["login"],
        "gone@example.invalid"
    );
}

#[tokio::test]
async fn phone_verification_and_suspended_accounts() {
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/login" if seen.body["login"] == "sus@example.invalid" => (
            403,
            json!({"suspended_user_token": "TEST-SUSPENDED", "message": "x"}),
        ),
        "/auth/login" => (400, json!({"code": 70007, "message": "verify"})),
        _ => default_routes(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    assert!(matches!(
        outcome(&mut events).await,
        Event::PhoneVerificationRequired
    ));
    commands
        .send(Command::Login {
            login: "sus@example.invalid".into(),
            password: Zeroizing::new("x".into()),
            undelete: false,
        })
        .unwrap();
    assert!(matches!(outcome(&mut events).await, Event::Suspended));
}

#[tokio::test]
async fn forgot_password_sent_one_time_and_field_error() {
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/forgot" if seen.body["login"] == "otl@example.invalid" => (200, json!({"method": "one_time_login"})),
        "/auth/forgot" if seen.body["login"] == "" => (
            400,
            json!({"code": 50035, "errors": {"login": {"_errors": [{"code": "BASE_TYPE_REQUIRED", "message": "This field is required"}]}}}),
        ),
        "/auth/forgot" => (200, json!({})),
        _ => default_routes(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    commands
        .send(Command::Forgot {
            login: "tester@example.invalid".into(),
        })
        .unwrap();
    let Event::ForgotSent { login } = outcome(&mut events).await else {
        panic!("not sent")
    };
    assert_eq!(login, "tester@example.invalid");
    commands
        .send(Command::Forgot {
            login: "otl@example.invalid".into(),
        })
        .unwrap();
    assert!(matches!(
        outcome(&mut events).await,
        Event::ForgotOneTimeLogin
    ));
    commands
        .send(Command::Forgot {
            login: String::new(),
        })
        .unwrap();
    let Event::ForgotFields(errors) = outcome(&mut events).await else {
        panic!("no field error")
    };
    assert_eq!(errors.field("login"), Some("This field is required"));
}

/// How the fake remote auth gateway behaves in one connection.
#[derive(Clone, Copy, PartialEq)]
enum Script {
    /// Full handshake, scan, approval.
    Approve,
    /// Handshake with a fingerprint that is not the key's.
    BadFingerprint,
    /// Handshake, then `cancel`.
    Cancel,
    /// Handshake, then silence (a code on screen nobody scans).
    Hold,
}

struct Gateway {
    url: String,
    connections: Arc<Mutex<u32>>,
    proofs_ok: Arc<Mutex<Vec<bool>>>,
}

fn encrypt_to(public: &RsaPublicKey, plain: &[u8]) -> String {
    let cipher = public
        .encrypt(&mut rsa::rand_core::OsRng, Oaep::new::<Sha256>(), plain)
        .unwrap();
    STANDARD.encode(cipher)
}

async fn gateway(scripts: Vec<Script>, key_slot: Arc<Mutex<Option<RsaPublicKey>>>) -> Gateway {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/?v=2", listener.local_addr().unwrap());
    let connections = Arc::new(Mutex::new(0u32));
    let proofs_ok = Arc::new(Mutex::new(Vec::new()));
    let (count, proofs) = (connections.clone(), proofs_ok.clone());
    tokio::spawn(async move {
        let key_slot = key_slot;
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let index = {
                let mut count = count.lock().unwrap();
                *count += 1;
                *count as usize - 1
            };
            let script = scripts.get(index).copied().unwrap_or(Script::Approve);
            let proofs = proofs.clone();
            let key_slot = key_slot.clone();
            tokio::spawn(async move {
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let send = |value: Value| Message::text(value.to_string());
                // A held session heartbeats fast, to prove the acks; the scripted ones sleep inside their handlers, so they ask
                // for beats far apart.
                let interval = if script == Script::Hold { 40 } else { 5000 };
                socket.send(send(json!({"op": "hello", "timeout_ms": 120000, "heartbeat_interval": interval}))).await.unwrap();
                let mut public: Option<RsaPublicKey> = None;
                let nonce = [9u8; 32];
                while let Some(Ok(frame)) = socket.next().await {
                    let Message::Text(text) = frame else { continue };
                    let message: Value = serde_json::from_str(&text).unwrap();
                    match message["op"].as_str().unwrap() {
                        "heartbeat" => socket
                            .send(send(json!({"op": "heartbeat_ack"})))
                            .await
                            .unwrap(),
                        "init" => {
                            let der = STANDARD
                                .decode(message["encoded_public_key"].as_str().unwrap())
                                .unwrap();
                            let key = RsaPublicKey::from_public_key_der(&der).unwrap();
                            socket
                                .send(send(json!({"op": "nonce_proof", "encrypted_nonce": encrypt_to(&key, &nonce)})))
                                .await
                                .unwrap();
                            *key_slot.lock().unwrap() = Some(key.clone());
                            public = Some(key);
                        }
                        "nonce_proof" => {
                            let ok = message["nonce"] == URL_SAFE_NO_PAD.encode(nonce);
                            proofs.lock().unwrap().push(ok);
                            let key = public.clone().unwrap();
                            let der = key.to_public_key_der_for_test();
                            let fingerprint = if script == Script::BadFingerprint {
                                "not-the-fingerprint".to_owned()
                            } else {
                                URL_SAFE_NO_PAD.encode(Sha256::digest(&der))
                            };
                            socket.send(send(json!({"op": "pending_remote_init", "fingerprint": fingerprint}))).await.unwrap();
                            match script {
                                Script::Approve => {
                                    tokio::time::sleep(Duration::from_millis(150)).await;
                                    let record =
                                        encrypt_to(&key, b"123456789012345678:0:0:TestUser");
                                    socket.send(send(json!({"op": "pending_ticket", "encrypted_user_payload": record}))).await.unwrap();
                                    tokio::time::sleep(Duration::from_millis(150)).await;
                                    socket
                                        .send(send(
                                            json!({"op": "pending_login", "ticket": "TEST-TICKET"}),
                                        ))
                                        .await
                                        .unwrap();
                                }
                                Script::Cancel => {
                                    tokio::time::sleep(Duration::from_millis(150)).await;
                                    socket.send(send(json!({"op": "cancel"}))).await.unwrap();
                                }
                                Script::BadFingerprint | Script::Hold => {}
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
    });
    Gateway {
        url,
        connections,
        proofs_ok,
    }
}

trait SpkiForTest {
    fn to_public_key_der_for_test(&self) -> Vec<u8>;
}

impl SpkiForTest for RsaPublicKey {
    fn to_public_key_der_for_test(&self) -> Vec<u8> {
        use rsa::pkcs8::EncodePublicKey;
        self.to_public_key_der().unwrap().as_bytes().to_vec()
    }
}

/// Collects `Event::Qr` steps until the success arrives (or the test times out).
async fn qr_until_success(events: &mut UnboundedReceiver<Event>) -> (Vec<String>, String) {
    let mut steps = Vec::new();
    loop {
        match next(events).await {
            Event::Qr(RemoteEvent::Initializing) => steps.push("initializing".into()),
            Event::Qr(RemoteEvent::Code { fingerprint }) => {
                assert_eq!(fingerprint.len(), 43);
                steps.push("code".into());
            }
            Event::Qr(RemoteEvent::Scanned(user)) => {
                assert_eq!(user.username, "TestUser");
                assert_eq!(user.id, "123456789012345678");
                assert_eq!(user.avatar, None);
                steps.push("scanned".into());
            }
            Event::Qr(RemoteEvent::Finishing) => steps.push("finishing".into()),
            Event::Success(token) => return (steps, token.expose().to_owned()),
            other => panic!("unexpected {other:?}"),
        }
    }
}

async fn token_rest(slot: Arc<Mutex<Option<RsaPublicKey>>>) -> Rest {
    rest_server(move |seen| match seen.path.as_str() {
        "/users/@me/remote-auth/login" => {
            let key = slot.lock().unwrap().clone();
            match key {
                Some(key) if seen.body["ticket"] == "TEST-TICKET" => (
                    200,
                    json!({"encrypted_token": encrypt_to(&key, b"TEST.QR.TOKEN")}),
                ),
                _ => (400, json!({"message": "no key"})),
            }
        }
        _ => default_routes(seen),
    })
    .await
}

#[tokio::test]
async fn qr_login_handshake_scan_approve_and_exchange() {
    let slot = Arc::new(Mutex::new(None));
    let gateway = gateway(vec![Script::Approve], slot.clone()).await;
    let rest = token_rest(slot).await;
    let (commands, mut events) = spawn(config(&rest, Some(gateway.url.clone()))).unwrap();
    commands.send(Command::StartQr).unwrap();
    let (steps, token) = qr_until_success(&mut events).await;
    assert_eq!(steps, ["initializing", "code", "scanned", "finishing"]);
    assert_eq!(token, "TEST.QR.TOKEN");
    // The nonce proof the client sent decrypted to what the server encrypted.
    assert_eq!(*gateway.proofs_ok.lock().unwrap(), vec![true]);
    let exchange = &rest.requests_to("/users/@me/remote-auth/login")[0];
    assert_eq!(exchange.body["ticket"], "TEST-TICKET");
    assert!(exchange.header("authorization").is_none());
}

#[tokio::test]
async fn qr_restarts_after_a_bad_fingerprint_and_after_cancel() {
    let slot = Arc::new(Mutex::new(None));
    let gateway = gateway(
        vec![Script::BadFingerprint, Script::Cancel, Script::Approve],
        slot.clone(),
    )
    .await;
    let rest = token_rest(slot).await;
    let (commands, mut events) = spawn(config(&rest, Some(gateway.url.clone()))).unwrap();
    commands.send(Command::StartQr).unwrap();
    let (_, token) = qr_until_success(&mut events).await;
    assert_eq!(token, "TEST.QR.TOKEN");
    // Bad fingerprint, cancel, then the good session: three connections, three keys.
    assert_eq!(*gateway.connections.lock().unwrap(), 3);
}

#[tokio::test]
async fn not_me_start_over_opens_a_new_session() {
    let slot = Arc::new(Mutex::new(None));
    let gateway = gateway(vec![Script::Hold, Script::Approve], slot.clone()).await;
    let rest = token_rest(slot).await;
    let (commands, mut events) = spawn(config(&rest, Some(gateway.url.clone()))).unwrap();
    commands.send(Command::StartQr).unwrap();
    loop {
        if matches!(next(&mut events).await, Event::Qr(RemoteEvent::Code { .. })) {
            break;
        }
    }
    commands.send(Command::RestartQr).unwrap();
    let (_, token) = qr_until_success(&mut events).await;
    assert_eq!(token, "TEST.QR.TOKEN");
    assert_eq!(*gateway.connections.lock().unwrap(), 2);
}

#[tokio::test]
async fn stopping_qr_closes_the_socket_and_logout_sends_the_token_once() {
    let slot = Arc::new(Mutex::new(None));
    let gateway = gateway(vec![Script::Hold], slot).await;
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/logout" => (204, Value::Null),
        _ => default_routes(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, Some(gateway.url.clone()))).unwrap();
    commands.send(Command::StartQr).unwrap();
    loop {
        if matches!(next(&mut events).await, Event::Qr(RemoteEvent::Code { .. })) {
            break;
        }
    }
    commands.send(Command::StopQr).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(*gateway.connections.lock().unwrap(), 1);

    let api = dc_auth::api::AuthApi::new(rest.base.clone(), Identity::new()).unwrap();
    api.logout(&dc_proto::secrets::Token::new("TEST.LOGOUT.TOKEN"))
        .await
        .unwrap();
    let sent = &rest.requests_to("/auth/logout")[0];
    assert_eq!(sent.header("authorization"), Some("TEST.LOGOUT.TOKEN"));
}

#[tokio::test]
async fn refused_codes_show_the_sentence_the_dialog_shows() {
    let rest = rest_server(|seen| match seen.path.as_str() {
        "/auth/mfa/totp" => (
            429,
            json!({"message": "You are being rate limited.", "retry_after": 2.5}),
        ),
        "/auth/mfa/backup" => (502, Value::Null),
        "/auth/mfa/sms" => (
            429,
            json!({"message": "You are being rate limited.", "retry_after": 2.5}),
        ),
        "/auth/mfa/sms/send" => (
            429,
            json!({"message": "You are being rate limited.", "retry_after": 2.5}),
        ),
        _ => mfa_login(seen),
    })
    .await;
    let (commands, mut events) = spawn(config(&rest, None)).unwrap();
    login(&commands, false);
    assert!(matches!(outcome(&mut events).await, Event::Mfa(_)));
    let code = |kind, text: &str| Command::Mfa {
        kind,
        code: text.into(),
    };
    commands.send(code(MfaKind::Totp, "000000")).unwrap();
    let Event::MfaFailed(message) = outcome(&mut events).await else {
        panic!("a rate limit must show in the dialog")
    };
    assert_eq!(message, "You are being rate limited.");
    commands.send(code(MfaKind::Backup, "abcd1234")).unwrap();
    let Event::MfaFailed(message) = outcome(&mut events).await else {
        panic!("a 502 must show in the dialog")
    };
    assert_eq!(message, "POST /auth/mfa/backup [502]");
    commands.send(code(MfaKind::Sms, "654321")).unwrap();
    let Event::MfaFailed(message) = outcome(&mut events).await else {
        panic!("a rate limit must show in the dialog")
    };
    assert_eq!(message, "POST /auth/mfa/sms [429]");
    commands.send(Command::SendSms).unwrap();
    let Event::SmsFailed(message) = outcome(&mut events).await else {
        panic!("a rate limit must show in the dialog")
    };
    assert_eq!(message, "You are being rate limited.");

    // No answer at all: superagent's own sentence.
    let api = dc_auth::api::AuthApi::new("http://127.0.0.1:9", Identity::new()).unwrap();
    let challenge = dc_auth::MfaChallenge {
        ticket: "t".into(),
        methods: vec![MfaMethod::Totp],
        login_instance_id: None,
    };
    let Err(Failure::MfaInvalid(message)) =
        api.mfa(MfaKind::Totp, "000000", &challenge, None).await
    else {
        panic!("a dead network must show in the dialog")
    };
    assert_eq!(message, dc_auth::api::NETWORK_MESSAGE);
}
