//! Reordering channels and categories by dragging them in the channel list. The drop maths is the official's
//! (module 44757 `QO` = where a drop lands, `RL` = the positions it writes; module 616740 for the drag source, the
//! drop target and the classes of the rows; module 466152 for the write `PATCH /guilds/:id/channels`). The line is
//! `.containerDragBefore/After` of stylesheets ed1ab6525c3040 (categories) and f4b2f93f8f92b7 (channels), the dimmed
//! rows are `.disabled_Wx6SnQ>div{opacity:.3}`.

use super::{Root, color};
use crate::net::Request;
use dc_state::{Channel, ChannelKind, Guild, Id};
use gpui::{
    AnyElement, Bounds, Context, DragMoveEvent, IntoElement, Pixels, Render, SharedString, Window,
    canvas, div, prelude::*, px,
};
use serde_json::{Value, json};
use std::cell::RefCell;

/// What a type may be dropped among (`ws` = module 44757 `a`): categories with categories, text-like channels with
/// text-like ones, voice and Stage channels with each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cls {
    Category,
    Text,
    Vocal,
}

pub(super) fn cls_of(kind: ChannelKind) -> Cls {
    match kind {
        ChannelKind::Category => Cls::Category,
        ChannelKind::Voice | ChannelKind::Other(13) => Cls::Vocal,
        _ => Cls::Text,
    }
}

/// One entry of the list (`{channel, index}` of the `GuildCategoryStore`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Item {
    pub id: Id,
    pub cls: Cls,
    pub parent: Option<Id>,
    pub position: i64,
}

/// The uncategorized channels (no category) and then each category with its children, text before voice: module
/// 769765 `getCategories`.
#[derive(Clone, Debug, Default)]
pub(super) struct Layout {
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Group {
    pub category: Option<Item>,
    pub children: Vec<Item>,
}

impl Layout {
    pub(super) fn of(guild: &Guild) -> Self {
        let item = |channel: &Channel| Item {
            id: channel.id,
            cls: cls_of(channel.kind),
            parent: channel.parent_id,
            position: channel.position,
        };
        let mut categories: Vec<&Channel> = guild
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Category)
            .collect();
        categories.sort_by_key(|channel| (channel.position, channel.id));
        let kids = |parent: Option<Id>| {
            let mut rows: Vec<&Channel> = guild
                .channels
                .iter()
                .filter(|channel| {
                    channel.parent_id == parent && channel.kind != ChannelKind::Category
                })
                .collect();
            rows.sort_by_key(|channel| {
                (
                    cls_of(channel.kind) == Cls::Vocal,
                    channel.position,
                    channel.id,
                )
            });
            rows.into_iter().map(item).collect::<Vec<_>>()
        };
        let mut groups = vec![Group {
            category: None,
            children: kids(None),
        }];
        for category in categories {
            groups.push(Group {
                category: Some(item(category)),
                children: kids(Some(category.id)),
            });
        }
        Self { groups }
    }

    /// Module 999903: the groups in `order` flattened, each category before its children, then filtered.
    pub(super) fn flat(&self, order: &[usize], keep: impl Fn(&Item) -> bool) -> Vec<Item> {
        let mut out = Vec::new();
        for &at in order {
            let group = &self.groups[at];
            out.extend(group.category);
            out.extend(group.children.iter().copied());
        }
        out.retain(|item| keep(item));
        out
    }

    fn natural(&self) -> Vec<usize> {
        (0..self.groups.len()).collect()
    }

    /// Every entry, the index of each being its `position` for the drop maths.
    pub(super) fn all(&self) -> Vec<Item> {
        self.flat(&self.natural(), |_| true)
    }

    /// The list a drag works on (module 616740 `channelList`): what a collapsed category hides is left out, and an
    /// empty category stays (only a person who may manage channels drags at all).
    pub(super) fn visible(&self, collapsed: impl Fn(Id) -> bool) -> Vec<Item> {
        self.flat(&self.natural(), |item| match item.parent {
            Some(parent) if item.cls != Cls::Category => !collapsed(parent),
            _ => true,
        })
    }
}

