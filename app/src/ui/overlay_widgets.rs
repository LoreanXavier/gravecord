//! What the overlay window draws (module 427830 and the widgets of module 415173), in the Onyx theme.
//!
//! - The scrim (`overlayBackground_APCwFf`): `--background-scrim`, its opacity eased over 0.2 s (`ease`) between
//!   locked (0) and unlocked (1). A press on it locks the overlay again.
//! - The top-right toolbar while unlocked (`topRightContainer_APCwFf`, 24 px from the top and the end, 8 px apart):
//!   the small button group Widgets / Hide All / Reset Default Layout (`overlay-secondary`), then the Report and
//!   Close buttons (`buttonContainer_kay3av`: a 48 px circle, Close with its keybind under it).
//! - Each widget in its drag container (`wrapper_yjvjn7`, module 129537): 8 px padding and a 2 px dashed border,
//!   both transparent while locked; unlocked the box is `rgba(0,0,0,.16)` with a `hsla(0,0%,100%,.16)` border, and its
//!   extras (`extras_yjvjn7`) sit 10 px under it, or above it when the widget is anchored to the bottom: the title pill
//!   (`titleWrapper_Ry5oWJ`) and the 24 px buttons Settings, pin (Show/Hide window in game) and Hide widget
//!   (`button_Ry5oWJ`). They fade in over 0.3 s `cubic-bezier(.16,1,.3,1)` from 6 px up. A widget that is not pinned
//!   is hidden while locked (`visibility: hidden`).
//! - The empty state when every widget is gone (`emptyState_fzU3RU`): "No widgets on screen".
//! - The notifications widget (`container_jlSvwJ`) and its toasts (`animationWrapper_yoCgi_`, module 376113).

use super::Root;
use super::overlay::{Toast, ToastAction, onyx};
use dc_overlay::drag::{self, Container, Drag, Operation};
use dc_overlay::layout::{self, Anchor, Widget};
use dc_overlay::notify;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, Rgba, canvas, div, img,
    prelude::*, px, relative, rgba, svg,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// `bgShade_Ry5oWJ`: `rgba(19,19,24,.9)` with a `hsla(0,0%,100%,.08)` border.
const SHADE: u32 = 0x131318e6;
const SHADE_BORDER: u32 = 0xffffff14;
/// `button_kay3av` and `keybind_kay3av`: `rgba(19,19,24,.8)`; hovered `rgba(28,29,35,.8)`.
const ROUND: u32 = 0x131318cc;
const ROUND_HOVER: u32 = 0x1c1d23cc;
/// `wrapper_yjvjn7.unlocked`: `rgba(0,0,0,.16)` and `hsla(0,0%,100%,.16)`.
const BOX: u32 = 0x00000029;
const BOX_BORDER: u32 = 0xffffff29;
/// `animationWrapper_yoCgi_`: `#131318`; hovered when clickable `rgba(28,29,35,.95)`.
const TOAST: u32 = 0x131318ff;
const TOAST_HOVER: u32 = 0x1c1d23f2;

/// The scrim's transition: `opacity .2s ease`.
pub(super) const SCRIM: super::motion::Css =
    super::motion::Css::new(200., super::motion::Ease::Ease);
/// The extras' transition: `opacity .3s cubic-bezier(.16,1,.3,1)`, and under `.full-motion` the 6 px rise over 0.3 s
/// `ease-out`.
pub(super) const EXTRAS: super::motion::Ease = super::motion::Ease::Bezier(0.16, 1., 0.3, 1.);
pub(super) const EXTRAS_MS: f64 = 300.;

