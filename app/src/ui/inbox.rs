//! The titlebar Inbox (`container_uTMZTY`, 560 wide, at most 576 tall), measured in
//! `design/dom/popouts/inbox-*-darker-cozy.json`: a header (inbox icon, "Inbox", Mark All as Read on Unreads, View
//! Friend Requests), five tabs (Unreads, Mentions, Bookmarks, Reminders (n), Scheduled) and a scrolling page of
//! 12px-round cards. Reminders and bookmarks come from `GET /users/@me/saved-messages` (each result carries its
//! message), mentions from `GET /users/@me/mentions?limit=25&roles=true&everyone=true`.

use super::{Root, avatar_source, color, icon};
use crate::parity::Parity;
use dc_state::{Guild, Id, Message};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, deferred, div, img,
    prelude::*, px,
};
use serde_json::Value;

const WIDTH: f32 = 560.;
/// `upsellImage_jMz1Ee` and `upsellImage_XzStbz`, and the demo message's Wumpus.
const BOOKMARKS_PICTURE: &str = "https://cdn.discordapp.com/assets/content/bd0327359da757ec3765798e53afa43e172da33e2255bf985c3667f9ff57c8cb.svg";
const SCHEDULED_PICTURE: &str = "https://cdn.discordapp.com/assets/content/c5582c3bbb97001b8a0f4686a09a93c734eef416f3d0e3c83eba4b6e8bbee2c2.svg";
const WUMPUS_AVATAR: &str = "https://cdn.discordapp.com/assets/content/2532e8cd0600ccfbc61ff81c21959bde90a6645d0e71644a92f6cde5bfc48439.png";
const MAX_HEIGHT: f32 = 576.;
pub(super) const MENTIONS_KEY: &str = "inbox-mentions";

/// RecentMentionsStore's filter (`recentMentionFilterSettings` in local storage): @everyone and role mentions on by
/// default, every server unless "This Server" was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MentionFilter {
    pub everyone: bool,
    pub roles: bool,
    pub all_servers: bool,
}

impl Default for MentionFilter {
    fn default() -> Self {
        Self {
            everyone: true,
            roles: true,
            all_servers: true,
        }
    }
}

const FILTER_STORAGE: &str = "recentMentionFilterSettings";

impl MentionFilter {
    pub(super) fn stored() -> Self {
        let Some(value) = crate::storage::get(FILTER_STORAGE) else {
            return Self::default();
        };
        Self {
            everyone: value["everyoneFilter"].as_bool().unwrap_or(true),
            roles: value["roleFilter"].as_bool().unwrap_or(true),
            all_servers: value["guildFilter"].as_str() != Some("THIS_SERVER"),
        }
    }

    fn store(self) {
        crate::storage::set(
            FILTER_STORAGE,
            serde_json::json!({
                "guildFilter": if self.all_servers { "ALL_SERVERS" } else { "THIS_SERVER" },
                "everyoneFilter": self.everyone,
                "roleFilter": self.roles,
            }),
        );
    }

    /// `fetchRecentMentions` query: `guild_id` only for "This Server" inside a server.
    pub(super) fn route(self, guild: Option<Id>) -> String {
        let mut route = format!(
            "/users/@me/mentions?limit=25&roles={}&everyone={}",
            self.roles, self.everyone
        );
        if let (false, Some(guild)) = (self.all_servers, guild) {
            route.push_str(&format!("&guild_id={guild}"));
        }
        route
    }
}
pub(super) const UNREAD_PREFIX: &str = "inbox-unread:";
/// The Unreads tutorial card stays dismissed once "Got it!" is pressed.
const TUTORIAL_SEEN: &str = "inbox-unreads-tutorial-seen";
/// The join line's green arrow, a CSS background in the capture (`icon_bLMKT5`).
const JOIN_ARROW: &str = "https://discordapp.com/assets/8b9de960da063cb9.svg";
/// USER_JOIN.
const USER_JOIN: u64 = 7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum InboxTab {
    #[default]
    Unreads,
    Mentions,
    Bookmarks,
    Reminders,
    Scheduled,
    /// Not in the pinned client: KeywordNotify's log (`plugin_hooks/keyword_notify.rs`).
    Keywords,
}

/// One saved message from the saved-messages answer: the message and its reminder time, if any.
pub(super) fn saved_entries(body: &Value) -> Vec<(Message, Option<String>)> {
    body["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let message = Message::from_value(&entry["message"])?;
            let due = entry["save_data"]["due_at"].as_str().map(str::to_owned);
            Some((message, due))
        })
        .collect()
}

impl Root {
    /// Unread, unmuted text channels in rail order, as the Unreads tab lists them.
    pub(super) fn unread_channels(&self) -> Vec<(Id, Id)> {
        let now = chrono::Utc::now();
        self.stores
            .guilds
            .iter()
            .flat_map(|guild| {
                // In the guild's own order: categories by position, channels by position inside them.
                let place = |channel: &dc_state::Channel| {
                    let parent = channel
                        .parent_id
                        .and_then(|parent| guild.channels.iter().find(|known| known.id == parent))
                        .map_or(-1, |parent| parent.position);
                    (parent, channel.position)
                };
                let mut channels: Vec<&dc_state::Channel> = guild.channels.iter().collect();
                channels.sort_by_key(|channel| place(channel));
                channels
                    .into_iter()
                    .filter(|channel| channel.kind == dc_state::ChannelKind::Text)
                    .filter(move |channel| {
                        !self
                            .stores
                            .guild_settings
                            .channel_muted(guild.id, channel.id, now)
                    })
                    .filter(|channel| self.stores.is_unread(channel.id))
                    .map(move |channel| (guild.id, channel.id))
            })
            .filter(|(_, channel)| !self.inbox_read.contains(channel))
            .collect()
    }

