//! Forum browsing: modules 473503 (preview batches), 7909 (archive/search), 207777 (active sorting),
//! 422844 (local filters), and the list geometry in channel/pt-forum-bugs-darker-cozy.
use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Channel, Id, Message, Thread};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ScrollHandle, SharedString, Subscription,
    Task, Window, div, img, prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

pub(super) struct Forum {
    field: Entity<Editor>,
    _events: Subscription,
    debounce: Option<Task<()>>,
    query: String,
    generation: u64,
    sort: u64,
    gallery: bool,
    tags: BTreeSet<Id>,
    match_all: bool,
    guidelines: bool,
    menu: Option<&'static str>,
    posts: HashMap<Id, Message>,
    requested: HashSet<Id>,
    archived: Vec<Id>,
    results: Option<Vec<Id>>,
    loading: bool,
    has_more: bool,
    /// `ArchivedThreadsStore` marks a failed page `failed`, and `getCanLoadMore` is false while it is set (module
    /// 45494); nothing is drawn for it. A 429 lets the next scroll retry once `retry_at` has passed.
    failed: bool,
    retry_at: Option<Instant>,
    scroll: ScrollHandle,
    new_post: Option<NewPost>,
}

struct NewPost {
    title: Entity<Editor>,
    body: Entity<Editor>,
    tags: BTreeSet<Id>,
    sending: bool,
    error: Option<String>,
    _events: Vec<Subscription>,
}

fn post_body(name: &str, content: &str, archive: Option<u64>, tags: &BTreeSet<Id>) -> Value {
    // Module 971389, forum create; 268761 supplies the three-day fallback.
    json!({"name":name, "auto_archive_duration":archive.unwrap_or(4320),
        "applied_tags":tags.iter().map(Id::to_string).collect::<Vec<_>>(),
        "message":{"content":content,"sticker_ids":[]}})
}

fn search_route(channel: Id, query: &str, tags: &BTreeSet<Id>, all: bool, offset: usize) -> String {
    let mut route = format!("/channels/{channel}/threads/search?");
    if query.is_empty() {
        route.push_str(&format!(
            "archived=true&sort_by=last_message_time&sort_order=desc&limit=25&offset={offset}"
        ));
    } else {
        route.push_str(&format!("name={}", super::search::encode(query)));
    }
    route.push_str(if all {
        "&tag_setting=match_all"
    } else {
        "&tag_setting=match_some"
    });
    if !tags.is_empty() {
        route.push_str("&tag=");
        route.push_str(&tags.iter().map(Id::to_string).collect::<Vec<_>>().join(","));
    }
    route
}

/// A failed page blocks further loads until the user changes the filter or reopens the forum; a 429 only holds
/// them back until its `retry_after` has passed.
fn can_retry(failed: bool, retry_at: Option<Instant>, now: Instant) -> bool {
    !failed || retry_at.is_some_and(|at| now >= at)
}

fn retry_deadline(report: &Value, now: Instant) -> Option<Instant> {
    if report["status"].as_u64() != Some(429) {
        return None;
    }
    let seconds = report["body"]["retry_after"].as_f64()?;
    (seconds.is_finite() && seconds > 0.).then(|| now + Duration::from_secs_f64(seconds.min(3600.)))
}

/// Text for a failed post creation: the cases of module 971389's catch, each as its toast body.
fn create_failure_text(report: &Value) -> String {
    let body = &report["body"];
    match body["code"].as_u64() {
        Some(160006) => "This server has reached the maximum number of active posts. Free up space by archiving a post.".to_owned(),
        Some(160007) => "This server has reached the maximum number of active announcement threads. Free up space by archiving a thread.".to_owned(),
        _ if report["status"].as_u64() == Some(429) => "You are being rate limited.".to_owned(),
        Some(50035) if body["errors"]["name"].is_object() => body["errors"]["name"]["_errors"][0]
            ["message"]
            .as_str()
            .unwrap_or("An error occurred, please try again.")
            .to_owned(),
        _ => "An error occurred, please try again.".to_owned(),
    }
}

fn matching(thread: &Thread, tags: &BTreeSet<Id>, all: bool) -> bool {
    tags.is_empty()
        || if all {
            tags.iter().all(|tag| thread.applied_tags.contains(tag))
        } else {
            tags.iter().any(|tag| thread.applied_tags.contains(tag))
        }
}

fn active_threads(
    threads: &[Thread],
    parent: Id,
    sort: u64,
    tags: &BTreeSet<Id>,
    all: bool,
) -> Vec<Id> {
    let mut threads: Vec<_> = threads
        .iter()
        .filter(|t| t.parent_id == parent && !t.archived && matching(t, tags, all))
        .collect();
    threads.sort_by_key(|t| {
        (
            std::cmp::Reverse(t.pinned()),
            std::cmp::Reverse(if sort == 0 {
                t.last_message_id.unwrap_or(t.id)
            } else {
                t.id
            }),
        )
    });
    threads.into_iter().map(|t| t.id).collect()
}

