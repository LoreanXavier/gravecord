//! The middle of the editor: the live profile card (`profile_s3DB6Y`, 400 wide) and the stage around it
//! (`outer_P1YEjQ`), with the save bar. The card is the profile with its name, pronouns and bio editable in place
//! (module 927071, `profileBody_s3DB6Y`).

use super::{Mode, Tab, text};
use crate::parity::Parity;
use crate::ui::{Root, avatar_source, color, dm_profile, icon, name_styles};
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

impl Root {
    pub(super) fn profile_editor_stage(
        &mut self,
        user: &User,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let card = self.profile_editor_card(user, window, cx);
        let columns = div()
            .flex()
            .w(px(861.))
            .h(px(640.))
            .child(card)
            .child(self.profile_editor_widgets(cx));
        let mut stage = div()
            .parity("profile-editor-stage")
            .relative()
            .flex_1()
            .h_full()
            .bg(color("--background-base-lower"))
            .pl(px(48.))
            .pt(px(80.))
            .child(columns);
        if let Some(bar) = self.profile_editor_bar(cx) {
            stage = stage.child(bar);
        }
        stage.into_any_element()
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
        let banner_image = nitro.then(|| self.profile_editor_preset_banner()).flatten();
        let open = self.profile_editor.as_ref().expect("editor is open");
        let premium = self.profile_editor_premium();
        let theme = nitro.then(|| open.trying.theme_colors());
        let banner = super::panel::banner_color(open, user);
        let avatar_image = self.images.get(&avatar_source(user).0).cloned();
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
        let styled_name = user.display_name_styles.is_some();
        let upsell = !premium && !nitro && !open.upsell_closed;
        let (name, pronouns, bio) = (open.name.clone(), open.pronouns.clone(), open.bio.clone());
        let username = user.username.clone();
        let since = dm_profile::member_since(user.id);

        let banner_fill = div()
            .w(px(400.))
            .h(px(140.))
            .rounded_t(px(16.))
            .overflow_hidden()
            .bg(rgba((banner << 8) | 0xff))
            .children(banner_image.map(|image| {
                img(image)
                    .w(px(400.))
                    .h(px(140.))
                    .object_fit(gpui::ObjectFit::Cover)
            }));
        let avatar = div()
            .absolute()
            .left(px(23.))
            .top(px(71.))
            .size(px(138.))
            .rounded_full()
            .bg(color("--background-surface-high"))
            .p(px(9.))
            .child(
                div()
                    .relative()
                    .size(px(120.))
                    .rounded_full()
                    .child(
                        div()
                            .size(px(120.))
                            .rounded_full()
                            .overflow_hidden()
                            .children(
                                avatar_image.map(|image| img(image).size(px(120.)).rounded_full()),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(88.))
                            .top(px(88.))
                            .size(px(32.))
                            .rounded_full()
                            .bg(color("--background-surface-high"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(24.))
                                    .rounded_full()
                                    .bg(color("--status-danger"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .w(px(12.))
                                            .h(px(4.))
                                            .rounded(px(2.))
                                            .bg(rgba(0xffffffff)),
                                    ),
                            ),
                    )
                    .children(decoration.map(|image| {
                        div()
                            .absolute()
                            .left(px(-12.))
                            .top(px(-12.))
                            .child(img(image).size(px(144.)))
                    })),
            );
        let bubble = div()
            .absolute()
            .left(px(172.))
            .top(px(132.))
            .w(px(180.))
            .when(nitro, |this| {
                // The themed card wraps the bubble to two lines over a dark fill.
                this.h(px(54.)).bg(rgba(0x1f1c2bff))
            })
            .when(!nitro, |this| {
                this.h(px(36.)).bg(color("--background-surface-highest"))
            })
            .italic()
            .px(px(12.))
            .rounded(px(16.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(icon("status_add", 16., "--icon-subtle"))
            .child(div().flex_1().min_w(px(0.)).child(text(
                if nitro {
                    "Pick a mythical creature as a pet"
                } else {
                    "Just finished reading..."
                },
                14.,
                18.,
                400.,
                "--text-muted",
            )));

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
        let handle_row = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(!nitro, |this| this.h(px(24.)).mt(px(3.)))
            .child(text(username, 14., 18., 400., "--text-strong"))
            .child(
                div()
                    .size(px(4.))
                    .rounded_full()
                    .bg(color("--text-default")),
            )
            .child(div().min_w(px(80.)).child(pronouns));
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
                    .child(icon("msg_edit", 16., "--control-primary-text-default"))
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
                .w(px(336.))
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
                                    if let Some(open) = this.profile_editor.as_mut() {
                                        open.upsell_closed = true;
                                    }
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
                                    "settings_nitro",
                                    16.,
                                    "--control-secondary-text-default",
                                ))
                                .child(text(
                                    "Get Nitro",
                                    14.,
                                    18.,
                                    500.,
                                    "--control-secondary-text-default",
                                ))
                                .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx))),
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
        let body = div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .pl(px(32.))
            .pr(px(32.))
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
        let _ = window;

        let mut card = div()
            .parity("profile-editor-card")
            .id("profile-editor-card")
            .relative()
            .w(px(400.))
            .h(px(640.))
            .flex_none()
            .rounded(px(16.))
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
                    .child(thought_dot(172., 121., 8.))
                    .child(thought_dot(188., 131., 14.))
                    .child(bubble),
            )
            .child(body);
        if let Some(theme) = theme {
            // Dark profile theme: the gradient under `--profile-gradient-overlay-color: #00000099`.
            let shade = |rgb: u32| {
                let channel = |shift: u32| (((rgb >> shift) & 0xff) as f32 * 0.4).round() as u32;
                (channel(16) << 24) | (channel(8) << 16) | (channel(0) << 8) | 0xff
            };
            card = card.bg(linear_gradient(
                180.,
                linear_color_stop(rgba(shade(theme[0])), 0.),
                linear_color_stop(rgba(shade(theme[1])), 1.),
            ));
        }
        let effect_layer = effect.and_then(|sku| {
            self.profile_effect_layer(
                "profile-editor",
                sku,
                super::super::profile_effects::Surface::profile(400., false),
                window,
                cx,
            )
        });
        card.children(effect_layer).into_any_element()
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
        Some(
            div()
                .absolute()
                .bottom(px(24.))
                .left(px(42.))
                .w(px(612.))
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
        if let Some(open) = self.profile_editor.as_mut() {
            open.pending = super::save::Pending::new(open.guild);
            open.error = None;
            let (name, pronouns, bio) =
                (open.name.clone(), open.pronouns.clone(), open.bio.clone());
            let (saved_pronouns, saved_bio) = (open.saved.pronouns.clone(), open.saved.bio.clone());
            name.update(cx, |editor, _| editor.set_text(me.display_name()));
            pronouns.update(cx, |editor, _| editor.set_text(&saved_pronouns));
            bio.update(cx, |editor, _| editor.set_text(&saved_bio));
        }
        cx.notify();
    }
}
