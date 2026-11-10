//! The full profile (`user-profile-modal-v2`, module 927071): 960 wide and the window's height, a 400px identity
//! column (banner, 120px avatar, names, Add Friend / Message / More, bio, Member Since, the private note) beside
//! tabs for recent activity (the content-inventory outbox), mutual friends and mutual servers. Measured in
//! `design/dom/modals/full-profile-{activity,friends,servers}-darker-cozy.json`.
//!
//! Activity cards follow module 506326: a played game shows its icon, name, the time since (`aJ` over the entry's
//! snowflake) and "New Player" on a first session; a listening session shows the track's art, title and artist.

use super::friends::{RELATIONSHIP_FRIEND, RELATIONSHIP_INCOMING, RELATIONSHIP_OUTGOING};
use super::profile_menu::Which;
use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, Hsla, IntoElement, SharedString, Subscription, Window,
    deferred, div, hsla, img, prelude::*, px, rgba, svg,
};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Activity,
    Friends,
    Servers,
}

pub(super) struct FullProfile {
    pub user: Id,
    guild: Option<Id>,
    pub tab: Tab,
    pub(super) note: Entity<Editor>,
    /// The note as last saved, so a blur only writes a change.
    saved_note: String,
    _events: Subscription,
}

/// `PLAYED_GAME` and `LISTENED_SESSION` in `ContentInventoryEntryType`.
const PLAYED_GAME: u64 = 1;
const LISTENED_SESSION: u64 = 5;
/// `ContentInventoryTraitType.FIRST_TIME`.
const TRAIT_FIRST_TIME: u64 = 1;

/// `aJ` with the `E` format set: "Ns ago" up to a minute, then minutes, hours under 12, days under 9, weeks under 4,
/// then months of 30 days, each rounded.
pub(super) fn time_ago(seconds: i64) -> String {
    let span = seconds.abs();
    let round = |unit: f64| (seconds as f64 / unit).round() as i64;
    if span < 60 {
        format!("{seconds}s ago")
    } else if span < 3_600 {
        format!("{}m ago", round(60.))
    } else if span < 12 * 3_600 {
        format!("{}h ago", round(3_600.))
    } else if span < 9 * 86_400 {
        format!("{}d ago", round(86_400.))
    } else if span < 4 * 7 * 86_400 {
        format!("{}w ago", round(7. * 86_400.))
    } else {
        format!("{}mo ago", round(30. * 86_400.))
    }
}

/// Milliseconds since the Unix epoch of a snowflake.
fn snowflake_ms(id: Id) -> i64 {
    (id >> 22) as i64 + 1_420_070_400_000
}

fn profile_route(user: Id, guild: Option<Id>) -> String {
    let mut route = format!(
        "/users/{user}/profile?type=popout&with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=false"
    );
    if let Some(guild) = guild {
        route.push_str(&format!("&guild_id={guild}"));
    }
    route
}

/// 370480.An formats the server's ISO date in the current local timezone.
fn profile_date(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%b %-d, %Y")
                .to_string()
        })
}

impl Root {
    /// The profile on one of its tabs: the Mutual Friends and Mutual Servers links open it there.
    pub(super) fn open_full_profile_tab(
        &mut self,
        user: Id,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_full_profile(user, window, cx);
        if let Some(open) = self.full_profile.as_mut() {
            open.tab = tab;
        }
        cx.notify();
    }

