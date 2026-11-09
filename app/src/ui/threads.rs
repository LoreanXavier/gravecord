//! Threads: the rows under a channel in the sidebar (`container_reDhww` with its spine), the header's Threads popout
//! (`browser_l-3BKx`, 480 wide: search, Create, the joined list or the empty state) and creating one, as recorded in
//! `TRAFFIC/thread-create.json`: `POST /channels/{parent}/threads {name, type: 11 | 12, auto_archive_duration: 4320,
//! location: "Thread Browser Toolbar"}`, then the first message in the new thread.
//! Measured in `design/dom/popouts/threads-darker-cozy.json`, `threads-list-darker-cozy.json` and
//! `channel/thread-view-darker-cozy.json`.

use super::{Root, avatar_source, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, Thread};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window,
    deferred, div, img, prelude::*, px,
};
use serde_json::json;

/// `contain_FbXIeL` in the empty popout.
const EMPTY_PICTURE: &str = "https://cdn.discordapp.com/assets/content/a55e53565529b99537f24e9111b39ad84565ad76ac91760cfb5450a069dd640c.svg";
/// GUILD_PUBLIC_THREAD and GUILD_PRIVATE_THREAD.
const PUBLIC_THREAD: u64 = 11;
const PRIVATE_THREAD: u64 = 12;
/// Three days, the composer's default in the recording.
const AUTO_ARCHIVE: u64 = 4320;
pub(super) const CREATE_KEY: &str = "thread-create";

pub(crate) struct ThreadBrowser {
    pub search: Entity<Editor>,
    _events: Subscription,
    pub query: String,
}

/// The New Thread form: name, private, and the first message.
pub(crate) struct NewThread {
    pub parent: Id,
    /// Create Thread on a message: the message the thread grows from (`location: "Message"`).
    pub starter: Option<dc_state::Message>,
    pub name: Entity<Editor>,
    pub message: Entity<Editor>,
    pub private: bool,
    _events: [Subscription; 2],
}

/// The short relative time of thread cards: "1m ago", "3h ago", "2d ago".
pub(super) fn short_ago(milliseconds: i64) -> String {
    let minutes = (milliseconds / 60_000).max(1);
    if minutes < 60 {
        format!("{minutes}m ago")
    } else if minutes < 60 * 24 {
        format!("{}h ago", minutes / 60)
    } else {
        format!("{}d ago", minutes / (60 * 24))
    }
}

impl Root {
    fn guild_threads(&self, parent: Option<Id>) -> Vec<Thread> {
        let Some(guild) = self.current_guild() else {
            return Vec::new();
        };
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let mut list: Vec<Thread> = guild
            .threads
            .iter()
            .filter(|thread| parent.is_none_or(|parent| thread.parent_id == parent))
            .cloned()
            // A thread you started is one you are in, whether or not READY listed the membership.
            .map(|mut thread| {
                thread.joined |= thread.owner_id.is_some() && thread.owner_id == me;
                thread
            })
            .collect();
        list.sort_by_key(|thread| std::cmp::Reverse(thread.last_message_id.unwrap_or(thread.id)));
        list
    }

