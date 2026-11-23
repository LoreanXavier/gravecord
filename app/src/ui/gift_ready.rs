//! Gift Ready / post-purchase gift delivery modal (module 998035).
//!
//! Renders every state of `ec` / `er` / `ed` of module 998035:
//! - Single-recipient and bulk (`giftCount > 1` with `{numMembers} of {maxMemberLimit}`) friend selector (`Gift Recipient`,
//!   `Type your friend’s name`, `Send Now`),
//! - Sent confirmation (`Your gift has been sent to`), delivery error (`Uh-oh! Something went wrong!`), and ready state
//!   (`Gift ready for delivery`),
//! - All 4 generosity summaries (month/year × sent/unsent) and lost-delivery fallback (`qB8aya`),
//! - Private Gift Link copy bar (`Private Gift Link`, Streamer Mode redaction `Gift code hidden. Use copy button.`,
//!   `Copy` / `Copied!` / `Failed`), 48-hour expiry disclaimer (`QWKUpn`), DM delivery hint / error (`8/N3v3` / `jo5Vbl`),
//!   and `Next` progression button (`PDTjLN`).

use super::discover::text;
use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{AnyElement, Context, IntoElement, SharedString, deferred, div, prelude::*, px, rgba};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GiftInterval {
    Month,
    Year,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub enum CopyMode {
    #[default]
    Default,
    Success,
    Error,
}

/// Open state for the Gift Ready delivery modal (module 998035).
#[derive(Clone, Debug)]
pub struct GiftReadyModal {
    pub gift_code: String,
    pub sku_name: String,
    pub interval: GiftInterval,
    pub interval_count: u64,
    pub gift_count: usize,
    pub selected_recipients: Vec<String>,
    pub sent_to_name: Option<String>,
    pub delivery_error: bool,
    pub message_send_error: bool,
    pub copy_mode: CopyMode,
    pub show_next_cta: bool,
    pub picker_open: bool,
}

impl GiftReadyModal {
    pub fn demo(mode: &str) -> Self {
        match mode.trim() {
            "sent" => Self {
                gift_code: "AbCdEfGh1234".to_owned(),
                sku_name: "Nitro".to_owned(),
                interval: GiftInterval::Month,
                interval_count: 1,
                gift_count: 1,
                selected_recipients: Vec::new(),
                sent_to_name: Some("lorean xavier".to_owned()),
                delivery_error: false,
                message_send_error: false,
                copy_mode: CopyMode::Default,
                show_next_cta: true,
                picker_open: false,
            },
            "bulk" => Self {
                gift_code: "AbCdEfGh1234".to_owned(),
                sku_name: "Nitro".to_owned(),
                interval: GiftInterval::Year,
                interval_count: 1,
                gift_count: 3,
                selected_recipients: vec!["Wren".to_owned()],
                sent_to_name: None,
                delivery_error: false,
                message_send_error: false,
                copy_mode: CopyMode::Default,
                show_next_cta: false,
                picker_open: false,
            },
            "error" => Self {
                gift_code: "AbCdEfGh1234".to_owned(),
                sku_name: "Nitro".to_owned(),
                interval: GiftInterval::Month,
                interval_count: 1,
                gift_count: 1,
                selected_recipients: Vec::new(),
                sent_to_name: None,
                delivery_error: true,
                message_send_error: true,
                copy_mode: CopyMode::Error,
                show_next_cta: false,
                picker_open: false,
            },
            _ => Self {
                gift_code: "AbCdEfGh1234".to_owned(),
                sku_name: "Nitro".to_owned(),
                interval: GiftInterval::Month,
                interval_count: 1,
                gift_count: 1,
                selected_recipients: Vec::new(),
                sent_to_name: None,
                delivery_error: false,
                message_send_error: false,
                copy_mode: CopyMode::Default,
                show_next_cta: false,
                picker_open: false,
            },
        }
    }
}

/// Heading text (`ec` of module 998035).
pub fn gift_ready_heading(modal: &GiftReadyModal) -> &'static str {
    if modal.sent_to_name.is_some() && !modal.delivery_error {
        dc_intl::t("zOmK9N")
    } else if modal.delivery_error {
        dc_intl::t("d1lrmU")
    } else {
        dc_intl::t("/s1xR7")
    }
}

