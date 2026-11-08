//! Draws generated settings pages. Components and their measurements:
//! switch row `baseSettingWrapper_4Z2WJy` + `container_VhFsFg` (grid of label and control), switch `container_9AraPt`,
//! radio group `group_oJuRbG`, select `selectField_vWC4bk`, slider `slider_OnqacX`, row `baseControlItem_FiiQ6I`.

use super::model::{ButtonKind, Choice, Node, RadioOption, Runs};
use super::voice;
use super::{Root, SANS, button, category_header, color, divider, info_notice, kerning, mask_icon};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, Font, FontWeight, HighlightStyle, IntoElement, Pixels, SharedString,
    StyledText, TextRun, Window, div, font, prelude::*, px, relative,
};

/// The panel's inner width: `panel_nNq2Vb` max-width 696.
pub(super) const PANEL: f32 = 696.;
/// `column-gap` of a horizontal field.
const COLUMN_GAP: f32 = 24.;
const SWITCH_WIDTH: f32 = 48.;

/// `--custom-description-max-width: 70ch` of a horizontal field, in the description's 14px text.
fn description_max_width(window: &mut Window) -> Pixels {
    measure(window, "0", 14., 400.) * 70.
}

pub(super) fn measure_text(window: &mut Window, text: &str, size: f32, weight: f32) -> Pixels {
    measure(window, text, size, weight)
}

/// The widest line of `text`; a line break ends a line as it does in the page.
fn measure(window: &mut Window, text: &str, size: f32, weight: f32) -> Pixels {
    if text.contains('\n') {
        return text
            .lines()
            .map(|line| measure(window, line, size, weight))
            .fold(px(0.), Pixels::max);
    }
    if text.is_empty() {
        return px(0.);
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
    window
        .text_system()
        .shape_line(SharedString::from(text.to_owned()), px(size), &[run], None)
        .width
}

/// Max-content and min-content widths of a label column: the title line and the description as one line,
/// and the widest single word of either.
fn label_extent(
    window: &mut Window,
    label: &str,
    badge: bool,
    description: Runs,
) -> (Pixels, Pixels) {
    let text: String = description.iter().map(|(part, _)| *part).collect();
    let description_cap = description_max_width(window);
    // The NEW badge: 8px gap, 8px padding each side around 12px semibold text.
    let badge_width = if badge {
        px(8. + 16.) + measure(window, "NEW", 12., 600.)
    } else {
        px(0.)
    };
    let widest = measure(window, label, 16., 500.)
        .max(measure(window, &text, 14., 400.).min(description_cap));
    let max_content = (measure(window, label, 16., 500.) + badge_width).max(widest);
    let mut min_content = badge_width;
    for word in label.split_whitespace() {
        min_content = min_content.max(measure(window, word, 16., 500.));
    }
    for word in text.split_whitespace() {
        min_content = min_content.max(measure(window, word, 14., 400.));
    }
    (max_content, min_content)
}

/// Chromium's sizing of two `auto` grid tracks: each starts at min-content, free space first grows them to
/// max-content, then whatever remains is split evenly. Returns the label track's width.
pub(super) fn label_track(available: f32, label: (Pixels, Pixels), control: f32) -> f32 {
    let (label_max, label_min) = (f32::from(label.0), f32::from(label.1));
    let mut free = available - label_min - control;
    if free <= 0. {
        return label_min;
    }
    let grow = free.min(label_max - label_min).max(0.);
    free -= grow;
    label_min + grow + free / 2.
}

/// The Voice & Video sounds list and how many of its nodes (four switches, three rules) show before its accordion.
const VOICE_SOUNDS_LIST: &str = "voice_and_video_sounds_list";
const SOUNDS_SHOWN_NODES: usize = 7;

pub(super) fn runs_text(runs: Runs, base: &str) -> impl IntoElement {
    let mut text = String::new();
    let mut links = Vec::new();
    for (part, link) in runs {
        let start = text.len();
        text.push_str(part);
        if *link {
            links.push((
                start..text.len(),
                HighlightStyle {
                    color: Some(color("--text-link").into()),
                    ..HighlightStyle::default()
                },
            ));
        }
    }
    div()
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(color(base))
        .child(StyledText::new(text).with_highlights(links))
}

fn label_block(label: &'static str, badge: bool, description: Runs) -> gpui::Div {
    label_block_capped(label, badge, description, None)
}

fn label_block_capped(
    label: &'static str,
    badge: bool,
    description: Runs,
    cap: Option<Pixels>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .min_w(px(0.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .when(badge, |this| this.child(super::new_badge())),
        )
        .when(!description.is_empty(), |this| {
            this.child(
                div()
                    .when_some(cap, |this, cap| this.max_w(cap))
                    .child(runs_text(description, "--text-subtle")),
            )
        })
}

/// `switchIndicator_9AraPt`: 48 by 24, the 16px thumb inside a 24px box that sits at 1px or 24px.
fn switch(
    id: SharedString,
    on: bool,
    disabled: bool,
    cx: &mut Context<Root>,
    key: &'static str,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .relative()
        .w(px(SWITCH_WIDTH))
        .h(px(24.))
        .rounded(px(16.))
        .border_1()
        .border_color(color("--input-border-default"))
        .bg(if on {
            color("--background-brand")
        } else {
            color("--input-background-default")
        })
        // `disabled_9AraPt`: half opacity, no clicks.
        .when(disabled, |this| this.opacity(0.5))
        .when(!disabled, |this| {
            this.cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.setting_choices.insert(key, Choice::On(!on));
                    this.voice_setting_changed(key);
                    cx.notify();
                }))
        })
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(if on { 27. } else { 3. }))
                .size(px(16.))
                .rounded_full()
                .bg(gpui::white()),
        )
}

