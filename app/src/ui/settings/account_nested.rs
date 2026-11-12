//! The Account page's two nested panels that are not lists of controls: Account Standing
//! (`ACCOUNT_STANDING_NESTED_PANEL`, module 158935 `bw`, body module 37380) and Family Center
//! (`ACCOUNT_FAMILY_CENTER_NESTED_PANEL`, `b$`, body module 890623) with its Activity, My Family and Settings tabs.
//! Sizes and colours are from `design/dom/settings/account-standing-nested-darker-cozy.json` and the
//! `family-center-*` dumps beside it; the standing comes from `GET /safety-hub/@me` (module 308645 `Yn`).

use super::{ButtonKind, Choice, button, color, mask_icon, render};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::{Root, avatar_source};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use serde_json::Value;

/// The key the standing answer is stored under in `Root::fetched`.
pub(in crate::ui) const SAFETY_HUB_KEY: &str = "safety-hub";

/// `Rsh.SAFETY_HUB` (module 652215).
pub(in crate::ui) fn standing_request() -> Request {
    Request::Fetch {
        key: SAFETY_HUB_KEY.to_owned(),
        route: "/safety-hub/@me".to_owned(),
    }
}

/// `PN` of module 739010: the five standings, in the order the track lists them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Standing {
    AllGood,
    Limited,
    VeryLimited,
    AtRisk,
    Suspended,
}

pub(in crate::ui) const STANDINGS: [Standing; 5] = [
    Standing::AllGood,
    Standing::Limited,
    Standing::VeryLimited,
    Standing::AtRisk,
    Standing::Suspended,
];

impl Standing {
    /// `ALL_GOOD` 100, `LIMITED` 200, `VERY_LIMITED` 300, `AT_RISK` 400, `SUSPENDED` 500.
    pub(in crate::ui) fn from_state(state: u64) -> Option<Self> {
        Some(match state {
            100 => Self::AllGood,
            200 => Self::Limited,
            300 => Self::VeryLimited,
            400 => Self::AtRisk,
            500 => Self::Suspended,
            _ => return None,
        })
    }

    /// The track's labels (`m` of module 881979: "All good", "Limited", ...).
    pub(in crate::ui) fn label(self) -> &'static str {
        match self {
            Self::AllGood => "All good",
            Self::Limited => "Limited",
            Self::VeryLimited => "Very limited",
            Self::AtRisk => "At risk",
            Self::Suspended => "Suspended",
        }
    }

    /// The coloured word of "Your account is ...".
    fn hook(self) -> &'static str {
        match self {
            Self::AllGood => "all good",
            Self::Limited => "limited",
            Self::VeryLimited => "very limited",
            Self::AtRisk => "at risk",
            Self::Suspended => "suspended",
        }
    }

    /// The description (`I` and `x` of module 881979); the all good one carries the two policy links.
    pub(in crate::ui) fn description(self) -> super::Runs {
        match self {
            Self::AllGood => &[
                ("Thank you for upholding Discord's ", false),
                ("Terms of Service", true),
                (" and ", false),
                ("Community Guidelines", true),
                (". If you have a violation, it will appear here.", false),
            ],
            Self::Limited => &[(
                "You may lose access to some parts of Discord if you break the rules again.",
                false,
            )],
            Self::VeryLimited => &[(
                "You can't use some parts of Discord. You may be suspended if you break the rules again.",
                false,
            )],
            Self::AtRisk => &[(
                "You broke Discord's rules. You will be permanently suspended if you break them again.",
                false,
            )],
            Self::Suspended => &[(
                "Due to serious policy violations, your account is permanently suspended. You can no longer use Discord.",
                false,
            )],
        }
    }

    /// The word's colour on the nested page and the active marker's (`I` of module 881979).
    fn track_color(self) -> &'static str {
        match self {
            Self::AllGood => "--status-positive",
            Self::Limited => "--status-warning",
            Self::VeryLimited => "--orange-345",
            Self::AtRisk => "--icon-feedback-critical",
            Self::Suspended => "--icon-muted",
        }
    }

    fn track_icon(self) -> &'static str {
        match self {
            Self::AllGood => "circle_check",
            Self::Limited | Self::VeryLimited | Self::AtRisk => "standing_circle_error",
            Self::Suspended => "standing_circle_x",
        }
    }
}

