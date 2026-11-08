//! First screen: rail, channel list, user panel, header, messages, composer.
//! Geometry and colors are from `design/dom/channel/text-darker-cozy.json`; every color is an official token.

use crate::editor::{Editor, EditorEvent, EditorStyle};
use crate::inline::{BoxStyle, Inline, Piece, SharedSelection, inline};
use crate::net::{Request, Update};
use crate::parity::Parity;
use chrono::Datelike;
use dc_markdown::highlight::highlight;
use dc_markdown::{Block, Kind, Span, parse};
use dc_state::grouping::starts_group;
use dc_state::member_list::Row as MemberRow;
use dc_state::{
    Channel, ChannelKind, Guild, Id, Message, Stores, TYPING_DURATION, User, fit_media,
};
use dc_tokens::Theme;
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable, Font,
    FontFeatures, FontStyle, FontWeight, Hsla, Image, ImageFormat, IntoElement, KeyDownEvent,
    ListAlignment, ListState, MouseButton, MouseDownEvent, Pixels, Render, Rgba, SharedString,
    StyledText, TextRun, WeakEntity, Window, WindowControlArea, div, font, img, list, prelude::*,
    px, rgba, svg,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

mod actions;
mod apps;
mod badge_paths;
mod blocked;
mod dm;
mod dm_profile;
mod embed;
mod files;
mod forward;
mod friends;
mod gifs;
mod group_dm;
mod history;
mod home;
mod inbox;
mod links;
mod menu;
mod modal;
mod notify;
mod picker;
mod pins;
mod plaintext_extensions;
mod profile;
mod quickswitcher;
mod reminder_modal;
mod saved;
mod settings;
mod tooltip;
mod unread;
mod uploads;
mod voice;

use tooltip::{Body, Side, Tip};

const SANS: &str = "gg sans";
/// The CSS family is `gg mono`; the font file calls itself `gg sans mono`.
const MONO: &str = "gg sans mono";

/// The page sets `text-rendering: optimizelegibility`. gpui hands DirectWrite an explicit feature list, so `kern` has to
/// be on that list or text comes out wider than the official client's.
fn kerning() -> FontFeatures {
    FontFeatures(Arc::new(vec![("kern".to_owned(), 1)]))
}

/// The palette every color comes from. "Dark" in the Appearance page is `Darker` here, "Ash" is `Dark` and "Onyx" is
/// `Midnight`, as the token sets are named.
static THEME: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);

pub(crate) fn theme() -> Theme {
    match THEME.load(std::sync::atomic::Ordering::Relaxed) {
        0 => Theme::Dark,
        1 => Theme::Light,
        3 => Theme::Midnight,
        _ => Theme::Darker,
    }
}

pub(crate) fn set_theme(theme: Theme) {
    let value = match theme {
        Theme::Dark => 0,
        Theme::Light => 1,
        Theme::Darker => 2,
        Theme::Midnight => 3,
    };
    THEME.store(value, std::sync::atomic::Ordering::Relaxed);
}

fn color(name: &str) -> Rgba {
    rgba(theme().color(name))
}

/// Selection highlight, measured on the official client. The bundle sets no `::selection`.
const SELECTION: u32 = 0x093da7ff;

fn hsla_from(value: u32) -> Hsla {
    rgba(value).into()
}

fn hsla(name: &str) -> Hsla {
    color(name).into()
}

fn icon(name: &str, size: f32, tint: &str) -> impl IntoElement {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color(tint))
}

enum Row {
    /// The start of a DM's history.
    Welcome,
    Divider(String),
    /// The red line before the first unread message.
    NewDivider,
    Spacer,
    Message {
        index: usize,
        head: bool,
    },
    /// A run of messages from blocked people, folded; `first` is the first one's id.
    Blocked {
        first: Id,
        count: usize,
    },
}

pub struct Root {
    stores: Stores,
    requests: UnboundedSender<Request>,
    guild: Option<Id>,
    channel: Option<Id>,
    rows: Vec<Row>,
    list: ListState,
    composer: Entity<Editor>,
    focus: FocusHandle,
    images: HashMap<String, Arc<Image>>,
    requested: HashSet<String>,
    selection: SharedSelection,
    hovered: Option<Id>,
    /// The message being edited, with its own field. Enter saves, Escape cancels.
    editing: Option<(Id, Entity<Editor>)>,
    /// The message the next send answers.
    replying: Option<Message>,
    menu: Option<menu::OpenMenu>,
    /// The menu that ran the last action, for actions that open beside it.
    last_menu: Option<menu::OpenMenu>,
    confirm_delete: Option<Id>,
    show_members: bool,
    /// The profile beside a DM (`Hide User Profile` in its header); shown until toggled off.
    show_dm_profile: bool,
    friends_tab: friends::FriendsTab,
    inbox: Option<inbox::InboxTab>,
    gifs: Option<gifs::Gifs>,
    shown_blocked: HashSet<Id>,
    shown_blocked_profiles: HashSet<Id>,
    hovered_dm: Option<Id>,
    group_dm: Option<group_dm::GroupDm>,
    add_friend: Option<friends::AddFriend>,
    people_search: Option<friends::PeopleSearch>,
    friend_menu: Option<friends::FriendMenu>,
    confirm_remove_friend: Option<Id>,
    quick_switcher: Option<quickswitcher::QuickSwitcher>,
    notify_open: bool,
    /// The Mute Channel row while its durations show.
    notify_submenu: Option<Bounds<Pixels>>,
    /// The open Pinned Messages popout, by the anchor key of the button that opened it.
    pins_open: Option<&'static str>,
    confirm_unpin: Option<Message>,
    forward: Option<forward::Forward>,
    /// Channels this session opened, newest first: the forward dialog's first suggestions.
    channel_history: Vec<Id>,
    speech: crate::speech::Speech,
    /// The message Speak Message is reading.
    speaking_message: Option<Id>,
    toast: Option<saved::Toast>,
    app_toast: Option<saved::AppToast>,
    /// The open submenu of the message menu: the row it hangs off.
    submenu: Option<menu::Submenu>,
    custom_reminder: Option<reminder_modal::CustomReminder>,
    /// The open channel and the ack its unread messages started after, for the NEW divider.
    unread_from: Option<(Id, Id)>,
    /// The expression picker, open from the composer or for a reaction.
    picker: Option<picker::Picker>,
    /// Emoji picked this session, for the frecency list until the stored settings catch up.
    emoji_used: Vec<(String, u64)>,
    /// The message waiting in the "Pin It. Pin It Good." modal.
    confirm_pin: Option<Message>,
    /// Reactions given this session, for the frecency list until the stored settings catch up.
    reactions_used: Vec<(String, u64)>,
    history: history::History,
    /// The Leaving Discord modal, open while a link waits for confirmation.
    leaving: Option<links::Leaving>,
    last_typing_sent: Option<Instant>,
    tip: Option<tooltip::Active>,
    /// Channels whose first message has been loaded: the welcome goes above it.
    complete_history: HashSet<Id>,
    /// Width of a message's content column, set each frame: media never grows past it (`max-width: 100%`).
    content_width: f32,
    /// The open profile popout.
    profile: Option<profile::Popout>,
    /// The voice call, from joining until leaving.
    call: Option<voice::Call>,
    self_mute: bool,
    self_deaf: bool,
    /// Users the voice connection reports as speaking.
    speaking: HashSet<Id>,
    /// Text of attachments shown as plaintext previews, by `text:` key.
    texts: HashMap<String, String>,
    /// Files picked or dropped, waiting for the next send.
    pending_files: Vec<uploads::Pending>,
    /// This view, for handlers built while rendering that have no context of their own.
    weak: WeakEntity<Root>,
    /// Spoilers opened by a click: message, block and the first span of the spoiler.
    revealed: HashSet<(Id, u32, usize)>,
    settings: Option<settings::Open>,
    /// Settings the user changed in the settings pages, by key.
    setting_choices: HashMap<&'static str, settings::Choice>,
    /// Bodies of `Request::Fetch`, by key.
    fetched: HashMap<String, serde_json::Value>,
    tip_anchors: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
    pub ready: bool,
}

