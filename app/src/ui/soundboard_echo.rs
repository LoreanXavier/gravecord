//! Soundboard Echo store, floating combo pills, milestone celebration banner, and Sound Details popout
//! (module 120246 `SoundboardEchoStore`, `eS`, `eE`, `ec`, `eu`, `ea`, `eo`, `ep`, `ex`; styles modules 858981
//! `bwNRJo` and 998812 `Ag-7tW`).
//!
//! Tracks `VOICE_CHANNEL_EFFECT_SEND` soundboard plays with a 7.5s active window (`T`), merges repeated plays into an
//! echo pill (`xN`), shows who played the sound on hover, expands "See Sound Details" (Discord Default, Private Server,
//! Member Server with Nitro upsell, or Discoverable Server with Preview Server), and triggers combo milestone callouts
//! at 100..100000 echoes.

use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

/// `T` of module 120246: 7.5 seconds before an echo expires.
pub(super) const ECHO_TTL: Duration = Duration::from_millis(7_500);
/// Key prefix for discoverable guild lookup (`796774 nh`: `GET /soundboard-sounds/{sound_id}/guild`).
pub(super) const ECHO_GUILD_PREFIX: &str = "soundboard-echo-guild:";
/// Combo milestones (`ef` of module 120246).
const COMBO_MILESTONES: &[u32] = &[100, 200, 366, 513, 789, 1_000, 5_000, 10_000, 100_000];

/// A single active or recently expired soundboard echo (`SoundboardEchoStore`).
#[derive(Clone, Debug)]
pub(crate) struct EchoEntry {
    pub sound_id: String,
    pub sound_name: String,
    pub source_guild_id: String,
    pub emoji_id: Option<String>,
    pub emoji_name: Option<String>,
    pub emoji_animated: bool,
    pub author_id: Id,
    pub most_recent_user_id: Option<Id>,
    pub played_by: Vec<Id>,
    pub count: u32,
    pub started_at: Instant,
    pub last_played_at: Instant,
    pub expired: bool,
}

/// Source guild state for "See Sound Details" (`eu` of module 120246).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SourceGuildState {
    DefaultSound,
    Loading,
    Unavailable,
    Member { guild_id: Id, name: String },
    Discoverable { guild_id: Id, name: String },
}

/// State held on `Root` (`SoundboardEchoStore` + hover/details UI).
#[derive(Default)]
pub(crate) struct SoundboardEchoState {
    pub channel: Option<Id>,
    pub active: BTreeMap<String, EchoEntry>,
    pub expired: BTreeMap<String, EchoEntry>,
    pub hovered_list: bool,
    pub hovered_sound: Option<String>,
    pub details_open_sound: Option<String>,
    pub discoverable_guilds: BTreeMap<String, Option<(Id, String)>>,
    pub pending_lookups: BTreeSet<String>,
    pub combo_banner: Option<(u32, Instant)>,
}

/// Formats the "Played by ..." description (`ec` of module 120246).
pub(super) fn format_played_by(names: &[String], total_count: usize) -> String {
    let shown: Vec<&str> = names.iter().take(3).map(String::as_str).collect();
    let extra = total_count.saturating_sub(shown.len()) as i64;
    match shown.as_slice() {
        [a] if extra > 0 => dc_intl::format("EXaquk", &[("a", (*a).into()), ("n", extra.into())]),
        [a] => dc_intl::format("DU+Rd+", &[("a", (*a).into())]),
        [a, b] if extra > 0 => dc_intl::format(
            "EO8JM8",
            &[("a", (*a).into()), ("b", (*b).into()), ("n", extra.into())],
        ),
        [a, b] => dc_intl::format("qr8Cg3", &[("a", (*a).into()), ("b", (*b).into())]),
        [a, b, c] if extra > 0 => dc_intl::format(
            "+z+JUw",
            &[
                ("a", (*a).into()),
                ("b", (*b).into()),
                ("c", (*c).into()),
                ("n", extra.into()),
            ],
        ),
        [a, b, c] => dc_intl::format(
            "3Zq6FM",
            &[("a", (*a).into()), ("b", (*b).into()), ("c", (*c).into())],
        ),
        _ => dc_intl::format("P5JeeM", &[("n", (total_count as i64).into())]),
    }
}

