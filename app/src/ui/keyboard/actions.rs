//! What each shortcut does to the window. Every action calls the function the same control calls with the mouse, so a
//! key and a click cannot drift apart.

use super::nav::{self, HOME, Place, ServerTarget};
use super::{Action, Hit};
use crate::ui::{Root, picker};
use dc_state::{ChannelKind, Id};
use gpui::{ClipboardItem, Context, Window, point, px};
use std::time::{Duration, Instant};

/// `throttle(..., 50)` on the server step (module 194482).
const SERVER_STEP_GAP: Duration = Duration::from_millis(50);

/// One stop of the Home channel step: the fixed pages first, then the direct messages (module 980372 `I()` and
/// `getPrivateChannelIds`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HomeStop {
    Friends,
    Nitro,
    Shop,
    Quests,
    Dm(Id),
}

impl Root {
    /// Runs the action a key hit. True when the key is used up.
    pub(super) fn run_shortcut(
        &mut self,
        hit: Hit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        tracing::debug!(
            action = hit.entry.name,
            bind = hit.bind,
            module = hit.entry.source,
            "shortcut"
        );
        match hit.entry.action {
            Action::ServerNext => self.step_server(1, cx),
            Action::ServerPrev => self.step_server(-1, cx),
            Action::ChannelNext => self.step_channel(1, cx),
            Action::ChannelPrev => self.step_channel(-1, cx),
            Action::NavigateBack => self.go_back(cx),
            Action::NavigateForward => self.go_forward(cx),
            Action::UnreadNext => self.step_unread(1, false, cx),
            Action::UnreadPrev => self.step_unread(-1, false, cx),
            Action::MentionChannelNext => self.step_unread(1, true, cx),
            Action::MentionChannelPrev => self.step_unread(-1, true, cx),
            Action::TogglePreviousGuild => self.toggle_previous_guild(cx),
            Action::JumpToGuild => {
                let digit = hit
                    .bind
                    .rsplit('+')
                    .next()
                    .and_then(|digit| digit.parse::<usize>().ok());
                if let Some(digit) = digit
                    && !self.jump_to_guild(digit, cx)
                {
                    return false;
                }
            }
            Action::MarkChannelRead => {
                self.escape_chain(window, cx);
            }
            Action::MarkServerRead => self.mark_server_read(cx),
            Action::ToggleChannelPins => {
                let anchor = if self.current_dm().is_some() {
                    "dm-pins"
                } else {
                    "pins"
                };
                self.toggle_pins(anchor, cx);
            }
            Action::ToggleInbox => self.toggle_inbox(cx),
            Action::MarkTopInboxChannelRead => self.mark_top_inbox_channel_read(cx),
            Action::ToggleUsers => self.toggle_users(cx),
            Action::ToggleHelp => cx.open_url("https://support.discord.com"),
            Action::ToggleMute => self.toggle_self_mute(cx),
            Action::ToggleDeafen => self.toggle_self_deaf(cx),
            Action::ToggleCategoryCollapsed => self.toggle_all_categories(cx),
            Action::SearchSoundboard => self.toggle_soundboard(window, cx),
            Action::ScrollUp | Action::ScrollDown => {
                let page = f32::from(self.list.viewport_bounds().size.height) * 0.9;
                let page = if hit.entry.action == Action::ScrollUp {
                    -page
                } else {
                    page
                };
                self.list.scroll_by(px(page));
                cx.notify();
            }
            Action::QuickSwitcherShow | Action::OpenChannelTab => {
                if self.quick_switcher.is_none() {
                    self.open_quick_switcher(window, cx);
                }
            }
            Action::CreateDmGroup => {
                if self.guild.is_some() {
                    self.select_home(None);
                }
                self.open_group_dm_picker(window, cx);
            }
            Action::CreateThread => {
                if self
                    .current_channel()
                    .is_some_and(|channel| channel.kind == ChannelKind::Text)
                {
                    self.open_new_thread(window, cx);
                }
            }
            Action::SearchEmojis | Action::SearchGifs | Action::SearchStickers => {
                self.toggle_expression_picker(hit.entry.action, window, cx);
            }
            Action::JumpToFirstUnread => self.jump_to_first_unread(),
            Action::CreateGuild => self.open_add_server(window, cx),
            Action::UploadFile => {
                if self.composer_reachable() {
                    self.pick_files(cx);
                }
            }
            Action::ReturnToAudioChannel | Action::JumpToCurrentCall => self.go_to_call(cx),
            Action::FocusSearch => {
                if self.composer_reachable() {
                    self.focus_search(window, cx);
                }
            }
            Action::BrowserDevtools => {}
            Action::CopyChannelLink => self.copy_channel_link(cx),
            Action::OpenSettings => {
                if self.settings.is_none() {
                    self.open_settings(cx);
                }
            }
            Action::ToggleFullscreen => window.toggle_fullscreen(),
            // No picture of the Keyboard Shortcuts help exists to build it from, there is no scaling of the whole
            // interface and no direct message call UI, so those keys are left alone. Shift+F10 opens the menu of the
            // focused element (`context_key`).
            Action::ToggleHotkeys => self.toggle_keyboard_help(cx),
            Action::OpenContextMenu => return self.open_focused_context_menu(window, cx),
            Action::ZoomIn
            | Action::ZoomOut
            | Action::ZoomReset
            | Action::CallAccept
            | Action::CallStart => {
                return false;
            }
            Action::VibeWithWumpus
            | Action::CloseChannelTab
            | Action::ToggleChannelTabPin
            | Action::RecordVoiceMessage
            | Action::OpenAppDirectory => return false,
        }
        true
    }

