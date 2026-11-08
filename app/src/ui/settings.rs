//! User settings. Shell from `modal__F9ieB`, sidebar `sidebar_cR4ic2`, nav items `item_xyJHva`, subnav `subnav_8qMUr2`,
//! content `content_3Rvb29`, panel `panel_nNq2Vb`, rows `baseControlItem_FiiQ6I`, buttons `button_F7qvhY`.
//! Sizes and colors from `design/dom/settings/account-darker-cozy.json`; strings from `STRINGS/en-US.json`.

mod appearance;
mod model;
mod pages;
mod render;
pub(crate) use render::radio_dot;
mod swatches;

use super::{Root, SANS, avatar_source, color, kerning, window_controls};
use crate::net::Request;
use crate::parity::Parity;
use dc_proto::pin;
use gpui::{
    AnyElement, Bounds, BoxShadow, Context, ElementId, FontWeight, HighlightStyle, IntoElement,
    ScrollHandle, SharedString, StyledText, Window, canvas, deferred, div, fill, hsla, img, point,
    prelude::*, px, size, svg,
};
pub(super) use model::Choice;
use model::{ButtonKind, Runs};

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
    GameOverlay,
    ConnectedApps,
    Developer,
    LogOut,
}

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

fn label_of(page: Page) -> &'static str {
    SECTIONS
        .iter()
        .flat_map(|(_, entries)| entries.iter())
        .find(|entry| entry.page == page)
        .map_or("", |entry| entry.label)
}

pub(super) struct Open {
    page: Page,
    anchor: usize,
    content: ScrollHandle,
    nav: ScrollHandle,
}

impl Open {
    fn new(page: Page) -> Self {
        Self {
            page,
            anchor: 0,
            content: ScrollHandle::new(),
            nav: ScrollHandle::new(),
        }
    }
}

struct Anchor {
    label: &'static str,
    badge: bool,
    /// The panel child the subnav scrolls to.
    target: usize,
}

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
            })
            .collect();
    }
    pages::nodes(entry.page)
        .iter()
        .enumerate()
        .filter_map(|(target, node)| match node {
            model::Node::Category {
                title: Some(title), ..
            } => Some(Anchor {
                label: title,
                badge: false,
                target,
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

fn mask_icon(path: &str, size: f32, tint: &str) -> gpui::Svg {
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

fn styled_runs(parts: &[(&str, bool)], base: &str, link_style: HighlightStyle) -> impl IntoElement {
    let mut text = String::new();
    let mut links = Vec::new();
    for (part, link) in parts {
        let start = text.len();
        text.push_str(part);
        if *link {
            links.push((start..text.len(), link_style));
        }
    }
    div()
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(color(base))
        .child(StyledText::new(text).with_highlights(links))
}

struct Row {
    title: &'static str,
    badge: bool,
    description: Option<AnyElement>,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    clickable: bool,
}

impl Row {
    fn new(title: &'static str) -> Self {
        Self {
            title,
            badge: false,
            description: None,
            leading: None,
            trailing: None,
            clickable: false,
        }
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

    fn badge(mut self) -> Self {
        self.badge = true;
        self
    }

    fn render(self) -> AnyElement {
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
                    .child(self.title),
            )
            .when(self.badge, |this| this.child(new_badge()));
        div()
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
            )
            .into_any_element()
    }
}

fn new_badge() -> impl IntoElement {
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
        .child("NEW")
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
fn category_header(title: &'static str, description: Runs) -> impl IntoElement {
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
                        .child(title),
                )
                .when(!description.is_empty(), |this| {
                    this.child(described(description, "--text-subtle"))
                }),
        )
        .child(div().h(px(1.)))
}

/// Heading `heading-xl/medium` with 20px under it, then the rows 8px apart.
fn category(title: Option<&'static str>, rows: Vec<Row>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .children(title.map(|title| category_header(title, &[])))
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

/// `container_0v8Ae1 info_0v8Ae1`.
fn info_notice(parts: Runs) -> impl IntoElement {
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
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(noticed(parts, "--text-strong")),
                ),
        )
        .child(
            div()
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
                )),
        )
}

