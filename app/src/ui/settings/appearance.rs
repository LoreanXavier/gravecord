//! The Appearance page's own rows: default theme swatches (`themeSelection_IhyaN_`), the custom theme card
//! (`container_yH_g_W`) and the color themes panel (`colorThemesBackground_A7cGXu`). Measured in
//! `design/dom/settings/appearance-darker-cozy.json`.

use super::model::{Layer, Swatch};
use super::swatches::SWATCHES;
use super::{Root, button, color, mask_icon};
use dc_tokens::Theme;
use gpui::{
    AnyElement, BoxShadow, Context, FontWeight, Image, ImageFormat, IntoElement, Pixels,
    SharedString, Window, div, img, point, prelude::*, px,
};
use std::sync::Arc;

/// The in-app icons of `746322` `v8()` in the order the page lists them: the name and the asset's file.
pub(super) const APP_ICONS: &[(&str, &str)] = &[
    ("Blurple (Default)", "5926a37cee0904ce"),
    ("Goth", "e8396e79419904ca"),
    ("WHAM", "2d62663c4b493f8e"),
    ("Charcoal", "28f89b5f78dd6f80"),
    ("Ceramic", "96f9a2942150ba17"),
    ("Pastel", "22bb7108ad1c466f"),
    ("Matey", "9af4b996c897420f"),
    ("Tactical", "9ad8ec824f71bd05"),
    ("Sunset Ave", "5bf3279039efa66c"),
    ("Galactic Chrome", "3a2d57c0bde9b218"),
    ("Holo", "181e40b5ea872cdb"),
    ("Sakura", "1e208c512641758d"),
    ("Sherbet Dreamsicle", "e14578cc054054c0"),
    ("Gaming", "5e5cc9b058e63987"),
    ("Mainframe", "82c7a89d15622af4"),
    ("Prismatic Waves", "441d164362b711b1"),
    ("uwu~", "e5f2a55421a6dc7e"),
    ("Fuming", "9b50f7ba00ee941f"),
    ("D64", "a629a2a3fe1ccbcc"),
    ("Blurple Twilight", "685b2900a9423534"),
    ("In Rainbows", "10cd104567438715"),
    ("Midnight Prism", "7ebbc1790864207a"),
    ("Color Wave", "7ce5a942a5cc7cba"),
];

/// The ids of `APP_ICONS`, in the same order (`LW.DEFAULT` and `Ic` of module 350972).
pub(super) const APP_ICON_IDS: [&str; 23] = [
    "AppIcon",
    "BrandDarkIcon",
    "MangaIcon",
    "MatteDarkIcon",
    "MatteLightIcon",
    "PastelIcon",
    "PirateIcon",
    "CamoIcon",
    "SunsetIcon",
    "GalaxyIcon",
    "Y2KIcon",
    "CherryBlossomIcon",
    "BeanieIcon",
    "GamingIcon",
    "CircuitIcon",
    "HoloWavesIcon",
    "BlushIcon",
    "AngryIcon",
    "ControllerIcon",
    "BlurpleTwilightIcon",
    "InRainbowsIcon",
    "MidnightPrismIcon",
    "ColorWaveIcon",
];

/// `AppIconPersistedStoreState` (module 526162): a local store, `{client: {desktop, coachmarkImpressions}}`.
const APP_ICON_STORE: &str = "AppIconPersistedStoreState";

/// The index of the icon an `AppIconPersistedStoreState` value holds; an unknown or missing id is the default.
pub(super) fn app_icon_of(state: &serde_json::Value) -> usize {
    state
        .pointer("/client/desktop")
        .and_then(|id| id.as_str())
        .and_then(|id| APP_ICON_IDS.iter().position(|known| *known == id))
        .unwrap_or(0)
}

/// `APP_ICON_UPDATED`: the store's `desktop` takes the id, its impressions stay.
pub(super) fn app_icon_updated(state: serde_json::Value, index: usize) -> serde_json::Value {
    let mut state = if state.is_object() {
        state
    } else {
        serde_json::json!({})
    };
    let client = state
        .as_object_mut()
        .map(|state| {
            state
                .entry("client")
                .or_insert_with(|| serde_json::json!({"coachmarkImpressions": 0}))
        })
        .and_then(|client| client.as_object_mut());
    if let Some(client) = client {
        client.insert(
            "desktop".to_owned(),
            serde_json::json!(APP_ICON_IDS[index.min(APP_ICON_IDS.len() - 1)]),
        );
    }
    state
}

