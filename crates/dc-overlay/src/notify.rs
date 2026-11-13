//! The notifications widget's queue (`OverlayNotificationsStore`, module 348639; the widget `rS` of module 415173;
//! the stacking of modules 376113 and 57019).
//!
//! - [`Queue::push`] (`eh`): a notification goes before the first one of the same or a lower priority; past 40 the
//!   last falls off. While one is focused (being replied to) new ones wait aside and join when it lets go (`eE`).
//! - Each has a timer from its own duration (5 s unless given): when it runs out the notification is removed, or only
//!   marked timed out when its expiry is "externally managed" (text messages, calls, activity invites), so the
//!   unlocked overlay can still list it.
//! - [`Queue::set_locked`] (`OVERLAY_SET_INPUT_LOCKED`): locking puts a focused one back; unlocking drops the nudges,
//!   restarts the timers of the plain ones and focuses the newest text message.
//! - [`Queue::visible`] (`rS`): at most five, and while locked none that timed out.
//! - [`stack_pose`] (`Ru`, module 57019): locked, the first is drawn and the next ones peek out under it, smaller;
//!   unlocked they are listed 8 px apart.
//!
//! Time is passed in as milliseconds, so the rules are pure.

/// `zb` (module 897720).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Generic,
    Text,
    IncomingCall,
    Nudge,
}

/// `In` (module 897720): higher goes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Normal = 0,
    High = 1,
    Urgent = 2,
}

/// `yFH` (module 652215).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Active,
    TimedOut,
    Dismissed,
    Focused,
}

/// `es`: the default duration.
pub const DURATION: f64 = 5000.;
/// `eo`: the welcome nudge's.
pub const NUDGE_DURATION: f64 = 8000.;
/// `ec`: an activity invite's.
pub const ACTIVITY_DURATION: f64 = 30_000.;
/// `eu`: a timed-out text message older than this is dropped when the list is tidied.
pub const TEXT_KEPT: f64 = 30_000.;
/// The welcome nudge comes a second after the overlay is mounted (module 427830).
pub const MOUNT_DELAY: f64 = 1000.;
/// The list's length cap.
pub const MAX: usize = 40;
/// The widget lists the first five (`i > 4` ends the loop).
pub const SHOWN: usize = 5;

/// What a notification is pushed with (`ed` merged with the caller's options).
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub priority: Priority,
    pub kind: Kind,
    pub duration: f64,
    pub externally_managed: bool,
    pub channel: Option<u64>,
    pub unique_key: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            priority: Priority::Normal,
            kind: Kind::Generic,
            duration: DURATION,
            externally_managed: false,
            channel: None,
            unique_key: None,
        }
    }
}

/// One notification and its timer.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<T> {
    pub id: u64,
    pub status: Status,
    pub timestamp: f64,
    pub options: Options,
    /// When the timer fires and what it sets, while it runs.
    timer: Option<(f64, Status)>,
    pub content: T,
}

impl<T> Entry<T> {
    fn start(&mut self, now: f64, externally_managed: bool) {
        if self.timer.is_none() {
            let target = if externally_managed {
                Status::TimedOut
            } else {
                Status::Dismissed
            };
            self.timer = Some((now + self.options.duration, target));
        }
    }

    fn stop(&mut self) {
        self.timer = None;
    }

    /// When the timer fires, while it runs.
    pub fn deadline(&self) -> Option<f64> {
        self.timer.map(|(at, _)| at)
    }
}

/// The store's list (`eA`), the ones held while one is focused (`ep`) and the hold flag (`ef`).
#[derive(Clone, Debug)]
pub struct Queue<T> {
    list: Vec<Entry<T>>,
    held: Vec<Entry<T>>,
    holding: bool,
    next: u64,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            held: Vec::new(),
            holding: false,
            next: 1,
        }
    }
}

impl<T: Clone> Queue<T> {
    pub fn entries(&self) -> &[Entry<T>] {
        &self.list
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty() && self.held.is_empty()
    }

