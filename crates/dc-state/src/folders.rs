//! Server folders and the order of the server rail: `SortedGuildStore` (module 711014), its `GuildsTree` (263715) and
//! `ExpandedGuildFolderStore` (383394). The tree has two levels: root items are guilds or folders, a folder holds
//! guilds only. The order and the folders live in the account's proto settings (`guild_folders`, field 14); every
//! change here ends as the entries `to_entries` returns, which `dc_proto::settings::set_guild_folders` writes.

use std::collections::BTreeSet;

use crate::Id;
use dc_proto::settings::GuildFolderEntry;

/// A folder: `folderId` (a random 32 bit number the client picks), its optional name and colour, its guilds in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folder {
    pub id: u64,
    pub name: Option<String>,
    pub color: Option<u64>,
    pub guilds: Vec<Id>,
}

/// One root item of the rail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Guild(Id),
    Folder(Folder),
}

impl Item {
    /// The node id: the guild id, or the folder id.
    pub fn id(&self) -> u64 {
        match self {
            Item::Guild(id) => *id,
            Item::Folder(folder) => folder.id,
        }
    }
}

/// Where "Move to..." sends a server (`move-to-first` / `move-to-last`, module 865815).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    First,
    Last,
}

#[derive(Clone, Debug)]
pub struct GuildFolders {
    roots: Vec<Item>,
    /// `ExpandedGuildFolderStore.expandedFolders`.
    expanded: BTreeSet<u64>,
    /// State of the generator that picks folder ids (`Math.random() * 2^32` in the official tree).
    seed: u64,
}

impl Default for GuildFolders {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9, |since| since.as_nanos() as u64)
            | 1;
        Self {
            roots: Vec::new(),
            expanded: BTreeSet::new(),
            seed,
        }
    }
}

/// Where a node sits: root index, and the index inside the folder for a guild that is in one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    root: usize,
    child: Option<usize>,
}

impl GuildFolders {
    /// `N` (module 711014): the tree from the settings (`folders`, else the legacy `positions` as plain guilds), then
    /// guilds this client does not know are dropped, empty folders removed, and known guilds the settings do not list
    /// go on top in the order of `unlisted` (the caller sorts them newest first).
    pub fn rebuild(
        &mut self,
        entries: &[GuildFolderEntry],
        positions: &[Id],
        known: &[Id],
        unlisted: &[Id],
    ) {
        let mut seen = BTreeSet::new();
        let mut roots = Vec::new();
        let take = |id: Id, seen: &mut BTreeSet<Id>| known.contains(&id) && seen.insert(id);
        if entries.is_empty() {
            for id in positions {
                if take(*id, &mut seen) {
                    roots.push(Item::Guild(*id));
                }
            }
        } else {
            for entry in entries {
                match (entry.id, entry.guild_ids.as_slice()) {
                    (_, []) => {}
                    (None, [first, ..]) => {
                        if take(*first, &mut seen) {
                            roots.push(Item::Guild(*first));
                        }
                    }
                    (Some(id), ids) => {
                        let guilds: Vec<Id> = ids
                            .iter()
                            .copied()
                            .filter(|guild| take(*guild, &mut seen))
                            .collect();
                        if !guilds.is_empty() {
                            roots.push(Item::Folder(Folder {
                                id,
                                name: entry.name.clone().filter(|name| !name.is_empty()),
                                color: entry.color,
                                guilds,
                            }));
                        }
                    }
                }
            }
        }
        let mut front: Vec<Item> = unlisted
            .iter()
            .copied()
            .filter(|id| known.contains(id) && !seen.contains(id))
            .map(Item::Guild)
            .collect();
        front.dedup_by_key(|item| item.id());
        front.append(&mut roots);
        self.roots = front;
        self.prune_expanded();
    }

    pub fn roots(&self) -> &[Item] {
        &self.roots
    }

    /// `getFlattenedGuildIds`: every guild in rail order, folders opened out.
    pub fn flattened(&self) -> Vec<Id> {
        self.roots
            .iter()
            .flat_map(|item| match item {
                Item::Guild(id) => vec![*id],
                Item::Folder(folder) => folder.guilds.clone(),
            })
            .collect()
    }

