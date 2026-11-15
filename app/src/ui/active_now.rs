//! Friends' grouped activity feed: 596339 EI/EE, NowPlayingViewStore 943577.
use super::{Root, color, icon};
use crate::{net::Request, parity::Parity};
use dc_state::{
    Id,
    now_playing::{self, Activity, Application, Party, Stream},
};
use gpui::{
    AnyElement, Context, FontWeight, Image, ImageFormat, IntoElement, ObjectFit, SharedString, div,
    img, prelude::*, px, relative,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, LazyLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const AFFINITIES: &str = "affinities";
const CACHE: &str = "UserAffinitiesStoreV2";

pub(super) struct State {
    body: Value,
    fetched_at: u64,
    requested_at: Option<u64>,
    tick_running: bool,
    pub(super) avatars: Vec<Id>,
}

impl Default for State {
    fn default() -> Self {
        let body = crate::storage::get(CACHE).unwrap_or(Value::Null);
        Self {
            fetched_at: body["lastFetched"].as_u64().unwrap_or(0),
            body,
            requested_at: None,
            tick_running: false,
            avatars: Vec::new(),
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis() as u64)
}

fn text(
    value: impl Into<SharedString>,
    size: f32,
    line: f32,
    weight: f32,
    tint: &'static str,
) -> gpui::Div {
    div()
        .min_w(px(0.))
        .truncate()
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(weight))
        .text_color(color(tint))
        .child(value.into())
}

fn elapsed(activity: &Activity, time: u64) -> String {
    let Some(start) = activity.started else {
        return String::new();
    };
    let minutes = time.abs_diff(start) / 60_000;
    let (key, amount) = if minutes < 1 {
        ("Bf3PRU", 0)
    } else if minutes < 60 {
        ("A9P+dT", minutes)
    } else if minutes < 1440 {
        ("KwVzfh", minutes / 60)
    } else {
        ("hbUpBN", minutes / 1440)
    };
    dc_intl::format(
        key,
        &[
            (
                "name",
                activity.value["name"].as_str().unwrap_or_default().into(),
            ),
            ("time", (amount as i64).into()),
        ],
    )
}

fn game_icon_url(game: &Value) -> Option<String> {
    let icon = &game["media"]["icon"];
    let value = icon["value"].as_str().filter(|value| !value.is_empty())?;
    match icon["type"].as_str()? {
        "hash" => Some(format!(
            "https://cdn.discordapp.com/app-icons/{}/{value}.png?size=64&keep_aspect_ratio=false",
            game["id"].as_str()?
        )),
        "url" => {
            let Ok(mut url) = reqwest::Url::parse(value) else {
                return Some(value.into());
            };
            if matches!(
                url.host_str(),
                Some("images-ext-1.discordapp.net" | "images-ext-2.discordapp.net")
            ) && url.path().starts_with("/external/")
            {
                let pairs: Vec<_> = url
                    .query_pairs()
                    .filter(|(key, _)| key != "width" && key != "height")
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect();
                url.set_query(None);
                url.query_pairs_mut()
                    .extend_pairs(pairs)
                    .append_pair("width", "64")
                    .append_pair("height", "64");
            } else if url.host_str() == Some("cdn.discordapp.com") {
                if url
                    .query_pairs()
                    .find(|(key, _)| key == "size")
                    .and_then(|(_, size)| size.parse::<u32>().ok())
                    .is_some_and(|size| size > 0 && size < 64)
                {
                    return Some(value.into());
                }
                let pairs: Vec<_> = url
                    .query_pairs()
                    .filter(|(key, _)| key != "size")
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect();
                url.set_query(None);
                url.query_pairs_mut()
                    .extend_pairs(pairs)
                    .append_pair("size", "64");
            }
            Some(url.into())
        }
        _ => None,
    }
}

fn preview_art() -> Arc<Image> {
    static DARK: LazyLock<Arc<Image>> = LazyLock::new(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            include_bytes!("../../assets/icons/active_now_stream_empty_dark.svg").to_vec(),
        ))
    });
    static LIGHT: LazyLock<Arc<Image>> = LazyLock::new(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            include_bytes!("../../assets/icons/active_now_stream_empty_light.svg").to_vec(),
        ))
    });
    if super::theme() == dc_tokens::Theme::Light {
        LIGHT.clone()
    } else {
        DARK.clone()
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct MaskedSource(Arc<Image>, bool);
struct MaskedArt;
impl gpui::Asset for MaskedArt {
    type Source = MaskedSource;
    type Output = Option<Arc<gpui::RenderImage>>;
    fn load(
        source: Self::Source,
        cx: &mut gpui::App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let original = source.0.to_image_data(renderer.clone()).ok()?;
            let svg = renderer
                .parse_svg(if source.1 {
                    include_bytes!("../../assets/icons/active_now_voice_mask.svg")
                } else {
                    include_bytes!("../../assets/icons/active_now_party_mask.svg")
                })
                .ok()?;
            let mut frames = Vec::new();
            for index in 0..original.frame_count() {
                let size = original.size(index);
                let mask = renderer
                    .render_parsed(&svg, gpui::SvgSize::ExactSize(size))
                    .ok()?;
                let mut pixels = original.as_bytes(index)?.to_vec();
                for (pixel, mask) in pixels
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(mask.as_bytes(0)?.as_chunks::<4>().0.iter())
                {
                    pixel[3] = ((u16::from(pixel[3]) * u16::from(mask[3]) + 127) / 255) as u8;
                }
                let buffer = image::RgbaImage::from_raw(
                    u32::from(size.width),
                    u32::from(size.height),
                    pixels,
                )?;
                frames.push(image::Frame::from_parts(
                    buffer,
                    0,
                    0,
                    original.delay(index),
                ));
            }
            Some(Arc::new(gpui::RenderImage::new(frames)))
        }
    }
}
fn masked_art(image: Arc<Image>, voice: bool, size: f32) -> AnyElement {
    let source = MaskedSource(image, voice);
    img(move |window: &mut gpui::Window, cx: &mut gpui::App| {
        window.use_asset::<MaskedArt>(&source, cx).flatten().map(Ok)
    })
    .size(px(size))
    .into_any_element()
}

