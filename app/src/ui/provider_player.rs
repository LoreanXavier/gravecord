//! Provider players in chat (module 265705 `ee` with `x7` of module 693942): a video embed whose provider has no
//! proxied file (YouTube, TikTok, Twitch) shows its poster with Play and Open in browser; Play puts the provider's own
//! player in the embed's box (`autoplay=1&auto_play=1&pageType=2` for YouTube, `utm_source=discord.gg` for TikTok,
//! Twitch's `parent` list and `autoplay=true`). The player is a system web view (`dc_webhost::WebHost::player`) laid
//! over the box as `web_frames.rs` lays the Spotify frames: it follows the box as the chat scrolls, cut to the chat's
//! visible part, hides while a menu, popout or dialog is over it, and closes when the
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
        // Another embed's player closes: its view no longer follows the box.
        self.web_frames.painted.borrow_mut().unlink(ANCHOR);
        self.provider_player = Some(ProviderPlayer {
            key,
            url,
            web: None,
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

    /// Called every frame: opens, hides or closes the player from where its box was drawn in the last frame. Where
    /// the view sits is sent by the box's paint (`web_frames::Painted::place`), cut to the chat's visible part.
    pub(super) fn provider_player_frame(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.provider_player.is_none() {
            return;
        }
        let seen = self.web_frames.painted.borrow().last_place(ANCHOR);
        let covered = self.web_surfaces_covered();
        let painted = self.web_frames.painted.clone();
        let Some(player) = self.provider_player.as_mut() else {
            return;
        };
        let Some(placement) = seen else {
            // The box's paint hid the view already; it closes once the box stays gone.
            player.unseen += 1;
            if player.unseen > GRACE_FRAMES {
                painted.borrow_mut().unlink(ANCHOR);
                self.provider_player = None;
                cx.notify();
            }
            return;
        };
        player.unseen = 0;
        let visible = !covered;
        if player.web.is_none() {
            let Some(dir) = crate::storage::data_dir().map(|dir| dir.join("webview")) else {
                return;
            };
            match WebHost::player(window, dir, placement, &player.url, Some(REFERER)) {
                Ok(web) => {
                    web.set_visible(visible);
                    painted.borrow_mut().link(ANCHOR, web.placer(), placement);
                    player.web = Some(web);
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
        if let Some(web) = player.web.as_ref()
            && visible != player.visible
        {
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
