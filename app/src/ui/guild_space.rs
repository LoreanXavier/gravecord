//! Guild Space ("Server Hub") page and 2-column widget editor (`GuildSpacePage`, module 930417).
//!
//! Renders the Server Hub header bar (`04IVMq` / `OBskVU`), unpublished banner, 240px banner
//! header with online/member/boost stats and edit controls, responsive 2fr/1fr widget layout,
//! Add Widget catalog modal, and floating bottom Save/Reset action bar.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};
use serde_json::json;

/// Module 77580 (`GuildRoomSelectorModal`, CSS `692772` / `538517.iX`): selectable Guild Room background.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GuildRoomBackground {
    #[default]
    Default = 0,
    LivingRoom2 = 1,
}

impl GuildRoomBackground {
    pub(crate) const ALL: [Self; 2] = [Self::Default, Self::LivingRoom2];

    pub(crate) fn number(self) -> u32 {
        match self {
            Self::Default => 1,
            Self::LivingRoom2 => 2,
        }
    }

    pub(crate) fn display_name(self) -> String {
        if dc_intl::has_key("3xb4VY") {
            dc_intl::format(
                "3xb4VY",
                &[("number", dc_intl::Arg::Num(f64::from(self.number())))],
            )
        } else {
            format!("Living Room {}", self.number())
        }
    }

    pub(crate) fn aspect_ratio(self) -> f32 {
        match self {
            Self::Default => 4384. / 3208.,
            Self::LivingRoom2 => 4384. / 3400.,
        }
    }
}

/// Module 77580 (`GuildRoomSelectorModal`) open modal state.
#[derive(Clone, Debug)]
pub(crate) struct GuildRoomSelectorModal {
    pub guild_id: Id,
    pub channel_id: Id,
    pub current_bg: GuildRoomBackground,
    pub selected_bg: GuildRoomBackground,
}

/// Module 77580 (`303227.AQ`): `POST /guilds/{guildId}/rooms/{channelId}/update` (`{ background }`).
pub(crate) fn update_guild_room_request(
    guild_id: Id,
    channel_id: Id,
    background: GuildRoomBackground,
) -> Request {
    Request::Rest {
        key: Some(format!("guild-room:update:{guild_id}:{channel_id}")),
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild_id}/rooms/{channel_id}/update"),
        body: Some(json!({ "background": background as u8 })),
    }
}

