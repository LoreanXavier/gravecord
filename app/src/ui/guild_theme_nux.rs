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
                                .flex_none()
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
                                            .flex()
                                            .flex_col()
                                            .gap(px(12.))
                                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                                if let Some(m) = this.guild_theme_nux_modal.as_mut()
                                                    && m.dropdown_open
                                                {
                                                    m.dropdown_open = false;
                                                    cx.notify();
                                                }
                                            }))
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
                                                        if let Some(m) =
                                                            this.guild_theme_nux_modal.as_mut()
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
                                            .children(dropdown_menu),
                                    )
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
}

/// Preset definitions for Server Theme Power-Up Editor (module 214103 / 476931 / 378292):
/// `(id, intl_key, start_hex, end_hex, default_intensity)`.
const THEME_PRESETS: [(u8, &str, u32, u32, u8); 10] = [
    (1, "+k8Q/o", 0x69426aff, 0x111731ff, 100),  // Twilight
    (2, "tXRcxU", 0x5359adff, 0x121238ff, 100),  // Denim
    (3, "qn90j5", 0x245b92ff, 0x141d40ff, 50),   // Ocean
    (4, "XyrxOp", 0x533d9eff, 0x1a1035ff, 50),   // Blurple
    (5, "x3h/OV", 0x5e4c85ff, 0x1e1740ff, 50),   // Obsidian
    (6, "tTJY6S", 0x8a3f7fff, 0x2c0d25ff, 50),   // Plum
    (7, "S8/OZA", 0x9b2c2cff, 0x2a0c0cff, 100),  // Fire
    (8, "nCbxEN", 0x6c523dff, 0x241912ff, 100),  // Gold Dust
    (9, "TiGQro", 0x58694eff, 0x222a1cff, 100),  // Moss
    (10, "6ZeQDd", 0x297071ff, 0x18203fff, 100), // Jade
];

/// Server Theme Power-Up Editor right panel state (module 214103: `/6NbRv`, `zry4l5`, `HkvkHP`,
/// `jSrqR2`, `H8idcP`, `0LL6FX`, `Dk+OWY`, `5vlJkY`, `uSL2Gy`, `xlXkaL`, `c9MBEH`, `yBZMsQ`,
/// `13/7kX`, `1Qm822`, `cY+Oob`, `ETE/oC`, `fEptJP`; `PATCH /guilds/{id}/theme`).
#[derive(Clone, Debug)]
pub(crate) struct GuildThemeEditor {
    pub guild_id: Id,
    pub preview_light: bool,
    pub hue_deg: u16,
    pub tone_pct: u8,
    pub intensity: u8,
    pub selected_preset: Option<u8>,
    pub has_changes: bool,
    pub confirm_apply: bool,
    pub error: Option<String>,
}

impl GuildThemeEditor {
    pub(crate) fn new(guild_id: Id) -> Self {
        Self {
            guild_id,
            preview_light: false,
            hue_deg: 235,
            tone_pct: 25,
            intensity: 74,
            selected_preset: Some(4),
            has_changes: true,
            confirm_apply: false,
            error: None,
        }
    }
}

impl Root {
    /// Renders the Server Theme Power-Up Editor right panel + "Apply Server Theme?" confirm dialog (module 214103).
    pub(in crate::ui) fn guild_theme_editor_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ed = self.guild_theme_editor.as_ref()?;
        let gid = ed.guild_id;
        let intensity = ed.intensity;
        let hue = ed.hue_deg;
        let tone = ed.tone_pct;
        let selected_preset = ed.selected_preset;

