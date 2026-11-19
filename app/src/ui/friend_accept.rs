//! Accepting a friend request the way module 393426 does it (`maybeConfirmFriendRequestAccept`, used by the friends
//! list, the request notice and the profile's buttons through module 390848).
//!
//! - Someone this client already knows is no stranger is accepted at once with `confirm_stranger_request: true`.
//! - Someone known to be a stranger first gets "Accept friend request?" (module 691464: "This may be someone you
//!   don't know..." with Cancel and Accept); Accept sends the request with `confirm_stranger_request: true`.
//! - Anyone else is accepted with `confirm_stranger_request: false`. When the answer is
//!   `RELATIONSHIP_INVALID_NO_CONFIRMATION` (80013) the person is marked a stranger (`UPDATE_STRANGER_STATUS`) and the
//!   dialog asks; any other success marks them no stranger.
//!
//! `isStranger` of `RelationshipStore` (module 994500) keeps each answer for five minutes.

use super::Root;
use super::chat_notices::{Alert, AlertButton, Then};
use super::modal::Tone;
use crate::net::Request;
use dc_state::Id;
use gpui::Context;
use serde_json::{Value, json};

/// The key of an accept's answer: `friend-accept:{user}`.
pub(super) const KEY_PREFIX: &str = "friend-accept:";

/// `RELATIONSHIP_INVALID_NO_CONFIRMATION` of module 652215.
const NO_CONFIRMATION: u64 = 80_013;

/// `UPDATE_STRANGER_STATUS` keeps its answer this long (`expiry: Date.now() + 3e5`).
const STRANGER_TTL_MS: i64 = 300_000;

/// `addRelationship` of module 49229 for an accept: no `type`, no `from_friend_suggestion`, the confirmation flag.
pub(super) fn accept_body(confirm: bool) -> Value {
    json!({ "confirm_stranger_request": confirm })
}

/// The answer asks for the stranger confirmation (`N` of module 393426).
pub(super) fn needs_confirmation(answer: &Value) -> bool {
    answer["ok"] != true && answer["body"]["code"].as_u64() == Some(NO_CONFIRMATION)
}

/// `isStranger` of module 994500: the kept answer, dropped once expired.
pub(super) fn stranger_status(kept: Option<(bool, i64)>, now_ms: i64) -> Option<bool> {
    let (stranger, expiry) = kept?;
    (expiry >= now_ms).then_some(stranger)
}

/// Module 691464 on the dialog of 189213.
pub(super) fn stranger_alert(user: Id) -> Alert {
    Alert {
        id: "friend-request-stranger",
        title: dc_intl::t("eJzSDT").to_owned(),
        subtitle: Some(vec![(dc_intl::t("GB4jUw").to_owned(), false)]),
        content: None,
        buttons: vec![
            AlertButton {
                id: "friend-request-stranger-cancel",
                label: dc_intl::t("ETE/oC").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "friend-request-stranger-accept",
                label: dc_intl::t("MMlhsr").to_owned(),
                tone: Tone::Primary,
                then: Then::AcceptStranger { user },
            },
        ],
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

impl Root {
    /// `maybeConfirmFriendRequestAccept` of module 393426 for a friend request (game friend requests take their own
    /// route and never ask).
    pub(super) fn accept_friend_request(&mut self, user: Id, cx: &mut Context<Self>) {
        let kept = self.chat_extra.strangers.get(&user).copied();
        match stranger_status(kept, now_ms()) {
            Some(false) => self.send_friend_accept(user, true),
            Some(true) => self.chat_extra.alert = Some(stranger_alert(user)),
            None => self.send_friend_accept(user, false),
        }
        cx.notify();
    }

    /// Accept of module 691464.
    pub(super) fn accept_stranger(&mut self, user: Id, cx: &mut Context<Self>) {
        self.send_friend_accept(user, true);
        cx.notify();
    }

    /// `PUT /users/@me/relationships/{user}`; the request turns into a friend at once (put back if the server asks
    /// for the confirmation).
    fn send_friend_accept(&mut self, user: Id, confirm: bool) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{KEY_PREFIX}{user}")),
            method: reqwest::Method::PUT,
            route: format!("/users/@me/relationships/{user}"),
            body: Some(accept_body(confirm)),
        });
        if self.stores.relationships.get(&user) == Some(&super::friends::RELATIONSHIP_INCOMING) {
            self.stores
                .relationships
                .insert(user, super::friends::RELATIONSHIP_FRIEND);
            self.chat_extra.accepting.insert(user);
        }
        self.stores.clear_relationship_request_flags(user);
        self.rebuild_rows();
    }

    /// The answer of an accept: 80013 puts the request back and asks; a success marks the person no stranger.
    pub(super) fn friend_accept_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(user) = key
            .strip_prefix(KEY_PREFIX)
            .and_then(|user| user.parse::<Id>().ok())
        else {
            return;
        };
        let optimistic = self.chat_extra.accepting.remove(&user);
        if needs_confirmation(body) {
            self.chat_extra
                .strangers
                .insert(user, (true, now_ms() + STRANGER_TTL_MS));
            if optimistic
                && self.stores.relationships.get(&user)
                    == Some(&super::friends::RELATIONSHIP_FRIEND)
            {
                self.stores
                    .relationships
                    .insert(user, super::friends::RELATIONSHIP_INCOMING);
                self.rebuild_rows();
            }
            self.chat_extra.alert = Some(stranger_alert(user));
            cx.notify();
        } else if body["ok"] == true {
            self.chat_extra
                .strangers
                .insert(user, (false, now_ms() + STRANGER_TTL_MS));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_accept_carries_the_confirmation_flag() {
        assert_eq!(
            accept_body(false),
            json!({"confirm_stranger_request": false})
        );
        assert_eq!(accept_body(true), json!({"confirm_stranger_request": true}));
    }

    #[test]
    fn only_80013_asks_for_the_confirmation() {
        assert!(needs_confirmation(
            &json!({"ok": false, "status": 400, "body": {"code": 80013, "message": "x"}})
        ));
        assert!(!needs_confirmation(
            &json!({"ok": false, "status": 400, "body": {"code": 80000}})
        ));
        assert!(!needs_confirmation(
            &json!({"ok": true, "status": 204, "body": null})
        ));
    }

    #[test]
    fn the_stranger_answer_lasts_five_minutes() {
        assert_eq!(stranger_status(None, 10), None);
        assert_eq!(stranger_status(Some((true, 1_000)), 999), Some(true));
        assert_eq!(stranger_status(Some((false, 1_000)), 1_000), Some(false));
        assert_eq!(stranger_status(Some((true, 1_000)), 1_001), None);
    }

    #[test]
    fn the_dialog_offers_cancel_and_accept() {
        let alert = stranger_alert(7);
        assert_eq!(alert.title, "Accept friend request?");
        assert_eq!(alert.buttons.len(), 2);
        assert!(matches!(
            alert.buttons[1].then,
            Then::AcceptStranger { user: 7 }
        ));
    }
}
