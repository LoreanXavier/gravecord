//! The Gravecord group of the settings sidebar: the master switch, the Plugins list and Backup & Restore. Not part of
//! the pinned client, so there is no capture to match: the pages are built from the settings layer's own pieces
//! (headings, rows, switches, radio dots, buttons, tokens) so they read as part of it. The plugins are in
//! `crate::plugins`.

use super::{ButtonKind, button, color, divider, mask_icon, render};
use crate::plugins::{self, Def, Kind, Report};
use crate::ui::Root;
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*, px};
use std::collections::HashSet;

/// The pages' own state, kept while settings is open.
#[derive(Default)]
pub(in crate::ui) struct State {
    /// The Plugins page's search box.
    search: Option<gpui::Entity<crate::editor::Editor>>,
    search_sub: Option<gpui::Subscription>,
    /// Plugins whose settings are unfolded.
    expanded: HashSet<&'static str>,
    /// What the last import did, or why it failed.
    pub(in crate::ui) outcome: Option<Result<Report, String>>,
    /// Where the last export was written.
    exported: Option<String>,
}

/// The largest backup read (the owner's Equicord backup is about 100 KB).
const MAX_BACKUP_BYTES: u64 = 16 * 1024 * 1024;

/// The settings button with a press: the button inside a box that takes the click.
fn pressable(id: &'static str, text: &'static str, kind: ButtonKind) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(format!("{id}-press")))
        .flex_none()
        .child(button(id, text, kind))
}

