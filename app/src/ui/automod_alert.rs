//! AutoMod's raid notification in the safety alerts channel (module 117096 `iC` around `ix`, its `raid` card `iE`):
//! a type 24 message whose embed is `auto_moderation_notification` with `notification_type` `raid` (or none).
//!
//! The row (cozy) is the system line shell of module 888675 (`mainContainer_q-l2Gj`) with AutoMod's 40px avatar in the
//! gutter (`/assets/32336875baac6ac1.png`, the gif while hovered), "AutoMod" (`text-brand`, semibold), the SYSTEM tag,
//! "safety alert!" and the time, then the card (`embedCard_q-l2Gj`: `--background-base-lower`, 8px corners, 4px above
//! and 12px below; `cardContent_q-l2Gj` 12px in, 8px apart):
//!
//! - the header (`cardHeader_q-l2Gj`, 4px apart): the 12px chat warning (a join raid) or circled error (a DM raid) in
//!   `--text-feedback-critical` and "Unusual Join Activity Detected" / "Unusual DM Activity Detected"
//!   (`body-md/semibold`, `text-feedback-critical`);
//! - the subheader (`dotSeparatedRow_q-l2Gj`, 4px apart, `body-xs/medium`): "~N Joins Actioned", "~N DMs Detected",
//!   the dot (4px, `--background-mod-subtle`) and how long ago the raid began;
//! - "Activity began on {dateTime}. ..." (`body-md/normal`, `text-muted`);
//! - for a member holding any of `Sz` (`shouldShowIncidentActions` of module 585510), the footer
//!   (`cardFooter_q-l2Gj`: `--background-mod-normal`, 4px in): the brand shield and Actions (the text button of module
//!   123292, `primary`: `--text-brand`), which opens `moderation-raid-context` (module 495372) at the pointer, the dot,
//!   then the resolved reason (`Oj` of module 233444, Mark as resolved while none), which opens "What happened?"
//!   (module 580812).
//!
//! GAP: the compact layout (`compact_q-l2Gj`) and the other notification types (`mention_raid`, `activity_alerts_enabled`,
//! `interaction_blocked`, the fallback) are not drawn here; they keep the ordinary message row.

use super::chat_notices::{Alert, AlertButton, Content, Then};
use super::modal::Tone;
use super::{Root, color, icon};
use dc_state::{Id, Message, message_kinds};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

const AVATAR: &str = "https://discord.com/assets/32336875baac6ac1.png";
const AVATAR_ANIMATED: &str = "https://discord.com/assets/d9bd8932f55dc4ba.gif";
/// Both, for the list's image requests.
pub(super) const AVATARS: [&str; 2] = [AVATAR, AVATAR_ANIMATED];

/// `RAID_TYPE` of the card (`it` of module 117096).
const DM_RAID: &str = "DM_RAID";

/// The reasons of module 580812 (`no` of module 615550) with their labels, in its order.
pub(super) const RESOLUTIONS: [(&str, &str); 4] = [
    ("LEGITIMATE_ACTIVITY", "yeaXw5"),
    ("DM_SPAM", "o++3B8"),
    ("JOIN_RAID", "UfHAwZ"),
    ("OTHER", "K3UWeR"),
];
pub(super) const RESOLUTION_OTHER: usize = 3;

/// The fields of the card (`Bk` of module 233444: the first embed's fields by `rawName`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct RaidFields {
    pub notification_type: Option<String>,
    pub join_attempts: Option<i64>,
    pub raid_datetime: Option<chrono::DateTime<chrono::Utc>>,
    pub dms_sent: Option<i64>,
    pub raid_type: Option<String>,
    pub resolved_reason: Option<String>,
}

pub(super) fn raid_fields(message: &Message) -> RaidFields {
    let Some(embed) = message.embeds.first().filter(|embed| {
        embed.kind == "auto_moderation_notification" || embed.kind == "auto_moderation_message"
    }) else {
        return RaidFields::default();
    };
    let field = |name: &str| {
        embed
            .fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.value.clone())
    };
    RaidFields {
        notification_type: field("notification_type"),
        join_attempts: field("join_attempts").and_then(|value| value.parse().ok()),
        raid_datetime: field("raid_datetime")
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
            .map(|at| at.with_timezone(&chrono::Utc)),
        dms_sent: field("dms_sent").and_then(|value| value.parse().ok()),
        raid_type: field("raid_type"),
        resolved_reason: field("resolved_reason"),
    }
}

