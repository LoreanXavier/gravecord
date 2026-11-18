//! The widgets' Settings menus.
//!
//! - The voice widget's (module 51444, `overlay-voice-widget-context-menu`, "Overlay Actions"): the radio groups
//!   Avatar Size (Large, Small), Display Names (Always, Only While Speaking, Never) and Display Users (Always, Only
//!   While Speaking) of `OverlayStoreV2`, then the Max Users Displayed slider of the widget's
//!   `meta.voiceStatesMaxShown` (0 to 25, under 1 shown as "Off" and stored as -1, else the floor).
//! - The notifications widget's (module 320490, `overlay-notifications-widget-context-menu`): the one checkbox "Show
//!   text chat notifications", ticked while `TEXT_CHAT` is not among the disabled notification settings; a click
//!   flips it (`OVERLAY_SET_NOTIFICATION_DISABLED_SETTING`).
//!
//! - The Video widget's (module 662554, `overlay-video-widget-context-menu`, "Overlay Actions"): Show My Own Camera
//!   (module 575079 `self-video-hide`), Horizontal Layout (module 915908 `widget-layout-type`, ticked unless
//!   `meta.horizontal` is `false`), and the Opacity slider (module 871648: 0 to 100, the widget's `opacity` times 100,
//!   written as `LAYOUT_UPDATE_WIDGET {opacity: value / 100}`).
//! - The Go Live widget's (module 543824, `overlay-go-live-widget-context-menu`): Horizontal Layout (ticked only when
//!   `meta.horizontal` is `true`) and the Opacity slider.
//!
//! Horizontal Layout flips `meta.horizontal` (`LAYOUT_SET_WIDGET_META`) and closes the menu (`Z_`).
//!
//! The menus write the same stores the Game Overlay page writes, so either shows the other's change at once.

use crate::layout::Layout;
use crate::store::{self, AvatarSize, DisplayNameMode, DisplayUserMode, OverlaySettings};
use serde_json::Value;

/// The slider's `minValue` and `maxValue`.
pub const MAX_USERS_MIN: f32 = 0.;
pub const MAX_USERS_MAX: f32 = 25.;

/// Which widget's menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Voice,
    Notifications,
    Video,
    GoLive,
}

/// One choice of a menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    AvatarSize(AvatarSize),
    DisplayNames(DisplayNameMode),
    DisplayUsers(DisplayUserMode),
    /// The slider's value (`j`: under 1 is "Off").
    MaxUsers(f32),
    /// The notifications menu's checkbox.
    ToggleTextChatNotifications,
    /// Horizontal Layout of the video or Go Live widget.
    Horizontal,
    /// The Opacity slider's value, 0 to 100.
    Opacity(f32),
    /// Show My Own Camera (module 575079).
    ShowOwnCamera,
}

/// The Opacity slider's `maxValue`.
pub const OPACITY_MAX: f32 = 100.;

/// The layout after a widget's own action (Horizontal Layout, Opacity) on widget `id`; false for another action.
pub fn apply_widget(layout: &mut Layout, id: &str, action: Action) -> bool {
    let Some(widget) = layout.widgets().into_iter().find(|widget| widget.id == id) else {
        return false;
    };
    match action {
        Action::Horizontal => layout.set_meta(id, "horizontal", Value::Bool(!widget.horizontal())),
        Action::Opacity(value) => {
            layout.set_opacity(id, value.clamp(0., OPACITY_MAX) / OPACITY_MAX)
        }
        _ => false,
    }
}

/// The Opacity slider's knob as a fraction of its track.
pub fn opacity_fraction(opacity: f32) -> f32 {
    opacity.clamp(0., 1.)
}

/// The slider's value, `100 * t.opacity` (rounded: a stored fraction such as 0.42 is not exact in binary).
pub fn opacity_label(opacity: f32) -> u32 {
    (opacity.clamp(0., 1.) * OPACITY_MAX).round() as u32
}

/// `renderValue`: `None` for "Off", else the whole count.
pub fn max_users_label(value: f32) -> Option<u32> {
    (value >= 1.).then(|| value.floor() as u32)
}

