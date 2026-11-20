//! The connected-account card of Settings > Connections (module 158935 `TW`, its reconnect upsells `TY`/`TM`/`TU`/`Tw`
//! and its server rows `TH`) and the requests of module 77468 its controls send.
//!
//! Every switch keeps its own state at once and sends `PATCH /users/@me/connections/{type}/{escape(id)}` with the
//! official body (`visibility`, `metadata_visibility`, `show_activity`, `friend_sync`, all booleans); the server's
//! `USER_CONNECTIONS_UPDATE` that follows reads the list again, so what is on screen after it (and after a restart)
//! is the server's. Turning on Display on profile or Display details for an unverified account sends nothing and
//! starts the platform's reconnect (module 80320) instead; the choice is sent once the account comes back verified.
//! GAP: the Xbox Game Pass perk upsells (`Tk` of `TY`, `TS` partner footer, `Tj` banner) need the Game Pass
//! experiments (`555837`, `631368`) and are not drawn.

use super::actions::Dialog;
use super::connections::Account;
use super::{mask_icon, new_badge};
use crate::net::Request;
use crate::ui::full_profile::{self, ConnectionMeta};
use crate::ui::platform_presence;
use crate::ui::{Root, color};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use serde_json::{Value, json};
use std::time::Duration;

pub(super) const UPDATE_KEY: &str = "settings-connection-update:";
pub(super) const REFRESH_KEY: &str = "settings-connection-refresh:";
pub(super) const CALLBACK_KEY: &str = "settings-connection-callback:";
pub(super) const JOIN_KEY: &str = "settings-connection-join:";

/// `ewM` of module 652215: the platforms whose card offers "Display {platform} as your status".
pub(super) const SHOWS_ACTIVITY: [&str; 5] = [
    "spotify",
    "xbox",
    "playstation",
    "playstation-stg",
    "crunchyroll",
];
/// `txh` of module 652215: the platforms whose card offers "Sync friends to Discord".
pub(super) const FRIEND_SYNC: [&str; 2] = ["facebook", "contacts"];
/// `Tm` of module 158935: the console links whose unlink warns about the authorized application it also removes.
const TWO_WAY: [&str; 4] = ["xbox", "playstation", "playstation-stg", "crunchyroll"];
/// `hasMetadata` of module 573648.
pub(super) const HAS_METADATA: [&str; 10] = [
    "bluesky",
    "steam",
    "reddit",
    "twitter_legacy",
    "twitter",
    "paypal",
    "ebay",
    "tiktok",
    "letterboxd",
    "mastodon",
];

/// The switches of the card, in their order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Toggle {
    /// "Display on profile" (`f7yOAX`).
    Visibility,
    /// "Display details on profile" (`FYKGsL`).
    Details,
    /// "Display {platform} as your status" (`6u6J0q`).
    Activity,
    /// "Sync friends to Discord" (`+KCMSi`).
    FriendSync,
}

impl Toggle {
    /// The body field of `setVisibility`, `setMetadataVisibility`, `setShowActivity` and `setFriendSync`.
    fn field(self) -> &'static str {
        match self {
            Toggle::Visibility => "visibility",
            Toggle::Details => "metadata_visibility",
            Toggle::Activity => "show_activity",
            Toggle::FriendSync => "friend_sync",
        }
    }
}

/// `escape()` of the route builders (`CONNECTION`, `CONNECTION_REFRESH` of module 652215): letters, digits and
/// `@*_+-./` stay, every other byte below 256 becomes `%XX` and every other code unit `%uXXXX`.
pub(super) fn js_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for unit in text.encode_utf16() {
        match unit {
            0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A => out.push(char::from(unit as u8)),
            _ if b"@*_+-./".contains(&(unit as u8)) && unit < 0x80 => {
                out.push(char::from(unit as u8))
            }
            0..=0xFF => out.push_str(&format!("%{unit:02X}")),
            _ => out.push_str(&format!("%u{unit:04X}")),
        }
    }
    out
}

/// `CONNECTION(type, id)` of module 652215.
pub(super) fn connection_route(kind: &str, id: &str) -> String {
    format!("/users/@me/connections/{kind}/{}", js_escape(id))
}

/// `update` of module 77468 for one switch.
pub(super) fn update_request(kind: &str, id: &str, toggle: Toggle, on: bool) -> Request {
    Request::Rest {
        key: Some(format!("{UPDATE_KEY}{kind}:{id}")),
        method: reqwest::Method::PATCH,
        route: connection_route(kind, id),
        body: Some(json!({ toggle.field(): on })),
    }
}

/// `refresh` of module 77468: `POST CONNECTION_REFRESH`.
pub(super) fn refresh_request(kind: &str, id: &str) -> Request {
    Request::Rest {
        key: Some(format!("{REFRESH_KEY}{id}")),
        method: reqwest::Method::POST,
        route: format!("{}/refresh", connection_route(kind, id)),
        body: None,
    }
}

