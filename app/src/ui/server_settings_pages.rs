//! Server settings pages beyond Server Profile, each measured in `design/dom/settings/guild-<page>-darker-cozy.json`.
//!
//! - Invites (section `INVITES`): every invite of the server (`GET /guilds/{id}/invites`) with its channel under the
//!   inviter; Pause Invites opens Security Actions, Create Invite Link the invite dialog; the X revokes
//!   (`DELETE /invites/{code}`).
//! - Bans (module 989433 `EM`, `EO`, `ED`): `GET /guilds/{id}/bans?limit=1000`, the search asks
//!   `GET /guilds/{id}/bans/search` (module 778858 `searchGuildBans`) and filters as `EM`'s matcher does; a row opens
//!   module 392516's dialog, whose Revoke Ban is `DELETE /guilds/{id}/bans/{user}`.

use super::channel_settings_pages::expires_in;
use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, div, img,
    prelude::*, px,
};
use serde_json::Value;

pub(super) const INVITES_PREFIX: &str = "guild-invites:";
pub(super) const BANS_PREFIX: &str = "guild-bans:";
pub(super) const WEBHOOKS_PREFIX: &str = "guild-webhooks:";
const WEBHOOK_MADE_PREFIX: &str = "guild-webhooks:made:";
/// `emptyStateImage_2S81sZ`: the Bots and Apps picture.
const NO_APPS_ART: &str = "https://cdn.discordapp.com/assets/content/21f7110f79ffef19fffd9358370afbf23891b43abf872f3c8e77ccc59807b058.svg";
/// Module 29093: the dark No Bans picture.
const NO_BANS_ART: &str = "https://discordapp.com/assets/626d8d0fc1024e29.svg";

/// `InviteFlags.IS_APPLICATION_BYPASS` (module 821418): members joining through the link skip join applications.
pub(super) const INVITE_APPLICATION_BYPASS: u64 = 8;

/// A role an invite grants, as the server has it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct InviteRole {
    pub id: Id,
    pub name: String,
    pub color: u32,
}

/// `er` of module 422542: the invite's `roles` (objects with an `id`, or bare ids) that the server still has, in the
/// invite's order.
pub(super) fn invite_roles(guild: Option<&dc_state::Guild>, invite: &Value) -> Vec<InviteRole> {
    let Some(guild) = guild else {
        return Vec::new();
    };
    invite["roles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|role| {
            role["id"]
                .as_str()
                .or_else(|| role.as_str())
                .and_then(|id| id.parse::<Id>().ok())
        })
        .filter_map(|id| {
            let name = guild.roles.get(&id)?.clone();
            let color = guild.role_info.get(&id).map_or(0, |info| info.color);
            Some(InviteRole { id, name, color })
        })
        .collect()
}

/// Bans page state: the search field, the query last searched, and the ban whose dialog is open.
pub(crate) struct BansState {
    pub(super) search: Entity<Editor>,
    pub(super) query: String,
    pub(super) open: Option<Value>,
    _events: Subscription,
}

/// Module 859126 `H`: the query's words, with anything that looks like a user id taken out as an id.
pub(super) fn split_query(query: &str) -> (Vec<String>, Vec<String>) {
    let mut words = Vec::new();
    let mut ids = Vec::new();
    for part in query.split_whitespace() {
        if part.len() >= 17 && part.chars().all(|c| c.is_ascii_digit()) {
            ids.push(part.to_owned());
        } else {
            words.push(part.to_owned());
        }
    }
    (words, ids)
}

/// `EM`'s matcher: no query shows every ban; otherwise an id match, or the first word inside the username or global
/// name.
fn ban_matches(user: &Value, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let (words, ids) = split_query(query);
    let id = user["id"].as_str().unwrap_or_default();
    if ids.iter().any(|known| known == id) {
        return true;
    }
    let Some(word) = words.first().map(|word| word.to_lowercase()) else {
        return false;
    };
    let has = |key: &str| {
        user[key]
            .as_str()
            .is_some_and(|name| name.to_lowercase().contains(&word))
    };
    has("username") || has("global_name")
}

impl Root {
    /// Text with one link inside that wraps with it, as the official `anchor` inside a paragraph does.
    pub(super) fn text_with_link(
        &self,
        id: &'static str,
        before: &str,
        link: &str,
        after: &str,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        self.text_with_styled_link(
            id,
            (before, link, after),
            gpui::HighlightStyle {
                color: Some(color("--text-link").into()),
                ..Default::default()
            },
            cx,
            run,
        )
    }

    /// The same with its own look for the link: an inline notice's is underlined in the text's color.
    pub(super) fn text_with_styled_link(
        &self,
        id: &'static str,
        (before, link, after): (&str, &str, &str),
        style: gpui::HighlightStyle,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        let text = format!("{before}{link}{after}");
        let range = before.len()..before.len() + link.len();
        let styled = gpui::StyledText::new(text).with_highlights([(range.clone(), style)]);
        let root = cx.entity().downgrade();
        gpui::InteractiveText::new(id, styled)
            .on_click(vec![range], move |_, _, cx| {
                let _ = root.update(cx, |this, cx| {
                    run(this, cx);
                    cx.notify();
                });
            })
            .into_any_element()
    }

