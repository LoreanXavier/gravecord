//! Login: password, second factor, captcha, forgot password, QR (remote auth), and the OS credential store.
//! The screens are `app/src/ui/login.rs`; everything that talks to Discord or to the credential store is here, so
//! it is tested against fake servers and without a window.

// `Failure` carries the form errors and the captcha challenge by value; a login failure is rare and the controller
// matches on it directly, so boxing would only add noise at every call site.
#![allow(clippy::result_large_err)]

pub mod api;
pub mod credentials;
pub mod errors;
pub mod flow;
pub mod qr;
pub mod remote;

pub use api::{MfaChallenge, MfaKind, MfaMethod};
pub use errors::{CaptchaAnswer, CaptchaChallenge, Failure, FormErrors};
pub use flow::{Command, Config, Event, spawn};
pub use remote::{RemoteEvent, RemoteUser};
