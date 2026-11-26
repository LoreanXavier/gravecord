//! BetterFolders (Vencord `src/plugins/betterFolders`, GPL-3.0-or-later): an open server folder's servers in a
//! second 72px column beside the server rail (`FolderSideBar`, the rail's `GuildsBar` filtered to the open folders),
//! with the folder rules of its Flux handlers. Not part of the pinned client.
//!
//! - Sidebar: while a folder is open its servers leave the rail (the rail draws the open folder's header alone) and
//!   are drawn in the column, under the folder's icon as Show Folder Icon says. Keep Icons draws the folder in the
//!   rail as if it were closed (its four small icons). Sidebar Animation slides the column open and shut over 200ms
//!   (`Animations.Transition` from width 0). The column is not drawn while no folder is open.
//! - `CHANNEL_SELECT`: going to another server opens its folder (Force Open) or closes it (Close Server Folder), and
//!   a server outside folders, or Home, closes every folder (Close All Folders).
//! - `TOGGLE_GUILD_FOLDER_EXPAND`: Close Others leaves only the folder just toggled open.
//! - Home closes every folder first (Close All Home Button); `LOGOUT` closes every folder.

use super::super::motion::{self, Ease, css};
use super::super::tooltip::{Body, Tip};
use super::super::{Root, color, icon};
use dc_state::Id;
use dc_state::folders::{Folder, Item};
use gpui::{AnyElement, Context, IntoElement, MouseButton, SharedString, div, prelude::*, px};
use serde_json::Value;
use std::cell::{Cell, RefCell};

const ID: &str = "BetterFolders";
/// The column is the rail's width (`guilds.getBoundingClientRect().width`).
const WIDTH: f32 = 72.;
const FOLDER: f32 = 48.;
const AVATAR: f32 = 40.;
/// `config={{ duration: 200 }}`.
const SLIDE_MS: f64 = 200.;
/// `FolderIconDisplay`.
const ICON_NEVER: i64 = 0;
const ICON_MORE_THAN_ONE: i64 = 2;

