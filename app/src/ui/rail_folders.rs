//! Server folders in the rail and dragging servers to reorder them (`FolderItem` 283001, the guild drop targets 531053,
//! `SortedGuildStore` 711014, stylesheet classes `folder*_RTMSuZ`, `*_nL08_v`, `dragInner_OkKLIP`). Geometry is read
//! from the captures under `design/golden/windows/rail` and `design/dom/rail`; what no capture shows (the animation
//! timings, unread marks on a closed folder, a drop into an existing folder) comes from the modules and is marked.

use super::motion::{self, Ease, css};
use super::tooltip::{Body, Tip};
use super::{Root, color, icon, unread_badges};
use dc_state::Id;
use dc_state::folders::{Folder, Item};
use gpui::{
    AnyElement, Bounds, Context, DragMoveEvent, FontWeight, Image, IntoElement, Pixels, Render,
    Rgba, SharedString, Window, canvas, div, img, prelude::*, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::sync::Arc;

/// `--guildbar-avatar-size`, `--guildbar-folder-size` and the rail's width (`--custom-guild-list-width`).
const AVATAR: f32 = 40.;
const FOLDER: f32 = 48.;
const WIDTH: f32 = 72.;
/// `FOLDER_ITEM_ANIMATION_DURATION` (module 319060) and `--custom-folder-item-animation-duration` (150ms).
const FOLDER_MS: f64 = 150.;
/// The persisted `ExpandedGuildFolderStore` (`persistKey`, module 383394).
const EXPANDED_KEY: &str = "ExpandedGuildFolderStore";

/// What is being dragged: a server or a folder. The drag image is the item's own icon.
#[derive(Clone)]
pub(super) struct RailDrag {
    pub id: Id,
    pub folder: bool,
    pub image: Option<Arc<Image>>,
    pub label: SharedString,
}

struct Ghost {
    drag: RailDrag,
}

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let size = if self.drag.folder { FOLDER } else { AVATAR };
        let tile = div()
            .size(px(size))
            .rounded(px(12.))
            .overflow_hidden()
            .opacity(0.7)
            .flex()
            .items_center()
            .justify_center()
            .bg(color("--background-mod-subtle"));
        match &self.drag.image {
            Some(image) => tile.child(img(image.clone()).size(px(size)).rounded(px(12.))),
            None => tile
                .text_size(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(self.drag.label.clone()),
        }
    }
}

/// Where a drop would land. Each is one of the transparent targets of the capture (`target_nL08_v`,
/// `centerTarget_nL08_v`, `folderTarget_nL08_v`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Over {
    /// The upper target of a row: dropped there, the source goes above it.
    Above(Id),
    /// The lower target of a root row: combine into it (a folder is made around a lone server).
    Center(Id),
    /// The 24px target at the end of an open folder: append to the folder.
    FolderEnd(Id),
    /// The target over the Add a Server tile: below the last root item.
    BelowLast(Id),
}

#[derive(Default)]
pub(super) struct State {
    /// The item being dragged, once the drag has moved.
    pub drag: Option<(Id, bool)>,
    pub over: Option<Over>,
    loaded: bool,
}

// ---------------------------------------------------------------------------------------------------------------
// Where the rows were painted, for the drop targets
// ---------------------------------------------------------------------------------------------------------------

/// One painted row: an item of the rail (a root server, a folder header, a server inside a folder).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RowRect {
    pub id: Id,
    pub folder: bool,
    pub parent: Option<Id>,
    pub top: f32,
    pub height: f32,
}

#[derive(Default)]
struct Frame {
    rows: Vec<RowRect>,
    /// (folder, bottom of its group) of every open folder, in order.
    ends: Vec<(Id, f32)>,
    /// Top of the Add a Server tile.
    add_top: Option<f32>,
    last_root: Option<Id>,
}

thread_local! {
    static FRAME: RefCell<Frame> = RefCell::default();
}

fn record(row: RowRect) {
    FRAME.with(|frame| frame.borrow_mut().rows.push(row));
}

/// A zone of the transparent drop layer, `y0..y1` from the top of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Zone {
    y0: f32,
    y1: f32,
    x1: f32,
    over: Over,
}

