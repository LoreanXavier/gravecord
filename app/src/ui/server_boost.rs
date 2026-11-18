//! Server Settings > Boost Perks (module 989433 `jf`, `jE`, `E6`, `jn`, `jh`, save `jN`; requests module 310527) and the
//! dialog every locked perk opens (module 331617, lists module 598933).
//!
//! The progress bar, banner, invite background and custom link wait for Save Changes. A locked one offers Unlock with
//! Boosting; the dialog's Boost This Server goes to the account's Server Boost page, since buying Boosts is not drawn.
//! Measured in `design/dom/settings/guild-boost-darker-cozy.json`, `modals/boost-unlock-*-darker-cozy.json`.

use super::tooltip::{Side, Tip};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use dc_state::Id;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, IntoElement, PathPromptOptions,
    Subscription, deferred, div, img, linear_color_stop, linear_gradient, prelude::*, px,
};
use serde_json::{Value, json};

pub(super) const BOOST_PREFIX: &str = "guild-boost:";
const VANITY_PREFIX: &str = "guild-boost:vanity:";
const SAVED_PREFIX: &str = "guild-boost:saved:";
/// The dark theme's picture of the bar in a channel list.
const PROGRESS_ART: &str = "https://cdn.discordapp.com/assets/content/b8d65794b7cc0b9eeca2f0842d619d0da7f31312a984da6146429bc1570995fc.png";
/// Module 387103.
const PERKS_ART: &str = "https://cdn.discordapp.com/assets/content/a9ce5551f37c49862694d063777be6e02b9190ff9a205c7c62f5fe9cecb673de.svg";
/// Module 652215's articles.
const BANNER_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360028716472";
const SPLASH_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/4415841146391";
const VANITY_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/115001542132";

/// `QR`, `Lp`, `DU`: what each locked perk's dialog lists, with `M`'s closing line.
const POWERFUL: (&str, &str) = (
    "boost_perk_3",
    "Powerful upgrades: larger file upload sizes, increased audio quality, and more!",
);
const BANNER_PERKS: [(&str, &str); 4] = [
    (
        "boost_perk_0",
        "Set a unique icon for each role in the server",
    ),
    (
        "boost_perk_1",
        "Upload up to 30 custom stickers and 150 custom emoji",
    ),
    ("boost_perk_2", "Screen share in 1440p/60fps"),
    POWERFUL,
];
const INVITE_PERKS: [(&str, &str); 4] = [
    (
        "boost_perk_images",
        "Customize your server invites with a custom background",
    ),
    ("boost_perk_1", "Upload custom stickers for everyone to use"),
    ("boost_perk_gif", "Stand out with an animated server icon"),
    POWERFUL,
];
const VANITY_PERKS: [(&str, &str); 4] = [
    (
        "boost_perk_v0",
        "Easily add new members with a Custom Invite Link",
    ),
    ("boost_perk_gif", "Stand out with an animated server banner"),
    (
        "boost_perk_1",
        "Upload up to 60 custom stickers and 250 custom emoji",
    ),
    POWERFUL,
];

/// Which perks a dialog lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Perks {
    Banner,
    Invite,
    Vanity,
}

impl Perks {
    fn list(self) -> [(&'static str, &'static str); 4] {
        match self {
            Self::Banner => BANNER_PERKS,
            Self::Invite => INVITE_PERKS,
            Self::Vanity => VANITY_PERKS,
        }
    }
}

/// What the page saves.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct BoostValues {
    progress_bar: bool,
    banner: Option<String>,
    splash: Option<String>,
    vanity: String,
}

/// `jN`'s two requests: the guild's own fields, then the link.
fn guild_body(saved: &BoostValues, now: &BoostValues) -> Option<Value> {
    let mut body = json!({});
    if saved.progress_bar != now.progress_bar {
        body["premium_progress_bar_enabled"] = json!(now.progress_bar);
    }
    if saved.banner != now.banner {
        body["banner"] = json!(now.banner);
    }
    if saved.splash != now.splash {
        body["splash"] = json!(now.splash);
    }
    (body != json!({})).then_some(body)
}

