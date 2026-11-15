//! Server Settings > Welcome Screen (module 989433 `uu`/`uc`, status card `us`, channel row `ud`, store `dZ`; the
//! right-click menu is module 36456, the confirm modal 825894 and the "Explore the Server" modal 538471).
//! Measured in `design/dom/settings/guild-welcome*-darker-cozy.json`.
//!
//! The page lists only for a Community server whose welcome screen has channels and that is not a Server Guide
//! guild (`GUILD_SERVER_GUIDE`). In this build the screen can only be viewed and disabled, and its channels removed or
//! reordered: Enable is always disabled (`disabled: null==s||!n||d` in `us`). Every change is written at once with
//! `PATCH /guilds/{id}/welcome-screen`; when it fails the action bar says so and "Try Again" sends it again.

use super::modal::{self, Tone};
use super::server_settings::Page;
use super::{Root, color, icon};
use crate::net::Request;
use dc_state::{Id, permissions};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Pixels, Point, SharedString, div, img,
    prelude::*, px,
};
use serde_json::{Value, json};

pub(super) const WELCOME_PREFIX: &str = "guild-welcome:";
const GET_PREFIX: &str = "guild-welcome:get:";
const PATCH_PREFIX: &str = "guild-welcome:patch:";
const GUILD_PREFIX: &str = "guild-welcome:guild:";
const INVALID: &str = "You must remove all invalid channels first before you can save.";
const GONE: &str = "This channel is no longer in Welcome Screen because it does not exist or is not viewable by @everyone";
const CONFIRM: &str = "Are you sure you want to remove this channel from recommended channels?";

#[derive(Default)]
pub(crate) struct WelcomeState {
    /// The first answer came (a 404 is an empty screen).
    pub(super) loaded: bool,
    pub(super) description: Option<String>,
    /// `welcome_channels` as the server sent them: each is written back as it was.
    pub(super) channels: Vec<Value>,
    /// The guild's rules channel (`GET /guilds/{id}`): its glyph differs.
    rules: Option<Id>,
    /// The last write failed: the bar says so, and holds what to send again.
    error: bool,
    pending: Option<Value>,
    /// "Explore the Server".
    preview: bool,
    /// The row whose remove confirmation is open.
    remove: Option<usize>,
    /// The row menu: its row and where it was opened.
    menu: Option<(usize, Point<Pixels>)>,
}

struct Drag(usize);

struct Ghost;

impl gpui::Render for Ghost {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// `ud`'s numbers: the text of a channel row.
fn text_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn channel_id(row: &Value) -> Option<Id> {
    row["channel_id"].as_str()?.parse().ok()
}

/// `j` in `uc`: the row moved to `to`.
fn moved(channels: &[Value], from: usize, to: usize) -> Vec<Value> {
    let mut list = channels.to_vec();
    if from < list.len() && to < list.len() && from != to {
        let row = list.remove(from);
        list.insert(to, row);
    }
    list
}

impl WelcomeState {
    fn read(&mut self, body: &Value) {
        self.description = body["description"].as_str().map(str::to_owned);
        self.channels = body["welcome_channels"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        self.loaded = true;
    }
}

impl Root {
    fn welcome_mut(&mut self) -> Option<&mut WelcomeState> {
        self.server_settings.as_mut().map(|open| &mut open.welcome)
    }

    /// Escape: the row menu, the remove confirmation and the preview close one at a time before the page does.
    pub(super) fn close_welcome_layer(&mut self) -> bool {
        self.welcome_mut().is_some_and(|state| {
            state.menu.take().is_some()
                || state.remove.take().is_some()
                || std::mem::take(&mut state.preview)
        })
    }

