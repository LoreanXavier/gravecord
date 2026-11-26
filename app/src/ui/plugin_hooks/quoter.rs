//! Quoter (Equicord `src/equicordplugins/quoter`, GPL-3.0-or-later): "Quote" at the end of a message's menu (a message
//! with text) opens "Catch Them In 4K.", a dialog with the quote picture (`plugins/quoter.rs`) and its switches:
//! Grayscale, Save as GIF and Show Watermark with the watermark's text. Export saves the picture into Downloads (the
//! browser's download of the original), Send puts it in the open channel's message box as an upload
//! (`promptToUpload`). The three switches are stored when changed, as the original's hidden settings. Not part of the
//! pinned client.
//!
//! The picture is drawn off the UI thread each time a switch or the watermark changes. Its avatar is the author's at
//! 512 when that file is loaded, else the one the chat shows; its font is the app's own (gg sans), in place of the
//! original's Google fonts.

use super::super::{Root, color, modal};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::quoter::{self as picture, CustomEmoji, Face, Quote};
use dc_state::Message;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ParentElement, SharedString, Styled,
    Subscription, div, img, prelude::*, px,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

const ID: &str = "Quoter";

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

struct Dialog {
    message: Message,
    gray: bool,
    watermark_on: bool,
    gif: bool,
    watermark: Entity<Editor>,
    _typing: Subscription,
    /// The picture as last drawn: the file and its preview.
    file: Option<(Arc<Vec<u8>>, Arc<gpui::Image>)>,
    /// What the last drawing was asked with, so a frame asks again only when something changed.
    drawn: Option<Inputs>,
    /// Drawings asked for; an answer for an older one is dropped.
    generation: u64,
}

/// Everything a drawing depends on.
#[derive(Clone, Debug, PartialEq)]
struct Inputs {
    gray: bool,
    watermark: Option<String>,
    gif: bool,
    avatar: Option<String>,
    emojis: Vec<u64>,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

/// A switch the dialog starts from (the original's hidden settings, which remember the dialog's switches).
fn flag(key: &str) -> bool {
    crate::plugins::with(|state| state.value(ID, key) == Value::Bool(true))
}

/// `folder/name`, or `name (1)`, `name (2)`, ... when that is taken (as the app's downloads are named).
fn free_name(folder: &std::path::Path, name: &str) -> std::path::PathBuf {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem.to_owned(), format!(".{extension}")),
        None => (name.to_owned(), String::new()),
    };
    let mut target = folder.join(name);
    let mut copy = 1;
    while target.exists() {
        target = folder.join(format!("{stem} ({copy}){extension}"));
        copy += 1;
    }
    target
}

/// The picture as a file of its own name in a fresh temporary folder, for the upload to read.
fn scratch_file(name: &str, bytes: &[u8]) -> Option<std::path::PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let dir = std::env::temp_dir()
        .join("gravecord-quoter")
        .join(nanos.to_string());
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

