//! The header of a compact message sits on the first line of its text (stylesheet 738015: `.compact .contents` is
//! `padding-inline-start: 5rem` with `text-indent: calc((5rem - 1rem) * -1)`, `.compact .header` and
//! `.compact .messageContent` are `display: inline`): the time, the avatar and the name start 1rem in, the text
//! follows the name on the same line, and every line after the first starts 5rem in.
//!
//! The header is drawn as its own row over the text; the text's first line starts where the header ends. That width
//! is measured when the text is laid out (time and name shaped as drawn) and corrected by the width the header
//! really had, recorded when it is painted (name styles and decorations draw it wider than the shaped name).

use crate::inline::FirstLineIndent;
use dc_state::Id;
use gpui::{Font, FontWeight, Pixels, SharedString, TextRun, Window, px};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// `--custom-message-margin-compact-indent` (5rem) less the 1rem the header starts at.
pub(super) const TEXT_START: f32 = 64.;

thread_local! {
    /// The painted width of each compact header, by message.
    static WIDTHS: RefCell<HashMap<Id, f32>> = RefCell::new(HashMap::new());
}

/// What the header holds, to measure it before it is painted.
#[derive(Clone)]
pub(super) struct Lead {
    pub message: Id,
    pub font: Font,
    pub time: String,
    /// The time's box (`2.25rem`, `3.1rem` at 24 hours) and the `0.25rem` after it.
    pub time_min: f32,
    /// `1em` with `.1em` before and `.25em` after when "Show user avatars" is on.
    pub avatar: Option<f32>,
    pub name: String,
    pub size: f32,
}

fn width(window: &Window, font: &Font, text: &str, size: f32, weight: f32) -> f32 {
    let mut font = font.clone();
    font.weight = FontWeight(weight);
    let run = TextRun {
        len: text.len(),
        font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(SharedString::from(text.to_owned()), px(size), &[run], None)
            .width,
    )
}

/// The header's width as shaped: the time box and its margin, the avatar, the name and its `0.25rem`.
pub(super) fn estimate(window: &Window, lead: &Lead) -> f32 {
    let time = width(window, &lead.font, &lead.time, 11., 500.).max(lead.time_min) + 4.;
    let avatar = lead.avatar.map_or(0., |size| size * 1.35);
    let name = width(window, &lead.font, &lead.name, lead.size, 500.) + 4.;
    time + avatar + name
}

/// How far in the text's first line starts, from the 5rem line start.
pub(super) fn indent(lead: Lead) -> FirstLineIndent {
    Rc::new(move |window: &Window| -> Pixels {
        let known = WIDTHS.with(|widths| widths.borrow().get(&lead.message).copied());
        let header = known.unwrap_or_else(|| {
            let shaped = estimate(window, &lead);
            WIDTHS.with(|widths| widths.borrow_mut().insert(lead.message, shaped));
            shaped
        });
        px((header - TEXT_START).max(0.))
    })
}

/// Keeps the painted width of a header; true when it differs from the one the text was laid out with (the shaped
/// estimate the first time).
pub(super) fn record(message: Id, header: f32) -> bool {
    WIDTHS.with(|widths| {
        let mut widths = widths.borrow_mut();
        if widths.len() > 4096 {
            widths.clear();
        }
        let before = widths.insert(message, header);
        before.is_none_or(|before| (before - header).abs() > 0.5)
    })
}

impl super::Root {
    /// What the compact header of `message` holds (as `row` draws it).
    pub(super) fn compact_lead_of(&self, message: &dc_state::Message) -> Lead {
        let mut font = gpui::font(super::SANS);
        font.features = super::kerning();
        let size = self.chat_font_size();
        Lead {
            message: message.id,
            font,
            time: super::settings::prefs::clock(&message.local_time()),
            time_min: if super::settings::prefs::clock_24() {
                49.6
            } else {
                36.
            },
            avatar: self
                .pref_on("appearance_display_compact_avatars", true)
                .then_some(size),
            name: self.guild_display_name(&message.author),
            size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_is_recorded_once_until_its_width_changes() {
        assert!(record(1, 120.));
        assert!(!record(1, 120.2));
        assert!(record(1, 130.));
    }
}
