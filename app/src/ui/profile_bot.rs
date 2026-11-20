//! A bot's (an app's) profile surfaces. Module 589022 and `iN` of module 148540 route a profile in the same order:
//! a restricted profile, then a bot that is not a user (`isNonUserBot`: the system account or a `0000` bot, which
//! keep the user surfaces' non-user-bot branches), then any other bot to these:
//!
//! - The bot popout (module 476897): the profile theme around the 300px card, only the bot overflow menu (module
//!   630891) on the banner, the names with the app tag (465829), the mutuals (791556), the app's buttons (`buttons_
//!   0VkShF`: Add App of module 570314, the Store of module 571077), the clamped bio (442228), the activity cards
//!   (646986), the popular commands (612770), the roles under a "Roles" heading for a member (900179 + 403369) and the
//!   message field when the account may write to it (848674 with 570287). No profile effect, frame, private notice,
//!   wishlist notice or widgets.
//! - The bot profile modal (`e8` of module 148540): the legacy `user-profile-modal` (600 by 780 at most
//!   `100vh - 32px`, `root_NxZlvf`), the 210px banner, the 120px avatar 24px in and 68px over the header line, the
//!   header buttons (Message or its icon, Store, Add App), the profile read warning (559506, `fetchError_NxZlvf`), the
//!   large names, and the tabs Bio (`ep`: bio, Commands, Links, Roles, Created On, Note), Mutual Servers (`e0`) and Data
//!   Access (`$`).

use super::Root;
use super::dm_profile::Palette;
use super::profile_menu::Which;
use super::tooltip::{Side, Tip};
use crate::parity::Parity;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, SharedString, Window, deferred, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, rgba,
};
use serde_json::Value;

/// The tabs of `e8` (`$` of module 148540): Bio, Mutual Servers, Data Access.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum BotTab {
    #[default]
    Info,
    Servers,
    DataAccess,
}

/// `A.bot` past `isNonUserBot()`: the bot surfaces of modules 476897 and 148540.
pub(super) fn app_bot(user: &User) -> bool {
    user.bot && !user.non_user_bot()
}

/// `oauth2_install_params` of any integration type of the application (`Ie` / `Z` of modules 204776 and 692848).
fn integration_install(app: &Value) -> bool {
    app["integration_types_config"]
        .as_object()
        .is_some_and(|types| {
            types
                .values()
                .any(|kind| !kind["oauth2_install_params"].is_null())
        })
}

/// `Ie` of module 204776: the application can be installed (a custom install URL, install params, or install params
/// in an integration type).
pub(super) fn installable(app: &Value) -> bool {
    !app["custom_install_url"].is_null()
        || !app["install_params"].is_null()
        || integration_install(app)
}

/// `V` of module 342384: the custom install URL, else the authorize page with the client id and, without
/// integration-type install params, the install params' permissions and scopes.
pub(super) fn add_app_url(app: &Value) -> String {
    if let Some(url) = app["custom_install_url"].as_str() {
        return url.to_owned();
    }
    let mut query = format!("client_id={}", app["id"].as_str().unwrap_or_default());
    let params = &app["install_params"];
    if params.is_object() && !integration_install(app) {
        if let Some(permissions) = params["permissions"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| params["permissions"].as_u64().map(|bits| bits.to_string()))
        {
            query.push_str(&format!("&permissions={permissions}"));
        }
        if let Some(scopes) = params["scopes"].as_array() {
            let joined: Vec<&str> = scopes.iter().filter_map(Value::as_str).collect();
            // `URLSearchParams` writes a space as `+`.
            query.push_str(&format!("&scope={}", joined.join("+")));
        }
    }
    format!("https://discord.com/oauth2/authorize?{query}")
}

/// `isDiscordUrl` of module 998218: the host is discord.com, discordapp.com or discordmerch.com or under them.
fn discord_url(url: &str) -> bool {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["discordapp.com", "discord.com", "discordmerch.com"]
        .iter()
        .any(|base| host == *base || host.ends_with(&format!(".{base}")))
}

/// Module 570314's icon: a plus without a custom install URL (or one on Discord), else the external link.
pub(super) fn install_glyph(app: &Value) -> &'static str {
    match app["custom_install_url"].as_str() {
        Some(url) if !discord_url(url) => "link_external_small",
        _ => "bot_plus_small",
    }
}

/// Copy Link of module 630891 (`X`): an application with an embedded activity on the main surface (878014 `W(app,
/// MAIN)`) links to its activity page with the account as referrer, any other to its install URL.
pub(super) fn copy_link_url(app: &Value, me: Option<Id>) -> String {
    let main = app["embedded_surfaces"]
        .as_array()
        .is_some_and(|surfaces| surfaces.iter().any(|surface| surface.as_u64() == Some(0)));
    if main {
        let id = app["id"].as_str().unwrap_or_default();
        return match me {
            Some(me) => format!("https://discord.com/activities/{id}?referrer_id={me}"),
            None => format!("https://discord.com/activities/{id}"),
        };
    }
    add_app_url(app)
}

/// A press of Add App (570314 `F`): the app launcher on the app inside a DM with a bot, else `o` of module 692848.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum AddAppPress {
    OpenApp,
    /// The custom install URL opens in the browser.
    Link(String),
    /// The OAuth2 modal with these args (`openOAuth2Modal`).
    Authorize(Value),
    Nothing,
}

pub(super) fn add_app_press(app: &Value, in_bot_dm: bool) -> AddAppPress {
    if in_bot_dm {
        return AddAppPress::OpenApp;
    }
    let id = app["id"].as_str().unwrap_or_default().to_owned();
    if let Some(url) = app["custom_install_url"].as_str() {
        return AddAppPress::Link(url.to_owned());
    }
    if integration_install(app) {
        // `openOAuth2Modal({clientId})`: the modal reads the install params of the integration type; the guild install
        // is taken when the application has one, else the user install.
        let config = &app["integration_types_config"];
        let (kind, params) = ["0", "1"]
            .into_iter()
            .find_map(|kind| {
                let params = &config[kind]["oauth2_install_params"];
                params.is_object().then_some((kind, params))
            })
            .unwrap_or(("0", &Value::Null));
        return AddAppPress::Authorize(serde_json::json!({
            "client_id": id,
            "scopes": params["scopes"],
            "permissions": params["permissions"],
            "integration_type": kind,
        }));
    }
    let params = &app["install_params"];
    if params.is_object() {
        return AddAppPress::Authorize(serde_json::json!({
            "client_id": id,
            "scopes": params["scopes"],
            "permissions": params["permissions"],
        }));
    }
    AddAppPress::Nothing
}

