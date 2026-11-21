//! Message Apps & User Apps context submenu (module 173500):
//! - Command index fetch (`APPLICATION_COMMAND_INDEX_GUILD`/`_CHANNEL`) with 3-dot loading state (`loadingWrapper_trRdNX`).
//! - Search input (`m1UwbP` "Search commands") filtering commands by command name / application name (limit 10).
//! - "Frequently Used Commands" (`V0w2ap`) group backed by persisted command frecency (`CommandFrecency`).
//! - "Apps" (`PHjkRE`) group with per-application nested submenus (20x20 app icon + caret -> level-2 command list).
//! - Scrollable container capped at `max-height: 500px` (`list_wAHdND`).
//! - Bot user footer (`700210`): "Manage Server Integration" (`IuSJT8`) when user has `MANAGE_GUILD`.
//! - Execution via `POST /interactions` (type 2, data.type 2 USER / 3 MESSAGE) recording frecency.

use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Bounds, Context, Focusable, FontWeight, IntoElement, Pixels, SharedString, Window,
    deferred, div, img, prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Local storage key for command frecency (`id -> { count, last_used_ms }`).
const FRECENCY_STORAGE_KEY: &str = "CommandFrecency";
/// Official search result cap (`173500`: `limit: 10`).
const SEARCH_LIMIT: usize = 10;
/// Maximum frecency rows shown in "Frequently Used Commands" (`V0w2ap`).
const FRECENCY_LIMIT: usize = 5;
/// `MANAGE_GUILD` permission bit (`1 << 5`).
const MANAGE_GUILD: u64 = 1 << 5;

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandEntry {
    command: Value,
    application: Option<Value>,
    id: String,
    name: String,
    app_id: String,
    app_name: String,
    app_icon: Option<String>,
}

#[derive(Clone, Debug)]
struct AppSection {
    app_id: String,
    app_name: String,
    app_icon: Option<String>,
    commands: Vec<CommandEntry>,
}

#[derive(Clone, Copy, Debug)]
enum AppTarget {
    Message { channel: Id, message: Id },
    User { user: Id, guild: Option<Id> },
}

fn load_frecency() -> BTreeMap<String, (u32, i64)> {
    let Some(val) = crate::storage::get(FRECENCY_STORAGE_KEY) else {
        return BTreeMap::new();
    };
    let Some(map) = val.as_object() else {
        return BTreeMap::new();
    };
    map.iter()
        .filter_map(|(k, v)| {
            let count = v.get("count").and_then(Value::as_u64)? as u32;
            let last = v.get("last_used_ms").and_then(Value::as_i64).unwrap_or(0);
            Some((k.clone(), (count, last)))
        })
        .collect()
}

pub(super) fn record_command_use(command_id: &str) {
    if command_id.is_empty() {
        return;
    }
    let mut map = load_frecency();
    let now = chrono::Utc::now().timestamp_millis();
    let entry = map.entry(command_id.to_owned()).or_insert((0, 0));
    entry.0 = entry.0.saturating_add(1);
    entry.1 = now;
    let obj: serde_json::Map<String, Value> = map
        .into_iter()
        .map(|(k, (count, last_used_ms))| {
            (k, json!({ "count": count, "last_used_ms": last_used_ms }))
        })
        .collect();
    crate::storage::set(FRECENCY_STORAGE_KEY, Value::Object(obj));
}

