//! The pager under the search results (module 252715's `eg.A`, module 876689, over the paginator of module 662203)
//! and the limits of module 477654: at most 10,000 results are paged (`paginationTotalCount`), 400 pages, and from
//! page 400 on "We can only return up to 400 pages of results." stands over the results (`E+2azY`, a warning inline
//! notice, `paginationLimitHeader_QHVbp3`); page 400's button carries the same text as its tooltip. The pager shows
//! when more than 25 results are counted, in `paginationDock_QHVbp3` (1px `--border-subtle` top border,
//! `--shadow-medium`, 16px side padding). The look comes from `design/dom/home/shop-browse-darker-cozy.json`, the same
//! paginator under the Shop: 28px round page buttons (radius 14, padding 6, 4px margins, 16px semibold
//! `--text-strong`, the selected one `--brand-500` with white text), 40px Previous/Next buttons (radius 8, 8px and
//! 12px side padding, 16px chevrons 4px from the text, half opacity when disabled) and the 28px `…` gap
//! (`heading-xs/semibold` 14/18 in `--text-default`, 8px/4px margins). A gap opens a 50px page number field
//! (`jumpToPageInlineInput_0MrDiE`); Enter jumps to a page between 1 and the last, leaving it closes it.

use super::tooltip::{Body, Side, Tip};
use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use gpui::{AnyElement, Context, FontWeight, IntoElement, Window, div, prelude::*, px};

/// `pageSize` of the search pager (and `limit` of each search page).
pub(super) const PAGE_SIZE: u64 = 25;
/// `1e4` of module 477654: the most results the pager counts.
const MAX_TOTAL: u64 = 10_000;
/// `paginationMaxVisiblePage`.
const MAX_PAGE: u64 = 400;
/// `maxVisiblePages` of module 876689.
const MAX_VISIBLE_PAGES: u64 = 5;
/// `--custom-paginator-round-button-size`.
const ROUND: f32 = 28.;

/// What module 477654's hook answers for a search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Limits {
    /// `paginationTotalCount`: the results the pager counts, at most 10,000.
    pub total_count: u64,
    /// `isPaginationTotalCountLimited`: more than 10,000 results.
    pub limited: bool,
    /// `isMaxVisiblePageWarningVisible`: the limit holds and the shown page is 400 (index 399) or later.
    pub warning: bool,
}

/// `o` of module 477654: `total` is the count to page (the last answer's while the same query and sort search again).
pub(super) fn limits(total: u64, offset: u64) -> Limits {
    let limited = total > MAX_TOTAL;
    Limits {
        total_count: total.min(MAX_TOTAL),
        limited,
        warning: limited && offset / PAGE_SIZE >= MAX_PAGE - 1,
    }
}

/// One entry of `getPageList` (module 662203).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Item {
    Back { disabled: bool },
    Page { page: u64, selected: bool },
    Gap { left: bool },
    Next { disabled: bool },
}

/// `getNeighborBounds` and `getPageList` of module 662203 (`hideMaxPage` false): Previous, page 1 and a gap when
/// the window of `visible` pages leaves it out, the window, a gap and the last page, Next.
pub(super) fn page_list(total_pages: u64, selected: u64, visible: u64) -> Vec<Item> {
    let up = visible.div_ceil(2);
    let down = visible / 2;
    let (low, high) = if selected <= up {
        (1, visible as i64)
    } else if selected as i64 > total_pages as i64 - down as i64 {
        (total_pages as i64 - visible as i64 + 1, total_pages as i64)
    } else {
        (selected as i64 - up as i64 + 1, (selected + down) as i64)
    };
    let (mut low, mut high) = (low.max(1), high.min(total_pages as i64));
    let mut items = vec![Item::Back {
        disabled: selected == 1,
    }];
    let mut right = Vec::new();
    if low > 1 {
        items.push(Item::Page {
            page: 1,
            selected: false,
        });
        items.push(Item::Gap { left: true });
        low += 2;
    }
    if high < total_pages as i64 {
        right.push(Item::Gap { left: false });
        right.push(Item::Page {
            page: total_pages,
            selected: false,
        });
        high -= 2;
    }
    items.extend((low..=high).map(|page| Item::Page {
        page: page as u64,
        selected: page as u64 == selected,
    }));
    items.extend(right);
    items.push(Item::Next {
        disabled: selected == total_pages,
    });
    items
}

/// The page whose number field is open, and the field.
pub(super) struct Jump {
    pub left: bool,
    pub field: gpui::Entity<Editor>,
    _events: gpui::Subscription,
    _blur: gpui::Subscription,
}

