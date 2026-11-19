//! OpenInApp (Vencord `src/plugins/openInApp`, GPL-3.0-or-later): a link of Spotify, Steam, Epic Games, Tidal, Apple
//! Music, VRChat or Telegram opens in its app (`spotify://…`, `steam://openurl/…`, …) instead of the browser, each
//! behind its own switch, with the toast "Opened link in native app". A short link (`spotify.link`, `s.team`) is
//! resolved first (HEAD requests, five redirects at most) and, when it does not lead to a link of the app, opens in the
//! browser. On a profile, a Spotify or Steam connection opens the account in its app (`spotify:user:<id>`,
//! `steam://openurl/https://steamcommunity.com/profiles/<id>`). Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use gpui::Context;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const ID: &str = "OpenInApp";
/// A short link's resolution comes back under this key: `<prefix><original url>`.
pub(in crate::ui) const KEY: &str = "plugin-open-in-app:";

struct Rule {
    key: &'static str,
    pattern: &'static str,
    short: Option<&'static str>,
}

const RULES: [Rule; 7] = [
    Rule {
        key: "spotify",
        pattern: r"^https://open\.spotify\.com/(?:intl-[a-z]{2}/)?(track|album|artist|playlist|user|episode|prerelease)/(.+)(?:\?.+?)?$",
        short: Some(r"^https://spotify\.link/.+$"),
    },
    Rule {
        key: "steam",
        pattern: r"^https://(steamcommunity\.com|(?:help|store)\.steampowered\.com)/.+$",
        short: Some(r"^https://s.team/.+$"),
    },
    Rule {
        key: "epic",
        pattern: r"^https://store\.epicgames\.com/(.+)$",
        short: None,
    },
    Rule {
        key: "tidal",
        pattern: r"^https://(?:listen\.)?tidal\.com/(?:browse/)?(track|album|artist|playlist|user|video|mix)/([a-f0-9-]+).*",
        short: None,
    },
    Rule {
        key: "itunes",
        pattern: r"^https://(?:geo\.)?music\.apple\.com/([a-z]{2}/)?(album|artist|playlist|song|curator)/([^/?#]+)/?([^/?#]+)?(?:\?.*)?(?:#.*)?$",
        short: None,
    },
    Rule {
        key: "vrcx",
        pattern: r"^https://vrchat.com/home/(user|avatar|world|group)/(.+)$",
        short: None,
    },
    Rule {
        key: "telegram",
        pattern: r"^https://t\.me/([a-zA-Z0-9_]+)$",
        short: None,
    },
];

fn compiled() -> &'static Vec<(Regex, Option<Regex>)> {
    static ALL: OnceLock<Vec<(Regex, Option<Regex>)>> = OnceLock::new();
    ALL.get_or_init(|| {
        RULES
            .iter()
            .map(|rule| {
                (
                    Regex::new(rule.pattern).expect("rule pattern"),
                    rule.short
                        .map(|short| Regex::new(short).expect("short pattern")),
                )
            })
            .collect()
    })
}

/// `rule.replace` of the rule at `index` on a matched URL.
fn rewrite(index: usize, url: &str) -> Option<String> {
    let (pattern, _) = &compiled()[index];
    let caps = pattern.captures(url)?;
    let group = |n: usize| caps.get(n).map_or("", |m| m.as_str());
    Some(match RULES[index].key {
        "spotify" => format!("spotify://{}/{}", group(1), group(2)),
        "steam" => format!("steam://openurl/{url}"),
        "epic" => format!("com.epicgames.launcher://store/{}", group(1)),
        "tidal" => format!("tidal://{}/{}", group(1), group(2)),
        "itunes" => {
            if caps.get(4).is_some() {
                format!(
                    "itunes://music.apple.com/us/{}/{}/{}",
                    group(2),
                    group(3),
                    group(4)
                )
            } else {
                format!("itunes://music.apple.com/us/{}/{}", group(2), group(3))
            }
        }
        "vrcx" => format!("vrcx://{}/{}", group(1), group(2)),
        _ => format!("tg://resolve?domain={}", group(1)),
    })
}

