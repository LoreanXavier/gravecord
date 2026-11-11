//! Message types in the list: the thread starter (`n$` in module 891522), the system lines of the table `ls` in module
//! 117096 (the shell `T.A` of module 888675: the icon in the 72px gutter, the text, the time), and the "used /command"
//! bar of a command reply (`nV`). What each type draws is decided in `dc_state::message_kinds`.

use super::{Root, color, header_time, icon};
use dc_state::Message;
use dc_state::message_kinds::{self, Line, LineContext, LineIcon, Part};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px, rgb,
};

/// The image behind an icon that the official draws as a CSS background (the `n(...)` asset modules), as the cache
/// key and the address.
fn icon_image(icon: LineIcon) -> Option<(&'static str, &'static str)> {
    Some(match icon {
        // Module 617184, also the join line's arrow.
        LineIcon::ArrowIn => ("inbox:join", super::dm::JOIN_ARROW),
        // Module 884797.
        LineIcon::ArrowOut => (
            "system:arrow-out",
            "https://discordapp.com/assets/53e42368b1409f1a.svg",
        ),
        // Module 175476.
        LineIcon::Call => (
            "system:call",
            "https://discordapp.com/assets/61ccab047dad737a.svg",
        ),
        // Module 130158.
        LineIcon::CallMissed => (
            "system:call-missed",
            "https://discordapp.com/assets/6f076a64de25de4b.svg",
        ),
        // Module 290388.
        LineIcon::Warning => (
            "system:warning",
            "https://discordapp.com/assets/49dd33256719a93e.svg",
        ),
        // Module 222548.
        LineIcon::Discovery => (
            "system:discovery",
            "https://discordapp.com/assets/5c56b22c17556059.svg",
        ),
        _ => return None,
    })
}

/// Every image the system lines of these messages need, as (key, address).
pub(super) fn wanted(messages: &[Message]) -> Vec<(String, String)> {
    let mut images = Vec::new();
    for message in messages {
        if message_kinds::presentation(message) != message_kinds::Presentation::System {
            continue;
        }
        if let Some(line) = message_kinds::system_line(message, &LineContext::default())
            && let Some((key, url)) = icon_image(line.icon)
        {
            images.push((key.to_owned(), url.to_owned()));
        }
    }
    images
}

impl Root {
    /// What `system_line` needs to know about the channel the list is showing.
    fn line_context(&self) -> LineContext {
        let thread = self.current_thread();
        let in_forum_post = thread.as_ref().is_some_and(|thread| {
            self.current_guild().is_some_and(|guild| {
                guild
                    .channels
                    .iter()
                    .any(|channel| channel.id == thread.parent_id && channel.is_forum())
            })
        });
        LineContext {
            guild_name: self.current_guild().map(|guild| guild.name.clone()),
            in_thread: thread.is_some(),
            // `ChannelTypes.GROUP_DM`.
            in_group_dm: self.current_dm().is_some_and(|dm| dm.kind == 3),
            in_forum_post,
            me: self.stores.current_user.as_ref().map_or(0, |user| user.id),
        }
    }

    /// The message a thread starter stands for: the one it references, as the server embedded it, else one already
    /// loaded in the parent channel.
    pub(super) fn starter_target(&self, starter: &Message) -> Option<Message> {
        if let Some(referenced) = &starter.referenced {
            return Some((**referenced).clone());
        }
        let (channel, id) = (starter.reference_channel_id?, starter.reference_id?);
        self.stores
            .messages
            .get(&channel)?
            .iter()
            .find(|known| known.id == id)
            .cloned()
    }

