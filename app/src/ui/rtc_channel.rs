//! The voice panel's channel line: its right-click menu `rtc-channel` ("Actions", module 576701) and the strip of the
//! channel's people under the panel's connection row that its Show Voice Users switches on (`TB`/`Tg` of module
//! 596339).
//!
//! The menu: the input mode radios (module 1611: Voice Activity, Push to Talk), Input Volume and Output Volume (modules
//! 486487 and 298242), for a server's voice or Stage channel Invite (module 475777 through 531335) and Copy Link
//! (module 886393), then Pop Out (while the call is not popped out) and the Show Voice Users box, the account's
//! `debug.rtcPanelShowVoiceStates` (`vL` of module 253932).
//!
//! The strip (`scroller_7FtMGA voiceUsers_9U6LUN`, CSS of modules 588986 and 187529): the other people of the channel
//! as 24px avatars 4px apart (a Stage's speakers only), a mute or deafen mark over an avatar (red for a server mute or
//! deafen), and when they do not fit on one line the last place holds Show All Users (+), which lets the strip wrap,
//! and Minimize (-) folds it again. A person's name, and their state, is the avatar's tooltip.

use super::context_menu::{Act, Entry, Row};
use super::tooltip::{Side, Tip};
use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use dc_proto::settings::RTC_PANEL_SHOW_VOICE_STATES;
use dc_state::{ChannelKind, Id};
use gpui::{AnyElement, Context, IntoElement, SharedString, Window, div, img, prelude::*, px};
use serde_json::json;

/// The avatars that fit on one line of `width` px (`Math.floor((width + 4) / 28)`, 24px avatars 4px apart), one less
/// when `count` people do not all fit, for Show All Users (module 596339 `Tg`).
pub(super) fn visible_avatars(width: f32, count: usize) -> usize {
    let fit = ((width + 4.) / 28.).floor().max(0.) as usize;
    (if count > fit {
        fit.saturating_sub(1)
    } else {
        fit
    })
    .max(1)
}

/// The mark over a person's avatar, in `Tm`'s order: server deafened, deafened, server muted, muted for me, muted.
/// The icon, whether it is red, and the state for the tooltip.
pub(super) fn voice_mark(
    server_deaf: bool,
    deaf: bool,
    server_mute: bool,
    local_mute: bool,
    mute: bool,
) -> Option<(&'static str, bool, &'static str)> {
    use super::voice_marks::{HEADPHONES_DENY, HEADPHONES_SLASH, MIC_DENY, MIC_SLASH};
    // `Tm`: server deafen and server mute are the deny glyphs, coloured; muted for me is the deny microphone, not.
    if server_deaf {
        Some((HEADPHONES_DENY, true, "btxSdB"))
    } else if deaf {
        Some((HEADPHONES_SLASH, false, "NjmiOL"))
    } else if server_mute {
        Some((MIC_DENY, true, "uLddbQ"))
    } else if local_mute {
        Some((MIC_DENY, false, "9F+xJU"))
    } else if mute {
        Some((MIC_SLASH, false, "tjtv3P"))
    } else {
        None
    }
}

/// The width of the strip's line: the 240px panel less its 8px padding on each side, widened by the scroller's -3px
/// and -7px margins and narrowed by its 4px start padding.
const STRIP_WIDTH: f32 = 240. - 16. + 3. + 7. - 4.;

/// Logistic speaking ring inset spread (module 556525 `T_.v`: `0.5 + (max - 0.5) / (1 + exp(-0.03 * (db + 45)))`).
pub(super) fn speaking_ring_spread(db: f32, max_spread: f32) -> f32 {
    if !db.is_finite() {
        return 2.0_f32.min(max_spread);
    }
    0.5 + (max_spread - 0.5) / (1.0 + (-0.03 * (db + 45.0)).exp())
}

impl Root {
    /// `vL.useSetting()`: off unless the account says so.
    pub(super) fn show_voice_users(&self) -> bool {
        self.stores
            .proto_varint(
                RTC_PANEL_SHOW_VOICE_STATES.path,
                RTC_PANEL_SHOW_VOICE_STATES.field,
            )
            .is_some_and(|value| value != 0)
    }

