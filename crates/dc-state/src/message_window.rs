//! The loaded window of a channel's messages, ported from `ChannelMessages` (module 755364) and the fetch logic of
//! `MessageActions` (module 138399, `fetchMessages`, `_tryFetchMessagesCached`, `jumpToMessage`, `jumpToPresent`) with
//! the reducers of `MessageStore` (module 596344). The array itself stays in `Stores::messages`; this holds what the
//! official class keeps beside it: `hasMoreBefore`, `hasMoreAfter`, the jump state and the two caches.

use crate::Id;

/// `ChannelMessages.truncate`: a window longer than this is cut down.
pub const TRUNCATE_LIMIT: usize = 120;
/// `ChannelMessages.truncate`: the length a cut leaves (`truncateBottom(60)`, `truncateTop(60)`).
pub const TRUNCATE_KEEP: usize = 60;
/// `class m` `cache`: the most messages one side's cache holds.
pub const CACHE_LIMIT: usize = 150;
/// The first page the app asks for when it opens a channel.
pub const INITIAL_PAGE: u32 = 50;
/// `jumpToMessage`, `focusMessage`: the page an `around` fetch asks for.
pub const JUMP_PAGE: u32 = 30;
/// `ScrollManager.loadMore` caps its page at this many messages.
pub const LOAD_MORE_CAP: u32 = 30;
/// Module 811091 `extractTimestamp`: the Discord epoch in milliseconds.
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// The milliseconds since the Unix epoch a snowflake was made at (`extractTimestamp`).
pub fn extract_timestamp(id: Id) -> u64 {
    (id >> 22) + DISCORD_EPOCH_MS
}

/// What the window needs of a message.
pub trait Windowed: Clone {
    fn message_id(&self) -> Id;
}

impl Windowed for crate::Message {
    fn message_id(&self) -> Id {
        self.id
    }
}

/// Module 326337 (`lX.h`): how many messages fit the window, for a page. `window_height` is the window's height.
pub fn page_size(window_height: f32, compact: bool, font_size: f32) -> u32 {
    let height = (window_height - 145.).max(400.);
    let row = if compact { 65. } else { 91. } * (font_size / 16.);
    let rows = (height / row).ceil();
    (1.05 * rows).ceil().clamp(10., 30.) as u32
}

/// `scrollManager.loadMore`: `Math.min(30, 2 * lX.h(...))`.
pub fn load_more_limit(page: u32) -> u32 {
    LOAD_MORE_CAP.min(2 * page)
}

/// One side's cache (`class m`): messages cut off the window, kept to be put back without a fetch.
#[derive(Clone, Debug)]
pub struct Cache<T> {
    messages: Vec<T>,
    was_at_edge: bool,
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            was_at_edge: false,
        }
    }
}

impl<T: Windowed> Cache<T> {
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn was_at_edge(&self) -> bool {
        self.was_at_edge
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.was_at_edge = false;
    }

    pub fn has(&self, id: Id) -> bool {
        self.messages.iter().any(|known| known.message_id() == id)
    }

    /// `cache(e, t)`. `before` caches hold the older side, so new messages go after the ones kept.
    fn cache(&mut self, before: bool, new: Vec<T>, at_edge: bool) {
        if self.messages.is_empty() {
            self.was_at_edge = at_edge;
        }
        if self.messages.len() + new.len() > CACHE_LIMIT {
            self.was_at_edge = false;
            if new.len() > CACHE_LIMIT {
                self.messages = if before {
                    new[new.len() - CACHE_LIMIT..].to_vec()
                } else {
                    new[..CACHE_LIMIT].to_vec()
                };
                return;
            }
            let room = CACHE_LIMIT - new.len();
            self.messages = if before {
                self.messages[self.messages.len().saturating_sub(room)..].to_vec()
            } else {
                self.messages[..room.min(self.messages.len())].to_vec()
            };
        }
        if before {
            self.messages.extend(new);
        } else {
            let mut joined = new;
            joined.append(&mut self.messages);
            self.messages = joined;
        }
    }

    fn extract_all(&mut self) -> Vec<T> {
        std::mem::take(&mut self.messages)
    }

    /// `extract(n)`: the `count` messages nearest the window.
    fn extract(&mut self, before: bool, count: usize) -> Vec<T> {
        if before {
            let start = self.messages.len().saturating_sub(count);
            self.messages.split_off(start)
        } else {
            let end = count.min(self.messages.len());
            self.messages.drain(..end).collect()
        }
    }
}

