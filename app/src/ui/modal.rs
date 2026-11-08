//! The design system modal (`container_dvN-MP`, size md): 480 wide on `--background-surface-high`, a 20px semibold
//! title with a close button, a muted subtitle, an optional body, and a footer of two equal buttons. Measured in
//! `design/dom/modals/leaving-discord-darker-cozy.json` and `pin-confirm-darker-cozy.json`; the scrim is black at
//! 0.72 (`scrim_od1dl1`).

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
}

pub(super) type Handler = Box<dyn Fn(&mut Root, &mut Window, &mut Context<Root>)>;

pub(super) struct Button {
    pub id: &'static str,
    pub label: SharedString,
    pub tone: Tone,
    pub run: Handler,
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

fn footer_button(spec: Button, cx: &mut Context<Root>) -> AnyElement {
    let (background, border, text) = match spec.tone {
        Tone::Secondary => (
            color("--control-secondary-background-default"),
            color("--border-muted"),
            color("--control-secondary-text-default"),
        ),
        Tone::Primary | Tone::PrimaryDisabled => (
            color("--control-primary-background-default"),
            color("--control-primary-border-default"),
            color("--white"),
        ),
        Tone::Critical => (
            color("--control-critical-primary-background-default"),
            color("--control-critical-primary-border-default"),
            color("--white"),
        ),
    };
    let run = spec.run;
    let disabled = spec.tone == Tone::PrimaryDisabled;
    div()
        .id(spec.id)
        .when(disabled, |this| this.opacity(0.5))
        .flex_1()
        .h(px(40.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(background)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(text)
        .child(spec.label)
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            if !disabled {
                run(this, window, cx);
            }
            cx.notify();
        }))
        .into_any_element()
}

pub(super) fn render(spec: Spec, cx: &mut Context<Root>) -> AnyElement {
    render_sized(spec, 480., cx)
}

/// `size-sm_dvN-MP`: the same dialog 400 wide (Delete Channel).
pub(super) fn render_small(spec: Spec, cx: &mut Context<Root>) -> AnyElement {
    render_sized(spec, 400., cx)
}

fn render_sized(spec: Spec, width: f32, cx: &mut Context<Root>) -> AnyElement {
    let close: std::rc::Rc<Handler> = std::rc::Rc::new(spec.close);
    let close_button = close.clone();
    let close_scrim = close.clone();
    let buttons: Vec<AnyElement> = spec
        .buttons
        .into_iter()
        .map(|button| footer_button(button, cx))
        .collect();
    let modal = div()
        .parity_with_border(spec.id, 1.)
        .id(spec.id)
        .occlude()
        .w(px(width))
        .pt(px(8.))
        .pb(px(16.))
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
                .pt(px(16.))
                .px(px(24.))
                .pb(px(8.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .h(px(24.))
                        .flex()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(spec.title),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("{}-close", spec.id)))
                                .mt(px(-16.))
                                .mr(px(-16.))
                                .size(px(40.))
                                .rounded(px(8.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    close_button(this, window, cx);
                                    cx.notify();
                                })),
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
        .children(
            spec.body
                .map(|body| div().pt(px(16.)).pl(px(24.)).pr(px(24.)).child(body)),
        )
        // A dialog without buttons (Invite) ends with its body.
        .when(!buttons.is_empty(), |this| {
            this.child(
                div()
                    .mt(px(8.))
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .flex()
                    .gap(px(8.))
                    .children(buttons),
            )
        });
    deferred(
        div()
            .id(SharedString::from(format!("{}-layer", spec.id)))
            .occlude()
            .absolute()
            .inset_0()
            .bg(rgba(0x000000b8))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, _, window, cx| {
                close_scrim(this, window, cx);
                cx.notify();
            }))
            .child(modal),
    )
    .with_priority(1)
    .into_any_element()
}

impl Root {
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
            None => "Hey, just double checking that you want to pin this message to the current channel for posterity and greatness?".to_owned(),
        };
        let target = (message.channel_id, message.id);
        Some(render(
            Spec {
                id: "pin-modal",
                title: "Pin It. Pin It Good.".into(),
                subtitle: Some(div().child(subtitle).into_any_element()),
                body: Some(self.message_preview(&message)),
                buttons: vec![
                    button("pin-cancel", "Cancel", Tone::Secondary, |this, _, _| {
                        this.confirm_pin = None;
                    }),
                    button(
                        "pin-confirm",
                        "Oh yeah. Pin it",
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
