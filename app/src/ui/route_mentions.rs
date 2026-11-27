//! Mentions drawn from links and ids rather than from `<#id>`:
//!
//! - `<id:home|browse|customize|guide|linked-roles[:role]>` (the staticRouteLink rule of module 605275): a chip with
//!   the page's icon (module 232042: SignPost, ChannelListMagnifyingGlass or Link) and its name (`Q` of module 78390),
//!   then, for a linked role the account knows, a chevron (module 37632) and the role's name (`f`). A click opens the
//!   page (`i` of module 392605).
//! - links to a channel, a message or a forum post (`channelOrMessageUrl` `V` and `mediaPostLink` `H`): a channel
//!   mention (`F` of module 605275, drawn by module 556300) whose parts depend on where the link leads and where it is
//!   read: the server's icon and name (module 906754) or the channel, a chevron, then the channel or a chat bubble for
//!   a message. A click follows the link.
//!
//! The markdown gives each mention as three spans (`dc_markdown::Part`); each becomes one piece of the inline text,
//! joined into one box (`BoxStyle::join_*`) that lights up as one (`BoxStyle::group`).
use super::channel_mention::Facts;
use super::{Root, font, hsla, kerning};
use crate::inline::{IconArt, Piece};
use dc_markdown::{Kind, Part, Span};
use dc_state::Id;
use gpui::{FontStyle, FontWeight, px};

/// One part of a mention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Chunk {
    /// `P`: a server's icon (1em, module 906754) and its name.
    Guild { id: Id, name: String },
    /// `M`: a channel's icon (module 232042) and its name, in italics for the unknown one (`k`).
    Channel {
        icon: &'static str,
        text: String,
        italic: bool,
    },
    /// `U`: the chat bubble alone (a message, or a forum post).
    Message,
    /// Text with no icon: the item of a server navigation chip.
    Text(String),
}

/// What a link mention draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Look {
    /// `#name` as plain text (`isMentionable` false).
    Plain(String),
    /// A plain link (`G`): a channel of a server this client does not have.
    Link,
    /// The mention: `inContent` (if any), then `content`.
    Mention { before: Option<Chunk>, main: Chunk },
}

/// `EJ(name, 32)` of module 23339: longer than 32 characters, cut to 31, trailing spaces and dots dropped, then `…`.
pub(super) fn truncate_name(name: &str) -> String {
    let count = name.chars().count();
    if count <= 32 {
        return name.to_owned();
    }
    let cut: String = name.chars().take(31).collect();
    format!(
        "{}\u{2026}",
        cut.trim_end_matches(|c: char| c.is_whitespace() || c == '.')
    )
}

fn channel_chunk(icon: &'static str, name: &str) -> Chunk {
    Chunk::Channel {
        icon,
        text: truncate_name(name),
        italic: false,
    }
}

/// `F` of module 605275: the mention of a channel the store holds. `message` is whether the link names a message,
/// `current` the server of the channel being read, `guild_name` the name of a server the session holds.
pub(super) fn known_look(
    facts: &Facts,
    message: bool,
    current: Option<Id>,
    guild_name: &dyn Fn(Id) -> Option<String>,
) -> Look {
    if !facts.can_view {
        return Look::Mention {
            before: None,
            main: Chunk::Channel {
                icon: "icons/channel_lock.svg",
                text: dc_intl::t("/YzI63").to_owned(),
                italic: false,
            },
        };
    }
    if !facts.mentionable {
        return Look::Plain(format!("#{}", facts.name));
    }
    let guild = facts
        .guild
        .and_then(|guild| guild_name(guild).map(|name| (guild, name)));
    let Some((guild, name)) = guild else {
        if facts.dm {
            return Look::Mention {
                before: Some(channel_chunk(facts.icon, &facts.name)),
                main: Chunk::Message,
            };
        }
        return Look::Link;
    };
    let channel = channel_chunk(facts.icon, &facts.name);
    let server = Chunk::Guild {
        id: guild,
        name: truncate_name(&name),
    };
    let same = current == Some(guild);
    match (same, message) {
        (true, true) => match (&facts.parent, facts.forum_post) {
            (Some((parent, icon)), true) => Look::Mention {
                before: Some(channel_chunk(icon, parent)),
                main: channel,
            },
            _ => Look::Mention {
                before: Some(channel),
                main: Chunk::Message,
            },
        },
        (true, false) => Look::Mention {
            before: None,
            main: channel,
        },
        (false, true) => Look::Mention {
            before: Some(server),
            main: if facts.forum_post {
                channel
            } else {
                Chunk::Message
            },
        },
        (false, false) => Look::Mention {
            before: Some(server),
            main: channel,
        },
    }
}