/// `jump` in the fetch actions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Jump {
    pub message_id: Option<Id>,
    pub present: bool,
    pub flash: bool,
    pub offset: Option<i32>,
    pub return_message_id: Option<Id>,
}

impl Jump {
    pub fn to_message(message: Id, flash: bool, return_message_id: Option<Id>) -> Self {
        Self {
            message_id: Some(message),
            flash,
            return_message_id,
            ..Self::default()
        }
    }

    pub fn to_present() -> Self {
        Self {
            present: true,
            ..Self::default()
        }
    }
}

/// What `fetchMessages` was asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FetchSpec {
    pub channel: Id,
    pub before: Option<Id>,
    pub after: Option<Id>,
    pub limit: u32,
    pub jump: Option<Jump>,
    /// `truncate: true` on scroll loads and reloads.
    pub truncate: bool,
    /// The channel's newest message (`lastMessageId`), to tell when an `around` page reaches the present.
    pub last_message_id: Option<Id>,
    /// When the request started, in Unix milliseconds (`requestStartTime`).
    pub started_ms: u64,
}

impl FetchSpec {
    /// `GET /channels/{id}/messages` with `before`, `after`, `limit` and `around` (`ENDPOINTS.md`).
    pub fn route(&self) -> String {
        let mut parts = Vec::new();
        if let Some(before) = self.before {
            parts.push(format!("before={before}"));
        }
        if let Some(after) = self.after {
            parts.push(format!("after={after}"));
        }
        parts.push(format!("limit={}", self.limit));
        if let Some(around) = self.jump.as_ref().and_then(|jump| jump.message_id) {
            parts.push(format!("around={around}"));
        }
        format!("/channels/{}/messages?{}", self.channel, parts.join("&"))
    }

    /// `isBefore`.
    pub fn is_before(&self) -> bool {
        self.before.is_some()
    }

    /// `isAfter`.
    pub fn is_after(&self) -> bool {
        self.after.is_some()
    }
}

/// The `hasMoreBefore` and `hasMoreAfter` `fetchMessages` hands to `LOAD_MESSAGES_SUCCESS`, from the ids the page
/// came back with (newest first, as the route returns them).
pub fn has_more(spec: &FetchSpec, ids: &[Id]) -> (bool, bool) {
    let before = spec.before.is_some();
    let after = spec.after.is_some();
    let plain = !before && !after;
    let around = spec.jump.as_ref().and_then(|jump| jump.message_id);
    let full = ids.len() == spec.limit as usize;
    let mut more_before = around.is_some() || (full && (before || plain));
    let mut more_after = around.is_some() || (after && full);
    if let Some(target) = around {
        let half = (spec.limit / 2) as usize;
        let upper = half + (spec.limit % 2) as usize;
        let mut sorted: Vec<Id> = std::iter::once(target).chain(ids.iter().copied()).collect();
        sorted.sort_unstable();
        sorted.dedup();
        let at = sorted.iter().position(|id| *id == target).unwrap_or(0);
        if at + 1 < upper {
            more_before = false;
        }
        if ids.len().saturating_sub(at) < half {
            more_after = false;
        }
        if more_after
            && let (Some(newest), Some(last)) = (ids.first(), spec.last_message_id)
            && *newest == last
        {
            more_after = false;
        }
    }
    (more_before, more_after)
}

/// What `loadComplete` is given besides the messages.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub is_before: bool,
    pub is_after: bool,
    pub before: Option<Id>,
    pub after: Option<Id>,
    pub limit: Option<usize>,
    pub jump: Option<Jump>,
    pub has_more_before: bool,
    pub has_more_after: bool,
    pub avoid_initial_scroll: bool,
    pub request_start_ms: Option<u64>,
}

/// The per-channel state beside the message array (`ChannelMessages` fields).
#[derive(Clone, Debug)]
pub struct ChannelWindow<T = crate::Message> {
    pub ready: bool,
    pub has_more_before: bool,
    pub has_more_after: bool,
    pub loading_more: bool,
    pub has_fetched: bool,
    pub error: bool,
    pub jumped: bool,
    pub jumped_to_present: bool,
    pub jump_target_id: Option<Id>,
    pub jump_target_offset: i32,
    pub jump_flash: bool,
    pub jump_return_target_id: Option<Id>,
    pub jump_sequence_id: u64,
    /// `isAtBottom` of the scroller store, set by the view.
    pub at_bottom: bool,
    before: Cache<T>,
    after: Cache<T>,
}

