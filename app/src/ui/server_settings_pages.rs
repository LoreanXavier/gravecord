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
                            EditorEvent::Submit(_) => this.search_bans(cx),
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
            Page::AuditLog => self.request_audit(guild),
            Page::SafetySetup => self.request_safety(guild),
            Page::Integrations => {
                self.request_image(NO_APPS_ART.to_owned(), NO_APPS_ART.to_owned());
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
        if let Some(guild) = key.strip_prefix(&format!("{BANS_PREFIX}search:")) {
            let found = self
                .fetched
                .get(key)
                .and_then(|body| body.as_array().cloned())
                .unwrap_or_default();
            let list_key = format!("{BANS_PREFIX}{guild}");
            let list = self
                .fetched
                .entry(list_key)
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(list) = list.as_array_mut() {
                for ban in found {
                    if !list
                        .iter()
                        .any(|known| known["user"]["id"] == ban["user"]["id"])
                    {
                        list.push(ban);
                    }
                }
            }
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
        } else if let Some(guild) = key
            .strip_prefix(WEBHOOKS_PREFIX)
            .and_then(|guild| guild.parse::<Id>().ok())
        {
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
        let webhooks: Vec<Value> = self
            .fetched
            .get(&format!("{WEBHOOKS_PREFIX}{guild}"))
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default();
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
        let small_button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(32.))
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
                    small_button("server-integrations-webhook", "Create Webhook")
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
                small_button("server-integrations-followed", "Learn More")
                    .on_click(cx.listener(|_, _, _, cx| cx.open_url("https://support.discord.com/hc/en-us/articles/360028384531")))
                    .into_any_element(),
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
            .child(
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
                    ),
            )
            .into_any_element()
    }

    fn primary_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
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
        let rows = invites.into_iter().map(|invite| {
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
                .child(div().w(px(142.)).flex_none().child(mono(code.clone())))
                .child(div().w(px(48.)).flex_none().child(mono(if max > 0 {
                    format!("{uses}/{max}")
                } else {
                    uses.to_string()
                })))
                .child(div().w(px(95.)).flex_none().child(mono(left)))
                .child(div().w(px(95.)).flex_none())
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
        });
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
                            .child("Active invite links"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("server-invites-pause")
                                    .h(px(40.))
                                    .px(px(15.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--control-critical-secondary-border-default"))
                                    .bg(color("--control-critical-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-critical-secondary-text-default"))
                                    .child("Pause Invites")
                                    .on_click(cx.listener(move |this, _, _, cx| this.open_security_actions(guild, cx))),
                            )
                            .child(Self::primary_button("server-invites-create", "Create Invite Link").on_click(cx.listener(move |this, _, _, cx| {
                                this.server_settings = None;
                                this.open_invite(cx);
                            }))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .child(head("Inviter", 203.))
                    .child(head("Invite Code", 163.))
                    .child(head("Uses", 71.))
                    .child(head("Expires", 121.))
                    .child(head("Roles", 102.)),
            )
            .children(rows)
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
        let loaded = self.fetched.get(&format!("{BANS_PREFIX}{guild}"));
        let bans: Vec<Value> = loaded
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default()
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
        let empty = bans.is_empty() && loaded.is_some();
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
                            if let Some(list) = this
                                .fetched
                                .get_mut(&format!("{BANS_PREFIX}{guild}"))
                                .and_then(|body| body.as_array_mut())
                            {
                                list.retain(|ban| {
                                    ban["user"]["id"].as_str() != Some(user.as_str())
                                });
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

#[cfg(test)]
mod tests {
    use super::*;

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
