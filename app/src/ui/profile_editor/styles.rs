//! The display name style editor ("Change Display Name Style", module 974591) and the Nitro preview's presets
//! (module 461797). The choices of fonts, effects and colours come from module 750656; the effect and colour
//! counts from module 501803. The preview side shows the profile, a message and a member row with the style.

use super::colour::{self, Target};
use super::{Mode, text};
use crate::parity::Parity;
use crate::ui::modal::{self, Tone};
use crate::ui::motion::Posed as _;
use crate::ui::{Root, color, icon, name_styles};
use dc_state::User;
use dc_state::display_name_styles::{DisplayNameStyles, Effect};
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, ObjectFit, ScrollHandle, SharedString,
    Window, deferred, div, img, linear_color_stop, linear_gradient, prelude::*, px, rgba,
};

/// Fonts in the order of the editor's grid (`rA` of module 750656): the normal font, then the ten fonts.
pub(super) const FONTS: [u32; 12] = [11, 12, 3, 4, 6, 7, 8, 10, 13, 14, 15, 16];
/// The last four fonts and the last two effects carry the Nitro-only dot (`_k` and `gz`).
const PREMIUM_FONTS: [u32; 4] = [13, 14, 15, 16];
/// Effects in the order of the grid (`re`): Solid, Gradient, Neon, Toon, Pop, Gummy, Prism.
pub(super) const EFFECTS: [u32; 7] = [1, 2, 3, 4, 5, 8, 7];
const PREMIUM_EFFECTS: [u32; 2] = [8, 7];
/// `DisplayNameStylesSeenStore` (module 974591): a "new" dot stays on a tile until that font or effect was chosen once.
const SEEN_KEY: &str = "DisplayNameStylesSeenStore";