impl Root {
    pub(super) fn active_now_fetched(&mut self, key: &str, body: &Value) {
        if key != AFFINITIES || !body["user_affinities"].is_array() {
            return;
        }
        self.active_now.fetched_at = now();
        self.active_now.body = body.clone();
        let aliases = [
            ("other_user_id", "otherUserId"),
            ("user_segment", "userSegment"),
            ("other_user_segment", "otherUserSegment"),
            ("is_friend", "isFriend"),
            ("dm_probability", "dmProbability"),
            ("dm_rank", "dmRank"),
            ("vc_probability", "vcProbability"),
            ("vc_rank", "vcRank"),
            ("server_message_probability", "serverMessageProbability"),
            ("server_message_rank", "serverMessageRank"),
            ("communication_probability", "communicationProbability"),
            ("communication_rank", "communicationRank"),
        ];
        let users: Vec<Value> = body["user_affinities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let mut mapped = serde_json::Map::new();
                for (from, to) in aliases {
                    if let Some(value) = row.get(from) {
                        mapped.insert(to.into(), value.clone());
                    }
                }
                Value::Object(mapped)
            })
            .collect();
        crate::storage::set(
            CACHE,
            json!({"userAffinities":users,"lastFetched":self.active_now.fetched_at}),
        );
    }

    pub(super) fn active_now_visible(&self) -> bool {
        self.guild.is_none()
            && self.channel.is_none()
            && !self.nitro_open
            && !self.discover_active()
            && !self.quests_active()
            && !self.shop_active()
            && !self.message_requests_active()
            && self.viewport.0 > 1200.
    }

    fn active_now_prepare(&mut self, cx: &mut Context<Self>) {
        let time = now();
        if self.stores.consents.get("personalization") == Some(&true)
            && time.saturating_sub(self.active_now.fetched_at) > 8 * 60 * 60 * 1000
            && self
                .active_now
                .requested_at
                .is_none_or(|at| time.saturating_sub(at) >= 300_000)
        {
            self.active_now.requested_at = Some(time);
            let _ = self.requests.send(Request::Fetch {
                key: AFFINITIES.into(),
                route: "/users/@me/affinities/v2/users".into(),
            });
        }
        if !self.active_now.tick_running {
            self.active_now.tick_running = true;
            cx.spawn(async |root, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_secs(10))
                        .await;
                    if root
                        .update(cx, |root, cx| {
                            let visible = root.active_now_visible();
                            root.active_now.tick_running = visible;
                            if visible {
                                cx.notify();
                            }
                            visible
                        })
                        .ok()
                        != Some(true)
                    {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    fn active_now_application(&self, id: &str) -> Option<Application> {
        if id.starts_with("twitch:") {
            return Some(Application {
                id: id.into(),
                name: dc_intl::t!("JIPtgq").into(),
                icon: None,
            });
        }
        let body = self.fetched.get(&format!("presence-app:{id}"))?;
        let app = body
            .as_array()?
            .iter()
            .find(|app| app["id"].as_str() == Some(id))?;
        Some(Application {
            id: id.into(),
            name: app["name"].as_str()?.into(),
            icon: app["icon"].as_str().map(|hash| {
                format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=64")
            }),
        })
    }

    fn active_now_game_id(&self, application: &str) -> Option<String> {
        let app = self
            .fetched
            .get(&format!("presence-app:{application}"))?
            .as_array()?
            .iter()
            .find(|value| value["id"] == application)?;
        super::inventory_popout::canonical_game_id(app).map(str::to_owned)
    }

    fn active_now_game(&self, application: &str) -> Option<&Value> {
        self.fetched.get(&format!(
            "inventory-game:{}",
            self.active_now_game_id(application)?
        ))
    }

    pub(super) fn active_now_column(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.active_now_prepare(cx);
        let affinity = now_playing::affinities(
            self.fetched
                .get(AFFINITIES)
                .filter(|body| body["user_affinities"].is_array())
                .unwrap_or(&self.active_now.body),
        );
        let sort = match self
            .stores
            .experiments
            .variant("2025-08-voice-user-affinity")
        {
            Some(1) => now_playing::Sort::VoiceProbability,
            Some(2) => now_playing::Sort::CommunicationProbability,
            _ => now_playing::Sort::Default,
        };
        let feed = now_playing::feed(&self.stores, &affinity, sort, |id| {
            self.active_now_application(id)
        });
        let games = feed
            .parties
            .iter()
            .flat_map(|party| party.activities.iter())
            .filter_map(|activity| self.active_now_game_id(&activity.application.id))
            .collect();
        self.request_inventory_game_ids(games);
        for id in feed.missing_applications {
            self.overlay_request_game_icon(&id);
        }
        self.active_now.avatars = feed
            .parties
            .iter()
            .filter_map(|party| party.priority.first().copied())
            .collect();
        let empty = feed.parties.is_empty();
        let cards = feed
            .parties
            .iter()
            .enumerate()
            .map(|(index, party)| self.active_now_party(party, index == 0, cx))
            .collect::<Vec<_>>();
        div()
            .parity_left("active-now", 1.)
            .flex_grow_0()
            .flex_shrink_1()
            .flex_basis(relative(0.3))
            .min_w(px(360.))
            .max_w(px(420.))
            .h_full()
            .border_l_1()
            .border_color(color("--background-mod-subtle"))
            .child(
                div()
                    .id("active-now-scroller")
                    .size_full()
                    .overflow_y_scroll()
                    .px(px(16.))
                    .pt(px(14.))
                    .pb(px(16.))
                    .child(text(dc_intl::t!("F4Zggw"), 20., 24., 600., "--text-strong"))
                    .when(empty, |column| {
                        column.child(
                            div()
                                .parity("active-card")
                                .mt(px(9.))
                                .p(px(16.))
                                .rounded(px(8.))
                                .bg(color("--background-base-low"))
                                .text_center()
                                .child(text(dc_intl::t!("ngJ/5u"), 16., 20., 600., "--text-strong"))
                                .child(
                                    div()
                                        .mt(px(4.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--interactive-text-default"))
                                        .child(dc_intl::t!("99ZWxQ")),
                                ),
                        )
                    })
                    .children(cards),
            )
            .into_any_element()
    }

    fn active_now_avatar(
        &self,
        user: Id,
        size: f32,
        status: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(person) = self.stores.users.get(&user) else {
            return div().size(px(size)).into_any_element();
        };
        let (key, url) = super::avatar_source(person);
        self.want_image(key, url, cx);
        let art = if status {
            self.member_avatar(person, self.stores.status(user), false, false, cx)
                .into_any_element()
        } else {
            self.event_avatar(Some(person), size)
        };
        self.profile_trigger(
            format!("active-now-avatar-{user}-{size}-{status}"),
            user,
            art,
            cx,
        )
    }

    fn active_now_art(
        &self,
        application: &Application,
        size: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if application.id == "spotify" {
            static SPOTIFY: LazyLock<Arc<Image>> = LazyLock::new(|| {
                Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    include_bytes!("../../assets/icons/active_now_spotify.png").to_vec(),
                ))
            });
            return img(SPOTIFY.clone()).size(px(size)).into_any_element();
        }
        if let Some(url) = &application.icon {
            self.want_image(url.clone(), url.clone(), cx);
            if let Some(image) = self.images.get(url) {
                return img(image.clone())
                    .size(px(size))
                    .rounded(px(if size == 24. { 4. } else { 0. }))
                    .into_any_element();
            }
        }
        if size == 24. {
            static UNKNOWN: LazyLock<Arc<Image>> = LazyLock::new(|| {
                Arc::new(Image::from_bytes(
                    ImageFormat::Svg,
                    include_bytes!("../../assets/icons/active_now_unknown_game.svg").to_vec(),
                ))
            });
            return img(UNKNOWN.clone()).size(px(size)).into_any_element();
        }

        div().size(px(size)).into_any_element()
    }

    fn active_now_guild_art(&self, guild: &dc_state::Guild, cx: &mut Context<Self>) -> AnyElement {
        let source = guild.icon.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/icons/{}/{hash}.webp?size=64",
                guild.id
            )
        });
        let art = source.and_then(|url| {
            self.want_image(url.clone(), url.clone(), cx);
            self.images.get(&url).cloned()
        });
        let abbreviation = guild.acronym();
        let size = [14., 14., 12., 12., 10., 8., 6.][abbreviation.chars().count().min(6)];
        div()
            .size(px(32.))
            .relative()
            .flex_none()
            .child(
                art.map(|art| masked_art(art, true, 32.))
                    .unwrap_or_else(|| {
                        div()
                            .size_full()
                            .rounded_full()
                            .relative()
                            .bg(color("--background-surface-high"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(text(abbreviation, size, 16., 400., "--white"))
                            .child(
                                div()
                                    .absolute()
                                    .right(px(-4.))
                                    .bottom(px(-4.))
                                    .size(px(20.))
                                    .rounded_full()
                                    .bg(color("--background-base-low")),
                            )
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .absolute()
                    .right(px(-2.))
                    .bottom(px(-2.))
                    .size(px(16.))
                    .rounded_full()
                    .bg(color(if super::theme() == dc_tokens::Theme::Light {
                        "--primary-300"
                    } else {
                        "--primary-800"
                    }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("voice_channel", 10., "--text-default")),
            )
            .into_any_element()
    }

    fn active_now_title(&self, party: &Party) -> String {
        let name = |id: Id| {
            self.stores
                .users
                .get(&id)
                .map(|user| self.dm_display_name(user))
                .unwrap_or_default()
        };
        let first = party.priority.first().copied().unwrap_or(0);
        let second = party
            .priority
            .get(1)
            .copied()
            .or_else(|| {
                party
                    .members
                    .iter()
                    .copied()
                    .find(|id| !party.priority.contains(id))
            })
            .unwrap_or(first);
        match party.members.len() {
            0 | 1 => name(first),
            2 => dc_intl::format(
                "t1DyC1",
                &[
                    ("user1", name(first).into()),
                    ("user2", name(second).into()),
                ],
            ),
            count => dc_intl::format(
                "5CSEcJ",
                &[
                    ("user1", name(first).into()),
                    ("user2", name(second).into()),
                    ("extras", ((count - 2) as i64).into()),
                ],
            ),
        }
    }

    fn active_now_party(&self, party: &Party, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let activity = party.activities.first();
        let subtitle = if party.activities.len() > 1 {
            dc_intl::t!("sau5tt").to_owned()
        } else if let Some(activity) = activity {
            match activity.value["type"].as_u64() {
                Some(2) if party.members.len() == 1 => dc_intl::format(
                    "NF5xop",
                    &[(
                        "name",
                        activity.value["name"].as_str().unwrap_or_default().into(),
                    )],
                ),
                Some(3) => dc_intl::format(
                    "pW3Ip3",
                    &[(
                        "name",
                        activity.value["name"].as_str().unwrap_or_default().into(),
                    )],
                ),
                Some(1) => dc_intl::t!("HyGHj7").into(),
                _ => elapsed(activity, now()),
            }
        } else {
            dc_intl::t!("grGyaf").into()
        };
        let header = div()
            .when(first, |row| row.parity("active-now-header"))
            .p(px(16.))
            .h(px(68.))
            .rounded_t(px(15.))
            .bg(color("--background-mod-muted"))
            .child(
                div()
                    .h(px(38.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(self.active_now_avatar(party.priority[0], 32., true, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(20.))
                            .child(text(
                                self.active_now_title(party),
                                16.,
                                20.,
                                600.,
                                "--text-strong",
                            ))
                            .child(text(subtitle, 14., 18., 400., "--text-muted")),
                    )
                    .when_some(activity, |row, activity| {
                        row.child(
                            if activity.application.id == "spotify" && party.members.len() == 1 {
                                icon("active_now_spotify", 24., "--platform-spotify")
                                    .into_any_element()
                            } else {
                                self.active_now_art(&activity.application, 24., cx)
                            },
                        )
                    }),
            );
        let mut sections = Vec::new();
        for voice in &party.voice {
            let Some(guild) = self.stores.guild(voice.guild) else {
                continue;
            };
            let Some(channel) = guild
                .channels
                .iter()
                .find(|channel| channel.id == voice.channel)
            else {
                continue;
            };
            let (guild_id, channel_id) = (voice.guild, voice.channel);
            let row = div()
                .id(SharedString::from(format!("active-now-voice-{channel_id}")))
                .when(first, |row| row.parity("active-now-voice"))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .id(SharedString::from(format!("active-now-guild-{channel_id}")))
                        .cursor_pointer()
                        .child(self.active_now_guild_art(guild, cx))
                        .on_click(cx.listener(move |root, _, _, cx| root.go_guild(guild_id, cx))),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "active-now-channel-{channel_id}"
                        )))
                        .flex_1()
                        .min_w(px(0.))
                        .cursor_pointer()
                        .hover(|style| style.underline())
                        .child(text(guild.name.clone(), 14., 18., 600., "--text-strong"))
                        .child(text(channel.name.clone(), 12., 16., 400., "--text-muted"))
                        .on_click(cx.listener(move |root, _, _, cx| {
                            root.open_voice_view(guild_id, channel_id, cx);
                            root.request_join_voice(guild_id, channel_id, cx);
                        })),
                )
                .child(self.active_now_members(&voice.members, false, cx));
            sections.push(row.into_any_element());
        }
        for stream in &party.streams {
            sections.push(self.active_now_stream(stream, cx));
        }
        for (index, activity) in party.activities.iter().enumerate() {
            sections.push(self.active_now_activity(
                activity,
                party.members.len() == 1,
                first && index == 0,
                cx,
            ));
        }
        let last = sections.len().saturating_sub(1);
        div()
            .id(SharedString::from(format!("active-now-party-{}", party.id)))
            .when(first, |card| card.parity_with_border("active-now-card", 1.))
            .mt(px(12.))
            .w_full()
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(16.))
            .overflow_hidden()
            .bg(color("--background-base-low"))
            .child(header)
            .children(sections.into_iter().enumerate().map(|(index, section)| {
                div()
                    .p(px(16.))
                    .when(index != last, |row| {
                        row.border_b_1().border_color(color("--border-subtle"))
                    })
                    .child(section)
            }))
            .into_any_element()
    }

    fn active_now_members(&self, members: &[Id], solo: bool, cx: &mut Context<Self>) -> AnyElement {
        let shown = if solo && members.len() < 2 {
            0
        } else if members.len() == 3 {
            3
        } else {
            members.len().min(2)
        };
        div()
            .flex()
            .items_center()
            .children(members.iter().take(shown).enumerate().map(|(index, id)| {
                let art = self
                    .stores
                    .users
                    .get(id)
                    .and_then(|user| self.images.get(&super::avatar_source(user).0))
                    .filter(|_| index + 1 < shown || members.len() > 3)
                    .map(|image| {
                        self.profile_trigger(
                            format!("active-party-avatar-{id}"),
                            *id,
                            masked_art(image.clone(), false, 24.),
                            cx,
                        )
                    })
                    .unwrap_or_else(|| self.active_now_avatar(*id, 24., false, cx));
                div().ml(px(if index == 0 { 0. } else { -2. })).child(art)
            }))
            .when(members.len() > 3, |row| {
                row.child(
                    text(
                        format!("+{}", (members.len() - 2).min(99)),
                        14.,
                        18.,
                        600.,
                        "--text-default",
                    )
                    .ml(px(-2.))
                    .h(px(24.))
                    .min_w(px(24.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(12.))
                    .bg(color("--background-base-lower")),
                )
            })
            .into_any_element()
    }

    fn active_now_activity(
        &self,
        activity: &Activity,
        solo: bool,
        probe: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let music = now_playing::is_spotify(&activity.value);
        let rich = activity.value["assets"].is_object() || music;
        let game = (!rich && activity.value["type"] == 0)
            .then(|| self.active_now_game(&activity.application.id))
            .flatten();
        let profile = game.is_some_and(|game| game["game_flags"].as_u64().unwrap_or(0) & 1 == 0);
        let canonical = game.map(|game| Application {
            id: game["id"]
                .as_str()
                .unwrap_or(&activity.application.id)
                .into(),
            name: game["name"]
                .as_str()
                .unwrap_or(&activity.application.name)
                .into(),
            icon: game_icon_url(game).or_else(|| activity.application.icon.clone()),
        });
        let application = canonical.as_ref().unwrap_or(&activity.application);
        let title = if rich {
            activity.value["details"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&activity.application.name)
                .to_owned()
        } else {
            application.name.clone()
        };
        let subtitle = if rich {
            activity.value["state"]
                .as_str()
                .unwrap_or_default()
                .replace("; ", ", ")
        } else {
            dc_intl::format(
                "C4WXvc",
                &[("memberCount", (activity.members.len() as i64).into())],
            )
        };
        let art = super::live_activity::asset_url(&activity.value, "large_image").and_then(|url| {
            self.want_image(url.clone(), url.clone(), cx);
            self.images.get(&url).cloned()
        });
        div()
            .when(probe, |row| row.parity("active-now-activity"))
            .flex()
            .when(music, |row| row.items_start())
            .when(!music, |row| row.items_center())
            .gap(px(12.))
            .min_h(px(if profile && application.icon.is_some() {
                35.
            } else {
                34.
            }))
            .child(
                div()
                    .w(px(32.))
                    .h(px(if profile && application.icon.is_some() {
                        35.
                    } else {
                        32.
                    }))
                    .relative()
                    .child(
                        div()
                            .size(px(32.))
                            .when(probe, |art| art.parity("active-now-art"))
                            .child(
                                art.map(|art| {
                                    img(art)
                                        .size(px(32.))
                                        .object_fit(ObjectFit::Cover)
                                        .rounded(px(if music { 0. } else { 4. }))
                                        .into_any_element()
                                })
                                .unwrap_or_else(|| self.active_now_art(application, 32., cx)),
                            ),
                    )
                    .when(music && !solo, |art| {
                        art.child(div().absolute().right(px(-4.)).bottom(px(-4.)).child(icon(
                            "active_now_spotify",
                            16.,
                            "--platform-spotify",
                        )))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(20.))
                    .child(
                        text(title, 14., 18., 600., "--text-strong")
                            .when(probe, |title| title.parity("active-now-activity-title")),
                    )
                    .child(text(subtitle, 12., 16., 400., "--text-muted")),
            )
            .child(self.active_now_members(&activity.members, music && solo, cx))
            .into_any_element()
    }

    fn active_now_can_watch(&self, stream: &Stream) -> bool {
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.guild == stream.guild && call.channel == stream.channel)
        {
            return true;
        }
        if stream.guild == 0 {
            return self
                .stores
                .private_channels
                .iter()
                .any(|channel| channel.id == stream.channel);
        }
        let Some(guild) = self.stores.guild(stream.guild) else {
            return false;
        };
        let Some(channel) = guild
            .channels
            .iter()
            .find(|channel| channel.id == stream.channel)
        else {
            return false;
        };
        let Some(me) = self.stores.me() else {
            return false;
        };
        dc_state::permissions::can_in_channel(
            guild,
            channel,
            me,
            dc_state::permissions::CONNECT | dc_state::permissions::VIEW_CHANNEL,
        ) && (channel.user_limit == 0
            || self.stores.voice_members(guild.id, channel.id).len() < channel.user_limit as usize
            || dc_state::permissions::can_in_channel(
                guild,
                channel,
                me,
                dc_state::permissions::MOVE_MEMBERS,
            ))
    }

    fn active_now_stream(&self, stream: &Stream, cx: &mut Context<Self>) -> AnyElement {
        let allowed = self.active_now_can_watch(stream);
        let key = super::stream_previews::key_of(stream.guild, stream.channel, stream.owner);
        let preview = allowed
            .then(|| self.stream_previews.url(&key.to_string()))
            .flatten()
            .and_then(|url| self.images.get(url))
            .cloned();
        if allowed {
            let root = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = root.update(cx, |root, _| root.stream_preview_want(key));
            });
        }
        let owner = stream.owner;
        let name = self
            .stores
            .users
            .get(&owner)
            .map(|user| self.dm_display_name(user))
            .unwrap_or_default();
        let destination = stream.clone();
        div()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(self.active_now_avatar(owner, 32., false, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(text(name, 14., 18., 600., "--text-strong"))
                            .child(text(dc_intl::t!("eXan7B"), 12., 16., 400., "--text-muted")),
                    )
                    .child(
                        text("LIVE", 12., 16., 600., "--white")
                            .px(px(6.))
                            .rounded(px(8.))
                            .bg(color("--status-danger")),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("active-now-stream-{owner}")))
                    .mt(px(12.))
                    .w_full()
                    .aspect_ratio(16. / 9.)
                    .rounded(px(8.))
                    .overflow_hidden()
                    .relative()
                    .bg(color("--background-mod-normal"))
                    .when_some(preview.clone(), |tile, preview| {
                        tile.child(img(preview).size_full().object_fit(ObjectFit::Contain))
                    })
                    .when(preview.is_none(), |tile| {
                        tile.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(10.))
                                .child(
                                    div().w(relative(0.8)).h(relative(0.6)).child(
                                        img(preview_art())
                                            .size_full()
                                            .object_fit(ObjectFit::Contain),
                                    ),
                                )
                                .child(text(
                                    if allowed {
                                        dc_intl::t!("uQZTBV")
                                    } else {
                                        dc_intl::t!("pgUTZC")
                                    },
                                    14.,
                                    18.,
                                    400.,
                                    "--text-default",
                                )),
                        )
                    })
                    .when(allowed, |tile| {
                        tile.cursor_pointer()
                            .on_click(cx.listener(move |root, _, _, cx| {
                                if !root.active_now_can_watch(&destination) {
                                    return;
                                }
                                if destination.guild == 0 {
                                    root.join_dm_call(destination.channel, false, cx)
                                } else {
                                    root.open_voice_view(
                                        destination.guild,
                                        destination.channel,
                                        cx,
                                    );
                                    root.request_watch_voice(
                                        destination.guild,
                                        destination.channel,
                                        destination.owner,
                                        cx,
                                    );
                                    return;
                                }
                                if root.call.as_ref().is_some_and(|call| {
                                    call.guild == destination.guild
                                        && call.channel == destination.channel
                                }) {
                                    root.watch_stream_of(destination.owner, cx)
                                }
                            }))
                    }),
            )
            .into_any_element()
    }
}

