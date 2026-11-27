//! Message Requests (`/message-requests`, module 793706): the "Message Requests" row of the Home sidebar, the page with
//! its Requests and Spam tabs, the rows with Accept DM / Ignore, and the preview panel of an opened request.
//!
//! Confirmed against captures: the header, the tabs and both empty states
//! (`design/golden/windows/message-requests/*`). The rows and the opened request are built from the modules and CSS:
//! the row from 793706 with CSS `messageRequestItem_rRxPBT`, `container_OVKWNO`, `*_42Xsjf` and `*_nMe-a6`; the
//! opened request is `tf` / `B`, the ordinary chat of the request channel (699344 with `chatInputType` SIDEBAR) under
//! a DM header bar, in the resizable chat sidebar of module 812771. That sidebar is the channel browser's preview
//! (`channel-browser-preview/pt*` captures), so the frame follows that port. No capture of a page with a request
//! exists yet: the clean capture client must be signed in again first.
use super::dm::{Tone, small_button};
use super::motion::{self, Transitions as _};
use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, PrivateChannel, User};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ScrollHandle, SharedString, Window, div, img,
    prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// Module 467840: the accept confirmation is for accounts in Great Britain (`c = new Set(["GB"])`) whose NSFW content
/// is not allowed.
pub(super) fn accept_needs_confirmation(country: Option<&str>, nsfw_allowed: bool) -> bool {
    !nsfw_allowed && country == Some("GB")
}

/// Keys of this page's REST answers (`Update::Fetched`).
pub(super) const KEY_PREFIX: &str = "mr:";

/// 4e11cf8e80a816c6 is `e_` (dark themes) and a09cfd7678b20f8c is `eI` (light themes) of module 793706.
const ART_DARK: &str = "https://discordapp.com/assets/4e11cf8e80a816c6.svg";
const ART_LIGHT: &str = "https://discordapp.com/assets/a09cfd7678b20f8c.svg";
const ART_KEY: &str = "message-requests-art";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Requests,
    Spam,
}

/// What a request's accept or ignore is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Busy {
    Accepting,
    Rejecting,
}

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    pub tab: Tab,
    /// `VIEW_MESSAGE_REQUEST` sidebar state: the request shown in the panel.
    pub sidebar: Option<Id>,
    pub hovered: Option<Id>,
    pub busy: HashMap<Id, Busy>,
    pub scroll: ScrollHandle,
    /// Channels whose previews were asked for and have not come back.
    pub loading: HashSet<Id>,
    /// The panel's width once dragged (`messageRequestSidebarWidth`, read from storage at first use).
    pub width: Option<f32>,
    /// Pointer x and panel width when a drag of the resize handle began.
    pub resize_from: Option<(f32, f32)>,
}

/// Module 812771's minimum width.
const PANEL_MIN: f32 = 450.;
/// `--chat-resize-handle-width`.
const RESIZE_HANDLE: f32 = 8.;

/// 812771 with `maxWidth: pageWidth - 650` (`tf` of 793706): the width the panel takes and whether it floats over the
/// list (743898 `P`: `maxWidth <= minWidth`). The stored width is raised to the minimum, then `clamp(w, 450, max)`,
/// which, as lodash's, takes the upper bound first, so a floating panel is always 450.
pub(super) fn panel_layout(page_width: f32, stored: Option<f32>) -> (f32, bool) {
    let max = page_width - 650.;
    let wanted = stored.unwrap_or(PANEL_MIN).max(PANEL_MIN);
    (wanted.min(max).max(PANEL_MIN), max <= PANEL_MIN)
}

/// Which rows of the list draw their top border (`messageRequestItem_rRxPBT:not(.firstItem)`): not the first, and not
/// one whose own box or whose previous row is hovered (`:hover`, `:hover + .messageRequestItem`). The selected row
/// (`selected_nMe-a6`) keeps its border; `siblingSelected` only drops a bottom border these rows do not have.
pub(super) fn row_has_border(index: usize, hovered: Option<usize>) -> bool {
    index > 0 && hovered != Some(index) && hovered != Some(index - 1)
}

/// The resting second line of a row (`tt`): "No Mutual Servers" alone is one 18px line; the icons' row
/// (`mutualGuildsContainer_42Xsjf`) adds 4px above. The hover line rolls by 107% of this.
fn roll_height(has_mutual: bool) -> f32 {
    if has_mutual { 22. } else { 18. }
}

// ---- the calls the official sends (module 966107) ----

/// A REST call that is built here and sent by `Root::mr_send`: method, route and body.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Call {
    pub method: reqwest::Method,
    pub route: String,
    pub body: Option<Value>,
}

/// `RK`: `PUT /channels/<id>/recipients/@me` with `consent_status` ACCEPTED (2, module 961523).
pub(super) fn accept_call(channel: Id) -> Call {
    Call {
        method: reqwest::Method::PUT,
        route: format!("/channels/{channel}/recipients/@me"),
        body: Some(json!({"consent_status": 2})),
    }
}

/// `UK`: `DELETE /channels/<id>/recipients/@me`.
pub(super) fn reject_call(channel: Id) -> Call {
    Call {
        method: reqwest::Method::DELETE,
        route: format!("/channels/{channel}/recipients/@me"),
        body: None,
    }
}

/// `ST` in chunks of 50 (190857): `PUT /channels/recipients/@me/batch-reject` with the channel ids.
pub(super) fn reject_all_calls(channels: &[Id]) -> Vec<Call> {
    channels
        .chunks(50)
        .map(|chunk| Call {
            method: reqwest::Method::PUT,
            route: "/channels/recipients/@me/batch-reject".to_owned(),
            body: Some(json!({
                "channel_ids": chunk.iter().map(ToString::to_string).collect::<Vec<_>>()
            })),
        })
        .collect()
}

/// 978914: up to 25 channels per `GET /users/@me/message-requests/supplemental-data`.
pub(super) fn supplemental_route(channels: &[Id]) -> String {
    let query: Vec<String> = channels
        .iter()
        .map(|channel| format!("channel_ids={channel}"))
        .collect();
    format!(
        "/users/@me/message-requests/supplemental-data?{}",
        query.join("&")
    )
}