fn seen(list: &str) -> Vec<u32> {
    crate::storage::get(SEEN_KEY)
        .and_then(|value| value.get(list).cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

/// `DISPLAY_NAME_STYLES_MARK_FONT_SEEN` and `..._MARK_EFFECT_SEEN`: `list` is `seenFontIds` or `seenEffectIds`.
pub(super) fn mark_seen(list: &str, id: u32) {
    let mut saved = crate::storage::get(SEEN_KEY).unwrap_or_else(|| serde_json::json!({}));
    let mut ids = seen(list);
    if ids.contains(&id) {
        return;
    }
    ids.push(id);
    saved[list] = serde_json::json!(ids);
    crate::storage::set(SEEN_KEY, saved);
}

/// The default swatch of the single-colour effects (the first colour after the custom swatch in the capture).
const DEFAULT_COLOR: u32 = 0xf42098;

/// The default colour of a one-colour effect (module 430571); Solid reads `--text-default`.
pub(super) fn default_color(effect: u32) -> u32 {
    match effect {
        1 => 0xefeff1,
        3 => 6888941,
        5 => 1036166,
        _ => DEFAULT_COLOR,
    }
}

/// `ii` of module 750656: the palettes offered for an effect.
pub(super) fn palettes(effect: u32) -> Vec<Vec<u32>> {
    match effect {
        8 => vec![
            vec![0xe9a9d5, 0xa9ddf0, 0xb9a7ee, 0xb9eacb],
            vec![0xff6fc2, 0xf54fa8, 0xff9bd6, 0xd96ab7],
            vec![0xffb57a, 0xff936f, 0xff7c78, 0xf06b8a],
            vec![0xc4ed65, 0x98de60, 7130467, 4570214],
            vec![8173823, 9363664, 0x9d94f5, 6737904],
            vec![0xd77bff, 0xb56bff, 0xe45ce5, 9137407],
            vec![0xd83a5f, 0xc94bcb, 9133311, 0xb06cff],
        ],
        7 => vec![
            vec![0xfe9242, 3534206, 0xffe047, 0xff4cd2, 5793266],
            vec![8316888, 8178687, 8359167, 0xb56dff, 0xf38ad8],
            vec![7997702, 0xc81e12, 0xf45a18, 0xff9500, 0xffd447],
        ],
        2 | 6 => vec![
            vec![2797222, 16762000],
            vec![0xe45eff, 2522592],
            vec![8277963, 6802328],
            vec![0xefb4aa, 0xe46ce2],
            vec![0xf5983c, 0xe55ae7],
        ],
        _ => [2417517, 1874155, 0xc32aff, 0xda0149, 0xf93722, 0xc6a615]
            .into_iter()
            .map(|value| vec![value])
            .collect(),
    }
}

/// `as` of module 501803.
pub(super) fn color_count(effect: u32) -> usize {
    Effect::from_id(effect).color_count()
}

/// A Nitro preview preset (module 461797): theme colours, the name style and the banner.
pub(super) struct Preset {
    pub name: &'static str,
    pub dark: [u32; 2],
    pub font: u32,
    pub effect: u32,
    pub color: u32,
    /// The animated banner (`getBannerSrc(false)`).
    pub banner: &'static str,
}

const CDN: &str = "https://cdn.discordapp.com/assets/content/";

pub(super) const PRESETS: [Preset; 5] = [
    Preset {
        name: "above_the_clouds",
        dark: [5600251, 6553557],
        font: 4,
        effect: 5,
        color: 959694,
        banner: "7f64544be702b2be6c8a06404ba7640492c9f681d5dca4d259203c2e261768fd.gif",
    },
    Preset {
        name: "cyberpunk",
        dark: [0xd846bb, 4395410],
        font: 8,
        effect: 3,
        color: 2949343,
        banner: "94a7319b29322a23ced6f5ed6b7021f57410d258a8da26380805f983ee828922.gif",
    },
    Preset {
        name: "shadow_realm",
        dark: [3880879, 0xad3e3a],
        font: 7,
        effect: 4,
        color: 0xff0000,
        banner: "bb0cce9167be90490426d15e620f76a4808a5336bd90287a694906b727ce72d7.gif",
    },
    Preset {
        name: "starlit_dream",
        dark: [4334982, 0xe4e2d3],
        font: 3,
        effect: 4,
        color: 3343795,
        banner: "b790b270feea90d345aba62f7c3fff9957659a38234ad6287e48654d537df4a6.gif",
    },
    Preset {
        name: "neon_space",
        dark: [0xd620ff, 5826546],
        font: 6,
        effect: 3,
        color: 28737,
        banner: "7e2098e376c104d5f03837013e243a21338970801c723fff9b2d5e2247afb411.gif",
    },
];

/// The choices of the Nitro preview (never saved).
#[derive(Default)]
pub(super) struct Trying {
    pub preset: Option<usize>,
    pub styles: Option<DisplayNameStyles>,
}

impl Trying {
    /// The preset a harness run starts on, or a random one (`B$` of module 461797).
    pub(super) fn start(&mut self) {
        if self.preset.is_none() {
            let index = harness_preset().unwrap_or_else(|| pseudo_random(PRESETS.len()));
            self.set_preset(index);
        }
    }

    pub(super) fn set_preset(&mut self, index: usize) {
        let preset = &PRESETS[index];
        self.preset = Some(index);
        self.styles = Some(DisplayNameStyles {
            font_id: preset.font,
            effect_id: preset.effect,
            colors: vec![preset.color],
        });
    }

    /// Surprise Me! in the panel: another preset.
    pub(super) fn surprise(&mut self) {
        let current = self.preset;
        let mut index = pseudo_random(PRESETS.len());
        if Some(index) == current {
            index = (index + 1) % PRESETS.len();
        }
        self.set_preset(index);
    }

    pub(super) fn theme_colors(&self) -> [u32; 2] {
        PRESETS[self.preset.unwrap_or(PRESETS.len() - 1)].dark
    }
}

fn pseudo_random(bound: usize) -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    nanos % bound
}

/// A harness run starts the Nitro preview on the preset named in `GRAVECORD_PROFILE_PRESET`.
fn harness_preset() -> Option<usize> {
    let name = std::env::var("GRAVECORD_PROFILE_PRESET").ok()?;
    PRESETS.iter().position(|preset| preset.name == name)
}

/// The editor's working choice.
pub(super) struct State {
    pub font: u32,
    pub effect: u32,
    pub colors: Vec<u32>,
    pub picking: Option<colour::State>,
    pub light: bool,
    pub scroll: ScrollHandle,
    /// The style the editor opened with; Apply stays dim until the choice differs from it.
    initial: (u32, u32, Vec<u32>),
}

impl State {
    pub(super) fn new(start: Option<DisplayNameStyles>) -> Self {
        let (font, effect, colors) = match start {
            Some(styles) => (styles.font_id, styles.effect_id, styles.colors),
            None => (11, 1, Vec::new()),
        };
        let mut state = Self {
            font,
            effect,
            colors,
            picking: None,
            light: false,
            scroll: ScrollHandle::new(),
            initial: (0, 0, Vec::new()),
        };
        state.fit_colors();
        state.initial = (state.font, state.effect, state.colors.clone());
        state
    }

    pub(super) fn changed(&self) -> bool {
        self.initial != (self.font, self.effect, self.colors.clone())
    }

    /// Re-fits the colours after the capture script changes the effect.
    #[cfg(feature = "capture")]
    pub(super) fn refit(&mut self) {
        self.fit_colors();
    }

    /// The colours always number what the effect reads.
    fn fit_colors(&mut self) {
        let count = color_count(self.effect);
        if self.colors.len() != count {
            self.colors = if count == 1 {
                vec![default_color(self.effect)]
            } else {
                palettes(self.effect)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| vec![DEFAULT_COLOR])
            };
        }
    }

    pub(super) fn set_color(&mut self, index: usize, rgb: u32) {
        if let Some(slot) = self.colors.get_mut(index) {
            *slot = rgb;
        }
    }

    pub(super) fn styles(&self) -> DisplayNameStyles {
        DisplayNameStyles {
            font_id: self.font,
            effect_id: self.effect,
            colors: self.colors.clone(),
        }
    }
}

