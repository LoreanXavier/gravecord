//! First screen: rail, channel list, user panel, header, messages, composer.
//! Geometry and colors are from `design/dom/channel/text-darker-cozy.json`; every color is an official token.

use crate::editor::{Editor, EditorEvent, EditorStyle};
use crate::inline::{BoxStyle, Inline, Piece, SharedSelection, inline};
use crate::net::{Request, Update};
use crate::parity::Parity;
use chrono::Datelike;
use dc_markdown::highlight::highlight;
use dc_markdown::{Block, Span, parse};
use dc_state::grouping::starts_group;
use dc_state::{Channel, ChannelKind, Guild, Id, Message, Stores, User};
use dc_tokens::Theme;
use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, Focusable, Font, FontFeatures,
    FontStyle, FontWeight, Hsla, Image, ImageFormat, IntoElement, KeyDownEvent, ListAlignment,
    ListState, Render, Rgba, SharedString, StyledText, TextRun, Window, WindowControlArea, div,
    font, img, list, prelude::*, px, rgba, svg,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

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
    pub ready: bool,
}

impl Root {
    pub fn new(requests: UnboundedSender<Request>, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            let mut text_font: Font = font(SANS);
            text_font.features = kerning();
            Editor::new(
                EditorStyle {
                    font: text_font,
                    text: hsla("--text-default"),
                    placeholder: hsla("--channel-text-area-placeholder"),
                    selection: hsla_from(SELECTION),
                    caret: hsla("--text-default"),
                },
                cx,
            )
        });
        cx.subscribe(&composer, |this, _, event, _| match event {
            EditorEvent::Submit(content) => this.send(content.clone()),
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
                if name == "MESSAGE_CREATE" || name == "MESSAGE_DELETE" {
                    self.rebuild_rows();
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
            "guild={} channel={} composer={composer:?} rows={} selected={:?}",
            self.current_guild()
                .map(|guild| guild.name.as_str())
                .unwrap_or("-"),
            self.current_channel()
                .map(|channel| channel.name.as_str())
                .unwrap_or("-"),
            self.rows.len(),
            self.selected_text(),
        )
    }

    fn send(&mut self, content: String) {
        if let Some(channel) = self.channel {
            let _ = self.requests.send(Request::Send { channel, content });
        }
    }

    #[cfg(feature = "capture")]
    pub fn composer(&self) -> Entity<Editor> {
        self.composer.clone()
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
        let title = self
            .current_guild()
            .map(|guild| guild.name.clone())
            .unwrap_or_default();
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
                    .child(
                        div()
                            .size(px(16.))
                            .rounded(px(4.))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(8.))
                            .text_color(color("--text-default"))
                            .child(acronym),
                    )
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
            .when(selected, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .w(px(4.))
                        .h(px(40.))
                        .rounded_r(px(4.))
                        .bg(color("--text-strong")),
                )
            })
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
            .child(tool("home", 24.))
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
        div()
            .flex_none()
            .h(px(34.))
            .pt(px(1.))
            .pl(px(8.))
            .child(
                div()
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
                    .child(icon(name, 20., tint))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color(tint))
                            .child(channel.name.clone()),
                    )
                    .when(selected, |this| {
                        this.child(
                            div()
                                .flex()
                                .gap(px(4.))
                                .child(icon("invite_small", 16., tint))
                                .child(icon("settings_small", 16., tint)),
                        )
                    }),
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
                    .child(status_dot(&self.stores.status)),
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

    fn header(&self) -> impl IntoElement {
        let name = self
            .current_channel()
            .map(|channel| channel.name.clone())
            .unwrap_or_default();
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
            .child(icon("header_hash", 20., "--channels-default"))
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
                    .child(tool("members")),
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

    fn spans(&self, spans: &[Span], text_color: Hsla, key: (u64, u32)) -> Inline {
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
                    size: if style.code { px(13.6) } else { px(16.) },
                    color: text_color,
                    underline: style.underline,
                    strike: style.strike,
                    boxed: style.code.then(inline_code_box),
                }
            })
            .collect();
        inline(pieces, px(22.)).selectable(key, self.selection.clone(), hsla_from(SELECTION))
    }

    fn content(&self, message: &Message) -> impl IntoElement {
        let text = hsla("--text-default");
        let blocks = parse(&message.content);
        let quote_at: Vec<usize> = blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| matches!(block, Block::Quote(_)))
            .map(|(index, _)| index)
            .collect();
        div()
            .flex()
            .flex_col()
            .children(blocks.into_iter().enumerate().map(|(index, block)| {
                // A quote keeps 4px above it and above whatever follows it.
                let after_quote = index > 0 && quote_at.contains(&(index - 1));
                match block {
                    Block::Paragraph(spans) => div()
                        .min_h(px(22.))
                        .when(after_quote, |this| this.mt(px(4.)))
                        .child(self.spans(&spans, text, (message.id, index as u32)))
                        .into_any_element(),
                    Block::Quote(spans) => div()
                        .mt(px(4.))
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
                            (message.id, index as u32),
                        )))
                        .into_any_element(),
                    Block::Code { language, text } => div()
                        .mt(px(6.))
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
            }))
    }

    fn row(&self, index: usize) -> AnyElement {
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
                // Padding, not margin: gpui lists measure an item without its margins.
                let base = div().relative().pl(px(72.)).pr(px(64.)).pb(px(2.));
                if !*head {
                    return base
                        .pt(px(2.))
                        .child(self.content(message))
                        .into_any_element();
                }
                let avatar = self.images.get(&avatar_source(&message.author).0).cloned();
                base.pt(px(19.))
                    .child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(21.))
                            .size(px(40.))
                            .rounded_full()
                            .overflow_hidden()
                            .children(avatar.map(|image| img(image).size(px(40.)).rounded_full())),
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
                                    .child(message.local_time().format("%-I:%M %p").to_string()),
                            ),
                    )
                    .child(self.content(message))
                    .into_any_element()
            }
        }
    }

    fn composer_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let placeholder = format!(
            "Message #{}",
            self.current_channel()
                .map(|channel| channel.name.as_str())
                .unwrap_or_default()
        );
        self.composer
            .update(cx, |composer, _| composer.set_placeholder(placeholder));
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
        )
    }
}

