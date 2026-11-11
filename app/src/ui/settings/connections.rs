//! Connected Apps: the account's connections (`GET /users/@me/connections`) and authorized apps
//! (`GET /oauth2/tokens`), each row with the button that starts its removal. Removing opens the confirm dialog first;
//! only that dialog's confirm button sends `DELETE /users/@me/connections/{type}/{id}` or `DELETE /oauth2/tokens/{id}`
//! (module 158935 `Tq`, `TW`, `TK`; the add-connection grid and its OAuth pages are not built).

use super::actions::Dialog;
use super::{button, mask_icon};
use crate::net::Request;
use crate::ui::{Root, color};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use serde_json::Value;

pub(super) const ACCOUNTS: &str = "connections_connected_accounts_setting";
pub(super) const APPS: &str = "authorized_apps_list_setting";
pub(super) const CONNECTIONS_KEY: &str = "settings-connections";
pub(super) const TOKENS_KEY: &str = "settings-oauth-tokens";
pub(super) const CONNECTIONS_ROUTE: &str = "/users/@me/connections";
pub(super) const TOKENS_ROUTE: &str = "/oauth2/tokens";

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
}

/// The accounts of the connections list, skipping revoked ones the way the store's `getAccounts` still lists them but
/// the page filters by support (`isSupported`): a connection without a type or id is left out.
pub(super) fn accounts_of(list: &[Value]) -> Vec<Account> {
    list.iter()
        .filter_map(|item| {
            Some(Account {
                kind: item["type"].as_str()?.to_owned(),
                id: item["id"].as_str()?.to_owned(),
                name: item["name"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

pub(super) fn account_removal(account: &Account) -> Request {
    Request::Rest {
        key: None,
        method: reqwest::Method::DELETE,
        route: format!("/users/@me/connections/{}/{}", account.kind, account.id),
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
        .py(px(48.))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
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
    pub(super) fn connected_accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        // `TJ` renders nothing while `hidePersonalInformation` holds.
        if self.streamer_hides_personal() {
            return div().into_any_element();
        }
        let Some(list) = self.settings_extra.connections.as_ref() else {
            return div().h(px(216.)).into_any_element();
        };
        let accounts = accounts_of(list);
        if accounts.is_empty() {
            // `hh.SG` with `WenGZ2` and `aoLS84`.
            return empty_state(
                "No Connections",
                "Connect your accounts to unlock special Discord integrations",
            );
        }
        let rows: Vec<AnyElement> = accounts
            .into_iter()
            .map(|account| {
                let shown = account.clone();
                card(SharedString::from(format!(
                    "connection-{}-{}",
                    account.kind, account.id
                )))
                .child(
                    div()
                        .size(px(40.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-muted"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(mask_icon("settings_connected_apps", 24., "--icon-strong")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(account.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(color("--text-subtle"))
                                .child(account.kind.clone()),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("disconnect-{}", account.id)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // `apVJux`: the dialog first; the removal is its confirm button.
                            this.settings_extra.dialog = Some(Dialog::Disconnect(shown.clone()));
                            cx.notify();
                        }))
                        .child(button(
                            SharedString::from(format!("disconnect-button-{}", account.id)),
                            "Disconnect",
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
            .child(
                div()
                    .text_size(px(16.))
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
                "No Authorized Apps",
                "Enhance your Discord experience by authorizing and integrating third-party services",
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
                                "Deauthorize",
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
                name: "gabe".into()
            }]
        );
    }

    #[test]
    fn removing_an_account_names_its_type_and_id() {
        let request = account_removal(&Account {
            kind: "github".into(),
            id: "42".into(),
            name: String::new(),
        });
        let Request::Rest { method, route, .. } = request else {
            panic!("a rest request");
        };
        assert_eq!(method, reqwest::Method::DELETE);
        assert_eq!(route, "/users/@me/connections/github/42");
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
