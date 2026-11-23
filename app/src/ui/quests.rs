//! Quest Home, the Quests row of the Home sidebar and of the Discover sidebar (the router's `/quest-home`, module
//! 635668). The header bar with its tabs and the Orbs balance pill, the "Introducing Discord Orbs" hero, the sort and
//! filter menus, the empty states and the Claimed Quests tab live here; the data is `dc_state::quests` (the official
//! `QuestStore` and `VirtualCurrencyStore`).
//!
//! The captured account has no quests, so no quest card was ever captured. The grid card (`n6` of module 635668) is
//! drawn from the bundle alone and marked unconfirmed ([`quest_tile`], reached in captures with
//! `gw QUESTS_CAPTURE_CURRENT <body>`); the claimed list's cards (`s$`, [`Root::claimed_card`]) are drawn from the
//! bundle too and equally unconfirmed. Enrolling in, progressing and claiming a quest are account writes; `dc_state::quests` describes the requests
//! and nothing here sends them.
//!
//! Layout is measured in `design/dom/home/quests-*-darker-cozy.json`. Pieces followed from the bundle: `lP` (the
//! header, module 651266's tab bar), `lw` (the hero, the legacy "orbs" variant because the `2026-06-orbs-holdout`
//! experiment is off), `sD` (the All Quests list with its sort and filter popouts), `sM` (its empty states), `sZ`/`sX`
//! (Claimed Quests) and `H` of module 863705 (the balance pill).

use super::discover::{gutter_scrollbar, text};
use super::motion::{PopKind, PopSide, Popout as _};
use super::quest_bar::{QuestBarState, progress_badge};
use super::quest_copy::{
    CtaAction, CtaContext, CtaSurface, CtaVariant, InstructionsContext, card_description,
    format_month_day, game_cta_subtitle, platform_menu_aria_label, platform_menu_options,
    primary_cta_label, redemption_instructions, resolve_cta, reward_name,
};
use super::quest_menu::{QuestMenuState, QuestSurface};
use super::quest_promoted::EnrollmentBlockedModal;
use super::quest_split_modal::SplitQuestModalState;
use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_intl::Arg;
use dc_state::quests::{
    ActivePlatformScreen, Filter, Quest, QuestCtaState, QuestPlatform, QuestStore, SortMethod,
    VirtualCurrencyStore, filter_and_sort,
};
use gpui::{
    AnyElement, Context, IntoElement, ScrollHandle, SharedString, Window, deferred, div, img,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgba,
};

pub(super) const FETCH_PREFIX: &str = "quests:";
/// Capture only: the claimed list came from `gw QUESTS_CAPTURE_CLAIMED`.
#[cfg(feature = "capture")]
pub(super) static CLAIMED_INJECTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
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

/// Video Quest Player & Bounty Ad Watch-Time Progress modal state (modules 767868, 579666, 491812:
/// persistent share overlay `WmfZHZ`/`RDE0Sc`/`+5kSoW`, close button `491812` `cpT0Cq`, watch-time
/// progress `0F4Peo`/`45cddS`, `nLXlh+` Orbs target marker, `o6FLcF` Promoted footer, `w6PeBF`
/// pause-on-unfocus message, `uLjCfn` claim error toast, `rStN/7`/`E1du/q` unsupported fallback,
/// and `mxaHfx` end screen).
#[derive(Clone, Debug)]
pub(crate) struct VideoQuestModal {
    pub quest_id: String,
    pub title: String,
    pub subtitle: String,
    pub cta_label: String,
    pub cta_link: String,
    pub ended: bool,
    pub unsupported_fallback: bool,
    pub watch_secs: u32,
    pub duration_secs: u32,
    /// Module 579666 bounty ad watch-time mode (`None` for standard `767868` video quests).
    pub bounty_orbs: Option<u64>,
    pub paused_away: bool,
    pub claim_error: bool,
    /// Module 491147 (`VideoQuestModal` footer): reward progress ring & advertiser details.
    pub reward_name: String,
    pub publisher: String,
    pub cta_subtitle: String,
    pub reward_image: Option<String>,
    pub claimed: bool,
    pub progress_hovered: bool,
    /// Module 40705 `discord-web-video-player` control bar state.
    pub dvp: super::video_player::DvpControlState,
}

/// Module `491147` / `738883`: hover progress percentage (capped at 99% until `completed == true`).
pub(crate) fn video_quest_progress_percent(watched: f64, duration: f64, completed: bool) -> u32 {
    if completed {
        100
    } else if duration <= 0.0 {
        0
    } else {
        ((watched / duration * 100.0).floor() as u32).min(99)
    }
}

/// Module `491147` (`U`): `NRp4K4` ("Claim reward: {rewardName}") when completed and unclaimed,
/// else `12IWP2` ("Reward: {rewardName}").
pub(crate) fn video_quest_reward_tooltip(
    completed: bool,
    claimed: bool,
    reward_name: &str,
) -> String {
    let key = if completed && !claimed {
        "NRp4K4"
    } else {
        "12IWP2"
    };
    dc_intl::format(
        key,
        &[("rewardName", dc_intl::Arg::Str(reward_name.into()))],
    )
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
    /// The claimed card under the pointer (`hovered_HjAsMI`).
    pub claimed_hover: Option<String>,
    /// Quest Home tile under the pointer (module 793217).
    pub hovered_tile: Option<String>,
    /// Quest whose console/desktop platform selector dropdown is open (module 793217 `eb`).
    pub open_platform_selector: Option<String>,
    /// Quest whose `Play` platform dropdown is open (module 452411 `tc`).
    pub open_platform_menu: Option<String>,
    /// Bottom-left Quest Bar state (module 66246).
    pub bar: QuestBarState,
    /// Quest card context menu, disclosure modal, and error/unverified alerts (modules 270045, 348559, 201805).
    pub menus: QuestMenuState,
    /// `SplitQuestModal` account connect flow (module 968779).
    pub split_modal: Option<SplitQuestModalState>,
    /// Enrollment-blocked ("You've been busy!", module 124232) countdown modal.
    pub enrollment_blocked_modal: Option<EnrollmentBlockedModal>,
    /// Quests access suspended modal (module 700085: `WfwodX` / `I27WXW` / `hvVgAZ`).
    pub access_suspended_modal: bool,
    /// Capture-only promoted card preview quest id (module 107543).
    pub promoted_preview_quest: Option<String>,
    /// Activity Panel Quest Card (module 834615).
    pub activity_panel_card: Option<super::quest_bar::ActivityPanelQuestCard>,
    /// In-app Video Quest Player modal & end screen (module 767868).
    pub video_modal: Option<VideoQuestModal>,
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
            claimed_hover: None,
            hovered_tile: None,
            open_platform_selector: None,
            open_platform_menu: None,
            bar: QuestBarState::default(),
            menus: QuestMenuState::default(),
            split_modal: None,
            enrollment_blocked_modal: None,
            access_suspended_modal: false,
            promoted_preview_quest: None,
            activity_panel_card: None,
            video_modal: None,
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
fn opens_below(button_bottom: f32, menu_height: f32, viewport_height: f32) -> bool {
    button_bottom + 8. + menu_height <= viewport_height
}

/// Platform label helper for module 793217 (`ep` / `eT` / `eb`).
fn tile_platform_option_label(platform: Option<QuestPlatform>, short: bool) -> &'static str {
    match (platform, short) {
        (Some(QuestPlatform::Desktop), false) => dc_intl::t("QXc01+"),
        (Some(QuestPlatform::Console), false) => dc_intl::t("8lAfuB"),
        (Some(QuestPlatform::Desktop), true) => dc_intl::t("g6Dr44"),
        (Some(QuestPlatform::Console), true) => dc_intl::t("iyNbj5"),
        (None, _) => dc_intl::t("WLDKSb"),
    }
}

/// Video header button tooltip label (`$` of module 793217).
fn tile_video_play_tooltip(quest: &Quest, now_ms: i64) -> &'static str {
    if quest.is_completed() {
        dc_intl::t("YsCuyF")
    } else if quest.is_enrolled() {
        dc_intl::t("74KqrR")
    } else if quest.is_expired(now_ms) {
        dc_intl::t("I6JG46")
    } else {
        dc_intl::t("umdNin")
    }
}

