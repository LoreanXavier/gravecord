//! User settings. Shell from `modal__F9ieB`, sidebar `sidebar_cR4ic2`, nav items `item_xyJHva`, subnav `subnav_8qMUr2`,
//! content `content_3Rvb29`, panel `panel_nNq2Vb`, rows `baseControlItem_FiiQ6I`, buttons `button_F7qvhY`.
//! Sizes and colors from `design/dom/settings/account-darker-cozy.json`; strings from `STRINGS/en-US.json`.

mod account_dialogs;
mod account_nested;
mod account_panels;
mod mfa;
pub(in crate::ui) use account_dialogs::FORM_KEY as ACCOUNT_FORM_KEY;
pub(in crate::ui) use account_dialogs::USERNAME_KEY as ACCOUNT_USERNAME_KEY;
pub(in crate::ui) use account_panels::LOGOUT_KEY as SESSIONS_LOGOUT_KEY;
pub(in crate::ui) use mfa::{CREDENTIALS_KEY as MFA_CREDENTIALS_KEY, MFA_KEY};
mod actions;
mod appearance;
mod billing;
pub(in crate::ui) mod client_theme;
mod clips;
mod connection_card;
mod connection_dialogs;
mod connection_link;
mod connections;
mod content_filters;
mod controls;
mod custom_theme;
pub(in crate::ui) mod gift_inventory_page;
pub(in crate::ui) mod gift_redeem;
mod keybinds;
mod languages;
mod model;
mod mods;
pub(in crate::ui) mod nitro_mgmt;
pub(super) mod nitro_page;
mod notifications_page;
mod overlay_page;
mod pages;
pub(super) mod payment_sources;
mod phone_countries;
pub(super) mod prefs;
mod preview;
mod privacy;
mod render;
mod scroll_spring;
mod search;
mod search_terms;
mod server_boost_page;
mod soundboard;
mod streamer;
mod streamer_cover;
mod tester;
pub(crate) use render::radio_dot;
mod high_contrast_tokens;
mod swatches;
mod system;
pub(in crate::ui) mod theme_mix_tokens;
mod theme_state;
mod voice;
mod voice_camera;
pub(super) use actions::State as ActionsState;
pub(super) use clips::ClipsState;
pub(super) use voice::{Profile, VoiceState, set_soundboard_volume, soundboard_volume};

use super::motion::Transitions as _;
use super::{Root, SANS, avatar_source, color, kerning, motion};
use crate::net::Request;
use crate::parity::Parity;
use dc_proto::pin;
use gpui::{
    AnyElement, Bounds, BoxShadow, Context, ElementId, FontWeight, HighlightStyle, IntoElement,
    Pixels, ScrollHandle, SharedString, StyledText, Window, canvas, deferred, div, fill, hsla, img,
    point, prelude::*, px, size, svg,
};
pub(super) use model::Choice;
use model::{ButtonKind, Runs};
use scroll_spring::ScrollSpring;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Account,
    DataPrivacy,
    MessagingPermissions,
    Notifications,
    Nitro,
    ServerBoost,
    Subscriptions,
    GiftInventory,
    Billing,
    VoiceVideo,
    Appearance,
    Accessibility,
    System,
    LanguageTime,
    RegisteredGames,
    ActivityPrivacy,
    Clips,
    GameOverlay,
    ConnectedApps,
    Developer,
    LogOut,
    /// The Gravecord group (`mods.rs`): built-in plugins, not in the pinned client.
    Mods,
    Plugins,
    Backup,
    /// The Gravecord group (`tester.rs`): the updater and bug reports, not in the pinned client.
    Updates,
    ReportBug,
}

/// Every page that is drawn from the generated rows.
pub(super) const ALL_PAGES: &[Page] = &[
    Page::DataPrivacy,
    Page::MessagingPermissions,
    Page::Notifications,
    Page::Nitro,
    Page::ServerBoost,
    Page::Subscriptions,
    Page::GiftInventory,
    Page::Billing,
    Page::VoiceVideo,
    Page::Appearance,
    Page::Accessibility,
    Page::System,
    Page::LanguageTime,
    Page::RegisteredGames,
    Page::ActivityPrivacy,
    Page::GameOverlay,
    Page::ConnectedApps,
    Page::Developer,
];

struct Entry {
    page: Page,
    label: &'static str,
    icon: &'static str,
    /// Subnav entries: label and whether a NEW badge follows it.
    anchors: &'static [(&'static str, bool)],
}

const fn entry(page: Page, label: &'static str, icon: &'static str) -> Entry {
    Entry {
        page,
        label,
        icon,
        anchors: &[],
    }
}

/// Sidebar sections in the capture's order. `None` labels are visually hidden.
const SECTIONS: &[(Option<&str>, &[Entry])] = &[
    (
        None,
        &[
            Entry {
                page: Page::Account,
                label: "Account",
                icon: "account",
                anchors: &[
                    ("Account Info", false),
                    ("Password & Security", false),
                    ("Account Status", true),
                    ("Family Center", false),
                ],
            },
            entry(Page::DataPrivacy, "Data & Privacy", "data_privacy"),
            entry(
                Page::MessagingPermissions,
                "Messaging Permissions",
                "messaging_permissions",
            ),
            entry(Page::Notifications, "Notifications", "notifications"),
        ],
    ),
    (
        Some("Billing"),
        &[
            entry(Page::Nitro, "Nitro", "nitro"),
            entry(Page::ServerBoost, "Server Boost", "server_boost"),
            entry(Page::Subscriptions, "Subscriptions", "subscriptions"),
            entry(Page::GiftInventory, "Gift Inventory", "gift_inventory"),
            entry(Page::Billing, "Billing", "billing"),
        ],
    ),
    (
        Some("Experience"),
        &[
            entry(Page::VoiceVideo, "Voice & Video", "voice_video"),
            entry(Page::Appearance, "Appearance", "appearance"),
            entry(Page::Accessibility, "Accessibility", "accessibility"),
            entry(Page::System, "System", "system"),
            entry(Page::LanguageTime, "Language & Time", "language_time"),
        ],
    ),
    (
        Some("Games & Apps"),
        &[
            entry(
                Page::RegisteredGames,
                "Registered Games",
                "registered_games",
            ),
            entry(
                Page::ActivityPrivacy,
                "Activity Privacy",
                "activity_privacy",
            ),
            Entry {
                page: Page::Clips,
                label: "Clips",
                icon: "clips",
                anchors: clips::ANCHORS,
            },
            entry(Page::GameOverlay, "Game Overlay", "game_overlay"),
            entry(Page::ConnectedApps, "Connected Apps", "connected_apps"),
        ],
    ),
    (
        None,
        &[
            entry(Page::Developer, "Developer", "developer"),
            entry(Page::LogOut, "Log Out", "log_out"),
        ],
    ),
];

/// The Gravecord group: built-in plugins (`mods.rs`, `crate::plugins`), the updater and bug reports (`tester.rs`),
/// which the pinned client does not have. It goes after
/// the official groups and before Developer and Log Out, so the official entries keep their places; its labels are
/// not official messages, so it stays out of `SECTIONS`.
const GRAVECORD_SECTION: (Option<&str>, &[Entry]) = (
    Some("Gravecord"),
    &[
        entry(Page::Mods, "Gravecord", "mods"),
        entry(Page::Plugins, "Plugins", "plugins"),
        entry(Page::Backup, "Backup & Restore", "backup"),
        entry(Page::Updates, "Updates", "updates"),
        entry(Page::ReportBug, "Report a Bug", "report_bug"),
    ],
);

/// The sidebar's groups: the official ones with the Gravecord group before the last.
fn sections() -> impl Iterator<Item = &'static (Option<&'static str>, &'static [Entry])> {
    let (head, tail) = SECTIONS.split_at(SECTIONS.len() - 1);
    head.iter()
        .chain(std::iter::once(&GRAVECORD_SECTION))
        .chain(tail)
}

fn label_of(page: Page) -> &'static str {
    sections()
        .flat_map(|(_, entries)| entries.iter())
        .find(|entry| entry.page == page)
        .map_or("", |entry| entry.label)
}

pub(super) struct Open {
    page: Page,
    /// The subnav entry lit now: the content's scroll position moves it (`currentCategoryKey`).
    anchor: Rc<Cell<usize>>,
    content: ScrollHandle,
    /// The sidebar's scroller: one for as long as settings stays open, so a page change keeps its place.
    nav: ScrollHandle,
    /// The sidebar's animated scroll (module 354730), shared by every page of this opening.
    nav_spring: Rc<RefCell<ScrollSpring>>,
    /// The sidebar still has to bring the subnav into view; it does once, when the page opens.
    nav_pending: Rc<Cell<Reveal>>,
    /// The content's animated scroll: a subnav click on the open page glides there.
    content_spring: Rc<RefCell<ScrollSpring>>,
    /// The content's scroll offset the scroll-spy last saw, and whether the next change is a click's own scroll.
    spy: Rc<Cell<(f32, bool)>>,
    /// Opened by a link or by opening settings: the open page's subnav is drawn at its height at once (it mounts
    /// with `height: auto`, module 276530) and the sidebar is put at it without gliding.
    linked: bool,
}

/// How the sidebar brings the open page's subnav into view once it has its height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reveal {
    Done,
    /// The page was opened by a link: `navigate(..., {animateSidebarScroll: false})`.
    Instant,
    /// A sidebar click: the subnav's `onMount` waits for its height transition, then
    /// `scrollSidebarNodeIntoView(container, {animate: true, block: "nearest"})`.
    AfterExpand,
}

impl Open {
    pub(super) fn new(page: Page) -> Self {
        Self {
            page,
            anchor: Rc::default(),
            content: ScrollHandle::new(),
            nav: ScrollHandle::new(),
            nav_spring: Rc::default(),
            nav_pending: Rc::new(Cell::new(Reveal::Instant)),
            content_spring: Rc::default(),
            spy: Rc::default(),
            linked: true,
        }
    }

    /// The page a sidebar click opens: a new content pane at its top (`animatePanelScroll` is false for another
    /// panel), the sidebar where it was.
    fn switched(page: Page, from: &Open) -> Self {
        from.nav_spring.borrow_mut().abort();
        Self {
            page,
            anchor: Rc::default(),
            content: ScrollHandle::new(),
            nav: from.nav.clone(),
            nav_spring: from.nav_spring.clone(),
            nav_pending: Rc::new(Cell::new(Reveal::AfterExpand)),
            content_spring: Rc::default(),
            spy: Rc::default(),
            linked: false,
        }
    }
}

struct Anchor {
    label: &'static str,
    badge: bool,
    /// The panel child the subnav scrolls to.
    target: usize,
    /// The category's marker the content is scrolled to (`settings-marker:{key}`), when the page has one.
    key: Option<&'static str>,
}

/// The subnav thumb's spring (module 276530).
const SUBNAV_THUMB_SPRING: motion::SpringConfig = motion::SpringConfig::new(300., 20.).mass(0.1);

thread_local! {
    /// The category the content scrolls to once its marker has a place on screen (a click on the subnav, or a deep link).
    static PENDING_SCROLL: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// The pending scroll glides (a subnav click on the open page: `animatePanelScroll` is true for the same panel).
    static PENDING_GLIDE: Cell<bool> = const { Cell::new(false) };
}