/// Which column of the 2fr/1fr Server Hub grid a widget occupies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GuildSpaceColumn {
    #[default]
    Main,
    Side,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GuildSpaceWidget {
    pub id: String,
    pub widget_type: String,
    pub title: String,
    pub summary: String,
    pub column: GuildSpaceColumn,
    pub position: usize,
    pub requires_hydration: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GuildSpaceCatalogItem {
    pub widget_type: String,
    pub title: String,
    pub description: String,
    pub boost_price: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct GuildSpaceState {
    pub guild_id: Id,
    pub guild_name: String,
    pub online_count: u32,
    pub member_count: u32,
    pub boost_count: u32,
    pub can_edit: bool,
    pub is_published: bool,
    pub editing: bool,
    pub catalog_open: bool,
    pub catalog_insert_col: GuildSpaceColumn,
    pub loading: bool,
    pub save_error: bool,
    pub has_custom_banner: bool,
    pub widgets: Vec<GuildSpaceWidget>,
    pub catalog: Vec<GuildSpaceCatalogItem>,
}

fn hub_copy(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        let s = dc_intl::t(key);
        if !s.trim().is_empty() {
            return s.to_owned();
        }
    }
    fallback.to_owned()
}

pub(crate) fn format_active_widgets(count: usize) -> String {
    if dc_intl::has_key("hpAg80") {
        let s = dc_intl::format("hpAg80", &[("panelCount", (count as f64).into())]);
        if !s.trim().is_empty() {
            return s;
        }
    }
    if count == 1 {
        "1 widget active".to_owned()
    } else {
        format!("{count} widgets active")
    }
}

pub(crate) fn format_boost_price(boost_price: u32) -> String {
    if boost_price == 0 {
        return hub_copy("Lx0P8k", "Free");
    }
    if dc_intl::has_key("8wD0Un") {
        let s = dc_intl::format("8wD0Un", &[("boostPrice", (boost_price as f64).into())]);
        if !s.trim().is_empty() {
            return s;
        }
    }
    format!("{boost_price} Boosts")
}

pub(crate) fn fetch_guild_space_request(guild_id: Id) -> Request {
    Request::Rest {
        key: Some(format!("guild-space:get:{guild_id}")),
        method: reqwest::Method::GET,
        route: format!("/guilds/{guild_id}/space"),
        body: None,
    }
}

pub(crate) fn save_guild_space_request(state: &GuildSpaceState) -> Request {
    let widgets: Vec<_> = state
        .widgets
        .iter()
        .map(|w| {
            json!({
                "id": w.id,
                "type": w.widget_type,
                "position": w.position,
                "config": {
                    "column": match w.column {
                        GuildSpaceColumn::Main => "main",
                        GuildSpaceColumn::Side => "side",
                    }
                }
            })
        })
        .collect();
    Request::Rest {
        key: Some(format!("guild-space:put:{}", state.guild_id)),
        method: reqwest::Method::PUT,
        route: format!("/guilds/{}/space", state.guild_id),
        body: Some(json!({
            "header": { "custom_banner": state.has_custom_banner },
            "widgets": widgets,
        })),
    }
}

pub(crate) fn publish_guild_space_request(guild_id: Id) -> Request {
    Request::Rest {
        key: Some(format!("guild-space:publish:{guild_id}")),
        method: reqwest::Method::PATCH,
        route: format!("/guilds/{guild_id}/space/settings"),
        body: Some(json!({ "publish_status": "PUBLISHED" })),
    }
}

pub(crate) fn fetch_guild_space_catalog_request(guild_id: Id) -> Request {
    Request::Rest {
        key: Some(format!("guild-space:catalog:{guild_id}")),
        method: reqwest::Method::GET,
        route: format!("/guilds/{guild_id}/space/widgets/catalog"),
        body: None,
    }
}

#[allow(dead_code)]
pub(crate) fn hydrate_guild_space_widgets_request(guild_id: Id, widget_ids: &[&str]) -> Request {
    let joined = widget_ids.join(",");
    Request::Rest {
        key: Some(format!("guild-space:hydrate:{guild_id}")),
        method: reqwest::Method::GET,
        route: format!("/guilds/{guild_id}/space/widgets/hydrate?widget_ids={joined}"),
        body: None,
    }
}

impl Root {
    #[allow(dead_code)]
    pub(crate) fn open_guild_space(&mut self, guild_id: Id, cx: &mut Context<Self>) {
        let (guild_name, boost_count) = self
            .stores
            .guild(guild_id)
            .map(|g| (g.name.clone(), g.premium_subscription_count as u32))
            .unwrap_or_else(|| (dc_intl::t("04IVMq").to_owned(), 0));
        let member_count = self
            .stores
            .member_counts
            .get_member_count(guild_id)
            .unwrap_or(1)
            .max(1) as u32;
        let online_count = self
            .stores
            .member_counts
            .get_online_count(guild_id)
            .unwrap_or((member_count as u64 / 4).max(1)) as u32;
        self.guild_space = Some(GuildSpaceState {
            guild_id,
            guild_name,
            online_count,
            member_count,
            boost_count,
            can_edit: true,
            is_published: true,
            editing: false,
            catalog_open: false,
            catalog_insert_col: GuildSpaceColumn::Main,
            loading: false,
            save_error: false,
            has_custom_banner: false,
            widgets: Vec::new(),
            catalog: default_catalog(),
        });
        let _ = self.requests.send(fetch_guild_space_request(guild_id));
        let _ = self
            .requests
            .send(fetch_guild_space_catalog_request(guild_id));
        cx.notify();
    }

    pub(crate) fn close_guild_space(&mut self) -> bool {
        if let Some(gs) = self.guild_space.as_mut()
            && gs.catalog_open
        {
            gs.catalog_open = false;
            return true;
        }
        self.guild_space.take().is_some()
    }

    pub(in crate::ui) fn guild_space_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.guild_space.as_ref()?;
        let editing = state.editing;
        let can_edit = state.can_edit;
        let guild_id = state.guild_id;

        let main_widgets: Vec<&GuildSpaceWidget> = state
            .widgets
            .iter()
            .filter(|w| w.column == GuildSpaceColumn::Main)
            .collect();
        let side_widgets: Vec<&GuildSpaceWidget> = state
            .widgets
            .iter()
            .filter(|w| w.column == GuildSpaceColumn::Side)
            .collect();

        let render_widget = |w: &GuildSpaceWidget| {
            let wid = w.id.clone();
            div()
                .id(SharedString::from(format!("guild-space-widget-{}", w.id)))
                .p(px(16.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(w.title.clone()),
                        )
                        .when(editing, |row| {
                            row.child(
                                div()
                                    .id(SharedString::from(format!(
                                        "guild-space-remove-widget-{wid}"
                                    )))
                                    .size(px(26.))
                                    .rounded(px(6.))
                                    .bg(color("--background-mod-subtle"))
                                    .hover(|s| s.bg(color("--background-mod-normal")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("ob_trash", 14., "--text-feedback-critical"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(gs) = this.guild_space.as_mut() {
                                            gs.widgets.retain(|item| item.id != wid);
                                        }
                                        cx.notify();
                                    })),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(w.summary.clone()),
                )
        };

        let empty_col = |col: GuildSpaceColumn, id_suffix: &'static str| {
            div()
                .id(SharedString::from(format!("guild-space-empty-{id_suffix}")))
                .min_h(px(160.))
                .p(px(24.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-mod-muted"))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .child(hub_copy("/OtM0k", "Add widgets to this column.")),
                )
                .when(editing, |box_el| {
                    box_el.child(
                        div()
                            .id(SharedString::from(format!(
                                "guild-space-add-col-{id_suffix}"
                            )))
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(icon("access_circle_plus", 14., "--white"))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(hub_copy("L8Xfoo", "Add Widget")),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(gs) = this.guild_space.as_mut() {
                                    gs.catalog_insert_col = col;
                                    gs.catalog_open = true;
                                }
                                cx.notify();
                            })),
                    )
                })
        };

        let catalog_overlay = state.catalog_open.then(|| {
            let insert_col = state.catalog_insert_col;
            let items: Vec<AnyElement> = state
                .catalog
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let item_clone = item.clone();
                    div()
                        .id(SharedString::from(format!("guild-space-catalog-{idx}")))
                        .w(px(260.))
                        .p(px(14.))
                        .rounded(px(10.))
                        .bg(color("--background-mod-muted"))
                        .border_1()
                        .border_color(color("--border-muted"))
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .cursor_pointer()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(gs) = this.guild_space.as_mut() {
                                let pos = gs
                                    .widgets
                                    .iter()
                                    .filter(|w| w.column == insert_col)
                                    .count();
                                gs.widgets.push(GuildSpaceWidget {
                                    id: format!("w_{}_{}", item_clone.widget_type, gs.widgets.len() + 1),
                                    widget_type: item_clone.widget_type.clone(),
                                    title: item_clone.title.clone(),
                                    summary: item_clone.description.clone(),
                                    column: insert_col,
                                    position: pos,
                                    requires_hydration: false,
                                });
                                gs.catalog_open = false;
                            }
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(item.title.clone()),
                                )
                                .child(
                                    div()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(6.))
                                        .bg(color("--background-surface-highest"))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-muted"))
                                        .child(format_boost_price(item.boost_price)),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(item.description.clone()),
                        )
                        .into_any_element()
                })
                .collect();

            div()
                .id("guild-space-catalog-backdrop")
                .absolute()
                .inset_0()
                .bg(rgba(0x00000099))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        if let Some(gs) = this.guild_space.as_mut() {
                            gs.catalog_open = false;
                        }
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .id("guild-space-catalog-modal")
                        .w(px(580.))
                        .p(px(20.))
                        .rounded(px(14.))
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(hub_copy("L8Xfoo", "Add Widget")),
                                )
                                .child(
                                    div()
                                        .id("guild-space-catalog-close")
                                        .size(px(28.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("modal_close", 18., "--icon-subtle"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(gs) = this.guild_space.as_mut() {
                                                gs.catalog_open = false;
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(color("--text-muted"))
                                .child(hub_copy(
                                    "N8nJ+T",
                                    "Click the widget you want to add to your Server Hub, and then drag and drop them around.",
                                )),
                        )
                        .child(div().flex().flex_wrap().gap(px(8.)).children(items)),
                )
        });

        Some(
            deferred(
                div()
                    .id("guild-space-page-overlay")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(color("--background-base-lower"))
                    .flex()
                    .flex_col()
                    // Top header bar (`742589.A`: HomeIcon + "Server Hub" `04IVMq` + Settings `OBskVU` + Close)
                    .child(
                        div()
                            .h(px(48.))
                            .flex_none()
                            .px(px(16.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(icon("home", 18., "--icon-strong"))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("04IVMq")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .when(can_edit, |row| {
                                        row.child(
                                            div()
                                                .id("guild-space-settings-btn")
                                                .h(px(30.))
                                                .px(px(10.))
                                                .rounded(px(6.))
                                                .bg(color("--background-mod-subtle"))
                                                .hover(|s| s.bg(color("--background-mod-normal")))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.))
                                                .aria_label(SharedString::from(dc_intl::t(
                                                    "OBskVU",
                                                )))
                                                .child(icon("settings", 15., "--icon-strong"))
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--text-strong"))
                                                        .child(dc_intl::t("3D5yo/")),
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.guild_space = None;
                                                    this.open_server_settings(guild_id, cx);
                                                })),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("guild-space-close-btn")
                                            .size(px(30.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.guild_space = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    // Scrollable content (`.content_QQ_HWZ`, max-width 1400px, p 16px, pb 96px)
                    .child(
                        div()
                            .id("guild-space-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .p(px(16.))
                            .pb(px(96.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(1160.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    // Loading indicator (`ZTNur7`) while `GET /guilds/{id}/space` is in flight
                                    .when(state.loading, |col| {
                                        col.child(
                                            div()
                                                .px(px(16.))
                                                .py(px(10.))
                                                .rounded(px(10.))
                                                .bg(color("--background-surface-high"))
                                                .text_size(px(13.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("ZTNur7")),
                                        )
                                    })
                                    // Unpublished notice bar (`872188.A`)
                                    .when(can_edit && !state.is_published, |col| {
                                        col.child(
                                            div()
                                                .px(px(16.))
                                                .py(px(10.))
                                                .rounded(px(10.))
                                                .bg(color("--background-surface-high"))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-default"))
                                                        .child(
                                                            "This Server Hub is unpublished. Publish it so members can see it.",
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .id("guild-space-publish-btn")
                                                        .h(px(30.))
                                                        .px(px(12.))
                                                        .rounded(px(6.))
                                                        .bg(color(
                                                            "--control-primary-background-default",
                                                        ))
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--white"))
                                                        .child(dc_intl::tr("Publish"))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(gs) =
                                                                this.guild_space.as_mut()
                                                            {
                                                                gs.is_published = true;
                                                                let _ = this.requests.send(
                                                                    publish_guild_space_request(
                                                                        gs.guild_id,
                                                                    ),
                                                                );
                                                            }
                                                            cx.notify();
                                                        })),
                                                ),
                                        )
                                    })
                                    // 240px Banner Header (`.header_ESecGU`)
                                    .child(
                                        div()
                                            .relative()
                                            .w_full()
                                            .h(px(240.))
                                            .rounded(px(16.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .overflow_hidden()
                                            .bg(linear_gradient(
                                                135.,
                                                linear_color_stop(rgba(0x3b44a8ff), 0.),
                                                linear_color_stop(rgba(0x1b1d29ff), 1.),
                                            ))
                                            .child(div().absolute().inset_0().bg(linear_gradient(
                                                0.,
                                                linear_color_stop(rgba(0x000000cc), 0.),
                                                linear_color_stop(rgba(0x00000000), 0.7),
                                            )))
                                            .child(
                                                div()
                                                    .relative()
                                                    .size_full()
                                                    .p(px(20.))
                                                    .flex()
                                                    .flex_col()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .justify_end()
                                                            .gap(px(8.))
                                                            .when(editing, |row| {
                                                                row.child(
                                                                    div()
                                                                        .id("guild-space-edit-banner")
                                                                        .h(px(32.))
                                                                        .px(px(10.))
                                                                        .rounded(px(8.))
                                                                        .bg(rgba(0x00000088))
                                                                        .cursor_pointer()
                                                                        .flex()
                                                                        .items_center()
                                                                        .gap(px(6.))
                                                                        .child(icon(
                                                                            "ob_image_plus",
                                                                            14.,
                                                                            "--white",
                                                                        ))
                                                                        .child(
                                                                            div()
                                                                                .text_size(px(12.))
                                                                                .font_weight(
                                                                                    FontWeight(
                                                                                        600.,
                                                                                    ),
                                                                                )
                                                                                .text_color(color(
                                                                                    "--white",
                                                                                ))
                                                                                .child(hub_copy(
                                                                                    "EN+0gW",
                                                                                    "Edit Banner Image",
                                                                                )),
                                                                        )
                                                                        .on_click(cx.listener(
                                                                            |this, _, _, cx| {
                                                                                if let Some(gs) =
                                                                                    this.guild_space
                                                                                        .as_mut()
                                                                                {
                                                                                    gs.has_custom_banner =
                                                                                        !gs.has_custom_banner;
                                                                                }
                                                                                cx.notify();
                                                                            },
                                                                        )),
                                                                )
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_end()
                                                            .justify_between()
                                                            .gap(px(16.))
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .gap(px(6.))
                                                                    .child(
                                                                        div()
                                                                            .text_size(px(24.))
                                                                            .font_weight(
                                                                                FontWeight(700.),
                                                                            )
                                                                            .text_color(color(
                                                                                "--white",
                                                                            ))
                                                                            .child(
                                                                                state
                                                                                    .guild_name
                                                                                    .clone(),
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .flex()
                                                                            .items_center()
                                                                            .gap(px(14.))
                                                                            .child(
                                                                                div()
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .gap(px(6.))
                                                                                    .child(
                                                                                        div()
                                                                                            .size(px(
                                                                                                8.,
                                                                                            ))
                                                                                            .rounded_full()
                                                                                            .bg(color(
                                                                                                "--icon-status-online",
                                                                                            )),
                                                                                    )
                                                                                    .child(
                                                                                        div()
                                                                                            .text_size(
                                                                                                px(13.),
                                                                                            )
                                                                                            .text_color(
                                                                                                rgba(0xffffffd9),
                                                                                            )
                                                                                            .child(format!(
                                                                                                "{} Online",
                                                                                                state.online_count
                                                                                            )),
                                                                                    ),
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .gap(px(6.))
                                                                                    .child(
                                                                                        div()
                                                                                            .size(px(
                                                                                                8.,
                                                                                            ))
                                                                                            .rounded_full()
                                                                                            .bg(rgba(0xffffffb8)),
                                                                                    )
                                                                                    .child(
                                                                                        div()
                                                                                            .text_size(
                                                                                                px(13.),
                                                                                            )
                                                                                            .text_color(
                                                                                                rgba(0xffffffd9),
                                                                                            )
                                                                                            .child(format!(
                                                                                                "{} Members",
                                                                                                state.member_count
                                                                                            )),
                                                                                    ),
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .text_size(px(
                                                                                        13.,
                                                                                    ))
                                                                                    .text_color(
                                                                                        rgba(0xffffffb8),
                                                                                    )
                                                                                    .child(format!(
                                                                                        "{} Boosts",
                                                                                        state.boost_count
                                                                                    )),
                                                                            ),
                                                                    ),
                                                            )
                                                            .when(can_edit && !editing, |row| {
                                                                row.child(
                                                                    div()
                                                                        .id("guild-space-enter-edit")
                                                                        .h(px(36.))
                                                                        .px(px(14.))
                                                                        .rounded(px(8.))
                                                                        .bg(color(
                                                                            "--background-surface-high",
                                                                        ))
                                                                        .hover(|s| {
                                                                            s.bg(color(
                                                                                "--background-surface-highest",
                                                                            ))
                                                                        })
                                                                        .cursor_pointer()
                                                                        .flex()
                                                                        .items_center()
                                                                        .gap(px(6.))
                                                                        .child(icon(
                                                                            "pm_edit",
                                                                            14.,
                                                                            "--icon-strong",
                                                                        ))
                                                                        .child(
                                                                            div()
                                                                                .text_size(px(13.))
                                                                                .font_weight(
                                                                                    FontWeight(
                                                                                        600.,
                                                                                    ),
                                                                                )
                                                                                .text_color(color(
                                                                                    "--text-strong",
                                                                                ))
                                                                                .child(hub_copy(
                                                                                    "KcOpCm",
                                                                                    "Edit Server Hub",
                                                                                )),
                                                                        )
                                                                        .on_click(cx.listener(
                                                                            |this, _, _, cx| {
                                                                                if let Some(gs) =
                                                                                    this.guild_space
                                                                                        .as_mut()
                                                                                {
                                                                                    gs.editing =
                                                                                        true;
                                                                                }
                                                                                cx.notify();
                                                                            },
                                                                        )),
                                                                )
                                                            }),
                                                    ),
                                            ),
                                    )
                                    // 2fr / 1fr Widget Grid (`.grid_zj0usA`)
                                    .child(
                                        div()
                                            .w_full()
                                            .flex()
                                            .items_start()
                                            .gap(px(16.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(460.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(12.))
                                                    .when(main_widgets.is_empty(), |col| {
                                                        col.child(empty_col(
                                                            GuildSpaceColumn::Main,
                                                            "main",
                                                        ))
                                                    })
                                                    .children(
                                                        main_widgets.iter().map(|w| render_widget(w)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .w(px(340.))
                                                    .flex_none()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(12.))
                                                    .when(side_widgets.is_empty(), |col| {
                                                        col.child(empty_col(
                                                            GuildSpaceColumn::Side,
                                                            "side",
                                                        ))
                                                    })
                                                    .children(
                                                        side_widgets.iter().map(|w| render_widget(w)),
                                                    ),
                                            ),
                                    ),
                            ),
                    )
                    // Floating bottom Save/Reset action bar (`.actionBar_8UbsPg`)
                    .when(editing, |page| {
                        page.child(
                            div()
                                .absolute()
                                .bottom(px(16.))
                                .left_0()
                                .right_0()
                                .flex()
                                .justify_center()
                                .child(
                                    div()
                                        .w(px(580.))
                                        .px(px(16.))
                                        .py(px(12.))
                                        .rounded(px(12.))
                                        .bg(color("--background-surface-high"))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(10.))
                                                .child(
                                                    div()
                                                        .id("guild-space-bar-add-widget")
                                                        .h(px(32.))
                                                        .px(px(12.))
                                                        .rounded(px(8.))
                                                        .bg(color("--background-mod-subtle"))
                                                        .hover(|s| {
                                                            s.bg(color("--background-mod-normal"))
                                                        })
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(6.))
                                                        .child(icon(
                                                            "access_circle_plus",
                                                            14.,
                                                            "--icon-strong",
                                                        ))
                                                        .child(
                                                            div()
                                                                .text_size(px(13.))
                                                                .font_weight(FontWeight(600.))
                                                                .text_color(color("--text-strong"))
                                                                .child(hub_copy(
                                                                    "L8Xfoo",
                                                                    "Add Widget",
                                                                )),
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(gs) =
                                                                this.guild_space.as_mut()
                                                            {
                                                                gs.catalog_insert_col =
                                                                    GuildSpaceColumn::Main;
                                                                gs.catalog_open = true;
                                                            }
                                                            cx.notify();
                                                        })),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(if state.save_error {
                                                            color("--text-feedback-critical")
                                                        } else {
                                                            color("--text-muted")
                                                        })
                                                        .child(if state.save_error {
                                                            hub_copy(
                                                                "HmFYc5",
                                                                "Your changes couldn't be saved. Try again.",
                                                            )
                                                        } else {
                                                            format_active_widgets(
                                                                state.widgets.len(),
                                                            )
                                                        }),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .id("guild-space-cancel-edit")
                                                        .h(px(32.))
                                                        .px(px(12.))
                                                        .rounded(px(8.))
                                                        .cursor_pointer()
                                                        .hover(|s| {
                                                            s.bg(color("--background-mod-subtle"))
                                                        })
                                                        .flex()
                                                        .items_center()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--text-default"))
                                                        .child(dc_intl::t("ETE/oC"))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(gs) =
                                                                this.guild_space.as_mut()
                                                            {
                                                                gs.editing = false;
                                                                gs.catalog_open = false;
                                                            }
                                                            cx.notify();
                                                        })),
                                                )
                                                .child(
                                                    div()
                                                        .id("guild-space-save-edit")
                                                        .h(px(32.))
                                                        .px(px(14.))
                                                        .rounded(px(8.))
                                                        .bg(color(
                                                            "--control-primary-background-default",
                                                        ))
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--white"))
                                                        .child(dc_intl::t("R3BPH+"))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(gs) =
                                                                this.guild_space.as_mut()
                                                            {
                                                                let req =
                                                                    save_guild_space_request(gs);
                                                                let _ = this.requests.send(req);
                                                                gs.editing = false;
                                                                gs.save_error = false;
                                                            }
                                                            cx.notify();
                                                        })),
                                                ),
                                        ),
                                ),
                        )
                    })
                    .children(catalog_overlay),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_guild_space(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.guild_space = None;
            cx.notify();
            return "guild space closed".to_owned();
        }
        let edit = arg.trim() == "edit";
        self.guild_space = Some(GuildSpaceState {
            guild_id: self.guild.unwrap_or(1),
            guild_name: "gravecord fixture".to_owned(),
            online_count: 148,
            member_count: 1240,
            boost_count: 14,
            can_edit: true,
            is_published: !edit,
            editing: edit,
            catalog_open: false,
            catalog_insert_col: GuildSpaceColumn::Main,
            loading: false,
            save_error: false,
            has_custom_banner: true,
            widgets: vec![
                GuildSpaceWidget {
                    id: "w1".into(),
                    widget_type: "announcements".into(),
                    title: "Latest Announcements".into(),
                    summary: "Pinned server news and changelog updates from #announcements.".into(),
                    column: GuildSpaceColumn::Main,
                    position: 0,
                    requires_hydration: false,
                },
                GuildSpaceWidget {
                    id: "w2".into(),
                    widget_type: "game_leaderboard".into(),
                    title: "Weekly Game Leaderboard".into(),
                    summary: "Top players across voice channels this week.".into(),
                    column: GuildSpaceColumn::Main,
                    position: 1,
                    requires_hydration: true,
                },
                GuildSpaceWidget {
                    id: "w3".into(),
                    widget_type: "events".into(),
                    title: "Upcoming Events".into(),
                    summary: "Friday Community Raid Night • 20:00 UTC".into(),
                    column: GuildSpaceColumn::Side,
                    position: 0,
                    requires_hydration: false,
                },
            ],
            catalog: default_catalog(),
        });
        cx.notify();
        format!("guild space opened (edit={edit})")
    }
}

