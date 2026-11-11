//! The Camera section of Voice & Video: the preview with Test Video, and the camera select. Measured in
//! `design/dom/settings/voice-video-noise-darker-cozy.json` (the preview is a 696x220 box inside a 236px row, the
//! button 104x40 at its centre; the select is 40px high under a "Camera" label, the help line under it).
//!
//! The chosen device is stored in `audio.video_device`, as the call's camera button reads it. Test Video opens the
//! camera on a worker (the open can take seconds), shows its pictures, and closes it again on Stop Testing or when the
//! page goes away.
//!
//! Video Background (None, Blur, Custom) is not here: this client does not process camera video, and a row of
//! backgrounds that change nothing would be a lie.

use super::{ButtonKind, Root, button, color, mask_icon};
use crate::ui::video_tiles::render_image;
use dc_media::Picture;
use dc_media::camera::{Camera, CameraDevice};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, RenderImage, SharedString, Task, deferred, div,
    prelude::*, px,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const REPAINT: Duration = Duration::from_millis(100);

/// A running Test Video.
pub(in crate::ui) struct Test {
    camera: Option<Camera>,
    /// The newest picture the capture thread delivered.
    latest: Arc<Mutex<Option<Picture>>>,
    image: Option<Arc<RenderImage>>,
    error: Option<String>,
    _task: Task<()>,
}

#[derive(Default)]
pub(in crate::ui) struct CameraPage {
    pub open: bool,
    pub devices: Vec<CameraDevice>,
    pub test: Option<Test>,
}

/// The stored camera id, if it is still one of `devices`; else the first device.
pub(in crate::ui) fn chosen(devices: &[CameraDevice]) -> Option<String> {
    crate::storage::get("audio.video_device")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|id| devices.iter().any(|device| &device.id == id))
        .or_else(|| devices.first().map(|device| device.id.clone()))
}

impl Root {
    /// Closes the test camera and gives its image back (leaving the page, Stop Testing, a call's camera starting).
    pub(in crate::ui) fn camera_test_stop(&mut self, cx: &mut Context<Self>) {
        if let Some(test) = self.voice.camera.test.take()
            && let Some(image) = test.image
        {
            cx.drop_image(image, None);
        }
        cx.notify();
    }

