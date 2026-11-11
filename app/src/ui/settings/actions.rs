//! What the buttons, navigator rows and links of the generated pages do, and the switches whose value lives in an
//! account route rather than the settings proto. The pages are drawn from the official DOM (`pages.rs`); this module
//! is where a row's key becomes behaviour:
//!
//! - `Control` rows with `clickable` are the official navigator rows (`AK` in module 158935): they open the settings
//!   page named by their `destinationKey`.
//! - A link in a row's text is either a Help Center article (`getArticleURL`, `eC.A`) or an `openUserSettings` call.
//! - The consent switches of Data & Privacy read `ConsentStore` (module 153488) and write `POST /users/@me/consent`
//!   with `{grant, revoke}` (module 290595); turning one off asks first (`CU.O`, a `VoidConfirmModal`).
//! - Request my data opens the data package dialog (module 970018) and posts `{backends}` to `/users/@me/harvest`
//!   (module 631670 `$I`).
//!
//! A harness run refuses account writes in `net.rs`, so a request here is proved by its unit test and the effect by
//! the frame (the switch keeps the value it was given when the route was refused).

use super::{Open, SECTIONS, anchors_of};
use crate::net::Request;
use crate::ui::{Root, color, modal};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, WeakEntity, div, prelude::*, px,
};
use serde_json::{Value, json};
use std::cell::RefCell;

/// `getArticleURL` (module 975571) builds the Help Center address from the article id.
const HELP: &str = "https://support.discord.com/hc/en-us/articles/";
/// `API_DOCS` (module 652215): `MARKETING_ENDPOINT` is the site root.
const API_DOCS: &str = "https://discord.com/developers/docs/intro";

pub(in crate::ui) const CONSENT_ROUTE: &str = "/users/@me/consent";
pub(in crate::ui) const CONSENT_KEY: &str = "settings-consent";
const CONSENT_WRITE_KEY: &str = "settings-consent-write";
const HARVEST_ROUTE: &str = "/users/@me/harvest";
const HARVEST_KEY: &str = "settings-harvest";
const HARVEST_WRITE_KEY: &str = "settings-harvest-write";

pub(in crate::ui) const USAGE_STATISTICS: &str = "usage_statistics";
pub(in crate::ui) const PERSONALIZATION: &str = "personalization";

/// The data package choices of the Request My Data dialog, in the dialog's order: the `backends` value the route
/// takes, then the label (strings `rfe/x8`, `j+d6RN`, `KO88BS`, `wb7QJ3`, `0dO1t+`, `JN9c36`, `yaLeEB`).
pub(in crate::ui) const HARVEST_TYPES: [(&str, &str); 7] = [
    ("Account", "Account"),
    ("Analytics", "Your Activity"),
    ("Activities", "Activities"),
    ("Ads", "Ads"),
    ("Messages", "Messages"),
    ("Servers", "Servers"),
    ("Zendesk", "Support Tickets"),
];

