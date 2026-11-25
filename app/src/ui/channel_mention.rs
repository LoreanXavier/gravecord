//! What a `<#id>` channel mention shows (the `channelMention` rule of module 605275: `w` looks the id up in the
//! channel store, `F` builds the mention, `k` the unknown one) and the icon it carries (`QG` of module 147036, drawn by
//! module 232042). There is no fetch: an id the store does not hold reads "unknown" in italics with the text icon until
//! the channel arrives by other means (a guild's threads, a forum's post list), and then reads its name.

use super::Root;
use dc_state::permissions::{self, CONNECT, VIEW_CHANNEL};
use dc_state::{ChannelKind, Id};

/// `ChannelFlags.IS_SPOILER_CHANNEL`.
const SPOILER_FLAG: u64 = 1 << 21;

/// One `<#id>` as the message shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Look {
    /// A mention pill: its icon (one of `icons/`), its text, whether the text is in italics (the unknown channel),
    /// and the channel a click opens (`(guild, channel)`; none for the unknown and the inaccessible ones).
    Pill {
        icon: &'static str,
        text: String,
        italic: bool,
        open: Option<(Id, Id)>,
    },
    /// Plain text: a channel the rule does not make a mention of (a category, `isMentionable`), or a channel of a
    /// server this client does not have (`#unknown-channel`).
    Plain(String),
}

/// What `QG` says of a channel, as one of our icons. `connect` is the CONNECT permission (voice and stage).
fn icon_for(kind: u64, nsfw: bool, spoiler: bool, connect: bool, post: bool) -> &'static str {
    match kind {
        // GUILD_VOICE: "voice", "voice-nsfw", "voice-spoiler" or "voice-locked".
        2 if !connect => "icons/channel_lock.svg",
        2 if nsfw => "icons/voice_channel_nsfw.svg",
        2 if spoiler => "icons/voice_channel_spoiler.svg",
        2 => "icons/channel_type_voice.svg",
        // GUILD_STAGE_VOICE: "stage" or "stage-locked".
        13 if !connect => "icons/channel_lock.svg",
        13 => "icons/stage_channel.svg",
        // Threads: "post" (ChatIcon) under a forum or media channel, "thread" otherwise.
        10..=12 if post => "icons/chat_bubble.svg",
        10..=12 => "icons/threads.svg",
        // GUILD_FORUM, GUILD_MEDIA.
        15 if nsfw => "icons/forum_channel_nsfw.svg",
        15 => "icons/forum_channel.svg",
        16 if nsfw => "icons/media_channel_nsfw.svg",
        16 => "icons/media_channel.svg",
        // GUILD_ANNOUNCEMENT ("announcement-spoiler" and "forum-spoiler" have no art of their own here).
        5 if nsfw => "icons/announcement_channel_nsfw.svg",
        5 => "icons/announcement_channel.svg",
        // GUILD_APP ("app-nsfw" and "app-spoiler" have no art of their own here).
        21 => "icons/cp_apps.svg",
        // Text and everything else: "text", "text-nsfw", "text-spoiler".
        _ if nsfw => "icons/text_channel_nsfw.svg",
        _ if spoiler => "icons/text_channel_spoiler.svg",
        _ => "icons/text_channel.svg",
    }
}

fn kind_number(kind: &ChannelKind) -> u64 {
    match kind {
        ChannelKind::Text => 0,
        ChannelKind::Voice => 2,
        ChannelKind::Category => 4,
        ChannelKind::Other(number) => *number,
    }
}

/// `k`: an id the store does not hold. `zLZPmk` lowercased, in italics, with the text icon.
fn unknown() -> Look {
    Look::Pill {
        icon: "icons/text_channel.svg",
        text: dc_intl::t("zLZPmk").to_lowercase(),
        italic: true,
        open: None,
    }
}