/// The wrappers `wrapper_nL08_v`: `top: -16px; bottom: -4px` around every row, halved into the upper and the lower
/// target for a root row (a row inside a folder has the upper one only), then the open folders' end targets, then
/// the one over Add a Server. Later zones lie over earlier ones, as the later elements of the page do.
fn zones(frame: &Frame) -> Vec<Zone> {
    let mut out = Vec::new();
    let mut ended = 0;
    for (index, row) in frame.rows.iter().enumerate() {
        let (y0, y1) = (row.top - 16., row.top + row.height + 4.);
        if row.parent.is_none() {
            let mid = (y0 + y1) / 2.;
            out.push(Zone {
                y0,
                y1: mid,
                x1: WIDTH,
                over: Over::Above(row.id),
            });
            out.push(Zone {
                y0: mid,
                y1,
                x1: WIDTH,
                over: Over::Center(row.id),
            });
        } else {
            out.push(Zone {
                y0,
                y1,
                x1: WIDTH,
                over: Over::Above(row.id),
            });
        }
        // The end target of a folder comes after its last child.
        let next_parent = frame.rows.get(index + 1).map(|next| next.parent);
        if let Some(parent) = row.parent
            && next_parent != Some(Some(parent))
            && let Some((_, bottom)) = frame.ends.iter().find(|(folder, _)| *folder == parent)
        {
            out.push(Zone {
                y0: bottom - 24.,
                y1: *bottom,
                x1: WIDTH,
                over: Over::FolderEnd(parent),
            });
            ended += 1;
        }
    }
    let _ = ended;
    if let (Some(top), Some(last)) = (frame.add_top, frame.last_root) {
        out.push(Zone {
            y0: top - 16.,
            y1: top + AVATAR + 4.,
            x1: 64.,
            over: Over::BelowLast(last),
        });
    }
    out
}

/// What may be dropped where (`canDrop`, module 531053 `p`): never on itself; a folder is not combined and never
/// goes inside a folder; only a server joins a folder.
fn can_drop(source: (Id, bool), over: Over, rows: &[RowRect]) -> bool {
    let (id, folder) = source;
    let row_of = |target: Id| rows.iter().find(|row| row.id == target);
    match over {
        Over::Above(target) | Over::BelowLast(target) => {
            target != id && (!folder || row_of(target).is_none_or(|row| row.parent.is_none()))
        }
        Over::Center(target) => target != id && !folder,
        Over::FolderEnd(target) => target != id && !folder,
    }
}

/// The target the pointer is over, for a drag of `source`.
fn target_at(frame: &Frame, source: (Id, bool), x: f32, y: f32) -> Option<Over> {
    zones(frame)
        .into_iter()
        .rev()
        .find(|zone| {
            x >= 0.
                && x < zone.x1
                && y >= zone.y0
                && y < zone.y1
                && can_drop(source, zone.over, &frame.rows)
        })
        .map(|zone| zone.over)
}

/// `moveById(source, target, below, combine)` arguments of a drop (module 531053 `p`).
pub(super) fn drop_move(source: Id, over: Over) -> (Id, Id, bool, bool) {
    match over {
        Over::Above(target) => (source, target, false, false),
        Over::BelowLast(target) => (source, target, true, false),
        Over::Center(target) | Over::FolderEnd(target) => (source, target, true, true),
    }
}

/// The request that saves the order and the folders: `PATCH /users/@me/settings-proto/1` with the `guild_folders`
/// message (module 817281 `um` through `updateAsync`, module 594061).
pub(super) fn guild_folders_request(patch: &str) -> Option<crate::net::Request> {
    (!patch.is_empty()).then(|| crate::net::Request::Rest {
        key: None,
        method: reqwest::Method::PATCH,
        route: "/users/@me/settings-proto/1".to_owned(),
        body: Some(json!({"settings": patch})),
    })
}

fn folder_color(value: Option<u64>) -> Option<Rgba> {
    value.map(|value| gpui::rgb(value as u32))
}

fn with_alpha(mut colour: Rgba, alpha: f32) -> Rgba {
    colour.a = alpha;
    colour
}

impl Root {
    /// Once a frame: the open folders come back from the previous run, and a finished drag clears its marks.
    pub(super) fn rail_folders_frame(&mut self, cx: &mut Context<Self>) {
        if !self.rail_folders.loaded && !self.stores.guilds.is_empty() {
            self.rail_folders.loaded = true;
            let ids: Vec<u64> = crate::storage::get(EXPANDED_KEY)
                .and_then(|value| value["expandedFolders"].as_array().cloned())
                .into_iter()
                .flatten()
                .filter_map(|id| id.as_u64())
                .collect();
            self.stores.folders.restore_expanded(ids);
        }
        if self.rail_folders.drag.is_some() && !cx.has_active_drag() {
            self.rail_folders.drag = None;
            self.rail_folders.over = None;
        }
    }

