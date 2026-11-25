//! Copy combo easter egg (`ClickToCopy`, module 206845; `Shaker`, module 568602; green tooltip
//! `tooltipGreen_3_v8qp`, module 664343).
//!
//! Used by the Settings sidebar build version footer (module 276530 `eJ`, `"Click to copy version"`,
//! `9Al4Qd`) and the Account Popout username copy trigger (module 993382 `nm`, `"Copy Username"`,
//! `y5MwJy`).
//!
//! State machine (`206845.js`):
//! - `combo` starts at `0`, `copied` at `false`, `force_open` at `false`.
//! - Click writes `copy_value` to clipboard; when `!force_open` increments `combo`; sets
//!   `force_open = true` and `copied = true`; arms `hide_timer` (1000 ms -> clears `force_open`) and
//!   `combo_reset_timer` (2000 ms -> resets `combo = 0`).
//! - Re-entering the trigger while `copied` cancels `hide_timer` and clears `force_open` so
//!   consecutive clicks escalate across all 11 tiers (`Copied!` -> `BEYOND GODLIKE!!!!`).
//! - Tiers 10 (`GODLIKE!!!`, `combo == 10`) and 11 (`BEYOND GODLIKE!!!!`, `combo >= 11`) enable
//!   the 568602 `Shaker` (`±5px` offset toggled every other frame unless reduced motion is active).

use super::tooltip::{Active, Body, Side, Tip};
use super::{Root, icon};
use gpui::{ClipboardItem, Context, IntoElement, SharedString, Window, div, prelude::*, px};
use std::time::Duration;

/// `hideTimer`: 1 second (`1 * SECOND`, module 206845).
pub const HIDE_MS: u64 = 1_000;
/// `comboResetTimer`: 2 seconds (`2 * SECOND`, module 206845).
pub const COMBO_RESET_MS: u64 = 2_000;
/// Default hover delay (`S = HALF_SECOND`, module 206845).
pub const DEFAULT_DELAY_MS: u64 = 500;
/// `Shaker` intensity (`5px`, module 568602).
pub const SHAKE_INTENSITY: f32 = 5.0;

/// The 11 combo tiers (`f` array of module 206845, indexed by `clamp(combo - 1, 0, 10)`).
pub const COMBO_KEYS: [&str; 11] = [
    "MSaeTe", // Copied!
    "UmrCw7", // Double Copy!
    "gKE0Jq", // Triple Copy!
    "4DSKbi", // Dominating!!
    "+8ENdT", // Rampage!!
    "GlWHv8", // Mega Copy!!
    "hIzxU9", // Unstoppable!!
    "26uMPL", // Wicked Sick!!
    "uFs7R2", // Monster Copy!!!
    "bLXdcY", // GODLIKE!!!
    "gPg9fS", // BEYOND GODLIKE!!!!
];

/// Localized combo label for `combo >= 1` (`206845.js`).
pub fn combo_label(combo: u32) -> &'static str {
    let idx = combo.saturating_sub(1).min((COMBO_KEYS.len() - 1) as u32) as usize;
    dc_intl::t(COMBO_KEYS[idx])
}

/// Whether the tooltip shakes (`b >= f.length - 1`, i.e., `combo >= 10`: GODLIKE!!! / BEYOND GODLIKE!!!!).
pub fn is_shaking(combo: u32) -> bool {
    combo as usize >= COMBO_KEYS.len() - 1
}

/// Deterministic every-other-frame shake offset (`568602.b`, `intensity = 5`).
pub fn shake_offset(tick: u64, reduced_motion: bool) -> (f32, f32) {
    if reduced_motion || tick % 2 == 1 {
        return (0.0, 0.0);
    }
    // Lightweight xorshift hash mapped to [-SHAKE_INTENSITY, +SHAKE_INTENSITY].
    let mut x = tick.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    let dx = ((x & 0xFFFF) as f32 / 65535.0) * 2.0 - 1.0;
    let dy = (((x >> 16) & 0xFFFF) as f32 / 65535.0) * 2.0 - 1.0;
    (dx * SHAKE_INTENSITY, dy * SHAKE_INTENSITY)
}

#[derive(Clone, Debug, Default)]
pub(super) struct CopyComboState {
    pub active_key: Option<String>,
    pub combo: u32,
    pub copied: bool,
    pub force_open: bool,
    pub generation: u64,
    pub shake_tick: u64,
}

