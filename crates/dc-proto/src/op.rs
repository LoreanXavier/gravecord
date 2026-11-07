//! Gateway opcodes, from the `eQ` enum in module 868.

macro_rules! opcodes {
    ($($name:ident = $value:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum Op {
            $($name = $value,)*
        }

        impl Op {
            pub fn from_u64(value: u64) -> Option<Self> {
                match value {
                    $($value => Some(Self::$name),)*
                    _ => None,
                }
            }
        }
    };
}

opcodes! {
    Dispatch = 0,
    Heartbeat = 1,
    Identify = 2,
    PresenceUpdate = 3,
    VoiceStateUpdate = 4,
    VoiceServerPing = 5,
    Resume = 6,
    Reconnect = 7,
    RequestGuildMembers = 8,
    InvalidSession = 9,
    Hello = 10,
    HeartbeatAck = 11,
    CallConnect = 13,
    GuildSubscriptions = 14,
    StreamCreate = 18,
    StreamDelete = 19,
    StreamWatch = 20,
    StreamPing = 21,
    StreamSetPaused = 22,
    RequestGuildApplicationCommands = 24,
    RequestForumUnreads = 28,
    RemoteCommand = 29,
    GetDeletedEntityIdsNotMatchingHash = 30,
    RequestSoundboardSounds = 31,
    RequestLastMessages = 34,
    SearchRecentMembers = 35,
    GuildSubscriptionsBulk = 37,
    GuildChannelsResync = 38,
    RequestChannelMemberCount = 39,
    QosHeartbeat = 40,
    UpdateTimeSpentSessionId = 41,
    RequestChannelInfo = 43,
}

#[cfg(test)]
mod tests {
    use super::Op;

    #[test]
    fn opcode_numbers_match_the_pinned_build() {
        assert_eq!(Op::QosHeartbeat as u8, 40);
        assert_eq!(Op::from_u64(43), Some(Op::RequestChannelInfo));
        assert_eq!(Op::from_u64(12), None);
    }
}