impl<T> Default for ChannelWindow<T> {
    fn default() -> Self {
        Self {
            ready: false,
            has_more_before: true,
            has_more_after: false,
            loading_more: false,
            has_fetched: false,
            error: false,
            jumped: false,
            jumped_to_present: false,
            jump_target_id: None,
            jump_target_offset: 0,
            jump_flash: true,
            jump_return_target_id: None,
            jump_sequence_id: 1,
            at_bottom: false,
            before: Cache {
                messages: Vec::new(),
                was_at_edge: false,
            },
            after: Cache {
                messages: Vec::new(),
                was_at_edge: false,
            },
        }
    }
}

fn position<T: Windowed>(array: &[T], id: Id) -> Option<usize> {
    array.iter().position(|known| known.message_id() == id)
}

impl<T: Windowed> ChannelWindow<T> {
    pub fn before_cache(&self) -> &Cache<T> {
        &self.before
    }

    pub fn after_cache(&self) -> &Cache<T> {
        &self.after
    }

    /// `has(id, includeCache = false)` on the array.
    pub fn has(array: &[T], id: Id) -> bool {
        position(array, id).is_some()
    }

    /// `hasPresent`: the window reaches the newest message, or the after cache does.
    pub fn has_present(&self) -> bool {
        (!self.after.is_empty() && self.after.was_at_edge) || !self.has_more_after
    }

    /// `loadStart`.
    pub fn load_start(&mut self, jump: Option<&Jump>) {
        self.loading_more = true;
        self.jumped = jump.is_some();
        self.jumped_to_present = jump.is_some_and(|jump| jump.present);
        self.jump_target_id = jump.and_then(|jump| jump.message_id);
        self.jump_target_offset = jump.and_then(|jump| jump.offset).unwrap_or(0);
        self.jump_return_target_id = jump.and_then(|jump| jump.return_message_id);
        if jump.is_some() {
            self.ready = false;
        }
    }

    /// `LOAD_MESSAGES_FAILURE`.
    pub fn load_failed(&mut self) {
        self.loading_more = false;
        self.error = true;
    }

    /// `ChannelMessages.truncateTop(keep)`: the oldest messages go to the before cache.
    pub fn truncate_top(&mut self, array: &mut Vec<T>, keep: usize) {
        if array.len() <= keep {
            return;
        }
        let cut = array.len() - keep;
        let removed: Vec<T> = array.drain(..cut).collect();
        let at_edge = !self.has_more_before;
        self.before.cache(true, removed, at_edge);
        self.has_more_before = true;
    }

    /// `ChannelMessages.truncateBottom(keep)`: the newest go to the after cache and the window detaches.
    pub fn truncate_bottom(&mut self, array: &mut Vec<T>, keep: usize) {
        if array.len() <= keep {
            return;
        }
        let removed = array.split_off(keep);
        let at_edge = !self.has_more_after;
        self.after.cache(false, removed, at_edge);
        self.has_more_after = true;
    }

    /// `truncate(bottom, top)`.
    pub fn truncate(&mut self, array: &mut Vec<T>, bottom: bool, top: bool) {
        if array.len() <= TRUNCATE_LIMIT {
            return;
        }
        if bottom {
            self.truncate_bottom(array, TRUNCATE_KEEP);
        } else if top {
            self.truncate_top(array, TRUNCATE_KEEP);
        }
    }

    /// `_merge(e, before, clear)`: `new` is oldest first.
    fn merge(&mut self, array: &mut Vec<T>, new: Vec<T>, before: bool, clear_cache: bool) {
        let mut fresh = Vec::new();
        for message in new {
            match position(array, message.message_id()) {
                Some(at) => array[at] = message,
                None => fresh.push(message),
            }
        }
        if clear_cache {
            if before {
                self.before.clear();
            } else {
                self.after.clear();
            }
        }
        if before {
            fresh.append(array);
            *array = fresh;
        } else {
            array.extend(fresh);
        }
    }

