//! The Keyboard Shortcuts help ("Keyboard Combos", `Ctrl+/`): module 453244 (the modal, the arrow pad and the Raging
//! Demon sequence), the list it shows from 158781 (`eN`, `em`, `eT`) and the key caps of 475358. Measured in
//! `design/dom/modals/keyboard-shortcuts-*-darker-cozy.json` (top, scrolled to 500, 1000, 1500 and the bottom) and
//! styled by `703942.*.css` (`keyboardShortcutsModal`, `keyboardShortcutListGroup`, `keybindGroup`, `ddrArrows`) and
//! `448400.*.css` (`combo`, `key`, `bindArrow`).
//!
//! The binds shown are the first bind of each action in `keyboard::TABLE`, so the help and the dispatcher cannot
//! disagree; the few the official layout lists without an action of the table (`any-character`, the arrows of the
//! easter egg, Decline incoming call) are written out as in 158781.

use super::{Action, TABLE};
use crate::parity::Parity;
use crate::ui::{Root, SANS, color, kerning};
use gpui::{
    AnyElement, Context, Font, FontWeight, IntoElement, ScrollHandle, SharedString, TextRun,
    Window, deferred, div, font, img, prelude::*, px, rgba,
};
use std::time::{Duration, Instant};

/// The four arrows of the pad, in the order the page lays them out (left, down, up, right).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Arrow {
    Left,
    Down,
    Up,
    Right,
}

const ARROWS: [Arrow; 4] = [Arrow::Left, Arrow::Down, Arrow::Up, Arrow::Right];

impl Arrow {
    fn index(self) -> usize {
        ARROWS.iter().position(|arrow| *arrow == self).unwrap_or(0)
    }

    /// The key the page reads it from (`F` in 453244).
    pub(in crate::ui) fn from_key(key: &str) -> Option<Arrow> {
        match key {
            "left" => Some(Arrow::Left),
            "right" => Some(Arrow::Right),
            "up" => Some(Arrow::Up),
            "down" => Some(Arrow::Down),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Arrow::Left => "ddr-left",
            Arrow::Down => "ddr-down",
            Arrow::Up => "ddr-up",
            Arrow::Right => "ddr-right",
        }
    }

    fn asset(self) -> &'static str {
        match self {
            Arrow::Left => "icons/ddr_left.svg",
            Arrow::Down => "icons/ddr_down.svg",
            Arrow::Up => "icons/ddr_up.svg",
            Arrow::Right => "icons/ddr_right.svg",
        }
    }
}

/// An arrow stays lit for 500 ms after a press or a click (`W` in 453244).
const LIT: Duration = Duration::from_millis(500);
/// The sound is throttled to one per 100 ms per arrow (`k` in 453244).
const SOUND_GAP: Duration = Duration::from_millis(100);

/// What the open help keeps: its scroller and the arrows that are lit.
pub(in crate::ui) struct Help {
    pub scroll: ScrollHandle,
    lit: [Option<Instant>; 4],
    sounded: [Option<Instant>; 4],
}

impl Help {
    pub(in crate::ui) fn new() -> Self {
        Help {
            scroll: ScrollHandle::new(),
            lit: [None; 4],
            sounded: [None; 4],
        }
    }

    fn is_lit(&self, arrow: Arrow) -> bool {
        self.lit[arrow.index()].is_some_and(|since| since.elapsed() < LIT)
    }

    /// `arrowDown`: the arrow lights up and its sound plays, at most every 100 ms.
    pub(in crate::ui) fn press(&mut self, arrow: Arrow) {
        let at = arrow.index();
        self.lit[at] = Some(Instant::now());
        if self.sounded[at].is_none_or(|since| since.elapsed() >= SOUND_GAP) {
            self.sounded[at] = Some(Instant::now());
            let _ = crate::sounds::play(arrow.name());
        }
    }
}

// ----- the list: 158781 `eN`, grouped by `group`, in the order of `B` in 453244 -----