/// Whether the standard footer (`eL` of module 793217) renders the secondary external link CTA beside the primary CTA.
fn tile_shows_external_cta(state: QuestCtaState, quest: &Quest) -> bool {
    match state {
        QuestCtaState::ExpiredClaimable
        | QuestCtaState::Expired
        | QuestCtaState::ExpiredClaimed
        | QuestCtaState::Unenrolled => true,
        QuestCtaState::Claimed | QuestCtaState::Completed => {
            quest.is_watch_video() || quest.is_activity_quest()
        }
        QuestCtaState::Incomplete | QuestCtaState::Enrolled => {
            quest.is_watch_video() || quest.is_activity_quest() || quest.is_achievement_in_game()
        }
    }
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
                library: false,
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
        // Capture only: once a run has put its own claimed list in, the account's answer does not replace it.
        #[cfg(feature = "capture")]
        if key == CLAIMED_KEY
            && CLAIMED_INJECTED.load(std::sync::atomic::Ordering::Relaxed)
            && !self
                .fetched
                .get(key)
                .is_some_and(|body| body["capture"] == true)
        {
            return;
        }
        let body = self.fetched.get(key).cloned().unwrap_or_default();
        let failed = crate::net::is_fetch_failure(&body);
        match key {
            CURRENT_KEY if failed => self.quests.store.fail_current(),
            CURRENT_KEY => {
                self.quests.store.apply_current(&body);
                let wanted: Vec<String> = self
                    .quests
                    .store
                    .quests
                    .iter()
                    .flat_map(|q| {
                        [
                            q.config
                                .assets_hero
                                .as_deref()
                                .map(|a| quest_asset(&q.id, a)),
                            q.config
                                .assets_reward_tile
                                .as_deref()
                                .or(q.config.assets_game_tile.as_deref())
                                .map(|a| quest_asset(&q.id, a)),
                            q.config
                                .assets_logotype_dark
                                .as_deref()
                                .or(q.config.assets_logotype.as_deref())
                                .map(|a| quest_asset(&q.id, a)),
                        ]
                    })
                    .flatten()
                    .collect();
                for url in wanted {
                    if !self.images.contains_key(&url) {
                        self.request_image(url.clone(), url);
                    }
                }
            }
            CLAIMED_KEY if failed => self.quests.store.fail_claimed(),
            CLAIMED_KEY => {
                self.quests.store.apply_claimed(&body);
                // The cards' reward pictures and logotypes.
                let wanted: Vec<String> = self
                    .quests
                    .store
                    .claimed_by_date()
                    .iter()
                    .flat_map(|quest| [claimed_picture(quest), claimed_logo(quest)])
                    .flatten()
                    .collect();
                for url in wanted {
                    if !self.images.contains_key(&url) {
                        self.request_image(url.clone(), url);
                    }
                }
            }
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

    /// The quest cards of the grid (module 793217, CSS `CXA5dw` / `clpvxq` / `DLRZY4` / `bvFqCe`).
    fn quest_tiles(&self, quests: &[&Quest], cx: &mut Context<Self>) -> Vec<AnyElement> {
        if quests.is_empty() {
            return Vec::new();
        }
        let avail = (self.viewport.0 - self.combined_sidebar_width() - 48.).max(360.);
        let cols = if avail >= 1180. {
            3.
        } else if avail >= 740. {
            2.
        } else {
            1.
        };
        let card_w = ((avail - (cols - 1.) * 20.) / cols).clamp(320., 460.);
        vec![
            div()
                .mt(px(24.))
                .w_full()
                .flex_none()
                .flex()
                .flex_wrap()
                .gap(px(20.))
                .children(
                    quests
                        .iter()
                        .enumerate()
                        .map(|(idx, quest)| self.quest_tile(quest, idx == 0, card_w, cx)),
                )
                .into_any_element(),
        ]
    }

    /// Full Quest Home card (`eF` / `$` / `eU` / `eD` / `W` of module 793217).
    fn quest_tile(
        &self,
        quest: &Quest,
        featured: bool,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = chrono::Utc::now().timestamp_millis();
        let is_video = quest.is_watch_video_desktop();
        let expired = quest.is_expired(now);
        let claimed = quest.is_claimed();
        let enrolled = quest.is_enrolled();
        let completed = quest.is_completed();
        let is_nitro = self.stores.account.premium_type == Some(2);
        let hovered = self.quests.hovered_tile.as_deref() == Some(quest.id.as_str());
        let hover_id = quest.id.clone();

        let hero_img = quest
            .config
            .assets_hero
            .as_deref()
            .map(|a| quest_asset(&quest.id, a))
            .and_then(|u| self.images.get(&u).cloned());
        let reward_img = quest
            .config
            .assets_reward_tile
            .as_deref()
            .or(quest.config.assets_game_tile.as_deref())
            .map(|a| quest_asset(&quest.id, a))
            .and_then(|u| self.images.get(&u).cloned());
        let logo_img = quest
            .config
            .assets_logotype_dark
            .as_deref()
            .or(quest.config.assets_logotype.as_deref())
            .map(|a| quest_asset(&quest.id, a))
            .and_then(|u| self.images.get(&u).cloned());

        let promoter = quest
            .config
            .cosponsor_name
            .as_deref()
            .unwrap_or(&quest.config.game_publisher)
            .to_owned();
        let expiry_str = format_month_day(&quest.config.expires_at);
        let ends_copy = dc_intl::format(
            "7D8r4F",
            &[(
                "expiryDate",
                Arg::Str(std::borrow::Cow::Borrowed(&expiry_str)),
            )],
        );
        let error_hints = self.quests.store.error_hints(&quest.id);
        let warning_hint = self.quests.bar.warning_override.map(|w| w.hint_text());
        let show_hint_bar = !completed
            && !quest.is_progressing_on_console(now)
            && (!error_hints.is_empty() || warning_hint.is_some());

        // Header (`container_CXA5dw`, 150px tall)
        let play_link = quest.config.cta_link.clone();
        let actions_qid = quest.id.clone();
        let details_qid = quest.id.clone();
        let video_tip = tile_video_play_tooltip(quest, now);
        let header = div()
            .relative()
            .w_full()
            .h(px(150.))
            .rounded_t(px(8.))
            .overflow_hidden()
            .bg(color("--background-surface-high"))
            // Module 918338 (`QuestTileBanner`): hero image or fallback gradient with `d/+YKd` label.
            .child(render_quest_tile_banner(
                &quest.config.quest_name,
                hero_img,
                QuestBannerVariant::Banner,
                false,
            ))
            // `overlay_CXA5dw`: smooth bottom scrim into `--background-surface-high`
            .child(div().absolute().inset_0().bg(linear_gradient(
                0.,
                linear_color_stop(color("--background-surface-high"), 0.),
                linear_color_stop(color("--background-surface-high").opacity(0.), 0.72),
            )))
            // `contents_CXA5dw`: top pills/utility buttons, partner logo, promoted-by + expiry
            .child(
                div()
                    .relative()
                    .size_full()
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    // `topRow_CXA5dw`
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .when(featured && !expired, |row| {
                                        row.child(
                                            div()
                                                .px(px(8.))
                                                .h(px(20.))
                                                .rounded_full()
                                                .bg(color("--control-primary-background-default"))
                                                .flex()
                                                .items_center()
                                                .child(
                                                    text(11., 14., 700., "--text-overlay-light")
                                                        .child(dc_intl::t("Jt6u7B")),
                                                ),
                                        )
                                    })
                                    .when(quest.preview, |row| {
                                        row.child(
                                            div()
                                                .px(px(8.))
                                                .h(px(20.))
                                                .rounded_full()
                                                .bg(color("--background-mod-strong"))
                                                .flex()
                                                .items_center()
                                                .child(
                                                    text(11., 14., 700., "--text-overlay-light")
                                                        .child(dc_intl::t("SKNnqq")),
                                                ),
                                        )
                                    }),
                            )
                            // Video play button (`utilButtonWrapper_CXA5dw`, 24x24 circle -> opens module 767868 player)
                            .when(is_video, |row| {
                                let v_qid = quest.id.clone();
                                let v_title = quest.config.quest_name.clone();
                                let v_sub = quest.config.game_title.clone();
                                let v_cta = if quest.config.cta_button_label.is_empty() {
                                    dc_intl::t("l7E81v").to_owned()
                                } else {
                                    quest.config.cta_button_label.clone()
                                };
                                row.child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "quest-tile-play-{}",
                                            quest.id
                                        )))
                                        .flex_none()
                                        .size(px(24.))
                                        .rounded_full()
                                        .bg(rgba(0x11121499))
                                        .hover(|s| s.bg(rgba(0x111214cc)))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.open_video_quest_modal(
                                                v_qid.clone(),
                                                v_title.clone(),
                                                v_sub.clone(),
                                                v_cta.clone(),
                                                play_link.clone(),
                                                cx,
                                            );
                                        }))
                                        .child(icon("clips_play", 12., "--text-overlay-light"))
                                        .when(false, |el| {
                                            el.child(video_tip).child(dc_intl::t("RscU7I"))
                                        }),
                                )
                            })
                            // Actions button (`DEoVWZ`)
                            .child(
                                div()
                                    .id(SharedString::from(format!("quest-tile-more-{}", quest.id)))
                                    .flex_none()
                                    .size(px(24.))
                                    .rounded_full()
                                    .bg(rgba(0x11121499))
                                    .hover(|s| s.bg(rgba(0x111214cc)))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(
                                        move |this, ev: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            let pos = ev.position();
                                            this.open_quest_menu(
                                                actions_qid.clone(),
                                                QuestSurface::QuestHome,
                                                true,
                                                true,
                                                true,
                                                f32::from(pos.x),
                                                f32::from(pos.y) + 6.,
                                                cx,
                                            );
                                        },
                                    ))
                                    .child(icon("more_horizontal", 14., "--text-overlay-light"))
                                    .when(false, |el| el.child(dc_intl::t("DEoVWZ"))),
                            ),
                    )
                    // Partner logotype (`partnerLogotypes_CXA5dw`)
                    .children(logo_img.map(|im| {
                        div()
                            .h(px(30.))
                            .flex()
                            .items_center()
                            .child(img(im).max_h(px(30.)).max_w(px(140.)))
                    }))
                    // `bottomRow_CXA5dw`: "Promoted by" + verified badge + publisher | "Ends M/D"
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        text(12., 16., 500., "--text-muted")
                                            .flex_none()
                                            .child(dc_intl::t("VAbKhK")),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .size(px(15.))
                                            .rounded_full()
                                            .bg(color("--status-positive"))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                "checkbox_check",
                                                9.,
                                                "--text-overlay-light",
                                            ))
                                            .when(false, |el| el.child(dc_intl::t("OfMjx9"))),
                                    )
                                    .child(
                                        text(12., 16., 500., "--text-overlay-light")
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(promoter),
                                    ),
                            )
                            .when(!expired && !claimed, |row| {
                                row.child(
                                    text(12., 16., 500., "--text-muted")
                                        .flex_none()
                                        .child(ends_copy),
                                )
                            }),
                    ),
            );

        // Optional warning/error hint strip (`W` of module 793217)
        let hint_strip = show_hint_bar.then(|| {
            let is_err = !error_hints.is_empty();
            let msg = if is_err {
                error_hints[0].clone()
            } else {
                warning_hint.unwrap_or_default().to_owned()
            };
            div()
                .w_full()
                .px(px(12.))
                .py(px(8.))
                .bg(color("--background-base-lowest"))
                .border_t_1()
                .border_color(color("--border-subtle"))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icon(
                            if is_err {
                                "standing_circle_error"
                            } else {
                                "circle_info"
                            },
                            14.,
                            if is_err {
                                "--text-feedback-critical"
                            } else {
                                "--text-default"
                            },
                        ))
                        .child(text(12., 16., 500., "--text-muted").child(msg)),
                )
                .when(is_err, |row| {
                    row.child(
                        div()
                            .id(SharedString::from(format!(
                                "quest-hint-details-{}",
                                details_qid
                            )))
                            .flex_none()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.quests.bar.expanded = true;
                                this.quests.store.set_selected_platform(
                                    &details_qid,
                                    Some(QuestPlatform::Console),
                                );
                                cx.notify();
                            }))
                            .child(
                                text(12., 16., 600., "--text-overlay-light")
                                    .hover(|s| s.underline())
                                    .child(dc_intl::t("yKJi+/")),
                            ),
                    )
                })
                .into_any_element()
        });

        // Body (`eU` of module 793217, `container_clpvxq`)
        let details = quest.task_details();
        let pct = quest
            .third_party_task_details()
            .map_or(details.percent_complete, |d| d.percent_complete);
        let pct_int = (pct * 100.).floor().clamp(0., 100.) as u64;
        let heading = dc_intl::format(
            "EAYZAr",
            &[(
                "questName",
                Arg::Str(std::borrow::Cow::Borrowed(&quest.config.quest_name)),
            )],
        );
        let is_orbs = quest.config.has_virtual_currency_reward();
        let reward_heading: AnyElement = if claimed && is_orbs {
            let orbs = quest
                .user_status
                .as_ref()
                .and_then(|s| s.orb_quantity_claimed)
                .or_else(|| quest.config.orb_quantity(is_nitro))
                .unwrap_or(0) as i64;
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(icon("pm_orbs", 14., "--text-strong"))
                .child(text(16., 20., 600., "--text-strong").child(dc_intl::format(
                    "nLXlh+",
                    &[("orbAmount", Arg::Num((orbs) as f64))],
                )))
                .into_any_element()
        } else if claimed {
            text(16., 20., 600., "--text-strong")
                .child(reward_name(quest, is_nitro, false))
                .into_any_element()
        } else if is_orbs {
            let orbs = quest.config.orb_quantity(is_nitro).unwrap_or(0) as i64;
            let orb_text = dc_intl::format("nLXlh+", &[("orbAmount", Arg::Num((orbs) as f64))]);
            let claim_text: String = dc_intl::format_runs(
                "0IUT4Y",
                &[(
                    "rewardWithArticleHook",
                    Arg::Str(std::borrow::Cow::Borrowed(&orb_text)),
                )],
            )
            .into_iter()
            .map(|(t, _)| t)
            .collect();
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(icon("pm_orbs", 14., "--text-strong"))
                .child(text(16., 20., 600., "--text-strong").child(claim_text))
                .into_any_element()
        } else {
            let with_art = reward_name(quest, is_nitro, true);
            let claim_text: String = dc_intl::format_runs(
                "0IUT4Y",
                &[(
                    "rewardWithArticleHook",
                    Arg::Str(std::borrow::Cow::Borrowed(&with_art)),
                )],
            )
            .into_iter()
            .map(|(t, _)| t)
            .collect();
            text(16., 20., 600., "--text-strong")
                .child(claim_text)
                .into_any_element()
        };
        let description = card_description(
            quest,
            InstructionsContext {
                has_nitro: is_nitro,
                without_markdown: false,
                has_game_profile_link: false,
                needs_to_connect: false,
            },
        );
        let extra_note: Option<String> = if claimed {
            redemption_instructions(quest, 0)
                .map(str::to_owned)
                .or_else(|| Some(game_cta_subtitle(quest)))
        } else {
            None
        };

        let reward_visual = if enrolled {
            progress_badge(76., pct_int, completed)
        } else {
            let reward_is_nitro = quest
                .config
                .has_reward(dc_state::quests::reward::FRACTIONAL_PREMIUM);
            let is_nitro = self.premium() == dc_state::premium::PremiumType::Nitro;
            let orb_amt = quest
                .user_status
                .as_ref()
                .and_then(|s| s.orb_quantity_claimed)
                .or_else(|| quest.config.orb_quantity(is_nitro))
                .unwrap_or(0);
            let primary_name = quest.config.primary_reward_name(false);
            let tile_label = quest_reward_tile_label(
                is_orbs,
                claimed,
                orb_amt,
                (!primary_name.is_empty()).then_some(primary_name.as_str()),
            );
            self.quest_reward_tile(
                QuestRewardTileProps {
                    id: format!("quest-reward-tile-{}", quest.id),
                    image: reward_img,
                    size_px: Some(80.),
                    is_orbs,
                    is_nitro: reward_is_nitro,
                    learn_more: Some(QuestRewardLearnMore::Text),
                    alt_label: tile_label,
                },
                Some(Box::new(move |this, cx| {
                    this.open_quests(cx);
                })),
                cx,
            )
        };

        let body = div()
            .w_full()
            .px(px(16.))
            .pt(px(12.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(reward_visual)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(text(13., 17., 500., "--text-brand").child(heading))
                            .child(reward_heading)
                            .child(text(13., 18., 400., "--text-muted").child(description))
                            .children(
                                extra_note
                                    .map(|note| text(12., 16., 400., "--text-subtle").child(note)),
                            ),
                    ),
            )
            .child(self.quest_tile_footer(quest, now, cx));

        div()
            .id(SharedString::from(format!("quest-tile-{}", quest.id)))
            .flex_none()
            .w(px(width))
            .rounded(px(10.))
            .border_1()
            .border_color(color(if hovered {
                "--border-normal"
            } else {
                "--border-subtle"
            }))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                if *over {
                    this.quests.hovered_tile = Some(hover_id.clone());
                } else if this.quests.hovered_tile.as_deref() == Some(hover_id.as_str()) {
                    this.quests.hovered_tile = None;
                }
                cx.notify();
            }))
            .child(header)
            .children(hint_strip)
            .child(body)
            .into_any_element()
    }

    /// Footer (`eD` / `ew` / `eb` / `e_` / `eQ` / `eL` of module 793217 + `452411` CTA button).
    fn quest_tile_footer(&self, quest: &Quest, now_ms: i64, cx: &mut Context<Self>) -> AnyElement {
        let state = quest.cta_state(now_ms);
        let blocked = self.quests.store.enrollment_blocked_until.is_some();

        // `ew`: Enrollment blocked while unenrolled -> Unavailable + Why?
        if state == QuestCtaState::Unenrolled && blocked {
            return div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .h(px(36.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .opacity(0.55)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            text(14., 18., 500., "--control-secondary-text-default")
                                .child(dc_intl::t("8LKchl")),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("quest-why-{}", quest.id)))
                        .flex_1()
                        .min_w(px(0.))
                        .h(px(36.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .hover(|s| s.bg(color("--control-primary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.open_url("https://support.discord.com/hc/articles/22225719947543");
                        }))
                        .child(
                            text(14., 18., 500., "--control-primary-text-default")
                                .child(dc_intl::t("vY9GgG")),
                        ),
                )
                .into_any_element();
        }

        let (has_xbox, has_psn) = self.quest_console_connections();
        let has_console_conn = has_xbox || has_psn;
        let selected = self.quests.store.selected_platform(&quest.id);
        let active_screen = quest.active_platform_screen(selected, now_ms);
        let is_progressing = quest.is_progressing_on_console(now_ms);
        let awaiting_console = quest.is_play_on_console() && !has_console_conn && !is_progressing;

        // `eb`: Enrolled console quest with platform selector (`drVw4T`, `QXc01+`, `8lAfuB`, `g6Dr44`, `iyNbj5`, `WLDKSb`)
        if state == QuestCtaState::Enrolled && quest.is_play_on_console() {
            let platforms = quest.supported_platforms();
            let selector_open =
                self.quests.open_platform_selector.as_deref() == Some(quest.id.as_str());
            let qid_toggle = quest.id.clone();
            let qid_desk = quest.id.clone();
            let qid_cons = quest.id.clone();
            let selected_chip = match active_screen {
                ActivePlatformScreen::Desktop => Some(QuestPlatform::Desktop),
                ActivePlatformScreen::Console => Some(QuestPlatform::Console),
                ActivePlatformScreen::Select => None,
            };
            let trigger_text = if active_screen == ActivePlatformScreen::Select {
                dc_intl::t("drVw4T")
            } else {
                tile_platform_option_label(selected_chip, true)
            };
            let trigger_icon = match selected_chip {
                Some(QuestPlatform::Desktop) => Some("platform_desktop"),
                Some(QuestPlatform::Console) => Some("bot_game_controller"),
                None => None,
            };

            let selector = div()
                .relative()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "quest-platform-select-{}",
                            quest.id
                        )))
                        .relative()
                        .w_full()
                        .h(px(36.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color(if active_screen == ActivePlatformScreen::Select {
                            "--control-primary-background-default"
                        } else {
                            "--control-secondary-background-default"
                        }))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.quests.open_platform_selector.as_deref()
                                == Some(qid_toggle.as_str())
                            {
                                this.quests.open_platform_selector = None;
                            } else {
                                this.quests.open_platform_selector = Some(qid_toggle.clone());
                            }
                            cx.notify();
                        }))
                        .child(self.anchor_probe(format!("quest-platform-select-{}", quest.id)))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .children(
                                    trigger_icon.map(|ic| {
                                        icon(ic, 15., "--control-secondary-icon-default")
                                    }),
                                )
                                .child(
                                    text(
                                        13.,
                                        18.,
                                        500.,
                                        if active_screen == ActivePlatformScreen::Select {
                                            "--control-primary-text-default"
                                        } else {
                                            "--control-secondary-text-default"
                                        },
                                    )
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(trigger_text),
                                ),
                        )
                        .child(icon(
                            if selector_open {
                                "automod_chevron_up"
                            } else {
                                "automod_chevron_down"
                            },
                            14.,
                            if active_screen == ActivePlatformScreen::Select {
                                "--control-primary-text-default"
                            } else {
                                "--control-secondary-icon-default"
                            },
                        )),
                )
                .when(selector_open, |wrap| {
                    let qid_out = quest.id.clone();
                    let key_out = SharedString::from(format!("quest-platform-select-{}", quest.id));
                    wrap.child(
                        deferred(
                            div()
                                .id(SharedString::from(format!(
                                    "quest-platform-dropdown-{}",
                                    quest.id
                                )))
                                .occlude()
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .on_mouse_down_out(cx.listener(
                                    move |this, event: &gpui::MouseDownEvent, _, cx| {
                                        let on_trigger = this
                                            .tip_anchors
                                            .borrow()
                                            .get(&key_out)
                                            .is_some_and(|b| b.contains(&event.position));
                                        if !on_trigger
                                            && this.quests.open_platform_selector.as_deref()
                                                == Some(qid_out.as_str())
                                        {
                                            this.quests.open_platform_selector = None;
                                            cx.notify();
                                        }
                                    },
                                ))
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(px(40.))
                                .p(px(6.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-high"))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "quest-opt-desktop-{}",
                                            quest.id
                                        )))
                                        .px(px(8.))
                                        .py(px(6.))
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.quests.store.set_selected_platform(
                                                &qid_desk,
                                                Some(QuestPlatform::Desktop),
                                            );
                                            this.quests.store.clear_error_hints(&qid_desk);
                                            this.quests.open_platform_selector = None;
                                            cx.notify();
                                        }))
                                        .child(icon("platform_desktop", 15., "--text-default"))
                                        .child(text(13., 18., 500., "--text-default").child(
                                            tile_platform_option_label(
                                                Some(QuestPlatform::Desktop),
                                                false,
                                            ),
                                        )),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "quest-opt-console-{}",
                                            quest.id
                                        )))
                                        .px(px(8.))
                                        .py(px(6.))
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.quests.store.set_selected_platform(
                                                &qid_cons,
                                                Some(QuestPlatform::Console),
                                            );
                                            this.quests.open_platform_selector = None;
                                            cx.notify();
                                        }))
                                        .child(icon("bot_game_controller", 15., "--text-default"))
                                        .child(text(13., 18., 500., "--text-default").child(
                                            tile_platform_option_label(
                                                Some(QuestPlatform::Console),
                                                false,
                                            ),
                                        )),
                                )
                                .when(false, |el| {
                                    el.child(tile_platform_option_label(None, false))
                                }),
                        )
                        .with_priority(2),
                    )
                });

            if active_screen == ActivePlatformScreen::Select {
                return div().w_full().flex().child(selector).into_any_element();
            }
            if platforms.len() > 1 {
                return div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.quest_cta_button(
                        quest,
                        CtaContext {
                            surface: CtaSurface::QuestHomeTile,
                            now_ms,
                            is_progressing,
                            awaiting_console_connections: awaiting_console,
                            active_screen,
                            has_linked_game_account: false,
                            animate_refresh_icon: true,
                        },
                        cx,
                    ))
                    .child(selector)
                    .into_any_element();
            }
        }

        // `e_`: Enrolled/Incomplete desktop-only play quest -> external CTA link full width
        if matches!(state, QuestCtaState::Enrolled | QuestCtaState::Incomplete)
            && quest.is_desktop_only_play()
            && !quest.config.cta_button_label.is_empty()
        {
            return self.quest_external_cta_button(quest, cx);
        }

        // `eQ`: Completed/Claimed activity quest -> activity launch + primary CTA
        if matches!(state, QuestCtaState::Completed | QuestCtaState::Claimed)
            && quest.is_activity_quest()
        {
            let link = quest.config.cta_link.clone();
            let short = primary_cta_label(quest, true);
            return div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .id(SharedString::from(format!("quest-activity-{}", quest.id)))
                        .flex_1()
                        .min_w(px(0.))
                        .h(px(36.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|s| s.bg(color("--control-secondary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if !link.is_empty() {
                                cx.open_url(&link);
                            }
                        }))
                        .child(icon(
                            "bot_game_controller",
                            15.,
                            "--control-secondary-icon-default",
                        ))
                        .child(
                            text(14., 18., 500., "--control-secondary-text-default").child(short),
                        ),
                )
                .child(self.quest_cta_button(
                    quest,
                    CtaContext {
                        surface: CtaSurface::QuestHomeTile,
                        now_ms,
                        is_progressing,
                        awaiting_console_connections: awaiting_console,
                        active_screen,
                        has_linked_game_account: false,
                        animate_refresh_icon: true,
                    },
                    cx,
                ))
                .into_any_element();
        }

        // `eL`: standard footer (optional external CTA + primary CTA from module 452411)
        let show_external =
            tile_shows_external_cta(state, quest) && !quest.config.cta_button_label.is_empty();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(show_external, |row| {
                row.child(self.quest_external_cta_button(quest, cx))
            })
            .child(self.quest_cta_button(
                quest,
                CtaContext {
                    surface: CtaSurface::QuestHomeTile,
                    now_ms,
                    is_progressing,
                    awaiting_console_connections: awaiting_console,
                    active_screen,
                    has_linked_game_account: false,
                    animate_refresh_icon: true,
                },
                cx,
            ))
            .into_any_element()
    }

    fn quest_external_cta_button(&self, quest: &Quest, cx: &mut Context<Self>) -> AnyElement {
        let link = quest.config.cta_link.clone();
        let label = quest.config.cta_button_label.clone();
        div()
            .id(SharedString::from(format!("quest-ext-cta-{}", quest.id)))
            .flex_1()
            .min_w(px(0.))
            .h(px(36.))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .hover(|s| s.bg(color("--control-secondary-background-hover")))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |_, _, _, cx| {
                if !link.is_empty() {
                    cx.open_url(&link);
                }
            }))
            .child(
                text(14., 18., 500., "--control-secondary-text-default")
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label),
            )
            .into_any_element()
    }

    /// Quest CTA button + optional `Play` platform dropdown (`tm` / `tc` of module 452411).
    fn quest_cta_button(
        &self,
        quest: &Quest,
        ctx: CtaContext,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let spec = resolve_cta(quest, ctx);
        let qid = quest.id.clone();
        let link = quest.config.cta_link.clone();
        let action = spec.action;
        let primary = matches!(
            spec.variant,
            CtaVariant::Primary | CtaVariant::OverlayPrimary
        );
        let menu_open = action == CtaAction::OpenPlatformMenu
            && self.quests.open_platform_menu.as_deref() == Some(quest.id.as_str());
        let (xbox, psn) = self.quest_console_connections();
        let menu_opts = if menu_open {
            platform_menu_options(
                quest,
                self.quests.store.selected_platform(&quest.id),
                None,
                xbox,
                psn,
                !self.quests.store.error_hints(&quest.id).is_empty(),
            )
        } else {
            Vec::new()
        };

        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .child(
                div()
                    .id(SharedString::from(format!("quest-cta-{}", quest.id)))
                    .relative()
                    .w_full()
                    .h(px(36.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color(if primary {
                        "--control-primary-background-default"
                    } else {
                        "--control-secondary-background-default"
                    }))
                    .when(!spec.disabled, |el| {
                        el.hover(|s| {
                            s.bg(color(if primary {
                                "--control-primary-background-hover"
                            } else {
                                "--control-secondary-background-hover"
                            }))
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.handle_quest_cta_action(&qid, &link, action, cx);
                        }))
                    })
                    .when(spec.disabled, |el| el.opacity(0.6))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(self.anchor_probe(format!("quest-cta-{}", quest.id)))
                    .child(
                        text(
                            14.,
                            18.,
                            500.,
                            if primary {
                                "--control-primary-text-default"
                            } else {
                                "--control-secondary-text-default"
                            },
                        )
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(spec.label),
                    )
                    .when(spec.trailing_external, |el| {
                        el.child(icon(
                            "link_external_small",
                            14.,
                            if primary {
                                "--control-primary-text-default"
                            } else {
                                "--control-secondary-icon-default"
                            },
                        ))
                    })
                    .when(spec.trailing_chevron, |el| {
                        el.child(icon(
                            if menu_open {
                                "automod_chevron_up"
                            } else {
                                "automod_chevron_down"
                            },
                            14.,
                            if primary {
                                "--control-primary-text-default"
                            } else {
                                "--control-secondary-icon-default"
                            },
                        ))
                    }),
            )
            .when(menu_open, |wrap| {
                let aria = platform_menu_aria_label();
                let qid_opt = quest.id.clone();
                let link_opt = quest.config.cta_link.clone();
                let qid_out = quest.id.clone();
                let key_out = SharedString::from(format!("quest-cta-{}", quest.id));
                wrap.child(
                    deferred(
                        div()
                            .id(SharedString::from(format!("quest-play-menu-{}", quest.id)))
                            .occlude()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .on_mouse_down_out(cx.listener(
                                move |this, event: &gpui::MouseDownEvent, _, cx| {
                                    let on_trigger = this
                                        .tip_anchors
                                        .borrow()
                                        .get(&key_out)
                                        .is_some_and(|b| b.contains(&event.position));
                                    if !on_trigger
                                        && this.quests.open_platform_menu.as_deref()
                                            == Some(qid_out.as_str())
                                    {
                                        this.quests.open_platform_menu = None;
                                        cx.notify();
                                    }
                                },
                            ))
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(px(40.))
                            .p(px(6.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .when(false, |el| el.child(aria))
                            .children(menu_opts.into_iter().map(move |opt| {
                                let q = qid_opt.clone();
                                let l = link_opt.clone();
                                let target = opt.target;
                                div()
                                    .id(SharedString::from(format!(
                                        "quest-play-opt-{}-{}",
                                        q, opt.id
                                    )))
                                    .px(px(8.))
                                    .py(px(6.))
                                    .rounded(px(4.))
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.quests.open_platform_menu = None;
                                        match target {
                                            super::quest_copy::PlatformMenuTarget::Desktop => {
                                                this.quests.store.set_selected_platform(
                                                    &q,
                                                    Some(QuestPlatform::Desktop),
                                                );
                                                if !l.is_empty() {
                                                    cx.open_url(&l);
                                                }
                                            }
                                            super::quest_copy::PlatformMenuTarget::Console(_) => {
                                                this.quests.store.set_selected_platform(
                                                    &q,
                                                    Some(QuestPlatform::Console),
                                                );
                                            }
                                            super::quest_copy::PlatformMenuTarget::CheckConnection => {
                                                this.quests.store.clear_error_hints(&q);
                                                this.open_quest_connection_helper(&q, true, cx);
                                            }
                                        }
                                        cx.notify();
                                    }))
                                    .child(text(13., 18., 500., "--text-default").child(opt.label))
                            })),
                    )
                    .with_priority(2),
                )
            })
            .into_any_element()
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
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_orb_wallet(cx);
                    }))
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
        let tiles = self.quest_tiles(&visible, cx);
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

    /// `s$`: a square card of the reward's picture under a gradient, the quest's logotype 20px from the bottom; on hover
    /// the gradient rises 80px, the picture dims and "{quest} Quest" / "{reward} claimed on {date}" slide up 20px from
    /// the bottom with the logotype above them (`container_HjAsMI` and the rest of 6aed4ae4 CSS).
    fn claimed_card(
        &self,
        quest: &dc_state::quests::ClaimedQuest,
        side: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group: SharedString = format!("claimed-{}", quest.id).into();
        let hovered = self.quests.claimed_hover.as_deref() == Some(quest.id.as_str());
        let hover_id = quest.id.clone();
        let picture = claimed_picture(quest).and_then(|url| self.images.get(&url).cloned());
        let logo = claimed_logo(quest).and_then(|url| self.images.get(&url).cloned());
        let reward = quest.claimed_reward();
        // `I`: an Orbs reward reads "{n} Orbs", others their name.
        let reward_name = match (
            reward["type"].as_u64(),
            quest
                .user_status
                .as_ref()
                .and_then(|s| s.orb_quantity_claimed),
        ) {
            (Some(4), Some(orbs)) => format!("{orbs} Orbs"),
            _ => reward["messages"]["name"]
                .as_str()
                .or_else(|| reward["name"].as_str())
                .unwrap_or_default()
                .to_owned(),
        };
        let date = quest
            .user_status
            .as_ref()
            .and_then(|s| s.claimed_at.as_deref())
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| {
                let at = at.with_timezone(&chrono::Local);
                format!(
                    "{}/{}",
                    chrono::Datelike::month(&at),
                    chrono::Datelike::day(&at)
                )
            })
            .unwrap_or_default();
        let overlay_top = side / 2.;
        div()
            .id(group)
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                if *over {
                    this.quests.claimed_hover = Some(hover_id.clone());
                } else if this.quests.claimed_hover.as_deref() == Some(hover_id.as_str()) {
                    this.quests.claimed_hover = None;
                }
                cx.notify();
            }))
            .relative()
            .w(px(side))
            .h(px(side))
            .rounded(px(16.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .children(picture.map(|picture| {
                div()
                    .absolute()
                    .inset_0()
                    .when(hovered, |this| this.opacity(0.8))
                    .child(img(picture).size_full().object_fit(gpui::ObjectFit::Cover))
            }))
            // `overlay_HjAsMI` with `darkThemeGradient`: `--primary-660` at the bottom, half at the middle, clear on top.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(if hovered {
                        overlay_top - 80.
                    } else {
                        overlay_top
                    }))
                    .h(px(side))
                    .bg(linear_gradient(
                        0.,
                        linear_color_stop(color("--primary-660"), 0.),
                        linear_color_stop(color("--primary-660").opacity(0.), 1.),
                    )),
            )
            .children(logo.map(|logo| {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(if hovered { 84. } else { 20. }))
                    .flex()
                    .justify_center()
                    .child(img(logo).max_h(px(45.)).max_w(px(140.)))
            }))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(if hovered { 20. } else { -60. }))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    .child(
                        div()
                            .mb(px(4.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(gpui::FontWeight(600.))
                            .text_color(color("--text-overlay-light"))
                            .child(format!("{} Quest", quest.quest_name)),
                    )
                    .child(
                        div()
                            .opacity(0.75)
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(gpui::FontWeight(500.))
                            .text_color(color("--text-overlay-light"))
                            .child(format!("{reward_name} claimed on {date}")),
                    ),
            )
            .into_any_element()
    }

    /// `sZ`: the claimed quests, or the spinner while they load, or the note with its art.
    fn quests_claimed_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let claimed = self.quests.store.claimed_by_date();
        let loading = self.quests.store.is_fetching_claimed_quests && claimed.is_empty();
        let empty = claimed.is_empty() && !loading;
        let art = self.images.get(CLAIMED_IMAGE).cloned();
        // `gridContainer_5UwkbV`: columns of at least 230px, 24px apart, filling the page; a card is square up to 330.
        let width = (self.viewport.0 - self.combined_sidebar_width() - 40.).max(230.);
        let columns = ((width + 24.) / (230. + 24.)).floor().max(1.);
        let side = ((width - (columns - 1.) * 24.) / columns).min(330.);
        let cards: Vec<AnyElement> = claimed
            .iter()
            .map(|quest| self.claimed_card(quest, side, cx))
            .collect();
        div()
            .w_full()
            .px(px(20.))
            .py(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .when(!cards.is_empty(), |this| {
                this.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .gap(px(24.))
                        .children(cards),
                )
            })
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