    /// `getCompatibleGuildFolders`: the entries the settings store, a plain guild as an entry without an id.
    pub fn to_entries(&self) -> Vec<GuildFolderEntry> {
        self.roots
            .iter()
            .filter(|item| !matches!(item, Item::Folder(folder) if folder.guilds.is_empty()))
            .map(|item| match item {
                Item::Guild(id) => GuildFolderEntry {
                    id: None,
                    name: None,
                    color: None,
                    guild_ids: vec![*id],
                },
                Item::Folder(folder) => GuildFolderEntry {
                    id: Some(folder.id),
                    name: folder.name.clone(),
                    color: folder.color,
                    guild_ids: folder.guilds.clone(),
                },
            })
            .collect()
    }

    pub fn folder(&self, id: u64) -> Option<&Folder> {
        self.roots.iter().find_map(|item| match item {
            Item::Folder(folder) if folder.id == id => Some(folder),
            _ => None,
        })
    }

    /// The folder holding `guild`, if any.
    pub fn folder_of(&self, guild: Id) -> Option<&Folder> {
        self.roots.iter().find_map(|item| match item {
            Item::Folder(folder) if folder.guilds.contains(&guild) => Some(folder),
            _ => None,
        })
    }

    fn place(&self, id: u64) -> Option<Place> {
        for (root, item) in self.roots.iter().enumerate() {
            match item {
                Item::Guild(guild) if *guild == id => return Some(Place { root, child: None }),
                Item::Folder(folder) => {
                    if folder.id == id {
                        return Some(Place { root, child: None });
                    }
                    if let Some(child) = folder.guilds.iter().position(|guild| *guild == id) {
                        return Some(Place {
                            root,
                            child: Some(child),
                        });
                    }
                }
                Item::Guild(_) => {}
            }
        }
        None
    }

    /// Takes a node out of the tree, as `_pluckNode`. A folder left empty stays until `sweep`.
    fn pluck(&mut self, id: u64) -> Option<Item> {
        let place = self.place(id)?;
        let item = match place.child {
            None => self.roots.remove(place.root),
            Some(child) => {
                let Item::Folder(folder) = &mut self.roots[place.root] else {
                    return None;
                };
                Item::Guild(folder.guilds.remove(child))
            }
        };
        Some(item)
    }

    /// `GUILD_MOVE_BY_ID` (`R`): `source` goes beside `target` (after it when `below`), or, with `combine`, into the
    /// folder `target` is (one is made around a lone guild). The official store throws on a folder dropped into a
    /// folder, a folder combined, or a combine onto a guild that is already in a folder: those return false.
    pub fn move_by_id(&mut self, source: u64, target: u64, below: bool, combine: bool) -> bool {
        let moved = self.move_by_id_raw(source, target, below, combine);
        self.sweep();
        moved
    }

    fn move_by_id_raw(&mut self, source: u64, target: u64, below: bool, combine: bool) -> bool {
        if source == target {
            return false;
        }
        let (Some(from), Some(to)) = (self.place(source), self.place(target)) else {
            return false;
        };
        let source_is_folder =
            from.child.is_none() && matches!(self.roots[from.root], Item::Folder(_));
        if combine {
            if source_is_folder || to.child.is_some() {
                return false;
            }
            let folder_id = match &self.roots[to.root] {
                Item::Folder(folder) => folder.id,
                Item::Guild(guild) => {
                    let guild = *guild;
                    let id = self.fresh_folder_id();
                    self.roots[to.root] = Item::Folder(Folder {
                        id,
                        guilds: vec![guild],
                        ..Folder::default()
                    });
                    id
                }
            };
            return self.move_into(source, folder_id, below);
        }
        if source_is_folder && to.child.is_some() {
            return false;
        }
        self.move_next_to(source, target, below)
    }

    /// What `N` does after every change: empty folders go, and so do expanded ids of folders that are gone.
    fn sweep(&mut self) {
        self.roots
            .retain(|item| !matches!(item, Item::Folder(folder) if folder.guilds.is_empty()));
        self.prune_expanded();
    }

