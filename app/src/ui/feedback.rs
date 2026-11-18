//! The in-app feedback surveys after a call, a stream and a report.
//!
//! - The manager (module 220162, `FeedbackManager`): `VOICE_CHANNEL_SHOW_FEEDBACK` when we leave a voice channel
//!   (module 597798), `STREAM_CLOSE` when we stop a stream or stop watching one (`D` of module 401843), and
//!   `IN_APP_REPORTS_SHOW_FEEDBACK` after a report (`Zv` of module 369053) may each open a survey. Module 409181's
//!   configs gate it: a 20% chance (50% for reports), a cooldown over the type's group since any of the group's last
//!   surveys (a day for the call and stream ones, two days for reports), no opt-out (`optOutExpiryTime`), and for the
//!   call one, a call that never connected or once had someone else in it. When two are asked for within the 200 ms
//!   debounce, the one first in `uf` of module 670455 wins (stream before call). Showing one stamps its
//!   `lastImpressionTime` in the account's `inAppFeedbackSettings`.
//! - The call (module 9501) and stream (module 218738) surveys are `FeedbackModalV2` (module 382935): the three faces
//!   (module 321563), then the trouble category (call only), then the problem list (module 124480), and for Other a
//!   text box ("Describe your issue" with the help request link). The report survey (module 555738) is the older
//!   `FeedbackModal` (module 612479): the faces stay above its problem list.
//! - Every way out of a survey submits what was picked (the modal's unmount effect): a good face, a problem without a
//!   text box, Submit, Escape and the scrim. A problem then opens "Feedback Submitted" (module 845671) with the
//!   survey's thanks, or for a bad or neutral face one time in a hundred (module 878234) the Upload Logs question
//!   (module 72514). "Don't show again" opts the type out (`n3` of module 930932).
//!
//! Laid out from the capture `design/dom/modals/report-feedback-darker-cozy.json` (the 612479 survey on its faces) and
//! the CSS of modules 830141, 694413, 164249 and 439248 for the later steps, which no capture shows.

use super::chat_notices::{Alert, AlertButton, Content, Then};
use super::{Root, color, icon};
use crate::editor::Editor;
use crate::net::Request;
use dc_proto::settings::InAppFeedbackState;
use gpui::{
    AnyElement, BoxShadow, Context, Entity, FontWeight, IntoElement, SharedString, deferred, div,
    img, point, prelude::*, px,
};
use rand::seq::SliceRandom;
use serde_json::json;
use std::time::Duration;

/// `getSubmitRequestURL` of module 975571 on Windows (`platform` is `process.platform`).
const HELP_REQUEST_URL: &str = "https://support.discord.com/hc/en-us/requests/new?platform=win32";
/// The faces of module 321563 (`face` of the `2026-06-post-call-survey-assets` experiment, its default), as the CSS of
/// module 164249 names their pictures.
const FACES: [&str; 3] = [
    "https://discord.com/assets/6251823cd63fdeac.svg",
    "https://discord.com/assets/c0070cb5c7749661.svg",
    "https://discord.com/assets/d7b222069635263e.svg",
];
/// `fs` of module 670455: the opt-out of "Don't show again" (year 2243).
const OPT_OUT_FOREVER_MS: u64 = 8_640_000_000_000_000;
const DAY_MS: u64 = 86_400_000;
/// `maxLength` of the text box.
const MAX_TEXT: usize = 1024;

/// A survey type (`InAppFeedbackType`, the keys of `inAppFeedbackSettings`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Voice,
    Stream {
        streamer: bool,
    },
    Report,
    /// The video background survey (module 932140), asked for instead of the call's when a background was used.
    VideoBackground,
}

impl Kind {
    fn number(self) -> u64 {
        match self {
            Kind::Voice => 0,
            Kind::Stream { .. } => 1,
            Kind::Report => 4,
            Kind::VideoBackground => 2,
        }
    }

    /// The position in `uf` of module 670455 (VIDEO_BACKGROUND, STREAM, ACTIVITY, VOICE, IN_APP_REPORTS, ...): a survey
    /// already waiting keeps its place against one later in the list.
    fn rank(self) -> u8 {
        match self {
            Kind::VideoBackground => 0,
            Kind::Stream { .. } => 1,
            Kind::Voice => 3,
            Kind::Report => 4,
        }
    }

    /// Module 409181: `chance` and `cooldown`.
    fn config(self) -> (f64, u64) {
        match self {
            Kind::Voice | Kind::Stream { .. } | Kind::VideoBackground => (0.2, DAY_MS),
            Kind::Report => (0.5, 2 * DAY_MS),
        }
    }

    /// The types of the same group (`AV` holds VOICE, STREAM, VIDEO_BACKGROUND and ACTIVITY; `SAFETY` the reports):
    /// a survey waits out its cooldown since the last one of any of them.
    fn group(self) -> &'static [u64] {
        match self {
            Kind::Voice | Kind::Stream { .. } | Kind::VideoBackground => &[0, 1, 2, 3],
            Kind::Report => &[4],
        }
    }
}

