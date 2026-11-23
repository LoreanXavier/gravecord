//! The voice connection recovering by itself, through the real client code (`dc-voice`) against the simulated voice
//! server: a dropped control socket resumes (op 7, op 9), a socket that went silent is found by the expedited
//! heartbeat of a network event, a Stage channel connects at DAVE protocol version 0 (and a voice channel refuses
//! it), and a session the server dropped connects again on the same endpoint and token, with no new voice server.
use dc_callsim::server::{Ticket, VoiceServer};
use dc_voice::reconnect::{Next, Reconnect};
use dc_voice::{Controls, DisconnectReason, Event, Join, RtcState, SystemEvent};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ROOM: (u64, u64) = (7000, 7001);

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
}

/// One participant: its events, newest last, and how many connections it opened.
#[derive(Clone, Default)]
struct Log {
    events: Arc<Mutex<Vec<Event>>>,
    connects: Arc<Mutex<u32>>,
}

impl Log {
    fn count(&self, wanted: &Event) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| *event == wanted)
            .count()
    }
    fn any(&self, wanted: impl Fn(&Event) -> bool) -> bool {
        self.events.lock().unwrap().iter().any(wanted)
    }
    fn connects(&self) -> u32 {
        *self.connects.lock().unwrap()
    }
}

async fn wait(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < limit, "{what} within {limit:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `label` joins the room on `server` and, as `RTCConnection` does, connects again after every end it did not ask
/// for, on the same endpoint and token.
fn join(server: &Arc<VoiceServer>, label: &str, user: u64, stage: bool) -> (Controls, Log) {
    let session = format!("session-{label}");
    let token = server.issue(Ticket {
        user,
        session: session.clone(),
        server_id: ROOM.0,
        channel_id: ROOM.1,
        label: label.to_owned(),
    });
    let endpoint = server.endpoint();
    let controls = Controls::new();
    let log = Log::default();
    let (running, record) = (controls.clone(), log.clone());
    tokio::spawn(async move {
        let mut reconnect = Reconnect::new();
        let rtc_connection_id = dc_voice::rtc_connection_id();
        loop {
            *record.connects.lock().unwrap() += 1;
            let join = Join {
                endpoint: endpoint.clone(),
                token: token.clone(),
                session_id: session.clone(),
                server_id: ROOM.0,
                channel_id: ROOM.1,
                user_id: user,
                stage,
                rtc_connection_id: rtc_connection_id.clone(),
                report_no_route: reconnect.report_no_route(),
            };
            let (task, mut events) = dc_voice::connect(join, &running);
            let mut ended = None;
            while let Some(event) = events.recv().await {
                match &event {
                    Event::ProtocolSelected => reconnect.routed(),
                    Event::Connected => reconnect.secured(),
                    Event::Disconnected { reason } => ended = Some(reason.clone()),
                    _ => {}
                }
                record.events.lock().unwrap().push(event);
            }
            let _ = task.await;
            let Some(reason) = ended else { return };
            let next = reconnect.after(&reason, &mut rand::rng());
            match next {
                Next::Retry(delay) => tokio::time::sleep(delay).await,
                Next::Stop => return,
            }
        }
    });
    (controls, log)
}

fn start() -> (tokio::runtime::Runtime, Arc<VoiceServer>) {
    dc_voice::sim::activate();
    let runtime = runtime();
    let server = runtime.block_on(VoiceServer::start()).unwrap();
    (runtime, server)
}

#[test]
fn a_dropped_control_socket_resumes_the_same_session() {
    let (runtime, server) = start();
    runtime.block_on(async {
        let (a, a_log) = join(&server, "a", 3001, false);
        let (b, b_log) = join(&server, "b", 3002, false);
        wait("both connect", Duration::from_secs(20), || {
            a_log.count(&Event::Connected) == 1 && b_log.count(&Event::Connected) == 1
        })
        .await;
        // A resumable close: the client keeps UDP and DAVE and sends op 7.
        assert_eq!(server.close("a", false, 4000), 1);
        wait("a resumes", Duration::from_secs(15), || {
            server.resumes() == 1
        })
        .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(a_log.connects(), 1, "no new connection: {:?}", server.log());
        assert!(!a_log.any(|event| matches!(event, Event::Disconnected { .. })));
        assert!(!b_log.any(|event| matches!(event, Event::ClientDisconnected { .. })));
        a.disconnect();
        b.disconnect();
    });
}

#[test]
fn a_network_event_finds_a_silent_socket_with_an_expedited_heartbeat() {
    let (runtime, server) = start();
    // Without the expedited heartbeat, a silent socket is found only one minute later.
    server.set_heartbeat_interval(60_000.0);
    runtime.block_on(async {
        let (a, a_log) = join(&server, "a", 3101, false);
        wait("a connects", Duration::from_secs(20), || {
            a_log.count(&Event::Connected) == 1
        })
        .await;
        server.set_acks_muted(true);
        let asked = Instant::now();
        a.system_event(SystemEvent::NetworkOnline);
        wait("the socket drops", Duration::from_secs(8), || {
            server
                .log()
                .iter()
                .any(|line| line.contains("socket dropped"))
        })
        .await;
        let found = asked.elapsed();
        assert!(
            found >= Duration::from_millis(4900) && found < Duration::from_secs(8),
            "the 5 s expedited deadline: {found:?}"
        );
        server.set_acks_muted(false);
        wait("a resumes", Duration::from_secs(15), || {
            server.resumes() == 1
        })
        .await;
        assert_eq!(a_log.connects(), 1);
        a.disconnect();
    });
}

#[test]
fn going_offline_gives_the_socket_fifteen_seconds() {
    let (runtime, server) = start();
    server.set_heartbeat_interval(60_000.0);
    runtime.block_on(async {
        let (a, a_log) = join(&server, "a", 3201, false);
        wait("a connects", Duration::from_secs(20), || {
            a_log.count(&Event::Connected) == 1
        })
        .await;
        server.set_acks_muted(true);
        let asked = Instant::now();
        a.system_event(SystemEvent::NetworkOffline);
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert!(
            !server
                .log()
                .iter()
                .any(|line| line.contains("socket dropped")),
            "still within the 15 s offline deadline"
        );
        wait("the socket drops", Duration::from_secs(10), || {
            server
                .log()
                .iter()
                .any(|line| line.contains("socket dropped"))
        })
        .await;
        assert!(asked.elapsed() >= Duration::from_millis(14_900));
        server.set_acks_muted(false);
        wait("a resumes", Duration::from_secs(15), || {
            server.resumes() == 1
        })
        .await;
        a.disconnect();
    });
}

#[test]
fn a_stage_channel_connects_at_dave_version_zero_and_a_voice_channel_refuses_it() {
    let (runtime, server) = start();
    server.set_dave_version(0);
    // a speaks a tone; b's speaker measures it.
    dc_voice::sim::set_plan(
        "session-a",
        dc_voice::sim::Plan {
            tone_hz: Some(440.0),
            level: 0.5,
            ..Default::default()
        },
    );
    dc_voice::sim::set_plan(
        "session-b",
        dc_voice::sim::Plan {
            listen_hz: vec![440.0],
            ..Default::default()
        },
    );
    runtime.block_on(async {
        let (a, a_log) = join(&server, "a", 3301, true);
        let (b, b_log) = join(&server, "b", 3302, true);
        wait("both connect unencrypted", Duration::from_secs(20), || {
            a_log.count(&Event::Connected) == 1 && b_log.count(&Event::Connected) == 1
        })
        .await;
        assert!(a_log.any(|event| *event == Event::State(RtcState::RtcConnected)));
        assert!(
            !a_log.any(|event| matches!(event, Event::PrivacyCode(_))),
            "no DAVE group, no privacy code"
        );
        // Media flows between them without end-to-end encryption.
        wait("b hears a", Duration::from_secs(10), || {
            dc_voice::sim::report("session-b").audible > 25
        })
        .await;
        // A voice channel (not Stage) refuses version 0: 4804, then the MLS failure backoff.
        let (c, c_log) = join(&server, "c", 3303, false);
        wait("c refuses the downgrade", Duration::from_secs(20), || {
            c_log.any(|event| {
                *event
                    == Event::Disconnected {
                        reason: DisconnectReason::Closed(4804),
                    }
            })
        })
        .await;
        a.disconnect();
        b.disconnect();
        c.disconnect();
    });
}

#[test]
fn a_dropped_session_connects_again_on_the_same_server_without_a_voice_server_update() {
    let (runtime, server) = start();
    runtime.block_on(async {
        let (a, a_log) = join(&server, "a", 3401, false);
        wait("a connects", Duration::from_secs(20), || {
            a_log.count(&Event::Connected) == 1
        })
        .await;
        // 4006 cannot be resumed; no VOICE_SERVER_UPDATE follows in this world.
        assert_eq!(server.close("a", false, 4006), 1);
        wait("a connects again", Duration::from_secs(20), || {
            a_log.count(&Event::Connected) == 2
        })
        .await;
        assert_eq!(a_log.connects(), 2);
        assert!(a_log.any(|event| {
            *event
                == Event::Disconnected {
                    reason: DisconnectReason::Closed(4006),
                }
        }));
        assert_eq!(
            server
                .log()
                .iter()
                .filter(|line| line.contains("ticket for a"))
                .count(),
            1,
            "the same token: {:?}",
            server.log()
        );
        a.disconnect();
    });
}
