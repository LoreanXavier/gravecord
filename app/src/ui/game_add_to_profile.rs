//! The game profile's "Add to Profile" button and menu (module 581745, `navId: "game-profile-add-to-profile"`): for a
//! game that may be shown on a profile (`XX` of module 735321: not adult-rated, not on the blocked list), one row per
//! game widget (`i`): Favorite / Games I Like / Games in Rotation / Want to Play, each "Remove from …" once the game
//! is in it, and "Max games reached" under a full widget, which is then disabled. A row adds the game at the front of
//! that widget (making the widget when the board has none) or takes it out, and saves the board (`eN`,
//! `savePendingWidgets`: `PUT /users/@me/widgets` with the widgets that are not empty). Not here yet: the game's
//! own stats widget row ("{name} Stats", from the featured widget configs).
use super::Root;
use super::context_menu::{Act, Entry, Row};
use super::profile_editor::save::{
    UserWidget, WidgetData, WidgetGame, parse_widgets, saveable_widgets, widgets_to_json,
};
use dc_state::Id;
use gpui::Context;

/// `i` of the menu: the widget, its add and remove labels, its icon.
const KINDS: [(&str, &str, &str, &str); 4] = [
    ("favorite_games", "Favorite", "Unfavorite", "heart"),
    (
        "played_games",
        "Games I Like",
        "Remove from Games I Like",
        "thumbs_up",
    ),
    (
        "current_games",
        "Games in Rotation",
        "Remove from Games in Rotation",
        "rotate",
    ),
    (
        "want_to_play_games",
        "Want to Play",
        "Remove from Want to Play",
        "joystick",
    ),
];

fn empty(kind: &str) -> WidgetData {
    match kind {
        "favorite_games" => WidgetData::FavoriteGames { games: Vec::new() },
        "played_games" => WidgetData::PlayedGames { games: Vec::new() },
        "current_games" => WidgetData::CurrentGames { games: Vec::new() },
        _ => WidgetData::WantToPlayGames { games: Vec::new() },
    }
}

/// `u(type, add)`: the board after adding the game to (or taking it out of) the widget of `kind`; `None` when nothing
/// changes (already there, full, or absent).
fn toggled(board: &[UserWidget], kind: &str, game: &str, add: bool) -> Option<Vec<UserWidget>> {
    let index = board.iter().position(|widget| widget.data.kind() == kind);
    let mut board = board.to_vec();
    if add {
        if let Some(index) = index {
            let widget = &mut board[index];
            let max = widget.data.max_games();
            let games = widget.data.games_mut()?;
            if games.iter().any(|existing| existing.game_id == game) || games.len() >= max {
                return None;
            }
            games.insert(
                0,
                WidgetGame {
                    game_id: game.to_owned(),
                    comment: None,
                    tags: Vec::new(),
                },
            );
        } else {
            let mut data = empty(kind);
            data.games_mut()?.push(WidgetGame {
                game_id: game.to_owned(),
                comment: None,
                tags: Vec::new(),
            });
            board.insert(0, UserWidget { id: None, data });
        }
    } else {
        let games = board.get_mut(index?)?.data.games_mut()?;
        games.retain(|existing| existing.game_id != game);
    }
    Some(board)
}

/// The rows: the label by whether the game is in the widget, and "Max games reached" on a full one it is not in.
fn rows(board: &[UserWidget], game: &str) -> Vec<(u8, String, &'static str, Option<&'static str>)> {
    KINDS
        .iter()
        .enumerate()
        .map(|(index, (kind, add, remove, icon))| {
            let widget = board.iter().find(|widget| widget.data.kind() == *kind);
            let games = widget
                .and_then(|widget| widget.data.games())
                .unwrap_or_default();
            let inside = games.iter().any(|existing| existing.game_id == game);
            let full =
                !inside && widget.is_some_and(|widget| games.len() >= widget.data.max_games());
            (
                index as u8,
                dc_intl::tr(if inside { remove } else { add }).to_string(),
                *icon,
                full.then_some("Max games reached"),
            )
        })
        .collect()
}

