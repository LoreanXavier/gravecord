//! ZipPreview (Equicord `src/equicordplugins/zipPreview`, GPL-3.0-or-later): a file attachment named `*.zip` gets a
//! 48 by 18 chevron under its name and size (`flex-wrap` and a full-width row, `order: 3`); opening it loads the
//! archive (a Discord attachment only after the native question "Preview ZIP attachment", Cancel / Preview ZIP) and
//! lists it: a folder icon with the path as links, the folders ("name/"), then the files with their sizes; a text or
//! picture file of 10 MB at most opens in a large dialog (text: its code block, Copy and Download; picture: Download).
//! States: "Loading ZIP preview...", the error, "This ZIP is empty.", "This folder is empty.", "Only showing first 1000
//! entries.". The archive side is `crate::plugins::zip_preview`. Not part of the pinned client.
//!
//! GAP: the opening and closing animation (180 ms of row height, opacity and a 4px slide): the list opens at once.

use super::super::{Root, color, icon, modal};
use crate::plugins::zip_preview::{self as zip, Entry, Kind, State};
use dc_state::{Attachment, Id};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, PromptLevel, SharedString,
    StatefulInteractiveElement, Styled, StyledImage as _, Window, div, img, px,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

const ID: &str = "ZipPreview";
pub(in crate::ui) const KEY: &str = "plugin-zip-preview:";

/// One attachment's view: open or not, and the folder shown.
#[derive(Clone, Debug, Default)]
struct View {
    open: bool,
    path: String,
}