/// The Account page's categories, in the subnav's order: the keys their markers carry.
const ACCOUNT_KEYS: [&str; 4] = [
    "account_info_category",
    "account_password_security_category",
    "account_status_category",
    "account_family_center_category",
];

/// Where the scroll-spy puts a clicked category's marker below the top of the view. The bundle scrolls the target
/// to `padding: 16 + r` below the top (module 397274 `scrollIntoView({..., padding: 16 + r})`, `r` the bottom of
/// the sticky decoration, 0 on this page); the reference frame (`account-status-darker-cozy`) has the heading
/// 24px under the top, so the category's box starts 8px above its heading. Our marker is the spacer under the
/// heading (32px line + 20px padding = 52px below the heading's top).
const ACCOUNT_SCROLL_MARGIN: f32 = 16. + 8. + 52.;

/// The NEW badges on Account Status (subnav entry and Age Group row) go once the category has been in view
/// (`account-status-darker-cozy` shows neither).
static ACCOUNT_STATUS_SEEN: AtomicBool = AtomicBool::new(false);

/// `TINY_BRONCO_NOTICE` of module 554146: the age group notice over Account Status, gone once dismissed
/// (`onDismiss` of `ACCOUNT_STANDING_CATEGORY`, module 158935).
const AGE_NOTICE_CONTENT: u16 = 821;

/// `M1` of module 961943: where "learn more on our blog" goes (`bD`, module 158935).
const AGE_GROUP_BLOG: &str = "https://discord.com/blog/safer-for-teens-same-discord-for-adults";

/// `_J` of module 158935: the help article "some content and settings" opens for an unconfirmed age group, the
/// country's own (`q5` of module 961943) or the general one (`GF`), through `getArticleURL` (module 975571).
fn age_gated_article(country: Option<&str>) -> String {
    let article = match country {
        Some("AU") => "38832004637207-Age-Assurance-for-Australian-Users",
        Some("BR") => "38860612202775-Age-Assurance-for-Brazilian-Users",
        Some("GB") => "33362401287959-Age-Assurance-for-UK-Users",
        _ => "42178056096663#h_01M0XEC24HG29RFJ4C04NM3SXM",
    };
    format!("https://support.discord.com/hc/en-us/articles/{article}")
}

impl Root {
    /// `_J`'s `LocationMetadataStore.getCountryCode()`: the article for the country READY reported.
    pub(in crate::ui) fn age_gated_article_url(&self) -> String {
        age_gated_article(self.stores.location_metadata.get_country_code())
    }
}

/// The NEW badge after the Accessibility sidebar entry (`settings-appearance-67-top-darker-cozy` shows it beside
/// the entry; `accessibility-darker-cozy`, with the page open, does not): it goes once the page has been opened.
static ACCESSIBILITY_SEEN: AtomicBool = AtomicBool::new(false);

/// The Account page lists its own anchors; every other page lists its titled sections.
fn anchors_of(entry: &Entry) -> Vec<Anchor> {
    if !entry.anchors.is_empty() {
        return entry
            .anchors
            .iter()
            .enumerate()
            .map(|(index, (label, badge))| Anchor {
                label,
                badge: *badge,
                // Categories sit between dividers: every other child of the panel.
                target: index * 2,
                // The marker each category draws: the click glides to it once it is laid out. Without one the click
                // could only ask the scroller for a child it does not track, and Clips' entries never moved.
                key: match entry.page {
                    Page::Account => Some(ACCOUNT_KEYS[index]),
                    Page::Clips => clips::CATEGORIES.get(index).copied(),
                    _ => None,
                },
            })
            .collect();
    }
    pages::nodes(entry.page)
        .iter()
        .enumerate()
        .filter_map(|(target, node)| match node {
            model::Node::Category {
                key,
                title: Some(title),
                ..
            } => Some(Anchor {
                label: title,
                badge: false,
                target,
                key: Some(*key),
            }),
            _ => None,
        })
        .collect()
}

/// Lines a subnav label takes in its 184px column, wrapping between words.
fn wrapped_lines(window: &mut Window, label: &str, badge: bool) -> usize {
    let column = if badge { 184. - 8. - 41.6 } else { 184. };
    let space = f32::from(render::measure_text(window, " ", 16., 400.));
    let mut lines = 1;
    let mut used = 0.;
    for word in label.split_whitespace() {
        let width = f32::from(render::measure_text(window, word, 16., 400.));
        if used > 0. && used + space + width > column {
            lines += 1;
            used = width;
        } else {
            used += if used > 0. { space + width } else { width };
        }
    }
    lines
}

/// "Voice & Video" becomes "voice-video".
fn page_slug(label: &str) -> String {
    let mut slug = String::new();
    for word in label
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word.to_ascii_lowercase());
    }
    slug
}

/// `NI` in the settings bundle: every character before the `@` becomes a star.
pub(super) fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) => format!("{}@{domain}", "*".repeat(local.chars().count())),
        None => email.to_owned(),
    }
}

/// `G7zwOk`: "{count} device" or "{count} devices".
fn devices(count: usize) -> String {
    format!("{count} {}", if count == 1 { "device" } else { "devices" })
}

/// The add-game popout's buttons (module 158935 `fv`): the settings button, secondary or primary.
pub(in crate::ui) fn popout_button(
    id: impl Into<ElementId>,
    label: &'static str,
    primary: bool,
) -> impl IntoElement {
    button(
        id,
        label,
        if primary {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        },
    )
}

/// `md` text button: 40 tall, at least 100 wide, 15px inside a 1px border.
fn button(id: impl Into<ElementId>, label: &'static str, kind: ButtonKind) -> impl IntoElement {
    let (background, border, text, hover_background, hover_border) = match kind {
        ButtonKind::Primary => (
            "--control-primary-background-default",
            "--control-primary-border-default",
            "--control-primary-text-default",
            "--control-primary-background-hover",
            "--control-primary-border-hover",
        ),
        ButtonKind::Secondary => (
            "--control-secondary-background-default",
            "--control-secondary-border-default",
            "--control-secondary-text-default",
            "--control-secondary-background-hover",
            "--control-secondary-border-hover",
        ),
        ButtonKind::CriticalSecondary => (
            "--control-critical-secondary-background-default",
            "--control-critical-secondary-border-default",
            "--control-critical-secondary-text-default",
            "--control-critical-secondary-background-hover",
            "--control-critical-secondary-border-hover",
        ),
        ButtonKind::CriticalPrimary => (
            "--control-critical-primary-background-default",
            "--control-critical-primary-border-default",
            "--control-critical-primary-text-default",
            "--control-critical-primary-background-hover",
            "--control-critical-primary-border-hover",
        ),
    };
    div()
        .id(id)
        .flex_none()
        .h(px(40.))
        .min_w(px(100.))
        .px(px(15.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .border_1()
        .border_color(color(border))
        .bg(color(background))
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(color(hover_background))
                .border_color(color(hover_border))
        })
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(color(text))
        .child(label)
}

pub(super) fn mask_icon(path: &str, size: f32, tint: &str) -> gpui::Svg {
    svg()
        .path(format!("icons/{path}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color(tint))
}

fn value(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-default"))
        .child(text.into())
}

/// Plain text with link runs in `--text-link`, as `body-sm/normal` descriptions write them.
fn described(parts: &[(&str, bool)], base: &str) -> impl IntoElement {
    styled_runs(
        parts,
        base,
        HighlightStyle {
            color: Some(color("--text-link").into()),
            ..HighlightStyle::default()
        },
    )
}

/// Inline notices keep links in the text color and underline them.
fn noticed(parts: &[(&str, bool)], base: &str) -> impl IntoElement {
    styled_runs(
        parts,
        base,
        HighlightStyle {
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.),
                color: Some(color(base).into()),
                wavy: false,
            }),
            ..HighlightStyle::default()
        },
    )
}

/// The text of the runs and the byte ranges of its link runs.
fn run_ranges(parts: &[(&str, bool)]) -> (String, Vec<std::ops::Range<usize>>) {
    let mut text = String::new();
    let mut links = Vec::new();
    for (part, link) in parts {
        let start = text.len();
        text.push_str(part);
        if *link {
            links.push(start..text.len());
        }
    }
    (text, links)
}

fn styled_runs(parts: &[(&str, bool)], base: &str, link_style: HighlightStyle) -> impl IntoElement {
    let (text, links) = run_ranges(parts);
    div()
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(color(base))
        .child(
            StyledText::new(text)
                .with_highlights(links.into_iter().map(|range| (range, link_style))),
        )
}

/// `noticed` (or, for a description, `described_linked`) whose link runs take presses: `press` gets the index of
/// the pressed link among the runs' links.
fn linked_runs(
    id: SharedString,
    parts: &[(&str, bool)],
    base: &str,
    press: LinkPress,
) -> impl IntoElement + use<> {
    linked_runs_styled(
        id,
        parts,
        base,
        HighlightStyle {
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.),
                color: Some(color(base).into()),
                wavy: false,
            }),
            ..HighlightStyle::default()
        },
        press,
    )
}

/// An account standing's text (`x` and `I` of module 881979): All good's links open the terms and the guidelines
/// (`TOS_LINK`, `COMMUNITY_GUIDELINES` of module 239093); the other standings have none.
fn standing_text(
    id: &'static str,
    runs: Runs,
    base: &str,
    root: gpui::WeakEntity<Root>,
) -> impl IntoElement + use<> {
    described_linked(
        id.into(),
        runs,
        base,
        Box::new(move |index, cx| {
            let url = if index == 0 {
                "https://discord.com/terms"
            } else {
                "https://discord.com/guidelines"
            };
            let _ = root.update(cx, |this, cx| {
                this.settings_open_url(url, cx);
            });
        }),
    )
}

/// `described` whose link runs take presses.
fn described_linked(
    id: SharedString,
    parts: &[(&str, bool)],
    base: &str,
    press: LinkPress,
) -> impl IntoElement + use<> {
    linked_runs_styled(
        id,
        parts,
        base,
        HighlightStyle {
            color: Some(color("--text-link").into()),
            ..HighlightStyle::default()
        },
        press,
    )
}

fn linked_runs_styled(
    id: SharedString,
    parts: &[(&str, bool)],
    base: &str,
    link_style: HighlightStyle,
    press: LinkPress,
) -> impl IntoElement + use<> {
    let (text, links) = run_ranges(parts);
    let styled = StyledText::new(text)
        .with_highlights(links.iter().map(|range| (range.clone(), link_style)));
    div()
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(color(base))
        .child(
            gpui::InteractiveText::new(id, styled)
                // `e.stopPropagation()`: a link in a pressable row does not also press the row.
                .on_click(links, move |index, _, cx| {
                    cx.stop_propagation();
                    press(index, cx);
                }),
        )
}

struct Row {
    title: &'static str,
    badge: bool,
    description: Option<AnyElement>,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    clickable: bool,
    /// What a press on the row does (a nested panel opens).
    press: Option<(gpui::ElementId, RowPress)>,
}

/// What a row's press runs.
type RowPress = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App)>;

impl Row {
    fn new(title: &'static str) -> Self {
        Self {
            title,
            badge: false,
            description: None,
            leading: None,
            trailing: None,
            clickable: false,
            press: None,
        }
    }

    fn press(
        mut self,
        id: impl Into<gpui::ElementId>,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.press = Some((id.into(), Box::new(handler)));
        self
    }

    fn description(mut self, element: impl IntoElement) -> Self {
        self.description = Some(element.into_any_element());
        self
    }

    fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing = Some(element.into_any_element());
        self
    }

    /// A clickable row ends in its value and a chevron, 12px apart.
    fn link(mut self, value_text: Option<String>) -> Self {
        self.clickable = true;
        self.trailing = Some(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .children(value_text.map(value))
                .child(mask_icon(
                    "settings_chevron",
                    24.,
                    "--interactive-text-default",
                ))
                .into_any_element(),
        );
        self
    }

    fn badge(self) -> Self {
        self.badge_if(true)
    }

    fn badge_if(mut self, shown: bool) -> Self {
        self.badge = shown;
        self
    }

    fn render(mut self) -> AnyElement {
        let press = self.press.take();
        let title = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr(self.title)),
            )
            .when(self.badge, |this| this.child(new_badge()));
        let row = div()
            .flex()
            .gap(px(12.))
            .min_h(px(48.))
            .mx(px(-12.))
            .p(px(12.))
            .rounded(px(16.))
            .when(self.clickable, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
            })
            .children(
                self.leading
                    .map(|leading| div().flex().items_center().child(leading)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(4.))
                    .child(title)
                    .children(self.description),
            )
            .children(
                self.trailing
                    .map(|trailing| div().flex().items_center().child(trailing)),
            );
        match press {
            Some((id, handler)) => row.id(id).on_click(handler).into_any_element(),
            None => row.into_any_element(),
        }
    }
}

pub(super) fn new_badge() -> impl IntoElement {
    badge(dc_intl::tr("NEW"))
}

/// `badge_...`: a pill beside a title, NEW or BETA.
fn badge(text: &'static str) -> impl IntoElement {
    div()
        .flex_none()
        .h(px(16.))
        .px(px(8.))
        .rounded_full()
        .bg(color("--badge-primary-background"))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(600.))
        .text_color(gpui::white())
        .child(text)
}

/// 48px square behind a row's leading icon.
fn tile(icon_name: &str, background: &str, tint: &str) -> impl IntoElement {
    div()
        .size(px(48.))
        .rounded(px(8.))
        .bg(color(background))
        .flex()
        .items_center()
        .justify_center()
        .child(mask_icon(icon_name, 24., tint))
}

/// `heading-xl/medium` with its note 4px under it, 20px of space, then a 1px spacer before the rows.
fn category_header(
    title: &'static str,
    description: Runs,
    marker: Option<AnyElement>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .pb(px(20.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(24.))
                        .line_height(px(32.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::tr(title)),
                )
                .when(!description.is_empty(), |this| {
                    // `runs_text` takes the link presses for the category `link_owner` named (Profile Privacy's
                    // Learn more).
                    this.child(render::runs_text(description, "--text-subtle"))
                }),
        )
        // The spacer carries `data-settings-category-key`: the scroll-spy watches it.
        .child(div().relative().h(px(1.)).children(marker))
}

/// Heading `heading-xl/medium` with 20px under it, then the rows 8px apart.
fn category(title: Option<&'static str>, rows: Vec<Row>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .children(title.map(|title| category_header(title, &[], None)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(rows.into_iter().map(Row::render)),
        )
        .into_any_element()
}

/// `category` whose header carries the marker the subnav scrolls to and the scroll-spy watches.
fn category_marked(title: &'static str, marker: AnyElement, rows: Vec<Row>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .child(category_header(title, &[], Some(marker)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(rows.into_iter().map(Row::render)),
        )
        .into_any_element()
}

/// `divider_nNq2Vb`: 1px rule with 40px above and below.
fn divider() -> AnyElement {
    div()
        .my(px(40.))
        .h(px(1.))
        .bg(color("--border-subtle"))
        .into_any_element()
}

/// What a press on the `index`th link run of a text does.
type LinkPress = Box<dyn Fn(usize, &mut gpui::App)>;

