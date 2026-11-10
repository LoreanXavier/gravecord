//! The Quick Switcher (`quickswitcher_a1DYes` in a large modal), opened with Ctrl+K or "Find or start a conversation":
//! "Where would you like to go?", Previous Channels (this session's history and the channel frecency, the open
//! channel left out) or matches for the query (`@` people, `#` text channels, `!` voice channels, `*` servers), arrow
//! keys to pick, Enter to go. The arrows above it point at the box (`tutorial_12gY2-`). Measured in
//! `design/dom/modals/quickswitcher-darker-cozy.json`.

use super::actions::FRECENCY_KEY;
use super::{Root, avatar_source, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use dc_state::frecency::{Frecency, Kind, Usage};
use dc_state::{ChannelKind, Id, User};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, Image, ImageFormat, IntoElement,
    SharedString, Subscription, Window, deferred, div, img, prelude::*, px, rgba,
};
use std::sync::Arc;

const MAX_RESULTS: usize = 15;

// The prefix/substring tiers of 787578. Equal scores keep username, friend nickname, global name order.
// Fuzzy/normalized matches and guild nicknames still need the full search index.
fn matched_user_name<'a>(
    user: &'a User,
    nickname: Option<&'a str>,
    query: &str,
) -> Option<&'a str> {
    let mut best = None;
    let mut best_score = 0;
    for name in [
        Some(user.username.as_str()),
        nickname,
        user.global_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let lower = name.to_lowercase();
        let score = if lower.starts_with(query) {
            10
        } else if lower.contains(query) {
            5
        } else {
            0
        };
        if score > best_score {
            best = Some(name);
            best_score = score;
        }
    }
    best
}

/// The tutorial arrows: asset, rotation in degrees, its laid-out box (x, y, width, height) relative to the
/// switcher's left and the tutorial's top, measured from the capture.
const ARROWS: [(&str, f32, [f32; 4]); 3] = [
    ("a8546d03debd645f", 0., [0., 118., 43., 18.]),
    ("43a0c71f5cc76261", 40., [-4.8, 24., 51.6, 48.5]),
    ("6c67a101e81dc0ad", 70., [65.3, 0., 34.4, 47.6]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    Dm(Id),
    Channel(Id),
    Guild(Id),
}

pub(crate) struct QuickSwitcher {
    pub(super) search: Entity<Editor>,
    _events: Subscription,
    query: String,
    selected: usize,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// The SVG of an arrow turned by `degrees` about its centre (and mirrored for the right side, which the official
/// layout flips with `scaleX(-1)`), sized to the turned box.
fn rotated_svg(
    source: &str,
    degrees: f32,
    mirror: bool,
    width: f32,
    height: f32,
) -> Option<String> {
    let start = source.find("<svg")?;
    let open_end = start + source[start..].find('>')?;
    let inner = source[open_end + 1..].trim_end().strip_suffix("</svg>")?;
    let attributes = &source[start..open_end];
    let number = |name: &str| -> Option<f32> {
        let at = attributes.find(&format!(" {name}=\""))? + name.len() + 3;
        attributes[at..]
            .split('"')
            .next()?
            .trim_end_matches("px")
            .parse()
            .ok()
    };
    let (own_width, own_height) = (number("width")?, number("height")?);
    let view_box = attributes
        .find("viewBox=\"")
        .and_then(|at| attributes[at + 9..].split('"').next())
        .unwrap_or("")
        .to_owned();
    let view_box = if view_box.is_empty() {
        format!("0 0 {own_width} {own_height}")
    } else {
        view_box
    };
    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><g transform="translate({cx} {cy}) scale({flip} 1) rotate({degrees}) translate({ox} {oy})"><svg width="{own_width}" height="{own_height}" viewBox="{view_box}">{inner}</svg></g></svg>"#,
        flip = if mirror { -1 } else { 1 },
        cx = width / 2.,
        cy = height / 2.,
        ox = -own_width / 2.,
        oy = -own_height / 2.,
    ))
}

