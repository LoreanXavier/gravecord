//! The coded links of a message (module 486889 `ei`, the types of module 167189).
//!
//! A message record carries `codedLinks: NON_PARSED.has(type) ? [] : ei(content)` (module 432005). `ei`:
//!
//! 1. puts `http://` before a bare Discord host (`q`: `discord.gg`, `discord.new`, `discord.com`, `discordapp.com`)
//!    at the start of the text or after whitespace, leaving hosts inside a link alone;
//! 2. blanks the code spans and code blocks (`p`), finds the links (`I`, module 486889's copy of the URL rule) and
//!    trims trailing `.,;:?'*_~|`; the first `dev://` link (`V`) is added last;
//! 3. reads each link with the `url` package's `parse` (module 567243) and checks it, in this order, against the
//!    invite and template hosts and the client's own hosts (`discord.com`, canary, ptb, `discordapp.com`);
//! 4. keeps each code once, whatever its type, and at most ten.
//!
//! The card each type draws is `renderCodedLinks` of module 679636; gift links are not coded links (module 45938).
use regex::Regex;
use std::sync::OnceLock;

/// The types of module 167189, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Invite,
    Template,
    BuildOverride,
    ManualBuildOverride,
    Experiment,
    Event,
    ChannelLink,
    AppDirectoryProfile,
    AppDirectoryStorefront,
    AppDirectoryStorefrontSku,
    AppOauth2Link,
    ActivityBookmark,
    EmbeddedActivityInvite,
    GuildProduct,
    ServerShop,
    SocialLayerStorefront,
    SocialLayerStorefrontApp,
    QuestsEmbed,
    CollectiblesShop,
    GameProfile,
    GameServerShare,
    UserProfile,
    UserWishlist,
    GameOrganizationInvite,
    ConjureProject,
}

/// One `{type, code, url}` of `ei`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodedLink {
    pub kind: Kind,
    pub code: String,
    pub url: String,
}

/// `MRS.NON_PARSED` of module 448761: recipient add/remove... types whose content is never parsed for links.
const NON_PARSED: [u64; 6] = [4, 12, 18, 27, 28, 31];
/// `ei` keeps the first ten.
const LIMIT: usize = 10;
/// A query longer than this is ignored (`e.query.length <= 1e3`).
const QUERY_LIMIT: usize = 1000;

/// `GLOBAL_ENV.INVITE_HOST` and `GUILD_TEMPLATE_HOST` of the pinned build.
pub const INVITE_HOST: &str = "discord.gg";
pub const TEMPLATE_HOST: &str = "discord.new";
/// `W`, `Y`, `K`, `$`, `X`: `WEBAPP_ENDPOINT` (`//discord.com`), canary, ptb, discordapp.com, discord.com.
const PRIMARY_HOSTS: [&str; 5] = [
    "discord.com",
    "canary.discord.com",
    "ptb.discord.com",
    "discordapp.com",
    "discord.com",
];
/// `z`: the hosts `q` gives a scheme to (invite, template, `WEBAPP_ENDPOINT`, discordapp.com, discord.com).
const BARE_HOSTS: [&str; 5] = [
    "discord.gg",
    "discord.new",
    "discord.com",
    "discordapp.com",
    "discord.com",
];
/// `tM` of module 679636 is outside the parser; `GAME_SHOPS` of module 758836 is the tab `/shop` reads.
const GAME_SHOPS_TAB: &str = "game-shops";
/// `k`: the prefix of a wishlist code.
const WISHLIST_PREFIX: &str = "wishlist-";
/// `F.APPLICATIONS_COMMANDS` of module 179771.
const APPLICATIONS_COMMANDS: &str = "applications.commands";
/// The two hosts of module 729160.
const CONJURE_HOSTS: [&str; 2] = ["conjure.town", "conjure.city"];

fn regex(cell: &'static OnceLock<Regex>, source: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(source).expect("coded link pattern"))
}

/// `I` of module 486889. JavaScript's `[¡-￿]` without the `u` flag also takes both halves of a surrogate
/// pair, so here the range runs to the last code point. `h` is ip-regex's `v4().source`.
fn url_rule() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(
        &CELL,
        concat!(
            r#"(?i)(?:(?:(?:https?:)?//)|www\.)(?:[^\s:@]+(?::[^\s@]*)?@)?"#,
            r#"(?:localhost|(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]\d|\d)(?:\.(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]\d|\d)){3}"#,
            r#"|(?:[a-z\x{a1}-\x{10ffff}0-9\-_]+\.)+(?:[a-z\x{a1}-\x{10ffff}]{2,}))"#,
            r#"(?::\d{2,5})?(?:[/?#][^\s"()\[\]<>]*)?"#,
        ),
    )
}

/// `V`: a `dev://` link (build overrides).
fn dev_rule() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"(?i)dev://[\w\-.~:/?#\[\]@!$&'()*+,;=%]+")
}

/// `q`: `((https?://[^ ]*)|^|\s)(hosts)`, global, case sensitive.
fn bare_host_rule() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        let hosts: Vec<String> = BARE_HOSTS.iter().map(|host| regex::escape(host)).collect();
        Regex::new(&format!(r"((https?://[^ ]*)|^|\s)({})", hosts.join("|")))
            .expect("coded link pattern")
    })
}

/// `q`'s replace: a host with no link before it gets `http://`.
fn add_schemes(text: &str) -> String {
    bare_host_rule()
        .replace_all(text, |caps: &regex::Captures<'_>| {
            if caps.get(2).is_some() {
                caps[0].to_owned()
            } else {
                format!("{}http://{}", &caps[1], &caps[3])
            }
        })
        .into_owned()
}

/// How many backticks start at `at`.
fn ticks(bytes: &[u8], at: usize) -> usize {
    bytes[at..].iter().take_while(|b| **b == b'`').count()
}