/// Records an element's bounds in the overlay window under `key`.
pub(super) fn probe(
    bounds: &Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    key: String,
) -> impl IntoElement {
    let bounds = bounds.clone();
    canvas(
        move |area, _, _| {
            bounds.borrow_mut().insert(key.clone(), area);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn icon(name: &str, size: f32, tint: Rgba) -> gpui::Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(tint)
}

/// A pixel anchor as the element's position.
fn placed<E: Styled + FluentBuilder>(element: E, anchor: Anchor) -> E {
    element
        .when_some(anchor.left, |this, left| this.left(px(left)))
        .when_some(anchor.right, |this, right| this.right(px(right)))
        .when_some(anchor.top, |this, top| this.top(px(top)))
        .when_some(anchor.bottom, |this, bottom| this.bottom(px(bottom)))
}

/// What one widget's frame needs from the overlay state.
pub(super) struct Frame {
    pub locked: bool,
    pub width: f32,
    pub height: f32,
    /// 0 to 1: how far the unlocked look has come in.
    pub unlocked: f32,
    /// The extras' rise in px (6 at the start of the fade, 0 at rest).
    pub rise: f32,
}

impl Root {
    /// `Z` of module 427830: a 48 px round button, the keybind under it when there is one.
    fn overlay_round_button(
        &self,
        id: &'static str,
        name: &'static str,
        keybind: Option<String>,
        on_click: impl Fn(&mut Root, &mut Context<Root>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .id(id)
                    .size(px(48.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(onyx("--border-subtle"))
                    .bg(rgba(ROUND))
                    .text_color(onyx("--interactive-text-default"))
                    .cursor_pointer()
                    .hover(|style| {
                        style
                            .bg(rgba(ROUND_HOVER))
                            .text_color(onyx("--interactive-text-hover"))
                    })
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        on_click(this, cx);
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path(format!("icons/{name}.svg"))
                            .size(px(24.))
                            .text_color(onyx("--interactive-text-default")),
                    ),
            )
            .when_some(keybind, |this, keybind| {
                this.child(
                    div()
                        .mt(px(4.))
                        .px(px(4.))
                        .py(px(2.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(onyx("--border-subtle"))
                        .bg(rgba(ROUND))
                        .text_size(px(10.))
                        .line_height(px(12.))
                        .font_weight(FontWeight(600.))
                        .text_color(onyx("--interactive-text-default"))
                        .whitespace_nowrap()
                        .child(keybind),
                )
            })
            .into_any_element()
    }

    /// A `size: "sm"` button of the toolbar's group: 32 px high, 4/12 px padding, a 16 px icon 4 px from its text in
    /// `body-sm/medium`, 8 px corners, the `overlay-secondary` (or `overlay-primary`) colours.
    fn overlay_small_button(
        &self,
        id: &'static str,
        name: &'static str,
        text: &'static str,
        primary: bool,
        on_click: impl Fn(&mut Root, &mut Context<Root>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let token = |part: &str, state: &str| {
            onyx(&format!(
                "--control-overlay-{}-{part}-{state}",
                if primary { "primary" } else { "secondary" }
            ))
        };
        div()
            .id(id)
            .flex_none()
            .h(px(32.))
            .min_w(px(60.))
            .px(px(12.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(token("border", "default"))
            .bg(token("background", "default"))
            .text_color(token("text", "default"))
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(token("background", "hover"))
                    .border_color(token("border", "hover"))
            })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                on_click(this, cx);
                cx.notify();
            }))
            .child(icon(name, 16., token("icon", "default")))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .whitespace_nowrap()
                    .child(text),
            )
            .into_any_element()
    }

    /// `to` of module 427830 while unlocked: the toolbar at the top right.
    pub(super) fn overlay_toolbar(&self, keybind: String, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .top(px(24.))
            .right(px(24.))
            .flex()
            .items_start()
            .justify_end()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.overlay_small_button(
                        "overlay-widgets-browse-button",
                        "overlay_widgets",
                        dc_intl::tr("Widgets"),
                        self.overlay.widgets_panel,
                        |this, _| this.overlay.widgets_panel = !this.overlay.widgets_panel,
                        cx,
                    ))
                    .child(self.overlay_small_button(
                        "overlay-widgets-hide-all-button",
                        "overlay_eye_slash",
                        dc_intl::tr("Hide All"),
                        false,
                        |this, _| this.overlay_change_layout(|layout| layout.hide_all()),
                        cx,
                    ))
                    .child(self.overlay_small_button(
                        "overlay-widgets-reset-button",
                        "overlay_refresh",
                        dc_intl::tr("Reset Default Layout"),
                        false,
                        |this, _| {
                            this.overlay_change_layout(|layout| {
                                layout.reset(&mut super::overlay::new_widget_id);
                                true
                            })
                        },
                        cx,
                    )),
            )
            // Report opens the Report a Problem dialog (module 967055).
            .child(self.overlay_round_button(
                "overlay-report-button",
                "overlay_flag",
                None,
                |this, cx| this.overlay_open_report(cx),
                cx,
            ))
            .child(self.overlay_round_button(
                "overlay-close-button",
                "overlay_x_large",
                Some(keybind),
                |this, cx| {
                    if !this.overlay.locked {
                        this.overlay_toggle_lock(cx);
                    }
                },
                cx,
            ))
            .into_any_element()
    }

    /// `e9`: unlocked with no widget drawn, the centred "No widgets on screen" and its Widgets button.
    pub(super) fn overlay_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(icon("overlay_widgets", 32., onyx("--white")))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(onyx("--text-strong"))
                                    .child(dc_intl::tr("No widgets on screen")),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(onyx("--text-muted"))
                                    .child(dc_intl::tr("Open Widgets to choose what shows up.")),
                            ),
                    )
                    .child(self.overlay_small_button(
                        "overlay-widgets-empty-browse",
                        "overlay_widgets",
                        dc_intl::tr("Widgets"),
                        true,
                        |this, _| this.overlay.widgets_panel = true,
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// A 24 px round button of a widget's extras (`button_Ry5oWJ`), its 12 px icon; `active` gives the brand border
    /// and icon (a pinned widget's pin).
    fn overlay_extras_button(
        &self,
        id: String,
        name: &'static str,
        active: bool,
        on_click: impl Fn(&mut Root, &mut Context<Root>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(gpui::SharedString::from(id))
            .size(px(24.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(if active {
                onyx("--background-brand")
            } else {
                rgba(SHADE_BORDER)
            })
            .bg(rgba(SHADE))
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                on_click(this, cx);
                cx.notify();
            }))
            .child(icon(
                name,
                12.,
                if active {
                    onyx("--background-brand")
                } else {
                    onyx("--interactive-text-active")
                },
            ))
            .into_any_element()
    }

    /// `J` of module 427830 inside `extras_yjvjn7`: the title, the widget's own buttons and Hide widget.
    fn overlay_extras(
        &self,
        widget: &Widget,
        title: &'static str,
        frame: &Frame,
        group: gpui::SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let operating = self
            .overlay
            .drag
            .as_ref()
            .is_some_and(|drag| drag.id == widget.id && drag.drag.started);
        let below = !(widget.anchor.bottom.is_some() && widget.anchor.top.is_none());
        let right = widget.anchor.right.is_some() && widget.anchor.left.is_none();
        let id = widget.id.clone();
        let pin_id = id.clone();
        let hide_id = id.clone();
        // `renderButtons`: the voice and notifications widgets have Settings and the pin; quick actions and chat none.
        let menu = match widget.kind.as_str() {
            layout::VOICE => Some(dc_overlay::menus::Menu::Voice),
            layout::NOTIFICATIONS => Some(dc_overlay::menus::Menu::Notifications),
            _ => None,
        };
        let row = div()
            .flex()
            .flex_row()
            .gap(px(4.))
            .min_w(px(0.))
            .px(px(8.))
            .child(
                div()
                    .h(px(22.))
                    .px(px(8.))
                    .py(px(2.))
                    .flex()
                    .items_center()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(rgba(SHADE_BORDER))
                    .bg(rgba(SHADE))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(onyx("--text-strong"))
                    .whitespace_nowrap()
                    .child(title),
            )
            // Settings opens the widget's menu at the pointer (modules 51444, 320490).
            .when_some(menu, |this, menu| {
                this.child(self.overlay_extras_button(
                    format!("overlay-settings-{id}"),
                    "overlay_settings",
                    false,
                    move |this, _| this.overlay_open_menu(menu),
                    cx,
                ))
                .child(self.overlay_extras_button(
                    format!("overlay-pin-{id}"),
                    "overlay_pin",
                    widget.pinned,
                    move |this, _| {
                        let id = pin_id.clone();
                        this.overlay_change_layout(|layout| layout.set_pinned(&id, None));
                    },
                    cx,
                ))
            })
            .child(self.overlay_extras_button(
                format!("overlay-hide-{id}"),
                "overlay_eye_slash",
                false,
                move |this, _| {
                    let id = hide_id.clone();
                    this.overlay_change_layout(|layout| layout.set_hidden(&[id], true));
                },
                cx,
            ));
        div()
            .absolute()
            .left_0()
            .min_w(relative(1.))
            .max_w(relative(1.))
            .flex()
            .when(right, |this| this.justify_end())
            .when(below, |this| this.top(relative(1.)).pt(px(10.)))
            .when(!below, |this| this.bottom(relative(1.)).pb(px(10.)))
            // `.lockExtras .extras` (or the box hovered) at full opacity; while the widget is moved or resized they go.
            .opacity(if self.overlay.show_widgets && !operating {
                frame.unlocked
            } else {
                0.
            })
            .when(!self.overlay.show_widgets && !operating, |this| {
                this.group_hover(group, |style| style.opacity(1.))
            })
            .mt(px(if below { -frame.rise } else { frame.rise }))
            .child(row)
            .into_any_element()
    }

    /// The drag container of one widget (`eO` of module 427830 around `v` of module 129537).
    pub(super) fn overlay_widget_frame(
        &self,
        widget: &Widget,
        title: &'static str,
        content: AnyElement,
        frame: &Frame,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dragging = self
            .overlay
            .drag
            .as_ref()
            .filter(|drag| drag.id == widget.id && drag.drag.started);
        let anchor = match dragging {
            Some(drag) => drag.drag.anchor,
            None => drag::with_defaults(layout::anchor_pixels(
                widget.anchor,
                frame.width,
                frame.height,
            )),
        };
        // `f`: the requested size is never under the minimum; a resize in progress draws its own size.
        let (fixed_w, fixed_h) =
            match dragging.filter(|drag| drag.drag.operation != Operation::Move) {
                Some(drag) => (Some(drag.drag.size.0), Some(drag.drag.size.1)),
                None => drag::requested_size(
                    widget.size.pixels(frame.width, frame.height),
                    widget.min_size,
                ),
            };
        // `hidden: !H`: shown while unlocked, or pinned (`isShownInGame` is the pin) while locked.
        let shown = widget.pinned || !frame.locked;
        let unlocked = !frame.locked;
        let id = widget.id.clone();
        let config = widget.config();
        let min_size = widget.min_size;
        let group = gpui::SharedString::from(format!("overlay-widget-group-{}", widget.id));
        // `.unlocked.lockExtras`, `.unlocked.operation` and `.unlocked:hover`: the box shows.
        let boxed = self.overlay.show_widgets || dragging.is_some();
        let handles = if unlocked {
            drag::handles(config.resize_x, config.resize_y)
        } else {
            Vec::new()
        };
        let element = div()
            .id(gpui::SharedString::from(format!(
                "overlay-widget-{}",
                widget.id
            )))
            .group(group.clone())
            .absolute()
            .when(dragging.is_some(), |this| this.occlude())
            .min_w(px(widget.min_size.0))
            .min_h(px(widget.min_size.1))
            .when_some(fixed_w, |this, width| {
                this.w(px(width.max(widget.min_size.0)))
            })
            .when_some(fixed_h, |this, height| {
                this.h(px(height.max(widget.min_size.1)))
            })
            .p(px(drag::PADDING))
            .border(px(drag::BORDER))
            .border_dashed()
            .rounded(px(drag::RADIUS))
            .border_color(gpui::transparent_black())
            .when(unlocked, |this| {
                let mut background = rgba(BOX);
                background.a *= frame.unlocked;
                let mut border = rgba(BOX_BORDER);
                border.a *= frame.unlocked;
                if boxed {
                    this.bg(background).border_color(border)
                } else {
                    this.hover(move |style| style.bg(background).border_color(border))
                }
            })
            .when(!shown, |this| this.invisible())
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if this.overlay.locked || !config.drag_anywhere {
                        return;
                    }
                    this.overlay_drag_start(&id, min_size, event.position, Operation::Move);
                }),
            )
            .child(probe(&self.overlay.bounds, format!("widget-{}", widget.id)))
            .child(content)
            .children(
                handles
                    .into_iter()
                    .map(|handle| self.overlay_resize_handle(&widget.id, min_size, handle, cx)),
            )
            .when(unlocked, |this| {
                this.child(self.overlay_extras(widget, title, frame, group, cx))
            });
        placed(element, anchor).into_any_element()
    }

    /// One resize handle (`renderResizeHandles`): a press starts that resize, its cursor while hovered.
    fn overlay_resize_handle(
        &self,
        id: &str,
        min_size: (f32, f32),
        handle: drag::Handle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cursor = match handle.cursor {
            drag::Cursor::NorthSouth => gpui::CursorStyle::ResizeUpDown,
            drag::Cursor::EastWest => gpui::CursorStyle::ResizeLeftRight,
            drag::Cursor::NorthWestSouthEast => gpui::CursorStyle::ResizeUpLeftDownRight,
            drag::Cursor::NorthEastSouthWest => gpui::CursorStyle::ResizeUpRightDownLeft,
        };
        let widget = id.to_owned();
        let operation = handle.operation;
        let element = div()
            .id(gpui::SharedString::from(format!(
                "overlay-resize-{id}-{operation:?}"
            )))
            .absolute()
            .when_some(handle.width, |this, width| this.w(px(width)))
            .when_some(handle.height, |this, height| this.h(px(height)))
            .cursor(cursor)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if !this.overlay.locked {
                        this.overlay_drag_start(&widget, min_size, event.position, operation);
                    }
                }),
            );
        placed(element, handle.anchor).into_any_element()
    }
}

