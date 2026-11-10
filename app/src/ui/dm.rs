//! Direct messages: the welcome at the start of the history (`container_w6h4R2`) and system messages
//! (`container_bLMKT5`). Measured in `design/dom/dm/loreanxavier-darker-cozy.json`.

use super::{Root, avatar_source, color, header_time, icon};
use crate::parity::Parity;
use dc_state::Message;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// `FRIEND_REQUEST_ACCEPTED` in the message type enum.
pub(super) const FRIEND_REQUEST_ACCEPTED: u64 = 67;
/// `CHANNEL_PINNED_MESSAGE`.
pub(super) const CHANNEL_PINNED_MESSAGE: u64 = 6;

/// `USER_JOIN` and `THREAD_CREATED`.
pub(super) const USER_JOIN: u64 = 7;
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
enum Tone {
    Secondary,
    Primary,
    CriticalPrimary,
}

/// The `sm` design-system button of the DM welcome, in the variant the relationship asks for.
fn small_button(
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

impl Root {
    /// The start of a DM: the other person large, the opening line, mutual servers and the friend buttons.
    pub(super) fn dm_welcome(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.current_dm()?;
        let user = self.stores.dm_recipient(channel)?.clone();
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
            div().pt(px(16.)).child(
            div()
                .parity("dm-welcome")
                .ml(px(16.))
                .mr(px(32.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .size(px(80.))
                        .rounded_full()
                        .children(avatar.map(|picture| img(picture).size(px(80.)).rounded_full())),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .text_size(px(32.))
                        .line_height(px(40.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(name.clone()),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .text_size(px(24.))
                        .line_height(px(32.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(user.username.clone()),
                )
                .child(
                    div()
                        .mt(px(20.))
                        .flex()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child("This is the beginning of your direct message history with\u{a0}")
                        .child(div().font_weight(FontWeight(600.)).child(name))
                        .child("."),
                )
                .child(
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
                                            .children(
                                                first_mutual
                                                    .map(|picture| img(picture).size(px(24.))),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-subtle"))
                                            .child(if mutual == 1 {
                                                "1 Mutual Server".to_owned()
                                            } else {
                                                format!("{mutual} Mutual Servers")
                                            }),
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
                ).into_any_element(),
        )
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
        let mut list = Vec::new();
        match relationship {
            1 => list.push(
                small_button("welcome-remove", "Remove Friend", Tone::Secondary, true)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.set_relationship(id, None, cx)),
                    )
                    .into_any_element(),
            ),
            2 => list.push(
                small_button("welcome-unblock", "Unblock", Tone::Secondary, true)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.set_relationship(id, None, cx)),
                    )
                    .into_any_element(),
            ),
            4 => list.push(
                small_button("welcome-sent", "Friend Request Sent", Tone::Primary, false)
                    .into_any_element(),
            ),
            _ => list.push(
                small_button("welcome-add", "Add Friend", Tone::Primary, true)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.set_relationship(id, Some(0), cx)),
                    )
                    .into_any_element(),
            ),
        }
        if relationship != 2 {
            list.push(
                small_button("welcome-block", "Block", Tone::Secondary, true)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_relationship(id, Some(2), cx);
                        this.show_success_toast(format!("{name} blocked"), cx);
                    }))
                    .into_any_element(),
            );
        }
        // `s2.I`: a message of theirs in this DM to report (a system line counts: it is theirs too).
        let first_theirs = self
            .stores
            .messages
            .get(&channel)
            .and_then(|messages| messages.iter().find(|message| message.author.id == id))
            .cloned();
        if matches!(relationship, 0 | 2 | 3)
            && let Some(first) = first_theirs
        {
            list.push(
                small_button("welcome-report", "Report Spam", Tone::CriticalPrimary, true)
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
            .child("\u{a0}pinned\u{a0}")
            .child(
                link(
                    format!("pin-notice-message-{}", message.id),
                    "a message".to_owned(),
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
            .child("\u{a0}to this channel. See all\u{a0}")
            .child(
                link(
                    format!("pin-notice-pins-{}", message.id),
                    "pinned messages".to_owned(),
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

    /// `getSystemMessageUserJoin` in module 779014: one of thirteen lines, picked by the message's snowflake time
    /// modulo 13, as (before, after) around the name.
    pub(super) fn join_line(message: &Message) -> (&'static str, &'static str) {
        const LINES: [(&str, &str); 13] = [
            ("", " joined the party."),
            ("", " is here."),
            ("Welcome, ", ". We hope you brought pizza."),
            ("A wild ", " appeared."),
            ("", " just landed."),
            ("", " just slid into the server."),
            ("", " just showed up!"),
            ("Welcome ", ". Say hi!"),
            ("", " hopped into the server."),
            ("Everyone welcome ", "!"),
            ("Glad you're here, ", "."),
            ("Good to see you, ", "."),
            ("Yay you made it, ", "!"),
        ];
        let timestamp = (message.id >> 22) + 1_420_070_400_000;
        LINES[(timestamp % LINES.len() as u64) as usize]
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
                                "1 Message \u{203a}".to_owned()
                            } else {
                                format!("{count} Messages \u{203a}")
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
                    .child("\u{a0}started a thread:\u{a0}")
                    .child(bold(message.content.clone()))
                    .child(".\u{a0}See all\u{a0}")
                    .child(
                        bold("threads".to_owned())
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

    pub(super) fn system_message(
        &self,
        message: &Message,
        head: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (icon_name, tail, tint) = match message.kind {
            FRIEND_REQUEST_ACCEPTED => (
                "system_friend_accepted",
                " accepted your friend request.",
                "--status-positive",
            ),
            CHANNEL_PINNED_MESSAGE => ("system_pinned", "", "--channels-default"),
            THREAD_CREATED => ("system_thread", "", "--text-muted"),
            _ => ("system_friend_accepted", "", "--status-positive"),
        };
        div()
            .pt(px(if head { 17. } else { 0. }))
            .pr(px(16.))
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