/// What the row's 48px tile shows (`_` and `A` of module 881979): icon, tint, background. Unknown while loading.
pub(in crate::ui) fn tile_look(
    standing: Option<Standing>,
) -> (&'static str, &'static str, &'static str) {
    match standing {
        Some(Standing::AllGood) => (
            "settings_standing",
            "--icon-feedback-positive",
            "--background-feedback-mod-positive",
        ),
        Some(Standing::Limited) => (
            "standing_warning",
            "--icon-feedback-warning",
            "--background-feedback-mod-warning",
        ),
        Some(Standing::VeryLimited) => ("standing_warning", "--orange-345", "--opacity-orange-12"),
        Some(Standing::AtRisk) => (
            "standing_circle_error",
            "--icon-feedback-critical",
            "--background-feedback-mod-critical",
        ),
        Some(Standing::Suspended) => (
            "standing_circle_x",
            "--icon-muted",
            "--background-mod-muted",
        ),
        None => (
            "standing_circle_question",
            "--icon-muted",
            "--background-mod-muted",
        ),
    }
}

/// The standing in a `GET /safety-hub/@me` answer: `account_standing.state`.
pub(in crate::ui) fn standing_of(body: &Value) -> Option<Standing> {
    Standing::from_state(body["account_standing"]["state"].as_u64()?)
}

/// The Family Center tabs, in the tab bar's order.
pub(in crate::ui) const TABS: [&str; 3] = ["Activity", "My Family", "Settings"];

/// The two pictures of the Family Center banners (`headerImage_IQWzzZ`, `bannerArt_L5c99o`).
pub(in crate::ui) const ACTIVITY_ART: (&str, &str) = (
    "family_activity_art",
    "https://discord.com/assets/bc9100f5e54f39a8.svg",
);
pub(in crate::ui) const FAMILY_ART: (&str, &str) = (
    "family_my_family_art",
    "https://cdn.discordapp.com/assets/content/bb4093f4ae937ad09bc359b162988c3cbb7d49d924073fee7d66a763e738fc67.svg",
);

/// The tab "Get Started" opens: My Family (`D.u9.REQUESTS`).
pub(in crate::ui) const GET_STARTED_TAB: usize = 1;
/// The banner's text before its link (`EMCf6j`).
const FAMILY_BLURB: &str = "We built Family Center to provide you with more context on how your teen uses Discord so you can work together on building positive online behaviors. ";
/// The `articleLink` of the parent's banner text.
pub(in crate::ui) const FAMILY_ARTICLE: &str =
    "https://support.discord.com/hc/articles/14155043715735";
/// `privacy.family_center_enabled_v2` (field 21 of `PrivacySettings`): "Enable Family Center tab".
pub(in crate::ui) const FAMILY_ENABLED: &str = "family_center_enabled";
/// The email category behind "Receive Activity Summary emails" (`FAMILY_CENTER_DIGEST`, module 723923).
pub(in crate::ui) const FAMILY_DIGEST_CATEGORY: &str = "family_center_digest";

/// One feature card of the Activity tab: the icon, the title and the text.
struct Card {
    icon: &'static str,
    title: &'static str,
    text: &'static str,
    /// The card's width in the 696px panel as dumped (`container_lb1Q8G` sizes itself to its text, the row shrinks the
    /// three to fit): its share of the row.
    width: f32,
}

const CARDS: [Card; 3] = [
    Card {
        icon: "fam_card_private",
        title: "Messages stay private",
        text: "You can see who your teen is talking to, while still respecting their privacy.",
        width: 216.844,
    },
    Card {
        icon: "fam_card_sharing",
        title: "Transparent sharing",
        text: "You and your teen see the exact same information, so you're on the same page.",
        width: 227.078,
    },
    Card {
        icon: "fam_card_connect",
        title: "Easily connect",
        text: "Setup is as simple as using your phone to scan the QR code your teen shows you.",
        width: 232.078,
    },
];

