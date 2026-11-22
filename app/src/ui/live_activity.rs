//! Live profile cards: modules 802880, 960549, 583846 and 913659.
use super::{Root, color, icon};
use crate::{net::Request, parity::Parity};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, SharedString, div, img, prelude::*, px,
    relative, svg,
};
use serde_json::Value;
use std::time::Duration;

use super::platform_presence;

/// `aspectRatio: "crunchyroll"` (module 802880 over 623671): `.aspectRatio_1AI0dL.crunchyroll_1AI0dL` is
/// `aspect-ratio: 2/3` at the image size's width, so a Crunchyroll show's poster is half again as tall as the square
/// art of other activities (cover fit).
pub(super) fn poster_height(activity: &Value, image_size: f32) -> f32 {
    if platform_presence::is_crunchyroll(activity) {
        image_size * 1.5
    } else {
        image_size
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Surface {
    Popout,
    Sidebar,
    Modal,
}

fn component(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

pub(super) fn millis(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str().filter(|s| !s.is_empty())
}
fn duration_text(seconds: i64, padded: bool) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    let minutes = seconds / 60 % 60;
    let seconds = seconds % 60;
    if hours > 0 {
        if padded {
            format!("{hours:02}:{minutes:02}:{seconds:02}")
        } else {
            format!("{hours}:{minutes:02}:{seconds:02}")
        }
    } else if padded {
        format!("{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}
fn elapsed(activity: &Value, now: i64) -> Option<String> {
    let start =
        millis(&activity["timestamps"]["start"]).or_else(|| millis(&activity["created_at"]))?;
    let end = millis(&activity["timestamps"]["end"]);
    let countdown = activity["timestamps"]["isCountDown"]
        .as_bool()
        .unwrap_or(false);
    let span = if countdown && end.is_some_and(|end| end > now) {
        end? - now
    } else {
        (if countdown {
            now
        } else {
            end.map_or(now, |end| end.min(now))
        }) - start
    };
    Some(duration_text(span.max(0) / 1000, false))
}
/// `gF` of module 583846: the party chip's text, the state with "(n of max)" (or "(n)") after it.
/// `aria-valuetext` of module 753713: "{elapsed} of {duration}", each as `XK` of module 583846 writes it ("1 hour, 2
/// minutes, 3 seconds" without the hours when there are none), the elapsed time floored to ten seconds and held to
/// the duration.
pub(super) fn playback_value_text(elapsed_s: i64, duration_s: i64) -> String {
    let words = |total: i64| {
        let total = total.max(0);
        let (hours, minutes, seconds) = (total / 3600, (total / 60) % 60, total % 60);
        let mut parts = Vec::new();
        if hours > 0 {
            parts.push(dc_intl::format(
                "xCjYxK",
                &[("hours", (hours as f64).into())],
            ));
        }
        parts.push(dc_intl::format(
            "iXLF9W",
            &[("minutes", (minutes as f64).into())],
        ));
        parts.push(dc_intl::format(
            "geSp4K",
            &[("seconds", (seconds as f64).into())],
        ));
        parts.join(", ")
    };
    let elapsed = elapsed_s.clamp(0, duration_s.max(0)) / 10 * 10;
    dc_intl::format(
        "bnnNER",
        &[
            ("elapsed", words(elapsed).into()),
            ("duration", words(duration_s).into()),
        ],
    )
}

pub(super) fn party_text(state: Option<&str>, party: &Value) -> Option<String> {
    let size = &party["size"];
    let (count, max) = (size[0].as_i64(), size[1].as_i64());
    let count_text = match (count, max) {
        (Some(count), Some(max)) if count > 0 && max > 0 => {
            Some(dc_intl::t!("wmUSiy", count = count, max = max))
        }
        (Some(count), _) if count > 0 => Some(dc_intl::t!("UTYMsa", count = count)),
        _ => None,
    };
    match (state, count_text) {
        (Some(state), Some(count)) => Some(format!("{state} ({count})")),
        (Some(state), None) => Some(state.to_owned()),
        (None, count) => count,
    }
}

/// The picture of an activity's `large_image` or `small_image`: 913659 `uD(application_id, asset, [160, 160])`
/// (`cdn::activity_asset`), as module 325158 `b` asks for both.
pub(super) fn asset_url(activity: &Value, key: &str) -> Option<String> {
    let asset = field(&activity["assets"], key)?;
    super::cdn::activity_asset(field(activity, "application_id"), asset, ACTIVITY_ART)
}

/// 487887 `iu.LARGE`: the side every activity card asks its pictures at.
pub(super) const ACTIVITY_ART: u32 = 160;
/// `connectionEntrypointUrl` of the application (module 724302): where linking an account starts.
fn connection_url(application: &Value) -> Option<String> {
    application["connection_entrypoint_url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
}

/// The application id the account link is looked up under: `parentId ?? id` (module 591022, 724302).
fn link_id(application: &Value) -> Option<String> {
    application["parent_id"]
        .as_str()
        .or_else(|| application["id"].as_str())
        .map(str::to_owned)
}

/// The key of the `GET /oauth2/tokens?application_ids=` that says whether an application is already linked.
fn tokens_key(id: &str) -> String {
    format!("oauth-token:{id}")
}

/// Whether "Connect Account" shows (802880: `fetched && canStartAuthorization && !hasAlreadyLinked`): the token
/// lookup for the application answered (`tokens` is its array; a failed lookup counts as answered with no token,
/// 733110 `USER_AUTHORIZED_APPS_REQUEST_FAILED` -> FETCHED), nothing in it is for this application, and the
/// application has a connection entry point (the web flow of 724302; the RPC flow needs a game connected over
/// local RPC, which a client without one never has).
pub(super) fn needs_account_link(application: &Value, tokens: Option<&Value>) -> Option<String> {
    let url = connection_url(application)?;
    let id = link_id(application)?;
    let tokens = tokens?.as_array()?;
    let linked = tokens
        .iter()
        .any(|token| token["application"]["id"].as_str() == Some(id.as_str()));
    (!linked).then_some(url)
}

/// Module 607781: distributor storefronts parsed from `application.third_party_skus` in official
/// priority order `[xbox_game_pass, steam]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DistributorOption {
    pub distributor: &'static str,
    pub single_label: String,
    pub menu_label: String,
    pub icon_name: &'static str,
    pub url: String,
}

/// Resolves all available Play-on-Distributor storefronts in priority order (module 607781):
/// 1. `xbox_game_pass` -> `"Play with Xbox Game Pass"` (`s7+3um`) / menu `"Xbox Game Pass"` (`QpN/Iz`)
/// 2. `steam` -> `"Get on Steam"` (`L1N2gh`) / menu `"Steam"` (`FsANs4`)
pub(super) fn distributor_options(application: &Value) -> Vec<DistributorOption> {
    let Some(skus) = application["third_party_skus"].as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (dist, single_key, menu_key, icon_name) in [
        (
            "xbox_game_pass",
            "s7+3um",
            "QpN/Iz",
            "activity_connect_xbox",
        ),
        ("steam", "L1N2gh", "FsANs4", "activity_steam"),
    ] {
        if let Some(sku) = skus.iter().find(|s| s["distributor"] == dist)
            && let Some(id) = field(sku, "id").or_else(|| field(sku, "sku"))
        {
            let encoded = component(id);
            let url = if dist == "xbox_game_pass" {
                format!("https://www.xbox.com/games/store/title/{encoded}?ocid=discord7937464")
            } else {
                format!("https://store.steampowered.com/app/{encoded}")
            };
            out.push(DistributorOption {
                distributor: dist,
                single_label: dc_intl::t(single_key).to_owned(),
                menu_label: dc_intl::t(menu_key).to_owned(),
                icon_name,
                url,
            });
        }
    }
    out
}

#[cfg(test)]
fn steam_url(application: &Value) -> Option<String> {
    distributor_options(application)
        .into_iter()
        .find(|d| d.distributor == "steam")
        .map(|d| d.url)
}

/// Module 623671: Activity / App `ContentImage` sizing spec (`SIZE_60`, `SIZE_72`, `SIZE_100`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContentImageSize {
    Size60,
    #[allow(dead_code)]
    Size72,
    Size100,
}

impl ContentImageSize {
    pub(super) fn main_px(self) -> f32 {
        match self {
            Self::Size60 => 60.,
            Self::Size72 => 72.,
            Self::Size100 => 100.,
        }
    }

    /// Module 623671: small sub-image diameter (`24px` for SIZE_60, `32px` for SIZE_72 / SIZE_100).
    pub(super) fn small_px(self) -> f32 {
        match self {
            Self::Size60 => 24.,
            Self::Size72 | Self::Size100 => 32.,
        }
    }
}

fn metadata_route(user: Id, activity: &Value) -> Option<String> {
    Some(format!(
        "/users/{user}/sessions/{}/activities/{}/metadata",
        component(field(activity, "session_id")?),
        component(field(activity, "application_id")?),
    ))
}

fn button_url(metadata: &Value, index: usize) -> Option<String> {
    let url = reqwest::Url::parse(metadata["button_urls"].get(index)?.as_str()?).ok()?;
    url.host_str()?;
    Some(url.into())
}

impl Root {
    fn live_button_clicked(
        &mut self,
        user: Id,
        activity: &Value,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(metadata) = activity["metadata"].as_object().filter(|m| !m.is_empty()) {
            if let Some(url) = button_url(&Value::Object(metadata.clone()), index) {
                self.follow_link(&url, "", cx);
            }
            return;
        }
        let Some(route) = metadata_route(user, activity) else {
            return;
        };
        let key = format!("presence-button:{index}:{route}");
        if let Some(metadata) = self.fetched.get(&key) {
            if let Some(url) = button_url(metadata, index) {
                self.follow_link(&url, "", cx);
            }
        } else {
            let _ = self.requests.send(Request::Fetch { key, route });
        }
    }

    pub(super) fn live_button_fetched(&mut self, key: &str, body: Value, cx: &mut Context<Self>) {
        let index = key.split(':').nth(1).and_then(|s| s.parse().ok());
        if let Some(url) = index.and_then(|index| button_url(&body, index)) {
            self.fetched.insert(key.to_owned(), body);
            self.follow_link(&url, "", cx);
        }
    }

    fn live_users(&self) -> Vec<Id> {
        let mut users = Vec::new();
        if let Some(profile) = &self.profile {
            users.push(profile.user);
        }
        if let Some(profile) = &self.full_profile {
            users.push(profile.user);
        }
        // The editor is your own full profile (module 927071 in editing mode): its Activity tab shows your cards.
        if self.profile_editor.is_some()
            && let Some(me) = self.stores.current_user.as_ref()
        {
            users.push(me.id);
        }
        if self.show_dm_profile
            && let Some(user) = self.current_dm().and_then(|c| self.stores.dm_recipient(c))
        {
            users.push(user.id);
        }
        // The voice user popout's person (module 772800).
        users.extend(self.voice_popout_user());
        users.sort_unstable();
        users.dedup();
        users
    }

    fn live_clock_delay(&self) -> Option<Duration> {
        let activities: Vec<&Value> = self
            .live_users()
            .into_iter()
            .filter_map(|user| self.stores.presence.get(user, self.guild))
            .flat_map(|presence| presence.visible_activities())
            .collect();
        if activities
            .iter()
            .any(|a| a["name"] == "Spotify" && millis(&a["timestamps"]["end"]).is_some())
        {
            Some(Duration::from_millis(500))
        } else if activities
            .iter()
            .any(|a| !a["timestamps"].is_null() || !a["created_at"].is_null())
            || self.inventory_has_live_card()
        {
            Some(Duration::from_secs(
                if !self.notification_focused || crate::ui::motion::reduced() {
                    15
                } else {
                    1
                },
            ))
        } else {
            None
        }
    }

    pub(super) fn prepare_live_activities(&mut self, cx: &mut Context<Self>) {
        self.spotify_render_lifecycle();
        let activities: Vec<Value> = self
            .live_users()
            .into_iter()
            .filter(|user| !self.is_blocked(*user) && !self.stores.is_ignored(*user))
            .filter_map(|user| self.stores.presence.get(user, self.guild))
            .flat_map(|presence| presence.visible_activities().into_iter().cloned())
            .collect();
        // A game's record tells whether its card's menu offers View Game Profile.
        for activity in activities.iter().filter(|activity| activity["type"] == 0) {
            if let Some(app) = activity["application_id"].as_str() {
                self.request_game_record(app);
            }
        }
        if activities.iter().any(dc_state::now_playing::is_spotify) {
            self.request_image(
                super::spotify_activity::ICON_KEY.into(),
                format!(
                    "{}/assets/168e10535f129f11.js",
                    dc_proto::pin::WEBAPP_ORIGIN
                ),
            );
        }
        for activity in &activities {
            // The header's platform mark (770689 `platformIcon`) and the console picture of 325158 `P`.
            if let Some(platform) = platform_presence::provider(activity) {
                self.request_image(
                    platform_presence::asset_key(platform.white_svg),
                    platform_presence::asset_url(platform.white_svg),
                );
            }
            if let Some(file) = platform_presence::fallback_art(activity, false) {
                self.request_image(
                    platform_presence::asset_key(file),
                    platform_presence::asset_url(file),
                );
            }
            for key in ["large_image", "small_image"] {
                if let Some(url) = asset_url(activity, key) {
                    self.request_image(url.clone(), url);
                }
            }
            if let Some(app) = field(activity, "application_id") {
                let key = format!("presence-app:{app}");
                // A failed lookup left its placeholder; once it is due again the placeholder goes.
                if self.retry_failed_fetch(&key) {
                    self.fetched.remove(&key);
                }
                if !self.fetched.contains_key(&key) {
                    self.fetched.insert(key.clone(), Value::Null);
                    let _ = self.requests.send(Request::Fetch {
                        key,
                        route: format!("/applications/public?application_ids={app}"),
                    });
                }
                // 704824 `useEffect`: the first render of a card with a known application asks the token store
                // about it (`GET /oauth2/tokens?application_ids=`), once.
                let linked_id = self
                    .fetched
                    .get(&format!("presence-app:{app}"))
                    .and_then(|body| body.as_array())
                    .and_then(|apps| apps.first())
                    .and_then(link_id);
                if let Some(id) = linked_id {
                    let key = tokens_key(&id);
                    if !self.fetched.contains_key(&key) {
                        self.fetched.insert(key.clone(), Value::Null);
                        let _ = self.requests.send(Request::Fetch {
                            key,
                            route: format!("/oauth2/tokens?application_ids={id}"),
                        });
                    }
                }
            }
        }
        if let Some(mut delay) = self.live_clock_delay()
            && !self.presence_ticking
        {
            self.presence_ticking = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(delay).await;
                    let next = this
                        .update(cx, |this, cx| {
                            let next = this.live_clock_delay();
                            this.presence_ticking = next.is_some();
                            cx.notify();
                            next
                        })
                        .ok()
                        .flatten();
                    let Some(next) = next else {
                        break;
                    };
                    delay = next;
                }
            })
            .detach();
        }
    }

    /// The answer of the token lookup: the array of tokens, or none when the lookup failed (733110 still counts the
    /// application as fetched then).
    pub(super) fn live_tokens_fetched(&mut self, key: String, body: Value) {
        let tokens = if crate::net::is_fetch_failure(&body) || !body.is_array() {
            Value::Array(Vec::new())
        } else {
            body
        };
        self.fetched.insert(key, tokens);
    }

    pub(super) fn live_application_fetched(&mut self, body: &Value) {
        for app in body.as_array().into_iter().flatten() {
            if let (Some(id), Some(hash)) = (field(app, "id"), field(app, "icon")) {
                // 325158 `O`: the application's `getIconURL(iu.LARGE)`.
                let url = super::cdn::app_icon(id, hash, ACTIVITY_ART);
                self.request_image(format!("app-icon:{id}"), url);
            }
        }
    }
}

/// Module 727353 (`711589.K`): stream preview action overlay copy (`XvBdeT`, `JH1SJ+`, `7Xq/nV`, `pgUTZC`).
pub(super) fn stream_action_label(
    is_own: bool,
    is_watching: bool,
    can_watch: bool,
) -> &'static str {
    if is_own {
        dc_intl::t("XvBdeT")
    } else if is_watching {
        dc_intl::t("JH1SJ+")
    } else if can_watch {
        dc_intl::t("7Xq/nV")
    } else {
        dc_intl::t("pgUTZC")
    }
}