/// A quest asset's address (`N` of module 760914): a path as it is, a bare name under the quest's folder.
pub(super) fn quest_asset_url(quest: &str, asset: &str) -> String {
    quest_asset(quest, asset)
}

impl Root {
    /// Opens the in-app Video Quest Player modal (module 767868 / 579666).
    pub(super) fn open_video_quest_modal(
        &mut self,
        quest_id: String,
        title: String,
        subtitle: String,
        cta_label: String,
        cta_link: String,
        cx: &mut Context<Self>,
    ) {
        let found = self.quests.store.quests.iter().find(|q| q.id == quest_id);
        let completed = found.is_some_and(|q| {
            q.user_status
                .as_ref()
                .is_some_and(|s| s.completed_at.is_some())
        });
        // Module 700085 (`321538.es`): opening an uncompleted video quest while quest access is
        // suspended opens the Quests access suspended modal instead of starting the quest.
        if self.quests.store.is_quest_access_suspended() && !completed {
            self.quests.access_suspended_modal = true;
            cx.notify();
            return;
        }
        let reward_name = found
            .and_then(|q| q.config.rewards.first())
            .map(|r| r.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| title.clone());
        let publisher = found
            .map(|q| q.config.game_publisher.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| subtitle.clone());
        let cta_subtitle = cta_label.clone();
        let reward_image = found
            .and_then(|q| q.config.assets_reward_tile.as_deref())
            .filter(|s| !s.is_empty())
            .map(|asset| quest_asset(&quest_id, asset));
        let claimed = found.is_some_and(|q| {
            q.user_status
                .as_ref()
                .is_some_and(|s| s.claimed_at.is_some())
        });
        if let Some(url) = &reward_image {
            self.request_image(format!("quest-reward:{quest_id}"), url.clone());
        }
        self.quests.video_modal = Some(VideoQuestModal {
            quest_id,
            title,
            subtitle,
            cta_label,
            cta_link,
            ended: false,
            unsupported_fallback: false,
            watch_secs: 0,
            duration_secs: 30,
            bounty_orbs: None,
            paused_away: false,
            claim_error: false,
            reward_name,
            publisher,
            cta_subtitle,
            reward_image,
            claimed,
            progress_hovered: false,
            dvp: super::video_player::DvpControlState::for_video(0, 30, false),
        });
        cx.notify();
    }