/// One line of a section of the Activity tab: icon, title and text.
struct Line {
    icon: &'static str,
    title: &'static str,
    text: &'static str,
}

struct Section {
    title: &'static str,
    note: Option<&'static str>,
    lines: &'static [Line],
    /// The "will not see" row's cross is drawn in the critical colour.
    critical: bool,
}

const SECTIONS: [Section; 3] = [
    Section {
        title: "What connected guardians can see",
        note: Some(
            "Our goal is to help you stay informed about your teen's Discord activity so you can have meaningful conversations and support them as they navigate digital spaces. We provide insights into how your teen uses Discord, and your teen sees the same information, to encourage open dialogue. We allow you to see:",
        ),
        lines: &[
            Line {
                icon: "fam_new_friends",
                title: "New friends",
                text: "New friends that your teen added or accepted",
            },
            Line {
                icon: "fam_servers",
                title: "Servers joined or participated in",
                text: "Includes servers your teen has recently created, joined, sent messages in, or participated in a voice and video call",
            },
            Line {
                icon: "fam_messaged",
                title: "Users messaged or called",
                text: "This includes direct messages and group chats",
            },
            Line {
                icon: "fam_voice_time",
                title: "Total time spent in voice or video",
                text: "Minutes spent in calls across direct messages group chats, and servers",
            },
            Line {
                icon: "fam_purchases",
                title: "Total purchases",
                text: "All purchases your teen made including Shop and subscriptions",
            },
            Line {
                icon: "fam_gifts",
                title: "Gifts received",
                text: "All gifts your teen received in the last week, including who sent them and how much they spent",
            },
            Line {
                icon: "fam_reports",
                title: "Reports shared by your teen",
                text: "If they choose to share a report they filed, you'll be notified via email",
            },
        ],
        critical: false,
    },
    Section {
        title: "What guardians can manage",
        note: Some(
            "Guardians can set up safeguards to give their teens the best experience on Discord while being safe.",
        ),
        lines: &[
            Line {
                icon: "fam_screen_time",
                title: "Screen Time Limits",
                text: "Give your teen a break by choosing the hours and days when Discord won't be accessible.",
            },
            Line {
                icon: "fam_spending_cap",
                title: "Spending cap",
                text: "Set a monthly budget for your teen's Discord purchases, including shop items, gifting, and more. Does not include subscriptions.",
            },
            Line {
                icon: "fam_teen_settings",
                title: "Teen Account Settings",
                text: "You can manage certain settings for your teen's account that they can't edit on their own.",
            },
        ],
        critical: false,
    },
    Section {
        title: "What guardians will not see",
        note: None,
        lines: &[Line {
            icon: "fam_not_see",
            title: "What your teen writes or says",
            text: "You will not see the contents of your teens' messages or calls.",
        }],
        critical: true,
    },
];

/// `body-sm/medium` in the muted colour, the text under a title everywhere on the Family Center page.
fn muted(text: &'static str, size: f32, line: f32) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-muted"))
        .child(text)
}

impl Root {
    /// The standing the account holds, once `GET /safety-hub/@me` answered.
    pub(in crate::ui) fn standing(&self) -> Option<Standing> {
        standing_of(self.fetched.get(SAFETY_HUB_KEY)?)
    }

    /// The pictures of the Family Center page are asked for once, when the page opens.
    pub(in crate::ui) fn family_art_requested(&mut self) {
        for (key, url) in [ACTIVITY_ART, FAMILY_ART] {
            self.request_image(key.to_owned(), url.to_owned());
        }
    }

    /// A press on the Account Standing or Family Center row: the panel opens at its top, the subnav lights the
    /// section it belongs to, and Back returns the Account page to where it was.
    pub(in crate::ui) fn account_panel_open(
        &mut self,
        panel: super::account_panels::Panel,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.settings.as_ref() {
            self.settings_extra.nested_return = Some(open.content.offset());
            open.content.set_offset(gpui::point(px(0.), px(0.)));
            open.anchor.set(match panel {
                super::account_panels::Panel::Standing => 2,
                super::account_panels::Panel::Family => 3,
                super::account_panels::Panel::Sessions | super::account_panels::Panel::Mfa => 1,
            });
            // The jump to the top is not the reader scrolling.
            open.spy.set((0., true));
        }
        self.settings_extra.panel = Some(panel);
        if panel == super::account_panels::Panel::Family {
            self.settings_extra.family_tab = 0;
            self.family_art_requested();
        }
        cx.notify();
    }

