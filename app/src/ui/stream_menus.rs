//! The call bar's two chevron menus: the Share button's while we stream (module 600125: Stop Streaming, Change Stream,
//! Stream Quality, Share Stream Audio) and the camera button's (the devices and Video Settings). The Share menu is
//! measured in `design/dom/popouts/stream-chevron-menu-darker-cozy.json`: 181px wide, centred over the button, 8px
//! above it.
//!
//! Report Problem (module 74329) opens a feedback form this client has no counterpart for, so the row is left out.

use super::go_live::{self, Prefs, Preset};
use super::{Root, color};
use dc_media::screen::{self, Resolution};
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px};

/// What a radio row does when it is clicked.
type Run = Box<dyn Fn(&mut Root, &mut Context<Root>)>;

const WIDTH: f32 = 181.;
const SUBMENU_WIDTH: f32 = 200.;
/// The window's left edge to the call view's: the rail and the sidebar.
const VIEW_LEFT: f32 = 375.;
const VIEW_TOP: f32 = 32.;

/// `ResolutionLabel`: the radio rows of Stream Quality.
fn resolution_label(resolution: Resolution) -> String {
    match resolution {
        Resolution::Source => "Source".to_owned(),
        other => format!("{}p", other.height()),
    }
}

impl Root {
    fn menu_at(&self, anchor_id: &str, width: f32, height: f32) -> Option<(f32, f32)> {
        let anchor = self.tip_anchors.borrow().get(anchor_id).copied()?;
        let centre = f32::from(anchor.left()) + f32::from(anchor.size.width) / 2.;
        Some((
            centre - width / 2. - VIEW_LEFT,
            f32::from(anchor.top()) - 8. - height - VIEW_TOP,
        ))
    }

