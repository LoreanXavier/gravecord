//! The connection of the pinned client's Chromium (152, under Electron 44) for every request and socket to a Discord
//! host: its TLS ClientHello, ALPN, HTTP/2 settings, priorities and pseudo-header order, as wreq-util's Chrome 152
//! profile builds them, and the header order Chromium writes a `fetch()` in. Requests to any other host keep the plain
//! client ([`crate::plain`]).
//!
//! Adapted from Serein (MIT OR Apache-2.0), crates/client-core/src/fingerprint/transport.rs.

use dc_proto::pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};
use wreq::Uri;
use wreq::header::{HeaderValue, OrigHeaderMap};

/// The Chromium of the pinned build (`Chrome/152.0.7977.169`, `pin::USER_AGENT`) on Windows. wreq-util's profile for
/// that major version, less the `trust_anchors` extension: Chrome 152 sends it, the Electron 44 desktop client does
/// not (Serein's recording of the desktop client). Its headers are left out: they describe a page navigation, and
/// each request here carries its own.
pub fn emulation() -> wreq::Emulation {
    let mut emulation = wreq::IntoEmulation::into_emulation(
        wreq_util::Emulation::builder()
            .profile(wreq_util::Profile::Chrome152)
            .platform(wreq_util::Platform::Windows)
            .headers(false)
            .build(),
    );
    if let Some(tls) = &mut emulation.tls_options {
        tls.trust_anchors = None;
    }
    emulation
}

/// The header order of a `fetch()` from the web app's page, as Chromium writes it: the page's own headers in the order
/// Blink hands them over (the recorded `requestHeaders` of `TRAFFIC/identify.json`, without the two the network
/// stack adds itself), then the ones the network stack adds. A header not listed goes after these.
pub const FETCH_ORDER: &[&str] = &[
    "content-length",
    "sec-ch-ua-platform",
    "authorization",
    "x-installation-id",
    "x-debug-options",
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "x-discord-timezone",
    "x-super-properties",
    "x-discord-locale",
    "x-context-properties",
    "x-fingerprint",
    "x-captcha-key",
    "x-captcha-rqtoken",
    "x-captcha-session-id",
    "x-audit-log-reason",
    "user-agent",
    "content-type",
    "accept",
    "origin",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-dest",
    "referer",
    "accept-encoding",
    "accept-language",
    "cookie",
    "priority",
];

/// [`FETCH_ORDER`] as wreq takes it.
pub fn fetch_order() -> OrigHeaderMap {
    let mut order = OrigHeaderMap::new();
    for name in FETCH_ORDER {
        order.insert(*name);
    }
    order
}

/// The cookies Discord's hosts set (Cloudflare's `__cfruid` and `_cfuvid`, Discord's `__dcfduid` and `__sdcfduid`),
/// kept for the run and sent back to the hosts they belong to, as the browser's cookie store does. Shared by every
/// Discord client of the process, so a new pool (`get_hedged`) and the login client send the same ones.
pub fn cookies() -> Arc<wreq::cookie::Jar> {
    static JAR: OnceLock<Arc<wreq::cookie::Jar>> = OnceLock::new();
    JAR.get_or_init(|| Arc::new(wreq::cookie::Jar::default()))
        .clone()
}

/// Whether a client carrying the Discord identity may follow a redirect from `previous` to `next`: to another Discord
/// host, or to the same host and port. A hop to any other host is refused, so the identity never reaches a site
/// outside Discord through a redirect.
pub fn may_follow(previous: &Uri, next: &Uri) -> bool {
    fn port(uri: &Uri) -> Option<u16> {
        uri.port_u16().or(match uri.scheme_str() {
            Some("https" | "wss") => Some(443),
            Some("http" | "ws") => Some(80),
            _ => None,
        })
    }
    next.host().is_some_and(crate::plain::is_discord_host)
        || (next.host().is_some() && next.host() == previous.host() && port(next) == port(previous))
}

/// Ten hops as a browser allows, never off Discord ([`may_follow`]).
pub fn redirects(limit: usize) -> wreq::redirect::Policy {
    wreq::redirect::Policy::custom(move |attempt| {
        if attempt.previous.len() > limit {
            attempt.error("too many redirects")
        } else if attempt
            .previous
            .last()
            .is_some_and(|previous| may_follow(previous, &attempt.uri))
        {
            attempt.follow()
        } else {
            attempt.error("redirect away from Discord refused")
        }
    })
}

