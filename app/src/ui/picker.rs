//! The expression picker's emoji tab (`emoji-picker-tab-panel`), measured in
//! `design/dom/pickers/emoji-composer-darker-cozy.json`: tabs, the search field, the skin tone button and Add Emoji
//! over a 48px category rail and a 448px list of 32px section headings and rows of nine 48px cells, with the hovered
//! emoji named in a 48px inspector. Unicode emoji are cells of the sprite sheet (module 617057, `spriteIndex`).

mod nitro_packs;

use super::actions::FRECENCY_KEY;
use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, color, editor_style};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Transitions as _;
use dc_state::Id;
use dc_state::frecency::{Frecency, Kind, Usage};
use dc_tokens::emoji_data::{CATEGORIES, EMOJIS};
use gpui::{
    AnyElement, Bounds, Context, Entity, Focusable, FontWeight, IntoElement, ListAlignment,
    ListOffset, ListState, MouseDownEvent, Pixels, Point, SharedString, Subscription, Window,
    deferred, div, img, list, prelude::*, px, svg,
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
const GIFT_HEIGHT: f32 = 557.;
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
    /// The forward dialog's optional message.
    ForwardNote,
    /// Any other text field with its own emoji button (New Thread, Create Channel, the profile popout).
    Field,
    InventoryReply(Id),
    Reaction {
        channel: Id,
        message: Id,
    },
    /// Server Profile's trait emoji button, by trait index.
    Trait(u8),
    /// "Upload a Sound"'s Related Emoji.
    SoundEmoji,
    /// "Upload a file"'s Related Emoji (stickers).
    StickerEmoji,
    /// Onboarding's answer or task dialog.
    OnboardingEmoji,
    /// Server Web Page's reason to join, by position.
    ReasonEmoji(u8),
    /// "Add emoji confetti" of Gift Nitro (module 354972, `EmojiIntention.GIFT`).
    GiftConfetti,
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
    pub(super) fn message_text(&self, tone: u8) -> String {
        match self {
            Self::Unicode(index) => toned(&EMOJIS[usize::from(*index)], tone)
                .surrogates
                .to_owned(),
            Self::Custom { id, name, animated } => {
                format!("<{}:{name}:{id}>", if *animated { "a" } else { "" })
            }
        }
    }
    fn name(&self) -> &str {
        match self {
            Item::Unicode(index) => EMOJIS[usize::from(*index)].names[0],
            Item::Custom { name, .. } => name,
        }
    }

    /// Image key in `Root::images`, in the chosen skin tone (0 none, 1-5 the five tones).
    pub(super) fn image_key(&self, tone: u8) -> String {
        match self {
            Item::Unicode(index) => {
                let base = &EMOJIS[usize::from(*index)];
                let emoji = toned(base, tone);
                // Tone variants live on the diversity sheet with their base.
                match emoji.sprite {
                    Some(sprite) if base.has_diversity => format!("dsprite:{sprite}"),
                    Some(sprite) => format!("sprite:{sprite}"),
                    None => format!("emoji:{}", emoji.surrogates),
                }
            }
            Item::Custom { id, .. } => format!("custom:{id}"),
        }
    }
}

/// `diversitySurrogate`: an emoji with skin tones shows the chosen one's variant.
pub(super) fn toned(
    emoji: &'static dc_tokens::emoji_data::Emoji,
    tone: u8,
) -> &'static dc_tokens::emoji_data::Emoji {
    if tone == 0 || !emoji.has_diversity {
        return emoji;
    }
    emoji
        .diversity_children
        .get(usize::from(tone) - 1)
        .map_or(emoji, |child| &EMOJIS[usize::from(*child)])
}

/// The skin tone kept between sessions (`emoji.skin_tone` in storage.json).
pub(super) fn stored_tone() -> u8 {
    crate::storage::get("emoji.skin_tone")
        .and_then(|value| value.as_u64())
        .map_or(0, |tone| tone.min(5) as u8)
}

