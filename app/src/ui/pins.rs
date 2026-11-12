//! Pinned Messages (`messagesPopoutWrap_6bgemo`): a 420px popout under the header's pin button. Pins come from
//! `GET /channels/{id}/messages/pins?limit=25` (`{items: [{pinned_at, message}], has_more}`); each shows as a card
//! with Jump and Unpin on hover; none shows the empty illustration and the protip. Unpinning asks first ("Unpin
//! Message", module 145530) unless shift is held. Measured in `design/dom/popouts/pins*-darker-cozy.json`.

use super::{Root, avatar_source, color, header_time, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, Message};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, deferred, div, img,
    prelude::*, px,
};

const EMPTY_IMAGE: &str = "https://discordapp.com/assets/1772d54cb566d95b.svg";

fn pins_key(channel: Id) -> String {
    format!("pins:{channel}")
}

impl Root {
    pub(super) fn toggle_pins(&mut self, anchor: &'static str, cx: &mut Context<Self>) {
        if self.pins_open.take().is_some() {
            cx.notify();
            return;
        }
        let Some(channel) = self.channel else {
            return;
        };
        self.pins_open = Some(anchor);
        self.ack_open_pins();
        let _ = self.requests.send(Request::Fetch {
            key: pins_key(channel),
            route: format!("/channels/{channel}/messages/pins?limit=25"),
        });
        self.request_image("pins-empty".to_owned(), EMPTY_IMAGE.to_owned());
        cx.notify();
    }