/// Module 727353 `Z` (Standalone / Overlay Stream Preview Popout): header (`4CQq9Q` / `Jpkr/q`),
/// 142px preview (`previewContainer_1v6-1l`) with hover overlay, and protip (`YRdw70` / `CCTgbF`).
#[allow(dead_code)]
pub(super) fn stream_preview_popout(
    app_name: Option<&str>,
    thumb: Option<std::sync::Arc<gpui::Image>>,
    is_own: bool,
    is_watching: bool,
    is_overlay: bool,
    hide_tip: bool,
) -> AnyElement {
    let header_title = match app_name.filter(|s| !s.is_empty()) {
        Some(name) => dc_intl::format("4CQq9Q", &[("name", dc_intl::Arg::Str(name.into()))]),
        None => dc_intl::t("Jpkr/q").to_owned(),
    };
    let action_text = stream_action_label(is_own, is_watching, true);
    let protip = if is_overlay {
        dc_intl::t("YRdw70")
    } else {
        dc_intl::t("CCTgbF")
    };
    div()
        .max_w(px(252.))
        .w(px(252.))
        .rounded(px(12.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-high"))
        .shadow(vec![super::shadow_high()])
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(10.))
                .pt(px(10.))
                .pb(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .truncate()
                        .child(header_title),
                ),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(142.))
                .bg(color("--background-base-lowest"))
                .children(thumb.map(|im| img(im).size_full().object_fit(gpui::ObjectFit::Cover)))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(color("--opacity-black-60"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(action_text),
                ),
        )
        .when(!hide_tip, |card| {
            card.child(
                div()
                    .p(px(12.))
                    .text_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-muted"))
                    .child(protip),
            )
        })
        .into_any_element()
}