impl Root {
    pub fn new(requests: UnboundedSender<Request>, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Editor::new(editor_style(), cx));
        cx.subscribe(&composer, |this, _, event, _| match event {
            EditorEvent::Submit(content) => {
                this.last_typing_sent = None;
                this.send(content.clone());
            }
            EditorEvent::Edited => this.notify_typing(),
            EditorEvent::Cancel => {}
        })
        .detach();
        Self {
            stores: Stores::default(),
            requests,
            guild: None,
            channel: None,
            rows: Vec::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(1000.)),
            composer,
            focus: cx.focus_handle(),
            images: HashMap::new(),
            requested: HashSet::new(),
            selection: SharedSelection::default(),
            hovered: None,
            editing: None,
            replying: None,
            menu: None,
            last_menu: None,
            confirm_delete: None,
            show_members: false,
            show_dm_profile: true,
            friends_tab: friends::FriendsTab::default(),
            inbox: None,
            gifs: None,
            shown_blocked: HashSet::new(),
            shown_blocked_profiles: HashSet::new(),
            hovered_dm: None,
            group_dm: None,
            add_friend: None,
            people_search: None,
            friend_menu: None,
            confirm_remove_friend: None,
            quick_switcher: None,
            notify_open: false,
            notify_submenu: None,
            pins_open: None,
            confirm_unpin: None,
            forward: None,
            channel_history: Vec::new(),
            speech: crate::speech::Speech::new(),
            speaking_message: None,
            toast: None,
            app_toast: None,
            submenu: None,
            custom_reminder: None,
            unread_from: None,
            picker: None,
            emoji_used: Vec::new(),
            confirm_pin: None,
            reactions_used: Vec::new(),
            history: history::History::default(),
            leaving: None,
            last_typing_sent: None,
            tip: None,
            complete_history: HashSet::new(),
            content_width: 793.,
            profile: None,
            call: None,
            self_mute: false,
            self_deaf: false,
            speaking: HashSet::new(),
            texts: HashMap::new(),
            pending_files: Vec::new(),
            revealed: HashSet::new(),
            weak: cx.weak_entity(),
            settings: None,
            setting_choices: HashMap::new(),
            fetched: HashMap::new(),
            tip_anchors: Rc::default(),
            ready: false,
        }
    }

    pub fn update(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Dispatch { name, data } => {
                self.stores.dispatch(&name, &data);
                if name.starts_with("RELATIONSHIP_") {
                    self.rebuild_rows();
                }
                if name == "READY" {
                    // The account's saved theme; unset keeps the default.
                    match self.stores.theme {
                        1 => set_theme(Theme::Dark),
                        2 => set_theme(Theme::Light),
                        3 => set_theme(Theme::Darker),
                        4 => set_theme(Theme::Midnight),
                        _ => {}
                    }
                    self.after_ready();
                }
                if name == "MESSAGE_CREATE"
                    && data["channel_id"].as_str()
                        == self.channel.map(|id| id.to_string()).as_deref()
                {
                    // A message in the open channel is read as it arrives.
                    self.read_current_channel();
                }
                if matches!(
                    name.as_str(),
                    "MESSAGE_CREATE" | "MESSAGE_DELETE" | "MESSAGE_UPDATE" | "MESSAGE_ACK"
                ) {
                    self.rebuild_rows();
                }
                if matches!(name.as_str(), "VOICE_STATE_UPDATE" | "VOICE_SERVER_UPDATE") {
                    self.maybe_connect_voice();
                }
                if name == "GUILD_MEMBER_LIST_UPDATE" {
                    self.request_member_images();
                }
                if name == "TYPING_START" {
                    // Redraw when the notice runs out.
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(TYPING_DURATION).await;
                        let _ = this.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                }
            }
            Update::Messages { channel, page } => {
                // A page shorter than asked for reaches the start of the channel.
                if page.as_array().map_or(0, Vec::len) < crate::net::MESSAGE_PAGE as usize {
                    self.complete_history.insert(channel);
                }
                self.stores.load_messages(channel, &page);
                if Some(channel) == self.channel {
                    self.read_current_channel();
                }
                self.rebuild_rows();
                self.request_images();
                self.ready = true;
            }
            Update::Voice(event) => self.on_voice_event(event, cx),
            Update::Fetched { key, body } => {
                if self.friends_fetched(&key, &body, cx) {
                    return;
                }
                let gif_answer = key.starts_with("gifs:");
                let profile = key
                    .strip_prefix("profile:")
                    .and_then(|user| user.parse::<Id>().ok());
                let frecency = key == actions::FRECENCY_KEY;
                if key == saved::SAVED_KEY {
                    self.stores.load_saved(&body);
                }
                self.fetched.insert(key.clone(), body);
                if gif_answer {
                    self.request_gif_images(&key);
                }
                if let Some(user) = profile {
                    self.request_profile_images(user);
                }
                if frecency {
                    let wanted: Vec<&'static str> = self
                        .quick_reactions()
                        .into_iter()
                        .map(|emoji| emoji.surrogates)
                        .collect();
                    for surrogates in wanted {
                        self.request_emoji_image(surrogates);
                    }
                }
                cx.notify();
            }
            Update::Image { key, bytes } if key.starts_with("text:") => {
                self.texts
                    .insert(key, String::from_utf8_lossy(&bytes).into_owned());
                cx.notify();
            }
            Update::Image { key, bytes } => {
                let format = if key.starts_with("emoji:")
                    || key.starts_with("filekind:")
                    || bytes.starts_with(b"<svg")
                {
                    ImageFormat::Svg
                } else if bytes.starts_with(b"\x89PNG") {
                    ImageFormat::Png
                } else if bytes.starts_with(b"RIFF") {
                    ImageFormat::Webp
                } else {
                    ImageFormat::Jpeg
                };
                self.images
                    .insert(key, Arc::new(Image::from_bytes(format, bytes)));
            }
            Update::Images(images) => {
                for (key, bytes) in images {
                    self.images
                        .insert(key, Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
                }
            }
        }
        cx.notify();
    }

    fn after_ready(&mut self) {
        let Some(guild) = self.stores.guilds.first() else {
            return;
        };
        let guild_id = guild.id;
        let channel = guild
            .sidebar_rows()
            .iter()
            .find(|channel| channel.kind == ChannelKind::Text)
            .map(|channel| channel.id);
        self.select(guild_id, channel);
        // Opening on the last channel is not a step back can return from.
        self.history = history::History::default();
        self.request_images();
        self.request_emoji_button();
        self.request_frecency();
        self.request_saved();
    }

    /// The composer's emoji button is a sprite cell tinted with the theme's icon color, so it follows the theme.
    pub(crate) fn request_emoji_button(&mut self) {
        let tint = theme().color("--interactive-text-default");
        let _ = self.requests.send(Request::Sprite {
            key: "emoji_button".to_owned(),
            tint,
        });
    }

    fn select(&mut self, guild: Id, channel: Option<Id>) {
        let from = self.route();
        self.history.visit(
            from,
            history::Route {
                guild: Some(guild),
                channel,
            },
        );
        self.guild = Some(guild);
        self.channel = channel;
        self.unread_from = None;
        if let Some(channel) = channel {
            self.remember_channel(channel);
        }
        if let Some(channel) = channel {
            let _ = self.requests.send(Request::Subscribe { guild, channel });
            let _ = self.requests.send(Request::LoadMessages { channel });
        }
        self.rebuild_rows();
    }

    fn request_image(&mut self, key: String, url: String) {
        if self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Image { key, url });
        }
    }

    fn request_images(&mut self) {
        let guilds: Vec<(Id, String)> = self
            .stores
            .guilds
            .iter()
            .filter_map(|guild| Some((guild.id, guild.icon.clone()?)))
            .collect();
        for (id, icon) in guilds {
            self.request_image(
                format!("guild:{id}"),
                format!("https://cdn.discordapp.com/icons/{id}/{icon}.webp?size=96"),
            );
        }
        let mut users: Vec<User> = self.stores.current_user.iter().cloned().collect();
        users.extend(self.stores.friends().into_iter().cloned());
        users.extend(
            self.stores
                .private_channels
                .iter()
                .filter_map(|channel| self.stores.dm_recipient(channel))
                .cloned(),
        );
        let mut media = Vec::new();
        if let Some(messages) = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
        {
            users.extend(messages.iter().map(|message| message.author.clone()));
            for message in messages.iter() {
                media.extend(embed::wanted(message));
                media.extend(files::wanted(message));
            }
            for attachment in messages.iter().flat_map(|message| &message.attachments) {
                if attachment.is_image() {
                    let (width, height) = fit_media(
                        attachment.width.unwrap_or(1),
                        attachment.height.unwrap_or(1),
                    );
                    let url = format!(
                        "{}format=webp&quality=lossless&width={width}&height={height}",
                        attachment.proxy_url,
                    );
                    media.push((format!("attachment:{}", attachment.id), url));
                }
            }
        }
        if let Some(messages) = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
        {
            for block in messages.iter().flat_map(|message| parse(&message.content)) {
                for span in block_spans(&block) {
                    match &span.kind {
                        Kind::Unicode => {
                            if let Some(file) = dc_tokens::emoji_file(&span.text) {
                                let url = format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN);
                                media.push((format!("emoji:{}", span.text), url));
                            }
                        }
                        Kind::Emoji { id, .. } => media.push((
                            format!("custom:{id}"),
                            format!("https://cdn.discordapp.com/emojis/{id}.webp?size=96"),
                        )),
                        _ => {}
                    }
                }
            }
            for reaction in messages.iter().flat_map(|message| &message.reactions) {
                if let Some(file) = dc_tokens::emoji_file(&reaction.name) {
                    let url = format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN);
                    media.push((format!("emoji:{}", reaction.name), url));
                }
            }
        }
        for (key, url) in media {
            self.request_image(key, url);
        }
        for user in users {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
            if let Some(url) = user.tag.as_ref().and_then(dc_state::ServerTag::badge_url) {
                self.request_image(url.clone(), url);
            }
        }
    }

    fn request_member_images(&mut self) {
        let Some(list) = self
            .guild
            .and_then(|guild| self.stores.member_lists.get(&guild))
        else {
            return;
        };
        let users: Vec<User> = list
            .rows
            .iter()
            .filter_map(|row| match row {
                MemberRow::Member { user, .. } => Some(user.clone()),
                _ => None,
            })
            .collect();
        for user in users {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
        }
    }

    /// The official panel: 264px wide, 42px rows, section headers with their counts.
    fn member_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self
            .guild
            .and_then(|guild| self.stores.member_lists.get(&guild));
        let owner = self.current_guild().and_then(|guild| guild.owner_id);
        let rows: Vec<AnyElement> = list
            .map(|list| list.rows.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|row| match row {
                MemberRow::Group { id, count } => div()
                    .h(px(42.))
                    .pt(px(20.))
                    .pl(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    .child(format!("{} \u{2014} {count}", group_label(id)))
                    .into_any_element(),
                MemberRow::Member { user, nick, status } => {
                    let avatar = self.images.get(&avatar_source(user).0).cloned();
                    let name = nick
                        .clone()
                        .unwrap_or_else(|| user.display_name().to_owned());
                    let item = div()
                        .h(px(40.))
                        .w(px(248.))
                        .pl(px(8.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .child(
                            div()
                                .relative()
                                .size(px(32.))
                                .children(
                                    avatar.map(|image| img(image).size(px(32.)).rounded_full()),
                                )
                                .child(status_dot(status, "--background-base-lower")),
                        )
                        .child(
                            div()
                                .ml(px(12.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--channels-default"))
                                .child(name),
                        )
                        .when(owner == Some(user.id), |this| {
                            this.child(div().ml(px(4.)).child(icon(
                                "owner_crown",
                                14.,
                                "--status-warning",
                            )))
                        });
                    div()
                        .h(px(42.))
                        .pt(px(1.))
                        .pl(px(8.))
                        .child(self.profile_trigger_side(
                            format!("member-{}", user.id),
                            user.id,
                            item,
                            true,
                            cx,
                        ))
                        .into_any_element()
                }
                MemberRow::Unknown => div().h(px(42.)).into_any_element(),
            })
            .collect();
        div()
            .parity("members")
            .flex_none()
            .w(px(264.))
            .h_full()
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .children(rows)
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        let empty = Vec::new();
        let messages = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .unwrap_or(&empty);
        let mut last_day = None;
        let complete = self
            .channel
            .is_some_and(|channel| self.complete_history.contains(&channel));
        if complete && self.current_dm().is_some() {
            self.rows.push(Row::Welcome);
        }
        let divider = self.divider_after();
        let mut divided = false;
        let blocked: HashSet<Id> = self
            .stores
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == blocked::RELATIONSHIP_BLOCKED)
            .map(|(user, _)| *user)
            .collect();
        // The run of blocked messages being folded: its first id, and whether it is open.
        let mut run: Option<(Id, bool)> = None;
        for (index, message) in messages.iter().enumerate() {
            let local = message.local_time();
            let day = (local.year(), local.ordinal());
            let new_day = last_day != Some(day);
            if new_day {
                self.rows
                    .push(Row::Divider(local.format("%B %-d, %Y").to_string()));
                last_day = Some(day);
            }
            if blocked.contains(&message.author.id) {
                let open = match run {
                    Some((first, open)) if !new_day => {
                        if let Some(Row::Blocked { count, .. }) = self
                            .rows
                            .iter_mut()
                            .rev()
                            .find(|row| matches!(row, Row::Blocked { first: known, .. } if *known == first))
                        {
                            *count += 1;
                        }
                        open
                    }
                    _ => {
                        let open = self.shown_blocked.contains(&message.id);
                        self.rows.push(Row::Blocked {
                            first: message.id,
                            count: 1,
                        });
                        run = Some((message.id, open));
                        open
                    }
                };
                if open {
                    self.rows.push(Row::Message { index, head: true });
                }
                continue;
            }
            run = None;
            let mut head = new_day || index == 0 || starts_group(&messages[index - 1], message);
            if !divided && divider.is_some_and(|acked| message.id > acked) {
                // The divider starts a new group, as the official list breaks one there.
                self.rows.push(Row::NewDivider);
                divided = true;
                head = true;
            }
            self.rows.push(Row::Message { index, head });
        }
        self.rows.push(Row::Spacer);
        self.list.reset(self.rows.len());
    }

    /// Brings a loaded message of the open channel into view.
    fn scroll_to_message(&mut self, message: Id) {
        let messages = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel));
        let Some(index) =
            messages.and_then(|list| list.iter().position(|known| known.id == message))
        else {
            return;
        };
        let row = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Message { index: at, .. } if *at == index));
        if let Some(row) = row {
            self.list.scroll_to_reveal_item(row);
        }
    }

    fn current_guild(&self) -> Option<&Guild> {
        self.guild.and_then(|guild| self.stores.guild(guild))
    }

    /// The current channel's name, or the other person's in a DM.
    fn channel_title(&self) -> String {
        if let Some(channel) = self.current_channel() {
            return channel.name.clone();
        }
        self.current_dm()
            .and_then(|channel| self.stores.dm_recipient(channel))
            .map(|user| user.display_name().to_owned())
            .unwrap_or_default()
    }

    fn current_dm(&self) -> Option<&dc_state::PrivateChannel> {
        let channel = self.channel?;
        self.stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
    }

    fn current_channel(&self) -> Option<&Channel> {
        let channel = self.channel?;
        self.current_guild()?
            .channels
            .iter()
            .find(|known| known.id == channel)
    }

    /// The selected text across messages, one block per line.
    pub fn selected_text(&self) -> String {
        let selection = self.selection.borrow();
        let empty = Vec::new();
        let messages = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .unwrap_or(&empty);
        let mut lines = Vec::new();
        for message in messages {
            for (index, block) in parse(&message.content).iter().enumerate() {
                let text = block_text(block);
                if let Some((from, to)) = selection.range_in((message.id, index as u32), text.len())
                {
                    lines.push(text[from..to].to_owned());
                }
            }
        }
        lines.join("\n")
    }

    /// For test scripts: where the app is now.
    #[cfg(feature = "capture")]
    pub fn describe(&self, cx: &App) -> String {
        let composer = self.composer.read(cx).text();
        let add_friend = self
            .add_friend
            .as_ref()
            .map(|form| (form.name.read(cx).text().to_owned(), form.result.clone()));
        format!(
            "add_friend={add_friend:?} friends_tab={:?} guild={} channel={} composer={composer:?} rows={} selected={:?} hovered={:?} editing={} embeds={} theme={} voice={}",
            self.friends_tab,
            self.current_guild()
                .map(|guild| guild.name.as_str())
                .unwrap_or("-"),
            self.current_channel()
                .map(|channel| channel.name.as_str())
                .unwrap_or("-"),
            self.rows.len(),
            self.selected_text(),
            self.hovered,
            self.editing.is_some(),
            self.channel
                .and_then(|channel| self.stores.messages.get(&channel))
                .map_or(0, |messages| messages
                    .iter()
                    .map(|message| message.embeds.len())
                    .sum::<usize>()),
            self.stores.theme,
            match &self.call {
                Some(call) if call.connected => "connected",
                Some(_) => "connecting",
                None => "none",
            },
        )
    }

    fn begin_edit(&mut self, message: Id, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_text(text);
            editor
        });
        cx.subscribe(&editor, move |this, _, event, cx| {
            match event {
                // Saving an empty edit asks to delete the message instead.
                EditorEvent::Submit(content) if content.is_empty() => {
                    this.confirm_delete = Some(message);
                    this.editing = None;
                }
                EditorEvent::Submit(content) => {
                    if let Some(channel) = this.channel {
                        let _ = this.requests.send(Request::Edit {
                            channel,
                            message,
                            content: content.clone(),
                        });
                    }
                    this.editing = None;
                }
                EditorEvent::Cancel => this.editing = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        })
        .detach();
        window.focus(&editor.read(cx).focus_handle(cx), cx);
        self.editing = Some((message, editor));
        cx.notify();
    }

    /// `TYPING_START_LOCAL`: at most one notice per 80% of the notice's length.
    fn notify_typing(&mut self) {
        let due = self
            .last_typing_sent
            .is_none_or(|sent| sent.elapsed() >= TYPING_DURATION.mul_f32(0.8));
        if let (true, Some(channel)) = (due, self.channel) {
            self.last_typing_sent = Some(Instant::now());
            let _ = self.requests.send(Request::Typing { channel });
        }
    }

    fn typing_bar(&self) -> Option<AnyElement> {
        let channel = self.channel?;
        let users = self.stores.typing_users(channel, Instant::now());
        let names: Vec<String> = users
            .iter()
            .filter_map(|user| self.stores.users.get(user))
            .map(|user| user.display_name().to_owned())
            .collect();
        let (lead, tail) = match names.as_slice() {
            [] => return None,
            [one] => (one.clone(), " is typing..."),
            [one, two] => (format!("{one}, {two}"), " are typing..."),
            _ => ("Several people".to_owned(), " are typing..."),
        };
        let dot = || {
            div()
                .size(px(6.))
                .rounded_full()
                .bg(color("--text-default"))
        };
        Some(
            div()
                .absolute()
                .left(px(16.))
                .top(px(-24.))
                .h(px(24.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--interactive-text-default"))
                .child(
                    div()
                        .ml(px(8.))
                        .flex()
                        .gap(px(3.))
                        .child(dot())
                        .child(dot())
                        .child(dot()),
                )
                .child(
                    div()
                        .ml(px(6.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(lead),
                )
                .child(tail)
                .into_any_element(),
        )
    }

    fn send(&mut self, content: String) {
        if content.is_empty() && self.pending_files.is_empty() {
            return;
        }
        let reply = self.replying.take().map(|message| (self.guild, message.id));
        let files = self
            .pending_files
            .drain(..)
            .map(|pending| pending.path)
            .collect();
        if let Some(channel) = self.channel {
            let _ = self.requests.send(Request::Send {
                channel,
                content,
                files,
                reply,
            });
        }
    }

    pub(crate) fn begin_reply(
        &mut self,
        message: Message,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replying = Some(message);
        window.focus(&self.composer.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// The official preview: a spine into the avatar, 16px avatar, name, and one line of the text.
    fn reply_preview(&self, referenced: &Message) -> impl IntoElement {
        let avatar = self
            .images
            .get(&avatar_source(&referenced.author).0)
            .cloned();
        let text = first_spans(&referenced.content);
        div()
            .relative()
            .h(px(18.))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .child(
                div()
                    .absolute()
                    .left(px(-37.))
                    .top(px(8.))
                    .w(px(33.))
                    .h(px(12.))
                    .border_l_2()
                    .border_t_2()
                    .rounded_tl(px(6.))
                    .border_color(color("--spine-default")),
            )
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .children(avatar.map(|image| img(image).size(px(16.)).rounded_full())),
            )
            .child(
                div()
                    .ml(px(4.))
                    .flex_none()
                    .opacity(0.64)
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(referenced.author.display_name().to_owned()),
            )
            .child(
                div()
                    .ml(px(4.))
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(18.))
                    .child(self.preview_text(&text)),
            )
    }

    fn preview_text(&self, spans: &[Span]) -> Inline {
        let color_value = hsla("--text-default");
        let pieces = spans
            .iter()
            .map(|span| {
                let style = span.style;
                let mut run_font: Font = font(if style.code { MONO } else { SANS });
                run_font.features = kerning();
                run_font.weight = if style.bold {
                    FontWeight(700.)
                } else {
                    FontWeight(400.)
                };
                run_font.style = if style.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                };
                Piece {
                    text: span.text.clone(),
                    font: run_font,
                    size: if style.code { px(11.9) } else { px(14.) },
                    color: color_value,
                    underline: style.underline,
                    strike: style.strike,
                    boxed: style.code.then(preview_code_box),
                    image: None,
                }
            })
            .collect();
        inline(pieces, px(18.)).truncate()
    }

    /// Every text field that is open now, for the test harness to find the focused one.
    #[cfg(feature = "capture")]
    pub fn open_editors(&self) -> Vec<Entity<Editor>> {
        let mut list: Vec<Entity<Editor>> = Vec::new();
        list.extend(self.group_dm.as_ref().map(|picker| picker.search.clone()));
        list.extend(
            self.quick_switcher
                .as_ref()
                .map(|switcher| switcher.search.clone()),
        );
        if let Some(forward) = self.forward.as_ref() {
            list.push(forward.search.clone());
            list.push(forward.note.clone());
        }
        if let Some(form) = self.add_friend.as_ref() {
            list.push(form.name.clone());
            list.push(form.note.clone());
        }
        list.extend(
            self.people_search
                .as_ref()
                .map(|search| search.field.clone()),
        );
        list.push(self.active_editor());
        list
    }

    /// The field with focus: the inline editor while editing, else the composer.
    #[cfg(feature = "capture")]
    pub fn active_editor(&self) -> Entity<Editor> {
        if let Some(search) = self.picker.as_ref().map(picker::Picker::search) {
            return search;
        }
        self.editing
            .as_ref()
            .map_or_else(|| self.composer.clone(), |(_, editor)| editor.clone())
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "c" && keystroke.modifiers.control {
            let text = self.selected_text();
            if !text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
    }

    fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match (self.current_guild(), self.channel) {
            (Some(guild), _) => guild.name.clone(),
            (None, None) => "Friends".to_owned(),
            // A DM's window title is the section, not the person (`title_LnjVAX` in the DM capture).
            (None, Some(_)) => "Direct Messages".to_owned(),
        };
        let in_dm = self.guild.is_none() && self.channel.is_some();
        let acronym = self.current_guild().map(Guild::acronym).unwrap_or_default();
        div()
            .parity("titlebar")
            .relative()
            .flex_none()
            .h(px(32.))
            .w_full()
            .window_control_area(WindowControlArea::Drag)
            .child(self.history_button(true, cx))
            .child(self.history_button(false, cx))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(if self.guild.is_some() {
                        div()
                            .size(px(16.))
                            .rounded(px(4.))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(8.))
                            .text_color(color("--text-default"))
                            .child(acronym)
                            .into_any_element()
                    } else if in_dm {
                        icon("title_direct_messages", 18., "--text-default").into_any_element()
                    } else {
                        icon("home_friends", 16., "--text-default").into_any_element()
                    })
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(title),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .right(px(0.))
                    .top(px(0.))
                    .flex()
                    .items_center()
                    .child(
                        bar_button("inbox", "inbox")
                            .relative()
                            .mr(px(12.))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_inbox(cx)))
                            .child(self.anchor_probe("bar-inbox")),
                    )
                    .child(
                        // `ar` in module 596339: an anchor to the Help Center (`tP.C`, https:// + support host).
                        bar_button("help", "help")
                            .mr(px(16.))
                            .on_click(|_, _, cx| cx.open_url("https://support.discord.com")),
                    )
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(20.))
                            .mr(px(8.))
                            .bg(color("--background-mod-subtle")),
                    )
                    .child(window_controls()),
            )
    }

    fn rail_item(&self, guild: &Guild, cx: &mut Context<Self>) -> impl IntoElement {
        let id = guild.id;
        let selected = self.guild == Some(id);
        let image = self.images.get(&format!("guild:{id}")).cloned();
        let group = SharedString::from(format!("guild-row-{id}"));
        let tile = div()
            .id(SharedString::from(format!("guild-{id}")))
            .size(px(40.))
            .rounded(px(12.))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                let channel = this.stores.guild(id).and_then(|guild| {
                    guild
                        .sidebar_rows()
                        .iter()
                        .find(|row| row.kind == ChannelKind::Text)
                        .map(|row| row.id)
                });
                this.select(id, channel);
                this.request_images();
                cx.notify();
            }));
        let tile = match image {
            Some(image) => tile.child(img(image).size(px(40.)).rounded(px(12.))),
            None => tile
                .bg(if selected {
                    color("--background-brand")
                } else {
                    color("--background-mod-subtle")
                })
                .text_size(px(18.))
                .line_height(px(21.6))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .child(guild.acronym()),
        };
        div()
            .relative()
            .w(px(72.))
            .h(px(40.))
            .flex()
            .justify_center()
            .group(group.clone())
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(if selected { 0. } else { 10. }))
                    .w(px(4.))
                    .h(px(if selected { 40. } else { 20. }))
                    .rounded_r(px(4.))
                    .bg(color("--text-strong"))
                    .when(!selected, |this| {
                        this.opacity(0.)
                            .group_hover(group, |style| style.opacity(1.))
                    }),
            )
            // `item__o8_Xq` unread: an 8px pill while a channel in the guild is unread.
            .when(!selected && self.stores.guild_unread(guild), |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(16.))
                        .w(px(4.))
                        .h(px(8.))
                        .rounded_r(px(4.))
                        .bg(color("--text-strong")),
                )
            })
            .child(self.with_tip(format!("guild-{id}"), Tip::rail(Body::Guild(id)), tile, cx))
    }

    fn rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tool = |name: &str, size: f32| {
            div()
                .size(px(40.))
                .rounded(px(12.))
                .bg(color("--background-mod-subtle"))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, size, "--text-default"))
        };
        let list = div()
            .parity("rail")
            .flex_1()
            .min_h(px(0.))
            .w(px(72.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .child(self.home_tile(cx))
            .child(
                div()
                    .w(px(32.))
                    .h(px(1.))
                    .bg(color("--background-mod-subtle")),
            )
            .children(
                self.stores
                    .guilds
                    .iter()
                    .map(|guild| self.rail_item(guild, cx).into_any_element())
                    .collect::<Vec<_>>(),
            )
            .child(self.with_tip(
                "add-server",
                Tip::rail(Body::Heading("Add a Server".into())),
                tool("add_server", 20.),
                cx,
            ))
            .child(self.with_tip(
                "discover",
                Tip::rail(Body::Heading("Discover".into())),
                tool("discover", 20.),
                cx,
            ));
        // The official rail ends where the user panel starts.
        div()
            .flex_none()
            .w(px(72.))
            .h_full()
            .flex()
            .flex_col()
            .child(list)
            .child(div().flex_none().h(px(50.)))
    }

    fn channel_row(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        if channel.kind == ChannelKind::Category {
            let collapsed = self.guild.is_some_and(|guild| {
                self.stores
                    .guild_settings
                    .channel(guild, channel.id)
                    .collapsed
            });
            let (guild, category) = (self.guild, channel.id);
            return div()
                .id(SharedString::from(format!("category-{}", channel.id)))
                .flex_none()
                .pt(px(16.))
                .h(px(40.))
                .pl(px(16.))
                .pr(px(17.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(guild) = guild {
                        this.toggle_category(guild, category);
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--channels-default"))
                        .child(channel.name.clone())
                        .child(
                            svg()
                                .path("icons/category_chevron.svg")
                                .size(px(12.))
                                .flex_none()
                                .text_color(color("--channels-default"))
                                .when(collapsed, |this| {
                                    this.with_transformation(gpui::Transformation::rotate(
                                        gpui::radians(-std::f32::consts::FRAC_PI_2),
                                    ))
                                }),
                        )
                        .child(div().flex_1())
                        .child(icon("plus_small", 16., "--channels-default")),
                )
                .into_any_element();
        }
        let selected = self.channel == Some(channel.id);
        // The voice channel this client is in reads like a selected one, with a green speaker and a status line.
        let in_call = channel.kind == ChannelKind::Voice
            && self
                .call
                .as_ref()
                .is_some_and(|call| call.channel == channel.id);
        // `modeUnreadImportant`: an unread channel reads in `--text-strong`, with a pill at the sidebar's edge.
        // `modeMuted`: a muted channel never reads unread, and dims to half in `--text-muted`.
        let muted = self.guild.is_some_and(|guild| {
            self.stores
                .guild_settings
                .channel_muted(guild, channel.id, chrono::Utc::now())
        });
        let unread =
            !muted && channel.kind == ChannelKind::Text && self.stores.is_unread(channel.id);
        let dim = muted && !selected && !in_call;
        let tint = if selected || in_call || unread {
            "--text-strong"
        } else if dim {
            "--text-muted"
        } else {
            "--channels-default"
        };
        let name = if channel.kind == ChannelKind::Voice {
            "voice_channel"
        } else {
            "text_channel"
        };
        let group = SharedString::from(format!("channel-{}", channel.id));
        let voice = channel.kind == ChannelKind::Voice;
        let actions: &[&str] = if voice {
            &["voice_action_1", "voice_action_2", "voice_action_3"]
        } else {
            &["invite_small", "settings_small"]
        };
        let action_tint = if selected || in_call {
            "--text-strong"
        } else {
            "--interactive-icon-default"
        };
        let action_group = group.clone();
        let name_group = group.clone();
        let target = channel.id;
        div()
            .flex_none()
            .relative()
            .h(px(34.))
            .pt(px(1.))
            .pl(px(8.))
            .when(unread, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(1.))
                        .top(px(13.))
                        .w(px(4.))
                        .h(px(8.))
                        .rounded_r(px(4.))
                        .bg(color("--interactive-text-active")),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!("channel-row-{}", channel.id)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if voice {
                            if let Some(guild) = this.guild {
                                this.join_voice(guild, target, cx);
                            }
                        } else {
                            let guild = this.guild;
                            if let Some(guild) = guild {
                                this.select(guild, Some(target));
                                this.request_images();
                            }
                            cx.notify();
                        }
                    }))
                    .group(group)
                    .h(px(32.))
                    .w(px(286.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(selected, |this| {
                        this.parity("channel-selected")
                            .bg(color("--background-mod-strong"))
                    })
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(color("--background-mod-subtle")))
                    })
                    .child(div().when(dim, |this| this.opacity(0.5)).child(icon(
                        name,
                        20.,
                        if in_call {
                            "--icon-voice-connected"
                        } else if dim {
                            "--icon-muted"
                        } else {
                            tint
                        },
                    )))
                    .child(
                        div()
                            .flex_1()
                            .when(dim, |this| this.opacity(0.5))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color(tint))
                            .group_hover(name_group, |style| {
                                style.text_color(color("--text-strong"))
                            })
                            .child(channel.name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .when(!selected && !in_call, |this| {
                                this.opacity(0.)
                                    .group_hover(action_group, |style| style.opacity(1.))
                            })
                            .children(
                                actions
                                    .iter()
                                    .map(|name| icon(name, 16., action_tint).into_any_element()),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let guild = self.current_guild();
        let name = guild.map(|guild| guild.name.clone()).unwrap_or_default();
        let plain_row = |icon_name: &str, label: &str| {
            div().flex_none().h(px(34.)).pt(px(1.)).pl(px(8.)).child(
                div()
                    .h(px(32.))
                    .w(px(286.))
                    .pl(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(icon_name, 20., "--channels-default"))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--channels-default"))
                            .child(label.to_owned()),
                    ),
            )
        };
        let rows: Vec<AnyElement> = guild
            .map(|guild| {
                let collapsed = |category: Option<Id>| {
                    category.is_some_and(|category| {
                        self.stores
                            .guild_settings
                            .channel(guild.id, category)
                            .collapsed
                    })
                };
                guild
                    .sidebar_rows()
                    .into_iter()
                    // A collapsed category keeps only the open channel and the call this client is in.
                    .filter(|row| {
                        row.kind == ChannelKind::Category
                            || !collapsed(row.parent_id)
                            || self.channel == Some(row.id)
                            || self
                                .call
                                .as_ref()
                                .is_some_and(|call| call.channel == row.id)
                    })
                    .flat_map(|row| {
                        let mut rows = vec![self.channel_row(row, cx)];
                        if row.kind == ChannelKind::Voice {
                            if self
                                .call
                                .as_ref()
                                .is_some_and(|call| call.channel == row.id)
                            {
                                rows.push(voice::status_line());
                            }
                            rows.extend(self.voice_members(guild.id, row.id));
                        }
                        rows
                    })
                    .collect()
            })
            .unwrap_or_default();
        div()
            .parity("sidebar")
            .relative()
            .flex_none()
            .w(px(303.))
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_l_1()
                    .border_t_1()
                    .rounded_tl(px(12.))
                    .border_color(color("--background-mod-subtle")),
            )
            .child(div().flex_none().h(px(1.)))
            .child(
                div()
                    .parity_below("guild-header", 1.)
                    .flex_none()
                    .h(px(49.))
                    .border_b_1()
                    .border_color(color("--background-mod-subtle"))
                    .px(px(8.))
                    .pt(px(8.))
                    .child(
                        div()
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .pl(px(8.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(name),
                            )
                            .child(div().ml(px(2.)).child(icon(
                                "chevron_down",
                                16.,
                                "--text-strong",
                            )))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .size(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("invite", 20., "--text-strong")),
                            ),
                    ),
            )
            .child(div().flex_none().h(px(12.)))
            .child(plain_row("events", "Events"))
            .child(plain_row("boosts", "Server Boosts"))
            .child(
                div()
                    .flex_none()
                    .mt(px(11.))
                    .ml(px(16.))
                    .w(px(271.))
                    .h(px(1.))
                    .bg(color("--background-mod-subtle")),
            )
            .children(rows)
    }

    fn user_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        // Muted and deafened buttons turn red with their chevron: `redGlow_Y-8oZW`.
        let tint = |muted: bool| {
            if muted {
                "--icon-voice-muted"
            } else {
                "--interactive-text-default"
            }
        };
        let button = |name: &str| {
            div()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, 20., "--interactive-text-default"))
        };
        let toggle = |name: &str, muted: bool| {
            div()
                .size(px(32.))
                .when(muted, |this| {
                    this.rounded_l(px(8.)).bg(color("--background-voice-muted"))
                })
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, 20., tint(muted)))
        };
        let chevron = |muted: bool| {
            div()
                .w(px(16.))
                .h(px(32.))
                .ml(px(1.))
                .when(muted, |this| {
                    this.rounded_r(px(8.)).bg(color("--background-voice-muted"))
                })
                .flex()
                .items_center()
                .justify_center()
                .child(icon("audio_chevron", 16., tint(muted)))
        };
        let mic_muted = self.self_mute || self.self_deaf;
        let row = div()
            .h(px(56.))
            .flex()
            .items_center()
            .pl(px(11.))
            .child(
                div()
                    .relative()
                    .size(px(32.))
                    .children(avatar.map(|image| img(image).size(px(32.)).rounded_full()))
                    .child(status_dot(&self.stores.status, "--background-base-low")),
            )
            .child(
                div()
                    .ml(px(8.))
                    .w(px(136.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(user.display_name().to_owned()),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(px(12.))
                            .line_height(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--interactive-text-default"))
                            .child(status_label(&self.stores.status)),
                    ),
            )
            .child(
                div()
                    .ml(px(12.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("toggle-mute")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_self_mute(cx)))
                            .child(self.with_tip(
                                "mic",
                                Tip::text(if mic_muted { "Unmute" } else { "Mute" }, Side::Top),
                                toggle(if mic_muted { "mic_muted" } else { "mic" }, mic_muted),
                                cx,
                            )),
                    )
                    .child(chevron(mic_muted)),
            )
            .child(
                div()
                    .ml(px(8.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("toggle-deafen")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_self_deaf(cx)))
                            .child(self.with_tip(
                                "deafen",
                                Tip::text(
                                    if self.self_deaf { "Undeafen" } else { "Deafen" },
                                    Side::Top,
                                ),
                                toggle(
                                    if self.self_deaf {
                                        "headphones_deafened"
                                    } else {
                                        "headphones"
                                    },
                                    self.self_deaf,
                                ),
                                cx,
                            )),
                    )
                    .child(chevron(self.self_deaf)),
            )
            .child(
                div()
                    .id("open-settings")
                    .ml(px(8.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx)))
                    .child(self.with_tip(
                        "user-settings",
                        Tip::text("User Settings", Side::Top),
                        button("settings"),
                        cx,
                    )),
            );
        // `panels_pog62O`: one bordered box; while in voice the call panel sits above the user row.
        div()
            .parity_with_border("user-panel", 1.)
            .absolute()
            .left(px(8.))
            .bottom(px(8.))
            .w(px(359.))
            .rounded(px(8.))
            .bg(color("--background-base-low"))
            .border_1()
            .border_color(color("--background-mod-subtle"))
            .flex()
            .flex_col()
            .children(self.voice_panel(cx))
            .child(row)
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.channel_title();
        let dm_user = self
            .current_dm()
            .and_then(|channel| self.stores.dm_recipient(channel))
            .cloned();
        let muted = self.channel_muted_now();
        let search = format!(
            "Search {}",
            match &dm_user {
                Some(user) => user.username.as_str(),
                None => self
                    .current_guild()
                    .map(|guild| guild.name.as_str())
                    .unwrap_or_default(),
            }
        );
        // Header icons brighten to `--interactive-text-default` under the pointer.
        // A button whose popout is open shows in `--interactive-text-active`.
        let open_popout = |name: &str| match name {
            "bell" | "bell_muted" => self.notify_open,
            "pin" => self.pins_open.is_some(),
            _ => false,
        };
        let tool = |name: &str| {
            let group = SharedString::from(format!("header-{name}"));
            let active = open_popout(name);
            div()
                .group(group.clone())
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path(format!("icons/{name}.svg"))
                        .size(px(20.))
                        .flex_none()
                        .text_color(if active {
                            color("--interactive-text-active")
                        } else {
                            color("--text-muted")
                        })
                        .group_hover(group, |style| {
                            style.text_color(color("--interactive-text-default"))
                        }),
                )
        };
        div()
            .parity_below("chat-header", 1.)
            .flex_none()
            .h(px(49.))
            .border_b_1()
            .border_color(color("--background-mod-subtle"))
            .pl(px(15.))
            .pr(px(8.))
            .flex()
            .items_center()
            .bg(color("--background-base-lower"))
            .when(self.guild.is_some(), |this| {
                this.child(icon("header_hash", 20., "--channels-default"))
            })
            .when_some(dm_user.clone(), |this, user| {
                // `avatar_z_gUBg`: 20px with the status cut into its corner (`svg-mask-avatar-status-round-20`).
                let presence = self
                    .stores
                    .presences
                    .get(&user.id)
                    .cloned()
                    .unwrap_or_else(|| "offline".to_owned());
                let picture = self.images.get(&avatar_source(&user).0).cloned();
                this.child(
                    div()
                        .ml(px(1.))
                        .relative()
                        .size(px(20.))
                        .children(picture.map(|picture| img(picture).size(px(20.)).rounded_full()))
                        .child(status_dot_small(&presence, "--background-base-lower")),
                )
            })
            .child(
                div()
                    .ml(px(if dm_user.is_some() { 8. } else { 9. }))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(name),
            )
            .child(div().flex_1())
            .when(dm_user.is_some(), |this| {
                let shown = self.show_dm_profile;
                this.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(self.with_tip(
                            "dm-call",
                            Tip::text("Start Voice Call", Side::Bottom),
                            tool("dm_call"),
                            cx,
                        ))
                        .child(self.with_tip(
                            "dm-video",
                            Tip::text("Start Video Call", Side::Bottom),
                            tool("dm_video"),
                            cx,
                        ))
                        .child(
                            self.with_tip(
                                "dm-pins",
                                Tip::text("Pinned Messages", Side::Bottom),
                                tool("pin"),
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tip = None;
                                this.toggle_pins("dm-pins", cx);
                            })),
                        )
                        .child(self.with_tip(
                            "dm-add",
                            Tip::text("Add to DM", Side::Bottom),
                            tool("dm_add"),
                            cx,
                        ))
                        .child(
                            self.with_tip(
                                "dm-profile",
                                Tip::text(
                                    if shown {
                                        "Hide User Profile"
                                    } else {
                                        "Show User Profile"
                                    },
                                    Side::Bottom,
                                ),
                                div()
                                    .id("toggle-dm-profile")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_dm_profile = !this.show_dm_profile;
                                        cx.notify();
                                    }))
                                    .size(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        "dm_profile",
                                        20.,
                                        if shown {
                                            "--text-strong"
                                        } else {
                                            "--text-muted"
                                        },
                                    )),
                                cx,
                            ),
                        ),
                )
            })
            .when(dm_user.is_none(), |this| {
                this.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(self.with_tip(
                            "threads",
                            Tip::text("Threads", Side::Bottom),
                            tool("threads"),
                            cx,
                        ))
                        .child(
                            self.with_tip(
                                "notifications",
                                Tip::text("Notification Settings", Side::Bottom),
                                tool(if muted { "bell_muted" } else { "bell" }),
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tip = None;
                                this.notify_open = !this.notify_open;
                                this.notify_submenu = None;
                                cx.notify();
                            })),
                        )
                        .child(
                            self.with_tip(
                                "pins",
                                Tip::text("Pinned Messages", Side::Bottom),
                                tool("pin"),
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tip = None;
                                this.toggle_pins("pins", cx);
                            })),
                        )
                        .child(
                            div()
                                .id("toggle-members")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_members = !this.show_members;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .size(px(32.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "members",
                                            20.,
                                            if self.show_members {
                                                "--text-strong"
                                            } else {
                                                "--text-muted"
                                            },
                                        )),
                                ),
                        ),
                )
            })
            .child(
                div()
                    .ml(px(12.))
                    .parity_with_border("search", 1.)
                    .w(px(244.))
                    .h(px(32.))
                    .rounded(px(8.))
                    .bg(color("--input-background-default"))
                    .border_1()
                    .border_color(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .pl(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--input-placeholder-text-default"))
                    .child(search)
                    .child(div().flex_1())
                    .child(div().mr(px(5.)).child(icon(
                        "search",
                        16.,
                        "--interactive-text-default",
                    ))),
            )
    }

    /// What a mention or stamp shows, resolved against the stores.
    fn span_label(&self, span: &Span) -> String {
        match &span.kind {
            Kind::User(id) => {
                let user = self.stores.users.get(id).or(self
                    .stores
                    .current_user
                    .as_ref()
                    .filter(|user| user.id == *id));
                format!("@{}", user.map_or("unknown-user", User::display_name))
            }
            Kind::Role(id) => {
                let role = self.current_guild().and_then(|guild| guild.roles.get(id));
                format!("@{}", role.map_or("deleted-role", String::as_str))
            }
            Kind::Channel(id) => self
                .current_guild()
                .and_then(|guild| guild.channels.iter().find(|channel| channel.id == *id))
                .map_or_else(|| "unknown".to_owned(), |channel| channel.name.clone()),
            Kind::Emoji { name, .. } => format!(":{name}:"),
            Kind::Timestamp { unix, format } => format_timestamp(*unix, *format),
            Kind::Text | Kind::Link(_) | Kind::Everyone | Kind::Unicode => span.text.clone(),
        }
    }

    fn piece(&self, span: &Span, base: Hsla, size: f32, weight: f32, emoji_size: f32) -> Piece {
        let style = span.style;
        let mention = matches!(
            span.kind,
            Kind::User(_) | Kind::Role(_) | Kind::Channel(_) | Kind::Everyone
        );
        let mut run_font: Font = font(if style.code { MONO } else { SANS });
        run_font.features = kerning();
        run_font.weight = match (style.bold, mention) {
            (true, _) => FontWeight(700.),
            (false, true) => FontWeight(500.),
            (false, false) => FontWeight(weight),
        };
        run_font.style = if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        let tint = match span.kind {
            Kind::Link(_) => hsla("--text-link"),
            _ if mention => hsla("--mention-foreground"),
            // A hidden spoiler paints its text in the box's own color.
            _ if style.spoiler => hsla("--spoiler-hidden-background"),
            _ => base,
        };
        let boxed = if style.code {
            Some(inline_code_box())
        } else if style.spoiler {
            Some(pill(0., 4., "--spoiler-hidden-background"))
        } else if matches!(span.kind, Kind::Channel(_)) {
            let mut channel = pill(2., 3., "--mention-background");
            channel.icon = Some(("icons/text_channel.svg".into(), px(16.), px(20.)));
            Some(channel)
        } else if mention {
            Some(pill(2., 3., "--mention-background"))
        } else if matches!(span.kind, Kind::Timestamp { .. }) {
            Some(pill(2., 3., "--interactive-background-active"))
        } else {
            None
        };
        let emoji_key = match &span.kind {
            Kind::Unicode => Some(format!("emoji:{}", span.text)),
            Kind::Emoji { id, .. } => Some(format!("custom:{id}")),
            _ => None,
        };
        let image = emoji_key
            .and_then(|key| self.images.get(&key).cloned())
            .map(|image| (image, px(emoji_size)));
        Piece {
            text: self.span_label(span),
            font: run_font,
            size: if style.code {
                px(size * 0.85)
            } else {
                px(size)
            },
            color: tint,
            underline: style.underline,
            strike: style.strike,
            boxed,
            image,
        }
    }

    fn spans(&self, spans: &[Span], text_color: Hsla, key: (u64, u32)) -> Inline {
        self.spans_edited(spans, text_color, key, false)
    }

    /// A message's last paragraph ends in `(edited)` when the message was edited: `edited_2i8Bfm`, 10px in
    /// `--chat-text-muted`, after a space in the message's own size.
    fn spans_edited(
        &self,
        spans: &[Span],
        text_color: Hsla,
        key: (u64, u32),
        edited: bool,
    ) -> Inline {
        let jumbo = dc_markdown::is_jumbo(spans);
        let emoji_size = if jumbo { 48. } else { 22. };
        // A spoiler is a run of spoiler spans; clicking any of them opens the run, keyed by its first span.
        let mut run_start = vec![0; spans.len()];
        for index in 0..spans.len() {
            run_start[index] =
                if index > 0 && spans[index].style.spoiler && spans[index - 1].style.spoiler {
                    run_start[index - 1]
                } else {
                    index
                };
        }
        let mut pieces: Vec<Piece> = spans
            .iter()
            .enumerate()
            .map(|(index, span)| {
                let open =
                    span.style.spoiler && self.revealed.contains(&(key.0, key.1, run_start[index]));
                let mut piece = self.piece(span, text_color, 16., 400., emoji_size);
                if open {
                    piece.color = text_color;
                    piece.boxed = Some(pill(0., 4., "--__spoiler-background-color--revealed"));
                }
                piece
            })
            .collect();
        let spoilers: Vec<Option<usize>> = spans
            .iter()
            .enumerate()
            .map(|(index, span)| span.style.spoiler.then_some(run_start[index]))
            .collect();
        if edited {
            let plain = Span {
                text: " ".to_owned(),
                style: dc_markdown::Style::default(),
                kind: Kind::Text,
            };
            pieces.push(self.piece(&plain, text_color, 16., 400., emoji_size));
            let mut label = self.piece(&plain, hsla("--chat-text-muted"), 10., 400., emoji_size);
            label.text = "(edited)".to_owned();
            pieces.push(label);
        }
        let line = if jumbo { px(48.) } else { px(22.) };
        let mut element =
            inline(pieces, line).selectable(key, self.selection.clone(), hsla_from(SELECTION));
        let links = link_runs(spans);
        if spoilers.iter().any(Option::is_some) || links.iter().any(Option::is_some) {
            let root = self.weak.clone();
            element = element.on_piece_click(move |piece, _, cx| {
                let _ = root.update(cx, |this, cx| {
                    // A hidden spoiler takes the first click; a link inside it follows on the next.
                    if let Some(Some(start)) = spoilers.get(piece).copied()
                        && this.revealed.insert((key.0, key.1, start))
                    {
                        cx.notify();
                        return;
                    }
                    if let Some(Some((url, label))) = links.get(piece).cloned() {
                        this.follow_link(&url, &label, cx);
                    }
                });
            });
        }
        element
    }

    /// `h1`, `h2` and `h3` from the capture: size and line height per level, all bold.
    fn heading(&self, spans: &[Span], level: u8, key: (u64, u32)) -> Inline {
        let (size, line) = match level {
            1 => (24., 33.),
            2 => (20., 27.5),
            _ => (16., 22.),
        };
        let strong = hsla("--text-strong");
        let pieces = spans
            .iter()
            .map(|span| {
                let mut piece = self.piece(span, strong, size, 700., size * 1.375);
                piece.font.weight = FontWeight(700.);
                piece
            })
            .collect();
        inline(pieces, px(line)).selectable(key, self.selection.clone(), hsla_from(SELECTION))
    }

    fn subtext(&self, spans: &[Span], key: (u64, u32)) -> Inline {
        let muted = hsla("--interactive-text-default");
        let pieces = spans
            .iter()
            .map(|span| self.piece(span, muted, 14., 400., 19.))
            .collect();
        inline(pieces, px(19.25)).selectable(key, self.selection.clone(), hsla_from(SELECTION))
    }

    /// The official pills: 30px tall, a 20px emoji, the count 6px after it, 4px between pills.
    fn reactions(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pills: Vec<AnyElement> = message
            .reactions
            .iter()
            .enumerate()
            .map(|(index, reaction)| {
                let image = self
                    .images
                    .get(&format!("emoji:{}", reaction.name))
                    .cloned();
                let (channel, id, emoji, add) = (
                    message.channel_id,
                    message.id,
                    reaction_route_name(reaction),
                    !reaction.me,
                );
                let group = SharedString::from(format!("reaction-{}-{index}", message.id));
                div()
                    .id(group.clone())
                    .group(group.clone())
                    .h(px(30.))
                    .mr(px(4.))
                    .mb(px(4.))
                    .px(px(6.))
                    .rounded(px(8.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .cursor_pointer()
                    .border_1()
                    .when(reaction.me, |this| {
                        this.bg(color("--reaction-background-reacted-default"))
                            .border_color(color("--reaction-border-reacted-default"))
                    })
                    .when(!reaction.me, |this| {
                        this.bg(color("--reaction-background-default"))
                            .border_color(gpui::transparent_black())
                            .hover(|this| {
                                this.bg(color("--reaction-background-hover"))
                                    .border_color(color("--reaction-border-hover"))
                            })
                    })
                    .on_click(cx.listener(move |this, _, _, _| {
                        if add {
                            this.track_reaction(&emoji);
                        }
                        let _ = this.requests.send(Request::React {
                            channel,
                            message: id,
                            emoji: emoji.clone(),
                            add,
                            location: "Message",
                            burst: false,
                        });
                    }))
                    .child(
                        div()
                            .size(px(20.))
                            .children(image.map(|image| img(image).size(px(20.)))),
                    )
                    .child(
                        div()
                            .ml(px(6.))
                            .min_w(px(12.))
                            .flex()
                            .justify_center()
                            .text_size(px(16.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(if reaction.me {
                                color("--reaction-text-reacted-default")
                            } else {
                                color("--text-subtle")
                            })
                            .when(!reaction.me, |this| {
                                this.group_hover(group.clone(), |this| {
                                    this.text_color(color("--reaction-text-hover"))
                                })
                            })
                            .child(reaction.count.to_string()),
                    )
                    .into_any_element()
            })
            .collect();
        (!pills.is_empty()).then(|| {
            // `forceShow` in module 367790: the add button always follows the pills.
            let add = div()
                .id(SharedString::from(format!("reaction-add-{}", message.id)))
                .relative()
                .h(px(30.))
                .w(px(34.))
                .mb(px(4.))
                .mr(px(4.))
                .rounded(px(8.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .border_1()
                .border_color(gpui::transparent_black())
                .bg(color("--reaction-background-default"))
                .hover(|this| {
                    this.bg(color("--reaction-background-hover"))
                        .border_color(color("--reaction-border-hover"))
                })
                .child(self.anchor_probe(format!("reaction-add-{}", message.id)))
                .on_click({
                    let key = format!("reaction-add-{}", message.id);
                    let channel = message.channel_id;
                    let target = message.id;
                    cx.listener(move |this, _, window, cx| {
                        let anchor = this.tip_anchors.borrow().get(key.as_str()).copied();
                        if let Some(bounds) = anchor {
                            this.open_picker(
                                picker::Target::Reaction {
                                    channel,
                                    message: target,
                                },
                                gpui::point(
                                    bounds.right() + px(PICKER_REACTION_OFFSET),
                                    bounds.top(),
                                ),
                                window,
                                cx,
                            );
                        }
                    })
                })
                .child(icon("msg_react", 18., "--reaction-text-default"));
            div()
                .pt(px(2.))
                .pb(px(2.))
                .flex()
                .flex_wrap()
                .items_center()
                .children(pills)
                .child(add)
                .into_any_element()
        })
    }

    /// Images in their grid: 2px above, 4px between, natural size up to the official limits, 8px corners.
    fn attachments(&self, message: &Message) -> Option<AnyElement> {
        let images: Vec<AnyElement> = message
            .attachments
            .iter()
            .filter(|attachment| attachment.is_image())
            .map(|attachment| {
                let (width, height) = fit_media(
                    attachment.width.unwrap_or(1),
                    attachment.height.unwrap_or(1),
                );
                let (width, height) = (width as f32, height as f32);
                let shown = width.min(self.content_width.max(1.));
                let (width, height) = (px(shown), px(height * shown / width));
                let image = self
                    .images
                    .get(&format!("attachment:{}", attachment.id))
                    .cloned();
                div()
                    .w(width)
                    .h(height)
                    .rounded(px(8.))
                    .bg(color("--background-base-low"))
                    .children(image.map(|image| img(image).w(width).h(height).rounded(px(8.))))
                    .into_any_element()
            })
            .collect();
        (!images.is_empty()).then(|| {
            // 3px under each image is the inline-block baseline gap of the official mosaic.
            div()
                .pb(px(3.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(images)
                .into_any_element()
        })
    }

    /// `container_YUQhib`: pictures, files and embeds in one grid, 2px above and below, 4px between them.
    /// A forward (`container_4dy_KE`): a 4px quote bar, "Forwarded" in italic beside its arrow, the original's
    /// content and files, and a footer naming where it came from, which jumps there.
    fn forwarded_block(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        let forwarded = message.forwarded.as_ref()?;
        // The copy renders as a message of its own so content, files and embeds look the same.
        let copy = Message {
            content: forwarded.content.clone(),
            attachments: forwarded.attachments.clone(),
            embeds: forwarded.embeds.clone(),
            reactions: Vec::new(),
            edited: false,
            forwarded: None,
            ..message.clone()
        };
        let source = forwarded
            .channel_id
            .and_then(|channel| {
                self.stores
                    .guilds
                    .iter()
                    .flat_map(|guild| &guild.channels)
                    .find(|known| known.id == channel)
            })
            .map(|channel| format!("#{}", channel.name));
        // Only a guild channel this client knows gets the footer; a DM source shows none.
        let time = header_time(forwarded.local_time(), chrono::Local::now());
        let footer = source.map(|name| format!("{name}  \u{2022}  {time}"));
        let (guild, channel) = (forwarded.guild_id, forwarded.channel_id);
        Some(
            div()
                .py(px(2.))
                .flex()
                .child(
                    div()
                        .flex_none()
                        .w(px(4.))
                        .rounded(px(2.))
                        .bg(color("--border-subtle")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .pl(px(12.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mt(px(2.))
                                .h(px(18.))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(icon("forwarded_icon", 16., "--text-muted"))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .italic()
                                        .text_color(color("--text-muted"))
                                        .child("Forwarded"),
                                ),
                        )
                        .when(!copy.content.is_empty(), |this| {
                            this.child(self.content(&copy))
                        })
                        .children(self.accessories(&copy))
                        .children(footer.map(|footer| {
                            div()
                                .id(SharedString::from(format!(
                                    "forwarded-footer-{}",
                                    message.id
                                )))
                                .mt(px(4.))
                                .py(px(1.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    match (guild, channel) {
                                        (Some(guild), Some(channel)) => {
                                            this.select(guild, Some(channel))
                                        }
                                        (None, Some(channel)) => this.select_home(Some(channel)),
                                        _ => {}
                                    }
                                    cx.notify();
                                }))
                                .child(footer)
                                .child(icon("forwarded_chevron", 12., "--text-muted"))
                        })),
                )
                .into_any_element(),
        )
    }

    fn accessories(&self, message: &Message) -> Option<AnyElement> {
        let parts: Vec<AnyElement> = [
            self.attachments(message),
            self.files(message),
            self.embeds(message),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| {
            div()
                .py(px(2.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(parts)
                .into_any_element()
        })
    }

    fn content(&self, message: &Message) -> impl IntoElement {
        let text = hsla("--text-default");
        let blocks = parse(&message.content);
        let gaps = block_gaps(&blocks);
        // The last item's bottom margin stays inside the message.
        let trailing = if matches!(blocks.last(), Some(Block::ListItem { .. })) {
            4.
        } else {
            0.
        };
        let last = blocks.len().saturating_sub(1);
        div().flex().flex_col().pb(px(trailing)).children(
            blocks
                .into_iter()
                .zip(gaps)
                .enumerate()
                .map(|(index, (block, gap))| {
                    let key = (message.id, index as u32);
                    match block {
                        Block::Paragraph(spans) => div()
                            .min_h(px(22.))
                            .mt(px(gap))
                            .child(self.spans_edited(
                                &spans,
                                text,
                                key,
                                message.edited && index == last,
                            ))
                            .into_any_element(),
                        Block::Heading { level, spans } => div()
                            .mt(px(gap))
                            .child(self.heading(&spans, level, key))
                            .into_any_element(),
                        Block::Subtext(spans) => div()
                            .mt(px(gap))
                            .child(self.subtext(&spans, key))
                            .into_any_element(),
                        Block::ListItem {
                            depth,
                            number,
                            spans,
                        } => {
                            let indent = 16. * (f32::from(depth) + 1.);
                            let marker = match number {
                                Some(number) => div()
                                    .absolute()
                                    .left(px(-18.))
                                    .top(px(0.))
                                    .line_height(px(22.))
                                    .text_color(color("--text-default"))
                                    .child(format!("{number}."))
                                    .into_any_element(),
                                None => {
                                    let disc = div()
                                        .absolute()
                                        .left(px(-17.))
                                        .top(px(8.))
                                        .size(px(5.))
                                        .rounded_full();
                                    if depth == 0 {
                                        disc.bg(color("--text-default")).into_any_element()
                                    } else {
                                        disc.border_1()
                                            .border_color(color("--text-default"))
                                            .into_any_element()
                                    }
                                }
                            };
                            div()
                                .relative()
                                .mt(px(gap))
                                .ml(px(indent))
                                .min_h(px(22.))
                                .child(marker)
                                .child(self.spans(&spans, text, key))
                                .into_any_element()
                        }
                        Block::Quote(spans) => div()
                            .mt(px(gap))
                            .flex()
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(4.))
                                    .rounded(px(4.))
                                    .bg(color("--spine-default")),
                            )
                            .child(div().pl(px(12.)).pr(px(8.)).child(self.spans(
                                &spans,
                                hsla("--text-subtle"),
                                key,
                            )))
                            .into_any_element(),
                        Block::Code { language, text } => div()
                            .mt(px(gap))
                            .w(px(640.))
                            .p(px(7.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(color("--background-mod-subtle"))
                            .bg(color("--background-code"))
                            .font_family(MONO)
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-code"))
                            .children(code_lines(language.as_deref(), &text))
                            .into_any_element(),
                    }
                }),
        )
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(index) else {
            return div().into_any_element();
        };
        match row {
            Row::Spacer => div().h(px(40.)).into_any_element(),
            Row::NewDivider => self.new_divider(),
            Row::Blocked { first, count } => self.blocked_row(*first, *count, cx),
            Row::Welcome => self
                .dm_welcome(cx)
                .unwrap_or_else(|| div().into_any_element()),
            Row::Divider(date) => div()
                .pt(px(24.))
                .pl(px(16.))
                .pr(px(30.))
                .child(
                    div()
                        .h(px(1.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(color("--background-mod-subtle"))
                        .child(
                            div()
                                .px(px(4.))
                                .bg(color("--background-base-lower"))
                                .text_size(px(12.))
                                .line_height(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-muted"))
                                .child(date.clone()),
                        ),
                )
                .into_any_element(),
            Row::Message { index, head } => {
                let channel = self.channel.unwrap_or_default();
                let Some(message) = self
                    .stores
                    .messages
                    .get(&channel)
                    .and_then(|messages| messages.get(*index))
                else {
                    return div().into_any_element();
                };
                if dm::is_system(message) {
                    return self.system_message(message, cx);
                }
                // The 17px between groups is outside the hover box, and the box stops 16px short of the edge. Padding,
                // not margin: gpui lists measure an item without its margins.
                let id = message.id;
                let own = self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|user| user.id == message.author.id);
                let hovered = self.hovered == Some(id);
                let editing = self
                    .editing
                    .as_ref()
                    .filter(|(target, _)| *target == id)
                    .map(|(_, editor)| editor.clone());
                let body = div()
                    .id(SharedString::from(format!("message-{id}")))
                    .relative()
                    .pl(px(72.))
                    .pr(px(24.))
                    .pt(px(2.))
                    .pb(px(2.))
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        if *inside {
                            this.hovered = Some(id);
                        } else if this.hovered == Some(id) {
                            this.hovered = None;
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.open_menu(menu::Anchor::Pointer(event.position), id, own, cx);
                        }),
                    )
                    .when(hovered && editing.is_none(), |this| {
                        this.child(self.message_toolbar(message, own, cx))
                    });
                let content = match editing {
                    Some(editor) => self.edit_box(editor).into_any_element(),
                    None => div()
                        .when(!message.content_is_inline_media(), |this| {
                            this.child(self.content(message))
                        })
                        .children(self.forwarded_block(message, cx))
                        .children(self.accessories(message))
                        .children(self.reactions(message, cx))
                        .into_any_element(),
                };
                let row = div().pr(px(16.));
                if !*head {
                    return row.child(body.child(content)).into_any_element();
                }
                let avatar = self.images.get(&avatar_source(&message.author).0).cloned();
                let reply_offset = if message.referenced.is_some() {
                    22.
                } else {
                    0.
                };
                let preview = message.referenced.as_ref().map(|referenced| {
                    div()
                        .mb(px(4.))
                        .child(self.reply_preview(referenced))
                        .into_any_element()
                });
                row.pt(px(17.))
                    .child(
                        body.children(preview)
                            .child(
                                div()
                                    .absolute()
                                    .left(px(16.))
                                    .top(px(4. + reply_offset))
                                    .size(px(40.))
                                    .child(self.profile_trigger(
                                        format!("avatar-{id}"),
                                        message.author.id,
                                        div().size(px(40.)).children(
                                            avatar.map(|image| {
                                                img(image).size(px(40.)).rounded_full()
                                            }),
                                        ),
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .child(
                                        self.profile_trigger(
                                            format!("name-{id}"),
                                            message.author.id,
                                            div()
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-strong"))
                                                .line_height(px(22.))
                                                .child(message.author.display_name().to_owned()),
                                            cx,
                                        ),
                                    )
                                    .child(
                                        div()
                                            .ml(px(8.))
                                            .text_size(px(12.))
                                            .line_height(px(22.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--channels-default"))
                                            .child(header_time(
                                                message.local_time(),
                                                chrono::Local::now(),
                                            )),
                                    ),
                            )
                            .child(content),
                    )
                    .into_any_element()
            }
        }
    }

    /// Own messages offer Edit; the official bar is at -25px, 14px in from the row's right edge.
    fn message_toolbar(
        &self,
        message: &Message,
        own: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = message.id;
        let text = message.content.clone();
        let quick: Vec<&'static dc_tokens::emoji_data::Emoji> =
            self.quick_reactions().into_iter().take(3).collect();
        let has_quick = !quick.is_empty();
        let button = |name: &str| {
            div()
                .size(px(28.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .child(icon(name, 20., "--interactive-text-default"))
        };
        div()
            .id(SharedString::from(format!("toolbar-{id}")))
            .occlude()
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                // The bar reaches above its row, so it has to keep the row's hover alive.
                if *inside {
                    this.hovered = Some(id);
                    cx.notify();
                }
            }))
            .absolute()
            .top(px(-25.))
            .right(px(14.))
            .h(px(34.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--background-mod-subtle"))
            .flex()
            .items_center()
            // The three most frecent reactions, then a separator (`separator_DpHsW6`).
            .children(quick.into_iter().enumerate().map(|(index, emoji)| {
                let picture = self
                    .images
                    .get(&format!("emoji:{}", emoji.surrogates))
                    .cloned();
                let channel = message.channel_id;
                div()
                    .id(SharedString::from(format!("toolbar-quick-{index}")))
                    .size(px(28.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .children(picture.map(|picture| img(picture).size(px(20.))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.track_reaction(emoji.surrogates);
                        let _ = this.requests.send(Request::React {
                            channel,
                            message: id,
                            emoji: emoji.surrogates.to_owned(),
                            add: true,
                            location: "Message Hover Bar",
                            burst: false,
                        });
                        cx.notify();
                    }))
            }))
            .when(has_quick, |this| {
                this.child(
                    div()
                        .mx(px(4.))
                        .w(px(1.))
                        .h(px(24.))
                        .rounded_full()
                        .bg(color("--border-subtle")),
                )
            })
            .child(
                self.with_tip(
                    "toolbar-react",
                    Tip::text("Add Reaction", Side::Top),
                    button("msg_react"),
                    cx,
                )
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    let anchor = this.tip_anchors.borrow().get("toolbar-react").copied();
                    if let (Some(bounds), Some(channel)) = (anchor, this.channel) {
                        this.tip = None;
                        this.open_picker(
                            picker::Target::Reaction {
                                channel,
                                message: id,
                            },
                            gpui::point(bounds.left(), bounds.top()),
                            window,
                            cx,
                        );
                    }
                })),
            )
            .when(own, |this| {
                this.child(
                    div()
                        .id("toolbar-edit")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.begin_edit(id, &text, window, cx);
                        }))
                        .child(self.with_tip(
                            "toolbar-edit-tip",
                            Tip::text("Edit", Side::Top),
                            button("msg_edit"),
                            cx,
                        )),
                )
            })
            .child(
                self.with_tip(
                    "toolbar-forward",
                    Tip::text("Forward", Side::Top),
                    button("msg_forward"),
                    cx,
                )
                .cursor_pointer()
                .on_click({
                    let found = message.clone();
                    cx.listener(move |this, _, window, cx| {
                        this.tip = None;
                        this.open_forward(found.clone(), window, cx);
                    })
                }),
            )
            .child(
                self.with_tip(
                    "toolbar-more",
                    Tip::text("More", Side::Top),
                    button("msg_more"),
                    cx,
                )
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    let anchor = this.tip_anchors.borrow().get("toolbar-more").copied();
                    if let Some(bounds) = anchor {
                        this.tip = None;
                        this.open_menu(
                            menu::Anchor::MoreButton(gpui::point(bounds.left(), bounds.top())),
                            id,
                            own,
                            cx,
                        );
                    }
                })),
            )
    }

    fn edit_box(&self, editor: Entity<Editor>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mt(px(8.))
                    .w(px(795.))
                    .min_h(px(58.))
                    .rounded(px(8.))
                    .bg(color("--chat-background-default"))
                    .border_1()
                    .border_color(color("--background-mod-subtle"))
                    .px(px(15.))
                    .py(px(17.))
                    .child(editor.clone()),
            )
            .child(
                div()
                    .h(px(26.))
                    .pt(px(7.))
                    .text_size(px(12.))
                    .line_height(px(12.))
                    .text_color(color("--text-default"))
                    .flex()
                    .child("escape to ")
                    .child(
                        div()
                            .id("edit-cancel")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .on_click({
                                let root = self.weak.clone();
                                move |_, _, cx| {
                                    let _ = root.update(cx, |this, cx| {
                                        this.editing = None;
                                        cx.notify();
                                    });
                                }
                            })
                            .child("cancel"),
                    )
                    .child(" \u{2022} enter to ")
                    .child(
                        div()
                            .id("edit-save")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .on_click({
                                let editor = editor.clone();
                                move |_, _, cx| editor.update(cx, |editor, cx| editor.submit(cx))
                            })
                            .child("save"),
                    ),
            )
    }

    fn composer_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let prefix = if self.guild.is_some() { "#" } else { "@" };
        let placeholder = format!("Message {prefix}{}", self.channel_title());
        self.composer
            .update(cx, |composer, _| composer.set_placeholder(placeholder));
        let reply_bar = self.replying.as_ref().map(|message| {
            div()
                .h(px(36.))
                .rounded_t(px(8.))
                .bg(color("--background-surface-higher"))
                .flex()
                .items_center()
                .pl(px(16.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child("Replying to ")
                .child(
                    div()
                        .font_weight(FontWeight(600.))
                        .child(message.author.display_name().to_owned()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .id("reply-close")
                        .w(px(40.))
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.replying = None;
                            cx.notify();
                        }))
                        .child(icon("reply_close", 16., "--interactive-text-default")),
                )
        });
        let emoji = self.images.get("emoji_button").cloned();
        let emoji_button = div()
            .id("composer-emoji")
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                let composer = this.tip_anchors.borrow().get("composer-box").copied();
                if let Some(bounds) = composer {
                    this.open_picker(
                        picker::Target::Composer,
                        // The probe sits inside the composer's 1px border.
                        gpui::point(bounds.left(), bounds.top() - px(9.)),
                        window,
                        cx,
                    );
                }
            }))
            .children(emoji.map(|image| img(image).size(px(18.))));
        let tool = |name: &'static str| {
            div()
                .id(name)
                .size(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(icon(name, 20., "--interactive-text-default"))
        };
        // GIF opens the expression picker on its tab, from the same place as the emoji button.
        let gif_button = tool("gif").on_click(cx.listener(move |this, _, window, cx| {
            let composer = this.tip_anchors.borrow().get("composer-box").copied();
            if let Some(bounds) = composer {
                this.open_picker_tab(
                    picker::Tab::Gifs,
                    gpui::point(bounds.left(), bounds.top() - px(9.)),
                    window,
                    cx,
                );
            }
        }));
        let gift_button = tool("gift");
        let sticker_button = tool("sticker");
        let apps_button = tool("apps");
        // The form overlaps the list by 16px, as the official layout does.
        div()
            .relative()
            .flex_none()
            .mt(px(-16.))
            .px(px(8.))
            .pb(px(8.))
            .children(self.typing_bar())
            .child(
                div()
                    .parity_with_border("composer", 1.)
                    .relative()
                    .child(self.anchor_probe("composer-box"))
                    .min_h(px(58.))
                    .rounded(px(8.))
                    .bg(color("--chat-background-default"))
                    .border_1()
                    .border_color(color("--background-mod-subtle"))
                    .flex()
                    .flex_col()
                    .children(self.upload_area(cx))
                    .children(reply_bar)
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .pl(px(11.))
                            .pr(px(12.))
                            .child(
                                div()
                                    .id("attach")
                                    .pt(px(12.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| this.pick_files(cx)))
                                    .child(tool("attach")),
                            )
                            .child(
                                div()
                                    .ml(px(20.))
                                    .flex_1()
                                    .min_w(px(0.))
                                    .py(px(17.))
                                    .parity("composer-text")
                                    .child(self.composer.clone()),
                            )
                            .child(
                                div()
                                    .pt(px(12.))
                                    .flex()
                                    .gap(px(8.))
                                    .child(gift_button)
                                    .child(gif_button)
                                    .child(sticker_button)
                                    .child(emoji_button)
                                    .child(apps_button),
                            ),
                    ),
            )
    }
}

fn editor_style() -> EditorStyle {
    let mut text_font: Font = font(SANS);
    text_font.features = kerning();
    EditorStyle {
        font: text_font,
        size: crate::editor::FONT_SIZE,
        line_height: crate::editor::LINE_HEIGHT,
        text: hsla("--text-default"),
        placeholder: hsla("--channel-text-area-placeholder"),
        selection: hsla_from(SELECTION),
        caret: hsla("--text-default"),
    }
}

/// A message's text as copied: its blocks, one per line.
/// The spans of the first text block, for one-line previews of a message.
fn first_spans(content: &str) -> Vec<Span> {
    parse(content)
        .into_iter()
        .find_map(|block| match block {
            Block::Paragraph(spans)
            | Block::Quote(spans)
            | Block::Subtext(spans)
            | Block::Heading { spans, .. }
            | Block::ListItem { spans, .. } => Some(spans),
            Block::Code { .. } => None,
        })
        .unwrap_or_default()
}

/// For each span, the link it belongs to: its address and the whole run's visible text (a masked link's label can span
/// several styled spans).
fn link_runs(spans: &[Span]) -> Vec<Option<(String, String)>> {
    let mut out = vec![None; spans.len()];
    let mut index = 0;
    while index < spans.len() {
        let Kind::Link(url) = &spans[index].kind else {
            index += 1;
            continue;
        };
        let start = index;
        let mut label = String::new();
        while index < spans.len() && matches!(&spans[index].kind, Kind::Link(other) if other == url)
        {
            label.push_str(&spans[index].text);
            index += 1;
        }
        for slot in &mut out[start..index] {
            *slot = Some((url.clone(), label.clone()));
        }
    }
    out
}

fn parse_block_text(content: &str) -> String {
    parse(content)
        .iter()
        .map(block_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The route form of a reaction: the emoji itself, or `name:id` for a custom one.
fn reaction_route_name(reaction: &dc_state::Reaction) -> String {
    match reaction.emoji_id {
        Some(id) => format!("{}:{id}", reaction.name),
        None => reaction.name.clone(),
    }
}

fn block_spans(block: &Block) -> &[Span] {
    match block {
        Block::Paragraph(spans)
        | Block::Quote(spans)
        | Block::Subtext(spans)
        | Block::Heading { spans, .. }
        | Block::ListItem { spans, .. } => spans,
        Block::Code { .. } => &[],
    }
}

/// Plain text of a block, the same bytes the inline engine lays out.
fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph(spans)
        | Block::Quote(spans)
        | Block::Subtext(spans)
        | Block::Heading { spans, .. }
        | Block::ListItem { spans, .. } => spans.iter().map(|span| span.text.as_str()).collect(),
        Block::Code { text, .. } => text.clone(),
    }
}

/// The space above each block. CSS margins collapse, so a pair of neighbors takes the larger of its two.
fn block_gaps(blocks: &[Block]) -> Vec<f32> {
    let top = |block: &Block| match block {
        Block::Heading { level: 1, .. } => 8.,
        Block::Heading { .. } => 16.,
        Block::Quote(_) | Block::ListItem { .. } => 4.,
        Block::Code { .. } => 6.,
        Block::Paragraph(_) | Block::Subtext(_) => 0.,
    };
    let bottom = |block: &Block| match block {
        Block::Heading { .. } => 8.,
        Block::Quote(_) | Block::ListItem { .. } => 4.,
        _ => 0.,
    };
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| match index.checked_sub(1) {
            Some(previous) => f32::max(top(block), bottom(&blocks[previous])),
            None => top(block),
        })
        .collect()
}

