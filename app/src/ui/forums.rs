//! Forum browsing: modules 473503 (preview batches), 7909 (archive/search), 207777 (active sorting),
//! 422844 (local filters), and the list geometry in channel/pt-forum-bugs-darker-cozy.
use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Channel, Id, Message, Thread};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ScrollHandle, SharedString, Subscription,
    Task, Window, div, img, prelude::*, px, svg,
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
    /// The forum's ack when it was first shown: `ackMessageIdAtChannelSelect` (module 218461), the line past which
    /// a post counts as new.
    ack_at_select: Id,
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
    // Module 971389, forum create; 268761 supplies the three-day fallback. A leading `@silent` (677413) leaves the
    // content and sets `flags` to SUPPRESS_NOTIFICATIONS, which is otherwise left out.
    let (silent, content) = dc_state::split_silent_prefix(content);
    let mut body = json!({"name":name, "auto_archive_duration":archive.unwrap_or(4320),
        "applied_tags":tags.iter().map(Id::to_string).collect::<Vec<_>>(),
        "message":{"content":content,"sticker_ids":[]}});
    if silent {
        body["message"]["flags"] = json!(dc_state::SUPPRESS_NOTIFICATIONS_FLAG);
    }
    body
}

/// The chevron of the Sort & View and tags buttons: it points up while their popout is open.
fn chevron(open: bool) -> gpui::Svg {
    // Chevron, icon and label of both buttons are the captured pixels (171,172,178) = `--interactive-icon-default`.
    let chevron = icon("chevron_down", 20., "--interactive-icon-default");
    if open {
        chevron.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5)))
    } else {
        chevron
    }
}