impl Root {
    pub(super) fn live_activity_cards(
        &self,
        user: Id,
        surface: Surface,
        strong: Hsla,
        subtle: Hsla,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // Not in the pinned client: BetterActivities shows every activity in the popout
        // (`plugin_hooks/better_activities.rs`).
        if surface == Surface::Popout
            && let Some(cards) = self.better_activities_popout(user, strong, subtle, cx)
        {
            return cards;
        }
        self.live_activity_cards_picked(user, surface, strong, subtle, None, cx)
    }

    /// The cards of `live_activity_cards`, or with `pick` (BetterActivities) only the visible activities it names,
    /// each with or without its buttons, and never stacked.
    pub(in crate::ui) fn live_activity_cards_picked(
        &self,
        user: Id,
        surface: Surface,
        strong: Hsla,
        subtle: Hsla,
        pick: Option<&[(usize, bool)]>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let large = surface == Surface::Modal;
        if self.is_blocked(user) || self.stores.is_ignored(user) {
            return Vec::new();
        }
        let Some(presence) = self.stores.presence.get(user, self.guild) else {
            return Vec::new();
        };
        let bot = self.stores.users.get(&user).is_some_and(|u| u.bot);
        let palette = self.profile_palette(user, self.guild, surface == Surface::Sidebar);
        let profile_theme = palette
            .theme
            .as_ref()
            .map_or_else(super::theme, |theme| theme.tokens);
        let card_background = if self.dm_palette(user).themed {
            color("--background-mod-normal")
        } else {
            color("--user-profile-overlay-background")
        };
        let now = chrono::Utc::now().timestamp_millis();
        let activities = presence.visible_activities();
        let stacked = pick.is_none() && !large && activities.len() > 1;
        activities
            .into_iter()
            .enumerate()
            .filter(|(index, _)| {
                pick.map_or(large || *index == 0, |pick| {
                    pick.iter().any(|(shown, _)| shown == index)
                })
            })
            .map(|(index, activity)| {
                let bare = pick.is_some_and(|pick| {
                    pick.iter().any(|(shown, buttons)| *shown == index && !buttons)
                });
                let app_id = field(activity, "application_id");
                let application = app_id
                    .and_then(|id| self.fetched.get(&format!("presence-app:{id}")))
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first());
                let connect_url = application.and_then(|application| {
                    needs_account_link(
                        application,
                        link_id(application).and_then(|id| self.fetched.get(&tokens_key(&id))),
                    )
                });
                // 325158 `P`: the activity's art, else the application's icon, else Xbox's or PlayStation's own
                // picture for a console game.
                let has_large = asset_url(activity, "large_image").is_some()
                    || application.and_then(|app| field(app, "icon")).is_some();
                let art = asset_url(activity, "large_image")
                    .and_then(|url| self.images.get(&url))
                    .cloned()
                    .or_else(|| {
                        app_id.and_then(|id| self.images.get(&format!("app-icon:{id}")).cloned())
                    })
                    .or_else(|| {
                        platform_presence::fallback_art(activity, has_large).and_then(|file| {
                            self.images.get(&platform_presence::asset_key(file)).cloned()
                        })
                    });
                let (heading, header_platform) = platform_presence::heading(activity);
                let header_icon = header_platform.and_then(|platform| {
                    self.images
                        .get(&platform_presence::asset_key(platform.white_svg))
                        .cloned()
                });
                let own = self.stores.current_user.as_ref().is_some_and(|me| me.id == user);
                let connect_offer = (!own && !bot)
                    .then(|| platform_presence::connect_offer(activity, &self.linked_platforms()))
                    .flatten();
                // 325158 `b`: a Crunchyroll activity's small image is not drawn.
                let small = asset_url(activity, "small_image")
                    .filter(|_| !platform_presence::is_crunchyroll(activity))
                    .and_then(|url| self.images.get(&url))
                    .cloned();
                let kind = activity["type"].as_u64().unwrap_or(0);
                let spotify = dc_state::now_playing::is_spotify(activity);
                let missing_spotify_art = spotify && asset_url(activity, "large_image").is_none();
                // Module 285909 draws the button when its menu has rows: a game always has Report Issue; another
                // activity only View All Activity, in a popout.
                // In the full profile the official card draws it for a game too (`headerContextMenu_Bu4Al8`).
                let more = !bot
                    && if spotify {
                        own || surface == Surface::Popout
                    } else if kind == 0 {
                        true
                    } else {
                        !large && surface == Surface::Popout
                    };
                let game = (kind == 0).then(|| {
                    (
                        super::report_issue::Detected {
                            name: field(activity, "name").unwrap_or_default().to_owned(),
                            application_id: app_id.map(|id| id.to_string()),
                            art: asset_url(activity, "large_image"),
                        },
                        activity["flags"].as_u64().unwrap_or(0) & 256 != 0,
                    )
                });
                let title = if kind == 0 {
                    field(activity, "name")
                } else {
                    field(activity, "details").or_else(|| field(activity, "name"))
                }
                .unwrap_or_default();
                let subtitle = if kind == 0 {
                    field(activity, "details")
                } else {
                    field(activity, "state")
                }
                .map(|s| {
                    if spotify {
                        s.replace("; ", ", ")
                    } else {
                        s.to_owned()
                    }
                });
                // `te` of module 802880: a game's state line only when it has no party; with one, the state goes
                // into the party chip (`cy` of module 78871).
                let has_party = activity["party"].is_object();
                let extra = if kind == 0 {
                    field(activity, "state").filter(|_| !has_party)
                } else if matches!(kind, 2 | 3) {
                    None
                } else {
                    field(&activity["assets"], "large_text")
                };
                let content_size = if large {
                    ContentImageSize::Size100
                } else {
                    ContentImageSize::Size60
                };
                let image_size = content_size.main_px();
                let small_size = content_size.small_px();
                let poster_height = poster_height(activity, image_size);
                let large_tip = field(&activity["assets"], "large_text").map(str::to_owned);
                let small_tip = field(&activity["assets"], "small_text").map(str::to_owned);
                let glyph = if kind == 2 {
                    "activity_music"
                } else if matches!(kind, 1 | 3) {
                    "activity_tv"
                } else {
                    "activity_game"
                };
                let duration = if !bot && matches!(kind, 0 | 2 | 3 | 5) && !spotify {
                    elapsed(activity, now / 1000 * 1000)
                } else {
                    None
                };
                let text_line = |text: String| {
                    div()
                        .min_w(px(0.))
                        .text_ellipsis()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(if spotify { palette.text } else { subtle })
                        .child(text)
                };
                let mut details = div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(if spotify { palette.text } else { strong })
                                    .child(title.to_owned()),
                            )
                            .children(subtitle.map(text_line))
                            .children(extra.map(|s| text_line(s.to_owned()))),
                    );
                // 282197 `A`: after the timer the badges of the matching feed entry; 506326 `te`: "New Player" for the
                // FIRST_TIME trait (USER_PROFILE location: subtle icon and text).
                let new_player = !bot
                    && kind == 0
                    && self
                        .stores
                        .content_inventory
                        .matching_inbox_entry(
                            dc_state::content_inventory::GLOBAL_FEED,
                            user,
                            activity,
                        )
                        .is_some_and(|entry| entry.first_time());
                let mut new_player_badge = new_player.then(|| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(icon("activity_new_player", 12., "--text-subtle"))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-subtle"))
                                .child("New Player"),
                        )
                });
                // `cy` of module 78871 (282197 `A` puts it first for a game): the party's size behind the group glyph.
                let mut party_chip = (kind == 0 && !bot && has_party)
                    .then(|| party_text(field(activity, "state"), &activity["party"]))
                    .flatten()
                    .map(|text| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("activity_party", 12., "--text-subtle"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-subtle"))
                                    .child(text),
                            )
                    });
                if duration.is_none() && (party_chip.is_some() || new_player_badge.is_some()) {
                    details = details.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x(px(8.))
                            .children(party_chip.take())
                            .children(new_player_badge.take()),
                    );
                }
                if let Some(duration) = duration {
                    details = details.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x(px(8.))
                            .children(party_chip)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(icon(glyph, 12., "--text-feedback-positive"))
                                    .child(
                                        div()
                                            .font_features(gpui::FontFeatures(std::sync::Arc::new(
                                                vec![("kern".into(), 1), ("tnum".into(), 1)],
                                            )))
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-feedback-positive"))
                                            .child(duration),
                                    ),
                            )
                            .children(new_player_badge),
                    );
                }
                if spotify
                    && let (Some(start), Some(end)) = (
                        millis(&activity["timestamps"]["start"]),
                        millis(&activity["timestamps"]["end"]),
                    )
                    && end > start
                {
                    let progress = ((now - start) as f32 / (end - start) as f32).clamp(0., 1.);
                    // Module 753713: the bar is a progress bar named Playback progress whose value reads "{elapsed} of
                    // {duration}" in words, the elapsed time rounded down to ten seconds.
                    let spoken = playback_value_text((now - start) / 1000, (end - start) / 1000);
                    details = details.child(
                        div()
                            .id(SharedString::from(format!("activity-progress-{user}-{index}-{large}")))
                            .role(gpui::Role::ProgressIndicator)
                            .aria_label(dc_intl::t("z2lxfe"))
                            .aria_description(spoken)
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(subtle)
                            .child(duration_text(
                                (now - start).clamp(0, end - start) / 1000,
                                true,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(2.))
                                    .rounded_full()
                                    .bg(color("--opacity-16"))
                                    .child(
                                        div()
                                            .w(relative(progress))
                                            .h_full()
                                            .rounded_full()
                                            .bg(strong),
                                    ),
                            )
                            .child(duration_text((end - start) / 1000, true)),
                    );
                }
                let mut actions: Option<AnyElement> = None;
                let buttons = activity["buttons"].as_array().filter(|b| !b.is_empty());
                if let Some(buttons) = buttons {
                    let buttons = buttons
                        .iter()
                        .enumerate()
                        .filter_map(|(button_index, label)| {
                            // 802880 `tc`: a Crunchyroll activity's buttons read Watch (`I6JG46`).
                            let label = if platform_presence::is_crunchyroll(activity) {
                                dc_intl::t("I6JG46").to_owned()
                            } else {
                                label.as_str()?.to_owned()
                            };
                            let activity = activity.clone();
                            Some(
                                div()
                                    .id(SharedString::from(format!(
                                        "activity-button-{user}-{index}-{button_index}-{large}"
                                    )))
                                    .when(!large, |this| this.w_full())
                                    .px(px(12.))
                                    .h(px(32.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--control-secondary-border-default"))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(strong)
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.live_button_clicked(user, &activity, button_index, cx);
                                    }))
                                    .child(label),
                            )
                        });
                    actions = Some(
                        div()
                            .flex()
                            .when(!large, |this| this.flex_col())
                            .flex_wrap()
                            .gap(px(8.))
                            .children(buttons)
                            .into_any_element(),
                    );
                } else if !application
                    .map(distributor_options)
                    .unwrap_or_default()
                    .is_empty()
                    || connect_url.is_some()
                    || application.and_then(|a| cloud_play_launch_app_id(a, "en-US")).is_some()
                {
                    let distributors = application.map(distributor_options).unwrap_or_default();
                    let cloud_play_app = application.and_then(|a| cloud_play_launch_app_id(a, "en-US"));
                    let mut stack = div().flex().flex_col().gap(px(8.));
                    // Module 985629 (`CloudPlayCtaButton`): "Try Game" (`jaYS/h`) with top tooltip `JVwWva`.
                    if let Some(cp_app_id) = cloud_play_app {
                        let cp_btn = div()
                            .id(SharedString::from(format!(
                                "activity-cloud-play-{user}-{index}-{large}"
                            )))
                            .when(!large, |d| d.w_full())
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(6.))
                            .cursor_pointer()
                            .child(icon("plugin_cloud_download", 16., "--text-strong"))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(strong)
                                    .child(dc_intl::t("jaYS/h")),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if let Ok(aid) = cp_app_id.parse::<Id>() {
                                    this.put_app_toast(
                                        format!("{} ({aid})", dc_intl::t("jaYS/h")),
                                        true,
                                        cx,
                                    );
                                }
                            }));
                        stack = stack.child(self.with_tip(
                            format!("activity-cloud-play-tip-{user}-{index}-{large}"),
                            super::tooltip::Tip::text(dc_intl::t("JVwWva"), super::tooltip::Side::Top),
                            cp_btn,
                            cx,
                        ));
                    }
                    // 802880 `th`: Connect Account, when the application has a connection entry point and this
                    // account has no linked account for it (the token fetch is not made: an unlinked account is
                    // assumed, so the button shows).
                    if let Some(url) = connect_url.clone() {
                        stack = stack.child(
                            div()
                                .id(SharedString::from(format!(
                                    "activity-connect-{user}-{index}-{large}"
                                )))
                                .h(px(32.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.follow_link(&url, "Connect Account", cx)
                                }))
                                .child(icon("activity_game", 24., "--text-strong"))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(strong)
                                        .child("Connect Account"),
                                ),
                        );
                    }
                    // Module 607781: single distributor -> direct storefront CTA; multiple distributors ->
                    // "Play" (`nSHoxC`) dropdown button opening "Store options" (`3XhYOS`) menu.
                    if distributors.len() == 1 {
                        let opt = distributors[0].clone();
                        let url = opt.url.clone();
                        let label_for_click = opt.single_label.clone();
                        stack = stack.child(
                            div()
                                .id(SharedString::from(format!(
                                    "activity-dist-{}-{user}-{index}-{large}",
                                    opt.distributor
                                )))
                                .h(px(32.))
                                .px(px(11.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--control-secondary-border-default"))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.follow_link(&url, &label_for_click, cx)
                                }))
                                .child(icon(opt.icon_name, 16., "--text-strong"))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(strong)
                                        .child(opt.single_label),
                                ),
                        );
                    } else if distributors.len() > 1 {
                        let menu_key = format!("activity-dist-menu-{user}-{index}-{large}");
                        let menu_open = self.distributor_menu.as_deref() == Some(menu_key.as_str());
                        let key_click = menu_key.clone();
                        stack = stack.child(
                            div()
                                .relative()
                                .child(
                                    div()
                                        .id(SharedString::from(menu_key.clone()))
                                        .h(px(32.))
                                        .px(px(11.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--control-secondary-border-default"))
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-secondary-background-hover"))
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(4.))
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            if this.distributor_menu.as_deref()
                                                == Some(key_click.as_str())
                                            {
                                                this.distributor_menu = None;
                                            } else {
                                                this.distributor_menu = Some(key_click.clone());
                                            }
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(strong)
                                                .child(dc_intl::t("nSHoxC")),
                                        )
                                        .child(icon("chevron_down", 14., "--text-strong")),
                                )
                                .when(menu_open, |wrap| {
                                    wrap.child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "{menu_key}-popover"
                                            )))
                                            .occlude()
                                            .absolute()
                                            .left_0()
                                            .right_0()
                                            .top(px(36.))
                                            .p(px(6.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-surface-highest"))
                                            .shadow(vec![gpui::BoxShadow {
                                                color: gpui::hsla(0., 0., 0., 0.32),
                                                offset: gpui::point(px(0.), px(6.)),
                                                blur_radius: px(14.),
                                                spread_radius: px(0.),
                                                inset: false,
                                            }])
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .px(px(8.))
                                                    .py(px(4.))
                                                    .text_size(px(11.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("3XhYOS")),
                                            )
                                            .children(distributors.into_iter().map(|opt| {
                                                let url = opt.url.clone();
                                                let lbl = opt.menu_label.clone();
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "{menu_key}-{}",
                                                        opt.distributor
                                                    )))
                                                    .px(px(8.))
                                                    .py(px(6.))
                                                    .rounded(px(4.))
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-strong"))
                                                    .child(icon(opt.icon_name, 16., "--text-strong"))
                                                    .child(opt.menu_label)
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            cx.stop_propagation();
                                                            this.distributor_menu = None;
                                                            this.follow_link(&url, &lbl, cx);
                                                            cx.notify();
                                                        },
                                                    ))
                                            })),
                                    )
                                }),
                        );
                    }
                    actions = Some(stack.into_any_element());
                } else if let Some(url) = (connect_offer.is_none() && !own && !bot)
                    .then(|| platform_presence::stream_url(activity))
                    .flatten()
                {
                    // 802880 `tV`: a Twitch or YouTube stream gets Watch (`I6JG46`), which opens the stream.
                    actions = Some(
                        div()
                            .id(SharedString::from(format!(
                                "activity-watch-{user}-{index}-{large}"
                            )))
                            .when(!large, |this| this.w_full())
                            .px(px(11.))
                            .h(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.follow_link(&url, "", cx);
                            }))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(strong)
                            .child(dc_intl::t("I6JG46"))
                            .into_any_element(),
                    );
                } else if let Some(platform) = connect_offer {
                    // 802880 `tP`: "Connect Xbox" / "Connect PlayStation Network" (`XWSHTb`), which opens that
                    // platform's link dialog (module 80320).
                    let kind = platform.kind;
                    let (glyph, size) = if kind == "xbox" {
                        ("activity_connect_xbox", 16.)
                    } else {
                        ("activity_connect_playstation", 18.)
                    };
                    actions = Some(
                        div()
                            .id(SharedString::from(format!(
                                "activity-connect-platform-{user}-{index}-{large}"
                            )))
                            .when(!large, |this| this.w_full())
                            // `$.FD` (993401): the secondary `sm` button of 821609, 1px border.
                            .px(px(11.))
                            .h(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.open_connection_dialog(kind, cx);
                            }))
                            .child(icon(glyph, size, "--text-strong"))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(strong)
                                    .child(dc_intl::format(
                                        "XWSHTb",
                                        &[("platform", platform.name.into())],
                                    )),
                            )
                            .into_any_element(),
                    );
                }
                if spotify && !own && activity["flags"].as_u64().unwrap_or_default() & 48 != 0 {
                    actions = Some(self.spotify_buttons(user, activity, surface, &palette, cx));
                }
                if bare {
                    actions = None;
                }
                actions = actions.map(|actions| div().mt(px(4.)).child(actions).into_any_element());
                if large {
                    details = details.children(actions.take());
                }
                let card = div()
                    .id(SharedString::from(format!(
                        "live-activity-{user}-{index}-{large}"
                    )))
                    .when(index == 0, |this| this.parity("live-activity-card"))
                    .p(px(if large { 12. } else { 8. }))
                    .rounded(px(if large { 12. } else { 8. }))
                    .bg(card_background)
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .overflow_hidden()
                                    .text_size(px(if surface == Surface::Sidebar { 14. } else { 12. }))
                                    .line_height(px(if surface == Surface::Sidebar { 18. } else { 16. }))
                                    .font_weight(FontWeight(500.))
                                    .text_color(strong)
                                    .child(heading)
                                    // `platformIcon_Bu4Al8`: the platform's white mark, 12px, in `--icon-muted`.
                                    .when(header_platform.is_some(), |header| {
                                        header.child(
                                            div()
                                                .when(spotify, |icon| icon.parity("live-spotify-platform"))
                                                .size(px(12.))
                                                .flex_none()
                                                .children(header_icon.map(|picture| {
                                                    svg().data(&picture.bytes).size_full().text_color(palette.token("--icon-muted"))
                                                })),
                                        )
                                    }),
                            )
                            .when(more, |this| {
                                let key = SharedString::from(format!("activity-more-{user}-{index}-{large}"));
                                let clicked = key.clone();
                                let right_clicked = key.clone();
                                let keyboard = key.clone();
                                let focus = self.stop(key.as_ref(), cx).tab_index(0).tab_stop(true);
                                let ringed = self.keyboard_mode() && self.keys.stops.has_focus(&focus);
                                let key_focus = focus.clone();
                                let popout = surface == Surface::Popout;
                                let (game_click, game_right, game_key) = (game.clone(), game.clone(), game.clone());
                                this.child(
                                    div()
                                        .w(px(20.))
                                        .h(px(if surface == Surface::Sidebar { 18. } else { 16. }))
                                        .flex_none()
                                        .flex()
                                        .items_start()
                                        .justify_start()
                                        .child(self.with_tip(
                                            key,
                                            super::tooltip::Tip::text(dc_intl::tr("More"), super::tooltip::Side::Top),
                                            div()
                                                .id(SharedString::from(format!("activity-menu-button-{user}-{index}-{large}")))
                                                .size(px(16.))
                                                .relative()
                                                .track_focus(&focus)
                                                .tab_index(0)
                                                .when(ringed, |button| button.child(super::keyboard::hollow_ring(4.)))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.open_live_activity_menu(user, spotify, game_click.clone(), popout, &clicked, window, cx);
                                                }))
                                                .on_mouse_up(gpui::MouseButton::Right, cx.listener(move |this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.open_live_activity_menu(user, spotify, game_right.clone(), popout, &right_clicked, window, cx);
                                                }))
                                                .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                                                    if key_focus.is_focused(window) && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                                        cx.stop_propagation();
                                                        this.open_live_activity_menu(user, spotify, game_key.clone(), popout, &keyboard, window, cx);
                                                    }
                                                }))
                                                .child(icon("more_horizontal", 16., "--interactive-text-default")),
                                            cx,
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(if large { 12. } else { 8. }))
                            .items_center()
                            .when(!bot, |this| {
                                // Module 623671: Activity / App ContentImage tile.
                                // - No `overflow_hidden` on outer box so `small_image` protrudes `-4px, -4px`
                                //   with a 2px card-background cutout border (`24px` at SIZE_60, `32px` at SIZE_72/100).
                                // - Fallback `inventory_unknown_game` icon shown whenever `art.is_none()`
                                //   with `2B/phM` ("App icon") accessible label.
                                let show_fallback = missing_spotify_art || art.is_none();
                                let large_box = div()
                                    .w(px(image_size))
                                    .h(px(poster_height))
                                    .rounded(px(8.))
                                    .overflow_hidden()
                                    .when(show_fallback, |image| {
                                        image
                                            .when(missing_spotify_art, |d| {
                                                d.parity("live-spotify-art-fallback")
                                            })
                                            .child(icon(
                                                "inventory_unknown_game",
                                                image_size,
                                                if profile_theme == dc_tokens::Theme::Light {
                                                    "--black"
                                                } else {
                                                    "--white"
                                                },
                                            ))
                                    })
                                    .children(art.map(|picture| {
                                        img(picture)
                                            .w(px(image_size))
                                            .h(px(poster_height))
                                            .object_fit(gpui::ObjectFit::Cover)
                                            .rounded(px(8.))
                                    }));
                                let large_el = if let Some(tip) = large_tip.clone() {
                                    self.with_tip(
                                        SharedString::from(format!(
                                            "activity-large-art-{user}-{index}-{large}"
                                        )),
                                        super::tooltip::Tip::body(
                                            super::tooltip::Body::Text(tip.into()),
                                            super::tooltip::Side::Top,
                                        ),
                                        large_box,
                                        cx,
                                    )
                                    .into_any_element()
                                } else {
                                    large_box.into_any_element()
                                };
                                let small_el = small.map(|picture| {
                                    let badge = div()
                                        .size(px(small_size))
                                        .rounded_full()
                                        .border_2()
                                        .border_color(card_background)
                                        .bg(card_background)
                                        .overflow_hidden()
                                        .child(
                                            img(picture)
                                                .size_full()
                                                .object_fit(gpui::ObjectFit::Cover)
                                                .rounded_full(),
                                        );
                                    let inner = if let Some(tip) = small_tip.clone() {
                                        self.with_tip(
                                            SharedString::from(format!(
                                                "activity-small-art-{user}-{index}-{large}"
                                            )),
                                            super::tooltip::Tip::body(
                                                super::tooltip::Body::Text(tip.into()),
                                                super::tooltip::Side::Top,
                                            ),
                                            badge,
                                            cx,
                                        )
                                        .into_any_element()
                                    } else {
                                        badge.into_any_element()
                                    };
                                    div()
                                        .absolute()
                                        .right(px(-4.))
                                        .bottom(px(-4.))
                                        .child(inner)
                                });
                                this.child(
                                    div()
                                        .relative()
                                        .w(px(image_size))
                                        .h(px(poster_height))
                                        .flex_none()
                                        .child(large_el)
                                        .children(small_el),
                                )
                            })
                            .child(details),
                    )
                    .children(actions);
                if stacked {
                    div()
                        .relative()
                        .pb(px(8.))
                        .child(
                            div()
                                .id(SharedString::from(format!("activity-stack-{user}")))
                                .absolute()
                                .left(px(8.))
                                .right(px(8.))
                                .bottom(px(0.))
                                .h(px(8.))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .mt(px(-16.))
                                        .h(px(24.))
                                        .rounded(px(8.))
                                        .bg(card_background),
                                )
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.profile = None;
                                    this.open_full_profile_tab(
                                        user,
                                        super::full_profile::Tab::Activity,
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .child(card.relative().occlude())
                        .into_any_element()
                } else {
                    card.into_any_element()
                }
            })
            .collect()
    }
}