/// Generosity body copy (`$` of module 998035: `qB8aya`, `4ZJ+7Z`, `P+z55d`, `p0pZXP`, `bXqk3o`).
pub fn gift_ready_body(modal: &GiftReadyModal) -> String {
    if modal.delivery_error {
        return dc_intl::t("qB8aya").to_owned();
    }
    let sent = modal.sent_to_name.is_some();
    let key = match (modal.interval, sent) {
        (GiftInterval::Month, true) => "4ZJ+7Z",
        (GiftInterval::Month, false) => "P+z55d",
        (GiftInterval::Year, true) => "p0pZXP",
        (GiftInterval::Year, false) => "bXqk3o",
    };
    dc_intl::format(
        key,
        &[
            ("skuName", Arg::Str(Cow::Borrowed(&modal.sku_name))),
            ("intervalCount", Arg::Num(modal.interval_count as f64)),
        ],
    )
}

/// Copy button label (`J` of module 998035: `XVvPjU`, `i4GM3L`, `OpuAlK`).
pub fn gift_copy_label(mode: CopyMode) -> &'static str {
    match mode {
        CopyMode::Success => dc_intl::t("XVvPjU"),
        CopyMode::Error => dc_intl::t("i4GM3L"),
        CopyMode::Default => dc_intl::t("OpuAlK"),
    }
}

impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn script_gift_ready(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        self.gift_ready_modal = Some(GiftReadyModal::demo(arg));
        cx.notify();
        format!("gift-ready: mode={}", arg.trim())
    }

    /// Renders the Gift Ready post-purchase delivery modal (module 998035).
    pub(super) fn gift_ready_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.gift_ready_modal.as_ref()?;
        let heading = gift_ready_heading(modal);
        let body = gift_ready_body(modal);
        let copy_label = gift_copy_label(modal.copy_mode);
        let streamer_on = self.streamer_hides_personal();
        let gift_url = format!("https://discord.gift/{}", modal.gift_code);
        let link_display = if streamer_on {
            dc_intl::t("0RLn47").to_owned()
        } else {
            gift_url.clone()
        };
        let is_bulk = modal.gift_count > 1;
        let bulk_counter = dc_intl::format(
            "H55rqz",
            &[
                (
                    "numMembers",
                    Arg::Num(modal.selected_recipients.len() as f64),
                ),
                ("maxMemberLimit", Arg::Num(modal.gift_count as f64)),
            ],
        );
        let can_send = !modal.selected_recipients.is_empty()
            && modal.selected_recipients.len() <= modal.gift_count;
        let friends: Vec<String> = self
            .stores
            .relationships
            .iter()
            .filter(|&(_, &kind)| kind == super::friends::RELATIONSHIP_FRIEND)
            .filter_map(|(id, _)| {
                self.stores
                    .users
                    .get(id)
                    .filter(|u| !u.bot)
                    .map(|u| u.username.clone())
            })
            .collect();

        let private_link_box = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(text(12., 16., 700., "--text-subtle").child(dc_intl::t("/dG4NA")))
            .child(
                div()
                    .w_full()
                    .h(px(40.))
                    .pl(px(12.))
                    .pr(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        text(
                            13.,
                            18.,
                            400.,
                            if streamer_on {
                                "--text-muted"
                            } else {
                                "--text-default"
                            },
                        )
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(link_display),
                    )
                    .child(
                        div()
                            .id("gift-ready-copy")
                            .flex_none()
                            .h(px(28.))
                            .px(px(12.))
                            .rounded(px(6.))
                            .bg(color(match modal.copy_mode {
                                CopyMode::Success => "--status-positive",
                                CopyMode::Error => "--text-feedback-critical",
                                CopyMode::Default => "--control-primary-background-default",
                            }))
                            .hover(|s| s.opacity(0.9))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    gift_url.clone(),
                                ));
                                if let Some(m) = this.gift_ready_modal.as_mut() {
                                    m.copy_mode = CopyMode::Success;
                                }
                                cx.notify();
                            }))
                            .child(text(13., 16., 600., "--text-overlay-light").child(copy_label)),
                    ),
            )
            .child(text(12., 16., 400., "--text-muted").child(dc_intl::t("QWKUpn")));

        let recipient_picker = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(text(12., 16., 700., "--text-subtle").child(dc_intl::t("MJw05f")))
                    .when(is_bulk, |row| {
                        row.child(text(12., 16., 600., "--text-strong").child(bulk_counter))
                    }),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("gift-recipient-select")
                            .relative()
                            .flex_1()
                            .min_w(px(0.))
                            .h(px(38.))
                            .px(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lowest"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(m) = this.gift_ready_modal.as_mut() {
                                    m.picker_open = !m.picker_open;
                                }
                                cx.notify();
                            }))
                            .child(
                                text(
                                    13.,
                                    18.,
                                    400.,
                                    if modal.selected_recipients.is_empty() {
                                        "--text-muted"
                                    } else {
                                        "--text-default"
                                    },
                                )
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(
                                    if modal.selected_recipients.is_empty() {
                                        dc_intl::t("J019jZ").to_owned()
                                    } else {
                                        modal.selected_recipients.join(", ")
                                    },
                                ),
                            )
                            .child(icon("automod_chevron_down", 14., "--text-muted"))
                            .child(self.anchor_probe("gift-recipient-select"))
                            .when(modal.picker_open && !friends.is_empty(), |trigger| {
                                trigger.child(
                                    deferred(
                                        div()
                                            .id("gift-friend-list")
                                            .occlude()
                                            .absolute()
                                            .left(px(-1.))
                                            .right(px(-1.))
                                            .top(px(41.))
                                            .p(px(4.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-surface-high"))
                                            .shadow(vec![super::shadow_high()])
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            // `maxOptionsVisible` 5 (select of module 890497): five 30px rows, 2px gaps, 4px padding.
                                            .max_h(px(166.))
                                            .overflow_y_scroll()
                                            .on_click(|_, _, cx| cx.stop_propagation())
                                            .on_mouse_down_out(cx.listener(
                                                |this, event: &gpui::MouseDownEvent, _, cx| {
                                                    let on_trigger = this
                                                        .tip_anchors
                                                        .borrow()
                                                        .get("gift-recipient-select")
                                                        .is_some_and(|field| {
                                                            field.contains(&event.position)
                                                        });
                                                    if !on_trigger
                                                        && let Some(m) =
                                                            this.gift_ready_modal.as_mut()
                                                        && m.picker_open
                                                    {
                                                        m.picker_open = false;
                                                        cx.notify();
                                                    }
                                                },
                                            ))
                                            .children(friends.into_iter().map(|name| {
                                                let picked =
                                                    modal.selected_recipients.contains(&name);
                                                let n_click = name.clone();
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "gift-friend-{name}"
                                                    )))
                                                    .flex_none()
                                                    .px(px(8.))
                                                    .py(px(6.))
                                                    .rounded(px(4.))
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Some(m) =
                                                            this.gift_ready_modal.as_mut()
                                                        {
                                                            if m.gift_count == 1 {
                                                                m.selected_recipients =
                                                                    vec![n_click.clone()];
                                                                m.picker_open = false;
                                                            } else if let Some(pos) = m
                                                                .selected_recipients
                                                                .iter()
                                                                .position(|x| x == &n_click)
                                                            {
                                                                m.selected_recipients.remove(pos);
                                                            } else if m.selected_recipients.len()
                                                                < m.gift_count
                                                            {
                                                                m.selected_recipients
                                                                    .push(n_click.clone());
                                                            }
                                                        }
                                                        cx.notify();
                                                    }))
                                                    .child(
                                                        text(13., 18., 500., "--text-default")
                                                            .child(name),
                                                    )
                                                    .when(picked, |r| {
                                                        r.child(icon(
                                                            "checkbox_check",
                                                            14.,
                                                            "--text-brand",
                                                        ))
                                                    })
                                            })),
                                    )
                                    .with_priority(99),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("gift-ready-send-now")
                            .flex_none()
                            .h(px(38.))
                            .px(px(14.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .when(can_send, |el| {
                                el.hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.gift_ready_modal.as_mut() {
                                            m.sent_to_name = m.selected_recipients.first().cloned();
                                            m.delivery_error = false;
                                            m.picker_open = false;
                                        }
                                        cx.notify();
                                    }))
                            })
                            .when(!can_send, |el| el.opacity(0.5))
                            .flex()
                            .items_center()
                            .child(
                                text(14., 18., 600., "--control-primary-text-default")
                                    .child(dc_intl::t("+EgwQn")),
                            ),
                    ),
            )
            .child(
                text(
                    12.,
                    16.,
                    400.,
                    if modal.message_send_error {
                        "--text-feedback-critical"
                    } else {
                        "--text-muted"
                    },
                )
                .child(dc_intl::t(if modal.message_send_error {
                    "jo5Vbl"
                } else {
                    "8/N3v3"
                })),
            );

        let card = div()
            .id("gift-ready-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .w(px(460.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .flex()
            .flex_col()
            .child(
                div()
                    .id("gift-ready-close")
                    .absolute()
                    .top(px(14.))
                    .right(px(14.))
                    .size(px(28.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.gift_ready_modal = None;
                        cx.notify();
                    }))
                    .child(icon("close", 16., "--interactive-text-default")),
            )
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .size(px(56.))
                            .rounded_full()
                            .bg(rgba(0x5865f226))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("home_nitro", 28., "--text-brand")),
                    )
                    .child(
                        text(20., 26., 700., "--text-strong")
                            .text_center()
                            .child(heading),
                    )
                    .when_some(modal.sent_to_name.clone(), |col, recipient| {
                        col.child(
                            div()
                                .px(px(14.))
                                .py(px(6.))
                                .rounded_full()
                                .bg(color("--background-surface-high"))
                                .child(text(15., 20., 600., "--text-strong").child(recipient)),
                        )
                    })
                    .child(
                        text(14., 20., 400., "--text-default")
                            .text_center()
                            .child(body),
                    )
                    .when(
                        modal.sent_to_name.is_none() && !modal.delivery_error,
                        |col| col.child(recipient_picker),
                    )
                    .when(modal.sent_to_name.is_none(), |col| {
                        col.child(div().w_full().h(px(1.)).bg(color("--border-subtle")))
                            .child(private_link_box)
                    }),
            )
            .when(modal.show_next_cta, |card| {
                card.child(
                    div()
                        .px(px(24.))
                        .py(px(14.))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lower"))
                        .rounded_b(px(12.))
                        .child(
                            div()
                                .id("gift-ready-next")
                                .w_full()
                                .h(px(38.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.gift_ready_modal = None;
                                    cx.notify();
                                }))
                                .child(
                                    text(14., 18., 600., "--control-primary-text-default")
                                        .child(dc_intl::t("PDTjLN")),
                                ),
                        ),
                )
            });

        Some(
            deferred(
                div()
                    .id("gift-ready-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.gift_ready_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

impl Root {
    pub(super) fn close_gift_friend_select(&mut self) -> bool {
        if let Some(m) = self.gift_ready_modal.as_mut()
            && m.picker_open
        {
            m.picker_open = false;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gift_ready_copy_matches_all_delivery_states() {
        let ready = GiftReadyModal::demo("ready");
        assert_eq!(gift_ready_heading(&ready), "Gift ready for delivery");
        assert!(gift_ready_body(&ready).contains("Send this gift to a friend"));

        let sent = GiftReadyModal::demo("sent");
        assert_eq!(gift_ready_heading(&sent), "Your gift has been sent to");
        assert!(gift_ready_body(&sent).contains("You've given"));

        let err = GiftReadyModal::demo("error");
        assert_eq!(gift_ready_heading(&err), "Uh-oh! Something went wrong!");
        assert!(gift_ready_body(&err).contains("Wumpus got lost"));

        assert_eq!(gift_copy_label(CopyMode::Default), "Copy");
        assert_eq!(gift_copy_label(CopyMode::Success), "Copied!");
        assert_eq!(gift_copy_label(CopyMode::Error), "Failed");
    }
}
