//! The game profile's "Similar Games" (module 581745 `ik`/`iC`): the games of
//! `GET /content-inventory/users/@me/similar-games/{id}` (`$M` of module 880405, without the game itself and the ones
//! on module 277450's list), as 3:4 covers eight to a page (`scroller_lfwvtm`, `--custom-similar-games-per-page: 8`,
//! 16px apart) in the page-step scroller of module 151869. A cover shows the game's name on hover and opens that
//! game's profile when one opens (`useShouldOpenGameProfile`). Not here yet: the eight skeletons while the answer is
//! out, the cover's zoom on hover, the drop shadow (`--shadow-low-filter`) and the blurred backdrop behind
//! an icon that stands in for a missing cover.
use super::carousel::Row;
use super::{Root, color};
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};
use serde_json::Value;

pub(super) const KEY_PREFIX: &str = "game-similar:";
/// Module 277450 `b`.
const LEFT_OUT: [&str; 4] = [
    "700136079562375258",
    "1402418693958275202",
    "1402418696126992445",
    "1417993715611467826",
];
const PER_PAGE: f32 = 8.;
const GAP: f32 = 16.;
const CLIENT: f32 = 766.66;

fn card_width() -> f32 {
    (CLIENT - (PER_PAGE - 1.) * GAP) / PER_PAGE
}

/// `similar_games` without the game itself and the left-out ones.
fn similar_ids(game: &str, answer: &Value) -> Vec<String> {
    answer["body"]["similar_games"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().map(str::to_owned))
        .filter(|id| id != game && !LEFT_OUT.contains(&id.as_str()))
        .collect()
}

