//! Compact activity rows from modules 342312/566903 and the captured member DOM.

use super::{Root, color, icon};
use dc_state::{Id, User, presence::activity_text};
use gpui::{AnyElement, FontWeight, IntoElement, div, img, prelude::*, px};
use serde_json::Value;

fn emoji_source(emoji: &Value) -> Option<String> {
    if let Some(id) = emoji["id"].as_str() {
        Some(format!(
            "https://cdn.discordapp.com/emojis/{id}.webp?size=32"
        ))
    } else {
        dc_tokens::emoji_file(emoji["name"].as_str()?)
            .map(|file| format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN))
    }
}

/// Module 342312 `L` with module 834757 `nr`: a stream is of the streamer's game, their first `PLAYING` activity
/// that is not an embedded one (flag `EMBEDDED`, 256), and that activity then stays out of the entries after it.
/// Returns the game's name for the stream entry and the activity entries.
fn stream_entries<'a>(
    all: &'a [Value],
    visible: Vec<&'a Value>,
    stream: bool,
) -> (Option<&'a str>, Vec<&'a Value>) {
    if !stream {
        return (None, visible);
    }
    let game = all.iter().find(|activity| {
        activity["type"] == 0 && activity["flags"].as_u64().unwrap_or(0) & 256 == 0
    });
    let entries = visible
        .into_iter()
        .filter(|activity| !game.is_some_and(|game| activity["name"] == game["name"]))
        .collect();
    let name = game
        .and_then(|game| game["name"].as_str())
        .filter(|name| !name.is_empty());
    (name, entries)
}

/// `container_HigqCD` with `activityContainer_HigqCD` (`max-width: 100%; overflow: hidden`): the line's flex row. The
/// max width keeps it inside a parent that does not stretch it (a column with `items_start`), where it would otherwise
/// take its text's full width.
fn activity_row(compact: bool, line: f32) -> gpui::Div {
    div()
        .min_w(px(0.))
        .max_w_full()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(4.))
        .h(px(line))
        .text_size(px(if compact { 12. } else { 14. }))
        .line_height(px(line))
        .font_weight(FontWeight(500.))
}

/// `truncated_HigqCD`: `min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap`.
fn truncated(text: impl IntoElement) -> gpui::Div {
    div().min_w(px(0.)).truncate().child(text)
}

/// `subText_J1Adu_` (module 56606) under a list row's name: `margin-top: -2px; overflow: hidden; text-overflow:
/// ellipsis; white-space: nowrap`, cut at the width of the name column (the member list and DM rows).
pub(super) fn sub_text() -> gpui::Div {
    div()
        .mt(px(-2.))
        .max_w_full()
        .overflow_hidden()
        .whitespace_nowrap()
}

impl Root {
    /// The custom-status emoji of the one presence a `PRESENCE_UPDATE` carries. A busy server sends many of these a
    /// second, so they must not rescan every presence the store holds (`request_presence_images`).
    pub(super) fn request_presence_image_of(&mut self, data: &Value) {
        let url = data["activities"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|activity| activity["type"] == 4)
            .find_map(|activity| emoji_source(&activity["emoji"]));
        if let Some(url) = url {
            self.request_image(url.clone(), url);
        }
    }

    pub(super) fn request_presence_images(&mut self) {
        let urls: Vec<String> = self
            .stores
            .presence
            .all()
            .filter_map(|p| p.custom())
            .filter_map(|a| emoji_source(&a["emoji"]))
            .collect();
        for url in urls {
            self.request_image(url.clone(), url);
        }
    }

    pub(super) fn presence_line(
        &self,
        user: &User,
        guild: Option<Id>,
        compact: bool,
    ) -> Option<AnyElement> {
        self.presence_line_with(
            user,
            guild,
            compact,
            if compact { 16. } else { 18. },
            "--channels-default",
        )
    }

    /// Whether `presence_line` has a line for this person, without building it (the member list needs the row's
    /// height, which depends on it). The conditions are those `presence_line_with` returns `None` on.
    pub(super) fn has_presence_line(&self, user: &User, guild: Option<Id>) -> bool {
        if self.is_blocked(user.id) || self.stores.is_ignored(user.id) {
            return false;
        }
        let Some(presence) = self.stores.presence.get(user.id, guild) else {
            return false;
        };
        if matches!(presence.status.as_str(), "offline" | "invisible") {
            return false;
        }
        presence.custom().is_some()
            || !presence.visible_activities().is_empty()
            || self.voice_activity(user.id, guild).is_some()
    }