    /// `container_reDhww`: a joined, active thread under its channel: the 10x19 spine 24px in, the name 36px in.
    pub(super) fn thread_rows(&self, parent: Id, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let threads: Vec<Thread> = self
            .guild_threads(Some(parent))
            .into_iter()
            .filter(|thread| thread.joined)
            .collect();
        let count = threads.len();
        threads
            .into_iter()
            .enumerate()
            .map(|(index, thread)| {
                let selected = self.channel == Some(thread.id);
                let id = thread.id;
                let guild = self.guild;
                div()
                    .id(SharedString::from(format!("thread-row-{id}")))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.open_context_menu(
                                super::context_menu::Target::Thread(id),
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .relative()
                    .ml(px(8.))
                    .w(px(286.))
                    .h(px(34.))
                    // Between rows the spine runs on: a 2px rule from the top.
                    .when(index > 0, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(px(24.))
                                .top(px(0.))
                                .w(px(2.))
                                .h(px(10.))
                                .rounded(px(2.))
                                .bg(color("--background-mod-strong")),
                        )
                    })
                    .child(
                        div().absolute().left(px(24.)).top(px(0.)).child(
                            gpui::svg()
                                .path("icons/thread_spine.svg")
                                .w(px(10.))
                                .h(px(19.))
                                .text_color(color("--background-mod-strong")),
                        ),
                    )
                    .when(index + 1 < count, |this| this)
                    .child(
                        div().ml(px(36.)).py(px(1.)).child(
                            div()
                                .h(px(32.))
                                .px(px(8.))
                                .rounded(px(8.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .when(selected, |this| this.bg(color("--background-mod-strong")))
                                .when(!selected, |this| {
                                    this.hover(|style| style.bg(color("--background-mod-subtle")))
                                })
                                .text_size(px(14.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(500.))
                                .text_color(if selected {
                                    color("--text-strong")
                                } else {
                                    color("--channels-default")
                                })
                                .child(thread.name.clone()),
                        ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(guild) = guild {
                            this.select(guild, Some(id));
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect()
    }

    pub(super) fn toggle_thread_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.thread_browser.take().is_some() {
            cx.notify();
            return;
        }
        self.request_image("threads:empty".to_owned(), EMPTY_PICTURE.to_owned());
        // Each card shows its thread's latest message.
        for thread in self.guild_threads(self.channel) {
            if !self.stores.messages.contains_key(&thread.id) {
                let _ = self
                    .requests
                    .send(Request::LoadMessages { channel: thread.id });
            }
        }
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Search for Thread Name".to_owned());
            editor
        });
        let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            if let Some(browser) = this.thread_browser.as_mut() {
                match event {
                    EditorEvent::Edited => browser.query = editor.read(cx).text().to_owned(),
                    EditorEvent::Submit(text) => {
                        editor.update(cx, |editor, _| editor.set_text(text))
                    }
                    EditorEvent::Cancel => this.thread_browser = None,
                }
            }
            cx.notify();
        });
        window.focus(&gpui::Focusable::focus_handle(search.read(cx), cx), cx);
        self.thread_browser = Some(ThreadBrowser {
            search,
            _events: events,
            query: String::new(),
        });
        cx.notify();
    }

    /// The message menu's Create Thread: the panel with the message under the name, its text as the name.
    pub(super) fn open_new_thread_from(
        &mut self,
        message: dc_state::Message,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_new_thread(window, cx);
        let placeholder: String = message.content.chars().take(100).collect();
        if let Some(form) = self.new_thread.as_mut() {
            form.name
                .update(cx, |editor, _| editor.set_placeholder(placeholder));
            form.starter = Some(message);
        }
        cx.notify();
    }

    pub(super) fn open_new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = self.channel else {
            return;
        };
        self.thread_browser = None;
        let name = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("New Thread".to_owned());
            editor
        });
        let message = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Enter a message to start the conversation!".to_owned());
            editor
        });
        let name_events = cx.subscribe(&name, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    editor.update(cx, |editor, _| editor.set_text(text));
                    this.create_thread(cx);
                }
                EditorEvent::Cancel => this.new_thread = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        let message_events = cx.subscribe(&message, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    this.create_thread_with(text, cx);
                }
                EditorEvent::Cancel => this.new_thread = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        window.focus(&gpui::Focusable::focus_handle(message.read(cx), cx), cx);
        self.new_thread = Some(NewThread {
            parent,
            starter: None,
            name,
            message,
            private: false,
            _events: [name_events, message_events],
        });
        cx.notify();
    }

    fn create_thread(&mut self, cx: &mut Context<Self>) {
        let text = self
            .new_thread
            .as_ref()
            .map(|form| form.message.read(cx).text().trim().to_owned())
            .unwrap_or_default();
        self.create_thread_with(text, cx);
    }

    /// The thread needs its first message: with none, nothing is sent.
    fn create_thread_with(&mut self, message: String, cx: &mut Context<Self>) {
        let Some(form) = self.new_thread.as_ref() else {
            return;
        };
        if message.trim().is_empty() {
            return;
        }
        let typed = form.name.read(cx).text().trim().to_owned();
        // An empty name takes the starter message's text, or the first message's start.
        let name = if !typed.is_empty() {
            typed
        } else if let Some(starter) = form.starter.as_ref() {
            starter.content.chars().take(100).collect::<String>()
        } else {
            message.chars().take(40).collect::<String>()
        };
        let (route, location) = match form.starter.as_ref() {
            // As TRAFFIC/thread-from-message.json records it.
            Some(starter) => (
                format!("/channels/{}/messages/{}/threads", form.parent, starter.id),
                "Message",
            ),
            None => (
                format!("/channels/{}/threads", form.parent),
                "Thread Browser Toolbar",
            ),
        };
        let body = json!({
            "name": name,
            "type": if form.private { PRIVATE_THREAD } else { PUBLIC_THREAD },
            "auto_archive_duration": AUTO_ARCHIVE,
            "location": location,
        });
        self.pending_thread_message = Some(message);
        let _ = self.requests.send(Request::Rest {
            key: Some(CREATE_KEY.to_owned()),
            method: reqwest::Method::POST,
            route,
            body: Some(body),
        });
        self.new_thread = None;
        cx.notify();
    }

    /// The created thread: send its first message and open it.
    pub(super) fn thread_created(&mut self, report: &serde_json::Value, cx: &mut Context<Self>) {
        if !report["ok"].as_bool().unwrap_or(false) {
            return;
        }
        let body = &report["body"];
        let Some(thread) = Thread::from_value(body) else {
            return;
        };
        if let Some(guild) = self.guild.and_then(|guild| {
            self.stores
                .guilds
                .iter_mut()
                .find(|known| known.id == guild)
        }) {
            guild.threads.retain(|known| known.id != thread.id);
            let mut joined = thread.clone();
            joined.joined = true;
            guild.threads.push(joined);
        }
        if let Some(content) = self.pending_thread_message.take() {
            let _ = self.requests.send(Request::Send {
                channel: thread.id,
                content,
                files: Vec::new(),
                reply: None,
            });
        }
        if let Some(guild) = self.guild {
            self.select(guild, Some(thread.id));
        }
        cx.notify();
    }

    /// `card_*` in the joined list: the name, the last author and message, the time, and the owner's avatar.
    fn thread_card(&self, thread: &Thread, cx: &mut Context<Self>) -> AnyElement {
        let id = thread.id;
        let guild = self.guild;
        let last = self
            .stores
            .messages
            .get(&id)
            .and_then(|list| list.last())
            .cloned();
        let owner = thread
            .owner_id
            .and_then(|owner| self.stores.users.get(&owner))
            .cloned();
        let owner_avatar = owner
            .as_ref()
            .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        let since = thread
            .last_message_id
            .map(|message| {
                let ms = (message >> 22) + 1_420_070_400_000;
                short_ago(chrono::Utc::now().timestamp_millis() - ms as i64)
            })
            .unwrap_or_default();
        div()
            .id(SharedString::from(format!("thread-card-{id}")))
            .mb(px(8.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .cursor_pointer()
            .hover(|style| style.bg(color("--background-mod-subtle")))
            .flex()
            .items_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.thread_browser = None;
                if let Some(guild) = guild {
                    this.select(guild, Some(id));
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(thread.name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .children(last.map(|message| {
                                div()
                                    .flex()
                                    .min_w(px(0.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-default"))
                                            .child(format!(
                                                "{}:\u{a0}",
                                                message.author.display_name()
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_color(color("--text-default"))
                                            .child(message.content.clone()),
                                    )
                            }))
                            .child(
                                div()
                                    .text_color(color("--text-muted"))
                                    .child(format!("\u{2022} {since}")),
                            ),
                    ),
            )
            .child(
                div()
                    .ml(px(8.))
                    .size(px(20.))
                    .children(owner_avatar.map(|image| img(image).size(px(20.)).rounded_full())),
            )
            .into_any_element()
    }

    pub(super) fn thread_browser_popout(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let browser = self.thread_browser.as_ref()?;
        let anchor = self.tip_anchors.borrow().get("header-threads").copied();
        // The popout's right edge meets the Threads button's.
        let left = anchor
            .map_or(416., |bounds| f32::from(bounds.right()) - 480.)
            .max(8.);
        let _ = window;
        let query = browser.query.trim().to_lowercase();
        let threads: Vec<Thread> = self
            .guild_threads(self.channel)
            .into_iter()
            .filter(|thread| query.is_empty() || thread.name.to_lowercase().contains(&query))
            .collect();
        let joined: Vec<&Thread> = threads.iter().filter(|thread| thread.joined).collect();
        let others: Vec<&Thread> = threads.iter().filter(|thread| !thread.joined).collect();
        let empty = threads.is_empty();
        let picture = self.images.get("threads:empty").cloned();
        let mut list: Vec<AnyElement> = Vec::new();
        let heading = |text: String| {
            div()
                .mt(px(16.))
                .mb(px(8.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-default"))
                .child(text)
                .into_any_element()
        };
        if !joined.is_empty() {
            list.push(heading(format!("{} joined threads", joined.len())));
            list.extend(joined.iter().map(|thread| self.thread_card(thread, cx)));
        }
        if !others.is_empty() {
            list.push(heading(format!("{} other active threads", others.len())));
            list.extend(others.iter().map(|thread| self.thread_card(thread, cx)));
        }
        let body: AnyElement = if empty {
            div()
                .p(px(32.))
                .h(px(400.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .mb(px(12.))
                        .w(px(256.))
                        .h(px(144.))
                        .children(picture.map(|image| img(image).w(px(256.)).h(px(144.)))),
                )
                .child(
                    div()
                        .mb(px(8.))
                        .text_size(px(24.))
                        .line_height(px(32.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child("There are no threads."),
                )
                .child(
                    div()
                        .w(px(384.))
                        .text_center()
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child("Stay focused on a conversation with a thread - a temporary text channel."),
                )
                .child(
                    div()
                        .id("threads-create-cta")
                        .mt(px(24.))
                        .h(px(40.))
                        .px(px(15.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-primary-border-default"))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .on_click(cx.listener(|this, _, window, cx| this.open_new_thread(window, cx)))
                        .child("Create Thread"),
                )
                .into_any_element()
        } else {
            // With threads the popout is 400 tall: the 49px header and the list.
            div()
                .id("threads-list")
                .h(px(351.))
                .overflow_y_scroll()
                .px(px(16.))
                .children(list)
                .into_any_element()
        };
        let popout = div()
            .parity("thread-browser")
            .id("thread-browser")
            .occlude()
            .w(px(480.))
            .rounded(px(8.))
            .bg(color("--background-surface-higher"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.thread_browser = None;
                cx.notify();
            }))
            .child(
                div()
                    .h(px(49.))
                    .px(px(16.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .mr(px(8.))
                            .child(icon("threads_title", 24., "--text-subtle")),
                    )
                    .child(
                        div()
                            .mr(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Threads"),
                    )
                    .child(
                        div()
                            .mr(px(16.))
                            .w(px(1.))
                            .h(px(24.))
                            .bg(color("--border-subtle")),
                    )
                    .child(
                        div()
                            .w(px(220.5))
                            .h(px(32.))
                            .pl(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-active"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("threads_search", 12., "--icon-default")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .pr(px(12.))
                                    .child(browser.search.clone()),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("threads-create")
                            .ml(px(8.))
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-primary-border-default"))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_new_thread(window, cx)),
                            )
                            .child("Create"),
                    ),
            )
            .child(body);
        Some(
            deferred(div().absolute().top(px(81.)).left(px(left)).child(popout))
                .with_priority(2)
                .into_any_element(),
        )
    }

    /// The New Thread panel (`chatLayerWrapper_FrbQJp`, 480 wide on the right): the header with close, then at the
    /// bottom the thread icon, Thread Name, Private Thread and the first message's composer.
    pub(super) fn new_thread_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.new_thread.as_ref()?;
        let private = form.private;
        let from_message = form.starter.is_some();
        let starter = form.starter.as_ref().map(|message| {
            let avatar = self.images.get(&avatar_source(&message.author).0).cloned();
            div()
                .relative()
                .mt(px(16.))
                .pl(px(56.))
                .child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(2.))
                        .size(px(40.))
                        .children(avatar.map(|image| img(image).size(px(40.)).rounded_full())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(message.author.display_name().to_owned()),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .text_size(px(12.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(super::settings::prefs::clock(&message.local_time())),
                        ),
                )
                .child(
                    div()
                        .line_height(px(22.))
                        .text_color(color("--text-default"))
                        .child(message.content.clone()),
                )
        });
        Some(
            div()
                .parity("new-thread")
                .flex_none()
                .w(px(480.))
                .h_full()
                .rounded_tl(px(12.))
                .border_t_1()
                .border_l_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(49.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .child(icon("threads_title", 20., "--text-muted"))
                        .child(
                            div()
                                .ml(px(6.))
                                .flex_1()
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child("New Thread"),
                        )
                        .child(
                            div()
                                .id("new-thread-close")
                                .size(px(32.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.new_thread = None;
                                    cx.notify();
                                }))
                                .child(icon("modal_close", 20., "--text-subtle")),
                        ),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .px(px(16.))
                        .pb(px(6.))
                        .child(
                            div()
                                .size(px(64.))
                                .rounded_full()
                                .bg(color("--background-mod-strong"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("threads_title", 32., "--text-subtle")),
                        )
                        .child(
                            div()
                                .mt(px(16.))
                                .mb(px(8.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(if from_message {
                                    "Thread Name (Optional)"
                                } else {
                                    "Thread Name"
                                }),
                        )
                        .child(
                            div()
                                .h(px(40.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--input-border-default"))
                                .bg(color("--input-background-default"))
                                .flex()
                                .items_center()
                                .child(div().w_full().child(form.name.clone())),
                        )
                        .children(starter)
                        .when(!from_message, |this| {
                            this.child(
                                div()
                                    .mt(px(24.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child("Private Thread"),
                            )
                        })
                        .when(!from_message, |this| {
                            this.child(
                                div()
                                    .id("new-thread-private")
                                    .mt(px(8.))
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(form) = this.new_thread.as_mut() {
                                            form.private = !form.private;
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .size(px(20.))
                                            .rounded(px(4.))
                                            .border_1()
                                            .border_color(if private {
                                                color("--checkbox-border-selected-default")
                                            } else {
                                                color("--checkbox-border-default")
                                            })
                                            .bg(if private {
                                                color("--checkbox-background-selected-default")
                                            } else {
                                                color("--checkbox-background-default")
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(private, |this| {
                                                this.child(icon("checkbox_check", 18., "--white"))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child("Only people you invite and moderators can see"),
                                    ),
                            )
                        }),
                )
                .child(
                    div().px(px(8.)).pb(px(8.)).child(
                        div()
                            .min_h(px(56.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .flex()
                            .items_center()
                            .px(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .py(px(17.))
                                    .child(form.message.clone()),
                            )
                            .children(self.images.get("emoji_button").cloned().map(|image| {
                                let field = form.message.clone();
                                div()
                                    .id("new-thread-emoji")
                                    .ml(px(8.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(
                                        move |this, event: &gpui::ClickEvent, window, cx| {
                                            let at = event.position();
                                            this.picker_field = Some(field.clone());
                                            this.open_picker(
                                                super::picker::Target::Field,
                                                gpui::point(at.x, at.y - px(24.)),
                                                window,
                                                cx,
                                            );
                                        },
                                    ))
                                    .child(img(image).size(px(18.)).grayscale(true))
                            })),
                    ),
                )
                .into_any_element(),
        )
    }
}
