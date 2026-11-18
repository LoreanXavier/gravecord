//! CallTimer (Vencord `src/plugins/callTimer`, GPL-3.0-or-later): how long you have been in your voice connection,
//! under the channel name of the connection panel, and with `allCallTimers` how long everyone has been in their voice
//! channel, by their row in the channel list. Not part of the pinned client.
//!
//! Join times are the original's `userJoinTimes`: a person's time starts when the client first sees them in a voice
//! channel or sees them move to another one, and ends when they leave. The original follows `VOICE_STATE_UPDATES`
//! and `PASSIVE_UPDATE_V1`; here the voice states of the store are compared after each of those, which gives the same
//! map.

use super::super::tooltip::{Body, Side, Tip};
use super::super::{MONO, Root, color};
use crate::plugins::State;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, ParentElement, Styled, div, px, svg};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

const ID: &str = "CallTimer";

/// The settings that change what is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Settings {
    human: bool,
    all: bool,
    without_hover: bool,
    role_color: bool,
    track_self: bool,
    seconds: bool,
}

impl Settings {
    fn read(state: &State) -> Option<Self> {
        if !state.is_on(ID) {
            return None;
        }
        let flag = |key| state.value(ID, key) == Value::Bool(true);
        Some(Self {
            human: state.value(ID, "format").as_str() == Some("human"),
            all: flag("allCallTimers"),
            without_hover: flag("showWithoutHover"),
            role_color: flag("showRoleColor"),
            track_self: flag("trackSelf"),
            seconds: flag("showSeconds"),
        })
    }
}

fn current() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(Settings::read)
}

/// `formatDurationMs(ms, human, seconds)` of Vencord's `utils/text.ts`.
pub(super) fn format_duration_ms(ms: u64, human: bool, seconds: bool) -> String {
    let format = |n: u64| {
        if human {
            n.to_string()
        } else {
            format!("{n:02}")
        }
    };
    let unit = |s: &str| if human { s.to_owned() } else { String::new() };
    let delim = if human { " " } else { ":" };
    let d = ms / 86_400_000;
    let h = ms % 86_400_000 / 3_600_000;
    let m = ms % 86_400_000 % 3_600_000 / 60_000;
    let s = ms % 86_400_000 % 3_600_000 % 60_000 / 1000;
    let mut res = String::new();
    if d > 0 {
        res += &format!("{d}{}{delim}", unit("d"));
    }
    if h > 0 || !res.is_empty() || !seconds {
        res += &format!("{}{}{delim}", format(h), unit("h"));
    }
    if m > 0 || !res.is_empty() || !human || !seconds {
        res += &format!("{}{}", format(m), unit("m"));
    }
    if seconds && (m > 0 || !res.is_empty() || !human) {
        res += delim;
    }
    if seconds {
        res += &format!("{}{}", format(s), unit("s"));
    }
    res
}

/// `userJoinTimes`: each person's voice channel and when they were first seen in it.
#[derive(Debug, Default)]
pub(super) struct Joins {
    by_user: HashMap<Id, (Id, u64)>,
}

impl Joins {
    /// Compares the voice states now (user, channel) with the map: a new channel starts a time, a missing one ends it.
    pub(super) fn observe(&mut self, now: impl IntoIterator<Item = (Id, Id)>, at: u64) {
        let now: HashMap<Id, Id> = now
            .into_iter()
            .filter(|(_, channel)| *channel != 0)
            .collect();
        self.by_user.retain(|user, _| now.contains_key(user));
        for (user, channel) in now {
            match self.by_user.get(&user) {
                Some((known, _)) if *known == channel => {}
                _ => {
                    self.by_user.insert(user, (channel, at));
                }
            }
        }
    }

    pub(super) fn since(&self, user: Id) -> Option<u64> {
        self.by_user.get(&user).map(|(_, at)| *at)
    }
}