#[derive(Clone, Debug)]
enum SectionIcon {
    Category(&'static str),
    Guild(Id),
    Pack {
        id: String,
        representative_emoji_id: Option<Id>,
    },
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

/// The expression picker's tabs (`navList_58b0l2`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    Gifs,
    Stickers,
    #[default]
    Emoji,
}

pub(crate) struct Picker {
    pub target: Target,
    pub tab: Tab,
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
    /// `2026-09-default-emojis`: showing the `"Nitro Emoji Packs"` catalog inside the picker (`eo`).
    pub(super) show_emoji_packs: bool,
    pub(super) expanded_pack_ids: HashSet<String>,
}

impl Picker {
    /// The search field's focus handle.
    pub(super) fn search_handle(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.search.read(cx).focus_handle(cx)
    }

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

pub(super) fn now_ms() -> u64 {
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
        if let Ok(id) = key.parse::<u64>() {
            if let Some(emoji) = self
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
            if nitro_packs::experiment_enabled(self)
                && let Some(emoji) = self
                    .resolved_emoji_packs()
                    .emoji(key)
                    .filter(|emoji| emoji.available)
            {
                return nitro_packs::pack_item(emoji);
            }
        }
        let index = dc_tokens::emoji_data::NAMES
            .binary_search_by(|(name, _)| (*name).cmp(key))
            .ok()?;
        Some(Item::Unicode(dc_tokens::emoji_data::NAMES[index].1))
    }

    /// `eA.frequently`: emoji picker uses, with this session's on top.
    pub(super) fn frequent_emoji(&self) -> Vec<Item> {
        let frecency = self.emoji_frecency();
        frecency
            .frequently(now_ms(), |key| self.emoji_lookup(key).is_some())
            .iter()
            .filter_map(|key| self.emoji_lookup(key))
            .collect()
    }

    /// The emoji frecency: the account's stored history, or the defaults, with this session's uses on top.
    pub(super) fn emoji_frecency(&self) -> Frecency {
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
    }

    fn picker_sections(&self, query: &str) -> Vec<Section> {
        let target = self
            .picker
            .as_ref()
            .map_or(Target::Composer, |picker| picker.target);
        self.picker_sections_for(target, query)
    }