    pub(super) fn open_full_profile(
        &mut self,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let note = cx.new(|cx| {
            // `input_CJLDJI`: body-sm on the profile's text colour, the placeholder in italics.
            let style = crate::editor::EditorStyle {
                size: 14.,
                line_height: 18.,
                text: hsla(0., 0., 1., 0.96),
                placeholder: hsla(0., 0., 1., 0.96),
                placeholder_italic: true,
                caret: hsla(0., 0., 1., 0.96),
                ..super::editor_style()
            };
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder("Click to add a note".to_owned());
            editor
        });
        let events = cx.subscribe(&note, |this, editor, event: &EditorEvent, cx| {
            match event {
                // Enter keeps the note; Escape leaves the field.
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                    this.save_note(text);
                }
                EditorEvent::Cancel => this.full_profile = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        self.full_profile = Some(FullProfile {
            user,
            guild: self.guild,
            tab: Tab::Activity,
            note,
            saved_note: String::new(),
            _events: events,
        });
        for (key, route) in [
            (format!("profile:{user}"), profile_route(user, self.guild)),
            (
                format!("outbox:{user}"),
                format!("/content-inventory/users/{user}/outbox"),
            ),
            (format!("note:{user}"), format!("/users/@me/notes/{user}")),
        ] {
            let _ = self.requests.send(Request::Fetch { key, route });
        }
        if let Some(known) = self.stores.users.get(&user).cloned() {
            let (image, url) = avatar_source(&known);
            self.request_image(image, url);
        }
        let _ = window;
        cx.notify();
    }

    /// Answers this modal waits for: the outbox (then its games' icons) and the note.
    pub(super) fn full_profile_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(open) = self.full_profile.as_ref() else {
            return;
        };
        let user = open.user;
        if key == format!("profile:{user}") {
            if let Some(guild) = open.guild
                && let Some(body) = self.fetched.get(key)
                && body["guild_member_profile"]["guild_id"].as_str()
                    == Some(guild.to_string().as_str())
                && body["guild_member"].is_object()
            {
                self.stores
                    .members
                    .apply_member(guild, &body["guild_member"], Some(&body["user"]));
            }
        } else if key == format!("outbox:{user}") {
            let entries: Vec<Value> = self.fetched[key]["entries"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let games: Vec<String> = entries
                .iter()
                .filter_map(|entry| entry["extra"]["application_id"].as_str().map(str::to_owned))
                .collect();
            if !games.is_empty() {
                let query: Vec<String> = games
                    .iter()
                    .map(|id| format!("application_ids={id}"))
                    .collect();
                let _ = self.requests.send(Request::Fetch {
                    key: format!("apps:{user}"),
                    route: format!("/applications/public?{}", query.join("&")),
                });
            }
            for entry in &entries {
                if let Some(art) = entry["extra"]["entries"][0]["media"]["image_url"].as_str() {
                    self.request_image(art.to_owned(), art.to_owned());
                }
            }
        } else if key == format!("apps:{user}") {
            let apps: Vec<Value> = self.fetched[key].as_array().cloned().unwrap_or_default();
            for app in apps {
                if let (Some(id), Some(icon)) = (app["id"].as_str(), app["icon"].as_str()) {
                    let url = format!(
                        "https://cdn.discordapp.com/app-icons/{id}/{icon}.png?size=160&keep_aspect_ratio=false"
                    );
                    self.request_image(format!("app-icon:{id}"), url);
                }
            }
        } else if key == format!("note:{user}") {
            let note = self.fetched[key]["note"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if let Some(open) = self.full_profile.as_mut() {
                open.saved_note = note.clone();
                open.note.update(cx, |editor, _| editor.set_text(&note));
            }
        } else if key == format!("profile:{user}") {
            let friends: Vec<dc_state::User> = self.fetched[key]["mutual_friends"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(super::dm_profile::mutual_user)
                .collect();
            for friend in friends {
                let (image, url) = avatar_source(&friend);
                self.request_image(image, url);
            }
        }
        cx.notify();
    }

    /// `PUT /users/@me/notes/{id}` when the note changed.
    fn save_note(&mut self, text: String) {
        let Some(open) = self.full_profile.as_mut() else {
            return;
        };
        if open.saved_note == text {
            return;
        }
        open.saved_note = text.clone();
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PUT,
            route: format!("/users/@me/notes/{}", open.user),
            body: Some(serde_json::json!({"note": text})),
        });
    }

    fn close_full_profile(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.full_profile.as_ref() {
            let text = open.note.read(cx).text().to_owned();
            self.save_note(text);
        }
        self.full_profile = None;
        cx.notify();
    }

    fn section_heading(text: impl Into<SharedString>, tint: Hsla) -> gpui::Div {
        div()
            .h(px(16.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(tint)
            .child(text.into())
    }

    fn full_profile_member_since(
        &self,
        user: Id,
        guild: Option<Id>,
        strong: Hsla,
        subtle: Hsla,
    ) -> AnyElement {
        let account_date = super::dm_profile::member_since(user);
        let text = |value: String| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(strong)
                .child(value)
        };
        let member = guild.and_then(|guild| {
            self.stores
                .guild(guild)
                .zip(self.stores.members.get(guild, user))
        });
        let Some((guild, member)) = member else {
            return text(account_date).into_any_element();
        };
        let guild_image = self.images.get(&format!("guild:{}", guild.id)).cloned();
        div()
            .parity("full-profile-member-since")
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        svg()
                            .path("icons/home.svg")
                            .size(px(16.))
                            .text_color(strong),
                    )
                    .child(text(account_date)),
            )
            .child(div().size(px(4.)).rounded_full().bg(subtle))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .size(px(16.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .children(guild_image.map(|image| img(image).size_full()))
                            .when(guild.icon.is_none(), |d| {
                                d.text_size(px(8.))
                                    .text_color(strong)
                                    .child(guild.acronym())
                            }),
                    )
                    .children(member.joined_at.as_deref().and_then(profile_date).map(text)),
            )
            .into_any_element()
    }

    /// One `card_Bu4Al8`: art, title, subtitle and the badges row.
    fn activity_card(
        &self,
        entry: &Value,
        now_ms: i64,
        strong: Hsla,
        subtle: Hsla,
    ) -> Option<AnyElement> {
        let kind = entry["content_type"].as_u64()?;
        let id: Id = entry["id"].as_str()?.parse().ok()?;
        let ago = time_ago((now_ms - snowflake_ms(id)) / 1_000);
        let first_time = entry["traits"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|item| {
                item["type"].as_u64() == Some(TRAIT_FIRST_TIME) && item["first_time"] == true
            });
        let (art, title, subtitle, glyph) = match kind {
            PLAYED_GAME => {
                let app = entry["extra"]["application_id"]
                    .as_str()
                    .unwrap_or_default();
                (
                    self.images.get(&format!("app-icon:{app}")).cloned(),
                    entry["extra"]["game_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    None,
                    "activity_game",
                )
            }
            LISTENED_SESSION => {
                let media = &entry["extra"]["entries"][0]["media"];
                let url = media["image_url"].as_str().unwrap_or_default();
                (
                    self.images.get(url).cloned(),
                    media["title"].as_str().unwrap_or_default().to_owned(),
                    media["artists"][0]["name"].as_str().map(str::to_owned),
                    "activity_music",
                )
            }
            _ => return None,
        };
        let badge = |glyph: &'static str, text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(icon(glyph, 12., "--white"))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(subtle)
                        .child(text),
                )
        };
        Some(
            div()
                .relative()
                .p(px(12.))
                .rounded(px(12.))
                .bg(rgba(0x97979f29))
                .flex()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(60.))
                        .flex_none()
                        .rounded(px(8.))
                        .overflow_hidden()
                        .children(art.map(|picture| img(picture).size(px(60.)).rounded(px(8.)))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(strong)
                                        .child(title),
                                )
                                .children(subtitle.map(|subtitle| {
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(strong)
                                        .child(subtitle)
                                })),
                        )
                        .child(div().flex().gap(px(8.)).child(badge(glyph, ago)).when(
                            kind == PLAYED_GAME && first_time,
                            |this| {
                                this.child(badge("activity_new_player", "New Player".to_owned()))
                            },
                        )),
                )
                .child(div().absolute().top(px(12.)).right(px(12.)).child(icon(
                    "more_horizontal",
                    16.,
                    "--white",
                )))
                .into_any_element(),
        )
    }

    fn mutual_row(
        &self,
        id: SharedString,
        leading: AnyElement,
        name: String,
        tint: Hsla,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .p(px(4.))
            .my(px(1.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|style| style.bg(rgba(0xffffff14)))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(leading)
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(tint)
                    .child(name),
            )
    }

    pub(super) fn full_profile_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.full_profile.as_ref()?;
        let user = self.stores.users.get(&open.user)?.clone();
        let id = user.id;
        let tab = open.tab;
        let body = self.fetched.get(&format!("profile:{id}"));
        let palette = self.dm_palette(id);
        let strong: Hsla = palette.text;
        let subtle: Hsla = hsla(0., 0., 1., 0.88);
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let presence = self.stores.status(id).to_owned();
        let friends: Vec<dc_state::User> = body
            .and_then(|body| body["mutual_friends"].as_array())
            .into_iter()
            .flatten()
            .filter_map(super::dm_profile::mutual_user)
            .collect();
        let servers: Vec<(Id, Option<String>)> = body
            .and_then(|body| body["mutual_guilds"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some((
                    entry["id"].as_str()?.parse().ok()?,
                    entry["nick"].as_str().map(str::to_owned),
                ))
            })
            .collect();
        let pronouns = body
            .and_then(|body| body["user_profile"]["pronouns"].as_str())
            .filter(|pronouns| !pronouns.is_empty())
            .map(str::to_owned);
        let bio = body
            .and_then(|body| body["user_profile"]["bio"].as_str())
            .filter(|bio| !bio.is_empty())
            .map(str::to_owned);
        let plural = |count: usize, one: &str, many: &str| {
            if count == 1 {
                format!("1 {one}")
            } else {
                format!("{count} {many}")
            }
        };
        let relationship = self.stores.relationships.get(&id).copied();
        let friend_since = (relationship == Some(RELATIONSHIP_FRIEND))
            .then(|| {
                self.stores
                    .relationship_since
                    .get(&id)
                    .and_then(|since| profile_date(since))
            })
            .flatten();
        let me = self.stores.current_user.as_ref().map(|known| known.id) == Some(id);

        // `profileHeader_s3DB6Y`: the 140px banner and the avatar over its edge.
        let header = div()
            .relative()
            .h(px(220.))
            .child(div().h(px(140.)).rounded_t(px(16.)).bg(palette.top))
            .child(
                div()
                    .absolute()
                    .left(px(32.))
                    .top(px(80.))
                    .size(px(120.))
                    .rounded_full()
                    .children(avatar.map(|picture| img(picture).size(px(120.)).rounded_full()))
                    .child(status_dot_120(&presence)),
            );
        let names = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.))
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(strong)
                    .child(user.display_name().to_owned()),
            )
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(strong)
                    .child(user.username.clone())
                    .when_some(pronouns, |this, pronouns| {
                        this.child(div().size(px(4.)).rounded_full().bg(strong))
                            .child(pronouns)
                    })
                    .when(!self.badges(id).is_empty(), |this| {
                        this.child(
                            div()
                                .h(px(22.))
                                .px(px(3.))
                                .py(px(1.))
                                .rounded(px(8.))
                                .bg(hsla(240. / 360., 0.03, 0.61, 0.12))
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .children(self.badges(id)),
                        )
                    }),
            );
        let secondary_button = |key: &'static str, glyph: &'static str| {
            div()
                .id(key)
                .relative()
                .size(px(32.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgba(0x97979f0a))
                .bg(rgba(0xffffff14))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(glyph, 16., "--white"))
        };
        let primary = match relationship {
            None | Some(0) if !me => Some(
                div()
                    .id("full-profile-add-friend")
                    .h(px(32.))
                    .px(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgba(0xffffff14))
                    .bg(gpui::rgb(0x555555))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("user_plus", 16., "--white"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child("Add Friend"),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.set_relationship(id, Some(0), cx)),
                    )
                    .into_any_element(),
            ),
            Some(RELATIONSHIP_FRIEND | RELATIONSHIP_INCOMING | RELATIONSHIP_OUTGOING) => Some(
                div()
                    .relative()
                    .child(self.anchor_probe("full-profile-friend"))
                    .child(
                        secondary_button(
                            "full-profile-friend",
                            if relationship == Some(RELATIONSHIP_FRIEND) {
                                "user_check"
                            } else {
                                "user_clock"
                            },
                        )
                        .when(
                            relationship != Some(RELATIONSHIP_OUTGOING),
                            |this| {
                                this.on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_profile_menu_at(
                                        id,
                                        Which::Friend,
                                        "full-profile-friend",
                                        true,
                                        cx,
                                    );
                                }))
                            },
                        ),
                    )
                    .into_any_element(),
            ),
            _ => None,
        };
        let buttons = div()
            .parity("full-profile-buttons")
            .h(px(32.))
            .flex()
            .gap(px(8.))
            .when(relationship == Some(RELATIONSHIP_FRIEND) && !me, |d| {
                d.child(
                    div()
                        .id("full-profile-message-primary")
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(rgba(0xffffff14))
                        .bg(gpui::rgb(0x555555))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(icon("chat_bubble", 16., "--white"))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--white"))
                                .child("Message"),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.full_profile = None;
                            this.open_dm_with(id, cx);
                        })),
                )
            })
            .children(primary)
            .when(!me && relationship != Some(RELATIONSHIP_FRIEND), |this| {
                this.child(
                    secondary_button("full-profile-message", "chat_bubble").on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.full_profile = None;
                            this.open_dm_with(id, cx);
                        },
                    )),
                )
            })
            .child(
                div()
                    .relative()
                    .child(self.anchor_probe("full-profile-more"))
                    .child(
                        secondary_button("full-profile-more", "more_horizontal").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.open_profile_menu_at(
                                    id,
                                    Which::Overflow,
                                    "full-profile-more",
                                    true,
                                    cx,
                                );
                            }),
                        ),
                    ),
            );
        let section = |heading: Option<&'static str>| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(heading.map(|heading| Self::section_heading(heading, subtle)))
        };
        let note_field = div()
            .ml(px(-7.))
            .mt(px(-4.))
            .px(px(7.))
            .py(px(1.))
            .rounded(px(8.))
            .child(open.note.clone());
        let identity = div()
            .w(px(400.))
            .h_full()
            .flex_none()
            .rounded(px(16.))
            .bg(palette.top)
            .shadow(vec![gpui::BoxShadow {
                color: hsla(0., 0., 0., 0.16),
                offset: gpui::point(px(0.), px(4.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .size_full()
                    .rounded(px(16.))
                    .bg(palette.overlay)
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(
                        div()
                            .id("full-profile-body")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .mt(px(-4.))
                            .pt(px(4.))
                            .pl(px(32.))
                            // `profileBody` pads 24 and keeps a stable 8px gutter for its thin scrollbar.
                            .pr(px(32.))
                            .pb(px(32.))
                            .flex()
                            .flex_col()
                            .gap(px(24.))
                            .child(names)
                            .child(buttons)
                            .when_some(bio, |this, bio| {
                                this.child(section(None).child(self.bio(&bio, strong, cx)))
                            })
                            .child(section(Some("Member Since")).child(
                                self.full_profile_member_since(id, open.guild, strong, subtle),
                            ))
                            .when_some(friend_since, |d, date| {
                                d.child(
                                    section(Some("Friends Since")).child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(strong)
                                            .child(date),
                                    ),
                                )
                            })
                            .when_some(
                                open.guild.and_then(|guild| {
                                    self.profile_role_chips_for(guild, id, strong)
                                }),
                                |d, roles| {
                                    d.child(
                                        section(Some("Roles"))
                                            .parity("full-profile-roles")
                                            .gap(px(4.))
                                            .child(roles),
                                    )
                                },
                            )
                            .when(!me, |this| {
                                this.child(
                                    section(Some("Note (only visible to you)")).child(note_field),
                                )
                            }),
                    ),
            );
        // `top_6zZYhF`: the tabs over a 1px line, 24px apart, the chosen one underlined.
        let tab_item = |which: Tab, label: String| {
            let selected = which == tab;
            div()
                .id(SharedString::from(format!("full-profile-tab-{label}")))
                .relative()
                .h(px(30.))
                .mb(px(-1.))
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(if selected { strong } else { subtle })
                .child(label)
                .when(selected, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(2.))
                            .bg(strong),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(open) = this.full_profile.as_mut() {
                        open.tab = which;
                    }
                    cx.notify();
                }))
        };
        let tabs = div()
            .h(px(30.))
            .border_b_1()
            .border_color(hsla(0., 0., 1., 0.12))
            .flex()
            .gap(px(24.))
            .child(tab_item(Tab::Activity, "Activity".to_owned()))
            .when(!friends.is_empty(), |this| {
                this.child(tab_item(
                    Tab::Friends,
                    plural(friends.len(), "Mutual Friend", "Mutual Friends"),
                ))
            })
            .when(!servers.is_empty(), |this| {
                this.child(tab_item(
                    Tab::Servers,
                    plural(servers.len(), "Mutual Server", "Mutual Servers"),
                ))
            });
        let now_ms = chrono::Utc::now().timestamp_millis();
        let panel: AnyElement = match tab {
            Tab::Activity => {
                let current = self.live_activity_cards(id, true, strong, subtle, cx);
                let cards: Vec<AnyElement> = self
                    .fetched
                    .get(&format!("outbox:{id}"))
                    .and_then(|outbox| outbox["entries"].as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| self.activity_card(entry, now_ms, strong, subtle))
                    .collect();
                div()
                    .pt(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(!current.is_empty(), |this| {
                        this.child(Self::section_heading("Current activity", subtle))
                            .child(div().flex().flex_col().gap(px(8.)).children(current))
                    })
                    .when(!cards.is_empty(), |this| {
                        this.child(Self::section_heading("Recent activity", subtle))
                            .child(div().flex().flex_col().gap(px(8.)).children(cards))
                    })
                    .into_any_element()
            }
            Tab::Friends => div()
                .pt(px(20.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(friends.into_iter().map(|friend| {
                    let picture = self.images.get(&avatar_source(&friend).0).cloned();
                    let status = self
                        .stores
                        .presences
                        .get(&friend.id)
                        .cloned()
                        .unwrap_or_else(|| "offline".to_owned());
                    let leading = div()
                        .relative()
                        .size(px(40.))
                        .children(picture.map(|picture| img(picture).size(px(40.)).rounded_full()))
                        .child(super::status_dot_40(&status, "--black"))
                        .into_any_element();
                    let friend_id = friend.id;
                    let tag = friend.tag.clone();
                    self.mutual_row(
                        SharedString::from(format!("full-profile-friend-{friend_id}")),
                        leading,
                        friend.display_name().to_owned(),
                        strong,
                    )
                    .children(tag.as_ref().map(|tag| self.server_tag_chip(tag)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_full_profile(friend_id, window, cx);
                    }))
                    .into_any_element()
                }))
                .into_any_element(),
            Tab::Servers => div()
                .pt(px(20.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(servers.into_iter().filter_map(|(guild, nick)| {
                    let known = self.stores.guilds.iter().find(|known| known.id == guild)?;
                    let name = known.name.clone();
                    let picture = self.images.get(&format!("guild:{guild}")).cloned();
                    let leading = div()
                        .size(px(40.))
                        .flex_none()
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(color("--background-surface-high"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .text_color(color("--text-default"))
                        .when_some(picture, |this, picture| {
                            this.child(img(picture).size(px(40.)).rounded(px(12.)))
                        })
                        .when(known.icon.is_none(), |this| this.child(known.acronym()))
                        .into_any_element();
                    Some(
                        self.mutual_row(
                            SharedString::from(format!("full-profile-server-{guild}")),
                            leading,
                            name,
                            strong,
                        )
                        .children(nick.map(|nick| {
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(subtle)
                                .child(nick)
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.full_profile = None;
                            this.select(guild, None);
                            cx.notify();
                        }))
                        .into_any_element(),
                    )
                }))
                .into_any_element(),
        };
        let columns = div().w(px(880.)).h_full().flex().child(identity).child(
            div()
                .flex_1()
                .min_w(px(0.))
                .pt(px(20.))
                .pl(px(32.))
                .pr(px(16.))
                .flex()
                .flex_col()
                .child(tabs)
                .child(
                    // `tabBarPanel_y4ZEM5` starts 2px above the tab line's bottom.
                    div()
                        .id("full-profile-panel")
                        .mt(px(-2.))
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .pb(px(16.))
                        .child(panel),
                ),
        );
        let height = f32::from(window.viewport_size().height);
        let modal = div()
            .parity("full-profile")
            .id("full-profile")
            .occlude()
            .relative()
            .w(px(962.))
            .h(px(height))
            .px(px(1.))
            .rounded(px(12.))
            .bg(palette.top)
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .size_full()
                    .rounded(px(12.))
                    .bg(rgba(0x000000b8))
                    .pt(px(80.))
                    .pl(px(48.))
                    .pr(px(32.))
                    .child(columns),
            )
            .child(
                div()
                    .id("full-profile-close")
                    .absolute()
                    .top(px(40.))
                    .right(px(7.))
                    .size(px(32.))
                    .rounded(px(8.))
                    .bg(rgba(0xffffff14))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 16., "--white"))
                    .on_click(cx.listener(|this, _, _, cx| this.close_full_profile(cx))),
            );
        Some(
            deferred(
                div()
                    .id("full-profile-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.close_full_profile(cx)))
                    .child(modal),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// `svg-mask-avatar-status-round-120`: a 24px dot at (88, 88) in a 40px hole.
fn status_dot_120(status: &str) -> impl IntoElement {
    div()
        .absolute()
        .left(px(80.))
        .top(px(80.))
        .size(px(40.))
        .rounded_full()
        .bg(color("--black"))
        .flex()
        .items_center()
        .justify_center()
        .child(super::status_dot_inner(status, "--black", 24.))
}

#[cfg(test)]
mod tests {
    use super::{profile_date, profile_route, time_ago};

    #[test]
    fn the_profile_request_names_the_guild_only_when_one_is_in_view() {
        assert!(!profile_route(7, None).contains("guild_id"));
        assert!(profile_route(7, None).starts_with("/users/7/profile?type=popout"));
        assert!(profile_route(7, Some(9)).ends_with("&guild_id=9"));
    }

    #[test]
    fn profile_dates_read_as_month_day_year() {
        // Noon UTC is the same calendar day in every zone from UTC-11 to UTC+11.
        assert_eq!(
            profile_date("2021-10-29T12:00:00.000000+00:00").as_deref(),
            Some("Oct 29, 2021")
        );
        assert_eq!(profile_date("not a date"), None);
    }

    #[test]
    fn time_ago_rounds_into_the_official_bands() {
        assert_eq!(time_ago(42), "42s ago");
        assert_eq!(time_ago(90), "2m ago");
        assert_eq!(time_ago(8 * 3_600 + 1_000), "8h ago");
        assert_eq!(time_ago(30 * 3_600), "1d ago");
        assert_eq!(time_ago(10 * 86_400), "1w ago");
        assert_eq!(time_ago(40 * 86_400), "1mo ago");
    }
}
