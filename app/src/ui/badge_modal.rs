//! The badge directory modal (module 319842) and what it reads: the catalog of a user's badges,
//! `GET /users/{id}/badges` (`USER_BADGES`, module 682618 `RS`; `@me` for the signed-in user).

use super::tooltip::{Body, Side, Tip};
use super::{Root, SANS, avatar_source, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, Image, IntoElement, SharedString, deferred, div, img,
    prelude::*, px, rgba,
};
use serde_json::Value;
use std::sync::Arc;

/// `fetched` key of a user's catalog.
pub(super) fn catalog_key(user: &str) -> String {
    format!("badge-catalog:{user}")
}

/// The route `USER_BADGES` builds: the signed-in user is `@me`.
pub(super) fn catalog_route(user: Option<Id>) -> String {
    match user {
        Some(user) => format!("/users/{user}/badges"),
        None => "/users/@me/badges".to_owned(),
    }
}

/// Key names and types of a JSON value, no values: objects list their keys, arrays their length and the shape of
/// the first element. Used by the harness to log the response shape of a route the snapshot has no record of.
#[cfg(feature = "capture")]
pub(super) fn shape(value: &Value, depth: usize) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(_) => "bool".to_owned(),
        Value::Number(_) => "number".to_owned(),
        Value::String(_) => "string".to_owned(),
        Value::Array(items) => match items.first() {
            Some(first) if depth > 0 => format!("[{} x {}]", items.len(), shape(first, depth - 1)),
            _ => format!("[{}]", items.len()),
        },
        Value::Object(map) => {
            if depth == 0 {
                return "{..}".to_owned();
            }
            let fields: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{key}: {}", shape(value, depth - 1)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
    }
}

impl Root {
    /// Harness: `badge catalog` asks for the signed-in user's catalog and the test DM user's (read-only GETs).
    #[cfg(feature = "capture")]
    pub(crate) fn script_badge_catalog(&mut self) -> String {
        let other = self
            .stores
            .private_channels
            .iter()
            .find(|dm| dm.id == super::dm_call::TEST_DM)
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id);
        let _ = self.requests.send(Request::Fetch {
            key: catalog_key("me"),
            route: catalog_route(None),
        });
        if let Some(other) = other {
            let _ = self.requests.send(Request::Fetch {
                key: catalog_key(&other.to_string()),
                route: catalog_route(Some(other)),
            });
        }
        "requested".to_owned()
    }

    /// Harness: `badge shape` logs the key names and types of each fetched catalog.
    #[cfg(feature = "capture")]
    pub(crate) fn script_badge_shape(&self) -> String {
        let mut out = Vec::new();
        for (key, body) in &self.fetched {
            if key.starts_with("badge-catalog:") {
                out.push(format!("{key} => {}", shape(body, 5)));
            }
        }
        if out.is_empty() {
            "no catalog fetched".to_owned()
        } else {
            out.join("\n")
        }
    }
}

/// `fetched` keys of the catalogs start with this.
pub(super) const CATALOG_PREFIX: &str = "badge-catalog:";

// Badge ids of the catalog (`BadgeId` enum, module 70283).
const PREMIUM_TENURE: u64 = 1;
const GUILD_BOOSTER: u64 = 12;
const APRIL_FOOLS_2026: u64 = 15;
const ACCOUNT_AGE: u64 = 18;
const STREAMING: u64 = 19;
const GAME_TIME: u64 = 20;
const GAME_VARIETY: u64 = 21;
const STAFF: u64 = 22;

/// The catalog id of a profile badge id (`w0`, module 731309): the legacy names map to numbers, and the rest are the
/// enum's names in lower case.
/// Module 851767 as it stores a profile: the Nitro badge reads `8zbGNR` and a tenure badge (`VD` of module 682901,
/// `premium_tenure_*_month_v2`) `Hu4jfi` with `premium_since`; a booster badge reads `IWkAq7` with
/// `premium_guild_since`; each date in the `medium` style. Any other badge keeps the text the server sent.
pub(super) fn badge_description(body: &Value, id: &str, description: &str) -> String {
    let date = |key: &str| {
        body[key]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|date| {
                date.with_timezone(&chrono::Local)
                    .format("%b %-d, %Y")
                    .to_string()
            })
    };
    let with_date = |key: &'static str, date: String| {
        dc_intl::format(
            key,
            &[(
                "date",
                dc_intl::Arg::When {
                    date: date.into(),
                    time: String::new().into(),
                },
            )],
        )
    };
    let tenure = id.starts_with("premium_tenure_") && id.ends_with("_month_v2");
    if (id == "premium" || tenure)
        && let Some(since) = date("premium_since")
    {
        return with_date(if tenure { "Hu4jfi" } else { "8zbGNR" }, since);
    }
    if id.starts_with("guild_booster_lvl")
        && let Some(since) = date("premium_guild_since")
    {
        return with_date("IWkAq7", since);
    }
    description.to_owned()
}

pub(super) fn catalog_id(profile_id: &str) -> Option<u64> {
    let legacy = match profile_id {
        "staff" | "staff_hidden" | "staff_personal" => Some(22),
        "premium"
        | "premium_tenure_1_month_v2"
        | "premium_tenure_3_month_v2"
        | "premium_tenure_6_month_v2"
        | "premium_tenure_12_month_v2"
        | "premium_tenure_24_month_v2"
        | "premium_tenure_36_month_v2"
        | "premium_tenure_60_month_v2"
        | "premium_tenure_72_month_v2" => Some(1),
        "partner" => Some(2),
        "certified_moderator" => Some(3),
        "hypesquad" => Some(4),
        "hypesquad_house_1" => Some(5),
        "hypesquad_house_2" => Some(6),
        "hypesquad_house_3" => Some(7),
        "bug_hunter_level_1" => Some(8),
        "bug_hunter_level_2" => Some(9),
        "verified_developer" => Some(10),
        "early_supporter" => Some(11),
        id if id.starts_with("guild_booster_lvl") => Some(12),
        "legacy_username" => Some(13),
        "quest_completed" => Some(14),
        "april_fools_2026" => Some(15),
        "orb_profile_badge" => Some(16),
        _ => None,
    };
    legacy.or_else(|| match profile_id.to_ascii_uppercase().as_str() {
        "GIFTING" => Some(17),
        "ACCOUNT_AGE" => Some(18),
        "STREAMING" => Some(19),
        "GAME_TIME" => Some(20),
        "GAME_VARIETY" => Some(21),
        _ => None,
    })
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str().filter(|text| !text.is_empty())
}

fn tier<'a>(badge: &'a Value, key: Option<&str>) -> Option<&'a Value> {
    let key = key?;
    badge["tiers"]
        .as_array()?
        .iter()
        .find(|tier| tier["key"].as_str() == Some(key))
}

/// `DW`: the tier a badge is shown at: the current one when owned, else the next, else the first.
fn shown_tier(badge: &Value) -> Option<&Value> {
    let key = if badge["owned"].as_bool().unwrap_or(false) {
        badge["current_tier"].as_str()
    } else {
        badge["next_tier"].as_str()
    };
    tier(badge, key).or_else(|| badge["tiers"].as_array()?.first())
}

/// The key a catalog picture is kept under.
fn art_key(url: &str) -> String {
    format!("badge-art:{url}")
}

