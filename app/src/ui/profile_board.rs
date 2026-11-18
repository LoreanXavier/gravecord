//! The Board's widgets as a visitor sees them (module 720060 `nR`).
//!
//! Every widget sits in the card of module 40989 (`container_FNEkcu`: 12px top, 16px sides and bottom, `--radius-lg`,
//! `--user-profile-overlay-background`, a 1px `--border-muted` edge, 8px between header and body). Its header
//! (`headerRow_ltNcM4`) is the title in `heading-xs/medium` `text-default` and, on someone else's profile, the 24px
//! Widget Actions button (`button_rOupvJ`, `MoreHorizontalIcon` `sm`, `--icon-subtle`, `--icon-strong` on hover) whose
//! menu has Report Widget.
//!
//! Favorite Game and Games in Rotation list game detail cards (module 132085, `cardList_JPOF3d`, 12px apart; Games in
//! Rotation shows two, then Show more); Games I Like and Want to Play are cover grids (module 793693, eight, then Show
//! more). Both use `gameWidgetGrid_--V09L`: three or four 1fr columns by the card's content width, the cover one column
//! at 3:4 (`gameCover_R0hw0B`), the details the remaining columns, centred against the cover. A detail card shows the
//! game's name, the owner's comment behind a quote icon (`body-sm/normal` `text-muted`) and the tags (module 229087:
//! `tag_E5LDT0`, the tag's icon at 12px before its `footnote/medium` `text-subtle` label).

use super::Root;
use super::dm_profile::Palette;
use super::profile_editor::save::WidgetGame;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, Window, div, img, prelude::*, px, svg};

/// The content box of a Board card in the full profile: the 432px tab column (`tabPanelScroller_y4ZEM5`, 448 wide
/// less its 8px right padding, `design/dom/modals/full-profile-darker-cozy.json`) less the card's 16px sides and
/// 1px edges.
pub(super) const FULL_PROFILE_CARD_CONTENT: f32 = 432. - 2. * 16. - 2.;

/// Games shown before Show more: Games in Rotation's cards (`nl`, 2) and a cover grid (`nj`, 8).
pub(super) const CARD_LIST_LIMIT: usize = 2;
pub(super) const COVER_GRID_LIMIT: usize = 8;

/// `gameWidgetGrid_--V09L` for a container `width` wide: the column count and the gap (`--space-*`), by its container
/// queries at 192, 266, 324 and 360px.
pub(super) fn game_grid(width: f32) -> (usize, f32) {
    let columns = if width >= 324. { 4 } else { 3 };
    let gap = if width >= 360. {
        16.
    } else if width >= 324. {
        12.
    } else if width >= 266. {
        16.
    } else if width >= 192. {
        12.
    } else {
        8.
    };
    (columns, gap)
}

/// One grid column of a container `width` wide.
pub(super) fn grid_column(width: f32) -> f32 {
    let (columns, gap) = game_grid(width);
    (width - gap * (columns as f32 - 1.)) / columns as f32
}

/// The game cover's size (`gameCover_R0hw0B`, `aspect-ratio: 3/4`, one column).
pub(super) fn cover_size(width: f32) -> (f32, f32) {
    let column = grid_column(width);
    (column, column * 4. / 3.)
}

/// The icon a tag shows (`K_` of module 126601).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TagIcon {
    Ribbon,
    ThumbsUp,
    ThumbsDown,
    Friends,
}

impl TagIcon {
    fn path(self) -> &'static str {
        match self {
            Self::Ribbon => "icons/tag_ribbon.svg",
            Self::ThumbsUp => "icons/tag_thumbs_up.svg",
            Self::ThumbsDown => "icons/tag_thumbs_down.svg",
            Self::Friends => "icons/tag_friends.svg",
        }
    }
}

