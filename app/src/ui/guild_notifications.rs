//! The server's Notification Settings (server menu), measured in
//! `design/dom/modals/guild-notifications-darker-cozy.json` and ported from module 131527: Mute, the server
//! notification level, Community Activity Alerts, the suppress switches, Mute New Events and Mobile Push. Each switch
//! patches its field through `PATCH /users/@me/guilds/settings` (`muted`, `message_notifications`,
//! `suppress_everyone`, `suppress_roles`, `notify_highlights` 1 disabled / 2 enabled, `mute_scheduled_events`,
//! `mobile_push`).

use super::create_channel::{radio, switch};
use super::{Root, color, modal};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use serde_json::{Value, json};

const ALL_MESSAGES: u64 = 0;
const ONLY_MENTIONS: u64 = 1;
const NO_MESSAGES: u64 = 2;
const HIGHLIGHTS_DISABLED: u64 = 1;
const HIGHLIGHTS_ENABLED: u64 = 2;

impl Root {
    pub(super) fn open_guild_notifications(&mut self, cx: &mut Context<Self>) {
        self.guild_notifications = self.guild;
        cx.notify();
    }

    /// One field of the guild's settings, changed here at once and sent to the server.
    fn patch_guild_setting(&mut self, guild: Id, field: &'static str, value: Value) {
        if let Some(settings) = self.stores.guild_settings.guilds.get_mut(&guild) {
            settings.apply(field, &value);
        } else {
            let mut settings = dc_state::guild_settings::GuildSettings::from_value(&json!({}));
            settings.apply(field, &value);
            self.stores.guild_settings.guilds.insert(guild, settings);
        }
        let _ = self.requests.send(Request::GuildSettings {
            body: json!({"guilds": {guild.to_string(): {field: value}}}),
        });
    }