pub(super) const ACTIVITY_JOIN_PREFIX: &str = "activity-join:";

/// Action attached to an item produced by `build_activity_menu_items` (module `468389`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ActivityMenuAction {
    Separator,
    InviteToJoin {
        target_user: Id,
        app_id: Id,
        party_id: Option<String>,
    },
    InviteToListen {
        target_user: Id,
        party_id: Option<String>,
    },
    Join {
        user_id: Id,
        session_id: String,
        app_id: Id,
    },
    AskToJoin {
        user_id: Id,
        app_id: Id,
        party_id: Option<String>,
    },
    SpotifyPlay {
        user_id: Id,
    },
    SpotifyListenAlong {
        user_id: Id,
    },
    JoinEmbedded {
        app_id: Id,
    },
}

/// One row emitted by `468389` (`useActivityMenuItems`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActivityMenuItem {
    pub label: String,
    pub subtext: Option<String>,
    pub disabled: bool,
    pub action: ActivityMenuAction,
}

/// Inputs to the pure `468389` activity context menu builder.
#[derive(Clone, Debug, Default)]
pub(crate) struct ActivityMenuCtx {
    pub me_id: Id,
    pub target_user_id: Id,
    pub can_send_messages: bool,
    pub is_voice_or_dm: bool,
    pub own_activities: Vec<Value>,
    pub target_activities: Vec<Value>,
    pub target_embedded_apps: Vec<(Id, String)>,
    pub joining_keys: std::collections::HashSet<String>,
    pub spotify_available: bool,
    pub spotify_disabled_reason: Option<String>,
}

