//! The tutorial tips of a new account: the store of module 179913 (`TutorialIndicatorStore`), the pulsing indicator of
//! module 600160 (`x`, CSS `indicator_hqoWiU`), the coachmark of module 244083 (title, body, "Got it!" and "Skip all
//! tips", CSS `popoutRoot_KWWX2H`) and the popover of module 43105 that three tips open instead (graphic, close
//! button, "Got it!" only, CSS `popover_81IzEA`).
//!
//! The store is seeded by READY's `tutorial` object (`indicators_suppressed`, `indicators_confirmed`) and written with
//! `PUT /tutorial/indicators/<id>` ("Got it!", module 787541 `X8`) and `POST /tutorial/indicators/suppress` ("Skip
//! all tips", `n7`). A READY without the object leaves every tip suppressed. Most accounts are suppressed.
//!
//! Where a tip is mounted (module 728321 `A`): `organize-by-topic` on the first named category and
//! `voice-conversations` on the first voice channel of the channel list (module 616740), `create-first-server` /
//! `create-more-servers` on the rail's Add a Server button (module 396655). `instant-invite` is mounted only on a
//! channel row with `isDefaultChannel`, which nothing in the pinned bundle sets, so it never shows; `whos-online`,
//! `server-settings`, `friends-list` and `create-more-servers` need it (or a tip after it) confirmed by the server.
//! There is no channel-drag tip in module 616740.

use super::{Root, color, icon, modal, motion};
use crate::net::Request;
use dc_state::{Channel, ChannelKind, Id};
use gpui::{
    Anchor, AnyElement, Context, FontWeight, IntoElement, Radians, SharedString, Transformation,
    anchored, deferred, div, img, point, prelude::*, px, relative, svg,
};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::f32::consts::{FRAC_PI_2, PI};

/// The side a tip's indicator sits on, or its popout opens to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// Which popout the tip's indicator opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Popout {
    /// Module 244083: title, body, "Got it!", "Skip all tips".
    Coachmark,
    /// Module 43105 (the `O` table of module 600160): graphic, title, body, "Got it!".
    Popover(&'static str),
}

/// One tip: its prerequisites (module 179913 `a`) and how its popout opens (module 600160 `_`).
#[derive(Clone, Copy, Debug)]
pub(super) struct Definition {
    pub id: &'static str,
    pub prerequisites: &'static [&'static str],
    pub popout_position: Side,
    /// `spacing` of module 244083 (`u ?? 0`).
    pub spacing: f32,
    /// `highPriority`: the exclaim and dot are drawn, and the circles pulse brighter.
    pub high_priority: bool,
    pub popout: Popout,
}

const CDN: &str = "https://cdn.discordapp.com/assets/content/";

/// The tips in the order of module 179913. `popoutOffset` of that table is never read, so it is not kept.
pub(super) const DEFINITIONS: &[Definition] = &[
    Definition {
        id: "voice-conversations",
        prerequisites: &[],
        popout_position: Side::Right,
        spacing: 0.,
        high_priority: true,
        popout: Popout::Popover(
            "2fbf80a634ec810152549bc64cc498329014001bf81341a6fd08ca73f0370316.svg",
        ),
    },
    Definition {
        id: "writing-messages",
        prerequisites: &["voice-conversations"],
        popout_position: Side::Top,
        spacing: 0.,
        high_priority: true,
        popout: Popout::Popover(
            "bdb4aa801d39492170cf3db05e482b46af127a4a87ba006b661eb376daeaee04.svg",
        ),
    },
    Definition {
        id: "direct-messages",
        prerequisites: &[],
        popout_position: Side::Right,
        spacing: 0.,
        high_priority: false,
        popout: Popout::Coachmark,
    },
    Definition {
        id: "create-first-server",
        prerequisites: &[],
        popout_position: Side::Right,
        spacing: 0.,
        high_priority: true,
        popout: Popout::Coachmark,
    },
    Definition {
        id: "organize-by-topic",
        prerequisites: &[],
        popout_position: Side::Right,
        spacing: 0.,
        high_priority: false,
        popout: Popout::Popover(
            "5d2fe3296aafdf78f5a3c44a803c714f1c90e3b0d6d1c7290aa5fdf2e79831e3.svg",
        ),
    },
    Definition {
        id: "instant-invite",
        prerequisites: &["organize-by-topic"],
        popout_position: Side::Right,
        spacing: 24.,
        high_priority: true,
        popout: Popout::Coachmark,
    },
    Definition {
        id: "whos-online",
        prerequisites: &["instant-invite"],
        popout_position: Side::Left,
        spacing: 16.,
        high_priority: false,
        popout: Popout::Coachmark,
    },
    Definition {
        id: "server-settings",
        prerequisites: &["instant-invite"],
        popout_position: Side::Bottom,
        spacing: 10.,
        high_priority: false,
        popout: Popout::Coachmark,
    },
    Definition {
        id: "friends-list",
        prerequisites: &["instant-invite"],
        popout_position: Side::Right,
        spacing: 8.,
        high_priority: false,
        popout: Popout::Coachmark,
    },
    Definition {
        id: "create-more-servers",
        prerequisites: &["server-settings"],
        popout_position: Side::Right,
        spacing: 8.,
        high_priority: false,
        popout: Popout::Coachmark,
    },
];

pub(super) fn definition(id: &str) -> Option<&'static Definition> {
    DEFINITIONS.iter().find(|definition| definition.id == id)
}

