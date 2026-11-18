//! "Failed to start camera" (module 566566 on module 200749) over a person's tile whose video never showed.
//!
//! The video component (module 259264) starts a 20 second timer while a video is loading (not paused, incoming video
//! on) and dispatches `VIDEO_STREAM_READY_TIMEOUT` when no picture came (`Z` of module 37965); a picture stops it and
//! clears the error (`W`). The AV error manager (module 822209) turns the timed-out video of this account into
//! `video-stream-sender-ready-timeout` (error code 2011) and another person's into
//! `video-stream-receiver-ready-timeout` (2012); the user tile (module 83982) then draws module 566566: the art
//! (`411e01be602621ba.svg`, none under 400 wide, 88 high under 1000, else 134, 16 under it), "Failed to start camera"
//! (`body-md/semibold`, white, 4 under it), "Error: <code>" (`body-sm/semibold`, `text-muted`), the help line
//! (`body-sm/medium`, `--primary-200`, not under 400 wide) and Try again (secondary, `sm` under 400 wide), which
//! clears the error and turns the person's video off and on again a second later.

use super::Root;
use dc_tokens::Theme;
use gpui::{AnyElement, Context, FontWeight, IntoElement, Rgba, div, img, prelude::*, px, rgba};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// `f` of module 259264.
pub(super) const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// `errorCode` of the two error types in module 487329.
pub(super) const SENDER_READY_TIMEOUT: i64 = 2011;
pub(super) const RECEIVER_READY_TIMEOUT: i64 = 2012;
const ART: &str = "https://discord.com/assets/411e01be602621ba.svg";
/// `VOICE_VIDEO_TROUBLESHOOTING` ("360045138471", module 652215) through `getArticleURL` (module 975571, en-us).
const HELP_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360045138471";

fn mid(name: &str) -> Rgba {
    rgba(Theme::Midnight.color(name))
}

/// The loading videos and when each began, and the ones that timed out.
#[derive(Default)]
pub(super) struct VideoReady {
    loading: HashMap<u64, Instant>,
    timed_out: std::collections::HashSet<u64>,
}

impl VideoReady {
    /// A person's tile is drawn: `loading` while their video is expected but no picture has come. Whether the error
    /// shows, and whether the wait just began (the caller wakes the view when it runs out).
    pub(super) fn observe(&mut self, user: u64, loading: bool, now: Instant) -> (bool, bool) {
        if !loading {
            self.loading.remove(&user);
            self.timed_out.remove(&user);
            return (false, false);
        }
        let began = !self.loading.contains_key(&user);
        let since = *self.loading.entry(user).or_insert(now);
        if now.duration_since(since) >= READY_TIMEOUT {
            self.timed_out.insert(user);
        }
        (self.timed_out.contains(&user), began)
    }

    /// Try again: the error goes and the wait starts over.
    pub(super) fn retry(&mut self, user: u64, now: Instant) {
        self.timed_out.remove(&user);
        self.loading.insert(user, now);
    }
}

impl Root {
    /// Module 566566 for a tile `width` wide.
    pub(super) fn camera_failed_content(
        &self,
        user: u64,
        width: f32,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let me = self.stores.current_user.as_ref().map(|me| me.id) == Some(user);
        let code = if me {
            SENDER_READY_TIMEOUT
        } else {
            RECEIVER_READY_TIMEOUT
        };
        // `J` of module 200749.
        let small = width < 400.;
        let art_height = if small {
            None
        } else if width < 1000. {
            Some(88.)
        } else {
            Some(134.)
        };
        let art = self.images.get(ART).cloned();
        if art.is_none() && art_height.is_some() {
            self.want_image(ART.to_owned(), ART.to_owned(), cx);
        }
        let lead = dc_intl::format_runs("d486Wm", &[("helpUrl", HELP_ARTICLE.to_owned().into())]);
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .bg(mid("--primary-700"))
            .p(px(4.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .children(art_height.map(|art_height| {
                div()
                    .w_full()
                    .h(px(art_height))
                    .mb(px(16.))
                    .flex()
                    .justify_center()
                    .children(art.map(|art| {
                        img(art)
                            .h(px(art_height))
                            .object_fit(gpui::ObjectFit::Contain)
                    }))
            }))
            .child(
                div()
                    .mb(px(4.))
                    .text_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(mid("--white"))
                    .child(dc_intl::t("z+mxvo")),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(mid("--text-muted"))
                    .child(dc_intl::format("ejOT95", &[("errorCode", code.into())])),
            )
            .when(!small, |this| {
                // "Having issues with video calls? Let us try to help." with the help center link.
                let mut text = String::new();
                let mut link = 0..0;
                for (run, hook) in lead {
                    let start = text.len();
                    text.push_str(&run);
                    if hook.is_some() {
                        link = start..text.len();
                    }
                }
                let styled = gpui::StyledText::new(text).with_highlights([(
                    link.clone(),
                    gpui::HighlightStyle {
                        color: Some(mid("--text-link").into()),
                        ..Default::default()
                    },
                )]);
                this.child(
                    div()
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(mid("--primary-200"))
                        .child(
                            gpui::InteractiveText::new(
                                gpui::SharedString::from(format!("camera-failed-help-{user}")),
                                styled,
                            )
                            .on_click(vec![link], {
                                let open = cx.listener(|this, _: &usize, _, cx| {
                                    this.follow_link(HELP_ARTICLE, "", cx);
                                });
                                move |index, window, cx| open(&index, window, cx)
                            }),
                        ),
                )
            })
            .when(!selected, |this| {
                this.child(
                    div().mt(px(16.)).child(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "camera-failed-retry-{user}"
                            )))
                            .h(px(if small { 32. } else { 40. }))
                            .px(px(if small { 12. } else { 16. }))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(mid("--control-secondary-border-default"))
                            .bg(mid("--control-secondary-background-default"))
                            .hover(|style| style.bg(mid("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(if small { 14. } else { 16. }))
                            .line_height(px(if small { 18. } else { 20. }))
                            .font_weight(FontWeight(500.))
                            .text_color(mid("--control-secondary-text-default"))
                            .child(dc_intl::t("hxmQ/e"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.camera_retry(user, cx);
                            })),
                    ),
                )
            })
            .into_any_element()
    }

    /// Try again (`W` of module 37965, then local video off and on a second later, `setDisableLocalVideo`).
    fn camera_retry(&mut self, user: u64, cx: &mut Context<Self>) {
        self.video_ready.borrow_mut().retry(user, Instant::now());
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_fails_after_twenty_seconds_without_a_picture() {
        let mut ready = VideoReady::default();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        assert_eq!(ready.observe(1, true, start), (false, true));
        assert_eq!(ready.observe(1, true, at(19)), (false, false));
        assert_eq!(ready.observe(1, true, at(20)), (true, false));
        // A picture clears it.
        assert_eq!(ready.observe(1, false, at(21)), (false, false));
        assert_eq!(ready.observe(1, true, at(22)), (false, true));
        // Try again starts the wait over.
        assert!(ready.observe(1, true, at(43)).0);
        ready.retry(1, at(43));
        assert!(!ready.observe(1, true, at(50)).0);
    }
}
