//! The profile styles panel (`panel_BfN7RR`, 242 wide): the header with the profile select, the groups of tiles
//! (`group_Z5MObs`, `tile_HhFoZE`) that open the pickers, and the locked Nitro block with "Preview Nitro". In the
//! Nitro preview the same panel holds the try-it-out groups (module 927071 and the `premiumTryItOut` components).

use super::{Kind, Mode, Open, label, picker, styles, text};
use crate::parity::Parity;
use crate::ui::{Root, avatar_source, color, icon, name_styles};
use dc_state::User;
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, SharedString, Window, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, rgba, svg,
};

/// `tile_HhFoZE`: a 12% grey rounded tile that is a button.
pub(super) fn tile(id: impl Into<SharedString>, width: f32, height: f32) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .relative()
        .w(px(width))
        .h(px(height))
        .flex_none()
        .rounded(px(8.))
        .overflow_hidden()
        .bg(color("--background-mod-subtle"))
        .cursor_pointer()
        .hover(|style| style.bg(color("--background-mod-normal")))
}

/// `overlayIcon_HhFoZE`: the white 24px plus that marks an empty tile.
pub(super) fn plus_badge() -> Div {
    div()
        .size(px(24.))
        .rounded_full()
        .bg(rgba(0xffffffff))
        .flex()
        .items_center()
        .justify_center()
        .child(
            svg()
                .path("icons/plus_small.svg")
                .size(px(16.))
                .text_color(rgba(0x000000ff)),
        )
}

fn group(title: &'static str) -> Div {
    div().flex().flex_col().gap(px(8.)).child(label(title))
}

impl Root {
    pub(super) fn profile_editor_panel(
        &mut self,
        user: &User,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(open) = self.profile_editor.as_ref() else {
            return div().into_any_element();
        };
        let mode = open.mode;
        let body = match mode {
            Mode::Main => self.profile_editor_main_panel(user, cx),
            Mode::Nitro => self.profile_editor_nitro_panel(user, cx),
        };
        div()
            .parity("profile-editor-panel")
            .w(px(242.))
            .h_full()
            .flex_none()
            .bg(color("--background-surface-high"))
            .pt(px(32.))
            .pl(px(1.))
            .flex()
            .flex_col()
            .child(body)
            .into_any_element()
    }