impl Root {
    /// The count the pager pages: the answer's, or while the same query and sort search again the one before
    /// (`searchResultsPaginationKey`); none while a new search runs.
    pub(super) fn search_pager_total(&self, answered: Option<u64>) -> u64 {
        let Some(search) = self.search.as_ref() else {
            return 0;
        };
        answered.or(search.carried_total).unwrap_or(0)
    }

    /// `onPageChange`: the same query again at `25 * index` (`fetchMessages`, module 252715 `N`).
    fn search_page(&mut self, page: u64, cx: &mut Context<Self>) {
        let Some(query) = self.search.as_ref().and_then(|search| search.shown.clone()) else {
            return;
        };
        if let Some(search) = self.search.as_mut() {
            search.jump = None;
        }
        self.run_search_at(query, (page.max(1) - 1) * PAGE_SIZE, cx);
    }

    /// Capture only: `search page <n>` turns to a page as the pager's buttons do.
    #[cfg(feature = "capture")]
    pub(crate) fn script_search_page(&mut self, page: u64, cx: &mut Context<Self>) -> String {
        self.search_page(page, cx);
        let offset = self.search.as_ref().map_or(0, |search| search.offset);
        format!("search: page {page} (offset {offset})")
    }

    fn open_page_jump(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut style = editor_style();
            style.size = 16.;
            style.line_height = 20.;
            Editor::new(style, cx)
        });
        let events = cx.subscribe(&field, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    let pages = this.search_total_pages();
                    // `P`: a whole number between 1 and the last page.
                    match text.trim().parse::<u64>() {
                        Ok(page) if (1..=pages).contains(&page) => this.search_page(page, cx),
                        _ => {}
                    }
                }
                EditorEvent::Cancel => {
                    if let Some(search) = this.search.as_mut() {
                        search.jump = None;
                    }
                }
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        let handle = gpui::Focusable::focus_handle(field.read(cx), cx);
        window.focus(&handle, cx);
        // `onBlur`: the field closes and forgets what was typed.
        let blur = cx.on_focus_out(&handle, window, |this, _, _, cx| {
            if let Some(search) = this.search.as_mut() {
                search.jump = None;
            }
            cx.notify();
        });
        if let Some(search) = self.search.as_mut() {
            search.jump = Some(Jump {
                left,
                field,
                _events: events,
                _blur: blur,
            });
        }
        cx.notify();
    }

    fn search_total_pages(&self) -> u64 {
        let answered = self
            .fetched
            .get(super::search::RESULTS_KEY)
            .and_then(super::search::total_of);
        limits(self.search_pager_total(answered), 0)
            .total_count
            .div_ceil(PAGE_SIZE)
    }

    /// "We can only return up to 400 pages of results." over the results (`d.w` type warning, 16px under it).
    pub(super) fn search_page_warning(&self) -> AnyElement {
        let text = dc_intl::format("E+2azY", &[("maxPages", (MAX_PAGE as f64).into())]);
        div()
            .mb(px(16.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--inlinenotice-border-warning"))
            .bg(color("--background-feedback-mod-warning"))
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .flex_none()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .child(icon("warning_triangle", 16., "--icon-feedback-warning")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-strong"))
                    .child(text),
            )
            .into_any_element()
    }

    /// `paginationDock_QHVbp3` with the paginator, or nothing for 25 results or fewer.
    pub(super) fn search_pager(
        &self,
        total: u64,
        offset: u64,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let limits = limits(total, offset);
        if limits.total_count <= PAGE_SIZE {
            return None;
        }
        let pages = limits.total_count.div_ceil(PAGE_SIZE);
        let selected = offset / PAGE_SIZE + 1;
        let jump = self.search.as_ref().and_then(|search| search.jump.as_ref());
        let end = |id: &'static str, label: &'static str, back: bool, disabled: bool| {
            let chevron = || {
                icon(
                    if back {
                        "chevron_small_left"
                    } else {
                        "overlay_chevron_small_right"
                    },
                    16.,
                    "--text-strong",
                )
            };
            div()
                .id(id)
                .h(px(40.))
                .m(px(4.))
                .pl(px(if back { 8. } else { 12. }))
                .pr(px(if back { 12. } else { 8. }))
                .rounded(px(8.))
                .flex()
                .items_center()
                .text_size(px(16.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .when(disabled, |this| this.opacity(0.5))
                .when(!disabled, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(color("--background-mod-normal")))
                })
                .when(back, |this| this.child(div().mr(px(4.)).child(chevron())))
                .child(dc_intl::t(label))
                .when(!back, |this| this.child(div().ml(px(4.)).child(chevron())))
        };
        let mut nav = div().mx_auto().py(px(4.)).flex().items_center();
        for item in page_list(pages, selected, MAX_VISIBLE_PAGES) {
            nav = match item {
                Item::Back { disabled } => nav.child(
                    end("search-page-back", "vgfxaA", true, disabled).when(!disabled, |this| {
                        this.on_click(cx.listener(move |this, _, _, cx| {
                            this.search_page(selected - 1, cx);
                        }))
                    }),
                ),
                Item::Next { disabled } => nav.child(
                    end("search-page-next", "XiOHRX", false, disabled).when(!disabled, |this| {
                        this.on_click(cx.listener(move |this, _, _, cx| {
                            this.search_page(selected + 1, cx);
                        }))
                    }),
                ),
                Item::Page { page, selected } => {
                    let button = div()
                        .id(gpui::ElementId::Name(format!("search-page-{page}").into()))
                        .h(px(ROUND))
                        .min_w(px(ROUND))
                        .m(px(4.))
                        .p(px(6.))
                        .rounded(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(16.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .map(|this| {
                            if selected {
                                this.bg(color("--brand-500")).text_color(color("--white"))
                            } else {
                                this.text_color(color("--text-strong"))
                                    .cursor_pointer()
                                    .hover(|style| {
                                        style
                                            .bg(color("--background-mod-normal"))
                                            .text_color(color("--interactive-text-hover"))
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.search_page(page, cx);
                                    }))
                            }
                        })
                        .child(page.to_string());
                    // `renderPageWrapper`: page 400 of a limited count says why there is no later page.
                    if limits.limited && page == MAX_PAGE {
                        let text =
                            dc_intl::format("E+2azY", &[("maxPages", (MAX_PAGE as f64).into())]);
                        nav.child(self.with_tip(
                            "search-page-limit",
                            Tip::body(Body::Text(text.into()), Side::Top),
                            button,
                            cx,
                        ))
                    } else {
                        nav.child(button)
                    }
                }
                Item::Gap { left } => match jump.filter(|jump| jump.left == left) {
                    // The standard text input (module 95477) of the gap's number field, 50px wide. GAP: the number
                    // field's arrow-key and wheel steps are not ported; Enter jumps, Escape closes.
                    Some(jump) => nav.child(
                        div()
                            .w(px(50.))
                            .h(px(40.))
                            .mx(px(4.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .child(jump.field.clone()),
                    ),
                    None => nav.child(
                        div()
                            .id(if left {
                                "search-page-gap-left"
                            } else {
                                "search-page-gap-right"
                            })
                            .w(px(ROUND))
                            .mx(px(4.))
                            .my(px(8.))
                            .cursor_pointer()
                            .flex()
                            .justify_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child("\u{2026}")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_page_jump(left, window, cx);
                            })),
                    ),
                },
            };
        }
        Some(
            div()
                .flex_none()
                .px(px(16.))
                .border_t_1()
                .border_color(color("--border-subtle"))
                .shadow(vec![super::hover_bar::popover_shadow(1.)])
                .child(div().w_full().text_color(color("--text-strong")).child(nav))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Item, limits, page_list};

    fn pages(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Back { disabled } => format!("<{}", if *disabled { "x" } else { "" }),
                Item::Next { disabled } => format!(">{}", if *disabled { "x" } else { "" }),
                Item::Gap { .. } => "...".to_owned(),
                Item::Page { page, selected } => {
                    format!("{page}{}", if *selected { "*" } else { "" })
                }
            })
            .collect()
    }

    #[test]
    fn the_page_list_keeps_five_pages_around_the_selected_one() {
        assert_eq!(pages(&page_list(2, 1, 5)), ["<x", "1*", "2", ">"]);
        assert_eq!(
            pages(&page_list(20, 1, 5)),
            ["<x", "1*", "2", "3", "...", "20", ">"]
        );
        assert_eq!(
            pages(&page_list(20, 10, 5)),
            ["<", "1", "...", "10*", "...", "20", ">"]
        );
        assert_eq!(
            pages(&page_list(20, 20, 5)),
            ["<", "1", "...", "18", "19", "20*", ">x"]
        );
        assert_eq!(
            pages(&page_list(5, 3, 5)),
            ["<", "1", "2", "3*", "4", "5", ">"]
        );
    }

    #[test]
    fn the_count_stops_at_ten_thousand_and_warns_from_page_four_hundred() {
        let small = limits(60, 0);
        assert_eq!(
            (small.total_count, small.limited, small.warning),
            (60, false, false)
        );
        let big = limits(25_000, 0);
        assert_eq!(
            (big.total_count, big.limited, big.warning),
            (10_000, true, false)
        );
        assert!(!limits(25_000, 398 * 25).warning);
        assert!(limits(25_000, 399 * 25).warning);
        // Exactly 10,000 is not limited: page 400 is the real last page.
        assert!(!limits(10_000, 399 * 25).warning);
    }
}
