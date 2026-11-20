//! Share Nitro Trial Friend Picker & Reminder modal (module 581846).

#[cfg(feature = "capture")]
use super::editor_style;
use super::{Root, color, icon};
use crate::editor::Editor;
#[cfg(feature = "capture")]
use crate::editor::EditorEvent;
use dc_intl::Arg;
#[cfg(feature = "capture")]
use gpui::Window;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, deferred,
    div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ReferralFriend {
    pub id: String,
    pub username: String,
    pub tag: String,
    pub failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ReferralState {
    Selecting,
    Sent,
    FetchError,
}

#[allow(dead_code)]
pub(crate) struct NitroReferralModal {
    pub max_referrals: usize,
    pub is_reminder: bool,
    pub expire_days: u32,
    pub friends: Vec<ReferralFriend>,
    pub selected: Vec<String>,
    pub search: Entity<Editor>,
    pub state: ReferralState,
    pub general_error: bool,
    _sub: Subscription,
}

/// Official remaining-selection helper copy (`UfPk1t`, `hZnIz4`, `3PJw6c`).
pub(crate) fn remaining_selection_copy(selected: usize, max: usize) -> String {
    let left = max.saturating_sub(selected);
    if left == 0 {
        dc_intl::format("3PJw6c", &[("numReferrals", Arg::Num(max as f64))])
    } else if left == 1 {
        dc_intl::t("UfPk1t").to_owned()
    } else {
        dc_intl::format("hZnIz4", &[("numFriends", Arg::Num(left as f64))])
    }
}

impl Root {
    pub(crate) fn close_nitro_referral(&mut self) -> bool {
        self.nitro_referral_modal.take().is_some()
    }

    pub(in crate::ui) fn nitro_referral_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.nitro_referral_modal.as_ref()?;
        let query = modal.search.read(cx).text().trim().to_lowercase();
        let filtered: Vec<&ReferralFriend> = modal
            .friends
            .iter()
            .filter(|f| {
                query.is_empty()
                    || f.username.to_lowercase().contains(&query)
                    || f.tag.to_lowercase().contains(&query)
            })
            .collect();
        let selected_count = modal.selected.len();
        let max_ref = modal.max_referrals.max(1);

        let close_btn = div()
            .id("nitro-ref-close")
            .flex_shrink_0()
            .size(px(30.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(color("--background-mod-subtle")))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("modal_close", 18., "--interactive-icon-default"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_nitro_referral();
                cx.notify();
            }));

        let content: AnyElement = match modal.state {
            ReferralState::Sent => {
                let heading = if modal.is_reminder {
                    dc_intl::t("Pcb7KF").to_owned()
                } else {
                    dc_intl::format(
                        "iWtOfC",
                        &[("numReferrals", Arg::Num(selected_count.max(1) as f64))],
                    )
                };
                let expiry = dc_intl::format(
                    "CZaMHn",
                    &[
                        ("days", Arg::Num(modal.expire_days as f64)),
                        (
                            "helpdeskArticle",
                            Arg::Str(Cow::Borrowed(
                                "https://support.discord.com/hc/en-us/articles/1500008498081",
                            )),
                        ),
                    ],
                );
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(icon(
                        "nitro_wheel",
                        40.,
                        "--control-primary-background-default",
                    ))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .text_center()
                            .child(heading),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .text_center()
                            .child(expiry),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .w_full()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("nitro-ref-close-done")
                                    .flex_1()
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("wpSqAW"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_nitro_referral();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("nitro-ref-see-dms")
                                    .flex_1()
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("xkVQPN"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_nitro_referral();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            ReferralState::FetchError => div()
                .p(px(28.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(icon("circle_info", 36., "--text-feedback-critical"))
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("a9HOKg")),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .text_center()
                        .child(dc_intl::t("JjjeZb")),
                )
                .child(
                    div()
                        .id("nitro-ref-err-close")
                        .mt(px(8.))
                        .h(px(36.))
                        .px(px(20.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("wpSqAW"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_nitro_referral();
                            cx.notify();
                        })),
                )
                .into_any_element(),
            ReferralState::Selecting => {
                let subtitle = if modal.is_reminder {
                    dc_intl::t("mPmPri").to_owned()
                } else {
                    dc_intl::format("GcKM5Z", &[("numReferrals", Arg::Num(max_ref as f64))])
                };
                let helper = remaining_selection_copy(selected_count, max_ref);
                let primary_label = if modal.is_reminder {
                    dc_intl::t("SY9tyI")
                } else {
                    dc_intl::t("ItpQxk")
                };
                let can_send = selected_count > 0;

                let list_el: AnyElement = if filtered.is_empty() {
                    div()
                        .py(px(32.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("PFp+aJ")),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(color("--text-muted"))
                                .text_center()
                                .child(dc_intl::t("eBIGB4")),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(filtered.into_iter().enumerate().map(|(i, friend)| {
                            let is_sel = modal.selected.contains(&friend.id);
                            let fid = friend.id.clone();
                            let err_line = friend.failed.then(|| {
                                dc_intl::format(
                                    "RO3T4B",
                                    &[("userName", Arg::Str(Cow::Borrowed(&friend.username)))],
                                )
                            });
                            div()
                                .id(SharedString::from(format!("nitro-ref-friend-{i}")))
                                .p(px(10.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(if is_sel {
                                    color("--control-primary-background-default")
                                } else {
                                    color("--border-subtle")
                                })
                                .bg(if is_sel {
                                    color("--background-mod-subtle")
                                } else {
                                    color("--background-base-lower")
                                })
                                .cursor_pointer()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
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
                                                        .size(px(32.))
                                                        .rounded_full()
                                                        .bg(color("--background-surface-higher"))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(
                                                            friend
                                                                .username
                                                                .chars()
                                                                .next()
                                                                .unwrap_or('?')
                                                                .to_uppercase()
                                                                .to_string(),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .child(
                                                            div()
                                                                .text_size(px(14.))
                                                                .font_weight(FontWeight(600.))
                                                                .text_color(color("--text-strong"))
                                                                .child(friend.username.clone()),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(px(12.))
                                                                .text_color(color("--text-muted"))
                                                                .child(friend.tag.clone()),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .size(px(20.))
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(if is_sel {
                                                    color("--control-primary-background-default")
                                                } else {
                                                    color("--border-strong")
                                                })
                                                .bg(if is_sel {
                                                    color("--control-primary-background-default")
                                                } else {
                                                    gpui::hsla(0., 0., 0., 0.).into()
                                                })
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .when(is_sel, |b| {
                                                    b.child(icon("checkbox_check", 12., "--white"))
                                                }),
                                        ),
                                )
                                .when_some(err_line, |row, msg| {
                                    row.child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(color("--text-feedback-critical"))
                                            .child(msg),
                                    )
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) = this.nitro_referral_modal.as_mut() {
                                        if let Some(pos) = m.selected.iter().position(|x| x == &fid)
                                        {
                                            m.selected.remove(pos);
                                        } else if m.selected.len() < m.max_referrals {
                                            m.selected.push(fid.clone());
                                        }
                                    }
                                    cx.notify();
                                }))
                        }))
                        .into_any_element()
                };

                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(20.))
                            .pt(px(18.))
                            .pb(px(12.))
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("Lm2nFc")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(subtitle),
                                    ),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .px(px(20.))
                            .pb(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .when(modal.general_error, |col| {
                                col.child(
                                    div()
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .bg(rgba(0xf23f421f))
                                        .border_1()
                                        .border_color(color("--text-feedback-critical"))
                                        .text_size(px(12.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(dc_intl::t("7VBEue")),
                                )
                            })
                            .child(
                                div()
                                    .h(px(38.))
                                    .px(px(10.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(icon("search", 16., "--icon-muted"))
                                    .child(
                                        div().flex_1().min_w(px(0.)).child(modal.search.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .id("nitro-ref-scroll")
                                    .max_h(px(240.))
                                    .overflow_y_scroll()
                                    .child(list_el),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(color("--text-muted"))
                                    .child(helper),
                            ),
                    )
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
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("nitro-ref-cancel")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("wpSqAW"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_nitro_referral();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("nitro-ref-submit")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(!can_send, |b| b.opacity(0.5))
                                    .when(can_send, |b| b.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(primary_label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if can_send
                                            && let Some(m) = this.nitro_referral_modal.as_mut()
                                        {
                                            m.state = ReferralState::Sent;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        };

        let dialog = div()
            .id("nitro-referral-modal")
            .occlude()
            .w(px(460.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(content);

        Some(
            deferred(
                div()
                    .id("nitro-referral-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_nitro_referral();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_referral(
        &mut self,
        arg: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let search = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_single_line(true);
            ed.set_placeholder(dc_intl::t("Kd5RaI").to_owned());
            ed
        });
        let sub = cx.subscribe_in(&search, window, |_, _, _: &EditorEvent, _, cx| {
            cx.notify();
        });
        let state = match arg {
            "sent" => ReferralState::Sent,
            "error" => ReferralState::FetchError,
            _ => ReferralState::Selecting,
        };
        self.nitro_referral_modal = Some(NitroReferralModal {
            max_referrals: 3,
            is_reminder: arg == "remind",
            expire_days: 14,
            friends: vec![
                ReferralFriend {
                    id: "u1".to_owned(),
                    username: "lorean xavier".to_owned(),
                    tag: "@loreanxavier".to_owned(),
                    failed: false,
                },
                ReferralFriend {
                    id: "u2".to_owned(),
                    username: "Wren".to_owned(),
                    tag: "@wren".to_owned(),
                    failed: false,
                },
                ReferralFriend {
                    id: "u3".to_owned(),
                    username: "Wumpus".to_owned(),
                    tag: "@wumpus".to_owned(),
                    failed: arg == "rowerr",
                },
            ],
            selected: vec!["u1".to_owned()],
            search,
            state,
            general_error: arg == "bannererr",
            _sub: sub,
        });
        cx.notify();
        format!("nitro-referral: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_20_keys_of_nitro_referral_581846_resolve() {
        assert!(remaining_selection_copy(2, 3).contains("1 more friend"));
        assert!(remaining_selection_copy(1, 3).contains("2 more friends"));
        assert!(remaining_selection_copy(3, 3).contains("3 friend limit"));
        for key in [
            "mPmPri", "UfPk1t", "hZnIz4", "3PJw6c", "GcKM5Z", "Lm2nFc", "PFp+aJ", "eBIGB4",
            "a9HOKg", "JjjeZb", "wpSqAW", "SY9tyI", "ItpQxk", "Kd5RaI", "RO3T4B", "xkVQPN",
            "Pcb7KF", "iWtOfC", "7VBEue", "CZaMHn",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