    /// Claims a completed video bounty reward (`POST /quests/creatives/{bountyId}/claim-reward`,
    /// module 579666 / 845890.Hq).
    fn claim_video_bounty_reward(&self, bounty_id: &str) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("quests:bounty-claim:{bounty_id}")),
            method: reqwest::Method::POST,
            route: format!("/quests/creatives/{bounty_id}/claim-reward"),
            body: Some(serde_json::json!({})),
        });
    }

    /// Shared Quest Claim Reward button with lock icon & tooltips (module 612378: `WYchde`
    /// "Explore the Shop", `vTgCWx` "View Reward", `cfY4PE` "Claim Reward", `NJ6Bnm`
    /// "Not eligible to claim", `USNO1K` "Complete the video to unlock and claim!").
    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui) fn quest_claim_reward_button(
        &self,
        id: &'static str,
        quest_id: String,
        completed: bool,
        claimed: bool,
        is_orbs: bool,
        is_expired: bool,
        is_video: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = if claimed {
            if is_orbs {
                dc_intl::t("WYchde")
            } else {
                dc_intl::t("vTgCWx")
            }
        } else {
            dc_intl::t("cfY4PE")
        };
        let disabled = !completed && !claimed;
        let tip_text = if !completed && is_expired {
            Some(dc_intl::t("NJ6Bnm"))
        } else if !completed && is_video {
            Some(dc_intl::t("USNO1K"))
        } else {
            None
        };
        let btn = div()
            .id(id)
            .px(px(12.))
            .h(px(32.))
            .rounded(px(6.))
            .bg(color("--control-secondary-background-default"))
            .when(!disabled, |b| {
                b.cursor_pointer()
                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quest_activity_header = None;
                        this.quests.video_modal = None;
                        if claimed && is_orbs {
                            this.quests_open_orbs_shop(cx);
                        } else {
                            this.open_quest_reward_modal(&quest_id, cx);
                        }
                    }))
            })
            .when(disabled, |b| b.opacity(0.55))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .when(!claimed, |b| {
                b.child(icon(
                    "access_lock",
                    13.,
                    if completed {
                        "--icon-strong"
                    } else {
                        "--icon-muted"
                    },
                ))
            })
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(label),
            );
        if let Some(tip) = tip_text {
            self.with_tip(
                format!("{id}-tip"),
                super::tooltip::Tip::text(tip, super::tooltip::Side::Top),
                btn,
                cx,
            )
            .into_any_element()
        } else {
            btn.into_any_element()
        }
    }

    /// Reports video quest watch progress (`POST /quests/{questId}/video-progress`, module 343559.uI / 767868).
    fn report_video_quest_progress(&self, quest_id: &str, timestamp: u32) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("quests:video-progress:{quest_id}")),
            method: reqwest::Method::POST,
            route: format!("/quests/{quest_id}/video-progress"),
            body: Some(serde_json::json!({ "timestamp": timestamp })),
        });
    }

    /// Renders the Video Quest Player modal, persistent top-right share overlay (`WmfZHZ`/`RDE0Sc`/`+5kSoW`),
    /// unsupported fallback (`rStN/7` desktop / `E1du/q` web), and end screen (`mxaHfx`, module 767868).
    pub(in crate::ui) fn video_quest_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.quests.video_modal.as_ref()?;
        let _share_aria = dc_intl::t("RDE0Sc");
        let _web_unsupported = dc_intl::t("E1du/q");
        let share_url = format!("https://discord.com/quests/{}", modal.quest_id);
        let qid_progress = modal.quest_id.clone();
        let duration = modal.duration_secs.max(1);
        let remaining = duration.saturating_sub(modal.watch_secs);
        let pct = ((modal.watch_secs as f32) / (duration as f32)).clamp(0., 1.);
        let cta_link = modal.cta_link.clone();
        let cta_link_footer = modal.cta_link.clone();
        let end_subtitle = if modal.subtitle.is_empty() {
            dc_intl::t("mxaHfx").to_owned()
        } else {
            modal.subtitle.clone()
        };
        let is_bounty = modal.bounty_orbs.is_some();
        let bounty_orbs = modal.bounty_orbs.unwrap_or(50);
        let orbs_balance = self.quests.currency.balance.unwrap_or(750);
        let timer_label = if remaining == 0 {
            dc_intl::t("0F4Peo").to_owned()
        } else {
            dc_intl::format("45cddS", &[("seconds", Arg::Num(f64::from(remaining)))])
        };
        let orbs_target_label = dc_intl::format(
            "nLXlh+",
            &[(
                "orbAmount",
                Arg::Str(std::borrow::Cow::Owned(bounty_orbs.to_string())),
            )],
        );

        // Module 579666 persistent top-left cluster: 32x32 circular watch-time timer (`0F4Peo`/`45cddS`)
        // + Orbs balance pill.
        let top_left_bounty = is_bounty.then(|| {
            div()
                .absolute()
                .top(px(14.))
                .left(px(14.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .id("video-quest-watch-timer")
                        .aria_label(timer_label.clone())
                        .px(px(10.))
                        .h(px(32.))
                        .rounded_full()
                        .bg(rgba(0x111214cc))
                        .border_1()
                        .border_color(if remaining == 0 {
                            color("--status-positive")
                        } else {
                            rgba(0xffffff33)
                        })
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(if remaining == 0 {
                            icon("circle_check", 16., "--status-positive").into_any_element()
                        } else {
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight(700.))
                                .text_color(color("--white"))
                                .child(format!("{remaining}s"))
                                .into_any_element()
                        })
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight(500.))
                                .text_color(color("--white"))
                                .child(timer_label),
                        ),
                )
                .child(
                    div()
                        .h(px(32.))
                        .px(px(10.))
                        .rounded_full()
                        .bg(rgba(0x111214cc))
                        .border_1()
                        .border_color(rgba(0xffffff26))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .child(icon("shop_orb_18", 14., "--white"))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight(600.))
                                .text_color(color("--white"))
                                .child(orbs_balance.to_string()),
                        ),
                )
        });

        // Persistent top-right overlay (`ts` of module 767868 + close button `491812` `cpT0Cq`).
        let top_overlay = div()
            .absolute()
            .top(px(14.))
            .right(px(14.))
            .flex()
            .items_center()
            .gap(px(8.))
            .when(!is_bounty, |row| {
                row.child(
                    self.with_tip(
                        "video-quest-share-tip",
                        super::tooltip::Tip::text(
                            dc_intl::t("WmfZHZ"),
                            super::tooltip::Side::Bottom,
                        ),
                        div()
                            .id("video-quest-share-btn")
                            .size(px(34.))
                            .rounded_full()
                            .bg(rgba(0x111214cc))
                            .hover(|s| s.bg(rgba(0x111214ee)))
                            .border_1()
                            .border_color(rgba(0xffffff26))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("clips_link", 16., "--white"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    share_url.clone(),
                                ));
                                this.put_app_toast(dc_intl::t("+5kSoW").to_owned(), true, cx);
                                cx.notify();
                            })),
                        cx,
                    ),
                )
            })
            .child(
                // Module 491812 (`ad-video-modal-close-btn`, `cpT0Cq` "Close")
                self.with_tip(
                    "ad-video-modal-close-tip",
                    super::tooltip::Tip::text(dc_intl::t("cpT0Cq"), super::tooltip::Side::Bottom),
                    div()
                        .id("ad-video-modal-close-btn")
                        .size(px(34.))
                        .rounded_full()
                        .bg(rgba(0x111214cc))
                        .hover(|s| s.bg(rgba(0x111214ee)))
                        .border_1()
                        .border_color(rgba(0xffffff26))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("modal_close", 16., "--white"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.quests.video_modal = None;
                            cx.notify();
                        })),
                    cx,
                ),
            );

        let viewport_body: AnyElement = if modal.unsupported_fallback {
            div()
                .flex_1()
                .px(px(40.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .child(icon("circle_info", 36., "--text-muted"))
                .child(
                    div()
                        .max_w(px(420.))
                        .text_center()
                        .text_size(px(15.))
                        .line_height(px(22.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("rStN/7")),
                )
                .into_any_element()
        } else if modal.ended {
            // End Screen (`w` of module 767868, CSS 831056): dark scrim, centered clickable CTA card,
            // vertical 2x40 divider (`#747783` @ 30%), and ArrowLargeRightIcon.
            div()
                .flex_1()
                .bg(rgba(0x0b0c10dd))
                .flex()
                .items_center()
                .justify_center()
                .p(px(28.))
                .child(
                    div()
                        .id("video-quest-end-card")
                        .px(px(24.))
                        .py(px(18.))
                        .rounded(px(12.))
                        .bg(rgba(0x1e1f22ee))
                        .border_1()
                        .border_color(rgba(0xffffff22))
                        .hover(|s| s.bg(rgba(0x2b2d31ff)))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(18.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !cta_link.is_empty() {
                                cx.open_url(&cta_link);
                            }
                            this.quests.video_modal = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(gpui::FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(modal.cta_label.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .font_weight(gpui::FontWeight(500.))
                                        .text_color(color("--text-muted"))
                                        .child(end_subtitle),
                                ),
                        )
                        // Vertical 2x40 divider (`#747783` @ 0.30 opacity)
                        .child(div().flex_none().w(px(2.)).h(px(40.)).bg(rgba(0x7477834d)))
                        .child(icon("shop_arrow_right", 22., "--white")),
                )
                .into_any_element()
        } else {
            // Active video quest player canvas + progress bar + Complete/End preview trigger.
            div()
                .flex_1()
                .flex()
                .flex_col()
                .justify_between()
                .p(px(24.))
                .when(is_bounty, |col| col.pt(px(56.)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .when(!is_bounty, |col| {
                            col.child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(gpui::FontWeight(700.))
                                    .text_color(color("--white"))
                                    .child(modal.title.clone()),
                            )
                        })
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(color("--text-muted"))
                                .child(format!("{}s / {}s", modal.watch_secs, duration)),
                        ),
                )
                .child(
                    div().flex_1().flex().items_center().justify_center().child(
                        div()
                            .id("video-quest-play-trigger")
                            .size(px(64.))
                            .rounded_full()
                            .bg(rgba(0x5865f2ee))
                            .hover(|s| s.bg(rgba(0x4752c4ff)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("clips_play", 24., "--white"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if is_bounty {
                                    this.claim_video_bounty_reward(&qid_progress);
                                } else {
                                    this.report_video_quest_progress(&qid_progress, duration + 1);
                                }
                                if let Some(v) = this.quests.video_modal.as_mut() {
                                    v.watch_secs = v.duration_secs;
                                    v.paused_away = false;
                                    v.ended = true;
                                    v.dvp.current_secs = v.duration_secs;
                                    v.dvp.max_watched_secs = v.duration_secs;
                                    v.dvp.ended = true;
                                    v.dvp.playing = false;
                                }
                                cx.notify();
                            })),
                    ),
                )
                .when(modal.paused_away, |col| {
                    // Module 755372 / 579666 pause-on-unfocus notice (`w6PeBF`)
                    col.child(
                        div()
                            .mb(px(8.))
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .bg(rgba(0x111214dd))
                            .border_1()
                            .border_color(rgba(0xffffff22))
                            .text_size(px(12.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("w6PeBF")),
                    )
                })
                .when(modal.claim_error, |col| {
                    // Module 579666 claim failure banner (`uLjCfn`)
                    col.child(
                        div()
                            .mb(px(8.))
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .bg(rgba(0xda373c33))
                            .border_1()
                            .border_color(color("--status-danger"))
                            .text_size(px(12.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("uLjCfn")),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .when(is_bounty, |bar| {
                            bar.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(4.))
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(icon("shop_orb_18", 14., "--white"))
                                    .child(orbs_target_label),
                            )
                        })
                        .child(
                            div()
                                .w_full()
                                .h(px(6.))
                                .rounded_full()
                                .bg(rgba(0xffffff26))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui::relative(pct))
                                        .bg(color("--control-primary-background-default")),
                                ),
                        )
                        .child({
                            // Module 40705 `discord-web-video-player` 3-group control bar
                            let on_mutate: super::video_player::DvpMutator =
                                std::rc::Rc::new(|this, apply, cx| {
                                    if let Some(v) = this.quests.video_modal.as_mut() {
                                        apply(&mut v.dvp);
                                        v.watch_secs = v.dvp.current_secs;
                                        v.ended = v.dvp.ended;
                                    }
                                    cx.notify();
                                });
                            self.render_dvp_control_bar(
                                "quest-dvp",
                                &modal.dvp,
                                super::video_player::DvpSize::Md,
                                false,
                                false,
                                false,
                                None,
                                on_mutate,
                                cx,
                            )
                        }),
                )
                .when(modal.dvp.transcript_open && !modal.ended, |col| {
                    // Module 40705 `transcriptCont_hFq1lM`: 363px panel (`VZkd/n` "Transcript" + `6/dkHh` close)
                    col.child(
                        div()
                            .id("quest-dvp-transcript-drawer")
                            .occlude()
                            .absolute()
                            .right(px(16.))
                            .bottom(px(108.))
                            .w(px(363.))
                            .p(px(14.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(gpui::FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("VZkd/n")),
                                    )
                                    .child(
                                        self.with_tip(
                                            "quest-dvp-transcript-close-tip".to_owned(),
                                            super::tooltip::Tip::text(
                                                dc_intl::t("6/dkHh"),
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id("quest-dvp-transcript-close")
                                                .size(px(24.))
                                                .rounded(px(6.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor_pointer()
                                                .hover(|s| {
                                                    s.bg(color("--interactive-background-hover"))
                                                })
                                                .child(icon("close", 14., "--icon-default"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    if let Some(v) =
                                                        this.quests.video_modal.as_mut()
                                                    {
                                                        v.dvp.transcript_open = false;
                                                    }
                                                    cx.notify();
                                                })),
                                            cx,
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(modal.title.clone()),
                            ),
                    )
                })
                .into_any_element()
        };

        // Footer: module 491147 (`U` 48x48 reward progress ring + `T` advertiser details `EQa7os` / `EuHF34`)
        // for standard video quests; module 579666 (`o6FLcF` Promoted) for bounty ads + module 612378 Claim button.
        let completed = remaining == 0;
        let pct_int = video_quest_progress_percent(
            modal.watch_secs as f64,
            modal.duration_secs as f64,
            completed,
        );
        let footer_left: AnyElement = if is_bounty {
            let bounty_link = modal.cta_link.clone();
            let bounty_details = self.quest_advertiser_details(
                "bounty-quest-advertiser",
                modal.title.clone(),
                dc_intl::t("o6FLcF").to_owned(),
                if modal.cta_subtitle.is_empty() {
                    None
                } else {
                    Some(modal.cta_subtitle.clone())
                },
                None,
                true,
                Box::new(move |this, cx| {
                    if !bounty_link.is_empty() {
                        this.follow_link(&bounty_link, "", cx);
                    }
                }),
                cx,
            );
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .flex_none()
                        .size(px(36.))
                        .rounded(px(8.))
                        .bg(rgba(0x5865f233))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("home_quests", 18., "--white")),
                )
                .child(bounty_details)
                .into_any_element()
        } else {
            let reward_tip =
                video_quest_reward_tooltip(completed, modal.claimed, &modal.reward_name);
            let can_claim_ring = completed && !modal.claimed;
            let ring_qid = modal.quest_id.clone();
            let reward_pic = modal
                .reward_image
                .as_deref()
                .and_then(|_| self.images.get(&format!("quest-reward:{}", modal.quest_id)))
                .cloned();
            let ring_border = if completed {
                color("--status-positive")
            } else {
                color("--control-primary-background-default")
            };
            let progress_ring = self.with_tip(
                "video-quest-reward-ring-tip".to_owned(),
                super::tooltip::Tip::body(
                    super::tooltip::Body::Text(SharedString::from(reward_tip)),
                    super::tooltip::Side::Top,
                ),
                div()
                    .id("video-quest-reward-ring")
                    .relative()
                    .mt(px(5.))
                    .size(px(48.))
                    .rounded_full()
                    .border_2()
                    .border_color(ring_border)
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .when(can_claim_ring, |el| {
                        el.cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.open_quest_reward_modal(&ring_qid, cx);
                            }))
                    })
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if let Some(v) = this.quests.video_modal.as_mut() {
                            v.progress_hovered = *hovered;
                            cx.notify();
                        }
                    }))
                    .child(match reward_pic {
                        Some(pic) => gpui::img(pic)
                            .size(px(36.))
                            .rounded_full()
                            .into_any_element(),
                        None => icon("home_quests", 20., "--white").into_any_element(),
                    })
                    .when(modal.progress_hovered, |el| {
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .bg(rgba(0x000000b3))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(gpui::FontWeight(500.))
                                .text_color(color("--white"))
                                .child(format!("{pct_int}%")),
                        )
                    }),
                cx,
            );
            let adv_link = modal.cta_link.clone();
            let adv_title = dc_intl::format(
                "EQa7os",
                &[("questName", dc_intl::Arg::Str(modal.title.as_str().into()))],
            );
            let (adv_sub, adv_cta) = if modal.publisher.is_empty() {
                (modal.cta_subtitle.clone(), None)
            } else if modal.cta_subtitle.is_empty() {
                (modal.publisher.clone(), None)
            } else {
                (modal.publisher.clone(), Some(modal.cta_subtitle.clone()))
            };
            let advertiser = self.quest_advertiser_details(
                "video-quest-advertiser",
                adv_title,
                adv_sub,
                adv_cta,
                Some("circle_check"),
                false,
                Box::new(move |this, cx| {
                    if !adv_link.is_empty() {
                        this.follow_link(&adv_link, "", cx);
                    }
                }),
                cx,
            );
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(progress_ring)
                .child(advertiser)
                .into_any_element()
        };
        let footer = div()
            .px(px(20.))
            .py(px(12.))
            .bg(color("--background-surface-high"))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(footer_left)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.quest_claim_reward_button(
                        "video-quest-claim-btn",
                        modal.quest_id.clone(),
                        completed,
                        modal.claimed,
                        is_bounty,
                        false,
                        true,
                        cx,
                    ))
                    .child(
                        div()
                            .id("video-quest-footer-cta")
                            .cursor_pointer()
                            .px(px(14.))
                            .h(px(32.))
                            .rounded(px(6.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight(600.))
                            .text_color(color("--white"))
                            .child(modal.cta_label.clone())
                            .on_click(cx.listener(move |_, _, _, cx| {
                                if !cta_link_footer.is_empty() {
                                    cx.open_url(&cta_link_footer);
                                }
                            })),
                    ),
            );

        let dialog = div()
            .id("video-quest-modal")
            .occlude()
            .relative()
            .w(px(720.))
            .h(px(465.))
            .rounded(px(12.))
            .bg(rgba(0x111214ff))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if let Some(v) = this.quests.video_modal.as_mut()
                    && v.dvp.handle_shortcut(event.keystroke.key.as_str())
                {
                    v.watch_secs = v.dvp.current_secs;
                    v.ended = v.dvp.ended;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(viewport_body)
                    .children(top_left_bounty)
                    .child(top_overlay),
            )
            .child(footer);

        Some(
            deferred(
                div()
                    .id("video-quest-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000c8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quests.video_modal = None;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Capture harness: `video-quest [playing|end|unsupported|bounty|bounty-earned|bounty-error|close]` (modules 767868, 579666, 491812, 612378).
    #[cfg(feature = "capture")]
    pub(crate) fn script_video_quest(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.quests.video_modal = None;
                cx.notify();
                "video-quest: closed".to_owned()
            }
            mode => {
                let is_bounty = mode.starts_with("bounty");
                let earned = mode == "bounty-earned" || mode == "end";
                self.quests.video_modal = Some(VideoQuestModal {
                    quest_id: "q_767868".to_owned(),
                    title: "Starfall Odyssey Launch Trailer".to_owned(),
                    subtitle: String::new(),
                    cta_label: "Explore Starfall Odyssey".to_owned(),
                    cta_link: "https://discord.com/quests".to_owned(),
                    ended: mode == "end",
                    unsupported_fallback: mode == "unsupported",
                    watch_secs: if earned { 30 } else { 12 },
                    duration_secs: 30,
                    bounty_orbs: is_bounty.then_some(50),
                    paused_away: mode == "bounty",
                    claim_error: mode == "bounty-error",
                    reward_name: "Starfall Avatar Decoration".to_owned(),
                    publisher: "Nebula Studios".to_owned(),
                    cta_subtitle: "Learn more".to_owned(),
                    reward_image: None,
                    claimed: false,
                    progress_hovered: mode == "ring-hover",
                    dvp: {
                        let mut d = super::video_player::DvpControlState::for_video(
                            if earned { 30 } else { 12 },
                            30,
                            mode == "end",
                        );
                        if mode == "speed" {
                            d.speed_open = true;
                            d.playback_rate = 1.5;
                        }
                        if mode == "transcript" {
                            d.transcript_open = true;
                        }
                        d
                    },
                });
                cx.notify();
                format!("video-quest: {mode}")
            }
        }
    }

    /// Module 637691 — Video Quest / Bounty Advertiser Header Details CTA (`Check it out!`).
    /// Hover surface `--background-mod-subtle` (`10px` radius, `4px 10px 4px 8px` padding, flush
    /// `-4px -10px -4px -8px`), `EuHF34` tooltip, optional prefix icon, and 4x4px `--border-strong`
    /// circular dot spacer between `subtitle` and `subtitle_cta`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn quest_advertiser_details(
        &self,
        id: &'static str,
        title: String,
        subtitle: String,
        subtitle_cta: Option<String>,
        prefix_icon: Option<&'static str>,
        is_flush: bool,
        on_click: QuestClick,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let box_el = div()
            .id(id)
            .min_w_0()
            .when(is_flush, |d| {
                d.mt(px(-4.)).mr(px(-10.)).mb(px(-4.)).ml(px(-8.))
            })
            .pt(px(4.))
            .pr(px(10.))
            .pb(px(4.))
            .pl(px(8.))
            .rounded(px(10.))
            .hover(|s| s.bg(color("--background-mod-subtle")))
            .cursor_pointer()
            .flex()
            .flex_col()
            .gap(px(4.))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                on_click(this, cx);
                cx.notify();
            }))
            .child(
                div()
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(gpui::FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when_some(prefix_icon, |row, glyph| {
                        row.child(icon(glyph, 14., "--icon-muted"))
                    })
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(gpui::FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .child(subtitle),
                    )
                    .when_some(subtitle_cta.filter(|s| !s.is_empty()), |row, cta| {
                        row.child(
                            div()
                                .flex_none()
                                .mx(px(2.))
                                .size(px(4.))
                                .rounded_full()
                                .bg(color("--border-strong")),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(gpui::FontWeight(500.))
                                .text_color(color("--text-subtle"))
                                .child(cta),
                        )
                    }),
            );
        self.with_tip(
            format!("{id}-tip"),
            super::tooltip::Tip::text(dc_intl::t("EuHF34"), super::tooltip::Side::Top),
            box_el,
            cx,
        )
        .into_any_element()
    }

    /// Module 639701 — Shared `QuestRewardTile`: `64x64` default (or custom/full-width),
    /// `6px 12px 32px rgba(0,0,0,.24)` drop shadow, `1px rgba(255,255,255,.16)` highlight rim,
    /// Nitro fractional visual variant, and optional `learnMoreStyle` (`Text` `DYAleT` or `Icon`)
    /// hover overlay.
    pub(super) fn quest_reward_tile(
        &self,
        props: QuestRewardTileProps,
        on_click: Option<QuestClick>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let size = props.size_px.unwrap_or(64.);
        let id_str = props.id.clone();
        let has_learn = props.learn_more.is_some();
        let has_image = props.image.is_some();
        let tile = div()
            .id(SharedString::from(id_str))
            .group("quest-reward-tile")
            .relative()
            .flex_none()
            .size(px(size))
            .rounded(px(if has_learn { 8. } else { 4. }))
            .overflow_hidden()
            .border_1()
            .border_color(rgba(0xffffff29))
            .bg(if props.is_nitro {
                gpui::linear_gradient(
                    135.,
                    gpui::linear_color_stop(rgba(0x5865f2ff), 0.),
                    gpui::linear_color_stop(rgba(0xeb459eff), 1.),
                )
            } else {
                color("--background-base-lowest").into()
            })
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(6.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .items_center()
            .justify_center()
            .when_some(on_click, |d, handler| {
                d.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        handler(this, cx);
                        cx.notify();
                    }))
            })
            .children(
                props
                    .image
                    .map(|im| img(im).size_full().object_fit(gpui::ObjectFit::Cover)),
            )
            .when(!has_image, |d| {
                d.child(icon(
                    if props.is_nitro {
                        "nitro_wheel"
                    } else if props.is_orbs {
                        "pm_orbs"
                    } else {
                        "home_quests"
                    },
                    (size * 0.42).clamp(20., 40.),
                    if props.is_nitro {
                        "--white"
                    } else {
                        "--text-brand"
                    },
                ))
            })
            .when_some(props.learn_more, |d, style| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x00000080))
                        .opacity(0.)
                        .group_hover("quest-reward-tile", |s| s.opacity(1.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .child(match style {
                            QuestRewardLearnMore::Text => div()
                                .text_center()
                                .text_size(px(11.))
                                .line_height(px(13.))
                                .font_weight(gpui::FontWeight(600.))
                                .text_color(color("--white"))
                                .child(quest_reward_learn_more_text())
                                .into_any_element(),
                            QuestRewardLearnMore::Icon => {
                                icon("info_circle", 16., "--white").into_any_element()
                            }
                        }),
                )
            });
        self.with_tip(
            format!("{}-alt-tip", props.id),
            super::tooltip::Tip::body(
                super::tooltip::Body::Text(SharedString::from(props.alt_label)),
                super::tooltip::Side::Top,
            ),
            tile,
            cx,
        )
        .into_any_element()
    }
}