/// `de` of module 233444 and the `raid` branch of `ix`: an AutoMod notification whose type is `raid` or unset.
pub(super) fn is_raid_card(message: &Message) -> bool {
    message.kind == message_kinds::AUTO_MODERATION_ACTION
        && message
            .embeds
            .iter()
            .any(|embed| embed.kind == "auto_moderation_notification")
        && raid_fields(message)
            .notification_type
            .is_none_or(|kind| kind == "raid")
}

/// `Oj` of module 233444: the footer's second button.
pub(super) fn resolved_label(reason: Option<&str>) -> &'static str {
    match reason {
        None => dc_intl::t("Gh3A0O"),
        Some("LEGITIMATE_ACTIVITY") => dc_intl::t("riQ+HH"),
        Some("DM_SPAM") => dc_intl::t("j5V0ij"),
        Some("JOIN_RAID") => dc_intl::t("qhaRbG"),
        Some(_) => dc_intl::t("GPg6JM"),
    }
}

/// `W_` of module 615550: the reason the false alarm report sends for the chosen ones.
pub(super) fn resolution_reason(chosen: &[usize]) -> &'static str {
    for (index, (kind, _)) in RESOLUTIONS.iter().enumerate().take(3) {
        if chosen.contains(&index) {
            return kind;
        }
    }
    "OTHER"
}

/// Module 580812 on the dialog of module 189213: "What happened?", its subtitle, the checkbox group of
/// `RESOLUTIONS` with the Other text box, Cancel and Mark as resolved.
pub(super) fn resolve_alert(guild: Id, message: Id) -> Alert {
    Alert {
        id: "raid-resolve",
        title: dc_intl::t("1zmw/H").to_owned(),
        subtitle: Some(vec![(dc_intl::t("nF79oO").to_owned(), false)]),
        content: Some(Content::RaidResolve { chosen: Vec::new() }),
        buttons: vec![
            AlertButton {
                id: "raid-resolve-cancel",
                label: dc_intl::t("ETE/oC").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "raid-resolve-submit",
                label: dc_intl::t("Gh3A0O").to_owned(),
                tone: Tone::Primary,
                then: Then::ResolveRaid { guild, message },
            },
        ],
    }
}

/// `A` of module 859706: `POST /guilds/{id}/auto-moderation/false-alarm` with the alert and the reason.
pub(super) fn false_alarm_request(guild: Id, message: Id, reason: &str) -> crate::net::Request {
    crate::net::Request::Rest {
        key: None,
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild}/auto-moderation/false-alarm"),
        body: Some(serde_json::json!({
            "alert_message_id": message.to_string(),
            "reason": reason,
        })),
    }
}

/// `kZ` of module 233444 (used by `485304.js`): whether an AutoMod alert message has already been
/// marked completed (`alert_actions_execution.actions` contains `"1"`, `SET_COMPLETED = 1`).
pub(super) fn alert_is_completed(message: &Message) -> bool {
    let Some(raw) = message.embeds.first().and_then(|embed| {
        embed
            .fields
            .iter()
            .find(|f| f.name == "alert_actions_execution")
            .map(|f| f.value.as_str())
    }) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    parsed
        .get("actions")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|map| map.contains_key("1"))
}

/// Module 794267 (`845625.G.SUBMIT_FEEDBACK = 4`): report an AutoMod alert as a false positive.
pub(super) const ALERT_ACTION_SUBMIT_FEEDBACK: u8 = 4;

/// `E5` of module 179832 (used by `485304.js`): `POST /guilds/{guildId}/auto-moderation/alert-action`
/// with `alert_action_type = 1` (`SET_COMPLETED`) or `2` (`UNSET_COMPLETED`).
pub(super) fn alert_action_request(
    guild: Id,
    channel: Id,
    message: Id,
    currently_completed: bool,
) -> crate::net::Request {
    let action_type: u8 = if currently_completed { 2 } else { 1 };
    crate::net::Request::Rest {
        key: Some(format!("automod-alert-action:{message}")),
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild}/auto-moderation/alert-action"),
        body: Some(serde_json::json!({
            "message_id": message.to_string(),
            "channel_id": channel.to_string(),
            "alert_action_type": action_type,
        })),
    }
}

/// Module 794267 (`179832.E5(messageId, channel, 4)`): `POST /guilds/{guildId}/auto-moderation/alert-action`
/// with `alert_action_type = 4` (`SUBMIT_FEEDBACK`).
pub(super) fn submit_feedback_request(guild: Id, channel: Id, message: Id) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("automod-feedback:{message}")),
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild}/auto-moderation/alert-action"),
        body: Some(serde_json::json!({
            "message_id": message.to_string(),
            "channel_id": channel.to_string(),
            "alert_action_type": ALERT_ACTION_SUBMIT_FEEDBACK,
        })),
    }
}

