//! The user panel's microphone and headphone chevrons (`menu_7ppJp2` "Audio Device Actions"), measured in
//! `design/dom/menus/mic-chevron-darker-cozy.json`, `headphones-chevron-darker-cozy.json`, `mic-devices-` and
//! `mic-profile-`: the device row with its submenu of devices (Windows Default first, then each device split into
//! its name and kind), Input Profile (Voice Isolation, Studio, Custom), the volume slider, and Voice Settings. Device
//! and volume choices go to `dc_voice::prefs()` and are kept in storage.json.

use super::{Root, color, icon};
use crate::parity::Parity;
use crate::storage;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Point, SharedString, deferred, div, prelude::*, px,
};
use serde_json::json;

const INPUT_DEVICE: &str = "audio.input_device";
const OUTPUT_DEVICE: &str = "audio.output_device";
const INPUT_VOLUME: &str = "audio.input_volume";
const OUTPUT_VOLUME: &str = "audio.output_volume";
use super::settings::Profile;
const INPUT_PROFILE: &str = "audio.input_profile";
const MENU_WIDTH: f32 = 222.;
const SLIDER_WIDTH: f32 = 188.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Input,
    Output,
}

/// A submenu row: title, the line under it, the device it picks (none for the default or a profile), picked.
type SubRow = (String, Option<String>, Option<String>, bool);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sub {
    Devices,
    Profile,
}

pub(crate) struct AudioMenu {
    pub side: Side,
    pub at: Point<Pixels>,
    pub sub: Option<Sub>,
    pub dragging: bool,
}

/// "Microphone (H2015 Pro)" reads as "H2015 Pro" over "Microphone", as the official list shows it.
pub(crate) fn split_device(name: &str) -> (String, Option<String>) {
    match (name.find(" ("), name.ends_with(')')) {
        (Some(open), true) => (
            name[open + 2..name.len() - 1].to_owned(),
            Some(name[..open].to_owned()),
        ),
        _ => (name.to_owned(), None),
    }
}

/// Applies the stored choices to the voice engine at start.
pub(super) fn restore_audio_prefs() {
    let prefs = dc_voice::prefs();
    if let Some(device) =
        storage::get(INPUT_DEVICE).and_then(|value| value.as_str().map(str::to_owned))
    {
        prefs.set_input_device(Some(device));
    }
    if let Some(device) =
        storage::get(OUTPUT_DEVICE).and_then(|value| value.as_str().map(str::to_owned))
    {
        prefs.set_output_device(Some(device));
    }
    if let Some(gain) = storage::get(INPUT_VOLUME).and_then(|value| value.as_f64()) {
        prefs.set_input_gain(gain as f32);
    }
    if let Some(gain) = storage::get(OUTPUT_VOLUME).and_then(|value| value.as_f64()) {
        prefs.set_output_gain(gain as f32);
    }
}

/// 792251 `M`: a stored volume (100 = unity) to its slider position.
pub(super) fn volume_to_slider(volume: f32) -> f32 {
    if volume == 0. {
        return 0.;
    }
    let ratio = volume / 100.;
    let position = if ratio < 1. {
        ratio.powf(1. / 2.8)
    } else {
        20. * ratio.log10() / 6. + 1.
    };
    position * 100.
}

/// 792251 `w`: a slider position to the stored volume.
pub(super) fn slider_to_volume(position: f32) -> f32 {
    if position == 0. {
        return 0.;
    }
    let ratio = position / 100.;
    let volume = if ratio < 1. {
        ratio.powf(2.8)
    } else {
        10f32.powf((ratio - 1.) * 6. / 20.)
    };
    volume * 100.
}

/// The linear gain of a slider position (0 to 100 for the input, 0 to 200 for the output).
fn slider_gain(position: f32) -> f32 {
    slider_to_volume(position) / 100.
}

