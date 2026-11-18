//! Connected Apps: the account's connections (`GET /users/@me/connections`) and authorized apps
//! (`GET /oauth2/tokens`), each row with the button that starts its removal. Removing opens the confirm dialog first;
//! only that dialog's confirm button sends `DELETE /users/@me/connections/{type}/{id}` or `DELETE /oauth2/tokens/{id}`
//! (module 158935 `Tq`, `TW`, `TK`). The "Add a new connection" grid below the header draws the provider buttons; a
//! button opens its authorize URL (`GET /connections/{type}/authorize`). The providers that open an in-app dialog
//! first (Crunchyroll, Xbox, PlayStation, domain, Bluesky) have theirs in `connection_dialogs.rs`.

use super::actions::Dialog;
use super::button;
use crate::net::Request;
use crate::ui::tooltip::{Body, Side, Tip};
use crate::ui::{Root, color};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use serde_json::Value;
use std::sync::Arc;

pub(super) const ACCOUNTS: &str = "connections_connected_accounts_setting";
pub(super) const APPS: &str = "authorized_apps_list_setting";
pub(super) const CONNECTIONS_KEY: &str = "settings-connections";
pub(super) const TOKENS_KEY: &str = "settings-oauth-tokens";
pub(super) const CONNECTIONS_ROUTE: &str = "/users/@me/connections";
pub(super) const TOKENS_ROUTE: &str = "/oauth2/tokens";
pub(super) const ADD: &str = "connections_add_connections_setting";
pub(super) const APPS_KEY: &str = "settings-connection-apps";
pub(super) const AUTHORIZE_KEY: &str = "settings-connection-authorize-";

/// The two applications of the grid's first buttons (module 534952 `Gl`: Riot Games, Battle.net).
const GRID_APPS: [&str; 2] = ["1443033465766281327", "1356665549089800303"];

pub(super) fn apps_route() -> String {
    let query: Vec<String> = GRID_APPS
        .iter()
        .map(|id| format!("application_ids={id}"))
        .collect();
    format!("/applications/public?{}", query.join("&"))
}

/// A platform of module 573648 the grid offers: its type, name, light and dark icon files (`/assets/<file>`), and
/// whether it has `hasMetadata`. Only `enabled` entries without `migrationData` are here (Riot Games, Battle.net and
/// League of Legends are replaced by the application buttons).
const PROVIDERS: [(&str, &str, &str, &str, bool); 20] = [
    (
        "twitch",
        "Twitch",
        "4fda00c96319c8ae.svg",
        "4fda00c96319c8ae.svg",
        false,
    ),
    (
        "youtube",
        "YouTube",
        "0fa530ba9c04ac32.svg",
        "0fa530ba9c04ac32.svg",
        false,
    ),
    (
        "bluesky",
        "Bluesky",
        "2709f058378a099d.svg",
        "2709f058378a099d.svg",
        true,
    ),
    (
        "bungie",
        "Bungie.net",
        "5b0c8422db0e5507.svg",
        "099cd81cf3b4cc98.svg",
        false,
    ),
    (
        "steam",
        "Steam",
        "dbfa31449965dbd5.svg",
        "1f7ec18f3695d4cf.svg",
        true,
    ),
    (
        "reddit",
        "Reddit",
        "adfd927dcc2049a5.svg",
        "adfd927dcc2049a5.svg",
        true,
    ),
    (
        "facebook",
        "Facebook",
        "17be29f77bee4405.svg",
        "17be29f77bee4405.svg",
        false,
    ),
    (
        "twitter",
        "X",
        "5cf814b9693aac47.svg",
        "a61999ae9bfb9658.svg",
        true,
    ),
    (
        "spotify",
        "Spotify",
        "d5719388ffc613da.svg",
        "d5719388ffc613da.svg",
        false,
    ),
    (
        "xbox",
        "Xbox",
        "0133fed7d4dc775c.svg",
        "c4f09fda61827e19.svg",
        false,
    ),
    (
        "github",
        "GitHub",
        "4db64e0649c64e7e.svg",
        "a35ff3e86ffa1eb2.svg",
        false,
    ),
    (
        "playstation",
        "PlayStation Network",
        "792da49081c21457.svg",
        "eed203a7ec517e23.svg",
        false,
    ),
    (
        "epicgames",
        "Epic Games",
        "4fb7893066c0ef83.svg",
        "199eceff4fca1a0c.svg",
        false,
    ),
    (
        "roblox",
        "Roblox",
        "e04a073ed006173a.svg",
        "a4d8e9b0404a2d00.svg",
        false,
    ),
    (
        "paypal",
        "PayPal",
        "dcb64a4ff8f61b2c.svg",
        "dcb64a4ff8f61b2c.svg",
        true,
    ),
    (
        "ebay",
        "eBay",
        "b28a7a265581b6e3.svg",
        "b28a7a265581b6e3.svg",
        true,
    ),
    (
        "letterboxd",
        "Letterboxd",
        "4f8b8634a81979a6.svg",
        "6b6632ec799ccf64.svg",
        true,
    ),
    (
        "crunchyroll",
        "Crunchyroll",
        "94ef3e8b7fa85a2e.svg",
        "94ef3e8b7fa85a2e.svg",
        false,
    ),
    (
        "domain",
        "Domain",
        "10d4972595802406.svg",
        "b4376756bcbbf1ef.svg",
        false,
    ),
    (
        "amazon-music",
        "Amazon Music",
        "e3c4aacc1a54395d.svg",
        "e3c4aacc1a54395d.svg",
        false,
    ),
];