/// Where a drop puts the dragged channel: after `reference` (None: first), under `parent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Landing {
    pub reference: Option<Id>,
    pub parent: Option<Id>,
}

fn at(list: &[Item], index: isize) -> Option<Item> {
    usize::try_from(index)
        .ok()
        .and_then(|index| list.get(index))
        .copied()
}

/// Module 44757 `d`: from the entry `id`, step by `step` to the first entry that takes a `cls`.
fn scan(step: isize, id: Id, cls: Cls, list: &[Item]) -> Option<Item> {
    let mut index = list.iter().position(|item| item.id == id)? as isize;
    while let Some(item) = at(list, index) {
        if item.cls == cls {
            return Some(item);
        }
        index += step;
    }
    None
}

/// Module 44757 `c(n, list, true)`: the entry's place in the whole list.
fn index_in(list: &[Item], id: Id) -> Option<usize> {
    list.iter().position(|item| item.id == id)
}

/// Module 44757 `QO`: where dropping `dragged` (at `source`) on `target` (at `over`) lands, or `None` when it cannot.
pub(super) fn resolve(
    dragged: &Item,
    source: i64,
    target: &Item,
    over: i64,
    list: &[Item],
) -> Option<Landing> {
    let here = index_in(list, target.id).unwrap_or(0) as isize;
    let land = |item: &Item| Landing {
        reference: Some(item.id),
        parent: item.parent,
    };
    if dragged.cls == Cls::Category {
        if over == source || (over < source && target.cls == Cls::Category) {
            return Some(land(target));
        }
        if over > source {
            let next = at(list, here + 1);
            let back = scan(-1, target.id, dragged.cls, list)?;
            if back.id == dragged.id {
                return None;
            }
            return next
                .is_none_or(|next| next.cls == Cls::Category)
                .then_some(Landing {
                    reference: Some(back.id),
                    parent: None,
                });
        }
        return None;
    }
    if dragged.cls == target.cls {
        return Some(land(target));
    }
    if over < source {
        let before = at(list, here - 1);
        let after = scan(1, target.id, dragged.cls, list);
        if target.cls == Cls::Category {
            let Some(before) = before else {
                return Some(Landing {
                    reference: None,
                    parent: None,
                });
            };
            let after = after?;
            if before.cls == dragged.cls || (dragged.cls == Cls::Vocal && before.cls == Cls::Text) {
                return Some(Landing {
                    reference: Some(after.id),
                    parent: before.parent,
                });
            }
            if before.cls == Cls::Category {
                return Some(Landing {
                    reference: Some(after.id),
                    parent: Some(before.id),
                });
            }
            return None;
        }
        if before.is_some() || dragged.cls == Cls::Vocal {
            let (Some(before), Some(after)) = (before, after) else {
                return None;
            };
            return (dragged.cls == Cls::Text
                && (before.cls == Cls::Text || before.cls == Cls::Category))
                .then_some(Landing {
                    reference: Some(after.id),
                    parent: target.parent,
                });
        }
        return Some(Landing {
            reference: after.map(|item| item.id),
            parent: None,
        });
    }
    let next = at(list, here + 1);
    let back = scan(-1, target.id, dragged.cls, list);
    if target.cls == Cls::Category {
        let back = back?;
        return match next {
            None => Some(Landing {
                reference: Some(back.id),
                parent: Some(target.id),
            }),
            Some(next) => {
                if next.cls == dragged.cls || (dragged.cls == Cls::Text && next.cls == Cls::Vocal) {
                    Some(Landing {
                        reference: Some(back.id),
                        parent: next.parent,
                    })
                } else if next.cls == Cls::Category {
                    Some(Landing {
                        reference: Some(back.id),
                        parent: Some(target.id),
                    })
                } else {
                    None
                }
            }
        };
    }
    let back = back?;
    if dragged.cls == Cls::Vocal {
        match next {
            None => {
                return Some(Landing {
                    reference: Some(back.id),
                    parent: target.parent,
                });
            }
            Some(next) if next.cls == Cls::Category => {
                return Some(Landing {
                    reference: Some(back.id),
                    parent: target.parent,
                });
            }
            Some(next) if next.cls == Cls::Vocal => {
                return Some(Landing {
                    reference: Some(back.id),
                    parent: next.parent,
                });
            }
            Some(_) => {}
        }
    }
    None
}