/// Where a mounted tip's indicator goes (the render data of `TUTORIAL_INDICATOR_SHOW`, module 728321 `show`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Place {
    /// The wrapper's `position` (not the popout's).
    pub position: Side,
    /// `inlineSpecs`: the origin, relative to the anchor's box, and the target's size. Without it the anchor's own
    /// box is the target.
    pub inline: Option<((f32, f32), (f32, f32))>,
    /// `offsetX` / `offsetY`.
    pub offset: (f32, f32),
}

/// `animationContainer_hqoWiU`: the indicator is 36 by 36.
pub(super) const INDICATOR: f32 = 36.;

/// `updateCache` of module 600160: the indicator's top left, from the anchor's box `(x, y, width, height)`.
pub(super) fn indicator_origin(anchor: (f32, f32, f32, f32), place: Place) -> (f32, f32) {
    let (origin, (width, height)) = match place.inline {
        Some(((dx, dy), size)) => ((anchor.0 + dx, anchor.1 + dy), size),
        None => ((anchor.0, anchor.1), (anchor.2, anchor.3)),
    };
    let (mut x, mut y) = ((width - INDICATOR) / 2., (height - INDICATOR) / 2.);
    match place.position {
        Side::Left => x = -INDICATOR,
        Side::Right => x = width,
        Side::Bottom => y = height,
        Side::Top => y = -INDICATOR,
    }
    (origin.0 + x + place.offset.0, origin.1 + y + place.offset.1)
}

/// Module 265872 with `align` from module 244083 (`center` for top and bottom, else `top`): the popout's corner that
/// is pinned, and where, from the indicator's top left.
pub(super) fn coachmark_corner(
    indicator: (f32, f32),
    side: Side,
    spacing: f32,
) -> (Anchor, (f32, f32)) {
    let (x, y) = indicator;
    match side {
        Side::Right => (Anchor::TopLeft, (x + INDICATOR + spacing, y)),
        Side::Left => (Anchor::TopRight, (x - spacing, y)),
        Side::Bottom => (
            Anchor::TopCenter,
            (x + INDICATOR / 2., y + INDICATOR + spacing),
        ),
        Side::Top => (Anchor::BottomCenter, (x + INDICATOR / 2., y - spacing)),
    }
}

/// GAP: the gap between the indicator and the module 43105 popover (`BasePopover` of module 195118) is not measured.
const POPOVER_SPACING: f32 = 8.;

/// Module 43105 (`alignmentStrategy: "trigger-center"`): the popover is centred on the indicator.
pub(super) fn popover_corner(indicator: (f32, f32), side: Side) -> (Anchor, (f32, f32)) {
    let (x, y) = indicator;
    let half = INDICATOR / 2.;
    match side {
        Side::Right => (
            Anchor::LeftCenter,
            (x + INDICATOR + POPOVER_SPACING, y + half),
        ),
        Side::Left => (Anchor::RightCenter, (x - POPOVER_SPACING, y + half)),
        Side::Bottom => (
            Anchor::TopCenter,
            (x + half, y + INDICATOR + POPOVER_SPACING),
        ),
        Side::Top => (Anchor::BottomCenter, (x + half, y - POPOVER_SPACING)),
    }
}

/// `TutorialIndicatorStore` (module 179913) and the open popout.
pub(super) struct TutorialTips {
    /// `h`: the gateway is open.
    connected: bool,
    /// `d`: every indicator is suppressed.
    suppressed: bool,
    /// `o[id] === false`: the tips the server has as confirmed, or dismissed in this session.
    confirmed: HashSet<&'static str>,
    /// `c`: unconfirmed and every prerequisite confirmed.
    visible: HashSet<&'static str>,
    /// The tip whose popout is open (the popout's own state in modules 244083 and 600160 `E`).
    pub open: Option<&'static str>,
    /// The open server's first named category and first voice channel, found when its channel list is built.
    firsts: Cell<(Option<Id>, Option<Id>)>,
    /// The popout buttons' focus handles, by button id.
    focus: RefCell<Vec<(&'static str, gpui::FocusHandle)>>,
}

impl Default for TutorialTips {
    fn default() -> Self {
        let mut tips = Self {
            connected: false,
            suppressed: true,
            confirmed: HashSet::new(),
            visible: HashSet::new(),
            open: None,
            firsts: Cell::new((None, None)),
            focus: RefCell::new(Vec::new()),
        };
        tips.recompute();
        tips
    }
}

impl TutorialTips {
    /// `f` of module 179913.
    fn recompute(&mut self) {
        self.visible.clear();
        if self.suppressed {
            return;
        }
        for definition in DEFINITIONS {
            let shown = !self.confirmed.contains(definition.id)
                && definition
                    .prerequisites
                    .iter()
                    .all(|before| self.confirmed.contains(before));
            if shown {
                self.visible.insert(definition.id);
            }
        }
    }

    /// `CONNECTION_OPEN`: READY's `tutorial` (null when the account has none).
    pub(super) fn connection_open(&mut self, tutorial: &Value) {
        self.connected = true;
        self.suppressed = true;
        self.confirmed.clear();
        if !tutorial.is_null() {
            // `d = t.indicators_suppressed`: a missing field reads as not suppressed.
            self.suppressed = tutorial["indicators_suppressed"].as_bool().unwrap_or(false);
            for id in tutorial["indicators_confirmed"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if let Some(definition) = definition(id) {
                    self.confirmed.insert(definition.id);
                }
            }
        }
        self.open = None;
        self.recompute();
    }

    /// `CONNECTION_CLOSED`.
    pub(super) fn connection_closed(&mut self) {
        self.connected = false;
    }

