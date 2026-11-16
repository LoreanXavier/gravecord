//! Pop Out of the call bar (`handleOpenPopout` of module 540462): the call moves to a window of its own and the
//! place it had says so ("You've popped out the player to another window"). The window draws the same call view
//! as the main one, from the same state; closing it, pressing Pop Out in it again or leaving the call brings the
//! call back.
//!
//! The tooltips of the controls in the window are drawn by its own tooltip layer, at its coordinates; the main
//! window's layer leaves them out (`tooltip::drawn_in`).
//!
//! Not from the official client: the window's first size (960 by 540) and its title.

use super::Root;
use gpui::{
    AnyElement, Bounds, Context, IntoElement, Render, TitlebarOptions, WeakEntity, Window,
    WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb, size,
};

/// The root view of the popped-out window: the call view of the app's `Root`, at this window's size.
pub(crate) struct Popout {
    root: WeakEntity<Root>,
}

impl Render for Popout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let area = window.viewport_size();
        let (width, height) = (f32::from(area.width), f32::from(area.height));
        let id = window.window_handle().window_id();
        let (body, tips) = self
            .root
            .upgrade()
            .map(|root| {
                root.update(cx, |root, cx| {
                    let body = root.call_popout_body(width, height, cx);
                    (body, root.tip_layer_in(Some(id)))
                })
            })
            .unwrap_or_default();
        div()
            .id("call-popout")
            .relative()
            .size_full()
            .bg(rgb(0x000000))
            .children(body)
            .children(tips)
    }
}

impl Root {
    pub(super) fn call_popped_out(&self) -> bool {
        self.vv.popout.is_some()
    }

