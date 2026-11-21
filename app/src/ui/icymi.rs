//! ICYMI ("In Case You Missed It") feed page (module 411831 + 4106).
//!
//! Full-page feed view (`BVt.ICYMI`):
//! - Header: `jnXV/V` ("In Case You Missed It"), `Ac2OZA` ("ALPHA") badge, `wzzjk9` ("Refresh").
//! - Sticky `"New content available"` pill button.
//! - Feed cards with headers (`8P08G9` Announcement, `hMFMY9` Message, `6pFsLQ` Event scheduled,
//!   `bYNuVx` Post, `fxOLPR` updated their status, `ktOTRQ` Recently Played, `CHUAYk` in).
//! - Bodies: `/50eHi` ("Most played"), `TxqPQR` ("Active Now"), `+DLsD8` ("{count} people are interested").
//! - REST (`4106`): `GET /users/@me/gravity-icymi` & `POST /gravity-content`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgb, rgba};
use serde_json::{Value, json};

pub(super) const ICYMI_FEED_KEY: &str = "icymi:feed";
pub(super) const ICYMI_HYDRATE_KEY: &str = "icymi:hydrate";

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum IcymiCardKind {
    Announcement {
        guild_name: String,
        channel_name: String,
        excerpt: String,
    },
    Message {
        guild_name: String,
        channel_name: String,
        author: String,
        excerpt: String,
    },
    GuildEvent {
        guild_name: String,
        channel_name: String,
        event_title: String,
        active_now: bool,
        start_time: String,
        interested_count: u64,
    },
    ForumPost {
        guild_name: String,
        channel_name: String,
        post_title: String,
    },
    CustomStatus {
        user_name: String,
        status_text: String,
    },
    PlayedGame {
        user_name: String,
        game_name: String,
        most_played: bool,
    },
}

#[derive(Clone, Debug)]
pub struct IcymiItem {
    pub id: String,
    pub timestamp: String,
    pub kind: IcymiCardKind,
}

#[derive(Clone, Debug, Default)]
pub(super) struct IcymiState {
    pub open: bool,
    pub loading: bool,
    pub refreshing: bool,
    pub has_new_content: bool,
    pub items: Vec<IcymiItem>,
}

/// Builds `GET /users/@me/gravity-icymi` (`4106`).
pub fn fetch_feed_request(refresh: bool) -> Request {
    let route = if refresh {
        "/users/@me/gravity-icymi?refresh=true".to_owned()
    } else {
        "/users/@me/gravity-icymi".to_owned()
    };
    Request::Fetch {
        key: ICYMI_FEED_KEY.to_owned(),
        route,
    }
}