    fn picker_sections_for(&self, target: Target, query: &str) -> Vec<Section> {
        let packs_enabled =
            nitro_packs::experiment_enabled(self) && nitro_packs::intention_allows_packs(target);
        let pack_store = packs_enabled.then(|| self.resolved_emoji_packs());
        if !query.trim().is_empty() {
            let lowered = query.trim().to_lowercase();
            let mut seen_custom = HashSet::new();
            let mut items: Vec<Item> = self
                .stores
                .guilds
                .iter()
                .flat_map(|guild| &guild.emojis)
                .filter(|emoji| emoji.available && emoji.name.to_lowercase().contains(&lowered))
                .filter(|emoji| seen_custom.insert(emoji.id))
                .map(|emoji| Item::Custom {
                    id: emoji.id,
                    name: emoji.name.clone(),
                    animated: emoji.animated,
                })
                .collect();
            if let Some(store) = pack_store.as_ref() {
                for emoji in store.indexable_pack_emojis(true) {
                    if emoji.name.to_lowercase().contains(&lowered)
                        && let Some(Item::Custom { id, name, animated }) =
                            nitro_packs::pack_item(emoji)
                        && seen_custom.insert(id)
                    {
                        items.push(Item::Custom { id, name, animated });
                    }
                }
            }
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
        if let Some(store) = pack_store.as_ref() {
            sections.extend(nitro_packs::pinned_pack_sections(store));
        }
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
                    Target::Composer
                    | Target::ForwardNote
                    | Target::Field
                    | Target::Trait(_)
                    | Target::SoundEmoji
                    | Target::StickerEmoji
                    | Target::OnboardingEmoji
                    | Target::GiftConfetti
                    | Target::ReasonEmoji(_) => "Find the perfect emoji",
                    Target::Reaction { .. } | Target::InventoryReply(_) => {
                        "Find the perfect reaction"
                    }
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
                    // The highlighted cell, which the arrows and the pointer move (118057); the first result
                    // until one is.
                    let first = this.picker.as_ref().and_then(|picker| {
                        picker.hovered.clone().or_else(|| {
                            picker
                                .sections
                                .first()
                                .and_then(|section| section.items.first())
                                .cloned()
                        })
                    });
                    if let Some(item) = first {
                        this.choose_emoji(item, false, cx);
                    }
                }
                EditorEvent::Cancel => this.picker = None,
            }
            cx.notify();
        });
        if nitro_packs::experiment_enabled(self) && nitro_packs::intention_allows_packs(target) {
            self.ensure_emoji_packs();
        } else {
            self.sync_emoji_packs();
        }
        let sections = self.picker_sections_for(target, "");
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
            tab: Tab::Emoji,
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
            show_emoji_packs: false,
            expanded_pack_ids: HashSet::new(),
        });
        window.focus(&search.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// The composer's GIF and sticker buttons, and the tab bar: open the picker on `tab`, or close it when that tab
    /// is already showing.
    pub(super) fn open_picker_tab(
        &mut self,
        tab: Tab,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(picker) = self.picker.as_ref()
            && picker.target == Target::Composer
        {
            if picker.tab == tab {
                self.picker = None;
                cx.notify();
            } else {
                self.set_picker_tab(tab, window, cx);
            }
            return;
        }
        self.open_picker(Target::Composer, anchor, window, cx);
        self.set_picker_tab(tab, window, cx);
    }

    pub(super) fn set_picker_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = self.picker.as_mut() {
            picker.tab = tab;
        }
        match tab {
            Tab::Gifs => self.ensure_gifs(window, cx),
            Tab::Stickers => self.ensure_stickers(window, cx),
            Tab::Emoji => {
                if let Some(picker) = self.picker.as_ref() {
                    window.focus(&picker.search.read(cx).focus_handle(cx), cx);
                }
            }
        }
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
                // Unsheeted emoji, and tone variants (which have no sheet cell), load as files.
                Item::Unicode(index)
                    if toned(&EMOJIS[usize::from(*index)], self.skin_tone)
                        .sprite
                        .is_none() =>
                {
                    let surrogates = toned(&EMOJIS[usize::from(*index)], self.skin_tone).surrogates;
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
                let surrogates = toned(&EMOJIS[usize::from(*index)], self.skin_tone).surrogates;
                (surrogates.to_owned(), surrogates.to_owned())
            }
            Item::Custom { id, name, animated } => (
                format!("<{}:{name}:{id}>", if *animated { "a" } else { "" }),
                format!("{name}:{id}"),
            ),
        };
        // Emoji of other servers and animated emoji need Nitro in a message: the upsell opens instead.
        if matches!(
            target,
            Target::Composer | Target::ForwardNote | Target::Field | Target::InventoryReply(_)
        ) && let Item::Custom { id, animated, .. } = &item
            && self.picker_item_locked(*id, *animated)
        {
            self.picker = None;
            self.open_nitro(cx);
            return;
        }
        let usage_key = match &item {
            Item::Unicode(_) => item.name().to_owned(),
            Item::Custom { id, .. } => id.to_string(),
        };
        match target {
            Target::InventoryReply(entry) => {
                self.send_inventory_reply(entry, text.clone(), false, cx)
            }
            Target::Composer => {
                self.emoji_used.push((usage_key, now_ms()));
                self.composer
                    .update(cx, |editor, cx| editor.insert(&text, cx));
            }
            Target::ForwardNote => {
                self.emoji_used.push((usage_key, now_ms()));
                if let Some(note) = self.forward.as_ref().map(super::forward::Forward::note) {
                    note.update(cx, |editor, cx| editor.insert(&text, cx));
                }
            }
            Target::Field => {
                self.emoji_used.push((usage_key, now_ms()));
                if let Some(editor) = self.picker_field.clone() {
                    editor.update(cx, |editor, cx| editor.insert(&text, cx));
                }
            }
            Target::SoundEmoji => {
                self.emoji_used.push((usage_key, now_ms()));
                match &item {
                    // A unicode emoji goes by its characters here (module 122998's `emojiName`).
                    Item::Unicode(_) => self.set_sound_emoji(None, reaction.clone(), cx),
                    Item::Custom { id, name, .. } => {
                        self.set_sound_emoji(Some(id.to_string()), name.clone(), cx)
                    }
                }
            }
            Target::OnboardingEmoji => {
                self.emoji_used.push((usage_key, now_ms()));
                match &item {
                    Item::Unicode(_) => {
                        self.set_onboarding_emoji(None, Some(reaction.clone()), false, cx)
                    }
                    Item::Custom { id, name, animated } => self.set_onboarding_emoji(
                        Some(id.to_string()),
                        Some(name.clone()),
                        *animated,
                        cx,
                    ),
                }
            }
            Target::ReasonEmoji(index) => {
                self.emoji_used.push((usage_key, now_ms()));
                // The listing keeps a reason's emoji by its characters; a custom emoji cannot be one.
                if let Item::Unicode(_) = &item {
                    self.landing_set_reason_emoji(usize::from(index), Some(reaction.clone()), cx);
                }
            }
            Target::StickerEmoji => {
                self.emoji_used.push((usage_key, now_ms()));
                match &item {
                    // Module 759886 keeps a unicode emoji by its characters, a custom one by id and name.
                    Item::Unicode(_) => self.set_sticker_emoji(None, Some(reaction.clone()), cx),
                    Item::Custom { id, name, .. } => {
                        self.set_sticker_emoji(Some(id.to_string()), Some(name.clone()), cx)
                    }
                }
            }
            Target::Trait(index) => {
                self.emoji_used.push((usage_key, now_ms()));
                let emoji = match &item {
                    Item::Unicode(index) => super::server_settings::TraitEmoji {
                        id: None,
                        name: Some(EMOJIS[usize::from(*index)].names[0].to_owned()),
                        animated: false,
                    },
                    Item::Custom { id, name, animated } => super::server_settings::TraitEmoji {
                        id: Some(id.to_string()),
                        name: Some(name.clone()),
                        animated: *animated,
                    },
                };
                self.set_trait_emoji(usize::from(index), emoji, cx);
            }
            Target::GiftConfetti => {
                // Module 354972 keeps the emoji (its characters or its custom id) and shows the name with its
                // underscores as spaces.
                self.emoji_used.push((usage_key, now_ms()));
                let (glyph, name, id) = match &item {
                    Item::Unicode(index) => (
                        Some(reaction.clone()),
                        EMOJIS[usize::from(*index)].names[0].to_owned(),
                        None,
                    ),
                    Item::Custom { id, name, .. } => (None, name.clone(), Some(*id)),
                };
                self.set_gift_confetti(glyph, name, id);
            }
            Target::Reaction { channel, message } => {
                self.reactions_used.push((usage_key, now_ms()));
                self.send_and_apply_reaction(
                    channel,
                    message,
                    super::menu::ReactionOp {
                        emoji: reaction,
                        add: true,
                        location: "Message Reaction Picker",
                        burst,
                    },
                    cx,
                );
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
        let sticky_tag = if sticky { "sticky" } else { "row" };
        let pack_remove = match &entry.icon {
            SectionIcon::Pack { id, .. } => {
                let pack_id = id.clone();
                Some(
                    div()
                        .id(SharedString::from(format!(
                            "picker-pack-remove-{section}-{sticky_tag}"
                        )))
                        .px(px(8.))
                        .h(px(22.))
                        .rounded(px(6.))
                        .bg(color("--control-secondary-background-default"))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .hover(|style| style.bg(color("--control-secondary-background-hover")))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .cursor_pointer()
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.toggle_pin_emoji_pack(&pack_id, false, window, cx);
                        }))
                        .child(dc_intl::t("2zxYCx")),
                )
            }
            _ => None,
        };
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
            SectionIcon::Pack { .. } => svg()
                .path("icons/nitro_wheel.svg")
                .size(px(16.))
                .flex_none()
                .text_color(color("--text-default"))
                .into_any_element(),
        };
        div()
            .id(SharedString::from(format!(
                "picker-heading-{section}-{sticky_tag}"
            )))
            .h(px(HEADING))
            .w_full()
            .pl(px(8.))
            .pr(px(4.))
            .bg(color("--background-surface-high"))
            .flex()
            .items_center()
            .justify_between()
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
            .children(pack_remove)
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
                        let picture = self.images.get(&item.image_key(self.skin_tone)).cloned();
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
                            .children(picture.map(|picture| {
                                let art = img(picture).size(px(40.));
                                // An animated emoji plays under the pointer only (module 565645: hover or
                                // interaction); the renderer advances the frames of an identified image.
                                if selected {
                                    art.id(SharedString::from(format!(
                                        "picker-art-{index}-{offset}"
                                    )))
                                    .into_any_element()
                                } else {
                                    art.into_any_element()
                                }
                            }))
                    }))
                    .into_any_element()
            }
            None => div().into_any_element(),
        }
    }

    /// An arrow, Home or End over the emoji grid (module 118057): the highlighted cell moves and the list follows it.
    pub(super) fn emoji_grid_key(&mut self, key: &str, ctrl: bool, cx: &mut Context<Self>) -> bool {
        let Some(picker) = self.picker.as_mut() else {
            return false;
        };
        // The rows of cells, as (row of the list, section, first item, cells).
        let cells: Vec<(usize, usize, usize, usize)> = picker
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| match row {
                Row::Cells { section, start, .. } => Some((
                    index,
                    *section,
                    *start,
                    (picker.sections[*section].items.len() - start).min(COLUMNS),
                )),
                Row::Heading(_) => None,
            })
            .collect();
        let lengths: Vec<usize> = cells.iter().map(|cell| cell.3).collect();
        let at = picker.hovered.as_ref().and_then(|hovered| {
            cells
                .iter()
                .enumerate()
                .find_map(|(row, (_, section, start, len))| {
                    picker.sections[*section].items[*start..start + len]
                        .iter()
                        .position(|item| item == hovered)
                        .map(|col| (row, col))
                })
        });
        let Some(to) = (match at {
            Some(at) => super::keyboard::grid_step(&lengths, at, key, ctrl),
            None => super::keyboard::grid_first(&lengths),
        }) else {
            // Left at the first cell stays with the caret; the other keys at an end do nothing.
            return !(at == Some((0, 0)) && key == "left") && at.is_some();
        };
        let (row, section, start, _) = cells[to.0];
        picker.hovered = Some(picker.sections[section].items[start + to.1].clone());
        super::keyboard::reveal_row(&picker.list, row);
        cx.notify();
        true
    }

    fn picker_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.picker.as_ref() else {
            return div().into_any_element();
        };
        let packs_active = self.nitro_packs_active();
        let light = super::theme() == dc_tokens::Theme::Light;
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
        let mut children: Vec<AnyElement> = Vec::new();
        if packs_active {
            children.push(self.nitro_packs_rail_button(cx));
        }
        for (index, section) in picker.all.iter().enumerate() {
            let selected = index == current;
            let is_pack = matches!(section.icon, SectionIcon::Pack { .. });
            let prev_is_pack =
                index > 0 && matches!(picker.all[index - 1].icon, SectionIcon::Pack { .. });
            let next_is_pack = picker
                .all
                .get(index + 1)
                .is_some_and(|next| matches!(next.icon, SectionIcon::Pack { .. }));
            if is_pack && !prev_is_pack && index > 0 {
                children.push(
                    div()
                        .w_full()
                        .h(px(1.))
                        .my(px(8.))
                        .flex_none()
                        .bg(color("--border-subtle"))
                        .into_any_element(),
                );
            }
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
                SectionIcon::Pack {
                    representative_emoji_id,
                    ..
                } => {
                    let picture = representative_emoji_id
                        .and_then(|id| self.images.get(&format!("custom:{id}")).cloned());
                    div()
                        .size(px(24.))
                        .rounded(px(4.))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(picture.map(|picture| img(picture).size(px(24.))))
                        .when(representative_emoji_id.is_none(), |this| {
                            this.child(
                                svg()
                                    .path("icons/nitro_wheel.svg")
                                    .size(px(18.))
                                    .text_color(color("--icon-default")),
                            )
                        })
                        .into_any_element()
                }
            };
            let mb = if index == 0 && !next_is_pack { 8. } else { 2. };
            let item = div()
                .id(SharedString::from(format!("picker-rail-item-{index}")))
                .group(group)
                .relative()
                .flex_none()
                .size(px(32.))
                .p(px(4.))
                .mb(px(mb))
                .rounded(px(if is_pack { 8. } else { 4. }))
                .cursor_pointer()
                .when(selected && !is_pack, |this| {
                    this.bg(color("--background-base-low"))
                })
                .when(selected && is_pack, |this| {
                    this.child(
                        img(nitro_packs::button_gradient_image(light))
                            .id(SharedString::from(format!("picker-rail-pack-bg-{index}")))
                            .absolute()
                            .inset_0()
                            .rounded(px(8.))
                            .size(px(32.)),
                    )
                })
                .when(!selected, |this| {
                    this.hover(|style| style.bg(color("--interactive-background-hover")))
                })
                .when(matches!(section.icon, SectionIcon::Category(_)), |this| {
                    this.rule(crate::ui::motion::Rule::PickerCategory)
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
                .child(inner);
            if is_pack {
                children.push(
                    self.with_tip(
                        SharedString::from(format!("picker-rail-pack-{index}")),
                        super::tooltip::Tip::body(
                            super::tooltip::Body::Text(section.label.clone().into()),
                            super::tooltip::Side::Right,
                        ),
                        item,
                        cx,
                    )
                    .into_any_element(),
                );
            } else {
                children.push(item.into_any_element());
            }
            if is_pack && !next_is_pack && index + 1 < picker.all.len() {
                children.push(
                    div()
                        .w_full()
                        .h(px(1.))
                        .my(px(8.))
                        .flex_none()
                        .bg(color("--border-subtle"))
                        .into_any_element(),
                );
            }
        }
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
            .children(children)
            .into_any_element()
    }

    fn picker_inspector(&self) -> AnyElement {
        let hovered = self
            .picker
            .as_ref()
            .and_then(|picker| picker.hovered.clone());
        let pack_store = self
            .nitro_packs_active()
            .then(|| self.resolved_emoji_packs());
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
                            .or_else(|| self.images.get(&item.image_key(self.skin_tone)))
                            .cloned()
                    }
                    Item::Custom { .. } => {
                        self.images.get(&item.image_key(self.skin_tone)).cloned()
                    }
                };
                let pack = match (&item, pack_store.as_ref()) {
                    (Item::Custom { id, .. }, Some(store)) => {
                        store.pack_for_emoji(&id.to_string()).cloned()
                    }
                    _ => None,
                };
                this.child(div().size(px(28.)).children(
                    picture.map(|picture| img(picture).id("picker-inspector-art").size(px(28.))),
                ))
                .child(if let Some(pack) = pack {
                    let (prefix, suffix) = nitro_packs::inspector_pack_parts(&pack.name);
                    div()
                        .ml(px(8.))
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(15.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .truncate()
                                .child(format!(":{}:", item.name())),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(400.))
                                .text_color(color("--text-subtle"))
                                .when(!prefix.is_empty(), |this| this.child(prefix))
                                .child(
                                    svg()
                                        .path("icons/nitro_wheel.svg")
                                        .size(px(10.))
                                        .flex_none()
                                        .text_color(color("--text-subtle")),
                                )
                                .child(suffix),
                        )
                        .into_any_element()
                } else {
                    div()
                        .ml(px(8.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(format!(":{}:", item.name()))
                        .into_any_element()
                })
            })
            .into_any_element()
    }

    pub(super) fn picker_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let picker = self.picker.as_ref()?;
        if self.nitro_packs_active() && self.nitro_packs_needs_sync() {
            cx.spawn(async move |this, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.sync_emoji_packs();
                    let query = this
                        .picker
                        .as_ref()
                        .map(|picker| picker.search.read(cx).text().to_owned())
                        .unwrap_or_default();
                    let all = this.picker_sections("");
                    if let Some(picker) = this.picker.as_mut() {
                        picker.all = all;
                    }
                    this.refresh_picker(&query);
                    cx.notify();
                });
            })
            .detach();
        }
        let viewing_packs = picker.show_emoji_packs && self.nitro_packs_active();
        let viewport = window.viewport_size();
        let inventory_reply = matches!(picker.target, Target::InventoryReply(_));
        // Composer: the right edge 32px in from the window's, the bottom 8px above the composer.
        // Reactions: beside the hover bar, kept inside the window.
        let reaction = matches!(
            picker.target,
            Target::Reaction { .. } | Target::InventoryReply(_)
        );
        let trait_target = matches!(
            picker.target,
            Target::Trait(_)
                | Target::SoundEmoji
                | Target::StickerEmoji
                | Target::OnboardingEmoji
                | Target::GiftConfetti
                | Target::ReasonEmoji(_)
        );
        // Gift Nitro's confetti picker: a title block over the search field, 117 down to the list, 392 of list and
        // the 48px inspector (design/dom/dm/gift-step2-confetti-picker-darker-cozy.json: 408 to 908, 0 to 557).
        let gift_confetti = matches!(picker.target, Target::GiftConfetti);
        // Neither the reaction picker nor a trait's has the tab bar (`design/dom/popouts/trait-emoji-darker-cozy.json`).
        let tabless = reaction || trait_target;
        let height = if gift_confetti {
            GIFT_HEIGHT
        } else if tabless {
            REACTION_HEIGHT
        } else {
            PICKER_HEIGHT
        };
        // Composer: the right edge 32px in from the window's, the bottom at the anchor (8px above the composer).
        // Reactions: 8px left of the button, top-aligned, then nudged up to stay 12px inside the window.
        let (left, top) = if inventory_reply {
            // 289287.dF: top/right of the 18px button, inverted and shifted into the viewport.
            let above = f32::from(picker.anchor.y) - height - 8.;
            let top = if above >= 0. {
                above
            } else {
                f32::from(picker.anchor.y) + 18. + 8.
            };
            (
                f32::from(picker.anchor.x) - PICKER_WIDTH,
                top.clamp(0., (f32::from(viewport.height) - height).max(0.)),
            )
        } else if trait_target {
            // Trait emoji: the left edge at the button's, above it, held inside the window (0 in the capture).
            let top = (f32::from(picker.anchor.y) - 8. - height)
                .clamp(0., (f32::from(viewport.height) - height).max(0.));
            (f32::from(picker.anchor.x), top)
        } else if reaction {
            (
                f32::from(picker.anchor.x) - PICKER_WIDTH - 8.,
                f32::from(picker.anchor.y).min(f32::from(viewport.height) - EDGE - height),
            )
        } else {
            // 24px in from the composer's right edge, which a docked member list pulls in from the window's.
            let right = self
                .tip_anchors
                .borrow()
                .get("composer-box")
                .map_or(f32::from(viewport.width) - 8., |bounds| {
                    f32::from(bounds.right()) + 1.
                });
            (
                right - 24. - PICKER_WIDTH,
                f32::from(picker.anchor.y) - height,
            )
        };
        let left = left.clamp(
            EDGE,
            (f32::from(viewport.width) - PICKER_WIDTH - EDGE).max(EDGE),
        );
        // The gift picker's box (border included) starts one pixel above the window in the capture: its content
        // is at 0 and the clamp is on the content.
        let top = if gift_confetti {
            top - 1.
        } else if trait_target {
            top
        } else {
            top.max(EDGE)
        };
        let rows = self.picker_rows(cx);
        let current = self
            .picker
            .as_ref()
            .map(|picker| picker.tab)
            .unwrap_or_default();
        let tab = |id: &'static str, label: &'static str, which: Tab| {
            let selected = current == which;
            div()
                .id(id)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_picker_tab(which, window, cx)),
                )
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
        let tabs = [
            tab("picker-tab-gifs", "GIFs", Tab::Gifs),
            tab("picker-tab-stickers", "Stickers", Tab::Stickers),
            tab("picker-tab-emoji", "Emoji", Tab::Emoji),
        ];
        let search = picker.search.clone();
        let focused = search.read(cx).focus_handle(cx).is_focused(window);
        let header = div()
            .h(px(53.))
            .when(inventory_reply, |d| {
                d.ml(px(-1.))
                    .w(px(500.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
            })
            .px(px(16.))
            .pb(px(12.))
            .flex()
            .gap(px(12.))
            .child(
                div()
                    .w(px(if inventory_reply {
                        374.
                    } else if reaction {
                        260.
                    } else {
                        300.
                    }))
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
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pr(px(12.))
                            .child(search.clone()),
                    ),
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
            .child({
                // `diversitySelector_eHzdRB`: the clap in the current tone; open, the six tones drop below it.
                let clap = dc_tokens::emoji_by_name("clap");
                // The bare clap is a diversity sheet cell; its tones load as files (`emoji:` keys).
                let tone_image = |tone: u8| {
                    let emoji = clap.map(|emoji| toned(emoji, tone))?;
                    match emoji.sprite {
                        Some(sprite) => self.images.get(&format!("dsprite:{sprite}")).cloned(),
                        None => self
                            .images
                            .get(&format!("emoji:{}", emoji.surrogates))
                            .cloned(),
                    }
                };
                let open = self.skin_menu;
                div()
                    .id("picker-skin-tone")
                    .relative()
                    .mt(px(1.))
                    .size(px(38.))
                    .p(px(7.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        // A button: the click takes focus from the search field.
                        window.blur(cx);
                        this.skin_menu = !this.skin_menu;
                        if let Some(clap) = dc_tokens::emoji_by_name("clap") {
                            for tone in 1..=5u8 {
                                let surrogates = toned(clap, tone).surrogates;
                                if let Some(file) = dc_tokens::emoji_file(surrogates) {
                                    this.request_image(
                                        format!("emoji:{surrogates}"),
                                        format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN),
                                    );
                                }
                            }
                        }
                        cx.notify();
                    }))
                    .children(tone_image(self.skin_tone).map(|picture| img(picture).size(px(24.))))
                    .when(open, |this| {
                        this.child(
                            deferred(
                                div()
                                    .id("picker-skin-tones")
                                    .occlude()
                                    .absolute()
                                    .left(px(-1.))
                                    .top(px(-1.))
                                    .w(px(40.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-surface-higher"))
                                    .children((0..6u8).map(|tone| {
                                        div()
                                            .id(SharedString::from(format!("skin-tone-{tone}")))
                                            .size(px(38.))
                                            .p(px(7.))
                                            .cursor_pointer()
                                            .hover(|style| {
                                                style.bg(color("--interactive-background-hover"))
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.skin_tone = tone;
                                                this.skin_menu = false;
                                                crate::storage::set(
                                                    "emoji.skin_tone",
                                                    serde_json::json!(tone),
                                                );
                                                // The visible emoji switch to their tone variants.
                                                if let Some(sections) = this
                                                    .picker
                                                    .as_ref()
                                                    .map(|picker| picker.all.clone())
                                                {
                                                    this.request_picker_images(&sections);
                                                }
                                                cx.notify();
                                            }))
                                            .children(
                                                tone_image(tone)
                                                    .map(|picture| img(picture).size(px(24.))),
                                            )
                                    })),
                            )
                            .with_priority(1),
                        )
                    })
            })
            .when(!inventory_reply, |header| {
                header.child(
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
                        // Emoji are uploaded on the server's Emoji page.
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(guild) = this.guild {
                                this.picker = None;
                                this.open_server_settings_at(
                                    guild,
                                    super::server_settings::Page::Emoji,
                                    cx,
                                );
                            }
                        }))
                        .child("Add Emoji"),
                )
            });
        // `customGiftContent_gYY06b`: the title (body-md/semibold) and its line (body-sm/normal, 38px together), 10px
        // above the 468px search field, which has no tone selector or Add Emoji button beside it.
        let header = if gift_confetti {
            div()
                .flex_none()
                .px(px(16.))
                .pb(px(12.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .text_color(color("--text-default"))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .child("Add emoji confetti"),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .child("This will play when your friend opens their gift"),
                        ),
                )
                .child(
                    div()
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
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .pr(px(12.))
                                .child(search.clone()),
                        ),
                )
        } else {
            header
        };
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
        let panel = if viewing_packs {
            self.nitro_packs_panel(tabless, cx)
        } else {
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
                        .child(self.picker_rail(cx))
                        .child(
                            div()
                                .w(px(if inventory_reply { 450. } else { 448. }))
                                .flex()
                                .flex_col()
                                .child(body)
                                .child(self.picker_inspector()),
                        ),
                )
                .into_any_element()
        };
        Some(
            deferred(
                div()
                    .parity_with_border("emoji-picker", if inventory_reply { 0. } else { 1. })
                    .id("emoji-picker")
                    // The reaction picker is a popout with `animation: FADE` on the left (`animatorLeft fade` in
                    // design/dom/pickers/emoji-reaction); the composer's picker has no animator in its capture
                    // (pickers/emoji-composer), so it appears at once.
                    .when(reaction && !inventory_reply, |this| {
                        this.popout("emoji-picker", PopKind::Fade, PopSide::Left)
                    })
                    .occlude()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(PICKER_WIDTH))
                    .h(px(height))
                    .pt(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .when(inventory_reply, |d| d.border_0())
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
                    .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        // A press on the composer's own buttons is their toggle (the popout ignores its reference
                        // element), not a dismissal followed by a click that opens it again.
                        let on_tools =
                            event.button == gpui::MouseButton::Left
                                && matches!(
                                    this.picker.as_ref().map(|picker| &picker.target),
                                    Some(Target::Composer)
                                )
                                && this.tip_anchors.borrow().get("composer-box").is_some_and(
                                    |bounds| on_composer_tools(*bounds, event.position),
                                );
                        // The closing press goes no further for non-right presses; right-clicks retarget in one click.
                        if !on_tools
                            && this.press_outside(super::keyboard::Layer::Picker, cx)
                            && event.button != gpui::MouseButton::Right
                        {
                            cx.stop_propagation();
                        }
                    }))
                    .when(!tabless, |this| {
                        this.child(div().px(px(16.)).flex().gap(px(8.)).children(tabs))
                    })
                    .child(match current {
                        Tab::Gifs => self.gif_panel(window, cx),
                        Tab::Stickers => self.sticker_panel(window, cx),
                        Tab::Emoji => panel,
                    }),
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

/// The width the composer's button strip (gift, GIF, sticker, emoji, apps: five 32px buttons) takes at its right end.
const COMPOSER_TOOLS_WIDTH: f32 = 5. * 32.;

/// Whether a press lands on the composer's button strip.
fn on_composer_tools(composer: Bounds<Pixels>, position: Point<Pixels>) -> bool {
    composer.contains(&position) && position.x >= composer.right() - px(COMPOSER_TOOLS_WIDTH + 8.)
}

#[cfg(test)]
mod toggle_tests {
    use super::*;
    use gpui::point;

    #[test]
    fn only_the_button_strip_of_the_composer_is_a_toggle_press() {
        let composer = Bounds::new(point(px(384.), px(655.)), gpui::size(px(888.), px(56.)));
        assert!(on_composer_tools(composer, point(px(1202.), px(683.))));
        assert!(!on_composer_tools(composer, point(px(700.), px(683.))));
        assert!(!on_composer_tools(composer, point(px(1202.), px(300.))));
    }
}