/// One changed channel of the write: a new position and/or a new parent (`Some(None)` is "no category").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Delta {
    pub id: Id,
    pub position: Option<i64>,
    pub parent: Option<Option<Id>>,
}

/// Module 111613 `moveItemFromTo`.
fn move_item(list: &[Item], from: usize, to: usize) -> Vec<Item> {
    let mut out = list.to_vec();
    let item = out.remove(from);
    out.insert(to.min(out.len()), item);
    out
}

/// Module 111613 `calculatePositionDeltas`: every entry whose place in `new` is not the position it has.
fn position_deltas(old: &[Item], new: &[Item]) -> Vec<Delta> {
    let mut ids: Vec<Id> = old.iter().map(|item| item.id).collect();
    let mut new_ids: Vec<Id> = new.iter().map(|item| item.id).collect();
    ids.sort_unstable();
    new_ids.sort_unstable();
    if ids != new_ids {
        return Vec::new();
    }
    new.iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let was = old
                .iter()
                .find(|known| known.id == item.id)
                .map(|known| known.position);
            let index = index as i64;
            (was != Some(index) || item.position != index).then_some(Delta {
                id: item.id,
                position: Some(index),
                parent: None,
            })
        })
        .collect()
}

/// Module 44757 `c(n, list)`: the place of `n` among the entries that take its type.
fn place(list: &[Item], item: Option<&Item>) -> Option<usize> {
    let Some(item) = item else {
        return Some(0);
    };
    list.iter()
        .filter(|known| known.cls == item.cls)
        .position(|known| known.id == item.id)
}

/// Module 44757 `RL`: the changes a drop of `dragged` after `reference`, under `parent`, writes.
pub(super) fn position_updates(
    dragged: &Item,
    reference: Option<&Item>,
    parent: Option<Id>,
    layout: &Layout,
) -> Vec<Delta> {
    let mut out: Vec<Delta> = Vec::new();
    let mut order = layout.natural();
    let mut reorder = |list: &[Item], from: Option<usize>, to: Option<usize>| -> Vec<Item> {
        let moved = match (from, to) {
            (Some(from), Some(to))
                if list.get(from).is_some_and(|item| item.id == dragged.id)
                    && list.get(to).is_some() =>
            {
                move_item(list, from, to)
            }
            _ => list.to_vec(),
        };
        out.extend(position_deltas(list, &moved));
        moved
    };
    if dragged.cls == Cls::Category {
        // The categories alone, the uncategorized group left where it is.
        let cats: Vec<Item> = layout.groups[1..]
            .iter()
            .filter_map(|group| group.category)
            .collect();
        let moved = reorder(&cats, place(&cats, Some(dragged)), place(&cats, reference));
        let mut next = vec![0];
        for item in moved {
            if let Some(at) = layout
                .groups
                .iter()
                .position(|group| group.category == Some(item))
            {
                next.push(at);
            }
        }
        order = next;
    }
    if dragged.cls == Cls::Text || dragged.cls == Cls::Category {
        let list = layout.flat(&order, |item| item.cls == Cls::Text);
        reorder(&list, place(&list, Some(dragged)), place(&list, reference));
    }
    if dragged.cls == Cls::Vocal || dragged.cls == Cls::Category {
        let list = layout.flat(&order, |item| item.cls == Cls::Vocal);
        reorder(&list, place(&list, Some(dragged)), place(&list, reference));
    }
    if dragged.parent != parent {
        match out.iter_mut().find(|delta| delta.id == dragged.id) {
            Some(delta) => delta.parent = Some(parent),
            None => out.push(Delta {
                id: dragged.id,
                position: None,
                parent: Some(parent),
            }),
        }
    }
    out
}

