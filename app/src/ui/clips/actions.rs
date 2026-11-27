//! What the editor's header and menu do to a clip: delete (module 590146 `Delete` item and the confirm modal),
//! export the trimmed video to a file (`Export` > `Video File`) and show the file in its folder. Strings are the
//! official ones: `Lzhuiz` "Delete this clip?", `yxCBqo` the warning, `oMoKCq` "Exporting your Clip".
use super::super::keyboard::{Layer, outside};
use super::super::modal::{self, Tone};
use super::super::{Root, color, icon};
use super::editor::Editor;
use dc_clips::mp4;
use gpui::{
    AnyElement, Context, FontWeight, SharedString, Window, anchored, deferred, div, prelude::*, px,
};
use std::path::{Path, PathBuf};

/// The More menu is open (the "More Options" button of the editor's header), or the Clips capture
/// mode context menu (`clips-capture-mode`, module 707116) is open.
#[derive(Default)]
pub(in crate::ui) struct Menu {
    pub open: bool,
    pub capture_mode_open: bool,
    pub allow_voice_in_others_clips: bool,
}

/// Module 707116 (`clips-capture-mode`): formats the Manual Clips subtext (`QWyKXz` with keybind,
/// `Sb+V+3` without) and Auto Clips subtext (`BvKcH9` with keybind, `VWJbxP` without).
pub(crate) fn clips_mode_subtexts(keybind: Option<&str>) -> (String, String) {
    match keybind.filter(|k| !k.trim().is_empty()) {
        Some(kb) => (
            dc_intl::format(
                "QWyKXz",
                &[("keybind", dc_intl::Arg::Str(std::borrow::Cow::Borrowed(kb)))],
            ),
            dc_intl::format(
                "BvKcH9",
                &[("keybind", dc_intl::Arg::Str(std::borrow::Cow::Borrowed(kb)))],
            ),
        ),
        None => (
            dc_intl::t("Sb+V+3").to_owned(),
            dc_intl::t("VWJbxP").to_owned(),
        ),
    }
}

/// How far the exported video's front can be before the handle: the copy starts at the keyframe at or before the
/// start, so it runs from there (`ClipFile::trimmed`).
pub(super) fn keyframe_lead_ms(file: &mp4::ClipFile, start_ms: u64) -> u64 {
    let (starts, _) = file.video_starts_ms();
    starts
        .iter()
        .enumerate()
        .filter(|(index, start)| **start <= start_ms && file.video.samples[*index].sync)
        .map(|(_, start)| start_ms - *start)
        .next_back()
        .unwrap_or(0)
}

/// What an export wrote.
#[derive(Debug)]
pub(super) struct Exported {
    pub path: PathBuf,
    /// Milliseconds the file begins before the start handle (keyframe alignment).
    pub lead_ms: u64,
    pub length_ms: u64,
}

/// A file name for a clip's title: the characters Windows refuses become `_`.
pub(super) fn file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').to_owned();
    if cleaned.is_empty() {
        "Clip".to_owned()
    } else {
        cleaned
    }
}

/// `name.mp4`, or `name (1).mp4`, `name (2).mp4` ... the first that does not exist.
pub(super) fn free_path(folder: &Path, stem: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let first = folder.join(format!("{stem}.mp4"));
    if !exists(&first) {
        return first;
    }
    (1..)
        .map(|n| folder.join(format!("{stem} ({n}).mp4")))
        .find(|path| !exists(path))
        .expect("an unbounded range has a free name")
}

/// Writes the part of `source` between the handles, with the audio the switches keep, into `folder` as a plain video
/// file (the official export holds no clip document either).
pub(super) fn export_trimmed(
    source: &Path,
    start_ms: u64,
    end_ms: u64,
    keep: &[&str],
    folder: &Path,
    stem: &str,
) -> std::io::Result<Exported> {
    let mut reader = std::io::BufReader::new(std::fs::File::open(source)?);
    let file = mp4::read_clip(&mut reader)?;
    let lead_ms = keyframe_lead_ms(&file, start_ms);
    let mut part = file.trimmed(start_ms, end_ms).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the range holds no picture",
        )
    })?;
    part.keep_audio(keep);
    part.fold_audio_to_single_stereo();
    let (_, length_ms) = part.video_starts_ms();
    std::fs::create_dir_all(folder)?;
    let path = free_path(folder, stem, |path| path.exists());
    let partial = path.with_extension("mp4.part");
    let result = (|| {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&partial)?);
        part.write(&mut out, &[])?;
        std::io::Write::flush(&mut out)?;
        std::fs::rename(&partial, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result?;
    Ok(Exported {
        path,
        lead_ms,
        length_ms,
    })
}