#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_active_now(&mut self, variant: &str, cx: &mut Context<Self>) -> String {
        if !crate::inert::on() {
            return "synthetic activity requires inert capture".into();
        }
        if !matches!(
            variant,
            "game" | "stream" | "spotify" | "spotify-party" | "voice" | "watch-fixture" | "empty"
        ) {
            return "unknown activity fixture".into();
        }
        self.select_home(None);
        self.stores.relationships.clear();
        self.stores.consents.insert("personalization".into(), false);
        let count = if variant == "empty" {
            0
        } else if matches!(variant, "spotify-party" | "voice") {
            2
        } else {
            1
        };
        for index in 0..count {
            let id = if index == 0 {
                100_000_000_000_000_001
            } else {
                100_000_000_000_000_005
            };
            self.stores.dispatch("RELATIONSHIP_ADD", &json!({"id":id.to_string(),"type":1,"user":{
                "id":id.to_string(),"username":if index==0{"Reference Player"}else{"Reference Guest"},"avatar":"reference"}}));
            let activity = if variant.starts_with("spotify") {
                json!({"type":2,"name":"Spotify","details":"Reference Track","state":"Reference Artist",
                "party":{"id":"spotify:reference-session"},"session_id":"spotify:reference-session","sync_id":"reference-track","flags":48,
                "assets":{"large_image":"mp://discord.com/assets/58188da2d047938e.svg","large_text":"Reference Album"},
                "timestamps":{"start":now()-60_000,"end":now()+120_000}})
            } else {
                json!({"type":0,"name":"Reference Game",
                    "application_id":"100000000000000002","timestamps":{"start":now()-7_200_000}})
            };
            self.stores.dispatch(
                "PRESENCE_UPDATE",
                &json!({"user":{"id":id.to_string()},"status":"online","activities":if variant=="voice"{json!([])}else{json!([activity])}}),
            );
            if let Some(user) = self.stores.users.get(&id) {
                let avatar=br##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"><rect width="64" height="64" fill="#5865f2"/><circle cx="32" cy="32" r="16" fill="white"/></svg>"##;
                let rendered = Image::from_bytes(ImageFormat::Svg, avatar.to_vec())
                    .to_image_data(cx.svg_renderer())
                    .unwrap();
                let size = rendered.size(0);
                let mut pixels = rendered.as_bytes(0).unwrap().to_vec();
                for pixel in pixels.as_chunks_mut::<4>().0.iter_mut() {
                    pixel.swap(0, 2);
                }
                let pixels = image::RgbaImage::from_raw(
                    u32::from(size.width),
                    u32::from(size.height),
                    pixels,
                )
                .unwrap();
                let mut png = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(pixels)
                    .write_to(&mut png, image::ImageFormat::Png)
                    .unwrap();
                let art = Arc::new(Image::from_bytes(ImageFormat::Png, png.into_inner()));
                self.images
                    .insert(super::avatar_source(user).0, art.clone());
                self.images.insert(
                    "https://cdn.discordapp.com/app-icons/100000000000000002/reference.png?size=64"
                        .into(),
                    art,
                );
            }
        }
        self.fetched.insert(
            "presence-app:100000000000000002".into(),
            json!([{"id":"100000000000000002","name":"Reference Game","icon":"reference"}]),
        );
        if variant == "voice" {
            self.stores.dispatch("GUILD_CREATE", &json!({"id":"100000000000000003","name":"Reference Server",
                "owner_id":self.stores.current_user.as_ref().map_or(0,|user|user.id).to_string(),"channels":[{"id":"100000000000000004","name":"Reference Voice","type":2}],
                "voice_states":[{"user_id":"100000000000000001","channel_id":"100000000000000004","session_id":"one","discoverable":true},
                    {"user_id":"100000000000000005","channel_id":"100000000000000004","session_id":"two","discoverable":true}]}));
        }
        if matches!(variant, "stream" | "watch-fixture") {
            let (guild, channel) = if variant == "watch-fixture" {
                ("1568692564637524109", "1568692565136375901")
            } else {
                ("100000000000000003", "100000000000000004")
            };
            self.stores.dispatch(
                "VOICE_STATE_UPDATE",
                &json!({"guild_id":guild,"channel_id":channel,
                "user_id":"100000000000000001","session_id":"synthetic","self_stream":true}),
            );
        }
        cx.notify();
        format!("synthetic {variant}; local stores only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_game_icons_follow_hash_and_proxy_asset_routes() {
        assert_eq!(
            game_icon_url(&json!({"id":"42","media":{"icon":{"type":"hash","value":"icon"}}}))
                .as_deref(),
            Some(
                "https://cdn.discordapp.com/app-icons/42/icon.png?size=64&keep_aspect_ratio=false"
            )
        );
        assert_eq!(game_icon_url(&json!({"media":{"icon":{"type":"url","value":"https://images-ext-1.discordapp.net/external/a?width=128&height=128&format=png"}}})).as_deref(),Some("https://images-ext-1.discordapp.net/external/a?format=png&width=64&height=64"));
        let small = "https://cdn.discordapp.com/app-icons/42/icon.png?size=32";
        assert_eq!(
            game_icon_url(&json!({"media":{"icon":{"type":"url","value":small}}})).as_deref(),
            Some(small)
        );
        let media = "https://media.discordapp.net/app-icons/42/icon.png?size=128";
        assert_eq!(
            game_icon_url(&json!({"media":{"icon":{"type":"url","value":media}}})).as_deref(),
            Some(media)
        );
        assert!(game_icon_url(&json!({"media":{"icon":{"type":"hash","value":""}}})).is_none());
    }
}