/// The application of a bot's profile answer (`UserProfileStore` `application`).
fn profile_app(body: Option<&Value>) -> Option<&Value> {
    body.map(|body| &body["application"])
        .filter(|app| app.is_object() && app["id"].is_string())
}

/// The empty Mutual Servers art (`emptyIconGuilds_6X8yxV`, `images-dark`).
const EMPTY_GUILDS_ART: &str = "https://discord.com/assets/1fc96c69951bfa5c.svg";
/// The Streamer Mode art (`emptyIconStreamerMode_6X8yxV`, `images-dark`).
const STREAMER_ART: &str = "https://discord.com/assets/4120cce23ff52c68.svg";
/// The Help Center article of the Data Access tab (`b6nqk0`'s `helpCenterUrl`).
const DATA_ACCESS_HELP: &str = "https://support.discord.com/hc/articles/7933951485975";

impl Root {
    /// Whether the selected channel is a DM with exactly one bot (module 177640).
    fn in_bot_dm(&self) -> bool {
        self.current_dm()
            .filter(|dm| dm.kind == 1 && dm.recipients.len() == 1)
            .and_then(|dm| self.stores.users.get(&dm.recipients[0]))
            .is_some_and(|known| known.bot)
    }

    /// Module 570287: the message field shows when the account is neither the user nor lurking the server, and is
    /// their friend or shares a server whose direct messages it allows, or has a game friend in them with game DMs.
    pub(super) fn bot_can_message(&self, user: Id) -> bool {
        let me = self.stores.current_user.as_ref().map(|known| known.id);
        if me == Some(user)
            || self
                .guild
                .is_some_and(|guild| self.stores.lurking_guilds.contains(&guild))
        {
            return false;
        }
        if self.stores.relationships.get(&user) == Some(&super::friends::RELATIONSHIP_FRIEND) {
            return true;
        }
        let restricted = self
            .stores
            .proto_ids(&dc_proto::settings::DM_RESTRICTED_GUILDS);
        self.stores.guilds.iter().any(|guild| {
            !restricted.contains(&guild.id) && self.stores.members.get(guild.id, user).is_some()
        })
    }