/// `H5`: when the badge (at its current tier) was obtained.
fn obtained_at(badge: &Value) -> Option<&str> {
    badge["current_tier"]
        .as_str()
        .and_then(|key| badge["tier_obtained_at"][key].as_str())
        .or_else(|| text(badge, "obtained_at"))
}

/// `M/D/YY` of an ISO time in the local zone, the `date` `short` format of the en-US strings.
fn short_date(iso: &str) -> Option<String> {
    let time = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    let local = time.with_timezone(&chrono::Local);
    use chrono::Datelike;
    Some(format!(
        "{}/{}/{:02}",
        local.month(),
        local.day(),
        local.year() % 100
    ))
}

/// The three pieces of a badge tooltip and the modal title (`em` in module 567556, `e1` in 319842).
pub(super) struct Naming {
    pub eyebrow: Option<String>,
    pub title: String,
}

fn naming(badge_id: u64, name: &str, tier: Option<&Value>) -> Naming {
    let tier_name = tier.and_then(|tier| text(tier, "name").or_else(|| text(tier, "key")));
    if badge_id == PREMIUM_TENURE {
        let title = match tier_name {
            Some(tier) => format!("Nitro {tier}"),
            None => "Nitro".to_owned(),
        };
        return Naming {
            eyebrow: None,
            title,
        };
    }
    match tier.and_then(|tier| text(tier, "name")) {
        Some(tier_name) => Naming {
            eyebrow: Some(name.to_owned()),
            title: tier_name.to_owned(),
        },
        None => Naming {
            eyebrow: None,
            title: name.to_owned(),
        },
    }
}

impl Root {
    fn badge_catalog_key(&self, user: Id) -> String {
        match self.stores.current_user.as_ref() {
            Some(me) if me.id == user => catalog_key("me"),
            _ => catalog_key(&user.to_string()),
        }
    }

    fn catalog_badge(&self, catalog: &str, id: u64) -> Option<&Value> {
        self.fetched
            .get(catalog)?
            .get("badges")?
            .as_array()?
            .iter()
            .find(|badge| badge["badge_id"].as_u64() == Some(id))
    }

    /// Asks for a user's catalog once per session (`RS`, called by the badge list of a profile when the store has
    /// none).
    pub(super) fn request_badge_catalog(&mut self, user: Id) {
        let key = self.badge_catalog_key(user);
        if self.requested.insert(key.clone()) {
            let me = self.stores.current_user.as_ref().map(|me| me.id);
            let route = catalog_route(if Some(user) == me { None } else { Some(user) });
            let _ = self.requests.send(Request::Fetch { key, route });
        }
    }

    /// The catalog arrived: the pictures the badge list, the tooltips and the modal's tabs draw.
    pub(super) fn badge_catalog_fetched(&mut self, key: &str) {
        let mut urls: Vec<String> = Vec::new();
        for badge in self
            .fetched
            .get(key)
            .and_then(|body| body["badges"].as_array())
            .into_iter()
            .flatten()
        {
            for field in ["simple_icon_url", "complex_icon_static_url"] {
                urls.extend(text(badge, field).map(str::to_owned));
            }
            if let Some(tier) = shown_tier(badge) {
                for field in ["simple_icon_url", "complex_icon_static_url"] {
                    urls.extend(text(tier, field).map(str::to_owned));
                }
            }
        }
        for url in urls {
            self.request_image(art_key(&url), url);
        }
        if key == catalog_key("me") {
            self.request_gifting_art();
        }
    }

    /// The Send a Gift banner's data (module 806677 `z`, 492275): gifts left to the next Gifting badge tier
    /// (`getRemainingToNextTier`: threshold minus current of the first progress entry), that tier's name and its
    /// picture (`Se`: complex static icon, else simple). None without a catalog or a next tier.
    pub(super) fn gifting_banner(&self) -> Option<(u64, String, Option<Arc<Image>>)> {
        let catalog = catalog_key("me");
        let badge = self.catalog_badge(&catalog, 17)?;
        let next = tier(badge, badge["next_tier"].as_str())?;
        let progress = badge["progress"].as_array()?.first()?;
        let remaining = match (progress["threshold"].as_u64(), progress["current"].as_u64()) {
            (Some(threshold), current) => threshold.saturating_sub(current.unwrap_or(0)),
            _ => 0,
        };
        let name = text(next, "name")?.to_owned();
        let picture = self
            .art(text(next, "complex_icon_static_url").or_else(|| text(next, "simple_icon_url")));
        Some((remaining, name, picture))
    }

    /// Asks for the pictures of every Gifting badge tier (the banner draws the next one).
    pub(super) fn request_gifting_art(&mut self) {
        self.request_badge_tiers(&catalog_key("me"), 17);
    }

    /// Every tier picture of one badge, asked for when the modal shows it.
    fn request_badge_tiers(&mut self, catalog: &str, id: u64) {
        let urls: Vec<String> = self
            .catalog_badge(catalog, id)
            .and_then(|badge| badge["tiers"].as_array())
            .into_iter()
            .flatten()
            .flat_map(|tier| {
                ["simple_icon_url", "complex_icon_static_url"]
                    .into_iter()
                    .filter_map(|field| text(tier, field).map(str::to_owned))
            })
            .collect();
        for url in urls {
            self.request_image(art_key(&url), url);
        }
    }

    fn art(&self, url: Option<&str>) -> Option<Arc<Image>> {
        self.images.get(&art_key(url?)).cloned()
    }

    /// The tooltip over a profile badge (`em`): the art, the names and the line under them, from the catalog; the
    /// profile's own description when the catalog has nothing for it.
    fn badge_tip(&self, user: Id, profile_id: &str, description: &str) -> Body {
        let catalog = self.badge_catalog_key(user);
        let badge = catalog_id(profile_id).and_then(|id| self.catalog_badge(&catalog, id));
        let Some(badge) = badge else {
            return Body::Text(description.to_owned().into());
        };
        let current = tier(badge, badge["current_tier"].as_str());
        let name = text(badge, "name").unwrap_or(description);
        let id = badge["badge_id"].as_u64().unwrap_or(0);
        let Naming { eyebrow, title } = naming(id, name, current);
        let body = text(badge, "info_label").map(str::to_owned).or_else(|| {
            obtained_at(badge)
                .and_then(short_date)
                .map(|date| format!("Unlocked on {date}"))
        });
        let image = text(badge, "complex_icon_static_url")
            .or_else(|| text(badge, "simple_icon_url"))
            .map(art_key);
        Body::Badge {
            image,
            eyebrow: eyebrow.map(Into::into),
            title: title.into(),
            body: body.map(Into::into),
        }
    }

