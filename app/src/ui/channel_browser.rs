//! Member channel browser: 624186, 406997, 543465 and 816662.
mod preview;
use super::{Root, color, editor_style, icon};
use crate::ui::motion::Transitions as _;
use crate::{
    editor::{Editor, EditorEvent},
    net::Request,
    parity::Parity,
};
use dc_state::{Channel, ChannelKind, Guild, Id, permissions};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, ScrollHandle, SharedString,
    Subscription, div, img, prelude::*, px,
};
use serde_json::json;
use std::collections::BTreeMap;

const OPT_IN: u64 = 4096;
const FAVORITE: u64 = 2048;
const OPT_IN_GUILD: u64 = 16384;
const INTRO: u16 = 98;
const NEW_BADGE: u16 = 133;
const INTRO_ART: &str = "https://discord.com/assets/b755a73425584e3c.svg";
const ONBOARDING: &str = "channel-browser:onboarding:";
const INTRO_TITLE: &str = "Choose Your Channels";
const INTRO_DESCRIPTION: &str =
    "Customize what's on your channel list, so you see just the stuff you care about.";
const INTRO_TIP: &str =
    "Tip: Channels with mentions will always appear on your channel list, even if you hide them.";

pub(super) struct Browser {
    guild: Id,
    pub(super) search: Entity<Editor>,
    _events: Subscription,
    query: String,
    scroll: ScrollHandle,
    preview_width: f32,
    resize_from: Option<(f32, f32)>,
    action_focus: BTreeMap<Id, gpui::FocusHandle>,
}

fn has_prompts(guild: &Guild) -> bool {
    guild
        .features
        .iter()
        .any(|feature| feature == "GUILD_ONBOARDING_HAS_PROMPTS")
}

fn label(guild: &Guild) -> &'static str {
    if has_prompts(guild) {
        "Channels & Roles"
    } else {
        "Browse Channels"
    }
}

fn follows(flags: u64) -> bool {
    flags & OPT_IN != 0
}

fn follows_channel(settings: &dc_state::guild_settings::GuildSettings, channel: &Channel) -> bool {
    settings
        .channels
        .get(&channel.id)
        .is_some_and(|entry| follows(entry.flags))
        || channel.parent_id.is_some_and(|parent| {
            settings
                .channels
                .get(&parent)
                .is_some_and(|entry| follows(entry.flags))
        })
}

fn follow_flags(flags: u64, on: bool) -> u64 {
    if on {
        flags | OPT_IN
    } else {
        flags & !(OPT_IN | FAVORITE)
    }
}

fn legacy_selection(guild: &Guild, settings: &dc_state::guild_settings::GuildSettings) -> bool {
    settings.flags & OPT_IN_GUILD == 0
        && guild.my_flags & 2 == 0
        && !settings
            .channels
            .values()
            .any(|channel| follows(channel.flags))
}

fn follow_changes(
    settings: &dc_state::guild_settings::GuildSettings,
    legacy: bool,
    channel: &Channel,
    available: impl Iterator<Item = Id>,
    defaults: &[Id],
) -> BTreeMap<Id, u64> {
    let old = |id| settings.channels.get(&id).map_or(0, |entry| entry.flags);
    if legacy {
        available
            .filter(|id| *id != channel.id)
            .chain((channel.kind == ChannelKind::Category).then_some(channel.id))
            .chain(defaults.iter().copied())
            .map(|id| (id, follow_flags(old(id), true)))
            .collect()
    } else {
        BTreeMap::from([(
            channel.id,
            follow_flags(old(channel.id), !follows(old(channel.id))),
        )])
    }
}

/// 91871 compares UTF-16 code units in order; topics use a substring instead.
fn search_matches(channel: &Channel, query: &str) -> bool {
    let name = channel.name.to_lowercase();
    let mut units = name.encode_utf16();
    query
        .encode_utf16()
        .all(|unit| units.any(|known| known == unit))
        || channel
            .topic
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(query)
}