fn heading(text: &'static str) -> AnyElement {
    div()
        .pb(px(20.))
        .text_size(px(24.))
        .line_height(px(32.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
        .child(text)
        .into_any_element()
}

fn label(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
        .child(text.into())
}

fn subtle(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(color("--text-subtle"))
        .child(text.into())
}

/// A row: label and description on the left, the control at the end.
fn row(title: impl Into<SharedString>, description: &str, control: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(label(title))
                .when(!description.is_empty(), |this| {
                    this.child(subtle(description.to_owned()))
                }),
        )
        .child(control)
        .into_any_element()
}

/// The settings that the plugin search matches: name, description and setting labels, ignoring case.
fn matches(def: &Def, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let found = |text: &str| text.to_lowercase().contains(query);
    found(def.name)
        || found(def.description)
        || def
            .settings
            .iter()
            .any(|setting| found(setting.label) || found(setting.description))
}

/// The report of an import, in words.
fn report_lines(report: &Report) -> Vec<String> {
    let mut lines = vec![format!(
        "Imported {} plugin{} ({} turned on).",
        report.ported.len(),
        if report.ported.len() == 1 { "" } else { "s" },
        report.turned_on
    )];
    if !report.kept.is_empty() {
        lines.push(format!(
            "{} plugin{} it turns on {} not built in yet. Their settings are kept and apply once {} built in.",
            report.kept.len(),
            if report.kept.len() == 1 { "" } else { "s" },
            if report.kept.len() == 1 { "is" } else { "are" },
            if report.kept.len() == 1 { "it is" } else { "they are" },
        ));
    }
    if !report.invalid.is_empty() {
        lines.push(format!(
            "Settings this version cannot hold, left as they were: {}.",
            report.invalid.join(", ")
        ));
    }
    for skipped in &report.skipped {
        lines.push(format!("Not imported: {skipped}."));
    }
    lines
}

/// Reads a backup file and applies it.
fn import_file(path: &std::path::Path) -> Result<Report, String> {
    let size = std::fs::metadata(path)
        .map_err(|_| "The file could not be read.".to_owned())?
        .len();
    if size > MAX_BACKUP_BYTES {
        return Err("The file is too large to be a plugin backup.".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|_| "The file could not be read.".to_owned())?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "The file is not JSON.".to_owned())?;
    let mut outcome = Err(String::new());
    plugins::update(|state| {
        outcome = plugins::import(state, &value).map_err(|error| error.to_string());
    });
    outcome
}

impl Root {
    pub(super) fn plugin_search_ready(&mut self, cx: &mut Context<Self>) {
        if self.settings_extra.mods.search.is_some() {
            return;
        }
        let editor = cx.new(|cx| {
            let mut editor = crate::editor::Editor::new(super::search::field_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder_message("Search plugins");
            editor
        });
        self.settings_extra.mods.search_sub = Some(
            cx.subscribe(&editor, |_, _, _: &crate::editor::EditorEvent, cx| {
                cx.notify()
            }),
        );
        self.settings_extra.mods.search = Some(editor);
    }

    fn plugin_query(&self, cx: &gpui::App) -> String {
        self.settings_extra
            .mods
            .search
            .as_ref()
            .map(|editor| editor.read(cx).text().trim().to_lowercase())
            .unwrap_or_default()
    }

    /// The Gravecord page: the master switch and what is on.
    pub(super) fn mods_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let state = plugins::snapshot();
        // Imported plugins that were on in the backup but are not built in yet.
        let kept = state
            .plugins
            .iter()
            .filter(|(id, entry)| plugins::def(id).is_none() && entry.enabled == Some(true))
            .count();
        let master = render::plain_switch(
            "mods-master".into(),
            state.master,
            |_, on| plugins::update(|state| state.master = on),
            cx,
        )
        .into_any_element();
        let mut overview = vec![
            row(
                "Enable plugins",
                "Off turns every plugin off at once. Each plugin keeps its own switch for when this is on again.",
                master,
            ),
            row(
                format!(
                    "{} of {} plugins on",
                    state.enabled_count(),
                    plugins::DEFS.len()
                ),
                if state.master {
                    "Turn plugins on and change their settings on the Plugins page."
                } else {
                    "Plugins are off: none of them acts until this switch is on."
                },
                pressable("mods-open-plugins", "Plugins", ButtonKind::Secondary).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.settings_nav_clicked(super::Page::Plugins, "plugins");
                        cx.notify();
                    }),
                ),
            ),
        ];
        if kept > 0 {
            overview.push(row(
                format!("{kept} imported plugins are not built in yet"),
                "They were on in the imported backup. Their settings are kept and apply once they are built in.",
                div(),
            ));
        }
        vec![
            heading("Plugins"),
            div()
                .flex()
                .flex_col()
                .gap(px(24.))
                .children(overview)
                .into_any_element(),
            divider(),
            heading("Updates"),
            subtle(
                "Plugins are part of the app: they are rewritten natively, not patched in, so a Discord update does not break \
                 them, and they update with the app itself. There is no separate plugin updater.",
            )
            .into_any_element(),
        ]
    }

    /// The Plugins page: the search box and a card per plugin.
    pub(super) fn plugins_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let state = plugins::snapshot();
        let query = self.plugin_query(cx);
        let mut body = Vec::new();
        if !state.master {
            body.push(
                div()
                    .mb(px(16.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-normal"))
                    .child(subtle(
                        "Plugins are turned off on the Gravecord page. Switches here are kept and apply once plugins are on.",
                    ))
                    .into_any_element(),
            );
        }
        if let Some(editor) = self.settings_extra.mods.search.clone() {
            let focused = gpui::Focusable::focus_handle(editor.read(cx), cx).is_focused(window);
            body.push(
                div()
                    .mb(px(16.))
                    .h(px(40.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(if focused {
                        "--input-border-active"
                    } else {
                        "--input-border-default"
                    }))
                    .bg(color("--input-background-default"))
                    .child(mask_icon("settings_search", 16., "--icon-default"))
                    .child(div().flex_1().min_w(px(0.)).child(editor))
                    .into_any_element(),
            );
        }
        let shown: Vec<&'static Def> = plugins::DEFS
            .iter()
            .filter(|def| matches(def, &query))
            .collect();
        if shown.is_empty() {
            body.push(subtle("No plugin matches.").into_any_element());
        }
        for def in shown {
            body.push(self.plugin_card(def, &state, cx));
        }
        vec![
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(body)
                .into_any_element(),
        ]
    }

    fn plugin_card(
        &self,
        def: &'static Def,
        state: &plugins::State,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = def.id;
        let open = self.settings_extra.mods.expanded.contains(id);
        let enabled = state.enabled(id);
        let toggle = render::plain_switch(
            SharedString::from(format!("plugin-switch-{id}")),
            enabled,
            move |_, on| plugins::update(|state| state.set_enabled(id, on)),
            cx,
        )
        .into_any_element();
        let fold = (!def.settings.is_empty()).then(|| {
            div()
                .id(SharedString::from(format!("plugin-fold-{id}")))
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let expanded = &mut this.settings_extra.mods.expanded;
                    if !expanded.remove(id) {
                        expanded.insert(id);
                    }
                    cx.notify();
                }))
                .child(mask_icon(
                    if open {
                        "settings_accordion_closed"
                    } else {
                        "settings_accordion_open"
                    },
                    20.,
                    "--text-default",
                ))
        });
        let head = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(label(def.name))
                            .child(
                                div()
                                    .px(px(6.))
                                    .rounded(px(4.))
                                    .bg(color("--background-mod-normal"))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(def.origin.name()),
                            ),
                    )
                    .child(subtle(def.description))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(format!("Based on {}'s {}", def.origin.name(), def.source)),
                    ),
            )
            .children(fold)
            .child(toggle);
        div()
            .id(SharedString::from(format!("plugin-card-{id}")))
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(head)
            .when(open, |this| {
                this.child(div().h(px(1.)).bg(color("--border-subtle")))
                    .children(
                        def.settings
                            .iter()
                            .map(|setting| self.plugin_setting(def, setting, state, cx)),
                    )
            })
            .into_any_element()
    }

    fn plugin_setting(
        &self,
        def: &'static Def,
        setting: &'static plugins::Setting,
        state: &plugins::State,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = def.id;
        let key = setting.key;
        let value = state.value(id, key);
        match &setting.kind {
            Kind::Switch { .. } => row(
                setting.label,
                setting.description,
                render::plain_switch(
                    SharedString::from(format!("plugin-setting-{id}-{key}")),
                    value.as_bool().unwrap_or(false),
                    move |_, on| {
                        plugins::update(|state| {
                            state.set_value(id, key, &serde_json::Value::Bool(on));
                        })
                    },
                    cx,
                ),
            ),
            Kind::Number { min, max, step, .. } => {
                let current = value.as_f64().unwrap_or(*min);
                let (min, max, step) = (*min, *max, *step);
                let stepper = |sign: f64, glyph: &'static str| {
                    let next = (current + sign * step).clamp(min, max);
                    div()
                        .id(SharedString::from(format!(
                            "plugin-step-{id}-{key}-{}",
                            if sign < 0. { "down" } else { "up" }
                        )))
                        .size(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .text_color(color("--control-secondary-text-default"))
                        .text_size(px(16.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--control-secondary-background-hover")))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            plugins::update(|state| {
                                state.set_value(id, key, &serde_json::Value::from(next));
                            });
                            cx.notify();
                        }))
                        .child(glyph)
                };
                let shown = if current.fract() == 0. {
                    format!("{current:.0}")
                } else {
                    format!("{current}")
                };
                row(
                    setting.label,
                    setting.description,
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(stepper(-1., "\u{2212}"))
                        .child(
                            div()
                                .min_w(px(40.))
                                .flex()
                                .justify_center()
                                .text_size(px(16.))
                                .text_color(color("--text-strong"))
                                .child(shown),
                        )
                        .child(stepper(1., "+")),
                )
            }
            Kind::Select { options, .. } => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(label(setting.label))
                .child(subtle(setting.description))
                .child(div().flex().flex_col().gap(px(4.)).children(
                    options.iter().enumerate().map(|(index, (name, lit))| {
                        let selected = lit.matches(&value);
                        let lit = *lit;
                        div()
                            .id(SharedString::from(format!(
                                "plugin-option-{id}-{key}-{index}"
                            )))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .p(px(8.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .when(selected, |this| this.bg(color("--background-mod-normal")))
                            .hover(|style| style.bg(color("--background-mod-muted")))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                plugins::update(|state| {
                                    state.set_value(id, key, &lit.to_value());
                                });
                                cx.notify();
                            }))
                            .child(render::radio_dot(selected))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(*name),
                            )
                    }),
                ))
                .into_any_element(),
        }
    }

    /// Backup & Restore: import a backup, export ours.
    pub(super) fn backup_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let outcome = self
            .settings_extra
            .mods
            .outcome
            .as_ref()
            .map(|outcome| match outcome {
                Ok(report) => report_lines(report),
                Err(error) => vec![error.clone()],
            });
        let failed = matches!(self.settings_extra.mods.outcome, Some(Err(_)));
        vec![
            heading("Import"),
            row(
                "Import a backup",
                "An Equicord or Vencord backup (their Backup & Restore page, Export Settings), or a file exported here. \
                 Only plugin settings are read; plugins it does not name keep their settings.",
                pressable("mods-import", "Import", ButtonKind::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.pick_backup(cx))),
            ),
            div()
                .when_some(outcome, |this, lines| {
                    this.mt(px(16.))
                        .p(px(12.))
                        .rounded(px(8.))
                        .bg(color(if failed {
                            "--background-feedback-mod-critical"
                        } else {
                            "--background-mod-normal"
                        }))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .children(lines.into_iter().map(|line| {
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(line)
                        }))
                })
                .into_any_element(),
            divider(),
            heading("Export"),
            row(
                "Export plugin settings",
                "Saves which plugins are on and their settings to a file that can be imported here again.",
                pressable("mods-export", "Export", ButtonKind::Secondary)
                    .on_click(cx.listener(|this, _, _, cx| this.save_backup(cx))),
            ),
            div()
                .when_some(self.settings_extra.mods.exported.clone(), |this, path| {
                    this.mt(px(16.)).child(subtle(format!("Saved to {path}")))
                })
                .into_any_element(),
        ]
    }

    fn pick_backup(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let outcome = import_file(&path);
            let _ = this.update(cx, |this, cx| {
                this.settings_extra.mods.outcome = Some(outcome);
                cx.notify();
            });
        })
        .detach();
    }

    fn save_backup(&mut self, cx: &mut Context<Self>) {
        let directory = std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let name = format!(
            "gravecord-plugins-{}.json",
            chrono::Local::now().format("%Y-%m-%d")
        );
        let body =
            serde_json::to_vec_pretty(&plugins::export(&plugins::snapshot())).unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let saved = std::fs::write(&path, body).is_ok();
            let _ = this.update(cx, |this, cx| {
                if saved {
                    this.settings_extra.mods.exported = Some(path.display().to_string());
                } else {
                    this.show_app_toast("The file could not be saved.", cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The harness's `plugin` step: `state`, `master on|off`, `on <id>`, `off <id>`, `set <id> <key> <json>`,
    /// `import <path>`, `fold <id>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_plugin(&mut self, words: &[&str]) -> String {
        match words {
            ["state"] => plugins::snapshot().to_value().to_string(),
            ["master", switch @ ("on" | "off")] => {
                plugins::update(|state| state.master = *switch == "on");
                format!("master {switch}")
            }
            [switch @ ("on" | "off"), id] => match plugins::def(id) {
                Some(def) => {
                    plugins::update(|state| state.set_enabled(def.id, *switch == "on"));
                    format!("{} {switch}", def.id)
                }
                None => format!("no plugin {id}"),
            },
            ["set", id, key, value @ ..] => {
                let text = value.join(" ");
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    return format!("cannot read {text}");
                };
                let mut stored = false;
                plugins::update(|state| stored = state.set_value(id, key, &value));
                if stored {
                    format!("{id}.{key} = {value}")
                } else {
                    format!("{id}.{key} cannot be {value}")
                }
            }
            ["fold", id] => match plugins::def(id) {
                Some(def) => {
                    self.settings_extra.mods.expanded.insert(def.id);
                    format!("{} unfolded", def.id)
                }
                None => format!("no plugin {id}"),
            },
            ["import", path @ ..] => {
                let outcome = import_file(std::path::Path::new(&path.join(" ")));
                let line = match &outcome {
                    Ok(report) => report_lines(report).join(" "),
                    Err(error) => error.clone(),
                };
                self.settings_extra.mods.outcome = Some(outcome);
                line
            }
            _ => "usage: plugin state | master on|off | on|off <id> | set <id> <key> <json> | fold <id> | import <path>"
                .to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_reads_names_descriptions_and_settings() {
        let def = plugins::def("MessageClickActions").unwrap();
        assert!(matches(def, ""));
        assert!(matches(def, "messageclick"));
        assert!(matches(def, "double-click"));
        assert!(matches(def, "direct messages"));
        assert!(!matches(def, "tracking"));
    }

    #[test]
    fn the_report_names_what_was_kept_and_skipped() {
        let report = Report {
            ported: vec!["ClearURLs"],
            turned_on: 1,
            kept: vec!["A".to_owned(), "B".to_owned()],
            kept_off: 3,
            invalid: vec![],
            skipped: vec!["Quick CSS and themes"],
        };
        assert_eq!(
            report_lines(&report),
            vec![
                "Imported 1 plugin (1 turned on).",
                "2 plugins it turns on are not built in yet. Their settings are kept and apply once they are built in.",
                "Not imported: Quick CSS and themes.",
            ]
        );
    }
}
