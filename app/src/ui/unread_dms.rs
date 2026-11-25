//! Private DM rail entries, live RTC upper badges, and diagonal group avatars (396655/478063/486020/598104/771640/862328/941971).
use super::{
    Root, color, context_menu,
    tooltip::{Body, Tip},
    unread_badges,
};
use crate::parity::Parity;
use base64::Engine as _;
use dc_state::{Id, PrivateChannel, Stores, User};
use gpui::{
    AnyElement, Context, Image, ImageFormat, IntoElement, RenderImage, SharedString, div, img,
    prelude::*, px,
};
use md5::{Digest, Md5};
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "capture")]
const FIXTURE_DM: u64 = 1568693333864480939;
const GROUP_DEFAULTS: [&str; 8] = [
    "b8c21efe7946ee22.png",
    "392f18c1d7fa4cf1.png",
    "f06f874912a9a404.png",
    "84ada88a442f014d.png",
    "adc958faf2495e83.png",
    "e8a3e602806eb4e0.png",
    "2ccd0ae2e8934844.png",
    "95fad9fee060f9ae.png",
];

fn source(url: String) -> (String, String) {
    (format!("rail-image:{:x}", Md5::digest(url.as_bytes())), url)
}

pub(super) fn avatar(user: &User, size: u32) -> (String, String) {
    source(match &user.avatar {
        Some(hash) => format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.webp?size={size}",
            user.id
        ),
        None => format!(
            "https://cdn.discordapp.com/embed/avatars/{}.png",
            user.default_avatar_index()
        ),
    })
}

pub(super) fn group_icon(channel: &PrivateChannel) -> (String, String) {
    let url = if let Some(hash) = channel.icon.as_ref() {
        if hash.starts_with("data:") {
            hash.clone()
        } else {
            match channel.application_id {
                Some(app) => {
                    format!("https://cdn.discordapp.com/app-icons/{app}/{hash}.png?size=32")
                }
                None => format!(
                    "https://cdn.discordapp.com/channel-icons/{}/{hash}.png?size=32",
                    channel.id
                ),
            }
        }
    } else {
        let file = if channel.application_id.is_some() {
            "aa9bb230adf0286f.png"
        } else {
            // The snowflake epoch is divisible by this eight-image table's length.
            GROUP_DEFAULTS[((channel.id >> 22) % 8) as usize]
        };
        format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN)
    };
    source(url)
}

fn uses_diagonal_facepile(channel: &PrivateChannel) -> bool {
    channel.kind == 3 && channel.icon.is_none() && channel.recipients.len() >= 2
}

fn show_left_pill(animating: bool, selected: bool, hovered: bool, mentions: u64) -> bool {
    !animating && (selected || hovered || mentions > 0)
}

fn decode_data_url_bytes(data_url: &str) -> Option<(&str, Vec<u8>)> {
    let rest = data_url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta.split(';').next().unwrap_or("");
    if !meta
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"))
    {
        return None;
    }
    let clean: String = payload
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&clean)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&clean))
        .ok()?;
    Some((mime, bytes))
}

/// Decodes a `data:image/...;base64,...` URI into a BGRA `gpui::RenderImage`.
fn decode_data_render_image(data_url: &str) -> Option<Arc<RenderImage>> {
    let (_, bytes) = decode_data_url_bytes(data_url)?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    let mut rgba = decoded.into_rgba8();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
}

/// Decodes a `data:image/...;base64,...` URI into an `Arc<gpui::Image>` for `Root::images`.
fn decode_data_image(data_url: &str) -> Option<Arc<Image>> {
    let (mime, bytes) = decode_data_url_bytes(data_url)?;
    if mime.eq_ignore_ascii_case("image/svg+xml") {
        return Some(Arc::new(Image::from_bytes(ImageFormat::Svg, bytes)));
    }
    let decoded = image::load_from_memory(&bytes).ok()?;
    let mut png = Vec::new();
    decoded
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(Arc::new(Image::from_bytes(ImageFormat::Png, png)))
}

