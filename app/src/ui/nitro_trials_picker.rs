//! Send Nitro Trials picker (module 303682, `SelectFriendsModal`) and confirmation modal (module 810852,
//! `TrialsSharedConfirmationModal`), backed by the referral endpoints (module 326084 / `ReferralTrialStore` 851746):
//! `GET /users/@me/referrals/eligibility`, `POST /users/@me/referrals/eligible-users`, and
//! `POST /users/@me/referrals/{recipientId}`.

use super::{Root, avatar_source, color, editor_style, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    SharedString, StyledText, Window, deferred, div, img, prelude::*, px, rgba,
};
use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Range;

const REFERRAL_HELP_URL: &str = "https://support.discord.com/hc/articles/22567595858327";

/// Three-screen router of module 303682 (`SelectFriendsModalScreens`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrialsPickerScreen {
    SelectFriends,
    Confirmation,
    Reminder,
}

/// Recipient status in `ReferralTrialStore` (module 851746 `aK`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecipientTrialStatus {
    Eligible,
    PendingResend,
    Redeemed,
}

/// Candidate friend row inside `SelectFriendsModal` (module 303682).
#[derive(Clone, Debug)]
pub(crate) struct TrialCandidate {
    pub user_id: Id,
    pub display_name: String,
    pub username: String,
    pub avatar_url: Option<String>,
    pub status: RecipientTrialStatus,
}

/// Per-recipient delivery result displayed in `TrialsSharedConfirmationModal` (module 810852).
#[derive(Clone, Debug)]
pub(crate) struct TrialShareResult {
    pub user_id: Id,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub succeeded: bool,
}

/// Open state of `SelectFriendsModal` (module 303682 + 810852).
pub(crate) struct NitroTrialsPickerModal {
    pub screen: TrialsPickerScreen,
    pub referrals_remaining: Option<usize>,
    pub has_eligible_friends: bool,
    pub fetch_error: bool,
    pub candidates: Vec<TrialCandidate>,
    pub selected: HashSet<Id>,
    pub search_editor: Option<Entity<crate::editor::Editor>>,
    pub submitting: bool,
    pub was_reminder: bool,
    pub results: Vec<TrialShareResult>,
}

impl NitroTrialsPickerModal {
    pub(crate) fn new(referrals_remaining: Option<usize>, candidates: Vec<TrialCandidate>) -> Self {
        let has_eligible = !candidates.is_empty();
        Self {
            screen: TrialsPickerScreen::SelectFriends,
            referrals_remaining,
            has_eligible_friends: has_eligible,
            fetch_error: false,
            candidates,
            selected: HashSet::new(),
            search_editor: None,
            submitting: false,
            was_reminder: false,
            results: Vec::new(),
        }
    }

    /// Pending-resend recipients do not consume the remaining referral quota (module 303682).
    pub(crate) fn selected_new_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|c| {
                self.selected.contains(&c.user_id) && c.status == RecipientTrialStatus::Eligible
            })
            .count()
    }

    pub(crate) fn can_select(&self, candidate: &TrialCandidate) -> bool {
        if self.selected.contains(&candidate.user_id) {
            return true;
        }
        if candidate.status == RecipientTrialStatus::Redeemed {
            return false;
        }
        if candidate.status == RecipientTrialStatus::PendingResend {
            return true;
        }
        self.referrals_remaining
            .is_some_and(|rem| self.selected_new_count() < rem)
    }

    pub(crate) fn share_button_label(&self, no_search_results: bool) -> String {
        if no_search_results {
            return dc_intl::t("wpSqAW").to_owned();
        }
        let count = self.selected.len();
        if count > 1 {
            dc_intl::format("iW2stn", &[("nTrials", Arg::Num(count as f64))])
        } else {
            dc_intl::t("ItpQxk").to_owned()
        }
    }
}