    pub(super) fn dispatch(&mut self, name: &str, data: &Value) {
        if name == "READY" {
            self.connection_open(&data["tutorial"]);
        }
    }

    /// `shouldShow` (`i.Fr`, a mobile browser, is never true here).
    pub(super) fn should_show(&self, id: &str) -> bool {
        self.connected && !self.suppressed && self.visible.contains(id)
    }

    /// `shouldShowAnyIndicators`.
    pub(super) fn should_show_any(&self) -> bool {
        !self.suppressed
    }

    /// `TUTORIAL_INDICATOR_DISMISS`: the route of its `PUT`.
    pub(super) fn dismiss(&mut self, id: &'static str) -> String {
        self.confirmed.insert(id);
        if self.open == Some(id) {
            self.open = None;
        }
        self.recompute();
        format!("/tutorial/indicators/{id}")
    }

    /// `TUTORIAL_INDICATOR_SUPPRESS_ALL`: the route of its `POST`.
    pub(super) fn suppress_all(&mut self) -> &'static str {
        self.suppressed = true;
        self.open = None;
        "/tutorial/indicators/suppress"
    }
}

/// The keyframes of one property: (percent, value).
type Stops = &'static [(f64, f64)];

/// `inner-circle-loop_hqoWiU` (opacity, then scale) and its high priority variant.
const INNER_OPACITY: Stops = &[(0., 0.1), (42., 0.25), (100., 0.1)];
const INNER_OPACITY_HIGH: Stops = &[(0., 0.1), (42., 0.5), (100., 0.1)];
const INNER_SCALE: Stops = &[(0., 1.1), (42., 0.9), (100., 1.1)];
/// `outer-circle-loop_hqoWiU` and its high priority variant.
const OUTER_OPACITY: Stops = &[(0., 0.), (51., 0.2), (100., 0.)];
const OUTER_OPACITY_HIGH: Stops = &[(0., 0.), (51., 0.4), (100., 0.)];
const OUTER_SCALE: Stops = &[(0., 1.1), (51., 0.9), (100., 1.1)];
/// `exclaim-loop_hqoWiU` and `dot-loop_hqoWiU`: `translateY` in px.
const EXCLAIM_Y: Stops = &[
    (0., 0.),
    (21., -4.),
    (46., 0.),
    (51., 0.),
    (72., -4.),
    (100., 0.),
];
const DOT_Y: Stops = &[
    (0., 0.),
    (19., -5.),
    (44., 0.),
    (49., 0.),
    (70., -5.),
    (95., 0.),
    (100., 0.),
];
/// Every loop runs 1.5s.
const LOOP_MS: f64 = 1500.;

/// One property at `percent`, each keyframe segment under `ease`.
fn along(stops: Stops, percent: f64, ease: motion::Ease) -> f64 {
    for pair in stops.windows(2) {
        let ((from_at, from), (to_at, to)) = (pair[0], pair[1]);
        if percent <= to_at {
            let span = to_at - from_at;
            let x = if span <= 0. {
                1.
            } else {
                (percent - from_at) / span
            };
            return from + (to - from) * ease.apply(x);
        }
    }
    stops.last().map_or(0., |last| last.1)
}

/// The indicator's loops at `now_ms`: (inner opacity, inner scale, outer opacity, outer scale, exclaim y, dot y).
/// Without full motion the circles rest at opacity .1 and nothing moves.
pub(super) fn pulse(now_ms: f64, high: bool, full_motion: bool) -> [f32; 6] {
    if !full_motion {
        return [0.1, 1., 0.1, 1., 0., 0.];
    }
    let percent = now_ms.rem_euclid(LOOP_MS) / LOOP_MS * 100.;
    // The circles take the default timing function (`ease`); the exclaim and dot are `ease-in-out`.
    let circle = motion::Ease::Ease;
    let bounce = motion::Ease::EaseInOut;
    [
        along(
            if high {
                INNER_OPACITY_HIGH
            } else {
                INNER_OPACITY
            },
            percent,
            circle,
        ) as f32,
        along(INNER_SCALE, percent, circle) as f32,
        along(
            if high {
                OUTER_OPACITY_HIGH
            } else {
                OUTER_OPACITY
            },
            percent,
            circle,
        ) as f32,
        along(OUTER_SCALE, percent, circle) as f32,
        along(EXCLAIM_Y, percent, bounce) as f32,
        along(DOT_Y, percent, bounce) as f32,
    ]
}

/// The coachmark's title and body (`getTutorialPopoutText` of module 600160).
pub(super) fn coachmark_text(id: &str) -> Option<(&'static str, String)> {
    let (title, body) = match id {
        "whos-online" => ("l2nSoI", "y6tbBB"),
        // GAP: the `GroupPlusIcon` of `shareIconHook` is not drawn inside the sentence; its label stands in.
        "instant-invite" => {
            return Some((
                dc_intl::t("XcehY3"),
                dc_intl::format(
                    "ymkBti",
                    &[("shareIcon", dc_intl::t("VINpSK").to_owned().into())],
                ),
            ));
        }
        "server-settings" => ("wc8wi5", "mIfDhc"),
        "direct-messages" => ("V07x5y", "GEBBey"),
        "create-more-servers" => ("50FORo", "zVL4qa"),
        "create-first-server" => ("CtOX9T", "TflqTe"),
        "friends-list" => ("Fnk9L4", "NTq0AZ"),
        _ => return None,
    };
    Some((dc_intl::t(title), dc_intl::format(body, &[])))
}