/// `C` and `R` of module 220162 over the stored states: not opted out, and the cooldown passed for every type of the
/// group. (`O`, the chance, and the call's own check are the caller's.)
pub(super) fn may_show(
    kind: Kind,
    state_of: impl Fn(u64) -> InAppFeedbackState,
    now_ms: u64,
) -> bool {
    if state_of(kind.number())
        .opt_out_expiry_ms
        .is_some_and(|until| now_ms < until)
    {
        return false;
    }
    let (_, cooldown) = kind.config();
    kind.group().iter().all(|other| {
        state_of(*other)
            .last_impression_ms
            .unwrap_or(0)
            .saturating_add(cooldown)
            < now_ms
    })
}

/// The face picked (`P0` of module 670455).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rating {
    Bad,
    Neutral,
    Good,
}

const RATINGS: [Rating; 3] = [Rating::Bad, Rating::Neutral, Rating::Good];

#[derive(Clone, Debug)]
struct Problem {
    /// The option's value (`reason_code` of the survey's analytics), read by the harness.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    value: &'static str,
    /// The English text of module 205176's table (or of the main one), looked up with `tr`.
    label: &'static str,
    freeform: bool,
}

#[derive(Clone, Debug)]
struct Category {
    label: &'static str,
    problems: Vec<Problem>,
}

fn problem(value: &'static str, label: &'static str) -> Problem {
    Problem {
        value,
        label,
        freeform: false,
    }
}

fn other() -> Problem {
    Problem {
        value: "FREEFORM",
        label: "Other",
        freeform: true,
    }
}

/// The option trees of module 930932 (`A_` connection, `Op` audio, `de` camera, `TK` people, `wq` stream), each
/// shuffled with Other last (`S` of module 382935).
fn categories(kind: Kind, rng: &mut impl rand::Rng) -> Vec<Category> {
    if kind == Kind::VideoBackground {
        // Module 932140 lists its problems in this order, OTHER last and with the text box; nothing shuffles them.
        return vec![Category {
            label: "",
            problems: vec![
                problem(
                    "BAD_OUTLINE",
                    "The outline on my body was fuzzy or had a halo",
                ),
                problem(
                    "BACKGROUND_DETECTION_ACCURACY",
                    "The background detection was not accurate",
                ),
                problem("FLICKERING", "I saw flickering objects on the screen"),
                problem(
                    "BACKGROUND_DETECTION_LAG",
                    "Background detection was laggy when I moved",
                ),
                Problem {
                    value: "OTHER",
                    label: "Other",
                    freeform: true,
                },
            ],
        }];
    }
    let mut tree = match kind {
        Kind::Voice => vec![
            Category {
                label: "Connection or latency",
                problems: vec![
                    problem("COULD_NOT_CONNECT", "I couldn't connect"),
                    problem("HIGH_TTC", "It took a long time for me to connect"),
                    problem("HIGH_LATENCY", "I had high latency while in the call"),
                    problem("ROBOT_VOICE", "Someone else's voice sounded robotic"),
                    problem("DESYNC", "Audio and video were desynced"),
                    problem("CUTTING", "Someone else's voice was cutting in and out"),
                ],
            },
            Category {
                label: "Microphone or audio",
                problems: vec![
                    problem("NO_AUDIO:SELF", "People couldn't hear me at all"),
                    problem("NO_AUDIO:OTHERS", "I couldn't hear anyone at all"),
                    problem("LOW_QUALITY_AUDIO", "Other people had low quality audio"),
                    problem("ROBOT", "Someone else's voice sounded robotic"),
                    problem("ECHO", "Other people were echoing"),
                    problem(
                        "TOO_QUIET_OR_LOUD",
                        "Someone else's voice was too quiet or too loud",
                    ),
                    problem("CUTTING", "Someone else's voice was cutting in and out"),
                    problem("DELAYED", "Audio was delayed"),
                    problem(
                        "BACKGROUND_NOISE",
                        "Someone else had a lot of background noise",
                    ),
                    problem(
                        "UNABLE_TO_FIND_DEVICE",
                        "I couldn't use the correct audio device",
                    ),
                    problem("COMPLAINTS", "Other people complained about my audio"),
                    // TROUBLE_WITH_SPEAKERPHONE is mobile only (`disabled: !isMobile`).
                    problem("NO_GAME_AUDIO", "I couldn't hear my game's audio"),
                ],
            },
            Category {
                label: "Camera",
                problems: vec![
                    problem("NO_VIDEO:SELF", "My camera only showed a black screen"),
                    problem(
                        "NO_VIDEO:SOMEONE",
                        "Someone else's camera only showed a black screen",
                    ),
                    problem(
                        "FREEZING_OR_HITCHING",
                        "Someone else's camera feed was freezing or hitching",
                    ),
                    problem(
                        "BLURRY_OR_PIXELATED",
                        "Someone else's camera feed was blurry or pixelated",
                    ),
                    problem(
                        "UNABLE_TO_ENABLE_DEVICE",
                        "I couldn't find or turn on my camera",
                    ),
                    problem("DESYNC", "Audio and video were desynced"),
                ],
            },
            Category {
                label: "People in the call",
                problems: vec![problem(
                    "TOXIC_OR_INAPPROPRIATE",
                    "People were being toxic or inappropriate",
                )],
            },
        ],
        Kind::Stream { streamer } => vec![Category {
            label: if streamer {
                "Streaming"
            } else {
                "Watching a stream"
            },
            problems: if streamer {
                vec![
                    problem("COULD_NOT_LOAD", "People couldn't load my stream"),
                    problem("BLACK_SCREEN", "People said my stream was a black screen"),
                    problem(
                        "FREEZING_OR_HITCHING",
                        "People said my stream was freezing or hitching",
                    ),
                    problem("LAG", "People said my stream was lagging"),
                    problem(
                        "BLURRY_OR_PIXELATED",
                        "People said my stream was blurry or pixelated",
                    ),
                    problem("NO_AUDIO", "People said they couldn't hear my stream"),
                    problem("BAD_AUDIO", "People said my stream had low audio quality"),
                    problem("STOPPED_UNEXPECTEDLY", "My stream stopped unexpectedly"),
                    problem(
                        "DESYNC",
                        "People said my stream's video and audio were desynced",
                    ),
                    problem("NO_GAME_AUDIO", "I couldn't hear my game's audio"),
                ]
            } else {
                vec![
                    problem("COULD_NOT_LOAD", "I couldn't load the stream"),
                    problem("BLACK_SCREEN", "The stream was a black screen"),
                    problem(
                        "FREEZING_OR_HITCHING",
                        "The stream was freezing or hitching",
                    ),
                    problem("LAG", "The stream was lagging"),
                    problem("BLURRY_OR_PIXELATED", "The stream was blurry or pixelated"),
                    problem("NO_AUDIO", "I couldn't hear the stream"),
                    problem("BAD_AUDIO", "The stream had low audio quality"),
                    problem("STOPPED_UNEXPECTEDLY", "The stream stopped unexpectedly"),
                    problem("DESYNC", "The stream's audio and video were desynced"),
                ]
            },
        }],
        // Module 555738's three reasons; `ed` of module 930932 shuffles them with Other last.
        Kind::Report => vec![Category {
            label: "",
            problems: vec![
                problem(
                    "I couldn't find what I was looking for",
                    "I couldn't find what I was looking for",
                ),
                problem(
                    "I found the language confusing",
                    "I found the language in the menu confusing",
                ),
            ],
        }],
        Kind::VideoBackground => Vec::new(),
    };
    tree.shuffle(rng);
    for category in &mut tree {
        category.problems.shuffle(rng);
        category.problems.push(other());
    }
    tree
}

