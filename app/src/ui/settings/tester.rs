//! Updates and Report a Bug, in the Gravecord group of the settings sidebar. Not part of the pinned client (its updater
//! has no page: the title bar button is all it shows), so there is no capture to match: the pages are built from the
//! settings layer's own pieces (headings, rows, radio dots, buttons, tokens) so they read as part of it.
//!
//! Updates shows what `crate::updater` is doing, which the official client keeps silent about: the version, a check
//! running or failed, the download's progress, Restart to Update once it is verified, and the channel (Stable, or
//! Nightly for every tester build). Report a Bug names the log files and who to send them to.

use super::render::radio_dot;
use super::{ButtonKind, button, color, divider};
use crate::ui::Root;
use crate::updater::{self, Channel, State};
use gpui::{AnyElement, ClipboardItem, Context, FontWeight, SharedString, div, prelude::*, px};
use std::path::{Path, PathBuf};

/// Where testers open an issue: the update repository's new-issue page (`updater::repository`).
pub(crate) fn issues_url() -> String {
    format!("{}/issues/new", updater::repository().trim_end_matches('/'))
}

/// Who testers send their logs to.
pub(crate) const CONTACT: &str = "loreanxavier";

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

/// A row: label and description on the left, the controls at the end.
fn row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    controls: Vec<AnyElement>,
) -> AnyElement {
    let description: SharedString = description.into();
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
                    this.child(subtle(description))
                }),
        )
        .child(div().flex_none().flex().gap(px(8.)).children(controls))
        .into_any_element()
}

fn pressable(id: &'static str, text: &'static str, kind: ButtonKind) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(format!("{id}-press")))
        .flex_none()
        .child(button(id, text, kind))
}

/// The tester build's tag beside the version.
fn tag(text: &'static str) -> AnyElement {
    div()
        .px(px(6.))
        .h(px(18.))
        .rounded(px(4.))
        .bg(color("--background-brand"))
        .flex()
        .items_center()
        .text_size(px(12.))
        .font_weight(FontWeight(600.))
        .text_color(gpui::white())
        .child(text)
        .into_any_element()
}

/// "Last checked at 18:30" in local time.
fn checked_at(seconds: u64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_opt(seconds as i64, 0)
        .single()
        .map(|time| format!("Last checked at {}.", time.format("%H:%M")))
        .unwrap_or_default()
}

/// The status line of the Updates page: its title and description, from the updater's state and status.
pub(crate) fn status_text(state: &State, status: &updater::Status) -> (String, String) {
    let checked = status.last_checked.map(checked_at).unwrap_or_default();
    match state {
        State::Ready { version } => (
            format!("Version {version} is ready"),
            "Restart to install it. Gravecord opens again on its own.".to_owned(),
        ),
        State::Downloading { version } => {
            let progress = status.progress.map_or(String::new(), |(done, total)| {
                format!(
                    " {}% of {:.1} MB.",
                    done.saturating_mul(100) / total.max(1),
                    total as f64 / 1_000_000.
                )
            });
            (format!("Downloading version {version}"), format!("Checked and verified before it can be installed.{progress}"))
        }
        State::NotAvailable if status.checking => ("Checking for updates".to_owned(), String::new()),
        State::NotAvailable if status.not_installed => (
            "Updates need the installed app".to_owned(),
            "This copy was not started from an install (gravecord-setup.exe), so it cannot replace itself.".to_owned(),
        ),
        State::NotAvailable => match &status.error {
            Some(error) => (
                "The last check failed".to_owned(),
                format!("{error}. {checked}").trim().to_owned(),
            ),
            None if status.last_checked.is_some() => ("Gravecord is up to date".to_owned(), checked),
            None => ("Updates are checked every hour".to_owned(), String::new()),
        },
    }
}

/// The log files a bug report needs, as (title, description, path, whether it is a folder).
pub(crate) fn report_files() -> Vec<(&'static str, &'static str, PathBuf, bool)> {
    let Some(data) = crate::storage::data_dir() else {
        return Vec::new();
    };
    vec![
        (
            "App logs",
            "What the app did, one file per start (the newest five are kept).",
            crate::logging::tester_folder().unwrap_or_else(|| data.join("logs")),
            true,
        ),
        (
            "Crash log",
            "Written when the app closes on an error.",
            data.join("panic.log"),
            false,
        ),
        (
            "Voice logs",
            "Written while Debug Logging is on in Voice & Video > Advanced.",
            data.join("voice"),
            true,
        ),
    ]
}

/// Shows a file in Explorer (a folder opens; a file is selected in its folder). A capture run only logs it.
fn reveal(path: &Path, cx: &mut gpui::App) {
    if crate::storage::is_capture_run() {
        tracing::info!("report a bug: reveal (capture no-op)");
        return;
    }
    if path.is_dir() {
        cx.open_with_system(path);
    } else {
        cx.reveal_path(path);
    }
}

