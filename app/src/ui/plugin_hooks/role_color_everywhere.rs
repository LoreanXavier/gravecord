//! RoleColorEverywhere (Vencord `src/plugins/roleColorEverywhere`, GPL-3.0-or-later): the colour of a person's top
//! coloured role (`GuildMemberStore`'s `colorString`) where the client draws them plainly. Each setting is one place:
//! - `chatMentions`: a user mention in a message takes the colour as a role mention does (`color` and `roleColors`
//!   handed to the mention: the text in the colour or gradient, the colour at .1 behind it, .3 under the pointer);
//! - `memberList`: a role's group header in the member list (`getRoleStyle`);
//! - `voiceUsers`: the names under a voice channel (`getColorStyle`);
//! - `reactorsList`: the names in the reactions dialog;
//! - `pollResults`: the names in the poll voters dialog;
//! - `colorChatMessages` with `messageSaturation`: a message's text is `color-mix(in oklab, <colour> s%, --text-default)`,
//!   not for a message whose send failed.
//!
//! `makeColorStyle`: in a server with `ENHANCED_ROLE_COLORS` and a role with a second colour, the text is the role's
//! gradient (`linear-gradient(to right, c1, c2[, c3], c1)` at `100px auto`), else the plain colour.
//!
//! GAP: the message box draws no mention pills, so `chatMentions`' Slate half has no place here; `colorChatMessages`
//! also mixes `--text-strong` and `--text-muted` inside the message, which the content here does not read; and the
//! CustomUserColors override is not ported. Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use dc_state::{Id, RoleColors};
use gpui::{Hsla, IntoElement, ParentElement, Styled};
use serde_json::Value;

const ID: &str = "RoleColorEverywhere";

/// A place the plugin colours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Place {
    ChatMentions,
    MemberList,
    VoiceUsers,
    ReactorsList,
    PollResults,
}

impl Place {
    fn key(self) -> &'static str {
        match self {
            Place::ChatMentions => "chatMentions",
            Place::MemberList => "memberList",
            Place::VoiceUsers => "voiceUsers",
            Place::ReactorsList => "reactorsList",
            Place::PollResults => "pollResults",
        }
    }
}

fn colors_in(state: &State, place: Place) -> bool {
    state.is_on(ID) && state.value(ID, place.key()) != Value::Bool(false)
}

/// The colour a place takes: the role colour, and its gradient where the server has enhanced role colours.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::ui) struct Look {
    pub(in crate::ui) color: u32,
    pub(in crate::ui) gradient: Option<RoleColors>,
}

/// `messageSaturation` while `colorChatMessages` is on (a saturation of 0 colours nothing).
fn message_saturation(state: &State) -> Option<f32> {
    if !state.is_on(ID) || state.value(ID, "colorChatMessages") != Value::Bool(true) {
        return None;
    }
    let saturation = state.value(ID, "messageSaturation").as_f64().unwrap_or(30.) as f32;
    (saturation > 0.).then_some(saturation.clamp(0., 100.))
}