        let preset_grid: Vec<AnyElement> = THEME_PRESETS
            .iter()
            .map(|&(pid, key, c1, c2, default_mix)| {
                let active = selected_preset == Some(pid);
                div()
                    .id(gpui::SharedString::from(format!(
                        "guild-theme-preset-{pid}"
                    )))
                    .w(px(56.))
                    .h(px(48.))
                    .rounded(px(8.))
                    .bg(gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(rgba(c1), 0.),
                        gpui::linear_color_stop(rgba(c2), 1.),
                    ))
                    .border_2()
                    .border_color(if active {
                        color("--white")
                    } else {
                        color("--border-subtle")
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_end()
                    .pb(px(4.))
                    .child(
                        div()
                            .text_size(px(9.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t(key)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(e) = this.guild_theme_editor.as_mut() {
                            e.selected_preset = Some(pid);
                            e.intensity = default_mix;
                            e.has_changes = true;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        let panel = div()
            .id("guild-theme-editor-panel")
            .occlude()
            .w(px(340.))
            .h_full()
            .bg(color("--background-surface-high"))
            .border_l_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            // Scrollable body
            .child(
                div()
                    .id("guild-theme-editor-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    // Heading `/6NbRv` ("Server Theme")
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("/6NbRv")),
                    )
                    // Preview Theme (`zry4l5` / `HkvkHP`) + Dark/Light pill toggle (`Dk+OWY` / `5vlJkY`)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("zry4l5")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("HkvkHP")),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .p(px(3.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .id("guild-theme-dark")
                                            .flex_1()
                                            .h(px(30.))
                                            .rounded(px(6.))
                                            .bg(if !ed.preview_light {
                                                color("--background-mod-subtle")
                                            } else {
                                                rgba(0x00000000)
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("Dk+OWY"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(e) = this.guild_theme_editor.as_mut() {
                                                    e.preview_light = false;
                                                }
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("guild-theme-light")
                                            .flex_1()
                                            .h(px(30.))
                                            .rounded(px(6.))
                                            .bg(if ed.preview_light {
                                                color("--background-mod-subtle")
                                            } else {
                                                rgba(0x00000000)
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("5vlJkY"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(e) = this.guild_theme_editor.as_mut() {
                                                    e.preview_light = true;
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    // Colors (`uSL2Gy`): Hue + Tone gradient tracks
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("uSL2Gy")),
                            )
                            .child(
                                div()
                                    .id("guild-theme-hue-bar")
                                    .relative()
                                    .w_full()
                                    .h(px(14.))
                                    .rounded_full()
                                    .bg(gpui::linear_gradient(
                                        90.,
                                        gpui::linear_color_stop(rgba(0xff3b30ff), 0.),
                                        gpui::linear_color_stop(rgba(0x5865f2ff), 1.),
                                    ))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(e) = this.guild_theme_editor.as_mut() {
                                            e.hue_deg = (e.hue_deg + 36) % 360;
                                            e.selected_preset = None;
                                            e.has_changes = true;
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(-2.))
                                            .left(gpui::relative(
                                                (hue as f32 / 360.).clamp(0.02, 0.94),
                                            ))
                                            .size(px(18.))
                                            .rounded_full()
                                            .bg(color("--white"))
                                            .border_2()
                                            .border_color(rgba(0x00000066)),
                                    ),
                            )
                            .child(
                                div()
                                    .id("guild-theme-tone-bar")
                                    .relative()
                                    .w_full()
                                    .h(px(14.))
                                    .rounded_full()
                                    .bg(gpui::linear_gradient(
                                        90.,
                                        gpui::linear_color_stop(rgba(0x8ea1e1ff), 0.),
                                        gpui::linear_color_stop(rgba(0x141724ff), 1.),
                                    ))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(e) = this.guild_theme_editor.as_mut() {
                                            e.tone_pct = ((e.tone_pct + 15) % 60) + 15;
                                            e.selected_preset = None;
                                            e.has_changes = true;
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(-2.))
                                            .left(gpui::relative(
                                                ((tone as f32 - 15.) / 60.).clamp(0.02, 0.94),
                                            ))
                                            .size(px(18.))
                                            .rounded_full()
                                            .bg(color("--white"))
                                            .border_2()
                                            .border_color(rgba(0x00000066)),
                                    ),
                            ),
                    )
                    // Color Intensity (`xlXkaL`)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("xlXkaL")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(format!("{intensity}%")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("guild-theme-intensity-bar")
                                    .relative()
                                    .w_full()
                                    .h(px(8.))
                                    .rounded_full()
                                    .bg(color("--background-mod-strong"))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(e) = this.guild_theme_editor.as_mut() {
                                            e.intensity = (e.intensity + 10) % 101;
                                            e.has_changes = true;
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .h_full()
                                            .w(gpui::relative(intensity as f32 / 100.))
                                            .rounded_full()
                                            .bg(color("--control-primary-background-default")),
                                    ),
                            ),
                    )
                    // Presets (`jSrqR2`)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("jSrqR2")),
                            )
                            .child(div().flex().flex_wrap().gap(px(6.)).children(preset_grid)),
                    )
                    // Surprise Me! (`c9MBEH`) + Reset (`yBZMsQ`)
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("guild-theme-surprise")
                                    .flex_1()
                                    .h(px(34.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("c9MBEH"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(e) = this.guild_theme_editor.as_mut() {
                                            e.hue_deg = (e.hue_deg + 73) % 360;
                                            e.intensity = 75;
                                            e.selected_preset = None;
                                            e.has_changes = true;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("guild-theme-reset")
                                    .flex_1()
                                    .h(px(34.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .when(!ed.has_changes, |b| b.opacity(0.5))
                                    .when(ed.has_changes, |b| {
                                        b.cursor_pointer()
                                            .hover(|s| {
                                                s.bg(color("--control-secondary-background-hover"))
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(e) = this.guild_theme_editor.as_mut() {
                                                    *e = GuildThemeEditor::new(gid);
                                                    e.has_changes = false;
                                                }
                                                cx.notify();
                                            }))
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("yBZMsQ")),
                            ),
                    )
                    .when_some(ed.error.clone(), |col, err| {
                        col.child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-feedback-critical"))
                                .child(err),
                        )
                    }),
            )
            // Pinned footer: Back (`13/7kX`) + Apply (`1Qm822`)
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .bg(color("--modal-footer-background"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("guild-theme-back")
                            .flex_1()
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("13/7kX"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.guild_theme_editor = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("guild-theme-apply")
                            .flex_1()
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .when(!ed.has_changes, |b| b.opacity(0.5))
                            .when(ed.has_changes, |b| {
                                b.cursor_pointer()
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(e) = this.guild_theme_editor.as_mut() {
                                            e.confirm_apply = true;
                                        }
                                        cx.notify();
                                    }))
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("1Qm822")),
                    ),
            );

        // Optional "Apply Server Theme?" confirm dialog (`H8idcP`, `0LL6FX`, `ETE/oC`, `cY+Oob`)
        let confirm_modal = ed.confirm_apply.then(|| {
            let body_json = if let Some(pid) = selected_preset {
                serde_json::json!({
                    "preset_id": pid,
                    "custom_user_theme_settings": serde_json::Value::Null,
                    "enabled": true,
                })
            } else {
                serde_json::json!({
                    "preset_id": serde_json::Value::Null,
                    "custom_user_theme_settings": {
                        "colors": ["#5865F2"],
                        "gradient_color_stops": [],
                        "gradient_angle": 0,
                        "base_mix": intensity,
                    },
                    "enabled": true,
                })
            };
            div()
                .id("guild-theme-confirm-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("guild-theme-confirm-dialog")
                        .w(px(420.))
                        .rounded(px(12.))
                        .bg(color("--modal-background"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .p(px(20.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("H8idcP")),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t("0LL6FX")),
                                ),
                        )
                        .child(
                            div()
                                .px(px(20.))
                                .py(px(14.))
                                .bg(color("--modal-footer-background"))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .id("guild-theme-confirm-cancel")
                                        .h(px(36.))
                                        .px(px(16.))
                                        .rounded(px(8.))
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-secondary-background-hover"))
                                        })
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("ETE/oC"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(e) = this.guild_theme_editor.as_mut() {
                                                e.confirm_apply = false;
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("guild-theme-confirm-ok")
                                        .h(px(36.))
                                        .px(px(16.))
                                        .rounded(px(8.))
                                        .bg(color("--control-primary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-primary-background-hover"))
                                        })
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::t("cY+Oob"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let _ = this.requests.send(crate::net::Request::Rest {
                                                key: Some(format!("guild:theme:{gid}")),
                                                method: reqwest::Method::PATCH,
                                                route: format!("/guilds/{gid}/theme"),
                                                body: Some(body_json.clone()),
                                            });
                                            this.guild_theme_editor = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
        });

        Some(
            gpui::deferred(
                div()
                    .id("guild-theme-editor-layer")
                    .absolute()
                    .inset_0()
                    .flex()
                    .justify_end()
                    .child(panel)
                    .children(confirm_modal),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }

    /// Capture harness: `guild-theme-nux [guild|personal|error|editor|editor-confirm|close]` (166998 & 214103).
    #[cfg(feature = "capture")]
    pub(crate) fn script_guild_theme_nux(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let gid = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        match arg.trim() {
            "close" => {
                self.guild_theme_nux_modal = None;
                self.guild_theme_editor = None;
                cx.notify();
                "guild-theme-nux: closed".to_owned()
            }
            "editor" | "editor-confirm" => {
                self.guild_theme_nux_modal = None;
                let mut ed = GuildThemeEditor::new(gid);
                ed.confirm_apply = arg.trim() == "editor-confirm";
                self.guild_theme_editor = Some(ed);
                cx.notify();
                format!("guild-theme-nux: {}", arg.trim())
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
