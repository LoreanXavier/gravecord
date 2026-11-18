//! The DM profile's friend request parts (module 289287 `oE`):
//!
//! * The friend request notice (module 477534) in an `Overlay` of module 946356, once for a pending incoming friend
//!   request and once for every pending incoming Game Friend request (922590 `fi`): "**X** sent you a friend
//!   request." (`uIomXw`), "... from [icon] **Game**." when the request came from a game (`V15uUI`, the request's
//!   `origin_application_id`), "**X** sent you a **Game Friend** request from [icon] **Game**." (`syHjLL`); the
//!   request's own message under a View Request cover (module 420825 `z`, classes of 266600); Accept and Ignore.
//!   With an application named and its record not known yet, the notice waits (`U&&null==_`).
//! * The Game Friend section (module 361311 under heading `Uv/eTx`): "You are Game Friends from [icon] **A**." for
//!   the games of 821269 `q`, nothing for a friend.
//!
//! Accept and Ignore go through 393426: a friend request is `PUT {}` / `DELETE` on the relationship (the friends
//! list's own calls); a Game Friend request `PUT {type: 1}` / `DELETE` on `/users/@me/game-relationships/{user}/{app}`,
//! whose failure raises the alert of module 579872 ("Game Friend request failed", the response's error or "An error
//! occurred, please try again."). Their success lines (`zRf8cO`, `taJiuc`, `XMf21q`) are screen reader announcements
//! (765178 `announce`): nothing is drawn for them.

use super::chat_notices::{Alert, Then};
use super::dm_profile::Palette;
use super::{Root, icon};
use crate::net::Request;
use dc_state::{Id, User, game_relationships};
use gpui::{AnyElement, Context, FontWeight, SharedString, div, img, prelude::*, px};
use serde_json::{Value, json};

/// `relationship-type` 3.
const PENDING_INCOMING: u64 = 3;
/// The key prefix of a Game Friend request answer.
const GAME_WRITE: &str = "game-relationship-write:";

thread_local! {
    /// Notices whose request message the viewer uncovered (`useState` of 420825's `x`, one per notice), by user and
    /// the Game Friend request's application.
    static SHOWN_NOTES: std::cell::RefCell<std::collections::HashSet<(Id, Option<Id>)>> = Default::default();
}

/// `hideFriendRequestNotes` when the account has set it (the inverted "Show personalized messages" switch), else
/// `nsfwAllowed === false` (module 420825 `s`, 840387).
pub(super) fn notes_hidden(stored: Option<bool>, nsfw_allowed: bool) -> bool {
    stored.unwrap_or(!nsfw_allowed)
}

/// The games named in module 361311's sentence: one, two, or a list ending in "and".
pub(super) fn game_friend_key(count: usize) -> Option<&'static str> {
    match count {
        0 => None,
        1 => Some("wQ6urw"),
        2 => Some("C98CSN"),
        _ => Some("UxpwAh"),
    }
}

/// The route of one Game Friend link (`USER_GAME_RELATIONSHIP` of module 652215).
pub(super) fn game_relationship_route(user: Id, application: Id) -> String {
    format!("/users/@me/game-relationships/{user}/{application}")
}

/// A run of a sentence: text (bold or not), or the hook (a tag without `$`) standing where an element goes.
#[derive(Debug, PartialEq)]
pub(super) enum Run {
    Text(String, bool),
    Hook(Box<str>),
}

