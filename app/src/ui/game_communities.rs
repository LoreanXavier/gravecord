//! The game profile's Communities tab (module 581745 `tH`): the game's community servers as cards, from
//! `GET /discoverable-guilds?guild_ids=…` (`te`, `DISCOVERABLE_GUILDS_FETCH_SUCCESS`, `jE` of module 449054), in a grid
//! (`grid_zGFyZa`: columns of at least 280px, 12px apart) or a list (`list_zGFyZa`), with the "View as" switch
//! (`tT`). While the answer is out, eight skeleton cards (`tU`/`tB`). A card opens the server as a preview, as the
//! Discover cards do (`eQ.Z2`, a lurk join), and closes the profile. Not here yet: the guild badge before the name
//! (`tr.A`), the languages filter (`tP`, shown with two or more preferred locales) and the skeletons' shimmer.
use super::{Root, color};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};
use serde_json::Value;

pub(super) const KEY_PREFIX: &str = "game-communities:";
/// `grid-template-columns: repeat(auto-fill, minmax(280px, 1fr))`.
const MIN_COLUMN: f32 = 280.;
/// `--space-12` between cards.
const GAP: f32 = 12.;

/// `tA`: how the cards are laid out (`tC`, grid by default, kept for the session).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Layout {
    #[default]
    Grid,
    List,
}

/// One community server as `jE` reads it.
#[derive(Clone, Debug, PartialEq)]
struct Community {
    id: Id,
    name: String,
    icon: Option<String>,
    description: String,
    presence: Option<u64>,
    members: Option<u64>,
}

/// A REST answer arrives as `{ok, status, body}`; the guilds are in `body`.
fn communities_of(answer: &Value) -> Vec<Community> {
    answer["body"]["guilds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|guild| {
            Some(Community {
                id: guild["id"].as_str()?.parse().ok()?,
                name: guild["name"].as_str().unwrap_or_default().to_owned(),
                icon: guild["icon"].as_str().map(str::to_owned),
                description: guild["description"].as_str().unwrap_or_default().to_owned(),
                presence: guild["approximate_presence_count"].as_u64(),
                members: guild["approximate_member_count"].as_u64(),
            })
        })
        .collect()
}

/// `e6.getGuilds(ids)`: the answers in the game's order, servers the service did not return left out.
fn in_game_order(ids: &[Id], found: Vec<Community>) -> Vec<Community> {
    ids.iter()
        .filter_map(|id| found.iter().find(|c| c.id == *id).cloned())
        .collect()
}

/// The route `GUILD_DISCOVERY` with `guild_ids` repeated, as the API client sends an array.
fn route(ids: &[Id]) -> String {
    let query: Vec<String> = ids.iter().map(|id| format!("guild_ids={id}")).collect();
    format!("/discoverable-guilds?{}", query.join("&"))
}

/// Columns of the grid at `width`: as many 280px columns with 12px gaps as fit, at least one.
fn columns(width: f32) -> usize {
    (((width + GAP) / (MIN_COLUMN + GAP)).floor() as usize).max(1)
}

fn icon_url(community: &Community) -> Option<String> {
    community.icon.as_ref().map(|icon| {
        let ext = if icon.starts_with("a_") {
            "gif"
        } else {
            "webp"
        };
        format!(
            "https://cdn.discordapp.com/icons/{}/{icon}.{ext}?size=48",
            community.id
        )
    })
}

