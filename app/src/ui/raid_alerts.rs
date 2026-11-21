//! Guild Raid / DM Spam Alert Banners (module 731204) and Security Actions Modal (module 671576).
//!
//! Renders active raid / DM-spam / paused-invites / paused-DMs banners inside Member Safety
//! (`member_safety.rs`) and Server Safety Setup (`server_safety_community.rs`), plus the
//! Security Actions confirmation modal (`PUT /guilds/{guildId}/incident-actions`) and
//! false-alarm dismissal (`POST /guilds/{guildId}/auto-moderation/false-alarm`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::{Value, json};
use std::borrow::Cow;

pub(crate) const INCIDENT_ACTIONS_KEY: &str = "guild:incident-actions";
pub(crate) const FALSE_ALARM_KEY: &str = "guild:incident-false-alarm";

/// Active alert kind determined from `Guild` incident fields (module 731204).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RaidAlertKind {
    InvitesAndDmsPaused,
    DmsPaused,
    InvitesPaused,
    UnusualDmActivity,
    UnusualJoinActivity,
}

/// Snapshot of an active guild raid/security incident banner.
#[derive(Clone, Debug)]
pub(crate) struct RaidBannerData {
    pub guild_id: Id,
    pub guild_name: String,
    pub kind: RaidAlertKind,
    pub time_label: String,
    pub invites_paused: bool,
    pub dms_paused: bool,
    pub invites_permanently_disabled: bool,
}

/// Duration choices in hours for `vKYZzc` ("Select a time") in Security Actions (`671576`).
const DURATION_OPTIONS: &[(u32, &str)] = &[
    (1, "1 hour"),
    (2, "2 hours"),
    (4, "4 hours"),
    (6, "6 hours"),
    (12, "12 hours"),
    (24, "24 hours"),
];

/// Open Security Actions modal state (`671576`).
#[derive(Clone, Debug)]
pub(crate) struct SecurityActionsModal {
    pub guild_id: Id,
    pub guild_name: String,
    pub pause_invites: bool,
    pub pause_dms: bool,
    pub invites_permanently_disabled: bool,
    pub duration_hours: u32,
    pub busy: bool,
    pub error: Option<String>,
}

/// Preview overlay mode for harness visual verification (`raid-alerts` command).
#[cfg(feature = "capture")]
#[derive(Clone, Debug)]
pub(crate) enum RaidAlertPreview {
    Banner(RaidBannerData),
}

impl RaidBannerData {
    /// Headline copy from module 731204 (`CEygF8`, `Wmbjcz`, `Q1QrAi`, `raRxxT`, `4nbAta`).
    pub(crate) fn title(&self) -> &'static str {
        match self.kind {
            RaidAlertKind::InvitesAndDmsPaused => dc_intl::t("CEygF8"),
            RaidAlertKind::DmsPaused => dc_intl::t("Wmbjcz"),
            RaidAlertKind::InvitesPaused => dc_intl::t("Q1QrAi"),
            RaidAlertKind::UnusualDmActivity => dc_intl::t("raRxxT"),
            RaidAlertKind::UnusualJoinActivity => dc_intl::t("4nbAta"),
        }
    }

    /// Body description copy (`hCZitf`, `HNKxf+`, `M3iSyL`, `4QIIZl`).
    pub(crate) fn body(&self) -> String {
        match self.kind {
            RaidAlertKind::InvitesAndDmsPaused => dc_intl::format(
                "hCZitf",
                &[
                    ("guildName", Arg::Str(Cow::Borrowed(&self.guild_name))),
                    ("time", Arg::Str(Cow::Borrowed(&self.time_label))),
                ],
            ),
            RaidAlertKind::DmsPaused => dc_intl::format(
                "HNKxf+",
                &[
                    ("guildName", Arg::Str(Cow::Borrowed(&self.guild_name))),
                    ("time", Arg::Str(Cow::Borrowed(&self.time_label))),
                ],
            ),
            RaidAlertKind::InvitesPaused => dc_intl::format(
                "M3iSyL",
                &[
                    ("guildName", Arg::Str(Cow::Borrowed(&self.guild_name))),
                    ("time", Arg::Str(Cow::Borrowed(&self.time_label))),
                ],
            ),
            RaidAlertKind::UnusualDmActivity | RaidAlertKind::UnusualJoinActivity => {
                dc_intl::format(
                    "4QIIZl",
                    &[("dateTime", Arg::Str(Cow::Borrowed(&self.time_label)))],
                )
            }
        }
    }

    /// Whether the alert is a detected raid/DM spike (critical red) vs active pause (warning amber).
    pub(crate) fn is_detected_incident(&self) -> bool {
        matches!(
            self.kind,
            RaidAlertKind::UnusualDmActivity | RaidAlertKind::UnusualJoinActivity
        )
    }

    /// Primary button label: `UgXhdn` ("Security Actions") or `e1bMNf` ("Edit Security Actions").
    pub(crate) fn action_label(&self) -> &'static str {
        if self.invites_paused || self.dms_paused {
            dc_intl::t("e1bMNf")
        } else {
            dc_intl::t("UgXhdn")
        }
    }
}

