//! Capture-only comparison sheet for display name styles: one name drawn with a chosen font, effect and colours in
//! the three places the official style editor's preview shows it (the profile card's heading, a message header, a
//! member row), each at the position the official capture has it (Chromium puts a half-pixel position on the next
//! whole pixel, so the sheet does the same), on plain backgrounds. Nothing is stored or sent.
//!
//! `name-sheet <font id> <effect id> <rrggbb[,rrggbb...]> [at <ms>] [bg <card> <message> <row>]`, `name-sheet off`.
//! The official preview draws the card and the row animated (module 927071 `shouldAnimate`, 839964 `ANIMATED`) and
//! the message header with the font alone (315934 `PLAIN` while the pointer is elsewhere); `at <ms>` stops the
//! animation clock at that many milliseconds so a captured frame can be matched.

use super::{Display, StyledName, name};
use crate::ui::{Root, color};
use dc_state::display_name_styles::DisplayNameStyles;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ParentElement as _, Rgba, Styled as _, div, px,
};
use std::sync::Mutex;

/// The text the official capture's preview shows.
const TEXT: &str = "onugeccen";

struct Sheet {
    styles: DisplayNameStyles,
    backgrounds: [u32; 3],
}

fn sheet() -> &'static Mutex<Option<Sheet>> {
    static SHEET: Mutex<Option<Sheet>> = Mutex::new(None);
    &SHEET
}

fn hex(word: &str) -> Option<u32> {
    u32::from_str_radix(word.trim_start_matches('#'), 16).ok()
}

/// One place of the sheet: a plain panel with the name at an exact position, in the place's text style.
fn place(
    panel: (f32, f32, f32, f32),
    at: (f32, f32),
    text: (f32, f32, f32),
    background: u32,
    name: StyledName,
) -> AnyElement {
    let (left, top, width, height) = panel;
    let (size, line, weight) = text;
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .bg(Rgba {
            r: f32::from((background >> 16) as u8) / 255.,
            g: f32::from((background >> 8) as u8) / 255.,
            b: f32::from(background as u8) / 255.,
            a: 1.,
        })
        .child(
            div()
                .absolute()
                .left(px(at.0 - left))
                .top(px(at.1 - top))
                .text_size(px(size))
                .line_height(px(line))
                .font_weight(FontWeight(weight))
                .text_color(color("--text-strong"))
                .child(name),
        )
        .into_any_element()
}

/// The sheet over the whole window, when a step turned it on.
pub(in crate::ui) fn layer() -> Option<AnyElement> {
    let sheet = sheet().lock().ok()?;
    let sheet = sheet.as_ref()?;
    let styles = Some(&sheet.styles);
    let [card, message, row] = sheet.backgrounds;
    Some(
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .w(px(1280.))
            .h(px(720.))
            .bg(color("--background-base-lowest"))
            .child(place(
                (660., 280., 240., 80.),
                (702., 314.),
                (20., 24., 600.),
                card,
                name("sheet-card", TEXT, styles, Display::Animated),
            ))
            .child(place(
                (740., 360., 240., 70.),
                (780., 387.),
                (16., 22., 500.),
                message,
                name("sheet-message", TEXT, styles, Display::Font),
            ))
            .child(place(
                (690., 430., 240., 70.),
                (737., 459.),
                (16., 20., 500.),
                row,
                name("sheet-row", TEXT, styles, Display::Animated),
            ))
            .into_any_element(),
    )
}

impl Root {
    /// Capture harness step `name-sheet`: see the module comment.
    pub(crate) fn script_name_sheet(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let mut state = sheet().lock().expect("the sheet is not poisoned");
        if words == ["off"] {
            *state = None;
            super::set_frozen(None);
            cx.notify();
            return "name sheet: off".to_owned();
        }
        let [font, effect, colors, rest @ ..] = words else {
            return "name sheet: usage: name-sheet <font id> <effect id> <colours> [at <ms>] [bg a b c]"
                .to_owned();
        };
        let (Ok(font_id), Ok(effect_id)) = (font.parse::<u32>(), effect.parse::<u32>()) else {
            return "name sheet: bad font or effect id".to_owned();
        };
        let colors: Vec<u32> = colors.split(',').filter_map(hex).collect();
        let mut backgrounds = [0x1e1f22; 3];
        let mut frozen = None;
        let mut rest = rest.iter();
        while let Some(word) = rest.next() {
            match *word {
                "at" => frozen = rest.next().and_then(|ms| ms.parse::<f32>().ok()),
                "bg" => {
                    for slot in &mut backgrounds {
                        if let Some(value) = rest.next().and_then(|word| hex(word)) {
                            *slot = value;
                        }
                    }
                }
                _ => {}
            }
        }
        super::set_frozen(frozen.map(|ms| ms / 1000.));
        *state = Some(Sheet {
            styles: DisplayNameStyles {
                font_id,
                effect_id,
                colors,
            },
            backgrounds,
        });
        cx.notify();
        format!("name sheet: font {font_id} effect {effect_id} frozen={frozen:?}")
    }
}