    fn guild_ids(&self) -> Vec<Id> {
        self.stores.guilds.iter().map(|guild| guild.id).collect()
    }

    fn guild_of(&self, channel: Id) -> Option<Id> {
        self.stores.guilds.iter().find_map(|guild| {
            (guild.channels.iter().any(|known| known.id == channel)
                || guild.threads.iter().any(|known| known.id == channel))
            .then_some(guild.id)
        })
    }

    /// The channel last open in a server (`SelectedChannelStore.getChannelId(guild)`), else its first text channel.
    pub(in crate::ui) fn last_channel_of(&self, guild: Id) -> Option<Id> {
        let known = self.stores.guild(guild)?;
        self.channel_history
            .iter()
            .copied()
            .find(|channel| self.guild_of(*channel) == Some(guild))
            // From an earlier run: only a channel the server still has.
            .or_else(|| {
                crate::ui::last_channel::channel_of(guild)
                    .filter(|channel| self.guild_of(*channel) == Some(guild))
            })
            .or_else(|| {
                known
                    .sidebar_rows()
                    .iter()
                    .find(|row| row.kind == ChannelKind::Text)
                    .map(|row| row.id)
            })
    }

    pub(in crate::ui) fn go_guild(&mut self, guild: Id, cx: &mut Context<Self>) {
        let channel = self.last_channel_of(guild);
        self.select(guild, channel);
        self.request_images();
        cx.notify();
    }

    /// Home: the direct message last open, else the friends page (`getHomeLink`).
    fn go_home(&mut self, cx: &mut Context<Self>) {
        let dm = self.channel_history.iter().copied().find(|channel| {
            self.stores
                .private_channels
                .iter()
                .any(|known| known.id == *channel)
        });
        self.select_home(dm);
        self.request_images();
        cx.notify();
    }

    fn step_server(&mut self, delta: i32, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self
            .keys
            .last_step
            .is_some_and(|last| now.duration_since(last) < SERVER_STEP_GAP)
        {
            return;
        }
        self.keys.last_step = Some(now);
        let here = self.guild.map_or(Place::Home, Place::Guild);
        match nav::server_step(&self.guild_ids(), here, delta) {
            ServerTarget::Home => self.go_home(cx),
            ServerTarget::Guild(guild) => self.go_guild(guild, cx),
        }
    }

    fn toggle_previous_guild(&mut self, cx: &mut Context<Self>) {
        if self.guild.is_some() {
            self.go_home(cx);
        } else if let Some(guild) = self.keys.previous_guild
            && self.stores.guild(guild).is_some()
        {
            self.go_guild(guild, cx);
        }
    }