fn helpdesk_subtitle(
    id: &'static str,
    runs: Vec<(String, Option<Box<str>>)>,
    centered: bool,
) -> AnyElement {
    let mut full = String::new();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let link_style = HighlightStyle {
        color: Some(color("--text-link").into()),
        ..Default::default()
    };
    for (chunk, hook) in runs {
        let start = full.len();
        full.push_str(&chunk);
        let end = full.len();
        if hook.as_deref() == Some("$link") && end > start {
            ranges.push(start..end);
            highlights.push((start..end, link_style));
        }
    }
    let styled = StyledText::new(full).with_highlights(highlights);
    let interactive = InteractiveText::new(id, styled).on_click(ranges, |_, _, cx| {
        cx.open_url(REFERRAL_HELP_URL);
    });
    div()
        .text_size(px(14.))
        .line_height(px(20.))
        .text_color(color("--text-muted"))
        .when(centered, |d| d.text_center())
        .child(interactive)
        .into_any_element()
}

impl Root {
    pub(crate) fn close_nitro_trials_picker(&mut self) -> bool {
        self.nitro_trials_picker.take().is_some()
    }

    /// Opens the `SelectFriendsModal` (module 303682) and queries eligibility + eligible friends via module 326084.
    #[allow(dead_code)]
    pub(crate) fn open_nitro_trials_picker(&mut self, cx: &mut Context<Self>) {
        let candidates: Vec<TrialCandidate> = self
            .stores
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == 1)
            .filter_map(|(uid, _)| {
                let user = self.stores.users.get(uid)?;
                Some(TrialCandidate {
                    user_id: user.id,
                    display_name: user.display_name().to_owned(),
                    username: user.username.clone(),
                    avatar_url: Some(avatar_source(user).1),
                    status: RecipientTrialStatus::Eligible,
                })
            })
            .collect();
        self.nitro_trials_picker = Some(NitroTrialsPickerModal::new(Some(3), candidates));
        let _ = self.requests.send(Request::Rest {
            key: Some("referrals-eligibility".to_owned()),
            method: reqwest::Method::GET,
            route: "/users/@me/referrals/eligibility".to_owned(),
            body: None,
        });
        let _ = self.requests.send(Request::Rest {
            key: Some("referrals-eligible-users".to_owned()),
            method: reqwest::Method::POST,
            route: "/users/@me/referrals/eligible-users".to_owned(),
            body: Some(serde_json::json!({ "index": 0, "limit": 10, "search_query": "" })),
        });
        cx.notify();
    }

    fn ensure_trials_search_editor(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<crate::editor::Editor> {
        if let Some(ed) = self
            .nitro_trials_picker
            .as_ref()
            .and_then(|m| m.search_editor.clone())
        {
            return ed;
        }
        let placeholder = dc_intl::t("Kd5RaI").to_owned();
        let editor = cx.new(|cx| {
            let mut ed = crate::editor::Editor::new(editor_style(), cx);
            ed.set_single_line(true);
            ed.set_placeholder(placeholder);
            ed
        });
        if let Some(m) = self.nitro_trials_picker.as_mut() {
            m.search_editor = Some(editor.clone());
        }
        editor
    }

    fn submit_nitro_trials_picker(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.nitro_trials_picker.as_mut() else {
            return;
        };
        if modal.submitting || modal.selected.is_empty() {
            return;
        }
        modal.submitting = true;
        modal.was_reminder = modal.screen == TrialsPickerScreen::Reminder;
        let chosen: Vec<TrialCandidate> = modal
            .candidates
            .iter()
            .filter(|c| modal.selected.contains(&c.user_id))
            .cloned()
            .collect();
        let mut results = Vec::with_capacity(chosen.len());
        for c in chosen {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("referrals-send:{}", c.user_id)),
                method: reqwest::Method::POST,
                route: format!("/users/@me/referrals/{}", c.user_id),
                body: Some(serde_json::json!({})),
            });
            results.push(TrialShareResult {
                user_id: c.user_id,
                display_name: c.display_name,
                avatar_url: c.avatar_url,
                succeeded: true,
            });
        }
        if let Some(modal) = self.nitro_trials_picker.as_mut() {
            modal.submitting = false;
            modal.results = results;
            modal.screen = TrialsPickerScreen::Confirmation;
        }
        cx.notify();
    }

    pub(in crate::ui) fn nitro_trials_picker_layer(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let screen = self.nitro_trials_picker.as_ref()?.screen;
        if screen == TrialsPickerScreen::SelectFriends {
            let _ = self.ensure_trials_search_editor(cx);
        }
        let modal = self.nitro_trials_picker.as_ref()?;
        let query = modal
            .search_editor
            .as_ref()
            .map(|ed| ed.read(cx).text().trim().to_ascii_lowercase())
            .unwrap_or_default();

        let close_btn = || {
            div()
                .id("trials-picker-close")
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
                    this.close_nitro_trials_picker();
                    cx.notify();
                }))
        };

        let dialog: AnyElement = match modal.screen {
            TrialsPickerScreen::SelectFriends if modal.referrals_remaining.is_none() => div()
                .id("trials-picker-loading")
                .occlude()
                .w(px(360.))
                .p(px(32.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.))
                .text_color(color("--text-muted"))
                .child(dc_intl::t("2dVCLl"))
                .into_any_element(),

            TrialsPickerScreen::SelectFriends if modal.fetch_error => div()
                .id("trials-picker-error")
                .occlude()
                .w(px(400.))
                .p(px(24.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("lcuio4")),
                        )
                        .child(close_btn()),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("x09+CD")),
                )
                .into_any_element(),

            TrialsPickerScreen::SelectFriends if !modal.has_eligible_friends => {
                let runs = dc_intl::format_runs(
                    "OOCbz8",
                    &[(
                        "helpdeskArticle",
                        Arg::Str(Cow::Borrowed(REFERRAL_HELP_URL)),
                    )],
                );
                div()
                    .id("trials-picker-ineligible")
                    .occlude()
                    .w(px(400.))
                    .p(px(24.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("2YigPp")),
                            )
                            .child(close_btn()),
                    )
                    .child(helpdesk_subtitle("trials-ineligible-link", runs, false))
                    .into_any_element()
            }

            TrialsPickerScreen::SelectFriends | TrialsPickerScreen::Reminder => {
                let is_reminder = modal.screen == TrialsPickerScreen::Reminder;
                let filtered: Vec<&TrialCandidate> = modal
                    .candidates
                    .iter()
                    .filter(|c| {
                        is_reminder
                            || query.is_empty()
                            || c.display_name.to_ascii_lowercase().contains(&query)
                            || c.username.to_ascii_lowercase().contains(&query)
                    })
                    .collect();
                let no_search_results = !is_reminder && !query.is_empty() && filtered.is_empty();
                let cta_label = modal.share_button_label(no_search_results);
                let cta_enabled =
                    (!modal.selected.is_empty() || no_search_results) && !modal.submitting;

                // Selected friend tokens/chips inside the multi-select combobox (module 303682).
                let selected_chips: Vec<AnyElement> = modal
                    .candidates
                    .iter()
                    .filter(|c| modal.selected.contains(&c.user_id))
                    .map(|c| {
                        let uid = c.user_id;
                        div()
                            .id(SharedString::from(format!("trial-chip-{uid}")))
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .cursor_pointer()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(c.display_name.clone()),
                            )
                            .child(icon("close", 12., "--interactive-icon-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.nitro_trials_picker.as_mut() {
                                    m.selected.remove(&uid);
                                }
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect();

                let list_area: AnyElement = if no_search_results {
                    div()
                        .py(px(28.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("8+ywHD")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("CgQmY2")),
                        )
                        .into_any_element()
                } else {
                    div()
                        .max_h(px(260.))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .children(filtered.iter().map(|c| {
                            let uid = c.user_id;
                            let checked = modal.selected.contains(&uid);
                            let allowed = modal.can_select(c);
                            let disabled = !allowed && !checked;
                            let initial = c
                                .display_name
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_uppercase()
                                .to_string();
                            let avatar = div()
                                .size(px(32.))
                                .rounded(px(16.))
                                .overflow_hidden()
                                .bg(color("--brand-500"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(if let Some(url) = c.avatar_url.as_ref() {
                                    img(url.clone())
                                        .size(px(32.))
                                        .rounded(px(16.))
                                        .into_any_element()
                                } else {
                                    div().child(initial).into_any_element()
                                });
                            div()
                                .id(SharedString::from(format!("trial-candidate-{uid}")))
                                .w_full()
                                .px(px(10.))
                                .py(px(8.))
                                .rounded(px(8.))
                                .when(!disabled, |d| {
                                    d.cursor_pointer()
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                })
                                .when(disabled, |d| d.opacity(0.45))
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(avatar)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-strong"))
                                                .child(c.display_name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .child(c.username.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .size(px(20.))
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(if checked {
                                            color("--brand-500")
                                        } else {
                                            color("--interactive-icon-default")
                                        })
                                        .when(checked, |d| d.bg(color("--brand-500")))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(checked, |d| {
                                            d.child(icon("checkbox_check", 14., "--white"))
                                        }),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) = this.nitro_trials_picker.as_mut() {
                                        if m.selected.contains(&uid) {
                                            m.selected.remove(&uid);
                                        } else if let Some(cand) =
                                            m.candidates.iter().find(|x| x.user_id == uid).cloned()
                                            && m.can_select(&cand)
                                        {
                                            m.selected.insert(uid);
                                        }
                                    }
                                    cx.notify();
                                }))
                        }))
                        .into_any_element()
                };

                div()
                    .id("trials-picker-modal")
                    .occlude()
                    .w(px(480.))
                    .p(px(24.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .line_height(px(28.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t(if is_reminder {
                                                "rKmy8I"
                                            } else {
                                                "2dVCLl"
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t(if is_reminder {
                                                "VDlF6o"
                                            } else {
                                                "DXgoi2"
                                            })),
                                    ),
                            )
                            .child(close_btn()),
                    )
                    .when(!is_reminder, |d| {
                        d.child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lowest"))
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(6.))
                                .child(icon("search", 16., "--interactive-icon-default"))
                                .children(selected_chips)
                                .children(
                                    modal
                                        .search_editor
                                        .clone()
                                        .map(|ed| div().flex_1().min_w(px(120.)).child(ed)),
                                ),
                        )
                    })
                    .child(list_area)
                    .child(
                        div()
                            .id("trials-picker-submit")
                            .w_full()
                            .h(px(40.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .when(cta_enabled, |d| {
                                d.hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .cursor_pointer()
                            })
                            .when(!cta_enabled, |d| d.opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(cta_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if no_search_results {
                                    this.close_nitro_trials_picker();
                                    cx.notify();
                                } else {
                                    this.submit_nitro_trials_picker(cx);
                                }
                            })),
                    )
                    .into_any_element()
            }

            // Module 810852: TrialsSharedConfirmationModal.
            TrialsPickerScreen::Confirmation => {
                let succeeded: Vec<&TrialShareResult> =
                    modal.results.iter().filter(|r| r.succeeded).collect();
                let title = if succeeded.is_empty() {
                    dc_intl::t("7VBEue")
                } else if modal.was_reminder {
                    dc_intl::t("GP5lbq")
                } else {
                    dc_intl::t("tKCltd")
                };
                let subtitle_runs = if succeeded.len() == 1 && !modal.was_reminder {
                    dc_intl::format_runs(
                        "s8VHbZ",
                        &[
                            (
                                "recipientName",
                                Arg::Str(Cow::Borrowed(&succeeded[0].display_name)),
                            ),
                            (
                                "helpdeskArticle",
                                Arg::Str(Cow::Borrowed(REFERRAL_HELP_URL)),
                            ),
                        ],
                    )
                } else if modal.was_reminder {
                    dc_intl::format_runs(
                        "4gJJfD",
                        &[(
                            "helpdeskArticle",
                            Arg::Str(Cow::Borrowed(REFERRAL_HELP_URL)),
                        )],
                    )
                } else {
                    dc_intl::format_runs(
                        "AwGSWl",
                        &[(
                            "helpdeskArticle",
                            Arg::Str(Cow::Borrowed(REFERRAL_HELP_URL)),
                        )],
                    )
                };

                div()
                    .id("trials-confirmation-modal")
                    .occlude()
                    .w(px(460.))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .flex()
                    .flex_col()
                    .child(
                        // Nitro-pink header accent of module 810852.
                        div()
                            .w_full()
                            .px(px(24.))
                            .py(px(20.))
                            .bg(rgba(0xb4539533))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(icon("nitro_wheel", 28., "--guild-boosting-pink"))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(title),
                                    ),
                            )
                            .child(close_btn()),
                    )
                    .child(
                        div()
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(helpdesk_subtitle(
                                "trials-confirm-subtitle",
                                subtitle_runs,
                                false,
                            ))
                            .child(div().flex().flex_col().gap(px(8.)).children(
                                modal.results.iter().map(|r| {
                                    let uid = r.user_id;
                                    let fail_line = (!r.succeeded).then(|| {
                                        dc_intl::format(
                                            "RO3T4B",
                                            &[(
                                                "userName",
                                                Arg::Str(Cow::Borrowed(&r.display_name)),
                                            )],
                                        )
                                    });
                                    let initial = r
                                        .display_name
                                        .chars()
                                        .next()
                                        .unwrap_or('?')
                                        .to_uppercase()
                                        .to_string();
                                    div()
                                        .id(SharedString::from(format!("trial-result-{uid}")))
                                        .px(px(12.))
                                        .py(px(10.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lower"))
                                        .flex()
                                        .items_center()
                                        .gap(px(10.))
                                        .child(
                                            div()
                                                .size(px(32.))
                                                .rounded(px(16.))
                                                .overflow_hidden()
                                                .when(!r.succeeded, |d| d.opacity(0.45))
                                                .bg(color("--brand-500"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(if let Some(url) = r.avatar_url.as_ref() {
                                                    img(url.clone())
                                                        .size(px(32.))
                                                        .rounded(px(16.))
                                                        .into_any_element()
                                                } else {
                                                    div().child(initial).into_any_element()
                                                }),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.))
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(r.display_name.clone()),
                                                )
                                                .children(fail_line.map(|line| {
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(color(
                                                            "--text-feedback-critical",
                                                        ))
                                                        .child(line)
                                                })),
                                        )
                                        .when(r.succeeded, |d| {
                                            d.child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "trial-result-msg-{uid}"
                                                    )))
                                                    .px(px(12.))
                                                    .h(px(32.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-secondary-background-default",
                                                    ))
                                                    .hover(|s| {
                                                        s.bg(color(
                                                            "--control-secondary-background-hover",
                                                        ))
                                                    })
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.))
                                                    .child(icon(
                                                        "chat_bubble",
                                                        14.,
                                                        "--interactive-icon-default",
                                                    ))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .font_weight(FontWeight(500.))
                                                            .text_color(color("--text-strong"))
                                                            .child(dc_intl::t("g33r/P")),
                                                    )
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.close_nitro_trials_picker();
                                                            this.open_dm_with(uid, cx);
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                        })
                                }),
                            )),
                    )
                    .into_any_element()
            }
        };

        Some(
            deferred(
                div()
                    .id("trials-picker-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.close_nitro_trials_picker();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(dialog),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_trials_picker(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let arg = arg.trim();
        if arg == "close" {
            self.close_nitro_trials_picker();
            cx.notify();
            return "nitro trials picker closed".to_owned();
        }
        let candidates = vec![
            TrialCandidate {
                user_id: 1001,
                display_name: "plü".to_owned(),
                username: "plu_demo".to_owned(),
                avatar_url: None,
                status: RecipientTrialStatus::Eligible,
            },
            TrialCandidate {
                user_id: 1002,
                display_name: "Wren".to_owned(),
                username: "wren".to_owned(),
                avatar_url: None,
                status: RecipientTrialStatus::PendingResend,
            },
            TrialCandidate {
                user_id: 1003,
                display_name: "lawleis".to_owned(),
                username: "lawleis".to_owned(),
                avatar_url: None,
                status: RecipientTrialStatus::Redeemed,
            },
        ];
        let mut modal = NitroTrialsPickerModal::new(Some(3), candidates);
        match arg {
            "ineligible" => {
                modal.has_eligible_friends = false;
            }
            "error" => {
                modal.fetch_error = true;
            }
            "reminder" => {
                modal.screen = TrialsPickerScreen::Reminder;
                modal.selected.insert(1002);
            }
            "confirm" => {
                modal.screen = TrialsPickerScreen::Confirmation;
                modal.results = vec![
                    TrialShareResult {
                        user_id: 1001,
                        display_name: "plü".to_owned(),
                        avatar_url: None,
                        succeeded: true,
                    },
                    TrialShareResult {
                        user_id: 1002,
                        display_name: "Wren".to_owned(),
                        avatar_url: None,
                        succeeded: false,
                    },
                ];
            }
            _ => {
                modal.selected.insert(1001);
                modal.selected.insert(1002);
            }
        }
        self.nitro_trials_picker = Some(modal);
        cx.notify();
        format!("nitro trials picker opened ({arg})")
    }
}

/// Variant of the single-recipient Share a Nitro Trial modal (module 536110).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ShareTrialVariant {
    Profile,
    HdStreaming,
}

