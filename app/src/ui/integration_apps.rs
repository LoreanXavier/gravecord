//! Server Settings > Integrations > Bots and Apps (module 986057 `t0`): the server's app integrations from
//! `GET /guilds/{id}/integrations?include_applications=true&include_role_connections_metadata=true` (module 794967,
//! `GUILD_SETTINGS_LOADED_INTEGRATIONS`), one card each (`tw` / `ep`: the 48px icon, the name, "Added on {date} by
//! {user}", the Bot or Verified Bot and webhook tags, Manage), the search box over more than four, and its empty
//! result. Not drawn: the Commands tag (it needs the app's command index), the premium ticket, and the app's own page
//! behind Manage.

use super::{Root, color, icon};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use serde_json::Value;

pub(super) const INTEGRATIONS_PREFIX: &str = "guild-integrations:";
/// `emptyStateSearchImage_2S81sZ`'s dark picture.
pub(super) const NO_MATCH_ART: &str = "https://discord.com/assets/9d2bd46b521600c7.svg";
/// `VERIFIED_BOT` of the user flags.
const VERIFIED_BOT: u64 = 1 << 16;
/// Snowflakes count from 2015.
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

/// One app of the list: `{application, integration}` of `t0`'s grouping.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AppIntegration {
    pub(super) integration: String,
    pub(super) app: String,
    pub(super) name: String,
    pub(super) icon: Option<String>,
    /// The bot's username and whether it is verified, when the app has a bot.
    pub(super) bot: Option<(String, bool)>,
    pub(super) bot_scope: bool,
    /// `getUserTag` of who added it.
    pub(super) user: Option<String>,
    /// The application's description, shown on its page.
    pub(super) description: Option<String>,
}

impl AppIntegration {
    /// `getApplicationIconURL` at 48 (no WebP, the aspect kept off).
    pub(super) fn icon_url(&self) -> Option<String> {
        self.icon.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/app-icons/{}/{hash}.png?size=48&keep_aspect_ratio=false",
                self.app
            )
        })
    }

    /// When it was added: the integration id's moment.
    fn added_ms(&self) -> Option<i64> {
        let id: u64 = self.integration.parse().ok()?;
        Some((id >> 22) as i64 + DISCORD_EPOCH_MS)
    }
}

/// `t0`'s grouping: the `discord` integrations with an application, one per application (a later one replaces an
/// earlier), in the order they came.
pub(super) fn app_integrations(list: &Value) -> Vec<AppIntegration> {
    let mut out: Vec<AppIntegration> = Vec::new();
    for item in list.as_array().into_iter().flatten() {
        if item["type"].as_str() != Some("discord") {
            continue;
        }
        let application = &item["application"];
        let Some(app) = application["id"].as_str() else {
            continue;
        };
        let bot = &application["bot"];
        let user = &item["user"];
        let entry = AppIntegration {
            integration: item["id"].as_str().unwrap_or_default().to_owned(),
            app: app.to_owned(),
            name: application["name"].as_str().unwrap_or_default().to_owned(),
            icon: application["icon"].as_str().map(str::to_owned),
            description: application["description"]
                .as_str()
                .filter(|text| !text.is_empty())
                .map(str::to_owned),
            bot: bot["id"].as_str().map(|_| {
                let flags = bot["public_flags"].as_u64().unwrap_or(0);
                (
                    bot["username"].as_str().unwrap_or_default().to_owned(),
                    flags & VERIFIED_BOT != 0,
                )
            }),
            bot_scope: item["scopes"]
                .as_array()
                .is_some_and(|scopes| scopes.iter().any(|scope| scope == "bot")),
            user: user["username"]
                .as_str()
                .map(|name| match user["discriminator"].as_str() {
                    Some(tag) if tag != "0" => format!("{name}#{tag}"),
                    _ => name.to_owned(),
                }),
        };
        match out.iter_mut().find(|known| known.app == entry.app) {
            Some(known) => *known = entry,
            None => out.push(entry),
        }
    }
    out
}

