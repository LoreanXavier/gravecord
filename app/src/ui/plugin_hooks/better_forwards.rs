//! BetterForwards (Equicord `src/equicordplugins/betterForwards`, GPL-3.0-or-later). In the forward dialog, a message
//! with attachments or embeds gets "Forward everything" (on by default, with the tooltip that the text is not forwarded
//! without it) and, in place of the preview's file line, pickers of its attachments (a tag each, with its kind's icon)
//! and of its embeds (each embed's title over a tag per embed and per extra picture): with "Forward everything" off the
//! forward keeps only what is chosen (`forward_only`). `resendOnFail` sends a forward that failed again as text
//! (`forwardPreface` before each line, the links of the files five to a message, the note last) instead of the Forward
//! Failed dialog; `dontFollowForwards` stays in the channel after forwarding to one place unless Shift is held on Send;
//! `selfForward` lists the current channel too. A forwarded message's footer becomes the original's: its server's
//! picture and name when it is another server (a press opens the server's profile card), its channel's picture and
//! name (a press goes to the message), and its time. Not part of the pinned client.
//!
//! The NSFW bypass of `resendOnFail` (`MESSAGE_FORWARDING_NSFW_NOT_ALLOWED`) has nothing to bypass: the base forward
//! dialog does not refuse an age-restricted source yet.
//!
//! GAP: the server name's fallback picture from the public widget banner (`widget.png`): the name is shown without it.
//! The profile card opens beside the name, where the original's popout opens above it.

use super::super::Root;
use dc_state::{Id, Message};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, img, px,
};
use serde_json::Value;
use std::cell::{Cell, RefCell};

const ID: &str = "BetterForwards";

/// The open dialog's choice: the forwarded message's id, and what is kept with "Forward everything" off.
#[derive(Clone, Debug, Default, PartialEq)]
struct Choice {
    message: Id,
    /// `hasOpts`: "Forward everything" is off.
    picked: bool,
    attachments: Vec<Id>,
    embeds: Vec<usize>,
}

thread_local! {
    static CHOICE: RefCell<Choice> = RefCell::new(Choice::default());
    /// `setShift`: Shift was held on the last press of Send.
    static SHIFT: Cell<bool> = const { Cell::new(false) };
    /// The note of the batch in flight, for the text a failed forward is sent again as.
    static NOTE: RefCell<String> = const { RefCell::new(String::new()) };
    /// The tag under the pointer.
    static HOVERED: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("BetterForwards")
}

/// Whether the plugin is on (the base footer of a forward gives way to the plugin's).
pub(in crate::ui) fn better_forwards_on() -> bool {
    on()
}

fn flag(name: &str) -> bool {
    crate::plugins::with(|state| state.value(ID, name) == Value::Bool(true))
}

/// `selfForward`: the current channel stays in the dialog's list.
pub(in crate::ui) fn better_forwards_self_forward() -> bool {
    on() && flag("selfForward")
}

/// `shouldTransition`: with `dontFollowForwards`, the app goes to the one place only when Shift was held.
pub(in crate::ui) fn better_forwards_follow() -> bool {
    !(on() && flag("dontFollowForwards")) || SHIFT.with(Cell::get)
}

/// `setShift` from the Send button's press.
pub(in crate::ui) fn better_forwards_send_pressed(shift: bool) {
    SHIFT.with(|held| held.set(shift));
}

/// A sub-embed's tag: its index, its label, and whether it is the embed's main one.
type SubEmbed = (usize, String, bool);

/// The sub-embeds a message's embeds show (`EmbedPicker`): each embed's title and its tags.
fn sub_embeds(message: &Message) -> Vec<(String, Vec<SubEmbed>)> {
    let mut id = 0;
    message
        .embeds
        .iter()
        .enumerate()
        .map(|(index, embed)| {
            let title = embed
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .or_else(|| {
                    embed
                        .description
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                })
                .map_or_else(|| format!("Embed {}", index + 1), str::to_owned);
            let images: Vec<&dc_state::EmbedMedia> = if embed.images.is_empty() {
                embed.image.iter().collect()
            } else {
                embed.images.iter().collect()
            };
            let mut tags = Vec::new();
            if !images.is_empty() {
                for (at, image) in images.iter().enumerate() {
                    let count = if images.len() > 1 {
                        format!("{} ", at + 1)
                    } else {
                        String::new()
                    };
                    tags.push((
                        id,
                        format!(
                            "{}Image {count}({} x {})",
                            if at == 0 { "Embed + " } else { "" },
                            image.width,
                            image.height
                        ),
                        at == 0,
                    ));
                    id += 1;
                }
            } else if embed.video.is_some() {
                tags.push((id, "Embed + Video".to_owned(), true));
                id += 1;
            } else {
                tags.push((id, "Embed".to_owned(), true));
                id += 1;
            }
            (title, tags)
        })
        .collect()
}