/// A volume from Voice & Video's sliders, as a linear gain, to the engine and storage.json.
pub(super) fn store_gain(side: Side, gain: f32) {
    let prefs = dc_voice::prefs();
    match side {
        Side::Input => {
            prefs.set_input_gain(gain);
            storage::set(INPUT_VOLUME, json!(prefs.input_gain()));
        }
        Side::Output => {
            prefs.set_output_gain(gain);
            storage::set(OUTPUT_VOLUME, json!(prefs.output_gain()));
        }
    }
}

impl Root {
    pub(super) fn toggle_audio_menu(&mut self, side: Side, cx: &mut Context<Self>) {
        let anchor = self
            .tip_anchors
            .borrow()
            .get(match side {
                Side::Input => "mic-chevron",
                Side::Output => "headphones-chevron",
            })
            .copied();
        if self
            .audio_menu
            .as_ref()
            .is_some_and(|menu| menu.side == side)
        {
            self.audio_menu = None;
        } else if let Some(bounds) = anchor {
            self.audio_menu = Some(AudioMenu {
                side,
                at: gpui::point(bounds.left() - px(33.), bounds.top()),
                sub: None,
                dragging: false,
            });
        }
        cx.notify();
    }

    pub(super) fn set_device(
        &mut self,
        side: Side,
        device: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let prefs = dc_voice::prefs();
        let key = match side {
            Side::Input => {
                prefs.set_input_device(device.clone());
                INPUT_DEVICE
            }
            Side::Output => {
                prefs.set_output_device(device.clone());
                OUTPUT_DEVICE
            }
        };
        storage::set(
            key,
            device.map_or(serde_json::Value::Null, |device| json!(device)),
        );
        // `setInputDevice` / `setOutputDevice` end with `mention3` on the voice output (module 827343).
        crate::sounds::play_device_changed();
        cx.notify();
    }

    /// The slider at `x` (window pixels) inside the 188px track that starts at `left`.
    fn set_volume_at(&mut self, side: Side, x: Pixels, left: f32, cx: &mut Context<Self>) {
        let fraction = ((f32::from(x) - left) / SLIDER_WIDTH).clamp(0., 1.);
        // The same perceptual curve and 1-step slider as Voice & Video's (module 792251): both write the same keys.
        match side {
            Side::Input => store_gain(side, slider_gain((fraction * 100.).round())),
            // The output slider runs to 200 %.
            Side::Output => store_gain(side, slider_gain((fraction * 200.).round())),
        }
        cx.notify();
    }

