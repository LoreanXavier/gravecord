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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum InboxTab {
    #[default]
    Unreads,
    Mentions,
    Bookmarks,
    Reminders,
    Scheduled,
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
    pub(super) fn toggle_inbox(&mut self, cx: &mut Context<Self>) {
        if self.inbox.take().is_none() {
            self.inbox = Some(InboxTab::default());
            self.request_saved();
            self.request_image("inbox:bookmarks".to_owned(), BOOKMARKS_PICTURE.to_owned());
            self.request_image("inbox:scheduled".to_owned(), SCHEDULED_PICTURE.to_owned());
            self.request_image("inbox:wumpus".to_owned(), WUMPUS_AVATAR.to_owned());
        }
        cx.notify();
    }

    fn set_inbox_tab(&mut self, tab: InboxTab, cx: &mut Context<Self>) {
        self.inbox = Some(tab);
        if tab == InboxTab::Mentions {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: MENTIONS_KEY.to_owned(),
                route: "/users/@me/mentions?limit=25&roles=true&everyone=true".to_owned(),
            });
        }
        cx.notify();
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
    fn inbox_channel_header(&self, message: &Message, buttons: Vec<AnyElement>) -> AnyElement {
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
    fn inbox_message(&self, message: &Message) -> AnyElement {
        let avatar = self.images.get(&avatar_source(&message.author).0).cloned();
        let time = message.local_time().format("%-I:%M %p").to_string();
        div()
            .relative()
            .my(px(12.))
            .pl(px(72.))
            .pr(px(16.))
            .py(px(2.))
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
            .child(self.content(message))
            .into_any_element()
    }

    fn inbox_button(
        &self,
        id: String,
        name: &'static str,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
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
            .on_click(cx.listener(move |this, _, _, cx| {
                run(this, cx);
                cx.notify();
            }))
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
                    format!("Due {span} ago")
                } else {
                    format!("Due in {span}")
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
                move |this, cx| {
                    // Snooze moves the reminder an hour on, the first of the snooze choices.
                    this.set_reminder(
                        channel,
                        id,
                        chrono::Local::now() + chrono::Duration::hours(1),
                        cx,
                    );
                },
            ),
            self.inbox_button(format!("jump-{id}"), "inbox_jump", cx, move |this, cx| {
                this.inbox = None;
                this.jump_to_message(&jump, cx);
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
            .child(self.inbox_message(message))
            .into_any_element()
    }

    /// A bookmark: the channel with Jump and Remove Bookmark, then the message.
    fn bookmark_card(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let (channel, id) = (message.channel_id, message.id);
        let jump = message.clone();
        let buttons = vec![
            self.inbox_button(format!("jump-{id}"), "inbox_jump", cx, move |this, cx| {
                this.inbox = None;
                this.jump_to_message(&jump, cx);
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
            .child(self.inbox_message(message))
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
            "Collect your favorite memories",
            "Right-click any message and pick\u{a0}",
            "Bookmark Message",
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
            "Send it at the perfect moment",
            "Draft your message then click the plus button and pick\u{a0}",
            "Schedule Message",
            "\u{a0}to choose when to send. We'll handle the rest.",
            demo,
        )
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
                        let buttons = vec![self.inbox_button(
                            format!("mention-jump-{}", message.id),
                            "inbox_jump",
                            cx,
                            move |this, cx| {
                                this.inbox = None;
                                this.jump_to_message(&jump, cx);
                            },
                        )];
                        div()
                            .mb(px(16.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .overflow_hidden()
                            .child(self.inbox_channel_header(message, buttons))
                            .child(self.inbox_message(message))
                            .into_any_element()
                    })
                    .collect()
            }
            InboxTab::Scheduled => vec![self.scheduled_empty()],
            InboxTab::Unreads => vec![self.inbox_empty(
                "Stay up to date with latest messages!",
                "Unread messages from all your unmuted channels will show up here.",
            )],
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
            (InboxTab::Unreads, "Unreads".to_owned()),
            (InboxTab::Mentions, "Mentions".to_owned()),
            (InboxTab::Bookmarks, "Bookmarks".to_owned()),
            (
                InboxTab::Reminders,
                if reminders > 0 {
                    format!("Reminders ({reminders})")
                } else {
                    "Reminders".to_owned()
                },
            ),
            (InboxTab::Scheduled, "Scheduled".to_owned()),
        ]
        .into_iter()
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
        let popout = div()
            .parity("inbox")
            .id("inbox")
            .occlude()
            .w(px(WIDTH))
            .max_h(px(MAX_HEIGHT))
            .rounded(px(8.))
            .bg(color("--background-surface-higher"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.inbox = None;
                cx.notify();
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
                                    .child("Inbox"),
                            )
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