/// The switches that are on, while the plugin is.
fn switches(state: &State) -> Option<Vec<bool>> {
    let key = |name: &str| state.value(ID, name) != Value::Bool(false);
    // In `RULES`' order.
    state.is_on(ID).then(|| {
        vec![
            key("spotify"),
            key("steam"),
            key("epic"),
            key("tidal"),
            key("itunes"),
            key("vrcx"),
            key("telegram"),
        ]
    })
}

fn current() -> Option<Vec<bool>> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(switches)
}

/// What a link does under the plugin: open the app's address, wait for a short link, or nothing.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    App(String),
    Resolve,
    None,
}

fn plan(on: &[bool], url: &str) -> Plan {
    for (index, (pattern, short)) in compiled().iter().enumerate() {
        if !on[index] {
            continue;
        }
        if short.as_ref().is_some_and(|short| short.is_match(url)) {
            return Plan::Resolve;
        }
        if pattern.is_match(url)
            && let Some(app) = rewrite(index, url)
        {
            return Plan::App(app);
        }
    }
    Plan::None
}

impl Root {
    /// `handleLink`: true when the plugin took the link (opened it in its app, or is resolving a short link).
    pub(in crate::ui) fn open_in_app(&mut self, url: &str, cx: &mut Context<Self>) -> bool {
        let Some(on) = current() else {
            return false;
        };
        match plan(&on, url) {
            Plan::App(app) => {
                self.open_in_app_now(&app, cx);
                true
            }
            Plan::Resolve => {
                let _ = self.requests.send(crate::net::Request::ResolveRedirect {
                    key: format!("{KEY}{url}"),
                    url: url.to_owned(),
                });
                true
            }
            Plan::None => false,
        }
    }

    fn open_in_app_now(&mut self, app: &str, cx: &mut Context<Self>) {
        self.show_success_toast("Opened link in native app".to_owned(), cx);
        cx.open_url(app);
    }

    /// A short link was resolved: its app, or the browser (`window.open`). True when the key was ours.
    pub(in crate::ui) fn open_in_app_resolved(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(original) = key.strip_prefix(KEY) else {
            return false;
        };
        let url = body["url"].as_str().unwrap_or(original).to_owned();
        match current().map(|on| plan(&on, &url)) {
            Some(Plan::App(app)) => self.open_in_app_now(&app, cx),
            _ => cx.open_url(&url),
        }
        true
    }

    /// `handleAccountView`: a Spotify or Steam connection opens in its app. True when it did.
    pub(in crate::ui) fn open_in_app_account(
        &mut self,
        kind: &str,
        id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(on) = current() else {
            return false;
        };
        let at = |key: &str| RULES.iter().position(|rule| rule.key == key);
        let app = match kind {
            "spotify" if at("spotify").is_some_and(|index| on[index]) => {
                format!("spotify:user:{id}")
            }
            "steam" if at("steam").is_some_and(|index| on[index]) => {
                format!("steam://openurl/https://steamcommunity.com/profiles/{id}")
            }
            _ => return false,
        };
        cx.open_url(&app);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<bool> {
        vec![true; RULES.len()]
    }

    #[test]
    fn links_become_the_apps_addresses() {
        let on = all();
        assert_eq!(
            plan(&on, "https://open.spotify.com/track/abc"),
            Plan::App("spotify://track/abc".to_owned())
        );
        assert_eq!(
            plan(&on, "https://store.steampowered.com/app/1"),
            Plan::App("steam://openurl/https://store.steampowered.com/app/1".to_owned())
        );
        assert_eq!(
            plan(&on, "https://music.apple.com/us/album/name/123"),
            Plan::App("itunes://music.apple.com/us/album/name/123".to_owned())
        );
        assert_eq!(
            plan(&on, "https://t.me/someone"),
            Plan::App("tg://resolve?domain=someone".to_owned())
        );
        assert_eq!(plan(&on, "https://spotify.link/xyz"), Plan::Resolve);
        assert_eq!(plan(&on, "https://example.com/"), Plan::None);
        let mut off = all();
        off[0] = false;
        assert_eq!(plan(&off, "https://open.spotify.com/track/abc"), Plan::None);
    }

    #[test]
    fn every_switch_defaults_on() {
        let mut state = State::default();
        assert_eq!(switches(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(switches(&state), Some(all()));
    }
}
