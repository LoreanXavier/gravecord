//! The mute and deafen marks a call draws for a person, each picked as its official module picks it. The glyphs are
//! the official icons' own paths (the assets below), never the user panel's Lottie frames, which are another drawing.
//!
//! - 85448 `CallAvatar` (a DM call's avatars): one mark in a red disk, local mute, then deafened, then muted.
//! - 175203 (a call tile's name pill): one mark, server deafened, server muted, deafened, muted for me, muted.
//! - 293277 `Kc` (a voice channel's rows and the overlay): the microphone mark when muted, then the headphones mark
//!   when deafened, each the deny glyph and red (`iconServer_YUoOxm`) for a server mute or deafen.
//!
//! The soundboard mark (`SoundboardDenyIcon`) of all three sits behind `2026-10-soundboard-mute-indicator` (module
//! 421768), off by default, so it is not drawn.
use dc_state::VoiceState;

/// `MicrophoneSlashIcon` (module 83107).
pub(super) const MIC_SLASH: &str = "voice_member_muted";
/// `MicrophoneDenyIcon` (module 666654).
pub(super) const MIC_DENY: &str = "overlay_server_muted";
/// `HeadphonesSlashIcon` (module 990836).
pub(super) const HEADPHONES_SLASH: &str = "overlay_headphones_slash";
/// `HeadphonesDenyIcon` (module 615675).
pub(super) const HEADPHONES_DENY: &str = "overlay_server_deafened";

/// What a person's voice state and this client's own choices say about their microphone and headphones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    /// `isVoiceMuted()` (module 288737): self mute, server mute, Stage suppression or a raised hand.
    pub mute: bool,
    /// `isVoiceDeafened()`: self or server deafen.
    pub deaf: bool,
    pub server_mute: bool,
    pub server_deaf: bool,
    /// Muted for this client only (`isLocalMute`, the person's menu).
    pub local_mute: bool,
}

impl Marks {
    pub(super) fn of(state: &VoiceState, local_mute: bool) -> Self {
        Self {
            mute: state.self_mute
                || state.mute
                || state.suppress
                || state.request_to_speak_timestamp.is_some(),
            deaf: state.self_deaf || state.deaf,
            server_mute: state.mute,
            server_deaf: state.deaf,
            local_mute,
        }
    }

    /// Whether any mark shows; `CallAvatar` then cuts the badge's place out of the avatar and drops its speaking ring.
    pub(super) fn any(self) -> bool {
        self.mute || self.deaf || self.local_mute
    }

    /// `CallAvatar`: `E ? MicrophoneDenyIcon : g ? HeadphonesSlashIcon : a ? MicrophoneSlashIcon : ...`.
    pub(super) fn call_avatar(self) -> Option<&'static str> {
        if self.local_mute {
            Some(MIC_DENY)
        } else if self.deaf {
            Some(HEADPHONES_SLASH)
        } else if self.mute {
            Some(MIC_SLASH)
        } else {
            None
        }
    }

    /// The tile's `eb`: `serverDeafened ? HeadphonesDeny : serverMuted ? MicrophoneDeny : deafened ? HeadphonesSlash :
    /// localMuted ? MicrophoneDeny : muted ? MicrophoneSlash : ...`.
    pub(super) fn tile(self) -> Option<&'static str> {
        if self.server_deaf {
            Some(HEADPHONES_DENY)
        } else if self.server_mute {
            Some(MIC_DENY)
        } else if self.deaf {
            Some(HEADPHONES_SLASH)
        } else if self.local_mute {
            Some(MIC_DENY)
        } else if self.mute {
            Some(MIC_SLASH)
        } else {
            None
        }
    }

    /// `Kc`'s marks in order, each with whether it is a server's (red). The channel list passes muted for me in
    /// `mute` too (module 588224 `mute: i || _`).
    pub(super) fn voice_user(self) -> Vec<(&'static str, bool)> {
        let mut marks = Vec::new();
        if self.mute || self.local_mute {
            let deny = self.server_mute || self.local_mute;
            marks.push((if deny { MIC_DENY } else { MIC_SLASH }, self.server_mute));
        }
        if self.deaf || self.server_deaf {
            marks.push((
                if self.server_deaf {
                    HEADPHONES_DENY
                } else {
                    HEADPHONES_SLASH
                },
                self.server_deaf,
            ));
        }
        marks
    }
}

