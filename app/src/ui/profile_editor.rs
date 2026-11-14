//! The profile editor ("Edit Profiles" in User Settings), module 927071's editor shell: a 1184 wide full-height
//! layer with the profile styles panel (242 wide) on the left, the live profile preview in the middle and the
//! widgets column on the right. Measured in `design/dom/settings/profile-editor-darker-cozy.json` and
//! `profile-editor-preview-nitro-darker-cozy.json`.
//!
//! Every change is held as a pending change of the open profile (`save.rs`), shown at once in the preview, and
//! written when the save bar's button is pressed (module 631670, 159001 and 207803 build the requests).

mod activity;
mod colour;
mod geometry;
mod panel;
mod picker;
mod preview;
pub(super) mod save;
mod servers;
mod styles;
mod upload;
pub(super) mod widgets;
mod wishlist;

use super::motion::Posed as _;
use super::shop::data::{self as shop_data, Product};
use super::{Root, SANS, color, kerning};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window,
    deferred, div, hsla, prelude::*, px,
};
use save::{Equipped, Pending, UserWidget, Write};
use serde_json::Value;

pub(super) const FETCH_PREFIX: &str = "pedit:";
/// `DismissibleContent.USER_PROFILE_PREMIUM_AND_SHOP_ENTRY_POINTS` (module 554146).
const UPSELL_CONTENT: u16 = 458;
/// `DismissibleContent.DISPLAY_NAME_STYLES_FLYWHEEL_NEW_BADGE_PROFILE_PAGE` (module 554146).
pub(super) const NAME_STYLES_NEW_CONTENT: u16 = 774;

/// The left panel shows the profile's own styles, or the Nitro preview ("Preview Nitro").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Main,
    Nitro,
}

/// The tabs of the widgets column (`tabBarItem_y4ZEM5`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Board,
    Activity,
    Wishlist,
}

/// A collectible kind a picker edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Nameplate,
    Decoration,
    Effect,
    Frame,
}

impl Kind {
    /// `CollectiblesItemType` of module 575593 as the shop data reads it.
    pub(super) fn item_kind(self) -> i64 {
        match self {
            Self::Decoration => shop_data::KIND_AVATAR_DECORATION,
            Self::Effect => shop_data::KIND_PROFILE_EFFECT,
            Self::Nameplate => shop_data::KIND_NAMEPLATE,
            Self::Frame => shop_data::KIND_PROFILE_FRAME,
        }
    }
}

/// What the profile has on it now, read from the account's user and its profile payload.
#[derive(Clone, Debug, Default)]
pub(super) struct Saved {
    pub bio: String,
    pub pronouns: String,
    pub banner: Option<String>,
    pub accent_color: Option<u32>,
    pub theme_colors: Option<[u32; 2]>,
    pub equipped: Equipped,
    pub widgets: Vec<UserWidget>,
}

pub(super) struct Open {
    /// `None` edits the main profile; `Some` a server profile.
    guild: Option<Id>,
    pending: Pending,
    saved: Saved,
    mode: Mode,
    tab: Tab,
    picker: Option<picker::State>,
    banner_color: Option<colour::State>,
    styles: Option<styles::State>,
    /// The Nitro preview's choices (never saved): name style, theme colours, banner preset.
    trying: styles::Trying,
    name: Entity<Editor>,
    pronouns: Entity<Editor>,
    bio: Entity<Editor>,
    select_open: bool,
    tag_select_open: bool,
    tag_search: Entity<Editor>,
    board_widgets: Vec<UserWidget>,
    widget_modal: Option<widgets::ModalState>,
    game_picker: Option<widgets::GamePickerState>,
    widget_comment_editors: std::collections::HashMap<String, Entity<Editor>>,
    widget_confirm_remove: Option<usize>,
    panel_collapsed: bool,
    upsell_closed: bool,
    /// The status bubble's prompt, drawn once per open from the prompt list (module 645507).
    prompt: &'static str,
    /// The editor's sizes for the window it is drawn in (`geometry.rs`), updated every frame.
    geometry: geometry::Geometry,
    /// Harness only: the widget suggestions' `maxWidgetOptions` in place of the account's experiment.
    #[cfg(feature = "capture")]
    suggestions: Option<usize>,
    saving: bool,
    error: Option<String>,
    owned: Vec<Product>,
    offers: Vec<Product>,
    /// The pictures chosen for the avatar and the banner, decoded for the preview.
    avatar_image: Option<std::sync::Arc<gpui::Image>>,
    /// The other profiles' pending changes while one is open (`getPendingChanges(guildId)`).
    stashed: std::collections::HashMap<Option<Id>, Pending>,
    banner_image: Option<std::sync::Arc<gpui::Image>>,
    pub(super) _events: Vec<Subscription>,
}

impl Open {
    pub(super) fn has_changes(&self) -> bool {
        !self.pending.is_empty()
    }
}

/// `GET /users/{id}/profile` as the editor (`u7` of module 927071) asks it: `type: "modal"`, `withMutualGuilds`,
/// `withMutualFriendsCount` and not `withMutualFriends`, in the query order of `fetchProfile` (module 803306).
fn profile_route(user: Id, guild: Option<Id>) -> String {
    let mut route = format!(
        "/users/{user}/profile?type=modal&with_mutual_guilds=true&with_mutual_friends=false&with_mutual_friends_count=true"
    );
    if let Some(guild) = guild {
        route.push_str(&format!("&guild_id={guild}"));
    }
    route
}

