//! The channel header's Notification Settings (`channel-context`): Mute Channel with its durations (module 995102:
//! 15 minutes to 24 hours, or -1 for until turned back on), or Unmute Channel with "Muted until"; then the levels as
//! radios, the first naming what the channel would inherit. Changes go out as
//! `PATCH /users/@me/guilds/settings` `{guilds: {g: {channel_overrides: {c: {...}}}}}` (TRAFFIC/notif, mute).
//! Measured in `design/dom/menus/notifications-darker-cozy.json` and `mute-submenu-darker-cozy.json`.

use super::{Root, color};
use crate::net::Request;
use dc_state::Id;
use dc_state::guild_settings::{ALL_MESSAGES, INHERIT, NO_MESSAGES, ONLY_MENTIONS};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, deferred, div, prelude::*,
    px, svg,
};
use serde_json::json;

/// (seconds, label) of `p()` in module 995102.
const DURATIONS: [(i64, &str); 6] = [
    (900, "For 15 Minutes"),
    (3600, "For 1 Hour"),
    (10800, "For 3 Hours"),
    (28800, "For 8 Hours"),
    (86400, "For 24 Hours"),
    (-1, "Until I turn it back on"),
];

fn level_label(level: u64) -> &'static str {
    match level {
        ONLY_MENTIONS => "Only @mentions",
        NO_MESSAGES => "Nothing",
        _ => "All Messages",
    }
}

/// `j7h4AJ` with `toLocaleString(en-US, {month, day: numeric, hour: numeric, minute: 2-digit})`: `11/8, 4:46 AM`.
pub(crate) fn muted_until(end: chrono::DateTime<chrono::Utc>) -> String {
    let local = end.with_timezone(&chrono::Local);
    format!("Muted until {}", local.format("%-m/%-d, %-I:%M %p"))
}

impl Root {
    fn channel_settings_target(&self) -> Option<(Id, Id, Option<Id>)> {
        let guild = self.guild?;
        let channel = self.current_channel()?;
        Some((guild, channel.id, channel.parent_id))
    }

    /// Writes one channel override field and mirrors it locally until the gateway's update arrives.
    pub(super) fn patch_channel_override(
        &mut self,
        guild: Id,
        channel: Id,
        settings: serde_json::Value,
    ) {
        {
            let entry = self
                .stores
                .guild_settings
                .guilds
                .entry(guild)
                .or_default()
                .channels
                .entry(channel)
                .or_insert(dc_state::guild_settings::Override {
                    message_notifications: INHERIT,
                    ..Default::default()
                });
            if let Some(level) = settings["message_notifications"].as_u64() {
                entry.message_notifications = level;
            }
            if let Some(muted) = settings["muted"].as_bool() {
                entry.muted = muted;
                entry.mute_end = settings["mute_config"]["end_time"]
                    .as_str()
                    .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                    .map(|time| time.with_timezone(&chrono::Utc));
            }
        }
        let body = json!({"guilds": {guild.to_string(): {"channel_overrides": {channel.to_string(): settings}}}});
        let _ = self.requests.send(Request::GuildSettings { body });
    }

    /// Collapses or opens a category (`collapsed` in its channel override).
    pub(super) fn toggle_category(&mut self, guild: Id, category: Id) {
        let collapsed = self
            .stores
            .guild_settings
            .channel(guild, category)
            .collapsed;
        self.stores
            .guild_settings
            .guilds
            .entry(guild)
            .or_default()
            .channels
            .entry(category)
            .or_insert(dc_state::guild_settings::Override {
                message_notifications: INHERIT,
                ..Default::default()
            })
            .collapsed = !collapsed;
        let body = json!({"guilds": {guild.to_string(): {"channel_overrides": {category.to_string(): {"collapsed": !collapsed}}}}});
        let _ = self.requests.send(Request::GuildSettings { body });
    }

    pub(super) fn channel_muted_now(&self) -> bool {
        self.channel_settings_target()
            .is_some_and(|(guild, channel, _)| {
                self.stores
                    .guild_settings
                    .channel_muted(guild, channel, chrono::Utc::now())
            })
    }

