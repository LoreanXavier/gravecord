//! The expression picker's emoji tab (`emoji-picker-tab-panel`), measured in
//! `design/dom/pickers/emoji-composer-darker-cozy.json`: tabs, the search field, the skin tone button and Add Emoji
//! over a 48px category rail and a 448px list of 32px section headings and rows of nine 48px cells, with the hovered
//! emoji named in a 48px inspector. Unicode emoji are cells of the sprite sheet (module 617057, `spriteIndex`).

use super::actions::FRECENCY_KEY;
use super::{Root, color, editor_style};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Id;
use dc_state::frecency::{Frecency, Kind, Usage};
use dc_tokens::emoji_data::{CATEGORIES, EMOJIS};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, ListAlignment, ListOffset,
    ListState, Pixels, Point, SharedString, Subscription, Window, deferred, div, img, list,
    prelude::*, px, svg,
};
use std::collections::HashSet;

const COLUMNS: usize = 9;
const CELL: f32 = 48.;
const HEADING: f32 = 32.;
const SECTION_GAP: f32 = 12.;
pub(super) const PICKER_WIDTH: f32 = 498.;
pub(super) const PICKER_HEIGHT: f32 = 486.;
/// The reaction picker has no tabs but a taller list (`design/dom/pickers/emoji-reaction-darker-cozy.json`).
const REACTION_HEIGHT: f32 = 509.;
/// Popouts keep this far from the window's edges when nudged (`nudgeTopAlignment`, module 623646).
const EDGE: f32 = 12.;

