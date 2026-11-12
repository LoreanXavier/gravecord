//! The WebAuthn calls of the login's security key slide and of Settings > Security Keys, on a thread of their own
//! (the system prompt blocks until the person answers it).
//!
//! The real call is made only by a shipped build. Under the `capture` feature, and in tests, a stub answers instead,
//! because the system's prompt is drawn by another process and could appear on a real desktop: `cancel` (the
//! default: the person closed the prompt), `wait` (the prompt stays open and never answers) or `ok` (a made-up
//! credential), chosen by `GRAVECORD_WEBAUTHN_STUB`.

use gpui::Window;
use tokio::sync::oneshot;

pub type Answer = Result<String, dc_webauthn::Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `navigator.credentials.get`: the login's second factor.
    Get,
    /// `navigator.credentials.create`: a new security key or passkey.
    Create,
}

/// Whether this build may ask the system for a prompt.
pub fn real_calls() -> bool {
    !cfg!(feature = "capture") && !cfg!(test)
}

/// The handle of the window the system prompt belongs to (0 when the window has none, as in the test platform).
pub fn parent(window: &Window) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(),
        _ => 0,
    }
}

/// Starts the call; the receiver gets the credential JSON or the reason there is none.
pub fn start(kind: Kind, window: isize, options: String) -> oneshot::Receiver<Answer> {
    let (sender, receiver) = oneshot::channel();
    if !real_calls() {
        stub(kind, sender);
        return receiver;
    }
    std::thread::spawn(move || {
        let answer = match kind {
            Kind::Get => dc_webauthn::get_assertion(window, &options),
            Kind::Create => dc_webauthn::create_credential(window, &options),
        };
        if let Err(error) = &answer {
            tracing::warn!("webauthn {kind:?}: {error}");
        }
        let _ = sender.send(answer);
    });
    receiver
}

/// The stub's answer after a short pause, as a person would take a moment.
fn stub(kind: Kind, sender: oneshot::Sender<Answer>) {
    let mode = std::env::var("GRAVECORD_WEBAUTHN_STUB").unwrap_or_default();
    crate::inert::note(&format!("webauthn {kind:?} stubbed ({mode:?})"));
    if mode == "wait" {
        // The prompt never closes: the sender stays alive for the rest of the run.
        std::mem::forget(sender);
        return;
    }
    let answer = if mode == "ok" {
        Ok(made_up_credential())
    } else {
        Err(dc_webauthn::Error::Cancelled)
    };
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let _ = sender.send(answer);
    });
}

/// A credential in the shape the API takes, with made-up bytes (capture builds only).
fn made_up_credential() -> String {
    serde_json::json!({
        "authenticatorAttachment": "cross-platform",
        "clientExtensionResults": {},
        "id": "AQIDBA",
        "rawId": "AQIDBA",
        "response": {"authenticatorData": "AQ", "clientDataJSON": "e30", "signature": "AQ"},
        "type": "public-key",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_never_ask_the_system() {
        assert!(!real_calls());
    }

    #[test]
    fn the_stub_answers_cancelled() {
        let mut receiver = start(Kind::Get, 0, "{}".to_owned());
        let answer = loop {
            match receiver.try_recv() {
                Ok(answer) => break answer,
                Err(oneshot::error::TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(error) => panic!("{error}"),
            }
        };
        assert_eq!(answer, Err(dc_webauthn::Error::Cancelled));
    }
}
