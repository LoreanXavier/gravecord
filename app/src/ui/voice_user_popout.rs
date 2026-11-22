//! VoiceUserPopout (module 772800), opened from a voice user row of the channel list (module 340851, with the
//! preview state of module 588224): 100ms after the pointer enters the row (`setTimeout` of 340851) and 50ms more
//! (`J_(50)` of 588224) the popout opens 17px to the right of the row, unless the row is the account on another
//! client or the row's profile popout is open; it closes 175ms after the pointer has left the row and the popout.
//!
//! With the guild in `2026-04_voice_user_duration` (module 402313, `T`) it starts with `F`: the avatar, the name and
//! how long the person has been in the channel (`connectedAt`); then, or without `T` alone, the person's cards. With
//! neither, nothing opens. Styles: `*_utcclU` (module 8936, stylesheet 847810) over `popover_81IzEA` (module 489387).

use super::member_avatars::HIDDEN_STATUS;
use super::voice_badge::HoverCard;
use super::{Root, color};
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, BoxShadow, Context, FontWeight, IntoElement, SharedString, div, hsla, point,
    prelude::*, px,
};
use serde_json::Value;

/// Module 402313: treatment 1 shows how long people have been in voice; the default is off.
pub(super) const DURATION_EXPERIMENT: &str = "2026-04_voice_user_duration";
/// 100ms of the row's `setTimeout`, then 50ms of `J_(50)` in module 588224.
pub(super) const OPEN_AFTER: u64 = 150;
/// `spacing: 17` of the row's popout.
pub(super) const SPACING: f32 = 17.;
/// `ActivityFlags.EMBEDDED` (module 652215 `jUm`).
const EMBEDDED: u64 = 1 << 8;

/// Module 845056 `N`: a game or something watched that says anything about itself, or anything listened to.
pub(super) fn voice_activity_shown(activity: &Value) -> bool {
    match activity["type"].as_u64() {
        Some(0 | 3) => [
            "name",
            "application_id",
            "assets",
            "state",
            "details",
            "party",
        ]
        .iter()
        .any(|key| !activity[*key].is_null()),
        Some(2) => true,
        _ => false,
    }
}

/// Module 765379: an activity flagged `EMBEDDED` (an Activity running in a call) is left to the embedded app cards.
fn embedded(activity: &Value) -> bool {
    activity["flags"].as_u64().unwrap_or(0) & EMBEDDED == EMBEDDED
}

/// The row's popout state that the card needs: the person, the channel and its guild.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct VoiceUserTarget {
    pub guild: Id,
    pub channel: Id,
    pub user: Id,
}

impl Root {
    /// The person whose popout is open, for the live cards' assets (`live_users`).
    pub(super) fn voice_popout_user(&self) -> Option<Id> {
        match self.voice_hover_open()? {
            HoverCard::User(target) => Some(target.user),
            HoverCard::Channel { .. } => None,
        }
    }

    /// The pointer entered (`inside`) or left a voice user row (`ee` and `eI` of module 340851).
    pub(super) fn voice_row_hover(
        &mut self,
        target: VoiceUserTarget,
        inside: bool,
        cx: &mut Context<Self>,
    ) {
        let key = SharedString::from(format!("voice-member-{}", target.user));
        if !inside {
            self.voice_hover_leave(&key, cx);
            return;
        }
        // `isSelfOnOtherClient`: the account's own row for another session has no popout.
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let other_client = me == Some(target.user)
            && self
                .stores
                .voice_members(target.guild, target.channel)
                .iter()
                .find(|state| state.user_id == target.user)
                .is_some_and(|state| {
                    self.stores.session_id.as_deref() != Some(state.session_id.as_str())
                });
        // `shouldShowHoverPopout && !em`: not while the row's own profile popout is open.
        let profile_open = self
            .profile
            .as_ref()
            .is_some_and(|open| open.user == target.user);
        if other_client || profile_open {
            return;
        }
        self.voice_hover_enter(key, HoverCard::User(target), OPEN_AFTER, cx);
    }