/// Where exports go: the Downloads folder (a capture run may point `GRAVECORD_EXPORT_DIR` elsewhere).
fn export_folder() -> Option<PathBuf> {
    #[cfg(feature = "capture")]
    if let Some(dir) = std::env::var_os("GRAVECORD_EXPORT_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Downloads"))
}

impl Root {
    fn clips_open_editor_ref(&self) -> Option<&Editor> {
        self.clips
            .engine
            .ui
            .modal
            .as_ref()
            .and_then(|modal| modal.editor.as_ref())
    }

    /// The trash button: asks first; Shift+click deletes at once (module 590146).
    pub(super) fn clips_ask_delete(&mut self, now: bool, cx: &mut Context<Self>) {
        let Some(id) = self
            .clips_open_editor_ref()
            .map(|editor| editor.clip.meta.id.clone())
        else {
            return;
        };
        self.clips.engine.ui.confirm_delete = Some(id);
        if now {
            self.clips_delete_confirmed(cx);
        }
        cx.notify();
    }

    /// Removes the clip's file and its library entry, and closes the editor without writing anything back.
    pub(super) fn clips_delete_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.clips.engine.ui.confirm_delete.take() else {
            return;
        };
        // Deleting from the editor closes it (and the modal when it was opened into the editor); from the grid the
        // grid stays.
        let from_grid = self
            .clips
            .engine
            .ui
            .modal
            .as_ref()
            .is_some_and(|modal| modal.editor.is_none() && modal.gallery.is_some());
        if let Some(modal) = self.clips.engine.ui.modal.as_mut() {
            modal.editor = None;
        }
        if !from_grid
            && self
                .clips
                .engine
                .ui
                .modal
                .as_ref()
                .is_none_or(|modal| modal.direct)
        {
            self.clips.engine.ui.modal = None;
        }
        self.clips.engine.ui.menu.open = false;
        if let Some(clip) = self.clips.engine.clips.remove(&id)
            && let Err(error) = std::fs::remove_file(&clip.filepath)
        {
            tracing::warn!(
                "clips: deleting {} failed: {error}",
                clip.filepath.display()
            );
        }
        self.clips.engine.new_clip_ids.retain(|new| *new != id);
        if let Some(Some(old)) = self.clips.engine.ui.pictures.borrow_mut().remove(&id) {
            cx.drop_image(old, None);
        }
        cx.notify();
    }

    /// The confirm dialog: "Delete this clip?" with Cancel and Delete (critical).
    pub(in crate::ui) fn clips_delete_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.clips.engine.ui.confirm_delete.as_ref()?;
        Some(modal::render(
            modal::Spec {
                id: "clips-delete-modal",
                title: "Delete this clip?".into(),
                subtitle: Some(
                    div()
                        .child("Deleting a clip cannot be undone. Would you like to delete this clip in Discord & remove the file from your computer?")
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    modal::button("clips-delete-cancel", "Cancel", Tone::Secondary, |this, _, _| {
                        this.clips.engine.ui.confirm_delete = None;
                    }),
                    modal::button("clips-delete-confirm", "Delete", Tone::Critical, |this, _, cx| {
                        this.clips_delete_confirmed(cx);
                    }),
                ],
                close: Box::new(|this, _, _| this.clips.engine.ui.confirm_delete = None),
            },
            cx,
        ))
    }

    pub(super) fn clips_toggle_menu(&mut self, cx: &mut Context<Self>) {
        let open = !self.clips.engine.ui.menu.open;
        if open {
            self.close_all_context_menus();
        }
        self.clips.engine.ui.menu.open = open;
        cx.notify();
    }

    pub(in crate::ui) fn clips_menu_open(&self) -> bool {
        self.clips.engine.ui.menu.open || self.clips_card_menu_open()
    }

    pub(in crate::ui) fn clips_close_menu(&mut self) {
        self.clips.engine.ui.menu.open = false;
        self.clips_close_card_menu();
    }

    /// Exports the trimmed video to the export folder and says so with the official toast.
    pub(super) fn clips_export(&mut self, cx: &mut Context<Self>) {
        self.clips.engine.ui.menu.open = false;
        let Some(editor) = self.clips_open_editor_ref() else {
            return;
        };
        let mut meta = editor.clip.meta.clone();
        meta.name = editor.edit.name.clone();
        let title = super::model::title(&meta);
        let job = ShareClip {
            source: editor.clip.filepath.clone(),
            start_ms: (editor.edit.crop.start * 1000.).round() as u64,
            end_ms: (editor.edit.crop.end * 1000.).round() as u64,
            keep: editor.edit.kept_tracks(),
            stem: file_stem(&title),
            title,
            thumbnail: None,
        };
        self.clips_export_job(job, cx);
    }

    /// Writes a clip's trimmed video to the export folder.
    pub(super) fn clips_export_job(&mut self, job: ShareClip, cx: &mut Context<Self>) {
        let ShareClip {
            source,
            start_ms,
            end_ms,
            keep,
            stem,
            ..
        } = job;
        let Some(folder) = export_folder() else {
            return;
        };
        self.show_app_toast("Exporting your Clip", cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { export_trimmed(&source, start_ms, end_ms, &keep, &folder, &stem) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                this.clips.engine.ui.last_export = match result {
                    Ok(done) => format!(
                        "{} lead {} ms length {} ms",
                        done.path.display(),
                        done.lead_ms,
                        done.length_ms
                    ),
                    Err(error) => format!("failed {error}"),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Show in Folder: opens the file's folder with the file selected. A capture run only notes it.
    pub(super) fn clips_show_in_folder(&mut self, cx: &mut Context<Self>) {
        self.clips.engine.ui.menu.open = false;
        let Some(path) = self
            .clips_open_editor_ref()
            .map(|editor| editor.clip.filepath.clone())
        else {
            return;
        };
        self.clips_show_path(path, cx);
    }

    pub(super) fn clips_show_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.clips.engine.ui.last_export = format!("show {}", path.display());
        #[cfg(all(windows, not(feature = "capture")))]
        {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        }
        cx.notify();
    }

    /// The More menu card under its button: `Export` with `Video File`, then `Show in Folder`.
    pub(in crate::ui) fn clips_menu_layer(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.clips.engine.ui.menu.capture_mode_open {
            return Some(self.render_clips_capture_mode_menu(cx));
        }
        if !self.clips.engine.ui.menu.open || self.clips_open_editor_ref().is_none() {
            return None;
        }
        let at = self
            .tip_anchors
            .borrow()
            .get("clips-more")
            .map(|bounds| bounds.bottom_left() + gpui::point(px(0.), px(4.)))?;
        let row = |id: &'static str, label: &'static str, glyph: &'static str| {
            div()
                .id(SharedString::from(id))
                .h(px(36.))
                .flex_none()
                .p(px(8.))
                .rounded(px(2.))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(color("--interactive-background-hover"))
                        .rounded(px(4.))
                })
                .child(
                    div()
                        .mr(px(8.))
                        .child(icon(glyph, 18., "--interactive-text-default")),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
        };
        let group = div()
            .h(px(24.))
            .flex_none()
            .px(px(8.))
            .flex()
            .items_center()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-muted"))
            .child("Export");
        let card = div()
            .id("clips-menu")
            .occlude()
            .rounded(px(8.))
            .bg(color("--background-surface-higher"))
            .border_1()
            .border_color(color("--border-subtle"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .p(px(8.))
                    .w(px(188.))
                    .flex()
                    .flex_col()
                    .child(group)
                    .child(
                        row("clips-menu-video", "Video File", "clips_download")
                            .on_click(cx.listener(|this, _, _, cx| this.clips_export(cx))),
                    )
                    .child(
                        div()
                            .my(px(8.))
                            .h(px(1.))
                            .flex_none()
                            .bg(color("--border-subtle")),
                    )
                    .child(
                        row("clips-menu-folder", "Show in Folder", "clips_folder")
                            .on_click(cx.listener(|this, _, _, cx| this.clips_show_in_folder(cx))),
                    ),
            );
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(outside(card, Layer::ClipMenu, cx)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// Module 829903 / 243258: formats multi-clip video count label (`SJ6pPX` `"{count} videos"`).
pub(in crate::ui) fn share_clips_count_label(count: usize) -> String {
    dc_intl::format("SJ6pPX", &[("count", dc_intl::Arg::Num(count as f64))])
}

/// What a Share sends (module 243258 `CLIPS_SHARE_MODAL`, module 829903 `shareClips`): the clip's
/// trimmed video (or screenshot), exported when the message goes.
pub(in crate::ui) struct ShareClip {
    pub source: PathBuf,
    pub start_ms: u64,
    pub end_ms: u64,
    pub keep: Vec<&'static str>,
    /// The file name without its extension, and the title shown in the preview.
    pub stem: String,
    pub title: String,
    pub thumbnail: Option<std::sync::Arc<gpui::RenderImage>>,
}

/// Where a shared clip is written before it is uploaded.
fn share_folder() -> PathBuf {
    std::env::temp_dir().join("gravecord-clip-share")
}

impl Root {
    /// The editor's Share button: opens the forward modal for the clip as it is edited.
    pub(super) fn clips_share(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.clips_open_editor_ref() else {
            return;
        };
        let mut meta = editor.clip.meta.clone();
        meta.name = editor.edit.name.clone();
        let title = super::model::title(&meta);
        let share = ShareClip {
            source: editor.clip.filepath.clone(),
            start_ms: (editor.edit.crop.start * 1000.).round() as u64,
            end_ms: (editor.edit.crop.end * 1000.).round() as u64,
            keep: editor.edit.kept_tracks(),
            stem: file_stem(&title),
            title,
            thumbnail: self
                .clips
                .engine
                .ui
                .pictures
                .borrow()
                .get(&editor.clip.meta.id)
                .cloned()
                .flatten(),
        };
        self.share_clips_dispatch(vec![share], None, None, window, cx);
    }

    /// Module 829903 (`shareClips` dispatcher): when `direct_channel` is `Some(ch)` and valid in
    /// stores, directly exports and uploads all clips to that channel (with optional `reply` ref);
    /// otherwise opens the `243258` Share Clip modal with the full `Vec<ShareClip>`.
    pub(in crate::ui) fn share_clips_dispatch(
        &mut self,
        clips: Vec<ShareClip>,
        direct_channel: Option<dc_state::Id>,
        reply: Option<(Option<dc_state::Id>, dc_state::Id, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if clips.is_empty() {
            return;
        }
        let valid_direct = direct_channel.filter(|&cid| {
            self.stores.private_channels.iter().any(|c| c.id == cid)
                || self
                    .stores
                    .guilds
                    .iter()
                    .any(|g| g.channels.iter().any(|c| c.id == cid))
        });
        if let Some(channel) = valid_direct {
            self.clips_send_share_batch(clips, vec![channel], String::new(), reply, cx);
        } else {
            self.open_share_clips(clips, window, cx);
        }
    }

    /// Exports one or more clips (module 829903 `shareClips`) to temporary files, then sends them
    /// with the optional note & reply reference to each destination channel.
    pub(in crate::ui) fn clips_send_share_batch(
        &mut self,
        shares: Vec<ShareClip>,
        channels: Vec<dc_state::Id>,
        note: String,
        reply: Option<(Option<dc_state::Id>, dc_state::Id, bool)>,
        cx: &mut Context<Self>,
    ) {
        let folder = share_folder();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Earlier shares leave their files behind once uploaded; a day is long enough.
                    if let Ok(entries) = std::fs::read_dir(&folder) {
                        for entry in entries.flatten() {
                            let old = entry
                                .metadata()
                                .and_then(|meta| meta.modified())
                                .ok()
                                .and_then(|time| time.elapsed().ok())
                                .is_some_and(|age| age > std::time::Duration::from_secs(86_400));
                            if old {
                                let _ = std::fs::remove_file(entry.path());
                            }
                        }
                    }
                    let mut exported = Vec::with_capacity(shares.len());
                    for ShareClip {
                        source,
                        start_ms,
                        end_ms,
                        keep,
                        stem,
                        ..
                    } in shares
                    {
                        exported.push(export_trimmed(
                            &source, start_ms, end_ms, &keep, &folder, &stem,
                        )?);
                    }
                    Ok::<Vec<Exported>, std::io::Error>(exported)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(done_list) => {
                        if let Some(first) = done_list.first() {
                            this.clips.engine.ui.last_export =
                                format!("shared {}", first.path.display());
                        }
                        let uploads: Vec<crate::net::Upload> = done_list
                            .into_iter()
                            .map(|done| {
                                let name = done
                                    .path
                                    .file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                crate::net::Upload {
                                    path: done.path,
                                    name,
                                    description: None,
                                }
                            })
                            .collect();
                        for channel in channels {
                            let _ = this.requests.send(crate::net::Request::Send {
                                channel,
                                content: note.clone(),
                                files: uploads.clone(),
                                reply,
                                context: None,
                            });
                        }
                    }
                    Err(error) => {
                        this.clips.engine.ui.last_export = format!("share failed {error}");
                        // `iufib1` (module 829903).
                        this.show_app_toast(dc_intl::t("iufib1"), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        self.close_clips_modal(cx);
    }
}

impl Root {
    /// The preview of Share Clip (`forwardPreview` in stylesheet N34jMW; module 243258 + 829903):
    /// shows single clip title or `SJ6pPX` `"{count} videos"` for multi-clip batches, with a 56 px
    /// thumbnail and `+N` overflow badge when more than one clip is attached.
    pub(in crate::ui) fn clips_share_preview(&self, clips: &[ShareClip]) -> AnyElement {
        let Some(first) = clips.first() else {
            return div().into_any_element();
        };
        let extra = clips.len().saturating_sub(1);
        let label = if extra > 0 {
            share_clips_count_label(clips.len())
        } else {
            first.title.clone()
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div().flex_1().min_w(px(0.)).py(px(4.)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icon("clips_clips", 16., "--text-muted"))
                        .child(
                            div()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(label),
                        ),
                ),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(56.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(gpui::black())
                    .children(first.thumbnail.clone().map(|picture| {
                        gpui::img(picture)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            .opacity(0.6)
                    }))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("clips_play", 20., "--white")),
                    )
                    .when(extra > 0, |tile| {
                        tile.child(
                            div()
                                .absolute()
                                .right(px(4.))
                                .bottom(px(4.))
                                .px(px(5.))
                                .py(px(1.))
                                .rounded(px(4.))
                                .bg(gpui::rgba(0x000000cc))
                                .text_size(px(11.))
                                .font_weight(FontWeight(700.))
                                .text_color(gpui::rgba(0xffffffff))
                                .child(format!("+{extra}")),
                        )
                    }),
            )
            .into_any_element()
    }

    /// Module 707116 (`clips-capture-mode`): 3-group context menu:
    /// 1. `"Clips capture mode"` (`eIX9be`): Off (`uO/2T5`), Manual (`bBlUzK` + `QWyKXz`/`Sb+V+3`),
    ///    Manual + Auto (`4gzBi+` + `BvKcH9`/`VWJbxP`) radios backed by `clips_capture_mode()`.
    /// 2. Voice checkbox (`IxZyA+` "Use my voice in others' Clips", subtext `t23RLK`).
    /// 3. `"Clips Settings"` (`J1hsY2`) with gear icon opening User Settings > Clips.
    fn render_clips_capture_mode_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let (enabled, auto) = self.clips.clips_capture_mode();
        let mode: u8 = if !enabled {
            0
        } else if !auto {
            1
        } else {
            2
        };
        let voice_on = self.clips.engine.ui.menu.allow_voice_in_others_clips;
        let (manual_sub, auto_sub) = clips_mode_subtexts(Some("Alt+C"));

        let radio_row = |id: &'static str,
                         title: &'static str,
                         sub: Option<String>,
                         selected: bool,
                         next_enabled: bool,
                         next_auto: bool| {
            div()
                .id(id)
                .px(px(8.))
                .py(px(8.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(color("--interactive-background-hover")))
                .flex()
                .items_start()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .when_some(sub, |col, text| {
                            col.child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(text),
                            )
                        }),
                )
                .child(
                    div()
                        .mt(px(2.))
                        .size(px(18.))
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(color(if selected {
                            "--background-brand"
                        } else {
                            "--interactive-icon-default"
                        }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected, |dot| {
                            dot.child(
                                div()
                                    .size(px(8.))
                                    .rounded_full()
                                    .bg(color("--background-brand")),
                            )
                        }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.clips.set_clips_capture_mode(next_enabled, next_auto);
                    cx.notify();
                }))
        };

        let card = div()
            .id("clips-capture-mode")
            .occlude()
            .on_click(|_, _, cx| cx.stop_propagation())
            .w(px(300.))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::t("eIX9be")),
            )
            .child(radio_row(
                "clips-capture-mode-off",
                dc_intl::t("uO/2T5"),
                None,
                mode == 0,
                false,
                false,
            ))
            .child(radio_row(
                "clips-capture-mode-manual",
                dc_intl::t("bBlUzK"),
                Some(manual_sub),
                mode == 1,
                true,
                false,
            ))
            .child(radio_row(
                "clips-capture-mode-auto",
                dc_intl::t("4gzBi+"),
                Some(auto_sub),
                mode == 2,
                true,
                true,
            ))
            .child(div().my(px(4.)).h(px(1.)).bg(color("--border-subtle")))
            .child(
                div()
                    .id("clips-capture-mode-voice")
                    .px(px(8.))
                    .py(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("IxZyA+")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("t23RLK")),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .size(px(18.))
                            .flex_none()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(color(if voice_on {
                                "--background-brand"
                            } else {
                                "--interactive-icon-default"
                            }))
                            .when(voice_on, |b| {
                                b.bg(color("--background-brand"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("circle_check", 14., "--white"))
                            }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clips.engine.ui.menu.allow_voice_in_others_clips =
                            !this.clips.engine.ui.menu.allow_voice_in_others_clips;
                        cx.notify();
                    })),
            )
            .child(div().my(px(4.)).h(px(1.)).bg(color("--border-subtle")))
            .child(
                div()
                    .id("clips-capture-mode-settings")
                    .h(px(36.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("settings", 18., "--interactive-text-default"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("J1hsY2")),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clips.engine.ui.menu.capture_mode_open = false;
                        this.open_settings_section("Clips", cx);
                    })),
            );

        deferred(
            div()
                .id("clips-capture-mode-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::rgba(0x00000044))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clips.engine.ui.menu.capture_mode_open = false;
                    cx.notify();
                }))
                .child(card),
        )
        .with_priority(4)
        .into_any_element()
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_clips_capture_mode(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        self.clips.engine.ui.menu.capture_mode_open = !arg.eq_ignore_ascii_case("close");
        if arg.eq_ignore_ascii_case("auto") {
            self.clips.set_clips_capture_mode(true, true);
            self.clips.engine.ui.menu.allow_voice_in_others_clips = true;
        }
        cx.notify();
        format!(
            "clips-capture-mode: open={}",
            self.clips.engine.ui.menu.capture_mode_open
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_lose_what_windows_refuses() {
        assert_eq!(file_stem("Clip: a/b?"), "Clip_ a_b_");
        assert_eq!(file_stem("  "), "Clip");
        assert_eq!(file_stem("end."), "end");
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let folder = Path::new("out");
        let taken = [folder.join("a.mp4"), folder.join("a (1).mp4")];
        assert_eq!(
            free_path(folder, "a", |path| taken.iter().any(|t| t == path)),
            folder.join("a (2).mp4")
        );
        assert_eq!(free_path(folder, "b", |_| false), folder.join("b.mp4"));
    }

    #[test]
    fn clips_capture_mode_menu_707116_strings() {
        let (manual, auto) = clips_mode_subtexts(Some("Alt+C"));
        assert!(manual.contains("Alt+C"));
        assert!(auto.contains("Alt+C"));
        let (manual_none, auto_none) = clips_mode_subtexts(None);
        assert!(!manual_none.is_empty());
        assert!(!auto_none.is_empty());
        for k in [
            "J1hsY2", "eIX9be", "uO/2T5", "bBlUzK", "4gzBi+", "IxZyA+", "t23RLK",
        ] {
            assert!(dc_intl::has_key(k), "missing 707116 key {k}");
        }
    }

    #[test]
    fn share_clips_dispatcher_829903_strings() {
        let count_lbl = share_clips_count_label(3);
        assert!(count_lbl.contains('3'), "got {count_lbl}");
        assert!(!dc_intl::t("iufib1").is_empty());
    }
}