/// The body of `PATCH /guilds/:id/channels` (module 778858 `batchChannelUpdate`).
pub(super) fn update_body(deltas: &[Delta]) -> Value {
    Value::Array(
        deltas
            .iter()
            .map(|delta| {
                let mut entry = json!({"id": delta.id.to_string()});
                if let Some(position) = delta.position {
                    entry["position"] = json!(position);
                }
                if let Some(parent) = delta.parent {
                    entry["parent_id"] = json!(parent.map(|parent| parent.to_string()));
                }
                entry
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------------------------------------------
// The drag in the list
// ---------------------------------------------------------------------------------------------------------------

/// What is dragged: a channel or a category of the open server.
#[derive(Clone)]
pub(super) struct ChannelDrag {
    pub id: Id,
    pub label: SharedString,
}

struct Ghost {
    drag: ChannelDrag,
}

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The browser's drag image of the row: its text on the hovered row's background, a little see-through.
        div()
            .h(px(32.))
            .px(px(16.))
            .flex()
            .items_center()
            .rounded(px(4.))
            .opacity(0.7)
            .bg(color("--background-mod-subtle"))
            .text_size(px(16.))
            .text_color(color("--text-strong"))
            .child(self.drag.label.clone())
    }
}

/// Which side of the target the line is on, and what the drop would do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Over {
    pub target: Id,
    pub after: bool,
    pub drop: Landing,
}

#[derive(Default)]
pub(super) struct State {
    /// The entry being dragged, once the drag has moved.
    pub drag: Option<Item>,
    pub over: Option<Over>,
}

struct Row {
    id: Id,
    top: f32,
    height: f32,
}

thread_local! {
    static ROWS: RefCell<Vec<Row>> = const { RefCell::new(Vec::new()) };
}

impl Root {
    /// A finished drag clears its marks (once a frame).
    pub(super) fn channel_drag_frame(&mut self, cx: &mut Context<Self>) {
        if self.channel_drag.drag.is_some() && !cx.has_active_drag() {
            self.channel_drag.drag = None;
            self.channel_drag.over = None;
        }
    }

    /// Called when the list is rebuilt: the painted rows of the last frame are forgotten.
    pub(super) fn channel_drag_begin(&self) {
        ROWS.with(|rows| rows.borrow_mut().clear());
    }

    /// Whether this person may drag channels here (module 79843: `MANAGE_CHANNELS`), and the entry is not pinned.
    fn channel_drag_allowed(&self, channel: &Channel) -> bool {
        self.can_manage_channels()
            && self.guild.is_some_and(|guild| {
                self.stores.guild_settings.channel(guild, channel.id).flags & 2048 == 0
            })
    }

    /// A row of the list as a drag source and drop target: the dimmed look while a drag of another kind is under
    /// way, and the green line on the side the drop would land.
    pub(super) fn channel_drag_wrap(
        &self,
        channel: &Channel,
        row: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.channel_drag_allowed(channel) {
            return row;
        }
        let id = channel.id;
        let cls = cls_of(channel.kind);
        let drag = ChannelDrag {
            id,
            label: channel.name.clone().into(),
        };
        // `isDisabled` (`lY`): a drag under way dims the channel rows its kind cannot be dropped among.
        let dim = cls != Cls::Category
            && self
                .channel_drag
                .drag
                .is_some_and(|source| source.cls != cls);
        let line = self.channel_drag.over.filter(|over| over.target == id);
        let mut wrapper = div()
            .id(SharedString::from(format!("channel-drag-{id}")))
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .on_drag(drag, |drag, _, _, cx| {
                cx.new(|_| Ghost { drag: drag.clone() })
            })
            .child(
                canvas(
                    move |bounds: Bounds<Pixels>, _, _| {
                        ROWS.with(|rows| {
                            rows.borrow_mut().push(Row {
                                id,
                                top: f32::from(bounds.origin.y),
                                height: f32::from(bounds.size.height),
                            })
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(div().when(dim, |this| this.opacity(0.3)).child(row));
        if let Some(over) = line {
            // `inset-inline: 8px 0`, 4px high, `top: -2px` / `bottom: -2px`, `--green-360`, a 3px soft shadow.
            let mark = div()
                .absolute()
                .left(px(8.))
                .right(px(9.))
                .h(px(4.))
                .rounded(px(2.))
                .bg(color("--green-360"))
                .shadow(vec![gpui::BoxShadow {
                    color: color("--opacity-black-40").into(),
                    offset: gpui::point(px(0.), px(0.)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                    inset: false,
                }]);
            wrapper = wrapper.child(if over.after {
                mark.bottom(px(-2.))
            } else {
                mark.top(px(-2.))
            });
        }
        let _ = cx;
        wrapper.into_any_element()
    }

    fn channel_drag_context(&self) -> Option<(Id, Layout)> {
        let guild = self.current_guild()?;
        Some((guild.id, Layout::of(guild)))
    }

    /// The drag moves over the list: which row it is over and what a drop there would do.
    pub(super) fn channel_drag_moved(
        &mut self,
        event: &DragMoveEvent<ChannelDrag>,
        cx: &mut Context<Self>,
    ) {
        let id = event.drag(cx).id;
        let y = f32::from(event.event.position.y);
        let target = ROWS.with(|rows| {
            rows.borrow()
                .iter()
                .find(|row| y >= row.top && y < row.top + row.height)
                .map(|row| row.id)
        });
        let Some((guild, layout)) = self.channel_drag_context() else {
            return;
        };
        let all = layout.all();
        let collapsed = |category: Id| {
            self.stores
                .guild_settings
                .channel(guild, category)
                .collapsed
        };
        let list = layout.visible(collapsed);
        let place = |wanted: Id| all.iter().position(|item| item.id == wanted);
        let (Some(source), over) = (all.iter().find(|item| item.id == id).copied(), target) else {
            return;
        };
        let over = over.filter(|target| *target != id).and_then(|target| {
            let item = all.iter().find(|item| item.id == target)?;
            let drop = resolve(
                &source,
                place(id)? as i64,
                item,
                place(target)? as i64,
                &list,
            )?;
            Some(Over {
                target,
                after: place(target)? > place(id)?,
                drop,
            })
        });
        if self.channel_drag.drag != Some(source) || self.channel_drag.over != over {
            self.channel_drag.drag = Some(source);
            self.channel_drag.over = over;
            cx.notify();
        }
    }

    /// The drop: the positions the official client sends (`PATCH /guilds/:id/channels`). The list is not reordered
    /// here; it changes when the server's update arrives.
    pub(super) fn channel_dropped(&mut self, drag: &ChannelDrag, cx: &mut Context<Self>) {
        let over = self.channel_drag.over.take();
        self.channel_drag.drag = None;
        if let (Some(over), Some((guild, layout))) = (over, self.channel_drag_context()) {
            let all = layout.all();
            let dragged = all.iter().find(|item| item.id == drag.id).copied();
            let reference = over
                .drop
                .reference
                .and_then(|id| all.iter().find(|item| item.id == id).copied());
            if let Some(dragged) = dragged {
                let deltas =
                    position_updates(&dragged, reference.as_ref(), over.drop.parent, &layout);
                if !deltas.is_empty() {
                    let _ = self.requests.send(Request::Rest {
                        key: None,
                        method: reqwest::Method::PATCH,
                        route: format!("/guilds/{guild}/channels"),
                        body: Some(update_body(&deltas)),
                    });
                }
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT_CAT: Id = 10;
    const VOICE_CAT: Id = 11;
    const GENERAL: Id = 20;
    const LOUNGE: Id = 21;

    fn item(id: Id, cls: Cls, parent: Option<Id>, position: i64) -> Item {
        Item {
            id,
            cls,
            parent,
            position,
        }
    }

    /// The fixture guild's shape: a text category with one text channel, a voice category with one voice channel.
    fn fixture() -> Layout {
        Layout {
            groups: vec![
                Group::default(),
                Group {
                    category: Some(item(TEXT_CAT, Cls::Category, None, 0)),
                    children: vec![item(GENERAL, Cls::Text, Some(TEXT_CAT), 0)],
                },
                Group {
                    category: Some(item(VOICE_CAT, Cls::Category, None, 1)),
                    children: vec![item(LOUNGE, Cls::Vocal, Some(VOICE_CAT), 0)],
                },
            ],
        }
    }

    fn find(list: &[Item], id: Id) -> Item {
        *list.iter().find(|item| item.id == id).unwrap()
    }

    #[test]
    fn a_text_channel_over_the_voice_category_goes_to_its_top() {
        let layout = fixture();
        let list = layout.all();
        let (dragged, target) = (find(&list, GENERAL), find(&list, VOICE_CAT));
        let drop = resolve(&dragged, 1, &target, 2, &list).unwrap();
        assert_eq!(
            drop,
            Landing {
                reference: Some(GENERAL),
                parent: Some(VOICE_CAT)
            }
        );
        // Only its category changes; the position stays.
        let deltas = position_updates(&dragged, Some(&dragged), drop.parent, &layout);
        assert_eq!(
            deltas,
            vec![Delta {
                id: GENERAL,
                position: None,
                parent: Some(Some(VOICE_CAT))
            }]
        );
        assert_eq!(
            update_body(&deltas),
            json!([{"id": "20", "parent_id": "11"}])
        );
    }

    #[test]
    fn a_category_over_the_one_above_goes_before_it_and_both_get_new_positions() {
        let layout = fixture();
        let list = layout.all();
        let (dragged, target) = (find(&list, VOICE_CAT), find(&list, TEXT_CAT));
        let drop = resolve(&dragged, 3, &target, 0, &list).unwrap();
        assert_eq!(
            drop,
            Landing {
                reference: Some(TEXT_CAT),
                parent: None
            }
        );
        let deltas = position_updates(&dragged, Some(&target), None, &layout);
        assert_eq!(
            update_body(&deltas),
            json!([{"id": "11", "position": 0}, {"id": "10", "position": 1}])
        );
    }

    #[test]
    fn kinds_that_cannot_share_a_place_take_no_drop() {
        let layout = fixture();
        let list = layout.all();
        // A text channel over a voice channel below it, a voice channel over a text channel above it.
        let (general, lounge) = (find(&list, GENERAL), find(&list, LOUNGE));
        assert_eq!(resolve(&general, 1, &lounge, 3, &list), None);
        assert_eq!(resolve(&lounge, 3, &general, 1, &list), None);
    }

    #[test]
    fn a_voice_channel_over_the_text_category_above_it_has_no_place() {
        let layout = fixture();
        let list = layout.all();
        let (lounge, cat) = (find(&list, LOUNGE), find(&list, TEXT_CAT));
        // Before the first category nothing precedes it, so the voice channel would land uncategorized.
        let drop = resolve(&lounge, 3, &cat, 0, &list).unwrap();
        assert_eq!(
            drop,
            Landing {
                reference: None,
                parent: None
            }
        );
    }

    #[test]
    fn moving_a_text_channel_renumbers_the_text_channels() {
        // Two text channels in one category with stale positions: the drop writes the whole order.
        let layout = Layout {
            groups: vec![
                Group::default(),
                Group {
                    category: Some(item(TEXT_CAT, Cls::Category, None, 0)),
                    children: vec![
                        item(GENERAL, Cls::Text, Some(TEXT_CAT), 5),
                        item(22, Cls::Text, Some(TEXT_CAT), 6),
                    ],
                },
            ],
        };
        let list = layout.all();
        let dragged = find(&list, GENERAL);
        let target = find(&list, 22);
        let drop = resolve(&dragged, 1, &target, 2, &list).unwrap();
        let reference = find(&list, drop.reference.unwrap());
        let deltas = position_updates(&dragged, Some(&reference), drop.parent, &layout);
        assert_eq!(
            update_body(&deltas),
            json!([{"id": "22", "position": 0}, {"id": "20", "position": 1}])
        );
    }
}