    /// `mod+1`: Home; `mod+2` to `mod+9`: the first eight servers. False when there is no such server.
    fn jump_to_guild(&mut self, digit: usize, cx: &mut Context<Self>) -> bool {
        if digit == 1 {
            if self.guild.is_none() {
                self.select_home(None);
                cx.notify();
            } else {
                self.go_home(cx);
            }
            return true;
        }
        match self.guild_ids().get(digit - 2).copied() {
            Some(guild) => {
                self.go_guild(guild, cx);
                true
            }
            None => false,
        }
    }

    /// The text channels the sidebar shows, in its order: collapsed categories hide theirs unless the channel is open,
    /// has a mention or holds the call (`forEachShownChannel` with `withCurrentVoiceChannel`, module 310953).
    fn navigable_channels(&self, guild: Id) -> Vec<Id> {
        let Some(known) = self.stores.guild(guild) else {
            return Vec::new();
        };
        let now = chrono::Utc::now();
        known
            .sidebar_rows()
            .into_iter()
            .filter(|row| row.kind != ChannelKind::Category)
            .filter(|row| self.followed_sidebar_row(known, row))
            .filter(|row| {
                !self.hide_muted(guild)
                    || self.sidebar_channel_attention(known, row)
                    || !self.stores.guild_settings.channel_muted(guild, row.id, now)
            })
            .filter(|row| {
                let collapsed = row.parent_id.is_some_and(|category| {
                    self.stores
                        .guild_settings
                        .channel(guild, category)
                        .collapsed
                });
                !collapsed || self.sidebar_channel_attention(known, row)
            })
            .filter(|row| {
                row.kind == ChannelKind::Text
                    || row.is_forum()
                    || self.channel == Some(row.id)
                    || self
                        .call
                        .as_ref()
                        .is_some_and(|call| call.channel == row.id)
            })
            .map(|row| row.id)
            .collect()
    }

    fn home_stops(&self) -> Vec<HomeStop> {
        let mut stops = vec![
            HomeStop::Friends,
            HomeStop::Nitro,
            HomeStop::Shop,
            HomeStop::Quests,
        ];
        stops.extend(
            self.stores
                .private_channels
                .iter()
                .map(|channel| HomeStop::Dm(channel.id)),
        );
        stops
    }

    fn home_here(&self) -> HomeStop {
        if let Some(channel) = self.channel {
            HomeStop::Dm(channel)
        } else if self.nitro_open {
            HomeStop::Nitro
        } else if self.shop_active() {
            HomeStop::Shop
        } else if self.quests_active() {
            HomeStop::Quests
        } else {
            HomeStop::Friends
        }
    }

    fn open_home_stop(&mut self, stop: HomeStop, cx: &mut Context<Self>) {
        match stop {
            HomeStop::Friends => self.select_home(None),
            HomeStop::Nitro => self.open_nitro(cx),
            HomeStop::Shop => self.open_shop(cx),
            HomeStop::Quests => self.open_quests(cx),
            HomeStop::Dm(channel) => self.select_home(Some(channel)),
        }
        self.request_images();
        cx.notify();
    }

    /// `CHANNEL_NEXT` and `CHANNEL_PREV` (module 980372 `p`).
    fn step_channel(&mut self, delta: i32, cx: &mut Context<Self>) {
        match self.guild {
            None => {
                let stops = self.home_stops();
                let here = self.home_here();
                let at = stops
                    .iter()
                    .position(|stop| *stop == here)
                    .map(|at| at as i32);
                let len = stops.len() as i32;
                if len == 0 {
                    return;
                }
                let next = at.unwrap_or(-1) + delta;
                let next = if next < 0 {
                    len - 1
                } else if next >= len {
                    0
                } else {
                    next
                };
                self.open_home_stop(stops[next as usize], cx);
            }
            Some(guild) => {
                let list = self.navigable_channels(guild);
                if let Some(channel) = nav::channel_step(&list, self.channel, delta) {
                    self.select(guild, Some(channel));
                    cx.notify();
                }
            }
        }
    }

