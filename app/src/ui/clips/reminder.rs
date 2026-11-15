//! The clips reminder panel (module 596339 `pm`, wrapper `pa`/`ps`, thumbnail stack `pE`; store 468550; stylesheets
//! dfLAdA the wrapper, UQqFnR the panel, KeBYth the stack, 0YKZJi the countdown bar): opened when a game session stops
//! with clips of it in the session, it counts down one minute (paused while the pointer is over it) and offers the
//! gallery.
use super::super::motion::{self, Ease};
use super::super::{Root, color, icon};
use crate::parity::Parity;
use dc_clips::library::Clip;
use gpui::{
    AnyElement, Context, FontWeight, ObjectFit, SharedString, div, img, prelude::*, px, relative,
};

/// `ph`: one minute.
pub(super) const DURATION_MS: f64 = 60_000.;
/// `.full-motion .wrapper`: height, opacity and transform take .22 s.
const FADE_MS: f64 = 220.;
/// `thumbnailSlideUp`: .6 s `cubic-bezier(.24,.35,.35,1)`.
const SLIDE_MS: f64 = 600.;
const SLIDE_EASE: Ease = Ease::Bezier(0.24, 0.35, 0.35, 1.);

/// The reminder store (`isOpen`, `applicationId`) with its clock.
pub(in crate::ui) struct Reminder {
    pub app_id: String,
    /// Motion-clock milliseconds when it opened.
    pub opened_at: f64,
    /// Time spent under the pointer so far, and since when the pointer is over it.
    pub paused_ms: f64,
    pub hover_since: Option<f64>,
    /// Set when it started to leave (dismissed or timed out).
    pub closing_at: Option<f64>,
}

impl Reminder {
    pub(super) fn new(app_id: String, now: f64) -> Self {
        Reminder {
            app_id,
            opened_at: now,
            paused_ms: 0.,
            hover_since: None,
            closing_at: None,
        }
    }

    /// How much of the minute has run (`Ep` timer: it stops while hovered and resumes with what is left).
    pub(super) fn elapsed(&self, now: f64) -> f64 {
        let paused = self.paused_ms + self.hover_since.map_or(0., |since| (now - since).max(0.));
        (now - self.opened_at - paused).max(0.)
    }

    pub(super) fn timed_out(&self, now: f64) -> bool {
        self.elapsed(now) >= DURATION_MS
    }

    pub(super) fn set_hovered(&mut self, hovered: bool, now: f64) {
        match (hovered, self.hover_since) {
            (true, None) => self.hover_since = Some(now),
            (false, Some(since)) => {
                self.paused_ms += (now - since).max(0.);
                self.hover_since = None;
            }
            _ => {}
        }
    }

    /// The panel is gone once its exit finished.
    pub(super) fn finished(&self, now: f64) -> bool {
        self.closing_at.is_some_and(|at| now - at >= FADE_MS)
    }
}

/// One thumbnail's pose (`pd`, `pc`): offsets in px from its slot, at rest and hovered, and its place in the stack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pose {
    pub slot: Slot,
    pub x: f32,
    pub y: f32,
    pub hover_x: f32,
    pub hover_y: f32,
    /// The entrance order (`h.indexOf`): the one drawn first comes in first.
    pub enter_rank: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Middle,
    Left,
    Right,
}

/// The poses of a stack of `count` clips (1 to 3), in the order they are drawn (lowest z-index first). Rotations
/// (right 6 deg, left -8 deg, two clips 7 and -9 deg) are not drawn: a box cannot be turned here.
pub(super) fn poses(count: usize) -> Vec<(usize, Pose)> {
    let count = count.min(3);
    let pose = |slot, x, y, hover_x, hover_y, enter_rank| Pose {
        slot,
        x,
        y,
        hover_x,
        hover_y,
        enter_rank,
    };
    match count {
        0 => Vec::new(),
        1 => vec![(0, pose(Slot::Middle, 0., -10., 0., -18., 0))],
        2 => vec![
            (1, pose(Slot::Middle, -30., 2., -40., 2., 0)),
            (0, pose(Slot::Middle, 30., 0., 40., -8., 1)),
        ],
        _ => vec![
            (2, pose(Slot::Left, 0., 10., -10., 10., 0)),
            (1, pose(Slot::Right, 0., 10., 10., 10., 1)),
            (0, pose(Slot::Middle, 0., -10., 0., -18., 2)),
        ],
    }
}

