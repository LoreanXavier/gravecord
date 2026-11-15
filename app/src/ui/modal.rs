//! The design system modal (`container_dvN-MP`, size md): 480 wide on `--background-surface-high`, a 20px semibold
//! title with a close button, a muted subtitle, an optional body, and a footer of two equal buttons. Measured in
//! `design/dom/modals/leaving-discord-darker-cozy.json` and `pin-confirm-darker-cozy.json`; the scrim is black at
//! 0.72 (`scrim_od1dl1`).

use super::motion::{self, Transitions as _};
use super::{Root, avatar_source, color, header_time, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Message;
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, IntoElement, SharedString, Window, deferred, div,
    img, prelude::*, px, rgba,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Secondary,
    Primary,
    /// Primary while its form is incomplete: half opacity, and the click does nothing.
    PrimaryDisabled,
    Critical,
    /// `critical-secondary` (Revoke Ban).
    CriticalSecondary,
    /// The `active` button variant (Start Event): the green connected control.
    Active,
    /// Primary while its request is in flight (`loading`, module 657718): the label is hidden (`opacity:0`), three
    /// pulsing dots take its place, and the click does nothing.
    PrimaryLoading,
}

pub(super) type Handler = Box<dyn Fn(&mut Root, &mut Window, &mut Context<Root>)>;

pub(super) struct Button {
    pub id: &'static str,
    pub label: SharedString,
    pub tone: Tone,
    pub run: Handler,
    pub focus: Option<gpui::FocusHandle>,
    /// A leading icon (asset name, size in px, colour token), as the design system button's `icon` prop.
    pub icon: Option<(&'static str, f32, &'static str)>,
}

impl Button {
    /// The button with a leading icon; the icon and the label sit 4px apart (`buttonChildren`).
    pub(super) fn with_icon(mut self, name: &'static str, size: f32, token: &'static str) -> Self {
        self.icon = Some((name, size, token));
        self
    }
}

pub(super) fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    tone: Tone,
    run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
) -> Button {
    Button {
        id,
        label: label.into(),
        tone,
        run: Box::new(run),
        focus: None,
        icon: None,
    }
}

pub(super) struct Spec {
    pub id: &'static str,
    pub title: SharedString,
    /// Some dialogs (Create Channel) have none, and no gap for one.
    pub subtitle: Option<AnyElement>,
    pub body: Option<AnyElement>,
    /// The footer: two equal buttons, or one across it (Done).
    pub buttons: Vec<Button>,
    /// Close button, scrim click and Escape.
    pub close: Handler,
}

thread_local! {
    /// The handles of dialog primary buttons, one per button id, kept across frames.
    static PRIMARY_HANDLES: std::cell::RefCell<std::collections::HashMap<&'static str, gpui::FocusHandle>> =
        std::cell::RefCell::default();
    /// The primary button of the dialog the current frame laid out.
    static PRIMARY_NOW: std::cell::RefCell<Option<gpui::FocusHandle>> = const { std::cell::RefCell::new(None) };
}

fn primary_handle(id: &'static str, cx: &mut Context<Root>) -> gpui::FocusHandle {
    PRIMARY_HANDLES.with(|handles| {
        handles
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| cx.focus_handle())
            .clone()
    })
}

fn note_primary(handle: &gpui::FocusHandle) {
    PRIMARY_NOW.with(|now| {
        now.borrow_mut().get_or_insert_with(|| handle.clone());
    });
}

/// A frame starts: no dialog button has been laid out yet.
pub(super) fn begin_frame() {
    PRIMARY_NOW.with(|now| *now.borrow_mut() = None);
}

/// The first primary button the frame laid out in a dialog footer.
pub(super) fn dialog_primary() -> Option<gpui::FocusHandle> {
    PRIMARY_NOW.with(|now| now.borrow().clone())
}