/// `getAvatarSizeInfo` (module 778712) of a call avatar: (status, offset) for SIZE_80 and SIZE_40.
pub(super) fn call_status_size(size: f32) -> (f32, f32) {
    if size >= 80. { (16., 4.) } else { (12., 0.) }
}

/// `svg-mask-avatar-call-status-*` (module 573435 `S` with `g`): the hole cut out of the avatar under the badge, as
/// its centre from the avatar's top left and its radius, in pixels. `corner = (status / 2 + offset) / size`, the hole
/// `corner + 0.05` of the size across from the centre `1 - corner`.
pub(super) fn call_status_hole(size: f32) -> (f32, f32) {
    let (status, offset) = call_status_size(size);
    let corner = (status / 2. + offset) / size;
    (size * (1. - corner), size * (corner + 0.05))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> VoiceState {
        VoiceState::default()
    }

    #[test]
    fn a_deafened_person_shows_the_slashed_headphones_everywhere_not_the_microphone() {
        // Deafening also self-mutes, as the gateway reports it.
        let marks = Marks::of(
            &VoiceState {
                self_mute: true,
                self_deaf: true,
                ..state()
            },
            false,
        );
        assert_eq!(marks.call_avatar(), Some(HEADPHONES_SLASH));
        assert_eq!(marks.tile(), Some(HEADPHONES_SLASH));
        assert_eq!(
            marks.voice_user(),
            [(MIC_SLASH, false), (HEADPHONES_SLASH, false)]
        );
    }

    #[test]
    fn muted_only_shows_the_slashed_microphone() {
        let marks = Marks::of(
            &VoiceState {
                self_mute: true,
                ..state()
            },
            false,
        );
        assert_eq!(marks.call_avatar(), Some(MIC_SLASH));
        assert_eq!(marks.tile(), Some(MIC_SLASH));
        assert_eq!(marks.voice_user(), [(MIC_SLASH, false)]);
        assert!(Marks::default().voice_user().is_empty());
        assert_eq!(Marks::default().call_avatar(), None);
        assert_eq!(Marks::default().tile(), None);
    }

    #[test]
    fn server_states_take_the_deny_glyphs_and_red_where_the_module_colours_them() {
        let server_deaf = Marks::of(
            &VoiceState {
                deaf: true,
                mute: true,
                ..state()
            },
            false,
        );
        assert_eq!(server_deaf.tile(), Some(HEADPHONES_DENY));
        assert_eq!(
            server_deaf.voice_user(),
            [(MIC_DENY, true), (HEADPHONES_DENY, true)]
        );
        // `CallAvatar` knows no server state: a server deafen is a deafen there.
        assert_eq!(server_deaf.call_avatar(), Some(HEADPHONES_SLASH));
        let server_mute = Marks::of(
            &VoiceState {
                mute: true,
                ..state()
            },
            false,
        );
        assert_eq!(server_mute.tile(), Some(MIC_DENY));
        assert_eq!(server_mute.call_avatar(), Some(MIC_SLASH));
        // A Stage audience member is muted, not server muted.
        let audience = Marks::of(
            &VoiceState {
                suppress: true,
                ..state()
            },
            false,
        );
        assert_eq!(audience.voice_user(), [(MIC_SLASH, false)]);
    }

    #[test]
    fn muted_for_me_is_the_deny_microphone_and_leads_the_call_avatar() {
        let local = Marks::of(&state(), true);
        assert_eq!(local.call_avatar(), Some(MIC_DENY));
        assert_eq!(local.tile(), Some(MIC_DENY));
        assert_eq!(local.voice_user(), [(MIC_DENY, false)]);
        let local_and_deaf = Marks::of(
            &VoiceState {
                self_deaf: true,
                ..state()
            },
            true,
        );
        assert_eq!(local_and_deaf.call_avatar(), Some(MIC_DENY));
        assert_eq!(local_and_deaf.tile(), Some(HEADPHONES_SLASH));
    }

    #[test]
    fn the_avatar_hole_leaves_a_ring_around_the_badge() {
        // SIZE_80: a 24px badge at the bottom right, centre (68, 68); the hole is 16px across from it, a 4px ring.
        let (centre, radius) = call_status_hole(80.);
        assert!((centre - 68.).abs() < 1e-4);
        assert!((radius - 16.).abs() < 1e-4);
        // SIZE_40: a 12px badge at (34, 34), the hole 8px, a 2px ring.
        let (centre, radius) = call_status_hole(40.);
        assert!((centre - 34.).abs() < 1e-4);
        assert!((radius - 8.).abs() < 1e-4);
    }
}
