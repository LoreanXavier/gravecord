//! Provider players in chat (module 265705 `ee` with `x7` of module 693942): a video embed whose provider has no
//! proxied file (YouTube, TikTok, Twitch) shows its poster with Play and Open in browser; Play puts the provider's own
//! player in the embed's box (`autoplay=1&auto_play=1&pageType=2` for YouTube, `utm_source=discord.gg` for TikTok,
//! Twitch's `parent` list and `autoplay=true`). The player is a system web view (`dc_webhost::WebHost::player`) laid
//! over the box: it follows the box as the chat scrolls, hides while a menu or dialog is over it, and closes when the
//! box is no longer drawn (the message scrolled out of the list or the channel changed), as the official list unmounts
//! the frame. One player plays at a time; Play on another embed moves it there.
use super::Root;
use dc_webhost::WebHost;
use gpui::{Context, Window};

/// The anchor the playing embed's box records its bounds under.
pub(super) const ANCHOR: &str = "provider-player";
/// The page the official frame sits in, sent as the player's `Referer` (providers refuse players without one).
const REFERER: &str = "https://discord.com/";
/// Frames the box may go undrawn before the player closes (a frame or two can pass while a list re-lays out).
const GRACE_FRAMES: u32 = 6;

pub(super) struct ProviderPlayer {
    /// `embed:{message}:{index}` of the box that plays.
    pub key: String,
    url: String,
    web: Option<WebHost>,
    bounds: (f32, f32, f32, f32),
    visible: bool,
    unseen: u32,
}

/// `x7`: the provider's player address with its parameters.
pub(super) fn player_url(url: &str, provider: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return String::new();
    };
    let set = |parsed: &mut reqwest::Url, key: &str, value: &str| {
        let kept: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(k, _)| k != key)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let mut query = parsed.query_pairs_mut();
        query.clear();
        for (k, v) in kept {
            query.append_pair(&k, &v);
        }
        query.append_pair(key, value);
    };
    match provider {
        "YouTube" => {
            for (key, value) in [("autoplay", "1"), ("auto_play", "1"), ("pageType", "2")] {
                set(&mut parsed, key, value);
            }
        }
        "TikTok" => set(&mut parsed, "utm_source", "discord.gg"),
        "Twitch" => {
            let kept: Vec<(String, String)> = parsed
                .query_pairs()
                .filter(|(k, _)| k != "parent")
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            {
                let mut query = parsed.query_pairs_mut();
                query.clear();
                for (k, v) in kept {
                    query.append_pair(&k, &v);
                }
                for parent in [
                    "discord.co",
                    "discord.com",
                    "discordapp.com",
                    "ptb.discord.com",
                    "canary.discord.com",
                    "localhost",
                ] {
                    query.append_pair("parent", parent);
                }
            }
            set(&mut parsed, "autoplay", "true");
        }
        _ => {}
    }
    parsed.to_string()
}

impl Root {
    /// Play on a provider embed: the player opens on that box at the next frame.
    pub(super) fn provider_play(&mut self, key: String, url: String, cx: &mut Context<Self>) {
        if url.is_empty() {
            return;
        }
        self.provider_player = Some(ProviderPlayer {
            key,
            url,
            web: None,
            bounds: (0., 0., 0., 0.),
            visible: false,
            unseen: 0,
        });
        cx.notify();
    }

    pub(super) fn provider_playing(&self, key: &str) -> bool {
        self.provider_player
            .as_ref()
            .is_some_and(|player| player.key == key)
    }

    /// Something of the app's own is over the chat: the web view, which paints above the window, steps aside.
    fn provider_covered(&self) -> bool {
        self.context.open.is_some()
            || self.menu.is_some()
            || self.quick_switcher.is_some()
            || self.lightbox.is_some()
            || self.game_profile.is_some()
            || self.settings.is_some()
    }

    /// Called every frame: places, hides or closes the player from where its box was drawn last.
    pub(super) fn provider_player_frame(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.provider_player.is_none() {
            return;
        }
        let seen = self.tip_anchors.borrow_mut().remove(ANCHOR).map(|bounds| {
            (
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            )
        });
        let covered = self.provider_covered();
        let Some(player) = self.provider_player.as_mut() else {
            return;
        };
        let Some(bounds) = seen else {
            player.unseen += 1;
            if player.unseen > GRACE_FRAMES {
                self.provider_player = None;
                cx.notify();
            } else if let Some(web) = player.web.as_ref().filter(|_| player.visible) {
                web.set_visible(false);
                player.visible = false;
            }
            return;
        };
        player.unseen = 0;
        let visible = !covered;
        if player.web.is_none() {
            let Some(dir) = crate::storage::data_dir().map(|dir| dir.join("webview")) else {
                return;
            };
            match WebHost::player(window, dir, bounds, &player.url, Some(REFERER)) {
                Ok(web) => {
                    web.set_visible(visible);
                    player.web = Some(web);
                    player.bounds = bounds;
                    player.visible = visible;
                    tracing::info!("provider player opened");
                }
                Err(error) => {
                    tracing::warn!("provider player could not open: {error}");
                    self.provider_player = None;
                }
            }
            return;
        }
        let Some(web) = player.web.as_ref() else {
            return;
        };
        if bounds != player.bounds {
            web.set_bounds(bounds);
            player.bounds = bounds;
        }
        if visible != player.visible {
            web.set_visible(visible);
            player.visible = visible;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_get_the_official_parameters() {
        let youtube = player_url("https://www.youtube.com/embed/abc?start=5", "YouTube");
        assert!(youtube.contains("start=5") && youtube.contains("autoplay=1"));
        assert!(youtube.contains("auto_play=1") && youtube.contains("pageType=2"));
        let tiktok = player_url("https://www.tiktok.com/embed/v2/1", "TikTok");
        assert!(tiktok.ends_with("utm_source=discord.gg"));
        let twitch = player_url(
            "https://player.twitch.tv/?channel=x&parent=meta.tag",
            "Twitch",
        );
        assert!(!twitch.contains("meta.tag") && twitch.contains("parent=discord.com"));
        assert!(twitch.contains("autoplay=true"));
    }
}
