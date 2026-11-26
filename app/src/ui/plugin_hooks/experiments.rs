//! Experiments (Vencord `src/plugins/experiments`, GPL-3.0-or-later): the official client's developer-only "Staff Only"
//! section with its Experiments page (module 158935 `SS`, rows `SE` for user and `Sh` for guild experiments), which
//! the original opens by making the client think the account is a developer. Above the list, the original's warning
//! card. Each row opens to the account's assignment and the override of its bucket or variant (module 271478); the
//! overrides are stored (`plugin-experiments-overrides`, as the official stores keep theirs in local storage) and
//! applied to the account's experiment stores while the plugin is on (`plugins/experiments.rs`). Not part of the
//! pinned client.

use super::super::{Root, color};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::experiments::{
    self as table, Definition, Kind, Overrides, System, guild_summary, overrides_from,
    overrides_value,
};
use dc_state::experiments::GuildFacts;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ParentElement, SharedString, Styled,
    Subscription, div, prelude::*, px,
};
use std::cell::RefCell;
use std::collections::HashSet;

const ID: &str = "Experiments";
const STORAGE_KEY: &str = "plugin-experiments-overrides";

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

#[derive(Default)]
struct Page {
    search: Option<(Entity<Editor>, Subscription)>,
    /// The rows opened by a click (an overridden row starts open, `defaultOpen`).
    toggled: HashSet<&'static str>,
    details: HashSet<&'static str>,
}

thread_local! {
    /// The stored overrides, read once.
    static OVERRIDES: RefCell<Option<Overrides>> = const { RefCell::new(None) };
    /// Bumped on every change of the overrides.
    static CHANGES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// What the stores were last given: whether the plugin was on, and the change count.
    static APPLIED: std::cell::Cell<Option<(bool, u64)>> = const { std::cell::Cell::new(None) };
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
}

fn overrides() -> Overrides {
    OVERRIDES.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                if cfg!(test) {
                    return Overrides::new();
                }
                crate::storage::get(STORAGE_KEY)
                    .map(|value| overrides_from(&value))
                    .unwrap_or_default()
            })
            .clone()
    })
}

fn set_override(name: &str, bucket: Option<i64>) {
    let mut all = overrides();
    match bucket {
        Some(bucket) => all.insert(name.to_owned(), bucket),
        None => all.remove(name),
    };
    if !cfg!(test) {
        crate::storage::set(STORAGE_KEY, overrides_value(&all));
    }
    OVERRIDES.with(|cell| *cell.borrow_mut() = Some(all));
    CHANGES.with(|changes| changes.set(changes.get() + 1));
}

/// Whether the sidebar shows the Staff Only section.
pub(in crate::ui) fn experiments_on() -> bool {
    on()
}

/// The original's warning card (`WarningCard`, an `ErrorCard`), with this app's name for the mod's.
fn warning_card() -> AnyElement {
    let paragraph = |text: &'static str| {
        div()
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(text)
    };
    div()
        .mb(px(16.))
        .p(px(16.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--border-feedback-critical"))
        .bg(color("--background-feedback-mod-critical"))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child("Hold on!!"),
        )
        .child(paragraph(
            "Experiments are unreleased Discord features. They might not work, or even break your client or get \
             your account disabled.",
        ))
        .child(paragraph(
            "Only use experiments if you know what you're doing. Gravecord is not responsible for any damage caused \
             by enabling experiments. If you don't know what an experiment does, ignore it. Do not ask us what \
             experiments do either, we probably don't know.",
        ))
        .child(paragraph(
            "No, you cannot use server-side features like checking the \"Send to Client\" box.",
        ))
        .into_any_element()
}

impl Root {
    /// Gives the account's experiment stores the stored overrides while the plugin is on, and takes them away when it
    /// is turned off; only when one of the two changed.
    pub(in crate::ui) fn experiments_prepare(&mut self) {
        let wanted = (on(), CHANGES.with(std::cell::Cell::get));
        if APPLIED.with(std::cell::Cell::get) == Some(wanted) {
            return;
        }
        APPLIED.with(|applied| applied.set(Some(wanted)));
        let experiments = &mut self.stores.experiments;
        let guilds = &mut self.stores.guild_experiments;
        experiments.clear_overrides();
        guilds.clear_overrides();
        if !wanted.0 {
            return;
        }
        crate::plugins::note(ID);
        for (name, bucket) in overrides() {
            match table::definition(&name).map(|definition| (definition.system, definition.kind)) {
                Some((System::Legacy, Kind::User)) => {
                    experiments.set_legacy_override(&name, Some(bucket))
                }
                Some((System::Legacy, Kind::Guild)) => guilds.set_override(&name, Some(bucket)),
                Some((System::Apex, Kind::User)) => experiments.set_override(&name, Some(bucket)),
                // The app keeps no apex guild assignments, so there is nothing to override (GAP).
                Some((System::Apex, Kind::Guild)) | None => {}
            }
        }
        self.data_revision += 1;
    }

