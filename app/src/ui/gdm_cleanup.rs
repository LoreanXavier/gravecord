//! Clean Up Inactive Group DMs (module 877760), opened from the Direct Messages header's More button (`j` of module
//! 830507: the `clean-up-inactive-gdms` menu with "Clean up Group DMs...").
//!
//! The candidates are taken once when it opens: every group DM that is not a message request or spam whose last
//! activity (the last message's time, else its id's, else the channel's) is over 180 days old, oldest first, at most
//! 5000; the first 1000 start selected. The dialog (module 189213) has "Clean Up Inactive Group DMs" and its subtitle,
//! the list controls (`listControls_mVhsM9`: the Sort trigger, `body-sm/medium` in `--text-subtle`, `--text-strong` on
//! hover, with the up-down arrows; the text button Select All / Unselect All / Select 1,000), the 480 high list of
//! 48px rows (module 359378's `destinationRow_dhbHaH`: the 32px group icon 12px from the name, `body-md/semibold`, and
//! "N Members" in `body-xs/normal` `text-muted`, the checkbox 16px after; at 1000 selected the others are disabled),
//! and Leave Group DMs Silently / Leave N Group DMs Silently (`critical-primary`, loading while it runs) which posts
//! `POST /users/@me/channels/bulk-leave {channel_ids}`: a 409, a 429 (with `retry_after`) and other errors keep the
//! dialog with a critical toast; success toasts "Left N Group DMs" and closes. With no candidates: the empty state
//! (`emptyState_mVhsM9`) and Cancel.
//! GAP: the group icon is the channel's own picture or default (module 598104 draws a two-avatar facepile for an
//! iconless group), and the screen reader announcements are not made.

use super::tooltip::{Side, Tip};
use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px, svg,
};
use std::collections::BTreeSet;

/// `t` of the candidate finder: group DMs quiet this many days.
const INACTIVE_DAYS: i64 = 180;
/// `e`: at most this many candidates.
const MAX_CANDIDATES: usize = 5000;
/// The most that can be selected (`1e3`).
pub(super) const MAX_SELECTED: usize = 1000;
const LEAVE_KEY: &str = "gdm-cleanup:leave";

pub(super) struct GdmCleanup {
    /// `(channel, last activity in ms)`, oldest first.
    candidates: Vec<(Id, i64)>,
    pub(super) newest_first: bool,
    selected: BTreeSet<Id>,
    leaving: bool,
}

/// The snowflake's time in ms.
fn snowflake_ms(id: Id) -> i64 {
    ((id >> 22) as i64) + 1_420_070_400_000
}

/// `lastMessageTimestamp ?? lastMessageId ?? channel.lastMessageId ?? channel.id` of module 877760, in ms: the read
/// state's newest message (moved by every MESSAGE_CREATE), else the channel's own last message, else its creation.
pub(super) fn last_activity(latest: Option<Id>, last_message_id: Id, channel: Id) -> i64 {
    let last = latest
        .filter(|id| *id > 0)
        .or((last_message_id > 0).then_some(last_message_id))
        .unwrap_or(channel);
    snowflake_ms(last)
}

/// The candidates of module 877760: `(channel, last activity)` of the quiet group DMs, oldest first.
pub(super) fn candidates(channels: impl Iterator<Item = (Id, i64)>, now_ms: i64) -> Vec<(Id, i64)> {
    let cutoff = now_ms - INACTIVE_DAYS * 24 * 60 * 60 * 1000;
    let mut found: Vec<(Id, i64)> = channels.filter(|(_, last)| *last < cutoff).collect();
    found.sort_by_key(|(id, last)| (*last, *id));
    found.truncate(MAX_CANDIDATES);
    found
}

/// The button's text: Leave Group DMs Silently with none selected, else Leave N Group DM(s) Silently.
fn leave_label(count: usize) -> String {
    if count == 0 {
        dc_intl::t("YBjUwi").to_owned()
    } else {
        dc_intl::t!("SakG2D", count = count).to_string()
    }
}