// ----- the drag -----

impl Root {
    /// The press on a widget (`handleMouseDown` with `dragAnywhere`, or a drag handle such as the Chat widget's
    /// header) or on a resize handle (`handleResizeMouseDown`): the operation starts once the pointer has gone 2 px.
    pub(super) fn overlay_drag_start(
        &mut self,
        id: &str,
        min_size: (f32, f32),
        position: gpui::Point<Pixels>,
        operation: Operation,
    ) {
        let Some(bounds) = self
            .overlay
            .bounds
            .borrow()
            .get(&format!("widget-{id}"))
            .copied()
        else {
            return;
        };
        let (width, height) = self.overlay.size;
        let container = Container::window(width, height);
        let layout = self.overlay_layout();
        let Some(widget) = layout.widgets().into_iter().find(|widget| widget.id == id) else {
            return;
        };
        let anchor = layout::anchor_pixels(widget.anchor, width, height);
        // The probe fills the padding box: the border is outside it.
        let size = (
            f32::from(bounds.size.width) + 2. * drag::BORDER,
            f32::from(bounds.size.height) + 2. * drag::BORDER,
        );
        let pointer = (f32::from(position.x), f32::from(position.y));
        self.overlay.drag = Some(super::overlay::WidgetDrag {
            id: id.to_owned(),
            drag: Drag::start(operation, pointer, anchor, size, container),
            min_size,
        });
    }