/// The `Accept-Encoding` Chromium sends (`TRAFFIC/identify.json`); wreq's own lists the same codings in another order.
pub const ACCEPT_ENCODING: &str = "gzip, deflate, br, zstd";

/// A builder for a client that talks to Discord: the Chromium handshake and header order, the pinned user agent and
/// `Accept-Encoding`, the shared cookie store, redirects kept on Discord, a connect timeout that covers TCP and TLS.
/// Callers add their own headers, timeouts and pool settings.
pub fn builder() -> wreq::ClientBuilder {
    let mut headers = wreq::header::HeaderMap::new();
    headers.insert(
        wreq::header::ACCEPT_ENCODING,
        HeaderValue::from_static(ACCEPT_ENCODING),
    );
    wreq::Client::builder()
        .emulation(emulation())
        .orig_headers(fetch_order())
        .default_headers(headers)
        .user_agent(pin::USER_AGENT)
        .cookie_provider(cookies())
        .redirect(redirects(10))
        .connect_timeout(Duration::from_secs(10))
}

/// The shared client for a file from a Discord host outside the API routes (an attachment, a sound, a video).
pub fn client() -> wreq::Client {
    static CLIENT: OnceLock<wreq::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            builder()
                .pool_max_idle_per_host(8)
                .build()
                .unwrap_or_else(|_| wreq::Client::new())
        })
        .clone()
}

/// The header names of the browser's HTTP/1.1 WebSocket upgrade, in its order and casing (Serein's recording of the
/// desktop client). Cloudflare answers the gateway's upgrade with 403 when `Origin` arrives lowercased.
const WEBSOCKET_ORDER: &[&str] = &[
    "Host",
    "Connection",
    "Pragma",
    "Cache-Control",
    "User-Agent",
    "Upgrade",
    "Origin",
    "Sec-WebSocket-Version",
    "Accept-Encoding",
    "Accept-Language",
    "Sec-WebSocket-Key",
    "Sec-WebSocket-Extensions",
];

/// Why a WebSocket could not be opened: the connection, the TLS handshake or the upgrade failed.
pub type SocketError = wreq::Error;

/// Opens a WebSocket to a Discord host from the web app's page (`origin`, `pin::WEBAPP_ORIGIN` for the gateway and
/// voice) with the Chromium handshake, as the browser opens one: no proxy, no redirect, no retry. The upgraded
/// connection is handed to tungstenite with the caller's frame limits. `ws://` (a local test server) works the same.
pub async fn websocket(
    url: &str,
    origin: &str,
    accept_language: &str,
    config: WebSocketConfig,
) -> Result<WebSocketStream<wreq::Upgraded>, SocketError> {
    open(url, origin, accept_language, config, None).await
}

/// [`websocket`] to addresses already looked up for the url's host (the voice server's, whose lookup is timed on
/// its own), instead of a lookup of its own.
pub async fn websocket_to(
    url: &str,
    resolved: (&str, &[std::net::SocketAddr]),
    origin: &str,
    accept_language: &str,
    config: WebSocketConfig,
) -> Result<WebSocketStream<wreq::Upgraded>, SocketError> {
    open(url, origin, accept_language, config, Some(resolved)).await
}