    /// The badges of a profile (`eJ`): each draws its picture, shows its tooltip on hover and opens the directory
    /// on its tab when pressed. `place` names the surface so every tooltip has its own key.
    pub(super) fn profile_badges(
        &self,
        place: &'static str,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(body) = self.fetched.get(&format!("profile:{user}")) else {
            return Vec::new();
        };
        // Not in the pinned client: GlobalBadges' badges lead the row (`plugin_hooks/global_badges.rs`).
        let leading = self.global_badge_elements(place, user, cx);
        let name = self
            .stores
            .users
            .get(&user)
            .map(|user| user.display_name().to_owned());
        leading
            .into_iter()
            .chain(
                super::profile::display_badges(body, self.guild)
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, badge)| {
                        let icon_hash = badge["icon"].as_str()?;
                        let id = badge["id"].as_str().unwrap_or_default().to_owned();
                        let description = badge_description(
                            body,
                            &id,
                            badge["description"].as_str().unwrap_or_default(),
                        );
                        let picture = self.images.get(&format!("badge:{icon_hash}")).cloned();
                        let tip = Tip::body(self.badge_tip(user, &id, &description), Side::Top)
                            .after(300);
                        let username = name.clone();
                        let badge_num = catalog_id(&id);
                        let is_own = self
                            .stores
                            .current_user
                            .as_ref()
                            .is_some_and(|me| me.id == user);
                        let badge_label = description.clone();
                        let target = div()
                            .id(SharedString::from(format!("{place}-badge-{user}-{index}")))
                            .size(px(20.))
                            .cursor_pointer()
                            .children(picture.map(|picture| img(picture).size(px(20.))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_badge_modal(
                                    Some(user),
                                    username.clone(),
                                    badge_num,
                                    false,
                                    cx,
                                );
                            }))
                            .when(is_own, |el| {
                                el.on_mouse_down(
                                    gpui::MouseButton::Right,
                                    cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        let bid = badge_num.unwrap_or(index as u64 + 1);
                                        this.open_badge_customization_menu(
                                            bid,
                                            badge_label.clone(),
                                            ev.position,
                                            cx,
                                        );
                                    }),
                                )
                            });
                        Some(
                            self.with_tip(
                                format!("{place}-badge-tip-{user}-{index}"),
                                tip,
                                target,
                                cx,
                            )
                            .into_any_element(),
                        )
                    }),
            )
            .collect()
    }
}

/// The open directory (`BadgeDirectoryModal`, module 319842).
pub(crate) struct BadgeModal {
    /// `targetUserId`: whose badges were asked for; none for the signed-in user's own.
    pub target: Option<Id>,
    /// `targetUsername`.
    pub username: Option<String>,
    /// The tab shown (`initialBadgeId`, then the tab pressed).
    pub selected: Option<u64>,
    /// `viewingCurrentUserBadges`: "View Your Badges" was pressed.
    pub own_view: bool,
    /// Tabs opened once: their "new" dot is gone (`BadgeDirectorySeenStore`).
    pub seen: Vec<u64>,
}

/// One slot of the left column: 48px, the 24px picture centred.
const SLOT: f32 = 48.;
/// The art of the detail column, `badgeGraphic`: 120 high at the catalog's 25:16 shape.
const ART_HEIGHT: f32 = 120.;
const ART_WIDTH: f32 = 187.1875;

/// `BadgeDirectorySeenStore` (module 319842): the badges whose "new" dot was already seen, kept across restarts.
const SEEN_KEY: &str = "BadgeDirectorySeenStore";