    pub(super) fn request_welcome(&mut self, guild: Id) {
        for (key, route) in [
            (
                format!("{GET_PREFIX}{guild}"),
                format!("/guilds/{guild}/welcome-screen"),
            ),
            (format!("{GUILD_PREFIX}{guild}"), format!("/guilds/{guild}")),
        ] {
            let _ = self.requests.send(Request::Rest {
                key: Some(key),
                method: reqwest::Method::GET,
                route,
                body: None,
            });
        }
    }

    /// The sidebar entry: `canManageGuild && COMMUNITY && a screen with channels && !GUILD_SERVER_GUIDE`.
    pub(super) fn welcome_listed(&self, guild: Id) -> bool {
        let Some(open) = self.server_settings.as_ref() else {
            return false;
        };
        let Some((known, me)) = self.stores.guild(guild).zip(self.stores.me()) else {
            return false;
        };
        let has = |name: &str| known.features.iter().any(|feature| feature == name);
        has("COMMUNITY")
            && !has("GUILD_SERVER_GUIDE")
            && permissions::can_in_guild(known, me, permissions::MANAGE_GUILD)
            && (!open.welcome.channels.is_empty() || open.page == Page::Welcome)
    }

    pub(super) fn welcome_dirty(&self) -> bool {
        self.server_settings
            .as_ref()
            .is_some_and(|open| open.welcome.error)
    }

    pub(super) fn welcome_bar_message(&self) -> &'static str {
        INVALID
    }

    pub(super) fn welcome_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.remove(key).unwrap_or_default();
        let ok = report["ok"] == true;
        let body = report["body"].clone();
        let guild = self.server_settings.as_ref().map_or(0, |open| open.guild);
        if key.starts_with(GUILD_PREFIX) {
            let rules = body["rules_channel_id"]
                .as_str()
                .and_then(|id| id.parse().ok());
            if let Some(state) = self.welcome_mut() {
                state.rules = rules;
            }
        } else if key.starts_with(GET_PREFIX) {
            let mut wanted = Vec::new();
            if let Some(state) = self.welcome_mut() {
                if ok {
                    state.read(&body);
                    wanted = state.channels.clone();
                } else {
                    // A 404 is an empty screen.
                    state.channels.clear();
                    state.loaded = true;
                }
            }
            for row in wanted {
                self.welcome_request_emoji(&row);
            }
        } else if key.starts_with(PATCH_PREFIX) {
            let mut wanted = Vec::new();
            if let Some(state) = self.welcome_mut() {
                if ok {
                    state.read(&body);
                    state.error = false;
                    state.pending = None;
                    wanted = state.channels.clone();
                } else {
                    state.error = true;
                }
            }
            for row in wanted {
                self.welcome_request_emoji(&row);
            }
            if !ok && guild != 0 {
                // The page shows what the server has, as the store did before the write.
                self.request_welcome(guild);
            }
        }
        cx.notify();
    }

    fn welcome_request_emoji(&mut self, row: &Value) {
        if let Some(id) = row["emoji_id"].as_str() {
            self.request_image(
                format!("custom:{id}"),
                format!("https://cdn.discordapp.com/emojis/{id}.webp?size=48"),
            );
        } else if let Some(name) = row["emoji_name"].as_str() {
            self.request_emoji_image(name);
        }
    }