type QuestClick = Box<dyn Fn(&mut Root, &mut Context<Root>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum QuestRewardLearnMore {
    Text,
    Icon,
}

pub(super) struct QuestRewardTileProps {
    pub id: String,
    pub image: Option<std::sync::Arc<gpui::Image>>,
    pub size_px: Option<f32>,
    pub is_orbs: bool,
    pub is_nitro: bool,
    pub learn_more: Option<QuestRewardLearnMore>,
    pub alt_label: String,
}

/// Module 918338 (`QuestTileBanner`): layout variant (`banner` vs `modalPanel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuestBannerVariant {
    Banner,
    ModalPanel,
}

/// Module 918338 (`k()`): computes hero image request dimensions `{ width, height }` for
/// `width = 454 * min(2, dpr)` and `aspect = 2640 / 740`.
#[allow(dead_code)]
pub(super) fn quest_hero_image_size(dpr: f32) -> (f32, f32) {
    let w = 454.0 * dpr.clamp(1.0, 2.0);
    let h = w / (2640.0 / 740.0);
    (w, h)
}

/// Module 918338 (`d/+YKd`): `"{questName} quest banner"` accessible label / alt text.
pub(super) fn quest_banner_alt(quest_name: &str) -> String {
    if quest_name.trim().is_empty() {
        dc_intl::t("P84bAD").to_owned()
    } else {
        dc_intl::format("d/+YKd", &[("questName", quest_name.into())])
    }
}