impl Root {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = Some(Open::new(Page::Account));
        // The custom theme card's banner (`bannerImage_yH_g_W`).
        self.request_image(
            "theme_upsell_banner".to_owned(),
            "https://cdn.discordapp.com/assets/content/568728735ebc2ee3eb3a7ab29d37c1f698f413d29cee50f9a19ed2d2eefea27f.svg"
                .to_owned(),
        );
        let _ = self.requests.send(Request::Fetch {
            key: "sessions".to_owned(),
            route: "/auth/sessions".to_owned(),
        });
        cx.notify();
    }

    /// Opens settings on the page whose label, lower case with dashes, is `slug`: "data-privacy", "voice-video".
    pub(crate) fn open_settings_page(&mut self, slug: &str, cx: &mut Context<Self>) {
        self.open_settings(cx);
        let page = SECTIONS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .find(|entry| page_slug(entry.label) == slug)
            .map(|entry| entry.page);
        if let (Some(open), Some(page)) = (self.settings.as_mut(), page) {
            *open = Open::new(page);
        }
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = None;
        cx.notify();
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
        let anchors = if active {
            anchors_of(entry)
        } else {
            Vec::new()
        };
        let page = entry.page;
        let item = div()
            .id(SharedString::from(format!("settings-nav-{}", entry.icon)))
            .h(px(36.))
            .p(px(8.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .text_color(color(if active {
                "--text-strong"
            } else {
                "--text-muted"
            }))
            .when(active, |this| this.bg(color("--background-mod-normal")))
            .when(!active, |this| {
                this.hover(move |style| {
                    style
                        .bg(color(if destructive {
                            "--background-feedback-mod-critical"
                        } else {
                            "--background-mod-muted"
                        }))
                        .text_color(color("--text-strong"))
                })
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(open) = this.settings.as_mut()
                    && page != Page::LogOut
                {
                    *open = Open::new(page);
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
                            .text_color(color(if active {
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
                            .child(entry.label),
                    ),
            );
        div()
            .flex()
            .flex_col()
            .child(item)
            .when_some(
                (active && !anchors.is_empty()).then_some(anchors),
                |this, anchors| this.child(self.settings_subnav(&anchors, window, cx)),
            )
            .into_any_element()
    }

    /// The grid of `subnav_8qMUr2`: a 2px track 17px in, labels 17px past it and 16px apart.
    /// Labels wrap in the 184px column, so the thumb is placed by the measured heights above it.
    fn settings_subnav(
        &self,
        anchors: &[Anchor],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.settings.as_ref().map_or(0, |open| open.anchor);
        let heights: Vec<f32> = anchors
            .iter()
            .map(|anchor| 20. * wrapped_lines(window, anchor.label, anchor.badge) as f32)
            .collect();
        let total = heights.iter().sum::<f32>() + 16. * heights.len().saturating_sub(1) as f32;
        let thumb_top: f32 = heights
            .iter()
            .take(current)
            .map(|height| height + 16.)
            .sum();
        let thumb_height = heights.get(current).copied().unwrap_or(20.);
        let track = div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(2.))
            .h(px(total))
            .rounded(px(1.))
            .bg(color("--background-mod-subtle"))
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(thumb_top))
                    .w(px(2.))
                    .h(px(thumb_height))
                    .rounded_full()
                    .bg(color("--interactive-text-active")),
            );
        let rows: Vec<AnyElement> = anchors
            .iter()
            .enumerate()
            .map(|(index, anchor)| {
                let active = index == current;
                let target = anchor.target;
                div()
                    .id(SharedString::from(format!("settings-anchor-{index}")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .cursor_pointer()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color(if active {
                        "--interactive-text-active"
                    } else {
                        "--text-muted"
                    }))
                    .hover(|style| style.text_color(color("--interactive-text-hover")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this.settings.as_mut() {
                            open.anchor = index;
                            open.content.scroll_to_top_of_item(target);
                        }
                        cx.notify();
                    }))
                    .child(div().min_w(px(0.)).child(anchor.label))
                    .when(anchor.badge, |this| this.child(new_badge()))
                    .into_any_element()
            })
            .collect();
        div()
            .ml(px(17.))
            .py(px(8.))
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

    fn settings_profile(&self) -> AnyElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        div()
            .h(px(64.))
            .p(px(8.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .hover(|style| style.bg(color("--background-mod-muted")))
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
                            .text_color(color("--text-muted"))
                            .child("Edit Profiles")
                            .child(mask_icon("settings_edit_pencil", 12., "--text-muted")),
                    ),
            )
            .into_any_element()
    }

    fn settings_search(&self) -> AnyElement {
        div()
            .h(px(40.))
            .pl(px(4.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .child(
                div()
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(mask_icon("settings_search", 16., "--icon-default")),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--input-placeholder-text-default"))
                    .child("Search"),
            )
            .into_any_element()
    }

    fn settings_footer(&self) -> AnyElement {
        let footnote = |text: String, tint: &str| {
            div()
                .text_size(px(10.))
                .line_height(px(12.))
                .text_color(color(tint))
                .child(text)
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
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
                        format!("{} (x64)", pin::CLIENT_VERSION),
                        "--text-muted",
                    )),
            )
            .child(
                div()
                    .h(px(21.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(footnote("Privacy Policy".to_owned(), "--text-link"))
                    .child(footnote("\u{2022}".to_owned(), "--text-muted"))
                    .child(footnote("Terms of Service".to_owned(), "--text-link"))
                    .child(footnote("\u{2022}".to_owned(), "--text-muted"))
                    .child(footnote("More".to_owned(), "--text-muted")),
            )
            .into_any_element()
    }

    fn settings_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let nav_scroll = self
            .settings
            .as_ref()
            .map(|open| open.nav.clone())
            .unwrap_or_default();
        let sections = SECTIONS
            .iter()
            .enumerate()
            .map(|(index, (label, entries))| {
                let last = index + 1 == SECTIONS.len();
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
                            .child(label)
                    }))
                    .child(
                        div().flex().flex_col().gap(px(4.)).children(
                            entries
                                .iter()
                                .map(|entry| self.settings_nav_item(entry, window, cx)),
                        ),
                    )
            });
        div()
            .parity_right("settings-sidebar", 1.)
            .flex_none()
            .w(px(253.))
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
                    .child(div().pb(px(8.)).child(self.settings_profile()))
                    .child(self.settings_search()),
            )
            .child(
                div()
                    .id("settings-nav")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&nav_scroll)
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
                            .child(self.settings_footer()),
                    ),
            )
            .into_any_element()
    }

    fn account_page(&self) -> Vec<AnyElement> {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let account = &self.stores.account;
        let email = account.email.as_deref().map(mask_email);
        let edit = || button("settings-edit", "Edit", ButtonKind::Secondary);
        let info = category(
            Some("Account Info"),
            vec![
                Row::new("Username").trailing(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(value(user.username.clone()))
                        .child(edit()),
                ),
                Row::new("Email").trailing(
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
                                    .child("Reveal"),
                            ),
                        )
                        .child(edit()),
                ),
                Row::new("Phone Number").trailing(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(value(match &account.phone {
                            Some(phone) => mask_phone(phone),
                            None => "You haven't added a phone number yet.".to_owned(),
                        }))
                        .child(button(
                            "settings-phone",
                            if account.phone.is_some() {
                                "Edit"
                            } else {
                                "Add"
                            },
                            ButtonKind::Secondary,
                        )),
                ),
            ],
        );
        let sessions = self
            .fetched
            .get("sessions")
            .and_then(|body| body["user_sessions"].as_array())
            .map(|list| devices(list.len()));
        let security = category(
            Some("Password & Security"),
            vec![
                Row::new("Password").trailing(edit()),
                Row::new("Multi-Factor Authentication")
                    .link((!account.mfa_enabled).then(|| "Set up".to_owned())),
                Row::new("Logged-in Devices").link(sessions),
            ],
        );
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
                    .child("Account Status"),
            )
            .child(div().h(px(1.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(info_notice(&[
                        ("Age groups determine if you can access content and settings meant for adults. Otherwise, Discord works the same. See your age group below and ", false),
                        ("learn more on our blog", true),
                        (".", false),
                    ]))
                    .child(
                        Row::new("Age Group")
                            .badge()
                            .leading(tile(
                                "settings_age_group",
                                "--background-mod-subtle",
                                "--icon-strong",
                            ))
                            .description(described(
                                &[
                                    ("Your age group hasn\u{2019}t been confirmed yet, so ", false),
                                    ("some content and settings", true),
                                    (" meant for adults aren\u{2019}t available until you ", false),
                                    ("confirm you\u{2019}re an adult", true),
                                    (".", false),
                                ],
                                "--text-subtle",
                            ))
                            .trailing(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(16.))
                                    .child(value("Unconfirmed"))
                                    .child(button(
                                        "settings-age",
                                        "Confirm Age",
                                        ButtonKind::Secondary,
                                    )),
                            )
                            .render(),
                    )
                    .child(
                        Row::new("Account Standing")
                            .leading(tile(
                                "settings_standing",
                                "--background-feedback-mod-positive",
                                "--icon-feedback-positive",
                            ))
                            .description(described(
                                &[
                                    ("Thank you for upholding Discord's ", false),
                                    ("Terms of Service", true),
                                    (" and ", false),
                                    ("Community Guidelines", true),
                                    (". If you have a violation, it will appear here.", false),
                                ],
                                "--text-subtle",
                            ))
                            .link(Some("All good".to_owned()))
                            .render(),
                    ),
            )
            .into_any_element();
        let family = category(
            Some("Family Center"),
            vec![
                Row::new("Set up Family Center")
                    .description(described(
                        &[(
                            "Connect with a parent or guardian so they can stay in the loop. Your messages stay private and you\u{2019}ll always see exactly what they see.",
                            false,
                        )],
                        "--text-subtle",
                    ))
                    .link(None),
            ],
        );
        let removal = category(
            None,
            vec![
                Row::new("Disable your account")
                    .description(described(
                        &[("Temporarily disable your account.", false)],
                        "--text-subtle",
                    ))
                    .trailing(button(
                        "settings-disable",
                        "Disable Account",
                        ButtonKind::CriticalSecondary,
                    )),
                Row::new("Close your account")
                    .description(described(
                        &[("Permanently close your account.", false)],
                        "--text-subtle",
                    ))
                    .trailing(button(
                        "settings-delete",
                        "Delete Account",
                        ButtonKind::CriticalPrimary,
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
        let body = match open.page {
            Page::Account => self.account_page(),
            page => pages::nodes(page)
                .iter()
                .map(|node| {
                    div()
                        .parity(node.key())
                        .flex()
                        .flex_col()
                        .child(self.render_node(node, render::PANEL, window, cx))
                        .into_any_element()
                })
                .collect(),
        };
        div()
            .parity("settings-content")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(color("--background-base-low"))
            .child(
                div()
                    .parity_below("settings-header", 1.)
                    .flex_none()
                    .h(px(49.))
                    .pl(px(16.))
                    .pr(px(8.))
                    .border_b_1()
                    .border_color(color("--border-muted"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(label_of(open.page)),
                    )
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
                    .child(
                        div()
                            .id("settings-content")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&open.content)
                            .child(
                                div().flex().justify_center().child(
                                    div()
                                        .parity("settings-panel")
                                        .w_full()
                                        .max_w(px(728.))
                                        .my(px(64.))
                                        .px(px(16.))
                                        .flex()
                                        .flex_col()
                                        .children(body),
                                ),
                            ),
                    )
                    .child(auto_scrollbar(open.content.clone())),
            )
            .into_any_element()
    }

    pub(super) fn settings_layer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.settings.as_ref()?;
        let modal = div()
            .parity_with_border("settings-modal", 1.)
            .id("settings-modal")
            .occlude()
            .size_full()
            .max_w(px(1400.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .overflow_hidden()
            .flex()
            .font_family(SANS)
            .font_features(kerning())
            .child(self.settings_sidebar(window, cx))
            .child(self.settings_content(window, cx));
        Some(
            deferred(
                div()
                    .id("settings-layer")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(color("--background-scrim"))
                    .p(px(40.))
                    .flex()
                    .justify_center()
                    .child(modal)
                    .child(div().absolute().top_0().right_0().child(window_controls())),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// `auto_tqapBM`: a 16px gutter whose thumb keeps 4px clear on every side, at least 40px tall.
fn auto_scrollbar(handle: ScrollHandle) -> impl IntoElement {
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
    use super::{devices, mask_email, mask_phone};

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

    #[test]
    fn device_counts_use_the_plural_rule() {
        assert_eq!(devices(1), "1 device");
        assert_eq!(devices(3), "3 devices");
    }
}