    /// Right click on the panel's channel line.
    pub(super) fn open_rtc_channel_menu(
        &mut self,
        guild: Id,
        channel: Id,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            super::context_menu::Target::RtcChannel { guild, channel },
            at,
            cx,
        );
    }

    pub(super) fn rtc_channel_items(&self, guild: Id, channel: Id) -> Vec<Entry> {
        let push_to_talk = self.kb_push_to_talk();
        let radio = |act: Act, label: &str, on: bool| {
            let mut row = Row::new(act, label);
            row.check = Some(on);
            row.radio = true;
            Entry::Row(row)
        };
        let mut items = vec![
            radio(Act::RtcVoiceActivity, dc_intl::t("cHCEOJ"), !push_to_talk),
            // The desktop app: plain Push to Talk (the browser's is "Limited").
            radio(Act::RtcPushToTalk, dc_intl::t("Q8gkVL"), push_to_talk),
            Entry::Separator,
            Entry::Row(Row::new(Act::InputVolume, dc_intl::t("OX2Bnr"))),
            Entry::Row(Row::new(Act::OutputVolume, dc_intl::t("eATD2B"))),
        ];
        // `isGuildVocal`: a server's voice or Stage channel.
        let kind = self
            .stores
            .guild(guild)
            .and_then(|known| known.channels.iter().find(|known| known.id == channel))
            .map(|known| known.kind);
        if guild != 0 && matches!(kind, Some(ChannelKind::Voice | ChannelKind::Other(13))) {
            items.push(Entry::Separator);
            items.push(Entry::Row(Row::new(
                Act::Invite,
                // `E` of module 531335: a voice channel's invite, else the channel's.
                if kind == Some(ChannelKind::Voice) {
                    dc_intl::t("EE+P0H")
                } else {
                    dc_intl::t("0jeAXt")
                },
            )));
            items.push(Entry::Row(Row::new(Act::CopyLink, dc_intl::t("WqhZss"))));
        }
        items.push(Entry::Separator);
        if !self.call_popped_out() {
            items.push(Entry::Row(
                Row::new(Act::PopOutCall, dc_intl::t("JzWezz")).trailing("voice_view_popout"),
            ));
        }
        let mut show = Row::new(Act::ShowVoiceUsers, dc_intl::t("vkbSB0"));
        show.check = Some(self.show_voice_users());
        items.push(Entry::Row(show));
        items
    }

    pub(super) fn run_rtc_channel(
        &mut self,
        act: Act,
        channel: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match act {
            Act::RtcVoiceActivity => self.set_push_to_talk(false),
            Act::RtcPushToTalk => self.set_push_to_talk(true),
            Act::PopOutCall => {
                if !self.call_popped_out() {
                    self.toggle_call_popout(window, cx);
                }
            }
            Act::ShowVoiceUsers => {
                let on = !self.show_voice_users();
                let patch = self
                    .stores
                    .set_pref(&RTC_PANEL_SHOW_VOICE_STATES, u64::from(on));
                if !patch.is_empty() && !crate::storage::settings_writes_blocked() {
                    let _ = self.requests.send(Request::Rest {
                        key: None,
                        method: reqwest::Method::PATCH,
                        route: "/users/@me/settings-proto/1".into(),
                        body: Some(json!({ "settings": patch })),
                    });
                }
            }
            Act::Invite | Act::CopyLink => self.run_context(
                act,
                super::context_menu::Target::Channel(channel),
                window,
                cx,
            ),
            _ => {}
        }
        cx.notify();
    }

    /// Input Volume or Output Volume as the menu's slider row (`MenuControlItem` with the slider of module 964355):
    /// the same perceptual scale as the user panel's menus, to 200% for the output in the desktop app.
    pub(super) fn rtc_volume_row(
        &self,
        row: &Row,
        side: super::audio_menu::Side,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::audio_menu::Side as AudioSide;
        use super::menu_slider::{self, MenuSliderConfig, ThemePalette};
        let prefs = dc_voice::prefs();
        let (max, gain, id) = match side {
            AudioSide::Input => (100, prefs.input_gain(), "rtc-input-volume"),
            AudioSide::Output => (200, prefs.output_gain(), "rtc-output-volume"),
        };
        self.menu_slider_row(
            MenuSliderConfig {
                id: id.into(),
                label: row.label.clone(),
                min: 0,
                max,
                value: menu_slider::amplitude_to_perceptual(gain, max),
                palette: ThemePalette::Default,
                format: menu_slider::percent_format(),
                on_change: std::rc::Rc::new(move |_, pct, cx| {
                    super::audio_menu::store_gain(
                        side,
                        menu_slider::perceptual_to_amplitude(pct, max),
                    );
                    cx.notify();
                }),
            },
            cx,
        )
    }

    /// The strip of the channel's people (`TB`), when Show Voice Users is on and anyone else is there.
    pub(super) fn rtc_voice_users(
        &self,
        guild: Id,
        channel: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.show_voice_users() {
            return None;
        }
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let stage = self.is_stage_voice_channel(guild, channel);
        let mut people: Vec<&dc_state::VoiceState> = Vec::new();
        for state in self.stores.voice_members(guild, channel) {
            // A Stage lists its speakers (`getMutableParticipants(SPEAKER)`).
            if stage && state.suppress {
                continue;
            }
            if Some(state.user_id) == me
                || people.iter().any(|known| known.user_id == state.user_id)
            {
                continue;
            }
            people.push(state);
        }
        if people.is_empty() {
            return None;
        }
        let expanded = self.voice_users_expanded;
        let fit = visible_avatars(STRIP_WIDTH, people.len());
        let more = people.len() > fit;
        let shown: Vec<&dc_state::VoiceState> = if more && !expanded {
            people[..fit].to_vec()
        } else {
            people
        };
        let avatars: Vec<AnyElement> = shown
            .into_iter()
            .map(|state| {
                let user = self
                    .stores
                    .users
                    .get(&state.user_id)
                    .cloned()
                    .unwrap_or_default();
                let (key, url) = avatar_source(&user);
                let picture = self.images.get(&key).cloned();
                if picture.is_none() {
                    self.want_image(key, url, cx);
                }
                let local_mute = self
                    .context
                    .mix
                    .get(&state.user_id)
                    .is_some_and(|mix| mix.1);
                let mark = voice_mark(
                    state.deaf,
                    state.self_deaf,
                    state.mute,
                    local_mute,
                    state.self_mute,
                );
                let name = user.display_name().to_owned();
                let tip = match mark {
                    Some((_, _, status)) => dc_intl::format(
                        "1+MVBP",
                        &[
                            ("userName", name.into()),
                            ("status", dc_intl::t(status).into()),
                        ],
                    ),
                    None => name,
                };
                let is_playing_sound = self.soundboard.user_playing(state.user_id);
                let is_speaking = self.speaking.contains(&state.user_id);
                // Module 556525 (`T_.v`) + 551826 (`VOICE_USER` sound shake): inset speaking ring
                // (`--status-speaking` or `--brand-500` when playing soundboard) + subtle shake offset.
                let ring_token = if is_playing_sound {
                    Some("--brand-500")
                } else if is_speaking {
                    Some("--status-speaking")
                } else {
                    None
                };
                let (shake_x, shake_y) =
                    if is_playing_sound && !self.pref_on("reduced_motion", false) {
                        let tick = (std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() / 32)
                            .unwrap_or(0)
                            + state.user_id as u128) as usize;
                        const OFFSETS: [(f32, f32); 4] =
                            [(-1.5, -1.0), (1.5, 1.0), (-1.0, 1.5), (1.0, -1.5)];
                        OFFSETS[tick % OFFSETS.len()]
                    } else {
                        (0., 0.)
                    };
                let id = SharedString::from(format!("rtc-voice-user-{}", state.user_id));
                self.with_tip(
                    format!("rtc-voice-user-tip-{}", state.user_id),
                    Tip::body(super::tooltip::Body::Text(tip.into()), Side::Top),
                    div()
                        .id(id)
                        .flex_none()
                        .size(px(24.))
                        .rounded_full()
                        .overflow_hidden()
                        .cursor_pointer()
                        .relative()
                        .when(shake_x != 0. || shake_y != 0., |d| {
                            d.left(px(shake_x)).top(px(shake_y))
                        })
                        .children(picture.map(|picture| img(picture).size(px(24.)).rounded_full()))
                        .when_some(ring_token, |d, token| {
                            let spread = speaking_ring_spread(-28.0, 3.0);
                            d.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded_full()
                                    .border(px(spread))
                                    .border_color(color(token))
                                    .child(
                                        div()
                                            .size_full()
                                            .rounded_full()
                                            .border_1()
                                            .border_color(color("--background-base-lower")),
                                    ),
                            )
                        })
                        .children(mark.map(|(name, red, _)| {
                            div()
                                .absolute()
                                .inset_0()
                                .p(px(4.))
                                .bg(color("--opacity-black-28"))
                                .child(icon(name, 16., if red { "--red-400" } else { "--white" }))
                        })),
                    cx,
                )
                .into_any_element()
            })
            .collect();
        let toggle_label = if expanded {
            dc_intl::t("Z/Tya5")
        } else {
            dc_intl::t("XR5BAc")
        };
        let toggle = more.then(|| {
            self.with_tip(
                "rtc-voice-users-toggle-tip",
                Tip::text(toggle_label, Side::Top),
                div()
                    .id("rtc-voice-users-toggle")
                    .flex_none()
                    .size(px(24.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                    .child(icon(
                        if expanded {
                            "automod_minus"
                        } else {
                            "plus_small"
                        },
                        20.,
                        "--control-icon-only-icon-default",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.voice_users_expanded = !this.voice_users_expanded;
                        cx.notify();
                    })),
                cx,
            )
        });
        // Module 576701 + 556525 (`T_.v`) + 551826: voice strip with inset speaking ring and soundboard shake.
        Some(
            div()
                .ml(px(-3.))
                .mr(px(-7.))
                .pl(px(4.))
                .max_h(px(132.))
                .pt(px(2.))
                .pb(px(4.))
                .when(expanded, |this| this.mb(px(4.)))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .when(expanded, |this| this.flex_wrap())
                        .children(avatars)
                        .children(toggle),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_keeps_a_place_for_show_all_users() {
        // 214px: (214 + 4) / 28 = 7 avatars.
        assert_eq!(visible_avatars(214., 3), 7);
        assert_eq!(visible_avatars(214., 7), 7);
        assert_eq!(visible_avatars(214., 8), 6);
        assert_eq!(visible_avatars(10., 4), 1);
    }

    #[test]
    fn marks_follow_the_official_order() {
        assert_eq!(
            voice_mark(true, true, true, true, true).map(|m| m.2),
            Some("btxSdB")
        );
        assert_eq!(
            voice_mark(false, true, true, false, true).map(|m| m.2),
            Some("NjmiOL")
        );
        assert_eq!(
            voice_mark(false, false, true, true, true).map(|m| (m.1, m.2)),
            Some((true, "uLddbQ"))
        );
        assert_eq!(
            voice_mark(false, false, false, true, true).map(|m| m.2),
            Some("9F+xJU")
        );
        assert_eq!(voice_mark(false, false, false, false, false), None);
    }

    #[test]
    fn marks_draw_the_official_deny_and_slash_glyphs() {
        use super::super::voice_marks::{HEADPHONES_DENY, HEADPHONES_SLASH, MIC_DENY, MIC_SLASH};
        let icon = |marks: (bool, bool, bool, bool, bool)| {
            voice_mark(marks.0, marks.1, marks.2, marks.3, marks.4).map(|m| m.0)
        };
        assert_eq!(
            icon((true, false, false, false, false)),
            Some(HEADPHONES_DENY)
        );
        assert_eq!(
            icon((false, true, false, false, true)),
            Some(HEADPHONES_SLASH)
        );
        assert_eq!(icon((false, false, true, false, false)), Some(MIC_DENY));
        assert_eq!(icon((false, false, false, true, false)), Some(MIC_DENY));
        assert_eq!(icon((false, false, false, false, true)), Some(MIC_SLASH));
    }

    #[test]
    fn speaking_ring_spread_is_bounded_logistic() {
        let loud = speaking_ring_spread(-10.0, 3.0);
        let quiet = speaking_ring_spread(-90.0, 3.0);
        assert!(loud > quiet);
        assert!(loud <= 3.0 && quiet >= 0.5);
        assert_eq!(speaking_ring_spread(f32::NEG_INFINITY, 3.0), 2.0);
    }
}