    /// The buckets the account's guilds are in, READY's assignment without the override (`Current Assignments`).
    fn experiments_guild_buckets(&self, name: &str) -> Vec<i64> {
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        self.stores
            .guilds
            .iter()
            .map(|guild| {
                let facts = GuildFacts {
                    id: guild.id,
                    member_count: self.stores.member_counts.get_member_count(guild.id),
                    features: guild.features.as_slice(),
                    hub_type: None,
                    has_vanity_url: guild.vanity_url_code.is_some(),
                    now_ms: now,
                };
                self.stores
                    .guild_experiments
                    .assigned_bucket(name, &facts)
                    .unwrap_or(dc_state::experiments::NOT_ELIGIBLE)
            })
            .collect()
    }

    /// The row's override label and the line under it (`T` of `SE`, `Current Assignments` of `Sh`), and whether the
    /// server sent a config for it (the warning under the override when not).
    fn experiments_assignment(
        &self,
        definition: &Definition,
    ) -> (&'static str, String, bool, String) {
        let name = definition.name;
        match (definition.system, definition.kind) {
            (System::Legacy, Kind::User) => {
                let assigned = self.stores.experiments.assigned_legacy_bucket(name);
                (
                    "Bucket Override",
                    format!(
                        "Currently assigned to bucket {}",
                        assigned.unwrap_or(dc_state::experiments::NOT_ELIGIBLE)
                    ),
                    assigned.is_some(),
                    assigned.map_or("None".to_owned(), |bucket| {
                        format!("{{\"bucket\": {bucket}}}")
                    }),
                )
            }
            (System::Apex, Kind::User) => {
                let assigned = self.stores.experiments.assigned_variant(name);
                (
                    "Variant Override",
                    assigned.map_or("Currently unassigned".to_owned(), |variant| {
                        format!("Currently assigned to variant {variant}")
                    }),
                    assigned.is_some(),
                    assigned.map_or("None".to_owned(), |variant| {
                        format!("{{\"variantId\": {variant}}}")
                    }),
                )
            }
            (System::Legacy, Kind::Guild) => {
                let buckets = self.experiments_guild_buckets(name);
                (
                    "Bucket Override",
                    format!("Current Assignments: {}", guild_summary(&buckets)),
                    self.stores.guild_experiments.loaded(name),
                    "None".to_owned(),
                )
            }
            (System::Apex, Kind::Guild) => (
                "Bucket Override",
                format!(
                    "Current Assignments: {}",
                    guild_summary(&vec![
                        dc_state::experiments::NOT_ELIGIBLE;
                        self.stores.guilds.len()
                    ])
                ),
                false,
                "None".to_owned(),
            ),
        }
    }