/// A message's runs, `$b` parts marked bold, with each named hook kept where the message puts it (`format_runs` prints
/// a hook given a value as that value, in a run of its own tagged with the hook's name).
pub(super) fn sentence(key: &str, values: &[(&str, String)], hooks: &[&str]) -> Vec<Run> {
    let mut args: Vec<(&str, dc_intl::Arg<'_>)> = values
        .iter()
        .map(|(name, value)| (*name, value.clone().into()))
        .collect();
    args.extend(hooks.iter().map(|hook| (*hook, "\u{1}".into())));
    dc_intl::format_runs(key, &args)
        .into_iter()
        .map(|(text, tag)| match tag {
            Some(tag) if hooks.contains(&&*tag) => Run::Hook(tag),
            tag => Run::Text(text, tag.as_deref() == Some("$b")),
        })
        .collect()
}

/// A sentence laid out word by word in a wrapping row, so an element (a game's icon) flows inline with the words.
/// A line may only break after white space, as in the page: a word, an element and the punctuation after it stay
/// together.
fn wrapped(runs: Vec<Run>, mut elements: impl FnMut(&str) -> Vec<AnyElement>) -> gpui::Div {
    let mut row = div().flex().flex_wrap().items_end();
    let mut group: Vec<AnyElement> = Vec::new();
    let flush = |row: gpui::Div, group: &mut Vec<AnyElement>| {
        if group.is_empty() {
            return row;
        }
        row.child(
            div()
                .flex_none()
                .flex()
                .items_end()
                .children(std::mem::take(group)),
        )
    };
    for run in runs {
        match run {
            Run::Text(text, bold) => {
                for (_, word) in crate::inline::words(&text) {
                    group.push(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .when(bold, |this| this.font_weight(FontWeight(600.)))
                            .child(word.to_owned())
                            .into_any_element(),
                    );
                    if word.ends_with(char::is_whitespace) {
                        row = flush(row, &mut group);
                    }
                }
            }
            Run::Hook(hook) => group.extend(elements(&hook)),
        }
    }
    flush(row, &mut group)
}

impl Root {
    /// The record of an application from `/applications/public` (ApplicationStore through 429913).
    pub(super) fn public_application(&self, application: Id) -> Option<&Value> {
        self.fetched
            .get(&format!("presence-app:{application}"))?
            .as_array()?
            .first()
    }

    /// Asks for an application's record once (429913 `fetchApplications`); its icon follows the answer.
    pub(super) fn request_public_application(&mut self, application: Id) {
        let key = format!("presence-app:{application}");
        if self.retry_failed_fetch(&key) {
            self.fetched.remove(&key);
        }
        if !self.fetched.contains_key(&key) {
            self.fetched.insert(key.clone(), Value::Null);
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/applications/public?application_ids={application}"),
            });
        }
    }

    /// The games the DM profile's request notices and Game Friend section name.
    pub(super) fn request_friend_notice_apps(&mut self, user: Id) {
        let mut apps: Vec<Id> = self
            .stores
            .relationship_origin_apps
            .get(&user)
            .copied()
            .into_iter()
            .collect();
        apps.extend(
            self.stores
                .game_relationships
                .applications_for_user(user, PENDING_INCOMING),
        );
        apps.extend(
            self.stores
                .game_relationships
                .applications_for_user(user, game_relationships::FRIEND),
        );
        for app in apps {
            self.request_public_application(app);
        }
    }

    /// A gateway event that names a request's game: its record is asked for (the notice waits for it).
    pub(super) fn friend_notice_event(&mut self, name: &str, data: &Value) {
        let field = match name {
            "GAME_RELATIONSHIP_ADD" => "application_id",
            "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => "origin_application_id",
            _ => return,
        };
        if let Some(app) = data[field].as_str().and_then(|id| id.parse().ok()) {
            self.request_public_application(app);
        }
    }

    /// An `/applications/public` answer that left out the asked-for game: GameRelationshipStore drops its pending
    /// entries (`APPLICATIONS_FETCH_SUCCESS` with `unknownApplicationIds`).
    pub(super) fn public_application_answered(&mut self, key: &str, body: &Value) {
        let Some(app) = key
            .strip_prefix("presence-app:")
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return;
        };
        let known = body.as_array().is_some_and(|apps| {
            apps.iter()
                .any(|record| record["id"].as_str() == Some(&app.to_string()))
        });
        if body.is_array() && !known {
            self.stores.drop_unknown_game_applications(&[app]);
        }
    }

    /// The `16px` game icon of the sentences (`gameIcon_qqn4GH` / `applicationIcon_UhAaNq`, module 769015).
    fn notice_game_icon(&self, application: Id, size: f32) -> AnyElement {
        match self.images.get(&format!("app-icon:{application}")).cloned() {
            Some(picture) => img(picture)
                .flex_none()
                .size(px(size))
                .rounded(px(4.))
                .into_any_element(),
            None => icon("inventory_unknown_game", size, "--text-default").into_any_element(),
        }
    }

    /// The friend request notices of the DM profile, in `oE`'s order: the friend request, then each Game Friend
    /// request.
    pub(super) fn friend_request_notices(
        &self,
        user: &User,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut notices = Vec::new();
        if self.stores.relationships.get(&user.id) == Some(&PENDING_INCOMING) {
            let origin = self.stores.relationship_origin_apps.get(&user.id).copied();
            notices.extend(self.friend_request_notice(user, origin, false, palette, cx));
        }
        for app in self
            .stores
            .game_relationships
            .applications_for_user(user.id, PENDING_INCOMING)
        {
            notices.extend(self.friend_request_notice(user, Some(app), true, palette, cx));
        }
        notices
    }

    fn friend_request_notice(
        &self,
        user: &User,
        application: Option<Id>,
        game: bool,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let name = user.display_name().to_owned();
        let record = match application {
            Some(app) => Some(self.public_application(app)?),
            None => None,
        };
        let text_color = palette.token("--text-default");
        let sentence = match (application, record) {
            (Some(app), Some(record)) => {
                let app_name = record["name"].as_str().unwrap_or_default().to_owned();
                let key = if game { "syHjLL" } else { "V15uUI" };
                let runs = sentence(
                    key,
                    &[("username", name), ("applicationName", app_name)],
                    &["applicationIcon"],
                );
                // `gameIcon_qqn4GH`: the 16px icon sits on the line's bottom (`vertical-align: bottom`).
                wrapped(runs, |_| vec![self.notice_game_icon(app, 16.)]).into_any_element()
            }
            _ => wrapped(sentence("uIomXw", &[("username", name)], &[]), |_| {
                Vec::new()
            })
            .into_any_element(),
        };
        let user_id = user.id;
        let accept = self
            .profile_small_button(
                format!(
                    "{}-request-accept-{}",
                    if game { "game" } else { "friend" },
                    application.unwrap_or_default()
                ),
                dc_intl::t("Zcibdf"),
                true,
                palette,
            )
            .on_click(
                cx.listener(move |this, _, _, cx| match application.filter(|_| game) {
                    Some(app) => this.answer_game_request(user_id, app, true),
                    // GAP: `maybeConfirmFriendRequestAccept`'s stranger confirmation (691464) is not built; the friends
                    // list's accept has none either.
                    None => this.set_relationship(user_id, Some(0), cx),
                }),
            );
        let ignore = self
            .profile_small_button(
                format!(
                    "{}-request-ignore-{}",
                    if game { "game" } else { "friend" },
                    application.unwrap_or_default()
                ),
                dc_intl::t("xuio0C"),
                false,
                palette,
            )
            .on_click(
                cx.listener(move |this, _, _, cx| match application.filter(|_| game) {
                    Some(app) => this.answer_game_request(user_id, app, false),
                    None => this.set_relationship(user_id, None, cx),
                }),
            );
        let light = palette.theme.as_ref().is_some_and(|theme| theme.light());
        Some(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .rounded(px(8.))
                .overflow_hidden()
                .bg(palette.token("--user-profile-overlay-background"))
                .when(light, |this| {
                    this.border_1()
                        .border_color(palette.token("--border-muted"))
                })
                .child(
                    // `container_qqn4GH`.
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .py(px(10.))
                        .px(px(12.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(text_color)
                        .child(sentence)
                        .children(self.request_note(
                            user_id,
                            application.filter(|_| game),
                            palette,
                            cx,
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(accept)
                                .child(ignore),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The request's own message (module 420825 `z`, `container_CbAHMP`): covered by View Request until pressed.
    fn request_note(
        &self,
        user: Id,
        application: Option<Id>,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let note = self
            .stores
            .relationship_notes
            .get(&user)
            .filter(|note| !note.is_empty())?;
        let stored = match self.setting_choices.get("friend_requests_notes_setting") {
            Some(super::settings::Choice::On(show)) => Some(!show),
            _ => self
                .stores
                .prefs
                .get("friend_requests_notes_setting")
                .map(|value| *value != 0),
        };
        if notes_hidden(stored, self.stores.account.nsfw_allowed) {
            return None;
        }
        let notice = (user, application);
        let shown = SHOWN_NOTES.with(|set| set.borrow().contains(&notice));
        let cover = (!shown).then(|| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    self.profile_small_button(
                        format!("friend-request-view-{}", application.unwrap_or_default()),
                        dc_intl::t("sB0q4C"),
                        false,
                        palette,
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        SHOWN_NOTES.with(|set| set.borrow_mut().insert(notice));
                        cx.notify();
                    })),
                )
        });
        Some(
            div()
                .relative()
                .p(px(12.))
                .rounded(px(4.))
                .border_1()
                .border_color(palette.token("--border-muted"))
                .bg(palette.token("--background-mod-subtle"))
                .child(
                    // `heading-sm/medium`; `hidden_CbAHMP` keeps its room while covered.
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(palette.token("--text-default"))
                        .when(!shown, |this| this.invisible())
                        .child(note.clone()),
                )
                .children(cover)
                .into_any_element(),
        )
    }

    /// The design system's `sm` button inside a profile card (32px, 16px across, `body-sm/medium`), in the
    /// card's theme: `primary` or `secondary`.
    pub(super) fn profile_small_button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        primary: bool,
        palette: &Palette,
    ) -> gpui::Stateful<gpui::Div> {
        let id: SharedString = id.into();
        let eye = id.starts_with("friend-request-view");
        let family = if primary { "primary" } else { "secondary" };
        let token = |part: &str| palette.token(&format!("--control-{family}-{part}"));
        div()
            .id(id)
            .flex_none()
            .h(px(32.))
            .min_w(px(60.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(token("border-default"))
            .bg(token("background-default"))
            .hover(|style| style.bg(token("background-hover")))
            .active(|style| style.bg(token("background-active")))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(if primary {
                palette.token("--white")
            } else {
                token("text-default")
            })
            .when(eye, |this| {
                this.child(icon("login_eye", 16., "--control-secondary-text-default"))
            })
            .child(label.into())
    }

    /// 393426 `acceptFriendRequest` / `cancelFriendRequest` with an application: the Game Friend link is set to a
    /// friend or deleted. The store changes when the gateway says so.
    fn answer_game_request(&mut self, user: Id, application: Id, accept: bool) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{GAME_WRITE}{user}:{application}")),
            method: if accept {
                reqwest::Method::PUT
            } else {
                reqwest::Method::DELETE
            },
            route: game_relationship_route(user, application),
            body: accept.then(|| json!({"type": game_relationships::FRIEND})),
        });
    }

    /// The answer to a Game Friend write: a failure shows the alert of 393426 `d`.
    pub(super) fn game_request_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if !key.starts_with(GAME_WRITE) {
            return false;
        }
        if body["ok"] != true {
            // `getAnyErrorMessage` (627784): a field error or the response's message, if there is one.
            let answer = &body["body"];
            let message = if answer["message"].is_string() || answer["errors"].is_object() {
                super::server_automod::any_error_message(answer)
            } else {
                dc_intl::t("fEptJP").to_owned()
            };
            self.chat_extra.alert = Some(Alert::show(
                "game-request-failed",
                dc_intl::t("328j/I"),
                Some(message),
                None,
                None,
                Then::Close,
            ));
            cx.notify();
        }
        true
    }

    /// The Game Friend section of the DM profile (heading `Uv/eTx` over module 361311), for a person who is not a
    /// friend (821269 `q`) and has game friendships with this account whose games are known.
    pub(super) fn game_friends_section(&self, user: Id, palette: &Palette) -> Option<AnyElement> {
        // GAP: 821269 also leaves out a provisional account (`isProvisional`), which this client does not know.
        if self.stores.relationships.get(&user) == Some(&super::friends::RELATIONSHIP_FRIEND) {
            return None;
        }
        let apps: Vec<(Id, String)> = self
            .stores
            .game_relationships
            .applications_for_user(user, game_relationships::FRIEND)
            .into_iter()
            .filter_map(|app| {
                let name = self.public_application(app)?["name"].as_str()?.to_owned();
                Some((app, name))
            })
            .collect();
        let key = game_friend_key(apps.len())?;
        let text = palette.token("--text-default");
        let strong = palette.token("--text-strong");
        // Each game is 102876: its 24px icon (`applicationIcon_UhAaNq`: 1.33em tall, 2px before and 4px after) and
        // its name in bold, a comma after it inside a list (`useComma`).
        // In the list the last comma's space folds into the " and " after it, as white space collapses.
        let game = |(app, name): &(Id, String), comma: Option<&str>| {
            div()
                .flex_none()
                .flex()
                .items_end()
                .child(
                    div()
                        .ml(px(2.))
                        .mr(px(4.))
                        .child(self.notice_game_icon(*app, 14. * 1.33)),
                )
                .child(
                    div()
                        .whitespace_nowrap()
                        .font_weight(FontWeight(600.))
                        .child(format!("{name}{}", comma.unwrap_or_default())),
                )
                .into_any_element()
        };
        let runs = sentence(
            key,
            &[],
            &[
                "applicationName",
                "applicationName2",
                "applications",
                "applicationNameLast",
            ],
        );
        let (last, rest) = apps.split_last()?;
        let pieces = wrapped(runs, |hook| match hook {
            "applicationName" => apps
                .first()
                .map(|app| game(app, None))
                .into_iter()
                .collect(),
            "applicationName2" => apps.get(1).map(|app| game(app, None)).into_iter().collect(),
            "applications" => rest
                .iter()
                .enumerate()
                .map(|(at, app)| game(app, Some(if at + 1 == rest.len() { "," } else { ", " })))
                .collect(),
            "applicationNameLast" => vec![game(last, None)],
            _ => Vec::new(),
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .h(px(16.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(strong)
                        .child(dc_intl::t("Uv/eTx")),
                )
                .child(
                    pieces
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(text),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_notes_follow_the_setting_then_the_nsfw_gate() {
        assert!(notes_hidden(Some(true), true));
        assert!(!notes_hidden(Some(false), false));
        assert!(notes_hidden(None, false));
        assert!(!notes_hidden(None, true));
    }

    #[test]
    fn a_sentence_keeps_its_hooks_where_the_message_puts_them() {
        let runs = sentence(
            "V15uUI",
            &[
                ("username", "Wren".into()),
                ("applicationName", "Kart Rally".into()),
            ],
            &["applicationIcon"],
        );
        assert_eq!(
            runs,
            [
                Run::Text("Wren".into(), true),
                Run::Text(" sent you a friend request from ".into(), false),
                Run::Hook("applicationIcon".into()),
                Run::Text(" ".into(), false),
                Run::Text("Kart Rally".into(), true),
                Run::Text(".".into(), false),
            ]
        );
        let runs = sentence("C98CSN", &[], &["applicationName", "applicationName2"]);
        assert_eq!(runs[1], Run::Hook("applicationName".into()));
        assert_eq!(runs[3], Run::Hook("applicationName2".into()));
    }

    #[test]
    fn the_game_friend_sentence_has_one_two_or_a_listed_form() {
        assert_eq!(game_friend_key(0), None);
        assert_eq!(game_friend_key(1), Some("wQ6urw"));
        assert_eq!(game_friend_key(2), Some("C98CSN"));
        assert_eq!(game_friend_key(5), Some("UxpwAh"));
        assert_eq!(
            game_relationship_route(7, 9),
            "/users/@me/game-relationships/7/9"
        );
    }
}
