//! Registered Games and the presence a detected game makes. Follows the settings panel of module 158935 (the
//! `REGISTERED_GAMES_*` sections, rows `ff`, the add popout `fv`), `RunningGameStore` (module 397369), and
//! `SelfPresenceStore`/`ConnectionStore` for the activity that is sent (modules 894768, 225123). Styles are the
//! module CSS `kQPUsd` (rows), `HLMBUf` (the add popout) and `XRlxvQ` (the "Not seeing your game?" line).

use super::settings::Choice;
use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::games::{self, GameScan, SendLimiter};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::running_games::{Game, RunningGames, key};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where `RunningGameStore` keeps its object in local storage (`persistKey`).
const STORAGE_KEY: &str = "RunningGameStore";
/// The popout's open state and chosen window, in the settings page's choices.
const ADD_OPEN: &str = "games_add_popout/open";
const ADD_SELECT: &str = "games_add_select";
/// `GAME_NAME` input: `maxLength: 128`.
const NAME_LIMIT: usize = 128;

pub(super) struct Games {
    runtime: games::Runtime,
    store: RunningGames,
    /// The activities the last presence update carried.
    /// The status and activities of the last presence update. The status is empty until one pass has seen it: the
    /// connection's own handshake already carried it.
    sent: (String, Vec<Value>),
    limiter: SendLimiter,
    connected: bool,
    /// The stored object as last written, so an unchanged scan writes nothing.
    saved: Value,
    /// The key of the row whose overflow menu is open.
    menu: RefCell<Option<String>>,
    /// The name inputs of games that are not in the database, by game key.
    editors: RefCell<HashMap<String, NameInput>>,
}

struct NameInput {
    editor: Entity<Editor>,
    name: String,
    _events: Subscription,
    _blur: Subscription,
}

impl Games {
    pub(super) fn new(updates: tokio::sync::mpsc::UnboundedSender<crate::net::Update>) -> Self {
        let stored = crate::storage::get(STORAGE_KEY).unwrap_or(Value::Null);
        let store = RunningGames::from_json(&stored);
        let runtime = games::Runtime::new(updates);
        runtime.set_overrides(overrides(&store));
        Self {
            runtime,
            saved: store.to_json(),
            store,
            sent: (String::new(), Vec::new()),
            limiter: SendLimiter::default(),
            connected: false,
            menu: RefCell::new(None),
            editors: RefCell::new(HashMap::new()),
        }
    }
}