/// The popover's title and body (the `O` table of module 600160).
pub(super) fn popover_text(id: &str, server_name: &str) -> Option<(&'static str, String)> {
    match id {
        "voice-conversations" => Some((dc_intl::t("w5HAll"), dc_intl::t("Ww4hhq").to_owned())),
        "writing-messages" => Some((
            dc_intl::t("nkP0ly"),
            dc_intl::format("oQuG3K", &[("serverName", server_name.to_owned().into())]),
        )),
        "organize-by-topic" => Some((dc_intl::t("j/5+IX"), dc_intl::t("mufH2P").to_owned())),
        _ => None,
    }
}

/// `nf` of module 616740: the category's tip, inline.
const CATEGORY_PLACE: Place = Place {
    position: Side::Right,
    inline: Some(((-36., 7.), (232., 40.))),
    offset: (0., 0.),
};
/// The first voice channel's tip (`position: "right"`, `offsetX: -20`).
const VOICE_PLACE: Place = Place {
    position: Side::Right,
    inline: None,
    offset: (-20., 0.),
};
/// `tY` of module 396655, inline in `tutorialContainer_rwEpIL` (`position: relative`, as wide as the 72px
/// `listItem_rwEpIL`: the 40px button and 16px on each side). The indicator is drawn inside the button's own box
/// here, so the origin is 16px further left.
const ADD_SERVER_PLACE: Place = Place {
    position: Side::Right,
    inline: Some(((-32., 0.), (48., 48.))),
    offset: (0., 0.),
};

/// The tips of the channel list (their popout only opens while a server's channel list is the sidebar).
const CHANNEL_LIST_TIPS: [&str; 2] = ["organize-by-topic", "voice-conversations"];
/// Every tip with a target here (the rail button takes one of its two, by whether the account owns a server).
#[cfg(feature = "capture")]
const MOUNTED: [&str; 4] = [
    "organize-by-topic",
    "voice-conversations",
    "create-first-server",
    "create-more-servers",
];

/// Where the indicator sits inside its target's box (`left` or, from the far edge, `right`; then `top`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Inset {
    Left(f32, f32),
    Right(f32, f32),
}

/// The indicator inside a target of `height` whose width is not known while it is built: an inline tip from its
/// specs, a tip at the target's right from the far edge.
pub(super) fn inset(place: Place, height: f32) -> Inset {
    if place.inline.is_some() {
        let (x, y) = indicator_origin((0., 0., 0., 0.), place);
        return Inset::Left(x, y);
    }
    let y = (height - INDICATOR) / 2. + place.offset.1;
    match place.position {
        // `offsetX = targetWidth`, then the offset: its right edge is `INDICATOR + offset` past the target's.
        Side::Right => Inset::Right(-(INDICATOR + place.offset.0), y),
        Side::Left => Inset::Left(-INDICATOR + place.offset.0, y),
        // GAP: no tip mounted here opens above or below a target of unknown width.
        Side::Top => Inset::Left(place.offset.0, -INDICATOR + place.offset.1),
        Side::Bottom => Inset::Left(place.offset.0, height + place.offset.1),
    }
}

impl Root {
    /// The channel list is built again: its first named category (`getNamedCategoryFromSection(5)`) and first voice
    /// channel (`getFirstVoiceChannel`), found once for its rows.
    pub(super) fn tutorial_sidebar_begin(&self) {
        let tips = &self.tutorial_tips;
        let wanted = CHANNEL_LIST_TIPS.iter().any(|id| tips.should_show(id));
        let firsts = match self.current_guild() {
            Some(guild) if wanted => {
                let rows = guild.sidebar_rows();
                let first =
                    |kind: ChannelKind| rows.iter().find(|row| row.kind == kind).map(|row| row.id);
                (first(ChannelKind::Category), first(ChannelKind::Voice))
            }
            _ => (None, None),
        };
        tips.firsts.set(firsts);
    }

    /// The channel list's tip on `channel` (module 616740): `organize-by-topic` on the first named category,
    /// `voice-conversations` on the first voice channel. Added to the row's own (relative) box.
    pub(super) fn tutorial_channel_tip(
        &self,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tips = &self.tutorial_tips;
        if !tips.should_show_any() {
            return None;
        }
        let (first_category, first_voice) = tips.firsts.get();
        let (id, first, place, height) = match channel.kind {
            ChannelKind::Category => (
                "organize-by-topic",
                first_category,
                CATEGORY_PLACE,
                self.category_space_before() + 24.,
            ),
            // GAP: the list clips the 16px the indicator reaches past the row's right edge.
            ChannelKind::Voice => (
                "voice-conversations",
                first_voice,
                VOICE_PLACE,
                if self.voice_row_has_line(channel) {
                    50.
                } else {
                    34. + self.name_line_extra()
                },
            ),
            _ => return None,
        };
        if first != Some(channel.id) || !tips.should_show(id) {
            return None;
        }
        Some(self.tutorial_indicator(definition(id)?, inset(place, height), cx))
    }

