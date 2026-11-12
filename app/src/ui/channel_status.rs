//! Voice channel status: the request for a guild's statuses, the status line under a voice channel's name, the
//! modal that sets it and the request that clears it.
//!
//! A guild's statuses and call start times are asked for once per connection, when one of its voice channels is
//! shown (`useChannelStatus` in module 882840 calls module 52074: op 43 `REQUEST_CHANNEL_INFO`, `fields: ["status",
//! "voice_start_time"]`). Setting or clearing one is `PUT /channels/<id>/voice-status {status}` (`Rsh.UPDATE_VOICE_CHANNEL_STATUS`,
//! `updateVoiceChannelStatus` in module 435183); the answer is a 204, and the change comes back as
//! `VOICE_CHANNEL_STATUS_UPDATE`.
//!
//! The sidebar line is `r4` of module 616740: a status shows under any voice channel, as `statusText_UqhBJC`; this
//! client being connected and allowed to set one makes it a button (`hoverable_UqhBJC`, the pencil with it), and with
//! no status "Set a channel status" and a pencil stand in its place. The modal is module 136523 on the design system
//! modal of module 772707 (`graphic`, `title`, `subtitle`); measured in `design/dom/ch-vstatus/modal-*`.

use super::{Root, color, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_proto::op::Op;
use dc_state::{Channel, ChannelKind, Id, permissions};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, RenderImage, Rgba,
    SharedString, Subscription, Window, deferred, div, img, prelude::*, px, rgba, svg,
};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::sync::Arc;

/// The status as spans when it holds an emoji, with everything else plain text; `None` when it holds none (or there is
/// no status).
fn emoji_spans(status: Option<&str>) -> Option<Vec<dc_markdown::Span>> {
    use dc_markdown::{Kind, Span, Style};
    let spans: Vec<Span> = dc_markdown::inline(status?)
        .into_iter()
        .map(|span| match span.kind {
            Kind::Unicode | Kind::Emoji { .. } => span,
            _ => Span {
                style: Style::default(),
                kind: Kind::Text,
                ..span
            },
        })
        .collect();
    spans
        .iter()
        .any(|span| !matches!(span.kind, Kind::Text))
        .then_some(spans)
}

/// The key the answer of the status request comes back under (`Update::Fetched`).
pub(super) const STATUS_KEY: &str = "voice-status";
/// `src` of the modal's graphic (module 379649).
const TEAPOT_ART: &str = "https://discord.com/assets/4ed48b8404cc8dd4.svg";
/// `maxCharacterCount` and `showRemainingCharsAfterCount` of the modal's field (module 136523).
const MAX_CHARACTERS: usize = 500;
const COUNT_AFTER: usize = 250;
/// The modal's box: `container_dvN-MP size-md` is 480 wide; the header's gradient and graphic are 478 by 244 inside
/// its 1px border.
pub(super) const MODAL_WIDTH: f32 = 480.;
pub(super) const HEADER_WIDTH: u32 = 478;
pub(super) const HEADER_HEIGHT: u32 = 244;

/// The open status modal: which channel, the text field and the state of the request.
pub(crate) struct StatusModal {
    channel: Id,
    field: Entity<Editor>,
    _events: Subscription,
    /// The status the field opened with (`k`): sending it unchanged just closes the modal.
    initial: String,
    /// The request is out (`M`); a second press does nothing until it answers.
    busy: bool,
    /// The message of a failed request, shown under the field (`errorMessage` of the form item).
    error: Option<String>,
    /// The field takes focus on the next frame (the opener has no window).
    focus_pending: Cell<bool>,
}

impl StatusModal {
    /// The field, for the harness's `typed` step.
    #[cfg(feature = "capture")]
    pub(crate) fn field(&self) -> Entity<Editor> {
        self.field.clone()
    }
}

/// What the sidebar line of a voice channel needs to know.
struct Line {
    status: Option<String>,
    /// `m` of `r4`: `SET_VOICE_CHANNEL_STATUS` in this channel (module 532622).
    can_set: bool,
}