    /// `eh`: adds a notification; returns its id.
    pub fn push(&mut self, content: T, options: Options, now: f64) -> u64 {
        let id = self.next;
        self.next += 1;
        let entry = Entry {
            id,
            status: Status::Active,
            timestamp: now,
            options,
            timer: None,
            content,
        };
        let priority = entry.options.priority;
        let target = if self.holding {
            &mut self.held
        } else {
            &mut self.list
        };
        match target
            .iter()
            .position(|other| other.options.priority <= priority)
        {
            Some(at) => target.insert(at, entry),
            None => target.push(entry),
        }
        if target.len() > MAX
            && let Some(mut last) = target.pop()
        {
            last.stop();
        }
        if !self.holding
            && let Some(entry) = self.list.iter_mut().find(|entry| entry.id == id)
        {
            let managed = entry.options.externally_managed;
            entry.start(now, managed);
        }
        id
    }

    /// `eE`: with nothing focused any more, the held ones join and every timer runs again.
    fn release(&mut self, now: f64) {
        if !self.holding
            || self
                .list
                .iter()
                .any(|entry| entry.status == Status::Focused)
        {
            return;
        }
        self.holding = false;
        self.list.append(&mut self.held);
        self.list.truncate(MAX);
        for entry in &mut self.list {
            let managed = entry.options.externally_managed;
            entry.start(now, managed);
        }
    }

    /// `eN` (`OVERLAY_UPDATE_NOTIFICATION_STATUS`): dismissed removes it, focused moves it to the front and holds new
    /// ones, any other status is set. Its timer stops either way.
    pub fn update_status(&mut self, id: u64, status: Status, now: f64) -> bool {
        let Some(at) = self.list.iter().position(|entry| entry.id == id) else {
            return false;
        };
        self.list[at].stop();
        match status {
            Status::Focused => {
                let mut entry = self.list.remove(at);
                entry.status = Status::Focused;
                self.list.insert(0, entry);
                self.holding = true;
                return true;
            }
            Status::Dismissed => {
                self.list.remove(at);
            }
            other => self.list[at].status = other,
        }
        self.release(now);
        true
    }

    /// `e_`: timed-out text messages past the newest `keep`, or older than 30 s, are dismissed.
    pub fn tidy_text(&mut self, keep: usize, now: f64) {
        let mut timed_out: Vec<(u64, f64)> = self
            .list
            .iter()
            .filter(|entry| entry.options.kind == Kind::Text && entry.status == Status::TimedOut)
            .map(|entry| (entry.id, entry.timestamp))
            .collect();
        timed_out.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (index, (id, timestamp)) in timed_out.into_iter().enumerate() {
            if index >= keep || timestamp < now - TEXT_KEPT {
                self.update_status(id, Status::Dismissed, now);
            }
        }
    }

    /// `eS`: of the notifications sharing a unique key only the newest stays.
    pub fn dedupe(&mut self, now: f64) {
        let mut keyed: Vec<(u64, f64, String)> = self
            .list
            .iter()
            .filter_map(|entry| {
                Some((entry.id, entry.timestamp, entry.options.unique_key.clone()?))
            })
            .collect();
        keyed.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut seen = std::collections::HashSet::new();
        for (id, _, key) in keyed {
            if !seen.insert(key) {
                self.update_status(id, Status::Dismissed, now);
            }
        }
    }

    /// The timers that ran out by `now` set their status. Returns whether anything changed.
    pub fn tick(&mut self, now: f64) -> bool {
        let due: Vec<(u64, Status)> = self
            .list
            .iter()
            .filter_map(|entry| match entry.timer {
                Some((at, target)) if at <= now => Some((entry.id, target)),
                _ => None,
            })
            .collect();
        let changed = !due.is_empty();
        for (id, status) in due {
            self.update_status(id, status, now);
        }
        changed
    }

    /// The next time a timer fires, if one runs.
    pub fn next_deadline(&self) -> Option<f64> {
        self.list
            .iter()
            .filter_map(Entry::deadline)
            .min_by(f64::total_cmp)
    }

