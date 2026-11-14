//! The call bar's two chevron menus: the Share button's while we stream (`manage-streams`, module 848362, with the
//! stream's own items of module 600125 in their `minimal` order: Stop Streaming, Change Stream, Stream Quality, Share
//! Stream Audio) and the camera button's (the devices and Video Settings). The Share menu is measured in
//! `design/dom/popouts/stream-chevron-menu-darker-cozy.json`: 182.34px wide, centred over the button, 8px above it.
//!
//! Report Problem (module 74329) opens a feedback form this client has no counterpart for, so its group is left out.
//!
//! Every row is the menu component of `go_live` (`MenuItem`, `MenuCheckboxItem`, `MenuRadioItem`, the group labels and
//! separators) in the call view's `midnight` palette. A press inside the menu or its submenu stays there: the panels
//! occlude what is under them and stop the press, and each closes the menu only for a press outside both.

use super::go_live::{self, Prefs, Tint};
use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, color};
use dc_media::screen::Resolution;
use dc_tokens::Theme;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseDownEvent, Pixels, Point, SharedString,
    deferred, div, prelude::*, px, rgba,
};

/// `menu_M3qk4j` of the share menu: the rows (164.34) and 8px of padding and the border either side.
const WIDTH: f32 = 182.34;
/// `submenu_M3qk4j`: at least 188 wide.
const SUBMENU_WIDTH: f32 = 188.;
/// Anchors of the two panels and of the Stream Quality row, read back to keep presses inside them.
const SHARE_PANEL: &str = "voice-share-menu";
const SHARE_ORIGIN: &str = "voice-share-origin";
const QUALITY_PANEL: &str = "voice-quality-menu";
const QUALITY_ROW: &str = "vmenu-stream-quality";

/// `midnight`: the call container carries `theme-midnight`, whatever the app's own theme is.
fn mid(name: &str) -> gpui::Rgba {
    rgba(Theme::Midnight.color(name))
}

/// The Frame Rate group of Stream Quality (`ce` of module 753070): 15, 30 and 60.
pub(super) const STREAM_FRAME_RATES: [u32; 3] = [15, 30, 60];
/// The Resolution group (`Jk`): 480p to Source.
pub(super) const STREAM_RESOLUTIONS: [Resolution; 5] = [
    Resolution::P480,
    Resolution::P720,
    Resolution::P1080,
    Resolution::P1440,
    Resolution::Source,
];

/// `bW+JCW`: "15 FPS".
pub(super) fn frame_rate_label(fps: u32) -> String {
    format!("{fps} FPS")
}

/// What a choice of Stream Quality does: apply the pair as Custom, or open the upsell when the account may not use it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QualityChoice {
    Apply(Resolution, u32),
    Upsell,
}

/// `P` of module 600125 for a Frame Rate row.
pub(super) fn choose_frame_rate(
    prefs: &Prefs,
    account: go_live::Account,
    fps: u32,
) -> QualityChoice {
    match go_live::frame_rate_choice(prefs, account, fps) {
        (true, resolution) => QualityChoice::Apply(resolution, fps),
        (false, _) => QualityChoice::Upsell,
    }
}

/// `P` of module 600125 for a Resolution row.
pub(super) fn choose_resolution(
    prefs: &Prefs,
    account: go_live::Account,
    resolution: Resolution,
) -> QualityChoice {
    match go_live::resolution_choice(prefs, account, resolution) {
        (true, fps) => QualityChoice::Apply(resolution, fps),
        (false, _) => QualityChoice::Upsell,
    }
}

impl Root {
    fn menu_at(&self, anchor_id: &str, width: f32, height: f32) -> Option<(f32, f32)> {
        let anchor = self.tip_anchors.borrow().get(anchor_id).copied()?;
        let centre = f32::from(anchor.left()) + f32::from(anchor.size.width) / 2.;
        let (ox, oy) = self.call_origin();
        let top = super::audio_menu::popout_top(
            f32::from(anchor.top()),
            f32::from(anchor.bottom()),
            height,
            8.,
            self.viewport.1,
        );
        Some((centre - width / 2. - ox, top - oy))
    }

    /// Whether a press is inside one of the share menu's panels.
    fn inside_share_menus(&self, position: Point<Pixels>) -> bool {
        let anchors = self.tip_anchors.borrow();
        let inside = |key: &str| {
            anchors
                .get(key)
                .is_some_and(|bounds| bounds.contains(&position))
        };
        inside(SHARE_PANEL) || (self.vv.quality_open && inside(QUALITY_PANEL))
    }

    fn close_share_menu(&mut self, cx: &mut Context<Self>) {
        self.vv.menu = None;
        self.vv.quality_open = false;
        cx.notify();
    }

    /// Applies a Stream Quality choice: the store takes Custom with the pair (`setGoLiveSource` with
    /// `PRESET_CUSTOM`), and the running stream follows it. A choice the account may not use opens the upsell.
    pub(super) fn apply_stream_quality(&mut self, choice: QualityChoice, cx: &mut Context<Self>) {
        match choice {
            QualityChoice::Upsell => {
                self.close_share_menu(cx);
                self.open_stream_upsell(false, cx);
            }
            QualityChoice::Apply(resolution, fps) => {
                let prefs = Prefs::load();
                let next = Prefs {
                    preset: go_live::Preset::Custom,
                    resolution,
                    fps,
                    ..prefs
                };
                if next != prefs {
                    self.restart_stream_with(next, cx);
                }
                cx.notify();
            }
        }
    }