/// `ec` of `t0`: an empty query, the app's id, or the query in the app's or its bot's name; over a hundred apps
/// the name must contain it (`tX`), otherwise its letters in order do (module 91871).
pub(super) fn app_matches(app: &AppIntegration, query: &str, many: bool) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() || app.app == query {
        return true;
    }
    let found = |hay: &str| {
        let hay = hay.to_lowercase();
        if many {
            hay.contains(&query)
        } else {
            let mut rest = hay.chars();
            query.chars().all(|want| rest.any(|have| have == want))
        }
    };
    found(&app.name) || app.bot.as_ref().is_some_and(|(name, _)| found(name))
}

/// The webhooks an app made in this server (its `webhooks`, by `application_id`).
pub(super) fn app_webhooks(app: &str, webhooks: &[Value]) -> usize {
    webhooks
        .iter()
        .filter(|hook| hook["application_id"].as_str() == Some(app))
        .count()
}

impl Root {
    pub(super) fn request_app_integrations(&mut self, guild: Id) {
        let _ = self.requests.send(crate::net::Request::Fetch {
            key: format!("{INTEGRATIONS_PREFIX}{guild}"),
            route: format!(
                "/guilds/{guild}/integrations?include_applications=true&include_role_connections_metadata=true"
            ),
        });
        self.request_image(NO_MATCH_ART.to_owned(), NO_MATCH_ART.to_owned());
    }

    /// The apps, once fetched; `None` while nothing came (the empty state stays, as before the fetch).
    pub(super) fn app_integration_list(&self, guild: Id) -> Option<Vec<AppIntegration>> {
        let body = self.fetched.get(&format!("{INTEGRATIONS_PREFIX}{guild}"))?;
        if crate::net::is_fetch_failure(body) {
            return None;
        }
        Some(app_integrations(body))
    }

    /// The list arrived: it is kept and the apps' icons are asked for.
    pub(super) fn app_integrations_fetched(
        &mut self,
        key: &str,
        body: Value,
        cx: &mut Context<Self>,
    ) {
        for app in app_integrations(&body) {
            if let Some(url) = app.icon_url() {
                self.request_image(url.clone(), url);
            }
        }
        self.fetched.insert(key.to_owned(), body);
        cx.notify();
    }

    /// The search box over more than four apps (`t$`, `searchContainer_2S81sZ`), made when the page opens.
    pub(super) fn ensure_app_search(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_ref() else {
            return;
        };
        if open.apps_search.is_some() {
            return;
        }
        let editor = cx.new(|cx| {
            let mut editor = crate::editor::Editor::new(super::editor_style(), cx);
            editor.set_placeholder(dc_intl::t("5prvKe").to_owned());
            editor
        });
        let events = cx.subscribe(
            &editor,
            |this, editor, event: &crate::editor::EditorEvent, cx| {
                if let crate::editor::EditorEvent::Edited = event {
                    let text = editor.read(cx).text().to_owned();
                    if let Some(open) = this.server_settings.as_mut() {
                        open.apps_query = text;
                    }
                    cx.notify();
                }
            },
        );
        if let Some(open) = self.server_settings.as_mut() {
            open.apps_search = Some((editor, events));
        }
    }

