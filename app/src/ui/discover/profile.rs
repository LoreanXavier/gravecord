//! The server profile popout a directory card's corner button opens (`eA` of module 300290 wraps module 517905, whose
//! lazy body is module 191565). It reads `GET /guilds/{id}/profile` (`GUILD_PROFILE`, fetched by module 250527 `L4` and
//! parsed by module 9994 `wr`) and draws the profile card of module 42780: the banner (361 of 649640), the squircle icon,
//! the name with its badge and server tag (86376), the counts and founding month, the description, favourite games and
//! traits (136696) and the call to action (123213, rules from 461888). The footer is module 191565's Server Tag upsell.
//! Measured in `design/dom/discover/server-card-preview-darker-cozy.json`.
//!
//! The Join button is `joinGuild` with `lurker: true` (module 449054 `Z2`), the request a card click sends: it is an
//! account write, and a harness run refuses it centrally.
use super::data::{self, GuildCard};
use super::text;
use crate::parity::Parity;
use crate::ui::{Root, color, icon};
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, ObjectFit, Pixels, SharedString, Window,
    anchored, deferred, div, img, point, prelude::*, px,
};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Module 250527 `L4` asks again once the profile it holds is older than a minute.
const FRESH: Duration = Duration::from_secs(60);
/// The `1` of `1 << 22` below the Discord epoch: a snowflake's time is its high bits plus this many milliseconds.
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
/// Module 191565: a profile of a server the viewer may not see answers this code.
const MISSING_ACCESS: u64 = 50_001;
/// Module 3666 `Wv`: traits wrap over two lines with a 4px gap.
const TRAIT_LINES: usize = 2;
const TRAIT_GAP: f32 = 4.;
/// A trait chip: 8px of padding and a 1px border on each side of its text.
const TRAIT_CHROME: f32 = 18.;
/// The popout is 300 wide and sits 8px to the left of its button.
const WIDTH: f32 = 300.;
const BUTTON_GAP: f32 = 8.;
/// Module 191565's popout keeps this much clear of the window edge (the measured bottom margin).
const EDGE_MARGIN: f32 = 12.;
/// The `GUILD_TAGS` guild feature: servers that have it can already carry a tag.
const GUILD_TAGS: &str = "GUILD_TAGS";
/// The experiment behind the upsell footer (module 191565 `f`): variations 1 and 2 turn it on.
const UPSELL_EXPERIMENT: &str = "2026-10-server-tag-upsell-on-guild-profile";

/// A server's profile (`wr` of module 9994).
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Profile {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    pub custom_banner: Option<String>,
    pub online: u64,
    pub members: u64,
    pub brand_color: Option<String>,
    pub visibility: i64,
    /// The trait labels in position order (empty slots dropped).
    pub traits: Vec<String>,
    pub games: Vec<String>,
    /// Game id to `(activity level, activity score)`.
    pub activity: HashMap<String, (i64, i64)>,
    pub features: Vec<String>,
    pub tag: Option<String>,
    pub badge_hash: Option<String>,
    pub boosts: u64,
    pub tier: u64,
}

impl Profile {
    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|known| known == feature)
    }
}