/// The page's state.
#[derive(Default)]
pub(crate) struct BoostState {
    saved: BoostValues,
    now: BoostValues,
    uses: u64,
    vanity_field: Option<(Entity<Editor>, Subscription)>,
    error: Option<String>,
    pub(super) dialog: Option<Perks>,
}

impl Root {
    fn boost_mut(&mut self) -> Option<&mut BoostState> {
        self.server_settings.as_mut().map(|open| &mut open.boost)
    }

    pub(super) fn request_boost(&mut self, guild: Id) {
        for url in [PROGRESS_ART, PERKS_ART] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{BOOST_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route: format!("/guilds/{guild}"),
            body: None,
        });
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{VANITY_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route: format!("/guilds/{guild}/vanity-url"),
            body: None,
        });
    }

    pub(super) fn boost_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        if let Some(guild) = key.strip_prefix(SAVED_PREFIX) {
            if report["ok"] == true {
                if let Ok(guild) = guild.parse::<Id>() {
                    self.request_boost(guild);
                }
            } else if let Some(state) = self.boost_mut() {
                state.error = report["body"]["message"].as_str().map(str::to_owned);
            }
        } else if key.starts_with(VANITY_PREFIX) {
            let code = report["body"]["code"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let uses = report["body"]["uses"].as_u64().unwrap_or(0);
            let field = cx.new(|cx| {
                let mut editor = Editor::new(super::editor_style(), cx);
                editor.set_text(&code);
                editor
            });
            let events = cx.subscribe(&field, |this, editor, event: &EditorEvent, cx| {
                if let EditorEvent::Submit(text) = event {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                let text = editor.read(cx).text().to_owned();
                if let Some(state) = this.boost_mut() {
                    state.now.vanity = text;
                }
                cx.notify();
            });
            if let Some(state) = self.boost_mut() {
                state.saved.vanity = code.clone();
                state.now.vanity = code;
                state.uses = uses;
                state.vanity_field = Some((field, events));
            }
        } else if report["ok"] == true {
            let body = &report["body"];
            let values = BoostValues {
                progress_bar: body["premium_progress_bar_enabled"]
                    .as_bool()
                    .unwrap_or(false),
                banner: body["banner"].as_str().map(str::to_owned),
                splash: body["splash"].as_str().map(str::to_owned),
                vanity: self
                    .server_settings
                    .as_ref()
                    .map(|open| open.boost.saved.vanity.clone())
                    .unwrap_or_default(),
            };
            if let Some(state) = self.boost_mut() {
                state.saved = values.clone();
                state.now = values;
                state.error = None;
            }
        }
        cx.notify();
    }

    pub(super) fn boost_dirty(&self) -> bool {
        self.server_settings
            .as_ref()
            .is_some_and(|open| open.boost.saved != open.boost.now)
    }

    pub(super) fn reset_boost(&mut self, cx: &mut Context<Self>) {
        let mut field = None;
        if let Some(state) = self.boost_mut() {
            state.now = state.saved.clone();
            state.error = None;
            field = state
                .vanity_field
                .as_ref()
                .map(|(editor, _)| (editor.clone(), state.saved.vanity.clone()));
        }
        if let Some((editor, code)) = field {
            editor.update(cx, |editor, _| editor.set_text(&code));
        }
        cx.notify();
    }

    pub(super) fn save_boost(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(state) = self.server_settings.as_ref().map(|open| &open.boost) else {
            return;
        };
        let body = guild_body(&state.saved, &state.now);
        let vanity = (state.saved.vanity != state.now.vanity).then(|| state.now.vanity.clone());
        if let Some(code) = vanity {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{SAVED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}/vanity-url"),
                body: Some(json!({ "code": code })),
            });
        }
        if let Some(body) = body {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{SAVED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}"),
                body: Some(body),
            });
        }
        cx.notify();
    }

    /// Any locked perk's button: opens the dialog with that perk's list.
    pub(super) fn open_perks(&mut self, perks: Perks, cx: &mut Context<Self>) {
        if let Some(state) = self.boost_mut() {
            state.dialog = Some(perks);
        }
        self.request_image(PERKS_ART.to_owned(), PERKS_ART.to_owned());
        cx.notify();
    }

    pub(super) fn close_perks(&mut self) -> bool {
        if let Some(state) = self.boost_mut()
            && state.dialog.is_some()
        {
            state.dialog = None;
            return true;
        }
        false
    }

    /// The banner opens the select image modal with `GUILD_BANNER` and its cropper (module 989433 `EK`, modules 685663
    /// and 142630); the invite background has no upload type and goes up as picked.
    fn pick_boost_image(&mut self, splash: bool, cx: &mut Context<Self>) {
        if !splash {
            self.open_image_select(super::image_crop::Purpose::GuildBanner, cx);
            return;
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.first()
            {
                let _ = this.update(cx, |this, cx| {
                    let uri = super::server_settings::data_uri(path);
                    if let Some(state) = this.boost_mut() {
                        state.now.splash = uri;
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// `onCrop` of the server banner: the cropped picture is the pending banner.
    pub(super) fn boost_banner_cropped(&mut self, uri: String, cx: &mut Context<Self>) {
        if let Some(state) = self.boost_mut() {
            state.now.banner = Some(uri);
        }
        cx.notify();
    }

    /// `EY.u`: the white button with its gem.
    fn boost_unlock(id: &'static str, perks: Perks, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(id)
            .w(px(202.))
            .h(px(40.))
            .pl(px(16.))
            .rounded(px(8.))
            .bg(gpui::rgb(0xffffff))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(250. / 360., 0.9, 0.7, 0.45),
                offset: gpui::point(px(0.), px(0.)),
                blur_radius: px(14.),
                spread_radius: px(0.),
                inset: false,
            }])
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                gpui::svg()
                    .path("icons/boost_gem.svg")
                    .size(px(20.))
                    .text_color(gpui::rgb(0x000000)),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(gpui::rgb(0x000000))
                    .child("Unlock with Boosting"),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_perks(perks, cx)))
            .into_any_element()
    }

    /// `gu.A`: the level a perk needs; `Click to learn more` over it.
    fn boost_badge(
        &self,
        id: &'static str,
        level: &'static str,
        perks: Perks,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let badge = div()
            .id(id)
            .ml(px(4.))
            .h(px(18.))
            .pl(px(2.))
            .pr(px(6.))
            .rounded(px(4.))
            .bg(color("--background-mod-subtle"))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(3.))
            .child(icon("boost_badge", 14., "--text-strong"))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(level),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_perks(perks, cx)));
        self.with_tip(
            format!("{id}-tip"),
            Tip::text("Click to learn more", Side::Top),
            badge,
            cx,
        )
        .into_any_element()
    }

    /// `E6`, `jn`: words and button left, the picture right.
    fn boost_image_section(
        &self,
        (title, badge, first, second): (&'static str, AnyElement, &'static str, AnyElement),
        (unlocked, splash, value): (bool, bool, Option<String>),
        perks: Perks,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let upload = div()
            .id(if splash {
                "boost-upload-splash"
            } else {
                "boost-upload-banner"
            })
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
            .child(if splash {
                "Upload Background"
            } else {
                "Upload Banner"
            })
            .on_click(cx.listener(move |this, _, _, cx| this.pick_boost_image(splash, cx)));
        let id = if splash {
            "boost-unlock-splash"
        } else {
            "boost-unlock-banner"
        };
        let tile = div()
            .id(if splash {
                "boost-tile-splash"
            } else {
                "boost-tile-banner"
            })
            .relative()
            .w(px(314.))
            .h(px(180.))
            .rounded(px(12.))
            .bg(gpui::rgb(0x4f505a))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .when(value.is_some(), |this| {
                this.child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(gpui::rgb(0xffffff))
                        .child(if splash {
                            "Change Background"
                        } else {
                            "Change Banner"
                        }),
                )
            })
            .child(
                div()
                    .absolute()
                    .top(px(16.))
                    .right(px(16.))
                    .size(px(28.))
                    .rounded_full()
                    .when(!unlocked, |this| this.opacity(0.5))
                    .bg(gpui::rgb(0xffffff))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path("icons/boost_add_image.svg")
                            .size(px(18.))
                            .text_color(gpui::rgb(0x4f505a)),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if unlocked {
                    this.pick_boost_image(splash, cx);
                } else {
                    this.open_perks(perks, cx);
                }
            }));
        div()
            .flex()
            .gap(px(32.))
            .child(
                div()
                    .w(px(314.))
                    .flex_none()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(badge),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .child(first),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .child(second),
                    )
                    .child(div().mt(px(16.)).child(if unlocked {
                        upload.into_any_element()
                    } else {
                        Self::boost_unlock(id, perks, cx)
                    })),
            )
            .child(div().w(px(314.)).h(px(182.)).flex_none().child(tile))
            .into_any_element()
    }

    /// The page (`jf`).
    pub(super) fn server_boost_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.server_settings.as_ref().map(|open| &open.boost) else {
            return div().into_any_element();
        };
        let known = self.stores.guild(guild);
        let has = |name: &str| known.is_some_and(|known| known.features.iter().any(|f| f == name));
        let me = self.stores.me();
        let can = |bit: u64| {
            known
                .zip(me)
                .is_some_and(|(known, me)| dc_state::permissions::can_in_guild(known, me, bit))
        };
        let manage = can(dc_state::permissions::MANAGE_GUILD);
        let admin = can(dc_state::permissions::ADMINISTRATOR);
        let now = state.now.clone();
        let error = state.error.clone();
        let uses = state.uses;
        let vanity_field = state
            .vanity_field
            .as_ref()
            .map(|(editor, _)| editor.clone());
        let art = self.images.get(PROGRESS_ART).cloned();
        let banner_words = self.text_with_link(
            "boost-banner-link",
            "The recommended minimum size is 960x540 and recommended aspect ratio is 16:9. ",
            "Learn more",
            ".",
            cx,
            |_, cx| cx.open_url(BANNER_ARTICLE),
        );
        let splash_words = self.text_with_link(
            "boost-splash-link",
            "The recommended minimum size is 1920x1080 and recommended aspect ratio is 16:9. ",
            "Learn more",
            "",
            cx,
            |_, cx| cx.open_url(SPLASH_ARTICLE),
        );
        let vanity_words = self.text_with_link(
            "boost-vanity-link",
            "Bring others to your server easily with your own customized invite link.  Heads up though, anyone with the link can join and you'll need at least one text channel that is open to all server members. ",
            "Learn more.",
            "",
            cx,
            |_, cx| cx.open_url(VANITY_ARTICLE),
        );
        let divider = || div().h(px(1.)).bg(color("--border-subtle"));
        let banner_badge = self.boost_badge("boost-badge-banner", "LVL 2", Perks::Banner, cx);
        let splash_badge = self.boost_badge("boost-badge-splash", "LVL 1", Perks::Invite, cx);
        let vanity_badge = self.boost_badge("boost-badge-vanity", "LVL 3", Perks::Vanity, cx);
        let banner = self.boost_image_section(
            (
                "Server Banner Background",
                banner_badge,
                "This image will display at the top of your channels list.",
                banner_words,
            ),
            (has("BANNER") && manage, false, now.banner.clone()),
            Perks::Banner,
            cx,
        );
        let splash = self.boost_image_section(
            (
                "Server Invite Background",
                splash_badge,
                "This image will display when your server invite is viewed in a browser, as well as in invite confirmation screens and Server Onboarding.",
                splash_words,
            ),
            (has("INVITE_SPLASH") && manage, true, now.splash.clone()),
            Perks::Invite,
            cx,
        );
        let vanity_open = has("VANITY_URL");
        let vanity: AnyElement = if vanity_open {
            let code_set = !now.vanity.is_empty();
            div()
                .mt(px(16.))
                .p(px(16.))
                .rounded(px(8.))
                .bg(color("--background-base-lower"))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .mb(px(8.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child("Invite URL"),
                        )
                        .when(code_set, |this| {
                            this.child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-subtle"))
                                    .child(if uses == 1 {
                                        "1 use".to_owned()
                                    } else {
                                        format!("{uses} uses")
                                    }),
                            )
                        }),
                )
                .child(
                    div()
                        .h(px(40.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .h_full()
                                .px(px(12.))
                                .flex()
                                .items_center()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child("discord.gg/"),
                        )
                        .children(vanity_field.map(|field| div().flex_1().child(field))),
                )
                .when(!state.saved.vanity.is_empty(), |this| {
                    this.child(
                        div().mt(px(16.)).flex().child(
                            div()
                                .id("boost-vanity-remove")
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
                                .child("Remove Custom Invite Link")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let field = this.boost_mut().and_then(|state| {
                                        state.now.vanity.clear();
                                        state.vanity_field.as_ref().map(|(field, _)| field.clone())
                                    });
                                    if let Some(field) = field {
                                        field.update(cx, |editor, _| editor.set_text(""));
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                })
                .into_any_element()
        } else {
            div()
                .mt(px(16.))
                .child(Self::boost_unlock("boost-unlock-vanity", Perks::Vanity, cx))
                .into_any_element()
        };
        let progress_on = now.progress_bar;
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Boost Perks"),
            )
            .child(
                div()
                    .mt(px(40.))
                    .flex()
                    .gap(px(32.))
                    .child(
                        div()
                            .w(px(314.))
                            .flex_none()
                            .flex()
                            .justify_between()
                            .items_start()
                            .child(
                                div()
                                    .w(px(242.))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child("Show Boost progress bar"),
                                    )
                                    .child(
                                        div()
                                            .mt(px(4.))
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-subtle"))
                                            .child("Servers with the Boost progress bar enabled get more Boosts. This progress bar will display in your channel list, attached to your server name (or server banner if you have one set)."),
                                    ),
                            )
                            .child(
                                div()
                                    .id("boost-progress-switch")
                                    .when(!manage, |this| this.opacity(0.5))
                                    .when(manage, |this| {
                                        this.cursor_pointer().on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(state) = this.boost_mut() {
                                                state.now.progress_bar = !state.now.progress_bar;
                                            }
                                            cx.notify();
                                        }))
                                    })
                                    .child(super::create_channel::switch(progress_on)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(314.))
                            .h(px(315.))
                            .flex_none()
                            .children(art.map(|art| img(art).w(px(314.)).h(px(312.)).rounded(px(8.)))),
                    ),
            )
            .child(div().my(px(40.)).child(divider()))
            .child(banner)
            .child(div().my(px(40.)).child(divider()))
            .child(splash)
            .when(admin, |this| {
                this.child(div().my(px(40.)).child(divider())).child(
                    div()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child("Custom Invite Link"),
                                )
                                .child(vanity_badge),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-subtle"))
                                .child(vanity_words),
                        )
                        .child(vanity),
                )
            })
            .children(error.map(|error| {
                div()
                    .mt(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error)
            }))
            .into_any_element()
    }

    /// Module 331617.
    pub(super) fn boost_perks_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let perks = self.server_settings.as_ref()?.boost.dialog?;
        let art = self.images.get(PERKS_ART).cloned();
        let close = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.close_perks();
            cx.notify();
        });
        let close_scrim = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.close_perks();
            cx.notify();
        });
        let rows: Vec<AnyElement> = perks
            .list()
            .into_iter()
            .map(|(glyph, text)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_none().child(icon(glyph, 20., "--text-muted")))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(text),
                    )
                    .into_any_element()
            })
            .collect();
        let pose = crate::ui::motion::modal_pose("boost-perks");
        Some(
            deferred(
                div()
                    .id("boost-perks-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(gpui::rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(close_scrim)
                    .child(
                        div()
                            .posed(pose)
                            .id("boost-perks")
                            .occlude()
                            .relative()
                            .w(px(480.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(0.))
                                    .left(px(0.))
                                    .right(px(0.))
                                    .h(px(150.))
                                    .rounded_t(px(11.))
                                    .bg(linear_gradient(
                                        180.,
                                        linear_color_stop(
                                            color("--expressive-gradient-pink-end"),
                                            0.,
                                        ),
                                        linear_color_stop(
                                            color("--expressive-gradient-pink-end").opacity(0.),
                                            1.,
                                        ),
                                    )),
                            )
                            .child(
                                div()
                                    .relative()
                                    .pt(px(24.))
                                    .px(px(32.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(
                                        div()
                                            .h(px(144.))
                                            .pt(px(20.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .children(
                                                art.map(|art| img(art).w(px(192.)).h(px(108.))),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt(px(8.))
                                            .text_size(px(24.))
                                            .line_height(px(32.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child("Unlock perks for everyone with Boosts"),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(32.))
                                    .px(px(32.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(12.))
                                    .children(rows),
                            )
                            .child(
                                div().mt(px(32.)).px(px(32.)).pb(px(32.)).child(
                                    div()
                                        .id("boost-this-server")
                                        .h(px(40.))
                                        .rounded(px(8.))
                                        .bg(gpui::rgb(0xffffff))
                                        .shadow(vec![gpui::BoxShadow {
                                            color: gpui::hsla(250. / 360., 0.9, 0.7, 0.45),
                                            offset: gpui::point(px(0.), px(0.)),
                                            blur_radius: px(14.),
                                            spread_radius: px(0.),
                                            inset: false,
                                        }])
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(4.))
                                        .child(
                                            gpui::svg()
                                                .path("icons/boost_gem.svg")
                                                .size(px(20.))
                                                .text_color(gpui::rgb(0x000000)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(16.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(gpui::rgb(0x000000))
                                                .child("Boost This Server"),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            // The account's free Boosts go to this server (`boost_apply.rs`).
                                            this.close_perks();
                                            match this.guild {
                                                Some(guild) => this.boost_this_server(guild, cx),
                                                None => {
                                                    this.server_settings = None;
                                                    this.open_settings_section("Server Boost", cx);
                                                }
                                            }
                                        })),
                                ),
                            )
                            .child(
                                div()
                                    .id("boost-perks-close")
                                    .absolute()
                                    .top(px(8.))
                                    .right(px(8.))
                                    .size(px(38.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        "modal_close",
                                        20.,
                                        "--control-icon-only-icon-default",
                                    ))
                                    .on_click(close),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// `H.M2T[TIER_3]` (module 652215): the Boosts Level 3 takes.
const TIER_3_BOOSTS: u64 = 14;
/// `tg` of module 616740: Level 3 plus the price of every power-up not included in a level (module 568065 `sy`:
/// Server Tag 3, Enhanced Role Styles 3, the badge packs 3 + 5 + 3 + 2). The experiment-gated Guild Theme and Game
/// Servers are left out, as an account outside those experiments sees it.
const POWERUP_BOOSTS: u64 = 3 + 3 + 3 + 5 + 3 + 2;

/// The bar's goal: `tg(guild)`.
pub(super) fn boost_goal(features: &[String]) -> u64 {
    if features.iter().any(|f| f == "PREMIUM_TIER_3_OVERRIDE") {
        POWERUP_BOOSTS
    } else {
        TIER_3_BOOSTS + POWERUP_BOOSTS
    }
}

/// The bar's label, `/rbPDs` and `Ehpq+7` of the strings: "applied/max Boosts", or just the count at the goal.
pub(super) fn boost_count_text(applied: u64, goal: u64) -> String {
    if applied >= goal {
        format!("{applied} Boosts")
    } else {
        format!("{applied}/{goal} Boosts")
    }
}

/// The progress fill's width as a fraction of the bar (`Math.min(applied / max * 100, 100)`).
pub(super) fn boost_fraction(applied: u64, goal: u64) -> f32 {
    if goal == 0 {
        return 0.;
    }
    (applied as f32 / goal as f32).min(1.)
}

impl Root {
    /// The boost progress bar of the channel list (`tp`/`tf` of module 616740, styles `.container_BFMMcK` and
    /// friends of `ed1ab6525c3040dd.css`): drawn when the server has the bar switched on and the New Member
    /// "Get Started" bar is not in its slot. No official frame shows it (the captured servers show Get Started), so
    /// its geometry is the CSS's and the row height `57` of the list's row measure.
    pub(super) fn boost_progress_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.current_guild()?;
        let goal = boost_goal(&guild.features);
        if !guild.premium_progress_bar_enabled || goal == 0 || self.guide_progress_shown() {
            return None;
        }
        let applied = guild.premium_subscription_count;
        let fraction = boost_fraction(applied, goal);
        let pink = super::hsla("--guild-boosting-pink-refresh");
        // `.progress` is `inset: 2px` of the 26px bar; its width is `calc(d% - 4px)` of the bar.
        let fill = gpui::relative(fraction);
        Some(
            div()
                .id("boost-progress")
                .parity("boost-progress")
                .flex_none()
                .flex()
                .flex_col()
                .h(px(57.))
                .child(
                    div()
                        .flex_none()
                        .ml(px(8.))
                        .pt(px(2.))
                        .px(px(2.))
                        .pb(px(12.))
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .id("boost-progress-open")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.open_boost_page(cx)))
                        .child(
                            div()
                                .relative()
                                .h(px(26.))
                                .rounded(px(9999.))
                                .bg(color("--background-base-low"))
                                .child(
                                    div()
                                        .absolute()
                                        .top(px(2.))
                                        .left(px(2.))
                                        .h(px(22.))
                                        .w(fill)
                                        .rounded(px(11.))
                                        .when(fraction <= 0.05, |bar| {
                                            bar.rounded_tr(px(0.)).rounded_br(px(0.))
                                        })
                                        .bg(gpui::linear_gradient(
                                            270.,
                                            linear_color_stop(pink.opacity(0.4), 0.),
                                            linear_color_stop(pink.opacity(0.1), 1.),
                                        )),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .px(px(8.))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(8.))
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-default"))
                                        .child("Boost Goal")
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(2.))
                                                .opacity(0.7)
                                                .child(boost_count_text(applied, goal))
                                                .child(icon(
                                                    "menu_caret_right",
                                                    12.,
                                                    "--text-default",
                                                )),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channel_list_bar_counts_to_its_goal() {
        assert_eq!(boost_goal(&[]), 33);
        assert_eq!(boost_goal(&["PREMIUM_TIER_3_OVERRIDE".to_owned()]), 19);
        assert_eq!(boost_count_text(2, 33), "2/33 Boosts");
        assert_eq!(boost_count_text(33, 33), "33 Boosts");
        assert_eq!(boost_fraction(33, 33), 1.);
        assert_eq!(boost_fraction(40, 33), 1.);
        assert!((boost_fraction(2, 33) - 2. / 33.).abs() < 1e-6);
    }

    #[test]
    fn only_what_changed_is_saved() {
        let saved = BoostValues::default();
        assert_eq!(guild_body(&saved, &saved), None);
        let now = BoostValues {
            progress_bar: true,
            ..Default::default()
        };
        assert_eq!(
            guild_body(&saved, &now),
            Some(json!({"premium_progress_bar_enabled": true}))
        );
    }

    #[test]
    fn each_perk_opens_its_own_list() {
        assert_eq!(
            Perks::Banner.list()[1].1,
            "Upload up to 30 custom stickers and 150 custom emoji"
        );
        assert_eq!(
            Perks::Vanity.list()[0].1,
            "Easily add new members with a Custom Invite Link"
        );
        assert_eq!(Perks::Invite.list()[3], POWERFUL);
    }
}
