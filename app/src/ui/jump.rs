//! Jumping to a message that may not be loaded, and the window that follows it.
//!
//! The store keeps a window of a channel's messages that may stop short of the newest one (`ChannelMessages`,
//! `dc_state::message_window`). A jump (`MessageActions.jumpToMessage`, module 138399) loads a page `around` the
//! message when the window does not cover it, then the view scrolls the message to the middle and flashes it
//! (module 891522: `scrollToMessage`, `getOffsetOrientationFromNode`, the `flash` wrapper `ts`). Scrolling a window
//! that is not at the newest message loads `before` and `after` pages (`ScrollManager.loadMore`).

use super::{Root, Row, color, motion};
use crate::net::Request;
use dc_state::Id;
use dc_state::message_window::{self, FetchSpec, Jump};
use gpui::{AnyElement, Context, ListOffset, Window, div, prelude::*, px};

/// `ts`: the flash wrapper keeps `data-flash` for this long.
const FLASH_MS: f64 = 2000.;
/// `.full-motion .flash_EajVQV`: the background and the overlay fade out over this.
const FADE_OUT: motion::Css = motion::Css::new(600., motion::Ease::Ease).full_motion();
/// `.full-motion .flash_EajVQV[data-flash=true]`: they fade in over this.
const FADE_IN: motion::Css = motion::Css::new(200., motion::Ease::Ease).full_motion();
/// `flashPulse_EajVQV`: `.6s ease-in-out`, opacity 1 at 0, 40, 80 and 100 percent and 0 at 20 and 60.
const PULSE_MS: f64 = 600.;
const PULSE: [(f32, f32); 6] = [
    (0., 1.),
    (0.2, 0.),
    (0.4, 1.),
    (0.6, 0.),
    (0.8, 1.),
    (1., 1.),
];
/// The pulse starts when the flashed row has not moved for three animation frames (`ts`).
const PULSE_DELAY_MS: f64 = 50.;
/// `scrollManager.loadMore` and `getOffsetToTriggerLoading`: a load starts this close to the edge of the content.
const TRIGGER: f32 = 500.;
/// `getOffsetOrientationFromNode`: the gap above the target and the nudge of the middle (`-8`).
const TOP_PADDING: f32 = 16.;
const TOP_PADDING_UNREAD: f32 = 32.;
const MIDDLE_NUDGE: f32 = 8.;
/// Frames a jump waits for its row to be measured before it gives up.
const MAX_FRAMES: u32 = 40;

#[derive(Default)]
pub(super) struct JumpUi {
    pending: Option<Pending>,
    flash: Option<Flash>,
    /// The channel a route with a message opens: its first page is the jump's, not the newest.
    route: Option<(Id, Id)>,
}

struct Pending {
    channel: Id,
    message: Id,
    flash: bool,
    sequence: u64,
    anchored: bool,
    frames: u32,
}

#[derive(Clone, Copy)]
struct Flash {
    channel: Id,
    message: Id,
    sequence: u64,
    started: f64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// Where the target's top sits in the scroller after a jump (`getOffsetOrientationFromNode(node, "middle", pad)`):
/// `scrollTop = min(top - H / 2 + h / 2 - 8, top - pad)`, so the node's top is `max(H / 2 - h / 2 + 8, pad)` below
/// the top of the viewport.
pub(super) fn middle_offset(viewport: f32, row: f32, padding: f32) -> f32 {
    (viewport / 2. - row / 2. + MIDDLE_NUDGE).max(padding)
}

/// The flash of a message at `elapsed` ms: how opaque the background is and the overlay, and where the pulse is.
/// `None` once it has faded out.
pub(super) fn flash_at(elapsed: f64) -> Option<(bool, f64)> {
    if elapsed < 0. {
        return Some((true, 0.));
    }
    if elapsed >= FLASH_MS + FADE_OUT.duration_ms {
        return None;
    }
    Some((elapsed < FLASH_MS, elapsed))
}

/// The overlay's opacity from the pulse animation, `None` outside it.
pub(super) fn pulse_opacity(elapsed: f64) -> Option<f32> {
    let into = elapsed - PULSE_DELAY_MS;
    (0. ..PULSE_MS)
        .contains(&into)
        .then(|| motion::keyframes(&PULSE, motion::Ease::EaseInOut, (into / PULSE_MS) as f32))
}

impl Root {
    /// The guild a channel belongs to, or `None` for a direct message.
    fn guild_containing(&self, channel: Id) -> Option<Id> {
        self.stores
            .guilds
            .iter()
            .find(|guild| {
                guild.channels.iter().any(|known| known.id == channel)
                    || guild.threads.iter().any(|known| known.id == channel)
            })
            .map(|guild| guild.id)
    }

