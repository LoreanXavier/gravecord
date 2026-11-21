//! The new server welcome (module 302125 `A`, card `E`): the start of a server's default channel. Module 891522 `ab`
//! draws it in place of the channel's own start header when the channel is `getDefaultChannel(guild)` (module
//! 808728: the first channel by raw `position` of the selectable kinds the account can view); threads, forum posts
//! and media posts take their own branches first.
//!
//! "Welcome to {guild}" (`rkHVKf`, two paragraphs) over one sentence: `gwyU/J` for a server older than 30 days
//! (`NOr`, 2592e6 ms from the id's snowflake), else `1ach9C` for its owner and `ezm+/j` for anyone else. A young
//! server adds the guide sentence (`UOtD32`) and the step cards, in this order: Invite your friends (Create Invite on
//! the channel or a vanity code; done with more than one member), Personalize your server with an icon (Manage
//! Server; done with an icon), Send your first message (Send Messages; done once any loaded message of the server is
//! the account's own user message), Download the Discord App (web only, so never here), Add your first app (Manage
//! Server; done when `GET /guilds/{id}/integrations` lists any), Unlock perks for everyone with Boosts (Administrator;
//! done with any boost).
//!
//! Measured in `design/dom/channel/after-upload.json` (`container_rh88Fa`) and `883221.*.css`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::{ChannelKind, Guild, Id, message_kinds, permissions};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// `NOr` (module 652215): a server older than this is no longer new.
const NEW_FOR_MS: i64 = 2_592_000_000;
/// The Discord epoch of snowflake ids.
const EPOCH_MS: i64 = 1_420_070_400_000;
/// `USER_JOIN` (module 652215 `lAJ`).
const USER_JOIN: u64 = 7;
/// The answer of `GET /guilds/{id}/integrations` (module 794967 `c`) for the Add your first app card.
const APPS_PREFIX: &str = "new-server-apps:";
/// `GUILD_GETTING_STARTED` ("360045138571") through `getArticleURL` (module 975571, en-us) with module 302125's query.
const GUIDE_URL: &str = "https://support.discord.com/hc/en-us/articles/360045138571?utm_source=discord&utm_medium=blog&utm_campaign=2020-06_help-new-user&utm_content=--t%3Apm";

/// Capture only: the server in view counts as created just now, so the young-server cards show on the fixture.
#[cfg(feature = "capture")]
pub(crate) static AS_NEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One step card (`key` in module 302125).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Invite,
    Personalize,
    Message,
    Download,
    AddApp,
    Boost,
}

impl Step {
    /// The card's title string.
    fn title_key(self) -> &'static str {
        match self {
            Step::Invite => "q9n0Ta",
            Step::Personalize => "c5kxPh",
            Step::Message => "SoP7+l",
            Step::Download => "pGVNI9",
            Step::AddApp => "IhHDEO",
            Step::Boost => "6Qbqxw",
        }
    }

    /// `iconUrl`.
    fn icon_url(self) -> &'static str {
        match self {
            Step::Invite => "https://discord.com/assets/ea08bfae3e0ab96d.svg",
            Step::Personalize => "https://discord.com/assets/428a003b3c729aa6.svg",
            Step::Message => "https://discord.com/assets/2ed198e767bd5423.svg",
            Step::Download => "https://discord.com/assets/eea7561d0cfcff41.svg",
            // Module 383669.
            Step::AddApp => "https://discord.com/assets/261f952bf028fa34.svg",
            Step::Boost => {
                "https://cdn.discordapp.com/assets/content/bc3217e772906510d881b75ebefea754b9c3ba903ddf6f994e46e5c5a85770a3.svg"
            }
        }
    }

    fn id(self) -> &'static str {
        match self {
            Step::Invite => "invite",
            Step::Personalize => "customize",
            Step::Message => "message",
            Step::Download => "download",
            Step::AddApp => "addapp",
            Step::Boost => "boost",
        }
    }
}

/// What the steps of module 302125 read.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Facts {
    /// Older than `NOr`: no cards at all.
    pub old: bool,
    /// `HZ()` of module 574381: the web client, the only one with the download card.
    pub web: bool,
    pub can_invite: bool,
    pub can_manage_guild: bool,
    pub can_message: bool,
    pub admin: bool,
    pub populated: bool,
    pub personalized: bool,
    pub messaged: bool,
    pub downloaded: bool,
    pub has_apps: bool,
    pub boosted: bool,
}

