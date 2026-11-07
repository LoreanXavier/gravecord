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
use dc_state::{Channel, ChannelKind, Guild, Id, Message, Stores, User};
use dc_tokens::Theme;
use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, Focusable, Font, FontFeatures,
    FontStyle, FontWeight, Hsla, Image, ImageFormat, IntoElement, KeyDownEvent, ListAlignment,
    ListState, MouseButton, MouseDownEvent, Render, Rgba, SharedString, StyledText, TextRun,
    Window, WindowControlArea, div, font, img, list, prelude::*, px, rgba, svg,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

mod home;
mod menu;

const SANS: &str = "gg sans";
/// The CSS family is `gg mono`; the font file calls itself `gg sans mono`.
const MONO: &str = "gg sans mono";

/// The page sets `text-rendering: optimizelegibility`. gpui hands DirectWrite an explicit feature list, so `kern` has to
/// be on that list or text comes out wider than the official client's.
fn kerning() -> FontFeatures {
    FontFeatures(Arc::new(vec![("kern".to_owned(), 1)]))
}

fn color(name: &str) -> Rgba {
    rgba(Theme::Darker.color(name))
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
    Divider(String),
    Spacer,
    Message { index: usize, head: bool },
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
    confirm_delete: Option<Id>,
    show_members: bool,
    pub ready: bool,
}

impl Root {
    pub fn new(requests: UnboundedSender<Request>, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Editor::new(editor_style(), cx));
        cx.subscribe(&composer, |this, _, event, _| {
            if let EditorEvent::Submit(content) = event {
                this.send(content.clone());
            }
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
            confirm_delete: None,
            show_members: false,
            ready: false,
        }
    }

