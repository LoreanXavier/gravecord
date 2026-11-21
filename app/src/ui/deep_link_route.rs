//! Where a `discord://` link or a link to discord.com leads inside the app: the web router's places (`deep_link.rs`
//! lists the table, read from module 375389) opened through the same functions the interface uses. A link never
//! opens a browser for a place the app has; a place the app has not built yet is reported, not faked.

use super::server_settings::Page;
use super::{Id, Root};
use crate::deep_link::{self, Route};
use dc_state::ChannelKind;
use gpui::Context;

/// The key of the fetch that checks a linked channel this client has not loaded: `{guild}:{channel}:{message or 0}`.
pub(crate) const CHANNEL_CHECK_KEY: &str = "link-channel-check:";

/// What a link did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The place is open.
    Opened(&'static str),
    /// The link names something this account does not have (an unknown server or channel) or must not open by a link.
    Missing(&'static str),
    /// A place of the web router the app has no screen for yet.
    NotBuilt(&'static str),
}

impl Outcome {
    fn label(&self) -> String {
        match self {
            Outcome::Opened(place) => format!("opened {place}"),
            Outcome::Missing(why) => format!("missing: {why}"),
            Outcome::NotBuilt(place) => format!("not built: {place}"),
        }
    }
}

/// `d` in module 718446: lower case, `_`, spaces and `%20` as dashes.
fn dashed(text: &str) -> String {
    text.to_lowercase()
        .replace("%20", "-")
        .replace(['_', ' '], "-")
}

/// The settings page of the app that holds what `/settings/<section>[/<subsection>]` shows (`parseSettingsUrl`, module
/// 718446: the section and subsection joined are tried first, then the section), as the slug `open_settings_page`
/// takes. `None` is a section this app's settings have no page for.
pub(crate) fn settings_slug(section: &str, subsection: Option<&str>) -> Option<&'static str> {
    let section = dashed(section);
    if let Some(subsection) = subsection {
        match format!("{section}/{}", dashed(subsection)).as_str() {
            "account/account-standing" => return Some("account"),
            "content-and-social/connected-games" => return Some("connected-apps"),
            "notifications/emails" => return Some("notifications"),
            _ => {}
        }
    }
    Some(match section.as_str() {
        "account" | "account-standing" | "family-center" | "sessions" => "account",
        "content-and-social" => "messaging-permissions",
        "connected-games" | "authorized-apps" | "connections" => "connected-apps",
        "data-and-privacy" | "privacy-and-safety" => "data-privacy",
        "clips" => "clips",
        "premium" => "nitro",
        "guild-boosting" => "server-boost",
        "subscriptions" => "subscriptions",
        "inventory" => "gift-inventory",
        "billing" => "billing",
        "appearance" | "text" => "appearance",
        "accessibility" => "accessibility",
        "voice" => "voice-video",
        "notifications" | "emails" => "notifications",
        "system" | "windows" | "linux" => "system",
        "language" => "language-time",
        "advanced" | "developer-options" | "experiments" => "developer",
        "activity-privacy" => "activity-privacy",
        "registered-games" => "registered-games",
        "overlay" => "game-overlay",
        _ => return None,
    })
}

/// The server settings page a `/guilds/<id>/settings/<section>` link names.
fn guild_settings_page(section: Option<&str>) -> Page {
    match section.map(dashed).as_deref() {
        Some("roles") => Page::Roles,
        Some("emoji" | "emojis") => Page::Emoji,
        Some("stickers") => Page::Stickers,
        Some("soundboard") => Page::Soundboard,
        Some("members") => Page::Members,
        Some("invites") => Page::Invites,
        Some("bans") => Page::Bans,
        Some("audit-log") => Page::AuditLog,
        Some("integrations") => Page::Integrations,
        Some("automod" | "moderation") => Page::AutoMod,
        Some("community") => Page::Community,
        Some("onboarding") => Page::Onboarding,
        Some("analytics" | "insights") => Page::Insights,
        Some("tag") => Page::Tag,
        Some("access") => Page::Access,
        _ => Page::Profile,
    }
}

impl Root {
    /// A link of the app: true when it is one the app keeps (a `discord://` link always, a discord.com link when its
    /// path is a place of the app), false when the caller should treat it as an ordinary web link.
    pub(crate) fn open_deep_link(&mut self, url: &str, cx: &mut Context<Self>) -> bool {
        let Some(link) = deep_link::parse(url) else {
            return false;
        };
        let route = deep_link::route(&link);
        if deep_link::parse_protocol(url).is_none() && matches!(route, Route::Other(_)) {
            return false;
        }
        let outcome = self.open_routed(route, cx);
        tracing::info!(outcome = %outcome.label(), "link opened in the app");
        cx.notify();
        true
    }

    /// Capture harness: `link <url>` opens a link as a click on it would and says what happened.
    #[cfg(feature = "capture")]
    pub(crate) fn script_deep_link(&mut self, url: &str, cx: &mut Context<Self>) -> String {
        let Some(link) = deep_link::parse(url) else {
            return "link: not a link of the app".to_owned();
        };
        let outcome = self.open_routed(deep_link::route(&link), cx);
        cx.notify();
        format!(
            "link: {} (guild={:?} channel={:?})",
            outcome.label(),
            self.guild,
            self.channel
        )
    }

    fn channel_kind(&self, channel: Id) -> Option<ChannelKind> {
        self.stores
            .guilds
            .iter()
            .flat_map(|guild| &guild.channels)
            .find(|known| known.id == channel)
            .map(|known| known.kind)
    }

    /// The server that holds `channel`, a thread included.
    fn guild_holding(&self, channel: Id) -> Option<Id> {
        self.stores
            .guilds
            .iter()
            .find(|guild| {
                guild.channels.iter().any(|known| known.id == channel)
                    || guild.threads.iter().any(|known| known.id == channel)
            })
            .map(|guild| guild.id)
    }

    fn knows_guild(&self, guild: Id) -> bool {
        self.stores.guilds.iter().any(|known| known.id == guild)
    }

    /// The channel a server opens on when a link names only the server: the one last open there, else the first text
    /// channel of its list.
    fn default_channel_of(&self, guild: Id) -> Option<Id> {
        super::last_channel::channel_of(guild).or_else(|| {
            self.stores.guild(guild).and_then(|known| {
                known
                    .sidebar_rows()
                    .iter()
                    .find(|channel| channel.kind == ChannelKind::Text)
                    .map(|channel| channel.id)
            })
        })
    }

    /// Runs `open` with the window once the current update is over (some openers build text fields, which need it).
    fn with_window(
        &mut self,
        cx: &mut Context<Self>,
        open: impl FnOnce(&mut Root, &mut gpui::Window, &mut Context<Root>) + 'static,
    ) {
        let Some(handle) = cx.windows().into_iter().next() else {
            return;
        };
        let root = cx.entity();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                root.update(cx, |root, cx| open(root, window, cx));
            });
        });
    }

    /// `open_route`, then what module 362280 does when the place is not there: a server the account is not in, or a
    /// channel, thread or DM it cannot see, shows "You don't have access to this link." (module 763969, `kind`
    /// "channel" through `showInaccessibleLinkModal` of 9588).
    fn open_routed(&mut self, route: Route, cx: &mut Context<Self>) -> Outcome {
        // A channel of a known server that is not loaded may be a thread the list does not hold yet: like
        // `loadThread`, it is fetched first, and only a failed fetch shows the dialog.
        let unloaded = match &route {
            Route::Channel {
                guild: Some(guild),
                channel,
                message,
            } if self.knows_guild(*guild) && self.guild_holding(*channel).is_none() => {
                Some((*guild, *channel, *message))
            }
            Route::Thread {
                guild,
                thread,
                message,
                ..
            } if self.knows_guild(*guild) && self.guild_holding(*thread).is_none() => {
                Some((*guild, *thread, *message))
            }
            _ => None,
        };
        if let Some((guild, channel, message)) = unloaded {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: format!(
                    "{CHANNEL_CHECK_KEY}{guild}:{channel}:{}",
                    message.unwrap_or(0)
                ),
                route: format!("/channels/{channel}"),
            });
            return Outcome::Missing("channel (asked the server)");
        }
        let routed_place = matches!(
            route,
            Route::Guild(_)
                | Route::Channel { .. }
                | Route::Thread { .. }
                | Route::GuildPage { .. }
        );
        let outcome = self.open_route(route, cx);
        if routed_place && matches!(outcome, Outcome::Missing(_)) {
            self.show_inaccessible_link(false);
        }
        outcome
    }

    /// The answer about a linked channel that was not loaded: a thread of that server joins the list and the link
    /// opens it; anything else is a place the account cannot see.
    pub(super) fn link_channel_checked(
        &mut self,
        rest: &str,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let mut parts = rest.split(':').map(|part| part.parse::<Id>().ok());
        let (Some(Some(guild)), Some(Some(channel)), Some(Some(message))) =
            (parts.next(), parts.next(), parts.next())
        else {
            return;
        };
        let thread = body["fetch_failed"].as_bool() != Some(true)
            && body["guild_id"].as_str() == Some(guild.to_string().as_str())
            && matches!(body["type"].as_u64(), Some(10..=12));
        if thread {
            self.stores.dispatch("THREAD_CREATE", body);
            let message = (message != 0).then_some(message);
            match message {
                Some(message) => self.jump_to_message_id(channel, message, true, None),
                None => self.select(guild, Some(channel)),
            }
        } else {
            self.show_inaccessible_link(false);
        }
        cx.notify();
    }

    /// Module 763969: the title, the subtitle for a user or for a server or channel, and Okay.
    pub(super) fn show_inaccessible_link(&mut self, user: bool) {
        self.chat_extra.alert = Some(super::chat_notices::Alert::show(
            "inaccessible-link",
            dc_intl::tr("You don't have access to this link."),
            Some(
                dc_intl::tr(if user {
                    "This link is to a user you don't have access to."
                } else {
                    "This link is to a server or channel you don't have access to."
                })
                .to_owned(),
            ),
            None,
            None,
            super::chat_notices::Then::Close,
        ));
    }

    fn open_route(&mut self, route: Route, cx: &mut Context<Self>) -> Outcome {
        use super::discover::Place;
        match route {
            Route::Home => {
                self.select_home(None);
                Outcome::Opened("home")
            }
            Route::Guild(guild) => {
                if !self.knows_guild(guild) {
                    return Outcome::Missing("server");
                }
                let channel = self.default_channel_of(guild);
                self.select(guild, channel);
                Outcome::Opened("server")
            }
            Route::Channel {
                guild,
                channel,
                message,
            } => {
                if let Some(guild) = guild {
                    if !self.knows_guild(guild) || self.guild_holding(channel) != Some(guild) {
                        return Outcome::Missing("channel");
                    }
                    // A link never joins a voice channel: that is a click on the channel's own row.
                    if self.channel_kind(channel) == Some(ChannelKind::Voice) {
                        self.select(guild, self.default_channel_of(guild));
                        return Outcome::Opened("server (voice channels are joined from the list)");
                    }
                } else if !self
                    .stores
                    .private_channels
                    .iter()
                    .any(|dm| dm.id == channel)
                {
                    return Outcome::Missing("direct message");
                }
                match message {
                    Some(message) => self.jump_to_message_id(channel, message, true, None),
                    None => match guild {
                        Some(guild) => self.select(guild, Some(channel)),
                        None => self.select_home(Some(channel)),
                    },
                }
                Outcome::Opened(if message.is_some() {
                    "message"
                } else {
                    "channel"
                })
            }
            Route::Thread {
                guild,
                channel: _,
                thread,
                message,
            } => {
                if self.guild_holding(thread) != Some(guild) {
                    return Outcome::Missing("thread");
                }
                match message {
                    Some(message) => self.jump_to_message_id(thread, message, true, None),
                    None => self.select(guild, Some(thread)),
                }
                Outcome::Opened("thread")
            }
            Route::GuildPage { guild, name } => {
                if !self.knows_guild(guild) {
                    return Outcome::Missing("server");
                }
                let channel = self.default_channel_of(guild);
                self.select(guild, channel);
                match name.as_str() {
                    "@home" => {
                        self.open_guild_guide(cx);
                        Outcome::Opened("server guide")
                    }
                    "channel-browser" => {
                        self.open_channel_browser(cx);
                        Outcome::Opened("channel browser")
                    }
                    "boosts" | "shop" | "game-shop" => Outcome::NotBuilt("server page of the list"),
                    _ => Outcome::Opened("server"),
                }
            }
            Route::User(user) => {
                self.with_window(cx, move |root, window, cx| {
                    root.open_full_profile(user, window, cx);
                });
                Outcome::Opened("profile")
            }
            Route::Settings {
                section,
                subsection,
            } => {
                match settings_slug(&section, subsection.as_deref()) {
                    Some(slug) => self.open_settings_page(slug, cx),
                    None if dashed(&section) == "profile-customization" => {
                        self.open_profile_editor(cx);
                    }
                    None => self.open_settings(cx),
                }
                Outcome::Opened("settings")
            }
            Route::GuildSettings { guild, section } => {
                if !self.knows_guild(guild) {
                    return Outcome::Missing("server");
                }
                self.open_server_settings_at(guild, guild_settings_page(section.as_deref()), cx);
                Outcome::Opened("server settings")
            }
            Route::DiscoverServers => {
                self.open_discover(Place::Servers, cx);
                Outcome::Opened("discovery")
            }
            Route::DiscoverApps => {
                self.open_discover(Place::Apps, cx);
                Outcome::Opened("app directory")
            }
            Route::DiscoverApp(app) => {
                self.open_discover(Place::App(app), cx);
                Outcome::Opened("app page")
            }
            Route::Store => {
                self.open_nitro(cx);
                Outcome::Opened("nitro")
            }
            Route::Shop => {
                self.open_shop(cx);
                Outcome::Opened("shop")
            }
            Route::Quests(_) => {
                self.open_quests(cx);
                Outcome::Opened("quests")
            }
            Route::Event { guild, .. } => {
                if !self.knows_guild(guild) {
                    return Outcome::Missing("server");
                }
                let channel = self.default_channel_of(guild);
                self.select(guild, channel);
                self.open_guild_events(cx);
                Outcome::Opened("events of the server")
            }
            Route::FamilyCenter => {
                self.open_settings_page("account", cx);
                Outcome::Opened("settings")
            }
            Route::CreateGuild => {
                self.with_window(cx, |root, window, cx| root.open_add_server(window, cx));
                Outcome::Opened("add a server")
            }
            Route::BoostMarketing(_) => Outcome::NotBuilt("server boost page"),
            Route::Invite(code) => {
                self.open_accept_invite_modal(&code, cx);
                Outcome::Opened("invite modal")
            }
            Route::Gift(_) => Outcome::NotBuilt("gift modal"),
            Route::Template(_) => Outcome::NotBuilt("server template modal"),
            Route::PickGuildSettings { section } => {
                let feature = if matches!(
                    section.as_deref(),
                    Some("role-subscriptions" | "subscriptions")
                ) {
                    super::pick_guild_settings::PickGuildFeature::ServerSubscriptionsOnboarding
                } else {
                    super::pick_guild_settings::PickGuildFeature::Default
                };
                self.open_pick_guild_settings(section, feature, cx);
                Outcome::Opened("server settings picker")
            }
            Route::Library => {
                self.open_library(super::library::Tab::Games, cx);
                Outcome::Opened("library")
            }
            Route::MessageRequests => {
                self.open_message_requests(cx);
                Outcome::Opened("message requests")
            }
            Route::Other(_) => Outcome::Missing("unknown place"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_sections_map_to_the_pages_that_hold_them() {
        assert_eq!(settings_slug("voice", None), Some("voice-video"));
        assert_eq!(
            settings_slug("privacy-and-safety", None),
            Some("data-privacy")
        );
        assert_eq!(settings_slug("guild-boosting", None), Some("server-boost"));
        assert_eq!(
            settings_slug("account", Some("account-standing")),
            Some("account")
        );
        assert_eq!(
            settings_slug("content-and-social", Some("connected-games")),
            Some("connected-apps")
        );
        assert_eq!(
            settings_slug("content_and_social", None),
            Some("messaging-permissions")
        );
        assert_eq!(settings_slug("keybinds", None), None);
        assert_eq!(settings_slug("nowhere", None), None);
    }

    #[test]
    fn server_settings_sections_map_to_their_pages() {
        assert!(matches!(guild_settings_page(Some("roles")), Page::Roles));
        assert!(matches!(
            guild_settings_page(Some("audit_log")),
            Page::AuditLog
        ));
        assert!(matches!(guild_settings_page(None), Page::Profile));
        assert!(matches!(
            guild_settings_page(Some("overview")),
            Page::Profile
        ));
    }
}