pub(super) fn footer_button(spec: Button, cx: &mut Context<Root>) -> AnyElement {
    footer_button_sized(spec, true, cx)
}

/// `fill`: the button takes its share of the footer; else it is as wide as its text (beside an action bar input).
fn footer_button_sized(spec: Button, fill: bool, cx: &mut Context<Root>) -> AnyElement {
    let (background, border, text) = match spec.tone {
        Tone::Secondary => (
            color("--control-secondary-background-default"),
            color("--border-muted"),
            color("--control-secondary-text-default"),
        ),
        Tone::Primary | Tone::PrimaryDisabled | Tone::PrimaryLoading => (
            color("--control-primary-background-default"),
            color("--control-primary-border-default"),
            color("--white"),
        ),
        Tone::Active => (
            color("--control-connected-background-default"),
            color("--control-connected-border-default"),
            color("--control-connected-text-default"),
        ),
        Tone::Critical => (
            color("--control-critical-primary-background-default"),
            color("--control-critical-primary-border-default"),
            color("--white"),
        ),
        Tone::CriticalSecondary => (
            color("--control-critical-secondary-background-default"),
            color("--control-critical-secondary-border-default"),
            color("--control-critical-secondary-text-default"),
        ),
    };
    // The `:hover` and `:active` colours of the variant, as the design system button sets them.
    let family = match spec.tone {
        Tone::Secondary => "secondary",
        Tone::Primary | Tone::PrimaryDisabled | Tone::PrimaryLoading => "primary",
        Tone::Active => "connected",
        Tone::Critical => "critical-primary",
        Tone::CriticalSecondary => "critical-secondary",
    };
    let state_colors = |state: &str| {
        (
            color(&format!("--control-{family}-background-{state}")),
            color(&format!("--control-{family}-border-{state}")),
            color(&format!("--control-{family}-text-{state}")),
        )
    };
    let hover = state_colors("hover");
    let active = state_colors("active");
    let run = std::rc::Rc::new(spec.run);
    let keyboard_run = run.clone();
    let loading = spec.tone == Tone::PrimaryLoading;
    // A loading button swallows clicks and hover (`ea = disabled || loading`), but keeps full opacity.
    let disabled = spec.tone == Tone::PrimaryDisabled || loading;
    // The primary button of a dialog keeps a handle across frames: it is what a dialog takes focus on when it opens,
    // so that Enter confirms it (the footer's submit button of the official confirm modals).
    let spec_focus = spec.focus.clone().or_else(|| {
        (matches!(spec.tone, Tone::Primary | Tone::Critical | Tone::Active)).then(|| {
            let handle = primary_handle(spec.id, cx);
            note_primary(&handle);
            handle
        })
    });
    let focus = spec_focus.clone();
    super::keyboard::ring_when_focused(div().id(spec.id))
        // A button of a dialog is a tab stop; Enter and Space click it (gpui turns them into the click below).
        .when(spec_focus.is_none(), |button| {
            button.focusable().tab_index(0)
        })
        .when_some(
            spec_focus.map(|handle| handle.tab_index(0)),
            |button, focus| button.track_focus(&focus),
        )
        .when(disabled && !loading, |this| this.opacity(0.5))
        .when(fill, |this| this.flex_1())
        .when(!fill, |this| this.px(px(15.)))
        // A secondary button beside an action bar input is 100 wide (Cancel in Filters).
        .when(
            !fill && (spec.tone == Tone::Secondary || spec.id == "voice-dialog-confirm"),
            |this| this.min_w(px(100.)),
        )
        .h(px(40.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(background)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        // `.button_9OQtot:disabled, [aria-disabled=true]{cursor:not-allowed;opacity:.5}` (193433).
        .when(disabled && !loading, |this| this.cursor_not_allowed())
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(text)
        .when(!disabled, |this| {
            this.hover(move |style| style.bg(hover.0).border_color(hover.1).text_color(hover.2))
                .active(move |style| {
                    style
                        .bg(active.0)
                        .border_color(active.1)
                        .text_color(active.2)
                })
        })
        .rule(motion::Rule::Button)
        .when_some(spec.icon, |this, (name, size, token)| {
            this.gap(px(4.)).child(
                gpui::svg()
                    .path(format!("icons/{name}.svg"))
                    .size(px(size))
                    .text_color(color(token)),
            )
        })
        .when(!loading, |this| this.child(spec.label.clone()))
        .when(loading, |this| {
            // `.buttonChildren.loading{opacity:0}` keeps the label's width; the dots sit centred over it
            // (`.spinnerWrapper` absolute, 100% size; `.spinner-md{transform:scale(.9)}`).
            this.relative()
                .child(div().opacity(0.).child(spec.label.clone()))
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(super::components::dots(spec.id, "--white")),
                )
        })
        .on_key_down(
            cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                if focus.as_ref().is_some_and(|focus| focus.is_focused(window))
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    if !disabled {
                        keyboard_run(this, window, cx);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }
            }),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            if !disabled {
                run(this, window, cx);
            }
            cx.notify();
        }))
        .into_any_element()
}

