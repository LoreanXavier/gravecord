//! The channel list's icon for a channel and the icon's tooltip: `gU` and `Bq` of 713654, as `ChannelItemIcon`
//! (166444) calls them, with `locked` and `hasActiveThreads` and nothing else.
use dc_state::ChannelKind;

const ANNOUNCEMENT: ChannelKind = ChannelKind::Other(5);
const STAGE: ChannelKind = ChannelKind::Other(13);
const FORUM: ChannelKind = ChannelKind::Other(15);
const MEDIA: ChannelKind = ChannelKind::Other(16);

#[derive(Clone, Copy, Debug)]
pub(super) struct Facts {
    pub kind: ChannelKind,
    /// The guild's rules channel.
    pub rules: bool,
    pub nsfw: bool,
    /// 148719: @everyone cannot connect (voice, stage), or the @everyone overwrite denies View Channel.
    pub private: bool,
    /// 51758 `V`: the account is still pending in a guild with the membership gate.
    pub pending: bool,
    /// The row's `locked`: the account cannot connect. Only read for voice and stage.
    pub locked: bool,
    pub active_threads: bool,
}

/// The plain padlock: a voice or stage channel the account cannot enter.
const LOCK: &str = "channel_private_lock";

pub(super) fn name(facts: &Facts) -> &'static str {
    if facts.rules {
        return "ob_rules";
    }
    let pick = |nsfw: &'static str, lock: &'static str, plain: &'static str| {
        if facts.nsfw {
            nsfw
        } else if facts.private {
            lock
        } else {
            plain
        }
    };
    // Voice and stage: a pending member sees every one locked, the private ones as the plain padlock.
    let vocal = |lock: &'static str, plain: &'static str| {
        if facts.pending {
            if facts.private { LOCK } else { lock }
        } else if facts.locked {
            LOCK
        } else if facts.private {
            lock
        } else {
            plain
        }
    };
    match facts.kind {
        ANNOUNCEMENT => pick(
            "announcement_channel_nsfw",
            "announcement_channel_lock",
            "announcement_channel",
        ),
        FORUM => pick("forum_channel_nsfw", "forum_channel_lock", "forum_channel"),
        MEDIA => pick("media_channel_nsfw", "media_channel_lock", "media_channel"),
        STAGE => vocal("stage_channel_lock", "stage_channel"),
        ChannelKind::Voice if facts.nsfw => "voice_channel_nsfw",
        ChannelKind::Voice => vocal("voice_channel_lock", "voice_channel"),
        _ => pick("text_channel_nsfw", "text_channel_lock", "text_channel"),
    }
}

/// The tooltip on the icon; a kind the official names nothing for has none.
pub(super) fn label(facts: &Facts) -> Option<&'static str> {
    if facts.rules {
        return Some("Rules");
    }
    let pick = |nsfw: &'static str, limited: &'static str, plain: &'static str| {
        if facts.nsfw {
            nsfw
        } else if facts.private {
            limited
        } else {
            plain
        }
    };
    Some(match facts.kind {
        ChannelKind::Text if facts.active_threads => "Text (Active Threads)",
        ChannelKind::Text => pick("Text (Age-Restricted)", "Text (Limited)", "Text"),
        FORUM => pick("Forum (Age-Restricted)", "Forum (Limited)", "Forum"),
        MEDIA => pick(
            "Media Channel (Age-Restricted)",
            "Media Channel (Limited)",
            "Media Channel",
        ),
        STAGE if facts.locked => "Stage (Locked)",
        STAGE if facts.private => "Stage (Limited)",
        STAGE => "Stage",
        ChannelKind::Voice if facts.locked => "Voice (Locked)",
        ChannelKind::Voice => pick("Voice (Age-Restricted)", "Voice (Limited)", "Voice"),
        ANNOUNCEMENT => pick(
            "Announcements (Age-Restricted)",
            "Announcements (Limited)",
            "Announcements",
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(kind: ChannelKind) -> Facts {
        Facts {
            kind,
            rules: false,
            nsfw: false,
            private: false,
            pending: false,
            locked: false,
            active_threads: false,
        }
    }

    #[test]
    fn a_pending_member_sees_every_voice_channel_locked_and_private_ones_as_a_padlock() {
        // The two voice rows of the reference server for an account that has not passed the gate.
        let open = Facts {
            pending: true,
            locked: true,
            ..facts(ChannelKind::Voice)
        };
        assert_eq!(name(&open), "voice_channel_lock");
        assert_eq!(label(&open), Some("Voice (Locked)"));
        let private = Facts {
            private: true,
            ..open
        };
        assert_eq!(name(&private), "channel_private_lock");
    }

    #[test]
    fn voice_icons_tell_private_from_locked_for_a_full_member() {
        let plain = facts(ChannelKind::Voice);
        assert_eq!(name(&plain), "voice_channel");
        assert_eq!(label(&plain), Some("Voice"));
        // Private, and the account has Connect.
        let private = Facts {
            private: true,
            ..plain
        };
        assert_eq!(name(&private), "voice_channel_lock");
        assert_eq!(label(&private), Some("Voice (Limited)"));
        // Private, and the account lacks Connect.
        let locked = Facts {
            locked: true,
            ..private
        };
        assert_eq!(name(&locked), "channel_private_lock");
        assert_eq!(label(&locked), Some("Voice (Locked)"));
        let nsfw = Facts {
            nsfw: true,
            ..plain
        };
        assert_eq!(name(&nsfw), "voice_channel_nsfw");
        assert_eq!(label(&nsfw), Some("Voice (Age-Restricted)"));
    }

    #[test]
    fn text_like_channels_put_age_restriction_before_privacy() {
        for (kind, plain, lock, nsfw) in [
            (
                ChannelKind::Text,
                "text_channel",
                "text_channel_lock",
                "text_channel_nsfw",
            ),
            (
                ANNOUNCEMENT,
                "announcement_channel",
                "announcement_channel_lock",
                "announcement_channel_nsfw",
            ),
            (
                FORUM,
                "forum_channel",
                "forum_channel_lock",
                "forum_channel_nsfw",
            ),
            (
                MEDIA,
                "media_channel",
                "media_channel_lock",
                "media_channel_nsfw",
            ),
        ] {
            let base = facts(kind);
            assert_eq!(name(&base), plain);
            let private = Facts {
                private: true,
                ..base
            };
            assert_eq!(name(&private), lock);
            assert_eq!(
                name(&Facts {
                    nsfw: true,
                    ..private
                }),
                nsfw
            );
        }
        let threads = Facts {
            active_threads: true,
            private: true,
            ..facts(ChannelKind::Text)
        };
        assert_eq!(label(&threads), Some("Text (Active Threads)"));
        assert_eq!(name(&threads), "text_channel_lock");
        assert_eq!(
            label(&Facts {
                private: true,
                ..facts(FORUM)
            }),
            Some("Forum (Limited)")
        );
    }

    #[test]
    fn the_rules_channel_and_stage_channels() {
        let rules = Facts {
            rules: true,
            private: true,
            ..facts(ChannelKind::Text)
        };
        assert_eq!(name(&rules), "ob_rules");
        assert_eq!(label(&rules), Some("Rules"));
        let stage = facts(STAGE);
        assert_eq!(name(&stage), "stage_channel");
        assert_eq!(label(&stage), Some("Stage"));
        let locked = Facts {
            locked: true,
            ..stage
        };
        assert_eq!(name(&locked), "channel_private_lock");
        assert_eq!(label(&locked), Some("Stage (Locked)"));
        assert_eq!(label(&facts(ChannelKind::Category)), None);
    }
}