    /// Whether this person goes live in a voice channel of this server (`applicationStream` of module 342312, which
    /// the streaming store keeps from the voice states' `self_stream`).
    fn streams_in(&self, user: Id, guild: Option<Id>) -> bool {
        guild
            .and_then(|guild| self.stores.voice_states.get(&guild))
            .is_some_and(|states| {
                states
                    .iter()
                    .any(|state| state.user_id == user && state.self_stream)
            })
    }

    /// Module 342312 `F`: the line a person in a voice channel of this server gets after their activities, "In voice"
    /// (`msxteM`) or "In a stage" (`QygGCN`) for a Stage channel; `Some(true)` for the Stage.
    fn voice_activity(&self, user: Id, guild: Option<Id>) -> Option<bool> {
        let guild = guild?;
        let state = self
            .stores
            .voice_states
            .get(&guild)?
            .iter()
            .find(|state| state.user_id == user)?;
        let channel = self
            .stores
            .guild(guild)?
            .channels
            .iter()
            .find(|channel| channel.id == state.channel_id)?;
        Some(channel.kind == dc_state::ChannelKind::Other(13))
    }

    /// The same line at another line height and colour: the user panel draws it 13px high in its own text colour
    /// (`activityStatusText` under `panels_pog62O`).
    pub(super) fn presence_line_with(
        &self,
        user: &User,
        guild: Option<Id>,
        compact: bool,
        line: f32,
        tint: &'static str,
    ) -> Option<AnyElement> {
        self.presence_line_inner(user, guild, compact, line, tint, None, false)
    }

    /// The line where the pinned client draws module 342312's `ActivityStatus` (the member list and the DM list),
    /// which BetterActivities patches: its icons close the line, and `removeGameActivityStatus` drops the activity
    /// part (`plugin_hooks/better_activities.rs`). With the plugin off it is `presence_line_with`.
    pub(super) fn activity_status_line(
        &self,
        user: &User,
        guild: Option<Id>,
        line: f32,
        tint: &'static str,
        place: &'static str,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let (icons, remove_game) = self.better_activities_status(user, guild, place, tint, cx);
        self.presence_line_inner(user, guild, true, line, tint, icons, remove_game)
    }