/// `k`: a channel the store does not hold. The server of the link shows first when the session has it and it is not
/// the one being read; the channel reads "unknown" in italics with the text icon.
pub(super) fn unknown_look(
    guild: Option<Id>,
    current: Option<Id>,
    guild_name: &dyn Fn(Id) -> Option<String>,
) -> Look {
    let before = guild
        .filter(|guild| Some(*guild) != current)
        .and_then(|guild| {
            guild_name(guild).map(|name| Chunk::Guild {
                id: guild,
                name: truncate_name(&name),
            })
        });
    Look::Mention {
        before,
        main: Chunk::Channel {
            icon: "icons/text_channel.svg",
            text: dc_intl::t("zLZPmk").to_lowercase(),
            italic: true,
        },
    }
}

/// `Q` of module 78390: the name of a navigation id.
pub(super) fn static_route_label(id: &str) -> &'static str {
    match id {
        "browse" => dc_intl::t("et6wav"),
        "customize" => dc_intl::t("h9mGOP"),
        "linked-roles" => dc_intl::t("ghtnss"),
        // "home" and "guide".
        _ => dc_intl::t("VbpLyU"),
    }
}

/// The icon module 232042 gives a navigation id.
pub(super) fn static_route_icon(id: &str) -> &'static str {
    match id {
        "browse" | "customize" => "icons/channel_browse.svg",
        "linked-roles" => "icons/discover_link.svg",
        _ => "icons/ob_signpost.svg",
    }
}

/// Where a navigation chip leads (`i` of module 392605): Server Guide, Browse Channels and Channels & Roles need a
/// Community server; Linked Roles opens the role connections dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StaticTarget {
    Guide,
    Browse,
    Customize,
    LinkedRoles,
    Nothing,
}

pub(super) fn static_target(id: &str, guild_known: bool, community: bool) -> StaticTarget {
    if !guild_known {
        return StaticTarget::Nothing;
    }
    match id {
        "linked-roles" => StaticTarget::LinkedRoles,
        _ if !community => StaticTarget::Nothing,
        "browse" => StaticTarget::Browse,
        "customize" => StaticTarget::Customize,
        _ => StaticTarget::Guide,
    }
}

/// The hover group of a mention made of parts: the same for the three spans of one link.
fn group_of(span: &Span) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    span.text.hash(&mut hasher);
    match &span.kind {
        Kind::StaticRoute { id, item, .. } => (id, item).hash(&mut hasher),
        Kind::ChannelUrl {
            guild,
            channel,
            message,
            thread,
            ..
        } => (guild, channel, message, thread).hash(&mut hasher),
        _ => {}
    }
    hasher.finish()
}

/// What the chevron between the parts takes: `.icon_FDzO1k` is .5em square with 4px before and 2px after.
fn chevron_room(size: f32) -> (f32, f32, f32) {
    let icon = size / 2.;
    (icon, 4. + icon + 2., 4.)
}

impl Root {
    /// The look of a channel or message link.
    pub(super) fn url_look(
        &self,
        guild: Option<Id>,
        channel: Id,
        message: Option<Id>,
        thread: Option<Id>,
    ) -> Look {
        let current = self.guild;
        let guild_name = |id: Id| self.stores.guild(id).map(|guild| guild.name.clone());
        // `H` reads the thread first, then the channel.
        let facts = thread
            .and_then(|thread| self.link_channel_facts(thread))
            .or_else(|| self.link_channel_facts(channel));
        match facts {
            Some(facts) => known_look(&facts, message.is_some(), current, &guild_name),
            None => unknown_look(guild, current, &guild_name),
        }
    }

