//! Stage channels (type 13): the sidebar row's topic line, the Stage view with its waiting and live states
//! (modules 895903 and 859894 for the waiting card), and the Start the Stage modal (module 128347).
//!
//! Nothing here has been seen in the official client yet (the fixture guild has no Stage channel): the strings, the
//! card (stylesheet class `container__B7Y3X`) and the empty state (`headerContainer_YMXX4L`) are read from the
//! modules; every other value is assumed and said so where it is used. The harness never starts, edits or ends a
//! Stage.

use super::channel_status::{HEADER_HEIGHT, HEADER_WIDTH, MODAL_WIDTH, header_gradient};
use super::{Root, color, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
#[cfg(not(feature = "capture"))]
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Id, permissions};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, Rgba, Subscription, Window,
    deferred, div, img, prelude::*, px, rgba,
};
#[cfg(not(feature = "capture"))]
use serde_json::json;
use std::cell::Cell;

/// `ChannelKind::Other(13)`: `GUILD_STAGE_VOICE`.
const STAGE: ChannelKind = ChannelKind::Other(13);
/// Module 973324: the modal's graphic.
const GRAPHIC: &str = "https://cdn.discordapp.com/assets/content/ef7fa86f8c519fe9764867c1ba473ee1939292fddb2c897885aab732e3bd31d2.svg";
/// `maxLength` of the topic field (module 128347).
const MAX_TOPIC: usize = 120;
/// `privacy_level` 2: `GUILD_ONLY`, what the modal starts with.
#[cfg(not(feature = "capture"))]
const GUILD_ONLY: u64 = 2;
/// The key the answer of a start, edit or end comes back under.
pub(super) const STAGE_KEY: &str = "stage-instance";

/// The Start the Stage modal's state (`V` of module 128347).
pub(super) struct StartStage {
    channel: Id,
    field: Entity<Editor>,
    _events: Subscription,
    /// `sendStartNotification`: Notify @everyone.
    notify: bool,
    editing: bool,
    busy: bool,
    error: Option<String>,
    focus_pending: Cell<bool>,
}

impl Root {
    pub(super) fn is_stage_channel(channel: &Channel) -> bool {
        channel.kind == STAGE
    }

    /// The open channel is a Stage channel: its view replaces the chat column.
    pub(super) fn stage_view_open(&self) -> bool {
        self.current_channel().is_some_and(Self::is_stage_channel)
    }

    /// `E.QY`: the right to start and edit a Stage (Stage moderator). Assumed to be Manage Channels, Mute Members and
    /// Move Members together; module 233993's value was not read.
    fn can_moderate_stage(&self, channel: &Channel) -> bool {
        const MUTE_MEMBERS: u64 = 1 << 22;
        const MOVE_MEMBERS: u64 = 1 << 24;
        [permissions::MANAGE_CHANNELS, MUTE_MEMBERS, MOVE_MEMBERS]
            .into_iter()
            .all(|permission| self.can_in_channel(channel, permission))
    }

