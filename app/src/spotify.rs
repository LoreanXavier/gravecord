//! The login-scoped Spotify dealer reader (655116). Capture runs never start provider sessions.

use crate::net::Update;
use dc_http::{
    Client,
    spotify::{Dealer, Error, LinkedAccount, Spotify},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, mpsc},
    task::{JoinHandle, JoinSet},
    time::Instant,
};

pub enum Command {
    Accounts(Vec<LinkedAccount>),
    Refresh,
    Perform { request: u64, operation: Operation },
}

pub enum Operation {
    Play(dc_state::spotify::Play),
    Premium(String),
    Pause(String),
}

impl Operation {
    fn account(&self) -> &str {
        match self {
            Self::Play(plan) => &plan.account_id,
            Self::Premium(account) | Self::Pause(account) => account,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Played,
    Premium(bool),
    Paused,
    Failed,
    Refused,
}

fn reply(updates: &mpsc::UnboundedSender<Update>, request: u64, reply: Reply) {
    let _ = updates.send(Update::SpotifyReply { request, reply });
}

struct OwnedTask(JoinHandle<()>);

impl Drop for OwnedTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct Worker {
    commands: mpsc::UnboundedSender<Command>,
    _task: OwnedTask,
}

impl Worker {
    pub fn start(client: Arc<Client>, updates: mpsc::UnboundedSender<Update>) -> Option<Self> {
        if crate::storage::is_capture_run() {
            tracing::info!("Spotify provider sessions disabled in capture runs");
            return None;
        }
        if cfg!(test) {
            return None;
        }
        let (commands, receiver) = mpsc::unbounded_channel();
        Some(Self {
            commands,
            _task: OwnedTask(tokio::spawn(run(client, updates, receiver))),
        })
    }

    pub fn observer(&self) -> Observer {
        Observer(self.commands.clone())
    }

    pub fn request(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

#[derive(Clone)]
pub struct Observer(mpsc::UnboundedSender<Command>);

fn observed(name: &str, data: &Value) -> Option<Command> {
    match name {
        "READY" => Some(Command::Accounts(linked_accounts(
            &data["connected_accounts"],
        ))),
        "USER_CONNECTIONS_UPDATE" => Some(Command::Refresh),
        _ => None,
    }
}

pub fn linked_accounts(accounts: &Value) -> Vec<LinkedAccount> {
    accounts
        .as_array()
        .into_iter()
        .flatten()
        .filter(|account| account["type"] == "spotify")
        .filter_map(|account| {
            Some(LinkedAccount {
                id: account["id"].as_str()?.into(),
                show_activity: account["show_activity"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

impl Observer {
    pub fn event(&self, name: &str, data: &Value) {
        if let Some(command) = observed(name, data) {
            let _ = self.0.send(command);
        }
    }
}

fn dispatch(updates: &mpsc::UnboundedSender<Update>, name: &'static str, data: Value) {
    let _ = updates.send(Update::Dispatch {
        name: name.into(),
        data,
    });
}

fn socket_state(updates: &mpsc::UnboundedSender<Update>, account: &str, connected: bool) {
    let _ = updates.send(Update::SpotifySocket {
        account: account.into(),
        connected,
    });
}

async fn run(
    client: Arc<Client>,
    updates: mpsc::UnboundedSender<Update>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    dispatch(
        &updates,
        "SPOTIFY_SET_PROTOCOL_REGISTERED",
        json!({"isRegistered": protocol_registered()}),
    );
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut retry_at = None;
    loop {
        let command = tokio::select! {
            command = commands.recv() => match command { Some(command) => command, None => break },
            _ = async { if let Some(at) = retry_at { tokio::time::sleep_until(at).await; } else { std::future::pending::<()>().await; } } => Command::Refresh,
        };
        if !matches!(&command, Command::Perform { .. }) {
            retry_at = None;
        }
        let accounts = match command {
            Command::Accounts(accounts) => accounts,
            Command::Refresh => match dc_http::spotify::connected_accounts(&client).await {
                Ok(accounts) => accounts,
                Err(Error::RateLimited(wait)) => {
                    retry_at = Instant::now().checked_add(wait);
                    continue;
                }
                Err(_) => continue,
            },
            Command::Perform { request, operation } => {
                if let Some(session) = sessions.get(operation.account())
                    && !session.task.0.is_finished()
                    && session.commands.send((request, operation)).is_ok()
                {
                    continue;
                }
                reply(&updates, request, Reply::Failed);
                continue;
            }
        };
        let metadata: Vec<Value> = accounts
            .iter()
            .map(|account| {
                json!({"id":account.id,
            "type":"spotify", "show_activity":account.show_activity})
            })
            .collect();
        dispatch(
            &updates,
            "USER_CONNECTIONS_UPDATE",
            json!({"local":true,"accounts":metadata}),
        );
        sessions.retain(|id, task| {
            accounts.iter().any(|account| account.id == *id) && !task.task.0.is_finished()
        });
        for account in accounts {
            if let std::collections::hash_map::Entry::Vacant(entry) =
                sessions.entry(account.id.clone())
                && let Some(session) = Session::start(client.clone(), updates.clone(), account.id)
            {
                entry.insert(session);
            }
        }
    }
}

#[cfg(windows)]
fn protocol_registered() -> bool {
    use windows::{
        Win32::UI::Shell::{
            ASSOCF_IS_PROTOCOL, ASSOCF_NOFIXUPS, ASSOCSTR_EXECUTABLE, AssocQueryStringW,
        },
        core::{PWSTR, w},
    };
    let mut length = 0;
    // The first query sizes the UTF-16 output; the second resolves the registered protocol handler.
    unsafe {
        let _ = AssocQueryStringW(
            ASSOCF_IS_PROTOCOL | ASSOCF_NOFIXUPS,
            ASSOCSTR_EXECUTABLE,
            w!("spotify"),
            w!("open"),
            None,
            &mut length,
        );
        if !(2..=32768).contains(&length) {
            return false;
        }
        let mut buffer = vec![0u16; length as usize];
        AssocQueryStringW(
            ASSOCF_IS_PROTOCOL | ASSOCF_NOFIXUPS,
            ASSOCSTR_EXECUTABLE,
            w!("spotify"),
            w!("open"),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut length,
        )
        .is_ok()
    }
}

#[cfg(not(windows))]
fn protocol_registered() -> bool {
    false
}

struct Session {
    commands: mpsc::UnboundedSender<(u64, Operation)>,
    task: OwnedTask,
}

impl Session {
    fn start(
        client: Arc<Client>,
        updates: mpsc::UnboundedSender<Update>,
        account: String,
    ) -> Option<Self> {
        let notices = updates.clone();
        let linked_account = account.clone();
        let api = Spotify::new(client, account.clone())
            .ok()?
            .on_credential(move || {
                dispatch(
                    &notices,
                    "SPOTIFY_ACCOUNT_ACCESS_TOKEN",
                    json!({"accountId":linked_account}),
                );
            });
        let api = Arc::new(Mutex::new(api));
        let (commands, receiver) = mpsc::unbounded_channel();
        let task = OwnedTask(tokio::spawn(async move {
            tokio::select! {
                _ = session(api.clone(), updates.clone(), account.clone()) => {},
                _ = perform(api, updates, account, receiver) => {},
            }
        }));
        Some(Self { commands, task })
    }
}

/// The item a `GET /me/player` answer plays: its own id, or the id it was relinked from.
fn playing_item(state: &Value) -> Option<String> {
    let item = &state["item"];
    item["linked_from"]["id"]
        .as_str()
        .or_else(|| item["id"].as_str())
        .map(str::to_owned)
}

/// Whether the account's player now plays `track`.
async fn plays(api: &mut Spotify, track: &str) -> bool {
    for wait in [1500u64, 1500, 2000] {
        tokio::time::sleep(Duration::from_millis(wait)).await;
        if let Ok(state) = api.player().await
            && state["is_playing"] == true
            && playing_item(&state).as_deref() == Some(track)
        {
            return true;
        }
    }
    false
}

/// The official client sends one `PUT /me/player/play` and trusts it; recent Spotify desktop builds accept that call
/// and play nothing, so listening along only works with the web player. This client checks the player afterwards and
/// falls back: first Spotify Connect's transfer to the same device, then, when that device is this computer's Spotify
/// app, the track's `spotify:` address opened in the app itself, followed by a seek to where the host is by then.
async fn play_verified(api: &mut Spotify, plan: &dc_state::spotify::Play) -> Result<(), Error> {
    let started = Instant::now();
    api.play(&plan.device_id, &plan.body(), plan.repeat).await?;
    if plan.resource != dc_state::spotify::Resource::Track || plays(api, &plan.track_id).await {
        return Ok(());
    }
    tracing::info!("Spotify play request did not take; transferring playback");
    let position = |started: Instant| plan.position_ms + started.elapsed().as_millis() as i64;
    if api.transfer(&plan.device_id).await.is_ok() {
        let mut retry = plan.clone();
        retry.position_ms = position(started);
        if api.play(&plan.device_id, &retry.body(), None).await.is_ok()
            && plays(api, &plan.track_id).await
        {
            return Ok(());
        }
    }
    let desktop = api.devices().await.ok().is_some_and(|devices| {
        devices["devices"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|device| {
                device["id"].as_str() == Some(plan.device_id.as_str())
                    && device["type"] == "Computer"
            })
    });
    // The id comes from another person's presence: only a Spotify base-62 id is handed to the system.
    let base62 = !plan.track_id.is_empty()
        && plan.track_id.len() <= 32
        && plan.track_id.chars().all(|c| c.is_ascii_alphanumeric());
    if !desktop || !base62 {
        return Ok(());
    }
    tracing::info!("Spotify desktop app ignored the request; opening the track in the app");
    let uri = format!("spotify:track:{}", plan.track_id);
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(&uri).spawn();
    if plays(api, &plan.track_id).await {
        let _ = api.seek(position(started), Some(&plan.device_id)).await;
    }
    Ok(())
}

async fn perform(
    api: Arc<Mutex<Spotify>>,
    updates: mpsc::UnboundedSender<Update>,
    account: String,
    mut receiver: mpsc::UnboundedReceiver<(u64, Operation)>,
) {
    while let Some((request, operation)) = receiver.recv().await {
        let mut api = api.lock().await;
        let result = match operation {
            Operation::Play(plan) => play_verified(&mut api, &plan).await.map(|()| {
                dispatch(
                    &updates,
                    "SPOTIFY_PLAYER_PLAY",
                    json!({"id":plan.track_id,"position":plan.position_ms}),
                );
                Reply::Played
            }),
            Operation::Premium(_) => api.profile().await.map(|body| {
                // Not in the pinned client: SpotifyCrack reads every account as Premium (`plugin_hooks/spotify_crack.rs`).
                let premium = body["product"] == "premium" || crate::ui::spotify_premium_forced();
                dispatch(
                    &updates,
                    "SPOTIFY_PROFILE_UPDATE",
                    json!({"accountId":account,"isPremium":premium}),
                );
                Reply::Premium(premium)
            }),
            Operation::Pause(_) => api.pause().await.map(|()| {
                dispatch(&updates, "SPOTIFY_PLAYER_PAUSE", json!({}));
                Reply::Paused
            }),
        };
        reply(
            &updates,
            request,
            result.as_ref().copied().unwrap_or(Reply::Failed),
        );
        if result == Err(Error::Revoked) {
            dispatch(
                &updates,
                "SPOTIFY_ACCOUNT_ACCESS_TOKEN_REVOKE",
                json!({"accountId":account}),
            );
            return;
        }
    }
}

/// 158390: each failure adds up to twice the current delay; the Spotify caller caps it at a minute.
fn next_delay(current: Duration, jitter: f64) -> Duration {
    Duration::from_secs_f64((current.as_secs_f64() * (1. + 2. * jitter)).min(60.))
}

async fn session(
    api: Arc<Mutex<Spotify>>,
    updates: mpsc::UnboundedSender<Update>,
    account: String,
) {
    let mut delay = Duration::from_millis(500);
    loop {
        let result = connection(api.clone(), &updates, &account, &mut delay).await;
        socket_state(&updates, &account, false);
        if result == Err(Error::Revoked) {
            dispatch(
                &updates,
                "SPOTIFY_ACCOUNT_ACCESS_TOKEN_REVOKE",
                json!({"accountId":account}),
            );
            return;
        }
        delay = next_delay(delay, rand::random());
        let delay = match result {
            Err(Error::RateLimited(wait)) => wait.max(delay),
            _ => delay,
        };
        let Some(at) = Instant::now().checked_add(delay) else {
            return;
        };
        tokio::time::sleep_until(at).await;
    }
}

type Job = Result<(), Error>;

/// The context the account's current player state holds (`getPlayerState(e).context` of module 655116).
type LastContext = Arc<std::sync::Mutex<Option<Value>>>;

/// `ec` of module 655116: a provider player snapshot becomes `SPOTIFY_PLAYER_STATE` with its context: an album is
/// its own context, a playlist is fetched once per uri and kept only while public. A failed playlist fetch other than
/// a 404 dispatches nothing, as the official promise rejects.
async fn player_state_event(
    api: &mut Spotify,
    account: &str,
    state: &Value,
    last: &LastContext,
) -> Result<Option<Value>, Error> {
    use dc_state::spotify::{ContextSource, context_source};
    let previous = last.lock().ok().and_then(|held| held.clone());
    let context = match context_source(state, previous.as_ref()) {
        ContextSource::None => Value::Null,
        ContextSource::Ready(context) => context,
        ContextSource::Playlist(id) => match api.playlist(&id).await {
            Ok(body) => body,
            Err(Error::Status(404)) => Value::Null,
            Err(error @ (Error::Revoked | Error::RateLimited(_))) => return Err(error),
            Err(_) => return Ok(None),
        },
    };
    Ok(Some(event_with_context(account, state, context, last)))
}

/// The event of a snapshot whose context is known; the account's held context follows what the store keeps (a
/// playing track's context, nothing otherwise).
fn event_with_context(account: &str, state: &Value, context: Value, last: &LastContext) -> Value {
    let mut event = dc_state::spotify::player_event(account, state);
    event["context"] = dc_state::spotify::public_context(context);
    if let Ok(mut held) = last.lock() {
        *held = (event["isPlaying"] == true && event["track"].is_object())
            .then(|| event["context"].clone())
            .filter(|context| !context.is_null());
    }
    event
}

fn scan(
    jobs: &mut JoinSet<Job>,
    api: Arc<Mutex<Spotify>>,
    updates: mpsc::UnboundedSender<Update>,
    account: String,
    profile: bool,
    last: LastContext,
) {
    jobs.spawn(async move {
        let mut api = api.lock().await;
        if profile {
            let body = api.profile().await?;
            dispatch(
                &updates,
                "SPOTIFY_PROFILE_UPDATE",
                // Not in the pinned client: SpotifyCrack (`plugin_hooks/spotify_crack.rs`).
                json!({"accountId":account,"isPremium":body["product"]=="premium" || crate::ui::spotify_premium_forced()}),
            );
        }
        let body = api.devices().await?;
        let devices: Vec<Value> = body["devices"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|device| {
                json!({
            "id":device["id"], "name":device["name"], "type":device["type"],
            "is_active":device["is_active"], "is_restricted":device["is_restricted"],
            "volume_percent":device["volume_percent"]})
            })
            .collect();
        dispatch(
            &updates,
            "SPOTIFY_SET_DEVICES",
            json!({"accountId":account,"devices":devices}),
        );
        let state = api.player().await?;
        if let Some(event) = player_state_event(&mut api, &account, &state, &last).await? {
            dispatch(&updates, "SPOTIFY_PLAYER_STATE", event);
        }
        Ok(())
    });
}

/// `handleMessage` of module 655116: the events every payload of a dealer `message` carries.
fn dealer_events(message: &Value) -> impl Iterator<Item = &Value> {
    message["payloads"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|payload| payload["events"].as_array().into_iter().flatten())
}

fn connection_id(uri: &str) -> Option<String> {
    let suffix = uri.strip_prefix("hm://pusher/v1/connections/")?;
    percent_encoding::percent_decode_str(suffix)
        .decode_utf8()
        .ok()
        .map(|s| s.into_owned())
}

async fn connection(
    api: Arc<Mutex<Spotify>>,
    updates: &mpsc::UnboundedSender<Update>,
    account: &str,
    delay: &mut Duration,
) -> Job {
    let last: LastContext = Arc::default();
    let (initial, mut dealer): (Option<Value>, Dealer) = {
        let mut api = api.lock().await;
        let initial = match api.player().await {
            Ok(state) => player_state_event(&mut api, account, &state, &last).await?,
            Err(Error::Revoked) => return Err(Error::Revoked),
            Err(Error::RateLimited(wait)) => return Err(Error::RateLimited(wait)),
            // `eu`: a failed player request is an empty player state (`Z`).
            Err(_) => Some(dc_state::spotify::player_event(account, &Value::Null)),
        };
        (initial, api.open_dealer().await?)
    };
    *delay = Duration::from_millis(500);
    socket_state(updates, account, true);
    if let Some(initial) = initial {
        dispatch(updates, "SPOTIFY_PLAYER_STATE", initial);
    }
    let mut jobs = JoinSet::new();
    scan(
        &mut jobs,
        api.clone(),
        updates.clone(),
        account.into(),
        true,
        last.clone(),
    );
    let mut ping = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(30),
        Duration::from_secs(30),
    );
    let mut refresh = false;
    let mut refresh_after = Instant::now();
    loop {
        tokio::select! {
            _ = ping.tick() => dealer.ping().await?,
            finished = jobs.join_next(), if !jobs.is_empty() => {
                if matches!(finished, Some(Ok(Err(Error::Revoked)))) { return Err(Error::Revoked) }
                if let Some(Ok(Err(Error::RateLimited(wait)))) = finished {
                    refresh = true;
                    let Some(at) = Instant::now().checked_add(wait) else { return Err(Error::RateLimited(wait)) };
                    refresh_after = at.max(refresh_after);
                }
            }
            _ = tokio::time::sleep_until(refresh_after), if refresh && jobs.is_empty() => {
                refresh = false;
                refresh_after = Instant::now() + Duration::from_secs(3);
                scan(&mut jobs, api.clone(), updates.clone(), account.into(), false, last.clone());
            }
            message = dealer.receive() => {
                let Some(message) = message? else { return Ok(()) };
                if message["type"] != "message" { continue }
                if let Some(id) = message["uri"].as_str().and_then(connection_id) {
                    let api = api.clone();
                    jobs.spawn(async move { api.lock().await.register_notifications(&id).await });
                } else {
                    for event in dealer_events(&message) {
                        match event["type"].as_str() {
                            Some("PLAYER_STATE_CHANGED") if event["event"]["state"].is_object() => {
                                let state = &event["event"]["state"];
                                let previous = last.lock().ok().and_then(|held| held.clone());
                                match dc_state::spotify::context_source(state, previous.as_ref()) {
                                    // Only a playlist not seen yet waits for the provider; the rest goes at once.
                                    dc_state::spotify::ContextSource::Playlist(_) => {
                                        let (api, updates, account, last, state) =
                                            (api.clone(), updates.clone(), account.to_owned(), last.clone(), state.clone());
                                        jobs.spawn(async move {
                                            let mut api = api.lock().await;
                                            if let Some(event) = player_state_event(&mut api, &account, &state, &last).await? {
                                                dispatch(&updates, "SPOTIFY_PLAYER_STATE", event);
                                            }
                                            Ok(())
                                        });
                                    }
                                    dc_state::spotify::ContextSource::Ready(context) => {
                                        dispatch(updates, "SPOTIFY_PLAYER_STATE", event_with_context(account, state, context, &last));
                                    }
                                    dc_state::spotify::ContextSource::None => {
                                        dispatch(updates, "SPOTIFY_PLAYER_STATE", event_with_context(account, state, Value::Null, &last));
                                    }
                                }
                            }
                            Some("DEVICE_STATE_CHANGED") => refresh = true,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_accounts_only_contain_provider_metadata() {
        let Command::Accounts(accounts) = observed(
            "READY",
            &json!({"connected_accounts":[
            {"id":"a","type":"spotify","show_activity":true,"unrelated":"ignored"},
            {"id":"b","type":"twitch"}]}),
        )
        .unwrap() else {
            panic!("wrong command")
        };
        assert_eq!(
            accounts,
            [LinkedAccount {
                id: "a".into(),
                show_activity: true
            }]
        );
        assert!(matches!(
            observed("USER_CONNECTIONS_UPDATE", &Value::Null),
            Some(Command::Refresh)
        ));
        assert!(observed("PRESENCE_UPDATE", &Value::Null).is_none());
        assert_eq!(
            linked_accounts(&json!([
                {"id":"a","type":"spotify","show_activity":true,"unrelated":"ignored"},
                {"type":"spotify"},
                {"id":"b","type":"twitch"}
            ])),
            accounts
        );
        assert!(linked_accounts(&json!([])).is_empty());
    }

    #[test]
    fn connection_ids_and_source_backoff_preserve_the_wire_rules() {
        assert_eq!(
            connection_id("hm://pusher/v1/connections/a%2Fb+c").as_deref(),
            Some("a/b+c")
        );
        assert!(connection_id("https://example.invalid/a").is_none());
        assert_eq!(
            next_delay(Duration::from_millis(500), 0.),
            Duration::from_millis(500)
        );
        assert_eq!(
            next_delay(Duration::from_millis(500), 0.5),
            Duration::from_secs(1)
        );
        assert_eq!(
            next_delay(Duration::from_secs(30), 1.),
            Duration::from_secs(60)
        );
    }

    #[tokio::test]
    async fn test_runs_cannot_start_a_provider_worker() {
        let client = Arc::new(
            Client::new(
                dc_proto::secrets::Token::new("fixture"),
                dc_proto::identity::Identity::new(),
            )
            .unwrap(),
        );
        let (updates, _) = mpsc::unbounded_channel();
        assert!(Worker::start(client, updates).is_none());
    }

    #[tokio::test]
    async fn removing_a_session_cancels_its_in_flight_work() {
        struct Finished(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for Finished {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }
        let (ready, started) = tokio::sync::oneshot::channel();
        let (finished, done) = tokio::sync::oneshot::channel();
        let (commands, mut receiver) = mpsc::unbounded_channel::<(u64, Operation)>();
        let task = OwnedTask(tokio::spawn(async move {
            let _finished = Finished(Some(finished));
            let _ = ready.send(());
            let _ = receiver.recv().await;
        }));
        let session = Session {
            commands: commands.clone(),
            task,
        };
        started.await.unwrap();
        drop(session);
        tokio::time::timeout(Duration::from_secs(1), done)
            .await
            .unwrap()
            .unwrap();
        assert!(
            commands
                .send((1, Operation::Pause("fixture".into())))
                .is_err()
        );
    }
    /// A dealer frame as 655116 `handleMessage` reads it: `type`, `uri` and `payloads[].events[]` with
    /// `PLAYER_STATE_CHANGED` carrying the Web API player object in `event.state`.
    fn dealer_frame(playing: bool, progress: i64) -> Value {
        json!({"type": "message", "uri": "wss://event", "headers": {"Content-Type": "application/json"},
            "payloads": [{"events": [{"type": "PLAYER_STATE_CHANGED", "uri": "wss://event",
                "event": {"state": {"is_playing": playing, "repeat_state": "context", "progress_ms": progress,
                    "shuffle_state": false, "timestamp": 1,
                    "device": {"id": "dev", "name": "Desk", "type": "Computer", "is_active": true,
                        "is_restricted": false, "volume_percent": 64},
                    "context": {"type": "album", "uri": "spotify:album:4aawyAB9vmqN3uQ7FjRGTy",
                        "href": "https://api.spotify.com/v1/albums/4aawyAB9vmqN3uQ7FjRGTy",
                        "external_urls": {"spotify": "https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy"}},
                    "item": {"type": "track", "id": "11dFghVXANMlKmJXsNCbNl", "name": "Cut To The Feeling",
                        "duration_ms": 207_959, "is_local": false, "popularity": 1,
                        "album": {"id": "4aawyAB9vmqN3uQ7FjRGTy", "name": "Cut To The Feeling", "type": "album",
                            "images": [{"url": "https://i.scdn.co/image/ab67616d0000b273e3c9b5a5c8b8f5a3",
                                "width": 640, "height": 640}]},
                        "artists": [{"id": "6sFIWsNpZYqfjUpaCgueju", "name": "Carly Rae Jepsen",
                            "type": "artist"}]}}}}]}]})
    }

    /// The dealer frame goes through the worker's context step and the store, and the account's op 3 carries the
    /// LISTENING activity after its custom status; paused, the op 3 has the custom status alone again.
    #[test]
    fn a_dealer_player_state_becomes_the_listening_activity_of_op_3() {
        let mut store = dc_state::spotify::SpotifyStore::default();
        store.dispatch(
            "READY",
            &json!({"connected_accounts": [{"id": "spot", "type": "spotify", "show_activity": true}]}),
            0,
        );
        store.dispatch(
            "SPOTIFY_ACCOUNT_ACCESS_TOKEN",
            &json!({"accountId": "spot"}),
            0,
        );
        let last = LastContext::default();
        let custom = json!({"name": "Custom Status", "type": 4, "state": "hi", "emoji": null, "metadata": {}});
        let op3 = |store: &dc_state::spotify::SpotifyStore, idle: bool, status: &str| {
            let spotify = store
                .should_show_activity(idle)
                .then(|| store.activity(42))
                .flatten();
            // `shared_local_activities` already shares nothing for an invisible account.
            let local = if status == "invisible" {
                Vec::new()
            } else {
                vec![custom.clone()]
            };
            let activities = crate::games::with_spotify(local, spotify, status);
            crate::games::presence_body_idle(status, &activities, 0, false)
        };
        let before = op3(&store, false, "online");
        let deliver = |store: &mut dc_state::spotify::SpotifyStore, frame: &Value, now: i64| {
            for event in dealer_events(frame) {
                assert_eq!(event["type"], "PLAYER_STATE_CHANGED");
                let state = &event["event"]["state"];
                let context =
                    match dc_state::spotify::context_source(state, last.lock().unwrap().as_ref()) {
                        dc_state::spotify::ContextSource::Ready(context) => context,
                        other => panic!("an album context needs no fetch: {other:?}"),
                    };
                store.dispatch(
                    "SPOTIFY_PLAYER_STATE",
                    &event_with_context("spot", state, context, &last),
                    now,
                );
            }
        };
        deliver(&mut store, &dealer_frame(true, 7_959), 1_700_000_010_000);
        let after = op3(&store, false, "online");
        eprintln!("op 3 before: {before}\nop 3 after: {after}");
        assert_eq!(
            before,
            json!({"status": "online", "since": 0, "activities": [custom], "afk": false})
        );
        assert_eq!(
            after,
            json!({"status": "online", "since": 0, "afk": false, "activities": [custom, {
                "type": 2, "name": "Spotify",
                "assets": {"large_image": "spotify:ab67616d0000b273e3c9b5a5c8b8f5a3",
                    "large_text": "Cut To The Feeling"},
                "details": "Cut To The Feeling", "state": "Carly Rae Jepsen",
                "timestamps": {"start": 1_700_000_002_041_i64, "end": 1_700_000_210_000_i64},
                "party": {"id": "spotify:42"}, "sync_id": "11dFghVXANMlKmJXsNCbNl", "flags": 48,
                "metadata": {"context_uri": "spotify:album:4aawyAB9vmqN3uQ7FjRGTy",
                    "album_id": "4aawyAB9vmqN3uQ7FjRGTy", "artist_ids": ["6sFIWsNpZYqfjUpaCgueju"],
                    "type": "track", "button_urls": []}}]})
        );
        // The held context is reused for the next frame of the same album.
        assert!(last.lock().unwrap().is_some());
        // Idle or invisible: the custom status alone, or nothing.
        assert_eq!(op3(&store, true, "online"), before);
        assert_eq!(op3(&store, false, "invisible")["activities"], json!([]));
        // Paused: the activity goes with the next update.
        deliver(&mut store, &dealer_frame(false, 9_000), 1_700_000_012_000);
        assert_eq!(op3(&store, false, "online"), before);
        assert!(last.lock().unwrap().is_none());
    }
}