    /// `moveNextTo`: `id` is taken out and put beside `target`, in the same parent.
    fn move_next_to(&mut self, id: u64, target: u64, below: bool) -> bool {
        let Some(item) = self.pluck(id) else {
            return false;
        };
        let Some(place) = self.place(target) else {
            // The target vanished with the emptied folder it was: keep the node at the end rather than lose it.
            self.roots.push(item);
            return false;
        };
        let offset = usize::from(below);
        match (place.child, item) {
            (None, item) => self.roots.insert(place.root + offset, item),
            (Some(child), Item::Guild(guild)) => {
                if let Item::Folder(folder) = &mut self.roots[place.root] {
                    folder.guilds.insert(child + offset, guild);
                }
            }
            (Some(_), item) => self.roots.push(item),
        }
        true
    }

    /// `moveInto`: a guild goes to the end (`append`) or the start of the folder.
    fn move_into(&mut self, guild: Id, folder_id: u64, append: bool) -> bool {
        let Some(Item::Guild(guild)) = self.pluck(guild) else {
            return false;
        };
        match self.roots.iter_mut().find_map(|item| match item {
            Item::Folder(folder) if folder.id == folder_id => Some(folder),
            _ => None,
        }) {
            Some(folder) => {
                let at = if append { folder.guilds.len() } else { 0 };
                folder.guilds.insert(at, guild);
                true
            }
            None => {
                self.roots.insert(0, Item::Guild(guild));
                false
            }
        }
    }

    fn fresh_folder_id(&mut self) -> u64 {
        loop {
            // xorshift64*: the official picks `Math.floor(2^32 * Math.random())` until it is unused.
            self.seed ^= self.seed >> 12;
            self.seed ^= self.seed << 25;
            self.seed ^= self.seed >> 27;
            let id = self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32;
            if id != 0 && self.place(id).is_none() {
                return id;
            }
        }
    }

    /// "Move to..." (`moveFirst` / `moveLast`, module 865815): beside the first or last root item, before or after it.
    /// False when there is only one root item (the official menu is not shown then) or the guild is not known.
    pub fn move_guild(&mut self, id: u64, placement: Placement) -> bool {
        if self.roots.len() < 2 {
            return false;
        }
        let (target, below) = match placement {
            Placement::First => (self.roots[0].id(), false),
            Placement::Last => (self.roots[self.roots.len() - 1].id(), true),
        };
        self.move_by_id(id, target, below, false)
    }

    /// Whether "Move to... First in List" / "Last in List" is disabled (`isFirst`, `isLast`): the node already is the
    /// first or last root item. `None` when the menu has no such submenu (fewer than two root items, unknown node).
    pub fn placement_disabled(&self, id: u64) -> Option<(bool, bool)> {
        if self.roots.len() < 2 || self.place(id).is_none() {
            return None;
        }
        Some((
            self.roots.first().is_some_and(|item| item.id() == id),
            self.roots.last().is_some_and(|item| item.id() == id),
        ))
    }

    /// `GUILD_FOLDER_CREATE_LOCAL` (`L`): a folder around the first guild, the rest appended in order. Returns the id.
    pub fn create_folder(&mut self, sources: &[Id], name: Option<&str>) -> Option<u64> {
        self.create_folder_as(None, sources, name)
    }

    /// `create_folder` with a chosen id (a capture script names its folders); `None` picks a fresh one.
    pub fn create_folder_as(
        &mut self,
        wanted: Option<u64>,
        sources: &[Id],
        name: Option<&str>,
    ) -> Option<u64> {
        let (first, rest) = sources.split_first()?;
        let place = self.place(*first)?;
        if place.child.is_some() || matches!(self.roots[place.root], Item::Folder(_)) {
            return None;
        }
        if wanted.is_some_and(|id| self.place(id).is_some()) {
            return None;
        }
        let id = wanted.unwrap_or_else(|| self.fresh_folder_id());
        self.roots[place.root] = Item::Folder(Folder {
            id,
            name: name.filter(|name| !name.is_empty()).map(str::to_owned),
            guilds: vec![*first],
            ..Folder::default()
        });
        for guild in rest {
            self.move_into(*guild, id, true);
        }
        self.sweep();
        Some(id)
    }