impl Root {
    /// Synthetic opt-in state on the fixture; account and guild writes stay disabled.
    #[cfg(feature = "capture")]
    pub(crate) fn script_browser(&mut self, action: &str, cx: &mut Context<Self>) -> String {
        const FIXTURE: Id = 1568692564637524109;
        if action == "fixture" {
            if self.guild != Some(FIXTURE)
                || std::env::var("GRAVECORD_NO_SETTINGS_WRITE").as_deref() != Ok("1")
            {
                return "fixture setup refused".into();
            }
            let Some(guild) = self
                .stores
                .guilds
                .iter_mut()
                .find(|guild| guild.id == FIXTURE)
            else {
                return "fixture unavailable".into();
            };
            if !guild.features.iter().any(|feature| feature == "COMMUNITY") {
                guild.features.push("COMMUNITY".into());
            }
            let settings = self
                .stores
                .guild_settings
                .guilds
                .entry(FIXTURE)
                .or_default();
            settings.flags |= OPT_IN_GUILD;
            for channel in &guild.channels {
                let entry = settings.channels.entry(channel.id).or_insert(
                    dc_state::guild_settings::Override {
                        message_notifications: dc_state::guild_settings::INHERIT,
                        ..Default::default()
                    },
                );
                entry.flags = follow_flags(
                    entry.flags,
                    channel.kind == ChannelKind::Category || channel.parent_id.is_none(),
                );
            }
            self.open_channel_browser(cx);
        }
        let Some(guild) = self.current_guild() else {
            return "no guild".into();
        };
        let followed = guild
            .channels
            .iter()
            .filter(|channel| {
                follows(
                    self.stores
                        .guild_settings
                        .channel(guild.id, channel.id)
                        .flags,
                )
            })
            .count();
        let visible = guild
            .channels
            .iter()
            .filter(|channel| self.followed_sidebar_row(guild, channel))
            .count();
        let query = self
            .channel_browser
            .as_ref()
            .map_or("", |browser| browser.query.as_str());
        format!(
            "open={} preview={} width={} followed={followed} sidebar={visible} query={query:?}",
            self.channel_browser_open,
            self.channel_browser_open && self.channel.is_some(),
            self.browser_preview_width()
        )
    }

    pub(super) fn sidebar_channel_attention(&self, guild: &Guild, channel: &Channel) -> bool {
        self.channel == Some(channel.id)
            || self
                .call
                .as_ref()
                .is_some_and(|call| call.channel == channel.id)
            || self.stores.read_states.mentions(channel.id) > 0
            || guild.threads.iter().any(|thread| {
                thread.parent_id == channel.id
                    && (self.channel == Some(thread.id)
                        || (!thread.archived
                            && thread.joined
                            && self.stores.read_states.mentions(thread.id) > 0))
            })
    }

    pub(super) fn followed_sidebar_row(&self, guild: &Guild, channel: &Channel) -> bool {
        let Some(settings) = self.stores.guild_settings.guilds.get(&guild.id) else {
            return true;
        };
        if settings.flags & OPT_IN_GUILD == 0
            || !guild.features.iter().any(|feature| feature == "COMMUNITY")
        {
            return true;
        }
        if channel.kind == ChannelKind::Category {
            let children: Vec<_> = guild
                .channels
                .iter()
                .filter(|child| {
                    child.kind != ChannelKind::Category && child.parent_id == Some(channel.id)
                })
                .collect();
            return if children.is_empty() {
                follows_channel(settings, channel)
                    && self.can_in_channel(
                        channel,
                        permissions::MANAGE_CHANNELS | permissions::VIEW_CHANNEL,
                    )
            } else {
                children
                    .iter()
                    .any(|child| self.followed_sidebar_row(guild, child))
            };
        }
        if self.has_guild_guide() && channel.flags & 128 != 0 {
            return self.channel == Some(channel.id);
        }
        // Keep selected/mentioned channels reachable until their separate Suggested section is rendered.
        follows_channel(settings, channel) || self.sidebar_channel_attention(guild, channel)
    }

