//! ClipUpload (Equicord `src/equicordplugins/clipUpload.desktop`, GPL-3.0-or-later): the composer's "+" menu ends
//! with "Upload Clip File", which opens "Upload Clip File": Select File (an MP4 or M4V, 500 MB at most) with "Parse clip
//! metadata from file" (the file's own clip box: its game name becomes the title, its users the participants, its game
//! the application), File name, Title, Participants (your friends, with the consent line), Created at, Message,
//! Game/Application (the games seen, or a custom id) and the Spoiler / Remix / Thumbnail boxes; "Created at must be a
//! valid date." while it is not one. Upload stamps a copy of the file with the clip box the client writes
//! (`updateClipMetadata(path, "{}")`), sends it as a clip (the pending reply too) with "Checking clip file." and
//! "Clip uploaded.". Not part of the pinned client.
//!
//! GAP: the FFmpeg retry of a file Discord refuses as "not a valid clip" (50174): there is no FFmpeg here, so that
//! upload ends with the original's "This file is not compatible..." text; the clip upload button on a clip attached
//! in the composer (the app attaches no clips there: a clip is shared from the Clips Gallery); the participant and
//! game selects are drawn as lists under their search fields instead of drop-downs.

use super::super::{Root, color, modal};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::clip_upload as clip;
use dc_state::Id;
use gpui::AppContext as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    PathPromptOptions, SharedString, StatefulInteractiveElement, Styled, Subscription, div, img,
    px,
};
use serde_json::Value;
use std::cell::RefCell;
use std::path::PathBuf;

/// Where the answer of the upload comes back.
const KEY: &str = "plugin-clip-upload:";
/// `FRIEND` of the relationships store.
const FRIEND: u64 = 1;

/// What a click on a select's option does.
type Pick = Box<dyn Fn(&mut Root, &mut Context<Root>)>;

/// A text field of the dialog.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Field {
    FileName,
    Title,
    CreatedAt,
    Message,
    Application,
    Participants,
}

/// The file chosen: its stamped copy and its name.
struct Picked {
    name: String,
    stamped: PathBuf,
}

struct Dialog {
    channel: Id,
    editors: Vec<(Field, Entity<Editor>, Subscription)>,
    file: Option<Picked>,
    participants: Vec<String>,
    spoiler: bool,
    remix: bool,
    thumbnail: bool,
    parse_metadata: bool,
    uploading: bool,
    /// The chosen file is being read and stamped.
    reading: bool,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("ClipUpload")
}

