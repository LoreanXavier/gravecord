//! BetterActivities (Equicord `src/equicordplugins/betterActivities`, GPL-3.0-or-later). Not part of the pinned client.
//!
//! - `memberList`: where the pinned client draws module 342312's `ActivityStatus` (the member list, module 110574, and
//!   the DM list, module 181254), `patchActivityList` closes the line with a row of icons (5px in, 3px apart,
//!   `iconSize` px, round): each activity's large image, else its small one (`mp:` on the media proxy, left out when
//!   it is a GIF and `renderGifs` is off; `spotify:` on Spotify's CDN; else the application's asset), or with no
//!   assets the application's icon (fetched when unknown) or the Xbox mark, one per picture; an activity named
//!   "Twitch" adds the Twitch mark, first with `specialFirst`. Hovering an icon shows the activity's card in a
//!   tooltip, without its buttons. With no icon but an activity that is not a custom status, the default activity
//!   glyph stands there instead. Bots get nothing. `hideTooltip` drops the icons where a caller passes
//!   `hideTooltip`; neither of these two places does.
//! - `userPopout`: the profile popout's activity cards (module 646986) become every activity, as a carousel (one
//!   card, then the arrows and a dot for each) or a list (`allActivitiesStyle`); a card after the first has no
//!   buttons (in the carousel: unless it is of the same application as the first).
//! - `removeGameActivityStatus`: those lines drop the activity part (glyph, text, "+N") and the dot after it, keeping
//!   the custom status. On the pinned client the plugin's regexes land elsewhere: the first matches the branch that
//!   never renders (`k` is always null), the second removes the dot and the custom status instead, the opposite of
//!   the setting's name. The setting's name is what is ported.
//! - The popout patch on the pinned client also swallows the profile widgets in front of the cards and puts the
//!   carousel behind their condition (the activity cards would show only with widgets); the cards are what is ported,
//!   in their place, and the widgets stay.
//!
//! GAP: the cards are the ones the app draws (its visible activities: a second game, a repeat of an application and
//! a hang status are left out, as in the app's own popout); the tooltip opens at once, without the tooltip's spring;
//! the arrows' tooltips use the app's tooltip colour, not `--background-secondary-alt`.

use super::super::live_activity::{Surface, asset_url};
use super::super::{Root, color, icon, tooltip};
use crate::net::Request;
use crate::plugins::State;
use dc_state::{Id, User};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Bounds, Context, Hsla, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    Pixels, SharedString, StatefulInteractiveElement, Styled, StyledImage, div, img, px, svg,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

const ID: &str = "BetterActivities";
/// `xboxUrl` in `utils.tsx`.
const XBOX: &str = "/assets/9a15d086141be29d9fcd.png";

#[derive(Clone, Debug, PartialEq)]
struct Settings {
    member_list: bool,
    icon_size: f32,
    special_first: bool,
    render_gifs: bool,
    remove_game: bool,
    user_popout: bool,
    hide_tooltip: bool,
    list: bool,
}

fn settings_of(state: &State) -> Option<Settings> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
    Some(Settings {
        member_list: flag("memberList"),
        icon_size: key("iconSize").as_f64().unwrap_or(15.) as f32,
        special_first: flag("specialFirst"),
        render_gifs: flag("renderGifs"),
        remove_game: flag("removeGameActivityStatus"),
        user_popout: flag("userPopout"),
        hide_tooltip: flag("hideTooltip"),
        list: key("allActivitiesStyle").as_str() == Some("list"),
    })
}

fn current() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(settings_of)
}

/// One icon of the row: a picture, or the Twitch mark; and the activity (its place in the raw list) it stands for.
#[derive(Clone, Debug, PartialEq)]
enum Glyph {
    Picture(String),
    Twitch,
}

#[derive(Clone, Debug, PartialEq)]
struct ActivityIcon {
    glyph: Glyph,
    activity: usize,
}

/// What closes the line.
#[derive(Clone, Debug, PartialEq)]
enum Tail {
    Icons(Vec<ActivityIcon>),
    /// `DefaultActivityIcon` (`size="xs"`).
    Default,
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str().filter(|s| !s.is_empty())
}

/// `addImage` of `getApplicationIcons`.
fn picture_url(image: &str, application: Option<&str>, render_gifs: bool) -> Option<String> {
    if let Some(path) = image.strip_prefix("mp:") {
        let url = format!("https://media.discordapp.net/{path}");
        return (render_gifs || !url.ends_with(".gif")).then_some(url);
    }
    if image.starts_with("spotify:") {
        let hash = image.split(':').nth(1).unwrap_or_default();
        return Some(format!("https://i.scdn.co/image/{hash}"));
    }
    // With no application the original's URL reads `app-assets/undefined/…`, a picture that never loads.
    Some(format!(
        "https://cdn.discordapp.com/app-assets/{}/{image}.png",
        application?
    ))
}

