//! Following a link, ported from `Y` in module 889558: a link whose text is its own address opens at once (unless it
//! sits in a DM with someone who is not a friend), as does any link to a trusted domain; anything else asks first with
//! the "Leaving Discord" modal (module 885293, measured in `design/dom/modals/leaving-discord-darker-cozy.json`), which
//! can trust the domain (or the protocol) from then on (`MASKED_LINK_ADD_TRUSTED_DOMAIN`, kept by `MaskedLinkStore`).

use super::{Root, color, icon};
use crate::parity::Parity;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::{Value, json};

/// `MaskedLinkStore`'s storage key.
const STORE_KEY: &str = "MaskedLinkStore";

/// The open "Leaving Discord" modal.
pub(crate) struct Leaving {
    pub url: String,
    /// Not http(s): the modal trusts the protocol instead of the domain.
    pub protocol: bool,
    /// The "Trust … from now on" checkbox.
    pub trust: bool,
}

/// The parts the modal draws: `protocol` + `//`, the host in semibold, the rest muted (`m.u` in module 60888).
pub(crate) struct Parts {
    pub protocol: String,
    pub authority: String,
    pub host: String,
    pub rest: String,
}

pub(crate) fn split(url: &str) -> Parts {
    let (protocol, after) = match url.find(':') {
        Some(index) => (url[..=index].to_owned(), &url[index + 1..]),
        None => (String::new(), url),
    };
    let (authority, after) = match after.strip_prefix("//") {
        Some(rest) => ("//".to_owned(), rest),
        None => (String::new(), after),
    };
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    Parts {
        protocol,
        authority,
        host: after[..end].to_owned(),
        rest: after[end..].to_owned(),
    }
}

/// `(guild, channel, message)` of a link to a message: `https://discord.com/channels/<guild or @me>/<channel>/<message>`
/// on discord.com, ptb.discord.com or canary.discord.com.
pub(crate) fn message_link(url: &str) -> Option<(Option<u64>, u64, u64)> {
    let parts = split(url);
    if !matches!(protocol_of(url).as_str(), "http:" | "https:")
        || !matches!(
            hostname(url).as_str(),
            "discord.com" | "ptb.discord.com" | "canary.discord.com" | "discordapp.com"
        )
    {
        return None;
    }
    let path = parts.rest.split(['?', '#']).next().unwrap_or_default();
    let mut segments = path.trim_matches('/').split('/');
    if segments.next()? != "channels" {
        return None;
    }
    let guild = segments.next()?;
    let channel = segments.next()?.parse().ok()?;
    let message = segments.next()?.parse().ok()?;
    if segments.next().is_some() {
        return None;
    }
    let guild = if guild == "@me" {
        None
    } else {
        Some(guild.parse().ok()?)
    };
    Some((guild, channel, message))
}

/// `b.E`: the hostname, lowercased, without credentials or port.
pub(crate) fn hostname(url: &str) -> String {
    let host = split(url).host;
    let host = host.rsplit('@').next().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default();
    host.to_ascii_lowercase()
}

/// `b.J`: the protocol with its colon, lowercased.
fn protocol_of(url: &str) -> String {
    split(url).protocol.to_ascii_lowercase()
}

