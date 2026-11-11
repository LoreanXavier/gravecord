//! `discord://` links and discord.com links that stay in the app.
//!
//! The official desktop shell (desktop core `handleOpenUrl`) takes a `discord:` URL, keeps only its pathname (the
//! host, `-` by convention, is ignored), resolves it against the web app's address (`getSanitizedPath`) and sends
//! `MAIN_WINDOW_PATH` with the path and the query to the page, whose router (module 375389, `eC` and the `eP` set)
//! shows the place. So the routes of a link are the routes of the web app. The shell's bootstrap reads the URL from its
//! arguments (`extractUrlFromArgs`: the argument after `--url --`, else the first one starting with `discord://`) and
//! registers the protocol under `HKCU\Software\Classes` (`installProtocol`).
//!
//! This module is pure: the parsing, the route table, the argument rule, the registry values and the file a second
//! launch leaves for the running instance. The places themselves open in `ui/deep_link_route.rs`.

use std::path::{Path, PathBuf};

/// `Constants.APP_PROTOCOL`.
pub const PROTOCOL: &str = "discord";

/// Hosts whose links the app opens itself (`isDiscordHostname` for the web app's own address).
const WEB_HOSTS: [&str; 4] = [
    "discord.com",
    "ptb.discord.com",
    "canary.discord.com",
    "discordapp.com",
];

/// A path and query inside the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub path: String,
    pub query: String,
}

/// What `new URL(path, WEBAPP_ENDPOINT).pathname` leaves of a path: absolute, with `.` and `..` resolved.
fn sanitize_path(raw: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    let mut path = String::from("/");
    path.push_str(&stack.join("/"));
    // A trailing slash is part of a URL's pathname.
    if raw.ends_with('/') && path.len() > 1 {
        path.push('/');
    }
    path
}

/// Splits what follows the host into the path and the query; the fragment is dropped, as `pathname` and `search` do.
fn split_tail(tail: &str) -> (&str, &str) {
    let tail = tail.split('#').next().unwrap_or_default();
    match tail.split_once('?') {
        Some((path, query)) => (path, query),
        None => (tail, ""),
    }
}

fn strip_scheme<'a>(url: &'a str, scheme: &str) -> Option<&'a str> {
    let head = url.get(..scheme.len())?;
    head.eq_ignore_ascii_case(scheme)
        .then(|| &url[scheme.len()..])
}

/// `handleOpenUrl`: a `discord:` URL. Any host is accepted (`discord://-/...`, `discord:///...`, `discord:/...`).
pub fn parse_protocol(url: &str) -> Option<Link> {
    let rest = strip_scheme(url.trim(), "discord:")?;
    let tail = match rest.strip_prefix("//") {
        Some(after) => after.find(['/', '?', '#']).map_or("", |end| &after[end..]),
        None => rest,
    };
    let (path, query) = split_tail(tail);
    Some(Link {
        path: sanitize_path(path),
        query: query.to_owned(),
    })
}