fn page_threads(body: &Value, parent: Id) -> Vec<Thread> {
    body["threads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Thread::from_value)
        .filter(|t| t.parent_id == parent)
        .collect()
}

fn gallery_columns(width: f32) -> (usize, f32) {
    // Module 745431, sR: minimum 320, maximum 450, gap 16.
    if width <= 320. {
        return (1, 320.);
    }
    let columns = ((width / 450. + width / 320.) / 2.).floor().max(1.) as usize;
    (
        columns,
        (width - 16. * (columns - 1) as f32) / columns as f32,
    )
}

fn gallery_height(width: f32, media: bool) -> f32 {
    120. + ((width - 26.) * if media { 0.5625 } else { 2. / 3. }).ceil()
}

impl Root {
    pub(super) fn close_forum_menu(&mut self) -> bool {
        self.channel
            .and_then(|id| self.forums.get_mut(&id))
            .is_some_and(|view| view.menu.take().is_some())
    }
    #[cfg(feature = "capture")]
    pub(super) fn forum_editors(&self) -> Vec<Entity<Editor>> {
        let Some(view) = self.channel.and_then(|id| self.forums.get(&id)) else {
            return Vec::new();
        };
        if let Some(form) = &view.new_post {
            vec![form.title.clone(), form.body.clone()]
        } else {
            vec![view.field.clone()]
        }
    }

    fn open_forum_post(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        if !self.current_channel().is_some_and(|c| {
            c.id == id && self.can_in_channel(c, dc_state::permissions::SEND_MESSAGES)
        }) {
            return;
        }
        let title = cx.new(|cx| {
            let mut style = editor_style();
            style.size = 20.;
            style.line_height = 32.;
            style.font.weight = FontWeight(600.);
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder("Title".into());
            editor.set_keeps_text(true);
            editor.set_reports_all_edits(true);
            editor
        });
        let body = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Enter a message...".into());
            editor.set_enter_inserts_newline(true);
            editor.set_reports_all_edits(true);
            editor
        });
        let events = [&title, &body]
            .into_iter()
            .map(|editor| {
                cx.subscribe(editor, move |this, _, event: &EditorEvent, cx| {
                    if matches!(event, EditorEvent::Cancel)
                        && let Some(v) = this.forums.get_mut(&id)
                    {
                        v.new_post = None;
                    }
                    cx.notify();
                })
            })
            .collect();
        window.focus(&gpui::Focusable::focus_handle(title.read(cx), cx), cx);
        if let Some(view) = self.forums.get_mut(&id) {
            view.new_post = Some(NewPost {
                title,
                body,
                tags: BTreeSet::new(),
                sending: false,
                error: None,
                _events: events,
            });
        }
        cx.notify();
    }

    fn submit_forum_post(&mut self, id: Id, cx: &mut Context<Self>) {
        let Some(channel) = self.current_channel().filter(|c| c.id == id).cloned() else {
            return;
        };
        if !self.can_in_channel(&channel, dc_state::permissions::SEND_MESSAGES) {
            return;
        }
        let Some(form) = self.forums.get_mut(&id).and_then(|v| v.new_post.as_mut()) else {
            return;
        };
        let name = form.title.read(cx).text().trim().to_owned();
        let content = form.body.read(cx).text().to_owned();
        if form.sending
            || name.is_empty()
            || content.trim().is_empty()
            || (channel.flags & 16 != 0 && form.tags.is_empty())
        {
            return;
        }
        form.sending = true;
        form.error = None;
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("forum-create:{id}")),
            method: reqwest::Method::POST,
            route: format!("/channels/{id}/threads?use_nested_fields=true"),
            body: Some(post_body(
                &name,
                &content,
                channel.default_auto_archive_duration,
                &form.tags,
            )),
        });
        cx.notify();
    }

    fn forum_form(&self, channel: &Channel, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = channel.id;
        let form = self.forums.get(&id)?.new_post.as_ref()?;
        let enabled = !form.sending
            && !form.title.read(cx).text().trim().is_empty()
            && !form.body.read(cx).text().trim().is_empty()
            && (channel.flags & 16 == 0 || !form.tags.is_empty());
        Some(
            div()
                .parity("forum-new-post")
                .border_1()
                .border_color(color("--border-subtle"))
                .rounded(px(12.))
                .bg(color("--input-background-default"))
                .pt(px(8.))
                .child(
                    div()
                        .px(px(12.))
                        .flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("forum-close-post")
                                .size(px(26.))
                                .mt(px(3.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("close", 16., "--text-muted"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(v) = this.forums.get_mut(&id) {
                                        v.new_post = None;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .child(div().h(px(32.)).child(form.title.clone()))
                                .child(div().min_h(px(80.)).child(form.body.clone())),
                        ),
                )
                .when(!channel.available_tags.is_empty(), |row| {
                    row.child(
                        div()
                            .px(px(12.))
                            .py(px(8.))
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .children(
                                channel
                                    .available_tags
                                    .iter()
                                    .filter(|tag| {
                                        !tag.moderated
                                            || self.can_in_channel(
                                                channel,
                                                dc_state::permissions::MANAGE_THREADS,
                                            )
                                    })
                                    .map(|tag| {
                                        let tag_id = tag.id;
                                        div()
                                            .id(SharedString::from(format!(
                                                "forum-create-tag-{tag_id}"
                                            )))
                                            .px(px(12.))
                                            .h(px(32.))
                                            .rounded_full()
                                            .cursor_pointer()
                                            .bg(color(if form.tags.contains(&tag_id) {
                                                "--brand-500"
                                            } else {
                                                "--background-mod-subtle"
                                            }))
                                            .text_color(color("--text-default"))
                                            .text_size(px(14.))
                                            .line_height(px(32.))
                                            .child(tag.name.clone())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(form) = this
                                                    .forums
                                                    .get_mut(&id)
                                                    .and_then(|v| v.new_post.as_mut())
                                                {
                                                    if form.tags.contains(&tag_id) {
                                                        form.tags.remove(&tag_id);
                                                    } else if form.tags.len() < 5 {
                                                        form.tags.insert(tag_id);
                                                    }
                                                }
                                                cx.notify();
                                            }))
                                    }),
                            ),
                    )
                })
                .children(form.error.clone().map(|error| {
                    div()
                        .px(px(12.))
                        .text_color(color("--text-danger"))
                        .child(error)
                }))
                .child(
                    div()
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .h(px(61.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(
                            div()
                                .id("forum-submit-post")
                                .h(px(32.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .bg(color("--brand-500"))
                                .when(!enabled, |row| row.opacity(0.5))
                                .when(enabled, |row| row.cursor_pointer())
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(14.))
                                .text_color(color("--white"))
                                .child(icon("forum_new_post", 16., "--white"))
                                .child("Post")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if enabled {
                                        this.submit_forum_post(id, cx);
                                    }
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    fn ensure_forum(&mut self, channel: &Channel, cx: &mut Context<Self>) {
        let id = channel.id;
        if let std::collections::hash_map::Entry::Vacant(entry) = self.forums.entry(id) {
            let field = cx.new(|cx| {
                let mut style = editor_style();
                style.size = 20.;
                style.line_height = 28.;
                style.font.weight = FontWeight(600.);
                let mut editor = Editor::new(style, cx);
                editor.set_reports_all_edits(true);
                editor.set_placeholder("Search".to_owned());
                editor
            });
            let events = cx.subscribe(&field, move |this, editor, event: &EditorEvent, cx| {
                let query = match event {
                    EditorEvent::Submit(text) => {
                        editor.update(cx, |editor, _| editor.set_text(text));
                        text.clone()
                    }
                    EditorEvent::Edited => editor.read(cx).text().to_owned(),
                    EditorEvent::Cancel => {
                        editor.update(cx, |editor, _| editor.set_text(""));
                        String::new()
                    }
                };
                let Some(view) = this.forums.get_mut(&id) else {
                    return;
                };
                view.query = query;
                view.generation += 1;
                let generation = view.generation;
                view.debounce = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(350))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if this
                            .forums
                            .get(&id)
                            .is_some_and(|v| v.generation == generation)
                        {
                            this.reload_forum(id);
                            cx.notify();
                        }
                    });
                }));
                cx.notify();
            });
            entry.insert(Forum {
                field,
                _events: events,
                debounce: None,
                query: String::new(),
                generation: 0,
                sort: channel.default_sort_order.unwrap_or(0),
                gallery: channel.is_media() || channel.default_forum_layout == 2,
                tags: BTreeSet::new(),
                match_all: false,
                guidelines: false,
                menu: None,
                posts: HashMap::new(),
                requested: HashSet::new(),
                archived: Vec::new(),
                results: None,
                loading: false,
                has_more: true,
                failed: false,
                retry_at: None,
                scroll: ScrollHandle::new(),
                new_post: None,
            });
            self.load_forum_page(id);
        }
        let known: Vec<_> = self
            .current_guild()
            .into_iter()
            .flat_map(|g| &g.threads)
            .filter(|t| t.parent_id == id && !t.archived)
            .map(|t| t.id)
            .collect();
        let view = self.forums.get_mut(&id).unwrap();
        let missing: Vec<_> = known
            .into_iter()
            .filter(|post| !view.posts.contains_key(post) && view.requested.insert(*post))
            .collect();
        for batch in missing.chunks(10) {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("forum-preview:{id}:{}", batch[0])),
                method: reqwest::Method::POST,
                route: format!("/channels/{id}/post-data"),
                body: Some(
                    json!({"thread_ids":batch.iter().map(Id::to_string).collect::<Vec<_>>()}),
                ),
            });
        }
    }

    /// Opening the forum again retries a failed page (the failed entry is dropped when the filter changes in 45494).
    pub(super) fn forum_reopened(&mut self, channel: Id) {
        if let Some(view) = self.forums.get_mut(&channel)
            && view.failed
        {
            view.failed = false;
            view.retry_at = None;
        }
    }

    fn reload_forum(&mut self, channel: Id) {
        if let Some(view) = self.forums.get_mut(&channel) {
            view.generation += 1;
            view.archived.clear();
            view.results = None;
            view.loading = false;
            view.has_more = true;
            view.failed = false;
            view.retry_at = None;
            view.requested.clear();
            view.scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        self.load_forum_page(channel);
    }

    fn load_forum_page(&mut self, channel: Id) {
        let Some(view) = self.forums.get_mut(&channel) else {
            return;
        };
        if view.loading || !view.has_more || !can_retry(view.failed, view.retry_at, Instant::now())
        {
            return;
        }
        view.failed = false;
        view.retry_at = None;
        view.loading = true;
        let route = search_route(
            channel,
            &view.query,
            &view.tags,
            view.match_all,
            view.archived.len(),
        );
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("forum-page:{channel}:{}", view.generation)),
            method: reqwest::Method::GET,
            route,
            body: None,
        });
    }

    pub(super) fn forum_fetched(
        &mut self,
        key: &str,
        report: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(id) = key.strip_prefix("forum-create:") {
            if let Ok(id) = id.parse::<Id>() {
                let created = if report["ok"].as_bool() == Some(true) {
                    Thread::from_value(&report["body"]).filter(|t| t.parent_id == id)
                } else {
                    None
                };
                let guild_id = self
                    .stores
                    .guilds
                    .iter()
                    .find(|g| g.channels.iter().any(|c| c.id == id))
                    .map(|g| g.id);
                if let (Some(mut thread), Some(guild_id)) = (created, guild_id) {
                    thread.joined = true;
                    let post = thread.id;
                    if let Some(guild) = self.stores.guilds.iter_mut().find(|g| g.id == guild_id) {
                        guild.threads.retain(|t| t.id != post);
                        guild.threads.push(thread);
                    }
                    if let Some(view) = self.forums.get_mut(&id) {
                        view.new_post = None;
                    }
                    if self.channel == Some(id) {
                        self.select(guild_id, Some(post));
                    }
                } else if let Some(form) =
                    self.forums.get_mut(&id).and_then(|v| v.new_post.as_mut())
                {
                    // A 2xx whose thread cannot be built is the same failure as an error status.
                    form.sending = false;
                    form.error = Some(create_failure_text(report));
                }
            }
            cx.notify();
            return true;
        }
        let Some((kind, tail)) = key.split_once(':') else {
            return false;
        };
        if !matches!(kind, "forum-page" | "forum-preview") {
            return false;
        }
        let Some((channel, serial)) = tail.split_once(':') else {
            return true;
        };
        let Ok(channel) = channel.parse::<Id>() else {
            return true;
        };
        let Some(view) = self.forums.get_mut(&channel) else {
            return true;
        };
        if kind == "forum-page" && serial.parse::<u64>().ok() != Some(view.generation) {
            return true;
        }
        if kind == "forum-page" {
            view.loading = false;
        }
        if report["ok"].as_bool() != Some(true) {
            if kind == "forum-page" {
                view.failed = true;
                view.retry_at = retry_deadline(report, Instant::now());
                if !view.query.is_empty() {
                    // FORUM_SEARCH_FAILURE: loading ends and the results become an empty list (768953).
                    view.results = Some(Vec::new());
                }
            }
            cx.notify();
            return true;
        }
        let body = &report["body"];
        let mut messages = Vec::new();
        let guild_id = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel))
            .map(|guild| guild.id);
        if kind == "forum-preview" {
            for entry in body["threads"]
                .as_object()
                .into_iter()
                .flat_map(|v| v.values())
            {
                messages.extend(Message::from_value(&entry["first_message"]));
                if let Some(guild) = guild_id
                    && let Some(member) =
                        dc_state::members::Member::from_value(&entry["owner"], None)
                {
                    self.stores.members.upsert(guild, member);
                }
            }
        } else {
            let threads = page_threads(body, channel);
            if let Some(guild) = guild_id {
                for thread in body["threads"].as_array().into_iter().flatten() {
                    if let Some(member) =
                        dc_state::members::Member::from_value(&thread["owner"], None)
                    {
                        self.stores.members.upsert(guild, member);
                    }
                }
            }
            let ids: Vec<_> = threads.iter().map(|t| t.id).collect();
            if view.query.is_empty() {
                for id in ids {
                    if !view.archived.contains(&id) {
                        view.archived.push(id);
                    }
                }
                view.has_more = body["has_more"].as_bool().unwrap_or(false) && !threads.is_empty();
            } else {
                view.results = Some(ids);
                view.has_more = false;
            }
            if let Some(guild) = self
                .stores
                .guilds
                .iter_mut()
                .find(|g| g.channels.iter().any(|c| c.id == channel))
            {
                for mut thread in threads {
                    if let Some(old) = guild.threads.iter_mut().find(|old| old.id == thread.id) {
                        thread.joined |= old.joined;
                        *old = thread;
                    } else {
                        guild.threads.push(thread);
                    }
                }
            }
            messages.extend(
                body["first_messages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Message::from_value),
            );
        }
        let mut images = Vec::new();
        for message in messages {
            for attachment in &message.attachments {
                if attachment.is_image() {
                    images.push((
                        format!("forum-thumbnail:{}", attachment.id),
                        attachment.proxy_url.clone(),
                    ));
                }
            }
            self.stores
                .users
                .insert(message.author.id, message.author.clone());
            view.posts.insert(message.channel_id, message);
        }
        for (key, url) in images {
            self.request_image(key, url);
        }
        cx.notify();
        true
    }

    fn forum_card(&self, channel: &Channel, thread: &Thread, cx: &mut Context<Self>) -> AnyElement {
        let view = &self.forums[&channel.id];
        let message = view.posts.get(&thread.id);
        let unread = self.stores.is_unread(thread.id);
        let id = thread.id;
        let guild = self.guild;
        let tags: Vec<_> = thread
            .applied_tags
            .iter()
            .filter_map(|id| channel.available_tags.iter().find(|tag| tag.id == *id))
            .collect();
        let timestamp = if view.sort == 1 {
            id
        } else {
            thread.last_message_id.unwrap_or(id)
        };
        let elapsed =
            chrono::Utc::now().timestamp_millis() - ((timestamp >> 22) + 1_420_070_400_000) as i64;
        let ago = if elapsed > 30 * 86_400_000 {
            ">30d ago".to_owned()
        } else {
            super::threads::short_ago(elapsed)
        };
        let thumbnail = message
            .and_then(|m| m.attachments.iter().find(|a| a.is_image()))
            .and_then(|a| self.images.get(&format!("forum-thumbnail:{}", a.id)))
            .cloned();
        div()
            .id(SharedString::from(format!("forum-post-{id}")))
            .when(thread.pinned(), |row| {
                row.parity_with_border("forum-pinned-card", 1.)
            })
            .w_full()
            .flex_none()
            .mb(px(16.))
            .px(px(10.))
            .py(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(12.))
            .bg(color("--background-base-low"))
            .hover(|s| s.bg(color("--background-mod-subtle")))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(guild) = guild {
                    this.select(guild, Some(id));
                }
                cx.notify();
            }))
            .flex()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .when(thread.pinned() || !tags.is_empty(), |row| {
                        row.child(
                            div()
                                .h(px(24.))
                                .mb(px(8.))
                                .flex()
                                .gap(px(8.))
                                .when(thread.pinned(), |row| {
                                    row.child(
                                        div()
                                            .size(px(22.))
                                            .rounded_full()
                                            .bg(color("--brand-500"))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("forum_pin", 16., "--white")),
                                    )
                                })
                                .children(tags.iter().map(|tag| {
                                    div()
                                        .rounded_full()
                                        .px(px(8.))
                                        .h(px(24.))
                                        .text_size(px(12.))
                                        .line_height(px(24.))
                                        .font_weight(FontWeight(500.))
                                        .bg(color("--background-mod-subtle"))
                                        .text_color(color("--text-strong"))
                                        .child(tag.name.clone())
                                })),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color(if unread {
                                "--text-strong"
                            } else {
                                "--text-muted"
                            }))
                            .mb(px(6.))
                            .child(thread.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .h(px(18.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(color(if unread {
                                "--text-default"
                            } else {
                                "--text-muted"
                            }))
                            .children(message.map(|m| {
                                let name = self.guild_display_name(&m.author);
                                let text = format!("{name}: {}", m.content.replace('\n', " "));
                                gpui::StyledText::new(text).with_highlights([(
                                    0..name.len(),
                                    gpui::HighlightStyle {
                                        color: Some(
                                            self.guild_author_color(m.author.id, "--text-muted")
                                                .into(),
                                        ),
                                        font_weight: Some(FontWeight(500.)),
                                        ..Default::default()
                                    },
                                )])
                            })),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(icon("forum_reply_count", 14., "--text-default"))
                                    .child(thread.message_count.to_string()),
                            )
                            .child(div().text_color(color("--border-subtle")).child("·"))
                            .child(ago),
                    ),
            )
            .children(thumbnail.map(|picture| {
                img(picture)
                    .size(px(72.))
                    .rounded(px(8.))
                    .object_fit(gpui::ObjectFit::Cover)
            }))
            .into_any_element()
    }

    fn forum_gallery_card(
        &self,
        channel: &Channel,
        thread: &Thread,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = &self.forums[&channel.id];
        let message = view.posts.get(&thread.id);
        let id = thread.id;
        let guild = self.guild;
        let elapsed =
            chrono::Utc::now().timestamp_millis() - ((id >> 22) + 1_420_070_400_000) as i64;
        let ago = if elapsed > 30 * 86_400_000 {
            ">30d ago".to_owned()
        } else {
            super::threads::short_ago(elapsed)
        };
        let thumbnail = message
            .and_then(|m| m.attachments.iter().find(|a| a.is_image()))
            .and_then(|a| self.images.get(&format!("forum-thumbnail:{}", a.id)))
            .cloned();
        let height = gallery_height(width, channel.is_media());
        div()
            .id(SharedString::from(format!("forum-gallery-post-{id}")))
            .when(thread.pinned(), |row| {
                row.parity_with_border("forum-pinned-gallery-card", 1.)
            })
            .w(px(width))
            .h(px(height))
            .flex_none()
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(12.))
            .bg(color("--background-base-low"))
            .overflow_hidden()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(guild) = guild {
                    this.select(guild, Some(id));
                }
                cx.notify();
            }))
            .child(
                div()
                    .px(px(12.))
                    .pt(px(12.))
                    .h(px(66.))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .children(message.map(|m| {
                                div()
                                    .text_color(
                                        self.guild_author_color(m.author.id, "--text-default"),
                                    )
                                    .child(self.guild_display_name(&m.author))
                            }))
                            .child(div().text_size(px(12.)).child(ago)),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color(if self.stores.is_unread(id) {
                                "--text-strong"
                            } else {
                                "--text-muted"
                            }))
                            .child(thread.name.clone()),
                    ),
            )
            .child(
                div()
                    .mx(px(12.))
                    .h(px(height - 120.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .overflow_hidden()
                    .relative()
                    .child(if let Some(picture) = thumbnail {
                        img(picture)
                            .w_full()
                            .h_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            .into_any_element()
                    } else {
                        div()
                            .p(px(12.))
                            .text_color(color("--text-muted"))
                            .children(message.map(|m| self.content(m)))
                            .into_any_element()
                    })
                    .when(thread.pinned(), |row| {
                        row.child(
                            div()
                                .absolute()
                                .left(px(12.))
                                .bottom(px(8.))
                                .size(px(22.))
                                .rounded_full()
                                .bg(color("--brand-500"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("forum_pin", 16., "--white")),
                        )
                    }),
            )
            .child(
                div()
                    .px(px(12.))
                    .pt(px(12.))
                    .h(px(48.))
                    .flex()
                    .gap(px(4.))
                    .text_size(px(14.))
                    .line_height(px(24.))
                    .text_color(color("--text-muted"))
                    .child(icon("forum_reply_count", 14., "--text-muted"))
                    .child(thread.message_count.to_string()),
            )
            .into_any_element()
    }

    pub(super) fn forum_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(channel) = self.current_channel().cloned() else {
            return div().into_any_element();
        };
        self.ensure_forum(&channel, cx);
        let id = channel.id;
        let view = &self.forums[&id];
        let gallery = view.gallery;
        let (_, card_width) = gallery_columns(
            self.viewport.0 - 375. - if self.show_members { 264. } else { 0. } - 48.,
        );
        let active = active_threads(
            self.current_guild().map_or(&[], |g| &g.threads),
            id,
            view.sort,
            &view.tags,
            view.match_all,
        );
        let groups = if let Some(results) = &view.results {
            vec![(false, results.clone())]
        } else {
            vec![(false, active), (true, view.archived.clone())]
        };
        let mut cards = Vec::new();
        for (older, ids) in groups {
            if older && !ids.is_empty() {
                cards.push(
                    div()
                        .w_full()
                        .pt(px(16.))
                        .pb(px(8.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child("Older Posts")
                        .into_any_element(),
                );
            }
            for post in ids {
                if let Some(thread) = self
                    .current_guild()
                    .and_then(|g| g.threads.iter().find(|t| t.id == post))
                {
                    cards.push(if gallery {
                        self.forum_gallery_card(&channel, thread, card_width, cx)
                    } else {
                        self.forum_card(&channel, thread, cx)
                    });
                }
            }
        }
        let field = view.field.clone();
        let query = view.query.clone();
        let search_status = if view.loading {
            "Searching...".to_owned()
        } else {
            match view.results.as_ref().map(Vec::len) {
                Some(0) => "No matching posts".to_owned(),
                Some(count) => format!(
                    "{count} {} with \"{query}\"",
                    if count == 1 { "post" } else { "posts" }
                ),
                None => "No matching posts".to_owned(),
            }
        };
        let composing = view.new_post.is_some();
        let can_post = self.can_in_channel(&channel, dc_state::permissions::SEND_MESSAGES);
        let guidelines = view.guidelines;
        let tags = view.tags.clone();
        let scroll = view.scroll.clone();
        let list = div()
            .id("forum-scroll")
            .parity("forum-list")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .pl(px(16.))
            .pr(px(32.))
            .pt(px(20.))
            .when(gallery, |row| row.px(px(24.)).pt(px(24.)))
            .pb(px(24.))
            .on_scroll(cx.listener(move |this, _, _, cx| {
                if this
                    .forums
                    .get(&id)
                    .is_some_and(|v| -v.scroll.offset().y >= v.scroll.max_offset().y - px(150.))
                {
                    this.load_forum_page(id);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .parity_with_border("forum-search", 1.)
                    .h(px(50.))
                    .px(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .rounded(px(12.))
                    .when(!query.is_empty(), |row| row.rounded_b(px(0.)))
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(composing, |row| row.hidden())
                    .child(icon("forum_search", 26., "--text-muted"))
                    .child(div().flex_1().min_w(px(0.)).child(field))
                    .child(
                        div()
                            .id("forum-guidelines")
                            .cursor_pointer()
                            .size(px(32.))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("forum_guidelines_btn", 20., "--text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(v) = this.forums.get_mut(&id) {
                                    v.guidelines = !v.guidelines;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("forum-new-post-button")
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(color("--brand-500"))
                            .when(!can_post, |row| row.opacity(0.5))
                            .when(can_post, |row| row.cursor_pointer())
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(14.))
                            .text_color(color("--white"))
                            .child(icon("forum_new_post", 16., "--white"))
                            .child("New Post")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if can_post {
                                    this.open_forum_post(id, window, cx);
                                }
                            })),
                    ),
            )
            .children(self.forum_form(&channel, cx))
            .when(!query.is_empty() && !composing, |list| {
                list.child(
                    div()
                        .parity_with_border("forum-matching-posts", 1.)
                        .h(px(33.))
                        .px(px(12.))
                        .rounded_b(px(12.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-default"))
                        .child(search_status)
                        .child(
                            div()
                                .id("forum-clear-search")
                                .cursor_pointer()
                                .text_color(color("--text-brand"))
                                .font_weight(FontWeight(600.))
                                .child("Clear")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(view) = this.forums.get_mut(&id) {
                                        view.query.clear();
                                        view.field.update(cx, |field, _| field.set_text(""));
                                    }
                                    this.reload_forum(id);
                                    cx.notify();
                                })),
                        )
                        .child(div().flex_1())
                        .when(!can_post, |row| {
                            row.child(
                                div()
                                    .text_color(color("--text-muted"))
                                    .child("You do not have permission to post in this channel."),
                            )
                        }),
                )
            })
            .when(guidelines, |list| {
                list.child(
                    div()
                        .mt(px(16.))
                        .p(px(16.))
                        .rounded(px(12.))
                        .bg(color("--background-base-low"))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(channel.topic.clone().unwrap_or_default()),
                )
            })
            .child(
                div()
                    .h(px(68.))
                    .pt(px(16.))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("forum-sort")
                            .relative()
                            .child(self.anchor_probe("forum-sort-anchor"))
                            .h(px(32.))
                            .pl(px(12.))
                            .pr(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .rounded_full()
                            .cursor_pointer()
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .gap(px(0.))
                            .text_size(px(14.))
                            .text_color(color("--text-default"))
                            .child(icon("forum_sort", 16., "--text-muted"))
                            .child(div().ml(px(8.)).mr(px(4.)).child("Sort & View"))
                            .child(icon("chevron_down", 20., "--text-muted"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(v) = this.forums.get_mut(&id) {
                                    v.menu = if v.menu == Some("sort") {
                                        None
                                    } else {
                                        Some("sort")
                                    };
                                }
                                cx.notify();
                            })),
                    )
                    .when(!channel.available_tags.is_empty(), |row| {
                        row.child(
                            div()
                                .mx(px(8.))
                                .mt(px(4.))
                                .h(px(24.))
                                .w(px(1.))
                                .flex_none()
                                .bg(color("--border-subtle")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_wrap()
                                .h(px(32.))
                                .gap(px(8.))
                                .overflow_hidden()
                                .children(channel.available_tags.iter().map(|tag| {
                                    let tag_id = tag.id;
                                    div()
                                        .id(SharedString::from(format!("forum-tag-{tag_id}")))
                                        .flex_none()
                                        .h(px(32.))
                                        .px(px(12.))
                                        .rounded_full()
                                        .cursor_pointer()
                                        .bg(color(if tags.contains(&tag_id) {
                                            "--brand-500"
                                        } else {
                                            "--background-mod-subtle"
                                        }))
                                        .text_size(px(14.))
                                        .line_height(px(32.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(tag.name.clone())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(v) = this.forums.get_mut(&id)
                                                && !v.tags.remove(&tag_id)
                                            {
                                                v.tags.insert(tag_id);
                                            }
                                            this.reload_forum(id);
                                            cx.notify();
                                        }))
                                })),
                        )
                        .child(
                            div()
                                .id("forum-all-tags")
                                .relative()
                                .child(self.anchor_probe("forum-tags-anchor"))
                                .flex_none()
                                .h(px(32.))
                                .px(px(12.))
                                .rounded_full()
                                .bg(color("--background-mod-subtle"))
                                .cursor_pointer()
                                .text_size(px(14.))
                                .text_color(color("--text-default"))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(if tags.is_empty() {
                                    "All".to_owned()
                                } else {
                                    tags.len().to_string()
                                })
                                .child(icon("chevron_down", 20., "--text-muted"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(v) = this.forums.get_mut(&id) {
                                        v.menu = if v.menu == Some("tags") {
                                            None
                                        } else {
                                            Some("tags")
                                        };
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .when(gallery, |row| row.flex().flex_wrap().gap(px(16.)))
                    .children(cards),
            );
        let content = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(1.))
            .bg(color("--background-base-lower"))
            .child(self.header(cx))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(list)
                    .when(self.show_members, |row| {
                        row.child(
                            div()
                                .w(px(1.))
                                .flex_none()
                                .h_full()
                                .bg(color("--background-mod-subtle")),
                        )
                        .child(self.member_panel(cx))
                    }),
            );
        content.into_any_element()
    }

    pub(super) fn forum_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.current_channel().filter(|c| c.is_forum())?;
        let id = channel.id;
        let view = self.forums.get(&id)?;
        let menu = view.menu?;
        let sort = view.sort;
        let gallery = view.gallery;
        let default_sort = channel.default_sort_order.unwrap_or(0);
        let default_gallery = channel.is_media() || channel.default_forum_layout == 2;
        let anchor = self
            .tip_anchors
            .borrow()
            .get(if menu == "sort" {
                "forum-sort-anchor"
            } else {
                "forum-tags-anchor"
            })
            .copied()?;
        // The sort probe is inside its 1px border; popouts align to the button's outer box.
        let top = f32::from(anchor.bottom()) + if menu == "sort" { 9. } else { 8. };
        let left = if menu == "sort" {
            f32::from(anchor.left()) - 1.
        } else {
            (f32::from(anchor.right()) - 370.).max(0.)
        };
        let all = view.match_all;
        let close = div()
            .id("forum-menu-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(v) = this.forums.get_mut(&id) {
                    v.menu = None;
                }
                cx.notify();
            }));
        let mut layer = div().absolute().top_0().left_0().size_full().child(close);
        if menu == "sort" {
            let entries = [
                ("Sort by", None),
                ("Recently Active", Some(("sort", 0))),
                ("Date Posted", Some(("sort", 1))),
                ("View as", None),
                ("List", Some(("layout", 0))),
                ("Gallery", Some(("layout", 1))),
                ("Tag Matching", None),
                ("Match Some", Some(("tags", 0))),
                ("Match All", Some(("tags", 1))),
                ("Reset to default", Some(("reset", 0))),
            ];
            layer =
                layer.child(
                    div()
                        .parity_with_border("forum-sort-popout", 1.)
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(180.))
                        .p(px(8.))
                        .occlude()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-high"))
                        .children(entries.into_iter().enumerate().map(
                            |(index, (label, action))| {
                                let selected = action.is_some_and(|(kind, value)| {
                                    if kind == "sort" {
                                        sort == value
                                    } else if kind == "layout" {
                                        gallery == (value == 1)
                                    } else if kind == "tags" {
                                        all == (value == 1)
                                    } else {
                                        false
                                    }
                                });
                                div()
                                    .id(SharedString::from(format!("forum-menu-{index}")))
                                    .relative()
                                    .h(px(if index == 9 { 34. } else { 36. }))
                                    .px(px(8.))
                                    .when(matches!(index, 3 | 6 | 9), |row| {
                                        row.mt(px(17.)).child(
                                            div()
                                                .absolute()
                                                .top(px(-9.))
                                                .left_0()
                                                .right_0()
                                                .h(px(1.))
                                                .bg(color("--border-subtle")),
                                        )
                                    })
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(14.))
                                    .line_height(px(if action.is_some() { 18. } else { 20. }))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color(if action.is_some() {
                                        "--text-strong"
                                    } else {
                                        "--text-muted"
                                    }))
                                    .when(action.is_some(), |row| {
                                        row.cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                    })
                                    .child(div().flex_1().child(label))
                                    .when(action.is_some() && index != 9, |row| {
                                        row.child(super::settings::radio_dot(selected))
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some((kind, value)) = action {
                                            if let Some(v) = this.forums.get_mut(&id) {
                                                if kind == "sort" {
                                                    v.sort = value
                                                } else if kind == "layout" {
                                                    v.gallery = value == 1
                                                } else if kind == "reset" {
                                                    v.sort = default_sort;
                                                    v.gallery = default_gallery;
                                                    v.match_all = false;
                                                } else {
                                                    v.match_all = value == 1
                                                }
                                                v.menu = None;
                                            }
                                            this.reload_forum(id);
                                            cx.notify();
                                        }
                                    }))
                            },
                        )),
                );
        }
        if menu == "tags" {
            layer = layer.child(
                div()
                    .parity_with_border("forum-tags-popout", 1.)
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(370.))
                    .p(px(16.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .occlude()
                    .child(
                        div()
                            .h(px(28.))
                            .pl(px(8.))
                            .text_size(px(16.))
                            .line_height(px(28.))
                            .text_color(color("--text-muted"))
                            .child(format!("Select Tags  {}", view.tags.len())),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .mb(px(16.))
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .children(
                                channel
                                    .available_tags
                                    .iter()
                                    .filter(|tag| {
                                        !tag.moderated
                                            || self.can_in_channel(
                                                channel,
                                                dc_state::permissions::MANAGE_THREADS,
                                            )
                                    })
                                    .map(|tag| {
                                        let tag_id = tag.id;
                                        div()
                                            .id(SharedString::from(format!(
                                                "forum-popout-tag-{tag_id}"
                                            )))
                                            .h(px(32.))
                                            .px(px(12.))
                                            .rounded_full()
                                            .cursor_pointer()
                                            .bg(color(if view.tags.contains(&tag_id) {
                                                "--brand-500"
                                            } else {
                                                "--background-mod-subtle"
                                            }))
                                            .text_size(px(14.))
                                            .line_height(px(32.))
                                            .text_color(color("--text-default"))
                                            .child(tag.name.clone())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(v) = this.forums.get_mut(&id)
                                                    && !v.tags.remove(&tag_id)
                                                {
                                                    v.tags.insert(tag_id);
                                                }
                                                this.reload_forum(id);
                                                cx.notify();
                                            }))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("forum-clear-tags")
                            .pt(px(8.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .cursor_pointer()
                            .text_size(px(16.))
                            .text_color(color("--text-link"))
                            .child("Clear all")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(v) = this.forums.get_mut(&id) {
                                    v.tags.clear();
                                    v.menu = None;
                                }
                                this.reload_forum(id);
                                cx.notify();
                            })),
                    ),
            );
        }
        Some(gpui::deferred(layer).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gallery_columns_and_card_heights_follow_the_bundle() {
        assert_eq!(gallery_columns(592.), (1, 592.));
        assert_eq!(gallery_columns(856.), (2, 420.));
        assert_eq!(gallery_columns(300.), (1, 320.));
        assert_eq!(gallery_height(592., false), 498.);
        assert_eq!(gallery_height(592., true), 439.);
    }
    #[test]
    fn a_new_post_sends_its_first_message_in_the_thread_request() {
        let body = post_body("A title", "A body", None, &BTreeSet::from([7]));
        assert_eq!(
            body,
            json!({"name":"A title","auto_archive_duration":4320,"applied_tags":["7"],"message":{"content":"A body","sticker_ids":[]}})
        );
        assert_eq!(
            post_body("Title", "Body", Some(1440), &BTreeSet::new())["auto_archive_duration"],
            1440
        );
        assert!(body.get("type").is_none());
    }
    #[test]
    fn archive_and_name_search_use_distinct_queries() {
        let tags = BTreeSet::from([4, 7]);
        assert_eq!(
            search_route(2, "", &tags, true, 25),
            "/channels/2/threads/search?archived=true&sort_by=last_message_time&sort_order=desc&limit=25&offset=25&tag_setting=match_all&tag=4,7"
        );
        assert_eq!(
            search_route(2, "a & b", &BTreeSet::new(), false, 0),
            "/channels/2/threads/search?name=a%20%26%20b&tag_setting=match_some"
        );
    }
    #[test]
    fn active_posts_pin_first_then_sort_and_match_tags() {
        let a = Thread {
            id: 10,
            parent_id: 2,
            last_message_id: Some(100),
            applied_tags: vec![4],
            ..Thread::default()
        };
        let b = Thread {
            id: 11,
            parent_id: 2,
            last_message_id: Some(50),
            applied_tags: vec![4, 7],
            ..Thread::default()
        };
        let pinned = Thread {
            id: 9,
            parent_id: 2,
            flags: 2,
            ..Thread::default()
        };
        let archived = Thread {
            id: 12,
            parent_id: 2,
            archived: true,
            ..Thread::default()
        };
        let other = Thread {
            id: 13,
            parent_id: 3,
            ..Thread::default()
        };
        let list = [a, b, pinned, archived, other];
        assert_eq!(
            active_threads(&list, 2, 0, &BTreeSet::new(), false),
            [9, 10, 11]
        );
        assert_eq!(
            active_threads(&list, 2, 1, &BTreeSet::new(), false),
            [9, 11, 10]
        );
        assert_eq!(
            active_threads(&list, 2, 0, &BTreeSet::from([4, 7]), true),
            [11]
        );
        assert_eq!(
            active_threads(&list, 2, 0, &BTreeSet::from([4, 7]), false),
            [10, 11]
        );
    }
    #[test]
    fn a_failed_page_stops_scroll_loading_until_retried() {
        let now = Instant::now();
        assert!(can_retry(false, None, now));
        assert!(!can_retry(true, None, now));
        let report = json!({"ok":false,"status":429,"body":{"retry_after":2.5}});
        let at = retry_deadline(&report, now).unwrap();
        assert!(!can_retry(true, Some(at), now));
        assert!(can_retry(true, Some(at), now + Duration::from_secs(3)));
        assert!(
            retry_deadline(
                &json!({"ok":false,"status":403,"body":{"retry_after":2.5}}),
                now
            )
            .is_none()
        );
        assert!(retry_deadline(&json!({"ok":false,"status":429,"body":{}}), now).is_none());
    }
    #[test]
    fn create_failures_use_the_official_texts() {
        let text = |status, body: Value| {
            create_failure_text(&json!({"ok":false,"status":status,"body":body}))
        };
        assert!(
            text(400, json!({"code":160006}))
                .starts_with("This server has reached the maximum number of active posts")
        );
        assert_eq!(text(429, json!({})), "You are being rate limited.");
        assert_eq!(
            text(
                400,
                json!({"code":50035,"errors":{"name":{"_errors":[{"message":"Too long"}]}}})
            ),
            "Too long"
        );
        assert_eq!(text(500, json!({})), "An error occurred, please try again.");
        // A 2xx that carries no thread is reported the same way.
        assert_eq!(
            create_failure_text(&json!({"ok":true,"status":200,"body":{}})),
            "An error occurred, please try again."
        );
    }
    #[test]
    fn a_page_cannot_inject_another_forums_posts() {
        let body = json!({"threads":[{"id":"10","parent_id":"2","type":11},{"id":"11","parent_id":"3","type":11}]});
        assert_eq!(
            page_threads(&body, 2)
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            [10]
        );
    }
}