/// `defaultOpts`: every attachment, and an index for each embed's picture (or one for an embed without).
fn defaults(message: &Message) -> (Vec<Id>, Vec<usize>) {
    let count: usize = message
        .embeds
        .iter()
        .map(|embed| embed.images.len().max(1))
        .sum();
    (
        message.attachments.iter().map(|a| a.id).collect(),
        (0..count).collect(),
    )
}

/// The forwarded message's content: a forward's copy, or the message.
fn content_of(message: &Message) -> Message {
    match message.forwarded.as_ref() {
        Some(copy) => Message {
            content: copy.content.clone(),
            attachments: copy.attachments.clone(),
            embeds: copy.embeds.clone(),
            ..message.clone()
        },
        None => message.clone(),
    }
}

/// The attachment kind's icon (`AttachmentIcon`).
fn attachment_icon(attachment: &dc_state::Attachment) -> &'static str {
    let kind = attachment.content_type.as_deref().unwrap_or_default();
    if attachment.is_clip() {
        "clips_clips"
    } else if attachment.is_video() {
        "search_has_video"
    } else if attachment.is_image() {
        "search_has_image"
    } else if kind.starts_with("audio/") {
        "activity_music"
    } else if kind.starts_with("text/") {
        "clips_letters"
    } else {
        "search_has_file"
    }
}

impl Root {
    /// The dialog opened for `message`: everything is forwarded until "Forward everything" is turned off.
    pub(in crate::ui) fn better_forwards_opened(&self, message: Option<&Message>) {
        CHOICE.with(|choice| {
            *choice.borrow_mut() = Choice {
                message: message.map_or(0, |message| message.id),
                ..Choice::default()
            }
        });
    }

    /// The forward with this nonce keeps only what is chosen (`forward_only`, the empty choice sent as attachment
    /// `0`, the embed indices in order).
    pub(in crate::ui) fn better_forwards_sending(&self, nonce: &str, note: &str) {
        if !on() {
            return;
        }
        NOTE.with(|kept| *kept.borrow_mut() = note.to_owned());
        let choice = CHOICE.with(|choice| choice.borrow().clone());
        if !choice.picked {
            return;
        }
        let mut attachments: Vec<String> = choice.attachments.iter().map(Id::to_string).collect();
        let mut embeds = choice.embeds.clone();
        if attachments.is_empty() && embeds.is_empty() {
            attachments.push("0".to_owned());
        }
        embeds.sort_unstable();
        crate::plugins::better_forwards::set(nonce, attachments, embeds);
    }

    /// `resendOnFail`: the failed places get the message again as text, and no Forward Failed dialog. True when it
    /// took the failure.
    pub(in crate::ui) fn better_forwards_failed(
        &mut self,
        message: &Message,
        failed: &[Id],
        cx: &mut Context<Self>,
    ) -> bool {
        if !on() || !flag("resendOnFail") {
            return false;
        }
        let preface =
            crate::plugins::with(|state| match state.value(ID, "forwardPreface").as_str() {
                Some("-#") => "-#",
                _ => ">",
            });
        let source = content_of(message);
        let choice = CHOICE.with(|choice| choice.borrow().clone());
        let urls: Vec<String> = source
            .attachments
            .iter()
            .filter(|attachment| {
                !(choice.picked && choice.message == message.id)
                    || choice.attachments.contains(&attachment.id)
            })
            .map(|attachment| attachment.url.clone())
            .collect();
        let note = NOTE.with(|note| note.borrow().clone());
        let texts = crate::plugins::better_forwards::resend_texts(
            preface,
            message.channel_id,
            &source.content,
            &urls,
            &note,
        );
        for channel in failed {
            for text in &texts {
                self.start_local_text(*channel, text.clone());
            }
        }
        cx.notify();
        true
    }