/// `LC+S+m` and `zRl6XR` with the thousands grouped as en-US does.
fn grouped(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl Root {
    fn community_ids(&self, game: &str) -> Vec<Id> {
        self.fetched
            .get(&format!("{}{game}", super::game_profile::KEY_PREFIX))
            .and_then(|body| {
                body.as_array()
                    .and_then(|games| games.first())
                    .map(|record| record["community_guild_ids"].clone())
            })
            .and_then(|ids| {
                ids.as_array().map(|ids| {
                    ids.iter()
                        .filter_map(|id| id.as_str()?.parse().ok())
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    /// `te.load`: asks once per profile for the game's community servers.
    pub(super) fn game_communities_fetch(&mut self, game: &str) {
        let ids = self.community_ids(game);
        let key = format!("{KEY_PREFIX}{game}");
        if ids.is_empty() || self.fetched.contains_key(&key) || !self.requested.insert(key.clone())
        {
            return;
        }
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some(key),
            method: reqwest::Method::GET,
            route: route(&ids),
            body: None,
        });
    }

    /// The answer: kept, and the icons asked for.
    pub(super) fn game_communities_fetched(&mut self, key: &str, body: Value) -> bool {
        if !key.starts_with(KEY_PREFIX) {
            return false;
        }
        tracing::debug!(
            status = body["status"].as_u64(),
            failed = body["fetch_failed"].as_bool(),
            guilds = communities_of(&body).len(),
            "game communities answer"
        );
        for community in communities_of(&body) {
            if let Some(url) = icon_url(&community) {
                self.request_image(url.clone(), url);
            }
        }
        self.fetched.insert(key.to_owned(), body);
        true
    }

    fn game_community_card(
        &self,
        community: &Community,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let icon = icon_url(community).and_then(|url| self.images.get(&url).cloned());
        let id = community.id;
        let icon_box = div()
            .flex_none()
            .size(px(48.))
            .rounded(px(8.))
            .border_4()
            .border_color(color("--background-surface-high"))
            .bg(color("--background-mod-normal"))
            .overflow_hidden()
            .children(icon.map(|icon| img(icon).size_full()));
        let name = div()
            .min_w(px(0.))
            .truncate()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-default"))
            .child(community.name.clone());
        let description = (!community.description.is_empty()).then(|| {
            div()
                .line_clamp(2)
                .text_ellipsis()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(community.description.clone())
        });
        let count = |tint: &'static str, text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().size(px(8.)).rounded_full().bg(color(tint)))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-muted"))
                        .child(text),
                )
        };
        // `th`: online then total, 16px apart.
        let counts = div()
            .flex()
            .items_center()
            .gap(px(16.))
            .children(
                community
                    .presence
                    .map(|n| count("--icon-feedback-positive", format!("{} Online", grouped(n)))),
            )
            .children(community.members.map(|n| {
                count(
                    "--text-default",
                    format!(
                        "{} {}",
                        grouped(n),
                        if n == 1 { "Member" } else { "Members" }
                    ),
                )
            }));
        let card = div()
            .id(("game-community", id as usize))
            .cursor_pointer()
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(12.))
            .hover(|style| style.bg(color("--background-surface-higher")))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.game_profile = None;
                this.servers_join(id, cx);
            }));
        match layout {
            // `tY` / `tj`: the icon and the text side by side, 16px in.
            Layout::Grid => card
                .p(px(16.))
                .flex()
                .gap(px(16.))
                .child(icon_box)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(name)
                        .children(description)
                        .child(counts),
                )
                .into_any_element(),
            // `tW` (`container_ySYCnn`): 8px block, 8/16px inline, the counts at the end.
            Layout::List => card
                .py(px(8.))
                .pl(px(8.))
                .pr(px(16.))
                .flex()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(icon_box)
                        .child(
                            div()
                                .min_w(px(0.))
                                .max_w(px(540.))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(name)
                                .children(description),
                        ),
                )
                .child(div().ml_auto().child(counts))
                .into_any_element(),
        }
    }

    fn game_community_skeleton(layout: Layout) -> AnyElement {
        let bar = |w: gpui::DefiniteLength, h: f32| {
            div()
                .w(w)
                .h(px(h))
                .rounded(px(4.))
                .bg(color("--background-mod-normal"))
        };
        let frame = div()
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(12.));
        match layout {
            Layout::Grid => frame
                .p(px(16.))
                .flex()
                .gap(px(16.))
                .child(
                    div()
                        .flex_none()
                        .size(px(48.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-normal")),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(bar(gpui::relative(0.6), 20.))
                        .child(bar(gpui::relative(0.9), 12.))
                        .child(bar(gpui::relative(0.55), 12.)),
                )
                .into_any_element(),
            Layout::List => frame
                .py(px(8.))
                .pl(px(8.))
                .pr(px(16.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex_none()
                        .size(px(48.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-normal")),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(bar(gpui::relative(0.6), 20.))
                        .child(bar(gpui::relative(0.9), 12.)),
                )
                .child(bar(px(160.).into(), 12.))
                .into_any_element(),
        }
    }

    fn game_communities_layout_button(
        &self,
        which: Layout,
        current: Layout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (path, id) = match which {
            Layout::Grid => ("icons/grid_square.svg", "game-communities-grid"),
            Layout::List => ("icons/list_bullets.svg", "game-communities-list"),
        };
        let pressed = which == current;
        div()
            .id(id)
            .size(px(32.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(pressed, |this| this.bg(color("--background-mod-strong")))
            .hover(|style| style.bg(color("--background-mod-normal")))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.game_communities_layout = which;
                cx.notify();
            }))
            .child(
                gpui::svg()
                    .path(path)
                    .size(px(16.))
                    .text_color(color(if pressed {
                        "--text-strong"
                    } else {
                        "--icon-subtle"
                    })),
            )
            .into_any_element()
    }

    /// `tH`'s section: "Browse Similar Communities", the view switch and the cards, at `width`.
    pub(super) fn game_communities_section(
        &self,
        game: &str,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = self.game_communities_layout;
        let ids = self.community_ids(game);
        let answer = self.fetched.get(&format!("{KEY_PREFIX}{game}"));
        let cards: Vec<AnyElement> = match answer {
            None => (0..8)
                .map(|_| Self::game_community_skeleton(layout))
                .collect(),
            Some(body) => in_game_order(&ids, communities_of(body))
                .iter()
                .map(|community| self.game_community_card(community, layout, cx))
                .collect(),
        };
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .min_h(px(32.))
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Browse Similar Communities")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lowest"))
                    .child(self.game_communities_layout_button(Layout::Grid, layout, cx))
                    .child(self.game_communities_layout_button(Layout::List, layout, cx)),
            );
        let inner = width - 32.;
        let body = match layout {
            Layout::List => div()
                .flex()
                .flex_col()
                .gap(px(GAP))
                .children(cards)
                .into_any_element(),
            Layout::Grid => {
                let n = columns(inner);
                let column = (inner - GAP * (n as f32 - 1.)) / n as f32;
                let mut rows = Vec::new();
                let mut cards = cards.into_iter().peekable();
                while cards.peek().is_some() {
                    let row: Vec<AnyElement> = cards
                        .by_ref()
                        .take(n)
                        .map(|card| {
                            div()
                                .w(px(column))
                                .flex_none()
                                .child(card)
                                .into_any_element()
                        })
                        .collect();
                    rows.push(div().flex().gap(px(GAP)).children(row));
                }
                div()
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .children(rows)
                    .into_any_element()
            }
        };
        div()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(header)
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_answer_is_read_as_je_does_and_kept_in_the_games_order() {
        let body = json!({"ok": true, "status": 200, "body": {"guilds": [
            {"id": "2", "name": "B", "icon": "a_x", "description": "d", "approximate_presence_count": 5,
             "approximate_member_count": 1200},
            {"id": "1", "name": "A", "icon": null},
        ]}});
        let found = in_game_order(&[1, 3, 2], communities_of(&body));
        assert_eq!(found.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(found[1].members, Some(1200));
        assert_eq!(
            icon_url(&found[1]).as_deref(),
            Some("https://cdn.discordapp.com/icons/2/a_x.gif?size=48")
        );
        assert_eq!(icon_url(&found[0]), None);
    }

    #[test]
    fn route_columns_and_counts() {
        assert_eq!(
            route(&[1, 2]),
            "/discoverable-guilds?guild_ids=1&guild_ids=2"
        );
        assert_eq!(columns(1166.), 4);
        assert_eq!(columns(200.), 1);
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(grouped(999), "999");
    }
}
