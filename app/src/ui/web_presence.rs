//! Server Web Presence / Discoverable Content Channel Feature Picker & Summary Browser
//! (module 284305, feature bundle 137005, REST 603013).
//!
//! Covers all 4 surfaces of module 284305:
//! 1. Discoverable Content on the Web settings section (`OLNu03`, `yTP3YK`, `+ELXr0` Channel
//!    Summaries, `HvBP0Z` Channel Messages, `ZS+NM3` Selected Channels with `+N more` `F6iMs4` /
//!    `PoWNfe` None, `Bz71mg` Review content shown on the web -> `+dSZbK` Review)
//! 2. Channel Feature Picker subview (`13/7kX` Back, `bM+M/m` Breadcrumb, `vgmc/d`/`MFxb27`,
//!    `KaAdWl` "{count} Selected", `l4rqaz` Select All / `MECJZo` Deselect All, `wM7uRI` Search
//!    Channels, `f5cMAg` No channels were found, `MDtgYB` save failure notice)
//! 3. Summary Review modal (`kGOOse` All channels, `/YzI63` No Access, `pvuST8` Remove by link or
//!    ID, `WSGINd` Most Recent, `cpT0Cq` Close, `6vB9E2` View on web, `LGCOmh` Remove from web,
//!    confirm `YbNQ4h`/`DEN16e`/`Gt2OY/`, empty/off states `fWZps3`/`EQjhnD`)
//! 4. Member Channel Notice Bar (`XRu92R` + `WAI6xu` Dismiss).
//!
//! REST: `GET`/`PATCH /guilds/{id}/discoverable-content`,
//! `GET /guilds/{id}/discoverable-content/summaries`,
//! `POST /guilds/{id}/discoverable-content/summaries/{id}/remove`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::HashSet;

pub(crate) const DISCOVERABLE_GET_KEY: &str = "web-presence:get";
pub(crate) const DISCOVERABLE_PATCH_KEY: &str = "web-presence:patch";
pub(crate) const SUMMARIES_REMOVE_KEY: &str = "web-presence:summary-remove";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebPresenceFeature {
    ChannelSummaries,
    ChannelMessages,
}

