//! The server end to end: the connection logic over an in-memory stream, and the Windows pipe with a test-only name
//! (never `discord-ipc-*`: a game may be using those).

use dc_rpc::{BoxFuture, Config, Decoder, Directory, Event, Opcode, Shared, encode, serve};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

struct Games;

impl Directory for Games {
    fn application(&self, id: &str) -> BoxFuture<'_, Option<Value>> {
        let found = (id == "100").then(|| json!({"id": "100", "name": "P.T. Silent Hills"}));
        Box::pin(async move { found })
    }
    fn assets(&self, _: &str) -> BoxFuture<'_, Option<Vec<Value>>> {
        Box::pin(async { Some(vec![json!({"id": "555", "name": "pt", "type": 1})]) })
    }
    fn external_assets(&self, _: &str, _: Vec<String>) -> BoxFuture<'_, Option<Vec<Value>>> {
        Box::pin(async { None })
    }
    fn current_user(&self) -> Option<Value> {
        Some(
            json!({"id": "9", "username": "me", "discriminator": "0", "global_name": null,
            "avatar": null, "avatar_decoration_data": null, "bot": false, "flags": 0,
            "public_flags": 0, "premium_type": 0}),
        )
    }
}

struct Client<S> {
    stream: S,
    decoder: Decoder,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Client<S> {
    fn new(stream: S) -> Self {
        Self {
            stream,
            decoder: Decoder::default(),
        }
    }

    async fn send(&mut self, opcode: Opcode, payload: Value) {
        self.stream
            .write_all(&encode(opcode, &payload))
            .await
            .unwrap();
    }

    async fn recv(&mut self) -> Option<(Opcode, Value)> {
        let mut buffer = [0u8; 4096];
        loop {
            if let Some(frame) = self.decoder.next_frame().unwrap() {
                return Some(frame);
            }
            let read = tokio::time::timeout(Duration::from_secs(5), self.stream.read(&mut buffer))
                .await
                .expect("the server answers within five seconds")
                .unwrap_or(0);
            if read == 0 {
                return None;
            }
            self.decoder.push(&buffer[..read]).unwrap();
        }
    }

    async fn handshake(&mut self) -> Value {
        self.send(Opcode::Handshake, json!({"v": 1, "client_id": "100"}))
            .await;
        let (opcode, ready) = self.recv().await.unwrap();
        assert_eq!(opcode, Opcode::Frame);
        ready
    }

    async fn command(&mut self, cmd: &str, args: Value) -> Value {
        self.send(
            Opcode::Frame,
            json!({"cmd": cmd, "args": args, "nonce": "n1"}),
        )
        .await;
        self.recv().await.unwrap().1
    }

    async fn closed_with(&mut self) -> (i64, String) {
        let (opcode, body) = self.recv().await.expect("a close frame");
        assert_eq!(opcode, Opcode::Close);
        assert!(
            self.recv().await.is_none(),
            "the stream ends after the close frame"
        );
        (
            body["code"].as_i64().unwrap(),
            body["message"].as_str().unwrap().to_owned(),
        )
    }
}

struct Rig {
    client: Client<tokio::io::DuplexStream>,
    events: mpsc::UnboundedReceiver<Event>,
    _stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

fn rig_with(config: Config) -> Rig {
    let (events_tx, events) = mpsc::unbounded_channel();
    let shared =
        Arc::new(Shared::new(Arc::new(Games), events_tx, config).with_clock(|| 1_794_312_400_000));
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (stop_tx, stop) = watch::channel(false);
    let task = tokio::spawn(async move { serve(theirs, &shared, stop).await });
    Rig {
        client: Client::new(ours),
        events,
        _stop: stop_tx,
        task,
    }
}

fn new_rig() -> Rig {
    rig_with(Config::default())
}

#[tokio::test]
async fn the_handshake_answers_with_ready_in_the_official_shape() {
    let mut rig = new_rig();
    let ready = rig.client.handshake().await;
    assert_eq!(ready["cmd"], "DISPATCH");
    assert_eq!(ready["evt"], "READY");
    assert_eq!(ready["nonce"], Value::Null);
    assert_eq!(ready["data"]["v"], 1);
    assert_eq!(ready["data"]["config"]["cdn_host"], "cdn.discordapp.com");
    assert_eq!(ready["data"]["config"]["api_endpoint"], "//discord.com/api");
    assert_eq!(ready["data"]["config"]["environment"], "production");
    assert_eq!(ready["data"]["user"]["username"], "me");
    assert!(matches!(
        rig.events.recv().await,
        Some(Event::Connected { socket: 1, .. })
    ));
}

#[tokio::test]
async fn handshake_checks_close_with_the_official_codes() {
    let cases = [
        (
            json!({"v": 2, "client_id": "100"}),
            1003,
            "Invalid Version: 2",
        ),
        (
            json!({"v": 0, "client_id": "100"}),
            1003,
            "Invalid Version: 0",
        ),
        (
            json!({"v": null, "client_id": "100"}),
            1003,
            "Invalid Version: 0",
        ),
        (json!({"v": 1}), 4000, "No Client ID Specified"),
        (
            json!({"v": 1, "client_id": ""}),
            4000,
            "No Client ID Specified",
        ),
        (
            json!({"v": 1, "client_id": "404"}),
            4000,
            "Invalid Client ID",
        ),
    ];
    for (payload, code, message) in cases {
        let mut rig = new_rig();
        rig.client.send(Opcode::Handshake, payload.clone()).await;
        assert_eq!(
            rig.client.closed_with().await,
            (code, message.to_owned()),
            "{payload}"
        );
    }
}

#[tokio::test]
async fn a_version_that_is_not_a_number_passes_like_the_official_comparison() {
    let mut rig = new_rig();
    rig.client
        .send(Opcode::Handshake, json!({"client_id": "100"}))
        .await;
    assert_eq!(rig.client.recv().await.unwrap().1["evt"], "READY");
}

#[tokio::test]
async fn frames_in_the_wrong_order_and_bad_frames_close_as_unsupported() {
    let mut rig = new_rig();
    rig.client
        .send(Opcode::Frame, json!({"cmd": "SET_ACTIVITY", "nonce": "1"}))
        .await;
    assert_eq!(
        rig.client.closed_with().await,
        (1003, "did not handshake".into())
    );

    let mut rig = new_rig();
    rig.client.handshake().await;
    rig.client
        .send(Opcode::Handshake, json!({"v": 1, "client_id": "100"}))
        .await;
    assert_eq!(
        rig.client.closed_with().await,
        (1003, "already did handshake".into())
    );

    let mut rig = new_rig();
    let mut bad = 9i32.to_le_bytes().to_vec();
    bad.extend(0i32.to_le_bytes());
    rig.client.stream.write_all(&bad).await.unwrap();
    assert_eq!(
        rig.client.closed_with().await,
        (1003, "protocol error".into())
    );
}

#[tokio::test]
async fn ping_is_answered_with_pong_and_close_with_close() {
    let mut rig = new_rig();
    rig.client.send(Opcode::Ping, json!("abc")).await;
    assert_eq!(
        rig.client.recv().await.unwrap(),
        (Opcode::Pong, json!("abc"))
    );
    rig.client.send(Opcode::Close, json!({})).await;
    assert_eq!(
        rig.client.closed_with().await,
        (1000, "client disconnect".into())
    );
}

#[tokio::test]
async fn a_silent_client_is_closed_after_the_handshake_timeout() {
    let mut rig = rig_with(Config {
        handshake_timeout: Duration::from_millis(60),
        ..Config::default()
    });
    assert_eq!(
        rig.client.closed_with().await,
        (1006, "Handshake timeout".into())
    );
}

#[tokio::test]
async fn set_update_and_clear_an_activity() {
    let mut rig = new_rig();
    rig.client.handshake().await;
    assert!(matches!(
        rig.events.recv().await,
        Some(Event::Connected { .. })
    ));

    let reply = rig
        .client
        .command(
            "SET_ACTIVITY",
            json!({"pid": 3456, "activity": {
                "details": "In the menus", "state": "New Game+ 4/4",
                "timestamps": {"start": 1794312327},
                "assets": {"large_image": "pt", "large_text": "P.T. PC Port"}}}),
        )
        .await;
    let expected = json!({
        "details": "In the menus", "state": "New Game+ 4/4",
        "timestamps": {"start": 1794312327000u64},
        "assets": {"large_image": "555", "large_text": "P.T. PC Port"},
        "name": "P.T. Silent Hills", "type": 0, "application_id": "100",
        "platform": "desktop", "metadata": {}
    });
    assert_eq!(reply["cmd"], "SET_ACTIVITY");
    assert_eq!(reply["evt"], Value::Null);
    assert_eq!(reply["nonce"], "n1");
    assert_eq!(reply["data"], expected);
    assert_eq!(
        rig.events.recv().await,
        Some(Event::Activity {
            socket: 1,
            pid: Some(3456),
            application_id: Some("100".into()),
            activity: Some(expected),
            party_privacy: None,
        })
    );

    let update = rig
        .client
        .command(
            "SET_ACTIVITY",
            json!({"pid": 3456, "activity": {"state": "Chapter 2"}}),
        )
        .await;
    assert_eq!(update["data"]["state"], "Chapter 2");
    assert!(matches!(
        rig.events.recv().await,
        Some(Event::Activity {
            activity: Some(_),
            ..
        })
    ));

    let cleared = rig
        .client
        .command("SET_ACTIVITY", json!({"pid": 3456, "activity": null}))
        .await;
    assert_eq!(cleared["data"], Value::Null);
    assert_eq!(cleared["evt"], Value::Null);
    assert_eq!(
        rig.events.recv().await,
        Some(Event::Activity {
            socket: 1,
            pid: Some(3456),
            application_id: Some("100".into()),
            activity: None,
            party_privacy: None,
        })
    );
}

#[tokio::test]
async fn closing_the_pipe_reports_the_socket_gone() {
    let mut rig = new_rig();
    rig.client.handshake().await;
    rig.client
        .command(
            "SET_ACTIVITY",
            json!({"pid": 1, "activity": {"state": "ab"}}),
        )
        .await;
    drop(rig.client);
    rig.task.await.unwrap();
    let mut last = None;
    while let Some(event) = rig.events.recv().await {
        last = Some(event);
    }
    assert_eq!(last, Some(Event::Disconnected { socket: 1 }));
}

#[tokio::test]
async fn requests_are_refused_with_the_official_errors() {
    let mut rig = new_rig();
    rig.client.handshake().await;

    rig.client
        .send(Opcode::Frame, json!({"cmd": "SET_ACTIVITY", "args": {}}))
        .await;
    let no_nonce = rig.client.recv().await.unwrap().1;
    assert_eq!(no_nonce["evt"], "ERROR");
    assert_eq!(no_nonce["cmd"], "SET_ACTIVITY");
    assert_eq!(
        no_nonce["data"],
        json!({"code": 4000, "message": "Payload requires a nonce"})
    );

    let unknown = rig.client.command("NOT_A_COMMAND", json!({})).await;
    assert_eq!(
        unknown["data"],
        json!({"code": 4002, "message": "Invalid command: NOT_A_COMMAND"})
    );
    assert_eq!(unknown["nonce"], "n1");

    for cmd in ["GET_GUILDS", "AUTHORIZE", "SUBSCRIBE", "SET_VOICE_SETTINGS"] {
        let refused = rig.client.command(cmd, json!({})).await;
        assert_eq!(refused["evt"], "ERROR", "{cmd}");
        assert_eq!(
            refused["data"],
            json!({"code": 4006, "message": "Not authenticated or invalid scope"}),
            "{cmd}"
        );
    }

    let invalid = rig
        .client
        .command(
            "SET_ACTIVITY",
            json!({"pid": 1, "activity": {"state": "x"}}),
        )
        .await;
    assert_eq!(
        invalid["data"],
        json!({"code": 4000, "message": "\"activity.state\" length must be at least 2 characters long"})
    );

    let no_pid = rig
        .client
        .command("SET_ACTIVITY", json!({"activity": {"state": "ab"}}))
        .await;
    assert_eq!(
        no_pid["data"],
        json!({"code": 4002, "message": "nonzero pid required"})
    );

    // Every refusal left the connection open.
    let ok = rig
        .client
        .command(
            "SET_ACTIVITY",
            json!({"pid": 1, "activity": {"state": "ab"}}),
        )
        .await;
    assert_eq!(ok["evt"], Value::Null);
}

#[tokio::test]
async fn several_clients_each_keep_their_own_socket_and_activity() {
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared::new(Arc::new(Games), events_tx, Config::default()));
    let (_stop_tx, stop) = watch::channel(false);
    let mut clients = Vec::new();
    for _ in 0..2 {
        let (ours, theirs) = tokio::io::duplex(64 * 1024);
        let (shared, stop) = (shared.clone(), stop.clone());
        tokio::spawn(async move { serve(theirs, &shared, stop).await });
        let mut client = Client::new(ours);
        client.handshake().await;
        clients.push(client);
    }
    for (index, client) in clients.iter_mut().enumerate() {
        client
            .command(
                "SET_ACTIVITY",
                json!({"pid": 10 + index, "activity": {"state": format!("s{index}")}}),
            )
            .await;
    }
    let mut seen = Vec::new();
    while seen.len() < 2 {
        if let Some(Event::Activity { socket, pid, .. }) = events.recv().await {
            seen.push((socket, pid.unwrap()));
        }
    }
    seen.sort_unstable();
    assert_eq!(seen, vec![(1, 10), (2, 11)]);
}

#[tokio::test]
async fn handshakes_of_one_application_wait_for_a_token() {
    let config = Config {
        throttle_capacity: 1,
        throttle_window: Duration::from_millis(300),
        ..Config::default()
    };
    let (events_tx, _events) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared::new(Arc::new(Games), events_tx, config));
    let (_stop_tx, stop) = watch::channel(false);
    let mut readies = Vec::new();
    let started = std::time::Instant::now();
    for _ in 0..2 {
        let (ours, theirs) = tokio::io::duplex(64 * 1024);
        let (shared, stop) = (shared.clone(), stop.clone());
        tokio::spawn(async move { serve(theirs, &shared, stop).await });
        let mut client = Client::new(ours);
        client.handshake().await;
        readies.push(started.elapsed());
        std::mem::forget(client);
    }
    assert!(readies[0] < Duration::from_millis(150));
    assert!(readies[1] >= Duration::from_millis(250), "{readies:?}");
}