/// `F` for a channel the account may not view: "No Access" (`/YzI63`) with the lock.
fn no_access() -> Look {
    Look::Pill {
        icon: "icons/channel_lock.svg",
        text: dc_intl::t("/YzI63").to_owned(),
        italic: false,
        open: None,
    }
}

impl Root {
    /// `w` then `F` (or `k`) for `<#id>`.
    pub(super) fn channel_mention_look(&self, id: Id) -> Look {
        let me = self.stores.me();
        // The server in view first, then the others (the store holds every channel the session knows).
        let guilds = self.current_guild().into_iter().chain(
            self.stores
                .guilds
                .iter()
                .filter(|guild| Some(guild.id) != self.guild),
        );
        for guild in guilds {
            if let Some(channel) = guild.channels.iter().find(|channel| channel.id == id) {
                let kind = kind_number(&channel.kind);
                let can = |permission| {
                    me.is_some_and(|me| permissions::can_in_channel(guild, channel, me, permission))
                };
                if !can(VIEW_CHANNEL) {
                    return no_access();
                }
                // `isMentionable`: every type but a category.
                if kind == 4 {
                    return Look::Plain(format!("#{}", channel.name));
                }
                return Look::Pill {
                    icon: icon_for(
                        kind,
                        channel.nsfw,
                        channel.flags & SPOILER_FLAG != 0,
                        can(CONNECT),
                        false,
                    ),
                    text: channel.name.clone(),
                    italic: false,
                    open: Some((guild.id, id)),
                };
            }
            if let Some(thread) = guild.threads.iter().find(|thread| thread.id == id) {
                let parent = guild
                    .channels
                    .iter()
                    .find(|channel| channel.id == thread.parent_id);
                // A thread is viewed through its parent.
                if let Some(parent) = parent
                    && !me.is_some_and(|me| {
                        permissions::can_in_channel(guild, parent, me, VIEW_CHANNEL)
                    })
                {
                    return no_access();
                }
                let post =
                    parent.is_some_and(|parent| matches!(kind_number(&parent.kind), 15 | 16));
                return Look::Pill {
                    icon: icon_for(thread.kind, false, false, true, post),
                    text: thread.name.clone(),
                    italic: false,
                    open: Some((guild.id, id)),
                };
            }
        }
        if let Some(private) = self
            .stores
            .private_channels
            .iter()
            .find(|channel| channel.id == id)
        {
            return Look::Pill {
                icon: "icons/text_channel.svg",
                text: self.private_channel_name(private),
                italic: false,
                open: Some((0, id)),
            };
        }
        unknown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_channel_type_takes_its_official_icon() {
        assert_eq!(
            icon_for(0, false, false, true, false),
            "icons/text_channel.svg"
        );
        assert_eq!(
            icon_for(0, true, false, true, false),
            "icons/text_channel_nsfw.svg"
        );
        assert_eq!(
            icon_for(0, false, true, true, false),
            "icons/text_channel_spoiler.svg"
        );
        assert_eq!(
            icon_for(2, false, false, true, false),
            "icons/channel_type_voice.svg"
        );
        assert_eq!(
            icon_for(2, false, false, false, false),
            "icons/channel_lock.svg"
        );
        assert_eq!(
            icon_for(13, false, false, true, false),
            "icons/stage_channel.svg"
        );
        assert_eq!(
            icon_for(11, false, false, true, true),
            "icons/chat_bubble.svg"
        );
        assert_eq!(icon_for(11, false, false, true, false), "icons/threads.svg");
        assert_eq!(
            icon_for(15, false, false, true, false),
            "icons/forum_channel.svg"
        );
        assert_eq!(
            icon_for(5, false, false, true, false),
            "icons/announcement_channel.svg"
        );
    }

    #[test]
    fn an_unknown_channel_reads_unknown_in_italics() {
        assert_eq!(
            unknown(),
            Look::Pill {
                icon: "icons/text_channel.svg",
                text: "unknown".to_owned(),
                italic: true,
                open: None,
            }
        );
    }
}
