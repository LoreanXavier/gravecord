//! Legacy `UserActivity` card (module 663249) across `ActivityFeed`, `StreamPreview`, and `VoiceChannel` layouts.
//!
//! Formats all 20 activity headers, platform relays (`PS4`/`PS5`, `Xbox`, `Samsung Galaxy`, session providers),
//! guild/embedded/streaming headers, Twitch `playing {game}` overlay, clickable Spotify track/artists/album rows,
//! party sizes (`(count of max)` / `(count)`), Stage speaker/listener summaries, and channel details.

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::borrow::Cow;

/// Official `663249.O` (`UserActivity.Types`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ActivityCardKind {
    ActivityFeed,
    StreamPreview,
    VoiceChannel,
}

/// Activity type subset rendered by module 663249 (`ActivityTypes`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ActivityKind {
    Playing,
    Streaming,
    Listening,
    Watching,
    Competing,
    Stage,
    ScreenShare,
}

/// Module 961272 (`UserActivityActions`): action metadata carried by a `UserActivityCardData`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UserActivityActionsSpec {
    pub is_current_user: bool,
    pub is_rich: bool,
    pub platform_connected: bool,
    pub spotify_can_play: bool,
    pub spotify_can_sync: bool,
    pub stage_in_channel: bool,
    pub is_watching_stream: bool,
    pub can_watch_stream: bool,
    pub has_other_active_streams: bool,
    /// Bit 0 (`INSTANCE = 1`) enables the `"Notify Me"` (`vwl1PK`) action.
    pub is_instance: bool,
    /// Custom activity buttons (`activity.buttons[]`: `(label, optional_resolved_url)`).
    pub custom_buttons: Vec<(String, Option<String>)>,
}

/// Module 961272: resolved action button specification across the 5 official priority branches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UserActivityAction {
    ConnectConsole {
        platform: &'static str,
        icon: &'static str,
    },
    SpotifyPlay,
    SpotifyListenAlong,
    StageListenIn {
        disabled: bool,
    },
    WatchStream {
        label: String,
        active: bool,
        disabled: bool,
        show_multi: bool,
    },
    WatchExternal {
        url: String,
    },
    NotifyMe,
    CustomButton {
        index: usize,
        label: String,
        url: Option<String>,
    },
}

/// Module 961272 (`729937`): `GET /users/{userId}/sessions/{sessionId}/activities/{appId}/metadata`.
#[allow(dead_code)]
pub(crate) fn activity_metadata_route(
    user_id: &str,
    session_id: &str,
    application_id: Option<&str>,
) -> String {
    let app = application_id.unwrap_or("0");
    format!("/users/{user_id}/sessions/{session_id}/activities/{app}/metadata")
}