    /// `header_cuaHiV`: the profile select and the collapse button.
    fn profile_editor_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let name = match self.profile_editor.as_ref().and_then(|open| open.guild) {
            None => "Main Profile".to_owned(),
            Some(guild) => self
                .stores
                .guilds
                .iter()
                .find(|known| known.id == guild)
                .map(|known| known.name.clone())
                .unwrap_or_default(),
        };
        div()
            .h(px(49.))
            .flex_none()
            .px(px(8.))
            .pt(px(4.))
            .flex()
            .items_start()
            .child(
                div()
                    .id("profile-editor-select")
                    .mt(px(4.))
                    .h(px(32.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .child(text(name, 16., 20., 500., "--text-strong"))
                    .child(icon("chevron_down", 18., "--text-strong"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.select_open = !open.select_open;
                        }
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(
                div()
                    .size(px(32.))
                    .mt(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("profile_editor_collapse", 16., "--icon-subtle")),
            )
            .into_any_element()
    }

    fn profile_editor_main_panel(&mut self, user: &User, cx: &mut Context<Self>) -> AnyElement {
        let open = self.profile_editor.as_ref().expect("editor is open");
        let avatar_image = self.images.get(&avatar_source(user).0).cloned();
        let decoration_image = user
            .avatar_decoration
            .as_deref()
            .and_then(|decoration| {
                self.images
                    .get(&format!("avatar-decoration:{}", decoration.sku_id))
            })
            .cloned();
        let banner = banner_color(open, user);
        let effect = open
            .pending
            .profile_effect
            .unwrap_or(open.saved.equipped.profile_effect);
        let frame = open
            .pending
            .profile_frame
            .unwrap_or(open.saved.equipped.profile_frame);
        let premium = self.profile_editor_premium();

        let nameplate =
            tile("profile-editor-tile-nameplate", 208., 42.)
                .child(self.nameplate_preview(user, 208., None))
                .child(div().absolute().right(px(8.)).top(px(9.)).child(
                    if user.nameplate.is_some() {
                        div()
                    } else {
                        plus_badge()
                    },
                ))
                .on_click(
                    cx.listener(|this, _, _, cx| this.profile_editor_pick(Kind::Nameplate, cx)),
                );
        let avatar = |decorated: bool| {
            let mut stack = div().relative().size(px(72.)).rounded_full();
            stack = stack.child(
                div()
                    .size(px(72.))
                    .rounded_full()
                    .overflow_hidden()
                    .children(
                        avatar_image
                            .clone()
                            .map(|image| img(image).size(px(72.)).rounded_full()),
                    ),
            );
            if decorated {
                stack = stack.children(decoration_image.clone().map(|image| {
                    div()
                        .absolute()
                        .left(px(-7.2))
                        .top(px(-7.2))
                        .child(img(image).size(px(86.4)))
                }));
            }
            stack
        };
        let avatar_tile = tile("profile-editor-tile-avatar", 100., 100.)
            .flex()
            .items_center()
            .justify_center()
            .child(avatar(false))
            .on_click(cx.listener(|this, _, _, cx| {
                // The avatar upload is not built: it needs the picture dialog and the crop (BLOCKED.md).
                cx.notify();
                let _ = this;
            }));
        let decoration_tile = tile("profile-editor-tile-decoration", 100., 100.)
            .flex()
            .items_center()
            .justify_center()
            .child(if user.avatar_decoration.is_some() {
                avatar(true)
            } else {
                div()
                    .size(px(72.))
                    .rounded_full()
                    .border_2()
                    .border_dashed()
                    .border_color(color("--border-strong"))
                    .bg(color("--background-mod-subtle"))
            })
            .children(user.avatar_decoration.is_none().then(|| {
                div()
                    .absolute()
                    .top(px(38.))
                    .left(px(38.))
                    .child(plus_badge())
            }))
            .on_click(cx.listener(|this, _, _, cx| this.profile_editor_pick(Kind::Decoration, cx)));
        let banner_tile = tile("profile-editor-tile-banner", 208., 42.)
            .bg(rgba((banner << 8) | 0xff))
            .on_click(cx.listener(|this, _, _, cx| {
                this.profile_editor_open_banner_color(cx);
            }));
        let effect_tile = tile("profile-editor-tile-effect", 100., 100.)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(60.))
                    .h(px(88.))
                    .rounded(px(4.))
                    .bg(color("--background-base-low"))
                    .overflow_hidden()
                    .children(self.profile_editor_art(effect, Kind::Effect, 60., 88.)),
            )
            .children(effect.is_none().then(|| {
                div()
                    .absolute()
                    .top(px(38.))
                    .left(px(38.))
                    .child(plus_badge())
            }))
            .on_click(cx.listener(|this, _, _, cx| this.profile_editor_pick(Kind::Effect, cx)));
        let frame_tile = tile("profile-editor-tile-frame", 100., 100.)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(60.))
                    .rounded(px(4.))
                    .bg(color("--background-base-low"))
                    .overflow_hidden()
                    .children(self.profile_editor_art(frame, Kind::Frame, 60., 60.)),
            )
            .children(frame.is_none().then(|| {
                div()
                    .absolute()
                    .top(px(38.))
                    .left(px(38.))
                    .child(plus_badge())
            }))
            .on_click(cx.listener(|this, _, _, cx| this.profile_editor_pick(Kind::Frame, cx)));

