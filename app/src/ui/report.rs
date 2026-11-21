//! In-app reporting (modules 422495 and 369053): the server sends a menu of nodes (`GET /reporting/menu/{name}`),
//! each with a header, a subheader, elements (the reported user, checkboxes, links, breadcrumbs, success offers),
//! child options and a button (next, submit, done, cancel). The client walks it, keeping each step's choice, and
//! submits `POST /reporting/{name}` with the visited node ids as breadcrumbs and the checked keys per element. The
//! answer moves to the success node or shows the error line. Measured in `design/dom/modals/report-user-*.json`;
//! the menu itself is recorded in `design/snapshot/TRAFFIC/report-menu-user.json`.

use super::{Root, avatar_source, color, header_time, icon};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ScrollHandle, SharedString, Window, deferred,
    div, img, prelude::*, px, rgba,
};
use serde_json::{Value, json};

pub(super) const MENU_KEY: &str = "report-menu";
pub(super) const SUBMIT_KEY: &str = "report-submit";
/// `et.t02.INVALID_FORM_BODY`.
const INVALID_FORM_BODY: u64 = 50035;
/// The success element's illustration (module 700129), 288 by 162.
const SUCCESS_ART: &str = "https://cdn.discordapp.com/assets/content/9d1441338ce9090a7c1e9fab8fe34e58daa2ab7df4ac84fa280c308869d8f27b.svg";

/// One step taken: the node, where it led (the option's label, empty for a button) and its checked boxes.
#[derive(Clone, Debug)]
struct Step {
    node: u64,
    label: String,
    checked: Vec<(String, String, String)>,
}

pub(super) struct Report {
    user: Id,
    guild: Option<Id>,
    /// What is reported besides a person: a message (`message`, or `first_dm` for a DM's spam report) as its channel,
    /// id, author's name and text.
    message: Option<ReportedMessage>,
    /// A server event or an application when that is what is reported.
    subject: Option<Subject>,
    menu: Option<Value>,
    node: u64,
    history: Vec<Step>,
    /// The current node's ticked boxes: (element name, key, label).
    checked: Vec<(String, String, String)>,
    submitting: bool,
    error: Option<String>,
    /// The content scroller and the message preview's own scroller (thin scrollbars).
    scroll: ScrollHandle,
    preview_scroll: ScrollHandle,
}

/// What `h` in module 369053 builds a body for besides users and messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subject {
    Event {
        guild: Id,
        event: Id,
    },
    App {
        app: Id,
        guild: Option<Id>,
        channel: Option<Id>,
        entrypoint: &'static str,
    },
    /// A Board widget (`t0.WIDGET`): its owner and the widget's id.
    Widget {
        user: Id,
        widget: Id,
    },
    /// A live Stage (`t0.STAGE_CHANNEL`, Report Stage of module 304694): its instance, channel and guild.
    Stage {
        instance: Id,
        channel: Id,
        guild: Id,
    },
}

impl Subject {
    fn name(self) -> &'static str {
        match self {
            Self::Event { .. } => "guild_scheduled_event",
            Self::App { .. } => "application",
            Self::Widget { .. } => "widget",
            Self::Stage { .. } => "stage_channel",
        }
    }
}

#[derive(Clone, Debug)]
struct ReportedMessage {
    /// The report's `name`: `message`, or `first_dm` for the Report Spam of a DM's welcome.
    name: &'static str,
    channel: Id,
    id: Id,
    /// What the `message_preview` element shows (`messagePreviewContainer_QOd4rY`).
    message: Option<Box<dc_state::Message>>,
}

fn node(menu: &Value, id: u64) -> &Value {
    &menu["nodes"][id.to_string()]
}

/// `lt(node, type)`: the node's element of that type.
fn element<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
    node["elements"]
        .as_array()?
        .iter()
        .find(|element| element["type"] == kind)
}

impl Root {
    pub(super) fn open_report_user(
        &mut self,
        user: Id,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_report(user, None, None, cx);
    }

    /// Report Event (module 928658 `uA`).
    pub(super) fn open_report_event(&mut self, guild: Id, event: Id, cx: &mut Context<Self>) {
        self.start_report(0, None, Some(Subject::Event { guild, event }), cx);
    }