/// Parses an ISO-8601 timestamp string and returns true if it is in the future.
fn is_future_iso(
    iso: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = iso?;
    let parsed = chrono::DateTime::parse_from_rfc3339(raw)
        .ok()?
        .with_timezone(&chrono::Utc);
    (parsed > now).then_some(parsed)
}

/// Parses an ISO-8601 detection timestamp and returns true if within the 2-hour active incident window.
fn is_recent_incident(
    iso: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = iso?;
    let parsed = chrono::DateTime::parse_from_rfc3339(raw)
        .ok()?
        .with_timezone(&chrono::Utc);
    let age = now.signed_duration_since(parsed);
    (age.num_seconds() >= 0 && age.num_hours() < 2).then_some(parsed)
}

fn format_local_time(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.with_timezone(&chrono::Local)
        .format("%b %d, %I:%M %p")
        .to_string()
}

/// `PUT /guilds/{guildId}/incident-actions` (module 671576 / 731204).
pub(crate) fn incident_actions_request(
    guild_id: Id,
    pause_invites: bool,
    pause_dms: bool,
    duration_hours: u32,
) -> Request {
    let until = (chrono::Utc::now() + chrono::Duration::hours(i64::from(duration_hours)))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let body = json!({
        "invites_disabled_until": if pause_invites { Value::String(until.clone()) } else { Value::Null },
        "dms_disabled_until": if pause_dms { Value::String(until) } else { Value::Null },
    });
    Request::Rest {
        key: Some(INCIDENT_ACTIONS_KEY.to_owned()),
        method: reqwest::Method::PUT,
        route: format!("/guilds/{guild_id}/incident-actions"),
        body: Some(body),
    }
}

/// `POST /guilds/{guildId}/auto-moderation/false-alarm` ("Mark as Resolved" / `ISiXba`).
pub(crate) fn mark_resolved_request(guild_id: Id) -> Request {
    Request::Rest {
        key: Some(FALSE_ALARM_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild_id}/auto-moderation/false-alarm"),
        body: Some(json!({})),
    }
}

