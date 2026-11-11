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

/// One info line per outside effect of a menu row: what it does and how long the payload is, never the payload.
/// In an inert run the effect itself is then skipped and gpui notes that on its own `inert:` line.
pub(super) fn log_effect(row: &str, effect: &str, length: usize) {
    tracing::info!(row, effect, length, "menu action effect");
    #[cfg(feature = "capture")]
    if let Ok(mut lines) = EFFECTS.lock() {
        lines.push(format!("effect: {row}: {effect}, {length} bytes"));
    }
}

/// Harness: the effect lines of the row that just ran, for the script's answer.
#[cfg(feature = "capture")]
static EFFECTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[cfg(feature = "capture")]
pub(super) fn take_effects() -> Vec<String> {
    EFFECTS
        .lock()
        .map(|mut lines| std::mem::take(&mut *lines))
        .unwrap_or_default()
}

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
            Action::EndPoll => self.poll_end_ask(message.channel_id, message.id),
            Action::Report => self.open_report_message(&message, false, window, cx),
            Action::CopySelection => {
                let text = self.menu_extra.selection.clone();
                log_effect("Copy", "clipboard text", text.len());
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            // Module 826308: the text goes into a Google search in the browser.
            Action::SearchGoogle => {
                let query = dc_http::percent_encode(&self.menu_extra.selection);
                log_effect("Search with Google", "open browser", query.len());
                cx.open_url(&format!("https://www.google.com/search?q={query}"));
            }
            Action::CopyUrl => {
                if let Some(url) = self.menu_url() {
                    log_effect("Copy Link", "clipboard text", url.len());
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
                    self.show_success_toast("Link Copied!".into(), cx);
                }
            }
            // `^(tel|sms|mailto):([^?;]+)`: what follows the scheme, up to a query or a parameter.
            Action::CopyContact => {
                if let Some(url) = self.menu_url()
                    && let Some((_, rest)) = url.split_once(':')
                {
                    let address = rest.split(['?', ';']).next().unwrap_or(rest).to_owned();
                    log_effect("Copy Contact", "clipboard text", address.len());
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(address));
                    self.show_success_toast("Link Copied!".into(), cx);
                }
            }
            Action::SendSms => {
                if let Some(url) = self.menu_url() {
                    self.follow_link(&url.replacen("tel:", "sms:", 1), &url, cx);
                }
            }
            Action::OpenUrl => {
                if let Some(url) = self.menu_url() {
                    log_effect("Open Link", "follow link", url.len());
                    let label = match &self.menu_extra.hit {
                        Some(super::menu::Hit::Link { label, .. }) => label.clone(),
                        _ => String::new(),
                    };
                    self.follow_link(&url, &label, cx);
                }
            }
            Action::CopyImage => {
                if let Some(image) = self.menu_image() {
                    log_effect("Copy Image", "clipboard image", image.bytes().len());
                    cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image));
                    self.show_success_toast("Image Copied!".into(), cx);
                } else {
                    self.show_app_toast("Unable to copy image", cx);
                }
            }
            Action::SaveImage => self.save_menu_image(cx),
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

    /// The address of what the open menu was opened on.
    fn menu_url(&self) -> Option<String> {
        match self.menu_extra.hit.as_ref()? {
            super::menu::Hit::Link { url, .. } | super::menu::Hit::Media { url, .. } => {
                Some(url.clone())
            }
        }
    }

    fn menu_image(&self) -> Option<std::sync::Arc<gpui::Image>> {
        match self.menu_extra.hit.as_ref()? {
            super::menu::Hit::Media { key, .. } => self.images.get(key).cloned(),
            super::menu::Hit::Link { .. } => None,
        }
    }

    /// Module 115184 `saveImage`: a save dialog named after the file, then the picture's bytes go there.
    fn save_menu_image(&mut self, cx: &mut Context<Self>) {
        let (Some(image), Some(url)) = (self.menu_image(), self.menu_url()) else {
            return;
        };
        let name = url
            .split(['?', '#'])
            .next()
            .and_then(|path| path.rsplit('/').next())
            .filter(|name| !name.is_empty())
            .unwrap_or("image.png")
            .to_owned();
        let directory = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        log_effect(
            "Save Image",
            "save dialog then file write",
            image.bytes().len(),
        );
        let chosen = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |this, cx| {
            let saved = match chosen.await {
                Ok(Ok(Some(path))) => Some(std::fs::write(path, image.bytes()).is_ok()),
                _ => None,
            };
            let _ = this.update(cx, |this, cx| match saved {
                Some(true) => this.show_success_toast("Image Saved".into(), cx),
                Some(false) => this.show_app_toast("Unable to save image", cx),
                None => {}
            });
        })
        .detach();
    }
}
