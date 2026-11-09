//! Local notification preferences and desktop delivery. Eligibility lives in dc-state.
use crate::{net::Update, storage};
use dc_state::Id;
use serde_json::{Value, json};
use std::sync::{
    Mutex, OnceLock,
    mpsc::{SyncSender, sync_channel},
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

pub enum SoundAction {
    Play,
    Schedule(Duration),
    Pending,
}
#[derive(Default)]
pub struct SoundThrottle {
    last: Option<Instant>,
    pending: Option<&'static str>,
    scheduled: bool,
}
impl SoundThrottle {
    pub fn request(&mut self, sound: &'static str, now: Instant) -> SoundAction {
        if self.scheduled {
            self.pending = Some(sound);
            return SoundAction::Pending;
        }
        let interval = Duration::from_secs(1);
        let elapsed = self.last.map(|last| now.saturating_duration_since(last));
        if elapsed.is_none_or(|elapsed| elapsed >= interval) {
            self.last = Some(now);
            return SoundAction::Play;
        }
        self.pending = Some(sound);
        self.scheduled = true;
        SoundAction::Schedule(interval.saturating_sub(elapsed.unwrap_or_default()))
    }
    pub fn finish(&mut self, now: Instant) -> Option<&'static str> {
        self.scheduled = false;
        let sound = self.pending.take();
        if sound.is_some() {
            self.last = Some(now);
        }
        sound
    }
}

#[cfg(windows)]
mod badge_art;
#[cfg(windows)]
mod native_icon;
#[cfg(windows)]
mod taskbar;
#[cfg(windows)]
mod window_hook;
#[cfg(windows)]
mod windows;
pub const APP_ID: &str = "gravecord.desktop";

pub fn configure_process() {
    if storage::is_capture_run() {
        return;
    }
    #[cfg(windows)]
    if let Err(error) = unsafe {
        ::windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
            &::windows::core::HSTRING::from(APP_ID),
        )
    } {
        tracing::warn!(code=?error.code(), "desktop application identity unavailable");
    }
}

pub fn observe_window(window: &gpui::Window) {
    if storage::is_capture_run() {
        return;
    }
    #[cfg(windows)]
    window_hook::install(window);
    #[cfg(not(windows))]
    let _ = window;
}

pub struct Notice {
    pub channel: Id,
    pub message: Id,
    pub title: String,
    pub body: String,
}
pub(super) enum Command {
    Show(Notice),
    Clear(Id),
    Window(usize),
    Badge(i64),
    Flash(bool),
    TaskbarReady,
}

pub struct Runtime {
    sender: Option<SyncSender<Command>>,
    capture: bool,
    badge: Option<i64>,
    #[cfg(windows)]
    binding: Option<window_hook::Binding>,
    #[cfg(windows)]
    taskbar_generation: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl Runtime {
    pub fn new(updates: UnboundedSender<Update>) -> Self {
        let capture = storage::is_capture_run();
        #[cfg(windows)]
        let taskbar_generation = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        if capture {
            return Self {
                sender: None,
                capture,
                badge: None,
                #[cfg(windows)]
                binding: None,
                #[cfg(windows)]
                taskbar_generation,
            };
        }
        let (sender, receiver) = sync_channel(20);
        #[cfg(windows)]
        let worker_generation = taskbar_generation.clone();
        #[cfg(windows)]
        if std::thread::Builder::new()
            .name("gravecord-notifications".to_owned())
            .spawn(move || windows::run(receiver, updates, worker_generation))
            .is_err()
        {
            tracing::warn!("could not start the notification worker");
            return Self {
                sender: None,
                capture,
                badge: None,
                #[cfg(windows)]
                binding: None,
                #[cfg(windows)]
                taskbar_generation,
            };
        }
        #[cfg(not(windows))]
        let _ = (receiver, updates);
        Self {
            sender: Some(sender),
            capture,
            badge: None,
            #[cfg(windows)]
            binding: None,
            #[cfg(windows)]
            taskbar_generation,
        }
    }
    pub fn attach(&mut self, window: &gpui::Window) {
        if self.capture {
            return;
        }
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = HasWindowHandle::window_handle(window)
                && let RawWindowHandle::Win32(handle) = handle.as_raw()
                && let Some(sender) = &self.sender
            {
                let _ = sender.try_send(Command::Window(handle.hwnd.get() as usize));
                self.binding = window_hook::bind(window, sender, &self.taskbar_generation);
            }
        }
        #[cfg(not(windows))]
        let _ = window;
    }
    pub fn badge(&mut self, count: i64) {
        if self.badge == Some(count) {
            return;
        }
        if self.capture {
            record(if count < 0 {
                "badge-unread"
            } else if count == 0 {
                "badge-clear"
            } else {
                "badge-count"
            });
            self.badge = Some(count);
        } else if let Some(sender) = &self.sender
            && sender.try_send(Command::Badge(count)).is_ok()
        {
            self.badge = Some(count);
        }
    }
    pub fn flash(&self, enabled: bool) {
        if self.capture {
            record(if enabled { "flash-start" } else { "flash-stop" });
        } else if let Some(sender) = &self.sender {
            let _ = sender.try_send(Command::Flash(enabled));
        }
    }
    pub fn show(&self, notice: Notice) {
        if self.capture {
            record("toast");
            return;
        }
        if let Some(sender) = &self.sender
            && sender.try_send(Command::Show(notice)).is_err()
        {
            tracing::debug!("notification worker queue unavailable");
        }
    }
    pub fn clear(&self, channel: Id) {
        if self.capture {
            record("clear-channel");
            return;
        }
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(Command::Clear(channel));
        }
    }
}