/// `radioIndicator_oJuRbG`: a 20px ring, 2px stroke, filled with an 8px dot when chosen.
pub(crate) fn radio_dot(selected: bool) -> impl IntoElement {
    div()
        .flex_none()
        .size(px(20.))
        .rounded_full()
        .border_2()
        .border_color(color(if selected {
            "--radio-border-selected-default"
        } else {
            "--radio-border-default"
        }))
        .bg(color(if selected {
            "--radio-background-selected-default"
        } else {
            "--radio-background-default"
        }))
        .flex()
        .items_center()
        .justify_center()
        .when(selected, |this| {
            this.child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(color("--radio-thumb-background-active")),
            )
        })
}

/// `selectField_vWC4bk`: 40 tall with a 1px border, the value 13px in, a 20px chevron 8px from the right.
fn select_field(value: &'static str, width: Option<f32>) -> gpui::Div {
    select_field_open(value, width, false)
}

/// The field, its border brand and its chevron turned up while its list is open (`isOpen_vWC4bk`).
fn select_field_open(value: &'static str, width: Option<f32>, open: bool) -> gpui::Div {
    div()
        .flex_none()
        .when_some(width, |this, width| this.w(px(width)))
        .when(width.is_none(), |this| this.w_full())
        .h(px(40.))
        .rounded(px(8.))
        .border_1()
        .border_color(if open {
            color("--text-brand")
        } else {
            color("--input-border-default")
        })
        .bg(color("--input-background-default"))
        .flex()
        .items_center()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .pl(px(12.))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-strong"))
                .child(value),
        )
        .child(
            div()
                .flex_none()
                .w(px(36.))
                .pr(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    mask_icon("settings_select_chevron", 20., "--text-strong").when(open, |this| {
                        this.with_transformation(gpui::Transformation::rotate(gpui::percentage(
                            0.5,
                        )))
                    }),
                ),
        )
}

/// Selects that open, as module 158935's `r2` lists its options: the key, the key its open state is kept under in
/// `setting_choices`, and the labels in order. The rest still show the captured value only.
const SELECT_OPTIONS: &[(&str, &str, &[&str])] = &[(
    "voice_noise_suppression_setting",
    "voice_noise_suppression_setting/open",
    &["DeepFilterNet", "Standard", "None"],
)];

/// Noise Suppression's description names what runs. The official one says "Powered by Krisp", which cannot run here
/// (BLOCKED.md); the owner asked for DeepFilterNet to be shown as itself in the same design (11-08).
const NOISE_DESCRIPTION: Runs = &[
    ("Reduces background noise from your mic. Powered by ", false),
    ("DeepFilterNet", true),
    (".", false),
];

/// The select's option index as the processors take it: Krisp, Standard, None.
pub(super) fn suppression(index: usize) -> dc_voice::Suppression {
    match index {
        0 => dc_voice::Suppression::DeepFilterNet,
        1 => dc_voice::Suppression::Standard,
        _ => dc_voice::Suppression::None,
    }
}