    /// `OVERLAY_SET_INPUT_LOCKED`.
    pub fn set_locked(&mut self, locked: bool, now: f64) {
        if locked {
            let focused: Vec<u64> = self
                .list
                .iter()
                .filter(|entry| entry.status == Status::Focused)
                .map(|entry| entry.id)
                .collect();
            for id in focused {
                self.update_status(id, Status::Active, now);
            }
            return;
        }
        self.tidy_text(3, now);
        let ids: Vec<(u64, Kind, Status, bool)> = self
            .list
            .iter()
            .map(|entry| {
                (
                    entry.id,
                    entry.options.kind,
                    entry.status,
                    entry.options.externally_managed,
                )
            })
            .collect();
        for (id, kind, status, managed) in ids {
            if kind == Kind::Nudge {
                self.update_status(id, Status::Dismissed, now);
            } else if status == Status::Active
                && !managed
                && let Some(entry) = self.list.iter_mut().find(|entry| entry.id == id)
            {
                entry.stop();
                entry.start(now, managed);
            }
        }
        let newest_text = self
            .list
            .iter()
            .filter(|entry| entry.options.kind == Kind::Text)
            .max_by(|a, b| a.timestamp.total_cmp(&b.timestamp))
            .map(|entry| entry.id);
        if let Some(id) = newest_text {
            self.update_status(id, Status::Focused, now);
        }
    }

    /// `CHANNEL_SELECT`: the text messages of a channel just opened go.
    pub fn channel_selected(&mut self, channel: u64, now: f64) -> bool {
        let before = self.list.len();
        self.list.retain(|entry| {
            entry.options.kind != Kind::Text || entry.options.channel != Some(channel)
        });
        let changed = self.list.len() != before;
        if changed {
            self.release(now);
        }
        changed
    }

    /// `eC`: the incoming call notification of a channel.
    pub fn call_of(&self, channel: u64) -> Option<u64> {
        self.list
            .iter()
            .find(|entry| {
                entry.options.kind == Kind::IncomingCall && entry.options.channel == Some(channel)
            })
            .map(|entry| entry.id)
    }

    /// `rS`: what the widget lists, with each one's index in the stack.
    pub fn visible(&self, locked: bool) -> Vec<(usize, &Entry<T>)> {
        self.list
            .iter()
            .filter(|entry| !(locked && entry.status == Status::TimedOut))
            .take(SHOWN)
            .enumerate()
            .collect()
    }
}

/// The spring every notification moves with (`A` of module 57019).
pub const SPRING: (f64, f64, f64) = (320., 25., 0.8);
/// How long a dismissed one takes to go (`setTimeout(n, 300)`).
pub const LEAVE_MS: f64 = 300.;

/// Where one notification rests (`E` of `Ru`, module 57019).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub opacity: f32,
    pub scale: f32,
    /// Its offset down from the widget's top.
    pub offset: f32,
    /// Its content's opacity (the stacked ones show only their card).
    pub content_opacity: f32,
    pub height: f32,
}

/// `f`: locked, the stacked ones are offset 4 px per place (`20 * index / 5`); unlocked, they sit at their own top.
fn stack_offset(index: usize, locked: bool, top: f32) -> f32 {
    if locked && index != 0 {
        20. * (index as f32 / 5.).max(0.)
    } else {
        top
    }
}

/// The pose of the notification at `index`, `top` px down the list (the heights before it plus 8 px each), its own
/// height and the first one's.
pub fn stack_pose(index: usize, locked: bool, top: f32, height: f32, first_height: f32) -> Pose {
    let step = (1. - index as f32 / 4.).min(1.);
    Pose {
        opacity: if locked && index > 4 {
            0.
        } else if locked {
            step
        } else {
            1.
        },
        scale: if locked { step } else { 1. },
        offset: stack_offset(index, locked, top),
        content_opacity: if locked && index > 0 { 0. } else { 1. },
        height: if locked { first_height } else { height },
    }
}

