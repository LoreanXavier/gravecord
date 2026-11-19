//! The placeholder messages a channel shows while its first page loads or while older/newer pages can still be
//! fetched (`hasMoreBefore` / `hasMoreAfter`, module 891522 `lQ` around module 114212, classes of module 960036,
//! stylesheet 123141): grey blobs at low opacity in the shape of message groups, drawn where the messages will be.
//!
//! The specs (`generateMessageSpecs` of 891522 with the group count of module 326337 `I`) are drawn once per load,
//! never per frame: the official widths and opacities are lodash `random` at mount, so they are random here too.

use super::{Root, color};
use dc_state::Id;
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use rand::Rng;

/// One placeholder row of a group (module 114212 `u`).
#[derive(Clone, Debug, PartialEq)]
struct Line {
    /// `random(120, 320)`, the content blob's width at 16px.
    content: u32,
    /// `random(.02, .08)`: the compact timestamp blob's opacity (`F` without one).
    stamp_opacity: f32,
}

/// A group of rows (one module 114212 `A`) with its shared name blob and maybe an attachment.
#[derive(Clone, Debug, PartialEq)]
struct Group {
    lines: Vec<Line>,
    /// `random(80, 120)` and `random(.1, .2)`: the name blob of the group.
    name_width: u32,
    name_opacity: f32,
    /// `{width: random(140, 400), height: random(100, 320)}`.
    attachment: Option<(u32, u32)>,
}

/// The block drawn for one load, and what it was drawn for.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Held {
    channel: Id,
    compact: bool,
    font: f32,
    height: f32,
    groups: Vec<Group>,
}

impl Held {
    fn total_lines(&self) -> usize {
        self.groups.iter().map(|group| group.lines.len()).sum()
    }

    fn line_at(&self, mut flat: usize) -> Option<(&Group, usize)> {
        for group in &self.groups {
            if flat < group.lines.len() {
                return Some((group, flat));
            }
            flat -= group.lines.len();
        }
        None
    }
}

/// Module 326337 `o`: `clamp(10, 30, ceil(1.05 * ceil(height / ((compact ? 65 : 91) * fontSize / 16))))`, where the
/// height is `max(windowHeight - 145, 400)`.
fn group_count(window_height: f32, compact: bool, font: f32) -> usize {
    let height = (window_height - 145.).max(400.);
    let per = if compact { 65. } else { 91. } * (font / 16.);
    ((1.05 * (height / per).ceil()).ceil() as usize).clamp(10, 30)
}

/// `generateMessageSpecs({compact, messageGroups, groupRange: 4, attachments, fontSize, groupSpacing})`: the groups
/// with 1 to 4 rows, `max(1, round(groups / 30 * 8))` attachments on distinct groups.
fn specs(window_height: f32, compact: bool, font: f32, rng: &mut impl Rng) -> Vec<Group> {
    let count = group_count(window_height, compact, font);
    // `V = compact ? K : round(.87 * K)`.
    let groups = if compact {
        count
    } else {
        (0.87 * count as f32).round() as usize
    };
    let attachments = ((groups as f32 / 30. * 8.).round() as usize).max(1);
    let mut out: Vec<Group> = (0..groups)
        .map(|_| Group {
            lines: (0..rng.random_range(1..=4))
                .map(|_| Line {
                    content: rng.random_range(120..=320),
                    stamp_opacity: rng.random_range(0.02..0.08),
                })
                .collect(),
            name_width: rng.random_range(80..=120),
            name_opacity: rng.random_range(0.1..0.2),
            attachment: None,
        })
        .collect();
    let mut free: Vec<usize> = (0..groups).collect();
    for _ in 0..attachments.min(groups) {
        let at = free.remove(rng.random_range(0..free.len()));
        out[at].attachment = Some((rng.random_range(140..=400), rng.random_range(100..=320)));
    }
    out
}