impl Root {
    pub(super) fn updates_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let state = updater::state();
        let status = updater::status();
        let (title, description) = status_text(&state, &status);
        let action = match state {
            State::Ready { .. } => {
                pressable("updates-restart", "Restart to Update", ButtonKind::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.update_clicked(cx)))
                    .into_any_element()
            }
            State::Downloading { .. } => div().into_any_element(),
            State::NotAvailable => {
                pressable("updates-check", "Check for Updates", ButtonKind::Secondary)
                    .when(status.checking, |this| this.opacity(0.5))
                    .on_click(cx.listener(|_, _, _, cx| {
                        updater::request_check();
                        cx.notify();
                    }))
                    .into_any_element()
            }
        };
        let version = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(label(format!("Version {}", updater::current_version())))
            .when(updater::tester_build(), |this| this.child(tag("TESTER")))
            .into_any_element();
        let channel = updater::channel();
        let choice =
            |id: &'static str, choice: Channel, name: &'static str, about: &'static str| {
                div()
                    .id(id)
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-subtle"))
                    .hover(|this| this.bg(color("--background-mod-normal")))
                    .child(radio_dot(channel == choice))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(label(name))
                            .child(subtle(about)),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if updater::channel() != choice {
                            updater::set_channel(choice);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            };
        let feed = format!(
            "Updates come from the releases of {}, and only a setup signed with the release key is installed.",
            updater::repository().trim_start_matches("https://")
        );
        vec![
            heading("Updates"),
            div()
                .flex()
                .flex_col()
                .gap(px(24.))
                .child(version)
                .child(row(title, description, vec![action]))
                .into_any_element(),
            divider(),
            heading("Channel"),
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(choice(
                    "updates-channel-stable",
                    Channel::Stable,
                    "Stable",
                    "The releases everyone gets.",
                ))
                .child(choice(
                    "updates-channel-nightly",
                    Channel::Nightly,
                    "Nightly",
                    "Every tester build as soon as it is published. Newer, and with newer bugs: send them in from Report a Bug.",
                ))
                .child(div().pt(px(8.)).child(subtle(feed)))
                .into_any_element(),
        ]
    }

    pub(super) fn report_bug_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let logging = crate::logging::tester_folder().is_some();
        let intro = div()
            .p(px(16.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(label("Something went wrong?"))
            .child(subtle(format!(
                "Open an issue on GitHub Issues (a free GitHub account is enough) and attach the files below, or send \
                 them to {CONTACT} on Discord. Say what you did and what happened; a screenshot or video helps too. The \
                 logs say what the app did; they hold no password or token."
            )))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .pt(px(4.))
                    .child(
                        pressable("report-open-issues", "Open GitHub Issues", ButtonKind::Primary).on_click(
                            cx.listener(|_, _, _, cx| {
                                if crate::storage::is_capture_run() {
                                    tracing::info!("report a bug: issues (capture no-op)");
                                } else {
                                    cx.open_url(&issues_url());
                                }
                            }),
                        ),
                    )
                    .child(
                        pressable("report-copy-contact", "Copy Discord Username", ButtonKind::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(CONTACT.to_owned()));
                                this.put_app_toast("Copied!".to_owned(), true, cx);
                            })),
                    ),
            )
            .into_any_element();
        let files: Vec<AnyElement> = report_files()
            .into_iter()
            .enumerate()
            .map(|(index, (title, about, path, _))| {
                let shown = path.display().to_string();
                let (open, copy) = (path.clone(), shown.clone());
                let id = |name: &str| SharedString::from(format!("report-{name}-{index}"));
                let description = if title == "App logs" && !logging {
                    "Off: only tester builds keep one. Turn it on below, then do what went wrong again.".to_owned()
                } else {
                    format!("{about}\n{shown}")
                };
                row(
                    title,
                    description,
                    vec![
                        div()
                            .id(id("open"))
                            .child(button(id("open-button"), "Open Folder", ButtonKind::Secondary))
                            .on_click(cx.listener(move |_, _, _, cx| reveal(&open, cx)))
                            .into_any_element(),
                        div()
                            .id(id("copy"))
                            .child(button(id("copy-button"), "Copy Path", ButtonKind::Secondary))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                this.put_app_toast("Copied!".to_owned(), true, cx);
                            }))
                            .into_any_element(),
                    ],
                )
            })
            .collect();
        let keep = (!updater::tester_build()).then(|| {
            row(
                "Keep a log file",
                "Tester builds always keep one. Here it starts now and on every start until it is turned off.",
                vec![
                    super::render::plain_switch(
                        "report-keep-log".into(),
                        logging,
                        |_, on| set_keep_log(on),
                        cx,
                    )
                    .into_any_element(),
                ],
            )
        });
        vec![
            heading("Report a Bug"),
            intro,
            div().h(px(24.)).into_any_element(),
            div()
                .flex()
                .flex_col()
                .gap(px(24.))
                .children(files)
                .children(keep)
                .into_any_element(),
        ]
    }
}

fn set_keep_log(on: bool) {
    crate::storage::set("logging.keep", serde_json::json!(on));
    if on {
        if crate::logging::tester_folder().is_none()
            && !crate::storage::is_capture_run()
            && let Some(dir) = crate::storage::data_dir()
        {
            crate::logging::start_tester_file(&dir.join("logs"));
        }
    } else {
        crate::logging::stop_tester_file();
    }
}
