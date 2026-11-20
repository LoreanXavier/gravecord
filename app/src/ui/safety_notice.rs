//! The safety notices of a system DM, drawn in place of their embed (`renderSafetySystemNotification` of module 679636:
//! a message whose only embed is `safety_system_notification` gets `sq`). The fields of the embed carry the notice
//! (module 393033 `f4`, names from module 239093 `g0`); the look is `safetyPolicyNoticeContainer_LQmP0J` and its
//! children (module 975430, stylesheet 10886).

use super::{Root, color, icon};
use dc_state::{Embed, Message};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};

/// `SAFETY_SYSTEM_NOTIFICATION` of the embed types (module 508602).
const KIND: &str = "safety_system_notification";

/// `2026-09-safety-warning-age-text`: in variation 1, a notice older than thirty days reads ">30 days ago".
const AGE_TEXT_EXPERIMENT: &str = "2026-09-safety-warning-age-text";

/// What `f4` of module 393033 reads from the embed's fields (`rawName` -> `rawValue`).
#[derive(Debug, Default, PartialEq)]
pub(super) struct Notice {
    pub header: String,
    /// `icon_type`: `default` (the announcements icon) or `danger` (the warning icon); anything else draws none.
    pub icon: Option<String>,
    pub body: String,
    /// `ctas`, split on commas, empty entries dropped.
    pub ctas: Vec<String>,
    /// `timestamp`: Unix seconds (`parseFloat(... ?? 0)`).
    pub timestamp: f64,
    /// `theme`: `default` or `danger` (the footer's look).
    pub theme: Option<String>,
    pub learn_more_link: Option<String>,
    pub classification_id: Option<String>,
}

/// `renderSafetySystemNotification`: exactly one embed, of the notification type, with fields.
pub(super) fn notice_of(message: &Message) -> Option<Notice> {
    match message.embeds.as_slice() {
        [embed] if embed.kind == KIND => parse(embed),
        _ => None,
    }
}

/// `f4`: the fields by name (a later field of the same name wins, as the reduce does).
fn parse(embed: &Embed) -> Option<Notice> {
    if embed.fields.is_empty() {
        return None;
    }
    let field = |name: &str| {
        embed
            .fields
            .iter()
            .rev()
            .find(|field| field.name == name)
            .map(|field| field.value.clone())
    };
    Some(Notice {
        header: field("header").unwrap_or_default(),
        icon: field("icon_type"),
        body: field("body").unwrap_or_default(),
        ctas: field("ctas")
            .unwrap_or_default()
            .split(',')
            .filter(|cta| !cta.is_empty())
            .map(str::to_owned)
            .collect(),
        timestamp: field("timestamp")
            .and_then(|value| parse_float(&value))
            .unwrap_or(0.),
        theme: field("theme"),
        learn_more_link: field("learn_more_link"),
        classification_id: field("classification_id"),
    })
}

/// `parseFloat`: the longest leading decimal number, so "1731000000.5abc" is 1731000000.5.
fn parse_float(value: &str) -> Option<f64> {
    let value = value.trim_start();
    let end = value
        .char_indices()
        .take_while(|(index, ch)| {
            ch.is_ascii_digit() || *ch == '.' || (*index == 0 && (*ch == '-' || *ch == '+'))
        })
        .map(|(index, ch)| index + ch.len_utf8())
        .last()?;
    (1..=end)
        .rev()
        .find_map(|cut| value[..cut].parse::<f64>().ok())
}

/// `sO`: whole days since the notice (`moment().diff(moment.unix(timestamp), "days")`, cut toward zero).
pub(super) fn days_ago(timestamp: f64, now_ms: i64) -> i64 {
    let elapsed = now_ms as f64 - timestamp * 1000.;
    (elapsed / 86_400_000.).trunc() as i64
}

/// `sG`: ">30 days ago" (`8qYr4l`) past thirty days when the experiment is on, else `eevFb6` (Today, Yesterday,
/// {n} days ago).
pub(super) fn age_text(days: i64, age_text_on: bool) -> String {
    if age_text_on && days > 30 {
        dc_intl::t("8qYr4l").to_owned()
    } else {
        dc_intl::t!("eevFb6", daysAgo = days)
    }
}

