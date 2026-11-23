//! The middle of the editor: the live profile card (`profile_s3DB6Y`, up to 400 wide) and the stage around it
//! (`outer_P1YEjQ`), with the save bar. The card is the profile with its name, pronouns and bio editable in place
//! (module 927071, `profileBody_s3DB6Y`).

use super::{Mode, Tab, text};
use crate::parity::Parity;
use crate::ui::{Root, color, dm_profile, icon, name_styles, profile_theme};
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, Rgba, SharedString, Window, div, img,
    prelude::*, px, rgba, svg,
};

fn tinted_icon(name: &'static str, size: f32, tint: Rgba) -> gpui::Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .text_color(tint)
}

fn tinted_text(
    value: impl Into<SharedString>,
    size: f32,
    line_height: f32,
    weight: f32,
    tint: Rgba,
) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(line_height))
        .font_weight(FontWeight(weight))
        .text_color(tint)
        .child(value.into())
}

fn section(title: &'static str, palette: &dm_profile::Palette) -> Div {
    div().flex().flex_col().gap(px(8.)).child(tinted_text(
        title,
        12.,
        16.,
        500.,
        palette.token("--text-subtle"),
    ))
}

fn round_button(
    id: &'static str,
    glyph: &'static str,
    palette: &dm_profile::Palette,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .size(px(32.))
        .rounded(px(8.))
        .bg(palette.token("--control-secondary-background-default"))
        .border_1()
        .border_color(palette.token("--control-secondary-border-default"))
        .flex()
        .items_center()
        .justify_center()
        .child(tinted_icon(
            glyph,
            16.,
            palette.token("--control-secondary-text-default"),
        ))
}

/// A dot of the thought bubble leading from the avatar to the status bubble.
fn thought_dot(left: f32, top: f32, size: f32, palette: &dm_profile::Palette) -> Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .size(px(size))
        .rounded_full()
        .bg(palette.token("--background-surface-highest"))
}

/// Composite `overlay` over `base` when the gradient is painted without stacking translucent layers.
fn blend(base: Rgba, overlay: Rgba) -> Rgba {
    profile_theme::blend_over(base, overlay)
}

impl Root {
    pub(super) fn profile_editor_palette(&self) -> dm_profile::Palette {
        match self.profile_editor_theme() {
            Some([primary, secondary]) => {
                let viewer = super::super::theme();
                let pt = super::super::profile_theme::ProfileTheme::new(
                    (primary, secondary),
                    None,
                    viewer,
                    self.pref_on("sync_profile_themes", false),
                );
                dm_profile::Palette {
                    top: pt.primary,
                    bottom: pt.secondary,
                    overlay: pt.overlay.into(),
                    text: pt.token("--text-default").into(),
                    subtle: pt.token("--interactive-text-default").into(),
                    themed: true,
                    theme: Some(pt),
                }
            }
            None => dm_profile::Palette {
                top: color("--chat-background-default"),
                bottom: color("--chat-background-default"),
                overlay: gpui::hsla(0., 0., 0., 0.),
                text: color("--text-default").into(),
                subtle: color("--interactive-text-default").into(),
                themed: false,
                theme: None,
            },
        }
    }

    pub(super) fn profile_editor_stage(
        &mut self,
        user: &User,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = self.profile_editor_palette();
        self.profile_editor_sync_field_styles(&palette, cx);
        let geometry = self
            .profile_editor
            .as_ref()
            .expect("editor is open")
            .geometry;
        let card = self.profile_editor_card(user, window, cx);
        // `profileContentColumns_s3DB6Y`: the card and the widgets column side by side, down to the editor's bottom.
        let columns = div()
            .relative()
            .flex()
            .w(px(geometry.columns_width()))
            .h(px(geometry.card_height()))
            .child(card)
            .child(self.profile_editor_widgets(&palette, window, cx));
        // `inner_P1YEjQ profileContentInner_s3DB6Y`: the column's padding, the title bar's height at its top. A themed
        // profile (`custom-user-profile-theme`) paints `modal_background` with the 300px overlay fade (`253781`).
        let full_stage = geometry.panel_collapsed || geometry.panel_over;
        let stage_tl = if full_stage { geometry.radius } else { 0. };
        let stage_bl = stage_tl;
        let stage_tr = geometry.radius;
        let stage_br = geometry.radius;
        let inner_tl = (stage_tl - 1.).max(0.);
        let inner_tr = (stage_tr - 1.).max(0.);
        let inner_br = (stage_br - 1.).max(0.);
        let inner_bl = (stage_bl - 1.).max(0.);
        let mut stage = div()
            .parity("profile-editor-stage")
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .rounded_tl(px(stage_tl))
            .rounded_tr(px(stage_tr))
            .rounded_br(px(stage_br))
            .rounded_bl(px(stage_bl))
            .overflow_hidden()
            .bg(palette
                .theme
                .as_ref()
                .map_or_else(|| color("--background-base-lower"), |t| t.modal_background))
            .pl(px(geometry.padding))
            .pt(px(geometry.content_top()))
            .when(geometry.content_border > 0., |this| {
                this.border_r_1().when(full_stage, |d| d.border_l_1())
            })
            .when(geometry.tall, |this| this.border_t_1().border_b_1())
            .border_color(color("--border-normal"))
            .children(palette.modal_backdrop_rounded(inner_tl, inner_tr, inner_br, inner_bl))
            .child(columns);
        if let Some(bar) = self.profile_editor_bar(cx) {
            stage = stage.child(bar);
        }
        stage.into_any_element()
    }