/// Module 794267 (`449585.w2` / `189213.a`): "Did we get it wrong?" AutoMod false-positive feedback
/// confirmation dialog (`7bdzNo`, `Lbpk6m`, Cancel `ETE/oC`, Yes `p89ACt`).
pub(super) fn feedback_alert(guild: Id, channel: Id, message: Id) -> Alert {
    Alert {
        id: "automod-feedback",
        title: dc_intl::t("7bdzNo").to_owned(),
        subtitle: Some(vec![(dc_intl::t("Lbpk6m").to_owned(), false)]),
        content: None,
        buttons: vec![
            AlertButton {
                id: "automod-feedback-cancel",
                label: dc_intl::t("ETE/oC").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "automod-feedback-confirm",
                label: dc_intl::t("p89ACt").to_owned(),
                tone: Tone::Primary,
                then: Then::SubmitAutomodFeedback {
                    guild,
                    channel,
                    message,
                },
            },
        ],
    }
}

/// `fromNow` of moment (its `en` relative time words and default thresholds: 45 s, 45 min, 22 h, 26 d, 11 months).
fn from_now(at: chrono::DateTime<chrono::Utc>, now: chrono::DateTime<chrono::Utc>) -> String {
    let seconds = (now - at).num_seconds().max(0);
    let minutes = (seconds as f64 / 60.).round() as i64;
    let hours = (seconds as f64 / 3600.).round() as i64;
    let days = (seconds as f64 / 86_400.).round() as i64;
    match seconds {
        0..45 => "a few seconds ago".to_owned(),
        45..90 => "a minute ago".to_owned(),
        _ if minutes < 45 => format!("{minutes} minutes ago"),
        _ if minutes < 90 => "an hour ago".to_owned(),
        _ if hours < 22 => format!("{hours} hours ago"),
        _ if hours < 36 => "a day ago".to_owned(),
        _ if days < 26 => format!("{days} days ago"),
        _ if days < 46 => "a month ago".to_owned(),
        _ if days < 320 => format!("{} months ago", (days as f64 / 30.4).round() as i64),
        _ if days < 548 => "a year ago".to_owned(),
        _ => format!("{} years ago", (days as f64 / 365.).round() as i64),
    }
}

fn dot() -> gpui::Div {
    div()
        .size(px(4.))
        .flex_none()
        .rounded_full()
        .bg(color("--background-mod-subtle"))
}

impl Root {
    /// `shouldShowIncidentActions` of module 585510: any of `Sz` in the server.
    pub(super) fn incident_actions_shown(&self, guild: Id) -> bool {
        self.stores
            .guild(guild)
            .zip(self.stores.me())
            .is_some_and(|(known, me)| {
                dc_state::permissions::guild_permissions(known, me)
                    & super::channel_settings_pages::INCIDENT_ACTIONS
                    != 0
            })
    }

