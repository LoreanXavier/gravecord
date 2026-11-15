use super::Root;
use crate::tray::{Action, State, Voice};
use gpui::Context;

impl Root {
    fn tray_voice(&self) -> Voice {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let state = self
            .call
            .as_ref()
            .and_then(|call| self.stores.voice_states.get(&call.guild))
            .and_then(|states| states.iter().find(|state| Some(state.user_id) == me));
        Voice {
            connected: self.call.as_ref().is_some_and(|call| call.connected),
            speaking: me.is_some_and(|me| self.speaking.contains(&me)),
            self_mute: self.self_mute || self.self_deaf,
            self_deaf: self.self_deaf,
            server_mute: state.is_some_and(|state| state.mute),
            server_deaf: state.is_some_and(|state| state.deaf),
            suppressed: state.is_some_and(|state| state.suppress),
        }
    }
    pub(super) fn refresh_tray(&mut self, unread: bool) {
        self.tray_runtime.refresh(State {
            voice: self.tray_voice(),
            unread,
            icons: crate::branding::selected(),
            close_to_tray: self.pref_on("os_minimize_to_tray", true),
        });
    }
    pub(super) fn tray_action(&mut self, action: Action, cx: &mut Context<Self>) {
        let state = State {
            voice: self.tray_voice(),
            ..State::default()
        };
        // Recheck after queuing: a disconnect or server restriction can arrive while the menu is open.
        if !state.allows(action) {
            return;
        }
        match action {
            Action::Mute => self.toggle_self_mute(cx),
            Action::Deafen => self.toggle_self_deaf(cx),
            Action::VoiceSettings => self.open_settings_section("Voice & Video", cx),
            Action::Acknowledgements => cx.open_url("https://discord.com/acknowledgements"),
            Action::Quit => cx.quit(),
            Action::CheckUpdates => {
                crate::updater::request_check();
            }
        }
    }
    #[cfg(feature = "capture")]
    pub(crate) fn script_tray_check_updates(&mut self, cx: &mut Context<Self>) -> String {
        if !crate::storage::is_capture_run() {
            return "tray update check requires capture".into();
        }
        self.tray_action(Action::CheckUpdates, cx);
        format!(
            "tray update check: capture no-op; state {:?}",
            crate::updater::state()
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_tray(&mut self) -> String {
        if !crate::storage::is_capture_run() {
            return "tray preview requires capture".into();
        }
        let mut output = Vec::new();
        for icons in [
            crate::branding::IconSet::Gravecord,
            crate::branding::IconSet::Original,
        ] {
            let mut state = State {
                icons,
                ..State::default()
            };
            for step in 0..6 {
                match step {
                    1 => state.unread = true,
                    2 => state.voice.connected = true,
                    3 => state.voice.speaking = true,
                    4 => state.voice.self_mute = true,
                    5 => state.voice.self_deaf = true,
                    _ => {}
                }
                self.tray_runtime.refresh(state);
                output.push(format!("{} {:?}", icons.name(), state.icon()));
            }
        }
        self.refresh_notification_badge();
        format!("tray capture no-op: {}", output.join(", "))
    }

    /// `badge render <count> <png>`: the taskbar overlay that `count` would set, written as PNG instead of being
    /// handed to the shell. `<png>` holds the icon's own 256 px pixels as the taskbar receives them; `-16`, `-20`,
    /// `-24` and `-32` siblings are smooth reductions to the overlay sizes at 100/125/150/200 % (a preview: the shell's
    /// own scaler is not known), and `-frame16` is the ICO's 16 px frame that was used before, for comparison.
    #[cfg(feature = "capture")]
    pub(crate) fn script_badge_render(&self, count: &str, png: &str) -> String {
        if !crate::storage::is_capture_run() {
            return "badge render requires capture".into();
        }
        let Ok(count) = count.parse::<i64>() else {
            return "usage: badge render <count> <absolute png>".into();
        };
        render_badge(count, std::path::Path::new(png))
    }

    /// `tray render <state> <png> [size]`: the tray picture for a state (default, unread, connected, speaking, muted,
    /// deafened) in the selected icon set at the size the tray asks for (16 at 100 %), as the icon holds it.
    #[cfg(feature = "capture")]
    pub(crate) fn script_tray_render(&self, words: &[&str]) -> String {
        use crate::tray::Icon;
        if !crate::storage::is_capture_run() {
            return "tray render requires capture".into();
        }
        let (Some(state), Some(png)) = (words.first(), words.get(1)) else {
            return "usage: tray render <state> <absolute png> [size]".into();
        };
        let icon = match *state {
            "default" => Icon::Default,
            "unread" => Icon::Unread,
            "connected" => Icon::Connected,
            "speaking" => Icon::Speaking,
            "muted" => Icon::Muted,
            "deafened" => Icon::Deafened,
            _ => return format!("unknown tray state {state}"),
        };
        let size = words
            .get(2)
            .and_then(|size| size.parse::<u32>().ok())
            .unwrap_or(16);
        let set = crate::branding::selected();
        let Some((size, pixels)) = crate::branding::tray_pixels(set, icon, size) else {
            return format!("tray picture {} unavailable", icon.file());
        };
        match save_rgba(size, pixels, std::path::Path::new(png)) {
            Ok(()) => format!(
                "tray render: {} {}{} at {size} px -> {png}",
                set.name(),
                icon.file(),
                suffix_of(size)
            ),
            Err(error) => format!("tray render failed: {error}"),
        }
    }
}

#[cfg(feature = "capture")]
fn suffix_of(size: u32) -> &'static str {
    match size {
        20 => "@1.25x",
        21 => "@1.33x",
        22 => "@1.4x",
        24 => "@1.5x",
        29 => "@1.8x",
        32 => "@2x",
        40 => "@2.5x",
        48 => "@3x",
        64 => "@4x",
        80 => "@5x",
        _ => "",
    }
}

/// Writes icon pixels as the icon stores them (its colour is read as straight alpha by the shell).
#[cfg(feature = "capture")]
fn save_rgba(size: u32, pixels: Vec<u8>, path: &std::path::Path) -> Result<(), String> {
    image::RgbaImage::from_raw(size, size, pixels)
        .ok_or_else(|| "pixel buffer size".to_owned())?
        .save(path)
        .map_err(|error| error.to_string())
}

#[cfg(all(feature = "capture", windows))]
fn render_badge(count: i64, path: &std::path::Path) -> String {
    use crate::notifications::taskbar::{OVERLAY_SIZE, badge_index, overlay_pixels};
    let Some(index) = badge_index(count) else {
        return format!("badge render: count {count} clears the overlay; nothing written");
    };
    let pixels = match overlay_pixels(count) {
        Some(Ok(pixels)) => pixels,
        Some(Err(error)) => return format!("badge render failed: {error}"),
        None => return "badge render: no overlay".into(),
    };
    let size = OVERLAY_SIZE as u32;
    let Some(image) = image::RgbaImage::from_raw(size, size, pixels) else {
        return "badge render: pixel buffer size".into();
    };
    let stem = path.with_extension("");
    let sibling = |tag: &str| std::path::PathBuf::from(format!("{}-{tag}.png", stem.display()));
    let mut written = vec![path.display().to_string()];
    if let Err(error) = image.save(path) {
        return format!("badge render failed: {error}");
    }
    for target in [16, 20, 24, 32] {
        let small = image::imageops::resize(
            &image,
            target,
            target,
            image::imageops::FilterType::Triangle,
        );
        let file = sibling(&target.to_string());
        if small.save(&file).is_ok() {
            written.push(file.display().to_string());
        }
    }
    if let Some(frame) = crate::branding::badge_pixels(crate::branding::selected(), index) {
        let file = sibling("frame16");
        if save_rgba(16, frame, &file).is_ok() {
            written.push(file.display().to_string());
        }
    }
    format!(
        "badge render: count {count} -> badge-{}.ico at {size} px: {}",
        index + 1,
        written.join(", ")
    )
}

#[cfg(all(feature = "capture", not(windows)))]
fn render_badge(_count: i64, _path: &std::path::Path) -> String {
    "badge render: the taskbar overlay exists on Windows only".into()
}