/// `ewM` of module 652215: the platforms the grid lists first among those without metadata.
const LISTED_FIRST: [&str; 5] = [
    "spotify",
    "xbox",
    "playstation",
    "playstation-stg",
    "crunchyroll",
];

/// The grid's order (`gn`, module 643068): `_.sortBy` on [not connected first, metadata first, `ewM` first, name],
/// names compared as plain strings (so "X" comes before "eBay").
pub(super) fn grid_order(connected: &[String]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..PROVIDERS.len()).collect();
    order.sort_by_key(|&index| {
        let (kind, name, _, _, metadata) = PROVIDERS[index];
        (
            connected.iter().any(|have| have == kind),
            !metadata,
            !LISTED_FIRST.contains(&kind),
            name,
        )
    });
    order
}

/// Platforms whose first step is a dialog of its own (module 80320) and not the browser hand-off.
fn opens_a_dialog(kind: &str) -> bool {
    matches!(
        kind,
        "crunchyroll" | "xbox" | "playstation" | "domain" | "bluesky"
    )
}

fn icon_key(kind: &str, dark: bool) -> String {
    format!(
        "connection-icon:{kind}:{}",
        if dark { "dark" } else { "light" }
    )
}

/// `connectionEntrypointUrl` of an application (`connection_entrypoint_url`), when it has one.
fn entrypoint_of(app: &Value) -> Option<String> {
    app["connection_entrypoint_url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
}

fn app_icon_key(id: &str) -> String {
    format!("connection-app-icon:{id}")
}

/// `AioIGb`: "1 connection" or "N connections".
pub(super) fn count_line(count: usize) -> String {
    if count == 1 {
        "1 connection".to_owned()
    } else {
        format!("{count} connections")
    }
}

/// One connected account as a row shows it: its platform, its name and the route that removes it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(in crate::ui) struct Account {
    pub kind: String,
    pub id: String,
    pub name: String,
    /// The platform's name (module 573648), which the Unlink dialog names.
    pub platform: String,
    /// A two-way console link (`Tm.has(type) && twoWayLink`): its authorized application is removed too.
    pub two_way: bool,
}

/// The accounts of the connections list, skipping revoked ones the way the store's `getAccounts` still lists them but
/// the page filters by support (`isSupported`): a connection without a type or id is left out.
pub(super) fn accounts_of(list: &[Value]) -> Vec<Account> {
    list.iter()
        .filter_map(super::connection_card::account_of)
        .collect()
}

pub(super) fn account_removal(account: &Account) -> Request {
    Request::Rest {
        key: None,
        method: reqwest::Method::DELETE,
        route: super::connection_card::connection_route(&account.kind, &account.id),
        body: None,
    }
}

/// An authorized app: the token to revoke, the app's name and what it may do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(in crate::ui) struct App {
    pub token: String,
    pub name: String,
    pub scopes: Vec<String>,
}

