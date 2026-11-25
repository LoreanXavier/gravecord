//! What the open server's member list asks the gateway for: the guild's subscription when a channel is selected
//! (661670 `CHANNEL_SELECT`), the 100-row ranges around the scrolled view (289287 `updateSubscription`, 361610
//! `NJ`), and the whole subscription again on a new session or a resume (661670 `k`). The list the channel reads
//! is its member list id (963307 `D`).

use super::{Root, member_window};
use dc_state::guild_subscriptions::{self, DEFAULT_RANGES, Range};
use dc_state::{Id, member_list};
use gpui::Context;

/// 289287: `updateSubscription` is a lodash `debounce` of 50 ms.
const RANGE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(50);

/// The channel whose member list a view of `channel` shows and subscribes (661670 `x`): the channel itself; for a
/// thread, its parent. The official lists an announcement thread's parent; another thread has a thread member list
/// there, which is not ported (the panel of a thread reads the guild's list), so its parent's list stands in.
fn list_channel(guild: &dc_state::Guild, channel: Id) -> Option<&dc_state::Channel> {
    let parent = guild
        .threads
        .iter()
        .find(|thread| thread.id == channel)
        .map(|thread| thread.parent_id);
    let known = guild
        .channels
        .iter()
        .find(|known| known.id == parent.unwrap_or(channel))?;
    match (parent, known.kind.as_u64()) {
        (None, 10..=12) => known
            .parent_id
            .and_then(|parent| guild.channels.iter().find(|known| known.id == parent)),
        _ => Some(known),
    }
}

impl Root {
    /// The member list id the open channel reads (963307 `D`): "everyone" when no channel is open.
    fn open_list_id(&self) -> Option<(Id, String)> {
        let guild_id = self.guild?;
        let Some(guild) = self.stores.guild(guild_id) else {
            return Some((guild_id, member_list::EVERYONE.to_owned()));
        };
        let Some(channel) = self
            .channel
            .and_then(|channel| list_channel(guild, channel))
        else {
            return Some((guild_id, member_list::EVERYONE.to_owned()));
        };
        Some((guild_id, member_list::list_id(guild, channel, None)))
    }

    /// The guilds a dropped or new connection keeps subscribed (661670 `w`): the open one and the voice call's
    /// (`RTCConnectionStore`, module 495867).
    fn kept_guilds(&self) -> Vec<Id> {
        self.guild
            .into_iter()
            .chain(self.call.as_ref().map(|call| call.guild))
            .filter(|guild| *guild != 0)
            .collect()
    }

    /// Sends the subscription changes not sent yet (661670 `flush` and `GUILD_SUBSCRIPTIONS_FLUSH`, which leaves out
    /// a guild the client no longer has).
    pub(super) fn flush_guild_subscriptions(&mut self) {
        let Some(mut pending) = self.guild_subscriptions.take_pending() else {
            return;
        };
        pending.retain(|guild, _| {
            guild
                .parse::<Id>()
                .is_ok_and(|guild| self.stores.guild(guild).is_some())
        });
        if !pending.is_empty() {
            let _ = self.requests.send(crate::net::Request::Subscribe {
                subscriptions: pending,
            });
        }
    }

    /// 661670 `G` on `CHANNEL_SELECT`: the guild's typing, activities and threads, and the channel's member list with
    /// its first hundred rows when the channel has no ranges yet. The official asks only while the members section is
    /// open, which it is by default; that section's store is not ported (F-0081), so the list is asked for always.
    pub(super) fn subscribe_selected_channel(&mut self, guild: Id, channel: Id) {
        self.guild_subscriptions.subscribe_guild(guild);
        if let Some(list) = self
            .stores
            .guild(guild)
            .and_then(|known| list_channel(known, channel))
            .map(|list| list.id)
        {
            self.guild_subscriptions
                .subscribe_channel(guild, list, &DEFAULT_RANGES, true);
        }
        self.flush_guild_subscriptions();
    }

    /// 661670 `k` on `CONNECTION_OPEN`: a new session keeps the open guild's subscription and sends it whole; the
    /// member lists start over (the store dropped them on `READY`).
    pub(super) fn guild_subscriptions_connection_open(&mut self) {
        let keep = self.kept_guilds();
        let stores = &self.stores;
        if let Some(whole) = self
            .guild_subscriptions
            .connection_open(&keep, |guild| stores.guild(guild).is_some())
        {
            let _ = self.requests.send(crate::net::Request::Subscribe {
                subscriptions: whole,
            });
        }
        // The next look at the member list asks for its ranges again.
        self.member_window.seen = None;
    }

