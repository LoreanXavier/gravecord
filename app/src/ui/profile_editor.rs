//! The profile editor ("Edit Profiles" in User Settings), module 927071's editor shell: a 1184 wide full-height
//! layer with the profile styles panel (242 wide) on the left, the live profile preview in the middle and the
//! widgets column on the right. Measured in `design/dom/settings/profile-editor-darker-cozy.json` and
//! `profile-editor-preview-nitro-darker-cozy.json`.
//!
//! Every change is held as a pending change of the open profile (`save.rs`), shown at once in the preview, and
//! written when the save bar's button is pressed (module 631670, 159001 and 207803 build the requests).

mod colour;
mod panel;
mod picker;
mod preview;
pub(super) mod save;
mod styles;
mod widgets;

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
use save::{Equipped, Pending, Write};
use serde_json::Value;

pub(super) const FETCH_PREFIX: &str = "pedit:";

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
    upsell_closed: bool,
    saving: bool,
    error: Option<String>,
    owned: Vec<Product>,
    offers: Vec<Product>,
    _events: Vec<Subscription>,
}

impl Open {
    pub(super) fn has_changes(&self) -> bool {
        !self.pending.is_empty()
    }
}

/// `GET /users/{id}/profile` for the editor, as the profile fetch of the full profile asks it.
fn profile_route(user: Id, guild: Option<Id>) -> String {
    let mut route = format!(
        "/users/{user}/profile?type=popout&with_mutual_guilds=false&with_mutual_friends=false&with_mutual_friends_count=false"
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

impl Root {
    pub(super) fn open_profile_editor(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.stores.current_user.clone() else {
            return;
        };
        let user_id = me.id;
        let field = |size: f32, lh: f32, weight: f32, placeholder: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut font = super::editor_style().font;
                font.weight = FontWeight(weight);
                let style = crate::editor::EditorStyle {
                    font,
                    size,
                    line_height: lh,
                    text: hsla(0., 0., 1., 0.96),
                    placeholder: color("--text-muted").into(),
                    placeholder_italic: true,
                    caret: hsla(0., 0., 1., 0.96),
                    ..super::editor_style()
                };
                let mut editor = Editor::new(style, cx);
                editor.set_single_line(true);
                editor.set_placeholder(placeholder.to_owned());
                editor
            })
        };
        let name = field(24., 32., 600., "Display name", cx);
        let pronouns = field(14., 18., 400., "Add pronouns", cx);
        let bio = field(14., 18., 400., "Share a fun fact about yourself", cx);
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
            upsell_closed: false,
            saving: false,
            error: None,
            owned: Vec::new(),
            offers: Vec::new(),
            _events: events,
        });
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
        if open.picker.is_some() {
            open.picker = None;
        } else if open.banner_color.is_some() {
            open.banner_color = None;
        } else if open.styles.is_some() {
            open.styles = None;
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
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        let what = key.strip_prefix(FETCH_PREFIX).unwrap_or_default();
        match what {
            "profile" => {
                let color = |value: &Value| value.as_u64().map(|number| number as u32);
                let profile = &body["user_profile"];
                open.saved.bio = profile["bio"].as_str().unwrap_or_default().to_owned();
                open.saved.pronouns = profile["pronouns"].as_str().unwrap_or_default().to_owned();
                open.saved.banner = profile["banner"].as_str().map(str::to_owned);
                open.saved.accent_color = color(&body["user"]["accent_color"]);
                open.saved.theme_colors = body["user_profile"]["theme_colors"]
                    .as_array()
                    .and_then(|list| Some([color(list.first()?)?, color(list.get(1)?)?]));
                for entry in body["user_profile"]["collectibles"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .chain(body["collectibles"].as_array().into_iter().flatten())
                {
                    let sku = entry["sku_id"].as_str().and_then(|sku| sku.parse().ok());
                    match entry["type"].as_i64() {
                        Some(1) => open.saved.equipped.profile_effect = sku,
                        Some(3) => open.saved.equipped.profile_frame = sku,
                        _ => {}
                    }
                }
                let (bio, pronouns) = (open.saved.bio.clone(), open.saved.pronouns.clone());
                let (bio_field, pronoun_field) = (open.bio.clone(), open.pronouns.clone());
                bio_field.update(cx, |editor, _| editor.set_text(&bio));
                pronoun_field.update(cx, |editor, _| editor.set_text(&pronouns));
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
                open.saving = false;
                if body["ok"] == true {
                    self.profile_editor = None;
                } else {
                    open.error = Some("Something went wrong. Try again.".to_owned());
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// `true` when the editor opened a premium-gated choice without Nitro (the official shows its upsell).
    pub(super) fn profile_editor_premium(&self) -> bool {
        self.premium() != dc_state::premium::PremiumType::None
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
        user
    }

    pub(super) fn profile_editor_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.profile_editor.as_ref()?;
        let user = self.profile_editor_user();
        self.profile_editor_request_art(&user);
        let close = div()
            .id("profile-editor-close")
            .absolute()
            .top(px(40.))
            .right(px(8.))
            .child(
                div()
                    .id("profile-editor-close-button")
                    .size(px(32.))
                    .rounded(px(8.))
                    .bg(color("--control-secondary-background-default"))
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
        let root = div()
            .parity("profile-editor")
            .id("profile-editor")
            .occlude()
            .relative()
            .w(px(1184.))
            .h_full()
            .flex_none()
            .rounded(px(12.))
            .overflow_hidden()
            .bg(color("--background-base-lower"))
            .flex()
            .font_family(SANS)
            .font_features(kerning())
            .child(self.profile_editor_panel(&user, window, cx))
            .child(self.profile_editor_stage(&user, window, cx))
            .child(close)
            .children(self.profile_editor_banner_popout(cx));
        let layer = deferred(
            div()
                .id("profile-editor-layer")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(color("--background-scrim"))
                .flex()
                .justify_center()
                .child(root),
        )
        .with_priority(2)
        .into_any_element();
        Some(layer)
    }

    /// The banner colour popout over the editor, and the click-away layer that closes it.
    fn profile_editor_banner_popout(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.profile_editor.as_ref()?.banner_color.as_ref()?;
        let popout = self.profile_editor_colour_popout(234., 364., cx)?;
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

    /// Asks for the art the preview needs: the avatar decoration of the pending choice.
    fn profile_editor_request_art(&mut self, user: &User) {
        if let Some(decoration) = user.avatar_decoration.as_deref() {
            self.request_image(
                format!("avatar-decoration:{}", decoration.sku_id),
                decoration.static_url(),
            );
        }
    }
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
    /// `main`, `preset <name>`, `pick <nameplate|decoration|effect|frame> [sku]`, `banner-color`, `styles`,
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
            ["banner-color"] => self.profile_editor_script_banner(cx),
            ["styles"] => self.profile_editor_script_styles(cx),
            ["font", id] => self.profile_editor_script_style(|state| {
                state.font = id.parse().unwrap_or(state.font);
            }),
            ["effect", id] => self.profile_editor_script_style(|state| {
                state.effect = id.parse().unwrap_or(state.effect);
                state.colors.clear();
            }),
            ["colors", hex @ ..] => {
                let colors: Vec<u32> = hex
                    .iter()
                    .filter_map(|value| u32::from_str_radix(value.trim_start_matches('#'), 16).ok())
                    .collect();
                self.profile_editor_script_style(|state| state.colors = colors);
            }
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