    /// Pop Out: opens the window, or closes it when it is open.
    pub(super) fn toggle_call_popout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.vv.popout.is_some() {
            self.close_call_popout(cx);
            return;
        }
        if self.call.is_none() {
            return;
        }
        if self.vv.fullscreen {
            self.vv.fullscreen = false;
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        }
        self.vv.menu = None;
        let root = cx.entity().downgrade();
        // The window draws its first frame as it opens, and that frame reads this view: it opens once this update
        // is over.
        cx.defer(move |cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(540.)), cx);
            let view = root.clone();
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("gravecord".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    crate::notifications::observe_window(window);
                    let closed = view.clone();
                    window.on_window_should_close(cx, move |window, cx| {
                        let id = window.window_handle().window_id();
                        let _ = closed.update(cx, |root, cx| {
                            root.drop_tips_of(id);
                            root.vv.popout_on_top = false;
                            root.vv.popout = None;
                            cx.notify();
                        });
                        true
                    });
                    cx.new(|cx| {
                        if let Some(entity) = view.upgrade() {
                            // Whatever changes the call redraws this window too.
                            cx.observe(&entity, |_, _, cx| cx.notify()).detach();
                        }
                        Popout { root: view }
                    })
                },
            );
            if let Ok(handle) = opened {
                let _ = root.update(cx, |root, cx| {
                    root.vv.popout = Some(handle);
                    cx.notify();
                });
            }
        });
        cx.notify();
    }

    /// Stay On Top and Remove From Top of the popped-out window (`handleStayOnTop`).
    pub(super) fn toggle_call_popout_on_top(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.vv.popout else {
            return;
        };
        let on = !self.vv.popout_on_top;
        self.vv.popout_on_top = on;
        // The window is being drawn when its own button asks: it is changed once this update is over.
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| crate::autostart::set_topmost(window, on));
        });
        cx.notify();
    }

    pub(super) fn close_call_popout(&mut self, cx: &mut Context<Self>) {
        self.vv.popout_on_top = false;
        if let Some(handle) = self.vv.popout.take() {
            self.drop_tips_of(gpui::AnyWindowHandle::from(handle).window_id());
            // The window may be the one handling the press that closes it: it goes once this update is over.
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
        cx.notify();
    }

    /// The popped-out window does not outlive the call.
    pub(super) fn call_popout_frame(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() {
            // What belonged to the call that ended.
            self.vv.end_shows_stream.set(false);
            self.call_drags_end();
            if self.vv.popout.is_some() {
                self.close_call_popout(cx);
            }
        }
    }

    /// The call view for the popped-out window.
    fn call_popout_body(
        &mut self,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (guild, channel) = self.call.as_ref().map(|call| (call.guild, call.channel))?;
        self.vv.popout_render = true;
        let body = self.call_view(guild, channel, width, height, cx);
        self.vv.popout_render = false;
        Some(
            div()
                .id("call-popout-body")
                .absolute()
                .inset_0()
                // Module 156652: a press wakes the controls as a move does; the pointer leaving the area makes it idle at once.
                .capture_any_mouse_down(cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                    this.voice_view_moved(cx);
                }))
                .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                    if !*inside {
                        this.voice_view_left(cx);
                    }
                }))
                .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                    if event.pressed_button.is_none() {
                        this.call_drags_end();
                    }
                    this.voice_view_moved(cx);
                }))
                .child(body)
                .into_any_element(),
        )
    }

    /// What the call's place in the main window shows while the call is in its own window (`screenMessage`).
    pub(super) fn call_popped_out_message(&self) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(gpui::FontWeight(600.))
            .text_color(rgb(0xffffff))
            .child("You've popped out the player to another window")
            .into_any_element()
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// Drives only this run's windows; the close step posts the same native message as the popout's X.
    pub(crate) fn script_call_window(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        match action {
            "fullscreen" => self.toggle_call_fullscreen(window, cx),
            "fullscreen-native" => {
                if let Err(error) = crate::harness::native::allow_fullscreen(window) {
                    return format!("call-window refused: {error}");
                }
                self.toggle_call_fullscreen(window, cx);
            }
            "popout" => self.toggle_call_popout(window, cx),
            "popout-fullscreen-native" => {
                if let Some(handle) = self.vv.popout {
                    cx.defer(move |cx| {
                        let _ = handle.update(cx, |_, window, _| {
                            match crate::harness::native::allow_fullscreen(window) {
                                Ok(()) => window.toggle_fullscreen(),
                                Err(error) => tracing::warn!(%error, "native fullscreen test refused"),
                            }
                        });
                    });
                }
            }
            "popout-state" => {
                return self.vv.popout.map_or_else(|| "no popout".into(), |handle| {
                    handle.update(cx, |_, window, _| format!(
                        "native_fullscreen={} native=[{}]", window.is_fullscreen(),
                        crate::harness::native::window_state(window),
                    )).unwrap_or_else(|error| error.to_string())
                });
            }
            "minimize-native" | "restore-native" => {
                if let Err(error) = crate::harness::native::minimize_or_restore(window, action == "minimize-native") {
                    return format!("call-window refused: {error}");
                }
            }
            "close-native" => {
                if let Some(handle) = self.vv.popout {
                    match handle.update(cx, |_, window, cx| crate::harness::native::close(window, cx)) {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => return format!("call-window refused: {error}"),
                        Err(error) => return format!("call-window unavailable: {error}"),
                    }
                }
            }
            "state" => {}
            _ => return "call-window: expected fullscreen, fullscreen-native, popout, popout-fullscreen-native, popout-state, close-native, minimize-native, restore-native or state".into(),
        }
        format!(
            "native_fullscreen={} call_fullscreen={} popped_out={} native=[{}]",
            window.is_fullscreen(),
            self.vv.fullscreen,
            self.vv.popout.is_some(),
            crate::harness::native::window_state(window)
        )
    }
}

/// The handle `Root` keeps of the window.
pub(crate) type Handle = WindowHandle<Popout>;

#[cfg(feature = "capture")]
impl Root {
    /// The popped-out call window, for the harness's `popout click` and `popout shot`.
    pub(crate) fn call_popout_window(&self) -> Option<gpui::AnyWindowHandle> {
        self.vv.popout.map(Into::into)
    }
}
