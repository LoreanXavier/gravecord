//! Inert mode (`GRAVECORD_INERT=1`, capture feature only): the app can look and read but cannot act on the outside
//! world. Requests from the UI are default-denied in `net.rs`; gpui skips the system effects (clipboard, browser,
//! file dialogs, quit, window close and minimise); capture devices, log out and the like call `skip` here.
//! Every refusal or skip is logged as `inert: ...` and counted, so the crawler counts the control as wired.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNT: AtomicU64 = AtomicU64::new(0);
static REQUESTS: AtomicU64 = AtomicU64::new(0);
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether this run is inert. Always false without the capture feature.
pub fn on() -> bool {
    cfg!(feature = "capture") && std::env::var("GRAVECORD_INERT").as_deref() == Ok("1")
}

/// Turns the mode on in gpui too. Called once at start.
pub fn init() {
    if on() {
        let _ = gpui::INERT_NOTE.set(|what| note(&format!("skipped {what}")));
        gpui::INERT.store(true, Ordering::Relaxed);
    }
}

/// Logs one `inert:` line and counts it.
pub fn note(text: &str) {
    COUNT.fetch_add(1, Ordering::Relaxed);
    tracing::warn!("inert: {text}");
    if let Ok(mut lines) = LINES.lock() {
        lines.push(format!("inert: {text}"));
    }
}

/// True (after logging) when an effect that opens a device or ends the session must not run.
pub fn skip(what: &str) -> bool {
    if !on() {
        return false;
    }
    note(&format!("skipped {what}"));
    true
}

/// How many refusals and skips so far.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub fn count() -> u64 {
    COUNT.load(Ordering::Relaxed)
}

/// Counts a request that passed (a read); the crawler counts it as the control having done something.
pub fn request_allowed() {
    REQUESTS.fetch_add(1, Ordering::Relaxed);
}

/// How many requests have passed so far.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub fn requests() -> u64 {
    REQUESTS.load(Ordering::Relaxed)
}

/// How many log lines exist.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub fn line_count() -> usize {
    LINES.lock().map(|lines| lines.len()).unwrap_or(0)
}

/// The log lines from `from` on.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub fn lines_since(from: usize) -> Vec<String> {
    LINES
        .lock()
        .map(|lines| lines.get(from..).unwrap_or_default().to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::net::{Inert, Request};

    #[test]
    fn requests_are_classified_default_deny() {
        let method = |method: reqwest::Method| Request::Rest {
            key: None,
            method,
            route: "/channels/1/messages".into(),
            body: None,
        };
        assert_eq!(method(reqwest::Method::GET).inert(), Inert::Allow);
        assert_eq!(method(reqwest::Method::POST).inert(), Inert::Deny);
        assert_eq!(method(reqwest::Method::DELETE).inert(), Inert::Deny);
        let cases = [
            (
                Request::Spotify(crate::spotify::Command::Perform {
                    request: 1,
                    operation: crate::spotify::Operation::Premium("fixture".into()),
                }),
                Inert::Deny,
            ),
            (
                Request::Spotify(crate::spotify::Command::Refresh),
                Inert::Deny,
            ),
            (
                Request::Spotify(crate::spotify::Command::Accounts(vec![
                    dc_http::spotify::LinkedAccount {
                        id: "fixture".into(),
                        show_activity: true,
                    },
                ])),
                Inert::Deny,
            ),
            (Request::Typing { channel: 1 }, Inert::Deny),
            (
                Request::Delete {
                    channel: 1,
                    message: 2,
                },
                Inert::Deny,
            ),
            (
                Request::Pin {
                    channel: 1,
                    message: 2,
                    pin: true,
                },
                Inert::Deny,
            ),
            (
                Request::Subscribe {
                    subscriptions: serde_json::Map::new(),
                },
                Inert::Allow,
            ),
            (Request::LoadMessages { channel: 1 }, Inert::Allow),
            (
                Request::Image {
                    key: "k".into(),
                    url: "u".into(),
                },
                Inert::Allow,
            ),
            (Request::EmojiSheet, Inert::Allow),
            (
                Request::Presence {
                    status: "online".into(),
                    activities: Vec::new(),
                    since: 0,
                    afk: false,
                },
                Inert::Deny,
            ),
        ];
        for (request, want) in cases {
            assert_eq!(request.inert(), want, "{}", request.variant());
        }
    }
}