/// `getApplicationIcons(activities)`: `application_icon` answers the application's icon URL once its record is
/// known (`None` while it is not); `ask` is told about an application whose record is wanted.
fn application_icons(
    activities: &[Value],
    render_gifs: bool,
    application_icon: &dyn Fn(&str) -> Option<Option<String>>,
    ask: &mut dyn FnMut(&str),
    xbox: &str,
) -> Vec<(String, usize)> {
    let mut icons = Vec::new();
    for (index, activity) in activities.iter().enumerate() {
        let application = text(activity, "application_id");
        let platform = text(activity, "platform");
        let spotify = text(activity, "id").is_some_and(|id| id.starts_with("spotify:"));
        if application.is_none() && platform.is_none() && !spotify {
            continue;
        }
        let assets = &activity["assets"];
        if assets.is_object() {
            let image = text(assets, "large_image").or_else(|| text(assets, "small_image"));
            if let Some(url) = image.and_then(|image| picture_url(image, application, render_gifs))
            {
                icons.push((url, index));
            }
        } else if let Some(application) = application {
            match application_icon(application) {
                Some(Some(url)) => icons.push((url, index)),
                Some(None) if platform == Some("xbox") => icons.push((xbox.to_owned(), index)),
                Some(None) => {}
                None => {
                    ask(application);
                    if platform == Some("xbox") {
                        icons.push((xbox.to_owned(), index));
                    }
                }
            }
        } else if platform == Some("xbox") {
            icons.push((xbox.to_owned(), index));
        }
    }
    icons
}

/// `patchActivityList` from the icons of `getApplicationIcons`: one per picture, the Twitch mark, or the default
/// glyph.
fn tail(activities: &[Value], pictures: Vec<(String, usize)>, special_first: bool) -> Option<Tail> {
    let mut icons: Vec<ActivityIcon> = Vec::new();
    for (url, activity) in pictures {
        if !icons
            .iter()
            .any(|icon| icon.glyph == Glyph::Picture(url.clone()))
        {
            icons.push(ActivityIcon {
                glyph: Glyph::Picture(url),
                activity,
            });
        }
    }
    if let Some(index) = activities.iter().position(|a| a["name"] == "Twitch") {
        let twitch = ActivityIcon {
            glyph: Glyph::Twitch,
            activity: index,
        };
        if special_first {
            icons.insert(0, twitch);
        } else {
            // `splice(activityIndex, 0, …)`: past the end it appends.
            icons.insert(index.min(icons.len()), twitch);
        }
    }
    if !icons.is_empty() {
        return Some(Tail::Icons(icons));
    }
    activities
        .iter()
        .any(|activity| activity["type"] != 4)
        .then_some(Tail::Default)
}

thread_local! {
    /// The icon under the pointer, and where each drawn icon was last frame.
    static HOVERED: RefCell<Option<SharedString>> = const { RefCell::new(None) };
    static ANCHORS: RefCell<HashMap<SharedString, Bounds<Pixels>>> = RefCell::new(HashMap::new());
    /// The applications already asked for.
    static ASKED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// The carousel's activity per person (its `id`, else its name).
    static SHOWN: RefCell<HashMap<Id, String>> = RefCell::new(HashMap::new());
}

fn activity_key(activity: &Value) -> String {
    text(activity, "id")
        .or_else(|| text(activity, "name"))
        .unwrap_or_default()
        .to_owned()
}

impl Root {
    /// The application's icon URL once its record is in (`Some(None)` when it has none).
    fn better_activities_app_icon(&self, application: &str) -> Option<Option<String>> {
        let body = self.fetched.get(&format!("presence-app:{application}"))?;
        let record = body.as_array()?.first()?;
        Some(
            text(record, "icon").map(|hash| {
                format!("https://cdn.discordapp.com/app-icons/{application}/{hash}.png")
            }),
        )
    }

