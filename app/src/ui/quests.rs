//! Quest Home, the Quests row of the Home sidebar and of the Discover sidebar (the router's `/quest-home`, module
//! 635668). The header bar with its tabs and the Orbs balance pill, the "Introducing Discord Orbs" hero, the sort and
//! filter menus, the empty states and the Claimed Quests tab live here; the data is `dc_state::quests` (the official
//! `QuestStore` and `VirtualCurrencyStore`).
//!
//! The captured account has no quests, so no quest card was ever captured. The grid card (`n6` of module 635668) is
//! drawn from the bundle alone and marked unconfirmed ([`quest_tile`], reached in captures with
//! `gw QUESTS_CAPTURE_CURRENT <body>`); the claimed list's cards (`s$`) are not drawn. Enrolling in, progressing and claiming a quest are account writes; `dc_state::quests` describes the requests
//! and nothing here sends them.
//!
//! Layout is measured in `design/dom/home/quests-*-darker-cozy.json`. Pieces followed from the bundle: `lP` (the
//! header, module 651266's tab bar), `lw` (the hero, the legacy "orbs" variant because the `2026-06-orbs-holdout`
//! experiment is off), `sD` (the All Quests list with its sort and filter popouts), `sM` (its empty states), `sZ`/`sX`
//! (Claimed Quests) and `H` of module 863705 (the balance pill).

use super::discover::{gutter_scrollbar, text};
use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::quests::{Filter, QuestStore, SortMethod, VirtualCurrencyStore, filter_and_sort};
use gpui::{
    AnyElement, Context, IntoElement, ScrollHandle, SharedString, Window, deferred, div, img,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgba,
};

pub(super) const FETCH_PREFIX: &str = "quests:";
const CURRENT_KEY: &str = "quests:current";
const CLAIMED_KEY: &str = "quests:claimed";
const BALANCE_KEY: &str = "quests:balance";
const CLAIMED_IMAGE: &str = "quests:claimed-art";
/// `sX`: the empty Claimed Quests art, 415 by 200, dark (module 317017) and light (module 826223).
const CLAIMED_ART_DARK: &str = "https://discord.com/assets/4e11cf8e80a816c6.svg";
const CLAIMED_ART_LIGHT: &str = "https://discord.com/assets/a09cfd7678b20f8c.svg";
/// `PAID_TERMS_ORBS` of module 652215 (`MARKETING_ENDPOINT` is `https://discord.com`).
const ORBS_TERMS: &str = "https://discord.com/terms/paid-services-terms#14";

/// `isVirtualCurrencyEnabled` of `lw` (`!QUEST_HOME_DESKTOP.enabled`, the `2026-06-orbs-holdout` experiment being
/// off): the hero background is the looping orbs video (`fbd6cf99...webm`, `529e0aea...mp4`), as the capture shows.
/// With it off `lk` draws the PNG below instead (a mountain with a sword), which no capture shows. Neither video
/// codec (VP9, HEVC) can be decoded here, so with it on the hero keeps its black ground.
const VIRTUAL_CURRENCY: bool = true;
/// The hero's picture when [`VIRTUAL_CURRENCY`] is off (`lk`, module 635668).
const HERO_VIDEO: &str = "https://cdn.discordapp.com/assets/content/fff9144c814ffc3126fdcaf5ec5bc68eff6d09e23ac8af5a3fbe61b05970cda0.png";
pub(super) const HERO_VIDEO_KEY: &str = "quests:hero-video";
const HERO_FRAME: &str = "quests:hero-frame";

/// The page area starts 375px from the left: the rail and the 303px sidebar.
const GUTTER: f32 = 8.;
const HEADER: f32 = 49.;
const HERO_HEIGHT: f32 = 500.;
const SORT_MENU_HEIGHT: f32 = 208.;
const FILTER_MENU_HEIGHT: f32 = 348.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    All,
    Claimed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Menu {
    #[default]
    None,
    Sort,
    Filters,
}

