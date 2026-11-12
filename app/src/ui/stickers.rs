//! The expression picker's Stickers tab, measured in `design/dom/pickers/stickers-*-darker-cozy.json`: a 48px category
//! rail beside a list of 32px section headings and rows of four 98px cells, with the inspected sticker named in a 48px
//! bar. Categories are built by `dc_state::stickers` (module 256449 `pD`), the grid is module 256449 `_c`, the click
//! follows module 989161 (`nm`): a sticker goes out at once, or is staged in the composer when it has text, files or
//! another sticker with it. Art comes from module 378058: Lottie files are drawn by `crate::lottie`, the other formats
//! are images from the media proxy.

use super::tooltip::{Body, Tip};
use super::{Root, color, editor_style};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Transitions as _;
use dc_state::Id;
use dc_state::stickers::{
    Category, CategoryInputs, CategoryKind, Format, SendContext, Sendability, Sticker,
};
use gpui::{
    AnyElement, Bounds, Context, Entity, Focusable, FontWeight, IntoElement, ListAlignment,
    ListOffset, ListState, SharedString, Subscription, Window, canvas, div, fill, img, list, point,
    prelude::*, px, size, svg,
};
use std::collections::HashSet;

/// `GET /sticker-packs?locale=`, module 514983.
pub(super) const PACKS_KEY: &str = "sticker-packs";
pub(super) const PACKS_ROUTE: &str = "/sticker-packs?locale=en-US";
const COLUMNS: usize = 4;
/// Rows of the grid in view when the list has not reported yet (the grid is 4 rows tall), and the rows beyond the
/// visible ones that are given a picture so a scroll does not show empty cells.
const VISIBLE_ROWS: usize = 5;
const ROWS_BEYOND: usize = 1;
const CELL: f32 = 98.;
const ROW_GAP: f32 = 12.;
const HEADING: f32 = 32.;
const UPSELL: f32 = 54.;
/// `kr(48 * 2)` of module 776231 is 96 and `kr(160 * 2)` is 320; one image serves the picker and the message list.
const ART_SIZE: u32 = 320;
/// `STICKER_DIMENSION` in module 148355: a sticker in a message.
pub(super) const MESSAGE_STICKER: f32 = 160.;
/// The ten suggestions under "no results" (module 357354 `tB`).
const SUGGESTIONS: [&str; 10] = [
    "laugh", "wave", "yes", "dance", "sad", "no", "hi", "bye", "cry", "ok",
];

/// One cell of the grid: a sticker, or the tile that opens the server's Stickers page.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Sticker(Sticker),
    Create,
}

#[derive(Clone, Debug)]
struct Section {
    key: String,
    category: Option<Category>,
    cells: Vec<Cell>,
}

#[derive(Clone, Copy, Debug)]
enum StickerRow {
    Heading(usize),
    Upsell(usize),
    Cells { section: usize, start: usize },
}

pub(crate) struct StickerPicker {
    search: Entity<Editor>,
    _events: Subscription,
    sections: Vec<Section>,
    rows: Vec<StickerRow>,
    list: ListState,
    /// The rows the list last showed. Every row is built to be measured, but only these (and a row beyond) ask for a
    /// sticker's picture and start its animation.
    visible: std::ops::Range<usize>,
    collapsed: HashSet<String>,
    /// `handleStickerInspect`: the sticker last hovered; the bar names the first sticker until one is.
    hovered: Option<Cell>,
    query: String,
    /// The list has been scrolled: only then does the rail mark a category (module 357354 sets it from `onScroll`).
    scrolled: bool,
    /// Sections for the rail, which lists the unfiltered categories.
    categories: Vec<Category>,
}

/// What the Stickers tab keeps for the session.
#[derive(Default)]
pub(crate) struct Stickers {
    picker: Option<StickerPicker>,
    /// `favoriteStickers`, once this session changed it (else the account's settings are read).
    favorites: Option<Vec<Id>>,
    /// The sticker waiting in the composer (`ADD_STICKER_PREVIEW`).
    staged: Option<Id>,
    /// Frames of the stickers that move (`sticker_player`).
    pub(super) player: super::sticker_player::Shared,
}

impl Stickers {
    #[cfg(feature = "capture")]
    pub fn search(&self) -> Option<Entity<Editor>> {
        self.picker.as_ref().map(|picker| picker.search.clone())
    }

    pub(crate) fn take_staged(&mut self) -> Option<Id> {
        self.staged.take()
    }
}

/// Module 378058 `zg`: the picture's address for a sticker.
pub(super) fn art_url(sticker: &Sticker) -> String {
    let id = sticker.id;
    match sticker.format() {
        Some(Format::Lottie) => format!("https://discord.com/stickers/{id}.json"),
        Some(Format::Apng) => format!(
            "https://media.discordapp.net/stickers/{id}.png?size={ART_SIZE}&passthrough=false"
        ),
        Some(Format::Gif) => {
            format!("https://media.discordapp.net/stickers/{id}.png?size={ART_SIZE}")
        }
        _ => format!(
            "https://media.discordapp.net/stickers/{id}.webp?size={ART_SIZE}&quality=lossless"
        ),
    }
}

/// Module 378058 `zg` for a sticker that animates: the Lottie file, or the media proxy's animated PNG or GIF (no
/// `passthrough=false`, and a GIF as itself rather than as the still).
pub(super) fn animated_art_url(id: Id, format: Format) -> String {
    match format {
        Format::Lottie => format!("https://discord.com/stickers/{id}.json"),
        Format::Gif => format!("https://media.discordapp.net/stickers/{id}.gif?size={ART_SIZE}"),
        _ => format!("https://media.discordapp.net/stickers/{id}.png?size={ART_SIZE}"),
    }
}

pub(super) fn art_key(id: Id) -> String {
    format!("sticker:{id}")
}

fn sections_of(
    categories: &[Category],
    current_guild: Option<Id>,
    can_create: impl Fn(Id) -> bool,
    slots: impl Fn(Id) -> usize,
) -> Vec<Section> {
    categories
        .iter()
        .map(|category| {
            let mut cells: Vec<Cell> = category
                .stickers
                .iter()
                .cloned()
                .map(Cell::Sticker)
                .collect();
            // `k++` in `_c`: the guild's own list ends with the upload tile while it has free slots.
            if category.kind == CategoryKind::Guild
                && let Some(guild) = category
                    .id
                    .parse::<Id>()
                    .ok()
                    .filter(|id| Some(*id) == current_guild && can_create(*id))
                && category.stickers.len() < slots(guild)
            {
                cells.push(Cell::Create);
            }
            Section {
                key: category.id.clone(),
                category: Some(category.clone()),
                cells,
            }
        })
        .collect()
}

