//! Small chat-side dialogs and the thread requests that go one after the other.
//!
//! - The alert of module 579872 `show`: a small modal with a title, the body as its subtitle and one "Okay" (or the
//!   given confirm text), with an optional secondary button. Pin this post? (module 41402) is one.
//! - Thread updates of module 7909 that take two requests in a row (`lockThread` of an archived thread unarchives
//!   it first; replacing the forum's pinned post unpins the old one before the new one is pinned): each waits for the
//!   one before it to succeed.

use super::Root;
use super::modal::{self, Tone};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, px};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

/// The key of a step's answer: `thread-step:{chain}`.
pub(super) const KEY_PREFIX: &str = "thread-step:";

/// `ChannelFlags.PINNED` (module 746080 `lx`).
pub(super) const PINNED: u64 = 1 << 1;

/// What the alert's buttons do besides closing it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Then {
    Close,
    /// Module 41402's `onConfirm`: `await unpin(old)`, then `pin(post)`.
    ReplacePin {
        post: Id,
        old: Id,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AlertButton {
    pub id: &'static str,
    pub label: String,
    pub secondary: bool,
    pub then: Then,
}

/// One open alert (module 579872 `show`).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Alert {
    pub id: &'static str,
    pub title: String,
    pub body: Option<String>,
    pub buttons: Vec<AlertButton>,
}

impl Alert {
    /// `show({title, body, confirmText?, cancelText?, onConfirm})`: the cancel button (when given) first, then the
    /// confirm button, "Okay" unless named.
    pub(super) fn show(
        id: &'static str,
        title: impl Into<String>,
        body: Option<String>,
        cancel: Option<String>,
        confirm: Option<String>,
        then: Then,
    ) -> Self {
        let mut buttons = Vec::new();
        if let Some(cancel) = cancel.filter(|text| !text.is_empty()) {
            buttons.push(AlertButton {
                id: "alert-cancel",
                label: cancel,
                secondary: true,
                then: Then::Close,
            });
        }
        buttons.push(AlertButton {
            id: "alert-confirm",
            label: confirm.unwrap_or_else(|| dc_intl::tr("Okay").to_owned()),
            secondary: false,
            then,
        });
        Self {
            id,
            title: title.into(),
            body,
            buttons,
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    pub alert: Option<Alert>,
    /// A right press on a `#channel` mention: where, and the channel (read by the message menu's opener).
    pub mention_hit: Option<(gpui::Point<gpui::Pixels>, Id)>,
    /// The requests still to go of each chain, by chain number.
    chains: HashMap<u64, VecDeque<(Id, Value)>>,
    next_chain: u64,
}

/// A thread action of the context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThreadStep {
    Close,
    Open,
    Lock,
    Unlock,
    Pin,
    Unpin,
}

/// The `PATCH /channels/{thread}` bodies module 7909 sends for an action, in order.
///
/// - `archiveThread(e, false)`: `{archived: true}`; `unarchiveThread(e, false)`: `{archived: false}`.
/// - `lockThread`: an archived thread is first unarchived, then `{locked: true, archived: <was archived>}`.
/// - `unlockThread`: an archived thread is first `unarchiveThread(e, true)` (`{archived: false, locked: false}`),
///   then `{locked: false, archived: <was archived>}`.
/// - `pin`: `updateFlags(flags | PINNED, isArchived)`: `{flags}`, with `archived: false` for an archived post;
///   `unpin`: `{flags: flags & ~PINNED}`.
pub(super) fn thread_bodies(step: ThreadStep, archived: bool, flags: u64) -> Vec<Value> {
    match step {
        ThreadStep::Close => vec![json!({"archived": true})],
        ThreadStep::Open => vec![json!({"archived": false})],
        ThreadStep::Lock => {
            let mut out = Vec::new();
            if archived {
                out.push(json!({"archived": false}));
            }
            out.push(json!({"locked": true, "archived": archived}));
            out
        }
        ThreadStep::Unlock => {
            let mut out = Vec::new();
            if archived {
                out.push(json!({"archived": false, "locked": false}));
            }
            out.push(json!({"locked": false, "archived": archived}));
            out
        }
        ThreadStep::Pin => {
            let mut body = json!({"flags": flags | PINNED});
            if archived {
                body["archived"] = json!(false);
            }
            vec![body]
        }
        ThreadStep::Unpin => vec![json!({"flags": flags & !PINNED})],
    }
}

impl Root {
    /// The thread as the store has it: (archived, flags).
    fn thread_state(&self, thread: Id) -> Option<(bool, u64)> {
        self.stores
            .guilds
            .iter()
            .find_map(|guild| guild.threads.iter().find(|known| known.id == thread))
            .map(|known| (known.archived, known.flags))
    }