/// The age label of a row (module 469881 without the `seconds` unit, as 793706 calls it): `1m ago` under a minute,
/// then minutes, hours and days, `>30d ago` up to a year with `>1y` never reached, and the date after that.
pub(super) fn age_label(now_ms: u64, then_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(then_ms) / 1000;
    match seconds {
        0..60 => "1m ago".to_owned(),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86400 => format!("{}h ago", seconds / 3600),
        86400..2_592_000 => format!("{}d ago", seconds / 86400),
        2_592_000..31_104_000 => ">30d ago".to_owned(),
        _ => chrono::DateTime::from_timestamp_millis(then_ms as i64)
            .map(|time| time.format("%B %-d, %Y").to_string())
            .unwrap_or_default(),
    }
}

/// `e0.o` placeholders for a preview with no text (448368).
fn placeholder(message: &dc_state::Message) -> Option<&'static str> {
    if !message.content.is_empty() {
        return None;
    }
    if message.is_voice_message() {
        Some("Click to see voice message")
    } else if !message.attachments.is_empty() || !message.embeds.is_empty() {
        Some("Click to see attachment")
    } else {
        None
    }
}

/// The motion key of one row's roll.
fn roll_key(channel: Id) -> u64 {
    channel ^ 0x6d72_726f_6c6c_0000
}

impl Root {
    /// The page is the Home content while its flag is set and no other Home page took the content area. The open
    /// request's channel is the selected one while its panel shows (as the channel browser's preview), and the page
    /// stays the content.
    pub(super) fn message_requests_active(&self) -> bool {
        self.message_requests.open
            && self.guild.is_none()
            && (self.channel.is_none() || self.channel == self.message_requests.sidebar)
            && !self.nitro_open
            && !self.discover.open
            && !self.shop.open
            && !self.quests.open
    }