    /// `fetchApplication`, once per application (its answer lands where the profile cards' lookups land).
    fn better_activities_ask(&self, application: &str) {
        let key = format!("presence-app:{application}");
        if self.fetched.contains_key(&key) {
            return;
        }
        if ASKED.with(|asked| asked.borrow_mut().insert(key.clone())) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/applications/public?application_ids={application}"),
            });
        }
    }

    /// For `activity_status_line`: the icons that close the line, and whether the activity part goes.
    pub(in crate::ui) fn better_activities_status(
        &self,
        user: &User,
        guild: Option<Id>,
        place: &'static str,
        tint: &'static str,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, bool) {
        let Some(settings) = current() else {
            return (None, false);
        };
        let remove_game = settings.remove_game;
        // Neither the member list nor the DM list passes `hideTooltip`, so `settings.hide_tooltip` keeps them.
        if !settings.member_list || user.bot {
            return (None, remove_game);
        }
        let Some(presence) = self.stores.presence.get(user.id, guild) else {
            return (None, remove_game);
        };
        let activities = &presence.activities;
        let xbox = format!("{}{XBOX}", dc_proto::pin::WEBAPP_ORIGIN);
        let mut wanted = Vec::new();
        let pictures = application_icons(
            activities,
            settings.render_gifs,
            &|application| self.better_activities_app_icon(application),
            &mut |application| wanted.push(application.to_owned()),
            &xbox,
        );
        for application in wanted {
            self.better_activities_ask(&application);
        }
        let element = match tail(activities, pictures, settings.special_first) {
            None => None,
            Some(Tail::Default) => {
                Some(icon("inventory_unknown_game", 12., tint).into_any_element())
            }
            Some(Tail::Icons(icons)) => Some(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .ml(px(5.))
                    .gap(px(3.))
                    .children(icons.into_iter().enumerate().map(|(index, entry)| {
                        self.better_activities_icon(
                            user.id,
                            guild,
                            activities,
                            entry,
                            format!("{place}-{}-{index}", user.id).into(),
                            settings.icon_size,
                            cx,
                        )
                    }))
                    .into_any_element(),
            ),
        };
        (element, remove_game)
    }

    #[allow(clippy::too_many_arguments)]
    fn better_activities_icon(
        &self,
        user: Id,
        guild: Option<Id>,
        activities: &[Value],
        entry: ActivityIcon,
        key: SharedString,
        size: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let glyph: AnyElement = match &entry.glyph {
            Glyph::Twitch => svg()
                .path("icons/plugin_twitch.svg")
                .size(px(size))
                .text_color(gpui::rgb(0x5a3e85))
                .into_any_element(),
            Glyph::Picture(url) => match self.images.get(url) {
                Some(image) => img(image.clone())
                    .size(px(size))
                    .rounded_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                None => {
                    self.want_image(url.clone(), url.clone(), cx);
                    div().size(px(size)).into_any_element()
                }
            },
        };
        let hovered = HOVERED.with(|hovered| hovered.borrow().as_ref() == Some(&key));
        let card = hovered
            .then(|| ANCHORS.with(|anchors| anchors.borrow().get(&key).copied()))
            .flatten()
            .and_then(|anchor| {
                let card =
                    self.better_activities_card(user, guild, &activities[entry.activity], cx)?;
                Some(tooltip::placed_content(
                    anchor,
                    div().p(px(1.)).child(card).into_any_element(),
                ))
            });
        let (stored, hover_key) = (key.clone(), key.clone());
        div()
            .id(SharedString::from(format!("better-activities-{key}")))
            .relative()
            .flex_none()
            .size(px(size))
            .child(glyph)
            .child(
                gpui::canvas(
                    move |bounds, _, _| {
                        ANCHORS.with(|anchors| anchors.borrow_mut().insert(stored.clone(), bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_hover(cx.listener(move |_, inside: &bool, _, cx| {
                HOVERED.with(|hovered| {
                    let mut hovered = hovered.borrow_mut();
                    if *inside {
                        *hovered = Some(hover_key.clone());
                    } else if hovered.as_ref() == Some(&hover_key) {
                        *hovered = None;
                    }
                });
                cx.notify();
            }))
            .children(card)
            .into_any_element()
    }

    /// `ActivityTooltip`: the activity's card, as the popout draws it, without its buttons.
    fn better_activities_card(
        &self,
        user: Id,
        guild: Option<Id>,
        activity: &Value,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let presence = self.stores.presence.get(user, guild)?;
        let visible = presence.visible_activities();
        let index = visible
            .iter()
            .position(|shown| std::ptr::eq(*shown, activity))
            .or_else(|| {
                visible.iter().position(|shown| {
                    (!shown["application_id"].is_null()
                        && shown["application_id"] == activity["application_id"])
                        || (!shown["name"].is_null() && shown["name"] == activity["name"])
                })
            })?;
        for shown in &visible {
            for key in ["large_image", "small_image"] {
                if let Some(url) = asset_url(shown, key) {
                    self.want_image(url.clone(), url, cx);
                }
            }
        }
        if let Some(application) = text(visible[index], "application_id") {
            self.better_activities_ask(application);
        }
        let palette = self.profile_palette(user, guild, false);
        let card = self
            .live_activity_cards_picked(
                user,
                Surface::Popout,
                palette.text,
                palette.subtle,
                Some(&[(index, false)]),
                cx,
            )
            .into_iter()
            .next()?;
        Some(div().w(px(178.)).child(card).into_any_element())
    }

    /// `showAllActivitiesComponent`: the popout's cards when the plugin takes them.
    pub(in crate::ui) fn better_activities_popout(
        &self,
        user: Id,
        strong: Hsla,
        subtle: Hsla,
        cx: &mut Context<Self>,
    ) -> Option<Vec<AnyElement>> {
        let settings = current().filter(|settings| settings.user_popout)?;
        let presence = self.stores.presence.get(user, self.guild)?;
        let visible = presence.visible_activities();
        if visible.is_empty() {
            return Some(Vec::new());
        }
        if settings.list {
            let picks: Vec<(usize, bool)> = (0..visible.len()).map(|i| (i, i == 0)).collect();
            let cards = self.live_activity_cards_picked(
                user,
                Surface::Popout,
                strong,
                subtle,
                Some(&picks),
                cx,
            );
            return Some(vec![
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .children(cards)
                    .into_any_element(),
            ]);
        }
        let keys: Vec<String> = visible.iter().map(|a| activity_key(a)).collect();
        let shown = SHOWN.with(|shown| shown.borrow().get(&user).cloned());
        let index = shown
            .and_then(|key| keys.iter().position(|k| *k == key))
            .unwrap_or(0);
        // The first activity's own props (its buttons) go to a card of the same application.
        let buttons = visible[index]["application_id"] == visible[0]["application_id"];
        let cards = self.live_activity_cards_picked(
            user,
            Surface::Popout,
            strong,
            subtle,
            Some(&[(index, buttons)]),
            cx,
        );
        let mut column = div().flex().flex_col().children(cards);
        if visible.len() > 1 {
            column = column.child(self.better_activities_controls(user, &keys, index, cx));
        }
        Some(vec![column.into_any_element()])
    }

    /// `CarouselControls`: the arrows (which wrap around) and a dot per activity.
    fn better_activities_controls(
        &self,
        user: Id,
        keys: &[String],
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = keys.len();
        let arrow =
            |name: &'static str, label: &'static str, target: usize, cx: &mut Context<Self>| {
                let key = keys[target].clone();
                self.with_tip(
                    format!("better-activities-{label}-{user}"),
                    tooltip::Tip::text(label, tooltip::Side::Top),
                    div()
                        .id(SharedString::from(format!(
                            "better-activities-{label}-{user}"
                        )))
                        .cursor_pointer()
                        .child(
                            svg()
                                .path(format!("icons/{name}.svg"))
                                .size(px(24.))
                                .text_color(gpui::rgb(0xdddddd)),
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            SHOWN.with(|shown| shown.borrow_mut().insert(user, key.clone()));
                            cx.notify();
                        })),
                    cx,
                )
            };
        let left = arrow(
            "plugin_caret_left",
            "Left",
            if index >= 1 { index - 1 } else { count - 1 },
            cx,
        );
        let right = arrow(
            "plugin_caret_right",
            "Right",
            if index + 1 < count { index + 1 } else { 0 },
            cx,
        );
        let dots = keys.iter().enumerate().map(|(dot, key)| {
            let selected = dot == index;
            let key = key.clone();
            div()
                .id(SharedString::from(format!(
                    "better-activities-dot-{user}-{dot}"
                )))
                .mx(px(4.))
                .size(px(10.))
                .rounded_full()
                .cursor_pointer()
                .bg(color(if selected {
                    "--brand-500"
                } else {
                    "--interactive-muted"
                }))
                .opacity(if selected { 1. } else { 0.6 })
                .when(!selected, |this| this.hover(|style| style.opacity(1.)))
                .on_click(cx.listener(move |_, _, _, cx| {
                    SHOWN.with(|shown| shown.borrow_mut().insert(user, key.clone()));
                    cx.notify();
                }))
        });
        div()
            .mt(px(10.))
            .p(px(5.))
            .rounded(px(3.))
            .bg(color("--user-profile-overlay-background"))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(left)
            .child(div().flex().items_center().children(dots))
            .child(right)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn icons(
        activities: &[Value],
        gifs: bool,
        known: Option<Option<String>>,
    ) -> (Vec<(String, usize)>, Vec<String>) {
        let mut asked = Vec::new();
        let found = application_icons(
            activities,
            gifs,
            &|_| known.clone(),
            &mut |application| asked.push(application.to_owned()),
            "xbox.png",
        );
        (found, asked)
    }

    #[test]
    fn the_settings_follow_the_original_defaults() {
        let mut state = State::default();
        assert_eq!(settings_of(&state), None);
        state.set_enabled(ID, true);
        let settings = settings_of(&state).unwrap();
        assert!(settings.member_list && settings.special_first && settings.render_gifs);
        assert!(settings.user_popout && settings.hide_tooltip && !settings.remove_game);
        assert_eq!((settings.icon_size, settings.list), (15., false));
    }

    #[test]
    fn pictures_follow_get_application_icons() {
        let activities = vec![
            json!({"type": 4, "name": "Custom Status", "state": "hi"}),
            json!({"type": 0, "name": "Game", "application_id": "11", "assets": {"large_image": "abc", "small_image": "def"}}),
            json!({"type": 2, "name": "Spotify", "id": "spotify:1", "assets": {"large_image": "spotify:ab67"}}),
            json!({"type": 0, "name": "Gif", "application_id": "12", "assets": {"small_image": "mp:external/x/a.gif"}}),
            json!({"type": 0, "name": "Bare", "application_id": "13", "assets": {}}),
            json!({"type": 0, "name": "Again", "application_id": "11", "assets": {"large_image": "abc"}}),
        ];
        let (found, asked) = icons(&activities, true, None);
        assert!(
            asked.is_empty(),
            "an activity with assets never looks up its application"
        );
        assert_eq!(
            found,
            vec![
                (
                    "https://cdn.discordapp.com/app-assets/11/abc.png".to_owned(),
                    1
                ),
                ("https://i.scdn.co/image/ab67".to_owned(), 2),
                (
                    "https://media.discordapp.net/external/x/a.gif".to_owned(),
                    3
                ),
                (
                    "https://cdn.discordapp.com/app-assets/11/abc.png".to_owned(),
                    5
                ),
            ]
        );
        let (found, _) = icons(&activities, false, None);
        assert!(
            !found.iter().any(|(url, _)| url.ends_with(".gif")),
            "renderGifs off drops GIFs"
        );
        // One icon per picture.
        let Some(Tail::Icons(row)) = tail(&activities, icons(&activities, true, None).0, true)
        else {
            panic!("icons expected");
        };
        assert_eq!(row.len(), 3);
    }

    #[test]
    fn without_assets_the_application_icon_or_xbox_stands_in() {
        let game =
            vec![json!({"type": 0, "name": "G", "application_id": "21", "platform": "xbox"})];
        let (found, asked) = icons(&game, true, None);
        assert_eq!(asked, ["21"]);
        assert_eq!(found, vec![("xbox.png".to_owned(), 0)]);
        let (found, asked) = icons(&game, true, Some(Some("icon.png".to_owned())));
        assert!(asked.is_empty());
        assert_eq!(found, vec![("icon.png".to_owned(), 0)]);
        let plain = vec![json!({"type": 0, "name": "G", "application_id": "22"})];
        assert_eq!(icons(&plain, true, Some(None)).0, Vec::new());
        // Nothing to draw but a game: the default glyph.
        assert_eq!(tail(&plain, Vec::new(), true), Some(Tail::Default));
        let custom = vec![json!({"type": 4, "state": "hi"})];
        assert_eq!(tail(&custom, Vec::new(), true), None);
    }

    #[test]
    fn twitch_goes_first_or_at_its_place() {
        let activities = vec![
            json!({"type": 0, "name": "A", "application_id": "1", "assets": {"large_image": "a"}}),
            json!({"type": 0, "name": "B", "application_id": "2", "assets": {"large_image": "b"}}),
            json!({"type": 1, "name": "Twitch"}),
        ];
        let pictures = icons(&activities, true, None).0;
        let Some(Tail::Icons(first)) = tail(&activities, pictures.clone(), true) else {
            panic!()
        };
        assert_eq!(first[0].glyph, Glyph::Twitch);
        let Some(Tail::Icons(placed)) = tail(&activities, pictures, false) else {
            panic!()
        };
        assert_eq!(placed[2].glyph, Glyph::Twitch);
        assert_eq!(placed[2].activity, 2);
    }
}