    /// Module 727353 `P` (`streamPreviewWrapper_1v6-1l`): Voice User Activities stream card with
    /// `HV97U/` ("Streaming Now"), small LIVE badge, 142px preview (`stream_previews`), and `711589.K` hover CTA.
    fn voice_user_stream_card(
        &self,
        guild: Id,
        channel: Id,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let streaming = self
            .stores
            .voice_members(guild, channel)
            .iter()
            .any(|s| s.user_id == user && s.self_stream);
        if !streaming {
            return None;
        }
        let me = self.stores.current_user.as_ref().map(|u| u.id);
        let is_own = me == Some(user);
        let is_watching = !is_own && self.video_stream_of(user).is_some();
        let action_label = super::live_activity::stream_action_label(is_own, is_watching, true);
        let preview_key = format!("stream-preview:guild:{guild}:{channel}:{user}");
        let thumb = self.images.get(&preview_key).cloned();
        let can_watch = !is_own && !is_watching;
        let group = gpui::SharedString::from(format!("voice-stream-card-{user}"));
        let preview_box = div()
            .id(gpui::SharedString::from(format!(
                "voice-stream-preview-{user}"
            )))
            .group(group.clone())
            .relative()
            .w_full()
            .h(px(142.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .when(can_watch, |d| {
                d.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.watch_stream_joining(guild, channel, user, cx);
                    }))
            })
            .children(thumb.map(|img_arc| {
                gpui::img(img_arc)
                    .size_full()
                    .rounded(px(8.))
                    .object_fit(gpui::ObjectFit::Cover)
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(8.))
                    .bg(color("--opacity-black-60"))
                    .opacity(0.)
                    .group_hover(group, |s| s.opacity(1.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(action_label),
            );
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("HV97U/")),
                        )
                        .child(
                            div()
                                .px(px(6.))
                                .py(px(1.))
                                .rounded_full()
                                .bg(color("--status-danger"))
                                .text_size(px(10.))
                                .line_height(px(14.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child("LIVE"),
                        ),
                )
                .child(preview_box)
                .into_any_element(),
        )
    }
}

/// Module 804630 (`_Q(activityType)`): Voice User Activity card header label (`J4MQFX`, `xeqvTg`, `ep6av7`, `1+boPi`).
pub(super) fn voice_activity_header(activity_type: u64) -> &'static str {
    match activity_type {
        0 | 5 => dc_intl::t("J4MQFX"), // Playing Now
        3 => dc_intl::t("xeqvTg"),     // Watching Now
        2 => dc_intl::t("ep6av7"),     // Listening Now
        _ => dc_intl::t("1+boPi"),     // Happening Now
    }
}

/// Module 804630 (`Gf`): resolved activity summary for Voice User Popout (`329554` / `804630.vP`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ResolvedVoiceActivity {
    pub is_spotify: bool,
    pub activity_type: u64,
    pub header: &'static str,
    pub title: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub sync_id: Option<String>,
    pub image_url: Option<String>,
}