/// Module 961272 (`ep`): resolves the ordered action list for `UserActivityCardData`.
pub(crate) fn resolve_user_activity_actions(
    data: &UserActivityCardData,
) -> (bool, Vec<UserActivityAction>) {
    let spec = &data.actions;
    // Branch 1: non-rich Console Connect (Xbox / PlayStation Network).
    if !spec.is_rich
        && !spec.platform_connected
        && let Some(p) = data.platform.as_deref()
    {
        match p.to_ascii_lowercase().as_str() {
            "xbox" => {
                return (
                    false,
                    vec![UserActivityAction::ConnectConsole {
                        platform: "Xbox",
                        icon: "activity_connect_xbox",
                    }],
                );
            }
            "ps4" | "ps5" | "playstation" => {
                return (
                    false,
                    vec![UserActivityAction::ConnectConsole {
                        platform: "PlayStation Network",
                        icon: "activity_connect_playstation",
                    }],
                );
            }
            _ => {}
        }
    }
    // Branch 2: Spotify horizontal row (`PLAY` + `SYNC`).
    if data
        .platform
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case("spotify"))
    {
        let mut out = Vec::new();
        if spec.spotify_can_play || data.spotify_track_url.is_some() {
            out.push(UserActivityAction::SpotifyPlay);
        }
        if spec.spotify_can_sync {
            out.push(UserActivityAction::SpotifyListenAlong);
        }
        return (true, out);
    }
    // Branch 3: Stage Instance (`ZYO5OK` "Listen In").
    if data.activity_kind == ActivityKind::Stage {
        return (
            false,
            vec![UserActivityAction::StageListenIn {
                disabled: spec.stage_in_channel,
            }],
        );
    }
    // Branch 4: Stream Preview / Live Stream (`7Xq/nV` / `JH1SJ+` / `XvBdeT` + `wCrzut` multi-stream).
    if data.card_kind == ActivityCardKind::StreamPreview
        || data.activity_kind == ActivityKind::ScreenShare
    {
        let label = if spec.is_current_user {
            dc_intl::t("XvBdeT").to_owned()
        } else if spec.is_watching_stream {
            dc_intl::t("JH1SJ+").to_owned()
        } else {
            dc_intl::t("7Xq/nV").to_owned()
        };
        let disabled = spec.is_current_user || spec.is_watching_stream || !spec.can_watch_stream;
        let show_multi =
            spec.has_other_active_streams && !spec.is_watching_stream && !spec.is_current_user;
        return (
            false,
            vec![UserActivityAction::WatchStream {
                label,
                active: !spec.is_watching_stream,
                disabled,
                show_multi,
            }],
        );
    }
    // Branch 5: Default Activity Stack (Watch Twitch/Stream, Notify Me, Custom Activity Buttons).
    let mut out = Vec::new();
    if let Some(url) = data.stream_url.as_ref().filter(|u| !u.trim().is_empty()) {
        out.push(UserActivityAction::WatchExternal { url: url.clone() });
    }
    if spec.is_instance {
        out.push(UserActivityAction::NotifyMe);
    }
    for (index, (label, url)) in spec.custom_buttons.iter().enumerate() {
        if !label.trim().is_empty() {
            out.push(UserActivityAction::CustomButton {
                index,
                label: label.clone(),
                url: url.clone(),
            });
        }
    }
    (false, out)
}

/// Input data for a `UserActivity` card (module 663249).
#[derive(Clone, Debug)]
pub(crate) struct UserActivityCardData {
    pub card_kind: ActivityCardKind,
    pub activity_kind: ActivityKind,
    pub hide_header: bool,
    pub name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    pub large_text: Option<String>,
    pub small_text: Option<String>,
    /// Platform identifier (`ps4`, `ps5`, `xbox`, `samsung`, `spotify`, `twitch`, `youtube`, etc.).
    pub platform: Option<String>,
    /// Session relay provider display name (module 287743).
    pub session_platform: Option<String>,
    /// Guild where the activity / stream / voice channel takes place.
    pub guild_name: Option<String>,
    pub channel_name: Option<String>,
    pub is_embedded: bool,
    pub show_channel_details: bool,
    pub party_count: Option<u32>,
    pub party_max: Option<u32>,
    pub stage_speakers: Option<u32>,
    pub stage_listeners: Option<u32>,
    pub spotify_track_url: Option<String>,
    pub spotify_artists: Vec<(String, Option<String>)>,
    pub spotify_album: Option<(String, Option<String>)>,
    pub stream_url: Option<String>,
    pub elapsed_label: Option<String>,
    /// Module 961272 (`UserActivityActions`) button metadata.
    pub actions: UserActivityActionsSpec,
}