    /// The item of a navigation chip: a linked role's name (`f` of module 78390: a role of the server whose
    /// `tags.guild_connections` is set).
    fn static_route_item(&self, id: &str, item: Option<u64>) -> Option<String> {
        if id != "linked-roles" {
            return None;
        }
        let guild = self.current_guild()?;
        let role = item?;
        guild
            .role_info
            .get(&role)
            .filter(|info| info.guild_connection)?;
        guild.roles.get(&role).cloned()
    }

    /// The piece of a span that is part of a link or navigation mention; none for other spans.
    pub(super) fn route_mention_piece(&self, span: &Span, size: f32) -> Option<Piece> {
        let (part, chunks): (Part, Option<(Option<Chunk>, Chunk)>) = match &span.kind {
            Kind::StaticRoute { id, item, part } => {
                let main = Chunk::Channel {
                    icon: static_route_icon(id),
                    text: static_route_label(id).to_owned(),
                    italic: false,
                };
                // With an item the page's name comes first and the item after the chevron.
                let item = self.static_route_item(id, *item).map(Chunk::Text);
                (
                    *part,
                    Some(match item {
                        Some(item) => (Some(main), item),
                        None => (None, main),
                    }),
                )
            }
            Kind::ChannelUrl {
                guild,
                channel,
                message,
                thread,
                part,
            } => match self.url_look(*guild, *channel, *message, *thread) {
                Look::Mention { before, main } => (*part, Some((before, main))),
                Look::Plain(text) => {
                    return Some(self.plain_part(span, *part, Some(text), false, size));
                }
                Look::Link => {
                    return Some(self.plain_part(span, *part, None, true, size));
                }
            },
            _ => return None,
        };
        let (before, main) = chunks?;
        let group = group_of(span);
        let empty = || self.plain_part(span, Part::Before, Some(String::new()), false, size);
        let piece = match part {
            Part::Before => match &before {
                Some(chunk) => self.chunk_piece(chunk, size, group, (false, true)),
                None => empty(),
            },
            Part::Chevron => {
                if before.is_none() {
                    empty()
                } else {
                    let (icon, room, inset) = chevron_room(size);
                    let mut piece =
                        self.chunk_piece(&Chunk::Text(String::new()), size, group, (true, true));
                    if let Some(boxed) = piece.boxed.as_mut() {
                        boxed.icon = Some((
                            "icons/bot_chevron_small_right.svg".into(),
                            px(icon),
                            px(room),
                        ));
                        boxed.icon_inset = px(inset);
                    }
                    piece
                }
            }
            Part::Main => self.chunk_piece(&main, size, group, (before.is_some(), false)),
        };
        Some(piece)
    }

    /// A part drawn as plain text or as a link, or nothing (only the main part carries the text).
    fn plain_part(
        &self,
        span: &Span,
        part: Part,
        text: Option<String>,
        link: bool,
        size: f32,
    ) -> Piece {
        let shown = if part == Part::Main {
            text.unwrap_or_else(|| span.text.clone())
        } else {
            String::new()
        };
        let mut run_font = font(super::SANS);
        run_font.features = kerning();
        let link = link && part == Part::Main;
        Piece {
            text: shown,
            font: run_font,
            size: px(size),
            color: if link {
                hsla("--text-link")
            } else {
                hsla("--text-default")
            },
            underline: link && self.underline_links(),
            strike: false,
            boxed: None,
            image: None,
            hover: None,
            hover_underline: link,
            gradient: None,
        }
    }

