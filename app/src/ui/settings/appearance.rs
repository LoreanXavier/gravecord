//! The Appearance page's own rows: default theme swatches (`themeSelection_IhyaN_`), the custom theme card
//! (`container_yH_g_W`) and the color themes panel (`colorThemesBackground_A7cGXu`). Measured in
//! `design/dom/settings/appearance-darker-cozy.json`.

use super::model::{Layer, Swatch};
use super::swatches::SWATCHES;
use super::{Root, button, color, mask_icon};
use crate::ui::{set_theme, theme};
use dc_tokens::Theme;
use gpui::{
    AnyElement, Context, FontWeight, Image, ImageFormat, IntoElement, SharedString, div, img,
    prelude::*, px,
};
use std::sync::Arc;

fn rgba_hex(value: u32) -> (String, f32) {
    (format!("#{:06x}", value >> 8), (value & 0xff) as f32 / 255.)
}

/// One gradient layer as an SVG gradient across a `size` square, CSS angle conventions.
fn layer_svg(id: usize, layer: &Layer, size: f32) -> (String, String) {
    let radians = layer.angle.to_radians();
    let (dx, dy) = (radians.sin(), -radians.cos());
    // The CSS gradient line passes through the centre and is long enough to reach both far corners.
    let length = (size * dx.abs()) + (size * dy.abs());
    let centre = size / 2.;
    let (x1, y1) = (centre - dx * length / 2., centre - dy * length / 2.);
    let (x2, y2) = (centre + dx * length / 2., centre + dy * length / 2.);
    let stops: String = layer
        .stops
        .iter()
        .map(|(value, at)| {
            let (hex, alpha) = rgba_hex(*value);
            format!(r#"<stop offset="{at}" stop-color="{hex}" stop-opacity="{alpha}"/>"#)
        })
        .collect();
    (
        format!(
            r#"<linearGradient id="g{id}" gradientUnits="userSpaceOnUse" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}">{stops}</linearGradient>"#
        ),
        format!(r#"<rect width="{size}" height="{size}" rx="8" fill="url(#g{id})"/>"#),
    )
}

pub(super) fn swatch_image(swatch: &Swatch) -> Arc<Image> {
    let size = 48.;
    let mut defs = String::new();
    let mut body = String::new();
    if let Some(base) = swatch.base {
        let (hex, alpha) = rgba_hex(base);
        body.push_str(&format!(
            r#"<rect width="{size}" height="{size}" rx="8" fill="{hex}" fill-opacity="{alpha}"/>"#
        ));
    }
    // CSS lists the top layer first; paint from the bottom.
    for (id, layer) in swatch.layers.iter().enumerate().rev() {
        let (gradient, rect) = layer_svg(id, layer, size);
        defs.push_str(&gradient);
        body.push_str(&rect);
    }
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}"><defs>{defs}</defs>{body}</svg>"#
    );
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
}

fn palette_of(name: &str) -> Option<Theme> {
    match name {
        "Light" => Some(Theme::Light),
        "Ash" => Some(Theme::Dark),
        "Dark" => Some(Theme::Darker),
        "Onyx" => Some(Theme::Midnight),
        _ => None,
    }
}