/// `container_0v8Ae1 info_0v8Ae1`. With `dismiss`, the Dismiss button (`onDismiss` of the notice) runs it; with
/// `links`, a press on a link run runs it with the link's index.
fn info_notice(
    id: &'static str,
    parts: Runs,
    links: Option<LinkPress>,
    dismiss: Option<RowPress>,
) -> impl IntoElement {
    let close = div()
        .id(SharedString::from(format!("{id}-dismiss")))
        .flex_none()
        .size(px(32.))
        .mt(px(-7.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .child(mask_icon(
            "settings_notice_close",
            16.,
            "--control-icon-only-icon-default",
        ));
    let close = match dismiss {
        Some(press) => close
            .cursor_pointer()
            .hover(|style| style.bg(color("--control-icon-only-background-hover")))
            .on_click(press),
        None => close,
    };
    let text: AnyElement = match links {
        Some(press) => {
            linked_runs(SharedString::from(id), parts, "--text-strong", press).into_any_element()
        }
        None => noticed(parts, "--text-strong").into_any_element(),
    };
    div()
        .mb(px(8.))
        .flex()
        .gap(px(16.))
        .p(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--inlinenotice-border-info"))
        .bg(color("--background-feedback-mod-info"))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .gap(px(8.))
                .child(div().h(px(18.)).flex().items_center().child(mask_icon(
                    "settings_notice_info",
                    16.,
                    "--icon-feedback-info",
                )))
                .child(div().flex_1().min_w(px(0.)).child(text)),
        )
        .child(close)
}

impl Root {
    /// Dismiss on the age group notice: `markAsDismissed(USER_DISMISS)` sets the content's bit in the account's
    /// `dismissed_contents` (a settings-proto write, held back in a harness run).
    fn dismiss_age_notice(&mut self) {
        if self.stores.content_dismissed(AGE_NOTICE_CONTENT) {
            return;
        }
        let patch = self.stores.dismiss_content(AGE_NOTICE_CONTENT);
        if patch.is_empty() || crate::storage::settings_writes_blocked() {
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me/settings-proto/1".into(),
            body: Some(serde_json::json!({"settings": patch})),
        });
    }

    /// The pictures the settings pages draw at once: the custom theme banner, the overlay and preview art, the
    /// Language select's flags, two emoji and the app icons. Asked for when settings open, and ahead of time after
    /// startup (`READY_SUPPLEMENTAL`) so the first open draws them at once.
    pub(super) fn request_settings_art(&mut self) {
        // The custom theme card's banner (`bannerImage_yH_g_W`).
        self.request_image(
            "theme_upsell_banner".to_owned(),
            "https://cdn.discordapp.com/assets/content/568728735ebc2ee3eb3a7ab29d37c1f698f413d29cee50f9a19ed2d2eefea27f.svg"
                .to_owned(),
        );
        for (key, url) in overlay_page::requests() {
            self.request_image(key.to_owned(), url.to_owned());
        }
        for (key, url) in preview::requests() {
            self.request_image(key, url);
        }
        // The Language select's flags (module 172471).
        for (key, url) in languages::flag_requests() {
            self.request_image(key, url);
        }
        for glyph in ["\u{1f940}", "\u{1faa4}"] {
            self.request_emoji_image(glyph);
        }
        for index in 0..appearance::APP_ICONS.len() {
            self.request_image(
                appearance::app_icon_key(index),
                appearance::app_icon_url(index),
            );
        }
    }

    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = Some(Open::new(Page::Account));
        self.guild_search_ready(cx);
        self.settings_search_ready(cx);
        self.plugin_search_ready(cx);
        self.gift_field_ready(cx);
        // The settings pages' own pictures go first: the Nitro reads below queue dozens of pictures for the Nitro
        // pages, and the theme card's banner waited behind them for one to three seconds.
        self.request_settings_art();
        self.request_nitro();
        let _ = self.requests.send(Request::Fetch {
            key: "sessions".to_owned(),
            route: "/auth/sessions".to_owned(),
        });
        self.settings_pages_opened();
        cx.notify();
    }

    /// Opens settings on the page with this sidebar label ("Data & Privacy", "Server Boost").
    pub(super) fn open_settings_section(&mut self, label: &str, cx: &mut Context<Self>) {
        self.open_settings(cx);
        let page = sections()
            .flat_map(|(_, entries)| entries.iter())
            .filter(|entry| entry.page != Page::Clips || self.clips_listed())
            .find(|entry| entry.label == label)
            .map(|entry| entry.page);
        if let (Some(open), Some(page)) = (self.settings.as_mut(), page) {
            *open = Open::new(page);
        }
        cx.notify();
    }

    /// `openUserSettings(category)`: the page that holds the category, scrolled to it.
    pub(super) fn open_settings_category(
        &mut self,
        page_label: &str,
        category: &str,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(cx);
        let entry = sections()
            .flat_map(|(_, entries)| entries.iter())
            .filter(|entry| entry.page != Page::Clips || self.clips_listed())
            .find(|entry| entry.label == page_label);
        if let (Some(open), Some(entry)) = (self.settings.as_mut(), entry) {
            *open = Open::new(entry.page);
            if let Some((index, anchor)) = anchors_of(entry)
                .iter()
                .enumerate()
                .find(|(_, anchor)| anchor.label == category)
            {
                open.anchor.set(index);
                open.spy.set((0., true));
                if anchor.key.is_none() {
                    open.content.scroll_to_top_of_item(anchor.target);
                }
                PENDING_SCROLL.set(anchor.key);
            }
        }
        cx.notify();
    }

    /// Opens settings on the page whose label, lower case with dashes, is `slug`: "data-privacy", "voice-video".
    pub(crate) fn open_settings_page(&mut self, slug: &str, cx: &mut Context<Self>) {
        self.open_settings(cx);
        let page = sections()
            .flat_map(|(_, entries)| entries.iter())
            .filter(|entry| entry.page != Page::Clips || self.clips_listed())
            .find(|entry| page_slug(entry.label) == slug)
            .map(|entry| entry.page);
        if let (Some(open), Some(page)) = (self.settings.as_mut(), page) {
            *open = Open::new(page);
        }
        cx.notify();
    }

    /// Closes settings. The layer's exit (opacity to 0, scale to 1.1) is drawn from a copy of its last frame by
    /// `motion::driver`.
    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = None;
        crate::sounds::stop_preview();
        cx.notify();
    }

    /// A sidebar item clicked (`navigate(panel, {animateSidebarScroll: true})`, modules 276530 and 272053): another
    /// page opens at its top while the sidebar keeps its place and glides the item into view (`block: "nearest"`,
    /// 12px of padding; its subnav has no height yet). The open page's item goes to its first category, gliding.
    fn settings_nav_clicked(&mut self, page: Page, icon: &'static str) {
        self.settings_extra.panel = None;
        let Some(open) = self.settings.as_mut() else {
            return;
        };
        if open.page == page {
            let entry = sections()
                .flat_map(|(_, entries)| entries.iter())
                .find(|entry| entry.page == page);
            let first = entry.and_then(|entry| anchors_of(entry).into_iter().next());
            match first {
                Some(anchor) => self.settings_category_clicked(0, anchor.target, anchor.key),
                None => {
                    let scroll = -f64::from(f32::from(open.content.offset().y));
                    open.content_spring.borrow_mut().start(scroll, 0.);
                }
            }
            return;
        }
        *open = Open::switched(page, open);
        let item = self
            .tip_anchors
            .borrow()
            .get(&SharedString::from(format!("nav-item-{icon}")))
            .copied();
        let view = open.nav.bounds();
        if let Some(item) = item
            && view.size.height > px(0.)
        {
            let scroll = -f32::from(open.nav.offset().y);
            let target = nearest_scroll(
                scroll,
                (f32::from(view.top()), f32::from(view.bottom())),
                (f32::from(item.top()), f32::from(item.bottom())),
                12.,
                f32::from(open.nav.max_offset().y),
            );
            if (target - scroll).abs() >= 0.5 {
                if motion::reduced() {
                    open.nav.set_offset(point(open.nav.offset().x, px(-target)));
                } else {
                    open.nav_spring
                        .borrow_mut()
                        .start(f64::from(scroll), f64::from(target));
                }
            }
        }
    }

    /// A subnav entry clicked on the open page: the entry lights at once and the content glides to the category
    /// (`animatePanelScroll` is true on the same panel; module 397274 scrolls with `padding: 16 + sticky`).
    fn settings_category_clicked(
        &mut self,
        index: usize,
        target: usize,
        key: Option<&'static str>,
    ) {
        let exited_panel = self.settings_extra.panel.take().is_some();
        let Some(open) = self.settings.as_mut() else {
            return;
        };
        open.anchor.set(index);
        // The click's own scroll does not move the lit entry (`isSidebarCategoryAutoSelectEnabled`).
        open.spy.set((open.spy.get().0, true));
        let placed = !exited_panel
            && key.is_some_and(|key| {
                self.tip_anchors
                    .borrow()
                    .contains_key(&SharedString::from(format!("settings-marker:{key}")))
            });
        PENDING_SCROLL.set(key);
        PENDING_GLIDE.set(placed);
        if !placed {
            // A marker not drawn yet: the scroll-spy places it once it is (the scroller tracks only its direct
            // children, so asking it for a section by index never moved anything).
            open.content_spring.borrow_mut().abort();
            if key.is_none() {
                open.content.scroll_to_top_of_item(target);
            }
        }
    }

    fn settings_nav_item(
        &self,
        entry: &'static Entry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.settings.as_ref();
        let active = open.is_some_and(|open| open.page == entry.page);
        let destructive = entry.page == Page::LogOut;
        if active && entry.page == Page::Accessibility {
            ACCESSIBILITY_SEEN.store(true, Ordering::Relaxed);
        }
        let new_badge_after = entry.page == Page::Accessibility
            && !active
            && !ACCESSIBILITY_SEEN.load(Ordering::Relaxed);
        // Every page's categories: the open page's list is drawn, and the one of the page just left while it
        // closes.
        let anchors = anchors_of(entry);
        let page = entry.page;
        let icon = entry.icon;
        // The sidebar is a list: one tab stop on the open page, the arrows, Home and End move between items.
        let handle = self.stop(&format!("settings-nav-{}", entry.icon), cx);
        self.join("settings-nav", super::keyboard::Nav::List, &handle);
        // Only the page's own layer is open: a dialog above it takes the tab order.
        let item = super::keyboard::focusable(
            div().id(SharedString::from(format!("settings-nav-{}", entry.icon))),
            &handle,
            active && self.modal_open() <= 1,
            false,
        )
        .relative()
        .h(px(36.))
        .p(px(8.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        // The Log Out entry (`item_xyJHva destructive_xyJHva`) carries an inline `color: var(--text-feedback-critical)`
        // and `var(--icon-feedback-critical)` on its icon, so hover leaves its colours alone.
        .text_color(color(if destructive {
            "--text-feedback-critical"
        } else if active {
            "--text-strong"
        } else {
            "--text-muted"
        }))
        // `.item_xyJHva.active_xyJHva`: `transition: background-color .3s ease`. A row that is not the open one has
        // no transition (`:hover:not(.active) { transition: none }`), so the fill only fades in.
        .map(|this| {
            let key = motion::key(&("settings-nav-fill", entry.icon));
            let fill = color("--background-mod-normal");
            if active {
                this.bg(motion::color(
                    key,
                    fill,
                    motion::css(300., motion::Ease::Ease),
                ))
            } else {
                // Kept clear at once, so the next time the row opens its fill starts from nothing.
                let clear = gpui::Rgba { a: 0., ..fill };
                motion::color(key, clear, motion::css(0., motion::Ease::Linear));
                this
            }
        })
        .when(!active, |this| {
            this.hover(move |style| {
                let style = style.bg(color(if destructive {
                    "--background-feedback-mod-critical"
                } else {
                    "--background-mod-muted"
                }));
                if destructive {
                    style
                } else {
                    style.text_color(color("--text-strong"))
                }
            })
        })
        // `.item_xyJHva.active`: `.full-motion` `background-color .3s ease`; the `:hover:not(.active)` rule is
        // `transition: none`, so only an item that becomes active fades its fill in.
        .transition(
            motion::Props::BACKGROUND,
            if active {
                motion::css(300., motion::Ease::Ease).full_motion()
            } else {
                motion::css(0., motion::Ease::Linear)
            },
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if page == Page::LogOut {
                this.logout_confirm = true;
            } else {
                this.settings_nav_clicked(page, icon);
            }
            cx.notify();
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    svg()
                        .path(format!("icons/settings_{}.svg", entry.icon))
                        .size(px(20.))
                        .flex_none()
                        .text_color(color(if destructive {
                            "--icon-feedback-critical"
                        } else if active {
                            "--text-strong"
                        } else {
                            "--text-muted"
                        })),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .child(dc_intl::tr(entry.label)),
                ),
        )
        .when(new_badge_after, |this| this.child(new_badge()))
        .when(
            self.keyboard_mode() && self.keys.stops.has_focus(&handle),
            |this| this.child(super::keyboard::hollow_ring(8.)),
        );
        let anchors_map = self.tip_anchors.clone();
        let probe_key = SharedString::from(format!("nav-item-{}", entry.icon));
        let item = item.child(
            gpui::canvas(
                move |bounds, _, _| {
                    anchors_map.borrow_mut().insert(probe_key.clone(), bounds);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        // `.subnavContainer_8qMUr2`: `overflow-y: clip; transition: height .2s ease-out, opacity .15s ease-out 80ms`.
        // The list of the page that opens grows to its height while the one of the page left shrinks to nothing.
        // A page opened by a link mounts its list at full height (`useState(active ? "auto" : "0")`); so does every
        // list when motion is reduced (`.full-motion` carries the transition).
        let instant = open.is_some_and(|open| open.linked) || motion::reduced();
        let subnav = (!anchors.is_empty()).then(|| {
            let full = self.settings_subnav_height(&anchors, window);
            let height = motion::value(
                motion::key(&("settings-subnav-height", entry.icon)),
                if active { full } else { 0. },
                if instant {
                    motion::css(0., motion::Ease::Linear)
                } else {
                    motion::css(200., motion::Ease::EaseOut)
                },
            );
            let opacity = motion::value(
                motion::key(&("settings-subnav-opacity", entry.icon)),
                if active { 1. } else { 0. },
                if instant {
                    motion::css(0., motion::Ease::Linear)
                } else {
                    motion::css(150., motion::Ease::EaseOut).delay(80.)
                },
            );
            (height, opacity, (height - full).abs() < 0.01)
        });
        div()
            .flex()
            .flex_col()
            .child(item)
            .when_some(
                subnav.filter(|(height, _, _)| active || *height > 0.5),
                |this, (height, opacity, expanded)| {
                    this.child(
                        div()
                            .h(px(height))
                            .overflow_hidden()
                            .opacity(opacity)
                            .child(self.settings_subnav(
                                &anchors, entry.icon, active, expanded, window, cx,
                            )),
                    )
                },
            )
            .into_any_element()
    }

    /// The height of a page's category list: its rows, the 16px between them and 8px above and under.
    fn settings_subnav_height(&self, anchors: &[Anchor], window: &mut Window) -> f32 {
        let seen = ACCOUNT_STATUS_SEEN.load(Ordering::Relaxed);
        let rows: f32 = anchors
            .iter()
            .map(|anchor| {
                20. * wrapped_lines(window, dc_intl::tr(anchor.label), anchor.badge && !seen) as f32
            })
            .sum();
        rows + 16. * anchors.len().saturating_sub(1) as f32 + 16.
    }

    /// The grid of `subnav_8qMUr2`: a 2px track 17px in, labels 17px past it and 16px apart.
    /// Labels wrap in the 184px column, so the thumb is placed by the measured heights above it.
    fn settings_subnav(
        &self,
        anchors: &[Anchor],
        icon: &'static str,
        open_page: bool,
        expanded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The list of a page that is closing keeps no lit entry of the page that opened.
        let current = if open_page {
            self.settings.as_ref().map_or(0, |open| open.anchor.get())
        } else {
            usize::MAX
        };
        if anchors.get(current).and_then(|anchor| anchor.key) == Some(ACCOUNT_KEYS[2]) {
            ACCOUNT_STATUS_SEEN.store(true, Ordering::Relaxed);
        }
        let seen = ACCOUNT_STATUS_SEEN.load(Ordering::Relaxed);
        let heights: Vec<f32> = anchors
            .iter()
            .map(|anchor| {
                20. * wrapped_lines(window, dc_intl::tr(anchor.label), anchor.badge && !seen) as f32
            })
            .collect();
        let total = heights.iter().sum::<f32>() + 16. * heights.len().saturating_sub(1) as f32;
        let thumb_top: f32 = heights
            .iter()
            .take(current)
            .map(|height| height + 16.)
            .sum();
        let thumb_height = heights.get(current).copied().unwrap_or(20.);
        // Module 276530: the thumb's `y` and `height` are react-spring values (`{mass: .1, friction: 20, tension:
        // 300}`) that move to the lit row's anchor; the first layout of a list and reduced motion set them at once.
        // Each page that opens gets a new `Open`, so its `anchor` cell names this mount of the list.
        let (thumb_top, thumb_height) = match self.settings.as_ref().filter(|_| open_page) {
            Some(open) if !motion::reduced() => {
                let mount = Rc::as_ptr(&open.anchor) as usize;
                (
                    motion::spring_value(
                        motion::key(&("settings-subnav-thumb-y", mount)),
                        thumb_top,
                        SUBNAV_THUMB_SPRING,
                    ),
                    motion::spring_value(
                        motion::key(&("settings-subnav-thumb-height", mount)),
                        thumb_height,
                        SUBNAV_THUMB_SPRING,
                    ),
                )
            }
            _ => (thumb_top, thumb_height),
        };
        let track = div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(2.))
            .h(px(total))
            .rounded(px(1.))
            .bg(color("--background-mod-subtle"))
            .when(open_page, |track| {
                track.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(thumb_top))
                        .w(px(2.))
                        .h(px(thumb_height))
                        .rounded_full()
                        .bg(color("--interactive-text-active")),
                )
            });
        let rows: Vec<AnyElement> = anchors
            .iter()
            .enumerate()
            .map(|(index, anchor)| {
                let active = index == current;
                let (target, key) = (anchor.target, anchor.key);
                div()
                    .id(SharedString::from(format!("settings-anchor-{index}")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .cursor_pointer()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    // `.subnav .item:hover` and `.subnav .item.active` weigh the same and the active rule comes
                    // later: the lit row keeps its colour under the pointer.
                    .text_color(color(if active {
                        "--interactive-text-active"
                    } else {
                        "--text-muted"
                    }))
                    .when(!active, |this| {
                        this.hover(|style| style.text_color(color("--interactive-text-hover")))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_category_clicked(index, target, key);
                        cx.notify();
                    }))
                    .child(div().min_w(px(0.)).child(dc_intl::tr(anchor.label)))
                    .when(anchor.badge && !seen, |this| this.child(new_badge()))
                    .into_any_element()
            })
            .collect();
        let anchors_map = self.tip_anchors.clone();
        let reveal = self.settings.as_ref().filter(|_| open_page).map(|open| {
            (
                open.nav.clone(),
                open.nav_spring.clone(),
                open.nav_pending.clone(),
            )
        });
        div()
            .relative()
            .ml(px(17.))
            .py(px(8.))
            .children(reveal.map(|(nav, spring, pending)| {
                nav_into_view(nav, spring, pending, expanded, anchors_map, icon)
            }))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .pl(px(19.))
                    .child(track)
                    .children(rows),
            )
            .into_any_element()
    }

    fn settings_profile(&self, cx: &mut Context<Self>) -> AnyElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        // The whole card is the sidebar item (`item_xyJHva`, `role="link"`, 220x64 in account-darker-cozy) that holds
        // `profileCustomizationTab_iVwosX` (module 158935, `_k`): a press anywhere on it, the avatar included, opens
        // the profile editor. It is hoisted above the list, so it is a tab stop of its own, not one of the list's.
        // Hover: `--background-mod-muted` and `--text-strong`, which the "Edit Profiles" line and its pencil take
        // (`currentColor`).
        let handle = self.stop("settings-nav-profile", cx);
        let focused = self.keyboard_mode() && self.keys.stops.has_focus(&handle);
        super::keyboard::focusable(
            div().id("settings-edit-profiles"),
            &handle,
            self.modal_open() <= 1,
            false,
        )
        .group("settings-edit-profiles")
        .relative()
        .h(px(64.))
        .p(px(8.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(12.))
        .cursor_pointer()
        .text_color(color("--text-muted"))
        .hover(|style| {
            style
                .bg(color("--background-mod-muted"))
                .text_color(color("--text-strong"))
        })
        .on_click(cx.listener(|this, _, _, cx| {
            this.open_profile_editor(cx);
        }))
        .when(focused, |this| this.child(super::keyboard::hollow_ring(8.)))
        .child(
            div()
                .size(px(48.))
                .rounded_full()
                .overflow_hidden()
                .children(avatar.map(|image| img(image).size(px(48.)).rounded_full())),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(user.display_name().to_owned()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .child(dc_intl::tr("Edit Profiles"))
                        .child(
                            mask_icon("settings_edit_pencil", 12., "--text-muted")
                                .group_hover("settings-edit-profiles", |style| {
                                    style.text_color(color("--text-strong"))
                                }),
                        ),
                ),
        )
        .into_any_element()
    }

    fn settings_search(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.settings_extra.search.clone() else {
            return div().h(px(40.)).into_any_element();
        };
        let focused = gpui::Focusable::focus_handle(editor.read(cx), cx).is_focused(window);
        let typed = !editor.read(cx).text().is_empty();
        let clear = editor.clone();
        div()
            .h(px(40.))
            .pl(px(4.))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(px(8.))
            .border_1()
            // `wrapper_UcGqzv` focused: `--input-border-active` (search-voice-darker-cozy dump).
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .bg(color("--input-background-default"))
            // Focused: `outline: 2px solid var(--custom-input-focus-border-color); outline-offset: -2px` (448400 css,
            // `.wrapper_UcGqzv:has(input:focus)`), a 2px ring inside the border box.
            .relative()
            .children(focused.then(|| {
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(-1.))
                    .right(px(-1.))
                    .bottom(px(-1.))
                    .rounded(px(8.))
                    .border_2()
                    .border_color(color("--input-border-active"))
            }))
            .child(
                div()
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(mask_icon("settings_search", 16., "--icon-default")),
            )
            // `input_0PLWhS` is 38 tall with 1px of top padding: the text centres on a half pixel, which Chromium rounds down to the lower line.
            .child(
                div()
                    .h(px(38.))
                    .pt(px(2.))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .child(div().w_full().child(editor)),
            )
            // The clear control: a 30px box with the 16px round X, centred 20px in from the field's edge.
            .children(typed.then(|| {
                div()
                    .id("settings-search-clear")
                    .size(px(30.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(mask_icon("discover_clear", 16., "--icon-default"))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        clear.update(cx, |editor, cx| {
                            editor.set_text("");
                            window.focus(&gpui::Focusable::focus_handle(editor, cx), cx);
                        });
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }

    fn settings_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        const VERSION_KEY: &str = "copy-combo-version";
        let footnote = |text: String, tint: &str| {
            div()
                .text_size(px(10.))
                .line_height(px(12.))
                .text_color(color(tint))
                .child(text)
        };
        let copied = self.copy_combo.is_copied_for(VERSION_KEY);
        let tip = if copied {
            let combo = self.copy_combo.combo;
            let mut t = super::tooltip::Tip::body(
                super::tooltip::Body::Green(SharedString::from(super::copy_combo::combo_label(
                    combo,
                ))),
                super::tooltip::Side::Top,
            );
            if super::copy_combo::is_shaking(combo) && !super::motion::reduced() {
                t = t.shaken(super::copy_combo::shake_offset(
                    self.copy_combo.shake_tick,
                    super::motion::reduced(),
                ));
            }
            t
        } else {
            // Module 276530 `eJ` + 206845: "Click to copy version" (`9Al4Qd`), 500ms delay.
            super::tooltip::Tip::text(dc_intl::t("9Al4Qd"), super::tooltip::Side::Top)
                .after(super::copy_combo::DEFAULT_DELAY_MS)
        };
        let version_row = div()
            .id(VERSION_KEY)
            .h(px(16.))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if *over {
                    this.copy_combo.on_hover_enter(VERSION_KEY);
                } else {
                    this.copy_combo.on_hover_leave(VERSION_KEY);
                }
                cx.notify();
            }))
            .on_click(cx.listener(|this, _, window, cx| {
                this.trigger_copy_combo(
                    VERSION_KEY,
                    super::copy_combo::build_version_copy_text(),
                    super::tooltip::Side::Top,
                    window,
                    cx,
                );
            }))
            .child(footnote(
                format!(
                    "{} {} ({})",
                    pin::RELEASE_CHANNEL,
                    pin::CLIENT_BUILD_NUMBER,
                    &pin::VERSION_HASH[..7]
                ),
                "--text-muted",
            ))
            .child(footnote(
                // The official line is the host's version; gravecord's is its own.
                format!("{} (x64)", env!("CARGO_PKG_VERSION")),
                "--text-muted",
            ));
        div()
            .flex()
            .flex_col()
            .child(self.with_tip(VERSION_KEY, tip, version_row, cx))
            .child(
                div()
                    .h(px(21.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(footnote(
                        dc_intl::tr("Privacy Policy").to_owned(),
                        "--text-link",
                    ))
                    .child(footnote("\u{2022}".to_owned(), "--text-muted"))
                    .child(footnote(
                        dc_intl::tr("Terms of Service").to_owned(),
                        "--text-link",
                    ))
                    .child(footnote("\u{2022}".to_owned(), "--text-muted"))
                    .child(footnote(dc_intl::tr("More").to_owned(), "--text-muted")),
            )
            .into_any_element()
    }

    /// A key moved focus onto a sidebar item: the list scrolls just far enough to show it.
    pub(super) fn reveal_settings_nav(&self, focused: &gpui::FocusHandle) {
        let Some(open) = self.settings.as_ref() else {
            return;
        };
        let Some(key) = self.keys.stops.key_of(focused) else {
            return;
        };
        let Some(icon) = key.strip_prefix("settings-nav-") else {
            return;
        };
        let anchors = self.tip_anchors.borrow();
        let Some(item) = anchors.get(&SharedString::from(format!("nav-item-{icon}"))) else {
            return;
        };
        let view = open.nav.bounds();
        let mut offset = open.nav.offset();
        if item.top() < view.top() {
            offset.y += view.top() - item.top();
        } else if item.bottom() > view.bottom() {
            offset.y -= item.bottom() - view.bottom();
        }
        open.nav.set_offset(offset);
    }

    fn settings_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (nav_scroll, nav_spring) = self
            .settings
            .as_ref()
            .map(|open| (open.nav.clone(), open.nav_spring.clone()))
            .unwrap_or_default();
        let shadow = nav_scroll.offset().y < px(-12.);
        let base: gpui::Hsla = color("--background-base-lower").into();
        let profile_card = self.settings_profile(cx);
        let query = self.settings_query(cx);
        // The search keeps the items whose panel matches, and the section labels above them (module 122088).
        let kept = |entry: &&Entry| {
            (entry.page != Page::Clips || self.clips_listed())
                && search::page_matches(entry.page, entry.label, &query)
        };
        let visible: Vec<_> = sections()
            .filter(|(_, entries)| entries.iter().any(|entry| kept(&entry)))
            .collect();
        let no_results = visible.is_empty();
        let search_box = self.settings_search(window, cx);
        let sections = visible.iter().enumerate().map(|(index, (label, entries))| {
            let last = index + 1 == visible.len();
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .pb(px(8.))
                .when(!last, |this| {
                    this.border_b_1().border_color(color("--border-subtle"))
                })
                .children(label.map(|label| {
                    div()
                        .h(px(30.))
                        .px(px(8.))
                        .pt(px(8.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::tr(label))
                }))
                .child(
                    div().flex().flex_col().gap(px(4.)).children(
                        entries
                            .iter()
                            // Clips is listed for Nitro only (`sw`, module 734066).
                            .filter(|entry| kept(entry))
                            .map(|entry| self.settings_nav_item(entry, window, cx)),
                    ),
                )
        });
        div()
            .parity_right("settings-sidebar", 1.)
            .flex_none()
            .w(px(SETTINGS_SIDEBAR_WIDTH))
            .h_full()
            .pt(px(16.))
            .pl(px(12.))
            .border_r_1()
            .border_color(color("--border-muted"))
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .pl(px(4.))
                    .pr(px(16.))
                    .child(div().pb(px(8.)).child(profile_card))
                    .child(search_box)
                    // `emptySearchResultsContainer_eCEVGd`: 24px above and below, the two lines 4px apart, centred.
                    .when(no_results, |this| {
                        this.child(
                            div()
                                .py(px(24.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .child(
                                    div()
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::tr("No search results found")),
                                )
                                .child(
                                    div()
                                        .text_color(color("--text-subtle"))
                                        .child(dc_intl::tr("We tried, but came up empty.")),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(scroll_driver(nav_scroll.clone(), nav_spring.clone()))
                    .child(
                        div()
                            .id("settings-nav")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&nav_scroll)
                            .on_scroll(move |_, _, _| nav_spring.borrow_mut().abort())
                            .child(
                                div()
                                    .pt(px(12.))
                                    .pb(px(16.))
                                    .pl(px(4.))
                                    .pr(px(16.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(24.))
                                    .child(div().flex().flex_col().gap(px(8.)).children(sections))
                                    .child(self.settings_footer(cx)),
                            ),
                    )
                    // `withShadow_cR4ic2::after`: once the list is scrolled past 12px, the fixed head fades into it.
                    .when(shadow, |this| {
                        this.child(div().absolute().top_0().left_0().w(px(224.)).h(px(40.)).bg(
                            gpui::linear_gradient(
                                180.,
                                gpui::linear_color_stop(base, 0.),
                                gpui::linear_color_stop(gpui::Hsla { a: 0., ..base }, 1.),
                            ),
                        ))
                    }),
            )
            .into_any_element()
    }

    fn account_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let account = &self.stores.account;
        let reveal_email = self.settings_extra.reveal_email;
        let reveal_phone = self.settings_extra.reveal_phone;
        let email = account.email.as_deref().map(|email| {
            if reveal_email {
                email.to_owned()
            } else {
                mask_email(email)
            }
        });
        // A button that opens the first dialog of its action (`actions::Dialog::Account`).
        let opens = |kind: &'static str,
                     id: &'static str,
                     label: &'static str,
                     tone: ButtonKind,
                     cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(format!("account-{kind}")))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_account_form(kind, cx);
                    cx.notify();
                }))
                .child(button(id, label, tone))
        };
        let marker = |index: usize| {
            self.anchor_probe(format!("settings-marker:{}", ACCOUNT_KEYS[index]))
                .into_any_element()
        };
        let info = category_marked(
            dc_intl::tr("Account Info"),
            marker(0),
            vec![
                Row::new(dc_intl::tr("Username")).trailing(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(value(user.username.clone()))
                        .child(opens(
                            "username",
                            "settings-edit",
                            dc_intl::tr("Edit"),
                            ButtonKind::Secondary,
                            cx,
                        )),
                ),
                Row::new(dc_intl::tr("Email")).trailing(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(
                            div().flex().gap(px(4.)).children(email.map(value)).child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-brand"))
                                    .cursor_pointer()
                                    .id("account-reveal-email")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_extra.reveal_email =
                                            !this.settings_extra.reveal_email;
                                        cx.notify();
                                    }))
                                    .child(if reveal_email {
                                        dc_intl::tr("Hide")
                                    } else {
                                        dc_intl::tr("Reveal")
                                    }),
                            ),
                        )
                        .child(opens(
                            "email",
                            "settings-edit-email",
                            dc_intl::tr("Edit"),
                            ButtonKind::Secondary,
                            cx,
                        )),
                ),
                Row::new(dc_intl::tr("Phone Number")).trailing(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(value(match &account.phone {
                            Some(phone) if reveal_phone => phone.clone(),
                            Some(phone) => mask_phone(phone),
                            None => dc_intl::tr("You haven't added a phone number yet.").to_owned(),
                        }))
                        .children(account.phone.as_ref().map(|_| {
                            div()
                                .id("account-reveal-phone")
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-brand"))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings_extra.reveal_phone =
                                        !this.settings_extra.reveal_phone;
                                    cx.notify();
                                }))
                                .child(if reveal_phone {
                                    dc_intl::tr("Hide")
                                } else {
                                    dc_intl::tr("Reveal")
                                })
                        }))
                        .child(opens(
                            "phone",
                            "settings-phone",
                            if account.phone.is_some() {
                                dc_intl::tr("Edit")
                            } else {
                                dc_intl::tr("Add")
                            },
                            ButtonKind::Secondary,
                            cx,
                        )),
                ),
            ],
        );
        let sessions = self
            .fetched
            .get("sessions")
            .and_then(|body| body["user_sessions"].as_array())
            .map(|list| devices(list.len()));
        let security = category_marked(
            dc_intl::tr("Password & Security"),
            marker(1),
            vec![
                Row::new(dc_intl::tr("Password")).trailing(opens(
                    "password",
                    "settings-edit-password",
                    dc_intl::tr("Edit"),
                    ButtonKind::Secondary,
                    cx,
                )),
                Row::new(dc_intl::tr("Multi-Factor Authentication"))
                    .link(Some(
                        if account.mfa_enabled {
                            dc_intl::tr("Enabled")
                        } else {
                            dc_intl::tr("Set up")
                        }
                        .to_owned(),
                    ))
                    .press(
                        "account-mfa",
                        cx.listener(|this, _, _, cx| {
                            this.account_panel_open(account_panels::Panel::Mfa, cx);
                            let _ = this.requests.send(mfa::credentials());
                            cx.notify();
                        }),
                    ),
                Row::new(dc_intl::tr("Logged-in Devices"))
                    .link(sessions)
                    .press(
                        "account-sessions",
                        cx.listener(|this, _, _, cx| {
                            this.account_panel_open(account_panels::Panel::Sessions, cx);
                        }),
                    ),
            ],
        );
        let standing = self.standing();
        let (tile_icon, tile_tint, tile_background) = account_nested::tile_look(standing);
        let root = cx.weak_entity();
        let status = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .pb(px(20.))
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Account Status")),
            )
            .child(div().relative().h(px(1.)).child(marker(2)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children((!self.stores.content_dismissed(AGE_NOTICE_CONTENT)).then(|| {
                        info_notice(
                            "account-age-notice",
                            &[
                                ("Age groups determine if you can access content and settings meant for adults. Otherwise, Discord works the same. See your age group below and ", false),
                                ("learn more on our blog", true),
                                (".", false),
                            ],
                            Some(Box::new({
                                let root = root.clone();
                                move |_, cx| {
                                    let _ = root.update(cx, |this, cx| {
                                        this.settings_open_url(AGE_GROUP_BLOG, cx);
                                    });
                                }
                            })),
                            Some(Box::new(cx.listener(|this, _, _, cx| {
                                this.dismiss_age_notice();
                                cx.notify();
                            }))),
                        )
                    }))
                    .child(
                        Row::new(dc_intl::tr("Age Group"))
                            .badge_if(!ACCOUNT_STATUS_SEEN.load(Ordering::Relaxed))
                            .leading(tile(
                                "settings_age_group",
                                "--background-mod-subtle",
                                "--icon-strong",
                            ))
                            // `W0/7DD` with `handleOnAgeGatedContentHook: _J`; the second link is
                            // `handleOnConfirmAgeHook` (age verification itself), not built.
                            .description(described_linked(
                                "account-age-group-text".into(),
                                &[
                                    ("Your age group hasn\u{2019}t been confirmed yet, so ", false),
                                    ("some content and settings", true),
                                    (" meant for adults aren\u{2019}t available until you ", false),
                                    ("confirm you\u{2019}re an adult", true),
                                    (".", false),
                                ],
                                "--text-subtle",
                                Box::new({
                                    let root = root.clone();
                                    move |index, cx| {
                                        if index == 0 {
                                            let _ = root.update(cx, |this, cx| {
                                                let url = this.age_gated_article_url();
                                                this.settings_open_url(&url, cx);
                                            });
                                        }
                                    }
                                }),
                            ))
                            .trailing(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(16.))
                                    .child(value(dc_intl::tr("Unconfirmed")))
                                    .child(button(
                                        "settings-age",
                                        dc_intl::tr("Confirm Age"),
                                        ButtonKind::Secondary,
                                    )),
                            )
                            .render(),
                    )
                    .child(
                        Row::new(dc_intl::tr("Account Standing"))
                            .leading(tile(tile_icon, tile_background, tile_tint))
                            .description(match standing {
                                // `x` of module 881979: the All good text's links open the terms and the
                                // guidelines (`TOS_LINK`, `COMMUNITY_GUIDELINES` of module 239093).
                                Some(standing) => standing_text(
                                    "account-standing-text",
                                    standing.description(),
                                    "--text-subtle",
                                    root.clone(),
                                )
                                .into_any_element(),
                                None => described(
                                    &[(dc_intl::tr("Loading your account standing..."), false)],
                                    "--text-subtle",
                                )
                                .into_any_element(),
                            })
                            .link(Some(
                                standing
                                    .map_or(dc_intl::tr("Loading"), account_nested::Standing::label)
                                    .to_owned(),
                            ))
                            .press(
                                "account-standing",
                                cx.listener(|this, _, _, cx| {
                                    this.account_panel_open(account_panels::Panel::Standing, cx);
                                }),
                            )
                            .render(),
                    ),
            )
            .into_any_element();
        let family = category_marked(
            dc_intl::tr("Family Center"),
            marker(3),
            vec![
                Row::new(dc_intl::tr("Set up Family Center"))
                    .description(described(
                        &[(
                            dc_intl::tr("Connect with a parent or guardian so they can stay in the loop. Your messages stay private and you\u{2019}ll always see exactly what they see."),
                            false,
                        )],
                        "--text-subtle",
                    ))
                    .link(None)
                    .press(
                        "account-family-center",
                        cx.listener(|this, _, _, cx| {
                            this.account_panel_open(account_panels::Panel::Family, cx);
                        }),
                    ),
            ],
        );
        let removal = category(
            None,
            vec![
                Row::new(dc_intl::tr("Disable your account"))
                    .description(described(
                        &[(dc_intl::tr("Temporarily disable your account."), false)],
                        "--text-subtle",
                    ))
                    .trailing(opens(
                        "disable",
                        "settings-disable",
                        dc_intl::tr("Disable Account"),
                        ButtonKind::CriticalSecondary,
                        cx,
                    )),
                Row::new(dc_intl::tr("Close your account"))
                    .description(described(
                        &[(dc_intl::tr("Permanently close your account."), false)],
                        "--text-subtle",
                    ))
                    .trailing(opens(
                        "delete",
                        "settings-delete",
                        dc_intl::tr("Delete Account"),
                        ButtonKind::CriticalPrimary,
                        cx,
                    )),
            ],
        );
        vec![
            info,
            divider(),
            security,
            divider(),
            status,
            divider(),
            family,
            divider(),
            removal,
        ]
    }

    fn settings_content(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.settings.as_ref() else {
            return div().into_any_element();
        };
        actions::link_owner_clear();
        let panel_width = panel_inner_width(window.viewport_size().width.into());
        let mut body = match open.page {
            Page::Account => match self.settings_extra.panel {
                Some(account_panels::Panel::Standing) => self.standing_body(cx),
                Some(account_panels::Panel::Family) => self.family_body(cx),
                Some(panel) => self.account_panel_page(panel, cx),
                None => self.account_page(cx),
            },
            Page::Clips => self.clips_page(window, cx),
            Page::Mods => self.mods_page(cx),
            Page::Plugins => self.plugins_page(window, cx),
            Page::Backup => self.backup_page(cx),
            Page::Updates => self.updates_page(cx),
            Page::ReportBug => self.report_bug_page(cx),
            page => {
                let mut els: Vec<AnyElement> = pages::nodes(page)
                    .iter()
                    .map(|node| {
                        div()
                            .parity(node.key())
                            .flex()
                            .flex_col()
                            .child(self.render_node(node, panel_width, window, cx))
                            .into_any_element()
                    })
                    .collect();
                // Module 868511 + 158935: Application Test Mode row under Developer when Developer Mode is on.
                if page == Page::Developer && self.developer_mode() {
                    let active_id = self.app_test_mode_active.application_id.clone();
                    els.push(
                        div()
                            .id("settings-app-test-mode-row")
                            .mt(px(12.))
                            .pt(px(16.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(16.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("erOqlh")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("52hMnD")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("settings-app-test-mode-open")
                                    .flex_none()
                                    .h(px(34.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_app_test_mode_modal(cx);
                                    }))
                                    .child(if active_id.is_some() {
                                        dc_intl::t("d6TR3I")
                                    } else {
                                        dc_intl::t("qwuK5I")
                                    }),
                            )
                            .into_any_element(),
                    );
                }
                els
            }
        };
        if open.page == Page::Appearance {
            body.insert(0, self.icon_set_picker(cx));
        }
        // Streamer Mode hiding personal information: the panel is the notice of module 369143.
        let cover = self.streamer_cover(open.page, cx);
        // A nested panel's header is a back button and the breadcrumb (`contentHeaderLeft_3Rvb29`).
        let nested = (open.page == Page::Account)
            .then_some(self.settings_extra.panel)
            .flatten()
            .filter(|panel| panel.has_breadcrumb());
        div()
            .parity("settings-content")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(client_theme::solid("--background-base-low"))
            .child(
                div()
                    .parity_below("settings-header", 1.)
                    .flex_none()
                    .h(px(49.))
                    .pl(px(if nested.is_some() { 8. } else { 16. }))
                    .pr(px(8.))
                    .border_b_1()
                    .border_color(color("--border-muted"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(match nested {
                        Some(panel) => self.nested_header(panel.title(), cx),
                        None => div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr(label_of(open.page)))
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .id("settings-close")
                            .size(px(32.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                            .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx)))
                            .child(mask_icon(
                                "settings_close",
                                16.,
                                "--control-icon-only-icon-default",
                            )),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(scroll_driver(
                        open.content.clone(),
                        open.content_spring.clone(),
                    ))
                    .child(
                        div()
                            .id("settings-content")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&open.content)
                            .on_scroll({
                                let spring = open.content_spring.clone();
                                move |_, _, _| spring.borrow_mut().abort()
                            })
                            .child(
                                div().flex().justify_center().px(px(16.)).child(
                                    div()
                                        .parity("settings-panel")
                                        .w_full()
                                        .min_w(px(300.))
                                        .max_w(px(728.))
                                        .ml(px(panel_nudge(window.viewport_size().width.into())))
                                        .my(px(64.))
                                        .px(px(16.))
                                        // `panel_nNq2Vb` clips what the Theme fieldset's min-content width pushes
                                        // past its right edge (the 200% captures).
                                        .overflow_x_hidden()
                                        .flex()
                                        .flex_col()
                                        .children(body),
                                ),
                            )
                            // `bottomMarker` (`data-panel-bottom`): a 1px block after the panel that every page ends
                            // with, which adds that pixel to the scrollable height.
                            .child(div().w_full().h(px(1.))),
                    )
                    .when_some(cover, |this, cover| {
                        this.child(
                            div()
                                .id("settings-streamer-cover")
                                .occlude()
                                .absolute()
                                .inset_0()
                                .bg(client_theme::solid("--background-base-low"))
                                .child(cover),
                        )
                    })
                    .when(open.page == Page::Accessibility, |this| {
                        this.child(self.accessibility_preview(window))
                    })
                    .child(auto_scrollbar(open.content.clone()))
                    .child(scroll_spy(
                        open.content.clone(),
                        open.content_spring.clone(),
                        open.anchor.clone(),
                        open.spy.clone(),
                        self.tip_anchors.clone(),
                        if open.page == Page::Clips {
                            clips::CATEGORIES.to_vec()
                        } else if open.page == Page::Account {
                            ACCOUNT_KEYS.to_vec()
                        } else {
                            pages::nodes(open.page)
                                .iter()
                                .filter_map(|node| match node {
                                    model::Node::Category {
                                        key,
                                        title: Some(_),
                                        ..
                                    } => Some(*key),
                                    _ => None,
                                })
                                .collect()
                        },
                        if open.page == Page::Account {
                            ACCOUNT_SCROLL_MARGIN
                        } else {
                            0.
                        },
                    )),
            )
            .into_any_element()
    }

    pub(super) fn settings_layer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.settings.as_ref()?;
        // User Settings is a modal (`layer-modal`, module 521489), not a layer of the layer stack: the scrim and the
        // dialog enter on the modal spring (module 150591, `scale(.9)` to 1).
        let pose = motion::modal_pose("settings");
        let mut scrim = color("--background-scrim");
        scrim.a *= pose.scrim;
        // At 1080 CSS pixels and under the modal fills the window (`modal__F9ieB`): no margin, border, radius or
        // shadow, the 32px top bar kept clear above it and a 1px `--border-muted` line under that.
        let full = f32::from(window.viewport_size().width) <= FULL_WINDOW_MODAL;
        let modal = div()
            .parity_with_border("settings-modal", if full { 0. } else { 1. })
            .id("settings-modal")
            .capture_any_mouse_down(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                if this.voice_mouse(event.button, true, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_any_mouse_up(cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                if this.voice_mouse(event.button, false, cx) {
                    cx.stop_propagation();
                }
            }))
            .opacity(pose.opacity)
            .paint_scale(pose.scale, point(0.5, 0.5))
            .occlude()
            .size_full()
            .max_w(px(1400.))
            .when(!full, |this| {
                this.rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.24),
                        offset: point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
            })
            .when(full, |this| this.pt(px(32.)))
            .bg(client_theme::solid("--background-base-lower"))
            .overflow_hidden()
            .flex()
            .font_family(SANS)
            .font_features(kerning())
            .child(
                div()
                    .size_full()
                    .flex()
                    .when(full, |this| {
                        this.border_t_1().border_color(color("--border-muted"))
                    })
                    .child(self.settings_sidebar(window, cx))
                    .child(self.settings_content(window, cx)),
            );
        // The window's own buttons stay above the scrim, and they are not part of the dialog: drawn as a layer of
        // their own, the copy of the dialog that plays its exit does not carry them.
        let layer = deferred(
            div()
                .id("settings-layer")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(scrim)
                .p(px(if full {
                    0.
                } else {
                    modal_padding(f32::from(window.viewport_size().width))
                }))
                .flex()
                .justify_center()
                // A press on the scrim beside the dialog closes User Settings, as the X does (module 521489: the
                // modal's backdrop). The dialog occludes, so a press inside it never reaches this.
                .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx)))
                .child(modal),
        )
        .with_priority(1);
        // The window's own buttons and the drag area above the scrim are the app's fixed system bar
        // (`Root::system_bar`), drawn over every dialog, so the copy of the dialog that plays its exit does not
        // carry them.
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(layer)
                .into_any_element(),
        )
    }
}