fn command_entry(command: Value, application: Option<Value>) -> CommandEntry {
    let id = command["id"].as_str().unwrap_or_default().to_owned();
    let name = command["name"].as_str().unwrap_or_default().to_owned();
    let app_id = command["application_id"]
        .as_str()
        .or_else(|| application.as_ref().and_then(|a| a["id"].as_str()))
        .unwrap_or_default()
        .to_owned();
    let app_name = application
        .as_ref()
        .and_then(|a| a["name"].as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("App")
        .to_owned();
    let app_icon = application
        .as_ref()
        .and_then(|a| a["icon"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    CommandEntry {
        command,
        application,
        id,
        name,
        app_id,
        app_name,
        app_icon,
    }
}

/// Scores and filters commands by command name or application name (`173500` search mode, capped at `limit`).
fn filter_commands(entries: &[CommandEntry], query: &str, limit: usize) -> Vec<CommandEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return entries.iter().take(limit).cloned().collect();
    }
    let mut scored: Vec<(u8, &CommandEntry)> = entries
        .iter()
        .filter_map(|entry| {
            let cmd_low = entry.name.to_lowercase();
            let app_low = entry.app_name.to_lowercase();
            let score = if cmd_low == q {
                0
            } else if cmd_low.starts_with(&q) {
                1
            } else if cmd_low.contains(&q) {
                2
            } else if app_low.starts_with(&q) {
                3
            } else if app_low.contains(&q) {
                4
            } else {
                return None;
            };
            Some((score, entry))
        })
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| sa.cmp(sb).then_with(|| a.name.cmp(&b.name)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry.clone())
        .collect()
}

/// Top frecent commands matching `entries` using `frecency` scores (`count` desc, `last_used_ms` desc).
fn frecent_commands(
    entries: &[CommandEntry],
    frecency: &BTreeMap<String, (u32, i64)>,
    limit: usize,
) -> Vec<CommandEntry> {
    let mut used: Vec<((u32, i64), &CommandEntry)> = entries
        .iter()
        .filter_map(|entry| {
            let score = frecency.get(&entry.id).copied()?;
            if score.0 == 0 {
                return None;
            }
            Some((score, entry))
        })
        .collect();
    used.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| a.name.cmp(&b.name)));
    used.into_iter()
        .take(limit)
        .map(|(_, entry)| entry.clone())
        .collect()
}

/// Groups commands into per-application sections sorted by application name (`173500` Apps group).
fn group_by_application(entries: &[CommandEntry]) -> Vec<AppSection> {
    let mut by_app: BTreeMap<String, AppSection> = BTreeMap::new();
    for entry in entries {
        let key = if entry.app_id.is_empty() {
            entry.app_name.clone()
        } else {
            entry.app_id.clone()
        };
        let section = by_app.entry(key).or_insert_with(|| AppSection {
            app_id: entry.app_id.clone(),
            app_name: entry.app_name.clone(),
            app_icon: entry.app_icon.clone(),
            commands: Vec::new(),
        });
        section.commands.push(entry.clone());
    }
    let mut sections: Vec<AppSection> = by_app.into_values().collect();
    sections.sort_by_key(|a| a.app_name.to_lowercase());
    sections
}

impl Root {
    pub(super) fn command_index_key(&self) -> Option<String> {
        match (self.guild, self.channel) {
            (Some(guild), _) => Some(format!("command-index:guild:{guild}")),
            (None, Some(channel)) => Some(format!("command-index:channel:{channel}")),
            _ => None,
        }
    }

    pub(super) fn command_index_value(&self) -> Option<&Value> {
        self.command_index_key()
            .and_then(|key| self.fetched.get(&key))
    }

    pub(super) fn command_index_loading(&self) -> bool {
        let Some(key) = self.command_index_key() else {
            return false;
        };
        self.requested.contains(&key) && !self.fetched.contains_key(&key)
    }

    pub(super) fn request_command_index(&mut self) {
        let Some(key) = self.command_index_key() else {
            return;
        };
        if !self.requested.insert(key.clone()) {
            return;
        }
        let route = match (self.guild, self.channel) {
            (Some(guild), _) => format!("/guilds/{guild}/application-command-index"),
            (None, Some(channel)) => format!("/channels/{channel}/application-command-index"),
            _ => return,
        };
        let _ = self.requests.send(Request::Fetch { key, route });
    }