    pub(super) fn guild_notifications_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.guild_notifications?;
        let name = self.stores.guild(guild)?.name.clone();
        let settings = self
            .stores
            .guild_settings
            .guilds
            .get(&guild)
            .cloned()
            .unwrap_or_else(|| dc_state::guild_settings::GuildSettings::from_value(&json!({})));
        let muted = settings.muted;
        // `divider_ZaYWLb` (a 2px box with a 1px rule): 20px under a section, 30px before the next.
        let divider = |above: f32, below: f32| {
            div()
                .mt(px(above))
                .mb(px(below))
                .h(px(2.))
                .border_t_1()
                .border_color(color("--border-subtle"))
        };
        let label = |text: SharedString| {
            div()
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let description = |text: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-subtle"))
                .child(text)
        };
        // A switch row: label (and description) in 358px, the switch on the right; the click flips `field`.
        let toggle = |id: &'static str,
                      title: SharedString,
                      detail: Option<&'static str>,
                      on: bool,
                      disabled: bool,
                      flip: Box<dyn Fn(&mut Root)>| {
            div()
                .id(id)
                .flex()
                .when(detail.is_none(), |this| this.items_center())
                .gap(px(24.))
                .when(!disabled, |this| this.cursor_pointer())
                .when(disabled, |this| this.opacity(0.5))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !disabled {
                        flip(this);
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(label(title))
                        .children(detail.map(description)),
                )
                .child(switch(on))
        };
        let level = |value: u64, text: AnyElement| {
            let picked = settings.message_notifications == value;
            div()
                .id(SharedString::from(format!("guild-level-{value}")))
                .relative()
                .h(px(20.))
                .pl(px(32.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.patch_guild_setting(guild, "message_notifications", json!(value));
                    cx.notify();
                }))
                .child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .child(radio(picked)),
                )
                .child(
                    div()
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(text),
                )
        };
        let body = div()
            .id("guild-notifications-body")
            .h(px(444.))
            .mt(px(2.))
            .mr(px(-16.))
            .pr(px(8.))
            .overflow_y_scroll()
            .child(
                div()
                    .pt(px(12.))
                    .child(toggle(
                        "guild-mute",
                        format!("Mute {name}").into(),
                        Some(
                            "Muting a server prevents unread indicators and notifications from appearing unless you are mentioned.",
                        ),
                        muted,
                        false,
                        Box::new(move |this| this.patch_guild_setting(guild, "muted", json!(!muted))),
                    ))
                    .child(divider(20., 30.))
                    .child(label("Server Notification Settings".into()).mb(px(8.)))
                    .child(
                        div()
                            .pt(px(8.))
                            .pb(px(4.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(level(ALL_MESSAGES, div().child("All Messages").into_any_element()))
                            .child(level(
                                ONLY_MENTIONS,
                                div()
                                    .flex()
                                    .child("Only\u{a0}")
                                    .child(div().font_weight(FontWeight(600.)).child("@mentions"))
                                    .into_any_element(),
                            ))
                            .child(level(NO_MESSAGES, div().child("Nothing").into_any_element())),
                    )
                    .child(divider(30., 20.))
                    .child(label("Community Activity Alerts".into()).mb(px(4.)))
                    .child(description(
                        "Receive notifications for DM or join activity that exceeds usual numbers for your server.",
                    ))
                    .child(
                        div()
                            .mt(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            // `disableRaidAlertNag` / `disableRaidAlertPush` live in the settings proto's guild alert
                            // settings; this session keeps them until proto writes land.
                            .child({
                                let on = !self.raid_alerts_off.contains(&(guild, 0));
                                div().pb(px(8.)).child(toggle(
                                    "guild-raid-nag",
                                    "In-app alerts".into(),
                                    Some(
                                        "A global bar that appears across the top of Discord when you are using it, regardless of what channel or server you're in at the time.",
                                    ),
                                    on,
                                    false,
                                    Box::new(move |this| {
                                        if !this.raid_alerts_off.remove(&(guild, 0)) {
                                            this.raid_alerts_off.insert((guild, 0));
                                        }
                                    }),
                                ))
                            })
                            .child({
                                let on = !self.raid_alerts_off.contains(&(guild, 1));
                                div().pb(px(8.)).child(toggle(
                                    "guild-raid-push",
                                    "Push notifications".into(),
                                    Some("Sends to mobile or desktop devices when you are not using Discord."),
                                    on,
                                    false,
                                    Box::new(move |this| {
                                        if !this.raid_alerts_off.remove(&(guild, 1)) {
                                            this.raid_alerts_off.insert((guild, 1));
                                        }
                                    }),
                                ))
                            }),
                    )
                    .child(divider(30., 20.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child({
                                let on = settings.suppress_everyone;
                                toggle(
                                    "guild-suppress-everyone",
                                    "Suppress @everyone and @here".into(),
                                    None,
                                    on,
                                    false,
                                    Box::new(move |this| {
                                        this.patch_guild_setting(guild, "suppress_everyone", json!(!on))
                                    }),
                                )
                            })
                            .child(div().h(px(2.)).border_t_1().border_color(color("--border-subtle")))
                            .child({
                                let on = settings.suppress_roles;
                                toggle(
                                    "guild-suppress-roles",
                                    "Suppress All Role @mentions".into(),
                                    None,
                                    on,
                                    false,
                                    Box::new(move |this| this.patch_guild_setting(guild, "suppress_roles", json!(!on))),
                                )
                            })
                            .child(div().h(px(2.)).border_t_1().border_color(color("--border-subtle")))
                            .child({
                                let suppressed = muted || settings.notify_highlights == HIGHLIGHTS_DISABLED;
                                toggle(
                                    "guild-suppress-highlights",
                                    "Suppress Highlights".into(),
                                    Some(
                                        "Highlights provide occasional updates when your friends are chatting in busy servers, and more.",
                                    ),
                                    suppressed,
                                    muted,
                                    Box::new(move |this| {
                                        let value = if suppressed { HIGHLIGHTS_ENABLED } else { HIGHLIGHTS_DISABLED };
                                        this.patch_guild_setting(guild, "notify_highlights", json!(value))
                                    }),
                                )
                            })
                            .child(div().h(px(2.)).border_t_1().border_color(color("--border-subtle")))
                            .child({
                                let on = settings.mute_scheduled_events;
                                toggle(
                                    "guild-mute-events",
                                    "Mute New Events".into(),
                                    None,
                                    on,
                                    false,
                                    Box::new(move |this| {
                                        this.patch_guild_setting(guild, "mute_scheduled_events", json!(!on))
                                    }),
                                )
                            })
                            .child(div().h(px(2.)).border_t_1().border_color(color("--border-subtle")))
                            .child({
                                let on = !muted && settings.mobile_push;
                                toggle(
                                    "guild-mobile-push",
                                    "Mobile Push Notifications".into(),
                                    None,
                                    on,
                                    muted,
                                    Box::new(move |this| this.patch_guild_setting(guild, "mobile_push", json!(!on))),
                                )
                            }),
                    )
                    .child(div().h(px(40.))),
            )
            .into_any_element();
        Some(modal::render(
            modal::Spec {
                id: "guild-notifications-modal",
                title: "Notification Settings".into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![modal::button(
                    "guild-notifications-done",
                    "Done",
                    modal::Tone::Primary,
                    |this, _, _| this.guild_notifications = None,
                )],
                close: Box::new(|this, _, _| this.guild_notifications = None),
            },
            cx,
        ))
    }
}