    /// `loadFromCache(before, count)`.
    pub fn load_from_cache(&mut self, array: &mut Vec<T>, before: bool, count: usize) {
        let side = if before {
            &mut self.before
        } else {
            &mut self.after
        };
        let taken = side.extract(before, count);
        let more = !side.is_empty() || !side.was_at_edge;
        self.merge(array, taken, before, false);
        if before {
            self.has_more_before = more;
        } else {
            self.has_more_after = more;
        }
        self.ready = true;
        self.loading_more = false;
    }

    /// `jumpToPresent(limit)` from the caches, when `has_present`.
    pub fn jump_to_present(&mut self, array: &mut Vec<T>, limit: usize) {
        let mut newer = self.after.extract_all();
        self.has_more_after = false;
        let split = newer.len().saturating_sub(limit);
        let keep = newer.split_off(split);
        let window = std::mem::take(array);
        self.before.cache(true, window, false);
        self.before.cache(true, newer, false);
        self.merge(array, keep, false, false);
        self.has_more_before = !self.before.is_empty();
        self.jumped = true;
        self.jump_target_id = None;
        self.jump_target_offset = 0;
        self.jumped_to_present = true;
        self.jump_flash = false;
        self.jump_return_target_id = None;
        self.jump_sequence_id += 1;
        self.ready = true;
        self.loading_more = false;
    }

    /// `jumpToMessage`: the message is already in the window.
    pub fn jump_to_message(&mut self, jump: &Jump) {
        self.jumped = true;
        self.jumped_to_present = false;
        self.jump_target_id = jump.message_id;
        self.jump_target_offset = jump.message_id.and(jump.offset).unwrap_or(0);
        self.jump_sequence_id += 1;
        self.jump_flash = jump.flash;
        self.jump_return_target_id = jump.return_message_id;
        self.ready = true;
        self.loading_more = false;
    }

    /// `loadComplete`. `new` is newest first, as the route returns it.
    pub fn load_complete(&mut self, array: &mut Vec<T>, mut new: Vec<T>, loaded: &Loaded) {
        new.reverse();
        let jump = loaded.jump.as_ref();
        let anchor = if loaded.is_before {
            loaded.before
        } else {
            loaded.after
        };
        if jump.is_none()
            && self.ready
            && loaded.is_before != loaded.is_after
            && let Some(anchor) = anchor
        {
            let edge = if loaded.is_before {
                array.first()
            } else {
                array.last()
            };
            if edge.is_some_and(|edge| edge.message_id() != anchor) {
                // A page for an edge the window no longer has.
                let side_len = if loaded.is_before {
                    self.before.len()
                } else {
                    self.after.len()
                };
                if side_len > 0 {
                    let count = loaded.limit.unwrap_or(new.len());
                    self.load_from_cache(array, loaded.is_before, count);
                } else {
                    self.loading_more = false;
                }
                return;
            }
        }
        if (loaded.is_before || loaded.is_after) && jump.is_none() && self.ready {
            self.merge(array, new, loaded.is_before, true);
        } else {
            // The messages that arrived during the fetch stay, unless the page is around a message.
            let newest = new.last().map(|message| message.message_id());
            let kept: Vec<T> = match (newest, loaded.request_start_ms) {
                (Some(newest), Some(started))
                    if jump.is_none_or(|jump| jump.message_id.is_none()) =>
                {
                    array
                        .iter()
                        .filter(|known| {
                            known.message_id() > newest
                                && extract_timestamp(known.message_id()) >= started
                        })
                        .cloned()
                        .collect()
                }
                _ => Vec::new(),
            };
            *array = new;
            self.before.clear();
            self.after.clear();
            if !kept.is_empty() {
                self.merge(array, kept, false, false);
            }
        }
        self.ready = true;
        self.loading_more = false;
        self.jump_flash = jump.is_some_and(|jump| jump.flash);
        self.jumped = jump.is_some();
        self.jumped_to_present = jump.is_some_and(|jump| jump.present);
        self.jump_target_id = jump.and_then(|jump| jump.message_id);
        self.jump_target_offset = match jump {
            Some(Jump {
                message_id: Some(_),
                offset: Some(offset),
                ..
            }) => *offset,
            _ => 0,
        };
        if jump.is_some() && !loaded.avoid_initial_scroll {
            self.jump_sequence_id += 1;
        }
        self.jump_return_target_id = jump.and_then(|jump| jump.return_message_id);
        if !(jump.is_none() && loaded.is_after) {
            self.has_more_before = loaded.has_more_before;
        }
        if !(jump.is_none() && loaded.is_before) {
            self.has_more_after = loaded.has_more_after;
        }
        self.has_fetched = true;
        self.error = false;
    }

