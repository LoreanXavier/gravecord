//! Private DM rail entries and diagonal group avatars (396655/478063/486020).
use super::{
    Root, color, context_menu,
    tooltip::{Body, Tip},
    unread_badges,
};
use crate::parity::Parity;
use dc_state::{PrivateChannel, User};
use gpui::{AnyElement, Context, IntoElement, SharedString, div, img, prelude::*, px};
use md5::{Digest, Md5};

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

fn avatar(user: &User, size: u32) -> (String, String) {
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

fn group_icon(channel: &PrivateChannel) -> (String, String) {
    let url = if let Some(hash) = channel.icon.as_ref() {
        match channel.application_id {
            Some(_) if hash.starts_with("data:") => hash.clone(),
            Some(app) => format!("https://cdn.discordapp.com/app-icons/{app}/{hash}.png?size=32"),
            None => format!(
                "https://cdn.discordapp.com/channel-icons/{}/{hash}.png?size=32",
                channel.id
            ),
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

thread_local! {
    /// The rail's items as last drawn, so that one that goes away can be drawn while it leaves.
    static RAIL_CACHE: std::cell::RefCell<std::collections::HashMap<u64, (PrivateChannel, u64)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
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
        dc_state::private_channels::unread_channels(&self.stores)
            .into_iter()
            .map(|channel| {
                (
                    channel.clone(),
                    self.stores.read_states.mentions(channel.id),
                )
            })
            .collect()
    }

    pub(super) fn request_rail_dm_images(&mut self) {
        let mut images = Vec::new();
        for (channel, _) in self.rail_dm_entries() {
            if channel.kind == 1 {
                if let Some(user) = self.stores.dm_recipient(&channel) {
                    images.push(avatar(user, 48));
                }
            } else if channel.icon.is_none() && channel.recipients.len() >= 2 {
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
        }
        for (key, url) in images {
            self.request_image(key, url);
        }
    }

    fn rail_dm_art(&self, channel: &PrivateChannel, highlighted: bool) -> AnyElement {
        let group_child = channel.kind == 3 && channel.icon.is_none();
        let scale = if group_child && highlighted { 1.1 } else { 1. };
        let facepile = group_child && channel.recipients.len() >= 2;
        let background = if highlighted {
            "--background-brand"
        } else {
            "--background-base-lowest"
        };
        let art = if facepile {
            let size = 32. * scale;
            let part = size / 1.5;
            let offset = part / 2.;
            let gap = 3. * scale;
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
            div()
                .relative()
                .flex_none()
                .size(px(size))
                .children(images[0].clone().map(|image| {
                    img(image)
                        .absolute()
                        .left_0()
                        .top_0()
                        .size(px(part))
                        .rounded_full()
                }))
                .child(
                    div()
                        .absolute()
                        .left(px(offset - gap))
                        .top(px(offset - gap))
                        .size(px(part + 2. * gap))
                        .rounded_full()
                        .bg(color(background)),
                )
                .children(images[1].clone().map(|image| {
                    img(image)
                        .absolute()
                        .left(px(offset))
                        .top(px(offset))
                        .size(px(part))
                        .rounded_full()
                }))
                .into_any_element()
        } else {
            let source = if channel.kind == 1 {
                self.stores
                    .dm_recipient(channel)
                    .map(|user| avatar(user, 48))
            } else {
                Some(group_icon(channel))
            };
            let image = source.and_then(|(key, _)| self.images.get(&key)).cloned();
            div()
                .flex_none()
                .size(px(40. * scale))
                .children(image.map(|image| img(image).size(px(40. * scale)).rounded_full()))
                .into_any_element()
        };
        div()
            .flex_none()
            .size(px(40. * scale))
            .flex()
            .items_center()
            .justify_center()
            .child(art)
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
                let height = if selected {
                    40.
                } else if hovered {
                    20.
                } else {
                    8.
                };
                let tile = div()
                    .when(probe, |tile| tile.parity("dm-rail-tile"))
                    .id(SharedString::from(format!("rail-dm-{id}")))
                    .size(px(40.))
                    .rounded_full()
                    .when(
                        channel.kind == 3 && channel.icon.is_none() && (selected || hovered),
                        |tile| tile.bg(color("--background-brand")),
                    )
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
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
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
                    .child(self.rail_dm_art(&channel, selected || hovered));
                div()
                    .relative()
                    .flex_none()
                    .w(px(72.))
                    .h(px(40. * pose.height))
                    .opacity(pose.opacity)
                    .paint_scale(pose.scale, gpui::point(0.5, 0.5))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .when(probe, |pill| pill.parity("dm-rail-pill"))
                            .absolute()
                            .left(px(-4.))
                            .top(px((40. - height) / 2.))
                            .w(px(8.))
                            .h(px(height))
                            .rounded_r(px(4.))
                            .bg(color("--text-strong")),
                    )
                    .child(self.with_tip(
                        format!("rail-dm-{id}"),
                        Tip::rail(Body::Heading(self.private_channel_name(&channel).into())),
                        tile,
                        cx,
                    ))
                    .child(unread_badges::rail_number(
                        count,
                        false,
                        probe.then_some("dm-rail-badge"),
                    ))
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
