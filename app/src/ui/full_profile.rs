//! The full profile (`user-profile-modal-v2`, module 927071): 960 wide and the window's height, a 400px identity
//! column (banner, 120px avatar, names, Add Friend / Message / More, bio, Member Since, the private note) beside
//! tabs for recent activity (the content-inventory outbox), mutual friends and mutual servers. Measured in
//! `design/dom/modals/full-profile-{activity,friends,servers}-darker-cozy.json`.
//!
//! Activity cards follow module 506326: a played game shows its icon, name, the time since (`aJ` over the entry's
//! snowflake) and "New Player" on a first session; a listening session shows the track's art, title and artist.

use super::friends::{RELATIONSHIP_FRIEND, RELATIONSHIP_INCOMING, RELATIONSHIP_OUTGOING};
use super::profile_editor::save::{PersonalSection, WidgetData, parse_widgets};
use super::profile_editor::widgets::{featured_app, tag_choices, widget_data_title};
use super::profile_menu::Which;
use super::{Root, avatar_source, color};
use crate::editor::{Editor, EditorEvent, EditorStyle};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, Hsla, IntoElement, Rgba, SharedString, Subscription,
    Window, deferred, div, hsla, img, prelude::*, px, rgba, svg,
};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    /// `WIDGETS` ("Board"): the profile's widgets, first when it has any.
    Board,
    Activity,
    /// `WISHLIST`: the profile's wishlist, when it has one.
    Wishlist,
    Friends,
    Servers,
}

/// The tabs of module 927071 (`e3`), in order: Board when the profile has widgets, Activity, Wishlist when it has a
/// wishlist, then the mutual friends and servers of someone else.
pub(super) fn profile_tabs(body: Option<&Value>, other: bool) -> Vec<Tab> {
    let mut tabs = Vec::new();
    if body.is_some_and(|body| {
        parse_widgets(body)
            .iter()
            .any(|widget| widget.data.is_valid())
    }) {
        tabs.push(Tab::Board);
    }
    tabs.push(Tab::Activity);
    if body.is_some_and(|body| !super::shop::data::wishlist_ids(body).is_empty()) {
        tabs.push(Tab::Wishlist);
    }
    if other {
        tabs.push(Tab::Friends);
        tabs.push(Tab::Servers);
    }
    tabs
}