pub(super) fn apps_of(list: &[Value]) -> Vec<App> {
    let mut apps: Vec<App> = Vec::new();
    for item in list {
        let (Some(token), Some(name)) = (item["id"].as_str(), item["application"]["name"].as_str())
        else {
            continue;
        };
        apps.push(App {
            token: token.to_owned(),
            name: name.to_owned(),
            scopes: item["scopes"]
                .as_array()
                .map(|scopes| {
                    scopes
                        .iter()
                        .filter_map(|scope| scope.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    apps
}

pub(super) fn app_removal(app: &App) -> Request {
    Request::Rest {
        key: None,
        method: reqwest::Method::DELETE,
        route: format!("/oauth2/tokens/{}", app.token),
        body: None,
    }
}

fn empty_state(title: &'static str, body: &'static str) -> AnyElement {
    div()
        // `.emptyState_yX-kzH`: at least 200px tall, the notice centred in it. The captured box measures 216 with
        // 8px between the title and the line (settings-full/connected-apps-00: title 16px and line 20px lower than
        // a 200px box with a 4px gap draws them, divider 26px lower).
        .min_h(px(216.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(20.))
                .line_height(px(24.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-subtle"))
                .child(title),
        )
        .child(
            div()
                .max_w(px(440.))
                .text_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-subtle"))
                .child(body),
        )
        .into_any_element()
}

fn card(id: SharedString) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(16.))
        .p(px(16.))
        .rounded(px(8.))
        .bg(color("--background-surface-high"))
        .border_1()
        .border_color(color("--border-subtle"))
}

impl Root {
    /// The grid's icons: both theme variants of every platform, as the page picks one by the theme.
    pub(super) fn request_connection_icons(&mut self) {
        for (kind, _, light, dark, _) in PROVIDERS {
            self.request_image(
                icon_key(kind, false),
                format!("https://discord.com/assets/{light}"),
            );
            self.request_image(
                icon_key(kind, true),
                format!("https://discord.com/assets/{dark}"),
            );
        }
    }

    /// The two application buttons show the application's own icon (`getApplicationIconURL`).
    pub(super) fn request_connection_app_icons(&mut self) {
        let apps = self
            .settings_extra
            .connection_apps
            .clone()
            .unwrap_or_default();
        for app in apps {
            if let (Some(id), Some(icon)) = (app["id"].as_str(), app["icon"].as_str()) {
                self.request_image(
                    app_icon_key(id),
                    format!("https://cdn.discordapp.com/app-icons/{id}/{icon}.png?size=64"),
                );
            }
        }
    }

    /// "Add a new connection" (`CONNECTIONS_ADD_CONNECTIONS_SETTING`, module 158935 `TZ`): the two application buttons,
    /// then one button per platform, 44px squares 8px apart.
    pub(super) fn connection_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let dark = !matches!(crate::ui::theme(), dc_tokens::Theme::Light);
        let connected: Vec<String> =
            accounts_of(self.settings_extra.connections.as_deref().unwrap_or(&[]))
                .into_iter()
                .map(|account| account.kind)
                .collect();
        let tile = |id: String, picture: Option<Arc<gpui::Image>>, round: bool| {
            div()
                .id(SharedString::from(id))
                .size(px(44.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .border_1()
                // `.accountButtonInner_yX-kzH`
                .border_color(color("--control-secondary-border-default"))
                .bg(color("--control-secondary-background-default"))
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(color("--control-secondary-background-hover"))
                        .border_color(color("--control-secondary-border-hover"))
                })
                // `.applicationIcon_jn4e52` rounds the application buttons' icons only; a platform's is the
                // background image of its button.
                .children(picture.map(|picture| {
                    img(picture)
                        .size(px(28.))
                        .when(round, |image| image.rounded(px(28.)))
                }))
        };
        let mut tiles: Vec<AnyElement> = Vec::new();
        for id in GRID_APPS {
            let name = self
                .settings_extra
                .connection_apps
                .iter()
                .flatten()
                .find(|app| app["id"].as_str() == Some(id))
                .and_then(|app| app["name"].as_str())
                .unwrap_or_default()
                .to_owned();
            let picture = self.images.get(&app_icon_key(id)).cloned();
            let button = tile(format!("connect-app-{id}"), picture, true)
                .on_click(cx.listener(move |this, _, _, cx| this.connection_app_clicked(id, cx)));
            tiles.push(
                self.with_tip(
                    format!("connect-tip-{id}"),
                    Tip::body(Body::Text(name.into()), Side::Top),
                    button,
                    cx,
                )
                .into_any_element(),
            );
        }
        for index in grid_order(&connected) {
            let (kind, name, ..) = PROVIDERS[index];
            let picture = self.images.get(&icon_key(kind, dark)).cloned();
            let button = tile(format!("connect-{kind}"), picture, false)
                .on_click(cx.listener(move |this, _, _, cx| this.connection_clicked(kind, cx)));
            tiles.push(
                self.with_tip(
                    format!("connect-tip-{kind}"),
                    Tip::body(Body::Text(name.into()), Side::Top),
                    button,
                    cx,
                )
                .into_any_element(),
            );
        }
        div()
            .pt(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Add a new connection")),
            )
            // `.connectionsContainer_yX-kzH`: wrapping, 8px gaps, 8px above and below.
            .child(
                div()
                    .py(px(8.))
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(tiles),
            )
            .into_any_element()
    }

    /// The address the last settings link or connection button would have opened (harness runs open nothing).
    #[cfg(feature = "capture")]
    pub(crate) fn settings_last_url(&self) -> Option<String> {
        self.settings_extra.last_url.clone()
    }

    /// A platform button (module 289498): `GET /connections/{type}/authorize` and the address it answers opens in the
    /// browser (`738104`). Five platforms start with a dialog of their own first (`80320`, `connection_dialogs.rs`).
    pub(super) fn connection_clicked(&mut self, kind: &'static str, cx: &mut Context<Self>) {
        if opens_a_dialog(kind) {
            self.open_connection_dialog(kind, cx);
            return;
        }
        self.connection_authorize(kind, cx);
    }

    /// An application button (module 672130): `startAuthorization` of module 724302. Its web flow opens the
    /// application's `connectionEntrypointUrl` through the masked-link check (`h` of module 889558); the RPC flow needs
    /// the game connected over local RPC, which never happens here. Without an entry point the button is disabled.
    fn connection_app_clicked(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let app = self
            .settings_extra
            .connection_apps
            .iter()
            .flatten()
            .find(|app| app["id"].as_str() == Some(id));
        let Some(url) = app.and_then(entrypoint_of) else {
            return;
        };
        let name = app
            .and_then(|app| app["name"].as_str())
            .unwrap_or_default()
            .to_owned();
        if crate::storage::is_capture_run() {
            tracing::info!("connection application {id}: opens {url}");
            self.settings_extra.last_url = Some(url);
            cx.notify();
            return;
        }
        self.follow_link(&url, &name, cx);
    }

    pub(in crate::ui) fn connection_authorize(
        &mut self,
        kind: &'static str,
        cx: &mut Context<Self>,
    ) {
        let route = format!("/connections/{kind}/authorize");
        if crate::storage::is_capture_run() {
            // The route creates an authorization state at the server; a harness run never starts one.
            tracing::info!("connection {kind}: hand-off skipped, GET {route} then the browser");
            self.settings_extra.last_url = Some(route);
            cx.notify();
            return;
        }
        let _ = self.requests.send(Request::Fetch {
            key: format!("{AUTHORIZE_KEY}{kind}"),
            route,
        });
    }

    /// The authorize answer: the address to open (`i.A.authorize` returns `body.url`).
    pub(in crate::ui) fn connection_authorize_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.gift_answered(key, body, cx) {
            return true;
        }
        if key == super::connection_dialogs::DOMAIN_KEY {
            self.connection_domain_answered(body, cx);
            return true;
        }
        if key.starts_with(super::connection_card::UPDATE_KEY)
            || key.starts_with(super::connection_card::CALLBACK_KEY)
        {
            // The server answers with `USER_CONNECTIONS_UPDATE`, which reads the list again.
            return true;
        }
        if let Some(id) = key.strip_prefix(super::connection_card::REFRESH_KEY) {
            self.connection_refresh_answered(id.to_owned(), cx);
            return true;
        }
        if let Some(id) = key.strip_prefix(super::connection_card::JOIN_KEY) {
            self.connection_join_answered(id, body);
            cx.notify();
            return true;
        }
        if !key.starts_with(AUTHORIZE_KEY) {
            return false;
        }
        let url = body["url"].as_str();
        if let Some(url) = url {
            self.settings_open_url(url, cx);
        }
        self.connection_authorized(url.is_some(), cx);
        true
    }

    pub(super) fn connected_accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        // `TJ` renders nothing while `hidePersonalInformation` holds.
        if self.streamer_hides_personal() {
            return div().into_any_element();
        }
        let Some(list) = self.settings_extra.connections.as_ref() else {
            return div().h(px(216.)).into_any_element();
        };
        let dark = !matches!(crate::ui::theme(), dc_tokens::Theme::Light);
        // `Tq`: only supported platforms (`isSupported`), one `TW` card each.
        let rows: Vec<AnyElement> = list
            .iter()
            .filter_map(|item| self.connection_card(item, dark, cx))
            .collect();
        if rows.is_empty() {
            // `hh.SG` with `WenGZ2` and `aoLS84`.
            // 10px more room under the grid than the apps list has under its description (captured positions).
            return div()
                .mt(px(10.))
                .child(empty_state(
                    dc_intl::tr("No Connections"),
                    dc_intl::tr("Connect your accounts to unlock special Discord integrations"),
                ))
                .into_any_element();
        }
        // `connectionList_yX-kzH`: a grid 24px apart, the count heading first.
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(count_line(rows.len())),
            )
            .children(rows)
            .into_any_element()
    }

    pub(super) fn authorized_apps(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(list) = self.settings_extra.tokens.as_ref() else {
            return div().h(px(216.)).into_any_element();
        };
        let apps = apps_of(list);
        if apps.is_empty() {
            return empty_state(
                dc_intl::tr("No Authorized Apps"),
                dc_intl::tr(
                    "Enhance your Discord experience by authorizing and integrating third-party services",
                ),
            );
        }
        let rows: Vec<AnyElement> = apps
            .into_iter()
            .map(|app| {
                let shown = app.clone();
                card(SharedString::from(format!("app-{}", app.token)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(app.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .text_color(color("--text-subtle"))
                                    .child(app.scopes.join(", ")),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("deauthorize-{}", app.token)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.settings_extra.dialog =
                                    Some(Dialog::Deauthorize(shown.clone()));
                                cx.notify();
                            }))
                            .child(button(
                                SharedString::from(format!("deauthorize-button-{}", app.token)),
                                dc_intl::tr("Deauthorize"),
                                super::model::ButtonKind::CriticalSecondary,
                            )),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(rows)
            .into_any_element()
    }

    /// The confirm buttons of the two dialogs: the removal goes out and the list drops the row at once.
    pub(super) fn account_removed(&mut self, account: &Account) {
        let _ = self.requests.send(account_removal(account));
        if let Some(list) = self.settings_extra.connections.as_mut() {
            list.retain(|item| item["id"].as_str() != Some(account.id.as_str()));
        }
    }

    pub(super) fn app_removed(&mut self, app: &App) {
        let _ = self.requests.send(app_removal(app));
        if let Some(list) = self.settings_extra.tokens.as_mut() {
            list.retain(|item| item["id"].as_str() != Some(app.token.as_str()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_application_button_opens_its_entry_point_only() {
        let riot =
            json!({"id": GRID_APPS[0], "connection_entrypoint_url": "https://example.com/link"});
        assert_eq!(
            entrypoint_of(&riot).as_deref(),
            Some("https://example.com/link")
        );
        assert_eq!(entrypoint_of(&json!({"id": GRID_APPS[1]})), None);
        assert_eq!(
            entrypoint_of(&json!({"connection_entrypoint_url": ""})),
            None
        );
    }

    #[test]
    fn accounts_keep_type_id_and_name() {
        let list = vec![
            json!({"type": "steam", "id": "7", "name": "gabe", "revoked": false}),
            json!({"name": "no type"}),
        ];
        assert_eq!(
            accounts_of(&list),
            vec![Account {
                kind: "steam".into(),
                id: "7".into(),
                name: "gabe".into(),
                platform: "Steam".into(),
                two_way: false,
            }]
        );
    }

    #[test]
    fn removing_an_account_names_its_type_and_id() {
        let request = account_removal(&Account {
            kind: "bluesky".into(),
            id: "did:plc:42".into(),
            name: String::new(),
            platform: "Bluesky".into(),
            two_way: false,
        });
        let Request::Rest { method, route, .. } = request else {
            panic!("a rest request");
        };
        assert_eq!(method, reqwest::Method::DELETE);
        assert_eq!(route, "/users/@me/connections/bluesky/did%3Aplc%3A42");
    }

    #[test]
    fn authorized_apps_carry_their_token_and_scopes() {
        let list = vec![json!({
            "id": "99",
            "application": {"name": "Some Bot"},
            "scopes": ["identify", "guilds"]
        })];
        let apps = apps_of(&list);
        assert_eq!(apps[0].token, "99");
        assert_eq!(apps[0].scopes, vec!["identify", "guilds"]);
        let Request::Rest { route, .. } = app_removal(&apps[0]) else {
            panic!("a rest request");
        };
        assert_eq!(route, "/oauth2/tokens/99");
    }

    #[test]
    fn the_count_line_is_singular_for_one() {
        assert_eq!(count_line(1), "1 connection");
        assert_eq!(count_line(3), "3 connections");
    }
}
