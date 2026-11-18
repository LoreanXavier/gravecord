//! WhoReacted (Vencord `src/plugins/whoReacted`, GPL-3.0-or-later): after a reaction's count, the avatars of the people
//! who reacted (`UserSummaryItem`, five at most, at 90% and half an em after the count). Each reaction's first hundred
//! reactors are fetched once (`GET …/reactions/<emoji>?limit=100&type=<type>`, a read), one request every 250 ms, the
//! newest first (`queue.unshift`), and merged into the users as the original's `USER_UPDATE` does. Nothing is drawn on a
//! message with more than ten reactions. With `clickableAvatars` an avatar opens the person's profile instead of
//! reacting. Not part of the pinned client.

use super::super::{Root, avatar_source, color};
use crate::plugins::State;
use dc_state::{Id, Message, User};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, img, px,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

const ID: &str = "WhoReacted";
/// The fetched reactors' key: `<prefix><message>:<emoji>:<type>`.
const KEY: &str = "plugin-who-reacted:";
/// `UserSummaryItem` `max={5}`.
const MAX: usize = 5;
/// `sleep(250)` after each request.
const SPACING: Duration = Duration::from_millis(250);

thread_local! {
    /// Requests waiting their turn, the newest first; and whether the queue is being worked through.
    static QUEUE: RefCell<(VecDeque<(String, String)>, bool)> = const { RefCell::new((VecDeque::new(), false)) };
}

/// `clickableAvatars`, while the plugin is on.
fn read(state: &State) -> Option<bool> {
    state
        .is_on(ID)
        .then(|| state.value(ID, "clickableAvatars") != Value::Bool(false))
}

/// What `UserSummaryItem` draws for `count` people at most `MAX`: the avatars shown and the `+N` after them.
pub(super) fn summary(count: usize) -> (usize, Option<usize>) {
    if count > MAX {
        (MAX - 1, Some(count - (MAX - 1)))
    } else {
        (count, None)
    }
}

fn key(message: Id, emoji: &str) -> String {
    format!("{KEY}{message}:{emoji}:0")
}

impl Root {
    /// The avatars after the count of the reaction `emoji_route` names, or nothing. The first call for a reaction queues its fetch.
    pub(in crate::ui) fn who_reacted(
        &self,
        message: &Message,
        emoji_route: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if crate::plugins::bypassed() {
            return None;
        }
        let clickable = crate::plugins::with(read)?;
        if message.reactions.len() > 10 {
            return None;
        }
        let key = key(message.id, emoji_route);
        let Some(body) = self.fetched.get(&key) else {
            if !self.requested.contains(&key) {
                let route = format!(
                    "/channels/{}/messages/{}/reactions/{}?limit=100&type=0",
                    message.channel_id,
                    message.id,
                    super::super::reactions_modal::encode(emoji_route),
                );
                let fresh = QUEUE.with(|queue| {
                    let mut queue = queue.borrow_mut();
                    if queue.0.iter().any(|(queued, _)| *queued == key) {
                        return false;
                    }
                    queue.0.push_front((key.clone(), route));
                    !std::mem::replace(&mut queue.1, true)
                });
                if fresh {
                    cx.spawn(async move |this, cx| {
                        while let Some((key, route)) =
                            QUEUE.with(|queue| queue.borrow_mut().0.pop_front())
                        {
                            let _ = this.update(cx, |this, _| {
                                if this.requested.insert(key.clone()) {
                                    let _ = this
                                        .requests
                                        .send(crate::net::Request::Fetch { key, route });
                                }
                            });
                            cx.background_executor().timer(SPACING).await;
                        }
                        QUEUE.with(|queue| queue.borrow_mut().1 = false);
                    })
                    .detach();
                }
            }
            return None;
        };
        let users: Vec<User> = body
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(User::from_value)
            .collect();
        if users.is_empty() {
            return None;
        }
        let (shown, more) = summary(users.len());
        // `transform: scale(0.9)` of 16px avatars.
        let side = 16. * 0.9;
        let avatars = users.iter().take(shown).enumerate().map(|(index, user)| {
            let (image_key, url) = avatar_source(user);
            let picture = self.images.get(&image_key).cloned();
            if picture.is_none() {
                self.want_image(image_key, url, cx);
            }
            let face = div()
                .size(px(side))
                .rounded_full()
                .overflow_hidden()
                .bg(color("--background-mod-normal"))
                .children(picture.map(|picture| img(picture).size_full().rounded_full()));
            let face = if clickable {
                // `showUserPopout`; the press stops at the avatar (`handleClickAvatar`).
                self.profile_trigger(
                    format!("who-reacted-{}-{}-{index}", message.id, user.id),
                    user.id,
                    face,
                    cx,
                )
            } else {
                face.into_any_element()
            };
            div()
                .id(SharedString::from(format!(
                    "who-reacted-{}-{}",
                    message.id, user.id
                )))
                .when(index > 0, |this| this.ml(px(-3.6)))
                .rounded_full()
                .border_1()
                .border_color(color("--background-base-lower"))
                .when(clickable, |this| {
                    this.on_click(|_, _, cx| cx.stop_propagation())
                })
                .child(face)
        });
        Some(
            div()
                .ml(px(8.))
                .flex()
                .items_center()
                .children(avatars)
                .children(more.map(|more| {
                    div()
                        .ml(px(-3.6))
                        .h(px(side))
                        .px(px(4.))
                        .rounded_full()
                        .bg(color("--background-mod-strong"))
                        .text_size(px(10.8))
                        .line_height(px(side))
                        .text_color(color("--text-default"))
                        .child(format!("+{more}"))
                }))
                .into_any_element(),
        )
    }

    /// The reactors of a fetch, merged into the users (`USER_UPDATE`); the body stays in `fetched` for the avatars.
    pub(in crate::ui) fn who_reacted_fetched(&mut self, key: &str, body: &Value) {
        if !key.starts_with(KEY) {
            return;
        }
        for user in body.as_array().into_iter().flatten() {
            self.stores.merge_user(user);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn five_at_most_with_the_rest_counted() {
        assert_eq!(summary(0), (0, None));
        assert_eq!(summary(5), (5, None));
        assert_eq!(summary(6), (4, Some(2)));
        assert_eq!(summary(100), (4, Some(96)));
        assert_eq!(key(7, "\u{1f44d}"), "plugin-who-reacted:7:\u{1f44d}:0");
        let mut state = State::default();
        assert_eq!(read(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(read(&state), Some(true));
        state.set_value(ID, "clickableAvatars", &json!(false));
        assert_eq!(read(&state), Some(false));
    }
}
