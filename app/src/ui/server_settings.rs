//! Server settings (the guild menu's Server Settings) on the `standardSidebarView_jnk11m` shell: the server's name
//! over Server Profile, Server Tag, Engagement, Boost Perks; Expression; People; Apps; Moderation; Enable Community,
//! Server Template and Delete Server. Measured in `design/dom/settings/guild-*-darker-cozy.json`.
//!
//! Server Profile (module 989433 `I7`, fields in module 9994): Name, Icon, Banner (the server icon's color or one of
//! module 282435's presets, drawn as module 649640's `n6` gradient), Traits, Description, Private Profile, beside the
//! invite card preview. Edits stay local until Save, which sends the whole profile as module 9994 `ow` does in one
//! `PATCH /guilds/{id}/profile` (module 250527 `_C`).

use super::{Root, color, icon, window_controls};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, PathPromptOptions, SharedString,
    Subscription, Window, deferred, div, img, prelude::*, px,
};

pub(super) const PROFILE_KEY: &str = "guild-profile:";
/// The save's reply comes back under this key, then is read like a fetch.
const SAVE_KEY: &str = "guild-profile:save:";
/// Module 280513 `n`: guild profile visibility; `i.VISIBLE` is PUBLIC and PUBLIC_WITH_RECRUITMENT.
const VISIBILITY_PUBLIC: u64 = 1;
const VISIBILITY_RESTRICTED: u64 = 2;
const VISIBILITY_PUBLIC_WITH_RECRUITMENT: u64 = 3;
/// Module 282435 `aA`: the banner presets after "Server Icon Color".
pub(super) const BANNER_PRESETS: [(&str, &str); 9] = [
    ("Soul", "#ff1c90"),
    ("Volcano", "#e81d1e"),
    ("Marsh", "#e86e1d"),
    ("Thunder", "#e8c02f"),
    ("Rising", "#71368a"),
    ("Zephyr", "#029FFC"),
    ("Cascade", "#4fe2ca"),
    ("Earth", "#406601"),
    ("Boulder", "#272727"),
];
/// Module 989433 `I0`: a trait label is at most 24 characters.
const TRAIT_MAX: usize = 24;

/// `#rrggbb` as bytes.
pub(super) fn hex_rgb(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Profile,
    Tag,
    Engagement,
    BoostPerks,
    Emoji,
    Stickers,
    Soundboard,
    Members,
    Roles,
    Invites,
    Access,
    Integrations,
    AppDirectory,
    SafetySetup,
    AuditLog,
    Bans,
    AutoMod,
    Community,
    Template,
}