const FLAG_JOIN: u64 = 1 << 1;
const FLAG_SYNC: u64 = 1 << 4;
const FLAG_EMBEDDED: u64 = 1 << 7;
const FLAG_CONTEXTLESS: u64 = 1 << 8;

/// Module `468389` (`useActivityMenuItems`): builds self-invite items, optional separator, and target
/// user Join / Joining / Joined / Ask to Join / Spotify / Embedded Activity items.
pub(crate) fn build_activity_menu_items(ctx: &ActivityMenuCtx) -> Vec<ActivityMenuItem> {
    if ctx.target_user_id == 0
        || ctx.target_user_id == ctx.me_id
        || (!ctx.can_send_messages && !ctx.is_voice_or_dm)
    {
        return Vec::new();
    }
    let mut group1 = Vec::new();
    for act in &ctx.own_activities {
        let kind = act["type"].as_u64().unwrap_or(0);
        let flags = act["flags"].as_u64().unwrap_or(0);
        let name = act["name"].as_str().unwrap_or_default().to_owned();
        let app_id = act["application_id"]
            .as_str()
            .and_then(|s| s.parse::<Id>().ok())
            .unwrap_or(0);
        let party_id = act["party"]["id"].as_str().map(str::to_owned);
        if kind == 0 && (flags & FLAG_JOIN != 0 || flags & FLAG_EMBEDDED != 0) && app_id != 0 {
            group1.push(ActivityMenuItem {
                label: dc_intl::t("3fRySx").to_owned(),
                subtext: (!name.is_empty()).then_some(name),
                disabled: false,
                action: ActivityMenuAction::InviteToJoin {
                    target_user: ctx.target_user_id,
                    app_id,
                    party_id,
                },
            });
        } else if kind == 2 && flags & FLAG_SYNC != 0 {
            group1.push(ActivityMenuItem {
                label: dc_intl::t("5vvGpV").to_owned(),
                subtext: (!name.is_empty()).then_some(name),
                disabled: false,
                action: ActivityMenuAction::InviteToListen {
                    target_user: ctx.target_user_id,
                    party_id,
                },
            });
        }
    }

    let mut group2 = Vec::new();
    for act in &ctx.target_activities {
        let kind = act["type"].as_u64().unwrap_or(0);
        let flags = act["flags"].as_u64().unwrap_or(0);
        let name = act["name"].as_str().unwrap_or_default().to_owned();
        let app_id = act["application_id"]
            .as_str()
            .and_then(|s| s.parse::<Id>().ok())
            .unwrap_or(0);
        let session_id = act["session_id"].as_str().unwrap_or_default().to_owned();
        let party_id = act["party"]["id"].as_str().map(str::to_owned);
        let embedded = flags & FLAG_EMBEDDED != 0;
        let contextless = flags & FLAG_CONTEXTLESS != 0;

        if kind == 0 && flags & FLAG_JOIN != 0 && (!embedded || contextless) && app_id != 0 {
            let same_party = party_id.as_deref().is_some_and(|pid| {
                ctx.own_activities.iter().any(|own| {
                    own["application_id"]
                        .as_str()
                        .and_then(|s| s.parse::<Id>().ok())
                        == Some(app_id)
                        && own["party"]["id"].as_str() == Some(pid)
                })
            });
            let join_key = format!("{}:{app_id}", ctx.target_user_id);
            let joining = ctx.joining_keys.contains(&join_key);
            if same_party {
                group2.push(ActivityMenuItem {
                    label: dc_intl::t("DPfdsq").to_owned(),
                    subtext: (!name.is_empty()).then_some(name),
                    disabled: true,
                    action: ActivityMenuAction::Join {
                        user_id: ctx.target_user_id,
                        session_id,
                        app_id,
                    },
                });
            } else if party_id.is_some() && !session_id.is_empty() {
                group2.push(ActivityMenuItem {
                    label: dc_intl::t(if joining { "bf6Ci7" } else { "VJlc0S" }).to_owned(),
                    subtext: (!name.is_empty()).then_some(name),
                    disabled: joining,
                    action: ActivityMenuAction::Join {
                        user_id: ctx.target_user_id,
                        session_id,
                        app_id,
                    },
                });
            } else {
                group2.push(ActivityMenuItem {
                    label: dc_intl::t("OKsSCR").to_owned(),
                    subtext: (!name.is_empty()).then_some(name),
                    disabled: false,
                    action: ActivityMenuAction::AskToJoin {
                        user_id: ctx.target_user_id,
                        app_id,
                        party_id,
                    },
                });
            }
        } else if kind == 2 && flags & FLAG_SYNC != 0 {
            group2.push(ActivityMenuItem {
                label: dc_intl::t("rRffNz").to_owned(),
                subtext: ctx.spotify_disabled_reason.clone(),
                disabled: !ctx.spotify_available,
                action: ActivityMenuAction::SpotifyPlay {
                    user_id: ctx.target_user_id,
                },
            });
            group2.push(ActivityMenuItem {
                label: dc_intl::t("gXYoq2").to_owned(),
                subtext: ctx.spotify_disabled_reason.clone(),
                disabled: !ctx.spotify_available,
                action: ActivityMenuAction::SpotifyListenAlong {
                    user_id: ctx.target_user_id,
                },
            });
        }
    }
    for (app_id, app_name) in &ctx.target_embedded_apps {
        let join_key = format!("{}:{app_id}", ctx.target_user_id);
        let joining = ctx.joining_keys.contains(&join_key);
        group2.push(ActivityMenuItem {
            label: dc_intl::t(if joining { "bf6Ci7" } else { "4i2vj+" }).to_owned(),
            subtext: (!app_name.is_empty()).then_some(app_name.clone()),
            disabled: joining,
            action: ActivityMenuAction::JoinEmbedded { app_id: *app_id },
        });
    }

    let mut out = group1;
    if !out.is_empty() && !group2.is_empty() {
        out.push(ActivityMenuItem {
            label: String::new(),
            subtext: None,
            disabled: true,
            action: ActivityMenuAction::Separator,
        });
    }
    out.extend(group2);
    out
}

