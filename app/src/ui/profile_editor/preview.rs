//! The middle of the editor: the live profile card (`profile_s3DB6Y`, up to 400 wide) and the stage around it
//! (`outer_P1YEjQ`), with the save bar. The card is the profile with its name, pronouns and bio editable in place
//! (module 927071, `profileBody_s3DB6Y`).

use super::{Mode, Tab, text};
use crate::parity::Parity;
use crate::ui::{Root, color, dm_profile, icon, name_styles};
use dc_state::User;
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, Window, div, img, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};

fn section(title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(text(title, 12., 16., 500., "--text-subtle"))
}

fn round_button(id: &'static str, glyph: &'static str) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .size(px(32.))
        .rounded(px(8.))
        .bg(color("--control-secondary-background-default"))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(glyph, 16., "--control-secondary-text-default"))
}

/// A dot of the thought bubble leading from the avatar to the status bubble.
fn thought_dot(left: f32, top: f32, size: f32) -> Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .size(px(size))
        .rounded_full()
        .bg(color("--background-surface-highest"))
}

/// Composite `overlay` over `base` when the gradient is painted without stacking translucent layers.
fn blend(base: gpui::Rgba, overlay: gpui::Rgba) -> gpui::Rgba {
    let a = overlay.a;
    gpui::Rgba {
        r: overlay.r * a + base.r * (1. - a),
        g: overlay.g * a + base.g * (1. - a),
        b: overlay.b * a + base.b * (1. - a),
        a: 1.,
    }
}

fn themed_fill(theme: &super::super::profile_theme::ProfileTheme) -> gpui::Background {
    linear_gradient(
        180.,
        linear_color_stop(blend(theme.primary, theme.overlay), 0.),
        linear_color_stop(blend(theme.secondary, theme.overlay), 1.),
    )
}