/// gravecord's own switch under Echo Cancellation, not in the official client (`dc_voice`'s `dereverb.rs`,
/// DECISIONS.md). The generated pages stay as captured; it is drawn after the row it follows.
pub(in crate::ui) const ROOM_ECHO_KEY: &str = "gravecord_reduce_room_echo";
#[cfg(test)]
const ROOM_ECHO_AFTER: &str = "voice_echo_cancellation_setting";
pub(super) static ROOM_ECHO: Node = Node::Switch {
    key: ROOM_ECHO_KEY,
    label: "Reduce Room Echo",
    badge: false,
    description: &[(
        "Takes the echo of your room off your voice, so you don't sound far away in a bare room.",
        false,
    )],
    on: false,
};

/// `slider_OnqacX hasMarks_OnqacX`: marks above a 4px bar at 34px, the 16px grabber centred on the bar.
fn slider(
    marks: &'static [(&'static str, f32)],
    default_mark: Option<usize>,
    value: f32,
    width: f32,
) -> impl IntoElement {
    let track = width - 10.;
    let last = marks.len().saturating_sub(1);
    // Without marks the slider is 24 tall and the bar sits at its middle.
    let offset = if marks.is_empty() { 0. } else { 24. };
    div()
        .relative()
        .h(px(24. + offset))
        .w_full()
        .children(marks.iter().enumerate().map(|(index, (text, at))| {
            let x = 5. + track * at;
            let label_color = if Some(index) == default_mark {
                "--text-feedback-positive"
            } else {
                "--text-subtle"
            };
            // The first and last labels are pulled in so they end at the track's edges.
            let shift = if index == 0 {
                0.5
            } else if index == last {
                -0.5
            } else {
                0.
            };
            div()
                .absolute()
                .top_0()
                .left(px(x - 12.))
                .w(px(24.))
                .h(px(48.))
                .flex()
                .flex_col()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .relative()
                        .left(relative(shift))
                        .pl(px(1.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color(label_color))
                        .whitespace_nowrap()
                        .child(*text),
                )
                .child(
                    div()
                        .w(px(2.))
                        .h(px(24.))
                        .rounded(px(12.))
                        .bg(color("--slider-track-background")),
                )
        }))
        .child(
            div()
                .absolute()
                .top(px(10. + offset))
                .left_0()
                .w_full()
                .h(px(4.))
                .rounded(px(4.))
                .overflow_hidden()
                .bg(color("--slider-track-background"))
                .child(
                    div()
                        .h_full()
                        .w(px(5. + track * value))
                        .border_1()
                        .border_color(gpui::hsla(0., 0., 1., 0.1))
                        .bg(color("--background-brand")),
                ),
        )
        .child(
            div()
                .absolute()
                .top(px(4. + offset))
                .left(px(5. + track * value - 8.))
                .size(px(16.))
                .rounded_full()
                .border_1()
                .border_color(color("--border-normal"))
                .bg(gpui::white()),
        )
}