/// A rounded box behind inline text: mentions, stamps and spoilers.
fn pill(padding: f32, radius: f32, background: &str) -> BoxStyle {
    BoxStyle {
        icon: None,
        padding_x: px(padding),
        height: px(21.),
        top: px(0.),
        background: hsla(background),
        border: Hsla::transparent_black(),
        radius: px(radius),
        text_line_height: px(22.),
        text_top: px(0.),
    }
}

/// `<t:...>` styles, formatted for en-US as the official client does.
fn format_timestamp(unix: i64, style: char) -> String {
    let Some(time) = chrono::DateTime::from_timestamp(unix, 0) else {
        return unix.to_string();
    };
    let local = time.with_timezone(&chrono::Local);
    match style {
        't' => local.format("%-I:%M %p").to_string(),
        'T' => local.format("%-I:%M:%S %p").to_string(),
        'd' => local.format("%m/%d/%Y").to_string(),
        'D' => local.format("%B %-d, %Y").to_string(),
        'F' => local.format("%A, %B %-d, %Y %-I:%M %p").to_string(),
        'R' => relative_time(chrono::Utc::now().timestamp() - unix),
        _ => local.format("%B %-d, %Y %-I:%M %p").to_string(),
    }
}

/// moment's relative time with its default thresholds, which is what `R` uses.
fn relative_time(seconds_ago: i64) -> String {
    let (seconds, past) = (seconds_ago.abs(), seconds_ago >= 0);
    let minutes = (seconds + 30) / 60;
    let hours = (seconds + 1800) / 3600;
    let days = (seconds + 43200) / 86400;
    let months = (days as f64 / 30.4).round() as i64;
    let years = (days as f64 / 365.).round() as i64;
    let span = match () {
        () if seconds < 45 => "a few seconds".to_owned(),
        () if seconds < 90 => "a minute".to_owned(),
        () if minutes < 45 => format!("{minutes} minutes"),
        () if minutes < 90 => "an hour".to_owned(),
        () if hours < 22 => format!("{hours} hours"),
        () if hours < 36 => "a day".to_owned(),
        () if days < 26 => format!("{days} days"),
        () if days < 46 => "a month".to_owned(),
        () if days < 320 => format!("{months} months"),
        () if days < 548 => "a year".to_owned(),
        () => format!("{years} years"),
    };
    if past {
        format!("{span} ago")
    } else {
        format!("in {span}")
    }
}

