//! The thread under the message that started it (module 679636 `r5`/`r6`, CSS `container_g2fiji` of stylesheet
//! 439120): its name and "N Messages ›" over the thread's latest message (module 679636 `r0`, the reply line of module
//! 448368 `V` in `threadMessageAccessory_2i8Bfm`), or why there is none, with how long ago it was active.

use super::reply_content::{self, Trailing};
use super::{Root, color, first_spans, icon};
use dc_state::{Id, Message, Thread};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// `HAS_THREAD`.
const HAS_THREAD: u64 = 32;

/// Module 321045 `Mw`: the count as the label writes it; past 50 for threads from before the count was kept
/// (`992549565104128000`), and past 100 000, a capped figure.
pub(super) fn message_count(count: u64, thread: Id) -> String {
    const COUNTED_SINCE: Id = 992_549_565_104_128_000;
    if thread <= COUNTED_SINCE && count >= 50 {
        "50+".to_owned()
    } else if count >= 100_000 {
        "100k+".to_owned()
    } else {
        count.to_string()
    }
}

/// Module 321045 `ub`: "N Messages ›", or "See Thread ›" without any.
pub(super) fn cta(count: u64, thread: Id) -> String {
    let shown = message_count(count, thread);
    if shown == "0" {
        return dc_intl::t("eXHkhl").to_owned();
    }
    match shown.parse::<u64>() {
        Ok(number) => dc_intl::t!("rfAXDV", count = number),
        Err(_) => dc_intl::t!("rfAXDV", count = shown.as_str()),
    }
}

/// What the bottom line of `r6` shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Bottom {
    /// `ZTo4HS`, alone: the thread is archived.
    Archived,
    /// `HYtNyE`: nobody wrote in it.
    Empty,
    /// `ZTo4HS` and the time: messages the client has not seen.
    Unseen,
    /// The latest message and the time.
    Latest,
}

pub(super) fn bottom(archived: bool, count: u64, latest: bool) -> Bottom {
    if archived {
        Bottom::Archived
    } else if count == 0 {
        Bottom::Empty
    } else if latest {
        Bottom::Latest
    } else {
        Bottom::Unseen
    }
}

impl Root {
    /// The thread a message started: the store's, else the copy the message carries.
    fn started_thread(&self, message: &Message) -> Option<Thread> {
        self.current_guild()
            .and_then(|guild| guild.threads.iter().find(|thread| thread.id == message.id))
            .cloned()
            .or_else(|| message.extra.thread.clone())
    }