    /// `receiveMessage` for a message of someone else. While the window is detached nothing is added (it only
    /// counts as unread elsewhere). Returns whether the message went into the array.
    pub fn receive_message(&mut self, array: &mut Vec<T>, message: T) -> bool {
        if self.has_more_after {
            self.after.was_at_edge = false;
            return false;
        }
        match position(array, message.message_id()) {
            Some(at) => array[at] = message,
            None => {
                let at = array.partition_point(|known| known.message_id() < message.message_id());
                array.insert(at, message);
            }
        }
        if self.at_bottom {
            self.truncate_top(array, TRUNCATE_KEEP);
        } else if array.len() > TRUNCATE_LIMIT {
            self.truncate_bottom(array, TRUNCATE_KEEP);
        }
        true
    }

    /// `remove`: a deleted message leaves the array and both caches.
    pub fn remove(&mut self, array: &mut Vec<T>, id: Id) {
        array.retain(|known| known.message_id() != id);
        self.before
            .messages
            .retain(|known| known.message_id() != id);
        self.after.messages.retain(|known| known.message_id() != id);
    }

    /// Where a jump to `message` has to go (`_tryFetchMessagesCached` for `jump.messageId`).
    pub fn jump_plan(&self, array: &[T], message: Id) -> JumpPlan {
        if Self::has(array, message) {
            return JumpPlan::InWindow;
        }
        if !self.ready {
            return JumpPlan::Fetch;
        }
        let stamp = extract_timestamp(message);
        let first = array
            .first()
            .map(|known| extract_timestamp(known.message_id()));
        let last = array
            .last()
            .map(|known| extract_timestamp(known.message_id()));
        let inside =
            matches!((first, last), (Some(first), Some(last)) if first < stamp && last > stamp);
        let at_start = !self.has_more_before && first.is_some_and(|first| first >= stamp);
        let at_end = !self.has_more_after && last.is_some_and(|last| last <= stamp);
        if at_start || at_end || inside {
            JumpPlan::InWindow
        } else {
            JumpPlan::Fetch
        }
    }
}