    /// 661670 `k` on `CONNECTION_RESUMED`: every subscribed guild's whole subscription goes again.
    pub(super) fn guild_subscriptions_resumed(&mut self) {
        let stores = &self.stores;
        if let Some(whole) = self
            .guild_subscriptions
            .connection_resumed(|guild| stores.guild(guild).is_some())
        {
            let _ = self.requests.send(crate::net::Request::Subscribe {
                subscriptions: whole,
            });
        }
    }

    /// 661670 `CONNECTION_CLOSED`: the other guilds' subscriptions are dropped without a send.
    pub(super) fn guild_subscriptions_closed(&mut self) {
        let keep = self.kept_guilds();
        self.guild_subscriptions.connection_closed(&keep);
    }

    /// The heights the member list draws its rows at, in order.
    pub(super) fn member_row_heights(&self, list: Option<&member_list::MemberList>) -> Vec<f32> {
        list.map(|list| list.rows.as_slice())
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(index, row)| match row {
                member_list::Row::Member { user, .. }
                    if list.and_then(|list| list.group_at(index)).is_none()
                        && self.has_presence_line(user, self.guild) =>
                {
                    44.
                }
                _ => 42.,
            })
            .collect()
    }

    /// The ranges the member list's scrolled view wants (289287 `getDimensions`, less what sits above the rows as
    /// `getContentFeedAdjustedDimensions` takes the content feed off, then 361610 `NJ`).
    fn member_ranges_in_view(&self) -> Vec<Range> {
        let guild = self.guild;
        let list = guild.and_then(|guild| self.stores.member_lists.get(&guild));
        let heights = self.member_row_heights(list);
        let scroll = &self.member_window.scroll;
        let laid_out = f32::from(scroll.bounds().size.height);
        let view = if laid_out > 0. {
            laid_out
        } else {
            self.viewport.1
        };
        let rows_total: f32 = heights.iter().sum();
        let before = if laid_out > 0. {
            (f32::from(scroll.max_offset().y) + view - rows_total).max(0.)
        } else {
            0.
        };
        let top = -f32::from(scroll.offset().y);
        let y = (top - before).max(0.);
        let height = (view - (before - top).max(0.)).max(0.);
        let first = member_window::official_offset(&heights, y);
        let last = member_window::official_offset(&heights, y + height);
        guild_subscriptions::ranges_for(first, last - first, member_window::OFFICIAL_ROW)
    }

    /// Each member list frame: the open guild shows its channel's list, and a changed scroll position or channel
    /// (289287 `componentDidMount`, `componentDidUpdate`, `handleScroll`) asks for the ranges in view 50 ms after
    /// the last change.
    pub(super) fn member_list_frame(&mut self, cx: &mut Context<Self>) {
        let Some((guild, list)) = self.open_list_id() else {
            return;
        };
        self.stores.member_lists.show(guild, &list);
        let Some(channel) = self.channel.and_then(|channel| {
            self.stores
                .guild(guild)
                .and_then(|known| list_channel(known, channel))
                .map(|list| list.id)
        }) else {
            return;
        };
        if !self.show_members {
            return;
        }
        let scroll = &self.member_window.scroll;
        let seen = (
            guild,
            channel,
            f32::from(scroll.offset().y),
            f32::from(scroll.bounds().size.height),
        );
        if self.member_window.seen == Some(seen) {
            return;
        }
        self.member_window.seen = Some(seen);
        self.member_window.due = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RANGE_DEBOUNCE).await;
            let _ = this.update(cx, |this, _| this.member_ranges_due(guild, channel));
        }));
    }

    /// The debounced `updateSubscription`: 361610 `GUILD_SUBSCRIPTIONS_CHANNEL` with the ranges in view, sent when
    /// they differ from the channel's own.
    fn member_ranges_due(&mut self, guild: Id, channel: Id) {
        self.member_window.due = None;
        if self.guild != Some(guild) || !self.show_members {
            return;
        }
        let ranges = self.member_ranges_in_view();
        if self
            .guild_subscriptions
            .subscribe_channel(guild, channel, &ranges, false)
        {
            self.flush_guild_subscriptions();
        }
    }
}