/// Formats the header line of `UserActivity` (module 663249 `renderHeader`).
pub(crate) fn format_activity_header(data: &UserActivityCardData) -> String {
    if let Some(guild) = data.guild_name.as_deref() {
        if data.is_embedded {
            return if data.activity_kind == ActivityKind::Watching {
                dc_intl::format("M/L8ot", &[("guildName", Arg::Str(Cow::Borrowed(guild)))])
            } else {
                dc_intl::format("4chKQu", &[("guildName", Arg::Str(Cow::Borrowed(guild)))])
            };
        }
        return dc_intl::format("sddlGK", &[("server", Arg::Str(Cow::Borrowed(guild)))]);
    }
    if let Some(provider) = data.session_platform.as_deref() {
        let key = match data.activity_kind {
            ActivityKind::Playing => "A17aM8",
            ActivityKind::Streaming => "Dzgz4u",
            ActivityKind::Listening => "EcHzWI",
            ActivityKind::Watching => "ENbTKQ",
            ActivityKind::Competing => "ikpHeS",
            ActivityKind::Stage | ActivityKind::ScreenShare => "eXan7B",
        };
        return if key == "eXan7B" {
            dc_intl::t("eXan7B").to_owned()
        } else {
            dc_intl::format(key, &[("platform", Arg::Str(Cow::Borrowed(provider)))])
        };
    }
    if data.activity_kind == ActivityKind::Playing
        && let Some(platform) = data.platform.as_deref()
    {
        let label = match platform.to_ascii_lowercase().as_str() {
            "ps4" => Some("PS4"),
            "ps5" => Some("PS5"),
            "xbox" => Some("Xbox"),
            "samsung" => Some("Samsung Galaxy"),
            _ => None,
        };
        if let Some(label) = label {
            return dc_intl::format("A17aM8", &[("platform", Arg::Str(Cow::Borrowed(label)))]);
        }
    }
    match data.activity_kind {
        ActivityKind::Playing => dc_intl::t("2TbM/G").to_owned(),
        ActivityKind::Streaming => dc_intl::format(
            "Dzgz4u",
            &[(
                "platform",
                Arg::Str(Cow::Borrowed(data.platform.as_deref().unwrap_or("Twitch"))),
            )],
        ),
        ActivityKind::Listening => {
            dc_intl::format("5sYPnr", &[("name", Arg::Str(Cow::Borrowed(&data.name)))])
        }
        ActivityKind::Watching => {
            dc_intl::format("Ge29Zy", &[("name", Arg::Str(Cow::Borrowed(&data.name)))])
        }
        ActivityKind::Competing => {
            dc_intl::format("SQCo6D", &[("name", Arg::Str(Cow::Borrowed(&data.name)))])
        }
        ActivityKind::Stage | ActivityKind::ScreenShare => dc_intl::t("eXan7B").to_owned(),
    }
}

/// Formats party size or Stage speaker/listener suffix (module 663249 `renderState`).
pub(crate) fn format_party_suffix(data: &UserActivityCardData) -> Option<String> {
    if data.activity_kind == ActivityKind::Stage {
        let speakers = data.stage_speakers.unwrap_or(0);
        let listeners = data.stage_listeners.unwrap_or(0);
        return Some(dc_intl::format(
            "JC/3xw",
            &[
                ("numSpeakers", Arg::Num(f64::from(speakers))),
                ("numListeners", Arg::Num(f64::from(listeners))),
            ],
        ));
    }
    let count = data.party_count?;
    match data.party_max {
        Some(max) if max > 0 => Some(dc_intl::format(
            "u//9By",
            &[
                ("count", Arg::Num(f64::from(count))),
                ("max", Arg::Num(f64::from(max))),
            ],
        )),
        _ => Some(dc_intl::format(
            "IM4J4e",
            &[("count", Arg::Num(f64::from(count)))],
        )),
    }
}

/// Formats channel details line when `channel == null` (`hq/Qze` `"in {guildName}"`).
pub(crate) fn format_channel_fallback(guild_name: &str) -> String {
    dc_intl::format(
        "hq/Qze",
        &[("guildName", Arg::Str(Cow::Borrowed(guild_name)))],
    )
}

/// Formats Twitch/YouTube stream game line (`gmCZRY` `"playing {game}"`).
pub(crate) fn format_streaming_game(game: &str) -> String {
    dc_intl::format("gmCZRY", &[("game", Arg::Str(Cow::Borrowed(game)))])
}