/// `p`: `` `{3,}[\s\S]*?(?:`{3,}|$)|(`{1,2})[^`]+\1(?!`) `` replaced by a space, global. A match of the first form
/// runs to the next run of three or more backticks (all of it) or to the end; the second needs a non-empty run of
/// other characters closed by the same one or two backticks, not followed by another.
fn blank_code(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut copied = 0;
    while at < bytes.len() {
        let run = ticks(bytes, at);
        let end = if run >= 3 {
            let mut search = at + run;
            loop {
                if search >= bytes.len() {
                    break Some(bytes.len());
                }
                let closing = ticks(bytes, search);
                if closing >= 3 {
                    break Some(search + closing);
                }
                search += 1;
            }
        } else if run >= 1 {
            // `{1,2}` greedy: two backticks first; with two, one cannot work (the content would start with a tick).
            let open = run;
            let start = at + open;
            let close = bytes[start..]
                .iter()
                .position(|b| *b == b'`')
                .map(|offset| start + offset);
            match close {
                Some(close) if close > start && ticks(bytes, close) == open => Some(close + open),
                _ => None,
            }
        } else {
            None
        };
        match end {
            Some(end) => {
                out.push_str(&text[copied..at]);
                out.push(' ');
                at = end;
                copied = end;
            }
            None => {
                // Next character (backticks are ASCII, so byte steps stay on boundaries for them; skip whole chars).
                at += text[at..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// `T`: trailing `.,;:?'*_~|` dropped.
fn trim_tail(url: &str) -> &str {
    url.trim_end_matches(['.', ',', ';', ':', '?', '\'', '*', '_', '~', '|'])
}

/// What `url.parse(e)` (module 567243, the legacy Node parser, no query parsing) yields that `ei` reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedUrl {
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub pathname: Option<String>,
    pub search: Option<String>,
    pub query: Option<String>,
    pub hash: Option<String>,
}

/// `[a-z0-9.+-]+:` at the start.
fn protocol_len(text: &str) -> Option<usize> {
    let len = text
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
        .count();
    (len > 0 && text.as_bytes().get(len) == Some(&b':')).then_some(len + 1)
}

/// `l`: the characters the parser percent-escapes in what follows the host.
const AUTO_ESCAPE: [char; 13] = [
    '\'', '{', '}', '|', '\\', '^', '`', '<', '>', '"', ' ', '\r', '\n',
];

fn escape_char(c: char) -> String {
    format!("%{:02X}", c as u32)
}

/// `url.parse(e)` for the parts `ei` reads.
pub fn parse_url(raw: &str) -> ParsedUrl {
    let mut out = ParsedUrl::default();
    // Back slashes before the query (or, when no `#` follows a `?`, before the first `#`) become slashes.
    let question = raw.find('?');
    let hash_at = raw.find('#');
    let splitter = match (question, hash_at) {
        (Some(q), Some(h)) if q < h => '?',
        _ => '#',
    };
    let mut fixed = String::with_capacity(raw.len());
    match raw.split_once(splitter) {
        Some((head, tail)) => {
            fixed.push_str(&head.replace('\\', "/"));
            fixed.push(splitter);
            fixed.push_str(tail);
        }
        None => fixed.push_str(&raw.replace('\\', "/")),
    }
    let mut rest: &str = fixed.trim();
    // The `s` fast path: no protocol, one or two leading slashes, no `#` (a path only).
    if !fixed.contains('#') && rest.starts_with('/') && !rest.starts_with("///") {
        let path_end = rest.find(['?']).unwrap_or(rest.len());
        let (path, search) = rest.split_at(path_end);
        if !path.chars().any(char::is_whitespace) && !search.chars().any(char::is_whitespace) {
            out.pathname = Some(path.to_owned());
            if !search.is_empty() {
                out.search = Some(search.to_owned());
                out.query = Some(search[1..].to_owned());
            }
            return out;
        }
    }
    let protocol = protocol_len(rest).map(|len| {
        let proto = rest[..len].to_owned();
        rest = &rest[len..];
        proto
    });
    let lower = protocol.as_ref().map(|proto| proto.to_ascii_lowercase());
    out.protocol = lower.clone();
    let hostless = lower.as_deref() == Some("javascript:");
    let slashed = |proto: &Option<String>| {
        matches!(
            proto.as_deref(),
            Some("http:" | "https:" | "ftp:" | "gopher:" | "file:")
        )
    };
    let auth_like = rest.starts_with("//") && {
        let after = &rest[2..];
        after.split_once('@').is_some_and(|(user, host)| {
            !user.is_empty() && !user.contains('/') && !host.is_empty() && !host.starts_with('/')
        })
    };
    let mut slashes = false;
    if (protocol.is_some() || auth_like) && rest.starts_with("//") && !hostless {
        rest = &rest[2..];
        slashes = true;
    }
    // `m[w]` is read with the protocol as written.
    let written_slashed = slashed(&protocol);
    let simple_rest = if !hostless && (slashes || (protocol.is_some() && !written_slashed)) {
        let host_end = rest.find(['/', '?', '#']);
        let at_sign = match host_end {
            None => rest.rfind('@'),
            Some(end) => rest[..end].rfind('@'),
        };
        if let Some(at) = at_sign {
            rest = &rest[at + 1..];
        }
        let end = rest
            .find(|c: char| {
                matches!(c, '%' | '/' | '?' | ';' | '#') || AUTO_ESCAPE.contains(&c) || c == '\t'
            })
            .unwrap_or(rest.len());
        let host = &rest[..end];
        let mut owned_rest = rest[end..].to_owned();
        // `parseHost`: a trailing `:digits` is the port.
        let (mut hostname, port) = match host.rfind(':') {
            Some(colon) if host[colon + 1..].bytes().all(|b| b.is_ascii_digit()) => {
                (host[..colon].to_owned(), Some(host[colon + 1..].to_owned()))
            }
            _ => (host.to_owned(), None),
        };
        let bracketed = hostname.starts_with('[') && hostname.ends_with(']');
        if !bracketed {
            let parts: Vec<&str> = hostname.split('.').collect();
            let valid = |part: &str| {
                part.len() <= 63
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'_' | b'-'))
            };
            for (index, part) in parts.iter().enumerate() {
                if part.is_empty() || valid(part) {
                    continue;
                }
                let ascii: String = part
                    .chars()
                    .map(|c| if (c as u32) > 127 { 'x' } else { c })
                    .collect();
                if valid(&ascii) {
                    continue;
                }
                let mut good: Vec<String> =
                    parts[..index].iter().map(|p| (*p).to_owned()).collect();
                let mut not_host: Vec<String> =
                    parts[index + 1..].iter().map(|p| (*p).to_owned()).collect();
                let lead = part
                    .char_indices()
                    .take_while(|(i, c)| {
                        *i < 63 && (c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '-'))
                    })
                    .count();
                let (head, tail) = part.split_at(lead);
                good.push(head.to_owned());
                not_host.insert(0, tail.to_owned());
                if !not_host.is_empty() {
                    owned_rest = format!("/{}{}", not_host.join("."), owned_rest);
                }
                hostname = good.join(".");
                break;
            }
        }
        hostname = if hostname.len() > 255 {
            String::new()
        } else {
            hostname.to_lowercase()
        };
        let host = match &port {
            Some(port) if !port.is_empty() => format!("{hostname}:{port}"),
            _ => hostname.clone(),
        };
        out.host = Some(host);
        if bracketed && !owned_rest.starts_with('/') {
            owned_rest.insert(0, '/');
        }
        owned_rest
    } else {
        rest.to_owned()
    };
    let mut rest = simple_rest;
    if !hostless {
        for c in AUTO_ESCAPE.iter().chain(['\t'].iter()) {
            if rest.contains(*c) {
                rest = rest.replace(*c, &escape_char(*c));
            }
        }
    }
    if let Some(at) = rest.find('#') {
        out.hash = Some(rest[at..].to_owned());
        rest.truncate(at);
    }
    if let Some(at) = rest.find('?') {
        out.search = Some(rest[at..].to_owned());
        out.query = Some(rest[at + 1..].to_owned());
        rest.truncate(at);
    }
    if !rest.is_empty() {
        out.pathname = Some(rest);
    }
    if slashed(&lower)
        && out.host.as_deref().is_some_and(|host| !host.is_empty())
        && out.pathname.is_none()
    {
        out.pathname = Some("/".to_owned());
    }
    out
}

/// `Z`: the link's host, `www.` dropped (any case), is the host.
fn same_host(host: &str, url: &ParsedUrl) -> bool {
    url.host.as_deref().is_some_and(|candidate| {
        let candidate = if candidate.len() >= 4 && candidate[..4].eq_ignore_ascii_case("www.") {
            &candidate[4..]
        } else {
            candidate
        };
        candidate == host
    })
}

/// `J`: the path on that host (no host here has a path prefix); an empty path is none.
fn remaining_path(host: &str, url: &ParsedUrl) -> Option<String> {
    if !same_host(host, url) {
        return None;
    }
    let path = url.pathname.clone().unwrap_or_default();
    (!path.is_empty()).then_some(path)
}

/// `ee`: the path on any of the client's own hosts.
pub fn primary_path(url: &ParsedUrl) -> Option<String> {
    PRIMARY_HOSTS
        .iter()
        .find_map(|host| remaining_path(host, url))
}

/// `N`: `^/([a-zA-Z0-9-]+)$`, the single segment of a short host.
fn single_segment(path: &str) -> Option<&str> {
    let segment = path.strip_prefix('/')?;
    (!segment.is_empty()
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    .then_some(segment)
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `[0-9-]+`.
fn is_digits_or_dash(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}

/// `isProbablyAValidSnowflake` (module 811091): 17 to 19 digits.
fn snowflake(text: &str) -> bool {
    (17..=19).contains(&text.len()) && is_digits(text)
}

/// `decodeURIComponent`; none when it would throw.
fn decode_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = text.get(at + 1..at + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `decode-uri-component` (query-string's decoder): like `decodeURIComponent`, keeping what it cannot decode.
fn decode_lenient(text: &str) -> String {
    decode_component(text).unwrap_or_else(|| text.to_owned())
}

/// A query value: query-string gives `null` for a key with no `=`, an array for a key given twice.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Null,
    One(String),
    Many(Vec<Option<String>>),
}

impl Value {
    /// `p` of module 372250: the first of an array.
    fn first(&self) -> Option<&str> {
        match self {
            Value::Null => None,
            Value::One(text) => Some(text),
            Value::Many(items) => items.first().and_then(|item| item.as_deref()),
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Value::One(text) => Some(text),
            _ => None,
        }
    }
}

/// `qg` of module 492462 (query-string, `arrayFormat: none`): a leading `?`, `#` or `&` dropped, `+` is a space.
fn query_string(query: &str) -> Vec<(String, Value)> {
    let query = query.trim();
    let query = query.strip_prefix(['?', '#', '&']).unwrap_or(query);
    let mut out: Vec<(String, Value)> = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let pair = pair.replace('+', " ");
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (decode_lenient(key), Some(decode_lenient(value))),
            None => (decode_lenient(&pair), None),
        };
        match out.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, slot)) => {
                let previous = std::mem::replace(slot, Value::Null);
                *slot = match previous {
                    Value::Null => Value::Many(vec![None, value]),
                    Value::One(text) => Value::Many(vec![Some(text), value]),
                    Value::Many(mut items) => {
                        items.push(value);
                        Value::Many(items)
                    }
                };
            }
            None => out.push((key, value.map_or(Value::Null, Value::One))),
        }
    }
    out
}