/// `GET /collectibles-purchases` with `variants_return_style` set to `VARIANTS_GROUP` (module 110676).
pub(super) const PURCHASES_ROUTE: &str =
    "/users/@me/collectibles-purchases?variants_return_style=2";
/// `GET /collectibles-categories/v2` as `en` of module 110676 asks it.
pub(super) const CATEGORIES_ROUTE: &str =
    "/collectibles-categories/v2?variants_return_style=2&include_bundles=true";

pub(super) fn profile_field_style(
    size: f32,
    lh: f32,
    weight: f32,
    text: gpui::Hsla,
    placeholder: gpui::Hsla,
) -> crate::editor::EditorStyle {
    let mut font = super::editor_style().font;
    font.weight = FontWeight(weight);
    crate::editor::EditorStyle {
        font,
        size,
        line_height: lh,
        text,
        placeholder,
        placeholder_italic: true,
        caret: text,
        ..super::editor_style()
    }
}

impl Root {
    pub(super) fn open_profile_editor(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.stores.current_user.clone() else {
            return;
        };
        let user_id = me.id;
        let field = |size: f32, lh: f32, weight: f32, placeholder: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let style = profile_field_style(
                    size,
                    lh,
                    weight,
                    hsla(0., 0., 1., 0.96),
                    color("--text-muted").into(),
                );
                let mut editor = Editor::new(style, cx);
                editor.set_single_line(true);
                editor.set_placeholder(placeholder.to_owned());
                editor
            })
        };
        let name = field(24., 32., 600., dc_intl::tr("Display name"), cx);
        let pronouns = field(14., 18., 400., dc_intl::tr("Add pronouns"), cx);
        let bio = field(
            14.,
            18.,
            400.,
            dc_intl::tr("Share a fun fact about yourself"),
            cx,
        );
        let tag_search = cx.new(|cx| {
            let style = profile_field_style(
                14.,
                18.,
                400.,
                color("--text-default").into(),
                color("--input-placeholder-text-default").into(),
            );
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
            editor.set_placeholder(dc_intl::t!("5h0QOP").to_owned());
            editor
        });
        name.update(cx, |editor, _| editor.set_text(me.display_name()));
        let events = vec![
            cx.subscribe(&name, |this, editor, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Edited) {
                    let text = editor.read(cx).text().to_owned();
                    this.profile_editor_set_name(text);
                }
                cx.notify();
            }),
            cx.subscribe(&pronouns, |this, editor, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Edited) {
                    let text = editor.read(cx).text().to_owned();
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.pending.pronouns = (text != open.saved.pronouns).then_some(text);
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&bio, |this, editor, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Edited) {
                    let text = editor.read(cx).text().to_owned();
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.pending.bio = (text != open.saved.bio).then_some(text);
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&tag_search, |_, _, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Edited) {
                    cx.notify();
                }
            }),
        ];
        let guild = None;
        self.profile_editor = Some(Open {
            guild,
            pending: Pending::new(guild),
            saved: Saved::default(),
            mode: Mode::Main,
            tab: Tab::Board,
            picker: None,
            banner_color: None,
            styles: None,
            trying: styles::Trying::default(),
            name,
            pronouns,
            bio,
            select_open: false,
            tag_select_open: false,
            tag_search,
            board_widgets: Vec::new(),
            widget_modal: None,
            game_picker: None,
            widget_comment_editors: std::collections::HashMap::new(),
            widget_confirm_remove: None,
            panel_collapsed: false,
            upsell_closed: false,
            prompt: preview::random_prompt(),
            geometry: geometry::Geometry::of(1280., 720.),
            #[cfg(feature = "capture")]
            suggestions: None,
            saving: false,
            error: None,
            owned: Vec::new(),
            offers: Vec::new(),
            avatar_image: None,
            stashed: std::collections::HashMap::new(),
            banner_image: None,
            _events: events,
        });
        if let Some(cached) = self.fetched.get(&format!("profile:{user_id}")).cloned() {
            self.profile_editor_apply_profile(&cached, true, cx);
        }
        for (key, route) in [
            (
                format!("{FETCH_PREFIX}profile"),
                profile_route(user_id, guild),
            ),
            (
                format!("{FETCH_PREFIX}purchases"),
                PURCHASES_ROUTE.to_owned(),
            ),
            (
                format!("{FETCH_PREFIX}categories"),
                CATEGORIES_ROUTE.to_owned(),
            ),
            (
                format!("{FETCH_PREFIX}outbox"),
                activity::outbox_route(user_id),
            ),
            (
                format!("{FETCH_PREFIX}wishlistprofile"),
                shop_data::own_profile_route(user_id),
            ),
            (
                format!("{FETCH_PREFIX}gifts"),
                wishlist::gifts_route(user_id),
            ),
        ] {
            let _ = self.requests.send(Request::Fetch { key, route });
        }
        cx.notify();
    }

    pub(super) fn close_profile_editor(&mut self, cx: &mut Context<Self>) {
        self.profile_editor = None;
        cx.notify();
    }

    /// Escape closes the innermost thing: a picker, the select, the Nitro preview, then the editor.
    pub(super) fn profile_editor_escape(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.profile_editor.as_mut() else {
            return false;
        };
        if open.widget_confirm_remove.is_some() {
            open.widget_confirm_remove = None;
        } else if open.game_picker.is_some() {
            open.game_picker = None;
        } else if open.widget_modal.is_some() {
            open.widget_modal = None;
        } else if open.picker.is_some() {
            open.picker = None;
        } else if open.banner_color.is_some() {
            open.banner_color = None;
        } else if open.styles.is_some() {
            open.styles = None;
        } else if open.tag_select_open {
            open.tag_select_open = false;
        } else if open.select_open {
            open.select_open = false;
        } else if open.mode == Mode::Nitro {
            open.mode = Mode::Main;
        } else {
            self.profile_editor = None;
        }
        cx.notify();
        true
    }

    fn profile_editor_set_name(&mut self, text: String) {
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        let current = self
            .stores
            .current_user
            .as_ref()
            .map(|user| user.display_name().to_owned())
            .unwrap_or_default();
        let value = (!text.is_empty() && text != current).then_some(text);
        match open.guild {
            None => open.pending.global_name = value.map(Some),
            Some(_) => open.pending.nickname = value.map(Some),
        }
    }

    /// The answer to a request of this editor.
    pub(super) fn profile_editor_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(body) = self.fetched.get(key).cloned() else {
            return;
        };
        let what = key.strip_prefix(FETCH_PREFIX).unwrap_or_default();
        match what {
            "outbox" | "apps" => self.profile_editor_activity_fetched(what),
            "wishlistprofile" | "wishlist" | "gifts" => self.profile_editor_wishlist_fetched(what),
            _ => {}
        }
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        match what {
            "profile" => {
                if body["refused"] == true {
                    return;
                }
                self.profile_editor_apply_profile(&body, false, cx);
                return;
            }
            "purchases" => {
                open.owned = body
                    .as_array()
                    .map(|list| list.iter().filter_map(shop_data::parse_product).collect())
                    .unwrap_or_default();
            }
            "categories" => {
                open.offers = body["categories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|category| shop_data::parse_category(category).products)
                    .collect();
            }
            "save" => {
                let ok = body["ok"] == true || body["refused"] == true;
                open.saving = false;
                if ok {
                    self.profile_editor_commit_save(&body["body"], cx);
                } else {
                    open.error = Some(dc_intl::tr("Something went wrong. Try again.").to_owned());
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn profile_editor_apply_profile(
        &mut self,
        body: &Value,
        initial: bool,
        cx: &mut Context<Self>,
    ) {
        if body["refused"] == true || body["ok"] == false {
            return;
        }
        let Some(guild) = self.profile_editor.as_ref().map(|open| open.guild) else {
            return;
        };
        // `USER_PROFILE_FETCH_SUCCESS` fills the one `UserProfileStore` every surface reads: the badges, the
        // effect and the theme of the main profile are the account's own profile there too.
        if !initial
            && guild.is_none()
            && let Some(me) = self.stores.current_user.as_ref().map(|user| user.id)
        {
            self.fetched.insert(format!("profile:{me}"), body.clone());
            self.request_profile_images(me);
        }
        let main_widgets = guild
            .and_then(|_| self.stores.current_user.as_ref().map(|u| u.id))
            .and_then(|me| self.fetched.get(&format!("profile:{me}")))
            .filter(|main| main["widgets"].is_array())
            .map(save::parse_widgets);
        let parsed_widgets = if body["widgets"].is_array() {
            Some(save::parse_widgets(body))
        } else {
            main_widgets
        };
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        let color = |value: &Value| value.as_u64().map(|number| number as u32);
        let for_guild = guild.is_some_and(|g| {
            body["guild_member_profile"]["guild_id"].as_str() == Some(g.to_string().as_str())
        });
        let profile = if for_guild {
            &body["guild_member_profile"]
        } else {
            &body["user_profile"]
        };
        open.saved.bio = profile["bio"].as_str().unwrap_or_default().to_owned();
        open.saved.pronouns = profile["pronouns"].as_str().unwrap_or_default().to_owned();
        open.saved.banner = profile["banner"].as_str().map(str::to_owned);
        open.saved.accent_color = color(&profile["accent_color"]).or_else(|| {
            guild
                .is_none()
                .then(|| color(&body["user"]["accent_color"]))
                .flatten()
        });
        open.saved.theme_colors = profile["theme_colors"]
            .as_array()
            .and_then(|list| Some([color(list.first()?)?, color(list.get(1)?)?]));
        for entry in profile["collectibles"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(
                guild
                    .is_none()
                    .then(|| body["collectibles"].as_array())
                    .flatten()
                    .into_iter()
                    .flatten(),
            )
        {
            let sku = entry["sku_id"].as_str().and_then(|sku| sku.parse().ok());
            match entry["type"].as_i64() {
                Some(1) => open.saved.equipped.profile_effect = sku,
                Some(3) => open.saved.equipped.profile_frame = sku,
                _ => {}
            }
        }
        if let Some(widgets) = parsed_widgets {
            if open.pending.widgets.is_none() {
                open.board_widgets = widgets.clone();
            }
            open.saved.widgets = widgets;
        }
        let bio = open
            .pending
            .bio
            .as_deref()
            .unwrap_or(&open.saved.bio)
            .to_owned();
        let pronouns = open
            .pending
            .pronouns
            .as_deref()
            .unwrap_or(&open.saved.pronouns)
            .to_owned();
        let game_ids = widgets::widget_game_ids(&open.board_widgets);
        let (bio_field, pronoun_field) = (open.bio.clone(), open.pronouns.clone());
        bio_field.update(cx, |editor, _| editor.set_text(&bio));
        pronoun_field.update(cx, |editor, _| editor.set_text(&pronouns));
        if !game_ids.is_empty() {
            self.request_games(&game_ids);
        }
        cx.notify();
    }

    fn profile_editor_commit_save(&mut self, response_body: &Value, _cx: &mut Context<Self>) {
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        let guild = open.guild;
        let pending = std::mem::replace(&mut open.pending, Pending::new(guild));
        if let Some(bio) = &pending.bio {
            open.saved.bio = bio.clone();
        }
        if let Some(pronouns) = &pending.pronouns {
            open.saved.pronouns = pronouns.clone();
        }
        if let Some(banner) = &pending.banner
            && banner.is_none()
        {
            open.saved.banner = None;
        }
        if let Some(accent) = pending.accent_color {
            open.saved.accent_color = accent;
        }
        if let Some(theme) = pending.theme_colors {
            open.saved.theme_colors = theme;
        }
        if let Some(effect) = pending.profile_effect {
            open.saved.equipped.profile_effect = effect;
        }
        if let Some(frame) = pending.profile_frame {
            open.saved.equipped.profile_frame = frame;
        }
        if pending.widgets.is_some() {
            let server_widgets = response_body["widgets"]
                .is_array()
                .then(|| save::parse_widgets(response_body))
                .filter(|list| !list.is_empty());
            let committed =
                server_widgets.unwrap_or_else(|| save::saveable_widgets(&open.board_widgets));
            open.saved.widgets = committed.clone();
            open.board_widgets = committed;
        }
        open.saving = false;
        open.error = None;
        open.widget_confirm_remove = None;
        open.game_picker = None;
        let saved_snapshot = open.saved.clone();
        let saved_widgets = open.saved.widgets.clone();

        let new_tag = pending.primary_guild_id.map(|choice| {
            choice.and_then(|guild_id| {
                let guild = self.stores.guilds.iter().find(|g| g.id == guild_id)?;
                let (tag, badge) = guild.selectable_server_tag()?;
                Some(dc_state::ServerTag {
                    guild_id,
                    badge: badge.map(str::to_owned),
                    tag: tag.to_owned(),
                })
            })
        });

        let Some(me_id) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return;
        };
        if let Some(user) = self.stores.current_user.as_mut() {
            if guild.is_none()
                && let Some(name) = &pending.global_name
            {
                user.global_name = name.clone();
            }
            if guild.is_none()
                && let Some(decoration) = pending.avatar_decoration
            {
                user.avatar_decoration = decoration.map(|sku_id| {
                    Box::new(dc_state::collectibles::AvatarDecoration {
                        sku_id,
                        asset: None,
                        expires_at: None,
                    })
                });
            }
            if guild.is_none()
                && let Some(plate) = pending.nameplate
            {
                user.nameplate = plate.map(|sku_id| dc_state::collectibles::Nameplate {
                    sku_id,
                    label: String::new(),
                    palette: String::new(),
                });
            }
            if guild.is_none()
                && let Some(styles) = &pending.display_name_styles
            {
                user.display_name_styles = styles.clone().map(Box::new);
            }
            if let Some(tag) = &new_tag {
                user.tag = tag.clone();
            }
            self.stores.users.insert(me_id, user.clone());
        }
        if let (Some(guild_id), Some(nick)) = (guild, &pending.nickname)
            && let Some(mut member) = self.stores.members.get(guild_id, me_id).cloned()
        {
            member.nick = nick.clone();
            self.stores.members.upsert(guild_id, member);
        }

        for key in [format!("profile:{me_id}"), format!("{FETCH_PREFIX}profile")] {
            let entry = self
                .fetched
                .entry(key)
                .or_insert_with(|| serde_json::json!({}));
            apply_saved_to_profile_json(entry, guild, &pending, &saved_snapshot, &new_tag);
        }

        if pending.widgets.is_some() {
            let game_ids = widgets::widget_game_ids(&saved_widgets);
            if !game_ids.is_empty() {
                let games_key = format!("profile-games:{me_id}");
                let entries: Vec<Value> = game_ids
                    .iter()
                    .filter_map(|id| self.profile_editor_known_game_value(id))
                    .collect();
                if !entries.is_empty() {
                    let slot = self
                        .fetched
                        .entry(games_key.clone())
                        .or_insert_with(|| Value::Array(Vec::new()));
                    if !slot.is_array() {
                        *slot = Value::Array(Vec::new());
                    }
                    if let Some(arr) = slot.as_array_mut() {
                        for record in entries {
                            let rid = record["id"].as_str().unwrap_or_default();
                            if !arr
                                .iter()
                                .any(|existing| existing["id"].as_str() == Some(rid))
                            {
                                arr.push(record);
                            }
                        }
                    }
                }
                self.requested.remove(&games_key);
                self.request_games(&game_ids);
            }
            self.request_profile_cards(me_id);
        }
        if pending.primary_guild_id.is_some() {
            self.request_profile_images(me_id);
        }
    }

    /// `true` when the editor opened a premium-gated choice without Nitro (the official shows its upsell).
    pub(super) fn profile_editor_premium(&self) -> bool {
        self.premium() == dc_state::premium::PremiumType::Nitro
    }

    /// Sends every request the pending changes need (`USER_PROFILE_SETTINGS_SUBMIT`). A change that needs Nitro
    /// opens the Nitro page instead for an account without it.
    pub(super) fn profile_editor_save(&mut self, cx: &mut Context<Self>) {
        let premium = self.profile_editor_premium();
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        if open.saving || open.pending.is_empty() {
            return;
        }
        if open.pending.needs_premium() && !premium {
            self.open_nitro(cx);
            return;
        }
        let writes: Vec<Write> = open.pending.writes(&open.saved.equipped);
        open.saving = true;
        open.error = None;
        let last = writes.len().saturating_sub(1);
        for (index, write) in writes.into_iter().enumerate() {
            let _ = self.requests.send(Request::Rest {
                key: (index == last).then(|| format!("{FETCH_PREFIX}save")),
                method: write.method,
                route: write.route,
                body: Some(write.body),
            });
        }
        cx.notify();
    }

    /// The account's user with the pending choices applied: what the preview, the message row and the member row
    /// draw.
    pub(super) fn profile_editor_user(&self) -> User {
        let mut user = self.stores.current_user.clone().unwrap_or_default();
        let Some(open) = self.profile_editor.as_ref() else {
            return user;
        };
        if let Some(Some(name)) = &open.pending.global_name {
            user.global_name = Some(name.clone());
        }
        if let Some(decoration) = &open.pending.avatar_decoration {
            user.avatar_decoration = decoration.map(|sku_id| {
                Box::new(dc_state::collectibles::AvatarDecoration {
                    sku_id,
                    asset: None,
                    expires_at: None,
                })
            });
        }
        if let Some(plate) = &open.pending.nameplate {
            user.nameplate = plate.map(|sku_id| dc_state::collectibles::Nameplate {
                sku_id,
                label: String::new(),
                palette: String::new(),
            });
        }
        if let Some(styles) = open
            .trying
            .styles
            .as_ref()
            .filter(|_| open.mode == Mode::Nitro)
        {
            user.display_name_styles = Some(Box::new(styles.clone()));
        } else if let Some(styles) = &open.pending.display_name_styles {
            user.display_name_styles = styles.clone().map(Box::new);
        }
        if let Some(choice) = open.pending.primary_guild_id {
            user.tag = choice.and_then(|guild_id| {
                let guild = self.stores.guilds.iter().find(|g| g.id == guild_id)?;
                let (tag, badge) = guild.selectable_server_tag()?;
                Some(dc_state::ServerTag {
                    guild_id,
                    badge: badge.map(str::to_owned),
                    tag: tag.to_owned(),
                })
            });
        }
        user
    }

    pub(super) fn profile_editor_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewport = window.viewport_size();
        let geometry =
            geometry::Geometry::of(f32::from(viewport.width), f32::from(viewport.height));
        self.profile_editor.as_mut()?.geometry = geometry;
        let user = self.profile_editor_user();
        self.profile_editor_request_art(&user);
        // `modalHeaderButtons_ZhNJfE`: 8px from the top (below the title bar under 785px) and the right.
        let close = div()
            .id("profile-editor-close")
            .absolute()
            .top(px(geometry.close_top()))
            .right(px(8.))
            .child(
                div()
                    .id("profile-editor-close-button")
                    .size(px(32.))
                    .rounded(px(8.))
                    .bg(color("--control-secondary-background-default"))
                    .hover(|style| style.bg(color("--control-secondary-background-hover")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(super::icon(
                        "modal_close",
                        16.,
                        "--control-secondary-text-default",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| this.close_profile_editor(cx))),
            );
        // `UserProfileModalV2` (module 927071) is on the legacy modal root (`root__KoJTP` in the capture, `EO` in
        // module 935462): the scrim fades in on `hP`, the editor from opacity 0 and `scale(.7)` over 200 ms. Its box
        // follows the window (`geometry.rs`).
        let pose =
            super::motion::legacy_modal_pose("profile-editor", "profile-editor-layer", false);
        let panel_collapsed = self
            .profile_editor
            .as_ref()
            .is_some_and(|open| geometry.panel_over && open.panel_collapsed);
        let panel = if panel_collapsed {
            None
        } else {
            let panel = self.profile_editor_panel(&user, window, cx);
            Some(if geometry.panel_over {
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .h_full()
                    .child(panel)
                    .into_any_element()
            } else {
                panel
            })
        };
        let expand = panel_collapsed.then(|| {
            div()
                .id("profile-editor-expand")
                .absolute()
                .top(px(geometry.close_top()))
                .right(px(48.))
                .size(px(32.))
                .rounded(px(8.))
                .backdrop_blur(px(4.))
                .bg(color("--control-secondary-background-default"))
                .hover(|style| style.bg(color("--control-secondary-background-hover")))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(super::icon(
                    "profile_editor_collapse",
                    16.,
                    "--control-secondary-text-default",
                ))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.panel_collapsed = false;
                    }
                    cx.notify();
                }))
        });
        let stage = self.profile_editor_stage(&user, window, cx);
        let root = div()
            .parity("profile-editor")
            .id("profile-editor")
            .posed(pose)
            .occlude()
            .absolute()
            .left(px(geometry.left))
            .top(px(geometry.top))
            .w(px(geometry.width))
            .h(px(geometry.height))
            .rounded(px(geometry.radius))
            .overflow_hidden()
            .bg(color("--background-base-lower"))
            .flex()
            .font_family(SANS)
            .font_features(kerning());
        let root = if geometry.panel_over {
            root.child(stage).children(panel.map(panel_slot))
        } else {
            root.children(panel.map(panel_slot)).child(stage)
        };
        let root = root
            .children(expand)
            .child(close)
            .children(self.profile_editor_banner_popout(cx))
            .children(self.profile_editor_server_list(cx))
            .children(self.profile_editor_tag_select_popout(cx))
            .children(self.profile_editor_game_picker_popout(cx))
            .children(self.profile_editor_widget_modal(cx))
            .children(self.profile_editor_remove_widget_modal(cx));
        let layer = deferred(
            div()
                .id("profile-editor-layer")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(pose.dim(color("--background-scrim")))
                .child(root),
        )
        .with_priority(2)
        .into_any_element();
        Some(layer)
    }

    /// `ee` of module 560296: the "Amp up your profile" card shows until `USER_PROFILE_PREMIUM_AND_SHOP_ENTRY_POINTS`
    /// is dismissed in the account's `dismissed_contents` (or closed in this session).
    pub(super) fn profile_editor_upsell_shown(&self) -> bool {
        let Some(open) = self.profile_editor.as_ref() else {
            return false;
        };
        open.mode != Mode::Nitro
            && !open.upsell_closed
            && !self.stores.content_dismissed(UPSELL_CONTENT)
    }

    /// The upsell's close (`Dr(..., {dismissAction: USER_DISMISS})`): the content's bit in `dismissed_contents`, a
    /// settings-proto write that a harness run holds back.
    pub(super) fn profile_editor_dismiss_upsell(&mut self) {
        if let Some(open) = self.profile_editor.as_mut() {
            open.upsell_closed = true;
        }
        self.dismiss_profile_content(UPSELL_CONTENT);
    }

    /// Sets a dismissible content's bit and sends the settings patch (`markAsDismissed`).
    pub(super) fn dismiss_profile_content(&mut self, content: u16) {
        if self.stores.content_dismissed(content) {
            return;
        }
        let patch = self.stores.dismiss_content(content);
        if patch.is_empty() || crate::storage::settings_writes_blocked() {
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me/settings-proto/1".into(),
            body: Some(serde_json::json!({"settings": patch})),
        });
    }

    /// The banner colour popout over the editor, and the click-away layer that closes it.
    fn profile_editor_banner_popout(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let geometry = self.profile_editor.as_ref()?.geometry;
        self.profile_editor.as_ref()?.banner_color.as_ref()?;
        let x = if geometry.panel_over {
            (geometry.width - geometry.panel_width - 280.).max(8.)
        } else {
            234.
        };
        let y = (geometry.system_bar + 332.)
            .min(geometry.height - 260.)
            .max(8.);
        let popout = self.profile_editor_colour_popout(x, y, cx)?;
        Some(
            div()
                .id("profile-editor-colour-away")
                .absolute()
                .inset_0()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.banner_color = None;
                    }
                    cx.notify();
                }))
                .child(popout)
                .into_any_element(),
        )
    }

    /// The saved profile's banner picture, once the account's profile and the picture arrived.
    pub(super) fn profile_editor_saved_banner(&self) -> Option<std::sync::Arc<gpui::Image>> {
        let open = self.profile_editor.as_ref()?;
        if matches!(open.pending.banner, Some(None)) {
            return None;
        }
        let me = self.stores.current_user.as_ref()?.id;
        let guild = open.guild;
        let body = guild
            .and_then(|_| self.fetched.get(&format!("{FETCH_PREFIX}profile")))
            .or_else(|| self.fetched.get(&format!("profile:{me}")))?;
        let (key, _) = super::profile_art::banner_source(me, guild, body)?;
        self.images.get(&key).cloned()
    }

    /// Asks for the art the preview needs: the avatar decoration of the pending choice.
    fn profile_editor_request_art(&mut self, user: &User) {
        if let Some(url) = user.tag.as_ref().and_then(|tag| tag.badge_url()) {
            self.request_image(url.clone(), url);
        }
        if let Some(decoration) = user.avatar_decoration.as_deref() {
            self.request_image(
                format!("avatar-decoration:{}", decoration.sku_id),
                decoration.static_url(),
            );
        }
    }
}

