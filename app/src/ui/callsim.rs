//! Capture only: the simulated-call mode. `callsim start` runs `dc-callsim` in this process; from then on the voice
//! requests of the app (op 4, op 13, the stream ops 18 to 22, ring and stop-ringing, the ringable check) are answered by
//! the simulator instead of being refused or sent, and the voice and stream connections the app opens go to its local
//! voice servers through the real `dc-voice` code. `dc_voice::sim::activate` makes every other voice endpoint
//! unreachable for the rest of the run and replaces microphone, speaker, camera and screen by simulated ones.
//!
//! The harness steps are `callsim ...` (see [`Root::script_callsim`]); the hook in `net.rs` is [`intercept`].
use super::Root;
use crate::net::{Request, Update};
use dc_callsim::{Feedback, Setup, Shape, Sim};
use gpui::Context;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// The running simulation. It lives until the process ends: its runtime must not be dropped on a runtime thread.
static SIM: Mutex<Option<Arc<Sim>>> = Mutex::new(None);

fn sim() -> Option<Arc<Sim>> {
    SIM.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The voice requests a simulation answers itself. Every one of them would otherwise reach Discord (or be refused by
/// inert mode); none of them may while it runs.
pub(crate) fn claims(request: &Request) -> bool {
    match request {
        Request::VoiceState { .. }
        | Request::StreamOp { .. }
        | Request::StreamConnect { .. }
        | Request::VoiceConnect { .. } => true,
        Request::Rest { route, .. } => call_route(route),
        _ => false,
    }
}

/// `/channels/<id>/call`, `/call/ring`, `/call/stop-ringing`.
fn call_route(route: &str) -> bool {
    let path = route.split('?').next().unwrap_or(route);
    path.starts_with("/channels/")
        && (path.ends_with("/call")
            || path.ends_with("/call/ring")
            || path.ends_with("/call/stop-ringing"))
}

/// The hook in `net.rs`: a request the simulation claims is answered here and never goes further; anything else comes
/// back unchanged. Without a running simulation everything comes back.
pub(crate) fn intercept(request: Request, updates: &UnboundedSender<Update>) -> Option<Request> {
    let Some(sim) = sim() else {
        return Some(request);
    };
    if !claims(&request) {
        return Some(request);
    }
    match request {
        Request::VoiceState {
            guild,
            channel,
            self_mute,
            self_deaf,
            self_video,
            ..
        } => sim.voice_state(guild, channel, (self_mute, self_deaf, self_video)),
        Request::StreamOp { op, data } => sim.stream_op(op as u8, &data),
        Request::VoiceConnect {
            join,
            controls,
            generation,
        } => {
            tap(&sim, &controls.hub());
            let updates = updates.clone();
            tokio::spawn(async move {
                let (task, mut events) = dc_voice::connect(join, &controls);
                while let Some(event) = events.recv().await {
                    if updates.send(Update::Voice(generation, event)).is_err() {
                        break;
                    }
                }
                let _ = task.await;
            });
        }
        Request::StreamConnect {
            join,
            source,
            hub,
            controls,
            generation,
        } => {
            tap(&sim, &hub);
            let key = join.key;
            let updates = updates.clone();
            tokio::spawn(async move {
                let (task, mut events) = dc_voice::connect_stream(join, source, &hub, &controls);
                while let Some(event) = events.recv().await {
                    // Previews would be uploaded to Discord: here they are only counted.
                    if let dc_voice::StreamEvent::Preview { jpeg } = &event {
                        PREVIEWS
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(jpeg.len());
                        continue;
                    }
                    if updates
                        .send(Update::Stream {
                            generation,
                            key,
                            event,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                let _ = task.await;
            });
        }
        Request::Rest {
            key,
            method,
            route,
            body,
        } => {
            let answer = sim.rest(method.as_str(), &route, body.as_ref());
            if let Some(key) = key {
                let body = json!({"ok": answer.is_some(), "status": if answer.is_some() { 200 } else { 404 },
                    "body": answer.unwrap_or(Value::Null)});
                let _ = updates.send(Update::Fetched { key, body });
            }
        }
        _ => {}
    }
    None
}

/// Stream previews the app would have uploaded, by size.
static PREVIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// What our client's hub delivers is recorded on the simulation's board for us.
fn tap(sim: &Sim, hub: &dc_voice::VideoHub) {
    let board = sim.my_board();
    hub.set_tap(Some(Arc::new(move |user, source, width, height| {
        let kind = match source {
            dc_voice::VideoSource::Camera => "camera",
            dc_voice::VideoSource::Stream(_) => "stream",
        };
        board.record(user, kind, width, height);
    })));
}

/// The fixture guild's General voice channel (the harness's `voice join`).
const FIXTURE_GUILD: u64 = 1_568_692_564_637_524_109;
const FIXTURE_VOICE: u64 = 1_568_692_565_136_375_901;

/// `1440p60`-style quality: height and fps; the width is 16:9.
fn quality(text: &str) -> Option<(u32, u32, u32)> {
    let (height, fps) = text.split_once('p')?;
    let (height, fps) = (height.parse::<u32>().ok()?, fps.parse::<u32>().ok()?);
    let width = (height * 16 / 9 + 1) & !1;
    ((90..=2160).contains(&height) && (1..=60).contains(&fps)).then_some((width, height, fps))
}

/// `capacity <kbps> loss <pct> delay <ms> jitter <ms> reorder <pct> queue <ms>`, any subset, or `clear`.
fn shape(words: &[&str]) -> Option<Shape> {
    let mut shape = Shape::default();
    if words == ["clear"] {
        return Some(shape);
    }
    for pair in words.chunks(2) {
        let [name, value] = pair else { return None };
        let number = value.parse::<f64>().ok()?;
        match *name {
            "capacity" => shape.capacity_bps = Some((number * 1000.0) as u64),
            "loss" => shape.loss_percent = number,
            "delay" => shape.delay_ms = number as u64,
            "jitter" => shape.jitter_ms = number as u64,
            "reorder" => shape.reorder_percent = number,
            "queue" => shape.queue_ms = number as u64,
            _ => return None,
        }
    }
    Some(shape)
}

impl Root {
    /// The `callsim` steps:
    /// `start dm|guild|group`, `join`, `leave`, `watch <peer>`, `watch-also <peer>`, `watch-joining <peer>`, `unwatch <peer>`,
    /// `golive <quality> motion|static|text`, `golive stop`, `camera`, `volume <peer> <percent>`,
    /// `peer add <name> [same-account]`, `peer <name> camera on|off`, `peer <name> stream start <quality> <content>`,
    /// `peer <name> stream stop`, `peer <name> watch me|<other>`, `peer <name> unwatch me|<other>`,
    /// `peer <name> leave`, `peer <name> shot <owner> camera|stream <png>`, `link <name> ...` (see [`shape`]),
    /// `sfu twcc on|off remb on|off rr on|off`, `server move`, `kick [stay|moved|gone]`, `stats [seconds]`,
    /// `reset`, `state`, `log`.
    pub(crate) fn script_callsim(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        if let ["start", place] = words {
            return self.callsim_start(place, cx);
        }
        let Some(sim) = sim() else {
            return "no simulation: callsim start dm|guild|group first".to_owned();
        };
        let setup = sim.setup();
        let user_of = |name: &str| -> Option<u64> {
            if name == "me" {
                Some(setup.me)
            } else {
                sim.peer_user(name)
            }
        };
        let said = match words {
            ["join"] => {
                if setup.guild == 0 {
                    // The header's call button: start or join the DM's call, without ringing anyone.
                    self.select_home(Some(setup.channel));
                    self.dm_call_clicked(setup.channel, false, true, cx);
                } else {
                    self.join_voice(setup.guild, setup.channel, cx);
                }
                "joining the simulated call".to_owned()
            }
            ["leave"] => {
                self.leave_voice(cx);
                "left".to_owned()
            }
            ["watch", name] => match user_of(name) {
                Some(owner) => {
                    sim.my_board().arm(owner, "stream");
                    self.watch_stream_of(owner, cx);
                    format!("Watch Stream of {name}")
                }
                None => format!("no participant {name}"),
            },
            // The eye button / shift click: this stream joins the ones already watched (`forceMultiple`).
            ["watch-also", name] => match user_of(name) {
                Some(owner) => {
                    sim.my_board().arm(owner, "stream");
                    self.watch_stream_with(owner, true, cx);
                    format!("Watch Stream of {name} beside the others")
                }
                None => format!("no participant {name}"),
            },
            ["watch-joining", name] => match user_of(name) {
                Some(owner) => {
                    sim.my_board().arm(owner, "stream");
                    self.watch_stream_joining(owner, cx);
                    format!("Watch Stream of {name} from outside the call")
                }
                None => format!("no participant {name}"),
            },
            ["unwatch", name] => match user_of(name) {
                Some(owner) => {
                    self.unwatch_stream_of(owner, cx);
                    format!("stopped watching {name}")
                }
                None => format!("no participant {name}"),
            },
            ["golive", "stop"] => {
                self.stream_stop(cx);
                "our stream stops".to_owned()
            }
            ["golive", quality_text, content] => {
                let (Some((width, height, fps)), Some(content)) =
                    (quality(quality_text), dc_media::sim::Content::parse(content))
                else {
                    return "usage: callsim golive 1440p60 motion|static|text".to_owned();
                };
                let mut plan = dc_voice::sim::plan(&setup.my_session);
                plan.share = Some(content);
                plan.share_tone_hz = Some(dc_callsim::STREAM_TONES[3]);
                dc_voice::sim::set_plan(&setup.my_session, plan);
                let bitrate = if height <= 720 && fps <= 30 { 3_500_000 } else { 9_000_000 };
                let resolution = match height {
                    480 => dc_media::screen::Resolution::P480,
                    720 => dc_media::screen::Resolution::P720,
                    1080 => dc_media::screen::Resolution::P1080,
                    1440 => dc_media::screen::Resolution::P1440,
                    _ => dc_media::screen::Resolution::Source,
                };
                self.stream_start(
                    dc_voice::StreamSource::Synthetic {
                        width,
                        height,
                        fps,
                        bitrate,
                    },
                    super::voice_video::Started {
                        name: "Simulated Screen".to_owned(),
                        info: dc_media::screen::Source {
                            id: dc_media::screen::SourceId::Display(1),
                            name: "Simulated Screen".to_owned(),
                            width,
                            height,
                            refresh_hz: fps,
                            icon: None,
                        },
                        quality: (resolution, fps),
                        icon: None,
                    },
                    cx,
                );
                format!("Go Live {width}x{height}@{fps} {}", content.name())
            }
            ["camera"] => {
                self.voice_camera_clicked(cx);
                "the camera button".to_owned()
            }
            ["volume", name, percent] => match (user_of(name), percent.parse::<f32>()) {
                (Some(owner), Ok(percent)) => {
                    super::stream_volume::set_percent(owner, percent);
                    self.apply_stream_volume(owner);
                    format!("stream volume of {name}: {percent}%")
                }
                _ => "usage: callsim volume <peer> <percent>".to_owned(),
            },
            ["peer", "add", name] => sim.peer_add(name, false),
            ["peer", "add", name, "same-account"] => sim.peer_add(name, true),
            ["peer", name, "camera", "on"] => sim.peer_camera(name, true),
            ["peer", name, "camera", "off"] => sim.peer_camera(name, false),
            ["peer", name, "stream", "start", quality_text, content] => {
                match (quality(quality_text), dc_media::sim::Content::parse(content)) {
                    (Some((width, height, fps)), Some(content)) => {
                        sim.peer_stream_start(name, width, height, fps, content)
                    }
                    _ => "usage: callsim peer <name> stream start 1440p60 motion|static|text".to_owned(),
                }
            }
            ["peer", name, "stream", "stop"] => sim.peer_stream_stop(name),
            ["peer", name, "watch", owner] => sim.peer_watch(name, owner),
            ["peer", name, "unwatch", owner] => sim.peer_unwatch(name, owner),
            ["peer", name, "leave"] => sim.peer_leave(name),
            ["peer", name, "events"] => sim.peer_events(name).join("
"),
            ["peer", name, "shot", owner, kind, path] => {
                sim.peer_shot(name, owner, kind, std::path::Path::new(path))
            }
            ["link", name, rest @ ..] => match shape(rest) {
                Some(shape) => {
                    sim.set_link(name, shape);
                    format!("link of {name}: {}", shape.describe())
                }
                None => "usage: callsim link <name> capacity <kbps> loss <pct> delay <ms> jitter <ms> reorder <pct> queue <ms> | clear".to_owned(),
            },
            ["sfu", rest @ ..] => {
                let mut feedback: Feedback = sim.feedback();
                for pair in rest.chunks(2) {
                    let on = pair.get(1) == Some(&"on");
                    match pair[0] {
                        "twcc" => feedback.transport_cc = on,
                        "remb" => feedback.remb = on,
                        "rr" => feedback.receiver_reports = on,
                        _ => {}
                    }
                }
                sim.set_feedback(feedback);
                format!("server feedback {feedback:?}")
            }
            ["server", "move"] => sim.server_move(),
            // `STREAM_SERVER_UPDATE` again for a stream we watch or send, and the old connection dropped.
            ["stream", "move", name] => match user_of(name) {
                Some(owner) => sim.stream_move(owner),
                None => format!("no participant {name}"),
            },
            // Our stream connections are dropped with this close code, and nothing follows on the gateway.
            ["stream", "drop", code] => match code.parse() {
                Ok(code) => sim.stream_drop(code),
                Err(_) => "usage: callsim stream drop <close code>".to_owned(),
            },
            ["kick"] => sim.kick("stay"),
            ["kick", after] => sim.kick(after),
            ["stats"] => sim.stats(Duration::from_secs(30)),
            ["stats", seconds] => sim.stats(Duration::from_secs(seconds.parse().unwrap_or(30))),
            ["reset"] => {
                sim.my_board().clear();
                dc_voice::sim::reset_report(&setup.my_session);
                "our measurements start again".to_owned()
            }
            ["state"] => self.callsim_state(&sim),
            ["log"] => sim.log().join("\n"),
            _ => "usage: see Root::script_callsim".to_owned(),
        };
        cx.notify();
        said
    }

    fn callsim_state(&self, sim: &Sim) -> String {
        let call = self.call.as_ref().map_or("no call".to_owned(), |call| {
            format!(
                "call connected {} camera {} others {}",
                call.connected,
                call.camera,
                call.has_others(self.stores.current_user.as_ref().map(|me| me.id))
            )
        });
        let focus = match self.vv.focus.tile {
            Some((user, kind)) => format!(
                "{} {:?}",
                if Some(user) == self.stores.current_user.as_ref().map(|me| me.id) {
                    "me".to_owned()
                } else {
                    "a participant".to_owned()
                },
                kind
            ),
            None => "none".to_owned(),
        };
        let counts = dc_voice::sim::counts();
        format!(
            "{call}; video {}; focused tile {focus}; server connections {}; process voice connections {} stream connections {} audio workers {}; previews not uploaded {}; endpoints refused {}",
            self.video.describe(),
            sim.connections(),
            counts.calls,
            counts.streams,
            counts.audio_workers,
            PREVIEWS.lock().unwrap_or_else(|e| e.into_inner()).len(),
            dc_voice::sim::refused_endpoints()
        )
    }

    fn callsim_start(&mut self, place: &str, cx: &mut Context<Self>) -> String {
        if sim().is_some() {
            return "a simulation is running already".to_owned();
        }
        let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
            return "not signed in yet".to_owned();
        };
        let my_session = self
            .stores
            .session_id
            .clone()
            .unwrap_or_else(|| "callsim-session".to_owned());
        let other = self
            .stores
            .private_channels
            .iter()
            .find(|dm| dm.id == super::dm_call::TEST_DM)
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id);
        let (guild, channel, people) = match place {
            "dm" => {
                let Some(other) = other else {
                    return "the test DM is not loaded".to_owned();
                };
                (0, super::dm_call::TEST_DM, vec![other])
            }
            "group" => {
                let Some(other) = other else {
                    return "the test DM is not loaded".to_owned();
                };
                // The local group DM of `dmcall group`, with the test person and two made-up people.
                let _ = self.script_dmcall(&["group"], cx);
                (
                    0,
                    super::dm_call::GROUP_DM,
                    vec![other, 1_570_000_000_000_000_201, 1_570_000_000_000_000_202],
                )
            }
            "guild" => (FIXTURE_GUILD, FIXTURE_VOICE, other.into_iter().collect()),
            _ => return "usage: callsim start dm|guild|group".to_owned(),
        };
        let updates = self.updates.clone();
        let sink: dc_callsim::Sink = Arc::new(move |name, data| {
            let _ = updates.send(Update::Dispatch { name, data });
        });
        // Our microphone sends nothing; our speaker listens for every participant's voice and stream tone.
        dc_voice::sim::set_plan(
            &my_session,
            dc_voice::sim::Plan {
                tone_hz: None,
                level: 0.0,
                listen_hz: dc_callsim::VOICE_TONES
                    .iter()
                    .chain(dc_callsim::STREAM_TONES.iter())
                    .copied()
                    .collect(),
                share: None,
                share_tone_hz: None,
            },
        );
        match Sim::start(
            Setup {
                me,
                my_session,
                guild,
                channel,
                people,
            },
            sink,
        ) {
            Ok(started) => {
                *SIM.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(started));
                if guild == 0 {
                    self.select_home(Some(channel));
                }
                cx.notify();
                format!(
                    "simulating a {place} call; voice requests are answered locally from now on"
                )
            }
            Err(error) => format!("the simulation did not start: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every voice-related request is the simulation's while it runs: none reaches the gateway or the REST client.
    #[test]
    fn every_voice_request_is_claimed_and_nothing_else() {
        let claimed = [
            Request::VoiceState {
                guild: 0,
                channel: Some(5),
                self_mute: false,
                self_deaf: false,
                self_video: false,
                flags: 0,
            },
            Request::StreamOp {
                op: dc_proto::op::Op::CallConnect,
                data: json!({}),
            },
            Request::StreamOp {
                op: dc_proto::op::Op::StreamWatch,
                data: json!({}),
            },
            Request::Rest {
                key: None,
                method: reqwest::Method::POST,
                route: "/channels/5/call/ring".into(),
                body: None,
            },
            Request::Rest {
                key: None,
                method: reqwest::Method::POST,
                route: "/channels/5/call/stop-ringing".into(),
                body: None,
            },
            Request::Rest {
                key: Some("k".into()),
                method: reqwest::Method::GET,
                route: "/channels/5/call".into(),
                body: None,
            },
        ];
        for request in &claimed {
            assert!(claims(request), "{}", request.variant());
        }
        let passed = [
            Request::Rest {
                key: None,
                method: reqwest::Method::GET,
                route: "/channels/5/messages".into(),
                body: None,
            },
            Request::Typing { channel: 5 },
        ];
        for request in &passed {
            assert!(!claims(request), "{}", request.variant());
        }
        // Op 4 and the stream ops are the only gateway sends of a call (`net.rs`), and both arms are claimed above.
        // That `dc-voice` refuses every endpoint but the simulator's once it is active is tested in `dc-voice`
        // (`sim::tests::only_the_local_simulator_is_an_endpoint`) and end to end in `dc-callsim`
        // (`tests/chain.rs`): activating it here would change every other test of this binary.
    }

    #[test]
    fn steps_parse_qualities_and_links() {
        assert_eq!(quality("1440p60"), Some((2560, 1440, 60)));
        assert_eq!(quality("720p30"), Some((1280, 720, 30)));
        assert_eq!(quality("180p15"), Some((320, 180, 15)));
        assert_eq!(quality("1440"), None);
        let link = shape(&["capacity", "3000", "loss", "2", "delay", "20"]).unwrap();
        assert_eq!(link.capacity_bps, Some(3_000_000));
        assert_eq!((link.loss_percent, link.delay_ms), (2.0, 20));
        assert_eq!(shape(&["clear"]), Some(Shape::default()));
        assert!(shape(&["capacity"]).is_none());
        assert!(call_route("/channels/9/call/ring?x=1"));
        assert!(!call_route("/channels/9/messages"));
    }
}
