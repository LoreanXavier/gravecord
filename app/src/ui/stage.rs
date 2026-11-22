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
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Id, permissions};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, RenderImage, Rgba,
    Subscription, Window, deferred, div, img, prelude::*, px, rgba,
};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::sync::Arc;

/// `ChannelKind::Other(13)`: `GUILD_STAGE_VOICE`.
const STAGE: ChannelKind = ChannelKind::Other(13);

/// Legacy field slot on `Root` (Stage tiles now open `tile_menu_layer` / `context_menu`).
#[derive(Clone, Copy)]
pub(super) struct TileMenu;
/// Module 973324: the modal's graphic.
const GRAPHIC: &str = "https://cdn.discordapp.com/assets/content/ef7fa86f8c519fe9764867c1ba473ee1939292fddb2c897885aab732e3bd31d2.svg";
/// The age coachmark's graphic (`image_sYj23M` in stage-ann/stage-live-controls).
const COACH_ART: &str = "https://cdn.discordapp.com/assets/content/971d3a140aa11a3830346a83266ea28b2f4e586e9b4f02eed4315538f6a9ebbe.svg";
/// The age notice of the start modal and its link (`ageVerificationNotice_CWLnwx`, stage-ann/stage-start-modal).
const AGE_NOTICE: &str = "You must confirm you're an adult to speak on Stages. Confirm age";
const AGE_LINK: &str = "Confirm age";
/// `maxLength` of the topic field (module 128347).
const MAX_TOPIC: usize = 120;
/// `privacy_level` 2: `GUILD_ONLY`, what the modal starts with.
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
    /// The blocked notice's "View all" popout is open (module 918192 `d`).
    blocked_list: bool,
}

impl StartStage {
    /// The blocked notice's View all popout is open (for the harness).
    #[cfg(feature = "capture")]
    pub(super) fn blocked_list_open(&self) -> bool {
        self.blocked_list
    }
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

/// `atPreviousMillisecond` (module 811091): the snowflake of the millisecond before the id's own, low bits zero.
fn snowflake_before(id: Id) -> Id {
    ((id >> 22).saturating_sub(1)) << 22
}

#[cfg(test)]
mod before_tests {
    use super::snowflake_before;