    pub(super) fn open_message_requests(&mut self, cx: &mut Context<Self>) {
        if self.message_requests_active() {
            return;
        }
        // Records the visit like `open_quests` does, so back and forward come back to this page.
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                message_requests: true,
                ..Default::default()
            },
        );
        self.set_message_requests_open(cx);
    }

    /// The part of `open_message_requests` that history replays also run.
    pub(super) fn set_message_requests_open(&mut self, cx: &mut Context<Self>) {
        self.favorites_view = false;
        self.discover.open = false;
        self.shop.open = false;
        self.quests.open = false;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.guild = None;
        self.channel = None;
        self.unread_from = None;
        self.stop_lurking_elsewhere();
        self.message_requests.open = true;
        self.message_requests.tab = Tab::Requests;
        self.message_requests.sidebar = None;
        self.rebuild_rows();
        self.request_images();
        cx.notify();
    }

    /// `openPrivateChannelAsSidebar` (`ta` / `th` of 793706): the request's chat opens in the panel. Its channel is
    /// the one the chat list, the welcome and the composer read, while the page keeps the content area.
    fn mr_open_panel(&mut self, channel: Id, cx: &mut Context<Self>) {
        self.message_requests.sidebar = Some(channel);
        self.channel = Some(channel);
        self.unread_from = None;
        self.menu = None;
        self.submenu = None;
        self.open_channel_messages(channel);
        // The welcome's mutual servers come from the other person's profile, as for any DM.
        let recipient = self
            .current_dm()
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id);
        if let Some(user) = recipient {
            let route = self.dm_profile_route(user);
            self.fetch_user_profile(user, route);
        }
        self.rebuild_rows();
        self.request_images();
        cx.notify();
    }

    /// `closeChannelSidebar`: the panel goes and nothing is selected again.
    pub(super) fn mr_close_panel(&mut self, cx: &mut Context<Self>) {
        let open = self.message_requests.sidebar.take();
        self.message_requests.resize_from = None;
        if open.is_some() && self.channel == open {
            self.channel = None;
            self.rebuild_rows();
        }
        cx.notify();
    }

    /// The request channels of the tab in view, newest first (`eS` in 793706).
    fn mr_ids(&self) -> Vec<Id> {
        match self.message_requests.tab {
            Tab::Requests => self.stores.message_request_ids(),
            Tab::Spam => self.stores.spam_request_ids(),
        }
    }

    fn mr_is_staff(&self) -> bool {
        self.stores
            .current_user
            .as_ref()
            .is_some_and(|user| (user.flags | user.public_flags) & dc_state::USER_FLAG_STAFF != 0)
    }

    /// Sends one call of this page; the answer comes back under `mr:<what>:<channel>`.
    fn mr_send(&mut self, what: &str, channel: Id, call: Call) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{KEY_PREFIX}{what}:{channel}")),
            method: call.method,
            route: call.route,
            body: call.body,
        });
    }

    /// Accept DM (`acceptMessageRequest` of module 190857): for an account in Great Britain whose NSFW content is not
    /// allowed (module 467840, `nsfwAllowed === false`) the confirmation of module 987176 asks first.
    pub(super) fn mr_accept(&mut self, channel: Id, cx: &mut Context<Self>) {
        if self.message_requests.busy.contains_key(&channel) {
            return;
        }
        if accept_needs_confirmation(
            self.stores.location_metadata.get_country_code(),
            self.stores.account.nsfw_allowed,
        ) {
            self.chat_extra.alert = Some(super::chat_notices::accept_request_alert(channel));
            cx.notify();
            return;
        }
        self.mr_accept_now(channel, cx);
    }

    /// `RK`: the row is busy until the answer.
    pub(super) fn mr_accept_now(&mut self, channel: Id, cx: &mut Context<Self>) {
        if self.message_requests.busy.contains_key(&channel) {
            return;
        }
        self.message_requests.busy.insert(channel, Busy::Accepting);
        self.mr_send("accept", channel, accept_call(channel));
        cx.notify();
    }

    /// Ignore (`UK`).
    pub(super) fn mr_reject(&mut self, channel: Id, cx: &mut Context<Self>) {
        if self.message_requests.busy.contains_key(&channel) {
            return;
        }
        self.message_requests.busy.insert(channel, Busy::Rejecting);
        self.mr_send("reject", channel, reject_call(channel));
        cx.notify();
    }

    /// Clear All (`rejectAll`): staff only in the official page, so no control reaches it for other accounts.
    fn mr_reject_all(&mut self, cx: &mut Context<Self>) {
        let ids = self.mr_ids();
        for (index, call) in reject_all_calls(&ids).into_iter().enumerate() {
            self.mr_send("reject-all", index as u64, call);
        }
        cx.notify();
    }

    /// The answers of this page's calls. `Update::Fetched` bodies are `{ok, status, body}`.
    pub(super) fn message_requests_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let mut parts = key.trim_start_matches(KEY_PREFIX).splitn(2, ':');
        let (what, tail) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let ok = body["ok"] == true;
        // A call refused by the inert harness never reached the server: nothing changed, nothing to report.
        let refused = body["refused"] == true;
        match what {
            "supplemental" => {
                let ids: Vec<Id> = tail.split(',').filter_map(|id| id.parse().ok()).collect();
                for id in &ids {
                    self.message_requests.loading.remove(id);
                }
                if ok {
                    self.stores
                        .message_requests_supplemental_success(&ids, &body["body"]);
                } else if !refused {
                    self.stores.message_requests_supplemental_error(&ids);
                }
            }
            "accept" | "reject" => {
                let Some(channel) = tail.parse::<Id>().ok() else {
                    return;
                };
                self.message_requests.busy.remove(&channel);
                if ok && what == "accept" {
                    // `MESSAGE_REQUEST_ACCEPT_OPTIMISTIC`, then `onAcceptSuccess`: the panel closes and, when this was
                    // the only request, the channel opens.
                    let single = self.stores.message_requests_count() == 1;
                    let spam = self.stores.is_spam_request(channel);
                    self.stores.message_request_accept_optimistic(channel);
                    self.mr_close_panel(cx);
                    if single && !spam {
                        self.select_home(Some(channel));
                    }
                } else if ok {
                    self.mr_close_panel(cx);
                } else if !refused {
                    // `EDYbS+`.
                    self.put_app_toast(
                        "Uh oh! Something went wrong while updating this message request. Please wait a moment and try again."
                            .to_owned(),
                        false,
                        cx,
                    );
                }
            }
            "reject-all" if !ok && !refused => {
                self.put_app_toast(
                    "Uh oh! Something went wrong while updating this message request. Please wait a moment and try again."
                        .to_owned(),
                    false,
                    cx,
                );
            }
            _ => {}
        }
        cx.notify();
    }

    /// Asks for what the rows need and has not got: previews (25 channels per call) and the people's mutual servers.
    fn mr_load(&mut self) {
        let ids = self.mr_ids();
        let wanted: Vec<Id> = self
            .stores
            .message_request_previews_to_load(&ids)
            .into_iter()
            .filter(|id| !self.message_requests.loading.contains(id))
            .collect();
        for chunk in wanted.chunks(25) {
            for id in chunk {
                self.message_requests.loading.insert(*id);
            }
            let joined = chunk
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{KEY_PREFIX}supplemental:{joined}")),
                method: reqwest::Method::GET,
                route: supplemental_route(chunk),
                body: None,
            });
        }
        let users: Vec<Id> = ids
            .iter()
            .filter_map(|id| {
                let channel = self.stores.private_channels.iter().find(|c| c.id == *id)?;
                self.stores.dm_recipient(channel).map(|user| user.id)
            })
            .collect();
        for user in users {
            let key = format!("profile:{user}");
            if self.fetched.contains_key(&key) || !self.requested.insert(key.clone()) {
                continue;
            }
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!(
                    "/users/{user}/profile?type=popout&with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=false"
                ),
            });
        }
        if !self.images.contains_key(ART_KEY) {
            let url = if matches!(super::theme(), dc_tokens::Theme::Light) {
                ART_LIGHT
            } else {
                ART_DARK
            };
            self.request_image(ART_KEY.to_owned(), url.to_owned());
        }
    }

    // ---- the page ----

    /// `tb`: the page (header and list), and beside it, over the page's whole height, the opened request (`tf`).
    /// `messages` is the chat list of the selected channel, which is the open request's while the panel shows.
    pub(super) fn message_requests_page(
        &mut self,
        window: &mut Window,
        messages: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.mr_load();
        let ids = self.mr_ids();
        let sidebar = self
            .message_requests
            .sidebar
            .filter(|id| self.channel == Some(*id))
            .filter(|id| self.stores.is_message_request(*id) || self.stores.is_spam_request(*id));
        let body = if ids.is_empty() {
            self.mr_empty()
        } else {
            self.mr_list(&ids, cx)
        };
        // `p.A` hands `tb` the width of the content area.
        let page_width = f32::from(window.viewport_size().width) - self.combined_sidebar_width();
        let stored = self.message_requests.width.or_else(|| {
            crate::storage::get("messageRequestSidebarWidth")
                .and_then(|value| value.as_f64())
                .map(|width| width as f32)
        });
        let (width, floating) = panel_layout(page_width, stored);
        let page = div().flex_1().min_w(px(0.)).h_full().pt(px(1.)).child(
            div()
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .bg(color("--background-base-lower"))
                .child(self.mr_header(cx))
                .child(div().flex_1().min_h(px(0.)).w_full().relative().child(body)),
        );
        let panel = sidebar.map(|id| self.mr_sidebar(id, messages, width, floating, cx));
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .child(page)
            .when_some(panel, |this, panel| {
                if floating {
                    // `floating_FrbQJp`: over the list at the right edge, the drag handle off.
                    this.child(div().absolute().top_0().right_0().bottom_0().child(panel))
                } else {
                    // The reserved space (`minWidth: width + handle`), the 8px handle, then the panel.
                    this.child(self.mr_resize_handle(width, cx)).child(panel)
                }
            })
            .into_any_element()
    }

    /// `resizeHandle_FrbQJp`: dragging left widens the panel up to the page's width less 650.
    fn mr_resize_handle(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("mr-panel-resize")
            .w(px(RESIZE_HANDLE))
            .h_full()
            .flex_none()
            .cursor_ew_resize()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.message_requests.resize_from = Some((f32::from(event.position.x), width));
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    pub(super) fn mr_move_resize(
        &mut self,
        event: &gpui::MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !self.message_requests_active() || !event.dragging() {
            return;
        }
        let Some((start_x, start_width)) = self.message_requests.resize_from else {
            return;
        };
        let page_width = f32::from(window.viewport_size().width) - self.combined_sidebar_width();
        let wanted = start_width + start_x - f32::from(event.position.x);
        let (width, _) = panel_layout(page_width, Some(wanted));
        self.message_requests.width = Some(width);
        cx.notify();
    }

    /// The drag's end saves the width (`updatedUnsyncedSettings`).
    pub(super) fn mr_end_resize(&mut self) {
        if self.message_requests.resize_from.take().is_some()
            && let Some(width) = self.message_requests.width
            && !crate::storage::settings_writes_blocked()
        {
            crate::storage::set("messageRequestSidebarWidth", serde_json::json!(width));
        }
    }

    /// The same header as the Friends page: 49px, icon 22px in, title 6px after it, a 4px dot, the tabs.
    fn mr_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.message_requests.tab;
        let spam = self.stores.spam_requests_count();
        let tab = |id: &'static str, label: String, which: Tab| {
            let active = current == which;
            div()
                .id(id)
                .mx(px(8.))
                .h(px(32.))
                .px(px(12.))
                .py(px(4.))
                .rounded(px(8.))
                .cursor_pointer()
                .flex()
                .items_center()
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .when(active, |this| {
                    this.bg(color("--background-mod-strong"))
                        .text_color(color("--text-strong"))
                })
                .when(!active, |this| {
                    this.text_color(color("--interactive-text-default"))
                        .hover(|style| {
                            style
                                .bg(color("--background-mod-subtle"))
                                .text_color(color("--text-strong"))
                        })
                })
                .rule(motion::Rule::TabItem)
                // `onItemSelect` of `tb` only changes the section: an opened request stays open.
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.message_requests.tab = which;
                    cx.notify();
                }))
                .child(label)
        };
        div()
            .parity_below("chat-header", 1.)
            .flex_none()
            .h(px(49.))
            .border_b_1()
            .border_color(color("--background-mod-subtle"))
            .pl(px(22.))
            .pr(px(8.))
            .flex()
            .items_center()
            .bg(color("--background-base-lower"))
            .child(icon("message_requests_envelope", 20., "--channels-default"))
            .child(
                div()
                    .ml(px(6.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Message Requests"),
            )
            .child(
                div()
                    .ml(px(12.))
                    .size(px(4.))
                    .rounded_full()
                    .bg(color("--background-mod-strong")),
            )
            .child(div().flex_none().w(px(4.)))
            .child(tab("mr-tab-requests", "Requests".to_owned(), Tab::Requests))
            .child(tab(
                "mr-tab-spam",
                // `ulKXHp`, or `5jtrlZ` with the count.
                if spam == 0 {
                    "Spam".to_owned()
                } else {
                    format!("Spam ({spam})")
                },
                Tab::Spam,
            ))
            .into_any_element()
    }

    /// `eN`: the 415x200 art, 40px above the note (the capture: art at y 267, note line at y 515, both centred).
    fn mr_empty(&self) -> AnyElement {
        let note = match self.message_requests.tab {
            // `SXrqTf` and `hasFPQ`.
            Tab::Requests => "There are no pending message requests. Here's Wumpus for now.",
            Tab::Spam => "There are no spam requests. Here's Wumpus for now.",
        };
        let art = self.images.get(ART_KEY).cloned();
        div()
            .parity("message-requests-empty")
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(415.))
                    .h(px(200.))
                    .mb(px(40.))
                    .children(art.map(|art| img(art).w(px(415.)).h(px(200.)))),
            )
            .child(
                div()
                    .mt(px(8.))
                    .min_h(px(20.))
                    .px(px(16.))
                    .text_center()
                    .line_height(px(20.))
                    .text_color(color("--text-subtle"))
                    .child(note),
            )
            .into_any_element()
    }

    fn mr_list(&self, ids: &[Id], cx: &mut Context<Self>) -> AnyElement {
        let spam = self.message_requests.tab == Tab::Spam;
        let count = ids.len();
        let staff = self.mr_is_staff();
        let hovered = self
            .message_requests
            .hovered
            .and_then(|hovered| ids.iter().position(|id| *id == hovered));
        let rows: Vec<AnyElement> = ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let channel = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == *id)?
                    .clone();
                Some(self.mr_row(&channel, index, spam, row_has_border(index, hovered), cx))
            })
            .collect();
        let scroll = self.message_requests.scroll.clone();
        let title = if spam {
            dc_intl::t!("C79Edh", count = count.to_string())
        } else if count > 0 {
            dc_intl::t!("rA4iWY", count = count.to_string())
        } else {
            dc_intl::t("flPU6g").to_owned()
        };
        div()
            .id("mr-scroll")
            .absolute()
            .inset_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(24.))
            .pt(px(24.))
            .pb(px(24.))
            .child(
                div()
                    .parity("message-requests-section")
                    .flex()
                    .items_center()
                    .ml(px(10.))
                    .mb(px(10.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    .child(title)
                    .when(staff && count > 0, |this| {
                        this.child(
                            div()
                                .mx(px(8.))
                                .text_color(color("--text-default"))
                                .child("\u{2022}"),
                        )
                        .child(
                            div()
                                .id("mr-clear-all")
                                .cursor_pointer()
                                .text_color(color("--text-link"))
                                .on_click(cx.listener(|this, _, _, cx| this.mr_reject_all(cx)))
                                .child("Clear All"),
                        )
                    }),
            )
            .children(rows)
            .into_any_element()
    }

    /// One request (`ek` holding `ti`, or `tu` on the Spam tab): `messageRequestItem_rRxPBT` (16px 10px,
    /// `body-md/medium`), on `--interactive-background-hover` with 8px corners while hovered, and on
    /// `--interactive-background-selected` (`selected_nMe-a6`) while its chat is open. Inside, `container_OVKWNO`
    /// (centred, spread): the 40px avatar 12px before the person (`userPreview_42Xsjf`: as wide as its content, at
    /// least a quarter of the row, 16px before the buttons), then the buttons at the end.
    fn mr_row(
        &self,
        channel: &PrivateChannel,
        index: usize,
        spam: bool,
        border: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = channel.id;
        let user: Option<User> = self.stores.dm_recipient(channel).cloned();
        let selected = self.message_requests.sidebar == Some(id) && self.channel == Some(id);
        let hovered = self.message_requests.hovered == Some(id);
        let busy =
            self.message_requests.busy.contains_key(&id) || self.stores.is_accepted_optimistic(id);
        let name = user.as_ref().map_or_else(
            || "Unknown User".to_owned(),
            |user| self.dm_display_name(user),
        );
        // `DiscordTag` with `showAccountIdentifier` (297413): the username follows, 5px on (`infoSpacing_PTA1Mz`), in
        // `--text-muted` (`globalName_42Xsjf`) for a person with a global name; without one its class is
        // `discriminator_42Xsjf`, `display: none`.
        let secondary = user
            .as_ref()
            .filter(|user| user.global_name.is_some())
            .map(|user| user.username.clone());
        let avatar = user.as_ref().and_then(|user| {
            let (key, url) = avatar_source(user);
            let image = self.images.get(&key).cloned();
            if image.is_none() {
                self.want_image(key, url, cx);
            }
            image
        });
        let loading_avatar = avatar.is_none();
        let preview = self.stores.message_request_preview(id);
        let then_ms = preview
            .message
            .as_ref()
            .map(|message| dc_state::read_state::snowflake_ms(message.id))
            .or_else(|| {
                (channel.last_message_id != 0)
                    .then(|| dc_state::read_state::snowflake_ms(channel.last_message_id))
            });
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64);
        let age = then_ms
            .map(|then| age_label(now_ms, then))
            .unwrap_or_default();
        // `e2`: the error line, the deleted-last-message line or a placeholder, in italics (`messageReplacement_c3PP-T`),
        // or the message itself (`messageContent_c3PP-T`); all at 16px on 18px lines, `--text-muted`, one line.
        let (hover_text, replacement) = if preview.error {
            ("Error loading message preview.".to_owned(), true)
        } else if !preview.loaded {
            (String::new(), false)
        } else {
            match preview.message.as_ref() {
                Some(message) => match placeholder(message) {
                    Some(line) => (line.to_owned(), true),
                    None => (message.content.replace('\n', " "), false),
                },
                None => ("The last message was deleted.".to_owned(), true),
            }
        };
        let (mutual, has_mutual) = match user.as_ref() {
            Some(user) => self.mr_mutual(user.id),
            None => (div().into_any_element(), false),
        };
        let height = roll_height(has_mutual);
        // `hoverRoll_Dq3ch1` (module 29160, CSS 433664): the resting line leaves up (translate3d(0,-107%,0), opacity 0)
        // while the hover line comes up from below, `transition: all .22s ease`. `ek` hands the row's own hover in as
        // `forceHover`; an open row does not keep it.
        let roll = crate::ui::motion::value(
            roll_key(id),
            if hovered { 1. } else { 0. },
            crate::ui::motion::css(220., crate::ui::motion::Ease::Ease),
        );
        let second_line = div()
            .relative()
            .flex()
            .flex_col()
            .h(px(height))
            .overflow_hidden()
            .child(
                div()
                    .relative()
                    .top(px(-height * 1.07 * roll))
                    .opacity(1. - roll)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(mutual),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .h(px(height))
                    .top(px(height * 1.07 * (1. - roll)))
                    .opacity(roll)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(16.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(400.))
                    .text_color(color("--text-muted"))
                    .when(replacement, |this| this.italic())
                    .child(hover_text),
            );
        let buttons = if spam {
            // `tu`: Accept DM as a secondary button, then Report Spam (`to`) while the message is known.
            div()
                .flex()
                .gap(px(8.))
                .flex_none()
                .ml_auto()
                .child(
                    small_button("mr-accept", "Accept DM", Tone::Secondary, !busy).on_click(
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.mr_accept(id, cx);
                        }),
                    ),
                )
                .when(
                    preview.message.is_some() || !(preview.loaded || preview.error),
                    |this| {
                        this.child(
                            small_button(
                                "mr-report",
                                "Report Spam",
                                Tone::CriticalPrimary,
                                preview.message.is_some(),
                            )
                            .on_click(cx.listener(move |_, _, _, cx| cx.stop_propagation())),
                        )
                    },
                )
        } else {
            // `actions_OVKWNO`: Ignore, 16px (`button_OVKWNO:not(:last-child)`), Accept DM.
            div()
                .flex()
                .flex_none()
                .ml_auto()
                .child(div().mr(px(16.)).child(
                    small_button("mr-ignore", "Ignore", Tone::Secondary, !busy).on_click(
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.mr_reject(id, cx);
                        }),
                    ),
                ))
                .child(
                    small_button("mr-accept", "Accept DM", Tone::Primary, !busy).on_click(
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.mr_accept(id, cx);
                        }),
                    ),
                )
        };
        div()
            .id(SharedString::from(format!("mr-row-{id}")))
            .w_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .px(px(10.))
            .py(px(16.))
            .cursor_pointer()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .when(index > 0, |this| {
                this.border_t_1().border_color(if border {
                    color("--border-subtle")
                } else {
                    gpui::transparent_black().into()
                })
            })
            .when(selected && !hovered, |this| {
                this.bg(color("--interactive-background-selected"))
                    .rounded(px(8.))
            })
            .when(hovered, |this| {
                this.bg(color("--interactive-background-hover"))
                    .rounded(px(8.))
            })
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if *inside {
                    this.message_requests.hovered = Some(id);
                } else if this.message_requests.hovered == Some(id) {
                    this.message_requests.hovered = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.mr_open_panel(id, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .child(
                        div()
                            .flex_none()
                            .mr(px(12.))
                            .size(px(40.))
                            .when(loading_avatar, |this| {
                                this.rounded_full().bg(color("--background-mod-subtle"))
                            })
                            .children(
                                avatar.map(|avatar| img(avatar).size(px(40.)).rounded_full()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .mr(px(16.))
                            .min_w(gpui::relative(0.25))
                            .overflow_hidden()
                            .child(
                                // `userContainerWithPreview_42Xsjf`: the name and the age, centred, 8px apart.
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        // `info_PTA1Mz tagContainer_42Xsjf`.
                                        div()
                                            .flex()
                                            .items_end()
                                            .mt(px(2.))
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(name),
                                            )
                                            .children(secondary.map(|secondary| {
                                                div()
                                                    .ml(px(5.))
                                                    .text_color(color("--text-muted"))
                                                    .child(secondary)
                                            })),
                                    )
                                    .child(
                                        // `timestampWithPreview_42Xsjf`, `body-xs/normal`.
                                        div()
                                            .whitespace_nowrap()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(400.))
                                            .text_color(color("--text-muted"))
                                            .child(age),
                                    ),
                            )
                            .child(second_line),
                    )
                    .child(buttons),
            )
            .into_any_element()
    }

    /// `tt`: the guilds' icons (16px, 3 at most, overlapping by 3px) and "N Mutual Servers" on a 4px-padded row
    /// (`mutualGuildsContainer_42Xsjf`), or "No Mutual Servers" alone; `body-sm/normal`, `--text-muted`. The flag says
    /// which, for the roll's height.
    fn mr_mutual(&self, user: Id) -> (AnyElement, bool) {
        let guilds: Vec<Id> = self
            .fetched
            .get(&format!("profile:{user}"))
            .and_then(|body| body["mutual_guilds"].as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|entry| entry["id"].as_str().and_then(|id| id.parse().ok()))
                    .collect()
            })
            .unwrap_or_default();
        let text = |label: String| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(400.))
                .text_color(color("--text-muted"))
                .child(label)
        };
        if guilds.is_empty() {
            // `jpY0X5`.
            return (
                text("No Mutual Servers".to_owned()).into_any_element(),
                false,
            );
        }
        let count = guilds.len();
        let row = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .pt(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .children(guilds.iter().take(3).enumerate().map(|(index, guild)| {
                        let image = self.images.get(&format!("guild:{guild}")).cloned();
                        div()
                            .size(px(16.))
                            .flex_none()
                            .when(index + 1 < count.min(3), |this| this.mr(px(-3.)))
                            .rounded(px(5.))
                            .overflow_hidden()
                            .bg(color("--background-base-lowest"))
                            .children(image.map(|image| img(image).size(px(16.))))
                    })),
            )
            .child(text(if count == 1 {
                // `eE3oep`.
                "1 Mutual Server".to_owned()
            } else {
                format!("{count} Mutual Servers")
            }));
        (row.into_any_element(), true)
    }

    // ---- the opened request ----

    /// The width the opened request's chat lays its messages out in.
    pub(super) fn mr_panel_width(&self, viewport_width: f32) -> f32 {
        let stored = self.message_requests.width.or_else(|| {
            crate::storage::get("messageRequestSidebarWidth")
                .and_then(|value| value.as_f64())
                .map(|width| width as f32)
        });
        panel_layout(viewport_width - self.combined_sidebar_width(), stored).0
    }

    /// The opened request (`tf` / `B` of 793706): module 812771's frame (`container_FrbQJp`, drawn as the channel
    /// browser's preview draws it, with `floating_FrbQJp`'s border and shadow over the list), the DM header bar whose
    /// toolbar is only Close (`zF` with `inSidebar` gives 849205: the 20px avatar with its status, 8px, the name), the
    /// spam banner, then the request's own chat (699344): the chat list of the selected channel, whose welcome carries
    /// Accept DM, Ignore and Report Spam (`aE` of 891522), and the composer.
    fn mr_sidebar(
        &self,
        channel: Id,
        messages: AnyElement,
        width: f32,
        floating: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let spam = self.stores.is_spam_request(channel);
        let user = self
            .current_dm()
            .and_then(|dm| self.stores.dm_recipient(dm))
            .cloned();
        let name = user.as_ref().map_or_else(
            || "Unknown User".to_owned(),
            |user| self.dm_display_name(user),
        );
        let busy = self.message_requests.busy.contains_key(&channel)
            || self.stores.is_accepted_optimistic(channel);
        let close = div()
            .id("mr-panel-close")
            .group("mr-panel-close")
            .size(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(
                gpui::svg()
                    .path("icons/ob_x.svg")
                    .size(px(20.))
                    .text_color(color("--text-muted"))
                    .group_hover("mr-panel-close", |style| {
                        style.text_color(color("--interactive-text-default"))
                    }),
            );
        let header = div()
            .parity_below("message-request-panel-header", 1.)
            .h(px(49.))
            .flex_none()
            .pl(px(16.))
            .pr(px(8.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .when_some(user.clone(), |this, user| {
                // `avatar_z_gUBg`: 20px with the status cut into its corner, 8px before the title; the hidden
                // "Direct Message" prefix before it takes 1px.
                let presence = self.stores.status(user.id).to_owned();
                let (key, url) = avatar_source(&user);
                let picture = self.images.get(&key).cloned();
                if picture.is_none() {
                    self.want_image(key, url, cx);
                }
                this.child(
                    div()
                        .ml(px(1.))
                        .relative()
                        .size(px(20.))
                        .flex_none()
                        .children(picture.map(|picture| img(picture).size(px(20.)).rounded_full()))
                        .child(super::status_dot_small(
                            &presence,
                            "--background-base-lower",
                        )),
                )
            })
            .child(
                div()
                    .ml(px(8.))
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(name),
            )
            .child(div().flex_1())
            .child(
                // `toolbar_ybUK7Z`: 8px in, then Close (`cpT0Cq`).
                div().pl(px(8.)).child(
                    self.with_tip(
                        "mr-panel-close",
                        super::tooltip::Tip::text("Close", super::tooltip::Side::Bottom),
                        close,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.mr_close_panel(cx))),
                ),
            );
        div()
            .parity("message-request-panel")
            .id("mr-sidebar")
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded_tl(px(12.))
            .border_t_1()
            .border_l_1()
            .border_r_1()
            .border_color(color(if floating {
                "--background-base-lower"
            } else {
                "--border-subtle"
            }))
            .bg(color("--background-base-lower"))
            .when(floating, |this| {
                // `floating_FrbQJp`: `box-shadow: 0 8px 40px var(--opacity-black-52)`.
                this.shadow(vec![gpui::BoxShadow {
                    color: color("--opacity-black-52").into(),
                    offset: gpui::point(px(0.), px(8.)),
                    blur_radius: px(40.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
            .child(header)
            .when(spam, |this| {
                // `hamBanner_aVwxbe`: 10px 16px on `--background-mod-normal`, text and a 12px-in button.
                this.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .w_full()
                        .px(px(16.))
                        .py(px(10.))
                        .bg(color("--background-mod-normal"))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child("Was this message spam? Let us know and help improve Discord\u{2019}s spam filtering."),
                        )
                        .child(
                            div().ml(px(12.)).flex_none().child(
                                small_button(
                                    "mr-not-spam",
                                    "Mark as Not Spam",
                                    Tone::Secondary,
                                    !busy,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // `markAsNotSpam` (module 190857), asking first (693864).
                                    this.mark_not_spam(channel, cx)
                                })),
                            ),
                        ),
                )
            })
            .child(
                div()
                    .parity("message-request-panel-messages")
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(messages)
                    .children(self.new_messages_bar(cx)),
            )
            .child(self.composer_bar(cx).into_any_element())
            .into_any_element()
    }
}

/// Harness-only data shaped like the official events for a stranger's DM: nothing is sent anywhere.
#[cfg(feature = "capture")]
mod fixture {
    use super::*;

    const NAMES: [(&str, &str); 6] = [
        ("Skye Marlow", "skyemarlow"),
        ("Noor Hale", "noorhale"),
        ("Pip Ashgrove", "pip.ash"),
        ("Rowan Vale", "rowanvale"),
        ("Tamsin Reed", "tamsin_r"),
        ("Orin Fell", "orinfell"),
    ];
    const SAYS: [&str; 6] = [
        "hey! saw you in the server, wanna chat?",
        "Are you the one who made the fan patch? It's brilliant",
        "hi, can we talk about the thing from yesterday",
        "heyyy",
        "Got a minute? I have a question about your build",
        "found your name through a friend of a friend, hope that's ok",
    ];

    /// The id of a fixture user or channel: far above any real id, so nothing real is touched.
    fn fake(base: u64, index: usize) -> Id {
        base + index as u64
    }

    /// More of what each person says after the first line, so the opened chat has a group to draw.
    const FOLLOW_UPS: [&str; 3] = [
        "no rush btw",
        "I can send the details here if that works for you",
        "anyway, have a good one",
    ];

    /// `CHANNEL_CREATE` for the `index`-th request (`is_message_request`, `is_spam` and the request time, with the
    /// stranger in `recipients`) and its messages, oldest first: for a request, three a minute apart (one group) and
    /// a fourth 15 minutes on (a new group); for spam, one.
    pub fn channel(index: usize, spam: bool, now_ms: u64) -> (Value, Vec<Value>) {
        let (display, handle) = NAMES[index % NAMES.len()];
        let user = fake(9_100_000_000_000_000_000, index + usize::from(spam) * 100);
        let base = now_ms - (index as u64 + 1) * 25 * 60_000 - u64::from(spam) * 90_000;
        let channel = dc_state::read_state::snowflake_at(base);
        let stamp_at = |ms: u64| {
            chrono::DateTime::from_timestamp_millis(ms as i64)
                .map(|time| time.to_rfc3339())
                .unwrap_or_default()
        };
        let author = json!({"id": user.to_string(), "username": handle, "global_name": display,
            "avatar": null, "discriminator": "0"});
        let said: Vec<(u64, String)> = if spam {
            vec![(
                30_000,
                "FREE NITRO for the first 100 people, claim it at example.invalid/claim".to_owned(),
            )]
        } else {
            vec![
                (30_000, SAYS[index % SAYS.len()].to_owned()),
                (90_000, FOLLOW_UPS[0].to_owned()),
                (150_000, FOLLOW_UPS[1].to_owned()),
                (15 * 60_000, FOLLOW_UPS[2].to_owned()),
            ]
        };
        let messages: Vec<Value> = said
            .into_iter()
            .map(|(offset, text)| {
                let id = dc_state::read_state::snowflake_at(base + offset);
                json!({
                    "id": id.to_string(), "channel_id": channel.to_string(), "type": 0,
                    "author": author.clone(), "content": text, "timestamp": stamp_at(base + offset),
                    "attachments": [], "embeds": []
                })
            })
            .collect();
        let last = messages
            .last()
            .map(|message| message["id"].clone())
            .unwrap_or(Value::Null);
        let channel_json = json!({
            "id": channel.to_string(), "type": 1, "last_message_id": last,
            "recipients": [author], "is_message_request": true, "is_spam": spam,
            "is_message_request_timestamp": stamp_at(base), "flags": 0
        });
        (channel_json, messages)
    }
}

impl Root {
    /// Harness steps `mr ...`: they only dispatch events into the stores, as the gateway would.
    ///
    /// `mr add <requests> <spam>`, `mr open [spam]`, `mr view <n>` (opens the n-th row's panel),
    /// `mr accepted <n>` and `mr removed <n>` (the events after an accept or a delete), `mr state`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_message_requests(
        &mut self,
        args: &[&str],
        cx: &mut Context<Self>,
    ) -> String {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64);
        match args {
            ["add", requests, spam] => {
                let (Ok(requests), Ok(spam)) = (requests.parse::<usize>(), spam.parse::<usize>()) else {
                    return "usage: mr add <requests> <spam>".into();
                };
                for (count, is_spam) in [(requests, false), (spam, true)] {
                    for index in 0..count {
                        let (channel, messages) = fixture::channel(index, is_spam, now_ms);
                        self.stores.dispatch("CHANNEL_CREATE", &channel);
                        for message in &messages {
                            self.stores.dispatch("MESSAGE_CREATE", message);
                        }
                        // The whole history is here, as after the fetch of a short DM: the chat starts with the
                        // welcome and nothing is requested.
                        if let Some(id) = channel["id"].as_str().and_then(|id| id.parse::<Id>().ok()) {
                            let window = self.stores.windows.entry(id).or_default();
                            window.ready = true;
                            window.has_fetched = true;
                            window.has_more_before = false;
                        }
                    }
                }
                self.request_images();
                cx.notify();
                format!(
                    "message requests: {} requests, {} spam",
                    self.stores.message_requests_count(),
                    self.stores.spam_requests_count()
                )
            }
            ["open"] | ["open", "requests"] | ["open", "spam"] => {
                // Through the visiting path, so a script can step back and forward over the page.
                if self.message_requests_active() {
                    self.set_message_requests_open(cx);
                } else {
                    self.open_message_requests(cx);
                }
                if args.get(1) == Some(&"spam") {
                    self.message_requests.tab = Tab::Spam;
                }
                "message requests: open".into()
            }
            ["view", n] => {
                let Some(id) = n.parse::<usize>().ok().and_then(|n| self.mr_ids().get(n).copied()) else {
                    return "message requests: no such row".into();
                };
                self.mr_open_panel(id, cx);
                format!("message requests: panel {id}")
            }
            ["accepted", n] | ["removed", n] => {
                let Some(id) = n.parse::<usize>().ok().and_then(|n| self.mr_ids().get(n).copied()) else {
                    return "message requests: no such row".into();
                };
                if args[0] == "accepted" {
                    self.stores.message_request_accept_optimistic(id);
                    self.stores.dispatch(
                        "CHANNEL_UPDATES",
                        &json!({"channels": [{"id": id.to_string(), "type": 1, "is_message_request": false, "is_spam": false}]}),
                    );
                } else {
                    self.stores.dispatch("CHANNEL_DELETE", &json!({"id": id.to_string(), "type": 1}));
                }
                self.mr_close_panel(cx);
                format!("message requests: {} {id}", args[0])
            }
            ["state"] => format!(
                "message requests: {} requests, {} spam, entry visible {}, unread mentions {}",
                self.stores.message_requests_count(),
                self.stores.spam_requests_count(),
                self.stores.message_requests_entry_visible(),
                dc_state::unread::total_counts(&self.stores, chrono::Utc::now()).mentions,
            ),
            _ => "usage: mr add <n> <n> | open [spam] | view <n> | accepted <n> | removed <n> | state".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_british_account_without_nsfw_confirms() {
        assert!(accept_needs_confirmation(Some("GB"), false));
        assert!(!accept_needs_confirmation(Some("GB"), true));
        assert!(!accept_needs_confirmation(Some("US"), false));
        assert!(!accept_needs_confirmation(None, false));
    }

    #[test]
    fn accept_puts_consent_status_accepted() {
        let call = accept_call(42);
        assert_eq!(call.method, reqwest::Method::PUT);
        assert_eq!(call.route, "/channels/42/recipients/@me");
        assert_eq!(call.body, Some(json!({"consent_status": 2})));
    }

    #[test]
    fn ignore_deletes_the_recipient_without_a_body() {
        let call = reject_call(42);
        assert_eq!(call.method, reqwest::Method::DELETE);
        assert_eq!(call.route, "/channels/42/recipients/@me");
        assert_eq!(call.body, None);
    }

    #[test]
    fn clear_all_rejects_in_batches_of_fifty() {
        let ids: Vec<Id> = (1..=120).collect();
        let calls = reject_all_calls(&ids);
        assert_eq!(calls.len(), 3);
        for call in &calls {
            assert_eq!(call.method, reqwest::Method::PUT);
            assert_eq!(call.route, "/channels/recipients/@me/batch-reject");
        }
        assert_eq!(
            calls[0].body.as_ref().unwrap()["channel_ids"]
                .as_array()
                .unwrap()
                .len(),
            50
        );
        assert_eq!(
            calls[2].body.as_ref().unwrap()["channel_ids"]
                .as_array()
                .unwrap()
                .len(),
            20
        );
        assert_eq!(calls[0].body.as_ref().unwrap()["channel_ids"][0], "1");
        assert!(reject_all_calls(&[]).is_empty());
    }

    #[test]
    fn supplemental_route_repeats_channel_ids() {
        assert_eq!(
            supplemental_route(&[5, 6]),
            "/users/@me/message-requests/supplemental-data?channel_ids=5&channel_ids=6"
        );
    }

    #[test]
    fn ages_follow_module_469881() {
        let at = |seconds: u64| age_label(seconds * 1000 + 1_000_000, 1_000_000);
        assert_eq!(at(5), "1m ago");
        assert_eq!(at(59), "1m ago");
        assert_eq!(at(60), "1m ago");
        assert_eq!(at(125), "2m ago");
        assert_eq!(at(3 * 3600 + 5), "3h ago");
        assert_eq!(at(2 * 86400), "2d ago");
        assert_eq!(at(40 * 86400), ">30d ago");
        // A year on: the long date.
        assert_eq!(
            age_label(1_800_000_000_000, 1_700_000_000_000),
            "November 14, 2023"
        );
    }

    #[test]
    fn the_panel_floats_at_450_until_the_page_leaves_room_beside_it() {
        // 1280 wide with the 72px rail and the 240px channel list: 968 of page, 318 of room, so it floats.
        assert_eq!(panel_layout(968., None), (450., true));
        assert_eq!(panel_layout(968., Some(700.)), (450., true));
        // 1100 is the edge: `maxWidth <= minWidth` still floats.
        assert_eq!(panel_layout(1100., None), (450., true));
        assert_eq!(panel_layout(1101., None), (450., false));
        // Docked, a stored width is kept between 450 and the page less 650.
        assert_eq!(panel_layout(1500., Some(600.)), (600., false));
        assert_eq!(panel_layout(1500., Some(900.)), (850., false));
        assert_eq!(panel_layout(1500., Some(200.)), (450., false));
    }

    #[test]
    fn a_hovered_row_and_the_one_after_it_lose_their_top_border() {
        assert!(!row_has_border(0, None));
        assert!(row_has_border(1, None));
        assert!(!row_has_border(2, Some(2)));
        assert!(!row_has_border(3, Some(2)));
        assert!(row_has_border(4, Some(2)));
        assert!(row_has_border(1, Some(3)));
    }

    #[test]
    fn the_roll_is_one_line_or_the_padded_icon_row() {
        assert_eq!(roll_height(false), 18.);
        assert_eq!(roll_height(true), 22.);
    }

    #[test]
    fn previews_without_text_use_the_reply_placeholders() {
        let message = |value: Value| dc_state::Message::from_value(&value).unwrap();
        let base = json!({"id": "1", "channel_id": "2", "type": 0, "content": "",
            "author": {"id": "3", "username": "x"}, "timestamp": "2026-11-10T00:00:00Z"});
        assert_eq!(placeholder(&message(base.clone())), None);
        let mut with_file = base.clone();
        with_file["attachments"] =
            json!([{"id": "9", "filename": "a.png", "size": 1, "url": "u", "proxy_url": "u"}]);
        assert_eq!(
            placeholder(&message(with_file)),
            Some("Click to see attachment")
        );
        let mut text = base;
        text["content"] = json!("hi");
        assert_eq!(placeholder(&message(text)), None);
    }
}