    /// One chunk as a piece of the mention box: 2px of padding at the outer sides, 3px corners
    /// (`.wrapper_NDaLCQ`), `--mention-background` behind `--mention-foreground` at the medium weight; the brand
    /// colour behind white while the pointer is over the mention (`interactive`).
    fn chunk_piece(&self, chunk: &Chunk, size: f32, group: u64, joins: (bool, bool)) -> Piece {
        let mut run_font = font(super::SANS);
        run_font.features = kerning();
        run_font.weight = FontWeight(500.);
        let mut boxed = super::pill(2., 3., "--mention-background");
        boxed.join_left = joins.0;
        boxed.join_right = joins.1;
        boxed.group = Some(group);
        let text = match chunk {
            Chunk::Guild { id, name } => {
                boxed.icon = Some(("icons/text_channel.svg".into(), px(size), px(size + 3.)));
                let guild = self.stores.guild(*id);
                boxed.icon_art = Some(match self.images.get(&format!("guild:{id}")) {
                    // `imageIcon_EyldYD`: 1em; the 40px icon's 12px corners make it round.
                    Some(image) => IconArt::Image(image.clone(), px(size / 2.)),
                    // `textIcon_EyldYD`: 1.05rem with the acronym at .5rem on `--background-base-lower`.
                    None => {
                        boxed.icon =
                            Some(("icons/text_channel.svg".into(), px(16.8), px(16.8 + 3.)));
                        IconArt::Text {
                            text: guild
                                .map(|guild| guild.acronym())
                                .unwrap_or_default()
                                .into(),
                            size: px(8.),
                            color: hsla("--text-default"),
                            background: hsla("--background-base-lower"),
                            radius: px(8.4),
                        }
                    }
                });
                name.clone()
            }
            Chunk::Channel { icon, text, italic } => {
                // `icon__RYHZ5`: 1em with 4px after it.
                boxed.icon = Some(((*icon).into(), px(size), px(size + 4.)));
                if *italic {
                    run_font.style = FontStyle::Italic;
                }
                text.clone()
            }
            Chunk::Message => {
                boxed.icon = Some(("icons/chat_bubble.svg".into(), px(size), px(size + 4.)));
                String::new()
            }
            Chunk::Text(text) => text.clone(),
        };
        Piece {
            text,
            font: run_font,
            size: px(size),
            color: hsla("--mention-foreground"),
            underline: false,
            strike: false,
            boxed: Some(boxed),
            image: None,
            hover: Some((hsla("--brand-500"), hsla("--white"))),
            hover_underline: false,
            gradient: None,
        }
    }

    /// The text a part stands for when the message is copied: the whole mention once.
    pub(super) fn route_mention_label(&self, span: &Span) -> Option<String> {
        match &span.kind {
            Kind::StaticRoute { id, item, part } => Some(match part {
                Part::Main => {
                    let mut label = static_route_label(id).to_owned();
                    if let Some(item) = self.static_route_item(id, *item) {
                        label.push_str(" \u{203A} ");
                        label.push_str(&item);
                    }
                    label
                }
                _ => String::new(),
            }),
            Kind::ChannelUrl { part, .. } => Some(match part {
                Part::Main => span.text.clone(),
                _ => String::new(),
            }),
            _ => None,
        }
    }