impl Root {
    /// Computes the active raid/security banner for `guild_id`, if any (module 731204).
    pub(super) fn active_raid_banner(&self, guild_id: Id) -> Option<RaidBannerData> {
        if !self.incident_actions_shown(guild_id) {
            return None;
        }
        let guild = self.stores.guild(guild_id)?;
        let now = chrono::Utc::now();
        let invites_until = is_future_iso(guild.invites_disabled_until.as_deref(), now);
        let dms_until = is_future_iso(guild.dms_disabled_until.as_deref(), now);
        let invites_perm = guild.features.iter().any(|f| f == "INVITES_DISABLED");

        if let (Some(inv), Some(dm)) = (invites_until, dms_until) {
            let until = inv.max(dm);
            return Some(RaidBannerData {
                guild_id,
                guild_name: guild.name.clone(),
                kind: RaidAlertKind::InvitesAndDmsPaused,
                time_label: format_local_time(until),
                invites_paused: true,
                dms_paused: true,
                invites_permanently_disabled: invites_perm,
            });
        }
        if let Some(until) = dms_until {
            return Some(RaidBannerData {
                guild_id,
                guild_name: guild.name.clone(),
                kind: RaidAlertKind::DmsPaused,
                time_label: format_local_time(until),
                invites_paused: false,
                dms_paused: true,
                invites_permanently_disabled: invites_perm,
            });
        }
        if let Some(until) = invites_until {
            return Some(RaidBannerData {
                guild_id,
                guild_name: guild.name.clone(),
                kind: RaidAlertKind::InvitesPaused,
                time_label: format_local_time(until),
                invites_paused: true,
                dms_paused: false,
                invites_permanently_disabled: invites_perm,
            });
        }
        if let Some(at) = is_recent_incident(guild.dm_spam_detected_at.as_deref(), now) {
            return Some(RaidBannerData {
                guild_id,
                guild_name: guild.name.clone(),
                kind: RaidAlertKind::UnusualDmActivity,
                time_label: format_local_time(at),
                invites_paused: false,
                dms_paused: false,
                invites_permanently_disabled: invites_perm,
            });
        }
        if let Some(at) = is_recent_incident(guild.raid_detected_at.as_deref(), now) {
            return Some(RaidBannerData {
                guild_id,
                guild_name: guild.name.clone(),
                kind: RaidAlertKind::UnusualJoinActivity,
                time_label: format_local_time(at),
                invites_paused: false,
                dms_paused: false,
                invites_permanently_disabled: invites_perm,
            });
        }
        None
    }

    /// Opens the Security Actions modal (`671576`) for a guild.
    pub(super) fn open_security_actions_modal(
        &mut self,
        data: &RaidBannerData,
        cx: &mut Context<Self>,
    ) {
        self.security_actions_modal = Some(SecurityActionsModal {
            guild_id: data.guild_id,
            guild_name: data.guild_name.clone(),
            pause_invites: data.invites_paused,
            pause_dms: data.dms_paused,
            invites_permanently_disabled: data.invites_permanently_disabled,
            duration_hours: 1,
            busy: false,
            error: None,
        });
        cx.notify();
    }