/// Plain text of a block, the same bytes the inline engine lays out.
fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph(spans) | Block::Quote(spans) => {
            spans.iter().map(|span| span.text.as_str()).collect()
        }
        Block::Code { text, .. } => text.clone(),
    }
}

/// `code.inline` from the captured DOM: 0.2em padding plus the border, 20px tall, 1px below the line top.
fn inline_code_box() -> BoxStyle {
    BoxStyle {
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
            cx.processor(|this, index, _, _| this.row(index)),
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
                    .child(self.sidebar())
                    .child(
                        div()
                            .parity("chat")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .pt(px(1.))
                            .child(self.header())
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .bg(color("--background-base-lower"))
                                    .child(
                                        div().parity("messages").flex_1().min_h(px(0.)).child(rows),
                                    )
                                    .child(self.composer_bar(cx)),
                            ),
                    ),
            )
            .child(self.user_panel())
    }
}

pub fn bind_focus(root: &gpui::Entity<Root>, window: &mut Window, cx: &mut App) {
    let handle = root.read(cx).composer.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
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
fn status_dot(status: &str) -> impl IntoElement {
    let tint = match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" => "--icon-status-offline",
        _ => "--icon-status-online",
    };
    div()
        .absolute()
        .right(px(-3.))
        .bottom(px(-3.))
        .size(px(16.))
        .rounded_full()
        .bg(color("--background-base-low"))
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
                    this.child(
                        div()
                            .w(px(6.))
                            .h(px(2.))
                            .rounded(px(1.))
                            .bg(color("--background-base-low")),
                    )
                }),
        )
}