impl Root {
    /// Module `273626` / `468389`: fetches join secret via `GET /users/{u}/sessions/{s}/activities/{a}/1`
    /// and dispatches `ACTIVITY_JOIN` or opens `join_url`.
    pub(super) fn join_user_activity(
        &mut self,
        user_id: Id,
        session_id: String,
        app_id: Id,
        channel_id: Option<Id>,
        cx: &mut Context<Self>,
    ) {
        if session_id.is_empty() || app_id == 0 {
            return;
        }
        let key = format!("{ACTIVITY_JOIN_PREFIX}{user_id}:{app_id}");
        self.joining_activities
            .insert(format!("{user_id}:{app_id}"));
        let mut route = format!(
            "/users/{user_id}/sessions/{}/activities/{app_id}/1",
            component(&session_id)
        );
        if let Some(cid) = channel_id {
            route.push_str(&format!("?channel_id={cid}"));
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(key),
            method: reqwest::Method::GET,
            route,
            body: None,
        });
        cx.notify();
    }

    pub(super) fn activity_join_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let suffix = key.strip_prefix(ACTIVITY_JOIN_PREFIX).unwrap_or_default();
        self.joining_activities.remove(suffix);
        if let Some(url) = body["join_url"].as_str().filter(|s| !s.is_empty()) {
            self.follow_link(url, "", cx);
        } else if let Some(secret) = body["secret"].as_str().filter(|s| !s.is_empty()) {
            let frame = dc_rpc::activity_join_dispatch_frame(secret);
            tracing::info!(event = %frame["evt"], "dispatched RPC ACTIVITY_JOIN frame");
        }
        cx.notify();
    }

    /// Module `308368` / `468389`: sends a JOIN (`1`), LISTEN (`3`), or JOIN_REQUEST (`5` Ask to Join)
    /// activity invite message to a DM or channel.
    pub(super) fn send_activity_invite(
        &mut self,
        target_user: Id,
        channel_id: Option<Id>,
        invite_type: u8,
        app_id: Option<Id>,
        party_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let dest = channel_id.or_else(|| self.dm_channel_for_user(target_user));
        let Some(cid) = dest else {
            self.open_dm_with(target_user, cx);
            return;
        };
        let mut activity = serde_json::json!({ "type": invite_type });
        if let Some(pid) = party_id.filter(|s| !s.is_empty()) {
            activity["party_id"] = serde_json::json!(pid);
        }
        let mut payload = serde_json::json!({
            "content": "",
            "activity": activity,
        });
        if let Some(aid) = app_id.filter(|id| *id != 0) {
            payload["application_id"] = serde_json::json!(aid.to_string());
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: format!("/channels/{cid}/messages"),
            body: Some(payload),
        });
        cx.notify();
    }
}