    fn save_expanded(&self) {
        let ids: Vec<u64> = self.stores.folders.expanded().collect();
        crate::storage::set(EXPANDED_KEY, json!({ "expandedFolders": ids }));
    }

    /// `toggleGuildFolderExpand`.
    pub(super) fn toggle_folder(&mut self, folder: Id, cx: &mut Context<Self>) {
        self.stores.folders.toggle_expanded(folder);
        self.save_expanded();
        cx.notify();
    }

    /// Sends the settings write (the harness runs keep it off: `GRAVECORD_NO_SETTINGS_WRITE`). Empty until the
    /// account's settings have arrived.
    fn send_rail_patch(&mut self, patch: String) {
        if let Some(request) = guild_folders_request(&patch) {
            let _ = self.requests.send(request);
        }
    }

    /// Writes the tree as it is now to the account's settings (`updateAsync("guildFolders", ...)`).
    pub(super) fn commit_rail(&mut self, cx: &mut Context<Self>) {
        let patch = self.stores.commit_guild_folders();
        self.send_rail_patch(patch);
        cx.notify();
    }

    /// "Move to... First in List / Last in List" of a server's menu (module 865815 `moveFirst`, `moveLast`).
    pub(super) fn move_guild(
        &mut self,
        id: Id,
        placement: dc_state::folders::Placement,
        cx: &mut Context<Self>,
    ) {
        let patch = self.stores.move_guild(id, placement);
        self.send_rail_patch(patch);
        cx.notify();
    }

    /// The drop of a drag: the move, then the write.
    fn rail_drop(&mut self, source: Id, over: Over, cx: &mut Context<Self>) {
        let (source, target, below, combine) = drop_move(source, over);
        if self
            .stores
            .folders
            .move_by_id(source, target, below, combine)
        {
            // A folder that was just made around a server opens: the official client leaves it closed.
            self.commit_rail(cx);
        }
        self.rail_folders.drag = None;
        self.rail_folders.over = None;
        self.save_expanded();
        cx.notify();
    }

    /// The rail's server entries: root servers and folders, in order.
    pub(super) fn rail_entries(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        FRAME.with(|frame| *frame.borrow_mut() = Frame::default());
        let roots = self.stores.folders.roots();
        FRAME.with(|frame| {
            frame.borrow_mut().last_root = roots.last().map(Item::id);
        });
        let mut out: Vec<AnyElement> = Vec::new();
        for item in roots {
            match item {
                Item::Guild(id) => {
                    if let Some(guild) = self.stores.guild(*id) {
                        out.push(
                            self.rail_draggable(*id, false, None, AVATAR, cx, |this, cx| {
                                this.rail_item(guild, cx).into_any_element()
                            }),
                        );
                    }
                }
                Item::Folder(folder) => out.push(self.rail_folder(folder, cx)),
            }
        }
        // The target over Add a Server: it sits in the rail's flow with no height, so the tile after it keeps its place.
        out.push(self.rail_below_last(cx));
        out
    }

