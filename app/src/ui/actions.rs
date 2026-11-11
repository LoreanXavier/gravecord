//! What the message menu's and hover bar's entries do, beyond the ones `menu.rs` handles inline.

use super::Root;
use super::menu::Action;
use dc_state::Message;
use dc_state::frecency::{Frecency, Kind, Usage};
use gpui::{Context, Window};

/// Settings-proto type 2, `FrecencyUserSettings`.
pub(super) const FRECENCY_KEY: &str = "settings-proto:2";

/// What module 121681 tracks for an account with no reaction history, in this order, all at once.
const DEFAULT_REACTIONS: [&str; 13] = [
    "100",
    "100",
    "thumbsup",
    "thumbsup",
    "thumbsdown",
    "thumbsdown",
    "heart",
    "point_up",
    "eyes",
    "weary",
    "laughing",
    "white_check_mark",
    "x",
];

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

impl Root {
    pub(super) fn request_frecency(&mut self) {
        let _ = self.requests.send(crate::net::Request::Fetch {
            key: FRECENCY_KEY.to_owned(),
            route: "/users/@me/settings-proto/2".to_owned(),
        });
    }

    /// `ef.frequently`: reaction emoji names by frecency, with this session's reactions tracked on top.
    pub(super) fn frequent_reactions(&self) -> Vec<String> {
        let mut frecency = Frecency::new(Kind::Reaction);
        let stored = self
            .fetched
            .get(FRECENCY_KEY)
            .and_then(|body| body["settings"].as_str())
            .map(|settings| {
                dc_proto::frecency::emoji_map(settings, dc_proto::frecency::EMOJI_REACTION_FRECENCY)
            })
            .unwrap_or_default();
        let now = now_ms();
        if stored.is_empty() {
            for name in DEFAULT_REACTIONS {
                frecency.track(name, now);
            }
        } else {
            frecency.overwrite(
                stored
                    .into_iter()
                    .map(|(key, item)| {
                        (
                            key,
                            Usage {
                                total_uses: item.total_uses,
                                recent_uses: item.recent_uses,
                            },
                        )
                    })
                    .collect(),
            );
        }
        for (name, at) in &self.reactions_used {
            frecency.track(name, *at);
        }
        // `lookupKey`: a unicode emoji by name, or a custom emoji this client knows.
        frecency.frequently(now, |key| {
            dc_tokens::emoji_by_name(key).is_some()
                || key
                    .parse::<u64>()
                    .is_ok_and(|id| self.stores.guilds.iter().any(|guild| guild.has_emoji(id)))
        })
    }

    pub(super) fn request_emoji_image(&mut self, surrogates: &str) {
        if let Some(file) = dc_tokens::emoji_file(surrogates) {
            let url = format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN);
            self.request_image(format!("emoji:{surrogates}"), url);
        }
    }

    /// Records a reaction for the frecency list (`track` on `ef`).
    pub(super) fn track_reaction(&mut self, surrogates: &str) {
        let name = dc_tokens::emoji_data::EMOJIS
            .iter()
            .find(|emoji| emoji.surrogates == surrogates)
            .map(|emoji| emoji.names[0].to_owned());
        if let Some(name) = name {
            self.reactions_used.push((name, now_ms()));
        }
    }

    /// Speak Message / Stop Speaking Message (`SPEAK_MESSAGE`, `STOP_SPEAKING`).
    fn speak_message(&mut self, message: Message, cx: &mut Context<Self>) {
        if self.speaking_message == Some(message.id) {
            self.speech.stop();
            self.speaking_message = None;
            cx.notify();
            return;
        }
        let users = &self.stores.users;
        let text = crate::speech::utterance(
            message.author.display_name(),
            &message.content,
            message
                .referenced
                .as_ref()
                .map(|referenced| referenced.author.display_name()),
            |id| {
                id.parse::<u64>()
                    .ok()
                    .and_then(|id| users.get(&id))
                    .map(|user| user.display_name().to_owned())
            },
        );
        self.speech.speak(text, self.speech_rate());
        self.speaking_message = Some(message.id);
        // Back to Speak Message once the voice falls silent.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(300))
                    .await;
                let done = this
                    .update(cx, |this, cx| {
                        if this.speech.is_speaking() {
                            return false;
                        }
                        this.speaking_message = None;
                        cx.notify();
                        true
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn run_message_action(
        &mut self,
        action: Action,
        message: Message,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Pin if message.pinned => self.confirm_unpin = Some(message),
            Action::Pin => self.confirm_pin = Some(message),
            Action::MarkUnread => self.mark_unread(message.channel_id, message.id),
            Action::Bookmark => self.toggle_bookmark(message.channel_id, message.id, cx),
            Action::Speak => self.speak_message(message, cx),
            Action::Forward => self.open_forward(message, window, cx),
            Action::Thread => self.open_new_thread_from(message, window, cx),
            Action::ViewReactions => self.open_view_reactions(message, cx),
            Action::AddReaction => {
                if let Some(super::menu::OpenMenu { anchor, .. }) = self.last_menu {
                    let at = match anchor {
                        super::menu::Anchor::Pointer(at) | super::menu::Anchor::MoreButton(at) => {
                            at
                        }
                    };
                    self.open_picker(
                        super::picker::Target::Reaction {
                            channel: message.channel_id,
                            message: message.id,
                        },
                        at,
                        window,
                        cx,
                    );
                }
            }
            _ => {
                let _ = (window, cx);
            }
        }
    }
}