    pub fn update(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Dispatch { name, data } => {
                self.stores.dispatch(&name, &data);
                if name == "READY" {
                    self.after_ready();
                }
                if matches!(
                    name.as_str(),
                    "MESSAGE_CREATE" | "MESSAGE_DELETE" | "MESSAGE_UPDATE"
                ) {
                    self.rebuild_rows();
                }
                if name == "GUILD_MEMBER_LIST_UPDATE" {
                    self.request_member_images();
                }
            }
            Update::Messages { channel, page } => {
                self.stores.load_messages(channel, &page);
                self.rebuild_rows();
                self.request_images();
                self.ready = true;
            }
            Update::Image { key, bytes } => {
                let format = if bytes.starts_with(b"\x89PNG") {
                    ImageFormat::Png
                } else if bytes.starts_with(b"RIFF") {
                    ImageFormat::Webp
                } else {
                    ImageFormat::Jpeg
                };
                self.images
                    .insert(key, Arc::new(Image::from_bytes(format, bytes)));
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
        self.request_images();
        let tint = Theme::Darker.color("--interactive-text-default");
        let _ = self.requests.send(Request::Sprite {
            key: "emoji_button".to_owned(),
            tint,
        });
    }

    fn select(&mut self, guild: Id, channel: Option<Id>) {
        self.guild = Some(guild);
        self.channel = channel;
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
        if let Some(messages) = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
        {
            users.extend(messages.iter().map(|message| message.author.clone()));
        }
        for user in users {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
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
    fn member_panel(&self) -> impl IntoElement {
        let list = self
            .guild
            .and_then(|guild| self.stores.member_lists.get(&guild));
        let owner = self.current_guild().and_then(|guild| guild.owner_id);
        let rows: Vec<AnyElement> =
            list.map(|list| list.rows.as_slice())
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
                        div()
                            .h(px(42.))
                            .pt(px(1.))
                            .pl(px(8.))
                            .child(
                                div()
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
                                            .children(avatar.map(|image| {
                                                img(image).size(px(32.)).rounded_full()
                                            }))
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
                                    }),
                            )
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
        for (index, message) in messages.iter().enumerate() {
            let local = message.local_time();
            let day = (local.year(), local.ordinal());
            let new_day = last_day != Some(day);
            if new_day {
                self.rows
                    .push(Row::Divider(local.format("%B %-d, %Y").to_string()));
                last_day = Some(day);
            }
            let head = new_day || index == 0 || starts_group(&messages[index - 1], message);
            self.rows.push(Row::Message { index, head });
        }
        self.rows.push(Row::Spacer);
        self.list.reset(self.rows.len());
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
        format!(
            "guild={} channel={} composer={composer:?} rows={} selected={:?} hovered={:?} editing={}",
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
            }
            cx.notify();
        })
        .detach();
        window.focus(&editor.read(cx).focus_handle(cx), cx);
        self.editing = Some((message, editor));
        cx.notify();
    }

    fn send(&mut self, content: String) {
        let reply = self.replying.take().map(|message| (self.guild, message.id));
        if let Some(channel) = self.channel {
            let _ = self.requests.send(Request::Send {
                channel,
                content,
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
        let text = parse(&referenced.content)
            .into_iter()
            .find_map(|block| match block {
                Block::Paragraph(spans)
                | Block::Quote(spans)
                | Block::Subtext(spans)
                | Block::Heading { spans, .. }
                | Block::ListItem { spans, .. } => Some(spans),
                Block::Code { .. } => None,
            })
            .unwrap_or_default();
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
                }
            })
            .collect();
        inline(pieces, px(18.)).truncate()
    }

    /// The field with focus: the inline editor while editing, else the composer.
    #[cfg(feature = "capture")]
    pub fn active_editor(&self) -> Entity<Editor> {
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

    fn titlebar(&self) -> impl IntoElement {
        let title = match (self.current_guild(), self.channel) {
            (Some(guild), _) => guild.name.clone(),
            (None, None) => "Friends".to_owned(),
            (None, Some(_)) => self.channel_title(),
        };
        let acronym = self.current_guild().map(Guild::acronym).unwrap_or_default();
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
            .parity("titlebar")
            .relative()
            .flex_none()
            .h(px(32.))
            .w_full()
            .window_control_area(WindowControlArea::Drag)
            .child(div().absolute().left(px(15.)).top(px(7.)).child(icon(
                "title_back",
                18.,
                "--interactive-text-default",
            )))
            .child(div().absolute().left(px(39.)).top(px(7.)).child(icon(
                "title_forward",
                18.,
                "--interactive-text-default",
            )))
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
                        div()
                            .size(px(24.))
                            .mr(px(12.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("inbox", 18., "--text-muted")),
                    )
                    .child(
                        div()
                            .size(px(24.))
                            .mr(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("help", 18., "--text-muted")),
                    )
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(20.))
                            .mr(px(8.))
                            .bg(color("--background-mod-subtle")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(control("min", "minimize", WindowControlArea::Min))
                            .child(control("max", "maximize", WindowControlArea::Max))
                            .child(control("close", "close", WindowControlArea::Close)),
                    ),
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
            .child(tile)
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
            .child(tool("add_server", 20.))
            .child(tool("discover", 20.));
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

    fn channel_row(&self, channel: &Channel) -> AnyElement {
        if channel.kind == ChannelKind::Category {
            return div()
                .flex_none()
                .pt(px(16.))
                .h(px(40.))
                .pl(px(16.))
                .pr(px(17.))
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
                        .child(icon("category_chevron", 12., "--channels-default"))
                        .child(div().flex_1())
                        .child(icon("plus_small", 16., "--channels-default")),
                )
                .into_any_element();
        }
        let selected = self.channel == Some(channel.id);
        let tint = if selected {
            "--text-strong"
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
        let action_tint = if selected {
            "--text-strong"
        } else {
            "--interactive-icon-default"
        };
        let action_group = group.clone();
        let name_group = group.clone();
        div()
            .flex_none()
            .h(px(34.))
            .pt(px(1.))
            .pl(px(8.))
            .child(
                div()
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
                    .child(icon(name, 20., tint))
                    .child(
                        div()
                            .flex_1()
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
                            .when(!selected, |this| {
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

    fn sidebar(&self) -> impl IntoElement {
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
                guild
                    .sidebar_rows()
                    .into_iter()
                    .map(|row| self.channel_row(row))
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

    fn user_panel(&self) -> impl IntoElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let button = |name: &str| {
            div()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, 20., "--interactive-text-default"))
        };
        let chevron = || {
            div()
                .w(px(16.))
                .h(px(32.))
                .ml(px(1.))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("audio_chevron", 16., "--interactive-text-default"))
        };
        div()
            .parity_with_border("user-panel", 1.)
            .absolute()
            .left(px(8.))
            .bottom(px(8.))
            .w(px(359.))
            .h(px(58.))
            .rounded(px(8.))
            .bg(color("--background-base-low"))
            .border_1()
            .border_color(color("--background-mod-subtle"))
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
                    .child(button("mic"))
                    .child(chevron()),
            )
            .child(
                div()
                    .ml(px(8.))
                    .flex()
                    .items_center()
                    .child(button("headphones"))
                    .child(chevron()),
            )
            .child(div().ml(px(8.)).child(button("settings")))
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.channel_title();
        let search = format!(
            "Search {}",
            self.current_guild()
                .map(|guild| guild.name.as_str())
                .unwrap_or_default()
        );
        let tool = |name: &str| {
            div()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, 20., "--text-muted"))
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
            .child(
                div()
                    .ml(px(9.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(name),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(tool("threads"))
                    .child(tool("bell"))
                    .child(tool("pin"))
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
            Kind::Text | Kind::Link(_) | Kind::Everyone => span.text.clone(),
        }
    }

    fn piece(&self, span: &Span, base: Hsla, size: f32, weight: f32) -> Piece {
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
        }
    }

    fn spans(&self, spans: &[Span], text_color: Hsla, key: (u64, u32)) -> Inline {
        let pieces = spans
            .iter()
            .map(|span| self.piece(span, text_color, 16., 400.))
            .collect();
        inline(pieces, px(22.)).selectable(key, self.selection.clone(), hsla_from(SELECTION))
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
                let mut piece = self.piece(span, strong, size, 700.);
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
            .map(|span| self.piece(span, muted, 14., 400.))
            .collect();
        inline(pieces, px(19.25)).selectable(key, self.selection.clone(), hsla_from(SELECTION))
    }

    fn content(&self, message: &Message) -> impl IntoElement {
        let text = hsla("--text-default");
        let blocks = parse(&message.content);
        let gaps = block_gaps(&blocks);
        div()
            .flex()
            .flex_col()
            .children(
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
                                .child(self.spans(&spans, text, key))
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
                                let marker = match (number, depth) {
                                    (Some(number), _) => format!("{number}."),
                                    (None, 0) => "\u{2022}".to_owned(),
                                    (None, _) => "\u{25E6}".to_owned(),
                                };
                                div()
                                    .relative()
                                    .mt(px(gap))
                                    .ml(px(indent))
                                    .min_h(px(22.))
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(-18.))
                                            .top(px(0.))
                                            .line_height(px(22.))
                                            .text_color(color("--text-default"))
                                            .child(marker),
                                    )
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
                            this.open_menu(event.position, id, own, cx);
                        }),
                    )
                    .when(hovered && editing.is_none(), |this| {
                        this.child(self.message_toolbar(message, own, cx))
                    });
                let content = match editing {
                    Some(editor) => self.edit_box(editor).into_any_element(),
                    None => self.content(message).into_any_element(),
                };
                let row = div().pr(px(16.));
                if !*head {
                    return row
                        .child(body.child(self.content(message)))
                        .into_any_element();
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
                                    .children(
                                        avatar.map(|image| img(image).size(px(40.)).rounded_full()),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .line_height(px(22.))
                                            .child(message.author.display_name().to_owned()),
                                    )
                                    .child(
                                        div()
                                            .ml(px(8.))
                                            .text_size(px(12.))
                                            .line_height(px(22.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--channels-default"))
                                            .child(
                                                message
                                                    .local_time()
                                                    .format("%-I:%M %p")
                                                    .to_string(),
                                            ),
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
            .child(button("msg_react"))
            .when(own, |this| {
                this.child(
                    div()
                        .id("toolbar-edit")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.begin_edit(id, &text, window, cx);
                        }))
                        .child(button("msg_edit")),
                )
            })
            .child(button("msg_forward"))
            .child(button("msg_more"))
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
                    .child(editor),
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
                    .child(div().text_color(color("--text-link")).child("cancel"))
                    .child(" \u{2022} enter to ")
                    .child(div().text_color(color("--text-link")).child("save")),
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
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .children(emoji.map(|image| img(image).size(px(18.))));
        let tool = |name: &str| {
            div()
                .size(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, 20., "--interactive-text-default"))
        };
        // The form overlaps the list by 16px, as the official layout does.
        div().flex_none().mt(px(-16.)).px(px(8.)).pb(px(8.)).child(
            div()
                .parity_with_border("composer", 1.)
                .min_h(px(58.))
                .rounded(px(8.))
                .bg(color("--chat-background-default"))
                .border_1()
                .border_color(color("--background-mod-subtle"))
                .flex()
                .flex_col()
                .children(reply_bar)
                .child(
                    div()
                        .flex()
                        .items_start()
                        .pl(px(11.))
                        .pr(px(12.))
                        .child(div().pt(px(12.)).child(tool("attach")))
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
                                .child(tool("gift"))
                                .child(tool("gif"))
                                .child(tool("sticker"))
                                .child(emoji_button)
                                .child(tool("apps")),
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
        text: hsla("--text-default"),
        placeholder: hsla("--channel-text-area-placeholder"),
        selection: hsla_from(SELECTION),
        caret: hsla("--text-default"),
    }
}

