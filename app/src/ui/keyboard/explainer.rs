//! Keyboard navigation explainer: 203480/502229 trigger, 645905 content, 775602 seen flag.
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use crate::ui::{Root, color, modal};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Keystroke, deferred, div, prelude::*, px,
};

const SEEN: &str = "keyboardNavigationExplainerModalSeen";
const ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/1500000056121";

pub(super) struct Explainer {
    remaining: u8,
    pub(super) open: bool,
}

impl Default for Explainer {
    fn default() -> Self {
        Self {
            remaining: 5,
            open: false,
        }
    }
}

impl Explainer {
    fn tab(&mut self, account_created: Option<u64>, now: u64, seen: bool) -> bool {
        self.remaining = self.remaining.saturating_sub(1);
        self.open = self.remaining == 0
            && !seen
            && account_created.is_some_and(|created| now.saturating_sub(created) >= 86_400_000);
        self.open
    }
}

fn counted_tab(key: &Keystroke, keyboard_mode: bool, modal_open: bool) -> bool {
    let m = key.modifiers;
    keyboard_mode && !modal_open && key.key == "tab" && !m.control && !m.alt && !m.platform
}

impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn script_keyboard_explainer(
        &mut self,
        command: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if !crate::inert::on() {
            return "keyboard-explainer requires inert capture".to_owned();
        }
        match command {
            "reset" => {
                self.keys.explainer = Explainer::default();
                self.keys.set_modality(false);
                crate::storage::set(SEEN, serde_json::json!(false));
            }
            "seen" => crate::storage::set(SEEN, serde_json::json!(true)),
            "state" => {}
            _ => return "keyboard-explainer reset|seen|state".to_owned(),
        }
        cx.notify();
        format!(
            "open={} remaining={} seen={}",
            self.keys.explainer.open,
            self.keys.explainer.remaining,
            crate::storage::get(SEEN)
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        )
    }

    pub(in crate::ui) fn keyboard_explainer_key(
        &mut self,
        key: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.keys.explainer.open && key.key == "escape" {
            self.keys.explainer.open = false;
            cx.notify();
            return true;
        }
        // The key which first enables keyboard mode is not handled by the source's Tab listener.
        if !counted_tab(
            key,
            self.keys.keyboard_modality,
            self.covering_dialogs() > 0,
        ) {
            return false;
        }
        let created = self
            .stores
            .current_user
            .as_ref()
            .map(|user| dc_state::read_state::snowflake_ms(user.id));
        let seen = crate::storage::get(SEEN)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        if self.keys.explainer.tab(created, now, seen) {
            // The source records this on mount, even if the user closes the dialog with Escape.
            crate::storage::set(SEEN, serde_json::json!(true));
            cx.notify();
            return true;
        }
        false
    }

    pub(in crate::ui) fn keyboard_explainer_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.keys.explainer.open {
            return None;
        }
        let title = dc_intl::format_runs("V4fxrp", &[("tab", "tab".into())])
            .into_iter()
            .map(|(text, tag)| {
                if tag.as_deref() == Some("tab") {
                    div()
                        .mx(px(4.))
                        .mt(px(6.))
                        .child(super::help::combo("tab"))
                        .into_any_element()
                } else {
                    div().child(text).into_any_element()
                }
            });
        let tip = |bind: &str, key: &str| {
            div()
                .flex()
                .items_start()
                .child(super::help::combo(bind).flex_none())
                .child(div().flex_1().min_w(px(0.)).child(dc_intl::t(key)))
        };
        let article = dc_intl::format_runs("EWFVSW", &[("link", ARTICLE.into())])
            .into_iter()
            .map(|(text, tag)| {
                if tag.as_deref() == Some("$link") {
                    let focus = self
                        .stop("keyboard-explainer-article", cx)
                        .tab_index(0)
                        .tab_stop(true);
                    let ringed = self.keyboard_mode() && self.keys.stops.has_focus(&focus);
                    let key_focus = focus.clone();
                    div()
                        .id("keyboard-explainer-article")
                        .relative()
                        .track_focus(&focus)
                        .text_color(color("--text-link"))
                        .cursor_pointer()
                        .hover(|style| style.underline())
                        .when(ringed, |link| {
                            link.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .top(px(-1.))
                                    .child(super::hollow_ring(4.)),
                            )
                        })
                        .on_key_down(cx.listener(
                            move |this, event: &gpui::KeyDownEvent, window, cx| {
                                if key_focus.is_focused(window) && event.keystroke.key == "enter" {
                                    this.follow_link(ARTICLE, ARTICLE, cx);
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.follow_link(ARTICLE, ARTICLE, cx)),
                        )
                        .child(text)
                        .into_any_element()
                } else {
                    div().child(text).into_any_element()
                }
            });
        let body = div()
            .id("keyboard-explainer-content")
            .parity("keyboard-explainer-content")
            .min_h(px(0.))
            .overflow_y_scroll()
            .py(px(32.))
            .pl(px(16.))
            .pr(px(24.))
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(
                div()
                    .parity("keyboard-explainer-title")
                    .h(px(33.))
                    .mb(px(8.))
                    .flex()
                    .justify_center()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .children(title),
            )
            .child(
                div()
                    .parity("keyboard-explainer-description")
                    .child(dc_intl::t!("oQA/OB")),
            )
            .child(
                div()
                    .parity("keyboard-explainer-tips")
                    .my(px(16.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-normal"))
                    .child(tip("mod+/", "43Qn2r"))
                    .child(tip("mod+k", "zJg4FY")),
            )
            .child(
                div()
                    .parity("keyboard-explainer-more")
                    .flex()
                    .flex_wrap()
                    .children(article),
            );
        let footer = div()
            .parity("keyboard-explainer-footer")
            .flex_none()
            .px(px(24.))
            .py(px(16.))
            .flex()
            .justify_end()
            .rounded_b(px(12.))
            .bg(color("--modal-footer-background"))
            .child(modal::small_footer_button(
                modal::button(
                    "keyboard-explainer-okay",
                    dc_intl::t!("BddRzS"),
                    modal::Tone::Primary,
                    |this, _, cx| {
                        this.keys.explainer.open = false;
                        cx.notify();
                    },
                ),
                cx,
            ));
        let pose = crate::ui::motion::legacy_modal_pose(
            "keyboard-explainer",
            "keyboard-explainer-layer",
            false,
        );
        Some(
            deferred(
                div()
                    .id("keyboard-explainer-layer")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(pose.dim(color("--background-scrim")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.keys.explainer.open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("keyboard-explainer-root")
                            .parity_with_border("keyboard-explainer-root", 1.)
                            .posed(pose)
                            .occlude()
                            .w(px(444.))
                            .max_w_full()
                            .min_h(px(220.))
                            .max_h(px(self.viewport.1.min(720.)))
                            .flex()
                            .flex_col()
                            .rounded(px(12.))
                            .overflow_hidden()
                            .border_1()
                            .border_color(color("--border-normal"))
                            .bg(color("--modal-background"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.16),
                                offset: gpui::point(px(0.), px(4.)),
                                blur_radius: px(8.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .child(body)
                            .child(footer),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explainer_counts_five_tabs_after_keyboard_mode_starts() {
        let key = Keystroke::parse("tab").unwrap();
        assert!(!counted_tab(&key, false, false));
        assert!(!counted_tab(&key, true, true));
        assert!(counted_tab(&key, true, false));
        assert!(counted_tab(
            &Keystroke::parse("shift-tab").unwrap(),
            true,
            false
        ));
        for bind in ["ctrl-tab", "alt-tab", "up", "home"] {
            assert!(!counted_tab(&Keystroke::parse(bind).unwrap(), true, false));
        }
        let mut state = Explainer::default();
        for _ in 0..4 {
            assert!(!state.tab(Some(0), 86_400_000, false));
        }
        assert!(state.tab(Some(0), 86_400_000, false));
    }

    #[test]
    fn explainer_waits_for_an_old_enough_account_and_stays_seen_after_close() {
        let mut state = Explainer::default();
        for _ in 0..6 {
            assert!(!state.tab(None, 86_400_000, false));
        }
        assert!(!state.tab(Some(1), 86_400_000, false));
        assert!(state.tab(Some(1), 86_400_001, false));
        state.open = false;
        assert!(!state.tab(Some(1), 86_400_002, true));
        let mut restarted = Explainer::default();
        for _ in 0..10 {
            assert!(!restarted.tab(Some(0), 86_400_000, true));
        }
    }

    #[test]
    fn explainer_title_and_article_keep_their_localized_hooks() {
        let title = dc_intl::format_runs("V4fxrp", &[("tab", "tab".into())]);
        assert!(
            title
                .iter()
                .any(|(text, tag)| tag.as_deref() == Some("tab") && text == "tab")
        );
        let article = dc_intl::format_runs("EWFVSW", &[("link", ARTICLE.into())]);
        assert!(
            article
                .iter()
                .any(|(text, tag)| tag.as_deref() == Some("$link") && !text.is_empty())
        );
    }
}