impl WebPresenceFeature {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::ChannelSummaries => dc_intl::t("+ELXr0"),
            Self::ChannelMessages => dc_intl::t("HvBP0Z"),
        }
    }

    pub(crate) fn picker_heading(self) -> &'static str {
        match self {
            Self::ChannelSummaries => dc_intl::t("vgmc/d"),
            Self::ChannelMessages => dc_intl::t("MFxb27"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WebSummaryItem {
    pub id: String,
    pub channel_id: Id,
    pub channel_name: String,
    pub title: String,
    pub people_count: u32,
    pub message_count: u32,
    pub date_label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebPresenceView {
    Overview,
    Picker(WebPresenceFeature),
    ReviewSummaries,
}

#[derive(Clone, Debug)]
pub(crate) struct WebPresenceModal {
    pub guild_id: Id,
    pub view: WebPresenceView,
    pub summaries_enabled: bool,
    pub messages_enabled: bool,
    pub selected_summaries: HashSet<Id>,
    pub selected_messages: HashSet<Id>,
    pub search_query: String,
    pub summaries: Vec<WebSummaryItem>,
    pub save_failed: bool,
    pub member_notice_dismissed: bool,
}

/// `PATCH /guilds/{guildId}/discoverable-content` (module 284305 / 603013).
pub(crate) fn patch_discoverable_content_request(
    guild_id: Id,
    summaries_enabled: bool,
    messages_enabled: bool,
    summary_channels: &HashSet<Id>,
    message_channels: &HashSet<Id>,
) -> Request {
    let to_arr = |set: &HashSet<Id>| -> Vec<Value> {
        set.iter()
            .map(|id| json!({ "channel_id": id.to_string(), "included": true }))
            .collect()
    };
    Request::Rest {
        key: Some(DISCOVERABLE_PATCH_KEY.to_owned()),
        method: reqwest::Method::PATCH,
        route: format!("/guilds/{guild_id}/discoverable-content"),
        body: Some(json!({
            "channel_summaries": { "enabled": summaries_enabled, "channels": to_arr(summary_channels) },
            "channel_messages": { "enabled": messages_enabled, "channels": to_arr(message_channels) },
        })),
    }
}

/// `POST /guilds/{guildId}/discoverable-content/summaries/{summaryId}/remove` (module 284305).
pub(crate) fn remove_summary_request(guild_id: Id, summary_id: &str, channel_id: Id) -> Request {
    Request::Rest {
        key: Some(SUMMARIES_REMOVE_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild_id}/discoverable-content/summaries/{summary_id}/remove"),
        body: Some(json!({ "channel_id": channel_id.to_string() })),
    }
}

impl Root {
    /// Handles REST answers for Web Presence / Discoverable Content (`284305`).
    pub(super) fn handle_web_presence_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.web_presence_modal.as_mut() else {
            return;
        };
        let failed = body["error"].is_string() || body["status"].as_u64().is_some_and(|s| s >= 400);
        if key == DISCOVERABLE_PATCH_KEY {
            modal.save_failed = failed;
            cx.notify();
        } else if key == DISCOVERABLE_GET_KEY && !failed {
            if let Some(en) = body["body"]["channel_summaries"]["enabled"].as_bool() {
                modal.summaries_enabled = en;
            }
            if let Some(en) = body["body"]["channel_messages"]["enabled"].as_bool() {
                modal.messages_enabled = en;
            }
            cx.notify();
        }
    }

    fn guild_text_channels(&self, guild_id: Id) -> Vec<(Id, String)> {
        self.stores
            .guild(guild_id)
            .map(|g| {
                g.channels
                    .iter()
                    .filter(|c| matches!(c.kind, dc_state::ChannelKind::Text))
                    .map(|c| (c.id, c.name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Modal layer for Server Web Presence / Channel Feature Picker (`284305`).
    pub(super) fn web_presence_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.web_presence_modal.as_ref()?;
        let channels = self.guild_text_channels(modal.guild_id);

        let content: AnyElement = match modal.view {
            WebPresenceView::Overview => self.render_web_presence_overview(modal, &channels, cx),
            WebPresenceView::Picker(feature) => {
                self.render_web_presence_picker(modal, feature, &channels, cx)
            }
            WebPresenceView::ReviewSummaries => self.render_web_presence_review(modal, cx),
        };

        Some(
            deferred(
                div()
                    .id("web-presence-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.web_presence_modal = None;
                        cx.notify();
                    }))
                    .child(content),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    fn render_web_presence_overview(
        &self,
        modal: &WebPresenceModal,
        channels: &[(Id, String)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let summary_preview = |selected: &HashSet<Id>| -> String {
            let names: Vec<&str> = channels
                .iter()
                .filter(|(id, _)| selected.contains(id))
                .map(|(_, n)| n.as_str())
                .collect();
            if names.is_empty() {
                dc_intl::t("PoWNfe").to_owned()
            } else if names.len() <= 3 {
                names
                    .iter()
                    .map(|n| format!("#{n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                let first3 = names[..3]
                    .iter()
                    .map(|n| format!("#{n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let more =
                    dc_intl::format("F6iMs4", &[("count", Arg::Num((names.len() - 3) as f64))]);
                format!("{first3} {more}")
            }
        };

        let feature_card = |feature: WebPresenceFeature, enabled: bool, selected: &HashSet<Id>| {
            let preview_text = summary_preview(selected);
            div()
                .rounded(px(10.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .p(px(14.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(15.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(feature.title()),
                        )
                        .child(
                            div()
                                .id(format!("wp-toggle-{feature:?}"))
                                .cursor_pointer()
                                .w(px(40.))
                                .h(px(22.))
                                .rounded_full()
                                .p(px(3.))
                                .bg(if enabled {
                                    color("--control-primary-background-default")
                                } else {
                                    color("--background-base-lowest")
                                })
                                .flex()
                                .items_center()
                                .when(enabled, |d| d.justify_end())
                                .child(div().size(px(16.)).rounded_full().bg(color("--white")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) = this.web_presence_modal.as_mut() {
                                        match feature {
                                            WebPresenceFeature::ChannelSummaries => {
                                                m.summaries_enabled = !m.summaries_enabled;
                                            }
                                            WebPresenceFeature::ChannelMessages => {
                                                m.messages_enabled = !m.messages_enabled;
                                            }
                                        }
                                        let _ =
                                            this.requests.send(patch_discoverable_content_request(
                                                m.guild_id,
                                                m.summaries_enabled,
                                                m.messages_enabled,
                                                &m.selected_summaries,
                                                &m.selected_messages,
                                            ));
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .when(enabled, |card| {
                    card.child(
                        div()
                            .id(format!("wp-open-picker-{feature:?}"))
                            .cursor_pointer()
                            .rounded(px(8.))
                            .bg(color("--background-surface-high"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .px(px(12.))
                            .py(px(10.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.web_presence_modal.as_mut() {
                                    m.view = WebPresenceView::Picker(feature);
                                    m.search_query.clear();
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("ZS+NM3")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(color("--text-strong"))
                                            .truncate()
                                            .child(preview_text),
                                    ),
                            )
                            .child(icon("shop_arrow_right", 14., "--icon-subtle")),
                    )
                })
        };

        div()
            .id("web-presence-overview")
            .w(px(620.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(24.))
                    .pt(px(20.))
                    .pb(px(14.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("OLNu03")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("yTP3YK")),
                            ),
                    )
                    .child(
                        div()
                            .id("web-presence-close")
                            .aria_label(dc_intl::t("cpT0Cq"))
                            .flex_none()
                            .size(px(32.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--icon-subtle"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.web_presence_modal = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .when(!modal.member_notice_dismissed, |col| {
                        col.child(
                            div()
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(rgba(0x5865f21f))
                                .px(px(12.))
                                .py(px(8.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(12.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::format("XRu92R", &[])),
                                )
                                .child(
                                    div()
                                        .id("wp-notice-dismiss")
                                        .flex_none()
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-link"))
                                        .child(dc_intl::t("WAI6xu"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(m) = this.web_presence_modal.as_mut() {
                                                m.member_notice_dismissed = true;
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                    })
                    .child(feature_card(
                        WebPresenceFeature::ChannelSummaries,
                        modal.summaries_enabled,
                        &modal.selected_summaries,
                    ))
                    .child(feature_card(
                        WebPresenceFeature::ChannelMessages,
                        modal.messages_enabled,
                        &modal.selected_messages,
                    ))
                    // Review content shown on the web row
                    .child(
                        div()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .p(px(14.))
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
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("Bz71mg")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("49TARv")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("wp-review-btn")
                                    .flex_none()
                                    .cursor_pointer()
                                    .px(px(14.))
                                    .h(px(32.))
                                    .rounded(px(6.))
                                    .bg(color("--control-secondary-background-default"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child(dc_intl::t("+dSZbK"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.web_presence_modal.as_mut() {
                                            m.view = WebPresenceView::ReviewSummaries;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(modal.save_failed, |col| {
                        col.child(
                            div()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(color("--status-danger"))
                                .bg(rgba(0xda373c1a))
                                .px(px(12.))
                                .py(px(8.))
                                .text_size(px(13.))
                                .text_color(color("--text-feedback-critical"))
                                .child(dc_intl::t("MDtgYB")),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_web_presence_picker(
        &self,
        modal: &WebPresenceModal,
        feature: WebPresenceFeature,
        channels: &[(Id, String)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = match feature {
            WebPresenceFeature::ChannelSummaries => &modal.selected_summaries,
            WebPresenceFeature::ChannelMessages => &modal.selected_messages,
        };
        let q = modal.search_query.trim().to_lowercase();
        let filtered: Vec<(Id, String)> = channels
            .iter()
            .filter(|(_, name)| q.is_empty() || name.to_lowercase().contains(&q))
            .cloned()
            .collect();
        let all_selected =
            !filtered.is_empty() && filtered.iter().all(|(id, _)| selected.contains(id));
        let all_ids: Vec<Id> = filtered.iter().map(|(id, _)| *id).collect();
        let count_label = dc_intl::format("KaAdWl", &[("count", Arg::Num(selected.len() as f64))]);

        div()
            .id("web-presence-picker")
            .w(px(580.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            // Breadcrumb header (`13/7kX` Back + `bM+M/m` Breadcrumb)
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
                            .id("wp-picker-breadcrumb")
                            .aria_label(dc_intl::t("bM+M/m"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("wp-picker-back")
                                    .cursor_pointer()
                                    .px(px(10.))
                                    .h(px(28.))
                                    .rounded(px(6.))
                                    .bg(color("--control-secondary-background-default"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child(dc_intl::t("13/7kX"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.web_presence_modal.as_mut() {
                                            m.view = WebPresenceView::Overview;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(format!(
                                        "{} / {}",
                                        dc_intl::t("tdhddn"),
                                        feature.title()
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("wp-picker-close")
                            .flex_none()
                            .size(px(28.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--icon-subtle"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.web_presence_modal = None;
                                cx.notify();
                            })),
                    ),
            )
            // Body: heading + count + Select All / Deselect All + Search Channels + list
            .child(
                div()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(feature.picker_heading()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-muted"))
                                            .child(count_label),
                                    )
                                    .child("•")
                                    .child(
                                        div()
                                            .id("wp-select-all")
                                            .cursor_pointer()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-link"))
                                            .child(if all_selected {
                                                dc_intl::t("MECJZo")
                                            } else {
                                                dc_intl::t("l4rqaz")
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(m) = this.web_presence_modal.as_mut() {
                                                    let set = match feature {
                                                        WebPresenceFeature::ChannelSummaries => {
                                                            &mut m.selected_summaries
                                                        }
                                                        WebPresenceFeature::ChannelMessages => {
                                                            &mut m.selected_messages
                                                        }
                                                    };
                                                    if all_selected {
                                                        for id in &all_ids {
                                                            set.remove(id);
                                                        }
                                                    } else {
                                                        for id in &all_ids {
                                                            set.insert(*id);
                                                        }
                                                    }
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(36.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .text_color(color("--text-muted"))
                            .child(if modal.search_query.is_empty() {
                                dc_intl::t("wM7uRI").to_owned()
                            } else {
                                modal.search_query.clone()
                            }),
                    )
                    .child(if filtered.is_empty() {
                        div()
                            .py(px(28.))
                            .text_center()
                            .text_size(px(13.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("f5cMAg"))
                            .into_any_element()
                    } else {
                        div()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .flex_col()
                            .children(filtered.into_iter().enumerate().map(|(idx, (cid, name))| {
                                let checked = selected.contains(&cid);
                                div()
                                    .id(format!("wp-chan-{idx}"))
                                    .cursor_pointer()
                                    .px(px(12.))
                                    .py(px(10.))
                                    .when(idx > 0, |d| {
                                        d.border_t_1().border_color(color("--border-subtle"))
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.web_presence_modal.as_mut() {
                                            let set = match feature {
                                                WebPresenceFeature::ChannelSummaries => {
                                                    &mut m.selected_summaries
                                                }
                                                WebPresenceFeature::ChannelMessages => {
                                                    &mut m.selected_messages
                                                }
                                            };
                                            if !set.remove(&cid) {
                                                set.insert(cid);
                                            }
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(15.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-muted"))
                                                    .child("#"),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-strong"))
                                                    .child(name),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .size(px(18.))
                                            .rounded(px(4.))
                                            .border_1()
                                            .border_color(if checked {
                                                color("--control-primary-background-default")
                                            } else {
                                                color("--border-subtle")
                                            })
                                            .bg(if checked {
                                                color("--control-primary-background-default")
                                            } else {
                                                color("--background-base-lowest")
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(checked, |d| {
                                                d.child(icon("checkbox_check", 12., "--white"))
                                            }),
                                    )
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn render_web_presence_review(
        &self,
        modal: &WebPresenceModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let guild_id = modal.guild_id;
        div()
            .id("web-presence-review")
            .w(px(680.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("wp-review-back")
                                    .cursor_pointer()
                                    .px(px(10.))
                                    .h(px(28.))
                                    .rounded(px(6.))
                                    .bg(color("--control-secondary-background-default"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child(dc_intl::t("13/7kX"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.web_presence_modal.as_mut() {
                                            m.view = WebPresenceView::Overview;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("Bz71mg")),
                            ),
                    )
                    .child(
                        div()
                            .id("wp-review-close")
                            .aria_label(dc_intl::t("cpT0Cq"))
                            .cursor_pointer()
                            .size(px(28.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--icon-subtle"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.web_presence_modal = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .min_h(px(320.))
                    // Left sidebar
                    .child(
                        div()
                            .w(px(200.))
                            .flex_none()
                            .p(px(12.))
                            .bg(color("--background-base-lower"))
                            .border_r_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .px(px(10.))
                                            .py(px(8.))
                                            .rounded(px(6.))
                                            .bg(rgba(0x5865f233))
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("kGOOse")),
                                    )
                                    .child(
                                        div()
                                            .px(px(10.))
                                            .py(px(6.))
                                            .text_size(px(12.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("/YzI63")),
                                    ),
                            )
                            .child(
                                div()
                                    .pt(px(10.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-link"))
                                    .child(dc_intl::t("pvuST8")),
                            ),
                    )
                    // Right summary pane
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .p(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("WSGINd").to_uppercase()),
                            )
                            .child(if modal.summaries.is_empty() {
                                div()
                                    .py(px(48.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(if modal.summaries_enabled {
                                                dc_intl::t("EQjhnD")
                                            } else {
                                                dc_intl::t("fWZps3")
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(color("--text-muted"))
                                            .child(if modal.summaries_enabled {
                                                dc_intl::t("9wh4rG")
                                            } else {
                                                dc_intl::t("oLMk33")
                                            }),
                                    )
                                    .into_any_element()
                            } else {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .children(modal.summaries.iter().enumerate().map(|(idx, s)| {
                                        let sid = s.id.clone();
                                        let cid = s.channel_id;
                                        let people_line = dc_intl::format(
                                            "31AOSO",
                                            &[
                                                ("people", Arg::Num(f64::from(s.people_count))),
                                                ("date", Arg::Str(Cow::Borrowed(&s.date_label))),
                                            ],
                                        );
                                        let msgs_line = dc_intl::format(
                                            "dvpl/w",
                                            &[("messages", Arg::Num(f64::from(s.message_count)))],
                                        );
                                        div()
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-base-lower"))
                                            .p(px(12.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
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
                                                            .text_size(px(14.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .truncate()
                                                            .child(s.title.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_none()
                                                            .text_size(px(11.))
                                                            .text_color(color("--text-muted"))
                                                            .child(format!("#{}", s.channel_name)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .child(format!("{people_line} • {msgs_line}")),
                                            )
                                            .child(
                                                div()
                                                    .pt(px(4.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_end()
                                                    .gap(px(10.))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--text-link"))
                                                            .child(dc_intl::t("6vB9E2")),
                                                    )
                                                    .child(
                                                        div()
                                                            .id(format!("wp-remove-sum-{idx}"))
                                                            .cursor_pointer()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color(
                                                                "--text-feedback-critical",
                                                            ))
                                                            .child(dc_intl::t("LGCOmh"))
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    let _ = this.requests.send(
                                                                        remove_summary_request(
                                                                            guild_id, &sid, cid,
                                                                        ),
                                                                    );
                                                                    if let Some(m) = this
                                                                        .web_presence_modal
                                                                        .as_mut()
                                                                    {
                                                                        m.summaries.retain(|x| {
                                                                            x.id != sid
                                                                        });
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    ),
                                            )
                                    }))
                                    .into_any_element()
                            }),
                    ),
            )
            .into_any_element()
    }

    /// Capture harness: `web-presence [overview|picker|review|close]` (module 284305).
    #[cfg(feature = "capture")]
    pub(crate) fn script_web_presence(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let gid = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let first_ch: HashSet<Id> = self
            .guild_text_channels(gid)
            .into_iter()
            .take(1)
            .map(|(id, _)| id)
            .collect();
        let make = |view| WebPresenceModal {
            guild_id: gid,
            view,
            summaries_enabled: true,
            messages_enabled: false,
            selected_summaries: first_ch.clone(),
            selected_messages: HashSet::new(),
            search_query: String::new(),
            summaries: vec![WebSummaryItem {
                id: "sum_1".to_owned(),
                channel_id: first_ch.iter().copied().next().unwrap_or(1),
                channel_name: "general".to_owned(),
                title: "Weekend game night poll & voice stream tips".to_owned(),
                people_count: 6,
                message_count: 24,
                date_label: "Nov 18, 2026".to_owned(),
            }],
            save_failed: false,
            member_notice_dismissed: false,
        };
        match arg.trim() {
            "close" => {
                self.web_presence_modal = None;
                cx.notify();
                "web-presence: closed".to_owned()
            }
            "picker" => {
                self.web_presence_modal = Some(make(WebPresenceView::Picker(
                    WebPresenceFeature::ChannelSummaries,
                )));
                cx.notify();
                "web-presence: picker".to_owned()
            }
            "review" => {
                self.web_presence_modal = Some(make(WebPresenceView::ReviewSummaries));
                cx.notify();
                "web-presence: review".to_owned()
            }
            _ => {
                self.web_presence_modal = Some(make(WebPresenceView::Overview));
                cx.notify();
                "web-presence: overview".to_owned()
            }
        }
    }
}