impl CopyComboState {
    /// Applies a click on `key` following `206845.js`: `L || k(b + 1), R(true), x(true)`.
    pub fn record_click(&mut self, key: &str) -> u64 {
        if self.active_key.as_deref() != Some(key) {
            self.active_key = Some(key.to_owned());
            self.combo = 0;
            self.copied = false;
            self.force_open = false;
        }
        if !self.force_open {
            self.combo = self.combo.saturating_add(1);
        }
        self.force_open = true;
        self.copied = true;
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    /// Re-hover while `copied` clears `force_open` (`w ? (T.stop(), R(!1)) : ...`, `206845.js`).
    pub fn on_hover_enter(&mut self, key: &str) {
        if self.active_key.as_deref() == Some(key) && self.copied {
            self.force_open = false;
        }
    }

    /// Hover leave when not force-opened resets `copied` back to idle tooltip text.
    pub fn on_hover_leave(&mut self, key: &str) {
        if self.active_key.as_deref() == Some(key) && !self.force_open {
            self.copied = false;
        }
    }

    pub fn is_copied_for(&self, key: &str) -> bool {
        self.active_key.as_deref() == Some(key) && self.copied
    }
}

/// Full Discord-style build version string copied by Settings footer (`276530.js` `eJ`).
pub fn build_version_copy_text() -> String {
    format!(
        "{} {} ({}) Host {} Build Override: N/A Windows",
        dc_proto::pin::RELEASE_CHANNEL,
        dc_proto::pin::CLIENT_BUILD_NUMBER,
        &dc_proto::pin::VERSION_HASH[..7],
        host_version(),
    )
}

/// The footer's second line, where the official client names its host version: gravecord's version, the commit it
/// was built from (`crate::provenance`) and the architecture, `0.1.0 (1a2b3c4) (x64)`. A capture build leaves the
/// commit out, so the settings screens it is compared on stay the same from one commit to the next.
pub fn host_version() -> String {
    host_version_of(
        env!("CARGO_PKG_VERSION"),
        (!cfg!(feature = "capture")).then(crate::provenance::short),
    )
}

fn host_version_of(version: &str, commit: Option<String>) -> String {
    match commit {
        Some(commit) => format!("{version} ({commit}) (x64)"),
        None => format!("{version} (x64)"),
    }
}

impl Root {
    /// Triggers the `206845` ClickToCopy combo on `key`, writing `copy_value` to the clipboard,
    /// showing the green combo tooltip (`664343`) anchored to `key`, and arming the 1s hide / 2s reset timers.
    pub(super) fn trigger_copy_combo(
        &mut self,
        key: &'static str,
        copy_value: String,
        side: Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(copy_value));
        let generation = self.copy_combo.record_click(key);
        let combo = self.copy_combo.combo;
        let label = combo_label(combo);
        let shaking = is_shaking(combo);
        let offset = if shaking {
            self.copy_combo.shake_tick = self.copy_combo.shake_tick.wrapping_add(1);
            shake_offset(self.copy_combo.shake_tick, super::motion::reduced())
        } else {
            (0.0, 0.0)
        };
        let win_id = window.window_handle().window_id();
        if let Some(anchor) = self.tip_anchors.borrow().get(key).copied() {
            let mut tip = Tip::body(Body::Green(SharedString::from(label)), side);
            if shaking && !super::motion::reduced() {
                tip = tip.shaken(offset);
            }
            self.tip = Some(Active::new(SharedString::from(key), tip, anchor).in_window(win_id));
            self.tip_pending = None;
        }
        cx.notify();