impl Root {
    /// A select that opens: the field, and under it while open, `selectDropdown_vWC4bk`'s list (rows 40 tall in 4px
    /// of padding, the chosen one ticked), measured in `design/dom/settings/voice-video-noise-open-darker-cozy.json`.
    fn settings_select(
        &self,
        key: &'static str,
        captured: &'static str,
        width: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((_, open_key, options)) =
            SELECT_OPTIONS.iter().find(|(select, ..)| *select == key)
        else {
            return select_field(captured, width).into_any_element();
        };
        let open_key: &'static str = open_key;
        let options: &'static [&'static str] = options;
        let current = options
            .iter()
            .position(|option| *option == captured)
            .unwrap_or(0);
        let chosen = self.choice_index(key, current);
        let label = options.get(chosen).copied().unwrap_or(captured);
        let open = self.choice_on(open_key, false);
        let rows: Vec<AnyElement> = options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                div()
                    .id(SharedString::from(format!("{key}-option-{index}")))
                    .flex_none()
                    .h(px(40.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(*option),
                    )
                    .when(index == chosen, |this| {
                        this.child(div().p(px(4.)).child(super::super::icon(
                            "select_check",
                            20.,
                            "--text-strong",
                        )))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.setting_choices.insert(key, Choice::Index(index));
                        this.setting_choices.insert(open_key, Choice::On(false));
                        this.voice_setting_changed(key);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        div()
            .relative()
            .flex_none()
            .child(
                select_field_open(label, width, open)
                    .id(SharedString::from(format!("{key}-field")))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.setting_choices.insert(open_key, Choice::On(!open));
                        cx.notify();
                    })),
            )
            .when(open, |this| {
                this.child(
                    gpui::deferred(
                        div()
                            .id(SharedString::from(format!("{key}-list")))
                            .occlude()
                            .absolute()
                            .top(px(44.))
                            .left_0()
                            .right_0()
                            .p(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.16),
                                offset: gpui::point(px(0.), px(2.)),
                                blur_radius: px(8.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                                this.setting_choices.insert(open_key, Choice::On(false));
                                cx.notify();
                            }))
                            .child(div().flex().flex_col().children(rows)),
                    )
                    .with_priority(2),
                )
            })
            .into_any_element()
    }

    fn choice_on(&self, key: &'static str, captured: bool) -> bool {
        match self.setting_choices.get(key) {
            Some(Choice::On(on)) => *on,
            _ => captured,
        }
    }

    fn choice_index(&self, key: &'static str, captured: usize) -> usize {
        match self.setting_choices.get(key) {
            Some(Choice::Index(index)) => *index,
            _ => captured,
        }
    }

    /// One node, with the 8px vertical padding every `baseSettingWrapper` keeps.
    pub(super) fn render_node(
        &self,
        node: &'static Node,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::Category {
                title,
                group,
                legend,
                split,
                gap,
                description,
                children,
                ..
            } => {
                let column = if *split { (width - 16.) / 2. } else { width };
                // Each row keeps its official key as its id, the anchor the subnav and search scroll to.
                // Rows the generated page lacks go after the row they follow; rows whose predicate fails are left out.
                let mut shown: Vec<&'static Node> = Vec::new();
                for child in children.iter() {
                    shown.push(child);
                    shown.extend(self.voice_rows_after(child.key()));
                }
                shown.retain(|child| self.voice_row_shown(child.key()));
                let rows: Vec<AnyElement> = shown
                    .into_iter()
                    .map(|child| {
                        // The sensitivity row is drawn whole now; its captured height no longer holds its place.
                        let padded = (child.key() != voice::SENSITIVITY)
                            .then(|| super::pages::padded(child.key()))
                            .flatten();
                        div()
                            .parity(child.key())
                            .id(SharedString::from(child.key()))
                            .when(*split, |this| this.flex_1().min_w(px(0.)))
                            .when_some(padded, |this, height| this.min_h(px(height)))
                            .flex()
                            .flex_col()
                            .child(self.render_node(child, column, window, cx))
                            .into_any_element()
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .children(title.map(|title| category_header(title, description)))
                    .children(legend.map(|legend| {
                        div()
                            .pt(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(legend)
                    }))
                    .child(
                        div()
                            .flex()
                            .when(*split, |this| this.flex_row().gap(px(16.)))
                            .when(!*split, |this| this.flex_col().gap(px(*gap)))
                            .when(legend.is_some(), |this| this.pt(px(16.)))
                            .children(group.map(|group| {
                                div()
                                    .mt(px(8.))
                                    .mb(px(4.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-muted"))
                                    .child(group)
                            }))
                            .children(rows),
                    )
                    .into_any_element()
            }
            Node::Divider => divider(),
            Node::Rule => div()
                .h(px(1.))
                .bg(color("--border-subtle"))
                .into_any_element(),
            Node::Switch { key, .. } if *key == voice::SENSITIVITY => {
                self.sensitivity_row(width, window, cx)
            }
            Node::Switch {
                key,
                label,
                badge,
                description,
                on,
            } => {
                // The sound rows show the notification store (`NotificationSettingsStore`), not a local copy.
                let on =
                    crate::sounds::switch_value(key).unwrap_or_else(|| self.choice_on(key, *on));
                let disabled = self.voice_disabled(key)
                    || (*key == crate::sounds::SELECTED_CHANNEL
                        && crate::sounds::switch_value(crate::sounds::DISABLE_ALL) == Some(true));
                let row = self.switch_row(
                    key,
                    label,
                    *badge,
                    description,
                    on,
                    disabled,
                    width,
                    window,
                    cx,
                );
                div()
                    .min_h(px(48.))
                    .py(px(8.))
                    // Debug Logging's fieldset: the switch, then Upload Logs and Show Folder 4px under it.
                    .child(if *key == voice::extra::DEBUG_LOGGING {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(row)
                            .child(self.debug_log_buttons(cx))
                            .into_any_element()
                    } else {
                        row.into_any_element()
                    })
                    // The row keeps its content at the top of its 48px (`baseSettingWrapper`), not centred in it.
                    .into_any_element()
            }
            Node::Slider { key, label, .. }
                if *key == voice::INPUT_VOLUME
                    || *key == voice::OUTPUT_VOLUME
                    || *key == voice::extra::ATTENUATION =>
            {
                self.slider_row(key, label, width, cx)
            }
            Node::Slider {
                label,
                badge,
                description,
                marks,
                default_mark,
                value,
                ..
            } => div()
                .min_h(px(48.))
                .py(px(8.))
                .flex()
                .flex_col()
                .child(label_block(label, *badge, description).mb(px(8.)))
                .child(slider(marks, *default_mark, *value, width))
                .into_any_element(),
            Node::Radio {
                key,
                label,
                badge,
                description,
                options,
                selected,
            } => {
                let selected = self.choice_index(key, *selected);
                div()
                    .min_h(px(48.))
                    .py(px(8.))
                    .flex()
                    .flex_col()
                    .child(label_block(label, *badge, description).mb(px(8.)))
                    .child(
                        div()
                            .pt(px(8.))
                            .pb(px(4.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .children(options.iter().enumerate().map(|(index, option)| {
                                self.radio_option(key, index, option, index == selected, cx)
                            })),
                    )
                    .into_any_element()
            }
            Node::Select {
                key,
                label,
                badge,
                description,
                value,
                vertical,
                width: field,
            } if SELECT_OPTIONS.iter().any(|(select, ..)| select == key) => {
                let field = if *vertical {
                    (*field < width).then_some(*field)
                } else {
                    Some(*field)
                };
                let select = self.settings_select(key, value, field, cx);
                let description = if *key == "voice_noise_suppression_setting" {
                    NOISE_DESCRIPTION
                } else {
                    description
                };
                if *vertical {
                    div()
                        .min_h(px(48.))
                        .py(px(8.))
                        .flex()
                        .flex_col()
                        .child(label_block(label, *badge, description).mb(px(8.)))
                        .child(select)
                        .into_any_element()
                } else {
                    div()
                        .min_h(px(48.))
                        .py(px(8.))
                        .flex()
                        .gap(px(COLUMN_GAP))
                        .child(
                            label_block(label, *badge, description)
                                .w(px(width - COLUMN_GAP - field.unwrap_or(0.))),
                        )
                        .child(select)
                        .into_any_element()
                }
            }
            Node::Select {
                label,
                badge,
                description,
                value,
                vertical,
                width: field,
                ..
            } => {
                if *vertical {
                    div()
                        .min_h(px(48.))
                        .py(px(8.))
                        .flex()
                        .flex_col()
                        .child(label_block(label, *badge, description).mb(px(8.)))
                        .child(select_field(value, (*field < width).then_some(*field)))
                        .into_any_element()
                } else {
                    div()
                        .min_h(px(48.))
                        .py(px(8.))
                        .flex()
                        .gap(px(COLUMN_GAP))
                        .child(
                            label_block(label, *badge, description)
                                .w(px(width - COLUMN_GAP - field)),
                        )
                        .child(select_field(value, Some(*field)))
                        .into_any_element()
                }
            }
            Node::Control {
                key,
                label,
                badge,
                description,
                icon,
                value,
                buttons,
                clickable,
            } => {
                let mut row = super::Row::new(label);
                if *badge {
                    row = row.badge();
                }
                if !description.is_empty() {
                    row = row.description(runs_text(description, "--text-subtle"));
                }
                if let Some(icon) = icon {
                    row = row.leading(super::tile(icon, "--background-mod-muted", "--icon-strong"));
                }
                if *clickable {
                    row = row.link((!value.is_empty()).then(|| (*value).to_owned()));
                } else if !buttons.is_empty() || !value.is_empty() {
                    row = row.trailing(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .when(!value.is_empty(), |this| this.child(super::value(*value)))
                            .children(buttons.iter().enumerate().map(|(index, (text, kind))| {
                                let id = SharedString::from(format!("button-{key}-{index}"));
                                let reset = *key == voice::extra::RESET;
                                // Reset all Voice & Video settings asks first (`rA`).
                                div()
                                    .id(SharedString::from(format!("{id}-click")))
                                    .when(reset, |this| {
                                        this.on_click(cx.listener(|this, _, _, cx| {
                                            this.voice.ask_reset();
                                            cx.notify();
                                        }))
                                    })
                                    .child(button(id, text, *kind))
                            })),
                    );
                }
                row.render()
            }
            Node::Notice { text, .. } => info_notice(text).into_any_element(),
            Node::Accordion {
                key,
                label,
                description,
                children,
            } => {
                let open = self.setting_choices.get(key) == Some(&Choice::On(true));
                // The sounds list shows its first four sounds (Deafen to Unmute, with the rules between) before
                // the "Show 18 more sounds" row; the rest sit behind it.
                let (shown, children): (&'static [Node], &'static [Node]) =
                    if *key == VOICE_SOUNDS_LIST && children.len() > SOUNDS_SHOWN_NODES + 1 {
                        (
                            &children[..SOUNDS_SHOWN_NODES],
                            &children[SOUNDS_SHOWN_NODES + 1..],
                        )
                    } else {
                        (&[], children)
                    };
                // `ji1uNt` / `0JYT98`: "Show 18 more sounds", then "Hide 18 sounds".
                let label: &'static str = if open && *key == VOICE_SOUNDS_LIST {
                    "Hide 18 sounds"
                } else {
                    label
                };
                let header = div()
                    .id(SharedString::from(format!("accordion-{key}")))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .mx(px(-12.))
                    .p(px(12.))
                    .rounded(px(16.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.setting_choices.insert(key, Choice::On(!open));
                        cx.notify();
                    }))
                    .child(if *key == voice::ADVANCED_ACCORDION {
                        // `useCollapsedSubtitle`: the titles of the rows it holds now.
                        label_block(label, false, &[]).flex_1().child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child(self.voice_accordion_subtitle(children)),
                        )
                    } else {
                        label_block(label, false, description).flex_1()
                    })
                    .child(
                        div()
                            .flex_none()
                            .size(px(48.))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            // `iconClosed_MdUQoo` turns the up chevron over (`rotateX(180deg)`): closed points down.
                            // The file named `settings_accordion_open` holds the turned one.
                            .child(mask_icon(
                                if open {
                                    "settings_accordion_closed"
                                } else {
                                    "settings_accordion_open"
                                },
                                24.,
                                "--text-default",
                            )),
                    );
                let rows: Vec<AnyElement> = if open {
                    children
                        .iter()
                        .filter(|child| self.voice_row_shown(child.key()))
                        .map(|child| self.render_node(child, width, window, cx))
                        .collect()
                } else {
                    Vec::new()
                };
                let shown_rows: Vec<AnyElement> = shown
                    .iter()
                    .map(|child| self.render_node(child, width, window, cx))
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .when(!shown_rows.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(16.))
                                .pb(px(16.))
                                .children(shown_rows),
                        )
                    })
                    .child(header)
                    .child(
                        div()
                            .when(!open, |this| this.h(px(0.)))
                            .when(open, |this| this.pt(px(16.)).flex().flex_col().gap(px(16.)))
                            .children(rows),
                    )
                    .into_any_element()
            }
            Node::Custom { key, height } => match *key {
                "appearance_default_themes" => self.default_themes(cx),
                "appearance_custom_themes_upsell" => self.custom_theme_card(),
                "appearance_client_themes" => self.color_themes(cx),
                _ => self
                    .voice_custom(key, width, window, cx)
                    .unwrap_or_else(|| div().h(px(*height)).into_any_element()),
            },
        }
    }

    /// `s5.e`: Upload Logs and Show Folder, secondary buttons 8px apart. Upload Logs is spent after one try (`isDisabled`)
    /// and shows as busy while it runs (`loading`).
    fn debug_log_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let (uploading, spent) = self.upload_state();
        let busy = uploading || spent;
        div()
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .id("voice-upload-logs")
                    .when(busy, |this| this.opacity(0.5))
                    .when(!busy, |this| {
                        this.on_click(cx.listener(|this, _, _, cx| this.voice_upload_logs(cx)))
                    })
                    .child(button(
                        "voice-upload-logs-button",
                        "Upload Logs",
                        ButtonKind::Secondary,
                    )),
            )
            .child(
                div()
                    .id("voice-show-folder")
                    .on_click(cx.listener(|this, _, _, cx| this.voice_show_folder(cx)))
                    .child(button(
                        "voice-show-folder-button",
                        "Show Folder",
                        ButtonKind::Secondary,
                    )),
            )
            .into_any_element()
    }

    /// A switch row (`container_VhFsFg`): the label and description in their grid track, the switch at the end.
    /// Disabled, the label and switch are at half opacity and the switch takes no clicks.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn switch_row(
        &self,
        key: &'static str,
        label: &'static str,
        badge: bool,
        description: Runs,
        on: bool,
        disabled: bool,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let extent = label_extent(window, label, badge, description);
        let label_width = label_track(width - COLUMN_GAP, extent, SWITCH_WIDTH);
        // "Preview Sound" plays the row's sound (`aZ`); the link is the whole description.
        let preview = crate::sounds::row_sound(key)
            .filter(|_| matches!(description, [("Preview Sound", true)]));
        let cap = description_max_width(window);
        let label_side = match preview {
            Some(sound) => label_block_capped(label, badge, &[], Some(cap)).child(
                div()
                    .max_w(cap)
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(
                        self.text_with_link(key, "", "Preview Sound", "", cx, move |_, _| {
                            crate::sounds::preview(sound)
                        }),
                    ),
            ),
            None => label_block_capped(label, badge, description, Some(cap)),
        };
        div()
            .flex()
            .when(description.is_empty(), |this| this.items_center())
            .justify_between()
            .gap(px(COLUMN_GAP))
            .child(
                label_side
                    .w(px(label_width))
                    .when(disabled, |this| this.opacity(0.5)),
            )
            .child(switch(
                SharedString::from(format!("switch-{key}")),
                on,
                disabled,
                cx,
                key,
            ))
    }

    fn radio_option(
        &self,
        key: &'static str,
        index: usize,
        option: &'static RadioOption,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(SharedString::from(format!("radio-{key}-{index}")))
            .flex()
            .gap(px(12.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.setting_choices.insert(key, Choice::Index(index));
                this.voice_setting_changed(key);
                cx.notify();
            }))
            .child(radio_dot(selected))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(option.label),
                    )
                    .when(!option.description.is_empty(), |this| {
                        this.child(
                            div()
                                .mt(px(4.))
                                .child(runs_text(option.description, "--text-subtle")),
                        )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    /// The select's rows in module 158935's order become the processors' choices: `KRISP`, `STANDARD`, `NONE`.
    #[test]
    fn noise_suppression_rows_map_to_the_processors() {
        let (_, _, options) = SELECT_OPTIONS
            .iter()
            .find(|(key, ..)| *key == "voice_noise_suppression_setting")
            .expect("the select opens");
        assert_eq!(*options, ["DeepFilterNet", "Standard", "None"]);
        assert_eq!(suppression(0), dc_voice::Suppression::DeepFilterNet);
        assert_eq!(suppression(1), dc_voice::Suppression::Standard);
        assert_eq!(suppression(2), dc_voice::Suppression::None);
    }

    /// gravecord's switch sits right after Echo Cancellation, off unless turned on.
    #[test]
    fn room_echo_follows_echo_cancellation() {
        assert_eq!(ROOM_ECHO_AFTER, "voice_echo_cancellation_setting");
        assert!(matches!(
            ROOM_ECHO,
            Node::Switch {
                key: ROOM_ECHO_KEY,
                on: false,
                ..
            }
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::label_track;
    use gpui::px;

    #[test]
    fn short_labels_split_the_spare_width_evenly() {
        // "Make links stand out more." on the Accessibility page: label 392, switch track 280.
        let width = label_track(672., (px(160.), px(60.)), 48.);
        assert_eq!(width, 392.);
    }

    #[test]
    fn long_labels_take_what_they_need_and_share_the_rest() {
        // A description 572 wide leaves 52, half of it to each track: 598 and 74, as captured.
        assert_eq!(label_track(672., (px(572.), px(80.)), 48.), 598.);
        // Wider than the room: the label stops where the switch begins.
        assert_eq!(label_track(672., (px(900.), px(80.)), 48.), 624.);
    }
}