/// Module 306677 for the gateway's `USER_CONNECTIONS_CALLBACK` (ConnectedAccountsStore, module 417869): the browser
/// finished the platform's sign-in and the code comes back here to finish the link.
pub(super) fn callback_request(provider: &str, data: &Value) -> Request {
    let mut body = json!({
        "code": data["code"],
        "state": data["state"],
        "insecure": false,
        "friend_sync": FRIEND_SYNC.contains(&provider),
    });
    if !data["openid_params"].is_null() {
        body["openid_params"] = data["openid_params"].clone();
    }
    Request::Rest {
        key: Some(format!("{CALLBACK_KEY}{provider}")),
        method: reqwest::Method::POST,
        route: format!("/connections/{provider}/callback"),
        body: Some(body),
    }
}

/// `joinServer` of module 77468: `POST /integrations/{id}/join`.
pub(super) fn join_request(integration: &str) -> Request {
    Request::Rest {
        key: Some(format!("{JOIN_KEY}{integration}")),
        method: reqwest::Method::POST,
        route: format!("/integrations/{integration}/join"),
        body: None,
    }
}

/// `PLATFORM_TWITTER` of module 646514 (`unsafe_rawColors`, no CSS variable).
const TWITTER_BLUE: u32 = 0x1da1f2;

/// The card's Verified on Twitter condition (module 158935): only an X account (`twitter`) whose metadata `verified`
/// (`pK.TWITTER_VERIFIED`) is the string `"1"`; Bluesky and Mastodon take the mark only on the profile.
pub(super) fn twitter_verified(kind: &str, metadata: &Value) -> bool {
    kind == "twitter" && metadata["verified"].as_str() == Some("1")
}

/// The record's flags arrive as `0`/`1` (`visibility`, `metadata_visibility`) or booleans.
pub(super) fn flag(value: &Value) -> bool {
    value
        .as_bool()
        .unwrap_or_else(|| value.as_i64().is_some_and(|n| n == 1))
}

/// What the card shows of one switch, read from the account record.
pub(super) fn toggle_state(item: &Value, toggle: Toggle) -> bool {
    flag(&item[toggle.field()])
}

/// The switches a card has, in its order: Display on profile, then details (`hasMetadata`), status (`ewM`) and
/// friend sync (`txh`).
pub(super) fn toggles_of(kind: &str) -> Vec<Toggle> {
    let mut out = vec![Toggle::Visibility];
    if HAS_METADATA.contains(&kind) {
        out.push(Toggle::Details);
    }
    if SHOWS_ACTIVITY.contains(&kind) {
        out.push(Toggle::Activity);
    }
    if FRIEND_SYNC.contains(&kind) {
        out.push(Toggle::FriendSync);
    }
    out
}

/// Details can be turned on only while the account shows on the profile and has metadata.
pub(super) fn toggle_disabled(item: &Value, toggle: Toggle) -> bool {
    toggle == Toggle::Details && (!flag(&item["visibility"]) || item["metadata"].is_null())
}

/// The platform's name and its light and dark icons (module 573648), for any supported type.
pub(super) fn platform_info(kind: &str) -> Option<(&'static str, &'static str, &'static str)> {
    full_profile::CONNECTED_PLATFORMS
        .iter()
        .find(|(known, ..)| *known == kind)
        .map(|&(_, name, _, light, dark)| (name, light, dark))
}

/// The record as the confirm dialog needs it.
pub(super) fn account_of(item: &Value) -> Option<Account> {
    let kind = item["type"].as_str()?;
    Some(Account {
        kind: kind.to_owned(),
        id: item["id"].as_str()?.to_owned(),
        name: item["name"].as_str().unwrap_or_default().to_owned(),
        platform: platform_info(kind)
            .map_or(kind, |(name, ..)| name)
            .to_owned(),
        two_way: TWO_WAY.contains(&kind) && flag(&item["two_way_link"]),
    })
}