    /// Module 945830: while the popout is open and the channel has unread pins, they are acknowledged
    /// (`ackPins`: `POST /channels/{id}/pins/ack`; the local time moves at once).
    pub(super) fn ack_open_pins(&mut self) {
        let Some(channel) = self.channel.filter(|_| self.pins_open.is_some()) else {
            return;
        };
        if self.stores.ack_pins(channel) {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::POST,
                route: format!("/channels/{channel}/pins/ack"),
                body: None,
            });
        }
    }

    fn pinned_messages(&self) -> Vec<Message> {
        let Some(channel) = self.channel else {
            return Vec::new();
        };
        self.fetched
            .get(&pins_key(channel))
            .and_then(|body| body["items"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| Message::from_value(&item["message"]))
            .collect()
    }

    fn unpin(&mut self, message: &Message, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Pin {
            channel: message.channel_id,
            message: message.id,
            pin: false,
        });
        // Drop it from the open list at once; the next fetch agrees.
        if let Some(items) = self
            .fetched
            .get_mut(&pins_key(message.channel_id))
            .and_then(|body| body["items"].as_array_mut())
        {
            let id = message.id.to_string();
            items.retain(|item| item["message"]["id"].as_str() != Some(id.as_str()));
        }
        cx.notify();
    }

    fn pinned_card(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let avatar = self.images.get(&avatar_source(&message.author).0).cloned();
        let group = SharedString::from(format!("pinned-{}", message.id));
        let jump = message.clone();
        let unpin = message.clone();
        div()
            .id(SharedString::from(format!("pinned-card-{}", message.id)))
            .group(group.clone())
            .relative()
            .mb(px(8.))
            .mr(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .overflow_hidden()
            .child(
                div()
                    .relative()
                    .ml(px(-4.))
                    .pt(px(16.))
                    .pb(px(16.))
                    .pl(px(72.))
                    .pr(px(16.))
                    .child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(18.))
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
                                    .ml(px(4.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(header_time(message.local_time(), chrono::Local::now())),
                            ),
                    )
                    .when(!message.content.is_empty(), |this| {
                        this.child(self.content(message))
                    })
                    .children(self.forwarded_block(message, cx))
                    .children(self.accessories(message, cx)),
            )
            // `actionButtons_6bgemo`: Jump and Unpin, top right, while hovered.
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .flex()
                    .gap(px(8.))
                    .invisible()
                    .group_hover(group, |style| style.visible())
                    .child(
                        div()
                            .id(SharedString::from(format!("pinned-jump-{}", message.id)))
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pins_open = None;
                                this.jump_to_message(&jump, cx);
                            }))
                            .child(dc_intl::tr("Jump")),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("pinned-unpin-{}", message.id)))
                            .size(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                if event.modifiers().shift {
                                    this.unpin(&unpin, cx);
                                } else {
                                    this.confirm_unpin = Some(unpin.clone());
                                    cx.notify();
                                }
                            }))
                            .child(icon("modal_close", 16., "--control-secondary-text-default")),
                    ),
            )
            .into_any_element()
    }

    /// Opens the message's channel and brings the message into view, loading the page around it when it is not
    /// loaded (`jumpToMessage` with `flash: true`).
    pub(super) fn jump_to_message(&mut self, message: &Message, cx: &mut Context<Self>) {
        self.jump_to_message_id(message.channel_id, message.id, true, None);
        cx.notify();
    }

    pub(super) fn pins_popout(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let anchor_key = self.pins_open?;
        let button = self.tip_anchors.borrow().get(anchor_key).copied()?;
        let pins = self.pinned_messages();
        let dm = self.current_dm().is_some();
        let viewport = window.viewport_size();
        let right = (f32::from(viewport.width) - f32::from(button.right())).max(8.);
        let top = f32::from(button.bottom()) + 8.;
        let body: AnyElement = if pins.is_empty() {
            let picture = self.images.get("pins-empty").cloned();
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .pt(px(8.))
                        .pl(px(8.))
                        .child(
                            div()
                                .mt(px(8.))
                                .mb(px(32.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .child(
                                    div()
                                        .size(px(120.))
                                        .children(picture.map(|picture| img(picture).size(px(120.)))),
                                )
                                .child(
                                    div()
                                        .mt(px(20.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .text_center()
                                        .child(if dm {
                                            dc_intl::tr("This direct message doesn't have\nany pinned messages... yet.")
                                        } else {
                                            dc_intl::tr("This channel doesn't have any\npinned messages... yet.")
                                        }),
                                ),
                        ),
                )
                .child(
                    div()
                        .p(px(16.))
                        .bg(color("--background-mod-subtle"))
                        .text_center()
                        .child(
                            div()
                                .py(px(10.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-feedback-positive"))
                                        .child(dc_intl::tr("Protip:")),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-default"))
                                        .child(if dm {
                                            dc_intl::tr("You can pin a message from its context menu.")
                                        } else {
                                            dc_intl::tr("Users with the 'Pin Messages' permission can pin a message from its context menu.")
                                        }),
                                ),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .id("pins-list")
                .max_h(px(f32::from(viewport.height) - top - 80.))
                .overflow_y_scroll()
                .pt(px(8.))
                .pl(px(8.))
                // `scrollbarGutterStable`: the thin scrollbar's 8px are kept whether or not it shows.
                .pr(px(8.))
                .children(pins.iter().map(|message| self.pinned_card(message, cx)))
                .into_any_element()
        };
        Some(
            deferred(
                div()
                    .parity("pins-popout")
                    .id("pins-popout")
                    .occlude()
                    .absolute()
                    .top(px(top))
                    .right(px(right))
                    .w(px(420.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![
                        gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 1., 0.08),
                            offset: gpui::point(px(0.), px(0.)),
                            blur_radius: px(0.),
                            spread_radius: px(1.),
                            inset: false,
                        },
                        gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.24),
                            offset: gpui::point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        },
                    ])
                    .overflow_hidden()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if this.press_outside(super::keyboard::Layer::Pins, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .p(px(16.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("pin", 24., "--interactive-text-default"))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::tr("Pinned Messages")),
                            ),
                    )
                    .child(body),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// "Unpin Message" (`CFF2vL`): the message, the shift protip (`oCVB3Y`), Cancel and a critical
    /// "Remove it please!".
    pub(super) fn unpin_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let message = self.confirm_unpin.clone()?;
        let tip_spans = dc_markdown::parse(
            "You can hold down shift when clicking **unpin message** to bypass this confirmation entirely.",
        );
        let tip_pieces = tip_spans
            .iter()
            .flat_map(|block| super::block_spans(block).to_vec())
            .map(|span| self.piece(&span, color("--text-default").into(), 14., 400., 16.))
            .collect();
        let body = div()
            .flex()
            .flex_col()
            .child(self.message_preview(&message))
            .child(
                div()
                    .mt(px(20.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-feedback-positive"))
                            .child(dc_intl::tr("Protip:")),
                    )
                    .child(crate::inline::inline(tip_pieces, px(16.))),
            )
            .into_any_element();
        let target = message.clone();
        Some(super::modal::render(
            super::modal::Spec {
                id: "unpin-modal",
                title: dc_intl::tr("Unpin Message").into(),
                subtitle: Some(
                    div()
                        .child(dc_intl::tr(
                            "You sure you want to remove this pinned message?",
                        ))
                        .into_any_element(),
                ),
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "unpin-cancel",
                        dc_intl::tr("Cancel"),
                        super::modal::Tone::Secondary,
                        |this, _, _| this.confirm_unpin = None,
                    ),
                    super::modal::button(
                        "unpin-confirm",
                        dc_intl::tr("Remove it please!"),
                        super::modal::Tone::Critical,
                        move |this, _, cx| {
                            this.confirm_unpin = None;
                            this.unpin(&target, cx);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.confirm_unpin = None),
            },
            cx,
        ))
    }
}