    /// Ensures the shared "Search commands" (`m1UwbP`) editor exists for the Apps submenu (`173500`).
    pub(super) fn ensure_apps_search(&mut self, cx: &mut Context<Self>) {
        if self.context.apps_search.is_some() {
            return;
        }
        let input = cx.new(|cx| {
            let mut style = editor_style();
            style.line_height = 18.;
            let mut editor = Editor::new(style, cx);
            editor.set_keeps_text(true);
            editor.set_reports_all_edits(true);
            editor.set_single_line(true);
            editor.set_placeholder(dc_intl::t("m1UwbP").to_owned());
            editor
        });
        let sub = cx.subscribe(&input, |this, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                this.context.apps_active_section = None;
                cx.notify();
            }
        });
        self.context.apps_search = Some(input);
        self.context.apps_search_sub = Some(sub);
    }

    pub(super) fn clear_apps_submenu_state(&mut self, cx: &mut Context<Self>) {
        self.context.apps_active_section = None;
        self.context.apps_active_bounds = None;
        if let Some(editor) = self.context.apps_search.clone() {
            editor.update(cx, |ed, cx| {
                if !ed.text().is_empty() {
                    ed.set_text("");
                    cx.notify();
                }
            });
        }
    }

    /// Message commands (`type: 3`) in the loaded index, with their application.
    fn message_commands(&self) -> Vec<(Value, Option<Value>)> {
        self.commands_of_type(3)
    }

    /// User commands (`type: 2`) in the loaded index: the user menu's Apps submenu (module 173500 with
    /// `commandTypes: [USER]`).
    pub(super) fn user_commands(&self) -> Vec<(Value, Option<Value>)> {
        self.commands_of_type(2)
    }

    /// A user command run on `user` (`executeCommand` of module 545152 with `commandTargetId`): `POST /interactions`
    /// type 2 whose data is the command, type 2, aimed at the user.
    pub(super) fn run_user_command(&mut self, command: &Value, user: Id) {
        if let Some(id) = command["id"].as_str() {
            record_command_use(id);
        }
        let session = self.stores.session_id.clone().unwrap_or_default();
        let Some(channel) = self.channel else {
            return;
        };
        let body = json!({
            "type": 2,
            "application_id": command["application_id"],
            "guild_id": self.guild.map(|guild| guild.to_string()),
            "channel_id": channel.to_string(),
            "session_id": session,
            "data": {
                "version": command["version"],
                "id": command["id"],
                "name": command["name"],
                "type": 2,
                "options": [],
                "application_command": command,
                "attachments": [],
                "target_id": user.to_string(),
            },
            "nonce": crate::net::nonce(),
            "analytics_location": "user_context_menu",
        });
        let _ = self.requests.send(Request::Interaction { body });
    }

    fn run_message_command(&mut self, command: &Value, channel: Id, message: Id) {
        if let Some(id) = command["id"].as_str() {
            record_command_use(id);
        }
        let session = self.stores.session_id.clone().unwrap_or_default();
        let body = json!({
            "type": 2,
            "application_id": command["application_id"],
            "guild_id": self.guild.map(|guild| guild.to_string()),
            "channel_id": channel.to_string(),
            "session_id": session,
            "data": {
                "version": command["version"],
                "id": command["id"],
                "name": command["name"],
                "type": 3,
                "options": [],
                "application_command": command,
                "attachments": [],
                "target_id": message.to_string(),
            },
            "nonce": crate::net::nonce(),
            "analytics_location": "message_context_menu",
        });
        let _ = self.requests.send(Request::Interaction { body });
    }

    fn commands_of_type(&self, kind: u64) -> Vec<(Value, Option<Value>)> {
        let Some(index) = self
            .command_index_key()
            .and_then(|key| self.fetched.get(&key))
        else {
            return Vec::new();
        };
        let applications = index["applications"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        index["application_commands"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|command| command["type"].as_u64() == Some(kind))
            .map(|command| {
                let application = applications
                    .iter()
                    .find(|application| application["id"] == command["application_id"])
                    .cloned();
                (command.clone(), application)
            })
            .collect()
    }

    fn app_icon_element(&self, app_id: &str, app_icon: Option<&str>) -> Option<AnyElement> {
        let hash = app_icon?;
        let picture = self
            .images
            .get(&format!("app-icon:{app_id}:{hash}"))
            .cloned()?;
        Some(
            img(picture)
                .size(px(20.))
                .rounded_full()
                .flex_none()
                .into_any_element(),
        )
    }

    fn apps_command_row(
        &self,
        id: SharedString,
        entry: CommandEntry,
        show_app_icon: bool,
        target: AppTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let leading = if show_app_icon {
            self.app_icon_element(&entry.app_id, entry.app_icon.as_deref())
        } else {
            None
        };
        let command = entry.command;
        div()
            .id(id)
            .h(px(34.))
            .px(px(8.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .flex()
            .items_center()
            .gap(px(8.))
            .children(leading)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(entry.name),
            )
            .child(
                div()
                    .flex_none()
                    .child(icon("send_message", 16., "--interactive-icon-default")),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.menu = None;
                this.submenu = None;
                this.close_context_menu(cx);
                this.clear_apps_submenu_state(cx);
                match target {
                    AppTarget::Message { channel, message } => {
                        this.run_message_command(&command, channel, message);
                    }
                    AppTarget::User { user, .. } => {
                        this.run_user_command(&command, user);
                    }
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn apps_loading_row() -> AnyElement {
        // `loadingWrapper_trRdNX`: height 16px, margin 2px 0, padding 8px 4px, 3 dots (4px radius = 8px dia).
        div()
            .my(px(2.))
            .px(px(4.))
            .py(px(8.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(color("--text-muted"))
                    .opacity(0.4),
            )
            .child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(color("--text-muted"))
                    .opacity(0.7),
            )
            .child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(color("--text-muted"))
                    .opacity(1.0),
            )
            .into_any_element()
    }

    fn apps_section_header(label: &str) -> AnyElement {
        div()
            .px(px(8.))
            .pt(px(6.))
            .pb(px(4.))
            .text_size(px(11.))
            .line_height(px(14.))
            .font_weight(FontWeight(700.))
            .text_color(color("--text-muted"))
            .child(label.to_uppercase())
            .into_any_element()
    }

    fn apps_separator() -> AnyElement {
        div()
            .my(px(4.))
            .mx(px(8.))
            .h(px(1.))
            .bg(color("--border-subtle"))
            .into_any_element()
    }

    /// Module 173500 submenu panel for Message commands (`menu.rs`).
    pub(super) fn apps_message_submenu(
        &self,
        channel: Id,
        message: Id,
        anchor: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entries: Vec<CommandEntry> = self
            .message_commands()
            .into_iter()
            .map(|(cmd, app)| command_entry(cmd, app))
            .collect();
        self.apps_submenu_panel(
            entries,
            AppTarget::Message { channel, message },
            anchor,
            window,
            cx,
        )
    }

    /// Module 173500 submenu panel for User commands (`context_menu.rs`).
    pub(super) fn apps_user_submenu(
        &self,
        user: Id,
        guild: Option<Id>,
        anchor: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entries: Vec<CommandEntry> = self
            .user_commands()
            .into_iter()
            .map(|(cmd, app)| command_entry(cmd, app))
            .collect();
        self.apps_submenu_panel(entries, AppTarget::User { user, guild }, anchor, window, cx)
    }

    fn apps_submenu_panel(
        &self,
        entries: Vec<CommandEntry>,
        target: AppTarget,
        anchor: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loading = self.command_index_loading();
        let query = self
            .context
            .apps_search
            .as_ref()
            .map(|ed| ed.read(cx).text().to_owned())
            .unwrap_or_default();
        let searching = !query.trim().is_empty();
        let width: f32 = 228.;
        let viewport = window.viewport_size();
        let right_side = f32::from(anchor.right()) + 4. + width <= f32::from(viewport.width) - 8.;
        let left = if right_side {
            f32::from(anchor.right()) + 4.
        } else {
            (f32::from(anchor.left()) - 4. - width).max(8.)
        };
        let top = (f32::from(anchor.top()) - 9.)
            .min((f32::from(viewport.height) - 240.).max(8.))
            .max(8.);

        let mut rows: Vec<AnyElement> = Vec::new();
        let mut active_section: Option<AppSection> = None;

        // Show search box whenever commands exist or user is actively searching (173500).
        if !loading && (!entries.is_empty() || searching) {
            let search_bar: AnyElement = if let Some(editor) = self.context.apps_search.clone() {
                let focus_ed = editor.clone();
                div()
                    .id("apps-command-search")
                    .mb(px(6.))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .bg(color("--input-background-default"))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .flex()
                    .items_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |_, _, window, cx| {
                            window.focus(&focus_ed.read(cx).focus_handle(cx), cx);
                            cx.stop_propagation();
                        }),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(editor))
                    .into_any_element()
            } else {
                div()
                    .id("apps-command-search-init")
                    .mb(px(6.))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .bg(color("--input-background-default"))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .flex()
                    .items_center()
                    .cursor_text()
                    .text_size(px(13.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::t("m1UwbP"))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.ensure_apps_search(cx);
                            if let Some(ed) = this.context.apps_search.clone() {
                                window.focus(&ed.read(cx).focus_handle(cx), cx);
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            };
            rows.push(search_bar);
        }

        if loading {
            rows.push(Self::apps_loading_row());
        } else if searching {
            let matches = filter_commands(&entries, &query, SEARCH_LIMIT);
            rows.push(Self::apps_separator());
            if matches.is_empty() {
                rows.push(
                    div()
                        .h(px(34.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .opacity(0.6)
                        .child(dc_intl::t("YSNlV2"))
                        .into_any_element(),
                );
            } else {
                for (index, entry) in matches.into_iter().enumerate() {
                    rows.push(self.apps_command_row(
                        SharedString::from(format!("apps-search-cmd-{index}")),
                        entry,
                        true,
                        target,
                        cx,
                    ));
                }
            }
        } else if entries.is_empty() {
            rows.push(
                div()
                    .h(px(34.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .text_color(color("--text-muted"))
                    .opacity(0.6)
                    .child(dc_intl::t("YSNlV2"))
                    .into_any_element(),
            );
        } else {
            let frecency = load_frecency();
            let top_used = frecent_commands(&entries, &frecency, FRECENCY_LIMIT);
            if !top_used.is_empty() {
                rows.push(Self::apps_section_header(dc_intl::t("V0w2ap")));
                for (index, entry) in top_used.into_iter().enumerate() {
                    rows.push(self.apps_command_row(
                        SharedString::from(format!("apps-frecent-cmd-{index}")),
                        entry,
                        true,
                        target,
                        cx,
                    ));
                }
                rows.push(Self::apps_separator());
            }
            let sections = group_by_application(&entries);
            rows.push(Self::apps_section_header(dc_intl::t("PHjkRE")));
            for (index, section) in sections.into_iter().enumerate() {
                let section_key = if section.app_id.is_empty() {
                    section.app_name.clone()
                } else {
                    section.app_id.clone()
                };
                let is_open =
                    self.context.apps_active_section.as_deref() == Some(section_key.as_str());
                let leading = self.app_icon_element(&section.app_id, section.app_icon.as_deref());
                let hover_key = format!("apps-section-{index}");
                let section_key_hover = section_key.clone();
                let hover_probe_key = hover_key.clone();
                let section_key_click = section_key.clone();
                let click_probe_key = hover_key.clone();
                let row = div()
                    .id(SharedString::from(hover_key.clone()))
                    .relative()
                    .h(px(34.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(is_open, |d| d.bg(color("--interactive-background-hover")))
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.anchor_probe(hover_key))
                    .children(leading)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(section.app_name.clone()),
                    )
                    .child(div().flex_none().child(icon(
                        "menu_caret",
                        16.,
                        "--interactive-icon-default",
                    )))
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        if !*inside {
                            return;
                        }
                        this.context.apps_active_section = Some(section_key_hover.clone());
                        this.context.apps_active_bounds = this
                            .tip_anchors
                            .borrow()
                            .get(hover_probe_key.as_str())
                            .copied();
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.context.apps_active_section = Some(section_key_click.clone());
                        this.context.apps_active_bounds = this
                            .tip_anchors
                            .borrow()
                            .get(click_probe_key.as_str())
                            .copied();
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .into_any_element();
                rows.push(row);
                if is_open {
                    active_section = Some(section);
                }
            }
        }

        // Module 700210 footer: for bot user targets when user has MANAGE_GUILD on current guild.
        if let AppTarget::User {
            user,
            guild: Some(guild_id),
        } = target
        {
            let is_bot = self.stores.users.get(&user).is_some_and(|known| known.bot);
            if is_bot && self.my_permission(guild_id, None, MANAGE_GUILD) {
                rows.push(Self::apps_separator());
                rows.push(
                    div()
                        .id("apps-manage-server-integration")
                        .h(px(34.))
                        .px(px(8.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("IuSJT8"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.clear_apps_submenu_state(cx);
                            this.open_server_settings_at(
                                guild_id,
                                super::server_settings::Page::Integrations,
                                cx,
                            );
                        }))
                        .into_any_element(),
                );
            }
        }

        // Level-2 nested per-app commands submenu when a section is hovered/opened.
        let nested: Option<AnyElement> = active_section.map(|section| {
            let n_width: f32 = 212.;
            let sec_anchor = self.context.apps_active_bounds.unwrap_or(anchor);
            let n_right = left + width + 4. + n_width <= f32::from(viewport.width) - 8.;
            let n_left = if n_right {
                left + width + 4.
            } else {
                (left - 4. - n_width).max(8.)
            };
            let n_top = (f32::from(sec_anchor.top()) - 8.)
                .min((f32::from(viewport.height) - 180.).max(8.))
                .max(8.);
            let nested_rows: Vec<AnyElement> = section
                .commands
                .into_iter()
                .enumerate()
                .map(|(idx, entry)| {
                    self.apps_command_row(
                        SharedString::from(format!("apps-nested-cmd-{idx}")),
                        entry,
                        false,
                        target,
                        cx,
                    )
                })
                .collect();
            div()
                .id("apps-nested-submenu")
                .occlude()
                .absolute()
                .left(px(n_left))
                .top(px(n_top))
                .w(px(n_width))
                .max_h(px(500.))
                .overflow_y_scroll()
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .p(px(8.))
                .flex()
                .flex_col()
                .children(nested_rows)
                .into_any_element()
        });

        deferred(
            div()
                .child(
                    div()
                        .id("submenu")
                        .child(self.anchor_probe("submenu"))
                        .occlude()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .max_h(px(500.))
                        .overflow_y_scroll()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.24),
                            offset: gpui::point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                .children(nested),
        )
        .with_priority(2)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<CommandEntry> {
        vec![
            command_entry(
                json!({"id": "1", "name": "Bookmark", "application_id": "100", "type": 3}),
                Some(json!({"id": "100", "name": "Keeper", "icon": "abc"})),
            ),
            command_entry(
                json!({"id": "2", "name": "Quote", "application_id": "100", "type": 3}),
                Some(json!({"id": "100", "name": "Keeper", "icon": "abc"})),
            ),
            command_entry(
                json!({"id": "3", "name": "Translate", "application_id": "200", "type": 3}),
                Some(json!({"id": "200", "name": "Polyglot"})),
            ),
        ]
    }

    #[test]
    fn filter_and_group_and_frecency_match_173500() {
        let entries = sample_entries();
        let filtered = filter_commands(&entries, "keep", SEARCH_LIMIT);
        assert_eq!(filtered.len(), 2);
        let exact = filter_commands(&entries, "translate", SEARCH_LIMIT);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].id, "3");

        let sections = group_by_application(&entries);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].app_name, "Keeper");
        assert_eq!(sections[0].commands.len(), 2);
        assert_eq!(sections[1].app_name, "Polyglot");
        assert_eq!(sections[1].commands.len(), 1);

        let frecency = BTreeMap::from([("3".to_owned(), (5, 200)), ("1".to_owned(), (2, 300))]);
        let top = frecent_commands(&entries, &frecency, FRECENCY_LIMIT);
        assert_eq!(
            top.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["3", "1"]
        );

        assert!(!dc_intl::t("m1UwbP").is_empty());
        assert!(!dc_intl::t("V0w2ap").is_empty());
        assert!(!dc_intl::t("PHjkRE").is_empty());
        assert!(!dc_intl::t("YSNlV2").is_empty());
        assert!(!dc_intl::t("IuSJT8").is_empty());
    }
}