    /// The pointer moved over the window while a widget is held.
    pub(super) fn overlay_drag_move(&mut self, position: gpui::Point<Pixels>) -> bool {
        let (width, height) = self.overlay.size;
        let Some(held) = self.overlay.drag.as_mut() else {
            return false;
        };
        let min_size = held.min_size;
        held.drag.moved(
            (f32::from(position.x), f32::from(position.y)),
            Container::window(width, height),
            min_size,
        )
    }

    /// The button came up (`handleOperationEnd`, then `onUpdate` or `onClick`): a moved widget keeps its anchor as
    /// fractions of the window (`LAYOUT_UPDATE_WIDGET`); either way it comes to the top (`LAYOUT_SET_TOP_WIDGET`).
    pub(super) fn overlay_drag_end(&mut self) {
        let Some(held) = self.overlay.drag.take() else {
            return;
        };
        let (width, height) = self.overlay.size;
        let anchor = held
            .drag
            .end(Container::window(width, height), held.drag.size)
            .map(|anchor| drag::to_fractions(anchor, width, height));
        let id = held.id;
        let resized = held.drag.started && held.drag.operation != Operation::Move;
        let pixels = held.drag.size;
        self.overlay_change_layout(|layout| {
            // `onUpdate`: the anchor as fractions (`NO`) and a resized size as it is stored (`R9`: fractions unless
            // fixed, auto sides stay auto).
            let size = resized
                .then(|| layout.widgets().into_iter().find(|widget| widget.id == id))
                .flatten()
                .map(|widget| widget.size.stored(pixels, width, height));
            let moved = anchor.is_some_and(|anchor| layout.update_widget(&id, Some(anchor), size));
            layout.set_top(&id) || moved
        });
        if resized {
            tracing::debug!("overlay widget resized to {}x{}", pixels.0, pixels.1);
        }
    }
}

