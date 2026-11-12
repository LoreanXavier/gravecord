//! The Nitro home of an account whose premium type is exactly Nitro (module 886268 `sQ`, `PREMIUM_SUBSCRIBER_NITRO_HOME`;
//! `L.YE` is `premiumType === TIER_2`). Basic and Classic keep the marketing home.
//!
//! Built from the module with no capture of a subscriber's home: the layout comes from the classes the module names
//! (`container_aWZQh8`, `section_KD8RnX`, `card_ualTdV`, `section_cpKOhU`, `container_gLR1C5`), the text from the
//! strings, the data from the stores the module reads. Pictures that are not in the snapshot are flat blocks of their
//! size. Card pictures, the gift banner, the bottom illustration and the page gradient load at run time from the
//! addresses the module names. Not built: the tenure-badge, Orbs and Quest-multiplier cards (program data), the
//! file-upload card (a rollout experiment), the subscriber-badge picture, the churn banner of a canceled subscription,
//! and the light-theme gradient. Entrance Sounds' button is drawn disabled (its settings category has no page here).
//! Everything here is unconfirmed against a real subscriber's screen.

use super::{Root, art_key};
use crate::ui::{avatar_source, color, icon};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, linear_color_stop,
    linear_gradient, prelude::*, px,
};

const GIFT_BANNER_KEY: &str = "nitro-home:gift-banner";
/// The default-variant banner picture of module 175020.
const GIFT_BANNER_URL: &str = "https://cdn.discordapp.com/assets/content/577cd1f06ad2e0559c8a531a20a840448c66d6a6251f5c0bac311ceba4d37396.png";
const BOTTOM_KEY: &str = "nitro-home:bottom";
/// `sq`: the bottom illustration, 112 x 85 (a bundled asset).
const BOTTOM_URL: &str = "https://discord.com/assets/cd2be35d285d4675.svg";
const BACKGROUND_KEY: &str = "nitro-home:background";
/// `sz`: the page's top gradient for a dark theme (`/assets/2ee0f277372e56e4.svg` for a light one).
const BACKGROUND_URL: &str = "https://discord.com/assets/3ebfa123a3805f56.svg";

/// `Wvc/I+`, `d+0STx`, `CqsxKI` by the local hour: morning is 5 to 12, afternoon 12 to 17, else evening (`tm`).
pub(super) fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=11 => "Good Morning,",
        12..=16 => "Good Afternoon,",
        _ => "Good Evening,",
    }
}

/// What a card's "Take me there" button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cta {
    /// The card has no button.
    None,
    /// `openUserSettings(...)`: the settings page with this slug.
    Settings(&'static str),
    /// `goToShop`.
    Shop,
    /// The module opens a settings category this client has no page for: the button is drawn and disabled.
    Unavailable,
}

/// A perk card (`tX.S`): the title, its description and the filter tabs it belongs to.
struct Perk {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    /// The picture the module names for the card: a CDN address or a bundled asset under `discord.com/assets`. The
    /// subscriber badge has none (the tenure badge's own image, or an icon).
    art: Option<&'static str>,
    /// Where "Take me there" goes (`onCtaClick`), or nothing when the module gives the card no button.
    cta: Cta,
    /// `bestof`, `appearance`, `upgrades` or `vip` (`categories`).
    categories: &'static [&'static str],
}

/// The cards of "Explore What's New" that need no program data (`sL`): in the module's order.
static WHATS_NEW: [Perk; 4] = [
    Perk {
        id: "display-name-styles",
        art: Some(
            "https://cdn.discordapp.com/assets/content/cd580e29aa6ad4aa731dba64c23331d6bc556ad2e236ec1b5781206f6e71cb50.svg",
        ),
        cta: Cta::Settings("account"),
        title: "Display Name Styles",
        description: "Mix and match styles to create an unforgettable display name.",
        categories: &["appearance"],
    },
    Perk {
        id: "client-themes",
        art: Some(
            "https://cdn.discordapp.com/assets/content/c0c412ad58e2520901e2cb967308eb432d0b349b3b5f54b14f141e12b80f7d42.svg",
        ),
        cta: Cta::Settings("appearance"),
        title: "Color Themes",
        description: "Create your perfect theme, or choose from 20+ themes like Cotton Candy, Midnight Blurple, and more.",
        categories: &["bestof", "appearance"],
    },
    Perk {
        id: "permadecos",
        art: Some(
            "https://cdn.discordapp.com/assets/content/ef081d5f86c6c1d63f7f026517c0226c59bba2618b3d65a7ce91dc10019b35ad.svg",
        ),
        cta: Cta::Settings("account"),
        title: "Lasting Quest rewards",
        description: "Hold onto many of the Avatar Decorations you\u{2019}ve earned from Quests longer.",
        categories: &["vip"],
    },
    Perk {
        id: "custom-app-icons",
        art: Some(
            "https://cdn.discordapp.com/assets/content/7bb191dd09571f8910a367c7cf35b500ea7b64dde29309c7c74831cc16b1dc1d.png",
        ),
        cta: Cta::Settings("appearance"),
        title: "Custom app icons",
        description: "Choose a mobile and in-app desktop icon that fits your vibe.",
        categories: &["appearance"],
    },
];