    /// `GUILD_FOLDER_EDIT_LOCAL` (`y`): the folder's name and exactly the guilds in `sources`: others leave the folder
    /// to sit after it, new ones are appended.
    pub fn edit_folder(&mut self, target: u64, sources: &[Id], name: &str) -> bool {
        let Some(folder) = self.folder(target) else {
            return false;
        };
        let current = folder.guilds.clone();
        let name = (!name.is_empty()).then(|| name.to_owned());
        if let Some(Item::Folder(folder)) = self
            .roots
            .iter_mut()
            .find(|item| matches!(item, Item::Folder(folder) if folder.id == target))
        {
            folder.name = name;
        }
        for guild in sources.iter().filter(|guild| !current.contains(guild)) {
            self.move_into(*guild, target, true);
        }
        let keep: BTreeSet<Id> = sources.iter().copied().collect();
        for guild in current.iter().filter(|guild| !keep.contains(guild)) {
            self.move_next_to(*guild, target, true);
        }
        self.sweep();
        true
    }

    /// The colour of a folder (the Folder Settings modal writes it with the name).
    pub fn set_folder_look(&mut self, target: u64, name: &str, color: Option<u64>) -> bool {
        let Some(Item::Folder(folder)) = self
            .roots
            .iter_mut()
            .find(|item| matches!(item, Item::Folder(folder) if folder.id == target))
        else {
            return false;
        };
        folder.name = (!name.is_empty()).then(|| name.to_owned());
        folder.color = color;
        true
    }

    /// `GUILD_FOLDER_DELETE_LOCAL` (`D`): every guild of the folder is moved to just after it, one by one, which
    /// leaves them in reverse order, and the emptied folder goes.
    pub fn delete_folder(&mut self, target: u64) -> bool {
        let Some(folder) = self.folder(target) else {
            return false;
        };
        let children = folder.guilds.clone();
        for guild in children {
            let Some(Place { root, .. }) = self.place(target) else {
                break;
            };
            let Some(item) = self.pluck_child(guild) else {
                continue;
            };
            self.roots.insert(root + 1, item);
        }
        self.sweep();
        true
    }

    /// A guild taken out of its folder without removing the folder when it empties.
    fn pluck_child(&mut self, guild: Id) -> Option<Item> {
        let place = self.place(guild)?;
        let child = place.child?;
        let Item::Folder(folder) = &mut self.roots[place.root] else {
            return None;
        };
        Some(Item::Guild(folder.guilds.remove(child)))
    }

    // ExpandedGuildFolderStore.

    pub fn is_expanded(&self, folder: u64) -> bool {
        self.expanded.contains(&folder)
    }