/// A message's text as copied: its blocks, one per line.
fn parse_block_text(content: &str) -> String {
    parse(content)
        .iter()
        .map(block_text)
        .collect::<Vec<_>>()
        .join("\n")
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = list(
            self.list.clone(),
            cx.processor(|this, index, _, cx| this.row(index, cx)),
        )
        .size_full();
        div()
            .id("root")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
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
            .child(self.titlebar())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.rail(cx))
                    .child(if self.guild.is_some() {
                        self.sidebar().into_any_element()
                    } else {
                        self.home_sidebar(cx).into_any_element()
                    })
                    .child(if self.guild.is_none() && self.channel.is_none() {
                        self.friends_panel().into_any_element()
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
                                                    .flex_1()
                                                    .min_h(px(0.))
                                                    .child(rows),
                                            )
                                            .child(self.composer_bar(cx)),
                                    )
                                    .when(self.show_members && self.guild.is_some(), |this| {
                                        this.child(
                                            div()
                                                .flex_none()
                                                .w(px(1.))
                                                .h_full()
                                                .bg(color("--background-mod-subtle")),
                                        )
                                        .child(self.member_panel())
                                    }),
                            )
                            .into_any_element()
                    }),
            )
            .child(self.user_panel())
            .children(self.context_menu(cx))
            .children(self.delete_modal(cx))
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
                .when(status == "dnd", |this| {
                    this.child(div().w(px(6.)).h(px(2.)).rounded(px(1.)).bg(color(ring)))
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::{block_gaps, relative_time};
    use dc_markdown::parse;

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
