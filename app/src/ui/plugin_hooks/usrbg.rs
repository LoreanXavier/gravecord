//! USRBG (Vencord `src/plugins/usrbg`, GPL-3.0-or-later): the list of USRBG banners (`usrbg.is-hardly.online/users`)
//! is fetched once while the plugin is on. A person on it gets that banner in profiles (`getPreviewBanner`), unless
//! they have a banner of their own and `nitroFirst` is the Nitro banner; with `voiceBackground` their voice tile
//! (without a camera or stream) shows it as its background, covering it, in place of the tile's colour. The plugin
//! card has "Get your own USRBG banner". Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use dc_state::Id;
use serde_json::Value;
use std::cell::{Cell, RefCell};

const ID: &str = "USRBG";
const API_URL: &str = "https://usrbg.is-hardly.online/users";
pub(in crate::ui) const KEY: &str = "plugin-usrbg";
const GUIDE: &str = "https://github.com/AutumnVN/usrbg#how-to-request-your-own-usrbg-banner";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Settings {
    nitro_first: bool,
    voice_background: bool,
}

fn settings_of(state: &State) -> Option<Settings> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
    Some(Settings {
        nitro_first: key("nitroFirst") != Value::Bool(false),
        voice_background: flag("voiceBackground"),
    })
}

fn current() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(settings_of)
}

/// `UsrbgApiReturn`.
#[derive(Clone, Debug, Default, PartialEq)]
struct Data {
    endpoint: String,
    bucket: String,
    prefix: String,
    users: std::collections::HashMap<Id, String>,
}

thread_local! {
    static DATA: RefCell<Option<Data>> = const { RefCell::new(None) };
    static ASKED: Cell<bool> = const { Cell::new(false) };
}

fn parse(body: &Value) -> Option<Data> {
    Some(Data {
        endpoint: body["endpoint"].as_str()?.to_owned(),
        bucket: body["bucket"].as_str()?.to_owned(),
        prefix: body["prefix"].as_str()?.to_owned(),
        users: body["users"]
            .as_object()?
            .iter()
            .filter_map(|(id, etag)| Some((id.parse().ok()?, etag.as_str()?.to_owned())))
            .collect(),
    })
}

/// `getImageUrl`.
fn image_url(data: &Data, user: Id) -> Option<String> {
    let etag = data.users.get(&user)?;
    Some(format!(
        "{}/{}/{}{user}?{etag}",
        data.endpoint, data.bucket, data.prefix
    ))
}

/// `patchBannerUrl`: the USRBG banner to show instead, if any (`has_banner`: the profile's own banner).
pub(in crate::ui) fn usrbg_banner(user: Id, has_banner: bool) -> Option<String> {
    let settings = current()?;
    if has_banner && settings.nitro_first {
        return None;
    }
    DATA.with(|data| {
        data.borrow()
            .as_ref()
            .and_then(|data| image_url(data, user))
    })
}

/// `getVoiceBackgroundStyles`: the banner behind a voice tile, with `voiceBackground`.
pub(in crate::ui) fn usrbg_voice_background(user: Id) -> Option<String> {
    current().filter(|settings| settings.voice_background)?;
    DATA.with(|data| {
        data.borrow()
            .as_ref()
            .and_then(|data| image_url(data, user))
    })
}

impl Root {
    /// `start`: the list is fetched the first time the plugin is seen on.
    pub(in crate::ui) fn usrbg_prepare(&mut self) {
        if ASKED.with(Cell::get) || current().is_none() {
            return;
        }
        ASKED.with(|asked| asked.set(true));
        let _ = self.requests.send(crate::net::Request::FetchPublicJson {
            key: KEY.to_owned(),
            url: API_URL.to_owned(),
        });
    }

    /// The list arrived (`res.ok`): banners of the profiles already open are asked for. True when the key was ours.
    pub(in crate::ui) fn usrbg_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if key != KEY {
            return false;
        }
        if let Some(data) = parse(body) {
            DATA.with(|cell| *cell.borrow_mut() = Some(data));
            let users: Vec<Id> = self
                .fetched
                .keys()
                .filter_map(|key| key.strip_prefix("profile:")?.parse().ok())
                .collect();
            for user in users {
                if let Some(body) = self.fetched.get(&format!("profile:{user}")).cloned()
                    && let Some((key, url)) =
                        super::super::profile_art::banner_source(user, self.guild, &body)
                {
                    self.request_image(key, url);
                }
            }
            cx.notify();
        }
        true
    }

    /// The plugin card's `settingsAboutComponent`: the link button to USRBG's guide.
    pub(in crate::ui) fn usrbg_settings(&self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        use super::super::{color, icon};
        use gpui::{
            InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
            div, px,
        };
        div()
            .id("usrbg-guide")
            .min_h(px(38.))
            .px(px(15.))
            .py(px(7.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .cursor_pointer()
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .text_color(color("--control-secondary-text-default"))
            .text_size(px(16.))
            .child("Get your own USRBG banner")
            .child(icon(
                "link_external_small",
                16.,
                "--control-secondary-text-default",
            ))
            .on_click(cx.listener(|_, _, _, cx| cx.open_url(GUIDE)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn banners_follow_the_list_and_nitro_first() {
        let data = parse(&json!({
            "endpoint": "https://e", "bucket": "b", "prefix": "p/",
            "users": {"5": "tag"}
        }))
        .unwrap();
        assert_eq!(image_url(&data, 5).as_deref(), Some("https://e/b/p/5?tag"));
        assert_eq!(image_url(&data, 6), None);
        let mut state = State::default();
        assert_eq!(settings_of(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(
            settings_of(&state),
            Some(Settings {
                nitro_first: true,
                voice_background: true
            })
        );
    }
}
