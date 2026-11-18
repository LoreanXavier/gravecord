//! Direct messages: the welcome at the start of the history (`container_w6h4R2`) and system messages
//! (`container_bLMKT5`). Measured in `design/dom/dm/loreanxavier-darker-cozy.json`.

use super::{Root, avatar_source, color, header_time, icon};
use crate::parity::Parity;
use dc_state::{Id, Message};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// `FRIEND_REQUEST_ACCEPTED` in the message type enum.
pub(super) const FRIEND_REQUEST_ACCEPTED: u64 = 67;
/// `CHANNEL_PINNED_MESSAGE`.
pub(super) const CHANNEL_PINNED_MESSAGE: u64 = 6;

/// `USER_JOIN` and `THREAD_CREATED`.
pub(super) const USER_JOIN: u64 = 7;

/// `SUPPRESS_JOIN_NOTIFICATION_REPLIES` of `system_channel_flags`.
const SUPPRESS_JOIN_NOTIFICATION_REPLIES: u64 = 1 << 3;

/// Module 117096 `tJ`: the stickers the "Wave to say hi!" button picks from, with their `format_type`.
const WELCOME_STICKERS: [(Id, u64); 6] = [
    (749_054_660_769_218_631, 3),
    (751_606_379_340_365_864, 3),
    (754_108_890_559_283_200, 3),
    (781_291_131_828_699_156, 2),
    (816_087_792_291_282_944, 3),
    (819_128_604_311_027_752, 3),
];

/// `tZ`: the sticker for a join message, by the account's and the message's snowflake times.
pub(super) fn welcome_sticker(me: Option<Id>, message: Id) -> (Id, u64) {
    let time = |id: Id| (id >> 22) + 1_420_070_400_000;
    let index = (me.map_or(0, time) + time(message)) % WELCOME_STICKERS.len() as u64;
    WELCOME_STICKERS[index as usize]
}
pub(super) const THREAD_CREATED: u64 = 18;
/// The join line's green arrow (`icon_bLMKT5`, a CSS background).
pub(super) const JOIN_ARROW: &str = "https://discordapp.com/assets/8b9de960da063cb9.svg";

/// System messages this client draws; the rest still render as ordinary messages.
pub(super) fn is_system(message: &Message) -> bool {
    matches!(
        message.kind,
        FRIEND_REQUEST_ACCEPTED | CHANNEL_PINNED_MESSAGE | USER_JOIN | THREAD_CREATED
    )
}

/// A `sm` secondary button: 32 tall, 11px inside a 1px border, 14px medium text.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tone {
    Secondary,
    Primary,
    CriticalPrimary,
}

/// The `sm` design-system button of the DM welcome, in the variant the relationship asks for.
pub(super) fn small_button(
    id: &'static str,
    label: &'static str,
    tone: Tone,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let (background, border, text) = match tone {
        Tone::Secondary => (
            "--control-secondary-background-default",
            "--control-secondary-border-default",
            "--control-secondary-text-default",
        ),
        Tone::Primary => (
            "--control-primary-background-default",
            "--control-primary-border-default",
            "--white",
        ),
        Tone::CriticalPrimary => (
            "--control-critical-primary-background-default",
            "--control-critical-primary-border-default",
            "--white",
        ),
    };
    div()
        .id(id)
        .h(px(32.))
        .px(px(11.))
        .rounded(px(8.))
        .border_1()
        .border_color(color(border))
        .bg(color(background))
        .flex()
        .items_center()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color(text))
        .when(enabled, |this| this.cursor_pointer())
        .when(!enabled, |this| this.opacity(0.5))
        .child(label)
}

/// A run of a message drawn between flex children: its edge spaces kept as no-break spaces, as the line wraps.
fn spaced(text: &str) -> String {
    let start = text.len() - text.trim_start_matches(' ').len();
    let end = text.trim_end_matches(' ').len().max(start);
    format!(
        "{}{}{}",
        "\u{a0}".repeat(start),
        &text[start..end],
        "\u{a0}".repeat(text.len() - end)
    )
}