    /// `create-first-server`, or `create-more-servers` once the account owns a server (module 396655 `tX`), on the
    /// rail's Add a Server button (added to its 40px tooltip box).
    pub(super) fn tutorial_rail_tip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tips = &self.tutorial_tips;
        if !tips.should_show_any() {
            return None;
        }
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let owns = me.is_some() && self.stores.guilds.iter().any(|guild| guild.owner_id == me);
        let id = if owns {
            "create-more-servers"
        } else {
            "create-first-server"
        };
        if !tips.should_show(id) {
            return None;
        }
        Some(self.tutorial_indicator(definition(id)?, inset(ADD_SERVER_PLACE, 40.), cx))
    }

    /// "Got it!": the tip is confirmed (module 787541 `X8`).
    pub(super) fn tutorial_dismiss(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let route = self.tutorial_tips.dismiss(id);
        if !crate::storage::settings_writes_blocked() {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PUT,
                route,
                body: None,
            });
        }
        // GAP: `CLOSE_TUTORIAL` (acknowledged) is not tracked; the app sends no analytics.
        cx.notify();
    }

    /// "Skip all tips": every indicator is suppressed (module 787541 `n7`).
    pub(super) fn tutorial_skip_all(&mut self, cx: &mut Context<Self>) {
        let route = self.tutorial_tips.suppress_all();
        if !crate::storage::settings_writes_blocked() {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::POST,
                route: route.to_owned(),
                body: None,
            });
        }
        // GAP: `DISMISS_ALL_TUTORIALS` is not tracked.
        cx.notify();
    }

    /// The gateway opened or closed (`CONNECTION_CLOSED` of the store).
    pub(super) fn tutorial_connection(&mut self, connected: bool) {
        if !connected {
            self.tutorial_tips.connection_closed();
        }
    }

    /// Capture only: `tutorial reset [confirmed ids...]` makes the account an unsuppressed one with those tips
    /// confirmed (READY's `tutorial`), `tutorial open <id>` opens a tip's popout, `tutorial close` closes it.
    #[cfg(feature = "capture")]
    pub(crate) fn script_tutorial(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        let line = match args {
            ["reset", confirmed @ ..] => {
                self.tutorial_tips.connection_open(&serde_json::json!({
                    "indicators_suppressed": false,
                    "indicators_confirmed": confirmed,
                }));
                let mut shown: Vec<&str> = DEFINITIONS
                    .iter()
                    .map(|definition| definition.id)
                    .filter(|id| self.tutorial_tips.should_show(id))
                    .collect();
                shown.sort_unstable();
                format!("reset, showing {}", shown.join(" "))
            }
            ["open", id] if !MOUNTED.contains(id) => format!("{id} not mounted"),
            ["open", id] => match definition(id) {
                Some(definition) if self.tutorial_tips.should_show(definition.id) => {
                    self.tutorial_tips.open = Some(definition.id);
                    format!("open {id}")
                }
                _ => format!("{id} not shown"),
            },
            ["close"] => {
                self.tutorial_tips.open = None;
                "closed".to_owned()
            }
            _ => "usage: tutorial reset [ids] | open <id> | close".to_owned(),
        };
        cx.notify();
        line
    }

    /// The open tip's popout, beside its indicator (the indicators themselves are drawn in their targets, as module
    /// 728321 renders `inlineSpecs` tips). Nothing while a layer (a dialog or Settings) is open (`hasLayers` of
    /// component `P` in module 600160).
    pub(super) fn tutorial_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // GAP: `hasLayers` is approximated by Settings and the Quick Switcher.
        if self.settings.is_some() || self.quick_switcher.is_some() {
            return None;
        }
        let id = self.tutorial_tips.open?;
        // The channel list is the sidebar only while a server is open (Home and Favorites draw their own).
        if !self.tutorial_tips.should_show(id)
            || (CHANNEL_LIST_TIPS.contains(&id) && (self.guild.is_none() || self.in_favorites()))
        {
            return None;
        }
        let definition = definition(id)?;
        // The indicator's box as the last frame painted it.
        let bounds = self
            .tip_anchors
            .borrow()
            .get(format!("tutorial-{id}").as_str())
            .copied()?;
        let at = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        Some(self.tutorial_popout(definition, at, cx))
    }

    /// `indicator_hqoWiU` at `inset` inside its target: the clickable 36px box with its circles (and, for a high
    /// priority tip, the exclaim and dot). It shows while the window has focus and its popout is closed, else it fades
    /// out over .2s. Its box is recorded for the popout (`tutorial-<id>`).
    fn tutorial_indicator(
        &self,
        definition: &'static Definition,
        inset: Inset,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = definition.id;
        let open = self.tutorial_tips.open == Some(id);
        let animating = motion::window_active() && !open;
        let opacity = motion::value(
            motion::key(&("tutorial-indicator", id)),
            if animating { 1. } else { 0. },
            motion::css(200., motion::Ease::EaseOut),
        );
        let full_motion = !motion::reduced();
        // No art is drawn yet (see below), so nothing asks for a frame per tick.
        let [
            inner_opacity,
            inner_scale,
            outer_opacity,
            outer_scale,
            exclaim_y,
            dot_y,
        ] = pulse(motion::now(), definition.high_priority, full_motion);
        // GAP: the four SVGs of `indicator_hqoWiU` (c200a7504d42173d, 5c37dde2c8764c10, dabf5fea0bb87bf8,
        // fd5f7ebed980d3e1) are not in the snapshot, so the indicator keeps its box, anchor and click but draws no
        // art rather than a guessed one; the pulse values stay computed for when the art is added.
        let _ = (
            inner_opacity,
            inner_scale,
            outer_opacity,
            outer_scale,
            exclaim_y,
            dot_y,
        );
        let placed = match inset {
            Inset::Left(x, y) => div().absolute().left(px(x)).top(px(y)),
            Inset::Right(x, y) => div().absolute().right(px(x)).top(px(y)),
        };
        placed
            .id(SharedString::from(format!("tutorial-indicator-{id}")))
            .occlude()
            .cursor_pointer()
            .size(px(INDICATOR))
            .opacity(opacity)
            .child(self.anchor_probe(format!("tutorial-{id}")))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                // The popout's target toggles it (`onClick` of module 600160 `E`, the Popout of 265872); the row or
                // button under it does not take the click.
                cx.stop_propagation();
                this.tutorial_tips.open = if this.tutorial_tips.open == Some(id) {
                    None
                } else {
                    Some(id)
                };
                cx.notify();
            }))
            .into_any_element()
    }

    /// The open tip's popout, beside its indicator at `at`.
    fn tutorial_popout(
        &self,
        definition: &'static Definition,
        at: (f32, f32),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = definition.id;
        let (corner, (x, y), body) = match definition.popout {
            Popout::Coachmark => {
                let (corner, place) =
                    coachmark_corner(at, definition.popout_position, definition.spacing);
                (corner, place, self.tutorial_coachmark(definition, cx))
            }
            Popout::Popover(graphic) => {
                let (corner, place) = popover_corner(at, definition.popout_position);
                (
                    corner,
                    place,
                    self.tutorial_popover(definition, graphic, cx),
                )
            }
        };
        // A press outside the popout and its indicator closes it (`onRequestClose`).
        let indicator = gpui::Bounds::new(
            point(px(at.0), px(at.1)),
            gpui::size(px(INDICATOR), px(INDICATOR)),
        );
        let frame = div()
            .on_mouse_down_out(
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    if this.tutorial_tips.open == Some(id) && !indicator.contains(&event.position) {
                        this.tutorial_tips.open = None;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(body);
        // GAP: the Popout's translate-in animation (module 44808) is not played.
        deferred(
            anchored()
                .anchor(corner)
                .position(point(px(x), px(y)))
                .snap_to_window_with_margin(px(8.))
                .child(frame),
        )
        .with_priority(1)
        .into_any_element()
    }

    /// A full-width design system button that does not take a dialog's focus.
    fn tutorial_button(
        &self,
        id: &'static str,
        label: &'static str,
        tone: modal::Tone,
        small: bool,
        run: impl Fn(&mut Root, &mut gpui::Window, &mut Context<Root>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut button = modal::button(id, label, tone, run);
        // Each button keeps one handle across frames, so Tab and Enter reach it.
        let focus = {
            let mut handles = self.tutorial_tips.focus.borrow_mut();
            match handles.iter().find(|(known, _)| *known == id) {
                Some((_, handle)) => handle.clone(),
                None => {
                    let handle = cx.focus_handle();
                    handles.push((id, handle.clone()));
                    handle
                }
            }
        };
        button.focus = Some(focus);
        if small {
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(modal::small_footer_button(button, cx))
                .into_any_element()
        } else {
            div()
                .w_full()
                .flex()
                .child(modal::footer_button(button, cx))
                .into_any_element()
        }
    }

    /// Module 244083 (`contentNarrowNoMedia_KWWX2H`, left aligned): 280 wide, 16 padding, radius 8,
    /// `--background-surface-highest` with `--shadow-high`; the title `body-md/semibold` in `--text-strong` with 4
    /// below, the body 14/18 medium in `--text-default` with 16 below, then "Got it!" and "Skip all tips" 8 apart.
    fn tutorial_coachmark(
        &self,
        definition: &'static Definition,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = definition.id;
        let (title, body) = coachmark_text(id).unwrap_or_default();
        // `popoutRoot_KWWX2H:before`: an 8px border triangle in the surface colour, 10px from the top beside a left or
        // right popout, centred under or over a top or bottom one. GAP: the 16x10 tooltip caret stands in for it.
        let (turn, arrow) = match definition.popout_position {
            Side::Right => (FRAC_PI_2, div().absolute().left(px(-13.)).top(px(13.))),
            Side::Left => (-FRAC_PI_2, div().absolute().right(px(-13.)).top(px(13.))),
            Side::Bottom => (PI, div().absolute().left(px(132.)).top(px(-10.))),
            Side::Top => (0., div().absolute().left(px(132.)).bottom(px(-10.))),
        };
        let arrow = arrow.w(px(16.)).h(px(10.)).child(
            svg()
                .absolute()
                .w(px(16.))
                .h(px(10.))
                .path("icons/tooltip_caret_fill.svg")
                .text_color(color("--background-surface-highest"))
                .with_transformation(Transformation::rotate(Radians(turn))),
        );
        div()
            .id(SharedString::from(format!("tutorial-coachmark-{id}")))
            .occlude()
            .relative()
            .w(px(280.))
            .p(px(16.))
            .rounded(px(8.))
            .bg(color("--background-surface-highest"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(arrow)
            .child(
                div()
                    .mb(px(4.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .child(
                div()
                    .mb(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(body),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.tutorial_button(
                        "tutorial-got-it",
                        dc_intl::t("+IrDzN"),
                        modal::Tone::Primary,
                        false,
                        move |this, _, cx| this.tutorial_dismiss(id, cx),
                        cx,
                    ))
                    .child(self.tutorial_button(
                        "tutorial-skip-all",
                        dc_intl::t("33wtxt"),
                        modal::Tone::Secondary,
                        false,
                        |this, _, cx| this.tutorial_skip_all(cx),
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// Module 43105 (`popover_81IzEA`): 240 wide (`--custom-popover-width` of module 195118), 16 padding, radius 12,
    /// `--background-surface-high` inside a 1px `--border-subtle` with `--shadow-high`, centred text; the close button
    /// 4px in from the top end, the graphic (at most 144 wide, 16:9, 16 above and 20 below), the title
    /// `heading-sm/semibold` and the body `body-sm/normal` in `--text-subtle` (4 apart, 20 above and below), and
    /// "Got it!" (`sm`, full width).
    fn tutorial_popover(
        &self,
        definition: &'static Definition,
        graphic: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = definition.id;
        let server = self
            .current_guild()
            .map(|guild| guild.name.clone())
            .unwrap_or_default();
        let (title, body) = popover_text(id, &server).unwrap_or_default();
        let url = format!("{CDN}{graphic}");
        let art = self.images.get(&url).cloned();
        if art.is_none() {
            self.want_image(url.clone(), url, cx);
        }
        // GAP: the 22x14 caret of module 31188 is drawn with the 16x10 tooltip caret, at the popover's middle edge.
        let (turn, caret) = match definition.popout_position {
            Side::Right => (
                FRAC_PI_2,
                div()
                    .absolute()
                    .left(px(-13.))
                    .top(relative(0.5))
                    .mt(px(-5.)),
            ),
            Side::Left => (
                -FRAC_PI_2,
                div()
                    .absolute()
                    .right(px(-13.))
                    .top(relative(0.5))
                    .mt(px(-5.)),
            ),
            Side::Bottom => (PI, div().absolute().left(px(112.)).top(px(-10.))),
            Side::Top => (0., div().absolute().left(px(112.)).bottom(px(-10.))),
        };
        let layer = |path: &'static str, tint: &str| {
            svg()
                .absolute()
                .w(px(16.))
                .h(px(10.))
                .path(path)
                .text_color(color(tint))
                .with_transformation(Transformation::rotate(Radians(turn)))
        };
        let caret = caret
            .w(px(16.))
            .h(px(10.))
            .child(layer(
                "icons/tooltip_caret_fill.svg",
                "--background-surface-high",
            ))
            .child(layer("icons/tooltip_caret_stroke.svg", "--border-subtle"));
        div()
            .id(SharedString::from(format!("tutorial-popover-{id}")))
            .occlude()
            .relative()
            .w(px(240.))
            .p(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            // The content is one block inside the popover's flex box, so its margins collapse: 16 over the graphic,
            // 20 between the graphic, the header and the button.
            .flex()
            .flex_col()
            .text_center()
            .child(caret)
            // `closeButton_81IzEA`. GAP: the `sm` close button's size is not measured; a 24px box holds a 16px X.
            .child(
                div()
                    .id(SharedString::from(format!("tutorial-popover-close-{id}")))
                    .absolute()
                    .top(px(4.))
                    .right(px(4.))
                    .size(px(24.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .child(icon("x_small", 16., "--icon-subtle"))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        if this.tutorial_tips.open == Some(id) {
                            this.tutorial_tips.open = None;
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .mt(px(16.))
                    .mb(px(20.))
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w(px(144.))
                            .h(px(81.))
                            .children(art.map(|art| img(art).w(px(144.)).h(px(81.)))),
                    ),
            )
            .child(
                div()
                    .mb(px(20.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(body),
                    ),
            )
            .child(self.tutorial_button(
                "tutorial-popover-got-it",
                dc_intl::t("+IrDzN"),
                modal::Tone::Primary,
                true,
                move |this, _, cx| this.tutorial_dismiss(id, cx),
                cx,
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ready(tutorial: Value) -> TutorialTips {
        let mut tips = TutorialTips::default();
        tips.dispatch("READY", &json!({ "tutorial": tutorial }));
        tips
    }

    fn shown(tips: &TutorialTips) -> Vec<&'static str> {
        DEFINITIONS
            .iter()
            .map(|definition| definition.id)
            .filter(|id| tips.should_show(id))
            .collect()
    }

    #[test]
    fn nothing_shows_before_ready_or_without_a_tutorial() {
        let tips = TutorialTips::default();
        assert!(!tips.should_show_any());
        assert!(shown(&tips).is_empty());
        let tips = ready(Value::Null);
        assert!(!tips.should_show_any());
        assert!(shown(&tips).is_empty());
    }

    #[test]
    fn a_suppressed_account_shows_nothing() {
        let tips = ready(json!({"indicators_suppressed": true, "indicators_confirmed": []}));
        assert!(shown(&tips).is_empty());
    }

    #[test]
    fn a_new_account_shows_the_tips_without_unmet_prerequisites() {
        let tips = ready(json!({"indicators_suppressed": false, "indicators_confirmed": []}));
        assert_eq!(
            shown(&tips),
            vec![
                "voice-conversations",
                "direct-messages",
                "create-first-server",
                "organize-by-topic"
            ]
        );
    }

    #[test]
    fn confirming_a_tip_opens_the_ones_after_it() {
        let mut tips = ready(json!({
            "indicators_suppressed": false,
            "indicators_confirmed": ["organize-by-topic"],
        }));
        assert!(tips.should_show("instant-invite"));
        assert!(!tips.should_show("whos-online"));
        assert_eq!(
            tips.dismiss("instant-invite"),
            "/tutorial/indicators/instant-invite"
        );
        assert!(!tips.should_show("instant-invite"));
        for id in ["whos-online", "server-settings", "friends-list"] {
            assert!(tips.should_show(id), "{id}");
        }
        assert!(!tips.should_show("create-more-servers"));
        tips.dismiss("server-settings");
        assert!(tips.should_show("create-more-servers"));
    }

    #[test]
    fn dismissing_closes_its_popout() {
        let mut tips = ready(json!({"indicators_suppressed": false, "indicators_confirmed": []}));
        tips.open = Some("direct-messages");
        tips.dismiss("direct-messages");
        assert_eq!(tips.open, None);
        assert!(!tips.should_show("direct-messages"));
    }

    #[test]
    fn skip_all_suppresses_every_tip() {
        let mut tips = ready(json!({"indicators_suppressed": false, "indicators_confirmed": []}));
        tips.open = Some("create-first-server");
        assert_eq!(tips.suppress_all(), "/tutorial/indicators/suppress");
        assert!(!tips.should_show_any());
        assert!(shown(&tips).is_empty());
        assert_eq!(tips.open, None);
    }

    #[test]
    fn a_closed_gateway_hides_the_tips_until_the_next_ready() {
        let mut tips = ready(json!({"indicators_suppressed": false, "indicators_confirmed": []}));
        tips.connection_closed();
        assert!(!tips.should_show("direct-messages"));
        // A new READY resets what this session dismissed to what the server has.
        tips.dismiss("direct-messages");
        tips.dispatch(
            "READY",
            &json!({"tutorial": {"indicators_suppressed": false, "indicators_confirmed": []}}),
        );
        assert!(tips.should_show("direct-messages"));
    }

    #[test]
    fn a_missing_suppressed_field_reads_as_not_suppressed() {
        let tips = ready(json!({"indicators_confirmed": ["voice-conversations"]}));
        assert!(tips.should_show("writing-messages"));
        assert!(!tips.should_show("voice-conversations"));
    }

    #[test]
    fn the_indicator_sits_beside_its_target() {
        // `nf` of the category: (-36 + 232, 7 + 2) from the category's box.
        assert_eq!(
            indicator_origin((0., 100., 240., 40.), CATEGORY_PLACE),
            (196., 109.)
        );
        // The voice row: 20px back over its right edge, centred on its 34px.
        assert_eq!(
            indicator_origin((8., 50., 224., 34.), VOICE_PLACE),
            (212., 49.)
        );
        let left = Place {
            position: Side::Left,
            inline: None,
            offset: (0., 0.),
        };
        assert_eq!(indicator_origin((100., 100., 16., 16.), left), (64., 90.));
        let top = Place {
            position: Side::Top,
            inline: None,
            offset: (75., 0.),
        };
        assert_eq!(indicator_origin((0., 500., 400., 44.), top), (257., 464.));
    }

    #[test]
    fn the_indicator_is_placed_inside_its_target() {
        // The category's inline specs: 196 from its left, 9 down.
        assert_eq!(inset(CATEGORY_PLACE, 40.), Inset::Left(196., 9.));
        // The voice row: 16px past its right edge (36 - 20), centred on 34px.
        assert_eq!(inset(VOICE_PLACE, 34.), Inset::Right(-16., -1.));
        // The rail button: `tY` from the 72px container, 16px left of the 40px button.
        assert_eq!(inset(ADD_SERVER_PLACE, 40.), Inset::Left(16., 6.));
    }

    #[test]
    fn the_coachmark_opens_beside_the_indicator() {
        assert_eq!(
            coachmark_corner((10., 20.), Side::Right, 24.),
            (Anchor::TopLeft, (70., 20.))
        );
        assert_eq!(
            coachmark_corner((100., 20.), Side::Left, 16.),
            (Anchor::TopRight, (84., 20.))
        );
        assert_eq!(
            coachmark_corner((100., 20.), Side::Bottom, 10.),
            (Anchor::TopCenter, (118., 66.))
        );
    }

    #[test]
    fn the_loops_follow_their_keyframes() {
        let rest = pulse(0., false, true);
        assert!((rest[0] - 0.1).abs() < 1e-4 && (rest[1] - 1.1).abs() < 1e-4);
        assert!(rest[2].abs() < 1e-4 && (rest[3] - 1.1).abs() < 1e-4);
        // 42% of 1.5s: the inner circle at its brightest, brighter for a high priority tip.
        let peak = pulse(630., false, true);
        assert!((peak[0] - 0.25).abs() < 1e-4 && (peak[1] - 0.9).abs() < 1e-4);
        assert!((pulse(630., true, true)[0] - 0.5).abs() < 1e-4);
        // 21%: the exclaim is 4px up.
        assert!((pulse(315., true, true)[4] + 4.).abs() < 1e-4);
        assert_eq!(pulse(630., true, false), [0.1, 1., 0.1, 1., 0., 0.]);
    }

    #[test]
    fn every_tip_has_its_text() {
        assert_eq!(dc_intl::t("+IrDzN"), "Got it!");
        assert_eq!(dc_intl::t("33wtxt"), "Skip all tips");
        for definition in DEFINITIONS {
            let text = match definition.popout {
                Popout::Coachmark => coachmark_text(definition.id),
                Popout::Popover(_) => popover_text(definition.id, "Lounge"),
            };
            let (title, body) = text.unwrap_or_else(|| panic!("{}", definition.id));
            assert!(!title.is_empty() && !body.is_empty(), "{}", definition.id);
        }
        assert_eq!(
            coachmark_text("direct-messages").unwrap().0,
            "Start a separate conversation"
        );
        assert!(
            popover_text("writing-messages", "Lounge")
                .unwrap()
                .1
                .contains("Lounge")
        );
        assert!(
            coachmark_text("instant-invite")
                .unwrap()
                .1
                .contains("next to any channel")
        );
    }
}