fn offset_minutes() -> i64 {
    i64::from(chrono::Local::now().offset().local_minus_utc()) / 60
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The stamped copies live here until the upload is answered or the dialog closes.
fn temp_folder() -> PathBuf {
    std::env::temp_dir().join("gravecord-clip-upload")
}

fn remove_copy(picked: Option<Picked>) {
    if let Some(picked) = picked
        && let Some(folder) = picked.stamped.parent()
        && folder.starts_with(temp_folder())
    {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// `pickClipFile`: the file read, its clip box parsed when asked, and a copy stamped with an empty clip document.
fn prepare(
    path: &std::path::Path,
    parse: bool,
    nonce: u64,
) -> Result<(Picked, Option<clip::Parsed>), String> {
    let failed = || "Couldn't prepare the selected file.".to_owned();
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(failed)?;
    if !clip::allowed(&name) {
        return Err(failed());
    }
    let size = std::fs::metadata(path).map_err(|_| failed())?.len();
    if size == 0 || size > clip::MAX_CLIP_SIZE {
        return Err(failed());
    }
    let bytes = std::fs::read(path).map_err(|_| "Couldn't read the selected file.".to_owned())?;
    let parsed = parse.then(|| clip::parse_metadata(&bytes)).flatten();
    let folder = temp_folder().join(nonce.to_string());
    std::fs::create_dir_all(&folder).map_err(|_| failed())?;
    let stamped = folder.join(&name);
    let written = std::fs::write(&stamped, &bytes)
        .and_then(|()| dc_clips::mp4::replace_metadata(&stamped, b"{}"));
    if written.is_err() {
        let _ = std::fs::remove_dir_all(&folder);
        return Err("Couldn't update clip metadata.".to_owned());
    }
    Ok((Picked { name, stamped }, parsed))
}

impl Root {
    /// The "+" menu's row (`channel-attach`).
    pub(in crate::ui) fn clip_upload_plus_label(&self) -> Option<&'static str> {
        (on() && self.channel.is_some()).then_some("Upload Clip File")
    }

    fn clip_upload_editor(
        field: Field,
        text: &str,
        placeholder: &str,
        single: bool,
        cx: &mut Context<Self>,
    ) -> (Field, Entity<Editor>, Subscription) {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(super::super::editor_style(), cx);
            editor.set_single_line(single);
            editor.set_placeholder(placeholder.to_owned());
            editor.set_text(text);
            editor
        });
        let sub = cx.subscribe(&editor, |_, _, _: &EditorEvent, cx| cx.notify());
        (field, editor, sub)
    }

    /// `openUploadClipFileModal`.
    pub(in crate::ui) fn clip_upload_open(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        let created = clip::datetime_local(now_ms(), offset_minutes());
        let editors = vec![
            Self::clip_upload_editor(
                Field::FileName,
                clip::DEFAULT_FILE_NAME,
                "my_clip.mp4",
                true,
                cx,
            ),
            Self::clip_upload_editor(Field::Title, "", "Epic Moment", true, cx),
            Self::clip_upload_editor(Field::Participants, "", "Select friends", true, cx),
            Self::clip_upload_editor(Field::CreatedAt, &created, "YYYY-MM-DDTHH:MM", true, cx),
            Self::clip_upload_editor(Field::Message, "", "Check out this clip!", false, cx),
            Self::clip_upload_editor(Field::Application, "", "Custom application ID", true, cx),
        ];
        let old = DIALOG.with(|dialog| {
            dialog.borrow_mut().replace(Dialog {
                channel,
                editors,
                file: None,
                participants: Vec::new(),
                spoiler: false,
                remix: false,
                thumbnail: false,
                parse_metadata: false,
                uploading: false,
                reading: false,
            })
        });
        remove_copy(old.and_then(|old| old.file));
        cx.notify();
    }

    fn clip_upload_close(&mut self, cx: &mut Context<Self>) {
        let old = DIALOG.with(|dialog| dialog.borrow_mut().take());
        // An upload under way keeps its copy until it is answered.
        if let Some(old) = old
            && !old.uploading
        {
            remove_copy(old.file);
        }
        cx.notify();
    }

    fn clip_upload_text(&self, field: Field, cx: &Context<Self>) -> String {
        DIALOG
            .with(|dialog| {
                dialog.borrow().as_ref().and_then(|dialog| {
                    dialog
                        .editors
                        .iter()
                        .find(|(known, _, _)| *known == field)
                        .map(|(_, editor, _)| editor.read(cx).text().to_owned())
                })
            })
            .unwrap_or_default()
    }

    fn clip_upload_set_text(&mut self, field: Field, text: &str, cx: &mut Context<Self>) {
        let editor = DIALOG.with(|dialog| {
            dialog.borrow().as_ref().and_then(|dialog| {
                dialog
                    .editors
                    .iter()
                    .find(|(known, _, _)| *known == field)
                    .map(|(_, editor, _)| editor.clone())
            })
        });
        if let Some(editor) = editor {
            editor.update(cx, |editor, _| editor.set_text(text));
        }
    }

    /// `chooseClipFile`.
    fn clip_upload_choose(&mut self, cx: &mut Context<Self>) {
        let parse =
            DIALOG.with(|dialog| dialog.borrow().as_ref().map(|dialog| dialog.parse_metadata));
        let Some(parse) = parse else {
            return;
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select clip file".into()),
        });
        let nonce = now_ms().unsigned_abs();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |_, cx| {
                DIALOG.with(|dialog| {
                    if let Some(dialog) = dialog.borrow_mut().as_mut() {
                        dialog.reading = true;
                    }
                });
                cx.notify();
            });
            let prepared = cx
                .background_executor()
                .spawn(async move { prepare(&path, parse, nonce) })
                .await;
            let _ = this.update(cx, |this, cx| this.clip_upload_picked(prepared, cx));
        })
        .detach();
    }

    fn clip_upload_picked(
        &mut self,
        prepared: Result<(Picked, Option<clip::Parsed>), String>,
        cx: &mut Context<Self>,
    ) {
        let open = DIALOG.with(|dialog| {
            let mut dialog = dialog.borrow_mut();
            dialog
                .as_mut()
                .map(|dialog| dialog.reading = false)
                .is_some()
        });
        let (picked, parsed) = match prepared {
            Ok(done) if open => done,
            Ok((picked, _)) => {
                remove_copy(Some(picked));
                return;
            }
            Err(message) => {
                self.put_app_toast(message, false, cx);
                cx.notify();
                return;
            }
        };
        let name = picked.name.clone();
        let old = DIALOG.with(|dialog| {
            dialog
                .borrow_mut()
                .as_mut()
                .and_then(|dialog| dialog.file.replace(picked))
        });
        remove_copy(old);
        if self.clip_upload_text(Field::FileName, cx) == clip::DEFAULT_FILE_NAME {
            self.clip_upload_set_text(Field::FileName, &name, cx);
        }
        let parsed = parsed.unwrap_or_default();
        let title = self.clip_upload_text(Field::Title, cx);
        let title = parsed
            .application_name
            .clone()
            .or_else(|| (!title.is_empty()).then_some(title.clone()))
            .unwrap_or_else(|| clip::title_from_name(&name));
        self.clip_upload_set_text(Field::Title, &title, cx);
        if !parsed.users.is_empty() {
            DIALOG.with(|dialog| {
                if let Some(dialog) = dialog.borrow_mut().as_mut()
                    && dialog.participants.is_empty()
                {
                    dialog.participants = parsed.users.clone();
                }
            });
        }
        if let Some(app) = parsed.application_id
            && self.clip_upload_text(Field::Application, cx).is_empty()
        {
            self.clip_upload_set_text(Field::Application, &app, cx);
        }
        cx.notify();
    }

    /// `canUpload`.
    fn clip_upload_ready(&self, cx: &Context<Self>) -> bool {
        let ready = DIALOG.with(|dialog| {
            dialog
                .borrow()
                .as_ref()
                .is_some_and(|dialog| dialog.file.is_some() && !dialog.uploading)
        });
        ready
            && !self.clip_upload_text(Field::FileName, cx).trim().is_empty()
            && !self.clip_upload_text(Field::Title, cx).trim().is_empty()
            && clip::parse_datetime(
                &self.clip_upload_text(Field::CreatedAt, cx),
                offset_minutes(),
            )
            .is_some()
    }

    /// `submit`.
    fn clip_upload_submit(&mut self, cx: &mut Context<Self>) {
        if !self.clip_upload_ready(cx) {
            return;
        }
        let Some(created) = clip::parse_datetime(
            &self.clip_upload_text(Field::CreatedAt, cx),
            offset_minutes(),
        ) else {
            return;
        };
        let application = self
            .clip_upload_text(Field::Application, cx)
            .trim()
            .to_owned();
        let options = DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            Some((
                dialog.channel,
                dialog.file.as_ref()?.stamped.clone(),
                dialog.participants.clone(),
                dialog.spoiler,
                dialog.remix,
                dialog.thumbnail,
            ))
        });
        let Some((channel, path, participants, spoiler, remix, thumbnail)) = options else {
            return;
        };
        let options = clip::Options {
            file_name: self.clip_upload_text(Field::FileName, cx).trim().to_owned(),
            participants,
            title: self.clip_upload_text(Field::Title, cx).trim().to_owned(),
            spoiler,
            remix,
            thumbnail,
            created_at: clip::iso(created),
            message: self.clip_upload_text(Field::Message, cx),
            application_id: (!application.is_empty()).then_some(application),
        };
        DIALOG.with(|dialog| {
            if let Some(dialog) = dialog.borrow_mut().as_mut() {
                dialog.uploading = true;
            }
        });
        // `DELETE_PENDING_REPLY`, its reference sent with the clip.
        let reply = self
            .replying
            .take()
            .filter(|_| self.channel == Some(channel))
            .map(|message| (self.guild, message.id));
        self.put_app_toast("Checking clip file.".to_owned(), false, cx);
        let _ = self.requests.send(crate::net::Request::PluginClipUpload {
            key: format!("{KEY}{channel}"),
            channel,
            path,
            options,
            reply,
        });
        cx.notify();
    }

    /// The upload was answered. True when the key was the plugin's.
    pub(in crate::ui) fn clip_upload_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if !key.starts_with(KEY) {
            return false;
        }
        let ok = body["ok"].as_bool() == Some(true);
        // The stamped copy goes once the clip is sent, or once nobody can send it again (the dialog closed meanwhile).
        let open = DIALOG.with(|dialog| dialog.borrow().is_some());
        if (ok || !open)
            && let Some(path) = body["path"].as_str()
        {
            remove_copy(Some(Picked {
                name: String::new(),
                stamped: PathBuf::from(path),
            }));
        }
        if ok {
            self.put_app_toast("Clip uploaded.".to_owned(), true, cx);
            DIALOG.with(|dialog| dialog.borrow_mut().take());
        } else {
            let message = body["message"]
                .as_str()
                .filter(|message| !message.is_empty())
                .unwrap_or("Failed to upload clip.")
                .to_owned();
            self.put_app_toast(message, false, cx);
            DIALOG.with(|dialog| {
                if let Some(dialog) = dialog.borrow_mut().as_mut() {
                    dialog.uploading = false;
                }
            });
        }
        cx.notify();
        true
    }

    /// `getUserLabel`.
    fn clip_upload_label(user: &dc_state::User) -> String {
        match &user.global_name {
            Some(name) => format!("{name} (@{})", user.username),
            None if user.discriminator != "0" && !user.discriminator.is_empty() => {
                format!("{}#{}", user.username, user.discriminator)
            }
            None => user.username.clone(),
        }
    }

    /// The participant options: friends who are not bots, by label, then chosen ids that are not friends.
    fn clip_upload_people(
        &self,
        chosen: &[String],
    ) -> Vec<(String, String, Option<dc_state::User>)> {
        let mut people: Vec<(String, String, Option<dc_state::User>)> = self
            .stores
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == FRIEND)
            .filter_map(|(id, _)| self.stores.users.get(id))
            .filter(|user| !user.bot && !user.system)
            .map(|user| {
                (
                    Self::clip_upload_label(user),
                    user.id.to_string(),
                    Some(user.clone()),
                )
            })
            .collect();
        people.sort_by(|a, b| a.0.cmp(&b.0));
        for id in chosen {
            if !people.iter().any(|(_, known, _)| known == id) {
                people.push((id.clone(), id.clone(), None));
            }
        }
        people
    }

    /// `getApplicationOptions`: the games running and seen (with their ids), by label.
    fn clip_upload_games(&self, selected: &str) -> Vec<(String, String)> {
        let mut games: Vec<(String, String)> = Vec::new();
        let store = &self.detect.store;
        let seen = store.games_seen(true);
        let all = store
            .visible_running_games()
            .into_iter()
            .chain(store.running_games())
            .chain(seen.iter())
            .chain(store.candidate_games());
        for game in all {
            if let Some(id) = &game.id
                && !games.iter().any(|(_, known)| known == id)
            {
                let name = game.name.clone().unwrap_or_default();
                games.push((format!("{name} ({id})"), id.clone()));
            }
        }
        if !selected.is_empty() && !games.iter().any(|(_, known)| known == selected) {
            games.push((selected.to_owned(), selected.to_owned()));
        }
        games.sort_by(|a, b| a.0.cmp(&b.0));
        games
    }

    /// `UploadClipFileModal`, while open.
    pub(in crate::ui) fn clip_upload_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (editors, file_name, participants, flags, parse, uploading, reading) =
            DIALOG.with(|dialog| {
                let dialog = dialog.borrow();
                let dialog = dialog.as_ref()?;
                Some((
                    dialog
                        .editors
                        .iter()
                        .map(|(field, editor, _)| (*field, editor.clone()))
                        .collect::<Vec<_>>(),
                    dialog.file.as_ref().map(|file| file.name.clone()),
                    dialog.participants.clone(),
                    (dialog.spoiler, dialog.remix, dialog.thumbnail),
                    dialog.parse_metadata,
                    dialog.uploading,
                    dialog.reading,
                ))
            })?;
        let editor = |field: Field| {
            editors
                .iter()
                .find(|(known, _)| *known == field)
                .map(|(_, editor)| editor.clone())
        };
        let heading = |text: &str| {
            div()
                .mb(px(8.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-muted"))
                .child(text.to_uppercase())
        };
        let field_box = |child: AnyElement, tall: bool| {
            div()
                .min_h(px(if tall { 80. } else { 40. }))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .text_color(color("--text-default"))
                .text_size(px(16.))
                .when(uploading, |this| this.opacity(0.5))
                .child(child)
        };
        let text_section =
            |title: &str, field: Field, tall: bool| {
                div().flex().flex_col().child(heading(title)).children(
                    editor(field).map(|editor| field_box(editor.into_any_element(), tall)),
                )
            };
        let check_row = |id: &'static str, label: &str, checked: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(16.))
                .text_color(color("--text-default"))
                .when(uploading, |this| this.opacity(0.5))
                .when(!uploading, |this| this.cursor_pointer())
                .child(super::super::server_games::checkbox(checked))
                .child(label.to_owned())
                .when(!uploading, |this| {
                    this.on_click(cx.listener(move |_, _, _, cx| {
                        DIALOG.with(|dialog| {
                            if let Some(dialog) = dialog.borrow_mut().as_mut() {
                                let flag = match id {
                                    "clip-upload-parse" => &mut dialog.parse_metadata,
                                    "clip-upload-spoiler" => &mut dialog.spoiler,
                                    "clip-upload-remix" => &mut dialog.remix,
                                    _ => &mut dialog.thumbnail,
                                };
                                *flag = !*flag;
                            }
                        });
                        cx.notify();
                    }))
                })
        };
        let created = self.clip_upload_text(Field::CreatedAt, cx);
        let notice = (!created.is_empty()
            && clip::parse_datetime(&created, offset_minutes()).is_none())
        .then(|| {
            div()
                .p(px(12.))
                .rounded(px(8.))
                .bg(color("--background-feedback-mod-critical"))
                .text_color(color("--text-default"))
                .text_size(px(14.))
                .child("Created at must be a valid date.")
        });
        let select_file = div()
            .id("clip-upload-select")
            .h(px(32.))
            .px(px(16.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .text_size(px(14.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .bg(color("--control-primary-background-default"))
            .when(uploading || reading, |this| this.opacity(0.5))
            .when(!uploading && !reading, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.clip_upload_choose(cx)))
            })
            .child("Select File");
        let file_section = div()
            .flex()
            .flex_col()
            .child(heading("File"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(select_file)
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(color("--text-default"))
                            .child(file_name.unwrap_or_else(|| "No file selected".to_owned())),
                    ),
            )
            .child(div().mt(px(8.)).child(check_row(
                "clip-upload-parse",
                "Parse clip metadata from file",
                parse,
                cx,
            )));
        // Participants: the search field, the chosen people, then the options it matches.
        let query = self
            .clip_upload_text(Field::Participants, cx)
            .to_lowercase();
        let people = self.clip_upload_people(&participants);
        let option_row = |id: SharedString,
                          label: String,
                          picture: Option<gpui::ImageSource>,
                          chosen: bool,
                          run: Pick,
                          cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(32.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(4.))
                .text_size(px(14.))
                .text_color(color(if chosen {
                    "--text-strong"
                } else {
                    "--text-default"
                }))
                .when(chosen, |this| this.bg(color("--background-mod-normal")))
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .when(!uploading, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            run(this, cx);
                            cx.notify();
                        }))
                })
                .children(
                    picture.map(|picture| img(picture).size(px(24.)).flex_none().rounded_full()),
                )
                .child(div().flex_1().min_w(px(0.)).truncate().child(label))
        };
        let picture_of = |user: &Option<dc_state::User>| {
            user.as_ref().and_then(|user| {
                self.images
                    .get(&super::super::avatar_source(user).0)
                    .cloned()
                    .map(gpui::ImageSource::from)
            })
        };
        let mut people_rows: Vec<AnyElement> = Vec::new();
        for (label, id, user) in &people {
            if !query.is_empty()
                && !label.to_lowercase().contains(&query)
                && !participants.contains(id)
            {
                continue;
            }
            let chosen = participants.contains(id);
            let toggled = id.clone();
            people_rows.push(
                option_row(
                    SharedString::from(format!("clip-upload-person-{id}")),
                    label.clone(),
                    picture_of(user),
                    chosen,
                    Box::new(move |_, _| {
                        DIALOG.with(|dialog| {
                            if let Some(dialog) = dialog.borrow_mut().as_mut() {
                                match dialog
                                    .participants
                                    .iter()
                                    .position(|known| *known == toggled)
                                {
                                    Some(at) => {
                                        dialog.participants.remove(at);
                                    }
                                    None => dialog.participants.push(toggled.clone()),
                                }
                            }
                        });
                    }),
                    cx,
                )
                .into_any_element(),
            );
        }
        let participants_section = div()
            .flex()
            .flex_col()
            .child(heading("Participants"))
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child("Only tag people who were present and have consented to being in this clip."),
            )
            .children(editor(Field::Participants).map(|editor| field_box(editor.into_any_element(), false)))
            .child(
                div()
                    .id("clip-upload-people")
                    .mt(px(4.))
                    .max_h(px(160.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(people_rows),
            );
        let application = self.clip_upload_text(Field::Application, cx);
        let mut game_rows: Vec<AnyElement> = Vec::new();
        for (label, id) in self.clip_upload_games(application.trim()) {
            let chosen = application.trim() == id;
            let picked = id.clone();
            game_rows.push(
                option_row(
                    SharedString::from(format!("clip-upload-game-{id}")),
                    label,
                    None,
                    chosen,
                    // A click on the chosen game clears it (`clearable`).
                    Box::new(move |this, cx| {
                        let text = if chosen {
                            String::new()
                        } else {
                            picked.clone()
                        };
                        this.clip_upload_set_text(Field::Application, &text, cx);
                    }),
                    cx,
                )
                .into_any_element(),
            );
        }
        let application_section = div()
            .flex()
            .flex_col()
            .child(heading("Game/Application"))
            .child(
                div()
                    .id("clip-upload-games")
                    .max_h(px(128.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .when(game_rows.is_empty(), |this| {
                        this.child(
                            div()
                                .h(px(32.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .text_color(color("--text-muted"))
                                .child("Select a game"),
                        )
                    })
                    .children(game_rows),
            )
            .children(editor(Field::Application).map(|editor| {
                div()
                    .mt(px(8.))
                    .child(field_box(editor.into_any_element(), false))
            }));
        let (spoiler, remix, thumbnail) = flags;
        let body = div()
            .id("clip-upload-body")
            .max_h(px(440.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(12.))
            .children(notice)
            .child(file_section)
            .child(text_section("File name", Field::FileName, false))
            .child(text_section("Title", Field::Title, false))
            .child(participants_section)
            .child(text_section("Created at", Field::CreatedAt, false))
            .child(text_section("Message", Field::Message, true))
            .child(application_section)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(check_row("clip-upload-spoiler", "Spoiler", spoiler, cx))
                    .child(check_row("clip-upload-remix", "Remix", remix, cx))
                    .child(check_row(
                        "clip-upload-thumbnail",
                        "Thumbnail",
                        thumbnail,
                        cx,
                    )),
            )
            .into_any_element();
        let ready = self.clip_upload_ready(cx);
        let spec = modal::Spec {
            id: "clip-upload-dialog",
            title: SharedString::from("Upload Clip File"),
            subtitle: None,
            body: Some(body),
            buttons: vec![
                modal::button(
                    "clip-upload-cancel",
                    "Cancel",
                    modal::Tone::Secondary,
                    move |this, _, cx| {
                        if !uploading {
                            this.clip_upload_close(cx);
                        }
                    },
                ),
                modal::button(
                    "clip-upload-send",
                    if uploading { "Uploading" } else { "Upload" },
                    if ready {
                        modal::Tone::Primary
                    } else {
                        modal::Tone::PrimaryDisabled
                    },
                    |this, _, cx| this.clip_upload_submit(cx),
                ),
            ],
            close: Box::new(|this, _, cx| this.clip_upload_close(cx)),
        };
        Some(modal::render(spec, cx))
    }

    /// The people's pictures, asked for while the dialog is open.
    pub(in crate::ui) fn clip_upload_prepare(&mut self) {
        if !DIALOG.with(|dialog| dialog.borrow().is_some()) {
            return;
        }
        let wanted: Vec<(String, String)> = self
            .stores
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == FRIEND)
            .filter_map(|(id, _)| self.stores.users.get(id))
            .map(super::super::avatar_source)
            .filter(|(key, _)| !self.images.contains_key(key))
            .collect();
        for (key, url) in wanted {
            self.request_image(key, url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_file_is_stamped_in_a_copy() {
        let folder = std::env::temp_dir().join(format!("clip-upload-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("Moment.mp4");
        // Two top-level boxes and an old clip box, as a clip file ends.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0, 0, 0, 16]);
        bytes.extend_from_slice(b"ftypisom\0\0\0\0");
        bytes.extend_from_slice(&[0, 0, 0, 8]);
        bytes.extend_from_slice(b"free");
        let mut old = vec![0, 0, 0, 0];
        old.extend_from_slice(b"uuid");
        old.extend_from_slice(&dc_clips::mp4::METADATA_UUID);
        old.extend_from_slice(br#"{"applicationName":"Game","users":["3"]}"#);
        let length = old.len() as u32;
        old[..4].copy_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&old);
        std::fs::write(&source, &bytes).unwrap();
        let (picked, parsed) = prepare(&source, true, 1).unwrap();
        let parsed = parsed.unwrap();
        assert_eq!(parsed.application_name.as_deref(), Some("Game"));
        assert_eq!(parsed.users, ["3"]);
        let stamped = std::fs::read(&picked.stamped).unwrap();
        assert_eq!(&stamped[..24], &bytes[..24]);
        assert!(stamped.ends_with(b"{}"));
        assert_eq!(
            clip::parse_metadata(&stamped),
            Some(clip::Parsed::default())
        );
        // The original is left as it was.
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        assert!(prepare(&folder.join("Moment.mov"), false, 2).is_err());
        remove_copy(Some(picked));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
