//! Member Verification Preview Disabled / Join Request applicant status page (module 840336):
//! Incomplete (`b`), Submitted (`P`), Rejected (`V`), and Withdraw/Discard confirmation dialog (`E`).

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum JoinRequestStatus {
    Incomplete,
    Submitted,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum JoinConfirmKind {
    Withdraw,
    Discard,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct JoinRequestModal {
    pub guild_id: Option<dc_state::Id>,
    pub guild_name: Option<String>,
    pub status: JoinRequestStatus,
    pub rejection_reason: Option<String>,
    pub can_reapply: bool,
    pub confirm: Option<JoinConfirmKind>,
}

/// Format bold-tagged `$b` runs as plain text segments for inline confirmation subtitles.
fn format_bold_runs(key: &str, args: &[(&str, Arg<'_>)]) -> Vec<(String, bool)> {
    dc_intl::format_runs(key, args)
        .into_iter()
        .map(|(text, hook)| (text, hook.as_deref() == Some("$b")))
        .collect()
}

impl Root {
    pub(crate) fn close_join_request_modal(&mut self) -> bool {
        self.join_request_modal.take().is_some()
    }

    pub(in crate::ui) fn join_request_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.join_request_modal.as_ref()?;
        let guild_name = modal.guild_name.as_deref();
        let can_reapply = modal.can_reapply;

        // If a Withdraw / Discard confirmation is open, render official `E` dialog above.
        if let Some(kind) = modal.confirm {
            let name_arg = guild_name.unwrap_or("this server");
            let subtitle_runs = match kind {
                JoinConfirmKind::Withdraw => {
                    format_bold_runs("9ZezpN", &[("name", Arg::Str(Cow::Borrowed(name_arg)))])
                }
                JoinConfirmKind::Discard => {
                    vec![(dc_intl::t("fJwWVt").to_owned(), false)]
                }
            };
            let confirm_dialog = div()
                .id("join-req-confirm")
                .occlude()
                .w(px(400.))
                .rounded(px(16.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .p(px(24.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("y0CVes")),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .children(subtitle_runs.into_iter().map(|(chunk, bold)| {
                            div()
                                .when(bold, |d| {
                                    d.font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                })
                                .child(chunk)
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("join-req-confirm-cancel")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("oEAioF"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.join_request_modal.as_mut() {
                                        m.confirm = None;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("join-req-confirm-yes")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-critical-primary-background-default"))
                                .hover(|s| {
                                    s.bg(color("--control-critical-primary-background-hover"))
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("p89ACt"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(gid) =
                                        this.join_request_modal.as_ref().and_then(|m| m.guild_id)
                                    {
                                        this.remove_join_request(gid);
                                    }
                                    this.close_join_request_modal();
                                    cx.notify();
                                })),
                        ),
                );
            return Some(
                deferred(
                    div()
                        .id("join-req-confirm-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b8))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(m) = this.join_request_modal.as_mut() {
                                m.confirm = None;
                            }
                            cx.notify();
                        }))
                        .child(confirm_dialog),
                )
                .into_any_element(),
            );
        }

        let (badge_icon, heading, sub_el, buttons_el): (
            &'static str,
            String,
            Option<AnyElement>,
            AnyElement,
        ) = match modal.status {
            JoinRequestStatus::Incomplete => {
                let title = match guild_name {
                    Some(name) => {
                        dc_intl::format("f5Jaw7", &[("guildName", Arg::Str(Cow::Borrowed(name)))])
                    }
                    None => dc_intl::t("0sTyEb").to_owned(),
                };
                let btns = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("join-req-open")
                            .w_full()
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("h3aGmv"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_join_request_modal();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("join-req-discard")
                            .w_full()
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(color("--control-critical-primary-background-default"))
                            .hover(|s| s.bg(color("--control-critical-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("OQFlFD"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(m) = this.join_request_modal.as_mut() {
                                    m.confirm = Some(JoinConfirmKind::Discard);
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element();
                ("access_list", title, None, btns)
            }
            JoinRequestStatus::Submitted => {
                let title = match guild_name {
                    Some(name) => {
                        dc_intl::format("0Qlu/6", &[("guildName", Arg::Str(Cow::Borrowed(name)))])
                    }
                    None => dc_intl::t("5iLvSx").to_owned(),
                };
                let sub = div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .text_center()
                    .child(dc_intl::t("FdsK4h"))
                    .into_any_element();
                let btns = div()
                    .id("join-req-withdraw-sub")
                    .w_full()
                    .h(px(38.))
                    .rounded(px(8.))
                    .bg(color("--control-critical-primary-background-default"))
                    .hover(|s| s.bg(color("--control-critical-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("g9tK0o"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(m) = this.join_request_modal.as_mut() {
                            m.confirm = Some(JoinConfirmKind::Withdraw);
                        }
                        cx.notify();
                    }))
                    .into_any_element();
                ("checklist_pending", title, Some(sub), btns)
            }
            JoinRequestStatus::Rejected => {
                let title = match guild_name {
                    Some(name) => {
                        dc_intl::format("P+/gzA", &[("guildName", Arg::Str(Cow::Borrowed(name)))])
                    }
                    None => dc_intl::t("gBPcuP").to_owned(),
                };
                let sub = modal
                    .rejection_reason
                    .as_deref()
                    .filter(|r| !r.is_empty())
                    .map(|reason| {
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .text_center()
                            .child(SharedString::from(format!(
                                "{} {reason}",
                                dc_intl::t("cf1psW")
                            )))
                            .into_any_element()
                    });
                let btns = div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .when(can_reapply, |row| {
                        row.child(
                            div()
                                .id("join-req-reapply")
                                .flex_1()
                                .h(px(38.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("I1LYVk"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(gid) =
                                        this.join_request_modal.as_ref().and_then(|m| m.guild_id)
                                    {
                                        this.reapply(gid);
                                    }
                                    if let Some(m) = this.join_request_modal.as_mut() {
                                        m.status = JoinRequestStatus::Incomplete;
                                        m.rejection_reason = None;
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .id("join-req-withdraw-rej")
                            .flex_1()
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(color("--control-critical-primary-background-default"))
                            .hover(|s| s.bg(color("--control-critical-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("g9tK0o"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if can_reapply {
                                    if let Some(m) = this.join_request_modal.as_mut() {
                                        m.confirm = Some(JoinConfirmKind::Withdraw);
                                    }
                                } else {
                                    this.close_join_request_modal();
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element();
                ("close", title, sub, btns)
            }
        };

        // Official `.confirmation_Br3ZUd`: max-width 400px, padding 24px, gap 24px, radius 16px.
        let card = div()
            .id("join-request-card")
            .occlude()
            .w(px(400.))
            .rounded(px(16.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .p(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(24.))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.35),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .size(px(48.))
                    .rounded_full()
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(badge_icon, 24., "--interactive-text-active")),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(26.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .text_center()
                            .child(heading),
                    )
                    .when_some(sub_el, |col, el| col.child(el)),
            )
            .child(buttons_el);

        Some(
            deferred(
                div()
                    .id("join-request-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_join_request_modal();
                        cx.notify();
                    }))
                    .child(card),
            )
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_join_request_page(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let (status, reason, confirm) = match arg {
            "submitted" => (JoinRequestStatus::Submitted, None, None),
            "incomplete" => (JoinRequestStatus::Incomplete, None, None),
            "withdraw" => (
                JoinRequestStatus::Submitted,
                None,
                Some(JoinConfirmKind::Withdraw),
            ),
            "discard" => (
                JoinRequestStatus::Incomplete,
                None,
                Some(JoinConfirmKind::Discard),
            ),
            _ => (
                JoinRequestStatus::Rejected,
                Some("Please include your GitHub handle in the introduction field.".to_owned()),
                None,
            ),
        };
        self.join_request_modal = Some(JoinRequestModal {
            guild_id: self.guild.or(Some(100)),
            guild_name: Some("gravecord fixture".to_owned()),
            status,
            rejection_reason: reason,
            can_reapply: true,
            confirm,
        });
        cx.notify();
        format!("join-request: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_17_keys_of_join_request_page_840336_resolve() {
        for key in [
            "0Qlu/6", "0sTyEb", "5iLvSx", "9ZezpN", "FdsK4h", "I1LYVk", "OQFlFD", "P+/gzA",
            "cf1psW", "f5Jaw7", "fJwWVt", "g9tK0o", "gBPcuP", "h3aGmv", "oEAioF", "p89ACt",
            "y0CVes",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
        let runs = format_bold_runs("9ZezpN", &[("name", Arg::Str(Cow::Borrowed("Guild")))]);
        assert!(runs.iter().any(|(t, bold)| t == "Guild" && *bold));
    }
}
