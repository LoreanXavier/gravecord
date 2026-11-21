//! Guild Theme NUX Modal ("This server has its own look", module 166998).
//!
//! Shown when visiting a guild with a custom `GUILD_THEME` before the user has picked a default
//! server-theme preference (`GUILD_THEME_NUX`).
//! - Title `Q9zFy9` ("This server has its own look"), subtitle `XLpBLj`
//! - 16:9 dynamic preview card contrasting the server's custom theme vs the user's personal theme
//! - Dropdown/select (`Q7mm4g` "Default theme in servers") with `aN3RNQ` ("Use a server's theme
//!   by default") and `js8y7t` ("Use my theme by default")
//! - Warning notice `tTHQAy` when personal theme is selected, error alert `fEptJP` on failure
//! - Single footer action: `cY+Oob` ("Confirm", primary) for Guild, or `cvoikF` ("Continue
//!   anyway", secondary) for Personal. Persists preference via `crate::storage`.

use super::{Root, color, icon};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, deferred, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};

const STORAGE_KEY: &str = "default-guild-theme-preference";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuildThemeChoice {
    Guild,
    Personal,
}

impl GuildThemeChoice {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Guild => dc_intl::t("aN3RNQ"),
            Self::Personal => dc_intl::t("js8y7t"),
        }
    }

    pub(crate) fn action_label(self) -> &'static str {
        match self {
            Self::Guild => dc_intl::t("cY+Oob"),
            Self::Personal => dc_intl::t("cvoikF"),
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct GuildThemeNuxModal {
    pub guild_id: Id,
    pub guild_name: String,
    pub choice: GuildThemeChoice,
    pub dropdown_open: bool,
    pub error: Option<String>,
}

impl Root {
    /// Opens the Guild Theme NUX modal (`166998`) for `guild_id`.
    #[allow(dead_code)]
    pub(super) fn open_guild_theme_nux(&mut self, guild_id: Id, cx: &mut Context<Self>) {
        let guild_name = self
            .stores
            .guild(guild_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Server".to_owned());
        let saved_personal = crate::storage::get(STORAGE_KEY)
            .and_then(|v| v.as_str().map(|s| s == "personal"))
            .unwrap_or(false);
        self.guild_theme_nux_modal = Some(GuildThemeNuxModal {
            guild_id,
            guild_name,
            choice: if saved_personal {
                GuildThemeChoice::Personal
            } else {
                GuildThemeChoice::Guild
            },
            dropdown_open: false,
            error: None,
        });
        cx.notify();
    }

    /// Modal layer for Guild Theme NUX (`166998`).
    pub(super) fn guild_theme_nux_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.guild_theme_nux_modal.as_ref()?;
        let is_personal = modal.choice == GuildThemeChoice::Personal;
        let choice = modal.choice;
        let dropdown_open = modal.dropdown_open;

        // 16:9 preview banner comparing Server Theme gradient vs Personal default dark theme.
        let preview = div()
            .w_full()
            .h(px(160.))
            .rounded(px(10.))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .bg(if is_personal {
                linear_gradient(
                    135.,
                    linear_color_stop(rgba(0x1e1f22ff), 0.),
                    linear_color_stop(rgba(0x2b2d31ff), 1.),
                )
            } else {
                linear_gradient(
                    135.,
                    linear_color_stop(rgba(0x3b2667ff), 0.),
                    linear_color_stop(rgba(0x1d4e89ff), 1.),
                )
            })
            .p(px(14.))
            .flex()
            .gap(px(10.))
            // Mock channel sidebar
            .child(
                div()
                    .w(px(104.))
                    .h_full()
                    .rounded(px(8.))
                    .bg(rgba(0x00000044))
                    .p(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .truncate()
                            .child(modal.guild_name.clone()),
                    )
                    .child(
                        div()
                            .h(px(8.))
                            .w(px(72.))
                            .rounded(px(4.))
                            .bg(rgba(0xffffff33)),
                    )
                    .child(
                        div()
                            .h(px(8.))
                            .w(px(58.))
                            .rounded(px(4.))
                            .bg(rgba(0xffffff22)),
                    )
                    .child(
                        div()
                            .h(px(8.))
                            .w(px(66.))
                            .rounded(px(4.))
                            .bg(rgba(0xffffff22)),
                    ),
            )
            // Mock message area
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .rounded(px(8.))
                    .bg(rgba(0x00000033))
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(div().size(px(22.)).rounded_full().bg(if is_personal {
                                        rgba(0x5865f2ff)
                                    } else {
                                        rgba(0xeb459eff)
                                    }))
                                    .child(
                                        div()
                                            .h(px(9.))
                                            .w(px(120.))
                                            .rounded(px(4.))
                                            .bg(rgba(0xffffff44)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(div().size(px(22.)).rounded_full().bg(if is_personal {
                                        rgba(0x23a55aff)
                                    } else {
                                        rgba(0x57f287ff)
                                    }))
                                    .child(
                                        div()
                                            .h(px(9.))
                                            .w(px(150.))
                                            .rounded(px(4.))
                                            .bg(rgba(0xffffff33)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(22.))
                            .rounded(px(6.))
                            .bg(rgba(0x00000044))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .text_size(px(10.))
                            .text_color(rgba(0xffffffaa))
                            .child(if is_personal {
                                "Personal theme"
                            } else {
                                "Server custom theme"
                            }),
                    ),
            );

        let dropdown_menu = dropdown_open.then(|| {
            div()
                .mt(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .p(px(4.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(
                    [GuildThemeChoice::Guild, GuildThemeChoice::Personal]
                        .into_iter()
                        .enumerate()
                        .map(|(idx, opt)| {
                            let selected = opt == choice;
                            div()
                                .id(format!("guild-theme-opt-{idx}"))
                                .cursor_pointer()
                                .px(px(10.))
                                .py(px(8.))
                                .rounded(px(6.))
                                .when(selected, |d| d.bg(rgba(0x5865f233)))
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) = this.guild_theme_nux_modal.as_mut() {
                                        m.choice = opt;
                                        m.dropdown_open = false;
                                        m.error = None;
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(14.))
                                        .text_color(color("--text-strong"))
                                        .child(opt.label()),
                                )
                                .when(selected, |d| {
                                    d.child(icon(
                                        "circle_check",
                                        16.,
                                        "--control-primary-background-default",
                                    ))
                                })
                        }),
                )
        });

        Some(
            deferred(
                div()
                    .id("guild-theme-nux-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.guild_theme_nux_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("guild-theme-nux-modal")
                            .w(px(460.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            // Blue gradient header + preview graphic
                            .child(
                                div()
                                    .p(px(20.))
                                    .bg(linear_gradient(
                                        180.,
                                        linear_color_stop(rgba(0x5865f244), 0.),
                                        linear_color_stop(rgba(0x00000000), 1.),
                                    ))
                                    .flex()
                                    .flex_col()
                                    .gap(px(14.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_start()
                                            .justify_between()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(px(20.))
                                                            .line_height(px(24.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .child(dc_intl::t("Q9zFy9")),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .line_height(px(18.))
                                                            .text_color(color("--text-default"))
                                                            .child(dc_intl::t("XLpBLj")),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .id("guild-theme-nux-close")
                                                    .flex_none()
                                                    .size(px(30.))
                                                    .rounded(px(6.))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon(
                                                        "modal_close",
                                                        16.,
                                                        "--icon-subtle",
                                                    ))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.guild_theme_nux_modal = None;
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                    .child(preview),
                            )
                            // Body: Select + optional Personal warning + optional Error
                            .child(
                                div()
                                    .px(px(20.))
                                    .pb(px(18.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .id("guild-theme-select")
                                            .aria_label(dc_intl::t("Q7mm4g"))
                                            .cursor_pointer()
                                            .h(px(40.))
                                            .px(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-base-lowest"))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(8.))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(m) = this.guild_theme_nux_modal.as_mut()
                                                {
                                                    m.dropdown_open = !m.dropdown_open;
                                                }
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_size(px(14.))
                                                    .text_color(color("--text-strong"))
                                                    .truncate()
                                                    .child(choice.label()),
                                            )
                                            .child(icon(
                                                "shop_select_chevron",
                                                16.,
                                                "--icon-subtle",
                                            )),
                                    )
                                    .children(dropdown_menu)
                                    .when(is_personal, |col| {
                                        col.child(
                                            div()
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--status-warning"))
                                                .bg(rgba(0xf0b2321a))
                                                .px(px(12.))
                                                .py(px(10.))
                                                .flex()
                                                .items_start()
                                                .gap(px(8.))
                                                .child(icon("circle_info", 16., "--status-warning"))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_size(px(13.))
                                                        .line_height(px(18.))
                                                        .text_color(color("--text-default"))
                                                        .child(dc_intl::t("tTHQAy")),
                                                ),
                                        )
                                    })
                                    .children(modal.error.as_ref().map(|err| {
                                        div()
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--status-danger"))
                                            .bg(rgba(0xda373c1a))
                                            .px(px(12.))
                                            .py(px(10.))
                                            .text_size(px(13.))
                                            .text_color(color("--text-feedback-critical"))
                                            .child(err.clone())
                                    })),
                            )
                            // Footer: single button (Confirm primary OR Continue anyway secondary)
                            .child(
                                div()
                                    .px(px(20.))
                                    .py(px(14.))
                                    .bg(color("--background-base-lower"))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .justify_end()
                                    .child(
                                        div()
                                            .id("guild-theme-nux-submit")
                                            .cursor_pointer()
                                            .px(px(20.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(if is_personal {
                                                color("--control-secondary-background-default")
                                            } else {
                                                color("--control-primary-background-default")
                                            })
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(if is_personal {
                                                color("--control-secondary-text-default")
                                            } else {
                                                color("--white")
                                            })
                                            .child(choice.action_label())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let val = if choice == GuildThemeChoice::Personal {
                                                    "personal"
                                                } else {
                                                    "guild"
                                                };
                                                if !crate::storage::is_capture_run() {
                                                    crate::storage::set(
                                                        STORAGE_KEY,
                                                        serde_json::Value::from(val),
                                                    );
                                                }
                                                this.guild_theme_nux_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `guild-theme-nux [guild|personal|error|close]` (module 166998).
    #[cfg(feature = "capture")]
    pub(crate) fn script_guild_theme_nux(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let gid = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        match arg.trim() {
            "close" => {
                self.guild_theme_nux_modal = None;
                cx.notify();
                "guild-theme-nux: closed".to_owned()
            }
            "personal" => {
                self.open_guild_theme_nux(gid, cx);
                if let Some(m) = self.guild_theme_nux_modal.as_mut() {
                    m.choice = GuildThemeChoice::Personal;
                }
                cx.notify();
                "guild-theme-nux: personal".to_owned()
            }
            "error" => {
                self.open_guild_theme_nux(gid, cx);
                if let Some(m) = self.guild_theme_nux_modal.as_mut() {
                    m.error = Some(dc_intl::t("fEptJP").to_owned());
                }
                cx.notify();
                "guild-theme-nux: error".to_owned()
            }
            _ => {
                self.open_guild_theme_nux(gid, cx);
                if let Some(m) = self.guild_theme_nux_modal.as_mut() {
                    m.choice = GuildThemeChoice::Guild;
                }
                cx.notify();
                "guild-theme-nux: guild".to_owned()
            }
        }
    }
}