/// Combo milestone celebration string (`ep` of module 120246).
pub(super) fn combo_milestone_text(milestone: u32) -> &'static str {
    match milestone {
        100 => dc_intl::t("ZctfZ/"),
        200 => dc_intl::t("LKiPeX"),
        366 => dc_intl::t("KiYrRC"),
        513 => dc_intl::t("Ec5lqb"),
        789 => dc_intl::t("24Kzud"),
        1_000 => dc_intl::t("5/WCf/"),
        5_000 => dc_intl::t("5ifydJ"),
        10_000 => dc_intl::t("N8tPFd"),
        100_000 => dc_intl::t("6hvboB"),
        _ => "",
    }
}

impl SoundboardEchoState {
    /// Clears echoes when switching voice channels (`VOICE_CHANNEL_SELECT`).
    pub(super) fn set_channel(&mut self, channel: Option<Id>) {
        if self.channel == channel {
            return;
        }
        self.channel = channel;
        self.active.clear();
        self.expired.clear();
        self.hovered_sound = None;
        self.details_open_sound = None;
        self.combo_banner = None;
    }

    /// Records a `VOICE_CHANNEL_EFFECT_SEND` soundboard play.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record_play(
        &mut self,
        channel_id: Id,
        sound_id: String,
        sound_name: String,
        source_guild_id: String,
        emoji_id: Option<String>,
        emoji_name: Option<String>,
        emoji_animated: bool,
        author_id: Id,
        played_by_user_id: Id,
        is_echo: bool,
        now: Instant,
    ) {
        self.channel = Some(channel_id);
        self.expired.remove(&sound_id);
        let next_count = if let Some(existing) = self.active.get_mut(&sound_id) {
            existing.count += 1;
            if !existing.played_by.contains(&played_by_user_id) {
                existing.played_by.push(played_by_user_id);
            }
            if played_by_user_id != existing.author_id {
                existing.most_recent_user_id = Some(played_by_user_id);
            }
            existing.last_played_at = now;
            existing.count
        } else {
            self.active.insert(
                sound_id.clone(),
                EchoEntry {
                    sound_id,
                    sound_name,
                    source_guild_id,
                    emoji_id,
                    emoji_name,
                    emoji_animated,
                    author_id,
                    most_recent_user_id: is_echo.then_some(played_by_user_id),
                    played_by: vec![played_by_user_id],
                    count: 1,
                    started_at: now,
                    last_played_at: now,
                    expired: false,
                },
            );
            1
        };
        if COMBO_MILESTONES.contains(&next_count) {
            self.combo_banner = Some((next_count, now));
        }
    }

    /// Expires entries older than `ECHO_TTL`.
    pub(super) fn sweep_expired(&mut self, now: Instant) -> bool {
        let mut changed = false;
        let stale: Vec<String> = self
            .active
            .iter()
            .filter(|(_, e)| now.saturating_duration_since(e.last_played_at) >= ECHO_TTL)
            .map(|(k, _)| k.clone())
            .collect();
        for id in stale {
            if let Some(mut entry) = self.active.remove(&id) {
                entry.expired = true;
                self.expired.insert(id, entry);
                changed = true;
            }
        }
        if let Some((_, at)) = self.combo_banner
            && now.saturating_duration_since(at) >= Duration::from_millis(1_500)
        {
            self.combo_banner = None;
            changed = true;
        }
        changed
    }

    /// Returns up to the 3 most recent echoes to display (`eS`).
    pub(super) fn visible_echoes(&self) -> Vec<EchoEntry> {
        let mut items: Vec<EchoEntry> = if self.hovered_list {
            self.active
                .values()
                .cloned()
                .chain(self.expired.values().cloned())
                .collect()
        } else {
            self.active.values().cloned().collect()
        };
        items.sort_by_key(|e| e.started_at);
        if items.len() > 3 {
            items = items.split_off(items.len() - 3);
        }
        items.reverse();
        items
    }
}