/// The slider's knob as a fraction of its track for a stored value (-1, Off, sits at the start).
pub fn max_users_fraction(stored: i64) -> f32 {
    ((stored.max(0) as f32 - MAX_USERS_MIN) / (MAX_USERS_MAX - MAX_USERS_MIN)).clamp(0., 1.)
}

/// The slider's value at a fraction of its track.
pub fn max_users_at(fraction: f32) -> f32 {
    MAX_USERS_MIN + fraction.clamp(0., 1.) * (MAX_USERS_MAX - MAX_USERS_MIN)
}

/// The settings an `OverlayStoreV2` action leaves; `None` for an action of another store.
pub fn apply_settings(settings: &OverlaySettings, action: Action) -> Option<OverlaySettings> {
    let mut next = settings.clone();
    match action {
        Action::AvatarSize(size) => next.avatar_size_mode = size,
        Action::DisplayNames(mode) => next.display_name_mode = mode,
        Action::DisplayUsers(mode) => next.display_user_mode = mode,
        _ => return None,
    }
    Some(next)
}

/// The layout after a `MaxUsers` action (the voice widget's meta); false for another action. As module 51444's `j`,
/// it changes nothing without a voice widget in the layout.
pub fn apply_layout(
    layout: &mut Layout,
    action: Action,
    new_id: &mut dyn FnMut() -> String,
) -> bool {
    let Action::MaxUsers(value) = action else {
        return false;
    };
    if layout
        .widget(crate::layout::VOICE)
        .is_none_or(|widget| widget.id.is_empty())
    {
        return false;
    }
    layout.set_voice_max_shown(value, new_id);
    true
}

/// The `OverlaySettingsStore` object after the checkbox's click; `None` for another action.
pub fn apply_notification_settings(stored: &Value, action: Action) -> Option<Value> {
    if action != Action::ToggleTextChatNotifications {
        return None;
    }
    let disabled = store::notification_disabled(stored, store::TEXT_CHAT);
    Some(store::set_notification_disabled(
        stored,
        store::TEXT_CHAT,
        !disabled,
    ))
}

