//! GlobalBadges (Equicord `src/equicordplugins/globalBadges`, GPL-3.0-or-later): the badges other client mods give
//! people (`<apiUrl>/users`, fetched while the plugin is on and every 30 minutes) lead a profile's badge row
//! (`BadgePosition.START`): each 20px picture round and at 90%, its tooltip the badge's text, and a right click its
//! menu: Copy Badge Name, Copy Badge Image Link. Vencord's and Equicord's own entries are left out, and each other
//! mod's behind its Show switch. The plugin card has "Join GlobalBadges Server". Not part of the pinned client.
//!
//! GAP: the badge directory entry a click opens in the original (the client mod's BadgeAPI fills Discord's badge
//! directory with its badges), the toolbox's "Refetch Global Badges", and the "Allow Host" notice (no content
//! security policy here). The Show switches are listed one by one, where the original edits them in one multi-select.

use super::super::Root;
use crate::plugins::State;
use dc_state::Id;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const ID: &str = "GlobalBadges";
pub(in crate::ui) const KEY: &str = "plugin-global-badges";
const INVITE_LINK: &str = "https://discord.gg/kwHCJPxp8t";
const REFRESH: Duration = Duration::from_secs(30 * 60);

/// `blockedMods`.
const BLOCKED: &[&str] = &["vencord", "equicord"];

/// A badge as the list gives it.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct GlobalBadge {
    pub image: String,
    pub tooltip: String,
    pub module: String,
}

thread_local! {
    /// The badge whose menu is open: its text and picture link.
    static MENU: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static BADGES: RefCell<HashMap<Id, Vec<GlobalBadge>>> = RefCell::new(HashMap::new());
    static LAST: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

fn api_url(state: &State) -> Option<String> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let base = key("apiUrl").as_str().unwrap_or_default().to_owned();
    if base.is_empty() {
        return None;
    }
    Some(if base.ends_with('/') {
        format!("{base}users")
    } else {
        format!("{base}/users")
    })
}

/// Whether badges of `module` are shown: not Vencord's or Equicord's, and its switch on (`conditionalMods`; an
/// unknown mod always).
fn shown(state: &State, module: &str) -> bool {
    if module.is_empty() || BLOCKED.contains(&module) {
        return false;
    }
    let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
    match module {
        "aero" => flag("showAero"),
        "velocity" => flag("showVelocity"),
        "badgevault" => flag("showCustom"),
        "nekocord" => flag("showNekocord"),
        "reviewdb" => flag("showReviewDB"),
        "aliucord" => flag("showAliucord"),
        "raincord" => flag("showRaincord"),
        "enmity" => flag("showEnmity"),
        "paicord" => flag("showPaicord"),
        "bunny" => flag("showBunny"),
        "goosemod" => flag("showGooseMod"),
        "replugged" => flag("showReplugged"),
        "betterdiscord" => flag("showBetterDiscord"),
        "vendroidenhanced" => flag("showVendroidEnhanced"),
        "revenge" => flag("showRevenge"),
        "record" => flag("showReCord"),
        _ => true,
    }
}

fn parse(body: &Value) -> HashMap<Id, Vec<GlobalBadge>> {
    body["users"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, badges)| {
            let list = badges
                .as_array()?
                .iter()
                .filter_map(|badge| {
                    Some(GlobalBadge {
                        image: badge["badge"].as_str()?.to_owned(),
                        tooltip: badge["tooltip"].as_str().unwrap_or_default().to_owned(),
                        module: badge["mod"].as_str().unwrap_or_default().to_owned(),
                    })
                })
                .collect();
            Some((id.parse().ok()?, list))
        })
        .collect()
}