    /// Whether the open channel's window stops short of its newest message (`messages.hasMoreAfter`).
    pub(super) fn window_detached(&self) -> bool {
        self.channel.is_some_and(|channel| {
            self.stores
                .windows
                .get(&channel)
                .is_some_and(|window| window.has_more_after)
        })
    }

    /// The page size of the window now (module 326337).
    fn page(&self) -> u32 {
        message_window::page_size(self.viewport.1, false, 16.)
    }

    /// `MessageManager` on a channel select (module 123363): the jump state is cleared, and the channel's newest
    /// page is fetched unless the route carries a message, which is fetched instead.
    pub(super) fn open_channel_messages(&mut self, channel: Id) {
        self.jump.pending = None;
        self.jump.flash = None;
        if let Some(window) = self.stores.windows.get_mut(&channel) {
            window.jump_target_id = None;
            window.jumped = false;
        }
        if self.jump.route.is_some_and(|(routed, _)| routed == channel) {
            return;
        }
        let _ = self.requests.send(Request::LoadMessages { channel });
    }

    /// The one way to go to a message (`jumpToMessage`): opens its channel when another one is open, loads the page
    /// around it when the window does not cover it, then scrolls to it and flashes it.
    pub(super) fn jump_to_message_id(
        &mut self,
        channel: Id,
        message: Id,
        flash: bool,
        return_to: Option<Id>,
    ) {
        if self.channel != Some(channel) {
            self.jump.route = Some((channel, message));
            match self.guild_containing(channel) {
                Some(guild) => self.select(guild, Some(channel)),
                None => self.select_home(Some(channel)),
            }
            self.jump.route = None;
        }
        let spec = FetchSpec {
            channel,
            limit: message_window::JUMP_PAGE,
            jump: Some(Jump::to_message(message, flash, return_to)),
            last_message_id: self.stores.read_states.latest.get(&channel).copied(),
            started_ms: now_ms(),
            ..FetchSpec::default()
        };
        self.fetch(spec);
    }

    /// Brings a message of the open channel into view without a flash (`jumpToMessage` with `flash: false`): the
    /// new-messages bar and the keyboard's jump to the oldest unread message.
    pub(super) fn scroll_to_message(&mut self, message: Id) {
        if let Some(channel) = self.channel {
            self.jump_to_message_id(channel, message, false, None);
        }
    }

    /// `jumpToPresent(channel, limit)`: drops the window and shows the newest page.
    pub(super) fn jump_to_present_with(&mut self, limit: u32) {
        let Some(channel) = self.channel else {
            return;
        };
        super::unread::set_window_detached(channel, false);
        let spec = FetchSpec {
            channel,
            limit,
            jump: Some(Jump::to_present()),
            started_ms: now_ms(),
            ..FetchSpec::default()
        };
        self.fetch(spec);
    }

    /// `fetchMessages`: the store decides whether the window can answer, else the request goes out.
    fn fetch(&mut self, spec: FetchSpec) {
        let channel = spec.channel;
        if self.stores.fetch_messages(&spec) {
            let _ = self.requests.send(Request::FetchMessages { spec });
        } else {
            self.after_window_change(channel, None);
        }
    }

    /// `LOAD_MESSAGES_SUCCESS` for a page the window asked for.
    pub(super) fn window_loaded(
        &mut self,
        spec: FetchSpec,
        page: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let channel = spec.channel;
        let anchor = self.scroll_anchor();
        self.stores.messages_loaded(&spec, &page);
        self.after_window_change(channel, Some(anchor));
        cx.notify();
    }

    /// Rebuilds what the window shows and starts the scroll it asks for: the jump target, the end for a jump to
    /// the present, or the place the view was at (`anchor`) after a page was added above or below it.
    fn after_window_change(&mut self, channel: Id, anchor: Option<Option<(Id, f32)>>) {
        if Some(channel) != self.channel {
            self.request_images();
            return;
        }
        let anchor = anchor.unwrap_or_else(|| self.scroll_anchor());
        let window = self.stores.window(channel);
        // Reading a channel acks its newest message: not while the window is somewhere else.
        if !window.has_more_after {
            self.read_current_channel();
        }
        self.rebuild_rows();
        if window.jumped_to_present && !window.has_more_after {
            self.list.scroll_to_end();
            self.jump.pending = None;
        } else if let (true, Some(target)) = (window.jumped, window.jump_target_id) {
            self.jump.flash = None;
            self.jump.pending = Some(Pending {
                channel,
                message: target,
                flash: window.jump_flash,
                sequence: window.jump_sequence_id,
                anchored: false,
                frames: 0,
            });
        } else if self.jump.pending.is_none() {
            self.restore_scroll_anchor(anchor);
        }
        self.request_images();
        self.ready = true;
    }