/// Renders the `UserActivity` card (module 663249).
pub(crate) fn render_user_activity_card(
    data: &UserActivityCardData,
    cx: &mut Context<Root>,
) -> AnyElement {
    let header = (!data.hide_header).then(|| {
        div()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(700.))
            .text_color(color("--text-muted"))
            .child(format_activity_header(data))
    });

    let is_spotify = data
        .platform
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case("spotify"));
    let is_ps = data
        .platform
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case("ps4") || p.eq_ignore_ascii_case("ps5"));

    let art_icon = match data.activity_kind {
        ActivityKind::Stage => "stage_channel",
        ActivityKind::ScreenShare => "voice_screen",
        ActivityKind::Streaming => "link_twitch",
        _ => "voice_activity",
    };
    let art_size = match data.card_kind {
        ActivityCardKind::ActivityFeed => 64.,
        ActivityCardKind::StreamPreview | ActivityCardKind::VoiceChannel => 56.,
    };
    let art = div()
        .size(px(art_size))
        .flex_shrink_0()
        .rounded(px(8.))
        .bg(color("--background-base-lowest"))
        .border_1()
        .border_color(color("--border-subtle"))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(art_icon, 26., "--interactive-icon-default"));

    let primary_title = match data.activity_kind {
        ActivityKind::Playing | ActivityKind::Stage | ActivityKind::ScreenShare => {
            if data.name.is_empty() && data.guild_name.is_some() {
                dc_intl::t("eXan7B").to_owned()
            } else {
                data.name.clone()
            }
        }
        _ => data.details.clone().unwrap_or_else(|| data.name.clone()),
    };

    let track_url = data
        .spotify_track_url
        .clone()
        .or_else(|| data.stream_url.clone());
    let title_row = div()
        .id("user-activity-title")
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(600.))
        .text_color(color("--text-strong"))
        .when(track_url.is_some(), |d| {
            let url = track_url.clone().unwrap_or_default();
            d.cursor_pointer()
                .hover(|s| s.underline())
                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
        })
        .child(primary_title);

    // Details row: Spotify `uU9le8` ("by {artistsHook}"), Streaming `gmCZRY` ("playing {game}"), Stage `small_text`, else `details`.
    let details_row: Option<AnyElement> = if is_spotify && !data.spotify_artists.is_empty() {
        let joined = data
            .spotify_artists
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let first_url = data.spotify_artists.iter().find_map(|(_, u)| u.clone());
        let label = dc_intl::format("uU9le8", &[("artists", Arg::Str(Cow::Borrowed(&joined)))]);
        Some(
            div()
                .id("user-activity-spotify-artists")
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .when_some(first_url, |d, url| {
                    d.cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
                })
                .child(label)
                .into_any_element(),
        )
    } else if data.activity_kind == ActivityKind::Streaming {
        data.state.as_deref().map(|game| {
            div()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(format_streaming_game(game))
                .into_any_element()
        })
    } else if data.activity_kind == ActivityKind::Stage {
        data.small_text.as_deref().map(|text| {
            div()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(text.to_owned())
                .into_any_element()
        })
    } else if data.activity_kind == ActivityKind::Playing {
        data.details.as_deref().map(|text| {
            div()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(text.to_owned())
                .into_any_element()
        })
    } else {
        data.state.as_deref().map(|text| {
            div()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(text.to_owned())
                .into_any_element()
        })
    };

    // State row: Spotify `vOLBEy` ("on {albumHook}"), or state + party/stage suffix.
    let party_suffix = format_party_suffix(data);
    let state_row: Option<AnyElement> =
        if is_spotify && let Some((album, album_url)) = data.spotify_album.as_ref() {
            let label = dc_intl::format("vOLBEy", &[("album", Arg::Str(Cow::Borrowed(album)))]);
            let url_opt = album_url.clone();
            Some(
                div()
                    .id("user-activity-spotify-album")
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .when_some(url_opt, |d, url| {
                        d.cursor_pointer()
                            .hover(|s| s.underline())
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
                    })
                    .child(label)
                    .into_any_element(),
            )
        } else {
            let base = if data.activity_kind == ActivityKind::Playing {
                data.state.as_deref()
            } else {
                data.large_text.as_deref()
            };
            match (base, party_suffix) {
                (Some(s), Some(p)) => Some(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(format!("{s} {p}"))
                        .into_any_element(),
                ),
                (Some(s), None) => Some(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(s.to_owned())
                        .into_any_element(),
                ),
                (None, Some(p)) => Some(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(p)
                        .into_any_element(),
                ),
                (None, None) => None,
            }
        };

    let channel_row = (data.show_channel_details)
        .then_some(data.guild_name.as_deref())
        .flatten()
        .map(|guild| match data.channel_name.as_deref() {
            Some(ch) => div()
                .pt(px(4.))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(12.))
                .text_color(color("--text-muted"))
                .child(guild.to_owned())
                .child(icon("bot_chevron_small_right", 12., "--text-muted"))
                .child(icon("voice_channel", 12., "--text-muted"))
                .child(ch.to_owned())
                .into_any_element(),
            None => div()
                .pt(px(4.))
                .text_size(px(12.))
                .text_color(color("--text-muted"))
                .child(format_channel_fallback(guild))
                .into_any_element(),
        });

    let platform_badge = (is_spotify || is_ps).then(|| {
        let badge_label = if is_spotify {
            "Spotify".to_owned()
        } else {
            data.platform
                .as_deref()
                .unwrap_or("PS")
                .to_ascii_uppercase()
        };
        div()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .bg(color("--background-mod-subtle"))
            .text_size(px(11.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-muted"))
            .child(badge_label)
    });

    let actions_row = render_user_activity_actions(data, cx);

    div()
        .p(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-base-lower"))
        .flex()
        .flex_col()
        .gap(px(8.))
        .children(header)
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(art)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(title_row)
                        .children(details_row)
                        .children(state_row)
                        .children(data.elapsed_label.clone().map(|t| {
                            div()
                                .text_size(px(12.))
                                .text_color(color("--text-muted"))
                                .child(t)
                        }))
                        .children(channel_row),
                )
                .children(platform_badge),
        )
        .children(actions_row)
        .into_any_element()
}