    pub(super) fn has_channel_browser(&self) -> bool {
        self.current_guild().is_some_and(|guild| {
            guild.features.iter().any(|feature| feature == "COMMUNITY")
                && !guild.features.iter().any(|feature| feature == "HUB")
                && (has_prompts(guild)
                    || self
                        .stores
                        .guild_settings
                        .guilds
                        .get(&guild.id)
                        .is_some_and(|settings| settings.flags & OPT_IN_GUILD != 0))
        })
    }

    fn dismiss_browser_content(&mut self, content: u16) {
        if self.stores.content_dismissed(content) {
            return;
        }
        let patch = self.stores.dismiss_content(content);
        if !crate::storage::settings_writes_blocked() {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".into(),
                body: Some(json!({"settings":patch})),
            });
        }
    }

    pub(super) fn open_channel_browser(&mut self, cx: &mut Context<Self>) {
        if !self.has_channel_browser() {
            return;
        }
        let from = self.route();
        let mut to = from;
        to.channel = None;
        to.guild_guide = false;
        to.channel_browser = true;
        self.history.visit(from, to);
        self.channel = None;
        self.guild_guide_open = false;
        self.channel_browser_open = true;
        self.member_view = None;
        self.dismiss_browser_content(NEW_BADGE);
        self.prepare_channel_browser(cx);
        cx.notify();
    }

    pub(super) fn prepare_channel_browser(&mut self, cx: &mut Context<Self>) {
        if !self.channel_browser_open {
            return;
        }
        let Some(guild) = self.guild else {
            return;
        };
        if self
            .channel_browser
            .as_ref()
            .is_none_or(|browser| browser.guild != guild)
        {
            let search = cx.new(|cx| {
                let mut style = editor_style();
                style.size = 16.;
                style.line_height = 20.;
                style.font.weight = FontWeight(400.);
                style.text = super::hsla("--input-text-default");
                style.caret = super::hsla("--input-text-default");
                style.placeholder = super::hsla("--input-placeholder-text-default");
                let mut editor = Editor::new(style, cx);
                editor.set_placeholder("Search Channels".into());
                editor.set_reports_all_edits(true);
                editor
            });
            let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
                let query = match event {
                    EditorEvent::Edited => editor.read(cx).text().to_owned(),
                    EditorEvent::Cancel => {
                        editor.update(cx, |editor, _| editor.set_text(""));
                        String::new()
                    }
                    EditorEvent::Submit(text) => {
                        editor.update(cx, |editor, _| editor.set_text(text));
                        text.clone()
                    }
                };
                if let Some(browser) = this.channel_browser.as_mut() {
                    browser.query = query.to_lowercase();
                    browser.scroll.set_offset(Default::default());
                }
                cx.notify();
            });
            self.channel_browser = Some(Browser {
                guild,
                search,
                _events: events,
                query: String::new(),
                scroll: ScrollHandle::new(),
                preview_width: crate::storage::get("threadSidebarWidth")
                    .and_then(|value| value.as_f64())
                    .map_or(480., |width| (width as f32).clamp(450., 600.)),
                resize_from: None,
                action_focus: BTreeMap::new(),
            });
        }
        if !self.stores.content_dismissed(INTRO) {
            self.request_image(INTRO_ART.into(), INTRO_ART.into());
        }
        let channels: Vec<_> = self
            .current_guild()
            .into_iter()
            .flat_map(|guild| guild.channels.iter().map(|channel| channel.id))
            .collect();
        if let Some(browser) = self.channel_browser.as_mut() {
            for channel in channels {
                browser
                    .action_focus
                    .entry(channel)
                    .or_insert_with(|| cx.focus_handle());
            }
        }
        if self.current_guild().is_some_and(|known| {
            legacy_selection(
                known,
                &self
                    .stores
                    .guild_settings
                    .guilds
                    .get(&guild)
                    .cloned()
                    .unwrap_or_default(),
            )
        }) {
            let key = format!("{ONBOARDING}{guild}");
            if self.requested.insert(key.clone()) {
                let _ = self.requests.send(Request::Fetch {
                    key,
                    route: format!("/guilds/{guild}/onboarding"),
                });
            }
        }
    }

    pub(super) fn channel_browser_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.has_channel_browser() {
            return None;
        }
        let guild = self.current_guild()?;
        let selected = self.channel_browser_open;
        Some(
            div()
                .flex_none()
                .h(px(34.))
                .pt(px(1.))
                .px(px(8.))
                .child(
                    div()
                        .id("channel-browser-link")
                        .group("channel-browser-link")
                        .parity("browser-link")
                        .h(px(32.))
                        .w(px(286.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .when(selected, |row| row.bg(color("--background-mod-normal")))
                        // A channel-like row: `.wrapper:hover .link` and `:active .link` (f4b2f93f8f92b7).
                        .when(!selected, |row| {
                            row.hover(|style| style.bg(color("--interactive-background-hover")))
                        })
                        .active(|style| style.bg(color("--interactive-background-active")))
                        .on_click(cx.listener(|this, _, _, cx| this.open_channel_browser(cx)))
                        .child(icon(
                            "channel_browse",
                            20.,
                            if selected {
                                "--text-strong"
                            } else {
                                "--channels-default"
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(16.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(500.))
                                .text_color(color(if selected {
                                    "--text-strong"
                                } else {
                                    "--channels-default"
                                }))
                                .when(!selected, |name| {
                                    name.group_hover("channel-browser-link", |style| {
                                        style.text_color(color("--interactive-text-hover"))
                                    })
                                })
                                .child(label(guild)),
                        )
                        .when(
                            !selected && !self.stores.content_dismissed(NEW_BADGE),
                            |row| {
                                row.child(
                                    div()
                                        .h(px(16.))
                                        .px(px(8.))
                                        .rounded(px(8.))
                                        .bg(color("--brand-500"))
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child("NEW"),
                                )
                            },
                        ),
                )
                .into_any_element(),
        )
    }

    fn browser_follow(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        let Some(known) = self.stores.guild(guild) else {
            return;
        };
        let Some(target) = known.channels.iter().find(|known| known.id == channel) else {
            return;
        };
        let settings = self
            .stores
            .guild_settings
            .guilds
            .get(&guild)
            .cloned()
            .unwrap_or_default();
        let legacy = legacy_selection(known, &settings);
        let onboarding_key = format!("{ONBOARDING}{guild}");
        if legacy
            && !self.fetched.contains_key(&onboarding_key)
            && self.fetch_failures.remove(&onboarding_key).is_some()
        {
            // The defaults never arrived: this click asks again, the next one follows.
            self.requested.insert(onboarding_key.clone());
            let _ = self.requests.send(Request::Fetch {
                key: onboarding_key,
                route: format!("/guilds/{guild}/onboarding"),
            });
            return;
        }
        let onboarding = self.fetched.get(&onboarding_key);
        if legacy && onboarding.is_none() {
            return;
        }
        let defaults: Vec<Id> = onboarding
            .into_iter()
            .flat_map(|body| body["default_channel_ids"].as_array())
            .flatten()
            .filter_map(|id| id.as_str()?.parse().ok())
            .collect();
        let available = known
            .channels
            .iter()
            .filter(|row| {
                row.kind != ChannelKind::Category
                    && row.kind != ChannelKind::Other(14)
                    && self.can_in_channel(row, permissions::VIEW_CHANNEL)
            })
            .map(|row| row.id);
        let changes = follow_changes(&settings, legacy, target, available, &defaults);
        if legacy && changes.is_empty() {
            return;
        }
        let entry = self.stores.guild_settings.guilds.entry(guild).or_default();
        let enable = entry.flags & OPT_IN_GUILD == 0;
        entry.flags |= OPT_IN_GUILD;
        for (&id, &flags) in &changes {
            entry
                .channels
                .entry(id)
                .or_insert(dc_state::guild_settings::Override {
                    message_notifications: dc_state::guild_settings::INHERIT,
                    ..Default::default()
                })
                .flags = flags;
        }
        let overrides: serde_json::Map<String, serde_json::Value> = changes
            .into_iter()
            .map(|(id, flags)| (id.to_string(), json!({"flags":flags})))
            .collect();
        let mut update = json!({"channel_overrides":overrides});
        if enable {
            update["flags"] = json!(entry.flags);
        }
        if !crate::storage::settings_writes_blocked() {
            let _ = self.requests.send(Request::GuildSettings {
                body: json!({"guilds":{guild.to_string():update}}),
            });
        }
        cx.notify();
    }

    fn browser_intro(&self, cx: &mut Context<Self>) -> AnyElement {
        // Pinned English NUX min-content width, measured in the 600px sidebar reference.
        // Blink's nested flex intrinsic width includes more than the widest individual word.
        let minimum = px(101.015625);
        let piece = |text: &str, weight| {
            self.piece(
                &dc_markdown::Span {
                    text: text.to_owned(),
                    style: Default::default(),
                    kind: dc_markdown::Kind::Text,
                },
                super::hsla("--text-default"),
                16.,
                weight,
                16.,
            )
        };
        let tip = crate::inline::inline(
            vec![piece(&INTRO_TIP[..4], 600.), piece(&INTRO_TIP[4..], 400.)],
            px(20.),
        );
        let description = crate::inline::inline(vec![piece(INTRO_DESCRIPTION, 400.)], px(20.));
        let text = div()
            .flex_shrink_1()
            .min_w(minimum)
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(INTRO_TITLE),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(description),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(tip),
            );
        let art = div()
            .w(px(180.))
            .min_w(px(160.))
            .aspect_ratio(1.6)
            .mb(px(-16.))
            .mr(px(24.))
            .flex_shrink_1()
            .children(
                self.images
                    .get(INTRO_ART)
                    .cloned()
                    .map(|picture| img(picture).size_full()),
            );
        div()
            .parity("browser-intro")
            .relative()
            .rounded(px(8.))
            .p(px(16.))
            .mb(px(24.))
            .bg(color("--background-base-lowest"))
            .child(div().flex().items_center().child(art).child(text))
            .child(
                div()
                    .id("browser-dismiss-intro")
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .size(px(24.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dismiss_browser_content(INTRO);
                        cx.notify();
                    }))
                    .child(icon("ob_x", 24., "--interactive-text-default")),
            )
            .into_any_element()
    }

    fn browser_channel_row(
        &self,
        guild: &Guild,
        channel: &Channel,
        first: bool,
        last: bool,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let parent_followed = channel.parent_id.is_some_and(|parent| {
            follows(self.stores.guild_settings.channel(guild.id, parent).flags)
        });
        let legacy = self
            .stores
            .guild_settings
            .guilds
            .get(&guild.id)
            .map_or(guild.my_flags & 2 == 0, |settings| {
                legacy_selection(guild, settings)
            });
        let checked = parent_followed
            || legacy
            || follows(
                self.stores
                    .guild_settings
                    .channel(guild.id, channel.id)
                    .flags,
            );
        let group = SharedString::from(format!("browser-channel-{}", channel.id));
        let (guild_id, channel_id) = (guild.id, channel.id);
        let focus = self
            .channel_browser
            .as_ref()
            .and_then(|browser| browser.action_focus.get(&channel_id))
            .cloned();
        let focused = focus.as_ref().is_some_and(|focus| focus.is_focused(window));
        let vocal = matches!(channel.kind, ChannelKind::Voice | ChannelKind::Other(13));
        let action = if vocal {
            "Join"
        } else if channel.is_forum() {
            "Open"
        } else {
            "View"
        };
        let tint = if parent_followed {
            "--text-muted"
        } else {
            "--text-default"
        };
        let mut subtitles = Vec::new();
        if channel.is_forum() {
            let count = guild
                .threads
                .iter()
                .filter(|thread| thread.parent_id == channel.id && !thread.archived)
                .count();
            if count > 0 {
                subtitles.push(format!(
                    "{count} Active {}",
                    if count == 1 { "Post" } else { "Posts" }
                ));
            }
        }
        if !vocal {
            let last = self
                .stores
                .read_states
                .latest
                .get(&channel.id)
                .copied()
                .or(channel.last_message_id)
                .unwrap_or(channel.id);
            let age =
                chrono::Utc::now().timestamp() - (((last >> 22) as i64 + 1_420_070_400_000) / 1000);
            subtitles.push(format!("Active {}", super::relative_time(age)));
        }
        if let Some(topic) = &channel.topic
            && !topic.is_empty()
        {
            subtitles.push(topic.clone());
        }
        let row = div()
            .id(group.clone())
            .group(group.clone())
            .px(px(12.))
            .py(px(8.))
            .min_h(px(36.))
            .border_t_1()
            .border_l_1()
            .border_r_1()
            .border_color(color("--border-subtle"))
            .when(last, |row| row.border_b_1().rounded_b(px(8.)))
            .when(first, |row| row.rounded_t(px(8.)))
            .bg(color("--background-base-low"))
            .when(!parent_followed, |row| {
                row.cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-subtle")))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if !parent_followed {
                    this.browser_follow(guild_id, channel_id, cx);
                }
            }))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mr(px(8.))
                            .child(icon(
                                super::server_onboarding::channel_glyph(
                                    guild,
                                    channel,
                                    guild.rules_channel_id,
                                ),
                                20.,
                                "--channel-icon",
                            ))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color(tint))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(channel.name.clone()),
                            ),
                    )
                    .when(!subtitles.is_empty(), |row| {
                        row.child(
                            div()
                                .mr(px(16.))
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-default"))
                                .overflow_hidden()
                                .children(subtitles.iter().enumerate().flat_map(
                                    |(index, text)| {
                                        let mut items = Vec::new();
                                        if index > 0 {
                                            items.push(
                                                div()
                                                    .size(px(4.))
                                                    .mx(px(8.))
                                                    .flex_none()
                                                    .rounded(px(4.))
                                                    .bg(color("--border-subtle"))
                                                    .into_any_element(),
                                            );
                                        }
                                        let topic = index + 1 == subtitles.len()
                                            && channel
                                                .topic
                                                .as_ref()
                                                .is_some_and(|topic| !topic.is_empty());
                                        items.push(
                                            div()
                                                .when(!topic, |text| text.flex_none())
                                                .when(topic, |text| {
                                                    text.min_w(px(0.))
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                })
                                                .child(text.clone())
                                                .into_any_element(),
                                        );
                                        items
                                    },
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id(SharedString::from(format!("browser-view-{channel_id}")))
                            .mr(px(16.))
                            .h(px(32.))
                            .min_w(px(60.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(color("--white"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .cursor_pointer()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--black"))
                            .flex()
                            .hidden()
                            .when(focused, |button| button.flex())
                            .when_some(focus.clone(), |button, focus| button.track_focus(&focus))
                            .group_hover(group, |style| style.flex())
                            .items_center()
                            .justify_center()
                            .child(action)
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        cx.stop_propagation();
                                        this.activate_browser_channel(
                                            guild_id,
                                            channel_id,
                                            event.keystroke.modifiers.shift,
                                            cx,
                                        );
                                    }
                                },
                            ))
                            .on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    cx.stop_propagation();
                                    if let Some(focus) = &focus {
                                        window.focus(focus, cx);
                                    }
                                    this.activate_browser_channel(
                                        guild_id,
                                        channel_id,
                                        event.modifiers().shift,
                                        cx,
                                    );
                                },
                            )),
                    )
                    .child(super::server_onboarding::checkbox(checked, parent_followed)),
            )
            .into_any_element();
        if parent_followed {
            self.with_tip(
                format!("browser-follow-category-{channel_id}"),
                super::tooltip::Tip::text(
                    "You must unfollow this category to add or remove specific channels.",
                    super::tooltip::Side::Top,
                ),
                row,
                cx,
            )
            .into_any_element()
        } else {
            row
        }
    }

    pub(super) fn channel_browser_view(
        &self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
        messages: AnyElement,
    ) -> AnyElement {
        let (Some(guild), Some(browser)) = (self.current_guild(), self.channel_browser.as_ref())
        else {
            return div().into_any_element();
        };
        let guild_id = guild.id;
        let mut groups: Vec<(Option<&Channel>, Vec<&Channel>)> = vec![(None, Vec::new())];
        for channel in guild.sidebar_rows() {
            if channel.kind == ChannelKind::Category {
                groups.push((Some(channel), Vec::new()));
                continue;
            }
            if channel.flags & 128 != 0 && self.has_guild_guide() {
                continue;
            }
            if !search_matches(channel, &browser.query)
                || !self.can_in_channel(channel, permissions::VIEW_CHANNEL)
            {
                continue;
            }
            if let Some((_, rows)) = groups.last_mut() {
                rows.push(channel);
            }
        }
        let mut list = div().px(px(8.)).pb(px(4.)).flex().flex_col();
        if !self.stores.content_dismissed(INTRO) {
            list = list.child(self.browser_intro(cx));
        }
        let mut first_heading = true;
        let mut first_row = true;
        for (category, rows) in groups {
            if rows.is_empty() && !browser.query.is_empty() {
                continue;
            }
            if rows.is_empty() && category.is_none() {
                continue;
            }
            let mut heading = div()
                .pt(px(if category.is_none() { 0. } else { 16. }))
                .pb(px(8.))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(
                            category
                                .map_or("Uncategorized", |category| category.name.as_str())
                                .to_owned(),
                        ),
                );
            if let Some(category) = category {
                let id = category.id;
                let checked = follows(self.stores.guild_settings.channel(guild_id, id).flags);
                heading =
                    heading.child(
                        div()
                            .id(SharedString::from(format!("browser-category-{id}")))
                            .min_w(px(0.))
                            .flex()
                            .items_center()
                            .gap(px(24.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.browser_follow(guild_id, id, cx)
                            }))
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child("Follow Category"),
                            )
                            .child(super::create_channel::switch(checked)),
                    );
            }
            list = list.child(heading.when(first_heading, |heading| {
                heading.parity("browser-first-category")
            }));
            first_heading = false;
            let count = rows.len();
            for (index, channel) in rows.into_iter().enumerate() {
                list = list.child(
                    div()
                        .when(first_row, |row| row.parity("browser-first-channel"))
                        .child(self.browser_channel_row(
                            guild,
                            channel,
                            index == 0,
                            index + 1 == count,
                            window,
                            cx,
                        )),
                );
                first_row = false;
            }
        }
        let search = browser.search.clone();
        let focused = search.read(cx).focus_handle(cx).is_focused(window);
        let border = if focused { 2. } else { 1. };
        let page = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(color("--background-base-lower"))
            .child(
                div()
                    .parity_below("browser-header", 1.)
                    .h(px(50.))
                    .flex_none()
                    .px(px(20.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("channel_browse", 20., "--icon-muted")),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(label(guild)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .py(px(16.))
                    .px(px(8.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .parity_with_border("browser-search", border)
                            .ml(px(8.))
                            .mr(px(24.))
                            .mb(px(16.))
                            .h(px(40.))
                            .flex_none()
                            .border(px(border))
                            .border_color(color(if focused {
                                "--input-border-active"
                            } else {
                                "--input-border-default"
                            }))
                            .rule(crate::ui::motion::Rule::TextField)
                            .rounded(px(8.))
                            .bg(color("--input-background-default"))
                            .pl(px(5. - border))
                            .pr(px(if browser.query.is_empty() {
                                13. - border
                            } else {
                                5. - border
                            }))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .size(px(30.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("forum_search", 16., "--icon-strong")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .h(px(20.))
                                    .child(search.clone()),
                            )
                            .when(!browser.query.is_empty(), |row| {
                                row.child(
                                    div()
                                        .id("browser-clear-search")
                                        .size(px(30.))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            search.update(cx, |editor, _| editor.set_text(""));
                                            window.focus(&search.read(cx).focus_handle(cx), cx);
                                            if let Some(browser) = this.channel_browser.as_mut() {
                                                browser.query.clear();
                                            }
                                            cx.notify();
                                        }))
                                        .child(icon(
                                            "circle_x_small",
                                            16.,
                                            "--interactive-text-default",
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .child(
                                div()
                                    .id("channel-browser-scroll")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&browser.scroll)
                                    .pr(px(16.))
                                    .child(list),
                            )
                            .child(super::settings::auto_scrollbar(browser.scroll.clone())),
                    ),
            )
            .into_any_element();
        if self.channel.is_none() {
            return page;
        }
        div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .child(page)
            .child(self.browser_resize_handle(cx))
            .child(self.browser_preview(messages, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn following_preserves_other_flags_and_unfollowing_clears_favorite() {
        assert_eq!(follow_flags(128 | FAVORITE, true), 128 | FAVORITE | OPT_IN);
        assert_eq!(follow_flags(128 | FAVORITE | OPT_IN, false), 128);
    }

    #[test]
    fn legacy_following_starts_from_visible_channels_and_keeps_defaults() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({"guilds":[{"id":"100","name":"fixture","channels":[
                {"id":"1","type":0,"name":"one"}, {"id":"10","type":4,"name":"category"}
            ]}]}),
        );
        let guild = &stores.guilds[0];
        let mut settings = dc_state::guild_settings::GuildSettings::default();
        settings.channels.insert(
            2,
            dc_state::guild_settings::Override {
                flags: FAVORITE | 64,
                ..Default::default()
            },
        );
        assert!(legacy_selection(guild, &settings));
        let removed = follow_changes(
            &settings,
            true,
            &guild.channels[0],
            [1, 2, 3].into_iter(),
            &[],
        );
        assert_eq!(
            removed,
            BTreeMap::from([(2, FAVORITE | 64 | OPT_IN), (3, OPT_IN)])
        );
        let defaults = follow_changes(
            &settings,
            true,
            &guild.channels[0],
            [1, 2, 3].into_iter(),
            &[1],
        );
        assert_eq!(defaults.get(&1), Some(&OPT_IN));
        let category = follow_changes(
            &settings,
            true,
            &guild.channels[1],
            [1, 2, 3].into_iter(),
            &[],
        );
        assert_eq!(category.len(), 4);
        assert_eq!(category.get(&10), Some(&OPT_IN));
        settings.flags |= OPT_IN_GUILD;
        assert!(!legacy_selection(guild, &settings));
    }

    #[test]
    fn following_a_category_includes_its_children_without_rewriting_their_flags() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({"guilds":[{"id":"100","name":"fixture","channels":[
                {"id":"1","type":0,"name":"one","parent_id":"10"},
                {"id":"2","type":0,"name":"two","parent_id":"11"}
            ]}]}),
        );
        let mut settings = dc_state::guild_settings::GuildSettings::default();
        settings.channels.insert(
            10,
            dc_state::guild_settings::Override {
                flags: OPT_IN,
                ..Default::default()
            },
        );
        let channels = &stores.guilds[0].channels;
        assert!(follows_channel(&settings, &channels[0]));
        assert!(!follows_channel(&settings, &channels[1]));
        settings.channels.get_mut(&10).unwrap().flags = 0;
        assert!(!follows_channel(&settings, &channels[0]));
        settings.channels.insert(
            1,
            dc_state::guild_settings::Override {
                flags: OPT_IN,
                ..Default::default()
            },
        );
        assert!(follows_channel(&settings, &channels[0]));
    }
    #[test]
    fn name_search_is_a_subsequence_and_topic_search_is_contiguous() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({"guilds":[{"id":"100","name":"fixture","channels":[
                {"id":"1","type":0,"name":"Patch-Notes","topic":"Render fixes"}
            ]}]}),
        );
        let channel = &stores.guilds[0].channels[0];
        assert!(search_matches(channel, "ptnts"));
        assert!(search_matches(channel, "render"));
        assert!(!search_matches(channel, "rndr"));
        assert!(!search_matches(channel, "notespatch"));
    }
}