fn overrides(store: &RunningGames) -> Vec<(String, String)> {
    store
        .overrides()
        .filter_map(|game| Some((game.exe_path.clone(), game.name.clone()?)))
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// moment's `fromNow()` in English, which `played` uses (module 397369 `ez`, moment is module 536637): its default
/// thresholds, 44 seconds, 45 minutes, 22 hours, 26 days, 11 months.
pub(super) fn from_now(last_focused: u64, now: u64) -> String {
    let seconds = now.saturating_sub(last_focused) as f64;
    let minutes = (seconds / 60.).round();
    let hours = (minutes / 60.).round();
    let days = (hours / 24.).round();
    // moment's month length is 146097 / 4800 days.
    let months = (days / (146_097. / 4_800.)).round();
    let years = (months / 12.).round();
    let phrase = if seconds <= 44. {
        "a few seconds".to_owned()
    } else if minutes <= 1. {
        "a minute".to_owned()
    } else if minutes < 45. {
        format!("{minutes} minutes")
    } else if hours <= 1. {
        "an hour".to_owned()
    } else if hours < 22. {
        format!("{hours} hours")
    } else if days <= 1. {
        "a day".to_owned()
    } else if days < 26. {
        format!("{days} days")
    } else if months <= 1. {
        "a month".to_owned()
    } else if months < 11. {
        format!("{months} months")
    } else if years <= 1. {
        "a year".to_owned()
    } else {
        format!("{years} years")
    };
    format!("{phrase} ago")
}

/// The Added Games subtitle: the IGDB credit once a game is listed, else "No games added".
const NO_GAMES: &[(&str, bool)] = &[("No games added", false)];
const IGDB: &[(&str, bool)] = &[
    (
        "Some information about games (such as genre or cover art) is provided by ",
        false,
    ),
    ("IGDB", true),
    (".", false),
];

impl Root {
    /// A gateway connection opened or closed: the lists are fetched when stale, the scan starts, and the
    /// presence goes out again (a new session starts without the activities).
    pub(super) fn detect_connection_changed(&mut self, connected: bool, cx: &mut Context<Self>) {
        self.detect.connected = connected;
        if !connected {
            return;
        }
        if !crate::storage::is_capture_run() {
            for (key, route) in self.detect.runtime.due_fetches(now_ms()) {
                let _ = self.requests.send(Request::Fetch {
                    key: key.to_owned(),
                    route: route.to_owned(),
                });
            }
            self.detect.runtime.start();
        }
        self.detect.sent = (String::new(), Vec::new());
        self.refresh_own_presence();
        cx.notify();
    }

    /// READY replaced the presence store's entries: the account's own is put back.
    pub(super) fn detect_ready(&mut self) {
        self.refresh_own_presence();
    }

    /// A detectable list's reply (`GAMES_DATABASE_UPDATE` and its siblings).
    pub(super) fn detect_lists_fetched(&mut self, key: &str, body: &Value) -> bool {
        self.detect.runtime.list_fetched(key, body, now_ms())
    }

    /// One scan: the running games change (`RUNNING_GAMES_CHANGE`), and the activity follows.
    pub(super) fn detect_scanned(&mut self, scan: GameScan, cx: &mut Context<Self>) {
        let now = now_ms();
        let games: Vec<Game> = scan
            .games
            .iter()
            .map(|found| Game {
                id: found.id.clone(),
                name: Some(found.name.clone()),
                exe_path: found.exe_path.clone(),
                // The window in front is the game being played: its last-focused time is now.
                last_focused: if scan.foreground == Some(found.pid) {
                    now / 1000
                } else {
                    0
                },
                pid: Some(found.pid),
                is_launcher: found.is_launcher,
                ..Game::default()
            })
            .collect();
        let candidates: Vec<Game> = scan
            .candidates
            .iter()
            .map(|found| Game {
                name: Some(found.name.clone()),
                exe_path: found.exe_path.clone(),
                pid: Some(found.pid),
                ..Game::default()
            })
            .collect();
        self.detect.store.set_candidates(candidates);
        self.detect.store.set_running(games, now);
        self.detect_changed(cx);
    }

    /// Persists the store when it changed, hands the hand-added games to the scanner, and updates the activity.
    fn detect_changed(&mut self, cx: &mut Context<Self>) {
        let stored = self.detect.store.to_json();
        if stored != self.detect.saved {
            crate::storage::set(STORAGE_KEY, stored.clone());
            self.detect.saved = stored;
            self.detect
                .runtime
                .set_overrides(overrides(&self.detect.store));
        }
        self.refresh_own_presence();
        cx.notify();
    }

    /// The account's custom status as the activity its presence carries (module 685396). A custom emoji counts only
    /// when the account may use it outside its server (`getUsableCustomEmojiById` with no server in view).
    fn own_custom_status_activity(&self) -> Option<Value> {
        let custom = self.stores.status_state()?.custom?;
        let emoji = if custom.emoji_id != 0 {
            self.stores
                .guilds
                .iter()
                .flat_map(|guild| guild.emojis.iter())
                .find(|emoji| emoji.id == custom.emoji_id && emoji.available)
                .filter(|emoji| {
                    let tier = self.premium();
                    tier.has(dc_state::premium::Feature::EmojisEverywhere)
                        && (!emoji.animated || tier.has(dc_state::premium::Feature::AnimatedEmojis))
                })
                .map(|emoji| games::StatusEmoji::Custom {
                    id: emoji.id,
                    name: emoji.name.clone(),
                    animated: emoji.animated,
                })
        } else if custom.emoji_name.is_empty() {
            None
        } else {
            Some(games::StatusEmoji::Unicode(custom.emoji_name.clone()))
        };
        games::custom_status_activity(&custom.text, custom.expires_at_ms, emoji, now_ms())
    }

    /// `SelfPresenceStore` and `ConnectionStore`: the account's own presence follows the visible game, and a
    /// changed activity list goes to the gateway as op 3 (never in a harness run, see `Request::Presence`).
    pub(super) fn refresh_own_presence(&mut self) {
        let show = self.pref_on("activity_privacy_setting", true);
        let status = self.stores.status.clone();
        let activities = games::shared_activities(
            self.own_custom_status_activity(),
            self.detect.store.activity(),
            show,
            &status,
        );
        if !status.is_empty() {
            self.stores
                .presence
                .set_own(games::presence_status(&status), activities.clone());
        }
        if status.is_empty() || !self.detect.connected || self.stores.current_user.is_none() {
            return;
        }
        let wire = games::presence_status(&status).to_owned();
        if !games::presence_changed(&self.detect.sent, &wire, &activities) {
            self.detect.sent.0 = wire;
            return;
        }
        if !self.detect.limiter.allow(now_ms()) {
            return;
        }
        self.detect.sent = (wire.clone(), activities.clone());
        let _ = self.requests.send(Request::Presence {
            status: wire,
            activities,
        });
    }

    /// The category's subtitle (`useSubtitle` of `REGISTERED_GAMES_ADDED_GAMES_CATEGORY`).
    pub(super) fn detect_description(
        &self,
        key: &str,
        fallback: &'static [(&'static str, bool)],
    ) -> &'static [(&'static str, bool)] {
        if key != "registered_games_added_games_category" {
            return fallback;
        }
        if self.detect.store.games_seen(false).is_empty() {
            NO_GAMES
        } else {
            IGDB
        }
    }

    pub(super) fn detect_custom(
        &self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match key {
            "registered_games_current_game_setting" => Some(self.current_game(window, cx)),
            "registered_games_added_games_setting" => Some(self.added_games(window, cx)),
            _ => None,
        }
    }

    /// `REGISTERED_GAMES_CURRENT_GAME_SETTING`: the card for the game being played, or "No game detected".
    fn current_game(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.detect.store.visible_game().cloned();
        let card = match &visible {
            None => div()
                .parity("registered-games-card")
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(16.))
                .p(px(16.))
                .rounded(px(12.))
                .bg(color("--background-mod-subtle"))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            name_line("No game detected", "--interactive-text-active")
                                .parity("registered-games-name"),
                        )
                        .child(self.add_it(window, cx)),
                )
                .into_any_element(),
            Some(game) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(self.game_row(game, true, window, cx))
                .child(self.add_it(window, cx))
                .into_any_element(),
        };
        div()
            .w_full()
            .min_h(px(94.))
            .pt(px(8.))
            .pb(px(8.))
            .child(card)
            .into_any_element()
    }

    /// `fO`: "Not seeing your game?" and the "Add it!" text button, with the popout under it.
    fn add_it(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let open = self.choice_open(ADD_OPEN);
        let candidates = self.detect.store.candidate_games();
        let options: Vec<gpui::SharedString> = candidates
            .iter()
            .map(|game| game.name.clone().unwrap_or_default().into())
            .collect();
        let selected = match self.setting_choices.get(ADD_SELECT) {
            Some(Choice::Index(index)) if *index < candidates.len() => Some(*index),
            _ => None,
        };
        let chosen_pid = selected.and_then(|index| candidates[index].pid);
        let popout = div()
            .id("registered-games-add-popout")
            .occlude()
            .absolute()
            .top(px(22.))
            .left(px(8.))
            .w(px(300.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.16),
                offset: gpui::point(px(0.), px(2.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(self.select_with_list(
                ADD_SELECT,
                "games_add_select/open",
                &options,
                usize::MAX,
                "Select",
                None,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(16.))
                    .child(
                        div()
                            .id("registered-games-add-cancel")
                            .child(super::settings::popout_button(
                                "registered-games-add-cancel-button",
                                "Cancel",
                                false,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.close_add_popout(cx))),
                    )
                    .child(
                        div()
                            .id("registered-games-add-confirm")
                            .when(chosen_pid.is_none(), |this| this.opacity(0.5))
                            .child(super::settings::popout_button(
                                "registered-games-add-confirm-button",
                                "Add Game",
                                true,
                            ))
                            .when_some(chosen_pid, |this, pid| {
                                this.on_click(
                                    cx.listener(move |this, _, _, cx| this.add_game(pid, cx)),
                                )
                            }),
                    ),
            );
        let _ = window;
        div()
            .parity("registered-games-add-line")
            .relative()
            .flex()
            .items_center()
            .self_start()
            .gap(px(4.))
            .text_size(px(14.))
            .line_height(px(14.))
            .text_color(color("--text-muted"))
            .child(div().child("Not seeing your game?"))
            .child(
                div()
                    .id("registered-games-add-it")
                    .cursor_pointer()
                    .text_color(color("--text-brand"))
                    .font_weight(FontWeight(500.))
                    .line_height(px(18.))
                    .child("Add it!")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let open = this.choice_open(ADD_OPEN);
                        this.setting_choices.insert(ADD_OPEN, Choice::On(!open));
                        this.setting_choices.remove(ADD_SELECT);
                        cx.notify();
                    })),
            )
            .when(open, |this| {
                this.child(gpui::deferred(popout).with_priority(3))
            })
            .into_any_element()
    }

    fn choice_open(&self, key: &'static str) -> bool {
        matches!(self.setting_choices.get(key), Some(Choice::On(true)))
    }

    fn close_add_popout(&mut self, cx: &mut Context<Self>) {
        self.setting_choices.insert(ADD_OPEN, Choice::On(false));
        self.setting_choices.remove(ADD_SELECT);
        cx.notify();
    }

    /// `RUNNING_GAME_ADD_OVERRIDE` for the chosen window.
    fn add_game(&mut self, pid: u32, cx: &mut Context<Self>) {
        self.detect.store.add_override(pid, now_ms());
        self.close_add_popout(cx);
        self.detect_changed(cx);
    }

    /// `fx`: the games seen, newest first, without the one on show.
    fn added_games(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let seen = self.detect.store.games_seen(false);
        if seen.is_empty() {
            return div().w_full().h(px(48.)).into_any_element();
        }
        let rows: Vec<AnyElement> = seen
            .iter()
            .map(|game| self.game_row(game, false, window, cx))
            .collect();
        div()
            .w_full()
            .pt(px(8.))
            .pb(px(32.))
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }

    /// `ff`: one game. The name (a check beside it for a game in the database, an input otherwise), when it was
    /// last played or "Now playing!", the overflow menu with Remove, and the detection switch.
    fn game_row(
        &self,
        game: &Game,
        now_playing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = key(game);
        let on_show = self
            .detect
            .store
            .visible_game()
            .is_some_and(|visible| key(visible) == id);
        let verified = game.id.is_some();
        let detect = self.detect.store.is_detection_enabled(game);
        let subtitle = if now_playing || on_show {
            "Now playing!".to_owned()
        } else if game.last_focused != 0 {
            format!(
                "Last played {}",
                from_now(game.last_focused, now_ms() / 1000)
            )
        } else {
            " ".to_owned()
        };
        let name = game.name.clone().unwrap_or_default();
        let name_part: AnyElement = if verified {
            div()
                .flex()
                .items_center()
                .child(name_line(
                    &name,
                    if now_playing {
                        "--interactive-text-active"
                    } else {
                        "--text-strong"
                    },
                ))
                .child(
                    div()
                        .ml(px(8.))
                        .size(px(18.))
                        .flex_none()
                        .rounded_full()
                        .bg(color("--background-brand"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("select_check", 14., "--white")),
                )
                .into_any_element()
        } else {
            self.name_input(&id, &name, now_playing, window, cx)
        };
        let removable = !now_playing && !on_show;
        let menu_open = self.detect.menu.borrow().as_deref() == Some(id.as_str());
        let group = gpui::SharedString::from(format!("registered-game-{id}"));
        let toggle_game = game.clone();
        let remove_game = game.clone();
        let menu_id = id.clone();
        div()
            .id(gpui::SharedString::from(format!(
                "registered-game-row-{id}"
            )))
            .group(group.clone())
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .min_h(px(40.))
            .when(!now_playing, |this| this.mb(px(16.)))
            .when(now_playing, |this| {
                this.w_full()
                    .p(px(16.))
                    .rounded(px(12.))
                    .bg(color("--background-mod-subtle"))
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(name_part)
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(subtitle),
                    ),
            )
            .when(removable, |this| {
                this.child(
                    div()
                        .relative()
                        .flex_none()
                        .when(!menu_open, |this| {
                            this.opacity(0.)
                                .group_hover(group.clone(), |style| style.opacity(1.))
                        })
                        .child(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "registered-game-more-{id}"
                                )))
                                .size(px(24.))
                                .rounded(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(|style| style.bg(color("--background-mod-normal")))
                                .child(icon("more_horizontal", 16., "--icon-default"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let mut menu = this.detect.menu.borrow_mut();
                                    *menu = if menu.as_deref() == Some(menu_id.as_str()) {
                                        None
                                    } else {
                                        Some(menu_id.clone())
                                    };
                                    drop(menu);
                                    cx.notify();
                                })),
                        )
                        .when(menu_open, |this| {
                            this.child(gpui::deferred(
                                div()
                                    .id("registered-game-menu")
                                    .occlude()
                                    .absolute()
                                    .top(px(28.))
                                    .right_0()
                                    .w(px(188.))
                                    .p(px(8.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-surface-higher"))
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        *this.detect.menu.borrow_mut() = None;
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .id("registered-game-remove")
                                            .h(px(32.))
                                            .px(px(8.))
                                            .rounded(px(4.))
                                            .flex()
                                            .items_center()
                                            .cursor_pointer()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-feedback-critical"))
                                            .hover(|style| {
                                                style.bg(color("--background-mod-normal"))
                                            })
                                            .child("Remove")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                *this.detect.menu.borrow_mut() = None;
                                                this.detect
                                                    .editors
                                                    .borrow_mut()
                                                    .remove(&key(&remove_game));
                                                this.detect
                                                    .store
                                                    .delete_entry(&remove_game, now_ms());
                                                this.detect_changed(cx);
                                            })),
                                    ),
                            ))
                        }),
                )
            })
            .child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "registered-game-switch-{id}"
                    )))
                    .ml(px(12.))
                    .flex_none()
                    .cursor_pointer()
                    .child(detection_switch(detect))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.detect.store.toggle_detection(&toggle_game, now_ms());
                        this.detect_changed(cx);
                    })),
            )
            .into_any_element()
    }

    /// `gameNameInput`: the name of a game that is not in the database is edited in place; Enter or leaving the
    /// field saves it (`editName`).
    fn name_input(
        &self,
        id: &str,
        name: &str,
        now_playing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut editors = self.detect.editors.borrow_mut();
        let stale = editors.get(id).is_some_and(|slot| slot.name != name);
        if stale {
            editors.remove(id);
        }
        if !editors.contains_key(id) {
            let editor = cx.new(|cx| {
                let mut style = editor_style();
                style.line_height = 22.;
                let mut input = Editor::new(style, cx);
                input.set_keeps_text(true);
                input.set_max_utf16(NAME_LIMIT);
                input.set_single_line(true);
                input.set_text(name);
                input
            });
            let row = id.to_owned();
            let events = cx.subscribe(&editor, {
                let row = row.clone();
                move |this, _, event: &EditorEvent, cx| {
                    if matches!(event, EditorEvent::Submit(_)) {
                        this.commit_name(&row, cx);
                    }
                }
            });
            let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
            let blur = cx.on_blur(&focus, window, {
                let row = row.clone();
                move |this, _, cx| this.commit_name(&row, cx)
            });
            editors.insert(
                id.to_owned(),
                NameInput {
                    editor,
                    name: name.to_owned(),
                    _events: events,
                    _blur: blur,
                },
            );
        }
        let editor = editors[id].editor.clone();
        let focused = gpui::Focusable::focus_handle(editor.read(cx), cx).is_focused(window);
        drop(editors);
        div()
            .h(px(24.))
            .w(px(240.))
            .ml(px(-5.))
            .px(px(4.))
            .rounded(px(3.))
            .border_1()
            .border_color(if focused {
                gpui::Hsla::from(color("--background-brand"))
            } else {
                gpui::transparent_black()
            })
            .when(focused, |this| this.bg(color("--background-mod-normal")))
            .hover(|style| style.bg(color("--background-mod-normal")))
            .text_color(color(if now_playing {
                "--interactive-text-active"
            } else {
                "--text-strong"
            }))
            .flex()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(editor))
            .into_any_element()
    }

    /// Saves the edited name of a game when it differs from the stored one.
    fn commit_name(&mut self, row: &str, cx: &mut Context<Self>) {
        let Some(typed) = self
            .detect
            .editors
            .borrow()
            .get(row)
            .map(|slot| slot.editor.read(cx).text().to_owned())
        else {
            return;
        };
        let typed = typed.trim().to_owned();
        let target = self
            .detect
            .store
            .games_seen(false)
            .into_iter()
            .find(|game| key(game) == row);
        if let Some(game) = target
            && !typed.is_empty()
            && game.name.as_deref() != Some(typed.as_str())
        {
            self.detect.store.edit_name(&game, &typed, now_ms());
            self.detect.editors.borrow_mut().remove(row);
            self.detect_changed(cx);
        }
    }
}