async fn open(
    url: &str,
    origin: &str,
    accept_language: &str,
    config: WebSocketConfig,
    resolved: Option<(&str, &[std::net::SocketAddr])>,
) -> Result<WebSocketStream<wreq::Upgraded>, SocketError> {
    let mut casing = OrigHeaderMap::new();
    for name in WEBSOCKET_ORDER {
        casing.insert(*name);
    }
    let mut builder = wreq::Client::builder()
        .emulation(emulation())
        .no_proxy()
        .tcp_nodelay(true)
        .redirect(wreq::redirect::Policy::none())
        .retry(wreq::retry::Policy::never());
    if let Some((host, addrs)) = resolved {
        builder = builder.resolve_to_addrs(host.to_owned(), addrs.iter().copied());
    }
    let mut request = builder
        .build()?
        .websocket(url)
        .header(wreq::header::USER_AGENT, pin::USER_AGENT)
        .header(wreq::header::CACHE_CONTROL, "no-cache")
        .header(wreq::header::PRAGMA, "no-cache")
        .header(wreq::header::ACCEPT_ENCODING, ACCEPT_ENCODING)
        .orig_headers(casing);
    if let Ok(origin) = HeaderValue::from_str(origin) {
        request = request.header(wreq::header::ORIGIN, origin);
    }
    if let Ok(language) = HeaderValue::from_str(accept_language) {
        request = request.header(wreq::header::ACCEPT_LANGUAGE, language);
    }
    let socket = request.send().await?.into_websocket().await?;
    Ok(WebSocketStream::from_raw_socket(socket.into_inner(), Role::Client, Some(config)).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_stay_on_discord() {
        let uri = |text: &str| text.parse::<Uri>().unwrap();
        let cdn = uri("https://cdn.discordapp.com/attachments/1/2/a.png");
        assert!(may_follow(&cdn, &uri("https://media.discordapp.net/a.png")));
        assert!(may_follow(&cdn, &uri("https://discord.com/a")));
        assert!(!may_follow(&cdn, &uri("https://example.com/a.png")));
        assert!(!may_follow(&cdn, &uri("https://evildiscordapp.net/a")));
        let local = uri("http://127.0.0.1:4000/a");
        assert!(may_follow(&local, &uri("http://127.0.0.1:4000/b")));
        assert!(!may_follow(&local, &uri("http://127.0.0.1:4001/b")));
        assert!(!may_follow(&local, &uri("http://localhost:4000/b")));
    }

    #[test]
    fn the_profile_is_the_pinned_chromium() {
        assert!(pin::USER_AGENT.contains(" Chrome/152."));
        let emulation = emulation();
        assert!(emulation.tls_options.is_some());
        assert!(emulation.http2_options.is_some());
        assert!(emulation.headers.is_empty());
    }

    /// A local server reads the upgrade as it arrives on the wire, answers it, and echoes one frame: the browser's
    /// header names, casing and order, the page's origin and language, and a working socket over `ws://`.
    #[tokio::test]
    async fn the_socket_upgrade_is_the_browsers() {
        use futures_util::{SinkExt, StreamExt};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_tungstenite::tungstenite::Message;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !seen.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).await.unwrap();
                seen.extend_from_slice(&buffer[..read]);
            }
            let head = String::from_utf8_lossy(&seen).into_owned();
            let key = head
                .lines()
                .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
                .unwrap()
                .trim()
                .to_owned();
            let accept =
                tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                         Sec-WebSocket-Accept: {accept}\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut socket = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
            if let Some(Ok(message)) = socket.next().await {
                socket.send(message).await.unwrap();
            }
            head
        });
        let mut socket = websocket(
            &format!("ws://{address}/?v=10"),
            pin::WEBAPP_ORIGIN,
            "en-US",
            WebSocketConfig::default(),
        )
        .await
        .unwrap();
        socket.send(Message::text("ping")).await.unwrap();
        assert_eq!(socket.next().await.unwrap().unwrap(), Message::text("ping"));
        let head = server.await.unwrap();
        let names: Vec<&str> = head
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':').map(|(name, _)| name))
            .collect();
        let order: Vec<&str> = WEBSOCKET_ORDER
            .iter()
            .copied()
            .filter(|name| names.contains(name))
            .collect();
        assert_eq!(names, order, "{head}");
        assert!(head.starts_with("GET /?v=10 HTTP/1.1\r\n"), "{head}");
        assert!(
            head.contains("\r\nOrigin: https://discordapp.com\r\n"),
            "{head}"
        );
        assert!(head.contains(&format!("\r\nUser-Agent: {}\r\n", pin::USER_AGENT)));
        assert!(head.contains("\r\nAccept-Language: en-US\r\n"), "{head}");
        assert!(head.contains("\r\nPragma: no-cache\r\n"), "{head}");
        assert!(
            head.contains("\r\nAccept-Encoding: gzip, deflate, br, zstd\r\n"),
            "{head}"
        );
    }
}