    /// The channels of a stop of the unread walk: Home lists the direct messages, a server its navigable channels
    /// plus the voice channels (`withVoiceChannels`).
    fn walk_channels(&self, guild: Id) -> Vec<Id> {
        if guild == HOME {
            return self
                .stores
                .private_channels
                .iter()
                .map(|channel| channel.id)
                .collect();
        }
        let Some(known) = self.stores.guild(guild) else {
            return Vec::new();
        };
        let shown = self.navigable_channels(guild);
        known
            .sidebar_rows()
            .into_iter()
            .filter(|row| shown.contains(&row.id) || row.kind == ChannelKind::Voice)
            .map(|row| row.id)
            .collect()
    }

    fn guild_muted(&self, guild: Id) -> bool {
        self.stores
            .guild_settings
            .guilds
            .get(&guild)
            .is_some_and(|settings| {
                dc_state::guild_settings::mute_active(
                    settings.muted,
                    settings.mute_end,
                    chrono::Utc::now(),
                )
            })
    }

    /// The `y` predicate of module 513291 for unread navigation, and `U` for mention navigation.
    fn unread_hit(&self, guild: Id, channel: Id, mentions_only: bool) -> bool {
        let mentions = self.stores.read_states.mentions(channel);
        if mentions_only {
            return mentions > 0;
        }
        if guild == HOME {
            return mentions > 0;
        }
        let Some(known) = self.stores.guild(guild) else {
            return false;
        };
        let Some(row) = known.channels.iter().find(|row| row.id == channel) else {
            return false;
        };
        if row.kind == ChannelKind::Voice {
            return mentions > 0
                || self
                    .call
                    .as_ref()
                    .is_some_and(|call| call.channel == channel);
        }
        let muted = self
            .stores
            .guild_settings
            .channel_muted(guild, channel, chrono::Utc::now());
        let level = self
            .stores
            .guild_settings
            .channel(guild, channel)
            .message_notifications;
        let level = if level == dc_state::guild_settings::INHERIT {
            self.stores.guild_settings.inherited(guild, row.parent_id)
        } else {
            level
        };
        (!muted || mentions > 0)
            && self.stores.is_unread(channel)
            && level == dc_state::guild_settings::ALL_MESSAGES
    }

    /// `UNREAD_NEXT`, `UNREAD_PREV` and the mention variants.
    fn step_unread(&mut self, delta: i32, mentions_only: bool, cx: &mut Context<Self>) {
        let here = self.guild.unwrap_or(HOME);
        let mut order = vec![HOME];
        order.extend(self.guild_ids());
        let found = nav::unread_step(
            &order,
            &|guild| self.walk_channels(guild),
            (here, self.channel),
            delta,
            &|guild| mentions_only || guild == here || !self.guild_muted(guild),
            &|guild, channel| self.unread_hit(guild, channel, mentions_only),
        );
        let Some((guild, channel)) = found else {
            return;
        };
        let voice = self
            .stores
            .guild(guild)
            .and_then(|known| known.channels.iter().find(|row| row.id == channel))
            .is_some_and(|row| row.kind == ChannelKind::Voice);
        if guild == HOME {
            self.select_home(Some(channel));
        } else if voice {
            // The official client shows the voice channel's chat; here the call view is its page.
            self.select(guild, Some(channel));
        } else {
            self.select(guild, Some(channel));
        }
        self.request_images();
        cx.notify();
    }

    /// `MARK_SERVER_READ` (module 795737): nothing when the server has nothing unread; then the chat jumps to the
    /// newest message.
    fn mark_server_read(&mut self, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else {
            return;
        };
        let Some(known) = self.stores.guild(guild) else {
            return;
        };
        if !self.stores.guild_unread(known) {
            return;
        }
        let channels: Vec<Id> = known.channels.iter().map(|channel| channel.id).collect();
        for channel in channels {
            self.mark_channel_read(channel);
        }
        self.unread_from = None;
        self.list.scroll_to_end();
        cx.notify();
    }