    /// Module 679636 `renderThreadAccessories`: a message flagged `HAS_THREAD` shows its thread.
    pub(super) fn thread_embed(
        &self,
        message: &Message,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if message.flags & HAS_THREAD == 0 {
            return None;
        }
        let thread = self.started_thread(message)?;
        let latest = self
            .stores
            .thread_messages
            .most_recent_message(thread.id)
            .cloned();
        let now = chrono::Utc::now().timestamp_millis();
        // `JO`: the time of the last message, else the thread's own.
        let active_ms = |snowflake: Id| ((snowflake >> 22) + 1_420_070_400_000) as i64;
        let active = active_ms(thread.last_message_id.unwrap_or(thread.id));
        let time = || {
            div()
                .flex_none()
                .ml(px(8.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(super::threads::short_ago(now - active))
        };
        let line = match bottom(thread.archived, thread.message_count, latest.is_some()) {
            Bottom::Archived => div().child(dc_intl::t("ZTo4HS")).into_any_element(),
            Bottom::Empty => div().child(dc_intl::t("HYtNyE")).into_any_element(),
            Bottom::Unseen => div()
                .flex()
                .items_center()
                .min_w(px(0.))
                .child(div().min_w(px(0.)).truncate().child(dc_intl::t("ZTo4HS")))
                .child(time())
                .into_any_element(),
            Bottom::Latest => div()
                .flex()
                .items_center()
                .min_w(px(0.))
                .children(
                    latest
                        .as_ref()
                        .map(|latest| self.thread_latest_line(latest)),
                )
                .child(time())
                .into_any_element(),
        };
        let (id, guild) = (thread.id, self.guild);
        let group = SharedString::from(format!("thread-embed-{id}"));
        // `systemMessageContainer_g2fiji` and `systemMessageSpine_g2fiji`: under a system message the box sits at
        // the line and the spine is shorter and nearer.
        let system = dc_state::message_kinds::is_system(message.kind);
        let (spine_left, spine_width, spine_bottom) = if system {
            (-32., 28., 32.)
        } else {
            (-40., 36., 29.)
        };
        Some(
            div()
                .relative()
                .flex()
                // `spine_g2fiji`: compact only, from the avatar column down into the box.
                .when(compact, |row| {
                    row.child(
                        div()
                            .absolute()
                            .left(px(spine_left))
                            .top_0()
                            .bottom(px(spine_bottom))
                            .w(px(spine_width))
                            .border_l_2()
                            .border_b_2()
                            .rounded_bl(px(8.))
                            .border_color(color("--border-subtle")),
                    )
                })
                .child(
                    div()
                        .id(group.clone())
                        .group(group.clone())
                        .when(!system, |container| container.mt(px(8.)))
                        .max_w(px(480.))
                        .min_w(px(0.))
                        .p(px(8.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-high"))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(guild) = guild {
                                this.select(guild, Some(id));
                            }
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .child(
                                    div()
                                        .mr(px(8.))
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_color(color("--text-strong"))
                                        .child(thread.name.clone()),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(color("--text-link"))
                                        .group_hover(group, |cta| cta.underline())
                                        .child(cta(thread.message_count, thread.id)),
                                ),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .flex()
                                .items_center()
                                .min_w(px(0.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(line),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Module 679636 `r0` (`threadMessageAccessory_2i8Bfm`, 18px): the 16px avatar 8px from the name (64% opacity,
    /// 4px after it), then the message as a reply line reads it.
    fn thread_latest_line(&self, latest: &Message) -> AnyElement {
        let avatar = self
            .images
            .get(&self.guild_avatar_source(&latest.author).0)
            .cloned();
        let line = reply_content::reply_line(self.reply_facts(latest));
        let text: AnyElement = match line.placeholder {
            None => self
                .preview_text(&first_spans(&latest.content), false)
                .into_any_element(),
            Some(placeholder) => div()
                .pr(px(2.))
                .italic()
                .whitespace_nowrap()
                .child(reply_content::placeholder_text(placeholder, latest))
                .into_any_element(),
        };
        let trailing = line.trailing.map(|kind| {
            let glyph = match kind {
                Trailing::Sticker => icon("sticker_small", 20., "--text-default"),
                Trailing::Command => icon("slash_box", 20., "--text-default"),
                Trailing::Voice => icon("cp_mic", 20., "--text-default")
                    .w(px(20. * 19. / 24.))
                    .h(px(20.)),
                Trailing::Image => icon("search_has_image", 20., "--text-default"),
            };
            div().ml(px(4.)).flex_none().child(glyph)
        });
        div()
            .h(px(18.))
            .flex()
            .items_center()
            .min_w(px(0.))
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .mr(px(8.))
                    .children(avatar.map(|image| img(image).size(px(16.)).rounded_full())),
            )
            .child(
                div()
                    .flex_none()
                    .mr(px(4.))
                    .opacity(0.64)
                    .font_weight(FontWeight(500.))
                    .text_color(self.guild_author_color(latest.author.id, "--text-strong"))
                    .child(
                        self.author_name(format!("thread-latest-{}", latest.id), &latest.author),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .when(line.forwarded, |row| {
                        row.child(div().mr(px(4.)).flex_none().child(icon(
                            "forwarded_icon",
                            20.,
                            "--interactive-icon-default",
                        )))
                    })
                    .child(div().min_w(px(0.)).overflow_hidden().child(text))
                    .children(trailing),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_label_caps_as_official() {
        assert_eq!(cta(0, 1_200_000_000_000_000_000), "See Thread \u{203a}");
        assert_eq!(cta(1, 1_200_000_000_000_000_000), "1 Message \u{203a}");
        assert_eq!(cta(7, 1_200_000_000_000_000_000), "7 Messages \u{203a}");
        assert_eq!(message_count(60, 900_000_000_000_000_000), "50+");
        assert_eq!(message_count(60, 1_200_000_000_000_000_000), "60");
        assert_eq!(message_count(100_000, 1_200_000_000_000_000_000), "100k+");
    }

    #[test]
    fn the_bottom_line_says_why_there_is_no_message() {
        assert_eq!(bottom(true, 5, true), Bottom::Archived);
        assert_eq!(bottom(false, 0, false), Bottom::Empty);
        assert_eq!(bottom(false, 3, false), Bottom::Unseen);
        assert_eq!(bottom(false, 3, true), Bottom::Latest);
    }
}