    fn experiments_row(
        &self,
        definition: &'static Definition,
        overridden: Option<i64>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = definition.name;
        let (toggled, details) = PAGE.with(|page| {
            let page = page.borrow();
            (page.toggled.contains(name), page.details.contains(name))
        });
        // `defaultOpen: null != overrideInfo`; a click flips it.
        let open = overridden.is_some() != toggled;
        let header = div()
            .id(SharedString::from(format!("experiment-{name}")))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .py(px(8.))
            .cursor_pointer()
            .on_click(cx.listener(move |_, _, _, cx| {
                PAGE.with(|page| {
                    let toggled = &mut page.borrow_mut().toggled;
                    if !toggled.remove(name) {
                        toggled.insert(name);
                    }
                });
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(definition.title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(name),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(14.))
                    .text_color(color("--text-muted"))
                    .child(match definition.kind {
                        Kind::User => "User",
                        Kind::Guild => "Guild",
                    }),
            );
        let mut row = div()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(color("--border-subtle"))
            .pb(px(8.))
            .mb(px(8.))
            .child(header);
        if !open {
            return row.into_any_element();
        }
        let (label, description, sent, descriptor) = self.experiments_assignment(definition);
        // The override (module 271478): one option per bucket, the chosen one lit, and a clear row while one is set
        // (`clearable`).
        let option = |bucket: Option<i64>, text: String, chosen: bool, cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(format!(
                    "experiment-{name}-{}",
                    bucket.map_or("clear".to_owned(), |bucket| bucket.to_string())
                )))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(chosen, |this| this.bg(color("--background-mod-normal")))
                .hover(|style| style.bg(color("--background-mod-muted")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    set_override(name, bucket);
                    this.experiments_prepare();
                    cx.notify();
                }))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color(if bucket.is_some() {
                            "--text-default"
                        } else {
                            "--text-muted"
                        }))
                        .child(text),
                )
        };
        let mut select = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"));
        for (bucket, text) in definition.buckets {
            select = select.child(option(
                Some(*bucket),
                (*text).to_owned(),
                overridden == Some(*bucket),
                cx,
            ));
        }
        if overridden.is_some() {
            select = select.child(option(None, "Clear Override".to_owned(), false, cx));
        }
        row = row
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label),
            )
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(description),
            )
            .child(select)
            .when(!sent, |this| {
                this.child(
                    div()
                        .mt(px(8.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(match (definition.system, definition.kind) {
                            (System::Legacy, _) | (System::Apex, Kind::User) => {
                                "Warning: Server did not send any experiment config. You may need to check the \
                                 \"Send to Client\" box in the admin UI."
                            }
                            (System::Apex, Kind::Guild) => {
                                "Warning: Server did not send an assignment for this experiment. Make sure the \
                                 experiment is configured to run on the APP surface in the admin UI."
                            }
                        }),
                )
            });
        let mono = |text: String| {
            div()
                .p(px(8.))
                .rounded(px(4.))
                .bg(color("--background-base-lowest"))
                .font_family(super::super::MONO)
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(700.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let debug_title = |text: &'static str| {
            div()
                .mt(px(8.))
                .mb(px(4.))
                .text_size(px(18.))
                .line_height(px(22.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        row = if details {
            row.child(debug_title("Server Descriptor"))
                .child(mono(descriptor))
                .child(debug_title("Override Descriptor"))
                .child(mono(overridden.map_or("None".to_owned(), |bucket| {
                    format!("{{\"bucket\": {bucket}, \"override\": true}}")
                })))
                .child(debug_title("Recent Exposures"))
                .child(mono("None".to_owned()))
        } else {
            row.child(
                div().mt(px(8.)).child(
                    div()
                        .id(SharedString::from(format!("experiment-{name}-details")))
                        .h(px(32.))
                        .px(px(16.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .w(gpui::Length::Auto)
                        .text_size(px(14.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .bg(color("--control-secondary-background-default"))
                        .cursor_pointer()
                        .on_click(cx.listener(move |_, _, _, cx| {
                            PAGE.with(|page| page.borrow_mut().details.insert(name));
                            cx.notify();
                        }))
                        .child("More Details \u{bb}"),
                ),
            )
        };
        row.into_any_element()
    }

    /// The Experiments page (`SS`): the warning card, the search, then the rows; "No Experiments Found" when the
    /// search keeps none.
    pub(in crate::ui) fn experiments_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !on() {
            return Vec::new();
        }
        let editor = PAGE.with(|page| {
            page.borrow()
                .search
                .as_ref()
                .map(|(editor, _)| editor.clone())
        });
        let editor = match editor {
            Some(editor) => editor,
            None => {
                let editor = cx.new(|cx| {
                    let mut editor = Editor::new(super::super::editor_style(), cx);
                    editor.set_single_line(true);
                    editor.set_placeholder("Search experiments".to_owned());
                    editor
                });
                let typing = cx.subscribe(&editor, |_, _, _: &EditorEvent, cx| cx.notify());
                PAGE.with(|page| page.borrow_mut().search = Some((editor.clone(), typing)));
                editor
            }
        };
        let query = editor.read(cx).text().to_owned();
        let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
        let overrides = overrides();
        let list = table::search(table::sorted(&overrides), &query);
        let mut out = vec![
            warning_card(),
            div()
                .cursor_text()
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                    window.focus(&focus, cx);
                })
                .mb(px(16.))
                .min_h(px(40.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .child(editor)
                .into_any_element(),
        ];
        if list.is_empty() {
            out.push(
                div()
                    .py(px(24.))
                    .flex()
                    .justify_center()
                    .text_size(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("No Experiments Found")
                    .into_any_element(),
            );
        }
        for definition in list {
            let overridden = overrides.get(definition.name).copied();
            out.push(self.experiments_row(definition, overridden, cx));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_is_kept_and_cleared() {
        set_override("2026-03_voice_badge", Some(1));
        assert_eq!(overrides().get("2026-03_voice_badge"), Some(&1));
        set_override("2026-03_voice_badge", None);
        assert!(overrides().is_empty());
    }
}