/// Module 918338 (`QuestTileBanner` `A`, CSS `271182`): renders the shared Quest hero banner
/// container for both `Banner` (full-bleed tile header) and `ModalPanel` (`max-h: 190px` with
/// vertical fade scrim into `--background-base-lowest`), supporting `hide_assets` and `d/+YKd`.
pub(super) fn render_quest_tile_banner(
    quest_name: &str,
    hero_img: Option<std::sync::Arc<gpui::Image>>,
    variant: QuestBannerVariant,
    hide_assets: bool,
) -> AnyElement {
    let alt = quest_banner_alt(quest_name);
    let mut base = div()
        .id(SharedString::from(format!("quest-banner-{quest_name}")))
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .bg(color("--background-base-low"))
        .aria_label(SharedString::from(alt));

    base = match variant {
        QuestBannerVariant::Banner => base.absolute().inset_0(),
        QuestBannerVariant::ModalPanel => base.relative().max_h(px(190.)).h(px(190.)),
    };

    if hide_assets {
        return base.into_any_element();
    }

    let media = match hero_img {
        Some(im) => img(im)
            .size_full()
            .object_fit(gpui::ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .size_full()
            .bg(linear_gradient(
                135.,
                linear_color_stop(rgba(0x5865f244), 0.),
                linear_color_stop(rgba(0x1e1f22ff), 1.),
            ))
            .into_any_element(),
    };

    base.child(media)
        .when(variant == QuestBannerVariant::ModalPanel, |d| {
            d.child(div().absolute().inset_0().bg(linear_gradient(
                0.,
                linear_color_stop(color("--background-base-lowest"), 0.),
                linear_color_stop(color("--background-base-lowest").opacity(0.), 0.85),
            )))
        })
        .into_any_element()
}

/// Module 639701: resolves `QuestRewardTile` alt label (`nLXlh+` `"{orbAmount} Orbs"` for claimed
/// virtual currency quests, else reward name or `UMclVN` `"Quest reward"`).
pub(super) fn quest_reward_tile_label(
    is_orbs: bool,
    claimed: bool,
    orb_amount: u64,
    reward_name: Option<&str>,
) -> String {
    if is_orbs && claimed {
        return dc_intl::format("nLXlh+", &[("orbAmount", (orb_amount as f64).into())]);
    }
    reward_name
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| dc_intl::t("UMclVN").to_owned())
}