/// Module 804630 (`Gf`): resolves a raw presence activity JSON into a voice user popout card model.
pub(super) fn resolve_voice_activity(activity: &serde_json::Value) -> ResolvedVoiceActivity {
    let is_spotify = dc_state::now_playing::is_spotify(activity)
        || (activity["type"].as_u64() == Some(2)
            && activity["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("spotify:")));
    let activity_type = if is_spotify {
        2
    } else {
        activity["type"].as_u64().unwrap_or(0)
    };
    let name = activity["name"].as_str().unwrap_or_default();
    let details = activity["details"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let state = activity["state"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let title = if is_spotify {
        details.clone().unwrap_or_else(|| name.to_owned())
    } else {
        name.to_owned()
    };
    let subtitle = if is_spotify { state.clone() } else { details };
    let detail = if is_spotify { None } else { state };
    let sync_id = activity["sync_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let image_url = activity["assets"]["large_image"]
        .as_str()
        .or_else(|| activity["assets"]["small_image"].as_str())
        .and_then(|raw| {
            if let Some(hash) = raw.strip_prefix("spotify:") {
                Some(format!("https://i.scdn.co/image/{hash}"))
            } else if let Some(ext) = raw.strip_prefix("mp:external/") {
                Some(format!("https://media.discordapp.net/external/{ext}"))
            } else {
                activity["application_id"].as_str().map(|app| {
                    format!("https://cdn.discordapp.com/app-assets/{app}/{raw}.png?size=128")
                })
            }
        });
    ResolvedVoiceActivity {
        is_spotify,
        activity_type,
        header: voice_activity_header(activity_type),
        title,
        subtitle,
        detail,
        sync_id,
        image_url,
    }
}

impl Root {
    /// Module 804630 (`vP` Spotify / `329554.v` compact card + `Tf` 32x32 artwork + `_Q` header):
    /// renders a compact voice user activity card (`-kuxsn`) for a resolved activity.
    fn voice_user_activity_card_804630(
        &self,
        resolved: &ResolvedVoiceActivity,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let thumb = resolved
            .image_url
            .as_deref()
            .and_then(|u| self.images.get(u).cloned());
        let open_spotify = resolved
            .is_spotify
            .then(|| resolved.sync_id.clone())
            .flatten();
        div()
            .id(gpui::SharedString::from(format!(
                "voice-act-804630-{index}"
            )))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child(resolved.header),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        // `Tf` (`image_-kuxsn`): 32x32 rounded-sm artwork.
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(thumb.map(|im| {
                                gpui::img(im).size_full().object_fit(gpui::ObjectFit::Cover)
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .when_some(open_spotify, |el, track_id| {
                                        el.cursor_pointer().on_mouse_down(
                                            gpui::MouseButton::Left,
                                            cx.listener(move |_, _, _, cx| {
                                                cx.open_url(&format!(
                                                    "https://open.spotify.com/track/{track_id}"
                                                ));
                                            }),
                                        )
                                    })
                                    .child(resolved.title.clone()),
                            )
                            .when_some(resolved.subtitle.clone(), |col, sub| {
                                col.child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(sub),
                                )
                            })
                            .when_some(resolved.detail.clone(), |col, det| {
                                col.child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(det),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Root {
    /// Module 772800 `A`, or `None` when it renders nothing (`!J`).
    /// Combines the stream card (`727353.P`) and voice user activity cards (`804630` `_Q`/`Gf`/`Tf`/`vP`).
    /// Note: embedded Activity cards (module 685399 through 329554) and the voice channel game card
    /// (module 655724 `B`, `$` of 208284/729829) are not yet drawn here.
    pub(super) fn voice_user_card(
        &self,
        target: VoiceUserTarget,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let VoiceUserTarget {
            guild,
            channel,
            user,
        } = target;
        let duration = self.guild_experiment_on(DURATION_EXPERIMENT, guild);
        let activities: Vec<ResolvedVoiceActivity> = self
            .stores
            .presence
            .get(user, Some(guild))
            .map(|presence| {
                presence
                    .visible_activities()
                    .iter()
                    .filter(|activity| voice_activity_shown(activity) && !embedded(activity))
                    .map(|activity| resolve_voice_activity(activity))
                    .collect()
            })
            .unwrap_or_default();
        let mut cards: Vec<AnyElement> = Vec::new();
        // Module 727353 `P` (Voice User Activities stream card): "Streaming Now" (`HV97U/`) + LIVE badge +
        // 142px rounded preview box with `--opacity-black-60` hover CTA (`711589.K`) and Watch Stream click.
        if let Some(stream_card) = self.voice_user_stream_card(guild, channel, user, cx) {
            cards.push(stream_card);
        }
        for (idx, resolved) in activities.iter().enumerate() {
            cards.push(self.voice_user_activity_card_804630(resolved, idx, cx));
        }
        let has_cards = !cards.is_empty();
        if !has_cards && !duration {
            return None;
        }
        // `scroller_utcclU` (inside `experimentContainer_utcclU`) or `container_utcclU`: the cards 12px apart, each
        // after the first under a 1px rule with 12px above it.
        let rule = if duration {
            "--border-subtle"
        } else {
            "--border-muted"
        };
        let list = |cards: Vec<AnyElement>| {
            cards.into_iter().enumerate().map(move |(index, card)| {
                div()
                    .when(index > 0, |this| {
                        this.border_t_1().border_color(color(rule)).pt(px(12.))
                    })
                    .child(card)
                    .into_any_element()
            })
        };
        let popover = || {
            // `popover_81IzEA`: `--background-surface-high`, `--radius-md`, a 1px `--border-subtle` ring inside and
            // `--shadow-high`.
            div()
                .bg(color("--background-surface-high"))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .flex()
                .flex_col()
        };
        if !duration {
            // `container_utcclU`: 280px, 16px inside, at most 90% of the window tall.
            return Some(
                popover()
                    .parity("voice-user-popout")
                    .w(px(280.))
                    .p(px(16.))
                    .gap(px(12.))
                    .children(list(cards))
                    .into_any_element(),
            );
        }
        let header = self.voice_user_duration(guild, channel, user, cx)?;
        Some(
            popover()
                .parity("voice-user-popout")
                .w(px(280.))
                .child(header)
                .when(has_cards, |this| {
                    // `scroller_utcclU`: a popover of its own with square top corners, 16px inside, at most 404px.
                    this.child(
                        popover()
                            .rounded_t(px(0.))
                            .max_h(px(404.))
                            .p(px(16.))
                            .gap(px(12.))
                            .children(list(cards)),
                    )
                })
                .into_any_element(),
        )
    }

    /// `F`: `durationContainer_utcclU` on `--opacity-4` with the top corners round, 12px by 16px inside: the 32px avatar
    /// (streaming, or the status unless offline), then the name (`body-sm/medium`, the member's nick or the display
    /// name, one line) and the time since `connectedAt`, 12px apart and pushed to the two ends.
    fn voice_user_duration(
        &self,
        guild: Id,
        channel: Id,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let known = self
            .stores
            .users
            .get(&user)
            .cloned()
            .or_else(|| self.stores.current_user.clone().filter(|me| me.id == user))?;
        let name = self
            .stores
            .members
            .get(guild, user)
            .and_then(|member| member.nick.clone())
            .unwrap_or_else(|| known.display_name().to_owned());
        let status = match self.stores.status(user) {
            "offline" | "invisible" => HIDDEN_STATUS,
            status => status,
        };
        let connected = self
            .stores
            .voice_members(guild, channel)
            .iter()
            .find(|state| state.user_id == user)
            .and_then(|state| state.connected_at)
            .map(|seconds| seconds * 1000);
        Some(
            div()
                .parity("voice-user-duration")
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .py(px(12.))
                .px(px(16.))
                .rounded_t(px(12.))
                .bg(color("--opacity-4"))
                .child(self.member_avatar(&known, status, false, false, cx))
                .child(
                    div()
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(SharedString::from(name)),
                        )
                        .children(connected.map(|start| self.elapsed_timer(start, cx))),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::voice_activity_shown;
    use serde_json::json;

    #[test]
    fn games_and_watching_need_something_listening_needs_nothing() {
        assert!(voice_activity_shown(&json!({"type": 0, "name": "Game"})));
        assert!(!voice_activity_shown(&json!({"type": 3})));
        assert!(voice_activity_shown(&json!({"type": 2})));
        assert!(!voice_activity_shown(
            &json!({"type": 4, "state": "custom"})
        ));
        assert!(!voice_activity_shown(&json!({"type": 1, "name": "Stream"})));
    }

    #[test]
    fn voice_user_activity_helpers_804630() {
        use super::{resolve_voice_activity, voice_activity_header};
        assert_eq!(voice_activity_header(0), "Playing Now");
        assert_eq!(voice_activity_header(5), "Playing Now");
        assert_eq!(voice_activity_header(3), "Watching Now");
        assert_eq!(voice_activity_header(2), "Listening Now");
        assert_eq!(voice_activity_header(99), "Happening Now");

        let spotify = resolve_voice_activity(&json!({
            "id": "spotify:1",
            "type": 2,
            "name": "Spotify",
            "details": "Track Title",
            "state": "Artist Name",
            "sync_id": "abc123",
            "assets": { "large_image": "spotify:deadbeef" }
        }));
        assert!(spotify.is_spotify);
        assert_eq!(spotify.header, "Listening Now");
        assert_eq!(spotify.title, "Track Title");
        assert_eq!(spotify.subtitle.as_deref(), Some("Artist Name"));
        assert_eq!(
            spotify.image_url.as_deref(),
            Some("https://i.scdn.co/image/deadbeef")
        );
    }
}
