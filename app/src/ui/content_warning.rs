//! Content Warning Popout ("Hold Up! ... Send Now", module 926262 + caller triggers 710272).
//!
//! Shown anchored above the message composer before sending/scheduling when a message:
//! 1. Mentions `@everyone` or `@here` outside inline/fenced code in a guild channel with >30
//!    members (`47E5Rz`, `sYW2cy`, `2YaiQ1`, footer `mVyrtu`), or
//! 2. Appears to contain a Discord authentication token (`sTwS1a`, no footer).
//!
//! Keyboard bindings: `Enter` confirms (`Send Now`, `KJnHq3` / `khjqdH`), `Escape` or outer click
//! cancels (`Edit Message`, `fsBWmS` / `0LzVPZ`) without clearing the composer draft.

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, StyledText, deferred, div,
    prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContentWarningKind {
    MentionEveryone {
        role: &'static str,
        count: u64,
        is_post: bool,
        is_thread: bool,
    },
    AuthToken,
}

#[derive(Clone, Debug)]
pub(crate) struct ContentWarningPopout {
    pub kind: ContentWarningKind,
    pub pending_content: String,
}

/// Strips fenced (```` ``` ````) and inline (`` ` ``) code spans before scanning for `@everyone`/`@here`.
fn strip_code_spans(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut in_fence = false;
    let mut in_inline = false;
    while i < bytes.len() {
        if !in_inline && i + 2 < bytes.len() && &bytes[i..i + 3] == b"```" {
            in_fence = !in_fence;
            i += 3;
            continue;
        }
        if !in_fence && bytes[i] == b'`' {
            in_inline = !in_inline;
            i += 1;
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        if !in_fence && !in_inline {
            out.push(ch);
        }
        i += ch.len_utf8();
    }
    out
}

/// Rounds member count down to nearest power-of-10 tier (`710272.js`: 30..99 -> 30, 100..999 -> 100, etc.).
fn round_mention_count(count: u64) -> u64 {
    if count < 100 {
        30
    } else if count < 1_000 {
        (count / 100) * 100
    } else {
        (count / 1_000) * 1_000
    }
}

/// Detects a Discord user/bot authentication token (`[A-Za-z0-9_-]{24,}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27,}`).
pub(crate) fn looks_like_discord_token(content: &str) -> bool {
    let is_tok = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    for word in content.split(|c: char| c.is_whitespace() || "\"'`<>()[]{}".contains(c)) {
        let mut parts = word.split('.');
        let (Some(a), Some(b), Some(c), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if a.len() >= 24
            && b.len() >= 6
            && c.len() >= 27
            && a.bytes().all(is_tok)
            && b.bytes().all(is_tok)
            && c.bytes().all(is_tok)
        {
            return true;
        }
    }
    false
}

/// Runs module `710272` pre-send content warning checks on `content`.
pub(crate) fn detect_content_warning(
    content: &str,
    member_count: u64,
    is_post: bool,
    is_thread: bool,
) -> Option<ContentWarningKind> {
    if looks_like_discord_token(content) {
        return Some(ContentWarningKind::AuthToken);
    }
    if member_count > 30 {
        let plain = strip_code_spans(content);
        for role in ["@everyone", "@here"] {
            if plain.contains(role) {
                return Some(ContentWarningKind::MentionEveryone {
                    role,
                    count: round_mention_count(member_count),
                    is_post,
                    is_thread,
                });
            }
        }
    }
    None
}

fn format_bold_styled(key: &str, args: &[(&str, Arg<'_>)]) -> StyledText {
    let runs = dc_intl::format_runs(key, args);
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (chunk, hook) in runs {
        let s = text.len();
        text.push_str(&chunk);
        if hook.as_deref() == Some("$b") && s < text.len() {
            highlights.push((
                s..text.len(),
                HighlightStyle {
                    font_weight: Some(FontWeight(700.)),
                    color: Some(color("--text-strong").into()),
                    ..HighlightStyle::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

impl Root {
    /// Checks `content` against module `710272` rules and opens module `926262` popout if flagged.
    /// Returns `true` when sending was intercepted and held pending confirmation.
    pub(super) fn intercept_content_warning(&mut self, content: &str) -> bool {
        let member_count = self
            .guild
            .and_then(|gid| self.stores.member_counts.get_member_count(gid))
            .unwrap_or(0);
        let (is_post, is_thread) = self
            .channel
            .and_then(|cid| {
                self.guild
                    .and_then(|gid| self.stores.guild(gid))
                    .map(|g| (g, cid))
            })
            .and_then(|(g, cid)| g.channels.iter().find(|c| c.id == cid))
            .map(|ch| {
                let thread = matches!(ch.kind, dc_state::ChannelKind::Other(10..=12));
                (false, thread)
            })
            .unwrap_or((false, false));
        if let Some(kind) = detect_content_warning(content, member_count, is_post, is_thread) {
            self.content_warning = Some(ContentWarningPopout {
                kind,
                pending_content: content.to_owned(),
            });
            return true;
        }
        false
    }

    /// Confirms and sends the held message from module `926262` ("Send Now").
    pub(super) fn confirm_content_warning(&mut self, cx: &mut Context<Self>) {
        if let Some(popout) = self.content_warning.take() {
            self.send_bypass_warning(popout.pending_content);
            cx.notify();
        }
    }

    /// Renders module `926262` Content Warning Popout (`contentWarningPopout_E4U_PW`) anchored
    /// above the composer: `mY3Y38` "Hold Up!", body (`47E5Rz`/`sYW2cy`/`2YaiQ1`/`sTwS1a`),
    /// `fsBWmS` "Edit Message" (`0LzVPZ`), `KJnHq3` "Send Now" (`khjqdH`), and optional `mVyrtu` footer.
    pub(super) fn content_warning_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popout = self.content_warning.as_ref()?;
        let (body_styled, footer_text): (StyledText, Option<&'static str>) = match &popout.kind {
            ContentWarningKind::MentionEveryone {
                role,
                count,
                is_post,
                is_thread,
            } => {
                let key = if *is_post {
                    "sYW2cy"
                } else if *is_thread {
                    "2YaiQ1"
                } else {
                    "47E5Rz"
                };
                let count_str = count.to_string();
                (
                    format_bold_styled(
                        key,
                        &[
                            ("role", Arg::Str(Cow::Borrowed(role))),
                            ("count", Arg::Str(Cow::Borrowed(&count_str))),
                        ],
                    ),
                    Some(dc_intl::t("mVyrtu")),
                )
            }
            ContentWarningKind::AuthToken => {
                (StyledText::new(dc_intl::t("sTwS1a").to_owned()), None)
            }
        };
        let esc_hint = format_bold_styled("0LzVPZ", &[]);
        let enter_hint = format_bold_styled("khjqdH", &[]);

        let card = div()
            .id("content-warning-popout")
            .occlude()
            .w(px(420.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .shadow_xl()
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
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
                            .gap(px(8.))
                            .child(icon("circle_info", 20., "--status-warning"))
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .line_height(px(22.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("mY3Y38")),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(body_styled),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .items_start()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .id("content-warning-edit")
                                            .w_full()
                                            .h(px(36.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-secondary-background-hover"))
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("fsBWmS"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.content_warning = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(color("--text-muted"))
                                            .child(esc_hint),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .id("content-warning-send")
                                            .w_full()
                                            .h(px(36.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-primary-background-hover"))
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("KJnHq3"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.confirm_content_warning(cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(color("--text-muted"))
                                            .child(enter_hint),
                                    ),
                            ),
                    ),
            )
            .when_some(footer_text, |col, footer| {
                col.child(
                    div()
                        .px(px(20.))
                        .py(px(12.))
                        .bg(color("--background-base-lower"))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("circle_info", 14., "--text-muted"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(footer),
                        ),
                )
            });

        Some(
            deferred(
                div()
                    .id("content-warning-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x00000044))
                    .flex()
                    .items_end()
                    .justify_center()
                    .pb(px(84.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.content_warning = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }

    /// Capture harness: `content-warning [everyone|token|close]` (module 926262).
    #[cfg(feature = "capture")]
    pub(crate) fn script_content_warning(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.content_warning = None;
                cx.notify();
                "content-warning: closed".to_owned()
            }
            "token" => {
                self.content_warning = Some(ContentWarningPopout {
                    kind: ContentWarningKind::AuthToken,
                    pending_content: "sample token".to_owned(),
                });
                cx.notify();
                "content-warning: token".to_owned()
            }
            _ => {
                self.content_warning = Some(ContentWarningPopout {
                    kind: ContentWarningKind::MentionEveryone {
                        role: "@everyone",
                        count: 100,
                        is_post: false,
                        is_thread: false,
                    },
                    pending_content: "@everyone meeting soon!".to_owned(),
                });
                cx.notify();
                "content-warning: everyone".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_everyone_and_ignores_code_spans() {
        assert_eq!(
            detect_content_warning("hi @everyone", 150, false, false),
            Some(ContentWarningKind::MentionEveryone {
                role: "@everyone",
                count: 100,
                is_post: false,
                is_thread: false,
            })
        );
        assert_eq!(
            detect_content_warning("use `@everyone` in code", 150, false, false),
            None
        );
        assert_eq!(
            detect_content_warning("hi @everyone", 15, false, false),
            None
        );
    }

    #[test]
    fn detects_discord_auth_token_shape() {
        let fake = "MTIzNDU2Nzg5MDEyMzQ1Njc4.AbCdEf.0123456789abcdef0123456789abcdef";
        assert_eq!(
            detect_content_warning(fake, 5, false, false),
            Some(ContentWarningKind::AuthToken)
        );
        assert_eq!(
            detect_content_warning("hello.world.test", 5, false, false),
            None
        );
    }
}