fn seen_indicators() -> Vec<u64> {
    crate::storage::get(SEEN_KEY)
        .and_then(|value| value["seenBadgeIndicatorIds"].as_array().cloned())
        .map(|ids| ids.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

impl Root {
    /// `BADGE_DIRECTORY_MARK_BADGE_INDICATOR_SEEN`: the detail of a badge with an indicator was shown to its owner.
    fn mark_badge_seen(&mut self, id: u64) {
        if !matches!(id, ACCOUNT_AGE | STREAMING | GAME_TIME | GAME_VARIETY) {
            return;
        }
        if let Some(modal) = self.badge_modal.as_mut()
            && !modal.seen.contains(&id)
        {
            modal.seen.push(id);
        }
        let mut stored = seen_indicators();
        if !stored.contains(&id) {
            stored.push(id);
            crate::storage::set(
                SEEN_KEY,
                serde_json::json!({ "seenBadgeIndicatorIds": stored }),
            );
        }
    }

    /// `openBadgeDirectoryModal`: asks for the catalog of the user shown and the viewer's own.
    pub(super) fn open_badge_modal(
        &mut self,
        target: Option<Id>,
        username: Option<String>,
        initial: Option<u64>,
        own_view: bool,
        cx: &mut Context<Self>,
    ) {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let other = target.is_some() && target != me && !own_view;
        let shown = if other { target } else { me };
        for user in [shown, me].into_iter().flatten() {
            let key = self.badge_catalog_key(user);
            let route = catalog_route(if Some(user) == me { None } else { Some(user) });
            self.requested.insert(key.clone());
            let _ = self.requests.send(Request::Fetch { key, route });
        }
        if let Some(initial) = initial {
            let catalog = shown
                .map(|user| self.badge_catalog_key(user))
                .unwrap_or_default();
            self.request_badge_tiers(&catalog, initial);
        }
        self.badge_modal = Some(BadgeModal {
            target,
            username,
            selected: initial,
            own_view,
            seen: seen_indicators(),
        });
        if let Some(initial) = initial
            && !other
        {
            self.mark_badge_seen(initial);
        }
        cx.notify();
    }

    /// Escape closes the directory.
    pub(super) fn badge_modal_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if key == "escape" && self.badge_modal.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    fn badge_modal_shown_user(&self) -> Option<Id> {
        let modal = self.badge_modal.as_ref()?;
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let other = modal.target.is_some() && modal.target != me && !modal.own_view;
        if other { modal.target } else { me }
    }

    fn badge_tab(
        &self,
        badge: &Value,
        selected: bool,
        new: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = badge["badge_id"].as_u64().unwrap_or(0);
        let picture = self.art(text(badge, "simple_icon_url"));
        div()
            .id(SharedString::from(format!("badge-tab-{id}")))
            .relative()
            .size(px(SLOT))
            .flex_none()
            .rounded(px(8.))
            .border_1()
            .border_color(if selected {
                color("--brand-400")
            } else {
                color("--border-muted")
            })
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .children(picture.map(|picture| img(picture).size(px(24.))))
            .when(new, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .size(px(8.))
                        .rounded_full()
                        .bg(color("--brand-500")),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                let shown = this.badge_modal_shown_user();
                let catalog = shown
                    .map(|user| this.badge_catalog_key(user))
                    .unwrap_or_default();
                this.request_badge_tiers(&catalog, id);
                if let Some(modal) = this.badge_modal.as_mut() {
                    modal.selected = Some(id);
                }
                if !this.badge_modal_shown_user().is_some_and(|shown| {
                    this.stores
                        .current_user
                        .as_ref()
                        .is_none_or(|me| me.id != shown)
                }) {
                    this.mark_badge_seen(id);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn badge_tier_tile(
        &self,
        tier: &Value,
        badge_owned: bool,
        show_requirement: bool,
    ) -> AnyElement {
        let unlocked = tier["owned"].as_bool().unwrap_or(false);
        let dimmed = !unlocked && badge_owned;
        let picture = self
            .art(text(tier, "simple_icon_url").or_else(|| text(tier, "complex_icon_static_url")));
        let requirement = if unlocked || !show_requirement {
            text(tier, "milestone_text").unwrap_or("").to_owned()
        } else {
            "Locked".to_owned()
        };
        let name_color = if unlocked {
            "--text-default"
        } else {
            "--text-muted"
        };
        let sub_color = if unlocked {
            "--text-subtle"
        } else {
            "--text-muted"
        };
        div()
            .w(px(77.))
            .py(px(8.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .size(px(32.))
                    .when(dimmed, |this| this.opacity(0.4))
                    .children(picture.map(|picture| img(picture).size(px(32.)))),
            )
            .child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .child(
                        div()
                            .font_weight(FontWeight(600.))
                            .text_color(color(name_color))
                            .child(text(tier, "name").unwrap_or("").to_owned()),
                    )
                    .when(!requirement.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(2.))
                                .text_color(color(sub_color))
                                .when(!unlocked, |this| {
                                    this.child(icon("access_lock", 8., sub_color))
                                })
                                .child(requirement),
                        )
                    }),
            )
            .into_any_element()
    }

    fn badge_cta(&self, badge_id: u64, viewer_owns: bool) -> Option<(&'static str, bool, u64)> {
        // `eu`: the label, and whether it is the expressive button; the id selects the action.
        Some(match badge_id {
            PREMIUM_TENURE => (
                if viewer_owns {
                    "Nitro Home"
                } else {
                    "Get Nitro"
                },
                !viewer_owns,
                PREMIUM_TENURE,
            ),
            GUILD_BOOSTER => (
                if viewer_owns {
                    "Manage Boosts"
                } else {
                    "Boost a server"
                },
                false,
                GUILD_BOOSTER,
            ),
            STAFF => ("Work With Us", false, STAFF),
            16 => ("Orbs Shop", false, 16),
            14 => ("Quests Home", false, 14),
            17 => ("Send Gift", false, 17),
            _ => return None,
        })
    }

    fn badge_cta_pressed(&mut self, badge_id: u64, cx: &mut Context<Self>) {
        self.badge_modal = None;
        self.profile = None;
        if badge_id == PREMIUM_TENURE {
            self.open_nitro(cx);
        } else if badge_id == GUILD_BOOSTER {
            // `openUserSettings(PREMIUM_GUILD_SUBSCRIPTIONS_PANEL)`, titled "Server Boost" (module 319842 `eu`).
            self.open_settings_page("server-boost", cx);
        } else if badge_id == STAFF {
            // `ctaAction` of STAFF in module 319842 opens the careers page.
            cx.open_url("https://discord.com/careers");
        } else if badge_id == 16 {
            // ORB_PROFILE: `COLLECTIBLES_SHOP_WITH_TAB(ORBS)`, the shop on its Orbs tab.
            self.open_shop(cx);
            self.shop_set_tab(super::shop::Tab::Orbs, cx);
        } else if badge_id == 14 {
            // QUEST_COMPLETED: `QUEST_HOME`.
            self.open_quests(cx);
        }
        cx.notify();
    }

    pub(super) fn badge_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.badge_modal.as_ref()?;
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let other = modal.target.is_some() && modal.target != me && !modal.own_view;
        let shown = if other { modal.target } else { me };
        let catalog = shown
            .map(|user| self.badge_catalog_key(user))
            .unwrap_or_default();
        let mine = self.badge_catalog_key(me.unwrap_or_default());
        let badges: &[Value] = self
            .fetched
            .get(&catalog)
            .and_then(|body| body["badges"].as_array())
            .map_or(&[], Vec::as_slice);
        let loaded = self.fetched.contains_key(&catalog);
        let (owned, earnable): (Vec<&Value>, Vec<&Value>) = {
            let mut owned = Vec::new();
            let mut earnable = Vec::new();
            for badge in badges {
                if badge["owned"].as_bool().unwrap_or(false) {
                    owned.push(badge);
                } else if badge["is_earnable"].as_bool().unwrap_or(false) {
                    earnable.push(badge);
                }
            }
            (owned, earnable)
        };
        let first = owned
            .first()
            .or(if other { None } else { earnable.first() })
            .and_then(|badge| badge["badge_id"].as_u64());
        let selected = modal
            .selected
            .filter(|id| self.catalog_badge(&catalog, *id).is_some())
            .or(first);
        let current = selected.and_then(|id| self.catalog_badge(&catalog, id));

        // The left column.
        let display = modal.username.clone().unwrap_or_default();
        let shortened: String = if display.chars().count() > 19 {
            format!("{}...", display.chars().take(19).collect::<String>())
        } else {
            display.clone()
        };
        let avatar = modal
            .target
            .and_then(|user| self.stores.users.get(&user))
            .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        let (title, lead) = if other && !display.is_empty() {
            (
                format!("{shortened}'s badges"),
                "Explore their badges, or view your badges to find out what you can unlock next.",
            )
        } else {
            (
                "Your badges".to_owned(),
                "Browse your badges and discover new ones you can unlock.",
            )
        };
        let title_row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(other && shortened == display, |this| {
                this.child(
                    div()
                        .size(px(24.))
                        .flex_none()
                        .rounded_full()
                        .overflow_hidden()
                        .children(avatar.map(|avatar| img(avatar).size(px(24.)).rounded_full())),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(title),
            );
        let seen = &modal.seen;
        let tabs = |list: &[&Value], cx: &mut Context<Self>| -> Vec<AnyElement> {
            list.iter()
                .map(|badge| {
                    let id = badge["badge_id"].as_u64().unwrap_or(0);
                    let new = !other
                        && matches!(id, ACCOUNT_AGE | STREAMING | GAME_TIME | GAME_VARIETY)
                        && !seen.contains(&id)
                        // The badge whose detail is open is marked seen at once (`useEffect` in module 319842).
                        && Some(id) != selected;
                    self.badge_tab(badge, Some(id) == selected, new, cx)
                })
                .collect()
        };
        let owned_tabs = tabs(&owned, cx);
        let earnable_tabs = if other {
            Vec::new()
        } else {
            tabs(&earnable, cx)
        };
        let grid = |tabs: Vec<AnyElement>| div().flex().flex_wrap().gap(px(8.)).children(tabs);
        let left_footer_label = if other {
            "View Your Badges"
        } else {
            "Customize Your Badges"
        };
        let left = div()
            .w(px(265.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(color("--border-subtle"))
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(title_row)
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .child(lead),
                    ),
            )
            .child(
                div()
                    .id("badge-directory-list")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pt(px(4.))
                    // `directoryContent_zoUCAE`: 16px end padding beside the `thin` scroller's stable 8px gutter.
                    .pr(px(24.))
                    .pb(px(24.))
                    .pl(px(24.))
                    .mt(px(-4.))
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .when(!owned_tabs.is_empty(), |this| {
                        this.child(div().flex().flex_col().gap(px(12.)).child(grid(owned_tabs)))
                    })
                    .when(!earnable_tabs.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--opacity-white-96"))
                                        .child("Available to unlock"),
                                )
                                .child(grid(earnable_tabs)),
                        )
                    }),
            )
            .child(
                div()
                    .p(px(16.))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .child(
                        div()
                            .id("badge-directory-switch")
                            .h(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-muted"))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|style| style.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(left_footer_label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if other {
                                    // `W`: View Your Badges.
                                    let (target, username) = this
                                        .badge_modal
                                        .as_ref()
                                        .map(|modal| (modal.target, modal.username.clone()))
                                        .unwrap_or_default();
                                    this.badge_modal = None;
                                    this.open_badge_modal(target, username, None, true, cx);
                                } else if let Some(me) =
                                    this.stores.current_user.as_ref().map(|me| me.id)
                                {
                                    // `ee`: Customize Your Badges closes the directory and opens the own profile editor.
                                    this.badge_modal = None;
                                    this.open_full_profile(me, window, cx);
                                }
                            })),
                    ),
            );

        // The detail column.
        let right_body: AnyElement = match current {
            None if !loaded => div().into_any_element(),
            None => div().into_any_element(),
            Some(badge) => self.badge_detail(badge, &catalog, &mine, other, modal, cx),
        };
        let full_bleed = self.viewport.1 <= 519. || self.viewport.0 <= 723.;
        let inner_r = if full_bleed { 0. } else { 11. };
        let right = div()
            .relative()
            .flex_1()
            .h_full()
            .min_w(px(0.))
            .rounded_r(px(inner_r))
            .bg(color("--background-base-lower"))
            .child(right_body)
            .child(
                div()
                    .id("badge-directory-close")
                    .absolute()
                    .top(px(16.))
                    .right(px(16.))
                    .size(px(32.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                    .child(icon("modal_close", 16., "--control-icon-only-icon-default"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.badge_modal = None;
                        cx.notify();
                    })),
            );

        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).

        let pose = super::motion::legacy_modal_pose("badge-modal", "badge-modal-layer", false);
        let modal_w = if full_bleed {
            self.viewport.0
        } else {
            682_f32.min((self.viewport.0 - 48.).max(320.))
        };
        let modal_h = if full_bleed {
            self.viewport.1
        } else {
            562_f32.min((self.viewport.1 - 48.).max(320.))
        };
        let card = div()
            .id("badge-modal")
            .occlude()
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .w(px(modal_w))
            .h(px(modal_h))
            .when(!full_bleed, |d| {
                d.rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-normal"))
            })
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(left)
            .child(right);
        Some(
            deferred(
                div()
                    .id("badge-modal-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(color("--background-scrim")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.badge_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `e1`: the detail column of one badge.
    fn badge_detail(
        &self,
        badge: &Value,
        catalog: &str,
        mine: &str,
        other: bool,
        modal: &BadgeModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = badge["badge_id"].as_u64().unwrap_or(0);
        let viewer = self.catalog_badge(mine, id);
        let _ = catalog;
        let owned = badge["owned"].as_bool().unwrap_or(false);
        let shown = shown_tier(badge);
        let art = self.art(
            shown
                .and_then(|tier| text(tier, "complex_icon_static_url"))
                .or_else(|| text(badge, "complex_icon_static_url"))
                .or_else(|| shown.and_then(|tier| text(tier, "simple_icon_url")))
                .or_else(|| text(badge, "simple_icon_url")),
        );
        let name = text(badge, "name").unwrap_or("");
        let nitro = id == PREMIUM_TENURE;
        let tier_name = shown.and_then(|tier| {
            if nitro {
                text(tier, "name").or_else(|| text(tier, "key"))
            } else {
                text(tier, "name")
            }
        });
        let (eyebrow, title) = if nitro {
            (
                None,
                tier_name.map_or("Nitro".to_owned(), |tier| format!("Nitro {tier}")),
            )
        } else if tier_name.is_some() && owned {
            (
                Some(name.to_owned()),
                tier_name.unwrap_or_default().to_owned(),
            )
        } else {
            (None, name.to_owned())
        };
        let has_tiers = badge["tiers"]
            .as_array()
            .is_some_and(|tiers| !tiers.is_empty());
        let obtained = obtained_at(badge);
        let status = if owned {
            if id == APRIL_FOOLS_2026 {
                "April 2026".to_owned()
            } else {
                match obtained.and_then(short_date) {
                    Some(date) if id != STAFF => format!("Unlocked {date}"),
                    _ => "Unlocked".to_owned(),
                }
            }
        } else {
            "Not unlocked yet".to_owned()
        };
        let mut segments: Vec<String> = Vec::new();
        segments.extend(text(badge, "info_label").map(str::to_owned));
        segments.push(status);

        let viewer_owns = viewer.is_some_and(|badge| badge["owned"].as_bool().unwrap_or(false));
        let description = if viewer.is_some_and(|viewer| {
            viewer["owned"].as_bool().unwrap_or(false)
                && viewer["tiers"]
                    .as_array()
                    .is_some_and(|tiers| !tiers.is_empty())
                && viewer["next_tier"].is_null()
        }) {
            "You've maxed out this badge. Sit back, relax, and enjoy the view from the top."
                .to_owned()
        } else {
            viewer
                .and_then(|viewer| text(viewer, "description"))
                .or_else(|| text(badge, "description"))
                .unwrap_or("")
                .to_owned()
        };
        let cta = self.badge_cta(id, viewer_owns);
        // `eg`: your own progress to the next level, shown for badges you own with a next tier.
        let progress_source = viewer.unwrap_or(badge);
        let next = tier(progress_source, progress_source["next_tier"].as_str());
        let own_progress =
            !other && progress_source["owned"].as_bool().unwrap_or(false) && next.is_some();
        let tier_source = if other {
            badge
        } else {
            viewer.unwrap_or(badge)
        };

        let dot = || div().size(px(3.)).rounded_full().bg(color("--text-subtle"));
        let accessory = div()
            .flex()
            .justify_center()
            .items_center()
            .gap(px(4.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-subtle"))
            .children(segments.into_iter().enumerate().map(|(index, segment)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(index > 0, |this| this.child(dot()))
                    .child(segment)
            }));
        let title_block = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    .text_color(color("--text-strong"))
                    .font_family(super::nitro::NITRO_FONT)
                    .when(nitro, |this| this.italic())
                    .font_weight(FontWeight(700.))
                    .text_size(px(24.))
                    .line_height(px(26.))
                    .children(eyebrow.map(|eyebrow| {
                        div()
                            .font_family(SANS)
                            .font_weight(FontWeight(500.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .mb(px(4.))
                            .text_color(color("--text-subtle"))
                            .child(eyebrow)
                    }))
                    .child(title.to_uppercase()),
            )
            .child(accessory);

        let progress_section = own_progress.then(|| {
            let progress = progress_source["progress"].get(0);
            let current_icon = tier(progress_source, progress_source["current_tier"].as_str())
                .and_then(|tier| {
                    text(tier, "simple_icon_url").or_else(|| text(tier, "complex_icon_static_url"))
                });
            let next_icon = next.and_then(|tier| {
                text(tier, "simple_icon_url").or_else(|| text(tier, "complex_icon_static_url"))
            });
            let helper = progress.and_then(|progress| text(progress, "progress_helper_text"));
            let fraction = progress
                .map(|progress| {
                    let current = progress["current"].as_f64().unwrap_or(0.);
                    let floor = progress["floor"].as_f64().unwrap_or(0.);
                    let threshold = progress["threshold"].as_f64().unwrap_or(0.);
                    if threshold > floor {
                        ((current - floor) / (threshold - floor)).clamp(0., 1.) as f32
                    } else {
                        0.
                    }
                })
                .unwrap_or(0.);
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child("Your progress"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .size(px(35.))
                                .flex_none()
                                .children(self.art(current_icon).map(|p| img(p).size(px(35.)))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(helper.unwrap_or("").to_owned()),
                                )
                                .child(
                                    div()
                                        .mt(px(8.))
                                        .mb(px(4.))
                                        .h(px(6.))
                                        .rounded(px(3.))
                                        .overflow_hidden()
                                        .bg(color("--background-mod-strong"))
                                        .child(
                                            div()
                                                .h_full()
                                                .w(gpui::relative(fraction))
                                                .rounded(px(3.))
                                                .bg(color("--brand-500")),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .size(px(35.))
                                .flex_none()
                                .children(self.art(next_icon).map(|p| img(p).size(px(35.)))),
                        ),
                )
        });
        let rule = || div().h(px(1.)).w_full().bg(color("--border-subtle"));
        let tiles: Vec<AnyElement> = tier_source["tiers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|tier| {
                self.badge_tier_tile(tier, tier_source["owned"].as_bool().unwrap_or(false), false)
            })
            .collect();
        let username = modal.username.clone().unwrap_or_default();
        let card_children = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(16.))
            .rounded(px(12.))
            .bg(color("--background-mod-muted"))
            .when_some(progress_section, |this, section| {
                this.child(section).child(rule())
            })
            .when(!description.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(16.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(description.clone()),
                        )
                        .children(cta.map(|(label, expressive, action)| {
                            let (background, border, text_color) = if expressive {
                                (
                                    color("--control-expressive-background-default"),
                                    color("--control-expressive-border-default"),
                                    color("--control-expressive-text-default"),
                                )
                            } else if viewer_owns {
                                (
                                    color("--control-secondary-background-default"),
                                    color("--border-muted"),
                                    color("--control-secondary-text-default"),
                                )
                            } else {
                                (
                                    color("--control-primary-background-default"),
                                    color("--control-primary-border-default"),
                                    color("--white"),
                                )
                            };
                            div()
                                .id("badge-directory-cta")
                                .flex_none()
                                .h(px(32.))
                                .px(px(11.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(border)
                                .bg(background)
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(text_color)
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.badge_cta_pressed(action, cx)
                                }))
                        })),
                )
            })
            .when(has_tiers, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .when(!description.is_empty() || own_progress, |this| {
                            this.child(rule())
                        })
                        .when(other && !username.is_empty(), |this| {
                            this.child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(format!("{username}'s Progress")),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .justify_center()
                                .gap(px(4.))
                                .children(tiles),
                        ),
                )
            });
        let missing = (other && !badge["is_earnable"].as_bool().unwrap_or(false) && !viewer_owns && id != STAFF)
            .then(|| {
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-muted"))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("This badge is no longer available, but others are waiting. View your badges to see what you can unlock.")
            });
        div()
            .id(SharedString::from(format!("badge-directory-detail-{id}")))
            .size_full()
            .overflow_y_scroll()
            .pt(px(16.))
            .px(px(24.))
            .pb(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .child(div().h(px(24.)).flex_none())
            .child(
                div()
                    .w(px(ART_WIDTH))
                    .h(px(ART_HEIGHT))
                    .flex_none()
                    .children(art.map(|art| img(art).w(px(ART_WIDTH)).h(px(ART_HEIGHT)))),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(title_block)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(card_children)
                            .children(missing),
                    ),
            )
            .into_any_element()
    }
}