    #[allow(clippy::too_many_arguments)]
    fn presence_line_inner(
        &self,
        user: &User,
        guild: Option<Id>,
        compact: bool,
        line: f32,
        tint: &'static str,
        icons: Option<AnyElement>,
        remove_game: bool,
    ) -> Option<AnyElement> {
        if self.is_blocked(user.id) || self.stores.is_ignored(user.id) {
            return None;
        }
        let presence = self.stores.presence.get(user.id, guild)?;
        if matches!(presence.status.as_str(), "offline" | "invisible") {
            return None;
        }
        let custom = presence.custom();
        let voice = self.voice_activity(user.id, guild);
        let stream = voice.is_some() && self.streams_in(user.id, guild);
        let (game_name, activities) =
            stream_entries(&presence.activities, presence.visible_activities(), stream);
        let first = activities.first().copied();
        // `Y`: the activities, plus one for the stream or else for the voice channel.
        let total = activities.len() + usize::from(voice.is_some());
        if custom.is_none() && total == 0 {
            return None;
        }
        let custom_text = custom.and_then(|a| a["state"].as_str());
        let hide_activity_text = compact && custom_text.is_some();
        let mut row = activity_row(compact, line).text_color(color(tint));
        if remove_game {
            // Not in the pinned client: BetterActivities' `removeGameActivityStatus`.
        } else if stream {
            // `R`: the TV glyph and the game's name, or "Sharing their screen" (`eXan7B`).
            if !user.bot {
                row = row.child(icon(
                    "activity_tv",
                    if compact { 12. } else { 14. },
                    "--status-positive",
                ));
            }
            if !hide_activity_text {
                row = row.child(truncated(game_name.map_or_else(
                    || dc_intl::tr("Sharing their screen").to_string(),
                    str::to_owned,
                )));
            }
        } else if let Some(activity) = first {
            let glyph = match activity["type"].as_u64() {
                Some(0 | 5) => Some("activity_game"),
                Some(2) => Some("activity_music"),
                Some(1 | 3) => Some("activity_tv"),
                _ => None,
            };
            if !user.bot
                && let Some(glyph) = glyph
            {
                row = row.child(icon(
                    glyph,
                    if compact { 12. } else { 14. },
                    "--status-positive",
                ));
            }
            if !hide_activity_text && let Some(text) = activity_text(activity) {
                row = row.child(truncated(text));
            }
        }
        // Only when nothing else leads: the voice entry is the last of the list and the first shown otherwise.
        if !remove_game
            && !stream
            && first.is_none()
            && let Some(stage) = voice
        {
            row = row
                .child(icon(
                    "voice_status_in_voice",
                    if compact { 12. } else { 14. },
                    "--status-positive",
                ))
                .when(!hide_activity_text, |this| {
                    this.child(truncated(if stage { "In a stage" } else { "In voice" }))
                });
        }
        // `$`: "+N" for the entries after the first one, in the positive status colour beside a custom status text.
        if !remove_game && !user.bot && total > 1 {
            row = row.child(
                div()
                    .flex_none()
                    .when(custom_text.is_some(), |this| {
                        this.text_color(color("--status-positive"))
                    })
                    .child(format!("+{}", total - 1)),
            );
        }
        if let Some(custom) = custom {
            if total > 0 && !remove_game {
                row = row.child(div().flex_none().child("•"));
            }
            if let Some(url) = emoji_source(&custom["emoji"]) {
                if let Some(image) = self.images.get(&url) {
                    row = row.child(
                        img(image.clone())
                            .size(px(if compact { 14. } else { 18. }))
                            .flex_none(),
                    );
                }
            } else if let Some(emoji) = custom["emoji"]["name"].as_str() {
                row = row.child(div().flex_none().child(emoji.to_owned()));
            }
            if let Some(text) = custom_text {
                row = row.child(truncated(text.to_owned()));
            }
        }
        // Not in the pinned client: BetterActivities' icons close the line.
        Some(row.children(icons).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::stream_entries;
    use serde_json::json;

    /// gpui's `text_ellipsis` only sets `text-overflow`: without `white-space: nowrap` (or a line clamp) the text
    /// wraps instead of being cut, as a CSS `text-overflow: ellipsis` without `nowrap` would. Every builder chain that
    /// asks for an ellipsis says which of the two it means (`truncate`, `whitespace_nowrap` or `line_clamp`). The
    /// chain is read from the element's constructor to its first child.
    #[test]
    fn every_ellipsis_is_single_line_or_clamped() {
        fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    rust_files(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }
        let starts = |line: &str| {
            line.contains("div()")
                || line.contains("text(")
                || line.contains("line()")
                || line.contains("=>")
                || line.trim_end().ends_with('|')
        };
        let ends = |line: &str| {
            line.contains(".child(") || line.contains(".children(") || line.contains("into_any")
        };
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let mut missing = Vec::new();
        for path in files {
            let shown = path
                .strip_prefix(&src)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // The built-in plugins keep their own rules (`PLUGINS.md`).
            if shown.starts_with("plugins") || shown.starts_with("ui/plugin_hooks") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                // Spelt in two halves so this test does not find itself.
                if !line.contains(concat!(".text_", "ellipsis"))
                    || line.trim_start().starts_with("//")
                {
                    continue;
                }
                let first = (0..=index).rev().find(|&i| starts(lines[i])).unwrap_or(0);
                let last = (index..lines.len())
                    .find(|&i| ends(lines[i]))
                    .unwrap_or(index);
                let chain = lines[first..=last].join("\n");
                if !["whitespace_nowrap", ".truncate()", "line_clamp"]
                    .iter()
                    .any(|mark| chain.contains(mark))
                {
                    missing.push(format!("{shown}:{}", index + 1));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "text_ellipsis without whitespace_nowrap, truncate or line_clamp: {missing:?}"
        );
    }

    #[test]
    fn a_stream_leads_with_its_game_and_drops_it_from_the_rest() {
        let all = vec![
            json!({"type": 0, "name": "Activity", "flags": 256}),
            json!({"type": 0, "name": "Deadlock"}),
            json!({"type": 2, "name": "Spotify"}),
        ];
        let visible: Vec<_> = all.iter().collect();
        let (game, rest) = stream_entries(&all, visible.clone(), true);
        assert_eq!(game, Some("Deadlock"));
        assert_eq!(
            rest.iter()
                .map(|a| a["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["Activity", "Spotify"]
        );
        let (game, rest) = stream_entries(&all, visible, false);
        assert_eq!(game, None);
        assert_eq!(rest.len(), 3);
        let none = vec![json!({"type": 2, "name": "Spotify"})];
        let (game, rest) = stream_entries(&none, none.iter().collect(), true);
        assert_eq!(game, None, "no game: the entry reads Sharing their screen");
        assert_eq!(rest.len(), 1);
    }
}

#[cfg(all(test, feature = "capture"))]
mod layout_tests {
    use super::{activity_row, sub_text, truncated};
    use gpui::{
        Bounds, Context, IntoElement, Pixels, Render, Window, canvas, div, prelude::*, px, size,
    };
    use std::{cell::Cell, rc::Rc};

    type Probe = Rc<Cell<Option<Bounds<Pixels>>>>;

    fn probe(slot: Probe) -> impl IntoElement {
        canvas(move |bounds, _, _| slot.set(Some(bounds)), |_, _, _, _| {})
            .absolute()
            .inset_0()
    }

    /// The member list's item as `ui.rs` lays it out: 248px wide, 42px with a status line, the avatar, then the name
    /// column (at most 188px, `items_start`) with the name and `sub_text` holding the status line.
    struct MemberRow {
        status: String,
        item: Probe,
        column: Probe,
        text: Probe,
    }

    impl Render for MemberRow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(
                div()
                    .relative()
                    .h(px(42.))
                    .w(px(248.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .child(probe(self.item.clone()))
                    .child(div().flex_none().size(px(32.)))
                    .child(
                        div()
                            .relative()
                            .ml(px(12.))
                            .min_w(px(0.))
                            .max_w(px(188.))
                            .flex()
                            .flex_col()
                            .items_start()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .min_w(px(0.))
                                    .max_w_full()
                                    .h(px(20.))
                                    .child(truncated("lorean xavier")),
                            )
                            .child(
                                sub_text().child(
                                    activity_row(true, 16.)
                                        .child(div().flex_none().child("\u{1f319}"))
                                        .child(
                                            truncated(self.status.clone())
                                                .relative()
                                                .child(probe(self.text.clone())),
                                        ),
                                ),
                            )
                            .child(probe(self.column.clone())),
                    ),
            )
        }
    }

    #[gpui::test]
    fn a_long_custom_status_stays_one_line_inside_the_member_row(cx: &mut gpui::TestAppContext) {
        let status = concat!(
            "Bugün çok yoğun bir gün geçirdim, şimdi kahve içip biraz dinleniyorum \u{2615} ",
            "— hoy estoy trabajando en un proyecto muy largo y complicado con mis amigos de España ",
            "\u{1f3ae} ğüşıöç ĞÜŞİÖÇ ñáéíóú ¿Qué tal? ¡Hasta luego! and more words past two hundred characters",
        );
        assert!(status.chars().count() > 200);
        let (item, column, text) = (Probe::default(), Probe::default(), Probe::default());
        let row = MemberRow {
            status: status.to_owned(),
            item: item.clone(),
            column: column.clone(),
            text: text.clone(),
        };
        cx.open_window(size(px(600.), px(200.)), |_, _| row);
        cx.run_until_parked();
        let (item, column, text) = (
            item.get().expect("item laid out"),
            column.get().expect("column laid out"),
            text.get().expect("status laid out"),
        );
        assert_eq!(item.size.height, px(42.), "the row keeps its height");
        assert_eq!(text.size.height, px(16.), "the status is one line");
        assert!(text.size.width > px(0.));
        assert!(column.size.width <= px(188.));
        assert!(
            text.right() <= column.right(),
            "the status ends inside the name column: {text:?} in {column:?}"
        );
        assert!(
            text.bottom() <= item.bottom(),
            "the status stays in its row"
        );
    }
}