fn lookup<'a>(query: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

/// `qg` of module 488428 (Node's querystring): `+` is `%20`, duplicates become arrays, a key with no `=` is `""`.
/// None when a part does not decode (`fB` then keeps the bare code).
fn node_query(query: &str) -> Option<Vec<(String, Vec<String>)>> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    if query.is_empty() {
        return Some(out);
    }
    for (index, pair) in query.split('&').enumerate() {
        if index >= 1000 {
            break;
        }
        let pair = pair.replace('+', "%20");
        let (key, value) = pair.split_once('=').unwrap_or((pair.as_str(), ""));
        let key = decode_component(key)?;
        let value = decode_component(value)?;
        match out.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, values)) => values.push(value),
            None => out.push((key, vec![value])),
        }
    }
    Some(out)
}

/// `encodeURIComponent`.
fn encode_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `fB` of module 21599: an invite's code with the `event`, `channel` and `message` of its query
/// (`code?event=..&channel=..&message=..`; the channel and message only when they are snowflakes).
pub fn invite_key(code: &str, search: Option<&str>) -> String {
    let Some(search) = search else {
        return code.to_owned();
    };
    let query = search.strip_prefix('?').unwrap_or(search);
    let Some(parsed) = node_query(query) else {
        return code.to_owned();
    };
    let first = |key: &str| {
        parsed
            .iter()
            .find(|(name, _)| name == key)
            .and_then(|(_, values)| values.first().cloned())
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(event) = first("event") {
        parts.push(format!("event={}", encode_component(&event)));
    }
    if let Some(channel) = first("channel").filter(|channel| snowflake(channel)) {
        parts.push(format!("channel={}", encode_component(&channel)));
        if let Some(message) = first("message").filter(|message| snowflake(message)) {
            parts.push(format!("message={}", encode_component(&message)));
        }
    }
    if parts.is_empty() {
        code.to_owned()
    } else {
        format!("{code}?{}", parts.join("&"))
    }
}

/// `y$` of module 21599: an invite key's parts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InviteKey {
    pub base_code: String,
    pub event: Option<String>,
    pub channel: Option<String>,
    pub message: Option<String>,
}

pub fn split_invite_key(key: &str) -> InviteKey {
    let Some((base, query)) = key.split_once('?') else {
        return InviteKey {
            base_code: key.to_owned(),
            ..InviteKey::default()
        };
    };
    let parsed = node_query(query).unwrap_or_default();
    let first = |name: &str| {
        parsed
            .iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, values)| values.first().cloned())
    };
    let channel = first("channel").filter(|channel| snowflake(channel));
    let message = channel
        .as_ref()
        .and_then(|_| first("message").filter(|message| snowflake(message)));
    InviteKey {
        base_code: base.to_owned(),
        event: first("event"),
        channel,
        message,
    }
}