/// `getGlobalBadges`: the badges shown for `user` (none while the plugin is off).
pub(in crate::ui) fn global_badges(user: Id) -> Vec<GlobalBadge> {
    if crate::plugins::bypassed() {
        return Vec::new();
    }
    crate::plugins::with(|state| {
        if !state.is_on(ID) {
            return Vec::new();
        }
        BADGES.with(|badges| {
            badges
                .borrow()
                .get(&user)
                .map(|list| {
                    list.iter()
                        .filter(|badge| shown(state, &badge.module))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        })
    })
}

impl Root {
    /// `start` and the 30 minute interval: the list is fetched while the plugin is on.
    pub(in crate::ui) fn global_badges_prepare(&mut self) {
        if crate::plugins::bypassed() {
            return;
        }
        let Some(url) = crate::plugins::with(api_url) else {
            LAST.with(|last| *last.borrow_mut() = None);
            return;
        };
        let due = LAST.with(|last| last.borrow().is_none_or(|at| at.elapsed() >= REFRESH));
        if !due {
            return;
        }
        LAST.with(|last| *last.borrow_mut() = Some(Instant::now()));
        let _ = self.requests.send(crate::net::Request::FetchPublicJson {
            key: KEY.to_owned(),
            url,
        });
    }

    /// The list arrived. True when the key was ours.
    pub(in crate::ui) fn global_badges_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if key != KEY {
            return false;
        }
        if body.is_object() {
            let parsed = parse(body);
            for badge in parsed.values().flatten() {
                self.request_image(format!("global-badge:{}", badge.image), badge.image.clone());
            }
            BADGES.with(|badges| *badges.borrow_mut() = parsed);
            cx.notify();
        }
        true
    }

    /// `BadgeContextMenu` at the pointer.
    fn global_badge_menu(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        name: String,
        link: String,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        MENU.with(|menu| *menu.borrow_mut() = Some((name, link)));
        self.open_context_menu(super::super::context_menu::Target::PluginBadge, at, cx);
    }

    /// The menu's rows: Copy Badge Name (when it has a text), Copy Badge Image Link.
    pub(in crate::ui) fn global_badge_items(&self) -> Vec<super::super::context_menu::Entry> {
        use super::super::context_menu::{Act, Entry, Row};
        let Some((name, link)) = MENU.with(|menu| menu.borrow().clone()) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if !name.is_empty() {
            items.push(Entry::Row(
                Row::new(Act::PluginCopyBadgeName, "Copy Badge Name").icon("menu_copy_text"),
            ));
        }
        if !link.is_empty() {
            items.push(Entry::Row(
                Row::new(Act::PluginCopyBadgeLink, "Copy Badge Image Link")
                    .icon("menu_copy_message_link"),
            ));
        }
        items
    }

    /// `copyWithToast`.
    pub(in crate::ui) fn global_badge_copy(&mut self, link: bool, cx: &mut gpui::Context<Self>) {
        let Some((name, url)) = MENU.with(|menu| menu.borrow().clone()) else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(if link {
            url
        } else {
            name
        }));
        self.show_success_toast("Copied to clipboard!".to_owned(), cx);
    }

    /// The plugin card's "Join GlobalBadges Server".
    pub(in crate::ui) fn global_badges_settings(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        use super::super::{color, icon};
        use gpui::{
            InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
            div, px,
        };
        div()
            .id("global-badges-join")
            .w_full()
            .h(px(50.))
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
            .child("Join GlobalBadges Server")
            .child(icon(
                "link_external_small",
                16.,
                "--control-secondary-text-default",
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.follow_link(INVITE_LINK, "Join GlobalBadges Server", cx)
            }))
            .into_any_element()
    }

    /// The badges before Discord's in a profile's row (`place` keeps each tooltip's key apart).
    pub(in crate::ui) fn global_badge_elements(
        &self,
        place: &'static str,
        user: Id,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        use super::super::tooltip::{Side, Tip};
        use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, div, img, px};
        global_badges(user)
            .into_iter()
            .enumerate()
            .map(|(index, badge)| {
                let picture = self
                    .images
                    .get(&format!("global-badge:{}", badge.image))
                    .cloned();
                let (name, link) = (badge.tooltip.clone(), badge.image.clone());
                let target = div()
                    .id(gpui::SharedString::from(format!(
                        "{place}-global-badge-{user}-{index}"
                    )))
                    .size(px(20.))
                    .rounded_full()
                    .overflow_hidden()
                    .children(picture.map(|picture| img(picture).size(px(20.)).rounded_full()))
                    .paint_scale(0.9, gpui::point(0.5, 0.5))
                    .on_mouse_up(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, window, cx| {
                            cx.stop_propagation();
                            this.global_badge_menu(
                                event.position,
                                name.clone(),
                                link.clone(),
                                window,
                                cx,
                            );
                        }),
                    );
                self.with_tip(
                    format!("{place}-global-badge-tip-{user}-{index}"),
                    Tip::body(
                        super::super::tooltip::Body::Text(badge.tooltip.into()),
                        Side::Top,
                    ),
                    target,
                    cx,
                )
                .into_any_element()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn badges_follow_the_switches_and_blocked_mods() {
        let parsed = parse(&json!({"users": {"5": [
            {"badge": "https://b/1.png", "tooltip": "One", "mod": "aero"},
            {"badge": "https://b/2.png", "tooltip": "Two", "mod": "vencord"},
            {"badge": "https://b/3.png", "tooltip": "Three", "mod": "other"}
        ]}}));
        assert_eq!(parsed[&5].len(), 3);
        let mut state = State::default();
        state.set_enabled(ID, true);
        assert!(shown(&state, "aero") && !shown(&state, "vencord") && shown(&state, "other"));
        state.set_value(ID, "showAero", &Value::Bool(false));
        assert!(!shown(&state, "aero"));
        assert_eq!(
            api_url(&state).as_deref(),
            Some("https://badges.equicord.org/users")
        );
    }
}
