//! The gallery's data: which clips a view lists, how they group into sections and how the grid lays out (module 409067
//! `ad` for the facets and the sort, 657766 `tn` for the sections and the grid metrics, 995273 and 61491 for the
//! texts on a card).
use chrono::{Datelike, TimeZone};
use dc_clips::library::Clip;
use std::collections::BTreeMap;

/// What the sidebar selects: the main links and one game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Facet {
    /// `All Clips`.
    All,
    /// `Favorites`.
    Favorites,
    /// A game, by application id.
    Game(String),
}

/// A game of the sidebar: only clips with an application id make one, sorted by name (module 409067).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Game {
    pub id: String,
    pub name: String,
    pub count: usize,
}

/// The gallery's own state while it is open.
pub(in crate::ui) struct State {
    pub facet: Facet,
    /// The card menu: the clip it is for and where it was opened.
    pub menu: Option<(String, gpui::Point<gpui::Pixels>)>,
    pub scroll: gpui::ScrollHandle,
}

impl State {
    pub(in crate::ui) fn new(facet: Facet) -> Self {
        State {
            facet,
            menu: None,
            scroll: gpui::ScrollHandle::new(),
        }
    }
}

/// The clips of a facet, newest first (`MOST_RECENT`).
pub(super) fn visible<'a>(clips: &'a BTreeMap<String, Clip>, facet: &Facet) -> Vec<&'a Clip> {
    let mut list: Vec<&Clip> = clips
        .values()
        .filter(|clip| match facet {
            Facet::All => true,
            Facet::Favorites => clip.meta.is_favorite == Some(true),
            Facet::Game(id) => clip.meta.application_id.as_deref() == Some(id),
        })
        .collect();
    list.sort_by_key(|clip| std::cmp::Reverse(clip.meta.created_at));
    list
}