/// The control's text: Unselect All when all that can be are selected, Select All when every candidate fits, else
/// Select 1,000.
fn select_label(selected: usize, total: usize) -> String {
    let limit = total.min(MAX_SELECTED);
    if limit > 0 && selected >= limit {
        dc_intl::t("jxi9GF").to_owned()
    } else if total <= MAX_SELECTED {
        dc_intl::t("mKxGpP").to_owned()
    } else {
        dc_intl::t!("9oRyjg", count = MAX_SELECTED).to_string()
    }
}

impl GdmCleanup {
    fn sorted(&self) -> Vec<(Id, i64)> {
        let mut list = self.candidates.clone();
        if self.newest_first {
            list.reverse();
        }
        list
    }
}

impl Root {
    pub(super) fn open_gdm_cleanup(&mut self, cx: &mut Context<Self>) {
        let now = chrono::Utc::now().timestamp_millis();
        let found = candidates(
            self.stores
                .private_channels
                .iter()
                .filter(|channel| channel.kind == 3 && !channel.message_request && !channel.spam)
                .map(|channel| {
                    let latest = self.stores.read_states.latest.get(&channel.id).copied();
                    (
                        channel.id,
                        last_activity(latest, channel.last_message_id, channel.id),
                    )
                }),
            now,
        );
        let selected = found.iter().take(MAX_SELECTED).map(|(id, _)| *id).collect();
        self.gdm_cleanup = Some(GdmCleanup {
            candidates: found,
            newest_first: false,
            selected,
            leaving: false,
        });
        self.request_group_images();
        cx.notify();
    }

    fn gdm_cleanup_toggle(&mut self, channel: Id, cx: &mut Context<Self>) {
        let Some(open) = self.gdm_cleanup.as_mut() else {
            return;
        };
        if !open.selected.remove(&channel) && open.selected.len() < MAX_SELECTED {
            open.selected.insert(channel);
        }
        cx.notify();
    }

    fn gdm_cleanup_select_all(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.gdm_cleanup.as_mut() else {
            return;
        };
        let limit = open.candidates.len().min(MAX_SELECTED);
        open.selected = if open.selected.len() >= limit {
            BTreeSet::new()
        } else {
            open.sorted()
                .into_iter()
                .take(MAX_SELECTED)
                .map(|(id, _)| id)
                .collect()
        };
        cx.notify();
    }