    pub fn expanded(&self) -> impl Iterator<Item = u64> + '_ {
        self.expanded.iter().copied()
    }

    /// `loadCache`-style restore of the persisted set.
    pub fn restore_expanded(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.expanded = ids.into_iter().collect();
        self.prune_expanded();
    }

    /// `TOGGLE_GUILD_FOLDER_EXPAND`. Returns the new state.
    pub fn toggle_expanded(&mut self, folder: u64) -> bool {
        if !self.expanded.remove(&folder) {
            self.expanded.insert(folder);
            return true;
        }
        false
    }

    /// `SET_GUILD_FOLDER_EXPANDED`.
    pub fn set_expanded(&mut self, folder: u64, expanded: bool) {
        if expanded {
            self.expanded.insert(folder);
        } else {
            self.expanded.remove(&folder);
        }
    }

    /// `GUILD_FOLDER_COLLAPSE` (the menu's Close All Folders). False when none was open.
    pub fn collapse_all(&mut self) -> bool {
        let any = !self.expanded.is_empty();
        self.expanded.clear();
        any
    }

    /// `USER_SETTINGS_PROTO_UPDATE` of the expanded store: ids of folders that no longer exist are dropped.
    fn prune_expanded(&mut self) {
        let alive: BTreeSet<u64> = self
            .roots
            .iter()
            .filter_map(|item| match item {
                Item::Folder(folder) => Some(folder.id),
                Item::Guild(_) => None,
            })
            .collect();
        self.expanded.retain(|id| alive.contains(id));
    }

    /// Restores the id generator, so a test gets known folder ids.
    pub fn seed_ids(&mut self, seed: u64) {
        self.seed = seed | 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(ids: &[Id]) -> Vec<GuildFolderEntry> {
        ids.iter()
            .map(|id| GuildFolderEntry {
                guild_ids: vec![*id],
                ..GuildFolderEntry::default()
            })
            .collect()
    }

    fn tree(ids: &[Id]) -> GuildFolders {
        let mut tree = GuildFolders::default();
        tree.rebuild(&plain(ids), &[], ids, &[]);
        tree
    }

    fn shape(tree: &GuildFolders) -> Vec<String> {
        tree.roots()
            .iter()
            .map(|item| match item {
                Item::Guild(id) => id.to_string(),
                Item::Folder(folder) => format!(
                    "[{}]",
                    folder
                        .guilds
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
            })
            .collect()
    }

    #[test]
    fn plain_entries_keep_their_order_and_unlisted_guilds_go_on_top() {
        let mut tree = GuildFolders::default();
        tree.rebuild(&plain(&[3, 1, 2]), &[], &[1, 2, 3, 8, 9], &[9, 8]);
        assert_eq!(shape(&tree), ["9", "8", "3", "1", "2"]);
    }

    #[test]
    fn legacy_positions_are_used_only_without_folders() {
        let mut tree = GuildFolders::default();
        tree.rebuild(&[], &[2, 1], &[1, 2], &[]);
        assert_eq!(shape(&tree), ["2", "1"]);
        tree.rebuild(&plain(&[1]), &[2, 1], &[1, 2], &[2]);
        assert_eq!(shape(&tree), ["2", "1"]);
    }

    #[test]
    fn unknown_guilds_duplicates_and_empty_folders_are_dropped() {
        let entries = vec![
            GuildFolderEntry {
                id: Some(100),
                guild_ids: vec![5, 6],
                ..Default::default()
            },
            GuildFolderEntry {
                id: Some(101),
                guild_ids: vec![7],
                ..Default::default()
            },
            GuildFolderEntry {
                id: Some(102),
                ..Default::default()
            },
            GuildFolderEntry {
                guild_ids: vec![5],
                ..Default::default()
            },
        ];
        let mut tree = GuildFolders::default();
        tree.rebuild(&entries, &[], &[5, 6], &[]);
        assert_eq!(shape(&tree), ["[5 6]"]);
    }

    #[test]
    fn a_plain_entry_with_several_ids_keeps_the_first_only() {
        let entries = vec![GuildFolderEntry {
            guild_ids: vec![4, 5],
            ..Default::default()
        }];
        let mut tree = GuildFolders::default();
        tree.rebuild(&entries, &[], &[4, 5], &[5]);
        assert_eq!(shape(&tree), ["5", "4"]);
    }

    #[test]
    fn drag_above_and_below() {
        let mut tree = tree(&[1, 2, 3, 4]);
        assert!(tree.move_by_id(4, 2, false, false));
        assert_eq!(shape(&tree), ["1", "4", "2", "3"]);
        assert!(tree.move_by_id(1, 3, true, false));
        assert_eq!(shape(&tree), ["4", "2", "3", "1"]);
        assert!(!tree.move_by_id(1, 1, true, false));
    }

    #[test]
    fn dropping_on_a_guild_makes_a_folder_and_dropping_on_a_folder_joins_it() {
        let mut tree = tree(&[1, 2, 3, 4]);
        tree.seed_ids(7);
        assert!(tree.move_by_id(1, 3, true, true));
        assert_eq!(shape(&tree), ["2", "[3 1]", "4"]);
        let folder = tree.folder_of(1).unwrap().id;
        assert!(tree.move_by_id(4, folder, true, true));
        assert_eq!(shape(&tree), ["2", "[3 1 4]"]);
        assert!(tree.move_by_id(2, folder, false, true));
        assert_eq!(shape(&tree), ["[2 3 1 4]"]);
        assert_eq!(tree.to_entries().len(), 1);
    }

    #[test]
    fn a_folder_cannot_be_combined_or_dropped_into_a_folder() {
        let mut tree = tree(&[1, 2, 3, 4]);
        let folder = tree.create_folder(&[1, 2], Some("x")).unwrap();
        assert!(!tree.move_by_id(folder, 3, false, true));
        assert!(!tree.move_by_id(folder, 2, false, false));
        // A guild cannot be combined with a guild already inside a folder.
        assert!(!tree.move_by_id(3, 2, true, true));
        assert_eq!(shape(&tree), ["[1 2]", "3", "4"]);
    }

    #[test]
    fn dragging_into_and_out_of_a_folder() {
        let mut tree = tree(&[1, 2, 3]);
        tree.create_folder(&[1, 2], None).unwrap();
        // Beside a guild that is inside the folder: joins the folder there.
        assert!(tree.move_by_id(3, 1, true, false));
        assert_eq!(shape(&tree), ["[1 3 2]"]);
        // Beside the folder: leaves it.
        let folder = tree.folder_of(3).unwrap().id;
        assert!(tree.move_by_id(3, folder, false, false));
        assert_eq!(shape(&tree), ["3", "[1 2]"]);
        // The last guild leaving removes the folder.
        assert!(tree.move_by_id(1, 3, true, false));
        assert!(tree.move_by_id(2, 3, true, false));
        assert_eq!(shape(&tree), ["3", "2", "1"]);
    }

    #[test]
    fn move_to_first_and_last() {
        let mut tree = tree(&[1, 2, 3]);
        assert_eq!(tree.placement_disabled(1), Some((true, false)));
        assert_eq!(tree.placement_disabled(3), Some((false, true)));
        assert!(tree.move_guild(3, Placement::First));
        assert_eq!(shape(&tree), ["3", "1", "2"]);
        assert!(tree.move_guild(3, Placement::Last));
        assert_eq!(shape(&tree), ["1", "2", "3"]);
        // A guild inside a folder is pulled out to the end.
        tree.create_folder(&[1, 2], None).unwrap();
        assert_eq!(tree.placement_disabled(1), Some((false, false)));
        assert!(tree.move_guild(1, Placement::Last));
        assert_eq!(shape(&tree), ["[2]", "3", "1"]);
        let single = GuildFolders::default();
        assert!(single.placement_disabled(1).is_none());
    }

    #[test]
    fn create_edit_and_delete_a_folder() {
        let mut tree = tree(&[1, 2, 3, 4]);
        let folder = tree.create_folder(&[2, 4], Some("Games")).unwrap();
        assert_eq!(shape(&tree), ["1", "[2 4]", "3"]);
        assert_eq!(tree.folder(folder).unwrap().name.as_deref(), Some("Games"));
        // Edit: keep 4, add 3, drop 2 (which lands after the folder).
        assert!(tree.edit_folder(folder, &[4, 3], ""));
        assert_eq!(shape(&tree), ["1", "[4 3]", "2"]);
        assert_eq!(tree.folder(folder).unwrap().name, None);
        // Delete: the guilds come out in reverse order after the folder's place.
        assert!(tree.delete_folder(folder));
        assert_eq!(shape(&tree), ["1", "3", "4", "2"]);
    }

    #[test]
    fn entries_round_trip_and_flatten() {
        let mut tree = tree(&[1, 2, 3]);
        let folder = tree.create_folder(&[1, 2], Some("A")).unwrap();
        assert!(tree.set_folder_look(folder, "A", Some(0xff0000)));
        let entries = tree.to_entries();
        assert_eq!(entries[0].color, Some(0xff0000));
        assert_eq!(entries[1].id, None);
        let mut again = GuildFolders::default();
        again.rebuild(&entries, &[], &[1, 2, 3], &[]);
        assert_eq!(again.roots(), tree.roots());
        assert_eq!(again.flattened(), vec![1, 2, 3]);
    }

    #[test]
    fn expanded_set_toggles_collapses_and_forgets_dead_folders() {
        let mut tree = tree(&[1, 2, 3]);
        let a = tree.create_folder(&[1, 2], None).unwrap();
        assert!(tree.toggle_expanded(a));
        assert!(tree.is_expanded(a));
        assert!(!tree.toggle_expanded(a));
        tree.set_expanded(a, true);
        assert!(tree.collapse_all());
        assert!(!tree.collapse_all());
        tree.set_expanded(a, true);
        tree.restore_expanded([a, 999]);
        assert_eq!(tree.expanded().collect::<Vec<_>>(), vec![a]);
        // A rebuild without the folder forgets it.
        tree.rebuild(&plain(&[1, 2, 3]), &[], &[1, 2, 3], &[]);
        assert!(!tree.is_expanded(a));
    }
}