/// `code.inline` from the captured DOM: 0.2em padding plus the border, 20px tall, 1px below the line top.
fn inline_code_box() -> BoxStyle {
    BoxStyle {
        icon: None,
        padding_x: px(3.72),
        height: px(20.),
        top: px(1.),
        background: hsla("--background-code"),
        border: hsla("--background-mod-strong"),
        radius: px(4.),
        text_line_height: px(18.),
        text_top: px(2.),
    }
}

/// Inline code inside a reply preview: 0.85 of 14px, so its box is smaller than in the message.
fn preview_code_box() -> BoxStyle {
    BoxStyle {
        icon: None,
        padding_x: px(3.38),
        height: px(18.),
        top: px(0.),
        background: hsla("--background-code"),
        border: hsla("--background-mod-strong"),
        radius: px(4.),
        text_line_height: px(18.),
        text_top: px(0.),
    }
}

/// Each line as styled runs in the code font, colored from the official `--text-code-*` tokens.
fn code_lines(language: Option<&str>, text: &str) -> Vec<AnyElement> {
    let mut code_font: Font = font(MONO);
    code_font.features = kerning();
    text.lines()
        .zip(highlight(language, text))
        .map(|(line, spans)| {
            let runs = spans
                .into_iter()
                .map(|(len, token)| TextRun {
                    len,
                    font: code_font.clone(),
                    color: hsla(token.css_name()),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                })
                .collect();
            div()
                .min_h(px(18.))
                .child(StyledText::new(line.to_owned()).with_runs(runs))
                .into_any_element()
        })
        .collect()
}

