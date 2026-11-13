//! Headless run of the simulator without the app: participant `b` goes live, participant `a` watches, the numbers
//! are printed every two seconds.
//!
//! `cargo run -p dc-callsim --example watch -- <width> <height> <fps> motion|static|text <seconds> [link args]`,
//! link args as the harness step: `capacity 8000 loss 2 delay 20 jitter 5` (applied to `a`), plus `twcc off`,
//! `remb on`, `rr off` for the server's feedback.
use dc_callsim::{Feedback, Setup, Shape, Sim};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let number =
        |i: usize, default: u64| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(default);
    let (width, height, fps) = (
        number(0, 320) as u32,
        number(1, 180) as u32,
        number(2, 30) as u32,
    );
    let content = args
        .get(3)
        .and_then(|c| dc_media::sim::Content::parse(c))
        .unwrap_or(dc_media::sim::Content::Motion);
    let seconds = number(4, 10);
    let mut shape = Shape::default();
    let mut feedback = Feedback::default();
    for pair in args.get(5..).unwrap_or_default().chunks(2) {
        let (name, value) = (pair[0].as_str(), pair.get(1).map_or("", String::as_str));
        let n = value.parse::<f64>().unwrap_or(0.0);
        match name {
            "capacity" => shape.capacity_bps = Some((n * 1000.0) as u64),
            "loss" => shape.loss_percent = n,
            "delay" => shape.delay_ms = n as u64,
            "jitter" => shape.jitter_ms = n as u64,
            "twcc" => feedback.transport_cc = value == "on",
            "remb" => feedback.remb = value == "on",
            "rr" => feedback.receiver_reports = value == "on",
            _ => {}
        }
    }
    let sim = Sim::start(
        Setup {
            me: 1000,
            my_session: "example".into(),
            guild: 0,
            channel: 3000,
            people: vec![2001, 2002],
        },
        Arc::new(|_, _| {}),
    )
    .expect("the simulator starts");
    sim.set_feedback(feedback);
    sim.set_link("a", shape);
    sim.peer_add("a", false);
    sim.peer_add("b", false);
    while !(sim.peer_connected("a") && sim.peer_connected("b")) {
        std::thread::sleep(Duration::from_millis(100));
    }
    println!(
        "{}",
        sim.peer_stream_start("b", width, height, fps, content)
    );
    std::thread::sleep(Duration::from_secs(1));
    println!("{}", sim.peer_watch("a", "b"));
    for _ in 0..seconds / 2 {
        std::thread::sleep(Duration::from_secs(2));
        if let Some(summary) = sim.peer_summary("a", "b", "stream", Duration::from_secs(2)) {
            println!("a sees b: {}", summary.line());
        }
    }
    println!("{}", sim.stats(Duration::from_secs(seconds.min(30))));
    for line in sim
        .peer_events("a")
        .iter()
        .chain(sim.peer_events("b").iter())
    {
        println!("{line}");
    }
}