    fn toggle_users(&mut self, cx: &mut Context<Self>) {
        if self.voice_view_open() {
            if let Some(channel) = self.channel
                && !self.vv.chat_open.remove(&channel)
            {
                self.vv.chat_open.insert(channel);
            }
        } else if self.current_dm().is_some() {
            self.show_dm_profile = !self.show_dm_profile;
        } else if self.guild.is_some() {
            self.show_members = !self.show_members;
        }
        cx.notify();
    }

    /// `TOGGLE_CATEGORY_COLLAPSED` (module 151199): collapses every open category, or opens them all when all are
    /// collapsed.
    fn toggle_all_categories(&mut self, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else {
            return;
        };
        let Some(known) = self.stores.guild(guild) else {
            return;
        };
        let categories: Vec<(Id, bool)> = known
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Category)
            .map(|channel| {
                (
                    channel.id,
                    self.stores
                        .guild_settings
                        .channel(guild, channel.id)
                        .collapsed,
                )
            })
            .collect();
        let collapse = categories.iter().any(|(_, collapsed)| !collapsed);
        for (category, collapsed) in categories {
            if collapsed != collapse {
                continue;
            }
            self.toggle_category(guild, category);
        }
        cx.notify();
    }

    /// The emoji, GIF and sticker pickers open from the composer's own buttons, so they open from the same place.
    fn toggle_expression_picker(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.composer_reachable() {
            return;
        }
        let Some(bounds) = self.tip_anchors.borrow().get("composer-box").copied() else {
            return;
        };
        let at = point(bounds.left(), bounds.top() - px(9.));
        match action {
            Action::SearchGifs => self.open_picker_tab(picker::Tab::Gifs, at, window, cx),
            Action::SearchStickers => self.open_picker_tab(picker::Tab::Stickers, at, window, cx),
            _ => self.open_picker(picker::Target::Composer, at, window, cx),
        }
    }

    /// `JUMP_TO_FIRST_UNREAD` (module 314519): the first message after the channel's ack, when it is loaded.
    fn jump_to_first_unread(&mut self) {
        let Some(channel) = self.channel else {
            return;
        };
        if !self.stores.is_unread(channel) {
            return;
        }
        let joined = self
            .current_guild()
            .map_or(0, dc_state::Guild::joined_snowflake);
        let acked = self.stores.read_states.acked(channel, joined);
        let first = self
            .stores
            .messages
            .get(&channel)
            .and_then(|messages| messages.iter().find(|message| message.id > acked))
            .map(|message| message.id);
        if let Some(message) = first {
            self.scroll_to_message(message);
        }
    }

    fn go_to_call(&mut self, cx: &mut Context<Self>) {
        let Some((guild, channel)) = self.call.as_ref().map(|call| (call.guild, call.channel))
        else {
            return;
        };
        if guild == HOME {
            self.select_home(Some(channel));
        } else {
            self.select(guild, Some(channel));
        }
        cx.notify();
    }

    fn copy_channel_link(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        let guild = self.guild_of(channel).unwrap_or_default();
        let place = if guild == HOME {
            "@me".to_owned()
        } else {
            guild.to_string()
        };
        cx.write_to_clipboard(ClipboardItem::new_string(format!(
            "https://discord.com/channels/{place}/{channel}"
        )));
        self.show_success_toast("Link Copied!".to_owned(), cx);
    }

    /// `MARK_TOP_INBOX_CHANNEL_READ`: only with the inbox open on its unread list.
    fn mark_top_inbox_channel_read(&mut self, cx: &mut Context<Self>) {
        if self.inbox != Some(crate::ui::inbox::InboxTab::Unreads) {
            return;
        }
        let Some((_, channel)) = self.unread_channels().into_iter().next() else {
            return;
        };
        let newest = self
            .fetched
            .get(&format!("{}{channel}", crate::ui::inbox::UNREAD_PREFIX))
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(dc_state::Message::from_value)
            .map(|message| message.id)
            .max();
        if let Some(message) = newest {
            self.ack_channel(channel, message);
            cx.notify();
        }
    }
}
