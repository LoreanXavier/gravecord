//! Recent Activity in the member list: 289287, 868065, 322789 and 86070.

use super::{Root, color, icon, modal};
use crate::{net::Request, parity::Parity};
use dc_state::{
    Id,
    content_inventory::{Entry, GLOBAL_FEED, member_feed_enabled},
    content_inventory_manager::{Conditions, ContentInventoryManager},
    permissions,
};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px, svg,
};
use serde_json::Value;
use std::collections::HashSet;

pub(super) const FEED_KEY: &str = "content-inventory:feed";

#[derive(Default)]
pub(super) struct State {
    manager: ContentInventoryManager,
    connected: bool,
    idle_ms: u64,
    timer_at: Option<i64>,
    timer: Option<gpui::Task<()>>,
    idle_timer: Option<gpui::Task<()>>,
    expanded: bool,
    hidden: bool,
    pub(super) about: bool,
    requested_members: HashSet<(Id, Id)>,
    #[cfg(feature = "capture")]
    requests: u32,
    #[cfg(feature = "capture")]
    responses: u32,
}

impl State {
    pub(super) fn restore() -> Self {
        Self {
            hidden: crate::storage::get("content_inventory.hidden")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            ..Self::default()
        }
    }
}

impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn inventory_status(&self) -> String {
        let conditions = self.inventory_conditions();
        let feed = self.stores.content_inventory.get_feed(GLOBAL_FEED);
        format!(
            "enabled={} connected={} focused={} hidden={} idle_ms={} requests={} responses={} entries={} scheduled={} push_driven={}",
            conditions.enabled,
            conditions.connected,
            conditions.focused,
            conditions.hidden,
            conditions.idle_ms,
            self.inventory.requests,
            self.inventory.responses,
            feed.map_or(0, |feed| feed.entries.len()),
            self.inventory.manager.next_fetch().is_some(),
            feed.is_some_and(|feed| feed.refresh_stale_inbox_after_ms.is_some()),
        )
    }

    pub(super) fn inventory_connection_open(&mut self) {
        self.inventory.requested_members.clear();
    }

    pub(super) fn inventory_connection_changed(&mut self, connected: bool, cx: &mut Context<Self>) {
        self.inventory.connected = connected;
        if !connected {
            self.inventory.manager.disconnected();
            self.inventory.timer = None;
            self.inventory.timer_at = None;
        } else {
            self.schedule_inventory(cx);
        }
    }

    fn inventory_conditions(&self) -> Conditions {
        Conditions {
            enabled: self
                .stores
                .experiments
                .variant("2026-03-content-inventory-memberlist-and-ranker")
                != Some(0),
            connected: self.inventory.connected && self.ready && self.stores.current_user.is_some(),
            focused: self.notification_focused,
            hidden: self.inventory.hidden,
            idle_ms: self.inventory.idle_ms,
        }
    }

    pub(super) fn start_inventory_idle_timer(&mut self, cx: &mut Context<Self>) {
        self.inventory.idle_timer = Some(cx.spawn(async move |this, cx| {
            loop {
                // IdleStore (885576) samples native input every ten seconds.
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(10))
                    .await;
                #[cfg(not(feature = "capture"))]
                let idle_ms = dc_hotkey::idle::system_idle_ms().unwrap_or(0);
                #[cfg(feature = "capture")]
                let idle_ms = 0;
                if this
                    .update(cx, |this, cx| {
                        this.inventory.idle_ms = idle_ms;
                        this.schedule_inventory(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(super) fn inventory_stale(&mut self, body: &Value, cx: &mut Context<Self>) {
        self.inventory.manager.inbox_stale(
            self.stores.content_inventory.get_feed(GLOBAL_FEED),
            body["refresh_after_ms"].as_u64(),
            chrono::Utc::now().timestamp_millis(),
        );
        self.schedule_inventory(cx);
    }

    pub(super) fn schedule_inventory(&mut self, cx: &mut Context<Self>) {
        let now = chrono::Utc::now().timestamp_millis();
        let conditions = self.inventory_conditions();
        self.inventory.manager.schedule(
            self.stores.content_inventory.get_feed(GLOBAL_FEED),
            conditions,
            now,
        );
        let deadline = self.inventory.manager.next_fetch();
        if deadline == self.inventory.timer_at {
            return;
        }
        self.inventory.timer = None;
        self.inventory.timer_at = deadline;
        if let Some(deadline) = deadline {
            let delay =
                std::time::Duration::from_millis(deadline.saturating_sub(now).max(0) as u64);
            self.inventory.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |this, cx| {
                    this.inventory.timer_at = None;
                    let conditions = this.inventory_conditions();
                    let feed = this.stores.content_inventory.get_feed(GLOBAL_FEED);
                    if this.inventory.manager.begin_fetch(
                        feed,
                        conditions,
                        chrono::Utc::now().timestamp_millis(),
                    ) {
                        let mut url = reqwest::Url::parse(
                            "https://discord.com/api/v9/content-inventory/users/@me",
                        )
                        .expect("constant URL");
                        let mut query = url.query_pairs_mut();
                        if let Some(cursor) = feed.and_then(|feed| feed.refresh_cursor()) {
                            query.append_pair("refresh_token", cursor);
                        }
                        query
                            .append_pair("for_game_profile", "false")
                            .append_pair("feature", "inbox");
                        drop(query);
                        let route = format!(
                            "/content-inventory/users/@me?{}",
                            url.query().unwrap_or_default()
                        );
                        let _ = this.requests.send(Request::Fetch {
                            key: FEED_KEY.to_owned(),
                            route,
                        });
                        #[cfg(feature = "capture")]
                        {
                            this.inventory.requests += 1;
                        }
                    }
                    cx.notify();
                });
            }));
        }
    }

    pub(super) fn inventory_hidden(&self) -> bool {
        self.inventory.hidden
    }

    pub(super) fn toggle_inventory_hidden(&mut self) {
        self.inventory.hidden = !self.inventory.hidden;
        crate::storage::set(
            "content_inventory.hidden",
            Value::Bool(self.inventory.hidden),
        );
    }

    pub(super) fn inventory_about_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.inventory.about {
            return None;
        }
        let row = |glyph, text: AnyElement| {
            div()
                .h(px(23.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(div().w(px(20.)).h(px(23.)).flex_none().child(icon(
                    glyph,
                    20.,
                    "--text-muted",
                )))
                .child(text)
        };
        let text = |text: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(text)
                .into_any_element()
        };
        let privacy = div()
            .id("inventory-privacy")
            .cursor_pointer()
            .text_color(color("--text-link"))
            .child("Activity Privacy")
            .on_click(cx.listener(|this, _, _, cx| {
                this.inventory.about = false;
                this.open_settings_page("activity-privacy", cx);
            }));
        let help = div()
            .id("inventory-help")
            .cursor_pointer()
            .text_color(color("--text-link"))
            .child("learn more")
            .on_click(cx.listener(|this, _, _, cx| {
                this.follow_link(
                    "https://support.discord.com/hc/en-us/articles/22045487931799",
                    "",
                    cx,
                );
            }));
        let body = div()
            .parity("inventory-about-notes")
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child("What you can do"),
            )
            .child(row(
                "home_friends",
                text("See the activity of people you talk to most"),
            ))
            .child(row(
                "chat_bubble",
                text("Spark a conversation with a reply or emoji"),
            ))
            .child(row(
                "pm_account",
                text("View and delete your past activity from your profile"),
            ))
            .child(row(
                "settings_small",
                div()
                    .flex()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child("Manage your ")
                    .child(privacy)
                    .child(" or ")
                    .child(help)
                    .into_any_element(),
            ));
        Some(modal::render(modal::Spec {
            id: "inventory-about", title: "About Recent Activity".into(),
            subtitle: Some(div().child("Keep up with what your friends are doing and easily start a conversation or join them!").into_any_element()),
            body: Some(div().pb(px(8.)).child(body).into_any_element()), buttons: Vec::new(),
            close: Box::new(|this, _, _| this.inventory.about = false),
        }, cx))
    }

    pub(super) fn inventory_fetched(&mut self, key: &str, body: &Value) -> bool {
        if key != FEED_KEY {
            return false;
        }
        if body["entries"].is_array() {
            #[cfg(feature = "capture")]
            {
                self.inventory.responses += 1;
            }
            self.inventory.manager.succeeded();
            self.stores.content_inventory.set_feed(
                GLOBAL_FEED,
                body,
                chrono::Utc::now().timestamp_millis(),
            );
        } else {
            let random = f64::from(uuid::Uuid::new_v4().as_u128() as u32) / 4_294_967_296.;
            if self
                .inventory
                .manager
                .failed(chrono::Utc::now().timestamp_millis(), random)
            {
                self.stores.content_inventory.clear_feed(GLOBAL_FEED);
            }
        }
        true
    }

    pub(super) fn prepare_inventory_feed(&mut self, cx: &mut Context<Self>) {
        self.schedule_inventory(cx);
        if !self.ready || self.stores.current_user.is_none() {
            return;
        }
        let Some(guild) = self.guild else {
            return;
        };
        let Some(feed) = self.stores.content_inventory.get_feed(GLOBAL_FEED) else {
            return;
        };
        let entries = feed.entries.clone();
        let mut members = Vec::new();
        for entry in entries {
            if self.stores.members.get(guild, entry.author).is_none()
                && self
                    .inventory
                    .requested_members
                    .insert((guild, entry.author))
            {
                members.push(entry.author);
            }
            if let Some(user) = self.stores.users.get(&entry.author).cloned() {
                let (key, url) = self.guild_avatar_source(&user);
                self.request_image(key, url);
            }
            if let Some(app) = entry.extra["application_id"]
                .as_str()
                .filter(|id| id.parse::<u64>().is_ok())
            {
                let key = format!("presence-app:{app}");
                if !self.fetched.contains_key(&key) {
                    self.fetched.insert(key.clone(), Value::Null);
                    let _ = self.requests.send(Request::Fetch {
                        key,
                        route: format!("/applications/public?application_ids={app}"),
                    });
                }
            }
            if let Some(url) = entry.extra["media"]["image_url"].as_str() {
                self.request_image(url.to_owned(), url.to_owned());
            }
        }
        if !members.is_empty() {
            let _ = self.requests.send(Request::GuildMembers {
                guilds: vec![guild],
                query: String::new(),
                limit: 0,
                user_ids: members,
            });
        }
    }

    fn member_inventory_entries(&self) -> Vec<&Entry> {
        if !self.inventory_conditions().enabled {
            return Vec::new();
        }
        let Some(guild) = self.current_guild() else {
            return Vec::new();
        };
        let Some(channel) = self.current_channel() else {
            return Vec::new();
        };
        if !member_feed_enabled(
            &guild.features,
            self.stores.member_counts.get_member_count(guild.id),
            channel.is_forum(),
        ) {
            return Vec::new();
        }
        let Some(feed) = self
            .stores
            .content_inventory
            .get_feed(GLOBAL_FEED)
            .filter(|f| f.request_id.is_some())
        else {
            return Vec::new();
        };
        let now = chrono::Utc::now();
        feed.entries
            .iter()
            .filter(|entry| {
                let presence = self.stores.presence.get(entry.author, None);
                let matched = presence.is_some_and(|p| {
                    p.visible_activities()
                        .iter()
                        .any(|activity| entry.matches_activity(activity))
                });
                if !entry.can_show_in_member_list(
                    now.timestamp_millis(),
                    presence.map_or("offline", |p| &p.status),
                    matched,
                ) {
                    return false;
                }
                if entry.participants.iter().any(|user| {
                    self.stores.private_channels.iter().any(|dm| {
                        dm.kind == 1
                            && dm.recipients.contains(user)
                            && self.stores.guild_settings.channel_muted(0, dm.id, now)
                    })
                }) {
                    return false;
                }
                let Some(member) = self.stores.members.get(guild.id, entry.author) else {
                    return false;
                };
                permissions::member_channel_permissions(guild, channel, entry.author, &member.roles)
                    & permissions::VIEW_CHANNEL
                    != 0
            })
            .collect()
    }

    fn inventory_card(&self, entry: &Entry, first: bool) -> Option<AnyElement> {
        let user = self.stores.users.get(&entry.author)?;
        let (title, art, first_badge) = match entry.kind {
            1 => {
                let app = entry.extra["application_id"].as_str().unwrap_or_default();
                (
                    entry.extra["game_name"].as_str().unwrap_or_default(),
                    self.images.get(&format!("app-icon:{app}")).cloned(),
                    (
                        "activity_game",
                        entry.age_text(chrono::Utc::now().timestamp_millis()),
                    ),
                )
            }
            6 if entry.trait_value(4, "range").and_then(Value::as_u64) == Some(1) => {
                let url = entry.extra["media"]["image_url"]
                    .as_str()
                    .unwrap_or_default();
                let count = entry.trait_value(12, "count").and_then(Value::as_u64);
                (
                    entry.extra["artist"]["name"].as_str().unwrap_or_default(),
                    self.images.get(url).cloned(),
                    (
                        "inventory_trophy",
                        count
                            .map(|count| format!("{count} tracks this week"))
                            .unwrap_or_default(),
                    ),
                )
            }
            _ => return None,
        };
        let badge = |glyph, text: String, tint| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(icon(glyph, 12., tint))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-subtle"))
                        .child(text),
                )
        };
        let avatar = self.images.get(&self.guild_avatar_source(user).0).cloned();
        let name = self.guild_display_name(user);
        let info = div()
            .flex_1()
            .min_w(px(0.))
            .mb(px(-2.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(18.))
                    .mb(px(2.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .size(px(16.))
                            .flex_none()
                            .mr(px(6.))
                            .children(avatar.map(|image| img(image).size(px(16.)).rounded_full())),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .text_ellipsis()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(self.guild_author_color(entry.author, "--channels-default"))
                            .child(name),
                    ),
            )
            .child(
                div()
                    .when(first, |d| d.parity("inventory-title"))
                    .h(px(18.))
                    .text_ellipsis()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(title.to_owned()),
            )
            .child(
                div()
                    .h(px(16.))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(!first_badge.1.is_empty(), |d| {
                        d.child(badge(first_badge.0, first_badge.1, "--icon-subtle"))
                    })
                    .when(
                        entry.kind == 1
                            && entry.trait_value(1, "first_time").and_then(Value::as_bool)
                                == Some(true),
                        |d| {
                            d.child(badge(
                                "activity_new_player",
                                "New Player".to_owned(),
                                "--status-positive",
                            ))
                        },
                    ),
            );
        Some(
            div()
                .when(first, |d| d.parity_with_border("inventory-card", 1.))
                .id(SharedString::from(format!("inventory-entry-{}", entry.id)))
                .ml(px(8.))
                .mt(px(8.))
                .w(px(248.))
                .h(px(70.))
                .flex_none()
                .p(px(8.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-muted"))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(info)
                .child(
                    div()
                        .when(first, |d| d.parity("inventory-thumbnail"))
                        .size(px(48.))
                        .flex_none()
                        .rounded(px(4.))
                        .overflow_hidden()
                        .child(match art {
                            Some(image) => {
                                img(image).size(px(48.)).rounded(px(4.)).into_any_element()
                            }
                            None => {
                                icon("inventory_unknown_game", 48., "--white").into_any_element()
                            }
                        }),
                )
                .into_any_element(),
        )
    }

    pub(super) fn inventory_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let entries = self.member_inventory_entries();
        if entries.is_empty() {
            return None;
        }
        let expanded = self.inventory.expanded;
        let hidden = self.inventory.hidden;
        let menu_open = self.context.open.as_ref().is_some_and(|open| {
            matches!(
                open.target,
                super::context_menu::Target::InventoryFeed
                    | super::context_menu::Target::InventoryFeedAtPointer
            )
        });
        let count = entries.len();
        let cards = entries
            .into_iter()
            .take(if hidden {
                0
            } else if expanded {
                count
            } else {
                3
            })
            .enumerate()
            .filter_map(|(index, entry)| self.inventory_card(entry, index == 0))
            .collect::<Vec<_>>();
        let header = div()
            .h(px(42.))
            .flex_none()
            .pt(px(20.))
            .pb(px(4.))
            .pl(px(16.))
            .pr(px(4.))
            .child(
                div()
                    .parity("inventory-header")
                    .id("inventory-expand")
                    .h(px(24.))
                    .mt(px(-3.))
                    .mb(px(-3.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                            this.open_context_menu(
                                super::context_menu::Target::InventoryFeedAtPointer,
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .when(hidden || count > 3, |d| {
                        d.cursor_pointer().on_click(cx.listener(|this, _, _, cx| {
                            if this.inventory.hidden {
                                this.toggle_inventory_hidden();
                            } else {
                                this.inventory.expanded = !this.inventory.expanded;
                            }
                            cx.notify();
                        }))
                    })
                    .child(format!("Activity — {count}"))
                    .child(
                        div()
                            .id("inventory-settings")
                            .relative()
                            .size(px(24.))
                            .flex_none()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .group("inventory-settings")
                            .child(
                                svg()
                                    .path("icons/settings_small.svg")
                                    .size(px(12.))
                                    .text_color(color(if menu_open {
                                        "--interactive-icon-active"
                                    } else {
                                        "--channels-default"
                                    }))
                                    .group_hover("inventory-settings", |s| {
                                        s.text_color(color("--interactive-icon-hover"))
                                    }),
                            )
                            .child(self.anchor_probe("inventory-settings"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                let bounds =
                                    this.tip_anchors.borrow().get("inventory-settings").copied();
                                if let Some(bounds) = bounds {
                                    this.open_context_menu(
                                        super::context_menu::Target::InventoryFeed,
                                        gpui::point(bounds.right(), bounds.bottom() + px(8.)),
                                        cx,
                                    );
                                }
                            })),
                    )
                    .child(div().flex_1())
                    .when(hidden || count > 3, |d| {
                        d.child(
                            div()
                                .w(px(24.))
                                .h(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(
                                    if hidden {
                                        "inventory_chevron_up"
                                    } else if expanded {
                                        "chevron_down"
                                    } else {
                                        "inventory_chevron_right"
                                    },
                                    18.,
                                    "--interactive-icon-default",
                                )),
                        )
                    }),
            );
        Some(
            div()
                .flex_none()
                .w(px(256.))
                .flex()
                .flex_col()
                .child(header)
                .children(cards)
                .into_any_element(),
        )
    }
}
