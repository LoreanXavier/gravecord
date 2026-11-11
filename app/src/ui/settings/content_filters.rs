//! Messaging Permissions' "Sensitive content filters" (`CONTENT_FILTERS_SETTING`, `yX` and `yz` in module 158935).
//!
//! Two tabs (Mature Sexual Media, Graphic Media), each with three selects: direct messages from friends, from
//! others and in server channels. The account's choice is `text_and_images.explicit_content_settings` (29) or
//! `gore_content_settings` (32); while it is unset the default follows `explicit_content_filter` (modules 632119 and
//! 444802). Changing one writes all three fields, as `Jz` and `qY` do.

use super::model::ButtonKind;
use super::render::runs_text;
use super::{Choice, button, mask_icon, noticed};
use crate::net::Request;
use crate::ui::{Root, color};
use dc_proto::settings::{CONTENT_REDACTION, EXPLICIT_CONTENT_FILTER, ProtoPref};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};

pub(super) const FILTERS: &str = "content_filters_setting";
const TAB: &str = "content_filters_tab";
/// One open flag per select: the layer system closes every `.../open` key on an outside press or Escape.
const OPEN: [[&str; 3]; 2] = [
    [
        "explicit_content_friend_dm/open",
        "explicit_content_non_friend_dm/open",
        "explicit_content_guilds/open",
    ],
    [
        "gore_content_friend_dm/open",
        "gore_content_non_friend_dm/open",
        "gore_content_guilds/open",
    ],
];

/// `ExplicitContentRedaction` (`eJ.TO` in module 873298).
pub(super) const SHOW: u64 = 1;
pub(super) const BLUR: u64 = 2;
pub(super) const BLOCK: u64 = 3;

/// What an unset row shows for a user who is not confirmed as an adult (`E` in 632119 and `c` in 444802): servers
/// blur; direct messages follow `explicit_content_filter` (explicit media) or friends blur and others block (gore).
pub(super) fn default_redaction(kind: usize, row: usize, filter: u64) -> u64 {
    if row == 2 {
        return BLUR;
    }
    if kind == 1 {
        return if row == 0 { BLUR } else { BLOCK };
    }
    match (row, filter) {
        (0, 0..=1) => BLUR,
        (0, _) => BLOCK,
        (_, 0) => BLUR,
        _ => BLOCK,
    }
}

/// The options a select lists: Show only for a confirmed adult, Block only in direct messages.
pub(super) fn options(row: usize, adult: bool) -> Vec<u64> {
    let mut list = if row == 2 {
        vec![BLUR]
    } else {
        vec![BLUR, BLOCK]
    };
    if adult {
        list.insert(0, SHOW);
    }
    list
}

fn label_of(value: u64) -> &'static str {
    // `S49Uad`, `D/157Y` and `5k5OFp`.
    match value {
        SHOW => "Show",
        BLOCK => "Block",
        _ => "Blur",
    }
}

fn icon_of(value: u64) -> &'static str {
    // The official draws an image for Show and Blur and a circle with a slash for Block; the image icon is not among
    // the extracted assets, so the eye stands in for it.
    match value {
        BLOCK => "blocked",
        _ => "eye_slash",
    }
}

/// The three values of a tab as they show now: the account's, else the default.
pub(super) fn current(kind: usize, stored: [Option<u64>; 3], filter: u64) -> [u64; 3] {
    let mut shown = [0; 3];
    for row in 0..3 {
        shown[row] = stored[row]
            .filter(|value| *value != 0)
            .unwrap_or_else(|| default_redaction(kind, row, filter));
    }
    shown
}

impl Root {
    fn redaction_values(&self, kind: usize) -> [u64; 3] {
        let filter = self
            .stores
            .proto_varint(EXPLICIT_CONTENT_FILTER.path, EXPLICIT_CONTENT_FILTER.field)
            .unwrap_or(1);
        let stored = std::array::from_fn(|row| {
            let pref: &ProtoPref = &CONTENT_REDACTION[kind][row];
            self.stores.proto_varint(pref.path, pref.field)
        });
        current(kind, stored, filter)
    }