thread_local! {
    static JOINS: RefCell<Joins> = RefCell::new(Joins::default());
    /// The one-second redraw while times run (`useFixedTimer`).
    static TICKER: RefCell<Option<gpui::Task<()>>> = const { RefCell::new(None) };
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

impl Root {
    /// After a voice state dispatch: the join times follow the store. Off, the map is dropped.
    pub(in crate::ui) fn call_timer_observe(&mut self, cx: &mut Context<Self>) {
        if current().is_none() {
            JOINS.with(|joins| *joins.borrow_mut() = Joins::default());
            TICKER.with(|ticker| ticker.borrow_mut().take());
            return;
        }
        let states: Vec<(Id, Id)> = self
            .stores
            .voice_states
            .values()
            .flatten()
            .map(|state| (state.user_id, state.channel_id))
            .collect();
        let running = JOINS.with(|joins| {
            let mut joins = joins.borrow_mut();
            joins.observe(states, now_ms());
            !joins.by_user.is_empty()
        });
        let ticking = TICKER.with(|ticker| ticker.borrow().is_some());
        if running && !ticking {
            let task = cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    let alive = this.update(cx, |_, cx| {
                        cx.notify();
                        current().is_some()
                    });
                    if !matches!(alive, Ok(true)) {
                        TICKER.with(|ticker| ticker.borrow_mut().take());
                        break;
                    }
                }
            });
            TICKER.with(|ticker| *ticker.borrow_mut() = Some(task));
        }
    }

    /// `ConnectionTimer`: your own time under the connection panel's channel line, in the code font, seconds always.
    pub(in crate::ui) fn call_timer_connection(&self) -> Option<AnyElement> {
        let settings = current()?;
        let me = self.stores.current_user.as_ref()?.id;
        let since = JOINS.with(|joins| joins.borrow().since(me))?;
        let text = format_duration_ms(now_ms().saturating_sub(since), settings.human, true);
        Some(
            div()
                .font_family(MONO)
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-subtle"))
                .child(text)
                .into_any_element(),
        )
    }

    /// `renderTimer` with `allCallTimers`: by a person's voice row, the clock icon with the time in its tooltip, or
    /// with `showWithoutHover` the time itself under the name (bold, mono, 11px). Not for yourself unless `trackSelf`.
    pub(in crate::ui) fn call_timer_row(
        &self,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<(bool, AnyElement)> {
        let settings = current()?;
        if !settings.all {
            return None;
        }
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        if me == Some(user) && !settings.track_self {
            return None;
        }
        let since = JOINS.with(|joins| joins.borrow().since(user))?;
        let text = format_duration_ms(
            now_ms().saturating_sub(since),
            settings.human,
            settings.seconds,
        );
        if settings.without_hover {
            let tint = match self.role_color_of(user).filter(|_| settings.role_color) {
                Some(rgb) => gpui::rgb(rgb),
                None => color("--channels-default"),
            };
            return Some((
                true,
                div()
                    .mt(px(-6.))
                    .font_family(MONO)
                    .font_weight(FontWeight(700.))
                    .text_size(px(11.))
                    .text_color(tint)
                    .child(text)
                    .into_any_element(),
            ));
        }
        let icon = svg()
            .path("icons/plugin_timer.svg")
            .size(px(16.))
            .text_color(color("--channels-default"));
        Some((
            false,
            self.with_tip(
                format!("call-timer-{user}"),
                Tip::body(Body::Text(text.into()), Side::Top),
                div().child(icon),
                cx,
            )
            .into_any_element(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn durations_format_as_vencord() {
        assert_eq!(format_duration_ms(12_000, false, true), "00:12");
        assert_eq!(format_duration_ms(3_723_000, false, true), "01:02:03");
        assert_eq!(format_duration_ms(90_061_000, false, true), "1:01:01:01");
        assert_eq!(format_duration_ms(12_000, true, true), "12s");
        assert_eq!(format_duration_ms(3_723_000, true, true), "1h 2m 3s");
        // Without seconds (the rows' default): hours and minutes.
        assert_eq!(format_duration_ms(3_723_000, false, false), "01:02");
        assert_eq!(format_duration_ms(12_000, false, false), "00:00");
        assert_eq!(format_duration_ms(125_000, true, false), "0h 2m");
    }

    #[test]
    fn join_times_start_on_a_new_channel_and_end_on_leaving() {
        let mut joins = Joins::default();
        joins.observe([(1, 10), (2, 10), (3, 0)], 100);
        assert_eq!(
            (joins.since(1), joins.since(2), joins.since(3)),
            (Some(100), Some(100), None)
        );
        joins.observe([(1, 10), (2, 11)], 200);
        assert_eq!(
            (joins.since(1), joins.since(2)),
            (Some(100), Some(200)),
            "2 moved"
        );
        joins.observe([(2, 11)], 300);
        assert_eq!(joins.since(1), None, "1 left");
    }

    #[test]
    fn settings_read_as_the_original() {
        assert_eq!(Settings::read(&State::default()), None);
        let mut state = State::default();
        state.set_enabled(ID, true);
        let settings = Settings::read(&state).unwrap();
        assert!(!settings.human && !settings.all && !settings.without_hover);
        assert!(!settings.role_color && !settings.track_self && !settings.seconds);
        state.set_value(ID, "format", &json!("human"));
        state.set_value(ID, "allCallTimers", &json!(true));
        let settings = Settings::read(&state).unwrap();
        assert!(settings.human && settings.all);
    }
}