impl Root {
    fn my_board(&self) -> Option<Vec<UserWidget>> {
        let me = self.stores.current_user.as_ref()?.id;
        let body = self.fetched.get(&format!("profile:{me}"))?;
        Some(parse_widgets(body))
    }

    /// The account's own profile, for its widgets, asked for when a game profile opens.
    pub(super) fn game_add_to_profile_wanted(&mut self) {
        let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return;
        };
        let key = format!("profile:{me}");
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key,
                route: self.dm_profile_route(me),
            });
        }
    }

    /// `null != b`: the button shows when the menu has rows, which needs the account's profile and a game a profile
    /// may show.
    pub(super) fn game_add_to_profile_shown(&self, game: &str) -> bool {
        self.my_board().is_some() && self.game_profile_available(game)
    }

    pub(super) fn game_widget_items(&self, game: Id) -> Vec<Entry> {
        let board = self.my_board().unwrap_or_default();
        rows(&board, &game.to_string())
            .into_iter()
            .map(|(index, label, icon, sub)| {
                let mut row = Row::new(Act::GameWidget(index), label).icon(icon);
                if let Some(sub) = sub {
                    row = row.sub(dc_intl::tr(sub)).disabled(true);
                }
                Entry::Row(row)
            })
            .collect()
    }

    /// `u` then `eN`: the board changes at once and is saved.
    pub(super) fn toggle_game_widget(&mut self, game: Id, kind: u8, cx: &mut Context<Self>) {
        let Some((kind, ..)) = KINDS.get(kind as usize) else {
            return;
        };
        let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return;
        };
        let board = self.my_board().unwrap_or_default();
        let game = game.to_string();
        let inside = board
            .iter()
            .find(|widget| widget.data.kind() == *kind)
            .and_then(|widget| widget.data.games())
            .is_some_and(|games| games.iter().any(|existing| existing.game_id == game));
        let Some(next) = toggled(&board, kind, &game, !inside) else {
            return;
        };
        let saved = saveable_widgets(&next);
        if let Some(body) = self.fetched.get_mut(&format!("profile:{me}")) {
            body["widgets"] = serde_json::Value::Array(widgets_to_json(&next));
        }
        let _ = self.requests.send(crate::net::Request::Rest {
            key: None,
            method: reqwest::Method::PUT,
            route: "/users/@me/widgets".into(),
            body: Some(serde_json::json!({ "widgets": widgets_to_json(&saved) })),
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(kind: &str, games: &[&str]) -> Vec<UserWidget> {
        let mut data = empty(kind);
        for game in games {
            data.games_mut().unwrap().push(WidgetGame {
                game_id: (*game).to_owned(),
                comment: None,
                tags: Vec::new(),
            });
        }
        vec![UserWidget {
            id: Some("1".into()),
            data,
        }]
    }

    #[test]
    fn adding_puts_the_game_first_and_makes_the_widget_when_missing() {
        let next = toggled(&board("current_games", &["a"]), "current_games", "b", true).unwrap();
        assert_eq!(next[0].data.games().unwrap()[0].game_id, "b");
        let next = toggled(&[], "want_to_play_games", "b", true).unwrap();
        assert_eq!(next[0].data.kind(), "want_to_play_games");
        assert!(
            toggled(
                &board("favorite_games", &["a"]),
                "favorite_games",
                "b",
                true
            )
            .is_none(),
            "one favorite"
        );
        let next = toggled(
            &board("played_games", &["a", "b"]),
            "played_games",
            "a",
            false,
        )
        .unwrap();
        assert_eq!(next[0].data.games().unwrap().len(), 1);
    }

    #[test]
    fn rows_name_the_removal_and_the_full_widget() {
        let rows = rows(&board("favorite_games", &["a"]), "a");
        assert_eq!(rows[0].1, "Unfavorite");
        assert_eq!(rows[1].1, "Games I Like");
        let rows = super::rows(&board("favorite_games", &["a"]), "b");
        assert_eq!(rows[0].3, Some("Max games reached"));
    }
}