/// Module 961272 (`UserActivityActions`, CSS `279954`/`408399`): renders the action button bar
/// beneath a `UserActivity` card (`buttonsWrapper_nwnlPG`, horizontal for Spotify else vertical).
pub(crate) fn render_user_activity_actions(
    data: &UserActivityCardData,
    cx: &mut Context<Root>,
) -> Option<AnyElement> {
    let (horizontal, actions) = resolve_user_activity_actions(data);
    if actions.is_empty() {
        return None;
    }
    let spotify_url = data.spotify_track_url.clone();
    let mut wrap = div().mt(px(4.)).flex().gap(px(8.));
    wrap = if horizontal {
        wrap.flex_row().items_center()
    } else {
        wrap.flex_col()
    };
    for (idx, action) in actions.into_iter().enumerate() {
        let el: AnyElement = match action {
            UserActivityAction::ConnectConsole {
                platform,
                icon: ico,
            } => {
                let label =
                    dc_intl::format("XWSHTb", &[("platform", Arg::Str(Cow::Borrowed(platform)))]);
                div()
                    .id(SharedString::from(format!("ua-connect-{idx}")))
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-subtle"))
                    .hover(|s| s.bg(color("--background-mod-normal")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(icon(ico, 16., "--icon-strong"))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_settings(cx);
                    }))
                    .into_any_element()
            }
            UserActivityAction::SpotifyPlay => {
                let url = spotify_url.clone();
                div()
                    .id(SharedString::from(format!("ua-spotify-play-{idx}")))
                    .flex_1()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-subtle"))
                    .hover(|s| s.bg(color("--background-mod-normal")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(icon("active_now_spotify", 16., "--icon-strong"))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("rRffNz")),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(u) = url.as_deref() {
                            cx.open_url(u);
                        }
                    }))
                    .into_any_element()
            }
            UserActivityAction::SpotifyListenAlong => div()
                .id(SharedString::from(format!("ua-spotify-sync-{idx}")))
                .size(px(32.))
                .rounded(px(8.))
                .bg(color("--background-mod-subtle"))
                .hover(|s| s.bg(color("--background-mod-normal")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .aria_label(SharedString::from(dc_intl::t("hR2x2Q")))
                .child(icon("user_play", 16., "--icon-strong"))
                .into_any_element(),
            UserActivityAction::StageListenIn { disabled } => div()
                .id(SharedString::from(format!("ua-stage-listen-{idx}")))
                .h(px(32.))
                .px(px(12.))
                .rounded(px(8.))
                .bg(if disabled {
                    color("--background-mod-subtle")
                } else {
                    color("--control-primary-background-default")
                })
                .when(disabled, |d| d.opacity(0.5))
                .when(!disabled, |d| d.cursor_pointer())
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .child(icon("stage_channel", 16., "--white"))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("ZYO5OK")),
                )
                .into_any_element(),
            UserActivityAction::WatchStream {
                label,
                active,
                disabled,
                show_multi,
            } => div()
                .flex()
                .flex_row()
                .items_stretch()
                .gap(px(8.))
                .child(
                    div()
                        .id(SharedString::from(format!("ua-watch-stream-{idx}")))
                        .flex_1()
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .bg(if active && !disabled {
                            color("--control-primary-background-default")
                        } else {
                            color("--background-mod-subtle")
                        })
                        .when(disabled, |d| d.opacity(0.5))
                        .when(!disabled, |d| d.cursor_pointer())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(if active && !disabled {
                                    color("--white")
                                } else {
                                    color("--text-strong")
                                })
                                .child(label),
                        ),
                )
                .when(show_multi, |row| {
                    row.child(
                        div()
                            .id(SharedString::from(format!("ua-watch-multi-{idx}")))
                            .size(px(32.))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .hover(|s| s.bg(color("--background-mod-normal")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .aria_label(SharedString::from(dc_intl::t("wCrzut")))
                            .child(icon("voice_view_eye_plus", 16., "--icon-strong")),
                    )
                })
                .into_any_element(),
            UserActivityAction::WatchExternal { url } => div()
                .id(SharedString::from(format!("ua-watch-ext-{idx}")))
                .h(px(32.))
                .px(px(12.))
                .rounded(px(8.))
                .bg(color("--background-mod-subtle"))
                .hover(|s| s.bg(color("--background-mod-normal")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("I6JG46")),
                )
                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
                .into_any_element(),
            UserActivityAction::NotifyMe => div()
                .id(SharedString::from(format!("ua-notify-me-{idx}")))
                .h(px(32.))
                .px(px(12.))
                .rounded(px(8.))
                .bg(color("--background-mod-subtle"))
                .hover(|s| s.bg(color("--background-mod-normal")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .child(icon("bell", 15., "--icon-strong"))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("vwl1PK")),
                )
                .into_any_element(),
            UserActivityAction::CustomButton { index, label, url } => div()
                .id(SharedString::from(format!("ua-custom-{idx}-{index}")))
                .h(px(32.))
                .px(px(12.))
                .rounded(px(8.))
                .bg(color("--background-mod-subtle"))
                .hover(|s| s.bg(color("--background-mod-normal")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    if let Some(u) = url.as_deref()
                        && (u.starts_with("https://") || u.starts_with("http://"))
                    {
                        cx.open_url(u);
                    }
                }))
                .into_any_element(),
        };
        wrap = wrap.child(el);
    }
    Some(wrap.into_any_element())
}