    /// The message at the top of the view and how far into it the view starts, to hold the place while rows are
    /// added around it. `None` when the view follows the end.
    pub(super) fn scroll_anchor(&self) -> Option<(Id, f32)> {
        if self.list.is_scrolled_to_end() == Some(true) {
            return None;
        }
        let top = self.list.logical_scroll_top();
        let channel = self.channel?;
        let messages = self.stores.messages.get(&channel)?;
        let (at, row) = self
            .rows
            .iter()
            .enumerate()
            .skip(top.item_ix)
            .find_map(|(at, row)| match row {
                Row::Message { index, .. } => Some((at, messages.get(*index)?.id)),
                _ => None,
            })?;
        let offset = if at == top.item_ix {
            f32::from(top.offset_in_item)
        } else {
            0.
        };
        Some((row, offset))
    }

    /// Scrolls the rows back to `anchor` after a rebuild.
    pub(super) fn restore_scroll_anchor(&mut self, anchor: Option<(Id, f32)>) {
        let Some((message, offset)) = anchor else {
            return;
        };
        if let Some(row) = self.row_of(message) {
            self.list.scroll_to(ListOffset {
                item_ix: row,
                offset_in_item: px(offset),
            });
        }
    }

    /// The row that draws `message` in the open channel.
    fn row_of(&self, message: Id) -> Option<usize> {
        let messages = self.stores.messages.get(&self.channel?)?;
        let index = messages.iter().position(|known| known.id == message)?;
        self.rows
            .iter()
            .position(|row| matches!(row, Row::Message { index: at, .. } if *at == index))
    }

    /// Called every frame, before the rows are built: moves a jump along, starts the loads the scroll position
    /// asks for, and tells the store whether the view is at the bottom.
    pub(super) fn pump_window(&mut self, window: &mut Window) {
        let Some(channel) = self.channel else {
            return;
        };
        let at_bottom = self.list.is_scrolled_to_end() == Some(true);
        if let Some(state) = self.stores.windows.get_mut(&channel) {
            state.at_bottom = at_bottom;
        }
        if self
            .jump
            .flash
            .is_some_and(|flash| flash_at(motion::now() - flash.started).is_none())
        {
            self.jump.flash = None;
        }
        if self.jump.pending.is_some() {
            self.advance_jump(window);
            return;
        }
        self.load_when_near_the_edge(channel);
    }