/// `color-mix(in oklab, a p%, b)`.
pub(super) fn mix_oklab(a: Hsla, p: f32, b: Hsla) -> Hsla {
    fn to_lab(color: gpui::Rgba) -> [f32; 3] {
        let linear = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (linear(color.r), linear(color.g), linear(color.b));
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        [
            0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        ]
    }
    fn from_lab([l, a, b]: [f32; 3], alpha: f32) -> gpui::Rgba {
        let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        let gamma = |c: f32| {
            let c = c.clamp(0., 1.);
            if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1. / 2.4) - 0.055
            }
        };
        gpui::Rgba {
            r: gamma(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
            g: gamma(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
            b: gamma(-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
            a: alpha,
        }
    }
    let (first, second) = (gpui::Rgba::from(a), gpui::Rgba::from(b));
    let w = p / 100.;
    let (la, lb) = (to_lab(first), to_lab(second));
    let mixed = [
        la[0] * w + lb[0] * (1. - w),
        la[1] * w + lb[1] * (1. - w),
        la[2] * w + lb[2] * (1. - w),
    ];
    from_lab(mixed, first.a * w + second.a * (1. - w)).into()
}

impl Root {
    /// The colour of `user` in `place` (in the server on show), or `None`: plugin or place off, no server, or no
    /// coloured role.
    pub(in crate::ui) fn role_color_everywhere(&self, place: Place, user: Id) -> Option<Look> {
        if crate::plugins::bypassed() || !crate::plugins::with(|state| colors_in(state, place)) {
            return None;
        }
        let guild = self.current_guild()?;
        let member = self.stores.members.get(guild.id, user)?;
        Some(Look {
            color: guild.role_color(&member.roles)?,
            gradient: guild.role_gradient(&member.roles),
        })
    }

    /// `getRoleStyle` of a member list group: the role's colour, or `None` (an online/offline group, or no colour).
    pub(in crate::ui) fn role_color_everywhere_role(&self, group: &str) -> Option<Look> {
        if crate::plugins::bypassed()
            || !crate::plugins::with(|state| colors_in(state, Place::MemberList))
        {
            return None;
        }
        let role: Id = group.parse().ok()?;
        let guild = self.current_guild()?;
        let info = guild.role_info.get(&role).filter(|info| info.color != 0)?;
        Some(Look {
            color: info.color,
            gradient: guild.role_colors_of(role),
        })
    }

    /// A message's text colour under `colorChatMessages`: the author's colour mixed into `base`, or `None`.
    pub(in crate::ui) fn role_color_everywhere_message(
        &self,
        author: Id,
        base: Hsla,
    ) -> Option<Hsla> {
        if crate::plugins::bypassed() {
            return None;
        }
        let saturation = crate::plugins::with(message_saturation)?;
        let guild = self.current_guild()?;
        let member = self.stores.members.get(guild.id, author)?;
        let color = guild.role_color(&member.roles)?;
        Some(mix_oklab(Hsla::from(gpui::rgb(color)), saturation, base))
    }
}

/// A name `text` in `holder`: in `look`'s colour or gradient, else in `fallback` as the pinned client draws it.
pub(in crate::ui) fn paint(
    holder: gpui::Div,
    look: Option<Look>,
    fallback: Hsla,
    text: String,
) -> gpui::Div {
    match look {
        None => holder.text_color(fallback).child(text),
        Some(Look {
            gradient: Some(colors),
            ..
        }) => holder.child(super::super::role_gradient::name(text, colors)),
        Some(Look { color, .. }) => holder.text_color(Hsla::from(gpui::rgb(color))).child(text),
    }
}

/// A member list group label `text` in `look`, or the plain label.
pub(in crate::ui) fn label(look: Option<Look>, text: String) -> gpui::AnyElement {
    match look {
        None => text.into_any_element(),
        Some(look) => paint(gpui::div(), Some(look), Hsla::default(), text).into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_place_has_its_switch_and_messages_need_theirs() {
        let mut state = State::default();
        assert!(!colors_in(&state, Place::VoiceUsers));
        state.set_enabled(ID, true);
        for place in [
            Place::ChatMentions,
            Place::MemberList,
            Place::VoiceUsers,
            Place::ReactorsList,
            Place::PollResults,
        ] {
            assert!(colors_in(&state, place), "{place:?} defaults on");
        }
        state.set_value(ID, "voiceUsers", &json!(false));
        assert!(!colors_in(&state, Place::VoiceUsers));
        assert_eq!(
            message_saturation(&state),
            None,
            "colorChatMessages defaults off"
        );
        state.set_value(ID, "colorChatMessages", &json!(true));
        assert_eq!(message_saturation(&state), Some(30.));
        state.set_value(ID, "messageSaturation", &json!(0));
        assert_eq!(
            message_saturation(&state),
            None,
            "a saturation of 0 colours nothing"
        );
    }

    #[test]
    fn oklab_mix_matches_the_css_ends_and_middle() {
        let red = Hsla::from(gpui::rgb(0xff0000));
        let white = Hsla::from(gpui::rgb(0xffffff));
        let close = |a: gpui::Rgba, b: gpui::Rgba| {
            (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
        };
        assert!(close(
            mix_oklab(red, 100., white).into(),
            gpui::rgb(0xff0000)
        ));
        assert!(close(mix_oklab(red, 0., white).into(), gpui::rgb(0xffffff)));
        // `color-mix(in oklab, red 50%, white)`: rgb(255 161 145) by the CSS Color 4 OKLab matrices.
        let middle: gpui::Rgba = mix_oklab(red, 50., white).into();
        assert!(
            (middle.r * 255. - 255.).abs() < 2.
                && (middle.g * 255. - 161.).abs() < 2.
                && (middle.b * 255. - 145.).abs() < 2.,
            "{middle:?}"
        );
    }
}