/// The cards of "Check Out These Nitro Favorites" (`sV`), without the ones What's New already shows.
static FAVORITES: [Perk; 13] = [
    Perk {
        id: "server-boosts",
        art: Some(
            "https://cdn.discordapp.com/assets/content/a9ce5551f37c49862694d063777be6e02b9190ff9a205c7c62f5fe9cecb673de.svg",
        ),
        cta: Cta::Settings("server-boost"),
        title: "2 Server Boosts + 30% off",
        description: "Support the servers you love with 2 included Boosts and 30% off when you buy more.",
        categories: &["bestof", "upgrades"],
    },
    Perk {
        id: "profiles",
        art: Some(
            "https://cdn.discordapp.com/assets/content/a89ff8a07704142beff857453a6c8aa15400cf711f8fe3cbbfa7ff4f0b2a334f.svg",
        ),
        cta: Cta::Settings("account"),
        title: "Customize your profile",
        description: "Explore all the ways to personalize your profile including animated avatars, color themes, and per-server profiles.",
        categories: &["bestof", "appearance"],
    },
    Perk {
        id: "hd-video",
        art: Some(
            "https://cdn.discordapp.com/assets/content/2af3a2846a762c266ce197f82418f1f084b3b7aa12a3f38d4865682a2f715072.svg",
        ),
        cta: Cta::None,
        title: "HD streaming",
        description: "Share every clutch moment in up to 4K at 60fps.",
        categories: &["bestof", "upgrades"],
    },
    Perk {
        id: "more-emojis",
        art: Some(
            "https://cdn.discordapp.com/assets/content/96dd57d4bf82369b228168d944da9a2d72bdf1f73640cd50802379ffcf925354.svg",
        ),
        cta: Cta::None,
        title: "Custom emojis anywhere",
        description: "Hype, roast, and meme with custom emojis anywhere.",
        categories: &["bestof", "upgrades"],
    },
    Perk {
        id: "large-uploads",
        art: Some(
            "https://cdn.discordapp.com/assets/content/804d87fc1c1330e6413159eaacc359de89bdfbec7b4602532b1bd074868783c8.svg",
        ),
        cta: Cta::None,
        title: "Larger file uploads",
        description: "Send files up to 1GB. Perfect for game clips, art, and more.",
        categories: &["bestof", "upgrades"],
    },
    Perk {
        id: "entrance-sounds",
        art: Some(
            "https://cdn.discordapp.com/assets/content/61471321446262d980f72210a31bbce561d7021e51f4ea2988d63e413df9fe04.svg",
        ),
        cta: Cta::Unavailable,
        title: "Entrance Sounds",
        description: "Make an entrance with a unique soundboard sound when you join voice channels.",
        categories: &["appearance"],
    },
    Perk {
        id: "custom-sounds",
        art: Some(
            "https://cdn.discordapp.com/assets/content/2d617cf5c40353ca4ec6ac76e1a48f6781439c18db06b8cfab0f194358abe555.svg",
        ),
        cta: Cta::None,
        title: "Custom sounds everywhere",
        description: "Bring on the chaos with custom soundboard sounds across voice channels.",
        categories: &["upgrades"],
    },
    Perk {
        id: "special-stickers",
        art: Some(
            "https://cdn.discordapp.com/assets/content/c561ce5c9866c0bdb3f8c9dc43164f873414c3d905d1ab4beace8020dc4933fc.svg",
        ),
        cta: Cta::None,
        title: "Custom stickers anywhere",
        description: "Drop your favorite stickers wherever you're chatting.",
        categories: &["upgrades"],
    },
    Perk {
        id: "super-reactions",
        art: Some(
            "https://cdn.discordapp.com/assets/content/8a9a80f2b924da025612a160c7dc74c5e0fe675a90e3424737e6c5927739a6d0.svg",
        ),
        cta: Cta::None,
        title: "Super Reactions",
        description: "Bring extra hype to the chat with action-packed, animated reactions.",
        categories: &["upgrades"],
    },
    Perk {
        id: "video-backgrounds",
        art: Some("https://discord.com/assets/1eb1b74667b4c0f0.svg"),
        cta: Cta::None,
        title: "Video backgrounds",
        description: "Add some personality to video calls with custom backgrounds.",
        categories: &["upgrades"],
    },
    Perk {
        id: "early-access",
        art: Some(
            "https://cdn.discordapp.com/assets/content/3ba8fce33a4733cdbadc10ac378d1b5523b0c0961e7edc74372c159e3009727f.svg",
        ),
        cta: Cta::None,
        title: "Early access",
        description: "Get your hands on certain new features before they\u{2019}re released to everyone else.",
        categories: &["vip"],
    },
    Perk {
        id: "badge",
        art: None,
        cta: Cta::None,
        title: "Subscriber badge",
        description: "A special badge, just for Nitro members. The longer you subscribe, the cooler it gets.",
        categories: &["vip"],
    },
    Perk {
        id: "special-pricing",
        art: Some(
            "https://cdn.discordapp.com/assets/content/1314d17fb23c01337cac4bfdaaff48c5e6ac1f515b0647f0964922b988f095c1.svg",
        ),
        cta: Cta::Shop,
        title: "Special shop pricing",
        description: "Enjoy sweet discounts on any\u{2014}or every!\u{2014}item at the Shop.",
        categories: &["vip"],
    },
];

