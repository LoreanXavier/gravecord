//! Guild Settings Picker / Creator Onboarding Modal (module 623509).
//!
//! Opened from `Route::PickGuildSettings` (`deep_link_route.rs`) or creator onboarding links.
//! Supports all three official feature variants:
//! - `"server-subscriptions-onboarding"`: `KzCF/6` ("Server Subscriptions"), `xMW8FH`
//! - `"server-subscriptions-create-tier-from-template"`: `aTFQKh` ("Welcome to Server Subscriptions"), `oTbFQg`
//! - Default settings picker: `V42OaH` ("Select a Server"), `7dJ16X`, plus optional Create a Server
//!   section (`anOisx`, `B44MTm`) calling `open_add_server`.

use super::{Root, color, icon};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PickGuildFeature {
    Default,
    ServerSubscriptionsOnboarding,
    ServerSubscriptionsTemplate,
    /// Module 208652: "Create a Server Tag" scrollable guild picker (`xO5QzM`, `h+7Yx3`, `zRl6XR`).
    CreateServerTag,
}

impl PickGuildFeature {
    #[allow(dead_code)]
    pub(crate) fn from_str_opt(raw: Option<&str>) -> Self {
        match raw {
            Some("server-subscriptions-onboarding") => Self::ServerSubscriptionsOnboarding,
            Some("server-subscriptions-create-tier-from-template") => {
                Self::ServerSubscriptionsTemplate
            }
            Some("create-server-tag") => Self::CreateServerTag,
            _ => Self::Default,
        }
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::ServerSubscriptionsOnboarding => dc_intl::t("KzCF/6"),
            Self::ServerSubscriptionsTemplate => dc_intl::t("aTFQKh"),
            Self::CreateServerTag => dc_intl::t("xO5QzM"),
            Self::Default => dc_intl::t("V42OaH"),
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::ServerSubscriptionsOnboarding => dc_intl::t("xMW8FH"),
            Self::ServerSubscriptionsTemplate => dc_intl::t("oTbFQg"),
            Self::CreateServerTag => dc_intl::t("h+7Yx3"),
            Self::Default => dc_intl::t("7dJ16X"),
        }
    }

    pub(crate) fn can_create_guild(self) -> bool {
        matches!(self, Self::Default)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PickGuildSettingsModal {
    pub feature: PickGuildFeature,
    pub section: Option<String>,
    pub selected_guild: Option<Id>,
    pub dropdown_open: bool,
}

impl Root {
    /// Opens the Guild Settings Picker / Creator Onboarding modal (`623509`).
    pub(super) fn open_pick_guild_settings(
        &mut self,
        section: Option<String>,
        feature: PickGuildFeature,
        cx: &mut Context<Self>,
    ) {
        let first = self.eligible_picker_guilds(feature).first().map(|g| g.0);
        self.pick_guild_settings_modal = Some(PickGuildSettingsModal {
            feature,
            section,
            selected_guild: first,
            dropdown_open: false,
        });
        cx.notify();
    }

    fn eligible_picker_guilds(&self, feature: PickGuildFeature) -> Vec<(Id, String)> {
        let me = self.stores.me().map(|u| u.id);
        self.stores
            .guilds
            .iter()
            .filter(|g| match feature {
                PickGuildFeature::ServerSubscriptionsOnboarding => {
                    me.is_none_or(|id| g.owner_id == Some(id))
                }
                PickGuildFeature::ServerSubscriptionsTemplate => {
                    me.is_none_or(|id| g.owner_id == Some(id))
                        || g.features.iter().any(|f| f == "ROLE_SUBSCRIPTIONS_ENABLED")
                }
                PickGuildFeature::Default | PickGuildFeature::CreateServerTag => true,
            })
            .map(|g| (g.id, g.name.clone()))
            .collect()
    }

    /// Modal layer for Guild Settings Picker / Creator Onboarding (`623509`) and
    /// "Create a Server Tag" guild picker (`208652`: `xO5QzM`, `h+7Yx3`, `zRl6XR`).
    pub(super) fn pick_guild_settings_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.pick_guild_settings_modal.as_ref()?;
        if modal.feature == PickGuildFeature::CreateServerTag {
            let rows: Vec<(Id, String, String, Option<u64>)> = self
                .stores
                .guilds
                .iter()
                .map(|g| {
                    let acronym = g.acronym();
                    let members = self.stores.member_counts.get_member_count(g.id);
                    (g.id, g.name.clone(), acronym, members)
                })
                .collect();
            let list = div()
                .id("create-server-tag-guild-list")
                .max_h(px(400.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(rows.into_iter().enumerate().map(
                    |(idx, (gid, name, acronym, member_count))| {
                        let icon_img = self.images.get(&format!("guild:{gid}")).cloned();
                        let member_text = member_count.map(|c| {
                            dc_intl::format("zRl6XR", &[("count", dc_intl::Arg::Num(c as f64))])
                        });
                        // `R[MEDIUM]` of module 682163: 40px tile acronym font size by length.
                        const SIZES: [f32; 7] = [16., 16., 14., 14., 12., 10., 8.];
                        let acronym_size = SIZES[acronym.chars().count().min(SIZES.len() - 1)];
                        div()
                            .id(format!("create-server-tag-guild-{idx}"))
                            .p(px(8.))
                            .rounded(px(16.))
                            .hover(|s| s.bg(color("--background-mod-normal")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pick_guild_settings_modal = None;
                                this.select(gid, None);
                                this.open_boost_page(cx);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(40.))
                                    .rounded(px(12.))
                                    .bg(color("--background-mod-subtle"))
                                    .overflow_hidden()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(if let Some(img_handle) = icon_img {
                                        gpui::img(img_handle)
                                            .size(px(40.))
                                            .rounded(px(12.))
                                            .into_any_element()
                                    } else {
                                        div()
                                            .text_size(px(acronym_size))
                                            .line_height(px(acronym_size * 1.2))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .whitespace_nowrap()
                                            .child(acronym)
                                            .into_any_element()
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-subtle"))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(name),
                                    )
                                    .when_some(member_text, |col, m| {
                                        col.child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(16.))
                                                .text_color(color("--text-subtle"))
                                                .child(m),
                                        )
                                    }),
                            )
                    },
                ));

            let dialog = div()
                .id("create-server-tag-modal")
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .w(px(460.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(12.))
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
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .line_height(px(26.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("xO5QzM")),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("h+7Yx3")),
                                ),
                        )
                        .child(
                            div()
                                .id("create-server-tag-close")
                                .flex_none()
                                .size(px(32.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("modal_close", 18., "--icon-subtle"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pick_guild_settings_modal = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(div().px(px(16.)).pb(px(16.)).child(list));

            return Some(
                deferred(
                    div()
                        .id("create-server-tag-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b8))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.pick_guild_settings_modal = None;
                                cx.notify();
                            }),
                        )
                        .child(dialog),
                )
                .with_priority(96)
                .into_any_element(),
            );
        }
        let guilds = self.eligible_picker_guilds(modal.feature);
        let selected = modal.selected_guild;
        let selected_label = selected
            .and_then(|gid| {
                guilds
                    .iter()
                    .find(|(id, _)| *id == gid)
                    .map(|(_, n)| n.clone())
            })
            .unwrap_or_else(|| dc_intl::t("etZ9tX").to_owned());
        let section = modal.section.clone();
        let can_create = modal.feature.can_create_guild();
        let dropdown_open = modal.dropdown_open;

        let dropdown_list = dropdown_open.then(|| {
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
                .children(guilds.iter().enumerate().map(|(idx, (gid, name))| {
                    let gid = *gid;
                    let is_sel = selected == Some(gid);
                    div()
                        .id(format!("pick-guild-opt-{idx}"))
                        .cursor_pointer()
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(6.))
                        .when(is_sel, |d| d.bg(rgba(0x5865f233)))
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = this.pick_guild_settings_modal.as_mut() {
                                m.selected_guild = Some(gid);
                                m.dropdown_open = false;
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
                                .child(name.clone()),
                        )
                        .when(is_sel, |row| {
                            row.child(icon(
                                "circle_check",
                                16.,
                                "--control-primary-background-default",
                            ))
                        })
                }))
        });

        let body = div()
            .px(px(24.))
            .py(px(16.))
            .flex()
            .flex_col()
            .gap(px(24.))
            // Top Stack: description + guild selection row (`guildSelection_0kOndy`)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(modal.feature.description()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("pick-guild-dropdown")
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(40.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lowest"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.pick_guild_settings_modal.as_mut() {
                                            m.dropdown_open = !m.dropdown_open;
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(14.))
                                            .text_color(if selected.is_some() {
                                                color("--text-strong")
                                            } else {
                                                color("--text-muted")
                                            })
                                            .truncate()
                                            .child(selected_label),
                                    )
                                    .child(icon("shop_select_chevron", 16., "--icon-subtle")),
                            )
                            .child(
                                div()
                                    .id("pick-guild-get-started")
                                    .flex_none()
                                    .h(px(40.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(selected.is_some(), |d| d.cursor_pointer())
                                    .when(selected.is_none(), |d| d.opacity(0.5))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("LhlgY9"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(gid) = selected {
                                            this.pick_guild_settings_modal = None;
                                            let target_page = match section.as_deref() {
                                                Some("subscriptions" | "role-subscriptions") => {
                                                    super::server_settings::Page::Access
                                                }
                                                Some("roles") => {
                                                    super::server_settings::Page::Roles
                                                }
                                                Some("safety") => {
                                                    super::server_settings::Page::SafetySetup
                                                }
                                                _ => super::server_settings::Page::Profile,
                                            };
                                            this.open_server_settings_at(gid, target_page, cx);
                                        }
                                    })),
                            ),
                    )
                    .children(dropdown_list),
            )
            // Optional Create a Server stack (`canCreateGuild`)
            .when(can_create, |col| {
                col.child(div().h(px(1.)).bg(color("--border-subtle")))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("anOisx")),
                            )
                            .child(
                                div()
                                    .id("pick-guild-create-server")
                                    .cursor_pointer()
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("B44MTm"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pick_guild_settings_modal = None;
                                        this.open_add_server(window, cx);
                                    })),
                            ),
                    )
            });

        Some(
            deferred(
                div()
                    .id("pick-guild-settings-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pick_guild_settings_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("pick-guild-settings-modal")
                            .w(px(480.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(modal.feature.title()),
                                    )
                                    .child(
                                        div()
                                            .id("pick-guild-settings-close")
                                            .flex_none()
                                            .size(px(32.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.pick_guild_settings_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(body),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `pick-guild-settings [default|onboarding|template|close]` (module 623509).
    #[cfg(feature = "capture")]
    pub(crate) fn script_pick_guild_settings(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        match arg.trim() {
            "close" => {
                self.pick_guild_settings_modal = None;
                cx.notify();
                "pick-guild-settings: closed".to_owned()
            }
            "onboarding" => {
                self.open_pick_guild_settings(
                    Some("role-subscriptions".to_owned()),
                    PickGuildFeature::ServerSubscriptionsOnboarding,
                    cx,
                );
                "pick-guild-settings: onboarding".to_owned()
            }
            "template" => {
                self.open_pick_guild_settings(
                    Some("role-subscriptions".to_owned()),
                    PickGuildFeature::ServerSubscriptionsTemplate,
                    cx,
                );
                "pick-guild-settings: template".to_owned()
            }
            "server-tag" => {
                self.open_pick_guild_settings(None, PickGuildFeature::CreateServerTag, cx);
                "pick-guild-settings: server-tag".to_owned()
            }
            _ => {
                self.open_pick_guild_settings(None, PickGuildFeature::Default, cx);
                "pick-guild-settings: default".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_server_tag_208652_intl_keys() {
        assert_eq!(
            PickGuildFeature::CreateServerTag.title(),
            "Create a Server Tag"
        );
        for k in ["xO5QzM", "h+7Yx3", "zRl6XR"] {
            assert!(dc_intl::has_key(k), "missing 208652 key {k}");
        }
    }
}
