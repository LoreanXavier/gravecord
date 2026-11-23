//! Student Hubs "Add a server to {guildName}" 5-slide modal (module 668822, templates/categories 946116,
//! directory entry creation 208882 `WD`).

use super::{Root, text};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::ui::{color, editor_style, icon};
use dc_intl::Arg;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window,
    deferred, div, img, prelude::*, px, rgba,
};
use serde_json::{Value, json};
use std::borrow::Cow;

pub(crate) const HUB_ADD_ENTRY_KEY: &str = "discover:hub:add-entry";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HubAddSlide {
    ChooseGuild,
    GuildTemplates,
    CustomizeNewGuild,
    CustomizeExisting,
    Confirmation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HubGuildTab {
    Available,
    Added,
}

/// Official Student Hub categories (`946116` `g2`: `oNHFUL`, `V1i8nS`, `Z2ygQt`, `0Bg9La`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HubCategory {
    pub id: u64,
    pub key: &'static str,
}

pub(crate) const HUB_CATEGORIES: &[HubCategory] = &[
    HubCategory {
        id: 1,
        key: "oNHFUL", // Clubs
    },
    HubCategory {
        id: 2,
        key: "V1i8nS", // Classes & Subjects
    },
    HubCategory {
        id: 3,
        key: "Z2ygQt", // Social & Study
    },
    HubCategory {
        id: 4,
        key: "0Bg9La", // Miscellaneous
    },
];

/// Official Student Hub server templates (`946116` `ul`).
pub(crate) const HUB_TEMPLATES: &[(&str, &str)] = &[
    ("HUB_STUDY", "fYwSi2"),       // Study Group
    ("HUB_SCHOOL_CLUB", "mmFA51"), // School Club
    ("HUB_CLASS", "e5gFS3"),       // Class
    ("HUB_SOCIAL", "oOj7It"),      // Social
    ("HUB_MAJOR", "9kly4U"),       // Subject or Major
    ("HUB_DORM", "ppGw/P"),        // Dorm
];

#[derive(Clone, Debug)]
pub(crate) struct HubGuildEntry {
    pub id: Id,
    pub name: String,
}

pub(crate) struct HubAddServerModal {
    pub directory_channel_id: Id,
    pub directory_guild_id: Id,
    pub directory_guild_name: String,
    pub slide: HubAddSlide,
    pub tab: HubGuildTab,
    pub available_guilds: Vec<HubGuildEntry>,
    pub added_guilds: Vec<HubGuildEntry>,
    pub selected_guild: Option<HubGuildEntry>,
    pub selected_template: Option<String>,
    pub is_existing_guild_flow: bool,
    pub new_guild_name: Entity<Editor>,
    pub description: Entity<Editor>,
    pub category_id: Option<u64>,
    pub category_dropdown_open: bool,
    pub busy: bool,
    pub error: Option<String>,
    _subs: Vec<Subscription>,
}