        let groups = div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(group("Nameplate").child(nameplate))
            .child(
                group("Avatar & Decoration").child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(avatar_tile)
                        .child(decoration_tile),
                ),
            )
            .child(group("Banner Color").child(banner_tile))
            .child(
                group("Profile Effect & Frame").child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(effect_tile)
                        .child(frame_tile),
                ),
            );

        let scroller = div()
            .id("profile-editor-panel-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p(px(16.))
            .child(groups);
        let nitro_block = (!premium).then(|| self.profile_editor_nitro_block(cx));
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(self.profile_editor_header(cx))
            .child(scroller)
            .children(nitro_block)
            .into_any_element()
    }

    /// `premiumTryItOutSection_cuaHiV`: the lock, the pitch and the Preview Nitro button.
    fn profile_editor_nitro_block(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .relative()
            .flex_none()
            .px(px(16.))
            .pt(px(22.))
            .pb(px(20.))
            .bg(linear_gradient(
                90.,
                linear_color_stop(rgba(0x402448ff), 0.),
                linear_color_stop(rgba(0x282151ff), 1.),
            ))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .absolute()
                    .top(px(-14.))
                    .left(px(106.))
                    .size(px(28.))
                    .rounded_full()
                    .bg(color("--background-surface-highest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("channel_lock", 16., "--icon-subtle")),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child("Level up your look, only with ")
                    .child(
                        div()
                            .id("profile-editor-nitro-link")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .child("Nitro")
                            .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx))),
                    ),
            )
            .child(
                div()
                    .id("profile-editor-preview-nitro")
                    .h(px(32.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .child(icon(
                        "settings_nitro",
                        16.,
                        "--control-primary-text-default",
                    ))
                    .child(text(
                        "Preview Nitro",
                        14.,
                        18.,
                        500.,
                        "--control-primary-text-default",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.mode = Mode::Nitro;
                            open.tab = super::Tab::Activity;
                            open.trying.start();
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// The Nitro preview panel: back arrow, the pitch, Surprise Me! and the four try-it-out groups.
    fn profile_editor_nitro_panel(&mut self, user: &User, cx: &mut Context<Self>) -> AnyElement {
        let open = self.profile_editor.as_ref().expect("editor is open");
        let trying = &open.trying;
        let shown = trying.styles.clone();
        let theme = trying.theme_colors();
        let header = div()
            .h(px(49.))
            .flex_none()
            .px(px(16.))
            .pt(px(4.))
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .id("profile-editor-nitro-back")
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon("shop_arrow_left", 20., "--icon-default"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.mode = Mode::Main;
                            open.tab = super::Tab::Board;
                        }
                        cx.notify();
                    })),
            )
            .child(text("Preview Nitro", 16., 20., 600., "--text-strong"));
        let name_preview = tile("profile-editor-try-name", 208., 42.)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(name_styles::name(
                        "profile-editor-try-name",
                        user.display_name().to_owned(),
                        shown.as_ref(),
                        name_styles::Display::Animated,
                    )),
            )
            .on_click(cx.listener(|this, _, _, cx| this.profile_editor_open_styles(cx)));
        let swatch = |value: u32, top: bool| {
            div()
                .absolute()
                .left(px(88.))
                .top(px(if top { 0. } else { 68. }))
                .size(px(32.))
                .rounded(px(8.))
                .border_2()
                .border_color(rgba(0xffffffff))
                .child(
                    div()
                        .size_full()
                        .rounded(px(6.))
                        .bg(rgba((value << 8) | 0xff)),
                )
        };
        let theme_tile = div()
            .id("profile-editor-try-theme")
            .relative()
            .w(px(208.))
            .h(px(100.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgba((theme[0] << 8) | 0xff), 0.),
                linear_color_stop(rgba((theme[1] << 8) | 0xff), 1.),
            ))
            .child(swatch(theme[0], true))
            .child(swatch(theme[1], false));
        let banner_tile = tile("profile-editor-try-banner", 208., 100.)
            .children(self.profile_editor_preset_banner().map(|image| {
                img(image)
                    .w(px(208.))
                    .h(px(100.))
                    .object_fit(gpui::ObjectFit::Cover)
            }))
            .child(div().absolute().inset_0().bg(rgba(0x00000066)));
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        let animated = tile("profile-editor-try-avatar", 208., 100.)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(72.))
                    .rounded_full()
                    .overflow_hidden()
                    .children(avatar.map(|image| img(image).size(px(72.)).rounded_full())),
            );
        let surprise = div()
            .id("profile-editor-surprise")
            .h(px(32.))
            .rounded(px(8.))
            .bg(color("--control-secondary-background-default"))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .cursor_pointer()
            .hover(|style| style.bg(color("--control-secondary-background-hover")))
            .child(
                gpui::svg()
                    .path("icons/profile_editor_dice.svg")
                    .size(px(16.))
                    .text_color(color("--control-secondary-text-default")),
            )
            .child(text(
                "Surprise Me!",
                14.,
                18.,
                500.,
                "--control-secondary-text-default",
            ))
            .on_click(cx.listener(|this, _, _, cx| this.profile_editor_surprise(cx)));
        let body = div()
            .id("profile-editor-nitro-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p(px(16.))
            .pt(px(8.))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(
                                "You're previewing what your profile could look like with Nitro",
                            ),
                    )
                    .child(surprise),
            )
            .child(group("Display Name Style").child(name_preview))
            .child(group("Profile Theme").child(theme_tile))
            .child(group("Profile Banner").child(banner_tile))
            .child(group("Animated Avatar").child(animated));
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .into_any_element()
    }

    pub(super) fn profile_editor_pick(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let current = self.profile_editor.as_ref().map(|open| match kind {
            Kind::Nameplate => open.pending.nameplate.unwrap_or(
                self.stores
                    .current_user
                    .as_ref()
                    .and_then(|user| user.nameplate.as_ref().map(|plate| plate.sku_id)),
            ),
            Kind::Decoration => open.pending.avatar_decoration.unwrap_or(
                self.stores
                    .current_user
                    .as_ref()
                    .and_then(|user| user.avatar_decoration.as_deref().map(|d| d.sku_id)),
            ),
            Kind::Effect => open
                .pending
                .profile_effect
                .unwrap_or(open.saved.equipped.profile_effect),
            Kind::Frame => open
                .pending
                .profile_frame
                .unwrap_or(open.saved.equipped.profile_frame),
        });
        if let Some(open) = self.profile_editor.as_mut() {
            open.picker = Some(picker::State::new(kind, current.flatten()));
        }
        cx.notify();
    }

    pub(super) fn profile_editor_open_banner_color(&mut self, cx: &mut Context<Self>) {
        let initial = self
            .profile_editor
            .as_ref()
            .zip(self.stores.current_user.as_ref())
            .map(|(open, user)| banner_color(open, user))
            .unwrap_or(0x5c64f4);
        if let Some(open) = self.profile_editor.as_mut() {
            open.banner_color = Some(super::colour::State::new(
                super::colour::Target::Banner,
                initial,
                cx,
            ));
        }
        cx.notify();
    }

    pub(super) fn profile_editor_open_styles(&mut self, cx: &mut Context<Self>) {
        let user = self.stores.current_user.clone().unwrap_or_default();
        if let Some(open) = self.profile_editor.as_mut() {
            let start = open
                .trying
                .styles
                .clone()
                .or_else(|| user.display_name_styles.as_deref().cloned());
            open.styles = Some(styles::State::new(start));
        }
        cx.notify();
    }

    fn profile_editor_surprise(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.profile_editor.as_mut() {
            open.trying.surprise();
        }
        cx.notify();
    }
}

/// The banner colour the profile shows: the pending choice, the saved accent colour, or the colour of the default
/// avatar (module 927071 suggests a colour from the avatar when none is set).
pub(super) fn banner_color(open: &Open, user: &User) -> u32 {
    open.pending
        .accent_color
        .flatten()
        .or(open.saved.accent_color)
        .unwrap_or_else(|| default_banner_color(user))
}

fn default_banner_color(_user: &User) -> u32 {
    0x5c64f4
}