    /// `n$` without a loaded message: the thread icon with "Sorry, we couldn't load the first message in this thread",
    /// or "Original message was deleted" when the server said the message is gone.
    pub(super) fn starter_missing_line(&self, starter: &Message) -> AnyElement {
        let text = if starter.extra.reference_deleted {
            "Original message was deleted"
        } else {
            "Sorry, we couldn't load the first message in this thread"
        };
        div()
            .relative()
            .py(px(2.))
            .flex()
            .items_start()
            .line_height(px(22.))
            .text_color(color("--channels-default"))
            .child(
                div()
                    .absolute()
                    .left(px(-72.))
                    .top(px(2.))
                    .w(px(72.))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("system_thread", 18., "--channels-default")),
            )
            .child(text)
            .into_any_element()
    }

    /// The gutter icon of a system line.
    fn line_icon(&self, line_icon: LineIcon) -> AnyElement {
        if let Some((key, _)) = icon_image(line_icon) {
            return div()
                .size(px(16.))
                .flex_none()
                .children(
                    self.images
                        .get(key)
                        .cloned()
                        .map(|image| img(image).size(px(16.))),
                )
                .into_any_element();
        }
        match line_icon {
            LineIcon::Pencil => icon("system_pencil", 16., "--channels-default").into_any_element(),
            LineIcon::Pin => icon("system_pinned", 18., "--channels-default").into_any_element(),
            LineIcon::Thread => icon("system_thread", 18., "--channels-default").into_any_element(),
            // `StageIcon` md; a started stage is `GREEN_360`.
            LineIcon::StageStart => svg_in("system_stage", 24., rgb(0x23a55a)),
            LineIcon::Stage => icon("system_stage", 24., "--text-muted").into_any_element(),
            LineIcon::Hand => icon("system_hand", 24., "--text-muted").into_any_element(),
            LineIcon::SpeakerArrow => {
                icon("system_mic_arrow", 24., "--text-muted").into_any_element()
            }
            LineIcon::Ticket => icon("system_ticket", 16., "--channels-default").into_any_element(),
            LineIcon::Refresh => {
                icon("system_refresh", 18., "--status-positive").into_any_element()
            }
            LineIcon::Voice => icon("system_voice", 24., "--status-positive").into_any_element(),
            LineIcon::GameController => {
                icon("system_game_controller", 18., "--channels-default").into_any_element()
            }
            LineIcon::Polls => icon("system_polls", 16., "--channels-default").into_any_element(),
            // `BoostGemIcon` in `GUILD_BOOSTING_PINK`, 16px.
            LineIcon::Boost => svg_in("system_boost_gem", 16., rgb(0xff73fa)),
            _ => div().into_any_element(),
        }
    }

    /// A system line from the table, or `None` when the official handler draws nothing for this message.
    pub(super) fn kind_line(
        &self,
        message: &Message,
        head: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let line = message_kinds::system_line(message, &self.line_context())?;
        Some(
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
                                .child(self.line_icon(line.icon)),
                        )
                        .child(self.line_text(message, &line, cx)),
                )
                .into_any_element(),
        )
    }

    /// The text of a line: words that wrap, names that open the profile, then the time (`T.A`'s timestamp) and the
    /// trailing action.
    fn line_text(&self, message: &Message, line: &Line, cx: &mut Context<Self>) -> AnyElement {
        let mut pieces: Vec<AnyElement> = Vec::new();
        let words = |text: &str, strong: bool| -> Vec<AnyElement> {
            text.split_inclusive(' ')
                .map(|word| {
                    div()
                        .when(strong, |this| {
                            this.font_weight(FontWeight(500.))
                                .text_color(color("--interactive-text-active"))
                        })
                        .child(word.to_owned())
                        .into_any_element()
                })
                .collect()
        };
        for (index, part) in line.parts.iter().enumerate() {
            match part {
                Part::Text(text) => pieces.extend(words(text, false)),
                Part::Bold(text) => pieces.extend(words(text, true)),
                Part::Link(text) | Part::Name(text) => pieces.extend(words(text, true)),
                Part::User(user) => pieces.push(
                    self.profile_trigger(
                        format!("system-{}-{index}", message.id),
                        user.id,
                        div()
                            .font_weight(FontWeight(500.))
                            .text_color(color("--interactive-text-active"))
                            .hover(|style| style.underline())
                            .child(self.guild_display_name(user)),
                        cx,
                    ),
                ),
            }
        }
        div()
            .py(px(2.))
            .flex()
            .flex_wrap()
            .items_start()
            .line_height(px(22.))
            .text_color(color("--channels-default"))
            .children(pieces)
            .child(
                div()
                    .ml(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight(500.))
                    .child(header_time(message.local_time(), chrono::Local::now())),
            )
            .children(line.action.as_ref().map(|action| {
                // `T.A.Action`: an em dash and the link, 3px in.
                div().ml(px(3.)).flex().child("\u{2014}").child(
                    div()
                        .id(SharedString::from(format!("system-action-{}", message.id)))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-link"))
                        .child(action.clone()),
                )
            }))
            .into_any_element()
    }

    /// `nV`: the bar above a command's reply, the avatar and "{user} used /{command}".
    pub(super) fn interaction_bar(&self, message: &Message) -> Option<AnyElement> {
        let (name, user) = message.extra.interaction.as_ref()?;
        let avatar = self.images.get(&self.guild_avatar_source(user).0).cloned();
        Some(
            div()
                .relative()
                .mb(px(4.))
                .h(px(18.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .child(
                    div()
                        .absolute()
                        .left(px(-37.))
                        .top(px(8.))
                        .w(px(33.))
                        .h(px(12.))
                        .border_l_2()
                        .border_t_2()
                        .rounded_tl(px(6.))
                        .border_color(color("--spine-default")),
                )
                .child(
                    div()
                        .size(px(16.))
                        .flex_none()
                        .children(avatar.map(|image| img(image).size(px(16.)).rounded_full())),
                )
                .child(
                    div()
                        .ml(px(4.))
                        .flex_none()
                        .opacity(0.64)
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(self.guild_display_name(user)),
                )
                .child(
                    div()
                        .ml(px(4.))
                        .flex_none()
                        .text_color(color("--text-default"))
                        .child("used"),
                )
                .child(
                    div()
                        .ml(px(4.))
                        .flex_none()
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-link"))
                        .child(format!("/{name}")),
                )
                .into_any_element(),
        )
    }
}

/// An icon in a fixed colour (the module passes `color` as a raw value).
fn svg_in(name: &str, size: f32, tint: gpui::Rgba) -> AnyElement {
    gpui::svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(tint)
        .into_any_element()
}

/// Capture harness: messages of kinds the fixture server has none of, put into the open channel in memory only (nothing
/// is sent anywhere).
#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_messages(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        use serde_json::json;
        let ["demo", set] = words else {
            return "messages: usage `messages demo <system|starter|command|content>`".to_owned();
        };
        let Some(channel) = self.channel else {
            return "messages: no channel is open".to_owned();
        };
        if *set == "unread" {
            // Every text channel but the open one gets a newer message than its ack, so its row reads unread.
            let newest = ((chrono::Utc::now().timestamp_millis() - 1_420_070_400_000) as u64) << 22;
            let others: Vec<dc_state::Id> = self
                .current_guild()
                .map(|guild| {
                    guild
                        .channels
                        .iter()
                        .filter(|one| one.kind == dc_state::ChannelKind::Text)
                        .map(|one| one.id)
                        .collect()
                })
                .unwrap_or_default();
            for one in &others {
                self.stores.read_states.ack_local(*one, 1, false);
                self.stores
                    .read_states
                    .message_created(*one, newest, None, false);
            }
            cx.notify();
            return format!("messages: {} channels read as unread", others.len());
        }
        let author = |id: u64, name: &str| json!({"id": id.to_string(), "username": name});
        let now = chrono::Utc::now();
        let base = ((now.timestamp_millis() - 1_420_070_400_000) as u64) << 22;
        let mut made: Vec<serde_json::Value> = Vec::new();
        let mut push = |kind: u64, content: &str, extra: serde_json::Value| {
            let n = made.len() as u64;
            let mut value = json!({
                "id": (base + n + 1).to_string(),
                "channel_id": channel.to_string(),
                "type": kind,
                "content": content,
                "timestamp": (now - chrono::Duration::seconds(300 - n as i64))
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                "author": author(900, "ada"),
            });
            for (key, field) in extra.as_object().into_iter().flatten() {
                value[key] = field.clone();
            }
            made.push(value);
        };
        match *set {
            "system" => {
                push(4, "general-two", json!({}));
                push(8, "", json!({}));
                push(9, "", json!({}));
                push(10, "", json!({}));
                push(11, "", json!({}));
                push(
                    12,
                    "Release notes #news",
                    json!({"message_reference": {"guild_id": "1", "channel_id": "2"}}),
                );
                push(14, "", json!({}));
                push(15, "", json!({}));
                push(16, "", json!({}));
                push(17, "", json!({}));
                push(27, "Friday show", json!({}));
                push(28, "Friday show", json!({}));
                push(29, "", json!({}));
                push(30, "", json!({}));
                push(31, "Friday show: part two", json!({}));
                push(
                    32,
                    "",
                    json!({"application": {"id": "5", "name": "Helper"}}),
                );
                push(
                    33,
                    "",
                    json!({"application": {"id": "5", "name": "Helper"}}),
                );
                push(
                    34,
                    "",
                    json!({"application": {"id": "5", "name": "Helper"}}),
                );
            }
            "system2" => {
                push(36, "", json!({}));
                push(37, "", json!({}));
                push(38, "", json!({}));
                push(39, "", json!({}));
                push(
                    44,
                    "",
                    json!({"purchase_notification": {"type": 0, "guild_product_purchase": {"listing_id": "1", "product_name": "Sticker pack"}}}),
                );
                push(
                    25,
                    "",
                    json!({"role_subscription_data": {"role_subscription_listing_id": "1", "tier_name": "Gold", "total_months_subscribed": 3, "is_renewal": false}}),
                );
                push(
                    51,
                    "",
                    json!({"application": {"id": "5", "name": "Some Game"}}),
                );
                push(52, "", json!({}));
                push(53, "", json!({}));
                push(54, "", json!({}));
                push(65, "", json!({}));
                push(
                    46,
                    "",
                    json!({"embeds": [{"type": "poll_result", "fields": [{"name": "poll_question_text", "value": "Which day for the stream?"}, {"name": "victor_answer_text", "value": "Friday"}, {"name": "total_votes", "value": "7"}]}]}),
                );
                // Types the official table draws nothing for: these four add no row.
                push(22, "", json!({}));
                push(40, "", json!({}));
                push(55, "", json!({}));
                push(63, "", json!({}));
                push(
                    0,
                    "the line above these four rows ends the system lines",
                    json!({}),
                );
            }
            "starter" => {
                // A starter whose message the server could not send, and one it said was deleted.
                push(
                    21,
                    "",
                    json!({"message_reference": {"channel_id": "1", "message_id": "2"}}),
                );
                push(
                    21,
                    "",
                    json!({"message_reference": {"channel_id": "1", "message_id": "3"}, "referenced_message": null}),
                );
            }
            "command" => {
                push(
                    20,
                    "pong",
                    json!({"interaction": {"id": "1", "type": 2, "name": "ping", "user": author(901, "grace")}, "author": author(902, "Helper")}),
                );
                push(
                    23,
                    "Translated: hello",
                    json!({"interaction": {"id": "2", "type": 3, "name": "Translate", "user": author(901, "grace")}, "author": author(902, "Helper")}),
                );
            }
            "reactions" => {
                // A message with three pills, and the reactors a fetch would have brought for each.
                let pills = [
                    ("\u{1f602}", 5, vec!["ada", "grace", "linus"]),
                    ("\u{2764}\u{fe0f}", 2, vec!["ada", "grace"]),
                    ("\u{1f44d}", 1, vec!["ada"]),
                ];
                let reactions: Vec<_> = pills
                    .iter()
                    .map(|(name, count, _)| json!({"emoji": {"id": null, "name": name}, "count": count, "me": false}))
                    .collect();
                push(0, "Reactions fixture", json!({"reactions": reactions}));
                let message = base + 1;
                for (name, _, who) in &pills {
                    let users: Vec<_> = who
                        .iter()
                        .enumerate()
                        .map(|(index, one)| author(700 + index as u64, one))
                        .collect();
                    let key = format!("reactors3:{message}:{name}");
                    self.requested.insert(key.clone());
                    self.fetched.insert(key, json!(users));
                }
            }
            other => return format!("messages: no demo set {other}"),
        }
        let list = self.stores.messages.entry(channel).or_default();
        list.extend(made.iter().filter_map(dc_state::Message::from_value));
        let added = made.len();
        self.request_images();
        self.rebuild_rows();
        cx.notify();
        format!("messages: added {added} demo messages to {channel}")
    }
}