    fn request_unreads(&mut self) {
        for (_, channel) in self.unread_channels().into_iter().take(20) {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: format!("{UNREAD_PREFIX}{channel}"),
                route: format!("/channels/{channel}/messages?limit=25"),
            });
        }
    }

    pub(super) fn toggle_inbox(&mut self, cx: &mut Context<Self>) {
        if self.inbox.take().is_none() {
            self.close_competing_popouts(super::keyboard::Layer::Inbox);
            // `onOpen` of the inbox button (module 113367 `Vc`): the overdue reminders' notice stops.
            self.mark_inbox_opened();
            self.inbox = Some(InboxTab::default());
            self.request_unreads();
            self.request_image("inbox:join".to_owned(), JOIN_ARROW.to_owned());
            self.request_saved();
            self.request_image("inbox:bookmarks".to_owned(), BOOKMARKS_PICTURE.to_owned());
            self.request_image("inbox:scheduled".to_owned(), SCHEDULED_PICTURE.to_owned());
            self.request_image("inbox:wumpus".to_owned(), WUMPUS_AVATAR.to_owned());
        }
        cx.notify();
    }

    /// `TOGGLE_INBOX` with the Scheduled tab (Manage Messages of the scheduled limit alert, module 859403).
    pub(super) fn open_inbox_scheduled(&mut self, cx: &mut Context<Self>) {
        if self.inbox.is_none() {
            self.toggle_inbox(cx);
        }
        self.set_inbox_tab(InboxTab::Scheduled, cx);
    }

    fn set_inbox_tab(&mut self, tab: InboxTab, cx: &mut Context<Self>) {
        self.inbox = Some(tab);
        if tab == InboxTab::Unreads {
            self.request_unreads();
        }
        if tab == InboxTab::Mentions {
            self.fetch_mentions();
        }
        // The Scheduled list is read when its tab opens (`sy` in module 713024).
        if tab == InboxTab::Scheduled {
            self.fetch_scheduled_messages();
        }
        cx.notify();
    }

    /// RecentMentionsStore (module 398269): `MESSAGE_DELETE`, `RECENT_MENTION_DELETE {message_id}` and
    /// `MESSAGE_DELETE_BULK` drop the messages from the Mentions list.
    pub(super) fn mentions_deleted(&mut self, name: &str, data: &Value, cx: &mut Context<Self>) {
        let ids: Vec<&str> = match name {
            "RECENT_MENTION_DELETE" => data["message_id"].as_str().into_iter().collect(),
            "MESSAGE_DELETE" => data["id"].as_str().into_iter().collect(),
            "MESSAGE_DELETE_BULK" => data["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect(),
            _ => return,
        };
        if let Some(Value::Array(list)) = self.fetched.get_mut(MENTIONS_KEY)
            && drop_mentions(list, &ids)
        {
            cx.notify();
        }
    }

    /// `deleteRecentMention` (module 378939): `DELETE /users/@me/mentions/{id}` and `RECENT_MENTION_DELETE`, which
    /// drops the message from the list at once.
    pub(super) fn delete_recent_mention(&mut self, message: Id, cx: &mut Context<Self>) {
        let _ = self.requests.send(crate::net::Request::Rest {
            key: None,
            method: reqwest::Method::DELETE,
            route: format!("/users/@me/mentions/{message}"),
            body: None,
        });
        let id = message.to_string();
        if let Some(Value::Array(list)) = self.fetched.get_mut(MENTIONS_KEY)
            && drop_mentions(list, &[id.as_str()])
        {
            cx.notify();
        }
    }

    fn fetch_mentions(&mut self) {
        let _ = self.requests.send(crate::net::Request::Fetch {
            key: MENTIONS_KEY.to_owned(),
            route: self.mention_filter.route(self.guild),
        });
    }

    /// `setGuildFilter`: the choice is kept and the list asked for again.
    fn set_mention_filter(&mut self, filter: MentionFilter, cx: &mut Context<Self>) {
        self.mention_filter = filter;
        filter.store();
        self.fetch_mentions();
        cx.notify();
    }

    /// The Mentions filter (`mentions-filter`, module 713024): checkbox rows under the Filter button, right-aligned to
    /// it 8px below. "Include all servers" only inside a server.
    pub(super) fn mentions_filter_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.mentions_filter_open || self.inbox != Some(InboxTab::Mentions) {
            return None;
        }
        let anchor = self.tip_anchors.borrow().get("inbox-filter").copied()?;
        let filter = self.mention_filter;
        let in_guild = self.guild.is_some() && self.current_dm().is_none();
        let row = |id: &'static str, label: &'static str, checked: bool, next: MentionFilter| {
            div()
                .id(id)
                .h(px(36.))
                .px(px(8.))
                .rounded(px(2.))
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(color("--interactive-background-hover"))
                        .rounded(px(4.))
                })
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .child(
                    div()
                        .size(px(20.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(if checked {
                            color("--checkbox-border-selected-default")
                        } else {
                            color("--checkbox-border-default")
                        })
                        .bg(if checked {
                            color("--checkbox-background-selected-default")
                        } else {
                            color("--checkbox-background-default")
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(checked, |this| {
                            this.child(icon("checkbox_check", 18., "--white"))
                        }),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_mention_filter(next, cx)))
        };
        let menu = div()
            .parity_with_border("mentions-filter", 1.)
            .id("mentions-filter")
            .occlude()
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                let on_button = this
                    .tip_anchors
                    .borrow()
                    .get("inbox-filter")
                    .is_some_and(|bounds| bounds.contains(&event.position));
                if !on_button
                    && this.press_outside(super::keyboard::Layer::MentionsFilter, cx)
                    && event.button != gpui::MouseButton::Right
                {
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_col()
            .child(row(
                "mentions-filter-everyone",
                "Include @everyone mentions",
                filter.everyone,
                MentionFilter {
                    everyone: !filter.everyone,
                    ..filter
                },
            ))
            .child(row(
                "mentions-filter-roles",
                "Include @role mentions",
                filter.roles,
                MentionFilter {
                    roles: !filter.roles,
                    ..filter
                },
            ))
            .when(in_guild, |this| {
                this.child(row(
                    "mentions-filter-servers",
                    "Include all servers",
                    filter.all_servers,
                    MentionFilter {
                        all_servers: !filter.all_servers,
                        ..filter
                    },
                ))
            });
        Some(
            deferred(
                gpui::anchored()
                    .anchor(gpui::Anchor::TopRight)
                    .position(gpui::point(anchor.right(), anchor.bottom() + px(8.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Saved messages still listed by the store (a removed one disappears at once), reminders or bookmarks.
    fn inbox_saved(&self, reminders: bool) -> Vec<(Message, Option<String>)> {
        self.fetched
            .get(super::saved::SAVED_KEY)
            .map(saved_entries)
            .unwrap_or_default()
            .into_iter()
            .filter(|(message, _)| {
                self.stores
                    .saved
                    .get(&(message.channel_id, message.id))
                    .is_some_and(|due| due.is_some() == reminders)
            })
            .collect()
    }

    /// `channelHeader_8ZcKCk`: the guild's 32px icon, a chevron, `#name` over "guild › category", and the buttons.
    pub(in crate::ui) fn inbox_channel_header(
        &self,
        message: &Message,
        buttons: Vec<AnyElement>,
    ) -> AnyElement {
        let guild: Option<&Guild> = self.stores.guilds.iter().find(|guild| {
            guild
                .channels
                .iter()
                .any(|channel| channel.id == message.channel_id)
        });
        let channel = guild.and_then(|guild| {
            guild
                .channels
                .iter()
                .find(|channel| channel.id == message.channel_id)
        });
        let category = guild.zip(channel).and_then(|(guild, channel)| {
            let parent = channel.parent_id?;
            guild
                .channels
                .iter()
                .find(|known| known.id == parent)
                .map(|known| known.name.clone())
        });
        let place = match (guild, category) {
            (Some(guild), Some(category)) => format!("{} \u{203a} {category}", guild.name),
            (Some(guild), None) => guild.name.clone(),
            _ => String::new(),
        };
        let name = channel
            .map(|channel| channel.name.clone())
            .unwrap_or_else(|| self.dm_name(message.channel_id));
        let guild_icon =
            guild.and_then(|guild| self.images.get(&format!("guild:{}", guild.id)).cloned());
        let acronym = guild.map(Guild::acronym).unwrap_or_default();
        div()
            .p(px(12.))
            .bg(color("--background-surface-higher"))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .size(px(32.))
                    .flex_none()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(color("--background-surface-high"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(color("--text-default"))
                    .when_some(guild_icon, |this, image| {
                        this.child(img(image).size(px(32.)))
                    })
                    .when(guild.is_some_and(|guild| guild.icon.is_none()), |this| {
                        this.child(acronym)
                    }),
            )
            .child(icon("inbox_chevron", 12., "--interactive-text-default"))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("inbox_channel", 12., "--channels-default"))
                            .child(
                                div()
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child(name),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child(place),
                    ),
            )
            .child(div().flex().gap(px(8.)).children(buttons))
            .into_any_element()
    }

    fn dm_name(&self, channel: Id) -> String {
        self.stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
            .and_then(|known| self.stores.dm_recipient(known))
            .map(|user| user.display_name().to_owned())
            .unwrap_or_default()
    }

    /// `singleMessage_0JZW4Z`: the message as the list draws a group's first line, 72px in, 12px above and below.
    fn inbox_message(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let content = self.content(message);
        self.inbox_message_with(message, content, cx)
    }

    /// The same, with the content drawn by the caller.
    pub(in crate::ui) fn inbox_message_with(
        &self,
        message: &Message,
        content: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let jump = message.clone();
        let avatar = self.images.get(&avatar_source(&message.author).0).cloned();
        let time = super::settings::prefs::clock(&message.local_time());
        div()
            .id(SharedString::from(format!("inbox-message-{}", message.id)))
            .relative()
            .my(px(12.))
            .pl(px(72.))
            .pr(px(16.))
            .py(px(2.))
            .cursor_pointer()
            // The message itself jumps to it, like the Jump button.
            .on_click(cx.listener(move |this, _, _, cx| {
                this.inbox = None;
                this.jump_to_message_checked(&jump, cx);
            }))
            .child(
                div()
                    .absolute()
                    .left(px(16.))
                    .top(px(4.))
                    .size(px(40.))
                    .rounded_full()
                    .children(avatar.map(|image| img(image).size(px(40.)).rounded_full())),
            )
            .child(
                div()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(message.author.display_name().to_owned()),
                    )
                    .child(
                        div()
                            .ml(px(8.))
                            .text_size(px(12.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(time),
                    ),
            )
            .child(content)
            .into_any_element()
    }

    pub(in crate::ui) fn inbox_button(
        &self,
        id: String,
        name: &'static str,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        self.inbox_button_shift(id, name, cx, move |this, _, cx| run(this, cx))
    }

    /// The same button, told whether Shift was held (Mark All as Read skips its question then, module 713024).
    fn inbox_button_shift(
        &self,
        id: String,
        name: &'static str,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, bool, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        div()
            .id(SharedString::from(id))
            .size(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .cursor_pointer()
            .hover(|style| style.bg(color("--control-secondary-background-hover")))
            .flex()
            .items_center()
            .justify_center()
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.click_at = window.mouse_position();
                    run(this, event.modifiers().shift, cx);
                    cx.notify();
                }),
            )
            .child(icon(name, 16., "--control-secondary-text-default"))
            .into_any_element()
    }

    /// `box_0JZW4Z` for a reminder: the due line, the channel with Snooze / Jump / Remove, then the message.
    fn reminder_card(&self, message: &Message, due: &str, cx: &mut Context<Self>) -> AnyElement {
        let (channel, id) = (message.channel_id, message.id);
        let jump = message.clone();
        let due_label = chrono::DateTime::parse_from_rfc3339(due)
            .map(|due| {
                let delta = due.timestamp_millis() - chrono::Utc::now().timestamp_millis();
                let span = super::saved::humanize(delta);
                if delta < 0 {
                    dc_intl::t!("Uq7Y+7", duration = span)
                } else {
                    dc_intl::t!("H4gnX9", duration = span)
                }
            })
            .unwrap_or_default();
        let overdue = chrono::DateTime::parse_from_rfc3339(due)
            .is_ok_and(|due| due.timestamp_millis() < chrono::Utc::now().timestamp_millis());
        let buttons = vec![
            self.inbox_button(
                format!("snooze-{id}"),
                "inbox_snooze",
                cx,
                move |this, _cx| {
                    // The edit menu (module 243656), labelled Snooze Reminder once the reminder is overdue.
                    this.snooze_menu = Some((channel, id, overdue, this.click_at));
                },
            ),
            self.inbox_button(format!("jump-{id}"), "inbox_jump", cx, move |this, cx| {
                this.inbox = None;
                this.jump_to_message_checked(&jump, cx);
            }),
            self.inbox_button(
                format!("remove-{id}"),
                "inbox_remove",
                cx,
                move |this, cx| {
                    this.complete_reminder(channel, id, cx);
                },
            ),
        ];
        div()
            .mb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .overflow_hidden()
            .child(
                div()
                    .p(px(12.))
                    .bg(color("--background-base-low"))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .when(overdue, |this| {
                        this.child(icon("inbox_overdue", 16., "--text-feedback-critical"))
                    })
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .child(due_label),
                    ),
            )
            .child(self.inbox_channel_header(message, buttons))
            .child(self.inbox_message(message, cx))
            .into_any_element()
    }

    /// A bookmark: the channel with Jump and Remove Bookmark, then the message.
    fn bookmark_card(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let (channel, id) = (message.channel_id, message.id);
        let jump = message.clone();
        let buttons = vec![
            self.inbox_button(format!("jump-{id}"), "inbox_jump", cx, move |this, cx| {
                this.inbox = None;
                this.jump_to_message_checked(&jump, cx);
            }),
            self.inbox_button(
                format!("unsave-{id}"),
                "inbox_remove",
                cx,
                move |this, cx| {
                    this.toggle_bookmark(channel, id, cx);
                },
            ),
        ];
        div()
            .mb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .overflow_hidden()
            .child(self.inbox_channel_header(message, buttons))
            .child(self.inbox_message(message, cx))
            .into_any_element()
    }

    fn inbox_empty(&self, title: &'static str, body: &'static str) -> AnyElement {
        div()
            .py(px(48.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-subtle"))
                    .child(body),
            )
            .into_any_element()
    }

    /// `container_FgrH4y`: the @ in an 80px circle with its stars, the title, and the green "Protip:".
    fn mentions_empty(&self) -> AnyElement {
        // `container_FgrH4y` starts at the page's top edge: no 16px above it.
        div()
            .mt(px(-16.))
            .h(px(360.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .mb(px(16.))
                    .w(px(80.))
                    .h(px(83.))
                    .child(
                        div()
                            .size(px(80.))
                            .rounded_full()
                            .bg(color("--background-base-low"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("inbox_at", 36., "--text-muted")),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(-10.))
                            .top(px(0.))
                            .child(img("icons/inbox_stars.svg").w(px(104.)).h(px(80.))),
                    ),
            )
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("You made it through everything!"),
            )
            .child(
                div()
                    .flex()
                    .items_end()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.67))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-feedback-positive"))
                            .child("Protip:"),
                    )
                    .child(
                        div()
                            .ml(px(3.5))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child(
                                "Whenever someone mentions you it will be saved here for 7 days.",
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `menu_jMz1Ee` / `menu_XzStbz`: the demo menu, rows 30 tall, the highlighted one on `--background-mod-subtle`.
    fn demo_menu(rows: [(&'static str, &'static str, bool); 2]) -> AnyElement {
        div()
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-highest"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .children(rows.into_iter().map(|(name, label, lit)| {
                div()
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(lit, |this| this.bg(color("--background-mod-subtle")))
                    .child(icon(
                        name,
                        16.,
                        if lit {
                            "--text-strong"
                        } else {
                            "--text-subtle"
                        },
                    ))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(label),
                    )
            }))
            .into_any_element()
    }

    /// The upsell column shared by Bookmarks and Scheduled: picture, title, the line with its bold action, demo.
    fn upsell(
        &self,
        picture: &str,
        title: &'static str,
        before: &'static str,
        action: &'static str,
        after: &'static str,
        demo: AnyElement,
    ) -> AnyElement {
        let image = self.images.get(picture).cloned();
        // The upsell pages keep no scrollbar gutter: 528 wide, 16px from both sides.
        div()
            .mr(px(-8.))
            .h(px(458.4))
            .pb(px(16.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .mb(px(24.))
                    .w(px(140.))
                    .h(px(93.33))
                    .children(image.map(|image| img(image).w(px(140.)).h(px(93.33)))),
            )
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(title),
            )
            .child(
                div()
                    .mb(px(16.))
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .text_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-subtle"))
                    .child(before)
                    .child(div().font_weight(FontWeight(600.)).child(action))
                    .child(after),
            )
            .child(div().w_full().child(demo))
            .into_any_element()
    }

    fn bookmarks_empty(&self) -> AnyElement {
        let wumpus = self.images.get("inbox:wumpus").cloned();
        let line = |text: &'static str| {
            div()
                .h(px(26.))
                .pl(px(72.))
                .py(px(2.))
                .line_height(px(22.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let demo = div()
            .relative()
            .h(px(150.))
            .child(
                div()
                    .mt(px(16.))
                    .py(px(8.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lowest"))
                    .child(
                        div()
                            .relative()
                            .h(px(48.))
                            .pl(px(72.))
                            .pt(px(2.))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(16.))
                                    .top(px(4.))
                                    .size(px(40.))
                                    .children(
                                        wumpus.map(|image| img(image).size(px(40.)).rounded_full()),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child("Wumpus"),
                                    )
                                    .child(
                                        div()
                                            .ml(px(8.))
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-muted"))
                                            .child(
                                                chrono::Local::now()
                                                    .format("%-I:%M %p")
                                                    .to_string(),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .line_height(px(22.))
                                    .text_color(color("--text-default"))
                                    .child("Concert tickets go on sale Friday at 10am!"),
                            ),
                    )
                    .child(line("wait, did you see this"))
                    .child(line("the setlist for tonight looks unreal")),
            )
            .child(
                div()
                    .absolute()
                    .left(px(301.))
                    .top(px(52.))
                    .child(Self::demo_menu([
                        ("inbox_bookmark", "Bookmark Message", true),
                        ("menu_create_reminder", "Create Reminder", false),
                    ])),
            )
            .into_any_element();
        self.upsell(
            "inbox:bookmarks",
            dc_intl::tr("Collect your favorite memories"),
            "Right-click any message and pick\u{a0}",
            dc_intl::tr("Bookmark Message"),
            ". Find it here, for your eyes only.",
            demo,
        )
    }

    fn scheduled_empty(&self) -> AnyElement {
        let channel = self
            .current_channel()
            .map(|channel| format!("Message #{}", channel.name))
            .unwrap_or_else(|| "Message".to_owned());
        let demo = div()
            .mt(px(16.))
            .h(px(107.))
            .child(
                div()
                    .ml(px(12.))
                    .mb(px(-5.))
                    .w(px(161.44))
                    .child(Self::demo_menu([
                        ("inbox_upload", "Upload a File", false),
                        ("inbox_schedule", "Schedule Message", true),
                    ])),
            )
            .child(
                div()
                    .h(px(42.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(24.))
                            .rounded_full()
                            .bg(color("--background-mod-strong"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("inbox_plus", 16., "--text-strong")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(channel),
                    ),
            )
            .into_any_element();
        self.upsell(
            "inbox:scheduled",
            dc_intl::tr("Send it at the perfect moment"),
            "Draft your message then click the plus button and pick\u{a0}",
            dc_intl::tr("Schedule Message"),
            "\u{a0}to choose when to send. We'll handle the rest.",
            demo,
        )
    }

    /// `tutorial_TqjWYy`: the Unreads introduction with its Got it! button.
    fn unreads_tutorial(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mt(px(16.))
            .mb(px(16.))
            .p(px(16.))
            .rounded(px(8.))
            .bg(color("--background-base-lowest"))
            .flex()
            .gap(px(16.))
            .child(
                div()
                    .size(px(40.))
                    .flex_none()
                    .rounded_full()
                    .bg(color("--background-base-low"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("inbox_tutorial", 24., "--text-subtle")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Stay up to date with latest messages!"),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child("Unread messages from all your unmuted channels will show up here. Time to get to that zero inbox!"),
                    )
                    .child(
                        div().mt(px(8.)).flex().child(
                            div()
                                .id("inbox-tutorial-got-it")
                                .h(px(32.))
                                .px(px(11.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--control-primary-border-default"))
                                .bg(color("--control-primary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--white"))
                                .on_click(cx.listener(|_, _, _, cx| {
                                    crate::storage::set(TUTORIAL_SEEN, serde_json::json!(true));
                                    cx.notify();
                                }))
                                .child(dc_intl::tr("Got it!")),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// A message of an unread card: a join line with the green arrow, or the message as the list draws it.
    fn unread_message(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        if message.kind == USER_JOIN {
            let (before, after) = Self::join_line(message);
            let arrow = self.images.get("inbox:join").cloned();
            return div()
                .relative()
                .mt(px(17.))
                .pl(px(72.))
                .py(px(2.))
                .child(
                    div()
                        .absolute()
                        .left(px(28.))
                        .top(px(7.))
                        .size(px(16.))
                        .children(arrow.map(|image| img(image).size(px(16.)))),
                )
                .child(
                    div()
                        .py(px(2.))
                        .flex()
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
                                .text_color(color("--text-muted"))
                                .child(super::header_time(
                                    message.local_time(),
                                    chrono::Local::now(),
                                )),
                        ),
                )
                .into_any_element();
        }
        self.inbox_message(message, cx)
    }

    /// One unread channel: the header with Notification Settings, Mark As Read and Collapse, then its new messages
    /// under their date line.
    fn unread_card(&self, guild: Id, channel: Id, cx: &mut Context<Self>) -> Option<AnyElement> {
        let joined = self.stores.guild(guild).map_or(0, Guild::joined_snowflake);
        let acked = self.stores.read_states.acked(channel, joined);
        let mut messages: Vec<Message> = self
            .fetched
            .get(&format!("{UNREAD_PREFIX}{channel}"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Message::from_value)
            .filter(|message| message.id > acked)
            .collect();
        messages.sort_by_key(|message| message.id);
        let first = messages.first()?.clone();
        let last = messages
            .last()
            .map(|message| message.id)
            .unwrap_or(first.id);
        let collapsed = self.inbox_collapsed.contains(&channel);
        let buttons = vec![
            self.inbox_button(
                format!("unread-bell-{channel}"),
                "inbox_bell",
                cx,
                move |this, _| {
                    this.inbox = None;
                    this.select(guild, Some(channel));
                    this.notify_open = true;
                },
            ),
            self.inbox_button(
                format!("unread-read-{channel}"),
                "inbox_mark_read",
                cx,
                move |this, _| {
                    this.ack_channel(channel, last);
                },
            ),
            div()
                .id(SharedString::from(format!("unread-collapse-{channel}")))
                .size(px(24.))
                .p(px(4.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.inbox_collapsed.remove(&channel) {
                        this.inbox_collapsed.insert(channel);
                    }
                    cx.notify();
                }))
                .child(
                    gpui::svg()
                        .path("icons/inbox_collapse.svg")
                        .size(px(16.))
                        .text_color(color("--text-subtle"))
                        // Folded, the chevron turns a quarter to the left.
                        // `chevron_UpFw5t`: `transform .2s`.
                        .with_transformation(super::motion::rotation(
                            super::motion::key(&("inbox-chevron", channel)),
                            if collapsed {
                                -std::f32::consts::FRAC_PI_2
                            } else {
                                0.
                            },
                            super::motion::css(200., super::motion::Ease::Ease),
                        )),
                )
                .into_any_element(),
        ];
        let date = first.local_time().format("%B %-d, %Y").to_string();
        Some(
            div()
                .mb(px(16.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .overflow_hidden()
                .child(self.inbox_channel_header(&first, buttons))
                .when(!collapsed, |this| {
                    this.child(
                        div()
                            .pr(px(16.))
                            .pb(px(16.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .child(
                                div()
                                    .relative()
                                    .ml(px(16.))
                                    .mt(px(16.))
                                    .h(px(1.))
                                    .bg(color("--border-subtle"))
                                    .flex()
                                    .justify_center()
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(-8.))
                                            .px(px(4.))
                                            .py(px(2.))
                                            .rounded(px(8.))
                                            .bg(color("--background-surface-high"))
                                            .text_size(px(12.))
                                            .line_height(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(date),
                                    ),
                            )
                            .children(
                                messages
                                    .iter()
                                    .map(|message| self.unread_message(message, cx)),
                            ),
                    )
                })
                .into_any_element(),
        )
    }

    /// Mark As Read on a card: ack its newest message, and drop the card.
    pub(super) fn ack_channel(&mut self, channel: Id, message: Id) {
        self.stores.read_states.ack_local(channel, message, false);
        self.data_revision += 1;
        self.inbox_read.insert(channel);
        let _ = self.requests.send(crate::net::Request::Ack {
            channel,
            message,
            manual: false,
            mention_count: 0,
            last_viewed: dc_state::read_state::days_since_epoch(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as u64)
                    .unwrap_or_default(),
            ),
        });
    }

    /// Mark All as Read (module 713024 `tI`): asks first, unless Shift is held; then every card's channel is acked
    /// at once through the bulk route (module 182179 `K`: `P.Uq`).
    fn ask_ack_all_unreads(&mut self, shift: bool, cx: &mut Context<Self>) {
        let listed = self.unread_reads();
        if listed.is_empty() {
            return;
        }
        if shift {
            self.ack_all_unreads(cx);
        } else {
            self.ask_destructive(super::destructive::Pending::mark_all_read(listed), cx);
        }
    }

    fn ack_all_unreads(&mut self, cx: &mut Context<Self>) {
        let listed = self.unread_reads();
        for (channel, _) in &listed {
            self.inbox_read.insert(*channel);
        }
        self.mark_read_bulk(&listed, cx);
    }

    /// The `(channel, newest message)` of every card of the Unreads tab.
    fn unread_reads(&self) -> Vec<(Id, Id)> {
        self.unread_channels()
            .into_iter()
            .filter_map(|(_, channel)| {
                let newest = self
                    .fetched
                    .get(&format!("{UNREAD_PREFIX}{channel}"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Message::from_value)
                    .map(|message| message.id)
                    .max()?;
                let known = self.stores.read_states.latest.get(&channel).copied();
                Some((channel, known.map_or(newest, |known| known.max(newest))))
            })
            .collect()
    }

    fn inbox_page(&self, tab: InboxTab, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match tab {
            InboxTab::Reminders => {
                let mut list = self.inbox_saved(true);
                list.sort_by(|a, b| a.1.cmp(&b.1));
                if list.is_empty() {
                    return vec![self.inbox_empty(
                        "No reminders",
                        "Right-click any message and pick Remind Me to get a nudge later.",
                    )];
                }
                list.iter()
                    .map(|(message, due)| {
                        self.reminder_card(message, due.as_deref().unwrap_or_default(), cx)
                    })
                    .collect()
            }
            InboxTab::Bookmarks => {
                let list = self.inbox_saved(false);
                if list.is_empty() {
                    return vec![self.bookmarks_empty()];
                }
                list.iter()
                    .map(|(message, _)| self.bookmark_card(message, cx))
                    .collect()
            }
            InboxTab::Mentions => {
                let list: Vec<Message> = self
                    .fetched
                    .get(MENTIONS_KEY)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Message::from_value)
                    .collect();
                if list.is_empty() {
                    return vec![self.mentions_empty()];
                }
                list.iter()
                    .map(|message| {
                        let jump = message.clone();
                        let id = message.id;
                        // Module 713024: Jump to Message, then Mark As Read (`deleteRecentMention`).
                        let buttons = vec![
                            self.inbox_button(
                                format!("mention-jump-{}", message.id),
                                "inbox_jump",
                                cx,
                                move |this, cx| {
                                    this.inbox = None;
                                    this.jump_to_message_checked(&jump, cx);
                                },
                            ),
                            self.inbox_button(
                                format!("mention-read-{}", message.id),
                                "inbox_mark_read",
                                cx,
                                move |this, cx| this.delete_recent_mention(id, cx),
                            ),
                        ];
                        div()
                            .mb(px(16.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .overflow_hidden()
                            .child(self.inbox_channel_header(message, buttons))
                            .child(self.inbox_message(message, cx))
                            .into_any_element()
                    })
                    .collect()
            }
            InboxTab::Scheduled => {
                let mut rows: Vec<_> = self
                    .stores
                    .scheduled_messages
                    .get_scheduled_messages_for_inbox()
                    .values()
                    .collect();
                if rows.is_empty() {
                    vec![self.scheduled_empty()]
                } else {
                    rows.sort_by(|a, b| a.send_at_timestamp.cmp(&b.send_at_timestamp));
                    rows.into_iter()
                        .map(|item| {
                            let id = item.scheduled_message_id;
                            let text = item.create_args.content.clone();
                            let when = item.send_at_timestamp.clone();
                            let when_edit = when.clone();
                            div()
                                .id(SharedString::from(format!("scheduled-row-{id}")))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-higher"))
                                .p(px(12.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.))
                                                .child(icon("webhook_clock", 14., "--icon-subtle"))
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-muted"))
                                                        .child(when),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.))
                                                .child(
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "scheduled-send-now-{id}"
                                                        )))
                                                        .px(px(8.))
                                                        .py(px(4.))
                                                        .rounded(px(6.))
                                                        .bg(color(
                                                            "--control-primary-background-default",
                                                        ))
                                                        .cursor_pointer()
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--white"))
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.send_now_scheduled_message(id, cx);
                                                        }))
                                                        .child(dc_intl::t("JLba51")),
                                                )
                                                .child(
                                                    // Module 713024 `s1` -> module 970244 `CI` -> module 23395 Edit schedule time.
                                                    self.with_tip(
                                                        format!("scheduled-edit-time-tip-{id}"),
                                                        super::tooltip::Tip::text(
                                                            dc_intl::t("SBcdAN"),
                                                            super::tooltip::Side::Top,
                                                        ),
                                                        div()
                                                            .id(SharedString::from(format!(
                                                                "scheduled-edit-time-{id}"
                                                            )))
                                                            .px(px(8.))
                                                            .py(px(4.))
                                                            .rounded(px(6.))
                                                            .bg(color(
                                                                "--control-secondary-background-default",
                                                            ))
                                                            .hover(|s| {
                                                                s.bg(color(
                                                                    "--control-secondary-background-hover",
                                                                ))
                                                            })
                                                            .cursor_pointer()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(4.))
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight(500.))
                                                            .text_color(color(
                                                                "--control-secondary-text-default",
                                                            ))
                                                            .child(icon(
                                                                "webhook_clock",
                                                                14.,
                                                                "--control-secondary-text-default",
                                                            ))
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.open_update_scheduled_modal(
                                                                        id,
                                                                        &when_edit,
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                        cx,
                                                    ),
                                                )
                                                .child(
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "scheduled-cancel-{id}"
                                                        )))
                                                        .px(px(8.))
                                                        .py(px(4.))
                                                        .rounded(px(6.))
                                                        .bg(color(
                                                            "--control-secondary-background-default",
                                                        ))
                                                        .cursor_pointer()
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color(
                                                            "--control-secondary-text-default",
                                                        ))
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.cancel_scheduled_message(id, cx);
                                                        }))
                                                        .child(dc_intl::t("ETE/oC")),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(text),
                                )
                                .into_any_element()
                        })
                        .collect()
                }
            }
            InboxTab::Keywords => self.keyword_notify_page_cards(cx),
            InboxTab::Unreads => {
                let mut list = Vec::new();
                if crate::storage::get(TUTORIAL_SEEN).is_none() {
                    list.push(self.unreads_tutorial(cx));
                }
                list.extend(
                    self.unread_channels()
                        .into_iter()
                        .filter_map(|(guild, channel)| self.unread_card(guild, channel, cx)),
                );
                list
            }
        }
    }

    pub(super) fn inbox_popout(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tab = self.inbox?;
        let anchor = self.tip_anchors.borrow().get("bar-inbox").copied()?;
        let viewport = window.viewport_size();
        // The popout's right edge is the button's, 4px under the title bar.
        let right = f32::from(viewport.width) - f32::from(anchor.right());
        let reminders = self
            .stores
            .saved
            .values()
            .filter(|due| due.is_some())
            .count();
        let tabs: Vec<AnyElement> = [
            (InboxTab::Unreads, dc_intl::tr("Unreads").to_owned()),
            (InboxTab::Mentions, dc_intl::tr("Mentions").to_owned()),
            (InboxTab::Bookmarks, dc_intl::tr("Bookmarks").to_owned()),
            (
                InboxTab::Reminders,
                if reminders > 0 {
                    format!("Reminders ({reminders})")
                } else {
                    dc_intl::tr("Reminders").to_owned()
                },
            ),
            (InboxTab::Scheduled, dc_intl::tr("Scheduled").to_owned()),
        ]
        .into_iter()
        // Not in the pinned client: KeywordNotify's tab after Scheduled (`plugin_hooks/keyword_notify.rs`).
        .chain(self.keyword_notify_tab().then(|| {
            (
                InboxTab::Keywords,
                super::plugin_hooks::KEYWORD_TAB_LABEL.to_owned(),
            )
        }))
        .map(|(which, label)| {
            let selected = which == tab;
            div()
                .id(SharedString::from(format!("inbox-tab-{which:?}")))
                .relative()
                .flex_1()
                // 35.6 in the capture with the bar's 1px rule inside it (the tabs sit 1px over it).
                .h(px(34.6))
                .px(px(8.))
                .pb(px(16.))
                .cursor_pointer()
                .flex()
                .justify_center()
                .text_size(px(14.))
                .line_height(px(19.6))
                .font_weight(FontWeight(600.))
                .text_color(if selected {
                    color("--text-brand")
                } else {
                    color("--text-subtle")
                })
                .on_click(cx.listener(move |this, _, _, cx| this.set_inbox_tab(which, cx)))
                .child(label)
                .when(selected, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(px(-1.))
                            .h(px(2.))
                            .rounded(px(2.))
                            .bg(color("--text-brand")),
                    )
                })
                .into_any_element()
        })
        .collect();
        let page = self.inbox_page(tab, cx);
        let max_h = MAX_HEIGHT.min((self.viewport.1 - 52.).max(240.));
        let popout = div()
            .parity("inbox")
            .id("inbox")
            .occlude()
            .w(px(WIDTH.min((self.viewport.0 - 16.).max(300.))))
            .max_h(px(max_h))
            .rounded(px(8.))
            .overflow_hidden()
            // `container_uTMZTY`: `--background-surface-high`, and its edge is `--shadow-border`, a 1px ring outside
            // the 560px box, with `--shadow-high` under it.
            .bg(color("--background-surface-high"))
            .shadow(vec![super::shadow_border(), super::shadow_high()])
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                if this.press_outside(super::keyboard::Layer::Inbox, cx)
                    && event.button != gpui::MouseButton::Right
                {
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .pt(px(16.))
                    .child(
                        div()
                            .mx(px(16.))
                            .h(px(34.))
                            .flex()
                            .items_center()
                            .child(icon("inbox_title", 20., "--text-strong"))
                            .child(
                                div()
                                    .ml(px(8.))
                                    .flex_1()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::tr("Inbox")),
                            )
                            .when(tab == InboxTab::Mentions, |this| {
                                this.child(
                                    div()
                                        .mr(px(8.))
                                        .relative()
                                        .child(self.anchor_probe("inbox-filter"))
                                        .child(self.inbox_button(
                                            "inbox-filter".to_owned(),
                                            "inbox_filter",
                                            cx,
                                            |this, _| {
                                                this.mentions_filter_open =
                                                    !this.mentions_filter_open
                                            },
                                        )),
                                )
                            })
                            .when(tab == InboxTab::Keywords, |this| {
                                this.child(self.keyword_notify_clear_all(cx))
                            })
                            .when(tab == InboxTab::Unreads, |this| {
                                this.child(div().mr(px(8.)).child(self.inbox_button_shift(
                                    "inbox-mark-all".to_owned(),
                                    "inbox_mark_all",
                                    cx,
                                    |this, shift, cx| this.ask_ack_all_unreads(shift, cx),
                                )))
                            })
                            .child(
                                // `friendRequestsButton_QwUPp6`: Friends, where requests wait.
                                div()
                                    .id("inbox-friend-requests")
                                    .size(px(34.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--control-secondary-border-default"))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.inbox = None;
                                        this.select_home(None);
                                        this.set_friends_tab(
                                            super::friends::FriendsTab::Pending,
                                            window,
                                            cx,
                                        );
                                    }))
                                    .child(icon(
                                        "inbox_friend_requests",
                                        16.,
                                        "--control-secondary-text-default",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(16.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .children(tabs),
                    ),
            )
            .child(
                div()
                    .id("inbox-page")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    // `scrollbarGutterStable`: 8px kept for the thin scrollbar on the right.
                    .pl(px(16.))
                    .pr(px(24.))
                    .pt(px(16.))
                    .children(page),
            );
        Some(
            deferred(div().absolute().top(px(36.)).right(px(right)).child(popout))
                .with_priority(2)
                .into_any_element(),
        )
    }
}

impl Root {
    /// `SavedMessagesStore.hasOverdueReminder` (module 85109 / 938483): whether any saved reminder is past due.
    pub(super) fn has_overdue_reminder(&self) -> bool {
        has_overdue_reminder_at(self.stores.saved.values(), chrono::Utc::now())
    }

    /// The snooze menu under the card's button: its label, the times of the reminder menu and Custom...
    pub(super) fn snooze_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (channel, message, overdue, at) = self.snooze_menu?;
        let now = chrono::Local::now();
        let item = |id: SharedString, text: &'static str| {
            div()
                .id(id)
                .h(px(34.))
                .p(px(8.))
                .rounded(px(2.))
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(color("--interactive-background-hover"))
                        .rounded(px(4.))
                })
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let mut rows: Vec<AnyElement> = vec![
            div()
                .min_h(px(32.))
                .p(px(8.))
                .text_size(px(14.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(if overdue {
                    dc_intl::tr("Snooze Reminder")
                } else {
                    dc_intl::tr("Remind Me")
                })
                .into_any_element(),
        ];
        for (index, (text, time)) in super::saved::reminder_choices(now).into_iter().enumerate() {
            rows.push(
                item(SharedString::from(format!("snooze-choice-{index}")), text)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.snooze_menu = None;
                        this.set_reminder(channel, message, time, cx);
                    }))
                    .into_any_element(),
            );
        }
        rows.push(
            item("snooze-custom".into(), dc_intl::tr("Custom..."))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.snooze_menu = None;
                    this.custom_reminder =
                        Some(super::reminder_modal::CustomReminder::new(channel, message));
                    cx.notify();
                }))
                .into_any_element(),
        );
        let layer = div()
            .id("snooze-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                div()
                    .id("snooze-menu")
                    .occlude()
                    .absolute()
                    .left(px((f32::from(at.x) - 180.).max(8.)))
                    .top(px(f32::from(at.y) + 16.))
                    .w(px(222.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .flex()
                    .flex_col()
                    .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        this.snooze_menu = None;
                        cx.notify();
                        if event.button != gpui::MouseButton::Right {
                            cx.stop_propagation();
                        }
                    }))
                    .children(rows),
            );
        Some(gpui::deferred(layer).with_priority(3).into_any_element())
    }
}