/// `sm` text button (module 821609): 32 tall, `body-sm/medium`, 11px inside a 1px border, at least 60 wide.
fn sm_button(
    id: SharedString,
    label: String,
    primary: bool,
    disabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let family = if primary { "primary" } else { "secondary" };
    let token = |part: &str, state: &str| color(&format!("--control-{family}-{part}-{state}"));
    div()
        .id(id)
        .flex_none()
        .h(px(32.))
        .min_w(px(60.))
        .px(px(11.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .border_1()
        .border_color(token("border", "default"))
        .bg(token("background", "default"))
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(token("text", "default"))
        .when(disabled, |this| this.opacity(0.5))
        .when(!disabled, |this| {
            this.cursor_pointer().hover(|style| {
                style
                    .bg(token("background", "hover"))
                    .border_color(token("border", "hover"))
            })
        })
        .child(label)
}

fn divider() -> AnyElement {
    // `divider_ZaYWLb`: a thin `--border-subtle` rule.
    div()
        .w_full()
        .h(px(1.))
        .bg(color("--border-subtle"))
        .into_any_element()
}

impl Root {
    /// The kinds of platform the account has linked (ConnectedAccountsStore `getAccounts`): what the "Connect Xbox"
    /// offers of the activity cards look at.
    pub(in crate::ui) fn linked_platforms(&self) -> Vec<&str> {
        self.settings_extra
            .connections
            .iter()
            .flatten()
            .filter_map(|item| item["type"].as_str())
            .collect()
    }

    /// ConnectedAccountsStore's gateway events (module 417869): `READY` brings the accounts (`CONNECTION_OPEN`), and
    /// `USER_CONNECTIONS_CALLBACK` carries the code of a sign-in finished in the browser, which goes to
    /// `POST /connections/{provider}/callback` (module 306677) to finish the link.
    pub(in crate::ui) fn connection_gateway(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => {
                // `CONNECTION_OPEN` replaces the accounts (another account signed in has its own).
                if let Some(list) = data["connected_accounts"].as_array() {
                    self.settings_extra.connections = Some(list.clone());
                }
            }
            "USER_CONNECTIONS_CALLBACK" => {
                let Some(provider) = data["provider"].as_str() else {
                    return;
                };
                if crate::storage::is_capture_run() {
                    // The code would finish a link at the server; a harness run never sends it.
                    tracing::info!(
                        "connection {provider}: callback POST not sent in a harness run"
                    );
                    self.settings_extra.last_url =
                        Some(format!("POST /connections/{provider}/callback"));
                    return;
                }
                let _ = self.requests.send(callback_request(provider, data));
            }
            _ => {}
        }
    }

    /// The platform icons of the cards (`icon.lightSVG` / `icon.darkSVG`), once the list is known.
    pub(super) fn request_connection_card_icons(&mut self) {
        let files: Vec<&'static str> = self
            .settings_extra
            .connections
            .iter()
            .flatten()
            .filter_map(|item| platform_info(item["type"].as_str()?))
            .flat_map(|(_, light, dark)| [light, dark])
            .collect();
        let kinds: Vec<String> = self
            .linked_platforms()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let upsells = kinds.iter().filter_map(|kind| match kind.as_str() {
            // `TV` and the PlayStation art of `TU`.
            "xbox" => Some("9df988a227916145.png"),
            "playstation" => Some("88954903b6a5b9cc.svg"),
            _ => None,
        });
        for file in files.into_iter().chain(upsells.collect::<Vec<_>>()) {
            self.request_image(
                platform_presence::asset_key(file),
                platform_presence::asset_url(file),
            );
        }
        let guild_icons: Vec<String> = self
            .settings_extra
            .connections
            .iter()
            .flatten()
            .flat_map(|item| item["integrations"].as_array().into_iter().flatten())
            .filter_map(|integration| {
                let guild = &integration["guild"];
                Some(format!(
                    "https://cdn.discordapp.com/icons/{}/{}.png?size=64",
                    guild["id"].as_str()?,
                    guild["icon"].as_str()?
                ))
            })
            .collect();
        for url in guild_icons {
            self.request_image(url.clone(), url);
        }
        // The Steam item pictures of the metadata box (`A` of module 169869).
        let items: Vec<&'static str> = self
            .settings_extra
            .connections
            .iter()
            .flatten()
            .flat_map(|item| {
                full_profile::connection_metadata(
                    item["type"].as_str().unwrap_or_default(),
                    &item["metadata"],
                )
            })
            .filter_map(|entry| match entry {
                ConnectionMeta::Item { icon, .. } => Some(icon),
                _ => None,
            })
            .collect();
        for file in items {
            self.request_image(
                full_profile::connection_icon_key(file),
                format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN),
            );
        }
    }

    /// `Tg.A({platformType})` of the card: the platform's own connect flow again (module 80320).
    fn connection_reconnect(&mut self, kind: &str, cx: &mut Context<Self>) {
        if let Some(platform) = platform_presence::platform_of(kind) {
            self.connection_clicked(platform.kind, cx);
        }
    }

    /// A switch of a card was flipped.
    pub(super) fn connection_toggled(
        &mut self,
        kind: &str,
        id: &str,
        toggle: Toggle,
        on: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self
            .settings_extra
            .connections
            .iter_mut()
            .flatten()
            .find(|item| item["id"].as_str() == Some(id) && item["type"].as_str() == Some(kind))
        else {
            return;
        };
        let verified = flag(&item["verified"]);
        if on && !verified && matches!(toggle, Toggle::Visibility | Toggle::Details) {
            // `N(n)` / `C(n)` and the reconnect: sent once the account is back verified.
            self.settings_extra
                .conn_pending
                .push((id.to_owned(), toggle));
            self.connection_reconnect(kind, cx);
            cx.notify();
            return;
        }
        item[toggle.field()] = match toggle {
            Toggle::Visibility | Toggle::Details => json!(u8::from(on)),
            Toggle::Activity | Toggle::FriendSync => json!(on),
        };
        let _ = self.requests.send(update_request(kind, id, toggle, on));
        if toggle == Toggle::Activity {
            // `setShowActivity` then `USER_CONNECTION_UPDATE`: the Spotify store stops (or starts) relaying.
            self.stores.spotify.dispatch(
                "USER_CONNECTION_UPDATE",
                &json!({"platformType": kind, "id": id, "showActivity": on}),
                chrono::Utc::now().timestamp_millis(),
            );
            if kind == "spotify"
                && let Some(list) = self.settings_extra.connections.as_ref()
            {
                let _ = self
                    .requests
                    .send(Request::Spotify(crate::spotify::Command::Accounts(
                        crate::spotify::linked_accounts(&Value::Array(list.clone())),
                    )));
            }
        }
        cx.notify();
    }

    /// The list came back: choices held for a then unverified account go out now if it is verified.
    pub(super) fn connection_pending_apply(&mut self) {
        let pending = std::mem::take(&mut self.settings_extra.conn_pending);
        for (id, toggle) in pending {
            let item = self
                .settings_extra
                .connections
                .iter_mut()
                .flatten()
                .find(|item| item["id"].as_str() == Some(id.as_str()));
            let Some(item) = item else {
                continue;
            };
            if item["verified"] == false {
                self.settings_extra.conn_pending.push((id, toggle));
                continue;
            }
            let Some(kind) = item["type"].as_str().map(str::to_owned) else {
                continue;
            };
            item[toggle.field()] = json!(1);
            let _ = self.requests.send(update_request(&kind, &id, toggle, true));
        }
    }

    fn connection_refresh_clicked(&mut self, kind: &str, id: &str, cx: &mut Context<Self>) {
        if self.settings_extra.conn_refresh.contains_key(id) {
            return;
        }
        self.settings_extra
            .conn_refresh
            .insert(id.to_owned(), false);
        let _ = self.requests.send(refresh_request(kind, id));
        cx.notify();
    }

    /// The refresh ended (either way): two seconds later the button reads Done and stays disabled.
    pub(super) fn connection_refresh_answered(&mut self, id: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                this.settings_extra.conn_refresh.insert(id, true);
                cx.notify();
            });
        })
        .detach();
    }

    fn connection_join_clicked(&mut self, integration: &str, cx: &mut Context<Self>) {
        self.settings_extra
            .conn_joining
            .insert(integration.to_owned(), None);
        let _ = self.requests.send(join_request(integration));
        cx.notify();
    }

    /// `USER_CONNECTIONS_INTEGRATION_JOINING(_ERROR)`: the request ended; a failure keeps its message under the row.
    pub(super) fn connection_join_answered(&mut self, integration: &str, body: &Value) {
        if body["ok"].as_bool() == Some(true) {
            self.settings_extra.conn_joining.remove(integration);
        } else {
            let message = body["body"]["message"].as_str().unwrap_or_default();
            self.settings_extra.conn_joining.insert(
                integration.to_owned(),
                Some(if message.is_empty() {
                    dc_intl::t("j2d6Km").to_owned()
                } else {
                    message.to_owned()
                }),
            );
        }
    }

    /// One switch of a card (`t4.d`, a horizontal form field whose label also flips it).
    fn connection_switch(
        &self,
        item: &Value,
        toggle: Toggle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind = item["type"].as_str().unwrap_or_default().to_owned();
        let id = item["id"].as_str().unwrap_or_default().to_owned();
        let on = toggle_state(item, toggle);
        let disabled = toggle_disabled(item, toggle);
        let label = match toggle {
            Toggle::Visibility => dc_intl::t("f7yOAX").to_owned(),
            Toggle::Details => dc_intl::t("FYKGsL").to_owned(),
            Toggle::Activity => dc_intl::format(
                "6u6J0q",
                &[(
                    "platform",
                    platform_info(&kind)
                        .map_or(kind.as_str(), |(name, ..)| name)
                        .into(),
                )],
            ),
            Toggle::FriendSync => dc_intl::t("+KCMSi").to_owned(),
        };
        let key = format!("{}-{kind}-{id}", toggle.field());
        let (label_kind, label_id) = (kind.clone(), id.clone());
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .id(SharedString::from(format!("connection-label-{key}")))
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .when(disabled, |this| this.opacity(0.5))
                    .when(!disabled, |this| {
                        this.cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.connection_toggled(&label_kind, &label_id, toggle, !on, cx);
                            }))
                    })
                    .child(label),
            )
            .child(super::render::plain_switch_with(
                SharedString::from(format!("connection-switch-{key}")),
                on,
                disabled,
                move |this, now, cx| this.connection_toggled(&kind, &id, toggle, now, cx),
                cx,
            ))
            .into_any_element()
    }

    /// The metadata box (`metadataContainer_yX-kzH`): the platform's details and Member since, or the invitation to
    /// add details, and the Refresh / Add details / Done button.
    fn connection_metadata_box(&self, item: &Value, cx: &mut Context<Self>) -> Option<AnyElement> {
        let kind = item["type"].as_str().unwrap_or_default().to_owned();
        let id = item["id"].as_str().unwrap_or_default().to_owned();
        let metadata = &item["metadata"];
        let mut entries: Vec<AnyElement> = full_profile::connection_metadata(&kind, metadata)
            .into_iter()
            .map(|entry| {
                // `connectedAccountVanityMetadata_VlBwYX` in `metadataItem_yX-kzH`: footnote text, 8px apart.
                let base = div()
                    .flex_none()
                    .mr(px(8.))
                    .text_size(px(10.))
                    .line_height(px(12.))
                    .text_color(color("--text-default"));
                match entry {
                    ConnectionMeta::Text(text) => base.child(text),
                    ConnectionMeta::Tag { label, paypal } => base
                        .rounded(px(12.))
                        .px(px(8.))
                        .py(px(1.))
                        .map(|this| {
                            if paypal {
                                this.bg(color("--status-positive-background"))
                                    .text_color(color("--white"))
                            } else {
                                this.bg(color("--background-mod-subtle"))
                            }
                        })
                        .child(label),
                    ConnectionMeta::Item { label, icon, .. } => base
                        .flex()
                        .items_center()
                        .children(
                            self.images
                                .get(&full_profile::connection_icon_key(icon))
                                .cloned()
                                .map(|picture| img(picture).size(px(18.)).mr(px(4.))),
                        )
                        .child(label),
                }
                .into_any_element()
            })
            .collect();
        if let Some(date) = metadata["created_at"]
            .as_str()
            .and_then(full_profile::profile_date)
        {
            // `connectedAccountVanityMetadataCreatedAt_yX-kzH`: body-xs in `text-subtle`.
            entries.push(
                div()
                    .flex_none()
                    .mr(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-subtle"))
                    .child(dc_intl::t!("9rfonh", date = date))
                    .into_any_element(),
            );
        }
        let refreshed = self.settings_extra.conn_refresh.get(&id).copied();
        let mut label = dc_intl::t("wzzjk9").to_owned();
        if entries.is_empty() {
            if !HAS_METADATA.contains(&kind.as_str()) {
                return None;
            }
            // `connectionMetadataUpsellTag_yX-kzH` (NEW) and `Up2ni7` with its Learn more link.
            entries.push(div().flex_none().child(new_badge()).into_any_element());
            entries.push(
                div()
                    .px(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .child(self.text_with_link(
                        "connection-details-upsell",
                        "Add connection details to your profile! ",
                        "Learn more",
                        "",
                        cx,
                        |this, cx| {
                            this.settings_open_url(
                                "https://support.discord.com/hc/en-us/articles/8063233404823",
                                cx,
                            )
                        },
                    ))
                    .into_any_element(),
            );
            label = dc_intl::t("LVh3/5").to_owned();
        }
        let done = refreshed == Some(true);
        if done {
            label = dc_intl::t("i4jeWR").to_owned();
        }
        let loading = refreshed == Some(false);
        let button = sm_button(
            SharedString::from(format!("connection-refresh-{id}")),
            label,
            false,
            done,
        )
        .when(loading, |this| this.opacity(0.5))
        .when(refreshed.is_none(), |this| {
            this.on_click(
                cx.listener(move |this, _, _, cx| this.connection_refresh_clicked(&kind, &id, cx)),
            )
        });
        Some(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-muted"))
                .bg(color("--background-mod-muted"))
                .children(entries)
                .child(div().ml_auto().child(button))
                .into_any_element(),
        )
    }

    /// `TY`: the reconnect offers of a one-way console link (`TM` with `Tw` for Xbox, `TU` for PlayStation).
    fn connection_upsell(&self, item: &Value, cx: &mut Context<Self>) -> Option<AnyElement> {
        let kind = item["type"].as_str()?;
        if flag(&item["two_way_link"]) {
            return None;
        }
        #[allow(clippy::type_complexity)]
        let (title, body, article, picture, width, height): (
            &'static str,
            &'static str,
            &'static str,
            &'static str,
            f32,
            f32,
        ) = match kind {
            "xbox" => (
                "2okkZV",
                "Now you can chat in Discord Voice Channels and Voice Calls on your Xbox! You'll need to reconnect to Xbox in order to enable this feature. ",
                "360003953831",
                "9df988a227916145.png",
                0.,
                45.,
            ),
            "playstation" => (
                "v20wwm",
                "Now you can chat in Discord Voice Channels and Voice Calls on your PlayStation 5! You'll need to reconnect to PlayStation Network in order to enable this feature. ",
                "4419534960919",
                "88954903b6a5b9cc.svg",
                82.,
                0.,
            ),
            _ => return None,
        };
        let platform: &'static str = if kind == "xbox" {
            "xbox"
        } else {
            "playstation"
        };
        let art = self
            .images
            .get(&platform_presence::asset_key(picture))
            .cloned()
            .map(|picture| {
                img(picture)
                    .flex_none()
                    .when(width > 0., |this| this.w(px(width)))
                    .when(height > 0., |this| this.h(px(height)))
            });
        Some(
            // `container_8W_RfC`: a `--background-mod-normal` row, 24px by 12px inside.
            div()
                .flex()
                .flex_row()
                .items_center()
                .py(px(24.))
                .px(px(12.))
                .rounded(px(8.))
                .bg(color("--background-mod-normal"))
                .children(art)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .mx(px(12.))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .child(div().flex().mr(px(8.)).child(new_badge()))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t(title)),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-default"))
                                .child(self.text_with_link(
                                    "connection-upsell",
                                    body,
                                    "Learn more",
                                    "",
                                    cx,
                                    move |this, cx| {
                                        this.settings_open_url(
                                            &format!("https://support.discord.com/hc/en-us/articles/{article}"),
                                            cx,
                                        )
                                    },
                                )),
                        ),
                )
                .child(
                    sm_button(
                        SharedString::from(format!("connection-upsell-{kind}")),
                        dc_intl::t("vD60Pv").to_owned(),
                        true,
                        false,
                    )
                    .h(px(40.))
                    .px(px(15.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .on_click(cx.listener(move |this, _, _, cx| this.connection_clicked(platform, cx))),
                )
                .into_any_element(),
        )
    }

    /// `TH`: a server that this account's subscription unlocks, with Join when the account is not in it.
    fn connection_integration(
        &self,
        integration: &Value,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = integration["id"].as_str()?.to_owned();
        let guild = &integration["guild"];
        let guild_id: Option<dc_state::Id> = guild["id"].as_str().and_then(|id| id.parse().ok());
        let joined = guild_id.is_some_and(|guild| self.stores.guild(guild).is_some());
        let state = self.settings_extra.conn_joining.get(&id);
        let joining = matches!(state, Some(None));
        let error = state.cloned().flatten();
        let name = guild["name"].as_str().unwrap_or_default().to_owned();
        let icon = guild_id
            .zip(guild["icon"].as_str())
            .and_then(|(guild, hash)| {
                self.images
                    .get(&format!(
                        "https://cdn.discordapp.com/icons/{guild}/{hash}.png?size=64"
                    ))
                    .cloned()
            });
        let acronym: String = name
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .collect();
        let join = (!joined).then(|| {
            let id = id.clone();
            sm_button(
                SharedString::from(format!("connection-join-{id}")),
                dc_intl::t(if joining { "RXvQQu" } else { "XpeFYr" }).to_owned(),
                true,
                joining,
            )
            .when(!joining, |this| {
                this.on_click(
                    cx.listener(move |this, _, _, cx| this.connection_join_clicked(&id, cx)),
                )
            })
        });
        Some(
            // `integrationWrapper_yX-kzH`.
            div()
                .mt(px(8.))
                .p(px(16.))
                .rounded(px(8.))
                .bg(color("--background-mod-normal"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            // `guildIcon_yX-kzH`: the Small guild icon (30px), 8px before the name.
                            div()
                                .size(px(30.))
                                .mr(px(8.))
                                .flex_none()
                                .rounded_full()
                                .overflow_hidden()
                                .bg(color("--background-mod-strong"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .text_color(color("--text-strong"))
                                .map(|this| match icon {
                                    Some(picture) => this.child(img(picture).size(px(30.))),
                                    None => this.child(acronym),
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-default"))
                                        .child(
                                            integration["account"]["name"]
                                                .as_str()
                                                .unwrap_or_default()
                                                .to_owned(),
                                        ),
                                ),
                        )
                        .children(join),
                )
                .children(error.map(|message| {
                    div()
                        .flex()
                        .justify_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-feedback-critical"))
                        .child(message)
                }))
                .into_any_element(),
        )
    }

    /// The Verified on Twitter mark of the card (module 158935 `l`, the flower of module 152578): the 16px flower in
    /// `PLATFORM_TWITTER` with the 16px CheckmarkSmall in white on it, in `connectionAccountLabelVerified_yX-kzH`
    /// (20 by 16, 4px after the name), tooltip `Jebrww` on top. GAP: no DOM capture of this mark (the fixture account
    /// has no verified X link).
    fn twitter_verified_badge(&self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let badge = div()
            .relative()
            .size(px(16.))
            .flex_none()
            .child(
                gpui::svg()
                    .path("icons/discover_flower.svg")
                    .size(px(16.))
                    .text_color(gpui::rgb(TWITTER_BLUE)),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path("icons/discover_flower_check.svg")
                            .size(px(16.))
                            .flex_none()
                            .text_color(gpui::rgb(0xffffff)),
                    ),
            );
        div()
            .flex_none()
            .w(px(20.))
            .h(px(16.))
            .ml(px(4.))
            .child(self.with_tip(
                format!("settings-conn-verified-{id}"),
                crate::ui::tooltip::Tip::text(dc_intl::t("Jebrww"), crate::ui::tooltip::Side::Top),
                badge,
                cx,
            ))
            .into_any_element()
    }

    /// One connected account (`TW`, `connection_yX-kzH`).
    pub(super) fn connection_card(
        &self,
        item: &Value,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let account = account_of(item)?;
        let (platform_name, light, dark_svg) = platform_info(&account.kind)?;
        let icon = self
            .images
            .get(&platform_presence::asset_key(if dark {
                dark_svg
            } else {
                light
            }))
            .cloned();
        let shown = account.clone();
        let header = div()
            // `connectionHeader_yX-kzH`: icon, the two lines, the X.
            .flex()
            .items_center()
            .child(
                div()
                    .size(px(32.))
                    .flex_none()
                    .children(icon.map(|picture| img(picture).size(px(32.)))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .ml(px(16.))
                    .flex()
                    .flex_col()
                    .child(
                        // `connectionAccountLabelContainer_yX-kzH`: the name, then the Verified on Twitter mark.
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .text_ellipsis()
                                    .child(account.name.clone()),
                            )
                            .children(
                                twitter_verified(&account.kind, &item["metadata"])
                                    .then(|| self.twitter_verified_badge(&account.id, cx)),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .text_ellipsis()
                            .child(platform_name),
                    ),
            )
            .child(
                // `connectionDelete_yX-kzH`: `XSmallIcon` xs, 8px each side, brighter on hover.
                div()
                    .id(SharedString::from(format!("disconnect-{}", account.id)))
                    .group(SharedString::from(format!("disconnect-{}", account.id)))
                    .mx(px(8.))
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // `ppppRJ`: the confirm dialog first; the removal is its Unlink button.
                        this.settings_extra.dialog = Some(Dialog::Disconnect(shown.clone()));
                        cx.notify();
                    }))
                    .child(
                        mask_icon("clips_x_small", 16., "--interactive-text-default").group_hover(
                            SharedString::from(format!("disconnect-{}", account.id)),
                            |style| style.text_color(color("--interactive-text-hover")),
                        ),
                    ),
            );
        let switches: Vec<AnyElement> = toggles_of(&account.kind)
            .into_iter()
            .map(|toggle| self.connection_switch(item, toggle, cx))
            .collect();
        let revoked = flag(&item["revoked"]);
        let integrations: Vec<AnyElement> = item["integrations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|integration| self.connection_integration(integration, cx))
            .collect();
        let kind = account.kind.clone();
        let tail: Option<AnyElement> = if revoked {
            // `iK.w` info notice with `6C4lgA`, its link starting the reconnect.
            Some(
                div()
                    .flex()
                    .gap(px(8.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--inlinenotice-border-info"))
                    .bg(color("--background-feedback-mod-info"))
                    .child(div().h(px(18.)).flex().items_center().child(mask_icon(
                        "settings_notice_info",
                        16.,
                        "--icon-feedback-info",
                    )))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-strong"))
                            .child(self.text_with_link(
                                "connection-revoked",
                                "This account has been revoked. Would you like to ",
                                "reconnect",
                                " it?",
                                cx,
                                move |this, cx| this.connection_reconnect(&kind, cx),
                            )),
                    )
                    .into_any_element(),
            )
        } else if !integrations.is_empty() {
            Some(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("fOe3fZ")),
                    )
                    .children(integrations)
                    .into_any_element(),
            )
        } else {
            None
        };
        Some(
            div()
                .id(SharedString::from(format!(
                    "connection-{}-{}",
                    account.kind, account.id
                )))
                .w_full()
                .flex()
                .flex_col()
                .gap(px(20.))
                .p(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-mod-subtle"))
                .child(header)
                .children(self.connection_upsell(item, cx))
                .children(self.connection_metadata_box(item, cx))
                .child(div().flex().flex_col().gap(px(20.)).children(switches))
                .when(tail.is_some(), |this| this.child(divider()))
                .children(tail)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_keeps_what_the_js_function_keeps() {
        assert_eq!(js_escape("123456"), "123456");
        assert_eq!(js_escape("did:plc:abc"), "did%3Aplc%3Aabc");
        assert_eq!(js_escape("a b/c@d.e_f-g+h*"), "a%20b/c@d.e_f-g+h*");
        assert_eq!(js_escape("ü"), "%FC");
        assert_eq!(js_escape("ğ"), "%u011F");
    }

    #[test]
    fn switches_send_the_official_bodies() {
        for (toggle, field) in [
            (Toggle::Visibility, "visibility"),
            (Toggle::Details, "metadata_visibility"),
            (Toggle::Activity, "show_activity"),
            (Toggle::FriendSync, "friend_sync"),
        ] {
            let Request::Rest {
                key,
                method,
                route,
                body,
            } = update_request("bluesky", "did:plc:x", toggle, true)
            else {
                panic!("a rest request");
            };
            assert_eq!(method, reqwest::Method::PATCH);
            assert_eq!(route, "/users/@me/connections/bluesky/did%3Aplc%3Ax");
            assert_eq!(body, Some(json!({ field: true })));
            assert!(key.unwrap().starts_with(UPDATE_KEY));
        }
        let Request::Rest { body, .. } = update_request("spotify", "a", Toggle::Activity, false)
        else {
            panic!("a rest request");
        };
        assert_eq!(body, Some(json!({"show_activity": false})));
    }

    #[test]
    fn refresh_join_and_callback_routes() {
        let Request::Rest { method, route, .. } = refresh_request("steam", "7656") else {
            panic!("a rest request");
        };
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/users/@me/connections/steam/7656/refresh");
        let Request::Rest { route, .. } = join_request("99") else {
            panic!("a rest request");
        };
        assert_eq!(route, "/integrations/99/join");
        let Request::Rest { route, body, .. } = callback_request(
            "github",
            &json!({"provider": "github", "code": "c", "state": "s", "openid_params": null}),
        ) else {
            panic!("a rest request");
        };
        assert_eq!(route, "/connections/github/callback");
        assert_eq!(
            body,
            Some(json!({"code": "c", "state": "s", "insecure": false, "friend_sync": false}))
        );
        let Request::Rest { body, .. } =
            callback_request("facebook", &json!({"code": "c", "state": "s"}))
        else {
            panic!("a rest request");
        };
        assert_eq!(body.unwrap()["friend_sync"], true);
    }

    #[test]
    fn a_card_has_the_switches_of_its_platform() {
        assert_eq!(toggles_of("github"), vec![Toggle::Visibility]);
        assert_eq!(
            toggles_of("steam"),
            vec![Toggle::Visibility, Toggle::Details]
        );
        assert_eq!(
            toggles_of("spotify"),
            vec![Toggle::Visibility, Toggle::Activity]
        );
        assert_eq!(
            toggles_of("playstation"),
            vec![Toggle::Visibility, Toggle::Activity]
        );
        assert_eq!(
            toggles_of("facebook"),
            vec![Toggle::Visibility, Toggle::FriendSync]
        );
    }

    #[test]
    fn details_need_the_profile_switch_and_metadata() {
        let on = json!({"visibility": 1, "metadata": {"game_count": "3"}});
        assert!(!toggle_disabled(&on, Toggle::Details));
        let hidden = json!({"visibility": 0, "metadata": {"game_count": "3"}});
        assert!(toggle_disabled(&hidden, Toggle::Details));
        let empty = json!({"visibility": 1});
        assert!(toggle_disabled(&empty, Toggle::Details));
        assert!(!toggle_disabled(&empty, Toggle::Visibility));
        assert!(toggle_state(&json!({"visibility": 1}), Toggle::Visibility));
        assert!(toggle_state(
            &json!({"show_activity": true}),
            Toggle::Activity
        ));
        assert!(!toggle_state(
            &json!({"metadata_visibility": 0}),
            Toggle::Details
        ));
    }

    #[test]
    fn only_a_verified_x_account_takes_the_twitter_mark() {
        assert!(twitter_verified("twitter", &json!({"verified": "1"})));
        assert!(!twitter_verified("twitter", &json!({"verified": "0"})));
        assert!(!twitter_verified("twitter", &json!({"verified": 1})));
        assert!(!twitter_verified("twitter", &Value::Null));
        assert!(!twitter_verified("bluesky", &json!({"verified": "1"})));
        assert!(!twitter_verified(
            "twitter_legacy",
            &json!({"verified": "1"})
        ));
    }

    #[test]
    fn the_unlink_dialog_knows_the_platform_and_the_two_way_link() {
        let account =
            account_of(&json!({"type": "xbox", "id": "1", "name": "Major", "two_way_link": true}))
                .unwrap();
        assert_eq!(account.platform, "Xbox");
        assert!(account.two_way);
        let account =
            account_of(&json!({"type": "github", "id": "1", "name": "a", "two_way_link": true}))
                .unwrap();
        assert!(!account.two_way);
    }
}