/// The result of [`ChannelWindow::jump_plan`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpPlan {
    /// The loaded window covers the message: scroll to it (`LOAD_MESSAGES_SUCCESS_CACHED` with `jump`).
    InWindow,
    /// Load a page `around` it.
    Fetch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct M(Id);
    impl Windowed for M {
        fn message_id(&self) -> Id {
            self.0
        }
    }

    fn ids(array: &[M]) -> Vec<Id> {
        array.iter().map(|m| m.0).collect()
    }

    fn range(from: u64, to: u64) -> Vec<M> {
        (from..=to).map(M).collect()
    }

    fn spec(before: Option<Id>, after: Option<Id>, limit: u32, around: Option<Id>) -> FetchSpec {
        FetchSpec {
            channel: 1,
            before,
            after,
            limit,
            jump: around.map(|id| Jump::to_message(id, true, None)),
            ..FetchSpec::default()
        }
    }

    fn ready() -> ChannelWindow<M> {
        ChannelWindow {
            ready: true,
            ..ChannelWindow::default()
        }
    }

    #[test]
    fn page_size_follows_module_326337() {
        // 720 high, cozy, 16px: ceil(575 / 91) = 7, 1.05 * 7 rounds up to 8, clamped up to 10.
        assert_eq!(page_size(720., false, 16.), 10);
        assert_eq!(load_more_limit(10), 20);
        assert_eq!(page_size(4000., true, 16.), 30);
        assert_eq!(load_more_limit(30), 30);
    }

    #[test]
    fn the_route_carries_before_after_limit_and_around() {
        assert_eq!(
            spec(Some(9), None, 20, None).route(),
            "/channels/1/messages?before=9&limit=20"
        );
        assert_eq!(
            spec(None, Some(9), 20, None).route(),
            "/channels/1/messages?after=9&limit=20"
        );
        assert_eq!(
            spec(None, None, 30, Some(7)).route(),
            "/channels/1/messages?limit=30&around=7"
        );
    }

    #[test]
    fn has_more_follows_fetch_messages() {
        let all: Vec<Id> = (1..=20).rev().collect();
        assert_eq!(
            has_more(&spec(Some(50), None, 20, None), &all),
            (true, false)
        );
        assert_eq!(
            has_more(&spec(Some(50), None, 20, None), &all[..5]),
            (false, false)
        );
        assert_eq!(
            has_more(&spec(None, Some(0), 20, None), &all),
            (false, true)
        );
        assert_eq!(
            has_more(&spec(None, Some(0), 20, None), &all[..5]),
            (false, false)
        );
        let page: Vec<Id> = (100..130).rev().collect();
        assert_eq!(
            has_more(&spec(None, None, 30, Some(115)), &page),
            (true, true)
        );
        let page: Vec<Id> = (1..=30).rev().collect();
        assert_eq!(
            has_more(&spec(None, None, 30, Some(3)), &page),
            (false, true)
        );
        let page: Vec<Id> = (1..=20).rev().collect();
        assert_eq!(
            has_more(&spec(None, None, 30, Some(19)), &page),
            (true, false)
        );
        let mut around = spec(None, None, 30, Some(115));
        around.last_message_id = Some(129);
        assert_eq!(
            has_more(&around, &(100..130).rev().collect::<Vec<_>>()),
            (true, false)
        );
    }

    #[test]
    fn an_around_load_replaces_the_window_and_detaches_it() {
        let mut window = ready();
        let mut array = range(900, 949);
        let load = Loaded {
            jump: Some(Jump::to_message(115, true, None)),
            has_more_before: true,
            has_more_after: true,
            ..Loaded::default()
        };
        let sequence = window.jump_sequence_id;
        window.load_complete(&mut array, (100..130).rev().map(M).collect(), &load);
        assert_eq!(ids(&array), (100..130).collect::<Vec<_>>());
        assert!(window.has_more_before && window.has_more_after);
        assert_eq!(window.jump_target_id, Some(115));
        assert!(window.jump_flash && window.jumped);
        assert_eq!(window.jump_sequence_id, sequence + 1);
    }

    #[test]
    fn an_after_load_merges_and_a_short_page_attaches_the_window_again() {
        let mut window = ready();
        let mut array = range(100, 129);
        window.has_more_after = true;
        let load = Loaded {
            is_after: true,
            after: Some(129),
            limit: Some(20),
            has_more_after: false,
            ..Loaded::default()
        };
        window.load_complete(&mut array, (130..=134).rev().map(M).collect(), &load);
        assert_eq!(array.last(), Some(&M(134)));
        assert!(!window.has_more_after);
        assert!(
            window.has_more_before,
            "an after page leaves has_more_before"
        );
    }

    #[test]
    fn a_before_load_prepends_and_keeps_has_more_after() {
        let mut window = ready();
        let mut array = range(100, 129);
        window.has_more_after = true;
        let load = Loaded {
            is_before: true,
            before: Some(100),
            limit: Some(20),
            has_more_before: false,
            has_more_after: false,
            ..Loaded::default()
        };
        window.load_complete(&mut array, (80..100).rev().map(M).collect(), &load);
        assert_eq!(array.first(), Some(&M(80)));
        assert!(!window.has_more_before);
        assert!(
            window.has_more_after,
            "a before page cannot attach the window"
        );
    }

    #[test]
    fn a_stale_page_is_dropped() {
        let mut window = ready();
        window.loading_more = true;
        let mut array = range(100, 129);
        let load = Loaded {
            is_before: true,
            before: Some(90),
            limit: Some(20),
            has_more_before: true,
            ..Loaded::default()
        };
        window.load_complete(&mut array, (70..90).rev().map(M).collect(), &load);
        assert_eq!(array.len(), 30);
        assert!(!window.loading_more);
    }

    #[test]
    fn truncating_the_far_end_keeps_60_and_caches_the_rest() {
        let mut window = ready();
        window.has_more_before = false;
        let mut array = range(1, 130);
        window.truncate(&mut array, false, true);
        assert_eq!(ids(&array), (71..=130).collect::<Vec<_>>());
        assert!(window.has_more_before);
        assert_eq!(window.before_cache().len(), 70);
        assert!(window.before_cache().was_at_edge());

        let mut window = ready();
        let mut array = range(1, 130);
        window.truncate(&mut array, true, false);
        assert_eq!(ids(&array), (1..=60).collect::<Vec<_>>());
        assert!(window.has_more_after);
        assert_eq!(window.after_cache().len(), 70);
        let mut array = range(1, 120);
        window.truncate(&mut array, true, false);
        assert_eq!(array.len(), 120);
    }

    #[test]
    fn the_cache_holds_at_most_150() {
        let mut cache = Cache::<M>::default();
        cache.cache(true, range(1, 100), true);
        cache.cache(true, range(101, 200), false);
        assert_eq!(cache.len(), 150);
        assert!(!cache.was_at_edge());
        assert_eq!(cache.messages.first(), Some(&M(51)));
        assert_eq!(cache.messages.last(), Some(&M(200)));
    }

    #[test]
    fn a_detached_window_does_not_take_new_messages() {
        let mut window = ready();
        window.has_more_after = true;
        let mut array = range(1, 30);
        assert!(!window.receive_message(&mut array, M(500)));
        assert_eq!(array.len(), 30);
        window.has_more_after = false;
        assert!(window.receive_message(&mut array, M(500)));
        assert_eq!(array.last(), Some(&M(500)));
    }

    #[test]
    fn a_message_at_the_bottom_cuts_the_top_and_one_scrolled_up_cuts_the_bottom() {
        let mut window = ready();
        window.at_bottom = true;
        let mut array = range(1, 100);
        window.receive_message(&mut array, M(200));
        assert_eq!(array.len(), 60);
        assert_eq!(array.last(), Some(&M(200)));
        assert!(window.has_more_before);

        let mut window = ready();
        let mut array = range(1, 120);
        window.receive_message(&mut array, M(200));
        assert_eq!(array.len(), 60);
        assert!(window.has_more_after);
        assert_eq!(array.last(), Some(&M(60)));
    }

    #[test]
    fn jump_to_present_from_the_cache_keeps_the_newest_page() {
        let mut window = ready();
        let mut array = range(1, 130);
        window.truncate(&mut array, true, false);
        assert!(window.has_more_after);
        // The cut was made while the window reached the present: the cache is at the edge.
        assert!(window.has_present());
        window.jump_to_present(&mut array, 30);
        assert_eq!(ids(&array), (91..=130).collect::<Vec<_>>().split_off(10));
        assert!(!window.has_more_after);
        assert!(window.has_more_before);
        assert!(window.jumped_to_present && !window.jump_flash);
        assert_eq!(window.jump_target_id, None);
    }

    #[test]
    fn jump_plan_uses_the_window_when_it_covers_the_message() {
        let mut window = ready();
        window.has_more_after = true;
        let stamp = |n: u64| n << 32;
        let array: Vec<M> = (10..20).map(|n| M(stamp(n))).collect();
        assert_eq!(window.jump_plan(&array, stamp(12)), JumpPlan::InWindow);
        assert_eq!(window.jump_plan(&array, stamp(12) + 5), JumpPlan::InWindow);
        assert_eq!(window.jump_plan(&array, stamp(3)), JumpPlan::Fetch);
        assert_eq!(window.jump_plan(&array, stamp(40)), JumpPlan::Fetch);
        window.has_more_after = false;
        assert_eq!(window.jump_plan(&array, stamp(40)), JumpPlan::InWindow);
    }

    #[test]
    fn messages_that_arrived_during_a_present_fetch_stay() {
        let mut window = ready();
        let sent = 5_000_001u64 << 22;
        let started = extract_timestamp(sent) - 10;
        let mut array = vec![M(100), M(sent)];
        let load = Loaded {
            request_start_ms: Some(started),
            jump: Some(Jump::to_present()),
            has_more_before: true,
            ..Loaded::default()
        };
        window.load_complete(&mut array, vec![M(102), M(101)], &load);
        assert_eq!(ids(&array), vec![101, 102, sent]);
    }

    #[test]
    fn deleting_removes_from_the_caches_too() {
        let mut window = ready();
        let mut array = range(1, 130);
        window.truncate(&mut array, false, true);
        window.remove(&mut array, 5);
        assert_eq!(window.before_cache().len(), 69);
        assert!(!window.before_cache().has(5));
    }
}