    /// Keep the inline name, pronouns, and bio editors readable against light and dark custom themes.
    fn profile_editor_sync_field_styles(
        &self,
        palette: &dm_profile::Palette,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.profile_editor.as_ref() else {
            return;
        };
        let strong = palette.token("--text-strong");
        let default = palette.token("--text-default");
        let placeholder = palette.token("--input-placeholder-text-default");
        let (name, pronouns, bio) = (open.name.clone(), open.pronouns.clone(), open.bio.clone());
        name.update(cx, |editor, _| {
            editor.set_style(super::profile_field_style(
                24.,
                32.,
                600.,
                strong.into(),
                placeholder.into(),
            ));
        });
        pronouns.update(cx, |editor, _| {
            editor.set_style(super::profile_field_style(
                14.,
                18.,
                400.,
                default.into(),
                placeholder.into(),
            ));
        });
        bio.update(cx, |editor, _| {
            editor.set_style(super::profile_field_style(
                14.,
                18.,
                400.,
                default.into(),
                placeholder.into(),
            ));
        });
    }

    /// The theme colours the preview paints (`useProfileThemeValues`, module 927071 `u3`): the try-it-out choice in
    /// the Nitro preview, else the pending or saved colours of a profile that may use them.
    pub(super) fn profile_editor_theme(&self) -> Option<[u32; 2]> {
        let open = self.profile_editor.as_ref()?;
        if open.mode == Mode::Nitro {
            return Some(open.trying.theme_colors());
        }
        if !self.profile_editor_premium() {
            return None;
        }
        match open.pending.theme_colors {
            Some(pending) => pending,
            None => open.saved.theme_colors,
        }
    }