thread_local! {
    static VIEWS: RefCell<HashMap<(Id, Id), View>> = RefCell::new(HashMap::new());
    /// The entry whose dialog is open.
    static DIALOG: RefCell<Option<Entry>> = const { RefCell::new(None) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

fn view(key: (Id, Id)) -> View {
    VIEWS.with(|views| views.borrow().get(&key).cloned().unwrap_or_default())
}

fn set_view(key: (Id, Id), change: impl FnOnce(&mut View)) {
    VIEWS.with(|views| change(views.borrow_mut().entry(key).or_default()));
}

fn link(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_color(color("--text-link"))
        .underline()
        .cursor_pointer()
        .child(text.into())
}

fn muted(text: impl Into<SharedString>) -> gpui::Div {
    div().text_color(color("--text-muted")).child(text.into())
}

impl Root {
    /// The row under a `.zip` file attachment's name, or `None` (another file, or the plugin off).
    pub(in crate::ui) fn zip_preview_inline(
        &self,
        message: Id,
        attachment: &Attachment,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !zip::is_zip(&attachment.display_name()) || attachment.url.is_empty() || !on() {
            return None;
        }
        let key = (message, attachment.id);
        let current = view(key);
        let url = attachment.url.clone();
        let toggle = div()
            .id(SharedString::from(format!(
                "zip-preview-toggle-{}",
                attachment.id
            )))
            .w(px(48.))
            .h(px(18.))
            .mx_auto()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(color("--text-muted"))
            .hover(|style| style.text_color(color("--text-link")))
            .child(
                gpui::svg()
                    .path(if current.open {
                        "icons/automod_chevron_up.svg"
                    } else {
                        "icons/automod_chevron_down.svg"
                    })
                    .size(px(16.)),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                let open = !view(key).open;
                set_view(key, |view| view.open = open);
                if open && !matches!(zip::state(&url), Some(State::Pending | State::Resolved(_))) {
                    this.zip_preview_load(&url, window, cx);
                }
                cx.notify();
            }));
        let content = current
            .open
            .then(|| self.zip_preview_content(key, &attachment.url, &current.path, cx));
        Some(
            div()
                .w_full()
                .min_w(px(0.))
                .text_size(px(14.))
                .line_height(px(20.))
                .text_color(color("--text-default"))
                .mt(px(if current.open { 14. } else { 4. }))
                .when(!current.open, |this| {
                    this.h(px(18.)).flex().justify_center()
                })
                .children(content)
                .child(toggle)
                .into_any_element(),
        )
    }

    /// `loadPreview`: a Discord attachment asks first (the native helper's message box), then the archive is fetched.
    fn zip_preview_load(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        zip::set_state(url, State::Pending);
        CANCELLED_SHOWN.with(|shown| shown.borrow_mut().retain(|known| known != url));
        let key = format!("{KEY}{url}");
        match zip::attachment_path(url) {
            None => {
                let _ = self.requests.send(crate::net::Request::FetchZip {
                    key,
                    url: url.to_owned(),
                    native: None,
                });
            }
            Some(path) => {
                let detail = format!(
                    "Attachment: {}\n\nIf you did not request this intentionally, choose Cancel.",
                    zip::dialog_name(&path)
                );
                let answer = window.prompt(
                    PromptLevel::Info,
                    "Discord is about to fetch this ZIP attachment for preview.",
                    Some(&detail),
                    &["Cancel", "Preview ZIP"],
                    cx,
                );
                let url = url.to_owned();
                cx.spawn(async move |this, cx| {
                    let chosen = answer.await.ok();
                    let _ = this.update(cx, |this, cx| {
                        if chosen == Some(1) {
                            let _ = this.requests.send(crate::net::Request::FetchZip {
                                key,
                                url,
                                native: Some(path),
                            });
                        } else {
                            zip::reject(&url, zip::CANCELLED.to_owned());
                            // The cancelled load still shows its message until the row is opened again.
                            CANCELLED_SHOWN.with(|shown| shown.borrow_mut().push(url));
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    /// An archive arrived (or failed): the rows show it.
    pub(in crate::ui) fn zip_preview_fetched(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if !key.starts_with(KEY) {
            return false;
        }
        cx.notify();
        true
    }

    fn zip_preview_content(
        &self,
        key: (Id, Id),
        url: &str,
        path: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = zip::state(url).or_else(|| {
            CANCELLED_SHOWN
                .with(|shown| shown.borrow().iter().any(|known| known == url))
                .then(|| State::Rejected(zip::CANCELLED.to_owned()))
        });
        let archive = match state {
            None | Some(State::Pending) => {
                return muted("Loading ZIP preview...").into_any_element();
            }
            Some(State::Rejected(message)) => return muted(message).into_any_element(),
            Some(State::Resolved(archive)) => archive,
        };
        if archive.entries.is_empty() {
            return muted("This ZIP is empty.").into_any_element();
        }
        let (folders, files) = zip::visible(&archive.entries, path);
        // `ZipPreviewBreadcrumb`: the folder icon, "/" and each part up to the current one.
        let mut crumbs = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .mb(px(4.))
            .text_color(color("--text-muted"))
            .child(icon("plugin_folder", 16., "--text-muted"));
        if path.is_empty() {
            crumbs = crumbs.child(muted("/"));
        } else {
            crumbs = crumbs.child(navigate(
                link("/"),
                format!("zip-crumb-root-{}", key.1),
                key,
                String::new(),
                cx,
            ));
            let parts: Vec<&str> = path.split('/').collect();
            for (index, part) in parts.iter().enumerate() {
                if index + 1 == parts.len() {
                    crumbs = crumbs.child(muted(format!("{part}/")));
                } else {
                    let target = parts[..=index].join("/");
                    crumbs = crumbs.child(navigate(
                        link((*part).to_owned()),
                        format!("zip-crumb-{}-{index}", key.1),
                        key,
                        target,
                        cx,
                    ));
                }
            }
        }
        let mut list = div().flex().flex_col().items_start();
        if folders.is_empty() && files.is_empty() {
            list = list.child(muted("This folder is empty."));
        }
        for folder in folders {
            let target = if path.is_empty() {
                folder.clone()
            } else {
                format!("{path}/{folder}")
            };
            list = list.child(navigate(
                link(format!("{folder}/")),
                format!("zip-folder-{}-{target}", key.1),
                key,
                target,
                cx,
            ));
        }
        for entry in files {
            let size = muted(format!("({})", zip::format_bytes(entry.size)));
            let row = div().flex().gap(px(4.));
            if entry.kind == Kind::Unsupported {
                list = list.child(row.child(muted(entry.name.clone())).child(size));
            } else {
                let opened = entry.clone();
                list = list.child(
                    row.id(SharedString::from(format!(
                        "zip-file-{}-{}",
                        key.1, entry.path
                    )))
                    .cursor_pointer()
                    .child(link(entry.name.clone()))
                    .child(size)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        DIALOG.with(|dialog| *dialog.borrow_mut() = Some(opened.clone()));
                        cx.notify();
                    })),
                );
            }
        }
        div()
            .flex()
            .flex_col()
            .child(crumbs)
            .when(archive.truncated, |this| {
                this.child(muted(format!(
                    "Only showing first {} entries.",
                    zip::MAX_ENTRIES
                )))
            })
            .child(list)
            .into_any_element()
    }

    /// The open entry's dialog (`openTextEntryModal`, `openImageEntryModal`), 680 wide (`size="lg"`).
    pub(in crate::ui) fn zip_preview_dialog(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let entry = DIALOG.with(|dialog| dialog.borrow().clone())?;
        if !on() {
            DIALOG.with(|dialog| *dialog.borrow_mut() = None);
            return None;
        }
        let tall = f32::from(window.viewport_size().height) - 240.;
        let close = |this: &mut Root, _: &mut Window, cx: &mut Context<Root>| {
            DIALOG.with(|dialog| *dialog.borrow_mut() = None);
            let _ = this;
            cx.notify();
        };
        let download = {
            let entry = entry.clone();
            move |this: &mut Root, _: &mut Window, cx: &mut Context<Root>| {
                this.zip_preview_save(&entry, cx);
            }
        };
        let (body, buttons) = if entry.kind == Kind::Image {
            let picture = Arc::new(gpui::Image::from_bytes(
                image_format(&entry.extension),
                entry.data.to_vec(),
            ));
            (
                div()
                    .min_h(px(360.))
                    .py(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(picture)
                            .max_w_full()
                            .max_h(px(f32::from(window.viewport_size().height) * 0.7))
                            .object_fit(gpui::ObjectFit::Contain),
                    )
                    .into_any_element(),
                vec![modal::button(
                    "zip-preview-download",
                    "Download",
                    modal::Tone::Primary,
                    download,
                )],
            )
        } else {
            let text = String::from_utf8_lossy(&entry.data).into_owned();
            let copied = text.clone();
            (
                div()
                    .id("zip-preview-code")
                    .max_h(px(tall.max(200.)))
                    .overflow_y_scroll()
                    .py(px(16.))
                    .child(self.code_block(
                        (u64::MAX - 7, 0),
                        0.,
                        Some(&zip::code_language(&entry.extension)),
                        &text,
                    ))
                    .into_any_element(),
                vec![
                    modal::button(
                        "zip-preview-copy",
                        "Copy",
                        modal::Tone::Secondary,
                        move |this, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
                            this.show_success_toast(
                                "File contents copied to clipboard!".into(),
                                cx,
                            );
                        },
                    ),
                    modal::button(
                        "zip-preview-download",
                        "Download",
                        modal::Tone::Primary,
                        download,
                    ),
                ],
            )
        };
        let spec = modal::Spec {
            id: "zip-preview-dialog",
            title: SharedString::from(entry.name.clone()),
            subtitle: None,
            body: Some(body),
            buttons,
            close: Box::new(close),
        };
        Some(modal::render_large(spec, cx))
    }

    /// `makeDownload` (`saveFile`): the entry is saved where the user chooses.
    fn zip_preview_save(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        let directory = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&directory, Some(&entry.name));
        let data = entry.data.clone();
        cx.spawn(async move |_, _| {
            if let Ok(Ok(Some(path))) = chosen.await {
                let _ = std::fs::write(path, &data[..]);
            }
        })
        .detach();
    }
}

thread_local! {
    /// The URLs whose load was cancelled: the row says so until it is opened again (the cache itself forgets them).
    static CANCELLED_SHOWN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// A link that opens the folder `target` of the attachment's view.
fn navigate(
    element: gpui::Div,
    id: String,
    key: (Id, Id),
    target: String,
    cx: &mut Context<Root>,
) -> AnyElement {
    element
        .id(SharedString::from(id))
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            let target = target.clone();
            set_view(key, |view| view.path = target);
            cx.notify();
        }))
        .into_any_element()
}

fn image_format(extension: &str) -> gpui::ImageFormat {
    match extension {
        "jpg" | "jpeg" => gpui::ImageFormat::Jpeg,
        "gif" => gpui::ImageFormat::Gif,
        "webp" => gpui::ImageFormat::Webp,
        "bmp" => gpui::ImageFormat::Bmp,
        _ => gpui::ImageFormat::Png,
    }
}