/// The cards in their order, each with whether it is done.
pub(super) fn steps(facts: &Facts) -> Vec<(Step, bool)> {
    let mut list = Vec::new();
    if facts.old {
        return list;
    }
    if facts.can_invite {
        list.push((Step::Invite, facts.populated));
    }
    if facts.can_manage_guild {
        list.push((Step::Personalize, facts.personalized));
    }
    if facts.can_message {
        list.push((Step::Message, facts.messaged));
    }
    if facts.web {
        list.push((Step::Download, facts.downloaded));
    }
    if facts.can_manage_guild {
        list.push((Step::AddApp, facts.has_apps));
    }
    if facts.admin {
        list.push((Step::Boost, facts.boosted));
    }
    list
}

/// The sentence under the title.
pub(super) fn subtitle_key(owner: bool, old: bool) -> &'static str {
    if old {
        "gwyU/J"
    } else if owner {
        "1ach9C"
    } else {
        "ezm+/j"
    }
}

/// When a snowflake id was made, in epoch milliseconds.
fn created_ms(id: Id) -> i64 {
    ((id >> 22) as i64) + EPOCH_MS
}

/// `extractTimestamp(id) < Date.now() - NOr`.
pub(super) fn is_old(guild: Id, now_ms: i64) -> bool {
    created_ms(guild) < now_ms - NEW_FOR_MS
}

/// `m` of module 95701 as far as server channels go: text, announcement, store, directory, forum, media and guild app.
fn selectable(kind: &ChannelKind) -> bool {
    matches!(
        kind,
        ChannelKind::Text | ChannelKind::Other(5 | 6 | 14 | 15 | 16 | 20)
    )
}

/// `getDefaultChannel`: the first of `(id, position, selectable and viewable)` by position. The official sort is
/// stable over the store's order, which ties break here by id.
pub(super) fn first_channel(channels: impl IntoIterator<Item = (Id, i64, bool)>) -> Option<Id> {
    channels
        .into_iter()
        .filter(|(_, _, usable)| *usable)
        .min_by_key(|(id, position, _)| (*position, *id))
        .map(|(id, _, _)| id)
}

/// Whether module 891522 draws the welcome for `channel` of `guild`.
pub(super) fn shows_for(channel: Id, default: Option<Id>) -> bool {
    default == Some(channel)
}

pub(super) fn default_channel(guild: &Guild, me: permissions::Me) -> Option<Id> {
    first_channel(guild.channels.iter().map(|channel| {
        (
            channel.id,
            channel.position,
            selectable(&channel.kind)
                && permissions::can_in_channel(guild, channel, me, permissions::VIEW_CHANNEL),
        )
    }))
}

fn now_for(guild: Id) -> i64 {
    #[cfg(feature = "capture")]
    if AS_NEW.load(std::sync::atomic::Ordering::Relaxed) {
        return created_ms(guild);
    }
    let _ = guild;
    chrono::Utc::now().timestamp_millis()
}