/// One cap of a combo: text, or an arrow glyph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Cap {
    Text(String),
    Arrow(Arrow),
}

/// `475358`: a bind string is split on `+`; each part is a named key, or itself.
pub(in crate::ui) fn caps(bind: &str) -> Vec<Cap> {
    bind.split('+')
        .map(|part| match part {
            "ctrl" | "mod" => Cap::Text("ctrl".into()),
            "cmd" => Cap::Text("cmd".into()),
            "alt" => Cap::Text("alt".into()),
            "shift" => Cap::Text("Shift".into()),
            "up" => Cap::Arrow(Arrow::Up),
            "down" => Cap::Arrow(Arrow::Down),
            "left" => Cap::Arrow(Arrow::Left),
            "right" => Cap::Arrow(Arrow::Right),
            "pageup" => Cap::Text("Page Up".into()),
            "pagedown" => Cap::Text("Page Down".into()),
            "any-character" => Cap::Text("Any Key".into()),
            "plus" => Cap::Text("+".into()),
            "return" | "enter" => Cap::Text("Enter".into()),
            "escape" | "esc" => Cap::Text("Esc".into()),
            "backspace" => Cap::Text("Backspace".into()),
            other => Cap::Text(other.to_owned()),
        })
        .collect()
}

pub(in crate::ui) struct Item {
    pub description: &'static str,
    /// One combo per bind.
    pub binds: Vec<&'static str>,
}

pub(in crate::ui) struct Section {
    pub title: &'static str,
    /// `eT`: the line under the heading.
    pub note: Option<&'static str>,
    pub items: Vec<Item>,
}

/// `eg(...)`: the first bind of each action.
fn first(action: Action) -> &'static str {
    TABLE
        .iter()
        .find(|entry| entry.action == action)
        .and_then(|entry| entry.binds.first().copied())
        .unwrap_or("")
}

fn item(description: &'static str, binds: Vec<&'static str>) -> Item {
    Item { description, binds }
}

fn action_item(description: &'static str, actions: &[Action]) -> Item {
    item(
        description,
        actions.iter().map(|action| first(*action)).collect(),
    )
}

