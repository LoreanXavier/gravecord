//! The sign-in of a plugin to its own web service (Decor, Streaks; their `AuthorizationStore`): the client's OAuth2
//! consent modal for the service's application, with the service's redirect (`oauth_begin_caller`), then the code of
//! the answered location traded for the service's token. Not part of the pinned client.
//!
//! The tokens are the account's, one per service and account (the original's `tokens[userId]`): kept in the system
//! credential store under `gravecord-plugins`, never in `storage.json`, a backup or a log. A capture build keeps them
//! in memory only (`plugin token <Id> fake` puts a stand-in there) and never trades a code: the modal posts no grant
//! and an inert run refuses the trade.

use super::super::Root;
use dc_state::Id;
use gpui::Context;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// The credential store's service name for the plugins' tokens.
const STORE: &str = "gravecord-plugins";

thread_local! {
    /// `(service, account)`: the token, `None` once read and absent.
    static TOKENS: RefCell<HashMap<(&'static str, Id), Option<String>>> = RefCell::default();
    static LOADING: RefCell<HashSet<(&'static str, Id)>> = RefCell::default();
}

fn entry(service: &str, user: Id) -> String {
    format!("{service}-{user}")
}

/// The account's token for `service`, once read.
pub(in crate::ui) fn token(service: &'static str, user: Id) -> Option<String> {
    TOKENS.with(|tokens| tokens.borrow().get(&(service, user)).cloned().flatten())
}

/// Whether the store was read for this account (a missing token is known to be missing).
pub(in crate::ui) fn token_known(service: &'static str, user: Id) -> bool {
    TOKENS.with(|tokens| tokens.borrow().contains_key(&(service, user)))
}

/// A stand-in token held in memory only (capture builds' `plugin token <Id> fake`), or none.
#[cfg(feature = "capture")]
fn set_capture_token(service: &'static str, user: Id, token: Option<String>) {
    TOKENS.with(|tokens| tokens.borrow_mut().insert((service, user), token));
}

impl Root {
    /// Capture only: `plugin token <Id> fake|clear`: a stand-in token for the plugin's service, in memory.
    #[cfg(feature = "capture")]
    pub(in crate::ui) fn script_service_token(
        &mut self,
        id: &str,
        fake: bool,
        cx: &mut Context<Self>,
    ) -> String {
        let (service, ready): (&'static str, fn(&mut Root, &mut Context<Root>)) = match id {
            "Decor" => (super::decor::SERVICE, |this, cx| this.decor_token_ready(cx)),
            "Streaks" => (super::streaks::SERVICE, |this, cx| {
                this.streaks_token_ready(cx)
            }),
            _ => return format!("token: {id} has no service"),
        };
        let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
            return "token: no account".to_owned();
        };
        set_capture_token(service, me, fake.then(|| "capture-stand-in".to_owned()));
        ready(self, cx);
        cx.notify();
        format!("token: {id} {}", if fake { "stand-in" } else { "cleared" })
    }

    /// Reads the account's token for `service` off the UI thread, once; `ready` runs when it is known.
    pub(in crate::ui) fn service_token_load(
        &mut self,
        service: &'static str,
        user: Id,
        ready: fn(&mut Root, &mut Context<Root>),
        cx: &mut Context<Self>,
    ) {
        if token_known(service, user)
            || !LOADING.with(|loading| loading.borrow_mut().insert((service, user)))
        {
            return;
        }
        if cfg!(feature = "capture") {
            LOADING.with(|loading| loading.borrow_mut().remove(&(service, user)));
            TOKENS.with(|tokens| {
                tokens.borrow_mut().entry((service, user)).or_insert(None);
            });
            ready(self, cx);
            return;
        }
        let name = entry(service, user);
        let read = cx.background_executor().spawn(async move {
            dc_auth::credentials::Store::new(STORE)
                .load(&name)
                .ok()
                .flatten()
                .map(|token| token.expose().to_owned())
        });
        cx.spawn(async move |this, cx| {
            let found = read.await;
            let _ = this.update(cx, |this, cx| {
                LOADING.with(|loading| loading.borrow_mut().remove(&(service, user)));
                TOKENS.with(|tokens| tokens.borrow_mut().insert((service, user), found));
                ready(this, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// `setToken`: kept for this account, and written to the credential store off the UI thread.
    pub(in crate::ui) fn service_token_set(
        &mut self,
        service: &'static str,
        user: Id,
        token: String,
    ) {
        TOKENS.with(|tokens| {
            tokens
                .borrow_mut()
                .insert((service, user), Some(token.clone()))
        });
        if cfg!(feature = "capture") {
            return;
        }
        let name = entry(service, user);
        std::thread::spawn(move || {
            let token = dc_proto::secrets::Token::new(token);
            if dc_auth::credentials::Store::new(STORE)
                .save(&name, &token)
                .is_err()
            {
                tracing::warn!("a plugin service token was not stored");
            }
        });
    }

    /// `remove(userId)`: forgotten here and in the credential store.
    pub(in crate::ui) fn service_token_forget(&mut self, service: &'static str, user: Id) {
        TOKENS.with(|tokens| tokens.borrow_mut().insert((service, user), None));
        if cfg!(feature = "capture") {
            return;
        }
        let name = entry(service, user);
        std::thread::spawn(move || {
            let _ = dc_auth::credentials::Store::new(STORE).forget(&name);
        });
    }

    /// `authorize()`: the consent modal for the service's application (`identify`, no permissions) with its redirect.
    pub(in crate::ui) fn service_sign_in(
        &mut self,
        service: &'static str,
        client_id: String,
        redirect_uri: String,
        cx: &mut Context<Self>,
    ) {
        self.oauth_begin_caller(
            service,
            client_id,
            redirect_uri,
            vec!["identify".to_owned()],
            cx,
        );
    }

    /// The modal's answer for `caller` (`oauth_finish`): a code goes to its plugin, which trades it; a refusal or an
    /// error shows as the original's "Failed to authorize" toast, a cancel says nothing.
    pub(in crate::ui) fn service_auth_answered(
        &mut self,
        caller: &str,
        result: Result<Value, (u32, String)>,
        cx: &mut Context<Self>,
    ) {
        let code = match result {
            Ok(answer) => answer["code"].as_str().map(str::to_owned),
            Err((_, message)) => {
                if message != "User cancelled authorization" {
                    self.put_app_toast(format!("Failed to authorize: {message}"), false, cx);
                }
                None
            }
        };
        let Some(code) = code else {
            return;
        };
        match caller {
            super::decor::SERVICE => self.decor_code(code, cx),
            super::streaks::SERVICE => self.streaks_code(code, cx),
            _ => {}
        }
    }
}
