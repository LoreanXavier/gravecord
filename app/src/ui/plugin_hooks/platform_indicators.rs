//! PlatformIndicators (Equicord `src/plugins/platformIndicators`, GPL-3.0-or-later): an icon per client a person is
//! signed in on (desktop, mobile, web, console, VR), coloured by that client's status, by their name in the member
//! list, in message headers and in profiles; with `colorMobileIndicator`, the phone in place of the status dot for
//! anyone on mobile, whatever their status. Not part of the pinned client.

use super::super::tooltip::{Body, Side, Tip};
use super::super::{Root, color};
use crate::plugins::State;
use dc_state::Id;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, px, svg};
use serde_json::Value;

const ID: &str = "PlatformIndicators";

/// Where the icons go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Place {
    MemberList,
    Message,
    Profile,
}

fn on_at(state: &State, place: Place) -> bool {
    let key = match place {
        Place::MemberList => "list",
        Place::Message => "messages",
        Place::Profile => "profiles",
    };
    state.is_on(ID) && state.value(ID, key) == Value::Bool(true)
}

/// `platformMap`, else the name capitalised.
pub(super) fn tooltip(platform: &str) -> String {
    match platform {
        "embedded" => "Embedded (Console or Game)".to_owned(),
        "vr" => "VR".to_owned(),
        _ => {
            let mut chars = platform.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

/// The icon file and its box: the console's per `ConsoleIcon`, unknown clients the desktop's.
fn icon_of(platform: &str, console: &str) -> (&'static str, f32, f32) {
    match platform {
        "mobile" => ("plugin_platform_mobile", 17., 17.),
        "web" => ("plugin_platform_web", 20., 20.),
        "vr" => ("plugin_platform_vr", 20., 20.),
        "embedded" => match console {
            "suncord" => ("plugin_platform_suncord", 24., 24.),
            "vencord" => ("plugin_platform_vencord", 20., 20.),
            _ => ("plugin_platform_embedded", 20., 20.),
        },
        _ => ("plugin_platform_desktop", 20., 20.),
    }
}

/// `useStatusFillColor`.
fn status_tint(status: &str) -> &'static str {
    match status {
        "idle" => "--icon-status-idle",
        "dnd" => "--icon-status-dnd",
        "online" => "--icon-status-online",
        _ => "--icon-status-offline",
    }
}

/// `isMobileOnline` with `colorMobileIndicator`: a mobile client at all, and no online desktop or VR one.
pub(super) fn mobile_any_in(clients: &Value) -> bool {
    !clients["mobile"].is_null() && clients["desktop"] != "online" && clients["vr"] != "online"
}

/// Whether the avatar status shows the phone for this person: as the pinned client (mobile and online), or with
/// PlatformIndicators' `colorMobileIndicator` for any status.
pub(in crate::ui) fn mobile_status(stores: &dc_state::Stores, user: Id, official: bool) -> bool {
    if official || crate::plugins::bypassed() {
        return official;
    }
    crate::plugins::with(|state| {
        state.is_on(ID)
            && state.value(ID, "colorMobileIndicator") == Value::Bool(true)
            && stores
                .presence
                .get(user, None)
                .is_some_and(|presence| mobile_any_in(&presence.clients))
    })
}

impl Root {
    /// The clients of a person and their statuses (`PresenceStore.getClientStatus`); for yourself the presence the
    /// account set, as this desktop client when the server sent no list (`useEnsureOwnStatus` reads the sessions).
    fn client_statuses(&self, user: Id) -> Vec<(String, String)> {
        let presence = self.stores.presence.get(user, None);
        let mut out: Vec<(String, String)> = presence
            .and_then(|presence| presence.clients.as_object())
            .map(|clients| {
                clients
                    .iter()
                    .filter_map(|(platform, status)| {
                        Some((platform.clone(), status.as_str()?.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        if out.is_empty() && me == Some(user) {
            let status = presence.map_or(self.stores.status.clone(), |presence| {
                presence.status.clone()
            });
            out.push(("desktop".to_owned(), status));
        }
        out
    }

    /// The icons of a person at `place`, or nothing (off there, a bot without `showBots`, no client known).
    pub(in crate::ui) fn platform_icons(
        &self,
        user: Id,
        place: Place,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if crate::plugins::bypassed() {
            return None;
        }
        let (show_bots, console) = crate::plugins::with(|state| {
            on_at(state, place).then(|| {
                (
                    state.value(ID, "showBots") == Value::Bool(true),
                    state
                        .value(ID, "ConsoleIcon")
                        .as_str()
                        .unwrap_or("equicord")
                        .to_owned(),
                )
            })
        })?;
        let bot = self.stores.users.get(&user).is_some_and(|known| known.bot);
        if bot && !show_bots {
            return None;
        }
        let clients = self.client_statuses(user);
        if clients.is_empty() {
            return None;
        }
        let small = matches!(place, Place::MemberList | Place::Profile);
        let icons = clients.into_iter().map(|(platform, status)| {
            let (file, width, height) = icon_of(&platform, &console);
            let shrink = if small { 3. } else { 0. };
            let picture = svg()
                .path(format!("icons/{file}.svg"))
                .w(px(width - shrink))
                .h(px(height - shrink))
                .text_color(color(status_tint(&status)));
            self.with_tip(
                format!("platform-{place:?}-{user}-{platform}"),
                Tip::body(Body::Text(tooltip(&platform).into()), Side::Top),
                div().child(picture),
                cx,
            )
            .into_any_element()
        });
        let row = div().flex().items_center().justify_center().gap(px(2.));
        let row = match place {
            Place::MemberList => row.ml(px(4.)),
            Place::Profile => row.mx(px(1.)).px(px(1.)),
            Place::Message => row.ml(px(4.)).mt(px(-1.)),
        };
        Some(row.children(icons).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tooltips_and_icons_follow_the_original() {
        assert_eq!(tooltip("desktop"), "Desktop");
        assert_eq!(tooltip("embedded"), "Embedded (Console or Game)");
        assert_eq!(tooltip("vr"), "VR");
        assert_eq!(icon_of("embedded", "suncord").0, "plugin_platform_suncord");
        assert_eq!(
            icon_of("something", "equicord").0,
            "plugin_platform_desktop"
        );
        assert_eq!(status_tint("dnd"), "--icon-status-dnd");
    }

    #[test]
    fn each_place_and_the_mobile_phone_have_their_setting() {
        let mut state = State::default();
        assert!(!on_at(&state, Place::Message));
        state.set_enabled(ID, true);
        assert!(
            on_at(&state, Place::Message)
                && on_at(&state, Place::MemberList)
                && on_at(&state, Place::Profile)
        );
        state.set_value(ID, "messages", &json!(false));
        assert!(!on_at(&state, Place::Message));
        assert!(mobile_any_in(&json!({"mobile": "idle"})));
        assert!(!mobile_any_in(
            &json!({"mobile": "idle", "desktop": "online"})
        ));
        assert!(!mobile_any_in(&json!({"web": "online"})));
    }
}