/// Module 639701: `DYAleT` `"Learn\nMore"` hover overlay copy.
pub(super) fn quest_reward_learn_more_text() -> String {
    let raw = dc_intl::t("DYAleT");
    if raw.trim().is_empty() {
        "Learn\nMore".to_owned()
    } else {
        raw.replace("<p>", "")
            .replace("</p>", "\n")
            .trim()
            .to_owned()
    }
}

fn quest_asset(quest: &str, asset: &str) -> String {
    if asset.starts_with("http://") || asset.starts_with("https://") {
        asset.to_owned()
    } else if asset.contains('/') {
        format!("https://cdn.discordapp.com/{asset}")
    } else {
        format!("https://cdn.discordapp.com/quests/{quest}/{asset}")
    }
}

/// The claimed reward's still picture (`reward`: the video when there is one; this client shows the picture).
fn claimed_picture(quest: &dc_state::quests::ClaimedQuest) -> Option<String> {
    let reward = quest.claimed_reward();
    reward["asset"]
        .as_str()
        .map(|asset| quest_asset(&quest.id, asset))
}

/// `logo_type` in the dark theme: `logotype_dark`, else `logotype`.
fn claimed_logo(quest: &dc_state::quests::ClaimedQuest) -> Option<String> {
    let assets = &quest.raw["config"]["assets"];
    assets["logotype_dark"]
        .as_str()
        .or_else(|| assets["logotype"].as_str())
        .map(|asset| quest_asset(&quest.id, asset))
}