/// A tile of the font and effect grids: 88x66 content with a 1px border, so 90x68 on a grid track of 88
/// (`fontButton` and `effectContainer`): the 8% muted background, the subtle border, and the brand border when chosen.
fn tile_frame(id: impl Into<SharedString>, selected: bool) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .relative()
        .w(px(90.))
        .h(px(68.))
        .flex_none()
        .rounded(px(8.))
        .bg(color("--background-mod-muted"))
        .border_1()
        .border_color(color(if selected {
            "--brand-500"
        } else {
            "--border-subtle"
        }))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
}

/// The grid track a tile sits in: 88 wide, with the tile overhanging it by the border.
fn grid_cell(tile: gpui::Stateful<Div>) -> Div {
    div().w(px(88.)).h(px(68.)).flex_none().child(tile)
}

/// `tileNewDot`: 4px, 80px from the tile's inner left edge and 4px below its inner top edge.
fn dot() -> Div {
    div()
        .absolute()
        .top(px(4.))
        .left(px(80.))
        .size(px(4.))
        .rounded_full()
        .bg(color("--brand-500"))
}

fn heading(title: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(text(title, 16., 20., 600., "--text-strong"))
        .child(icon("settings_nitro", 16., "--icon-default"))
}

fn effect_name(effect: u32) -> &'static str {
    match effect {
        1 => dc_intl::tr("Solid"),
        2 => dc_intl::tr("Gradient"),
        3 => dc_intl::tr("Neon"),
        4 => dc_intl::tr("Toon"),
        5 => dc_intl::tr("Pop"),
        8 => dc_intl::tr("Gummy"),
        7 => dc_intl::tr("Prism"),
        _ => "Glow",
    }
}