    /// The Stage view: the channel header over the waiting card list, or the live Stage.
    pub(super) fn stage_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(channel) = self.current_channel() else {
            return div().into_any_element();
        };
        let instance = self.stores.stage_instances.get_by_channel(channel.id);
        let target = channel.id;
        let moderator = self.can_moderate_stage(channel);
        let body = match instance {
            None => {
                // `headerContainer_YMXX4L`: centred, 24px above and below; the title is `heading-xxl/medium` and the
                // subtitle `heading-lg/medium`, 8px under it. Assumed sizes: 32/40 and 20/24.
                let header = div()
                    .my(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    .child(
                        div()
                            .text_size(px(32.))
                            .line_height(px(40.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Waiting for the Stage to start"),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child("What will you be talking about today?"),
                    );
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(header)
                    .children(moderator.then(|| {
                        div().mt(px(8.)).child(self.stage_card(
                            "stage-start-card",
                            "stage_channel",
                            "--green-360",
                            "Start the Stage",
                            "Pick a topic to start the conversation.",
                            cx.listener(move |this, _, _, cx| this.open_start_stage(target, cx)),
                        ))
                    }))
                    .into_any_element()
            }
            Some(instance) => {
                // The live Stage. Assumed: the topic as the title with the "Stage Live" label (`qGRagm`) over it;
                // the speakers' and the audience's layout (module 396655's neighbours) is not read.
                let topic = instance.topic.clone();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .bg(color("--green-360"))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child("Stage Live"),
                    )
                    .child(
                        div()
                            .mt(px(16.))
                            .max_w(px(640.))
                            .text_center()
                            .text_size(px(32.))
                            .line_height(px(40.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(topic),
                    )
                    .children(moderator.then(|| {
                        div().mt(px(24.)).child(self.stage_card(
                            "stage-edit-card",
                            "stage_channel",
                            "--green-360",
                            "Change the topic",
                            "Update what the Stage is about.",
                            cx.listener(move |this, _, _, cx| this.open_start_stage(target, cx)),
                        ))
                    }))
                    .into_any_element()
            }
        };
        div()
            .parity("stage-view")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(self.header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(color("--background-base-lower"))
                    .child(body),
            )
            .into_any_element()
    }

    /// `47868` with `container__B7Y3X`: 400 wide, 16px padded, a 12px radius on `--background-base-lower` (hover
    /// `--background-base-low`); the icon is a 6px padded circle in the card's colour, the text 16px in, a caret at
    /// the end.
    fn stage_card(
        &self,
        id: &'static str,
        glyph: &'static str,
        tint: &'static str,
        title: &'static str,
        description: &'static str,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .w(px(400.))
            .p(px(16.))
            .rounded(px(12.))
            .bg(color("--background-base-lower"))
            .hover(|style| style.bg(color("--background-base-low")))
            .cursor_pointer()
            .flex()
            .items_center()
            .on_click(on_click)
            .child(
                div()
                    .flex_none()
                    .p(px(6.))
                    .rounded_full()
                    .bg(color(tint))
                    .child(icon(glyph, 20., "--interactive-text-active")),
            )
            .child(
                div()
                    .ml(px(16.))
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-strong"))
                            .child(description),
                    ),
            )
            .child(icon("chevron_right_20", 16., "--interactive-text-default"))
            .into_any_element()
    }

    /// `tQ` of module 993838: the Start the Stage modal, or Change the topic when the Stage is live. For the menu
    /// and the view.
    pub(in crate::ui) fn open_start_stage(&mut self, channel: Id, cx: &mut Context<Self>) {
        let editing = self.stores.stage_instances.is_live(channel);
        let topic = self
            .stores
            .stage_instances
            .get_by_channel(channel)
            .map(|instance| instance.topic.clone())
            .unwrap_or_default();
        let field = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_reports_all_edits(true);
            // `ZwWruY`.
            editor.set_placeholder("The debate is over: diet is better than regular".to_owned());
            editor.set_text(&topic);
            editor
        });
        let events = cx.subscribe(&field, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(_) => this.submit_stage(cx),
                EditorEvent::Cancel => this.start_stage = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        self.request_image(GRAPHIC.to_owned(), GRAPHIC.to_owned());
        self.start_stage = Some(StartStage {
            channel,
            field,
            _events: events,
            // `ei && ea`: only a new Stage notifies, and not in a server of over fifty thousand; assumed on.
            notify: !editing,
            editing,
            busy: false,
            error: None,
            focus_pending: Cell::new(true),
        });
        cx.notify();
    }

    /// `ej`: `POST /stage-instances {channel_id, topic, privacy_level, send_start_notification}`, or `PATCH
    /// /stage-instances/<channel> {topic, privacy_level}` for a live one. The harness never sends either.
    fn submit_stage(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.start_stage.as_mut() else {
            return;
        };
        let topic = open.field.read(cx).text().trim().to_owned();
        if topic.is_empty() || topic.chars().count() > MAX_TOPIC || open.busy {
            return;
        }
        #[cfg(feature = "capture")]
        {
            eprintln!(
                "stage: refused in the harness (topic of {} chars, channel {})",
                topic.len(),
                open.channel
            );
        }
        #[cfg(not(feature = "capture"))]
        {
            open.busy = true;
            open.error = None;
            let (method, route, body) = if open.editing {
                (
                    reqwest::Method::PATCH,
                    format!("/stage-instances/{}", open.channel),
                    json!({"topic": topic, "privacy_level": GUILD_ONLY}),
                )
            } else {
                (
                    reqwest::Method::POST,
                    "/stage-instances".to_owned(),
                    json!({
                        "channel_id": open.channel.to_string(),
                        "topic": topic,
                        "privacy_level": GUILD_ONLY,
                        "send_start_notification": open.notify,
                    }),
                )
            };
            let _ = self.requests.send(Request::Rest {
                key: Some(STAGE_KEY.to_owned()),
                method,
                route,
                body: Some(body),
            });
        }
        cx.notify();
    }

    /// The answer of a start or an edit: the modal closes, and the instance arrives as `STAGE_INSTANCE_CREATE`.
    pub(super) fn stage_answered(&mut self, body: &serde_json::Value, cx: &mut Context<Self>) {
        let Some(open) = self.start_stage.as_mut() else {
            return;
        };
        open.busy = false;
        if body["ok"].as_bool().unwrap_or(false) {
            self.start_stage = None;
        } else {
            open.error = Some(
                body["body"]["message"]
                    .as_str()
                    .unwrap_or("Something went wrong")
                    .to_owned(),
            );
        }
        cx.notify();
    }

    /// Module 128347 on the design system modal with a graphic (`f.k`, as the status modal): "What will you be
    /// talking about?", "Write a topic to start your Stage.", the Stage Topic field, Notify @everyone, Cancel and
    /// Start Stage.
    pub(super) fn start_stage_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.start_stage.as_ref()?;
        if open.focus_pending.replace(false) {
            window.focus(&open.field.read(cx).focus_handle(cx), cx);
        }
        let length = open.field.read(cx).text().chars().count();
        let empty = open.field.read(cx).text().trim().is_empty();
        let over = length > MAX_TOPIC;
        let art = self.images.get(GRAPHIC).cloned();
        let gradient = header_gradient(
            color("--expressive-gradient-purple-start"),
            color("--expressive-gradient-purple-end"),
        );
        let pose = super::motion::modal_pose("start-stage-modal");
        let (title, subtitle) = if open.editing {
            ("Edit Stage", "Change the topic")
        } else {
            (
                "What will you be talking about?",
                "Write a topic to start your Stage.",
            )
        };
        let header = div()
            .relative()
            .flex_none()
            .w(px(HEADER_WIDTH as f32))
            .h(px(HEADER_HEIGHT as f32))
            .child(
                img(gradient)
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w(px(HEADER_WIDTH as f32))
                    .h(px(HEADER_HEIGHT as f32)),
            )
            // The graphic's box is the status modal's (`headerGraphicContainer`): assumed the same here.
            .children(art.map(|art| {
                div()
                    .absolute()
                    .top(px(36.))
                    .left(px(151.5))
                    .child(img(art).w(px(175.)).h(px(108.)))
            }))
            .child(
                div()
                    .absolute()
                    .top(px(168.))
                    .left(px(32.))
                    .w(px(414.))
                    .h(px(32.))
                    .flex()
                    .justify_center()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .child(
                div()
                    .absolute()
                    .top(px(208.))
                    .left(px(32.))
                    .w(px(414.))
                    .h(px(20.))
                    .flex()
                    .justify_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-subtle"))
                    .child(subtitle),
            );
        let input = div()
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(color(if over {
                "--text-feedback-critical"
            } else {
                "--border-subtle"
            }))
            .bg(color("--input-background-default"))
            .px(px(15.))
            .py(px(11.))
            .child(open.field.clone());
        let body = div()
            .flex_none()
            .px(px(32.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .h(px(20.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    // `5FPBOB`.
                    .child("Stage Topic"),
            )
            .child(input)
            .children(open.error.clone().map(|message| {
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(message)
            }));
        let notify = open.notify;
        let footer = div()
            .flex_none()
            .px(px(32.))
            .py(px(16.))
            .flex()
            .items_center()
            .justify_between()
            // `actionBarInput`: the checkbox on the left, only for a new Stage (`ei`).
            .child(div().children((!open.editing).then(|| {
                div()
                    .id("start-stage-notify")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.start_stage.as_mut() {
                            open.notify = !open.notify;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(20.))
                            .rounded(px(4.))
                            .border_2()
                            .border_color(color(if notify {
                                "--control-brand-foreground"
                            } else {
                                "--interactive-text-default"
                            }))
                            .when(notify, |this| this.bg(color("--control-brand-foreground")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(notify.then(|| icon("select_check", 16., "--white"))),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            // `Pe+Pwp`.
                            .child("Notify @everyone"),
                    )
            })))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .w(px(220.))
                    .child(modal::footer_button(
                        modal::button(
                            "start-stage-cancel",
                            "Cancel",
                            modal::Tone::Secondary,
                            |this, _, _| {
                                this.start_stage = None;
                            },
                        ),
                        cx,
                    ))
                    .child(modal::footer_button(
                        modal::button(
                            "start-stage-submit",
                            if open.editing {
                                "Save Changes"
                            } else {
                                "Start Stage"
                            },
                            if empty || over {
                                modal::Tone::PrimaryDisabled
                            } else if open.busy {
                                modal::Tone::PrimaryLoading
                            } else {
                                modal::Tone::Primary
                            },
                            |this, _, cx| this.submit_stage(cx),
                        ),
                        cx,
                    )),
            );
        let panel = div()
            .id("start-stage-modal")
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .occlude()
            .relative()
            .w(px(MODAL_WIDTH))
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
            .child(header)
            .child(div().flex_none().h(px(16.)))
            .child(body)
            .child(div().flex_none().h(px(16.)))
            .child(footer)
            .child(
                super::keyboard::inset_ring_when_focused(div().id("start-stage-close"))
                    .focusable()
                    .tab_index(0)
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .size(px(38.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 20., "--text-strong"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.start_stage = None;
                        cx.notify();
                    })),
            );
        Some(
            deferred(
                div()
                    .id("start-stage-modal-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(Rgba {
                        a: f32::from(0xb8u8) / 255. * pose.scrim,
                        ..rgba(0x000000ff)
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.start_stage = None;
                        cx.notify();
                    }))
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The sidebar row's second line: the topic of a live Stage (`renderSubtitle` of module 616740).
    pub(super) fn stage_topic(&self, channel: &Channel) -> Option<String> {
        if channel.kind != STAGE {
            return None;
        }
        self.stores
            .stage_instances
            .get_by_channel(channel.id)
            .map(|instance| instance.topic.clone())
    }
}