    /// Share Stream Audio (`$` of module 600125): the store's `soundshareEnabled` flips and the stream follows it.
    pub(super) fn toggle_stream_audio(&mut self, cx: &mut Context<Self>) {
        let prefs = Prefs::load();
        self.restart_stream_with(
            Prefs {
                sound: !prefs.sound,
                ..prefs
            },
            cx,
        );
        cx.notify();
    }

    /// The Share button's chevron menu.
    pub(super) fn share_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let prefs = Prefs::load();
        let tint: Tint = mid;
        // Hovering a row without a submenu closes the open one, as the official menu's focus moves.
        fn leave_submenu(
            cx: &mut Context<Root>,
        ) -> impl Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static {
            cx.listener(|this, inside: &bool, _, cx| {
                if *inside && this.vv.quality_open {
                    this.vv.quality_open = false;
                    cx.notify();
                }
            })
        }
        let rows: Vec<AnyElement> = vec![
            go_live::icon_row(
                "vmenu-stop-stream",
                "menu_stop_stream",
                dc_intl::tr("Stop Streaming"),
                true,
                tint,
                cx,
                |this, cx| {
                    this.close_share_menu(cx);
                    this.stream_stop(cx);
                },
            )
            .on_hover(leave_submenu(cx))
            .into_any_element(),
            go_live::icon_row(
                "vmenu-change-stream",
                "menu_change_stream",
                dc_intl::tr("Change Stream"),
                false,
                tint,
                cx,
                |this, cx| {
                    this.close_share_menu(cx);
                    this.open_go_live(cx);
                },
            )
            .on_hover(leave_submenu(cx))
            .into_any_element(),
            go_live::caret_row(
                QUALITY_ROW,
                dc_intl::tr("Stream Quality"),
                self.vv.quality_open,
                tint,
                cx,
                |this, open, cx| {
                    this.vv.quality_open = open;
                    cx.notify();
                },
            )
            .child(self.anchor_probe(QUALITY_ROW))
            .into_any_element(),
            go_live::check_row(
                "vmenu-stream-audio",
                dc_intl::tr("Share Stream Audio"),
                prefs.sound,
                tint,
                cx,
                |this, cx| this.toggle_stream_audio(cx),
            )
            .on_hover(leave_submenu(cx))
            .into_any_element(),
        ];
        let height = 36. * rows.len() as f32 + 18.;
        let Some((left, top)) = self.menu_at("call-screen-group", 640., height) else {
            return Vec::new();
        };
        let mut out = vec![
            deferred(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(640.))
                    .flex()
                    .justify_center()
                    .child(self.anchor_probe(SHARE_ORIGIN))
                    .child(
                        // `menu_M3qk4j` is as wide as its longest row: 182.34 in the capture, wider when this
                        // client's text runs longer.
                        go_live::menu_frame(SHARE_PANEL, tint)
                            .min_w(px(WIDTH))
                            .max_w(px(320.))
                            .child(self.anchor_probe(SHARE_PANEL))
                            .popout(SHARE_PANEL, PopKind::Fade, PopSide::Top)
                            .on_mouse_down_out(cx.listener(
                                |this, event: &MouseDownEvent, _, cx| {
                                    if !this.inside_share_menus(event.position) {
                                        this.vv.quality_open = false;
                                        this.close_call_popup(event, cx);
                                    }
                                },
                            ))
                            .children(rows),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        ];
        if self.vv.quality_open
            && let Some(row) = self.tip_anchors.borrow().get(QUALITY_ROW).copied()
        {
            // The view's origin in the window, read off the share container where it was drawn.
            let origin = self
                .tip_anchors
                .borrow()
                .get(SHARE_ORIGIN)
                .map(|panel| (f32::from(panel.left()) - left, f32::from(panel.top()) - top))
                .unwrap_or_else(|| self.call_origin());
            out.push(self.quality_submenu(row, origin, cx));
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
                        .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.close_call_popup(event, cx);
                        }))
                        .children(rows),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        ]
    }

    /// The rows of Stream Quality (module 600125 `W`): the Frame Rate group (15, 30, 60 FPS), a separator, and the
    /// Resolution group (480p to Source), each a radio checked at the stored choice. A choice the account may not use
    /// is not greyed: it opens the upsell. Other menus (a stream tile's) can show the same rows.
    pub(super) fn stream_quality_rows(
        &self,
        tint: Tint,
        cx: &mut Context<Self>,
    ) -> Vec<(AnyElement, f32)> {
        let prefs = Prefs::load();
        let mut rows: Vec<(AnyElement, f32)> =
            vec![(go_live::group_label(dc_intl::tr("Frame Rate"), tint), 36.)];
        for fps in STREAM_FRAME_RATES {
            rows.push((
                go_live::radio_row(
                    SharedString::from(format!("stream-settings-fps-{fps}")),
                    div().child(frame_rate_label(fps)).into_any_element(),
                    None,
                    prefs.fps == fps,
                    tint,
                    cx,
                    move |this, cx| {
                        let choice = choose_frame_rate(&Prefs::load(), this.stream_account(), fps);
                        this.apply_stream_quality(choice, cx);
                    },
                )
                .into_any_element(),
                36.,
            ));
        }
        rows.push((go_live::separator(tint), 17.));
        rows.push((go_live::group_label(dc_intl::tr("Resolution"), tint), 36.));
        for resolution in STREAM_RESOLUTIONS {
            rows.push((
                go_live::radio_row(
                    SharedString::from(format!(
                        "stream-settings-resolution-{}",
                        resolution.height()
                    )),
                    div()
                        .child(go_live::resolution_text(resolution))
                        .into_any_element(),
                    None,
                    prefs.resolution == resolution,
                    tint,
                    cx,
                    move |this, cx| {
                        let choice =
                            choose_resolution(&Prefs::load(), this.stream_account(), resolution);
                        this.apply_stream_quality(choice, cx);
                    },
                )
                .into_any_element(),
                36.,
            ));
        }
        rows
    }

    /// Stream Quality's submenu beside its row (`submenu_M3qk4j`): its first row level with the row, 4px to the
    /// right, nudged 8px inside the window. `origin` is where the call view starts in the window.
    fn quality_submenu(
        &self,
        row: gpui::Bounds<Pixels>,
        origin: (f32, f32),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tint: Tint = mid;
        let rows = self.stream_quality_rows(tint, cx);
        let height = 2. + 16. + rows.iter().map(|(_, height)| height).sum::<f32>();
        let (viewport_width, viewport_height) = self.viewport;
        let right_side = f32::from(row.right()) + 4. + SUBMENU_WIDTH <= viewport_width - 8.;
        let left = if right_side {
            f32::from(row.right()) + 4.
        } else {
            f32::from(row.left()) - 4. - SUBMENU_WIDTH
        };
        let top = (f32::from(row.top()) - 9.)
            .min(viewport_height - 8. - height)
            .max(8.);
        deferred(
            div()
                .absolute()
                .left(px(left - origin.0))
                .top(px(top - origin.1))
                .child(
                    go_live::menu_frame(QUALITY_PANEL, tint)
                        .w(px(SUBMENU_WIDTH))
                        .child(self.anchor_probe(QUALITY_PANEL))
                        .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            if !this.inside_share_menus(event.position) {
                                this.vv.quality_open = false;
                                this.close_call_popup(event, cx);
                            }
                        }))
                        .children(rows.into_iter().map(|(row, _)| row)),
                ),
        )
        .with_priority(3)
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
                        .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.close_call_popup(event, cx);
                        }))
                        .children(rows),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use go_live::{Account, Preset};

    fn account(premium: u64) -> Account {
        Account {
            premium: Some(premium),
            guild_tier: None,
        }
    }

    #[test]
    fn stream_quality_offers_the_official_groups() {
        // No 5 FPS: it is Screenshare's own rate, never a row of this menu.
        assert_eq!(STREAM_FRAME_RATES, [15, 30, 60]);
        assert_eq!(frame_rate_label(15), "15 FPS");
        assert_eq!(STREAM_RESOLUTIONS.len(), 5);
        assert_eq!(STREAM_RESOLUTIONS[4], Resolution::Source);
    }

    #[test]
    fn a_choice_applies_as_custom_or_opens_the_upsell() {
        let gaming = Prefs {
            preset: Preset::Video,
            resolution: Resolution::P720,
            fps: 30,
            ..Prefs::default()
        };
        assert_eq!(
            choose_frame_rate(&gaming, account(0), 15),
            QualityChoice::Apply(Resolution::P720, 15)
        );
        assert_eq!(
            choose_frame_rate(&gaming, account(0), 60),
            QualityChoice::Upsell
        );
        assert_eq!(
            choose_frame_rate(&gaming, account(1), 60),
            QualityChoice::Apply(Resolution::P720, 60)
        );
        assert_eq!(
            choose_resolution(&gaming, account(0), Resolution::P1080),
            QualityChoice::Upsell
        );
        assert_eq!(
            choose_resolution(&gaming, account(2), Resolution::Source),
            QualityChoice::Apply(Resolution::Source, 30)
        );
        // From Screenshare's Source 5 fps a resolution takes 30 fps.
        let screenshare = Prefs {
            preset: Preset::Documents,
            resolution: Resolution::Source,
            fps: 5,
            ..Prefs::default()
        };
        assert_eq!(
            choose_resolution(&screenshare, account(0), Resolution::P720),
            QualityChoice::Apply(Resolution::P720, 30)
        );
        // A frame rate keeps Source, which only Nitro streams above 5 fps.
        assert_eq!(
            choose_frame_rate(&screenshare, account(1), 15),
            QualityChoice::Upsell
        );
        assert_eq!(
            choose_frame_rate(&screenshare, account(2), 15),
            QualityChoice::Apply(Resolution::Source, 15)
        );
    }
}