/// `fixUpQuote`: user mentions written as `@username`.
fn fix_up_quote(root: &Root, quote: &str) -> String {
    let mut out = String::with_capacity(quote.len());
    let mut rest = quote;
    while let Some(start) = rest.find("<@") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let digits = tail.strip_prefix('!').unwrap_or(tail);
        let len = digits.bytes().take_while(u8::is_ascii_digit).count();
        let user = (len > 0 && digits[len..].starts_with('>'))
            .then(|| digits[..len].parse::<u64>().ok())
            .flatten()
            .and_then(|id| root.stores.users.get(&id));
        match user {
            Some(user) => {
                out.push('@');
                out.push_str(&user.username);
                rest = &digits[len + 1..];
            }
            None => {
                out.push_str("<@");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The custom emoji's picture key and address (the chat's own).
fn emoji_source(emoji: &CustomEmoji) -> (String, String) {
    let extension = if emoji.animated { "gif" } else { "webp" };
    (
        format!("quoter-emoji:{}:{}", emoji.id, emoji.animated),
        format!(
            "https://cdn.discordapp.com/emojis/{}.{extension}?size=96",
            emoji.id
        ),
    )
}

impl Root {
    /// The message menu's last row: "Quote", for a message with text (`children.push`, a group of its own).
    pub(in crate::ui) fn quoter_items(items: &mut Vec<super::super::menu::Item>, empty: bool) {
        use super::super::menu::{Action, Item};
        if empty || !on() {
            return;
        }
        crate::plugins::note(ID);
        if !items.is_empty() && !matches!(items.last(), Some(Item::Separator)) {
            items.push(Item::Separator);
        }
        items.push(Item::Entry(
            "Quote",
            "plugin_quote",
            Action::PluginQuote,
            false,
        ));
    }

    /// "Quote": opens the dialog for `message`.
    pub(in crate::ui) fn quoter_open(&mut self, message: &Message, cx: &mut Context<Self>) {
        if !on() {
            return;
        }
        let text = crate::plugins::with(|state| {
            state
                .value(ID, "watermark")
                .as_str()
                .unwrap_or_default()
                .to_owned()
        });
        let watermark = cx.new(|cx| {
            let mut editor = Editor::new(super::super::editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder("Watermark text (max 32 characters)".to_owned());
            editor.set_text(
                &text
                    .chars()
                    .take(picture::WATERMARK_MAX)
                    .collect::<String>(),
            );
            editor
        });
        let typing = cx.subscribe(&watermark, |_, editor, _: &EditorEvent, cx| {
            // `maxLength={32}`.
            let text = editor.read(cx).text().to_owned();
            if text.chars().count() > picture::WATERMARK_MAX {
                let kept: String = text.chars().take(picture::WATERMARK_MAX).collect();
                editor.update(cx, |editor, _| editor.set_text(&kept));
            }
            cx.notify();
        });
        // Ask for the avatar at the original's 512 (`getUserAvatarURL(author, true, 512)`).
        let (key, url) = super::super::avatar_source_at(&message.author, 512);
        self.want_image(key, url, cx);
        let (_, emojis) = picture::extract_custom_emojis(&message.content);
        for emoji in &emojis {
            let (key, url) = emoji_source(emoji);
            self.want_image(key, url, cx);
        }
        DIALOG.with(|dialog| {
            *dialog.borrow_mut() = Some(Dialog {
                message: message.clone(),
                gray: flag("grayscale"),
                watermark_on: flag("showWatermark"),
                gif: flag("saveAsGif"),
                watermark,
                _typing: typing,
                file: None,
                drawn: None,
                generation: 0,
            })
        });
        cx.notify();
    }

    fn quoter_close(cx: &mut Context<Self>) {
        DIALOG.with(|dialog| *dialog.borrow_mut() = None);
        cx.notify();
    }

    /// The avatar file to draw: the 512 one when loaded, else the chat's.
    fn quoter_avatar(&self, message: &Message) -> Option<(String, Arc<gpui::Image>)> {
        [
            super::super::avatar_source_at(&message.author, 512).0,
            super::super::avatar_source(&message.author).0,
        ]
        .into_iter()
        .find_map(|key| self.images.get(&key).cloned().map(|image| (key, image)))
    }

    /// Asks for a drawing when what it depends on changed since the last one.
    fn quoter_draw(&self, cx: &mut Context<Self>) {
        let Some((message, gray, watermark, gif, drawn, generation)) = DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            let watermark = dialog
                .watermark_on
                .then(|| dialog.watermark.read(cx).text().to_owned());
            Some((
                dialog.message.clone(),
                dialog.gray,
                watermark,
                dialog.gif,
                dialog.drawn.clone(),
                dialog.generation,
            ))
        }) else {
            return;
        };
        let avatar = self.quoter_avatar(&message);
        let quote = fix_up_quote(self, &message.content);
        let (text, emojis) = picture::extract_custom_emojis(&quote);
        let emoji_files: Vec<(u64, Arc<gpui::Image>)> = emojis
            .iter()
            .filter_map(|emoji| {
                let image = self.images.get(&emoji_source(emoji).0)?.clone();
                Some((emoji.id, image))
            })
            .collect();
        let inputs = Inputs {
            gray,
            watermark: watermark.clone(),
            gif,
            avatar: avatar.as_ref().map(|(key, _)| key.clone()),
            emojis: emoji_files.iter().map(|(id, _)| *id).collect(),
        };
        if drawn.as_ref() == Some(&inputs) {
            return;
        }
        let generation = generation + 1;
        DIALOG.with(|dialog| {
            if let Some(dialog) = dialog.borrow_mut().as_mut() {
                dialog.drawn = Some(inputs);
                dialog.generation = generation;
            }
        });
        let name = message
            .author
            .global_name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| message.author.username.clone());
        let username = message.author.username.clone();
        let emoji_ids: Vec<u64> = emojis.iter().map(|emoji| emoji.id).collect();
        let avatar = avatar.map(|(_, image)| image);
        let work = cx.background_executor().spawn(async move {
            let faces = crate::fonts::load_cached()?;
            let regular = Face::parse(faces.first()?)?;
            let italic = Face::parse(faces.get(1)?)?;
            let decode = |image: &gpui::Image| {
                let decoded = image::load_from_memory(image.bytes()).ok()?.to_rgba8();
                let (width, height) = decoded.dimensions();
                Some((decoded.into_raw(), width, height))
            };
            let avatar = avatar.as_deref().and_then(decode);
            let emoji_images: HashMap<u64, (Vec<u8>, u32, u32)> = emoji_files
                .iter()
                .filter_map(|(id, image)| Some((*id, decode(image)?)))
                .collect();
            let rgba = picture::render(
                &regular,
                &italic,
                &Quote {
                    text: &text,
                    name: &name,
                    username: &username,
                    avatar: avatar
                        .as_ref()
                        .map(|(pixels, width, height)| (pixels.as_slice(), *width, *height)),
                    grayscale: gray,
                    watermark: watermark.as_deref(),
                    emoji_ids: &emoji_ids,
                    emoji_images: &emoji_images,
                },
            )?;
            picture::encode(rgba, gif)
        });
        cx.spawn(async move |this, cx| {
            let file = work.await;
            let _ = this.update(cx, |_, cx| {
                DIALOG.with(|dialog| {
                    if let Some(dialog) = dialog.borrow_mut().as_mut()
                        && dialog.generation == generation
                    {
                        dialog.file = file.map(|bytes| {
                            let format = if gif {
                                gpui::ImageFormat::Gif
                            } else {
                                gpui::ImageFormat::Png
                            };
                            let preview = Arc::new(gpui::Image::from_bytes(format, bytes.clone()));
                            (Arc::new(bytes), preview)
                        });
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// The file's name, its bytes and the message, when the picture is drawn.
    fn quoter_file() -> Option<(String, Arc<Vec<u8>>)> {
        DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            let (bytes, _) = dialog.file.clone()?;
            let name = picture::file_name(
                &dialog.message.content,
                &dialog.message.author.username,
                dialog.gif,
            );
            Some((name, bytes))
        })
    }

    /// Export: the picture into Downloads under a free name (a capture run that may not write keeps it in the log).
    fn quoter_export(&mut self, cx: &mut Context<Self>) {
        let Some((name, bytes)) = Self::quoter_file() else {
            return;
        };
        super::super::actions::log_effect("Quoter Export", &name, bytes.len());
        if crate::inert::on() {
            return;
        }
        let saved = std::env::var_os("USERPROFILE").and_then(|home| {
            let folder = std::path::PathBuf::from(home).join("Downloads");
            std::fs::create_dir_all(&folder).ok()?;
            let target = free_name(&folder, &name);
            std::fs::write(&target, &bytes[..]).ok()?;
            Some(target.file_name()?.to_string_lossy().into_owned())
        });
        match saved {
            Some(saved) => self.show_success_toast(format!("Saved {saved} to Downloads"), cx),
            None => self.put_app_toast(format!("Unable to download {name}"), false, cx),
        }
    }

    /// Send: the picture becomes an upload of the open channel's message box, and the dialog closes.
    fn quoter_send(&mut self, cx: &mut Context<Self>) {
        let Some((name, bytes)) = Self::quoter_file() else {
            return;
        };
        if self.channel.is_none() {
            return;
        }
        super::super::actions::log_effect("Quoter Send", &name, bytes.len());
        let Some(path) = scratch_file(&name, &bytes) else {
            return;
        };
        self.attach_files(vec![path], cx);
        Self::quoter_close(cx);
    }

    /// The dialog, while it is open.
    pub(in crate::ui) fn quoter_dialog(
        &self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = DIALOG.with(|dialog| dialog.borrow().is_some());
        if !open {
            return None;
        }
        if !on() {
            DIALOG.with(|dialog| *dialog.borrow_mut() = None);
            return None;
        }
        self.quoter_draw(cx);
        let (gray, watermark_on, gif, editor, preview) = DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            Some((
                dialog.gray,
                dialog.watermark_on,
                dialog.gif,
                dialog.watermark.clone(),
                dialog.file.as_ref().map(|(_, preview)| preview.clone()),
            ))
        })?;
        // `FormSwitch`: the title (and description) on the left, the switch on the right, a divider under it
        // unless `hideBorder`.
        let switch_row = |id: &'static str,
                          title: &'static str,
                          description: Option<&'static str>,
                          value: bool,
                          border: bool,
                          cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex()
                .flex_col()
                .pb(px(16.))
                .mb(px(16.))
                .when(border, |this| {
                    this.border_b_1().border_color(color("--border-subtle"))
                })
                .cursor_pointer()
                .on_click(cx.listener(move |_, _, _, cx| {
                    let key = DIALOG.with(|dialog| {
                        let mut dialog = dialog.borrow_mut();
                        let dialog = dialog.as_mut()?;
                        let (flag, key) = match id {
                            "quoter-gray" => (&mut dialog.gray, "grayscale"),
                            "quoter-gif" => (&mut dialog.gif, "saveAsGif"),
                            _ => (&mut dialog.watermark_on, "showWatermark"),
                        };
                        *flag = !*flag;
                        Some((key, *flag))
                    });
                    // The dialog writes its switches back (`settings.store.grayscale = gray`, ...).
                    if let Some((key, value)) = key {
                        crate::plugins::update(|state| {
                            state.set_value(ID, key, &Value::Bool(value));
                        });
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(super::super::create_channel::switch(value)),
                )
                .children(description.map(|text| {
                    div()
                        .mt(px(8.))
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-subtle"))
                        .child(text)
                }))
        };
        let preview = div()
            .w_full()
            .mb(px(20.))
            .rounded(px(20.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            // The picture's own 2:1; before the first one is drawn, the empty `<img>` takes no room.
            .when_some(preview, |this, preview| {
                this.aspect_ratio(2.).child(img(preview).size_full())
            });
        // The modal's content scrolls when the window is too short for it (the watermark's box comes last).
        let tall = (f32::from(window.viewport_size().height) - 220.).max(200.);
        let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
        let body = div()
            .id("quoter-body")
            .max_h(px(tall))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(preview)
            .child(switch_row("quoter-gray", "Grayscale", None, gray, true, cx))
            .child(switch_row(
                "quoter-gif",
                "Save as GIF",
                Some("Saves/Sends the image as a GIF instead of a PNG"),
                gif,
                true,
                cx,
            ))
            .child(switch_row(
                "quoter-watermark",
                "Show Watermark",
                None,
                watermark_on,
                !watermark_on,
                cx,
            ))
            .when(watermark_on, |this| {
                this.child(
                    div()
                        .cursor_text()
                        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                            window.focus(&focus, cx);
                        })
                        .mt(px(8.))
                        .mb(px(20.))
                        .min_h(px(40.))
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .text_color(color("--text-default"))
                        .text_size(px(16.))
                        .child(editor),
                )
            })
            .into_any_element();
        let spec = modal::Spec {
            id: "quoter-dialog",
            title: SharedString::from("Catch Them In 4K."),
            subtitle: None,
            body: Some(body),
            buttons: vec![
                modal::button(
                    "quoter-export",
                    "Export",
                    modal::Tone::Primary,
                    |this, _, cx| this.quoter_export(cx),
                ),
                modal::button(
                    "quoter-send",
                    "Send",
                    modal::Tone::Primary,
                    |this, _, cx| this.quoter_send(cx),
                ),
            ],
            close: Box::new(|_, _, cx| Self::quoter_close(cx)),
        };
        Some(modal::render(spec, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_sources_are_the_cdn_files() {
        let emoji = CustomEmoji {
            id: 7,
            name: "x".into(),
            animated: true,
        };
        assert_eq!(
            emoji_source(&emoji).1,
            "https://cdn.discordapp.com/emojis/7.gif?size=96"
        );
    }
}
