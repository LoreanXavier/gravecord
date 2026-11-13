//! The perk card of the Nitro home (`S` of module 706778, stylesheet 909b47350bd7f623 `*_ualTdV`, inside the shine
//! card of module 872725 and the design system card `cardPrimary_qCN-Ri`).
//!
//! At rest the card shows its art, the title and one line of description. Hovered (`:hover` on the container) the
//! description opens up to five lines (`max-height: 1lh` to `5lh`), the button's 36px slot opens with 16px above it,
//! the button rises from below the card and fades in, and the text area's backdrop takes the card's colour. All of it
//! runs 400ms on `--custom-bezier: cubic-bezier(.36,.35,.1,1.23)` under `.full-motion`; the line clamp comes back
//! only after the 400ms (`-webkit-line-clamp 0s .4s allow-discrete`). The Orbs card's art scales to .9.
//!
//! Not drawn: the 48px backdrop blur behind the text (`backdrop-filter`), the Rive glow of a deep-linked card, the
//! featured layout (no card here is featured) and keyboard focus (`:focus-within`).

use super::super::{Root, SANS, color, icon, kerning, motion};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, SharedString, TextRun, Window, div,
    font, img, linear_color_stop, linear_gradient, prelude::*, px,
};
use motion::Transitions as _;

/// `--custom-bezier` of `cardContainer_ualTdV`.
pub(super) const BEZIER: motion::Ease = motion::Ease::Bezier(0.36, 0.35, 0.1, 1.23);

/// `.4s var(--custom-bezier)` under `.full-motion`.
pub(super) fn transition() -> motion::Css {
    motion::Css::new(400., BEZIER).full_motion()
}

/// `body-sm` line height: the description's `1lh`.
const LINE: f32 = 18.;
/// `card_ualTdV` height and padding.
pub(super) const CARD_HEIGHT: f32 = 380.;
const PAD: f32 = 20.;
/// The md button: 38px inside a 1px border.
const BUTTON: f32 = 40.;

/// The `primaryAsset` of a card, as the module hands it to `H`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Asset {
    None,
    /// A string: `<img class="primaryAssetImage_ualTdV">` at the picture's own size, at most 180px tall (234 with
    /// `primaryAssetImageLarge`), at most the wrapper's width, 16px corners. `shrink`: `primaryAssetScaleDownOnHover`.
    Image {
        key: String,
        url: String,
        max_height: f32,
        shrink: bool,
    },
    /// An image component of the module (`ik`, `iw`, `iH`...): `width`/`height` from its intrinsic size, the
    /// wrapper's `height: auto !important; max-width: 100%` keeping the ratio. `sticker` adds
    /// `stickerPrimaryAsset_ualTdV` (`scale(.9)`).
    Sized {
        key: String,
        url: String,
        width: f32,
        height: f32,
        sticker: bool,
    },
    /// The tenure badge (`badgeImage_w3BIh9`): at most 230 tall; `dimmed` (upcoming or withheld) is grey at 40% and
    /// 190 wide; `still` (reduced motion) is 220 x 180.
    Badge {
        key: String,
        url: String,
        dimmed: bool,
        still: bool,
    },
    /// `<img width="160">`: the subscriber badge card's picture of the badge the profile shows.
    Wide {
        key: String,
        url: String,
        width: f32,
    },
    /// `iJ`: the Nitro mark, 100 x 80, `ICON_MUTED`.
    NitroMark,
}

impl Asset {
    /// The download behind the asset.
    pub(super) fn source(&self) -> Option<(&str, &str)> {
        match self {
            Asset::Image { key, url, .. }
            | Asset::Sized { key, url, .. }
            | Asset::Badge { key, url, .. }
            | Asset::Wide { key, url, .. } => Some((key, url)),
            Asset::None | Asset::NitroMark => None,
        }
    }
}

/// What a card's button does (`onCtaClick`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    /// `openUserSettings(...)`: the settings page with this slug.
    Settings(&'static str),
    /// `goToShop` / `COLLECTIBLES_SHOP`.
    Shop,
    /// `nf(CUSTOM_THEME)`: the custom theme editor.
    CustomTheme,
    /// The tenure badge's details: the badge directory open on the Nitro badge.
    TenureBadge,
}

/// The footer of the Orbs card (`sl`): days to the next drop, Orbs earned so far.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct OrbsFooter {
    pub days: Option<i64>,
    pub total: i64,
}