pub(super) fn render(spec: Spec, cx: &mut Context<Root>) -> AnyElement {
    render_sized(spec, 480., false, None, None, cx)
}

/// The dialog that sits `top` pixels from the window's top edge and has a fixed `height` (the voters dialog of a
/// poll: `outerContainer_dvN-MP` pads 56px at the top and the 680 wide container is 608 high in a 720 window). Its
/// body is laid out by the caller, edge to edge below the header.
pub(super) fn render_pinned(
    spec: Spec,
    width: f32,
    top: f32,
    height: f32,
    cx: &mut Context<Root>,
) -> AnyElement {
    render_sized(spec, width, false, None, Some((top, height)), cx)
}

/// The pinned dialog with an input in its action bar (Add Account: a Back text button beside Continue).
pub(super) fn render_pinned_with_input(
    spec: Spec,
    width: f32,
    top: f32,
    height: f32,
    input: AnyElement,
    cx: &mut Context<Root>,
) -> AnyElement {
    render_sized(spec, width, false, Some(input), Some((top, height)), cx)
}

/// The dialog with an input in its action bar (`actionBarInput`): a checkbox or a text button on the left, the
/// buttons as wide as their text on the right (`actionBarLeading` grows, `actionBarTrailing` sits at the end).
pub(super) fn render_with_input(
    spec: Spec,
    input: AnyElement,
    cx: &mut Context<Root>,
) -> AnyElement {
    render_sized(spec, 480., false, Some(input), None, cx)
}

/// `size-sm_dvN-MP`: the same dialog 400 wide (Delete Channel).
pub(super) fn render_small(spec: Spec, cx: &mut Context<Root>) -> AnyElement {
    render_sized(spec, 400., false, None, None, cx)
}

/// The small dialog with its buttons one above the other across the width (Sync permissions?).
pub(super) fn render_small_stacked(spec: Spec, cx: &mut Context<Root>) -> AnyElement {
    render_sized(spec, 400., true, None, None, cx)
}

/// The dialog above a layer that is still open under it (User Settings).
pub(super) fn render_over_layers(spec: Spec, cx: &mut Context<Root>) -> AnyElement {
    render_sized_at(spec, 480., false, None, None, 4, cx)
}

/// The small dialog above a popout that is still open under it (the inbox's Mark All as Read question).
pub(super) fn render_small_over_popouts(spec: Spec, cx: &mut Context<Root>) -> AnyElement {
    render_sized_at(spec, 400., false, None, None, 4, cx)
}

fn render_sized(
    spec: Spec,
    width: f32,
    stacked: bool,
    input: Option<AnyElement>,
    pinned: Option<(f32, f32)>,
    cx: &mut Context<Root>,
) -> AnyElement {
    render_sized_at(spec, width, stacked, input, pinned, 1, cx)
}