impl Root {
    /// `$M`, once per game; the similar games' records come after.
    pub(super) fn game_similar_fetch(&mut self, game: &str) {
        let key = format!("{KEY_PREFIX}{game}");
        if game.parse::<u64>().is_err()
            || self.fetched.contains_key(&key)
            || !self.requested.insert(key.clone())
        {
            return;
        }
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some(key),
            method: reqwest::Method::GET,
            route: format!("/content-inventory/users/@me/similar-games/{game}"),
            body: None,
        });
    }

    pub(super) fn game_similar_fetched(&mut self, key: &str, body: Value) -> bool {
        let Some(game) = key.strip_prefix(KEY_PREFIX) else {
            return false;
        };
        let ids = similar_ids(game, &body);
        tracing::debug!(
            status = body["status"].as_u64(),
            games = ids.len(),
            "similar games answer"
        );
        for id in &ids {
            self.request_game_record(id);
        }
        self.fetched.insert(key.to_owned(), body);
        true
    }

    /// The 256px cover of a similar game (`getCoverURL(256)`), once its record is in.
    pub(super) fn game_similar_cover(&self, id: &str) -> Option<String> {
        let record = self
            .fetched
            .get(&format!("{}{id}", super::game_profile::KEY_PREFIX))?;
        let media = &record.as_array()?.first()?["media"];
        super::game_profile::asset(id, &media["cover"], 256)
    }

    /// A similar game's record, once it is in.
    fn game_similar_record(&self, id: &str) -> Option<Value> {
        self.fetched
            .get(&format!("{}{id}", super::game_profile::KEY_PREFIX))?
            .as_array()?
            .first()
            .cloned()
    }

    /// `x` of module 459746 when there is no cover: the icon (`getIconURL(256)`) at 85% over the low background, or
    /// with no icon either `j`: the name (three lines of `body-xs/medium`) and the release year (`footnote/normal`) on
    /// `#423d80`, both in `--text-overlay-light`.
    fn game_similar_fallback(&self, id: &str, name: &str, width: f32) -> AnyElement {
        let record = self.game_similar_record(id);
        let icon = record
            .as_ref()
            .and_then(|record| super::game_profile::asset(id, &record["media"]["icon"], 256))
            .and_then(|url| self.images.get(&url).cloned());
        if let Some(icon) = icon {
            return div()
                .size_full()
                .bg(color("--background-base-low"))
                .flex()
                .items_center()
                .justify_center()
                .child(img(icon).size(px(width * 0.85)).rounded(px(8.)))
                .into_any_element();
        }
        let year = record
            .as_ref()
            .and_then(|record| record["first_release_date"].as_str().map(str::to_owned))
            .and_then(|date| date.get(..4).map(|year| format!("({year})")));
        div()
            .size_full()
            .p(px(12.))
            .bg(gpui::rgb(0x423d80))
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .text_color(color("--text-overlay-light"))
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .line_clamp(3)
                    .child(name.to_owned()),
            )
            .child(div().flex_1())
            .children(year.map(|year| div().text_size(px(10.)).line_height(px(12.)).child(year)))
            .into_any_element()
    }

    /// `ik`: nothing until the answer has games.
    pub(super) fn game_similar_section(
        &self,
        game: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let answer = self.fetched.get(&format!("{KEY_PREFIX}{game}"))?;
        let ids = similar_ids(game, answer);
        if ids.is_empty() {
            return None;
        }
        let width = card_width();
        let row = Row {
            card: width,
            gap: GAP,
            client: CLIENT,
            count: ids.len(),
        };
        let scroll = self.game_similar_scroll.clamp(0., row.max_scroll());
        let cards: Vec<AnyElement> = ids
            .iter()
            .map(|id| {
                let picture = self
                    .game_similar_cover(id)
                    .and_then(|url| self.images.get(&url).cloned());
                let name = self.game_profile_name(id);
                let open = id.clone();
                let card = div()
                    .id(gpui::SharedString::from(format!("similar-game-{id}")))
                    .flex_none()
                    .w(px(width))
                    .h(px(width * 4. / 3.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(color("--background-mod-muted"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.game_profile_available(&open) {
                            this.open_game_profile(open.clone(), cx);
                        }
                    }))
                    .child(match picture {
                        Some(picture) => img(picture).size_full().into_any_element(),
                        None => self.game_similar_fallback(id, &name, width),
                    });
                // `J.m`: the game's name over the cover.
                self.with_tip(
                    format!("similar-game-tip-{id}"),
                    super::tooltip::Tip::body(
                        super::tooltip::Body::Text(name.into()),
                        super::tooltip::Side::Top,
                    ),
                    card,
                    cx,
                )
                .flex_none()
                .into_any_element()
            })
            .collect();
        let arrow = |left: bool, enabled: bool, cx: &mut Context<Self>| {
            div()
                .id(if left {
                    "similar-games-left"
                } else {
                    "similar-games-right"
                })
                .size(px(40.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--control-overlay-secondary-border-default"))
                .bg(gpui::rgba(0x0000008f))
                .flex()
                .items_center()
                .justify_center()
                .when(!enabled, |this| this.opacity(0.))
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.game_similar_scroll =
                                row.page_step(left, this.game_similar_scroll);
                            cx.notify();
                        }))
                })
                .child(super::icon(
                    if left {
                        "arrow_large_left"
                    } else {
                        "arrow_large_right"
                    },
                    20.,
                    "--white",
                ))
        };
        let shows_actions = row.max_scroll() > 0.5;
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div().min_h(px(32.)).flex().items_center().child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr("Similar Games")),
                    ),
                )
                .child(
                    div()
                        .relative()
                        .mx(px(-4.))
                        .my(px(-4.))
                        .p(px(4.))
                        .overflow_hidden()
                        .child(
                            div()
                                .id("similar-games")
                                .w(px(CLIENT))
                                .overflow_hidden()
                                .child(div().flex().gap(px(GAP)).ml(px(-scroll)).children(cards)),
                        )
                        .when(shows_actions, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .p(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(arrow(true, scroll > 0., cx))
                                    .child(arrow(false, scroll < row.max_scroll() - 1., cx)),
                            )
                        }),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eight_covers_fill_the_column_and_the_list_leaves_out_the_game_itself() {
        assert!((card_width() - 81.83).abs() < 0.01);
        let answer =
            json!({"ok": true, "body": {"similar_games": ["1", "2", "700136079562375258", "3"]}});
        assert_eq!(
            similar_ids("2", &answer),
            vec!["1".to_owned(), "3".to_owned()]
        );
    }
}