impl Root {
    pub(super) fn open_quick_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_switcher.is_some() {
            self.quick_switcher = None;
            cx.notify();
            return;
        }
        for (asset, _, _) in ARROWS {
            self.request_image(
                format!("text:qs-arrow-{asset}"),
                format!("https://discordapp.com/assets/{asset}.svg"),
            );
        }
        let search = cx.new(|cx| {
            // `input_a1DYes`: 22px text, centred in the 70px box.
            let style = crate::editor::EditorStyle {
                size: 22.,
                line_height: 28.,
                ..editor_style()
            };
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
            editor.set_placeholder("Where would you like to go?".to_owned());
            editor
        });
        let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    if let Some(switcher) = this.quick_switcher.as_mut() {
                        switcher.query = editor.read(cx).text().to_owned();
                        switcher.selected = 0;
                    }
                }
                EditorEvent::Submit(text) => {
                    editor.update(cx, |editor, _| editor.set_text(text));
                    this.choose_switcher_result(None, cx)
                }
                EditorEvent::Cancel => this.quick_switcher = None,
            }
            cx.notify();
        });
        window.focus(&search.read(cx).focus_handle(cx), cx);
        self.quick_switcher = Some(QuickSwitcher {
            search,
            _events: events,
            query: String::new(),
            selected: 0,
        });
        cx.notify();
    }

    /// Up and Down move the highlight; true when the key was taken.
    pub(super) fn switcher_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let count = self.switcher_results().len();
        let Some(switcher) = self.quick_switcher.as_mut() else {
            return false;
        };
        if count == 0 {
            return false;
        }
        match key {
            "up" => switcher.selected = (switcher.selected + count - 1) % count,
            "down" => switcher.selected = (switcher.selected + 1) % count,
            _ => return false,
        }
        cx.notify();
        true
    }

    fn destination_for_channel(&self, channel: Id) -> Option<Destination> {
        if self
            .stores
            .private_channels
            .iter()
            .any(|known| known.id == channel)
        {
            return Some(Destination::Dm(channel));
        }
        self.stores
            .guilds
            .iter()
            .flat_map(|guild| &guild.channels)
            .find(|known| known.id == channel)
            .filter(|known| matches!(known.kind, ChannelKind::Text | ChannelKind::Voice))
            .map(|_| Destination::Channel(channel))
    }

    pub(super) fn switcher_results(&self) -> Vec<Destination> {
        let Some(switcher) = self.quick_switcher.as_ref() else {
            return Vec::new();
        };
        let current = self.channel;
        let raw = switcher.query.trim();
        let mut out: Vec<Destination> = Vec::new();
        let push = |destination: Destination, out: &mut Vec<Destination>| {
            let id = match destination {
                Destination::Dm(id) | Destination::Channel(id) => Some(id),
                Destination::Guild(_) => None,
            };
            if (!raw.is_empty() || id.is_none() || id != current) && !out.contains(&destination) {
                out.push(destination);
            }
        };
        if raw.is_empty() {
            for channel in &self.channel_history {
                if let Some(destination) = self.destination_for_channel(*channel) {
                    push(destination, &mut out);
                }
            }
            let mut frecency = Frecency::new(Kind::Channel);
            let stored = self
                .fetched
                .get(FRECENCY_KEY)
                .and_then(|body| body["settings"].as_str())
                .map(|settings| {
                    dc_proto::frecency::id_map(
                        settings,
                        dc_proto::frecency::GUILD_AND_CHANNEL_FRECENCY,
                    )
                })
                .unwrap_or_default();
            frecency.overwrite(
                stored
                    .into_iter()
                    .map(|(key, item)| {
                        (
                            key.to_string(),
                            Usage {
                                total_uses: item.total_uses,
                                recent_uses: item.recent_uses,
                            },
                        )
                    })
                    .collect(),
            );
            for key in frecency.frequently(now_ms(), |key| {
                key.parse::<Id>()
                    .ok()
                    .and_then(|id| self.destination_for_channel(id))
                    .is_some()
            }) {
                if let Some(destination) = key
                    .parse::<Id>()
                    .ok()
                    .and_then(|id| self.destination_for_channel(id))
                {
                    push(destination, &mut out);
                }
            }
            out.truncate(MAX_RESULTS);
            return out;
        }
        // `@`, `#`, `!`, `*` narrow the search to one kind.
        let (mode, query) = match raw.chars().next() {
            Some(symbol @ ('@' | '#' | '!' | '*')) => (Some(symbol), raw[1..].trim()),
            _ => (None, raw),
        };
        let query = query.to_lowercase();
        let matches = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
        if mode.is_none() || mode == Some('@') {
            for dm in &self.stores.private_channels {
                if self.stores.dm_recipient(dm).is_some_and(|user| {
                    matched_user_name(user, self.nickname_for(user.id), &query).is_some()
                }) {
                    push(Destination::Dm(dm.id), &mut out);
                }
            }
        }
        for guild in &self.stores.guilds {
            for channel in &guild.channels {
                let wanted = matches!(
                    (mode, channel.kind),
                    (None | Some('#'), ChannelKind::Text) | (None | Some('!'), ChannelKind::Voice)
                );
                if wanted && matches(&channel.name) {
                    push(Destination::Channel(channel.id), &mut out);
                }
            }
        }
        if mode.is_none() || mode == Some('*') {
            for guild in &self.stores.guilds {
                if matches(&guild.name) {
                    push(Destination::Guild(guild.id), &mut out);
                }
            }
        }
        // Names that start with the query first, keeping the kinds' order otherwise.
        let starts = |destination: &Destination| -> bool {
            let name = match destination {
                Destination::Dm(id) => self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == *id)
                    .and_then(|dm| self.stores.dm_recipient(dm))
                    .and_then(|user| matched_user_name(user, self.nickname_for(user.id), &query))
                    .map(str::to_lowercase),
                Destination::Channel(id) => self
                    .stores
                    .guilds
                    .iter()
                    .flat_map(|guild| &guild.channels)
                    .find(|known| known.id == *id)
                    .map(|known| known.name.to_lowercase()),
                Destination::Guild(id) => self
                    .stores
                    .guild(*id)
                    .map(|guild| guild.name.to_lowercase()),
            };
            name.is_some_and(|name| name.starts_with(&query))
        };
        out.sort_by_key(|destination| !starts(destination));
        out.truncate(MAX_RESULTS * 2);
        out
    }

    fn choose_switcher_result(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let results = self.switcher_results();
        let Some(switcher) = self.quick_switcher.as_ref() else {
            return;
        };
        let Some(destination) = results.get(index.unwrap_or(switcher.selected)).copied() else {
            return;
        };
        self.quick_switcher = None;
        match destination {
            Destination::Dm(channel) => self.select_home(Some(channel)),
            Destination::Channel(channel) => {
                let guild = self
                    .stores
                    .guilds
                    .iter()
                    .find(|guild| guild.channels.iter().any(|known| known.id == channel))
                    .map(|guild| guild.id);
                let voice = self
                    .stores
                    .guilds
                    .iter()
                    .flat_map(|guild| &guild.channels)
                    .any(|known| known.id == channel && known.kind == ChannelKind::Voice);
                if let Some(guild) = guild {
                    if voice {
                        self.request_join_voice(guild, channel, cx);
                    } else {
                        self.select(guild, Some(channel));
                    }
                }
            }
            Destination::Guild(guild) => {
                let first = self.stores.guild(guild).and_then(|known| {
                    known
                        .sidebar_rows()
                        .iter()
                        .find(|channel| channel.kind == ChannelKind::Text)
                        .map(|channel| channel.id)
                });
                self.select(guild, first);
            }
        }
        cx.notify();
    }

    fn switcher_row(
        &self,
        index: usize,
        destination: Destination,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut unread = false;
        let (leading, name, username, note, misc): (
            AnyElement,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = match destination {
            Destination::Dm(channel) => {
                let user = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == channel)
                    .and_then(|dm| self.stores.dm_recipient(dm))
                    .cloned();
                let picture = user
                    .as_ref()
                    .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                unread = self.stores.read_states.mentions(channel) > 0;
                (
                    div()
                        .size(px(20.))
                        .rounded_full()
                        .children(picture.map(|picture| img(picture).size(px(20.)).rounded_full()))
                        .into_any_element(),
                    user.as_ref()
                        .map(|user| {
                            let query = self
                                .quick_switcher
                                .as_ref()
                                .map(|s| {
                                    s.query.trim().trim_start_matches('@').trim().to_lowercase()
                                })
                                .unwrap_or_default();
                            let nickname = self.nickname_for(user.id);
                            let matched = matched_user_name(user, nickname, &query);
                            // 320264 falls back to the relationship/display name when username matched.
                            match matched {
                                Some(name) if name != user.username => name.to_owned(),
                                _ => nickname.unwrap_or(user.display_name()).to_owned(),
                            }
                        })
                        .unwrap_or_default(),
                    user.map(|user| user.username),
                    None,
                    None,
                )
            }
            Destination::Channel(channel) => {
                let guild = self
                    .stores
                    .guilds
                    .iter()
                    .find(|guild| guild.channels.iter().any(|known| known.id == channel));
                let known =
                    guild.and_then(|guild| guild.channels.iter().find(|known| known.id == channel));
                let voice = known.is_some_and(|known| known.kind == ChannelKind::Voice);
                unread = !voice && self.stores.is_unread(channel);
                let category = known
                    .and_then(|known| known.parent_id)
                    .and_then(|parent| guild?.channels.iter().find(|known| known.id == parent))
                    .map(|category| category.name.clone());
                (
                    div()
                        .w(px(20.))
                        .flex()
                        .justify_center()
                        .child(icon(
                            if voice {
                                "voice_channel"
                            } else {
                                "text_channel"
                            },
                            16.,
                            "--interactive-text-default",
                        ))
                        .into_any_element(),
                    known.map(|known| known.name.clone()).unwrap_or_default(),
                    None,
                    if voice { None } else { category },
                    guild.map(|guild| guild.name.clone()),
                )
            }
            Destination::Guild(guild) => {
                let known = self.stores.guild(guild);
                let picture = self.images.get(&format!("guild:{guild}")).cloned();
                (
                    div()
                        .size(px(20.))
                        .rounded(px(6.))
                        .overflow_hidden()
                        .children(picture.map(|picture| img(picture).size(px(20.))))
                        .into_any_element(),
                    known.map(|known| known.name.clone()).unwrap_or_default(),
                    None,
                    None,
                    None,
                )
            }
        };
        div()
            .when(index == 0, |row| row.parity("switcher-first-result"))
            .id(SharedString::from(format!("switcher-row-{index}")))
            .h(px(34.))
            .px(px(10.))
            .rounded(px(3.))
            .cursor_pointer()
            .when(selected, |this| {
                this.bg(color("--interactive-background-selected"))
            })
            .flex()
            .items_center()
            .text_color(if unread {
                color("--text-strong")
            } else {
                color("--interactive-text-default")
            })
            .font_weight(FontWeight(500.))
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if *inside && let Some(switcher) = this.quick_switcher.as_mut() {
                    switcher.selected = index;
                    cx.notify();
                }
            }))
            .on_click(
                cx.listener(move |this, _, _, cx| this.choose_switcher_result(Some(index), cx)),
            )
            .child(leading)
            .child(
                div()
                    .ml(px(5.))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .line_height(px(34.))
                    .child(
                        div()
                            .text_color(color("--interactive-text-default"))
                            .when(unread, |this| this.text_color(color("--text-strong")))
                            .child(name),
                    )
                    .children(username.map(|username| {
                        div()
                            .font_weight(FontWeight(400.))
                            .opacity(0.6)
                            .child(username)
                    }))
                    .children(note.map(|note| {
                        div()
                            .mt(px(1.))
                            .text_size(px(10.))
                            .line_height(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child(note)
                    })),
            )
            .children(misc.map(|misc| {
                div()
                    .ml(px(4.))
                    .max_w(px(140.))
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .opacity(0.6)
                    .child(misc)
            }))
            .into_any_element()
    }

    fn arrow_image(
        &self,
        asset: &str,
        degrees: f32,
        mirror: bool,
        size: (f32, f32),
    ) -> Option<Arc<Image>> {
        let source = self.texts.get(&format!("text:qs-arrow-{asset}"))?;
        let svg = rotated_svg(source, degrees, mirror, size.0, size.1)?;
        Some(Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            svg.into_bytes(),
        )))
    }

    pub(super) fn quick_switcher_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let switcher = self.quick_switcher.as_ref()?;
        let results = self.switcher_results();
        let empty_query = switcher.query.trim().is_empty();
        let has_query_results = !empty_query && !results.is_empty();
        let viewport = window.viewport_size();
        let height = 398.42188_f32;
        let top = ((f32::from(viewport.height) - height) / 2.).max(8.);
        let left = ((f32::from(viewport.width) - 680.) / 2.).max(8.);
        let rows: Vec<AnyElement> = results
            .iter()
            .enumerate()
            .map(|(index, destination)| {
                self.switcher_row(index, *destination, index == switcher.selected, cx)
            })
            .collect();
        let symbol = |text: &'static str| {
            div()
                .mx(px(1.5))
                .p(px(2.))
                .rounded(px(4.))
                .bg(color("--background-base-lowest"))
                .text_size(px(12.))
                .line_height(px(16.))
                .child(text)
        };
        // The arrows: three on the left, mirrored on the right, 60px outside the box and 80px above it.
        let arrows: Vec<AnyElement> = ARROWS
            .iter()
            .filter(|(asset, _, _)| !has_query_results || *asset == ARROWS[0].0)
            .flat_map(|(asset, degrees, [x, y, width, height])| {
                [false, true].into_iter().filter_map(move |right| {
                    let picture = self.arrow_image(asset, *degrees, right, (*width, *height))?;
                    // The tutorial reaches 60px past the switcher box (1px inside the modal's border).
                    let base_x = if right {
                        left + 1. + 678. + 60. - x - width
                    } else {
                        left + 1. - 60. + x
                    };
                    Some(
                        div()
                            .absolute()
                            .left(px(base_x))
                            .top(px(top + 9. - 80.
                                + y
                                + if has_query_results {
                                    // 910839 moves the horizontal arrow nine pixels below the selected row's top.
                                    (69. + switcher.selected as f32 * 34.).min(250.)
                                } else {
                                    0.
                                }))
                            .child(img(picture).w(px(*width)).h(px(*height)))
                            .into_any_element(),
                    )
                })
            })
            .collect();
        let modal = div()
            .parity_with_border("quick-switcher", 1.)
            .id("quick-switcher")
            .occlude()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(680.))
            .rounded(px(12.))
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
            .pt(px(8.))
            .pb(px(16.))
            .child(
                div()
                    .pt(px(12.))
                    .px(px(20.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .parity_with_border("switcher-input", 1.)
                            .h(px(70.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .text_size(px(22.))
                            .child(div().flex_1().child(switcher.search.clone())),
                    )
                    .child(
                        div()
                            .id("switcher-results")
                            // `resultsArea_a1DYes` is 262 tall: 16 above a 236px scroller and 10 below it.
                            .h(px(236.))
                            .mr(px(-17.))
                            .pr(px(16.))
                            .mt(px(16.))
                            .mb(px(10.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .when(empty_query && !rows.is_empty(), |this| {
                                this.child(
                                    div()
                                        .h(px(34.))
                                        .px(px(10.))
                                        .pt(px(4.))
                                        .text_size(px(14.))
                                        .line_height(px(30.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--interactive-text-default"))
                                        .child("Previous Channels"),
                                )
                            })
                            .children(rows),
                    )
                    .child(
                        div()
                            .h(px(28.421875))
                            .pt(px(10.))
                            .when(!empty_query, |this| this.border_t_1().border_color(color("--border-subtle")))
                            .flex()
                            .items_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-strong"))
                            .child(
                                div()
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-feedback-positive"))
                                    .child("Protip:"),
                            )
                            .child(div().ml(px(3.)).child("Start searches with"))
                            .child(symbol("@"))
                            .child(symbol("#"))
                            .child(symbol("!"))
                            .child(symbol("*"))
                            .child(symbol("$"))
                            .child(div().ml(px(2.)).child("to narrow results."))
                            .child(
                                div()
                                    .id("switcher-learn-more")
                                    .ml(px(3.))
                                    .cursor_pointer()
                                    .text_color(color("--text-link"))
                                    .on_click(|_, _, cx| {
                                        cx.open_url("https://support.discord.com/hc/en-us/articles/115000070311")
                                    })
                                    .child("Learn more"),
                            ),
                    ),
            );
        Some(
            deferred(
                div()
                    .id("quick-switcher-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quick_switcher = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .right(px(0.))
                            .top(px(top + 9. - 80. + 40.))
                            .h(px(24.))
                            .flex()
                            .justify_center()
                            .text_size(px(18.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(if !has_query_results {
                                "Search for servers, channels or DMs"
                            } else {
                                "Select a result and press ENTER to jump to it"
                            }),
                    )
                    .children(arrows)
                    .child(modal),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{matched_user_name, rotated_svg};
    use dc_state::User;

    #[test]
    fn user_search_keeps_the_best_name_and_stable_ties() {
        let user = User {
            username: "an_alice".into(),
            global_name: Some("Alice Example".into()),
            ..Default::default()
        };
        assert_eq!(
            matched_user_name(&user, Some("Alice Friend"), "alice"),
            Some("Alice Friend")
        );
        assert_eq!(
            matched_user_name(&user, Some("Alice Friend"), "example"),
            Some("Alice Example")
        );
        assert_eq!(
            matched_user_name(&user, Some("an_alias"), "an_"),
            Some("an_alice")
        );
        assert_eq!(
            matched_user_name(&user, Some("Alice Friend"), "friend"),
            Some("Alice Friend")
        );
        assert_eq!(matched_user_name(&user, None, "friend"), None);
        assert_eq!(matched_user_name(&user, Some(""), "friend"), None);
    }

    #[test]
    fn rotates_inside_a_box_of_its_own() {
        let source = r#"<svg width="43" height="18" viewBox="0 0 43 18" xmlns="http://www.w3.org/2000/svg"><path d="M0 0h1"/></svg>"#;
        let out = rotated_svg(source, 40., false, 51.6, 48.5).unwrap();
        assert!(out.contains("rotate(40)"));
        assert!(out.contains(r#"<path d="M0 0h1"/>"#));
        assert!(out.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" width="51.6""#));
    }
}