/// `blob_oITdBA`: 1rem high, `.1875rem` from the top of its 1.375rem line, `.5rem` corners, `--text-default`.
fn blob(width: f32, opacity: f32, rem: f32) -> gpui::Div {
    div()
        .flex_none()
        .w(px(width))
        .h(px(rem))
        .mt(px(0.1875 * rem))
        .rounded(px(0.5 * rem))
        .bg(color("--text-default"))
        .opacity(opacity)
}

/// One row: `wrapper_oITdBA` with `cozy` (4.5rem in, 1rem at the end) or `compact` (.125rem above and below,
/// `contents` 5rem in).
fn line(group: &Group, index: usize, compact: bool, spacing: f32, rem: f32) -> AnyElement {
    let scale = rem / 16.;
    let start = index == 0;
    let entry = &group.lines[index];
    let name = || blob(group.name_width as f32 * scale, group.name_opacity, rem);
    // `content`: 1.375rem, the blob centred (cozy) or a flex item (compact `display: contents`).
    let content = blob(entry.content as f32 * scale, 0.06, rem);
    let row = div()
        .relative()
        .flex_none()
        .w_full()
        .pr(px(rem))
        // The first row of a group: `marginTop: groupSpacing / 16 rem`.
        .when(start, |row| row.mt(px(spacing * scale)));
    let row = if compact {
        row.py(px(0.125 * rem)).child(
            div()
                .h(px(1.375 * rem))
                .pl(px(5. * rem))
                .flex()
                .flex_wrap()
                .overflow_hidden()
                // `compactTimestamp`: 50 wide, `margin-inline: -4rem .25rem`, hidden after the first row.
                .child(
                    blob(50. * scale, entry.stamp_opacity, rem)
                        .ml(px(-4. * rem))
                        .mr(px(0.25 * rem))
                        .when(!start, |stamp| stamp.invisible()),
                )
                .child(div().flex_none().mr(px(0.5 * rem)).child(name()))
                .child(content),
        )
    } else {
        row.pl(px(4.5 * rem))
            .when(start, |row| {
                // `avatar_oITdBA`: 2.5rem (at most 40px) round, 1rem in, .25rem down, at .08.
                row.child(
                    div()
                        .absolute()
                        .left(px(rem))
                        .top(px(0.25 * rem))
                        .size(px((2.5 * rem).min(40.)))
                        .rounded_full()
                        .bg(color("--text-default"))
                        .opacity(0.08),
                )
                .child(
                    div()
                        .h(px(1.375 * rem))
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .child(name()),
                )
            })
            .child(
                div()
                    .h(px(1.375 * rem))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(content),
            )
    };
    // The group's attachment goes under its last row (`attachmentSpecs` on that row's `u`).
    let attachment = (index + 1 == group.lines.len())
        .then_some(group.attachment)
        .flatten();
    row.when_some(attachment, |row, (width, height)| {
        // `attachmentContainer` (.125rem above and below, .25rem more in compact) and `attachment` (.125rem down,
        // 6px corners, 5rem in when compact) at .03.
        row.child(
            div()
                .py(px(0.125 * rem))
                .when(compact, |box_| box_.mt(px(0.25 * rem)))
                .child(
                    div()
                        .mt(px(0.125 * rem))
                        .when(compact, |att| att.ml(px(5. * rem)))
                        .w(px(width as f32))
                        .h(px(height as f32))
                        .rounded(px(6.))
                        .bg(color("--text-default"))
                        .opacity(0.03),
                ),
        )
    })
    .into_any_element()
}

impl Root {
    /// The open channel is on its first load: its window is not ready, still has older messages to fetch and holds
    /// none (`hasMoreBefore` and an empty list), and no error stopped the load.
    fn chat_loading(&self) -> Option<Id> {
        let channel = self.channel?;
        let empty = self
            .stores
            .messages
            .get(&channel)
            .is_none_or(|list| list.is_empty());
        let loading = self
            .stores
            .windows
            .get(&channel)
            .is_none_or(|window| !window.ready && window.has_more_before && !window.error);
        (empty && loading).then_some(channel)
    }