/// Minimize, maximize and close. They sit in the official `systemBar`, above every layer and its scrim.
fn window_controls() -> impl IntoElement {
    let control = |id: &'static str, name: &str, area: WindowControlArea| {
        div()
            .id(id)
            .w(px(32.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(area)
            .child(icon(name, 12., "--interactive-text-default"))
    };
    div()
        .flex()
        .gap(px(4.))
        .child(control("min", "minimize", WindowControlArea::Min))
        .child(control("max", "maximize", WindowControlArea::Max))
        .child(control("close", "close", WindowControlArea::Close))
}

fn avatar_source(user: &User) -> (String, String) {
    match &user.avatar {
        Some(hash) => (
            format!("avatar:{}:{hash}", user.id),
            format!(
                "https://cdn.discordapp.com/avatars/{}/{hash}.webp?size=80",
                user.id
            ),
        ),
        None => {
            let index = user.default_avatar_index();
            (
                format!("avatar:default:{index}"),
                format!("https://cdn.discordapp.com/embed/avatars/{index}.png"),
            )
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The chat column less the message's 72px gutter and its 24 + 16px right padding.
        // The member list in a guild, the profile beside a DM.
        let members = if self.current_dm().is_some() && self.show_dm_profile {
            320.
        } else if self.show_members && self.guild.is_some() {
            264.
        } else {
            0.
        };
        self.content_width = f32::from(window.viewport_size().width) - 375. - members - 112.;
        let rows = list(
            self.list.clone(),
            cx.processor(|this, index, _, cx| this.row(index, cx)),
        )
        .size_full();
        div()
            .id("root")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Escape closes settings before the focused field, which would take it for itself, sees it.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let keys = &event.keystroke;
                // `QUICKSWITCHER_SHOW`: Ctrl+K anywhere.
                if keys.key == "k"
                    && keys.modifiers.control
                    && !keys.modifiers.shift
                    && !keys.modifiers.alt
                {
                    this.open_quick_switcher(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.group_dm.is_some() && this.group_dm_key(&keys.key, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.quick_switcher.is_some() {
                    if keys.key == "escape" {
                        this.quick_switcher = None;
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                    if this.switcher_key(&keys.key, cx) {
                        cx.stop_propagation();
                        return;
                    }
                }
                // `NAVIGATE_BACK`/`NAVIGATE_FORWARD`: global combos, so they work from the composer too.
                let alt_only = keys.modifiers.alt
                    && !keys.modifiers.control
                    && !keys.modifiers.shift
                    && !keys.modifiers.platform;
                if alt_only && (keys.key == "left" || keys.key == "right") {
                    if keys.key == "left" {
                        this.go_back(cx);
                    } else {
                        this.go_forward(cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape" && this.inbox.is_some() {
                    this.inbox = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape"
                    && (this.friend_menu.is_some() || this.confirm_remove_friend.is_some())
                {
                    this.friend_menu = None;
                    this.confirm_remove_friend = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape" && this.notify_open {
                    this.notify_open = false;
                    this.notify_submenu = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.confirm_unpin.is_some() {
                    this.confirm_unpin = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.pins_open.is_some() {
                    this.pins_open = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.forward.is_some() {
                    this.forward = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.custom_reminder.is_some() {
                    this.custom_reminder = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.submenu.is_some() {
                    this.submenu = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.picker.is_some() {
                    this.picker = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape"
                    && (this.confirm_pin.is_some()
                        || this.leaving.is_some()
                        || this.confirm_delete.is_some()
                        || this.menu.is_some())
                {
                    // The top layer closes first: a modal, then the menu under it.
                    if this.confirm_pin.is_some() {
                        this.confirm_pin = None;
                    } else if this.leaving.is_some() {
                        this.leaving = None;
                    } else if this.confirm_delete.is_some() {
                        this.confirm_delete = None;
                    } else {
                        this.menu = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.profile.is_some() {
                    this.profile = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.settings.is_some() {
                    this.close_settings(cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|this, _, _, cx| this.go_back(cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(|this, _, _, cx| this.go_forward(cx)),
            )
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                this.on_drop_files(paths, cx);
            }))
            // Capture runs before the text under the pointer starts its own selection.
            .capture_any_mouse_down(cx.listener(|this, _, window, _| {
                this.selection.borrow_mut().clear();
                window.refresh();
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(SANS)
            .font_features(kerning())
            .text_size(px(16.))
            .line_height(px(22.))
            .text_color(color("--text-default"))
            .bg(color("--background-base-lowest"))
            .children(self.settings_layer(window, cx))
            .children(self.profile_layer(cx))
            .children(self.tip_layer())
            .child(self.titlebar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.rail(cx))
                    .child(if self.guild.is_some() {
                        self.sidebar(cx).into_any_element()
                    } else {
                        self.home_sidebar(cx).into_any_element()
                    })
                    .child(if self.guild.is_none() && self.channel.is_none() {
                        self.friends_panel(cx).into_any_element()
                    } else {
                        div()
                            .parity("chat")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .pt(px(1.))
                            .child(self.header(cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .bg(color("--background-base-lower"))
                                    .flex_row()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .h_full()
                                            .child(
                                                div()
                                                    .parity("messages")
                                                    .relative()
                                                    .flex_1()
                                                    .min_h(px(0.))
                                                    .child(rows)
                                                    .children(self.new_messages_bar(cx)),
                                            )
                                            .child(
                                                match self
                                                    .current_dm()
                                                    .and_then(|channel| {
                                                        self.stores.dm_recipient(channel)
                                                    })
                                                    .map(|user| user.id)
                                                    .filter(|user| self.is_blocked(*user))
                                                {
                                                    // `wrapper_ST_Y69` sits where the composer's form does: 8px in
                                                    // and over the list's last 16px.
                                                    Some(user) => div()
                                                        .flex_none()
                                                        .mt(px(-16.))
                                                        .px(px(8.))
                                                        .pb(px(8.))
                                                        .child(self.blocked_composer(user, cx))
                                                        .into_any_element(),
                                                    None => {
                                                        self.composer_bar(cx).into_any_element()
                                                    }
                                                },
                                            ),
                                    )
                                    .when(self.show_members && self.guild.is_some(), |this| {
                                        this.child(
                                            div()
                                                .flex_none()
                                                .w(px(1.))
                                                .h_full()
                                                .bg(color("--background-mod-subtle")),
                                        )
                                        .child(self.member_panel(cx))
                                    })
                                    .children(self.dm_profile(cx)),
                            )
                            .into_any_element()
                    }),
            )
            .child(self.user_panel(cx))
            .children(self.context_menu(window, cx))
            .children(self.menu_submenu(window, cx))
            .children(self.delete_modal(cx))
            .children(self.leaving_modal(cx))
            .children(self.pin_modal(cx))
            .children(self.inbox_popout(window, cx))
            .children(self.friend_menu_layer(cx))
            .children(self.remove_friend_modal(cx))
            .children(self.group_dm_modal(window, cx))
            .children(self.quick_switcher_modal(window, cx))
            .children(self.notify_menu(window, cx))
            .children(self.notify_mute_submenu(window, cx))
            .children(self.unpin_modal(cx))
            .children(self.pins_popout(window, cx))
            .children(self.forward_modal(window, cx))
            .children(self.custom_reminder_modal(cx))
            .children(self.toast_layer())
            .children(self.app_toast_layer())
            .children(self.picker_layer(window, cx))
    }
}

pub fn bind_focus(root: &gpui::Entity<Root>, window: &mut Window, cx: &mut App) {
    let handle = root.read(cx).composer.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
}

/// Built-in sections; anything else is a hoisted role, named once roles are stored.
fn group_label(id: &str) -> &str {
    match id {
        "online" => "Online",
        "offline" => "Offline",
        other => other,
    }
}

/// English labels of the status picker. The full catalog comes with `STRINGS/`.
fn status_label(status: &str) -> &'static str {
    match status {
        "dnd" => "Do Not Disturb",
        "idle" => "Idle",
        "invisible" => "Invisible",
        _ => "Online",
    }
}

/// The presence dot in its cut-out ring. Do Not Disturb adds the bar across the dot.
/// The reaction picker opens beside the add button, its right edge this far right of the button's.
const PICKER_REACTION_OFFSET: f32 = picker::PICKER_WIDTH + 8.;

/// `clickable_fErCLM`: a 24px top bar button, `--icon-muted`, on hover `--background-mod-normal` behind
/// `--interactive-icon-hover`.
fn bar_button(id: &'static str, name: &'static str) -> gpui::Stateful<gpui::Div> {
    let group = SharedString::from(format!("bar-{id}"));
    div()
        .id(id)
        .group(group.clone())
        .size(px(24.))
        .rounded(px(6.))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .hover(|style| style.bg(color("--background-mod-normal")))
        .child(
            svg()
                .path(format!("icons/{name}.svg"))
                .size(px(18.))
                .flex_none()
                .text_color(color("--icon-muted"))
                .group_hover(group, |style| {
                    style.text_color(color("--interactive-icon-hover"))
                }),
        )
}

/// The status of a 20px avatar (`svg-mask-avatar-status-round-20`): a 6px dot at (14, 14) in a 10px hole.
fn status_dot_small(status: &str, ring: &str) -> impl IntoElement {
    let tint = match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        _ => "--icon-status-online",
    };
    div()
        .absolute()
        .left(px(12.))
        .top(px(12.))
        .size(px(10.))
        .rounded_full()
        .bg(color(ring))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(color(tint))
                .flex()
                .items_center()
                .justify_center()
                .when(status == "dnd", |this| {
                    this.child(div().w(px(4.)).h(px(1.)).rounded(px(0.5)).bg(color(ring)))
                }),
        )
}

fn status_dot(status: &str, ring: &str) -> impl IntoElement {
    let tint = match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        _ => "--icon-status-online",
    };
    div()
        .absolute()
        .right(px(-3.))
        .bottom(px(-3.))
        .size(px(16.))
        .rounded_full()
        .bg(color(ring))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(10.))
                .rounded_full()
                .bg(color(tint))
                .flex()
                .items_center()
                .justify_center()
                .relative()
                .when(status == "dnd", |this| {
                    this.child(div().w(px(6.)).h(px(2.)).rounded(px(1.)).bg(color(ring)))
                })
                // `svg-mask-status-offline`: a hole of half the dot's size.
                .when(matches!(status, "offline" | "invisible"), |this| {
                    this.child(div().size(px(5.)).rounded_full().bg(color(ring)))
                })
                // `svg-mask-status-idle`: the dot less a circle of radius 0.375 centred at a quarter.
                .when(status == "idle", |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(px(-1.25))
                            .top(px(-1.25))
                            .size(px(7.5))
                            .rounded_full()
                            .bg(color(ring)),
                    )
                }),
        )
}

/// `C(timestamp, true)` in the bundle's date helpers: today is the time alone, yesterday "Yesterday at" the time, and
/// older days the short date and time (moment's `L LT`).
fn header_time(
    local: chrono::DateTime<chrono::Local>,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    let days = (local.date_naive() - now.date_naive()).num_days();
    let time = local.format("%-I:%M %p");
    match days {
        0 => time.to_string(),
        -1 => format!("Yesterday at {time}"),
        days if days < -1 => format!("{} {time}", local.format("%m/%d/%Y")),
        _ => format!("{} {time}", local.format("%m/%d/%Y")),
    }
}

/// The status of an 80px avatar (`svg-mask-avatar-status-round-80`): a 16px dot at (60, 60) in a 24px hole.
fn status_dot_large(status: &str, ring: &str) -> impl IntoElement {
    let tint = match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        _ => "--icon-status-online",
    };
    div()
        .absolute()
        .left(px(56.))
        .top(px(56.))
        .size(px(24.))
        .rounded_full()
        .bg(color(ring))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .bg(color(tint))
                .flex()
                .items_center()
                .justify_center()
                .when(status == "dnd", |this| {
                    this.child(div().w(px(10.)).h(px(3.)).rounded(px(1.5)).bg(color(ring)))
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::{block_gaps, relative_time};
    use dc_markdown::parse;

    #[test]
    fn header_times_follow_the_calendar_rule() {
        use chrono::TimeZone;
        let now = chrono::Local
            .with_ymd_and_hms(2026, 11, 8, 1, 0, 0)
            .unwrap();
        let at = |day, hour, minute| {
            chrono::Local
                .with_ymd_and_hms(2026, 11, day, hour, minute, 0)
                .unwrap()
        };
        assert_eq!(super::header_time(at(8, 0, 30), now), "12:30 AM");
        assert_eq!(
            super::header_time(at(7, 22, 26), now),
            "Yesterday at 10:26 PM"
        );
        assert_eq!(super::header_time(at(5, 9, 5), now), "11/05/2026 9:05 AM");
    }

    #[test]
    fn relative_time_follows_the_captured_wording() {
        assert_eq!(relative_time(90_000), "a day ago");
        assert_eq!(relative_time(10), "a few seconds ago");
        assert_eq!(relative_time(3 * 3600), "3 hours ago");
        assert_eq!(relative_time(-600), "in 10 minutes");
    }

    #[test]
    fn neighboring_margins_collapse_as_in_the_capture() {
        // Heading one, two, three and subtext in the fixture: 8, then 16, 16, 8.
        let blocks = parse("# a\n## b\n### c\n-# d");
        assert_eq!(block_gaps(&blocks), [8., 16., 16., 8.]);
    }

    #[test]
    fn a_quote_keeps_its_margin_on_both_sides() {
        let blocks = parse("> a\nb");
        assert_eq!(block_gaps(&blocks), [4., 4.]);
    }
}