impl Root {
    pub(crate) fn close_user_activity_preview(&mut self) -> bool {
        self.user_activity_preview.take().is_some()
    }

    pub(in crate::ui) fn user_activity_preview_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let cards = self.user_activity_preview.as_ref()?;
        Some(
            deferred(
                div()
                    .id("user-activity-preview-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.close_user_activity_preview();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("user-activity-preview-modal")
                            .occlude()
                            .w(px(440.))
                            .p(px(20.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .children(cards.iter().enumerate().map(|(i, card)| {
                                div()
                                    .id(SharedString::from(format!("user-activity-card-{i}")))
                                    .child(render_user_activity_card(card, cx))
                            })),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_user_activity_card(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.close_user_activity_preview();
            cx.notify();
            return "user activity preview closed".to_owned();
        }
        self.user_activity_preview = Some(vec![
            UserActivityCardData {
                card_kind: ActivityCardKind::ActivityFeed,
                activity_kind: ActivityKind::Playing,
                hide_header: false,
                name: "Elden Ring".to_owned(),
                details: Some("Exploring Leyndell".to_owned()),
                state: Some("Co-op Party".to_owned()),
                large_text: None,
                small_text: None,
                platform: Some("ps5".to_owned()),
                session_platform: None,
                guild_name: None,
                channel_name: None,
                is_embedded: false,
                show_channel_details: false,
                party_count: Some(2),
                party_max: Some(4),
                stage_speakers: None,
                stage_listeners: None,
                spotify_track_url: None,
                spotify_artists: Vec::new(),
                spotify_album: None,
                stream_url: None,
                elapsed_label: Some("01:24:10 elapsed".to_owned()),
                actions: UserActivityActionsSpec {
                    is_rich: false,
                    platform_connected: false,
                    ..Default::default()
                },
            },
            UserActivityCardData {
                card_kind: ActivityCardKind::VoiceChannel,
                activity_kind: ActivityKind::Listening,
                hide_header: false,
                name: "Spotify".to_owned(),
                details: Some("1985".to_owned()),
                state: None,
                large_text: None,
                small_text: None,
                platform: Some("spotify".to_owned()),
                session_platform: None,
                guild_name: None,
                channel_name: None,
                is_embedded: false,
                show_channel_details: false,
                party_count: None,
                party_max: None,
                stage_speakers: None,
                stage_listeners: None,
                spotify_track_url: Some("https://open.spotify.com/track/demo".to_owned()),
                spotify_artists: vec![("Freddie Gibbs".to_owned(), None)],
                spotify_album: Some(("Alfredo".to_owned(), None)),
                stream_url: None,
                elapsed_label: None,
                actions: UserActivityActionsSpec {
                    is_rich: true,
                    spotify_can_play: true,
                    spotify_can_sync: true,
                    ..Default::default()
                },
            },
            UserActivityCardData {
                card_kind: ActivityCardKind::StreamPreview,
                activity_kind: ActivityKind::Stage,
                hide_header: false,
                name: "Community Town Hall".to_owned(),
                details: None,
                state: None,
                large_text: None,
                small_text: Some("Q&A Live".to_owned()),
                platform: None,
                session_platform: None,
                guild_name: Some("gravecord fixture".to_owned()),
                channel_name: Some("General".to_owned()),
                is_embedded: false,
                show_channel_details: true,
                party_count: None,
                party_max: None,
                stage_speakers: Some(3),
                stage_listeners: Some(42),
                spotify_track_url: None,
                spotify_artists: Vec::new(),
                spotify_album: None,
                stream_url: None,
                elapsed_label: None,
                actions: UserActivityActionsSpec::default(),
            },
        ]);
        cx.notify();
        "user activity preview opened".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_20_keys_of_user_activity_663249_resolve() {
        let keys = [
            "eXan7B", "Dzgz4u", "5sYPnr", "Ge29Zy", "A17aM8", "2TbM/G", "SQCo6D", "ENbTKQ",
            "EcHzWI", "ikpHeS", "M/L8ot", "4chKQu", "sddlGK", "gmCZRY", "uU9le8", "hq/Qze",
            "vOLBEy", "u//9By", "JC/3xw", "IM4J4e",
        ];
        for key in keys {
            let value = dc_intl::t(key);
            assert!(!value.is_empty(), "key {key} must resolve");
            assert_ne!(value, key, "key {key} fell back to literal");
        }
    }

    #[test]
    fn headers_and_party_formatting_match_module_663249() {
        let mut data = UserActivityCardData {
            card_kind: ActivityCardKind::ActivityFeed,
            activity_kind: ActivityKind::Playing,
            hide_header: false,
            name: "Elden Ring".into(),
            details: None,
            state: None,
            large_text: None,
            small_text: None,
            platform: Some("ps5".into()),
            session_platform: None,
            guild_name: None,
            channel_name: None,
            is_embedded: false,
            show_channel_details: false,
            party_count: Some(2),
            party_max: Some(4),
            stage_speakers: None,
            stage_listeners: None,
            spotify_track_url: None,
            spotify_artists: vec![],
            spotify_album: None,
            stream_url: None,
            elapsed_label: None,
            actions: UserActivityActionsSpec::default(),
        };
        assert!(format_activity_header(&data).contains("PS5"));
        assert_eq!(format_party_suffix(&data).as_deref(), Some("(2 of 4)"));
        data.party_max = Some(0);
        assert_eq!(format_party_suffix(&data).as_deref(), Some("(2)"));
        data.activity_kind = ActivityKind::Stage;
        data.stage_speakers = Some(3);
        data.stage_listeners = Some(15);
        let stage = format_party_suffix(&data).unwrap();
        assert!(stage.contains('3') && stage.contains("15"));
    }

    #[test]
    fn user_activity_actions_961272_all_branches_and_intl() {
        for key in [
            "XWSHTb", "ZYO5OK", "vwl1PK", "wCrzut", "I6JG46", "XvBdeT", "JH1SJ+", "7Xq/nV",
        ] {
            assert!(dc_intl::has_key(key), "missing 961272 key {key}");
        }
        assert_eq!(
            activity_metadata_route("u1", "s2", Some("app3")),
            "/users/u1/sessions/s2/activities/app3/metadata"
        );
        assert_eq!(
            activity_metadata_route("u1", "s2", None),
            "/users/u1/sessions/s2/activities/0/metadata"
        );
        let mut data = UserActivityCardData {
            card_kind: ActivityCardKind::ActivityFeed,
            activity_kind: ActivityKind::Playing,
            hide_header: false,
            name: "Halo".into(),
            details: None,
            state: None,
            large_text: None,
            small_text: None,
            platform: Some("xbox".into()),
            session_platform: None,
            guild_name: None,
            channel_name: None,
            is_embedded: false,
            show_channel_details: false,
            party_count: None,
            party_max: None,
            stage_speakers: None,
            stage_listeners: None,
            spotify_track_url: None,
            spotify_artists: vec![],
            spotify_album: None,
            stream_url: None,
            elapsed_label: None,
            actions: UserActivityActionsSpec::default(),
        };
        let (horiz, acts) = resolve_user_activity_actions(&data);
        assert!(!horiz);
        assert!(matches!(
            acts.as_slice(),
            [UserActivityAction::ConnectConsole {
                platform: "Xbox",
                ..
            }]
        ));

        data.platform = Some("spotify".into());
        data.actions.is_rich = true;
        data.actions.spotify_can_play = true;
        data.actions.spotify_can_sync = true;
        let (horiz, acts) = resolve_user_activity_actions(&data);
        assert!(horiz);
        assert_eq!(acts.len(), 2);

        data.platform = None;
        data.activity_kind = ActivityKind::Stage;
        let (_, acts) = resolve_user_activity_actions(&data);
        assert_eq!(
            acts,
            vec![UserActivityAction::StageListenIn { disabled: false }]
        );

        data.activity_kind = ActivityKind::ScreenShare;
        data.actions.can_watch_stream = true;
        data.actions.has_other_active_streams = true;
        let (_, acts) = resolve_user_activity_actions(&data);
        assert!(matches!(
            acts.as_slice(),
            [UserActivityAction::WatchStream {
                show_multi: true,
                active: true,
                disabled: false,
                ..
            }]
        ));

        data.activity_kind = ActivityKind::Playing;
        data.card_kind = ActivityCardKind::ActivityFeed;
        data.stream_url = Some("https://twitch.tv/demo".into());
        data.actions.is_instance = true;
        data.actions.custom_buttons =
            vec![("Join Website".into(), Some("https://example.com".into()))];
        let (_, acts) = resolve_user_activity_actions(&data);
        assert_eq!(acts.len(), 3);
    }
}