/// Module 985629 (`CloudPlayCtaButton` / `359549.Ay`): `CLOUD_GAMING_DEMO` application flag.
pub(crate) const CLOUD_GAMING_DEMO_FLAG: u64 = 1 << 16;

/// Module 985629 / 359549: returns the launchable Cloud Play application ID if the application
/// supports embedded Cloud Play demos in the active locale.
pub(crate) fn cloud_play_launch_app_id(application: &Value, locale: &str) -> Option<String> {
    let app_id = application["id"].as_str().filter(|s| !s.is_empty())?;
    let cfg = &application["embedded_activity_config"];
    if let Some(blocked) = cfg["blocked_locales"].as_array()
        && blocked.iter().any(|l| l.as_str() == Some(locale))
    {
        return None;
    }
    if let Some(supported) = cfg["supported_locales"].as_array()
        && !supported.is_empty()
        && !supported.iter().any(|l| l.as_str() == Some(locale))
    {
        return None;
    }
    let flags = application["flags"].as_u64().unwrap_or(0);
    let has_bot = application.get("bot").is_some_and(|b| b.is_object());
    if (flags & CLOUD_GAMING_DEMO_FLAG) != 0 && has_bot {
        return Some(app_id.to_owned());
    }
    if let Some(linked) = application.get("linked_games").and_then(Value::as_array) {
        for g in linked {
            if let Some(id) = g["id"].as_str().filter(|s| !s.is_empty())
                && g.get("bot").is_some_and(|b| b.is_object())
            {
                return Some(id.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_crunchyroll_show_takes_the_tall_poster() {
        let show = json!({"type": 3, "application_id": "981509069309354054"});
        assert_eq!(poster_height(&show, 60.), 90.);
        assert_eq!(poster_height(&show, 100.), 150.);
        let game = json!({"type": 0, "application_id": "1"});
        assert_eq!(poster_height(&game, 60.), 60.);
    }

    #[test]
    fn playback_value_text_floors_the_elapsed_time_to_ten_seconds() {
        let text = playback_value_text(83, 200);
        assert!(text.starts_with("1 minute"), "{text}");
        assert!(
            text.contains("20 seconds of 3 minutes, 20 seconds"),
            "{text}"
        );
    }

    #[test]
    fn a_party_chip_reads_the_state_and_the_party_size() {
        let party = json!({"id": "p", "size": [3, 4]});
        assert_eq!(
            party_text(Some("3 Squad"), &party).as_deref(),
            Some("3 Squad (3 of 4)")
        );
        assert_eq!(party_text(None, &party).as_deref(), Some("3 of 4"));
        assert_eq!(
            party_text(None, &json!({"size": [2, 0]})).as_deref(),
            Some("2")
        );
        assert_eq!(
            party_text(Some("Lobby"), &json!({"id": "p"})).as_deref(),
            Some("Lobby")
        );
        assert_eq!(party_text(None, &json!({})), None);
    }
    #[test]
    fn clocks_accept_wire_strings_cap_at_end_and_switch_after_countdown() {
        let mut a = json!({"timestamps":{"start":"1000","end":"3662000"}});
        assert_eq!(elapsed(&a, 4000000).as_deref(), Some("1:01:01"));
        a["timestamps"]["isCountDown"] = json!(true);
        assert_eq!(elapsed(&a, 3660000).as_deref(), Some("0:02"));
        assert_eq!(elapsed(&a, 3663000).as_deref(), Some("1:01:02"));
        assert_eq!(elapsed(&a, 0).as_deref(), Some("1:01:02"));
    }
    #[test]
    fn connect_account_follows_the_token_lookup() {
        let app = json!({"id": "7", "connection_entrypoint_url": "https://example.com/link"});
        // Not answered yet: nothing shows (`fetched` is false).
        assert_eq!(needs_account_link(&app, None), None);
        assert_eq!(needs_account_link(&app, Some(&Value::Null)), None);
        // Answered without a token for it: shows.
        assert_eq!(
            needs_account_link(&app, Some(&json!([]))).as_deref(),
            Some("https://example.com/link")
        );
        // Linked: gone.
        let tokens = json!([{"id": "1", "application": {"id": "7"}}]);
        assert_eq!(needs_account_link(&app, Some(&tokens)), None);
        // No entry point, no button.
        assert_eq!(
            needs_account_link(&json!({"id": "7"}), Some(&json!([]))),
            None
        );
        // A child application is looked up under its parent.
        let child = json!({"id": "8", "parent_id": "7", "connection_entrypoint_url": "u"});
        assert_eq!(needs_account_link(&child, Some(&tokens)), None);
    }

    #[test]
    fn rich_asset_routes_and_store_links_preserve_encoding() {
        let a = json!({"application_id":"42", "assets":{"large_image":"spotify:cover", "small_image":"mp:external/a/image.gif"}});
        assert_eq!(
            asset_url(&a, "large_image").as_deref(),
            Some("https://i.scdn.co/image/cover")
        );
        assert_eq!(
            asset_url(&a, "small_image").as_deref(),
            Some("https://media.discordapp.net/external/a/image.gif?format=webp&animated=true")
        );
        for (asset, expected) in [
            (
                "mp://discord.com/assets/cover.svg",
                "https://discord.com/assets/cover.svg",
            ),
            (
                "mp:/external/a/cover.webp",
                "https://media.discordapp.net/external/a/cover.webp?animated=true",
            ),
            (
                "mp:external/a/cover.png?width=32",
                "https://media.discordapp.net/external/a/cover.png?width=32",
            ),
        ] {
            assert_eq!(
                asset_url(&json!({"assets":{"large_image":asset}}), "large_image").as_deref(),
                Some(expected)
            );
        }
        assert!(
            steam_url(&json!({"third_party_skus":[{"distributor":"other","id":"1"}]})).is_none()
        );
        assert_eq!(
            steam_url(&json!({"third_party_skus":[{"distributor":"steam","id":"12/34"}]}))
                .as_deref(),
            Some("https://store.steampowered.com/app/12%2F34")
        );
    }

    #[test]
    fn custom_buttons_require_a_session_and_a_url_with_a_host() {
        assert!(metadata_route(1, &json!({"application_id":"2"})).is_none());
        assert_eq!(
            metadata_route(1, &json!({"session_id":"a/b","application_id":"2"})).as_deref(),
            Some("/users/1/sessions/a%2Fb/activities/2/metadata")
        );
        let metadata =
            json!({"button_urls":["https://example.com/game", "javascript:alert(1)", "/relative"]});
        assert_eq!(
            button_url(&metadata, 0).as_deref(),
            Some("https://example.com/game")
        );
        for index in 1..4 {
            assert!(button_url(&metadata, index).is_none());
        }
    }

    #[test]
    fn activity_menu_items_cover_module_468389_branches() {
        // Self user or lacking permission returns empty.
        let mut ctx = ActivityMenuCtx {
            me_id: 1,
            target_user_id: 1,
            can_send_messages: true,
            ..Default::default()
        };
        assert!(build_activity_menu_items(&ctx).is_empty());

        ctx.target_user_id = 2;
        ctx.can_send_messages = false;
        assert!(build_activity_menu_items(&ctx).is_empty());

        // Self PLAYING+JOIN & LISTENING+SYNC + target Joinable + Spotify + Embedded -> separator between groups.
        ctx.can_send_messages = true;
        ctx.own_activities = vec![
            json!({"type": 0, "flags": 2, "application_id": "10", "name": "GameA", "party": {"id": "p1"}}),
            json!({"type": 2, "flags": 16, "name": "Spotify", "party": {"id": "sp1"}}),
        ];
        ctx.target_activities = vec![
            json!({"type": 0, "flags": 2, "application_id": "20", "session_id": "s2", "name": "GameB", "party": {"id": "p2"}}),
            json!({"type": 0, "flags": 2, "application_id": "30", "name": "GameC"}),
            json!({"type": 2, "flags": 16, "name": "Spotify"}),
        ];
        ctx.target_embedded_apps = vec![(40, "Chess".into())];
        ctx.spotify_available = true;
        let items = build_activity_menu_items(&ctx);
        assert!(items.iter().any(|i| i.label == dc_intl::t("3fRySx")));
        assert!(items.iter().any(|i| i.label == dc_intl::t("5vvGpV")));
        assert!(
            items
                .iter()
                .any(|i| i.action == ActivityMenuAction::Separator)
        );
        assert!(items.iter().any(|i| i.label == dc_intl::t("VJlc0S")));
        assert!(items.iter().any(|i| i.label == dc_intl::t("OKsSCR")));
        assert!(items.iter().any(|i| i.label == dc_intl::t("rRffNz")));
        assert!(items.iter().any(|i| i.label == dc_intl::t("gXYoq2")));
        assert!(items.iter().any(|i| i.label == dc_intl::t("4i2vj+")));
        assert!(!dc_intl::t("0OiwfH").is_empty());

        // Joining state disables Join with bf6Ci7 ("Joining").
        ctx.joining_keys.insert("2:20".into());
        let joining_items = build_activity_menu_items(&ctx);
        assert!(
            joining_items
                .iter()
                .any(|i| i.label == dc_intl::t("bf6Ci7") && i.disabled)
        );
    }

    #[test]
    fn distributor_options_607781_priority_and_urls() {
        for k in ["s7+3um", "QpN/Iz", "L1N2gh", "FsANs4", "nSHoxC", "3XhYOS"] {
            assert!(dc_intl::has_key(k), "missing 607781 key {k}");
        }
        let app = json!({
            "third_party_skus": [
                {"distributor": "steam", "id": "730"},
                {"distributor": "xbox_game_pass", "id": "9NBLGGH2JHXJ"}
            ]
        });
        let opts = distributor_options(&app);
        assert_eq!(opts.len(), 2);
        // Priority order: Xbox Game Pass before Steam.
        assert_eq!(opts[0].distributor, "xbox_game_pass");
        assert_eq!(
            opts[0].url,
            "https://www.xbox.com/games/store/title/9NBLGGH2JHXJ?ocid=discord7937464"
        );
        assert_eq!(opts[1].distributor, "steam");
        assert_eq!(opts[1].url, "https://store.steampowered.com/app/730");
    }

    #[test]
    fn content_image_623671_sizes_and_intl() {
        assert!(dc_intl::has_key("2B/phM"));
        assert_eq!(ContentImageSize::Size60.main_px(), 60.);
        assert_eq!(ContentImageSize::Size60.small_px(), 24.);
        assert_eq!(ContentImageSize::Size72.main_px(), 72.);
        assert_eq!(ContentImageSize::Size72.small_px(), 32.);
        assert_eq!(ContentImageSize::Size100.main_px(), 100.);
        assert_eq!(ContentImageSize::Size100.small_px(), 32.);
    }

    #[test]
    fn cloud_play_cta_985629_eligibility_and_intl() {
        assert_eq!(dc_intl::t("jaYS/h"), "Try Game");
        assert_eq!(dc_intl::t("JVwWva"), "Try with no install required");
        let demo = json!({
            "id": "555",
            "flags": CLOUD_GAMING_DEMO_FLAG,
            "bot": {"id": "556"},
            "embedded_activity_config": {"blocked_locales": ["ja"]}
        });
        assert_eq!(
            cloud_play_launch_app_id(&demo, "en-US").as_deref(),
            Some("555")
        );
        assert!(cloud_play_launch_app_id(&demo, "ja").is_none());
        let no_bot = json!({"id": "555", "flags": CLOUD_GAMING_DEMO_FLAG});
        assert!(cloud_play_launch_app_id(&no_bot, "en-US").is_none());
    }
}