/// What module 121681 tracks for an account with no emoji history, once each.
const DEFAULT_EMOJI: [&str; 10] = [
    "thumbsup",
    "eyes",
    "laughing",
    "watermelon",
    "fork_and_knife",
    "yum",
    "weary",
    "tired_face",
    "poop",
    "100",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Composer,
    Reaction { channel: Id, message: Id },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Item {
    /// An index into `EMOJIS`.
    Unicode(u16),
    Custom {
        id: Id,
        name: String,
        animated: bool,
    },
}

impl Item {
    fn name(&self) -> &str {
        match self {
            Item::Unicode(index) => EMOJIS[usize::from(*index)].names[0],
            Item::Custom { name, .. } => name,
        }
    }

    /// Image key in `Root::images`.
    fn image_key(&self) -> String {
        match self {
            Item::Unicode(index) => {
                let emoji = &EMOJIS[usize::from(*index)];
                match emoji.sprite {
                    Some(sprite) if emoji.has_diversity => format!("dsprite:{sprite}"),
                    Some(sprite) => format!("sprite:{sprite}"),
                    None => format!("emoji:{}", emoji.surrogates),
                }
            }
            Item::Custom { id, .. } => format!("custom:{id}"),
        }
    }
}

#[derive(Clone, Debug)]
enum SectionIcon {
    Category(&'static str),
    Guild(Id),
}

#[derive(Clone, Debug)]
struct Section {
    key: String,
    label: String,
    icon: SectionIcon,
    items: Vec<Item>,
}

#[derive(Clone, Copy, Debug)]
enum Row {
    Heading(usize),
    Cells {
        section: usize,
        start: usize,
        last: bool,
    },
}

pub(crate) struct Picker {
    pub target: Target,
    /// The window position the picker opens from: the composer's emoji button, or the hover bar's react button.
    pub anchor: Point<Pixels>,
    search: Entity<Editor>,
    _search_events: Subscription,
    sections: Vec<Section>,
    /// The unfiltered sections, which the category rail always lists.
    all: Vec<Section>,
    rows: Vec<Row>,
    list: ListState,
    collapsed: HashSet<String>,
    hovered: Option<Item>,
    /// Super Reactions on: reactions go out as bursts (`type=1`).
    burst: bool,
}

impl Picker {
    #[cfg(feature = "capture")]
    pub fn search(&self) -> Entity<Editor> {
        self.search.clone()
    }
}

/// `Nu` in module 822123.
fn category_label(category: &str) -> &'static str {
    match category {
        "people" => "People",
        "nature" => "Nature",
        "food" => "Food",
        "activity" => "Activities",
        "travel" => "Travel",
        "objects" => "Objects",
        "symbols" => "Symbols",
        "flags" => "Flags",
        _ => "",
    }
}

fn category_icon(category: &str) -> &'static str {
    match category {
        "recent" => "picker_cat_recent",
        "people" => "picker_cat_people",
        "nature" => "picker_cat_nature",
        "food" => "picker_cat_food",
        "activity" => "picker_cat_activity",
        "travel" => "picker_cat_travel",
        "objects" => "picker_cat_objects",
        "symbols" => "picker_cat_symbols",
        _ => "picker_cat_flags",
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// Unicode emoji whose names contain `query`: an exact name first, then names starting with it, then the rest, each
/// group in dataset order.
pub(crate) fn search_unicode(query: &str) -> Vec<u16> {
    let query = query.to_lowercase().replace(' ', "_");
    let mut ranked: Vec<(u8, u16)> = EMOJIS
        .iter()
        .enumerate()
        .take_while(|(index, _)| {
            CATEGORIES
                .last()
                .is_none_or(|(_, _, end)| *index < usize::from(*end))
        })
        .filter_map(|(index, emoji)| {
            let rank = emoji
                .names
                .iter()
                .filter_map(|name| {
                    if *name == query {
                        Some(0)
                    } else if name.starts_with(&query) {
                        Some(1)
                    } else if name.contains(&query) {
                        Some(2)
                    } else {
                        None
                    }
                })
                .min()?;
            Some((rank, index as u16))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, index)| index).collect()
}

fn rows_of(sections: &[Section], collapsed: &HashSet<String>, headings: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for (section, entry) in sections.iter().enumerate() {
        if headings {
            rows.push(Row::Heading(section));
        }
        if collapsed.contains(&entry.key) {
            continue;
        }
        let count = entry.items.len().div_ceil(COLUMNS);
        for chunk in 0..count {
            rows.push(Row::Cells {
                section,
                start: chunk * COLUMNS,
                last: chunk + 1 == count,
            });
        }
    }
    rows
}

impl Root {
    fn emoji_lookup(&self, key: &str) -> Option<Item> {
        if let Ok(id) = key.parse::<u64>()
            && let Some(emoji) = self
                .stores
                .guilds
                .iter()
                .flat_map(|guild| &guild.emojis)
                .find(|emoji| emoji.id == id && emoji.available)
        {
            return Some(Item::Custom {
                id,
                name: emoji.name.clone(),
                animated: emoji.animated,
            });
        }
        let index = dc_tokens::emoji_data::NAMES
            .binary_search_by(|(name, _)| (*name).cmp(key))
            .ok()?;
        Some(Item::Unicode(dc_tokens::emoji_data::NAMES[index].1))
    }

    /// `eA.frequently`: emoji picker uses, with this session's on top.
    fn frequent_emoji(&self) -> Vec<Item> {
        let mut frecency = Frecency::new(Kind::Emoji);
        let stored = self
            .fetched
            .get(FRECENCY_KEY)
            .and_then(|body| body["settings"].as_str())
            .map(|settings| {
                dc_proto::frecency::emoji_map(settings, dc_proto::frecency::EMOJI_FRECENCY)
            })
            .unwrap_or_default();
        let now = now_ms();
        if stored.is_empty() {
            for name in DEFAULT_EMOJI {
                frecency.track(name, now);
            }
        } else {
            frecency.overwrite(
                stored
                    .into_iter()
                    .map(|(key, item)| {
                        (
                            key,
                            Usage {
                                total_uses: item.total_uses,
                                recent_uses: item.recent_uses,
                            },
                        )
                    })
                    .collect(),
            );
        }
        for (name, at) in &self.emoji_used {
            frecency.track(name, *at);
        }
        frecency
            .frequently(now, |key| self.emoji_lookup(key).is_some())
            .iter()
            .filter_map(|key| self.emoji_lookup(key))
            .collect()
    }

    fn picker_sections(&self, query: &str) -> Vec<Section> {
        if !query.trim().is_empty() {
            let lowered = query.trim().to_lowercase();
            let mut items: Vec<Item> = self
                .stores
                .guilds
                .iter()
                .flat_map(|guild| &guild.emojis)
                .filter(|emoji| emoji.available && emoji.name.to_lowercase().contains(&lowered))
                .map(|emoji| Item::Custom {
                    id: emoji.id,
                    name: emoji.name.clone(),
                    animated: emoji.animated,
                })
                .collect();
            items.extend(search_unicode(query.trim()).into_iter().map(Item::Unicode));
            return vec![Section {
                key: "search".to_owned(),
                label: String::new(),
                icon: SectionIcon::Category("recent"),
                items,
            }];
        }
        let mut sections = vec![Section {
            key: "recent".to_owned(),
            label: "Frequently Used".to_owned(),
            icon: SectionIcon::Category("recent"),
            items: self.frequent_emoji(),
        }];
        for guild in &self.stores.guilds {
            let items: Vec<Item> = guild
                .emojis
                .iter()
                .filter(|emoji| emoji.available)
                .map(|emoji| Item::Custom {
                    id: emoji.id,
                    name: emoji.name.clone(),
                    animated: emoji.animated,
                })
                .collect();
            if !items.is_empty() {
                sections.push(Section {
                    key: format!("guild:{}", guild.id),
                    label: guild.name.clone(),
                    icon: SectionIcon::Guild(guild.id),
                    items,
                });
            }
        }
        for (category, start, end) in CATEGORIES {
            sections.push(Section {
                key: (*category).to_owned(),
                label: category_label(category).to_owned(),
                icon: SectionIcon::Category(category),
                items: (*start..*end).map(Item::Unicode).collect(),
            });
        }
        sections
    }

    pub(super) fn open_picker(
        &mut self,
        target: Target,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .picker
            .as_ref()
            .is_some_and(|open| open.target == target)
        {
            self.picker = None;
            cx.notify();
            return;
        }
        if self.requested.insert("emoji-sheet".to_owned()) {
            let _ = self.requests.send(Request::EmojiSheet);
        }
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder(
                match target {
                    Target::Composer => "Find the perfect emoji",
                    Target::Reaction { .. } => "Find the perfect reaction",
                }
                .to_owned(),
            );
            editor
        });
        let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    let query = editor.read(cx).text().to_owned();
                    this.refresh_picker(&query);
                }
                // Enter picks the first result.
                EditorEvent::Submit(_) => {
                    let first = this
                        .picker
                        .as_ref()
                        .and_then(|picker| picker.sections.first())
                        .and_then(|section| section.items.first())
                        .cloned();
                    if let Some(item) = first {
                        this.choose_emoji(item, false, cx);
                    }
                }
                EditorEvent::Cancel => this.picker = None,
            }
            cx.notify();
        });
        let sections = self.picker_sections("");
        let collapsed = HashSet::new();
        let rows = rows_of(&sections, &collapsed, true);
        let list = ListState::new(rows.len(), ListAlignment::Top, px(400.));
        let hovered = sections
            .first()
            .and_then(|section| section.items.first())
            .cloned();
        self.request_picker_images(&sections);
        self.picker = Some(Picker {
            target,
            anchor,
            search: search.clone(),
            _search_events: events,
            all: sections.clone(),
            sections,
            rows,
            list,
            collapsed,
            hovered,
            burst: false,
        });
        window.focus(&search.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    fn request_picker_images(&mut self, sections: &[Section]) {
        let mut wanted = Vec::new();
        for item in sections.iter().flat_map(|section| &section.items) {
            match item {
                Item::Custom { id, animated, .. } => {
                    let extension = if *animated { "gif" } else { "webp" };
                    wanted.push((
                        format!("custom:{id}"),
                        format!("https://cdn.discordapp.com/emojis/{id}.{extension}?size=48"),
                    ));
                }
                Item::Unicode(index) if EMOJIS[usize::from(*index)].sprite.is_none() => {
                    let surrogates = EMOJIS[usize::from(*index)].surrogates;
                    if let Some(file) = dc_tokens::emoji_file(surrogates) {
                        wanted.push((
                            format!("emoji:{surrogates}"),
                            format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN),
                        ));
                    }
                }
                Item::Unicode(_) => {}
            }
        }
        for (key, url) in wanted {
            self.request_image(key, url);
        }
    }

    fn refresh_picker(&mut self, query: &str) {
        let sections = self.picker_sections(query);
        self.request_picker_images(&sections);
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let headings = query.trim().is_empty();
        picker.rows = rows_of(&sections, &picker.collapsed, headings);
        picker.hovered = sections
            .first()
            .and_then(|section| section.items.first())
            .cloned();
        picker.sections = sections;
        picker.list.reset(picker.rows.len());
    }

    fn toggle_section(&mut self, key: &str) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if !picker.collapsed.remove(key) {
            picker.collapsed.insert(key.to_owned());
        }
        picker.rows = rows_of(&picker.sections, &picker.collapsed, true);
        picker.list.reset(picker.rows.len());
    }

    fn jump_to_section(&mut self, section: usize, cx: &mut Context<Self>) {
        let searching = self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.sections.len() != picker.all.len());
        if searching {
            // A category clears the search, as the rail lists the full picker.
            if let Some(search) = self.picker.as_ref().map(|picker| picker.search.clone()) {
                search.update(cx, |editor, _| editor.set_text(""));
            }
            self.refresh_picker("");
        }
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let row = picker
            .rows
            .iter()
            .position(|row| matches!(row, Row::Heading(index) if *index == section));
        if let Some(row) = row {
            picker.list.scroll_to(ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
        }
    }

    /// A click on an emoji: into the composer, or onto the message. Shift keeps the picker open.
    fn choose_emoji(&mut self, item: Item, keep_open: bool, cx: &mut Context<Self>) {
        let Some((target, burst)) = self
            .picker
            .as_ref()
            .map(|picker| (picker.target, picker.burst))
        else {
            return;
        };
        let (text, reaction) = match &item {
            Item::Unicode(index) => {
                let surrogates = EMOJIS[usize::from(*index)].surrogates;
                (surrogates.to_owned(), surrogates.to_owned())
            }
            Item::Custom { id, name, animated } => (
                format!("<{}:{name}:{id}>", if *animated { "a" } else { "" }),
                format!("{name}:{id}"),
            ),
        };
        let usage_key = match &item {
            Item::Unicode(_) => item.name().to_owned(),
            Item::Custom { id, .. } => id.to_string(),
        };
        match target {
            Target::Composer => {
                self.emoji_used.push((usage_key, now_ms()));
                self.composer
                    .update(cx, |editor, cx| editor.insert(&text, cx));
            }
            Target::Reaction { channel, message } => {
                self.reactions_used.push((usage_key, now_ms()));
                let _ = self.requests.send(Request::React {
                    channel,
                    message,
                    emoji: reaction,
                    add: true,
                    location: "Message Reaction Picker",
                    burst,
                });
            }
        }
        if !keep_open {
            self.picker = None;
        }
        cx.notify();
    }

    fn picker_heading(&self, section: usize, sticky: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.picker.as_ref() else {
            return div().into_any_element();
        };
        let entry = &picker.sections[section];
        let collapsed = picker.collapsed.contains(&entry.key);
        let key = entry.key.clone();
        let icon_element = match &entry.icon {
            SectionIcon::Category(category) => svg()
                .path(format!("icons/{}.svg", category_icon(category)))
                .size(px(16.))
                .flex_none()
                .text_color(color("--text-default"))
                .into_any_element(),
            SectionIcon::Guild(guild) => div()
                .size(px(16.))
                .rounded(px(4.))
                .overflow_hidden()
                .children(
                    self.images
                        .get(&format!("guild:{guild}"))
                        .cloned()
                        .map(|picture| img(picture).size(px(16.))),
                )
                .into_any_element(),
        };
        div()
            .id(SharedString::from(format!(
                "picker-heading-{section}-{}",
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_section(&key);
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w(px(0.))
                    .text_color(color("--text-default"))
                    .child(icon_element)
                    .child(
                        div()
                            .ml(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .child(entry.label.clone()),
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

    fn picker_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.picker.as_ref() else {
            return div().into_any_element();
        };
        match picker.rows.get(index).copied() {
            Some(Row::Heading(section)) => self.picker_heading(section, false, cx),
            Some(Row::Cells {
                section,
                start,
                last,
            }) => {
                let items = &picker.sections[section].items;
                let end = (start + COLUMNS).min(items.len());
                div()
                    .h(px(CELL + if last { SECTION_GAP } else { 0. }))
                    .pl(px(8.))
                    .flex()
                    .items_start()
                    .children(items[start..end].iter().enumerate().map(|(offset, item)| {
                        let selected = picker.hovered.as_ref() == Some(item);
                        let picture = self.images.get(&item.image_key()).cloned();
                        let hovered_item = item.clone();
                        let chosen = item.clone();
                        div()
                            .id(SharedString::from(format!("picker-cell-{index}-{offset}")))
                            .size(px(CELL))
                            .p(px(4.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .when(selected, |this| {
                                this.bg(color("--interactive-background-selected"))
                            })
                            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                                if *inside && let Some(picker) = this.picker.as_mut() {
                                    picker.hovered = Some(hovered_item.clone());
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                this.choose_emoji(chosen.clone(), event.modifiers().shift, cx);
                            }))
                            .children(picture.map(|picture| img(picture).size(px(40.))))
                    }))
                    .into_any_element()
            }
            None => div().into_any_element(),
        }
    }

    fn picker_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.picker.as_ref() else {
            return div().into_any_element();
        };
        let top_row = picker.list.logical_scroll_top().item_ix;
        let searching = picker.sections.len() != picker.all.len();
        let current = picker
            .rows
            .get(top_row)
            .filter(|_| !searching)
            .map(|row| match row {
                Row::Heading(section) | Row::Cells { section, .. } => *section,
            })
            .unwrap_or(0);
        div()
            .id("picker-rail")
            .w(px(48.))
            .h_full()
            .bg(color("--background-base-lower"))
            .overflow_y_scroll()
            .pt(px(8.))
            .px(px(8.))
            .flex()
            .flex_col()
            .children(picker.all.iter().enumerate().map(|(index, section)| {
                let selected = index == current;
                let group = SharedString::from(format!("picker-rail-{index}"));
                let inner = match &section.icon {
                    SectionIcon::Category(category) => svg()
                        .path(format!("icons/{}.svg", category_icon(category)))
                        .size(px(24.))
                        .flex_none()
                        .text_color(if selected {
                            color("--interactive-text-active")
                        } else {
                            color("--interactive-text-default")
                        })
                        .group_hover(group.clone(), |style| {
                            style.text_color(color("--interactive-text-hover"))
                        })
                        .into_any_element(),
                    SectionIcon::Guild(guild) => div()
                        .size(px(24.))
                        .rounded_full()
                        .overflow_hidden()
                        .children(
                            self.images
                                .get(&format!("guild:{guild}"))
                                .cloned()
                                .map(|picture| img(picture).size(px(24.))),
                        )
                        .into_any_element(),
                };
                div()
                    .id(SharedString::from(format!("picker-rail-item-{index}")))
                    .group(group)
                    .relative()
                    .flex_none()
                    .size(px(32.))
                    .p(px(4.))
                    .mb(px(if index == 0 { 8. } else { 2. }))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(selected, |this| this.bg(color("--background-base-low")))
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(color("--interactive-background-hover")))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.jump_to_section(index, cx);
                        cx.notify();
                    }))
                    .when(selected, |this| {
                        // `item__o8_Xq selected`: an 8 by 24 white pill, its left half outside the rail.
                        this.child(
                            div()
                                .absolute()
                                .left(px(-8.))
                                .top(px(4.))
                                .w(px(4.))
                                .h(px(24.))
                                .rounded_r(px(4.))
                                .bg(color("--interactive-text-active")),
                        )
                    })
                    .child(inner)
            }))
            .into_any_element()
    }

    fn picker_inspector(&self) -> AnyElement {
        let hovered = self
            .picker
            .as_ref()
            .and_then(|picker| picker.hovered.clone());
        div()
            .h(px(48.))
            .flex_none()
            .px(px(16.))
            .bg(color("--background-base-lower"))
            .border_t_1()
            .border_color(color("--border-normal"))
            .flex()
            .items_center()
            .when_some(hovered, |this, item| {
                let picture = match &item {
                    Item::Unicode(index) => {
                        let surrogates = EMOJIS[usize::from(*index)].surrogates;
                        self.images
                            .get(&format!("emoji:{surrogates}"))
                            .or_else(|| self.images.get(&item.image_key()))
                            .cloned()
                    }
                    Item::Custom { .. } => self.images.get(&item.image_key()).cloned(),
                };
                this.child(
                    div()
                        .size(px(28.))
                        .children(picture.map(|picture| img(picture).size(px(28.)))),
                )
                .child(
                    div()
                        .ml(px(8.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(format!(":{}:", item.name())),
                )
            })
            .into_any_element()
    }

    pub(super) fn picker_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let picker = self.picker.as_ref()?;
        let viewport = window.viewport_size();
        // Composer: the right edge 32px in from the window's, the bottom 8px above the composer.
        // Reactions: beside the hover bar, kept inside the window.
        let reaction = matches!(picker.target, Target::Reaction { .. });
        let height = if reaction {
            REACTION_HEIGHT
        } else {
            PICKER_HEIGHT
        };
        // Composer: the right edge 32px in from the window's, the bottom at the anchor (8px above the composer).
        // Reactions: 8px left of the button, top-aligned, then nudged up to stay 12px inside the window.
        let (left, top) = if reaction {
            (
                f32::from(picker.anchor.x) - PICKER_WIDTH - 8.,
                f32::from(picker.anchor.y).min(f32::from(viewport.height) - EDGE - height),
            )
        } else {
            (
                f32::from(viewport.width) - 32. - PICKER_WIDTH,
                f32::from(picker.anchor.y) - height,
            )
        };
        let left = left.clamp(
            EDGE,
            (f32::from(viewport.width) - PICKER_WIDTH - EDGE).max(EDGE),
        );
        let top = top.max(EDGE);
        let rows = self.picker_rows(cx);
        let tab = |id: &'static str, label: &'static str, selected: bool| {
            div()
                .id(id)
                .h(px(30.))
                .px(px(12.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(if selected {
                    color("--text-strong")
                } else {
                    color("--text-default")
                })
                .when(selected, |this| this.bg(color("--background-mod-normal")))
                .when(!selected, |this| {
                    this.hover(|style| style.bg(color("--background-mod-subtle")))
                })
                .child(label)
        };
        let search = picker.search.clone();
        let focused = search.read(cx).focus_handle(cx).is_focused(window);
        let header = div()
            .h(px(53.))
            .px(px(16.))
            .pb(px(12.))
            .flex()
            .gap(px(12.))
            .child(
                div()
                    .w(px(if reaction { 260. } else { 300. }))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if focused {
                        color("--input-border-active")
                    } else {
                        color("--input-border-default")
                    })
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
            )
            .when(reaction, |this| {
                // `label_pfW0SY`: the Super Reactions toggle, a 32px circle.
                let burst = self.picker.as_ref().is_some_and(|picker| picker.burst);
                this.child(
                    div()
                        .id("picker-super-reactions")
                        .mt(px(4.))
                        .size(px(32.))
                        .p(px(6.))
                        .rounded_full()
                        .cursor_pointer()
                        .bg(if burst {
                            color("--control-primary-background-default")
                        } else {
                            color("--background-base-lowest")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(picker) = this.picker.as_mut() {
                                picker.burst = !picker.burst;
                            }
                            cx.notify();
                        }))
                        .child(
                            svg()
                                .path("icons/picker_super_reaction.svg")
                                .size(px(20.))
                                .text_color(color("--icon-default")),
                        ),
                )
            })
            .child(
                div().mt(px(1.)).size(px(38.)).p(px(7.)).children(
                    dc_tokens::emoji_by_name("clap")
                        .and_then(|emoji| emoji.sprite)
                        .and_then(|sprite| self.images.get(&format!("dsprite:{sprite}")).cloned())
                        .map(|picture| img(picture).size(px(24.))),
                ),
            )
            .child(
                div()
                    .id("picker-add-emoji")
                    .w(px(102.))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(color("--control-secondary-background-default"))
                    .hover(|style| style.bg(color("--control-secondary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--control-secondary-text-default"))
                    .child("Add Emoji"),
            );
        let sticky = picker
            .rows
            .get(picker.list.logical_scroll_top().item_ix)
            .map(|row| match row {
                Row::Heading(section) | Row::Cells { section, .. } => *section,
            })
            .filter(|_| {
                picker
                    .sections
                    .first()
                    .is_some_and(|section| section.key != "search")
            });
        let body = div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .child(rows)
            .children(sticky.map(|section| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(440.))
                    .child(self.picker_heading(section, true, cx))
            }));
        let panel = div()
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
                    .child(self.picker_rail(cx))
                    .child(
                        div()
                            .w(px(448.))
                            .flex()
                            .flex_col()
                            .child(body)
                            .child(self.picker_inspector()),
                    ),
            );
        Some(
            deferred(
                div()
                    .parity_with_border("emoji-picker", 1.)
                    .id("emoji-picker")
                    .occlude()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(PICKER_WIDTH))
                    .h(px(height))
                    .pt(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.24),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.picker = None;
                        cx.notify();
                    }))
                    .when(!reaction, |this| {
                        this.child(
                            div()
                                .px(px(16.))
                                .flex()
                                .gap(px(8.))
                                .child(tab("picker-tab-gifs", "GIFs", false))
                                .child(tab("picker-tab-stickers", "Stickers", false))
                                .child(tab("picker-tab-emoji", "Emoji", true)),
                        )
                    })
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn picker_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.picker.as_ref() else {
            return div().into_any_element();
        };
        if picker.rows.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-muted"))
                .child("No emoji match your search")
                .into_any_element();
        }
        list(
            picker.list.clone(),
            cx.processor(|this, index, _, cx| this.picker_row(index, cx)),
        )
        .size_full()
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{EMOJIS, search_unicode};

    #[test]
    fn search_ranks_exact_then_prefix_then_contains() {
        let found = search_unicode("joy");
        assert_eq!(EMOJIS[usize::from(found[0])].names[0], "joy");
        assert!(
            found
                .iter()
                .any(|index| EMOJIS[usize::from(*index)].names.contains(&"joy_cat"))
        );
        assert!(search_unicode("zzzz_nothing").is_empty());
    }
}