fn compute_rail_dm_entries(
    stores: &Stores,
    active_dm_call: Option<Id>,
) -> Vec<(PrivateChannel, u64)> {
    let mut entries: Vec<(PrivateChannel, u64)> =
        dc_state::private_channels::unread_channels(stores)
            .into_iter()
            .map(|channel| (channel.clone(), stores.read_states.mentions(channel.id)))
            .collect();
    // Module 396655: active connected DM call (`call.guild == 0`) is unconditionally pinned to index 0,
    // deduplicating any unread/mention entry for the same channel further down the rail.
    if let Some(call_channel) = active_dm_call
        && let Some(channel) = stores
            .private_channels
            .iter()
            .find(|channel| channel.id == call_channel)
    {
        entries.retain(|(existing, _)| existing.id != call_channel);
        entries.insert(
            0,
            (channel.clone(), stores.read_states.mentions(call_channel)),
        );
    }
    entries
}

/// Computes the upper call/video/stream badge `(icon_name, bg_token)` for a private channel rail item
/// (`396655:tO`, `771640:oi`, `628983`).
fn compute_upper_badge(
    stores: &Stores,
    channel_id: Id,
    is_connected: bool,
    own_camera: bool,
    own_stream: bool,
) -> Option<(&'static str, &'static str)> {
    let voice_states = stores.voice_members(0, channel_id);
    let has_voice_users = !voice_states.is_empty() || stores.calls.is_call_active(channel_id, None);
    if !is_connected && !has_voice_users {
        return None;
    }
    let screenshare =
        voice_states.iter().any(|state| state.self_stream) || (is_connected && own_stream);
    let video = screenshare
        || voice_states.iter().any(|state| state.self_video)
        || (is_connected && own_camera);
    let glyph = if screenshare {
        "rail_stream_badge"
    } else if video {
        "rail_camera_badge"
    } else {
        "voice_channel"
    };
    let bg = if is_connected {
        "--status-positive"
    } else {
        "--icon-overlay-dark"
    };
    Some((glyph, bg))
}

/// The people a DM's tooltip lists (module 396655 `tg`): those in an Activity in it, those streaming in it who are
/// not in the Activity, and those in its call who are in neither, each without blocked or ignored users and without
/// users the client does not know (module 297334 `aw`), in the order the stores give them (module 907459 `L` sorts only
/// under the `2025-08-voice-user-affinity` experiment, whose default is off).
pub(super) struct DmTooltipRows {
    pub voice: Vec<Id>,
    pub stream: Vec<Id>,
    pub activity: Vec<Id>,
}

pub(super) fn dm_tooltip_rows(
    voice: &[(Id, bool)],
    activity: &[Id],
    hidden: impl Fn(Id) -> bool,
) -> DmTooltipRows {
    let mut seen_activity: Vec<Id> = Vec::new();
    for user in activity {
        if !hidden(*user) && !seen_activity.contains(user) {
            seen_activity.push(*user);
        }
    }
    let mut stream: Vec<Id> = Vec::new();
    for (user, streaming) in voice {
        if *streaming && !hidden(*user) && !seen_activity.contains(user) && !stream.contains(user) {
            stream.push(*user);
        }
    }
    let mut in_call: Vec<Id> = Vec::new();
    for (user, _) in voice {
        if !hidden(*user)
            && !seen_activity.contains(user)
            && !stream.contains(user)
            && !in_call.contains(user)
        {
            in_call.push(*user);
        }
    }
    DmTooltipRows {
        voice: in_call,
        stream,
        activity: seen_activity,
    }
}

/// What the 24px avatar list of module 104171 (`max: 6`) draws for `count` people: how many avatars, and the
/// "+n" pill after them (`renderMoreUsers`: up to 99).
pub(super) fn summary_layout(count: usize, max: usize) -> (usize, Option<usize>) {
    let room = if count == max { count } else { max - 1 };
    let shown = room.min(count);
    let more = (shown < count).then(|| (count - shown).min(99));
    (shown, more)
}