/// Mutate a cached `profile:{me}` / `pedit:profile` payload in-place so reopening Profile Editor or Full Profile reflects saved edits immediately.
fn apply_saved_to_profile_json(
    root: &mut Value,
    guild: Option<Id>,
    pending: &Pending,
    saved: &Saved,
    tag: &Option<Option<dc_state::ServerTag>>,
) {
    if !root.is_object() {
        *root = serde_json::json!({});
    }
    let section_key = if guild.is_some() {
        "guild_member_profile"
    } else {
        "user_profile"
    };
    if !root[section_key].is_object() {
        root[section_key] = serde_json::json!({});
    }
    if let Some(guild_id) = guild {
        root[section_key]["guild_id"] = serde_json::json!(guild_id.to_string());
    }
    if pending.bio.is_some() {
        root[section_key]["bio"] = serde_json::json!(saved.bio);
    }
    if pending.pronouns.is_some() {
        root[section_key]["pronouns"] = serde_json::json!(saved.pronouns);
    }
    if pending.accent_color.is_some() {
        root[section_key]["accent_color"] = serde_json::json!(saved.accent_color);
        if guild.is_none() {
            if !root["user"].is_object() {
                root["user"] = serde_json::json!({});
            }
            root["user"]["accent_color"] = serde_json::json!(saved.accent_color);
        }
    }
    if pending.theme_colors.is_some() {
        root[section_key]["theme_colors"] = match saved.theme_colors {
            Some([a, b]) => serde_json::json!([a, b]),
            None => Value::Null,
        };
    }
    if matches!(pending.banner, Some(None)) {
        root[section_key]["banner"] = Value::Null;
    }
    if guild.is_none()
        && let Some(name) = &pending.global_name
    {
        if !root["user"].is_object() {
            root["user"] = serde_json::json!({});
        }
        root["user"]["global_name"] = serde_json::json!(name);
    }
    if let Some(choice) = tag {
        if !root["user"].is_object() {
            root["user"] = serde_json::json!({});
        }
        root["user"]["primary_guild"] = match choice {
            Some(t) => serde_json::json!({
                "identity_guild_id": t.guild_id.to_string(),
                "identity_enabled": true,
                "tag": t.tag,
                "badge": t.badge,
            }),
            None => serde_json::json!({
                "identity_guild_id": null,
                "identity_enabled": false,
            }),
        };
    }
    if pending.widgets.is_some() {
        root["widgets"] = Value::Array(save::widgets_to_json(&saved.widgets));
    }
}