impl Root {
    pub(super) fn close_quests_select(&mut self) -> bool {
        let open = self.quests.open_platform_selector.is_some()
            || self.quests.open_platform_menu.is_some();
        self.quests.open_platform_selector = None;
        self.quests.open_platform_menu = None;
        open
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
    fn quest_assets_live_under_the_quest_or_at_their_path() {
        assert_eq!(
            quest_asset("12", "reward.png"),
            "https://cdn.discordapp.com/quests/12/reward.png"
        );
        assert_eq!(
            quest_asset("12", "assets/x/y.png"),
            "https://cdn.discordapp.com/assets/x/y.png"
        );
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

    #[test]
    fn video_quest_footer_progress_and_reward_labels_match_module_491147() {
        assert_eq!(video_quest_progress_percent(0.0, 30.0, false), 0);
        assert_eq!(video_quest_progress_percent(15.0, 30.0, false), 50);
        assert_eq!(video_quest_progress_percent(29.95, 30.0, false), 99);
        assert_eq!(video_quest_progress_percent(30.0, 30.0, true), 100);
        assert_eq!(
            video_quest_reward_tooltip(false, false, "Orb Crown"),
            dc_intl::format("12IWP2", &[("rewardName", "Orb Crown".into())])
        );
        assert_eq!(
            video_quest_reward_tooltip(true, false, "Orb Crown"),
            dc_intl::format("NRp4K4", &[("rewardName", "Orb Crown".into())])
        );
        assert_eq!(
            video_quest_reward_tooltip(true, true, "Orb Crown"),
            dc_intl::format("12IWP2", &[("rewardName", "Orb Crown".into())])
        );
    }

    #[test]
    fn quest_advertiser_637691_and_reward_tile_639701_copy() {
        for k in ["EuHF34", "nLXlh+", "UMclVN", "DYAleT"] {
            assert!(dc_intl::has_key(k), "missing 637691/639701 key {k}");
        }
        let orbs = quest_reward_tile_label(true, true, 500, Some("Orbs"));
        assert!(orbs.contains("500"), "got {orbs}");
        let fallback = quest_reward_tile_label(false, false, 0, None);
        assert_eq!(fallback, dc_intl::t("UMclVN"));
        assert!(!quest_reward_learn_more_text().is_empty());
    }

    #[test]
    fn quest_tile_banner_918338_size_and_alt_copy() {
        assert!(dc_intl::has_key("d/+YKd"));
        assert!(dc_intl::has_key("P84bAD"));
        let alt = quest_banner_alt("Starfall");
        assert!(alt.contains("Starfall"), "got {alt}");
        assert_eq!(quest_banner_alt(""), dc_intl::t("P84bAD"));
        let (w1, h1) = quest_hero_image_size(1.0);
        assert!((w1 - 454.0).abs() < 0.01);
        assert!((h1 - (454.0 / (2640.0 / 740.0))).abs() < 0.05);
        let (w3, _) = quest_hero_image_size(3.0);
        assert!((w3 - 908.0).abs() < 0.01);
    }
}