    fn audio_row(
        &self,
        id: &'static str,
        title: &'static str,
        detail: String,
        sub: Sub,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self
            .audio_menu
            .as_ref()
            .is_some_and(|menu| menu.sub == Some(sub));
        div()
            .id(id)
            .h(px(52.))
            .p(px(8.))
            .rounded(px(2.))
            .flex()
            .items_center()
            .when(open, |this| {
                this.bg(color("--interactive-background-hover"))
                    .rounded(px(4.))
            })
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if *inside && let Some(menu) = this.audio_menu.as_mut() {
                    menu.sub = Some(sub);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(detail),
                    ),
            )
            .child(icon("menu_caret", 20., "--text-muted"))
            .into_any_element()
    }

    fn audio_slider(&self, side: Side, cx: &mut Context<Self>) -> AnyElement {
        let prefs = dc_voice::prefs();
        let fraction = match side {
            Side::Input => volume_to_slider(prefs.input_gain() * 100.) / 100.,
            Side::Output => volume_to_slider(prefs.output_gain() * 100.) / 200.,
        }
        .clamp(0., 1.);
        let left = self
            .audio_menu
            .as_ref()
            .map_or(0., |menu| f32::from(menu.at.x))
            + 1.
            + 8.
            + 8.;
        div()
            .id("audio-volume")
            .relative()
            .mx(px(8.))
            .h(px(24.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    if let Some(menu) = this.audio_menu.as_mut() {
                        menu.dragging = true;
                    }
                    this.set_volume_at(side, event.position.x, left, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if this.audio_menu.as_ref().is_some_and(|menu| menu.dragging) && event.dragging() {
                    this.set_volume_at(side, event.position.x, left, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(menu) = this.audio_menu.as_mut() {
                        menu.dragging = false;
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(10.))
                    .w(px(SLIDER_WIDTH))
                    .h(px(4.))
                    .rounded(px(4.))
                    .bg(color("--background-mod-strong"))
                    .child(
                        div()
                            .h_full()
                            .w(px(SLIDER_WIDTH * fraction))
                            .rounded(px(4.))
                            .bg(color("--background-brand")),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(SLIDER_WIDTH * fraction - 8.))
                    .top(px(4.))
                    .size(px(16.))
                    .rounded_full()
                    .bg(gpui::rgb(0xffffff)),
            )
            .into_any_element()
    }

    fn audio_submenu(&self, menu: &AudioMenu, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sub = menu.sub?;
        let side = menu.side;
        let radio = |picked: bool| super::create_channel::radio(picked);
        let mut rows: Vec<AnyElement> = Vec::new();
        let (top_offset, entries): (f32, Vec<SubRow>) = match sub {
            Sub::Devices => {
                let (devices, default, chosen) = match side {
                    Side::Input => (
                        dc_voice::input_devices(),
                        dc_voice::default_input_device(),
                        dc_voice::prefs().input_device(),
                    ),
                    Side::Output => (
                        dc_voice::output_devices(),
                        dc_voice::default_output_device(),
                        dc_voice::prefs().output_device(),
                    ),
                };
                let mut list = vec![(
                    "Windows Default".to_owned(),
                    default.map(|name| split_device(&name).0),
                    None,
                    chosen.is_none(),
                )];
                for device in devices {
                    let (name, kind) = split_device(&device);
                    let picked = chosen.as_deref() == Some(device.as_str());
                    list.push((name, kind, Some(device), picked));
                }
                (0., list)
            }
            Sub::Profile => {
                let current = storage::get(INPUT_PROFILE)
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "Custom".to_owned());
                (
                    52.,
                    Profile::ALL
                        .iter()
                        .map(|profile| {
                            (
                                profile.label().to_owned(),
                                None,
                                None,
                                current == profile.label(),
                            )
                        })
                        .collect(),
                )
            }
        };
        for (index, (title, detail, device, picked)) in entries.into_iter().enumerate() {
            let device_row = sub == Sub::Devices;
            let profile = title.clone();
            rows.push(
                div()
                    .id(SharedString::from(format!("audio-sub-{index}")))
                    .h(px(if detail.is_some() { 52. } else { 36. }))
                    .p(px(8.))
                    .rounded(px(2.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .hover(|style| {
                        style
                            .bg(color("--interactive-background-hover"))
                            .rounded(px(4.))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if device_row {
                            this.set_device(side, device.clone(), cx);
                        } else {
                            // `setActiveInputProfile`: the same store as Voice & Video's radios.
                            this.set_input_profile(Profile::from_label(&profile));
                        }
                        this.audio_menu = None;
                        cx.notify();
                    }))
                    .when(device_row, |this| {
                        this.child(div().mr(px(8.)).child(icon(
                            if side == Side::Input {
                                "mic"
                            } else {
                                "headphones"
                            },
                            16.,
                            "--text-muted",
                        )))
                    })
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .children(detail.map(|detail| {
                                div()
                                    .mt(px(2.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(detail)
                            })),
                    )
                    .child(radio(picked))
                    .into_any_element(),
            );
        }
        let left = f32::from(menu.at.x) + MENU_WIDTH - 4.;
        let top = self.audio_menu_top(menu) + top_offset;
        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .child(
                    div()
                        .id("audio-submenu")
                        .occlude()
                        .w(px(192.))
                        .p(px(8.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    /// The menu's top: its bottom sits 4px over the chevron.
    fn audio_menu_top(&self, menu: &AudioMenu) -> f32 {
        let height = match menu.side {
            Side::Input => 250.,
            Side::Output => 198.,
        };
        f32::from(menu.at.y) - 4. - height
    }

    pub(super) fn audio_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.audio_menu.as_ref()?;
        let side = menu.side;
        let prefs = dc_voice::prefs();
        let device_name = |chosen: Option<String>| {
            chosen
                .map(|name| split_device(&name).0)
                .unwrap_or_else(|| "Windows Default".to_owned())
        };
        let mut items: Vec<AnyElement> = Vec::new();
        match side {
            Side::Input => {
                items.push(self.audio_row(
                    "audio-input-device",
                    "Input Device",
                    device_name(prefs.input_device()),
                    Sub::Devices,
                    cx,
                ));
                let profile = storage::get(INPUT_PROFILE)
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "Custom".to_owned());
                items.push(self.audio_row(
                    "audio-input-profile",
                    "Input Profile",
                    profile,
                    Sub::Profile,
                    cx,
                ));
            }
            Side::Output => {
                items.push(self.audio_row(
                    "audio-output-device",
                    "Output Device",
                    device_name(prefs.output_device()),
                    Sub::Devices,
                    cx,
                ));
            }
        }
        let separator = || div().my(px(8.)).h(px(1.)).bg(color("--border-subtle"));
        let top = self.audio_menu_top(menu);
        let panel = div()
            .parity_with_border("audio-menu", 1.)
            .id("audio-menu")
            .occlude()
            .w(px(MENU_WIDTH))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .children(items)
            .child(separator())
            .child(
                div()
                    .id("audio-volume-row")
                    .h(px(58.))
                    .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                        if *inside && let Some(menu) = this.audio_menu.as_mut() {
                            menu.sub = None;
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .p(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(if side == Side::Input {
                                "Input Volume"
                            } else {
                                "Output Volume"
                            }),
                    )
                    .child(self.audio_slider(side, cx)),
            )
            .child(separator())
            .child(
                div()
                    .id("audio-voice-settings")
                    .h(px(36.))
                    .p(px(8.))
                    .rounded(px(2.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .hover(|style| {
                        style
                            .bg(color("--interactive-background-hover"))
                            .rounded(px(4.))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.audio_menu = None;
                        this.open_settings_section("Voice & Video", cx);
                    }))
                    .child(div().mr(px(8.)).child(icon(
                        "settings",
                        20.,
                        "--interactive-text-default",
                    )))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Voice Settings"),
                    ),
            );
        Some(
            // A click anywhere else lands on this backdrop and closes the menu (and its submenu).
            deferred(
                div()
                    .id("audio-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.audio_menu = None;
                            cx.notify();
                        }),
                    )
                    .child(div().absolute().left(menu.at.x).top(px(top)).child(panel))
                    .children(self.audio_submenu(menu, cx)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The user panel's sliders share storage with Voice & Video's, so they share its curve: halfway on the input
    /// slider is a gain of 0.1436 (0.5^2.8), not 0.5, and the far end of the output slider is 2.
    #[test]
    fn the_panel_sliders_use_the_perceptual_curve() {
        assert!((slider_gain(50.) - 0.143_58).abs() < 1e-4);
        assert_eq!(slider_gain(100.), 1.);
        assert!((slider_gain(200.) - 1.995_26).abs() < 1e-4);
        assert_eq!(slider_gain(0.), 0.);
        // Reading back gives the position that was set.
        let position = volume_to_slider(slider_gain(73.) * 100.);
        assert!((position - 73.).abs() < 1e-3);
    }

    use super::split_device;

    #[test]
    fn device_names_split_like_the_list() {
        assert_eq!(
            split_device("Microphone (H2015 Pro)"),
            ("H2015 Pro".to_owned(), Some("Microphone".to_owned()))
        );
        assert_eq!(split_device("Speakers"), ("Speakers".to_owned(), None));
    }
}