fn default_catalog() -> Vec<GuildSpaceCatalogItem> {
    vec![
        GuildSpaceCatalogItem {
            widget_type: "announcements".into(),
            title: "Announcements".into(),
            description: "Highlight recent posts from your announcement channels.".into(),
            boost_price: 0,
        },
        GuildSpaceCatalogItem {
            widget_type: "events".into(),
            title: "Scheduled Events".into(),
            description: "Show upcoming server events and RSVP counts.".into(),
            boost_price: 0,
        },
        GuildSpaceCatalogItem {
            widget_type: "game_leaderboard".into(),
            title: "Game Leaderboard".into(),
            description: "Showcase top active players in the server.".into(),
            boost_price: 2,
        },
        GuildSpaceCatalogItem {
            widget_type: "game_shop".into(),
            title: "Server Game Shop".into(),
            description: "Feature linked game drops and server perks.".into(),
            boost_price: 0,
        },
    ]
}

impl Root {
    /// Module 77580 (`GuildRoomSelectorModal`, CSS `692772`): modal choosing between Living Room 1
    /// (`DEFAULT = 0`) and Living Room 2 (`LIVING_ROOM_2 = 1`) for a Guild Room voice space.
    pub(super) fn guild_room_selector_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.guild_room_selector.as_ref()?;
        let is_unchanged = modal.current_bg == modal.selected_bg;
        let guild_id = modal.guild_id;
        let channel_id = modal.channel_id;
        let selected_bg = modal.selected_bg;
        let title = hub_copy("kmzWUc", "Choose a Room");
        let subtitle = hub_copy("Rqmi9/", "Select the room you'd like your space to use.");