/// `o` of module 126601: a game tag's label and icon. A tag outside this table (`skilled`, `master`, `noob`, ...) is
/// not drawn (`W` of module 394245 answers null and `_` of module 132085 filters it out).
pub(super) fn game_tag(id: &str) -> Option<(&'static str, TagIcon)> {
    Some(match id {
        "better_than_you" => (dc_intl::t!("jbIRBE"), TagIcon::Ribbon),
        "casual" => (dc_intl::t!("xcFFv6"), TagIcon::Ribbon),
        "intermediate" => (dc_intl::t!("A/mIs/"), TagIcon::Ribbon),
        "expert" => (dc_intl::t!("RIOFc2"), TagIcon::Ribbon),
        "obsessed" => (dc_intl::t!("isPJDu"), TagIcon::ThumbsUp),
        "love_it" => (dc_intl::t!("1rN7BF"), TagIcon::ThumbsUp),
        "kind_of_love_it" => (dc_intl::t!("bCBpVg"), TagIcon::ThumbsUp),
        "kind_of_hate_it" => (dc_intl::t!("/WcmcP"), TagIcon::ThumbsDown),
        "rage_quitting" => (dc_intl::t!("NXZ/MZ"), TagIcon::ThumbsDown),
        "open_to_play" => (dc_intl::t!("q30PoH"), TagIcon::Friends),
        "looking_for_group" => (dc_intl::t!("DWWAAQ"), TagIcon::Friends),
        "looking_for_tips" => (dc_intl::t!("KQDVvH"), TagIcon::Friends),
        "open_to_teach" => (dc_intl::t!("5HhQo+"), TagIcon::Friends),
        "looking_to_discuss" => (dc_intl::t!("GipOCq"), TagIcon::Friends),
        _ => return None,
    })
}

/// The tags of a game that are drawn, in the owner's order.
pub(super) fn drawn_tags(game: &WidgetGame) -> Vec<(&'static str, TagIcon)> {
    game.tags.iter().filter_map(|tag| game_tag(tag)).collect()
}

/// `commentIcon_2y0Mhw` floats at the start of the comment (`float: inline-start`, 3px down, 4px after it): the first
/// line runs beside the 12px quote and every later line starts under it at the left edge. Splits `text` into that
/// first line (whole words that fit `first` px, as the browser breaks it) and the rest, which wraps at full width.
pub(super) fn float_first_line(
    text: &str,
    first: f32,
    mut measure: impl FnMut(&str) -> f32,
) -> (String, String) {
    let mut end = 0;
    for (at, _) in text
        .char_indices()
        .filter(|(_, c)| c.is_whitespace())
        .chain(std::iter::once((text.len(), ' ')))
    {
        if measure(text[..at].trim_end()) <= first {
            end = at;
        } else {
            break;
        }
    }
    if end == 0 {
        // Not even the first word fits beside the quote: the line goes below it, all at full width.
        return (String::new(), text.to_owned());
    }
    (
        text[..end].trim_end().to_owned(),
        text[end..].trim_start().to_owned(),
    )
}

/// A tag pill's width for a label `text` px wide: 1px edges, 6px before the 12px icon, 4px, the label, 8px after.
pub(super) fn tag_width(text: f32) -> f32 {
    1. + 6. + 12. + 4. + text + 8. + 1.
}

/// `G` of module 132085 as the card mounts for a visitor: the tags that fit one 296px row, then those that fit a second
/// row less its 4px (the expand button is not drawn yet and there is no add button); the rest fold into `+N`.
pub(super) fn shown_tags(widths: &[f32]) -> usize {
    const ROW: f32 = 296.;
    let mut shown = 0;
    let mut used = 0.;
    for width in widths {
        used += width + 4.;
        if used > ROW {
            break;
        }
        shown += 1;
    }
    used = 0.;
    for width in &widths[shown..] {
        used += width + 4.;
        if used > ROW - 4. {
            break;
        }
        shown += 1;
    }
    shown
}

/// The image keys of a widget game: its cover, then its icon (both asked for by `request_profile_cards`).
pub(super) fn cover_key(game: &str) -> String {
    format!("profile-game:cover:{game}")
}