/// A request with a dialog in front of it, or the dialog that reports one.
#[derive(Clone)]
pub(in crate::ui) enum Dialog {
    Confirm {
        id: &'static str,
        header: &'static str,
        body: &'static str,
        confirm: &'static str,
        cancel: &'static str,
        then: Then,
    },
    /// "Do you want to also apply this change to all your existing servers?" (`uUr+GR` / `yAfu1p`): the default for
    /// new servers changed. No (the confirm button) keeps the servers' own entries, Yes writes the default into all.
    ApplyToServers {
        requests: bool,
        restrict: bool,
    },
    /// `jxXMEz`: pick what the package holds. `missing` shows the error under the list (`W1Rw3D`).
    RequestData {
        checked: [bool; 7],
        missing: bool,
    },
    /// `apVJux`: removing a connected account. Its confirm button is the only thing that sends the removal.
    Disconnect(super::connections::Account),
    /// `ygG62M`: revoking an authorized app's token.
    Deauthorize(super::connections::App),
    /// The first dialog of an Account page button (`Account::kind`): its title and text as the official shows them. The
    /// fields they ask for (password, new value) and the request behind the primary button are not built, so the
    /// primary button only closes it.
    Account(&'static str),
    Alert {
        title: String,
        body: String,
    },
}

/// What a confirmed dialog does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Then {
    Revoke(&'static str),
}

#[derive(Default)]
pub(in crate::ui) struct State {
    pub dialog: Option<Dialog>,
    /// The Account page's field dialogs (`account_dialogs.rs`).
    pub form: Option<super::account_dialogs::Form>,
    /// The Reveal links of the email and phone rows (`N_`, module 158935): the full value while set.
    pub panel: Option<super::account_panels::Panel>,
    pub notif: super::notifications_page::State,
    /// The System page's keybinds (`keybinds.rs`).
    pub kb: super::keybinds::State,
    /// The window is excluded from screen captures now (`streamer.rs`).
    pub content_protected: bool,
    /// How many streamer tools the last scan saw.
    pub streamer_tools: usize,
    pub reveal_email: bool,
    pub reveal_phone: bool,
    /// `GET /users/@me/harvest` (`harvestType`): the last package request, whose `created_at` rate limits the next.
    harvest: Option<Value>,
    harvest_loaded: bool,
    /// The server the Messaging Permissions selector shows; 0 is "All servers".
    pub dm_guild: u64,
    pub activity_sort: super::privacy::Sort,
    /// The last address a link opened (a harness run opens nothing and notes it here and in the log).
    pub last_url: Option<String>,
    /// `GET /users/@me/connections` and `GET /oauth2/tokens`, once they came back.
    pub connections: Option<Vec<Value>>,
    pub tokens: Option<Vec<Value>>,
    connections_asked: bool,
}

impl State {
    /// A dialog of this module is open: it takes the keys and the focus like every modal.
    pub(in crate::ui) fn has_dialog(&self) -> bool {
        self.dialog.is_some() || self.form.is_some()
    }