/// A link to the web app: `https://discord.com/channels/...` on discord.com, ptb, canary or discordapp.com.
pub fn parse_web(url: &str) -> Option<Link> {
    let url = url.trim();
    let rest = strip_scheme(url, "https://").or_else(|| strip_scheme(url, "http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = host
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !WEB_HOSTS.contains(&host.as_str()) {
        return None;
    }
    let (path, query) = split_tail(&rest[end..]);
    Some(Link {
        path: sanitize_path(path),
        query: query.to_owned(),
    })
}

/// Either kind of link.
pub fn parse(url: &str) -> Option<Link> {
    parse_protocol(url).or_else(|| parse_web(url))
}

/// Where a link leads. Ids are the numbers of the path; names the web router matches literally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// `/`, `/app`, `/channels/@me`: Home.
    Home,
    /// `/channels/<guild>`.
    Guild(u64),
    /// `/channels/<guild or @me>/<channel>[/<message>]`.
    Channel {
        guild: Option<u64>,
        channel: u64,
        message: Option<u64>,
    },
    /// `/channels/<guild>/<channel>/threads/<thread>[/<message>]`.
    Thread {
        guild: u64,
        channel: u64,
        thread: u64,
        message: Option<u64>,
    },
    /// `/channels/<guild>/<name>`: the guild's `shop`, `boosts`, `@home`, `channel-browser`, `onboarding`...
    GuildPage { guild: u64, name: String },
    /// `/users/<id>`.
    User(u64),
    /// `/invite/<code>`, `/app/invite-with-guild-onboarding/<code>`.
    Invite(String),
    /// `/gifts/<code>`, `/app/gifts/<code>`.
    Gift(String),
    /// `/template/<code>`.
    Template(String),
    /// `/settings/<section>[/<subsection>]`.
    Settings {
        section: String,
        subsection: Option<String>,
    },
    /// `/guilds/<guild>/settings[/<section>[/<subsection>]]`.
    GuildSettings { guild: u64, section: Option<String> },
    /// `/guilds/settings[/<section>]`: the picker of which server to open the settings of.
    PickGuildSettings { section: Option<String> },
    /// `/guilds/<guild>/premium-guild-subscriptions`.
    BoostMarketing(u64),
    /// `/guilds/create`.
    CreateGuild,
    /// `/guild-discovery`, `/discovery`, `/discovery/servers`, `/discovery/games`.
    DiscoverServers,
    /// `/discovery/applications`.
    DiscoverApps,
    /// `/discovery/applications/<id>`.
    DiscoverApp(u64),
    /// `/store`, `/store/skus/...`: Nitro.
    Store,
    /// `/shop`, `/shop/collection/<id>`.
    Shop,
    /// `/library` and its pages.
    Library,
    /// `/quest-home`, `/discovery/quests`, `/quests/<id>`, `/quest-preview/<id>`.
    Quests(Option<String>),
    /// `/events/<guild>/<event>`.
    Event { guild: u64, event: u64 },
    /// `/message-requests`.
    MessageRequests,
    /// `/family-center`.
    FamilyCenter,
    /// Anything else: the web app's own router would show its own page.
    Other(String),
}

fn id(segment: &str) -> Option<u64> {
    segment.parse().ok()
}

/// The web router's table (`eC`), for the places a desktop link can lead to.
pub fn route(link: &Link) -> Route {
    let path = link.path.trim_matches('/');
    let segments: Vec<&str> = if path.is_empty() {
        Vec::new()
    } else {
        path.split('/').collect()
    };
    let other = || Route::Other(link.path.clone());
    match segments.as_slice() {
        [] | ["app"] | ["login"] => Route::Home,
        ["channels"] | ["channels", "@me"] => Route::Home,
        ["channels", guild] => match id(guild) {
            Some(guild) => Route::Guild(guild),
            // `@favorites`, `@inbox`...: lists of Home.
            None if guild.starts_with('@') => Route::Home,
            None => other(),
        },
        ["channels", guild, "@home"] => id(guild).map_or_else(other, |guild| Route::GuildPage {
            guild,
            name: "@home".to_owned(),
        }),
        ["channels", guild, channel, rest @ ..] => {
            let guild = if *guild == "@me" {
                None
            } else if let Some(guild) = id(guild) {
                Some(guild)
            } else {
                return other();
            };
            let Some(channel_id) = id(channel) else {
                // Only a server has named pages.
                return match guild {
                    Some(guild) => Route::GuildPage {
                        guild,
                        name: (*channel).to_owned(),
                    },
                    None => other(),
                };
            };
            match rest {
                [] => Route::Channel {
                    guild,
                    channel: channel_id,
                    message: None,
                },
                ["threads", thread] | ["threads", thread, _] => {
                    let (Some(guild), Some(thread)) = (guild, id(thread)) else {
                        return other();
                    };
                    Route::Thread {
                        guild,
                        channel: channel_id,
                        thread,
                        message: rest.get(2).and_then(|message| id(message)),
                    }
                }
                [message] => match id(message) {
                    Some(message) => Route::Channel {
                        guild,
                        channel: channel_id,
                        message: Some(message),
                    },
                    None => other(),
                },
                _ => other(),
            }
        }
        ["users", user] => id(user).map_or_else(other, Route::User),
        ["invite", code, ..] | ["app", "invite-with-guild-onboarding", code] => {
            Route::Invite((*code).to_owned())
        }
        ["gifts", code, ..] | ["app", "gifts", code] => Route::Gift((*code).to_owned()),
        ["template", code, ..] => Route::Template((*code).to_owned()),
        ["settings", section] => Route::Settings {
            section: (*section).to_owned(),
            subsection: None,
        },
        ["settings", section, subsection, ..] => Route::Settings {
            section: (*section).to_owned(),
            subsection: Some((*subsection).to_owned()),
        },
        ["settings"] => other(),
        ["guilds", "create"] => Route::CreateGuild,
        ["guilds", "settings"] => Route::PickGuildSettings { section: None },
        ["guilds", "settings", section, ..] => Route::PickGuildSettings {
            section: Some((*section).to_owned()),
        },
        ["guilds", guild, "settings"] => {
            id(guild).map_or_else(other, |guild| Route::GuildSettings {
                guild,
                section: None,
            })
        }
        ["guilds", guild, "settings", section, ..] => {
            id(guild).map_or_else(other, |guild| Route::GuildSettings {
                guild,
                section: Some((*section).to_owned()),
            })
        }
        ["guilds", guild, "premium-guild-subscriptions"] => {
            id(guild).map_or_else(other, Route::BoostMarketing)
        }
        ["guild-discovery"] | ["discovery"] | ["discovery", "servers"] | ["discovery", "games"] => {
            Route::DiscoverServers
        }
        ["discovery", "quests"] => Route::Quests(None),
        ["discovery", "applications"] | ["discovery", "applications", "search"] => {
            Route::DiscoverApps
        }
        ["discovery", "applications", app, ..] => id(app).map_or_else(other, Route::DiscoverApp),
        ["store", ..] => Route::Store,
        ["shop", ..] => Route::Shop,
        ["library", ..] => Route::Library,
        ["quest-home"] => Route::Quests(None),
        ["quests", quest] | ["quest-preview", quest] => Route::Quests(Some((*quest).to_owned())),
        ["events", guild, event] => match (id(guild), id(event)) {
            (Some(guild), Some(event)) => Route::Event { guild, event },
            _ => other(),
        },
        ["message-requests"] => Route::MessageRequests,
        ["family-center"] => Route::FamilyCenter,
        _ => other(),
    }
}

/// The URL a launch carries (`extractUrlFromArgs`): the argument after `--url --`, else the first argument after the
/// program that starts with `discord://`.
pub fn url_from_args<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    let args: Vec<String> = args.into_iter().collect();
    if let Some(at) = args.iter().position(|arg| arg == "--url") {
        let separator = args.iter().position(|arg| arg == "--")?;
        if separator < at {
            return None;
        }
        return args.get(separator + 1).cloned();
    }
    let prefix = format!("{PROTOCOL}://");
    args.iter()
        .skip(1)
        .find(|arg| arg.to_ascii_lowercase().starts_with(&prefix))
        .cloned()
}