/// What the survey shows (`ec` of module 612479, the branches of module 382935).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Rating,
    Category,
    Problem,
    Freeform,
}

/// The picks of one survey; the pure half of the form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Picks {
    pub rating: Option<Rating>,
    pub category: Option<usize>,
    pub problem: Option<usize>,
}

impl Picks {
    /// The step for these picks, `single` when there is one category (it is picked from the start).
    pub(super) fn step(&self, freeform: impl Fn(usize, usize) -> bool) -> Step {
        match (self.rating, self.category, self.problem) {
            (None, ..) => Step::Rating,
            (Some(_), None, _) => Step::Category,
            (Some(_), Some(_), None) => Step::Problem,
            (Some(_), Some(category), Some(problem)) if freeform(category, problem) => {
                Step::Freeform
            }
            _ => Step::Problem,
        }
    }

    /// A face is picked: Good closes (and submits) at once.
    pub(super) fn pick_rating(&mut self, rating: Rating) -> bool {
        self.rating = Some(rating);
        rating == Rating::Good
    }

    /// A problem is picked: one without a text box closes at once.
    pub(super) fn pick_problem(&mut self, problem: usize, freeform: bool) -> bool {
        self.problem = Some(problem);
        !freeform
    }

    /// Back (`l` of module 382935): from the text box to the problems, from the problems to the categories (or to
    /// the faces when there is one category), from the categories to the faces.
    pub(super) fn back(&mut self, single: bool) {
        if self.problem.take().is_some() {
            return;
        }
        if self.category.is_some() && !single {
            self.category = None;
            return;
        }
        self.rating = None;
        if !single {
            self.category = None;
        }
    }
}

/// What submitting opens next (the `onSubmit` of modules 9501, 218738 and 555738).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum After {
    Nothing,
    /// Module 845671 with the survey's thanks.
    Thanks,
    /// Module 72514.
    UploadLogs,
}

/// `onSubmit`: nothing without a face and a problem; else Upload Logs for a bad or neutral face when `roll` (a
/// number in 0..1) is under 0.01 (module 878234, call and stream only), else the thanks.
pub(super) fn after_submit(kind: Kind, picks: &Picks, roll: f64) -> After {
    if picks.rating.is_none() || picks.problem.is_none() {
        return After::Nothing;
    }
    let bad = matches!(picks.rating, Some(Rating::Bad | Rating::Neutral));
    if !matches!(kind, Kind::Report | Kind::VideoBackground)
        && bad
        && picks.category.is_some()
        && roll < 0.01
    {
        return After::UploadLogs;
    }
    After::Thanks
}

pub(super) struct Form {
    kind: Kind,
    tree: Vec<Category>,
    picks: Picks,
    dont_show: bool,
    text: Entity<Editor>,
    hovered_face: Option<usize>,
}