/// The pose a new one comes in from: transparent, 10% larger and its own height above where it rests.
pub fn enter_pose(locked: bool, height: f32, first_height: f32) -> Pose {
    let own = if locked { first_height } else { height };
    Pose {
        opacity: 0.,
        scale: 1.1,
        offset: -own,
        content_opacity: 1.,
        height: own,
    }
}

/// The pose a dismissed one goes to: transparent, 80% and pushed down (by half its height when unlocked).
pub fn leave_pose(index: usize, locked: bool, top: f32, height: f32) -> Pose {
    Pose {
        opacity: 0.,
        scale: 0.8,
        offset: stack_offset(index, locked, top) + if locked { 0. } else { height / 2. },
        content_opacity: 1.,
        height,
    }
}

/// The scale the stacked ones are drawn at (`n.to([0, 1], [0.7, 1])`): the spring's 0..1 mapped onto 0.7..1.
pub fn drawn_scale(scale: f32) -> f32 {
    0.7 + scale * 0.3
}

/// The `top` of each listed notification from their heights: one after the other, 8 px apart (`computeLayout`).
pub fn tops(heights: &[f32]) -> Vec<f32> {
    let mut top = 0.;
    heights
        .iter()
        .map(|height| {
            let at = top;
            top += height + 8.;
            at
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(channel: u64) -> Options {
        Options {
            kind: Kind::Text,
            externally_managed: true,
            channel: Some(channel),
            ..Options::default()
        }
    }

    #[test]
    fn higher_priorities_go_first_and_newer_before_equal_ones() {
        let mut queue = Queue::default();
        let a = queue.push("a", Options::default(), 0.);
        let call = queue.push(
            "call",
            Options {
                priority: Priority::High,
                kind: Kind::IncomingCall,
                externally_managed: true,
                channel: Some(9),
                ..Options::default()
            },
            1.,
        );
        let b = queue.push("b", Options::default(), 2.);
        let order: Vec<u64> = queue.entries().iter().map(|entry| entry.id).collect();
        assert_eq!(order, vec![call, b, a]);
        assert_eq!(queue.call_of(9), Some(call));
        for index in 0..50 {
            queue.push("x", Options::default(), 3. + index as f64);
        }
        assert_eq!(queue.entries().len(), MAX);
    }

    #[test]
    fn plain_ones_go_and_text_ones_time_out() {
        let mut queue = Queue::default();
        let plain = queue.push("plain", Options::default(), 0.);
        let message = queue.push("message", text(1), 0.);
        assert_eq!(queue.next_deadline(), Some(DURATION));
        assert!(!queue.tick(4999.));
        assert!(queue.tick(5000.));
        assert!(queue.entries().iter().all(|entry| entry.id != plain));
        let kept = queue
            .entries()
            .iter()
            .find(|entry| entry.id == message)
            .unwrap();
        assert_eq!(kept.status, Status::TimedOut);
        // Locked, a timed-out one is not listed; unlocked it is.
        assert!(queue.visible(true).is_empty());
        assert_eq!(queue.visible(false).len(), 1);
        // The nudge lasts eight seconds.
        let nudge = queue.push(
            "nudge",
            Options {
                priority: Priority::Urgent,
                kind: Kind::Nudge,
                duration: NUDGE_DURATION,
                ..Options::default()
            },
            10_000.,
        );
        queue.tick(17_999.);
        assert!(queue.entries().iter().any(|entry| entry.id == nudge));
        queue.tick(18_000.);
        assert!(queue.entries().iter().all(|entry| entry.id != nudge));
    }

    #[test]
    fn unlocking_drops_nudges_and_focuses_the_newest_message() {
        let mut queue = Queue::default();
        queue.push(
            "nudge",
            Options {
                priority: Priority::Urgent,
                kind: Kind::Nudge,
                duration: NUDGE_DURATION,
                ..Options::default()
            },
            0.,
        );
        let older = queue.push("older", text(1), 10.);
        let newer = queue.push("newer", text(2), 20.);
        queue.set_locked(false, 100.);
        assert!(
            queue
                .entries()
                .iter()
                .all(|entry| entry.options.kind != Kind::Nudge)
        );
        assert_eq!(queue.entries()[0].id, newer);
        assert_eq!(queue.entries()[0].status, Status::Focused);
        // While one is focused new ones wait aside and no timer fires.
        let waiting = queue.push("waiting", Options::default(), 200.);
        assert!(queue.entries().iter().all(|entry| entry.id != waiting));
        // Locking again puts it back and lets the waiting one in.
        queue.set_locked(true, 300.);
        assert_eq!(queue.entries()[0].status, Status::Active);
        assert!(queue.entries().iter().any(|entry| entry.id == waiting));
        assert!(queue.entries().iter().any(|entry| entry.id == older));
    }

    #[test]
    fn tidying_keeps_the_newest_three_timed_out_messages_for_30_seconds() {
        let mut queue = Queue::default();
        for index in 0..5 {
            queue.push(index, text(index), index as f64 * 1000.);
        }
        queue.tick(10_000.);
        assert_eq!(queue.entries().len(), 5);
        queue.tidy_text(3, 10_000.);
        let left: Vec<u64> = queue.entries().iter().map(|entry| entry.content).collect();
        assert_eq!(left.len(), 3);
        assert!(left.contains(&4) && left.contains(&3) && left.contains(&2));
        queue.tidy_text(3, 40_000.);
        assert!(queue.entries().is_empty());
        // Opening the channel of a message removes it.
        queue.push(7, text(7), 50_000.);
        assert!(queue.channel_selected(7, 50_001.));
        assert!(queue.is_empty());
    }

    #[test]
    fn a_unique_key_keeps_only_the_newest() {
        let mut queue = Queue::default();
        let keyed = |key: &str| Options {
            unique_key: Some(key.to_owned()),
            duration: ACTIVITY_DURATION,
            ..Options::default()
        };
        queue.push("first", keyed("activity-1"), 0.);
        let second = queue.push("second", keyed("activity-1"), 1.);
        queue.push("other", keyed("activity-2"), 2.);
        queue.dedupe(3.);
        let ids: Vec<u64> = queue.entries().iter().map(|entry| entry.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&second));
    }

    #[test]
    fn at_most_five_are_listed() {
        let mut queue = Queue::default();
        for index in 0..8 {
            queue.push(index, Options::default(), 0.);
        }
        let shown = queue.visible(true);
        assert_eq!(shown.len(), SHOWN);
        assert_eq!(shown[4].0, 4);
    }

    #[test]
    fn locked_ones_stack_and_unlocked_ones_list() {
        let first = stack_pose(0, true, 0., 80., 80.);
        assert_eq!(
            first,
            Pose {
                opacity: 1.,
                scale: 1.,
                offset: 0.,
                content_opacity: 1.,
                height: 80.
            }
        );
        let second = stack_pose(1, true, 88., 60., 80.);
        assert_eq!(second.opacity, 0.75);
        assert_eq!(second.offset, 4.);
        assert_eq!(second.content_opacity, 0.);
        assert_eq!(second.height, 80.);
        assert!((drawn_scale(second.scale) - 0.925).abs() < 1e-6);
        assert_eq!(stack_pose(5, true, 0., 1., 1.).opacity, 0.);
        let listed = stack_pose(1, false, 88., 60., 80.);
        assert_eq!(
            (listed.opacity, listed.offset, listed.height),
            (1., 88., 60.)
        );
        assert_eq!(tops(&[80., 60., 40.]), vec![0., 88., 156.]);
        assert_eq!(enter_pose(false, 60., 80.).offset, -60.);
        assert_eq!(leave_pose(1, false, 88., 60.).offset, 118.);
        assert_eq!(leave_pose(1, true, 88., 60.).offset, 4.);
    }
}