/// One registry value of the protocol registration: the key under `HKEY_CURRENT_USER`, the value name (`None` is the
/// key's default value) and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryValue {
    pub key: String,
    pub name: Option<&'static str>,
    pub data: String,
}

/// What the official `installProtocol` writes under `HKCU\Software\Classes\<protocol>` for the executable `exe`.
pub fn protocol_values(protocol: &str, exe: &Path) -> Vec<RegistryValue> {
    let root = format!(r"Software\Classes\{protocol}");
    let exe = exe.display();
    vec![
        RegistryValue {
            key: root.clone(),
            name: None,
            data: format!("URL:{protocol} Protocol"),
        },
        RegistryValue {
            key: root.clone(),
            name: Some("URL Protocol"),
            data: String::new(),
        },
        RegistryValue {
            key: format!(r"{root}\DefaultIcon"),
            name: None,
            data: format!("\"{exe}\",-1"),
        },
        RegistryValue {
            key: format!(r"{root}\shell\open\command"),
            name: None,
            data: format!("\"{exe}\" --url -- \"%1\""),
        },
    ]
}

/// The launch argument that asks the program to register the protocol and exit (for the installer).
pub const ARG_REGISTER: &str = "--register-protocol";

/// Writes the values of [`protocol_values`] for the running executable under `HKCU`. Never called from a capture run
/// or a test.
#[cfg(windows)]
pub fn register_protocol() -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };
    use windows::core::PCWSTR;

    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain([0])
            .collect()
    }
    let exe = std::env::current_exe()?;
    for value in protocol_values(PROTOCOL, &exe) {
        let mut key = HKEY::default();
        let path = wide(&value.key);
        // SAFETY: the key path is a NUL-terminated wide string that outlives the call; the handle is closed below.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
        }
        .ok()
        .map_err(std::io::Error::other)?;
        let name = value.name.map(wide);
        let bytes: Vec<u8> = wide(&value.data)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        // SAFETY: `key` is open for writing; the name (or null for the default value) and the bytes outlive the call.
        let written = unsafe {
            RegSetValueExW(
                key,
                name.as_ref()
                    .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr())),
                None,
                REG_SZ,
                Some(&bytes),
            )
        };
        // SAFETY: closes the handle opened above.
        unsafe {
            let _ = RegCloseKey(key);
        }
        written.ok().map_err(std::io::Error::other)?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn register_protocol() -> std::io::Result<()> {
    Err(std::io::Error::other(
        "protocol registration is Windows only",
    ))
}