/// The sections in the order of `B` (Messages, Navigation, Voice and Video, Chat, Miscellaneous) with the entries
/// of each in the order of `eN`. `DND` is not in `B`, so the drag-and-drop shortcuts are not listed.
pub(in crate::ui) fn sections() -> Vec<Section> {
    use Action as A;
    vec![
        Section {
            title: "Messages",
            note: Some("These shortcuts work when focused on a message."),
            items: vec![
                item("Edit Message", vec!["e"]),
                item("Delete Message", vec!["backspace"]),
                item("Pin Message", vec!["p"]),
                item("Add Reaction", vec!["plus"]),
                item("Reply", vec!["r"]),
                item("Forward Message", vec!["f"]),
                item("Speak Message", vec!["s"]),
                item("Copy Text", vec!["mod+c"]),
                item("Mark Unread", vec!["alt+enter"]),
                item("Focus text area", vec!["escape"]),
            ],
        },
        Section {
            title: "Navigation",
            note: None,
            items: vec![
                action_item("Navigate between servers", &[A::ServerPrev, A::ServerNext]),
                action_item(
                    "Navigate between channels",
                    &[A::ChannelPrev, A::ChannelNext],
                ),
                action_item(
                    "Navigate forward and backward in page history",
                    &[A::NavigateBack, A::NavigateForward],
                ),
                action_item(
                    "Navigate between unread channels",
                    &[A::UnreadPrev, A::UnreadNext],
                ),
                action_item(
                    "Navigate between unread channels with mentions",
                    &[A::MentionChannelPrev, A::MentionChannelNext],
                ),
                action_item("Navigate to current call", &[A::JumpToCurrentCall]),
                action_item(
                    "Toggle between last server and DMs",
                    &[A::TogglePreviousGuild],
                ),
                action_item("Toggle QuickSwitcher", &[A::QuickSwitcherShow]),
                action_item("Create or join a server", &[A::CreateGuild]),
            ],
        },
        Section {
            title: "Voice and Video",
            note: None,
            items: vec![
                action_item("Toggle mute", &[A::ToggleMute]),
                action_item("Toggle deafen", &[A::ToggleDeafen]),
                action_item("Answer incoming call", &[A::CallAccept]),
                action_item("Decline incoming call", &[A::MarkChannelRead]),
                action_item("Start call in private message or group", &[A::CallStart]),
                action_item("Toggle soundboard", &[A::SearchSoundboard]),
            ],
        },
        Section {
            title: "Chat",
            note: None,
            items: vec![
                action_item("Mark server read", &[A::MarkServerRead]),
                action_item("Mark channel as read", &[A::MarkChannelRead]),
                action_item("Create a private group", &[A::CreateDmGroup]),
                action_item("Create a thread", &[A::CreateThread]),
                action_item("Toggle pins popout", &[A::ToggleChannelPins]),
                action_item("Toggle inbox popout", &[A::ToggleInbox]),
                action_item("Mark top inbox channel read", &[A::MarkTopInboxChannelRead]),
                action_item(
                    "Toggle channel member list or voice text chat",
                    &[A::ToggleUsers],
                ),
                action_item("Toggle emoji picker", &[A::SearchEmojis]),
                action_item("Toggle GIF picker", &[A::SearchGifs]),
                action_item("Toggle sticker picker", &[A::SearchStickers]),
                action_item("Scroll chat up or down", &[A::ScrollUp, A::ScrollDown]),
                action_item("Jump to oldest unread message", &[A::JumpToFirstUnread]),
                item("Focus text area", vec!["any-character"]),
                action_item("Record a voice message", &[A::RecordVoiceMessage]),
                action_item("Upload a file", &[A::UploadFile]),
                action_item("Copy channel link", &[A::CopyChannelLink]),
            ],
        },
        Section {
            title: "Miscellaneous",
            note: None,
            items: vec![
                action_item("Get help", &[A::ToggleHelp]),
                action_item("Search", &[A::FocusSearch]),
                action_item("Open context menu", &[A::OpenContextMenu]),
                item("Raging Demon", vec!["h+h+right+n+k"]),
                action_item("Listen to Lofi Beats with Wumpus", &[A::VibeWithWumpus]),
            ],
        },
    ]
}

// ----- the grid: `repeat(auto-fill, minmax(180px, max-content))` with a 24px gap -----

const MIN_TRACK: f32 = 180.;
const GAP: f32 = 24.;

/// How many columns fit: the count `auto-fill` makes from the 180px minimum.
pub(in crate::ui) fn columns(available: f32) -> usize {
    (((available + GAP) / (MIN_TRACK + GAP)).floor() as usize).max(1)
}

/// The track widths of `minmax(180px, max-content)`: every track starts at 180, and the free space is shared out
/// equally among the tracks that have not reached the widest content in them ("maximize tracks").
pub(in crate::ui) fn tracks(limits: &[f32], available: f32) -> Vec<f32> {
    let mut widths = vec![MIN_TRACK; limits.len()];
    let mut free =
        available - GAP * limits.len().saturating_sub(1) as f32 - MIN_TRACK * limits.len() as f32;
    while free > 0.001 {
        let growing: Vec<usize> = (0..widths.len())
            .filter(|at| widths[*at] + 0.001 < limits[*at])
            .collect();
        if growing.is_empty() {
            break;
        }
        let share = free / growing.len() as f32;
        for at in growing {
            let add = share.min(limits[at] - widths[at]);
            widths[at] += add;
            free -= add;
        }
    }
    widths
}