#[cfg(windows)]
mod pipe {
    use super::*;
    use dc_rpc::Server;
    use tokio::net::windows::named_pipe::ClientOptions;

    fn names(tag: &str) -> Vec<String> {
        let base = format!(r"\\.\pipe\gravecord-rpc-test-{}-{tag}", std::process::id());
        vec![format!("{base}-0"), format!("{base}-1")]
    }

    fn start(names: &[String]) -> (Server, mpsc::UnboundedReceiver<Event>) {
        let (events_tx, events) = mpsc::unbounded_channel();
        let config = Config {
            pipe_names: names.to_vec(),
            ..Config::default()
        };
        (
            Server::start(Arc::new(Games), events_tx, config).unwrap(),
            events,
        )
    }

    #[tokio::test]
    async fn a_game_sets_updates_and_clears_its_activity_over_a_real_pipe() {
        let names = names("single");
        let (server, mut events) = start(&names);
        assert_eq!(server.pipe_name(), names[0]);
        let mut game = Client::new(ClientOptions::new().open(&names[0]).unwrap());
        assert_eq!(game.handshake().await["evt"], "READY");
        let set = game
            .command("SET_ACTIVITY", json!({"pid": 77, "activity": {"state": "Chapter 1", "assets": {"large_image": "pt"}}}))
            .await;
        assert_eq!(set["data"]["assets"]["large_image"], "555");
        assert_eq!(set["data"]["name"], "P.T. Silent Hills");
        let update = game
            .command(
                "SET_ACTIVITY",
                json!({"pid": 77, "activity": {"state": "Chapter 2"}}),
            )
            .await;
        assert_eq!(update["data"]["state"], "Chapter 2");
        game.command("SET_ACTIVITY", json!({"pid": 77, "activity": null}))
            .await;
        drop(game);
        let mut kinds = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
            {
                Some(Event::Connected { .. }) => kinds.push("connected"),
                Some(Event::Activity {
                    activity: Some(_), ..
                }) => kinds.push("set"),
                Some(Event::Activity { activity: None, .. }) => kinds.push("clear"),
                Some(Event::Disconnected { .. }) => {
                    kinds.push("gone");
                    break;
                }
                None => break,
            }
        }
        assert_eq!(kinds, ["connected", "set", "set", "clear", "gone"]);
        server.shutdown().await;
    }

    #[tokio::test]
    async fn a_taken_pipe_is_skipped_and_two_clients_are_served_at_once() {
        let names = names("pair");
        let (first, _first_events) = start(&names);
        let (second, mut events) = start(&names);
        assert_eq!(first.pipe_name(), names[0]);
        assert_eq!(second.pipe_name(), names[1]);
        let mut a = Client::new(ClientOptions::new().open(&names[1]).unwrap());
        a.handshake().await;
        let mut b = Client::new(ClientOptions::new().open(&names[1]).unwrap());
        b.handshake().await;
        a.command(
            "SET_ACTIVITY",
            json!({"pid": 1, "activity": {"state": "aa"}}),
        )
        .await;
        b.command(
            "SET_ACTIVITY",
            json!({"pid": 2, "activity": {"state": "bb"}}),
        )
        .await;
        let mut sockets = std::collections::BTreeSet::new();
        while sockets.len() < 2 {
            if let Some(Event::Activity { socket, .. }) = events.recv().await {
                sockets.insert(socket);
            }
        }
        assert_eq!(sockets.len(), 2);
        // A third server finds no free name.
        let (events_tx, _events) = mpsc::unbounded_channel();
        let config = Config {
            pipe_names: names.clone(),
            ..Config::default()
        };
        assert!(Server::start(Arc::new(Games), events_tx, config).is_err());
        second.shutdown().await;
        // Shutdown said goodbye to its clients.
        assert_eq!(a.closed_with().await, (1000, "User logged out".into()));
        assert_eq!(b.closed_with().await, (1000, "User logged out".into()));
        first.shutdown().await;
        // The names are free again.
        let (again, _events) = start(&names);
        again.shutdown().await;
    }
}