/// The file a second launch leaves for the running instance: the link it was started with.
const PENDING_FILE: &str = "pending-link.txt";
/// A link longer than this is not a link of Discord's.
const MAX_LINK: usize = 4096;

fn pending_path(dir: &Path) -> PathBuf {
    dir.join(PENDING_FILE)
}

/// Leaves `url` for the running instance, replacing a link it has not taken yet. The file is written whole under
/// another name and renamed over the real one, so the reader never sees half of it.
pub fn write_pending(dir: &Path, url: &str) -> std::io::Result<()> {
    if url.len() > MAX_LINK || url.contains(['\r', '\n']) || parse(url).is_none() {
        return Err(std::io::Error::other("not a link the app opens"));
    }
    let target = pending_path(dir);
    let scratch = dir.join(format!("{PENDING_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&scratch, url)?;
    std::fs::rename(&scratch, &target).inspect_err(|_| {
        let _ = std::fs::remove_file(&scratch);
    })
}

/// Takes the link a second launch left, if one is there and is a link the app opens.
pub fn take_pending(dir: &Path) -> Option<String> {
    let target = pending_path(dir);
    let text = std::fs::read_to_string(&target).ok()?;
    let _ = std::fs::remove_file(&target);
    let url = text.trim().to_owned();
    (url.len() <= MAX_LINK && parse(&url).is_some()).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(url: &str) -> Route {
        route(&parse(url).expect("a link of the app"))
    }

    #[test]
    fn a_protocol_link_keeps_its_path_and_query_and_drops_the_host_and_fragment() {
        let parsed = parse_protocol("discord://-/channels/1/2/3?x=1#frag").unwrap();
        assert_eq!(parsed.path, "/channels/1/2/3");
        assert_eq!(parsed.query, "x=1");
        assert_eq!(
            parse_protocol("DISCORD:///settings/voice").unwrap().path,
            "/settings/voice"
        );
        assert_eq!(parse_protocol("discord:/users/5").unwrap().path, "/users/5");
        assert_eq!(parse_protocol("discord://-").unwrap().path, "/");
        assert_eq!(
            parse_protocol("discord://-/a/../b/./c").unwrap().path,
            "/b/c"
        );
        assert_eq!(
            parse_protocol("discord://-/../../channels/@me")
                .unwrap()
                .path,
            "/channels/@me"
        );
        assert!(parse_protocol("https://discord.com/x").is_none());
        assert!(parse_protocol("discordx://-/x").is_none());
    }

    #[test]
    fn web_links_are_the_apps_only_on_discords_own_hosts() {
        assert_eq!(
            parse_web("https://canary.discord.com/channels/1/2?a=b").unwrap(),
            Link {
                path: "/channels/1/2".to_owned(),
                query: "a=b".to_owned()
            }
        );
        assert!(parse_web("http://discordapp.com/users/9").is_some());
        assert!(parse_web("https://discord.com.evil.example/channels/1/2").is_none());
        assert!(parse_web("https://user@discord.com:443/invite/abc").is_some());
        assert!(parse_web("https://example.com/channels/1/2").is_none());
        assert!(parse("ftp://discord.com/x").is_none());
    }

    #[test]
    fn channel_links_route_to_channels_dms_threads_and_messages() {
        assert_eq!(
            link("discord://-/channels/10/20"),
            Route::Channel {
                guild: Some(10),
                channel: 20,
                message: None
            }
        );
        assert_eq!(
            link("discord://-/channels/10/20/30"),
            Route::Channel {
                guild: Some(10),
                channel: 20,
                message: Some(30)
            }
        );
        assert_eq!(
            link("discord://-/channels/@me/20"),
            Route::Channel {
                guild: None,
                channel: 20,
                message: None
            }
        );
        assert_eq!(
            link("discord://-/channels/10/20/threads/40/50"),
            Route::Thread {
                guild: 10,
                channel: 20,
                thread: 40,
                message: Some(50)
            }
        );
        assert_eq!(link("discord://-/channels/10"), Route::Guild(10));
        assert_eq!(link("discord://-/channels/@me"), Route::Home);
        assert_eq!(link("discord://-/channels/@favorites"), Route::Home);
        assert_eq!(
            link("discord://-/channels/10/shop"),
            Route::GuildPage {
                guild: 10,
                name: "shop".to_owned()
            }
        );
        assert_eq!(
            link("discord://-/channels/10/@home"),
            Route::GuildPage {
                guild: 10,
                name: "@home".to_owned()
            }
        );
        assert!(matches!(link("discord://-/channels/x/20"), Route::Other(_)));
    }

    #[test]
    fn the_other_places_of_the_table_route() {
        assert_eq!(link("discord://-/users/77"), Route::User(77));
        assert_eq!(
            link("discord://-/invite/abc"),
            Route::Invite("abc".to_owned())
        );
        assert_eq!(
            link("https://discord.com/app/invite-with-guild-onboarding/xyz"),
            Route::Invite("xyz".to_owned())
        );
        assert_eq!(link("discord://-/gifts/g1"), Route::Gift("g1".to_owned()));
        assert_eq!(
            link("discord://-/template/t1"),
            Route::Template("t1".to_owned())
        );
        assert_eq!(
            link("discord://-/settings/voice"),
            Route::Settings {
                section: "voice".to_owned(),
                subsection: None
            }
        );
        assert_eq!(
            link("discord://-/settings/account/account-standing"),
            Route::Settings {
                section: "account".to_owned(),
                subsection: Some("account-standing".to_owned())
            }
        );
        assert_eq!(
            link("discord://-/guilds/5/settings/roles"),
            Route::GuildSettings {
                guild: 5,
                section: Some("roles".to_owned())
            }
        );
        assert_eq!(
            link("discord://-/guilds/5/settings"),
            Route::GuildSettings {
                guild: 5,
                section: None
            }
        );
        assert_eq!(
            link("discord://-/guilds/settings/roles"),
            Route::PickGuildSettings {
                section: Some("roles".to_owned())
            }
        );
        assert_eq!(
            link("discord://-/guilds/5/premium-guild-subscriptions"),
            Route::BoostMarketing(5)
        );
        assert_eq!(link("discord://-/guilds/create"), Route::CreateGuild);
        assert_eq!(link("discord://-/guild-discovery"), Route::DiscoverServers);
        assert_eq!(
            link("discord://-/discovery/servers"),
            Route::DiscoverServers
        );
        assert_eq!(
            link("discord://-/discovery/applications"),
            Route::DiscoverApps
        );
        assert_eq!(
            link("discord://-/discovery/applications/12"),
            Route::DiscoverApp(12)
        );
        assert_eq!(link("discord://-/store"), Route::Store);
        assert_eq!(link("discord://-/shop/collection/3"), Route::Shop);
        assert_eq!(link("discord://-/library"), Route::Library);
        assert_eq!(link("discord://-/quest-home"), Route::Quests(None));
        assert_eq!(
            link("discord://-/quests/q9"),
            Route::Quests(Some("q9".to_owned()))
        );
        assert_eq!(
            link("discord://-/events/5/6"),
            Route::Event { guild: 5, event: 6 }
        );
        assert_eq!(link("discord://-/message-requests"), Route::MessageRequests);
        assert_eq!(link("discord://-/family-center"), Route::FamilyCenter);
        assert_eq!(link("discord://-/"), Route::Home);
        assert!(matches!(
            link("discord://-/nowhere/at/all"),
            Route::Other(_)
        ));
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_launch_url_is_read_as_the_official_does() {
        assert_eq!(
            url_from_args(args(&[
                "gravecord.exe",
                "--url",
                "--",
                "discord://-/channels/1/2"
            ])),
            Some("discord://-/channels/1/2".to_owned())
        );
        // `--url` without a separator before it, or with nothing after: no link, and no fallback scan.
        assert_eq!(
            url_from_args(args(&["g.exe", "--url", "discord://-/x"])),
            None
        );
        assert_eq!(
            url_from_args(args(&["g.exe", "--", "--url", "discord://-/x"])),
            None
        );
        assert_eq!(url_from_args(args(&["g.exe", "--url", "--"])), None);
        assert_eq!(
            url_from_args(args(&["g.exe", "--start-inactive", "Discord://-/users/3"])),
            Some("Discord://-/users/3".to_owned())
        );
        assert_eq!(url_from_args(args(&["discord://-/users/3"])), None);
        assert_eq!(url_from_args(args(&["g.exe", "--start-minimized"])), None);
    }

    #[test]
    fn the_registration_writes_the_official_values() {
        let exe = Path::new(r"C:\Apps\gravecord\gravecord.exe");
        let values = protocol_values("discord", exe);
        let find = |key: &str, name: Option<&str>| {
            values
                .iter()
                .find(|value| value.key == key && value.name == name)
                .map(|value| value.data.clone())
        };
        assert_eq!(values.len(), 4);
        assert_eq!(
            find(r"Software\Classes\discord", None).as_deref(),
            Some("URL:discord Protocol")
        );
        assert_eq!(
            find(r"Software\Classes\discord", Some("URL Protocol")).as_deref(),
            Some("")
        );
        assert_eq!(
            find(r"Software\Classes\discord\DefaultIcon", None).as_deref(),
            Some(r#""C:\Apps\gravecord\gravecord.exe",-1"#)
        );
        assert_eq!(
            find(r"Software\Classes\discord\shell\open\command", None).as_deref(),
            Some(r#""C:\Apps\gravecord\gravecord.exe" --url -- "%1""#)
        );
    }

    #[test]
    fn a_pending_link_is_taken_once_and_only_when_it_is_a_link_of_the_app() {
        let dir = std::env::temp_dir().join(format!("gravecord-link-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(take_pending(&dir), None);
        write_pending(&dir, "discord://-/channels/1/2").unwrap();
        write_pending(&dir, "discord://-/users/9").unwrap();
        assert_eq!(take_pending(&dir).as_deref(), Some("discord://-/users/9"));
        assert_eq!(take_pending(&dir), None);
        assert!(write_pending(&dir, "https://example.com/").is_err());
        assert!(write_pending(&dir, "discord://-/x\nrm").is_err());
        std::fs::write(pending_path(&dir), "file:///c:/windows/system32/calc.exe").unwrap();
        assert_eq!(take_pending(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