// ----- notifications -----

/// A toast as it was last drawn: kept for its leave animation once it is gone from the list.
#[derive(Clone, Debug)]
pub(super) struct DrawnToast {
    pub id: u64,
    pub toast: Toast,
    pub index: usize,
    pub top: f32,
    pub height: f32,
    pub locked: bool,
}

/// The toasts of the last frame that are not listed now start leaving (`leave` of `Ru`, module 57019); leaving ones
/// are kept for `LEAVE_MS`.
pub(super) fn update_leaving(
    drawn: &[DrawnToast],
    listed: &[u64],
    leaving: &mut Vec<(DrawnToast, f64)>,
    now: f64,
) {
    for gone in drawn.iter().filter(|toast| !listed.contains(&toast.id)) {
        if !leaving.iter().any(|(toast, _)| toast.id == gone.id) {
            leaving.push((gone.clone(), now));
        }
    }
    leaving.retain(|(toast, since)| now - since < notify::LEAVE_MS && !listed.contains(&toast.id));
}

/// What a toast's parts are drawn with.
fn text_block(
    text: String,
    size: f32,
    line: f32,
    weight: f32,
    tint: Rgba,
    lines: Option<usize>,
) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(weight))
        .text_color(tint)
        .when_some(lines, |this, lines| this.line_clamp(lines))
        .child(text)
}

