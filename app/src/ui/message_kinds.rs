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
        // Modules 175476 and 130158: the handset, green for a call and red for a missed one, 16px (golden
        // `design/golden/windows/dm-call`). Drawn from the local glyph so it does not wait for the asset.
        match line_icon {
            LineIcon::Call => {
                return icon("system_call", 16., "--status-positive").into_any_element();
            }
            LineIcon::CallMissed => {
                return icon("system_call", 16., "--red-400").into_any_element();
            }
            _ => {}
        }
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
        // `.mentioned` (524434): the warning tint with a 2px warning bar, as on an ordinary message that mentions
        // this account (the poll-result line names its author).
        let mentioned = self.mentions_me(message);
        Some(
            div()
                .pt(px(if head { 17. } else { 0. }))
                .pr(px(16.))
                .child(
                    div()
                        .relative()
                        .when(mentioned, |this| {
                            this.bg(color("--message-mentioned-background-default"))
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(2.))
                                        .bg(color("--icon-feedback-warning")),
                                )
                        })
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
                        .child(self.line_text(message, &line, cx))
                        .children(if message.kind == message_kinds::POLL_RESULT {
                            self.poll_result_card(message, cx)
                        } else {
                            None
                        }),
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
            // Module 888675 renders `[children, timestamp]` and the action is one of the children: the link comes
            // before the time. Module 117096: a call line is joinable while its call is active and this account is
            // not in it.
            .children(
                line.action
                    .as_ref()
                    .filter(|_| {
                        self.stores
                            .calls
                            .is_call_active(message.channel_id, Some(message.id))
                            && !self.dm_call_in(Some(message.channel_id))
                    })
                    .map(|action| {
                        // `T.A.Action`: an em dash and the link, 3px in.
                        div().ml(px(3.)).flex().child("\u{2014}").child(
                            div()
                                .id(SharedString::from(format!("system-action-{}", message.id)))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-link"))
                                .child(action.clone()),
                        )
                    }),
            )
            .child(
                div()
                    .ml(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight(500.))
                    .child(header_time(message.local_time(), chrono::Local::now())),
            )
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
        if *set == "typing" || *set == "typing-off" {
            // One or two people typing in the open channel: `typing` starts it (the notice runs out after ten
            // seconds, as it does on the wire), `typing-off` ends it.
            if *set == "typing-off" {
                self.stores.typing.remove(&channel);
            } else {
                for (id, name) in [(900_u64, "ada"), (901, "grace")] {
                    if let Some(user) =
                        dc_state::User::from_value(&json!({"id": id.to_string(), "username": name}))
                    {
                        self.stores.users.insert(user.id, user);
                    }
                    self.stores.dispatch(
                        "TYPING_START",
                        &json!({"channel_id": channel.to_string(), "user_id": id.to_string()}),
                    );
                }
            }
            cx.notify();
            return format!("messages: typing {set} in {channel}");
        }
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
            "stickers" => {
                // Eight messages of one animated sticker each (the pack the sticker picker opens on), a screenful of
                // animation at once.
                let ids = [
                    796140620111544330u64,
                    796140638093443092,
                    796140656941989888,
                    796140687639838730,
                    796140711719469097,
                    796140731256799242,
                    796140753143201873,
                    796140775896776765,
                ];
                for (n, id) in ids.iter().enumerate() {
                    push(
                        0,
                        "",
                        json!({"sticker_items": [{"id": id.to_string(), "name": format!("sticker {n}"), "format_type": 3}]}),
                    );
                }
            }
            "emoji" => {
                // Animated custom emoji in text. The picture comes from the file named by
                // `GRAVECORD_DEMO_EMOJI_FILE` (an animated WebP) instead of the network.
                push(
                    0,
                    "an animated emoji <a:demo:999000000000000001> in a line",
                    json!({}),
                );
                if let Some(bytes) = std::env::var_os("GRAVECORD_DEMO_EMOJI_FILE")
                    .and_then(|path| std::fs::read(path).ok())
                {
                    self.images.insert(
                        "custom-animated:999000000000000001".to_owned(),
                        std::sync::Arc::new(gpui::Image::from_bytes(
                            gpui::ImageFormat::Webp,
                            bytes,
                        )),
                    );
                    self.requested
                        .insert("custom-animated:999000000000000001".to_owned());
                }
            }
            "alt" => {
                // A spoiler in a line, and a picture whose sender wrote a description (shown by "Show image
                // descriptions"). The picture comes from `GRAVECORD_DEMO_EMOJI_FILE` instead of the network.
                push(0, "a line with ||a hidden spoiler|| inside", json!({}));
                push(
                    0,
                    "",
                    json!({"attachments": [{"id": "777001", "filename": "cat.png", "size": 1000,
                        "url": "https://cdn.example.invalid/cat.png", "proxy_url": "https://cdn.example.invalid/cat.png",
                        "content_type": "image/png", "width": 320, "height": 200,
                        "description": "A grey cat asleep on a windowsill in the morning sun"}]}),
                );
                // An animated upload: "Play animated GIFs" decides whether it moves.
                push(
                    0,
                    "",
                    json!({"attachments": [{"id": "777002", "filename": "anim.gif", "size": 1000,
                        "url": "https://cdn.example.invalid/anim.gif", "proxy_url": "https://cdn.example.invalid/anim.gif",
                        "content_type": "image/gif", "width": 160, "height": 100}]}),
                );
                if let Some(bytes) = std::env::var_os("GRAVECORD_DEMO_EMOJI_FILE")
                    .and_then(|path| std::fs::read(path).ok())
                {
                    for key in ["attachment:777001", "attachment:777002"] {
                        self.images.insert(
                            key.to_owned(),
                            std::sync::Arc::new(gpui::Image::from_bytes(
                                gpui::ImageFormat::Webp,
                                bytes.clone(),
                            )),
                        );
                    }
                }
            }
            "avatar" => {
                // Two messages from a user whose avatar hash is animated (`a_`) and one from a user whose is not. The
                // picture of the first comes from the file named by `GRAVECORD_DEMO_AVATAR_FILE` (an animated WebP)
                // instead of the network, under the keys of the files drawn at 64 and 128 pixels.
                let animated = json!({"id": "777", "username": "mover", "avatar": "a_demo"});
                push(0, "my avatar plays on hover", json!({"author": animated}));
                push(0, "and again", json!({"author": animated}));
                if let Some(bytes) = std::env::var_os("GRAVECORD_DEMO_AVATAR_FILE")
                    .and_then(|path| std::fs::read(path).ok())
                {
                    let image = std::sync::Arc::new(gpui::Image::from_bytes(
                        gpui::ImageFormat::Webp,
                        bytes,
                    ));
                    self.images
                        .insert("avatar:777:a_demo".to_owned(), image.clone());
                    self.requested.insert("avatar:777:a_demo".to_owned());
                    for side in [64, 128] {
                        let key = format!(
                            "avatar-live:https://cdn.discordapp.com/avatars/777/a_demo:{side}"
                        );
                        self.images.insert(key.clone(), image.clone());
                        self.requested.insert(key);
                    }
                }
            }
            "command-feedback" => {
                // A deferred reply (the loading flag) and a private reply.
                let bot = json!({"id": "902", "username": "Helper", "bot": true});
                push(
                    20,
                    "",
                    json!({"author": bot, "flags": 128, "interaction": {"id": "1", "type": 2, "name": "ping", "user": author(901, "grace")}}),
                );
                push(
                    20,
                    "Only the person who ran it sees this",
                    json!({"author": bot, "flags": 64, "interaction": {"id": "2", "type": 2, "name": "secret", "user": author(901, "grace")}}),
                );
            }
            "components" => {
                // A bot's message under each control: every button style, the five selects, and a disabled row.
                let bot = json!({"id": "902", "username": "Helper", "bot": true});
                let button = |style: u64, label: &str, id: &str| json!({"type": 2, "style": style, "label": label, "custom_id": id});
                push(
                    0,
                    "Pick one of these",
                    json!({"author": bot, "application_id": "902", "components": [
                        {"type": 1, "components": [
                            button(1, "Primary", "p"),
                            button(2, "Secondary", "s"),
                            button(3, "Success", "g"),
                            button(4, "Danger", "d"),
                            {"type": 2, "style": 5, "label": "Open the docs", "url": "https://discord.com/developers/docs"},
                            {"type": 2, "style": 1, "label": "Disabled", "custom_id": "x", "disabled": true},
                            {"type": 2, "style": 2, "emoji": {"name": "\u{1f44d}"}, "custom_id": "e"}
                        ]}
                    ]}),
                );
                push(
                    0,
                    "A string select, a multi select",
                    json!({"author": bot, "application_id": "902", "components": [
                        {"type": 1, "components": [{"type": 3, "custom_id": "one", "placeholder": "Choose a color", "options": [
                            {"label": "Red", "value": "r", "description": "The warm one"},
                            {"label": "Green", "value": "g"},
                            {"label": "Blue", "value": "b", "default": true}
                        ]}]},
                        {"type": 1, "components": [{"type": 3, "custom_id": "many", "min_values": 1, "max_values": 3, "options": [
                            {"label": "Alpha", "value": "a", "default": true},
                            {"label": "Beta", "value": "b", "default": true},
                            {"label": "Gamma", "value": "c"}
                        ]}]}
                    ]}),
                );
                push(
                    0,
                    "Entity selects",
                    json!({"author": bot, "application_id": "902", "components": [
                        {"type": 1, "components": [{"type": 5, "custom_id": "u", "placeholder": "Select a user"}]},
                        {"type": 1, "components": [{"type": 6, "custom_id": "r", "placeholder": "Select a role"}]},
                        {"type": 1, "components": [{"type": 8, "custom_id": "c", "placeholder": "Select a channel", "channel_types": [0]}]},
                        {"type": 1, "components": [{"type": 7, "custom_id": "m", "disabled": true, "placeholder": "Disabled mentionable"}]}
                    ]}),
                );
            }
            "components-v2" => {
                // The newer layout: a container with an accent, a section with a thumbnail, text, a separator, a
                // gallery and a file, then loose buttons.
                let bot = json!({"id": "902", "username": "Helper", "bot": true});
                push(
                    0,
                    "",
                    json!({"author": bot, "application_id": "902", "flags": 32768, "components": [
                        {"type": 10, "content": "# Release notes\nWhat changed this week, with a [link](https://discord.com)."},
                        {"type": 17, "accent_color": 5793266, "components": [
                            {"type": 9, "components": [
                                {"type": 10, "content": "**Server status**"},
                                {"type": 10, "content": "All systems are running."}
                            ], "accessory": {"type": 11, "media": {"url": "https://cdn.discordapp.com/embed/avatars/1.png"}}},
                            {"type": 14, "divider": true, "spacing": 1},
                            {"type": 10, "content": "Reduced text inside a container."},
                            {"type": 12, "items": [
                                {"media": {"url": "https://cdn.discordapp.com/embed/avatars/2.png", "width": 128, "height": 128}},
                                {"media": {"url": "https://cdn.discordapp.com/embed/avatars/3.png", "width": 128, "height": 128}}
                            ]},
                            {"type": 13, "file": {"url": "attachment://report.txt"}, "name": "report.txt", "size": 1234},
                            {"type": 1, "components": [
                                {"type": 2, "style": 1, "label": "Approve", "custom_id": "ok"},
                                {"type": 2, "style": 2, "label": "Later", "custom_id": "later"}
                            ]}
                        ]},
                        {"type": 14, "divider": true, "spacing": 2},
                        {"type": 9, "components": [{"type": 10, "content": "A section with a button"}],
                         "accessory": {"type": 2, "style": 2, "label": "Open", "custom_id": "open"}}
                    ]}),
                );
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