/// Module 9994 `wr`: the profile's fields; traits go to their `position` (0 to 4).
pub(in crate::ui) fn parse_profile(body: &Value) -> Option<Profile> {
    let id = data::id_of(&body["id"])?;
    let mut slots: Vec<Option<String>> = vec![None; 5];
    for entry in body["traits"].as_array().into_iter().flatten() {
        let (Some(position), Some(label)) = (entry["position"].as_u64(), entry["label"].as_str())
        else {
            continue;
        };
        if position < 5 {
            slots[position as usize] = Some(label.to_owned());
        }
    }
    let activity = body["game_activity"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(game, value)| {
            (
                game.clone(),
                (
                    value["activity_level"].as_i64().unwrap_or(0),
                    value["activity_score"].as_i64().unwrap_or(0),
                ),
            )
        })
        .collect();
    Some(Profile {
        id,
        name: body["name"].as_str().unwrap_or_default().to_owned(),
        description: body["description"].as_str().unwrap_or_default().to_owned(),
        icon: data::text_of(&body["icon_hash"]),
        custom_banner: data::text_of(&body["custom_banner_hash"]),
        online: body["online_count"].as_u64().unwrap_or(0),
        members: body["member_count"].as_u64().unwrap_or(0),
        brand_color: data::text_of(&body["brand_color_primary"]),
        visibility: body["visibility"].as_i64().unwrap_or(0),
        // `traits.filter(label.length > 0)` of module 136696.
        traits: slots
            .into_iter()
            .flatten()
            .filter(|label| !label.is_empty())
            .collect(),
        games: body["game_application_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|game| game.as_str().map(str::to_owned))
            .collect(),
        activity,
        features: body["features"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|feature| feature.as_str().map(str::to_owned))
            .collect(),
        tag: data::text_of(&body["tag"]),
        badge_hash: data::text_of(&body["badge_hash"]),
        boosts: body["premium_subscription_count"].as_u64().unwrap_or(0),
        tier: body["premium_tier"].as_u64().unwrap_or(0),
    })
}

/// `GET /guilds/{id}/profile` (`GUILD_PROFILE`).
pub(in crate::ui) fn profile_route(guild: u64) -> String {
    format!("/guilds/{guild}/profile")
}

/// Module 107123 `P`: `toLocaleDateString(locale, {year: "numeric", month: "short"})` of the snowflake's time
/// ("Jan 2024").
pub(in crate::ui) fn established(guild: u64) -> String {
    let millis = (guild >> 22) + DISCORD_EPOCH_MS;
    chrono::DateTime::from_timestamp_millis(millis as i64)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%b %Y")
                .to_string()
        })
        .unwrap_or_default()
}

/// Module 649640 `_3`: a server's own banner is its discovery splash, and only a discoverable server shows it.
fn banner_url(profile: &Profile) -> Option<String> {
    let hash = profile.custom_banner.as_ref()?;
    profile.has("DISCOVERABLE").then(|| {
        format!(
            "https://cdn.discordapp.com/discovery-splashes/{}/{hash}.jpg?size=512",
            profile.id
        )
    })
}

/// A hex colour (`#rrggbb`) as the banner's base colour.
fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let text = text.strip_prefix('#').unwrap_or(text);
    if text.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(text, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

/// What the popout shows of a profile request.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) enum Fetch {
    Loading,
    Ready(Box<Profile>),
    /// Code 50001: the server limits who can see its profile (module 576055).
    Limited,
    /// Any other failure (module 191565 `R`).
    Failed,
}

/// Which server's popout is open and where its button is.
#[derive(Clone, Debug)]
pub(in crate::ui) struct Popout {
    pub guild: u64,
    pub anchor: Bounds<Pixels>,
}

/// The call to action under the card (module 461888's `ctaType`, module 123213's labels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cta {
    GoToServer,
    Join,
}

impl Cta {
    fn label(self) -> &'static str {
        match self {
            Cta::GoToServer => "Go to Server",
            Cta::Join => "Join",
        }
    }
}

/// Module 461888, for the cases this client can act on: a member goes to the server, a discoverable server is joined as
/// a preview. Adopt Tag, View Application Status, Apply to Join and Accept have no flow here (BLOCKED.md).
fn cta_for(member: bool, profile: &Profile) -> Option<Cta> {
    if member {
        Some(Cta::GoToServer)
    } else if profile.has("DISCOVERABLE") {
        Some(Cta::Join)
    } else {
        None
    }
}

/// Module 3666 `Wv`'s fit: the index of the last chip that stays when `widths` wrap over `lines` lines of `limit`
/// pixels and the second line also has to leave room for the "+ N more" chip (`overflow` wide).
fn last_visible(widths: &[f32], lines: usize, limit: f32, overflow: f32) -> usize {
    let (mut line, mut used, mut last) = (0usize, 0f32, 0usize);
    for (index, width) in widths.iter().enumerate() {
        if width.is_nan() || *width > limit {
            continue;
        }
        let final_line = line == lines - 1;
        let is_last = index == widths.len() - 1;
        let need = used
            + width
            + if final_line && !is_last {
                overflow + TRAIT_GAP
            } else {
                0.
            };
        if need.round() >= limit.round() {
            if final_line {
                break;
            }
            line += 1;
            used = 0.;
        }
        last = index;
        used += width + TRAIT_GAP;
    }
    last
}