fn trace() -> &'static Mutex<Vec<&'static str>> {
    static TRACE: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    TRACE.get_or_init(Mutex::default)
}
fn record(event: &'static str) {
    tracing::debug!(event, "capture desktop notification no-op");
    if let Ok(mut events) = trace().lock() {
        events.push(event);
    }
}
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub fn events() -> Vec<&'static str> {
    trace()
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default()
}

#[cfg(all(windows, feature = "capture"))]
pub fn capture_badge_art() {
    let path = std::path::Path::new("design/notification-icons");
    if std::fs::create_dir_all(path).is_err() {
        return;
    }
    let font = crate::fonts::semibold();
    for index in [0, 9, 10] {
        if let Some(mut pixels) = badge_art::raster(index, font.as_deref()) {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                let alpha = u16::from(pixel[3]);
                for channel in &mut pixel[..3] {
                    *channel = (u16::from(*channel) * 255)
                        .checked_div(alpha)
                        .unwrap_or(0)
                        .min(255) as u8;
                }
            }
            if let Some(image) = image::RgbaImage::from_raw(16, 16, pixels) {
                let _ = image.save(path.join(format!("rendered-badge-{}.png", index + 1)));
            }
        }
    }
}

fn setting(key: &str) -> Option<(&'static str, &'static str, bool, bool)> {
    Some(match key {
        "desktop_notifications" => ("notifications", "desktopType", true, false),
        "task_bar_flashing" => ("notifications", "taskbarFlash", true, false),
        "enable_unread_message_badge" => ("notifications", "disableUnreadBadge", true, true),
        "streaming_enable_streamer_mode" => ("StreamerModeStore", "enabled", false, false),
        "streaming_auto_streamer_mode" => ("StreamerModeStore", "autoToggle", true, false),
        "streamer_mode_disable_sounds" => ("StreamerModeStore", "disableSounds", true, false),
        "streamer_mode_disable_notifications" => {
            ("StreamerModeStore", "disableNotifications", true, false)
        }
        _ => return None,
    })
}
fn read_switch(value: &Value, field: &str, default: bool, invert: bool) -> bool {
    if field == "desktopType" {
        return value[field]
            .as_str()
            .map_or(default, |value| value != "NEVER");
    }
    value[field].as_bool().map_or(default, |on| on != invert)
}
pub fn switch_value(key: &str, user: Option<Id>) -> Option<bool> {
    let (store, field, default, invert) = setting(key)?;
    let value = storage::get(store).unwrap_or(Value::Null);
    let value = if store == "StreamerModeStore" {
        &value[user?.to_string()]
    } else {
        &value
    };
    Some(read_switch(value, field, default, invert))
}
pub fn set_switch(key: &str, on: bool, user: Option<Id>) -> bool {
    let Some((store, field, _, invert)) = setting(key) else {
        return false;
    };
    let mut value = storage::get(store)
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let target = if store == "StreamerModeStore" {
        let Some(user) = user else {
            return true;
        };
        let target = &mut value[user.to_string()];
        if !target.is_object() {
            *target = json!({});
        }
        target
    } else {
        &mut value
    };
    target[field] = if field == "desktopType" {
        json!(if on { "ALL" } else { "NEVER" })
    } else {
        json!(on != invert)
    };
    storage::set(store, value);
    true
}

pub(super) fn toast_xml(title: &str, body: &str) -> String {
    fn escape(value: &str) -> String {
        if value.contains('\0') {
            return String::new();
        }
        value
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
            .collect::<String>()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual><audio silent=\"true\"/></toast>",
        escape(title),
        escape(body)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sound_bursts_keep_one_leading_and_one_latest_trailing_sound() {
        let now = Instant::now();
        let mut throttle = SoundThrottle::default();
        assert!(matches!(
            throttle.request("message1", now),
            SoundAction::Play
        ));
        assert!(
            matches!(throttle.request("message1", now + Duration::from_millis(300)), SoundAction::Schedule(delay) if delay == Duration::from_millis(700))
        );
        assert!(matches!(
            throttle.request("message3", now + Duration::from_millis(900)),
            SoundAction::Pending
        ));
        assert!(matches!(
            throttle.request("message1", now + Duration::from_millis(1100)),
            SoundAction::Pending
        ));
        assert_eq!(
            throttle.finish(now + Duration::from_millis(1100)),
            Some("message1")
        );
        assert_eq!(throttle.finish(now + Duration::from_millis(1100)), None);
    }
    #[test]
    fn toast_text_cannot_insert_xml_or_enable_system_audio() {
        let xml = toast_xml("A & B", "</text><audio silent=\"false\"/>");
        assert!(xml.contains("A &amp; B"));
        assert!(xml.contains("&lt;/text&gt;"));
        assert_eq!(xml.matches("<audio").count(), 1);
        assert!(xml.contains("<audio silent=\"true\"/>"));
        assert!(!toast_xml("private\0title", "body").contains("private"));
    }
    #[test]
    fn local_switches_preserve_official_defaults_and_inversions() {
        assert!(read_switch(&Value::Null, "desktopType", true, false));
        assert!(!read_switch(
            &json!({"desktopType":"NEVER"}),
            "desktopType",
            true,
            false
        ));
        assert!(!read_switch(
            &json!({"disableUnreadBadge":true}),
            "disableUnreadBadge",
            true,
            true
        ));
        assert!(read_switch(
            &json!({"disableUnreadBadge":false}),
            "disableUnreadBadge",
            true,
            true
        ));
    }
}