    /// Runs a context menu thread action; the bodies go one after the other.
    pub(super) fn run_thread_step(&mut self, thread: Id, step: ThreadStep) {
        let (archived, flags) = self.thread_state(thread).unwrap_or_default();
        let steps = thread_bodies(step, archived, flags)
            .into_iter()
            .map(|body| (thread, body))
            .collect();
        if matches!(step, ThreadStep::Pin | ThreadStep::Unpin) {
            // `updateFlags` dispatches the merged THREAD_UPDATE before its request.
            self.set_thread_flags(
                thread,
                if step == ThreadStep::Pin {
                    flags | PINNED
                } else {
                    flags & !PINNED
                },
            );
        }
        self.send_thread_chain(steps);
    }

    fn set_thread_flags(&mut self, thread: Id, flags: u64) {
        for guild in &mut self.stores.guilds {
            if let Some(known) = guild.threads.iter_mut().find(|known| known.id == thread) {
                known.flags = flags;
            }
        }
    }

    /// The forum's pinned post other than `post` (`Eh` of module 435470: the first thread of the parent with the
    /// PINNED flag).
    pub(super) fn pinned_post_of_parent(&self, post: Id) -> Option<Id> {
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.threads.iter().any(|known| known.id == post))?;
        let parent = guild
            .threads
            .iter()
            .find(|known| known.id == post)?
            .parent_id;
        guild
            .threads
            .iter()
            .find(|known| known.parent_id == parent && known.id != post && known.pinned())
            .map(|known| known.id)
    }

    /// Pin Post (module 41402): with another post pinned already, "Pin this post?" asks first.
    pub(super) fn pin_post(&mut self, post: Id) {
        match self.pinned_post_of_parent(post) {
            Some(old) => {
                self.chat_extra.alert = Some(Alert::show(
                    "pin-post-alert",
                    dc_intl::tr("Pin this post?"),
                    Some(dc_intl::t!("mi5+Vl").to_owned()),
                    None,
                    None,
                    Then::ReplacePin { post, old },
                ));
            }
            None => self.run_thread_step(post, ThreadStep::Pin),
        }
    }

    fn send_thread_chain(&mut self, steps: VecDeque<(Id, Value)>) {
        let chain = self.chat_extra.next_chain;
        self.chat_extra.next_chain += 1;
        self.chat_extra.chains.insert(chain, steps);
        self.send_next_step(chain);
    }

    fn send_next_step(&mut self, chain: u64) {
        let Some(steps) = self.chat_extra.chains.get_mut(&chain) else {
            return;
        };
        let Some((thread, body)) = steps.pop_front() else {
            self.chat_extra.chains.remove(&chain);
            return;
        };
        let last = steps.is_empty();
        if last {
            self.chat_extra.chains.remove(&chain);
        }
        let _ = self.requests.send(Request::Rest {
            key: (!last).then(|| format!("{KEY_PREFIX}{chain}")),
            method: reqwest::Method::PATCH,
            route: format!("/channels/{thread}"),
            body: Some(body),
        });
    }

    /// A step's answer: the next one goes when it succeeded, else the chain stops.
    pub(super) fn thread_step_fetched(&mut self, key: &str, body: &Value) {
        let Some(chain) = key
            .strip_prefix(KEY_PREFIX)
            .and_then(|chain| chain.parse().ok())
        else {
            return;
        };
        if body["ok"].as_bool() == Some(true) {
            self.send_next_step(chain);
        } else {
            self.chat_extra.chains.remove(&chain);
        }
    }