/// The panel's place in the editor's row.
fn panel_slot(panel: AnyElement) -> gpui::Div {
    div().h_full().flex_none().child(panel)
}

/// A body-sm label above a panel group (`label_Z5MObs`).
pub(super) fn label(text: &'static str) -> gpui::Div {
    div()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
        .child(text)
}

/// A text run of the editor with the sizes of the design system.
pub(super) fn text(
    value: impl Into<SharedString>,
    size: f32,
    line_height: f32,
    weight: f32,
    tint: &str,
) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(line_height))
        .font_weight(FontWeight(weight))
        .text_color(color(tint))
        .child(value.into())
}

#[cfg(feature = "capture")]
impl Root {
    /// Harness step `profile-editor <command>`: drives the editor without a pointer. `open`, `close`, `nitro`,
    /// `main`, `inject <file>`, `suggestions <n>`, `preset <name>`, `pick <nameplate|decoration|effect|frame> [sku]`, `banner-color`, `styles`,
    /// `font <id>`, `effect <id>`, `colors <hex>...`, `custom-color <hex>`. Nothing is sent to the network.
    pub(crate) fn script_profile_editor(
        &mut self,
        rest: &[&str],
        cx: &mut Context<Self>,
    ) -> String {
        match rest {
            ["open"] => self.open_profile_editor(cx),
            ["close"] => self.close_profile_editor(cx),
            ["nitro"] | ["main"] => {
                if let Some(open) = self.profile_editor.as_mut() {
                    if rest[0] == "nitro" {
                        open.mode = Mode::Nitro;
                        open.tab = Tab::Activity;
                        open.trying.start();
                    } else {
                        open.mode = Mode::Main;
                        open.tab = Tab::Board;
                    }
                }
            }
            ["preset", name] => {
                if let Some(index) = styles::PRESETS
                    .iter()
                    .position(|preset| preset.name == *name)
                    && let Some(open) = self.profile_editor.as_mut()
                {
                    open.trying.set_preset(index);
                }
            }
            ["pick", kind, sku @ ..] => {
                let kind = match *kind {
                    "nameplate" => Kind::Nameplate,
                    "decoration" => Kind::Decoration,
                    "effect" => Kind::Effect,
                    _ => Kind::Frame,
                };
                self.profile_editor_script_pick(
                    kind,
                    sku.first().and_then(|sku| sku.parse().ok()),
                    cx,
                );
            }
            ["tab", which] => {
                if let Some(open) = self.profile_editor.as_mut() {
                    open.tab = match *which {
                        "activity" => Tab::Activity,
                        "wishlist" => Tab::Wishlist,
                        _ => Tab::Board,
                    };
                }
            }
            // `inject <file>`: answers the open editor's profile fetch with a JSON payload read from the file, as
            // `GET /users/@me/profile` would (the harness account's own profile is bare).
            ["inject", path] => {
                let body = std::fs::read_to_string(path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok());
                let Some(body) = body else {
                    return format!("profile-editor: cannot read {path}");
                };
                let key = format!("{FETCH_PREFIX}profile");
                self.fetched.insert(key.clone(), body);
                self.profile_editor_fetched(&key, cx);
            }
            // `suggestions <n>`: the widget board as `2026-09-profile-widget-empty-state-suggestions` with
            // `maxWidgetOptions` n (0 restores the account's own assignment).
            ["suggestions", count] => {
                if let Some(open) = self.profile_editor.as_mut() {
                    open.suggestions = count.parse().ok().filter(|count| *count > 0);
                }
            }
            ["banner-color"] => self.profile_editor_script_banner(cx),
            ["styles"] => self.profile_editor_script_styles(cx),
            ["font", id] => self.profile_editor_script_style(|state| {
                state.font = id.parse().unwrap_or(state.font);
            }),
            ["effect", id] => self.profile_editor_script_style(|state| {
                state.effect = id.parse().unwrap_or(state.effect);
                state.colors.clear();
            }),
            // `server <id>` edits that server's profile, `server first` the first server of the rail, `server list`
            // opens the select, `server main` goes back to the main profile.
            ["server", which] => {
                let guild = match *which {
                    "main" => None,
                    "list" => {
                        if let Some(open) = self.profile_editor.as_mut() {
                            open.select_open = true;
                        }
                        self.profile_editor.as_ref().and_then(|open| open.guild)
                    }
                    "first" => self.stores.guilds.first().map(|guild| guild.id),
                    id => id.parse().ok(),
                };
                self.profile_editor_select_guild(guild, cx);
                if *which == "list"
                    && let Some(open) = self.profile_editor.as_mut()
                {
                    open.select_open = true;
                }
            }
            ["light"] => self.profile_editor_script_style(|state| state.light = true),
            ["colors", hex @ ..] => {
                let colors: Vec<u32> = hex
                    .iter()
                    .filter_map(|value| u32::from_str_radix(value.trim_start_matches('#'), 16).ok())
                    .collect();
                self.profile_editor_script_style(|state| state.colors = colors);
            }
            ["tag-open"] => {
                if let Some(open) = self.profile_editor.as_mut()
                    && open.guild.is_none()
                {
                    open.tag_select_open = true;
                }
                self.profile_editor_request_tag_art();
            }
            ["tag-pick", which @ ..] => {
                let choice = match which {
                    ["none"] | ["clear"] => None,
                    ["first"] | [] => self
                        .profile_editor_eligible_tags()
                        .first()
                        .map(|entry| entry.0),
                    [id] => id.parse().ok(),
                    _ => None,
                };
                self.profile_editor_pick_tag(choice, cx);
            }
            ["browse-widgets"] | ["widget-modal"] => {
                self.profile_editor_open_widget_modal(cx);
            }
            ["widget-modal-tab", which] => {
                if let Some(modal) = self
                    .profile_editor
                    .as_mut()
                    .and_then(|open| open.widget_modal.as_mut())
                {
                    modal.tab = if *which == "stats" {
                        widgets::ModalTab::GameStats
                    } else {
                        widgets::ModalTab::Interests
                    };
                }
            }
            ["widget-modal-search", query @ ..] => {
                if let Some(search) = self
                    .profile_editor
                    .as_ref()
                    .and_then(|open| open.widget_modal.as_ref())
                    .map(|modal| modal.search.clone())
                {
                    let text = query.join(" ");
                    search.update(cx, |editor, _| editor.set_text(&text));
                }
            }
            ["add-widget", kind] => {
                if let Some(data) = widgets::widget_data_from_script(kind) {
                    self.profile_editor_prepend_widget(data, cx);
                } else {
                    return format!("profile-editor: unknown widget kind {kind}");
                }
            }
            ["game-picker", index] => {
                if let Ok(idx) = index.parse::<usize>() {
                    self.profile_editor_open_game_picker(idx, cx);
                }
            }
            ["add-game", index, game_id] => {
                if let Ok(idx) = index.parse::<usize>() {
                    self.profile_editor_add_game_to_widget(idx, (*game_id).to_owned(), cx);
                }
            }
            ["remove-widget", index] => {
                if let Ok(idx) = index.parse::<usize>() {
                    self.profile_editor_remove_widget_now(idx, cx);
                }
            }
            ["save"] => self.profile_editor_save(cx),
            ["reset"] => self.profile_editor_reset(cx),
            ["dismiss-upsell"] => self.profile_editor_dismiss_upsell(),
            _ => return format!("profile-editor: unknown command {rest:?}"),
        }
        cx.notify();
        format!("profile-editor: {}", rest.join(" "))
    }