    /// A select was chosen: all three fields of the tab are written, the changed one with its new value.
    fn redaction_chosen(&mut self, kind: usize, row: usize, value: u64) {
        let mut values = self.redaction_values(kind);
        values[row] = value;
        let mut patch = String::new();
        for (slot, value) in values.iter().enumerate() {
            patch = self.stores.set_pref(&CONTENT_REDACTION[kind][slot], *value);
        }
        if !patch.is_empty() {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".to_owned(),
                body: Some(serde_json::json!({"settings": patch})),
            });
        }
    }

    pub(super) fn content_filters(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let kind = match self.setting_choices.get(TAB) {
            Some(Choice::Index(1)) => 1,
            _ => 0,
        };
        let values = self.redaction_values(kind);
        let tabs = ["Mature Sexual Media", "Graphic Media"];
        let titles = [
            "Direct messages from friends",
            "Direct messages from others",
            "Messages in server channels",
        ];
        let note = if kind == 0 {
            "Helps detect image-based media that may contain sexually explicit or suggestive material involving adults."
        } else {
            "Helps detect image-based media that may contain violent or potentially disturbing visual content."
        };
        let tab_column =
            div()
                .flex()
                .flex_col()
                .w(px(214.))
                .flex_none()
                .children(tabs.iter().enumerate().map(|(index, name)| {
                    let selected = index == kind;
                    div()
                        .id(SharedString::from(format!("content-filter-tab-{index}")))
                        .h(px(50.))
                        .pl(px(12.))
                        .flex()
                        .items_center()
                        .border_l_2()
                        .border_color(if selected {
                            color("--text-link")
                        } else {
                            color("--border-subtle")
                        })
                        .text_size(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(if selected {
                            color("--text-link")
                        } else {
                            color("--text-subtle")
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.setting_choices.insert(TAB, Choice::Index(index));
                            cx.notify();
                        }))
                        .child(*name)
                }));
        let rows = (0..3).map(|row| {
            let open_key = OPEN[kind][row];
            let open = self.setting_choices.get(open_key) == Some(&Choice::On(true));
            let adult = false;
            // The servers row cannot change while the account is not confirmed as an adult (`isDisabled`).
            let disabled = row == 2 && !adult;
            let value = values[row];
            let listed = options(row, adult);
            let tone = if disabled {
                "--text-subtle"
            } else {
                "--text-strong"
            };
            let option_rows: Vec<AnyElement> = if open {
                listed
                    .into_iter()
                    .map(|option| {
                        div()
                            .id(SharedString::from(format!(
                                "content-filter-option-{kind}-{row}-{option}"
                            )))
                            .h(px(40.))
                            .px(px(8.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .hover(|style| style.bg(color("--background-mod-normal")))
                            .text_size(px(16.))
                            .text_color(color("--text-strong"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.setting_choices.insert(open_key, Choice::On(false));
                                this.redaction_chosen(kind, row, option);
                                cx.notify();
                            }))
                            .child(mask_icon(icon_of(option), 20., "--text-strong"))
                            .child(label_of(option))
                            .into_any_element()
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let dropdown = open.then(|| {
                gpui::deferred(
                    crate::ui::keyboard::outside(
                        div().id(SharedString::from(format!(
                            "content-filter-list-{kind}-{row}"
                        ))),
                        crate::ui::keyboard::Layer::SettingsSelect,
                        cx,
                    )
                    .occlude()
                    .absolute()
                    .top(px(46.))
                    .right(px(0.))
                    .w(px(180.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-higher"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .flex_col()
                    .children(option_rows),
                )
                .with_priority(2)
                .into_any_element()
            });
            div()
                .relative()
                .child(
                    div()
                        .id(SharedString::from(format!("content-filter-{kind}-{row}")))
                        .h(px(49.))
                        .flex()
                        .items_center()
                        .when(row < 2, |this| {
                            this.border_b_1().border_color(color("--border-subtle"))
                        })
                        .when(row == 2, |this| {
                            this.border_b_1().border_color(color("--border-subtle"))
                        })
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color(if disabled {
                                    "--text-subtle"
                                } else {
                                    "--text-strong"
                                }))
                                .child(titles[row]),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "content-filter-value-{kind}-{row}"
                                )))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .when(!disabled, |this| this.cursor_pointer())
                                .when(!disabled, |this| {
                                    this.on_click(cx.listener(move |this, _, _, cx| {
                                        let now = this.setting_choices.get(open_key)
                                            == Some(&Choice::On(true));
                                        this.setting_choices.insert(open_key, Choice::On(!now));
                                        cx.notify();
                                    }))
                                })
                                .child(mask_icon(icon_of(value), 20., tone))
                                .child(
                                    div()
                                        .w(px(48.))
                                        .text_size(px(16.))
                                        .text_color(color(tone))
                                        .child(label_of(value)),
                                )
                                .child(mask_icon("settings_select_chevron", 20., tone)),
                        ),
                )
                .children(dropdown)
        });
        // `useInlineNotice`: shown while the account is not confirmed as an adult (captured text).
        let notice = div()
            .mb(px(8.))
            .flex()
            .items_center()
            .gap(px(16.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--inlinenotice-border-info"))
            .bg(color("--background-feedback-mod-info"))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .gap(px(8.))
                    .child(div().h(px(18.)).flex().items_center().child(mask_icon(
                        "settings_notice_info",
                        16.,
                        "--icon-feedback-info",
                    )))
                    .child(div().flex_1().min_w(px(0.)).child(noticed(
                        &[
                            (
                                "Some settings can only be changed if you’re in the adult age group. ",
                                false,
                            ),
                            ("Learn more", true),
                        ],
                        "--text-strong",
                    ))),
            )
            // `cI+bc/`: the age check opens a verification flow that is not built (listed as missing).
            .child(button("content-filters-confirm-age", "Confirm Age", ButtonKind::Secondary));
        let header = div()
            .pb(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Sensitive content filters"),
            )
            .child(runs_text(
                &[
                    (
                        "Choose how you want to see image-based media detected by Discord’s sensitive content filters. ",
                        false,
                    ),
                    ("Learn more", true),
                ],
                "--text-subtle",
            ));
        let body = div().flex().flex_row().child(tab_column).child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .children(rows)
                .child(
                    div()
                        .pt(px(12.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-subtle"))
                        .child(note),
                ),
        );
        let _ = width;
        // The page column is 696px wide (`select` width in the generated page).
        div()
            .w(px(696.))
            .flex()
            .flex_col()
            .child(notice)
            .child(header)
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_account_blurs_friends_and_blocks_others_like_the_captured_page() {
        // The captured page: friends Blur, others Block, servers Blur, with the filter at its default.
        assert_eq!(current(0, [None; 3], 1), [BLUR, BLOCK, BLUR]);
        assert_eq!(current(1, [None; 3], 1), [BLUR, BLOCK, BLUR]);
    }

    #[test]
    fn the_explicit_filter_setting_moves_the_defaults() {
        assert_eq!(current(0, [None; 3], 0), [BLUR, BLUR, BLUR]);
        assert_eq!(current(0, [None; 3], 2), [BLOCK, BLOCK, BLUR]);
    }

    #[test]
    fn a_stored_choice_wins_and_zero_means_unset() {
        assert_eq!(
            current(0, [Some(BLOCK), Some(0), None], 1),
            [BLOCK, BLOCK, BLUR]
        );
    }

    #[test]
    fn show_is_offered_only_to_adults_and_block_only_in_direct_messages() {
        assert_eq!(options(0, false), vec![BLUR, BLOCK]);
        assert_eq!(options(2, false), vec![BLUR]);
        assert_eq!(options(1, true), vec![SHOW, BLUR, BLOCK]);
    }
}