/// The byte ranges of `name` that match `query`, ignoring case (the list's search highlight).
fn title_matches(name: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let (haystack, needle) = (name.to_lowercase(), query.trim().to_lowercase());
    // Lower-casing can change byte lengths; offsets are only trusted when it did not.
    if needle.is_empty() || haystack.len() != name.len() {
        return Vec::new();
    }
    haystack
        .match_indices(&needle)
        .map(|(start, found)| start..start + found.len())
        .collect()
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

/// The reaction a post card shows: the most reacted emoji of the first message, the first of equals (`lQ`, module
/// 435470).
fn top_reaction(message: &Message) -> Option<&dc_state::Reaction> {
    message.reactions.iter().rev().max_by_key(|r| r.count)
}

/// The key a reaction's picture is stored under (`emoji:` for unicode, as a message's reactions use).
fn reaction_image_key(reaction: &dc_state::Reaction) -> String {
    match reaction.emoji_id {
        Some(id) => format!("custom:{id}"),
        None => format!("emoji:{}", reaction.name),
    }
}

/// Whether the post sidebar is docked at this window width: it floats over the chat when its maximum width,
/// `width - 240 - 528 + 375` (`renderThreadSidebar`, module 289287), is at most its 450px minimum (module 743898).
fn post_sidebar_fits(window_width: f32) -> bool {
    window_width - 393. > 450.
}

/// The post sidebar's width: the saved width or `(window.innerWidth - 240) / 2` (module 964404 `postSidebarWidth`),
/// clamped between 450 and `width - 393` (module 812771); 520 at 1280, of which 8px are the gap before the panel.
fn post_sidebar_width(window_width: f32) -> f32 {
    ((window_width - 240.) / 2.).clamp(450., (window_width - 393.).max(450.))
}

/// A tag chip's outer width: the label plus `.pill_snX-u4`'s 1px border and 12px padding on both sides.
/// Chromium's inline text box is the text's width rounded UP to a `LayoutUnit` (1/64 px): the capture's chips are
/// 101.09375, 125.390625 and 86.03125 wide for labels our shaper measures 75.082, 99.386 and 60.018. gpui's own text
/// element rounds up to a whole pixel (`TextLayout::layout`'s `.ceil()`), which is why the chip is given this width.
fn tag_chip_width(label_width: f32) -> f32 {
    layout_unit(label_width) + 26.
}

/// The row key of "Open threads in split view" (`dc_proto::settings` `use_thread_sidebar`).
const THREADS_SPLIT_VIEW: &str = "chat_threads_split_view";

/// Whether an open post shows beside its forum's list: only with split view on and a docked sidebar.
fn post_beside_list(split_view: bool, window_width: f32) -> bool {
    split_view && post_sidebar_fits(window_width)
}

/// `divider_BkGrJJ` between the sort button and the tags: 0.797 px in the capture (51/64).
const DIVIDER_WIDTH: f32 = 0.796875;

/// A text width as Chromium's inline box has it: rounded up to 1/64 px.
fn layout_unit(width: f32) -> f32 {
    (width * 64.).ceil() / 64.
}

/// Chromium lays the chips out at fractional x (the list starts at `start`, chips 8px apart, a chip that does not fit
/// wraps to the start) and paints each from the pixel its left edge rounds to up to the pixel its right edge rounds
/// to. gpui rounds an authored width before layout, so the chips take those painted widths: with the capture's
/// 564.125 start, 101.094, 125.391 and 86.031 become 101, 126 and 86 (`pt-forum-bugs-current`).
fn snapped_chip_widths(start: f32, widths: &[f32], limit: f32) -> Vec<f32> {
    // Midpoints round toward zero in gpui and away from it in Chromium; none of these edges sits on one.
    let round = |value: f32| value.round();
    let mut x = start;
    widths
        .iter()
        .map(|width| {
            if x > start && x + width > start + limit {
                x = start;
            }
            let (left, right) = (x, x + width);
            x = right + 8.;
            round(right) - round(left)
        })
        .collect()
}

/// How many chips of the tag list fit on its first row (`.tagList` is 32px high and clips the wrapped rows; chips are
/// 8px apart) and where the last of them ends. The "All" button follows that end by 8px.
fn first_tag_row(widths: &[f32], limit: f32) -> (usize, f32) {
    let (mut shown, mut end) = (0, 0.);
    for width in widths {
        let next = if shown == 0 {
            // A first chip wider than the list is clipped to it (`Low Prio…` in `pt-forum-post-open`).
            width.min(limit.max(0.))
        } else {
            end + 8. + *width
        };
        if shown > 0 && next > limit {
            break;
        }
        end = next;
        shown += 1;
    }
    (shown, end)
}

/// The count inside "(N New)" (`unreadCount` of `k6`, module 435470): "1+" while it is unknown or zero, else the
/// count capped by the post's messages, and "25+" from 25.
fn unread_count_text(count: Option<u64>, messages: u64) -> String {
    match count {
        Some(n) if n > 0 => {
            let n = n.min(messages);
            if n >= 25 {
                "25+".to_owned()
            } else {
                n.to_string()
            }
        }
        _ => "1+".to_owned(),
    }
}

/// `isNewForumThread` (module 218461): the post was never acked, is past the forum's ack at selection, and was
/// created after the account joined the server. No ack at selection, or an unknown join, means nothing is new.
fn is_new_post(post: Id, acked: Id, ack_at_select: Id, joined: Id) -> bool {
    acked == 0 && ack_at_select != 0 && post > ack_at_select && joined != 0 && post > joined
}

/// The width a label takes at a size and weight, as the text system shapes it.
fn measure_label(window: &mut Window, text: &str, size: f32, weight: f32) -> f32 {
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

impl Root {
    /// The forum on screen: the open forum channel, or the forum of the open post (the list stays beside the post).
    pub(super) fn forum_id(&self) -> Option<Id> {
        let channel = self.channel?;
        let guild = self.current_guild()?;
        if guild
            .channels
            .iter()
            .any(|c| c.id == channel && c.is_forum())
        {
            return Some(channel);
        }
        let parent = guild.threads.iter().find(|t| t.id == channel)?.parent_id;
        guild
            .channels
            .iter()
            .any(|c| c.id == parent && c.is_forum())
            .then_some(parent)
    }

    pub(super) fn forum_channel(&self, id: Id) -> Option<&Channel> {
        self.current_guild()?.channels.iter().find(|c| c.id == id)
    }

    /// The forum whose post is open beside its list.
    pub(super) fn open_post_forum(&self) -> Option<Id> {
        self.forum_id().filter(|forum| self.channel != Some(*forum))
    }

    pub(super) fn forum_menu_is_open(&self) -> bool {
        self.forum_id()
            .and_then(|id| self.forums.get(&id))
            .is_some_and(|view| view.menu.is_some())
    }

    pub(super) fn close_forum_menu(&mut self) -> bool {
        self.forum_id()
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
        if !self.forum_channel(id).is_some_and(|c| {
            !self.screening_pending()
                && self.can_in_channel(c, dc_state::permissions::SEND_MESSAGES)
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
        let Some(channel) = self.forum_channel(id).cloned() else {
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
                                        .on_mouse_up(
                                            gpui::MouseButton::Right,
                                            cx.listener(
                                                move |this, event: &gpui::MouseUpEvent, _, cx| {
                                                    let _ = this.open_forum_tag_menu(
                                                        tag_id,
                                                        event.position,
                                                        cx,
                                                    );
                                                },
                                            ),
                                        )
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
                        .text_color(color("--text-feedback-critical"))
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
        let ack_at_select = self.stores.read_states.acked(id, 0);
        // `forum-fixture-empty` shows "Search or create a post..." to a member who may post, `pt-forum-bugs` plain
        // "Search" to one who may not.
        let can_post = !self.screening_pending()
            && self.can_in_channel(channel, dc_state::permissions::SEND_MESSAGES);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.forums.entry(id) {
            let field = cx.new(|cx| {
                let mut style = editor_style();
                style.size = 20.;
                style.line_height = 28.;
                style.font.weight = FontWeight(600.);
                let mut editor = Editor::new(style, cx);
                editor.set_reports_all_edits(true);
                editor.set_placeholder(
                    if can_post {
                        "Search or create a post..."
                    } else {
                        "Search"
                    }
                    .to_owned(),
                );
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
                ack_at_select,
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
        // `REQUEST_FORUM_UNREADS` (module 435470, up to 180 posts): the unread posts whose count is not known yet,
        // each with the message its read state is acked to.
        let unknown: Vec<(Id, Id)> = self
            .current_guild()
            .into_iter()
            .flat_map(|g| &g.threads)
            .filter(|t| t.parent_id == id && !t.archived)
            .map(|t| t.id)
            .filter(|post| {
                self.stores.is_unread(*post)
                    && self
                        .stores
                        .account_events
                        .forum_unread_count(*post)
                        .is_none()
            })
            .map(|post| (post, self.stores.read_states.acked(post, 0)))
            .collect();
        let mut unknown = unknown;
        unknown.retain(|(post, _)| self.requested.insert(format!("forum-unreads:{post}")));
        unknown.truncate(180);
        if let Some(guild) = self.guild
            && !unknown.is_empty()
        {
            let _ = self.requests.send(Request::ForumUnreads {
                guild,
                channel: id,
                threads: unknown,
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
            if let Some(reaction) = top_reaction(&message) {
                let url = match reaction.emoji_id {
                    Some(id) => Some(format!(
                        "https://cdn.discordapp.com/emojis/{id}.webp?size=96"
                    )),
                    None => dc_tokens::emoji_file(&reaction.name)
                        .map(|file| format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN)),
                };
                images.extend(url.map(|url| (reaction_image_key(reaction), url)));
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

    /// A post's first message as one line of text with the inline styles it keeps (`formatInline` in module 745431's
    /// `e4`: bold, italic, underline and strikethrough; links and mentions stay plain).
    fn post_preview_text(
        &self,
        content: &str,
    ) -> (String, Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>) {
        let mut text = String::new();
        let mut runs = Vec::new();
        for span in dc_markdown::inline(content) {
            let start = text.len();
            text.push_str(&span.text.replace(['\r', '\n'], " "));
            let style = span.style;
            let mut run = gpui::HighlightStyle::default();
            if style.bold {
                run.font_weight = Some(FontWeight(700.));
            }
            if style.italic {
                run.font_style = Some(gpui::FontStyle::Italic);
            }
            if style.strike {
                run.strikethrough = Some(gpui::StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                });
            }
            if style.underline {
                run.underline = Some(gpui::UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                });
            }
            if run != gpui::HighlightStyle::default() && text.len() > start {
                runs.push((start..text.len(), run));
            }
        }
        (text, runs)
    }

    fn forum_card(&self, channel: &Channel, thread: &Thread, cx: &mut Context<Self>) -> AnyElement {
        let view = &self.forums[&channel.id];
        let message = view.posts.get(&thread.id);
        let unread = self.stores.is_unread(thread.id);
        // `isNewForumThread` (module 218461): never acked, created after the forum's ack at selection and after the
        // account joined the server.
        let joined = self
            .current_guild()
            .map_or(0, dc_state::Guild::joined_snowflake);
        let is_new = !thread.archived
            && is_new_post(
                thread.id,
                self.stores.read_states.acked(thread.id, 0),
                view.ack_at_select,
                joined,
            );
        let open = self.channel == Some(thread.id);
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
        let first_hidden = message
            .is_some_and(|m| self.is_blocked(m.author.id) || self.stores.is_ignored(m.author.id));
        let first_att = (!first_hidden)
            .then(|| {
                message.and_then(|m| {
                    m.attachments.iter().find(|a| {
                        a.is_image()
                            || a.content_type
                                .as_deref()
                                .is_some_and(|t| t.starts_with("video/"))
                    })
                })
            })
            .flatten();
        let thumbnail_obscured = first_att.is_some_and(|a| a.filename.starts_with("SPOILER_"));
        let thumbnail_video = first_att.is_some_and(|a| {
            a.content_type
                .as_deref()
                .is_some_and(|t| t.starts_with("video/"))
        });
        let thumbnail = (!first_hidden)
            .then(|| {
                message
                    .and_then(|m| m.attachments.iter().find(|a| a.is_image()))
                    .and_then(|a| self.images.get(&format!("forum-thumbnail:{}", a.id)))
                    .cloned()
            })
            .flatten();
        // The footer's reaction button (`.reaction_UNkajC.updateReactionButton`): the most reacted emoji of the first
        // message with its count, 28px high, read only here.
        let chip = message.and_then(top_reaction).map(|reaction| {
            let picture = self.images.get(&reaction_image_key(reaction)).cloned();
            div()
                .h(px(28.))
                .flex_none()
                .mr(px(4.))
                .px(px(6.))
                .rounded(px(8.))
                .border_1()
                .border_color(gpui::transparent_black())
                .bg(color("--reaction-background-default"))
                .hover(|s| {
                    s.bg(color("--reaction-background-hover"))
                        .border_color(color("--reaction-border-hover"))
                })
                .flex()
                .items_center()
                .child(
                    div()
                        .size(px(20.))
                        .children(picture.map(|picture| img(picture).size(px(20.)))),
                )
                .child(
                    div()
                        .ml(px(6.))
                        .min_w(px(12.))
                        .flex()
                        .justify_center()
                        .text_size(px(14.))
                        .line_height(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-subtle"))
                        .child(reaction.count.to_string()),
                )
        });
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
            .rounded(px(12.))
            // `.container_SOY5H6` (5aa4942e5de32d82): hover and the open post take the raised surface and the strong
            // border; the open post one step higher.
            .when(open, |card| {
                card.border_color(color("--border-strong"))
                    .bg(color("--background-surface-higher"))
            })
            .when(!open, |card| {
                card.border_color(color("--border-subtle"))
                    .bg(color("--background-base-low"))
                    .hover(|s| {
                        s.bg(color("--background-surface-high"))
                            .border_color(color("--border-strong"))
                    })
            })
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
                    .when(is_new || thread.pinned() || !tags.is_empty(), |row| {
                        row.child(
                            div()
                                .h(px(24.))
                                .mb(px(8.))
                                .flex()
                                .gap(px(8.))
                                .items_center()
                                // The NEW badge leads the tags row (`Ay` in module 52933).
                                .when(is_new, |row| {
                                    row.child(
                                        div()
                                            .h(px(16.))
                                            .px(px(8.))
                                            .rounded(px(8.))
                                            .bg(color("--brand-500"))
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child("NEW"),
                                    )
                                })
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
                                // `.pill_snX-u4.small`: 24px, 1px subtle border, 0 8px, `body-xs/semibold`.
                                .children(tags.iter().map(|tag| {
                                    let tag_id = tag.id;
                                    div()
                                        .id(SharedString::from(format!(
                                            "forum-post-{id}-tag-{tag_id}"
                                        )))
                                        // A tag's menu takes the press in developer mode; otherwise the post's does.
                                        .on_mouse_up(
                                            gpui::MouseButton::Right,
                                            cx.listener(
                                                move |this, event: &gpui::MouseUpEvent, _, cx| {
                                                    if this.open_forum_tag_menu(
                                                        tag_id,
                                                        event.position,
                                                        cx,
                                                    ) {
                                                        cx.stop_propagation();
                                                    }
                                                },
                                            ),
                                        )
                                        .rounded_full()
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .px(px(8.))
                                        .h(px(24.))
                                        .flex_none()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .flex()
                                        .items_center()
                                        .font_weight(FontWeight(600.))
                                        .bg(color("--control-secondary-background-default"))
                                        .text_color(color("--control-secondary-text-default"))
                                        .child(tag.name.clone())
                                })),
                        )
                    })
                    // `.header_SOY5H6` pads 6px under `.headerText` (8px end margin, two lines at most).
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
                            .mr(px(8.))
                            .max_h(px(48.))
                            .overflow_hidden()
                            .line_clamp(2)
                            .child(
                                gpui::StyledText::new(thread.name.clone()).with_highlights(
                                    title_matches(&thread.name, &view.query).into_iter().map(
                                        |range| {
                                            (
                                                range,
                                                gpui::HighlightStyle {
                                                    // `.highlight_*`: search matches, rgba(253, 184, 51, 0.3).
                                                    background_color: Some(
                                                        gpui::rgba(0xfdb8334d).into(),
                                                    ),
                                                    ..Default::default()
                                                },
                                            )
                                        },
                                    ),
                                ),
                            ),
                    )
                    // `.message_SOY5H6`: the author (`body-sm/semibold`, the role colour over the muted link
                    // colour, strong once the post has unread messages), ":", then the content at 4px,
                    // `font-weight: medium`, on one line.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(18.))
                            .overflow_hidden()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .child(match message {
                                Some(m) => {
                                    // Module 173404: a first message by someone blocked or ignored is not shown; the
                                    // line says so instead (`body-sm/medium`, `--text-muted`, `blockedMessage_SOY5H6`
                                    // italic).
                                    let hidden = if self.is_blocked(m.author.id) {
                                        Some(dc_intl::t("Lkp2fB"))
                                    } else if self.stores.is_ignored(m.author.id) {
                                        Some(dc_intl::t("yWK7ZM"))
                                    } else {
                                        None
                                    };
                                    if let Some(hidden) = hidden {
                                        div()
                                            .w_full()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .italic()
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-muted"))
                                            .child(hidden.to_owned())
                                            .into_any_element()
                                    } else {
                                        let name = self.guild_display_name(&m.author);
                                        let author_color = self.guild_author_color(
                                            m.author.id,
                                            if unread {
                                                "--text-strong"
                                            } else {
                                                "--text-muted"
                                            },
                                        );
                                        // A name with a display name style keeps its font.
                                        let name_element = match self
                                            .name_styles(&m.author, self.guild)
                                        {
                                            Some(styles) => super::name_styles::name(
                                                format!("post-preview-{}", m.id),
                                                name,
                                                Some(&styles),
                                                super::name_styles::Display::Font,
                                            )
                                            .into_any_element(),
                                            None => match self.role_gradient_of(&m.author) {
                                                Some(colors) => div()
                                                    .child(super::role_gradient::name(name, colors))
                                                    .into_any_element(),
                                                None => div().child(name).into_any_element(),
                                            },
                                        };
                                        let (text, runs) = self.post_preview_text(&m.content);
                                        div()
                                            .flex()
                                            .items_center()
                                            .w_full()
                                            .min_w(px(0.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_none()
                                                    .font_weight(FontWeight(600.))
                                                    .child(
                                                        div()
                                                            .text_color(author_color)
                                                            .child(name_element),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_color(color("--text-default"))
                                                            .child(":"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .ml(px(4.))
                                                    .h(px(18.))
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_ellipsis()
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color(if unread {
                                                        "--text-default"
                                                    } else {
                                                        "--text-muted"
                                                    }))
                                                    .child(
                                                        gpui::StyledText::new(text)
                                                            .with_highlights(runs),
                                                    ),
                                            )
                                            .into_any_element()
                                    }
                                }
                                None => div()
                                    .w_full()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .italic()
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("mE3KJN"))
                                    .into_any_element(),
                            }),
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
                            .children(chip)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(icon("forum_reply_count", 14., "--text-default"))
                                    .child(
                                        div()
                                            .font_weight(FontWeight(500.))
                                            .child(thread.message_count.to_string()),
                                    )
                                    // `unreadCount` (`k6`, module 435470): "1+" while the number of new messages is
                                    // unknown, else the count capped by the post's messages, "25+" from 25.
                                    .when(unread, |row| {
                                        let count = unread_count_text(
                                            self.stores
                                                .account_events
                                                .forum_unread_count(thread.id),
                                            thread.message_count,
                                        );
                                        row.child(
                                            div()
                                                .ml(px(4.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-brand"))
                                                .child(format!(
                                                    "({})",
                                                    dc_intl::format(
                                                        "z3PEth",
                                                        &[(
                                                            "count",
                                                            dc_intl::Arg::Str(
                                                                std::borrow::Cow::Owned(count)
                                                            ),
                                                        )],
                                                    )
                                                )),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(16.))
                                    .text_color(color("--border-subtle"))
                                    .child("\u{2022}"),
                            )
                            .child({
                                let posted_tip = dc_intl::format(
                                    "13euCd",
                                    &[(
                                        "timestamp",
                                        dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&ago)),
                                    )],
                                );
                                self.with_tip(
                                    format!("forum-posted-{id}"),
                                    super::tooltip::Tip::body(
                                        super::tooltip::Body::Text(posted_tip.into()),
                                        super::tooltip::Side::Top,
                                    ),
                                    div().id(format!("forum-ago-{id}")).child(ago.clone()),
                                    cx,
                                )
                            }),
                    ),
            )
            .children(if thumbnail_obscured {
                Some(
                    div()
                        .id(format!("forum-obscured-{id}"))
                        .aria_label(dc_intl::t("hqwnc2"))
                        .size(px(72.))
                        .rounded(px(8.))
                        .bg(color("--background-base-lowest"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child("SPOILER")
                        .into_any_element(),
                )
            } else if let Some(picture) = thumbnail {
                Some(
                    img(picture)
                        .size(px(72.))
                        .rounded(px(8.))
                        .object_fit(gpui::ObjectFit::Cover)
                        .into_any_element(),
                )
            } else if thumbnail_video {
                Some(
                    div()
                        .id(format!("forum-video-{id}"))
                        .aria_label(dc_intl::t("FlNoSV"))
                        .size(px(72.))
                        .rounded(px(8.))
                        .bg(color("--background-base-lowest"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("FlNoSV"))
                        .into_any_element(),
                )
            } else {
                None
            })
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
        let first_hidden = message
            .is_some_and(|m| self.is_blocked(m.author.id) || self.stores.is_ignored(m.author.id));
        let thumbnail = (!first_hidden)
            .then(|| {
                message
                    .and_then(|m| m.attachments.iter().find(|a| a.is_image()))
                    .and_then(|a| self.images.get(&format!("forum-thumbnail:{}", a.id)))
                    .cloned()
            })
            .flatten();
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
                                // `.author_nwIAlp` body-sm/semibold; `.postCreationTimestamp_x1LvFB` is text-strong.
                                div()
                                    .font_weight(FontWeight(600.))
                                    .text_color(
                                        self.guild_author_color(m.author.id, "--text-strong"),
                                    )
                                    .child(
                                        match self.role_gradient_of(&m.author).filter(|_| {
                                            self.name_styles(&m.author, self.guild).is_none()
                                        }) {
                                            Some(colors) => super::role_gradient::name(
                                                self.guild_display_name(&m.author),
                                                colors,
                                            )
                                            .into_any_element(),
                                            None => super::name_styles::name(
                                                format!("post-author-{}", m.id),
                                                self.guild_display_name(&m.author),
                                                self.name_styles(&m.author, self.guild).as_ref(),
                                                super::name_styles::Display::Font,
                                            )
                                            .into_any_element(),
                                        },
                                    )
                            }))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(400.))
                                    .text_color(color("--text-strong"))
                                    .child(ago),
                            ),
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
                            // `.textContentRead_x1LvFB`: a read post's preview text is muted.
                            .children(message.map(|m| {
                                self.content_tinted(
                                    m,
                                    if self.stores.is_unread(id) {
                                        "--text-default"
                                    } else {
                                        "--text-muted"
                                    },
                                )
                            }))
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
                    .gap(px(6.))
                    .text_size(px(14.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child(icon("forum_reply_count", 14., "--text-muted"))
                    .child(thread.message_count.to_string()),
            )
            .into_any_element()
    }

    pub(super) fn forum_view_in(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(channel) = self.current_channel().cloned() else {
            return div().into_any_element();
        };
        self.forum_column(channel, None, window, cx)
    }

    /// Script step `unread posts`: every post of `forum` gets a message newer than the last ack.
    #[cfg(feature = "capture")]
    pub(crate) fn script_unread_posts(&mut self, forum: Id, cx: &mut Context<Self>) {
        let posts: Vec<Id> = self
            .current_guild()
            .into_iter()
            .flat_map(|g| &g.threads)
            .filter(|t| t.parent_id == forum)
            .map(|t| t.id)
            .collect();
        for post in posts {
            self.stores
                .read_states
                .message_created(post, u64::MAX >> 1, None, false);
        }
        cx.notify();
    }

    /// Whether the post opens beside the list. The post sidebar (`PostSidebar`, module 812771) is not floating
    /// unless its maximum width, `width - 240 - 528 + 375` (`renderThreadSidebar`, module 289287), is at most its
    /// 450px minimum (module 743898); a floating sidebar overlays the chat, which is not built, so the whole chat view
    /// stands in for it. The member list does not count: `getSection` (module 799704) answers `SIDEBAR_CHAT`
    /// whenever a sidebar is open, so the member list is hidden beside an open post.
    pub(super) fn forum_split_fits(&self, window: &Window) -> bool {
        post_beside_list(
            self.threads_split_view(),
            f32::from(window.viewport_size().width),
        )
    }

    /// "Open threads in split view" (`useThreadSidebar`, settings proto `text_and_images.use_thread_sidebar`, on by
    /// default). Off, `openThreadSidebarForViewing` (module 747926 `JA`) closes the sidebar and opens the thread as
    /// its own channel, so a post fills the chat view instead of opening beside its forum's list.
    pub(super) fn threads_split_view(&self) -> bool {
        self.pref_on(THREADS_SPLIT_VIEW, true)
    }

    fn forum_sidebar_width(window: &Window) -> f32 {
        post_sidebar_width(f32::from(window.viewport_size().width))
    }

    /// A forum's list beside its open post (`threadSidebarOpen_z_gUBg`): the 385px list column and the 512px post
    /// panel, 8px apart on the lowest surface. `None` when no post of a forum is open.
    pub(super) fn forum_split_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let forum = self.open_post_forum()?;
        let channel = self.forum_channel(forum)?.clone();
        let sidebar = Self::forum_sidebar_width(window);
        // The list is 384px wide inside its 385px column at 1280 (the layout tree of `pt-forum-post-open`).
        let list_width =
            f32::from(window.viewport_size().width) - self.combined_sidebar_width() - sidebar - 1.;
        let list = self.forum_column(channel, Some(list_width), window, cx);
        let panel = self.forum_post_panel(forum, sidebar - 8., cx);
        Some(
            div()
                .flex()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .child(
                    div()
                        .flex()
                        .w(px(list_width))
                        .flex_none()
                        .h_full()
                        .child(list),
                )
                .child(
                    div()
                        .w(px(1.))
                        .flex_none()
                        .h_full()
                        .bg(color("--background-base-lowest")),
                )
                .child(
                    div()
                        .w(px(8.))
                        .flex_none()
                        .h_full()
                        .bg(color("--background-base-lowest")),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    /// The panel of an open post: its header (title, More, Close), the messages and the composer.
    fn forum_post_panel(&mut self, forum: Id, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let title = self.current_thread().map(|t| t.name).unwrap_or_default();
        let thread = self.channel;
        let guild = self.guild;
        let button = |id: &'static str, name: &'static str| {
            div()
                .id(id)
                .group(id)
                .size(px(32.))
                // `toolbar_ybUK7Z` (pt-forum-post-open): the 32px buttons sit at y 41, one pixel under the 48px
                // bar's centre.
                .mt(px(2.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(
                    svg()
                        .path(format!("icons/{name}.svg"))
                        .size(px(20.))
                        .flex_none()
                        .text_color(color("--text-muted")),
                )
        };
        let header = div()
            .flex_none()
            .h(px(49.))
            .border_b_1()
            .border_color(color("--background-mod-subtle"))
            .bg(color("--background-base-lower"))
            .pl(px(16.))
            // The toolbar ends 9px short of the panel edge (1271 of 1280 in pt-forum-post-open).
            .pr(px(9.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .child(
                button("forum-post-more", "more_horizontal").on_click(cx.listener(
                    move |this, event: &gpui::ClickEvent, _, cx| {
                        if let Some(thread) = thread {
                            this.open_context_menu(
                                super::context_menu::Target::Thread(thread),
                                event.position(),
                                cx,
                            );
                        }
                    },
                )),
            )
            .child(button("forum-post-close", "x_small").on_click(cx.listener(
                move |this, _, _, cx| {
                    if let Some(guild) = guild {
                        this.select(guild, Some(forum));
                    }
                    cx.notify();
                },
            )));
        div()
            .w(px(width))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .child(header)
            .child(self.regions.place(
                super::regions::Kind::Messages,
                super::regions::style_of(super::regions::Kind::Messages),
            ))
            .child(if self.screening_pending() {
                self.screening_composer(cx)
            } else if self.follow_blocker_applies() {
                self.follow_composer(cx)
            } else {
                self.regions
                    .place_composer()
                    .unwrap_or_else(|| self.composer_bar(cx).into_any_element())
            })
            .into_any_element()
    }

    /// `container_Wp3SYH`: the 64px icon over the post title at 32px, 16px in from the list's edges.
    pub(super) fn forum_post_intro(&self) -> AnyElement {
        let title = self.current_thread().map(|t| t.name).unwrap_or_default();
        div()
            .px(px(16.))
            .pt(px(16.))
            // `container_Wp3SYH` is 136px: 16 + 64 icon + 8 + 40 title + 8 (`pt-forum-post-open`).
            .pb(px(8.))
            .flex()
            .flex_col()
            .child(
                div()
                    .size(px(64.))
                    .flex()
                    .items_center()
                    .justify_center()
                    // `icon_Wp3SYH`: 40px in the 64px circle, drawn in the grey of the captured pixels (171,172,178).
                    .child(icon("forum_reply_count", 40., "--interactive-icon-default")),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(32.))
                    .line_height(px(40.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .into_any_element()
    }

    /// `container_HeWz1C`: Follow and Copy Link at the right of the strip under the first message, then the 8px
    /// rule on the lowest surface.
    pub(super) fn forum_post_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let thread = self.current_thread();
        let id = thread.as_ref().map_or(0, |t| t.id);
        let following = thread.as_ref().is_some_and(|t| t.joined);
        let guild = self.guild.unwrap_or_default();
        // The control is hidden while a member is still screened or cannot react (`pt-forum-post-open` has none).
        let can_react = !self.screening_pending()
            && thread.as_ref().is_some_and(|t| {
                self.forum_channel(t.parent_id).is_some_and(|forum| {
                    self.can_in_channel(forum, dc_state::permissions::ADD_REACTIONS)
                })
            });
        // The first message's reactions lead the strip, `floor((width - 280) / 58)` of them (`rC` in module 891522);
        // the add button then follows them without its label. The panel is 512px wide, so four fit.
        let first = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|messages| messages.iter().find(|m| m.id == id))
            .map(|message| {
                let mut message = message.clone();
                message.reactions.truncate(4);
                message
            })
            .filter(|message| !message.reactions.is_empty());
        let has_chips = first.is_some();
        let chips = first.and_then(|message| self.reactions(&message, cx));
        let secondary = |name: &'static str| {
            div()
                .id(name)
                .h(px(32.))
                .border_1()
                .border_color(color("--border-subtle"))
                .rounded(px(8.))
                .bg(color("--control-secondary-background-default"))
                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .h(px(45.))
                    .pl(px(14.))
                    // 12px inside the strip, which ends 16px short of the list's edge (`container_HeWz1C` 494px in a
                    // 510px panel, the scroller's gutter).
                    .pr(px(28.))
                    .flex()
                    .justify_between()
                    .items_end()
                    .child(div().children(chips).when(can_react && !has_chips, |left| {
                        // `.addReactButton_HeWz1C` on `.reactionBtn_UNkajC`: 30px pill, 18px icon, 8px gap.
                        left.child(
                            div()
                                .id("forum-add-reaction")
                                .h(px(30.))
                                .pl(px(9.))
                                .pr(px(10.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .rounded(px(8.))
                                .bg(color("--background-mod-subtle"))
                                .hover(|s| s.bg(color("--background-mod-normal")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(icon("menu_add_reaction", 18., "--text-strong"))
                                .child("React to Post")
                                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                                    this.open_picker(
                                        super::picker::Target::Reaction {
                                            channel: id,
                                            message: id,
                                        },
                                        event.position(),
                                        window,
                                        cx,
                                    );
                                })),
                        )
                    }))
                    .child(div().flex().gap(px(4.)).child(
                        secondary("forum-follow")
                            .px(px(11.))
                            .gap(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(icon(
                                if following { "checkmark_large" } else { "bell" },
                                16.,
                                "--control-secondary-text-default",
                            ))
                            .child(if following { "Following" } else { "Follow" })
                            .on_click(cx.listener(move |this, _, _, _| {
                                // `joinThread` / `leaveThread` (module 7909) with the "Forum Toolbar" location of the
                                // Follow button (module 891522): the gateway's THREAD_MEMBER_UPDATE follows.
                                let (method, route) = if following {
                                    (
                                        reqwest::Method::DELETE,
                                        format!(
                                            "/channels/{id}/thread-members/@me?location=Forum%20Toolbar"
                                        ),
                                    )
                                } else {
                                    (
                                        reqwest::Method::POST,
                                        format!(
                                            "/channels/{id}/thread-members/@me?location=Forum%20Toolbar"
                                        ),
                                    )
                                };
                                let _ = this.requests.send(Request::Rest {
                                    key: None,
                                    method,
                                    route,
                                    body: None,
                                });
                            })),
                    )
                    .child(
                        secondary("forum-copy-link")
                            .w(px(32.))
                            .child(icon("share_link", 16., "--control-secondary-text-default"))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!(
                                    "https://discord.com/channels/{guild}/{id}"
                                )));
                            })),
                    )),
            )
            .child(div().h(px(8.)).bg(color("--background-base-lowest")))
            .into_any_element()
    }

    /// The forum's own header while its post is open (`title_z_gUBg` with `threadSidebarOpen`): the icon and name, the
    /// notification and member buttons and the collapsed 64px search box.
    fn forum_column_header(&self, channel: &Channel) -> impl IntoElement {
        let glyph = |name: &'static str| {
            div()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path(format!("icons/{name}.svg"))
                        .size(px(20.))
                        .flex_none()
                        .text_color(color("--text-muted")),
                )
        };
        let facts = super::channel_icon::Facts {
            locked: false,
            ..self.channel_icon_facts(channel, false)
        };
        div()
            .flex_none()
            .h(px(49.))
            .border_b_1()
            .border_color(color("--background-mod-subtle"))
            .bg(color("--background-base-lower"))
            .pl(px(15.))
            .pr(px(8.))
            .flex()
            .items_center()
            .child(icon(
                super::channel_icon::name(&facts),
                20.,
                "--channels-default",
            ))
            .child(
                div()
                    .ml(px(9.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(channel.name.clone()),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    // Module 187360: slashed while the forum is muted or its level is not All Messages.
                    .child(glyph(if self.channel_bell_slashed() {
                        "bell_muted"
                    } else {
                        "bell"
                    }))
                    .child(glyph("members"))
                    .child(
                        div()
                            // `searchBar_i16Ad0` in `pt-forum-post-open`: 64px at x 687, 12px after the member button,
                            // 8px radius, 1px border.
                            .ml(px(4.))
                            .w(px(64.))
                            .flex_none()
                            .h(px(32.))
                            .rounded(px(8.))
                            .bg(color("--input-background-default"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .pl(px(8.))
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .child(
                                div()
                                    // `.searchBar_jDHkVJ` in a 64px collapsed box: the placeholder ellipsizes to "S…".
                                    .w(px(24.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child("Search"),
                            )
                            .child(icon("search", 16., "--text-muted")),
                    ),
            )
    }

    /// The forum's list column. `split` is its width when a post is open beside it.
    fn forum_column(
        &mut self,
        channel: Channel,
        split: Option<f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let show_members = self.show_members && split.is_none();
        let list_width = split.unwrap_or(
            self.viewport.0
                - self.combined_sidebar_width()
                - if self.show_members { 265. } else { 0. },
        );
        self.ensure_forum(&channel, cx);
        let id = channel.id;
        let view = &self.forums[&id];
        let gallery = view.gallery;
        let (_, card_width) = gallery_columns(list_width - 48.);
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
                        // `archivedDivider_BkGrJJ` is 16px tall inside the 40px `archivedDividerRow_BkGrJJ`.
                        .line_height(px(16.))
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
        // `canCreatePost`: `canChatInGuild` (false while membership screening is pending) and the permission.
        let can_post = !self.screening_pending()
            && self.can_in_channel(&channel, dc_state::permissions::SEND_MESSAGES);
        let guidelines = view.guidelines;
        let tags = view.tags.clone();
        // The tag list (`.tagList_BkGrJJ`) is what is left of the row after the sort button, the divider (16px
        // margins) and the hidden placeholder of the "All" button (62px and an 8px margin).
        let row_width = list_width - 48.;
        // The label is 70.328125 wide in the capture (`sortDropdownText_BkGrJJ`), the divider 0.796875.
        let sort_label = layout_unit(measure_label(window, "Sort & View", 14., 500.));
        let sort_width = 70. + sort_label;
        let tag_left = sort_width + 16. + DIVIDER_WIDTH + 16.;
        let tag_limit = row_width - tag_left - 8. - 62.;
        let tag_widths: Vec<f32> = channel
            .available_tags
            .iter()
            .map(|tag| tag_chip_width(measure_label(window, &tag.name, 14., 600.)))
            .collect();
        let (_, tags_end) = first_tag_row(&tag_widths, tag_limit);
        let chip_pixels = snapped_chip_widths(tag_left, &tag_widths, tag_limit);
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
                    .pl(px(12.))
                    .pr(px(8.))
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
                    .child(div().flex_1().min_w(px(0.)).overflow_hidden().child(field))
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
                            .child(icon("forum_guidelines_btn", 16., "--text-default"))
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
            .when(
                guidelines
                    && channel
                        .topic
                        .as_ref()
                        .is_some_and(|topic| !topic.is_empty()),
                |list| {
                    // `container_hbPaoT` (modules 745431 `nX`, 257701): 12px padding in a 1px border, the header
                    // row, then `descriptionContainer_hbPaoT` with the topic as markdown in `body-sm/normal`,
                    // at most 220px tall.
                    let topic = channel.topic.clone().unwrap_or_default();
                    list.child(
                        div()
                            .mt(px(16.))
                            .p(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .rounded(px(12.))
                            .bg(color("--background-base-low"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .h(px(24.))
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(icon("ob_book_check", 20., "--text-strong"))
                                            .child("Post Guidelines"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child({
                                                let guild_id = self.guild.unwrap_or(1);
                                                div()
                                                    .id("forum-post-perms-trigger")
                                                    .cursor_pointer()
                                                    .px(px(10.))
                                                    .h(px(24.))
                                                    .rounded(px(6.))
                                                    .bg(color("--control-secondary-background-default"))
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("TFGnmk"))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.open_forum_post_perms(guild_id, id, cx);
                                                    }))
                                            })
                                            .child(
                                                div()
                                                    .id("forum-guidelines-close")
                                                    .cursor_pointer()
                                                    .child(icon(
                                                        "x_small",
                                                        16.,
                                                        "--interactive-text-default",
                                                    ))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Some(v) = this.forums.get_mut(&id) {
                                                            v.guidelines = false;
                                                        }
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(12.))
                                    .p(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--background-mod-subtle"))
                                    .overflow_hidden()
                                    .child(div().max_h(px(220.)).overflow_hidden().child(
                                        self.bio(&topic, super::hsla("--text-default"), cx),
                                    )),
                            ),
                    )
                },
            )
            .child(
                div()
                    .h(px(68.))
                    .pt(px(16.))
                    .relative()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("forum-sort")
                            .relative()
                            .child(self.anchor_probe("forum-sort-anchor"))
                            // `sortDropdown_BkGrJJ` sits at y 168.5 beside the tag list at y 168 (`pt-forum-bugs-current`).
                            .mt(px(0.5))
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
                            .text_color(color("--interactive-icon-default"))
                            .child(icon("forum_sort", 16., "--interactive-icon-default"))
                            .child(
                                div()
                                    .ml(px(8.))
                                    .mr(px(4.))
                                    .w(px(sort_label))
                                    .child("Sort & View"),
                            )
                            .child(chevron(view.menu == Some("sort")))
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
                                .w(px(DIVIDER_WIDTH))
                                .flex_none()
                                .bg(color("--border-subtle")),
                        )
                        .child(
                            div()
                                .w(px(tag_limit.max(0.)))
                                .flex_none()
                                .flex()
                                .flex_wrap()
                                .h(px(32.))
                                .gap(px(8.))
                                .overflow_hidden()
                                .children(channel.available_tags.iter().enumerate().map(
                                    |(index, tag)| {
                                        let tag_id = tag.id;
                                        div()
                                            .id(SharedString::from(format!("forum-tag-{tag_id}")))
                                            .flex_none()
                                            // The chip is its measured label plus 2 x (12 padding + 1 border), as
                                            // `pill_snX-u4` is in Chromium (101.094 for a 75.094 label), painted
                                            // between the pixels its fractional edges round to. gpui rounds an
                                            // authored width before layout, so it gets that pixel width.
                                            .w(px(chip_pixels[index]))
                                            .max_w(px(tag_limit.max(0.)))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .h(px(32.))
                                            .px(px(12.))
                                            .rounded_full()
                                            .cursor_pointer()
                                            // `.pill_snX-u4`: 1px border; chosen tags use the reacted colours.
                                            .border_1()
                                            .when(tags.contains(&tag_id), |chip| {
                                                chip.border_color(color("--brand-500"))
                                                    .bg(color(
                                                        "--reaction-background-reacted-default",
                                                    ))
                                                    .text_color(color(
                                                        "--reaction-text-reacted-default",
                                                    ))
                                            })
                                            .when(!tags.contains(&tag_id), |chip| {
                                                chip.border_color(color("--border-subtle"))
                                                    .bg(color(
                                                        "--control-secondary-background-default",
                                                    ))
                                                    .text_color(color(
                                                        "--control-secondary-text-default",
                                                    ))
                                                    .hover(|s| {
                                                        s.bg(color(
                                                            "--control-secondary-background-hover",
                                                        ))
                                                    })
                                            })
                                            .text_size(px(14.))
                                            .line_height(px(30.))
                                            .font_weight(FontWeight(600.))
                                            .child(tag.name.clone())
                                            .on_mouse_up(
                                                gpui::MouseButton::Right,
                                                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                                    let _ = this.open_forum_tag_menu(tag_id, event.position, cx);
                                                }),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(v) = this.forums.get_mut(&id)
                                                    && !v.tags.remove(&tag_id)
                                                {
                                                    v.tags.insert(tag_id);
                                                }
                                                this.reload_forum(id);
                                                cx.notify();
                                            }))
                                    },
                                )),
                        )
                        .child(
                            div()
                                .id("forum-all-tags")
                                // `.tagsButton_BkGrJJ` is absolute, 8px after the last chip of the first row.
                                .absolute()
                                .top(px(16.))
                                .left(px(tag_left + tags_end + 8.))
                                .child(self.anchor_probe("forum-tags-anchor"))
                                .flex_none()
                                .h(px(32.))
                                .w(px(62.))
                                .pl(px(12.))
                                .pr(px(4.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .rounded_full()
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .text_size(px(14.))
                                .text_color(color("--interactive-icon-default"))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(if tags.is_empty() {
                                    "All".to_owned()
                                } else {
                                    tags.len().to_string()
                                })
                                .child(chevron(view.menu == Some("tags")))
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
            .child(if split.is_some() {
                self.forum_column_header(&channel).into_any_element()
            } else {
                self.header(cx).into_any_element()
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    // `scrollerBase_tqapBM auto_tqapBM`: the list keeps a stable 16px gutter, thumb 4px clear of it
                    // (`pt-forum-bugs-current`: x 1003..1010, y 86..215).
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .child(list)
                            .child(super::settings::auto_scrollbar(scroll.clone())),
                    )
                    .when(show_members, |row| {
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
        let channel = self.forum_channel(self.forum_id()?)?;
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
        let top = f32::from(anchor.bottom()) + if menu == "sort" { 9.5 } else { 9. };
        let left = if menu == "sort" {
            f32::from(anchor.left()) - 1.
        } else {
            (f32::from(anchor.right()) - 370.).max(0.)
        };
        let all = view.match_all;
        let mut layer = div().absolute().top_0().left_0().size_full();
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
                layer.child(super::keyboard::outside(
                    div()
                        .id("forum-sort-popout")
                        .parity_with_border("forum-sort-popout", 1.)
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(180.))
                        .px(px(8.))
                        .pt(px(9.))
                        .pb(px(7.))
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
                    super::keyboard::Layer::ForumMenu,
                    cx,
                ));
        }
        if menu == "tags" {
            layer = layer.child(super::keyboard::outside(
                div()
                    .id("forum-tags-popout")
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
                        // `.header_r0937z`: `.headerText` heading-sm/semibold, then `.countContainer` (20px brand disc,
                        // `body-xs/medium` count in white) 8px after it.
                        div()
                            .h(px(28.))
                            .pl(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child("Select Tags"),
                            )
                            .child(
                                div()
                                    .size(px(20.))
                                    .rounded_full()
                                    .bg(color("--brand-500"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(gpui::white())
                                    .child(view.tags.len().to_string()),
                            ),
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
                                        // `.tag_r0937z` on `.pill_snX-u4`: 1px border, `body-sm/semibold`.
                                        .h(px(32.))
                                        .px(px(12.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .rounded_full()
                                        .cursor_pointer()
                                        .bg(color(if view.tags.contains(&tag_id) {
                                            "--brand-500"
                                        } else {
                                            "--background-mod-subtle"
                                        }))
                                        .text_size(px(14.))
                                        .line_height(px(30.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-default"))
                                        .child(tag.name.clone())
                                        .on_mouse_up(
                                            gpui::MouseButton::Right,
                                            cx.listener(
                                                move |this, event: &gpui::MouseUpEvent, _, cx| {
                                                    let _ = this.open_forum_tag_menu(
                                                        tag_id,
                                                        event.position,
                                                        cx,
                                                    );
                                                },
                                            ),
                                        )
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
                        // `.separator_r0937z`: 1px rule inset 4px, then the `textButton` "Clear all" 8px under it.
                        div()
                            .id("forum-clear-tags")
                            .mx(px(4.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .cursor_pointer()
                            .child(
                                div()
                                    .mt(px(8.))
                                    .mx(px(-4.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-link"))
                                    .child("Clear all"),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(v) = this.forums.get_mut(&id) {
                                    v.tags.clear();
                                    v.menu = None;
                                }
                                this.reload_forum(id);
                                cx.notify();
                            })),
                    ),
                super::keyboard::Layer::ForumMenu,
                cx,
            ));
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
    fn a_search_highlights_every_case_insensitive_match_in_a_title() {
        assert_eq!(
            title_matches("Missing baby Physics", "physics"),
            vec![13..20]
        );
        assert_eq!(title_matches("aa aa", "aa"), vec![0..2, 3..5]);
        assert!(title_matches("anything", "  ").is_empty());
    }

    #[test]
    fn the_tag_list_keeps_the_chips_of_its_first_row() {
        // The measured row of pt-forum-bugs-current: three chips fit 348.9px, the All button follows the third.
        let widths = [101.1, 125.4, 86.0, 84.9, 122.5];
        let (shown, end) = first_tag_row(&widths, 348.9);
        assert_eq!(shown, 3);
        assert!((end - 328.5).abs() < 0.01);
        assert_eq!(first_tag_row(&widths, 1000.).0, 5);
        assert_eq!(first_tag_row(&[400.], 100.), (1, 100.));
        // Painted from the pixel the left edge rounds to up to the one the right edge rounds to.
        assert_eq!(
            snapped_chip_widths(173.125, &[101.09375, 125.390625, 86.03125], 348.9),
            [101., 126., 86.]
        );
        // The capture's chip widths (`pt-forum-bugs-colors`) from the labels as measured here.
        assert_eq!(tag_chip_width(75.082), 101.09375);
        assert_eq!(tag_chip_width(99.386), 125.390625);
        assert_eq!(tag_chip_width(60.018), 86.03125);
    }

    #[test]
    fn split_view_off_opens_a_post_as_its_own_channel() {
        assert!(post_beside_list(true, 1280.));
        assert!(!post_beside_list(false, 1280.));
        assert!(!post_beside_list(true, 843.));
        let pages = include_str!("settings/pages.rs");
        assert!(pages.contains(&format!("key: \"{THREADS_SPLIT_VIEW}\"")));
    }

    #[test]
    fn the_post_sidebar_docks_above_843_px_and_takes_half_the_window() {
        assert!(post_sidebar_fits(1280.));
        assert!(post_sidebar_fits(844.));
        assert!(!post_sidebar_fits(843.));
        assert_eq!(post_sidebar_width(1280.), 520.);
        // Never narrower than 450, never wider than the room that leaves the chat its 393px.
        assert_eq!(post_sidebar_width(900.), 450.);
        assert_eq!(post_sidebar_width(1920.), 840.);
        assert_eq!(post_sidebar_width(1000.), 450.);
    }

    #[test]
    fn unread_counts_follow_the_official_text() {
        assert_eq!(unread_count_text(None, 9), "1+");
        assert_eq!(unread_count_text(Some(0), 9), "1+");
        assert_eq!(unread_count_text(Some(3), 9), "3");
        assert_eq!(unread_count_text(Some(30), 90), "25+");
        assert_eq!(unread_count_text(Some(30), 4), "4");
    }
    #[test]
    fn a_post_is_new_past_the_forum_ack_and_the_join() {
        assert!(is_new_post(300, 0, 200, 100));
        assert!(!is_new_post(300, 250, 200, 100), "already acked");
        assert!(!is_new_post(150, 0, 200, 100), "older than the forum's ack");
        assert!(!is_new_post(300, 0, 0, 100), "no ack at selection");
        assert!(!is_new_post(300, 0, 200, 400), "created before joining");
        assert!(!is_new_post(300, 0, 200, 0), "join unknown");
    }
    #[test]
    fn a_post_shows_its_most_reacted_emoji() {
        let reaction = |name: &str, count| dc_state::Reaction {
            name: name.to_owned(),
            emoji_id: None,
            count,
            me: false,
        };
        let mut message = Message::from_value(&json!({"id":"1","channel_id":"2","content":"",
            "timestamp":"2026-01-01T00:00:00+00:00","author":{"id":"3","username":"a"}}))
        .unwrap();
        assert!(top_reaction(&message).is_none());
        message.reactions = vec![reaction("a", 2), reaction("b", 3), reaction("c", 3)];
        assert_eq!(top_reaction(&message).unwrap().name, "b");
    }
    #[test]
    fn a_new_post_sends_its_first_message_in_the_thread_request() {
        let body = post_body("A title", "A body", None, &BTreeSet::from([7]));
        assert_eq!(
            body,
            json!({"name":"A title","auto_archive_duration":4320,"applied_tags":["7"],"message":{"content":"A body","sticker_ids":[]}})
        );
        let silent = post_body("T", "@silent quiet", None, &BTreeSet::new());
        assert_eq!(
            silent["message"],
            json!({"content":"quiet","sticker_ids":[],"flags":4096})
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