    fn profile_editor_script_pick(&mut self, kind: Kind, sku: Option<u64>, cx: &mut Context<Self>) {
        self.profile_editor_pick(kind, cx);
        if let Some(picker) = self
            .profile_editor
            .as_mut()
            .and_then(|open| open.picker.as_mut())
            && sku.is_some()
        {
            picker.choice = sku;
        }
    }

    fn profile_editor_script_banner(&mut self, cx: &mut Context<Self>) {
        self.profile_editor_open_banner_color(cx);
    }

    fn profile_editor_script_styles(&mut self, cx: &mut Context<Self>) {
        self.profile_editor_open_styles(cx);
    }

    fn profile_editor_script_style(&mut self, change: impl FnOnce(&mut styles::State)) {
        if let Some(state) = self
            .profile_editor
            .as_mut()
            .and_then(|open| open.styles.as_mut())
        {
            change(state);
            state.refit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::profile_route;

    /// The editor asks for its profile as `u7` of module 927071 does: the modal type with mutual guilds and the mutual
    /// friends' count.
    #[test]
    fn the_editor_fetches_the_modal_profile() {
        assert_eq!(
            profile_route(7, None),
            "/users/7/profile?type=modal&with_mutual_guilds=true&with_mutual_friends=false&with_mutual_friends_count=true"
        );
        assert!(profile_route(7, Some(9)).ends_with("&guild_id=9"));
    }
}