impl Root {
    /// Handles a gateway `VOICE_CHANNEL_EFFECT_SEND` payload for soundboard echoes.
    pub(super) fn soundboard_echo_event(&mut self, payload: &Value, cx: &mut Context<Self>) {
        let Some(sound_id) = payload["sound_id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| payload["sound_id"].as_u64().map(|n| n.to_string()))
        else {
            return;
        };
        let Some(channel_id) = payload["channel_id"]
            .as_str()
            .and_then(|s| s.parse::<Id>().ok())
        else {
            return;
        };
        let Some(user_id) = payload["user_id"]
            .as_str()
            .and_then(|s| s.parse::<Id>().ok())
        else {
            return;
        };
        let author_id = payload["author_id"]
            .as_str()
            .and_then(|s| s.parse::<Id>().ok())
            .unwrap_or(user_id);
        let sound_name = payload["sound_name"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| self.soundboard_sound_name(&sound_id))
            .unwrap_or_else(|| sound_id.clone());
        let source_guild_id = payload["source_guild_id"]
            .as_str()
            .unwrap_or("0")
            .to_owned();
        let emoji_id = payload["emoji"]["id"].as_str().map(str::to_owned);
        let emoji_name = payload["emoji"]["name"].as_str().map(str::to_owned);
        let emoji_animated = payload["emoji"]["animated"].as_bool().unwrap_or(false);
        let is_echo = payload["is_echo"].as_bool().unwrap_or(false);
        if let Some(id) = emoji_id.as_deref() {
            let ext = if emoji_animated { "gif" } else { "webp" };
            self.request_image(
                format!("custom:{id}"),
                format!("https://cdn.discordapp.com/emojis/{id}.{ext}?size=48"),
            );
        }

        self.soundboard_echo.record_play(
            channel_id,
            sound_id,
            sound_name,
            source_guild_id,
            emoji_id,
            emoji_name,
            emoji_animated,
            author_id,
            user_id,
            is_echo,
            Instant::now(),
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ECHO_TTL).await;
            let _ = this.update(cx, |this, cx| {
                if this.soundboard_echo.sweep_expired(Instant::now()) {
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn soundboard_sound_name(&self, sound_id: &str) -> Option<String> {
        self.soundboard
            .defaults
            .iter()
            .chain(self.soundboard.guilds.values().flatten())
            .find(|s| s.id == sound_id)
            .map(|s| s.name.clone())
    }

    /// Resolves `SourceGuildState` (`eu` of module 120246).
    fn echo_source_guild_state(&self, entry: &EchoEntry) -> SourceGuildState {
        if entry.source_guild_id == "0" || entry.source_guild_id.is_empty() {
            return SourceGuildState::DefaultSound;
        }
        if let Ok(gid) = entry.source_guild_id.parse::<Id>()
            && let Some(guild) = self.stores.guild(gid)
        {
            return SourceGuildState::Member {
                guild_id: gid,
                name: guild.name.clone(),
            };
        }
        match self
            .soundboard_echo
            .discoverable_guilds
            .get(&entry.sound_id)
        {
            Some(Some((gid, name))) => SourceGuildState::Discoverable {
                guild_id: *gid,
                name: name.clone(),
            },
            Some(None) => SourceGuildState::Unavailable,
            None => SourceGuildState::Loading,
        }
    }

    /// Requests discoverable source guild info (`796774 nh`) when expanding details for an external sound.
    fn request_echo_source_guild(&mut self, sound_id: &str, source_guild_id: &str) {
        if source_guild_id == "0" || source_guild_id.is_empty() {
            return;
        }
        if let Ok(gid) = source_guild_id.parse::<Id>()
            && self.stores.guild(gid).is_some()
        {
            return;
        }
        if self
            .soundboard_echo
            .discoverable_guilds
            .contains_key(sound_id)
            || !self
                .soundboard_echo
                .pending_lookups
                .insert(sound_id.to_owned())
        {
            return;
        }
        let _ = self.requests.send(Request::Fetch {
            key: format!("{ECHO_GUILD_PREFIX}{sound_id}"),
            route: format!("/guilds/{source_guild_id}/preview"),
        });
    }

    /// Handles the discoverable guild response for an echo sound.
    pub(super) fn soundboard_echo_guild_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(sound_id) = key.strip_prefix(ECHO_GUILD_PREFIX) else {
            return;
        };
        self.soundboard_echo.pending_lookups.remove(sound_id);
        let body = self.fetched.get(key).cloned().unwrap_or(Value::Null);
        let found = body["id"]
            .as_str()
            .and_then(|s| s.parse::<Id>().ok())
            .and_then(|gid| body["name"].as_str().map(|n| (gid, n.to_owned())));
        self.soundboard_echo
            .discoverable_guilds
            .insert(sound_id.to_owned(), found);
        cx.notify();
    }

    /// Capture helper: `soundboard-echo [details]` stages a live soundboard echo + details popout for visual verification.
    #[cfg(feature = "capture")]
    pub(crate) fn script_soundboard_echo(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let me = self.stores.me().map(|u| u.id).unwrap_or(1);
        let guild = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id));
        let channel = self.channel.unwrap_or(1);
        let source_guild = guild
            .map(|g| g.to_string())
            .unwrap_or_else(|| "0".to_owned());
        let now = Instant::now();
        self.soundboard_echo.record_play(
            channel,
            "999001".to_owned(),
            "Airhorn".to_owned(),
            source_guild,
            None,
            Some("\u{1f4ef}".to_owned()),
            false,
            me,
            me,
            false,
            now,
        );
        self.soundboard_echo.record_play(
            channel,
            "999001".to_owned(),
            "Airhorn".to_owned(),
            "0".to_owned(),
            None,
            Some("\u{1f4ef}".to_owned()),
            false,
            me,
            me,
            true,
            now,
        );
        if arg.contains("details") || arg.contains("popout") {
            self.soundboard_echo.hovered_sound = Some("999001".to_owned());
            if arg.contains("details") {
                self.soundboard_echo.details_open_sound = Some("999001".to_owned());
            }
        }
        cx.notify();
        format!("soundboard-echo: staged Airhorn x2 ({arg})")
    }

    /// Renders the Soundboard Echo stack + hover Sound Details popout (module 120246 `eS`).
    pub(super) fn soundboard_echo_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let echoes = self.soundboard_echo.visible_echoes();
        if echoes.is_empty() {
            return None;
        }
        let has_nitro = self.stores.account.premium_type.unwrap_or(0) >= 2;

        let mut stack = div()
            .id("soundboard-echo-list")
            .absolute()
            .bottom(px(84.))
            .right(px(24.))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.));