impl Form {
    fn single(&self) -> bool {
        self.tree.len() == 1
    }

    fn freeform(&self, category: usize, problem: usize) -> bool {
        self.tree
            .get(category)
            .and_then(|category| category.problems.get(problem))
            .is_some_and(|problem| problem.freeform)
    }

    fn step(&self) -> Step {
        self.picks
            .step(|category, problem| self.freeform(category, problem))
    }
}

/// The manager's state: the open survey, and the one waiting out the debounce.
#[derive(Default)]
pub(super) struct Feedback {
    pub form: Option<Form>,
    pending: Option<Kind>,
}

impl Root {
    /// `possiblyShowFeedbackModal`: the gates, then after 200 ms the survey (unless a higher one took its place).
    pub(super) fn possibly_show_feedback(&mut self, kind: Kind, cx: &mut Context<Self>) {
        // A harness run never rolls the dice (a survey would cover other frames at random); `feedback gate` drives
        // the gates there.
        if crate::storage::is_capture_run() {
            return;
        }
        let (chance, _) = kind.config();
        if rand::random::<f64>() >= chance {
            return;
        }
        self.show_feedback_after_debounce(kind, cx);
    }

    /// The report modal closed (`onCloseCallback` of module 882276): `IN_APP_REPORTS_SHOW_FEEDBACK`, whether or not a
    /// report was sent (`isEligibleForFeedback` is true for every report this client opens).
    pub(super) fn report_closed(&mut self, cx: &mut Context<Self>) {
        if self.report.take().is_some() {
            self.possibly_show_feedback(Kind::Report, cx);
        }
    }

    /// We leave the voice channel (`selectVoiceChannel(null)` of module 597798): `VOICE_CHANNEL_SHOW_FEEDBACK`, for a
    /// call that never connected or that once had someone else in it (module 409181's eligibility check).
    pub(super) fn voice_left_by_hand(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &self.call else {
            return;
        };
        let ever_connected = call.connected_at.is_some();
        if !ever_connected || call.ever_multi {
            // `g.A.hasUsedBackgroundInCall`: VIDEO_BACKGROUND_SHOW_FEEDBACK (module 932140) instead of the call's.
            let kind = if call.used_background {
                Kind::VideoBackground
            } else {
                Kind::Voice
            };
            self.possibly_show_feedback(kind, cx);
        }
    }

    fn feedback_allowed(&self, kind: Kind) -> bool {
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        may_show(kind, |number| self.stores.in_app_feedback(number), now)
    }

