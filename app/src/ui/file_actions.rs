//! The text-file card's buttons, captured in `design/golden/windows/menus/file-language-`, `modals/file-expand-` and
//! `menus/file-more-darker-cozy`: Change language (a searchable list of module 652845's tree-sitter languages; the
//! preview is highlighted in the chosen one), Expand (the file in a 1024px dialog), and More (Download, Wrap Text).
//! Download saves the file into the user's Downloads folder.

use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::ui::motion::Posed as _;
use dc_state::{Attachment, Id};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, Pixels, Point, SharedString,
    Subscription, Window, deferred, div, prelude::*, px,
};

/// The keys of module 652845's language table, in its order.
pub(crate) const LANGUAGES: [&str; 101] = [
    "ada",
    "agda",
    "asciidoc",
    "asm",
    "awk",
    "bash",
    "batch",
    "c",
    "c-sharp",
    "caddy",
    "capnp",
    "cedar",
    "cedarschema",
    "clojure",
    "cmake",
    "commonlisp",
    "cpp",
    "css",
    "d",
    "dart",
    "devicetree",
    "diff",
    "dockerfile",
    "dot",
    "elisp",
    "elixir",
    "elm",
    "erlang",
    "fish",
    "fsharp",
    "gleam",
    "glsl",
    "go",
    "graphql",
    "groovy",
    "haskell",
    "hcl",
    "hlsl",
    "html",
    "idris",
    "ini",
    "java",
    "javascript",
    "jinja2",
    "jq",
    "json",
    "julia",
    "kotlin",
    "lean",
    "lua",
    "luau",
    "markdown",
    "markdown_inline",
    "matlab",
    "meson",
    "ninja",
    "nix",
    "objc",
    "ocaml",
    "perl",
    "php",
    "postscript",
    "powershell",
    "prolog",
    "python",
    "query",
    "r",
    "rego",
    "rescript",
    "ron",
    "ruby",
    "rust",
    "scala",
    "scheme",
    "scss",
    "solidity",
    "sparql",
    "sql",
    "ssh-config",
    "starlark",
    "styx",
    "svelte",
    "swift",
    "textproto",
    "thrift",
    "tlaplus",
    "toml",
    "tsx",
    "typescript",
    "typst",
    "verilog",
    "vhdl",
    "vim",
    "vue",
    "wit",
    "x86asm",
    "xml",
    "yaml",
    "yuri",
    "zig",
    "zsh",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileMenuKind {
    Language,
    More,
}

pub(crate) struct FileMenu {
    pub attachment: Attachment,
    pub kind: FileMenuKind,
    pub at: Point<Pixels>,
    pub search: Option<(Entity<Editor>, Subscription)>,
    pub query: String,
}

impl Root {
    pub(super) fn open_file_menu(
        &mut self,
        attachment: Attachment,
        kind: FileMenuKind,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .file_menu
            .as_ref()
            .is_some_and(|menu| menu.attachment.id == attachment.id && menu.kind == kind)
        {
            self.file_menu = None;
            cx.notify();
            return;
        }
        let search = (kind == FileMenuKind::Language).then(|| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(editor_style(), cx);
                editor.set_placeholder("Search languages".to_owned());
                editor
            });
            let events = cx.subscribe(&editor, |this, editor, event: &EditorEvent, cx| {
                if let Some(menu) = this.file_menu.as_mut() {
                    match event {
                        EditorEvent::Edited => menu.query = editor.read(cx).text().to_owned(),
                        EditorEvent::Submit(text) => {
                            editor.update(cx, |editor, _| editor.set_text(text))
                        }
                        EditorEvent::Cancel => this.file_menu = None,
                    }
                }
                cx.notify();
            });
            window.focus(&gpui::Focusable::focus_handle(editor.read(cx), cx), cx);
            (editor, events)
        });
        self.file_menu = Some(FileMenu {
            attachment,
            kind,
            at,
            search,
            query: String::new(),
        });
        cx.notify();
    }

    /// Saves the attachment into Downloads, under its own name (a number added when the name is taken).
    pub(super) fn download_attachment(&mut self, attachment: &Attachment, cx: &mut Context<Self>) {
        let _ = self.requests.send(crate::net::Request::Download {
            url: attachment.url.clone(),
            filename: attachment.filename.clone(),
            done: None,
        });
        cx.notify();
    }

    pub(super) fn file_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.file_menu.as_ref()?;
        let attachment = menu.attachment.clone();
        let id: Id = attachment.id;
        let height = match menu.kind {
            FileMenuKind::More => 78.,
            FileMenuKind::Language => 250.,
        };
        let panel: AnyElement = match menu.kind {
            FileMenuKind::More => {
                let wrapped = self.file_wrap.contains(&id);
                let download = attachment.clone();
                div()
                    .w(px(120.))
                    .p(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .child(
                        div()
                            .id("file-download")
                            .h(px(32.))
                            .px(px(8.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .hover(|style| style.bg(color("--interactive-background-hover")))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.file_menu = None;
                                this.download_attachment(&download, cx);
                            }))
                            .child("Download"),
                    )
                    .child(
                        div()
                            .id("file-wrap")
                            .h(px(32.))
                            .px(px(8.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .hover(|style| style.bg(color("--interactive-background-hover")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.file_wrap.remove(&id) {
                                    this.file_wrap.insert(id);
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child("Wrap Text"),
                            )
                            .child(
                                div()
                                    .size(px(18.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(if wrapped {
                                        color("--checkbox-border-selected-default")
                                    } else {
                                        color("--checkbox-border-default")
                                    })
                                    .bg(if wrapped {
                                        color("--checkbox-background-selected-default")
                                    } else {
                                        color("--checkbox-background-default")
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(wrapped, |this| {
                                        this.child(icon("checkbox_check", 16., "--white"))
                                    }),
                            ),
                    )
                    .into_any_element()
            }
            FileMenuKind::Language => {
                let query = menu.query.trim().to_lowercase();
                let current = self.file_language.get(&id).cloned();
                let rows: Vec<AnyElement> = LANGUAGES
                    .iter()
                    .filter(|language| query.is_empty() || language.contains(&query))
                    .map(|language| {
                        let picked = current.as_deref() == Some(*language);
                        let name = (*language).to_owned();
                        div()
                            .id(SharedString::from(format!("file-language-{language}")))
                            .h(px(32.))
                            .px(px(8.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .when(picked, |this| this.bg(color("--background-mod-normal")))
                            .hover(|style| style.bg(color("--interactive-background-hover")))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.file_language.insert(id, name.clone());
                                this.file_menu = None;
                                cx.notify();
                            }))
                            .child(*language)
                            .into_any_element()
                    })
                    .collect();
                div()
                    .w(px(176.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .children(menu.search.as_ref().map(|(editor, _)| {
                        div()
                            .h(px(36.))
                            .px(px(8.))
                            .mb(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-active"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .child(div().w_full().child(editor.clone()))
                    }))
                    .child(
                        div()
                            .id("file-languages")
                            .h(px(200.))
                            .overflow_y_scroll()
                            .children(rows),
                    )
                    .into_any_element()
            }
        };
        Some(
            deferred(
                div().child(super::keyboard::outside(
                    div()
                        .id("file-menu")
                        .popout(
                            "file-menu",
                            PopKind::Translate,
                            if menu.kind == FileMenuKind::More {
                                PopSide::Top
                            } else {
                                PopSide::Left
                            },
                        )
                        .occlude()
                        .absolute()
                        .left(
                            menu.at.x
                                - px(if menu.kind == FileMenuKind::More {
                                    120.
                                } else {
                                    0.
                                }),
                        )
                        // Too near the bottom, the menu opens upward from the button.
                        .top(if f32::from(menu.at.y) + height > 712. {
                            menu.at.y - px(height + 12.)
                        } else {
                            menu.at.y + px(12.)
                        })
                        .child(panel),
                    super::keyboard::Layer::FileMenu,
                    cx,
                )),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// Expand: the file in a 1024px dialog over a dark scrim, its name and size under it.
    pub(super) fn file_expanded_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let attachment = self.file_expanded.as_ref()?;
        let text = self
            .texts
            .get(&super::files::text_key(attachment))
            .cloned()
            .unwrap_or_default();
        let language = self.file_language.get(&attachment.id).map(String::as_str);
        let wrap = self.file_wrap.contains(&attachment.id);
        let pose = crate::ui::motion::modal_pose("file-expanded");
        Some(
            deferred(
                div()
                    .id("file-expanded-layer")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(pose.dim(gpui::rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.file_expanded = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .posed(pose)
                            .id("file-expanded")
                            .occlude()
                            .w(px(1024.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("file-expanded-text")
                                    .max_h(px(560.))
                                    .min_h(px(122.))
                                    .overflow_y_scroll()
                                    .p(px(8.))
                                    .text_size(px(14.))
                                    .line_height(px(16.))
                                    .when(!wrap, |this| this.whitespace_nowrap())
                                    .children(super::code_lines(language, &text)),
                            )
                            .child(
                                div()
                                    .h(px(67.))
                                    .px(px(16.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-surface-high"))
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-default"))
                                                    .child(attachment.filename.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(color("--text-subtle"))
                                                    .child(super::files::format_size(
                                                        attachment.size,
                                                    )),
                                            ),
                                    )
                                    .child(icon("file_language", 18., "--text-default"))
                                    .child(div().ml(px(16.)).child(icon(
                                        "file_more",
                                        18.,
                                        "--text-default",
                                    ))),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}