fn swatch_fill(colors: &[u32]) -> Div {
    let paint = |value: u32| rgba((value << 8) | 0xff);
    match colors {
        [] => div().size_full(),
        [one] => div().size_full().bg(paint(*one)),
        [a, b] => div().size_full().bg(linear_gradient(
            90.,
            linear_color_stop(paint(*a), 0.),
            linear_color_stop(paint(*b), 1.),
        )),
        many => div().size_full().flex().children(
            many.iter()
                .map(|value| div().flex_1().h_full().bg(paint(*value))),
        ),
    }
}

impl Root {
    pub(in crate::ui) fn profile_editor_styles_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.profile_editor.as_ref()?;
        let state = open.styles.as_ref()?;
        let (font, effect, colors, scroll) = (
            state.font,
            state.effect,
            state.colors.clone(),
            state.scroll.clone(),
        );
        let nitro = open.mode == Mode::Nitro;
        let premium = self.profile_editor_premium();
        let styles = state.styles();
        let changed = state.changed();
        let user = self.profile_editor_user();
        let _ = window;
        let (seen_fonts, seen_effects) = (seen("seenFontIds"), seen("seenEffectIds"));

        let mut fonts = div().flex().flex_wrap().gap(px(8.));
        for id in FONTS {
            let preview = DisplayNameStyles {
                font_id: id,
                effect_id: 1,
                colors: Vec::new(),
            };
            let mut tile = tile_frame(format!("styles-font-{id}"), id == font)
                .child(
                    div()
                        .text_size(px(22.))
                        .line_height(px(26.4))
                        .font_weight(FontWeight(600.))
                        .text_color(color(if id == font {
                            "--text-strong"
                        } else {
                            "--text-default"
                        }))
                        .child(name_styles::name(
                            format!("styles-font-{id}"),
                            "Gg",
                            (id != 11).then_some(&preview),
                            name_styles::Display::Font,
                        )),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this
                        .profile_editor
                        .as_mut()
                        .and_then(|open| open.styles.as_mut())
                    {
                        state.font = id;
                    }
                    if PREMIUM_FONTS.contains(&id) {
                        mark_seen("seenFontIds", id);
                    }
                    cx.notify();
                }));
            if PREMIUM_FONTS.contains(&id) && id != font && !seen_fonts.contains(&id) {
                tile = tile.child(dot());
            }
            fonts = fonts.child(grid_cell(tile));
        }

        let mut effects = div().flex().flex_wrap().gap(px(8.));
        for id in EFFECTS {
            let sample = DisplayNameStyles {
                font_id: 11,
                effect_id: id,
                // `previewStyles` of module 898985: the default colours of the effect (module 430571).
                colors: match id {
                    4 => vec![0xf42098],
                    3 => vec![6888941],
                    5 => vec![1036166],
                    _ => palettes(id).into_iter().next().unwrap_or_default(),
                },
            };
            let mut tile = tile_frame(format!("styles-effect-{id}"), id == effect)
                .child(
                    div()
                        // The 16px line sits a pixel lower in the browser than the text system puts it
                        // (measured against the effect tiles of `name-style-effect-*`).
                        .relative()
                        .top(px(1.))
                        .text_size(px(16.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(name_styles::name(
                            format!("styles-effect-{id}"),
                            effect_name(id),
                            (id != 1).then_some(&sample),
                            name_styles::Display::Animated,
                        )),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this
                        .profile_editor
                        .as_mut()
                        .and_then(|open| open.styles.as_mut())
                    {
                        state.effect = id;
                        state.colors.clear();
                        state.fit_colors();
                        state.picking = None;
                    }
                    if PREMIUM_EFFECTS.contains(&id) {
                        mark_seen("seenEffectIds", id);
                    }
                    cx.notify();
                }));
            if PREMIUM_EFFECTS.contains(&id) && id != effect && !seen_effects.contains(&id) {
                tile = tile.child(dot());
            }
            effects = effects.child(grid_cell(tile));
        }

        let colors_row = self.styles_color_row(effect, &colors, cx);
        let left = div()
            .id("styles-scroll")
            .track_scroll(&scroll)
            .overflow_y_scroll()
            .w(px(424.))
            .h(px(581.))
            .flex_none()
            .pl(px(24.))
            .pt(px(24.))
            .child(text(
                dc_intl::tr("Change Display Name Style"),
                20.,
                24.,
                600.,
                "--text-strong",
            ))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(heading(dc_intl::tr("Choose Font")))
                    .child(fonts),
            )
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(heading(dc_intl::tr("Choose Effect")))
                    .child(effects),
            )
            .child(
                div()
                    .mt(px(24.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(heading(dc_intl::tr("Choose Color")))
                    .child(colors_row),
            );

        let preview = self.styles_preview(&user, &styles, nitro, cx);
        let surprise = modal::button(
            "styles-surprise",
            dc_intl::tr("Surprise Me"),
            Tone::Secondary,
            |this, _, cx| {
                this.profile_editor_styles_surprise(cx);
            },
        )
        .with_icon("profile_editor_dice", 20., "--icon-strong");
        let apply = modal::button(
            "styles-apply",
            dc_intl::tr("Apply"),
            Tone::Primary,
            move |this, _, cx| {
                this.profile_editor_styles_apply(cx);
            },
        );
        let _ = premium;
        let modal_w = 836_f32.min((self.viewport.0 - 48.).max(320.));
        let modal_h = 672_f32.min((self.viewport.1 - 64.).max(320.));
        let modal_box = div()
            .parity("profile-editor-styles")
            .id("profile-editor-styles")
            .occlude()
            .on_click(|_, _, cx| cx.stop_propagation())
            .relative()
            .w(px(modal_w))
            .h(px(modal_h))
            .rounded(px(12.))
            .overflow_hidden()
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(left)
                    .child(preview),
            )
            .child(
                div()
                    .h(px(91.))
                    .flex_none()
                    .pr(px(24.))
                    .pt(px(24.))
                    .flex()
                    .flex_row_reverse()
                    .gap(px(8.))
                    .h(px(89.))
                    .child(
                        div()
                            .w(px(139.))
                            .flex()
                            .when(!changed, |this| this.opacity(0.5))
                            .child(modal::footer_button(apply, cx)),
                    )
                    .child(
                        div()
                            .w(px(139.))
                            .flex()
                            .child(modal::footer_button(surprise, cx)),
                    ),
            )
            .child(
                div()
                    .id("styles-close")
                    .absolute()
                    .top(px(13.))
                    .right(px(13.))
                    .size(px(32.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon("modal_close", 16., "--text-strong"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.styles = None;
                        }
                        cx.notify();
                    })),
            );
        let popout = self.profile_editor_colour_popout(60., 590., cx);
        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).
        let pose = crate::ui::motion::legacy_modal_pose(
            "profile-editor-styles",
            "profile-editor-styles-layer",
            false,
        );
        Some(
            deferred(
                div()
                    .id("profile-editor-styles-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(color("--background-scrim")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.styles = None;
                        }
                        cx.notify();
                    }))
                    .child(modal_box.posed(pose))
                    .children(popout),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// The swatches of "Choose Color" (`colorSelection_yIaH9I`): the custom swatch (the current colours, opening
    /// the picker), then the presets of the effect.
    fn styles_color_row(
        &mut self,
        effect: u32,
        colors: &[u32],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sets = palettes(effect);
        let count = color_count(effect);
        let current: Vec<u32> = colors.to_vec();
        let mut row = div().flex().flex_wrap().gap(px(8.));
        let value = default_color(effect);
        let is_custom = !sets.contains(&current) && !(count == 1 && current == [value]);
        // The custom swatch: one small square per colour, each opens the picker for that colour.
        let mut custom = div()
            .id("styles-custom")
            .h(px(40.))
            .rounded(px(8.))
            .overflow_hidden()
            .flex()
            .bg(color("--background-mod-subtle"))
            .when(is_custom, |this| {
                this.border_2().border_color(color("--border-focus"))
            })
            .w(px(match count {
                2 => 135.,
                5 => 231.,
                _ => 40.,
            }));
        let handles = matches!(count, 2 | 5);
        if is_custom || handles {
            custom = custom.child(swatch_fill(&current));
        }
        if handles {
            // The colour handles of the gradient and prism swatches.
            custom = custom.relative();
            for index in 0..count {
                let left = if count == 2 {
                    10. + 89. * index as f32
                } else {
                    10. + 46. * index as f32
                };
                custom = custom.child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(8.))
                        .size(px(24.))
                        .rounded(px(6.))
                        .border_2()
                        .border_color(rgba(0xffffffff)),
                );
            }
        }
        let custom = custom
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.profile_editor_styles_pick_colour(0, cx);
            }));
        row = row.child(custom);
        if count == 1 {
            row = row.child(
                div()
                    .id("styles-default")
                    .size(px(40.))
                    .rounded(px(8.))
                    .when(current == [value], |this| {
                        this.border_2().border_color(color("--border-focus"))
                    })
                    .bg(rgba((value << 8) | 0xff))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profile_editor_styles_set_colors(vec![value], cx);
                    })),
            );
        }
        for (index, set) in sets.into_iter().enumerate() {
            let selected = set == current;
            let paint = set.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("styles-preset-{index}")))
                    .size(px(40.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .when(selected, |this| {
                        this.border_2().border_color(color("--border-focus"))
                    })
                    .cursor_pointer()
                    .child(swatch_fill(&paint))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profile_editor_styles_set_colors(set.clone(), cx);
                    })),
            );
        }
        row.into_any_element()
    }

    /// The right side: the profile, a message and a member row showing the style, and the note.
    fn styles_preview(
        &mut self,
        user: &User,
        styles: &DisplayNameStyles,
        nitro: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let avatar = self.profile_editor_avatar_image(user);
        let banner = nitro.then(|| self.profile_editor_preset_banner()).flatten();
        let name = user.display_name().to_owned();
        let handle = user.username.clone();
        let theme = self
            .profile_editor
            .as_ref()
            .map(|open| open.trying.theme_colors())
            .unwrap_or([0x5c64f4, 0x5c64f4]);
        let light = self
            .profile_editor
            .as_ref()
            .and_then(|open| open.styles.as_ref())
            .is_some_and(|state| state.light);
        // `ThemeContext` of module 974591: the cards, message and member row take the light theme's tokens while the
        // preview's own panel and the toggle keep the app's.
        let app_theme = crate::ui::theme();
        if light {
            crate::ui::set_theme(dc_tokens::Theme::Light);
        }
        let styled = |key: &'static str, size: f32, weight: f32, display: name_styles::Display| {
            div()
                .text_size(px(size))
                .line_height(px(size * 1.2))
                .font_weight(FontWeight(weight))
                .text_color(color("--text-strong"))
                .child(name_styles::name(key, name.clone(), Some(styles), display).light(light))
        };
        let avatar_circle = |size: f32| {
            div()
                .size(px(size))
                .flex_none()
                .rounded_full()
                .overflow_hidden()
                .children(
                    avatar
                        .clone()
                        .map(|image| img(image).size(px(size)).rounded_full()),
                )
        };
        let card =
            div()
                .w(px(304.))
                .h(px(216.))
                .p(px(2.))
                .rounded(px(8.))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba((theme[0] << 8) | 0xff), 0.),
                    linear_color_stop(rgba((theme[1] << 8) | 0xff), 1.),
                ))
                .child(
                    div()
                        .relative()
                        .size_full()
                        .rounded(px(6.))
                        .overflow_hidden()
                        .bg(color("--background-surface-high"))
                        .when(nitro, |this| {
                            // The dark theme body: the gradient under the `#00000099` profile overlay.
                            let shade = |rgb: u32| {
                                let channel = |shift: u32| {
                                    (((rgb >> shift) & 0xff) as f32 * 0.4).round() as u32
                                };
                                (channel(16) << 24) | (channel(8) << 16) | (channel(0) << 8) | 0xff
                            };
                            this.bg(linear_gradient(
                                180.,
                                linear_color_stop(rgba(shade(theme[0])), 0.),
                                linear_color_stop(rgba(shade(theme[1])), 1.),
                            ))
                        })
                        .child(div().h(px(105.)).bg(rgba(0x5c64f4ff)).children(banner.map(
                            |image| {
                                img(image)
                                    .w(px(300.))
                                    .h(px(105.))
                                    .object_fit(ObjectFit::Cover)
                            },
                        )))
                        .child(
                            div()
                                .absolute()
                                .left(px(14.))
                                .top(px(59.))
                                .child(avatar_circle(80.)),
                        )
                        .child(
                            // Do Not Disturb badge on the avatar.
                            div()
                                .absolute()
                                .left(px(68.))
                                .top(px(113.))
                                .size(px(24.))
                                .rounded_full()
                                .bg(color("--background-surface-high"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .size(px(20.))
                                        .rounded_full()
                                        .bg(color("--status-danger"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .w(px(10.))
                                                .h(px(3.))
                                                .rounded(px(1.))
                                                .bg(rgba(0xffffffff)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(16.))
                                .top(px(152.))
                                .flex()
                                .flex_col()
                                .child(styled(
                                    "styles-preview-card",
                                    20.,
                                    600.,
                                    name_styles::Display::Animated,
                                ))
                                .child(text(handle, 14., 18., 400., "--text-strong")),
                        ),
                );
        let message = div()
            .ml(px(25.))
            .mt(px(-10.))
            .w(px(320.))
            .h(px(86.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .pl(px(20.))
            .pt(px(20.))
            .flex()
            .gap(px(14.))
            .child(avatar_circle(40.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(styled(
                                "styles-preview-message",
                                16.,
                                600.,
                                name_styles::Display::Font,
                            ))
                            .child(text("5:08 PM", 12., 20., 500., "--text-subtle")),
                    )
                    .child(text(
                        dc_intl::tr("does anyone read this?"),
                        16.,
                        22.,
                        400.,
                        "--text-default",
                    )),
            );
        let member = div()
            .mt(px(-4.))
            .w(px(282.))
            .h(px(44.))
            .rounded(px(8.))
            .bg(color("--background-surface-highest"))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(avatar_circle(32.))
            .child(styled(
                "styles-preview-member",
                16.,
                500.,
                name_styles::Display::Animated,
            ));
        crate::ui::set_theme(app_theme);
        let toggle = |id: &'static str, glyph: &'static str, is_light: bool| {
            div()
                .id(id)
                .size(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(light == is_light, |this| {
                    this.bg(color("--background-surface-high"))
                })
                .child(gpui::svg().path(glyph).size(px(16.)).text_color(color(
                    if light == is_light {
                        "--text-strong"
                    } else {
                        "--text-muted"
                    },
                )))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this
                        .profile_editor
                        .as_mut()
                        .and_then(|open| open.styles.as_mut())
                    {
                        state.light = is_light;
                    }
                    cx.notify();
                }))
        };
        div()
            .parity("profile-editor-styles-preview")
            .w(px(402.))
            .h_full()
            .flex_none()
            .rounded_tr(px(12.))
            .bg(color("--background-base-lower"))
            .overflow_hidden()
            .pl(px(12.))
            .pt(px(12.))
            .flex()
            .flex_col()
            .child(
                div()
                    .mt(px(32.))
                    .h(px(512.))
                    .flex_none()
                    .pl(px(24.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(card)
                    .child(message)
                    .child(member),
            )
            .child(
                div()
                    .pl(px(16.))
                    .flex()
                    .items_start()
                    .child(
                        div()
                            .w(px(254.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child("Note: Display name colors and effects will not appear in servers. Styles may also look different on other devices."),
                    )
                    .child(
                        div()
                            .ml(px(16.))
                            .p(px(4.))
                            .flex()
                            .gap(px(4.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lowest"))
                            .child(toggle("styles-dark", "icons/profile_editor_moon.svg", false))
                            .child(toggle("styles-light", "icons/profile_editor_sun.svg", true)),
                    ),
            )
            .into_any_element()
    }

    fn profile_editor_styles_set_colors(&mut self, colors: Vec<u32>, cx: &mut Context<Self>) {
        if let Some(state) = self
            .profile_editor
            .as_mut()
            .and_then(|open| open.styles.as_mut())
        {
            state.colors = colors;
            state.picking = None;
        }
        cx.notify();
    }

    fn profile_editor_styles_pick_colour(&mut self, index: usize, cx: &mut Context<Self>) {
        let initial = self
            .profile_editor
            .as_ref()
            .and_then(|open| open.styles.as_ref())
            .and_then(|state| state.colors.get(index).copied())
            .unwrap_or(DEFAULT_COLOR);
        let picking = colour::State::new(Target::Style(index), initial, cx);
        if let Some(state) = self
            .profile_editor
            .as_mut()
            .and_then(|open| open.styles.as_mut())
        {
            state.picking = Some(picking);
        }
        cx.notify();
    }

    /// Surprise Me in the editor (`gN` of module 501803): a random effect, a random font that is not Hexagon and
    /// a random palette of the effect.
    fn profile_editor_styles_surprise(&mut self, cx: &mut Context<Self>) {
        let effect = EFFECTS[pseudo_random(EFFECTS.len())];
        let font = FONTS[pseudo_random(FONTS.len())];
        let sets = palettes(effect);
        let colors = sets[pseudo_random(sets.len())].clone();
        if let Some(state) = self
            .profile_editor
            .as_mut()
            .and_then(|open| open.styles.as_mut())
        {
            state.font = font;
            state.effect = effect;
            state.colors = colors;
            state.picking = None;
        }
        cx.notify();
    }

    /// Apply: in the Nitro preview the style is only tried; otherwise it becomes a pending change (an account
    /// without Nitro is sent to the Nitro page when it saves).
    fn profile_editor_styles_apply(&mut self, cx: &mut Context<Self>) {
        let saved = self
            .stores
            .current_user
            .as_ref()
            .and_then(|user| user.display_name_styles.as_deref().cloned());
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        let Some(state) = open.styles.take() else {
            return;
        };
        let styles = state.styles();
        if open.mode == Mode::Nitro {
            open.trying.styles = Some(styles);
        } else {
            open.pending.display_name_styles =
                (saved.as_ref() != Some(&styles)).then_some(Some(styles));
        }
        cx.notify();
    }

    /// The banner of the Nitro preview's preset (the animated image; the first frame shows).
    pub(super) fn profile_editor_preset_banner(&mut self) -> Option<std::sync::Arc<gpui::Image>> {
        let index = self.profile_editor.as_ref()?.trying.preset?;
        self.shop_img(&format!("{CDN}{}", PRESETS[index].banner))
    }
}

/// Keeps `Font` in use for the table checks below.
#[cfg(test)]
mod tests {
    use super::*;
    use dc_state::display_name_styles::Font;

    #[test]
    fn grid_ids_are_the_editor_tables() {
        assert_eq!(FONTS.len(), 12);
        for id in FONTS {
            assert_eq!(Font::from_id(id).id(), id);
        }
        assert_eq!(
            EFFECTS.map(|id| Effect::from_id(id).color_count()),
            [1, 2, 1, 1, 1, 4, 5]
        );
    }

    #[test]
    fn palettes_match_the_colour_count() {
        for effect in EFFECTS {
            for set in palettes(effect) {
                assert_eq!(set.len(), color_count(effect), "effect {effect}");
            }
        }
    }

    #[test]
    fn a_preset_sets_the_style_and_theme() {
        let mut trying = Trying::default();
        trying.set_preset(4);
        let styles = trying.styles.clone().unwrap();
        assert_eq!(
            (styles.font_id, styles.effect_id, styles.colors),
            (6, 3, vec![28737])
        );
        assert_eq!(trying.theme_colors(), [0xd620ff, 5826546]);
    }
}