    /// One app's card (`tw` with `hasNextSection`): `card_ZjaEbs` (20px 16px, 16px under), the icon and the text 16px
    /// apart, the tags 8px apart after the name block, "Manage" and its 10px chevron at the right.
    pub(super) fn app_integration_card(
        &self,
        guild: Id,
        app: &AppIntegration,
        webhooks: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let app_id = app.app.clone();
        let picture = app
            .icon_url()
            .and_then(|url| self.images.get(&url).cloned());
        let added = app.added_ms().and_then(|ms| {
            chrono::DateTime::from_timestamp_millis(ms).map(|at| {
                at.with_timezone(&chrono::Local)
                    .format("%b %-d, %Y")
                    .to_string()
            })
        });
        let detail = added.map(|date| match &app.user {
            Some(user) => dc_intl::format(
                "Nu9sat",
                &[
                    (
                        "timestamp",
                        dc_intl::Arg::When {
                            date: date.clone().into(),
                            time: date.clone().into(),
                        },
                    ),
                    ("user", user.as_str().into()),
                ],
            ),
            None => dc_intl::format(
                "gcdJ8J",
                &[(
                    "timestamp",
                    dc_intl::Arg::When {
                        date: date.clone().into(),
                        time: date.into(),
                    },
                )],
            ),
        });
        // `tv.C` inline, small: 30px tags, 4px 8px in, the 16px icon 4px before the label.
        let tag = |glyph: &'static str, label: String| {
            div()
                .min_h(px(30.))
                .px(px(8.))
                .py(px(4.))
                .rounded(px(4.))
                .border_1()
                .border_color(color("--control-secondary-border-default"))
                .bg(color("--control-secondary-background-default"))
                .flex()
                .items_center()
                .gap(px(4.))
                .whitespace_nowrap()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--control-secondary-text-default"))
                .child(icon(glyph, 16., "--control-secondary-text-default"))
                .child(label)
        };
        let mut tags: Vec<gpui::Div> = Vec::new();
        if let Some((_, verified)) = &app.bot
            && app.bot_scope
        {
            tags.push(tag(
                "oauth_entry_robot",
                dc_intl::t(if *verified { "xxcTGy" } else { "AOdOYr" }).to_owned(),
            ));
        }
        if webhooks > 0 {
            tags.push(tag(
                "integrations_webhook",
                dc_intl::format("6HqDfT", &[("count", (webhooks as f64).into())]),
            ));
        }
        let _ = guild;
        div()
            .id(SharedString::from(format!(
                "integration-{}",
                app.integration
            )))
            .mb(px(16.))
            .py(px(20.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--card-background-default"))
            .border_color(color("--border-subtle"))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(open) = this.server_settings.as_mut() {
                    open.app_open = Some(app_id.clone());
                    open.scroll.set_offset(gpui::point(px(0.), px(0.)));
                }
                cx.notify();
            }))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    // `ep`: the icon only when the app has one, then the text and the tags 8px under it (a
                    // vertical `Stack`, its default 8px gap even when there are no tags), the tags 4px apart.
                    .when_some(app.icon_url(), |this, _| {
                        this.child(
                            div()
                                .size(px(48.))
                                .flex_none()
                                .rounded_full()
                                .overflow_hidden()
                                .bg(color("--background-surface-high"))
                                .children(
                                    picture
                                        .map(|picture| img(picture).size(px(48.)).rounded_full()),
                                ),
                        )
                    })
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(app.name.clone()),
                                    )
                                    .children(detail.map(|text| {
                                        // `detailsWrapper__wphjE`: 4px down, the clock 4px before `body-xs/normal`.
                                        div()
                                            .mt(px(4.))
                                            .flex()
                                            .items_center()
                                            .child(
                                                icon(
                                                    "oauth_entry_clock",
                                                    12.,
                                                    "--interactive-text-default",
                                                )
                                                .mr(px(4.)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(color("--text-default"))
                                                    .child(text),
                                            )
                                    })),
                            )
                            .child(div().flex().flex_wrap().gap(px(4.)).children(tags)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("Z/qRnM"))
                    .child(
                        icon("bot_chevron_small_right", 10., "--interactive-text-default")
                            .ml(px(8.)),
                    ),
            )
            .into_any_element()
    }

    /// The app's page behind Manage (`e3` of module 986057): the breadcrumb, the header (`ep` with `isHeader`: the 48px
    /// icon, the name in `heading-xl/semibold`, "Added on" and "Added by" 40px apart), the description, the 32px
    /// divider, the Bot section (its bot, or "This application does not have a bot in this server."), the Webhooks
    /// section, another divider, then the Remove App row. Not drawn: the commands and their permission overrides
    /// (`er`, it needs the app's command index), the bot's roles and permission lists (they need its member), and the
    /// webhooks' editors.
    pub(super) fn app_page(&self, guild: Id, app_id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(app) = self
            .app_integration_list(guild)
            .and_then(|apps| apps.into_iter().find(|app| app.app == app_id))
        else {
            return div().into_any_element();
        };
        let picture = app
            .icon_url()
            .and_then(|url| self.images.get(&url).cloned());
        let added = app.added_ms().and_then(|ms| {
            chrono::DateTime::from_timestamp_millis(ms).map(|at| {
                at.with_timezone(&chrono::Local)
                    .format("%b %-d, %Y")
                    .to_string()
            })
        });
        let mut details: Vec<(&'static str, String)> = Vec::new();
        if let Some(date) = added {
            details.push((
                "oauth_entry_clock",
                dc_intl::format(
                    "gcdJ8J",
                    &[(
                        "timestamp",
                        dc_intl::Arg::When {
                            date: date.clone().into(),
                            time: date.into(),
                        },
                    )],
                ),
            ));
        }
        if let Some(user) = &app.user {
            details.push((
                "app_page_user",
                dc_intl::format("qE7oqs", &[("user", user.as_str().into())]),
            ));
        }
        // `MANAGE_WEBHOOKS` and `MANAGE_GUILD`.
        let can_manage_webhooks = self.my_permission(guild, None, 1 << 29);
        let can_remove = self.my_permission(guild, None, super::server_members::MANAGE_GUILD);
        let webhooks = app_webhooks(&app.app, self.stores.integrations.webhooks_for_guild(guild));
        // `header_-KMphV`: 24px down, 16px under, the 24px icon 8px before `heading-sm/semibold`.
        let section_header = |glyph: &'static str, title: &'static str| {
            div()
                .mt(px(24.))
                .mb(px(16.))
                .flex()
                .items_center()
                .child(icon(glyph, 24., "--interactive-text-default").mr(px(8.)))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t(title)),
                )
        };
        // `emptyCard_vPsX0I`: 36px 16px, centred `body-sm/normal` in `text-muted`.
        let empty_card = |key: &'static str| {
            div()
                .py(px(36.))
                .px(px(16.))
                .rounded(px(8.))
                .bg(color("--card-background-default"))
                .flex()
                .justify_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(dc_intl::t(key))
        };
        let divider = || div().my(px(32.)).h(px(1.)).bg(color("--border-subtle"));
        let bot_section = app.bot.as_ref().map(|(name, verified)| {
            let body = if app.bot_scope {
                // `ek`'s top row (`card_uhMkuK`): the 32px picture 8px before the bot's name and its tag.
                let tag = if *verified { "xxcTGy" } else { "AOdOYr" };
                div()
                    .mb(px(16.))
                    .py(px(20.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .bg(color("--card-background-default"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .size(px(32.))
                            .mr(px(8.))
                            .flex_none()
                            .rounded_full()
                            .overflow_hidden()
                            .bg(color("--background-surface-high"))
                            .children(
                                picture
                                    .clone()
                                    .map(|picture| img(picture).size(px(32.)).rounded_full()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-strong"))
                            .child(name.clone()),
                    )
                    .child(
                        // GAP: the bot tag component (`eC.A`) is drawn as the list's small tag.
                        div()
                            .ml(px(4.))
                            .px(px(4.))
                            .rounded(px(4.))
                            .bg(color("--control-brand-foreground"))
                            .text_size(px(10.))
                            .line_height(px(15.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t(tag).to_uppercase()),
                    )
                    .into_any_element()
            } else {
                empty_card("pfLnza").into_any_element()
            };
            div()
                .mb(px(32.))
                .child(section_header("oauth_entry_robot", "AOdOYr"))
                .child(body)
        });
        let breadcrumb = div()
            .mb(px(20.))
            .flex()
            .items_center()
            .text_size(px(20.))
            .line_height(px(24.))
            .font_weight(FontWeight(600.))
            .child(
                div()
                    .id("server-app-back")
                    .cursor_pointer()
                    .text_color(color("--text-default"))
                    .child("Integrations")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.server_settings.as_mut() {
                            open.app_open = None;
                        }
                        cx.notify();
                    })),
            )
            .child(div().w(px(18.)).flex().justify_center().child(icon(
                "settings_chevron",
                12.,
                "--text-muted",
            )))
            .child(
                div()
                    .text_color(color("--text-strong"))
                    .child(app.name.clone()),
            );
        let header = div()
            .flex()
            .gap(px(16.))
            .when_some(app.icon_url(), |this, _| {
                this.child(
                    div()
                        .size(px(48.))
                        .flex_none()
                        .rounded_full()
                        .overflow_hidden()
                        .bg(color("--background-surface-high"))
                        .children(
                            picture
                                .clone()
                                .map(|picture| img(picture).size(px(48.)).rounded_full()),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(
                        div()
                            .mb(px(8.))
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(app.name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .children(details.into_iter().map(|(glyph, text)| {
                                // `detailsWrapper__wphjE headerDetailsWrapper__wphjE`.
                                div()
                                    .mt(px(4.))
                                    .mr(px(40.))
                                    .flex()
                                    .items_center()
                                    .child(
                                        icon(glyph, 12., "--interactive-text-default").mr(px(4.)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(text),
                                    )
                            })),
                    ),
            );
        let webhooks_body = if webhooks > 0 {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(dc_intl::format(
                    "6HqDfT",
                    &[("count", (webhooks as f64).into())],
                ))
                .into_any_element()
        } else {
            empty_card(if can_manage_webhooks {
                "ahPd2Y"
            } else {
                "axqYMl"
            })
            .into_any_element()
        };
        let remove = div()
            .mb(px(32.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t(if can_remove { "hdneL4" } else { "xRCMqx" })),
            )
            .child(
                // `critical-primary`, `sm`: opens `e8`'s confirmation.
                div()
                    .id("server-app-remove")
                    .flex_none()
                    .h(px(32.))
                    .px(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-critical-primary-border-default"))
                    .bg(color("--control-critical-primary-background-default"))
                    .when(!can_remove, |this| this.opacity(0.5))
                    .when(can_remove, |this| this.cursor_pointer())
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("ebGf4m"))
                    .on_click({
                        let integration = app.integration.clone();
                        let name = app.name.clone();
                        cx.listener(move |this, _, _, cx| {
                            if !can_remove {
                                return;
                            }
                            if let Some(open) = this.server_settings.as_mut() {
                                open.app_remove = Some((integration.clone(), name.clone()));
                            }
                            cx.notify();
                        })
                    }),
            );
        div()
            .flex()
            .flex_col()
            .child(breadcrumb)
            .child(header)
            .children(app.description.clone().map(|text| {
                // `headerDescription_vPsX0I`: 14px, 16px under the header.
                div()
                    .mt(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(text)
            }))
            .child(divider())
            .children(bot_section)
            .child(
                div()
                    .mb(px(32.))
                    .child(section_header("integrations_webhook", "t9ZX/I"))
                    .child(webhooks_body),
            )
            .child(divider())
            .child(remove)
            .into_any_element()
    }

    /// `e8` of module 986057 for an app without a subscription listing: module 732159's confirmation ("Remove
    /// {app}", "Are you sure you want to remove **{app}**?", Cancel and the `critical-primary` Remove App). Remove App
    /// is `disableIntegration`, `DELETE /guilds/{id}/integrations/{integration}`, a `Request::Rest` that inert mode
    /// refuses like every other write.
    pub(super) fn app_remove_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let (integration, name) = open.app_remove.clone()?;
        let guild = open.guild;
        let close = |this: &mut Root| {
            if let Some(open) = this.server_settings.as_mut() {
                open.app_remove = None;
            }
        };
        let runs = dc_intl::format_runs("FGE8ya", &[("applicationName", name.as_str().into())]);
        let subtitle = div()
            .flex()
            .flex_wrap()
            .children(runs.into_iter().map(|(text, tag)| {
                div()
                    .when(tag.is_some(), |this| this.font_weight(FontWeight(600.)))
                    .child(text.replace(' ', "\u{a0}"))
            }))
            .into_any_element();
        Some(super::modal::render(
            super::modal::Spec {
                id: "app-remove-dialog",
                title: dc_intl::format("rL9d/1", &[("applicationName", name.as_str().into())])
                    .into(),
                subtitle: Some(subtitle),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "app-remove-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "app-remove-confirm",
                        dc_intl::t("ebGf4m"),
                        super::modal::Tone::Critical,
                        move |this, _, _| {
                            let _ = this.requests.send(remove_app_request(guild, &integration));
                            close(this);
                        },
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }

    /// `EVWFNr`'s card: the 256x72 picture 24px over the line (`emptyStateSearchImage_2S81sZ`), the line in
    /// `body-sm/normal` `text-muted` inside the 128px card.
    pub(super) fn app_search_empty(&self) -> AnyElement {
        let art = self.images.get(NO_MATCH_ART).cloned();
        div()
            .flex()
            .flex_col()
            .items_center()
            .children(art.map(|art| img(art).w(px(256.)).h(px(72.)).mb(px(-24.))))
            .child(
                div()
                    .w_full()
                    .min_h(px(128.))
                    .py(px(32.))
                    .rounded(px(8.))
                    .bg(color("--card-background-default"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::t("EVWFNr")),
            )
            .into_any_element()
    }
}

/// `disableIntegration` (module 727419): the app leaves the server. A write, so inert mode refuses it.
pub(super) fn remove_app_request(guild: Id, integration: &str) -> crate::net::Request {
    crate::net::Request::Rest {
        key: None,
        method: reqwest::Method::DELETE,
        route: format!("/guilds/{guild}/integrations/{integration}"),
        body: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apps_are_the_discord_integrations_with_an_application() {
        let list = json!([
            {"id": "1", "type": "twitch", "application": {"id": "9", "name": "T"}},
            {"id": "2", "type": "discord", "scopes": ["bot", "applications.commands"],
             "user": {"username": "ann", "discriminator": "0"},
             "application": {"id": "10", "name": "Helper", "icon": "abc",
                             "bot": {"id": "10", "username": "helper", "public_flags": 65536}}},
            {"id": "3", "type": "discord", "application": {"id": "11", "name": "Hook"}},
            {"id": "4", "type": "discord"},
        ]);
        let apps = app_integrations(&list);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "Helper");
        assert_eq!(apps[0].bot, Some(("helper".to_owned(), true)));
        assert!(apps[0].bot_scope);
        assert_eq!(apps[0].user.as_deref(), Some("ann"));
        assert_eq!(
            apps[0].icon_url().as_deref(),
            Some("https://cdn.discordapp.com/app-icons/10/abc.png?size=48&keep_aspect_ratio=false")
        );
        assert_eq!(apps[1].bot, None);
        assert_eq!(apps[0].description, None);
        assert!(matches!(
            remove_app_request(1, "2").inert(),
            crate::net::Inert::Deny
        ));
        assert!(app_matches(&apps[0], "hlp", false));
        assert!(!app_matches(&apps[0], "hlp", true));
        assert!(app_matches(&apps[0], "10", true));
        assert!(app_matches(&apps[1], "  ", false));
        assert_eq!(
            app_webhooks(
                "10",
                &[
                    json!({"application_id": "10"}),
                    json!({"application_id": "1"})
                ]
            ),
            1
        );
    }
}