fn rows_of(sections: &[Section], collapsed: &HashSet<String>, headings: bool) -> Vec<StickerRow> {
    let mut rows = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        if headings {
            rows.push(StickerRow::Heading(index));
        }
        if collapsed.contains(&section.key) {
            continue;
        }
        if section
            .category
            .as_ref()
            .is_some_and(|category| category.kind == CategoryKind::EmptyGuildUpsell)
        {
            rows.push(StickerRow::Upsell(index));
            continue;
        }
        for start in (0..section.cells.len()).step_by(COLUMNS) {
            rows.push(StickerRow::Cells {
                section: index,
                start,
            });
        }
    }
    rows
}

/// Whether row `row` is among the rows the list shows or the one beyond them on either side.
fn row_in_view(row: usize, visible: &std::ops::Range<usize>) -> bool {
    row + ROWS_BEYOND >= visible.start && row < visible.end + ROWS_BEYOND
}

fn row_height(row: &StickerRow) -> f32 {
    match row {
        StickerRow::Heading(_) => HEADING,
        StickerRow::Upsell(_) => UPSELL,
        StickerRow::Cells { .. } => CELL + ROW_GAP,
    }
}

impl Root {
    /// `favoriteStickers.stickerIds`: this session's list once it changed, else the account's settings as fetched.
    pub(super) fn favorite_sticker_ids(&self) -> Vec<Id> {
        if let Some(list) = self.sticker_ui.favorites.as_ref() {
            return list.clone();
        }
        self.fetched
            .get(super::actions::FRECENCY_KEY)
            .and_then(|body| body["settings"].as_str())
            .map(dc_proto::frecency::favorite_stickers)
            .unwrap_or_default()
    }

    /// Module 361670 `W$`'s inputs for the channel the composer sends to.
    fn sticker_context(&self) -> SendContext {
        let channel = self.current_channel();
        let external = channel.is_some_and(|channel| {
            self.guild.is_none()
                || self.can_in_channel(channel, dc_state::stickers::USE_EXTERNAL_STICKERS)
        });
        SendContext {
            signed_in: self.stores.me().is_some(),
            everywhere: self
                .premium()
                .has(dc_state::premium::Feature::StickersEverywhere),
            channel_guild: self.guild,
            external,
        }
    }

    /// `SENDABLE` here: what the autocomplete's sticker results are filtered by.
    pub(super) fn sticker_is_sendable(&self, sticker: &Sticker) -> bool {
        self.sticker_sendability(sticker) == Sendability::Sendable
    }

    fn sticker_sendability(&self, sticker: &Sticker) -> Sendability {
        self.sticker_context().sendability(
            sticker,
            |guild| {
                self.stores
                    .guild(guild)
                    .is_some_and(|known| known.id == guild)
            },
            |pack| self.stores.stickers.pack(pack).is_some(),
        )
    }

    fn sticker_categories(&self) -> Vec<Category> {
        let me = self.stores.me();
        let guilds: Vec<(Id, String)> = self
            .stores
            .guilds
            .iter()
            .map(|guild| (guild.id, guild.name.clone()))
            .collect();
        let current = self.guild.and_then(|id| self.stores.guild(id));
        let can_manage = current.zip(me).is_some_and(|(guild, me)| {
            dc_state::permissions::can_in_guild(
                guild,
                me,
                dc_state::permissions::MANAGE_GUILD_EXPRESSIONS,
            ) || dc_state::permissions::can_in_guild(
                guild,
                me,
                dc_state::permissions::CREATE_GUILD_EXPRESSIONS,
            )
        });
        let can_use_external = self.current_channel().is_none_or(|channel| {
            self.guild.is_none()
                || self.can_in_channel(channel, dc_state::stickers::USE_EXTERNAL_EMOJIS)
        });
        let favorites = self.favorite_sticker_ids();
        let now = super::picker::now_ms();
        let frequent = self.stores.stickers.frequently(now);
        self.stores.stickers.categories(
            &CategoryInputs {
                favorites: &favorites,
                guilds: &guilds,
                current_guild: self.guild,
                can_manage_current: can_manage,
                can_use_external,
                frequent: &frequent,
                favorites_name: "Favorites",
                recent_name: "Frequently Used",
            },
            now,
        )
    }

    fn can_create_stickers(&self, guild: Id) -> bool {
        let Some(me) = self.stores.me() else {
            return false;
        };
        self.stores.guild(guild).is_some_and(|guild| {
            dc_state::permissions::can_in_guild(
                guild,
                me,
                dc_state::permissions::CREATE_GUILD_EXPRESSIONS,
            ) || dc_state::permissions::can_in_guild(
                guild,
                me,
                dc_state::permissions::MANAGE_GUILD_EXPRESSIONS,
            )
        })
    }