impl Root {
    /// `sq` of module 679636: the notice's card, last among the message's accessories.
    pub(super) fn safety_system_notice(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let notice = notice_of(message)?;
        let age_text_on = self.stores.experiments.variant(AGE_TEXT_EXPERIMENT) == Some(1);
        let when = age_text(
            days_ago(notice.timestamp, chrono::Utc::now().timestamp_millis()),
            age_text_on,
        );
        // `sZ`: `AnnouncementsIcon` at `xs` (12px) in `TEXT_LINK`, or `WarningIcon` (`md`, 24px) in
        // `ICON_FEEDBACK_CRITICAL`, inside `icon_LQmP0J` (14px square, 4px before the header).
        let glyph = match notice.icon.as_deref() {
            Some("default") => Some(icon("ob_announce", 12., "--text-link")),
            Some("danger") => Some(icon("warning_triangle", 24., "--icon-feedback-critical")),
            _ => None,
        };
        let danger = notice.theme.as_deref() == Some("danger");
        let id = message.id;
        let ctas: Vec<AnyElement> = notice
            .ctas
            .iter()
            .enumerate()
            .filter_map(|(index, cta)| {
                let (label, link, open_tools) = match cta.as_str() {
                    "policy_violation_detail" => {
                        notice.classification_id.as_ref()?;
                        (dc_intl::t("QsqdXC"), None, false)
                    }
                    "learn_more_link" => (
                        dc_intl::t("8/GdRB"),
                        Some(notice.learn_more_link.clone()?),
                        false,
                    ),
                    "safety_tools" => (dc_intl::t("eXlt01"), None, true),
                    _ => return None,
                };
                let channel_id = self.channel.unwrap_or(0);
                let author_id = message.author.id;
                // `detailsButton_LQmP0J` (`margin-inline: 8px`, `padding: 8px 0`) on an `Anchor` (`--text-link`,
                // underlined under the pointer) in the body's 16px with `line-height: 1`.
                let anchor = div()
                    .id(SharedString::from(format!(
                        "safety-notice-{id}-cta-{index}"
                    )))
                    .mx(px(8.))
                    .py(px(8.))
                    .text_size(px(16.))
                    .line_height(px(16.))
                    .text_color(color("--text-link"))
                    .cursor_pointer()
                    .hover(|style| style.underline())
                    .child(label);
                Some(match (link, open_tools) {
                    (Some(url), _) => anchor
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.follow_link(&url, &url, cx);
                        }))
                        .into_any_element(),
                    (None, true) => anchor
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.open_safety_tools(channel_id, author_id, cx);
                        }))
                        .into_any_element(),
                    (None, false) => {
                        let classification = notice
                            .classification_id
                            .clone()
                            .unwrap_or_else(|| "Community Guidelines".to_owned());
                        let explainer = notice
                            .learn_more_link
                            .clone()
                            .unwrap_or_else(|| "https://discord.com/guidelines".to_owned());
                        anchor
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.violation_detail_modal =
                                    Some(super::violation_detail::ViolationDetailModal {
                                        classification: classification.clone(),
                                        guild_name: None,
                                        origin: super::violation_detail::ViolationOrigin::User,
                                        actions: Vec::new(),
                                        expiration_date: None,
                                        explainer_url: explainer.clone(),
                                        appeal:
                                            super::violation_detail::ViolationAppealState::Eligible,
                                    });
                                cx.notify();
                            }))
                            .into_any_element()
                    }
                })
            })
            .collect();
        // `sX`: `defaultFooterContainer_LQmP0J` (`--background-mod-normal`, its rows from the start, lighter under the
        // pointer) or `dangerFooterContainer_LQmP0J` (`--control-critical-primary-background-default`, centred; its
        // hover rule is spelled `dangerfooterContainer` and never applies); both round the card's bottom corners.
        let footer = div()
            .id(SharedString::from(format!("safety-notice-{id}-footer")))
            .w_full()
            .flex()
            .flex_row()
            .rounded_b(px(8.))
            .text_color(color("--white"))
            .when(danger, |this| {
                this.items_start()
                    .justify_center()
                    .bg(color("--control-critical-primary-background-default"))
            })
            .when(!danger, |this| {
                this.items_center()
                    .justify_start()
                    .bg(color("--background-mod-normal"))
                    .hover(|style| style.bg(color("--control-secondary-background-hover")))
            })
            .children(ctas);
        Some(
            div()
                .id(SharedString::from(format!("safety-notice-{id}")))
                .flex()
                .flex_col()
                .rounded(px(8.))
                .bg(color("--card-background-default"))
                .child(
                    // `noticeContent_LQmP0J`: 12px around a column.
                    div()
                        .p(px(12.))
                        .flex()
                        .flex_col()
                        .items_start()
                        .child(
                            // `headerRow_LQmP0J` with `body-md/semibold` (16px / 20px) in `--text-default`.
                            div()
                                .flex()
                                .items_center()
                                .children(glyph.map(|glyph| {
                                    div().flex_none().size(px(14.)).mr(px(4.)).child(glyph)
                                }))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-default"))
                                        .child(notice.header),
                                ),
                        )
                        .child(
                            // `incidentTiming_LQmP0J`: 4px down; the `body-xs/medium` text keeps its own
                            // `--text-default` (`defaultColor_MMUIw6`).
                            div()
                                .mt(px(4.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(when),
                        )
                        .child(
                            // `noticeBody_LQmP0J`: 8px down, `body-md/normal` in `text-muted`, `pre-line`.
                            div()
                                .mt(px(8.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child(pre_line(&notice.body)),
                        ),
                )
                .child(footer)
                .into_any_element(),
        )
    }
}

/// `white-space: pre-line`: runs of spaces and tabs collapse to one, line breaks stay.
fn pre_line(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            line.split([' ', '\t'])
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(embeds: serde_json::Value) -> Message {
        let mut message = Message::from_value(&json!({
            "id": "5", "channel_id": "6", "type": 0, "content": "",
            "timestamp": "2026-11-18T20:01:00+00:00",
            "author": {"id": "643945264868098049", "username": "discord", "system": true, "bot": true},
            "embeds": embeds,
        }))
        .unwrap();
        message.channel_id = 6;
        message
    }

    #[test]
    fn the_notice_reads_its_fields_by_name() {
        let message = message(json!([{"type": "safety_system_notification", "fields": [
            {"name": "icon_type", "value": "default"},
            {"name": "header", "value": "We removed a violation from your account"},
            {"name": "body", "value": "We reviewed a violation."},
            {"name": "ctas", "value": "learn_more_link,"},
            {"name": "timestamp", "value": "1763496000"},
            {"name": "theme", "value": "default"},
            {"name": "learn_more_link", "value": "https://support.discord.com/hc/articles/1"},
        ]}]));
        let notice = notice_of(&message).unwrap();
        assert_eq!(notice.header, "We removed a violation from your account");
        assert_eq!(notice.icon.as_deref(), Some("default"));
        assert_eq!(notice.ctas, vec!["learn_more_link".to_owned()]);
        assert_eq!(notice.timestamp, 1_763_496_000.);
        assert_eq!(notice.theme.as_deref(), Some("default"));
        assert!(notice.classification_id.is_none());
    }

    #[test]
    fn only_a_lone_notification_embed_is_a_notice() {
        assert!(
            notice_of(&message(
                json!([{"type": "rich", "fields": [{"name": "header", "value": "x"}]}])
            ))
            .is_none()
        );
        assert!(notice_of(&message(json!([{"type": "safety_system_notification"}]))).is_none());
        let two = json!([
            {"type": "safety_system_notification", "fields": [{"name": "header", "value": "x"}]},
            {"type": "safety_system_notification", "fields": [{"name": "header", "value": "y"}]},
        ]);
        assert!(notice_of(&message(two)).is_none());
    }

    #[test]
    fn the_age_counts_whole_days_and_reads_as_the_plural_says() {
        let now = 1_763_500_000_000_i64;
        assert_eq!(days_ago(1_763_496_000., now), 0);
        assert_eq!(days_ago(1_763_496_000. - 86_400., now), 1);
        assert_eq!(days_ago(0., now), 20_410);
        assert_eq!(age_text(0, false), "Today");
        assert_eq!(age_text(1, false), "Yesterday");
        assert_eq!(age_text(5, false), "5 days ago");
        assert_eq!(age_text(31, false), "31 days ago");
        assert_eq!(age_text(31, true), ">30 days ago");
        assert_eq!(age_text(30, true), "30 days ago");
    }

    #[test]
    fn numbers_and_text_follow_the_browser() {
        assert_eq!(parse_float("1763496000.5abc"), Some(1_763_496_000.5));
        assert_eq!(parse_float("abc"), None);
        assert_eq!(pre_line("a  b\n c\t\td"), "a b\nc d");
    }
}