/// `100 + 140 * rank` ms.
fn enter_delay(rank: usize) -> f64 {
    100. + 140. * rank as f64
}

/// How far down a thumbnail still is, in px, `ms` after the panel opened (`thumbnailSlideUp`: from 300 px below).
pub(super) fn slide_offset(ms: f64, rank: usize) -> f32 {
    let t = ((ms - enter_delay(rank)) / SLIDE_MS).clamp(0., 1.);
    (300. * (1. - SLIDE_EASE.apply(t))) as f32
}

/// `{count} new clip(s)` (`2rwMjL`).
pub(super) fn count_text(count: usize) -> String {
    if count == 1 {
        "1 new clip".to_owned()
    } else {
        format!("{count} new clips")
    }
}

/// Matches a clip or running game to a reminder session by application ID or custom-game name.
pub(in crate::ui) fn clip_session_key(app_id: Option<&str>, name: &str) -> Option<String> {
    if let Some(id) = app_id.filter(|id| !id.is_empty()) {
        return Some(id.to_owned());
    }
    let trimmed = name.trim();
    (!trimmed.is_empty()).then(|| format!("name:{}", trimmed.to_ascii_lowercase()))
}

impl Root {
    /// `handleClipsReminder(gameId)`: opens the panel when reminders are on and the last session has a clip of the game.
    pub(in crate::ui) fn clips_session_stop(
        &mut self,
        app_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(app_id) = app_id else {
            return;
        };
        if !self.clips.reminders_enabled() {
            return;
        }
        let engine = &self.clips.engine;
        let has = engine.new_clip_ids.iter().any(|id| {
            engine.clips.get(id).is_some_and(|clip| {
                clip.meta.application_id.as_deref() == Some(app_id.as_str())
                    || clip_session_key(
                        clip.meta.application_id.as_deref(),
                        &clip.meta.application_name,
                    )
                    .as_deref()
                        == Some(app_id.as_str())
            })
        });
        if has {
            self.clips.engine.ui.reminder = Some(Reminder::new(app_id, motion::now()));
            self.clips.engine.ui.quick_bar_open = false;
            cx.notify();
        }
    }

    /// `CLIPS_SESSION_START` without a previous game: the panel closes at once.
    pub(in crate::ui) fn clips_session_start(&mut self, cx: &mut Context<Self>) {
        if self.clips.engine.ui.reminder.take().is_some() {
            cx.notify();
        }
    }

    /// `kF`: the panel starts to leave.
    pub(super) fn clips_reminder_dismiss(&mut self, cx: &mut Context<Self>) {
        if let Some(reminder) = self.clips.engine.ui.reminder.as_mut()
            && reminder.closing_at.is_none()
        {
            reminder.closing_at = Some(motion::now());
        }
        cx.notify();
    }

    /// Runs with the clips tick: times the panel out and removes it when it has left.
    pub(in crate::ui) fn clips_reminder_tick(&mut self, cx: &mut Context<Self>) {
        let now = motion::now();
        let Some(reminder) = self.clips.engine.ui.reminder.as_mut() else {
            return;
        };
        if reminder.closing_at.is_none() && reminder.timed_out(now) {
            reminder.closing_at = Some(now);
        }
        if reminder.finished(now) {
            self.clips.engine.ui.reminder = None;
        }
        cx.notify();
    }

    /// The clips the panel lists: the session's clips of its game, newest first.
    fn clips_reminder_clips(&self, app_id: &str) -> Vec<&Clip> {
        let engine = &self.clips.engine;
        let mut clips: Vec<&Clip> = engine
            .new_clip_ids
            .iter()
            .filter_map(|id| engine.clips.get(id))
            .filter(|clip| {
                clip.meta.application_id.as_deref() == Some(app_id)
                    || clip_session_key(
                        clip.meta.application_id.as_deref(),
                        &clip.meta.application_name,
                    )
                    .as_deref()
                        == Some(app_id)
            })
            .collect();
        clips.sort_by_key(|clip| std::cmp::Reverse(clip.meta.created_at));
        clips
    }