    /// Escape or an outside press: the dialog goes away and nothing it asked for happens.
    pub(in crate::ui) fn close_dialog(&mut self) {
        self.dialog = None;
        self.form = None;
    }
}

thread_local! {
    /// The row being built, so the links in its text can reach the root (`runs_text` has no context of its own).
    static LINKS: RefCell<Option<(WeakEntity<Root>, &'static str)>> = const { RefCell::new(None) };
}

/// A row of a generated page is about to be built: its links belong to `key`.
pub(super) fn link_owner(root: WeakEntity<Root>, key: &'static str) {
    LINKS.with(|links| *links.borrow_mut() = Some((root, key)));
}

/// The page is done: text built after this has no clickable links.
pub(super) fn link_owner_clear() {
    LINKS.with(|links| *links.borrow_mut() = None);
}

pub(super) fn link_context() -> Option<(WeakEntity<Root>, &'static str)> {
    LINKS.with(|links| links.borrow().clone())
}

/// The Help Center article a "Learn more" link of this row opens. Ids are the `MVz` constants of module 652215.
pub(super) fn article_of(key: &str) -> Option<&'static str> {
    Some(match key {
        // DATA_PRIVACY_CONTROLS
        "data_usage_statistics_setting" => "360004109911",
        // DATA_USED_FOR_RECOMMENDED
        "data_usage_personalization_setting" => "21865322754327",
        // QUESTS_PRIVACY_CONTROLS
        "data_usage_quests_3p_setting" => "22225542459415",
        // DISCOVERABLE_CONTENT_ON_THE_WEB
        "pinot_member_setting" => "44302319454359",
        // GDPR_REQUEST_DATA
        "data_harvest_request_setting" => "360004027692",
        // The Profile Privacy category's own article (module 158935 `CY`).
        "profile_privacy_category" => "38859942749463-Profile-Privacy-Setting-on-Discord",
        // END_TO_END_ENCRYPTION (`C0.Lu`).
        "persistent_verification_codes_setting" => "25968222946071",
        // MESSAGE_REQUESTS
        "permissions_message_requests_setting" | "message_requests_notice_setting" => {
            "7924992471191"
        }
        _ => return None,
    })
}

/// The page and category a navigator row opens (`destinationKey`, module 158935): sidebar label, then the subnav
/// entry to scroll to.
pub(super) fn destination_of(key: &str) -> Option<(&'static str, Option<&'static str>)> {
    Some(match key {
        // `S.X.ACTIVITY_PRIVACY_PANEL`.
        "profile_privacy_to_activity_privacy_navigator"
        | "registered_games_to_activity_privacy_navigator" => ("Activity Privacy", None),
        // `S.X.DATA_AND_PRIVACY_PANEL`.
        "activity_privacy_to_profile_privacy_navigator" => ("Data & Privacy", None),
        // `S.X.APPEARANCE_MESSAGES_CATEGORY`.
        "content_filters_appearance_navigator" => ("Appearance", Some("Messages & Media")),
        // `S.X.ACCESSIBILITY_PANEL` and `S.X.APPEARANCE_PANEL`.
        "appearance_theme_accessibility_navigator" | "appearance_chat_accessibility_navigator" => {
            ("Accessibility", None)
        }
        "accessibility_to_display_navigator" => ("Appearance", None),
        // `S.X.SOUNDS_CATEGORY` (Voice & Video) and `S.X.NOTIFICATIONS_SOUNDS_CATEGORY` (Notifications).
        "notifications_to_voice_and_video_sounds_navigator" => ("Voice & Video", Some("Sounds")),
        "voice_and_video_to_notification_sounds_navigator" => ("Notifications", Some("Sounds")),
        // The authorized apps list lives on Connected Apps.
        "connected_games_authorized_apps_navigator" => ("Connected Apps", Some("Authorized Apps")),
        _ => return None,
    })
}

/// Settings pages reached by `openUserSettings(key)` from the text of a row.
fn link_destination(key: &str, text: &str) -> Option<(&'static str, Option<&'static str>)> {
    match (key, text) {
        // `ACCOUNT_DISABLE_SETTING` and `ACCOUNT_DELETE_SETTING` are the last rows of the Account page.
        ("data_usage_disclaimer_setting", "Disabling" | "Deleting") => {
            Some(("Account", Some("Family Center")))
        }
        _ => None,
    }
}

/// The title, text, primary label and tone of an Account page dialog (`m5or54`, `p3280r`, `SfUuE+`, `8C6t3B`,
/// `jf5GGb` with `gk7h32`, `8lQ2rR` with `FB4H1D`).
fn account_dialog(
    kind: &str,
) -> (
    &'static str,
    Option<&'static str>,
    &'static str,
    modal::Tone,
) {
    match kind {
        "email" => ("Enter an email address", None, "Done", modal::Tone::Primary),
        // `QiGOak`, then the type list (`8lqkf8`): the dialog stops here, nothing is charged or saved.
        "payment" => (
            "Add Payment Method",
            Some("Select Payment Type"),
            "",
            modal::Tone::Primary,
        ),
        "phone" => ("Add a phone number", None, "Send", modal::Tone::Primary),
        "password" => ("Update your password", None, "Done", modal::Tone::Primary),
        "disable" => (
            "Disable Account",
            Some(
                "Are you sure that you want to disable your account? This will immediately log you out and make your account inaccessible to anyone.",
            ),
            "Disable Account",
            modal::Tone::Critical,
        ),
        "delete" => (
            "Delete Account",
            Some(
                "Are you sure that you want to delete your account? This will immediately log you out of your account and you will not be able to log in again.",
            ),
            "Delete Account",
            modal::Tone::Critical,
        ),
        _ => ("Change your username", None, "Done", modal::Tone::Primary),
    }
}

/// What the consent switch shows: the account's answer, or none before it is known.
pub(super) fn consent_of(key: &str) -> Option<&'static str> {
    match key {
        "data_usage_statistics_setting" => Some(USAGE_STATISTICS),
        "data_usage_personalization_setting" => Some(PERSONALIZATION),
        _ => None,
    }
}

/// The body of the consent route: grant what is turned on and revoke what is turned off.
pub(super) fn consent_body(grant: &[&str], revoke: &[&str]) -> Value {
    json!({"grant": grant, "revoke": revoke})
}