    /// Handles REST answers for `PUT /incident-actions` and `POST /auto-moderation/false-alarm`.
    pub(super) fn handle_raid_alert_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let failed = body["error"].is_string() || body["status"].as_u64().is_some_and(|s| s >= 400);
        if key == INCIDENT_ACTIONS_KEY {
            if let Some(modal) = self.security_actions_modal.as_mut() {
                modal.busy = false;
                if failed {
                    modal.error = Some(
                        body["body"]["message"]
                            .as_str()
                            .unwrap_or_else(|| dc_intl::t("5mlOCW"))
                            .to_owned(),
                    );
                } else {
                    let guild_id = modal.guild_id;
                    let until = (chrono::Utc::now()
                        + chrono::Duration::hours(i64::from(modal.duration_hours)))
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                    let pause_invites = modal.pause_invites;
                    let pause_dms = modal.pause_dms;
                    if let Some(guild) = self.stores.guilds.iter_mut().find(|g| g.id == guild_id) {
                        guild.invites_disabled_until = pause_invites.then_some(until.clone());
                        guild.dms_disabled_until = pause_dms.then_some(until);
                    }
                    self.security_actions_modal = None;
                }
            }
            cx.notify();
        } else if key == FALSE_ALARM_KEY {
            if !failed
                && let Some(guild_id) = self.guild
                && let Some(guild) = self.stores.guilds.iter_mut().find(|g| g.id == guild_id)
            {
                guild.raid_detected_at = None;
                guild.dm_spam_detected_at = None;
            }
            cx.notify();
        }
    }

    /// Renders an inline raid/security alert banner (module 731204) for Member Safety / Safety Setup.
    pub(super) fn render_raid_alert_banner(
        &self,
        data: &RaidBannerData,
        id_prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let critical = data.is_detected_incident();
        let accent_bg = if critical {
            rgba(0xda373c22)
        } else {
            rgba(0xf0b23222)
        };
        let accent_border = if critical {
            color("--status-danger")
        } else {
            color("--status-warning")
        };
        let accent_icon_tint = if critical {
            "--status-danger"
        } else {
            "--status-warning"
        };

        let open_data = data.clone();
        let guild_id = data.guild_id;

        div()
            .id(format!("{id_prefix}-raid-alert"))
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(accent_border)
            .bg(accent_bg)
            .px(px(16.))
            .py(px(12.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(div().flex_none().mt(px(2.)).child(icon(
                        "circle_info",
                        20.,
                        accent_icon_tint,
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(data.title()),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(data.body()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(critical, |row| {
                        row.child(
                            div()
                                .id(format!("{id_prefix}-raid-resolve"))
                                .cursor_pointer()
                                .px(px(12.))
                                .h(px(32.))
                                .rounded(px(6.))
                                .bg(color("--control-secondary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(dc_intl::t("ISiXba"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this.requests.send(mark_resolved_request(guild_id));
                                    if let Some(guild) =
                                        this.stores.guilds.iter_mut().find(|g| g.id == guild_id)
                                    {
                                        guild.raid_detected_at = None;
                                        guild.dm_spam_detected_at = None;
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .id(format!("{id_prefix}-raid-actions"))
                            .cursor_pointer()
                            .px(px(14.))
                            .h(px(32.))
                            .rounded(px(6.))
                            .bg(if critical {
                                color("--control-critical-primary-background-default")
                            } else {
                                color("--control-primary-background-default")
                            })
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(data.action_label())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_security_actions_modal(&open_data, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Optional inline banner for `guild_id` in Member Safety / Safety Setup.
    pub(super) fn guild_raid_alert_banner(
        &self,
        guild_id: Id,
        id_prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let data = self.active_raid_banner(guild_id)?;
        Some(self.render_raid_alert_banner(&data, id_prefix, cx))
    }

    /// Modal layer for Security Actions (`671576`) and harness banner preview (`731204`).
    pub(super) fn raid_alerts_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(modal) = self.security_actions_modal.as_ref() {
            return Some(self.render_security_actions_modal(modal, cx));
        }
        #[cfg(feature = "capture")]
        if let Some(RaidAlertPreview::Banner(data)) = self.raid_alert_preview.as_ref() {
            let banner = self.render_raid_alert_banner(data, "preview", cx);
            return Some(
                deferred(
                    div()
                        .id("raid-alert-preview-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b3))
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(24.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.raid_alert_preview = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .id("raid-alert-preview-card")
                                .w(px(740.))
                                .rounded(px(12.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lower"))
                                .p(px(20.))
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(banner),
                        ),
                )
                .with_priority(95)
                .into_any_element(),
            );
        }
        None
    }

    fn render_security_actions_modal(
        &self,
        modal: &SecurityActionsModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let toggle_pill = |on: bool, disabled: bool| {
            div()
                .flex_none()
                .w(px(40.))
                .h(px(24.))
                .rounded_full()
                .p(px(3.))
                .bg(if on {
                    color("--control-primary-background-default")
                } else {
                    color("--background-base-lowest")
                })
                .border_1()
                .border_color(if on {
                    color("--control-primary-background-default")
                } else {
                    color("--border-subtle")
                })
                .when(disabled, |d| d.opacity(0.5))
                .flex()
                .items_center()
                .when(on, |d| d.justify_end())
                .child(div().size(px(16.)).rounded_full().bg(color("--white")))
        };

        let invites_disabled = modal.invites_permanently_disabled;
        let duration_pills = DURATION_OPTIONS.iter().map(|&(hours, label)| {
            let selected = modal.duration_hours == hours;
            div()
                .id(format!("security-duration-{hours}"))
                .cursor_pointer()
                .px(px(10.))
                .h(px(30.))
                .rounded(px(6.))
                .border_1()
                .border_color(if selected {
                    color("--control-primary-background-default")
                } else {
                    color("--border-subtle")
                })
                .bg(if selected {
                    rgba(0x5865f233)
                } else {
                    color("--background-base-lowest")
                })
                .flex()
                .items_center()
                .text_size(px(13.))
                .font_weight(FontWeight(if selected { 600. } else { 400. }))
                .text_color(if selected {
                    color("--text-strong")
                } else {
                    color("--text-default")
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = this.security_actions_modal.as_mut() {
                        m.duration_hours = hours;
                    }
                    cx.notify();
                }))
        });

        let body = div()
            .px(px(20.))
            .py(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.))
            // Pause Invites row (`Uwsjn6`, `qPJkZh`, `9GPbsV`)
            .child(
                div()
                    .id("security-toggle-invites")
                    .when(!invites_disabled, |d| d.cursor_pointer())
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(16.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(m) = this.security_actions_modal.as_mut()
                            && !m.invites_permanently_disabled
                        {
                            m.pause_invites = !m.pause_invites;
                            m.error = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("Uwsjn6")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(if invites_disabled {
                                        dc_intl::t("9GPbsV")
                                    } else {
                                        dc_intl::t("qPJkZh")
                                    }),
                            ),
                    )
                    .child(toggle_pill(
                        modal.pause_invites || invites_disabled,
                        invites_disabled,
                    )),
            )
            .child(div().h(px(1.)).bg(color("--border-subtle")))
            // Pause DMs row (`wrDmA/`, `UQbJW7`)
            .child(
                div()
                    .id("security-toggle-dms")
                    .cursor_pointer()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(16.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(m) = this.security_actions_modal.as_mut() {
                            m.pause_dms = !m.pause_dms;
                            m.error = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("wrDmA/")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("UQbJW7")),
                            ),
                    )
                    .child(toggle_pill(modal.pause_dms, false)),
            )
            // Duration selector (`vKYZzc`) shown when either action is enabled
            .when(modal.pause_invites || modal.pause_dms, |col| {
                col.child(div().h(px(1.)).bg(color("--border-subtle")))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("vKYZzc").to_uppercase()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .children(duration_pills),
                            ),
                    )
            })
            .children(modal.error.as_ref().map(|err| {
                div()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(color("--status-danger"))
                    .bg(rgba(0xda373c1a))
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(13.))
                    .text_color(color("--text-feedback-critical"))
                    .child(err.clone())
            }));

        let guild_id = modal.guild_id;
        let pause_invites = modal.pause_invites;
        let pause_dms = modal.pause_dms;
        let duration_hours = modal.duration_hours;
        let busy = modal.busy;

        deferred(
            div()
                .id("security-actions-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.security_actions_modal = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .id("security-actions-modal")
                        .w(px(460.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-high"))
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px(px(20.))
                                .pt(px(18.))
                                .pb(px(14.))
                                .border_b_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_size(px(18.))
                                                .line_height(px(22.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("oCYAc7")),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .text_color(color("--text-muted"))
                                                .truncate()
                                                .child(modal.guild_name.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("security-actions-close")
                                        .flex_none()
                                        .size(px(32.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("modal_close", 18., "--icon-subtle"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.security_actions_modal = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(body)
                        .child(
                            div()
                                .px(px(20.))
                                .py(px(14.))
                                .bg(color("--background-base-lower"))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .id("security-actions-cancel")
                                        .cursor_pointer()
                                        .px(px(14.))
                                        .h(px(36.))
                                        .rounded(px(6.))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::tr("Cancel"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.security_actions_modal = None;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("security-actions-save")
                                        .when(!busy, |d| d.cursor_pointer())
                                        .when(busy, |d| d.opacity(0.6))
                                        .px(px(18.))
                                        .h(px(36.))
                                        .rounded(px(6.))
                                        .bg(color("--control-primary-background-default"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::tr("Save"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if busy {
                                                return;
                                            }
                                            if let Some(m) = this.security_actions_modal.as_mut() {
                                                m.busy = true;
                                                m.error = None;
                                            }
                                            let _ = this.requests.send(incident_actions_request(
                                                guild_id,
                                                pause_invites,
                                                pause_dms,
                                                duration_hours,
                                            ));
                                            cx.notify();
                                        })),
                                ),
                        ),
                ),
        )
        .with_priority(96)
        .into_any_element()
    }

    /// Capture harness: `raid-alerts [paused-both|paused-dms|paused-invites|raid|dm-spam|modal|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_raid_alerts(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let guild_name = self
            .stores
            .guild(guild_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Gravecord HQ".to_owned());
        let sample = |kind, invites_paused, dms_paused| RaidBannerData {
            guild_id,
            guild_name: guild_name.clone(),
            kind,
            time_label: "Nov 21, 08:00 PM".to_owned(),
            invites_paused,
            dms_paused,
            invites_permanently_disabled: false,
        };
        match arg.trim() {
            "paused-both" | "" => {
                self.security_actions_modal = None;
                self.raid_alert_preview = Some(RaidAlertPreview::Banner(sample(
                    RaidAlertKind::InvitesAndDmsPaused,
                    true,
                    true,
                )));
                cx.notify();
                "raid-alerts: paused-both".to_owned()
            }
            "paused-dms" => {
                self.security_actions_modal = None;
                self.raid_alert_preview = Some(RaidAlertPreview::Banner(sample(
                    RaidAlertKind::DmsPaused,
                    false,
                    true,
                )));
                cx.notify();
                "raid-alerts: paused-dms".to_owned()
            }
            "paused-invites" => {
                self.security_actions_modal = None;
                self.raid_alert_preview = Some(RaidAlertPreview::Banner(sample(
                    RaidAlertKind::InvitesPaused,
                    true,
                    false,
                )));
                cx.notify();
                "raid-alerts: paused-invites".to_owned()
            }
            "raid" => {
                self.security_actions_modal = None;
                self.raid_alert_preview = Some(RaidAlertPreview::Banner(sample(
                    RaidAlertKind::UnusualJoinActivity,
                    false,
                    false,
                )));
                cx.notify();
                "raid-alerts: raid".to_owned()
            }
            "dm-spam" => {
                self.security_actions_modal = None;
                self.raid_alert_preview = Some(RaidAlertPreview::Banner(sample(
                    RaidAlertKind::UnusualDmActivity,
                    false,
                    false,
                )));
                cx.notify();
                "raid-alerts: dm-spam".to_owned()
            }
            "modal" => {
                self.raid_alert_preview = None;
                let data = sample(RaidAlertKind::InvitesAndDmsPaused, true, true);
                self.open_security_actions_modal(&data, cx);
                "raid-alerts: modal".to_owned()
            }
            "close" => {
                self.raid_alert_preview = None;
                self.security_actions_modal = None;
                cx.notify();
                "raid-alerts: closed".to_owned()
            }
            other => format!("raid-alerts: unknown {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raid_banner_copy_formats_all_variants() {
        let b = RaidBannerData {
            guild_id: 1,
            guild_name: "Alpha".to_owned(),
            kind: RaidAlertKind::InvitesAndDmsPaused,
            time_label: "Nov 21, 08:00 PM".to_owned(),
            invites_paused: true,
            dms_paused: true,
            invites_permanently_disabled: false,
        };
        assert_eq!(b.title(), "Direct messages and invites are paused");
        assert!(b.body().contains("Alpha"));
        assert!(b.body().contains("Nov 21, 08:00 PM"));
        assert_eq!(b.action_label(), "Edit Security Actions");

        let r = RaidBannerData {
            kind: RaidAlertKind::UnusualJoinActivity,
            invites_paused: false,
            dms_paused: false,
            ..b
        };
        assert_eq!(r.title(), "Unusual join activity detected");
        assert!(r.is_detected_incident());
        assert_eq!(r.action_label(), "Security Actions");
    }
}