fn render_sized_at(
    spec: Spec,
    width: f32,
    stacked: bool,
    input: Option<AnyElement>,
    pinned: Option<(f32, f32)>,
    priority: usize,
    cx: &mut Context<Root>,
) -> AnyElement {
    let fill = input.is_none();
    let voice_switch = spec.id == "voice-switch-channel";
    let pinned_bar = pinned.is_some() && input.is_some();
    let close: std::rc::Rc<Handler> = std::rc::Rc::new(spec.close);
    let close_button = close.clone();
    let close_scrim = close.clone();
    let buttons: Vec<AnyElement> = spec
        .buttons
        .into_iter()
        .map(|button| footer_button_sized(button, fill, cx))
        .map(|button| {
            if stacked {
                div().w_full().flex().child(button).into_any_element()
            } else {
                button
            }
        })
        .collect();
    // The scrim and the dialog fade in on the official spring (modules 462824 and 150591).
    let pose = super::motion::modal_pose(spec.id);
    let (scrim, body_opacity) = (pose.scrim, pose.opacity);
    let modal = div()
        .parity_with_border(spec.id, 1.)
        .id(spec.id)
        .opacity(body_opacity)
        .paint_scale(pose.scale, gpui::point(0.5, 0.5))
        .occlude()
        .w(px(width))
        .pt(px(8.))
        .pb(px(if pinned.is_some() && !pinned_bar {
            0.
        } else {
            16.
        }))
        .when_some(pinned, |this, (_, height)| {
            this.h(px(height)).overflow_hidden()
        })
        .rounded(px(12.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-high"))
        .shadow(vec![gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.24),
            offset: gpui::point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        }])
        .flex()
        .flex_col()
        .child(
            div()
                .when(voice_switch, |header| header.parity("voice-switch-header"))
                .pt(px(16.))
                .px(px(24.))
                .pb(px(8.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        // A title longer than the row wraps and grows it (stage-ann/stage-end-confirm).
                        .min_h(px(24.))
                        .flex()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(spec.title),
                        )
                        .child(
                            super::keyboard::inset_ring_when_focused(
                                div().id(SharedString::from(format!("{}-close", spec.id))),
                            )
                            .focusable()
                            .tab_index(0)
                            .mt(px(-16.))
                            .mr(px(-16.))
                            .size(px(40.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    close_button(this, window, cx);
                                    cx.notify();
                                },
                            )),
                        ),
                )
                .children(spec.subtitle.map(|subtitle| {
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-subtle"))
                        .child(subtitle)
                })),
        )
        .children(spec.body.map(|body| {
            if pinned.is_some() {
                div().flex_1().min_h(px(0.)).child(body)
            } else {
                div().pt(px(16.)).pl(px(24.)).pr(px(24.)).child(body)
            }
        }))
        // A dialog without buttons (Invite) ends with its body.
        .when(!buttons.is_empty(), |this| {
            let bar = div()
                // The small dialog's action bar follows its header directly.
                .when(width > 400. && !voice_switch, |this| this.mt(px(8.)))
                .when(voice_switch, |bar| bar.parity("voice-switch-actions"))
                .pt(px(16.))
                .px(px(24.))
                .pb(px(8.))
                .flex()
                .when(stacked, |this| this.flex_col())
                .gap(px(8.));
            this.child(match input {
                None => bar.children(buttons),
                Some(input) => bar
                    .items_center()
                    .child(div().flex_1().min_w(px(0.)).child(input))
                    .child(div().flex().gap(px(8.)).children(buttons)),
            })
        });
    deferred(
        div()
            .id(SharedString::from(format!("{}-layer", spec.id)))
            .occlude()
            .absolute()
            .inset_0()
            .bg(gpui::Rgba {
                a: f32::from(0xb8u8) / 255. * scrim,
                ..rgba(0x000000ff)
            })
            .flex()
            .when(pinned.is_none(), |this| this.items_center())
            .when_some(pinned, |this, (top, _)| this.items_start().pt(px(top)))
            .justify_center()
            .on_click(cx.listener(move |this, _, window, cx| {
                close_scrim(this, window, cx);
                cx.notify();
            }))
            .child(modal),
    )
    .with_priority(priority)
    .into_any_element()
}