    /// A tag of a `TagGroup` (size sm): selectable, greyed while "Forward everything" is on.
    fn better_forwards_tag(
        &self,
        key: String,
        (icon, label): (&'static str, String),
        (selected, disabled): (bool, bool),
        toggle: impl Fn(&mut Root) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::super::color;
        let hovered =
            !disabled && HOVERED.with(|hovered| hovered.borrow().as_deref() == Some(key.as_str()));
        // `labeled_9EByBZ`: the secondary control's colours, the toggle's when selected.
        let (background, border, text) = match (selected, hovered) {
            (true, true) => (
                "--togglebutton-background-selected-hover",
                "--togglebutton-border-selected",
                "--control-secondary-text-hover",
            ),
            (true, false) => (
                "--togglebutton-background-selected",
                "--togglebutton-border-selected",
                "--control-secondary-text-default",
            ),
            (false, true) => (
                "--control-secondary-background-hover",
                "--control-secondary-border-hover",
                "--control-secondary-text-hover",
            ),
            (false, false) => (
                "--control-secondary-background-default",
                "--control-secondary-border-default",
                "--control-secondary-text-default",
            ),
        };
        let hover_key = key.clone();
        div()
            .id(SharedString::from(key))
            .min_h(px(30.))
            .max_w_full()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(color(border))
            .bg(color(background))
            .flex()
            .items_center()
            .gap(px(4.))
            .text_color(color(text))
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| this.cursor_pointer())
            .child(
                gpui::svg()
                    .path(format!("icons/{icon}.svg"))
                    .size(px(16.))
                    .flex_none()
                    .text_color(color(text)),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .whitespace_nowrap()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(400.))
                    .child(label),
            )
            .on_hover(cx.listener(move |_, inside: &bool, _, cx| {
                HOVERED.with(|hovered| {
                    let mut hovered = hovered.borrow_mut();
                    if *inside {
                        *hovered = Some(hover_key.clone());
                    } else if hovered.as_deref() == Some(hover_key.as_str()) {
                        *hovered = None;
                    }
                });
                cx.notify();
            }))
            .when(!disabled, |this| {
                this.on_click(cx.listener(move |this, _, _, cx| {
                    toggle(this);
                    cx.notify();
                }))
            })
            .into_any_element()
    }

    /// `ForwardPicker`: the attachments' tags, then each embed's title over its tags; none for a message with neither.
    pub(in crate::ui) fn better_forwards_picker(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !on() {
            return None;
        }
        let source = content_of(message);
        if source.embeds.is_empty() && source.attachments.is_empty() {
            return None;
        }
        let choice = CHOICE.with(|choice| choice.borrow().clone());
        let picked = choice.picked && choice.message == message.id;
        let (all_attachments, all_embeds) = defaults(&source);
        let chosen_attachments = if picked {
            choice.attachments.clone()
        } else {
            all_attachments
        };
        let chosen_embeds = if picked {
            choice.embeds.clone()
        } else {
            all_embeds
        };
        let mut groups: Vec<AnyElement> = Vec::new();
        if !source.attachments.is_empty() {
            let tags: Vec<AnyElement> = source
                .attachments
                .iter()
                .map(|attachment| {
                    let id = attachment.id;
                    self.better_forwards_tag(
                        format!("bf-attachment-{id}"),
                        (
                            attachment_icon(attachment),
                            attachment
                                .title
                                .clone()
                                .unwrap_or_else(|| attachment.filename.clone()),
                        ),
                        (chosen_attachments.contains(&id), !picked),
                        move |_| {
                            CHOICE.with(|choice| {
                                let mut choice = choice.borrow_mut();
                                if let Some(at) =
                                    choice.attachments.iter().position(|kept| *kept == id)
                                {
                                    choice.attachments.remove(at);
                                } else {
                                    choice.attachments.push(id);
                                }
                            });
                        },
                        cx,
                    )
                })
                .collect();
            groups.push(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(tags)
                    .into_any_element(),
            );
        }
        for (title, subs) in sub_embeds(&source) {
            let tags: Vec<AnyElement> = subs
                .into_iter()
                .map(|(index, label, main)| {
                    self.better_forwards_tag(
                        format!("bf-embed-{index}"),
                        (
                            if main {
                                "search_has_embed"
                            } else {
                                "search_has_image"
                            },
                            label,
                        ),
                        (chosen_embeds.contains(&index), !picked),
                        move |_| {
                            CHOICE.with(|choice| {
                                let mut choice = choice.borrow_mut();
                                if let Some(at) =
                                    choice.embeds.iter().position(|kept| *kept == index)
                                {
                                    choice.embeds.remove(at);
                                } else {
                                    choice.embeds.push(index);
                                }
                            });
                        },
                        cx,
                    )
                })
                .collect();
            groups.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(super::super::color("--text-subtle"))
                            .when(!picked, |this| this.opacity(0.5))
                            .child(title),
                    )
                    .child(div().flex().flex_wrap().gap(px(8.)).children(tags))
                    .into_any_element(),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(groups)
                .into_any_element(),
        )
    }

    /// `renderWrapper`'s "Forward everything" under the preview, for a message with attachments or embeds.
    pub(in crate::ui) fn better_forwards_everything(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use super::super::tooltip::{Side, Tip};
        if !on() {
            return None;
        }
        let source = content_of(message);
        if source.embeds.len() + source.attachments.len() == 0 {
            return None;
        }
        let choice = CHOICE.with(|choice| choice.borrow().clone());
        let everything = !(choice.picked && choice.message == message.id);
        let id = message.id;
        let (all_attachments, all_embeds) = defaults(&source);
        let info = div().id("bf-everything-info").size(px(20.)).child(
            gpui::svg()
                .path("icons/circle_info.svg")
                .size(px(20.))
                .text_color(super::super::color("--text-muted")),
        );
        Some(
            div()
                .mt(px(16.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .id("bf-everything")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(super::super::context_menu::menu_check_box(everything))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(super::super::color("--text-default"))
                                .child("Forward everything"),
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            CHOICE.with(|choice| {
                                let mut choice = choice.borrow_mut();
                                // `setOpts(!hasOpts ? defaultOpts : {})`.
                                *choice = if everything {
                                    Choice {
                                        message: id,
                                        picked: true,
                                        attachments: all_attachments.clone(),
                                        embeds: all_embeds.clone(),
                                    }
                                } else {
                                    Choice {
                                        message: id,
                                        ..Choice::default()
                                    }
                                };
                            });
                            cx.notify();
                        })),
                )
                .child(self.with_tip(
                    "bf-everything-tip",
                    Tip::text(
                        "Message text will not be forwarded when this option is disabled",
                        Side::Top,
                    ),
                    info,
                    cx,
                ))
                .into_any_element(),
        )
    }

    /// `ForwardFooter`: the server (when it is another one), the channel and the time of a forwarded message.
    pub(in crate::ui) fn better_forwards_footer(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use super::super::color;
        if !on() {
            return None;
        }
        let forwarded = message.forwarded.as_ref()?;
        let channel = forwarded.channel_id?;
        let target_guild = self.guild_of_channel(message.channel_id);
        let element = |key: String| {
            div()
                .id(SharedString::from(key))
                .group("bf-footer-element")
                .flex()
                .items_center()
                .gap(px(2.))
                .pt(px(1.))
                .pb(px(1.))
                .pl(px(2.))
                .rounded(px(4.))
                .text_color(color("--text-muted"))
        };
        let text = |label: String, italic: bool| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(400.))
                .when(italic, |this| this.italic())
                .child(label)
        };
        let arrow = || {
            gpui::svg()
                .path("icons/plugin_chevron_right.svg")
                .size(px(12.))
                .flex_none()
                .text_color(color("--text-muted"))
        };
        let mut parts: Vec<AnyElement> = Vec::new();
        if let Some(guild) = forwarded.guild_id
            && target_guild != Some(guild)
        {
            let known = self.stores.guild(guild);
            let profile = self
                .fetched
                .get(&super::super::message_header::profile_key(guild));
            let name = known
                .map(|known| known.name.clone())
                .or_else(|| profile.and_then(|body| body["name"].as_str().map(str::to_owned)));
            let picture = known
                .is_some_and(|known| known.icon.is_some())
                .then(|| self.images.get(&format!("guild:{guild}")).cloned())
                .flatten();
            let icon: Option<AnyElement> = match (&picture, &name) {
                (Some(picture), _) => Some(
                    img(picture.clone())
                        .size(px(16.))
                        .mr(px(2.))
                        .flex_none()
                        .rounded(px(4.))
                        .into_any_element(),
                ),
                (None, Some(name)) if known.is_some() => Some(
                    div()
                        .size(px(16.))
                        .mr(px(2.))
                        .flex_none()
                        .rounded(px(4.))
                        .overflow_hidden()
                        .bg(color("--background-base-lower"))
                        .text_color(color("--text-default"))
                        .text_size(px(9.6))
                        .line_height(px(16.))
                        .flex()
                        .justify_center()
                        .child(super::server_info::acronym(name))
                        .into_any_element(),
                ),
                _ => None,
            };
            let probe = format!("bf-footer-guild-{}", message.id);
            parts.push(
                element(format!("bf-guild-{}", message.id))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .relative()
                    .child(self.anchor_probe(probe.clone()))
                    .children(icon)
                    .child(text(
                        name.unwrap_or_else(|| "View server".to_owned()),
                        false,
                    ))
                    .child(arrow())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let anchor = this.tip_anchors.borrow().get(probe.as_str()).copied();
                        if let Some(anchor) = anchor {
                            this.open_guild_tag_popout(guild, anchor, cx);
                        }
                    }))
                    .into_any_element(),
            );
        }
        // `ChannelName`: the channel's picture and its name, or the unknown channel or person in italics.
        let in_guild = forwarded.guild_id.is_some();
        let known_channel = self
            .stores
            .guilds
            .iter()
            .flat_map(|guild| guild.channels.iter())
            .find(|known| known.id == channel);
        let private = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel);
        let (icon, name, unknown): (AnyElement, String, bool) = if let Some(known) = known_channel {
            let facts = super::super::channel_icon::Facts {
                kind: known.kind,
                rules: false,
                nsfw: known.nsfw,
                private: false,
                pending: false,
                locked: false,
                active_threads: false,
            };
            (
                super::super::icon(
                    super::super::channel_icon::name(&facts),
                    16.,
                    "--text-muted",
                )
                .into_any_element(),
                known.name.clone(),
                false,
            )
        } else if let Some(dm) = private {
            let picture = if dm.kind == 3 {
                let (key, _) = super::super::unread_dms::group_icon(dm);
                self.images.get(&key).cloned()
            } else {
                self.stores.dm_recipient(dm).and_then(|user| {
                    self.images
                        .get(&super::super::avatar_source(user).0)
                        .cloned()
                })
            };
            (
                match picture {
                    Some(picture) => img(picture)
                        .size(px(16.))
                        .mr(px(2.))
                        .rounded_full()
                        .into_any_element(),
                    None => div().into_any_element(),
                },
                self.private_channel_name(dm),
                false,
            )
        } else if in_guild {
            (
                super::super::icon("plugin_text", 16., "--text-muted").into_any_element(),
                dc_intl::tr("unknown-channel").to_lowercase(),
                true,
            )
        } else {
            (
                super::super::icon("plugin_at", 16., "--text-muted").into_any_element(),
                dc_intl::tr("Unknown User").to_owned(),
                true,
            )
        };
        let (guild, original) = (forwarded.guild_id, forwarded.message_id);
        parts.push(
            element(format!("bf-channel-{}", message.id))
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .child(icon)
                .child(text(name, unknown))
                .child(arrow())
                .on_click(cx.listener(move |this, _, _, cx| {
                    // `transitionTo(/channels/{guild ?? @me}/{channel}/{message})`.
                    match original {
                        Some(original) => this.jump_to_message_id(channel, original, true, None),
                        None => match guild {
                            Some(guild) => this.select(guild, Some(channel)),
                            None => this.select_home(Some(channel)),
                        },
                    }
                    cx.notify();
                }))
                .into_any_element(),
        );
        // `Timestamp`: the original's time, `calendarFormat`.
        let time = forwarded
            .message_id
            .map(|id| {
                let millis = dc_state::message_window::extract_timestamp(id) as i64;
                chrono::DateTime::from_timestamp_millis(millis)
                    .unwrap_or_default()
                    .with_timezone(&chrono::Local)
            })
            .unwrap_or_else(|| forwarded.local_time());
        parts.push(
            element(format!("bf-time-{}", message.id))
                .child(text(
                    super::super::timestamps::calendar_time(time, chrono::Local::now(), false),
                    false,
                ))
                .into_any_element(),
        );
        Some(
            div()
                .mt(px(3.))
                .ml(px(-2.))
                .flex()
                .flex_wrap()
                .children(parts)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_split_as_the_original_splits_them() {
        let message = Message::from_value(&serde_json::json!({
            "id": "1600000000000000031", "channel_id": "7", "content": "", "type": 0,
            "timestamp": "2026-11-19T00:00:00+00:00",
            "author": {"id": "5", "username": "someone"},
            "embeds": [
                {"type": "rich", "title": "  Title  ", "image": {"url": "https://i/1.png", "width": 10, "height": 20}},
                {"type": "video", "description": "Clip", "video": {"url": "https://v/1.mp4", "proxy_url": "https://p/1.mp4", "width": 640, "height": 360}},
                {"type": "rich"}
            ]
        }))
        .unwrap();
        let subs = sub_embeds(&message);
        assert_eq!(subs[0].0, "Title");
        assert_eq!(subs[0].1[0].1, "Embed + Image (10 x 20)");
        assert_eq!(subs[1].1[0].1, "Embed + Video");
        assert_eq!(subs[2].0, "Embed 3");
        assert_eq!(defaults(&message).1, [0, 1, 2]);
        // Off (the unit tests' state): the dialog follows the base.
        assert!(better_forwards_follow());
        assert!(!better_forwards_self_forward());
    }
}
