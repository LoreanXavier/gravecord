//! ValidUser (Vencord `src/plugins/validUser`, GPL-3.0-or-later): a mention of a user the client does not know shows
//! "@unknown-user"; with the plugin on, the pointer on it fetches the user (`GET /users/<id>`, a read) and the mention
//! then shows the name (`USER_UPDATE`). Requests go through a queue newest first (`queue.unshift`), one at a time with
//! `sleep(300)` after each (the original's `Queue(5)` runs them one by one too). A user asked for is not asked for
//! again, whatever the answer (the original's `fetching` keeps a refused id); rate limits are retried by the HTTP
//! client as for every request.
//!
//! GAP: the original also fills the profile it cannot fetch with badges deduced from the user's flags
//! (`USER_PROFILE_FETCH_FAILURE` + `fakeBadges`); the popout of such a user here shows what the profile route gives.
//! Not part of the pinned client.

use super::super::Root;
use dc_state::Id;
use gpui::Context;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

const ID: &str = "ValidUser";
/// The fetched user's key: `<prefix><id>`.
const KEY: &str = "plugin-valid-user:";
/// `.finally(() => sleep(300))`.
const SPACING: Duration = Duration::from_millis(300);

thread_local! {
    /// Users waiting their turn, the newest first; and whether the queue is being worked through.
    static QUEUE: RefCell<(VecDeque<Id>, bool)> = const { RefCell::new((VecDeque::new(), false)) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

impl Root {
    /// Per piece of `spans`: the unknown user a mention names, while the plugin is on; `None` (nothing to wire) while
    /// it is off or no mention is unknown.
    pub(in crate::ui) fn valid_user_unknown(
        &self,
        spans: &[dc_markdown::Span],
    ) -> Option<std::rc::Rc<Vec<Option<Id>>>> {
        if !on() {
            return None;
        }
        let unknown: Vec<Option<Id>> = spans
            .iter()
            .map(|span| match span.kind {
                dc_markdown::Kind::User(id) if !self.user_known(id) => Some(id),
                _ => None,
            })
            .collect();
        unknown
            .iter()
            .any(Option::is_some)
            .then(|| std::rc::Rc::new(unknown))
    }

    fn user_known(&self, id: Id) -> bool {
        self.stores.users.contains_key(&id)
            || self
                .stores
                .current_user
                .as_ref()
                .is_some_and(|me| me.id == id)
    }

    /// The pointer is on the mention of `id`: queues its fetch unless it is known, queued or asked for.
    pub(in crate::ui) fn valid_user_hovered(&mut self, id: Id, cx: &mut Context<Self>) {
        if !on() || self.user_known(id) || self.requested.contains(&format!("{KEY}{id}")) {
            return;
        }
        let fresh = QUEUE.with(|queue| {
            let mut queue = queue.borrow_mut();
            if queue.0.contains(&id) {
                return false;
            }
            queue.0.push_front(id);
            !std::mem::replace(&mut queue.1, true)
        });
        if !fresh {
            return;
        }
        tracing::info!("ValidUser: fetching unknown mentioned user {id}");
        cx.spawn(async move |this, cx| {
            while let Some(id) = QUEUE.with(|queue| queue.borrow_mut().0.pop_front()) {
                let _ = this.update(cx, |this, _| {
                    let key = format!("{KEY}{id}");
                    if !this.user_known(id) && this.requested.insert(key.clone()) {
                        let _ = this.requests.send(crate::net::Request::Fetch {
                            key,
                            route: format!("/users/{id}"),
                        });
                    }
                });
                cx.background_executor().timer(SPACING).await;
            }
            QUEUE.with(|queue| queue.borrow_mut().1 = false);
        })
        .detach();
    }

    /// A fetched user joins the users (`USER_UPDATE`), so its mentions show the name. True when the key was ours.
    pub(in crate::ui) fn valid_user_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = key.strip_prefix(KEY) else {
            return false;
        };
        if body["id"].as_str() == Some(rest) {
            self.stores.merge_user(body);
            cx.notify();
        } else {
            tracing::info!("ValidUser: no user in the answer for {rest}");
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_carry_the_user() {
        assert_eq!(format!("{KEY}{}", 42), "plugin-valid-user:42");
    }
}