    /// `dQ.i4`: shows the change and writes it.
    fn welcome_write(&mut self, guild: Id, change: Value, cx: &mut Context<Self>) {
        let Some(state) = self.welcome_mut() else {
            return;
        };
        if let Some(channels) = change["welcome_channels"].as_array() {
            state.channels = channels.clone();
        }
        state.pending = Some(change.clone());
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{PATCH_PREFIX}{guild}")),
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}/welcome-screen"),
            body: Some(change),
        });
        cx.notify();
    }

    pub(super) fn retry_welcome(&mut self, guild: Id, cx: &mut Context<Self>) {
        let pending = self.welcome_mut().and_then(|state| state.pending.clone());
        if let Some(change) = pending {
            self.welcome_write(guild, change, cx);
        } else {
            self.request_welcome(guild);
        }
    }

    pub(super) fn reset_welcome(&mut self, guild: Id, cx: &mut Context<Self>) {
        if let Some(state) = self.welcome_mut() {
            state.error = false;
            state.pending = None;
        }
        self.request_welcome(guild);
        cx.notify();
    }

    fn welcome_remove(&mut self, guild: Id, index: usize, cx: &mut Context<Self>) {
        let Some(state) = self.welcome_mut() else {
            return;
        };
        let mut channels = state.channels.clone();
        if index < channels.len() {
            channels.remove(index);
            self.welcome_write(guild, json!({ "welcome_channels": channels }), cx);
        }
    }

    fn welcome_move(&mut self, guild: Id, from: usize, to: usize, cx: &mut Context<Self>) {
        let Some(state) = self.welcome_mut() else {
            return;
        };
        let channels = moved(&state.channels, from, to);
        if channels != state.channels {
            self.welcome_write(guild, json!({ "welcome_channels": channels }), cx);
        }
    }

    /// `sf.A` in the notice: Onboarding opens in these settings.
    fn welcome_to_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        open.page = Page::Onboarding;
        let guild = open.guild;
        self.request_server_page(guild, Page::Onboarding, cx);
        cx.notify();
    }

    /// The channel behind a row: its name, glyph and whether this user can see it.
    fn welcome_target(&self, guild: Id, row: &Value) -> Option<(String, &'static str, bool)> {
        let known = self.stores.guild(guild)?;
        let channel = known
            .channels
            .iter()
            .find(|channel| Some(channel.id) == channel_id(row))?;
        let me = self.stores.me()?;
        let rules = self
            .server_settings
            .as_ref()
            .and_then(|open| open.welcome.rules);
        let visible = permissions::can_in_channel(known, channel, me, permissions::VIEW_CHANNEL);
        Some((
            channel.name.clone(),
            super::server_onboarding::channel_glyph(known, channel, rules),
            visible,
        ))
    }

    fn welcome_emoji(&self, row: &Value) -> Option<std::sync::Arc<gpui::Image>> {
        if let Some(id) = row["emoji_id"].as_str() {
            return self.images.get(&format!("custom:{id}")).cloned();
        }
        self.images
            .get(&format!("emoji:{}", row["emoji_name"].as_str()?))
            .cloned()
    }

    fn welcome_tile(&self, guild: Id, size: f32, radius: f32) -> AnyElement {
        let known = self.stores.guild(guild);
        let picture = self.images.get(&format!("guild:{guild}")).cloned();
        let acronym = known
            .map(|known| dc_state::Guild::acronym_of(&known.name))
            .unwrap_or_default();
        div()
            .size(px(size))
            .flex_none()
            .rounded(px(radius))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(17.))
            .line_height(px(21.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .when_some(picture.clone(), |this, picture| {
                this.child(img(picture).size(px(size)).rounded(px(radius)))
            })
            .when(picture.is_none(), |this| this.child(acronym))
            .into_any_element()
    }

    /// `ud`.
    fn welcome_row(
        &self,
        guild: Id,
        index: usize,
        row: &Value,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = self.welcome_target(guild, row);
        let visible = target.as_ref().is_some_and(|(_, _, visible)| *visible);
        let picture = self.welcome_emoji(row);
        let has_emoji = row["emoji_id"].is_string() || row["emoji_name"].is_string();
        let lead: AnyElement = match (&target, visible) {
            (Some(_), true) if has_emoji => div()
                .size(px(22.))
                .flex_none()
                .when_some(picture, |this, picture| {
                    this.child(img(picture).size(px(22.)))
                })
                .into_any_element(),
            (Some((_, glyph, _)), true) => icon(glyph, 24., "--icon-subtle").into_any_element(),
            _ => icon("welcome_notice", 24., "--icon-feedback-critical").into_any_element(),
        };
        let sub: AnyElement = match &target {
            Some((name, glyph, true)) => div()
                .flex()
                .items_center()
                .gap(px(2.))
                .child(icon(glyph, 12., "--text-default"))
                .child(name.clone())
                .into_any_element(),
            _ => div().child(GONE).into_any_element(),
        };
        let position = index;
        div()
            .id(SharedString::from(format!("welcome-channel-{index}")))
            .mt(px(8.))
            .px(px(16.))
            .py(px(12.))
            .rounded(px(4.))
            .bg(color("--background-base-low"))
            .cursor_grab()
            .flex()
            .items_center()
            .on_drag(Drag(index), |_, _, _, cx| cx.new(|_| Ghost))
            .on_drop(cx.listener(move |this, drag: &Drag, _, cx| {
                this.welcome_move(guild, drag.0, position, cx)
            }))
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                    this.close_all_context_menus();
                    if let Some(state) = this.welcome_mut() {
                        state.menu = Some((position, event.position));
                    }
                    cx.notify();
                }),
            )
            .child(lead)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .ml(px(16.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(text_of(row, "description")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child(sub),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("welcome-remove-{index}")))
                    .size(px(40.))
                    .flex_none()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                    .child(icon(
                        "channel_settings_trash",
                        20.,
                        "--control-icon-only-icon-default",
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.welcome_mut() {
                            state.remove = Some(position);
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// `us`: the card above the preview.
    fn welcome_status(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.server_settings.as_ref().map(|open| &open.welcome) else {
            return div().into_any_element();
        };
        // `WELCOME_SCREEN_ENABLED` in the guild's features: the write's answer does not carry it, the update does.
        let enabled = self.stores.guild(guild).is_some_and(|known| {
            known
                .features
                .iter()
                .any(|feature| feature == "WELCOME_SCREEN_ENABLED")
        });
        let (first, second) = if enabled {
            (
                "Your Welcome Screen is up and running.",
                "New members will be greeted by this screen when they join!",
            )
        } else {
            (
                "Your changes are saved automatically.",
                "Don\u{2019}t forget to enable when you\u{2019}re finished!",
            )
        };
        let button = |id: &'static str, label: &'static str, tone: Tone, live: bool| {
            let (background, border, text) = match tone {
                Tone::Primary => (
                    color("--control-primary-background-default"),
                    color("--control-primary-border-default"),
                    color("--white"),
                ),
                Tone::Critical => (
                    color("--control-critical-primary-background-default"),
                    color("--control-critical-primary-border-default"),
                    color("--white"),
                ),
                _ => (
                    color("--control-secondary-background-default"),
                    color("--border-muted"),
                    color("--control-secondary-text-default"),
                ),
            };
            div()
                .id(id)
                .h(px(32.))
                .px(px(11.))
                .rounded(px(8.))
                .border_1()
                .border_color(border)
                .bg(background)
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(text)
                .when(live, |this| this.cursor_pointer())
                .when(!live, |this| this.opacity(0.5))
                .child(label)
        };
        div()
            .mt(px(32.))
            .h(px(68.))
            .p(px(16.))
            .rounded_t(px(8.))
            .bg(color("--background-base-lowest"))
            .flex()
            .items_center()
            .child(if enabled {
                div()
                    .size(px(24.))
                    .flex_none()
                    .rounded_full()
                    .bg(gpui::rgb(0x3d9e60))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("ob_check", 16., "--white"))
                    .into_any_element()
            } else {
                div()
                    .size(px(24.))
                    .flex_none()
                    .rounded_full()
                    .bg(gpui::rgba(0x97979f14))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("channel_status_pencil", 14., "--text-subtle"))
                    .into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .ml(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-strong"))
                    .child(div().child(first))
                    .child(div().child(second)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .when(!state.channels.is_empty(), |this| {
                        this.child(
                            button("welcome-preview", "Preview", Tone::Secondary, true).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(state) = this.welcome_mut() {
                                        state.preview = true;
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                    })
                    .child(if enabled {
                        button("welcome-toggle", "Disable", Tone::Critical, true).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.welcome_write(guild, json!({ "enabled": false }), cx)
                            }),
                        )
                    } else {
                        // Enable is never available in this build (`disabled: null==s||!n||d`); whether it is on comes from the guild's `WELCOME_SCREEN_ENABLED` feature (`dq`).
                        button("welcome-toggle", "Enable", Tone::Primary, false)
                    }),
            )
            .into_any_element()
    }

    /// `uu`/`uc`.
    pub(super) fn server_welcome_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.server_settings.as_ref() else {
            return div().into_any_element();
        };
        let state = &open.welcome;
        let name = self
            .stores
            .guild(guild)
            .map(|known| known.name.clone())
            .unwrap_or_default();
        let rows: Vec<AnyElement> = state
            .channels
            .iter()
            .enumerate()
            .map(|(index, row)| self.welcome_row(guild, index, row, cx))
            .collect();
        let notice = div()
            .mb(px(16.))
            .px(px(16.))
            .pt(px(15.))
            .pb(px(17.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--status-warning"))
            .bg(gpui::rgba(0xfaa61a1a))
            .relative()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(
                div()
                    .absolute()
                    .left(px(16.))
                    .top(px(18.))
                    .child(icon("welcome_notice", 14., "--text-default")),
            )
            .child(self.text_with_link(
                "welcome-notice",
                "\u{2003}\u{2009}Changes to the Welcome Screen are disabled. The Welcome Screen will still appear for your members. A new and improved version of Welcome Screen is now available through ",
                "Onboarding",
                ", and will replace Welcome Screen if enabled.",
                cx,
                |this, cx| this.welcome_to_onboarding(cx),
            ));
        div()
            .flex()
            .flex_col()
            .pb(px(12.))
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Welcome Screen"),
            )
            .child(notice)
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("If enabled, this Welcome Screen appears for new members when they join your server. This screen helps new members discover all the cool things they can do in your server!"),
            )
            .child(self.welcome_status(guild, cx))
            .child(
                div()
                    .p(px(16.))
                    .rounded_b(px(8.))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .flex_col()
                    .child(self.welcome_header(guild, &name, state.description.as_deref(), false))
                    .child(div().mt(px(32.)).mb(px(32.)).h(px(1.)).bg(color("--border-subtle")))
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
                                    .child("Top things to do"),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child("Select up to 5 recommended channels. We suggest channels where people can interact, like channels for discussions, asking questions, reading news, or selecting roles."),
                            )
                            .child(div().mt(px(8.)).flex().flex_col().children(rows)),
                    ),
            )
            .into_any_element()
    }

    /// The tile, the greeting and the description: the page's preview and the modal share them.
    fn welcome_header(
        &self,
        guild: Id,
        name: &str,
        description: Option<&str>,
        modal: bool,
    ) -> AnyElement {
        let greeting = div()
            .text_size(px(24.))
            .line_height(px(32.))
            .font_weight(FontWeight(700.))
            .text_color(color("--text-strong"))
            .flex()
            .gap(px(6.))
            .child("Welcome to")
            .child(name.to_owned());
        if modal {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .child(div().mt(px(16.)).child(self.welcome_tile(guild, 64., 16.)))
                .child(div().mt(px(16.)).child(greeting))
                .when_some(description.map(str::to_owned), |this, text| {
                    this.child(
                        div()
                            .mt(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(text),
                    )
                })
                .into_any_element();
        }
        let text = description.filter(|text| !text.is_empty());
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(div().mt(px(16.)).child(self.welcome_tile(guild, 64., 16.)))
            .child(div().mt(px(16.)).mb(px(8.)).child(greeting))
            .child(
                div()
                    .w(px(440.))
                    .h(px(89.))
                    .px(px(10.))
                    .py(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    .opacity(0.6)
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(if text.is_some() {
                        color("--text-default")
                    } else {
                        color("--input-placeholder-text-default")
                    })
                    .child(text.map_or(
                        "What's this server about? What do people do here?".to_owned(),
                        str::to_owned,
                    )),
            )
            .into_any_element()
    }

    /// The remove confirmation, the preview and the row menu.
    pub(super) fn welcome_layers(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(open) = self.server_settings.as_ref() else {
            return Vec::new();
        };
        let guild = open.guild;
        let state = &open.welcome;
        let mut layers = Vec::new();
        if let Some(index) = state.remove
            && let Some(row) = state.channels.get(index)
        {
            let target = self.welcome_target(guild, row);
            let body: AnyElement = match target {
                Some((name, _, _)) => div()
                    .flex()
                    .gap(px(4.))
                    .child("Are you sure you want to remove")
                    .child(
                        div()
                            .font_weight(FontWeight(700.))
                            .child(format!("#{name}")),
                    )
                    .child("from recommended channels?")
                    .into_any_element(),
                None => div().child(CONFIRM).into_any_element(),
            };
            let spec = modal::Spec {
                id: "welcome-remove-modal",
                title: "Remove Recommended Channel".into(),
                subtitle: None,
                body: Some(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(body)
                        .into_any_element(),
                ),
                buttons: vec![
                    modal::button(
                        "welcome-remove-cancel",
                        "Cancel",
                        Tone::Secondary,
                        |this, _, _| {
                            if let Some(state) = this.welcome_mut() {
                                state.remove = None;
                            }
                        },
                    ),
                    modal::button(
                        "welcome-remove-confirm",
                        "Remove",
                        Tone::Critical,
                        move |this, _, cx| {
                            let index = this.welcome_mut().and_then(|state| state.remove.take());
                            if let Some(index) = index {
                                this.welcome_remove(guild, index, cx);
                            }
                        },
                    ),
                ],
                close: Box::new(|this, _, _| {
                    if let Some(state) = this.welcome_mut() {
                        state.remove = None;
                    }
                }),
            };
            layers.push(modal::render(spec, cx));
        }
        if state.preview {
            let name = self
                .stores
                .guild(guild)
                .map(|known| known.name.clone())
                .unwrap_or_default();
            let rows: Vec<AnyElement> = state
                .channels
                .iter()
                .enumerate()
                .filter_map(|(index, row)| {
                    let (channel_name, glyph, visible) = self.welcome_target(guild, row)?;
                    if !visible {
                        return None;
                    }
                    let picture = self.welcome_emoji(row);
                    let has_emoji = row["emoji_id"].is_string() || row["emoji_name"].is_string();
                    let target = channel_id(row)?;
                    Some(
                        div()
                            .id(SharedString::from(format!("welcome-preview-row-{index}")))
                            .h(px(60.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(color("--background-base-low"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .child(if has_emoji {
                                div()
                                    .size(px(24.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when_some(picture, |this, picture| {
                                        this.child(img(picture).size(px(24.)))
                                    })
                                    .into_any_element()
                            } else {
                                icon(glyph, 24., "--icon-subtle").into_any_element()
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .ml(px(12.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-strong"))
                                            .child(text_of(row, "description")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(2.))
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(icon(glyph, 12., "--text-default"))
                                            .child(channel_name),
                                    ),
                            )
                            .child(icon("welcome_chevron", 24., "--text-strong"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.server_settings = None;
                                this.select(guild, Some(target));
                                cx.notify();
                            }))
                            .into_any_element(),
                    )
                })
                .collect();
            let body = div()
                .flex()
                .flex_col()
                .child(self.welcome_header(guild, &name, state.description.as_deref(), true))
                .child(
                    div()
                        .mt(px(32.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child("Top things to do here"),
                )
                .child(
                    div()
                        .mt(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .children(rows),
                )
                .into_any_element();
            let spec = modal::Spec {
                id: "welcome-preview-modal",
                title: "Explore the Server".into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![modal::button(
                    "welcome-preview-done",
                    "I'll just look around for now",
                    Tone::Secondary,
                    |this, _, _| {
                        if let Some(state) = this.welcome_mut() {
                            state.preview = false;
                        }
                    },
                )],
                close: Box::new(|this, _, _| {
                    if let Some(state) = this.welcome_mut() {
                        state.preview = false;
                    }
                }),
            };
            layers.push(modal::render(spec, cx));
        }
        if let Some((index, at)) = state.menu {
            let last = state.channels.len().saturating_sub(1);
            let entry = |id: &'static str, label: &'static str, live: bool, danger: bool| {
                div()
                    .id(id)
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(2.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(if danger {
                        color("--text-feedback-critical")
                    } else {
                        color("--text-strong")
                    })
                    .when(live, |this| {
                        this.cursor_pointer().hover(|style| {
                            style
                                .bg(color("--interactive-background-hover"))
                                .rounded(px(4.))
                        })
                    })
                    .when(!live, |this| this.opacity(0.5))
                    .child(label)
            };
            let close = |this: &mut Root| {
                if let Some(state) = this.welcome_mut() {
                    state.menu = None;
                }
            };
            let menu = div()
                .id("welcome-menu")
                .occlude()
                .w(px(188.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                    close(this);
                    cx.notify();
                }))
                .child(
                    entry("welcome-menu-up", "Move Up", index > 0, false).on_click(cx.listener(
                        move |this, _, _, cx| {
                            close(this);
                            if index > 0 {
                                this.welcome_move(guild, index, index - 1, cx);
                            }
                        },
                    )),
                )
                .child(
                    entry("welcome-menu-down", "Move Down", index < last, false).on_click(
                        cx.listener(move |this, _, _, cx| {
                            close(this);
                            if index < last {
                                this.welcome_move(guild, index, index + 1, cx);
                            }
                        }),
                    ),
                )
                .child(div().my(px(8.)).h(px(1.)).bg(color("--border-subtle")))
                .child(
                    entry("welcome-menu-remove", "Remove Channel", true, true).on_click(
                        cx.listener(move |this, _, _, cx| {
                            close(this);
                            if let Some(state) = this.welcome_mut() {
                                state.remove = Some(index);
                            }
                            cx.notify();
                        }),
                    ),
                );
            layers.push(
                gpui::deferred(
                    gpui::anchored()
                        .position(at)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu),
                )
                .with_priority(2)
                .into_any_element(),
            );
        }
        layers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(names: &[&str]) -> Vec<Value> {
        names
            .iter()
            .enumerate()
            .map(|(at, name)| json!({"channel_id": (at + 1).to_string(), "description": name}))
            .collect()
    }

    #[test]
    fn a_row_moves_and_the_others_keep_their_order() {
        let list = rows(&["a", "b", "c"]);
        let moved_down = moved(&list, 0, 2);
        let names: Vec<&str> = moved_down
            .iter()
            .map(|row| row["description"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["b", "c", "a"]);
        assert_eq!(moved(&list, 1, 1), list);
        assert_eq!(moved(&list, 0, 9), list);
    }

    #[test]
    fn the_screen_reads_what_the_server_sent() {
        let mut state = WelcomeState::default();
        state.read(&json!({
            "description": "Hello",
            "welcome_channels": [{"channel_id": "7", "description": "Rules", "emoji_name": "\u{1f4dc}", "emoji_id": null}],
        }));
        assert!(state.loaded);
        assert_eq!(state.description.as_deref(), Some("Hello"));
        assert_eq!(channel_id(&state.channels[0]), Some(7));
        state.read(&json!({"description": null, "welcome_channels": []}));
        assert!(state.description.is_none() && state.channels.is_empty());
    }
}
