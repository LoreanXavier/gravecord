//! Notification Center Inbox rows, rich message summaries, and quick-action bar (module 617823).

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum NotificationContentKind {
    Plain(String),
    Forwarded,
    PollClosed,
    Gif,
    Images(usize),
    Sticker(String),
    VoiceMessage,
    MemberJoined,
    Empty,
}

/// Official `617823` message preview summarizer for Notification Center rows.
pub(crate) fn notification_summary(kind: &NotificationContentKind) -> String {
    match kind {
        NotificationContentKind::Plain(text) if !text.trim().is_empty() => text.clone(),
        NotificationContentKind::Forwarded => dc_intl::t("9ddYKt").to_owned(),
        NotificationContentKind::PollClosed => dc_intl::t("sad2PH").to_owned(),
        NotificationContentKind::Gif => dc_intl::t("p0oZmy").to_owned(),
        NotificationContentKind::Images(n) if *n > 1 => {
            dc_intl::format("rtfTKp", &[("count", Arg::Num(*n as f64))])
        }
        NotificationContentKind::Images(_) => dc_intl::t("tCcq5p").to_owned(),
        NotificationContentKind::Sticker(name) => {
            dc_intl::format("zY4v1B", &[("stickerName", Arg::Str(Cow::Borrowed(name)))])
        }
        NotificationContentKind::VoiceMessage => dc_intl::t("slFYgi").to_owned(),
        NotificationContentKind::MemberJoined => dc_intl::t("Yvvfw9").to_owned(),
        NotificationContentKind::Plain(_) | NotificationContentKind::Empty => {
            dc_intl::t("sDqZHL").to_owned()
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NotificationRow {
    pub id: String,
    pub author: String,
    pub channel_name: Option<String>,
    pub guild_name: String,
    pub timestamp: String,
    pub is_thread: bool,
    pub bookmarked: bool,
    pub content: NotificationContentKind,
}

#[derive(Clone, Debug)]
pub(crate) struct NotificationCenterModal {
    pub items: Vec<NotificationRow>,
    pub last_action: Option<String>,
}

impl Root {
    pub(crate) fn close_notification_center(&mut self) -> bool {
        self.notification_center_modal.take().is_some()
    }

    pub(in crate::ui) fn notification_center_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.notification_center_modal.as_ref()?;
        let body: AnyElement =
            if modal.items.is_empty() {
                div()
                    .py(px(56.))
                    .px(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(icon("circle_check", 40., "--status-positive"))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("6XMM+D")),
                    )
                    .into_any_element()
            } else {
                div()
                    .id("notif-center-list")
                    .max_h(px(460.))
                    .overflow_y_scroll()
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(modal.items.iter().enumerate().map(|(idx, item)| {
                        let ch_label = item
                            .channel_name
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| dc_intl::t("J90oLW"));
                        let summary = notification_summary(&item.content);
                        let bookmark_tip = if item.bookmarked {
                            dc_intl::t("SvXS1Z")
                        } else {
                            dc_intl::t("9p3D9p")
                        };
                        let actions_tip = if item.is_thread {
                            dc_intl::t("1NBjqb")
                        } else {
                            dc_intl::t("Xm41aV")
                        };
                        let row_id = item.id.clone();
                        let row_id_bm = item.id.clone();

                        div()
                        .p(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lower"))
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
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(format!("#{ch_label}")),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .child(format!("· {}", item.guild_name)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .child(self.with_tip(
                                            format!("notif-bm-tip-{idx}"),
                                            super::tooltip::Tip::text(
                                                bookmark_tip,
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id(SharedString::from(format!("notif-bm-{idx}")))
                                                .px(px(6.))
                                                .py(px(3.))
                                                .rounded(px(6.))
                                                .bg(color("--control-secondary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-secondary-background-hover",
                                                    ))
                                                })
                                                .cursor_pointer()
                                                .text_size(px(11.))
                                                .text_color(color("--text-strong"))
                                                .child(bookmark_tip)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.notification_center_modal.as_mut()
                                                        && let Some(r) = m
                                                            .items
                                                            .iter_mut()
                                                            .find(|x| x.id == row_id_bm)
                                                    {
                                                        r.bookmarked = !r.bookmarked;
                                                    }
                                                    cx.notify();
                                                })),
                                            cx,
                                        ))
                                        .child(self.with_tip(
                                            format!("notif-rem-tip-{idx}"),
                                            super::tooltip::Tip::text(
                                                dc_intl::t("mJ3P0N"),
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id(SharedString::from(format!("notif-rem-{idx}")))
                                                .px(px(6.))
                                                .py(px(3.))
                                                .rounded(px(6.))
                                                .bg(color("--control-secondary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-secondary-background-hover",
                                                    ))
                                                })
                                                .cursor_pointer()
                                                .text_size(px(11.))
                                                .text_color(color("--text-default"))
                                                .child(dc_intl::t("mJ3P0N"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.notification_center_modal.as_mut()
                                                    {
                                                        m.last_action =
                                                            Some(dc_intl::t("mJ3P0N").to_owned());
                                                    }
                                                    cx.notify();
                                                })),
                                            cx,
                                        ))
                                        .child(self.with_tip(
                                            format!("notif-act-tip-{idx}"),
                                            super::tooltip::Tip::text(
                                                actions_tip,
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id(SharedString::from(format!("notif-act-{idx}")))
                                                .size(px(24.))
                                                .rounded(px(6.))
                                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "more_horizontal",
                                                    14.,
                                                    "--interactive-icon-default",
                                                )),
                                            cx,
                                        ))
                                        .child(self.with_tip(
                                            format!("notif-read-tip-{idx}"),
                                            super::tooltip::Tip::text(
                                                dc_intl::t("e6RscS"),
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id(SharedString::from(format!("notif-read-{idx}")))
                                                .size(px(24.))
                                                .rounded(px(6.))
                                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "checkbox_check",
                                                    14.,
                                                    "--interactive-icon-default",
                                                ))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.notification_center_modal.as_mut()
                                                    {
                                                        m.items.retain(|x| x.id != row_id);
                                                    }
                                                    cx.notify();
                                                })),
                                            cx,
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(item.author.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(color("--text-muted"))
                                        .child(item.timestamp.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(19.))
                                .text_color(color("--text-default"))
                                .child(summary),
                        )
                    }))
                    .into_any_element()
            };

        let dialog = div()
            .id("notif-center-modal")
            .occlude()
            .w(px(560.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr("Inbox")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("notif-center-settings")
                                    .px(px(10.))
                                    .py(px(5.))
                                    .rounded(px(6.))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("h850Ss"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_notification_center();
                                        this.open_settings_section("Notifications", cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("notif-center-close")
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 16., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_notification_center();
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(body);

        Some(
            deferred(
                div()
                    .id("notif-center-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_notification_center();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_notification_center(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let items = if arg == "empty" {
            Vec::new()
        } else {
            vec![
                NotificationRow {
                    id: "n1".to_owned(),
                    author: "lorean xavier".to_owned(),
                    channel_name: Some("general".to_owned()),
                    guild_name: "gravecord fixture".to_owned(),
                    timestamp: "Today at 12:10 PM".to_owned(),
                    is_thread: false,
                    bookmarked: true,
                    content: NotificationContentKind::Forwarded,
                },
                NotificationRow {
                    id: "n2".to_owned(),
                    author: "onugeccen".to_owned(),
                    channel_name: Some("design-review".to_owned()),
                    guild_name: "gravecord fixture".to_owned(),
                    timestamp: "Today at 11:42 AM".to_owned(),
                    is_thread: true,
                    bookmarked: false,
                    content: NotificationContentKind::Images(3),
                },
                NotificationRow {
                    id: "n3".to_owned(),
                    author: "Wren".to_owned(),
                    channel_name: Some("polls".to_owned()),
                    guild_name: "gravecord fixture".to_owned(),
                    timestamp: "Yesterday".to_owned(),
                    is_thread: false,
                    bookmarked: false,
                    content: NotificationContentKind::PollClosed,
                },
                NotificationRow {
                    id: "n4".to_owned(),
                    author: "Wumpus".to_owned(),
                    channel_name: None,
                    guild_name: "gravecord fixture".to_owned(),
                    timestamp: "Yesterday".to_owned(),
                    is_thread: false,
                    bookmarked: false,
                    content: NotificationContentKind::Sticker("Wave".to_owned()),
                },
            ]
        };
        self.notification_center_modal = Some(NotificationCenterModal {
            items,
            last_action: None,
        });
        cx.notify();
        format!("notification-center: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_summary_covers_all_617823_kinds() {
        assert_eq!(
            notification_summary(&NotificationContentKind::Forwarded),
            "Forwarded a message"
        );
        assert_eq!(
            notification_summary(&NotificationContentKind::PollClosed),
            "Poll has closed"
        );
        assert_eq!(
            notification_summary(&NotificationContentKind::Gif),
            "Sent a GIF"
        );
        assert_eq!(
            notification_summary(&NotificationContentKind::Images(1)),
            "Sent an image"
        );
        assert_eq!(
            notification_summary(&NotificationContentKind::Images(4)),
            "Sent 4 images"
        );
        assert_eq!(
            notification_summary(&NotificationContentKind::Sticker("Party".to_owned())),
            "Sent a sticker: Party"
        );
        assert_eq!(
            notification_summary(&NotificationContentKind::VoiceMessage),
            "Sent a voice message"
        );
        assert_eq!(
            notification_summary(&NotificationContentKind::MemberJoined),
            "Joined the server!"
        );
        assert_eq!(
            notification_summary(&NotificationContentKind::Empty),
            "Sent a message"
        );
    }
}
