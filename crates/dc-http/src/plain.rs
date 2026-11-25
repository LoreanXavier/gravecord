//! The client for every host that is not Discord's: plugin services, Spotify's pictures, GIF previews, embed pictures
//! Discord gave no proxy address for, the update check. It sends none of the pinned client's headers: no Discord user
//! agent, no client information (`x-super-properties`), no `x-discord-*` locale or time zone, no `accept-language`, no
//! `origin` and no referer, not even on a redirect. It keeps no cookies. Its user agent is the pinned one without the
//! `discord/` and `Electron/` parts, which is what the same Chromium sends as a plain browser.
//!
//! Requests to Discord ([`is_discord_url`]) keep the pinned client and its header set unchanged.

use std::sync::OnceLock;
use std::time::Duration;

/// The registrable domains whose hosts are Discord's own: the API and web assets (`discord.com`, `discordapp.com`),
/// the CDN and media proxy (`discordapp.net`) and the voice servers (`discord.media`). A host is Discord's when it is
/// one of these or a subdomain of one. Activities (`discordsays.com`) are not: their pages front other parties'
/// servers, and they run in a web view anyway.
pub const DISCORD_DOMAINS: &[&str] = &[
    "discord.com",
    "discordapp.com",
    "discordapp.net",
    "discord.media",
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

/// The builder for a client outside the pinned one that may be asked for `url`: for a Discord address the bare
/// builder these requests always had (no user agent, no default headers), so they reach Discord exactly as before;
/// for any other address the plain [`builder`]. Neither sends a referer on a redirect.
pub fn builder_for(url: &str) -> reqwest::ClientBuilder {
    if is_discord_url(url) {
        reqwest::Client::builder().referer(false)
    } else {
        builder()
    }
}

/// The shared client for one request to `url` ([`builder_for`]'s choice, reqwest's default redirects).
pub fn client_for(url: &str) -> reqwest::Client {
    static BARE: OnceLock<reqwest::Client> = OnceLock::new();
    if is_discord_url(url) {
        BARE.get_or_init(|| {
            reqwest::Client::builder()
                .referer(false)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
    } else {
        client()
    }
}

/// Whether the pinned client, which carries the Discord identity, may follow a redirect from `previous` to `next`:
/// to the same host, or to another Discord host. A hop to any other host is refused, so the pinned header set never
/// reaches a site outside Discord through a redirect (reqwest drops only `authorization` and cookies on such a hop).
pub fn pinned_may_follow(previous: &reqwest::Url, next: &reqwest::Url) -> bool {
    next.host_str().is_some_and(is_discord_host)
        || (next.host_str() == previous.host_str()
            && next.port_or_known_default() == previous.port_or_known_default())
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
            "wss://c-ams01-1234.discord.media/",
            "https://discord.com/api/v9/gateway",
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
            "https://1234.discordsays.com/",
            "https://app-directory-assets.discordactivities.com/lofi/directory_01.gif",
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
    fn the_pinned_client_follows_redirects_only_within_discord() {
        let url = |text: &str| reqwest::Url::parse(text).unwrap();
        let cdn = url("https://cdn.discordapp.com/attachments/1/2/a.png");
        assert!(pinned_may_follow(
            &cdn,
            &url("https://media.discordapp.net/a.png")
        ));
        assert!(pinned_may_follow(&cdn, &url("https://discord.com/a")));
        assert!(!pinned_may_follow(&cdn, &url("https://example.com/a.png")));
        assert!(!pinned_may_follow(
            &cdn,
            &url("https://evildiscordapp.net/a")
        ));
        // A test or replay server on one host keeps its own redirects.
        let local = url("http://127.0.0.1:4000/a");
        assert!(pinned_may_follow(&local, &url("http://127.0.0.1:4000/b")));
        assert!(!pinned_may_follow(&local, &url("http://127.0.0.1:4001/b")));
        assert!(!pinned_may_follow(&local, &url("http://localhost:4000/b")));
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