    /// Report App (module 928658 `r3` + 630891): the guild and channel shown when opened,
    /// plus `GET /applications/{id}/public` so application metadata is available.
    pub(super) fn open_report_app(
        &mut self,
        app: Id,
        entrypoint: &'static str,
        cx: &mut Context<Self>,
    ) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("report-app-meta:{app}"),
            route: format!("/applications/{app}/public"),
        });
        let subject = Subject::App {
            app,
            guild: self.guild,
            channel: self.channel,
            entrypoint,
        };
        self.start_report(0, None, Some(subject), cx);
    }

    /// Report Widget (module 928658 `GJ`): `name: widget` with the owner's `user_id` and the `widget_id`.
    pub(super) fn open_report_widget(&mut self, user: Id, widget: Id, cx: &mut Context<Self>) {
        self.start_report(user, None, Some(Subject::Widget { user, widget }), cx);
    }

    /// Report Stage (module 928658 `pb`): the channel's live Stage instance, if it has one.
    pub(super) fn open_report_stage(&mut self, channel: Id, cx: &mut Context<Self>) {
        let Some(instance) = self.stores.stage_instances.get_by_channel(channel) else {
            return;
        };
        let subject = Subject::Stage {
            instance: instance.id,
            channel: instance.channel_id,
            guild: instance.guild_id,
        };
        self.start_report(0, None, Some(subject), cx);
    }

    fn start_report(
        &mut self,
        user: Id,
        message: Option<ReportedMessage>,
        subject: Option<Subject>,
        cx: &mut Context<Self>,
    ) {
        let name = subject
            .map(Subject::name)
            .or_else(|| message.as_ref().map(|message| message.name))
            .unwrap_or("user");
        self.report = Some(Report {
            user,
            guild: self.guild,
            message,
            subject,
            menu: None,
            node: 0,
            history: Vec::new(),
            checked: Vec::new(),
            submitting: false,
            error: None,
            scroll: ScrollHandle::new(),
            preview_scroll: ScrollHandle::new(),
        });
        let _ = self.requests.send(Request::Rest {
            key: Some(MENU_KEY.to_owned()),
            method: reqwest::Method::GET,
            route: format!("/reporting/menu/{name}"),
            body: None,
        });
        if let Some(known) = self.stores.users.get(&user).cloned() {
            let (image, url) = avatar_source(&known);
            self.request_image(image, url);
        }
        self.request_image(SUCCESS_ART.to_owned(), SUCCESS_ART.to_owned());
        cx.notify();
    }

    /// Report Message, and Report Spam on a DM's welcome (`first_dm`): `b8` and `V2` in module 928658.
    pub(super) fn open_report_message(
        &mut self,
        message: &dc_state::Message,
        first_dm: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = window;
        self.start_report(
            message.author.id,
            Some(ReportedMessage {
                name: if first_dm { "first_dm" } else { "message" },
                channel: message.channel_id,
                id: message.id,
                message: Some(Box::new(message.clone())),
            }),
            None,
            cx,
        );
    }

    pub(super) fn report_fetched(
        &mut self,
        key: &str,
        report: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if key == MENU_KEY {
            if let Some(open) = self.report.as_mut() {
                let body = &report["body"];
                if let Some(root) = body["root_node_id"].as_u64() {
                    open.node = root;
                    open.menu = Some(body.clone());
                } else {
                    self.report = None;
                    self.put_app_toast("Unable to load report menu.".to_owned(), false, cx);
                }
            }
            cx.notify();
            return true;
        }
        if key == SUBMIT_KEY {
            if let Some(open) = self.report.as_mut() {
                open.submitting = false;
                let menu = open.menu.clone().unwrap_or_default();
                if report["ok"] == true {
                    // Module 630891: close the underlying Report App chooser once submitted.
                    if matches!(
                        self.chat_extra
                            .alert
                            .as_ref()
                            .and_then(|a| a.content.as_ref()),
                        Some(super::chat_notices::Content::ReportApp { .. })
                    ) {
                        self.chat_extra.alert = None;
                    }
                    let success = menu["success_node_id"].as_u64().unwrap_or(0);
                    open.history.push(Step {
                        node: open.node,
                        label: String::new(),
                        checked: std::mem::take(&mut open.checked),
                    });
                    open.node = success;
                    open.error = None;
                } else {
                    open.error = Some(
                        if report["body"]["code"].as_u64() == Some(INVALID_FORM_BODY) {
                            "We couldn't locate the content you are trying to report. Please review the details of \
                         your submission and re-submit."
                            .to_owned()
                        } else {
                            "There was an error trying to submit this report. Please try again!"
                                .to_owned()
                        },
                    );
                }
            }
            cx.notify();
            return true;
        }
        false
    }

    /// `er([label, target])`: on to another node, the step remembered.
    fn report_go(&mut self, label: String, target: u64, cx: &mut Context<Self>) {
        if let Some(open) = self.report.as_mut() {
            open.history.push(Step {
                node: open.node,
                label,
                checked: std::mem::take(&mut open.checked),
            });
            open.node = target;
            open.error = None;
        }
        cx.notify();
    }

    fn report_back(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.report.as_mut()
            && let Some(step) = open.history.pop()
        {
            open.node = step.node;
            open.checked = step.checked;
            open.error = None;
        }
        cx.notify();
    }

    /// The body `h(menu, type, history)` builds for a user report.
    fn report_body(open: &Report, menu: &Value, success: u64) -> Value {
        let mut steps = open.history.clone();
        steps.push(Step {
            node: open.node,
            label: String::new(),
            checked: open.checked.clone(),
        });
        let _ = success;
        let mut elements = serde_json::Map::new();
        for step in &steps {
            for (name, key, _) in &step.checked {
                let entry = elements
                    .entry(name.clone())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Some(list) = entry.as_array_mut() {
                    list.push(json!(key));
                }
            }
        }
        let mut body = json!({
            "version": menu["version"],
            "variant": menu["variant"],
            "language": menu["language"].as_str().unwrap_or("en"),
            "breadcrumbs": steps.iter().map(|step| step.node).collect::<Vec<_>>(),
            "elements": elements,
        });
        match (open.subject, open.message.as_ref()) {
            (Some(Subject::Event { guild, event }), _) => {
                body["name"] = json!("guild_scheduled_event");
                body["guild_id"] = json!(guild.to_string());
                body["guild_scheduled_event_id"] = json!(event.to_string());
            }
            (
                Some(Subject::App {
                    app,
                    guild,
                    channel,
                    entrypoint,
                }),
                _,
            ) => {
                body["name"] = json!("application");
                body["application_id"] = json!(app.to_string());
                if let Some(guild) = guild {
                    body["guild_id"] = json!(guild.to_string());
                }
                if let Some(channel) = channel {
                    body["channel_id"] = json!(channel.to_string());
                }
                body["entrypoint"] = json!(entrypoint);
            }
            // `h` in module 369053: `{name, channel_id, guild_id, stage_instance_id}`.
            (
                Some(Subject::Stage {
                    instance,
                    channel,
                    guild,
                }),
                _,
            ) => {
                body["name"] = json!("stage_channel");
                body["channel_id"] = json!(channel.to_string());
                body["guild_id"] = json!(guild.to_string());
                body["stage_instance_id"] = json!(instance.to_string());
            }
            // `h` in module 369053: `{name, user_id, widget_id}`.
            (Some(Subject::Widget { user, widget }), _) => {
                body["name"] = json!("widget");
                body["user_id"] = json!(user.to_string());
                body["widget_id"] = json!(widget.to_string());
            }
            // `h` in module 369053: a message report carries its channel and id, and no guild.
            (None, Some(message)) => {
                body["name"] = json!(message.name);
                body["channel_id"] = json!(message.channel.to_string());
                body["message_id"] = json!(message.id.to_string());
            }
            (None, None) => {
                body["name"] = json!("user");
                body["user_id"] = json!(open.user.to_string());
                if let Some(guild) = open.guild {
                    body["guild_id"] = json!(guild.to_string());
                }
            }
        }
        body
    }

    fn report_submit(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.report.as_mut() else {
            return;
        };
        let Some(menu) = open.menu.clone() else {
            return;
        };
        let success = menu["success_node_id"].as_u64().unwrap_or(0);
        let body = Self::report_body(open, &menu, success);
        open.submitting = true;
        let _ = self.requests.send(Request::Rest {
            key: Some(SUBMIT_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: format!("/reporting/{}", body["name"].as_str().unwrap_or("user")),
            body: Some(body),
        });
        cx.notify();
    }

    /// Markdown in `body-sm/normal` on `--text-default`, its links followed like any other.
    fn report_markdown(&self, text: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let blocks = dc_markdown::parse(text);
        let tint: gpui::Hsla = color("--text-default").into();
        div().flex().flex_col().children(blocks.iter().map(|block| {
            let spans = super::block_spans(block).to_vec();
            let pieces = spans
                .iter()
                .map(|span| self.piece(span, tint, 14., 400., 18.))
                .collect();
            let links = super::link_runs(&spans);
            let root = cx.weak_entity();
            crate::inline::inline(pieces, px(18.)).on_piece_click(move |piece, _, cx| {
                if let Some(Some((url, label))) = links.get(piece).cloned() {
                    let _ = root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
                }
            })
        }))
    }

    fn report_heading(text: impl Into<SharedString>, weight: f32) -> gpui::Div {
        div()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(weight))
            .text_color(color("--text-default"))
            .child(text.into())
    }

    /// The message under "Selected Message" (`messagePreviewContainer_QOd4rY`, `design/dom/report/step1`): a cozy
    /// message (avatar 16px in, name, app tag, inline time, text) in a 100px scroller with 16px of padding above,
    /// right and below, an 8px radius and a thin scrollbar. The wrapper keeps 72px on the left and 16px on the
    /// right, and the scrollbar gutter takes 8px more.
    fn report_message_preview(
        &self,
        message: &ReportedMessage,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(shown) = message.message.as_deref() else {
            return div().into_any_element();
        };
        let handle = self
            .report
            .as_ref()
            .map(|open| open.preview_scroll.clone())
            .unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&shown.author).0).cloned();
        let tag = super::message_header::app_tag(
            shown.author.bot,
            shown.author.flags | shown.author.public_flags,
            false,
        )
        .map(|kind| self.app_tag_chip(kind, shown.id, cx));
        let wrapper = div()
            .relative()
            .pl(px(72.))
            .pr(px(16.))
            .py(px(2.))
            .child(
                div()
                    .absolute()
                    .left(px(16.))
                    .top(px(4.))
                    .size(px(40.))
                    .rounded_full()
                    .children(avatar.map(|picture| img(picture).size(px(40.)).rounded_full())),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .child(
                        div()
                            .h(px(22.))
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(shown.author.display_name().to_owned()),
                    )
                    .children(tag)
                    .child(
                        div()
                            .ml(px(4.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .text_size(px(12.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(header_time(shown.local_time(), chrono::Local::now())),
                    ),
            )
            .child(self.content(shown));
        div()
            .mb(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(Self::report_heading("Selected Message", 500.))
            .child(
                div()
                    .relative()
                    .max_h(px(100.))
                    .flex()
                    .flex_col()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .child(
                        div()
                            .id("report-preview")
                            .track_scroll(&handle)
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .py(px(16.))
                            .pr(px(24.))
                            .child(wrapper),
                    )
                    .child(super::menu::menu_scrollbar(handle.clone())),
            )
            .into_any_element()
    }

    /// `userContainer_SQw-pl` under "Selected User".
    fn report_user_preview(&self, user: Id) -> AnyElement {
        let known = self.stores.users.get(&user).cloned();
        let picture = known
            .as_ref()
            .and_then(|known| self.images.get(&avatar_source(known).0).cloned());
        div()
            .mb(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(Self::report_heading("Selected User", 500.))
            .child(
                div()
                    .p(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div().size(px(40.)).rounded_full().children(
                            picture.map(|picture| img(picture).size(px(40.)).rounded_full()),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(
                                        known
                                            .as_ref()
                                            .map(|known| known.display_name().to_owned())
                                            .unwrap_or_default(),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(known.map(|known| known.username).unwrap_or_default()),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `linksContainer_nlOah7`: "Learn More" over a row that opens the link.
    fn report_link(&self, data: &Value, cx: &mut Context<Self>) -> AnyElement {
        let text = data["link_text"].as_str().unwrap_or_default().to_owned();
        let url = data["url"].as_str().unwrap_or_default().to_owned();
        let hidden = data["is_header_hidden"] == true;
        let label = text.clone();
        div()
            .flex()
            .flex_col()
            .when(!hidden, |this| {
                this.child(Self::report_heading("Learn More", 500.).mb(px(8.)))
            })
            .child(
                div()
                    .id(SharedString::from(format!("report-link-{url}")))
                    .min_h(px(64.))
                    .pl(px(16.))
                    .pr(px(16.))
                    .rounded(px(12.))
                    .bg(color("--background-mod-subtle"))
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .my(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(text),
                    )
                    .child(icon("report_link_arrow", 24., "--icon-subtle"))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.follow_link(&url, &label, cx)),
                    ),
            )
            .into_any_element()
    }

    /// A checkbox element: `[key, label, description]` rows.
    fn report_checkboxes(&self, element: &Value, cx: &mut Context<Self>) -> AnyElement {
        let name = element["name"].as_str().unwrap_or_default().to_owned();
        let checked: Vec<String> = self
            .report
            .as_ref()
            .map(|open| {
                open.checked
                    .iter()
                    .filter(|(field, _, _)| *field == name)
                    .map(|(_, key, _)| key.clone())
                    .collect()
            })
            .unwrap_or_default();
        let rows: Vec<Value> = element["data"].as_array().cloned().unwrap_or_default();
        let count = rows.len();
        div()
            .flex()
            .flex_col()
            .children(rows.into_iter().enumerate().map(|(index, row)| {
                let key = row[0].as_str().unwrap_or_default().to_owned();
                let label = row[1].as_str().unwrap_or_default().to_owned();
                let description = row[2].as_str().map(str::to_owned);
                let on = checked.contains(&key);
                let field = name.clone();
                let (toggle_key, toggle_label) = (key.clone(), label.clone());
                div()
                    .id(SharedString::from(format!("report-check-{key}")))
                    .when(index + 1 < count, |this| this.mb(px(16.)))
                    .relative()
                    .pl(px(32.))
                    .cursor_pointer()
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(0.))
                            .size(px(20.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if on {
                                color("--checkbox-border-selected-default")
                            } else {
                                color("--checkbox-border-default")
                            })
                            .bg(if on {
                                color("--checkbox-background-selected-default")
                            } else {
                                color("--checkbox-background-default")
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(on, |this| {
                                this.child(icon("checkbox_check", 18., "--white"))
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .children(description.map(|description| {
                        div()
                            .mt(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(description)
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this.report.as_mut() {
                            if let Some(index) = open
                                .checked
                                .iter()
                                .position(|(name, key, _)| *name == field && *key == toggle_key)
                            {
                                open.checked.remove(index);
                            } else {
                                open.checked.push((
                                    field.clone(),
                                    toggle_key.clone(),
                                    toggle_label.clone(),
                                ));
                            }
                        }
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }

    /// `childButton_anhLH-` rows: one list, separated by `--border-muted`, its ends rounded.
    fn report_children(&self, children: &[Value], cx: &mut Context<Self>) -> AnyElement {
        let count = children.len();
        div()
            .flex()
            .flex_col()
            .children(children.iter().enumerate().map(|(index, child)| {
                let label = child[0].as_str().unwrap_or_default().to_owned();
                let target = child[1].as_u64().unwrap_or(0);
                let text = label.clone();
                div()
                    .id(SharedString::from(format!("report-option-{target}")))
                    .bg(color("--background-mod-subtle"))
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .when(index == 0, |this| this.rounded_t(px(12.)))
                    .when(index + 1 == count, |this| this.rounded_b(px(12.)))
                    .when(index + 1 < count, |this| {
                        this.border_b_1().border_color(color("--border-muted"))
                    })
                    .cursor_pointer()
                    .pl(px(16.))
                    .pr(px(16.))
                    .flex()
                    .items_center()
                    // `childContent_anhLH-` is 328 wide: 8px before the 24px chevron (design/dom/report/step1).
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .my(px(8.))
                            .py(px(12.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(text),
                    )
                    .child(icon("report_chevron", 24., "--icon-subtle"))
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.report_go(label.clone(), target, cx)
                        }),
                    )
            }))
            .into_any_element()
    }

    /// `ei`: what each step chose, a dot per line on a 2px rail.
    fn report_breadcrumbs(&self) -> Option<AnyElement> {
        let open = self.report.as_ref()?;
        let lines: Vec<String> = open
            .history
            .iter()
            .flat_map(|step| {
                let boxes: Vec<&str> = step
                    .checked
                    .iter()
                    .map(|(_, _, label)| label.as_str())
                    .collect();
                [
                    (!boxes.is_empty()).then(|| boxes.join(", ")),
                    (!step.label.is_empty()).then(|| step.label.clone()),
                ]
            })
            .flatten()
            .collect();
        if lines.is_empty() {
            return None;
        }
        let count = lines.len();
        Some(
            div()
                .mb(px(8.))
                .flex()
                .flex_col()
                .child(Self::report_heading("Report Category", 600.).mb(px(8.)))
                .children(lines.into_iter().enumerate().map(|(index, line)| {
                    div()
                        .relative()
                        .h(px(20.))
                        .when(index + 1 < count, |this| this.mb(px(8.)))
                        .flex()
                        .items_center()
                        .when(index + 1 < count, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .left(px(1.))
                                    .top(px(10.))
                                    .w(px(2.))
                                    .h(px(24.))
                                    .bg(color("--background-base-lowest")),
                            )
                        })
                        .child(
                            div()
                                .size(px(4.))
                                .rounded(px(2.))
                                .bg(color("--background-brand")),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(line),
                        )
                }))
                .into_any_element(),
        )
    }

    /// `J.PQ` rows under "More you can do": Ignore and Block the reported user.
    fn report_offers(
        &self,
        current: &Value,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ignore = element(current, "ignore_users").is_some();
        let block = element(current, "block_users").is_some();
        if !ignore && !block {
            return None;
        }
        let name = self
            .stores
            .users
            .get(&user)
            .map(|known| known.display_name().to_owned())
            .unwrap_or_default();
        let blocked = self.is_blocked(user);
        let ignored = self.stores.is_ignored(user);
        let row = |id: &'static str,
                   title: String,
                   line: &'static str,
                   button: &'static str,
                   done: bool,
                   critical: bool| {
            div()
                .px(px(16.))
                .py(px(16.))
                .flex()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-subtle"))
                                .child(line),
                        ),
                )
                .child(
                    div()
                        .id(id)
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color(if critical {
                            "--control-critical-primary-border-default"
                        } else {
                            "--control-secondary-border-default"
                        }))
                        .bg(color(if critical {
                            "--control-critical-primary-background-default"
                        } else {
                            "--control-secondary-background-default"
                        }))
                        .when(done, |this| this.opacity(0.5))
                        .when(!done, |this| this.cursor_pointer())
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color(if critical {
                            "--white"
                        } else {
                            "--control-secondary-text-default"
                        }))
                        .child(button),
                )
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        if ignore {
            let done = ignored || blocked;
            rows.push(
                row(
                    "report-ignore",
                    format!("Ignore {name}"),
                    "Get space without letting them know",
                    if done { "Ignored" } else { "Ignore" },
                    done,
                    false,
                )
                .when(!done, |this| {
                    this.on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.ignore_user(user, cx);
                        }),
                    )
                })
                .into_any_element(),
            );
        }
        if block {
            rows.push(
                row(
                    "report-block",
                    format!("Block {name}"),
                    "Stop seeing their messages",
                    if blocked { "Blocked" } else { "Block" },
                    blocked,
                    true,
                )
                .when(!blocked, |this| {
                    this.on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.block_user(user, cx);
                        }),
                    )
                })
                .into_any_element(),
            );
        }
        let count = rows.len();
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child("More you can do"),
                )
                .child(
                    div()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-muted"))
                        .bg(color("--background-base-lower"))
                        .flex()
                        .flex_col()
                        .children(rows.into_iter().enumerate().map(|(index, row)| {
                            div()
                                .when(index + 1 < count, |this| {
                                    this.border_b_1().border_color(color("--border-muted"))
                                })
                                .child(row)
                        })),
                )
                .into_any_element(),
        )
    }

    fn report_button(id: &'static str, tone: &str) -> gpui::Stateful<gpui::Div> {
        let (background, border, text) = match tone {
            "secondary" => (
                "--control-secondary-background-default",
                "--control-secondary-border-default",
                "--control-secondary-text-default",
            ),
            "critical" => (
                "--control-critical-primary-background-default",
                "--control-critical-primary-border-default",
                "--white",
            ),
            _ => (
                "--control-primary-background-default",
                "--control-primary-border-default",
                "--white",
            ),
        };
        div()
            .id(id)
            .flex_1()
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(border))
            .bg(color(background))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color(text))
    }

    pub(super) fn report_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.report.as_ref()?;
        let menu = open.menu.as_ref()?;
        let current = node(menu, open.node).clone();
        let user = open.user;
        let scroll = open.scroll.clone();
        let header = current["header"].as_str().unwrap_or_default().to_owned();
        let subheader = current["subheader"].as_str().map(str::to_owned);
        let button = current["button"].clone();
        let kind = button["type"].as_str().unwrap_or_default().to_owned();
        let target = button["target"].as_u64();
        let submit = kind == "submit";
        // `L`: Back while there is history (not on done); the node's button unless it is cancel.
        let back = !open.history.is_empty() && kind != "done";
        let forward = !button.is_null() && kind != "cancel";
        let required = current["is_multi_select_required"] == true;
        let checkbox = element(&current, "checkbox").cloned();
        let blocked_next = open.submitting || (required && open.checked.is_empty());
        let children: Vec<Value> = current["children"].as_array().cloned().unwrap_or_default();
        let mut body: Vec<AnyElement> = Vec::new();
        if let Some(art) =
            element(&current, "success").and_then(|_| self.images.get(SUCCESS_ART).cloned())
        {
            body.push(
                div()
                    .flex()
                    .justify_center()
                    .child(img(art).w(px(288.)).h(px(162.)))
                    .into_any_element(),
            );
        }
        if element(&current, "user_preview").is_some() {
            body.push(self.report_user_preview(user));
        }
        if element(&current, "message_preview").is_some()
            && let Some(message) = open.message.as_ref()
        {
            body.push(self.report_message_preview(message, cx));
        }
        if let Some(checkbox) = checkbox.as_ref() {
            body.push(self.report_checkboxes(checkbox, cx));
        }
        if !children.is_empty() {
            body.push(self.report_children(&children, cx));
        }
        for link in current["elements"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|element| element["type"] == "external_link")
        {
            // `skip_if_unlocalized`: a link the server could not translate stays out.
            if link["skip_if_unlocalized"] == true && link["is_localized"] != true {
                continue;
            }
            body.push(self.report_link(&link["data"], cx));
        }
        if element(&current, "breadcrumbs").is_some() {
            body.extend(self.report_breadcrumbs());
        }
        body.extend(self.report_offers(&current, user, cx));
        if let Some(error) = open.error.clone() {
            body.push(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error)
                    .into_any_element(),
            );
        }
        let label = match kind.as_str() {
            "submit" => "Submit Report",
            "next" => "Next",
            "cancel" => "Cancel",
            _ => "Done",
        };
        let tone = match kind.as_str() {
            "submit" => "critical",
            "cancel" => "secondary",
            _ => "primary",
        };
        let footer = (back || forward).then(|| {
            div()
                .flex_none()
                .w(px(440.))
                .p(px(24.))
                .bg(color("--background-surface-high"))
                .rounded_b(px(12.))
                .flex()
                .gap(px(8.))
                .when(back, |this| {
                    this.child(
                        Self::report_button("report-back", "secondary")
                            .when(open.submitting, |this| this.opacity(0.5))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.report_back(cx)))
                            .child("Back"),
                    )
                })
                .when(forward, |this| {
                    let kind = kind.clone();
                    this.child(
                        Self::report_button("report-next", tone)
                            .when(blocked_next, |this| this.opacity(0.5))
                            .when(!blocked_next, |this| this.cursor_pointer())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if blocked_next {
                                    return;
                                }
                                match kind.as_str() {
                                    "next" => {
                                        if let Some(target) = target {
                                            this.report_go(String::new(), target, cx);
                                        }
                                    }
                                    "submit" => this.report_submit(cx),
                                    _ => {
                                        this.report_closed(cx);
                                        cx.notify();
                                    }
                                }
                            }))
                            .child(label),
                    )
                })
        });
        let modal = div()
            .parity_with_border("report-modal", 1.)
            .id("report-modal")
            .occlude()
            .w(px(444.))
            // The container's `max-height: 504px` plus the 1px ring (design/dom/report/step1: 107 to 613).
            .max_h(px(506.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-normal"))
            .bg(color("--background-surface-high"))
            .on_click(|_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .flex_none()
                    .pt(px(24.))
                    .px(px(24.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .pr(px(32.))
                            .child(header),
                    )
                    .child(
                        div()
                            .id("report-close")
                            .absolute()
                            .top(px(8.))
                            .right(px(9.))
                            .size(px(40.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.report_closed(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(24.))
                    .children(subheader.map(|subheader| {
                        div()
                            .mt(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-subtle"))
                            .child(subheader)
                    }))
                    .when(submit, |this| {
                        // `Q0tSKT` under a submit node's subheader.
                        this.child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(self.report_markdown(
                                    "By submitting this report you confirm that it is truthful and made in good faith. Please follow our \
                                     [Community Guidelines](https://discord.com/guidelines) and do not submit false or duplicate \
                                     reports.",
                                    cx,
                                )),
                        )
                    }),
            )
            .child(
                // `content__KoJTP`: 24px above and left, 16px right plus the 8px scrollbar gutter, 16px between.
                // The 440px column sits in the 442px ring, so the thumb is 6px left of its edge at 859.
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .w(px(440.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("report-content")
                            .track_scroll(&scroll)
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .pt(px(24.))
                            .pl(px(24.))
                            .pr(px(24.))
                            .when(!back && !forward, |this| this.pb(px(24.)))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .children(body),
                    )
                    .child(super::menu::menu_scrollbar(scroll.clone())),
            )
            .children(footer);
        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).
        let pose = crate::ui::motion::legacy_modal_pose("report", "report-layer", false);
        Some(
            deferred(
                div()
                    .id("report-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.report_closed(cx);
                        cx.notify();
                    }))
                    .child(modal.posed(pose)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Value {
        json!({"version": "1.0", "variant": "4", "root_node_id": 63, "success_node_id": 69, "nodes": {}})
    }

    #[test]
    fn the_body_lists_every_step_and_the_ticked_keys() {
        let open = Report {
            user: 7,
            guild: None,
            message: None,
            subject: None,
            menu: Some(menu()),
            node: 25,
            history: vec![
                Step {
                    node: 63,
                    label: String::new(),
                    checked: Vec::new(),
                },
                Step {
                    node: 22,
                    label: String::new(),
                    checked: vec![(
                        "user_profile_select".into(),
                        "name".into(),
                        "Their name".into(),
                    )],
                },
                Step {
                    node: 19,
                    label: "Spam".into(),
                    checked: Vec::new(),
                },
            ],
            checked: Vec::new(),
            submitting: false,
            error: None,
            scroll: ScrollHandle::new(),
            preview_scroll: ScrollHandle::new(),
        };
        let body = Root::report_body(&open, &menu(), 69);
        assert_eq!(body["breadcrumbs"], json!([63, 22, 19, 25]));
        assert_eq!(body["elements"], json!({"user_profile_select": ["name"]}));
        assert_eq!(body["name"], "user");
        assert_eq!(body["user_id"], "7");
        assert_eq!(body["language"], "en");
        assert!(body.get("guild_id").is_none());
    }

    #[test]
    fn a_message_report_carries_its_channel_and_message_and_posts_to_its_own_route() {
        let open = Report {
            user: 7,
            guild: Some(3),
            message: Some(ReportedMessage {
                name: "first_dm",
                channel: 11,
                id: 12,
                message: None,
            }),
            subject: None,
            menu: Some(menu()),
            node: 63,
            history: Vec::new(),
            checked: Vec::new(),
            submitting: false,
            error: None,
            scroll: ScrollHandle::new(),
            preview_scroll: ScrollHandle::new(),
        };
        let body = Root::report_body(&open, &menu(), 69);
        assert_eq!(body["name"], "first_dm");
        assert_eq!(body["channel_id"], "11");
        assert_eq!(body["message_id"], "12");
        assert!(body.get("guild_id").is_none());
        assert!(body.get("user_id").is_none());
    }

    fn subject_report(subject: Subject) -> Report {
        Report {
            user: 0,
            guild: Some(3),
            message: None,
            subject: Some(subject),
            menu: Some(menu()),
            node: 63,
            history: Vec::new(),
            checked: Vec::new(),
            submitting: false,
            error: None,
            scroll: ScrollHandle::new(),
            preview_scroll: ScrollHandle::new(),
        }
    }

    #[test]
    fn a_widget_report_names_its_owner_and_widget_only() {
        let open = subject_report(Subject::Widget {
            user: 7,
            widget: 11,
        });
        let body = Root::report_body(&open, &menu(), 69);
        assert_eq!(body["name"], "widget");
        assert_eq!(body["user_id"], "7");
        assert_eq!(body["widget_id"], "11");
        assert!(body.get("guild_id").is_none());
        assert!(body.get("channel_id").is_none());
    }

    #[test]
    fn a_stage_report_names_its_instance_channel_and_guild() {
        let open = subject_report(Subject::Stage {
            instance: 5,
            channel: 6,
            guild: 3,
        });
        let body = Root::report_body(&open, &menu(), 69);
        assert_eq!(body["name"], "stage_channel");
        assert_eq!(body["stage_instance_id"], "5");
        assert_eq!(body["channel_id"], "6");
        assert_eq!(body["guild_id"], "3");
        assert!(body.get("user_id").is_none());
    }

    #[test]
    fn an_event_report_names_its_guild_and_event() {
        let open = subject_report(Subject::Event { guild: 3, event: 9 });
        let body = Root::report_body(&open, &menu(), 69);
        assert_eq!(body["name"], "guild_scheduled_event");
        assert_eq!(body["guild_id"], "3");
        assert_eq!(body["guild_scheduled_event_id"], "9");
        assert!(body.get("user_id").is_none());
        assert!(body.get("channel_id").is_none());
    }

    #[test]
    fn an_app_report_carries_the_entrypoint_and_the_context_it_was_opened_from() {
        let open = subject_report(Subject::App {
            app: 5,
            guild: Some(3),
            channel: None,
            entrypoint: "app_directory",
        });
        let body = Root::report_body(&open, &menu(), 69);
        assert_eq!(body["name"], "application");
        assert_eq!(body["application_id"], "5");
        assert_eq!(body["guild_id"], "3");
        assert_eq!(body["entrypoint"], "app_directory");
        assert!(body.get("channel_id").is_none());
        assert!(body.get("user_id").is_none());
    }
}