/// `gameName_kQPUsd`: 16px semibold on 22, in a 1px transparent border.
fn name_line(text: &str, tint: &str) -> gpui::Div {
    div()
        .px(px(1.))
        .py(px(1.))
        .text_size(px(16.))
        .line_height(px(22.))
        .font_weight(FontWeight(600.))
        .text_color(color(tint))
        .child(text.to_owned())
}

/// The detection switch, 48 by 24 as the settings switches are.
fn detection_switch(on: bool) -> impl IntoElement {
    div()
        .relative()
        .w(px(48.))
        .h(px(24.))
        .rounded(px(16.))
        .border_1()
        .border_color(color("--input-border-default"))
        .bg(if on {
            color("--background-brand")
        } else {
            color("--input-background-default")
        })
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(if on { 27. } else { 3. }))
                .size(px(16.))
                .rounded_full()
                .bg(gpui::white()),
        )
}

/// Harness only: a running game, two seen ones and a hand-added one, in memory (nothing is stored or sent).
#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_games_demo(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        let seen = |id: Option<&str>, name: &str, path: &str, ago_secs: u64| Game {
            id: id.map(str::to_owned),
            name: Some(name.to_owned()),
            exe_path: path.to_owned(),
            last_focused: now / 1000 - ago_secs,
            ..Game::default()
        };
        let mut store = RunningGames::default();
        store.set_running(Vec::new(), now - 1000);
        for (index, game) in [
            seen(Some("1"), "Hades", "c:/games/hades/hades.exe", 3 * 86_400),
            seen(
                Some("2"),
                "Celeste",
                "c:/games/celeste/celeste.exe",
                9 * 86_400,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            store.set_running(
                vec![Game {
                    pid: Some(index as u32 + 1),
                    ..game
                }],
                now - 1000,
            );
        }
        store.set_candidates(vec![Game {
            name: Some("mygame".into()),
            exe_path: "d:/mygame.exe".into(),
            pid: Some(9),
            ..Game::default()
        }]);
        store.set_running(Vec::new(), now);
        store.add_override(9, now - 2 * 3600 * 1000);
        store.set_running(
            vec![Game {
                id: Some("356".into()),
                name: Some("Minecraft".into()),
                exe_path: "c:/games/minecraft/minecraft.exe".into(),
                pid: Some(77),
                ..Game::default()
            }],
            now,
        );
        self.detect.store = store;
        cx.notify();
    }
}