impl Root {
    fn profile_editor_palette(&self) -> dm_profile::Palette {
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
        let geometry = self
            .profile_editor
            .as_ref()
            .expect("editor is open")
            .geometry;
        let palette = self.profile_editor_palette();
        let card = self.profile_editor_card(user, window, cx);
        // `profileContentColumns_s3DB6Y`: the card and the widgets column side by side, down to the editor's bottom.
        let columns = div()
            .flex()
            .w(px(geometry.columns_width()))
            .h(px(geometry.card_height()))
            .child(card)
            .child(self.profile_editor_widgets(window, cx));
        // `inner_P1YEjQ profileContentInner_s3DB6Y`: the column's padding, the title bar's height at its top. A themed
        // profile (`custom-user-profile-theme`) paints the whole column in its colours.
        let mut stage = div()
            .parity("profile-editor-stage")
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(color("--background-base-lower"))
            .pl(px(geometry.padding))
            .pt(px(geometry.content_top()))
            .when(geometry.content_border > 0., |this| {
                this.border_r_1().border_color(color("--border-normal"))
            })
            .child(columns);
        if let Some(theme) = palette.theme.as_ref() {
            stage = stage.bg(themed_fill(theme));
        }
        if let Some(bar) = self.profile_editor_bar(cx) {
            stage = stage.child(bar);
        }
        stage.into_any_element()
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
        let badges = if nitro {
            Vec::new()
        } else {
            self.profile_badges("profile-editor", user.id, cx)
        };
        let server_tag = if nitro {
            None
        } else {
            self.profile_editor_tag_pill(user)
        };
        let open = self.profile_editor.as_ref().expect("editor is open");
        let geometry = open.geometry;
        let premium = self.profile_editor_premium();
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

        let banner_fill = div()
            .w(px(width))
            .h(px(140.))
            .rounded_t(px(16.))
            .overflow_hidden()
            .bg(rgba((banner << 8) | 0xff))
            .children(banner_image.map(|image| {
                img(image)
                    .w(px(width))
                    .h(px(140.))
                    .object_fit(gpui::ObjectFit::Cover)
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
                .text_color(color("--text-strong"))
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
                    .child(text(username, 14., 18., 400., "--text-strong"))
                    .child(
                        div()
                            .size(px(4.))
                            .rounded_full()
                            .bg(color("--text-default")),
                    )
                    .child(div().min_w(px(80.)).child(pronouns)),
            )
            .children(server_tag)
            .when(!badges.is_empty(), |this| {
                this.child(
                    div()
                        .h(px(22.))
                        .px(px(3.))
                        .py(px(1.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-muted"))
                        .bg(color("--background-mod-subtle"))
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .children(badges),
                )
            });
        let buttons = div()
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .opacity(0.5)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("chat_bubble", 16., "--control-primary-text-default"))
                    .child(text(
                        "Message",
                        14.,
                        18.,
                        500.,
                        "--control-primary-text-default",
                    )),
            )
            .child(round_button("profile-editor-shop-button", "shop"))
            .child(round_button(
                "profile-editor-more-button",
                "more_horizontal",
            ));
        let upsell_card = upsell.then(|| {
            div()
                .w(px(width - 2. * pad))
                .h(px(86.))
                .rounded(px(8.))
                .border_2()
                .border_color(rgba(0x8a6ff0ff))
                .bg(color("--background-surface-highest"))
                .p(px(10.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .child(div().flex_1().child(text(
                            "Amp up your profile",
                            14.,
                            18.,
                            400.,
                            "--text-strong",
                        )))
                        .child(
                            div()
                                .id("profile-editor-upsell-close")
                                .cursor_pointer()
                                .child(icon("upsell_close", 16., "--icon-subtle"))
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
                                .bg(color("--control-secondary-background-default"))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .child(icon(
                                    if premium {
                                        "nitro_wheel"
                                    } else {
                                        "settings_nitro"
                                    },
                                    16.,
                                    "--control-secondary-text-default",
                                ))
                                .child(text(
                                    if premium { "View Perks" } else { "Get Nitro" },
                                    14.,
                                    18.,
                                    500.,
                                    "--control-secondary-text-default",
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
                                .bg(color("--control-secondary-background-default"))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .child(icon("shop", 16., "--control-secondary-text-default"))
                                .child(text(
                                    "Shop",
                                    14.,
                                    18.,
                                    500.,
                                    "--control-secondary-text-default",
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.profile_editor = None;
                                    this.close_settings(cx);
                                    this.open_shop(cx);
                                })),
                        ),
                )
        });
        let bio_section =
            (!nitro).then(|| section("Bio").child(div().ml(px(-7.)).px(px(7.)).child(bio)));
        let connections = (!nitro).then(|| {
            section("Connections").child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(icon("plus_small", 18., "--text-muted"))
                    .child(text("Add Connection", 12., 16., 500., "--text-muted")),
            )
        });
        // `profileBody_s3DB6Y`: the card's padding, scrolling under the header when it does not fit.
        let body = div()
            .id("profile-editor-card-body")
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
            .child(section("Member Since").child(text(since, 14., 18., 400., "--text-default")))
            .children(connections)
            .child(section("Note (only visible to you)").child(text(
                "Click to add a note",
                14.,
                18.,
                400.,
                "--text-muted",
            )));

        // `profile_s3DB6Y`: rounded at the top only, from the column's padding to the editor's bottom edge.
        let mut card = div()
            .parity("profile-editor-card")
            .id("profile-editor-card")
            .relative()
            .w(px(width))
            .h(px(geometry.card_height()))
            .flex_none()
            .rounded_t(px(16.))
            .overflow_hidden()
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .h(px(220.))
                    .flex_none()
                    .child(banner_fill)
                    .child(avatar)
                    .child(thought_dot(bubble_left, 121., 8.))
                    .child(thought_dot(bubble_left + 16., 131., 14.))
                    .child(bubble),
            )
            .child(body);
        if let Some(theme) = palette.theme.as_ref() {
            // `outline: 1px solid var(--border-muted)` keeps the themed card apart from the themed column.
            card = card
                .bg(themed_fill(theme))
                .border_1()
                .border_color(palette.token("--border-muted"));
        }
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
        card.children(frame_layer)
            .children(effect_layer)
            .into_any_element()
    }

    /// `uP` of module 927071: the server tag select beside the pronouns, with the account's tag or the Server Tag
    /// placeholder. Drawn while the account has a tag or is in a server with `GUILD_TAGS` (the official also lists the
    /// servers' own tags, which the guild objects here do not carry).
    fn profile_editor_tag_pill(&self, user: &User) -> Option<AnyElement> {
        let tag = user.tag.clone();
        let any_tag_server = self
            .stores
            .guilds
            .iter()
            .any(|guild| guild.features.iter().any(|feature| feature == "GUILD_TAGS"));
        if tag.is_none() && !any_tag_server {
            return None;
        }
        let placeholder = tag.is_none();
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
                    .child(text(tag.tag, 12., 16., 400., "--text-default"))
            }
            None => div().italic().child(text(
                dc_intl::tr("Server Tag"),
                12.,
                16.,
                400.,
                "--input-placeholder-text-default",
            )),
        };
        // `selectButtonPill_C3qxaU`: 24 high, 8px radius, 8px in (6px for the placeholder) and 6px at the end.
        Some(
            div()
                .id("profile-editor-server-tag")
                .h(px(24.))
                .pl(px(if placeholder { 6. } else { 8. }))
                .pr(px(6.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-muted"))
                .bg(color("--background-mod-subtle"))
                .hover(|style| style.bg(color("--background-mod-strong")))
                .cursor_pointer()
                .flex()
                .items_center()
                .child(content)
                .child(
                    div()
                        .ml(px(2.))
                        .child(icon("chevron_down", 12., "--text-default")),
                )
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
        let message = if nitro {
            "You're previewing what your profile could look like with Nitro"
        } else {
            "Careful — you have unsaved changes!"
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
                .bg(color("--background-surface-highest"))
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .child(text(message, 14., 18., 400., "--text-strong")),
                )
                .child(buttons)
                .into_any_element(),
        )
    }

    fn profile_editor_reset(&mut self, cx: &mut Context<Self>) {
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