/// The icon `AppIconPersistedStoreState` holds on this machine, whatever the account's Nitro.
fn stored_app_icon() -> usize {
    crate::storage::get(APP_ICON_STORE)
        .map(|state| app_icon_of(&state))
        .unwrap_or(0)
}

pub(super) fn app_icon_key(index: usize) -> String {
    format!("app_icon:{index}")
}

pub(super) fn app_icon_url(index: usize) -> String {
    format!("https://discord.com/assets/{}.webp", APP_ICONS[index].1)
}

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
    pub(super) fn icon_set_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::branding::{self, IconSet};
        let selected = branding::selected();
        let options = [
            (IconSet::Gravecord, "Gravecord"),
            (IconSet::Original, "Original"),
        ]
        .into_iter()
        .map(|(set, label)| {
            div()
                .id(SharedString::from(format!("icon-set-{}", set.name())))
                .flex_1()
                .h(px(64.))
                .px(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color(if set == selected {
                    "--border-strong"
                } else {
                    "--border-subtle"
                }))
                .bg(color("--background-surface-high"))
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .on_click(cx.listener(move |_, _, window, cx| {
                    branding::select(set);
                    crate::notifications::refresh_window_icons(window);
                    cx.notify();
                }))
                .child(img(branding::preview(set)).size(px(32.)))
                .child(div().flex_1().child(label))
                .child(super::radio_dot(set == selected))
        });
        div().id("appearance-icon-set").flex().flex_col().gap(px(8.)).pb(px(24.))
            .text_size(px(16.)).line_height(px(20.)).text_color(color("--text-strong"))
            .child(div().font_weight(FontWeight(600.)).child("Icon Set"))
            .child(div().text_size(px(14.)).line_height(px(18.)).text_color(color("--text-subtle"))
                .child("Choose Gravecord or original icons. Windows shortcuts keep the Gravecord icon."))
            .child(div().flex().gap(px(12.)).children(options)).into_any_element()
    }

    /// A 48px swatch with its 1px inset ring; the chosen one wears a 4px brand ring and a check. The caller adds the
    /// click.
    pub(super) fn swatch_base(
        &self,
        swatch: &'static Swatch,
        selected: bool,
    ) -> gpui::Stateful<gpui::Div> {
        let picture = swatch_image(swatch);
        div()
            .id(SharedString::from(format!("swatch-{}", swatch.name)))
            .relative()
            .flex_none()
            .size(px(48.))
            .cursor_pointer()
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
    }

    fn swatch(
        &self,
        swatch: &'static Swatch,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = palette_of(swatch.name);
        self.swatch_base(swatch, selected)
            .when_some(palette, |this, palette| {
                this.on_click(cx.listener(move |this, _, _, cx| {
                    this.choose_theme(palette);
                    cx.refresh_windows();
                    cx.notify();
                }))
            })
            // A colour theme: Nitro applies and saves it, any other account previews it.
            .when(
                palette.is_none() && swatch.group == "client" && !swatch.layers.is_empty(),
                |this| {
                    let name = swatch.name;
                    this.on_click(
                        cx.listener(move |this, _, _, cx| this.pick_client_theme(name, cx)),
                    )
                },
            )
            .when(
                palette.is_none()
                    && swatch.name != "Sync with computer"
                    && (swatch.group != "client" || swatch.layers.is_empty()),
                |this| this.on_click(cx.listener(|this, _, _, cx| this.open_purchase(false, cx))),
            )
            // `u_({theme: "system"})`: the device setting on, no colour theme.
            .when(swatch.name == "Sync with computer", |this| {
                this.on_click(cx.listener(|this, _, _, cx| {
                    this.theme_choose_system();
                    cx.refresh_windows();
                    cx.notify();
                }))
            })
            .into_any_element()
    }

    /// `appearance.theme`: shown now, kept for the next start and sent to the account as the official client does
    /// (`theme_state`). Choosing a palette by hand ends "Same as Device Theme" and any colour theme.
    pub(super) fn choose_theme(&mut self, palette: Theme) {
        self.theme_choose_default(super::theme_state::proto_of(palette));
        self.request_emoji_button();
    }

    pub(super) fn default_themes(&self, cx: &mut Context<Self>) -> AnyElement {
        // "Same as Device Theme" swaps the row for the Light Mode and Dark Mode selectors.
        if self.theme_same_as_device() {
            return self.dual_theme_selector(cx);
        }
        let swatches: Vec<AnyElement> = SWATCHES
            .iter()
            .filter(|swatch| swatch.group == "default")
            .map(|swatch| self.swatch(swatch, self.theme_tile_selected(swatch.name), cx))
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

    pub(super) fn custom_theme_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The text column shrinks with the card and stops at its longest word (`min-width: auto`).
        let title = "Make your Discord look *juuusst* right";
        let body = "Create your own custom theme to make Discord uniquely yours. Only with Nitro.";
        let widest = |window: &mut Window, text: &str, size: f32, weight: f32| {
            text.split_whitespace()
                .map(|word| super::render::measure_text(window, word, size, weight))
                .fold(px(0.), Pixels::max)
        };
        let column_min = widest(window, title, 20., 600.).max(widest(window, body, 14., 400.));
        let banner = self.images.get("theme_upsell_banner").cloned();
        div()
            .py(px(8.))
            .child(
                div()
                    // Grows with the wrapped text at a narrow width (the 200% golden's card is 180 high).
                    .min_h(px(104.))
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
                            .min_w(column_min)
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
                    // "Try it out" (`uw9zI7`, module 569386): the same Preview Theme mode a free account gets from the
                    // colour themes panel (`nf(CUSTOM_THEME)` turns the upsell preview on and closes settings).
                    .child(
                        div()
                            .id("theme-try-click")
                            // A subscriber gets the same editor; it used to switch to a light preset instead.
                            .on_click(cx.listener(|this, _, _, cx| this.open_custom_theme_editor(cx)))
                            .child(button("theme-try", "Try it out", super::ButtonKind::Primary)),
                    ),
            )
            .into_any_element()
    }

    /// `expressive_F7qvhY`: white, black text, the Nitro mark before it, and a glow of the Nitro colours round it
    /// (the canvas of 193 by 56 behind the 177 by 40 button). It opens the Nitro plans.
    pub(super) fn unlock_with_nitro(&self, id: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let glow = |red: f32, green: f32, blue: f32, alpha: f32, x: f32, y: f32| BoxShadow {
            color: hsla_from_rgb(red, green, blue, alpha),
            offset: point(px(x), px(y)),
            blur_radius: px(16.),
            spread_radius: px(0.),
            inset: false,
        };
        div()
            .id(id)
            .h(px(40.))
            .px(px(15.))
            .rounded(px(8.))
            .bg(gpui::white())
            .shadow(vec![
                glow(0.30, 0.55, 1.0, 0.55, -6., -3.),
                glow(0.62, 0.35, 0.90, 0.55, 8., -3.),
                glow(0.20, 0.80, 0.70, 0.45, -8., 5.),
                glow(0.85, 0.35, 0.80, 0.40, 8., 5.),
            ])
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(gpui::black())
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.open_purchase(false, cx)))
            .child(mask_icon("nitro_wheel", 20., "--black"))
            .child("Unlock with Nitro")
            .into_any_element()
    }

    /// The App Icon header's buttons (`useHeaderDecoration`): Preview Icon, then Unlock with Nitro.
    pub(super) fn app_icon_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        // `isUpsellPreview` of `AppIconPersistedStore` (module 526162) is `!canUsePremiumAppIcons`: a subscriber has no
        // upsell button, only Preview Icon.
        let unlock = (self.premium() == dc_state::premium::PremiumType::None)
            .then(|| self.unlock_with_nitro("icon-unlock", cx));
        div()
            .flex()
            .gap(px(12.))
            .child(
                div()
                    .id("icon-preview-click")
                    .on_click(cx.listener(|this, _, _, cx| this.open_purchase(false, cx)))
                    .child(button(
                        "icon-preview",
                        "Preview Icon",
                        super::ButtonKind::Secondary,
                    )),
            )
            .children(unlock)
            .into_any_element()
    }

    /// The in-app icon worn now (`getCurrentDesktopIcon`). Without Nitro the store falls back to the default
    /// (`f` of module 526162 resets it while `isUpsellPreview`).
    pub(in crate::ui) fn app_icon_selected(&self) -> usize {
        if self.premium() == dc_state::premium::PremiumType::None {
            return 0;
        }
        stored_app_icon()
    }

    /// The picture the server rail's Home button wears instead of Clyde (`nd` of module 396655): the in-app icon
    /// worn now, once it is not the default one and its picture has arrived.
    pub(in crate::ui) fn rail_app_icon(&self) -> Option<Arc<Image>> {
        let index = self.app_icon_selected();
        if index == 0 {
            return None;
        }
        self.images.get(&app_icon_key(index)).cloned()
    }

    /// Asks for the stored in-app icon's picture, so the Home button can wear it before Appearance ever opens.
    pub(in crate::ui) fn request_stored_app_icon(&mut self) {
        let index = stored_app_icon();
        if index != 0 {
            self.request_image(app_icon_key(index), app_icon_url(index));
        }
    }

    fn choose_app_icon(&mut self, index: usize, cx: &mut Context<Self>) {
        let state = crate::storage::get(APP_ICON_STORE).unwrap_or(serde_json::Value::Null);
        crate::storage::set(APP_ICON_STORE, app_icon_updated(state, index));
        cx.notify();
    }

    /// `appIconSelection_jWcVMn`: the in-app icons, 48px, 8px apart. A free account sees them locked
    /// (`disabled: isUpsellPreview`): Blurple wears the brand ring and the check, the rest take no clicks. A Nitro
    /// account picks one (`APP_ICON_UPDATED`, module 92533 `w`).
    pub(super) fn app_icon_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.app_icon_selected();
        let enabled = self.premium() != dc_state::premium::PremiumType::None;
        let tiles: Vec<AnyElement> = APP_ICONS
            .iter()
            .enumerate()
            .map(|(index, (name, _))| {
                let selected = index == chosen;
                let picture = self.images.get(&app_icon_key(index)).cloned();
                div()
                    .id(SharedString::from(format!("app-icon-{name}")))
                    .relative()
                    .flex_none()
                    .size(px(48.))
                    .rounded(px(8.))
                    .when(enabled, |this| {
                        this.cursor_pointer().on_click(
                            cx.listener(move |this, _, _, cx| this.choose_app_icon(index, cx)),
                        )
                    })
                    .children(picture.map(|image| img(image).size(px(48.)).rounded(px(8.))))
                    .when(selected, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(-2.))
                                .left(px(-2.))
                                .size(px(52.))
                                .rounded(px(10.))
                                .border_2()
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
            })
            .collect();
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            // `baseSettingWrapper_4Z2WJy` keeps 8px above and below its content (the grid's wrapper is 120px
            // tall around 104px of tiles in `dom/settings/appearance-darker-cozy`).
            .py(px(8.))
            .children(tiles)
            .into_any_element()
    }

    pub(super) fn color_themes(&self, cx: &mut Context<Self>) -> AnyElement {
        let nitro = self.premium() != dc_state::premium::PremiumType::None;
        let saved_tile = self
            .custom_wanted()
            .and_then(|saved| super::custom_theme::tile_image(&saved));
        let swatches: Vec<AnyElement> = SWATCHES
            .iter()
            .filter(|swatch| swatch.group == "client")
            .map(|swatch| {
                if swatch.name == "Custom Theme" {
                    div()
                        .id("theme-custom-tile")
                        .relative()
                        .flex_none()
                        .size(px(48.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-strong"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        // The tile opens the custom theme editor for every account (module 569386).
                        .on_click(cx.listener(|this, _, _, cx| this.open_custom_theme_editor(cx)))
                        // An account that holds a custom theme: its colours fill the tile, the palette is white and
                        // the selection circle sits on the corner (`Q` of module 270746, `showSelectionCircle`).
                        .children(saved_tile.clone().map(|picture| {
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .child(img(picture).size(px(48.)).rounded(px(8.)))
                        }))
                        .child(mask_icon(
                            "theme_palette",
                            24.,
                            if saved_tile.is_some() {
                                "--white"
                            } else {
                                "--text-default"
                            },
                        ))
                        .children(saved_tile.is_some().then(|| {
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
                                )))
                        }))
                        // `badgeContainer_lUEm2A`: a brand ring 4px out and a NEW badge over the top edge.
                        .children(saved_tile.is_none().then(|| {
                            div().absolute().top(px(-5.)).left(px(-5.)).child(mask_icon(
                                "theme_new_ring",
                                56.,
                                "--background-brand",
                            ))
                        }))
                        .child(
                            div()
                                .absolute()
                                .top(px(-9.))
                                .left(px(2.))
                                .child(super::new_badge()),
                        )
                        .into_any_element()
                } else {
                    let chosen = !super::client_theme::previewing()
                        && super::client_theme::preset_id(swatch.name)
                            .is_some_and(|id| self.theme_wanted().preset == Some(id));
                    self.swatch(swatch, chosen, cx)
                }
            })
            .collect();
        div()
            .py(px(8.))
            .child(
                div()
                    // Preview (no Nitro): `colorThemesBackground_A7cGXu`, a bordered panel. A subscriber sees the
                    // header and the tiles without it.
                    .when(!nitro, |this| {
                        this.p(px(24.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-normal"))
                            .bg(color("--background-base-lower"))
                    })
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            // `header_QF5mJe` is as tall as its content: in the 200% capture the heading shrinks to
                            // its min-content (the title and the subtext wrap) beside the two buttons.
                            .min_h(px(40.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_shrink(1.)
                                    // 146.1 device px at dpr 2 (the dump's `headings_QF5mJe`).
                                    .min_w(px(73.05))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(2.))
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            // The title wraps with the heading (its widest word, "Themes", is
                                            // 110.1 device px in the 200% dump).
                                            .child(
                                                div()
                                                    .flex_shrink(1.)
                                                    .min_w(px(55.05))
                                                    .child("Color Themes"),
                                            )
                                            .child(mask_icon("nitro_wheel", 16., "--text-default")),
                                    )
                                    .child(if nitro {
                                        // `np0X/u`
                                        super::described(
                                            &[("Make Discord yours. Only with Nitro.", false)],
                                            "--text-subtle",
                                        )
                                        .into_any_element()
                                    } else {
                                        // `DWIjJ/` with `onPremiumClick` (module 87719 `e`): the Nitro page.
                                        let root = cx.weak_entity();
                                        super::described_linked(
                                            "appearance-themes-nitro".into(),
                                            &[
                                                ("Unlock more themes with ", false),
                                                ("Nitro", true),
                                                (".", false),
                                            ],
                                            "--text-subtle",
                                            Box::new(move |_, cx| {
                                                let _ = root.update(cx, |this, cx| {
                                                    this.open_settings_section("Nitro", cx);
                                                });
                                            }),
                                        )
                                        .into_any_element()
                                    }),
                            )
                            .when(!nitro, |this| {
                                this.child(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .id("theme-preview-click")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.pick_client_theme("Mint Apple", cx)
                                                }))
                                                .child(button(
                                                    "theme-preview",
                                                    "Preview Theme",
                                                    super::ButtonKind::Secondary,
                                                )),
                                        )
                                        .child(self.unlock_with_nitro("theme-unlock", cx)),
                                )
                            }),
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