/// Module 748579 (`764231.T`): formats tenure timeframe label (`erUSmA` months or `qOdyDe` years, `6+ Years` at 72m).
pub(crate) fn tenure_time_frame_label(months: u32) -> String {
    if months < 12 {
        dc_intl::format(
            "erUSmA",
            &[("months", dc_intl::Arg::Num(f64::from(months)))],
        )
    } else {
        let years = (months / 12).max(1);
        dc_intl::format("qOdyDe", &[("years", dc_intl::Arg::Num(f64::from(years)))])
    }
}

/// Module 748579 (`content_vYDVPJ`): Tiered Nitro Tenure Badge Level-Up Popout state.
#[derive(Clone, Debug)]
pub(crate) struct TenureLevelUpPopout {
    pub prev_tier_index: Option<usize>,
    pub current_tier_index: usize,
    pub subscriber_since: Option<String>,
    pub ended: bool,
    pub replay_count: u32,
}

/// Accent colors for the 8 Nitro tenure tiers (`--custom-old-badge-color` / `--custom-new-badge-color`).
const TENURE_TIER_COLORS: [u32; 8] = [
    0xcd7f32ff, // Bronze (1m)
    0xc0c0c0ff, // Silver (3m)
    0xffd700ff, // Gold (6m)
    0xe5e4e2ff, // Platinum (1y)
    0x58c7f3ff, // Diamond (2y)
    0x3ba55dff, // Emerald (3y)
    0xe91e63ff, // Ruby (5y)
    0xb48effff, // Opal (6+y)
];