/// What the fixed system bar does over the window's top 32px.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SystemBar {
    /// Nothing covers the title bar: the page's own bar takes the pointer.
    Hidden,
    /// A dialog or User Settings is open: the bar is a drag area with the window's buttons, above every scrim, so
    /// a press there moves the window and never reaches the scrim that closes the dialog.
    Drag,
    /// A full-window layer (Server or Channel Settings) is open: only the buttons take the pointer
    /// (`disablePointerEvents_3zSH5d`).
    ButtonsOnly,
}

/// `v5` of module 71855 as module 446269 mounts it: `fixed`, shown while a layer or a modal is open, without pointer
/// events but for its buttons while a layer is. A layer alone draws the same buttons itself, in the same place, so
/// the bar adds them only once a dialog's scrim covers the layer's.
pub(super) fn system_bar_mode(dialogs: usize, layer: bool) -> SystemBar {
    match (dialogs > 0, layer) {
        (false, _) => SystemBar::Hidden,
        (true, true) => SystemBar::ButtonsOnly,
        (true, false) => SystemBar::Drag,
    }
}

impl Root {
    /// Dialogs and User Settings, without the menus and popouts that `modal_open` also counts: those do not cover
    /// the title bar, and a press on it still closes them.
    pub(super) fn covering_dialogs(&self) -> usize {
        let popouts = [
            self.menu.is_some(),
            self.context.open.is_some(),
            self.snooze_menu.is_some(),
            self.friend_menu.is_some(),
            self.profile_menu.is_some(),
            self.file_menu.is_some(),
            self.audio_menu.is_some(),
            self.account_switcher_open(),
            self.search_filters.is_some(),
        ]
        .into_iter()
        .filter(|open| *open)
        .count();
        // The `modal_open` counts the settings layers as one; of them only User Settings and the profile editor are
        // dialogs over the window, Server and Channel Settings fill it.
        let dialog_layers = usize::from(self.settings.is_some() || self.profile_editor.is_some());
        let layers = usize::from(
            dialog_layers > 0 || self.server_settings.is_some() || self.channel_settings.is_some(),
        );
        let settings_confirm = usize::from(
            self.channel_settings
                .as_ref()
                .is_some_and(|open| open.confirm_delete)
                || self.overview_confirm_open()
                || self.webhook_confirm_open()
                || self.safety_confirm_open(),
        );
        (self.modal_open() + dialog_layers + settings_confirm).saturating_sub(popouts + layers)
    }