    fn profile_editor_card(
        &mut self,
        user: &User,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let nitro = self
            .profile_editor
            .as_ref()
            .is_some_and(|open| open.mode == Mode::Nitro);
        let banner_image = nitro
            .then(|| self.profile_editor_preset_banner())
            .flatten()
            .or_else(|| {
                self.profile_editor
                    .as_ref()
                    .and_then(|open| open.banner_image.clone())
            })
            .or_else(|| {
                (!nitro)
                    .then(|| self.profile_editor_saved_banner())
                    .flatten()
            });
        let palette = self.profile_editor_palette();
        let upsell = self.profile_editor_upsell_shown();
        let badge_pill = if nitro {
            None
        } else {
            self.badge_pill("profile-editor", user.id, &palette, cx)
        };
        let server_tag = if nitro {
            None
        } else {
            self.profile_editor_tag_pill(user, &palette, cx)
        };
        let open = self.profile_editor.as_ref().expect("editor is open");
        let geometry = open.geometry;
        let premium = self.profile_editor_premium();
        let guild_upsell = super::servers::guild_upsell_opens(open.guild, premium);
        let banner = super::panel::banner_color(open, user);
        let avatar_image = self.profile_editor_avatar_image(user);
        let decoration = user
            .avatar_decoration
            .as_deref()
            .and_then(|decoration| {
                self.images
                    .get(&format!("avatar-decoration:{}", decoration.sku_id))
            })
            .cloned();
        let effect = open
            .pending
            .profile_effect
            .unwrap_or(open.saved.equipped.profile_effect);
        let frame = open
            .pending
            .profile_frame
            .unwrap_or(open.saved.equipped.profile_frame);
        let styled_name = user.display_name_styles.is_some();
        let prompt = open.prompt;
        let (name, pronouns, bio) = (open.name.clone(), open.pronouns.clone(), open.bio.clone());
        let username = user.username.clone();
        let since = dm_profile::member_since(user.id);
        let width = geometry.profile_width;
        let pad = geometry.card_padding();
        let avatar_size = geometry.avatar_size();
        let ring = avatar_size + 18.;
        let avatar_left = geometry.avatar_left() - 9.;
        // The status bubble starts 172px in on the full card (11px past the avatar's ring) and moves with it.
        let bubble_left = avatar_left + ring + 11.;
        // The ring and the status cut-out take the card's colour, the theme's composited top colour on a themed profile.
        let card_fill = |this: Div| match palette.theme.as_ref() {
            Some(theme) => this.bg(blend(theme.primary, theme.overlay)),
            None => this.bg(color("--background-surface-high")),
        };

        let banner_bg = match palette.theme.as_ref() {
            Some(theme) => blend(theme.primary, theme.overlay),
            None => rgba((banner << 8) | 0xff),
        };
        // `Y` of module 641318: interactive banner hover overlay (`N0bC3P` "Change Banner" or
        // `O1sT5v` "Unlock Banner" with pencil / Nitro wheel icon).
        let can_edit_banner = !guild_upsell;
        let banner_label = if can_edit_banner {
            dc_intl::t("N0bC3P")
        } else {
            dc_intl::t("O1sT5v")
        };
        let banner_icon = if can_edit_banner {
            "clips_pencil"
        } else {
            "nitro_wheel"
        };
        let banner_fill = div()
            .id("profile-editor-preview-banner")
            .group("profile-editor-preview-banner-group")
            .relative()
            .w_full()
            .h(px(140.))
            .rounded_t(px(16.))
            .overflow_hidden()
            .bg(banner_bg)
            .cursor_pointer()
            .children(banner_image.map(|image| {
                img(image)
                    .w_full()
                    .h(px(140.))
                    .rounded_t(px(16.))
                    .object_fit(gpui::ObjectFit::Cover)
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_t(px(16.))
                    .bg(color("--opacity-black-40"))
                    .opacity(0.)
                    .group_hover("profile-editor-preview-banner-group", |s| s.opacity(1.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(
                        gpui::svg()
                            .path(format!("icons/{banner_icon}.svg"))
                            .size(px(16.))
                            .text_color(rgba(0xffffffff)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(rgba(0xffffffff))
                            .child(banner_label),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if can_edit_banner {
                    this.profile_editor_pick_image(super::upload::Target::Banner, cx);
                } else {
                    this.profile_editor_open_guild_upsell(cx);
                }
            }));
        let dot = avatar_size * 32. / 120.;
        let avatar = card_fill(
            div()
                .absolute()
                .left(px(avatar_left))
                .top(px(71. + 120. - avatar_size))
                .size(px(ring))
                .rounded_full(),
        )
        .p(px(9.))
        .child(
            div()
                .relative()
                .size(px(avatar_size))
                .rounded_full()
                .child(
                    div()
                        .size(px(avatar_size))
                        .rounded_full()
                        .overflow_hidden()
                        .children(
                            avatar_image
                                .map(|image| img(image).size(px(avatar_size)).rounded_full()),
                        ),
                )
                .child(
                    card_fill(
                        div()
                            .absolute()
                            .left(px(avatar_size - dot))
                            .top(px(avatar_size - dot))
                            .size(px(dot))
                            .rounded_full(),
                    )
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(dot * 0.75))
                            .rounded_full()
                            .bg(color("--status-danger"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .w(px(dot * 0.375))
                                    .h(px(4.))
                                    .rounded(px(2.))
                                    .bg(rgba(0xffffffff)),
                            ),
                    ),
                )
                .children(decoration.map(|image| {
                    let side = avatar_size * 1.2;
                    div()
                        .absolute()
                        .left(px(-(side - avatar_size) / 2.))
                        .top(px(-(side - avatar_size) / 2.))
                        .child(img(image).size(px(side)))
                })),
        );
        // `E` of module 641318 + module 531835 (`overlay_zNeEEq`, `overlayIcon_zNeEEq`): on a server profile without
        // Nitro the avatar is a button for `onUpsellClick` (module 63473) and reveals a circular `--opacity-black-40`
        // veil with the 20px white NitroWheel on hover.
        let avatar = if guild_upsell {
            avatar
                .id("profile-editor-avatar-upsell")
                .group("profile-editor-avatar-upsell")
                .cursor_pointer()
                .child(
                    div()
                        .absolute()
                        .left(px(9.))
                        .top(px(9.))
                        .size(px(avatar_size))
                        .rounded_full()
                        .bg(color("--opacity-black-40"))
                        .opacity(0.)
                        .group_hover("profile-editor-avatar-upsell", |s| s.opacity(1.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui::svg()
                                .path("icons/nitro_wheel.svg")
                                .size(px(20.))
                                .text_color(rgba(0xffffffff)),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.profile_editor_open_guild_upsell(cx);
                }))
                .into_any_element()
        } else {
            let menu_open = open.avatar_menu_open;
            avatar
                .id("profile-editor-avatar-actions-trigger")
                .group("profile-editor-avatar-actions-group")
                .cursor_pointer()
                .child(
                    div()
                        .absolute()
                        .left(px(9.))
                        .top(px(9.))
                        .size(px(avatar_size))
                        .rounded_full()
                        .bg(color("--opacity-black-40"))
                        .opacity(0.)
                        .group_hover("profile-editor-avatar-actions-group", |s| s.opacity(1.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui::svg()
                                .path("icons/clips_pencil.svg")
                                .size(px(20.))
                                .text_color(rgba(0xffffffff)),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.avatar_menu_open = !open.avatar_menu_open;
                    }
                    cx.notify();
                }))
                .when(menu_open, |el| {
                    // `B` of module 641318: Avatar Actions popout menu (`ldIeAd`, `4OynCD`, `BVcYCx`).
                    el.child(
                        div()
                            .id("profile-editor-avatar-actions-menu")
                            .aria_label(dc_intl::t("ldIeAd"))
                            .occlude()
                            .absolute()
                            .left(px(0.))
                            .top(px(ring + 6.))
                            .w(px(188.))
                            .p(px(6.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.28),
                                offset: gpui::point(px(0.), px(8.)),
                                blur_radius: px(16.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .id("profile-editor-menu-change-avatar")
                                    .px(px(10.))
                                    .py(px(8.))
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .text_size(px(14.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("4OynCD"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(open) = this.profile_editor.as_mut() {
                                            open.avatar_menu_open = false;
                                        }
                                        this.profile_editor_pick_image(
                                            super::upload::Target::Avatar,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                div()
                                    .id("profile-editor-menu-change-decoration")
                                    .px(px(10.))
                                    .py(px(8.))
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .text_size(px(14.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("BVcYCx"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(open) = this.profile_editor.as_mut() {
                                            open.avatar_menu_open = false;
                                        }
                                        this.profile_editor_pick(super::Kind::Decoration, cx);
                                    })),
                            ),
                    )
                })
                .into_any_element()
        };
        // The status bubble (`e0.A` with the prompt of module 645507): grows automatically when text wraps inside `bubble_width`.
        let bubble_width = (width - bubble_left - 20.).clamp(120., 180.);
        let bubble = div()
            .absolute()
            .left(px(bubble_left))
            .top(px(132.))
            .w(px(bubble_width))
            .min_h(px(36.))
            .py(px(9.))
            .bg(palette.token("--background-surface-highest"))
            .px(px(12.))
            .rounded(px(16.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                gpui::svg()
                    .path("icons/status_add.svg")
                    .size(px(16.))
                    .text_color(palette.token("--icon-subtle")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .italic()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(palette.token("--text-muted"))
                    .child(dc_intl::tr(prompt)),
            );

        let name_row = if styled_name {
            div()
                .text_size(px(24.))
                .line_height(px(32.))
                .font_weight(FontWeight(600.))
                .text_color(palette.token("--text-strong"))
                .child(name_styles::name(
                    "profile-editor-name",
                    user.display_name().to_owned(),
                    user.display_name_styles.as_deref(),
                    name_styles::Display::Animated,
                ))
        } else {
            div().h(px(32.)).child(name)
        };
        // `usernameAndPronounsRow_C2XpHA` (9.33px apart, wrapping 4px apart): the handle and the pronouns, the server
        // tag select (`uP`) and the badges (`eW.A`).
        let handle_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(9.33))
            .gap_y(px(4.))
            .when(!nitro, |this| this.min_h(px(24.)).mt(px(3.)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(tinted_text(
                        username,
                        14.,
                        18.,
                        400.,
                        palette.token("--text-strong"),
                    ))
                    .child(
                        div()
                            .size(px(4.))
                            .rounded_full()
                            .bg(palette.token("--text-default")),
                    )
                    .child(div().min_w(px(80.)).child(pronouns)),
            )
            .children(server_tag)
            .children(badge_pill);
        let buttons = div()
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(palette.token("--control-primary-background-default"))
                    .opacity(0.5)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(tinted_icon(
                        "chat_bubble",
                        16.,
                        palette.token("--control-primary-text-default"),
                    ))
                    .child(tinted_text(
                        dc_intl::tr("Message"),
                        14.,
                        18.,
                        500.,
                        palette.token("--control-primary-text-default"),
                    )),
            )
            .child(round_button("profile-editor-shop-button", "shop", &palette))
            .child(round_button(
                "profile-editor-more-button",
                "more_horizontal",
                &palette,
            ));
        let sec_bg = palette.token("--control-secondary-background-default");
        let sec_hover = palette.token("--control-secondary-background-hover");
        let sec_border = palette.token("--control-secondary-border-default");
        let sec_fg = palette.token("--control-secondary-text-default");
        let upsell_card = upsell.then(|| {
            div()
                .w(px(width - 2. * pad))
                .h(px(86.))
                .rounded(px(8.))
                .border_2()
                .border_color(rgba(0x8a6ff0ff))
                .bg(palette.token("--background-surface-highest"))
                .p(px(10.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .child(div().flex_1().child(tinted_text(
                            "Amp up your profile",
                            14.,
                            18.,
                            400.,
                            palette.token("--text-strong"),
                        )))
                        .child(
                            div()
                                .id("profile-editor-upsell-close")
                                .cursor_pointer()
                                .child(tinted_icon(
                                    "upsell_close",
                                    16.,
                                    palette.token("--icon-subtle"),
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.profile_editor_dismiss_upsell();
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("profile-editor-get-nitro")
                                .h(px(32.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .bg(sec_bg)
                                .hover(move |style| style.bg(sec_hover))
                                .border_1()
                                .border_color(sec_border)
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .child(tinted_icon(
                                    if premium {
                                        "nitro_wheel"
                                    } else {
                                        "settings_nitro"
                                    },
                                    16.,
                                    sec_fg,
                                ))
                                .child(tinted_text(
                                    if premium { "View Perks" } else { "Get Nitro" },
                                    14.,
                                    18.,
                                    500.,
                                    sec_fg,
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if premium {
                                        // `openApplicationStore` (module 560296 `$`).
                                        this.profile_editor = None;
                                        this.close_settings(cx);
                                        this.open_shop(cx);
                                    } else {
                                        this.open_purchase(false, cx);
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id("profile-editor-upsell-shop")
                                .h(px(32.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .bg(sec_bg)
                                .hover(move |style| style.bg(sec_hover))
                                .border_1()
                                .border_color(sec_border)
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .child(tinted_icon("shop", 16., sec_fg))
                                .child(tinted_text("Shop", 14., 18., 500., sec_fg))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.profile_editor = None;
                                    this.close_settings(cx);
                                    this.open_shop(cx);
                                })),
                        ),
                )
        });
        let bio_section = if !nitro {
            Some(
                section("Bio", &palette)
                    .child(div().ml(px(-7.)).px(px(7.)).child(bio))
                    .into_any_element(),
            )
        } else {
            // Module 442228 in 641318 Nitro preview: clamped bio with View Full Bio disabled.
            let preview_bio = bio.read(cx).text().to_owned();
            let me_id = self.stores.current_user.as_ref().map_or(0, |u| u.id);
            let strong = palette.token("--text-strong").into();
            self.clamped_bio_with_opts(
                me_id,
                &preview_bio,
                strong,
                &palette,
                crate::ui::profile_other::ClampedBioOpts {
                    hide_personal_info: self.streamer_hides_personal(),
                    view_full_bio_disabled: true,
                    close_popout: false,
                },
                cx,
            )
            .map(|el| section("Bio", &palette).child(el).into_any_element())
        };
        let connections = (!nitro).then(|| {
            let muted = palette.token("--text-muted");
            // Module 927071: the button opens the Add Connection grid (`tQ`).
            section("Connections", &palette).child(
                div()
                    .id("profile-editor-add-connection")
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(tinted_icon("plus_small", 18., muted))
                    .child(tinted_text("Add Connection", 12., 16., 500., muted))
                    .on_click(cx.listener(|this, _, _, cx| this.open_add_connection(cx))),
            )
        });
        // `profileBody_s3DB6Y`: the card's padding, scrolling under the header when it does not fit.
        let body = div()
            .id("profile-editor-card-body")
            .relative()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(24.))
            .pl(px(pad))
            .pr(px(pad))
            .pb(px(pad))
            .pt(px(if nitro { 4. } else { 9. }))
            .mt(px(-4.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(name_row)
                    .child(handle_row),
            )
            .child(div().mt(px(if nitro { 2. } else { 4. })).child(buttons))
            .children(upsell_card)
            .children(bio_section)
            .child(section("Member Since", &palette).child(tinted_text(
                since,
                14.,
                18.,
                400.,
                palette.token("--text-default"),
            )))
            .children(connections)
            .child(
                section("Note (only visible to you)", &palette).child(
                    tinted_text(
                        "Click to add a note",
                        14.,
                        18.,
                        400.,
                        palette.token("--text-muted"),
                    )
                    .italic(),
                ),
            );

        // `profile_s3DB6Y`: rounded at the top only, from the column's padding to the editor's bottom edge,
        // with `box-shadow: var(--shadow-medium)` and `outline: 1px solid var(--border-muted)`.
        let mut card = div()
            .parity("profile-editor-card")
            .id("profile-editor-card")
            .relative()
            .w(px(width))
            .h(px(geometry.card_height()))
            .flex_none()
            .rounded_t(px(16.))
            .overflow_hidden()
            .shadow(vec![super::super::shadow_medium()])
            .bg(palette.theme.as_ref().map_or_else(
                || color("--background-surface-high"),
                |t| t.card_gradient_stops().0,
            ))
            .border_1()
            .border_color(palette.token("--border-muted"))
            .flex()
            .flex_col()
            .children(palette.card_backdrop(140.))
            .child(
                div()
                    .relative()
                    .h(px(220.))
                    .flex_none()
                    .child(banner_fill)
                    .child(avatar)
                    .child(thought_dot(bubble_left, 121., 8., &palette))
                    .child(thought_dot(bubble_left + 16., 131., 14., &palette))
                    .child(bubble),
            )
            .child(body);
        let frame_layer =
            self.profile_editor_art(frame, super::Kind::Frame, width, geometry.card_height());
        let effect_layer = effect.and_then(|sku| {
            self.profile_effect_layer(
                "profile-editor",
                sku,
                super::super::profile_effects::Surface::profile(width, false),
                window,
                cx,
            )
        });
        card = card.children(frame_layer).children(effect_layer);
        card.into_any_element()
    }

    pub(super) fn profile_editor_eligible_tags(&self) -> Vec<(Id, String, String, Option<String>)> {
        self.stores
            .guilds
            .iter()
            .filter_map(|guild| {
                let (tag, badge) = guild.selectable_server_tag()?;
                Some((
                    guild.id,
                    guild.name.clone(),
                    tag.to_owned(),
                    badge.map(str::to_owned),
                ))
            })
            .collect()
    }

    pub(super) fn profile_editor_request_tag_art(&mut self) {
        let requests: Vec<(String, String)> = self
            .profile_editor_eligible_tags()
            .into_iter()
            .flat_map(|(gid, _, _, badge)| {
                let guild_icon = self
                    .stores
                    .guilds
                    .iter()
                    .find(|g| g.id == gid)
                    .and_then(|g| g.icon.as_deref())
                    .map(|hash| {
                        (
                            format!("guild:{gid}"),
                            format!("https://cdn.discordapp.com/icons/{gid}/{hash}.png?size=64"),
                        )
                    });
                let clan_badge = badge.map(|hash| {
                    let url =
                        format!("https://cdn.discordapp.com/clan-badges/{gid}/{hash}.png?size=16");
                    (url.clone(), url)
                });
                guild_icon.into_iter().chain(clan_badge)
            })
            .collect();
        for (key, url) in requests {
            self.request_image(key, url);
        }
    }

    pub(super) fn profile_editor_pick_tag(&mut self, guild_id: Option<Id>, cx: &mut Context<Self>) {
        let saved = self
            .stores
            .current_user
            .as_ref()
            .and_then(|u| u.tag.as_ref())
            .map(|t| t.guild_id);
        if let Some(open) = self.profile_editor.as_mut() {
            if saved == guild_id {
                open.pending.primary_guild_id = None;
            } else {
                open.pending.primary_guild_id = Some(guild_id);
            }
            open.tag_select_open = false;
        }
        cx.notify();
    }

    /// `uP` of module 927071: the server tag select beside the pronouns, with the account's tag or the Server Tag
    /// placeholder (`dc_intl::t!("Pdd1nd")`).
    fn profile_editor_tag_pill(
        &self,
        user: &User,
        palette: &dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tag = user.tag.clone();
        let open = self.profile_editor.as_ref()?;
        let eligible = self.profile_editor_eligible_tags();
        let any_tag_server = !eligible.is_empty()
            || self
                .stores
                .guilds
                .iter()
                .any(|guild| guild.features.iter().any(|feature| feature == "GUILD_TAGS"));
        if tag.is_none() && !any_tag_server {
            return None;
        }
        let placeholder = tag.is_none();
        let editable = open.guild.is_none();
        let expanded = open.tag_select_open && editable;
        let text_default = palette.token("--text-default");
        let text_placeholder = palette.token("--input-placeholder-text-default");
        let content = match tag {
            Some(tag) => {
                let badge = tag
                    .badge_url()
                    .and_then(|url| self.images.get(&url).cloned());
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .children(badge.map(|image| img(image).size(px(14.))))
                    .child(tinted_text(tag.tag, 12., 16., 500., text_default))
            }
            None => div().italic().child(tinted_text(
                dc_intl::t!("Pdd1nd"),
                12.,
                16.,
                400.,
                text_placeholder,
            )),
        };
        let pill_hover = palette.pill_background(true);
        // `selectButtonPill_C3qxaU`: 24 high, 8px radius, 8px in (6px for the placeholder) and 6px at the end.
        let mut pill = div()
            .id("profile-editor-server-tag")
            .relative()
            .h(px(24.))
            .pl(px(if placeholder { 6. } else { 8. }))
            .pr(px(6.))
            .rounded(px(8.))
            .border_1()
            .border_color(palette.token("--border-muted"))
            .bg(palette.pill_background(expanded))
            .when(editable, |this| {
                this.hover(move |style| style.bg(pill_hover))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let search = this
                            .profile_editor
                            .as_ref()
                            .map(|open| open.tag_search.clone());
                        if let Some(search) = search {
                            search.update(cx, |editor, _| editor.set_text(""));
                        }
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.tag_select_open = !open.tag_select_open;
                        }
                        this.profile_editor_request_tag_art();
                        cx.notify();
                    }))
            })
            .flex()
            .items_center()
            .child(self.anchor_probe("profile-editor-server-tag"))
            .child(content)
            .when(editable, |this| {
                this.child(div().ml(px(2.)).child(tinted_icon(
                    if expanded {
                        "discover_select_chevron_up"
                    } else {
                        "chevron_down"
                    },
                    12.,
                    text_default,
                )))
            });
        if !editable {
            pill = pill.opacity(0.8);
        }
        Some(pill.into_any_element())
    }

    /// `.listbox_C3qxaU` (`927071` `uk` / `685073` `gC`): `280px` searchable Server Tag menu (`240px` empty state).
    pub(super) fn profile_editor_tag_select_popout(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.profile_editor.as_ref()?;
        if !open.tag_select_open || open.guild.is_some() {
            return None;
        }
        let search_editor = open.tag_search.clone();
        let query = search_editor.read(cx).text().trim().to_lowercase();
        let geometry = open.geometry;
        let anchor = self
            .tip_anchors
            .borrow()
            .get("profile-editor-server-tag")
            .copied();
        let eligible = self.profile_editor_eligible_tags();
        let selected_guild = self.profile_editor_user().tag.map(|t| t.guild_id);
        let accent = color("--control-primary-background-default");

        let panel = if eligible.is_empty() {
            div()
                .id("profile-editor-server-tag-popout")
                .occlude()
                .w(px(240.))
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t!("NV+MBV")),
                )
        } else {
            let filtered: Vec<_> = eligible
                .into_iter()
                .filter(|(_, name, tag, _)| {
                    query.is_empty()
                        || name.to_lowercase().contains(&query)
                        || tag.to_lowercase().contains(&query)
                })
                .collect();
            let clear_selected = selected_guild.is_none();
            let clear_row = div()
                .id("profile-editor-tag-option-none")
                .h(px(36.))
                .px(px(8.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(color("--interactive-background-hover")))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .italic()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t!("VxdWWH")),
                )
                .when(clear_selected, |this| {
                    this.child(tinted_icon("select_check", 16., accent))
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.profile_editor_pick_tag(None, cx);
                }));
            let guild_rows = filtered
                .into_iter()
                .map(|(gid, name, tag_text, badge_hash)| {
                    let selected = selected_guild == Some(gid);
                    let guild_icon = self.images.get(&format!("guild:{gid}")).cloned();
                    let acronym = self
                        .stores
                        .guilds
                        .iter()
                        .find(|g| g.id == gid)
                        .map(dc_state::Guild::acronym)
                        .unwrap_or_default();
                    let badge_url = badge_hash.map(|hash| {
                        format!("https://cdn.discordapp.com/clan-badges/{gid}/{hash}.png?size=16")
                    });
                    let badge_img = badge_url
                        .as_ref()
                        .and_then(|url| self.images.get(url).cloned());
                    let leading = div()
                        .size(px(20.))
                        .flex_none()
                        .rounded(px(6.))
                        .overflow_hidden()
                        .bg(color("--background-surface-high"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(9.))
                        .text_color(color("--text-default"))
                        .child(match guild_icon {
                            Some(image) => img(image).size(px(20.)).into_any_element(),
                            None => div().child(acronym).into_any_element(),
                        });
                    let chip = div()
                        .h(px(20.))
                        .px(px(6.))
                        .rounded(px(4.))
                        .bg(color("--background-mod-subtle"))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .children(badge_img.map(|image| img(image).size(px(14.))))
                        .child(text(tag_text, 12., 16., 600., "--text-default"));
                    div()
                        .id(SharedString::from(format!(
                            "profile-editor-tag-option-{gid}"
                        )))
                        .h(px(36.))
                        .px(px(8.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--interactive-background-hover")))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(leading)
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-strong"))
                                .child(name),
                        )
                        .child(chip)
                        .when(selected, |this| {
                            this.child(tinted_icon("select_check", 16., accent))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.profile_editor_pick_tag(Some(gid), cx);
                        }))
                });
            div()
                .id("profile-editor-server-tag-popout")
                .occlude()
                .w(px(280.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .h(px(34.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(tinted_icon("search", 16., color("--icon-subtle")))
                        .child(div().flex_1().min_w(px(0.)).child(search_editor)),
                )
                .child(
                    div()
                        .id("profile-editor-server-tag-list")
                        .max_h(px(240.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(clear_row)
                        .children(guild_rows),
                )
        };
        let pos = anchor.map_or_else(
            || {
                gpui::point(
                    px(geometry.left + geometry.panel_width + geometry.padding + 16.),
                    px(geometry.top + geometry.content_top() + 300.),
                )
            },
            |bounds| gpui::point(bounds.left(), bounds.bottom() + px(4.)),
        );
        Some(
            gpui::deferred(
                gpui::anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(pos)
                    .snap_to_window_with_margin(px(8.))
                    .child(panel.on_mouse_down_out(cx.listener(
                        |this, event: &gpui::MouseDownEvent, _, cx| {
                            if let Some(open) = this.profile_editor.as_mut() {
                                open.tag_select_open = false;
                            }
                            if event.button != gpui::MouseButton::Right {
                                cx.stop_propagation();
                            }
                            cx.notify();
                        },
                    ))),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// The bar at the bottom of the stage: Nitro preview exit, or the unsaved-changes bar with Save and Reset.
    fn profile_editor_bar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.profile_editor.as_ref()?;
        let nitro = open.mode == Mode::Nitro;
        let changed = open.has_changes();
        if !nitro && !changed {
            return None;
        }
        let saving = open.saving;
        let save_error = (!nitro).then(|| open.error.clone()).flatten();
        let message: SharedString = if nitro {
            SharedString::from("You're previewing what your profile could look like with Nitro")
        } else if let Some(err) = save_error.as_ref() {
            SharedString::from(err.clone())
        } else {
            SharedString::from("Careful — you have unsaved changes!")
        };
        let message_token = if save_error.is_some() {
            "--text-feedback-critical"
        } else {
            "--text-strong"
        };
        let buttons = if nitro {
            div()
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .id("profile-editor-exit-preview")
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .child(text(
                            "Exit Preview",
                            14.,
                            18.,
                            500.,
                            "--control-secondary-text-default",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(open) = this.profile_editor.as_mut() {
                                open.mode = Mode::Main;
                                open.tab = Tab::Board;
                            }
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("profile-editor-bar-get-nitro")
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .bg(rgba(0xffffffff))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .child(icon("settings_nitro", 16., "--black-500"))
                        .child(text("Get Nitro", 14., 18., 500., "--black-500"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx))),
                )
        } else {
            div()
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .id("profile-editor-reset")
                        .h(px(32.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .child(text("Reset", 14., 18., 500., "--text-strong"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.profile_editor_reset(cx);
                        })),
                )
                .child(
                    div()
                        .id("profile-editor-save")
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .opacity(if saving { 0.5 } else { 1. })
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .child(text(
                            "Save Changes",
                            14.,
                            18.,
                            500.,
                            "--control-primary-text-default",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| this.profile_editor_save(cx))),
                )
        };
        let geometry = open.geometry;
        let visible_stage = if geometry.panel_over && !open.panel_collapsed {
            geometry.width - geometry.panel_width
        } else {
            geometry.content_width()
        };
        let bar_width = (visible_stage - 84.).clamp(280., 612.);
        let bar_left = ((visible_stage - bar_width) / 2.).clamp(12., 42.);
        Some(
            div()
                .absolute()
                .bottom(px(24.))
                .left(px(bar_left))
                .w(px(bar_width))
                .h(px(48.))
                .px(px(12.))
                .rounded(px(12.))
                .backdrop_blur(px(4.))
                .bg(color("--background-surface-highest"))
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .child(text(message, 14., 18., 400., message_token)),
                )
                .child(buttons)
                .into_any_element(),
        )
    }

    pub(super) fn profile_editor_reset(&mut self, cx: &mut Context<Self>) {
        let me = self.stores.current_user.clone().unwrap_or_default();
        let guild = self.profile_editor.as_ref().and_then(|open| open.guild);
        let nick = guild
            .and_then(|id| self.stores.members.get(id, me.id))
            .and_then(|member| member.nick.clone());
        let shown_name = match (guild, nick) {
            (Some(_), Some(nick)) => nick,
            _ => me.display_name().to_owned(),
        };
        if let Some(open) = self.profile_editor.as_mut() {
            open.pending = super::save::Pending::new(open.guild);
            open.avatar_image = None;
            open.banner_image = None;
            open.picker = None;
            open.banner_color = None;
            open.tag_select_open = false;
            open.game_picker = None;
            open.widget_modal = None;
            open.widget_confirm_remove = None;
            open.board_widgets = open.saved.widgets.clone();
            open.error = None;
            let (name, pronouns, bio) =
                (open.name.clone(), open.pronouns.clone(), open.bio.clone());
            let (saved_pronouns, saved_bio) = (open.saved.pronouns.clone(), open.saved.bio.clone());
            name.update(cx, |editor, _| editor.set_text(&shown_name));
            pronouns.update(cx, |editor, _| editor.set_text(&saved_pronouns));
            bio.update(cx, |editor, _| editor.set_text(&saved_bio));
        }
        cx.notify();
    }
}

/// The status bubble's prompts (`P2` of module 403918) in their order; one is drawn at random per open (module 645507).
const PROMPTS: [&str; 31] = [
    "What have you been playing?",
    "What have you been listening to?",
    "What have you been watching?",
    "What are you currently obsessed with?",
    "Shower thought?",
    "Today I learned...",
    "What\u{2019}s your hot take?",
    "Best dad joke?",
    "Best emoji to describe your day?",
    "What\u{2019}s the most useless talent you have?",
    "What video game item would you be?",
    "What have you been reading?",
    "Last song stuck in your head?",
    "Most used emoji lately?",
    "Best thing you ate this week?",
    "What fictional world would you live in?",
    "What's your username origin story?",
    "If your life had a theme song...",
    "Favorite collectible?",
    "Most satisfying game mechanic?",
    "Favorite NPC companion?",
    "Food you\u{2019}re craving now?",
    "Pick a mythical creature as a pet",
    "Latest hobby?",
    "Favorite anime lately?",
    "Just ranked up in...",
    "Choose your character class",
    "Hit a high score in...",
    "Just finished playing...",
    "Just finished reading...",
    "Can\u{2019}t wait for...",
];

/// `Math.floor(Math.random() * list.length)` of module 645507, from the clock's nanoseconds.
pub(super) fn random_prompt() -> &'static str {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    PROMPTS[nanos as usize % PROMPTS.len()]
}

#[cfg(test)]
mod tests {
    use super::super::super::profile_theme::ProfileTheme;
    use super::{PROMPTS, blend, random_prompt};
    use dc_tokens::Theme;
    use gpui::rgba;

    /// Every prompt is an en-US message, and a random one comes from the list.
    #[test]
    fn the_prompts_are_messages() {
        for prompt in PROMPTS {
            assert!(
                dc_intl::keys_of(prompt).next().is_some(),
                "{prompt:?} is not an en-US message"
            );
        }
        assert!(PROMPTS.contains(&random_prompt()));
    }

    /// Dark themes stay dark under the black 60% overlay; light themes lift under the white 60% overlay.
    #[test]
    fn the_overlay_uses_profile_theme() {
        let dark = ProfileTheme::new((0x000000, 0x000000), None, Theme::Darker, false);
        assert_eq!(blend(dark.primary, dark.overlay), rgba(0x000000ff));
        let light = ProfileTheme::new((0xffffff, 0xffffff), None, Theme::Darker, false);
        assert!(light.light());
        assert_eq!(blend(light.primary, light.overlay), rgba(0xffffffff));
    }
}