fn has_overdue_reminder_at<'a>(
    saved: impl IntoIterator<Item = &'a Option<String>>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    saved.into_iter().flatten().any(|due| {
        chrono::DateTime::parse_from_rfc3339(due)
            .is_ok_and(|parsed| parsed.with_timezone(&chrono::Utc) <= now)
    })
}

/// Removes the listed message ids from a mentions answer; true when anything went.
fn drop_mentions(list: &mut Vec<Value>, ids: &[&str]) -> bool {
    let before = list.len();
    list.retain(|message| message["id"].as_str().is_none_or(|id| !ids.contains(&id)));
    list.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_messages_leave_the_mentions_list() {
        let mut list = vec![
            serde_json::json!({"id": "1"}),
            serde_json::json!({"id": "2"}),
            serde_json::json!({"id": "3"}),
        ];
        assert!(drop_mentions(&mut list, &["2"]));
        assert!(!drop_mentions(&mut list, &["9"]));
        assert!(drop_mentions(&mut list, &["1", "3"]));
        assert!(list.is_empty());
    }

    #[test]
    fn the_mentions_query_follows_the_filter() {
        let all = MentionFilter::default();
        assert_eq!(
            all.route(Some(5)),
            "/users/@me/mentions?limit=25&roles=true&everyone=true"
        );
        let narrow = MentionFilter {
            everyone: false,
            roles: true,
            all_servers: false,
        };
        assert_eq!(
            narrow.route(Some(5)),
            "/users/@me/mentions?limit=25&roles=true&everyone=false&guild_id=5"
        );
        assert_eq!(
            narrow.route(None),
            "/users/@me/mentions?limit=25&roles=true&everyone=false"
        );
    }
    use serde_json::json;

    #[test]
    fn saved_entries_carry_their_message_and_due_time() {
        let body = json!({"results": [{
            "save_data": {"channel_id": "1", "message_id": "2", "due_at": "2026-11-08T03:00:00+00:00"},
            "message": {"id": "2", "channel_id": "1", "content": "hi", "timestamp": "2026-11-08T01:00:00+00:00",
                "author": {"id": "3", "username": "a"}}
        }]});
        let entries = saved_entries(&body);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.as_deref(), Some("2026-11-08T03:00:00+00:00"));
    }

    #[test]
    fn overdue_reminder_badge_tracks_past_due_reminders() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-14T02:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let bookmarks_and_future = [None, Some("2026-11-14T03:00:00+00:00".to_owned())];
        assert!(!has_overdue_reminder_at(&bookmarks_and_future, now));
        let past_due = [None, Some("2026-11-14T01:59:59+00:00".to_owned())];
        assert!(has_overdue_reminder_at(&past_due, now));
    }
}
