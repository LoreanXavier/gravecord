//! Profile playback controls (960549/353411), device actions (981616) and premium dialog (990726).
use super::live_activity::Surface;
use super::motion::{self, Transitions as _};
use super::{Root, color, modal, tooltip};
use crate::{
    net::Request,
    parity::Parity,
    spotify::{Command, Operation, Reply},
};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px, svg};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

pub(super) const ICON_KEY: &str = "text:spotify-user-play-icon";
const METADATA_KEY: &str = "spotify-activity-metadata:";

#[derive(Default)]
pub(super) struct Actions {
    next: u64,
    pending: HashMap<u64, Pending>,
    pub premium_dialog: bool,
    /// When the listened-to host's Spotify activity went away (`j`, the five-minute grace of module 655116).
    host_gone_at: Option<std::time::Instant>,
    /// Since when the account has been speaking in a voice channel with Spotify playing (`V`), and the count of
    /// speaking changes, so a stale timer knows it is stale.
    speaking_since: Option<std::time::Instant>,
    speaking_turn: u64,
}

struct Pending {
    user: Id,
    activity: Value,
    sync: bool,
    surface: Surface,
    account: Option<String>,
    track_before: Option<String>,
    waiting_devices: Option<HashSet<String>>,
    loading: bool,
    checking_premium: bool,
}

/// Read only the two SVG paths from the pinned icon module; no downloaded code is executed.
fn user_play_icon(chunk: &str) -> Option<Vec<u8>> {
    if chunk.len() > 16 * 1024 {
        return None;
    }
    let module = chunk
        .split_once("835723(t,n,e)")?
        .1
        .split_once(",711589(")?
        .0;
    let paths: Vec<&str> = module
        .split("d:\"")
        .skip(1)
        .map(|part| part.split('"').next().unwrap_or_default())
        .collect();
    if paths.len() != 2
        || paths.iter().any(|path| {
            path.is_empty()
                || !path.bytes().all(|b| {
                    b.is_ascii_digit() || b"MmZzLlHhVvCcSsQqTtAaEe+-. ,\t\n\r".contains(&b)
                })
        })
    {
        return None;
    }
    Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><path d=\"{}\"/><path fill-rule=\"evenodd\" clip-rule=\"evenodd\" d=\"{}\"/></svg>", paths[0], paths[1]).into_bytes())
}

fn metadata_valid(metadata: &Value) -> bool {
    metadata["album_id"].is_string()
        && metadata["artist_ids"]
            .as_array()
            .is_none_or(|values| values.iter().all(Value::is_string))
        && metadata["button_urls"]
            .as_array()
            .is_none_or(|values| values.iter().all(Value::is_string))
        && dc_state::spotify::Resource::parse(metadata["type"].as_str().unwrap_or("track"))
            .is_some()
        && (metadata["type"].is_null() || metadata["type"].is_string())
}

/// 193433's ellipsis, scaled by .75 for the small button in 384209.
fn loading_dots(key: &str, tint: gpui::Rgba) -> AnyElement {
    use gpui::{Animation, AnimationExt as _};
    let dot = |index: usize| {
        div()
            .size(px(4.5))
            .mr(px(1.5))
            .flex()
            .items_center()
            .justify_center()
            .child(div().rounded_full().bg(tint).size(px(4.5)).with_animation(
                SharedString::from(format!("{key}-dot-{index}")),
                Animation::new(Duration::from_millis(1400)).repeat(),
                move |element, delta| {
                    let wave = super::nitro_purchase::pulse_wave(delta - 0.2 / 1.4 * index as f32);
                    element
                        .size(px(4.5 * (1. - 0.2 * wave)))
                        .opacity(1. - 0.7 * wave)
                },
            ))
    };
    div()
        .w(px(21.))
        .flex()
        .justify_center()
        .child(dot(0))
        .child(dot(1))
        .child(dot(2))
        .into_any_element()
}

impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn script_spotify_state(&self) -> String {
        format!(
            "pending={} loading={} premium={} connected={} active-device={} profile={}",
            self.spotify_actions.pending.len(),
            self.spotify_actions
                .pending
                .values()
                .filter(|pending| pending.loading)
                .count(),
            self.spotify_actions.premium_dialog,
            self.stores.spotify.has_connected_account(),
            self.stores.spotify.active_device().is_some(),
            self.profile.is_some()
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_spotify_premium(&mut self, cx: &mut Context<Self>) {
        self.spotify_actions.premium_dialog = true;
        cx.notify();
    }

    /// The listened-to host's presence (`syncWith(PresenceStore)` of module 655116): a new track or start plays
    /// from the host's position; no activity starts the five-minute grace, after which the listen-along stops and the
    /// shared track pauses (`ACTIVITY_SYNC_STOP`, `es`).
    pub(super) fn listen_along_presence(&mut self, event: &Value, cx: &mut Context<Self>) {
        use dc_state::spotify::SyncStep;
        match self
            .stores
            .spotify
            .presence_step(event, chrono::Utc::now().timestamp_millis())
        {
            SyncStep::Nothing => {}
            SyncStep::HostBack => self.spotify_actions.host_gone_at = None,
            SyncStep::Replay(plan) => {
                self.spotify_actions.host_gone_at = None;
                self.spotify_perform(0, Operation::Play(plan));
                cx.notify();
            }
            SyncStep::HostGone => {
                if self.spotify_actions.host_gone_at.is_some() {
                    return;
                }
                let since = std::time::Instant::now();
                self.spotify_actions.host_gone_at = Some(since);
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(
                            dc_state::spotify::LISTEN_ALONG_GRACE_MS,
                        ))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if this.spotify_actions.host_gone_at == Some(since) {
                            this.spotify_actions.host_gone_at = None;
                            if let Some(account) = this.stores.spotify.stop_sync() {
                                this.spotify_perform(0, Operation::Pause(account));
                            }
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
    }

    /// `ed` of module 655116, on the account's own `SPEAKING`: speaking in a voice channel while a Spotify track is
    /// playing starts a 30-second timer (`U`) that pauses it (`eo`, "Playback auto paused"); 100ms of silence (`H`)
    /// stops the timer.
    pub(super) fn spotify_speaking(&mut self, speaking: bool, cx: &mut Context<Self>) {
        // Not in the pinned client: SpotifyCrack's `noSpotifyAutoPause` (`plugin_hooks/spotify_crack.rs`).
        if super::plugin_hooks::spotify_no_auto_pause() {
            return;
        }
        self.spotify_actions.speaking_turn += 1;
        let turn = self.spotify_actions.speaking_turn;
        let playing = self.call.is_some() && self.stores.spotify.player().is_some();
        if speaking && playing {
            if self.spotify_actions.speaking_since.is_some() {
                return;
            }
            let since = std::time::Instant::now();
            self.spotify_actions.speaking_since = Some(since);
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(
                        dc_state::spotify::AUTO_PAUSE_MS,
                    ))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.spotify_actions.speaking_since != Some(since) {
                        return;
                    }
                    this.spotify_actions.speaking_since = None;
                    let account = this
                        .stores
                        .spotify
                        .active_device()
                        .map(|(account, _)| account.to_owned());
                    if let Some(account) = account {
                        tracing::info!("Spotify playback auto paused");
                        this.spotify_perform(0, Operation::Pause(account));
                    }
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            let _ = this.update(cx, |this, _| {
                if this.spotify_actions.speaking_turn == turn {
                    this.spotify_actions.speaking_since = None;
                }
            });
        })
        .detach();
    }

    /// `SPOTIFY_PLAYER_STATE`, `SPOTIFY_PLAYER_PLAY` and `USER_CONNECTIONS_UPDATE`: `SelfPresenceStore` recomputes
    /// (module 894768 `H`), so the account's presence carries the playing track or loses it, and the `W` and `Y`
    /// timers of module 655116 follow the store.
    pub(super) fn spotify_presence_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh_own_presence();
        self.schedule_presence_retry(cx);
        let now = chrono::Utc::now().timestamp_millis();
        let due = [
            self.stores.spotify.track_end_ms(),
            self.stores.spotify.listen_along_stop_ms(),
        ];
        for (slot, at) in due.into_iter().enumerate() {
            if self.detect.spotify_timers[slot] == at {
                continue;
            }
            self.detect.spotify_timers[slot] = at;
            let Some(at) = at else { continue };
            let wait = Duration::from_millis(u64::try_from(at - now).unwrap_or(0));
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| {
                    if this.detect.spotify_timers[slot] != Some(at) {
                        return;
                    }
                    this.detect.spotify_timers[slot] = None;
                    let now = chrono::Utc::now().timestamp_millis().max(at);
                    if slot == 0 {
                        // `W`: the track should have ended and no state followed (`Z`).
                        if !this.stores.spotify.expire_track(now) {
                            return;
                        }
                    } else {
                        // `Y`: "Stopping listening along", then `Z`.
                        let Some(pause) = this.stores.spotify.expire_listen_along(now) else {
                            return;
                        };
                        tracing::info!(
                            "Spotify listen along stopped: playback stopped or track changed"
                        );
                        if let Some(account) = pause {
                            this.spotify_perform(0, Operation::Pause(account));
                        }
                    }
                    this.spotify_presence_changed(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    fn spotify_perform(&self, request: u64, operation: Operation) {
        let _ = self
            .requests
            .send(Request::Spotify(Command::Perform { request, operation }));
    }

    pub(super) fn spotify_clicked(
        &mut self,
        user: Id,
        activity: &Value,
        sync: bool,
        surface: Surface,
        cx: &mut Context<Self>,
    ) {
        let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
        let controls = self.stores.spotify.controls(me, user, activity);
        if if sync {
            controls.sync_disabled()
        } else {
            controls.play_disabled()
        } {
            return;
        }
        self.spotify_actions.next += 1;
        let request = self.spotify_actions.next;
        let account = self
            .stores
            .spotify
            .active_device()
            .map(|(account, _)| account.to_owned());
        self.spotify_actions.pending.insert(
            request,
            Pending {
                user,
                activity: activity.clone(),
                sync,
                surface,
                account,
                track_before: self.stores.spotify.current_track_id().map(str::to_owned),
                waiting_devices: None,
                loading: true,
                checking_premium: false,
            },
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(30))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.spotify_actions.pending.remove(&request);
                cx.notify();
            });
        })
        .detach();
        if !controls.has_account {
            self.connection_authorize("spotify", cx);
        } else if self.stores.spotify.active_device().is_some() {
            self.spotify_device_ready(request, cx);
        } else {
            let devices: Vec<(String, String)> = self
                .stores
                .spotify
                .playable_computers()
                .into_iter()
                .map(|(account, device)| (account.to_owned(), device.id.clone()))
                .collect();
            if self.detect.spotify_running
                && let Some((account, device)) = devices.first()
            {
                self.stores.spotify.dispatch(
                    "SPOTIFY_SET_ACTIVE_DEVICE",
                    &serde_json::json!({"accountId":account,"deviceId":device}),
                    chrono::Utc::now().timestamp_millis(),
                );
                self.spotify_device_ready(request, cx);
            } else {
                self.spotify_actions
                    .pending
                    .get_mut(&request)
                    .unwrap()
                    .waiting_devices =
                    Some(devices.into_iter().map(|(_, device)| device).collect());
                if crate::storage::is_capture_run() {
                    crate::inert::note("skipped launch Spotify protocol handler");
                } else {
                    cx.open_url("spotify:");
                }
            }
        }
        cx.notify();
    }

    fn spotify_device_ready(&mut self, request: u64, cx: &mut Context<Self>) {
        let Some((account, _)) = self.stores.spotify.active_device() else {
            return;
        };
        let Some(pending) = self.spotify_actions.pending.get_mut(&request) else {
            return;
        };
        pending.account = Some(account.to_owned());
        pending.waiting_devices = None;
        let sync = pending.sync;
        let activity = pending.activity.clone();
        let user = pending.user;
        if sync {
            if self.stores.spotify.active_premium() == Some(true) {
                self.spotify_begin_sync(request, cx);
            } else if let Some(account) = self.spotify_actions.pending[&request].account.clone() {
                self.spotify_actions
                    .pending
                    .get_mut(&request)
                    .unwrap()
                    .checking_premium = true;
                self.spotify_perform(request, Operation::Premium(account));
            }
        } else {
            let metadata = activity["metadata"].clone();
            if metadata
                .as_object()
                .is_some_and(|object| !object.is_empty())
            {
                self.spotify_metadata(request, &metadata, cx);
            } else if let Some(session) = activity["session_id"].as_str() {
                let application = activity["application_id"].as_str().unwrap_or("0");
                let encode = |s: &str| {
                    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC)
                        .to_string()
                };
                let _ = self.requests.send(Request::Fetch {
                    key: format!("{METADATA_KEY}{request}"),
                    route: format!(
                        "/users/{user}/sessions/{}/activities/{}/metadata",
                        encode(session),
                        encode(application)
                    ),
                });
            } else {
                self.spotify_metadata(request, &Value::Null, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn spotify_devices_changed(&mut self, cx: &mut Context<Self>) {
        if !self
            .spotify_actions
            .pending
            .values()
            .any(|pending| pending.waiting_devices.is_some())
        {
            return;
        }
        let devices: Vec<(String, String)> = self
            .stores
            .spotify
            .playable_computers()
            .into_iter()
            .map(|(account, device)| (account.to_owned(), device.id.clone()))
            .collect();
        let ready: Vec<(u64, String, String)> = self
            .spotify_actions
            .pending
            .iter()
            .filter_map(|(request, pending)| {
                let previous = pending.waiting_devices.as_ref()?;
                devices
                    .iter()
                    .find(|(_, device)| !previous.contains(device))
                    .map(|(account, device)| (*request, account.clone(), device.clone()))
            })
            .collect();
        for (request, account, device) in ready {
            self.stores.spotify.dispatch(
                "SPOTIFY_SET_ACTIVE_DEVICE",
                &serde_json::json!({"accountId":account,"deviceId":device}),
                chrono::Utc::now().timestamp_millis(),
            );
            self.spotify_device_ready(request, cx);
        }
    }

    fn spotify_begin_sync(&mut self, request: u64, cx: &mut Context<Self>) {
        if let Some(pending) = self.spotify_actions.pending.get_mut(&request) {
            pending.checking_premium = false;
        }
        let Some(pending) = self.spotify_actions.pending.get(&request) else {
            return;
        };
        if self
            .stores
            .spotify
            .active_device()
            .map(|(account, _)| account)
            != pending.account.as_deref()
        {
            self.spotify_actions.pending.remove(&request);
            return;
        }
        if let Some(plan) = self.stores.spotify.start_sync(
            pending.user,
            &pending.activity,
            chrono::Utc::now().timestamp_millis(),
        ) {
            self.spotify_perform(request, Operation::Play(plan));
        }
        self.spotify_loading_changed();
        cx.notify();
    }

    fn spotify_metadata(&mut self, request: u64, metadata: &Value, cx: &mut Context<Self>) {
        let Some(pending) = self.spotify_actions.pending.get(&request) else {
            return;
        };
        let plan = metadata_valid(metadata)
            .then(|| self.stores.spotify.plan_play(&pending.activity, metadata))
            .flatten();
        if let Some(account) = self.stores.spotify.stop_sync() {
            self.spotify_perform(0, Operation::Pause(account));
        }
        if let Some(plan) = plan {
            self.spotify_perform(request, Operation::Play(plan));
        }
        cx.notify();
    }

    pub(super) fn spotify_metadata_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(request) = key
            .strip_prefix(METADATA_KEY)
            .and_then(|id| id.parse().ok())
        else {
            return false;
        };
        self.spotify_metadata(request, body, cx);
        true
    }

    pub(super) fn spotify_reply(&mut self, request: u64, reply: Reply, cx: &mut Context<Self>) {
        if !self.spotify_actions.pending.contains_key(&request) {
            return;
        }
        match reply {
            Reply::Premium(true) => self.spotify_begin_sync(request, cx),
            Reply::Premium(false) => {
                self.spotify_actions.pending.remove(&request);
                self.spotify_actions.premium_dialog = true;
            }
            Reply::Failed => {
                if self.spotify_actions.pending[&request].checking_premium {
                    self.spotify_actions.premium_dialog = true;
                }
                self.spotify_actions.pending.remove(&request);
            }
            Reply::Refused => {
                self.spotify_actions.pending.remove(&request);
            }
            Reply::Played | Reply::Paused => {}
        }
        cx.notify();
    }

    pub(super) fn spotify_loading_changed(&mut self) {
        let track = self.stores.spotify.current_track_id();
        let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
        for pending in self.spotify_actions.pending.values_mut() {
            let controls = self
                .stores
                .spotify
                .controls(me, pending.user, &pending.activity);
            if track != pending.track_before.as_deref() || controls.same_user || controls.same_party
            {
                pending.loading = false;
            }
        }
    }

    pub(super) fn spotify_render_lifecycle(&mut self) {
        let mut visible = Vec::new();
        if self.settings.is_none() {
            if let Some(profile) = &self.profile {
                visible.push((profile.user, Surface::Popout));
            }
            if let Some(profile) = &self.full_profile {
                visible.push((profile.user, Surface::Modal));
            }
            if self.show_dm_profile
                && let Some(user) = self
                    .current_dm()
                    .and_then(|channel| self.stores.dm_recipient(channel))
            {
                visible.push((user.id, Surface::Sidebar));
            }
        }
        for pending in self.spotify_actions.pending.values_mut() {
            if !visible.contains(&(pending.user, pending.surface))
                || !self
                    .stores
                    .presence
                    .get(pending.user, self.guild)
                    .is_some_and(|presence| {
                        presence
                            .visible_activities()
                            .iter()
                            .any(|activity| activity["sync_id"] == pending.activity["sync_id"])
                    })
            {
                pending.loading = false;
            }
        }
    }

    pub(super) fn spotify_buttons(
        &self,
        user: Id,
        activity: &Value,
        surface: Surface,
        palette: &super::dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let large = surface == Surface::Modal;
        let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
        let controls = self.stores.spotify.controls(me, user, activity);
        let name = self
            .stores
            .users
            .get(&user)
            .map_or("", |u| u.display_name());
        let make = |sync: bool, cx: &mut Context<Self>| {
            let key = format!("spotify-action-{user}-{sync}-{surface:?}");
            let loading = self.spotify_actions.pending.values().any(|p| {
                p.loading
                    && p.surface == surface
                    && p.user == user
                    && p.sync == sync
                    && p.activity["sync_id"] == activity["sync_id"]
            });
            let disabled = !loading
                && if sync {
                    controls.sync_disabled()
                } else {
                    controls.play_disabled()
                };
            let text = if sync {
                dc_intl::string("eU3inB").into_owned()
            } else {
                dc_intl::format(
                    if controls.has_account {
                        "LEgD7t"
                    } else {
                        "XWSHTb"
                    },
                    &[("platform", "Spotify".into())],
                )
            };
            let tip = if controls.own {
                Some(dc_intl::string(if sync { "AOU/CR" } else { "ZpDeUC" }).into_owned())
            } else if sync && (controls.same_user || controls.same_party) {
                Some(dc_intl::string("m8CScv").into_owned())
            } else if !sync && controls.same_track {
                Some(dc_intl::string("TYSymS").into_owned())
            } else if controls.not_playable {
                Some(dc_intl::format("SqJBnN", &[("name", "Spotify".into())]))
            } else if sync {
                Some(dc_intl::format("CT9xYI", &[("name", name.into())]))
            } else {
                None
            };
            let clicked = activity.clone();
            let keyboard = activity.clone();
            let focus = self
                .stop(&key, cx)
                .tab_index(0)
                .tab_stop(!disabled && !loading);
            let clicked_focus = focus.clone();
            let key_focus = focus.clone();
            let mut button =
                super::keyboard::ring_when_focused(div().id(SharedString::from(key.clone())))
                    .track_focus(&focus)
                    .h_full()
                    // In the full profile (`user-profile-modal-v2 .actions`, wrapping) the buttons are as wide as
                    // their text: Listen Along 99px, Connect Spotify 122px at 1280x720.
                    .when(!large, |b| b.w_full())
                    .when(!sync || large, |b| b.px(px(11.)))
                    .border_1()
                    .border_color(palette.token("--control-secondary-border-default"))
                    .rounded(px(8.))
                    .bg(palette.token("--control-secondary-background-default"))
                    .text_color(palette.token("--control-secondary-text-default"))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .when(disabled, |b| b.opacity(0.5).cursor_not_allowed())
                    .when(!disabled && !loading, |b| {
                        b.cursor_pointer()
                            .hover(|s| {
                                s.bg(palette.token("--control-secondary-background-hover"))
                                    .border_color(palette.token("--control-secondary-border-hover"))
                                    .text_color(palette.token("--control-secondary-text-hover"))
                            })
                            .active(|s| {
                                s.bg(palette.token("--control-secondary-background-active"))
                                    .border_color(
                                        palette.token("--control-secondary-border-active"),
                                    )
                                    .text_color(palette.token("--control-secondary-text-active"))
                            })
                    })
                    .rule(motion::Rule::Button)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if !disabled && !loading {
                            clicked_focus.focus(window, cx);
                            this.spotify_clicked(user, &clicked, sync, surface, cx);
                        }
                    }))
                    .on_key_down(cx.listener(
                        move |this, event: &gpui::KeyDownEvent, window, cx| {
                            if key_focus.is_focused(window)
                                && matches!(event.keystroke.key.as_str(), "enter" | "space")
                            {
                                cx.stop_propagation();
                                if !disabled && !loading {
                                    this.spotify_clicked(user, &keyboard, sync, surface, cx);
                                }
                            }
                        },
                    ));
            if loading {
                button = button.child(loading_dots(
                    &key,
                    palette.token("--control-secondary-text-default"),
                ));
            } else if sync && !large {
                if let Some(bytes) = self
                    .texts
                    .get(ICON_KEY)
                    .and_then(|text| user_play_icon(text))
                {
                    button = button.child(
                        svg()
                            .data(&bytes)
                            .size(px(16.))
                            .text_color(palette.token("--control-secondary-text-default")),
                    );
                }
            } else {
                if !large {
                    button = button.child(
                        svg()
                            .path("icons/active_now_spotify.svg")
                            .size(px(16.))
                            .text_color(palette.token("--control-secondary-text-default")),
                    );
                }
                button = button.child(text);
            }
            let content = match tip {
                Some(tip) => self
                    .with_tip(
                        key,
                        tooltip::Tip::body(tooltip::Body::Text(tip.into()), tooltip::Side::Top),
                        button,
                        cx,
                    )
                    .h_full()
                    .when(!large, |tip| tip.w_full())
                    .into_any_element(),
                None => button.into_any_element(),
            };
            div()
                .parity(if sync {
                    "live-spotify-sync"
                } else {
                    "live-spotify-play"
                })
                .h(px(32.))
                .when(sync && !large, |b| b.w(px(32.)).flex_none())
                .when(!sync && !large, |b| b.flex_1().min_w_0())
                .when(large, |b| b.flex_none())
                .child(content)
                .into_any_element()
        };
        let mut row = div().flex().gap(px(8.)).when(large, |row| row.flex_wrap());
        if large {
            row = row.child(make(true, cx)).child(make(false, cx));
        } else {
            row = row.child(make(false, cx)).child(make(true, cx));
        }
        row.into_any_element()
    }

    pub(super) fn spotify_premium_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.spotify_actions.premium_dialog {
            return None;
        }
        Some(modal::render_small_over_popouts(
            modal::Spec {
                id: "spotify-premium",
                title: dc_intl::string("DQsSGr").into_owned().into(),
                subtitle: Some(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .parity("spotify-premium-subtitle")
                        .text_color(color("--text-subtle"))
                        .child(dc_intl::string("xGMwi9").into_owned())
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    modal::button(
                        "spotify-premium-cancel",
                        dc_intl::string("ETE/oC").into_owned(),
                        modal::Tone::Secondary,
                        |this, _, cx| {
                            this.spotify_actions.premium_dialog = false;
                            cx.notify();
                        },
                    ),
                    modal::button(
                        "spotify-premium-upgrade",
                        dc_intl::string("n8U5jd").into_owned(),
                        modal::Tone::Primary,
                        |this, _, cx| {
                            this.spotify_actions.premium_dialog = false;
                            this.follow_link("https://www.spotify.com/premium/?utm_source=discord&utm_medium=desktop", "Spotify", cx);
                            cx.notify();
                        },
                    ),
                ],
                close: Box::new(|this, _, cx| {
                    this.spotify_actions.premium_dialog = false;
                    cx.notify();
                }),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downloaded_icon_is_data_only_and_requires_the_two_pinned_paths() {
        let chunk = "835723(t,n,e){d:\"M0 0L1 1Z\",d:\"M2 2Z\"},711589(";
        let svg = String::from_utf8(user_play_icon(chunk).unwrap()).unwrap();
        assert!(svg.contains("fill-rule=\"evenodd\""));
        assert!(user_play_icon(&chunk.replace("M2 2Z", "<script>")).is_none());
        assert!(user_play_icon("835723(t,n,e){d:\"M0 0Z\"},711589(").is_none());
    }

    #[test]
    fn metadata_requires_the_source_types_before_a_play_request() {
        use serde_json::json;
        assert!(metadata_valid(&json!({"album_id":"a","artist_ids":["b"]})));
        assert!(!metadata_valid(&json!({"album_id":"a","type":"invalid"})));
        assert!(!metadata_valid(
            &json!({"album_id":"a","artist_ids":[null]})
        ));
        assert!(!metadata_valid(&Value::Null));
    }
}