pub(super) struct State {
    pub open: bool,
    pub tab: Tab,
    pub sort: SortMethod,
    pub filters: Vec<Filter>,
    pub menu: Menu,
    pub scroll: ScrollHandle,
    pub store: QuestStore,
    pub currency: VirtualCurrencyStore,
    /// The hero video was asked for, and the size of its first frame once decoded.
    pub hero_requested: bool,
    pub hero_size: Option<(u32, u32)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::All,
            sort: SortMethod::Suggested,
            filters: Vec::new(),
            menu: Menu::None,
            scroll: ScrollHandle::new(),
            store: QuestStore::default(),
            currency: VirtualCurrencyStore::default(),
            hero_requested: false,
            hero_size: None,
        }
    }
}

/// The text of the Filters button (`CEfkXl`): the count shows once something is chosen.
fn filters_label(selected: usize) -> String {
    if selected == 0 {
        "Filters".to_owned()
    } else {
        format!("Filters: {selected}")
    }
}

/// `object-fit: cover` with an `object-position`: where a picture of `size` lands in a box, as left, top, width and
/// height in pixels. The picture scales until it covers the box; the overflow is shifted by the position's fractions.
fn cover_at(size: (u32, u32), area: (f32, f32), position: (f32, f32)) -> (f32, f32, f32, f32) {
    let (width, height) = (size.0 as f32, size.1 as f32);
    let scale = (area.0 / width).max(area.1 / height);
    let (scaled_width, scaled_height) = (width * scale, height * scale);
    (
        -(scaled_width - area.0) * position.0,
        -(scaled_height - area.1) * position.1,
        scaled_width,
        scaled_height,
    )
}

/// A popout opens below its button when the page has room for it and above otherwise (module 265872's auto flip).
/// The standard quest tile (`n3`, module 635668, unhovered and not enrolled), from `container_fBxaQT`,
/// `footer_2uXjfk` and `reward_tq3QTU` of 507de6202a746728.css. UNCONFIRMED: no tile was captured, so every value
/// is the bundle's and none was compared with a frame. The width is `eW` (`(1220 - 2 * SPACE_XL * 0.75) / 3`) with
/// `SPACE_XL` assumed 24 for the cozy density. Not drawn (assets are per-quest uploads absent from the snapshot, or
/// state the harness cannot reach): the hero picture (a flat block of its 85 % height stands in), the reward tile
/// picture, the header badge (`nM`: time left and task kind), the hover state and the footer description.
fn quest_tile(quest: &dc_state::quests::Quest) -> AnyElement {
    let base = |alpha: f32| {
        let mut base = color("--background-base-low");
        base.a = alpha;
        base
    };
    div()
        .flex_none()
        .relative()
        .w(px((1220. - 2. * 24. * 0.75) / 3.))
        .h(px(260.))
        .rounded(px(16.))
        .overflow_hidden()
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-base-low"))
        // `heroBackground_fBxaQT`: 85 % of the height, faded over its last 16px.
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(260. * 0.85))
                .bg(color("--background-base-lowest")),
        )
        // `scrim_fBxaQT`: clear down to a third, the page colour from two thirds (an 11-stop curve in the CSS).
        .child(
            div()
                .absolute()
                .top(px(-10.))
                .left_0()
                .w_full()
                .h(px(270.))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(base(0.), 0.23),
                    linear_color_stop(base(1.), 0.66),
                )),
        )
        // `footer_2uXjfk` with `reward_tq3QTU`: 20px inset, a 52px tile beside the game title and the reward line.
        .child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .w_full()
                .px(px(20.))
                .pb(px(20.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_none()
                                .size(px(52.))
                                .rounded(px(10.))
                                .bg(color("--background-base-lowest")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    text(14., 18., 500., "--text-overlay-light")
                                        .text_color(rgba(0xffffffcc))
                                        .child(quest.config.game_publisher.clone()),
                                )
                                .child(
                                    text(16., 20., 500., "--text-overlay-light")
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(quest.config.quest_name.clone()),
                                ),
                        ),
                )
                // `footerPromotedRow_2uXjfk`: "Promoted" (string `o6FLcF`), white at 80 %.
                .child(
                    text(14., 18., 400., "--text-overlay-light")
                        .text_color(rgba(0xffffffcc))
                        .child("Promoted"),
                ),
        )
        .into_any_element()
}

fn opens_below(button_bottom: f32, menu_height: f32, viewport_height: f32) -> bool {
    button_bottom + 8. + menu_height <= viewport_height
}