impl Root {
    /// The start of a DM: the other person large, the opening line, mutual servers and the friend buttons.
    pub(super) fn dm_welcome(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.current_dm()?;
        if channel.kind == 3 {
            return Some(self.group_welcome(channel, cx));
        }
        let user = self.stores.dm_recipient(channel)?.clone();
        // Module 891522 `a_`: a system DM is `sh` with the official line (`Rzvnig`) as its description, and no
        // username line, mutual servers or buttons.
        let system = self.stores.is_system_dm(channel);
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let mutual = self
            .fetched
            .get(&format!("profile:{}", user.id))
            .and_then(|body| body["mutual_guilds"].as_array().map(Vec::len))
            .unwrap_or(0);
        let relationship = self
            .stores
            .relationships
            .get(&user.id)
            .copied()
            .unwrap_or(0);
        let buttons = self.welcome_buttons(channel.id, &user, relationship, cx);
        let first_mutual = self
            .fetched
            .get(&format!("profile:{}", user.id))
            .and_then(|body| {
                body["mutual_guilds"][0]["id"]
                    .as_str()
                    .and_then(|id| id.parse::<u64>().ok())
            })
            .and_then(|guild| self.images.get(&format!("guild:{guild}")).cloned());
        let name = self.dm_display_name(&user);
        Some(
            // A list row's own margin is not measured, so the 16px above `container_w6h4R2` is padding on a wrapper.
            div()
                .pt(px(16.))
                .child(
                    div()
                        .parity("dm-welcome")
                        .id("dm-welcome")
                        .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                            if this.hovered_welcome != *inside {
                                this.hovered_welcome = *inside;
                                cx.notify();
                            }
                        }))
                        .ml(px(16.))
                        .mr(px(32.))
                        .flex()
                        .flex_col()
                        .child(div().size(px(80.)).rounded_full().children(
                            avatar.map(|picture| img(picture).size(px(80.)).rounded_full()),
                        ))
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(32.))
                                .line_height(px(40.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                // 891522: a 1:1 DM's heading takes the recipient's display name style, animated while hovered.
                                .child(super::name_styles::name(
                                    format!("dm-welcome-{}", user.id),
                                    name.clone(),
                                    self.name_styles(&user, None).as_ref(),
                                    if self.hovered_welcome {
                                        super::name_styles::Display::Animated
                                    } else {
                                        super::name_styles::Display::Static
                                    },
                                )),
                        )
                        // `header_w6h4R2` (`margin: 8px 0`) over `description_w6h4R2` (`body-md/normal`, 16px / 20px,
                        // `--text-default`).
                        .when(system, |this| {
                            this.child(
                                div()
                                    .mt(px(8.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("Rzvnig")),
                            )
                        })
                        .when(!system, |this| {
                            this.child(
                                div()
                                    .mt(px(8.))
                                    .text_size(px(24.))
                                    .line_height(px(32.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(user.username.clone()),
                            )
                        })
                        .when(!system, |this| {
                            this.child(
                                div()
                                    .mt(px(20.))
                                    .flex()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    // `Qvg+6+`: the text, the bold name, the full stop.
                                    .child(super::bold_message(
                                        "Qvg+6+",
                                        &[("username", (&name).into())],
                                        FontWeight(600.),
                                    )),
                            )
                        })
                        .when(!system, |this| {
                            this.child(
                                div()
                                    .mt(px(8.))
                                    .h(px(32.))
                                    .flex()
                                    .items_center()
                                    .when(mutual > 0, |this| {
                                        this.child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .size(px(24.))
                                                        .rounded(px(8.))
                                                        .overflow_hidden()
                                                        .children(first_mutual.map(|picture| {
                                                            img(picture).size(px(24.))
                                                        })),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .text_color(color("--text-subtle"))
                                                        .child(dc_intl::t!(
                                                            "E215W7",
                                                            count = mutual
                                                        )),
                                                )
                                                // The dot between the servers and the buttons.
                                                .child(
                                                    div()
                                                        .mx(px(4.))
                                                        .size(px(4.))
                                                        .rounded_full()
                                                        .bg(color("--background-mod-strong")),
                                                ),
                                        )
                                    })
                                    .child(div().ml(px(4.)).flex().gap(px(8.)).children(buttons)),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// `s2.I`: a message of theirs in this DM to report (a system line counts: it is theirs too).
    fn first_message_of(&self, channel: dc_state::Id, user: dc_state::Id) -> Option<Message> {
        self.stores
            .messages
            .get(&channel)
            .and_then(|messages| messages.iter().find(|message| message.author.id == user))
            .cloned()
    }

    /// Function `ap` of module 891522 by relationship: Remove Friend for a friend, Add Friend (or Accept for an
    /// incoming request) for nobody, "Friend Request Sent" disabled while one waits, Unblock once blocked; Block
    /// unless blocked; Report Spam (`s4`) while they are not a friend and have a message here. Each acts at
    /// once, as the official buttons do; Block shows `XXPrIs` "{name} blocked".
    fn welcome_buttons(
        &self,
        channel: dc_state::Id,
        user: &dc_state::User,
        relationship: u64,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let id = user.id;
        let name = user.display_name().to_owned();
        // `isNonUserBot` (module 619019): the system account or a bot with the `0000` tag gets no buttons (`aI`).
        if user.system || (user.bot && user.discriminator == "0000") {
            return Vec::new();
        }
        if user.bot {
            // `aI` gives a bot `s9` instead of `ap`: no friend or Block button. Its Report Spam (`s4`) needs only
            // a message of theirs (an application the account does not own is always reportable).
            let report = self.first_message_of(channel, id).map(|first| {
                small_button(
                    "welcome-report",
                    dc_intl::tr("Report Spam"),
                    Tone::CriticalPrimary,
                    true,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_report_message(&first, true, window, cx);
                }))
                .into_any_element()
            });
            return self.app_dm_buttons(channel, user, report, cx);
        }
        let mut list = Vec::new();
        match relationship {
            1 => list.push(
                small_button(
                    "welcome-remove",
                    dc_intl::tr("Remove Friend"),
                    Tone::Secondary,
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    // The same question the menu row asks before a friend is removed.
                    this.confirm_remove_friend = Some(super::friends::RemoveFriend::Menu(id));
                    cx.notify();
                }))
                .into_any_element(),
            ),
            2 => list.push(
                small_button(
                    "welcome-unblock",
                    dc_intl::tr("Unblock"),
                    Tone::Secondary,
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_relationship(id, None, cx)))
                .into_any_element(),
            ),
            // Not in the pinned client: CancelFriendRequest's button (`plugin_hooks.rs`).
            4 if super::plugin_hooks::cancel_friend_request() => list.push(
                small_button(
                    "welcome-cancel-request",
                    super::plugin_hooks::CANCEL_FRIEND_REQUEST,
                    Tone::CriticalPrimary,
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_relationship(id, None, cx)))
                .into_any_element(),
            ),
            4 => list.push(
                small_button(
                    "welcome-sent",
                    dc_intl::tr("Friend Request Sent"),
                    Tone::Primary,
                    false,
                )
                .into_any_element(),
            ),
            _ => list.push(
                small_button(
                    "welcome-add",
                    dc_intl::tr("Add Friend"),
                    Tone::Primary,
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_relationship(id, Some(0), cx)))
                .into_any_element(),
            ),
        }
        if relationship != 2 {
            list.push(
                small_button("welcome-block", dc_intl::tr("Block"), Tone::Secondary, true)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_relationship(id, Some(2), cx);
                        this.show_success_toast(dc_intl::t!("XXPrIs", username = &name), cx);
                    }))
                    .into_any_element(),
            );
        }
        if matches!(relationship, 0 | 2 | 3)
            && let Some(first) = self.first_message_of(channel, id)
        {
            list.push(
                small_button(
                    "welcome-report",
                    dc_intl::tr("Report Spam"),
                    Tone::CriticalPrimary,
                    true,
                )
                // `b8`: the first message of theirs is reported as `first_dm`.
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_report_message(&first, true, window, cx);
                }))
                .into_any_element(),
            );
        }
        list
    }

    /// A system line: an 18px icon centred in the 72px gutter, then who and what, and the time.
    /// `yIDvPL`: "{name} pinned **a message** to this channel. See all **pinned messages**.", each part a link:
    /// the name to the profile, the message to jump to, the pins popout.
    fn pin_notice(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let link = |id: String, text: String| {
            div()
                .id(SharedString::from(id))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .cursor_pointer()
                .child(text)
        };
        let target = message.reference_id;
        let channel = message.channel_id;
        div()
            .py(px(2.))
            .flex()
            .flex_wrap()
            .items_center()
            .line_height(px(22.))
            .text_color(color("--channels-default"))
            .child(
                self.profile_trigger(
                    format!("pin-notice-name-{}", message.id),
                    message.author.id,
                    div()
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(message.author.display_name().to_owned()),
                    cx,
                ),
            )
            // `yIDvPL`'s runs: name, " pinned ", the message link, " to this channel. See all ", the pins link, ".".
            .child(spaced(&super::message_part(
                "yIDvPL",
                &[("username", "".into())],
                0,
            )))
            .child(
                link(
                    format!("pin-notice-message-{}", message.id),
                    super::message_part("yIDvPL", &[("username", "".into())], 1),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let found = target.and_then(|target| {
                        this.stores
                            .messages
                            .get(&channel)
                            .and_then(|list| list.iter().find(|known| known.id == target))
                            .cloned()
                    });
                    if let Some(found) = found {
                        this.jump_to_message(&found, cx);
                    }
                })),
            )
            .child(spaced(&super::message_part(
                "yIDvPL",
                &[("username", "".into())],
                2,
            )))
            .child(
                link(
                    format!("pin-notice-pins-{}", message.id),
                    super::message_part("yIDvPL", &[("username", "".into())], 3),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    let key = if this.current_dm().is_some() {
                        "dm-pins"
                    } else {
                        "pins"
                    };
                    if this.pins_open.is_none() {
                        this.toggle_pins(key, cx);
                    }
                })),
            )
            .child(".")
            .child(
                div()
                    .ml(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight(500.))
                    .child(header_time(message.local_time(), chrono::Local::now())),
            )
            .into_any_element()
    }

    /// Module 117096 `tK`: the "Wave to say hi!" button shows under a join message of a server when the account may
    /// send here (not a media post, not a locked archived thread it cannot manage), has passed the rules screening,
    /// the newcomer is not a bot, and the server does not suppress join replies.
    fn welcome_cta_shown(&self, message: &Message) -> bool {
        let (Some(guild), Some(channel)) = (self.current_guild(), self.current_channel()) else {
            return false;
        };
        let send = self.can_in_channel(channel, dc_state::permissions::SEND_MESSAGES);
        // `UJ` of module 406704.
        let thread = guild.threads.iter().find(|thread| thread.id == channel.id);
        let media_post = thread.is_some_and(|thread| {
            guild.channels.iter().any(|parent| {
                parent.id == thread.parent_id && parent.kind == dc_state::ChannelKind::Other(16)
            })
        });
        let locked = thread.is_some_and(|thread| thread.archived && thread.locked)
            && !self.can_in_channel(channel, dc_state::permissions::MANAGE_THREADS);
        send && !media_post
            && !locked
            && !guild.my_pending
            && !message.author.bot
            && guild.system_channel_flags & SUPPRESS_JOIN_NOTIFICATION_REPLIES == 0
    }

    /// The button itself (`welcomeCTA_eZQ1Gc` around a secondary md button): the chosen sticker at 24 and the label,
    /// 4px apart, 40 high with 15px inside its 1px border; it sends the sticker as a reply that mentions the newcomer.
    fn welcome_cta(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.welcome_cta_shown(message) {
            return None;
        }
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let (sticker, format_type) = welcome_sticker(me, message.id);
        let interacting = self.hovered == Some(message.id);
        let art = self.sticker_art(
            sticker,
            dc_state::stickers::Format::from_wire(format_type),
            24.,
            interacting,
            false,
            cx,
        );
        let (channel, guild, reply_to) = (message.channel_id, self.guild, message.id);
        Some(
            div()
                .pb(px(2.))
                .flex()
                .child(
                    div()
                        .id(("welcome-cta", message.id))
                        .h(px(40.))
                        .min_w(px(100.))
                        .px(px(15.))
                        .py(px(7.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .text_color(color("--control-secondary-text-default"))
                        .hover(|style| {
                            style
                                .bg(color("--control-secondary-background-hover"))
                                .border_color(color("--control-secondary-border-hover"))
                                .text_color(color("--control-secondary-text-hover"))
                        })
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .size(px(24.))
                                .flex_none()
                                .children(art.map(|art| art.size(px(24.)))),
                        )
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .whitespace_nowrap()
                                .child(dc_intl::tr("Wave to say hi!")),
                        )
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.send_greet(channel, guild, reply_to, sticker);
                        })),
                )
                .into_any_element(),
        )
    }

    /// `sendGreetMessage`: the sticker goes to `/channels/{id}/greet` as a reply to the join message, mentioning its
    /// author (`getSendMessageOptionsForReply` with `shouldMention`, so no `allowed_mentions`).
    fn send_greet(&mut self, channel: Id, guild: Option<Id>, message: Id, sticker: Id) {
        let mut reference = serde_json::json!({
            "channel_id": channel.to_string(),
            "message_id": message.to_string(),
        });
        if let Some(guild) = guild {
            reference["guild_id"] = serde_json::json!(guild.to_string());
        }
        let _ = self.requests.send(crate::net::Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/greet"),
            body: Some(serde_json::json!({
                "sticker_ids": [sticker.to_string()],
                "message_reference": reference,
            })),
        });
        // `STICKER_TRACK_USAGE`.
        self.stores
            .stickers
            .track(&[sticker], super::picker::now_ms());
    }

    /// `getSystemMessageUserJoin` in module 779014: one of thirteen lines, picked by the message's snowflake time
    /// modulo 13, as (before, after) around the name.
    pub(super) fn join_line(message: &Message) -> (String, String) {
        const LINES: [&str; 13] = [
            "0cuj7l", "MuW+CN", "osqpHX", "5ToSh2", "JEB8ps", "pkOV5T", "kRb1J+", "EmKLY+",
            "rPtBnb", "5B/ekS", "ESNC3Y", "Iw6d8w", "WecSZ/",
        ];
        let timestamp = (message.id >> 22) + 1_420_070_400_000;
        let key = LINES[(timestamp % LINES.len() as u64) as usize];
        // The message around its `usernameHook`, split where the name goes.
        let text = dc_intl::format(key, &[("username", "\u{1}".into())]);
        match text.split_once('\u{1}') {
            Some((before, after)) => (before.to_owned(), after.to_owned()),
            None => (text, String::new()),
        }
    }

    /// THREAD_CREATED: "name started a thread: **thread**. See all **threads**." and the thread's card
    /// (`container_g2fiji`): its name, the message count in the link colour, and the latest message.
    fn thread_created_line(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let thread_id = message.reference_channel_id;
        let thread = thread_id.and_then(|thread| {
            self.current_guild()?
                .threads
                .iter()
                .find(|known| known.id == thread)
                .cloned()
        });
        let latest = thread_id
            .and_then(|thread| self.stores.messages.get(&thread))
            .and_then(|list| list.last())
            .cloned();
        let count = thread
            .as_ref()
            .map(|thread| thread.message_count)
            .unwrap_or(0)
            .max(u64::from(latest.is_some()));
        let guild = self.guild;
        let bold = |text: String| {
            div()
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let card =
            thread_id.map(|thread| {
                let avatar = latest.as_ref().and_then(|message| {
                    self.images.get(&avatar_source(&message.author).0).cloned()
                });
                div()
                    .id(SharedString::from(format!("thread-card-{thread}")))
                    .mt(px(4.))
                    .w(px(322.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(guild) = guild {
                            this.select(guild, Some(thread));
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .child(
                                div()
                                    .text_color(color("--text-strong"))
                                    .child(message.content.clone()),
                            )
                            .child(div().text_color(color("--text-link")).child(if count == 1 {
                                dc_intl::t!("rfAXDV", count = 1)
                            } else {
                                dc_intl::t!("rfAXDV", count = count)
                            })),
                    )
                    .children(latest.map(|latest| {
                        div()
                            .mt(px(2.))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .child(div().size(px(16.)).children(
                                avatar.map(|image| img(image).size(px(16.)).rounded_full()),
                            ))
                            .child(
                                div()
                                    .ml(px(8.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(latest.author.display_name().to_owned()),
                            )
                            .child(
                                div()
                                    .ml(px(4.))
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_color(color("--text-default"))
                                    .child(latest.content.clone()),
                            )
                            .child(div().ml(px(8.)).text_color(color("--text-muted")).child(
                                super::threads::short_ago(
                                    chrono::Utc::now().timestamp_millis()
                                        - latest.timestamp.timestamp_millis(),
                                ),
                            ))
                    }))
            });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .py(px(2.))
                    .flex()
                    .flex_wrap()
                    .line_height(px(22.))
                    .text_color(color("--text-muted"))
                    .child(bold(message.author.display_name().to_owned()))
                    // `cZ9Uf2`'s runs: actor, " started a thread: ", the thread, ". See all ", "threads", ".".
                    .child(spaced(&super::message_part(
                        "cZ9Uf2",
                        &[("actorName", "".into()), ("threadName", "".into())],
                        0,
                    )))
                    .child(bold(message.content.clone()))
                    .child(spaced(&super::message_part(
                        "cZ9Uf2",
                        &[("actorName", "".into()), ("threadName", "".into())],
                        1,
                    )))
                    .child(
                        bold(super::message_part(
                            "cZ9Uf2",
                            &[("actorName", "".into()), ("threadName", "".into())],
                            2,
                        ))
                        .id("see-all-threads")
                        .cursor_pointer()
                        .hover(|this| this.underline())
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.thread_browser.is_none() {
                                this.toggle_thread_browser(window, cx);
                            }
                        })),
                    )
                    .child(".")
                    .child(
                        div()
                            .ml(px(4.))
                            .text_size(px(12.))
                            .font_weight(FontWeight(500.))
                            .child(header_time(message.local_time(), chrono::Local::now())),
                    ),
            )
            .children(card)
            .into_any_element()
    }

    /// USER_JOIN's line with the green arrow in the gutter.
    fn join_message_line(&self, message: &Message) -> AnyElement {
        let (before, after) = Self::join_line(message);
        div()
            .py(px(2.))
            .flex()
            .flex_wrap()
            .line_height(px(22.))
            .text_color(color("--text-muted"))
            .child(before)
            .child(
                div()
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(message.author.display_name().to_owned()),
            )
            .child(after)
            .child(
                div()
                    .ml(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight(500.))
                    .child(header_time(message.local_time(), chrono::Local::now())),
            )
            .into_any_element()
    }

    pub(super) fn system_message(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let (icon_name, tail, tint) = match message.kind {
            FRIEND_REQUEST_ACCEPTED => (
                "system_friend_accepted",
                super::message_part("H/9Tlj", &[("username", "".into())], 0),
                "--status-positive",
            ),
            CHANNEL_PINNED_MESSAGE => ("system_pinned", String::new(), "--channels-default"),
            THREAD_CREATED => ("system_thread", String::new(), "--text-muted"),
            _ => ("system_friend_accepted", String::new(), "--status-positive"),
        };
        div()
            .child(
                div()
                    .relative()
                    .pl(px(72.))
                    .pr(px(24.))
                    .py(px(2.))
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(4.))
                            .w(px(72.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(message.kind != USER_JOIN, |this| {
                                this.child(icon(icon_name, 18., tint))
                            })
                            .when(message.kind == USER_JOIN, |this| {
                                this.children(
                                    self.images
                                        .get("inbox:join")
                                        .cloned()
                                        .map(|image| img(image).size(px(16.))),
                                )
                            }),
                    )
                    .when(message.kind == CHANNEL_PINNED_MESSAGE, |this| {
                        this.child(self.pin_notice(message, cx))
                    })
                    .when(message.kind == THREAD_CREATED, |this| {
                        this.child(self.thread_created_line(message, cx))
                    })
                    .when(message.kind == USER_JOIN, |this| {
                        this.child(self.join_message_line(message))
                            .children(self.welcome_cta(message, cx))
                    })
                    .when(
                        !matches!(
                            message.kind,
                            CHANNEL_PINNED_MESSAGE | THREAD_CREATED | USER_JOIN
                        ),
                        |this| {
                            this.child(
                                div()
                                    .py(px(2.))
                                    .flex()
                                    .items_center()
                                    .line_height(px(22.))
                                    .text_color(color("--channels-default"))
                                    .child(
                                        self.profile_trigger(
                                            format!("system-name-{}", message.id),
                                            message.author.id,
                                            div()
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-strong"))
                                                .hover(|style| style.underline())
                                                .child(message.author.display_name().to_owned()),
                                            cx,
                                        ),
                                    )
                                    .child(tail)
                                    .child(
                                        div()
                                            .ml(px(4.))
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(500.))
                                            .child(header_time(
                                                message.local_time(),
                                                chrono::Local::now(),
                                            )),
                                    ),
                            )
                        },
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::welcome_sticker;

    #[test]
    fn the_wave_sticker_is_module_117096s_pick() {
        // The fixture's join message and account, whose official button showed Choco (index 3).
        assert_eq!(
            welcome_sticker(Some(1_137_833_003_905_724_474), 1_568_999_739_821_785_150),
            (781_291_131_828_699_156, 2)
        );
        let (sticker, _) = welcome_sticker(None, 1_568_999_739_821_785_150);
        assert!(sticker > 0);
    }
}
