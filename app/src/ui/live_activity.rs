//! Live profile cards: modules 802880, 960549, 583846 and 913659.
use super::{Root, color, icon};
use crate::{net::Request, parity::Parity};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, SharedString, div, img, prelude::*, px,
    relative, rgba,
};
use serde_json::Value;
use std::time::Duration;

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

fn millis(value: &Value) -> Option<i64> {
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
fn asset_url(activity: &Value, key: &str) -> Option<String> {
    let asset = field(&activity["assets"], key)?;
    if let Some(hash) = asset.strip_prefix("spotify:") {
        return Some(format!("https://i.scdn.co/image/{}", component(hash)));
    }
    if let Some(path) = asset.strip_prefix("mp:") {
        let mut url = format!(
            "https://media.discordapp.net/{}",
            path.trim_start_matches('/')
        );
        if path.to_ascii_lowercase().ends_with(".gif") {
            url.push_str("?format=webp&animated=true");
        } else if path.to_ascii_lowercase().ends_with(".webp")
            || path.to_ascii_lowercase().ends_with(".avif")
        {
            url.push_str("?animated=true");
        }
        return Some(url);
    }
    if asset.contains(':') {
        return None;
    }
    let app = field(activity, "application_id")?;
    Some(format!(
        "https://cdn.discordapp.com/app-assets/{app}/{asset}.png?size=160"
    ))
}
fn heading(activity: &Value) -> String {
    let kind = activity["type"].as_u64().unwrap_or(0);
    let base = match kind {
        1 => "Streaming",
        2 => "Listening to",
        3 => "Watching",
        5 => "Competing in",
        _ => "Playing",
    };
    if matches!(kind, 2 | 3 | 5) && !activity["details"].is_null() {
        format!("{base} {}", activity["name"].as_str().unwrap_or_default())
    } else {
        base.to_owned()
    }
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
        {
            Some(Duration::from_secs(
                if !self.notification_focused || self.pref_on("reduced_motion", false) {
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
        let activities: Vec<Value> = self
            .live_users()
            .into_iter()
            .filter(|user| !self.is_blocked(*user) && !self.stores.is_ignored(*user))
            .filter_map(|user| self.stores.presence.get(user, self.guild))
            .flat_map(|presence| presence.visible_activities().into_iter().cloned())
            .collect();
        for activity in &activities {
            for key in ["large_image", "small_image"] {
                if let Some(url) = asset_url(activity, key) {
                    self.request_image(url.clone(), url);
                }
            }
            if let Some(app) = field(activity, "application_id") {
                let key = format!("presence-app:{app}");
                if !self.fetched.contains_key(&key) {
                    self.fetched.insert(key.clone(), Value::Null);
                    let _ = self.requests.send(Request::Fetch {
                        key,
                        route: format!("/applications/public?application_ids={app}"),
                    });
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
        large: bool,
        strong: Hsla,
        subtle: Hsla,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.is_blocked(user) || self.stores.is_ignored(user) {
            return Vec::new();
        }
        let Some(presence) = self.stores.presence.get(user, self.guild) else {
            return Vec::new();
        };
        let bot = self.stores.users.get(&user).is_some_and(|u| u.bot);
        let card_background = if self.dm_palette(user).themed {
            color("--background-mod-normal")
        } else {
            color("--user-profile-overlay-background")
        };
        let now = chrono::Utc::now().timestamp_millis();
        let activities = presence.visible_activities();
        let stacked = !large && activities.len() > 1;
        activities
            .into_iter()
            .take(if large { usize::MAX } else { 1 })
            .enumerate()
            .map(|(index, activity)| {
                let app_id = field(activity, "application_id");
                let application = app_id
                    .and_then(|id| self.fetched.get(&format!("presence-app:{id}")))
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first());
                let art = asset_url(activity, "large_image")
                    .and_then(|url| self.images.get(&url))
                    .cloned()
                    .or_else(|| {
                        app_id.and_then(|id| self.images.get(&format!("app-icon:{id}")).cloned())
                    });
                let small = asset_url(activity, "small_image")
                    .and_then(|url| self.images.get(&url))
                    .cloned();
                let kind = activity["type"].as_u64().unwrap_or(0);
                let spotify = kind == 2 && activity["name"] == "Spotify";
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
                let extra = if kind == 0 {
                    field(activity, "state")
                } else if matches!(kind, 2 | 3) {
                    None
                } else {
                    field(&activity["assets"], "large_text")
                };
                let image_size = if large { 100. } else { 60. };
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
                        .text_color(subtle)
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
                                    .text_color(strong)
                                    .child(title.to_owned()),
                            )
                            .children(subtitle.map(text_line))
                            .children(extra.map(|s| text_line(s.to_owned()))),
                    );
                if let Some(duration) = duration {
                    details = details.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon(glyph, 12., "--text-feedback-positive"))
                            .child(
                                div()
                                    .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![
                                        ("kern".into(), 1),
                                        ("tnum".into(), 1),
                                    ])))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-feedback-positive"))
                                    .child(duration),
                            ),
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
                    details = details.child(
                        div()
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
                                    .bg(rgba(0x97979f29))
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
                            let label = label.as_str()?.to_owned();
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
                } else if let Some(url) = application.and_then(steam_url) {
                    actions = Some(
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
                            )
                            .into_any_element(),
                    );
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
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(strong)
                            .child(heading(activity)),
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
                                        .size(px(image_size))
                                        .flex_none()
                                        .rounded(px(8.))
                                        .overflow_hidden()
                                        .children(art.map(|picture| {
                                            img(picture).size(px(image_size)).rounded(px(8.))
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
    fn clocks_accept_wire_strings_cap_at_end_and_switch_after_countdown() {
        let mut a = json!({"timestamps":{"start":"1000","end":"3662000"}});
        assert_eq!(elapsed(&a, 4000000).as_deref(), Some("1:01:01"));
        a["timestamps"]["isCountDown"] = json!(true);
        assert_eq!(elapsed(&a, 3660000).as_deref(), Some("0:02"));
        assert_eq!(elapsed(&a, 3663000).as_deref(), Some("1:01:02"));
        assert_eq!(elapsed(&a, 0).as_deref(), Some("1:01:02"));
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
