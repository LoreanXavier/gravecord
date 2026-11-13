//! The call bar's two chevron menus: the Share button's while we stream (module 600125: Stop Streaming, Change Stream,
//! Stream Quality, Share Stream Audio) and the camera button's (the devices and Video Settings). The Share menu is
//! measured in `design/dom/popouts/stream-chevron-menu-darker-cozy.json`: 181px wide, centred over the button, 8px
//! above it.
//!
//! Report Problem (module 74329) opens a feedback form this client has no counterpart for, so the row is left out.

use super::go_live::{self, Prefs, Preset};
use super::motion::{PopKind, PopSide, Popout as _};
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
                        .popout("voice-share-menu", PopKind::Fade, PopSide::Top)
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

    /// The streams of others this client watches: the owner and their name (`b` of module 848362).
    pub(super) fn watched_streams(&self) -> Vec<(u64, String)> {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let Some(call) = &self.call else {
            return Vec::new();
        };
        self.stores
            .voice_members(call.guild, call.channel)
            .into_iter()
            .filter(|state| Some(state.user_id) != me)
            .filter(|state| {
                self.video_stream_of(state.user_id)
                    .is_some_and(|info| info.role == dc_voice::streams::Role::Watch)
            })
            .map(|state| {
                let name = self
                    .stores
                    .users
                    .get(&state.user_id)
                    .map(|user| user.display_name().to_owned())
                    .unwrap_or_default();
                (state.user_id, name)
            })
            .collect()
    }

    /// `manage-streams` with `hideSelfOptions`: Stop Watching for each watched stream of someone else, none when
    /// the only one is the focused one.
    pub(super) fn watched_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let streams = self.watched_streams();
        let focused = match self.vv.focus.tile {
            Some((user, super::video_tiles::TileKind::Stream)) => Some(user),
            _ => None,
        };
        if streams.is_empty() || (streams.len() == 1 && Some(streams[0].0) == focused) {
            return Vec::new();
        }
        let rows: Vec<AnyElement> = streams
            .into_iter()
            .map(|(owner, name)| {
                self.vmenu_row_ex(
                    ("vmenu-stop-watching-of", owner as usize),
                    Some("voice_bar_screen_active"),
                    format!("Stop Watching \u{2014} {name}"),
                    None,
                    true,
                    false,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.vv.menu = None;
                        this.unwatch_stream_of(owner, cx);
                    },
                )
            })
            .collect();
        let height = 36. * rows.len() as f32 + 16.;
        let Some((left, top)) = self.menu_at("call-end-group", WIDTH, height) else {
            return Vec::new();
        };
        vec![
            deferred(
                div().absolute().left(px(left)).top(px(top)).child(
                    self.menu_panel(WIDTH, true)
                        .id("voice-watched-menu")
                        .popout("voice-watched-menu", PopKind::Fade, PopSide::Top)
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
        // `change-video-background` (module 855187): "Preview Camera" while the camera is off, "Change Video
        // Background" while it is on and the engine has the background filter.
        let camera_on = self.call.as_ref().is_some_and(|call| call.camera);
        if !devices.is_empty() && (!camera_on || self.video_background_shown()) {
            rows.push(Self::menu_separator(true));
            rows.push(self.vmenu_row(
                "vmenu-preview-camera",
                Some(if camera_on {
                    "vbg_blur"
                } else {
                    "voice_view_preview"
                }),
                if camera_on {
                    dc_intl::tr("Change Video Background")
                } else {
                    dc_intl::tr("Preview Camera")
                },
                None,
                true,
                cx,
                |this, _, cx| {
                    this.vv.menu = None;
                    this.open_camera_preview(cx);
                },
            ));
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

    /// A slider row of a tile menu: the bounds of the track are read back from `tip_anchors` under `id`, so a press or
    /// a drag anywhere on it gives the position as a fraction of the track.
    fn menu_slider(
        &self,
        id: &'static str,
        fraction: f32,
        cx: &mut Context<Self>,
        apply: impl Fn(&mut Root, f32, &mut Context<Root>) + Clone + 'static,
    ) -> AnyElement {
        use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, relative};
        let at = move |this: &Root, x: gpui::Pixels| -> Option<f32> {
            let bounds = this.tip_anchors.borrow().get(id).copied()?;
            let width = f32::from(bounds.size.width);
            (width > 0.).then(|| ((f32::from(x) - f32::from(bounds.left())) / width).clamp(0., 1.))
        };
        let (down, drag) = (apply.clone(), apply);
        div()
            .id(id)
            .relative()
            .mx(px(8.))
            .h(px(24.))
            .cursor_pointer()
            .child(self.anchor_probe(id))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    if let Some(fraction) = at(this, event.position.x) {
                        down(this, fraction, cx);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.dragging()
                    && let Some(fraction) = at(this, event.position.x)
                {
                    drag(this, fraction, cx);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(10.))
                    .w_full()
                    .h(px(4.))
                    .rounded(px(4.))
                    .bg(color("--background-mod-strong"))
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction))
                            .rounded(px(4.))
                            .bg(color("--background-brand")),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(fraction))
                    .ml(px(-8.))
                    .top(px(4.))
                    .size(px(16.))
                    .rounded_full()
                    .bg(gpui::rgb(0xffffff)),
            )
            .into_any_element()
    }

    /// `user-volume` in the stream context (module 972432): "Stream Volume" over a slider that runs to 200 percent.
    pub(super) fn stream_volume_row(&self, owner: u64, cx: &mut Context<Self>) -> AnyElement {
        use super::audio_menu::{slider_to_volume, volume_to_slider};
        use super::stream_volume as volume;
        let fraction =
            (volume_to_slider(volume::percent(owner)) / volume::MAX_PERCENT).clamp(0., 1.);
        div()
            .h(px(58.))
            .child(
                div()
                    .p(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Stream Volume"),
            )
            .child(self.menu_slider(
                "vmenu-stream-volume",
                fraction,
                cx,
                move |this, fraction, _| {
                    volume::set_percent(
                        owner,
                        slider_to_volume((fraction * volume::MAX_PERCENT).round()),
                    );
                    this.apply_stream_volume(owner);
                },
            ))
            .into_any_element()
    }

    /// `sidechain-compression` and, while it is on, `sidechain-parameters` (module 16001): Stream Attenuation, and its
    /// strength slider. The same values as the Voice & Video page's.
    pub(super) fn stream_attenuation_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.stream_attenuation_on();
        let fraction = self.stream_attenuation_fraction();
        let row = self.vmenu_row(
            "vmenu-stream-attenuation",
            None,
            "Stream Attenuation",
            Some(on),
            false,
            cx,
            |this, _, _| this.toggle_stream_attenuation(),
        );
        div()
            .flex()
            .flex_col()
            .child(row)
            .when(on, |this| {
                this.child(
                    div()
                        .h(px(58.))
                        .child(
                            div()
                                .p(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child("Stream Attenuation Strength"),
                        )
                        .child(self.menu_slider(
                            "vmenu-stream-attenuation-strength",
                            fraction,
                            cx,
                            |this, fraction, _| {
                                this.commit_stream_attenuation(fraction);
                                this.apply_stream_attenuation();
                            },
                        )),
                )
            })
            .into_any_element()
    }
}
