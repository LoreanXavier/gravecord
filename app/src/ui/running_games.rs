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
    pub(super) spotify_running: bool,
    runtime: games::Runtime,
    pub(super) store: RunningGames,
    /// The activities the last presence update carried.
    /// The status and activities of the last presence update. The status is empty until one pass has seen it: the
    /// connection's own handshake already carried it.
    sent: (String, Vec<Value>),
    /// The `since` and `afk` of the last presence update (the handshake carries zero and false).
    sent_idle: (u64, bool),
    /// `IdleStore` now (module 885576), sampled every ten seconds, and the last moment the app saw the account act.
    idle: crate::idle::Idle,
    app_activity_ms: u64,
    /// The timers of `handleIdleUpdate` (module 596339 `Pf` and `PA`): leave the call, move to the AFK channel.
    /// Dropping them stops them.
    voice_idle: Vec<gpui::Task<()>>,
    limiter: SendLimiter,
    /// When an update the limiter refused is due again, and the due time a (detached) timer is set for.
    retry_at: Option<u64>,
    retry_timer: Option<u64>,
    /// The times the `W` and `Y` timers of module 655116 (the Spotify track's end, a stopped listen-along) are set
    /// for; a timer whose time is no longer the store's does nothing.
    pub(super) spotify_timers: [Option<i64>; 2],
    /// `LocalActivityStore`: the activities games set over the local RPC pipe.
    local: dc_state::local_activity::LocalActivityStore,
    connected: bool,
    /// The stored object as last written, so an unchanged scan writes nothing.
    saved: Value,
    /// The key of the row whose overflow menu is open.
    menu: RefCell<Option<String>>,
    /// The name inputs of games that are not in the database, by game key.
    editors: RefCell<HashMap<String, NameInput>>,
    #[cfg(windows)]
    pending_window_pid: Option<u32>,
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
            spotify_running: false,
            runtime,
            saved: store.to_json(),
            store,
            sent: (String::new(), Vec::new()),
            sent_idle: (0, false),
            idle: crate::idle::Idle::default(),
            app_activity_ms: now_ms(),
            voice_idle: Vec::new(),
            limiter: SendLimiter::default(),
            retry_at: None,
            retry_timer: None,
            spotify_timers: [None, None],
            local: dc_state::local_activity::LocalActivityStore::default(),
            connected: false,
            menu: RefCell::new(None),
            editors: RefCell::new(HashMap::new()),
            #[cfg(windows)]
            pending_window_pid: None,
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
        if !crate::storage::is_capture_run() || games::live_presence_allowed() {
            for fetch in self.detect.runtime.due_fetches_all(now_ms()) {
                let request = match fetch {
                    games::DueFetch::Cdn { key, url, etag } => Request::FetchCdn {
                        key: key.to_owned(),
                        url: url.to_owned(),
                        etag,
                    },
                    games::DueFetch::Api { key, route } => Request::Fetch {
                        key: key.to_owned(),
                        route: route.to_owned(),
                    },
                };
                let _ = self.requests.send(request);
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
        self.detect.spotify_running = scan.spotify_running;
        self.streamer_scanned(scan.streamer_tools);
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
    pub(super) fn detect_cdn_fetched(
        &mut self,
        key: &str,
        status: u16,
        etag: Option<String>,
        games: Option<std::sync::Arc<Vec<dc_detect::DetectableGame>>>,
    ) -> bool {
        self.detect
            .runtime
            .cdn_fetched(key, status, etag, games, now_ms())
    }

    pub(super) fn persist_detect_store(&mut self) {
        let stored = self.detect.store.to_json();
        if stored != self.detect.saved {
            crate::storage::set(STORAGE_KEY, stored.clone());
            self.detect.saved = stored;
            self.detect
                .runtime
                .set_overrides(overrides(&self.detect.store));
        }
        self.overlay.sync_dirty = true;
    }

    pub(in crate::ui) fn detect_changed(&mut self, cx: &mut Context<Self>) {
        self.persist_detect_store();
        // The clip path records the visible game's window (module 489496: `getVisibleGame()?.windowHandle`).
        #[cfg(windows)]
        if !self.clips.engine.test_game {
            let resolve = |game: &Game| {
                let pid = game.pid?;
                Some(crate::clips_capture::Game {
                    pid,
                    window: dc_detect::main_window(pid)?,
                    name: game.name.clone().unwrap_or_default(),
                    // Not in the pinned client: ClipsEnhancements' Rich Presence tagging
                    // (`plugin_hooks/clips_enhancements.rs`).
                    application_id: self.clip_application(
                        game.name.as_deref().unwrap_or_default(),
                        game.id.clone(),
                    ),
                })
            };
            let visible = self.detect.store.visible_game().cloned();
            let game = visible.as_ref().and_then(resolve).or_else(|| {
                self.detect
                    .store
                    .visible_running_games()
                    .into_iter()
                    .filter(|game| !game.is_launcher)
                    .find_map(resolve)
            });
            if game.is_some() {
                self.detect.pending_window_pid = None;
            } else if let Some(pid) = visible
                .as_ref()
                .filter(|game| !game.is_launcher)
                .and_then(|game| game.pid)
                && self.detect.pending_window_pid != Some(pid)
            {
                // Newly launched DX/Vulkan games may register before their render window opens.
                self.detect.pending_window_pid = Some(pid);
                cx.spawn(async move |this, cx| {
                    for secs in [1u64, 1, 2] {
                        cx.background_executor()
                            .timer(std::time::Duration::from_secs(secs))
                            .await;
                        let done = this
                            .update(cx, |this, cx| {
                                if this.detect.pending_window_pid != Some(pid) {
                                    return true;
                                }
                                let Some(vis) = this.detect.store.visible_game() else {
                                    this.detect.pending_window_pid = None;
                                    return true;
                                };
                                if vis.pid != Some(pid) {
                                    this.detect.pending_window_pid = None;
                                    return true;
                                }
                                if let Some(window) = dc_detect::main_window(pid) {
                                    let resolved = crate::clips_capture::Game {
                                        pid,
                                        window,
                                        name: vis.name.clone().unwrap_or_default(),
                                        application_id: vis.id.clone(),
                                    };
                                    this.detect.pending_window_pid = None;
                                    this.clips_game_changed(Some(resolved), cx);
                                    cx.notify();
                                    return true;
                                }
                                false
                            })
                            .unwrap_or(true);
                        if done {
                            return;
                        }
                    }
                    let _ = this.update(cx, |this, _| {
                        if this.detect.pending_window_pid == Some(pid) {
                            this.detect.pending_window_pid = None;
                        }
                    });
                })
                .detach();
            }
            self.clips_game_changed(game, cx);
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

    /// Not in the pinned client: CustomRPC's `LOCAL_ACTIVITY_UPDATE` with `socketId: "CustomRPC"`, listed after
    /// every game's socket (a string key after the numeric ones), and the presence follows
    /// (`plugin_hooks/custom_rpc.rs`).
    pub(super) fn set_plugin_activity(&mut self, activity: Option<serde_json::Value>) {
        if self.detect.local.update(u64::MAX, None, activity, None) {
            self.refresh_own_presence();
        }
    }

    /// An event of the local RPC server: `LOCAL_ACTIVITY_UPDATE` and `RPC_APP_DISCONNECTED` change
    /// `LocalActivityStore`, and the account's presence follows (module 491009). `RPC_APP_CONNECTED` changes no
    /// activity.
    pub(super) fn rpc_event(&mut self, event: dc_rpc::Event, cx: &mut Context<Self>) {
        let changed = match event {
            dc_rpc::Event::Connected { .. } => false,
            dc_rpc::Event::Activity {
                socket,
                pid,
                activity,
                party_privacy,
                ..
            } => self
                .detect
                .local
                .update(socket, pid, activity, party_privacy),
            dc_rpc::Event::Disconnected { socket } => self.detect.local.disconnected(socket),
        };
        if changed {
            self.refresh_own_presence();
            cx.notify();
        }
    }

    /// `SelfPresenceStore` and `ConnectionStore`: the account's own presence follows the visible game, and a
    /// changed activity list goes to the gateway as op 3 (never in a harness run, see `Request::Presence`).
    pub(super) fn refresh_own_presence(&mut self) {
        let show = self.pref_on("activity_privacy_setting", true);
        // A status set for a time that has run out is online again, even when the server's update was missed.
        let status = self.stores.status_at(now_ms());
        if status != self.stores.status && !status.is_empty() {
            self.stores.status = status.clone();
        }
        // What a game's handshake and `SET_ACTIVITY` read: the account, and the party privacy settings.
        crate::rpc::set_user(self.stores.current_user.as_ref().map(crate::rpc::user_json));
        crate::rpc::set_party_privacy(
            self.pref_on("activity_privacy_friends_join_setting", true),
            self.pref_on("activity_privacy_voice_join_setting", true),
        );
        let activities = games::shared_local_activities(
            &self.detect.local,
            self.own_custom_status_activity(),
            self.detect.store.activity(),
            show,
            &status,
        );
        let spotify = self
            .stores
            .spotify
            .should_show_activity(self.detect.idle.since_ms.is_some())
            .then(|| {
                let me = self.stores.current_user.as_ref()?.id;
                self.stores.spotify.activity(me)
            })
            .flatten();
        let activities = games::with_spotify(activities, spotify, &status);
        // Not in the pinned client: IgnoreActivities keeps the ignored ones off your status
        // (`plugin_hooks/ignore_activities.rs`).
        let activities = self.ignore_activities_filter(activities);
        if !status.is_empty() {
            // Invisible shows nothing, whatever another session reports.
            let shown = if activities.is_empty() && status == "invisible" {
                Vec::new()
            } else {
                games::own_visible_activities(&activities, self.stores.remote_activities())
            };
            self.stores
                .presence
                .set_own(crate::idle::wire_status(&status, &self.detect.idle), shown);
        }
        // Without the account's own settings its saved status is unknown: nothing goes out that would guess it.
        if status.is_empty()
            || !self.detect.connected
            || self.stores.current_user.is_none()
            || !self.stores.settings_loaded()
        {
            return;
        }
        // Module 894768: an online account that is idle goes out as `idle`, with `since` and `afk`.
        let wire = crate::idle::wire_status(&status, &self.detect.idle).to_owned();
        let idle = (self.detect.idle.since(), self.detect.idle.afk);
        if !games::presence_changed(&self.detect.sent, &wire, &activities)
            && self.detect.sent_idle == idle
        {
            self.detect.sent.0 = wire;
            self.detect.retry_at = None;
            return;
        }
        if !self.detect.limiter.allow(now_ms()) {
            // Module 225123: the refused update goes out once a slot is free (`schedule_presence_retry`).
            self.detect.retry_at = Some(now_ms() + self.detect.limiter.wait_ms(now_ms()));
            return;
        }
        self.detect.retry_at = None;
        self.detect.sent = (wire.clone(), activities.clone());
        self.detect.sent_idle = idle;
        let _ = self.requests.send(Request::Presence {
            status: wire,
            activities,
            since: idle.0,
            afk: idle.1,
        });
    }

    /// An update the limiter refused goes out when its slot frees (module 225123): one timer, moved when the due time
    /// moves, and a later update that went out first makes it a no-op.
    pub(super) fn schedule_presence_retry(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.detect.retry_at else {
            return;
        };
        if self.detect.retry_timer == Some(at) {
            return;
        }
        self.detect.retry_timer = Some(at);
        let wait = at.saturating_sub(now_ms());
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(wait))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.detect.retry_at != Some(at) {
                    return;
                }
                // A timer that woke a hair early is refused again for the same slot: it must be set again.
                this.detect.retry_timer = None;
                this.refresh_own_presence();
                this.schedule_presence_retry(cx);
            });
        })
        .detach();
    }

    /// `IdleStore` (module 885576): every ten seconds the system's input age, with what the app saw the account do,
    /// says whether it is idle or AFK; a change goes out as op 3 (`refresh_own_presence`). A lapsed timed status is
    /// noticed on the same beat.
    pub(super) fn idle_beat(&mut self, system_idle_ms: u64, cx: &mut Context<Self>) {
        let now = now_ms();
        let afk_seconds = self
            .stores
            .prefs
            .get("mobile_notification_delay")
            .copied()
            .unwrap_or(crate::idle::DEFAULT_AFK_SECONDS);
        let idle = crate::idle::Idle::sample(
            now,
            system_idle_ms,
            self.detect.app_activity_ms,
            afk_seconds,
        );
        let lapsed = self.stores.status_at(now) != self.stores.status;
        // `IDLE` is dispatched when the idle state flips (module 885576 `N`), and `handleIdleUpdate` runs on it.
        let flipped = idle.since_ms.is_some() != self.detect.idle.since_ms.is_some();
        if idle != self.detect.idle || lapsed {
            self.detect.idle = idle;
            self.refresh_own_presence();
            self.schedule_presence_retry(cx);
            cx.notify();
        }
        if flipped {
            self.handle_idle_update(cx);
        }
    }

    /// `handleIdleUpdate` (module 596339 `PI`): both timers stop; then, idle in a call with no stream live, the call
    /// is left after five hours, and in a server with an AFK channel the account moves there `afkTimeout` seconds
    /// after its last activity, a move that does not count as activity (`bypassIdleUpdate`).
    pub(super) fn handle_idle_update(&mut self, cx: &mut Context<Self>) {
        self.detect.voice_idle.clear();
        // Not in the pinned client: DisableCallIdle (Vencord `src/plugins/disableCallIdle`) removes this handler.
        if crate::plugin_on!("DisableCallIdle") {
            return;
        }
        let spot = self.call.as_ref().map(|call| crate::idle::VoiceSpot {
            guild: (call.guild != 0).then_some(call.guild),
            channel: call.channel,
            stage: self
                .stores
                .guild(call.guild)
                .and_then(|guild| guild.channels.iter().find(|known| known.id == call.channel))
                .is_some_and(|channel| channel.kind == dc_state::ChannelKind::Other(13)),
        });
        let afk = spot.and_then(|spot| {
            let guild = spot.guild?;
            Some((
                *self.stores.afk_channels.get(&guild)?,
                *self.stores.afk_timeouts.get(&guild)?,
            ))
        });
        let Some(plan) = crate::idle::voice_plan(
            now_ms(),
            self.detect.idle.since_ms,
            self.own_stream().is_some(),
            spot,
            afk,
        ) else {
            return;
        };
        self.detect.voice_idle.push(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(plan.leave_in_ms))
                .await;
            let _ = this.update(cx, |this, cx| {
                // `selectVoiceChannel(null)`.
                if this.call.is_some() {
                    this.leave_voice(cx);
                    cx.notify();
                }
            });
        }));
        if let (Some((_, delay)), Some(guild)) = (plan.afk, spot.and_then(|spot| spot.guild)) {
            self.detect.voice_idle.push(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay.max(0) as u64))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    // The server's AFK channel as it is now; the move keeps the idle clock where it was.
                    let Some(afk) = this.stores.afk_channels.get(&guild).copied() else {
                        return;
                    };
                    if this.call.as_ref().is_none_or(|call| call.guild != guild) {
                        return;
                    }
                    let activity = this.detect.app_activity_ms;
                    this.move_voice_quietly(guild, afk, cx);
                    this.detect.app_activity_ms = activity;
                    cx.notify();
                });
            }));
        }
    }

    /// The account did something here (a channel opened, a word spoken): the idle clock starts over.
    pub(super) fn note_activity(&mut self) {
        self.detect.app_activity_ms = now_ms();
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
                // Below the select's own list (priority 2 in `select_with_list`), which opens over the buttons.
                this.child(
                    gpui::deferred(super::keyboard::outside(
                        popout,
                        super::keyboard::Layer::AddGame,
                        cx,
                    ))
                    .with_priority(1),
                )
            })
            .into_any_element()
    }

    fn choice_open(&self, key: &'static str) -> bool {
        matches!(self.setting_choices.get(key), Some(Choice::On(true)))
    }

    pub(super) fn add_popout_is_open(&self) -> bool {
        self.choice_open(ADD_OPEN)
    }

    pub(super) fn close_add_popout(&mut self, cx: &mut Context<Self>) {
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
        // Not in the pinned client: IgnoreActivities' "Enable Activity" check (`plugin_hooks/ignore_activities.rs`).
        let activity_check = self.ignore_activities_check(game);
        let check_game = game.clone();
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
                                    )
                                    .children(activity_check.map(|checked| {
                                        div()
                                            .id("registered-game-enable-activity")
                                            .h(px(32.))
                                            .px(px(8.))
                                            .rounded(px(4.))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .cursor_pointer()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--interactive-text-default"))
                                            .hover(|style| {
                                                style.bg(color("--background-mod-normal"))
                                            })
                                            .child("Enable Activity")
                                            .child(super::context_menu::menu_check_box(checked))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.ignore_activities_toggle(&check_game, cx);
                                            }))
                                    })),
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
        // Two windows still unregistered, so the "Add it!" list has rows to show over the popout.
        store.set_candidates(vec![
            Game {
                name: Some("Notepad Game".into()),
                exe_path: "d:/notepad-game.exe".into(),
                pid: Some(10),
                ..Game::default()
            },
            Game {
                name: Some("Sandbox".into()),
                exe_path: "d:/sandbox.exe".into(),
                pid: Some(11),
                ..Game::default()
            },
        ]);
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