fn measure(window: &mut Window, text: &str, size: f32, weight: f32) -> f32 {
    if text.is_empty() {
        return 0.;
    }
    let mut run_font: Font = font(SANS);
    run_font.weight = FontWeight(weight);
    run_font.features = kerning();
    let run = TextRun {
        len: text.len(),
        font: run_font,
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

/// A cap's width: 6px of padding and a 1px border each side around the text (`key`); an arrow is 10px.
fn cap_width(window: &mut Window, cap: &Cap) -> f32 {
    14. + match cap {
        Cap::Text(text) => measure(window, &text.to_uppercase(), 12., 600.),
        Cap::Arrow(_) => 10.,
    }
    .max(0.)
}

/// A row of caps: the widths and the 3px between them.
fn row_width(window: &mut Window, row: &[Cap]) -> f32 {
    row.iter().map(|cap| cap_width(window, cap)).sum::<f32>()
        + 3. * row.len().saturating_sub(1) as f32
}

/// The widest line of an item, plus its 12px padding each side.
fn item_extent(window: &mut Window, item: &Item) -> f32 {
    let mut widest = measure(window, item.description, 14., 400.);
    for bind in &item.binds {
        widest = widest.max(row_width(window, &caps(bind)));
    }
    widest + 24.
}

fn key_cap(cap: &Cap) -> AnyElement {
    let base = div()
        .flex_none()
        .h(px(23.))
        .min_w(px(14.))
        .px(px(6.))
        .pt(px(3.))
        .pb(px(4.))
        .border_1()
        .border_color(color("--border-subtle"))
        .rounded(px(4.))
        .bg(color("--background-mod-muted"))
        .text_color(color("--interactive-text-active"))
        .text_size(px(12.))
        .line_height(px(12.))
        .font_weight(FontWeight(600.))
        .flex()
        .justify_center()
        .shadow(vec![gpui::BoxShadow {
            color: color("--background-mod-muted").into(),
            offset: gpui::point(px(0.), px(-4.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        }]);
    match cap {
        Cap::Text(text) => base.child(text.to_uppercase()).into_any_element(),
        Cap::Arrow(arrow) => {
            let turn: f32 = match arrow {
                Arrow::Down => 0.,
                Arrow::Up => 180.,
                Arrow::Left => 90.,
                Arrow::Right => -90.,
            };
            base.child(
                gpui::svg()
                    .path("icons/key_arrow.svg")
                    .size(px(10.))
                    .flex_none()
                    .text_color(color("--interactive-text-active"))
                    .with_transformation(gpui::Transformation::rotate(gpui::radians(
                        turn.to_radians(),
                    ))),
            )
            .into_any_element()
        }
    }
}

/// `combo`: the caps of one bind, 3px apart.
pub(in crate::ui) fn combo(bind: &str) -> gpui::Div {
    div()
        .flex()
        .h(px(23.))
        .gap(px(3.))
        .children(caps(bind).iter().map(key_cap))
}

impl Root {
    /// Opens the help, or closes it when it is open (`mod+/`, 552992).
    pub(in crate::ui) fn toggle_keyboard_help(&mut self, cx: &mut Context<Self>) {
        self.keys.help = if self.keys.help.is_some() {
            None
        } else {
            Some(Help::new())
        };
        cx.notify();
    }

    /// Keys while the help is open: arrows light the pad, Page Up and Page Down scroll, Escape closes. Returns
    /// whether the key was used.
    pub(in crate::ui) fn keyboard_help_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(help) = self.keys.help.as_mut() else {
            return false;
        };
        if key == "escape" {
            self.keys.help = None;
            cx.notify();
            return true;
        }
        if let Some(arrow) = Arrow::from_key(key) {
            help.press(arrow);
            self.schedule_help_refresh(cx);
            cx.notify();
            return true;
        }
        if matches!(key, "pageup" | "pagedown") {
            let page = f32::from(help.scroll.bounds().size.height) * 0.9;
            let at = help.scroll.offset();
            help.scroll.set_offset(gpui::point(
                at.x,
                at.y + px(if key == "pageup" { page } else { -page }),
            ));
            cx.notify();
            return true;
        }
        false
    }

    /// The lit arrows go dark again after their half second.
    fn schedule_help_refresh(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(LIT + Duration::from_millis(30))
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    pub(in crate::ui) fn keyboard_help_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let help = self.keys.help.as_ref()?;
        let scroll = help.scroll.clone();
        let lit: Vec<bool> = ARROWS.iter().map(|arrow| help.is_lit(*arrow)).collect();
        let view = window.viewport_size();
        // `width: 80vw; max-width: 950px; min-width: 800px; max-height: 80vh`.
        let width = (f32::from(view.width) * 0.8).clamp(MIN_WIDTH, 950.);
        let height = f32::from(view.height) * 0.8;
        // The scroller loses 16px to its gutter, the section 16px to its margin and 8px each side to its padding.
        let list_width = width - 16. - 16. - 16.;
        let count = columns(list_width);
        let sections = sections();
        let body: Vec<AnyElement> = sections
            .iter()
            .enumerate()
            .map(|(at, section)| {
                self.help_section(
                    window,
                    section,
                    count,
                    list_width,
                    at,
                    at == sections.len() - 1,
                )
            })
            .collect();

        let arrows = div()
            .parity("keyboard-help-arrows")
            .absolute()
            .top(px(22.))
            .right(px(40.))
            .flex()
            .overflow_hidden()
            .children(ARROWS.iter().enumerate().map(|(at, arrow)| {
                let arrow = *arrow;
                div()
                    .id(SharedString::from(format!(
                        "keyboard-help-{}",
                        arrow.name()
                    )))
                    .m(px(4.))
                    .size(px(50.))
                    .flex_none()
                    .overflow_hidden()
                    .cursor_pointer()
                    // The sprite has the resting arrow at the top and the lit one 100px down.
                    .child(img(arrow.asset()).w(px(50.)).h(px(150.)).mt(px(if lit[at] {
                        -100.
                    } else {
                        0.
                    })))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(help) = this.keys.help.as_mut() {
                            help.press(arrow);
                        }
                        this.schedule_help_refresh(cx);
                        cx.stop_propagation();
                        cx.notify();
                    }))
            }));

        let modal = div()
            .parity("keyboard-help-modal")
            .relative()
            .w(px(width))
            .h(px(height))
            .bg(color("--background-base-lower"))
            .rounded(px(4.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .parity("keyboard-help-title")
                    .flex_none()
                    .h(px(54.))
                    .mb(px(5.))
                    .px(px(40.))
                    .pt(px(20.))
                    .flex()
                    .items_center()
                    .text_size(px(26.))
                    .line_height(px(34.))
                    .text_color(color("--text-strong"))
                    .child(div().mr(px(20.)).child("Keyboard Combos"))
                    .child(combo("mod+/").parity("keyboard-help-combo")),
            )
            .child(
                div()
                    .parity_below("keyboard-help-subtitle", 1.)
                    .flex_none()
                    .px(px(40.))
                    .pb(px(20.))
                    .border_b_1()
                    .border_color(color("--opacity-black-8"))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child("Master these to be the greatest Discord Discord Revolution player"),
            )
            .child(arrows)
            .child(
                div()
                    .parity("keyboard-help-scroller")
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("keyboard-help-scroller")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            // `scrollbar-gutter: stable`.
                            .pr(px(16.))
                            .child(
                                div()
                                    .parity("keyboard-help-list")
                                    .flex()
                                    .flex_col()
                                    .children(body),
                            ),
                    ),
            );
        Some(
            deferred(
                div()
                    .id("keyboard-help-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.keys.help = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .parity_with_border("keyboard-help-root", 1.)
                            .id("keyboard-help-root")
                            .occlude()
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.16),
                                offset: gpui::point(px(0.), px(4.)),
                                blur_radius: px(8.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .child(modal),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `keyboardShortcutSection`: 24px above and below, a 16px margin at the start, a hairline under it except the last.
    fn help_section(
        &self,
        window: &mut Window,
        section: &Section,
        count: usize,
        list_width: f32,
        at: usize,
        last: bool,
    ) -> AnyElement {
        let first = at == 0;
        let limits: Vec<f32> = (0..count)
            .map(|column| {
                section
                    .items
                    .iter()
                    .skip(column)
                    .step_by(count)
                    .map(|item| item_extent(window, item))
                    .fold(MIN_TRACK, f32::max)
            })
            .collect();
        let widths = tracks(&limits, list_width);
        let rows: Vec<AnyElement> =
            section
                .items
                .chunks(count)
                .enumerate()
                .map(|(at, row)| {
                    div()
                        .flex()
                        .gap(px(GAP))
                        .children(row.iter().enumerate().map(|(column, entry)| {
                            div()
                                .when(first && at == 0 && column == 0, |this| {
                                    this.parity("keyboard-help-item")
                                })
                                .flex_none()
                                .w(px(widths[column]))
                                .p(px(12.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-muted"))
                                .child(
                                    div()
                                        // A hair of slack: the track is the text's own width, and a rounding difference must not wrap it.
                                        .w(px(widths[column] - 24. + 1.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(entry.description),
                                )
                                .child(div().children(
                                    entry.binds.iter().map(|bind| combo(bind).mb(px(8.))),
                                ))
                        }))
                        .into_any_element()
                })
                .collect();
        div()
            .when(first, |this| this.parity("keyboard-help-section"))
            .ml(px(16.))
            .py(px(24.))
            .px(px(8.))
            .when(!last, |this| {
                this.border_b_1().border_color(color("--border-subtle"))
            })
            .child(
                div()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(section.title),
                    )
                    .children(section.note.map(|note| {
                        div()
                            .my(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(note)
                    })),
            )
            .child(
                div()
                    .when(first, |this| this.parity("keyboard-help-group"))
                    .mt(px(8.))
                    .py(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .children(rows),
            )
            .into_any_element()
    }
}

const MIN_WIDTH: f32 = 800.;

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(section: &Section) -> Vec<(String, Vec<String>)> {
        section
            .items
            .iter()
            .map(|entry| {
                let mut shown = Vec::new();
                for bind in &entry.binds {
                    for cap in caps(bind) {
                        shown.push(match cap {
                            Cap::Text(text) => text,
                            Cap::Arrow(arrow) => format!("{arrow:?}"),
                        });
                    }
                }
                (entry.description.to_owned(), shown)
            })
            .collect()
    }

    fn expect(rows: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        rows.iter()
            .map(|(description, shown)| {
                (
                    (*description).to_owned(),
                    shown.iter().map(|cap| (*cap).to_owned()).collect(),
                )
            })
            .collect()
    }

    /// The text of each card of the golden DOM dump, in order (`keyboard-shortcuts-darker-cozy.json`).
    #[test]
    fn the_sections_match_the_official_dump() {
        let all = sections();
        assert_eq!(all.len(), 5);
        assert_eq!(
            rendered(&all[0]),
            expect(&[
                ("Edit Message", &["e"]),
                ("Delete Message", &["Backspace"]),
                ("Pin Message", &["p"]),
                ("Add Reaction", &["+"]),
                ("Reply", &["r"]),
                ("Forward Message", &["f"]),
                ("Speak Message", &["s"]),
                ("Copy Text", &["ctrl", "c"]),
                ("Mark Unread", &["alt", "Enter"]),
                ("Focus text area", &["Esc"]),
            ])
        );
        assert_eq!(
            rendered(&all[1]),
            expect(&[
                (
                    "Navigate between servers",
                    &["ctrl", "alt", "Up", "ctrl", "alt", "Down"]
                ),
                ("Navigate between channels", &["alt", "Up", "alt", "Down"]),
                (
                    "Navigate forward and backward in page history",
                    &["alt", "Left", "alt", "Right"]
                ),
                (
                    "Navigate between unread channels",
                    &["alt", "Shift", "Up", "alt", "Shift", "Down"]
                ),
                (
                    "Navigate between unread channels with mentions",
                    &["ctrl", "Shift", "alt", "Up", "ctrl", "Shift", "alt", "Down"]
                ),
                ("Navigate to current call", &["ctrl", "Shift", "alt", "v"]),
                (
                    "Toggle between last server and DMs",
                    &["ctrl", "alt", "Right"]
                ),
                ("Toggle QuickSwitcher", &["ctrl", "k"]),
                ("Create or join a server", &["ctrl", "Shift", "n"]),
            ])
        );
        assert_eq!(
            rendered(&all[2]),
            expect(&[
                ("Toggle mute", &["ctrl", "Shift", "m"]),
                ("Toggle deafen", &["ctrl", "Shift", "d"]),
                ("Answer incoming call", &["ctrl", "Enter"]),
                ("Decline incoming call", &["Esc"]),
                ("Start call in private message or group", &["ctrl", "'"]),
                ("Toggle soundboard", &["ctrl", "Shift", "b"]),
            ])
        );
        assert_eq!(
            rendered(&all[3]),
            expect(&[
                ("Mark server read", &["Shift", "Esc"]),
                ("Mark channel as read", &["Esc"]),
                ("Create a private group", &["ctrl", "Shift", "t"]),
                ("Create a thread", &["ctrl", "Shift", "alt", "n"]),
                ("Toggle pins popout", &["ctrl", "p"]),
                ("Toggle inbox popout", &["ctrl", "i"]),
                ("Mark top inbox channel read", &["ctrl", "Shift", "e"]),
                (
                    "Toggle channel member list or voice text chat",
                    &["ctrl", "u"]
                ),
                ("Toggle emoji picker", &["ctrl", "e"]),
                ("Toggle GIF picker", &["ctrl", "g"]),
                ("Toggle sticker picker", &["ctrl", "s"]),
                ("Scroll chat up or down", &["Page Up", "Page Down"]),
                ("Jump to oldest unread message", &["Shift", "Page Up"]),
                ("Focus text area", &["Any Key"]),
                ("Record a voice message", &["alt", "r"]),
                ("Upload a file", &["ctrl", "Shift", "u"]),
                ("Copy channel link", &["ctrl", "Shift", "l"]),
            ])
        );
        assert_eq!(
            rendered(&all[4]),
            expect(&[
                ("Get help", &["ctrl", "Shift", "h"]),
                ("Search", &["ctrl", "f"]),
                ("Open context menu", &["Shift", "f10"]),
                ("Raging Demon", &["h", "h", "Right", "n", "k"]),
                (
                    "Listen to Lofi Beats with Wumpus",
                    &["ctrl", "alt", "Shift", "w"]
                ),
            ])
        );
    }

    #[test]
    fn four_columns_fit_the_widest_dialog() {
        assert_eq!(columns(902.), 4);
        // At the narrowest dialog (800 wide) the list is 752 wide: three columns.
        assert_eq!(columns(752.), 3);
    }

    #[test]
    fn free_space_is_shared_out_until_each_track_reaches_its_content() {
        // The Navigation group of the dump: 214.234, 187.312, 214.234, 214.219 (sum 830.0 plus 72 of gaps).
        let widths = tracks(&[400., 187.312, 400., 400.], 902.);
        assert!((widths[1] - 187.312).abs() < 0.01);
        let expected = (902. - 72. - 187.312) / 3.;
        for at in [0, 2, 3] {
            assert!((widths[at] - expected).abs() < 0.01, "{widths:?}");
        }
        // Messages: every card is as narrow as its content, so every track stays at the 180px minimum.
        assert_eq!(tracks(&[180., 180., 180., 180.], 902.), vec![180.; 4]);
        // A track whose content is wider than the room it is offered takes what is left, never more.
        let widths = tracks(&[245.812, 180., 180., 180.], 902.);
        assert!((widths[0] - 245.812).abs() < 0.01);
        assert_eq!(widths[1], 180.);
    }

    #[test]
    fn arrows_are_read_from_the_key_names() {
        assert_eq!(Arrow::from_key("left"), Some(Arrow::Left));
        assert_eq!(Arrow::from_key("enter"), None);
    }
}
