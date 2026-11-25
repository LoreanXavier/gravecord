//! The client for every host that is not Discord's: plugin services, Spotify's pictures, GIF previews, embed pictures
//! Discord gave no proxy address for, the update check. It sends none of the pinned client's headers: no Discord user
//! agent, no client information (`x-super-properties`), no `x-discord-*` locale or time zone, no `accept-language`, no
//! `origin` and no referer, not even on a redirect. It keeps no cookies. Its user agent is the pinned one without the
//! `discord/` and `Electron/` parts, which is what the same Chromium sends as a plain browser.
//!
//! Requests to Discord ([`is_discord_url`]) keep the pinned client and its header set unchanged.

use std::sync::OnceLock;
use std::time::Duration;

/// The registrable domains whose hosts are Discord's: the API, CDN and media proxy, the voice servers, the gateway,
/// Activities and their assets. A host is Discord's when it is one of these or a subdomain of one.
pub const DISCORD_DOMAINS: &[&str] = &[
    "discord.com",
    "discordapp.com",
    "discordapp.net",
    "discord.media",
    "discord.gg",
    "discordsays.com",
    "discordactivities.com",
];

/// Whether `host` is one of [`DISCORD_DOMAINS`] or a subdomain of one, compared on a label boundary, so
/// `evildiscord.com` and `discord.com.example.net` are not.
pub fn is_discord_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    DISCORD_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

/// Whether `url` is an http(s) or ws(s) address on a Discord host. Anything that does not parse is not.
pub fn is_discord_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "https" | "http" | "wss" | "ws")
            && parsed.host_str().is_some_and(is_discord_host)
    })
}

/// The user agent of non-Discord requests: the pinned one without its `discord/<version>` and `Electron/<version>`
/// tokens (`Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/<v> Safari/537.36`).
pub fn user_agent() -> &'static str {
    static AGENT: OnceLock<String> = OnceLock::new();
    AGENT.get_or_init(|| {
        dc_proto::pin::USER_AGENT
            .split(' ')
            .filter(|token| !token.starts_with("discord/") && !token.starts_with("Electron/"))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// A builder for a non-Discord client: the plain user agent, no default headers, no referer on redirects, no cookie
/// store, a connect timeout. Callers add their own redirect, timeout and HTTPS rules on top.
pub fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(user_agent())
        .referer(false)
        .connect_timeout(Duration::from_secs(10))
}

/// The shared non-Discord client (the [`builder`] defaults, up to ten redirects as reqwest follows them).
pub fn client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            builder()
                .pool_max_idle_per_host(8)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_hosts_are_matched_on_a_label_boundary() {
        for url in [
            "https://discordapp.com/api/v9/users/@me",
            "https://discord.com/assets/a.woff2",
            "https://cdn.discordapp.com/attachments/1/2/a.zip",
            "https://media.discordapp.net/attachments/1/2/a.png",
            "https://images-ext-1.discordapp.net/external/x/https/example.com/a.png",
            "wss://gateway.discord.gg/?v=9",
            "wss://c-ams01-1234.discord.media/",
            "https://1234.discordsays.com/",
            "https://app-directory-assets.discordactivities.com/lofi/directory_01.gif",
            "https://CDN.DiscordApp.com./a",
        ] {
            assert!(is_discord_url(url), "{url}");
        }
        for url in [
            "https://i.scdn.co/image/ab67616d0000b273",
            "https://static-cdn.jtvnw.net/previews-ttv/live_user_x.jpg",
            "https://i.ytimg.com/vi/x/hqdefault.jpg",
            "https://media.klipy.com/gifs/x.gif",
            "https://media.tenor.com/x.gif",
            "https://api.github.com/repos/x/y/releases/latest",
            "https://objects.githubusercontent.com/x",
            "https://translate.googleapis.com/translate_a/single",
            "https://badges.equicord.org/",
            "https://discord-attachments-uploads-prd.storage.googleapis.com/x",
            "https://evildiscord.com/",
            "https://discord.com.example.net/",
            "https://notdiscordapp.net/",
            "http://127.0.0.1:47874/cdn.discordapp.com/a",
            "file:///C:/discord.com",
            "not a url",
        ] {
            assert!(!is_discord_url(url), "{url}");
        }
    }

    #[test]
    fn the_plain_user_agent_is_the_pinned_one_without_discord_or_electron() {
        let agent = user_agent();
        assert!(agent.starts_with("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"));
        assert!(agent.contains(" Chrome/"));
        assert!(agent.ends_with(" Safari/537.36"));
        assert!(!agent.to_ascii_lowercase().contains("discord"));
        assert!(!agent.contains("Electron"));
        assert!(!agent.contains("  "));
    }
}
