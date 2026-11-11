//! Member activity hover details and replies (289287, 428249, 231188).

use super::{Root, color, editor_style, icon, picker};
use crate::ui::motion::Transitions as _;
use crate::{
    editor::{Editor, EditorEvent},
    net::Request,
    parity::Parity,
};
use dc_state::{
    Id,
    content_inventory::{Entry, GLOBAL_FEED},
};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, Pixels, Point, SharedString,
    Subscription, Task, Window, anchored, deferred, div, img, point, prelude::*, px, rgba,
};
use serde_json::{Value, json};
use std::time::Duration;

const REPLY_LIMIT: usize = 200;
const SEND_KEY: &str = "inventory-reply:";
/// How long a reply may stay unanswered before the popout is released.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct State {
    open: Option<Open>,
    hovered: Option<Id>,
    inside: bool,
    timer: Option<Task<()>>,
    pending: Option<Pending>,
}

struct Open {
    entry: Entry,
    guild: Id,
    channel: Id,
    at: Point<Pixels>,
    editor: Entity<Editor>,
    _events: Subscription,
    palette: [gpui::Hsla; 2],
    palette_source: Option<std::sync::Arc<gpui::Image>>,
    art_height: f32,
}

fn still_waiting(pending: Option<&Pending>, key: &str) -> bool {
    pending.is_some_and(|pending| pending.key == key)
}

struct Pending {
    key: String,
    user: Id,
    content: String,
    entry: Value,
    open_dm: bool,
    channel: Option<Id>,
}

fn reply_body(channel: Id, content: &str, entry: &Value) -> Option<Value> {
    if content.trim().is_empty() || content.encode_utf16().count() > REPLY_LIMIT {
        return None;
    }
    let mut body = crate::net::send_body(channel, content, None);
    body["content_inventory_entry"] = json!({"unverified_content": entry});
    Some(body)
}

fn spotify_url(kind: &str, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    let mut url = reqwest::Url::parse("https://open.spotify.com/").ok()?;
    url.path_segments_mut().ok()?.extend([kind, id]);
    url.query_pairs_mut()
        .append_pair("utm_source", "discord")
        .append_pair("utm_medium", "desktop");
    Some(url.into())
}

fn canonical_game_id(application: &Value) -> Option<&str> {
    // 327891/705751: GAME=5; old detectable applications must resolve a linked GAME.
    if application["type"] == 5 {
        application["id"].as_str()
    } else {
        application["linked_games"]
            .as_array()?
            .iter()
            .find(|game| game["application"]["type"] == 5)?["id"]
            .as_str()
    }
}

fn game_cover_url(game: &Value) -> Option<String> {
    let cover = &game["media"]["cover"];
    let value = cover["value"].as_str().filter(|value| !value.is_empty())?;
    match cover["type"].as_str()? {
        "hash" => Some(format!(
            "https://cdn.discordapp.com/app-icons/{}/{value}.webp?size=128&keep_aspect_ratio=true",
            game["id"].as_str()?
        )),
        "url" => {
            let mut url = reqwest::Url::parse(value).ok()?;
            if matches!(
                url.host_str(),
                Some("images-ext-1.discordapp.net" | "images-ext-2.discordapp.net")
            ) && url.path().starts_with("/external/")
            {
                let query: Vec<(String, String)> = url
                    .query_pairs()
                    .filter(|(key, _)| key != "keep_aspect_ratio" && key != "format")
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect();
                url.set_query(None);
                url.query_pairs_mut()
                    .extend_pairs(query)
                    .append_pair("keep_aspect_ratio", "true")
                    .append_pair("format", "webp");
            }
            Some(url.into())
        }
        _ => None,
    }
}

fn game_steam_url(game: &Value) -> Option<String> {
    let id = game["third_party_skus"].as_array()?.iter().find(|sku| {
        sku["distributor"] == "steam" && sku["id"].as_str().is_some_and(|id| !id.is_empty())
    })?["id"]
        .as_str()?;
    let mut url = reqwest::Url::parse("https://store.steampowered.com/app/").ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(id);
    Some(url.into())
}