    /// A `sm` button of module 821609 on the profile's tokens (as `profile_small_button`), with a 16px icon before
    /// the text, 4px apart.
    fn bot_button(
        &self,
        id: impl Into<SharedString>,
        label: SharedString,
        glyph: Option<&'static str>,
        primary: bool,
        full_width: bool,
        palette: &Palette,
    ) -> gpui::Stateful<gpui::Div> {
        let family = if primary { "primary" } else { "secondary" };
        let token = |part: &str| palette.token(&format!("--control-{family}-{part}"));
        let text = if primary {
            palette.token("--white")
        } else {
            token("text-default")
        };
        div()
            .id(id.into())
            .flex_none()
            .when(full_width, |this| this.flex_1().w_full())
            .h(px(32.))
            .min_w(px(60.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(token("border-default"))
            .bg(token("background-default"))
            .hover(|style| style.bg(token("background-hover")))
            .active(|style| style.bg(token("background-active")))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(text)
            .when_some(glyph, |this, glyph| {
                this.child(
                    gpui::svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(16.))
                        .flex_none()
                        .text_color(text),
                )
            })
            .child(label)
    }

    /// Add App / Open App (module 570314) when the application can be installed; on the desktop client (`h.p5`) its
    /// right-click menu `user-bot-profile-add-app` holds Copy Add App URL and the button keeps FD's secondary look.
    fn add_app_button(
        &self,
        user: Id,
        probe: &'static str,
        full_width: bool,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let app = profile_app(self.fetched.get(&format!("profile:{user}")))?;
        if !installable(app) {
            return None;
        }
        let open = self.in_bot_dm();
        let label = if open {
            dc_intl::t("Cia+A8")
        } else {
            dc_intl::t("NgXl3C")
        };
        let glyph = (!open).then(|| install_glyph(app));
        let press = add_app_press(app, open);
        let app_id = app["id"].as_str().unwrap_or_default().to_owned();
        Some(
            div()
                .relative()
                .when(full_width, |this| this.w_full())
                .flex()
                .child(self.anchor_probe(probe))
                .child(
                    self.bot_button(
                        SharedString::from(format!("{probe}-button")),
                        label.into(),
                        glyph,
                        false,
                        full_width,
                        palette,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.press_add_app(&press, &app_id, window, cx);
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.open_profile_menu_at(user, Which::AddApp, probe, true, cx);
                        }),
                    ),
                )
                .into_any_element(),
        )
    }

    fn press_add_app(
        &mut self,
        press: &AddAppPress,
        app: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match press {
            // `hg(app)` (OPEN_APP_LAUNCHER) and the popout closes.
            AddAppPress::OpenApp => {
                self.profile = None;
                self.full_profile = None;
                if self.slash.launcher.is_none() {
                    self.open_launcher(window, cx);
                }
                self.open_launcher_app(app.to_owned(), cx);
            }
            AddAppPress::Link(url) => self.follow_link(url, url, cx),
            AddAppPress::Authorize(args) => {
                let nonce = format!("profile-add-{app}");
                self.oauth_begin(nonce, args, cx);
            }
            AddAppPress::Nothing => {}
        }
        cx.notify();
    }

    /// Store of module 571077 (`PRESS_APP_STOREFRONT`, the primary button with the shop icon) for an application
    /// with `storefront_available`: the application storefront modal (module 719847).
    fn store_button(
        &self,
        user: Id,
        id: &'static str,
        full_width: bool,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let app = profile_app(self.fetched.get(&format!("profile:{user}")))?;
        if !super::app_storefront::storefront_available(app) {
            return None;
        }
        let app_id = app["id"].as_str().unwrap_or_default().to_owned();
        let guild = self.guild;
        Some(
            self.bot_button(
                id,
                dc_intl::t("V7j5aZ").into(),
                Some("home_shop"),
                true,
                full_width,
                palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_app_storefront(app_id.clone(), guild, cx);
            }))
            .into_any_element(),
        )
    }

    /// `buttons_0VkShF` of the bot popout: Add App, then the Store (module 571077), both full width.
    pub(super) fn bot_popout_buttons(
        &self,
        user: Id,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let add = self.add_app_button(user, "bot-popout-add-app", true, palette, cx);
        let store = self.store_button(user, "bot-popout-store", true, palette, cx);
        if add.is_none() && store.is_none() {
            return None;
        }
        Some(
            div()
                .parity("bot-popout-buttons")
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .children(add)
                .children(store)
                .into_any_element(),
        )
    }

    /// The bot popout's roles: module 403369 under the "Roles" heading of 900179, for a member of the server.
    pub(super) fn bot_roles_section(
        &self,
        user: Id,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let guild = self.guild?;
        self.stores.members.get(guild, user)?;
        let roles = self.profile_role_chips(user, palette.text, cx)?;
        Some(
            section(dc_intl::t("LPJmL/"), palette)
                .child(roles)
                .into_any_element(),
        )
    }

    /// The bot overflow button (module 630891: `PRESS_OPTIONS`, More), opening `user-bot-profile-overflow-menu`.
    pub(super) fn bot_more_button(
        &self,
        user: Id,
        probe: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = self.profile_palette(user, self.guild, false);
        let light = palette
            .theme
            .as_ref()
            .is_some_and(super::profile_theme::ProfileTheme::light);
        let target = div()
            .id(probe)
            .relative()
            .cursor_pointer()
            .child(self.anchor_probe(probe))
            .child(super::dm_profile::round_button_themed(
                "more_horizontal",
                palette.themed,
                light,
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.open_profile_menu_at(user, Which::Bot, probe, false, cx)
            }));
        self.with_tip(
            format!("{probe}-tip"),
            Tip::text(dc_intl::t!("UKOtz+"), Side::Top),
            target,
            cx,
        )
        .into_any_element()
    }

    /// Module 630891's groups: [View Full Profile (the popout's), Share Profile], [Ignore, Block, Report App], [Copy
    /// Link, Copy App ID (developer mode)].
    pub(super) fn bot_menu_groups(
        &self,
        user: Id,
        from_popout: bool,
        cx: &mut Context<Self>,
    ) -> Vec<Vec<AnyElement>> {
        let blocked = self.is_blocked(user);
        let ignored = self.stores.is_ignored(user);
        let app = profile_app(self.fetched.get(&format!("profile:{user}"))).cloned();
        let mut first = Vec::new();
        if from_popout {
            first.push(
                self.profile_row(
                    "bot-view-profile",
                    dc_intl::t("+Xp3hq").into(),
                    false,
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.profile_menu = None;
                    this.open_full_profile(user, window, cx);
                }))
                .into_any_element(),
            );
        }
        // GAP: the modal's `viewProfileItem` (module 722868, View Main/Server Profile) needs a server profile of the
        // bot, which this client does not tell apart; it is left out.
        first.push(
            self.profile_row(
                "bot-share-profile",
                dc_intl::t("sFN1/M").into(),
                false,
                false,
                cx,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.profile_menu = None;
                if let Some(known) = this.stores.users.get(&user).cloned() {
                    this.open_share_profile(known, window, cx);
                }
                cx.notify();
            }))
            .into_any_element(),
        );
        let mut second = Vec::new();
        if !blocked {
            second.push(
                self.profile_row(
                    "bot-ignore",
                    super::remediation::ignore_label(ignored),
                    false,
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.profile_menu = None;
                    if ignored {
                        this.unignore_user(user, cx);
                    } else {
                        this.remediation = Some((user, super::remediation::Remedy::Ignore));
                    }
                    cx.notify();
                }))
                .into_any_element(),
            );
        }
        second.push(
            self.profile_row(
                "bot-block",
                if blocked { "Unblock" } else { "Block" }.into(),
                !blocked,
                false,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.profile_menu = None;
                if blocked {
                    this.unblock_user(user, cx);
                } else {
                    this.remediation = Some((user, super::remediation::Remedy::Block));
                }
                cx.notify();
            }))
            .into_any_element(),
        );
        let mut third = Vec::new();
        if let Some(app) = app {
            let app_id: Id = app["id"]
                .as_str()
                .and_then(|id| id.parse().ok())
                .unwrap_or(0);
            second.push(
                self.profile_row(
                    "bot-report-app",
                    dc_intl::t("NgA5vp").into(),
                    true,
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.profile_menu = None;
                    this.chat_extra.alert =
                        Some(super::chat_notices::report_app_alert(user, app_id));
                    cx.notify();
                }))
                .into_any_element(),
            );
            let link = copy_link_url(&app, self.stores.current_user.as_ref().map(|me| me.id));
            third.push(
                self.profile_row(
                    "bot-copy-link",
                    dc_intl::t("WqhZss").into(),
                    false,
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.profile_menu = None;
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(link.clone()));
                    cx.notify();
                }))
                .into_any_element(),
            );
            if self.developer_mode() {
                third.push(
                    self.profile_row(
                        "bot-copy-app-id",
                        dc_intl::t("+NP/b2").into(),
                        false,
                        false,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profile_menu = None;
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(app_id.to_string()));
                        cx.notify();
                    }))
                    .into_any_element(),
                );
            }
        }
        vec![first, second, third]
    }

    /// `user-bot-profile-add-app`: Copy Add App URL (`XWDihq`, module 342384 `V`).
    pub(super) fn add_app_menu_rows(&self, user: Id, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(app) = profile_app(self.fetched.get(&format!("profile:{user}"))) else {
            return Vec::new();
        };
        let url = add_app_url(app);
        vec![
            self.profile_row(
                "bot-copy-add-app-url",
                dc_intl::t("XWDihq").into(),
                false,
                false,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.profile_menu = None;
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(url.clone()));
                cx.notify();
            }))
            .into_any_element(),
        ]
    }

    /// The bot profile modal (`e8` of module 148540) when the open full profile is an app's.
    pub(super) fn bot_profile_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (user_id, guild, tab, note) = {
            let open = self.full_profile.as_ref()?;
            (open.user, open.guild, open.bot_tab, open.note.clone())
        };
        let user = self.stores.users.get(&user_id)?.clone();
        let palette = self.profile_palette(user_id, guild, false);
        let default: Hsla = palette.token("--text-default").into();
        let placeholder: Hsla = palette.token("--input-placeholder-text-default").into();
        note.update(cx, |editor, _| {
            editor.set_style(crate::editor::EditorStyle {
                size: 14.,
                line_height: 18.,
                text: default,
                placeholder,
                placeholder_italic: true,
                caret: default,
                ..super::editor_style()
            });
        });
        let body = self.fetched.get(&format!("profile:{user_id}")).cloned();
        let presence = self.stores.status(user_id).to_owned();
        let vh = f32::from(window.viewport_size().height);
        let height = 780f32.min(vh - 32.);
        // `.user-profile-modal` with `--custom-user-profile-banner-height: 210px` (themeType MODAL).
        let banner = if palette.themed {
            palette.top
        } else {
            self.profile_banner_fill(&user, guild)
        };
        let banner_area = div()
            .parity("bot-modal-banner")
            .relative()
            .h(px(210.))
            .flex_none()
            .overflow_hidden()
            .bg(banner)
            .children(
                self.profile_banner_image_in(user_id, guild)
                    .map(|image| img(image).size_full().object_fit(gpui::ObjectFit::Cover)),
            )
            .children(self.live_banner_in(user_id, guild, 600., 210., cx));
        // `headerButtons_NxZlvf`: Message (its text, or its icon once another button follows), Store, Add App.
        let add = self.add_app_button(user_id, "bot-modal-add-app", false, &palette, cx);
        let store = self.store_button(user_id, "bot-modal-store", false, &palette, cx);
        let message_icon_only = add.is_some() || store.is_some();
        let message_label: SharedString = dc_intl::t("zROXEV").into();
        let message: AnyElement = if message_icon_only {
            let target = self
                .profile_small_button("bot-modal-message-icon", "", false, &palette)
                .min_w(px(32.))
                .w(px(32.))
                .px(px(0.))
                .child(
                    gpui::svg()
                        .path("icons/chat_bubble.svg")
                        .size(px(16.))
                        .text_color(palette.token("--control-secondary-text-default")),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.full_profile = None;
                    this.open_dm_with(user_id, cx);
                }));
            self.with_tip(
                "bot-modal-message-tip",
                Tip::text(dc_intl::t!("zROXEV"), Side::Top),
                target,
                cx,
            )
            .into_any_element()
        } else {
            self.bot_button(
                "bot-modal-message",
                message_label,
                Some("chat_bubble"),
                false,
                false,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.full_profile = None;
                this.open_dm_with(user_id, cx);
            }))
            .into_any_element()
        };
        let header = div()
            .parity("bot-modal-header")
            .relative()
            .mr(px(16.))
            .min_h(px(52.))
            .flex()
            .justify_end()
            .child(
                // `.user-profile-modal :where(.avatar_WRLIGC)`: 24px in, 68px over the line; the 8px cutout ring of
                // SIZE_120 around it.
                div()
                    .absolute()
                    .left(px(16.))
                    .top(px(-76.))
                    .size(px(136.))
                    .rounded_full()
                    .bg(palette.top)
                    .child(div().size_full().rounded_full().bg(palette.overlay)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(24.))
                    .top(px(-68.))
                    .size(px(120.))
                    .child(self.modal_avatar(&user, guild, &presence, cx)),
            )
            .child(
                div()
                    .parity("bot-modal-header-buttons")
                    .flex_1()
                    // `--custom-button-button-md-height` (38px) under 16px of padding.
                    .h(px(38. + 16.))
                    .pt(px(16.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(message)
                    .children(store)
                    .children(add),
            );
        let overflow = div()
            .absolute()
            .top(px(12.))
            .right(px(16.))
            .child(self.bot_more_button(user_id, "bot-modal-more", cx));
        let names = div()
            .parity("bot-modal-names")
            .mx(px(16.))
            .mt(px(16.))
            .child(self.profile_names("bot-modal", &user, guild, &palette, None, true, cx));
        let tab_item = |which: BotTab, label: String, cx: &mut Context<Self>| {
            let selected = which == tab;
            let active: Hsla = palette.token("--interactive-text-active").into();
            div()
                .id(SharedString::from(format!("bot-modal-tab-{which:?}")))
                .relative()
                .h(px(25.))
                .mb(px(-1.))
                .px(px(4.))
                .pb(px(16.))
                .rounded_t(px(4.))
                .text_size(px(14.))
                .line_height(px(14.))
                .text_color(if selected { active } else { default })
                .when(!selected, |this| this.cursor_pointer())
                .child(label)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.))
                        .rounded_t(px(2.))
                        .when(selected, |this| this.bg(active))
                        .when(!selected, |this| {
                            this.group_hover(format!("bot-modal-tab-{which:?}"), move |style| {
                                style.bg(active)
                            })
                        }),
                )
                .group(format!("bot-modal-tab-{which:?}"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(open) = this.full_profile.as_mut() {
                        open.bot_tab = which;
                    }
                    cx.notify();
                }))
        };
        let servers = mutual_servers(body.as_ref());
        let streamer = self.streamer_hides_personal();
        let tabs: AnyElement = if streamer {
            // `e1` in Streamer Mode: no tabs, the Streamer Mode art and "Streamer Mode Enabled".
            empty_state(self, STREAMER_ART, dc_intl::t("Br1ls3"), default, cx)
        } else {
            let content: AnyElement = match tab {
                BotTab::Info => self.bot_info_tab(&user, guild, body.as_ref(), &palette, note, cx),
                BotTab::Servers => self.bot_servers_tab(&servers, &palette, cx),
                BotTab::DataAccess => bot_data_access_tab(body.as_ref(), &palette, cx),
            };
            div()
                .parity("bot-modal-tabs")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(px(8.))
                .child(
                    div()
                        .mt(px(16.))
                        .px(px(16.))
                        .flex()
                        .gap(px(32.))
                        .border_b_1()
                        .border_color(palette.token("--user-profile-border"))
                        .child(tab_item(BotTab::Info, dc_intl::t("ZzAR2Y").to_owned(), cx))
                        .child(tab_item(
                            BotTab::Servers,
                            super::full_profile::mutual_servers_label(
                                servers.as_ref().map(Vec::len),
                            ),
                            cx,
                        ))
                        .child(tab_item(
                            BotTab::DataAccess,
                            dc_intl::t("WstFb0").to_owned(),
                            cx,
                        )),
                )
                .child(
                    div()
                        .id("bot-modal-tab-panel")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .child(content),
                )
                .into_any_element()
        };
        let warning = self
            .profile_fetch_warning(user_id)
            .map(|warning| div().mx(px(16.)).mt(px(16.)).child(warning));
        let inner = div()
            .relative()
            .size_full()
            .rounded(px(if palette.themed { 4. } else { 8. }))
            .overflow_hidden()
            .bg(palette.overlay)
            .flex()
            .flex_col()
            .child(banner_area)
            .child(header)
            .children(warning)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .overflow_hidden()
                    .child(names)
                    .child(tabs),
            )
            .child(overflow);
        let modal = div()
            .parity("bot-profile-modal")
            .id("bot-profile-modal")
            .occlude()
            .w(px(600.))
            .h(px(height))
            .rounded(px(8.))
            .overflow_hidden()
            .when(palette.themed, |this| this.p(px(4.)))
            .bg(linear_gradient(
                180.,
                linear_color_stop(palette.top, 0.),
                linear_color_stop(palette.bottom, 1.),
            ))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(inner);
        let pose = crate::ui::motion::legacy_modal_pose("bot-profile", "bot-profile-layer", false);
        use crate::ui::motion::Posed as _;
        Some(
            deferred(
                div()
                    .id("bot-profile-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.close_full_profile(cx)))
                    .child(modal.posed(pose)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `ep` of module 148540 (`scroller_jxHDeR`: 16px in, sections 20px apart): the bio unclamped, the Commands, the
    /// Links, the server roles, "Created On" and the note.
    fn bot_info_tab(
        &self,
        user: &User,
        guild: Option<Id>,
        body: Option<&Value>,
        palette: &Palette,
        note: gpui::Entity<crate::editor::Editor>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = user.id;
        let default: Hsla = palette.token("--text-default").into();
        let strong: Hsla = palette.token("--text-strong").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let bio = super::dm_profile::shown_bio(
            id,
            body.and_then(|body| body["user_profile"]["bio"].as_str())
                .filter(|bio| !bio.is_empty())
                .map(str::to_owned),
        );
        let app = profile_app(body);
        let links: Vec<(&'static str, &'static str, String)> = app
            .map(|app| {
                [
                    ("file", "s7STcY", &app["terms_of_service_url"]),
                    ("lock", "kH3JR5", &app["privacy_policy_url"]),
                ]
                .into_iter()
                .filter_map(|(glyph, key, url)| Some((glyph, key, url.as_str()?.to_owned())))
                .collect()
            })
            .unwrap_or_default();
        let link_color = palette.token("--text-link");
        let links_section = (!links.is_empty()).then(|| {
            section(dc_intl::t("l6DP2n"), palette)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .children(links.into_iter().map(|(glyph, key, url)| {
                            let label = dc_intl::t(key);
                            let target = url.clone();
                            div()
                                .id(SharedString::from(format!("bot-modal-link-{key}")))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .cursor_pointer()
                                .child(
                                    gpui::svg()
                                        .path(format!("icons/{}.svg", link_glyph(glyph)))
                                        .size(px(20.))
                                        .flex_none()
                                        .text_color(link_color),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(default)
                                        .child(label),
                                )
                                // `trusted: false`: the link goes through the leaving-Discord check.
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.follow_link(&target, label, cx);
                                }))
                        })),
                )
                .into_any_element()
        });
        // `s?.guildId != null` (503026): the profile was read with this server's member profile.
        let roles = guild.and_then(|guild| {
            let for_guild = body.is_some_and(|body| {
                body["guild_member_profile"].is_object()
                    && body["guild_member_profile"]["guild_id"]
                        .as_str()
                        .is_none_or(|known| known == guild.to_string())
            });
            if !for_guild {
                return None;
            }
            self.stores.members.get(guild, id)?;
            self.profile_role_chips_for(guild, id, default)
                .map(|roles| {
                    section(dc_intl::t("LPJmL/"), palette)
                        .child(roles)
                        .into_any_element()
                })
        });
        let note_hover = palette.token("--user-profile-background-hover");
        div()
            .parity("bot-modal-info")
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .when_some(bio, |this, bio| this.child(self.bio(&bio, default, cx)))
            .children(self.profile_commands_section(id, palette, cx))
            .children(links_section)
            .children(roles)
            .child(
                section(dc_intl::t("A//N4k"), palette)
                    .child(self.full_profile_member_since(id, guild, strong, default, subtle, cx)),
            )
            .child(
                section(dc_intl::t("mQKv+v"), palette).child(
                    div()
                        .ml(px(-7.))
                        .mt(px(-4.))
                        .px(px(7.))
                        .py(px(1.))
                        .rounded(px(8.))
                        .hover(move |style| style.bg(note_hover))
                        .child(note),
                ),
            )
            .into_any_element()
    }

    /// `e0` of module 148540 (`listScroller_6X8yxV`, 12px above and below): a row of module 51943 per mutual server,
    /// or the empty art with "No servers in common".
    fn bot_servers_tab(
        &self,
        servers: &Option<Vec<(Id, Option<String>)>>,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let default: Hsla = palette.token("--text-default").into();
        let strong: Hsla = palette.token("--text-strong").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let Some(servers) = servers.as_ref().filter(|servers| !servers.is_empty()) else {
            return empty_state(self, EMPTY_GUILDS_ART, dc_intl::t("zjVh8h"), default, cx);
        };
        let row_hover = palette.token("--user-profile-background-hover");
        let tile = palette.token("--background-base-low");
        div()
            .parity("bot-modal-servers")
            .py(px(12.))
            .mr(px(4.))
            .flex()
            .flex_col()
            .children(servers.iter().filter_map(|(guild, nick)| {
                let known = self.stores.guilds.iter().find(|known| known.id == *guild)?;
                let guild = *guild;
                let name = known.name.clone();
                let picture = self.images.get(&format!("guild:{guild}")).cloned();
                let leading = div()
                    .size(px(40.))
                    .flex_none()
                    .rounded(px(12.))
                    .overflow_hidden()
                    .bg(tile)
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
                        SharedString::from(format!("bot-modal-server-{guild}")),
                        leading,
                        name.clone(),
                        strong,
                        row_hover,
                    )
                    .ml(px(12.))
                    .aria_label(dc_intl::t!("Ce2iXp", guildName = name).to_string())
                    .children(nick.clone().map(|nick| {
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
            .into_any_element()
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// Capture only: `profile-popout <user>` opens the popout as a press on a name at 400,80 would.
    pub fn script_profile_popout(&mut self, user: Id, cx: &mut Context<Self>) -> String {
        if !self.stores.users.contains_key(&user) {
            return format!("profile popout: {user} unknown");
        }
        let anchor =
            gpui::Bounds::new(gpui::point(px(400.), px(80.)), gpui::size(px(40.), px(20.)));
        self.open_profile(user, anchor, false, cx);
        format!("profile popout: {user} open")
    }
}

/// The icons of the Links rows (`FileIcon`, `LockIcon`).
fn link_glyph(name: &str) -> &'static str {
    match name {
        "file" => "bot_file",
        _ => "access_lock",
    }
}

/// The mutual servers of the profile answer; `None` until it arrives (`186272` reads "Mutual Servers" then).
fn mutual_servers(body: Option<&Value>) -> Option<Vec<(Id, Option<String>)>> {
    let list = body?["mutual_guilds"].as_array()?;
    Some(
        list.iter()
            .filter_map(|entry| {
                Some((
                    entry["id"].as_str()?.parse().ok()?,
                    entry["nick"].as_str().map(str::to_owned),
                ))
            })
            .collect(),
    )
}

/// A section of module 900179: the `body-xs/semibold` heading in `text-default` over its content, 8px apart.
fn section(heading: &str, palette: &Palette) -> gpui::Div {
    div().flex().flex_col().gap(px(8.)).child(
        div()
            .h(px(16.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(600.))
            .text_color(palette.token("--text-default"))
            .child(heading.to_owned()),
    )
}

/// `empty_6X8yxV`: the 240 by 130 art over its line (`emptyText_6X8yxV`: 14px medium on 16px, 12px under it),
/// centred with 20px above and below.
fn empty_state(
    root: &Root,
    art: &'static str,
    text: &str,
    tint: Hsla,
    cx: &mut Context<Root>,
) -> AnyElement {
    let picture = root.images.get(art).cloned();
    if picture.is_none() {
        root.want_image(art.to_owned(), art.to_owned(), cx);
    }
    div()
        .parity("bot-modal-empty")
        .flex_1()
        .size_full()
        .py(px(20.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(240.))
                .h(px(130.))
                .children(picture.map(|picture| img(picture).size_full())),
        )
        .child(
            div()
                .mt(px(12.))
                .text_size(px(14.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(tint)
                .child(text.to_owned()),
        )
        .into_any_element()
}

/// `$` of module 148540 (`scroller__BRe_9`: 16px in, 32px apart): a row per intent the application asks for
/// (152249 `Z`, the limited flags too), then the baseline line with its Help Center link.
fn bot_data_access_tab(
    body: Option<&Value>,
    palette: &Palette,
    cx: &mut Context<Root>,
) -> AnyElement {
    let intents = profile_app(body)
        .map(super::discover::app_data::intents)
        .unwrap_or_default();
    let strong = palette.token("--text-strong");
    let default = palette.token("--text-default");
    let row = |glyph: &'static str, title: &'static str, description: &'static str| {
        // `row__BRe_9` (20px apart) with `rowIcon__BRe_9` (40px circle, `--background-mod-muted`, the 20px icon in
        // `--icon-default`) and `rowText__BRe_9` (4px apart).
        div()
            .flex()
            .items_center()
            .gap(px(20.))
            .child(
                div()
                    .size(px(40.))
                    .flex_none()
                    .rounded_full()
                    .bg(palette.token("--background-mod-muted"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path(format!("icons/{glyph}.svg"))
                            .size(px(20.))
                            .text_color(palette.token("--icon-default")),
                    ),
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
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(strong)
                            .child(dc_intl::t(title)),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(default)
                            .child(dc_intl::t(description)),
                    ),
            )
    };
    let link = palette.token("--text-link");
    div()
        .parity("bot-modal-data-access")
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(32.))
        .when(intents.message_content, |this| {
            this.child(row("discover_intent_chat", "7Tmhfu", "eu5x7z"))
        })
        .when(intents.presences, |this| {
            this.child(row("bot_game_controller", "awYSsM", "TpsXoC"))
        })
        .when(intents.members, |this| {
            this.child(row("discover_intent_members", "WuKeSJ", "wOBmtV"))
        })
        .child(
            // `b6nqk0` in `body-xs/normal`: "All apps can access a set of baseline data. Please visit the Help
            // Center to learn more."
            div()
                .flex()
                .flex_wrap()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(default)
                .child("All apps can access a set of baseline data. Please visit the\u{a0}")
                .child(
                    div()
                        .id("bot-modal-help-center")
                        .cursor_pointer()
                        .text_color(link)
                        .hover(|style| style.underline())
                        .child("Help Center")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.follow_link(DATA_ACCESS_HELP, "Help Center", cx);
                        })),
                )
                .child("\u{a0}to learn more."),
        )
        .into_any_element()
}

impl Root {
    /// `ip` of module 148540 (`RestrictedUserProfileModalV2`): confirmation dialog before viewing a blocked or
    /// ignored user's profile (`b33pLD` "Show Profile?", `T7QiLn`/`MnEowy`, `kcuWva`, `QxrDY1`/`W6fjkS`,
    /// `783123` `UJKH/l` "Yes, show profile", `254828` `QbcRCJ` "Don't show this again").
    pub(super) fn restricted_profile_modal(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.full_profile.as_ref()?;
        let user_id = open.user;
        let guild = open.guild;
        let blocked = self.is_blocked(user_id);
        let name = self
            .stores
            .users
            .get(&user_id)
            .map(|u| u.display_name().to_owned())
            .unwrap_or_else(|| user_id.to_string());
        let subtitle = dc_intl::format(
            if blocked { "T7QiLn" } else { "MnEowy" },
            &[(
                "username",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&name)),
            )],
        );
        let aria = dc_intl::format(
            "KRe1Fk",
            &[("name", dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&name)))],
        );
        let bullet1 = dc_intl::t("kcuWva");
        let bullet2 = dc_intl::t(if blocked { "QxrDY1" } else { "W6fjkS" });
        let avatar_img = self.stores.users.get(&user_id).and_then(|u| {
            let (key, _) = self.guild_avatar_source_in(u, guild);
            self.images.get(&key).cloned()
        });
        let no_avatar = avatar_img.is_none();

        let bullet_row = |text_str: &'static str| {
            div()
                .w_full()
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .bg(super::color("--background-surface-high"))
                .border_1()
                .border_color(super::color("--border-subtle"))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(super::icon("circle_check", 18., "--text-default"))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(super::color("--text-default"))
                        .child(text_str),
                )
        };

        let card = div()
            .id("restricted-profile-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .w(px(440.))
            .rounded(px(12.))
            .border_1()
            .border_color(super::color("--border-subtle"))
            .bg(super::color("--background-base-low"))
            .p(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(16.))
            .when(false, |el| el.child(aria))
            // Close button
            .child(
                div()
                    .id("restricted-profile-close")
                    .absolute()
                    .top(px(14.))
                    .right(px(14.))
                    .size(px(28.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(super::color("--background-mod-subtle")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.full_profile = None;
                        cx.notify();
                    }))
                    .child(super::icon("close", 16., "--interactive-text-default")),
            )
            // Avatar
            .child(
                div()
                    .mt(px(8.))
                    .size(px(80.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(super::color("--background-surface-high"))
                    .border_2()
                    .border_color(super::color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(avatar_img.map(|im| {
                        img(im)
                            .size_full()
                            .rounded_full()
                            .object_fit(gpui::ObjectFit::Cover)
                    }))
                    .when(no_avatar, |el| {
                        el.child(super::icon(
                            "safety_warning",
                            32.,
                            "--text-feedback-warning",
                        ))
                    }),
            )
            // Heading + subtitle
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .text_center()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(26.))
                            .font_weight(FontWeight(600.))
                            .text_color(super::color("--text-strong"))
                            .child(dc_intl::t("b33pLD")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(super::color("--text-default"))
                            .child(subtitle),
                    ),
            )
            // Two reassurance bullets
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(bullet_row(bullet1))
                    .child(bullet_row(bullet2)),
            )
            // Action buttons (`783123` + `254828`)
            .child(
                div()
                    .w_full()
                    .mt(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("restricted-profile-show")
                            .w_full()
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(super::color("--control-secondary-background-default"))
                            .hover(|s| s.bg(super::color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.full_profile.as_mut() {
                                    open.restricted_revealed = true;
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(super::color("--control-secondary-text-default"))
                                    .child(dc_intl::t("UJKH/l")),
                            ),
                    )
                    .when(!blocked, |col| {
                        col.child(
                            div()
                                .id("restricted-profile-dont-show-again")
                                .w_full()
                                .h(px(34.))
                                .rounded(px(8.))
                                .hover(|s| s.bg(super::color("--background-mod-subtle")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.restricted_profile_bypass.insert(user_id);
                                    if let Some(open) = this.full_profile.as_mut() {
                                        open.restricted_revealed = true;
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(super::color("--text-link"))
                                        .child(dc_intl::t("QbcRCJ")),
                                ),
                        )
                    }),
            );

        Some(
            deferred(
                div()
                    .id("restricted-profile-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.full_profile = None;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// `is` of module 148540 (`NonUserBotProfileModal`): webhook / system / `0000` bot profile modal with a single
    /// Bio tab (`ZzAR2Y`), bio text, and Created On date (`A//N4k`).
    pub(super) fn non_user_bot_modal(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.full_profile.as_ref()?;
        let user_id = open.user;
        let guild = open.guild;
        let user = self.stores.users.get(&user_id)?.clone();
        let display_name = user.display_name().to_owned();
        let body = self.fetched.get(&format!("profile:{user_id}"));
        let bio = body
            .and_then(|b| b["user_profile"]["bio"].as_str())
            .unwrap_or_default()
            .to_owned();
        let palette = self.profile_palette(user_id, guild, false);
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let created_el =
            self.full_profile_member_since(user_id, guild, strong, default, subtle, cx);
        let (av_key, _) = self.guild_avatar_source_in(&user, guild);
        let av_img = self.images.get(&av_key).cloned();

        let card = div()
            .id("non-user-bot-profile-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(600.))
            .max_h(px((self.viewport.1 - 32.).max(320.)))
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(super::color("--border-subtle"))
            .bg(super::color("--background-base-low"))
            .flex()
            .flex_col()
            // Banner
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(120.))
                    .bg(super::color("--control-primary-background-default"))
                    .child(
                        div()
                            .id("non-user-bot-close")
                            .absolute()
                            .top(px(12.))
                            .right(px(12.))
                            .size(px(28.))
                            .rounded_full()
                            .bg(rgba(0x00000066))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.full_profile = None;
                                cx.notify();
                            }))
                            .child(super::icon("close", 14., "--text-overlay-light")),
                    ),
            )
            // Avatar + name
            .child(
                div()
                    .px(px(24.))
                    .pt(px(16.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(72.))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(super::color("--background-surface-high"))
                            .border_2()
                            .border_color(super::color("--border-subtle"))
                            .children(av_img.map(|im| {
                                img(im)
                                    .size_full()
                                    .rounded_full()
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
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(super::color("--text-strong"))
                                    .child(display_name),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(super::color("--text-muted"))
                                    .child(user.username.clone()),
                            ),
                    ),
            )
            // Single Bio tab header (`ZzAR2Y`)
            .child(
                div()
                    .mx(px(24.))
                    .border_b_1()
                    .border_color(super::color("--border-subtle"))
                    .flex()
                    .child(
                        div()
                            .pb(px(8.))
                            .border_b_2()
                            .border_color(super::color("--text-strong"))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(super::color("--text-strong"))
                            .child(dc_intl::t("ZzAR2Y")),
                    ),
            )
            // Bio + Created On (`A//N4k`)
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .when(!bio.is_empty(), |col| {
                        col.child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(super::color("--text-default"))
                                .child(bio),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(super::color("--text-strong"))
                                    .child(dc_intl::t("A//N4k")),
                            )
                            .child(created_el),
                    ),
            );

        Some(
            deferred(
                div()
                    .id("non-user-bot-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.full_profile = None;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_restricted_profile(
        &mut self,
        mode: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let uid: Id = 903_573_240_566_399_026;
        self.restricted_profile_bypass.remove(&uid);
        match mode.trim() {
            "ignored" => {
                self.stores.relationships.remove(&uid);
                self.stores.ignored.insert(uid);
            }
            _ => {
                self.stores.ignored.remove(&uid);
                self.stores
                    .relationships
                    .insert(uid, super::blocked::RELATIONSHIP_BLOCKED);
            }
        }
        self.open_full_profile(uid, window, cx);
        format!("restricted-profile: mode={}", mode.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_a_bot_that_is_a_user_gets_the_bot_surfaces() {
        let bot = User {
            bot: true,
            discriminator: "4821".into(),
            ..User::default()
        };
        assert!(app_bot(&bot));
        let webhook = User {
            bot: true,
            discriminator: "0000".into(),
            ..User::default()
        };
        assert!(!app_bot(&webhook));
        let system = User {
            bot: true,
            system: true,
            discriminator: "4821".into(),
            ..User::default()
        };
        assert!(!app_bot(&system));
        assert!(!app_bot(&User::default()));
    }

    #[test]
    fn add_app_follows_the_install_kind() {
        let custom = json!({"id": "9", "custom_install_url": "https://example.com/add"});
        assert!(installable(&custom));
        assert_eq!(add_app_url(&custom), "https://example.com/add");
        assert_eq!(install_glyph(&custom), "link_external_small");
        assert_eq!(
            add_app_press(&custom, false),
            AddAppPress::Link("https://example.com/add".into())
        );
        let on_discord = json!({"id": "9", "custom_install_url": "https://discord.com/oauth2/authorize?client_id=9"});
        assert_eq!(install_glyph(&on_discord), "bot_plus_small");
        let params = json!({"id": "9", "install_params": {"scopes": ["bot", "applications.commands"], "permissions": "8"}});
        assert_eq!(
            add_app_url(&params),
            "https://discord.com/oauth2/authorize?client_id=9&permissions=8&scope=bot+applications.commands"
        );
        assert_eq!(
            add_app_press(&params, false),
            AddAppPress::Authorize(
                json!({"client_id": "9", "scopes": ["bot", "applications.commands"], "permissions": "8"})
            )
        );
        let types = json!({"id": "9", "install_params": {"scopes": ["bot"]}, "integration_types_config": {"1": {"oauth2_install_params": {"scopes": ["applications.commands"]}}}});
        assert_eq!(
            add_app_url(&types),
            "https://discord.com/oauth2/authorize?client_id=9"
        );
        assert_eq!(
            add_app_press(&types, false),
            AddAppPress::Authorize(json!({
                "client_id": "9",
                "scopes": ["applications.commands"],
                "permissions": null,
                "integration_type": "1",
            }))
        );
        // In a DM with a bot it is Open App.
        assert_eq!(add_app_press(&params, true), AddAppPress::OpenApp);
        let none = json!({"id": "9", "integration_types_config": {"0": {}}});
        assert!(!installable(&none));
    }

    #[test]
    fn copy_link_points_at_the_activity_for_embedded_apps() {
        let activity =
            json!({"id": "9", "embedded_surfaces": [0], "install_params": {"scopes": ["bot"]}});
        assert_eq!(
            copy_link_url(&activity, Some(5)),
            "https://discord.com/activities/9?referrer_id=5"
        );
        let plain = json!({"id": "9"});
        assert_eq!(
            copy_link_url(&plain, Some(5)),
            "https://discord.com/oauth2/authorize?client_id=9"
        );
    }

    #[test]
    fn mutual_servers_wait_for_the_answer() {
        assert_eq!(mutual_servers(None), None);
        assert_eq!(mutual_servers(Some(&json!({}))), None);
        assert_eq!(
            mutual_servers(Some(
                &json!({"mutual_guilds": [{"id": "3", "nick": "dj"}, {"id": "x"}]})
            )),
            Some(vec![(3, Some("dj".to_owned()))])
        );
    }
}