    /// The Share button's chevron menu.
    pub(super) fn share_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let prefs = Prefs::load();
        let rows = vec![
            self.vmenu_row_ex(
                "vmenu-stop-stream",
                Some("menu_stop_stream"),
                "Stop Streaming",
                None,
                true,
                true,
                false,
                cx,
                |this, _, cx| {
                    this.vv.menu = None;
                    this.stream_stop(cx);
                },
            ),
            self.vmenu_row_ex(
                "vmenu-change-stream",
                Some("menu_change_stream"),
                "Change Stream",
                None,
                true,
                false,
                false,
                cx,
                |this, _, cx| {
                    this.vv.menu = None;
                    this.open_go_live(cx);
                },
            ),
            self.vmenu_row_ex(
                "vmenu-stream-quality",
                None,
                "Stream Quality",
                None,
                true,
                false,
                true,
                cx,
                |this, _, _| this.vv.quality_open = !this.vv.quality_open,
            ),
            self.vmenu_row(
                "vmenu-stream-audio",
                None,
                "Share Stream Audio",
                Some(prefs.sound),
                true,
                cx,
                |this, _, cx| {
                    let mut prefs = Prefs::load();
                    prefs.sound = !prefs.sound;
                    this.restart_stream_with(prefs, cx);
                },
            ),
        ];
        let height = 36. * rows.len() as f32 + 16.;
        let Some((left, top)) = self.menu_at("call-screen-group", WIDTH, height) else {
            return Vec::new();
        };
        let mut out = vec![
            deferred(
                div().absolute().left(px(left)).top(px(top)).child(
                    self.menu_panel(WIDTH, true)
                        .id("voice-share-menu")
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.vv.menu = None;
                            this.vv.quality_open = false;
                            cx.notify();
                        }))
                        .children(rows),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        ];
        if self.vv.quality_open {
            // The submenu's bottom meets the menu's.
            let sub_height = 28. * 2. + 32. * 9. + 18.;
            out.push(self.quality_submenu(left + WIDTH + 8., top + height - sub_height, cx));
        }
        out
    }

    /// Stream Quality: the resolutions and the frame rates the account may use, as radios. A choice becomes the Custom
    /// preset and restarts the stream, because the capture's size and rate are fixed when it starts.
    fn quality_submenu(&self, left: f32, top: f32, cx: &mut Context<Self>) -> AnyElement {
        let premium = self.stores.account.premium_type;
        let prefs = Prefs::load();
        // What runs now, not what the stored preset would pick.
        let (resolution, fps) = self
            .own_started()
            .map(|started| started.quality)
            .unwrap_or_else(|| prefs.effective(premium));
        let header = |text: &'static str| {
            div()
                .h(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-muted"))
                .child(text)
        };
        let radio = |id: String,
                     label: String,
                     on: bool,
                     allowed: bool,
                     run: Run,
                     cx: &mut Context<Root>| {
            div()
                .id(gpui::SharedString::from(id))
                .h(px(32.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_between()
                .when(!allowed, |this| this.opacity(0.4))
                .when(allowed, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            run(this, cx);
                            cx.notify();
                        }))
                })
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .child(
                    div()
                        .size(px(16.))
                        .rounded_full()
                        .border_2()
                        .border_color(if on {
                            color("--brand-500")
                        } else {
                            color("--interactive-icon-default")
                        })
                        .when(on, |this| this.bg(color("--brand-500"))),
                )
        };
        let mut rows: Vec<AnyElement> = vec![header("Resolution").into_any_element()];
        for candidate in [
            Resolution::P480,
            Resolution::P720,
            Resolution::P1080,
            Resolution::P1440,
            Resolution::Source,
        ] {
            let fps_for = if screen::required_quality(candidate, fps).is_some() {
                fps
            } else {
                screen::offered_frame_rates(candidate)
                    .into_iter()
                    .next()
                    .unwrap_or(30)
            };
            let allowed = go_live::can_stream(premium, candidate, fps_for, Preset::Custom);
            rows.push(
                radio(
                    format!("vmenu-res-{}", candidate.height()),
                    resolution_label(candidate),
                    candidate == resolution,
                    allowed,
                    Box::new(move |this, cx| {
                        let mut prefs = Prefs::load();
                        prefs.preset = Preset::Custom;
                        prefs.resolution = candidate;
                        prefs.fps = fps_for;
                        this.restart_stream_with(prefs, cx);
                    }),
                    cx,
                )
                .into_any_element(),
            );
        }
        rows.push(header("Frame Rate").into_any_element());
        for rate in screen::FRAME_RATES {
            let allowed = go_live::can_stream(premium, resolution, rate, Preset::Custom);
            rows.push(
                radio(
                    format!("vmenu-fps-{rate}"),
                    format!("{rate} FPS"),
                    rate == fps,
                    allowed,
                    Box::new(move |this, cx| {
                        let mut prefs = Prefs::load();
                        prefs.preset = Preset::Custom;
                        prefs.resolution = resolution;
                        prefs.fps = rate;
                        this.restart_stream_with(prefs, cx);
                    }),
                    cx,
                )
                .into_any_element(),
            );
        }
        deferred(
            div().absolute().left(px(left)).top(px(top)).child(
                self.menu_panel(SUBMENU_WIDTH, true)
                    .id("voice-quality-menu")
                    .children(rows),
            ),
        )
        .with_priority(2)
        .into_any_element()
    }

    /// The camera button's chevron: the devices (the chosen one checked) and Video Settings.
    pub(super) fn camera_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let devices = dc_voice::camera_devices();
        let chosen = crate::storage::get("audio.video_device")
            .and_then(|value| value.as_str().map(str::to_owned))
            .filter(|id| !id.is_empty())
            .or_else(|| devices.first().map(|device| device.id.clone()));
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, device) in devices.iter().enumerate() {
            let id = device.id.clone();
            let on = chosen.as_deref() == Some(device.id.as_str());
            rows.push(
                div()
                    .id(gpui::SharedString::from(format!("vmenu-camera-{index}")))
                    .min_h(px(36.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.vv.menu = None;
                        this.choose_camera(id.clone(), cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(device.name.clone()),
                    )
                    .child(
                        div()
                            .size(px(16.))
                            .flex_none()
                            .rounded_full()
                            .border_2()
                            .border_color(if on {
                                color("--brand-500")
                            } else {
                                color("--interactive-icon-default")
                            })
                            .when(on, |this| this.bg(color("--brand-500"))),
                    )
                    .into_any_element(),
            );
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .h(px(36.))
                    .p(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child("No Camera Detected")
                    .into_any_element(),
            );
        }
        rows.push(Self::menu_separator(true));
        rows.push(self.vmenu_row(
            "vmenu-video-settings",
            Some("voice_view_gear"),
            "Video Settings",
            None,
            true,
            cx,
            |this, _, cx| {
                this.vv.menu = None;
                this.open_settings_section("Voice & Video", cx);
            },
        ));
        let height = 36. * (rows.len() as f32 - 1.) + 17. + 16.;
        let Some((left, top)) = self.menu_at("call-camera-group", 240., height) else {
            return Vec::new();
        };
        vec![
            deferred(
                div().absolute().left(px(left)).top(px(top)).child(
                    self.menu_panel(240., true)
                        .id("voice-camera-menu")
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.vv.menu = None;
                            cx.notify();
                        }))
                        .children(rows),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        ]
    }
}