    pub(in crate::ui) fn clips_reminder_open(&self) -> bool {
        self.clips.engine.ui.reminder.is_some()
    }

    /// The panel (`pm` in `ps`): the countdown bar, the dismiss button, the stack, the game and the button.
    pub(in crate::ui) fn clips_reminder_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let reminder = self.clips.engine.ui.reminder.as_ref()?;
        let clips = self.clips_reminder_clips(&reminder.app_id);
        if clips.is_empty() {
            return None;
        }
        let now = motion::now();
        motion::keep_animating();
        let since_open = now - reminder.opened_at;
        let leaving = reminder
            .closing_at
            .map(|at| ((now - at) / FADE_MS).clamp(0., 1.))
            .unwrap_or(0.);
        let entering = (since_open / FADE_MS).clamp(0., 1.);
        let opacity = (Ease::Ease.apply(entering) * (1. - leaving)) as f32;
        let remaining = (1. - reminder.elapsed(now) / DURATION_MS).clamp(0., 1.) as f32;
        let hovered = reminder.hover_since.is_some();
        let shown: Vec<&Clip> = clips.iter().take(3).copied().collect();
        let name = clips[0].meta.application_name.clone();
        let backdrop = super::panels::decode(&shown[0].meta.thumbnail);
        let mut stack = div().relative().h(px(96.)).w_full();
        for (index, pose) in poses(shown.len()) {
            let picture = self
                .clips
                .engine
                .ui
                .pictures
                .borrow_mut()
                .entry(shown[index].meta.id.clone())
                .or_insert_with(|| super::panels::decode(&shown[index].meta.thumbnail))
                .clone();
            let (x, y) = if hovered {
                (pose.hover_x, pose.hover_y)
            } else {
                (pose.x, pose.y)
            };
            // 150 wide, 16:9, centred in the 96 px band, `--radius-sm`, 1 px `--border-normal`, drop shadow.
            let slide = slide_offset(since_open, pose.enter_rank)
                + if leaving > 0. {
                    72. * leaving as f32
                } else {
                    0.
                };
            let top = (96. - 150. * 9. / 16.) / 2. + y + slide;
            let thumb = div()
                .absolute()
                .top(px(top))
                .w(px(150.))
                .h(px(150. * 9. / 16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-normal"))
                .overflow_hidden()
                .bg(color("--background-mod-subtle"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.2),
                    offset: gpui::point(px(0.), px(2.)),
                    blur_radius: px(4.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .children(
                    picture.map(|picture| img(picture).size_full().object_fit(ObjectFit::Cover)),
                );
            stack = stack.child(match pose.slot {
                Slot::Middle => thumb.left(relative(0.5)).ml(px(-75. + x)),
                Slot::Left => thumb.left(px(x)),
                Slot::Right => thumb.right(px(-x)),
            });
        }
        // `_`: `openClipsGallery()` then `kF()`.
        let review_stack = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.clips_open_gallery(super::Facet::All, cx);
            this.clips_reminder_dismiss(cx);
        });
        let review_button = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.clips_open_gallery(super::Facet::All, cx);
            this.clips_reminder_dismiss(cx);
        });
        let letter: String = name.chars().take(1).collect::<String>().to_uppercase();
        Some(
            div()
                .id("clips-reminder")
                .parity("clips-reminder")
                .w_full()
                .flex_none()
                .relative()
                .overflow_hidden()
                .opacity(opacity)
                .bg(color("--background-surface-high"))
                .border_b_1()
                .border_color(color("--border-muted"))
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if let Some(reminder) = this.clips.engine.ui.reminder.as_mut() {
                        reminder.set_hovered(*hovered, motion::now());
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .px(px(12.))
                        .pb(px(12.))
                        // The countdown bar: 4 px, `--background-mod-subtle` with the indicator shrinking to the left.
                        .child(
                            div()
                                .h(px(4.))
                                .w_full()
                                .rounded_b(px(4.))
                                .overflow_hidden()
                                .bg(color("--background-mod-subtle"))
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(remaining))
                                        .bg(color("--progressbar-indicator-background")),
                                ),
                        )
                        // `controls`: the dismiss button (`9CE7R5`), an `xs` icon-only button.
                        .child(
                            div().flex().justify_end().child(
                                div()
                                    .id("clips-reminder-dismiss")
                                    .size(px(24.))
                                    .rounded(px(6.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(|style| {
                                        style.bg(color("--control-icon-only-background-hover"))
                                    })
                                    .child(icon(
                                        "clips_x_small",
                                        16.,
                                        "--control-icon-only-icon-default",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clips_reminder_dismiss(cx)
                                    })),
                            ),
                        )
                        .child(
                            div()
                                .relative()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                // The blurred first thumbnail behind, faded downward (blur is not drawn).
                                .children(backdrop.map(|picture| {
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .w_full()
                                        .h(relative(0.7))
                                        .opacity(0.2)
                                        .child(
                                            img(picture).size_full().object_fit(ObjectFit::Cover),
                                        )
                                        // `mask-image: linear-gradient(180deg, #000, transparent)`.
                                        .child(div().absolute().inset_0().bg(
                                            gpui::linear_gradient(
                                                0.,
                                                gpui::linear_color_stop(
                                                    color("--background-surface-high"),
                                                    0.,
                                                ),
                                                gpui::linear_color_stop(
                                                    gpui::rgba(0x0000_0000),
                                                    1.,
                                                ),
                                            ),
                                        ))
                                }))
                                .child(
                                    div()
                                        .id("clips-reminder-stack")
                                        .mx(px(-12.))
                                        .cursor_pointer()
                                        .on_click(review_stack)
                                        .child(stack),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .size(px(20.))
                                                        .rounded(px(8.))
                                                        .border_1()
                                                        .border_color(color("--border-subtle"))
                                                        .bg(color("--background-mod-strong"))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-default"))
                                                        .child(letter),
                                                )
                                                .child(
                                                    div()
                                                        .overflow_hidden()
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(name),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(color("--text-muted"))
                                                .child(count_text(clips.len())),
                                        ),
                                )
                                .child(
                                    super::widgets::button(
                                        SharedString::from("clips-reminder-review"),
                                        "Review them",
                                        None,
                                        super::widgets::Variant::Secondary,
                                        super::widgets::Size::Sm,
                                        true,
                                    )
                                    .on_click(review_button),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minute_stops_while_the_pointer_is_over_the_panel() {
        let mut reminder = Reminder::new("1".into(), 1000.);
        assert_eq!(reminder.elapsed(11_000.), 10_000.);
        reminder.set_hovered(true, 11_000.);
        assert_eq!(reminder.elapsed(31_000.), 10_000.);
        reminder.set_hovered(false, 31_000.);
        assert_eq!(reminder.elapsed(41_000.), 20_000.);
        assert!(!reminder.timed_out(41_000.));
        assert!(reminder.timed_out(1000. + 20_000. + 60_000.));
    }

    #[test]
    fn the_panel_is_gone_a_fade_after_it_starts_to_leave() {
        let mut reminder = Reminder::new("1".into(), 0.);
        assert!(!reminder.finished(1e9));
        reminder.closing_at = Some(100.);
        assert!(!reminder.finished(300.));
        assert!(reminder.finished(320.));
    }

    #[test]
    fn stacks_follow_the_official_poses() {
        assert_eq!(poses(0).len(), 0);
        let three = poses(3);
        assert_eq!(
            three.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            [2, 1, 0]
        );
        assert_eq!(three[2].1.slot, Slot::Middle);
        assert_eq!(three[2].1.hover_y, -18.);
        let two = poses(2);
        assert_eq!(two[1].1.x, 30.);
        assert_eq!(two[0].1.x, -30.);
        assert_eq!(poses(5).len(), 3);
    }

    #[test]
    fn thumbnails_slide_in_after_their_delay() {
        assert_eq!(slide_offset(0., 0), 300.);
        assert_eq!(slide_offset(100., 0), 300.);
        assert_eq!(slide_offset(380., 2), 300.);
        assert_eq!(slide_offset(2000., 2), 0.);
        assert!(slide_offset(300., 0) < 300.);
        assert!(slide_offset(300., 0) < slide_offset(300., 1));
    }

    #[test]
    fn the_count_reads_singular_and_plural() {
        assert_eq!(count_text(1), "1 new clip");
        assert_eq!(count_text(4), "4 new clips");
    }
}
