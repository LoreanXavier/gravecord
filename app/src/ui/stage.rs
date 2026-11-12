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
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, RenderImage, Rgba,
    Subscription, Window, deferred, div, img, prelude::*, px, rgba,
};
#[cfg(not(feature = "capture"))]
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::sync::Arc;

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

/// The pulse gradient behind the waiting view (`pulseGradient_BUFBam`, a canvas): the rows' left edge and centre
/// colours read from stage-ann/stage-waiting, 28px apart from the top of the 905 by 687 page.
const GRADIENT: [(u32, u32); 25] = [
    (0x111023, 0x151435),
    (0x111127, 0x15153b),
    (0x12112a, 0x161540),
    (0x13122e, 0x19184a),
    (0x141332, 0x1a1a51),
    (0x151435, 0x1c1d56),
    (0x14153a, 0x1f215c),
    (0x16153e, 0x222463),
    (0x181744, 0x25286a),
    (0x18194a, 0x272b71),
    (0x1a1a51, 0x2a2e78),
    (0x1b1d56, 0x2d3180),
    (0x1d1f5a, 0x2f3587),
    (0x1f205d, 0x32388e),
    (0x1f205f, 0x353b95),
    (0x212362, 0x383e9c),
    (0x222465, 0x3a41a3),
    (0x232667, 0x3d45ab),
    (0x24266b, 0x3f45b1),
    (0x26286e, 0x434cb9),
    (0x262970, 0x4a52c1),
    (0x272a73, 0x4f59c8),
    (0x292a76, 0x5661cf),
    (0x282c78, 0x5d68d3),
    (0x2a2d79, 0x6570d8),
];

fn backdrop() -> Arc<RenderImage> {
    thread_local! {
        static CACHE: RefCell<Option<Arc<RenderImage>>> = const { RefCell::new(None) };
    }
    CACHE.with(|cache| {
        if let Some(image) = cache.borrow().as_ref() {
            return image.clone();
        }
        let (width, height) = (905u32, 687u32);
        let mut pixels = image::RgbaImage::new(width, height);
        let channel = |rgb: u32, shift: u32| ((rgb >> shift) & 0xff) as f32;
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            let row = ((y as f32 + 0.5) / 28. - 0.5).clamp(0., (GRADIENT.len() - 1) as f32);
            let (low, fraction) = (row.floor() as usize, row.fract());
            let high = (low + 1).min(GRADIENT.len() - 1);
            let across = ((x as f32 + 0.5) / width as f32 * 2. - 1.).abs();
            let mix = |shift: u32| {
                let at = |index: usize| {
                    let (edge, centre) = GRADIENT[index];
                    channel(centre, shift)
                        + (channel(edge, shift) - channel(centre, shift)) * across
                };
                at(low) + (at(high) - at(low)) * fraction
            };
            // Premultiplied BGRA, the layout the atlas takes.
            *pixel = image::Rgba([
                (mix(0) + 0.5) as u8,
                (mix(8) + 0.5) as u8,
                (mix(16) + 0.5) as u8,
                255,
            ]);
        }
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
        *cache.borrow_mut() = Some(image.clone());
        image
    })
}

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

/// `NewChannelsStore` (module 454058): a channel of a Community server shows the NEW pill while it is not vocal, not
/// a thread, was made within the last week and after this account joined, and has no read state of its own.
pub(super) fn new_channel_pill(
    channel: &Channel,
    community: bool,
    joined: Id,
    acked: Id,
    now_ms: u64,
) -> bool {
    const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
    let vocal = matches!(channel.kind, ChannelKind::Voice | STAGE);
    let thread = matches!(channel.kind, ChannelKind::Other(10..=12));
    let made = dc_state::read_state::snowflake_ms(channel.id);
    community
        && !vocal
        && !thread
        && channel.kind != ChannelKind::Category
        && acked == 0
        && made > now_ms.saturating_sub(WEEK_MS)
        && channel.id > joined
}