/// Whether a radio row or the checkbox is ticked now.
pub fn checked(action: Action, settings: &OverlaySettings, notifications: &Value) -> bool {
    match action {
        Action::AvatarSize(size) => settings.avatar_size_mode == size,
        Action::DisplayNames(mode) => settings.display_name_mode == mode,
        Action::DisplayUsers(mode) => settings.display_user_mode == mode,
        Action::MaxUsers(_) | Action::Horizontal | Action::Opacity(_) | Action::ShowOwnCamera => {
            false
        }
        Action::ToggleTextChatNotifications => {
            !store::notification_disabled(notifications, store::TEXT_CHAT)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_voice_menu_writes_the_page_stores() {
        let settings = OverlaySettings::default();
        let small = apply_settings(&settings, Action::AvatarSize(AvatarSize::Small)).unwrap();
        assert_eq!(small.avatar_size_mode, AvatarSize::Small);
        assert!(checked(
            Action::AvatarSize(AvatarSize::Small),
            &small,
            &Value::Null
        ));
        assert!(!checked(
            Action::AvatarSize(AvatarSize::Large),
            &small,
            &Value::Null
        ));
        let never = apply_settings(&small, Action::DisplayNames(DisplayNameMode::Never)).unwrap();
        assert_eq!(never.display_name_mode, DisplayNameMode::Never);
        assert_eq!(never.avatar_size_mode, AvatarSize::Small);
        let speaking = apply_settings(
            &never,
            Action::DisplayUsers(DisplayUserMode::OnlyWhileSpeaking),
        )
        .unwrap();
        assert_eq!(
            speaking.display_user_mode,
            DisplayUserMode::OnlyWhileSpeaking
        );
        // Round trip through the stored object the page reads.
        let stored = store::with_settings(&Value::Null, "1", &speaking);
        assert_eq!(store::settings_for(&stored, "1"), speaking);
        assert_eq!(apply_settings(&settings, Action::MaxUsers(3.)), None);
    }

    #[test]
    fn the_slider_stores_off_as_minus_one_and_floors() {
        let mut ids = {
            let mut next = 0;
            move || {
                next += 1;
                format!("w{next}")
            }
        };
        let mut layout = Layout::default();
        // No layout yet: no voice widget to write to.
        assert!(!apply_layout(&mut layout, Action::MaxUsers(4.), &mut ids));
        layout.ensure(&mut ids);
        assert!(apply_layout(&mut layout, Action::MaxUsers(4.6), &mut ids));
        assert_eq!(layout.voice_max_shown(), 4);
        assert!(apply_layout(&mut layout, Action::MaxUsers(0.3), &mut ids));
        assert_eq!(layout.voice_max_shown(), -1);
        assert!(!apply_layout(
            &mut layout,
            Action::ToggleTextChatNotifications,
            &mut ids
        ));
        assert_eq!(max_users_label(0.3), None);
        assert_eq!(max_users_label(12.9), Some(12));
        assert_eq!(max_users_fraction(-1), 0.);
        assert_eq!(max_users_fraction(25), 1.);
        assert_eq!(max_users_at(0.5), 12.5);
        assert_eq!(max_users_at(2.), 25.);
    }

    #[test]
    fn the_video_and_go_live_menus_write_their_widget() {
        let mut ids = {
            let mut next = 0;
            move || {
                next += 1;
                format!("w{next}")
            }
        };
        let mut layout = Layout::default();
        layout.ensure(&mut ids);
        let video = layout.widget(crate::layout::VIDEO).unwrap();
        let go_live = layout.widget(crate::layout::GO_LIVE).unwrap();
        // Defaults: the video widget is horizontal, the Go Live one is not.
        assert!(video.horizontal());
        assert!(!go_live.horizontal());
        assert!(apply_widget(&mut layout, &video.id, Action::Horizontal));
        assert!(apply_widget(&mut layout, &go_live.id, Action::Horizontal));
        assert!(!layout.widget(crate::layout::VIDEO).unwrap().horizontal());
        assert!(layout.widget(crate::layout::GO_LIVE).unwrap().horizontal());
        assert!(apply_widget(&mut layout, &go_live.id, Action::Opacity(42.)));
        let after = layout.widget(crate::layout::GO_LIVE).unwrap();
        assert!((after.opacity - 0.42).abs() < 1e-6);
        assert_eq!(opacity_label(after.opacity), 42);
        // Stored as the official object: survives a round trip.
        let again = Layout::from_json(&layout.to_json());
        assert!((again.widget(crate::layout::GO_LIVE).unwrap().opacity - 0.42).abs() < 1e-6);
        assert!(!apply_widget(&mut layout, "nope", Action::Horizontal));
        assert!(!apply_widget(&mut layout, &video.id, Action::MaxUsers(3.)));
        assert!(apply_widget(&mut layout, &video.id, Action::Opacity(150.)));
        assert_eq!(layout.widget(crate::layout::VIDEO).unwrap().opacity, 1.);
    }

    #[test]
    fn the_notifications_checkbox_flips_text_chat() {
        let stored = json!({"gameSettings": {"x": 1}});
        assert!(checked(
            Action::ToggleTextChatNotifications,
            &OverlaySettings::default(),
            &stored
        ));
        let off =
            apply_notification_settings(&stored, Action::ToggleTextChatNotifications).unwrap();
        assert!(store::notification_disabled(&off, store::TEXT_CHAT));
        assert_eq!(off["gameSettings"]["x"], 1);
        assert!(!checked(
            Action::ToggleTextChatNotifications,
            &OverlaySettings::default(),
            &off
        ));
        let on = apply_notification_settings(&off, Action::ToggleTextChatNotifications).unwrap();
        assert!(!store::notification_disabled(&on, store::TEXT_CHAT));
        assert_eq!(apply_notification_settings(&on, Action::MaxUsers(1.)), None);
    }
}