/// Nitro Evolving Tenure Badge Modal state (module 384048).
#[derive(Clone, Debug)]
pub(crate) struct EvolvingTenureBadgeModal {
    pub is_tier2_subscriber: bool,
    pub viewing_other_user: bool,
    pub earned_tier_index: Option<usize>,
    pub subscriber_since: Option<String>,
    pub days_to_next_tier: Option<u32>,
}

/// The 8 evolving Nitro tenure tiers (`682901.VD` + `384048`): Bronze -> Opal.
const TENURE_TIERS: &[(&str, &str, u32, bool)] = &[
    ("LR1C0a", "erUSmA", 1, false),
    ("rI1/3H", "erUSmA", 3, false),
    ("VkhVKS", "erUSmA", 6, false),
    ("gzUfNS", "qOdyDe", 1, true),
    ("fuwTPm", "qOdyDe", 2, true),
    ("qNhNk4", "qOdyDe", 3, true),
    ("wvX+eD", "qOdyDe", 5, true),
    ("/menIw", "qOdyDe", 6, true),
];

impl Root {
    /// Nitro Evolving Tenure Badge Modal layer (`384048`: `Og62j7`, `IdAP91`, `hvVgAZ`, `RtGeFS`,
    /// `ffatnT`, `JST6jl`, `Hu4jfi`, `PPL1Il`).
    pub(super) fn evolving_tenure_badge_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.evolving_tenure_modal.as_ref()?;
        let is_sub = modal.is_tier2_subscriber;
        let viewing_other = modal.viewing_other_user;
        let next_idx = modal.earned_tier_index.map(|i| i + 1);

        let header = if is_sub {
            dc_intl::t("Og62j7")
        } else {
            dc_intl::t("RtGeFS")
        };

