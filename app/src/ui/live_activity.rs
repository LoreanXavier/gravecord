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

fn party_text(state: Option<&str>, party: &Value) -> Option<String> {
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

pub(super) fn asset_url(activity: &Value, key: &str) -> Option<String> {
    let asset = field(&activity["assets"], key)?;
    if let Some(hash) = asset.strip_prefix("spotify:") {
        return Some(format!("https://i.scdn.co/image/{}", component(hash)));
    }
    if let Some(path) = asset.strip_prefix("mp:") {
        let mut url = reqwest::Url::parse("https://media.discordapp.net/")
            .ok()?
            .join(path)
            .ok()?;
        if path.to_ascii_lowercase().ends_with(".gif") {
            url.query_pairs_mut()
                .append_pair("format", "webp")
                .append_pair("animated", "true");
        } else if path.to_ascii_lowercase().ends_with(".webp")
            || path.to_ascii_lowercase().ends_with(".avif")
        {
            url.query_pairs_mut().append_pair("animated", "true");
        }
        return Some(url.into());
    }
    if asset.contains(':') {
        return None;
    }
    let app = field(activity, "application_id")?;
    Some(format!(
        "https://cdn.discordapp.com/app-assets/{app}/{asset}.png?size=160"
    ))
}
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

fn steam_url(application: &Value) -> Option<String> {
    let sku = application["third_party_skus"]
        .as_array()?
        .iter()
        .find(|sku| sku["distributor"] == "steam")?;
    let id = field(sku, "id").or_else(|| field(sku, "sku"))?;
    Some(format!(
        "https://store.steampowered.com/app/{}",
        component(id)
    ))
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
                let url = format!(
                    "https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=160&keep_aspect_ratio=false"
                );
                self.request_image(format!("app-icon:{id}"), url);
            }
        }
    }

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
                let image_size = if large { 100. } else { 60. };
                let poster_height = poster_height(activity, image_size);
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
                } else if application.and_then(steam_url).is_some() || connect_url.is_some() {
                    let mut stack = div().flex().flex_col().gap(px(8.));
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
                    if let Some(url) = application.and_then(steam_url) {
                        stack = stack.child(
                            div()
                                .id(SharedString::from(format!(
                                    "activity-steam-{user}-{index}-{large}"
                                )))
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
                                    this.follow_link(&url, "Get on Steam", cx)
                                }))
                                .child(icon("activity_steam", 16., "--text-strong"))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(strong)
                                        .child("Get on Steam"),
                                ),
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
                                this.child(
                                    div()
                                        .relative()
                                        .w(px(image_size))
                                        .h(px(poster_height))
                                        .flex_none()
                                        .rounded(px(8.))
                                        .overflow_hidden()
                                        .when(missing_spotify_art, |image| {
                                            image.parity("live-spotify-art-fallback").child(
                                                icon("inventory_unknown_game", image_size, if profile_theme == dc_tokens::Theme::Light { "--black" } else { "--white" }),
                                            )
                                        })
                                        .children(art.map(|picture| {
                                            img(picture)
                                                .w(px(image_size))
                                                .h(px(poster_height))
                                                .object_fit(gpui::ObjectFit::Cover)
                                                .rounded(px(8.))
                                        }))
                                        .children(small.map(|picture| {
                                            img(picture)
                                                .absolute()
                                                .right(px(0.))
                                                .bottom(px(0.))
                                                .size(px(20.))
                                                .rounded_full()
                                        })),
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
}