/// Builds `POST /gravity-content` batch hydration request (`4106`).
#[allow(dead_code)]
pub fn hydrate_content_request(
    message_items: &[(String, String)],
    activity_items: &[(String, String)],
) -> Request {
    let msgs: Vec<Value> = message_items
        .iter()
        .map(|(cid, mid)| json!({ "channel_id": cid, "message_id": mid }))
        .collect();
    let acts: Vec<Value> = activity_items
        .iter()
        .map(|(uid, cid)| json!({ "user_id": uid, "content_id": cid }))
        .collect();
    Request::Rest {
        key: Some(ICYMI_HYDRATE_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: "/gravity-content".to_owned(),
        body: Some(json!({
            "message_items": msgs,
            "activity_items": acts,
        })),
    }
}

#[cfg(feature = "capture")]
fn demo_icymi_items() -> Vec<IcymiItem> {
    vec![
        IcymiItem {
            id: "1".into(),
            timestamp: "12m ago".into(),
            kind: IcymiCardKind::GuildEvent {
                guild_name: "gravecord community".into(),
                channel_name: "stage-lounge".into(),
                event_title: "Friday Night Community Watch Party".into(),
                active_now: true,
                start_time: "Now".into(),
                interested_count: 42,
            },
        },
        IcymiItem {
            id: "2".into(),
            timestamp: "34m ago".into(),
            kind: IcymiCardKind::PlayedGame {
                user_name: "plü".into(),
                game_name: "Valorant".into(),
                most_played: true,
            },
        },
        IcymiItem {
            id: "3".into(),
            timestamp: "1h ago".into(),
            kind: IcymiCardKind::Announcement {
                guild_name: "gravecord community".into(),
                channel_name: "announcements".into(),
                excerpt: "Release notes and parity updates are now live in the changelog.".into(),
            },
        },
        IcymiItem {
            id: "4".into(),
            timestamp: "2h ago".into(),
            kind: IcymiCardKind::CustomStatus {
                user_name: "lawleis".into(),
                status_text: "Grinding ranked all weekend".into(),
            },
        },
    ]
}

impl Root {
    /// Opens the ICYMI page and requests `GET /users/@me/gravity-icymi`.
    #[allow(dead_code)]
    pub(super) fn open_icymi(&mut self, cx: &mut Context<Self>) {
        self.icymi.open = true;
        self.icymi.loading = self.icymi.items.is_empty();
        self.icymi.refreshing = true;
        let _ = self.requests.send(fetch_feed_request(false));
        cx.notify();
    }

    /// Handles `GET /users/@me/gravity-icymi` & `POST /gravity-content` responses.
    pub(super) fn handle_icymi_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        if key == ICYMI_FEED_KEY || key == ICYMI_HYDRATE_KEY {
            self.icymi.loading = false;
            self.icymi.refreshing = false;
            if let Some(arr) = body["items"].as_array()
                && !arr.is_empty()
            {
                let mut parsed = Vec::new();
                for (i, raw) in arr.iter().enumerate() {
                    let id = raw["id"].as_str().unwrap_or("0").to_owned();
                    let ts = raw["timestamp"].as_str().unwrap_or("Just now").to_owned();
                    let excerpt = raw["content"].as_str().unwrap_or("New activity").to_owned();
                    parsed.push(IcymiItem {
                        id: if id == "0" { format!("item-{i}") } else { id },
                        timestamp: ts,
                        kind: IcymiCardKind::Message {
                            guild_name: raw["guild_name"].as_str().unwrap_or("Server").to_owned(),
                            channel_name: raw["channel_name"]
                                .as_str()
                                .unwrap_or("general")
                                .to_owned(),
                            author: raw["author"].as_str().unwrap_or("Member").to_owned(),
                            excerpt,
                        },
                    });
                }
                self.icymi.items = parsed;
            }
            cx.notify();
        }
    }

    /// Renders module `411831` ICYMI ("In Case You Missed It") overlay/page.
    pub(super) fn icymi_overlay_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.icymi.open {
            return None;
        }
        let refreshing = self.icymi.refreshing;
        let in_word = dc_intl::t("CHUAYk");

        let card = div()
            .id("icymi-page-modal")
            .w(px(640.))
            .max_h(px(620.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            // Header (`jnXV/V` "In Case You Missed It" + `Ac2OZA` "ALPHA" + `wzzjk9` "Refresh")
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("discover_nav_quests", 18., "--icon-strong"))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("jnXV/V")),
                            )
                            .child(
                                div()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(rgb(0x5865f2))
                                    .text_size(px(10.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("Ac2OZA")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("icymi-refresh-btn")
                                    .aria_label(dc_intl::t("wzzjk9"))
                                    .h(px(30.))
                                    .px(px(10.))
                                    .rounded(px(6.))
                                    .bg(color("--control-secondary-background-default"))
                                    .when(!refreshing, |s| s.cursor_pointer())
                                    .when(refreshing, |s| s.opacity(0.5))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(12.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !refreshing {
                                            this.icymi.refreshing = true;
                                            this.icymi.has_new_content = false;
                                            let _ = this.requests.send(fetch_feed_request(true));
                                            cx.notify();
                                        }
                                    }))
                                    .child(icon("clips_retry", 14., "--icon-subtle"))
                                    .child(dc_intl::t("wzzjk9")),
                            )
                            .child(
                                div()
                                    .id("icymi-close-btn")
                                    .size(px(28.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.icymi.open = false;
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    ),
            )
            // Feed scroll container
            .child(
                div()
                    .id("icymi-feed-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    // Sticky `"New content available"` pill
                    .when(self.icymi.has_new_content && !refreshing, |col| {
                        col.child(
                            div().flex().justify_center().child(
                                div()
                                    .id("icymi-new-content-pill")
                                    .px(px(14.))
                                    .py(px(6.))
                                    .rounded(px(16.))
                                    .bg(rgb(0x5865f2))
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.icymi.has_new_content = false;
                                        this.icymi.refreshing = true;
                                        let _ = this.requests.send(fetch_feed_request(true));
                                        cx.notify();
                                    }))
                                    .child("New content available"),
                            ),
                        )
                    })
                    .when(self.icymi.loading && self.icymi.items.is_empty(), |col| {
                        col.child(
                            div()
                                .py(px(32.))
                                .text_center()
                                .text_size(px(14.))
                                .text_color(color("--text-muted"))
                                .child("Loading ICYMI feed..."),
                        )
                    })
                    .children(self.icymi.items.iter().map(|item| {
                        let (title, subtitle, body): (String, String, AnyElement) = match &item.kind
                        {
                            IcymiCardKind::Announcement {
                                guild_name,
                                channel_name,
                                excerpt,
                            } => (
                                guild_name.clone(),
                                format!("{} {in_word} #{channel_name}", dc_intl::t("8P08G9")),
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(SharedString::from(excerpt.clone()))
                                    .into_any_element(),
                            ),
                            IcymiCardKind::Message {
                                guild_name,
                                channel_name,
                                author,
                                excerpt,
                            } => (
                                guild_name.clone(),
                                format!("{} {in_word} #{channel_name}", dc_intl::t("hMFMY9")),
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(SharedString::from(author.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .text_color(color("--text-default"))
                                            .child(SharedString::from(excerpt.clone())),
                                    )
                                    .into_any_element(),
                            ),
                            IcymiCardKind::GuildEvent {
                                guild_name,
                                channel_name,
                                event_title,
                                active_now,
                                start_time,
                                interested_count,
                            } => {
                                let interested = dc_intl::format(
                                    "+DLsD8",
                                    &[("count", Arg::Num(*interested_count as f64))],
                                );
                                (
                                    guild_name.clone(),
                                    format!("{} {in_word} #{channel_name}", dc_intl::t("6pFsLQ")),
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(if *active_now {
                                                    color("--status-positive")
                                                } else {
                                                    color("--text-muted")
                                                })
                                                .child(if *active_now {
                                                    SharedString::from(dc_intl::t("TxqPQR"))
                                                } else {
                                                    SharedString::from(start_time.clone())
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(15.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .child(SharedString::from(event_title.clone())),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .child(SharedString::from(interested)),
                                        )
                                        .into_any_element(),
                                )
                            }
                            IcymiCardKind::ForumPost {
                                guild_name,
                                channel_name,
                                post_title,
                            } => (
                                guild_name.clone(),
                                format!("{} {in_word} #{channel_name}", dc_intl::t("bYNuVx")),
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(SharedString::from(post_title.clone()))
                                    .into_any_element(),
                            ),
                            IcymiCardKind::CustomStatus {
                                user_name,
                                status_text,
                            } => (
                                user_name.clone(),
                                dc_intl::t("fxOLPR").to_owned(),
                                div()
                                    .text_size(px(14.))
                                    .text_color(color("--text-default"))
                                    .child(SharedString::from(status_text.clone()))
                                    .into_any_element(),
                            ),
                            IcymiCardKind::PlayedGame {
                                user_name,
                                game_name,
                                most_played,
                            } => (
                                user_name.clone(),
                                dc_intl::t("ktOTRQ").to_owned(),
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon(
                                                "bot_game_controller",
                                                18.,
                                                "--icon-strong",
                                            ))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(SharedString::from(game_name.clone())),
                                            ),
                                    )
                                    .when(*most_played, |row| {
                                        row.child(
                                            div()
                                                .px(px(8.))
                                                .py(px(2.))
                                                .rounded(px(10.))
                                                .bg(rgb(0x5865f2))
                                                .text_size(px(11.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(dc_intl::t("/50eHi")),
                                        )
                                    })
                                    .into_any_element(),
                            ),
                        };

                        div()
                            .id(SharedString::from(format!("icymi-card-{}", item.id)))
                            .p(px(14.))
                            .rounded(px(10.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
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
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(SharedString::from(title)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .truncate()
                                                    .child(SharedString::from(subtitle)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.))
                                            .text_color(color("--text-muted"))
                                            .child(SharedString::from(item.timestamp.clone())),
                                    ),
                            )
                            .child(body)
                    }))
                    .when(!self.icymi.items.is_empty(), |col| {
                        col.child(
                            div()
                                .py(px(10.))
                                .text_center()
                                .text_size(px(12.))
                                .text_color(color("--text-muted"))
                                .child("You're all caught up!"),
                        )
                    }),
            );

        Some(
            gpui::deferred(
                div()
                    .id("icymi-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.icymi.open = false;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `icymi [open|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_icymi(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        if mode.trim() == "close" {
            self.icymi.open = false;
            cx.notify();
            return "icymi: closed".into();
        }
        self.icymi = IcymiState {
            open: true,
            loading: false,
            refreshing: false,
            has_new_content: true,
            items: demo_icymi_items(),
        };
        cx.notify();
        "icymi: open".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_icymi_feed_and_hydrate_requests() {
        assert!(matches!(
            fetch_feed_request(true),
            Request::Fetch { route, .. } if route.contains("refresh=true")
        ));
        assert!(matches!(
            hydrate_content_request(&[("1".into(), "2".into())], &[]),
            Request::Rest { route, .. } if route == "/gravity-content"
        ));
    }
}