    pub(in crate::ui) fn camera_test_start(&mut self, cx: &mut Context<Self>) {
        self.camera_test_stop(cx);
        // The call's camera owns the device while it is on.
        if self.call.as_ref().is_some_and(|call| call.camera) {
            return;
        }
        let devices = dc_voice::camera_devices();
        let device = chosen(&devices);
        let latest: Arc<Mutex<Option<Picture>>> = Arc::new(Mutex::new(None));
        let sink = latest.clone();
        let task = cx.spawn(async move |this, cx| {
            let opened = cx
                .background_executor()
                .spawn(async move {
                    if crate::inert::skip("camera") {
                        return Err(dc_media::Error::Unsupported("camera capture"));
                    }
                    Camera::start(device.as_deref(), move |frame| {
                        *sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame.preview);
                    })
                })
                .await;
            let Ok(still) = this.update(cx, |root, cx| {
                let Some(test) = root.voice.camera.test.as_mut() else {
                    return false;
                };
                match opened {
                    Ok(camera) => test.camera = Some(camera),
                    Err(error) => test.error = Some(error.to_string()),
                }
                cx.notify();
                true
            }) else {
                return;
            };
            if !still {
                return;
            }
            loop {
                cx.background_executor().timer(REPAINT).await;
                let picture = this
                    .update(cx, |root, cx| {
                        let test = root.voice.camera.test.as_mut()?;
                        if let Some(camera) = &test.camera
                            && camera.finished()
                        {
                            test.error = camera.error().or(Some("The camera stopped.".to_owned()));
                            test.camera = None;
                            cx.notify();
                        }
                        test.latest.lock().unwrap_or_else(|e| e.into_inner()).take()
                    })
                    .ok();
                let Some(picture) = picture else {
                    return;
                };
                let Some(picture) = picture else {
                    // No new picture: stop when the test is gone.
                    if this
                        .update(cx, |root, _| root.voice.camera.test.is_none())
                        .unwrap_or(true)
                    {
                        return;
                    }
                    continue;
                };
                let image = cx
                    .background_executor()
                    .spawn(async move { render_image(&picture) })
                    .await;
                let alive = this
                    .update(cx, |root, cx| {
                        let Some(test) = root.voice.camera.test.as_mut() else {
                            return false;
                        };
                        if let Some(image) = image
                            && let Some(old) = test.image.replace(image)
                        {
                            cx.drop_image(old, None);
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    return;
                }
            }
        });
        self.voice.camera.test = Some(Test {
            camera: None,
            latest,
            image: None,
            error: None,
            _task: task,
        });
        cx.notify();
    }

    /// `camera_video_preview`: the picture, and Test Video over an empty box.
    pub(super) fn camera_preview_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let test = self.voice.camera.test.as_ref();
        let running = test.is_some();
        let image = test.and_then(|test| test.image.clone());
        let message = test.and_then(|test| test.error.clone());
        let in_call = self.call.as_ref().is_some_and(|call| call.camera);
        div()
            .h(px(236.))
            .py(px(8.))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(220.))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(color("--background-base-low"))
                    .children(image.map(|image| {
                        gpui::img(image)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Contain)
                    }))
                    .children(message.map(|message| {
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px(px(16.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(message)
                    }))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(running, |this| this.items_end().pb(px(16.)))
                            .when(!in_call, |this| {
                                this.child(
                                    div()
                                        .id("camera-test")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if this.voice.camera.test.is_some() {
                                                this.camera_test_stop(cx);
                                            } else {
                                                this.camera_test_start(cx);
                                            }
                                        }))
                                        .child(button(
                                            "camera-test-button",
                                            if running {
                                                "Stop Testing"
                                            } else {
                                                "Test Video"
                                            },
                                            if running {
                                                ButtonKind::Secondary
                                            } else {
                                                ButtonKind::Primary
                                            },
                                        )),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    /// `camera_selection_setting`: the label, the select and the help line.
    pub(in crate::ui) fn camera_select_row(
        &self,
        bare: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let devices = if self.voice.camera.devices.is_empty() {
            dc_voice::camera_devices()
        } else {
            self.voice.camera.devices.clone()
        };
        let selected = chosen(&devices);
        let open = self.voice.camera.open;
        let name = devices
            .iter()
            .find(|device| Some(&device.id) == selected.as_ref())
            .map(|device| device.name.clone())
            .unwrap_or_else(|| "No Camera Detected".to_owned());
        let field = div()
            .id("camera-select")
            .h(px(40.))
            .w_full()
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(8.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.voice.camera.devices = dc_voice::camera_devices();
                this.voice.camera.open = !this.voice.camera.open;
                this.voice_sync_select_flag();
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .whitespace_nowrap()
                    .child(mask_icon("voice_member_camera", 20., "--text-default"))
                    .child(
                        div()
                            .ml(px(8.))
                            .text_color(color("--text-default"))
                            .child(name),
                    ),
            )
            .child(
                mask_icon("chevron_down", 18., "--text-default").when(open, |this| {
                    this.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5)))
                }),
            );
        let rows: Vec<AnyElement> = devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let picked = Some(&device.id) == selected.as_ref();
                let id = device.id.clone();
                div()
                    .id(SharedString::from(format!("camera-device-{index}")))
                    .h(px(48.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .when(picked, |this| {
                        this.bg(color("--interactive-background-selected"))
                    })
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.voice.camera.open = false;
                        this.voice_sync_select_flag();
                        this.choose_camera(id.clone(), cx);
                        // A running test follows the choice.
                        if this.voice.camera.test.is_some() {
                            this.camera_test_start(cx);
                        }
                    }))
                    .child(mask_icon(
                        "voice_member_camera",
                        20.,
                        if picked {
                            "--interactive-text-active"
                        } else {
                            "--text-subtle"
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .child(device.name.clone()),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .py(px(8.))
            .flex()
            .flex_col()
            .when(!bare, |this| {
                this.child(
                    div()
                        .mb(px(8.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child("Camera"),
                )
            })
            .child(div().relative().child(field).when(open, |this| {
                this.child(
                    deferred(
                        super::super::keyboard::outside(
                            div().id("camera-device-list"),
                            super::super::keyboard::Layer::SettingsSelect,
                            cx,
                        )
                        .occlude()
                        .absolute()
                        .top(px(48.))
                        .left_0()
                        .right_0()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .overflow_hidden()
                        .child(div().flex().flex_col().children(rows)),
                    )
                    .with_priority(2),
                )
            }))
            .when(!bare, |this| {
                this.child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-subtle"))
                        .child("Looking for more camera options? ")
                        .child(
                            div()
                                .id("camera-system-settings")
                                .cursor_pointer()
                                .text_color(color("--text-link"))
                                .hover(|style| style.underline())
                                .on_click(|_, _, cx| cx.open_url("ms-settings:camera"))
                                .child("Check out your system camera settings."),
                        ),
                )
            })
            .into_any_element()
    }

    /// The live picture of the preview modal: 430x242 (module 308229), fed by the same worker as Test Video.
    pub(in crate::ui) fn camera_preview_box(&self) -> AnyElement {
        let test = self.voice.camera.test.as_ref();
        let image = test.and_then(|test| test.image.clone());
        let message = test.and_then(|test| test.error.clone());
        div()
            .relative()
            .w(px(430.))
            .h(px(242.))
            .rounded(px(4.))
            .overflow_hidden()
            .bg(color("--background-base-low"))
            .children(image.map(|image| {
                gpui::img(image)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Contain)
            }))
            .children(message.map(|message| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(message)
            }))
            .into_any_element()
    }
}