/// The header gradient of `container_pmhtZL`: `linear-gradient(270deg in oklab, end, start)` under
/// `mask-image: radial-gradient(100% 100% at 50% 100%, transparent 60%, #000 100%)`, as pixels.
pub(super) fn header_gradient(start: Rgba, end: Rgba) -> Arc<RenderImage> {
    thread_local! {
        static CACHE: RefCell<Option<([u32; 8], Arc<RenderImage>)>> = const { RefCell::new(None) };
    }
    let key = [
        start.r.to_bits(),
        start.g.to_bits(),
        start.b.to_bits(),
        start.a.to_bits(),
        end.r.to_bits(),
        end.g.to_bits(),
        end.b.to_bits(),
        end.a.to_bits(),
    ];
    CACHE.with(|cache| {
        if let Some((known, image)) = cache.borrow().as_ref()
            && *known == key
        {
            return image.clone();
        }
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(gradient_pixels(
            start, end,
        ))]));
        *cache.borrow_mut() = Some((key, image.clone()));
        image
    })
}

fn to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(value: f32) -> f32 {
    let value = value.clamp(0., 1.);
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}

/// sRGB to Oklab (Ottosson's matrices, the space the gradient interpolates in).
fn to_oklab(color: Rgba) -> [f32; 3] {
    let (r, g, b) = (to_linear(color.r), to_linear(color.g), to_linear(color.b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        to_srgb(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        to_srgb(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s),
        to_srgb(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    ]
}

/// Premultiplied BGRA pixels, the layout the atlas takes.
fn gradient_pixels(start: Rgba, end: Rgba) -> image::RgbaImage {
    let (from, to) = (to_oklab(end), to_oklab(start));
    let alpha = start.a;
    let (width, height) = (HEADER_WIDTH as f32, HEADER_HEIGHT as f32);
    let mut pixels = image::RgbaImage::new(HEADER_WIDTH, HEADER_HEIGHT);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        // 270deg runs right to left: `end` at the right edge, `start` at the left one.
        let across = 1. - (x as f32 + 0.5) / width;
        let mixed = from_oklab([
            from[0] + (to[0] - from[0]) * across,
            from[1] + (to[1] - from[1]) * across,
            from[2] + (to[2] - from[2]) * across,
        ]);
        // The mask: an ellipse as wide and tall as the box, centred on the bottom edge's middle; clear inside 60% of
        // it, opaque at its rim.
        let dx = (x as f32 + 0.5 - width / 2.) / width;
        let dy = (y as f32 + 0.5 - height) / height;
        let mask = (((dx * dx + dy * dy).sqrt() - 0.6) / 0.4).clamp(0., 1.);
        let a = alpha * mask;
        // Straight alpha: the decoded images the atlas takes are not premultiplied.
        let channel = |value: f32| (value * 255. + 0.5) as u8;
        *pixel = image::Rgba([
            channel(mixed[2]),
            channel(mixed[1]),
            channel(mixed[0]),
            (a * 255. + 0.5) as u8,
        ]);
    }
    pixels
}

impl Root {
    /// Asks the gateway for the statuses of the guild in view when none were asked for on this connection and the
    /// guild has a voice channel.
    pub(super) fn channel_info_check(&mut self) {
        let Some(guild) = self.guild else {
            return;
        };
        if self.stores.channel_status.has_requested(guild) {
            return;
        }
        let has_voice = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .is_some_and(|known| {
                known
                    .channels
                    .iter()
                    .any(|channel| channel.kind == ChannelKind::Voice)
            });
        if !has_voice {
            return;
        }
        self.stores.dispatch(
            "FETCH_CHANNEL_INFO",
            &json!({"guild_id": guild.to_string()}),
        );
        let _ = self.requests.send(Request::StreamOp {
            op: Op::RequestChannelInfo,
            data: json!({"guild_id": guild.to_string(), "fields": ["status", "voice_start_time"]}),
        });
    }

    /// `updateVoiceChannelStatus`: sets the status of a voice channel, or clears it with an empty text.
    pub(super) fn set_voice_status(&mut self, channel: Id, status: &str) -> bool {
        // A harness run only ever touches the fixture guild's General voice channel.
        #[cfg(feature = "capture")]
        if channel != FIXTURE_VOICE {
            tracing::warn!("voice status request refused: not the fixture's voice channel");
            return false;
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(STATUS_KEY.to_owned()),
            method: reqwest::Method::PUT,
            route: format!("/channels/{channel}/voice-status"),
            body: Some(json!({"status": status})),
        });
        true
    }

    /// "Clear Channel Status" of the channel menu: the status request with an empty status
    /// (`updateVoiceChannelStatus(id, "")`, module 671483).
    pub(super) fn clear_channel_status(&mut self, channel: Id) -> bool {
        self.set_voice_status(channel, "")
    }

    /// The status of a voice channel of the guild in view.
    pub(super) fn voice_channel_status(&self, channel: Id) -> Option<&str> {
        self.stores
            .channel_status
            .get_channel_status(self.guild?, channel)
    }

    /// Whether this client is in the channel (`connected` of the row).
    pub(super) fn in_voice_channel(&self, channel: Id) -> bool {
        self.call
            .as_ref()
            .is_some_and(|call| call.channel == channel)
    }

    /// `m` of `r4`: the current user may set the channel's status (module 532622 with its one permission).
    pub(super) fn can_set_voice_status(&self, channel: &Channel) -> bool {
        self.can_in_channel(channel, permissions::SET_VOICE_CHANNEL_STATUS)
    }

    fn voice_line_facts(&self, channel: &Channel, connected: bool) -> Option<Line> {
        // A live Stage shows its topic under its name (`renderSubtitle` of module 616740): never a button.
        if let Some(topic) = self.stage_topic(channel) {
            return Some(Line {
                status: Some(topic),
                can_set: false,
            });
        }
        if channel.kind != ChannelKind::Voice {
            return None;
        }
        let status = self.voice_channel_status(channel.id).map(str::to_owned);
        let can_set = self.can_set_voice_status(channel);
        (status.is_some() || (connected && can_set)).then_some(Line { status, can_set })
    }

    /// The row has the second line (`linkBottom_TNyK-N`), so it is the taller one.
    pub(super) fn voice_row_has_line(&self, channel: &Channel) -> bool {
        self.voice_line_facts(channel, self.in_voice_channel(channel.id))
            .is_some()
    }

    /// `r4`: the status of the channel, or "Set a channel status" with a pencil while this client is in it.
    pub(super) fn voice_status_line(
        &self,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let connected = self.in_voice_channel(channel.id);
        let line = self.voice_line_facts(channel, connected)?;
        // `.statusDiv.hoverable:hover`: text and pencil go to the hover colour, only for the connected client that may
        // set it.
        let hoverable = connected && line.can_set;
        let target = channel.id;
        let group = SharedString::from(format!("voice-status-{target}"));
        let tint = |name: &'static str| color(name);
        let text = div()
            .min_w(px(0.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(tint("--channels-default"))
            .when(hoverable, |this| {
                this.group_hover(group.clone(), |style| {
                    style.text_color(color("--interactive-text-hover"))
                })
            })
            .child(match emoji_spans(line.status.as_deref()) {
                // `VOICE_CHANNEL_STATUS_RULES` (module 46054) parse custom and unicode emoji; the rest is text.
                Some(spans) => {
                    let base = color("--channels-default").into();
                    let pieces = spans
                        .iter()
                        .map(|span| self.piece(span, base, 12., 500., 16.))
                        .collect();
                    div()
                        .overflow_hidden()
                        .child(crate::inline::inline(pieces, px(16.)).truncate())
                        .into_any_element()
                }
                None => div()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(match &line.status {
                        Some(status) => status.clone(),
                        None => "Set a channel status".to_owned(),
                    })
                    .into_any_element(),
            });
        Some(
            div()
                .flex_none()
                .h(px(16.))
                .pl(px(28.))
                .flex()
                .items_center()
                .child(
                    // `.statusDiv_UqhBJC`: as wide as its text and pencil, so only they are the hover and click target.
                    div()
                        .id(SharedString::from(format!("voice-status-line-{target}")))
                        .group(group.clone())
                        .min_w(px(0.))
                        .h(px(16.))
                        .flex()
                        .items_center()
                        .when(hoverable, |this| this.cursor_pointer())
                        .child(text)
                        // `.pencilIcon_UqhBJC`: 4px in, 12px, in the text's colour.
                        .when(line.status.is_none(), |this| {
                            this.child(
                                div()
                                    .ml(px(4.))
                                    .flex_none()
                                    .text_color(tint("--channels-default"))
                                    .when(hoverable, |this| {
                                        this.group_hover(group, |style| {
                                            style.text_color(color("--interactive-text-hover"))
                                        })
                                    })
                                    .child(
                                        svg()
                                            .path("icons/channel_status_pencil.svg")
                                            .size(px(12.))
                                            .flex_none(),
                                    ),
                            )
                        })
                        // `handleVoiceStatusClick`: only the connected client acts, and the click stops there.
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.in_voice_channel(target) {
                                cx.stop_propagation();
                                this.open_channel_status_modal(target, cx);
                            }
                        })),
                )
                .into_any_element(),
        )
    }

    /// Opens the status modal of a voice channel, its field holding the current status (module 136523). The field
    /// takes focus on the next frame.
    pub(in crate::ui) fn open_channel_status_modal(&mut self, channel: Id, cx: &mut Context<Self>) {
        let Some(known) = self
            .stores
            .guilds
            .iter()
            .flat_map(|guild| guild.channels.iter())
            .find(|known| known.id == channel)
        else {
            return;
        };
        let name = known.name.clone();
        let initial = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel))
            .and_then(|guild| {
                self.stores
                    .channel_status
                    .get_channel_status(guild.id, channel)
            })
            .unwrap_or_default()
            .to_owned();
        let field = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            // `allowNewLines: false`; the counter and the button follow every edit.
            editor.set_single_line(true);
            editor.set_reports_all_edits(true);
            editor.set_placeholder(format!("Status for {name}"));
            editor.set_text(&initial);
            editor
        });
        let events = cx.subscribe(&field, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(_) => this.submit_channel_status(cx),
                EditorEvent::Cancel => this.channel_status = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        self.request_image(TEAPOT_ART.to_owned(), TEAPOT_ART.to_owned());
        self.channel_status = Some(StatusModal {
            channel,
            field,
            _events: events,
            initial,
            busy: false,
            error: None,
            focus_pending: Cell::new(true),
        });
        cx.notify();
    }

    fn close_channel_status(&mut self) {
        self.channel_status = None;
    }

    /// `D` of module 136523: sends the text; the modal closes when the answer is a 204.
    fn submit_channel_status(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.channel_status.as_mut() else {
            return;
        };
        let text = modal.field.read(cx).text().to_owned();
        // `G`: over the limit, or the request is out: Enter does nothing.
        if text.encode_utf16().count() > MAX_CHARACTERS || modal.busy {
            return;
        }
        let channel = modal.channel;
        // An unchanged status closes the modal at once (the request still follows, as the official one does).
        let unchanged = text == modal.initial;
        modal.error = None;
        modal.busy = !unchanged;
        let sent = self.set_voice_status(channel, &text);
        if unchanged {
            self.channel_status = None;
        } else if !sent && let Some(modal) = self.channel_status.as_mut() {
            modal.busy = false;
        }
        cx.notify();
    }

    /// The answer of the status request: 204 closes the modal, anything else shows its message.
    pub(super) fn voice_status_answered(
        &mut self,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        if let Some(modal) = self.channel_status.as_mut() {
            if body["ok"] == true {
                self.channel_status = None;
            } else {
                modal.busy = false;
                modal.error = body["body"]["message"].as_str().map(str::to_owned);
            }
        }
        cx.notify();
    }

    /// The editors of the open status modal, for the harness's typing.
    #[cfg(feature = "capture")]
    pub(super) fn channel_status_fields(&self) -> Option<Entity<Editor>> {
        self.channel_status.as_ref().map(StatusModal::field)
    }

    /// The modal of module 136523: gradient header with the teapot, title and subtitle, the "Status" field with its
    /// emoji button and counter, Cancel and Set Status.
    pub(super) fn channel_status_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.channel_status.as_ref()?;
        if open.focus_pending.replace(false) {
            window.focus(&open.field.read(cx).focus_handle(cx), cx);
        }
        let length = open.field.read(cx).text().encode_utf16().count();
        let over = length > MAX_CHARACTERS;
        let counting = length >= COUNT_AFTER;
        let remaining = MAX_CHARACTERS as i64 - length as i64;
        let art = self.images.get(TEAPOT_ART).cloned();
        let gradient = header_gradient(
            color("--expressive-gradient-purple-start"),
            color("--expressive-gradient-purple-end"),
        );
        let field = open.field.clone();
        let emoji_field = open.field.clone();
        let pose = super::motion::modal_pose("channel-status-modal");
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
            // `headerGraphicContainer_dvN-MP`: 288 wide at 32px in and 36px down; the image is 175 by 108, centred.
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
                    .child("What are we chatting about?"),
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
                    .child("Let others know what you're up to in the voice channel!"),
            );
        // `channelTextArea_2x8-Qn`: the field is up to 80 tall, scrolled to its last line; the emoji button stays in
        // the first 44px.
        let input = div()
            .relative()
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--input-background-default"))
            .flex()
            .pl(px(15.))
            .pr(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .max_h(px(80.))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .child(div().flex_none().py(px(11.)).child(field)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(32.))
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(self.images.get("emoji_button").cloned().map(|image| {
                        super::keyboard::inset_ring_when_focused(div().id("channel-status-emoji"))
                            .focusable()
                            .tab_index(0)
                            .size(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    let at = event.position();
                                    this.picker_field = Some(emoji_field.clone());
                                    this.open_picker(
                                        super::picker::Target::Field,
                                        gpui::point(at.x, at.y - px(24.)),
                                        window,
                                        cx,
                                    );
                                },
                            ))
                            .child(img(image).size(px(18.)).grayscale(true))
                    })),
            )
            // `characterCount_A7cPID`: the characters left, from 250 typed on.
            .when(counting, |this| {
                this.child(
                    div()
                        .absolute()
                        .right(px(14.))
                        .bottom(px(6.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color(if over {
                            "--text-feedback-critical"
                        } else {
                            "--text-default"
                        }))
                        .child(remaining.to_string()),
                )
            });
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
                    .child("Status"),
            )
            .child(input)
            .children(open.error.clone().map(|message| {
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(message)
            }));
        let footer = div()
            .flex_none()
            .px(px(32.))
            .py(px(16.))
            .flex()
            .gap(px(8.))
            .child(modal::footer_button(
                modal::button(
                    "channel-status-cancel",
                    "Cancel",
                    modal::Tone::Secondary,
                    |this, _, _| this.close_channel_status(),
                ),
                cx,
            ))
            .child(modal::footer_button(
                modal::button(
                    "channel-status-submit",
                    "Set Status",
                    if over || open.busy {
                        modal::Tone::PrimaryDisabled
                    } else {
                        modal::Tone::Primary
                    },
                    |this, _, cx| this.submit_channel_status(cx),
                ),
                cx,
            ));
        let panel = div()
            .id("channel-status-modal")
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
                super::keyboard::inset_ring_when_focused(div().id("channel-status-close"))
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
                        this.close_channel_status();
                        cx.notify();
                    })),
            );
        Some(
            deferred(
                div()
                    .id("channel-status-modal-layer")
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
                        this.close_channel_status();
                        cx.notify();
                    }))
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The harness's `voice status <text>` and `voice status clear`: only ever the fixture guild's General voice
    /// channel, and only while this client is in it.
    #[cfg(feature = "capture")]
    pub(crate) fn script_voice_status(&mut self, words: &[&str]) -> String {
        let shown = |root: &Self| {
            root.stores
                .guilds
                .iter()
                .find(|guild| {
                    guild
                        .channels
                        .iter()
                        .any(|channel| channel.id == FIXTURE_VOICE)
                })
                .and_then(|guild| {
                    root.stores
                        .channel_status
                        .get_channel_status(guild.id, FIXTURE_VOICE)
                })
                .unwrap_or("none")
                .to_owned()
        };
        // No words only reports what the store holds.
        if words.is_empty() {
            return format!("store shows: {}", shown(self));
        }
        let text = words.join(" ");
        let status = if text == "clear" { "" } else { text.as_str() };
        self.set_voice_status(FIXTURE_VOICE, status);
        format!(
            "status request sent ({} chars); store shows: {}",
            status.chars().count(),
            shown(self)
        )
    }
}

/// The fixture guild's voice channel "General", the only one a harness run may set a status on.
#[cfg(feature = "capture")]
const FIXTURE_VOICE: Id = 1_568_692_565_136_375_901;

#[cfg(test)]
mod tests {
    use super::*;
    use dc_markdown::Kind;

    #[test]
    fn plain_text_has_no_emoji_spans() {
        assert!(emoji_spans(None).is_none());
        assert!(emoji_spans(Some("lobby music **loud**")).is_none());
    }

    #[test]
    fn unicode_and_custom_emoji_become_spans_and_the_rest_stays_plain_text() {
        let spans = emoji_spans(Some("**party** \u{1F389} <:wave:123456789012345678>")).unwrap();
        assert!(spans.iter().any(|span| matches!(span.kind, Kind::Unicode)));
        assert!(
            spans
                .iter()
                .any(|span| matches!(span.kind, Kind::Emoji { .. }))
        );
        for span in &spans {
            if matches!(span.kind, Kind::Text) {
                assert_eq!(span.style, dc_markdown::Style::default());
            }
        }
    }
}