/// `Z_` of module 227327: digit-only ids, each once, at most six.
fn sku_ids<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if is_digits(id) && !out.iter().any(|known| known == id) {
            out.push(id.to_owned());
            if out.len() == 6 {
                break;
            }
        }
    }
    out
}

/// `er`: the SKU of the path, else the query's `skuIds` (comma separated, or given several times).
fn storefront_skus(sku: Option<&str>, query: Option<&[(String, Value)]>) -> Vec<String> {
    if let Some(sku) = sku {
        return sku_ids([sku]);
    }
    match query.and_then(|query| lookup(query, "skuIds")) {
        Some(Value::One(ids)) => sku_ids(ids.split(',')),
        Some(Value::Many(items)) => sku_ids(items.iter().flatten().flat_map(|ids| ids.split(','))),
        _ => Vec::new(),
    }
}

/// `m5`: `{skus joined by ,}-{scope}`.
fn storefront_code(skus: &[String], scope: &str) -> String {
    format!("{}-{scope}", skus.join(","))
}

/// `rg` of module 227327: a storefront code's scope (guild or application) and SKUs.
pub fn split_storefront_code(code: &str) -> Option<(String, Vec<String>)> {
    let parts: Vec<&str> = code.split('-').collect();
    let [skus, scope] = parts.as_slice() else {
        return None;
    };
    if !is_digits(scope) {
        return None;
    }
    let skus = sku_ids(skus.split(','));
    (!skus.is_empty()).then(|| ((*scope).to_owned(), skus))
}