    /// Ensures the cached skeleton spec matches `channel` and current display settings, returning its line count.
    /// Used for virtualised `Row::SkeletonBefore` / `Row::SkeletonAfter` rows above/below a loaded window so no
    /// skeleton rows are built while scrolled away from the edge.
    pub(super) fn ensure_chat_skeleton(&mut self, channel: Id, window_height: f32) -> usize {
        let compact = self.compact_messages();
        let font = self.chat_font_size();
        let fresh = self.chat_extra.skeleton.as_ref().is_none_or(|held| {
            held.channel != channel
                || held.compact != compact
                || held.font != font
                || (held.height - window_height).abs() > 1.
        });
        if fresh {
            self.chat_extra.skeleton = Some(Held {
                channel,
                compact,
                font,
                height: window_height,
                groups: specs(window_height, compact, font, &mut rand::rng()),
            });
        }
        self.chat_extra
            .skeleton
            .as_ref()
            .map_or(0, Held::total_lines)
    }

    /// One virtualised skeleton row (`flat_index` across the cached `Held` groups).
    pub(super) fn skeleton_row(&self, flat_index: usize) -> AnyElement {
        let Some(held) = self.chat_extra.skeleton.as_ref() else {
            return div().into_any_element();
        };
        let Some((group, index)) = held.line_at(flat_index) else {
            return div().into_any_element();
        };
        line(
            group,
            index,
            held.compact,
            self.message_group_spacing(),
            held.font,
        )
    }

    /// The placeholder block over the empty chat while the channel's first page loads, its bottom where the list's
    /// last row would end (above the 40px spacer).
    pub(super) fn chat_skeleton(&mut self, window_height: f32) -> Option<AnyElement> {
        let channel = self.chat_loading()?;
        let _ = self.ensure_chat_skeleton(channel, window_height);
        let held = self.chat_extra.skeleton.as_ref()?;
        let compact = held.compact;
        let font = held.font;
        let spacing = self.message_group_spacing();
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom(px(40.))
                .overflow_hidden()
                .flex()
                .flex_col()
                .justify_end()
                .bg(color("--background-base-lower"))
                .children(held.groups.iter().flat_map(|group| {
                    (0..group.lines.len())
                        .map(move |index| line(group, index, compact, spacing, font))
                }))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn the_group_count_follows_the_window_and_the_font() {
        // max(1000 - 145, 400) / 91 = 9.4 -> 10, * 1.05 = 10.5 -> 11.
        assert_eq!(group_count(1000., false, 16.), 11);
        // The 400px floor: 400 / 65 = 6.2 -> 7, * 1.05 -> 8, then the floor of 10.
        assert_eq!(group_count(300., true, 16.), 10);
        // 2000 - 145 = 1855; / 65 = 28.5 -> 29, * 1.05 = 30.45 -> 31, then the ceiling of 30.
        assert_eq!(group_count(2000., true, 16.), 30);
        // A larger font: 1855 / (91 * 1.5) = 13.6 -> 14, * 1.05 = 14.7 -> 15.
        assert_eq!(group_count(2000., false, 24.), 15);
    }

    #[test]
    fn specs_hold_one_to_four_rows_and_attachments_on_distinct_groups() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let groups = specs(1000., false, 16., &mut rng);
        // Cozy: round(.87 * 11) = 10 groups, max(1, round(10 / 30 * 8)) = 3 attachments.
        assert_eq!(groups.len(), 10);
        assert_eq!(
            groups
                .iter()
                .filter(|group| group.attachment.is_some())
                .count(),
            3
        );
        for group in &groups {
            assert!((1..=4).contains(&group.lines.len()));
            assert!((80..=120).contains(&group.name_width));
            assert!((0.1..0.2).contains(&group.name_opacity));
            for line in &group.lines {
                assert!((120..=320).contains(&line.content));
                assert!((0.02..0.08).contains(&line.stamp_opacity));
            }
            if let Some((width, height)) = group.attachment {
                assert!((140..=400).contains(&width) && (100..=320).contains(&height));
            }
        }
        // Compact keeps every group: K = 10 at this height (855 / 65 = 13.2 -> 14, * 1.05 -> 15).
        assert_eq!(specs(1000., true, 16., &mut rng).len(), 15);
    }
}