impl Root {
    /// A 48px swatch with its 1px inset ring; the chosen one wears a 4px brand ring and a check.
    fn swatch(
        &self,
        swatch: &'static Swatch,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picture = swatch_image(swatch);
        let palette = palette_of(swatch.name);
        div()
            .id(SharedString::from(format!("swatch-{}", swatch.name)))
            .relative()
            .flex_none()
            .size(px(48.))
            .cursor_pointer()
            .when_some(palette, |this, palette| {
                this.on_click(cx.listener(move |this, _, _, cx| {
                    set_theme(palette);
                    this.request_emoji_button();
                    cx.refresh_windows();
                    cx.notify();
                }))
            })
            .child(img(picture).size(px(48.)).rounded(px(8.)))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(px(48.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--interactive-text-default")),
            )
            .when(swatch.name == "Sync with computer", |this| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(48.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(mask_icon("theme_sync", 24., "--interactive-text-default")),
                )
            })
            .when(selected, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(-2.))
                        .left(px(-2.))
                        .size(px(52.))
                        .rounded(px(8.))
                        .border_4()
                        .border_color(color("--background-brand")),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(-8.))
                        .left(px(32.))
                        .size(px(24.))
                        .child(div().absolute().child(mask_icon(
                            "theme_check_disc",
                            24.,
                            "--white",
                        )))
                        .child(div().absolute().child(mask_icon(
                            "theme_check",
                            24.,
                            "--background-brand",
                        ))),
                )
            })
            .into_any_element()
    }

    pub(super) fn default_themes(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = theme();
        let swatches: Vec<AnyElement> = SWATCHES
            .iter()
            .filter(|swatch| swatch.group == "default")
            .map(|swatch| self.swatch(swatch, palette_of(swatch.name) == Some(current), cx))
            .collect();
        div()
            .py(px(8.))
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Default Themes"),
            )
            .child(div().pt(px(24.)).flex().gap(px(8.)).children(swatches))
            .into_any_element()
    }

    pub(super) fn custom_theme_card(&self) -> AnyElement {
        let banner = self.images.get("theme_upsell_banner").cloned();
        div()
            .py(px(8.))
            .child(
                div()
                    .h(px(104.))
                    .px(px(25.))
                    .py(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_none()
                            .w(px(117.))
                            .h(px(72.))
                            .children(banner.map(|picture| img(picture).w(px(117.)).h(px(69.)))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child("Make your Discord look *juuusst* right"),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child("Create your own custom theme to make Discord uniquely yours. Only with Nitro."),
                            ),
                    )
                    .child(button("theme-try", "Try it out", super::ButtonKind::Primary)),
            )
            .into_any_element()
    }

    pub(super) fn color_themes(&self, cx: &mut Context<Self>) -> AnyElement {
        let swatches: Vec<AnyElement> = SWATCHES
            .iter()
            .filter(|swatch| swatch.group == "client")
            .map(|swatch| {
                if swatch.name == "Custom Theme" {
                    div()
                        .relative()
                        .flex_none()
                        .size(px(48.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-strong"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(mask_icon("theme_palette", 24., "--text-default"))
                        // `badgeContainer_lUEm2A`: a brand ring 4px out and a NEW badge over the top edge.
                        .child(div().absolute().top(px(-5.)).left(px(-5.)).child(mask_icon(
                            "theme_new_ring",
                            56.,
                            "--background-brand",
                        )))
                        .child(
                            div()
                                .absolute()
                                .top(px(-9.))
                                .left(px(2.))
                                .child(super::new_badge()),
                        )
                        .into_any_element()
                } else {
                    self.swatch(swatch, false, cx)
                }
            })
            .collect();
        div()
            .py(px(8.))
            .child(
                div()
                    .p(px(24.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-normal"))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(40.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(2.))
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child("Color Themes")
                                            .child(mask_icon("nitro_wheel", 16., "--text-default")),
                                    )
                                    .child(super::described(
                                        &[
                                            ("Unlock more themes with ", false),
                                            ("Nitro", true),
                                            (".", false),
                                        ],
                                        "--text-subtle",
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(12.))
                                    .child(button(
                                        "theme-preview",
                                        "Preview Theme",
                                        super::ButtonKind::Secondary,
                                    ))
                                    // `expressive_F7qvhY`: white, black text, the Nitro mark before it.
                                    .child(
                                        div()
                                            .h(px(40.))
                                            .px(px(15.))
                                            .rounded(px(8.))
                                            .bg(gpui::white())
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(gpui::black())
                                            .child(mask_icon("nitro_wheel", 20., "--black"))
                                            .child("Unlock with Nitro"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .pt(px(24.))
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .children(swatches),
                    ),
            )
            .into_any_element()
    }
}