    #[test]
    fn the_previous_millisecond_drops_the_worker_and_sequence_bits() {
        let id = (1_000_u64 << 22) | 0x2A_BCDE;
        assert_eq!(snowflake_before(id), 999 << 22);
        assert_eq!(snowflake_before(5), 0);
    }
}

/// The Stage page's top bar (`topControls_rTsZa_` of stage-ann/stage-live-hover and stage-after-end): the channel's
/// glyph, the topic (the channel's name while no Stage is live), the speaking and audience counts, and the hand and
/// chat icons. Hidden until the pointer is over the page (`group` "stage-live").
fn stage_header(
    title: String,
    speaking: usize,
    audience: usize,
    audience_btn: Option<AnyElement>,
    requests: Option<AnyElement>,
) -> gpui::Div {
    div()
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
                        .child(title),
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
                        .child(div().ml(px(4.)).child(dc_intl::format(
                            "chmM9N",
                            &[("count", dc_intl::Arg::Num(speaking as f64))],
                        )))
                        .child(div().ml(px(8.)).child(icon("members", 16., "--text-muted")))
                        .child(div().ml(px(4.)).child(dc_intl::format(
                            "+v2pN2",
                            &[("count", dc_intl::Arg::Num(audience as f64))],
                        ))),
                ),
        )
        .child(
            div()
                .h(px(32.))
                .flex()
                .items_center()
                .gap(px(24.))
                // Module 410828 (`di`): audience toggle for all Stage users + moderator pending badge.
                .children(audience_btn)
                // The requests toggle of module 482877 (`stage_requests`), for someone with Mute Members.
                .children(requests)
                .child(icon("voice_view_chat", 24., "--interactive-icon-default")),
        )
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
        let before = snowflake_before(channel);
        self.stores.read_states.ack_local(channel, before, false);
        self.data_revision += 1;
        let _ = self.requests.send(crate::net::Request::Ack {
            channel,
            message: before,
            manual: false,
            mention_count: 0,
            last_viewed: dc_state::read_state::days_since_epoch(super::picker::now_ms()),
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

    /// The graphics of the Stage's popovers, asked for when pictures are (`request_images`).
    pub(super) fn request_stage_art(&mut self) {
        self.request_image(COACH_ART.to_owned(), COACH_ART.to_owned());
        self.request_image(GRAPHIC.to_owned(), GRAPHIC.to_owned());
    }

    /// The Stage view: the waiting view over its gradient, or the live/pre-start Stage room.
    /// Module 717965: after clicking "Continue without starting", stays in the Stage room (`is_live = false`) with
    /// event prompts (`394412`) and "Start the Stage" (`859894`) above bottom controls.
    pub(super) fn stage_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(channel) = self.current_channel() else {
            return div().into_any_element();
        };
        match self.stores.stage_instances.get_by_channel(channel.id) {
            None => {
                let connected_here = self
                    .call
                    .as_ref()
                    .is_some_and(|call| call.channel == channel.id);
                if connected_here && self.stage_left_start_screen == Some(channel.id) {
                    self.stage_live(channel, channel.name.clone(), false, cx)
                } else {
                    self.stage_waiting(channel, cx)
                }
            }
            Some(instance) => {
                let topic = instance.topic.clone();
                let connected_here = self
                    .call
                    .as_ref()
                    .is_some_and(|call| call.channel == channel.id);
                if connected_here {
                    self.stage_live(channel, topic, true, cx)
                } else {
                    self.stage_not_connected_card(channel, &topic, cx)
                }
            }
        }
    }

    /// Module 623413 — Not-Connected Stage Channel Card (`_mBjXoo`): shown when viewing a live
    /// Stage channel before joining. Renders ambient glow backdrop, up to 5 speaker tiles
    /// (`160x90` with `48px` avatars in a `168 * (len == 4 ? 2 : 3)` grid), topic heading,
    /// speaking summary (`FUVhyC` / `EQwZlN` / `zBcKoA` / `3AqFaG`), primary `"Join Stage"`
    /// (`7vb2cc`) or disabled `"Channel Locked"` (`TVBCKZ`) button, and Invite button.
    fn stage_not_connected_card(
        &self,
        channel: &Channel,
        topic: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let guild = self.guild.unwrap_or_default();
        let target = channel.id;
        let here = self.stores.voice_members(guild, target);
        let mut speakers = Vec::new();
        let mut audience_count = 0usize;
        for state in &here {
            if state.suppress {
                audience_count += 1;
            } else {
                let user = self
                    .stores
                    .users
                    .get(&state.user_id)
                    .cloned()
                    .or_else(|| {
                        self.stores
                            .current_user
                            .clone()
                            .filter(|u| u.id == state.user_id)
                    })
                    .unwrap_or_else(|| dc_state::User {
                        id: state.user_id,
                        username: format!("Speaker {}", state.user_id),
                        ..Default::default()
                    });
                speakers.push((user, self.speaking.contains(&state.user_id)));
            }
        }
        let speaker_names: Vec<String> = speakers
            .iter()
            .map(|(u, _)| {
                u.global_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&u.username)
                    .to_owned()
            })
            .collect();
        let summary = stage_speaking_summary(&speaker_names);
        let shown_speakers: Vec<_> = speakers.into_iter().take(5).collect();
        let grid_w = stage_speaker_grid_width(shown_speakers.len());
        let can_connect = self.can_in_channel(channel, permissions::CONNECT);
        let can_invite = self.can_in_channel(channel, permissions::CREATE_INSTANT_INVITE);
        let heading = if topic.trim().is_empty() {
            channel.name.clone()
        } else {
            topic.to_owned()
        };

        div()
            .parity("stage-not-connected-card")
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
                    .top_0()
                    .left_0()
                    .right_0()
                    .child(stage_header(
                        channel.name.clone(),
                        here.len().saturating_sub(audience_count),
                        audience_count,
                        None,
                        None,
                    )),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .px(px(24.))
                    .child(
                        div()
                            .max_w(px(560.))
                            .w_full()
                            .p(px(24.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(rgba(0xffffff1a))
                            .bg(Rgba {
                                a: 0.62,
                                ..rgba(0x111218ff)
                            })
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(16.))
                            .when(!shown_speakers.is_empty(), |card| {
                                card.child(
                                    div()
                                        .w(px(grid_w))
                                        .flex()
                                        .flex_wrap()
                                        .justify_center()
                                        .gap(px(8.))
                                        .children(shown_speakers.into_iter().map(
                                            |(user, active)| {
                                                let picture = self
                                                    .images
                                                    .get(&super::avatar_source(&user).0)
                                                    .cloned();
                                                let display = user
                                                    .global_name
                                                    .as_deref()
                                                    .filter(|s| !s.is_empty())
                                                    .unwrap_or(&user.username)
                                                    .to_owned();
                                                div()
                                                    .w(px(160.))
                                                    .h(px(90.))
                                                    .rounded(px(10.))
                                                    .border_1()
                                                    .border_color(if active {
                                                        color("--green-360")
                                                    } else {
                                                        rgba(0xffffff18)
                                                    })
                                                    .bg(Rgba {
                                                        a: 0.72,
                                                        ..rgba(0x1b1d26ff)
                                                    })
                                                    .flex()
                                                    .flex_col()
                                                    .items_center()
                                                    .justify_center()
                                                    .gap(px(6.))
                                                    .child(
                                                        div()
                                                            .size(px(48.))
                                                            .rounded_full()
                                                            .bg(color("--brand-500"))
                                                            .overflow_hidden()
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .children(picture.map(|pic| {
                                                                img(pic)
                                                                    .size(px(48.))
                                                                    .rounded_full()
                                                                    .object_fit(
                                                                        gpui::ObjectFit::Cover,
                                                                    )
                                                            })),
                                                    )
                                                    .child(
                                                        div()
                                                            .max_w(px(140.))
                                                            .text_ellipsis()
                                                            .text_size(px(12.))
                                                            .line_height(px(16.))
                                                            .font_weight(FontWeight(500.))
                                                            .text_color(color("--white"))
                                                            .child(display),
                                                    )
                                            },
                                        )),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_center()
                                            .text_size(px(22.))
                                            .line_height(px(28.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--white"))
                                            .child(heading),
                                    )
                                    .child(
                                        div()
                                            .text_center()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(summary),
                                    ),
                            )
                            .child(
                                div()
                                    .pt(px(4.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("stage-not-connected-join")
                                            .h(px(40.))
                                            .px(px(20.))
                                            .rounded(px(8.))
                                            .bg(if can_connect {
                                                color("--green-360")
                                            } else {
                                                color("--background-mod-subtle")
                                            })
                                            .when(can_connect, |d| {
                                                d.hover(|s| s.bg(color("--green-400")))
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Some(gid) = this.guild {
                                                            this.request_join_voice(
                                                                gid, target, cx,
                                                            );
                                                        }
                                                        cx.notify();
                                                    }))
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .gap(px(8.))
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(if can_connect {
                                                color("--white")
                                            } else {
                                                color("--text-muted")
                                            })
                                            .child(icon(
                                                if can_connect {
                                                    "stage_channel"
                                                } else {
                                                    "channel_lock"
                                                },
                                                16.,
                                                if can_connect {
                                                    "--white"
                                                } else {
                                                    "--text-muted"
                                                },
                                            ))
                                            .child(if can_connect {
                                                dc_intl::t("7vb2cc")
                                            } else {
                                                dc_intl::t("TVBCKZ")
                                            }),
                                    )
                                    .when(can_invite, |row| {
                                        row.child(
                                            div()
                                                .id("stage-not-connected-invite")
                                                .h(px(40.))
                                                .px(px(16.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(rgba(0xffffff24))
                                                .bg(Rgba {
                                                    a: 0.45,
                                                    ..rgba(0x2b2d31ff)
                                                })
                                                .hover(|s| s.bg(color("--background-mod-normal")))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .gap(px(6.))
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--white"))
                                                .child(icon("invite", 16., "--white"))
                                                .child(dc_intl::t("VINpSK"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.open_invite_for(
                                                        target,
                                                        super::invite::InviteTarget::Stage,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                })),
                                        )
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_stage_preview(&mut self, cx: &mut Context<Self>) -> String {
        let Some(gid) = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
        else {
            return "stage-preview: no guild".to_owned();
        };
        let stage_id: Id = 9_999_000_062_3413;
        if let Some(guild) = self.stores.guilds.iter_mut().find(|g| g.id == gid)
            && !guild.channels.iter().any(|c| c.id == stage_id)
        {
            guild.channels.push(Channel {
                id: stage_id,
                name: "town-hall".to_owned(),
                kind: STAGE,
                position: 0,
                parent_id: None,
                last_message_id: None,
                overwrites: Vec::new(),
                user_limit: 0,
                topic: Some("Community Q&A & Roadmap".to_owned()),
                rate_limit_per_user: 0,
                flags: 0,
                nsfw: false,
                default_auto_archive_duration: None,
                available_tags: Vec::new(),
                default_sort_order: None,
                default_forum_layout: 0,
                default_reaction_emoji: None,
                linked_lobby_application_id: None,
            });
        }
        self.stores.stage_instances.dispatch(
            "STAGE_INSTANCE_CREATE",
            &json!({
                "id": (stage_id + 1).to_string(),
                "guild_id": gid.to_string(),
                "channel_id": stage_id.to_string(),
                "topic": "Community Q&A & Roadmap",
                "privacy_level": 2,
                "discoverable_disabled": false,
                "guild_scheduled_event_id": null
            }),
        );
        let sample_users: Vec<Id> = self.stores.users.keys().copied().take(3).collect();
        let entry = self.stores.voice_states.entry(gid).or_default();
        entry.retain(|s| s.channel_id != stage_id);
        for (i, uid) in sample_users.into_iter().enumerate() {
            entry.push(dc_state::VoiceState {
                user_id: uid,
                channel_id: stage_id,
                session_id: format!("stage-preview-{i}"),
                suppress: false,
                ..Default::default()
            });
        }
        self.call = None;
        self.select(gid, Some(stage_id));
        cx.notify();
        "stage-preview: opened".to_owned()
    }

    /// `content_YMXX4L`: 400 wide, centred in the page; the header (24px above, 72 high: a 32/40 title and a 20/24
    /// subtitle 8px under it), then the cards 24px under it and 8px apart.
    fn stage_waiting(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let target = channel.id;
        let moderator = self.can_moderate_stage(channel);
        let guild = self.guild;
        let here = self.stores.voice_members(guild.unwrap_or_default(), target);
        let audience = here.iter().filter(|state| state.suppress).count();
        div()
            .parity("stage-view")
            .group("stage-live")
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
                    .top_0()
                    .left_0()
                    .right_0()
                    .child(stage_header(
                        channel.name.clone(),
                        here.len() - audience,
                        audience,
                        None,
                        None,
                    )),
            )
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
                                    .mt(px(24.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    // Module 394412 with `showDismiss: false`, before the cards (module 895903).
                                    .children(self.event_prompt_cards(
                                        guild.unwrap_or_default(),
                                        target,
                                        false,
                                        cx,
                                    ))
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
                                            "events",
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
                                            // The first visit shows the welcome first; "Got it!" joins.
                                            // Module 940518: the welcome opens while the key `HasBeenInStageChannel`
                                            // is unset and sets it at once, not when the dialog closes.
                                            if !this.stage_welcome_seen {
                                                this.stage_welcome_seen = true;
                                                this.stage_welcome = Some(target);
                                            } else if let Some(guild) = guild {
                                                this.stage_left_start_screen = Some(target);
                                                this.request_join_voice(guild, target, cx);
                                            }
                                            cx.notify();
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
            .child(icon("inventory_chevron_right", 16., "--white"))
            .into_any_element()
    }

    /// The live Stage on black: `participants_ZW-j0k` over the speakers' and audience's sections, the controls under
    /// them (`bottomControls_rTsZa_`).
    fn stage_live(
        &self,
        channel: &Channel,
        topic: String,
        is_live: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let guild = self.guild.unwrap_or_default();
        let target = channel.id;
        let mut speakers = Vec::new();
        let mut audience = Vec::new();
        // Audience members whose hand is up (`request_to_speak_timestamp`).
        let mut hands: Vec<Id> = Vec::new();
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let moderator = self.can_moderate_stage(channel);
        let mut listening = false;
        for state in self.stores.voice_members(guild, target) {
            listening |= state.suppress && Some(state.user_id) == me;
            if state.suppress && state.request_to_speak_timestamp.is_some() {
                hands.push(state.user_id);
            }
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
        let audience_btn = self.stage_audience_button(channel, hands.len(), cx);
        let requests = self.stage_requests_toggle(channel, hands.len(), cx);
        let in_call = me.is_some_and(|id| {
            self.stores
                .voice_members(guild, target)
                .iter()
                .any(|s| s.user_id == id)
        });
        let hand = self.stage_hand_button(guild, channel, cx);
        let video_full =
            self.video_stage_full_notice(channel, (speakers.len() + audience.len()) as u64, cx);
        let sidebar = self.stage_requests_sidebar(channel, cx);
        let tile = |user: &dc_state::User, is_speaker: bool| {
            let picture = self.images.get(&super::avatar_source(user).0).cloned();
            let person = user.id;
            let raised = hands.contains(&person);
            let probe = format!("stage-audience-{person}");
            let click_probe = probe.clone();
            let is_stage_mod = is_speaker
                && self
                    .stores
                    .guild(guild)
                    .is_some_and(|g| g.viewing_roles.is_none() && g.owner_id == Some(person));
            let mod_badge = is_stage_mod.then(|| {
                div()
                    .id(gpui::SharedString::from(format!(
                        "stage-mod-badge-{person}"
                    )))
                    .aria_label(dc_intl::t("GMZqSi"))
                    .absolute()
                    .top(px(6.))
                    .right(px(6.))
                    .size(px(20.))
                    .rounded_full()
                    .bg(color("--background-surface-high"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("mod_chip_shield", 12., "--status-positive"))
            });
            div()
                .id(gpui::SharedString::from(if is_speaker {
                    format!("stage-speaker-{person}")
                } else {
                    probe.clone()
                }))
                .relative()
                .children(mod_badge)
                .flex_none()
                .w(px(102.))
                .h(px(98.))
                .rounded(px(8.))
                .bg(color("--black"))
                .flex()
                .flex_col()
                .items_center()
                .when(!is_speaker, |this| {
                    // Module 75139: left-click on an audience tile opens UserProfilePopout; right-click opens 107632
                    // with `showMediaItems`, `showStageChannelItems` and `showChatItems: false`.
                    this.cursor_pointer()
                        .child(self.anchor_probe(probe))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let bounds =
                                this.tip_anchors.borrow().get(click_probe.as_str()).copied();
                            if let Some(bounds) = bounds {
                                this.open_profile(person, bounds, true, cx);
                            }
                        }))
                        .on_mouse_up(
                            gpui::MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                cx.stop_propagation();
                                this.context_stage_audience();
                                this.open_context_menu(
                                    super::context_menu::Target::User {
                                        user: person,
                                        guild: Some(guild),
                                    },
                                    event.position,
                                    cx,
                                );
                            }),
                        )
                })
                .when(is_speaker, |this| {
                    // Module 540462 -> 107632: right-click on any speaker tile opens the full Guild call user menu
                    // with `showStageChannelItems: true`.
                    this.on_mouse_up(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                            cx.stop_propagation();
                            this.vv.shown_call = (guild, target);
                            this.open_tile_menu(
                                super::call_menus::Subject::User(person),
                                false,
                                event.position,
                                cx,
                            );
                        }),
                    )
                })
                .child(
                    div()
                        .mt(px(8.))
                        .size(px(56.))
                        .rounded_full()
                        .overflow_hidden()
                        .children(picture.map(|picture| img(picture).size(px(56.)))),
                )
                // The raised hand (unconfirmed look: no capture of a Stage with hands up).
                .children(raised.then(|| {
                    div()
                        .absolute()
                        .top(px(8.))
                        .right(px(14.))
                        .size(px(24.))
                        .rounded_full()
                        .bg(color("--background-base-lowest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("system_hand", 16., "--white"))
                }))
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
        let section = |title: String,
                       people: &[dc_state::User],
                       icon_name: &'static str,
                       is_speaker: bool| {
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
                        .children(people.iter().map(|user| tile(user, is_speaker))),
                )
                .into_any_element()
        };
        // stage-live and stage-live-hover: at rest the page shows neither the header nor the controls; the pointer over
        // the page brings both (`visible`).
        let header = stage_header(
            topic,
            speakers.len(),
            audience.len(),
            Some(audience_btn),
            requests,
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
        // The leave button's red is the notification red (210, 45, 57) in stage-ann/stage-leave-menu.
        let red = color("--background-feedback-notification");
        let menu_open = moderator && self.stage_leave_menu;
        // `End Stage for Everyone` (stage-ann/stage-leave-menu): a 202 by 53 popout above the caret, the item 37 high
        // with a red circle-x and red text.
        let leave_menu = menu_open.then(|| {
            div()
                .id("stage-leave-menu")
                .occlude()
                .absolute()
                .left(px(455.))
                .top(px(552.))
                .w(px(201.8))
                .h(px(54.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(Rgba {
                    a: 0.12,
                    ..rgba(0x6c6d76ff)
                })
                .bg(color("--neutral-76"))
                // `menu_M3qk4j`: 0 12px 24px rgba(0,0,0,.24).
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    if !this.keys.outside_closed_this_press {
                        this.keys.outside_closed_this_press = true;
                        this.stage_leave_menu = false;
                        if event.button != gpui::MouseButton::Right {
                            cx.stop_propagation();
                        }
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .id("stage-end-item")
                        .h(px(36.))
                        .px(px(8.))
                        .rounded(px(2.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.stage_leave_menu = false;
                            this.stage_end_confirm = Some(target);
                            cx.notify();
                        }))
                        .child(icon("circle_x_small", 18., "--red-new-28"))
                        .child(
                            div()
                                .ml(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--red-new-28"))
                                .child("End Stage for Everyone"),
                        ),
                )
        });
        let controls = div()
            .absolute()
            .when(!menu_open, |this| this.invisible())
            .group_hover("stage-live", |style| style.visible())
            .left_0()
            .right_0()
            .bottom(px(16.))
            .flex()
            .justify_center()
            // The group is 280 wide at x = 687.5; Chromium snaps that to 688, GPUI to 687.
            .pl(px(1.))
            .child(
                div()
                    .flex()
                    .child(
                        div()
                            .h(px(50.))
                            .px(px(4.))
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
                            .child(round("stage-sound", "voice_soundboard_off"))
                            .child(hand)
                            .child(round("stage-mic", "stage_mic_off"))
                            .child(round("stage-more", "voice_view_more")),
                    )
                    .child(
                        div()
                            .ml(px(12.))
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
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.stage_leave_menu = false;
                                        this.stage_left_start_screen = None;
                                        this.leave_stage_or_ask(guild, target, cx);
                                        cx.notify();
                                    }))
                                    .child(icon("voice_disconnect", 20., "--white")),
                            )
                            .children(moderator.then(|| {
                                div()
                                    .id("stage-leave-caret")
                                    .size(px(24.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.stage_leave_menu = !this.stage_leave_menu;
                                        cx.notify();
                                    }))
                                    .child(
                                        gpui::svg()
                                            .path("icons/voice_view_caret.svg")
                                            .size(px(16.))
                                            .text_color(color("--white"))
                                            .when(menu_open, |this| {
                                                this.with_transformation(
                                                    gpui::Transformation::rotate(gpui::radians(
                                                        std::f32::consts::PI,
                                                    )),
                                                )
                                            }),
                                    )
                            })),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .right(px(16.))
                    .bottom(px(13.))
                    .flex()
                    .gap(px(20.))
                    .child(
                        div()
                            .id("stage-popout-btn")
                            .cursor_pointer()
                            .child(icon("voice_view_popout", 24., "--white"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_call_popout(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("stage-fullscreen-btn")
                            .cursor_pointer()
                            .child(icon("voice_view_fullscreen", 24., "--white"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.vv.fullscreen = !this.vv.fullscreen;
                                window.toggle_fullscreen();
                                cx.notify();
                            })),
                    ),
            );
        // `popoverGradientWrapper` of stage-ann/stage-live-controls: 240 by 241 above the hand button (box (641, 404)),
        // 12px radius on #242429 under the blue header gradient (144.6 high), the graphic 144 by 81 32px down, the
        // heading 20px under it, the button 20px under that, 16px of padding.
        let art = self.images.get(COACH_ART).cloned();
        let coach = (listening && !self.stage_coach_closed).then(|| {
            div()
                .absolute()
                .invisible()
                .group_hover("stage-live", |style| style.visible())
                .left(px(266.))
                .top(px(372.))
                .w(px(240.))
                .h(px(241.))
                .rounded(px(12.))
                .overflow_hidden()
                // `popoverGradientWrapper_81I`: oklab(.26239 .0025 -.0089), (36, 36, 41) in the capture.
                .bg(rgba(0x242429ff))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .child(
                    img(header_gradient(
                        color("--expressive-gradient-blue-start"),
                        color("--expressive-gradient-blue-end"),
                    ))
                    .object_fit(gpui::ObjectFit::Fill)
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(240.))
                    .h(px(144.6)),
                )
                .child(
                    div()
                        .id("stage-coach-close")
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .size(px(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.stage_coach_closed = true;
                            cx.notify();
                        }))
                        .child(icon("x_small", 16., "--white")),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(32.))
                        .left(px(48.))
                        .w(px(144.))
                        .h(px(81.))
                        .children(art.map(|art| img(art).w(px(144.)).h(px(81.)))),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(133.))
                        .left(px(16.))
                        .w(px(208.))
                        .h(px(40.))
                        .text_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("zvubnM")),
                )
                .child(
                    div()
                        .id("stage-coach-confirm")
                        .absolute()
                        .top(px(193.))
                        .left(px(16.))
                        .w(px(208.))
                        .h(px(32.))
                        .rounded(px(8.))
                        .bg(color("--background-brand"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.stage_coach_closed = true;
                            this.open_birthday_modal(super::birthday::Source::DeepLinkPrompt, cx);
                        }))
                        .child(dc_intl::t("KXVgjt")),
                )
        });
        let view = div()
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
            // Module 366265 under the header (`eA` of module 482877).
            .children(
                video_full.map(|notice| div().flex_none().px(px(16.)).pt(px(8.)).child(notice)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .children(self.call_popped_out().then(|| {
                        // Module 482877 (`J5bXZU`): popped-out player notice.
                        div()
                            .my(px(24.))
                            .p(px(20.))
                            .rounded(px(8.))
                            .bg(color("--background-surface-high"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .text_center()
                            .text_size(px(15.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("J5bXZU"))
                    }))
                    // Module 410828 (`nM`): "No one is on stage yet" (`P63Z/o`) over Stage backdrop when in call with 0 speakers.
                    .children(
                        (!self.call_popped_out() && in_call && speakers.is_empty()).then(|| {
                            div()
                                .id("stage-empty-speakers")
                                .relative()
                                .flex_1()
                                .min_h(px(200.))
                                .my(px(12.))
                                .rounded(px(12.))
                                .overflow_hidden()
                                .bg(gpui::linear_gradient(
                                    180.,
                                    gpui::linear_color_stop(rgba(0x1f2136ff), 0.),
                                    gpui::linear_color_stop(rgba(0x111218ff), 1.),
                                ))
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .relative()
                                        .text_size(px(20.))
                                        .line_height(px(24.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("P63Z/o")),
                                )
                        }),
                    )
                    .children((!self.call_popped_out() && !speakers.is_empty()).then(|| {
                        section(
                            format!("{} \u{2013} {}", dc_intl::t("CduOkx"), speakers.len()),
                            &speakers,
                            "members",
                            true,
                        )
                    }))
                    .children((!self.call_popped_out()).then(|| {
                        section(
                            format!("{} \u{2013} {}", dc_intl::t("3foUu5"), audience.len()),
                            &audience,
                            "members",
                            false,
                        )
                    })),
            )
            .children(coach)
            // `caret_uDaE_S`: 22 by 14 under the box, centred on the hand button (750, 644).
            .children((listening && !self.stage_coach_closed).then(|| {
                div()
                    .absolute()
                    .invisible()
                    .group_hover("stage-live", |style| style.visible())
                    .left(px(self.combined_sidebar_width()))
                    .top(px(612.))
                    .child(
                        icon("popout_caret_down", 22., "--white")
                            .text_color(rgba(0x242429ff))
                            .h(px(14.)),
                    )
            }))
            // Module 717965 / 394412: when in the Stage room before going live (`!is_live`), render `.eventPrompts_Z_HbQM`
            // above the bottom control bar for speakers and moderators (`showDismiss: true` + "Start the Stage").
            .children({
                let is_speaker_me = me.is_some_and(|id| speakers.iter().any(|u| u.id == id));
                (!is_live && (is_speaker_me || moderator)).then(|| {
                    let prompts = self.event_prompt_cards(guild, target, true, cx);
                    div()
                        .id("stage-live-event-prompts")
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(80.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.))
                        .children(prompts)
                        .children(moderator.then(|| {
                            div().w(px(400.)).child(self.stage_card(
                                "stage-prelive-start-card",
                                "stage_channel",
                                "Start the Stage",
                                "Pick a topic to start the conversation.",
                                cx.listener(move |this, _, _, cx| {
                                    this.open_start_stage(target, cx)
                                }),
                            ))
                        }))
                })
            })
            .child(controls)
            .children(leave_menu)
            .children(self.tile_menu_layer(cx));
        match sidebar {
            // The requests sidebar of module 374873 at the Stage's right.
            Some(sidebar) => div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .child(view)
                .child(sidebar)
                .into_any_element(),
            None => view.into_any_element(),
        }
    }

    /// The people in the open Stage channel, whose pictures the tiles draw.
    pub(super) fn stage_people(&self) -> Vec<dc_state::User> {
        let (Some(guild), Some(channel)) = (self.guild, self.channel) else {
            return Vec::new();
        };
        self.stores
            .voice_members(guild, channel)
            .into_iter()
            .filter_map(|state| self.stores.users.get(&state.user_id).cloned())
            .collect()
    }

    /// Modules 106401 and 237749 (`107632` Group 1): the Stage actions offered for `user` in `guild`.
    pub(super) fn stage_user_acts(
        &self,
        guild: Id,
        user: Id,
        stage_channel_items: bool,
    ) -> Vec<super::context_menu::Act> {
        use super::context_menu::Act;
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let Some(state) = self
            .stores
            .voice_states
            .get(&guild)
            .and_then(|states| states.iter().find(|state| state.user_id == user))
        else {
            return Vec::new();
        };
        let channel_id = state.channel_id;
        if channel_id == 0 {
            return Vec::new();
        }
        let Some(channel) = self
            .stores
            .guild(guild)
            .and_then(|known| known.channels.iter().find(|ch| ch.id == channel_id))
            .filter(|ch| Self::is_stage_channel(ch))
        else {
            return Vec::new();
        };
        const MUTE_MEMBERS: u64 = 1 << 22;
        const MOVE_MEMBERS: u64 = 1 << 24;
        let moderator = [permissions::MANAGE_CHANNELS, MUTE_MEMBERS, MOVE_MEMBERS]
            .into_iter()
            .all(|perm| self.my_permission(guild, Some(channel), perm));
        let can_mute = self.my_permission(guild, Some(channel), MUTE_MEMBERS);
        let own = Some(user) == me;
        let mut out = Vec::new();
        // Module 106401 (`id: "invite-speaker"`): offered whenever the person is suppressed in the Stage channel.
        if state.suppress {
            if moderator {
                out.push(if own {
                    Act::StageSpeak
                } else {
                    Act::StageInvite
                });
            } else if own && state.request_to_speak_timestamp.is_some() {
                out.push(Act::StageSpeak);
            }
        }
        // Module 237749 (`id: "audience"`): offered when `showStageChannelItems` and the person is speaking.
        if stage_channel_items && !state.suppress && (can_mute || own) {
            out.push(if own {
                Act::StageLeave
            } else {
                Act::StageMoveToAudience
            });
        }
        out
    }

    /// `Tf`, `gt`, `SA` and `e7` of module 849736: the voice-state patches behind the Stage context rows.
    pub(super) fn run_stage_user_act(
        &mut self,
        act: super::context_menu::Act,
        guild: Id,
        user: Id,
    ) {
        use super::context_menu::Act;
        let Some(channel) = self
            .stores
            .voice_states
            .get(&guild)
            .and_then(|states| states.iter().find(|state| state.user_id == user))
            .map(|state| state.channel_id)
            .filter(|&id| id != 0)
        else {
            return;
        };
        let patch = |this: &mut Self, route: String, body: serde_json::Value| {
            let _ = this.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route,
                body: Some(body),
            });
        };
        let own = format!("/guilds/{guild}/voice-states/@me");
        let theirs = format!("/guilds/{guild}/voice-states/{user}");
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        match act {
            Act::StageLeave => patch(
                self,
                own,
                json!({"suppress": true, "channel_id": channel.to_string(),
                       "self_video": false, "self_stream": false}),
            ),
            Act::StageSpeak => patch(
                self,
                own,
                json!({"suppress": false, "request_to_speak_timestamp": null,
                       "channel_id": channel.to_string()}),
            ),
            Act::StageInvite => patch(
                self,
                theirs,
                json!({"suppress": false, "request_to_speak_timestamp": now,
                       "channel_id": channel.to_string()}),
            ),
            Act::StageMoveToAudience => {
                // `gt` sends the plain patch (`O`) and then the one that also drops video and screen share.
                patch(
                    self,
                    theirs.clone(),
                    json!({"suppress": true, "channel_id": channel.to_string()}),
                );
                patch(
                    self,
                    theirs,
                    json!({"suppress": true, "channel_id": channel.to_string(),
                           "self_video": false, "self_stream": false}),
                );
            }
            _ => {}
        }
    }

    /// `tQ` of module 993838: the Start the Stage modal, or Change the topic when the Stage is live. For the menu
    /// and the view.
    /// `E` of module 918192 (`blockedNotice_jBt7uB`, 24 under it): the deny icon in `--red-400` (blocked people,
    /// or both) or the eye-slash (only ignored), the line in `body-xs/normal`, and View all (`blockedButton_jBt7uB`:
    /// the link colour, 12 px semibold, 4 px after the line), whose popout lists them on the left, bottom-aligned
    /// (`container_jBt7uB`: `--background-surface-high`, 4 px corners, the border and high shadows, at most 300 high,
    /// 8 px inside). `None` without blocked or ignored people in the channel.
    fn stage_blocked_notice(
        &self,
        channel: Id,
        list_open: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let guild = self.stores.guild_of_channel(channel)?.id;
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let states = self.stores.voice_members(guild, channel);
        let others = states.iter().filter(|state| Some(state.user_id) != me);
        let blocked: Vec<Id> = others
            .clone()
            .filter(|state| self.is_blocked(state.user_id))
            .map(|state| state.user_id)
            .collect();
        let ignored: Vec<Id> = others
            .filter(|state| {
                !self.is_blocked(state.user_id) && self.stores.is_ignored(state.user_id)
            })
            .map(|state| state.user_id)
            .collect();
        if blocked.is_empty() && ignored.is_empty() {
            return None;
        }
        let number = |count: usize| [("number", (count as i64).into())];
        let (glyph, text) = if !blocked.is_empty() && !ignored.is_empty() {
            ("deny_circle", dc_intl::t("P/KFXz").to_owned())
        } else if !ignored.is_empty() {
            (
                "eye_slash",
                dc_intl::format("Ri3o33", &number(ignored.len())),
            )
        } else {
            (
                "deny_circle",
                dc_intl::format("6Tcdt7", &number(blocked.len())),
            )
        };
        let tint = if glyph == "deny_circle" {
            "--red-400"
        } else {
            "--interactive-icon-default"
        };
        let popout = list_open.then(|| {
            let rows: Vec<AnyElement> = blocked
                .iter()
                .map(|user| self.stage_person_row(guild, channel, *user, true))
                .chain(
                    ignored
                        .iter()
                        .map(|user| self.stage_person_row(guild, channel, *user, false)),
                )
                .collect();
            div()
                .id("stage-blocked-list")
                .occlude()
                .absolute()
                .bottom_0()
                .right(gpui::relative(1.))
                .min_w(px(240.))
                .max_h(px(300.))
                .overflow_y_scroll()
                .p(px(8.))
                .rounded(px(4.))
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
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if let Some(open) = this.start_stage.as_mut() {
                        open.blocked_list = false;
                    }
                    cx.notify();
                }))
                .children(rows)
        });
        Some(
            div()
                .mb(px(24.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(icon(glyph, 24., tint))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-default"))
                                .child(text),
                        )
                        .child(
                            div()
                                .relative()
                                .child(
                                    div()
                                        .id("stage-blocked-view-all")
                                        .ml(px(4.))
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-link"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(open) = this.start_stage.as_mut() {
                                                open.blocked_list = !open.blocked_list;
                                            }
                                            cx.notify();
                                        }))
                                        .child(dc_intl::t("rUEjBe")),
                                )
                                .children(popout),
                        ),
                )
                .into_any_element(),
        )
    }

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
            blocked_list: false,
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
        // Under `GRAVECORD_INERT=1` the request is refused and logged as `inert: refused Rest ...`.
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
                    // `image_sYj23M` of stage-ann/stage-start-modal-typed: 162 by 108 at (558, 145).
                    .left(px(158.))
                    .child(img(art).w(px(162.)).h(px(108.)))
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
        // `control_VhFsFg` (stage-ann/stage-start-modal-typed): 40 high, a 1px ring that is brand while the field has
        // the focus, black at .12 inside, the text 12px in.
        let focused = open.field.read(cx).focus_handle(cx).is_focused(window);
        let input = div()
            .w_full()
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(if over {
                color("--text-feedback-critical")
            } else if focused {
                color("--brand-500")
            } else {
                Rgba { a: 0., ..rgba(0) }
            })
            .bg(Rgba {
                a: 0.12,
                ..rgba(0x000000ff)
            })
            .px(px(12.))
            .flex()
            .items_center()
            .child(div().flex_1().child(open.field.clone()));
        // `ageVerificationNotice_CWLnwx`: a warning box, 62 high with 12px of padding, whose link is drawn only
        // ("Confirm age" is never pressed). Shown always: the account's age state is not read.
        let age_notice = div()
            .mt(px(8.))
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(Rgba {
                a: 0.32,
                ..rgba(0xe8a838ff)
            })
            .bg(Rgba {
                a: 0.08,
                ..rgba(0xe8a838ff)
            })
            .p(px(12.))
            .flex()
            .items_start()
            .child(
                div()
                    .flex_none()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .child(icon("warning_triangle", 16., "--text-feedback-warning")),
            )
            .child(
                div()
                    .ml(px(8.))
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-strong"))
                    .child(gpui::StyledText::new(AGE_NOTICE).with_highlights([(
                        AGE_NOTICE.len() - AGE_LINK.len()..AGE_NOTICE.len(),
                        gpui::HighlightStyle {
                            underline: Some(gpui::UnderlineStyle {
                                thickness: px(1.),
                                color: None,
                                wavy: false,
                            }),
                            ..Default::default()
                        },
                    )])),
            );
        // `blockedUsersContainer_CWLnwx` (module 128347): for a Stage not live yet with blocked or ignored people in it,
        // the notice of module 918192.
        let blocked_notice = (!open.editing)
            .then(|| self.stage_blocked_notice(open.channel, open.blocked_list, cx))
            .flatten();
        let body = div()
            .flex_none()
            .px(px(32.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(blocked_notice)
            .child(
                div()
                    .h(px(20.))
                    .flex()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    // `5FPBOB`, with the required mark.
                    .child("Stage Topic")
                    .child(
                        div()
                            .ml(px(4.))
                            .text_color(color("--text-feedback-critical"))
                            .child("*"),
                    ),
            )
            .child(input)
            .child(age_notice)
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
                        // `checkboxIndicator_vTUVyc`: 20 square, a 4px radius, a 1px ring at .12 and brand inside when on.
                        div()
                            .size(px(20.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(Rgba {
                                a: 0.12,
                                ..rgba(0x97979fff)
                            })
                            .when(notify, |this| this.bg(color("--brand-500")))
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
                    .w(px(218.6))
                    .child(div().w(px(100.)).flex().child(modal::footer_button(
                        modal::button(
                            "start-stage-cancel",
                            "Cancel",
                            modal::Tone::Secondary,
                            |this, _, _| {
                                this.start_stage = None;
                            },
                        ),
                        cx,
                    )))
                    .child(div().w(px(110.6)).flex().child(modal::footer_button(
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
                    ))),
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

    /// The end question (stage-ann/stage-end-confirm): the small dialog (400 wide) with a critical End Stage.
    /// `DELETE /stage-instances/<channel>` (module 993838's end); the harness never sends it.
    pub(super) fn stage_end_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.stage_end_confirm?;
        Some(modal::render_small(
            modal::Spec {
                id: "stage-end-modal",
                title: "Are you sure you want to end the Stage for everyone?".into(),
                subtitle: Some(
                    div()
                        .child("The Stage will end immediately.")
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    modal::button(
                        "stage-end-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| {
                            this.stage_end_confirm = None;
                        },
                    ),
                    modal::button(
                        "stage-end-confirm",
                        "End Stage",
                        modal::Tone::Critical,
                        move |this, _, cx| {
                            this.stage_end_confirm = None;
                            // Under `GRAVECORD_INERT=1` the request is refused and logged.
                            let _ = this.requests.send(Request::Rest {
                                key: None,
                                method: reqwest::Method::DELETE,
                                route: format!("/stage-instances/{channel}"),
                                body: None,
                            });
                            cx.notify();
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.stage_end_confirm = None),
            },
            cx,
        ))
    }

    /// "Welcome to Stages!" (stage-ann/stage-welcome-modal): the start modal's shell without its field, 538 high: the
    /// header (216 high, title at 168), three 40px bullets 24px apart, "Got it!" full width 32px under them.
    pub(super) fn stage_welcome_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let target = self.stage_welcome?;
        let art = self.images.get(GRAPHIC).cloned();
        let gradient = header_gradient(
            color("--expressive-gradient-purple-start"),
            color("--expressive-gradient-purple-end"),
        );
        let pose = super::motion::modal_pose("stage-welcome-modal");
        let bullet = |glyph: &'static str, tint: &'static str, text: &'static str| {
            div()
                .flex_none()
                .h(px(40.))
                .mb(px(24.))
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_none()
                        .size(px(40.))
                        .rounded_full()
                        .bg(color("--background-base-lowest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(glyph, 20., tint)),
                )
                .child(
                    div()
                        .ml(px(16.))
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(text),
                )
        };
        let header = div()
            .relative()
            .flex_none()
            .w(px(HEADER_WIDTH as f32))
            .h(px(216.))
            .child(
                img(gradient)
                    .object_fit(gpui::ObjectFit::Fill)
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w(px(HEADER_WIDTH as f32))
                    .h(px(216.)),
            )
            .children(art.map(|art| {
                div()
                    .absolute()
                    .top(px(36.))
                    .left(px(158.))
                    .child(img(art).w(px(162.)).h(px(108.)))
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
                    .child("Welcome to Stages!"),
            );
        let body = div()
            .flex_none()
            .px(px(32.))
            .child(bullet(
                "mic_muted",
                "--text-feedback-critical",
                "Your mic will be muted unless you are invited to speak.",
            ))
            .child(bullet(
                "voice_disconnect",
                "--white",
                "You can leave and rejoin anytime without making noise.",
            ))
            .child(bullet(
                "system_hand",
                "--white",
                "Request to speak to join the conversation!",
            ));
        let got_it = move |this: &mut Self, cx: &mut Context<Self>| {
            this.stage_welcome = None;
            this.stage_welcome_seen = true;
            if let Some(guild) = this.guild {
                this.stage_left_start_screen = Some(target);
                this.request_join_voice(guild, target, cx);
            }
        };
        let panel =
            div()
                .id("stage-welcome-modal")
                .opacity(pose.opacity)
                .paint_scale(pose.scale, gpui::point(0.5, 0.5))
                .occlude()
                .relative()
                .w(px(MODAL_WIDTH))
                .pt(px(8.))
                .pb(px(32.))
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
                .child(div().flex_none().mt(px(32.)).px(px(32.)).flex().child(
                    modal::footer_button(
                        modal::button(
                            "stage-welcome-got-it",
                            "Got it!",
                            modal::Tone::Primary,
                            move |this, _, cx| got_it(this, cx),
                        ),
                        cx,
                    ),
                ))
                .child(
                    super::keyboard::inset_ring_when_focused(div().id("stage-welcome-close"))
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
                            this.stage_welcome = None;
                            this.stage_welcome_seen = true;
                            cx.notify();
                        })),
                );
        Some(
            deferred(
                div()
                    .id("stage-welcome-modal-layer")
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
                        this.stage_welcome = None;
                        cx.notify();
                    }))
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `channelNotice_hVh8FI` under the server name: "Live Now", the topic and where it is (stage-ann/stage-live: a
    /// 303 by 85 block with 16px of padding, the green dot at (88, 94), the 12/16 line, the 16/20 semibold title 4px
    /// under it and the channel line 8px under that). Closed with its X; nothing is sent.
    pub(super) fn live_now_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.current_guild()?;
        let (channel, instance) = guild.channels.iter().find_map(|channel| {
            (channel.kind == STAGE)
                .then(|| self.stores.stage_instances.get_by_channel(channel.id))
                .flatten()
                .map(|instance| (channel, instance))
        })?;
        if self.live_now_closed.contains(&instance.id) {
            return None;
        }
        let audience = self
            .stores
            .voice_members(guild.id, channel.id)
            .into_iter()
            .filter(|state| state.suppress)
            .count();
        let target = channel.id;
        let closing = instance.id;
        let line = |text: String| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--channels-default"))
                .child(text)
        };
        Some(
            div()
                .flex_none()
                .w(px(self.sidebar_width()))
                .h(px(85.))
                .pl(px(16.))
                .pr(px(16.))
                .pt(px(8.))
                .border_b_1()
                .border_color(color("--border-subtle"))
                .child(
                    div()
                        .h(px(16.))
                        .flex()
                        .items_center()
                        .child(div().size(px(8.)).rounded_full().bg(color("--green-360")))
                        .child(
                            div()
                                .ml(px(4.))
                                .flex_1()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-feedback-positive"))
                                .child("Live Now"),
                        )
                        .child(
                            div()
                                .id("live-now-close")
                                .w(px(20.))
                                .h(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.live_now_closed.push(closing);
                                    cx.notify();
                                }))
                                .child(icon("x_small", 16., "--channels-default")),
                        ),
                )
                .child(
                    div()
                        .id("live-now-topic")
                        .mt(px(4.))
                        .h(px(24.))
                        .pt(px(3.))
                        .cursor_pointer()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(instance.topic.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(guild) = this.guild {
                                this.select(guild, Some(target));
                            }
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .h(px(16.))
                        .flex()
                        .items_center()
                        .child(icon("stage_channel", 16., "--channels-default"))
                        .child(div().ml(px(4.)).child(line(channel.name.clone())))
                        .child(div().mx(px(4.)).child(line("\u{2022}".to_owned())))
                        .child(icon("headphones", 16., "--channels-default"))
                        .child(
                            div()
                                .ml(px(2.))
                                .child(line(format!("{audience} in audience"))),
                        ),
                )
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

impl Root {
    /// The audience's hand button of the Stage's controls (`eI` of module 717965, its state from module 361619). Its
    /// label: "Speaker abilities are only for confirmed adults" while the account must confirm its age (`Vv` of module
    /// 418208: READY `age_gated_features` bit 128 + `age_verification_status`), else Cancel Request
    /// (green) while the hand is up, Request to Speak, or Raise Hand Disabled (and disabled) when this member may not
    /// ask (`REQUEST_TO_SPEAK`, module 370228). Under the age gate, clicking opens the age confirmation dialog;
    /// otherwise a click raises or lowers the hand (`J7` of module 849736: `request_to_speak_timestamp` now or null).
    fn stage_hand_button(
        &self,
        guild: Id,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let mine = self
            .stores
            .voice_members(guild, channel.id)
            .into_iter()
            .find(|state| Some(state.user_id) == me)
            .cloned();
        let raised = mine
            .as_ref()
            .is_some_and(|state| state.request_to_speak_timestamp.is_some());
        let may_ask = self.my_permission(
            guild,
            Some(channel),
            dc_state::permissions::REQUEST_TO_SPEAK,
        );
        let age_gate = may_ask && self.stores.account.stage_speaking_age_gated();
        let label = hand_label(age_gate, raised, may_ask);
        let disabled = !may_ask && !raised;
        let in_stage = mine.is_some() && !age_gate;
        let target = channel.id;
        let button = div()
            .id("stage-hand")
            .size(px(40.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .when(raised, |this| this.bg(color("--control-connected-background-default")))
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
            })
            .child(icon("stage_hand_raise", 20., "--white"))
            .when(age_gate && !disabled, |this| {
                this.on_click(cx.listener(|this, _, _, cx| {
                    this.open_birthday_modal(super::birthday::Source::DeepLinkPrompt, cx);
                }))
            })
            .when(in_stage && !disabled, |this| {
                this.on_click(cx.listener(move |this, _, _, cx| {
                    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                    let body = if raised {
                        json!({"request_to_speak_timestamp": null, "channel_id": target.to_string()})
                    } else {
                        json!({"request_to_speak_timestamp": now, "channel_id": target.to_string()})
                    };
                    let _ = this.requests.send(Request::Rest {
                        key: None,
                        method: reqwest::Method::PATCH,
                        route: format!("/guilds/{guild}/voice-states/@me"),
                        body: Some(body),
                    });
                    cx.notify();
                }))
            });
        self.with_tip(
            "stage-hand-tip",
            super::tooltip::Tip::body(
                super::tooltip::Body::Text(label.into()),
                super::tooltip::Side::Top,
            ),
            button,
            cx,
        )
        .into_any_element()
    }
}

/// Module 623413: grid width for up to 5 speaker tiles (`168px` per column, 2 columns when 4
/// speakers else up to 3 columns).
pub(super) fn stage_speaker_grid_width(count: usize) -> f32 {
    let cols = match count {
        0 => 1,
        1..=3 => count,
        4 => 2,
        _ => 3,
    };
    168. * (cols as f32)
}

/// Module 623413: speaking summary copy (`FUVhyC` / `EQwZlN` / `zBcKoA` / `3AqFaG`).
pub(super) fn stage_speaking_summary(speakers: &[String]) -> String {
    match speakers {
        [] => dc_intl::t("FUVhyC").to_owned(),
        [a] => dc_intl::format("EQwZlN", &[("a", a.as_str().into())]),
        [a, b] => dc_intl::format(
            "zBcKoA",
            &[("a", a.as_str().into()), ("b", b.as_str().into())],
        ),
        [a, b, rest @ ..] => dc_intl::format(
            "3AqFaG",
            &[
                ("a", a.as_str().into()),
                ("b", b.as_str().into()),
                ("n", (rest.len() as f64).into()),
            ],
        ),
    }
}

/// The label of the hand button (`label` of `eI`).
pub(super) fn hand_label(age_gate: bool, raised: bool, may_ask: bool) -> &'static str {
    if age_gate {
        dc_intl::t("NzITVo")
    } else if raised {
        dc_intl::t("GCimTk")
    } else if may_ask {
        dc_intl::t("hLbG5N")
    } else {
        dc_intl::t("e4WMvx")
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
            default_reaction_emoji: None,
            linked_lobby_application_id: None,
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

#[cfg(test)]
mod hand_tests {
    #[test]
    fn the_hand_button_names_its_state() {
        assert_eq!(
            super::hand_label(true, true, true),
            "Speaker abilities are only for confirmed adults"
        );
        assert_eq!(super::hand_label(false, true, false), "Cancel Request");
        assert_eq!(super::hand_label(false, false, true), "Request to Speak");
        assert_eq!(
            super::hand_label(false, false, false),
            "Raise Hand Disabled"
        );
    }

    #[test]
    fn not_connected_stage_card_623413_summary_and_grid() {
        for k in ["FUVhyC", "EQwZlN", "zBcKoA", "3AqFaG", "7vb2cc", "TVBCKZ"] {
            assert!(dc_intl::has_key(k), "missing 623413 key {k}");
        }
        assert_eq!(
            super::stage_speaking_summary(&[]),
            "No one is currently speaking"
        );
        assert!(super::stage_speaking_summary(&["Alice".into()]).contains("Alice"));
        let two = super::stage_speaking_summary(&["Alice".into(), "Bob".into()]);
        assert!(two.contains("Alice") && two.contains("Bob"));
        let many = super::stage_speaking_summary(&[
            "Alice".into(),
            "Bob".into(),
            "Carol".into(),
            "Dave".into(),
        ]);
        assert!(many.contains("Alice") && many.contains("Bob") && many.contains("2"));
        assert_eq!(super::stage_speaker_grid_width(3), 504.);
        assert_eq!(super::stage_speaker_grid_width(4), 336.);
        assert_eq!(super::stage_speaker_grid_width(5), 504.);
    }
}