/// The width a run of text takes at a size and weight, as the text system shapes it.
fn measure(window: &mut Window, text: &str, size: f32, weight: f32) -> f32 {
    if text.is_empty() {
        return 0.;
    }
    let mut run_font = gpui::font(crate::ui::SANS);
    run_font.weight = FontWeight(weight);
    run_font.features = crate::ui::kerning();
    let run = gpui::TextRun {
        len: text.len(),
        font: run_font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(SharedString::from(text.to_owned()), px(size), &[run], None)
            .width,
    )
}

/// Module 107123 and 86376 `p`: "1 Member" or "2 Members" (`zRl6XR`).
fn member_label(count: u64) -> String {
    if count == 1 {
        "1 Member".to_owned()
    } else {
        format!("{} Members", data::group_digits(count))
    }
}

impl Root {
    /// The corner button was pressed: the popout opens beside it and its profile is asked for (module 191565 calls
    /// `fetchGuildProfile`, which skips the request while the held profile is under a minute old).
    pub(in crate::ui) fn servers_open_profile(&mut self, guild: u64, cx: &mut Context<Self>) {
        let Some(anchor) = self
            .tip_anchors
            .borrow()
            .get(&SharedString::from(format!("discover-card-more-{guild}")))
            .copied()
        else {
            return;
        };
        self.discover.servers.popout = Some(Popout { guild, anchor });
        let fresh = self
            .discover
            .servers
            .profile_times
            .get(&guild)
            .is_some_and(|at| at.elapsed() < FRESH)
            && matches!(
                self.discover.servers.profiles.get(&guild),
                Some(Fetch::Ready(_))
            );
        if !fresh {
            self.discover.servers.profiles.insert(guild, Fetch::Loading);
            let key = format!("discover:profile:{guild}");
            self.discover_forget(&key);
            self.discover_request(key, profile_route(guild));
        }
        cx.notify();
    }

    pub(in crate::ui) fn servers_close_profile(&mut self) -> bool {
        self.discover.servers.popout.take().is_some()
    }

    /// The profile answer: the popout's state, and the pictures it draws.
    pub(super) fn servers_profile_fetched(&mut self, id: &str, key: &str) {
        let Ok(guild) = id.parse::<u64>() else { return };
        let body = self.fetched.get(key).cloned().unwrap_or_default();
        if crate::net::is_fetch_failure(&body) {
            let limited = body["body"]["code"].as_u64() == Some(MISSING_ACCESS);
            self.discover.servers.profiles.insert(
                guild,
                if limited {
                    Fetch::Limited
                } else {
                    Fetch::Failed
                },
            );
            return;
        }
        let Some(profile) = parse_profile(&body) else {
            self.discover.servers.profiles.insert(guild, Fetch::Failed);
            return;
        };
        if let Some(icon) = &profile.icon {
            self.request_image(
                format!("guild:{guild}"),
                format!("https://cdn.discordapp.com/icons/{guild}/{icon}.webp?size=96"),
            );
        }
        if let Some(url) = banner_url(&profile) {
            self.discover_image(&url);
        }
        if let (Some(tag), Some(hash)) = (&profile.tag, &profile.badge_hash) {
            let chip = dc_state::ServerTag {
                guild_id: guild,
                badge: Some(hash.clone()),
                tag: tag.clone(),
            };
            if let Some(url) = chip.badge_url() {
                self.request_image(url.clone(), url);
            }
        }
        self.request_games(&profile.games);
        self.discover
            .servers
            .profile_times
            .insert(guild, Instant::now());
        self.discover
            .servers
            .profiles
            .insert(guild, Fetch::Ready(Box::new(profile)));
    }

    /// The window-level layers of the Discover page: the profile popout and the Student Hub dialog.
    pub(in crate::ui) fn discover_layers(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if !self.discover_active() {
            return Vec::new();
        }
        let mut layers = Vec::new();
        layers.extend(self.server_profile_popout(window, cx));
        layers.extend(self.hub_dialog_layer(cx));
        layers
    }