    /// What a page lists, asked for when the page opens.
    pub(super) fn request_server_page(
        &mut self,
        guild: Id,
        page: super::server_settings::Page,
        cx: &mut Context<Self>,
    ) {
        use super::server_settings::Page;
        match page {
            Page::Invites => {
                let _ = self.requests.send(Request::Fetch {
                    key: format!("{INVITES_PREFIX}{guild}"),
                    route: format!("/guilds/{guild}/invites"),
                });
            }
            Page::Bans => {
                self.request_image(NO_BANS_ART.to_owned(), NO_BANS_ART.to_owned());
                let _ = self.requests.send(Request::Fetch {
                    key: format!("{BANS_PREFIX}{guild}"),
                    route: format!("/guilds/{guild}/bans?limit=1000"),
                });
                if self
                    .server_settings
                    .as_ref()
                    .is_some_and(|open| open.bans.is_none())
                {
                    let search = cx.new(|cx| {
                        let mut editor = Editor::new(super::editor_style(), cx);
                        editor.set_placeholder("Search Bans by User Id or Username".to_owned());
                        editor
                    });
                    let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
                        match event {
                            EditorEvent::Submit(text) => {
                                editor.update(cx, |editor, _| editor.set_text(text));
                                this.search_bans(cx)
                            }
                            // Emptying the field shows every ban again (`EO`'s onChange).
                            EditorEvent::Edited => {
                                if editor.read(cx).text().trim().is_empty()
                                    && let Some(bans) = this
                                        .server_settings
                                        .as_mut()
                                        .and_then(|open| open.bans.as_mut())
                                {
                                    bans.query.clear();
                                }
                            }
                            EditorEvent::Cancel => this.server_settings = None,
                        }
                        cx.notify();
                    });
                    if let Some(open) = self.server_settings.as_mut() {
                        open.bans = Some(BansState {
                            search,
                            query: String::new(),
                            open: None,
                            _events: events,
                        });
                    }
                }
            }
            Page::Emoji => self.request_emojis(guild),
            Page::Members => self.request_members_page(guild, cx),
            Page::Engagement => self.request_engagement(guild),
            Page::Soundboard => self.request_sounds(guild),
            Page::Stickers => self.request_stickers(guild),
            Page::Access => self.request_access(guild, cx),
            Page::AutoMod => self.request_automod(guild, cx),
            Page::AuditLog => self.request_audit(guild),
            Page::SafetySetup => {
                self.request_safety(guild);
                let community = self.stores.guild(guild).is_some_and(|known| {
                    known.features.iter().any(|feature| feature == "COMMUNITY")
                });
                if community {
                    self.request_community_safety(guild, cx);
                }
            }
            Page::Insights => self.request_insights(guild),
            Page::Welcome => self.request_welcome(guild),
            Page::Landing => self.request_landing(guild),
            Page::BoostPerks => self.request_boost(guild),
            Page::Tag => self.request_tag(guild),
            Page::Community => self.request_community(guild),
            Page::Onboarding => self.request_onboarding_page(guild, cx),
            Page::Integrations => {
                self.request_image(NO_APPS_ART.to_owned(), NO_APPS_ART.to_owned());
                self.request_app_integrations(guild);
                self.ensure_app_search(cx);
                let _ = self.requests.send(Request::Fetch {
                    key: format!("{WEBHOOKS_PREFIX}{guild}"),
                    route: format!("/guilds/{guild}/webhooks"),
                });
            }
            Page::Template => self.request_template_page(guild, cx),
            Page::Roles => {
                self.request_roles_page();
                self.request_roles(guild, cx);
            }
            _ => {}
        }
    }

    /// Search: an empty query lists everything; otherwise the server is asked and the list filtered.
    fn search_bans(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        let guild = open.guild;
        let Some(bans) = open.bans.as_mut() else {
            return;
        };
        let query = bans.search.read(cx).text().trim().to_owned();
        bans.query = query.clone();
        if query.is_empty() {
            return;
        }
        let (words, ids) = split_query(&query);
        let mut route = format!("/guilds/{guild}/bans/search?limit=10");
        if let Some(word) = words.first() {
            route.push_str(&format!("&query={}", super::gifs::encode_query(word)));
        }
        for id in &ids {
            route.push_str(&format!("&user_ids={id}"));
        }
        let _ = self.requests.send(Request::Fetch {
            key: format!("{BANS_PREFIX}search:{guild}"),
            route,
        });
    }

    /// Search results join the loaded bans (`GUILD_SETTINGS_LOADED_BANS_BATCH`).
    pub(super) fn server_page_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        // The ban list lives in GuildSettingsStore: a list answer replaces it (`GUILD_SETTINGS_LOADED_BANS`), a search
        // answer joins it (`..._BATCH`), and the ban events change it.
        let listed = self
            .fetched
            .get(key)
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default();
        if let Some(guild) = key
            .strip_prefix(&format!("{BANS_PREFIX}search:"))
            .and_then(|guild| guild.parse::<Id>().ok())
        {
            self.stores.guild_bans.loaded_batch(guild, &listed);
        } else if let Some(guild) = key
            .strip_prefix(BANS_PREFIX)
            .and_then(|guild| guild.parse::<Id>().ok())
        {
            self.stores.guild_bans.loaded(guild, &listed);
        }
        let users: Vec<User> = self
            .fetched
            .get(key)
            .and_then(|body| body.as_array().cloned())
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                User::from_value(if entry["inviter"].is_object() {
                    &entry["inviter"]
                } else {
                    &entry["user"]
                })
            })
            .collect();
        for user in users {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
        }
        cx.notify();
    }

    /// A webhook was created: list them again.
    pub(super) fn webhooks_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(guild) = key.strip_prefix(WEBHOOK_MADE_PREFIX)
            && let Ok(guild) = guild.parse::<Id>()
        {
            let _ = self.requests.send(Request::Fetch {
                key: format!("{WEBHOOKS_PREFIX}{guild}"),
                route: format!("/guilds/{guild}/webhooks"),
            });
        } else if let Some(guild) = key.strip_prefix(super::server_webhooks::WEBHOOK_SAVED_PREFIX)
            && let Ok(guild) = guild.parse::<Id>()
        {
            let _ = self.requests.send(Request::Fetch {
                key: format!("{WEBHOOKS_PREFIX}{guild}"),
                route: format!("/guilds/{guild}/webhooks"),
            });
        } else if let Some((guild, channel)) = key
            .strip_prefix(super::gateway_screens::WEBHOOKS_CHANNEL_PREFIX)
            .and_then(|rest| rest.split_once(':'))
            .and_then(|(guild, channel)| {
                Some((guild.parse::<Id>().ok()?, channel.parse::<Id>().ok()?))
            })
        {
            // `fetchForChannel`'s answer replaces that channel's webhooks and keeps the server's others.
            let list = self.fetched[key].as_array().cloned().unwrap_or_default();
            self.stores
                .integrations
                .webhooks_loaded(guild, Some(channel), &list);
            self.request_webhook_images(guild);
        } else if let Some(guild) = key
            .strip_prefix(WEBHOOKS_PREFIX)
            .and_then(|guild| guild.parse::<Id>().ok())
        {
            // The server's list goes through WebhooksStore, which the pages read.
            let list = self.fetched[key].as_array().cloned().unwrap_or_default();
            self.stores.integrations.webhooks_loaded(guild, None, &list);
            self.request_webhook_images(guild);
        }
        cx.notify();
    }

    /// Create Webhook on the server's first text channel, named as the channel page names them (module 824953).
    pub(super) fn create_server_webhook(&mut self, guild: Id) {
        let channel = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .and_then(|known| {
                known
                    .channels
                    .iter()
                    .find(|channel| matches!(channel.kind, dc_state::ChannelKind::Text))
                    .map(|channel| channel.id)
            });
        let Some(channel) = channel else {
            return;
        };
        let name = super::channel_settings_pages::WEBHOOK_NAMES
            [(chrono::Utc::now().timestamp_subsec_nanos() % 2) as usize];
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{WEBHOOK_MADE_PREFIX}{guild}")),
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/webhooks"),
            body: Some(serde_json::json!({"name": name})),
        });
    }

    /// Integrations (module 989433 `uM`, measured in `guild-integrations-darker-cozy.json`): the intro, the Webhooks
    /// and Channels Followed cards, then Bots and Apps. The webhook editor behind View Webhooks is not drawn yet.
    pub(super) fn server_integrations_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let webhooks: Vec<Value> = self.stores.integrations.webhooks_for_guild(guild).to_vec();
        // A followed channel's webhook has type 2, an incoming one type 1.
        let incoming = webhooks
            .iter()
            .filter(|hook| hook["type"].as_u64() == Some(1))
            .count();
        let followed = webhooks
            .iter()
            .filter(|hook| hook["type"].as_u64() == Some(2))
            .count();
        let plural = |count: usize, one: &str| {
            if count == 1 {
                format!("1 {one}")
            } else {
                format!("{count} {one}s")
            }
        };
        // The button's width is the text's in Chromium (`button_F7qvhY sm`: 123.125 and 93.2656 in the dump); gpui's
        // text is a little wider, so the dump's width is fixed here.
        let small_button = |id: &'static str, label: &'static str, width: f32| {
            div()
                .id(id)
                .h(px(32.))
                .w(px(width))
                .justify_center()
                .px(px(11.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--control-primary-border-default"))
                .bg(color("--control-primary-background-default"))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .child(label)
        };
        let card =
            |glyph: &'static str, title: &'static str, detail: String, button: AnyElement| {
                div()
                    .h(px(88.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .bg(color("--card-background-default"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(48.))
                            .flex_none()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(glyph, 24., "--interactive-text-default")),
                    )
                    .child(
                        div()
                            .ml(px(8.))
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-default"))
                                    .child(detail),
                            ),
                    )
                    .child(button)
            };
        let divider = || div().my(px(32.)).h(px(1.)).bg(color("--border-subtle"));
        let art = self.images.get(NO_APPS_ART).cloned();
        // `t0`'s `eN`: the apps' cards (the search over more than four), or "No installed apps matched your search."
        // when the search leaves none; with no apps at all, the empty state below.
        let apps_section: Option<AnyElement> = self
            .app_integration_list(guild)
            .filter(|apps| !apps.is_empty())
            .map(|apps| {
                let open = self.server_settings.as_ref();
                let query = open.map(|open| open.apps_query.clone()).unwrap_or_default();
                let many = apps.len() > 100;
                let cards: Vec<AnyElement> = apps
                    .iter()
                    .filter(|app| super::integration_apps::app_matches(app, &query, many))
                    .map(|app| {
                        let hooks = super::integration_apps::app_webhooks(&app.app, &webhooks);
                        self.app_integration_card(guild, app, hooks, cx)
                    })
                    .collect();
                let search = (apps.len() > 4)
                    .then(|| open.and_then(|open| open.apps_search.as_ref()))
                    .flatten()
                    .map(|(editor, _)| {
                        // `md_0PLWhS`: 40 tall, the 30px icon box 4px in.
                        div()
                            .mb(px(16.))
                            .h(px(40.))
                            .pl(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .size(px(30.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("picker_search", 16., "--input-text-default")),
                            )
                            .child(div().flex_1().min_w(px(0.)).child(editor.clone()))
                    });
                let empty = cards.is_empty();
                div()
                    .flex()
                    .flex_col()
                    .children(search)
                    .children(cards)
                    .when(empty, |this| this.child(self.app_search_empty()))
                    .into_any_element()
            });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(20.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Integrations"),
            )
            .child(
                div().text_size(px(14.)).line_height(px(18.)).text_color(color("--text-default")).child(self.text_with_link(
                    "server-integrations-help",
                    "Customize your server with integrations. Manage webhooks, followed channels, and apps, as well as Twitch and YouTube settings for creators. ",
                    "Learn more about managing integrations.",
                    "",
                    cx,
                    |_, cx| cx.open_url("https://support.discord.com/hc/en-us/articles/360045093012"),
                )),
            )
            .child(divider())
            .child(card(
                "integrations_webhook",
                "Webhooks",
                plural(incoming, "webhook"),
                if incoming == 0 {
                    small_button("server-integrations-webhook", "Create Webhook", 123.125)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.create_server_webhook(guild);
                            cx.notify();
                        }))
                        .into_any_element()
                } else {
                    div()
                        .id("server-integrations-webhooks")
                        .cursor_pointer()
                        .child(icon("roles_arrow", 24., "--interactive-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| this.show_webhooks(guild, cx)))
                        .into_any_element()
                },
            ))
            .child(div().h(px(16.)))
            .child(card(
                "integrations_followed",
                "Channels Followed",
                plural(followed, "channel"),
                // `t0`: with followed channels the card's chevron opens their page, else Learn More opens the article.
                if followed > 0 {
                    div()
                        .id("server-integrations-followed")
                        .cursor_pointer()
                        .child(icon("roles_arrow", 24., "--interactive-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(open) = this.server_settings.as_mut() {
                                open.followed_open = true;
                                open.scroll.set_offset(gpui::point(px(0.), px(0.)));
                            }
                            let urls: Vec<String> = this
                                .stores
                                .integrations
                                .webhooks_for_guild(guild)
                                .iter()
                                .filter(|hook| hook["type"].as_u64() == Some(2))
                                .map(followed_avatar)
                                .collect();
                            for url in urls {
                                this.request_image(url.clone(), url);
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                } else {
                    small_button("server-integrations-followed", "Learn More", 93.2656)
                        .on_click(cx.listener(|_, _, _, cx| cx.open_url("https://support.discord.com/hc/en-us/articles/360028384531")))
                        .into_any_element()
                },
            ))
            .child(divider())
            .child(
                div()
                    .mb(px(16.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Bots and Apps"),
            )
            .child(apps_section.unwrap_or_else(|| {
                div()
                    .pt(px(32.))
                    .pb(px(32.))
                    .rounded(px(8.))
                    .bg(color("--card-background-default"))
                    .flex()
                    .flex_col()
                    .items_center()
                    .children(art.map(|art| img(art).w(px(249.)).h(px(140.)).mb(px(24.))))
                    .child(
                        div()
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child("You don't have any apps yet!"),
                    )
                    .child(
                        div().mt(px(8.)).text_size(px(14.)).line_height(px(18.)).text_color(color("--text-default")).child(self.text_with_link(
                            "server-integrations-directory",
                            "Visit the ",
                            "App Directory",
                            " to add apps to your server",
                            cx,
                            move |_, cx| cx.open_url(&format!("https://discord.com/application-directory?guild_id={guild}")),
                        )),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// Channels Followed (`tA` of module 986057): the breadcrumb, `5u+aV1` with its Learn more link, the divider, then
    /// one card per followed channel (`eH`: `header_mWF9-i` 20px 16px, the webhook's 48px avatar, its name, the
    /// announcement channel and "from {server}", the expand chevron), or "You're not following any channels" with
    /// Learn More. A card opens to its name, its channel and Unfollow.
    pub(super) fn followed_channels_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let hooks: Vec<Value> = self
            .stores
            .integrations
            .webhooks_for_guild(guild)
            .iter()
            .filter(|hook| hook["type"].as_u64() == Some(2))
            .cloned()
            .collect();
        let breadcrumb = div()
            .mb(px(20.))
            .flex()
            .items_center()
            .text_size(px(20.))
            .line_height(px(24.))
            .font_weight(FontWeight(600.))
            .child(
                div()
                    .id("server-followed-back")
                    .cursor_pointer()
                    .text_color(color("--text-default"))
                    .child("Integrations")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.server_settings.as_mut() {
                            open.followed_open = false;
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
                    .child(dc_intl::t("OrV60r")),
            );
        let runs = dc_intl::format_runs("5u+aV1", &[]);
        let (mut before, mut link) = (String::new(), String::new());
        for (text, tag) in runs {
            if tag.is_some() {
                link.push_str(&text);
            } else {
                before.push_str(&text);
            }
        }
        let intro = div()
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-default"))
            .child(
                self.text_with_link("server-followed-help", &before, &link, "", cx, |_, cx| {
                    cx.open_url("https://support.discord.com/hc/en-us/articles/360028384531")
                }),
            );
        let rows: Vec<AnyElement> = hooks
            .iter()
            .map(|hook| {
                let id = hook["id"].as_str().unwrap_or_default().to_owned();
                let avatar = followed_avatar(hook);
                let picture = self.images.get(&avatar).cloned();
                let channel = hook["source_channel"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let source = hook["source_guild"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let from = dc_intl::format_runs("H/jJ6R", &[])
                    .into_iter()
                    .map(|(text, _)| text)
                    .collect::<String>();
                let expanded = self
                    .server_settings
                    .as_ref()
                    .and_then(|open| open.followed_expanded.as_deref())
                    == Some(id.as_str());
                let name = hook["name"].as_str().unwrap_or_default().to_owned();
                let posted_in = hook["channel_id"]
                    .as_str()
                    .and_then(|channel| channel.parse::<Id>().ok())
                    .and_then(|channel| {
                        self.stores.guild(guild).and_then(|known| {
                            known
                                .channels
                                .iter()
                                .find(|c| c.id == channel)
                                .map(|c| c.name.clone())
                        })
                    })
                    .unwrap_or_default();
                let toggle = id.clone();
                let header =
                    div()
                        .id(SharedString::from(format!("followed-{id}")))
                        .py(px(20.))
                        .px(px(16.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(open) = this.server_settings.as_mut() {
                                open.followed_expanded =
                                    if open.followed_expanded.as_deref() == Some(toggle.as_str()) {
                                        None
                                    } else {
                                        Some(toggle.clone())
                                    };
                            }
                            cx.notify();
                        }))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(16.))
                                .child(
                                    div()
                                        .size(px(48.))
                                        .flex_none()
                                        .rounded_full()
                                        .overflow_hidden()
                                        .bg(color("--background-surface-high"))
                                        .children(picture.map(|picture| {
                                            img(picture).size(px(48.)).rounded_full()
                                        })),
                                )
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
                                                .child(
                                                    hook["name"]
                                                        .as_str()
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .mt(px(4.))
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .child(
                                                            icon(
                                                                "announcement_channel",
                                                                12.,
                                                                "--interactive-text-default",
                                                            )
                                                            .mr(px(4.)),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_color(color("--text-default"))
                                                                .child(channel),
                                                        ),
                                                )
                                                // `guildSource_mWF9-i` (muted, 8px either side) around `sourceName_mWF9-i`.
                                                .child(
                                                    div()
                                                        .mx(px(8.))
                                                        .flex()
                                                        .items_center()
                                                        .text_color(color("--text-muted"))
                                                        .child(from.trim_end().to_owned())
                                                        .child(
                                                            div()
                                                                .ml(px(4.))
                                                                .mr(px(8.))
                                                                .text_color(color("--text-default"))
                                                                .child(source),
                                                        ),
                                                ),
                                        ),
                                ),
                        )
                        // `eG`: the chevron turns over while the card is open.
                        .child(
                            icon("chevron_down", 24., "--interactive-text-default").when(
                                expanded,
                                |icon| {
                                    icon.with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(0.5),
                                    ))
                                },
                            ),
                        );
                // `body_mWF9-i` (0 16px 24px): the 24px-under divider, Announcement Name and Post To 16px apart,
                // the divider, then Unfollow. GAP: the two fields show the webhook's values; editing them is not
                // built.
                let body = expanded.then(|| {
                    let unfollow = (id.clone(), name.clone());
                    let field = |label: &'static str, value: String, select: bool| {
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .mb(px(8.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t(label)),
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
                                    .justify_between()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(value)
                                    .when(select, |this| {
                                        this.child(icon("chevron_down", 16., "--icon-subtle"))
                                    }),
                            )
                    };
                    div()
                        .px(px(16.))
                        .pb(px(24.))
                        .child(div().mb(px(24.)).h(px(1.)).bg(color("--border-subtle")))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(24.))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(16.))
                                        .child(field("I1o5gV", name.clone(), false))
                                        .child(field("4TuWfc", format!("#{posted_in}"), true)),
                                )
                                .child(div().h(px(1.)).bg(color("--border-subtle")))
                                .child(
                                    div().flex().child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "followed-unfollow-{id}"
                                            )))
                                            .h(px(32.))
                                            .px(px(11.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color(
                                                "--control-critical-secondary-border-default",
                                            ))
                                            .bg(color(
                                                "--control-critical-secondary-background-default",
                                            ))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color(
                                                "--control-critical-secondary-text-default",
                                            ))
                                            .child(dc_intl::t("CMy0Cj"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(open) = this.server_settings.as_mut() {
                                                    open.followed_unfollow = Some(unfollow.clone());
                                                }
                                                cx.notify();
                                            })),
                                    ),
                                ),
                        )
                });
                div()
                    .mb(px(16.))
                    .rounded(px(8.))
                    .bg(color("--card-background-default"))
                    .child(header)
                    .children(body)
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("gzuVH0")),
                )
                .child(div().mt(px(16.)).child(
                    Self::primary_button("server-followed-learn", dc_intl::t("ZwSt+T")).on_click(
                        cx.listener(|_, _, _, cx| {
                            cx.open_url(
                                "https://support.discord.com/hc/en-us/articles/360028384531",
                            )
                        }),
                    ),
                ))
        });
        div()
            .flex()
            .flex_col()
            .child(breadcrumb)
            .child(intro)
            .child(div().my(px(32.)).h(px(1.)).bg(color("--border-subtle")))
            .children(rows)
            .children(empty)
            .into_any_element()
    }

    /// Unfollow's confirmation (`eR.A` in `eH`): "Unfollow {name}", why, Cancel and Unfollow. Unfollow is
    /// `DELETE /webhooks/{id}`, a `Request::Rest` that inert mode refuses like every other write.
    pub(super) fn unfollow_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let (hook, name) = open.followed_unfollow.clone()?;
        let close = |this: &mut Root| {
            if let Some(open) = this.server_settings.as_mut() {
                open.followed_unfollow = None;
            }
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "unfollow-dialog",
                title: dc_intl::format("gBKqZ0", &[("name", name.as_str().into())]).into(),
                subtitle: Some(div().child(dc_intl::t("zO9jrD")).into_any_element()),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "unfollow-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "unfollow-confirm",
                        dc_intl::t("CMy0Cj"),
                        super::modal::Tone::Critical,
                        move |this, _, _| {
                            let _ = this.requests.send(Request::Rest {
                                key: None,
                                method: reqwest::Method::DELETE,
                                route: format!("/webhooks/{hook}"),
                                body: None,
                            });
                            close(this);
                        },
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }

    pub(super) fn primary_button(
        id: &'static str,
        label: &'static str,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(40.))
            .px(px(15.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-primary-border-default"))
            .bg(color("--control-primary-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .child(label)
    }

    /// `Wr` and `Fi` of module 347951: invites are paused (`INVITES_DISABLED`, or `invites_disabled_until` still to
    /// come), and the account may manage the server.
    pub(super) fn invites_pause_state(&self, guild: Id) -> (bool, bool) {
        let Some(known) = self.stores.guild(guild) else {
            return (false, false);
        };
        let now = chrono::Utc::now().timestamp_millis();
        let paused = known.features.iter().any(|f| f == "INVITES_DISABLED")
            || known
                .invites_disabled_until
                .as_deref()
                .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
                .is_some_and(|until| until.timestamp_millis() > now);
        // Not in the pinned client: ShowHiddenThings' showInvitesPaused makes `Fi` answer yes
        // (`plugin_hooks/show_hidden_things.rs`).
        let allowed = paused && super::plugin_hooks::show_hidden_invites_paused()
            || self.stores.me().is_some_and(|me| {
                dc_state::permissions::can_in_guild(known, me, dc_state::permissions::MANAGE_GUILD)
            });
        (paused, allowed)
    }

    /// The Roles cell (`z` of module 422542, `roleContainer_U5Umrb`: a row 4px apart): the first role as the pill of
    /// module 950072 (the role's colour at `27` alpha behind the pill of 417454) and, for more, a 24px square
    /// (`otherRoles_U5Umrb`, `--background-base-lower`, `--radius-xs`) with "+N" (`body-xs/medium`, `text-strong`)
    /// whose tooltip is "View All Roles"; its click lists every role (module 551309, `invite-roles-menu`).
    pub(super) fn invite_roles_cell(
        &self,
        code: &str,
        roles: &[InviteRole],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let first = roles.first()?;
        let tint = |alpha: f32| {
            let mut tint: gpui::Rgba = if first.color == 0 {
                color("--primary-300")
            } else {
                gpui::rgb(first.color)
            };
            tint.a *= alpha;
            tint
        };
        let first_id = first.id;
        let pill = div()
            .id(SharedString::from(format!(
                "server-invite-role-{code}-{first_id}"
            )))
            .aria_label(first.name.clone())
            .h(px(24.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(tint(f32::from(0x27_u8) / 255.))
            .flex()
            .items_center()
            .min_w(px(0.))
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                    if this.open_role_chip_menu(first_id, event.position, cx) {
                        cx.stop_propagation();
                    }
                }),
            )
            .child(
                div().mr(px(4.)).child(
                    div()
                        .mx(px(4.))
                        .size(px(12.))
                        .flex_none()
                        .rounded_full()
                        .bg(tint(1.)),
                ),
            )
            .child(
                div()
                    .mr(px(2.))
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--interactive-text-active"))
                    .child(first.name.clone()),
            );
        let more = roles.len() - 1;
        let names: Vec<(String, u32)> = roles
            .iter()
            .map(|role| (role.name.clone(), role.color))
            .collect();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .min_w(px(0.))
                .child(pill)
                .when(more > 0, |cell| {
                    cell.child(
                        self.with_tip(
                            format!("server-invite-roles-{code}"),
                            super::tooltip::Tip::text(
                                dc_intl::t("DY6n4q"),
                                super::tooltip::Side::Top,
                            ),
                            div()
                                .id(SharedString::from(format!(
                                    "server-invite-more-roles-{code}"
                                )))
                                .size(px(24.))
                                .flex_none()
                                .rounded(px(4.))
                                .bg(color("--background-base-lower"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(format!("+{more}"))
                                .on_click(cx.listener(
                                    move |this, event: &gpui::ClickEvent, _, cx| {
                                        this.open_invite_roles_menu(
                                            event.position(),
                                            names.clone(),
                                            cx,
                                        );
                                    },
                                )),
                            cx,
                        ),
                    )
                })
                .into_any_element(),
        )
    }

    /// The Invite Code cell (`inviteSource` of `ea` in module 422542): the code (`inviteCode_k0kUi1`, monospace, one
    /// line) and, for an invite with `IS_APPLICATION_BYPASS`, `bypass_k0kUi1`: "Bypass Join Applications"
    /// (`body-xs/normal`, `text-muted`) and the `xxs` info icon 4px after it, whose tooltip says what the link does.
    pub(super) fn invite_source_cell(
        &self,
        code: &str,
        invite: &Value,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bypass = invite["flags"].as_u64().unwrap_or(0) & INVITE_APPLICATION_BYPASS != 0;
        div()
            .flex()
            .flex_col()
            .max_w_full()
            .overflow_hidden()
            .child(
                div()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(super::MONO)
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(code.to_owned()),
            )
            .when(bypass, |source| {
                source.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .child(
                            // `text_k0kUi1`: one line, cut with an ellipsis before the icon gives way.
                            div()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("1i1bUl")),
                        )
                        .child(
                            self.with_tip(
                                format!("invite-bypass-{code}"),
                                super::tooltip::Tip::text(
                                    dc_intl::t("jvd/LF"),
                                    super::tooltip::Side::Top,
                                ),
                                div()
                                    .ml(px(4.))
                                    .child(icon("circle_info", 12., "--text-muted")),
                                cx,
                            ),
                        ),
                )
            })
            .into_any_element()
    }
    /// Capture only: `guild-features` lists each server's id, name and features.
    #[cfg(feature = "capture")]
    pub(crate) fn script_guild_features(&self) -> String {
        self.stores
            .guilds
            .iter()
            .map(|guild| {
                format!(
                    "guild {} {:?}: {} (invites paused, may manage: {:?})",
                    guild.id,
                    guild.name,
                    guild.features.join(","),
                    self.invites_pause_state(guild.id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Capture only: `invites demo` adds a local invite to the open server's fetched list (nothing is sent) that
    /// grants every role of the server and bypasses join applications, so the Roles and Invite Code cells show.
    #[cfg(feature = "capture")]
    pub(crate) fn script_invites_demo(&mut self, cx: &mut Context<Self>) -> String {
        let Some(guild) = self.guild else {
            return "invites demo: no server open".to_owned();
        };
        let roles: Vec<Value> = self
            .stores
            .guild(guild)
            .map(|known| {
                let mut ids: Vec<_> = known
                    .roles
                    .keys()
                    .copied()
                    .filter(|id| *id != guild)
                    .collect();
                ids.sort_unstable();
                ids.into_iter()
                    .map(|id| serde_json::json!({"id": id.to_string()}))
                    .collect()
            })
            .unwrap_or_default();
        let invite = serde_json::json!({
            "code": "demoBypass",
            "uses": 1,
            "max_uses": 0,
            "max_age": 0,
            "flags": INVITE_APPLICATION_BYPASS,
            "roles": roles,
            "created_at": chrono::Utc::now().to_rfc3339(),
        });
        let count = roles.len();
        let list = self
            .fetched
            .entry(format!("{INVITES_PREFIX}{guild}"))
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(list) = list.as_array_mut() {
            list.retain(|known| known["code"] != "demoBypass");
            list.push(invite);
        }
        cx.notify();
        format!("invites demo: one invite with {count} roles")
    }

    /// The "+N" of an invite's roles: `invite-roles-menu` (module 551309) at the click.
    fn open_invite_roles_menu(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        roles: Vec<(String, u32)>,
        cx: &mut Context<Self>,
    ) {
        self.context.invite_roles = roles;
        self.open_context_menu(super::context_menu::Target::InviteRoles, at, cx);
    }

    /// Invites (`header_k0kUi1`, `actions_yu8zQD`, the table): columns 190, 142, 48, 95 and 95 wide, 20 apart.
    pub(super) fn server_invites_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let invites: Vec<Value> = self
            .fetched
            .get(&format!("{INVITES_PREFIX}{guild}"))
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default();
        let now = chrono::Utc::now().timestamp_millis();
        let channel_name = |id: &str| {
            self.stores
                .guilds
                .iter()
                .flat_map(|known| known.channels.iter())
                .find(|channel| channel.id.to_string() == id)
                .map(|channel| channel.name.clone())
        };
        let mono = |text: String| {
            div()
                .font_family(super::MONO)
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let head = |text: &'static str, width: f32| {
            div()
                .w(px(width))
                .flex_none()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let rows = invites
            .into_iter()
            .map(|invite| {
                let code = invite["code"].as_str().unwrap_or_default().to_owned();
                let inviter = User::from_value(&invite["inviter"]);
                let picture = inviter
                    .as_ref()
                    .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                let channel = invite["channel"]["name"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| invite["channel"]["id"].as_str().and_then(channel_name))
                    .unwrap_or_default();
                let uses = invite["uses"].as_u64().unwrap_or(0);
                let max = invite["max_uses"].as_u64().unwrap_or(0);
                let created = invite["created_at"]
                    .as_str()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .map_or(now, |at| at.timestamp_millis());
                let left = expires_in(created, invite["max_age"].as_i64().unwrap_or(0), now);
                let group = SharedString::from(format!("server-invite-row-{code}"));
                let revoke = code.clone();
                // `er` of module 422542: the invite's roles as the server has them, then `IS_APPLICATION_BYPASS`
                // (8, module 821418) of its flags.
                let roles = invite_roles(self.stores.guild(guild), &invite);
                let source = self.invite_source_cell(&code, &invite, cx);
                div()
                    .group(group.clone())
                    .relative()
                    .h(px(62.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .gap(px(20.))
                    .items_center()
                    .child(
                        div()
                            .w(px(190.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(div().size(px(24.)).flex_none().rounded_full().children(
                                picture.map(|picture| img(picture).size(px(24.)).rounded_full()),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(
                                                inviter
                                                    .map(|user| user.display_name().to_owned())
                                                    .unwrap_or_default(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(format!("#{channel}")),
                                    ),
                            ),
                    )
                    .child(div().w(px(142.)).flex_none().child(source))
                    .child(div().w(px(48.)).flex_none().child(mono(if max > 0 {
                        format!("{uses}/{max}")
                    } else {
                        uses.to_string()
                    })))
                    .child(div().w(px(95.)).flex_none().child(mono(left)))
                    .child(
                        div()
                            .w(px(95.))
                            .flex_none()
                            .children(self.invite_roles_cell(&code, &roles, cx)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("server-invite-revoke-{code}")))
                            .absolute()
                            .right(px(-31.))
                            .top(px(-12.))
                            .size(px(16.))
                            .cursor_pointer()
                            .opacity(0.)
                            .group_hover(group, |style| style.opacity(1.))
                            .child(icon("modal_close", 16., "--icon-subtle"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.requests.send(Request::Rest {
                                    key: None,
                                    method: reqwest::Method::DELETE,
                                    route: format!("/invites/{revoke}"),
                                    body: None,
                                });
                                if let Some(list) = this
                                    .fetched
                                    .get_mut(&format!("{INVITES_PREFIX}{guild}"))
                                    .and_then(|body| body.as_array_mut())
                                {
                                    list.retain(|invite| {
                                        invite["code"].as_str() != Some(revoke.as_str())
                                    });
                                }
                                cx.notify();
                            })),
                    )
            })
            .collect::<Vec<_>>();
        let linked = self.linked_channels_for_guild(guild, None);
        let has_linked = !linked.is_empty();
        let linked_rows: Vec<AnyElement> = linked
            .into_iter()
            .map(|row| self.linked_channel_invite_row(row, true, cx))
            .collect();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .pb(px(40.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Invites"),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child("Create an invite link anyone can use to join, or invite friends directly."),
                    ),
            )
            .child(
                div()
                    .pb(px(24.))
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .mt(px(11.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(if !rows.is_empty() || has_linked {
                                dc_intl::t("DjWsya")
                            } else {
                                dc_intl::t("AmwmLg")
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child({
                                // `et` of module 422542: "Enable Invites" in the primary colour while invites are
                                // paused (`Wr` of module 347951), else "Pause Invites" (`critical-secondary`); both
                                // open Security Actions, and need Manage Server (`Fi`).
                                let (paused, allowed) = self.invites_pause_state(guild);
                                let (border, background, text) = if paused {
                                    (
                                        "--control-primary-border-default",
                                        "--control-primary-background-default",
                                        "--white",
                                    )
                                } else {
                                    (
                                        "--control-critical-secondary-border-default",
                                        "--control-critical-secondary-background-default",
                                        "--control-critical-secondary-text-default",
                                    )
                                };
                                div()
                                    .id("server-invites-pause")
                                    .h(px(40.))
                                    .px(px(15.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color(border))
                                    .bg(color(background))
                                    .when(allowed, |button| button.cursor_pointer())
                                    .when(!allowed, |button| button.opacity(0.5))
                                    .flex()
                                    .items_center()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color(text))
                                    .child(if paused {
                                        dc_intl::t("/dbw3H")
                                    } else {
                                        dc_intl::t("Uwsjn6")
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if allowed {
                                            this.open_security_actions(guild, cx)
                                        }
                                    }))
                            })
                            .child(Self::primary_button("server-invites-create", "Create Invite Link").on_click(cx.listener(move |this, _, _, cx| {
                                this.server_settings = None;
                                this.open_invite(cx);
                            }))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .child(head(dc_intl::t("EgHyKy"), 203.))
                    .child(head(
                        if has_linked {
                            dc_intl::t("l7VISQ")
                        } else {
                            dc_intl::t("LBlFEN")
                        },
                        163.,
                    ))
                    .child(head(dc_intl::t("erWrmA"), 71.))
                    .child(head(dc_intl::t("1aM27d"), 121.))
                    .child(head(dc_intl::t("LPJmL/"), 102.)),
            )
            .children(rows)
            .children(linked_rows)
            .into_any_element()
    }

    /// Bans (`container_zS9TPG`): the heading, the description with its Safety Setup link, search, then the list in
    /// a 760px card (`scrollerContainer_zS9TPG`) or the No Bans picture.
    pub(super) fn server_bans_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.bans.as_ref())
        else {
            return div().into_any_element();
        };
        let loaded = self.stores.guild_bans.is_loaded(guild);
        let bans: Vec<Value> = self
            .stores
            .guild_bans
            .rows(guild)
            .into_iter()
            .filter(|ban| ban_matches(&ban["user"], &state.query))
            .collect();
        let rows: Vec<_> =
            bans.iter()
                .map(|ban| {
                    let user = User::from_value(&ban["user"]);
                    let picture = user
                        .as_ref()
                        .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                    let global = ban["user"]["global_name"].as_str().map(str::to_owned);
                    let username = ban["user"]["username"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    let id = ban["user"]["id"].as_str().unwrap_or_default().to_owned();
                    let picked = ban.clone();
                    div()
                        .id(SharedString::from(format!("server-ban-{id}")))
                        .flex_none()
                        .h(px(56.))
                        .mb(px(4.))
                        .p(px(8.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-subtle"))
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .child(div().size(px(40.)).flex_none().rounded_full().children(
                            picture.map(|picture| img(picture).size(px(40.)).rounded_full()),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .pl(px(8.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(global.clone().unwrap_or_else(|| username.clone())),
                                )
                                .when(global.is_some(), |this| {
                                    this.child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(username.clone()),
                                    )
                                }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(bans) = this
                                .server_settings
                                .as_mut()
                                .and_then(|open| open.bans.as_mut())
                            {
                                bans.open = Some(picked.clone());
                            }
                            cx.notify();
                        }))
                })
                .collect();
        let art = self.images.get(NO_BANS_ART).cloned();
        let empty = bans.is_empty() && loaded;
        div()
            .pt(px(56.))
            .px(px(24.))
            .pb(px(24.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Server Ban List"),
                    )
                    .child(
                        div()
                            .mb(px(20.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(self.text_with_link(
                                "server-bans-safety",
                                "Bans by default are by account and IP. A user can circumvent an IP ban by using a proxy. Ban circumvention can be made very hard by enabling phone verification in ",
                                "Safety Setup",
                                ".",
                                cx,
                                |this, _| {
                                    if let Some(open) = this.server_settings.as_mut() {
                                        open.page = super::server_settings::Page::SafetySetup;
                                    }
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(40.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .pl(px(4.))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(div().size(px(30.)).flex().items_center().justify_center().child(icon("picker_search", 16., "--input-text-default")))
                                    .child(div().flex_1().min_w(px(0.)).pr(px(12.)).child(state.search.clone())),
                            )
                            .child(
                                Self::primary_button("server-bans-search", "Search")
                                    .w(px(100.))
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| this.search_bans(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .id("server-bans-list")
                    .h(px(760.))
                    .p(px(8.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--card-background-default"))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .when(empty, |this| {
                        this.items_center().child(
                            div()
                                .mt(px(185.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .children(art.map(|art| img(art).w(px(256.)).h(px(212.)).mb(px(40.))))
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .line_height(px(24.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-muted"))
                                        .child("No Bans"),
                                )
                                .child(
                                    div()
                                        .mt(px(8.))
                                        .w(px(300.))
                                        .text_center()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-muted"))
                                        .child("You haven't banned anybody... but if and when you must, do not hesitate!"),
                                ),
                        )
                    })
                    .children(rows),
            )
            .into_any_element()
    }

    /// Module 392516: the banned user's tag, "Ban Reason" over the reason (or "No reason provided"), Revoke Ban and
    /// Done.
    pub(super) fn ban_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let ban = open.bans.as_ref()?.open.clone()?;
        let guild = open.guild;
        let username = ban["user"]["username"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let user = ban["user"]["id"].as_str().unwrap_or_default().to_owned();
        let reason = ban["reason"]
            .as_str()
            .filter(|reason| !reason.is_empty())
            .map_or_else(|| "No reason provided".to_owned(), str::to_owned);
        let close = |this: &mut Root| {
            if let Some(bans) = this
                .server_settings
                .as_mut()
                .and_then(|open| open.bans.as_mut())
            {
                bans.open = None;
            }
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "ban-dialog",
                title: username.into(),
                subtitle: None,
                body: Some(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child("Ban Reason"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-subtle"))
                                .child(reason),
                        )
                        .into_any_element(),
                ),
                buttons: vec![
                    super::modal::button(
                        "ban-revoke",
                        "Revoke Ban",
                        super::modal::Tone::CriticalSecondary,
                        move |this, _, _| {
                            let _ = this.requests.send(Request::Rest {
                                key: None,
                                method: reqwest::Method::DELETE,
                                route: format!("/guilds/{guild}/bans/{user}"),
                                body: None,
                            });
                            if let Ok(revoked) = user.parse::<Id>() {
                                this.stores.guild_bans.revoked(guild, revoked);
                            }
                            close(this);
                        },
                    ),
                    super::modal::button(
                        "ban-done",
                        "Done",
                        super::modal::Tone::Primary,
                        move |this, _, _| close(this),
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }
}

/// `eJ` of module 986057: the webhook's avatar, or the default one (`discriminator: "0000"`).
fn followed_avatar(hook: &Value) -> String {
    let id = hook["id"].as_str().unwrap_or_default();
    match hook["avatar"].as_str() {
        Some(hash) => format!("https://cdn.discordapp.com/avatars/{id}/{hash}.webp?size=80"),
        None => "https://cdn.discordapp.com/embed/avatars/0.png".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_followed_channel_webhook_without_avatar_gets_the_default() {
        assert_eq!(
            followed_avatar(&serde_json::json!({"id": "5"})),
            "https://cdn.discordapp.com/embed/avatars/0.png"
        );
        assert_eq!(
            followed_avatar(&serde_json::json!({"id": "5", "avatar": "ab"})),
            "https://cdn.discordapp.com/avatars/5/ab.webp?size=80"
        );
    }

    /// `er` of module 422542: only the roles the server still has, in the invite's order, with their colours.
    #[test]
    fn an_invite_lists_the_roles_the_server_has() {
        let mut guild = dc_state::Guild::default();
        guild.roles.insert(1, "Mods".into());
        guild.roles.insert(2, "Plain".into());
        guild.role_info.insert(
            1,
            dc_state::RoleInfo {
                color: 0x3498db,
                ..Default::default()
            },
        );
        let invite = serde_json::json!({"roles": [{"id": "2"}, {"id": "9"}, "1"], "flags": 8});
        assert_eq!(
            invite_roles(Some(&guild), &invite),
            vec![
                InviteRole {
                    id: 2,
                    name: "Plain".into(),
                    color: 0,
                },
                InviteRole {
                    id: 1,
                    name: "Mods".into(),
                    color: 0x3498db,
                },
            ]
        );
        assert!(invite_roles(None, &invite).is_empty());
        assert_ne!(
            invite["flags"].as_u64().unwrap() & INVITE_APPLICATION_BYPASS,
            0
        );
        assert_eq!(dc_intl::t("1i1bUl"), "Bypass Join Applications");
        assert_eq!(dc_intl::t("DY6n4q"), "View All Roles");
        assert_eq!(dc_intl::t("/dbw3H"), "Enable Invites");
    }

    #[test]
    fn bans_filter_like_em() {
        let user = serde_json::json!({"id": "903573240566399026", "username": "loreanxavier", "global_name": "Lorean"});
        assert!(ban_matches(&user, ""));
        assert!(ban_matches(&user, "LOREAN"));
        assert!(ban_matches(&user, "903573240566399026"));
        assert!(!ban_matches(&user, "mor"));
        assert_eq!(
            split_query("abc 903573240566399026"),
            (
                vec!["abc".to_owned()],
                vec!["903573240566399026".to_owned()]
            )
        );
    }
}