fn artwork_style(image: Option<&gpui::Image>) -> ([gpui::Hsla; 2], f32) {
    let decoded = image.and_then(|image| image::load_from_memory(image.bytes()).ok());
    let height = decoded
        .as_ref()
        .filter(|image| image.width() > 0)
        .map_or(72., |image| {
            72. * image.height() as f32 / image.width() as f32
        });
    let colors = decoded.map(|image| crate::palette::avatar_palette(image.to_rgba8().as_raw()));
    let fallback: gpui::Hsla = color("--background-surface-high").into();
    let palette = std::array::from_fn(|index| {
        colors
            .as_ref()
            .and_then(|colors| colors.get(index))
            .map_or(fallback, |rgb| {
                let [r, g, b] = crate::palette::inventory_overlay_color(*rgb, 1.);
                rgba(u32::from_be_bytes([r, g, b, 255])).into()
            })
    });
    (palette, height)
}

impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn inventory_popout_status(&self) -> String {
        format!(
            "open={} inside={} menu={} picker={} pending={}",
            self.inventory_popout.open.is_some(),
            self.inventory_popout.inside,
            self.context.open.is_some(),
            self.picker.is_some(),
            self.inventory_popout.pending.is_some()
        )
    }

    fn inventory_avatar_url(&self, user: &dc_state::User) -> String {
        let (_, original) = self.guild_avatar_source(user);
        let Ok(mut url) = reqwest::Url::parse(&original) else {
            return original;
        };
        if url.query_pairs().any(|(key, _)| key == "size") {
            let query: Vec<(String, String)> = url
                .query_pairs()
                .filter(|(key, _)| key != "size")
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            url.set_query(None);
            url.query_pairs_mut()
                .extend_pairs(query)
                .append_pair("size", "24");
        }
        url.into()
    }
    pub(super) fn prepare_inventory_games(&mut self) {
        // A failed batch: the games it asked for may be asked for again.
        if self.retry_failed_fetch("inventory-games") {
            let fetched = &self.fetched;
            self.requested
                .retain(|key| !key.starts_with("inventory-game:") || fetched.contains_key(key));
        }
        let Some(feed) = self.stores.content_inventory.get_feed(GLOBAL_FEED) else {
            return;
        };
        let games: Vec<String> = feed
            .entries
            .iter()
            .filter_map(|entry| {
                let app = entry.extra["application_id"].as_str()?;
                let application = self
                    .fetched
                    .get(&format!("presence-app:{app}"))?
                    .as_array()?
                    .iter()
                    .find(|item| item["id"] == app)?;
                canonical_game_id(application)
                    .filter(|id| id.parse::<Id>().is_ok())
                    .map(str::to_owned)
            })
            .collect();
        let games: Vec<_> = games
            .into_iter()
            .filter(|id| self.requested.insert(format!("inventory-game:{id}")))
            .collect();
        // 920495 batches at most twenty game IDs.
        for batch in games.chunks(20) {
            let mut url =
                reqwest::Url::parse("https://discord.com/api/v9/games").expect("constant URL");
            for id in batch {
                url.query_pairs_mut().append_pair("game_ids", id);
            }
            let _ = self.requests.send(Request::Fetch {
                key: "inventory-games".to_owned(),
                route: format!("/games?{}", url.query().unwrap_or_default()),
            });
        }
    }

    pub(super) fn inventory_games_fetched(&mut self, key: &str, body: &Value) -> bool {
        if key != "inventory-games" {
            return false;
        }
        for game in body.as_array().into_iter().flatten() {
            let Some(id) = game["id"].as_str() else {
                continue;
            };
            if let Some(url) = game_cover_url(game) {
                self.request_image(format!("game-cover:{id}"), url);
            }
            self.fetched
                .insert(format!("inventory-game:{id}"), game.clone());
        }
        true
    }

    fn inventory_game(&self, entry: &Entry) -> Option<&Value> {
        let app = entry.extra["application_id"].as_str()?;
        let application = self
            .fetched
            .get(&format!("presence-app:{app}"))?
            .as_array()?
            .iter()
            .find(|item| item["id"] == app)?;
        self.fetched.get(&format!(
            "inventory-game:{}",
            canonical_game_id(application)?
        ))
    }
    pub(super) fn inventory_popout_open(&self, id: Id) -> bool {
        self.inventory_popout
            .open
            .as_ref()
            .is_some_and(|open| open.entry.id == id)
    }

    #[cfg(feature = "capture")]
    pub(super) fn inventory_reply_editor(&self) -> Option<Entity<Editor>> {
        self.inventory_popout
            .open
            .as_ref()
            .map(|open| open.editor.clone())
    }

    pub(super) fn close_inventory_popout(&mut self) -> bool {
        self.inventory_popout.timer = None;
        self.inventory_popout.hovered = None;
        self.inventory_popout.inside = false;
        self.inventory_popout.open.take().is_some()
    }

    pub(super) fn inventory_card_hover(&mut self, id: Id, inside: bool, cx: &mut Context<Self>) {
        self.inventory_popout.hovered = inside.then_some(id);
        self.inventory_popout.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.inventory_popout.hovered == Some(id) {
                    this.open_inventory_popout(id, cx);
                } else if !this.inventory_popout.inside
                    && this.picker.is_none()
                    && this.context.open.is_none()
                    && this.profile.is_none()
                {
                    this.inventory_popout.open = None;
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn prepare_inventory_popout(&mut self) {
        if self.inventory_popout.open.as_ref().is_some_and(|open| {
            Some(open.guild) != self.guild
                || Some(open.channel) != self.channel
                || self.inventory_hidden()
                || self.settings.is_some()
                || !self.show_members
        }) {
            self.close_inventory_popout();
        }
        let Some(open) = self.inventory_popout.open.as_ref() else {
            return;
        };
        let art = self.inventory_art(&open.entry);
        let same = match (&art, &open.palette_source) {
            (None, None) => true,
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
            _ => false,
        };
        if !same {
            let (palette, height) = artwork_style(art.as_deref());
            if let Some(open) = self.inventory_popout.open.as_mut() {
                open.palette = palette;
                open.art_height = height;
                open.palette_source = art;
            }
        }
    }

    pub(super) fn open_inventory_popout(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.inventory_popout_open(id) {
            return;
        }
        let Some((guild, channel)) = self.guild.zip(self.channel) else {
            return;
        };
        let Some(entry) = self
            .stores
            .content_inventory
            .get_feed(GLOBAL_FEED)
            .and_then(|feed| feed.entries.iter().find(|entry| entry.id == id))
            .cloned()
        else {
            return;
        };
        let Some(bounds) = self
            .tip_anchors
            .borrow()
            .get(format!("inventory-card-{id}").as_str())
            .copied()
        else {
            return;
        };
        let Some(user) = self.stores.users.get(&entry.author) else {
            return;
        };
        let name = self.guild_display_name(user);
        let avatar_url = self.inventory_avatar_url(user);
        let editor = cx.new(|cx| {
            let mut style = editor_style();
            style.size = 16.;
            style.line_height = 22.;
            style.text = color("--text-default").into();
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
            editor.set_max_utf16(REPLY_LIMIT);
            editor.set_placeholder(format!("Message @{name}"));
            editor
        });
        let events = cx.subscribe(&editor, move |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    this.send_inventory_reply(id, text.to_owned(), true, cx)
                }
                EditorEvent::Cancel => {
                    this.close_inventory_popout();
                }
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        let art = self.inventory_art(&entry);
        let (palette, art_height) = artwork_style(art.as_deref());
        self.inventory_popout.open = Some(Open {
            entry,
            guild,
            channel,
            // The canvas is inside the card's 1px border; the source anchor includes its 8px left margin.
            at: point(bounds.left() - px(17.), bounds.top() - px(1.)),
            editor,
            _events: events,
            palette,
            palette_source: art,
            art_height,
        });
        self.request_image(avatar_url.clone(), avatar_url);
        // The source's hotrail uses the same frequent-emoji list as its picker.
        if self.requested.insert("emoji-sheet".to_owned()) {
            let _ = self.requests.send(Request::EmojiSheet);
        }
        for emoji in self.frequent_emoji().iter().take(5) {
            if let picker::Item::Unicode(index) = emoji {
                self.request_emoji_image(
                    dc_tokens::emoji_data::EMOJIS[usize::from(*index)].surrogates,
                );
            }
        }
        cx.notify();
    }

    fn inventory_art(&self, entry: &Entry) -> Option<std::sync::Arc<gpui::Image>> {
        if entry.kind == 6 {
            self.images
                .get(
                    entry.extra["media"]["image_url"]
                        .as_str()
                        .unwrap_or_default(),
                )
                .cloned()
        } else {
            if let Some(game) = self.inventory_game(entry)
                && let Some(image) = game["id"]
                    .as_str()
                    .and_then(|id| self.images.get(&format!("game-cover:{id}")))
            {
                return Some(image.clone());
            }
            self.images
                .get(&format!(
                    "app-icon:{}",
                    entry.extra["application_id"].as_str().unwrap_or_default()
                ))
                .cloned()
        }
    }

    pub(super) fn send_inventory_reply(
        &mut self,
        entry: Id,
        content: String,
        open_dm: bool,
        cx: &mut Context<Self>,
    ) {
        if self.inventory_popout.pending.is_some() {
            return;
        }
        let Some(open) = self.inventory_popout.open.as_ref() else {
            return;
        };
        if open.entry.id != entry
            || reply_body(0, &content, open.entry.unverified_content()).is_none()
        {
            return;
        }
        #[cfg(feature = "capture")]
        if std::env::var("GRAVECORD_INVENTORY_SEND").as_deref() != Ok("1") {
            tracing::info!("inventory reply suppressed in capture");
            return;
        }
        let user = open.entry.author;
        let channel = self
            .stores
            .private_channels
            .iter()
            .find(|channel| channel.kind == 1 && channel.recipients == [user])
            .map(|channel| channel.id);
        let key = format!("{SEND_KEY}{}", crate::net::nonce());
        let waiting = key.clone();
        self.inventory_popout.pending = Some(Pending {
            key: key.clone(),
            user,
            content,
            entry: open.entry.unverified_content().clone(),
            open_dm,
            channel,
        });
        // A reply whose answer never comes must not keep the popout waiting for good.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REPLY_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                if still_waiting(this.inventory_popout.pending.as_ref(), &waiting) {
                    this.inventory_popout.pending = None;
                    cx.notify();
                }
            });
        })
        .detach();
        if channel.is_some() {
            self.post_inventory_reply();
        } else {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{key}:dm")),
                method: reqwest::Method::POST,
                route: "/users/@me/channels".to_owned(),
                body: Some(json!({"recipients":[user.to_string()]})),
            });
        }
        cx.notify();
    }

    fn post_inventory_reply(&self) {
        let Some(pending) = self.inventory_popout.pending.as_ref() else {
            return;
        };
        let Some(channel) = pending.channel else {
            return;
        };
        let Some(body) = reply_body(channel, &pending.content, &pending.entry) else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(pending.key.clone()),
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/messages"),
            body: Some(body),
        });
    }

    pub(super) fn inventory_reply_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if !key.starts_with(SEND_KEY) {
            return false;
        }
        let Some(pending) = self.inventory_popout.pending.as_mut() else {
            return true;
        };
        if key != pending.key && key != format!("{}:dm", pending.key) {
            return true;
        }
        if body["ok"] != true {
            if let Some(message) = body["body"]["message"].as_str() {
                self.put_app_toast(message.to_owned(), false, cx);
            }
            self.inventory_popout.pending = None;
            cx.notify();
            return true;
        }
        if key.ends_with(":dm") {
            let channel = &body["body"];
            let id = channel["id"].as_str().and_then(|id| id.parse().ok());
            pending.channel = id;
            if id.is_some()
                && channel["recipients"].as_array().is_some_and(|users| {
                    users
                        .iter()
                        .any(|user| user["id"].as_str() == Some(&pending.user.to_string()))
                })
            {
                self.stores.dispatch("CHANNEL_CREATE", channel);
                self.post_inventory_reply();
            } else {
                self.inventory_popout.pending = None;
            }
        } else {
            let pending = self.inventory_popout.pending.take().unwrap();
            if pending.open_dm {
                self.select_home(pending.channel);
            }
            self.close_inventory_popout();
        }
        cx.notify();
        true
    }

    pub(super) fn inventory_popout_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.inventory_popout.open.as_ref()?;
        let entry = &open.entry;
        let user = self.stores.users.get(&entry.author)?;
        let id = entry.id;
        let author = entry.author;
        let art = self.inventory_art(entry);
        let music = entry.kind == 6;
        let title = if music {
            &entry.extra["media"]["parent_title"]
        } else {
            &entry.extra["game_name"]
        }
        .as_str()
        .unwrap_or_default()
        .to_owned();
        let artist = entry.extra["artist"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let album_link = music
            .then(|| {
                spotify_url(
                    "album",
                    entry.extra["media"]["external_parent_id"]
                        .as_str()
                        .unwrap_or_default(),
                )
            })
            .flatten();
        let artist_link = music
            .then(|| {
                spotify_url(
                    "artist",
                    entry.extra["artist"]["external_id"]
                        .as_str()
                        .unwrap_or_default(),
                )
            })
            .flatten();
        let badge = if music {
            format!(
                "Top Artist — {} tracks this week",
                entry
                    .trait_value(12, "count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            )
        } else {
            entry.age_text(chrono::Utc::now().timestamp_millis())
        };
        let header = div()
            .parity("inventory-popout-header")
            .h(px(24.))
            .mb(px(16.))
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .child(
                        self.profile_trigger_side(
                            format!("inventory-popout-avatar-{id}"),
                            author,
                            div().size(px(24.)).flex_none().children(
                                self.images
                                    .get(&self.inventory_avatar_url(user))
                                    .or_else(|| self.images.get(&self.guild_avatar_source(user).0))
                                    .cloned()
                                    .map(|image| img(image).size(px(24.)).rounded_full()),
                            ),
                            true,
                            cx,
                        ),
                    )
                    .child(div().w(px(8.)).flex_none())
                    .child(
                        self.profile_trigger_side(
                            format!("inventory-popout-user-{id}"),
                            author,
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(super::name_styles::name(
                                    format!("inventory-popout-name-{id}"),
                                    self.guild_display_name(user),
                                    self.name_styles(user, self.guild).as_ref(),
                                    super::name_styles::Display::Font,
                                )),
                            true,
                            cx,
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(if music {
                                " listened to"
                            } else if entry.is_live() {
                                " is playing"
                            } else {
                                " played"
                            }),
                    ),
            )
            .child(
                div()
                    .id("inventory-popout-more")
                    .relative()
                    .w(px(16.))
                    .h(px(19.))
                    .child(self.anchor_probe("inventory-popout-more"))
                    .cursor_pointer()
                    .child(icon("more_horizontal", 16., "--text-muted"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let anchor = this
                            .tip_anchors
                            .borrow()
                            .get("inventory-popout-more")
                            .copied();
                        if let Some(anchor) = anchor {
                            this.open_context_menu(
                                super::context_menu::Target::InventoryAuthor { user: author },
                                point(
                                    anchor.right() + px(8.),
                                    px((f32::from(anchor.top()) + 0.5).round()),
                                ),
                                cx,
                            );
                        }
                    })),
            );
        let title_element = div()
            .id("inventory-popout-title")
            .max_w(px(if music { 156. } else { 212. }))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--content-inventory-overlay-text-primary"))
            .child(title)
            .when_some(album_link.clone(), |d, url| {
                d.cursor_pointer()
                    .hover(|s| s.underline())
                    .on_click(cx.listener(move |this, _, _, cx| this.follow_link(&url, "", cx)))
            });
        let thumbnail = div()
            .id("inventory-popout-art")
            .w(px(72.))
            .min_h(px(72.))
            .flex_none()
            .rounded(px(4.))
            .overflow_hidden()
            .children(art.map(|image| img(image).w(px(72.)).h(px(open.art_height)).rounded(px(4.))))
            .when_some(album_link, |d, url| {
                d.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.follow_link(&url, "", cx)))
            });
        let hero_inner = div()
            .relative()
            .w_full()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(thumbnail)
            .child(
                div()
                    .min_w(px(0.))
                    .max_w(px(212.))
                    .flex()
                    .flex_col()
                    .child(title_element)
                    .when(music, |d| {
                        d.child(
                            div()
                                .id("inventory-popout-artist")
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--content-inventory-overlay-text-secondary"))
                                .child(artist)
                                .when_some(artist_link, |d, url| {
                                    d.cursor_pointer().hover(|s| s.underline()).on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.follow_link(&url, "", cx)
                                        }),
                                    )
                                }),
                        )
                    })
                    .child(
                        div()
                            .mt(px(8.))
                            .h(px(16.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex().items_center().gap(px(4.)).child(icon(
                                if music {
                                    "inventory_trophy"
                                } else {
                                    "activity_game"
                                },
                                12.,
                                "--content-inventory-overlay-text-secondary",
                            ))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--content-inventory-overlay-text-secondary"))
                                    .child(badge),
                            ))
                            .when(!music && entry.trait_value(1,"first_time").and_then(Value::as_bool)==Some(true), |d| {
                                d.child(div().flex().items_center().gap(px(4.))
                                    .child(icon("activity_new_player",12.,"--content-inventory-overlay-text-secondary"))
                                    .child(div().text_size(px(12.)).line_height(px(16.)).text_color(color("--content-inventory-overlay-text-secondary")).child("New Player")))
                            }),
                    ),
            )
            .when(music, |d| {
                d.child(div().absolute().top_0().right_0().size(px(16.)).child(icon(
                    "inventory_spotify",
                    16.,
                    "--content-inventory-overlay-text-secondary",
                )))
            });
        let steam = self.inventory_game(entry).and_then(game_steam_url);
        let hero = div()
            .parity("inventory-popout-hero")
            .p(px(8.))
            .rounded(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .bg(gpui::linear_gradient(
                45.,
                gpui::linear_color_stop(open.palette[0], 0.),
                gpui::linear_color_stop(open.palette[1], 1.),
            ))
            .child(hero_inner)
            .when_some(steam, |d, url| {
                d.child(
                    div()
                        .id("inventory-steam")
                        .h(px(32.))
                        .w_full()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-overlay-primary-border-default"))
                        .bg(color("--control-overlay-primary-background-default"))
                        .text_color(color("--control-overlay-primary-text-default"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(color("--control-overlay-primary-background-hover"))
                                .border_color(color("--control-overlay-primary-border-hover"))
                        })
                        .child(icon(
                            "activity_steam",
                            16.,
                            "--control-overlay-primary-text-default",
                        ))
                        .child("Get on Steam")
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.follow_link(&url, "", cx)),
                        ),
                )
            });
        let emoji = self
            .frequent_emoji()
            .into_iter()
            .take(5)
            .map(|item| {
                let key = item.image_key(self.skin_tone);
                let text = item.message_text(self.skin_tone);
                div()
                    .id(SharedString::from(format!("inventory-emoji-{key}")))
                    .w(px(24.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .children(
                        self.images
                            .get(&key)
                            .cloned()
                            .map(|image| img(image).size(px(24.))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send_inventory_reply(id, text.clone(), false, cx)
                    }))
            })
            .collect::<Vec<_>>();
        let input = open.editor.clone();
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let replies = div()
            .parity("inventory-popout-replies")
            .relative()
            .p(px(16.))
            .rounded_b(px(12.))
            .bg(color("--background-surface-high"))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(16.))
                    .right(px(16.))
                    .h(px(1.))
                    .bg(color("--border-subtle")),
            )
            .child(
                div()
                    .h(px(48.))
                    .px(px(12.))
                    .pb(px(16.))
                    .flex()
                    .justify_between()
                    .items_center()
                    .children(emoji)
                    .child(
                        div()
                            .id("inventory-add-reaction")
                            .size(px(18.))
                            .child(self.anchor_probe("inventory-add-reaction"))
                            .cursor_pointer()
                            .child(icon("menu_add_reaction", 18., "--interactive-icon-default"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let anchor = this
                                    .tip_anchors
                                    .borrow()
                                    .get("inventory-add-reaction")
                                    .copied();
                                let Some(anchor) = anchor else { return };
                                this.open_picker(
                                    picker::Target::InventoryReply(id),
                                    point(anchor.right(), anchor.top()),
                                    window,
                                    cx,
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .parity_with_border("inventory-popout-input", 1.)
                    .id("inventory-popout-input")
                    .h(px(38.))
                    .border_1()
                    .rounded(px(8.))
                    .border_color(color(if focused {
                        "--input-border-active"
                    } else {
                        "--input-border-default"
                    }))
                    .rule(crate::ui::motion::Rule::TextField)
                    .bg(color("--input-background-default"))
                    .pl(px(15.))
                    .pr(px(12.))
                    .flex()
                    .items_center()
                    .child(div().flex_1().min_w(px(0.)).child(input.clone()))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        window.focus(&input.read(cx).focus_handle(cx), cx)
                    })),
            );
        let popup = div()
            .parity("inventory-popout")
            .id("inventory-popout")
            .w(px(348.))
            .occlude()
            .flex()
            .flex_col()
            .shadow(vec![gpui::BoxShadow {
                color: color("--opacity-black-32").into(),
                offset: point(px(0.), px(8.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                this.inventory_popout.inside = *inside;
                if !inside {
                    this.inventory_card_hover(id, false, cx);
                }
            }))
            .child(
                div()
                    .p(px(16.))
                    .rounded_t(px(12.))
                    .bg(color("--background-surface-high"))
                    .child(header)
                    .child(hero),
            )
            .child(replies);
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopRight)
                    .position(open.at)
                    .snap_to_window_with_margin(px(8.))
                    .child(popup),
            )
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_reply_that_timed_out_is_released() {
        let pending = Pending {
            key: "inventory-reply:1".into(),
            user: 1,
            content: String::new(),
            entry: Value::Null,
            open_dm: false,
            channel: None,
        };
        assert!(still_waiting(Some(&pending), "inventory-reply:1"));
        // A newer reply keeps its own wait; a finished one has nothing left to release.
        assert!(!still_waiting(Some(&pending), "inventory-reply:0"));
        assert!(!still_waiting(None, "inventory-reply:1"));
        assert!(REPLY_TIMEOUT > Duration::ZERO);
    }

    #[test]
    fn reply_preserves_inventory_content_and_uses_utf16_limit() {
        let entry = json!({"id":"1","author_id":"3","author_type":1,"content_type":6,"extra":{"artist":{"name":"Fixture"}},"traits":[],"future_field":true});
        let parsed = Entry::from_value(&entry).unwrap();
        let body = reply_body(2, "hello", parsed.unverified_content()).unwrap();
        assert_eq!(body["content_inventory_entry"]["unverified_content"], entry);
        assert_eq!(body["content"], "hello");
        assert!(reply_body(2, &"😀".repeat(100), &entry).is_some());
        assert!(reply_body(2, &"😀".repeat(101), &entry).is_none());
        assert!(reply_body(2, " \n", &entry).is_none());
        assert_eq!(
            spotify_url("album", "abc").as_deref(),
            Some("https://open.spotify.com/album/abc?utm_source=discord&utm_medium=desktop")
        );
    }

    #[test]
    fn game_cover_and_distributor_use_canonical_metadata() {
        assert_eq!(canonical_game_id(&json!({"id":"1","type":1})), None);
        assert_eq!(canonical_game_id(&json!({"id":"1","type":5})), Some("1"));
        assert_eq!(
            canonical_game_id(
                &json!({"id":"1","type":1,"linked_games":[{"id":"2","application":{"type":5}}]})
            ),
            Some("2")
        );
        assert_eq!(
            game_cover_url(&json!({"id":"2","media":{"cover":{"type":"hash","value":"cover"}}}))
                .as_deref(),
            Some(
                "https://cdn.discordapp.com/app-icons/2/cover.webp?size=128&keep_aspect_ratio=true"
            )
        );
        let game = json!({"id":"2","media":{"cover":{"type":"url","value":"https://images-ext-1.discordapp.net/external/example/cover.jpg?format=png&width=128"}},"third_party_skus":[{"distributor":"steam","id":"20"},{"distributor":"steam","id":"21"}]});
        assert_eq!(
            game_cover_url(&game).as_deref(),
            Some(
                "https://images-ext-1.discordapp.net/external/example/cover.jpg?width=128&keep_aspect_ratio=true&format=webp"
            )
        );
        assert_eq!(
            game_steam_url(&game).as_deref(),
            Some("https://store.steampowered.com/app/20")
        );
        assert!(game_cover_url(&json!({"media":{"cover":{"type":"hash","value":""}}})).is_none());
    }
}