impl Root {
    /// Quest Home is the Home content while its flag is set and nothing else has taken the content area.
    pub(super) fn quests_active(&self) -> bool {
        self.quests.open
            && self.guild.is_none()
            && self.channel.is_none()
            && !self.nitro_open
            && !self.discover.open
            && !self.shop.open
    }

    /// The Quests rows of both sidebars. Records the visit like `select_home` does.
    pub(super) fn open_quests(&mut self, cx: &mut Context<Self>) {
        if self.quests_active() {
            return;
        }
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                guild: None,
                channel: None,
                favorites: false,
                nitro: false,
                guild_guide: false,
                channel_browser: false,
                discover: None,
                quests: true,
                shop: false,
                message_requests: false,
            },
        );
        self.set_quests_open(cx);
    }

    /// The part of `open_quests` that history replays also run.
    pub(super) fn set_quests_open(&mut self, cx: &mut Context<Self>) {
        self.favorites_view = false;
        self.discover.open = false;
        self.shop.open = false;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.guild = None;
        self.channel = None;
        self.unread_from = None;
        self.stop_lurking_elsewhere();
        self.rebuild_rows();
        self.request_images();
        self.quests.open = true;
        self.quests.tab = Tab::All;
        self.quests.menu = Menu::None;
        self.quests.scroll.set_offset(point(px(0.), px(0.)));
        // `Qh` fetches with the "cache-and-network" policy: the quests are asked for on every visit.
        self.quests_fetch(CURRENT_KEY, dc_state::quests::CURRENT_ROUTE);
        self.quests.store.begin_current();
        if !VIRTUAL_CURRENCY && !self.quests.hero_requested {
            self.quests.hero_requested = true;
            let _ = self.requests.send(Request::Image {
                key: HERO_VIDEO_KEY.to_owned(),
                url: HERO_VIDEO.to_owned(),
            });
        }
        if self.quests.currency.should_fetch() {
            self.quests_fetch(BALANCE_KEY, dc_state::quests::BALANCE_ROUTE);
            self.quests.currency.begin();
        }
        cx.notify();
    }

    fn quests_fetch(&mut self, key: &str, route: &str) {
        self.requested.remove(key);
        self.fetched.remove(key);
        self.requested.insert(key.to_owned());
        let _ = self.requests.send(Request::Fetch {
            key: key.to_owned(),
            route: route.to_owned(),
        });
    }

    pub(super) fn quests_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let body = self.fetched.get(key).cloned().unwrap_or_default();
        let failed = crate::net::is_fetch_failure(&body);
        match key {
            CURRENT_KEY if failed => self.quests.store.fail_current(),
            CURRENT_KEY => self.quests.store.apply_current(&body),
            CLAIMED_KEY if failed => self.quests.store.fail_claimed(),
            CLAIMED_KEY => self.quests.store.apply_claimed(&body),
            BALANCE_KEY if failed => self.quests.currency.fail(),
            BALANCE_KEY => self.quests.currency.succeed(&body),
            _ => {}
        }
        cx.notify();
    }

    /// The header's tabs: `er` of module 635668 scrolls to the top and switches. The claimed list is asked for each
    /// time its tab opens (`Iq` fetches when the tab mounts).
    fn select_quests_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.quests.menu = Menu::None;
        self.quests.scroll.set_offset(point(px(0.), px(0.)));
        if self.quests.tab == tab {
            cx.notify();
            return;
        }
        self.quests.tab = tab;
        if tab == Tab::Claimed {
            self.quests_fetch(CLAIMED_KEY, dc_state::quests::CLAIMED_ROUTE);
            self.quests.store.begin_claimed();
            let art = if matches!(super::theme(), dc_tokens::Theme::Light) {
                CLAIMED_ART_LIGHT
            } else {
                CLAIMED_ART_DARK
            };
            self.request_image(CLAIMED_IMAGE.to_owned(), art.to_owned());
        }
        cx.notify();
    }

    /// "Explore Orbs Shop": the official client opens the Shop on its Orbs tab (`Cz` with `tab: ORBS`).
    pub(super) fn quests_open_orbs_shop(&mut self, cx: &mut Context<Self>) {
        self.open_shop(cx);
        self.shop_set_tab(super::shop::Tab::Orbs, cx);
    }

    /// The hero picture arrived: its size is read off the interface thread and the bytes are kept as the hero picture.
    pub(super) fn quests_hero_video_fetched(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let frame = cx
                .background_executor()
                .spawn(async move {
                    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
                        .with_guessed_format()
                        .map_err(|error| error.to_string())?
                        .into_dimensions()
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>((bytes, width, height))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match frame {
                    Ok((png, width, height)) => {
                        this.images.insert(
                            HERO_FRAME.to_owned(),
                            std::sync::Arc::new(gpui::Image::from_bytes(
                                gpui::ImageFormat::Png,
                                png,
                            )),
                        );
                        this.quests.hero_size = Some((width, height));
                    }
                    Err(error) => tracing::warn!("quest hero picture not decoded: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The quests the All Quests list shows: filtered and sorted as `Qh` does, expired ones dropped unless they wait
    /// to be claimed.
    fn visible_quests(&self) -> Vec<&dc_state::quests::Quest> {
        let now = chrono::Utc::now().timestamp_millis();
        filter_and_sort(
            &self.quests.store.quests,
            self.quests.sort,
            &self.quests.filters,
            now,
        )
        .into_iter()
        .filter(|quest| !quest.is_expired(now) || quest.awaits_claim())
        .collect()
    }

    /// The quest cards of the grid (`n6` of module 635668, `carouselItems_mtY8m_`: a 20px gap). UNCONFIRMED: no card
    /// was ever captured (the account has no quests); see [`quest_tile`]. Still missing as captures: an enrolled card,
    /// a completed card, a claimed card (Claimed Quests tab) and each reward kind's tile.
    fn quest_tiles(&self, quests: &[&dc_state::quests::Quest]) -> Vec<AnyElement> {
        if quests.is_empty() {
            return Vec::new();
        }
        vec![
            div()
                .mt(px(24.))
                .flex_none()
                .flex()
                .gap(px(20.))
                .children(quests.iter().map(|quest| quest_tile(quest)))
                .into_any_element(),
        ]
    }

    // ---- the page ----

    pub(super) fn quests_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scroll = self.quests.scroll.clone();
        let body = match self.quests.tab {
            Tab::All => self.quests_all_body(window, cx),
            Tab::Claimed => self.quests_claimed_body(cx),
        };
        let scroller = div()
            .id("quests-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .on_scroll(cx.listener(|this, _, _, cx| {
                // The popouts close when the page scrolls (`scrollBehavior: "close"`).
                if this.quests.menu != Menu::None {
                    this.quests.menu = Menu::None;
                    cx.notify();
                }
            }))
            .pr(px(GUTTER))
            .child(body);
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(1.))
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(color("--background-base-lower"))
                    .child(self.quests_header(cx))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .child(scroller)
                            .child(gutter_scrollbar(scroll)),
                    ),
            )
            .into_any_element()
    }

    /// `lP`: the 49px bar with its 24px icon, the tabs 44px on, and the balance pill at the right.
    fn quests_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = |id: &'static str, label: &'static str, which: Tab, cx: &mut Context<Self>| {
            let active = self.quests.tab == which;
            div()
                .id(id)
                .relative()
                .flex_none()
                .h(px(48.))
                .flex()
                .items_center()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.select_quests_tab(which, cx)))
                .child(text(16., 20., 500., "--text-strong").child(label))
                .when(active, |this| {
                    // The 2px underline sits over the bar's own 1px rule.
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(px(-1.))
                            .h(px(2.))
                            .bg(color("--text-strong")),
                    )
                })
        };
        let balance = self.quests.currency.shown();
        div()
            .parity_below("quests-header", 1.)
            .relative()
            .flex_none()
            .w_full()
            .h(px(HEADER))
            .pl(px(16.))
            .pr(px(12.))
            .border_b_1()
            .border_color(color("--background-mod-subtle"))
            .bg(color("--background-base-lower"))
            .flex()
            .items_center()
            .child(icon("home_quests", 24., "--text-strong"))
            .child(
                div()
                    .ml(px(44.))
                    .flex()
                    .items_center()
                    // `.tabs_fgRQ5j`: "All Quests" ends at 529.8 and "Claimed Quests" starts at 561.8.
                    .gap(px(32.))
                    .h(px(48.))
                    .child(tab("quests-tab-all", "All Quests", Tab::All, cx))
                    .child(tab(
                        "quests-tab-claimed",
                        "Claimed Quests",
                        Tab::Claimed,
                        cx,
                    )),
            )
            .child(div().flex_1())
            .child(
                // `Gy` of module 533609, the pill: 32 tall, a 1px rule, 12px sides, the orb in a 20px box and the number
                // 2px after it. Hovered, the fill takes the stronger step.
                div()
                    .id("quests-orbs")
                    .parity_with_border("quests-orbs", 1.)
                    .flex_none()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-mod-subtle"))
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w(px(20.))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(icon("pm_orbs", 18., "--text-strong")),
                    )
                    .child(
                        text(16., 20., 600., "--text-strong")
                            .ml(px(2.))
                            .child(balance.to_string()),
                    ),
            )
            .into_any_element()
    }

    /// `sD`: the hero, the "Available Quests" row with its two buttons, and the grid or an empty state.
    fn quests_all_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.visible_quests();
        let filtered = !self.quests.filters.is_empty();
        let loading = self.quests.store.is_fetching_current_quests && visible.is_empty();
        let tiles = self.quest_tiles(&visible);
        let empty = visible.is_empty() && !loading;
        let hero = self.quests_hero(cx);
        let controls = self.quests_heading_controls(window, cx);
        div()
            .w_full()
            .px(px(20.))
            .py(px(24.))
            .flex()
            .flex_col()
            .child(hero)
            .child(
                div()
                    .parity("quests-heading")
                    .mt(px(64.))
                    .h(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(text(20., 24., 500., "--text-strong").child("Available Quests"))
                    .child(controls),
            )
            .when(empty, |this| {
                let title = if filtered {
                    "There is nothing to see here"
                } else {
                    "Sorry adventurer, there are no Quests for you right now"
                };
                this.child(
                    div()
                        .parity("quests-empty")
                        .flex_none()
                        .w_full()
                        .max_w(px(751.))
                        .mx_auto()
                        .p(px(88.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(16.))
                        .text_center()
                        .child(text(24., 32., 600., "--text-strong").child(title))
                        .child(if filtered {
                            // `LdYS1H`: "No results found. See all Quests", the link clears the filters.
                            text(16., 20., 400., "--text-subtle")
                                .flex()
                                .flex_wrap()
                                .justify_center()
                                .child("No results found.\u{a0}")
                                .child(
                                    div()
                                        .id("quests-clear-filters")
                                        .cursor_pointer()
                                        .text_color(color("--text-link"))
                                        .hover(|style| style.underline())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.quests.filters.clear();
                                            cx.notify();
                                        }))
                                        .child("See all Quests"),
                                )
                        } else {
                            text(16., 20., 400., "--text-subtle").child(
                                "We couldn't find any Quests that are a good fit for you. Some Quests are only available in certain regions or to users of certain ages. Try checking back later!",
                            )
                        }),
                )
            })
            .children(tiles)
            .into_any_element()
    }

    /// `ev` and `lw`: 857 by 500, a 24px radius and a 1px rule; the picture, the fade at the left, the blue glow at the
    /// foot and the text and buttons over them (`heroBackgroundOrbs`, `backgroundOverlay`, `heroGradientOrbs`).
    fn quests_hero(&self, cx: &mut Context<Self>) -> AnyElement {
        // The picture is a looping video in the official client; the first frame of its MP4 stands in for it (what a
        // paused video shows), laid out as `object-fit: cover; object-position: 75% 50%` (`heroBannerAssetOrbs`).
        let black = |alpha: f32| gpui::hsla(0., 0., 0., alpha);
        let width = self
            .tip_anchors
            .borrow()
            .get("quests-hero")
            .map_or(857., |bounds| f32::from(bounds.size.width))
            - 2.;
        let picture = self
            .images
            .get(HERO_FRAME)
            .cloned()
            .zip(self.quests.hero_size)
            .map(|(image, size)| {
                let (left, top, picture_width, picture_height) =
                    cover_at(size, (width, HERO_HEIGHT - 2.), (0.75, 0.5));
                img(image)
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(picture_width))
                    .h(px(picture_height))
            });
        let background = div()
            .absolute()
            .inset_0()
            .rounded(px(23.))
            .overflow_hidden()
            .bg(gpui::black())
            .children(picture)
            // `backgroundOverlay_-l8fcd:after`: the page colour (black in this scope) fading out over 600px.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(px(192.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(black(1.), 0.),
                        linear_color_stop(black(0.83), 1.),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(192.))
                    .h_full()
                    .w(px(408.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(black(0.83), 0.),
                        linear_color_stop(black(0.), 1.),
                    )),
            )
            // `heroGradientOrbs`: brand at 25% opacity rising from the foot to a third of the height.
            .child(div().absolute().inset_0().bg(linear_gradient(
                0.,
                linear_color_stop(gpui::hsla(235. / 360., 0.86, 0.65, 0.25), 0.),
                linear_color_stop(gpui::hsla(235. / 360., 0.86, 0.65, 0.), 0.33),
            )));
        // `title_AOdYmi`: 44px in the display face, uppercase; the face sits 4px low in a 44px line box.
        let title = text(44., 44., 700., "--text-strong")
            .text_color(rgba(0xdcdcdfff))
            .relative()
            .top(px(-4.))
            .w_full()
            .font_family(super::nitro::NITRO_FONT)
            .child("INTRODUCING DISCORD ORBS");
        let subtitle = text(16., 20., 400., "--text-strong")
            .text_color(rgba(0xdcdcdfcc))
            .child("Reward Your Play. Earn through Quests. Spend in the Shop.");
        // The overlay buttons (`overlay-primary`, `overlay-secondary`), measured in the midnight scope the hero sets.
        let primary = div()
            .id("quests-hero-shop")
            .parity_with_border("quests-hero-shop", 1.)
            .flex_none()
            // 159.7 wide in the dump (`overlay-primary`); the face here measures a little narrower.
            .w(px(159.7))
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgba(0x97979f33))
            .bg(color("--control-overlay-primary-background-default"))
            .hover(|style| style.bg(color("--control-overlay-primary-background-hover")))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.quests_open_orbs_shop(cx)))
            .child(
                text(16., 20., 500., "--control-overlay-primary-text-default")
                    .child("Explore Orbs Shop"),
            );
        let secondary = div()
            .id("quests-hero-terms")
            .flex_none()
            // 166.7 wide in the dump (`overlay-secondary`).
            .w(px(166.7))
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgba(0x97979f29))
            .bg(color("--control-overlay-secondary-background-default"))
            .hover(|style| style.bg(color("--control-overlay-secondary-background-hover")))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| cx.open_url(ORBS_TERMS)))
            .child(
                text(16., 20., 500., "--control-overlay-secondary-text-default")
                    .child("Discord Orbs Terms"),
            );
        div()
            .parity_with_border("quests-hero", 1.)
            .child(self.anchor_probe("quests-hero"))
            .flex_none()
            .relative()
            .w_full()
            .h(px(HERO_HEIGHT))
            .rounded(px(24.))
            .border_1()
            .border_color(rgba(0x97979f33))
            .p(px(32.))
            .flex()
            .flex_col()
            .justify_end()
            .child(background)
            .child(
                div()
                    .relative()
                    .w(px(540.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(div().h(px(88.)).child(title))
                            .child(subtitle),
                    )
                    .child(div().flex().gap(px(8.)).child(primary).child(secondary)),
            )
            .into_any_element()
    }

    /// `headingControls_4L5sf6`: the Sort by button and the Filters button, 8px apart, with their popouts.
    fn quests_heading_controls(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let menu = self.quests.menu;
        let button = |id: &'static str,
                      label: String,
                      glyph: &'static str,
                      open: bool,
                      which: Menu,
                      width: f32,
                      cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex_none()
                // Widths from design/dom/home/quests-darker-cozy.json (106.9 and 82.6 with the default labels).
                .min_w(px(width))
                .h(px(32.))
                .px(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--control-secondary-border-default"))
                .bg(color(if open {
                    "--control-secondary-background-active"
                } else {
                    "--control-secondary-background-default"
                }))
                .hover(|style| style.bg(color("--control-secondary-background-hover")))
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.quests.menu = if this.quests.menu == which {
                        Menu::None
                    } else {
                        which
                    };
                    cx.notify();
                }))
                .child(text(14., 18., 500., "--control-secondary-text-default").child(label))
                .child(icon(glyph, 16., "--control-secondary-icon-default"))
        };
        let viewport = f32::from(window.viewport_size().height);
        let anchor_bottom = |key: &str| {
            self.tip_anchors
                .borrow()
                .get(key)
                .map(|bounds| f32::from(bounds.bottom()))
        };
        let sort_below = anchor_bottom("quests-sort")
            .is_some_and(|bottom| opens_below(bottom, SORT_MENU_HEIGHT, viewport));
        let filters_below = anchor_bottom("quests-filters")
            .is_some_and(|bottom| opens_below(bottom, FILTER_MENU_HEIGHT, viewport));
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .parity("quests-sort")
                    .relative()
                    .flex_none()
                    .child(button(
                        "quests-sort-button",
                        self.quests.sort.label().to_owned(),
                        "select_chevron",
                        menu == Menu::Sort,
                        Menu::Sort,
                        106.9,
                        cx,
                    ))
                    .child(self.anchor_probe("quests-sort"))
                    .when(menu == Menu::Sort, |this| {
                        this.child(self.sort_menu(sort_below, cx))
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(button(
                        "quests-filters-button",
                        filters_label(self.quests.filters.len()),
                        "inbox_filter",
                        menu == Menu::Filters,
                        Menu::Filters,
                        82.6,
                        cx,
                    ))
                    .child(self.anchor_probe("quests-filters"))
                    .when(menu == Menu::Filters, |this| {
                        this.child(self.filters_menu(filters_below, cx))
                    }),
            )
            .into_any_element()
    }

    /// `popout__S_k8m`: 242 wide, a 1px rule, 16px padding, the high surface and shadow, right-aligned to its button.
    fn quests_popout(
        &self,
        id: &'static str,
        anchor: &'static str,
        parity_id: &'static str,
        below: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let anchors = self.tip_anchors.clone();
        div()
            .parity_with_border(parity_id, 1.)
            .id(id)
            // `animatorTop translate` (design/dom/home/quests-sort-open).
            .popout(
                id,
                PopKind::Translate,
                if below { PopSide::Bottom } else { PopSide::Top },
            )
            .occlude()
            .on_mouse_down_out(
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    // A press on the button itself is the button's own toggle.
                    let on_button = anchors
                        .borrow()
                        .get(anchor)
                        .is_some_and(|bounds| bounds.contains(&event.position));
                    if !on_button {
                        this.quests.menu = Menu::None;
                        cx.notify();
                    }
                }),
            )
            .absolute()
            .right(px(0.))
            .when(below, |this| this.top(px(40.)))
            .when(!below, |this| this.bottom(px(40.)))
            .w(px(242.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::rgba(0x0000003d).into(),
                offset: point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .p(px(16.))
            .flex()
            .flex_col()
    }

    /// `s_`: "Sort by" and a radio per method; picking one applies it and closes the popout.
    fn sort_menu(&self, below: bool, cx: &mut Context<Self>) -> AnyElement {
        let current = self.quests.sort;
        deferred(
            self.quests_popout(
                "quests-sort-menu",
                "quests-sort",
                "quests-sort-menu",
                below,
                cx,
            )
            .child(text(14., 18., 600., "--text-subtle").child("Sort by"))
            .child(
                div()
                    .mt(px(16.))
                    .pt(px(8.))
                    .pb(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .children(SortMethod::ALL.into_iter().map(|method| {
                        div()
                            .id(SharedString::from(format!("quests-sort-{}", method.wire())))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.quests.sort = method;
                                this.quests.menu = Menu::None;
                                cx.notify();
                            }))
                            .child(super::create_channel::radio(method == current))
                            .child(text(16., 20., 400., "--text-default").child(method.label()))
                    })),
            ),
        )
        .with_priority(1)
        .into_any_element()
    }

    /// `sj`: a checkbox per option under "Reward" and "Quest Type", and Clear.
    fn filters_menu(&self, below: bool, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.quests.filters.clone();
        let mut menu = self.quests_popout(
            "quests-filters-menu",
            "quests-filters",
            "quests-filters-menu",
            below,
            cx,
        );
        for (index, (_, heading, options)) in Filter::GROUPS.iter().enumerate() {
            if index > 0 {
                menu = menu.child(
                    div()
                        .relative()
                        .left(px(-8.))
                        .w(px(224.))
                        .h(px(1.))
                        .mt(px(12.))
                        .mb(px(16.))
                        .bg(color("--border-subtle")),
                );
            }
            menu = menu
                .child(text(14., 18., 600., "--text-subtle").child(*heading))
                .child(
                    div()
                        .mt(px(16.))
                        .pt(px(8.))
                        .pb(px(4.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .children(options.iter().flatten().copied().map(|filter| {
                            let on = chosen.contains(&filter);
                            div()
                                .id(SharedString::from(format!(
                                    "quests-filter-{}",
                                    filter.wire()
                                )))
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(at) =
                                        this.quests.filters.iter().position(|f| *f == filter)
                                    {
                                        this.quests.filters.remove(at);
                                    } else {
                                        this.quests.filters.push(filter);
                                    }
                                    cx.notify();
                                }))
                                .child(super::server_games::checkbox(on))
                                .child(text(16., 20., 400., "--text-strong").child(filter.label()))
                        })),
                );
        }
        deferred(
            menu.child(
                div()
                    .relative()
                    .left(px(-8.))
                    .w(px(224.))
                    .h(px(1.))
                    .mt(px(12.))
                    .bg(color("--border-subtle")),
            )
            .child(
                // `submitWrapper_m3psIG`: the button hangs 8px into the popout's padding.
                div()
                    .relative()
                    .left(px(-8.))
                    .w(px(224.))
                    .h(px(24.))
                    .mt(px(8.))
                    .child(
                        div()
                            .id("quests-filters-clear")
                            .absolute()
                            .top_0()
                            .left_0()
                            .w(px(224.))
                            .h(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|style| style.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.quests.filters.clear();
                                this.quests.menu = Menu::None;
                                cx.notify();
                            }))
                            .child(
                                text(14., 18., 500., "--control-secondary-text-default")
                                    .child("Clear"),
                            ),
                    ),
            ),
        )
        .with_priority(1)
        .into_any_element()
    }

    /// `sZ`: the claimed quests, or the spinner while they load, or the note with its art.
    fn quests_claimed_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let claimed = self.quests.store.claimed_by_date();
        let loading = self.quests.store.is_fetching_claimed_quests && claimed.is_empty();
        let empty = claimed.is_empty() && !loading;
        let art = self.images.get(CLAIMED_IMAGE).cloned();
        div()
            .w_full()
            .px(px(20.))
            .py(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .when(empty, |this| {
                // `sX` (module 396478): a 440px column, the 415 by 200 art, 40px, then the note 8px down.
                this.child(
                    div()
                        .parity("quests-claimed-empty")
                        .w(px(440.))
                        .h(px(268.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(415.))
                                .h(px(200.))
                                .mb(px(40.))
                                .flex_none()
                                .children(art.map(|image| img(image).w(px(415.)).h(px(200.)))),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .flex()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child("You haven't completed any Quests yet!\u{a0}")
                                .child(
                                    div()
                                        .id("quests-see-all")
                                        .cursor_pointer()
                                        .text_color(color("--text-link"))
                                        .hover(|style| style.underline())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.select_quests_tab(Tab::All, cx)
                                        }))
                                        .child("See all Quests"),
                                ),
                        ),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hero_picture_covers_its_box_and_sits_at_three_quarters() {
        // 16:9 into 855 by 498: the height decides, 885.33 wide, and the overflow shifts by 75%.
        let (left, top, width, height) = cover_at((1920, 1080), (855., 498.), (0.75, 0.5));
        assert!((height - 498.).abs() < 0.01 && (top).abs() < 0.01);
        assert!((width - 885.333).abs() < 0.01);
        assert!((left + 22.75).abs() < 0.01);
        // A box wider than the picture scales it to the width instead.
        let (left, top, width, height) = cover_at((100, 100), (200., 100.), (0.75, 0.5));
        assert_eq!((left, width, height), (0., 200., 200.));
        assert!((top + 50.).abs() < 0.01);
    }

    #[test]
    fn the_filters_button_counts_its_choices() {
        assert_eq!(filters_label(0), "Filters");
        assert_eq!(filters_label(2), "Filters: 2");
    }

    #[test]
    fn a_popout_flips_above_when_below_does_not_fit() {
        // The captured button's bottom is at 702 in a 720 window: no room for 208px below it.
        assert!(!opens_below(702., SORT_MENU_HEIGHT, 720.));
        assert!(!opens_below(702., FILTER_MENU_HEIGHT, 720.));
        assert!(opens_below(300., SORT_MENU_HEIGHT, 720.));
    }
}