/// The filter tabs (`sB`): id and label.
const TABS: [(&str, &str); 4] = [
    ("bestof", "Best of Nitro"),
    ("appearance", "Appearance & Style"),
    ("upgrades", "Discord Upgrades"),
    ("vip", "VIP Extras"),
];

/// The favorites under a tab, in the module's order.
fn favorites_in(tab: &str) -> Vec<&'static Perk> {
    FAVORITES
        .iter()
        .filter(|perk| perk.categories.contains(&tab))
        .collect()
}

impl Root {
    /// `card_ualTdV`: 380px tall, 16px radius, 1px inset border, 20px padding, the art above the text. Hovered (or
    /// focused) the description opens to its full height (`max-height: 5lh`) and the button slides in under it
    /// (`ctaPlaceholder` 36px, 16px above).
    fn home_perk_card(
        &self,
        perk: &'static Perk,
        width: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hovered = self.nitro.hovered_perk == Some(perk.id);
        let card = div()
            .id(perk.id)
            .flex_none()
            .h(px(380.))
            .rounded(px(16.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .p(px(20.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                let now = over.then_some(perk.id);
                if this.nitro.hovered_perk != now
                    && (*over || this.nitro.hovered_perk == Some(perk.id))
                {
                    this.nitro.hovered_perk = now;
                    cx.notify();
                }
            }));
        let card = match width {
            Some(width) => card.w(px(width)),
            None => card.min_w(px(0.)),
        };
        let art = perk
            .art
            .and_then(|_| self.images.get(&art_key(perk.id)).cloned());
        let art_area = div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .items_center()
            .justify_center()
            .child(match art {
                // `primaryAssetImage_ualTdV`: contained, at most 180px tall and the card's width.
                Some(image) => img(image)
                    .max_h(px(180.))
                    .max_w_full()
                    .object_fit(gpui::ObjectFit::Contain)
                    .into_any_element(),
                // No address named, or not arrived yet: a flat block of the art's size.
                None => div()
                    .w(px(200.))
                    .h(px(160.))
                    .rounded(px(16.))
                    .bg(color("--background-surface-highest"))
                    .into_any_element(),
            });
        let button = (hovered && perk.cta != Cta::None).then(|| {
            let target = perk.cta;
            let enabled = target != Cta::Unavailable;
            div()
                .id(SharedString::from(format!("nitro-perk-cta-{}", perk.id)))
                .mt(px(16.))
                .h(px(36.))
                .w_full()
                .rounded(px(8.))
                .bg(color("--control-secondary-background-default"))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--control-secondary-text-default"))
                .when(!enabled, |this| this.opacity(0.5).cursor_not_allowed())
                .when(enabled, |this| this.cursor_pointer())
                .child(if perk.id == "special-pricing" {
                    "Visit the Shop"
                } else {
                    "Take me there"
                })
                .on_click(cx.listener(move |this, _, _, cx| match target {
                    Cta::Settings(slug) => this.open_settings_page(slug, cx),
                    Cta::Shop => this.open_shop(cx),
                    Cta::None | Cta::Unavailable => {}
                }))
        });
        card.child(art_area)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(perk.title),
                    )
                    .child(
                        if hovered {
                            div().mt(px(4.)).max_h(px(90.)).overflow_hidden()
                        } else {
                            div()
                                .mt(px(4.))
                                .h(px(18.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                        }
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(perk.description),
                    )
                    .children(button),
            )
            .into_any_element()
    }

    /// The greeting hero (`tm`): the time-of-day line, then the avatar and display name (`eL`). No buttons.
    fn nitro_greeting(&self) -> AnyElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let hour = chrono::Timelike::hour(&chrono::Local::now());
        div()
            .w_full()
            .pt(px(80.))
            .pb(px(64.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div().w_full().max_w(px(1200.)).px(px(32.)).child(
                    div()
                        .max_w(px(636.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            Self::nitro_heading(greeting(hour), 32., 34., true)
                                .text_color(color("--text-default")),
                        )
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .gap(px(16.))
                                // `avatar_uKEKOI`: 56px avatar in a 1.2x box.
                                .child(
                                    div()
                                        .flex_none()
                                        .size(px(67.2))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .size(px(56.))
                                                .rounded_full()
                                                .overflow_hidden()
                                                .bg(color("--background-surface-highest"))
                                                .children(avatar.map(|image| {
                                                    img(image).size(px(56.)).rounded_full()
                                                })),
                                        ),
                                )
                                // `displayName_uKEKOI`: 44px, one line.
                                .child(
                                    div()
                                        .max_w(px(380.))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(px(44.))
                                        .line_height(px(48.))
                                        .text_color(color("--text-default"))
                                        .child(user.display_name().to_owned()),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }

    /// `sK`: "Explore What's New", three cards to a row with 24px gaps.
    fn nitro_whats_new(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mb(px(88.))
            .w_full()
            .max_w(px(1200.))
            .px(px(32.))
            .child(
                Self::nitro_heading("Explore What's New", 24., 26., true)
                    .text_color(color("--text-strong"))
                    .mb(px(24.)),
            )
            .child(
                div().grid().grid_cols(3).gap(px(24.)).children(
                    WHATS_NEW
                        .iter()
                        .map(|perk| self.home_perk_card(perk, None, cx)),
                ),
            )
            .into_any_element()
    }

    /// `sV` lists the Orbs and Quest-multiplier cards under VIP Extras when their programs run; not built.
    /// The favorites, behind four filter pills, in a row that scrolls sideways.
    fn nitro_favorites(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.nitro.favorites_tab.min(TABS.len() - 1);
        let tabs = div()
            .mt(px(32.))
            .mb(px(20.))
            .ml(px(-8.))
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .children(TABS.iter().enumerate().map(|(index, (id, label))| {
                div()
                    .id(*id)
                    .h(px(32.))
                    .px(px(12.))
                    .rounded_full()
                    .bg(color("--control-secondary-background-default"))
                    .border_1()
                    .border_color(if index == selected {
                        color("--brand-500")
                    } else {
                        gpui::transparent_black().into()
                    })
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(*label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.nitro.favorites_tab = index;
                        cx.notify();
                    }))
            }));
        div()
            .mb(px(66.))
            .w_full()
            .max_w(px(1200.))
            .px(px(32.))
            .child(Self::nitro_heading(
                "Check Out These Nitro Favorites",
                24.,
                26.,
                true,
            ))
            .child(tabs)
            .child(
                div()
                    .id("nitro-favorites-row")
                    .mx(px(-8.))
                    .px(px(8.))
                    .flex()
                    .gap(px(20.))
                    .overflow_x_scroll()
                    .children(
                        favorites_in(TABS[selected].0)
                            .into_iter()
                            .map(|perk| self.home_perk_card(perk, Some(280.), cx)),
                    ),
            )
            .into_any_element()
    }

    /// `Y.A` (module 175020), default variant: a bordered card with the banner picture, "Gift Nitro", a line of copy and
    /// the Gift Nitro button.
    fn nitro_gift_banner(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .max_w(px(1200.))
            .px(px(32.))
            .child(
                div()
                    .w_full()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(color("--card-background-default"))
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .items_center()
                    // `bannerImage_gLR1C5`: 300px slot, the picture contained at 200px.
                    .child(
                        div()
                            .h(px(300.))
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(self.images.get(GIFT_BANNER_KEY).cloned().map(|image| {
                                img(image).max_h(px(200.)).object_fit(gpui::ObjectFit::Contain)
                            })),
                    )
                    .child(
                        div()
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .text_center()
                            .child(
                                div()
                                    .mb(px(12.))
                                    .text_size(px(40.))
                                    .line_height(px(44.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child("Gift Nitro"),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child("Gift someone Nitro to give them access to profile customization, emoji, and superpowered chat features."),
                            )
                            .child(
                                div()
                                    .mt(px(24.))
                                    .child(self.gift_button("nitro-home-gift", cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Starts the downloads of the pictures the module names.
    fn request_subscriber_art(&mut self) {
        for perk in WHATS_NEW.iter().chain(FAVORITES.iter()) {
            if let Some(url) = perk.art {
                self.request_image(art_key(perk.id), url.to_owned());
            }
        }
        self.request_image(GIFT_BANNER_KEY.to_owned(), GIFT_BANNER_URL.to_owned());
        self.request_image(BOTTOM_KEY.to_owned(), BOTTOM_URL.to_owned());
        self.request_image(BACKGROUND_KEY.to_owned(), BACKGROUND_URL.to_owned());
    }

    /// `tP`: the transparent navigation bar of `sX` (`navBar_jHfUxk`: a blurred bar with a 1px strong bottom border,
    /// 16px side padding). The Nitro wheel and the wordmark on the left (`brand_GB6rv4`, 8px apart), the wishlist
    /// button and Gift Nitro on the right (8px apart).
    fn nitro_subscriber_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right(px(16.))
            .h(px(49.))
            .px(px(16.))
            .border_b_1()
            .border_color(color("--border-strong"))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("nitro_wheel", 24., "--icon-strong"))
                    .child(Self::nitro_heading("Nitro", 14., 16., true)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("nitro-home-wishlist")
                            .size(px(32.))
                            .rounded(px(8.))
                            .bg(gpui::rgba(0x0000008f))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("golive_heart", 16., "--text-strong")),
                    )
                    .child(
                        div()
                            .id("nitro-home-bar-gift")
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(gpui::rgba(0x0000008f))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(icon("gift", 16., "--white"))
                            .child("Gift Nitro")
                            .on_click(cx.listener(|this, _, _, cx| this.open_purchase(true, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The whole page (`sQ`): the bar and background of `sX`, hero, What's New, favorites, gift banner, the spacer
    /// and the bottom picture.
    pub(super) fn nitro_subscriber_home(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.request_subscriber_art();
        let scroll = self.nitro.scroll.clone();
        let background = self.images.get(BACKGROUND_KEY).cloned();
        let bottom = self.images.get(BOTTOM_KEY).cloned();
        let page_color = color("--background-base-lower");
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(page_color)
            .child(
                div()
                    .id("nitro-subscriber-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            // `gradient_u8qKJM`: 984px tall at the top, masked to nothing from 60% down; the mask is
                            // drawn as a gradient of the page color over the lower 40%.
                            .children(background.map(|image| {
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .w_full()
                                    .h(px(984.))
                                    .child(img(image).w_full().h(px(984.)))
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .bottom_0()
                                            .w_full()
                                            .h(px(394.))
                                            .bg(linear_gradient(
                                                180.,
                                                linear_color_stop(page_color.opacity(0.), 0.),
                                                linear_color_stop(page_color, 1.),
                                            )),
                                    )
                            }))
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(self.nitro_greeting())
                                    .child(self.nitro_whats_new(cx))
                                    .child(self.nitro_favorites(cx))
                                    .child(self.nitro_gift_banner(cx))
                                    // `footerSpacing_IRnLy8`, then the 112 x 85 `bottomIllustration`.
                                    .child(div().h(px(180.)).w_full())
                                    .child(div().w(px(112.)).h(px(85.)).children(
                                        bottom.map(|image| img(image).w(px(112.)).h(px(85.))),
                                    )),
                            ),
                    ),
            )
            .child(self.nitro_subscriber_bar(cx))
            .child(crate::ui::settings::auto_scrollbar(scroll))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_greeting_follows_the_local_hour() {
        assert_eq!(greeting(4), "Good Evening,");
        assert_eq!(greeting(5), "Good Morning,");
        assert_eq!(greeting(11), "Good Morning,");
        assert_eq!(greeting(12), "Good Afternoon,");
        assert_eq!(greeting(16), "Good Afternoon,");
        assert_eq!(greeting(17), "Good Evening,");
        assert_eq!(greeting(0), "Good Evening,");
    }

    #[test]
    fn each_tab_lists_the_favorites_of_its_category() {
        let ids = |tab| {
            favorites_in(tab)
                .into_iter()
                .map(|perk| perk.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids("bestof"),
            [
                "server-boosts",
                "profiles",
                "hd-video",
                "more-emojis",
                "large-uploads"
            ]
        );
        assert_eq!(ids("appearance"), ["profiles", "entrance-sounds"]);
        assert_eq!(ids("upgrades").len(), 8);
        assert_eq!(ids("vip"), ["early-access", "badge", "special-pricing"]);
    }
}