thread_local! {
    /// The rail's items as last drawn, so that one that goes away can be drawn while it leaves.
    static RAIL_CACHE: std::cell::RefCell<HashMap<u64, (PrivateChannel, u64)>> =
        std::cell::RefCell::new(HashMap::new());
    /// Decoded BGRA `RenderImage` cache for optimistic `data:image/...;base64,...` group icons.
    static DATA_RENDER_CACHE: std::cell::RefCell<HashMap<String, Option<Arc<RenderImage>>>> =
        std::cell::RefCell::new(HashMap::new());
}

fn cached_data_render_image(key: &str, data_url: &str) -> Option<Arc<RenderImage>> {
    DATA_RENDER_CACHE.with(|cache| {
        if let Some(found) = cache.borrow().get(key) {
            return found.clone();
        }
        let decoded = decode_data_render_image(data_url);
        cache.borrow_mut().insert(key.to_owned(), decoded.clone());
        decoded
    })
}

impl Root {
    fn rail_dm_entries(&self) -> Vec<(PrivateChannel, u64)> {
        #[cfg(feature = "capture")]
        if let Some((count, group)) = self.capture_dm_rail {
            if count == 0 {
                return Vec::new();
            }
            let Some(mut channel) = self
                .stores
                .private_channels
                .iter()
                .find(|channel| channel.id == FIXTURE_DM)
                .cloned()
            else {
                return Vec::new();
            };
            if group {
                channel.kind = 3;
                channel.name = Some("fixture group".into());
                channel.icon = None;
                channel.application_id = None;
                if let Some(me) = self.stores.current_user.as_ref() {
                    channel.recipients.truncate(1);
                    channel.recipients.push(me.id);
                }
            }
            return vec![(channel, count)];
        }
        let active_dm_call = self
            .call
            .as_ref()
            .filter(|call| call.guild == 0)
            .map(|call| call.channel);
        compute_rail_dm_entries(&self.stores, active_dm_call)
    }

    pub(super) fn request_rail_dm_images(&mut self) {
        let mut images = Vec::new();
        for (channel, _) in self.rail_dm_entries() {
            if channel.kind == 1 {
                if let Some(user) = self.stores.dm_recipient(&channel) {
                    images.push(avatar(user, 48));
                }
            } else if uses_diagonal_facepile(&channel) {
                images.extend(
                    channel
                        .recipients
                        .iter()
                        .take(2)
                        .filter_map(|id| self.stores.users.get(id))
                        .map(|user| avatar(user, 32)),
                );
            } else {
                images.push(group_icon(&channel));
            }
            // The 24px avatars of its tooltip.
            let rows = self.dm_rows(channel.id);
            images.extend(
                rows.activity
                    .iter()
                    .chain(&rows.stream)
                    .chain(&rows.voice)
                    .filter_map(|id| self.dm_tooltip_user(*id))
                    .map(|user| avatar(&user, 24)),
            );
        }
        for (key, url) in images {
            if url.starts_with("data:") {
                if let std::collections::hash_map::Entry::Vacant(entry) = self.images.entry(key) {
                    let _ = cached_data_render_image(entry.key(), &url);
                    if let Some(decoded) = decode_data_image(&url) {
                        entry.insert(decoded);
                    }
                }
            } else {
                self.request_image(key, url);
            }
        }
    }

    /// A user the client knows (module 297334 `Vq`), the account included.
    fn dm_tooltip_user(&self, id: Id) -> Option<User> {
        self.stores.users.get(&id).cloned().or_else(|| {
            self.stores
                .current_user
                .clone()
                .filter(|current| current.id == id)
        })
    }