    /// The line of `iC` for a raid card, for the system row.
    pub(super) fn automod_raid_line(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fields = raid_fields(message);
        let guild = self.guild_of_channel(message.channel_id);
        let id = message.id;
        let hover_key = format!("automod-avatar:{id}");
        let animated = self.avatar_hovered(&hover_key) && !super::motion::reduced();
        let avatar = self
            .images
            .get(if animated { AVATAR_ANIMATED } else { AVATAR })
            .or_else(|| self.images.get(AVATAR))
            .cloned();
        let local = message.timestamp.with_timezone(&chrono::Local);
        let time = super::header_time(local, chrono::Local::now());
        let dm_raid = fields.raid_type.as_deref() == Some(DM_RAID);
        let title = if dm_raid {
            dc_intl::t("8+lHUb")
        } else {
            dc_intl::t("xMwcwV")
        };
        let mut subheader = div().flex().flex_row().items_center().gap(px(4.));
        let small = |text: String| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(text)
        };
        if let Some(count) = fields.join_attempts {
            subheader = subheader.child(small(dc_intl::format(
                "4ylIiu",
                &[("joinCount", count.into())],
            )));
        }
        if let Some(count) = fields.dms_sent {
            subheader = subheader.child(small(dc_intl::format(
                "5C8Mh3",
                &[("dmsSent", count.into())],
            )));
        }
        if let Some(at) = fields.raid_datetime {
            subheader = subheader
                .child(dot())
                .child(small(from_now(at, chrono::Utc::now())));
        }
        let content = fields.raid_datetime.map(|at| {
            // `toLocaleString(currentLocale, yc)` with `yc` of module 903093: `{month: "numeric", day: "numeric",
            // hour: "numeric", minute: "2-digit"}`, in en-US "11/19, 2:51 AM".
            let local = at.with_timezone(&chrono::Local);
            let date_time = local.format("%-m/%-d, %-I:%M %p").to_string();
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-muted"))
                .child(dc_intl::format(
                    "4QIIZl",
                    &[("dateTime", date_time.as_str().into())],
                ))
        });
        let text_button = |key: &'static str, label: &'static str| {
            div()
                .id(SharedString::from(format!("{key}-{id}")))
                .cursor_pointer()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-brand"))
                .hover(|style| style.underline())
                .child(label)
        };
        let channel_id = message.channel_id;
        let can_manage_messages = guild.is_some_and(|g| {
            self.stores
                .guild(g)
                .zip(self.stores.me())
                .is_some_and(|(known, me)| {
                    dc_state::permissions::guild_permissions(known, me)
                        & dc_state::permissions::MANAGE_MESSAGES
                        != 0
                })
        });
        let footer = guild
            .filter(|guild| self.incident_actions_shown(*guild))
            .map(|guild| {
                div()
                    .p(px(4.))
                    .bg(color("--background-mod-normal"))
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .p(px(8.))
                            .flex()
                            .items_center()
                            .child(div().mr(px(4.)).child(icon(
                                "guild_menu_privacy",
                                12.,
                                "--brand-500",
                            )))
                            .child(
                                text_button("automod-raid-actions", dc_intl::t("DEoVWZ")).on_click(
                                    cx.listener(move |this, click: &gpui::ClickEvent, _, cx| {
                                        this.open_context_menu(
                                            super::context_menu::Target::RaidAlert(guild),
                                            click.position(),
                                            cx,
                                        );
                                    }),
                                ),
                            ),
                    )
                    .child(dot())
                    .child(
                        div().p(px(8.)).flex().child(
                            text_button(
                                "automod-raid-resolve",
                                resolved_label(fields.resolved_reason.as_deref()),
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.open_raid_resolve(guild, id, window, cx);
                                },
                            )),
                        ),
                    )
                    .when(can_manage_messages, |row| {
                        row.child(dot()).child(div().p(px(8.)).flex().child(
                            text_button("automod-raid-feedback", dc_intl::t("7bdzNo")).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.open_automod_feedback(guild, channel_id, id, cx);
                                }),
                            ),
                        ))
                    })
            });
        let card = div()
            .mt(px(4.))
            .mb(px(12.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(icon(
                                        if dm_raid {
                                            "inbox_overdue"
                                        } else {
                                            "mod_info_chat_warning"
                                        },
                                        12.,
                                        "--text-feedback-critical",
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-feedback-critical"))
                                            .child(title),
                                    ),
                            )
                            .child(subheader),
                    )
                    .children(content),
            )
            .children(footer);
        let alert_msg_id = message.id;
        let alert_channel = message.channel_id;
        let alert_guild = self.guild.unwrap_or(0);
        let alert_author = message.author.id;
        div()
            .relative()
            .pl(px(72.))
            .pr(px(48.))
            .py(px(2.))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                    if alert_guild != 0 {
                        cx.stop_propagation();
                        this.open_context_menu(
                            super::context_menu::Target::ModerationUser {
                                user: alert_author,
                                guild: alert_guild,
                                channel: Some(alert_channel),
                                alert_id: Some(alert_msg_id),
                                show_transfer_ownership: true,
                            },
                            ev.position,
                            cx,
                        );
                    }
                }),
            )
            .child(
                div()
                    .id(SharedString::from(hover_key.clone()))
                    .on_hover(self.avatar_hover(hover_key, cx))
                    .absolute()
                    .left(px(16.))
                    .top(px(2.))
                    .size(px(40.))
                    .rounded_full()
                    .overflow_hidden()
                    .children(avatar.map(|avatar| img(avatar).size(px(40.)))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_size(px(16.))
                    .line_height(px(24.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .child(
                                div()
                                    .mr(px(4.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-brand"))
                                    .child(dc_intl::t("hG1StD")),
                            )
                            .child(div().mr(px(2.)).mt(px(2.)).child(self.app_tag_px(
                                super::message_header::AppTag::Official,
                                format!("automod-tag-{id}"),
                                cx,
                            )))
                            .child(
                                div()
                                    .ml(px(2.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("ufawcw")),
                            )
                            .child(
                                div()
                                    .ml(px(8.))
                                    .text_size(px(12.))
                                    .line_height(px(22.))
                                    .text_color(color("--text-muted"))
                                    .child(time),
                            ),
                    )
                    .child(card),
            )
            .into_any_element()
    }

    /// The resolved button of the card: module 580812 with its Other text box.
    pub(super) fn open_raid_resolve(
        &mut self,
        guild: Id,
        message: Id,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            // `.textArea_uI0xjn` keeps `line-height: normal`: 21px for gg sans at 16px.
            let mut style = super::editor_style();
            style.line_height = 21.;
            let mut input = crate::editor::Editor::new(style, cx);
            input.set_keeps_text(true);
            input.set_placeholder(dc_intl::t("PAM+JR").to_owned());
            input
        });
        self.chat_extra.lockdown_other = Some(input);
        self.chat_extra.alert = Some(resolve_alert(guild, message));
        cx.notify();
    }

    /// Module 794267: open the "Did we get it wrong?" false-positive feedback confirmation dialog.
    pub(super) fn open_automod_feedback(
        &mut self,
        guild: Id,
        channel: Id,
        message: Id,
        cx: &mut Context<Self>,
    ) {
        self.chat_extra.alert = Some(feedback_alert(guild, channel, message));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resolution_reason_follows_the_first_chosen_kind() {
        assert_eq!(resolution_reason(&[]), "OTHER");
        assert_eq!(resolution_reason(&[3]), "OTHER");
        assert_eq!(resolution_reason(&[2, 1]), "DM_SPAM");
        assert_eq!(resolution_reason(&[0, 2]), "LEGITIMATE_ACTIVITY");
        assert_eq!(resolved_label(None), "Mark as resolved");
    }

    #[test]
    fn from_now_reads_like_moment() {
        let now = chrono::Utc::now();
        assert_eq!(from_now(now, now), "a few seconds ago");
        assert_eq!(
            from_now(now - chrono::Duration::minutes(10), now),
            "10 minutes ago"
        );
        assert_eq!(
            from_now(now - chrono::Duration::hours(3), now),
            "3 hours ago"
        );
    }

    #[test]
    fn the_resolve_dialog_strings_are_the_official_ones() {
        let alert = resolve_alert(1, 2);
        assert_eq!(alert.title, "What happened?");
        let labels: Vec<&str> = alert.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Cancel", "Mark as resolved"]);
        assert_eq!(
            dc_intl::t(RESOLUTIONS[0].1),
            "Increased activity from normal members. No action needed."
        );
    }

    #[test]
    fn automod_alert_completed_and_action_request_follow_485304() {
        let base = Message::from_value(&serde_json::json!({
            "id": "1",
            "channel_id": "2",
            "author": { "id": "3", "username": "automod" },
            "timestamp": "2026-11-21T00:00:00.000Z",
            "content": "",
            "embeds": []
        }))
        .unwrap();
        assert!(!alert_is_completed(&base));
        let done = Message::from_value(&serde_json::json!({
            "id": "1",
            "channel_id": "2",
            "author": { "id": "3", "username": "automod" },
            "timestamp": "2026-11-21T00:00:00.000Z",
            "content": "",
            "embeds": [{
                "type": "auto_moderation_notification",
                "fields": [{
                    "name": "alert_actions_execution",
                    "value": "{\"actions\":{\"1\":{\"completed_at\":\"2026-11-21T00:00:00Z\"}}}"
                }]
            }]
        }))
        .unwrap();
        assert!(alert_is_completed(&done));
        let crate::net::Request::Rest { route, body, .. } = alert_action_request(10, 20, 30, true)
        else {
            panic!("expected REST request");
        };
        assert_eq!(route, "/guilds/10/auto-moderation/alert-action");
        assert_eq!(body.unwrap()["alert_action_type"], 2);
        let crate::net::Request::Rest { body, .. } = alert_action_request(10, 20, 30, false) else {
            panic!("expected REST request");
        };
        assert_eq!(body.unwrap()["alert_action_type"], 1);

        // Module 794267: AutoMod false-positive feedback modal & REST action type 4.
        let alert = feedback_alert(10, 20, 30);
        assert_eq!(alert.title, "Did we get it wrong?");
        assert_eq!(alert.buttons.len(), 2);
        assert_eq!(alert.buttons[0].label, "Cancel");
        assert_eq!(alert.buttons[1].label, "Yes");
        let crate::net::Request::Rest { route, body, .. } = submit_feedback_request(10, 20, 30)
        else {
            panic!("expected REST request");
        };
        assert_eq!(route, "/guilds/10/auto-moderation/alert-action");
        let payload = body.unwrap();
        assert_eq!(payload["alert_action_type"], 4);
        assert_eq!(payload["message_id"], "30");
        assert_eq!(payload["channel_id"], "20");
    }
}