    /// The back button or the "Account" crumb.
    pub(in crate::ui) fn account_panel_back(&mut self, cx: &mut Context<Self>) {
        self.settings_extra.panel = None;
        if let (Some(open), Some(offset)) = (
            self.settings.as_ref(),
            self.settings_extra.nested_return.take(),
        ) {
            open.content.set_offset(offset);
            open.spy.set((f32::from(offset.y), true));
        }
        cx.notify();
    }

    /// The header of a nested panel (`contentHeaderLeft_3Rvb29`): a 32px back button and the breadcrumb
    /// "Account / title", 8px apart.
    pub(in crate::ui) fn nested_header(
        &self,
        title: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let go_back = cx.listener(|this, _, _, cx| {
            this.account_panel_back(cx);
        });
        let up = cx.listener(|this, _, _, cx| {
            this.account_panel_back(cx);
        });
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id("settings-nested-back")
                    .size(px(32.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                    .on_click(go_back)
                    .child(mask_icon(
                        "role_back",
                        16.,
                        "--control-icon-only-icon-default",
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .child(
                        div()
                            .id("settings-breadcrumb-account")
                            .cursor_pointer()
                            .text_color(color("--text-subtle"))
                            .on_click(up)
                            .child(dc_intl::tr("Account")),
                    )
                    .child(div().text_color(color("--text-subtle")).child("/"))
                    .child(
                        div()
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(title),
                    ),
            )
            .into_any_element()
    }

    /// The Account Standing panel: the avatar in its ring, "Your account is ...", the policy text and the track of
    /// the five standings with the account's lit (module 37380 `L`).
    pub(in crate::ui) fn standing_body(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let standing = self.standing();
        let shown = standing.unwrap_or(Standing::AllGood);
        let title = div()
            .text_size(px(20.))
            .line_height(px(24.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(
                div().flex().child("Your account is ").child(
                    div()
                        .font_weight(FontWeight(600.))
                        .text_color(color(shown.track_color()))
                        .child(shown.hook()),
                ),
            );
        let status =
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(24.))
                .child(div().flex().flex_col().gap(px(8.)).child(title).child(
                    super::standing_text(
                        "standing-panel-text",
                        shown.description(),
                        "--text-default",
                        cx.weak_entity(),
                    ),
                ))
                .child(self.standing_track(shown));
        let card = div()
            .parity("settings-standing-card")
            .flex()
            .gap(px(16.))
            .pt(px(8.))
            .mb(px(32.))
            .child(
                div()
                    .flex_none()
                    .ml(px(16.))
                    .w(px(92.))
                    .h(px(92.))
                    .rounded_full()
                    .bg(color("--background-mod-strong"))
                    .p(px(6.))
                    .child(
                        div()
                            .size(px(80.))
                            .rounded_full()
                            .overflow_hidden()
                            .children(avatar.map(|image| img(image).size(px(80.)).rounded_full())),
                    ),
            )
            .child(status);
        vec![
            // The category's 1px marker, then the setting wrapper's 8px.
            div().h(px(1.)).into_any_element(),
            div()
                .py(px(8.))
                .flex()
                .flex_col()
                .child(card)
                .into_any_element(),
        ]
    }

    /// The five-step track: a 4px line behind 28px markers, the first label at the left edge, the last at the right
    /// and the others centred on the quarter marks.
    fn standing_track(&self, shown: Standing) -> AnyElement {
        const OPTION: f32 = 86.;
        let width = render::PANEL - 124.;
        let mut track = div().relative().w_full().h(px(54.));
        track = track.child(
            div()
                .absolute()
                .top(px(12.))
                .left_0()
                .w_full()
                .h(px(4.))
                .bg(color("--background-mod-strong")),
        );
        for (index, standing) in STANDINGS.iter().copied().enumerate() {
            let active = standing == shown;
            let left = match index {
                0 => 0.,
                4 => width - OPTION,
                _ => width * index as f32 / 4. - OPTION / 2. + 0.5,
            };
            let marker = if active {
                mask_icon(standing.track_icon(), 28., standing.track_color())
                    .rounded_full()
                    .bg(color("--background-base-low"))
                    .into_any_element()
            } else {
                div()
                    .flex_none()
                    .size(px(28.))
                    .rounded_full()
                    .bg(color("--background-base-low"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(index == 0, |this| this.ml(px(-6.)))
                    .when(index == 4, |this| this.mr(px(-3.)))
                    .child(
                        div()
                            .size(px(16.))
                            .rounded_full()
                            .bg(color("--background-mod-strong")),
                    )
                    .into_any_element()
            };
            let option = div()
                .absolute()
                .top_0()
                .left(px(left))
                .w(px(OPTION))
                .flex()
                .flex_col()
                .gap(px(8.))
                .map(|this| match index {
                    0 => this.items_start(),
                    4 => this.items_end(),
                    _ => this.items_center(),
                })
                .child(marker)
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(standing.label()),
                );
            track = track.child(option);
        }
        track.into_any_element()
    }

    /// The Family Center panel: the tab bar, then the tab (module 890623).
    pub(in crate::ui) fn family_body(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tab = self.settings_extra.family_tab.min(TABS.len() - 1);
        let bar = div()
            .parity("settings-family-tabs")
            .flex()
            .h(px(30.))
            .mb(px(24.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .children(TABS.iter().enumerate().map(|(index, label)| {
                let selected = index == tab;
                div()
                    .id(SharedString::from(format!("settings-family-tab-{index}")))
                    .relative()
                    .h(px(30.))
                    .mb(px(-1.))
                    .when(index > 0, |this| this.ml(px(24.)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_extra.family_tab = index;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color(if selected {
                                "--text-brand"
                            } else {
                                "--text-subtle"
                            }))
                            .child(dc_intl::tr(label)),
                    )
                    .when(selected, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(2.))
                                .bg(color("--text-brand")),
                        )
                    })
            }));
        let panel = match tab {
            0 => self.family_activity(cx),
            1 => self.family_my_family(),
            _ => self.family_settings(cx),
        };
        vec![
            div().h(px(1.)).into_any_element(),
            div()
                .py(px(8.))
                .flex()
                .flex_col()
                .child(bar)
                .child(panel)
                .into_any_element(),
        ]
    }

    /// `box_qCRz2b`: a 1px bordered card, 12px round, on the raised surface.
    fn family_box() -> gpui::Div {
        div()
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
    }

    fn family_activity(&self, cx: &mut Context<Self>) -> AnyElement {
        let art = self.images.get(ACTIVITY_ART.0).cloned();
        let banner = Self::family_box()
            .p(px(32.))
            .flex()
            .justify_between()
            .items_start()
            .child(
                div()
                    .flex_none()
                    .w(px(364.))
                    .child(
                        div()
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr(
                                "Stay informed about how your teen is using Discord.",
                            )),
                    )
                    .child(
                        div()
                            .mt(px(6.))
                            .max_w(px(364.))
                            .font_weight(FontWeight(500.))
                            .child(Self::family_learn_more(cx)),
                    )
                    // `t1` of module 890623: an adult's "Get Started" (`LhlgY9`) opens the My Family tab
                    // (`handleTabChange(REQUESTS)`).
                    .child(
                        div().mt(px(16.)).flex().child(
                            div()
                                .id("settings-family-get-started")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings_extra.family_tab = GET_STARTED_TAB;
                                    cx.notify();
                                }))
                                .child(button(
                                    "settings-family-get-started-button",
                                    dc_intl::tr("Get Started"),
                                    ButtonKind::Primary,
                                )),
                        ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .self_center()
                    .pl(px(16.))
                    .w(px(204.))
                    .children(art.map(|image| img(image).w(px(188.)).h(px(136.)))),
            );
        let cards = div()
            .mt(px(44.))
            .flex()
            .gap(px(10.))
            .children(CARDS.iter().map(|card| {
                Self::family_box()
                    // Content-sized in the reference; here the three share the row in that ratio so they follow
                    // the panel's width (they come out at the dumped widths at 696px).
                    .flex_grow(1.)
                    .flex_shrink(1.)
                    .flex_basis(px(card.width))
                    .min_w(px(0.))
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .size(px(36.))
                            .mb(px(10.))
                            .rounded_full()
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(mask_icon(card.icon, 20., "--text-strong")),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr(card.title)),
                    )
                    .child(
                        div()
                            .mt(px(6.))
                            .child(muted(dc_intl::tr(card.text), 14., 18.)),
                    )
            }));
        let sections = div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .children(SECTIONS.iter().map(|section| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .px(px(4.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::tr(section.title)),
                            )
                            .children(section.note.map(|note| muted(dc_intl::tr(note), 12., 16.))),
                    )
                    .child(
                        div()
                            .rounded(px(12.))
                            .overflow_hidden()
                            .bg(color("--background-surface-higher"))
                            .flex()
                            .flex_col()
                            .children(section.lines.iter().enumerate().map(|(index, line)| {
                                let last = index + 1 == section.lines.len();
                                div()
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .gap(px(16.))
                                    .p(px(12.))
                                    .child(div().flex_none().ml(px(12.)).child(mask_icon(
                                        line.icon,
                                        20.,
                                        if section.critical {
                                            "--icon-feedback-critical"
                                        } else {
                                            "--text-default"
                                        },
                                    )))
                                    .child(
                                        div()
                                            .max_w(px(500.))
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(18.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::tr(line.title)),
                                            )
                                            .child(div().mt(px(4.)).child(muted(
                                                dc_intl::tr(line.text),
                                                12.,
                                                16.,
                                            ))),
                                    )
                                    .when(!last, |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .left_0()
                                                .right_0()
                                                .bottom_0()
                                                .h(px(1.))
                                                .bg(color("--border-subtle")),
                                        )
                                    })
                            })),
                    )
            }));
        let _ = cx;
        div()
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(div().flex().flex_col().child(banner).child(cards))
            .child(sections)
            .into_any_element()
    }

    /// "Learn more" ends the banner's text (`anchorUnderlineOnHover_R6ReNl`) and opens the parent article
    /// (`EMCf6j` with its `articleLink`, `t3` of module 890623).
    fn family_learn_more(cx: &mut Context<Self>) -> impl IntoElement {
        let start = FAMILY_BLURB.len();
        let text = format!("{FAMILY_BLURB}Learn more");
        let link = start..text.len();
        let root = cx.weak_entity();
        div()
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-muted"))
            .child(
                gpui::InteractiveText::new(
                    "settings-family-learn-more",
                    gpui::StyledText::new(text).with_highlights([(
                        link.clone(),
                        gpui::HighlightStyle {
                            color: Some(color("--text-link").into()),
                            ..Default::default()
                        },
                    )]),
                )
                .on_click(vec![link], move |_, _, cx| {
                    let _ = root.update(cx, |this, cx| {
                        this.settings_open_url(FAMILY_ARTICLE, cx);
                        cx.notify();
                    });
                }),
            )
    }

    fn family_my_family(&self) -> AnyElement {
        let art = self.images.get(FAMILY_ART.0).cloned();
        let step = |number: &'static str, title: &'static str, text: &'static str| {
            div()
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .size(px(32.))
                        .mt(px(2.))
                        .rounded_full()
                        .border_1()
                        .border_color(color("--border-muted"))
                        .bg(color("--background-mod-subtle"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-brand"))
                        .child(number),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(muted(text, 14., 18.)),
                )
        };
        let banner = Self::family_box()
            .p(px(32.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .child(
                        div()
                            .flex_none()
                            .w(px(406.))
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .line_height(px(32.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::tr("Download the Discord app to connect with your teens!")),
                            )
                            .child(div().mt(px(8.)).mb(px(20.)).child(muted(
                                dc_intl::tr("In order to connect with your teen, you'll need to download the Discord app on your phone. You can then complete setup by going to Family Center in user settings. After connecting, you will see high-level information about your teen's activity on Discord."),
                                14.,
                                18.,
                            ))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(224.))
                            .children(art.map(|image| img(image).w(px(224.)).h(px(149.)))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(step(
                        "1",
                        dc_intl::tr("Ask your teen for their QR code"),
                        dc_intl::tr("Your teen can find this by going to Family Center in user settings and clicking My Family."),
                    ))
                    .child(step(
                        "2",
                        dc_intl::tr("Scan the QR code with your Discord app"),
                        dc_intl::tr("This will send a connection request to your teen, like magic!"),
                    ))
                    .child(step(
                        "3",
                        dc_intl::tr("Have your teen accept the connection request"),
                        dc_intl::tr("Once they give consent for you to view their activity data, you\u{2019}re good to go."),
                    )),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(banner)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .mb(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child("CONNECTED TEENS \u{2014} 0 OF 8"),
                    )
                    .child(div().p(px(32.)).flex().justify_center().child(muted(
                        dc_intl::tr("No family members connected yet."),
                        14.,
                        18.,
                    ))),
            )
            .into_any_element()
    }

    /// The Settings tab (module 890623 `lg`): a fieldset "General Settings" with the two switches, 24px apart.
    fn family_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.pref_on(FAMILY_ENABLED, false);
        let digest = self
            .settings_extra
            .notif
            .categories
            .as_ref()
            .and_then(|categories| categories.get(FAMILY_DIGEST_CATEGORY).copied())
            .unwrap_or(true);
        let row = |id: &'static str,
                   title: &'static str,
                   text: &'static str,
                   on: bool,
                   changed: fn(&mut Root, bool),
                   cx: &mut Context<Self>| {
            div()
                .flex()
                .justify_between()
                .gap(px(24.))
                .child(
                    div()
                        .w(px(598.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(
                            div()
                                .max_w(px(572.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child(text),
                        ),
                )
                .child(render::plain_switch(
                    SharedString::from(format!("switch-{id}")),
                    on,
                    changed,
                    cx,
                ))
        };
        div()
            .pt(px(20.))
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(12.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("General Settings")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(row(
                        FAMILY_ENABLED,
                        dc_intl::tr("Enable Family Center tab"),
                        dc_intl::tr("This will show you the Family Center tab. Here you can pair with your family members to create a safe, yet privacy conscious environment for your family!"),
                        enabled,
                        |root, on| {
                            root.setting_choices.insert(FAMILY_ENABLED, Choice::On(on));
                            root.pref_changed(FAMILY_ENABLED, on, 0);
                        },
                        cx,
                    ))
                    .child(row(
                        "family_center_email_digest",
                        dc_intl::tr("Receive Activity Summary emails"),
                        dc_intl::tr("You will receive a weekly email summarizing your teen(s)' recent activity. We recommend keeping this setting on."),
                        digest,
                        |root, on| {
                            root.settings_extra
                                .notif
                                .categories
                                .get_or_insert_with(Default::default)
                                .insert(FAMILY_DIGEST_CATEGORY.to_owned(), on);
                            let _ = root
                                .requests
                                .send(super::notifications_page::email_patch(&[(
                                    FAMILY_DIGEST_CATEGORY,
                                    on,
                                )]));
                        },
                        cx,
                    )),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_started_opens_my_family_and_learn_more_the_parent_article() {
        assert_eq!(TABS[GET_STARTED_TAB], "My Family");
        assert!(FAMILY_ARTICLE.ends_with("/articles/14155043715735"));
        assert!(FAMILY_BLURB.ends_with("behaviors. "));
    }

    #[test]
    fn the_standing_states_map_to_the_track() {
        assert_eq!(Standing::from_state(100), Some(Standing::AllGood));
        assert_eq!(Standing::from_state(500), Some(Standing::Suspended));
        assert_eq!(Standing::from_state(0), None);
        assert_eq!(Standing::AtRisk.label(), "At risk");
    }

    #[test]
    fn the_standing_is_read_from_the_safety_hub_answer() {
        let body = json!({"account_standing": {"state": 200}});
        assert_eq!(standing_of(&body), Some(Standing::Limited));
        assert_eq!(standing_of(&json!({})), None);
    }
}