        // Milestone combo banner (`ep`) if active.
        if let Some((milestone, _)) = self.soundboard_echo.combo_banner {
            let text = combo_milestone_text(milestone);
            if !text.is_empty() {
                stack = stack.child(
                    div()
                        .px(px(14.))
                        .py(px(6.))
                        .rounded(px(10.))
                        .bg(color("--brand-500"))
                        .text_size(px(20.))
                        .font_weight(FontWeight(800.))
                        .text_color(gpui::rgb(0xffffff))
                        .child(text),
                );
            }
        }

        for entry in echoes {
            let sid = entry.sound_id.clone();
            let sid_hover = sid.clone();
            let sid_click = sid.clone();
            let sid_details = sid.clone();
            let src_gid = entry.source_guild_id.clone();
            let show_popout = self.soundboard_echo.hovered_sound.as_deref() == Some(sid.as_str());
            let show_details =
                self.soundboard_echo.details_open_sound.as_deref() == Some(sid.as_str());

            let author_avatar = self.stores.users.get(&entry.author_id).and_then(|u| {
                let (key, _) = avatar_source(u);
                self.images.get(&key).cloned()
            });
            let second_avatar = entry
                .most_recent_user_id
                .filter(|&uid| uid != entry.author_id)
                .and_then(|uid| self.stores.users.get(&uid))
                .and_then(|u| {
                    let (key, _) = avatar_source(u);
                    self.images.get(&key).cloned()
                });

            let played_names: Vec<String> = entry
                .played_by
                .iter()
                .filter_map(|uid| {
                    self.stores
                        .users
                        .get(uid)
                        .map(|u| u.display_name().to_owned())
                })
                .collect();
            let played_by_text = format_played_by(&played_names, entry.played_by.len());

            let popout_emoji: Option<AnyElement> = entry
                .emoji_id
                .as_deref()
                .and_then(|id| self.images.get(&format!("custom:{id}")).cloned())
                .map(|image| {
                    let _animated = entry.emoji_animated;
                    img(image).size(px(40.)).flex_none().into_any_element()
                })
                .or_else(|| {
                    entry.emoji_name.clone().map(|em| {
                        div()
                            .size(px(40.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(28.))
                            .child(em)
                            .into_any_element()
                    })
                });
            let pill_emoji: Option<AnyElement> = entry
                .emoji_id
                .as_deref()
                .and_then(|id| self.images.get(&format!("custom:{id}")).cloned())
                .map(|image| img(image).size(px(20.)).flex_none().into_any_element())
                .or_else(|| {
                    entry
                        .emoji_name
                        .clone()
                        .map(|em| div().text_size(px(16.)).child(em).into_any_element())
                });

            // Hover popout (`ec` / `container_bwNRJo`, 288px wide, 16px radius).
            let popout = show_popout.then(|| {
                let details_section: AnyElement = if show_details {
                    match self.echo_source_guild_state(&entry) {
                        SourceGuildState::Loading => div()
                            .p(px(16.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .bg(color("--background-mod-subtle")),
                            )
                            .child(
                                div()
                                    .w(px(120.))
                                    .h(px(14.))
                                    .rounded(px(4.))
                                    .bg(color("--background-mod-subtle")),
                            )
                            .into_any_element(),
                        SourceGuildState::DefaultSound => div()
                            .p(px(16.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("home", 20., "--text-muted")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("rWPrJC")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("ixMwGl")),
                                    ),
                            )
                            .into_any_element(),
                        SourceGuildState::Unavailable => div()
                            .p(px(16.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("eye_slash", 20., "--text-muted")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("T7AWtb")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("Z4UD3R")),
                                    ),
                            )
                            .into_any_element(),
                        SourceGuildState::Member { name, .. } => {
                            let initial = name.chars().next().unwrap_or('?').to_string();
                            let upsell_copy = dc_intl::t!("/ZJIuo", guildName = name.as_str());
                            div()
                                .p(px(16.))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-higher"))
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .size(px(40.))
                                                .rounded(px(10.))
                                                .bg(color("--brand-500"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(16.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(gpui::rgb(0xffffff))
                                                .child(initial),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--text-strong"))
                                                        .child(name),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .line_height(px(16.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("l3QzyC")),
                                                ),
                                        ),
                                )
                                .when(!has_nitro, |d| {
                                    d.child(
                                        div()
                                            .pt(px(12.))
                                            .border_t_1()
                                            .border_color(color("--border-subtle"))
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(color("--text-muted"))
                                                    .child(upsell_copy),
                                            )
                                            .child(
                                                div()
                                                    .id("soundboard-echo-get-nitro")
                                                    .h(px(32.))
                                                    .w_full()
                                                    .rounded(px(8.))
                                                    .bg(color("--brand-500"))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(gpui::rgb(0xffffff))
                                                    .child(dc_intl::t("pj0XBN"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.open_nitro(cx);
                                                    })),
                                            ),
                                    )
                                })
                                .into_any_element()
                        }
                        SourceGuildState::Discoverable { guild_id, name } => {
                            let initial = name.chars().next().unwrap_or('?').to_string();
                            div()
                                .p(px(16.))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-higher"))
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .size(px(40.))
                                                .rounded(px(10.))
                                                .bg(color("--brand-500"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(16.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(gpui::rgb(0xffffff))
                                                .child(initial),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--text-strong"))
                                                        .child(name),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .line_height(px(16.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("s5No7R")),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .id("soundboard-echo-preview-server")
                                                .w_full()
                                                .py(px(6.))
                                                .rounded(px(8.))
                                                .bg(color("--background-mod-subtle"))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--interactive-text-default"))
                                                .child(dc_intl::t("BETi5Z"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.select(guild_id, None);
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("QzKwEQ")),
                                        ),
                                )
                                .into_any_element()
                        }
                    }
                } else {
                    div()
                        .px(px(16.))
                        .pb(px(16.))
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "echo-details-btn-{sid_details}"
                                )))
                                .w_full()
                                .py(px(6.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-subtle"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(4.))
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--interactive-text-default"))
                                .child(dc_intl::t("UQT3/h"))
                                .child(icon(
                                    "bot_chevron_small_right",
                                    14.,
                                    "--interactive-text-default",
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.soundboard_echo.details_open_sound =
                                        Some(sid_details.clone());
                                    this.request_echo_source_guild(&sid_details, &src_gid);
                                    cx.notify();
                                })),
                        )
                        .into_any_element()
                };

                div()
                    .w(px(288.))
                    .mb(px(6.))
                    .rounded(px(16.))
                    .overflow_hidden()
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .shadow_xl()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .p(px(16.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .children(popout_emoji)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(entry.sound_name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-muted"))
                                            .child(played_by_text),
                                    ),
                            ),
                    )
                    .child(details_section)
            });