/// The account's presence update for a game, as tests build it: the activity of the visible game, filtered as
/// `SelfPresenceStore` filters it.
#[cfg(test)]
fn activity_for(store: &RunningGames, show: bool, status: &str) -> Vec<Value> {
    games::shared_activities(None, store.activity(), show, status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_now_follows_moments_thresholds() {
        let at = |seconds: u64| from_now(1_000_000_000 - seconds, 1_000_000_000);
        assert_eq!(at(0), "a few seconds ago");
        assert_eq!(at(44), "a few seconds ago");
        assert_eq!(at(45), "a minute ago");
        assert_eq!(at(89), "a minute ago");
        assert_eq!(at(150), "3 minutes ago");
        assert_eq!(at(44 * 60), "44 minutes ago");
        assert_eq!(at(50 * 60), "an hour ago");
        assert_eq!(at(5 * 3600), "5 hours ago");
        assert_eq!(at(23 * 3600), "a day ago");
        assert_eq!(at(3 * 86_400), "3 days ago");
        assert_eq!(at(27 * 86_400), "a month ago");
        assert_eq!(at(100 * 86_400), "3 months ago");
        assert_eq!(at(400 * 86_400), "a year ago");
        assert_eq!(at(800 * 86_400), "2 years ago");
    }

    #[test]
    fn a_detected_game_makes_a_playing_activity_only_when_shared() {
        let mut store = RunningGames::default();
        store.set_running(
            vec![Game {
                id: Some("356".into()),
                name: Some("Minecraft".into()),
                exe_path: "c:/m.exe".into(),
                pid: Some(4),
                ..Game::default()
            }],
            1_700_000_000_000,
        );
        let on = activity_for(&store, true, "online");
        assert_eq!(on.len(), 1);
        assert_eq!(on[0]["type"], 0);
        assert!(activity_for(&store, false, "online").is_empty());
        let body = games::presence_body("online", &on);
        assert_eq!(body["activities"][0]["name"], "Minecraft");
        assert_eq!(body["activities"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_subtitle_switches_to_the_credit_once_a_game_is_listed() {
        assert_eq!(NO_GAMES[0].0, "No games added");
        assert_eq!(IGDB.len(), 3);
        assert!(IGDB[1].1);
    }
}