/// The games with clips, by name.
pub(super) fn games(clips: &BTreeMap<String, Clip>) -> Vec<Game> {
    let mut games: Vec<Game> = Vec::new();
    for clip in clips.values() {
        let Some(id) = clip.meta.application_id.as_ref() else {
            continue;
        };
        match games.iter_mut().find(|game| game.id == *id) {
            Some(game) => game.count += 1,
            None => games.push(Game {
                id: id.clone(),
                name: clip.meta.application_name.clone(),
                count: 1,
            }),
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// One section of the grid: `Today`, then `November 2026` and so on.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Section {
    pub title: String,
    /// The clips' positions in the list the sections were made from.
    pub start: usize,
    pub end: usize,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Groups a newest-first list by day (today) and month (`kB2R/0` "Today", else `en-US` long month and year).
pub(super) fn sections(clips: &[&Clip], now: chrono::DateTime<chrono::Local>) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for (index, clip) in clips.iter().enumerate() {
        let created = chrono::Local
            .timestamp_millis_opt(clip.meta.created_at as i64)
            .single()
            .unwrap_or(now);
        let title = if created.date_naive() == now.date_naive() {
            "Today".to_owned()
        } else {
            format!("{} {}", MONTHS[created.month0() as usize], created.year())
        };
        match out.last_mut() {
            Some(last) if last.title == title => last.end = index + 1,
            _ => out.push(Section {
                title,
                start: index,
                end: index + 1,
            }),
        }
    }
    out
}

/// Grid padding is 24 a side and the gap 20 (module 657766 `tn`).
const PAD: f32 = 24.;
const GAP: f32 = 20.;

/// Columns for a content width: `max(1, floor((w - 48 + 20) / 340))`.
pub(super) fn columns(width: f32) -> usize {
    (((width - 2. * PAD + GAP) / 340.).floor() as i64).max(1) as usize
}

/// A tile's width: `max(320, (w - 48 - 20 (cols - 1)) / cols)`.
pub(super) fn tile_width(width: f32, columns: usize) -> f32 {
    (320f32).max((width - 2. * PAD - GAP * (columns as f32 - 1.)) / columns as f32)
}

/// The tile's picture is 4:3.
pub(super) fn tile_height(tile: f32) -> f32 {
    (tile / (4. / 3.)).floor()
}

/// `m:ss` or `h:mm:ss` (module 61491 `rB`).
pub(super) fn duration_text(seconds: u64) -> String {
    let (hours, minutes, secs) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{}:{secs:02}", seconds / 60)
    }
}

/// The length a card shows: the edited length when it is shorter (module 657766 `e7`).
pub(super) fn shown_length_ms(clip: &Clip) -> u64 {
    let edited = clip.meta.edit_metadata.as_ref().and_then(|edit| {
        let start = edit.get("start")?.as_f64()?;
        let end = edit.get("end")?.as_f64()?;
        Some(((end - start) * 1000.).max(0.))
    });
    match edited {
        Some(ms) if (ms as u64) < clip.meta.length => ms as u64,
        _ => clip.meta.length,
    }
}

/// `{time} ago` with the abbreviated units (module 381849 `i` with strings `XIGt+W` ... `I1E8p6`).
pub(super) fn ago(created_ms: u64, now_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(created_ms) / 1000;
    let text = if seconds < 60 {
        "1m".to_owned()
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3600)
    } else if seconds < 2_592_000 {
        format!("{}d", seconds / 86_400)
    } else if seconds < 31_104_000 {
        format!("{}mo", seconds / 2_592_000)
    } else {
        format!("{}y", seconds / 31_104_000)
    };
    format!("{text} ago")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_clips::ClipMetadata;

    fn clip(id: &str, created: u64, app: Option<&str>, favourite: bool, length: u64) -> Clip {
        let mut meta =
            ClipMetadata::new_manual(id.to_owned(), created, "Game".to_owned(), Vec::new());
        meta.application_id = app.map(str::to_owned);
        meta.is_favorite = Some(favourite);
        meta.length = length;
        Clip {
            meta,
            filepath: id.into(),
            size_bytes: 0,
        }
    }

    fn library() -> BTreeMap<String, Clip> {
        let mut map = BTreeMap::new();
        for clip in [
            clip("a", 3_000, Some("1"), false, 5_000),
            clip("b", 2_000, Some("2"), true, 5_000),
            clip("c", 1_000, None, false, 5_000),
        ] {
            map.insert(clip.meta.id.clone(), clip);
        }
        map
    }

    #[test]
    fn views_filter_and_sort_newest_first() {
        let library = library();
        let ids = |facet| {
            visible(&library, &facet)
                .iter()
                .map(|clip| clip.meta.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(Facet::All), ["a", "b", "c"]);
        assert_eq!(ids(Facet::Favorites), ["b"]);
        assert_eq!(ids(Facet::Game("1".into())), ["a"]);
    }

    #[test]
    fn only_clips_with_an_application_id_make_a_game() {
        let found = games(&library());
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|game| game.count == 1));
    }

    #[test]
    fn the_grid_follows_the_official_metrics() {
        assert_eq!(columns(1127.), 3);
        assert_eq!(columns(300.), 1);
        assert!((tile_width(1127., 3) - 1039. / 3.).abs() < 0.01);
        assert_eq!(tile_width(300., 1), 320.);
        assert_eq!(tile_height(320.), 240.);
    }

    #[test]
    fn card_texts() {
        assert_eq!(duration_text(5), "0:05");
        assert_eq!(duration_text(75), "1:15");
        assert_eq!(duration_text(3725), "1:02:05");
        assert_eq!(ago(0, 30_000), "1m ago");
        assert_eq!(ago(0, 5 * 60_000), "5m ago");
        assert_eq!(ago(0, 3 * 3_600_000), "3h ago");
        assert_eq!(ago(0, 2 * 86_400_000), "2d ago");
    }

    #[test]
    fn an_edited_clip_shows_its_shorter_length() {
        let mut edited = clip("e", 0, None, false, 10_000);
        edited.meta.edit_metadata = Some(serde_json::json!({"start": 1.0, "end": 4.0}));
        assert_eq!(shown_length_ms(&edited), 3_000);
        edited.meta.edit_metadata = Some(serde_json::json!({"start": 0.0, "end": 10.0}));
        assert_eq!(shown_length_ms(&edited), 10_000);
    }
}