fn hsla_from_rgb(red: f32, green: f32, blue: f32, alpha: f32) -> gpui::Hsla {
    gpui::Rgba {
        r: red,
        g: green,
        b: blue,
        a: alpha,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_in_app_icon_has_its_official_id() {
        assert_eq!(APP_ICON_IDS.len(), APP_ICONS.len());
        assert_eq!(APP_ICON_IDS[0], "AppIcon");
        assert_eq!(APP_ICONS[2].0, "WHAM");
        assert_eq!(APP_ICON_IDS[2], "MangaIcon");
        assert_eq!(APP_ICON_IDS[22], "ColorWaveIcon");
    }

    #[test]
    fn the_chosen_icon_is_kept_in_the_persisted_store_shape() {
        assert_eq!(app_icon_of(&serde_json::Value::Null), 0);
        let state = app_icon_updated(serde_json::Value::Null, 9);
        assert_eq!(
            state,
            json!({"client": {"desktop": "GalaxyIcon", "coachmarkImpressions": 0}})
        );
        assert_eq!(app_icon_of(&state), 9);
        // Impressions survive a later pick, an unknown id reads as the default.
        let seen = json!({"client": {"desktop": "AppIcon", "coachmarkImpressions": 2}});
        assert_eq!(
            app_icon_updated(seen, 1),
            json!({"client": {"desktop": "BrandDarkIcon", "coachmarkImpressions": 2}})
        );
        assert_eq!(app_icon_of(&json!({"client": {"desktop": "Gone"}})), 0);
    }
}