/// Single-recipient Share a Nitro Trial modal state (module 536110 + 810852).
#[derive(Clone, Debug)]
pub(crate) struct SingleShareTrialModal {
    pub recipient_id: u64,
    pub recipient_name: String,
    pub variant: ShareTrialVariant,
    pub referrals_remaining: Option<u32>,
    pub sending: bool,
    pub confirmed: bool,
    pub failed: bool,
}

impl Root {
    /// Renders module `536110` Single-Recipient Share a Nitro Trial modal.
    pub(in crate::ui) fn single_share_trial_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.single_share_trial_modal.as_ref()?;
        let _loading_aria = dc_intl::t("c1wxcb");
        let name = &modal.recipient_name;
        let is_hd = modal.variant == ShareTrialVariant::HdStreaming;
        let heading = if modal.confirmed {
            dc_intl::t(if modal.failed { "7VBEue" } else { "tKCltd" }).to_owned()
        } else {
            dc_intl::format(
                if is_hd { "rdh1ou" } else { "LVlgM4" },
                &[(
                    "recipientName",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(name)),
                )],
            )
        };
        let body = if modal.confirmed {
            if is_hd {
                dc_intl::format(
                    "s8VHbZ",
                    &[
                        (
                            "recipientName",
                            dc_intl::Arg::Str(std::borrow::Cow::Borrowed(name)),
                        ),
                        (
                            "helpdeskArticle",
                            dc_intl::Arg::Str(std::borrow::Cow::Borrowed(
                                "https://support.discord.com",
                            )),
                        ),
                    ],
                )
            } else {
                dc_intl::format(
                    "AwGSWl",
                    &[(
                        "helpdeskArticle",
                        dc_intl::Arg::Str(std::borrow::Cow::Borrowed(
                            "https://support.discord.com",
                        )),
                    )],
                )
            }
        } else if is_hd {
            dc_intl::t("NkgMIP").to_owned()
        } else {
            dc_intl::format(
                "2nw1yD",
                &[(
                    "helpdeskArticle",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed("https://support.discord.com")),
                )],
            )
        };
        let err_msg = modal.failed.then(|| {
            dc_intl::format(
                "RO3T4B",
                &[(
                    "userName",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(name)),
                )],
            )
        });
        let can_send = !modal.sending && modal.referrals_remaining.unwrap_or(0) > 0;
        let rid = modal.recipient_id;

        let card = div()
            .id("single-share-trial-modal")
            .w(px(440.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(96.))
                    .px(px(20.))
                    .bg(gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(gpui::rgb(0x8547c6), 0.),
                        gpui::linear_color_stop(gpui::rgb(0xb845c1), 1.),
                    ))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(icon("nitro_wheel", 36., "--white"))
                    .child(
                        div()
                            .id("single-share-trial-close")
                            .size(px(28.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.single_share_trial_modal = None;
                                cx.notify();
                            }))
                            .child(icon("modal_close", 16., "--white")),
                    ),
            )
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(SharedString::from(heading)),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(SharedString::from(body)),
                    )
                    .when_some(err_msg, |col, err| {
                        col.child(
                            div()
                                .p(px(10.))
                                .rounded(px(6.))
                                .bg(rgba(0xda373c22))
                                .border_1()
                                .border_color(color("--status-danger"))
                                .text_size(px(13.))
                                .text_color(color("--text-feedback-critical"))
                                .child(SharedString::from(err)),
                        )
                    }),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .gap(px(10.))
                    .when(!modal.confirmed, |bar| {
                        bar.child(
                            div()
                                .id("single-share-trial-cancel")
                                .flex_1()
                                .h(px(38.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .text_color(color("--control-secondary-text-default"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.single_share_trial_modal = None;
                                    cx.notify();
                                }))
                                .child(dc_intl::t("ETE/oC")),
                        )
                        .child(
                            div()
                                .id("single-share-trial-send")
                                .flex_1()
                                .h(px(38.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .when(can_send, |s| s.cursor_pointer())
                                .when(!can_send, |s| s.opacity(0.5))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !can_send {
                                        return;
                                    }
                                    let _ = this.requests.send(crate::net::Request::Rest {
                                        key: None,
                                        method: reqwest::Method::POST,
                                        route: format!("/users/@me/referrals/{rid}"),
                                        body: Some(serde_json::json!({})),
                                    });
                                    if let Some(m) = this.single_share_trial_modal.as_mut() {
                                        m.confirmed = true;
                                    }
                                    cx.notify();
                                }))
                                .child(dc_intl::t(if is_hd { "dzQxHq" } else { "ItpQxk" })),
                        )
                    })
                    .when(modal.confirmed, |bar| {
                        bar.child(
                            div()
                                .id("single-share-trial-done")
                                .w_full()
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
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.single_share_trial_modal = None;
                                    cx.notify();
                                }))
                                .child(dc_intl::t("cpT0Cq")),
                        )
                    }),
            );

        Some(
            deferred(
                div()
                    .id("single-share-trial-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.single_share_trial_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_share_trial(
        &mut self,
        mode: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if mode.trim() == "close" {
            self.single_share_trial_modal = None;
            cx.notify();
            return "nitro-share-trial: closed".into();
        }
        let variant = if mode.trim().starts_with("hd") {
            ShareTrialVariant::HdStreaming
        } else {
            ShareTrialVariant::Profile
        };
        self.single_share_trial_modal = Some(SingleShareTrialModal {
            recipient_id: 1001,
            recipient_name: "plü".into(),
            variant,
            referrals_remaining: Some(3),
            sending: false,
            confirmed: mode.trim() == "confirmed",
            failed: mode.trim() == "hd-error",
        });
        cx.notify();
        format!("nitro-share-trial: {}", mode.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_keys_of_nitro_trials_picker_303682_and_810852_resolve() {
        let keys = [
            "2dVCLl", "DXgoi2", "Kd5RaI", "ItpQxk", "iW2stn", "wpSqAW", "8+ywHD", "CgQmY2",
            "lcuio4", "x09+CD", "2YigPp", "OOCbz8", "rKmy8I", "VDlF6o", "tKCltd", "GP5lbq",
            "7VBEue", "AwGSWl", "4gJJfD", "s8VHbZ", "RO3T4B", "g33r/P",
        ];
        for key in keys {
            let value = dc_intl::t(key);
            assert!(!value.is_empty(), "key {key} must resolve");
            assert_ne!(value, key, "key {key} fell back to literal");
        }
    }

    #[test]
    fn resend_candidates_do_not_consume_referral_quota_and_redeemed_are_locked() {
        let candidates = vec![
            TrialCandidate {
                user_id: 1,
                display_name: "One".into(),
                username: "one".into(),
                avatar_url: None,
                status: RecipientTrialStatus::Eligible,
            },
            TrialCandidate {
                user_id: 2,
                display_name: "Two".into(),
                username: "two".into(),
                avatar_url: None,
                status: RecipientTrialStatus::Eligible,
            },
            TrialCandidate {
                user_id: 3,
                display_name: "Pending".into(),
                username: "pending".into(),
                avatar_url: None,
                status: RecipientTrialStatus::PendingResend,
            },
            TrialCandidate {
                user_id: 4,
                display_name: "Done".into(),
                username: "done".into(),
                avatar_url: None,
                status: RecipientTrialStatus::Redeemed,
            },
        ];
        let mut modal = NitroTrialsPickerModal::new(Some(1), candidates.clone());
        assert!(modal.can_select(&candidates[0]));
        assert!(!modal.can_select(&candidates[3]));
        modal.selected.insert(1);
        // Quota (1) is full for new eligible friends, but PendingResend can still be selected.
        assert!(!modal.can_select(&candidates[1]));
        assert!(modal.can_select(&candidates[2]));
        modal.selected.insert(3);
        assert_eq!(modal.selected_new_count(), 1);
        assert!(modal.share_button_label(false).contains('2'));
        assert_eq!(modal.share_button_label(true), dc_intl::t("wpSqAW"));
    }
}
