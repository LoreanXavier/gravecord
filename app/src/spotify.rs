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
    let mut sessions: HashMap<String, OwnedTask> = HashMap::new();
    let mut retry_at = None;
    loop {
        let command = tokio::select! {
            command = commands.recv() => match command { Some(command) => command, None => break },
            _ = async { if let Some(at) = retry_at { tokio::time::sleep_until(at).await; } else { std::future::pending::<()>().await; } } => Command::Refresh,
        };
        retry_at = None;
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
            accounts.iter().any(|account| account.id == *id) && !task.0.is_finished()
        });
        for account in accounts {
            if let std::collections::hash_map::Entry::Vacant(entry) =
                sessions.entry(account.id.clone())
            {
                let task =
                    tokio::spawn(session(client.clone(), updates.clone(), account.id.clone()));
                entry.insert(OwnedTask(task));
            }
        }
    }
}

/// 158390: each failure adds up to twice the current delay; the Spotify caller caps it at a minute.
fn next_delay(current: Duration, jitter: f64) -> Duration {
    Duration::from_secs_f64((current.as_secs_f64() * (1. + 2. * jitter)).min(60.))
}

async fn session(client: Arc<Client>, updates: mpsc::UnboundedSender<Update>, account: String) {
    let Ok(api) = Spotify::new(client, account.clone()) else {
        return;
    };
    let notices = updates.clone();
    let linked_account = account.clone();
    let api = api.on_credential(move || {
        dispatch(
            &notices,
            "SPOTIFY_ACCOUNT_ACCESS_TOKEN",
            json!({"accountId":linked_account}),
        );
    });
    let api = Arc::new(Mutex::new(api));
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

fn scan(
    jobs: &mut JoinSet<Job>,
    api: Arc<Mutex<Spotify>>,
    updates: mpsc::UnboundedSender<Update>,
    account: String,
    profile: bool,
) {
    jobs.spawn(async move {
        let mut api = api.lock().await;
        if profile {
            let body = api.profile().await?;
            dispatch(
                &updates,
                "SPOTIFY_PROFILE_UPDATE",
                json!({"accountId":account,"isPremium":body["product"]=="premium"}),
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
        dispatch(
            &updates,
            "SPOTIFY_PLAYER_STATE",
            dc_state::spotify::player_event(&account, &state),
        );
        Ok(())
    });
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
    let (initial, mut dealer): (Option<Value>, Dealer) = {
        let mut api = api.lock().await;
        let initial = match api.player().await {
            Ok(state) => Some(state),
            Err(Error::Revoked) => return Err(Error::Revoked),
            Err(Error::RateLimited(wait)) => return Err(Error::RateLimited(wait)),
            Err(_) => None,
        };
        (initial, api.open_dealer().await?)
    };
    *delay = Duration::from_millis(500);
    socket_state(updates, account, true);
    dispatch(
        updates,
        "SPOTIFY_PLAYER_STATE",
        dc_state::spotify::player_event(account, initial.as_ref().unwrap_or(&Value::Null)),
    );
    let mut jobs = JoinSet::new();
    scan(
        &mut jobs,
        api.clone(),
        updates.clone(),
        account.into(),
        true,
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
                scan(&mut jobs, api.clone(), updates.clone(), account.into(), false);
            }
            message = dealer.receive() => {
                let Some(message) = message? else { return Ok(()) };
                if message["type"] != "message" { continue }
                if let Some(id) = message["uri"].as_str().and_then(connection_id) {
                    let api = api.clone();
                    jobs.spawn(async move { api.lock().await.register_notifications(&id).await });
                } else {
                    for event in message["payloads"].as_array().into_iter().flatten()
                        .flat_map(|payload| payload["events"].as_array().into_iter().flatten()) {
                        match event["type"].as_str() {
                            Some("PLAYER_STATE_CHANGED") if event["event"]["state"].is_object() => {
                                dispatch(updates, "SPOTIFY_PLAYER_STATE", dc_state::spotify::player_event(account, &event["event"]["state"]));
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
}