    /// The scroll of a jump: first the target's row goes to the top of the list, which makes the list measure it,
    /// then the list is moved so the row sits where `getOffsetOrientationFromNode` puts it.
    fn advance_jump(&mut self, window: &mut Window) {
        let Some(mut pending) = self.jump.pending.take() else {
            return;
        };
        if Some(pending.channel) != self.channel || self.stores.window(pending.channel).loading_more
        {
            return;
        }
        let Some(row) = self.row_of(pending.message) else {
            return;
        };
        if !pending.anchored {
            self.list.scroll_to(ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
            pending.anchored = true;
            self.jump.pending = Some(pending);
            window.request_animation_frame();
            return;
        }
        pending.frames += 1;
        match self.list.bounds_for_item(row) {
            Some(bounds) => {
                let viewport = self.list.viewport_bounds();
                let padding = if self.divider_after().is_some() {
                    TOP_PADDING_UNREAD
                } else {
                    TOP_PADDING
                };
                let wanted = middle_offset(
                    f32::from(viewport.size.height),
                    f32::from(bounds.size.height),
                    padding,
                );
                let at = f32::from(bounds.top() - viewport.top());
                let delta = at - wanted;
                if delta.abs() > 0.5 && pending.frames < MAX_FRAMES {
                    self.list.scroll_by(px(delta));
                    self.jump.pending = Some(pending);
                    window.request_animation_frame();
                    return;
                }
                self.finish_jump(&pending);
            }
            None if pending.frames < MAX_FRAMES => {
                self.jump.pending = Some(pending);
                window.request_animation_frame();
            }
            None => self.finish_jump(&pending),
        }
    }

    fn finish_jump(&mut self, pending: &Pending) {
        if pending.flash {
            self.jump.flash = Some(Flash {
                channel: pending.channel,
                message: pending.message,
                sequence: pending.sequence,
                started: motion::now(),
            });
        }
    }

    /// `handleScroll` of the scroll manager: near the top of the content `before` loads, near the bottom of a
    /// window that stops short of the newest message `after` loads (limit `min(30, 2 * page)`).
    fn load_when_near_the_edge(&mut self, channel: Id) {
        let state = self.stores.window(channel);
        if !state.ready || state.loading_more || state.error {
            return;
        }
        let Some(messages) = self.stores.messages.get(&channel) else {
            return;
        };
        let (Some(first), Some(last)) = (messages.first(), messages.last()) else {
            return;
        };
        // `is_scrolled_to_end` is `None` while rows are still unmeasured, so the offsets (which estimate) decide.
        let into = -f32::from(self.list.scroll_px_offset_for_scrollbar().y);
        let max = f32::from(self.list.max_offset_for_scrollbar().y);
        // Rows that are not measured yet count as a few pixels, which would read as "at the edge" right after every
        // rebuild: a window shorter than the view is waited out until its rows have a height.
        if max <= f32::from(self.list.viewport_bounds().size.height) {
            return;
        }
        let to_end = max - into;
        let limit = message_window::load_more_limit(self.page());
        let (before, after) = (
            (state.has_more_before && into <= TRIGGER).then_some(first.id),
            (state.has_more_after && to_end <= TRIGGER).then_some(last.id),
        );
        let (before, after) = match (before, after) {
            (Some(before), _) => (Some(before), None),
            (None, after) => (None, after),
        };
        if before.is_none() && after.is_none() {
            return;
        }
        let spec = FetchSpec {
            channel,
            before,
            after,
            limit,
            truncate: true,
            started_ms: now_ms(),
            ..FetchSpec::default()
        };
        // Rows are added above or below the view: it keeps the message it shows.
        let anchor = self.scroll_anchor();
        if self.stores.fetch_messages(&spec) {
            let _ = self.requests.send(Request::FetchMessages { spec });
        } else {
            self.after_window_change(channel, Some(anchor));
        }
    }

    /// The layers `ts` puts in a flashed message: the background, which is also the first child of the row, and the
    /// overlay with the 2px brand edge. Both are `--message-highlight-background-default`.
    pub(super) fn flash_layers(&self, message: Id) -> Vec<AnyElement> {
        let Some(flash) = self.jump.flash else {
            return Vec::new();
        };
        if Some(flash.channel) != self.channel || flash.message != message {
            return Vec::new();
        }
        let elapsed = motion::now() - flash.started;
        let Some((on, _)) = flash_at(elapsed) else {
            return Vec::new();
        };
        motion::keep_animating();
        let key = motion::key(&("flash", flash.channel, flash.message, flash.sequence));
        let css = if on { FADE_IN } else { FADE_OUT };
        let level = motion::value_from(key, 0., if on { 1. } else { 0. }, css);
        let mut tint = color("--message-highlight-background-default");
        let pulse = if motion::reduced() {
            None
        } else {
            pulse_opacity(elapsed)
        };
        let overlay = pulse.unwrap_or(level);
        tint.a *= level;
        let mut edge = color("--message-highlight-background-default");
        edge.a *= overlay;
        let mut brand = color("--text-brand");
        brand.a *= overlay;
        vec![
            div()
                .absolute()
                .inset_0()
                .rounded_r(px(4.))
                .bg(tint)
                .into_any_element(),
            div()
                .absolute()
                .inset_0()
                .rounded_r(px(4.))
                .border_l(px(2.))
                .border_color(brand)
                .bg(edge)
                .into_any_element(),
        ]
    }

    /// Where the jump target is on screen, for the harness.
    #[cfg(feature = "capture")]
    fn jump_target_report(&self, state: &message_window::ChannelWindow) -> String {
        let scroll = format!(
            " into={:.0} max={:.0} at_end={:?}",
            -f32::from(self.list.scroll_px_offset_for_scrollbar().y),
            f32::from(self.list.max_offset_for_scrollbar().y),
            self.list.is_scrolled_to_end()
        );
        let Some(target) = state.jump_target_id else {
            return scroll;
        };
        let viewport = self.list.viewport_bounds();
        match self
            .row_of(target)
            .map(|row| (row, self.list.bounds_for_item(row)))
        {
            Some((row, Some(bounds))) => format!(
                "{scroll} target_row={row} top={:.0} height={:.0} viewport_top={:.0} viewport_height={:.0}",
                f32::from(bounds.top()),
                f32::from(bounds.size.height),
                f32::from(viewport.top()),
                f32::from(viewport.size.height)
            ),
            Some((row, None)) => format!("{scroll} target_row={row} not measured"),
            None => format!("{scroll} target not in rows"),
        }
    }

    /// Harness: `jump <message id>`, `jump reply`, `jump present`, `jump state`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_jump(&mut self, rest: &[&str]) -> String {
        let Some(channel) = self.channel else {
            return "jump: no channel".to_owned();
        };
        match rest {
            ["present"] => {
                let page = self.page();
                self.jump_to_present_with(page);
                "jump: present".to_owned()
            }
            // The reply preview of the oldest loaded reply whose original is not loaded, as a click on it does.
            ["reply"] => {
                let target = self.stores.messages.get(&channel).and_then(|list| {
                    list.iter().find_map(|message| {
                        let original = message.referenced.as_ref()?;
                        (!list.iter().any(|known| known.id == original.id)).then_some((
                            original.channel_id,
                            original.id,
                            message.id,
                        ))
                    })
                });
                match target {
                    Some((channel, original, reply)) => {
                        self.jump_to_message_id(channel, original, true, Some(reply));
                        format!("jump: reply {original}")
                    }
                    None => "jump: no reply to an unloaded message".to_owned(),
                }
            }
            // The middle message of the loaded window, flashed.
            ["mid"] => {
                let middle = self
                    .stores
                    .messages
                    .get(&channel)
                    .and_then(|list| list.get(list.len() / 2))
                    .map(|message| message.id);
                match middle {
                    Some(message) => {
                        self.jump_to_message_id(channel, message, true, None);
                        format!("jump: mid {message}")
                    }
                    None => "jump: nothing loaded".to_owned(),
                }
            }
            ["state"] => {
                let state = self.stores.window(channel);
                let messages = self.stores.messages.get(&channel);
                format!(
                    "jump: ready={} before={} after={} loading={} target={:?} seq={} len={} first={:?} last={:?} pending={} flash={}",
                    state.ready,
                    state.has_more_before,
                    state.has_more_after,
                    state.loading_more,
                    state.jump_target_id,
                    state.jump_sequence_id,
                    messages.map_or(0, Vec::len),
                    messages.and_then(|list| list.first()).map(|m| m.id),
                    messages.and_then(|list| list.last()).map(|m| m.id),
                    self.jump.pending.is_some(),
                    self.jump.flash.is_some()
                ) + &self.jump_target_report(&state)
            }
            [message] => match message.parse::<Id>() {
                Ok(message) => {
                    self.jump_to_message_id(channel, message, true, None);
                    format!("jump: {message}")
                }
                Err(_) => "jump: not an id".to_owned(),
            },
            _ => "jump: present | reply | state | <message id>".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_is_centred_with_a_gap_of_16() {
        // `min(top - H/2 + h/2 - 8, top - 16)`: a 60px row in a 500px viewport sits 228px down.
        assert_eq!(middle_offset(500., 60., 16.), 500. / 2. - 30. + 8.);
        // A tall row cannot go above the padding.
        assert_eq!(middle_offset(500., 600., 16.), 16.);
        assert_eq!(middle_offset(500., 600., 32.), 32.);
    }

    #[test]
    fn the_flash_lasts_two_seconds_and_fades_out_over_six_tenths() {
        assert_eq!(flash_at(0.), Some((true, 0.)));
        assert_eq!(flash_at(1999.), Some((true, 1999.)));
        assert_eq!(flash_at(2000.), Some((false, 2000.)));
        assert_eq!(flash_at(2599.), Some((false, 2599.)));
        assert_eq!(flash_at(2600.), None);
    }

    #[test]
    fn the_pulse_blinks_the_overlay_twice_in_six_tenths() {
        assert_eq!(pulse_opacity(10.), None);
        let at = |ms: f64| pulse_opacity(PULSE_DELAY_MS + ms).unwrap();
        assert_eq!(at(0.), 1.);
        assert!(at(120.) < 0.01);
        assert!((at(240.) - 1.).abs() < 0.01);
        assert!(at(360.) < 0.01);
        assert!((at(480.) - 1.).abs() < 0.01);
        assert_eq!(pulse_opacity(PULSE_DELAY_MS + PULSE_MS), None);
    }
}
