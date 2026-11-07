//! Draws generated settings pages. Components and their measurements:
//! switch row `baseSettingWrapper_4Z2WJy` + `container_VhFsFg` (grid of label and control), switch `container_9AraPt`,
//! radio group `group_oJuRbG`, select `selectField_vWC4bk`, slider `slider_OnqacX`, row `baseControlItem_FiiQ6I`.

use super::model::{Choice, Node, RadioOption, Runs};
use super::{Root, SANS, button, category_header, color, divider, info_notice, kerning, mask_icon};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, Font, FontWeight, HighlightStyle, IntoElement, Pixels, SharedString,
    StyledText, TextRun, Window, div, font, prelude::*, px, relative,
};

/// The panel's inner width: `panel_nNq2Vb` max-width 696.
const PANEL: f32 = 696.;
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

fn runs_text(runs: Runs, base: &str) -> impl IntoElement {
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
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.setting_choices.insert(key, Choice::On(!on));
            cx.notify();
        }))
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
fn radio_dot(selected: bool) -> impl IntoElement {
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
fn select_field(value: &'static str, width: Option<f32>) -> impl IntoElement {
    div()
        .flex_none()
        .when_some(width, |this, width| this.w(px(width)))
        .when(width.is_none(), |this| this.w_full())
        .h(px(40.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--input-border-default"))
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
                .child(mask_icon("settings_select_chevron", 20., "--text-strong")),
        )
}

/// `slider_OnqacX hasMarks_OnqacX`: marks above a 4px bar at 34px, the 16px grabber centred on the bar.
fn slider(
    marks: &'static [(&'static str, f32)],
    default_mark: Option<usize>,
    value: f32,
) -> impl IntoElement {
    let track = PANEL - 10.;
    let last = marks.len().saturating_sub(1);
    div()
        .relative()
        .h(px(48.))
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
                .top(px(34.))
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
                .top(px(28.))
                .left(px(5. + track * value - 8.))
                .size(px(16.))
                .rounded_full()
                .border_1()
                .border_color(color("--border-normal"))
                .bg(gpui::white()),
        )
}

impl Root {
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::Category {
                title,
                group,
                legend,
                description,
                children,
                ..
            } => {
                // Each row keeps its official key as its id, the anchor the subnav and search scroll to.
                let rows: Vec<AnyElement> = children
                    .iter()
                    .map(|child| {
                        div()
                            .parity(child.key())
                            .id(SharedString::from(child.key()))
                            .flex()
                            .flex_col()
                            .child(self.render_node(child, window, cx))
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
                            .flex_col()
                            .gap(px(8.))
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
            Node::Switch {
                key,
                label,
                badge,
                description,
                on,
            } => {
                let extent = label_extent(window, label, *badge, description);
                let width = label_track(PANEL - COLUMN_GAP, extent, SWITCH_WIDTH);
                let on = self.choice_on(key, *on);
                div()
                    .min_h(px(48.))
                    .py(px(8.))
                    .flex()
                    .when(description.is_empty(), |this| this.items_center())
                    .justify_between()
                    .gap(px(COLUMN_GAP))
                    .child(
                        label_block_capped(
                            label,
                            *badge,
                            description,
                            Some(description_max_width(window)),
                        )
                        .w(px(width)),
                    )
                    .child(switch(
                        SharedString::from(format!("switch-{key}")),
                        on,
                        cx,
                        key,
                    ))
                    .into_any_element()
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
                .child(slider(marks, *default_mark, *value))
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
                label,
                badge,
                description,
                value,
                vertical,
                width,
                ..
            } => {
                if *vertical {
                    div()
                        .min_h(px(48.))
                        .py(px(8.))
                        .flex()
                        .flex_col()
                        .child(label_block(label, *badge, description).mb(px(8.)))
                        .child(select_field(value, (*width < PANEL).then_some(*width)))
                        .into_any_element()
                } else {
                    div()
                        .min_h(px(48.))
                        .py(px(8.))
                        .flex()
                        .gap(px(COLUMN_GAP))
                        .child(
                            label_block(label, *badge, description)
                                .w(px(PANEL - COLUMN_GAP - width)),
                        )
                        .child(select_field(value, Some(*width)))
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
                                button(
                                    SharedString::from(format!("button-{key}-{index}")),
                                    text,
                                    *kind,
                                )
                            })),
                    );
                }
                row.render()
            }
            Node::Notice { text, .. } => info_notice(text).into_any_element(),
            Node::Custom { height, .. } => div().h(px(*height)).into_any_element(),
        }
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