    fn alert_then(&mut self, then: Then) {
        self.chat_extra.alert = None;
        match then {
            Then::Close => {}
            Then::ReplacePin { post, old } => {
                let (old_archived, old_flags) = self.thread_state(old).unwrap_or_default();
                let (archived, flags) = self.thread_state(post).unwrap_or_default();
                self.set_thread_flags(old, old_flags & !PINNED);
                self.set_thread_flags(post, flags | PINNED);
                let mut steps: VecDeque<(Id, Value)> =
                    thread_bodies(ThreadStep::Unpin, old_archived, old_flags)
                        .into_iter()
                        .map(|body| (old, body))
                        .collect();
                steps.extend(
                    thread_bodies(ThreadStep::Pin, archived, flags)
                        .into_iter()
                        .map(|body| (post, body)),
                );
                self.send_thread_chain(steps);
            }
        }
    }

    /// Escape closes the open alert; true when there was one.
    pub(super) fn chat_alert_escape(&mut self) -> bool {
        self.chat_extra.alert.take().is_some()
    }

    pub(super) fn chat_alert_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let alert = self.chat_extra.alert.clone()?;
        let buttons = alert
            .buttons
            .iter()
            .cloned()
            .map(|button| {
                let then = button.then.clone();
                modal::button(
                    button.id,
                    button.label,
                    if button.secondary {
                        Tone::Secondary
                    } else {
                        Tone::Primary
                    },
                    move |this, _, _| this.alert_then(then.clone()),
                )
            })
            .collect();
        Some(modal::render_small(
            modal::Spec {
                id: alert.id,
                title: alert.title.into(),
                subtitle: alert.body.map(|body| div().child(body).into_any_element()),
                body: None,
                buttons,
                close: Box::new(|this, _, _| this.chat_extra.alert = None),
            },
            cx,
        ))
    }
}

/// The address Copy Link of a channel mention copies (module 147036 `af` with no thread: `O(guild, channel, message)`,
/// the `CHANNEL` route on the app's host; a DM is `@me`).
pub(super) fn channel_link(guild: Option<Id>, channel: Id, message: Option<Id>) -> String {
    let guild = guild.map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
    match message {
        Some(message) => format!("https://discord.com/channels/{guild}/{channel}/{message}"),
        None => format!("https://discord.com/channels/{guild}/{channel}"),
    }
}

/// The body text of a dialog (`body-md/normal`, `text-default`).
#[allow(dead_code)]
pub(super) fn body_text(text: impl Into<gpui::SharedString>) -> AnyElement {
    div()
        .text_size(px(16.))
        .line_height(px(20.))
        .text_color(super::color("--text-default"))
        .child(text.into())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locking_an_archived_thread_unarchives_it_first() {
        assert_eq!(
            thread_bodies(ThreadStep::Lock, true, 0),
            vec![
                json!({"archived": false}),
                json!({"locked": true, "archived": true})
            ]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Lock, false, 0),
            vec![json!({"locked": true, "archived": false})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Unlock, true, 0),
            vec![
                json!({"archived": false, "locked": false}),
                json!({"locked": false, "archived": true})
            ]
        );
    }

    #[test]
    fn pinning_sets_the_flag_and_opens_an_archived_post() {
        assert_eq!(
            thread_bodies(ThreadStep::Pin, false, 1 << 4),
            vec![json!({"flags": (1 << 4) | 2})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Pin, true, 0),
            vec![json!({"flags": 2, "archived": false})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Unpin, true, 2 | 16),
            vec![json!({"flags": 16})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Close, false, 0),
            vec![json!({"archived": true})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Open, true, 0),
            vec![json!({"archived": false})]
        );
    }

    #[test]
    fn a_channel_mention_link_names_its_server_or_me() {
        assert_eq!(
            channel_link(Some(1), 2, None),
            "https://discord.com/channels/1/2"
        );
        assert_eq!(
            channel_link(None, 2, Some(3)),
            "https://discord.com/channels/@me/2/3"
        );
    }

    #[test]
    fn the_alert_has_okay_unless_named_and_cancel_first() {
        let alert = Alert::show("a", "T", None, None, None, Then::Close);
        assert_eq!(alert.buttons.len(), 1);
        assert_eq!(alert.buttons[0].label, "Okay");
        let alert = Alert::show(
            "a",
            "T",
            None,
            Some("Cancel".into()),
            Some("Go".into()),
            Then::Close,
        );
        assert!(alert.buttons[0].secondary);
        assert_eq!(alert.buttons[1].label, "Go");
    }
}