    fn rail_below_last(&self, _cx: &mut Context<Self>) -> AnyElement {
        let last = self.stores.folders.roots().last().map(Item::id);
        let line = (last.is_some() && self.rail_folders.over == last.map(Over::BelowLast))
            .then(|| drop_line(4., WIDTH - 8. - 8.));
        div()
            .relative()
            .flex_none()
            .w(px(WIDTH))
            .h(px(0.))
            // The flex gap that follows is the Add tile's own; this marker takes none.
            .mb(px(-8.))
            .child(
                canvas(
                    |bounds, _, _| {
                        FRAME.with(|frame| {
                            frame.borrow_mut().add_top = Some(f32::from(bounds.origin.y));
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .children(line.map(|line| div().absolute().top(px(-6.)).left(px(4.)).child(line)))
            .into_any_element()
    }

    /// One row of the rail as a drag source and a drop target. While it is the item being dragged its place shows the
    /// dashed placeholder (`dragInner_OkKLIP`).
    fn rail_draggable(
        &self,
        id: Id,
        folder: bool,
        parent: Option<Id>,
        height: f32,
        cx: &mut Context<Self>,
        body: impl FnOnce(&Self, &mut Context<Self>) -> AnyElement,
    ) -> AnyElement {
        let dragging = self.rail_folders.drag == Some((id, folder));
        let guild = (!folder).then(|| self.stores.guild(id)).flatten();
        let drag = RailDrag {
            id,
            folder,
            image: (!folder)
                .then(|| self.images.get(&format!("guild:{id}")).cloned())
                .flatten(),
            label: guild
                .map(|guild| guild.acronym())
                .unwrap_or_default()
                .into(),
        };
        let over = self.rail_folders.over;
        let combine = over == Some(Over::Center(id));
        let mut row = div()
            .id(SharedString::from(format!("rail-row-{id}")))
            .relative()
            .flex_none()
            .w(px(WIDTH))
            .h(px(height))
            .on_drag(drag, |drag, _, _, cx| {
                cx.new(|_| Ghost { drag: drag.clone() })
            })
            .child(
                canvas(
                    move |bounds: Bounds<Pixels>, _, _| {
                        record(RowRect {
                            id,
                            folder,
                            parent,
                            top: f32::from(bounds.origin.y),
                            height: f32::from(bounds.size.height),
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        row = if dragging {
            row.child(placeholder(folder))
        } else if combine && !folder {
            row.child(self.combine_preview(id))
        } else {
            row.child(body(self, cx))
        };
        if over == Some(Over::Above(id)) {
            row = row.child(
                div()
                    .absolute()
                    .top(px(-6.))
                    .left(px(4.))
                    .child(drop_line(4., WIDTH - 8.)),
            );
        }
        if combine {
            row = row.child(
                div()
                    .absolute()
                    .left(px(4.))
                    .top(px(12.))
                    .size(px(16.))
                    .rounded_full()
                    .bg(color("--status-positive"))
                    .shadow(vec![gpui::BoxShadow {
                        color: with_alpha(color("--opacity-black-28"), 0.28).into(),
                        offset: gpui::point(px(0.), px(0.)),
                        blur_radius: px(3.),
                        spread_radius: px(0.),
                        inset: false,
                    }]),
            );
        }
        row.into_any_element()
    }

    /// A server another is held over to make a folder: drawn as the folder it would be, one mini icon in a small
    /// folder preview (`folderHeaderSmall`, `folderDragPreview_RTMSuZ`).
    fn combine_preview(&self, id: Id) -> AnyElement {
        div()
            .w(px(WIDTH))
            .h(px(AVATAR))
            .flex()
            .justify_center()
            .child(
                div()
                    .size(px(AVATAR))
                    .rounded(px(12.))
                    .bg(color("--background-mod-subtle"))
                    .p(px(3.))
                    .child(self.mini(id, 16., 0, 10.)),
            )
            .into_any_element()
    }

    /// A mini server icon of a folder preview: `corner` is its quarter of the 2x2 grid (the rounded one is the
    /// outer corner: 13px at 19px, 10px at 16px; the others 4px).
    fn mini(&self, id: Id, size: f32, corner: usize, outer: f32) -> AnyElement {
        let radii = |tile: gpui::Div| {
            let tile = tile.rounded(px(4.));
            match corner {
                0 => tile.rounded_tl(px(outer)),
                1 => tile.rounded_tr(px(outer)),
                2 => tile.rounded_bl(px(outer)),
                _ => tile.rounded_br(px(outer)),
            }
        };
        let tile = radii(div().size(px(size)).flex_none().overflow_hidden());
        match self.stores.guild(id) {
            Some(guild) => match self.images.get(&format!("guild:{id}")) {
                Some(image) => tile
                    .child(
                        img(image.clone())
                            .size(px(size))
                            .rounded(px(4.))
                            .map_corner(corner, outer),
                    )
                    .into_any_element(),
                None => tile
                    .bg(color("--background-base-lower"))
                    .flex()
                    .items_center()
                    .justify_center()
                    // `.acronym_qqAqr1` at the mini size: 10px over a 20px line.
                    .text_size(px(10.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(guild.acronym())
                    .into_any_element(),
            },
            // An unknown server: the red error tile (`folderPreviewGuildIconError`).
            None => tile
                .bg(color("--background-feedback-mod-critical"))
                .into_any_element(),
        }
    }

    /// The default folder name: its servers' names in order (`defaultFolderName`).
    fn folder_label(&self, folder: &Folder) -> String {
        match folder.name.as_deref().filter(|name| !name.is_empty()) {
            Some(name) => name.to_owned(),
            None => folder
                .guilds
                .iter()
                .filter_map(|id| self.stores.guild(*id))
                .map(|guild| guild.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// An open or closed folder: the header row (the pill, the folder button) and, below, its servers.
    fn rail_folder(&self, folder: &Folder, cx: &mut Context<Self>) -> AnyElement {
        let id = folder.id;
        let expanded = self.stores.folders.is_expanded(id);
        let dragging = self.rail_folders.drag == Some((id, true));
        let selected = self
            .guild
            .is_some_and(|guild| folder.guilds.contains(&guild));
        let tint = folder_color(folder.color);
        let (mut unread, mut mentions, mut high) = (false, 0u64, 0u64);
        for guild in folder.guilds.iter().filter_map(|id| self.stores.guild(*id)) {
            let counts = self.guild_rail_counts(guild);
            unread |= counts.unread;
            mentions += counts.mentions;
            high += counts.high_mentions;
        }
        let row_key = format!("folder-{id}");
        let grow = motion::value(
            motion::key(&("folder-height", id)),
            f32::from(expanded),
            css(FOLDER_MS, Ease::Linear),
        );
        let slide = motion::value(
            motion::key(&("folder-slide", id)),
            f32::from(expanded),
            css(FOLDER_MS, Ease::EaseOut),
        );
        let n = folder.guilds.len() as f32;
        let label = self.folder_label(folder);
        let over = self.rail_folders.over;
        let header = self.rail_draggable(id, true, None, FOLDER, cx, |this, cx| {
            this.folder_header(
                folder,
                expanded,
                slide,
                tint,
                selected,
                (unread, mentions, high),
                &row_key,
                &label,
                cx,
            )
        });
        let hovered_over = over == Some(Over::Center(id)) || over == Some(Over::FolderEnd(id));
        let mut group = div()
            .relative()
            .flex_none()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .child(header);
        // The strip behind an open folder (`folderGroupBackground_RTMSuZ`): not drawn while the folder is dragged.
        if !dragging && (expanded || grow > 0.) {
            let height = FOLDER + n * FOLDER * grow;
            let positive = hovered_over && expanded;
            let strip = div()
                .absolute()
                .left(px(12.))
                .top(px(0.))
                .w(px(FOLDER))
                .h(px(height))
                .rounded(px(16.))
                .border_1()
                .border_color(if positive {
                    color("--status-positive")
                } else {
                    color("--border-muted")
                })
                .bg(if positive {
                    color("--opacity-green-28")
                } else {
                    // `color-mix(in srgb, var(--custom-folder-color), transparent 85%)`.
                    tint.map_or(color("--background-mod-subtle"), |tint| {
                        with_alpha(tint, 0.15)
                    })
                })
                .opacity(motion::value(
                    motion::key(&("folder-strip", id)),
                    f32::from(expanded),
                    css(FOLDER_MS, Ease::EaseOut),
                ));
            group = group.child(strip);
        }
        if !dragging && grow > 0. {
            let mut list = div()
                .flex_none()
                .w(px(WIDTH))
                .h(px(n * FOLDER * grow))
                .overflow_hidden()
                .flex()
                .flex_col()
                .gap(px(8.));
            for (index, guild_id) in folder.guilds.iter().enumerate() {
                let Some(guild) = self.stores.guild(*guild_id) else {
                    continue;
                };
                let child =
                    self.rail_draggable(*guild_id, false, Some(id), AVATAR, cx, |this, cx| {
                        this.rail_item(guild, cx).into_any_element()
                    });
                // `.folderGuildsList>:first-child { margin-top: calc(var(--space-xs) - var(--custom-folder-padding)) }`.
                list = list.child(if index == 0 {
                    div().mt(px(4.)).child(child).into_any_element()
                } else {
                    child
                });
            }
            group = group.child(list);
        }
        if expanded && !dragging {
            let bottom_line = over == Some(Over::FolderEnd(id));
            group = group
                .child(
                    canvas(
                        move |bounds: Bounds<Pixels>, _, _| {
                            FRAME.with(|frame| {
                                frame
                                    .borrow_mut()
                                    .ends
                                    .push((id, f32::from(bounds.origin.y + bounds.size.height)));
                            });
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .children(bottom_line.then(|| {
                    div()
                        .absolute()
                        .bottom(px(0.))
                        .left(px(4.))
                        .child(drop_line(4., WIDTH - 8.))
                }));
        }
        group.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn folder_header(
        &self,
        folder: &Folder,
        expanded: bool,
        slide: f32,
        tint: Option<Rgba>,
        selected: bool,
        marks: (bool, u64, u64),
        row_key: &str,
        label: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = folder.id;
        let (unread, mentions, high) = marks;
        let hovered_over = self.rail_folders.over == Some(Over::Center(id));
        // The icon half and the preview half of `folderButtonContent_RTMSuZ` slide over the 48px window, 150ms
        // ease-out: the preview shows while closed, the icon while open.
        let icon_half = div().flex_none().size(px(FOLDER)).p(px(4.)).child(
            div()
                .id(SharedString::from(format!("folder-icon-{id}")))
                .size(px(AVATAR))
                .rounded(px(12.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(tint.unwrap_or_else(|| color("--background-brand")))
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .child(
                    icon("clips_folder", 18., "--background-brand")
                        .text_color(tint.unwrap_or_else(|| color("--background-brand"))),
                ),
        );
        let mut preview = div().size(px(AVATAR)).flex().flex_wrap().gap(px(2.));
        for (index, guild_id) in folder.guilds.iter().take(4).enumerate() {
            preview = preview.child(self.mini(*guild_id, 19., index, 13.));
        }
        let preview_half = div()
            .flex_none()
            .size(px(FOLDER))
            .p(px(4.))
            .rounded(px(16.))
            // `color-mix(in srgb, var(--custom-folder-color), transparent 60%)`.
            .bg(tint.map_or(color("--background-mod-subtle"), |tint| {
                with_alpha(tint, 0.4)
            }))
            .child(preview);
        let content = div()
            .absolute()
            .left(px(0.))
            .top(px(-FOLDER * (1. - slide)))
            .w(px(FOLDER))
            .h(px(FOLDER * 2.))
            .flex()
            .flex_col()
            .child(icon_half)
            .child(preview_half);
        let mut button = div()
            .id(SharedString::from(format!("folder-{id}")))
            .relative()
            .size(px(FOLDER))
            .rounded(px(12.))
            .overflow_hidden()
            .cursor_pointer()
            .child(content)
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_folder(id, cx)));
        if hovered_over && !expanded {
            // `[data-drop-hovering=true]:not(.isExpanded) .folderHeader`: green wash and outline.
            button = button
                .bg(color("--opacity-green-28"))
                .border_1()
                .border_color(color("--status-positive"));
        }
        let button = self.group_stop(
            button,
            "rail",
            super::keyboard::Nav::List,
            row_key,
            selected,
            cx,
        );
        // Closed, the pill shows for the folder (an open folder's pill is `disabled`); selected when a server in it is.
        let pill = (!expanded).then(|| {
            div()
                .absolute()
                .left(px(0.))
                .top(px(4.))
                .w(px(4.))
                .h(px(AVATAR))
                .child(motion::rail::pill(row_key, selected, unread))
        });
        let badge = (!expanded && mentions > 0)
            .then(|| unread_badges::rail_number(mentions, high == 0, None).right(px(10.)));
        div()
            .relative()
            .w(px(WIDTH))
            .h(px(FOLDER))
            .flex_none()
            .flex()
            .justify_center()
            .children(pill)
            .child(motion::hover_probe(motion::rail::hover_key(row_key)))
            .child(self.with_tip(
                format!("folder-{id}"),
                Tip::rail(Body::Heading(label.to_owned().into())),
                button,
                cx,
            ))
            .children(badge)
            .into_any_element()
    }

    /// Drag moves over the rail: which target the pointer is over.
    pub(super) fn rail_drag_moved(
        &mut self,
        event: &DragMoveEvent<RailDrag>,
        cx: &mut Context<Self>,
    ) {
        let drag = event.drag(cx).clone();
        let source = (drag.id, drag.folder);
        let at = event.event.position;
        let over = FRAME
            .with(|frame| target_at(&frame.borrow(), source, f32::from(at.x), f32::from(at.y)));
        if self.rail_folders.drag != Some(source) || self.rail_folders.over != over {
            self.rail_folders.drag = Some(source);
            self.rail_folders.over = over;
            cx.notify();
        }
    }

    /// The drop on the rail.
    pub(super) fn rail_dropped(&mut self, drag: &RailDrag, cx: &mut Context<Self>) {
        match self.rail_folders.over {
            Some(over) => self.rail_drop(drag.id, over, cx),
            None => {
                self.rail_folders.drag = None;
                cx.notify();
            }
        }
    }
}

fn placeholder(folder: bool) -> AnyElement {
    let size = if folder { FOLDER } else { AVATAR };
    div()
        .w(px(WIDTH))
        .h(px(size))
        .flex()
        .justify_center()
        .child(
            div()
                .size(px(size))
                .rounded(px(12.))
                .border_1()
                .border_dashed()
                .border_color(color("--border-subtle"))
                .bg(color("--background-mod-normal")),
        )
        .into_any_element()
}

/// The green line of an upper target (`.target.dragOver:before`): 4px, 2px corners, a soft shadow.
fn drop_line(height: f32, width: f32) -> gpui::Div {
    div()
        .w(px(width))
        .h(px(height))
        .rounded(px(2.))
        .bg(color("--status-positive"))
        .shadow(vec![gpui::BoxShadow {
            color: with_alpha(color("--opacity-black-28"), 0.28).into(),
            offset: gpui::point(px(0.), px(0.)),
            blur_radius: px(3.),
            spread_radius: px(0.),
            inset: false,
        }])
}

/// A server image with the one rounded corner of its quarter of a folder preview.
trait MapCorner {
    fn map_corner(self, corner: usize, outer: f32) -> Self;
}

impl MapCorner for gpui::Img {
    fn map_corner(self, corner: usize, outer: f32) -> Self {
        let this = self.rounded(px(4.));
        match corner {
            0 => this.rounded_tl(px(outer)),
            1 => this.rounded_tr(px(outer)),
            2 => this.rounded_bl(px(outer)),
            _ => this.rounded_br(px(outer)),
        }
    }
}

/// Capture-only script steps: more servers and a folder than the test account has, injected into the local stores
/// (official-shaped data, nothing is sent: the settings write stays off in harness runs).
#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_rail(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let parse = |text: &&str| text.parse::<u64>().ok();
        let note = match words {
            ["fake", count] => {
                let Ok(count) = count.parse::<u64>() else {
                    return "usage: rail fake <count>".into();
                };
                for index in 0..count {
                    let id = 9_100_000_000_000_000_000u64 + index;
                    self.stores.dispatch(
                        "GUILD_CREATE",
                        &json!({
                            "id": id.to_string(),
                            "joined_at": format!("2026-11-{:02}T00:00:00+00:00", 1 + index % 9),
                            "properties": {"name": format!("Fake Server {}", index + 1), "icon": null},
                            "channels": [],
                        }),
                    );
                }
                format!("rail: {count} fake servers (ids from 9100000000000000000)")
            }
            ["order", ids @ ..] => {
                let ids: Vec<u64> = ids.iter().filter_map(parse).collect();
                self.stores.set_rail_order(&ids);
                self.stores.commit_guild_folders();
                format!("rail: order {ids:?}")
            }
            ["folder", id, name, color, ids @ ..] => {
                let (Some(id), ids) = (
                    id.parse::<u64>().ok(),
                    ids.iter().filter_map(parse).collect::<Vec<u64>>(),
                ) else {
                    return "usage: rail folder <id> <name|-> <hex|-> <guild ids...>".into();
                };
                let name = (*name != "-").then(|| name.replace('_', " "));
                let color = u64::from_str_radix(color.trim_start_matches('#'), 16).ok();
                let made = self
                    .stores
                    .folders
                    .create_folder_as(Some(id), &ids, name.as_deref());
                if made.is_some() {
                    self.stores
                        .folders
                        .set_folder_look(id, name.as_deref().unwrap_or(""), color);
                }
                self.stores.commit_guild_folders();
                format!("rail: folder {id} made={}", made.is_some())
            }
            ["open", id] | ["close", id] => {
                let Some(id) = id.parse::<u64>().ok() else {
                    return "usage: rail open|close <folder id>".into();
                };
                self.stores.folders.set_expanded(id, words[0] == "open");
                format!("rail: folder {id} {}", words[0])
            }
            ["state"] => format!(
                "rail: {:?} expanded {:?}",
                self.stores
                    .folders
                    .roots()
                    .iter()
                    .map(|item| match item {
                        Item::Guild(id) => id.to_string(),
                        Item::Folder(folder) => format!("[{}:{:?}]", folder.id, folder.guilds),
                    })
                    .collect::<Vec<_>>(),
                self.stores.folders.expanded().collect::<Vec<_>>()
            ),
            _ => "usage: rail fake|order|folder|open|close|state".into(),
        };
        cx.notify();
        note
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: Id, folder: bool, parent: Option<Id>, top: f32, height: f32) -> RowRect {
        RowRect {
            id,
            folder,
            parent,
            top,
            height,
        }
    }

    /// The layout of `drag-line-top`: two servers at 89 and 137, Add a Server at 185.
    fn two_servers() -> Frame {
        Frame {
            rows: vec![
                row(1, false, None, 89., 40.),
                row(2, false, None, 137., 40.),
            ],
            ends: Vec::new(),
            add_top: Some(185.),
            last_root: Some(2),
        }
    }

    #[test]
    fn zones_are_the_captured_wrappers() {
        // `wrapper_nL08_v` of the first row is 73..133 (`top: -16px`, 60px), halved at 103.
        let frame = two_servers();
        let all = zones(&frame);
        assert_eq!((all[0].y0, all[0].y1), (73., 103.));
        assert_eq!((all[1].y0, all[1].y1), (103., 133.));
        // The Add a Server wrapper is 169..229 and 64 wide.
        let last = all.last().unwrap();
        assert_eq!((last.y0, last.y1, last.x1), (169., 229., 64.));
    }

    #[test]
    fn the_later_row_covers_the_end_of_the_earlier_ones_lower_target() {
        let frame = two_servers();
        // 125 is inside the first row's lower target (103..133) and the second row's upper one (121..151).
        assert_eq!(
            target_at(&frame, (9, false), 36., 125.),
            Some(Over::Above(2))
        );
        assert_eq!(
            target_at(&frame, (9, false), 36., 110.),
            Some(Over::Center(1))
        );
        assert_eq!(
            target_at(&frame, (9, false), 36., 80.),
            Some(Over::Above(1))
        );
        assert_eq!(target_at(&frame, (9, false), 80., 80.), None);
    }

    #[test]
    fn below_the_last_row_is_the_add_tile_target() {
        let frame = two_servers();
        assert_eq!(
            target_at(&frame, (1, false), 36., 200.),
            Some(Over::BelowLast(2))
        );
    }

    #[test]
    fn a_folder_cannot_be_combined_nor_put_inside_a_folder() {
        let frame = Frame {
            rows: vec![
                row(100, true, None, 89., 48.),
                row(1, false, Some(100), 141., 40.),
                row(2, false, Some(100), 189., 40.),
                row(3, false, None, 245., 40.),
            ],
            ends: vec![(100, 233.)],
            add_top: Some(293.),
            last_root: Some(3),
        };
        // Over the lower target of a root server, a folder source finds nothing to combine with.
        assert!(!can_drop((100, true), Over::Center(3), &frame.rows));
        // Nor does it go beside a server inside a folder.
        assert!(!can_drop((100, true), Over::Above(1), &frame.rows));
        assert!(can_drop((100, true), Over::Above(3), &frame.rows));
        // A server may join a folder at its end, and not itself.
        assert!(can_drop((3, false), Over::FolderEnd(100), &frame.rows));
        assert!(!can_drop((1, false), Over::Above(1), &frame.rows));
        // The end target lies after the last child: 209..233.
        assert_eq!(
            target_at(&frame, (3, false), 36., 231.),
            Some(Over::FolderEnd(100))
        );
    }

    #[test]
    fn the_write_is_the_settings_proto_patch_of_the_folders() {
        use dc_proto::settings::{GuildFolderEntry, decode, guild_folders, set_guild_folders};
        let entries = vec![
            GuildFolderEntry {
                guild_ids: vec![7],
                ..GuildFolderEntry::default()
            },
            GuildFolderEntry {
                id: Some(99),
                name: Some("night folder".into()),
                color: Some(0x2ecc71),
                guild_ids: vec![8, 9],
            },
        ];
        let (patch, _) = set_guild_folders(&[], &entries);
        let Some(crate::net::Request::Rest {
            method,
            route,
            body: Some(body),
            ..
        }) = guild_folders_request(&patch)
        else {
            panic!("a patch makes a request");
        };
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/users/@me/settings-proto/1");
        assert_eq!(body.as_object().unwrap().len(), 1);
        let sent = decode(body["settings"].as_str().unwrap());
        assert_eq!(guild_folders(&sent).0, entries);
        assert!(guild_folders_request("").is_none());
    }

    #[test]
    fn drops_become_the_official_move_arguments() {
        assert_eq!(drop_move(1, Over::Above(2)), (1, 2, false, false));
        assert_eq!(drop_move(1, Over::BelowLast(2)), (1, 2, true, false));
        assert_eq!(drop_move(1, Over::Center(2)), (1, 2, true, true));
        assert_eq!(drop_move(1, Over::FolderEnd(2)), (1, 2, true, true));
    }
}