    fn notify_row(
        &self,
        id: &'static str,
        label: AnyElement,
        subtext: Option<String>,
        trailing: Option<AnyElement>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .min_h(px(36.))
            .p(px(8.))
            .rounded(px(2.))
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(color("--interactive-background-hover"))
                    .rounded(px(4.))
            })
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(label)
                    .children(subtext.map(|subtext| {
                        div()
                            .mt(px(2.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(400.))
                            .text_color(color("--text-muted"))
                            .child(subtext)
                    })),
            )
            .children(trailing.map(|trailing| div().ml(px(8.)).child(trailing)))
    }

    fn label(text: &'static str) -> AnyElement {
        div()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(text)
            .into_any_element()
    }

    pub(super) fn notify_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.notify_open {
            return None;
        }
        let (guild, channel, category) = self.channel_settings_target()?;
        let button = self.tip_anchors.borrow().get("notifications").copied()?;
        let own = self.stores.guild_settings.channel(guild, channel);
        let muted = self
            .stores
            .guild_settings
            .channel_muted(guild, channel, chrono::Utc::now());
        let inherited = self.stores.guild_settings.inherited(guild, category);
        let level = own.message_notifications;
        let radio = |selected: bool| super::settings::radio_dot(selected).into_any_element();
        let mute_row = if muted {
            self.notify_row(
                "notify-unmute",
                Self::label("Unmute Channel"),
                own.mute_end.map(muted_until),
                None,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.patch_channel_override(guild, channel, json!({"muted": false}));
                this.notify_open = false;
                this.notify_submenu = None;
                cx.notify();
            }))
            .into_any_element()
        } else {
            self.notify_row(
                "notify-mute",
                Self::label("Mute Channel"),
                None,
                Some(
                    svg()
                        .path("icons/menu_caret.svg")
                        .size(px(20.))
                        .text_color(color("--text-muted"))
                        .into_any_element(),
                ),
            )
            .relative()
            .child(self.anchor_probe("notify-mute-row"))
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                if *inside {
                    this.notify_submenu = this.tip_anchors.borrow().get("notify-mute-row").copied();
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                // The row itself mutes until turned back on (`muted: true` alone).
                this.patch_channel_override(guild, channel, json!({"muted": true}));
                this.notify_open = false;
                this.notify_submenu = None;
                cx.notify();
            }))
            .into_any_element()
        };
        let levels: [(&'static str, u64); 4] = [
            ("notify-inherit", INHERIT),
            ("notify-all", ALL_MESSAGES),
            ("notify-mentions", ONLY_MENTIONS),
            ("notify-nothing", NO_MESSAGES),
        ];
        let level_rows: Vec<AnyElement> = levels
            .into_iter()
            .map(|(id, value)| {
                let (label, subtext) = match value {
                    INHERIT => (
                        Self::label(if category.is_some() {
                            "Use Category Default"
                        } else {
                            "Use Server Default"
                        }),
                        Some(level_label(inherited).to_owned()),
                    ),
                    ONLY_MENTIONS => (
                        div()
                            .flex()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Only\u{a0}")
                            .child(div().font_weight(FontWeight(600.)).child("@mentions"))
                            .into_any_element(),
                        None,
                    ),
                    other => (Self::label(level_label(other)), None),
                };
                self.notify_row(id, label, subtext, Some(radio(level == value)))
                    .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                        if *inside && this.notify_submenu.is_some() {
                            this.notify_submenu = None;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.patch_channel_override(
                            guild,
                            channel,
                            json!({"message_notifications": value}),
                        );
                        this.notify_open = false;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let right = (f32::from(window.viewport_size().width) - f32::from(button.right())).max(8.);
        let top = f32::from(button.bottom()) + 8.;
        Some(
            deferred(
                div()
                    .id("notify-menu")
                    .occlude()
                    .absolute()
                    .top(px(top))
                    .right(px(right))
                    .min_w(px(144.))
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
                        let in_submenu = this
                            .tip_anchors
                            .borrow()
                            .get("submenu")
                            .is_some_and(|bounds| bounds.contains(&event.position));
                        if !in_submenu {
                            this.notify_submenu = None;
                            if this.press_outside(super::keyboard::Layer::Notify, cx) {
                                cx.stop_propagation();
                            }
                        }
                    }))
                    .p(px(8.))
                    .flex()
                    .flex_col()
                    .child(mute_row)
                    .child(div().my(px(8.)).h(px(1.)).bg(color("--border-subtle")))
                    .children(level_rows),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Mute Channel's durations, beside the row.
    pub(super) fn notify_mute_submenu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.notify_open {
            return None;
        }
        let row = self.notify_submenu?;
        let (guild, channel, _) = self.channel_settings_target()?;
        let rows: Vec<(AnyElement, f32)> = DURATIONS
            .into_iter()
            .enumerate()
            .map(|(index, (seconds, label))| {
                (
                    self.submenu_item(
                        SharedString::from(format!("mute-for-{index}")),
                        None,
                        label.to_owned(),
                        34.,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let end = (seconds > 0).then(|| {
                            (chrono::Utc::now() + chrono::Duration::seconds(seconds))
                                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                                .to_string()
                        });
                        this.patch_channel_override(
                            guild,
                            channel,
                            json!({"muted": true, "mute_config": {"selected_time_window": seconds, "end_time": end}}),
                        );
                        this.notify_open = false;
                        this.notify_submenu = None;
                        cx.notify();
                    }))
                    .into_any_element(),
                    34.,
                )
            })
            .collect();
        Some(self.submenu_frame(rows, row, window))
    }
}

#[cfg(test)]
mod tests {
    use super::muted_until;

    #[test]
    fn muted_until_reads_like_the_locale_string() {
        let end = chrono::DateTime::parse_from_rfc3339("2026-11-08T01:46:03Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let text = muted_until(end);
        assert!(text.starts_with("Muted until 11/8, ") || text.starts_with("Muted until 11/7, "));
        assert!(text.ends_with("AM") || text.ends_with("PM"));
    }
}