    /// Escape closes whichever Discover layer is open, the dialog first.
    pub(in crate::ui) fn discover_escape(&mut self) -> bool {
        self.hub_dialog_close() || self.servers_close_profile()
    }

    fn server_profile_popout(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.discover.servers.popout.clone()?;
        let card = match self
            .discover
            .servers
            .profiles
            .get(&open.guild)
            .cloned()
            .unwrap_or(Fetch::Loading)
        {
            Fetch::Ready(profile) => self.profile_card(&profile, window, cx),
            Fetch::Loading => shell().child(self.profile_loading()).into_any_element(),
            Fetch::Limited => self.profile_limited(open.guild, cx),
            Fetch::Failed => self.profile_failed(open.guild, cx),
        };
        let popup = div()
            .id("discover-profile-popout")
            .occlude()
            .parity("discover-profile-popout")
            .w(px(WIDTH))
            .child(card);
        Some(
            deferred(
                div()
                    .id("discover-profile-trap")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.servers_close_profile();
                        cx.notify();
                    }))
                    .child(
                        anchored()
                            .anchor(gpui::Anchor::TopRight)
                            .position(point(
                                open.anchor.left() - px(BUTTON_GAP),
                                open.anchor.top(),
                            ))
                            .snap_to_window_with_margin(px(EDGE_MARGIN))
                            .child(popup),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The pulsing ellipsis stands in a card-sized box while the profile arrives.
    fn profile_loading(&self) -> AnyElement {
        div()
            .w_full()
            .h(px(160.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .children((0..3).map(|_| {
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(color("--text-muted"))
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// Module 576055: the banner colour, the icon and the name, and a line that the server limits who sees its profile.
    fn profile_limited(&self, guild: u64, cx: &mut Context<Self>) -> AnyElement {
        let name = self.discover_card_name(guild);
        let footer = self.profile_upsell(cx);
        shell()
            .child(self.profile_banner(guild, None, None))
            .child(self.profile_icon(guild, &name))
            .child(
                text(20., 24., 500., "--text-strong")
                    .px(px(16.))
                    .mt(px(8.))
                    .child(if name.is_empty() {
                        "Private Server".to_owned()
                    } else {
                        name
                    }),
            )
            .child(
                text(14., 18., 400., "--text-muted")
                    .px(px(16.))
                    .pt(px(8.))
                    .pb(px(16.))
                    .child("The server has limited who can see this profile."),
            )
            .children(footer)
            .into_any_element()
    }

    /// Module 191565 `R`: the name, and a retry line in the warning colour.
    fn profile_failed(&self, guild: u64, cx: &mut Context<Self>) -> AnyElement {
        let name = self.discover_card_name(guild);
        shell()
            .child(self.profile_banner(guild, None, None))
            .child(self.profile_icon(guild, &name))
            .child(
                text(20., 24., 500., "--text-strong")
                    .px(px(16.))
                    .mt(px(8.))
                    .child(if name.is_empty() {
                        "Unknown Server".to_owned()
                    } else {
                        name
                    }),
            )
            .child(
                div()
                    .id("discover-profile-retry")
                    .mx(px(16.))
                    .mt(px(8.))
                    .mb(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .child(icon("shop_circle_error", 20., "--text-feedback-warning"))
                    .child(
                        text(14., 18., 400., "--text-feedback-warning")
                            .child("Unable to load server profile"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.discover.servers.profiles.insert(guild, Fetch::Loading);
                        let key = format!("discover:profile:{guild}");
                        this.discover_forget(&key);
                        this.discover_request(key, profile_route(guild));
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// The name of the directory card the popout came from (the profile's own name once it is there).
    fn discover_card_name(&self, guild: u64) -> String {
        let from_lists = self
            .discover
            .servers
            .featured
            .ready()
            .into_iter()
            .flatten()
            .chain(
                self.discover
                    .servers
                    .by_category
                    .values()
                    .filter_map(|load| load.ready())
                    .flatten(),
            )
            .chain(
                self.discover
                    .servers
                    .results
                    .iter()
                    .flat_map(|results| results.guilds.iter()),
            )
            .find(|card: &&GuildCard| card.id == guild)
            .map(|card| card.name.clone());
        from_lists.unwrap_or_default()
    }

    /// `banner_Xweo-z`: 120 tall; the discovery splash when the server has one, else the radial gradient of
    /// module 649640 `n6` over the brand colour or the icon's main colour.
    fn profile_banner(
        &self,
        guild: u64,
        profile: Option<&Profile>,
        splash: Option<String>,
    ) -> AnyElement {
        let picture = splash
            .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned())
            .or_else(|| {
                let base = profile
                    .and_then(|profile| profile.brand_color.as_deref())
                    .and_then(parse_hex)
                    .unwrap_or_else(|| self.guild_banner_base(guild));
                Some(self.banner_image(base, 298, 120))
            });
        div()
            .flex_none()
            .w_full()
            .h(px(120.))
            .overflow_hidden()
            .children(picture.map(|image| img(image).size_full().object_fit(ObjectFit::Cover)))
            .into_any_element()
    }

    /// `avatarContainer_Xweo-z`: the 64px icon in a 70px squircle of the card colour, 34px up over the banner.
    fn profile_icon(&self, guild: u64, name: &str) -> AnyElement {
        let picture = self.images.get(&format!("guild:{guild}")).cloned();
        let initials: String = name
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .collect();
        div()
            .flex_none()
            .pl(px(16.))
            .mt(px(-34.))
            .child(
                div()
                    .size(px(70.))
                    .rounded(px(21.))
                    .p(px(3.))
                    .bg(color("--background-surface-highest"))
                    .child(
                        div()
                            .size(px(64.))
                            .rounded(px(19.))
                            .overflow_hidden()
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(match picture {
                                Some(image) => img(image)
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .into_any_element(),
                                None => text(18., 22., 500., "--text-default")
                                    .child(initials)
                                    .into_any_element(),
                            }),
                    ),
            )
            .into_any_element()
    }

    /// The tag chip after the name: `chipletContainerInner_k8jyN7`, 16 tall with a 14px badge.
    fn profile_tag_chip(&self, guild: u64, profile: &Profile) -> Option<AnyElement> {
        let tag = profile.tag.clone().filter(|tag| !tag.is_empty())?;
        let chip = dc_state::ServerTag {
            guild_id: guild,
            badge: profile.badge_hash.clone(),
            tag,
        };
        Some(
            self.server_tag_chip(&chip)
                .ml(px(0.))
                .mt(px(-2.))
                .into_any_element(),
        )
    }

    /// Module 877126: the 18px flower for a verified or partnered server, the discoverable community mark otherwise.
    fn profile_badge(profile: &Profile) -> Option<AnyElement> {
        let partner = profile.has("PARTNERED");
        let verified = profile.has("VERIFIED");
        if !(partner || verified || profile.has("DISCOVERABLE") || profile.has("COMMUNITY")) {
            return None;
        }
        let tint = if partner && !verified {
            "--brand-500"
        } else {
            "--green-360"
        };
        let inner = if partner && !verified {
            "discover_flower_partner"
        } else {
            "discover_flower_check"
        };
        Some(
            div()
                .flex_none()
                .w(px(20.))
                .h(px(18.))
                .pl(px(1.))
                .child(
                    div()
                        .relative()
                        .size(px(18.))
                        .child(icon("discover_flower", 18., tint))
                        .child(
                            div()
                                .absolute()
                                .top(px(2.5))
                                .left(px(2.5))
                                .child(icon(inner, 13., "--white")),
                        ),
                )
                .into_any_element(),
        )
    }

    fn profile_games(&mut self, profile: &Profile) -> Option<AnyElement> {
        // Module 136696: the games the server lists, the most played first, five of them at most.
        let mut known: Vec<(String, String, Option<std::sync::Arc<gpui::Image>>)> = Vec::new();
        for id in &profile.games {
            let Some(game) = self.games.get(id).cloned() else {
                continue;
            };
            let icon_url = game.icon.clone()?;
            let key = format!("game-icon:{icon_url}");
            self.request_image(key.clone(), icon_url);
            known.push((
                id.clone(),
                game.name.clone(),
                self.images.get(&key).cloned(),
            ));
        }
        known.sort_by_key(|(id, _, _)| {
            std::cmp::Reverse(profile.activity.get(id).map_or(0, |(_, score)| *score))
        });
        known.truncate(5);
        if known.is_empty() {
            return None;
        }
        let single = known.len() == 1;
        let tile = |id: &String, picture: Option<std::sync::Arc<gpui::Image>>| {
            let busy = profile
                .activity
                .get(id)
                .is_some_and(|(level, _)| *level == 2);
            div()
                .relative()
                .flex_none()
                .size(px(26.))
                .child(
                    div()
                        .size(px(26.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .overflow_hidden()
                        .children(picture.map(|image| img(image).size_full())),
                )
                .when(busy, |this| {
                    this.child(
                        div()
                            .absolute()
                            .top(px(-4.))
                            .left(px(16.))
                            .size(px(14.))
                            .rounded_full()
                            .bg(color("--background-surface-highest"))
                            .p(px(2.))
                            .child(icon("discover_fire", 10., "--orange-330")),
                    )
                })
        };
        Some(
            div()
                .flex_none()
                .h(px(26.))
                .flex()
                .items_center()
                .gap(px(if single { 8. } else { 4. }))
                .children(known.iter().map(|(id, name, picture)| {
                    let tile = tile(id, picture.clone());
                    if single {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(tile)
                            .child(text(14., 18., 500., "--text-default").child(name.clone()))
                            .into_any_element()
                    } else {
                        tile.into_any_element()
                    }
                }))
                .into_any_element(),
        )
    }

    /// Module 136696 `M`: the trait chips, two lines at most, the rest as a "+ N more" chip.
    fn profile_traits(&self, profile: &Profile, window: &mut Window) -> Option<AnyElement> {
        if profile.traits.is_empty() {
            return None;
        }
        let limit = WIDTH - 34.;
        let widths: Vec<f32> = profile
            .traits
            .iter()
            .map(|label| measure(window, label, 14., 400.) + TRAIT_CHROME)
            .collect();
        // The measuring chip reads "+ 1N more", the item count with a 1 in front of it.
        let probe = format!("+ 1{} more", profile.traits.len());
        let overflow = measure(window, &probe, 14., 400.) + TRAIT_CHROME;
        let last = last_visible(&widths, TRAIT_LINES, limit, overflow);
        let hidden = profile.traits.len() - (last + 1);
        let chip = |label: String| {
            text(14., 18., 400., "--text-default")
                .flex_none()
                .px(px(8.))
                .py(px(4.))
                .rounded(px(16.))
                .border_1()
                .border_color(color("--border-subtle"))
                .child(label)
        };
        let mut chips: Vec<AnyElement> = profile.traits[..=last]
            .iter()
            .map(|label| chip(label.clone()).into_any_element())
            .collect();
        if hidden > 0 {
            chips.push(chip(format!("+ {hidden} more")).into_any_element());
        }
        Some(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .gap(px(TRAIT_GAP))
                .children(chips)
                .into_any_element(),
        )
    }

    /// The upsell footer (module 191565 `k`): "Want your own Server Tag?" and Create one.
    fn profile_upsell(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let creatable = self.creatable_tag_guilds();
        let on = matches!(
            self.stores.experiments.variant(UPSELL_EXPERIMENT),
            Some(1 | 2)
        );
        if !on || creatable.is_empty() {
            return None;
        }
        let target = creatable[0];
        Some(
            div()
                .flex_none()
                .w_full()
                .h(px(43.))
                .px(px(16.))
                .bg(color("--background-surface-high"))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(text(14., 18., 400., "--text-muted").child("Want your own Server Tag?"))
                .child(
                    text(14., 18., 500., "--text-brand")
                        .id("discover-profile-create-tag")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // Module 152313: one server goes straight to its Server Tag page; several ask which first
                            // (that picker is not built, so the first by name is used).
                            this.discover.servers.popout = None;
                            this.discover.open = false;
                            this.open_server_settings_at(
                                target,
                                crate::ui::server_settings::Page::Tag,
                                cx,
                            );
                            cx.notify();
                        }))
                        .child("Create one"),
                )
                .into_any_element(),
        )
    }

    /// Module 2967: the viewer's servers that do not carry a tag yet and where they are an administrator, by name.
    fn creatable_tag_guilds(&self) -> Vec<u64> {
        let Some(me) = self.stores.me() else {
            return Vec::new();
        };
        let mut found: Vec<(String, u64)> = self
            .stores
            .guilds
            .iter()
            .filter(|guild| !guild.features.iter().any(|feature| feature == GUILD_TAGS))
            .filter(|guild| {
                dc_state::permissions::can_in_guild(guild, me, dc_state::permissions::ADMINISTRATOR)
            })
            .map(|guild| (guild.name.to_lowercase(), guild.id))
            .collect();
        found.sort();
        found.into_iter().map(|(_, id)| id).collect()
    }

    /// The loaded profile card (`container_Xweo-z` of module 42780).
    fn profile_card(
        &mut self,
        profile: &Profile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let guild = profile.id;
        let member = self.stores.guilds.iter().any(|known| known.id == guild);
        let cta = cta_for(member, profile);
        let games = self.profile_games(profile);
        let traits = self.profile_traits(profile, window);
        let badge = Self::profile_badge(profile);
        let chip = self.profile_tag_chip(guild, profile);
        let upsell = if profile.tag.is_some() {
            self.profile_upsell(cx)
        } else {
            None
        };
        let count_row = |dot: &'static str, label: String| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().flex_none().size(px(8.)).rounded_full().bg(color(dot)))
                .child(text(14., 18., 400., "--text-subtle").child(label))
        };
        let body_present = !profile.description.is_empty() || games.is_some() || traits.is_some();
        shell()
            .child(self.profile_banner(guild, Some(profile), banner_url(profile)))
            .child(self.profile_icon(guild, &profile.name))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .px(px(16.))
                    .mt(px(8.))
                    .child(
                        div()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                text(16., 20., 600., "--text-strong")
                                    .min_w(px(0.))
                                    .truncate()
                                    .child(profile.name.clone()),
                            )
                            .children(badge)
                            .children(chip),
                    )
                    .child(
                        div()
                            .h(px(18.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(count_row(
                                "--green-360",
                                format!("{} Online", data::group_digits(profile.online)),
                            ))
                            .child(count_row(
                                "--icon-status-offline",
                                member_label(profile.members),
                            )),
                    )
                    .child(
                        text(14., 18., 400., "--text-subtle")
                            .h(px(18.))
                            .child(format!("Est. {}", established(guild))),
                    ),
            )
            .when(body_present, |this| {
                this.child(
                    div()
                        .flex_none()
                        .w_full()
                        .px(px(16.))
                        .mt(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .when(!profile.description.is_empty(), |body| {
                            body.child(
                                text(14., 18., 400., "--text-subtle")
                                    .child(profile.description.clone()),
                            )
                        })
                        .children(games)
                        .children(traits),
                )
            })
            .children(cta.map(|cta| {
                div()
                    .flex_none()
                    .w_full()
                    .px(px(16.))
                    .mt(px(16.))
                    .pb(px(16.))
                    .child(self.profile_cta(guild, cta, cx))
            }))
            .when(cta.is_none(), |this| this.child(div().h(px(16.))))
            .children(upsell)
            .into_any_element()
    }

    /// `button_F7qvhY` sm active: 266 by 32, white text on the connected green.
    fn profile_cta(&self, guild: u64, cta: Cta, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("discover-profile-cta")
            .parity("discover-profile-cta")
            .w_full()
            .h(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-connected-border-default"))
            .bg(color("--control-connected-background-default"))
            .hover(|style| style.bg(color("--control-connected-background-hover")))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-connected-text-default"))
            .child(cta.label())
            .on_click(cx.listener(move |this, _, _, cx| {
                // Module 123213 `C`: the action runs, then the popout closes (`onComplete`).
                match cta {
                    Cta::GoToServer => {
                        this.discover.servers.popout = None;
                        this.discover.open = false;
                        this.select(guild, None);
                    }
                    Cta::Join => {
                        this.servers_join(guild, cx);
                        this.discover.servers.popout = None;
                    }
                }
                cx.notify();
            }))
            .into_any_element()
    }
}

/// `container_klbANn` over `container_Xweo-z`: the card colour with a 1px ring, 16px corners and the popout shadow.
fn shell() -> gpui::Div {
    div()
        .w(px(WIDTH))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(16.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-highest"))
        .shadow(vec![
            gpui::BoxShadow {
                color: gpui::hsla(0., 0., 1., 0.08),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            },
            gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            },
        ])
}

/// The request a Join press sends, for the tests: the lurker PUT a card click sends.
#[cfg(test)]
fn join_request(guild: u64, session: Option<&str>) -> crate::net::Request {
    super::servers::lurk_request(guild, session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Request;
    use serde_json::json;

    #[test]
    fn the_profile_is_read_from_the_guild_profile_route() {
        assert_eq!(
            profile_route(1193841000108531764),
            "/guilds/1193841000108531764/profile"
        );
    }

    #[test]
    fn a_profile_reads_the_fields_of_module_9994() {
        let body = json!({
            "id": "1193841000108531764", "name": "Marvel Rivals", "description": "The official server",
            "icon_hash": "abc", "custom_banner_hash": "def", "online_count": 1060621, "member_count": 4636516,
            "brand_color_primary": "#112233", "visibility": 2,
            "traits": [{"label": "PS4", "position": 4}, {"label": "Steam", "position": 0}, {"label": "", "position": 2}],
            "game_application_ids": ["1314395942253756416"],
            "game_activity": {"1314395942253756416": {"activity_level": 2, "activity_score": 9}},
            "features": ["DISCOVERABLE", "VERIFIED"], "tag": "RVLS", "badge_hash": "8e22",
            "premium_subscription_count": 2259, "premium_tier": 3
        });
        let profile = parse_profile(&body).unwrap();
        assert_eq!(profile.id, 1193841000108531764);
        assert_eq!(profile.traits, vec!["Steam".to_owned(), "PS4".to_owned()]);
        assert_eq!(profile.activity["1314395942253756416"], (2, 9));
        assert_eq!(profile.tag.as_deref(), Some("RVLS"));
        assert_eq!(profile.members, 4636516);
        assert_eq!(
            banner_url(&profile).as_deref(),
            Some(
                "https://cdn.discordapp.com/discovery-splashes/1193841000108531764/def.jpg?size=512"
            )
        );
        assert_eq!(parse_hex("#112233"), Some([0x11, 0x22, 0x33]));
        // Without the discoverable feature the banner is the generated gradient.
        let plain = Profile {
            features: vec![],
            ..profile
        };
        assert_eq!(banner_url(&plain), None);
    }

    #[test]
    fn the_founding_month_is_the_snowflakes_month() {
        // The captured server: created in January 2024 (snowflake 1193841000108531764).
        let text = established(1193841000108531764);
        assert!(text == "Jan 2024" || text == "Dec 2023", "{text}");
        assert_eq!(member_label(1), "1 Member");
        assert_eq!(member_label(4636516), "4,636,516 Members");
    }

    #[test]
    fn the_call_to_action_follows_membership_and_discoverability() {
        let discoverable = Profile {
            features: vec!["DISCOVERABLE".into()],
            ..Profile::default()
        };
        assert_eq!(cta_for(true, &discoverable), Some(Cta::GoToServer));
        assert_eq!(cta_for(false, &discoverable), Some(Cta::Join));
        assert_eq!(cta_for(false, &Profile::default()), None);
    }

    #[test]
    fn join_is_the_lurker_put_and_nothing_else() {
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = join_request(7, Some("s"))
        else {
            panic!("a rest request");
        };
        assert_eq!(method, reqwest::Method::PUT);
        assert_eq!(route, "/guilds/7/members/@me?lurker=true&session_id=s");
        assert_eq!(body, Some(json!({})));
        // A harness run refuses it centrally.
        assert!(crate::net::refuses_in_capture(&method, &route));
    }

    #[test]
    fn traits_wrap_over_two_lines_and_the_rest_is_counted() {
        // The captured chips (widths from the DOM) fit: 4 on the first line, PS4 on the second.
        let widths = [56.28, 88.22, 51.66, 41.69, 41.97];
        assert_eq!(last_visible(&widths, 2, 266., 74.33), 4);
        // Eight wide chips: the second line keeps what fits next to the overflow chip.
        let wide = [120.0; 8];
        assert_eq!(last_visible(&wide, 2, 266., 74.33), 2);
    }
}