pub(super) struct FullProfile {
    pub user: Id,
    guild: Option<Id>,
    pub tab: Tab,
    /// A tab was asked for (`initialSection`) or pressed; otherwise the first tab shows.
    pub explicit: bool,
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
            open.explicit = true;
        }
        cx.notify();
    }

    pub(super) fn open_full_profile(
        &mut self,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let note_palette = self.profile_palette(user, self.guild, false);
        let note = cx.new(|cx| {
            // `input_CJLDJI`: body-sm on the profile's text colour, the placeholder in italics.
            let style = crate::editor::EditorStyle {
                size: 14.,
                line_height: 18.,
                text: note_palette.text,
                placeholder: note_palette.text,
                placeholder_italic: true,
                caret: note_palette.text,
                ..super::editor_style()
            };
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder(dc_intl::t!("VBhOe2").to_owned());
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
            explicit: false,
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
        self.request_profile_images(user);
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
            let friends: Vec<dc_state::User> = self.fetched[key]["mutual_friends"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(super::dm_profile::mutual_user)
                .collect();
            for friend in friends {
                let (image, url) = avatar_source(&friend);
                self.request_image(image, url);
                if let Some(url) = friend.tag.as_ref().and_then(|tag| tag.badge_url()) {
                    self.request_image(url.clone(), url);
                }
            }
            self.request_profile_images(user);
        } else if key == format!("profile-games:{user}") {
            self.request_profile_cards(user);
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
            // The answer goes through NoteStore, which the field then reads.
            let note = self.fetched[key]["note"].as_str().map(str::to_owned);
            self.stores.notes.set(user, note);
            self.full_profile_note_changed(user, cx);
        }
        cx.notify();
    }

    /// The open profile's note field takes what NoteStore holds for its user (a read's answer, or another session's
    /// `USER_NOTE_UPDATE`).
    pub(super) fn full_profile_note_changed(&mut self, user: Id, cx: &mut Context<Self>) {
        let note = self.stores.notes.text_of(user).to_owned();
        if let Some(open) = self.full_profile.as_mut()
            && open.user == user
        {
            open.saved_note = note.clone();
            open.note.update(cx, |editor, _| editor.set_text(&note));
        }
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
        self.stores.notes.set(open.user, Some(text.clone()));
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PUT,
            route: format!("/users/@me/notes/{}", open.user),
            body: Some(serde_json::json!({"note": text})),
        });
    }

    pub(super) fn close_full_profile(&mut self, cx: &mut Context<Self>) {
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
        default: Hsla,
        subtle: Hsla,
    ) -> AnyElement {
        let account_date = super::dm_profile::member_since(user);
        let text = |value: String| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(default)
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
                            .text_color(default),
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
        palette: &super::dm_profile::Palette,
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
                .child(
                    svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(12.))
                        .text_color(subtle),
                )
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
                .border_1()
                .border_color(palette.token("--border-muted"))
                .bg(palette.token("--user-profile-overlay-background"))
                .flex()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(60.))
                        .flex_none()
                        .rounded(px(8.))
                        .overflow_hidden()
                        .bg(palette.tile_background(false))
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
                                this.child(badge(
                                    "activity_new_player",
                                    dc_intl::t!("keY6mW").to_owned(),
                                ))
                            },
                        )),
                )
                .child(
                    div().absolute().top(px(12.)).right(px(12.)).child(
                        svg()
                            .path("icons/more_horizontal.svg")
                            .size(px(16.))
                            .text_color(subtle),
                    ),
                )
                .into_any_element(),
        )
    }

    /// The Board (`a5` of module 927071): themed widget cards populated from the user's saved `widgets`.
    fn full_profile_board(
        &self,
        user: Id,
        palette: &super::dm_profile::Palette,
        strong: Hsla,
        default: Hsla,
        subtle: Hsla,
    ) -> AnyElement {
        let body = self.fetched.get(&format!("profile:{user}"));
        let answer = self.fetched.get(&format!("profile-games:{user}"));
        let game = |id: &str| {
            answer
                .and_then(Value::as_array)
                .and_then(|games| games.iter().find(|game| game["id"].as_str() == Some(id)))
        };
        let tag_map = tag_choices();
        let card_bg = palette.token("--user-profile-overlay-background");
        let border = palette.token("--border-muted");
        let tile_bg = palette.tile_background(false);
        let pill_bg = palette.pill_background(false);
        let widgets: Vec<AnyElement> = body
            .map(parse_widgets)
            .unwrap_or_default()
            .into_iter()
            .filter(|widget| widget.data.is_valid())
            .map(|widget| {
                let title = widget_data_title(&widget.data);
                let content: AnyElement = match &widget.data {
                    WidgetData::FavoriteGames { games }
                    | WidgetData::CurrentGames { games }
                    | WidgetData::WantToPlayGames { games }
                    | WidgetData::PlayedGames { games } => {
                        let rows: Vec<AnyElement> = games
                            .iter()
                            .map(|entry| {
                                let fetched = game(&entry.game_id);
                                let resolved = self.profile_editor_resolve_game(&entry.game_id);
                                let name = fetched
                                    .and_then(|item| item["name"].as_str())
                                    .filter(|name| !name.is_empty())
                                    .map_or(resolved.name, str::to_owned);
                                let picture = self
                                    .images
                                    .get(&format!("profile-game:{}", entry.game_id))
                                    .cloned()
                                    .or_else(|| {
                                        resolved
                                            .cover
                                            .as_deref()
                                            .and_then(|url| self.images.get(url).cloned())
                                    })
                                    .or_else(|| {
                                        resolved
                                            .icon
                                            .as_deref()
                                            .and_then(|url| self.images.get(url).cloned())
                                    });
                                let comment = entry
                                    .comment
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|text| !text.is_empty())
                                    .map(str::to_owned);
                                let tag_chips: Vec<AnyElement> = entry
                                    .tags
                                    .iter()
                                    .filter_map(|tag_id| {
                                        let label = tag_map
                                            .iter()
                                            .find(|(id, _)| id == tag_id)
                                            .map(|(_, label)| *label)?;
                                        Some(
                                            div()
                                                .px(px(6.))
                                                .py(px(2.))
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(border)
                                                .bg(pill_bg)
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(default)
                                                .child(label)
                                                .into_any_element(),
                                        )
                                    })
                                    .collect();
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .w(px(56.))
                                            .h(px(74.66))
                                            .flex_none()
                                            .rounded(px(8.))
                                            .overflow_hidden()
                                            .bg(tile_bg)
                                            .children(picture.map(|picture| {
                                                img(picture)
                                                    .w(px(56.))
                                                    .h(px(74.66))
                                                    .object_fit(gpui::ObjectFit::Cover)
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(16.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(strong)
                                                    .child(name),
                                            )
                                            .when(!tag_chips.is_empty(), |this| {
                                                this.child(
                                                    div()
                                                        .flex()
                                                        .flex_wrap()
                                                        .gap(px(4.))
                                                        .children(tag_chips),
                                                )
                                            })
                                            .when_some(comment, |this, comment| {
                                                this.child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .text_color(default)
                                                        .child(comment),
                                                )
                                            }),
                                    )
                                    .into_any_element()
                            })
                            .collect();
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .children(rows)
                            .into_any_element()
                    }
                    WidgetData::Application { application_id } => {
                        let resolved = self.profile_editor_resolve_game(application_id);
                        let featured = featured_app(application_id);
                        let (app_name, icon_url) = featured.map_or_else(
                            || (resolved.name, resolved.icon),
                            |(name, url)| (name.to_owned(), Some(url)),
                        );
                        let picture = icon_url
                            .as_deref()
                            .and_then(|url| self.images.get(url).cloned())
                            .or_else(|| {
                                self.images
                                    .get(&format!("profile-game:{application_id}"))
                                    .cloned()
                            });
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(48.))
                                    .flex_none()
                                    .rounded(px(8.))
                                    .overflow_hidden()
                                    .bg(tile_bg)
                                    .children(picture.map(|picture| {
                                        img(picture)
                                            .size(px(48.))
                                            .object_fit(gpui::ObjectFit::Cover)
                                    })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(strong)
                                            .child(app_name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(subtle)
                                            .child(dc_intl::t!("EKR8Ps")),
                                    ),
                            )
                            .into_any_element()
                    }
                    WidgetData::Personal { sections, .. } => {
                        let rendered: Vec<AnyElement> = sections
                            .iter()
                            .filter_map(|section| match section {
                                PersonalSection::Cover { title, subtitle }
                                    if !title.trim().is_empty() || !subtitle.trim().is_empty() =>
                                {
                                    Some(
                                        div()
                                            .p(px(10.))
                                            .rounded(px(8.))
                                            .bg(tile_bg)
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .when(!title.trim().is_empty(), |this| {
                                                this.child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(strong)
                                                        .child(title.clone()),
                                                )
                                            })
                                            .when(!subtitle.trim().is_empty(), |this| {
                                                this.child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .line_height(px(18.))
                                                        .text_color(default)
                                                        .child(subtitle.clone()),
                                                )
                                            })
                                            .into_any_element(),
                                    )
                                }
                                PersonalSection::Fields { fields } if !fields.is_empty() => Some(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .children(fields.iter().map(|field| {
                                            div()
                                                .p(px(10.))
                                                .rounded(px(8.))
                                                .bg(tile_bg)
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.))
                                                .when(!field.title.trim().is_empty(), |this| {
                                                    this.child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .line_height(px(16.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(subtle)
                                                            .child(field.title.clone()),
                                                    )
                                                })
                                                .when(
                                                    !field.description.trim().is_empty(),
                                                    |this| {
                                                        this.child(
                                                            div()
                                                                .text_size(px(14.))
                                                                .line_height(px(18.))
                                                                .text_color(default)
                                                                .child(field.description.clone()),
                                                        )
                                                    },
                                                )
                                        }))
                                        .into_any_element(),
                                ),
                                _ => None,
                            })
                            .collect();
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(rendered)
                            .into_any_element()
                    }
                    WidgetData::ClipsGallery { clips } => div()
                        .p(px(10.))
                        .rounded(px(8.))
                        .bg(tile_bg)
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(default)
                        .child(dc_intl::t!("pb2Was", numClips = clips.len()))
                        .into_any_element(),
                };
                div()
                    .p(px(16.))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(border)
                    .bg(card_bg)
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(Self::section_heading(title, subtle))
                    .child(content)
                    .into_any_element()
            })
            .collect();
        div()
            .pt(px(20.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(widgets)
            .into_any_element()
    }

    /// The Wishlist tab (`o9` of module 927071): the items of the first wishlist; pressing one opens its product.
    fn full_profile_wishlist(
        &mut self,
        user: Id,
        palette: &super::dm_profile::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let list = self.fetched.get(&format!("profile-wishlist:{user}"))?;
        let skus = super::dm_profile::unowned_wishlist(list);
        self.editor_wishlist_need(&skus);
        let mut cards = Vec::new();
        let card_bg = palette.token("--user-profile-overlay-background");
        let border = palette.token("--border-muted");
        for sku in skus {
            let preview = self.editor_wishlist_preview(&sku, 160., window, cx);
            cards.push(
                div()
                    .id(SharedString::from(format!("full-profile-wishlist-{sku}")))
                    .size(px(160.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border)
                    .overflow_hidden()
                    .bg(card_bg)
                    .cursor_pointer()
                    .children(preview)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop_open_product(&sku, cx);
                    }))
                    .into_any_element(),
            );
        }
        Some(
            div()
                .pt(px(20.))
                .flex()
                .flex_wrap()
                .gap(px(16.))
                .children(cards)
                .into_any_element(),
        )
    }

    fn mutual_row(
        &self,
        id: SharedString,
        leading: AnyElement,
        name: String,
        tint: Hsla,
        hover_bg: gpui::Rgba,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .p(px(4.))
            .my(px(1.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg))
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
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (open_user, open_guild, open_tab, explicit, note_entity) = {
            let open = self.full_profile.as_ref()?;
            (
                open.user,
                open.guild,
                open.tab,
                open.explicit,
                open.note.clone(),
            )
        };
        let palette = self.profile_palette(open_user, open_guild, false);
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let placeholder: Hsla = palette.token("--input-placeholder-text-default").into();
        note_entity.update(cx, |editor, _| {
            editor.set_style(EditorStyle {
                size: 14.,
                line_height: 18.,
                text: default,
                placeholder,
                placeholder_italic: true,
                caret: default,
                ..super::editor_style()
            });
        });
        let me_id = self.stores.current_user.as_ref().map(|known| known.id);
        let tab_list = profile_tabs(
            self.fetched.get(&format!("profile:{open_user}")),
            me_id != Some(open_user),
        );
        let tab = if explicit && tab_list.contains(&open_tab) {
            open_tab
        } else {
            tab_list.first().copied().unwrap_or(Tab::Activity)
        };
        let wishlist_panel = (tab == Tab::Wishlist)
            .then(|| self.full_profile_wishlist(open_user, &palette, window, cx));
        let open = self.full_profile.as_ref()?;
        let user = self.stores.users.get(&open.user)?.clone();
        let id = user.id;
        let body = self.fetched.get(&format!("profile:{id}"));
        let ring_color = palette.theme.map_or_else(
            || color("--background-surface-high"),
            |t| super::profile_theme::blend_over(t.primary, t.overlay),
        );
        let ring_token = if palette.theme.is_some_and(|t| t.light()) {
            "--white"
        } else {
            "--black"
        };
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
            .child(
                div()
                    .h(px(140.))
                    .rounded_t(px(16.))
                    .overflow_hidden()
                    .relative()
                    .when(!palette.themed, |this| {
                        this.bg(self.profile_banner_fill(&user, open.guild))
                    })
                    .children(
                        self.profile_banner_image(id)
                            .map(|image| img(image).size_full().object_fit(gpui::ObjectFit::Cover)),
                    )
                    .children(self.live_banner(id, 400., 140., cx)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(32.))
                    .top(px(80.))
                    .size(px(120.))
                    .rounded_full()
                    .children(self.live_avatar(&user, 120., true, cx).or_else(|| {
                        avatar.map(|picture| {
                            img(picture)
                                .size(px(120.))
                                .rounded_full()
                                .into_any_element()
                        })
                    }))
                    .child(status_dot_120(&presence, ring_color, ring_token)),
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
                    .child(super::name_styles::name(
                        format!("full-profile-name-{}", user.id),
                        user.display_name().to_owned(),
                        self.name_styles(&user, self.guild).as_ref(),
                        super::name_styles::Display::Animated,
                    )),
            )
            .child(
                div()
                    .min_h(px(24.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(default)
                    .child(user.username.clone())
                    .when_some(pronouns, |this, pronouns| {
                        this.child(div().size(px(4.)).rounded_full().bg(default))
                            .child(pronouns)
                    })
                    .children(
                        user.tag
                            .as_ref()
                            .map(|tag| self.profile_server_tag_pill(tag, &palette)),
                    )
                    .children(self.badge_pill("full-profile", id, &palette, cx)),
            );
        let secondary_bg = palette.token("--control-secondary-background-default");
        let secondary_hover = palette.token("--control-secondary-background-hover");
        let secondary_active = palette.token("--control-secondary-background-active");
        let secondary_border = palette.token("--control-secondary-border-default");
        let secondary_fg = palette.token("--control-secondary-text-default");
        let primary_bg = palette.token("--control-primary-background-default");
        let primary_hover = palette.token("--control-primary-background-hover");
        let primary_active = palette.token("--control-primary-background-active");
        let primary_fg = palette.token("--control-primary-text-default");
        let border_muted = palette.token("--border-muted");
        let secondary_button = |key: &'static str, glyph: &'static str| {
            div()
                .id(key)
                .relative()
                .size(px(32.))
                .rounded(px(8.))
                .border_1()
                .border_color(secondary_border)
                .bg(secondary_bg)
                .hover(move |style| style.bg(secondary_hover))
                .active(move |style| style.bg(secondary_active))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    gpui::svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(16.))
                        .text_color(secondary_fg),
                )
        };
        let primary = match relationship {
            None | Some(0) if !me => Some(
                div()
                    .id("full-profile-add-friend")
                    .h(px(32.))
                    .px(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border_muted)
                    .bg(primary_bg)
                    .hover(move |style| style.bg(primary_hover))
                    .active(move |style| style.bg(primary_active))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        svg()
                            .path("icons/user_plus.svg")
                            .size(px(16.))
                            .text_color(primary_fg),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(primary_fg)
                            .child(dc_intl::t!("w5uwoI")),
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
                        .border_color(border_muted)
                        .bg(primary_bg)
                        .hover(move |style| style.bg(primary_hover))
                        .active(move |style| style.bg(primary_active))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            svg()
                                .path("icons/chat_bubble.svg")
                                .size(px(16.))
                                .text_color(primary_fg),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(primary_fg)
                                .child(dc_intl::t!("BAB0yK")),
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
        let note_hover = palette.token("--user-profile-background-hover");
        let note_field = div()
            .ml(px(-7.))
            .mt(px(-4.))
            .px(px(7.))
            .py(px(1.))
            .rounded(px(8.))
            .hover(move |style| style.bg(note_hover))
            .child(open.note.clone());
        // `<ProfileEffects>` is the last child of `main.profile` (module 927071, `u8`): the identity column.
        let effect = self.profile_effect_sku(id).and_then(|sku| {
            let hovering = self.profile_effect_hovered("full-profile");
            self.profile_effect_layer(
                "full-profile",
                sku,
                super::profile_effects::Surface::profile(400., hovering),
                window,
                cx,
            )
        });
        let identity = div()
            .id("full-profile-identity")
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                this.profile_effect_hover("full-profile", *inside, cx);
            }))
            .relative()
            .w(px(400.))
            .h_full()
            .flex_none()
            .rounded(px(16.))
            .overflow_hidden()
            .border_1()
            .border_color(border_muted)
            .bg(palette.top)
            .children(palette.card_backdrop(140.))
            .shadow(vec![gpui::BoxShadow {
                color: hsla(0., 0., 0., 0.16),
                offset: gpui::point(px(0.), px(4.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .relative()
                    .size_full()
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
                                this.child(section(None).child(self.bio(&bio, default, cx)))
                            })
                            .child(section(Some(dc_intl::t!("a6XYD9"))).child(
                                self.full_profile_member_since(
                                    id, open.guild, strong, default, subtle,
                                ),
                            ))
                            .when_some(friend_since, |d, date| {
                                d.child(
                                    section(Some(dc_intl::t!("wlTO8v"))).child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(default)
                                            .child(date),
                                    ),
                                )
                            })
                            .when_some(
                                open.guild.and_then(|guild| {
                                    self.profile_role_chips_for(guild, id, default)
                                }),
                                |d, roles| {
                                    d.child(
                                        section(Some(dc_intl::t!("2SZsWX")))
                                            .parity("full-profile-roles")
                                            .gap(px(4.))
                                            .child(roles),
                                    )
                                },
                            )
                            .when(!me, |this| {
                                this.child(section(Some(dc_intl::t!("mQKv+v"))).child(note_field))
                            }),
                    ),
            )
            .children(effect);
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
                        open.explicit = true;
                    }
                    cx.notify();
                }))
        };
        let tabs = div()
            .h(px(30.))
            .border_b_1()
            .border_color(palette.token("--border-subtle"))
            .flex()
            .gap(px(24.))
            .when(tab_list.contains(&Tab::Board), |this| {
                this.child(tab_item(Tab::Board, dc_intl::t!("laViwx").to_owned()))
            })
            .child(tab_item(Tab::Activity, dc_intl::t!("chq59f").to_owned()))
            .when(tab_list.contains(&Tab::Wishlist), |this| {
                this.child(tab_item(Tab::Wishlist, dc_intl::t!("7lZ31J").to_owned()))
            })
            .when(!friends.is_empty(), |this| {
                this.child(tab_item(
                    Tab::Friends,
                    dc_intl::t!("5s9jl+", count = friends.len()),
                ))
            })
            .when(!servers.is_empty(), |this| {
                this.child(tab_item(
                    Tab::Servers,
                    dc_intl::t!("eE3oep", count = servers.len()),
                ))
            });
        let now_ms = chrono::Utc::now().timestamp_millis();
        let row_hover = palette.token("--user-profile-background-hover");
        let dot_cutout = palette.theme.map_or("--background-surface-high", |t| {
            if t.light() { "--white" } else { "--black" }
        });
        let tile_bg = palette.tile_background(false);
        let panel: AnyElement = match tab {
            Tab::Board => self.full_profile_board(id, &palette, strong, default, subtle),
            Tab::Wishlist => wishlist_panel
                .flatten()
                .unwrap_or_else(|| div().into_any_element()),
            Tab::Activity => {
                let current = self.live_activity_cards(id, true, strong, subtle, cx);
                let cards: Vec<AnyElement> = self
                    .fetched
                    .get(&format!("outbox:{id}"))
                    .and_then(|outbox| outbox["entries"].as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| self.activity_card(entry, now_ms, &palette, strong, subtle))
                    .collect();
                div()
                    .pt(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(!current.is_empty(), |this| {
                        this.child(Self::section_heading(dc_intl::t!("J6STd9"), subtle))
                            .child(div().flex().flex_col().gap(px(8.)).children(current))
                    })
                    .when(!cards.is_empty(), |this| {
                        this.child(Self::section_heading(dc_intl::t!("jzgEoL"), subtle))
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
                        .child(super::status_dot_40(&status, dot_cutout))
                        .into_any_element();
                    let friend_id = friend.id;
                    let tag = friend.tag.clone();
                    self.mutual_row(
                        SharedString::from(format!("full-profile-friend-{friend_id}")),
                        leading,
                        friend.display_name().to_owned(),
                        strong,
                        row_hover,
                    )
                    .children(
                        tag.as_ref()
                            .map(|tag| self.profile_server_tag_pill(tag, &palette)),
                    )
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
                        .bg(tile_bg)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .text_color(default)
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
                            row_hover,
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
            .w(px(960.))
            .h(px(height))
            .rounded(px(if palette.themed { 16. } else { 12. }))
            .overflow_hidden()
            .border_1()
            .border_color(border_muted)
            .bg(palette.theme.map_or_else(
                || color("--background-surface-high"),
                |t| t.modal_background,
            ))
            .children(palette.modal_backdrop())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .relative()
                    .size_full()
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
                    .border_1()
                    .border_color(secondary_border)
                    .bg(secondary_bg)
                    .hover(move |style| style.bg(secondary_hover))
                    .active(move |style| style.bg(secondary_active))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path("icons/modal_close.svg")
                            .size(px(16.))
                            .text_color(secondary_fg),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.close_full_profile(cx))),
            );
        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).
        let pose =
            crate::ui::motion::legacy_modal_pose("full-profile", "full-profile-layer", false);
        Some(
            deferred(
                div()
                    .id("full-profile-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.close_full_profile(cx)))
                    .child(modal.posed(pose)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// `svg-mask-avatar-status-round-120`: a 24px dot at (88, 88) in a 40px hole.
fn status_dot_120(status: &str, ring: Rgba, ring_token: &str) -> impl IntoElement {
    div()
        .absolute()
        .left(px(80.))
        .top(px(80.))
        .size(px(40.))
        .rounded_full()
        .bg(ring)
        .flex()
        .items_center()
        .justify_center()
        .child(super::status_dot_inner(status, ring_token, 24.))
}

#[cfg(test)]
mod tests {
    use super::{Tab, profile_date, profile_route, profile_tabs, time_ago};
    use serde_json::json;

    #[test]
    fn board_tab_appears_for_valid_game_or_application_widgets() {
        let app_widget = json!({
            "widgets": [{"id": "1", "data": {"type": "application", "application_id": "1346069614634864772"}}]
        });
        assert!(profile_tabs(Some(&app_widget), false).contains(&Tab::Board));

        let empty_fav = json!({
            "widgets": [{"id": "1", "data": {"type": "favorite_games", "games": []}}]
        });
        assert!(!profile_tabs(Some(&empty_fav), false).contains(&Tab::Board));

        let valid_fav = json!({
            "widgets": [{"id": "1", "data": {"type": "favorite_games", "games": [{"game_id": "1001"}]}}]
        });
        assert!(profile_tabs(Some(&valid_fav), true).contains(&Tab::Board));
    }

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