    fn show_feedback_after_debounce(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if !self.feedback_allowed(kind) || self.feedback.form.is_some() {
            return;
        }
        if self
            .feedback
            .pending
            .is_some_and(|waiting| waiting.rank() < kind.rank())
        {
            return;
        }
        let first = self.feedback.pending.replace(kind).is_none();
        if !first {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(kind) = this.feedback.pending.take() {
                    this.write_feedback_state(kind, |state| {
                        state.last_impression_ms =
                            Some(chrono::Utc::now().timestamp_millis().max(0) as u64);
                    });
                    this.open_feedback(kind, cx);
                }
            });
        })
        .detach();
    }

    /// The account's `inAppFeedbackSettings` entry for `kind`, changed and written (`updateSetting`).
    fn write_feedback_state(&mut self, kind: Kind, change: impl FnOnce(&mut InAppFeedbackState)) {
        let mut state = self.stores.in_app_feedback(kind.number());
        change(&mut state);
        let patch = self.stores.set_in_app_feedback(kind.number(), state);
        if !patch.is_empty() && !crate::storage::settings_writes_blocked() {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".into(),
                body: Some(json!({ "settings": patch })),
            });
        }
    }

    /// Opens the survey at once (the harness, and the manager after its gates).
    pub(super) fn open_feedback(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let text = cx.new(|cx| Editor::new(super::editor_style(), cx));
        let tree = categories(kind, &mut rand::rng());
        let single = tree.len() == 1;
        self.feedback.form = Some(Form {
            kind,
            tree,
            picks: Picks {
                category: single.then_some(0),
                ..Picks::default()
            },
            dont_show: false,
            text,
            hovered_face: None,
        });
        for face in FACES {
            if !self.images.contains_key(face) {
                self.want_image(face.to_owned(), face.to_owned(), cx);
            }
        }
        cx.notify();
    }

    /// The survey closes, whichever way: what was picked is submitted.
    pub(super) fn close_feedback(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.feedback.form.take() else {
            return;
        };
        if form.dont_show {
            self.write_feedback_state(form.kind, |state| {
                state.opt_out_expiry_ms = Some(OPT_OUT_FOREVER_MS);
            });
        }
        // GAP: the call survey's "People couldn't hear me at all" while muted for 98% of the call opens module
        // 813923 (the mute tutorial question), which is not built; the analytics events of the surveys are not sent.
        let thanks = match form.kind {
            Kind::Voice | Kind::Report => "d9+vQ8",
            Kind::VideoBackground => "aHB11f",
            Kind::Stream { .. } => "mMTVnv",
        };
        let roll = self.feedback_roll.take().unwrap_or_else(rand::random);
        let alert = match after_submit(form.kind, &form.picks, roll) {
            After::Nothing => None,
            // "Feedback Submitted" (module 845671): the thanks and Done.
            After::Thanks => Some(Alert {
                id: "feedback-submitted",
                title: dc_intl::t("KTjjrG").to_owned(),
                subtitle: None,
                content: Some(Content::Text(dc_intl::t(thanks).to_owned())),
                buttons: vec![AlertButton {
                    id: "feedback-submitted-done",
                    label: dc_intl::t("i4jeWR").to_owned(),
                    secondary: false,
                    then: Then::Close,
                }],
            }),
            // Module 72514: Cancel and Upload Logs (`uploadDebugLogFiles(RTC)`).
            After::UploadLogs => Some(Alert {
                id: "feedback-upload-logs",
                title: dc_intl::t("KTjjrG").to_owned(),
                subtitle: None,
                content: Some(Content::Text(dc_intl::t("ZvRR/t").to_owned())),
                buttons: vec![
                    AlertButton {
                        id: "feedback-upload-cancel",
                        label: dc_intl::t("ETE/oC").to_owned(),
                        secondary: true,
                        then: Then::Close,
                    },
                    AlertButton {
                        id: "feedback-upload-logs",
                        label: dc_intl::t("EbwFfR").to_owned(),
                        secondary: false,
                        then: Then::UploadLogs,
                    },
                ],
            }),
        };
        if let Some(alert) = alert {
            self.chat_extra.alert = Some(alert);
        }
        cx.notify();
    }

    /// Capture only: `feedback voice|stream|stream-viewer|report` opens a survey without its gates, `feedback
    /// thanks|upload` the dialog after it, `feedback state` the picks.
    #[cfg(feature = "capture")]
    pub(crate) fn script_feedback(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["voice"] => self.open_feedback(Kind::Voice, cx),
            ["stream"] => self.open_feedback(Kind::Stream { streamer: true }, cx),
            ["stream-viewer"] => self.open_feedback(Kind::Stream { streamer: false }, cx),
            ["report"] => self.open_feedback(Kind::Report, cx),
            ["video"] => self.open_feedback(Kind::VideoBackground, cx),
            // The call's camera ran with a background (what VideoState does with a background set).
            ["used-background"] => {
                if let Some(call) = self.call.as_mut() {
                    call.used_background = true;
                }
                return format!("used background: {}", self.call.is_some());
            }
            // What leaving the call asks for: the video background survey when a background was used.
            ["leave-kind"] => {
                return format!(
                    "leave kind: {}",
                    if self.call.as_ref().is_some_and(|call| call.used_background) {
                        "video_background"
                    } else {
                        "voice"
                    }
                );
            }
            // The gates without the chance: what leaving a call, stopping a stream or closing a report asks.
            ["gate", kind] => {
                let kind = match *kind {
                    "voice" => Kind::Voice,
                    "stream" => Kind::Stream { streamer: true },
                    "report" => Kind::Report,
                    "video" => Kind::VideoBackground,
                    _ => return "usage: feedback gate voice|stream|report|video".to_owned(),
                };
                let allowed = self.feedback_allowed(kind);
                self.show_feedback_after_debounce(kind, cx);
                return format!("{kind:?} survey allowed {allowed}");
            }
            // The dialog a survey's 1% roll opens (72514).
            ["upload"] => {
                self.feedback.form = Some(Form {
                    kind: Kind::Voice,
                    tree: categories(Kind::Voice, &mut rand::rng()),
                    picks: Picks {
                        rating: Some(Rating::Bad),
                        category: Some(0),
                        problem: Some(0),
                    },
                    dont_show: false,
                    text: cx.new(|cx| Editor::new(super::editor_style(), cx)),
                    hovered_face: None,
                });
                self.feedback_roll = Some(0.0);
                self.close_feedback(cx);
                return "upload logs asked".to_owned();
            }
            // Picks Other of the open problem list, wherever the shuffle put it.
            ["other"] => {
                if let Some(form) = self.feedback.form.as_mut()
                    && let Some(category) = form.picks.category
                {
                    let last = form.tree[category].problems.len() - 1;
                    form.picks.pick_problem(last, true);
                }
                cx.notify();
                return "Other picked".to_owned();
            }
            ["state"] => {
                return match &self.feedback.form {
                    Some(form) => {
                        let problem = form
                            .picks
                            .category
                            .zip(form.picks.problem)
                            .and_then(|(category, problem)| {
                                form.tree[category].problems.get(problem)
                            })
                            .map_or("none", |problem| problem.value);
                        format!(
                            "{:?} {:?} {:?} problem {problem}",
                            form.kind,
                            form.step(),
                            form.picks
                        )
                    }
                    None => "no survey".to_owned(),
                };
            }
            _ => {
                return "usage: feedback voice|stream|stream-viewer|report|gate voice|state"
                    .to_owned();
            }
        }
        "survey open".to_owned()
    }

    /// Escape closes the survey (and submits it).
    pub(super) fn feedback_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.feedback.form.is_none() {
            return false;
        }
        self.close_feedback(cx);
        true
    }

    /// The three faces (module 321563 without text labels): `ratingSelector_UNsDQI` 240 wide, the 64px faces spread
    /// `space-around`, grey (`grayscale(100%)`) unless picked or under the pointer, which also grows them by 6%.
    fn feedback_faces(&self, form: &Form, cx: &mut Context<Self>) -> AnyElement {
        let faces: Vec<AnyElement> = RATINGS
            .iter()
            .enumerate()
            .map(|(index, rating)| {
                let rating = *rating;
                let lit = form.picks.rating == Some(rating) || form.hovered_face == Some(index);
                let grow = form.hovered_face == Some(index) && !super::motion::reduced();
                let size = if grow { 64. * 1.06 } else { 64. };
                let picture = self.images.get(FACES[index]).cloned();
                div()
                    .id(SharedString::from(format!("feedback-face-{index}")))
                    .size(px(64.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        if let Some(form) = this.feedback.form.as_mut() {
                            if *inside {
                                form.hovered_face = Some(index);
                            } else if form.hovered_face == Some(index) {
                                form.hovered_face = None;
                            }
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let close = this
                            .feedback
                            .form
                            .as_mut()
                            .is_some_and(|form| form.picks.pick_rating(rating));
                        if close {
                            this.close_feedback(cx);
                        }
                        cx.notify();
                    }))
                    .children(picture.map(|picture| img(picture).size(px(size)).grayscale(!lit)))
                    .into_any_element()
            })
            .collect();
        // `problemInfo ratingSelectorContainer`: 8px above, 16px 24px inside.
        div()
            .mt(px(8.))
            .pt(px(16.))
            .pb(px(16.))
            .px(px(24.))
            .flex()
            .justify_center()
            .child(div().w(px(240.)).flex().justify_around().children(faces))
            .into_any_element()
    }

    /// A FormItem label (module 452027) over its control, 8px apart.
    fn feedback_labelled(label: String, control: AnyElement) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(!label.is_empty(), |this| {
                this.child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
            })
            .child(control)
    }

    /// The option list of module 124480 (`root_RJkt6C`): rows of 16px 12px with a 1px rule between, a chevron unless
    /// the row closes the survey (`hideCaret`).
    fn feedback_options(
        items: Vec<(String, bool)>,
        on_pick: impl Fn(&mut Root, usize, &mut Context<Root>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pick = std::rc::Rc::new(on_pick);
        let count = items.len();
        div()
            .bg(color("--background-surface-higher"))
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(8.))
            .overflow_hidden()
            .children(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, caret))| {
                        let pick = pick.clone();
                        div()
                            .id(SharedString::from(format!("feedback-option-{index}")))
                            .flex()
                            .items_center()
                            .justify_between()
                            .py(px(16.))
                            .px(px(12.))
                            .when(index + 1 < count, |this| {
                                this.border_b_1().border_color(color("--border-subtle"))
                            })
                            .cursor_pointer()
                            .text_color(color("--interactive-text-active"))
                            .hover(|style| {
                                style
                                    .bg(color("--background-mod-normal"))
                                    .border_color(gpui::transparent_black())
                            })
                            .child(
                                div()
                                    .pr(px(5.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .child(label),
                            )
                            .when(caret, |this| {
                                this.child(div().ml(px(8.)).flex_none().child(icon(
                                    "overlay_chevron_small_right",
                                    24.,
                                    "--interactive-text-default",
                                )))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                pick(this, index, cx);
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    /// "Describe your issue" (the text area of module 103557, up to 1024 characters) with the help request line.
    fn feedback_text_box(&self, form: &Form, cx: &mut Context<Self>) -> AnyElement {
        let length = form.text.read(cx).text().chars().count();
        if length > MAX_TEXT {
            form.text.update(cx, |editor, cx| {
                let kept: String = editor.text().chars().take(MAX_TEXT).collect();
                editor.set_text(&kept);
                cx.notify();
            });
        }
        let runs = dc_intl::format_runs("ybi2tD", &[("helpdeskURL", HELP_REQUEST_URL.into())]);
        let mut text = String::new();
        let mut link = 0..0;
        for (run, hook) in runs {
            let start = text.len();
            text.push_str(&run);
            if hook.is_some() {
                link = start..text.len();
            }
        }
        let styled = gpui::StyledText::new(text).with_highlights([(
            link.clone(),
            gpui::HighlightStyle {
                color: Some(color("--text-link").into()),
                ..Default::default()
            },
        )]);
        let open = cx.listener(|this, _: &usize, _, cx| {
            this.follow_link(HELP_REQUEST_URL, "", cx);
        });
        Self::feedback_labelled(
            dc_intl::t("h95hcn").to_owned(),
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                // GAP: module 103557's text area is drawn as the design system's text box.
                .child(
                    div()
                        .w_full()
                        .min_h(px(44.))
                        .px(px(10.))
                        .py(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .child(form.text.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-subtle"))
                        .child(
                            gpui::InteractiveText::new("feedback-help-request", styled)
                                .on_click(vec![link], move |index, window, cx| {
                                    open(&index, window, cx)
                                }),
                        ),
                )
                .into_any_element(),
        )
        .into_any_element()
    }

    pub(super) fn feedback_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.feedback.form.as_ref()?;
        let step = form.step();
        // 612479 (`FeedbackModal`): the report survey and the video background one.
        let v1 = matches!(form.kind, Kind::Report | Kind::VideoBackground);
        let (title, body) = match form.kind {
            Kind::Voice => ("Ss6tlb", "tLi4cR"),
            Kind::Stream { streamer: true } => ("5smP3R", "0uxA2V"),
            Kind::Stream { streamer: false } => ("5smP3R", "CqjnLN"),
            Kind::Report => ("MP5lDj", "7Ct0Dj"),
            Kind::VideoBackground => ("Wl8qXg", "vPw6j0"),
        };
        // `headerContainer`: the 24/32 title and the 16/20 body under it, centred, 16px 24px in and nothing under.
        let header = div()
            .pt(px(16.))
            .px(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .text_center()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t(title)),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t(body)),
            );
        let faces_shown = step == Step::Rating || (v1 && step == Step::Problem);
        // `content__KoJTP`: 24px at the sides, 8px above.
        let content = |inner: AnyElement, spacing: bool| {
            div()
                .px(px(24.))
                .pt(px(8.))
                .when(spacing, |this| this.pb(px(16.)))
                .child(inner)
                .into_any_element()
        };
        let middle: Option<AnyElement> = match step {
            Step::Rating => None,
            Step::Category => {
                let items = form
                    .tree
                    .iter()
                    .map(|category| (dc_intl::tr(category.label).to_owned(), false))
                    .collect();
                Some(content(
                    Self::feedback_labelled(
                        dc_intl::tr("What did you have trouble with?").to_owned(),
                        Self::feedback_options(
                            items,
                            |this, index, _| {
                                if let Some(form) = this.feedback.form.as_mut() {
                                    form.picks.category = Some(index);
                                }
                            },
                            cx,
                        ),
                    )
                    .into_any_element(),
                    false,
                ))
            }
            Step::Problem => {
                let category = form.picks.category.unwrap_or(0);
                let items = form.tree[category]
                    .problems
                    .iter()
                    .map(|problem| (dc_intl::tr(problem.label).to_owned(), problem.freeform))
                    .collect();
                Some(content(
                    Self::feedback_labelled(
                        // `problemTitle`: "Had an issue?" for the video background survey.
                        dc_intl::t(if form.kind == Kind::VideoBackground {
                            "UNFF85"
                        } else {
                            "FJmoxF"
                        })
                        .to_owned(),
                        Self::feedback_options(
                            items,
                            move |this, index, cx| {
                                let close = this.feedback.form.as_mut().is_some_and(|form| {
                                    let freeform = form.freeform(category, index);
                                    form.picks.pick_problem(index, freeform)
                                });
                                if close {
                                    this.close_feedback(cx);
                                }
                            },
                            cx,
                        ),
                    )
                    .into_any_element(),
                    v1,
                ))
            }
            Step::Freeform => Some(content(self.feedback_text_box(form, cx), false)),
        };
        // The footer (`footer__KoJTP footer_C-o8GY`): 16px 24px, `space-between`. The faces carry "Don't show again";
        // the later steps Back and, at the text box, Submit (612479 has no Back/Submit before its text box).
        let footer: Option<AnyElement> = if step == Step::Rating {
            let checked = form.dont_show;
            Some(
                div()
                    .id("feedback-dont-show")
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .cursor_pointer()
                    .child(super::server_games::checkbox(checked))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("5E9SB9")),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(form) = this.feedback.form.as_mut() {
                            form.dont_show = !form.dont_show;
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            )
        } else if !v1 || step == Step::Freeform {
            let back = super::modal::small_footer_button(
                super::modal::button(
                    "feedback-back",
                    dc_intl::t("13/7kX"),
                    super::modal::Tone::Secondary,
                    |this, _, cx| {
                        if let Some(form) = this.feedback.form.as_mut() {
                            let single = form.single();
                            if matches!(form.kind, Kind::Report | Kind::VideoBackground) {
                                // 612479: back from the text box to the problems, the text cleared.
                                form.picks.problem = None;
                            } else {
                                form.picks.back(single);
                            }
                            // Both clear the text on the way back from the box (`J("")`, `en("")`).
                            form.text.update(cx, |editor, cx| {
                                editor.set_text("");
                                cx.notify();
                            });
                        }
                    },
                ),
                cx,
            );
            let submit = (step == Step::Freeform).then(|| {
                super::modal::small_footer_button(
                    super::modal::button(
                        "feedback-submit",
                        dc_intl::t("geKm7t"),
                        super::modal::Tone::Primary,
                        |this, _, cx| this.close_feedback(cx),
                    ),
                    cx,
                )
            });
            Some(
                div()
                    .w_full()
                    .flex()
                    .justify_between()
                    .map(|this| {
                        if v1 {
                            // `horizontal-reverse`: Submit, then Back.
                            this.child(
                                div()
                                    .flex()
                                    .flex_row_reverse()
                                    .gap(px(8.))
                                    .child(back)
                                    .children(submit),
                            )
                        } else {
                            this.child(back).children(submit)
                        }
                    })
                    .into_any_element(),
            )
        } else {
            None
        };
        let pose =
            super::motion::legacy_modal_pose("feedback-modal", "feedback-modal-layer", false);
        let panel = div()
            .id("feedback-modal")
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .occlude()
            .w(px(444.))
            .min_h(px(220.))
            .max_h(px(720.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-normal"))
            .bg(color("--modal-background"))
            .shadow(vec![BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.16),
                offset: point(px(0.), px(4.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(header)
            .when(faces_shown, |this| {
                this.child(self.feedback_faces(form, cx))
            })
            .children(middle)
            .children(footer.map(|footer| {
                div()
                    .mt_auto()
                    .pt(px(16.))
                    .pb(px(16.))
                    .px(px(24.))
                    .rounded_b(px(12.))
                    .bg(color("--modal-footer-background"))
                    .child(footer)
            }));
        Some(
            deferred(
                div()
                    .id("feedback-modal-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(color("--background-scrim")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.close_feedback(cx)))
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(last: Option<u64>, opt_out: Option<u64>) -> InAppFeedbackState {
        InAppFeedbackState {
            last_impression_ms: last,
            opt_out_expiry_ms: opt_out,
        }
    }

    #[test]
    fn the_group_cooldown_and_the_opt_out_hold_a_survey_back() {
        let now = 10 * DAY_MS;
        assert!(may_show(
            Kind::Voice,
            |_| InAppFeedbackState::default(),
            now
        ));
        // A stream survey an hour ago holds the call one back: same group.
        let recent_stream = |kind: u64| {
            if kind == 1 {
                state(Some(now - 3_600_000), None)
            } else {
                InAppFeedbackState::default()
            }
        };
        assert!(!may_show(Kind::Voice, recent_stream, now));
        // ... but not the report one.
        assert!(may_show(Kind::Report, recent_stream, now));
        // A day and a bit later it may show.
        assert!(may_show(
            Kind::Voice,
            |_| state(Some(now - DAY_MS - 1), None),
            now
        ));
        assert!(!may_show(
            Kind::Voice,
            |kind| state(None, (kind == 0).then_some(OPT_OUT_FOREVER_MS)),
            now
        ));
        // An opt-out that ran out does not.
        assert!(may_show(
            Kind::Voice,
            |kind| state(None, (kind == 0).then_some(now - 1)),
            now
        ));
    }

    #[test]
    fn steps_follow_the_picks_and_every_close_submits_what_was_picked() {
        let freeform = |_: usize, problem: usize| problem == 9;
        let mut picks = Picks::default();
        assert_eq!(picks.step(freeform), Step::Rating);
        assert!(!picks.pick_rating(Rating::Bad));
        assert_eq!(picks.step(freeform), Step::Category);
        picks.category = Some(1);
        assert_eq!(picks.step(freeform), Step::Problem);
        // Other opens the text box; Back returns to the problems, then the categories, then the faces.
        assert!(!picks.pick_problem(9, true));
        assert_eq!(picks.step(freeform), Step::Freeform);
        picks.back(false);
        assert_eq!(picks.step(freeform), Step::Problem);
        picks.back(false);
        assert_eq!(picks.step(freeform), Step::Category);
        picks.back(false);
        assert_eq!(picks.step(freeform), Step::Rating);
        // A good face closes at once and submits nothing more.
        let mut good = Picks::default();
        assert!(good.pick_rating(Rating::Good));
        assert_eq!(after_submit(Kind::Voice, &good, 0.5), After::Nothing);
        // A problem closes at once: the thanks, or Upload Logs on the 1% roll for a bad or neutral face.
        let mut bad = Picks {
            category: Some(0),
            ..Picks::default()
        };
        bad.pick_rating(Rating::Neutral);
        assert!(bad.pick_problem(2, false));
        assert_eq!(after_submit(Kind::Voice, &bad, 0.5), After::Thanks);
        assert_eq!(after_submit(Kind::Voice, &bad, 0.005), After::UploadLogs);
        assert_eq!(after_submit(Kind::Report, &bad, 0.005), After::Thanks);
        // With one category (the stream survey) Back from the problems goes to the faces and keeps the category.
        let mut stream = Picks {
            rating: Some(Rating::Bad),
            category: Some(0),
            problem: None,
        };
        stream.back(true);
        assert_eq!(stream.rating, None);
        assert_eq!(stream.category, Some(0));
    }

    #[test]
    fn trees_end_with_other_and_hold_the_module_options() {
        let mut rng = rand::rng();
        let voice = categories(Kind::Voice, &mut rng);
        assert_eq!(voice.len(), 4);
        for category in &voice {
            assert!(category.problems.last().unwrap().freeform);
            assert_eq!(category.problems.iter().filter(|p| p.freeform).count(), 1);
        }
        let audio = voice
            .iter()
            .find(|category| category.label == "Microphone or audio")
            .unwrap();
        assert_eq!(audio.problems.len(), 13);
        let streamer = categories(Kind::Stream { streamer: true }, &mut rng);
        assert_eq!(streamer[0].problems.len(), 11);
        let viewer = categories(Kind::Stream { streamer: false }, &mut rng);
        assert_eq!(viewer[0].problems.len(), 10);
        let report = categories(Kind::Report, &mut rng);
        assert_eq!(report[0].problems.len(), 3);
        assert!(report[0].problems.iter().all(|p| !p.value.is_empty()));
    }
}