/// Official `DIRECTORY_CHANNEL_ENTRY` POST request (`208882` `WD`, `652215`).
pub(crate) fn add_directory_entry_request(
    directory_channel_id: Id,
    guild_id: Id,
    description: &str,
    primary_category_id: u64,
) -> Request {
    Request::Rest {
        key: Some(HUB_ADD_ENTRY_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/channels/{directory_channel_id}/directory-entries/{guild_id}"),
        body: Some(json!({
            "description": description,
            "primary_category_id": primary_category_id,
        })),
    }
}

fn guild_acronym(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(3)
        .collect()
}

impl Root {
    pub(crate) fn open_hub_add_server_modal(
        &mut self,
        directory_channel_id: Id,
        directory_guild_id: Id,
        directory_guild_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_guild_name = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_single_line(true);
            ed.set_placeholder(dc_intl::tr("Server Name").to_owned());
            ed
        });
        let description = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_placeholder(dc_intl::t("VzuITC").to_owned());
            ed
        });
        let s1 = cx.subscribe_in(&new_guild_name, window, |_, _, _: &EditorEvent, _, cx| {
            cx.notify();
        });
        let s2 = cx.subscribe_in(&description, window, |_, _, _: &EditorEvent, _, cx| {
            cx.notify();
        });

        let mut available = Vec::new();
        for &gid in &self.stores.guild_order {
            if gid == directory_guild_id {
                continue;
            }
            if let Some(g) = self.stores.guild(gid) {
                available.push(HubGuildEntry {
                    id: gid,
                    name: g.name.clone(),
                });
            }
        }

        self.hub_add_server_modal = Some(HubAddServerModal {
            directory_channel_id,
            directory_guild_id,
            directory_guild_name,
            slide: HubAddSlide::ChooseGuild,
            tab: HubGuildTab::Available,
            available_guilds: available,
            added_guilds: Vec::new(),
            selected_guild: None,
            selected_template: None,
            is_existing_guild_flow: true,
            new_guild_name,
            description,
            category_id: None,
            category_dropdown_open: false,
            busy: false,
            error: None,
            _subs: vec![s1, s2],
        });
        cx.notify();
    }

    pub(crate) fn close_hub_add_server_modal(&mut self) -> bool {
        self.hub_add_server_modal.take().is_some()
    }

    pub(crate) fn hub_add_server_fetched(&mut self, body: &Value) {
        let Some(modal) = self.hub_add_server_modal.as_mut() else {
            return;
        };
        modal.busy = false;
        if body["ok"] == true {
            modal.error = None;
            if let Some(g) = modal.selected_guild.clone() {
                modal.available_guilds.retain(|x| x.id != g.id);
                if !modal.added_guilds.iter().any(|x| x.id == g.id) {
                    modal.added_guilds.push(g);
                }
            }
            modal.slide = HubAddSlide::Confirmation;
        } else {
            modal.error = Some(crate::ui::server_automod::any_error_message(&body["body"]));
        }
    }

    fn hub_add_server_submit(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.hub_add_server_modal.as_mut() else {
            return;
        };
        if modal.busy {
            return;
        }
        let desc = modal.description.read(cx).text().trim().to_owned();
        let Some(cat) = modal.category_id else {
            return;
        };
        if desc.is_empty() {
            return;
        }
        let guild = modal.selected_guild.clone().unwrap_or_else(|| {
            let name = modal.new_guild_name.read(cx).text().trim().to_owned();
            HubGuildEntry {
                id: modal.directory_guild_id,
                name: if name.is_empty() {
                    "New Server".to_owned()
                } else {
                    name
                },
            }
        });
        modal.selected_guild = Some(guild.clone());
        modal.busy = true;
        modal.error = None;
        let req = add_directory_entry_request(modal.directory_channel_id, guild.id, &desc, cat);
        let _ = self.requests.send(req);
        cx.notify();
    }

    fn hub_guild_icon(&self, entry: &HubGuildEntry, size: f32) -> AnyElement {
        let pic = self.images.get(&format!("guild:{}", entry.id)).cloned();
        div()
            .flex_none()
            .size(px(size))
            .rounded(px((size * 0.3).round()))
            .overflow_hidden()
            .bg(color("--background-surface-higher"))
            .flex()
            .items_center()
            .justify_center()
            .when_some(pic, |b, picture| b.child(img(picture).size(px(size))))
            .when(
                !self.images.contains_key(&format!("guild:{}", entry.id)),
                |b| {
                    b.child(
                        div()
                            .text_size(px((size * 0.35).max(12.)))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(guild_acronym(&entry.name)),
                    )
                },
            )
            .into_any_element()
    }

    pub(in crate::ui) fn hub_add_server_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.hub_add_server_modal.as_ref()?;
        let hub_name = modal.directory_guild_name.clone();

        let close_btn = div()
            .id("hub-add-server-close")
            .size(px(32.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(color("--background-mod-subtle")))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("modal_close", 18., "--interactive-icon-default"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_hub_add_server_modal();
                cx.notify();
            }));

        let content: AnyElement = match modal.slide {
            HubAddSlide::ChooseGuild => {
                let title = dc_intl::format(
                    "9SKJdF",
                    &[("guildName", Arg::Str(Cow::Borrowed(&hub_name)))],
                );
                let tab = modal.tab;
                let tab_btn = |id: &'static str, label: &'static str, which: HubGuildTab| {
                    let active = tab == which;
                    div()
                        .id(id)
                        .pb(px(10.))
                        .border_b_2()
                        .border_color(if active {
                            color("--control-primary-background-default")
                        } else {
                            gpui::hsla(0., 0., 0., 0.).into()
                        })
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(if active { 600. } else { 500. }))
                        .text_color(if active {
                            color("--text-strong")
                        } else {
                            color("--text-muted")
                        })
                        .child(dc_intl::t(label))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = this.hub_add_server_modal.as_mut() {
                                m.tab = which;
                            }
                            cx.notify();
                        }))
                };

                let list_body: AnyElement = match tab {
                    HubGuildTab::Available => {
                        if modal.available_guilds.is_empty() {
                            div()
                                .py(px(36.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(12.))
                                .child(icon("discover_hub", 36., "--text-muted"))
                                .child(
                                    text(14., 20., 400., "--text-muted")
                                        .text_center()
                                        .child(dc_intl::t("liuEhd")),
                                )
                                .into_any_element()
                        } else {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .children(modal.available_guilds.iter().enumerate().map(
                                    |(idx, g)| {
                                        let chosen = g.clone();
                                        div()
                                            .id(SharedString::from(format!("hub-add-avail-{idx}")))
                                            .p(px(10.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-base-lower"))
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .child(self.hub_guild_icon(g, 40.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .text_size(px(15.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(g.name.clone()),
                                            )
                                            .child(icon(
                                                "chevron_right_20",
                                                18.,
                                                "--interactive-icon-default",
                                            ))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(m) = this.hub_add_server_modal.as_mut()
                                                {
                                                    m.is_existing_guild_flow = true;
                                                    m.selected_guild = Some(chosen.clone());
                                                    m.slide = HubAddSlide::CustomizeExisting;
                                                }
                                                cx.notify();
                                            }))
                                    },
                                ))
                                .into_any_element()
                        }
                    }
                    HubGuildTab::Added => {
                        if modal.added_guilds.is_empty() {
                            div()
                                .py(px(36.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(12.))
                                .child(icon("discover_hub", 36., "--text-muted"))
                                .child(
                                    text(14., 20., 400., "--text-muted")
                                        .text_center()
                                        .child(dc_intl::t("WJLuKK")),
                                )
                                .into_any_element()
                        } else {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .children(modal.added_guilds.iter().map(|g| {
                                    div()
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lower"))
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .child(self.hub_guild_icon(g, 40.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .text_size(px(15.))
                                                .text_color(color("--text-default"))
                                                .child(g.name.clone()),
                                        )
                                        .child(icon(
                                            "more_horizontal",
                                            18.,
                                            "--interactive-icon-default",
                                        ))
                                }))
                                .into_any_element()
                        }
                    }
                };

                div()
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
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(text(20., 24., 600., "--text-strong").child(title))
                                    .child(
                                        text(14., 18., 400., "--text-default")
                                            .child(dc_intl::t("pYFZ9p")),
                                    ),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .gap(px(20.))
                            .child(tab_btn("hub-tab-avail", "FTe8HS", HubGuildTab::Available))
                            .child(tab_btn("hub-tab-added", "epOumr", HubGuildTab::Added)),
                    )
                    .child(
                        div()
                            .id("hub-add-server-list")
                            .px(px(24.))
                            .py(px(16.))
                            .max_h(px(260.))
                            .overflow_y_scroll()
                            .child(list_body),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(16.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .rounded_b(px(12.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                text(16., 20., 600., "--text-strong").child(dc_intl::t("pgCZRP")),
                            )
                            .child(
                                div()
                                    .id("hub-add-create-new")
                                    .w_full()
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("WqJbLi"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.hub_add_server_modal.as_mut() {
                                            m.is_existing_guild_flow = false;
                                            m.slide = HubAddSlide::GuildTemplates;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            HubAddSlide::GuildTemplates => {
                let title = dc_intl::format(
                    "T7aLYT",
                    &[("guildName", Arg::Str(Cow::Borrowed(&hub_name)))],
                );
                let tpl_row = |id: SharedString, tpl_id: &'static str, label: &'static str| {
                    div()
                        .id(id)
                        .p(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lower"))
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t(label)),
                        )
                        .child(icon("chevron_right_20", 18., "--interactive-icon-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = this.hub_add_server_modal.as_mut() {
                                m.selected_template = Some(tpl_id.to_owned());
                                m.is_existing_guild_flow = false;
                                m.slide = HubAddSlide::CustomizeNewGuild;
                            }
                            cx.notify();
                        }))
                };
                div()
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
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(text(20., 24., 600., "--text-strong").child(title))
                                    .child(
                                        text(14., 18., 400., "--text-default")
                                            .child(dc_intl::t("RA+St6")),
                                    ),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .id("hub-add-templates-scroll")
                            .px(px(24.))
                            .pb(px(16.))
                            .max_h(px(320.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(tpl_row("hub-tpl-create".into(), "CREATE", "WqJbLi"))
                            .child(
                                div()
                                    .mt(px(8.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("JGDkfg").to_uppercase()),
                            )
                            .children(HUB_TEMPLATES.iter().map(|(tid, key)| {
                                tpl_row(SharedString::from(format!("hub-tpl-{tid}")), tid, key)
                            })),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .rounded_b(px(12.))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .id("hub-tpl-back")
                                    .h(px(34.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("13/7kX"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.hub_add_server_modal.as_mut() {
                                            m.slide = HubAddSlide::ChooseGuild;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            HubAddSlide::CustomizeNewGuild => {
                let can_next = !modal.new_guild_name.read(cx).text().trim().is_empty();
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(24.))
                            .pt(px(20.))
                            .pb(px(12.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                text(20., 24., 600., "--text-strong").child(dc_intl::t("WqJbLi")),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .pb(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                text(12., 16., 700., "--text-muted")
                                    .child(dc_intl::tr("Server Name").to_uppercase()),
                            )
                            .child(
                                div()
                                    .h(px(40.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .child(modal.new_guild_name.clone()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .rounded_b(px(12.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .id("hub-new-back")
                                    .h(px(36.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("13/7kX"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.hub_add_server_modal.as_mut() {
                                            m.slide = HubAddSlide::GuildTemplates;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("hub-new-next")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(!can_next, |b| b.opacity(0.5))
                                    .when(can_next, |b| b.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("PDTjLN"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !can_next {
                                            return;
                                        }
                                        if let Some(m) = this.hub_add_server_modal.as_mut() {
                                            let name =
                                                m.new_guild_name.read(cx).text().trim().to_owned();
                                            m.selected_guild = Some(HubGuildEntry {
                                                id: m.directory_guild_id,
                                                name,
                                            });
                                            m.slide = HubAddSlide::CustomizeExisting;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            HubAddSlide::CustomizeExisting => {
                let desc_text = modal.description.read(cx).text().to_owned();
                let desc_len = desc_text.chars().count();
                let can_submit =
                    !desc_text.trim().is_empty() && modal.category_id.is_some() && !modal.busy;
                let selected_cat_label = modal
                    .category_id
                    .and_then(|id| HUB_CATEGORIES.iter().find(|c| c.id == id))
                    .map(|c| dc_intl::t(c.key))
                    .unwrap_or_else(|| dc_intl::t("XqMe3N"));
                let error = modal.error.clone();
                let dropdown_open = modal.category_dropdown_open;
                let existing_flow = modal.is_existing_guild_flow;

                div()
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
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        text(20., 24., 600., "--text-strong")
                                            .child(dc_intl::t("5bQcoa")),
                                    )
                                    .child(
                                        text(14., 18., 400., "--text-default")
                                            .child(dc_intl::t("Ie60Wc")),
                                    ),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .pb(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
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
                                                text(14., 18., 600., "--text-strong")
                                                    .child(dc_intl::t("FFFAGt")),
                                            )
                                            .child(
                                                text(12., 16., 400., "--text-muted")
                                                    .child(format!("{desc_len}/200")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .min_h(px(76.))
                                            .p(px(10.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(if error.is_some() {
                                                color("--input-border-error-default")
                                            } else {
                                                color("--input-border-default")
                                            })
                                            .bg(color("--input-background-default"))
                                            .child(modal.description.clone()),
                                    )
                                    .when_some(error, |col, msg| {
                                        col.child(
                                            text(12., 16., 400., "--text-feedback-critical")
                                                .child(msg),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        text(14., 18., 600., "--text-strong")
                                            .child(dc_intl::t("Olo8FB")),
                                    )
                                    .child(
                                        div()
                                            .id("hub-cat-select")
                                            .relative()
                                            .h(px(40.))
                                            .px(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--input-border-default"))
                                            .bg(color("--input-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .text_color(if modal.category_id.is_some() {
                                                        color("--text-strong")
                                                    } else {
                                                        color("--input-placeholder-text-default")
                                                    })
                                                    .child(selected_cat_label),
                                            )
                                            .child(icon(
                                                "chevron_down",
                                                16.,
                                                "--interactive-icon-default",
                                            ))
                                            .child(self.anchor_probe("hub-cat-select"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(m) = this.hub_add_server_modal.as_mut()
                                                {
                                                    m.category_dropdown_open =
                                                        !m.category_dropdown_open;
                                                }
                                                cx.notify();
                                            })),
                                    )
                                    .when(dropdown_open, |col| {
                                        col.child(
                                            div()
                                                .id("hub-cat-list")
                                                .mt(px(4.))
                                                .p(px(4.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .bg(color("--background-base-lower"))
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.))
                                                // `maxOptionsVisible: 4` (module 668822): four 34px rows, 2px gaps, 4px padding.
                                                .max_h(px(150.))
                                                .overflow_y_scroll()
                                                .on_click(|_, _, cx| cx.stop_propagation())
                                                .on_mouse_down_out(cx.listener(
                                                    |this, event: &gpui::MouseDownEvent, _, cx| {
                                                        let on_trigger = this
                                                            .tip_anchors
                                                            .borrow()
                                                            .get("hub-cat-select")
                                                            .is_some_and(|field| {
                                                                field.contains(&event.position)
                                                            });
                                                        if !on_trigger
                                                            && let Some(m) =
                                                                this.hub_add_server_modal.as_mut()
                                                            && m.category_dropdown_open
                                                        {
                                                            m.category_dropdown_open = false;
                                                            cx.notify();
                                                        }
                                                    },
                                                ))
                                                .children(HUB_CATEGORIES.iter().map(|cat| {
                                                    let cid = cat.id;
                                                    let selected = modal.category_id == Some(cid);
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "hub-cat-opt-{cid}"
                                                        )))
                                                        .h(px(34.))
                                                        .flex_none()
                                                        .px(px(10.))
                                                        .rounded(px(6.))
                                                        .when(selected, |d| {
                                                            d.bg(color("--background-mod-strong"))
                                                        })
                                                        .hover(|s| {
                                                            s.bg(color("--background-mod-subtle"))
                                                        })
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_size(px(14.))
                                                        .text_color(color("--text-strong"))
                                                        .child(dc_intl::t(cat.key))
                                                        .when(selected, |d| {
                                                            d.child(icon(
                                                                "select_check",
                                                                14.,
                                                                "--control-primary-background-default",
                                                            ))
                                                        })
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if let Some(m) = this
                                                                    .hub_add_server_modal
                                                                    .as_mut()
                                                                {
                                                                    m.category_id = Some(cid);
                                                                    m.category_dropdown_open =
                                                                        false;
                                                                }
                                                                cx.notify();
                                                            },
                                                        ))
                                                })),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .rounded_b(px(12.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .id("hub-last-back")
                                    .h(px(36.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("13/7kX"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.hub_add_server_modal.as_mut() {
                                            m.slide = if existing_flow {
                                                HubAddSlide::ChooseGuild
                                            } else {
                                                HubAddSlide::CustomizeNewGuild
                                            };
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("hub-last-submit")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(!can_submit, |b| b.opacity(0.5))
                                    .when(can_submit, |b| b.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("H9jxS1"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if can_submit {
                                            this.hub_add_server_submit(cx);
                                        }
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            HubAddSlide::Confirmation => {
                let guild = modal
                    .selected_guild
                    .clone()
                    .unwrap_or_else(|| HubGuildEntry {
                        id: modal.directory_guild_id,
                        name: hub_name.clone(),
                    });
                let subtitle = if modal.is_existing_guild_flow {
                    dc_intl::format(
                        "R7Pqn5",
                        &[("guildName", Arg::Str(Cow::Borrowed(&hub_name)))],
                    )
                } else {
                    dc_intl::format(
                        "eIxPSv",
                        &[("guildName", Arg::Str(Cow::Borrowed(&hub_name)))],
                    )
                };
                let existing = modal.is_existing_guild_flow;
                let goto_gid = guild.id;
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(28.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(12.))
                            .child(self.hub_guild_icon(&guild, 64.))
                            .child(
                                text(24., 30., 600., "--text-strong").child(dc_intl::t("CueiPY")),
                            )
                            .child(
                                text(14., 20., 400., "--text-default")
                                    .text_center()
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(16.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .rounded_b(px(12.))
                            .flex()
                            .gap(px(8.))
                            .when(existing, |bar| {
                                bar.child(
                                    div()
                                        .id("hub-confirm-got-it")
                                        .flex_1()
                                        .h(px(38.))
                                        .rounded(px(8.))
                                        .bg(color("--control-primary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::t("X0WK+6"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_hub_add_server_modal();
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(!existing, |bar| {
                                bar.child(
                                    div()
                                        .id("hub-confirm-back-hub")
                                        .flex_1()
                                        .h(px(38.))
                                        .rounded(px(8.))
                                        .bg(color("--control-secondary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("X/VABv"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_hub_add_server_modal();
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("hub-confirm-go-server")
                                        .flex_1()
                                        .h(px(38.))
                                        .rounded(px(8.))
                                        .bg(color("--control-primary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::t("DymAQt"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.close_hub_add_server_modal();
                                            if this.stores.guild(goto_gid).is_some() {
                                                this.select(goto_gid, None);
                                            }
                                            cx.notify();
                                        })),
                                )
                            }),
                    )
                    .into_any_element()
            }
        };

        let dialog = div()
            .id("hub-add-server-modal")
            .occlude()
            .w(px(460.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.28),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(content);

        Some(
            deferred(
                div()
                    .id("hub-add-server-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_hub_add_server_modal();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
    #[cfg(feature = "capture")]
    pub(crate) fn script_hub_add_server(
        &mut self,
        arg: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        self.open_hub_add_server_modal(
            1568692565136375900,
            999000111222,
            "Stanford University".to_owned(),
            window,
            cx,
        );
        if let Some(m) = self.hub_add_server_modal.as_mut() {
            match arg {
                "templates" => m.slide = HubAddSlide::GuildTemplates,
                "new" => m.slide = HubAddSlide::CustomizeNewGuild,
                "last" => {
                    m.selected_guild = m.available_guilds.first().cloned();
                    m.category_id = Some(2);
                    m.category_dropdown_open = true;
                    m.slide = HubAddSlide::CustomizeExisting;
                }
                "confirm" => {
                    m.selected_guild = m.available_guilds.first().cloned();
                    m.slide = HubAddSlide::Confirmation;
                }
                _ => {}
            }
        }
        cx.notify();
        format!("hub-add-server: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_add_directory_entry_request_matches_208882() {
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = add_directory_entry_request(100, 200, "Study club for CS", 2)
        else {
            panic!("expected rest");
        };
        assert_eq!(key.as_deref(), Some(HUB_ADD_ENTRY_KEY));
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/channels/100/directory-entries/200");
        assert_eq!(
            body,
            Some(json!({
                "description": "Study club for CS",
                "primary_category_id": 2
            }))
        );
        for cat in HUB_CATEGORIES {
            assert!(!dc_intl::t(cat.key).is_empty());
        }
        for (_, key) in HUB_TEMPLATES {
            assert!(!dc_intl::t(key).is_empty());
        }
    }
}