    fn gdm_cleanup_leave(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.gdm_cleanup.as_mut() else {
            return;
        };
        if open.selected.is_empty() || open.leaving {
            return;
        }
        open.leaving = true;
        let ids: Vec<String> = open.selected.iter().map(Id::to_string).collect();
        let _ = self.requests.send(Request::Rest {
            key: Some(LEAVE_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: "/users/@me/channels/bulk-leave".to_owned(),
            body: Some(serde_json::json!({ "channel_ids": ids })),
        });
        cx.notify();
    }

    /// The answer of the bulk leave.
    pub(super) fn gdm_cleanup_answered(
        &mut self,
        key: &str,
        report: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if key != LEAVE_KEY {
            return false;
        }
        let Some(open) = self.gdm_cleanup.as_mut() else {
            return true;
        };
        if report["ok"].as_bool() == Some(true) {
            let count = open.selected.len();
            self.gdm_cleanup = None;
            self.put_app_toast(dc_intl::t!("b2r81u", count = count).to_string(), true, cx);
            return true;
        }
        open.leaving = false;
        let message = match report["status"].as_u64() {
            Some(409) => dc_intl::t("ojlzko").to_owned(),
            Some(429) => match report["body"]["retry_after"]
                .as_f64()
                .filter(|after| *after > 0.)
            {
                Some(after) => dc_intl::t!("SHTZTm", seconds = after.ceil() as u64).to_string(),
                None => dc_intl::t("f5Xg0V").to_owned(),
            },
            _ => dc_intl::t("+HM0x2").to_owned(),
        };
        self.put_app_toast(message, false, cx);
        true
    }

    fn gdm_cleanup_row(
        &self,
        channel: Id,
        at_limit: bool,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.gdm_cleanup.as_ref()?;
        let known = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)?;
        let selected = open.selected.contains(&channel);
        let disabled = at_limit && !selected;
        let name = self.private_channel_name(known);
        let members = known.recipients.len() + 1;
        let picture = self
            .images
            .get(&super::unread_dms::group_icon(known).0)
            .cloned();
        let avatar = match picture {
            Some(image) => img(image).size(px(32.)).rounded_full().into_any_element(),
            None => div()
                .size(px(32.))
                .rounded_full()
                .bg(color("--background-mod-normal"))
                .into_any_element(),
        };
        Some(
            div()
                .id(("gdm-cleanup-row", index))
                .min_h(px(48.))
                .mx(px(-8.))
                .px(px(8.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_between()
                .when(!disabled, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.gdm_cleanup_toggle(channel, cx)),
                        )
                })
                .when(disabled, |this| this.cursor_not_allowed())
                .child(
                    div()
                        .flex_shrink(1.)
                        .min_w(px(0.))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_none()
                                .when(disabled, |this| this.opacity(0.5))
                                .child(avatar),
                        )
                        .child(
                            div()
                                .my(px(4.))
                                .min_w(px(0.))
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color(if disabled {
                                            "--text-muted"
                                        } else {
                                            "--text-default"
                                        }))
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(
                                            dc_intl::t!("ABMKx3", members = members).to_string(),
                                        ),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .ml(px(16.))
                        .size(px(20.))
                        .when(disabled, |this| this.opacity(0.5))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(if selected {
                            color("--background-mod-subtle")
                        } else {
                            color("--checkbox-border-default")
                        })
                        .bg(if selected {
                            color("--control-primary-background-default")
                        } else {
                            color("--checkbox-background-default")
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected, |this| {
                            this.child(
                                svg()
                                    .path("icons/theme_check.svg")
                                    .size(px(18.))
                                    .text_color(color("--white")),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    pub(super) fn gdm_cleanup_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.gdm_cleanup.as_ref()?;
        let empty = open.candidates.is_empty();
        let count = open.selected.len();
        let at_limit = count >= MAX_SELECTED;
        let body: AnyElement = if empty {
            // `height: 480` capped by `max-height: 45vh`.
            div()
                .h(px(480f32.min(self.viewport.1 * 0.45)))
                .py(px(16.))
                .px(px(8.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .text_center()
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("s8TEkh")),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("Y//IPC")),
                )
                .into_any_element()
        } else {
            let rows: Vec<AnyElement> = open
                .sorted()
                .into_iter()
                .enumerate()
                .filter_map(|(index, (id, _))| self.gdm_cleanup_row(id, at_limit, index, cx))
                .collect();
            let controls = div()
                .flex()
                .items_center()
                .justify_between()
                .mb(px(8.))
                .child(
                    div()
                        .id("gdm-cleanup-sort")
                        .group("gdm-cleanup-sort")
                        .relative()
                        .child(self.anchor_probe("gdm-cleanup-sort"))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-subtle"))
                        .hover(|style| style.text_color(color("--text-strong")))
                        .child(dc_intl::t("FLgBG/"))
                        .child(
                            icon("members_sort", 12., "--text-subtle")
                                .group_hover("gdm-cleanup-sort", |style| {
                                    style.text_color(color("--text-strong"))
                                }),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let anchor = this.tip_anchors.borrow().get("gdm-cleanup-sort").copied();
                            if let Some(anchor) = anchor {
                                this.open_context_menu(
                                    super::context_menu::Target::GdmCleanupSort,
                                    gpui::point(anchor.left(), anchor.bottom() + px(8.)),
                                    cx,
                                );
                            }
                        })),
                )
                .child(
                    div()
                        .id("gdm-cleanup-select")
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-brand"))
                        .hover(|style| style.underline())
                        .child(select_label(count, open.candidates.len()))
                        .on_click(cx.listener(|this, _, _, cx| this.gdm_cleanup_select_all(cx))),
                );
            div()
                .flex()
                .flex_col()
                .child(controls)
                .child(
                    div()
                        .id("gdm-cleanup-list")
                        .h(px(480.))
                        .overflow_y_scroll()
                        .pb(px(16.))
                        .px(px(8.))
                        .children(rows),
                )
                .into_any_element()
        };
        let buttons = if empty {
            vec![super::modal::button(
                "gdm-cleanup-cancel",
                dc_intl::t("ETE/oC"),
                super::modal::Tone::Secondary,
                |this, _, cx| {
                    this.gdm_cleanup = None;
                    cx.notify();
                },
            )]
        } else {
            vec![super::modal::button(
                "gdm-cleanup-leave",
                SharedString::from(leave_label(count)),
                if count == 0 {
                    super::modal::Tone::PrimaryDisabled
                } else {
                    super::modal::Tone::Critical
                },
                |this, _, cx| this.gdm_cleanup_leave(cx),
            )]
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "gdm-cleanup",
                title: dc_intl::t("FrCK4r").into(),
                subtitle: Some(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("2Cgdhl"))
                        .into_any_element(),
                ),
                body: Some(body),
                buttons,
                close: Box::new(|this, _, _| this.gdm_cleanup = None),
            },
            cx,
        ))
    }

    /// The Direct Messages header's More button (`j` of module 830507): its menu (`clean-up-inactive-gdms`, "Direct
    /// Messages options") with "Clean up Group DMs...".
    pub(super) fn dm_header_more(&self, cx: &mut Context<Self>) -> AnyElement {
        // `.moreButton_Bce46W`: hidden (`@media (hover:hover)`) until the header is hovered or its menu is open
        // (`aria-expanded`).
        let expanded = self
            .context
            .open
            .as_ref()
            .is_some_and(|open| open.target == super::context_menu::Target::DmListMore);
        // Module 830507 (`j`): the button carries the "More" tooltip (`FCwJId`), above.
        let button = div()
            .id("dm-more")
            .relative()
            .when(!expanded, |button| {
                button
                    .opacity(0.)
                    .group_hover("dm-header", |style| style.opacity(1.))
            })
            .child(self.anchor_probe("dm-more"))
            .size(px(24.))
            .p(px(4.))
            .cursor_pointer()
            .child(icon("clips_more", 16., "--text-muted"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.tip = None;
                let anchor = this.tip_anchors.borrow().get("dm-more").copied();
                if let Some(anchor) = anchor {
                    // `position: "bottom", align: "left", spacing: 8`.
                    this.open_context_menu(
                        super::context_menu::Target::DmListMore,
                        gpui::point(anchor.left(), anchor.bottom() + px(8.)),
                        cx,
                    );
                }
            }));
        self.with_tip(
            "dm-more-tip",
            Tip::text(dc_intl::t("FCwJId"), Side::Top),
            button,
            cx,
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_quiet_group_dms_are_listed_oldest_first() {
        let day = 24 * 60 * 60 * 1000;
        let now = 1_000 * day;
        let list = candidates(
            [
                (1, now - 10 * day),
                (2, now - 400 * day),
                (3, now - 200 * day),
            ]
            .into_iter(),
            now,
        );
        assert_eq!(
            list.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[test]
    fn a_recent_message_keeps_an_old_group_out() {
        let snowflake = |ms: i64| ((ms - 1_420_070_400_000) as u64) << 22;
        let day = 24 * 60 * 60 * 1000;
        let now = 1_800_000_000_000;
        let old = snowflake(now - 900 * day);
        let recent = snowflake(now - 2 * day);
        // An old group with a recent message (from the read state, or only from the channel) is not listed.
        let channels = [
            (1, last_activity(Some(recent), old, old)),
            (2, last_activity(None, recent, old)),
            (3, last_activity(Some(0), 0, old)),
        ];
        let list = candidates(channels.into_iter(), now);
        assert_eq!(list.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![3]);
    }

    #[test]
    fn the_select_control_and_the_button_read_as_the_official_ones() {
        assert_eq!(select_label(0, 3), "Select All");
        assert_eq!(select_label(3, 3), "Unselect All");
        assert_eq!(select_label(0, 1500), "Select 1,000");
        assert_eq!(select_label(1000, 1500), "Unselect All");
        assert_eq!(leave_label(0), "Leave Group DMs Silently");
        assert_eq!(leave_label(1), "Leave 1 Group DM Silently");
        assert_eq!(leave_label(2), "Leave 2 Group DMs Silently");
    }
}
