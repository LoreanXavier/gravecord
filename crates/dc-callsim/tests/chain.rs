//! The whole chain without a window: gateway answers, two `dc-voice` clients in a call on the simulated voice server
//! (signalling, UDP, transport encryption, DAVE), a Go Live stream sent by one and watched by the other, decoded
//! pictures counted where the viewer receives them. Scenario (a) of the simulator's report, then (b) with the server's
//! feedback switched. Small pictures, generous bounds: it must hold on a loaded machine.
use dc_callsim::{Feedback, Setup, Sim};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    eprintln!("waiting: {what}");
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < limit, "{what} within {limit:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_stream_flows_end_to_end_and_survives_feedback_changes() {
    let dispatches: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let seen = dispatches.clone();
    let sim = Sim::start(
        Setup {
            me: 1000,
            my_session: "test-session".into(),
            guild: 0,
            channel: 3000,
            people: vec![2001, 2002],
        },
        Arc::new(move |name, data| seen.lock().unwrap().push((name, data))),
    )
    .unwrap();
    sim.peer_add("a", false);
    sim.peer_add("b", false);
    wait("both participants connect", Duration::from_secs(15), || {
        sim.peer_connected("a") && sim.peer_connected("b")
    });

    // (a) b goes live, a watches.
    sim.peer_stream_start("b", 320, 180, 30, dc_media::sim::Content::Motion);
    std::thread::sleep(Duration::from_millis(300));
    sim.peer_watch("a", "b");
    wait(
        "a's first picture of b's stream",
        Duration::from_secs(10),
        || {
            sim.peer_summary("a", "b", "stream", Duration::from_secs(60))
                .is_some_and(|summary| summary.pictures > 0)
        },
    );
    std::thread::sleep(Duration::from_secs(2));
    let watched = sim
        .peer_summary("a", "b", "stream", Duration::from_secs(2))
        .unwrap();
    assert!(
        watched.fps > 8.0,
        "{}\n{}",
        watched.line(),
        sim.stats(Duration::from_secs(5))
    );
    assert!(watched.longest_gap_ms < 1500, "{}", watched.line());
    assert_eq!(watched.size, (320, 180));
    // The app heard of the stream through b's voice state.
    assert!(dispatches.lock().unwrap().iter().any(|(name, data)| {
        name == "VOICE_STATE_UPDATE" && data["user_id"] == "2002" && data["self_stream"] == true
    }));

    // (b) a goes live, b watches; then the server stops all feedback and the stream keeps flowing.
    sim.peer_stream_start("a", 320, 180, 30, dc_media::sim::Content::Static);
    std::thread::sleep(Duration::from_millis(300));
    sim.peer_watch("b", "a");
    wait(
        "b's first picture of a's stream",
        Duration::from_secs(10),
        || {
            sim.peer_summary("b", "a", "stream", Duration::from_secs(60))
                .is_some_and(|summary| summary.pictures > 0)
        },
    );
    sim.set_feedback(Feedback {
        transport_cc: false,
        receiver_reports: false,
        remb: false,
    });
    std::thread::sleep(Duration::from_secs(2));
    let silent = sim
        .peer_summary("b", "a", "stream", Duration::from_secs(2))
        .unwrap();
    // A static desktop may skip identical pictures, so only delivery is asserted.
    assert!(silent.pictures > 0, "{}", silent.line());
    let report = sim.conn_report("a", true).expect("a's stream connection");
    assert!(
        report.series.video_in.iter().sum::<u64>() > 10_000,
        "{report:?}"
    );

    // Everything goes when they leave.
    sim.peer_leave("a");
    sim.peer_leave("b");
    wait("every connection closes", Duration::from_secs(10), || {
        sim.connections() == 0
    });
    assert_eq!(dc_voice::sim::refused_endpoints(), 0);
}

#[test]
fn peer_leave_in_two_person_call_keeps_voice_and_go_live_stream_connected() {
    let dispatches: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let seen = dispatches.clone();
    let sim = Sim::start(
        Setup {
            me: 1000,
            my_session: "test-session".into(),
            guild: 0,
            channel: 3001,
            people: vec![2001, 2002],
        },
        Arc::new(move |name, data| seen.lock().unwrap().push((name, data))),
    )
    .unwrap();
    sim.peer_add("a", false);
    sim.peer_add("b", false);
    wait("both participants connect", Duration::from_secs(15), || {
        sim.peer_connected("a") && sim.peer_connected("b")
    });

    // Wait for initial DAVE group readiness before a starts Go Live and b watches.
    sim.peer_stream_start("a", 320, 180, 30, dc_media::sim::Content::Motion);
    std::thread::sleep(Duration::from_millis(300));
    sim.peer_watch("b", "a");
    wait(
        "b receives a's stream pictures",
        Duration::from_secs(10),
        || {
            sim.peer_summary("b", "a", "stream", Duration::from_secs(60))
                .is_some_and(|summary| summary.pictures > 0)
        },
    );

    // Remote peer leaves cleanly: SFU sends op 13 ClientDisconnect and op 27 Remove (2 -> 1).
    sim.peer_leave("b");
    std::thread::sleep(Duration::from_secs(2));

    // Sole surviving streamer must keep BOTH voice and Go Live stream alive across removal.
    assert!(sim.peer_connected("a"), "voice disconnected when peer left");
    assert!(
        sim.conn_report("a", false).is_some(),
        "voice connection vanished when peer left"
    );
    assert!(
        sim.conn_report("a", true).is_some(),
        "Go Live stream connection vanished when peer left"
    );
    let events = sim.peer_events("a");
    assert!(
        !events.iter().any(|event| event.contains("] Disconnected")),
        "unexpected disconnect for sole survivor: {events:?}"
    );

    sim.peer_leave("a");
    wait("every connection closes", Duration::from_secs(10), || {
        sim.connections() == 0
    });
    assert_eq!(dc_voice::sim::refused_endpoints(), 0);
}