        let subheader: AnyElement = if is_sub {
            div()
                .text_center()
                .text_size(px(14.))
                .line_height(px(20.))
                .text_color(rgba(0xffffffcc))
                .child(dc_intl::t("IdAP91"))
                .into_any_element()
        } else {
            let text = dc_intl::format("ffatnT", &[]);
            div()
                .id("evolving-tenure-learn-more")
                .cursor_pointer()
                .text_center()
                .text_size(px(14.))
                .line_height(px(20.))
                .text_color(rgba(0xffffffcc))
                .child(text)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.evolving_tenure_modal = None;
                    this.open_nitro(cx);
                }))
                .into_any_element()
        };

        let cards =
            TENURE_TIERS
                .iter()
                .enumerate()
                .map(|(idx, &(name_key, unit_key, num, is_years))| {
                    let is_earned = modal.earned_tier_index == Some(idx);
                    let is_next = !viewing_other && next_idx == Some(idx);
                    let duration = if is_years {
                        dc_intl::format(unit_key, &[("years", dc_intl::Arg::Num(f64::from(num)))])
                    } else {
                        dc_intl::format(unit_key, &[("months", dc_intl::Arg::Num(f64::from(num)))])
                    };
                    let extra = if is_earned {
                        modal.subscriber_since.as_ref().map(|d| {
                            dc_intl::format(
                                "Hu4jfi",
                                &[("date", dc_intl::Arg::Str(std::borrow::Cow::Borrowed(d)))],
                            )
                        })
                    } else if is_next {
                        modal.days_to_next_tier.map(|days| {
                            dc_intl::format(
                                "PPL1Il",
                                &[("days", dc_intl::Arg::Num(f64::from(days)))],
                            )
                        })
                    } else {
                        None
                    };

                    div()
                        .w(px(140.))
                        .p(px(12.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(if is_earned {
                            rgba(0xeb459eff)
                        } else if is_next {
                            rgba(0x5865f2cc)
                        } else {
                            rgba(0xffffff1a)
                        })
                        .bg(if is_earned || is_next {
                            rgba(0xffffff1c)
                        } else {
                            rgba(0xffffff0a)
                        })
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .size(px(44.))
                                .rounded_full()
                                .bg(if is_earned {
                                    rgba(0xeb459e44)
                                } else {
                                    rgba(0x5865f233)
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("nitro_wheel", 24., "--white")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child(dc_intl::t(name_key)),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgba(0xffffffb3))
                                .child(duration),
                        )
                        .children(extra.map(|e| {
                            div()
                                .text_center()
                                .text_size(px(11.))
                                .font_weight(FontWeight(600.))
                                .text_color(rgba(0xf47fffff))
                                .child(e)
                        }))
                });

        let footer_cta = if !is_sub {
            Some(
                div()
                    .id("evolving-tenure-get-nitro")
                    .cursor_pointer()
                    .px(px(28.))
                    .h(px(40.))
                    .rounded(px(8.))
                    .bg(gpui::linear_gradient(
                        90.,
                        gpui::linear_color_stop(rgba(0x8e54e9ff), 0.),
                        gpui::linear_color_stop(rgba(0xeb459eff), 1.),
                    ))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("JST6jl"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.evolving_tenure_modal = None;
                        this.open_purchase(false, cx);
                    })),
            )
        } else if viewing_other {
            Some(
                div()
                    .id("evolving-tenure-learn-btn")
                    .cursor_pointer()
                    .px(px(24.))
                    .h(px(38.))
                    .rounded(px(8.))
                    .bg(gpui::linear_gradient(
                        90.,
                        gpui::linear_color_stop(rgba(0x5865f2ff), 0.),
                        gpui::linear_color_stop(rgba(0xeb459eff), 1.),
                    ))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("hvVgAZ"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.evolving_tenure_modal = None;
                        this.open_nitro(cx);
                    })),
            )
        } else {
            None
        };

        Some(
            gpui::deferred(
                div()
                    .id("evolving-tenure-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.evolving_tenure_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("evolving-tenure-modal")
                            .relative()
                            .w(px(660.))
                            .rounded(px(14.))
                            .border_1()
                            .border_color(rgba(0xffffff22))
                            .bg(gpui::linear_gradient(
                                205.,
                                gpui::linear_color_stop(rgba(0x251045ff), 0.),
                                gpui::linear_color_stop(rgba(0x1a1530ff), 1.),
                            ))
                            .p(px(28.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(16.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .id("evolving-tenure-close")
                                    .absolute()
                                    .top(px(14.))
                                    .right(px(14.))
                                    .size(px(32.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 18., "--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.evolving_tenure_modal = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .max_w(px(540.))
                                    .text_center()
                                    .text_size(px(24.))
                                    .line_height(px(30.))
                                    .font_weight(FontWeight(800.))
                                    .text_color(color("--white"))
                                    .child(header),
                            )
                            .child(subheader)
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_wrap()
                                    .justify_center()
                                    .gap(px(12.))
                                    .children(cards),
                            )
                            .children(footer_cta),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Module 748579 (`content_vYDVPJ`): Tiered Nitro Tenure Badge Level-Up Popout layer.
    pub(super) fn tenure_level_up_popout_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let popout = self.tenure_level_up.as_ref()?;
        let cur_idx = popout.current_tier_index.min(TENURE_TIERS.len() - 1);
        let (cur_name_key, _, cur_num, cur_is_years) = TENURE_TIERS[cur_idx];
        let cur_months = if cur_is_years { cur_num * 12 } else { cur_num };
        let cur_name = dc_intl::t(cur_name_key);
        let cur_color = rgba(TENURE_TIER_COLORS[cur_idx]);
        let prev_name = popout
            .prev_tier_index
            .and_then(|i| TENURE_TIERS.get(i))
            .map(|(k, _, _, _)| dc_intl::t(k));
        let prev_color = popout
            .prev_tier_index
            .and_then(|i| TENURE_TIER_COLORS.get(i).copied())
            .map(rgba);
        let is_light = super::theme() == dc_tokens::Theme::Light;
        let bg = if is_light {
            rgba(0xeeeff1ff)
        } else {
            rgba(0x000000ff)
        };
        let sub_line = popout.subscriber_since.as_deref().map(|date| {
            let tf = tenure_time_frame_label(cur_months);
            dc_intl::format(
                "f/OGgM",
                &[
                    ("timeFrame", dc_intl::Arg::Str(tf.into())),
                    ("date", dc_intl::Arg::Str(date.into())),
                ],
            )
        });
        let ended = popout.ended;

        let card = div()
            .id("tenure-level-up-popout")
            .occlude()
            .relative()
            .w(px(420_f32.min((self.viewport.0 - 48.).max(320.))))
            .h(px(460_f32.min((self.viewport.1 - 64.).max(320.))))
            .rounded(px(16.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(bg)
            .shadow(vec![super::shadow_high()])
            .overflow_hidden()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .p(px(24.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("tenure-level-up-close")
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .size(px(32.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(color("--control-icon-only-background-hover")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 16., "--control-icon-only-icon-default"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tenure_level_up = None;
                        cx.notify();
                    })),
            )
            // Level-up emblem centerpiece (replaces video frame in static/no-asset mode).
            .child(
                div()
                    .size(px(128.))
                    .rounded_full()
                    .bg(gpui::Rgba {
                        a: 0.16,
                        ..cur_color
                    })
                    .border_2()
                    .border_color(cur_color)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path("icons/settings_nitro.svg")
                            .size(px(56.))
                            .text_color(cur_color),
                    ),
            )
            // `textContainer_vYDVPJ`: NitroWordmark (60x15) + old/new tier titles + subscribedText + Play again.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .h(px(15.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("settings_nitro", 15., "--text-strong"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(15.))
                                    .font_weight(FontWeight(800.))
                                    .text_color(color("--text-strong"))
                                    .child("NITRO"),
                            ),
                    )
                    .children(prev_name.zip(prev_color).map(|(name, col)| {
                        div()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(gpui::Rgba { a: 0.65, ..col })
                            .child(name)
                    }))
                    .child(
                        div()
                            .text_size(px(30.))
                            .line_height(px(36.))
                            .font_weight(FontWeight(800.))
                            .text_color(cur_color)
                            .child(cur_name),
                    )
                    .children(sub_line.map(|line| {
                        div()
                            .mt(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(line)
                    }))
                    .when(ended, |col| {
                        col.child(
                            div()
                                .id("tenure-level-up-replay")
                                .mt(px(12.))
                                .h(px(32.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(icon("clips_play", 14., "--control-secondary-text-default"))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--control-secondary-text-default"))
                                        .child(dc_intl::t("hsvh0i")),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(p) = this.tenure_level_up.as_mut() {
                                        p.replay_count = p.replay_count.saturating_add(1);
                                        p.ended = true;
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            );

        Some(
            deferred(
                div()
                    .id("tenure-level-up-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(color("--background-scrim"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tenure_level_up = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `tenure-level-up [tier|close]` (module 748579).
    #[cfg(feature = "capture")]
    pub(crate) fn script_tenure_level_up(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let trimmed = arg.trim();
        if trimmed == "close" {
            self.tenure_level_up = None;
            cx.notify();
            return "tenure-level-up: closed".to_owned();
        }
        let idx = trimmed
            .parse::<usize>()
            .unwrap_or(4)
            .min(TENURE_TIERS.len() - 1);
        self.tenure_level_up = Some(TenureLevelUpPopout {
            prev_tier_index: idx.checked_sub(1),
            current_tier_index: idx,
            subscriber_since: Some("Oct 29, 2021".to_owned()),
            ended: true,
            replay_count: 0,
        });
        cx.notify();
        format!("tenure-level-up: tier {idx}")
    }

    /// Capture harness: `evolving-tenure [subscriber|upsell|other|close]` (module 384048).
    #[cfg(feature = "capture")]
    pub(crate) fn script_evolving_tenure(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.evolving_tenure_modal = None;
                cx.notify();
                "evolving-tenure: closed".to_owned()
            }
            "upsell" => {
                self.evolving_tenure_modal = Some(EvolvingTenureBadgeModal {
                    is_tier2_subscriber: false,
                    viewing_other_user: false,
                    earned_tier_index: None,
                    subscriber_since: None,
                    days_to_next_tier: None,
                });
                cx.notify();
                "evolving-tenure: upsell".to_owned()
            }
            "other" => {
                self.evolving_tenure_modal = Some(EvolvingTenureBadgeModal {
                    is_tier2_subscriber: true,
                    viewing_other_user: true,
                    earned_tier_index: Some(3),
                    subscriber_since: Some("Nov 5, 2024".to_owned()),
                    days_to_next_tier: None,
                });
                cx.notify();
                "evolving-tenure: other".to_owned()
            }
            _ => {
                self.evolving_tenure_modal = Some(EvolvingTenureBadgeModal {
                    is_tier2_subscriber: true,
                    viewing_other_user: false,
                    earned_tier_index: Some(2),
                    subscriber_since: Some("May 10, 2025".to_owned()),
                    days_to_next_tier: Some(42),
                });
                cx.notify();
                "evolving-tenure: subscriber".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nitro_and_booster_badges_read_their_dates_as_the_client_writes_them() {
        let body = json!({
            "premium_since": "2021-03-05T12:00:00.000000+00:00",
            "premium_guild_since": "2024-11-20T12:00:00.000000+00:00"
        });
        let nitro = badge_description(&body, "premium", "Subscriber since 3/5/21");
        assert!(nitro.contains("Mar 5, 2021"), "{nitro}");
        assert!(!nitro.contains("3/5/21"), "{nitro}");
        let tenure = badge_description(&body, "premium_tenure_12_month_v2", "x");
        assert!(tenure.contains("Mar 5, 2021"), "{tenure}");
        let boost = badge_description(
            &body,
            "guild_booster_lvl3",
            "Server boosting since 11/20/24",
        );
        assert!(boost.contains("Nov 20, 2024"), "{boost}");
        assert_eq!(
            badge_description(&body, "hypesquad_house_1", "HypeSquad"),
            "HypeSquad"
        );
        assert_eq!(
            badge_description(&json!({}), "premium", "Subscriber since 3/5/21"),
            "Subscriber since 3/5/21"
        );
    }

    #[test]
    fn profile_ids_map_to_catalog_ids() {
        assert_eq!(catalog_id("premium"), Some(PREMIUM_TENURE));
        assert_eq!(
            catalog_id("premium_tenure_12_month_v2"),
            Some(PREMIUM_TENURE)
        );
        assert_eq!(catalog_id("account_age"), Some(ACCOUNT_AGE));
        assert_eq!(catalog_id("guild_booster_lvl4"), Some(GUILD_BOOSTER));
        assert_eq!(catalog_id("staff"), Some(STAFF));
        assert_eq!(catalog_id("no_such_badge"), None);
    }

    #[test]
    fn nitro_title_joins_the_tier_and_other_badges_use_an_eyebrow() {
        let tier = json!({"key": "starter", "name": "Starter"});
        let nitro = naming(PREMIUM_TENURE, "Nitro", Some(&tier));
        assert_eq!(
            (nitro.eyebrow, nitro.title.as_str()),
            (None, "Nitro Starter")
        );
        let tier = json!({"key": "blossom", "name": "Blossom"});
        let age = naming(ACCOUNT_AGE, "Account Age", Some(&tier));
        assert_eq!(age.eyebrow.as_deref(), Some("Account Age"));
        assert_eq!(age.title, "Blossom");
        let plain = naming(STAFF, "Discord Staff", None);
        assert_eq!(
            (plain.eyebrow, plain.title.as_str()),
            (None, "Discord Staff")
        );
    }

    #[test]
    fn the_shown_tier_is_the_current_one_when_owned_else_the_next() {
        let owned = json!({"owned": true, "current_tier": "b", "next_tier": "c",
            "tiers": [{"key": "a"}, {"key": "b"}, {"key": "c"}]});
        assert_eq!(shown_tier(&owned).unwrap()["key"], "b");
        let locked =
            json!({"owned": false, "next_tier": "c", "tiers": [{"key": "a"}, {"key": "c"}]});
        assert_eq!(shown_tier(&locked).unwrap()["key"], "c");
        let none = json!({"owned": false, "tiers": [{"key": "a"}]});
        assert_eq!(shown_tier(&none).unwrap()["key"], "a");
    }

    #[test]
    fn dates_are_month_day_two_digit_year() {
        let date = short_date("2026-11-05T12:00:00+00:00").unwrap();
        assert!(
            date == "11/5/26" || date == "11/6/26" || date == "11/4/26",
            "{date}"
        );
        assert_eq!(short_date("not a date"), None);
    }

    #[test]
    fn tenure_level_up_time_frame_labels_748579() {
        assert_eq!(tenure_time_frame_label(1), "1 Month");
        assert_eq!(tenure_time_frame_label(3), "3 Months");
        assert_eq!(tenure_time_frame_label(6), "6 Months");
        assert_eq!(tenure_time_frame_label(12), "1 Year");
        assert_eq!(tenure_time_frame_label(24), "2 Years");
        assert_eq!(tenure_time_frame_label(72), "6+ Years");
        for key in ["f/OGgM", "hsvh0i", "erUSmA", "qOdyDe"] {
            assert!(dc_intl::has_key(key), "missing 748579 key {key}");
        }
    }
}