    /// The app's fixed system bar (`bar_3zSH5d systemBar_3zSH5d fixed_3zSH5d show_3zSH5d`, z-index 999999, the
    /// whole 32px top row with `-webkit-app-region: drag` in design/dom/settings/account-darker-cozy.json): over
    /// the scrim of every dialog, so pressing the title bar drags the window or works the button under it instead of
    /// closing the dialog. (The official bar's `onClick` also closes the Quick Switcher; here a press on the drag
    /// area goes to the window manager, as a drag.)
    pub(super) fn system_bar(&self) -> Option<AnyElement> {
        let dialog_over_layer = self.settings.is_some() || self.profile_editor.is_some();
        let layer = !dialog_over_layer
            && (self.server_settings.is_some() || self.channel_settings.is_some());
        let mode = system_bar_mode(self.covering_dialogs(), layer);
        if mode == SystemBar::Hidden {
            return None;
        }
        let bar = div()
            .id("system-bar")
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(32.))
            .flex()
            .justify_end()
            .when(mode == SystemBar::Drag, |bar| {
                bar.occlude()
                    .window_control_area(gpui::WindowControlArea::Drag)
            })
            .child(super::window_controls());
        // Above every dialog's scrim (priorities up to 4), under the motion driver.
        Some(deferred(bar).with_priority(5).into_any_element())
    }

    /// `message_LE5yVN`: a message as it appears in a confirmation, on `--background-base-low` with a 1px ring and a
    /// soft drop shadow, 10px above and below.
    pub(super) fn message_preview(&self, message: &Message) -> AnyElement {
        let avatar = self.images.get(&avatar_source(&message.author).0).cloned();
        div()
            .py(px(10.))
            .rounded(px(3.))
            .bg(color("--background-base-low"))
            .shadow(vec![
                gpui::BoxShadow {
                    color: gpui::hsla(225. / 360., 0.06, 0.13, 0.6),
                    offset: gpui::point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.2),
                    offset: gpui::point(px(0.), px(2.)),
                    blur_radius: px(10.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .child(
                div()
                    .relative()
                    .min_h(px(48.))
                    .pt(px(2.))
                    .pr(px(16.))
                    .pb(px(2.))
                    .pl(px(72.))
                    .child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(4.))
                            .size(px(40.))
                            .rounded_full()
                            .children(
                                avatar.map(|picture| img(picture).size(px(40.)).rounded_full()),
                            ),
                    )
                    .child(
                        div()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .line_height(px(22.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(message.author.display_name().to_owned()),
                            )
                            .child(
                                div()
                                    .ml(px(8.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(header_time(message.local_time(), chrono::Local::now())),
                            ),
                    )
                    .child(self.content(message)),
            )
            .into_any_element()
    }

    /// "Pin It. Pin It Good." (`bKMaZX`): asks before `PUT /channels/{id}/messages/pins/{id}`.
    pub(super) fn pin_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let message = self.confirm_pin.clone()?;
        let subtitle = match self.current_channel() {
            Some(channel) => format!(
                "Hey, just double checking that you want to pin this message to #{} for posterity and greatness?",
                channel.name
            ),
            None => dc_intl::tr("Hey, just double checking that you want to pin this message to the current channel for posterity and greatness?").to_owned(),
        };
        let target = (message.channel_id, message.id);
        Some(render(
            Spec {
                id: "pin-modal",
                title: dc_intl::tr("Pin It. Pin It Good.").into(),
                subtitle: Some(div().child(subtitle).into_any_element()),
                body: Some(self.message_preview(&message)),
                buttons: vec![
                    button(
                        "pin-cancel",
                        dc_intl::tr("Cancel"),
                        Tone::Secondary,
                        |this, _, _| {
                            this.confirm_pin = None;
                        },
                    ),
                    button(
                        "pin-confirm",
                        dc_intl::tr("Oh yeah. Pin it"),
                        Tone::Primary,
                        move |this, _, _| {
                            this.confirm_pin = None;
                            let _ = this.requests.send(Request::Pin {
                                channel: target.0,
                                message: target.1,
                                pin: true,
                            });
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.confirm_pin = None),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{SystemBar, system_bar_mode};

    /// Module 446269 shows the fixed bar while a modal or a layer is open; over a layer it takes no pointer but
    /// for its buttons. Here a layer alone keeps its own buttons, so the bar waits for a dialog over it.
    #[test]
    fn the_system_bar_covers_the_title_bar_while_a_dialog_is_open() {
        assert_eq!(system_bar_mode(0, false), SystemBar::Hidden);
        assert_eq!(system_bar_mode(1, false), SystemBar::Drag);
        assert_eq!(system_bar_mode(3, false), SystemBar::Drag);
        assert_eq!(system_bar_mode(0, true), SystemBar::Hidden);
        assert_eq!(system_bar_mode(1, true), SystemBar::ButtonsOnly);
    }
}