    /// `i` of module 392605: a click on a navigation chip.
    pub(super) fn open_static_route(&mut self, id: &str, cx: &mut gpui::Context<Self>) {
        let guild = self.current_guild();
        let community = guild.is_some_and(|guild| guild.features.iter().any(|f| f == "COMMUNITY"));
        match static_target(id, guild.is_some(), community) {
            StaticTarget::Guide => self.open_guild_guide(cx),
            StaticTarget::Browse => self.open_channel_browser(cx),
            // `CUSTOMIZE_COMMUNITY` is the Channels & Roles page; here the channel browser stands for it (it is
            // titled Channels & Roles when the server has onboarding prompts).
            StaticTarget::Customize => self.open_channel_browser(cx),
            // `GUILD_ROLE_CONNECTIONS_MODAL_SHOW`: this client has no role connections dialog yet.
            StaticTarget::LinkedRoles => {
                tracing::info!("linked roles chip: the role connections dialog is not built");
            }
            StaticTarget::Nothing => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(guild: Option<Id>) -> Facts {
        Facts {
            guild,
            name: "general".to_owned(),
            icon: "icons/text_channel.svg",
            can_view: true,
            mentionable: true,
            dm: guild.is_none(),
            forum_post: false,
            parent: None,
        }
    }

    fn names(id: Id) -> Option<String> {
        (id == 1 || id == 2).then(|| format!("server {id}"))
    }

    fn channel(name: &str) -> Chunk {
        Chunk::Channel {
            icon: "icons/text_channel.svg",
            text: name.to_owned(),
            italic: false,
        }
    }

    #[test]
    fn a_link_in_its_own_server_shows_the_channel() {
        assert_eq!(
            known_look(&facts(Some(1)), false, Some(1), &names),
            Look::Mention {
                before: None,
                main: channel("general")
            }
        );
        // A message: the channel, then the bubble.
        assert_eq!(
            known_look(&facts(Some(1)), true, Some(1), &names),
            Look::Mention {
                before: Some(channel("general")),
                main: Chunk::Message
            }
        );
    }

    #[test]
    fn a_link_into_another_server_starts_with_that_server() {
        let server = Chunk::Guild {
            id: 2,
            name: "server 2".to_owned(),
        };
        assert_eq!(
            known_look(&facts(Some(2)), false, Some(1), &names),
            Look::Mention {
                before: Some(server.clone()),
                main: channel("general")
            }
        );
        assert_eq!(
            known_look(&facts(Some(2)), true, None, &names),
            Look::Mention {
                before: Some(server),
                main: Chunk::Message
            }
        );
    }

    #[test]
    fn forum_posts_dms_and_hidden_channels() {
        let mut post = facts(Some(1));
        post.forum_post = true;
        post.parent = Some(("forum".to_owned(), "icons/forum_channel.svg"));
        assert_eq!(
            known_look(&post, true, Some(1), &names),
            Look::Mention {
                before: Some(Chunk::Channel {
                    icon: "icons/forum_channel.svg",
                    text: "forum".to_owned(),
                    italic: false
                }),
                main: channel("general")
            }
        );
        // A direct message: the conversation, then the bubble.
        assert_eq!(
            known_look(&facts(None), false, Some(1), &names),
            Look::Mention {
                before: Some(channel("general")),
                main: Chunk::Message
            }
        );
        let mut hidden = facts(Some(1));
        hidden.can_view = false;
        assert!(matches!(
            known_look(&hidden, false, Some(1), &names),
            Look::Mention {
                before: None,
                main: Chunk::Channel {
                    icon: "icons/channel_lock.svg",
                    ..
                }
            }
        ));
        let mut category = facts(Some(1));
        category.mentionable = false;
        assert_eq!(
            known_look(&category, false, Some(1), &names),
            Look::Plain("#general".to_owned())
        );
        // A server the session does not have: the link stays a link.
        assert_eq!(
            known_look(&facts(Some(9)), false, Some(1), &names),
            Look::Link
        );
    }

    #[test]
    fn an_unknown_channel_reads_unknown_after_its_server() {
        assert!(matches!(
            unknown_look(Some(2), Some(1), &names),
            Look::Mention {
                before: Some(Chunk::Guild { id: 2, .. }),
                main: Chunk::Channel { italic: true, .. }
            }
        ));
        assert!(matches!(
            unknown_look(Some(1), Some(1), &names),
            Look::Mention { before: None, .. }
        ));
        assert!(matches!(
            unknown_look(None, Some(1), &names),
            Look::Mention { before: None, .. }
        ));
    }

    #[test]
    fn names_are_cut_at_thirty_two() {
        assert_eq!(truncate_name("short"), "short");
        let long = "a".repeat(30) + " . bcd";
        assert_eq!(truncate_name(&long), format!("{}\u{2026}", "a".repeat(30)));
    }

    #[test]
    fn navigation_chips_name_and_open_their_pages() {
        assert_eq!(static_route_label("guide"), "Server Guide");
        assert_eq!(static_route_label("home"), "Server Guide");
        assert_eq!(static_route_label("browse"), "Browse Channels");
        assert_eq!(static_route_label("customize"), "Channels & Roles");
        assert_eq!(static_route_label("linked-roles"), "Linked Roles");
        assert_eq!(static_target("browse", true, true), StaticTarget::Browse);
        assert_eq!(static_target("home", true, true), StaticTarget::Guide);
        assert_eq!(static_target("guide", true, false), StaticTarget::Nothing);
        assert_eq!(
            static_target("linked-roles", true, false),
            StaticTarget::LinkedRoles
        );
        assert_eq!(
            static_target("customize", false, true),
            StaticTarget::Nothing
        );
    }
}