impl Root {
    /// `GET /guilds/{id}/integrations` with the applications, as module 302125 reads it on mount with Manage Server.
    fn request_new_server_apps(&mut self, guild: Id) {
        let key = format!("{APPS_PREFIX}{guild}");
        if self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!(
                    "/guilds/{guild}/integrations?include_applications=true&include_role_connections_metadata=true"
                ),
            });
        }
    }

    /// `guildMessaged`: a loaded message of the server is the account's own user message. Only read for a young
    /// server, whose channels hold few messages.
    fn messaged_in(&self, guild: &Guild, me: Id) -> bool {
        guild.channels.iter().any(|channel| {
            self.stores.messages.get(&channel.id).is_some_and(|list| {
                list.iter().any(|message| {
                    message.author.id == me && message_kinds::USER_MESSAGE.contains(&message.kind)
                })
            })
        })
    }

    /// `guildPopulated`: more than one member, or a join message in the system channel.
    /// GAP: the store keeps no `system_channel_id`, so a join message in any loaded channel of the server counts.
    fn populated(&self, guild: &Guild) -> bool {
        self.stores
            .member_counts
            .get_member_count(guild.id)
            .unwrap_or(0)
            > 1
            || guild.channels.iter().any(|channel| {
                self.stores
                    .messages
                    .get(&channel.id)
                    .is_some_and(|list| list.iter().any(|message| message.kind == USER_JOIN))
            })
    }

    /// The welcome in place of the channel start header, when the channel in view is the server's default one.
    pub(super) fn new_server_welcome(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.current_channel()?;
        let guild = self.current_guild()?;
        let me = self.stores.me()?;
        if !shows_for(channel.id, default_channel(guild, me)) {
            return None;
        }
        let guild_id = guild.id;
        let old = is_old(guild_id, now_for(guild_id));
        let owner = guild.owner_id == Some(me.id);
        let mut facts = Facts {
            old,
            ..Facts::default()
        };
        if !old {
            facts.can_invite =
                permissions::can_in_channel(guild, channel, me, permissions::CREATE_INSTANT_INVITE)
                    || guild.vanity_url_code.is_some();
            facts.can_manage_guild =
                permissions::can_in_guild(guild, me, permissions::MANAGE_GUILD);
            facts.can_message =
                permissions::can_in_channel(guild, channel, me, permissions::SEND_MESSAGES);
            facts.admin = permissions::can_in_guild(guild, me, permissions::ADMINISTRATOR);
            facts.populated = facts.can_invite && self.populated(guild);
            // `icon != null && !LayerStore.hasLayers()`: the chat is not drawn under an open settings layer.
            facts.personalized = guild.icon.is_some();
            facts.messaged = facts.can_message && self.messaged_in(guild, me.id);
            facts.has_apps = self
                .fetched
                .get(&format!("{APPS_PREFIX}{guild_id}"))
                .and_then(|body| body.as_array())
                .is_some_and(|list| !list.is_empty());
            facts.boosted = guild.premium_subscription_count > 0;
        }
        if facts.can_manage_guild && !self.requested.contains(&format!("{APPS_PREFIX}{guild_id}")) {
            let root = cx.entity();
            cx.defer(move |cx| {
                root.update(cx, |this, _| this.request_new_server_apps(guild_id));
            });
        }
        let list = steps(&facts);
        let name = guild.name.clone();
        // `rkHVKf` is two `$p` paragraphs: "Welcome to" and the name.
        let title: Vec<String> = dc_intl::string("rkHVKf")
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(|line| line.replace("{guildName}", &name))
            .collect();
        let sentence = dc_intl::t(subtitle_key(owner, old)).to_string();
        for (step, _) in &list {
            let url = step.icon_url();
            if !self.images.contains_key(url) {
                self.want_image(url.to_owned(), url.to_owned(), cx);
            }
        }
        // GAP: the entrance animation (title from -20px and fading in, cards staggered by 100ms) runs only for an
        // account whose user has neither `desktop` nor `mobile` set, which the stores do not keep; a desktop account
        // has `desktop` set, so it is left out.
        let subtitle = if list.is_empty() {
            div().child(sentence).into_any_element()
        } else {
            let mut text = format!("{sentence} ");
            let mut link = 0..0;
            for (run, hook) in
                dc_intl::format_runs("UOtD32", &[("guideURL", GUIDE_URL.to_owned().into())])
            {
                let start = text.len();
                text.push_str(&run);
                if hook.is_some() {
                    link = start..text.len();
                }
            }
            let styled = gpui::StyledText::new(text).with_highlights([(
                link.clone(),
                gpui::HighlightStyle {
                    color: Some(color("--text-link").into()),
                    underline: Some(gpui::UnderlineStyle {
                        thickness: px(1.),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )]);
            gpui::InteractiveText::new(
                SharedString::from(format!("new-server-guide-{guild_id}")),
                styled,
            )
            .on_click(vec![link], {
                let open = cx.listener(|this, _: &usize, _, cx| {
                    this.follow_link(GUIDE_URL, "", cx);
                });
                move |index, window, cx| open(&index, window, cx)
            })
            .into_any_element()
        };
        let cards: Vec<AnyElement> = list
            .iter()
            .map(|(step, done)| self.new_server_card(*step, *done, guild_id, cx))
            .collect();
        let has_cards = !cards.is_empty();
        Some(
            div()
                .pt(px(16.))
                .px(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(400.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .w_full()
                                .child(
                                    div()
                                        .w_full()
                                        .text_center()
                                        .text_size(px(32.))
                                        .line_height(px(40.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .children(
                                            title
                                                .into_iter()
                                                .map(|line| div().w_full().child(line)),
                                        ),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .mt(px(8.))
                                        .when(has_cards, |this| this.mb(px(12.)))
                                        .text_center()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(subtitle),
                                ),
                        )
                        .children(cards),
                )
                .into_any_element(),
        )
    }

    /// Card `E` of module 302125: 58 high with a 1px `--border-subtle` edge, 8px corners, 16px padding and 8px above;
    /// the 24px picture, the title 12px after it (14/18 semibold, 16px before the end mark) and the chevron, or the
    /// green check once done (picture, title and check at 0.6).
    /// GAP: the check's pop (`completed_rh88Fa`, 0.8s) when a step is done while the welcome is shown.
    fn new_server_card(
        &self,
        step: Step,
        done: bool,
        guild: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let faded = if done { 0.6 } else { 1. };
        let picture = match self.images.get(step.icon_url()) {
            Some(image) => img(image.clone())
                .size(px(24.))
                .flex_none()
                .into_any_element(),
            None => div().size(px(24.)).flex_none().into_any_element(),
        };
        let end = if done {
            div()
                .size(px(24.))
                .flex_none()
                .rounded_full()
                .bg(color("--status-positive"))
                .opacity(faded)
                .flex()
                .items_center()
                .justify_center()
                .child(icon("checkmark_large", 16., "--white"))
                .into_any_element()
        } else {
            icon("overlay_chevron_small_right", 24., "--icon-muted").into_any_element()
        };
        div()
            .id(SharedString::from(format!("new-server-step-{}", step.id())))
            .w_full()
            .mt(px(8.))
            .p(px(16.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-mod-muted"))
            .hover(|style| style.bg(color("--background-mod-subtle")))
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.new_server_step(step, guild, window, cx)
                }),
            )
            .child(div().flex_none().opacity(faded).child(picture))
            .child(
                div()
                    .flex_grow(1.)
                    .ml(px(12.))
                    .mr(px(16.))
                    .opacity(faded)
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t(step.title_key()).to_string()),
            )
            .child(end)
            .into_any_element()
    }

    /// What each card opens (`handleInvite` .. `handleBoost` of module 302125).
    fn new_server_step(
        &mut self,
        step: Step,
        guild: Id,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        match step {
            Step::Invite => self.open_invite(cx),
            Step::Personalize => {
                self.open_server_settings_at(guild, super::server_settings::Page::Profile, cx)
            }
            // The welcome is drawn in the channel in view, so this is `TEXTAREA_FOCUS`.
            // GAP: the composer's highlight flash (`highlight: true`).
            Step::Message => self.focus_composer(window, cx),
            // GAP: module 987482 (the download modal); the card never shows outside the web client.
            Step::Download => {}
            // `handleAddApplication`: "Introducing App Directory" (module 633605).
            Step::AddApp => self.open_app_directory_promo(guild, cx),
            // `CHANNEL(guild, GUILD_BOOSTS)`: the server's boost page.
            Step::Boost => self.open_boost_page(cx),
        }
        cx.notify();
    }
}

/// Module 633605's header picture (`aspectRatio: "6/4"`) and the 24px icons of its three lines.
const PROMO_ART: super::graphic_modal::Graphic = super::graphic_modal::Graphic {
    url: "https://cdn.discordapp.com/assets/content/2c7d0b82f2c7be0c182a49fcfab5e6a0b4931dccef76e6a851e54260aebca02a.svg",
    width: 288.,
    height: 192.,
};
const PROMO_LINES: [(&str, &str); 3] = [
    (
        "https://cdn.discordapp.com/assets/content/6f2327d812c75a6d27a59dd476f7e35db632b0e8ca8d3ced52d6ff0c867f6e5f.svg",
        "0vKo5F",
    ),
    (
        "https://cdn.discordapp.com/assets/content/96552954edc2aaf6953969b70c978f2601341c8c90edbc90e605e0392cada677.svg",
        "CprEDO",
    ),
    (
        "https://cdn.discordapp.com/assets/content/3b8d3a95cbf44767e9a35569bdbebbb5515039e038bfb53924f500e33dc225c4.svg",
        "wQJMPC",
    ),
];

impl Root {
    /// Opens module 633605 for this server and asks for its pictures.
    pub(super) fn open_app_directory_promo(&mut self, guild: Id, cx: &mut Context<Self>) {
        self.request_graphic(PROMO_ART);
        for (url, _) in PROMO_LINES {
            self.request_image(url.to_owned(), url.to_owned());
        }
        self.app_directory_promo = Some(guild);
        cx.notify();
    }

    /// "Introducing App Directory" (module 633605) on the dialog with a header graphic (module 772707 `k`: `size:
    /// "sm"`, `gradientColor: "blue"`, the 6/4 picture), "Search, browse, and explore thousands of apps", the list of
    /// three lines (`listContainer_pzCk_B`: `--background-surface-higher`, a `--border-subtle` border, 8px corners,
    /// 8px 16px of padding, 16px above; each `listItem_pzCk_B` a 24px icon 8px before its `body-sm/medium` text, 8px
    /// above and below; the thin `--border-subtle` divider at half opacity between them) and Check it out (`primary`),
    /// which closes it and opens the App Directory (`transitionToGlobalDiscovery({tab: APPS})`).
    pub(super) fn app_directory_promo_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.app_directory_promo?;
        let mut list = div()
            .mt(px(16.))
            .w_full()
            .py(px(8.))
            .px(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .flex()
            .flex_col()
            .gap(px(8.));
        for (index, (url, key)) in PROMO_LINES.iter().enumerate() {
            if index > 0 {
                list = list.child(
                    div()
                        .h(px(1.))
                        .w_full()
                        .bg(color("--border-subtle"))
                        .opacity(0.5),
                );
            }
            list = list.child(
                div()
                    .my(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div().size(px(24.)).flex_none().children(
                            self.images
                                .get(*url)
                                .cloned()
                                .map(|art| img(art).size(px(24.))),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t(key)),
                    ),
            );
        }
        let close =
            || -> super::modal::Handler { Box::new(|this, _, _| this.app_directory_promo = None) };
        Some(self.graphic_modal_with(
            super::graphic_modal::Spec {
                id: "app-directory-promo",
                graphic: Some(PROMO_ART),
                title: dc_intl::t("2tv5YG").into(),
                subtitle: Some(div().child(dc_intl::t("pgwlD7")).into_any_element()),
                body: Some(list.into_any_element()),
                buttons: vec![super::modal::button(
                    "app-directory-promo-check",
                    dc_intl::t("5e8Mrp"),
                    super::modal::Tone::Primary,
                    |this, _, cx| {
                        this.app_directory_promo = None;
                        this.open_discover(super::discover::Place::Apps, cx);
                    },
                )],
                close: close(),
            },
            super::graphic_modal::Look {
                width: 400.,
                gradient: (
                    "--expressive-gradient-blue-start",
                    "--expressive-gradient-blue-end",
                ),
                aspect: 6. / 4.,
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_directory_promo_strings_are_the_official_ones() {
        assert_eq!(dc_intl::t("2tv5YG"), "Introducing App Directory");
        assert_eq!(dc_intl::t("5e8Mrp"), "Check it out");
        assert_eq!(
            dc_intl::t(PROMO_LINES[1].1),
            "Automate useful tasks and free up time"
        );
    }

    fn snowflake_at(ms: i64) -> Id {
        ((ms - EPOCH_MS) as u64) << 22
    }

    #[test]
    fn it_shows_only_in_the_default_channel() {
        // The first viewable selectable channel by position, whatever its id.
        let default = first_channel([(30, 2, true), (20, 0, false), (10, 1, true), (40, 1, true)]);
        assert_eq!(default, Some(10));
        assert!(shows_for(10, default));
        assert!(!shows_for(30, default));
        assert!(!shows_for(10, first_channel([(10, 0, false)])));
        assert!(selectable(&ChannelKind::Other(15)));
        assert!(!selectable(&ChannelKind::Voice));
        assert!(!selectable(&ChannelKind::Category));
    }

    #[test]
    fn a_server_is_new_for_thirty_days() {
        let now = 1_800_000_000_000;
        assert!(!is_old(snowflake_at(now - NEW_FOR_MS + 1000), now));
        assert!(is_old(snowflake_at(now - NEW_FOR_MS - 1000), now));
        assert_eq!(subtitle_key(true, false), "1ach9C");
        assert_eq!(subtitle_key(false, false), "ezm+/j");
        assert_eq!(subtitle_key(true, true), "gwyU/J");
    }

    #[test]
    fn the_cards_follow_permissions_in_order() {
        let all = Facts {
            can_invite: true,
            can_manage_guild: true,
            can_message: true,
            admin: true,
            messaged: true,
            ..Facts::default()
        };
        let order: Vec<Step> = steps(&all).into_iter().map(|(step, _)| step).collect();
        assert_eq!(
            order,
            [
                Step::Invite,
                Step::Personalize,
                Step::Message,
                Step::AddApp,
                Step::Boost
            ]
        );
        assert_eq!(steps(&all)[2], (Step::Message, true));
        assert!(steps(&Facts { old: true, ..all }).is_empty());
        let member = Facts {
            can_message: true,
            ..Facts::default()
        };
        assert_eq!(steps(&member), [(Step::Message, false)]);
        assert_eq!(
            steps(&Facts {
                web: true,
                ..member
            })
            .len(),
            2
        );
    }
}