impl Root {
    /// Whether the list row of `channel` carries the NEW pill.
    pub(super) fn is_new_channel(&self, channel: &Channel) -> bool {
        let Some(guild) = self.current_guild() else {
            return false;
        };
        new_channel_pill(
            channel,
            guild.features.iter().any(|feature| feature == "COMMUNITY"),
            guild.joined_snowflake(),
            self.stores.read_states.acked(channel.id, 0),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as u64)
                .unwrap_or_default(),
        )
    }

    /// `O` of module 454058: opening a new channel acks it at the moment before it was made (it is no longer new).
    pub(super) fn view_new_channel(&mut self, channel: Id) {
        let Some(known) = self
            .current_guild()
            .and_then(|guild| guild.channels.iter().find(|known| known.id == channel))
            .cloned()
        else {
            return;
        };
        if !self.is_new_channel(&known) || self.stores.read_states.mentions(channel) != 0 {
            return;
        }
        let before = channel.saturating_sub(1 << 22);
        self.stores.read_states.ack_local(channel, before, false);
        self.data_revision += 1;
        let _ = self.requests.send(crate::net::Request::Ack {
            channel,
            message: before,
            manual: false,
            mention_count: 0,
            last_viewed: 0,
        });
    }

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

    /// The Stage view: the waiting view over its gradient, or the live Stage. Both fill the page under the title bar
    /// (stage-ann/stage-waiting and stage-live-hover: the page is 905 by 687 at (375, 33)).
    pub(super) fn stage_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(channel) = self.current_channel() else {
            return div().into_any_element();
        };
        match self.stores.stage_instances.get_by_channel(channel.id) {
            None => self.stage_waiting(channel, cx),
            Some(instance) => {
                let topic = instance.topic.clone();
                self.stage_live(channel, topic)
            }
        }
    }

    /// `content_YMXX4L`: 400 wide, centred in the page; the header (24px above, 72 high: a 32/40 title and a 20/24
    /// subtitle 8px under it), then the cards 24px under it and 8px apart.
    fn stage_waiting(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let target = channel.id;
        let moderator = self.can_moderate_stage(channel);
        let guild = self.guild;
        div()
            .parity("stage-view")
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(color("--black"))
            .overflow_hidden()
            .child(img(backdrop()).absolute().top_0().left_0().size_full())
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(400.))
                            .pt(px(24.))
                            .pb(px(24.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(72.))
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
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(16.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .children(moderator.then(|| {
                                        self.stage_card(
                                            "stage-start-card",
                                            "stage_channel",
                                            "Start the Stage",
                                            "Pick a topic to start the conversation.",
                                            cx.listener(move |this, _, _, cx| {
                                                this.open_start_stage(target, cx)
                                            }),
                                        )
                                    }))
                                    .children(moderator.then(|| {
                                        self.stage_card(
                                            "stage-event-card",
                                            "guild_menu_create_event",
                                            "Create Event",
                                            "Schedule a Stage for a future date.",
                                            cx.listener(move |this, _, window, cx| {
                                                this.open_create_event(window, cx)
                                            }),
                                        )
                                    }))
                                    .child(self.stage_card(
                                        "stage-join-card",
                                        "mic",
                                        "Continue without starting",
                                        "For mic checks. Start when you're ready.",
                                        cx.listener(move |this, _, _, cx| {
                                            if let Some(guild) = guild {
                                                this.request_join_voice(guild, target, cx);
                                            }
                                        }),
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `47868` with `container__B7Y3X` (`eventPrompt_YMXX4L`): 400 by 69, 16px padded, a 12px radius on black at 0.52;
    /// the icon is a 32px circle (6px padded) with a 20px glyph, the text 16px after it (a 16/20 semibold title over a
    /// 12/16 line), a caret at the end.
    fn stage_card(
        &self,
        id: &'static str,
        glyph: &'static str,
        title: &'static str,
        description: &'static str,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .w(px(400.))
            .h(px(69.))
            .px(px(16.))
            .rounded(px(12.))
            .bg(Rgba {
                a: 0.52,
                ..rgba(0x000000ff)
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .on_click(on_click)
            .child(
                div()
                    .flex_none()
                    .size(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(glyph, 20., "--white")),
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
            .child(icon("chevron_right_20", 16., "--white"))
            .into_any_element()
    }

    /// The live Stage on black: `participants_ZW-j0k` over the speakers' and audience's sections, the controls under
    /// them (`bottomControls_rTsZa_`).
    fn stage_live(&self, channel: &Channel, topic: String) -> AnyElement {
        let guild = self.guild.unwrap_or_default();
        let target = channel.id;
        let mut speakers = Vec::new();
        let mut audience = Vec::new();
        for state in self.stores.voice_members(guild, target) {
            let user = self
                .stores
                .users
                .get(&state.user_id)
                .cloned()
                .or_else(|| {
                    self.stores
                        .current_user
                        .clone()
                        .filter(|user| user.id == state.user_id)
                })
                .unwrap_or_default();
            if state.suppress {
                audience.push(user);
            } else {
                speakers.push(user);
            }
        }
        let tile = |user: &dc_state::User| {
            let picture = self.images.get(&super::avatar_source(user).0).cloned();
            div()
                .flex_none()
                .w(px(102.))
                .h(px(98.))
                .rounded(px(8.))
                .bg(color("--black"))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .mt(px(8.))
                        .size(px(56.))
                        .rounded_full()
                        .overflow_hidden()
                        .children(picture.map(|picture| img(picture).size(px(56.)))),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .max_w(px(94.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(user.display_name().to_owned()),
                )
                .into_any_element()
        };
        let section = |title: String, people: &[dc_state::User], icon_name: &'static str| {
            div()
                .flex_none()
                .child(
                    div()
                        .h(px(49.))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .child(div().ml(px(3.)).child(icon(
                            icon_name,
                            24.,
                            "--interactive-icon-default",
                        )))
                        .child(
                            div()
                                .ml(px(8.))
                                .flex_1()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(div().mr(px(16.)).child(icon(
                            "chevron_down",
                            20.,
                            "--interactive-icon-default",
                        ))),
                )
                .child(
                    div()
                        .pt(px(12.))
                        .pl(px(2.6))
                        .pb(px(12.))
                        .flex()
                        .flex_wrap()
                        .gap(px(5.))
                        .children(people.iter().map(&tile)),
                )
                .into_any_element()
        };
        // stage-live and stage-live-hover: at rest the page shows neither the header nor the controls; the pointer over
        // the page brings both (`visible`).
        let header = div()
            .flex_none()
            .h(px(49.))
            .px(px(16.))
            .pt(px(8.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .invisible()
            .group_hover("stage-live", |style| style.visible())
            .flex()
            .items_start()
            .child(
                div()
                    .h(px(32.))
                    .flex_1()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .size(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("stage_channel", 24., "--text-muted")),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(topic),
                    )
                    .child(
                        div()
                            .mx(px(16.))
                            .size(px(4.))
                            .rounded_full()
                            .bg(color("--text-muted")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(icon("mic", 16., "--text-muted"))
                            .child(
                                div()
                                    .ml(px(4.))
                                    .child(format!("{} is speaking", speakers.len())),
                            )
                            .child(div().ml(px(8.)).child(icon("members", 16., "--text-muted")))
                            .child(
                                div()
                                    .ml(px(4.))
                                    .child(format!("{} in audience", audience.len())),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(24.))
                    .child(icon("system_hand", 24., "--interactive-icon-default"))
                    .child(icon("voice_view_chat", 24., "--interactive-icon-default")),
            );
        let round = |id: &'static str, glyph: &'static str| {
            div()
                .id(id)
                .size(px(40.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .child(icon(glyph, 20., "--white"))
        };
        let red = color("--red-400");
        let controls = div()
            .absolute()
            .invisible()
            .group_hover("stage-live", |style| style.visible())
            .left_0()
            .right_0()
            .bottom(px(16.))
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .child(
                        div()
                            .h(px(50.))
                            .px(px(5.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(Rgba {
                                a: 0.06,
                                ..rgba(0x6c6d76ff)
                            })
                            .bg(rgba(0x131416ff))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(round("stage-sound", "voice_soundboard"))
                            .child(round("stage-hand", "system_hand"))
                            .child(round("stage-mic", "mic_muted"))
                            .child(round("stage-more", "voice_view_more")),
                    )
                    .child(
                        div()
                            .ml(px(15.))
                            .h(px(50.))
                            .w(px(74.))
                            .rounded(px(12.))
                            .bg(red)
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .id("stage-leave")
                                    .ml(px(5.))
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("voice_disconnect", 20., "--white")),
                            )
                            .child(
                                div()
                                    .size(px(24.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("voice_view_caret", 16., "--white")),
                            ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .right(px(16.))
                    .bottom(px(13.))
                    .flex()
                    .gap(px(20.))
                    .child(icon("voice_view_popout", 24., "--white"))
                    .child(icon("voice_view_fullscreen", 24., "--white")),
            );
        div()
            .parity("stage-view")
            .group("stage-live")
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(color("--black"))
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .children((!speakers.is_empty()).then(|| {
                        section(
                            format!("Speakers — {}", speakers.len()),
                            &speakers,
                            "members",
                        )
                    }))
                    .child(section(
                        format!("Audience — {}", audience.len()),
                        &audience,
                        "members",
                    )),
            )
            .child(controls)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: Id, kind: ChannelKind) -> Channel {
        Channel {
            id,
            name: "rules".to_owned(),
            kind,
            position: 0,
            parent_id: None,
            last_message_id: None,
            overwrites: Vec::new(),
            user_limit: 0,
            topic: None,
            rate_limit_per_user: 0,
            flags: 0,
            nsfw: false,
            default_auto_archive_duration: None,
            available_tags: Vec::new(),
            default_sort_order: None,
            default_forum_layout: 0,
        }
    }

    #[test]
    fn new_pill_needs_a_young_unread_text_channel_in_a_community() {
        let now = 1_900_000_000_000u64;
        let made = |ms_ago: u64| (now - ms_ago - 1_420_070_400_000) << 22;
        let day = 24 * 60 * 60 * 1000;
        let young = channel(made(day), ChannelKind::Text);
        assert!(new_channel_pill(&young, true, 1, 0, now));
        assert!(!new_channel_pill(&young, false, 1, 0, now));
        assert!(!new_channel_pill(&young, true, 1, 5, now));
        assert!(!new_channel_pill(&young, true, young.id + 1, 0, now));
        let old = channel(made(8 * day), ChannelKind::Text);
        assert!(!new_channel_pill(&old, true, 1, 0, now));
        let voice = channel(made(day), ChannelKind::Voice);
        assert!(!new_channel_pill(&voice, true, 1, 0, now));
    }
}