        // 1000ms hide timer (`T.start(1000, () => R(!1))`)
        let key_owned = key.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(HIDE_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.copy_combo.generation == generation
                    && this.copy_combo.active_key.as_deref() == Some(key_owned.as_str())
                {
                    this.copy_combo.force_open = false;
                    this.copy_combo.copied = false;
                    if this.tip.as_ref().is_some_and(|t| t.key() == key_owned) {
                        this.tip = None;
                    }
                    cx.notify();
                }
            });
        })
        .detach();

        // 2000ms combo reset timer (`m.start(2000, () => k(0))`)
        let key_reset = key.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(COMBO_RESET_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.copy_combo.generation == generation
                    && this.copy_combo.active_key.as_deref() == Some(key_reset.as_str())
                {
                    this.copy_combo.combo = 0;
                    this.copy_combo.copied = false;
                    this.copy_combo.force_open = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Clickable `Copy Username` icon button for the Account Popout (module 993382 `nm` + 206845).
    pub(super) fn copy_username_button(
        &self,
        username: String,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        const KEY: &str = "copy-combo-username";
        let copied = self.copy_combo.is_copied_for(KEY);
        let tip = if copied {
            let combo = self.copy_combo.combo;
            let mut t = Tip::body(
                Body::Green(SharedString::from(combo_label(combo))),
                Side::Top,
            );
            if is_shaking(combo) && !super::motion::reduced() {
                t = t.shaken(shake_offset(
                    self.copy_combo.shake_tick,
                    super::motion::reduced(),
                ));
            }
            t
        } else {
            // `y5MwJy`: "Copy Username", delay 0ms (`993382.js`).
            Tip::text(dc_intl::t("y5MwJy"), Side::Top)
        };
        // `copyButton_hpQ_E_`: a 24px button around the 16px icon.
        let btn = div()
            .id(KEY)
            .size(px(24.))
            .rounded_full()
            // `.copyButton_hpQ_E_{opacity:0}`, `.visible` while the popout is hovered or focused.
            .opacity(0.)
            .group_hover("status-popout", |style| style.opacity(1.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if *over {
                    this.copy_combo.on_hover_enter(KEY);
                } else {
                    this.copy_combo.on_hover_leave(KEY);
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.trigger_copy_combo(KEY, username.clone(), Side::Top, window, cx);
            }))
            .child(icon("menu_copy_text", 16., "--interactive-text-default"));
        self.with_tip(KEY, tip, btn, cx).into_any_element()
    }

    /// Capture-only harness helper: `copy-combo <tier 1..11 | reset>` exercises the green tooltip
    /// and shaker directly on screen.
    #[cfg(feature = "capture")]
    pub(crate) fn script_copy_combo(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        const KEY: &str = "copy-combo-demo";
        let trimmed = arg.trim();
        if trimmed == "reset" || trimmed == "close" {
            self.copy_combo = CopyComboState::default();
            if self.tip.as_ref().is_some_and(|t| t.key() == KEY) {
                self.tip = None;
            }
            cx.notify();
            return "copy-combo: reset".into();
        }
        let tier = trimmed.parse::<u32>().unwrap_or(2).clamp(1, 11);
        self.copy_combo.active_key = Some(KEY.to_owned());
        self.copy_combo.combo = tier;
        self.copy_combo.copied = true;
        self.copy_combo.force_open = true;
        self.copy_combo.shake_tick = self.copy_combo.shake_tick.wrapping_add(1);
        let label = combo_label(tier);
        let shaking = is_shaking(tier);
        let mut tip = Tip::body(Body::Green(SharedString::from(label)), Side::Top);
        if shaking {
            tip = tip.shaken(shake_offset(self.copy_combo.shake_tick, false));
        }
        let anchor = gpui::Bounds::new(
            gpui::point(px(160.), px(640.)),
            gpui::size(px(120.), px(20.)),
        );
        self.tip = Some(Active::new(SharedString::from(KEY), tip, anchor));
        cx.notify();
        format!("copy-combo: tier={tier} label={label} shaking={shaking}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_names_the_commit() {
        assert_eq!(
            host_version_of("0.1.0", Some("1a2b3c4-dirty".to_owned())),
            "0.1.0 (1a2b3c4-dirty) (x64)"
        );
        assert_eq!(host_version_of("0.1.0", None), "0.1.0 (x64)");
        if !cfg!(feature = "capture") {
            assert!(host_version().contains(&crate::provenance::short()));
            assert!(build_version_copy_text().contains(&host_version()));
        }
    }

    #[test]
    fn all_eleven_combo_tiers_resolve_and_clamp() {
        let expected = [
            "Copied!",
            "Double Copy!",
            "Triple Copy!",
            "Dominating!!",
            "Rampage!!",
            "Mega Copy!!",
            "Unstoppable!!",
            "Wicked Sick!!",
            "Monster Copy!!!",
            "GODLIKE!!!",
            "BEYOND GODLIKE!!!!",
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(combo_label((i + 1) as u32), *want);
        }
        assert_eq!(combo_label(25), "BEYOND GODLIKE!!!!");
        assert!(!is_shaking(9));
        assert!(is_shaking(10));
        assert!(is_shaking(11));
    }

    #[test]
    fn combo_escalates_on_rehover_and_respects_force_open() {
        let mut st = CopyComboState::default();
        st.record_click("k");
        assert_eq!(st.combo, 1);
        assert!(st.force_open);
        // Rapid click while still force-open keeps combo at 1 (`L || k(b+1)`).
        st.record_click("k");
        assert_eq!(st.combo, 1);
        // Re-hover clears force_open -> next click advances combo.
        st.on_hover_enter("k");
        assert!(!st.force_open);
        st.record_click("k");
        assert_eq!(st.combo, 2);
        assert_eq!(combo_label(st.combo), "Double Copy!");
    }

    #[test]
    fn shaker_respects_reduced_motion_and_bounds() {
        assert_eq!(shake_offset(2, true), (0.0, 0.0));
        assert_eq!(shake_offset(3, false), (0.0, 0.0));
        let (dx, dy) = shake_offset(4, false);
        assert!(dx.abs() <= SHAKE_INTENSITY && dy.abs() <= SHAKE_INTENSITY);
    }
}
