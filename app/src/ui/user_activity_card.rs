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
        .into_any_element()
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
}