    /// Opens the tab: its own search field, the pack list requested once, the account's sticker history read.
    pub(super) fn ensure_stickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Find the perfect sticker".to_owned());
            editor
        });
        let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    let query = editor.read(cx).text().to_owned();
                    this.refresh_stickers(&query, cx);
                }
                // Enter picks the first result.
                EditorEvent::Submit(_) => {
                    let first = this.sticker_ui.picker.as_ref().and_then(|picker| {
                        // The highlighted cell (module 118057) before the first result.
                        if let Some(Cell::Sticker(sticker)) = picker.hovered.as_ref() {
                            return Some(sticker.id);
                        }
                        picker.sections.first().and_then(|section| {
                            section.cells.iter().find_map(|cell| match cell {
                                Cell::Sticker(sticker) => Some(sticker.id),
                                Cell::Create => None,
                            })
                        })
                    });
                    if let Some(id) = first {
                        this.choose_sticker(id, false, cx);
                    }
                }
                EditorEvent::Cancel => this.picker = None,
            }
            cx.notify();
        });
        if let Some(settings) = self
            .fetched
            .get(super::actions::FRECENCY_KEY)
            .and_then(|body| body["settings"].as_str())
        {
            let history =
                dc_proto::frecency::id_map(settings, dc_proto::frecency::STICKER_FRECENCY)
                    .into_iter()
                    .map(|(id, item)| {
                        (
                            id,
                            dc_state::frecency::Usage {
                                total_uses: item.total_uses,
                                recent_uses: item.recent_uses,
                            },
                        )
                    })
                    .collect();
            self.stores.stickers.set_history(history);
        }
        if !self.stores.stickers.has_loaded_packs() && self.requested.insert(PACKS_KEY.to_owned()) {
            let _ = self.requests.send(Request::Fetch {
                key: PACKS_KEY.to_owned(),
                route: PACKS_ROUTE.to_owned(),
            });
        }
        self.sticker_ui.picker = Some(StickerPicker {
            search: search.clone(),
            _events: events,
            sections: Vec::new(),
            rows: Vec::new(),
            // Every row is measured at once (a few hundred plain boxes), so scroll distances are exact.
            list: ListState::new(0, ListAlignment::Top, px(400.)).measure_all(),
            visible: 0..VISIBLE_ROWS,
            collapsed: HashSet::new(),
            hovered: None,
            query: String::new(),
            scrolled: false,
            categories: Vec::new(),
        });
        if let Some(picker) = self.sticker_ui.picker.as_ref() {
            let weak = cx.weak_entity();
            picker.list.set_scroll_handler(move |event, _, cx| {
                let range = event.visible_range.clone();
                weak.update(cx, |this, cx| {
                    if let Some(picker) = this.sticker_ui.picker.as_mut() {
                        picker.scrolled = true;
                        picker.visible = range.clone();
                    }
                    this.request_row_art(range.start..range.end + 4);
                    cx.notify();
                })
                .ok();
            });
        }
        self.refresh_stickers("", cx);
        // The tab's own search field takes the keys, as the emoji tab's does (what is typed goes to it).
        window.focus(&search.read(cx).focus_handle(cx), cx);
    }

    /// The pack list arrived (`STICKER_PACKS_FETCH_SUCCESS`).
    pub(super) fn sticker_packs_fetched(&mut self, cx: &mut Context<Self>) {
        if let Some(body) = self.fetched.get(PACKS_KEY).cloned() {
            self.stores.stickers.set_packs(&body);
        }
        if self.sticker_ui.picker.is_some() {
            let query = self
                .sticker_ui
                .picker
                .as_ref()
                .map(|picker| picker.query.clone())
                .unwrap_or_default();
            self.refresh_stickers(&query, cx);
        }
    }

    /// Rebuilds the categories, the grid and the rail's pictures for `query`.
    fn refresh_stickers(&mut self, query: &str, cx: &mut Context<Self>) {
        let categories = self.sticker_categories();
        let current_guild = self.guild;
        let can_create = |guild: Id| self.can_create_stickers(guild);
        let slots = |guild: Id| {
            dc_state::stickers::slots(
                self.stores
                    .guild(guild)
                    .map_or(0, |guild| guild.premium_tier),
            )
        };
        let searching = !query.trim().is_empty();
        let sections = if searching {
            let now = super::picker::now_ms();
            let found = self.stores.stickers.search(
                query,
                now,
                |tags| {
                    tags.split(',')
                        .filter_map(|tag| {
                            dc_tokens::emoji_by_name(tag.trim()).map(|e| e.surrogates.to_owned())
                        })
                        .collect()
                },
                |guild| self.stores.guild(guild).map(|guild| guild.name.clone()),
                |sticker| {
                    matches!(
                        self.sticker_sendability(sticker),
                        Sendability::Sendable | Sendability::SendableWithPremium
                    )
                },
            );
            let (sendable, premium): (Vec<_>, Vec<_>) = found
                .into_iter()
                .partition(|hit| self.sticker_sendability(&hit.sticker) == Sendability::Sendable);
            let cells: Vec<Cell> = sendable
                .into_iter()
                .chain(premium)
                .map(|hit| Cell::Sticker(hit.sticker))
                .collect();
            vec![Section {
                key: "search".to_owned(),
                category: None,
                cells,
            }]
        } else {
            sections_of(&categories, current_guild, can_create, slots)
        };
        let me_collapsed = self
            .sticker_ui
            .picker
            .as_ref()
            .map(|picker| picker.collapsed.clone())
            .unwrap_or_default();
        let rows = rows_of(&sections, &me_collapsed, !searching);
        // The rail's pictures: pack covers and guild icons load at once, the grid's as its rows appear.
        let mut covers: Vec<Sticker> = Vec::new();
        for category in &categories {
            if category.kind == CategoryKind::Pack
                && let Some(cover) = category
                    .id
                    .parse::<Id>()
                    .ok()
                    .and_then(|pack| self.stores.stickers.pack(pack))
                    .and_then(|pack| pack.cover())
            {
                covers.push(cover.clone());
            }
        }
        for cover in covers {
            self.request_sticker_art(&cover);
        }
        let Some(picker) = self.sticker_ui.picker.as_mut() else {
            return;
        };
        picker.hovered = None;
        picker.query = query.to_owned();
        picker.sections = sections;
        picker.rows = rows;
        picker.categories = categories;
        picker.list.reset(picker.rows.len());
        picker.visible = 0..VISIBLE_ROWS;
        self.request_row_art(0..14);
        cx.notify();
    }

    /// Asks for the pictures of the stickers in rows `range`: those in view and a few beyond.
    fn request_row_art(&mut self, range: std::ops::Range<usize>) {
        let wanted: Vec<Sticker> = self
            .sticker_ui
            .picker
            .as_ref()
            .map(|picker| {
                range
                    .filter_map(|index| picker.rows.get(index))
                    .filter_map(|row| match row {
                        StickerRow::Cells { section, start } => {
                            let all = &picker.sections[*section].cells;
                            Some(&all[*start..(*start + COLUMNS).min(all.len())])
                        }
                        _ => None,
                    })
                    .flatten()
                    .filter_map(|cell| match cell {
                        Cell::Sticker(sticker) => Some(sticker.clone()),
                        Cell::Create => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        for sticker in wanted {
            self.request_sticker_art(&sticker);
        }
    }

    /// Asks for a sticker's picture once.
    pub(super) fn request_sticker_art(&mut self, sticker: &Sticker) {
        self.request_image(art_key(sticker.id), art_url(sticker));
    }

    fn toggle_sticker_section(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(picker) = self.sticker_ui.picker.as_mut() else {
            return;
        };
        if !picker.collapsed.remove(key) {
            picker.collapsed.insert(key.to_owned());
        }
        picker.rows = rows_of(&picker.sections, &picker.collapsed, true);
        picker.list.reset(picker.rows.len());
        cx.notify();
    }

    fn jump_to_sticker_section(&mut self, section: usize, cx: &mut Context<Self>) {
        let searching = self
            .sticker_ui
            .picker
            .as_ref()
            .is_some_and(|picker| !picker.query.trim().is_empty());
        if searching {
            // A category clears the search, as the rail lists the full picker.
            if let Some(search) = self
                .sticker_ui
                .picker
                .as_ref()
                .map(|picker| picker.search.clone())
            {
                search.update(cx, |editor, _| editor.set_text(""));
            }
            self.refresh_stickers("", cx);
        }
        let Some(picker) = self.sticker_ui.picker.as_mut() else {
            return;
        };
        if let Some(row) = picker
            .rows
            .iter()
            .position(|row| matches!(row, StickerRow::Heading(index) if *index == section))
        {
            picker.list.scroll_to(ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
        }
        cx.notify();
    }

    /// `handleStickerInspect`: the bar names this cell, and the search field's placeholder too.
    fn inspect_sticker(&mut self, cell: Cell, cx: &mut Context<Self>) {
        let name = match &cell {
            Cell::Sticker(sticker) => sticker.name.clone(),
            Cell::Create => "Upload a sticker to this server".to_owned(),
        };
        let Some(picker) = self.sticker_ui.picker.as_mut() else {
            return;
        };
        if picker.hovered.as_ref() == Some(&cell) {
            return;
        }
        picker.hovered = Some(cell);
        picker
            .search
            .update(cx, |editor, _| editor.set_placeholder(name));
        cx.notify();
    }

    /// An arrow, Home or End over the sticker grid (module 118057): the inspected cell moves and the list follows it.
    pub(super) fn sticker_grid_key(
        &mut self,
        key: &str,
        ctrl: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(picker) = self.sticker_ui.picker.as_ref() else {
            return false;
        };
        let cells: Vec<(usize, usize, usize, usize)> = picker
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| match row {
                StickerRow::Cells { section, start } => Some((
                    index,
                    *section,
                    *start,
                    (picker.sections[*section].cells.len() - start).min(COLUMNS),
                )),
                _ => None,
            })
            .collect();
        let lengths: Vec<usize> = cells.iter().map(|cell| cell.3).collect();
        let at = picker.hovered.as_ref().and_then(|hovered| {
            cells
                .iter()
                .enumerate()
                .find_map(|(row, (_, section, start, len))| {
                    picker.sections[*section].cells[*start..start + len]
                        .iter()
                        .position(|cell| cell == hovered)
                        .map(|col| (row, col))
                })
        });
        let Some(to) = (match at {
            Some(at) => super::keyboard::grid_step(&lengths, at, key, ctrl),
            None => super::keyboard::grid_first(&lengths),
        }) else {
            return !(at == Some((0, 0)) && key == "left") && at.is_some();
        };
        let (row, section, start, _) = cells[to.0];
        let cell = picker.sections[section].cells[start + to.1].clone();
        self.inspect_sticker(cell, cx);
        if let Some(picker) = self.sticker_ui.picker.as_ref() {
            super::keyboard::reveal_row(&picker.list, row);
        }
        true
    }

    /// A click on a sticker (module 989161 `nm`): alt toggles it among the favourites; otherwise it is sent, or
    /// staged in the composer when the message has other content.
    pub(super) fn choose_sticker(&mut self, id: Id, alt: bool, cx: &mut Context<Self>) {
        if alt {
            self.toggle_favorite_sticker(id, cx);
            return;
        }
        let Some(sticker) = self.stores.stickers.sticker(id).cloned() else {
            return;
        };
        match self.sticker_sendability(&sticker) {
            Sendability::Sendable => {}
            Sendability::SendableWithPremium => {
                // The picker's upsell: a sticker of another server needs Nitro.
                self.picker = None;
                self.open_nitro(cx);
                return;
            }
            Sendability::Nonsendable | Sendability::SendableWithBoostedGuild => return,
        }
        let Some(channel) = self.channel else {
            return;
        };
        let has_text = !self.composer.read(cx).text().trim().is_empty();
        if !self.pending_files.is_empty() || self.sticker_ui.staged.is_some() || has_text {
            self.sticker_ui.staged = Some(id);
        } else {
            let reply = self
                .replying
                .take()
                .map(|message| (self.guild, message.id, self.reply_ping));
            self.start_local_sticker(channel, reply, id);
            // `STICKER_TRACK_USAGE`.
            self.stores.stickers.track(&[id], super::picker::now_ms());
        }
        self.picker = None;
        cx.notify();
    }

    /// `uK` and `vr`: the sticker joins the favourites or leaves them, and the whole list goes to the account's
    /// settings (limit 250, as the official strings say).
    fn toggle_favorite_sticker(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.sticker_ui.favorites.is_none() && !self.frecency_arrived() {
            return;
        }
        let mut list = self.favorite_sticker_ids();
        if let Some(at) = list.iter().position(|known| *known == id) {
            list.remove(at);
        } else if list.len() < 250 {
            list.push(id);
        } else {
            return;
        }
        if !crate::storage::settings_writes_blocked() {
            let patch = dc_proto::frecency::favorite_stickers_patch(&list);
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/2".to_owned(),
                body: Some(serde_json::json!({"settings": patch})),
            });
        }
        self.sticker_ui.favorites = Some(list);
        let query = self
            .sticker_ui
            .picker
            .as_ref()
            .map(|picker| picker.query.clone())
            .unwrap_or_default();
        self.refresh_stickers(&query, cx);
    }

    /// The guild's or pack's picture for the inspector's right edge and the section headings.
    fn sticker_source(&self, sticker: &Sticker) -> (String, Option<Id>, Option<Id>) {
        if let Some(guild) = sticker.guild_id.filter(|_| sticker.is_guild()) {
            let name = self
                .stores
                .guild(guild)
                .map(|guild| guild.name.clone())
                .unwrap_or_default();
            (name, Some(guild), None)
        } else {
            let pack = sticker
                .pack_id
                .and_then(|pack| self.stores.stickers.pack(pack));
            (
                pack.map(|pack| pack.name.clone()).unwrap_or_default(),
                None,
                pack.and_then(|pack| pack.cover()).map(|cover| cover.id),
            )
        }
    }

    /// A server's icon, or its acronym on `--background-base-low` (`guildIconWithoutImage`).
    fn sticker_guild_badge(&self, guild: Id, size: f32, radius: f32) -> AnyElement {
        let picture = self.images.get(&format!("guild:{guild}")).cloned();
        let acronym = self
            .stores
            .guild(guild)
            .map(|guild| guild.acronym())
            .unwrap_or_default();
        div()
            .size(px(size))
            .flex_none()
            .rounded(px(radius))
            .overflow_hidden()
            .bg(color("--background-base-low"))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .line_height(px(12.))
            .text_color(color("--text-default"))
            .map(|this| match picture {
                Some(picture) => this.child(img(picture).size(px(size))),
                None => this.child(acronym),
            })
            .into_any_element()
    }

    pub(super) fn sticker_picture(&self, id: Id, size: f32) -> Option<AnyElement> {
        self.images
            .get(&art_key(id))
            .cloned()
            .map(|picture| img(picture).size(px(size)).into_any_element())
    }

    fn sticker_heading(&self, section: usize, sticky: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.sticker_ui.picker.as_ref() else {
            return div().into_any_element();
        };
        let Some(category) = picker.sections[section].category.as_ref() else {
            return div().into_any_element();
        };
        let key = category.id.clone();
        let collapsed = picker.collapsed.contains(&key);
        let icon: Option<AnyElement> = match category.kind {
            CategoryKind::Guild | CategoryKind::EmptyGuildUpsell => category
                .id
                .parse::<Id>()
                .ok()
                .map(|guild| self.sticker_guild_badge(guild, 16., 5.)),
            CategoryKind::Pack => category
                .id
                .parse::<Id>()
                .ok()
                .and_then(|pack| self.stores.stickers.pack(pack))
                .and_then(|pack| pack.cover())
                .and_then(|cover| self.sticker_picture(cover.id, 12.))
                .map(|picture| div().size(px(12.)).child(picture).into_any_element()),
            CategoryKind::Favorite => Some(
                svg()
                    .path("icons/sb_star.svg")
                    .size(px(16.))
                    .text_color(color("--text-default"))
                    .into_any_element(),
            ),
            CategoryKind::Recent => Some(
                svg()
                    .path("icons/picker_cat_recent.svg")
                    .size(px(16.))
                    .text_color(color("--text-default"))
                    .into_any_element(),
            ),
        };
        div()
            .id(SharedString::from(format!(
                "sticker-heading-{section}-{}",
                if sticky { "sticky" } else { "row" }
            )))
            .h(px(HEADING))
            .w_full()
            .pl(px(8.))
            .pr(px(4.))
            .bg(color("--background-surface-high"))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_sticker_section(&key, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w(px(0.))
                    .text_color(color("--text-default"))
                    .children(icon)
                    .child(
                        div()
                            .ml(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .child(category.name.clone()),
                    )
                    .child(
                        svg()
                            .path("icons/picker_collapse.svg")
                            .size(px(16.))
                            .flex_none()
                            .ml(px(4.))
                            .text_color(color("--text-default"))
                            .when(collapsed, |this| {
                                this.with_transformation(gpui::Transformation::rotate(
                                    gpui::radians(std::f32::consts::FRAC_PI_2),
                                ))
                            }),
                    ),
            )
            .into_any_element()
    }

    /// `upsell_NDbtCG`: an empty server's invitation to upload.
    fn picker_sticker_upsell(&self, section: usize, cx: &mut Context<Self>) -> AnyElement {
        let guild = self
            .sticker_ui
            .picker
            .as_ref()
            .and_then(|picker| picker.sections[section].category.as_ref())
            .and_then(|category| category.id.parse::<Id>().ok());
        // Module 357354 `tt` shows "Upload 15+ custom stickers with Server Boosting." only when the Boost level's
        // sticker slot count is 0, which the slot table (5, 15, 30, 60) never gives; every guild sees the first text.
        let (text, button) = (
            "Your server is waiting for you to upload some stickers!",
            "Manage Stickers",
        );
        div()
            .w_full()
            .h(px(UPSELL))
            .pl(px(12.))
            .pr(px(12.))
            .child(
                div()
                    .id(SharedString::from(format!("sticker-upsell-{section}")))
                    .h(px(UPSELL))
                    .rounded(px(4.))
                    .bg(color("--background-base-lowest"))
                    .py(px(8.))
                    .pl(px(8.))
                    .pr(px(12.))
                    .flex()
                    .items_center()
                    .child(
                        svg()
                            .path("icons/sticker.svg")
                            .w(px(24.))
                            .h(px(20.))
                            .flex_none()
                            .text_color(color("--interactive-text-default")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .ml(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(text),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "sticker-upsell-button-{section}"
                            )))
                            .flex_none()
                            .ml(px(8.))
                            .cursor_pointer()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-brand"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.picker = None;
                                if let Some(guild) = guild {
                                    this.open_server_settings_at(
                                        guild,
                                        super::server_settings::Page::Stickers,
                                        cx,
                                    );
                                }
                            }))
                            .child(button),
                    ),
            )
            .into_any_element()
    }

    fn sticker_cell(
        &self,
        row: usize,
        column: usize,
        cell: &Cell,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let inspected = self.inspected_cell().as_ref() == Some(cell);
        let id = SharedString::from(format!("sticker-cell-{row}-{column}"));
        match cell {
            Cell::Create => {
                let on = cell.clone();
                div()
                    .id(id)
                    .size(px(CELL))
                    .p(px(1.))
                    .child(
                        div()
                            .size(px(96.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .bg(if inspected {
                                color("--background-mod-normal")
                            } else {
                                color("--background-base-low")
                            })
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .size(px(36.))
                                    .rounded_full()
                                    .bg(color("--control-primary-background-default"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        svg()
                                            .path("icons/plus_small.svg")
                                            .size(px(18.))
                                            .text_color(color("--interactive-text-active")),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--interactive-text-active"))
                                    .child("Add Sticker"),
                            ),
                    )
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        if *inside {
                            this.inspect_sticker(on.clone(), cx);
                        }
                    }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(guild) = this.guild {
                            this.picker = None;
                            this.open_server_settings_at(
                                guild,
                                super::server_settings::Page::Stickers,
                                cx,
                            );
                        }
                    }))
                    .into_any_element()
            }
            Cell::Sticker(sticker) => {
                let locked = self.sticker_sendability(sticker) == Sendability::SendableWithPremium;
                // Module 357354: `isInteracting` is the inspected cell, and a grid this narrow prefers animation.
                // Every row is built so the list can measure it, but only the rows in view ask for their picture and
                // start to play: a few hundred cells fetching and rendering at once starve the ones on screen.
                #[cfg(feature = "capture")]
                let everywhere = crate::perf::legacy("picker");
                #[cfg(not(feature = "capture"))]
                let everywhere = false;
                let in_view = everywhere
                    || self
                        .sticker_ui
                        .picker
                        .as_ref()
                        .is_none_or(|picker| row_in_view(row, &picker.visible));
                let picture = if in_view {
                    self.sticker_art(sticker.id, sticker.format(), 96., inspected, false, cx)
                } else {
                    None
                };
                let on = cell.clone();
                let chosen = sticker.id;
                div()
                    .id(id)
                    .size(px(CELL))
                    .p(px(1.))
                    .rounded(px(12.))
                    .cursor_pointer()
                    .when(inspected, |this| {
                        this.bg(color("--interactive-background-selected"))
                    })
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        if *inside {
                            this.inspect_sticker(on.clone(), cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                        this.choose_sticker(chosen, event.modifiers().alt, cx);
                    }))
                    .children(picture.map(|picture| {
                        picture
                            .size(px(96.))
                            .when(locked, |this| this.grayscale(true))
                    }))
                    .into_any_element()
            }
        }
    }

    fn sticker_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self
            .sticker_ui
            .picker
            .as_ref()
            .and_then(|picker| picker.rows.get(index).copied())
        else {
            return div().into_any_element();
        };
        match row {
            StickerRow::Heading(section) => self.sticker_heading(section, false, cx),
            StickerRow::Upsell(section) => self.picker_sticker_upsell(section, cx),
            StickerRow::Cells { section, start } => {
                let cells: Vec<Cell> = self
                    .sticker_ui
                    .picker
                    .as_ref()
                    .map(|picker| {
                        let all = &picker.sections[section].cells;
                        all[start..(start + COLUMNS).min(all.len())].to_vec()
                    })
                    .unwrap_or_default();
                div()
                    .h(px(CELL + ROW_GAP))
                    .pl(px(8.))
                    .flex()
                    .items_start()
                    .gap(px(13.))
                    .children(
                        cells
                            .iter()
                            .enumerate()
                            .map(|(column, cell)| self.sticker_cell(index, column, cell, cx)),
                    )
                    .into_any_element()
            }
        }
    }

    /// The category the top of the list is in: the one holding its first row of stickers.
    fn sticker_current_section(&self) -> usize {
        let Some(picker) = self.sticker_ui.picker.as_ref() else {
            return 0;
        };
        let top = picker.list.logical_scroll_top().item_ix;
        picker
            .rows
            .iter()
            .skip(top)
            .find_map(|row| match row {
                StickerRow::Cells { section, .. } => Some(*section),
                _ => None,
            })
            .or_else(|| {
                picker.rows.get(top).map(|row| match row {
                    StickerRow::Heading(section)
                    | StickerRow::Upsell(section)
                    | StickerRow::Cells { section, .. } => *section,
                })
            })
            .unwrap_or(0)
    }

    fn sticker_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.sticker_ui.picker.as_ref() else {
            return div().into_any_element();
        };
        let searching = !picker.query.trim().is_empty();
        let current = self.sticker_current_section();
        let guild_count = picker
            .categories
            .iter()
            .filter(|category| {
                matches!(
                    category.kind,
                    CategoryKind::Guild | CategoryKind::EmptyGuildUpsell
                )
            })
            .count();
        let items: Vec<AnyElement> = picker
            .categories
            .iter()
            .enumerate()
            .map(|(index, category)| {
                let selected = picker.scrolled && !searching && index == current;
                let group = SharedString::from(format!("sticker-rail-{index}"));
                let inner: AnyElement = match category.kind {
                    CategoryKind::Guild | CategoryKind::EmptyGuildUpsell => category
                        .id
                        .parse::<Id>()
                        .ok()
                        .map(|guild| self.sticker_guild_badge(guild, 32., 10.))
                        .unwrap_or_else(|| div().into_any_element()),
                    CategoryKind::Pack => category
                        .id
                        .parse::<Id>()
                        .ok()
                        .and_then(|pack| self.stores.stickers.pack(pack))
                        .and_then(|pack| pack.cover())
                        .and_then(|cover| self.sticker_picture(cover.id, 32.))
                        .unwrap_or_else(|| div().size(px(32.)).into_any_element()),
                    CategoryKind::Favorite | CategoryKind::Recent => div()
                        .size(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(if category.kind == CategoryKind::Favorite {
                                    "icons/sb_star.svg"
                                } else {
                                    "icons/picker_cat_recent.svg"
                                })
                                .size(px(24.))
                                .text_color(color("--interactive-text-default")),
                        )
                        .into_any_element(),
                };
                let first_party = matches!(
                    category.kind,
                    CategoryKind::Pack | CategoryKind::Favorite | CategoryKind::Recent
                );
                let tile = div()
                    .id(SharedString::from(format!("sticker-rail-item-{index}")))
                    .group(group.clone())
                    .relative()
                    .flex_none()
                    .size(px(32.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(selected && first_party, |this| {
                        this.bg(color("--background-base-low"))
                    })
                    .when(!selected && first_party, |this| {
                        this.hover(|style| style.bg(color("--interactive-background-hover")))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.jump_to_sticker_section(index, cx);
                    }))
                    // `categoryPill`: a 4px bar, 20px tall, on the selected or hovered category.
                    .child(
                        div()
                            .absolute()
                            .left(px(-8.))
                            .top(px(6.))
                            .w(px(4.))
                            .h(px(20.))
                            .rounded_r(px(4.))
                            .bg(color("--interactive-text-active"))
                            .when(!selected, |this| {
                                this.invisible()
                                    .group_hover(group.clone(), |style| style.visible())
                            }),
                    )
                    .child(inner);
                let _ = guild_count;
                self.with_tip(
                    format!("sticker-rail-tip-{index}"),
                    Tip::rail(Body::Heading(category.name.clone().into())),
                    tile,
                    cx,
                )
                .mb(px(8.))
                .into_any_element()
            })
            .collect();
        div()
            .id("sticker-rail")
            .parity("sticker-rail")
            .w(px(48.))
            .h_full()
            .flex_none()
            .bg(color("--background-base-lower"))
            .overflow_y_scroll()
            .p(px(8.))
            .flex()
            .flex_col()
            .children(items)
            .into_any_element()
    }

    /// The inspected cell: the last one hovered, else the first sticker of the grid.
    fn inspected_cell(&self) -> Option<Cell> {
        let picker = self.sticker_ui.picker.as_ref()?;
        picker.hovered.clone().or_else(|| {
            picker
                .sections
                .iter()
                .flat_map(|section| section.cells.iter())
                .next()
                .cloned()
        })
    }

    /// `inspector_1bDrGE`: the inspected sticker's name and where it comes from.
    fn sticker_inspector(&self) -> AnyElement {
        if self.sticker_ui.picker.is_none() {
            return div().into_any_element();
        }
        let cell = self.inspected_cell();
        let bar = div()
            .probe("sticker-inspector", [1., 0., 0., 0.])
            .h(px(48.))
            .flex_none()
            .px(px(16.))
            .bg(color("--background-base-lower"))
            .border_t_1()
            .border_color(color("--border-normal"))
            .flex()
            .items_center();
        let Some(cell) = cell else {
            return bar.into_any_element();
        };
        match cell {
            Cell::Create => bar
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child("Upload a sticker to this server"),
                )
                .into_any_element(),
            Cell::Sticker(sticker) => {
                let (source, guild, cover) = self.sticker_source(&sticker);
                bar.child(
                    div()
                        .size(px(28.))
                        .flex_none()
                        .children(self.sticker_picture(sticker.id, 28.)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .ml(px(8.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(sticker.name.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--interactive-text-default"))
                                .child("from ")
                                .child(div().font_weight(FontWeight(700.)).child(source)),
                        ),
                )
                .child(div().size(px(32.)).flex_none().ml(px(8.)).map(|this| {
                    match (guild, cover) {
                        (Some(guild), _) => this.child(self.sticker_guild_badge(guild, 32., 10.)),
                        (None, Some(cover)) => this.children(self.sticker_picture(cover, 32.)),
                        _ => this,
                    }
                }))
                .into_any_element()
            }
        }
    }

    /// The list, or the "no results" page with its suggestions.
    fn sticker_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.sticker_ui.picker.as_ref() else {
            return div().into_any_element();
        };
        let searching = !picker.query.trim().is_empty();
        if searching && picker.rows.is_empty() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .px(px(16.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child("Try searching for another term or select one below"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(8.))
                        .children(SUGGESTIONS.iter().map(|word| {
                            let word = *word;
                            div()
                                .id(SharedString::from(format!("sticker-suggestion-{word}")))
                                .px(px(10.))
                                .py(px(4.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .bg(color("--background-mod-subtle"))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(search) = this
                                        .sticker_ui
                                        .picker
                                        .as_ref()
                                        .map(|picker| picker.search.clone())
                                    {
                                        search.update(cx, |editor, _| editor.set_text(word));
                                    }
                                    this.refresh_stickers(word, cx);
                                }))
                                .child(word)
                        })),
                )
                .into_any_element();
        }
        list(
            picker.list.clone(),
            cx.processor(|this, index, _, cx| this.sticker_row(index, cx)),
        )
        .size_full()
        .into_any_element()
    }

    /// The thin scroll bar over the list's right edge (`thin_tqapBM`): a 4px thumb 2px from the edge, at least 36px
    /// tall, inset 2px from the top and bottom.
    fn sticker_scrollbar(&self) -> impl IntoElement {
        let (heights, offset) = self
            .sticker_ui
            .picker
            .as_ref()
            .map(|picker| {
                let top = picker.list.logical_scroll_top();
                let before: f32 = picker.rows[..top.item_ix.min(picker.rows.len())]
                    .iter()
                    .map(row_height)
                    .sum();
                let total: f32 = picker.rows.iter().map(row_height).sum();
                (total, before + f32::from(top.offset_in_item))
            })
            .unwrap_or_default();
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<gpui::Pixels>, _, window, _| {
                let view = f32::from(bounds.size.height);
                let max = heights - view;
                if max <= 1. {
                    return;
                }
                let track = view - 4.;
                let thumb = (track * view / heights).max(36.);
                let top = 2. + (offset / max).clamp(0., 1.) * (track - thumb);
                let area = Bounds::new(
                    point(bounds.right() - px(6.), bounds.top() + px(top)),
                    size(px(4.), px(thumb)),
                );
                window.paint_quad(fill(area, color("--scrollbar-thin-thumb")).corner_radii(px(2.)));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// The tab's panel under the tab bar: the search field, the rail, the list and the inspector.
    pub(super) fn sticker_panel(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.sticker_ui.picker.as_ref() else {
            return div().into_any_element();
        };
        let search = picker.search.clone();
        let focused = search.read(cx).focus_handle(cx).is_focused(window);
        let header = div().h(px(53.)).px(px(16.)).pb(px(12.)).flex().child(
            div()
                .parity_with_border("sticker-search", 1.)
                .w(px(464.))
                .h(px(40.))
                .rounded(px(8.))
                .border_1()
                .border_color(if focused {
                    color("--input-border-active")
                } else {
                    color("--input-border-default")
                })
                .rule(crate::ui::motion::Rule::TextField)
                .bg(color("--input-background-default"))
                .pl(px(4.))
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .size(px(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path("icons/picker_search.svg")
                                .size(px(16.))
                                .text_color(color("--input-text-default")),
                        ),
                )
                .child(div().flex_1().min_w(px(0.)).pr(px(12.)).child(search)),
        );
        let sticky = (picker.query.trim().is_empty() && !picker.rows.is_empty())
            .then(|| {
                picker
                    .rows
                    .get(picker.list.logical_scroll_top().item_ix)
                    .map(|row| match row {
                        StickerRow::Heading(section)
                        | StickerRow::Upsell(section)
                        | StickerRow::Cells { section, .. } => *section,
                    })
            })
            .flatten();
        let body = div()
            .parity("sticker-list")
            .relative()
            .flex_1()
            .min_h(px(0.))
            .child(self.sticker_list(cx))
            .child(self.sticker_scrollbar())
            .children(sticky.map(|section| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(432.))
                    .child(self.sticker_heading(section, true, cx))
            }));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.sticker_rail(cx))
                    .child(
                        div()
                            .w(px(448.))
                            .flex()
                            .flex_col()
                            .child(body)
                            .child(self.sticker_inspector()),
                    ),
            )
            .into_any_element()
    }

    /// The staged sticker's preview above the composer (`ADD_STICKER_PREVIEW`): a 48px picture and a remove button.
    pub(super) fn staged_sticker_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.sticker_ui.staged?;
        // Module 989161 draws the preview with `isInteracting` while it is focused or hovered.
        let format = self
            .stores
            .stickers
            .sticker(id)
            .and_then(|sticker| sticker.format());
        let picture = self.sticker_art(id, format, 48., false, false, cx);
        Some(
            div()
                .relative()
                .flex_none()
                .size(px(48.))
                .children(picture.map(|picture| picture.size(px(48.))))
                .child(
                    div()
                        .id("staged-sticker-remove")
                        .absolute()
                        .top(px(-8.))
                        .right(px(-8.))
                        .size(px(16.))
                        .rounded_full()
                        .bg(color("--background-surface-highest"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sticker_ui.staged = None;
                            cx.notify();
                        }))
                        .child(
                            svg()
                                .path("icons/upload_remove.svg")
                                .size(px(12.))
                                .text_color(color("--control-critical-secondary-text-default")),
                        ),
                )
                .into_any_element(),
        )
    }

    /// A sticker in a message (`stickerContainer`): 160px, from the media proxy or the Lottie renderer.
    pub(super) fn message_stickers(
        &self,
        message: Id,
        stickers: &[(Id, String, u64)],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if stickers.is_empty() {
            return None;
        }
        // `isInteracting` of the message row (module 679636 `rH`): the pointer is over the message.
        let interacting = self.hovered == Some(message);
        Some(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .children(stickers.iter().map(|(id, name, format_type)| {
                    self.sticker_target(*id, name, *format_type, interacting, MESSAGE_STICKER, cx)
                }))
                .into_any_element(),
        )
    }

    /// The picker's rail jumps to a pack's section (the "View More Stickers" button of the sticker popout).
    pub(super) fn show_sticker_pack(&mut self, pack: Id, cx: &mut Context<Self>) {
        let index = self.sticker_ui.picker.as_ref().and_then(|picker| {
            picker.sections.iter().position(|section| {
                section
                    .category
                    .as_ref()
                    .is_some_and(|category| category.id == pack.to_string())
            })
        });
        if let Some(index) = index {
            self.jump_to_sticker_section(index, cx);
        }
    }
}