thread_local! {
    /// `lastGuildId`: the server open at the last frame, `None` before the first frame the plugin saw.
    static LAST_GUILD: Cell<Option<Option<Id>>> = const { Cell::new(None) };
    /// The folders the column showed last, drawn while it slides shut.
    static SHOWN: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// The column's width as drawn this frame, for what sits right of it.
    static DRAWN: Cell<f32> = const { Cell::new(0.) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

fn flag(key: &str) -> bool {
    crate::plugins::with(|state| state.value(ID, key) == Value::Bool(true))
}

/// Whether the column is used at all (the Sidebar switch).
fn sidebar() -> bool {
    on() && flag("sidebar")
}

/// `shouldShowFolderIconAndBackground` in the column.
pub(super) fn shows_folder_icon(mode: i64, open: usize) -> bool {
    match mode {
        ICON_NEVER => false,
        ICON_MORE_THAN_ONE => open > 1,
        _ => true,
    }
}

/// What `CHANNEL_SELECT` does when the open server changed: the folder to toggle, or every folder to close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OnSelect {
    Nothing,
    Toggle(u64),
    CloseAll,
}

/// The rules of the `CHANNEL_SELECT` handler for a server inside `folder` (open or not), or outside folders.
pub(super) fn on_select(
    folder: Option<(u64, bool)>,
    force_open: bool,
    close_server_folder: bool,
    close_all: bool,
) -> OnSelect {
    match folder {
        Some((id, false)) if force_open => OnSelect::Toggle(id),
        Some((id, true)) if close_server_folder => OnSelect::Toggle(id),
        Some(_) => OnSelect::Nothing,
        None if close_all => OnSelect::CloseAll,
        None => OnSelect::Nothing,
    }
}

impl Root {
    /// How the rail draws a folder that is `expanded`: (its header open, its servers listed under it). The pinned
    /// client draws both as `expanded`; with the column the servers move there, and Keep Icons draws the folder closed.
    pub(in crate::ui) fn better_folders_rail(expanded: bool) -> (bool, bool) {
        if !expanded || !sidebar() {
            return (expanded, expanded);
        }
        crate::plugins::note(ID);
        if flag("keepIcons") {
            (false, false)
        } else {
            (true, false)
        }
    }

    /// The open folders the column lists, in the rail's order (`getExpandedFolderIds`: open and not empty).
    fn better_folders_open(&self) -> Vec<&Folder> {
        self.stores
            .folders
            .roots()
            .iter()
            .filter_map(|item| match item {
                Item::Folder(folder)
                    if !folder.guilds.is_empty() && self.stores.folders.is_expanded(folder.id) =>
                {
                    Some(folder)
                }
                _ => None,
            })
            .collect()
    }

    /// Once a frame: the `CHANNEL_SELECT` rules when the open server changed.
    pub(in crate::ui) fn better_folders_frame(&mut self, cx: &mut Context<Self>) {
        if !on() {
            LAST_GUILD.with(|last| last.set(None));
            return;
        }
        let guild = self.guild;
        let Some(last) = LAST_GUILD.with(|last| last.replace(Some(guild))) else {
            return;
        };
        if last == guild {
            return;
        }
        let (force_open, close_server_folder, close_all) = (
            flag("forceOpen"),
            flag("closeServerFolder"),
            flag("closeAllFolders"),
        );
        if !force_open && !close_server_folder && !close_all {
            return;
        }
        let folder = guild
            .and_then(|guild| self.stores.folders.folder_of(guild))
            .map(|folder| (folder.id, self.stores.folders.is_expanded(folder.id)));
        crate::plugins::note(ID);
        match on_select(folder, force_open, close_server_folder, close_all) {
            OnSelect::Nothing => {}
            OnSelect::Toggle(id) => self.toggle_folder(id, cx),
            OnSelect::CloseAll => self.collapse_all_folders(cx),
        }
    }

    /// After a folder was toggled: Close Others closes every other open folder.
    pub(in crate::ui) fn better_folders_toggled(&mut self, folder: u64) {
        if !on() || !flag("closeOthers") {
            return;
        }
        let open: Vec<u64> = self.stores.folders.expanded().collect();
        if open.len() > 1 {
            crate::plugins::note(ID);
            for id in open.into_iter().filter(|id| *id != folder) {
                self.stores.folders.set_expanded(id, false);
            }
        }
    }

    /// The Home tile was pressed: Close All Home Button closes every folder first.
    pub(in crate::ui) fn better_folders_home(&mut self, cx: &mut Context<Self>) {
        if on() && flag("closeAllHomeButton") {
            crate::plugins::note(ID);
            self.collapse_all_folders(cx);
        }
    }

    /// `LOGOUT`: every folder closes.
    pub(in crate::ui) fn better_folders_logout(&mut self, cx: &mut Context<Self>) {
        if on() {
            crate::plugins::note(ID);
            self.collapse_all_folders(cx);
        }
    }

    /// The column's width as drawn this frame (0 while it is not drawn), for the parts placed right of it.
    pub(in crate::ui) fn better_folders_width() -> f32 {
        DRAWN.with(Cell::get)
    }

    /// The rail, with the column beside it while a folder is open.
    pub(in crate::ui) fn better_folders_beside(
        &self,
        rail: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !sidebar() {
            DRAWN.with(|drawn| drawn.set(0.));
            return rail;
        }
        let open = self.better_folders_open();
        let visible = !open.is_empty();
        if visible {
            SHOWN.with(|shown| {
                let mut shown = shown.borrow_mut();
                shown.clear();
                shown.extend(open.iter().map(|folder| folder.id));
            });
        }
        let width = if flag("sidebarAnim") {
            motion::value(
                motion::key(&"better-folders-column"),
                if visible { WIDTH } else { 0. },
                css(SLIDE_MS, Ease::EaseInOut),
            )
        } else if visible {
            WIDTH
        } else {
            0.
        };
        DRAWN.with(|drawn| drawn.set(width));
        if width <= 0. {
            SHOWN.with(|shown| shown.borrow_mut().clear());
            return rail;
        }
        crate::plugins::note(ID);
        // While it slides shut the column keeps the folders it showed.
        let folders: Vec<&Folder> = if visible {
            open
        } else {
            SHOWN.with(|shown| {
                shown
                    .borrow()
                    .iter()
                    .filter_map(|id| self.stores.folders.folder(*id))
                    .collect()
            })
        };
        let mode =
            crate::plugins::with(|state| state.value(ID, "showFolderIcon").as_i64().unwrap_or(1));
        let with_icon = shows_folder_icon(mode, folders.len());
        let mut list = div()
            .id("better-folders-column")
            .overflow_y_scroll()
            .flex_1()
            .min_h(px(0.))
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.));
        for folder in folders {
            list = list.child(self.better_folders_group(folder, with_icon, cx));
        }
        // The user panel floats over the bottom of the column as it does over the rail's.
        let reserved = (self.user_panel_height.unwrap_or(px(58.)) - px(8.)).max(px(0.));
        let column = div()
            .flex_none()
            .w(px(width))
            .h_full()
            .overflow_hidden()
            .child(
                div()
                    .w(px(WIDTH))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(list.child(div().flex_none().h(px(16.))))
                    .child(div().flex_none().h(reserved)),
            );
        div()
            .flex_none()
            .h_full()
            .flex()
            .child(rail)
            .child(column)
            .into_any_element()
    }

    /// One open folder in the column: its icon and the strip behind the group when they show, then its servers.
    fn better_folders_group(
        &self,
        folder: &Folder,
        with_icon: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = folder.id;
        let tint = super::super::rail_folders::folder_color(folder.color);
        let guilds: Vec<_> = folder
            .guilds
            .iter()
            .filter_map(|guild| self.stores.guild(*guild))
            .collect();
        let mut group = div()
            .relative()
            .flex_none()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .gap(px(8.));
        if with_icon {
            // `folderGroupBackground`: behind the open folder's icon and its servers.
            group = group.child(
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(0.))
                    .w(px(FOLDER))
                    .h(px(FOLDER + guilds.len() as f32 * FOLDER))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(tint.map_or(color("--background-mod-subtle"), |tint| {
                        super::super::rail_folders::with_alpha(tint, 0.15)
                    })),
            );
            let label = self.folder_label(folder);
            let button = div()
                .id(SharedString::from(format!("better-folders-icon-{id}")))
                .size(px(FOLDER))
                .rounded(px(12.))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(AVATAR))
                        .rounded(px(12.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .child(
                            icon("clips_folder", 18., "--background-brand")
                                .text_color(tint.unwrap_or_else(|| color("--background-brand"))),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_folder(id, cx)))
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                        this.open_folder_menu(id, event.position, cx);
                        cx.stop_propagation();
                    }),
                );
            group = group.child(
                div()
                    .w(px(WIDTH))
                    .h(px(FOLDER))
                    .flex()
                    .justify_center()
                    .child(self.with_tip(
                        format!("better-folders-icon-{id}"),
                        Tip::rail(Body::Heading(label.into())),
                        button,
                        cx,
                    )),
            );
        }
        for (index, guild) in guilds.into_iter().enumerate() {
            let row = self.rail_item(guild, cx).into_any_element();
            // Under the folder's icon the first server sits 4px from it, as in the rail's open folder
            // (`.folderGuildsList>:first-child`).
            group = group.child(if with_icon && index == 0 {
                div().mt(px(-4.)).child(row).into_any_element()
            } else {
                row
            });
        }
        group.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_icon_follows_its_setting() {
        assert!(!shows_folder_icon(0, 3));
        assert!(shows_folder_icon(1, 1));
        assert!(!shows_folder_icon(2, 1));
        assert!(shows_folder_icon(2, 2));
    }

    #[test]
    fn selecting_a_server_follows_the_flux_rules() {
        // Force Open opens a closed folder; Close Server Folder closes an open one.
        assert_eq!(
            on_select(Some((5, false)), true, false, false),
            OnSelect::Toggle(5)
        );
        assert_eq!(
            on_select(Some((5, true)), true, false, false),
            OnSelect::Nothing
        );
        assert_eq!(
            on_select(Some((5, true)), false, true, false),
            OnSelect::Toggle(5)
        );
        // A server outside folders (or Home) closes all only with Close All Folders.
        assert_eq!(on_select(None, true, true, false), OnSelect::Nothing);
        assert_eq!(on_select(None, false, false, true), OnSelect::CloseAll);
        // A server in a folder never closes the others.
        assert_eq!(
            on_select(Some((5, true)), false, false, true),
            OnSelect::Nothing
        );
    }
}