fn stored(field: &str) -> Vec<String> {
    let Some(value) = crate::storage::get(STORE_KEY) else {
        return Vec::new();
    };
    // Older clients stored a bare array of trusted domains.
    let list = if field == "trustedDomains" && value.is_array() {
        &value
    } else {
        &value[field]
    };
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn remember(field: &str, entry: String) {
    let mut domains = stored("trustedDomains");
    let mut protocols = stored("trustedProtocols");
    let list = if field == "trustedDomains" {
        &mut domains
    } else {
        &mut protocols
    };
    if !list.contains(&entry) {
        list.push(entry);
    }
    crate::storage::set(
        STORE_KEY,
        json!({ "trustedDomains": domains, "trustedProtocols": protocols }),
    );
}

/// `G` in module 889558: Discord's own hosts and those the user trusted.
pub(crate) fn is_trusted_domain(url: &str) -> bool {
    let host = hostname(url);
    // GLOBAL_ENV hosts from the pinned index.html, the Spotify hosts (`v.UN`), and `isDiscordHostname`.
    const HOSTS: [&str; 7] = [
        "discord.gg",
        "discord.gift",
        "discord.new",
        "media.discordapp.net",
        "discord.com",
        "open.spotify.com",
        "www.spotify.com",
    ];
    let discord = ["discordapp.com", "discord.com", "discordmerch.com"]
        .iter()
        .any(|root| host == *root || host.ends_with(&format!(".{root}")));
    HOSTS.contains(&host.as_str()) || discord || stored("trustedDomains").contains(&host)
}

fn is_trusted_protocol(url: &str) -> bool {
    stored("trustedProtocols").contains(&protocol_of(url))
}

impl Root {
    /// A click on a link in rendered markdown. `label` is the link's visible text.
    pub(super) fn follow_link(&mut self, url: &str, label: &str, cx: &mut Context<Self>) {
        // A link to a message of Discord (`/channels/<guild>/<channel>/<message>`) is a route of this client: it
        // opens the channel and jumps to the message.
        if let Some((guild, channel, message)) = message_link(url) {
            let known = self.stores.guilds.iter().any(|guild| {
                guild.channels.iter().any(|known| known.id == channel)
                    || guild.threads.iter().any(|known| known.id == channel)
            }) || (guild.is_none()
                && self
                    .stores
                    .private_channels
                    .iter()
                    .any(|dm| dm.id == channel));
            if known {
                self.jump_to_message_id(channel, message, true, None);
                cx.notify();
                return;
            }
        }
        // `W`: the text being the address trusts it, except in a DM with someone who is not a friend.
        let friendly = self
            .current_dm()
            .and_then(|channel| self.stores.dm_recipient(channel))
            .is_none_or(|user| self.stores.relationships.get(&user.id) == Some(&1));
        let trusted = friendly && label == url;
        let protocol = !matches!(protocol_of(url).as_str(), "http:" | "https:");
        let open = if protocol {
            is_trusted_protocol(url)
        } else {
            trusted || is_trusted_domain(url)
        };
        if open {
            cx.open_url(url);
        } else {
            self.leaving = Some(Leaving {
                url: url.to_owned(),
                protocol,
                trust: false,
            });
        }
        cx.notify();
    }

    fn confirm_leaving(&mut self, cx: &mut Context<Self>) {
        let Some(leaving) = self.leaving.take() else {
            return;
        };
        if leaving.trust {
            if leaving.protocol {
                remember("trustedProtocols", protocol_of(&leaving.url));
            } else {
                remember("trustedDomains", hostname(&leaving.url));
            }
        }
        cx.open_url(&leaving.url);
        cx.notify();
    }

    pub(super) fn leaving_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let leaving = self.leaving.as_ref()?;
        let parts = split(&leaving.url);
        let (lead, lead_weight, host_color, host_weight) = if leaving.protocol {
            ("--text-default", 600., "--text-muted", 400.)
        } else {
            ("--text-muted", 400., "--text-default", 600.)
        };
        let checked = leaving.trust;
        let trust_target = if leaving.protocol {
            parts.protocol.trim_end_matches(':').to_owned()
        } else {
            parts.host.clone()
        };
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .flex_1()
                .h(px(40.))
                .rounded(px(8.))
                .border_1()
                .border_color(if primary {
                    color("--control-primary-border-default")
                } else {
                    color("--border-muted")
                })
                .bg(if primary {
                    color("--control-primary-background-default")
                } else {
                    color("--control-secondary-background-default")
                })
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(if primary {
                    color("--white")
                } else {
                    color("--control-secondary-text-default")
                })
                .child(label)
        };
        let modal = div()
            .parity_with_border("leaving-modal", 1.)
            .id("leaving-modal")
            .occlude()
            .w(px(480.))
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
            .child(
                div()
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .h(px(24.))
                            .flex()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child("Leaving Discord"),
                            )
                            .child(
                                div()
                                    .id("leaving-close")
                                    .mt(px(-16.))
                                    .mr(px(-16.))
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        "modal_close",
                                        20.,
                                        "--control-icon-only-icon-default",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.leaving = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .flex()
                            .when(leaving.protocol, |this| {
                                // `aCYv1z`: the tail is bold.
                                this.child("This link is trying to open\u{a0}").child(
                                    div()
                                        .font_weight(FontWeight(600.))
                                        .child("an application on your device"),
                                )
                            })
                            .when(!leaving.protocol, |this| {
                                this.child("This link is taking you to the following website")
                            }),
                    ),
            )
            .child(
                div()
                    .pt(px(16.))
                    .pl(px(24.))
                    .pr(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .p(px(16.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_wrap()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .child(
                                div()
                                    .font_weight(FontWeight(lead_weight))
                                    .text_color(color(lead))
                                    .child(format!("{}{}", parts.protocol, parts.authority)),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight(host_weight))
                                    .text_color(color(host_color))
                                    .child(parts.host.clone()),
                            )
                            .child(div().text_color(color("--text-muted")).child(parts.rest)),
                    )
                    .child(
                        div()
                            .id("leaving-trust")
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(leaving) = this.leaving.as_mut() {
                                    leaving.trust = !leaving.trust;
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .size(px(20.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(if checked {
                                        color("--control-primary-background-default")
                                    } else {
                                        color("--checkbox-border-default")
                                    })
                                    .bg(if checked {
                                        color("--control-primary-background-default")
                                    } else {
                                        color("--checkbox-background-default")
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(checked, |this| {
                                        this.child(icon("theme_check", 18., "--white"))
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-muted"))
                                    .child("Trust\u{a0}")
                                    .child(div().font_weight(FontWeight(600.)).child(trust_target))
                                    .child("\u{a0}links from now on"),
                            ),
                    ),
            )
            .child(
                div()
                    .mt(px(8.))
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .flex()
                    .gap(px(8.))
                    .child(
                        button("leaving-back", "Go Back", false).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.leaving = None;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        button(
                            "leaving-visit",
                            if leaving.protocol {
                                "Open Link"
                            } else {
                                "Visit Site"
                            },
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.confirm_leaving(cx))),
                    ),
            );
        Some(
            deferred(
                div()
                    .id("leaving-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.leaving = None;
                        cx.notify();
                    }))
                    .child(modal),
            )
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::message_link;

    #[test]
    fn a_message_link_names_the_guild_channel_and_message() {
        assert_eq!(
            message_link("https://discord.com/channels/1/2/3"),
            Some((Some(1), 2, 3))
        );
        assert_eq!(
            message_link("https://canary.discord.com/channels/@me/20/30?x=1"),
            Some((None, 20, 30))
        );
        // A channel link has no message; other hosts and paths are not routes of this client.
        assert_eq!(message_link("https://discord.com/channels/1/2"), None);
        assert_eq!(message_link("https://example.com/channels/1/2/3"), None);
        assert_eq!(message_link("https://discord.com/channels/1/2/3/4"), None);
        assert_eq!(message_link("https://discord.com/channels/x/2/3"), None);
    }

    use super::{hostname, is_trusted_domain, split};

    #[test]
    fn splits_like_the_modal() {
        let parts = split("https://example.com/gravecord/test");
        assert_eq!(parts.protocol, "https:");
        assert_eq!(parts.authority, "//");
        assert_eq!(parts.host, "example.com");
        assert_eq!(parts.rest, "/gravecord/test");
        assert_eq!(hostname("https://user@Example.COM:8080/x"), "example.com");
    }

    #[test]
    fn discord_hosts_are_trusted() {
        assert!(is_trusted_domain("https://discord.com/channels/@me"));
        assert!(is_trusted_domain("https://cdn.discordapp.com/a.png"));
        assert!(is_trusted_domain("https://discord.gg/abc"));
        assert!(!is_trusted_domain("https://discord.com.evil.example/"));
    }
}
