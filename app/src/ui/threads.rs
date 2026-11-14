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
use crate::ui::motion::Posed as _;
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
    /// The channel whose threads are listed, and the archived ones loaded for it so far (`Fr`, module 424798).
    parent: Option<Id>,
    archived: Vec<Id>,
    loading: bool,
    has_more: bool,
    failed: bool,
    scroll: gpui::ScrollHandle,
    /// Capture only: the routes asked for so far, for `threads requests`.
    #[cfg(feature = "capture")]
    asked: Vec<String>,
}

/// The key of a page of archived threads asked for by the browser, followed by the channel id.
const BROWSER_PAGE_KEY: &str = "thread-browser:";

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

/// The width a run of text takes at a size and weight, as the text system shapes it.
fn measure_text(window: &mut Window, text: &str, size: f32, weight: f32) -> f32 {
    if text.is_empty() {
        return 0.;
    }
    let mut font = gpui::font(super::SANS);
    font.weight = FontWeight(weight);
    font.features = super::kerning();
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(SharedString::from(text.to_owned()), px(size), &[run], None)
            .width,
    )
}

/// `-webkit-line-clamp: 1` of a thread row's message: the text itself when it fits `free` px, else its first line
/// of whole words followed by an ellipsis, and `true` for "the box takes the whole free width".
fn clamp_words(window: &mut Window, text: &str, size: f32, free: f32) -> (String, bool) {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if measure_text(window, &one_line, size, 400.) <= free {
        return (one_line, false);
    }
    let mut fitted = String::new();
    for word in one_line.split(' ') {
        let next = if fitted.is_empty() {
            word.to_owned()
        } else {
            format!("{fitted} {word}")
        };
        if measure_text(window, &next, size, 400.) > free {
            break;
        }
        fitted = next;
    }
    (format!("{fitted} \u{2026}"), true)
}
/// `aK` (module 469881) with its default strings: "1m ago" under a minute, then minutes, hours and days ("m ago",
/// "h ago", "d ago"; string ids N9M4N1, p0KedC, gjK5av), ">30d ago" under a "year" of 12 thirty-day months and the
/// date after that (`format("LL")`, such as "November 12, 2026").
pub(super) fn short_ago(milliseconds: i64) -> String {
    let seconds = (milliseconds / 1000).max(0);
    if seconds < 60 {
        "1m ago".to_owned()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else if seconds < 2_592_000 {
        format!("{}d ago", seconds / 86_400)
    } else if seconds < 31_104_000 {
        ">30d ago".to_owned()
    } else {
        let then = chrono::Utc::now() - chrono::Duration::milliseconds(milliseconds);
        then.format("%B %-d, %Y").to_string()
    }
}

impl Root {
    /// `Update::Fetched`: the `members` of a message search (`SEARCH_MESSAGES_SUCCESS`, the body itself) and of a
    /// forum's thread search (`LOAD_ARCHIVED_THREADS_SUCCESS` / `LOAD_THREADS_SUCCESS`, in the report's `body`) are
    /// the account's thread memberships (module 152007).
    pub(super) fn thread_members_fetched(&mut self, key: &str, body: &serde_json::Value) {
        if key == super::search::RESULTS_KEY {
            let guild = self.guild;
            self.stores.thread_members_from_response(guild, body);
        } else if let Some(channel) = key
            .strip_prefix("forum-page:")
            .and_then(|rest| rest.split(':').next())
            .and_then(|channel| channel.parse::<Id>().ok())
        {
            let guild = self
                .stores
                .guilds
                .iter()
                .find(|guild| guild.channels.iter().any(|known| known.id == channel))
                .map(|guild| guild.id);
            self.stores
                .thread_members_from_response(guild, &body["body"]);
        }
    }

    fn guild_threads(&self, parent: Option<Id>) -> Vec<Thread> {
        let Some(guild) = self.current_guild() else {
            return Vec::new();
        };
        let mut list: Vec<Thread> = guild
            .threads
            .iter()
            .filter(|thread| parent.is_none_or(|parent| thread.parent_id == parent))
            .cloned()
            .collect();
        list.sort_by_key(|thread| std::cmp::Reverse(thread.last_message_id.unwrap_or(thread.id)));
        list
    }

    /// The threads listed under `parent` (`ActiveJoinedThreadsStore` and `eA.computeState`/`eT` in module 791513, ported
    /// in `dc_state::thread_list`): joined, active and relevant, newest joined first, the open thread on top.
    fn sidebar_threads(&self, parent: Id) -> Vec<Thread> {
        let Some(guild) = self.current_guild() else {
            return Vec::new();
        };
        let now = chrono::Utc::now();
        let open = self.channel.and_then(|channel| {
            guild
                .threads
                .iter()
                .find(|thread| thread.id == channel && thread.parent_id == parent)
                .map(|thread| thread.id)
        });
        let parent_row = guild.channels.iter().find(|channel| channel.id == parent);
        let category_collapsed = parent_row
            .and_then(|row| row.parent_id)
            .is_some_and(|category| {
                self.stores
                    .guild_settings
                    .channel(guild.id, category)
                    .collapsed
            });
        let parent_muted = self
            .stores
            .guild_settings
            .channel_muted(guild.id, parent, now);
        let voice_here = self
            .call
            .as_ref()
            .is_some_and(|call| call.channel == parent);
        let parent_selected = self.channel == Some(parent) || voice_here;
        // `o || d || (!category.isCollapsed && !isMuted) ? relevant : unread` (module 791513 `eA.computeState`).
        let source = if parent_selected || open.is_some() || (!category_collapsed && !parent_muted)
        {
            dc_state::thread_list::Source::Relevant
        } else {
            dc_state::thread_list::Source::Unread
        };
        let ids = dc_state::thread_list::sidebar_threads(&dc_state::thread_list::Query {
            threads: &guild.threads,
            parent,
            source,
            selected_thread: open,
            parent_selected,
            hide_muted: self.hide_muted(guild.id),
            standing: |thread: &Thread| dc_state::thread_list::Standing {
                has_unread: self.stores.is_unread(thread.id),
                mentions: self.stores.read_states.mentions(thread.id),
                in_voice: self
                    .call
                    .as_ref()
                    .is_some_and(|call| call.channel == thread.id),
            },
            now,
        });
        ids.into_iter()
            .filter_map(|id| guild.threads.iter().find(|thread| thread.id == id).cloned())
            .collect()
    }

    /// Capture only: `threads facts` names every thread of the open guild with the facts the sidebar rule reads.
    #[cfg(feature = "capture")]
    pub(crate) fn script_threads(&mut self, words: &[&str]) -> String {
        // `threads join <name part>`: the account's thread member arrives as it does when it joins in this session.
        if let ["join", name @ ..] = words {
            let name = name.join(" ");
            let Some(guild) = self.current_guild() else {
                return "threads: no guild".to_owned();
            };
            let (guild_id, found) = (
                guild.id,
                guild
                    .threads
                    .iter()
                    .find(|thread| thread.name.contains(&name))
                    .map(|thread| thread.id),
            );
            let (Some(thread), Some(me)) =
                (found, self.stores.current_user.as_ref().map(|user| user.id))
            else {
                return format!("threads: no thread named {name}");
            };
            self.stores.dispatch(
                "THREAD_MEMBER_UPDATE",
                &json!({
                    "id": thread.to_string(), "guild_id": guild_id.to_string(), "user_id": me.to_string(),
                    "flags": 1, "muted": false, "join_timestamp": chrono::Utc::now().to_rfc3339(),
                }),
            );
            return format!("threads: joined {thread}");
        }
        let Some(guild) = self.current_guild() else {
            return "threads: no guild".to_owned();
        };
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let now = chrono::Utc::now();
        let listed = self.sidebar_threads_all();
        let mut out = format!(
            "threads: {} known, listed {:?}",
            guild.threads.len(),
            listed
        );
        for thread in &guild.threads {
            out.push_str(&format!(
                "
  {} {:?} owner_is_me={} parent={} joined={} archived={} auto_archive_min={} last_activity_age_min={} unread={} mentions={} muted={} pinned={}",
                thread.id,
                thread.name,
                thread.owner_id.is_some() && thread.owner_id == me,
                thread.parent_id,
                thread.joined,
                thread.archived,
                thread.auto_archive_duration,
                (now.timestamp_millis() - thread.last_activity_ms()) / 60_000,
                self.stores.is_unread(thread.id),
                self.stores.read_states.mentions(thread.id),
                thread.is_muted(now),
                thread.pinned(),
            ));
        }
        out
    }

    /// Capture only: `threads archived [name part]` answers the open browser's archived-threads request the way the
    /// server does (`threads/search?archived=true`): the named thread of the open channel (default "gravecord test
    /// thread") comes back archived, with the member preview the store already holds. The request itself is not
    /// replaced: it still goes out on open and on scroll, and this only supplies the page the fixture has none of.
    /// Any other words go to `script_threads`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_threads_with(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        // `threads requests`: every archived-threads route the open browser has asked for, in order.
        if let ["requests"] = words {
            let Some(browser) = self.thread_browser.as_ref() else {
                return "threads: the browser is not open".to_owned();
            };
            return format!(
                "threads: {} archived-page requests{}",
                browser.asked.len(),
                browser
                    .asked
                    .iter()
                    .map(|route| format!("\n  GET {route}"))
                    .collect::<String>()
            );
        }
        // `threads loading`: the open browser is put back into the state of a first page still on its way.
        if let ["loading"] = words {
            let Some(browser) = self.thread_browser.as_mut() else {
                return "threads: the browser is not open".to_owned();
            };
            browser.loading = true;
            browser.failed = false;
            browser.archived.clear();
            cx.notify();
            return "threads: first page held as loading".to_owned();
        }
        let ["archived", name @ ..] = words else {
            return self.script_threads(words);
        };
        // `threads archived more`: the page says `has_more`, so scrolling to the end asks for the next one.
        let (more, name) = match name {
            ["more", rest @ ..] => (true, rest),
            rest => (false, rest),
        };
        let name = if name.is_empty() {
            "gravecord test thread".to_owned()
        } else {
            name.join(" ")
        };
        let Some(parent) = self
            .thread_browser
            .as_ref()
            .and_then(|browser| browser.parent)
        else {
            return "threads: the browser is not open".to_owned();
        };
        let Some(thread) = self
            .guild_threads(Some(parent))
            .into_iter()
            .find(|thread| thread.name.contains(&name))
        else {
            return format!("threads: no thread named {name}");
        };
        let now = chrono::Utc::now().timestamp_millis();
        let stamp = |at: i64| {
            chrono::DateTime::from_timestamp_millis(at)
                .map(|time| time.to_rfc3339())
                .unwrap_or_default()
        };
        let preview: Vec<String> = self
            .stores
            .thread_previews
            .member_ids_preview(thread.id)
            .unwrap_or_default()
            .iter()
            .map(|id| id.to_string())
            .collect();
        let body = json!({
            "threads": [{
                "id": thread.id.to_string(),
                "parent_id": thread.parent_id.to_string(),
                "name": thread.name,
                "type": thread.kind,
                "owner_id": thread.owner_id.map(|id| id.to_string()),
                "last_message_id": thread.last_message_id.map(|id| id.to_string()),
                "message_count": thread.message_count,
                "member_count": self.stores.thread_previews.member_count(thread.id).unwrap_or(0),
                "member_ids_preview": preview,
                "thread_metadata": {
                    "archived": true,
                    "archive_timestamp": stamp(now - 3 * 24 * 3_600_000),
                    "auto_archive_duration": thread.auto_archive_duration,
                    "locked": false,
                },
            }],
            "members": [],
            "has_more": more,
            "total_results": 1,
        });
        let key = format!("{BROWSER_PAGE_KEY}{parent}");
        self.thread_browser_fetched(&key, &json!({"ok": true, "body": body}), cx);
        format!("threads: archived page for {} fed", thread.id)
    }

    #[cfg(feature = "capture")]
    fn sidebar_threads_all(&self) -> Vec<Id> {
        let Some(guild) = self.current_guild() else {
            return Vec::new();
        };
        guild
            .channels
            .iter()
            .flat_map(|channel| self.sidebar_threads(channel.id))
            .map(|thread| thread.id)
            .collect()
    }

    /// `container_reDhww`: a joined, active thread under its channel: the 10x19 spine 24px in, the link 36px in, the row the sidebar's full 294px.
    pub(super) fn thread_rows(&self, parent: Id, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let threads = self.sidebar_threads(parent);
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
                    .w(px(294.))
                    .h(px(34.))
                    // `spineBorder_reDhww`: one 2px rule down the whole list, ending 24px above the last row's bottom, in
                    // `--spine-default` like the branch (`spine_reDhww`). The token is opaque, so the two can overlap.
                    .child(
                        div()
                            .absolute()
                            .left(px(24.))
                            .top(px(0.))
                            .w(px(2.))
                            .h(px(if index + 1 < count { 34. } else { 10. }))
                            // One rule in the official: only its two ends are rounded, so the rows' pieces meet
                            // without a seam.
                            .when(index == 0, |this| this.rounded_t(px(2.)))
                            .when(index + 1 == count, |this| this.rounded_b(px(2.)))
                            .bg(color("--spine-default")),
                    )
                    .child(
                        div().absolute().left(px(24.)).top(px(0.)).child(
                            gpui::svg()
                                .path("icons/thread_spine.svg")
                                .w(px(10.))
                                .h(px(19.))
                                .text_color(color("--spine-default")),
                        ),
                    )
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
            // The wrapper is `body-sm/normal` (14px) and the input inherits it (`font-size: inherit`, module 0PLWhS).
            let mut style = editor_style();
            style.size = 14.;
            style.line_height = 18.;
            style.font.weight = FontWeight(400.);
            style.text = super::hsla("--input-text-default");
            style.caret = super::hsla("--input-text-default");
            style.placeholder = super::hsla("--input-placeholder-text-default");
            let mut editor = Editor::new(style, cx);
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
        let parent = self.channel.and_then(|channel| {
            let guild = self.current_guild()?;
            match guild.threads.iter().find(|thread| thread.id == channel) {
                Some(thread) => Some(thread.parent_id),
                None => Some(channel),
            }
        });
        self.thread_browser = Some(ThreadBrowser {
            search,
            _events: events,
            query: String::new(),
            parent,
            archived: Vec::new(),
            loading: false,
            has_more: true,
            failed: false,
            scroll: gpui::ScrollHandle::new(),
            #[cfg(feature = "capture")]
            asked: Vec::new(),
        });
        // `Fr` loads the first page of archived threads when the browser mounts (module 424798).
        self.thread_browser_load();
        cx.notify();
    }

    /// `loadArchivedThreads` (module 7909): the next 25 archived threads of the channel, newest activity first.
    /// Nothing is asked while a page is loading, after the last page, or after a failure.
    fn thread_browser_load(&mut self) {
        let Some(browser) = self.thread_browser.as_mut() else {
            return;
        };
        let Some(parent) = browser.parent else {
            return;
        };
        if browser.loading || !browser.has_more || browser.failed {
            return;
        }
        browser.loading = true;
        let route = format!(
            "/channels/{parent}/threads/search?archived=true&sort_by=last_message_time&sort_order=desc&limit=25&tag_setting=match_some&offset={}",
            browser.archived.len()
        );
        tracing::debug!(
            "thread browser: asking for archived threads, offset {}",
            browser.archived.len()
        );
        #[cfg(feature = "capture")]
        browser.asked.push(route.clone());
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{BROWSER_PAGE_KEY}{parent}")),
            method: reqwest::Method::GET,
            route,
            body: None,
        });
    }

    /// `setNotificationSettings` (module 7909): the account's own settings of a thread, `{muted, mute_config}` or
    /// `{flags}`. The thread's member state changes at once (`hs`), the account joins the thread first when it is not a
    /// member, and `PATCH /channels/:id/thread-members/@me/settings` carries the change (`THREAD_MEMBER_SETTINGS`).
    pub(super) fn patch_thread_settings(&mut self, thread: Id, body: serde_json::Value) {
        let mut joined = true;
        for guild in &mut self.stores.guilds {
            let Some(known) = guild.threads.iter_mut().find(|known| known.id == thread) else {
                continue;
            };
            joined = known.joined;
            if let Some(muted) = body["muted"].as_bool() {
                known.muted = muted;
                known.mute_end = body["mute_config"]["end_time"]
                    .as_str()
                    .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                    .map(|time| time.with_timezone(&chrono::Utc));
            }
            if let Some(flags) = body["flags"].as_u64() {
                known.member_flags = Some(flags);
            }
            break;
        }
        if !joined {
            self.join_thread(thread);
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: format!("/channels/{thread}/thread-members/@me/settings"),
            body: Some(body),
        });
    }

    /// The body of Mute Thread's duration `seconds` (`p` in module 812141; `-1` or less is until it is turned back
    /// on, whose `end_time` is null).
    pub(super) fn thread_mute_body(seconds: i64) -> serde_json::Value {
        let end = (seconds > 0).then(|| {
            (chrono::Utc::now() + chrono::Duration::seconds(seconds))
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()
        });
        json!({"muted": true, "mute_config": {"selected_time_window": if seconds > 0 { seconds } else { -1 }, "end_time": end}})
    }

    /// The answer of `thread_browser_load` (`LOAD_ARCHIVED_THREADS_SUCCESS` / `_FAIL`): the threads join the guild's
    /// list as archived ones, their members the joined-threads store and the facepiles.
    pub(super) fn thread_browser_fetched(
        &mut self,
        key: &str,
        report: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(parent) = key
            .strip_prefix(BROWSER_PAGE_KEY)
            .and_then(|parent| parent.parse::<Id>().ok())
        else {
            return false;
        };
        let guild_id = self.guild;
        let Some(browser) = self.thread_browser.as_mut() else {
            return true;
        };
        if browser.parent != Some(parent) {
            return true;
        }
        browser.loading = false;
        if report["ok"].as_bool() != Some(true) {
            browser.failed = true;
            cx.notify();
            return true;
        }
        let body = &report["body"];
        let threads: Vec<Thread> = body["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Thread::from_value)
            .filter(|thread| thread.parent_id == parent)
            .collect();
        browser.has_more = body["has_more"].as_bool().unwrap_or(false) && !threads.is_empty();
        for thread in &threads {
            if !browser.archived.contains(&thread.id) {
                browser.archived.push(thread.id);
            }
        }
        if let Some(guild) = guild_id {
            for entry in body["threads"].as_array().into_iter().flatten() {
                if let Some(member) = dc_state::members::Member::from_value(&entry["owner"], None) {
                    self.stores.members.upsert(guild, member);
                }
            }
        }
        self.stores.thread_members_from_response(guild_id, body);
        self.stores.thread_previews.add_response(guild_id, body);
        if let Some(guild) = self
            .stores
            .guilds
            .iter_mut()
            .find(|guild| Some(guild.id) == guild_id)
        {
            for mut thread in threads {
                match guild.threads.iter_mut().find(|old| old.id == thread.id) {
                    Some(old) => {
                        thread.joined |= old.joined;
                        *old = thread;
                    }
                    None => guild.threads.push(thread),
                }
            }
        }
        cx.notify();
        true
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
        // The answer comes later: the server it names, not the one selected now, is where the thread lives.
        let created_in = created_thread_guild(body, self.guild);
        if let Some(guild) = created_in.and_then(|guild| {
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
        if let Some(guild) = created_in {
            self.select(guild, Some(thread.id));
        }
        cx.notify();
    }

    /// `container_yRVHqK` (module 28579, measured in `design/dom/threads/*.json`): a 72 px row with the name (and in the
    /// modal the parent channel), the last message of the thread or "Started by" for an archived one, and the
    /// facepile of its members. `show_channel_name` is the modal's.
    pub(super) fn thread_card(
        &self,
        thread: &Thread,
        show_channel_name: bool,
        row_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = thread.id;
        let guild = self.guild;
        let has_faces = self
            .stores
            .thread_previews
            .member_ids_preview(id)
            .is_some_and(|ids| !ids.is_empty());
        // `L`: ThreadMessageStore's most recent message, else the message store's (`getMessage` of the same id).
        let last = self
            .stores
            .thread_messages
            .most_recent_message(id)
            .cloned()
            .or_else(|| {
                self.stores
                    .messages
                    .get(&id)
                    .and_then(|list| list.last())
                    .cloned()
            });
        let now = chrono::Utc::now().timestamp_millis();
        let ago = |snowflake: Id| short_ago(now - ((snowflake >> 22) + 1_420_070_400_000) as i64);
        let parent_name = show_channel_name
            .then(|| {
                self.current_guild().and_then(|guild| {
                    guild
                        .channels
                        .iter()
                        .find(|channel| channel.id == thread.parent_id)
                        .map(|channel| format!("#{}", channel.name))
                })
            })
            .flatten();
        let line = || {
            div()
                .h(px(20.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
        };
        let subtext = if thread.archived {
            // `O`: "Started by <owner>  •  Last active <time>".
            let owner = thread
                .owner_id
                .and_then(|owner| self.stores.users.get(&owner))
                .cloned();
            let since = thread
                .last_message_id
                .map(ago)
                .or_else(|| thread.archive_timestamp_ms.map(|at| short_ago(now - at)))
                .unwrap_or_default();
            // `subtext_yRVHqK` is a `body-sm/normal` flex row: 18px tall (the 16px avatar is shorter than the line).
            let mut row = line().h(px(18.));
            if let Some(owner) = owner {
                row = row
                    .child(self.thread_avatar(&owner, 16., cx).mr(px(8.)))
                    // `GFDmd/` "Started by {authorHook}": the words stay `text-default`; the name is
                    // `startedByName_yRVHqK` (semibold, 4px in, at most 150px) in the member's role colour.
                    .child(div().flex_none().child("Started by"))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(150.))
                            .ml(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_weight(FontWeight(600.))
                            .text_color(self.guild_author_color(owner.id, "--text-default"))
                            .child(self.role_named(&owner, owner.display_name().to_owned())),
                    )
                    // `.bullet_yRVHqK`: `--background-mod-muted`, 8px each side.
                    .child(
                        div()
                            .flex_none()
                            .px(px(8.))
                            .text_color(color("--background-mod-muted"))
                            .child("\u{2022}"),
                    );
            }
            row.child(div().flex_none().child(format!("Last active {since}")))
                .into_any_element()
        } else if let Some(message) = last {
            // `D`: avatar, name, ": ", the content (16 px markup, clipped with an ellipsis), " • ", time.
            let author = self
                .stores
                .users
                .get(&message.author.id)
                .cloned()
                .unwrap_or_else(|| message.author.clone());
            let time = ago(message.id);
            // `messageContent` clamps to one line: when the text does not fit the line it wraps at a word and the
            // first line ends in an ellipsis ("gravecord app thread ..."), the box taking the whole free width. The
            // free width is the text column (the row less its 16 px padding and the facepile) less the 24 px avatar,
            // the name, ": ", " * " and the time.
            let faces_width = if has_faces { 40. } else { 0. };
            let fixed = 24.
                + measure_text(window, author.display_name(), 14., 600.)
                + measure_text(window, ":\u{a0}", 14., 400.)
                + measure_text(window, "\u{a0} \u{2022} \u{a0}", 14., 400.)
                + measure_text(window, &time, 14., 400.);
            let free = row_width - 32. - faces_width - fixed;
            let (shown, boxed) = clamp_words(window, &message.content, 16., free);
            line()
                .child(self.thread_avatar(&author, 16., cx).mr(px(8.)))
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight(600.))
                        .text_color(self.guild_author_color(author.id, "--text-default"))
                        .child(self.role_named(&author, author.display_name().to_owned())),
                )
                .child(div().flex_none().child(":\u{a0}"))
                .child(
                    div()
                        .flex_none()
                        .h(px(22.))
                        .when(boxed, |content| content.w(px(free)))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(16.))
                        .line_height(px(22.))
                        .child(shown),
                )
                .child(div().flex_none().child("\u{a0} \u{2022} \u{a0}"))
                .child(div().flex_none().child(time))
                .into_any_element()
        } else {
            // `B`: "No recent messages  •  <time>".
            let since = ago(thread.last_message_id.unwrap_or(thread.id));
            line()
                .child(format!("No recent messages\u{a0} \u{2022} \u{a0}{since}"))
                .into_any_element()
        };
        // `U`: the thread's members, from `getMemberIdsPreview` and `getMemberCount` (at most 50 shown).
        let preview: Vec<Id> = self
            .stores
            .thread_previews
            .member_ids_preview(id)
            .map(|ids| ids.iter().copied().take(50).collect())
            .unwrap_or_default();
        let count = self.stores.thread_previews.member_count(id).unwrap_or(0);
        let faces = (!preview.is_empty()).then(|| {
            let extra = count.saturating_sub(preview.len() as u64);
            let mut stack = div()
                .flex_none()
                .ml(px(16.))
                .h(px(40.))
                .flex()
                .items_center();
            for (index, user) in preview.iter().enumerate() {
                let face = match self.stores.users.get(user).cloned() {
                    Some(user) => self.thread_avatar(&user, 24., cx),
                    None => div()
                        .flex_none()
                        .size(px(24.))
                        .rounded_full()
                        .bg(color("--background-mod-strong")),
                };
                // The overlap of a stack is not in the captures (one member only); 8 px is assumed.
                stack = stack.child(face.when(index > 0, |face| face.ml(px(-8.))));
            }
            if extra > 0 {
                stack = stack.child(
                    div()
                        .ml(px(-8.))
                        .h(px(24.))
                        .min_w(px(24.))
                        .px(px(4.))
                        .rounded_full()
                        .bg(color("--background-mod-strong"))
                        .text_size(px(12.))
                        .line_height(px(24.))
                        .text_color(color("--text-default"))
                        .child(format!("+{extra}")),
                );
            }
            stack
        });
        div()
            .id(SharedString::from(format!("thread-card-{id}")))
            .w_full()
            .h(px(72.))
            .flex_none()
            .mb(px(8.))
            .p(px(15.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .cursor_pointer()
            .flex()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.thread_browser = None;
                this.thread_modal = false;
                if let Some(guild) = guild {
                    this.select(guild, Some(id));
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(40.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(
                                div()
                                    .flex_shrink(1.)
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(thread.name.clone()),
                            )
                            .children(parent_name.map(|name| {
                                div()
                                    .flex_none()
                                    .ml(px(8.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(name)
                            })),
                    )
                    .child(subtext),
            )
            .children(faces)
            .into_any_element()
    }

    /// A round avatar of `size` px (the default avatar when the account has none), requested when missing.
    fn thread_avatar(&self, user: &dc_state::User, size: f32, cx: &mut Context<Self>) -> gpui::Div {
        let (key, url) = avatar_source(user);
        match self.images.get(&key).cloned() {
            Some(image) => div()
                .flex_none()
                .size(px(size))
                .child(img(image).size(px(size)).rounded_full()),
            None => {
                self.want_image(key, url, cx);
                div()
                    .flex_none()
                    .size(px(size))
                    .rounded_full()
                    .bg(color("--background-mod-strong"))
            }
        }
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
        let search_focused =
            gpui::Focusable::focus_handle(browser.search.read(cx), cx).is_focused(window);
        let query = browser.query.trim().to_lowercase();
        let threads: Vec<Thread> = self
            .guild_threads(self.channel)
            .into_iter()
            .filter(|thread| !thread.archived)
            .filter(|thread| query.is_empty() || thread.name.to_lowercase().contains(&query))
            .collect();
        let joined: Vec<&Thread> = threads.iter().filter(|thread| thread.joined).collect();
        let others: Vec<&Thread> = threads.iter().filter(|thread| !thread.joined).collect();
        // The third section: the archived threads loaded so far, as the store returned them (newest activity first).
        let archived: Vec<Thread> = self
            .current_guild()
            .map(|guild| {
                browser
                    .archived
                    .iter()
                    .filter_map(|id| guild.threads.iter().find(|thread| thread.id == *id))
                    .filter(|thread| {
                        query.is_empty() || thread.name.to_lowercase().contains(&query)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let loading_first = threads.is_empty() && archived.is_empty() && browser.loading;
        let empty = threads.is_empty() && archived.is_empty() && !loading_first;
        let picture = self.images.get("threads:empty").cloned();
        let mut list: Vec<AnyElement> = Vec::new();
        let heading = |text: String| {
            // `sectionHeader_6_wQXw`: 14 px, weight 500, 8 px above and below (the box is 24 high).
            // The recording shows the text 1px below where the 18.67px line box rounds to in this text system.
            div()
                .mt(px(8.))
                .mb(px(8.))
                .h(px(24.))
                .relative()
                .top(px(1.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.67))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(text)
                .into_any_element()
        };
        if !joined.is_empty() {
            list.push(heading(format!("{} joined threads", joined.len())));
            list.extend(
                joined
                    .iter()
                    .map(|thread| self.thread_card(thread, false, 440., window, cx)),
            );
        }
        if !others.is_empty() {
            list.push(heading(format!("{} other active threads", others.len())));
            list.extend(
                others
                    .iter()
                    .map(|thread| self.thread_card(thread, false, 440., window, cx)),
            );
        }
        if !archived.is_empty() {
            // Module 628707 `wUNQ+w`.
            list.push(heading("older threads".to_owned()));
            list.extend(
                archived
                    .iter()
                    .map(|thread| self.thread_card(thread, false, 440., window, cx)),
            );
        }
        let scroll = browser.scroll.clone();
        let body: AnyElement = if loading_first {
            // `R` (module 628707): the `d.y` spinner, wandering cubes by default, in the list's `padding: 0 16px` with
            // `.spinner_6_wQXw { margin: 64px 0 }` (module 798393), centred by `.spinner_3H5mjG { display: flex }`.
            div()
                .h(px(351.))
                .px(px(16.))
                .pt(px(64.))
                .flex()
                .justify_center()
                .items_start()
                .child(super::login::wandering_cubes())
                .into_any_element()
        } else if empty {
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
                .track_scroll(&scroll)
                // `j`: the next page is asked for when less than 200 px of the list remain below the view.
                .on_scroll(cx.listener(|this, _: &gpui::ScrollEvent, _, cx| {
                    let near = this.thread_browser.as_ref().is_some_and(|browser| {
                        let max = browser.scroll.max_offset().y;
                        max + browser.scroll.offset().y < px(200.)
                    });
                    if near {
                        this.thread_browser_load();
                        cx.notify();
                    }
                }))
                // Rows are x432..872 of the 416..896 panel: 16 left, 24 right (the scrollbar's 8 px gutter).
                .pl(px(16.))
                .pr(px(24.))
                .children(list)
                .into_any_element()
        };
        let popout = div()
            .parity("thread-browser")
            .id("thread-browser")
            .occlude()
            .w(px(480.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                if this.press_outside(super::keyboard::Layer::ThreadBrowser, cx)
                    && event.button != gpui::MouseButton::Right
                {
                    cx.stop_propagation();
                }
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
                        // `.wrapper_UcGqzv`: a 1px border, and `:has(input:focus)` adds the 2px inset outline in the same
                        // colour, so a focused box shows a 2px ring (`--input-border-active`); unfocused it is
                        // `--input-border-default`.
                        div()
                            .w(px(220.5))
                            .h(px(32.))
                            .when(search_focused, |this| this.pl(px(3.)).border_2())
                            .when(!search_focused, |this| this.pl(px(4.)).border_1())
                            .rounded(px(8.))
                            .border_color(color(if search_focused {
                                "--input-border-active"
                            } else {
                                "--input-border-default"
                            }))
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
            deferred(
                div()
                    .absolute()
                    .top(px(81.))
                    .left(px(left))
                    .child(super::keyboard::outside(
                        popout,
                        super::keyboard::Layer::ThreadBrowser,
                        cx,
                    )),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// "All Active Threads" (module 400954): every active thread of the guild, newest message first, as 80 px rows with
    /// the parent channel's name, in a 480 by 608 list modal (a 72 px header, the list, a footer with the full-width
    /// Close). Measured in `design/golden/windows/threads/active-threads-modal-darker-cozy@1.png`.
    pub(super) fn active_threads_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.thread_modal {
            return None;
        }
        let guild = self.current_guild()?;
        let mut threads: Vec<Thread> = guild
            .threads
            .iter()
            .filter(|thread| !thread.archived)
            .cloned()
            .collect();
        threads
            .sort_by_key(|thread| std::cmp::Reverse(thread.last_message_id.unwrap_or(thread.id)));
        let rows: Vec<AnyElement> = threads
            .iter()
            .map(|thread| self.thread_card(thread, true, 430., window, cx))
            .collect();
        // The divider colour is read from the golden's pixels (50, 50, 55 on the modal's 36, 36, 41).
        let divider = gpui::rgba(0x323237ff);
        let modal = div()
            .parity("active-threads-modal")
            .id("active-threads-modal")
            .occlude()
            .w(px(480.))
            .h(px(608.))
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(divider)
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .h(px(72.))
                    .px(px(24.))
                    .border_b_1()
                    .border_color(divider)
                    .flex()
                    .items_center()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("All Active Threads")
                    // The X is a 14 px glyph centred at (850.5, 84.5) of the golden: a 40 px button 8 px from the
                    // right edge and 7.5 px from the top of the modal.
                    .child(
                        div()
                            .id("active-threads-x")
                            .absolute()
                            .top(px(8.5))
                            .right(px(8.))
                            .size(px(40.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.thread_modal = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("active-threads-list")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px(px(24.))
                    .pt(px(12.))
                    .children(rows),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(88.))
                    .p(px(24.))
                    .border_t_1()
                    .border_color(divider)
                    .child(
                        div()
                            .id("active-threads-done")
                            .h(px(40.))
                            .w_full()
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.thread_modal = false;
                                cx.notify();
                            }))
                            .child("Close"),
                    ),
            );
        let pose = crate::ui::motion::modal_pose("active-threads");
        Some(
            deferred(
                div()
                    .id("active-threads-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(gpui::rgba(0x000000b8)))
                    .flex()
                    .items_start()
                    .justify_center()
                    .pt(px(56.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.thread_modal = false;
                        cx.notify();
                    }))
                    .child(modal.posed(pose)),
            )
            .with_priority(1)
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

/// The server a created thread belongs to: its own `guild_id`, else the one selected.
fn created_thread_guild(body: &serde_json::Value, selected: Option<Id>) -> Option<Id> {
    body["guild_id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .or(selected)
}

#[cfg(test)]
mod created_tests {
    use super::*;

    #[test]
    fn a_created_thread_belongs_to_the_server_in_its_answer() {
        let body = json!({"id": "9", "guild_id": "55"});
        assert_eq!(created_thread_guild(&body, Some(7)), Some(55));
        assert_eq!(created_thread_guild(&json!({"id": "9"}), Some(7)), Some(7));
    }
}

#[cfg(test)]
mod ago_tests {
    use super::short_ago;

    #[test]
    fn the_age_follows_the_official_thresholds() {
        let minute = 60_000;
        assert_eq!(short_ago(5_000), "1m ago");
        assert_eq!(short_ago(59 * 1000), "1m ago");
        assert_eq!(short_ago(minute), "1m ago");
        assert_eq!(short_ago(45 * minute), "45m ago");
        assert_eq!(short_ago(60 * minute), "1h ago");
        assert_eq!(short_ago(23 * 60 * minute), "23h ago");
        assert_eq!(short_ago(24 * 60 * minute), "1d ago");
        assert_eq!(short_ago(29 * 24 * 60 * minute), "29d ago");
        assert_eq!(short_ago(30 * 24 * 60 * minute), ">30d ago");
        assert_eq!(short_ago(359 * 24 * 60 * minute), ">30d ago");
    }

    #[test]
    fn a_thread_mute_carries_its_window_and_end_time() {
        let hour = super::Root::thread_mute_body(3600);
        assert_eq!(hour["muted"], true);
        assert_eq!(hour["mute_config"]["selected_time_window"], 3600);
        let end = hour["mute_config"]["end_time"].as_str().unwrap();
        let end = chrono::DateTime::parse_from_rfc3339(end).unwrap();
        let ahead = (end.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds();
        assert!((3590..=3600).contains(&ahead), "{ahead}");
        // "Until I turn it back on" is -1 with no end (module 812141 `p`).
        for forever in [0, -1] {
            let body = super::Root::thread_mute_body(forever);
            assert_eq!(body["mute_config"]["selected_time_window"], -1);
            assert!(body["mute_config"]["end_time"].is_null());
        }
    }
}
