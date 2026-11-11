//! The Quick Bar: `I7` in module 596339, styled by module 442420 (`quickBar_y2dW7E`). It sits in the bottom-left stack
//! above the account panel and shows the newest clips of the game being played. A thumbnail opens the editor and the
//! fourth of a longer session opens the gallery in the official client; neither view exists here yet, so the
//! thumbnails do nothing when pressed.
use super::super::{Root, color};
use crate::parity::Parity;
use dc_clips::library::Clip;
use gpui::{AnyElement, Context, FontWeight, ObjectFit, RenderImage, div, img, prelude::*, px};
use std::sync::Arc;

/// Whether the bar is drawn at all.
const QUICK_BAR_PROVEN: bool = false;

/// Slots in the bar (`slice(0, 4)` padded to four).
const SLOTS: usize = 4;

/// The clip picture from its data URL, as an image the renderer takes (BGRA). `None` when there is none or it does
/// not decode.
fn decode(data_url: &str) -> Option<Arc<RenderImage>> {
    use base64::Engine as _;
    let payload = data_url.strip_prefix("data:")?.split_once(";base64,")?.1;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;
    let mut rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
}

/// Whether `clip` belongs to `game` (`e.applicationId === visibleGame.id`; a game with no id is told by its name).
fn belongs(clip: &Clip, game: &crate::clips_capture::Game) -> bool {
    clip.meta.application_id == game.application_id
        && (game.application_id.is_some() || clip.meta.application_name == game.name)
}

impl Root {
    /// The new clips of the visible game, newest first (`getLastClipsSession().clipIds` of that application, sorted
    /// `MOST_RECENT`).
    fn clips_quick_bar_clips(&self) -> Vec<&Clip> {
        let engine = &self.clips.engine;
        let Some(game) = engine.game.as_ref() else {
            return Vec::new();
        };
        let mut clips: Vec<&Clip> = engine
            .new_clip_ids
            .iter()
            .filter_map(|id| engine.clips.get(id))
            .filter(|clip| belongs(clip, game))
            .collect();
        clips.sort_by_key(|clip| std::cmp::Reverse(clip.meta.created_at));
        clips
    }

    /// `canShowReminders()` and a clip of the visible game in this session.
    pub(in crate::ui) fn clips_quick_bar_has_clips(&self) -> bool {
        self.clips.reminders_enabled() && !self.clips_quick_bar_clips().is_empty()
    }

    /// The Quick Bar, or nothing: shown while it is open, reminders are on and the visible game has a clip.
    pub(in crate::ui) fn clips_panels(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
        // Not shown until its frame has been compared with the official bar.
        if !QUICK_BAR_PROVEN
            || !self.clips.engine.ui.quick_bar_open
            || !self.clips_quick_bar_has_clips()
        {
            return None;
        }
        let clips = self.clips_quick_bar_clips();
        let more = clips.len() > SLOTS;
        let mut slots = Vec::with_capacity(SLOTS);
        for index in 0..SLOTS {
            // `.WK`: flex 1 1 0, 47px, `--radius-sm` (8px, module 952007), `--background-mod-subtle`, opacity .9.
            let slot = div()
                .flex_1()
                .min_w(px(0.))
                .h(px(47.))
                .rounded(px(8.))
                .overflow_hidden()
                .relative()
                .bg(color("--background-mod-subtle"))
                .opacity(0.9);
            let Some(clip) = clips.get(index) else {
                slots.push(slot.into_any_element());
                continue;
            };
            let picture = self
                .clips
                .engine
                .ui
                .pictures
                .borrow_mut()
                .entry(clip.meta.id.clone())
                .or_insert_with(|| decode(&clip.meta.thumbnail))
                .clone();
            let slot = slot.children(picture.map(|picture| {
                img(picture)
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element()
            }));
            // The fourth slot of a longer session counts the rest: `+{length - 3}` (`N7`: black 45%, centred).
            let slot = if index == SLOTS - 1 && more {
                slot.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .bg(gpui::rgba(0x0000_0073))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-overlay-light"))
                        .child(format!("+{}", clips.len() - 3)),
                )
            } else {
                slot
            };
            slots.push(slot.into_any_element());
        }
        Some(
            div()
                .parity("clips-quick-bar")
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(12.))
                .bg(color("--background-surface-high"))
                .border_b_1()
                .border_color(color("--border-subtle"))
                .children(slots)
                .into_any_element(),
        )
    }
}