/// The request that sets one consent (`Ce.U([name], [])` or `Ce.U([], [name])`).
pub(super) fn consent_request(name: &str, on: bool) -> Request {
    let body = if on {
        consent_body(&[name], &[])
    } else {
        consent_body(&[], &[name])
    };
    Request::Rest {
        key: Some(CONSENT_WRITE_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: CONSENT_ROUTE.to_owned(),
        body: Some(body),
    }
}

/// `{"backends": [...]}` for the checked types, in the dialog's order.
pub(super) fn harvest_body(checked: &[bool; 7]) -> Value {
    let backends: Vec<&str> = HARVEST_TYPES
        .iter()
        .zip(checked)
        .filter(|(_, on)| **on)
        .map(|((value, _), _)| *value)
        .collect();
    json!({"backends": backends})
}

/// The data package may be requested again 30 days after the last request (`Ch`, module 158935: `created_at` plus
/// `2592e6` ms). `None` while it may be requested.
pub(super) fn harvest_next_allowed(harvest: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    let created = chrono::DateTime::parse_from_rfc3339(harvest["created_at"].as_str()?).ok()?;
    let next = created.with_timezone(&chrono::Utc) + chrono::Duration::milliseconds(2_592_000_000);
    (next > chrono::Utc::now()).then_some(next)
}

impl Root {
    /// Settings opened: ask for what the pages list (`initialize` of the data usage category and the harvest row).
    pub(super) fn settings_pages_opened(&mut self) {
        self.settings_extra.panel = None;
        self.notification_page_opened();
        if !self.stores.consents.contains_key(USAGE_STATISTICS) {
            let _ = self.requests.send(Request::Fetch {
                key: CONSENT_KEY.to_owned(),
                route: CONSENT_ROUTE.to_owned(),
            });
        }
        if !self.settings_extra.connections_asked {
            self.settings_extra.connections_asked = true;
            let _ = self.requests.send(Request::Fetch {
                key: super::connections::CONNECTIONS_KEY.to_owned(),
                route: super::connections::CONNECTIONS_ROUTE.to_owned(),
            });
            let _ = self.requests.send(Request::Fetch {
                key: super::connections::TOKENS_KEY.to_owned(),
                route: super::connections::TOKENS_ROUTE.to_owned(),
            });
        }
        if !self.settings_extra.harvest_loaded {
            let _ = self.requests.send(Request::Fetch {
                key: HARVEST_KEY.to_owned(),
                route: HARVEST_ROUTE.to_owned(),
            });
        }
    }

    /// A fetch of this module came back: true when the key was ours.
    pub(in crate::ui) fn settings_fetched(&mut self, key: &str, body: &Value) -> bool {
        if self.notification_page_fetched(key, body) {
            return true;
        }
        match key {
            CONSENT_KEY => {
                self.stores.set_consents(body);
                true
            }
            CONSENT_WRITE_KEY => {
                // `d(r)`: the answer is the new consents object, dispatched as `UPDATE_CONSENTS`.
                if body["ok"].as_bool() == Some(true) {
                    self.stores.set_consents(&body["body"]);
                } else if body["refused"].as_bool() != Some(true) {
                    // A real failure leaves the consent as it was, which `consent_changed` had already moved.
                    let _ = self.requests.send(Request::Fetch {
                        key: CONSENT_KEY.to_owned(),
                        route: CONSENT_ROUTE.to_owned(),
                    });
                }
                true
            }
            super::connections::CONNECTIONS_KEY => {
                self.settings_extra.connections = body.as_array().cloned();
                true
            }
            super::connections::TOKENS_KEY => {
                self.settings_extra.tokens = body.as_array().cloned();
                true
            }
            HARVEST_KEY => {
                self.settings_extra.harvest_loaded = true;
                if body.is_object() {
                    self.settings_extra.harvest = Some(body.clone());
                }
                true
            }
            HARVEST_WRITE_KEY => {
                let ok = body["ok"].as_bool() == Some(true);
                if ok {
                    self.settings_extra.harvest = Some(body["body"].clone());
                }
                // `CA.A.show`: "Request received" or "Oops, something went wrong...".
                self.settings_extra.dialog = Some(if ok {
                    Dialog::Alert {
                        title: "Request received".to_owned(),
                        body: "Our privacy farmers have begun harvesting your data. This can take up to 30 days, but we'll email you when it\u{2019}s done.".to_owned(),
                    }
                } else {
                    Dialog::Alert {
                        title: "Oops, something went wrong...".to_owned(),
                        body: body["body"]["message"]
                            .as_str()
                            .unwrap_or("Please try again")
                            .to_owned(),
                    }
                });
                true
            }
            _ => false,
        }
    }

    /// The value of a switch kept in an account route: the consents. `None` for every other row.
    pub(super) fn action_switch_value(&self, key: &str) -> Option<bool> {
        if let Some(on) = self.notification_switch_value(key) {
            return Some(on);
        }
        if let Some(view) = self.messaging_view(key) {
            return Some(view.on);
        }
        let name = consent_of(key)?;
        self.stores.consents.get(name).copied()
    }

    /// A switch that cannot be pressed in the state the account is in.
    pub(super) fn action_switch_disabled(&self, key: &str) -> bool {
        self.messaging_view(key).is_some_and(|view| view.disabled)
    }

    /// A switch of this module was clicked. True when the key was one of its own.
    pub(super) fn action_switch_changed(
        &mut self,
        key: &'static str,
        on: bool,
        index: usize,
    ) -> bool {
        if self.privacy_changed(key, on, index) {
            return true;
        }
        let Some(name) = consent_of(key) else {
            return false;
        };
        // The switch shows the consent store, not the click.
        self.setting_choices.remove(key);
        if on {
            self.consent_changed(name, true);
        } else {
            self.settings_extra.dialog = Some(match name {
                // `OdPCbN`, `MGWabA`, `D3+rU4`, `kYpG0u`.
                USAGE_STATISTICS => Dialog::Confirm {
                    id: "consent-usage-off",
                    header: "Turn off use data to improve Discord?",
                    body: "If you turn this off, we\u{2019}ll stop processing new data for analytics. You will lose access to new experimental features and usage-based improvements to features like voice chat. If you'd like to further assert your right to object to our processing of your data, email privacy@discord.com.",
                    confirm: "Yes, I'm sure",
                    cancel: "No, take me back!",
                    then: Then::Revoke(USAGE_STATISTICS),
                },
                // `9SNpzv`, `gJvDDh`, `9g5UGw`, `+ZLPw9`.
                _ => Dialog::Confirm {
                    id: "consent-personalization-off",
                    header: "Turn off personalization?",
                    body: "By turning this off you lose access to things like personalized recommendations and suggestions. You won\u{2019}t be able to recover them in the future!",
                    confirm: "Yes, I'm sure",
                    cancel: "No, take me back!",
                    then: Then::Revoke(PERSONALIZATION),
                },
            });
        }
        true
    }

    fn consent_changed(&mut self, name: &str, on: bool) {
        // Shown at once; the route's answer (`UPDATE_CONSENTS`) replaces the store when it comes.
        self.stores.consents.insert(name.to_owned(), on);
        let _ = self.requests.send(consent_request(name, on));
    }

    /// A Control row with `clickable` was pressed: a navigator.
    pub(super) fn settings_navigate(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if let Some((page, category)) = destination_of(key) {
            self.settings_goto(page, category, cx);
        }
    }

    /// A button of a Control row.
    pub(super) fn settings_button_clicked(
        &mut self,
        key: &'static str,
        _index: usize,
        _cx: &mut Context<Self>,
    ) {
        if key == super::notifications_page::UNSUBSCRIBE {
            self.unsubscribe_marketing();
            return;
        }
        if key == "data_harvest_request_setting" && self.harvest_allowed() {
            self.settings_extra.dialog = Some(Dialog::RequestData {
                checked: [false; 7],
                missing: false,
            });
        }
    }

    /// Whether Request Data may be pressed: verified account and no request in the last 30 days (`Ch`).
    pub(super) fn harvest_allowed(&self) -> bool {
        self.stores.account.verified
            && self
                .settings_extra
                .harvest
                .as_ref()
                .is_none_or(|harvest| harvest_next_allowed(harvest).is_none())
    }

    /// A link in the text of a row (`link` is its index among the row's links, `text` its words).
    pub(super) fn settings_link_clicked(
        &mut self,
        key: &'static str,
        text: &'static str,
        cx: &mut Context<Self>,
    ) {
        if let Some((page, category)) = link_destination(key, text) {
            self.settings_goto(page, category, cx);
            return;
        }
        if key == "developer_mode" && text == "Discord API" {
            self.settings_open_url(API_DOCS, cx);
            return;
        }
        if text == "Learn more"
            && let Some(article) = article_of(key)
        {
            self.settings_open_url(&format!("{HELP}{article}"), cx);
        }
    }

    /// `window.open(url, "_blank")`: a harness run notes the address in its log and opens nothing.
    pub(super) fn settings_open_url(&mut self, url: &str, cx: &mut Context<Self>) {
        if crate::storage::is_capture_run() {
            tracing::info!("settings link opens {url}");
            self.settings_extra.last_url = Some(url.to_owned());
            return;
        }
        cx.open_url(url);
    }

    /// Opens the settings page with this sidebar label, at a subnav entry when one is named.
    pub(super) fn settings_goto(
        &mut self,
        label: &str,
        category: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let entry = SECTIONS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .find(|entry| entry.label == label);
        let Some(entry) = entry else {
            return;
        };
        let open = Open::new(entry.page);
        if let Some((index, anchor)) = category.and_then(|category| {
            anchors_of(entry)
                .into_iter()
                .enumerate()
                .find(|(_, anchor)| anchor.label == category)
        }) {
            open.anchor.set(index);
            open.spy.set((0., true));
            open.content.scroll_to_top_of_item(anchor.target);
        }
        self.settings = Some(open);
        cx.notify();
    }

    /// The dialogs of this module, drawn over the settings layer.
    pub(in crate::ui) fn settings_dialogs(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.settings_extra.form.is_some() {
            return self.account_form_dialog(cx);
        }
        let dialog = self.settings_extra.dialog.clone()?;
        let spec = match dialog {
            Dialog::Confirm {
                id,
                header,
                body,
                confirm,
                cancel,
                then,
            } => modal::Spec {
                id,
                title: header.into(),
                subtitle: Some(div().child(body).into_any_element()),
                body: None,
                buttons: vec![
                    modal::button(
                        "settings-confirm-cancel",
                        cancel,
                        modal::Tone::Secondary,
                        |this, _, _| this.settings_extra.dialog = None,
                    ),
                    modal::button(
                        "settings-confirm-ok",
                        confirm,
                        modal::Tone::Primary,
                        move |this, _, _| {
                            this.settings_extra.dialog = None;
                            match then {
                                Then::Revoke(name) => this.consent_changed(name, false),
                            }
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.settings_extra.dialog = None),
            },
            Dialog::Account(kind) => {
                let (title, body, confirm, tone) = account_dialog(kind);
                modal::Spec {
                    id: "settings-account-dialog",
                    title: title.into(),
                    subtitle: body.map(|body| div().child(body).into_any_element()),
                    body: None,
                    buttons: vec![
                        modal::button(
                            "settings-account-cancel",
                            "Cancel",
                            modal::Tone::Secondary,
                            |this, _, _| this.settings_extra.dialog = None,
                        ),
                        modal::button(
                            "settings-account-ok",
                            confirm,
                            tone,
                            |this, _, _| this.settings_extra.dialog = None,
                        ),
                    ]
                    .into_iter()
                    .filter(|button| !confirm.is_empty() || button.id != "settings-account-ok")
                    .collect(),
                    close: Box::new(|this, _, _| this.settings_extra.dialog = None),
                }
            }
            Dialog::Disconnect(account) => modal::Spec {
                id: "settings-disconnect",
                title: format!("Disconnect {}", account.name).into(),
                subtitle: Some(
                    div()
                        .child("Disconnecting your account might remove you from servers you joined via this account.")
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    modal::button(
                        "settings-disconnect-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| this.settings_extra.dialog = None,
                    ),
                    modal::button(
                        "settings-disconnect-ok",
                        "Disconnect",
                        modal::Tone::Critical,
                        move |this, _, _| {
                            this.settings_extra.dialog = None;
                            this.account_removed(&account);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.settings_extra.dialog = None),
            },
            Dialog::Deauthorize(app) => modal::Spec {
                id: "settings-deauthorize",
                title: "Deauthorize Application".into(),
                subtitle: Some(div().child(app.name.clone()).into_any_element()),
                body: None,
                buttons: vec![
                    modal::button(
                        "settings-deauthorize-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| this.settings_extra.dialog = None,
                    ),
                    modal::button(
                        "settings-deauthorize-ok",
                        "Deauthorize",
                        modal::Tone::Critical,
                        move |this, _, _| {
                            this.settings_extra.dialog = None;
                            this.app_removed(&app);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.settings_extra.dialog = None),
            },
            Dialog::ApplyToServers { requests, restrict } => modal::Spec {
                id: "settings-apply-to-servers",
                title: if requests {
                    "Message Requests"
                } else {
                    "Server Privacy"
                }
                .into(),
                subtitle: Some(
                    div()
                        .child(if requests {
                            "Do you want to also apply this change to all your existing servers? You can also disable messages requests on individual servers by visiting that servers setting's page."
                        } else {
                            "Do you want to also apply this change to all your existing servers? You can also disable direct messages on individual servers by visiting that server's settings page."
                        })
                        .into_any_element(),
                ),
                body: None,
                // The confirm button is "No" (only the default changes), the cancel button "Yes" (`gm1Vej`, `p89ACt`).
                buttons: vec![
                    modal::button(
                        "settings-apply-all",
                        "Yes",
                        modal::Tone::Secondary,
                        move |this, _, _| {
                            this.settings_extra.dialog = None;
                            this.apply_dm_default(requests, restrict, true);
                        },
                    ),
                    modal::button(
                        "settings-apply-default",
                        "No",
                        modal::Tone::Primary,
                        move |this, _, _| {
                            this.settings_extra.dialog = None;
                            this.apply_dm_default(requests, restrict, false);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.settings_extra.dialog = None),
            },
            Dialog::Alert { title, body } => modal::Spec {
                id: "settings-alert",
                title: title.into(),
                subtitle: Some(div().child(body).into_any_element()),
                body: None,
                buttons: vec![modal::button(
                    "settings-alert-ok",
                    "Okay",
                    modal::Tone::Primary,
                    |this, _, _| this.settings_extra.dialog = None,
                )],
                close: Box::new(|this, _, _| this.settings_extra.dialog = None),
            },
            Dialog::RequestData { checked, missing } => {
                let rows: Vec<AnyElement> = HARVEST_TYPES
                    .iter()
                    .enumerate()
                    .map(|(index, (_, label))| {
                        div()
                            .id(SharedString::from(format!("harvest-type-{index}")))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .h(px(32.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(Dialog::RequestData { checked, missing }) =
                                    this.settings_extra.dialog.as_mut()
                                {
                                    checked[index] = !checked[index];
                                    if checked.iter().any(|on| *on) {
                                        *missing = false;
                                    }
                                }
                                cx.notify();
                            }))
                            .child(crate::ui::server_games::checkbox(checked[index]))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(*label),
                            )
                            .into_any_element()
                    })
                    .collect();
                modal::Spec {
                    id: "request-data",
                    title: "Submit Data Request".into(),
                    // `fSv59b`.
                    subtitle: Some(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child("Select the data that you'd like to be included in your data package below. You can learn more about each of these selections here.")
                            .child("Note that some selections, like Messages, may delay the delivery of your package, and it may take us up to 30 days to collect your data. We will send you an email to the address you registered with when the package is ready.")
                            .into_any_element(),
                    ),
                    body: Some(
                        div()
                            .px(px(24.))
                            .flex()
                            .flex_col()
                            .children(rows)
                            .when(missing, |this| {
                                this.child(
                                    div()
                                        .pt(px(8.))
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child("Please select at least one data type for your request"),
                                )
                            })
                            .into_any_element(),
                    ),
                    buttons: vec![
                        modal::button(
                            "request-data-cancel",
                            "I've changed my mind",
                            modal::Tone::Secondary,
                            |this, _, _| this.settings_extra.dialog = None,
                        ),
                        modal::button(
                            "request-data-send",
                            "Request My Data",
                            modal::Tone::Primary,
                            |this, _, _| this.request_data_confirmed(),
                        ),
                    ],
                    close: Box::new(|this, _, _| this.settings_extra.dialog = None),
                }
            }
        };
        Some(modal::render(spec, cx))
    }

    /// "Request My Data": nothing checked asks for a choice, else the route is posted and the dialog closes.
    fn request_data_confirmed(&mut self) {
        let Some(Dialog::RequestData { checked, missing }) = self.settings_extra.dialog.as_mut()
        else {
            return;
        };
        if !checked.iter().any(|on| *on) {
            *missing = true;
            return;
        }
        let body = harvest_body(checked);
        self.settings_extra.dialog = None;
        let _ = self.requests.send(Request::Rest {
            key: Some(HARVEST_WRITE_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: HARVEST_ROUTE.to_owned(),
            body: Some(body),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_requests_grant_or_revoke_one_name() {
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = consent_request(USAGE_STATISTICS, true)
        else {
            panic!("a REST request");
        };
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/users/@me/consent");
        assert_eq!(
            body,
            Some(json!({"grant": ["usage_statistics"], "revoke": []}))
        );
        let Request::Rest { body, .. } = consent_request(PERSONALIZATION, false) else {
            panic!("a REST request");
        };
        assert_eq!(
            body,
            Some(json!({"grant": [], "revoke": ["personalization"]}))
        );
    }

    #[test]
    fn the_data_package_asks_for_the_checked_types() {
        let mut checked = [false; 7];
        checked[0] = true;
        checked[4] = true;
        assert_eq!(
            harvest_body(&checked),
            json!({"backends": ["Account", "Messages"]})
        );
        assert_eq!(harvest_body(&[false; 7]), json!({"backends": []}));
    }

    #[test]
    fn a_data_request_blocks_the_next_for_thirty_days() {
        let recent = chrono::Utc::now() - chrono::Duration::days(3);
        let request = json!({"created_at": recent.to_rfc3339()});
        assert!(harvest_next_allowed(&request).is_some());
        let old = chrono::Utc::now() - chrono::Duration::days(31);
        assert!(harvest_next_allowed(&json!({"created_at": old.to_rfc3339()})).is_none());
        assert!(harvest_next_allowed(&Value::Null).is_none());
    }

    #[test]
    fn navigator_rows_name_an_existing_page() {
        let labels: Vec<&str> = SECTIONS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .map(|entry| entry.label)
            .collect();
        for key in [
            "profile_privacy_to_activity_privacy_navigator",
            "activity_privacy_to_profile_privacy_navigator",
            "registered_games_to_activity_privacy_navigator",
            "content_filters_appearance_navigator",
            "connected_games_authorized_apps_navigator",
        ] {
            let (page, _) = destination_of(key).expect(key);
            assert!(labels.contains(&page), "{page}");
        }
        assert!(destination_of("os_open_on_startup").is_none());
    }

    #[test]
    fn learn_more_links_have_their_articles() {
        assert_eq!(
            article_of("data_usage_statistics_setting"),
            Some("360004109911")
        );
        assert!(article_of("developer_mode").is_none());
    }

    #[test]
    fn only_the_two_consent_rows_read_the_consent_store() {
        assert_eq!(
            consent_of("data_usage_statistics_setting"),
            Some("usage_statistics")
        );
        assert_eq!(
            consent_of("data_usage_personalization_setting"),
            Some("personalization")
        );
        assert_eq!(consent_of("data_usage_quests_setting"), None);
    }
}