pub(super) fn icon_key(game: &str) -> String {
    format!("profile-game:{game}")
}

/// The private profile notice (module 501193 in `Overlay` of module 946356): `overlay_P1YEjQ` (the profile's overlay
/// colour, `--radius-sm`, a `--border-muted` edge on a light profile) padded 12px (`banner_98tF3N`) around
/// `body-sm/normal` `P8ij6Z` with the shown name. Drawn by the full profile (927071), the DM panel (289287) and the
/// popout (348434) when the profile answer says `private`.
pub(super) fn private_notice(name: &str, palette: &Palette) -> AnyElement {
    let light = palette.theme.as_ref().is_some_and(|theme| theme.light());
    div()
        .flex_none()
        .flex()
        .flex_col()
        .p(px(12.))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(palette.token("--user-profile-overlay-background"))
        .when(light, |this| {
            this.border_1()
                .border_color(palette.token("--border-muted"))
        })
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(palette.token("--text-default"))
        .child(private_text(name))
        .into_any_element()
}

/// `P8ij6Z` with `username`.
pub(super) fn private_text(name: &str) -> String {
    dc_intl::t!("P8ij6Z", username = name).to_string()
}

impl Root {
    /// The card around one widget (`ee` of module 40989) with its header (`V`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn board_widget_frame(
        &self,
        user: Id,
        index: usize,
        widget_id: Option<&str>,
        title: gpui::SharedString,
        visitor: bool,
        embedded: bool,
        body: AnyElement,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let subtle = palette.token("--icon-subtle");
        let strong = palette.token("--icon-strong");
        let anchor = format!("board-widget-more-{index}");
        let group = format!("board-widget-more-group-{index}");
        let widget_id = widget_id.map(str::to_owned);
        let more = visitor.then(|| {
            let anchor_key = anchor.clone();
            div()
                .id(gpui::SharedString::from(anchor.clone()))
                .group(group.clone())
                .relative()
                .flex_none()
                .size(px(24.))
                .mr(px(-4.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(
                    svg()
                        .path("icons/more_horizontal.svg")
                        .size(px(18.))
                        .text_color(subtle)
                        .group_hover(group.clone(), move |style| style.text_color(strong)),
                )
                .child(self.anchor_probe(anchor.clone()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let bounds = this.tip_anchors.borrow().get(anchor_key.as_str()).copied();
                    if let Some(bounds) = bounds {
                        this.open_context_menu(
                            super::context_menu::Target::ProfileWidget {
                                user,
                                widget: widget_id
                                    .as_deref()
                                    .and_then(|id| id.parse().ok())
                                    .unwrap_or(0),
                            },
                            gpui::point(bounds.right() + px(8.), bounds.top()),
                            cx,
                        );
                    }
                }))
        });
        if embedded {
            // `widgetContainer_zEJODa` (an application widget): no padding, the content edge to edge and the menu in
            // `containerHeader_zEJODa`, 12px down and 16px in over it.
            return div()
                .relative()
                .rounded(px(16.))
                .border_1()
                .border_color(palette.token("--border-muted"))
                .bg(palette.token("--user-profile-overlay-background"))
                .child(body)
                .child(
                    div()
                        .absolute()
                        .top(px(12.))
                        .left(px(16.))
                        .right(px(16.))
                        .flex()
                        .justify_end()
                        .children(more),
                )
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(12.))
            .px(px(16.))
            .pb(px(16.))
            .rounded(px(16.))
            .border_1()
            .border_color(palette.token("--border-muted"))
            .bg(palette.token("--user-profile-overlay-background"))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        div().flex().flex_col().justify_center().child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(palette.token("--text-default"))
                                .child(title),
                        ),
                    )
                    .children(more),
            )
            .child(body)
            .into_any_element()
    }

    /// `R` of module 788593 with `FQ` of module 459746 (size COMPACT): the cover filling its 3:4 box, else the game's
    /// icon, else its name; `--radius-sm`, the cover's `0 3px 12px rgba(0,0,0,.16)` shadow.
    pub(super) fn board_game_cover(
        &self,
        game: &str,
        name: &str,
        width: f32,
        palette: &Palette,
    ) -> AnyElement {
        let height = width * 4. / 3.;
        let cover = self.images.get(&cover_key(game)).cloned();
        let icon = self.images.get(&icon_key(game)).cloned();
        let shadow = gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.16),
            offset: gpui::point(px(0.), px(3.)),
            blur_radius: px(12.),
            spread_radius: px(0.),
            inset: false,
        };
        let frame = div()
            .flex_none()
            .w(px(width))
            .h(px(height))
            .rounded(px(8.))
            .overflow_hidden();
        match (cover, icon) {
            (Some(cover), _) => frame
                .shadow(vec![shadow])
                .child(
                    img(cover)
                        .w(px(width))
                        .h(px(height))
                        .object_fit(gpui::ObjectFit::Cover),
                )
                .into_any_element(),
            (None, Some(icon)) => frame
                .shadow(vec![shadow])
                .bg(palette.tile_background(false))
                .flex()
                .items_center()
                .justify_center()
                .child(img(icon).size(px(width / 2.)))
                .into_any_element(),
            (None, None) => frame
                .bg(palette.token("--background-mod-muted"))
                .p(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .text_center()
                .text_size(px(10.))
                .line_height(px(12.))
                .font_weight(FontWeight(500.))
                .text_color(palette.token("--text-default"))
                .line_clamp(3)
                .child(name.to_owned())
                .into_any_element(),
        }
    }

    /// `z` of module 132085 for a visitor: the cover in the first grid column, then the name, comment and tags.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn board_game_card(
        &self,
        widget_key: &str,
        game: &WidgetGame,
        name: String,
        content_width: f32,
        palette: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (cover_width, _) = cover_size(content_width);
        let (_, gap) = game_grid(content_width);
        // `details_2y0Mhw` takes the card's width after the cover column; the quote (12px and 4px after it) floats at
        // the start of the comment's first line.
        let details_width = content_width - cover_width - gap;
        let comment = game
            .comment
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| {
                float_first_line(text, details_width - 16., |part| {
                    super::threads::measure_text(window, part, 14., 400.)
                })
            });
        let tags = drawn_tags(game);
        let widths: Vec<f32> = tags
            .iter()
            .map(|(label, _)| tag_width(super::threads::measure_text(window, label, 10., 500.)))
            .collect();
        let tags_key = format!("tags:{widget_key}:{}", game.game_id);
        let tags_open = self
            .full_profile
            .as_ref()
            .is_some_and(|open| open.board_is_expanded(&tags_key));
        let hidden = tags.len() - shown_tags(&widths);
        let tags: Vec<(&'static str, TagIcon)> = if tags_open || hidden == 0 {
            tags
        } else {
            tags.into_iter().take(widths.len() - hidden).collect()
        };
        let fold = (hidden > 0).then(|| {
            let strong = palette.token("--text-strong");
            let hover_bg = palette.token("--background-mod-normal");
            let toggle = tags_key.clone();
            div()
                .id(gpui::SharedString::from(format!("board-{tags_key}")))
                .flex()
                .items_center()
                .rounded(px(8.))
                .border_1()
                .border_color(palette.token("--border-subtle"))
                .text_color(palette.token("--text-subtle"))
                .cursor_pointer()
                .hover(move |style| style.bg(hover_bg).text_color(strong))
                .map(|this| {
                    if tags_open {
                        // `collapseButton_tqx5FD`: 3px 4px around a 12px left chevron in `--text-subtle`.
                        this.py(px(3.)).px(px(4.)).child(
                            svg()
                                .path("icons/chevron_small_left.svg")
                                .size(px(12.))
                                .text_color(palette.token("--text-subtle")),
                        )
                    } else {
                        this.py(px(3.))
                            .pl(px(6.))
                            .pr(px(8.))
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .font_weight(FontWeight(500.))
                            .child(format!("+{hidden}"))
                    }
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(open) = this.full_profile.as_mut() {
                        open.toggle_board_expanded(&toggle);
                    }
                    cx.notify();
                }))
        });
        let border = palette.token("--border-subtle");
        let tag_icon = palette.token("--interactive-icon-default");
        let tag_text = palette.token("--text-subtle");
        let muted_icon = palette.token("--icon-muted");
        let muted_text = palette.token("--text-muted");
        div()
            .flex()
            .items_start()
            .gap(px(gap))
            .child(self.board_game_cover(&game.game_id, &name, cover_width, palette))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .self_stretch()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(palette.token("--text-default"))
                            .child(name),
                    )
                    .when_some(comment, |this, (first, rest)| {
                        let line = || {
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(muted_text)
                        };
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .child(
                                            svg()
                                                .path("icons/widget_quote.svg")
                                                .flex_none()
                                                .size(px(12.))
                                                .mt(px(3.))
                                                .mr(px(4.))
                                                .text_color(muted_icon),
                                        )
                                        .when(!first.is_empty(), |row| {
                                            row.child(line().whitespace_nowrap().child(first))
                                        }),
                                )
                                .when(!rest.is_empty(), |column| {
                                    column.child(line().min_w(px(0.)).child(rest))
                                }),
                        )
                    })
                    .when(!tags.is_empty(), |this| {
                        this.child(
                            div().mt(px(4.)).flex().flex_wrap().gap(px(4.)).children(
                                tags.into_iter()
                                    .map(|(label, icon)| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .py(px(3.))
                                            .pl(px(6.))
                                            .pr(px(8.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(border)
                                            .child(
                                                svg()
                                                    .path(icon.path())
                                                    .flex_none()
                                                    .size(px(12.))
                                                    .text_color(tag_icon),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(10.))
                                                    .line_height(px(12.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(tag_text)
                                                    .child(label),
                                            )
                                            .into_any_element()
                                    })
                                    .chain(fold.map(IntoElement::into_any_element))
                                    .collect::<Vec<AnyElement>>(),
                            ),
                        )
                    }),
            )
            .into_any_element()
    }

    /// `A` of module 720060: Show more / Show less in `body-sm/medium` `text-subtle` (`button_zNz-f1`, strong and
    /// underlined on hover).
    pub(super) fn board_show_more(
        &self,
        key: String,
        expanded: bool,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strong = palette.token("--text-strong");
        let toggle = key.clone();
        div()
            .id(gpui::SharedString::from(format!("board-show-more-{key}")))
            .self_start()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(palette.token("--text-subtle"))
            .hover(move |style| style.text_color(strong).underline())
            .child(if expanded {
                dc_intl::t!("6MwJo/")
            } else {
                dc_intl::t!("lBeKY2")
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(open) = this.full_profile.as_mut() {
                    open.toggle_board_expanded(&toggle);
                }
                cx.notify();
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FULL_PROFILE_CARD_CONTENT, TagIcon, cover_size, drawn_tags, float_first_line, game_grid,
        game_tag, grid_column,
    };
    use crate::ui::profile_editor::save::WidgetGame;

    #[test]
    fn a_comment_runs_beside_the_quote_for_one_line_then_under_it() {
        // Ten pixels a character: 60px beside the quote hold "first words" no further than "first ".
        let measure = |text: &str| text.chars().count() as f32 * 10.;
        assert_eq!(
            float_first_line("first words then the rest", 110., measure),
            ("first words".to_owned(), "then the rest".to_owned())
        );
        assert_eq!(
            float_first_line("short", 110., measure),
            ("short".to_owned(), String::new())
        );
        // Not even one word fits beside it: everything goes under the quote.
        assert_eq!(
            float_first_line("unbreakable words", 50., measure),
            (String::new(), "unbreakable words".to_owned())
        );
    }

    #[test]
    fn the_full_profile_card_lays_games_on_four_columns_sixteen_apart() {
        assert_eq!(FULL_PROFILE_CARD_CONTENT, 398.);
        assert_eq!(game_grid(FULL_PROFILE_CARD_CONTENT), (4, 16.));
        let (width, height) = cover_size(FULL_PROFILE_CARD_CONTENT);
        assert!((width - 87.5).abs() < 1e-4);
        assert!((height - 116.6667).abs() < 1e-3);
    }

    #[test]
    fn tags_fold_into_plus_n_after_two_rows() {
        assert_eq!(super::tag_width(40.), 72.);
        // Three 92px pills fill a row (3 x 96 = 288), a fourth starts the second row.
        assert_eq!(super::shown_tags(&[92.; 3]), 3);
        assert_eq!(super::shown_tags(&[92.; 6]), 6);
        // The second row keeps 4px back: three more 92px pills (288) still fit 292, a seventh does not.
        assert_eq!(super::shown_tags(&[92.; 7]), 6);
        assert_eq!(super::shown_tags(&[]), 0);
        // A wide first pill leaves the rest of the first row unused, as the measuring loop does.
        assert_eq!(super::shown_tags(&[200., 100., 60.]), 3);
        assert_eq!(super::shown_tags(&[200., 100., 100., 100., 100.]), 3);
    }

    #[test]
    fn the_private_notice_names_the_profile_in_the_official_sentence() {
        let text = super::private_text("Ana");
        assert!(text.starts_with("Ana"), "{text}");
        assert!(text.contains("private"), "{text}");
    }

    #[test]
    fn the_grid_follows_every_container_query() {
        assert_eq!(game_grid(180.), (3, 8.));
        assert_eq!(game_grid(192.), (3, 12.));
        assert_eq!(game_grid(266.), (3, 16.));
        assert_eq!(game_grid(324.), (4, 12.));
        assert_eq!(game_grid(359.), (4, 12.));
        assert_eq!(game_grid(360.), (4, 16.));
        assert!((grid_column(300.) - (300. - 32.) / 3.).abs() < 1e-4);
    }

    #[test]
    fn every_official_tag_has_its_label_and_icon_and_unknown_ones_are_dropped() {
        let ribbon = ["better_than_you", "casual", "intermediate", "expert"];
        for tag in ribbon {
            assert_eq!(game_tag(tag).map(|(_, icon)| icon), Some(TagIcon::Ribbon));
        }
        for tag in ["obsessed", "love_it", "kind_of_love_it"] {
            assert_eq!(game_tag(tag).map(|(_, icon)| icon), Some(TagIcon::ThumbsUp));
        }
        for tag in ["kind_of_hate_it", "rage_quitting"] {
            assert_eq!(
                game_tag(tag).map(|(_, icon)| icon),
                Some(TagIcon::ThumbsDown)
            );
        }
        for tag in [
            "open_to_play",
            "looking_for_group",
            "looking_for_tips",
            "open_to_teach",
            "looking_to_discuss",
        ] {
            assert_eq!(game_tag(tag).map(|(_, icon)| icon), Some(TagIcon::Friends));
        }
        assert_eq!(
            game_tag("better_than_you").map(|(label, _)| label),
            Some("Better than you")
        );
        assert_eq!(game_tag("expert").map(|(label, _)| label), Some("Expert"));
        assert_eq!(
            game_tag("obsessed").map(|(label, _)| label),
            Some("Obsessed")
        );
        for tag in ["master", "skilled", "noob", "lfg", "made_up"] {
            assert!(game_tag(tag).is_none(), "{tag}");
        }
        let game = WidgetGame {
            game_id: "1".into(),
            comment: None,
            tags: vec!["better_than_you".into(), "master".into(), "obsessed".into()],
        };
        let drawn: Vec<&str> = drawn_tags(&game)
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(drawn, ["Better than you", "Obsessed"]);
    }
}