impl Root {
    /// One toast (`x` of module 376113): the 40 px icon 12 px before the details (title, body, hint, buttons), the
    /// dismiss button at the top right on hover.
    fn overlay_toast(&self, id: u64, toast: &Toast, cx: &mut Context<Self>) -> AnyElement {
        let group = gpui::SharedString::from(format!("overlay-toast-{id}"));
        let clickable = toast.click.is_some();
        let icon_box = toast.icon.as_ref().map(|key| {
            div()
                .flex_none()
                .size(px(40.))
                .mr(px(12.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .children(
                    self.images
                        .get(key)
                        .cloned()
                        .map(|image| img(image).size(px(40.)).rounded_full()),
                )
        });
        let button = |label: String, confirm: bool, action: ToastAction| {
            let (background, border, text) = if confirm {
                (
                    "--control-connected-background-default",
                    "--control-connected-border-default",
                    "--control-connected-text-default",
                )
            } else {
                (
                    "--control-secondary-background-default",
                    "--control-secondary-border-default",
                    "--control-secondary-text-default",
                )
            };
            div()
                .id(gpui::SharedString::from(format!(
                    "overlay-toast-{id}-{confirm}"
                )))
                .max_w(relative(0.5))
                .h(px(32.))
                .min_w(px(60.))
                .px(px(12.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .border_1()
                .border_color(onyx(border))
                .bg(onyx(background))
                .text_color(onyx(text))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .cursor_pointer()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.overlay_toast_action(id, action, cx);
                    cx.stop_propagation();
                }))
                .child(label)
        };
        let has_buttons = toast.confirm.is_some() || toast.cancel.is_some();
        div()
            .id(group.clone())
            .group(group.clone())
            .relative()
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(onyx("--border-subtle"))
            .bg(rgba(TOAST))
            .when(clickable, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(rgba(TOAST_HOVER)))
            })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when_some(toast.click, |this, action| {
                this.on_click(
                    cx.listener(move |this, _, _, cx| this.overlay_toast_action(id, action, cx)),
                )
            })
            .child(
                div().p(px(8.)).flex().children(icon_box).child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .overflow_hidden()
                        .when_some(toast.title.clone(), |this, title| {
                            this.child(text_block(
                                title,
                                14.,
                                18.,
                                600.,
                                onyx("--interactive-text-default"),
                                Some(2),
                            ))
                        })
                        .when_some(toast.body.clone(), |this, body| {
                            this.child(text_block(
                                body,
                                14.,
                                18.,
                                400.,
                                onyx("--interactive-text-default"),
                                toast.max_lines,
                            ))
                        })
                        .when_some(toast.hint.clone(), |this, hint| {
                            this.child(
                                text_block(hint, 10., 12., 500., onyx("--text-muted"), None)
                                    .mt(px(4.)),
                            )
                        })
                        .when(has_buttons, |this| {
                            this.child(
                                div()
                                    .mt(px(8.))
                                    .flex()
                                    .gap(px(8.))
                                    .when_some(toast.confirm.clone(), |this, label| {
                                        this.child(button(label, true, ToastAction::Confirm))
                                    })
                                    .when_some(toast.cancel.clone(), |this, label| {
                                        this.child(button(label, false, ToastAction::Cancel))
                                    }),
                            )
                        }),
                ),
            )
            // `DismissButton`: 8 px past the top right corner, shown while the toast is hovered.
            .child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "overlay-toast-{id}-dismiss"
                    )))
                    .absolute()
                    .top(px(-8.))
                    .right(px(-8.))
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(onyx("--border-subtle"))
                    .bg(onyx("--background-base-low"))
                    .invisible()
                    .group_hover(group, |style| style.visible())
                    .hover(|style| style.bg(onyx("--background-surface-high")))
                    .cursor_pointer()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.overlay_toast_action(id, ToastAction::Dismiss, cx);
                        cx.stop_propagation();
                    }))
                    .child(icon(
                        "overlay_x_small",
                        16.,
                        onyx("--interactive-text-default"),
                    )),
            )
            .into_any_element()
    }

    /// `rS` (module 415173): the listed toasts, stacked while locked (`Ru`, module 57019), each placed by its spring.
    /// The first one is a click zone (`W` of module 376113) unless the widget is unpinned and locked or the toast
    /// opts out.
    pub(super) fn overlay_notifications(
        &mut self,
        locked: bool,
        pinned: bool,
        show_empty: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = self.overlay_now();
        let disabled = self.overlay_notification_disabled(dc_overlay::store::TEXT_CHAT);
        let listed: Vec<(usize, u64, Toast)> = if disabled {
            Vec::new()
        } else {
            self.overlay
                .notices
                .visible(locked)
                .into_iter()
                .map(|(index, entry)| (index, entry.id, entry.content.clone()))
                .collect()
        };
        let heights: Vec<f32> = listed
            .iter()
            .map(|(_, id, _)| {
                self.overlay
                    .bounds
                    .borrow()
                    .get(&format!("toast-{id}"))
                    .map_or(0., |bounds| f32::from(bounds.size.height))
            })
            .collect();
        let tops = notify::tops(&heights);
        let first = heights.first().copied().unwrap_or(0.);
        let ids: Vec<u64> = listed.iter().map(|(_, id, _)| *id).collect();
        let previous = std::mem::take(&mut self.overlay.toasts_drawn);
        update_leaving(&previous, &ids, &mut self.overlay.leaving, now);
        self.overlay.toasts_drawn = listed
            .iter()
            .enumerate()
            .map(|(slot, (index, id, toast))| DrawnToast {
                id: *id,
                toast: toast.clone(),
                index: *index,
                top: tops[slot],
                height: heights[slot],
                locked,
            })
            .collect();
        let mut children = Vec::new();
        // The dismissed ones go: transparent, 80% and pushed down (drawn over the listed ones once reversed).
        let leaving = self.overlay.leaving.clone();
        for (gone, _) in &leaving {
            let rest = notify::leave_pose(gone.index, gone.locked, gone.top, gone.height);
            let pose = self.overlay_toast_pose(gone.id, rest, rest, now);
            self.overlay.animating = true;
            let scale = notify::drawn_scale(pose.scale);
            let width = 300.;
            let shrink_x = width * (1. - scale) / 2.;
            children.push(
                div()
                    .absolute()
                    .left(px(shrink_x))
                    .w(px(width - 2. * shrink_x))
                    .top(px(pose.offset + gone.height * (1. - scale)))
                    .opacity(pose.opacity)
                    .child(self.overlay_toast(gone.id, &gone.toast, cx))
                    .into_any_element(),
            );
        }
        for id in self
            .overlay
            .springs
            .keys()
            .filter(|(id, part)| {
                *part <= 4
                    && !ids.contains(id)
                    && !self.overlay.leaving.iter().any(|(gone, _)| gone.id == *id)
            })
            .copied()
            .collect::<Vec<_>>()
        {
            self.overlay.springs.remove(&id);
        }
        for (slot, (index, id, toast)) in listed.iter().enumerate() {
            let rest = notify::stack_pose(*index, locked, tops[slot], heights[slot], first);
            let start = notify::enter_pose(locked, heights[slot], first);
            let pose = self.overlay_toast_pose(*id, rest, start, now);
            let stacked = locked && *index != 0;
            let zone = *index == 0 && !toast.no_zone && !(!pinned && locked);
            let scale = notify::drawn_scale(pose.scale);
            // `transform-origin: 50% 100%`: the card shrinks towards its bottom middle.
            let width = 300.;
            let shrink_x = width * (1. - scale) / 2.;
            let full = if stacked { first } else { heights[slot] };
            let shrink_y = full * (1. - scale);
            let card = div()
                .absolute()
                .left(px(shrink_x))
                .w(px(width - 2. * shrink_x))
                .top(px(pose.offset + shrink_y))
                .opacity(pose.opacity)
                .when(stacked, |this| {
                    this.h(px((full * scale).max(0.))).overflow_hidden()
                })
                .child(
                    div()
                        .relative()
                        .opacity(if stacked { 0. } else { pose.content_opacity })
                        .child(self.overlay_toast(*id, toast, cx))
                        .child(probe(&self.overlay.bounds, format!("toast-{id}"))),
                )
                .when(stacked, |this| {
                    // The stacked ones show only their card.
                    this.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(onyx("--border-subtle"))
                            .bg(rgba(TOAST)),
                    )
                })
                .when(zone, |this| this.child(self.overlay_zone_probe()));
            children.push(card.into_any_element());
        }
        children.reverse();
        let empty = listed.is_empty() && show_empty && !locked;
        div()
            .relative()
            .min_w(px(300.))
            .min_h(px(100.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(children)
            .when(empty, |this| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("overlay_bell", 24., onyx("--white")).opacity(0.24)),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chrome_constants_are_the_stylesheets() {
        // `rgba(19,19,24,.9)`, `hsla(0,0%,100%,.08)`, `rgba(0,0,0,.16)`, `hsla(0,0%,100%,.16)`.
        assert_eq!(SHADE & 0xff, (0.9f32 * 255.).round() as u32);
        assert_eq!(SHADE_BORDER & 0xff, (0.08f32 * 255.).round() as u32);
        assert_eq!(BOX & 0xff, (0.16f32 * 255.).round() as u32);
        assert_eq!(BOX_BORDER & 0xff, (0.16f32 * 255.).round() as u32);
        assert_eq!(drag::RADIUS, 22.);
    }

    #[test]
    fn a_toast_gone_from_the_list_leaves_for_300_ms() {
        let toast = |id| DrawnToast {
            id,
            toast: Toast {
                kind: super::super::overlay::ToastKind::Text,
                icon: None,
                title: None,
                body: None,
                hint: None,
                max_lines: None,
                confirm: None,
                cancel: None,
                click: None,
                channel: None,
                no_zone: false,
            },
            index: 0,
            top: 0.,
            height: 60.,
            locked: true,
        };
        let mut leaving = Vec::new();
        update_leaving(&[toast(1), toast(2)], &[2], &mut leaving, 1000.);
        assert_eq!(leaving.iter().map(|(t, _)| t.id).collect::<Vec<_>>(), [1]);
        // Still leaving a little later, not twice.
        update_leaving(&[toast(2)], &[2], &mut leaving, 1200.);
        assert_eq!(leaving.len(), 1);
        // Gone after 300 ms.
        update_leaving(&[toast(2)], &[2], &mut leaving, 1300.);
        assert!(leaving.is_empty());
        // A toast listed again stops leaving.
        update_leaving(&[toast(3)], &[], &mut leaving, 2000.);
        update_leaving(&[], &[3], &mut leaving, 2010.);
        assert!(leaving.is_empty());
    }
}