            let _play_aria = dc_intl::t!("VOmeSq", name = entry.sound_name.as_str());
            let expired = entry.expired;
            let pill_body = div()
                .id(SharedString::from(format!("soundboard-echo-pill-{sid}")))
                .h(px(36.))
                .px(px(10.))
                .rounded(px(10.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .when(expired, |d| d.opacity(0.5))
                .when(!expired, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.border_color(color("--brand-500")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(ch) = this.soundboard_echo.channel {
                                let _ = this.requests.send(Request::Rest {
                                    key: None,
                                    method: reqwest::Method::POST,
                                    route: format!("/channels/{ch}/send-soundboard-sound"),
                                    body: Some(serde_json::json!({
                                        "sound_id": sid_click,
                                    })),
                                });
                            }
                            cx.notify();
                        }))
                })
                .flex()
                .items_center()
                .gap(px(8.))
                .children(pill_emoji)
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(entry.sound_name.clone()),
                )
                .child(div().w(px(1.)).h(px(20.)).bg(color("--border-subtle")))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(format!("x{}", entry.count)),
                );

            let pill: AnyElement = if expired {
                self.with_tip(
                    format!("echo-expired-tip-{sid}"),
                    super::tooltip::Tip::body(
                        super::tooltip::Body::Text(dc_intl::t("VqNHoS").into()),
                        super::tooltip::Side::Top,
                    ),
                    pill_body,
                    cx,
                )
                .into_any_element()
            } else {
                pill_body.into_any_element()
            };

            let avatar_stack = div()
                .id(SharedString::from(format!("soundboard-echo-avatars-{sid}")))
                .cursor_pointer()
                .flex()
                .items_center()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.soundboard_echo.hovered_sound.as_deref() == Some(sid_hover.as_str()) {
                        this.soundboard_echo.hovered_sound = None;
                        this.soundboard_echo.details_open_sound = None;
                    } else {
                        this.soundboard_echo.hovered_sound = Some(sid_hover.clone());
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(32.))
                        .rounded_full()
                        .bg(color("--brand-500"))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(author_avatar.map(|a| img(a).size(px(32.)).rounded_full())),
                )
                .children(second_avatar.map(|a| {
                    div()
                        .ml(px(-8.))
                        .size(px(32.))
                        .rounded_full()
                        .overflow_hidden()
                        .border_2()
                        .border_color(color("--background-base-lowest"))
                        .child(img(a).size(px(28.)).rounded_full())
                }));

            stack = stack.child(
                div().flex().flex_col().items_end().children(popout).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(avatar_stack)
                        .child(pill),
                ),
            );
        }

        Some(stack.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn played_by_formats_all_plural_and_count_branches() {
        assert_eq!(format_played_by(&["Alice".into()], 1), "Played by Alice");
        assert_eq!(
            format_played_by(&["Alice".into()], 2),
            "Played by Alice and 1 other"
        );
        assert_eq!(
            format_played_by(&["Alice".into()], 4),
            "Played by Alice and 3 others"
        );
        assert_eq!(
            format_played_by(&["Alice".into(), "Bob".into()], 2),
            "Played by Alice and Bob"
        );
        assert_eq!(
            format_played_by(&["Alice".into(), "Bob".into()], 5),
            "Played by Alice, Bob, and 3 others"
        );
        assert_eq!(
            format_played_by(&["Alice".into(), "Bob".into(), "Cara".into()], 3),
            "Played by Alice, Bob, and Cara"
        );
        assert_eq!(
            format_played_by(&["Alice".into(), "Bob".into(), "Cara".into()], 5),
            "Played by Alice, Bob, Cara, and 2 others"
        );
        assert_eq!(format_played_by(&[], 1), "Played by 1 person");
        assert_eq!(format_played_by(&[], 4), "Played by 4 people");
    }

    #[test]
    fn echo_store_merges_combos_and_expires_after_ttl() {
        let mut store = SoundboardEchoState::default();
        let t0 = Instant::now();
        store.record_play(
            10,
            "s1".into(),
            "Airhorn".into(),
            "0".into(),
            None,
            Some("🎺".into()),
            false,
            100,
            100,
            false,
            t0,
        );
        store.record_play(
            10,
            "s1".into(),
            "Airhorn".into(),
            "0".into(),
            None,
            Some("🎺".into()),
            false,
            100,
            200,
            true,
            t0 + Duration::from_secs(1),
        );
        let visible = store.visible_echoes();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].count, 2);
        assert_eq!(visible[0].played_by, vec![100, 200]);
        assert_eq!(visible[0].most_recent_user_id, Some(200));

        assert!(store.sweep_expired(t0 + Duration::from_secs(10)));
        assert!(store.active.is_empty());
        assert_eq!(store.expired.len(), 1);
    }

    #[test]
    fn all_soundboard_echo_strings_and_milestones_resolve() {
        for key in [
            "pj0XBN", "BETi5Z", "QzKwEQ", "rWPrJC", "ixMwGl", "T7AWtb", "Z4UD3R", "l3QzyC",
            "s5No7R", "UQT3/h", "VqNHoS",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "key {key} empty");
        }
        assert!(dc_intl::t!("/ZJIuo", guildName = "Acme").contains("Acme"));
        assert!(dc_intl::t!("VOmeSq", name = "Airhorn").contains("Airhorn"));
        for &m in COMBO_MILESTONES {
            assert!(!combo_milestone_text(m).is_empty(), "milestone {m} empty");
        }
    }
}