/// The pictures to fetch for a message's stickers (`o6` reads `sticker_items`).
pub(super) fn wanted(message: &dc_state::Message) -> Vec<(String, String)> {
    message
        .sticker_items
        .iter()
        .map(|(id, name, format_type)| {
            let sticker = Sticker {
                id: *id,
                name: name.clone(),
                description: None,
                tags: None,
                kind: 0,
                format_type: *format_type,
                guild_id: None,
                pack_id: None,
                available: true,
                user_id: None,
            };
            (art_key(*id), art_url(&sticker))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sticker(format_type: u64) -> Sticker {
        Sticker {
            id: 5,
            name: "s".to_owned(),
            description: None,
            tags: None,
            kind: 1,
            format_type,
            guild_id: None,
            pack_id: Some(1),
            available: true,
            user_id: None,
        }
    }

    #[test]
    fn only_the_rows_in_view_and_the_next_ask_for_their_picture() {
        let visible = 10..14;
        assert!(!row_in_view(8, &visible));
        assert!(row_in_view(9, &visible));
        assert!(row_in_view(10, &visible));
        assert!(row_in_view(13, &visible));
        assert!(row_in_view(14, &visible));
        assert!(!row_in_view(15, &visible));
        assert!(!row_in_view(300, &visible));
        assert!(row_in_view(0, &(0..VISIBLE_ROWS)));
    }

    #[test]
    fn art_urls_follow_module_378058() {
        assert_eq!(art_url(&sticker(3)), "https://discord.com/stickers/5.json");
        assert_eq!(
            art_url(&sticker(1)),
            "https://media.discordapp.net/stickers/5.webp?size=320&quality=lossless"
        );
        assert!(art_url(&sticker(2)).ends_with(".png?size=320&passthrough=false"));
    }

    #[test]
    fn rows_list_headings_the_upsell_and_four_cells_a_row() {
        let upsell = Section {
            key: "10".to_owned(),
            category: Some(Category {
                kind: CategoryKind::EmptyGuildUpsell,
                id: "10".to_owned(),
                name: "g".to_owned(),
                stickers: Vec::new(),
            }),
            cells: Vec::new(),
        };
        let pack = Section {
            key: "1".to_owned(),
            category: Some(Category {
                kind: CategoryKind::Pack,
                id: "1".to_owned(),
                name: "p".to_owned(),
                stickers: Vec::new(),
            }),
            cells: (0..9).map(|_| Cell::Sticker(sticker(1))).collect(),
        };
        let rows = rows_of(&[upsell, pack], &HashSet::new(), true);
        assert_eq!(rows.len(), 2 + 1 + 3);
        let mut collapsed = HashSet::new();
        collapsed.insert("1".to_owned());
        let folded = rows_of(
            &[Section {
                key: "1".to_owned(),
                category: None,
                cells: (0..9).map(|_| Cell::Sticker(sticker(1))).collect(),
            }],
            &collapsed,
            true,
        );
        assert_eq!(folded.len(), 1);
    }

    #[test]
    fn the_guild_list_ends_with_the_upload_tile_while_slots_remain() {
        let category = Category {
            kind: CategoryKind::Guild,
            id: "10".to_owned(),
            name: "g".to_owned(),
            stickers: vec![sticker(1)],
        };
        let sections = sections_of(std::slice::from_ref(&category), Some(10), |_| true, |_| 5);
        assert_eq!(sections[0].cells.last(), Some(&Cell::Create));
        let full = sections_of(std::slice::from_ref(&category), Some(10), |_| true, |_| 1);
        assert_eq!(full[0].cells.len(), 1);
        let other = sections_of(&[category], Some(11), |_| true, |_| 5);
        assert_eq!(other[0].cells.len(), 1);
    }
}