    fn dm_rows(&self, channel: Id) -> DmTooltipRows {
        let voice: Vec<(Id, bool)> = self
            .stores
            .voice_members(0, channel)
            .into_iter()
            .map(|state| (state.user_id, state.self_stream))
            .collect();
        let activity: Vec<Id> = self
            .activities
            .in_channel(channel)
            .into_iter()
            .flat_map(|instance| instance.participants.iter())
            .filter_map(|participant| participant.user_id.parse().ok())
            .collect();
        // `getBlockedOrIgnoredIDs` (module 297334 `aw`), and users the client does not know.
        dm_tooltip_rows(&voice, &activity, |user| {
            self.stores.relationships.get(&user) == Some(&2)
                || self.stores.is_ignored(user)
                || self.dm_tooltip_user(user).is_none()
        })
    }

    /// Module 396655 `tS`/`tg` inside `guildTooltipWrapper__-eZlF` (`body-md/semibold`, `text-default`, up to
    /// 196px, start-aligned): the name (`row__-eZlF rowGuildName__-eZlF`, `guildNameText__-eZlF
    /// guildNameTextLimitedSize__-eZlF`: two lines at most), then one row per kind of presence (`tm`: the 20px icon in
    /// `--interactive-text-default` 8px before the avatars, rows 8px apart), then the mute line (`tT`,
    /// `body-xs/normal` `text-muted`, `muteText__-eZlF` medium, `muteTextWithActivity__-eZlF` 8px above it: `tg` sets
    /// that class on elements that always exist, so the 8px is always there).
    pub(super) fn dm_tooltip_body(&self, channel: Id) -> AnyElement {
        let name = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
            .map(|known| self.private_channel_name(known))
            .unwrap_or_default();
        let rows = self.dm_rows(channel);
        let now = chrono::Utc::now();
        let settings = self.stores.guild_settings.channel(0, channel);
        let muted = self.stores.guild_settings.channel_muted(0, channel, now);
        let mute_line = muted.then(|| match settings.mute_end {
            // "Muted until {endTime}" (module 145408).
            Some(end) => super::notify::muted_until(end),
            // `fpKdS1`.
            None => dc_intl::t!("fpKdS1").to_owned(),
        });
        let row = |glyph: &str, users: &[Id]| -> Option<AnyElement> {
            if users.is_empty() {
                return None;
            }
            Some(
                div()
                    .flex()
                    .items_center()
                    .child(div().flex_none().mr(px(8.)).child(super::icon(
                        glyph,
                        20.,
                        "--interactive-text-default",
                    )))
                    .child(self.user_summary(users))
                    .into_any_element(),
            )
        };
        let lines: Vec<AnyElement> = [
            row("overlay_voice_normal", &rows.voice),
            row("overlay_screen_arrow", &rows.stream),
            row("cp_apps", &rows.activity),
        ]
        .into_iter()
        .flatten()
        .collect();
        div()
            .max_w(px(196.))
            .flex()
            .flex_col()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(gpui::FontWeight(600.))
            .text_color(color("--text-default"))
            .child(
                div()
                    .flex()
                    .items_start()
                    .child(div().min_w(px(0.)).line_clamp(2).child(name)),
            )
            .children(lines.into_iter().map(|line| div().mt(px(8.)).child(line)))
            .children(mute_line.map(|text| {
                div()
                    .mt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(gpui::FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child(text)
            }))
            .into_any_element()
    }

    /// Module 104171 at its default 24px with `max: 6` (`container_m-EqeQ size24_m-EqeQ`): each avatar but the last
    /// sits in `avatarContainerMasked_m-EqeQ` (6px under the next one, cut by `svg-mask-voice-user-summary-item`: the
    /// box less a circle at (58/48, 1/2) of radius 26/48, module 573435), then a "+n" pill (`moreUsers_m-EqeQ`: 24px
    /// high, at least 24px wide, 8px inside, 12px radius, `--background-base-lowest`, 12px semibold `text-default`).
    fn user_summary(&self, users: &[Id]) -> AnyElement {
        let (shown, more) = summary_layout(users.len(), 6);
        // The mask's hole, painted in the tooltip's own fill (`frame`): what shows through it.
        let hole = || {
            div()
                .absolute()
                .left(px(24. * 58. / 48. - 13.))
                .top(px(12. - 13.))
                .size(px(26.))
                .rounded_full()
                .bg(color("--background-surface-high"))
        };
        let mut items: Vec<AnyElement> = Vec::new();
        for (index, id) in users.iter().take(shown).enumerate() {
            let last = more.is_none() && index + 1 == shown;
            let picture = self
                .dm_tooltip_user(*id)
                .and_then(|user| self.images.get(&avatar(&user, 24).0).cloned());
            items.push(
                div()
                    .relative()
                    .flex_none()
                    .size(px(24.))
                    .when(!last, |item| item.mr(px(-6.)))
                    // `avatar_m-EqeQ`: round.
                    .children(picture.map(|picture| img(picture).size_full().rounded_full()))
                    .when(!last, |item| item.child(hole()))
                    .into_any_element(),
            );
        }
        if let Some(more) = more {
            items.push(
                div()
                    .relative()
                    .flex_none()
                    .h(px(24.))
                    .min_w(px(24.))
                    .px(px(8.))
                    .rounded(px(12.))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .line_height(px(12.))
                    .font_weight(gpui::FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(format!("+{more}"))
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .items_center()
            .children(items)
            .into_any_element()
    }

    fn rail_dm_art(&self, channel: &PrivateChannel, highlighted: bool) -> AnyElement {
        let facepile = uses_diagonal_facepile(channel);
        if facepile {
            let scale = if highlighted { 1.1 } else { 1. };
            let size = 32. * scale;
            let part = size / 1.5;
            let offset = part / 2.;
            let gap = 3. * scale;
            let background = if highlighted {
                "--background-brand"
            } else {
                "--background-base-lowest"
            };
            let images: Vec<_> = channel
                .recipients
                .iter()
                .take(2)
                .map(|id| {
                    self.stores
                        .users
                        .get(id)
                        .and_then(|user| self.images.get(&avatar(user, 32).0))
                        .cloned()
                })
                .collect();
            let art = div()
                .relative()
                .flex_none()
                .size(px(size))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .size(px(part))
                        .rounded_full()
                        .bg(color("--background-mod-subtle"))
                        .children(
                            images
                                .first()
                                .and_then(Clone::clone)
                                .map(|image| img(image).size(px(part)).rounded_full()),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(offset - gap))
                        .top(px(offset - gap))
                        .size(px(part + 2. * gap))
                        .rounded_full()
                        .bg(color(background)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(offset))
                        .top(px(offset))
                        .size(px(part))
                        .rounded_full()
                        .bg(color("--background-mod-subtle"))
                        .children(
                            images
                                .get(1)
                                .and_then(Clone::clone)
                                .map(|image| img(image).size(px(part)).rounded_full()),
                        ),
                );
            return div()
                .flex_none()
                .size(px(40.))
                .flex()
                .items_center()
                .justify_center()
                .child(art)
                .into_any_element();
        }

        let source = if channel.kind == 1 {
            self.stores
                .dm_recipient(channel)
                .map(|user| avatar(user, 48))
        } else {
            Some(group_icon(channel))
        };
        let child_img: Option<AnyElement> = source.and_then(|(key, url)| {
            if url.starts_with("data:")
                && let Some(render_img) = cached_data_render_image(&key, &url)
            {
                return Some(
                    img(render_img)
                        .size(px(40.))
                        .rounded_full()
                        .into_any_element(),
                );
            }
            self.images
                .get(&key)
                .cloned()
                .map(|image| img(image).size(px(40.)).rounded_full().into_any_element())
        });
        div()
            .flex_none()
            .size(px(40.))
            .flex()
            .items_center()
            .justify_center()
            .children(child_img)
            .into_any_element()
    }

    pub(super) fn unread_dm_rail(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let entries = self.rail_dm_entries();
        let ids: Vec<u64> = entries.iter().map(|(channel, _)| channel.id).collect();
        RAIL_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            for (channel, count) in &entries {
                cache.insert(channel.id, (channel.clone(), *count));
            }
        });
        let mut first_seen = false;
        super::motion::dm_rail::step(&ids)
            .into_iter()
            .filter_map(|(id, leaving, pose)| {
                let (channel, count) = RAIL_CACHE.with(|cache| cache.borrow().get(&id).cloned())?;
                Some((leaving, pose, channel, count))
            })
            .map(|(leaving, pose, channel, count)| {
                let id = channel.id;
                let probe = !leaving && !std::mem::replace(&mut first_seen, true);
                let selected = self.guild.is_none() && self.channel == Some(id);
                let hovered = self.hovered_rail_dm == Some(id);
                let highlighted = selected || hovered;
                let animating = leaving || pose.height < 0.999 || pose.opacity < 0.999;
                let pill_visible = show_left_pill(animating, selected, hovered, count);
                let height = if selected {
                    40.
                } else if hovered {
                    20.
                } else {
                    8.
                };
                let facepile = uses_diagonal_facepile(&channel);
                let tile_bg = if facepile {
                    highlighted.then_some("--background-brand")
                } else if highlighted && channel.kind == 3 && channel.icon.is_none() {
                    Some("--background-brand")
                } else {
                    Some("--background-mod-subtle")
                };

                let is_connected = self
                    .call
                    .as_ref()
                    .is_some_and(|call| call.guild == 0 && call.channel == id);
                let own_camera = self
                    .call
                    .as_ref()
                    .is_some_and(|call| call.guild == 0 && call.channel == id && call.camera);
                let own_stream = is_connected
                    && (self.in_a_stream()
                        || self
                            .streamers_in_call()
                            .is_some_and(|streamers| !streamers.is_empty()));
                let upper_badge =
                    compute_upper_badge(&self.stores, id, is_connected, own_camera, own_stream);

                let tip_body = Body::DirectMessage(id);

                let tile = div()
                    .when(probe, |tile| tile.parity("dm-rail-tile"))
                    .id(SharedString::from(format!("rail-dm-{id}")))
                    .size(px(40.))
                    .rounded_full()
                    .when_some(tile_bg, |tile, bg| tile.bg(color(bg)))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        if *inside {
                            this.hovered_rail_dm = Some(id);
                        } else if this.hovered_rail_dm == Some(id) {
                            this.hovered_rail_dm = None;
                        }
                        cx.notify();
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                            this.open_context_menu(
                                context_menu::Target::Dm(id),
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_home(Some(id));
                        cx.notify();
                    }))
                    .child(self.rail_dm_art(&channel, highlighted));

                div()
                    .relative()
                    .flex_none()
                    .w(px(72.))
                    .h(px(40. * pose.height))
                    .opacity(pose.opacity)
                    .paint_scale(pose.scale, gpui::point(0.5, 0.5))
                    .flex()
                    .justify_center()
                    .children(pill_visible.then(|| {
                        div()
                            .when(probe, |pill| pill.parity("dm-rail-pill"))
                            .absolute()
                            .left(px(-4.))
                            .top(px((40. - height) / 2.))
                            .w(px(8.))
                            .h(px(height))
                            .rounded_r(px(4.))
                            .bg(color("--text-strong"))
                    }))
                    .child(self.with_tip(format!("rail-dm-{id}"), Tip::rail(tip_body), tile, cx))
                    .children((count > 0).then(|| {
                        unread_badges::rail_number(count, false, probe.then_some("dm-rail-badge"))
                    }))
                    // Module 396655 (`tO`), 771640 (`oi`), 862328 (`BlobMask`): 20×20 outer cutout at
                    // (left: 38px, top: -2px) with 16×16 inner circle at (left: 40px, top: 0px) and 12×12 white icon.
                    .children(upper_badge.map(|(glyph, bg)| {
                        div()
                            .absolute()
                            .left(px(38.))
                            .top(px(-2.))
                            .size(px(20.))
                            .rounded_full()
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(16.))
                                    .rounded_full()
                                    .bg(color(bg))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(super::icon(glyph, 12., "--white")),
                            )
                    }))
                    .into_any_element()
            })
            .collect()
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_dm_rail(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        if std::env::var("GRAVECORD_NO_SETTINGS_WRITE").as_deref() != Ok("1")
            || (self.guild != Some(1568692564637524109) && self.capture_dm_rail.is_none())
        {
            return "DM rail preview must start in fixture guild with settings-write guard".into();
        }
        if words == ["clear"] {
            self.capture_dm_rail = None;
        } else {
            let (value, group) = match words {
                [value] => (*value, false),
                [value, "group"] => (*value, true),
                _ => return "usage: badge dm <0..99999> [group] | clear".into(),
            };
            let Some(count) = value.parse::<u64>().ok().filter(|count| *count <= 99999) else {
                return "invalid DM rail count".into();
            };
            if !self
                .stores
                .private_channels
                .iter()
                .any(|channel| channel.id == FIXTURE_DM && channel.kind == 1)
            {
                return "fixture DM unavailable".into();
            }
            self.capture_dm_rail = Some((count, group));
        }
        cx.notify();
        format!(
            "fixture DM rail preview: {:?}; view only",
            self.capture_dm_rail
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_rail_dm_entries, compute_upper_badge, decode_data_image, decode_data_render_image,
        dm_tooltip_rows, group_icon, show_left_pill, summary_layout, uses_diagonal_facepile,
    };
    use dc_state::{Stores, VoiceState};
    use serde_json::json;

    #[test]
    fn active_dm_call_pins_to_index_zero_and_deduplicates_mentions() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": { "id": "999", "username": "me" },
                "private_channels": [
                    { "id": "100", "type": 1, "recipient_ids": ["1"], "last_message_id": "1000" },
                    { "id": "200", "type": 1, "recipient_ids": ["2"], "last_message_id": "2000" },
                    { "id": "300", "type": 1, "recipient_ids": ["3"], "last_message_id": "3000" }
                ],
                "read_state": {
                    "entries": [
                        { "id": "100", "mention_count": 5, "last_message_id": "900" },
                        { "id": "200", "mention_count": 2, "last_message_id": "1900" }
                    ]
                }
            }),
        );

        // Zero-mention active DM call pins to index 0 ahead of unread mention DMs.
        let zero_mention_call = compute_rail_dm_entries(&stores, Some(300));
        let ids: Vec<(u64, u64)> = zero_mention_call
            .iter()
            .map(|(ch, mentions)| (ch.id, *mentions))
            .collect();
        assert_eq!(ids[0], (300, 0));
        assert!(ids.iter().any(|&(id, m)| id == 100 && m == 5));
        assert!(ids.iter().any(|&(id, m)| id == 200 && m == 2));

        // Active DM call that also has mentions > 0 at k > 0 moves to index 0 without duplicating.
        let deduped = compute_rail_dm_entries(&stores, Some(200));
        assert_eq!(deduped[0].0.id, 200);
        assert_eq!(deduped[0].1, 2);
        assert_eq!(deduped.iter().filter(|(ch, _)| ch.id == 200).count(), 1);
    }

    #[test]
    fn left_pill_visibility_hides_zero_mention_call_at_rest_and_while_animating() {
        assert!(!show_left_pill(false, false, false, 0));
        assert!(show_left_pill(false, false, true, 0));
        assert!(show_left_pill(false, true, false, 0));
        assert!(show_left_pill(false, false, false, 3));
        assert!(!show_left_pill(true, true, true, 5));
    }

    #[test]
    fn upper_badge_precedence_and_connected_coloring() {
        let mut stores = Stores::default();
        let ch = 555;

        assert_eq!(compute_upper_badge(&stores, ch, false, false, false), None);
        assert_eq!(
            compute_upper_badge(&stores, ch, true, false, false),
            Some(("voice_channel", "--status-positive"))
        );
        assert_eq!(
            compute_upper_badge(&stores, ch, true, true, false),
            Some(("rail_camera_badge", "--status-positive"))
        );

        // Disconnected remote voice state with self_video -> dark camera badge.
        stores.voice_states.insert(
            0,
            vec![VoiceState {
                user_id: 42,
                channel_id: ch,
                self_video: true,
                self_stream: false,
                ..Default::default()
            }],
        );
        assert_eq!(
            compute_upper_badge(&stores, ch, false, false, false),
            Some(("rail_camera_badge", "--icon-overlay-dark"))
        );

        // Stream takes precedence over camera.
        stores.voice_states.insert(
            0,
            vec![VoiceState {
                user_id: 42,
                channel_id: ch,
                self_video: true,
                self_stream: true,
                ..Default::default()
            }],
        );
        assert_eq!(
            compute_upper_badge(&stores, ch, true, true, false),
            Some(("rail_stream_badge", "--status-positive"))
        );
    }

    #[test]
    fn the_dm_tooltip_lists_activity_then_stream_then_call_without_repeats_or_blocked() {
        // 1 in the Activity and streaming, 2 streaming, 3 in the call, 4 blocked, 5 in the call twice.
        let voice = [
            (1, true),
            (2, true),
            (3, false),
            (4, false),
            (5, false),
            (5, false),
        ];
        let rows = dm_tooltip_rows(&voice, &[1, 1], |user| user == 4);
        assert_eq!(rows.activity, vec![1]);
        assert_eq!(rows.stream, vec![2]);
        assert_eq!(rows.voice, vec![3, 5]);
        let empty = dm_tooltip_rows(&[], &[], |_| false);
        assert!(empty.voice.is_empty() && empty.stream.is_empty() && empty.activity.is_empty());
    }

    #[test]
    fn the_avatar_list_shows_up_to_six_then_five_and_a_count() {
        assert_eq!(summary_layout(2, 6), (2, None));
        assert_eq!(summary_layout(6, 6), (6, None));
        assert_eq!(summary_layout(7, 6), (5, Some(2)));
        assert_eq!(summary_layout(200, 6), (5, Some(99)));
    }

    #[test]
    fn data_uri_group_icons_decode_to_bgra_and_facepile_handles_managed_groups() {
        let mut red_rgba = image::RgbaImage::new(1, 1);
        red_rgba.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgba8(red_rgba)
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);
        let uri = format!("data:image/png;base64,{b64}");

        let mut stores = Stores::default();
        stores.dispatch(
            "CHANNEL_CREATE",
            &json!({
                "id": "888",
                "type": 3,
                "recipient_ids": ["10"],
                "icon": uri
            }),
        );
        let ch = &stores.private_channels[0];
        let (_, resolved_url) = group_icon(ch);
        assert_eq!(resolved_url, uri);
        assert!(decode_data_render_image(&uri).is_some());
        assert!(decode_data_image(&uri).is_some());
        // Solo group DM (recipients.len() < 2) or custom icon does not use diagonal facepile.
        assert!(!uses_diagonal_facepile(ch));

        // Managed group DM with application_id.is_some() && icon.is_none() && recipients.len() >= 2 uses facepile.
        stores.dispatch(
            "CHANNEL_CREATE",
            &json!({
                "id": "889",
                "type": 3,
                "application_id": "9999",
                "recipient_ids": ["10", "20"]
            }),
        );
        let managed = stores
            .private_channels
            .iter()
            .find(|c| c.id == 889)
            .unwrap();
        assert!(uses_diagonal_facepile(managed));
    }
}