/// `scrollSidebarNodeIntoView` (`block: "nearest"`, 12px of padding): the new scroll amount that brings a node, at
/// `top`..`bottom` on screen, inside the scroller's `view_top`..`view_bottom`, moving as little as it can.
fn nearest_scroll(
    scroll: f32,
    (view_top, view_bottom): (f32, f32),
    (top, bottom): (f32, f32),
    padding: f32,
    max: f32,
) -> f32 {
    let delta = if top < view_top + padding {
        top - (view_top + padding)
    } else if bottom > view_bottom - padding {
        // Chromium's closest edge for a node below the view is its bottom, taller than the view or not: the 200 %
        // capture (`zoom/settings-appearance-200-top`: view 322..720 device px, the last subnav row ending 24 px
        // above the view's end, the subnav's top scrolled out) shows it.
        bottom - (view_bottom - padding)
    } else {
        0.
    };
    (scroll + delta).clamp(0., max.max(0.))
}

/// Once, when a page opens and its subnav has its full height: scrolls the sidebar so the subnav is in view, at
/// once for a page opened by a link, gliding after a click (the subnav's `onMount`, module 276530). Fills its
/// parent, the subnav.
fn nav_into_view(
    nav: ScrollHandle,
    spring: Rc<RefCell<ScrollSpring>>,
    pending: Rc<Cell<Reveal>>,
    expanded: bool,
    anchors_map: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
    icon: &'static str,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let mode = pending.get();
            if mode == Reveal::Done || !expanded {
                return;
            }
            let view = nav.bounds();
            if view.size.height <= px(0.) || bounds.size.height <= px(0.) {
                return;
            }
            pending.set(Reveal::Done);
            let max = f32::from(nav.max_offset().y);
            // While the click's own glide runs, the subnav's is measured from where that one is going.
            let scroll = -f32::from(nav.offset().y);
            let (base, delta) = {
                let spring_ref = spring.borrow();
                if spring_ref.is_animating() && mode != Reveal::Instant && !motion::reduced() {
                    let target = (spring_ref.target() as f32).clamp(0., max);
                    (target, scroll - target)
                } else {
                    (scroll, 0.)
                }
            };
            let item_top = anchors_map
                .borrow()
                .get(&SharedString::from(format!("nav-item-{icon}")))
                .map(|b| f32::from(b.top()) + delta);
            let subnav_top = f32::from(bounds.top()) + delta;
            let top = item_top.map_or(subnav_top, |t| t.min(subnav_top));
            let bottom = f32::from(bounds.bottom()) + delta;
            let target = nearest_scroll(
                base,
                (f32::from(view.top()), f32::from(view.bottom())),
                (top, bottom),
                12.,
                max,
            );
            if (target - base).abs() < 0.5 {
                return;
            }
            if mode == Reveal::Instant || motion::reduced() {
                spring.borrow_mut().abort();
                nav.set_offset(point(nav.offset().x, px(-target)));
            } else {
                spring
                    .borrow_mut()
                    .start(f64::from(scroll), f64::from(target));
            }
            window.refresh();
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Paints nothing: moves a scroller along its animated scroll each frame, before the scroller is laid out. The wheel
/// takes over: each scroller's `on_scroll_wheel` lets the spring go.
fn scroll_driver(handle: ScrollHandle, spring: Rc<RefCell<ScrollSpring>>) -> impl IntoElement {
    canvas(
        move |_, window, _| {
            let mut spring = spring.borrow_mut();
            if !spring.is_animating() {
                return;
            }
            let max = f32::from(handle.max_offset().y).max(0.);
            // The official spring (module 354730) sets `scrollTop` until it lands; only the wheel stops it (the
            // scrollers' `on_scroll`). Comparing positions here also stopped it for a touchpad's trailing
            // momentum, and a click then seemed to do nothing.
            if let Some(position) = spring.frame(motion::now()) {
                let position = (position as f32).clamp(0., max);
                handle.set_offset(point(handle.offset().x, px(-position)));
            }
            if spring.is_animating() {
                motion::keep_animating();
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// The scroll-spy (`IntersectionObserver` over the category markers, threshold 1): the first marker fully in view
/// lights its entry, the last one when the content is scrolled to its end. `markers` are the viewport-relative
/// `top`..`bottom` of each entry's marker, None when it is not on this page's screen.
fn spy_choice(markers: &[Option<(f32, f32)>], view: (f32, f32), at_end: bool) -> Option<usize> {
    let mut seen = markers.iter().enumerate().filter_map(|(index, marker)| {
        marker
            .filter(|(top, bottom)| *top >= view.0 && *bottom <= view.1)
            .map(|_| index)
    });
    if at_end {
        seen.next_back()
    } else {
        seen.next()
    }
}

/// Paints nothing; every frame it reads where the markers are and moves the lit subnav entry. Fills its parent.
fn scroll_spy(
    content: ScrollHandle,
    glide: Rc<RefCell<ScrollSpring>>,
    anchor: Rc<Cell<usize>>,
    spy: Rc<Cell<(f32, bool)>>,
    markers: Rc<std::cell::RefCell<std::collections::HashMap<SharedString, Bounds<gpui::Pixels>>>>,
    keys: Vec<&'static str>,
    margin: f32,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            // A subnav click or deep link: the marker of the category goes to the top of the view (`scrollIntoView`).
            if let Some(key) = PENDING_SCROLL.get() {
                let top = markers
                    .borrow()
                    .get(&SharedString::from(format!("settings-marker:{key}")))
                    .map(|bounds| bounds.top());
                if let Some(top) = top {
                    PENDING_SCROLL.set(None);
                    let offset = content.offset();
                    let wanted = (offset.y - (top - content.bounds().top() - px(margin)))
                        .clamp(-content.max_offset().y, px(0.));
                    if PENDING_GLIDE.replace(false) && !motion::reduced() {
                        glide.borrow_mut().start(
                            -f64::from(f32::from(offset.y)),
                            -f64::from(f32::from(wanted)),
                        );
                    } else {
                        glide.borrow_mut().abort();
                        content.set_offset(point(offset.x, wanted));
                    }
                    window.refresh();
                }
            }
            let offset = f32::from(content.offset().y);
            let (seen, skip) = spy.get();
            if (offset - seen).abs() < 0.01 {
                return;
            }
            // The click's glide is its own scroll from start to end; the first scroll after it lets the spy
            // choose again (`setPanelInitialScrollListener`, module 397274).
            if glide.borrow().is_animating() {
                spy.set((offset, true));
                return;
            }
            // The first change after a click is the click's own scroll.
            spy.set((offset, false));
            if skip {
                return;
            }
            let view = content.bounds();
            let at_end = f32::from(content.max_offset().y) + offset <= 0.5;
            let found: Vec<Option<(f32, f32)>> = keys
                .iter()
                .map(|key| {
                    markers
                        .borrow()
                        .get(&SharedString::from(format!("settings-marker:{key}")))
                        .map(|bounds| (f32::from(bounds.top()), f32::from(bounds.bottom())))
                })
                .collect();
            if let Some(index) = spy_choice(
                &found,
                (f32::from(view.top()), f32::from(view.bottom())),
                at_end,
            ) && anchor.get() != index
            {
                anchor.set(index);
                window.refresh();
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The settings content column is `728` wide and centred in the area right of the sidebar. When the spare width is odd
/// Chromium lays the panel on a half pixel and paints it at the next whole one; the layout here would round it down.
/// A 1px left margin moves the centre by half a pixel, which makes it land on that whole pixel.
fn panel_nudge(viewport_width: f32) -> f32 {
    let content = content_width(viewport_width);
    let spare = content - 728.;
    if spare > 0. && spare.rem_euclid(2.) == 1. {
        1.
    } else {
        0.
    }
}

/// The width the page rows get: the content column (the modal less its border and the sidebar) held between the panel's
/// `min-width: 300px` and 728, less its 16px of padding on each side (`panel_nNq2Vb`). 696 in a window of ordinary size,
/// narrower when the zoom leaves the window fewer logical pixels.
fn panel_inner_width(viewport_width: f32) -> f32 {
    // The column keeps 16px clear on each side of the panel box (the 200 % capture: 387 wide content, panel box 355,
    // rows 323).
    (content_width(viewport_width) - 32.).clamp(300., 728.) - 32.
}

/// The viewport width, in CSS pixels, at and under which the modal fills the window (`@container outer-container
/// (max-width: 1080px)` of `modal__F9ieB`: no border, no radius, no margin).
pub(super) const FULL_WINDOW_MODAL: f32 = 1080.;

/// `--custom-viewport-padding` of `modal__F9ieB`: 40, and the 32px top bar more above 1600 CSS pixels.
fn modal_padding(viewport_width: f32) -> f32 {
    if viewport_width > 1600. { 72. } else { 40. }
}

/// The width of the content column: the modal less its border and the sidebar; the whole window less the sidebar
/// once the modal fills it.
fn content_width(viewport_width: f32) -> f32 {
    if viewport_width <= FULL_WINDOW_MODAL {
        viewport_width - SETTINGS_SIDEBAR_WIDTH
    } else {
        (viewport_width - 2. * modal_padding(viewport_width)).min(1400.)
            - 2.
            - SETTINGS_SIDEBAR_WIDTH
    }
}

/// The settings sidebar's width, `sidebar_cR4ic2`: 253 inside the modal's border.
const SETTINGS_SIDEBAR_WIDTH: f32 = 253.;

/// `auto_tqapBM`: a 16px gutter whose thumb keeps 4px clear on every side, at least 40px tall.
pub(in crate::ui) fn auto_scrollbar(handle: ScrollHandle) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let max = handle.max_offset().y;
            if max <= px(0.) {
                return;
            }
            let viewport = bounds.size.height;
            let content = viewport + max;
            let thumb = (viewport * (viewport / content)).max(px(40.));
            let scrolled = -handle.offset().y;
            let top = bounds.top() + (viewport - thumb) * (scrolled / max);
            let area = Bounds::new(
                point(bounds.right() - px(12.), top + px(4.)),
                size(px(8.), thumb - px(8.)),
            );
            window.paint_quad(fill(area, color("--scrollbar-auto-thumb")).corner_radii(px(4.)));
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .w(px(16.))
    .h_full()
}

/// `Nx` in the settings bundle: all but the last four characters become stars.
fn mask_phone(phone: &str) -> String {
    let count = phone.chars().count();
    let shown: String = phone.chars().skip(count.saturating_sub(4)).collect();
    format!("{}{shown}", "*".repeat(count.saturating_sub(4)))
}

#[cfg(test)]
mod tests {
    use super::{age_gated_article, devices, mask_email, mask_phone, run_ranges};

    /// The subnav thumb (module 276530's `{mass: .1, friction: 20, tension: 300}`) glides one 36px row: a third of
    /// the way in 30 ms, never overshoots (the spring is overdamped), and is within half a pixel by 300 ms.
    #[test]
    fn the_subnav_thumb_glides_to_the_next_row() {
        let mut spring = super::motion::Spring::new(super::SUBNAV_THUMB_SPRING, 0., 0.);
        spring.retarget(36., 0.);
        spring.step_to(30.);
        assert!(spring.position() > 9. && spring.position() < 18.);
        let mut highest: f64 = 0.;
        for now in 31..=300 {
            spring.step_to(f64::from(now));
            highest = highest.max(spring.position());
        }
        assert!(highest <= 36.);
        assert!((spring.position() - 36.).abs() < 0.5);
    }

    #[test]
    fn age_group_links_go_where_the_bundle_sends_them() {
        assert_eq!(
            age_gated_article(None),
            "https://support.discord.com/hc/en-us/articles/42178056096663#h_01M0XEC24HG29RFJ4C04NM3SXM"
        );
        assert!(
            age_gated_article(Some("GB")).ends_with("33362401287959-Age-Assurance-for-UK-Users")
        );
        assert_eq!(age_gated_article(Some("TR")), age_gated_article(None));
        // The country READY reports picks the article (`LocationMetadataStore.getCountryCode`).
        let mut stores = dc_state::Stores::default();
        stores.dispatch("READY", &serde_json::json!({"country_code": "BR"}));
        assert!(
            age_gated_article(stores.location_metadata.get_country_code())
                .ends_with("38860612202775-Age-Assurance-for-Brazilian-Users")
        );
        let (text, links) = run_ranges(&[("see ", false), ("blog", true), (".", false)]);
        assert_eq!(&text[links[0].clone()], "blog");
    }

    #[test]
    fn email_and_phone_masks_match_the_bundle() {
        assert_eq!(mask_email("someone@gmail.com"), "*******@gmail.com");
        assert_eq!(mask_phone("+15551234567"), "********4567");
    }

    #[test]
    fn page_slugs_match_the_capture_names() {
        assert_eq!(super::page_slug("Voice & Video"), "voice-video");
        assert_eq!(super::page_slug("Data & Privacy"), "data-privacy");
    }

    /// The sidebar, its subnav and the Language page go through `dc_intl::tr`: every label is an en-US message.
    #[test]
    fn sidebar_and_language_labels_are_official_messages() {
        let known = |text: &str| dc_intl::keys_of(text).next().is_some();
        for (section, entries) in super::SECTIONS {
            if let Some(section) = section {
                assert!(known(section), "{section:?}");
            }
            for entry in entries.iter() {
                assert!(known(entry.label), "{:?}", entry.label);
                for anchor in super::anchors_of(entry) {
                    assert!(known(anchor.label), "{:?}", anchor.label);
                }
            }
        }
        assert!(known("Edit Profiles") && known("Search"));
        for node in super::pages::nodes(super::Page::LanguageTime) {
            let super::model::Node::Category { children, .. } = node else {
                continue;
            };
            for child in children.iter() {
                match child {
                    super::model::Node::Select {
                        label, description, ..
                    } => {
                        assert!(known(label), "{label:?}");
                        assert!(known(description[0].0), "{:?}", description[0].0);
                    }
                    super::model::Node::Radio { label, options, .. } => {
                        assert!(known(label), "{label:?}");
                        for option in options.iter() {
                            assert!(known(option.label), "{:?}", option.label);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn device_counts_use_the_plural_rule() {
        assert_eq!(devices(1), "1 device");
        assert_eq!(devices(3), "3 devices");
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    /// Voice & Video in `voice-video-advanced3`: the subnav ends at 887 unscrolled, the list's view is 169..679.
    #[test]
    fn the_sidebar_scrolls_just_enough_to_show_the_subnav() {
        let scroll = nearest_scroll(0., (169., 679.), (671., 887.), 12., 748.);
        assert_eq!(scroll, 220.);
        // Already inside the padded view: no move.
        assert_eq!(
            nearest_scroll(0., (169., 679.), (300., 500.), 12., 748.),
            0.
        );
        // Above the view: scrolls back up, never below zero.
        assert_eq!(
            nearest_scroll(100., (169., 679.), (150., 300.), 12., 748.),
            69.
        );
        assert_eq!(
            nearest_scroll(5., (169., 679.), (100., 300.), 12., 748.),
            0.
        );
        // Past the end of the list.
        assert_eq!(
            nearest_scroll(0., (169., 679.), (671., 1500.), 12., 300.),
            300.
        );
        // Taller than the view and below it: the bottom aligns, the top goes out of sight (the 200 % settings).
        assert_eq!(
            nearest_scroll(0., (161., 360.), (434., 650.), 12., 1000.),
            650. - (360. - 12.)
        );
    }

    #[test]
    fn the_first_marker_in_view_is_lit_and_the_last_at_the_end() {
        let view = (90., 679.);
        // advanced3: Streaming and Sounds are in view.
        let markers = [
            None,
            Some((-653., -652.)),
            Some((277., 278.)),
            Some((549., 550.)),
            None,
        ];
        assert_eq!(spy_choice(&markers, view, false), Some(2));
        // advanced: Advanced is in view at the end of the page.
        let markers = [None, None, None, Some((-72., -71.)), Some((455., 456.))];
        assert_eq!(spy_choice(&markers, view, true), Some(4));
        assert_eq!(spy_choice(&[None, None], view, false), None);
        // A marker half out of the view does not count.
        assert_eq!(spy_choice(&[Some((89., 90.))], view, false), None);
    }

    #[test]
    fn an_odd_spare_width_nudges_the_panel_to_the_next_pixel() {
        // 1280 wide: 1200 modal, 945 content, 217 spare.
        assert_eq!(panel_nudge(1280.), 1.);
        assert_eq!(panel_nudge(1281.), 0.);
        assert_eq!(panel_nudge(600.), 0.);
    }
}