/// The sidebar as captured: group headers, then their entries; `None` marks a separator.
/// A sidebar group: its header (the first takes the server's name), then its entries.
type Group = (Option<&'static str>, &'static [(Page, &'static str)]);
const SIDEBAR: &[Option<Group>] = &[
    Some((
        None,
        &[
            (Page::Profile, "Server Profile"),
            (Page::Tag, "Server Tag"),
            (Page::Engagement, "Engagement"),
            (Page::BoostPerks, "Boost Perks"),
        ],
    )),
    None,
    Some((
        Some("Expression"),
        &[
            (Page::Emoji, "Emoji"),
            (Page::Stickers, "Stickers"),
            (Page::Soundboard, "Soundboard"),
        ],
    )),
    None,
    Some((
        Some("People"),
        &[
            (Page::Members, "Members"),
            (Page::Roles, "Roles"),
            (Page::Invites, "Invites"),
            (Page::Access, "Access"),
        ],
    )),
    None,
    Some((
        Some("Apps"),
        &[
            (Page::Integrations, "Integrations"),
            (Page::AppDirectory, "App Directory"),
        ],
    )),
    None,
    Some((
        Some("Moderation"),
        &[
            (Page::SafetySetup, "Safety Setup"),
            (Page::AuditLog, "Audit Log"),
            (Page::Bans, "Bans"),
            (Page::AutoMod, "AutoMod"),
        ],
    )),
    None,
    Some((None, &[(Page::Community, "Enable Community")])),
    None,
    Some((None, &[(Page::Template, "Server Template")])),
];

/// A trait's emoji as the API stores it (`emoji_id`, `emoji_name`, `emoji_animated`); a unicode emoji goes by its
/// first name (`uniqueName` in module 7584).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TraitEmoji {
    pub id: Option<String>,
    pub name: Option<String>,
    pub animated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Values {
    name: String,
    description: String,
    /// The icon's hash, a picked image's data URI, or none.
    icon: Option<String>,
    custom_banner: Option<String>,
    color: Option<String>,
    traits: [(String, Option<TraitEmoji>); 5],
    visibility: u64,
    games: Vec<String>,
}

impl Values {
    /// Module 9994 `wr`: the profile body as the settings hold it.
    fn from_profile(body: &serde_json::Value) -> Self {
        let text = |key: &str| {
            body[key]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let mut traits: [(String, Option<TraitEmoji>); 5] = Default::default();
        for entry in body["traits"].as_array().into_iter().flatten() {
            let Some(position) = entry["position"].as_u64().filter(|position| *position < 5) else {
                continue;
            };
            let emoji = TraitEmoji {
                id: entry["emoji_id"].as_str().map(str::to_owned),
                name: entry["emoji_name"].as_str().map(str::to_owned),
                animated: entry["emoji_animated"].as_bool().unwrap_or(false),
            };
            let has_emoji = emoji.id.is_some() || emoji.name.is_some();
            traits[position as usize] = (
                entry["label"].as_str().unwrap_or_default().to_owned(),
                has_emoji.then_some(emoji),
            );
        }
        Values {
            name: body["name"].as_str().unwrap_or_default().to_owned(),
            description: body["description"].as_str().unwrap_or_default().to_owned(),
            icon: text("icon_hash"),
            custom_banner: text("custom_banner_hash"),
            color: text("brand_color_primary"),
            traits,
            visibility: body["visibility"].as_u64().unwrap_or(0),
            games: body["game_application_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect(),
        }
    }

    /// Module 9994 `ow`: what Save sends.
    fn body(&self) -> serde_json::Value {
        let traits: Vec<serde_json::Value> = self
            .traits
            .iter()
            .enumerate()
            .filter(|(_, (label, _))| !label.is_empty())
            .map(|(position, (label, emoji))| {
                let mut entry = serde_json::json!({"label": label, "position": position});
                if let Some(emoji) = emoji {
                    entry["emoji_id"] = serde_json::json!(emoji.id);
                    entry["emoji_name"] = serde_json::json!(emoji.name);
                    entry["emoji_animated"] = serde_json::json!(emoji.animated);
                }
                entry
            })
            .collect();
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "icon": self.icon,
            "custom_banner": self.custom_banner,
            "visibility": self.visibility,
            "brand_color_primary": self.color,
            "traits": traits,
            "game_application_ids": self.games,
        })
    }
}

/// Module 989433 `I4`: Private Profile is on unless the visibility is one of the visible ones (module 280513 `i`).
fn is_private(visibility: u64) -> bool {
    !matches!(
        visibility,
        VISIBILITY_PUBLIC | VISIBILITY_PUBLIC_WITH_RECRUITMENT
    )
}

pub(crate) struct ServerSettings {
    pub guild: Id,
    pub page: Page,
    pub(super) name: Entity<Editor>,
    pub(super) description: Entity<Editor>,
    pub(super) traits: [Entity<Editor>; 5],
    pub(super) trait_emoji: [Option<TraitEmoji>; 5],
    icon: Option<String>,
    custom_banner: Option<String>,
    color: Option<String>,
    visibility: u64,
    pub(super) games: Vec<String>,
    /// The Games search field, whether its list is open, and the last query whose results arrived.
    pub(super) games_query: Entity<Editor>,
    pub(super) games_open: bool,
    pub(super) games_shown_query: Option<String>,
    /// The profile as fetched (or last saved): Reset goes back to it, and the notice shows while the page differs.
    saved: Values,
    /// The profile has arrived; until then the page is empty, as the official one shows its spinner.
    loaded: bool,
    /// Delete Server's name field while it is open, and whether the typed name was wrong.
    pub(super) delete_name: Option<(Entity<Editor>, Subscription)>,
    delete_wrong: bool,
    /// The Bans page's search and open ban, made when the page first opens.
    pub(super) bans: Option<super::server_settings_pages::BansState>,
    /// The Emoji page's name fields.
    pub(super) emoji: Option<super::server_emoji::EmojiState>,
    _events: Vec<Subscription>,
}

impl ServerSettings {
    fn values(&self, cx: &Context<Root>) -> Values {
        let traits = std::array::from_fn(|index| {
            (
                self.traits[index].read(cx).text().to_owned(),
                self.trait_emoji[index].clone(),
            )
        });
        Values {
            name: self.name.read(cx).text().to_owned(),
            description: self.description.read(cx).text().to_owned(),
            icon: self.icon.clone(),
            custom_banner: self.custom_banner.clone(),
            color: self.color.clone(),
            traits,
            visibility: self.visibility,
            games: self.games.clone(),
        }
    }

    /// Puts `values` on the page.
    fn show(&mut self, values: &Values, cx: &mut Context<Root>) {
        self.name
            .update(cx, |editor, _| editor.set_text(&values.name));
        self.description
            .update(cx, |editor, _| editor.set_text(&values.description));
        for (editor, (label, _)) in self.traits.iter().zip(values.traits.iter()) {
            editor.update(cx, |editor, _| editor.set_text(label));
        }
        self.trait_emoji = std::array::from_fn(|index| values.traits[index].1.clone());
        self.icon = values.icon.clone();
        self.custom_banner = values.custom_banner.clone();
        self.color = values.color.clone();
        self.visibility = values.visibility;
        self.games = values.games.clone();
    }
}

/// An image file as the data URI the API takes for icons.
pub(super) fn data_uri(path: &std::path::Path) -> Option<String> {
    use base64::Engine;
    let bytes = std::fs::read(path).ok()?;
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

impl Root {
    pub(super) fn open_server_settings(&mut self, guild: Id, cx: &mut Context<Self>) {
        if !self.stores.guilds.iter().any(|known| known.id == guild) {
            return;
        }
        let plain = |cx: &mut Context<Self>| cx.new(|cx| Editor::new(super::editor_style(), cx));
        let name = plain(cx);
        let description = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Tell the world a bit about this server.".to_owned());
            editor
        });
        let traits: [Entity<Editor>; 5] = std::array::from_fn(|_| plain(cx));
        let mut events = Vec::new();
        for editor in std::iter::once(&name)
            .chain(std::iter::once(&description))
            .chain(traits.iter())
        {
            events.push(
                cx.subscribe(editor, |this, editor, event: &EditorEvent, cx| {
                    match event {
                        EditorEvent::Submit(text) => {
                            let text = text.clone();
                            editor.update(cx, |editor, _| editor.set_text(&text));
                        }
                        EditorEvent::Cancel => this.server_settings = None,
                        EditorEvent::Edited => {}
                    }
                    // Traits stop at 24 characters (`maxLength` on module 989433 `I0`'s field).
                    if let Some(open) = this.server_settings.as_ref()
                        && open.traits.contains(&editor)
                    {
                        let text = editor.read(cx).text().to_owned();
                        if text.chars().count() > TRAIT_MAX {
                            let cut: String = text.chars().take(TRAIT_MAX).collect();
                            editor.update(cx, |editor, _| editor.set_text(&cut));
                        }
                    }
                    cx.notify();
                }),
            );
        }
        let (games_query, games_events) = Self::new_games_query(cx);
        events.push(games_events);
        self.server_settings = Some(ServerSettings {
            guild,
            page: Page::Profile,
            name,
            description,
            traits,
            trait_emoji: Default::default(),
            icon: None,
            custom_banner: None,
            color: None,
            visibility: 0,
            games: Vec::new(),
            games_query,
            games_open: false,
            games_shown_query: None,
            saved: Values::default(),
            loaded: false,
            delete_name: None,
            delete_wrong: false,
            bans: None,
            emoji: None,
            _events: events,
        });
        let _ = self.requests.send(Request::Fetch {
            key: format!("{PROFILE_KEY}{guild}"),
            route: format!("/guilds/{guild}/profile"),
        });
        self.request_profile_games(guild);
        cx.notify();
    }

    /// The guild profile arrived (or Save's reply did): it becomes the saved values and fills the page.
    pub(super) fn server_profile_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        let body = if key == format!("{SAVE_KEY}{}", open.guild) {
            let report = &self.fetched[key];
            if report["ok"] != true {
                return;
            }
            report["body"].clone()
        } else if key == format!("{PROFILE_KEY}{}", open.guild) {
            self.fetched[key].clone()
        } else {
            return;
        };
        let values = Values::from_profile(&body);
        open.show(&values, cx);
        open.saved = values;
        open.loaded = true;
        let wanted: Vec<String> = open
            .saved
            .traits
            .iter()
            .filter_map(|(_, emoji)| match emoji {
                Some(TraitEmoji {
                    id: None,
                    name: Some(name),
                    ..
                }) => dc_tokens::emoji_by_name(name).map(|found| found.surrogates.to_owned()),
                _ => None,
            })
            .collect();
        for surrogates in wanted {
            self.request_emoji_image(&surrogates);
        }
        let picked = self
            .server_settings
            .as_ref()
            .map(|open| open.saved.games.clone())
            .unwrap_or_default();
        self.request_games(&picked);
        self.request_game_images();
        cx.notify();
    }

    fn pick_server_icon(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(uri) = paths.first().and_then(|path| data_uri(path))
            {
                let _ = this.update(cx, |this, cx| {
                    if let Some(open) = this.server_settings.as_mut() {
                        open.icon = Some(uri);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn open_delete_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| Editor::new(super::editor_style(), cx));
        let events = cx.subscribe(&field, |this, _, event: &EditorEvent, cx| {
            if let Some(open) = this.server_settings.as_mut() {
                match event {
                    EditorEvent::Cancel => open.delete_name = None,
                    EditorEvent::Edited => open.delete_wrong = false,
                    EditorEvent::Submit(_) => {}
                }
            }
            cx.notify();
        });
        let handle = gpui::Focusable::focus_handle(field.read(cx), cx);
        window.focus(&handle, cx);
        if let Some(open) = self.server_settings.as_mut() {
            open.delete_name = Some((field, events));
            open.delete_wrong = false;
        }
        cx.notify();
    }

    fn reset_server_settings(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        let saved = open.saved.clone();
        open.show(&saved, cx);
        cx.notify();
    }

    fn save_server_settings(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_ref() else {
            return;
        };
        let guild = open.guild;
        let body = open.values(cx).body();
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SAVE_KEY}{guild}")),
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}/profile"),
            body: Some(body),
        });
        cx.notify();
    }

    /// The trait emoji's picture: a unicode emoji's twemoji, a custom one's image.
    pub(super) fn trait_emoji_image(
        &self,
        emoji: &TraitEmoji,
    ) -> Option<std::sync::Arc<gpui::Image>> {
        match (&emoji.id, &emoji.name) {
            (Some(id), _) => self.images.get(&format!("custom:{id}")).cloned(),
            (None, Some(name)) => {
                let found = dc_tokens::emoji_by_name(name)?;
                self.images
                    .get(&format!("emoji:{}", found.surrogates))
                    .cloned()
            }
            _ => None,
        }
    }

    pub(super) fn set_trait_emoji(
        &mut self,
        index: usize,
        emoji: TraitEmoji,
        cx: &mut Context<Self>,
    ) {
        if let Some(TraitEmoji {
            id: None,
            name: Some(name),
            ..
        }) = Some(&emoji)
            && let Some(found) = dc_tokens::emoji_by_name(name)
        {
            let surrogates = found.surrogates.to_owned();
            self.request_emoji_image(&surrogates);
        }
        if let Some(open) = self.server_settings.as_mut()
            && index < 5
        {
            open.trait_emoji[index] = Some(emoji);
        }
        cx.notify();
    }

    fn server_profile_page(&self, open: &ServerSettings, cx: &mut Context<Self>) -> AnyElement {
        if !open.loaded {
            return div().w(px(415.)).into_any_element();
        }
        // `divider_mL61Cw`: 40px either side, `--border-normal`'s 0.44 alpha.
        let divider = || {
            div()
                .flex_none()
                .my(px(40.))
                .h(px(1.))
                .bg(gpui::rgba(0x97979f70))
        };
        let field = |editor: Entity<Editor>, height: f32| {
            div()
                .h(px(height))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .px(px(12.))
                .when(height <= 40., |this| this.flex().items_center())
                .when(height > 40., |this| this.pt(px(10.)))
                .child(div().w_full().child(editor))
        };
        let label = |text: &'static str| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let hint = |text: &'static str| {
            div()
                .mt(px(4.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-subtle"))
                .child(text)
        };
        let private = is_private(open.visibility);
        let guild = open.guild;

        // Icon: Change Server Icon, and Remove Icon while there is one (module 989433 `IW`).
        let icon_buttons = div()
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .id("server-settings-icon")
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
                    .child("Change Server Icon")
                    .on_click(cx.listener(|this, _, _, cx| this.pick_server_icon(cx))),
            )
            .when(open.icon.is_some(), |this| {
                this.child(
                    div()
                        .id("server-settings-remove-icon")
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-critical-secondary-border-default"))
                        .bg(color("--control-critical-secondary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-critical-secondary-text-default"))
                        .child("Remove Icon")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(open) = this.server_settings.as_mut() {
                                open.icon = None;
                            }
                            cx.notify();
                        })),
                )
            });

        // Banner: "Server Icon Color" then the presets, 3 to a row (module 989433 `Ib`, `grid_EAKysR`).
        let icon_base = self.guild_banner_base(guild);
        let mut tiles: Vec<AnyElement> = Vec::new();
        let presets = std::iter::once(("Server Icon Color", None))
            .chain(BANNER_PRESETS.iter().map(|(name, hex)| (*name, Some(*hex))));
        for (name, hex) in presets {
            let base = hex.and_then(hex_rgb).unwrap_or(icon_base);
            let selected = match hex {
                None => open.custom_banner.is_none() && open.color.is_none(),
                Some(hex) => open.custom_banner.is_none() && open.color.as_deref() == Some(hex),
            };
            let picture = self.banner_image(base, 131, 62);
            tiles.push(
                div()
                    .id(SharedString::from(format!("server-banner-{name}")))
                    .relative()
                    .w(px(133.))
                    .h(px(64.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .cursor_pointer()
                    .child(img(picture).w(px(131.)).h(px(62.)).rounded(px(7.)))
                    .when(selected, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(-4.))
                                .left(px(-4.))
                                .w(px(139.))
                                .h(px(70.))
                                .rounded(px(10.))
                                .border_1()
                                .border_color(color("--brand-400")),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this.server_settings.as_mut() {
                            open.color = hex.map(str::to_owned);
                            open.custom_banner = None;
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let mut grid = div().flex().flex_col().gap(px(8.));
        let mut tiles = tiles.into_iter().peekable();
        while tiles.peek().is_some() {
            grid = grid.child(div().flex().gap(px(8.)).children(tiles.by_ref().take(3)));
        }

        // Traits: five fields, two to a row, each with its emoji button (module 989433 `I2`, `I0`).
        let emoji_button = self.images.get("emoji_button").cloned();
        let mut trait_rows = div().flex().flex_col().gap(px(8.));
        for row in 0..3 {
            let mut line = div().flex().gap(px(8.));
            for index in (row * 2)..(row * 2 + 2).min(5) {
                let editor = open.traits[index].clone();
                let emoji = open.trait_emoji[index]
                    .as_ref()
                    .and_then(|emoji| self.trait_emoji_image(emoji));
                let filled =
                    !editor.read(cx).text().is_empty() || open.trait_emoji[index].is_some();
                let probe = SharedString::from(format!("server-trait-emoji-{index}"));
                line = line.child(
                    div()
                        .w(px(204.))
                        .h(px(40.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .pl(px(4.))
                        .pr(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .id(probe.clone())
                                .relative()
                                .child(self.anchor_probe(probe.clone()))
                                .size(px(30.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when_some(emoji.or(emoji_button.clone()), |this, picture| {
                                    this.child(img(picture).size(px(18.)))
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let key = format!("server-trait-emoji-{index}");
                                    let anchor =
                                        this.tip_anchors.borrow().get(key.as_str()).copied();
                                    if let Some(bounds) = anchor {
                                        this.open_picker(
                                            super::picker::Target::Trait(index as u8),
                                            // The 18px button sits 6px into its 30px box.
                                            gpui::point(bounds.left() + px(6.), bounds.top()),
                                            window,
                                            cx,
                                        );
                                    }
                                })),
                        )
                        .child(div().flex_1().min_w(px(0.)).child(editor.clone()))
                        .when(filled, |this| {
                            this.child(
                                div()
                                    .id(SharedString::from(format!("server-trait-clear-{index}")))
                                    .cursor_pointer()
                                    .child(icon("search_clear", 16., "--icon-muted"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(open) = this.server_settings.as_mut() {
                                            open.traits[index]
                                                .update(cx, |editor, _| editor.set_text(""));
                                            open.trait_emoji[index] = None;
                                        }
                                        cx.notify();
                                    })),
                            )
                        }),
                );
            }
            trait_rows = trait_rows.child(line);
        }

        // Private Profile beside the card non-members see, faded while the profile is public (module 989433 `I4`).
        let private_card = self.private_server_card(guild, open.color.as_deref());
        let private_row = div()
            .flex()
            .gap(px(32.))
            .child(
                div()
                    .w(px(192.))
                    .flex()
                    .gap(px(24.))
                    .child(
                        div()
                            .w(px(120.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label("Private Profile"))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child("When enabled, only server members can view profile content. Non-members won't be able to see this content unless they have an invite."),
                            ),
                    )
                    .child(
                        div()
                            .id("server-settings-private")
                            .cursor_pointer()
                            .child(super::create_channel::switch(private))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.server_settings.as_mut() {
                                    open.visibility = if !is_private(open.visibility) {
                                        VISIBILITY_RESTRICTED
                                    } else if open.saved.visibility == VISIBILITY_PUBLIC_WITH_RECRUITMENT {
                                        VISIBILITY_PUBLIC_WITH_RECRUITMENT
                                    } else {
                                        VISIBILITY_PUBLIC
                                    };
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(div().w(px(192.)).when(!private, |this| this.opacity(0.5)).child(private_card));

        div()
            .w(px(415.))
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Server Profile"),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child("Customize how your server appears in invite links and, if enabled, in Server Discovery and Announcement Channel messages"),
            )
            .child(div().mt(px(40.)).child(Self::field_label("Name")).child(field(open.name.clone(), 40.)))
            .child(divider())
            .child(label("Icon"))
            .child(hint("We recommend an image of at least 512x512.").mb(px(8.)))
            .child(icon_buttons)
            .child(divider())
            .child(label("Banner").mb(px(8.)))
            .child(grid)
            .child(divider())
            .child(label("Traits"))
            .child(hint("Add up to 5 traits to show off your server\u{2019}s interests and personality.").mb(px(8.)))
            .child(trait_rows)
            .child(divider())
            .child(label("Description"))
            .child(hint("How did your server get started? Why should people join?").mb(px(8.)))
            .child(
                div()
                    .id("server-settings-description")
                    .cursor_text()
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(open) = this.server_settings.as_ref() {
                            let handle = gpui::Focusable::focus_handle(open.description.read(cx), cx);
                            window.focus(&handle, cx);
                        }
                    }))
                    .child(field(open.description.clone(), 89.)),
            )
            .child(divider())
            .child(self.server_games_section(cx))
            .child(divider())
            .child(private_row)
            .into_any_element()
    }

    /// Module 576055: the card non-members see for a private server, the banner over "Private Server".
    fn private_server_card(&self, guild: Id, color_hex: Option<&str>) -> AnyElement {
        let base = color_hex
            .and_then(hex_rgb)
            .unwrap_or_else(|| self.guild_banner_base(guild));
        let banner = self.banner_image(base, 190, 120);
        let known = self.stores.guilds.iter().find(|known| known.id == guild);
        // Module 576055 is given `name: null` here: no picture, a "?" in its place, as captured.
        let _ = known;
        let picture = None;
        let acronym = "?".to_owned();
        div()
            .w(px(192.))
            .rounded(px(16.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-highest"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(img(banner).w(px(190.)).h(px(120.)).rounded_t(px(15.)))
            .child(
                div()
                    .mt(px(-34.))
                    .px(px(16.))
                    .child(self.profile_card_avatar(picture, acronym)),
            )
            .child(
                div()
                    .mt(px(8.))
                    .mb(px(8.))
                    .px(px(16.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Private Server"),
            )
            .child(
                div()
                    .mb(px(16.))
                    .px(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child("The server has limited who can see this profile."),
            )
            .into_any_element()
    }

    /// A page this client does not draw yet: its title, so the entry still lands somewhere visible.
    fn server_page_title(title: &'static str) -> AnyElement {
        div()
            .text_size(px(20.))
            .line_height(px(24.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child(title)
            .into_any_element()
    }

    pub(super) fn server_settings_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == open.guild)?;
        let page = open.page;
        let dirty = open.values(cx) != open.saved;
        let mut nav = div()
            .w(px(264.))
            .pt(px(60.))
            .pl(px(20.))
            .pr(px(6.))
            .pb(px(60.))
            .flex()
            .flex_col();
        for (index, group) in SIDEBAR.iter().enumerate() {
            match group {
                None => {
                    nav = nav.child(
                        div()
                            .flex_none()
                            .mx(px(10.))
                            .my(px(8.))
                            .h(px(1.))
                            .bg(color("--border-subtle")),
                    )
                }
                Some((header, items)) => {
                    let header = if index == 0 {
                        Some(guild.name.clone())
                    } else {
                        header.map(str::to_owned)
                    };
                    if let Some(header) = header {
                        nav = nav.child(
                            div()
                                .flex_none()
                                .h(px(if index == 0 { 24. } else { 30. }))
                                .px(px(10.))
                                .pb(px(6.))
                                .flex()
                                .items_end()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--channels-default"))
                                .child(header),
                        );
                    }
                    for (target, label) in items.iter() {
                        let target = *target;
                        let external = target == Page::AppDirectory;
                        nav = nav.child(
                            div()
                                .id(SharedString::from(format!("server-settings-{label}")))
                                .flex_none()
                                .h(px(32.))
                                .mb(px(2.))
                                .px(px(10.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(if target == page {
                                    color("--text-strong")
                                } else {
                                    color("--text-subtle")
                                })
                                .when(target == page, |this| {
                                    this.bg(color("--background-mod-strong"))
                                })
                                .when(target != page, |this| {
                                    this.hover(|style| style.bg(color("--background-mod-subtle")))
                                })
                                .child(div().flex_1().child(*label))
                                .when(external, |this| {
                                    this.child(icon("open_external", 16., "--icon-subtle"))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if external {
                                        // App Directory opens on the web, as the official entry's arrow says.
                                        cx.open_url(&format!(
                                            "https://discord.com/application-directory?guild_id={}",
                                            this.server_settings
                                                .as_ref()
                                                .map_or(0, |open| open.guild)
                                        ));
                                        return;
                                    }
                                    let Some(open) = this.server_settings.as_mut() else {
                                        return;
                                    };
                                    open.page = target;
                                    let guild = open.guild;
                                    this.request_server_page(guild, target, cx);
                                    cx.notify();
                                })),
                        );
                    }
                }
            }
        }
        nav = nav.child(
            div()
                .id("server-settings-delete")
                .flex_none()
                .h(px(32.))
                .px(px(10.))
                .rounded(px(4.))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-feedback-critical"))
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .child(div().flex_1().child("Delete Server"))
                .child(icon(
                    "channel_settings_trash",
                    16.,
                    "--text-feedback-critical",
                ))
                .on_click(cx.listener(|this, _, window, cx| this.open_delete_server(window, cx))),
        );
        let content: AnyElement = match page {
            Page::Profile => self.server_profile_page(open, cx),
            Page::Tag => Self::server_page_title("Server Tag"),
            Page::Engagement => Self::server_page_title("Engagement"),
            Page::BoostPerks => Self::server_page_title("Boost Perks"),
            Page::Emoji => self.server_emoji_page(open.guild, cx),
            Page::Stickers => Self::server_page_title("Stickers"),
            Page::Soundboard => Self::server_page_title("Soundboard"),
            Page::Members => Self::server_page_title("Server Members"),
            Page::Roles => Self::server_page_title("Roles"),
            Page::Invites => self.server_invites_page(open.guild, cx),
            Page::Access => Self::server_page_title("Access"),
            Page::Integrations => Self::server_page_title("Integrations"),
            Page::AppDirectory => Self::server_page_title("App Directory"),
            Page::SafetySetup => Self::server_page_title("Safety Setup"),
            Page::AuditLog => Self::server_page_title("Audit Log"),
            Page::Bans => self.server_bans_page(open.guild, cx),
            Page::AutoMod => Self::server_page_title("AutoMod"),
            Page::Community => Self::server_page_title("Enable Community"),
            Page::Template => Self::server_page_title("Server Template"),
        };
        // ESC sits right of the content column: 835 beside Server Profile's card, 740 after a plain page.
        let close_left = if page == Page::Profile { 835. } else { 740. };
        // Custom pages (Invites) put ESC 32px higher: their scroller starts at the window's top.
        let close_top = if page == Page::Invites { 0. } else { 32. };
        let preview = (page == Page::Profile).then(|| {
            let name = open.name.read(cx).text().to_owned();
            let body = self.server_preview_body(cx);
            div().w(px(300.)).flex_none().children(self.invite_preview(
                open.guild,
                open.color.as_deref().and_then(hex_rgb),
                Some(name),
                body,
                cx,
            ))
        });
        let close = div()
            .pt(px(60.))
            .w(px(36.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .id("server-settings-close")
                    .size(px(36.))
                    .rounded_full()
                    .border_2()
                    .border_color(color("--icon-subtle"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("settings_esc_close", 18., "--icon-subtle"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.server_settings = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--icon-subtle"))
                    .child("ESC"),
            );
        let notice = dirty.then(|| {
            div()
                .absolute()
                .left_0()
                .bottom_0()
                .w(px(740.))
                .px(px(20.))
                .pb(px(20.))
                .child(
                    div()
                        .h(px(54.))
                        .pl(px(16.))
                        .pr(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-highest"))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(gpui::white())
                                .child("Careful \u{2014} you have unsaved changes!"),
                        )
                        .child(
                            div()
                                .id("server-settings-reset")
                                .w(px(60.))
                                .h(px(32.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-brand"))
                                .child("Reset")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.reset_server_settings(cx)),
                                ),
                        )
                        .child(
                            div()
                                .id("server-settings-save")
                                .ml(px(10.))
                                .h(px(32.))
                                .px(px(11.))
                                .rounded(px(8.))
                                .bg(color("--control-connected-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(gpui::white())
                                .child("Save Changes")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.save_server_settings(cx)),
                                ),
                        ),
                )
        });
        Some(
            deferred(
                div()
                    .parity("server-settings")
                    .id("server-settings")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .child(
                        div()
                            .id("server-settings-sidebar")
                            .w(px(372.))
                            .h_full()
                            .flex_none()
                            .pr(px(8.))
                            .overflow_y_scroll()
                            .flex()
                            .justify_end()
                            // Not stretched, or the list could never be taller than the window and scroll.
                            .items_start()
                            .child(nav),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h_full()
                            .bg(color("--background-base-low"))
                            .child(
                                div()
                                    .id("server-settings-scroller")
                                    .mt(px(32.))
                                    .h_full()
                                    .overflow_y_scroll()
                                    .flex()
                                    // Stretched, the page would be exactly as tall as the scroller and never scroll.
                                    .items_start()
                                    .child(
                                        div()
                                            .pt(px(60.))
                                            .px(px(40.))
                                            .pb(px(100.))
                                            .flex()
                                            .gap(px(40.))
                                            // `contentColumn_jnk11m`: 660 inside its padding, except beside the card.
                                            .child(if page == Page::Profile {
                                                content
                                            } else {
                                                div()
                                                    .w(px(660.))
                                                    .flex_none()
                                                    .child(content)
                                                    .into_any_element()
                                            })
                                            // The preview's column; the card itself stays put (below).
                                            .when(preview.is_some(), |this| {
                                                this.child(div().w(px(300.)).flex_none())
                                            }),
                                    ),
                            )
                            // The preview card and ESC hold their place while the page scrolls, as in the capture.
                            .children(preview.map(|preview| {
                                div().absolute().left(px(495.)).top(px(92.)).child(preview)
                            }))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(close_left))
                                    .top(px(close_top))
                                    .child(close),
                            )
                            .children(notice),
                    )
                    .child(div().absolute().top_0().right_0().child(window_controls())),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// "Delete '{name}'" (`design/dom/modals/delete-server-darker-cozy.json`): the server's name typed back under
    /// "Enter server name", then `DELETE /guilds/{id}`; a wrong name says `c2/DS/`.
    pub(super) fn delete_server_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let (field, _) = open.delete_name.as_ref()?;
        let guild = open.guild;
        let name = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)?
            .name
            .clone();
        let wrong = open.delete_wrong;
        let body = div()
            .flex()
            .flex_col()
            .child(Self::field_label("Enter server name"))
            .child(
                div()
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if wrong {
                        color("--text-feedback-critical")
                    } else {
                        color("--input-border-default")
                    })
                    .bg(color("--input-background-default"))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .child(div().w_full().child(field.clone())),
            )
            .when(wrong, |this| {
                this.child(
                    div()
                        .mt(px(8.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-feedback-critical"))
                        .child("You didn't enter the server name correctly"),
                )
            })
            .into_any_element();
        let typed_name = name.clone();
        Some(super::modal::render(
            super::modal::Spec {
                id: "delete-server-modal",
                title: format!("Delete '{name}'").into(),
                subtitle: Some(
                    div()
                        .flex()
                        .flex_wrap()
                        .child("Are you sure you want to delete\u{a0}")
                        .child(div().font_weight(FontWeight(600.)).child(name.clone()))
                        .child("? This action cannot be undone.")
                        .into_any_element(),
                ),
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "delete-server-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        |this, _, _| {
                            if let Some(open) = this.server_settings.as_mut() {
                                open.delete_name = None;
                                open.delete_wrong = false;
                            }
                        },
                    ),
                    super::modal::button(
                        "delete-server-confirm",
                        "Delete Server",
                        super::modal::Tone::Critical,
                        move |this, _, cx| {
                            let Some(open) = this.server_settings.as_mut() else {
                                return;
                            };
                            let typed = open
                                .delete_name
                                .as_ref()
                                .map(|(field, _)| field.read(cx).text().to_owned())
                                .unwrap_or_default();
                            if typed != typed_name {
                                open.delete_wrong = true;
                                return;
                            }
                            let _ = this.requests.send(Request::Rest {
                                key: None,
                                method: reqwest::Method::DELETE,
                                route: format!("/guilds/{guild}"),
                                body: None,
                            });
                            this.server_settings = None;
                            this.select_home(None);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| {
                    if let Some(open) = this.server_settings.as_mut() {
                        open.delete_name = None;
                        open.delete_wrong = false;
                    }
                }),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_round_trips_as_module_9994_maps_it() {
        let body = serde_json::json!({
            "name": "gravecord fixture",
            "description": "",
            "icon_hash": null,
            "custom_banner_hash": null,
            "brand_color_primary": "",
            "visibility": 2,
            "traits": [{"label": "chill", "position": 3, "emoji_name": "fire", "emoji_id": null}],
            "game_application_ids": ["363445589247131668"],
        });
        let values = Values::from_profile(&body);
        // An empty brand color reads as none (`wr`), and Private Profile is on for RESTRICTED.
        assert_eq!(values.color, None);
        assert!(is_private(values.visibility));
        assert!(is_private(0));
        assert!(!is_private(VISIBILITY_PUBLIC_WITH_RECRUITMENT));
        assert_eq!(values.traits[3].0, "chill");
        let sent = values.body();
        // `ow` sends only labelled traits, with their position, and every other field as held.
        assert_eq!(
            sent["traits"],
            serde_json::json!([{"label": "chill", "position": 3, "emoji_id": null, "emoji_name": "fire", "emoji_animated": false}])
        );
        assert_eq!(sent["brand_color_primary"], serde_json::Value::Null);
        assert_eq!(sent["visibility"], 2);
        assert_eq!(
            sent["game_application_ids"],
            serde_json::json!(["363445589247131668"])
        );
    }

    #[test]
    fn presets_parse() {
        assert_eq!(hex_rgb("#029FFC"), Some([2, 159, 252]));
        assert!(BANNER_PRESETS.iter().all(|(_, hex)| hex_rgb(hex).is_some()));
    }
}