/// The props the module passes to `S`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Card {
    pub id: &'static str,
    pub title: String,
    pub description: String,
    pub pill: Option<String>,
    pub caption: Option<String>,
    pub progress: Option<f32>,
    pub asset: Asset,
    pub cta: Option<(String, Action)>,
    pub footer: Option<OrbsFooter>,
    pub categories: &'static [&'static str],
}

/// Lines `text` takes at `width` in the description's face (`body-sm/normal`, 14px).
pub(super) fn description_lines(window: &mut Window, text: &str, width: f32) -> usize {
    if text.is_empty() {
        return 1;
    }
    let mut face = font(SANS);
    face.weight = FontWeight(400.);
    face.features = kerning();
    let run = TextRun {
        len: text.len(),
        font: face,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_text(
            SharedString::from(text.to_owned()),
            px(14.),
            &[run],
            Some(px(width)),
            None,
        )
        .map(|lines| {
            lines
                .iter()
                .map(|line| line.wrap_boundaries().len() + 1)
                .sum::<usize>()
        })
        .unwrap_or(1)
        .max(1)
}

/// The description's height: its lines, held to `max-height` (1lh at rest, 5lh open, in between while it moves),
/// never under one line (`min-height: 1lh`).
pub(super) fn description_height(lines: usize, max_height: f32) -> f32 {
    (lines as f32 * LINE).min(max_height).max(LINE)
}

impl Root {
    /// One perk card `width` wide (`card` and `text` widths come from the grid or the 280px favorites slot).
    pub(super) fn home_perk_card(
        &self,
        card: &Card,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = card.id;
        let hovered = self.nitro.hovered_perk == Some(id);
        let css = transition();
        let open = |name: &str, on: f32, off: f32| {
            motion::value(
                motion::key(&("nitro-perk", id, name)),
                if hovered { on } else { off },
                css,
            )
        };
        let has_cta = card.cta.is_some();
        let max_height = open("description", 5. * LINE, LINE);
        let backdrop = open("backdrop", 1., 0.).clamp(0., 1.);
        let (slot, slot_gap) = if has_cta {
            (
                open("slot", 36., 0.).max(0.),
                open("slot-gap", 16., 0.).max(0.),
            )
        } else {
            (0., 0.)
        };
        let button_shown = open("button", 1., 0.).clamp(0., 1.);
        // `translateY(calc(100% + var(--space-20)))` at rest.
        let button_drop = open("button-drop", 0., BUTTON + PAD);
        let shrink = open("shrink", 0.9, 1.);
        let text_width = (width - 2. * PAD).max(0.);
        let lines = description_lines(window, &card.description, text_width);
        let clamped = !hovered && max_height <= LINE + 0.01;
        let description_height = description_height(lines, max_height);

        let pill = card.pill.clone().map(|text| {
            // `pill_ualTdV`: 24px in, uppercase, `body-xs/semibold` on `--badge-expressive-background`.
            div()
                .absolute()
                .left(px(24.))
                .top(px(24.))
                .px(px(8.))
                .py(px(2.))
                .rounded_full()
                .bg(color("--badge-expressive-background"))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(600.))
                .text_color(color("--badge-expressive-text"))
                .child(text.to_uppercase())
        });

        let art = self.perk_art(&card.asset, text_width, shrink);
        let backdrop_layer = (backdrop > 0.001).then(|| {
            // `blurOverlay_ualTdV`: the card's colour through a mask that is clear at the top and solid from 80px.
            let base = color("--card-background-default");
            let at = |alpha: f32| base.opacity((alpha * backdrop).clamp(0., 1.));
            let band = |top: f32, height: f32, from: f32, to: f32| {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(top))
                    .h(px(height))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(at(from), 0.),
                        linear_color_stop(at(to), 1.),
                    ))
            };
            // `contentContainer_ualTdV` spans the card (margins of -20px), the overlay 1px past its sides and bottom.
            div()
                .absolute()
                .left(px(-PAD))
                .right(px(-PAD))
                .top_0()
                .bottom(px(-PAD))
                .child(band(0., 12., 0., 0.35))
                .child(band(12., 24., 0.35, 0.7))
                .child(band(36., 24., 0.7, 0.92))
                .child(band(60., 20., 0.92, 1.))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(80.))
                        .bottom_0()
                        .bg(at(1.)),
                )
        });

        let description = div()
            .mt(px(4.))
            .h(px(description_height))
            .overflow_hidden()
            .text_size(px(14.))
            .line_height(px(LINE))
            .font_weight(FontWeight(400.))
            .text_color(color("--text-default"))
            .when(clamped, |this| this.line_clamp(1).text_ellipsis())
            .child(card.description.clone());

        let progress = card
            .progress
            .map(|progress| Self::perk_progress(progress, text_width));
        let footer = card.footer.clone().map(Self::orbs_footer);
        let caption = card.caption.clone().map(|caption| {
            div()
                .mb(px(4.))
                .text_size(px(14.))
                .line_height(px(LINE))
                .text_color(color("--text-muted"))
                .child(caption)
        });

        // `textContent_ualTdV` keeps its 20px top padding inside the content container.
        let text = div()
            .relative()
            .flex_none()
            .pt(px(PAD))
            .flex()
            .flex_col()
            .children(backdrop_layer)
            .children(caption)
            .child(
                div()
                    .relative()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(card.title.clone()),
            )
            .child(div().relative().child(description))
            .children(progress.map(|bar| div().relative().mt(px(12.)).child(bar)))
            .children(footer.map(|footer| div().relative().mt(px(12.)).child(footer)))
            .when(has_cta, |this| {
                this.child(div().mt(px(slot_gap)).h(px(slot)).flex_none())
            });

        let button = card.cta.clone().map(|(label, action)| {
            let hover = (
                color("--control-overlay-primary-background-hover"),
                color("--control-overlay-primary-border-hover"),
            );
            let active = (
                color("--control-overlay-primary-background-active"),
                color("--control-overlay-primary-border-active"),
            );
            div()
                .absolute()
                .left(px(PAD))
                .right(px(PAD))
                .bottom(px(PAD - button_drop))
                .opacity(button_shown)
                .child(
                    div()
                        .id(SharedString::from(format!("nitro-perk-cta-{id}")))
                        .w_full()
                        .h(px(BUTTON))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-overlay-primary-border-default"))
                        .bg(color("--control-overlay-primary-background-default"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-overlay-primary-text-default"))
                        .hover(move |style| style.bg(hover.0).border_color(hover.1))
                        .active(move |style| style.bg(active.0).border_color(active.1))
                        .rule(motion::Rule::Button)
                        .child(label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.perk_action(action, window, cx)
                        })),
                )
        });

        div()
            .id(SharedString::from(format!("nitro-perk-{id}")))
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(CARD_HEIGHT))
            .rounded(px(16.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--card-background-default"))
            .overflow_hidden()
            .p(px(PAD - 1.))
            .flex()
            .flex_col()
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                let now = over.then_some(id);
                if this.nitro.hovered_perk != now && (*over || this.nitro.hovered_perk == Some(id))
                {
                    this.nitro.hovered_perk = now;
                    cx.notify();
                }
            }))
            // `box-shadow: inset 0 0 0 1px var(--border-subtle)` inside the 1px border.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(15.))
                    .border_1()
                    .border_color(color("--border-subtle")),
            )
            .children(pill)
            // `primaryAssetWrapper_ualTdV`: takes what the text leaves, the art centred (and free to spill under the
            // text, which is drawn over it).
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .max_w(px(400.))
                    .mx_auto()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(art),
            )
            .child(text)
            .children(button)
            .into_any_element()
    }

    /// `H`: the art of a card in the wrapper `room` wide.
    fn perk_art(&self, asset: &Asset, room: f32, shrink: f32) -> AnyElement {
        let picture = |key: &str| self.images.get(key).cloned();
        match asset {
            Asset::None => div().into_any_element(),
            Asset::Image {
                key,
                max_height,
                shrink: scales,
                ..
            } => match picture(key) {
                Some(image) => {
                    let scale = if *scales { shrink } else { 1. };
                    img(image)
                        .max_h(px(max_height * scale))
                        .max_w(px(room * scale))
                        .rounded(px(16.))
                        .object_fit(ObjectFit::Contain)
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
            Asset::Sized {
                key,
                width,
                height,
                sticker,
                ..
            } => {
                // The box is laid out before the picture arrives, so nothing moves when it does.
                let shown = width.min(room);
                let tall = shown * height / width;
                let scale = if *sticker { 0.9 } else { 1. };
                div()
                    .flex_none()
                    .w(px(shown * scale))
                    .h(px(tall * scale))
                    .children(
                        picture(key)
                            .map(|image| img(image).size_full().object_fit(ObjectFit::Contain)),
                    )
                    .into_any_element()
            }
            Asset::Badge {
                key, dimmed, still, ..
            } => match picture(key) {
                Some(image) => {
                    let base = img(image).max_w(px(room)).object_fit(ObjectFit::Contain);
                    if *still {
                        base.w(px(220.)).h(px(180.)).into_any_element()
                    } else if *dimmed {
                        base.w(px(190.))
                            .max_h(px(230.))
                            .grayscale(true)
                            .opacity(0.4)
                            .into_any_element()
                    } else {
                        base.max_h(px(230.)).into_any_element()
                    }
                }
                None => div().into_any_element(),
            },
            Asset::Wide { key, width, .. } => match picture(key) {
                Some(image) => img(image)
                    .w(px(width.min(room)))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                None => div().into_any_element(),
            },
            Asset::NitroMark => icon("nitro_wheel", 80., "--icon-muted")
                .w(px(100.))
                .h(px(80.))
                .into_any_element(),
        }
    }

    /// `x.Ay` (module 914410), variant blue, weight light: a 4px track and the bar's gradient in the theme's blurples.
    fn perk_progress(progress: f32, width: f32) -> AnyElement {
        let (start, end) = match super::super::theme() {
            dc_tokens::Theme::Light => ("--blurple-80", "--blurple-38"),
            dc_tokens::Theme::Dark => ("--blurple-44", "--blurple-24"),
            dc_tokens::Theme::Darker => ("--blurple-54", "--blurple-24"),
            dc_tokens::Theme::Midnight => ("--blurple-59", "--blurple-24"),
        };
        let empty = progress <= 0.;
        // `min-width: 10px`.
        let filled = (width * progress.clamp(0., 1.)).max(10.);
        div()
            .w_full()
            .h(px(4.))
            .rounded(px(5.))
            .bg(color("--background-mod-normal"))
            .child(
                div()
                    .w(px(filled))
                    .h_full()
                    .rounded(px(5.))
                    .when(empty, |bar| bar.bg(color(end)))
                    .when(!empty, |bar| {
                        bar.bg(linear_gradient(
                            90.,
                            linear_color_stop(color(start), 0.),
                            linear_color_stop(color(end), 1.),
                        ))
                    }),
            )
            .into_any_element()
    }

    /// `sl`: the Orbs card's box of two rows split by a line.
    fn orbs_footer(footer: OrbsFooter) -> AnyElement {
        let row = |glyph: &'static str, label: String, value: String| {
            div()
                .px(px(16.))
                .py(px(12.))
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(14.))
                .line_height(px(LINE))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon(glyph, 18., "--text-default"))
                        .child(label),
                )
                .child(value)
        };
        let drop = footer.days.map(|days| {
            row(
                "field_calendar",
                dc_intl::t("H2M13c").to_owned(),
                dc_intl::format("xedPIb", &[("days", days.into())]),
            )
        });
        let earned = (footer.total > 0).then(|| {
            row(
                "pm_orbs",
                dc_intl::t("F7Bhsg").to_owned(),
                dc_intl::format("UDwsvL", &[("orbsCount", footer.total.into())]),
            )
        });
        let divider = (drop.is_some() && earned.is_some())
            .then(|| div().h(px(1.)).bg(color("--border-muted")));
        div()
            .rounded(px(12.))
            .overflow_hidden()
            .bg(color("--background-mod-normal"))
            .flex()
            .flex_col()
            .children(drop)
            .children(divider)
            .children(earned)
            .into_any_element()
    }

    /// The button of a card.
    fn perk_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        match action {
            Action::Settings(slug) => self.open_settings_page(slug, cx),
            Action::Shop => self.open_shop(cx),
            Action::CustomTheme => self.open_custom_theme_editor(cx),
            Action::TenureBadge => self.open_badge_modal(None, None, Some(1), true, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_description_is_one_line_at_rest_and_at_most_five_open() {
        assert_eq!(description_height(3, LINE), LINE);
        assert_eq!(description_height(3, 5. * LINE), 3. * LINE);
        assert_eq!(description_height(8, 5. * LINE), 5. * LINE);
        // Part way: the box grows with max-height until the text's own height stops it.
        assert_eq!(description_height(2, 30.), 30.);
        assert_eq!(description_height(2, 60.), 36.);
        assert_eq!(description_height(1, 90.), LINE);
    }

    #[test]
    fn the_card_curve_overshoots_as_the_stylesheet_says() {
        // cubic-bezier(.36,.35,.1,1.23) passes 1 before the end.
        let peak = (1..100)
            .map(|step| BEZIER.apply(step as f64 / 100.))
            .fold(0., f64::max);
        assert!(peak > 1.01, "{peak}");
        assert!((BEZIER.apply(1.) - 1.).abs() < 1e-9);
        assert_eq!(transition().duration_ms, 400.);
    }
}
