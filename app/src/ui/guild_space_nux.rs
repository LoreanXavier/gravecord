//! Module 725041 (`GUILD_SPACE_NUX_MODAL`): "Welcome to your Server Hub" expressive blue-gradient NUX modal.
//!
//! Introduces Guild Space (Server Hub) with a 16:9 graphic header, 3 feature rows (`iconContainer_XmFhxL` 42x42),
//! and Published vs Draft CTA buttons (`Jump in` + optional `Server Settings` -> Engagement).

use super::{Root, color, icon};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, linear_color_stop, linear_gradient,
    prelude::*, px, rgba,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GuildSpaceNuxModal {
    pub guild_id: Id,
    pub published: bool,
    pub edit_started: bool,
}

/// Feature row descriptor matching module 725041 (`135294` -> `956447.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NuxFeature {
    pub icon_name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

/// Returns the 3 feature rows of module 725041 for `published` vs draft state.
pub(super) fn features_for(published: bool) -> [NuxFeature; 3] {
    [
        NuxFeature {
            icon_name: "grid_square",
            title: "Uniquely yours",
            description: "Add, edit, and remove widgets to make it feel like home.",
        },
        NuxFeature {
            icon_name: "mod_refresh",
            title: "Always current",
            description: "Widgets refresh regularly with what\u{2019}s new.",
        },
        if published {
            NuxFeature {
                icon_name: "circle_check",
                title: "Up and running",
                description: "Currently live. Turn it off anytime in Server Settings.",
            }
        } else {
            NuxFeature {
                icon_name: "circle_check",
                title: "Ready when you are",
                description: "Publish when it feels right, and turn it off anytime.",
            }
        },
    ]
}

impl Root {
    /// Opens module 725041 (`GUILD_SPACE_NUX_MODAL`) for `guild_id`.
    pub(crate) fn open_guild_space_nux(
        &mut self,
        guild_id: Id,
        published: bool,
        cx: &mut Context<Self>,
    ) {
        self.guild_space_nux = Some(GuildSpaceNuxModal {
            guild_id,
            published,
            edit_started: false,
        });
        cx.notify();
    }

    /// Module 725041 (`GUILD_SPACE_NUX_MODAL`) layer. Auto-dismisses if active guild switched away.
    pub(super) fn guild_space_nux_layer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.guild_space_nux?;
        if self.guild.is_some() && self.guild != Some(state.guild_id) {
            self.guild_space_nux = None;
            return None;
        }
        let guild_id = state.guild_id;
        let published = state.published;
        let features = features_for(published);

        let graphic = div()
            .w_full()
            .h(px(210.))
            .rounded_t(px(12.))
            .bg(linear_gradient(
                135.,
                linear_color_stop(rgba(0x3a48b8ff), 0.),
                linear_color_stop(rgba(0x1f2547ff), 1.),
            ))
            .flex()
            .items_center()
            .justify_center()
            .relative()
            .child(
                div()
                    .w(px(240.))
                    .h(px(136.))
                    .rounded(px(12.))
                    .bg(rgba(0x161928cc))
                    .border_1()
                    .border_color(rgba(0xffffff24))
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(22.))
                                    .rounded(px(6.))
                                    .bg(color("--brand-500"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("grid_square", 14., "--white")),
                            )
                            .child(
                                div()
                                    .h(px(10.))
                                    .w(px(110.))
                                    .rounded(px(5.))
                                    .bg(rgba(0xffffff38)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .rounded(px(8.))
                                    .bg(rgba(0x5865f244))
                                    .border_1()
                                    .border_color(rgba(0x5865f288)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .rounded(px(8.))
                                    .bg(rgba(0xffffff14))
                                    .border_1()
                                    .border_color(rgba(0xffffff22)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("guild-space-nux-close")
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .size(px(32.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(0xffffff1a)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 18., "--white"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.guild_space_nux = None;
                        cx.notify();
                    })),
            );

        let rows: Vec<AnyElement> = features
            .iter()
            .map(|f| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        // `.iconContainer_XmFhxL`: 42x42, `--radius-md` (12px), `--background-mod-subtle`.
                        div()
                            .size(px(42.))
                            .flex_none()
                            .rounded(px(12.))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(f.icon_name, 20., "--icon-strong")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(f.title),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(f.description),
                            ),
                    )
                    .into_any_element()
            })
            .collect();

        let footer = div()
            .px(px(24.))
            .py(px(16.))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .when(published, |bar| {
                bar.child(
                    div()
                        .id("guild-space-nux-settings")
                        .h(px(38.))
                        .px(px(16.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|s| s.bg(color("--control-secondary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .child(dc_intl::t("154/bL"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.guild_space_nux = None;
                            this.open_server_settings(guild_id, cx);
                            if let Some(open) = this.server_settings.as_mut() {
                                open.page = super::server_settings::Page::Engagement;
                            }
                            cx.notify();
                        })),
                )
            })
            .child(
                div()
                    .id("guild-space-nux-jump")
                    .h(px(38.))
                    .px(px(20.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--control-primary-text-default"))
                    .child("Jump in")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.guild_space_nux = None;
                        cx.notify();
                    })),
            );

        let modal = div()
            .id("guild-space-nux-modal")
            .occlude()
            .w(px(480_f32.min((self.viewport.0 - 48.).max(320.))))
            .max_h(px((self.viewport.1 - 64.).max(320.)))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![super::shadow_high()])
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(graphic)
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child("Welcome to your Server Hub"),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-subtle"))
                                    .child("A shared place for @everyone to see what\u{2019}s happening in your server and goof around."),
                            ),
                    )
                    .child(div().flex().flex_col().gap(px(16.)).children(rows)),
            )
            .child(footer);

        Some(
            gpui::deferred(
                div()
                    .id("guild-space-nux-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(color("--background-scrim"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.guild_space_nux = None;
                        cx.notify();
                    }))
                    .child(modal),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_guild_space_nux(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        let gid = self.guild.unwrap_or(1);
        match mode.trim() {
            "close" => {
                self.guild_space_nux = None;
                cx.notify();
                "guild-space-nux: closed".to_owned()
            }
            "draft" => {
                self.open_guild_space_nux(gid, false, cx);
                "guild-space-nux: draft".to_owned()
            }
            _ => {
                self.open_guild_space_nux(gid, true, cx);
                "guild-space-nux: published".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_and_draft_features_match_725041() {
        let pub_rows = features_for(true);
        assert_eq!(pub_rows[0].title, "Uniquely yours");
        assert_eq!(pub_rows[1].title, "Always current");
        assert_eq!(pub_rows[2].title, "Up and running");

        let draft_rows = features_for(false);
        assert_eq!(draft_rows[2].title, "Ready when you are");
    }
}