        let cards = GuildRoomBackground::ALL.into_iter().map(|bg| {
            let is_selected = bg == selected_bg;
            let (grad_start, grad_end) = match bg {
                GuildRoomBackground::Default => (rgba(0x4b3f72ff), rgba(0x232036ff)),
                GuildRoomBackground::LivingRoom2 => (rgba(0x2d5a6bff), rgba(0x1b2c38ff)),
            };
            let thumb_h = (200. / bg.aspect_ratio()).round();
            div()
                .id(SharedString::from(format!("guild-room-opt-{}", bg as u8)))
                .flex_1()
                .min_w(px(0.))
                .p(px(8.))
                .rounded(px(8.))
                .bg(color("--background-mod-muted"))
                .hover(|s| s.bg(color("--background-mod-normal")))
                .when(is_selected, |d| {
                    d.border_2().border_color(color("--background-brand"))
                })
                .when(!is_selected, |d| {
                    d.border_1().border_color(color("--border-muted"))
                })
                .cursor_pointer()
                .flex()
                .flex_col()
                .justify_between()
                .gap(px(8.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = this.guild_room_selector.as_mut() {
                        m.selected_bg = bg;
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .w_full()
                        .h(px(thumb_h))
                        .rounded(px(6.))
                        .bg(linear_gradient(
                            135.,
                            linear_color_stop(grad_start, 0.),
                            linear_color_stop(grad_end, 1.),
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("home_shop", 28., "--white")),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(bg.display_name()),
                )
        });

        Some(
            deferred(
                div()
                    .id("guild-room-selector-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.guild_room_selector = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("guild-room-selector-modal")
                            .w(px(480.))
                            .max_w(px(480.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(
                                div()
                                    .px(px(20.))
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(18.))
                                                    .line_height(px(24.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(title),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(20.))
                                                    .text_color(color("--text-muted"))
                                                    .child(subtitle),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("guild-room-selector-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.guild_room_selector = None;
                                                cx.notify();
                                            }))
                                            .child(icon("close", 16., "--icon-subtle")),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(20.))
                                    .pb(px(20.))
                                    .flex()
                                    .items_stretch()
                                    .gap(px(8.))
                                    .children(cards),
                            )
                            .child(
                                div()
                                    .px(px(20.))
                                    .py(px(14.))
                                    .bg(color("--background-base-lower"))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("guild-room-selector-cancel")
                                            .h(px(36.))
                                            .px(px(14.))
                                            .rounded(px(8.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("ETE/oC"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.guild_room_selector = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("guild-room-selector-apply")
                                            .h(px(36.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .when(is_unchanged, |d| d.opacity(0.5))
                                            .when(!is_unchanged, |d| d.cursor_pointer())
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("1Qm822"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if is_unchanged {
                                                    return;
                                                }
                                                let _ =
                                                    this.requests.send(update_guild_room_request(
                                                        guild_id,
                                                        channel_id,
                                                        selected_bg,
                                                    ));
                                                this.guild_room_selector = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_guild_room_selector(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.guild_room_selector = None;
            cx.notify();
            return "guild-room-selector: closed".to_owned();
        }
        let selected_bg = if arg.trim() == "room2" {
            GuildRoomBackground::LivingRoom2
        } else {
            GuildRoomBackground::Default
        };
        self.guild_room_selector = Some(GuildRoomSelectorModal {
            guild_id: self.guild.unwrap_or(100),
            channel_id: self.channel.unwrap_or(200),
            current_bg: GuildRoomBackground::Default,
            selected_bg,
        });
        cx.notify();
        format!("guild-room-selector: opened ({selected_bg:?})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_space_930417_intl_and_requests() {
        for key in ["04IVMq", "OBskVU", "3D5yo/", "ZTNur7", "F8FvUy"] {
            assert!(dc_intl::has_key(key), "missing 930417 key {key}");
        }
        assert!(!format_active_widgets(1).is_empty());
        assert!(!format_active_widgets(3).is_empty());
        assert!(!format_boost_price(0).is_empty());
        assert!(format_boost_price(2).contains('2'));

        let state = GuildSpaceState {
            guild_id: 42,
            ..Default::default()
        };
        assert!(matches!(
            fetch_guild_space_request(42),
            Request::Rest { route, .. } if route == "/guilds/42/space"
        ));
        assert!(matches!(
            save_guild_space_request(&state),
            Request::Rest { route, .. } if route == "/guilds/42/space"
        ));
        assert!(matches!(
            publish_guild_space_request(42),
            Request::Rest { route, .. } if route == "/guilds/42/space/settings"
        ));
        assert!(matches!(
            fetch_guild_space_catalog_request(42),
            Request::Rest { route, .. } if route == "/guilds/42/space/widgets/catalog"
        ));
        assert!(matches!(
            hydrate_guild_space_widgets_request(42, &["w1", "w2"]),
            Request::Rest { route, .. } if route == "/guilds/42/space/widgets/hydrate?widget_ids=w1,w2"
        ));
    }

    #[test]
    fn guild_room_selector_77580_request_and_intl() {
        for key in ["kmzWUc", "Rqmi9/", "3xb4VY", "ETE/oC", "1Qm822", "iufib1"] {
            assert!(dc_intl::has_key(key), "missing 77580 key {key}");
        }
        assert!(GuildRoomBackground::Default.display_name().contains('1'));
        assert!(
            GuildRoomBackground::LivingRoom2
                .display_name()
                .contains('2')
        );
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = update_guild_room_request(11, 22, GuildRoomBackground::LivingRoom2)
        else {
            panic!("expected Rest");
        };
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/guilds/11/rooms/22/update");
        assert_eq!(body.unwrap()["background"], 1);
    }
}
