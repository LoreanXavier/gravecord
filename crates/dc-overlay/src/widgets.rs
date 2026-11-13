//! The widget registry as the Widgets panel and the overlay read it.
//!
//! - The panel's order (`b` of module 776162): Voice, Quick Actions, Notifications, Chat, Friends, Activity, Video,
//!   Streams. A kind is listed when the registry (`rI` of module 415173) gives it a label, a description and an icon,
//!   and its `predicate` passes.
//! - The predicates read the `2025-11-overlay-chat` experiment (module 589051, `WidgetRenderConfig`): Chat needs
//!   `hasChat`, Friends needs `hasFriendList`, Activity is shown only without the friends list. Its default (an
//!   account with no assignment) has neither, so the panel then lists Voice, Quick Actions, Notifications, Activity,
//!   Video and Streams.
//! - The overlay draws a widget of the layout only when its predicate passes (`eT` of module 427830), and the Chat
//!   widget only while unlocked with a voice channel or a chat to show (its `containerRenderGate`).

use crate::layout;

/// The experiment the chat and friends widgets hang on (module 589051).
pub const CHAT_EXPERIMENT: &str = "2025-11-overlay-chat";

/// `2025-11-overlay-chat`'s config: the default and its three variations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ChatConfig {
    pub has_chat: bool,
    pub has_friend_list: bool,
    pub show_now_playing_for_different_games: bool,
}

impl ChatConfig {
    /// The config of the variation the account is in (`None`: no assignment, the default).
    pub fn of(variant: Option<i64>) -> Self {
        let (has_chat, has_friend_list, show_now_playing_for_different_games) = match variant {
            Some(1) => (true, false, false),
            Some(2) => (true, true, false),
            Some(3) => (true, true, true),
            _ => (false, false, false),
        };
        Self {
            has_chat,
            has_friend_list,
            show_now_playing_for_different_games,
        }
    }
}

/// `b` of module 776162.
pub const PANEL: [&str; 8] = [
    layout::VOICE,
    layout::QUICK_ACTIONS,
    layout::NOTIFICATIONS,
    layout::TEXT_CHAT,
    layout::FRIENDS,
    layout::ACTIVITY,
    layout::VIDEO,
    layout::GO_LIVE,
];

/// A registered widget's label, description and icon (en-US texts of the strings named, for `dc_intl::tr`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// The icon under `icons/` (the registry's `icon`).
    pub icon: &'static str,
}

/// `rI` of module 415173: labels `nFv3Gb`, `77gmRL`, `gnKWdS`, `/VQax8`, `TdEu5X`, `6gwSFY`, `UPvOia`, `386XRo`; the
/// descriptions are module 52482's.
pub const ENTRIES: [Entry; 8] = [
    Entry {
        kind: layout::VOICE,
        label: "Voice",
        description: "Who's talking in your call",
        icon: "overlay_voice_normal",
    },
    Entry {
        kind: layout::QUICK_ACTIONS,
        label: "Quick Actions",
        description: "Mute, deafen and more",
        icon: "overlay_microphone",
    },
    Entry {
        kind: layout::NOTIFICATIONS,
        label: "Notifications",
        description: "Messages, calls and invites",
        icon: "overlay_bell",
    },
    Entry {
        kind: layout::TEXT_CHAT,
        label: "Chat",
        description: "Chat without leaving your game",
        icon: "overlay_chat",
    },
    Entry {
        kind: layout::FRIENDS,
        label: "Friends",
        description: "Find friends and chat",
        icon: "overlay_friends",
    },
    Entry {
        kind: layout::ACTIVITY,
        label: "Activity",
        description: "What friends are playing",
        icon: "overlay_game_controller",
    },
    Entry {
        kind: layout::VIDEO,
        label: "Video",
        description: "Cameras in your call",
        icon: "overlay_video",
    },
    Entry {
        kind: layout::GO_LIVE,
        label: "Streams",
        description: "Streams in your call",
        icon: "overlay_screen",
    },
];

pub fn entry(kind: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.kind == kind)
}

/// The registry's `predicate` of a kind (no predicate passes).
pub fn predicate(kind: &str, config: ChatConfig) -> bool {
    match kind {
        layout::TEXT_CHAT => config.has_chat,
        layout::FRIENDS => config.has_friend_list,
        layout::ACTIVITY => !config.has_friend_list,
        _ => true,
    }
}

/// The panel's rows, in its order.
pub fn panel(config: ChatConfig) -> Vec<&'static Entry> {
    PANEL
        .iter()
        .filter(|kind| predicate(kind, config))
        .filter_map(|kind| entry(kind))
        .collect()
}

/// The Chat widget's `containerRenderGate`: only unlocked, and only with a voice channel or a tracked chat.
pub fn chat_container_shown(locked: bool, voice_channel: bool, has_entries: bool) -> bool {
    !locked && (voice_channel || has_entries)
}

/// The widgets of the layout the overlay draws, lowest first: hidden ones and failed predicates are left out.
pub fn drawn(layout: &layout::Layout, config: ChatConfig) -> Vec<layout::Widget> {
    layout
        .drawn()
        .into_iter()
        .filter(|widget| predicate(&widget.kind, config))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_lists_by_the_chat_experiment() {
        let kinds = |config| panel(config).iter().map(|e| e.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds(ChatConfig::of(None)),
            [
                layout::VOICE,
                layout::QUICK_ACTIONS,
                layout::NOTIFICATIONS,
                layout::ACTIVITY,
                layout::VIDEO,
                layout::GO_LIVE
            ]
        );
        assert_eq!(
            kinds(ChatConfig::of(Some(1))),
            [
                layout::VOICE,
                layout::QUICK_ACTIONS,
                layout::NOTIFICATIONS,
                layout::TEXT_CHAT,
                layout::ACTIVITY,
                layout::VIDEO,
                layout::GO_LIVE
            ]
        );
        assert_eq!(
            kinds(ChatConfig::of(Some(3))),
            [
                layout::VOICE,
                layout::QUICK_ACTIONS,
                layout::NOTIFICATIONS,
                layout::TEXT_CHAT,
                layout::FRIENDS,
                layout::VIDEO,
                layout::GO_LIVE
            ]
        );
        assert!(ChatConfig::of(Some(3)).show_now_playing_for_different_games);
        assert_eq!(ChatConfig::of(Some(9)), ChatConfig::default());
        assert_eq!(entry(layout::GO_LIVE).unwrap().label, "Streams");
    }

    #[test]
    fn the_overlay_draws_only_widgets_whose_predicate_passes() {
        let mut layout = layout::Layout::default();
        let mut next = 0;
        layout.ensure(&mut || {
            next += 1;
            format!("w{next}")
        });
        let off = drawn(&layout, ChatConfig::default());
        assert!(off.iter().all(|widget| widget.kind != layout::TEXT_CHAT));
        assert!(off.iter().all(|widget| widget.kind != layout::FRIENDS));
        let on = drawn(&layout, ChatConfig::of(Some(2)));
        assert!(on.iter().any(|widget| widget.kind == layout::TEXT_CHAT));
        assert!(on.iter().all(|widget| widget.kind != layout::ACTIVITY));
        assert!(!chat_container_shown(true, true, true));
        assert!(!chat_container_shown(false, false, false));
        assert!(chat_container_shown(false, false, true));
        assert!(chat_container_shown(false, true, false));
    }
}