/// The `v` rule: `/game-shop/<guild>[/<sku>[/<slug>]]` or `/channels/<guild>/game-shop/<channel>[/<sku>[/<slug>]]`
/// (with `allowUnpagedChannelPath` false, `/channels/<guild>/game-shop` alone does not count).
fn game_shop_path(path: &str) -> Option<(String, Option<String>)> {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    let segments: Vec<&str> = trimmed.split('/').collect();
    match segments.as_slice() {
        ["", "game-shop", guild, rest @ ..] if is_digits(guild) => match rest {
            [] => Some(((*guild).to_owned(), None)),
            [sku] | [sku, _]
                if is_digits(sku) && rest.get(1).is_none_or(|slug| !slug.is_empty()) =>
            {
                Some(((*guild).to_owned(), Some((*sku).to_owned())))
            }
            _ => None,
        },
        ["", "channels", guild, "game-shop", channel, rest @ ..]
            if is_digits(guild) && is_digits(channel) =>
        {
            match rest {
                [] => Some(((*guild).to_owned(), None)),
                [sku] | [sku, _]
                    if is_digits(sku) && rest.get(1).is_none_or(|slug| !slug.is_empty()) =>
                {
                    Some(((*guild).to_owned(), Some((*sku).to_owned())))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// `R`: `^/events/(\d+)(?:/)(\d+)?((?:/)(\d+))?` on the pathname of any host: `{guild}-{event}[-{recurrence}]`.
fn event_code(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/events/")?;
    let guild_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if guild_len == 0 || rest.as_bytes().get(guild_len) != Some(&b'/') {
        return None;
    }
    let guild = &rest[..guild_len];
    let rest = &rest[guild_len + 1..];
    let event_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if event_len == 0 {
        return None;
    }
    let event = &rest[..event_len];
    let rest = &rest[event_len..];
    let recurrence = rest.strip_prefix('/').and_then(|after| {
        let len = after.bytes().take_while(u8::is_ascii_digit).count();
        (len > 0).then(|| &after[..len])
    });
    Some(match recurrence {
        Some(recurrence) => format!("{guild}-{event}-{recurrence}"),
        None => format!("{guild}-{event}"),
    })
}

/// `L`: `^/(application-directory|discovery/applications)/([0-9-]+)/?((about|images|privacy)/?)?$`.
fn app_directory_profile(path: &str) -> Option<&str> {
    let rest = path
        .strip_prefix("/application-directory/")
        .or_else(|| path.strip_prefix("/discovery/applications/"))?;
    let id_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b'-')
        .count();
    if id_len == 0 {
        return None;
    }
    let (id, tail) = rest.split_at(id_len);
    let tail = tail.strip_prefix('/').unwrap_or(tail);
    let ok = tail.is_empty() || {
        let word = tail.strip_suffix('/').unwrap_or(tail);
        matches!(word, "about" | "images" | "privacy")
    };
    ok.then_some(id)
}

/// `y`: `^/(application-directory|discovery/applications)/([0-9-]+)/store/?([0-9-]+)?/?$`.
fn app_directory_store(path: &str) -> Option<(&str, Option<&str>)> {
    let rest = path
        .strip_prefix("/application-directory/")
        .or_else(|| path.strip_prefix("/discovery/applications/"))?;
    let id_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b'-')
        .count();
    if id_len == 0 {
        return None;
    }
    let (id, tail) = rest.split_at(id_len);
    let tail = tail.strip_prefix("/store")?;
    let tail = tail.strip_prefix('/').unwrap_or(tail);
    let tail = tail.strip_suffix('/').unwrap_or(tail);
    if tail.is_empty() {
        Some((id, None))
    } else if is_digits_or_dash(tail) {
        Some((id, Some(tail)))
    } else {
        None
    }
}

/// One segment after a prefix, optionally followed by a slash: `^{prefix}(class+)/?$` (or without the slash).
fn one_segment<'a>(
    path: &'a str,
    prefix: &str,
    ok: fn(u8) -> bool,
    slash: bool,
) -> Option<&'a str> {
    let rest = path.strip_prefix(prefix)?;
    let rest = if slash {
        rest.strip_suffix('/').unwrap_or(rest)
    } else {
        rest
    };
    (!rest.is_empty() && rest.bytes().all(ok)).then_some(rest)
}

fn digit_or_dash(b: u8) -> bool {
    b.is_ascii_digit() || b == b'-'
}

fn digit(b: u8) -> bool {
    b.is_ascii_digit()
}

fn token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// `G`: `^/games/([0-9]+)(?:/[A-Za-z0-9-]*)?/?$`.
fn game_profile(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/games/")?;
    let id_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if id_len == 0 {
        return None;
    }
    let (id, tail) = rest.split_at(id_len);
    let ok = match tail.strip_prefix('/') {
        None => tail.is_empty(),
        Some(slug) => {
            let slug = slug.strip_suffix('/').unwrap_or(slug);
            slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }
    };
    ok.then_some(id)
}

/// `C`: `^/channels/([0-9]+|@me)/([0-9]+)$`.
fn channel_link(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/channels/")?;
    let (guild, channel) = rest.split_once('/')?;
    ((guild == "@me" || is_digits(guild)) && is_digits(channel)).then(|| rest.to_owned())
}

/// `O`: `^/(invite|template)/([a-zA-Z0-9-]+)/?\.?$`.
fn invite_or_template(path: &str) -> Option<(Kind, &str)> {
    let (kind, rest) = if let Some(rest) = path.strip_prefix("/invite/") {
        (Kind::Invite, rest)
    } else {
        (Kind::Template, path.strip_prefix("/template/")?)
    };
    let len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
        .count();
    if len == 0 {
        return None;
    }
    let (code, tail) = rest.split_at(len);
    let tail = tail.strip_prefix('/').unwrap_or(tail);
    matches!(tail, "" | ".").then_some((kind, code))
}

/// `t` of module 729160: a Conjure project link's application.
fn conjure_application(url: &ParsedUrl) -> Option<String> {
    if !matches!(url.protocol.as_deref(), Some("http:" | "https:")) {
        return None;
    }
    let host = url.host.as_deref()?.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if !CONJURE_HOSTS.contains(&host) {
        return None;
    }
    let path = url.pathname.as_deref()?;
    let path = path.strip_suffix('/').unwrap_or(path);
    let segments: Vec<&str> = path.split('/').collect();
    let (app, slug, ticket) = match segments.as_slice() {
        ["", app, slug] => (*app, *slug, None),
        ["", app, slug, ticket] => (*app, *slug, Some(*ticket)),
        _ => return None,
    };
    let app_ok = (17..=20).contains(&app.len()) && is_digits(app);
    let slug_ok = (1..=48).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let ticket_ok = ticket.is_none_or(|ticket| {
        ticket.split_once('.').is_some_and(|(a, b)| {
            (16..=200).contains(&a.len())
                && (16..=64).contains(&b.len())
                && a.bytes().all(token)
                && b.bytes().all(token)
        })
    });
    (app_ok && slug_ok && ticket_ok).then(|| app.to_owned())
}

/// `_` of module 123677, the parts `ei` reads: `client_id` and the scopes (`+` and spaces split them). It parses with
/// `arrayFormat: "bracket"`: `key[]` collects, a plain key given twice keeps the last value.
fn oauth2_query(query: &str) -> (String, Vec<String>) {
    let mut parsed: Vec<(String, Value)> = Vec::new();
    for (key, value) in query_string(query)
        .into_iter()
        .flat_map(|(key, value)| match value {
            Value::Many(items) => items
                .into_iter()
                .map(|item| (key.clone(), item.map_or(Value::Null, Value::One)))
                .collect::<Vec<_>>(),
            other => vec![(key, other)],
        })
    {
        match key.strip_suffix("[]") {
            Some(name) => match parsed.iter_mut().find(|(existing, _)| existing == name) {
                Some((_, Value::Many(items))) => items.push(value.as_str().map(str::to_owned)),
                Some((_, slot)) => {
                    let previous = std::mem::replace(slot, Value::Null);
                    *slot = Value::Many(vec![
                        previous.as_str().map(str::to_owned),
                        value.as_str().map(str::to_owned),
                    ]);
                }
                None => parsed.push((
                    name.to_owned(),
                    Value::Many(vec![value.as_str().map(str::to_owned)]),
                )),
            },
            None => match parsed.iter_mut().find(|(existing, _)| *existing == key) {
                Some((_, slot)) => *slot = value,
                None => parsed.push((key, value)),
            },
        }
    }
    // `t.client_id ?? ""`: an array reads as its values joined by commas.
    let client = match lookup(&parsed, "client_id") {
        None | Some(Value::Null) => String::new(),
        Some(Value::One(text)) => text.clone(),
        Some(Value::Many(items)) => items
            .iter()
            .map(|item| item.clone().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(","),
    };
    let scopes = lookup(&parsed, "scope")
        .and_then(Value::first)
        .unwrap_or_default()
        .replace('+', " ")
        .split(' ')
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
        .collect();
    (client, scopes)
}

/// `B`: `^#itemSkuId=([0-9]+)$`.
fn item_sku(hash: Option<&str>) -> Option<&str> {
    let id = hash?.strip_prefix("#itemSkuId=")?;
    is_digits(id).then_some(id)
}

/// The links `ei` looks at: `I`'s matches over the text with schemes added and code blanked, then `V`'s first.
fn candidate_urls(content: &str) -> Vec<String> {
    let text = add_schemes(content);
    let blanked = blank_code(&text);
    let mut urls: Vec<String> = url_rule()
        .find_iter(&blanked)
        .map(|found| trim_tail(found.as_str()).to_owned())
        .collect();
    if let Some(dev) = dev_rule().find(&text) {
        urls.push(dev.as_str().to_owned());
    }
    urls
}

/// The coded links of one link, in `ei`'s order; `false` when the link stops being read (`continue` on a backslash
/// in an invite).
fn link_types(raw: &str, push: &mut dyn FnMut(Kind, String)) {
    let url = parse_url(raw);
    if url.pathname.is_none() {
        return;
    }
    let invite_path = remaining_path(INVITE_HOST, &url);
    let template_path = remaining_path(TEMPLATE_HOST, &url);
    let primary = primary_path(&url);
    let query = url
        .query
        .as_deref()
        .filter(|query| query.len() <= QUERY_LIMIT);
    let parsed_query = query.map(query_string);
    if let Some(code) = invite_path.as_deref().and_then(single_segment)
        && matches!(url.protocol.as_deref(), Some("https:" | "http:"))
    {
        if raw.contains('\\') {
            return;
        }
        push(Kind::Invite, invite_key(code, url.search.as_deref()));
    }
    if let Some(code) = template_path.as_deref().and_then(single_segment) {
        push(Kind::Template, code.to_owned());
    }
    let path = primary.as_deref();
    if let Some((kind, code)) = path.and_then(invite_or_template) {
        if kind == Kind::Invite {
            if raw.contains('\\') {
                return;
            }
            push(Kind::Invite, invite_key(code, url.search.as_deref()));
        } else {
            push(kind, code.to_owned());
        }
    }
    if let Some(code) = path.and_then(channel_link) {
        push(Kind::ChannelLink, code);
    }
    if let Some(code) = url.pathname.as_deref().and_then(event_code) {
        push(Kind::Event, code);
    }
    if let (Some(path), Some(query)) = (path, query)
        && path.starts_with("/oauth2/authorize")
    {
        let (client, scopes) = oauth2_query(query);
        if !client.is_empty() && scopes.iter().all(|scope| scope == APPLICATIONS_COMMANDS) {
            push(Kind::AppOauth2Link, client);
        }
    }
    if let Some(id) = path.and_then(app_directory_profile) {
        push(Kind::AppDirectoryProfile, id.to_owned());
    }
    if let Some((app, sku)) = path.and_then(app_directory_store) {
        match sku {
            Some(sku) => push(Kind::AppDirectoryStorefrontSku, format!("{app}:{sku}")),
            None => push(Kind::AppDirectoryStorefront, app.to_owned()),
        }
    }
    if let Some(app) = path.and_then(|path| one_segment(path, "/activities/", digit_or_dash, true))
    {
        push(Kind::ActivityBookmark, app.to_owned());
    }
    if let Some(path) = path
        && let Some(rest) = path.strip_prefix("/channels/")
        && let Some((guild, product)) = rest.split_once("/shop/")
        && is_digits(guild)
        && is_digits(product)
    {
        push(Kind::GuildProduct, format!("{guild}-{product}"));
    }
    if let Some(path) = path
        && let Some(guild) = path
            .strip_prefix("/channels/")
            .and_then(|rest| rest.strip_suffix("/shop"))
        && is_digits(guild)
    {
        push(Kind::ServerShop, guild.to_owned());
    }
    if let Some((guild, sku)) = path.and_then(game_shop_path) {
        let skus = storefront_skus(sku.as_deref(), parsed_query.as_deref());
        if !skus.is_empty() {
            push(Kind::SocialLayerStorefront, storefront_code(&skus, &guild));
        }
    }
    if let Some(quest) = path.and_then(|path| one_segment(path, "/quests/", digit_or_dash, true)) {
        push(Kind::QuestsEmbed, quest.to_owned());
    }
    if let Some(game) = path.and_then(game_profile) {
        push(Kind::GameProfile, game.to_owned());
    }
    if let Some(code) =
        path.and_then(|path| one_segment(path, "/game-servers/share/", token, false))
    {
        push(Kind::GameServerShare, code.to_owned());
    }
    if let Some(code) =
        path.and_then(|path| one_segment(path, "/game-organizations/invite/", token, true))
    {
        push(Kind::GameOrganizationInvite, code.to_owned());
    }
    if let Some(user) = path.and_then(|path| one_segment(path, "/users/", digit, true)) {
        // `new URLSearchParams(I).getAll("tab")`.
        let tabs: Vec<String> = query
            .map(|query| {
                query
                    .trim_start_matches('?')
                    .split('&')
                    .filter_map(|pair| {
                        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                        let key = decode_lenient(&key.replace('+', " "));
                        (key == "tab").then(|| decode_lenient(&value.replace('+', " ")))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if tabs.len() == 1 && tabs[0] == "wishlist" {
            push(Kind::UserWishlist, format!("{WISHLIST_PREFIX}{user}"));
        } else {
            push(Kind::UserProfile, user.to_owned());
        }
    }
    if let Some(app) = conjure_application(&url) {
        push(Kind::ConjureProject, app);
    }
    if path == Some("/shop") {
        let parsed = parsed_query.as_deref();
        let tab = parsed.and_then(|query| lookup(query, "tab"));
        let application = parsed
            .and_then(|query| lookup(query, "applicationId"))
            .and_then(Value::as_str);
        let skus = match (tab.and_then(Value::as_str), application) {
            (Some(GAME_SHOPS_TAB), Some(_)) => {
                let sku = parsed
                    .and_then(|query| lookup(query, "skuId"))
                    .and_then(Value::as_str);
                storefront_skus(sku, parsed)
            }
            _ => Vec::new(),
        };
        match application {
            Some(application) if !skus.is_empty() => {
                push(
                    Kind::SocialLayerStorefrontApp,
                    storefront_code(&skus, application),
                );
            }
            _ => {
                // `${n ?? ""}`: an array joins with commas, a `null` reads as nothing.
                let tab = match tab {
                    None | Some(Value::Null) => String::new(),
                    Some(Value::One(text)) => text.clone(),
                    Some(Value::Many(items)) => items
                        .iter()
                        .map(|item| item.clone().unwrap_or_default())
                        .collect::<Vec<_>>()
                        .join(","),
                };
                let sku = item_sku(url.hash.as_deref()).unwrap_or_default();
                push(Kind::CollectiblesShop, format!("{tab}-{sku}"));
            }
        }
    }
}

/// `ei`: a text's coded links, each code once, at most ten.
pub fn coded_links_in(content: &str) -> Vec<CodedLink> {
    // A link needs `//` or `www.` (`I`), which `q` only adds before a bare host: text with none of them has no links,
    // and messages are read every frame.
    if !content.contains("//")
        && !content.to_ascii_lowercase().contains("www.")
        && !BARE_HOSTS.iter().any(|host| content.contains(host))
    {
        return Vec::new();
    }
    let mut seen: Vec<String> = Vec::new();
    let mut links: Vec<CodedLink> = Vec::new();
    for url in candidate_urls(content) {
        link_types(&url, &mut |kind, code| {
            if !seen.contains(&code) {
                seen.push(code.clone());
                links.push(CodedLink {
                    kind,
                    code,
                    url: url.clone(),
                });
            }
        });
    }
    links.truncate(LIMIT);
    links
}

/// `codedLinks` of a message record (module 432005).
pub fn message_coded_links(kind: u64, content: &str) -> Vec<CodedLink> {
    if NON_PARSED.contains(&kind) {
        Vec::new()
    } else {
        coded_links_in(content)
    }
}

/// `br`: the first coded link of a text (what a click on a link checks, module 697794).
pub fn first_coded_link(content: &str) -> Option<CodedLink> {
    coded_links_in(content).into_iter().next()
}

/// The single-segment code of a short host link (`N` on `discord.gg`, `discord.new` or `discord.gift`), for the router.
pub fn short_host_code(url: &str, host: &str) -> Option<String> {
    let parsed = parse_url(url);
    let path = remaining_path(host, &parsed)?;
    single_segment(&path).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(text: &str) -> Vec<(Kind, String)> {
        coded_links_in(text)
            .into_iter()
            .map(|link| (link.kind, link.code))
            .collect()
    }

    #[test]
    fn invite_links_on_every_host() {
        assert_eq!(
            links(
                "https://discord.gg/abc-1 http://discord.com/invite/Def2 https://canary.discord.com/invite/x3"
            ),
            vec![
                (Kind::Invite, "abc-1".to_owned()),
                (Kind::Invite, "Def2".to_owned()),
                (Kind::Invite, "x3".to_owned()),
            ]
        );
        // `www.` is dropped from the host; ptb and discordapp count.
        assert_eq!(
            links(
                "https://www.discord.gg/w1 https://ptb.discord.com/invite/p1/ https://discordapp.com/invite/d1."
            ),
            vec![
                (Kind::Invite, "w1".to_owned()),
                (Kind::Invite, "p1".to_owned()),
                (Kind::Invite, "d1".to_owned()),
            ]
        );
    }

    #[test]
    fn bare_hosts_need_the_start_or_whitespace() {
        assert_eq!(
            links("discord.gg/start"),
            vec![(Kind::Invite, "start".to_owned())]
        );
        assert_eq!(
            links("join\tdiscord.gg/tab"),
            vec![(Kind::Invite, "tab".to_owned())]
        );
        assert_eq!(
            links("join\ndiscord.gg/line"),
            vec![(Kind::Invite, "line".to_owned())]
        );
        // Glued to a word: no scheme is added, and the URL rule needs one (or `www.`).
        assert!(links("xdiscord.gg/glued").is_empty());
        assert!(links("(discord.gg/paren)").is_empty());
        // `q` is case sensitive.
        assert!(links("Discord.gg/upper").is_empty());
        // `www.` alone is matched but `url.parse` finds no host in it.
        assert!(links("www.discord.gg/nohost").is_empty());
        // Right after a scheme the link is already whole.
        assert_eq!(
            links("https://discord.gg/after"),
            vec![(Kind::Invite, "after".to_owned())]
        );
    }

    #[test]
    fn host_case_and_port() {
        // The parser lowercases the host; a port keeps it from matching.
        assert_eq!(
            links("https://DISCORD.GG/Up"),
            vec![(Kind::Invite, "Up".to_owned())]
        );
        assert!(links("https://discord.gg:443/port").is_empty());
    }

    #[test]
    fn invite_keys_keep_event_channel_and_message() {
        assert_eq!(
            links("https://discord.gg/abc?event=1234567890123456789"),
            vec![(Kind::Invite, "abc?event=1234567890123456789".to_owned())]
        );
        assert_eq!(
            links(
                "https://discord.com/invite/abc?message=12345678901234567&channel=12345678901234568&utm=x"
            ),
            vec![(
                Kind::Invite,
                "abc?channel=12345678901234568&message=12345678901234567".to_owned()
            )]
        );
        // A channel that is not a snowflake drops the message too.
        assert_eq!(
            links("https://discord.gg/abc?channel=12&message=12345678901234567"),
            vec![(Kind::Invite, "abc".to_owned())]
        );
        let key = split_invite_key("abc?event=99&channel=12345678901234568");
        assert_eq!(key.base_code, "abc");
        assert_eq!(key.event.as_deref(), Some("99"));
        assert_eq!(key.channel.as_deref(), Some("12345678901234568"));
        assert_eq!(key.message, None);
    }

    #[test]
    fn a_backslash_in_an_invite_skips_the_whole_link() {
        // The parser turns `\` into `/`, so the path still matches, but `ei` skips the link (`continue`), even the
        // event part a later rule would have found.
        assert!(links(r"https://discord.gg\abc").is_empty());
        assert!(links(r"https://discord.com/invite\abc").is_empty());
    }

    #[test]
    fn templates_on_both_hosts() {
        assert_eq!(
            links("https://discord.new/Tmpl1 https://discord.com/template/Tmpl2"),
            vec![
                (Kind::Template, "Tmpl1".to_owned()),
                (Kind::Template, "Tmpl2".to_owned()),
            ]
        );
        // Only http(s) links reach the parser with a host: `ftp://` matches as `//host`, which has none.
        assert!(links("ftp://discord.new/Tmpl3").is_empty());
        assert!(links("ftp://discord.gg/inv").is_empty());
    }

    #[test]
    fn channel_message_and_event_links() {
        assert_eq!(
            links("https://discord.com/channels/111/222 https://discord.com/channels/@me/333"),
            vec![
                (Kind::ChannelLink, "111/222".to_owned()),
                (Kind::ChannelLink, "@me/333".to_owned()),
            ]
        );
        // A message link is not a channel link.
        assert!(links("https://discord.com/channels/111/222/333").is_empty());
        assert_eq!(
            links("https://discord.com/events/111/222 https://discord.com/events/111/223/444"),
            vec![
                (Kind::Event, "111-222".to_owned()),
                (Kind::Event, "111-223-444".to_owned()),
            ]
        );
        // The event rule reads the path of any host, and needs the event id.
        assert_eq!(
            links("https://example.com/events/5/6"),
            vec![(Kind::Event, "5-6".to_owned())]
        );
        assert!(links("https://discord.com/events/111/").is_empty());
    }

    #[test]
    fn app_links() {
        assert_eq!(
            links(concat!(
                "https://discord.com/application-directory/123 ",
                "https://discord.com/discovery/applications/124/privacy ",
                "https://discord.com/application-directory/125/store ",
                "https://discord.com/application-directory/126/store/777 ",
                "https://discord.com/activities/127",
            )),
            vec![
                (Kind::AppDirectoryProfile, "123".to_owned()),
                (Kind::AppDirectoryProfile, "124".to_owned()),
                (Kind::AppDirectoryStorefront, "125".to_owned()),
                (Kind::AppDirectoryStorefrontSku, "126:777".to_owned()),
                (Kind::ActivityBookmark, "127".to_owned()),
            ]
        );
    }

    #[test]
    fn oauth2_links_only_for_command_scopes() {
        assert_eq!(
            links("https://discord.com/oauth2/authorize?client_id=55&scope=applications.commands"),
            vec![(Kind::AppOauth2Link, "55".to_owned())]
        );
        assert_eq!(
            links("https://discord.com/oauth2/authorize?client_id=56"),
            vec![(Kind::AppOauth2Link, "56".to_owned())]
        );
        assert!(
            links(
                "https://discord.com/oauth2/authorize?client_id=57&scope=bot+applications.commands"
            )
            .is_empty()
        );
        assert!(
            links("https://discord.com/oauth2/authorize?scope=applications.commands").is_empty()
        );
        // No query, or one over a thousand characters, is not read.
        assert!(links("https://discord.com/oauth2/authorize").is_empty());
        let long = format!(
            "https://discord.com/oauth2/authorize?client_id=58&x={}",
            "a".repeat(1000)
        );
        assert!(links(&long).is_empty());
    }

    #[test]
    fn shop_links() {
        assert_eq!(
            links(concat!(
                "https://discord.com/channels/10/shop ",
                "https://discord.com/channels/10/shop/20 ",
                "https://discord.com/game-shop/30/40 ",
                "https://discord.com/channels/31/game-shop/5/41/slug ",
                "https://discord.com/game-shop/32?skuIds=1,2,2,x,3",
            )),
            vec![
                (Kind::ServerShop, "10".to_owned()),
                (Kind::GuildProduct, "10-20".to_owned()),
                (Kind::SocialLayerStorefront, "40-30".to_owned()),
                (Kind::SocialLayerStorefront, "41-31".to_owned()),
                (Kind::SocialLayerStorefront, "1,2,3-32".to_owned()),
            ]
        );
        // The unpaged channel path does not count.
        assert!(links("https://discord.com/channels/31/game-shop").is_empty());
        assert_eq!(
            links("https://discord.com/shop?tab=game-shops&applicationId=9&skuId=8"),
            vec![(Kind::SocialLayerStorefrontApp, "8-9".to_owned())]
        );
        assert_eq!(
            links("https://discord.com/shop?tab=orbs#itemSkuId=77"),
            vec![(Kind::CollectiblesShop, "orbs-77".to_owned())]
        );
        assert_eq!(
            links("https://discord.com/shop"),
            vec![(Kind::CollectiblesShop, "-".to_owned())]
        );
        // `/shop` must be the whole path.
        assert!(links("https://discord.com/shop/collection/1").is_empty());
        assert_eq!(
            split_storefront_code("1,2-9"),
            Some(("9".to_owned(), vec!["1".to_owned(), "2".to_owned()]))
        );
        assert_eq!(split_storefront_code("1-2-3"), None);
    }

    #[test]
    fn game_user_and_other_links() {
        assert_eq!(
            links(concat!(
                "https://discord.com/quests/1234 ",
                "https://discord.com/games/99/some-game ",
                "https://discord.com/game-servers/share/Ab_c-1 ",
                "https://discord.com/game-organizations/invite/org_1/ ",
                "https://discord.com/users/42 ",
                "https://discord.com/users/43?tab=wishlist ",
                "https://conjure.town/12345678901234567/my-app",
            )),
            vec![
                (Kind::QuestsEmbed, "1234".to_owned()),
                (Kind::GameProfile, "99".to_owned()),
                (Kind::GameServerShare, "Ab_c-1".to_owned()),
                (Kind::GameOrganizationInvite, "org_1".to_owned()),
                (Kind::UserProfile, "42".to_owned()),
                (Kind::UserWishlist, "wishlist-43".to_owned()),
                (Kind::ConjureProject, "12345678901234567".to_owned()),
            ]
        );
        // Two tabs are not a wishlist.
        assert_eq!(
            links("https://discord.com/users/44?tab=wishlist&tab=x"),
            vec![(Kind::UserProfile, "44".to_owned())]
        );
    }

    #[test]
    fn codes_are_kept_once_across_types_and_at_most_ten() {
        // The same code under two types: the first wins.
        assert_eq!(
            links("https://discord.gg/123 https://discord.com/users/123"),
            vec![(Kind::Invite, "123".to_owned())]
        );
        let many: Vec<String> = (0..12)
            .map(|n| format!("https://discord.gg/c{n}"))
            .collect();
        let found = links(&many.join(" "));
        assert_eq!(found.len(), 10);
        assert_eq!(found[9].1, "c9");
    }

    #[test]
    fn code_spans_and_blocks_hide_links() {
        assert!(links("`https://discord.gg/inline`").is_empty());
        assert!(links("``https://discord.gg/double``").is_empty());
        assert!(links("```\nhttps://discord.gg/block\n```").is_empty());
        // An unclosed block runs to the end.
        assert!(links("```https://discord.gg/open").is_empty());
        // A span closed by a longer run is not a span; the backticks then stay in the link, escaped by the parser.
        assert!(links("`https://discord.gg/x1``").is_empty());
        assert_eq!(
            links("`https://discord.gg/x2 ``"),
            vec![(Kind::Invite, "x2".to_owned())]
        );
        assert_eq!(
            links("`code` https://discord.gg/after"),
            vec![(Kind::Invite, "after".to_owned())]
        );
        assert_eq!(blank_code("a `b` c ```d``` e"), "a   c   e");
    }

    #[test]
    fn trailing_punctuation_is_trimmed() {
        assert_eq!(
            links("see https://discord.gg/abc."),
            vec![(Kind::Invite, "abc".to_owned())]
        );
        // `!` is not trimmed; it ends up in the query.
        assert_eq!(
            links("https://discord.gg/abc?!"),
            vec![(Kind::Invite, "abc".to_owned())]
        );
        assert_eq!(
            links("<https://discord.gg/angle>"),
            vec![(Kind::Invite, "angle".to_owned())]
        );
    }

    #[test]
    fn text_without_a_link_shape_is_skipped_early() {
        assert!(links("just words, no links here").is_empty());
        assert_eq!(
            links("see discord.gg/fast"),
            vec![(Kind::Invite, "fast".to_owned())]
        );
        assert_eq!(
            links("x https://discord.gg/slash"),
            vec![(Kind::Invite, "slash".to_owned())]
        );
        // `www.` alone matches the url rule but has no host; the result is the same either way.
        assert!(links("WWW.example.com/events/1/2").is_empty());
    }

    #[test]
    fn non_parsed_types_have_no_links() {
        for kind in NON_PARSED {
            assert!(message_coded_links(kind, "https://discord.gg/abc").is_empty());
        }
        assert_eq!(message_coded_links(0, "https://discord.gg/abc").len(), 1);
    }

    #[test]
    fn the_parser_reads_like_url_parse() {
        let url = parse_url("https://User@WWW.Discord.com:8080/a/b?c=d#e");
        assert_eq!(url.protocol.as_deref(), Some("https:"));
        assert_eq!(url.host.as_deref(), Some("www.discord.com:8080"));
        assert_eq!(url.pathname.as_deref(), Some("/a/b"));
        assert_eq!(url.search.as_deref(), Some("?c=d"));
        assert_eq!(url.query.as_deref(), Some("c=d"));
        assert_eq!(url.hash.as_deref(), Some("#e"));
        assert_eq!(
            parse_url("http://discord.gg").pathname.as_deref(),
            Some("/")
        );
        assert_eq!(parse_url("//discord.gg/x").host, None);
        assert_eq!(
            parse_url("https://discord.gg/a'b").pathname.as_deref(),
            Some("/a%27b")
        );
        assert_eq!(
            short_host_code("https://discord.gift/AbC", "discord.gift").as_deref(),
            Some("AbC")
        );
        assert_eq!(
            short_host_code("https://discord.gift/a/b", "discord.gift"),
            None
        );
    }
}
