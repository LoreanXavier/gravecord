//! Announcement channels (type 5): the Follow button in the header and in place of the composer, the Follow modal
//! (module 464035), and Publish on an own message with its confirm (modules 891522 and 10502).
//!
//! Nothing here has been seen in the official client yet (no second Community server to capture): every layout value
//! is read from the modules and stylesheets named at each place, and the rest is marked as assumed. The harness never
//! sends a follow or a publish.

use super::{Root, color, icon, modal};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Id, Message, permissions};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Rgba, SharedString, deferred, div, img,
    prelude::*, px, rgba,
};
use serde_json::json;
use std::collections::HashMap;

/// The keys the answers come back under (`Update::Fetched`).
pub(super) const FOLLOW_KEY: &str = "channel-follow";
pub(super) const CROSSPOST_KEY: &str = "channel-crosspost";
pub(super) const STATS_KEY: &str = "follower-stats:";

/// `ChannelKind::Other(5)`: `GUILD_ANNOUNCEMENT`.
const ANNOUNCEMENT: ChannelKind = ChannelKind::Other(5);
/// `TOO_MANY_WEBHOOKS` (module 652215): 30007.
const TOO_MANY_WEBHOOKS: u64 = 30007;

/// Which select of the Follow modal is open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Select {
    Guild,
    Channel,
}

/// Module 505859's pictures: `u` for the dark themes (`M` of module 462887), `h` for light.
const FOLLOW_ART_DARK: [&str; 3] = [
    "https://discord.com/assets/6dadee9234160dc1.svg",
    "https://discord.com/assets/62d886a31822c89a.svg",
    "https://discord.com/assets/377cb29b8f197452.svg",
];
const FOLLOW_ART_LIGHT: [&str; 3] = [
    "https://discord.com/assets/36668e0a5832822f.svg",
    "https://discord.com/assets/3a31d989497ba4f1.svg",
    "https://discord.com/assets/f959ee6e6a8d2d0b.svg",
];
/// Module 505859's lines ("What a nice addition to your server. Lookin' real ... in here.").
const FOLLOW_LINES: [&str; 10] = [
    "w2o/60", "FiAvKg", "vKUFek", "veQl5T", "Pxb7BR", "W03w++", "95HTb5", "+XFelz", "hedHel", "jgC65t",
];

/// The picture for the current theme.
fn follow_art(index: usize) -> &'static str {
    if matches!(super::theme(), super::Theme::Light) {
        FOLLOW_ART_LIGHT[index]
    } else {
        FOLLOW_ART_DARK[index]
    }
}

/// The Follow modal's state (`B` of module 464035): the announcement channel, the chosen server and channel.
pub(super) struct FollowModal {
    channel: Id,
    guild: Option<Id>,
    destination: Option<Id>,
    open: Option<Select>,
    error: Option<String>,
    busy: bool,
}

/// `getFollowerStatsForChannel`: what the confirm of a publish says.
#[derive(Clone, Copy, Default)]
pub(super) struct FollowerStats {
    pub guilds_following: Option<u64>,
    pub users_seen: Option<u64>,
}

/// One server the account may follow into: the channels in it with Manage Webhooks.
struct Target {
    guild: Id,
    name: String,
    channels: Vec<(Id, String)>,
}

pub(super) type StatsMap = HashMap<Id, FollowerStats>;

impl Root {
    /// `type === GUILD_ANNOUNCEMENT && guild.features.has(NEWS)`: the channel others can follow.
    pub(super) fn followable_channel(&self, channel: &Channel) -> bool {
        channel.kind == ANNOUNCEMENT
            && self
                .current_guild()
                .is_some_and(|guild| guild.features.iter().any(|feature| feature == "NEWS"))
    }

    /// Module 678655 `eh`: a followable channel this account may not post in shows the Follow bar.
    pub(super) fn follow_blocker_applies(&self) -> bool {
        self.current_channel().is_some_and(|channel| {
            self.followable_channel(channel)
                && (self.follow_blocker_forced
                    || !self.can_in_channel(channel, permissions::SEND_MESSAGES))
        })
    }

    /// The first row of a server text or announcement channel (strings `Szye2K` and `jRNiwM` of module 818418): the
    /// 68px circle with the channel's glyph at 42px, "Welcome to #name!", "This is the start of the #name channel."
    /// and, with Manage Channels, the Edit Channel button. Measured in capture stage-ann/ann-view: icon at (391, 350),
    /// title box 40 high 8px under it, sentence 8px under that, button 12px under that.
    pub(super) fn channel_welcome(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.current_channel()?;
        let guild = self.guild?;
        let name = channel.name.clone();
        let target = channel.id;
        // The hash for every kind: the capture of an announcement channel (stage-ann/ann-view) shows it too.
        let glyph = "text_channel";
        let can_edit = self.my_permission(guild, Some(channel), permissions::MANAGE_CHANNELS);
        Some(
            div()
                .pt(px(12.))
                .child(
                    div()
                        .ml(px(16.))
                        .mr(px(32.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .size(px(68.))
                                .rounded_full()
                                .bg(rgba(0x97979f14))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(glyph, 42., "--text-strong")),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(32.))
                                .line_height(px(40.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(format!("Welcome to #{name}!")),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(16.))
                                .line_height(px(22.))
                                .text_color(color("--text-default"))
                                .child(format!("This is the start of the #{name} channel.")),
                        )
                        .when(can_edit, |column| {
                            column.child(
                                div().mt(px(12.)).flex().child(
                                    div()
                                        .id("channel-welcome-edit")
                                        .h(px(32.))
                                        .px(px(11.))
                                        .rounded(px(8.))
                                        .bg(rgba(0x97979f1f))
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .cursor_pointer()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_channel_settings(target, window, cx)
                                        }))
                                        .child(icon("settings_edit_pencil", 16., "--text-strong"))
                                        .child("Edit Channel"),
                                ),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// `renderFollowButton` (module 289287): `followButton_z_gUBg` is 4px 8px padded with 8px after it; the button is
    /// secondary, small. Assumed: 32 high with 11px sides, as the composer bar's button (`size="sm"`).
    pub(super) fn follow_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.current_channel()?;
        if !self.followable_channel(channel) {
            return None;
        }
        let target = channel.id;
        Some(
            div()
                .flex_none()
                // The wrapper starts 8px after the title wrapper (capture ann-view: 528.89 to 536.89), padded 8px.
                .ml(px(8.))
                .mr(px(8.))
                .px(px(8.))
                .py(px(4.))
                .child(
                    div()
                        .id("header-follow")
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_follow(target, cx)))
                        .child("Follow"),
                )
                .into_any_element(),
        )
    }

    /// `ea.A` (module 786051) with `Hl0Mqh` and "Follow" (module 678655 `render`): in place of the composer.
    pub(super) fn follow_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let channel = self.channel.unwrap_or_default();
        let seen = self
            .follower_stats
            .get(&channel)
            .and_then(|stats| stats.users_seen)
            .filter(|seen| *seen >= 1000);
        div()
            .relative()
            .flex_none()
            .mt(px(-16.))
            .px(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .parity_with_border("follow-composer", 1.)
                    .h(px(58.))
                    .py(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-low"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .ml(px(16.))
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .child(
                                div()
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(
                                        "Follow to get this channel's updates in your own server.",
                                    ),
                            )
                            // `C5bgrC`, from a thousand people seen on, rounded down to the thousand.
                            .children(seen.map(|seen| {
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(format!(
                                        "Over {} other people are also following.",
                                        super::role_popout::group_digits(seen / 1000 * 1000)
                                    ))
                            })),
                    )
                    .child(
                        div()
                            .id("composer-follow")
                            .flex_none()
                            .ml(px(12.))
                            .mr(px(12.))
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.open_follow(channel, cx)),
                            )
                            .child("Follow"),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn open_follow(&mut self, channel: Id, cx: &mut Context<Self>) {
        self.follow = Some(FollowModal {
            channel,
            guild: None,
            destination: None,
            open: None,
            error: None,
            busy: false,
        });
        cx.notify();
    }

    /// `getGuildsArray()` with the channels of the types in `N.XD` (text and announcement) where the account has
    /// Manage Webhooks.
    fn follow_targets(&self) -> Vec<Target> {
        let Some(me) = self.stores.me() else {
            return Vec::new();
        };
        self.stores
            .guilds
            .iter()
            .filter_map(|guild| {
                let channels: Vec<(Id, String)> = guild
                    .channels
                    .iter()
                    .filter(|channel| matches!(channel.kind, ChannelKind::Text | ANNOUNCEMENT))
                    .filter(|channel| {
                        permissions::can_in_channel(
                            guild,
                            channel,
                            me,
                            permissions::MANAGE_WEBHOOKS,
                        )
                    })
                    .map(|channel| (channel.id, channel.name.clone()))
                    .collect();
                (!channels.is_empty()).then(|| Target {
                    guild: guild.id,
                    name: guild.name.clone(),
                    channels,
                })
            })
            .collect()
    }

    /// `handleFollow`: `POST /channels/<source>/followers {webhook_channel_id}`. The harness never sends it.
    fn submit_follow(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.follow.as_mut() else {
            return;
        };
        let (Some(destination), false) = (open.destination, open.busy) else {
            return;
        };
        // Under `GRAVECORD_INERT=1` the request is refused and logged as `inert: refused Rest ...`.
        {
            open.busy = true;
            open.error = None;
            let _ = self.requests.send(Request::Rest {
                key: Some(FOLLOW_KEY.to_owned()),
                method: reqwest::Method::POST,
                route: format!("/channels/{}/followers", open.channel),
                body: Some(json!({"webhook_channel_id": destination})),
            });
        }
        cx.notify();
    }

    /// The answer of the follow request: `CHANNEL_FOLLOWER_CREATED` closes the modal; a failure says why.
    pub(super) fn follow_answered(&mut self, body: &serde_json::Value, cx: &mut Context<Self>) {
        let Some(open) = self.follow.as_mut() else {
            return;
        };
        open.busy = false;
        if body["ok"].as_bool().unwrap_or(false) {
            self.follow = None;
            self.open_follow_success();
        } else {
            open.error = Some(
                if body["body"]["code"].as_u64() == Some(TOO_MANY_WEBHOOKS) {
                    "The selected channel has too many webhooks. Please remove a webhook or select another channel."
                } else {
                    "Failed to follow the channel, please try again."
                }
                .to_owned(),
            );
        }
        cx.notify();
    }

    /// Module 505859 after a follow: one of its three pictures for the theme and one of its ten lines, each picked at
    /// random when it opens (`sample`).
    pub(super) fn open_follow_success(&mut self) {
        let nanos = chrono::Utc::now().timestamp_subsec_nanos() as usize;
        let art = nanos % FOLLOW_ART_DARK.len();
        let line = (nanos / 7) % FOLLOW_LINES.len();
        let url = follow_art(art);
        self.request_image(url.to_owned(), url.to_owned());
        self.follow_success = Some((art, line));
    }

    /// Module 505859 (`FollowSuccessModal`) on the design system modal (`EO`, `root_wRhEA_`): the picture 280 wide
    /// lifted 96px over the top (`image_wRhEA_`), then `modal_wRhEA_` (400 wide, 32px padding, centred): the line
    /// (`heading-xl/semibold` with `followSuccessHeader_wRhEA_`'s bold weight), the body (`body-md/medium`,
    /// `text-muted`, 16px above and 32px under it) and Got it! (`primary`).
    pub(super) fn follow_success_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (art, line) = self.follow_success?;
        let pose = super::motion::legacy_modal_pose(
            "follow-success-modal",
            "follow-success-layer",
            false,
        );
        let close = |this: &mut Self, cx: &mut Context<Self>| {
            this.follow_success = None;
            cx.notify();
        };
        let picture = self.images.get(follow_art(art)).cloned();
        let panel = div()
            .id("follow-success-modal")
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .occlude()
            .w(px(400.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .items_center()
            // The picture is lifted only once it has loaded (an `img` without its file has no height).
            .children(
                picture.map(|picture| div().w(px(280.)).mt(px(-96.)).child(img(picture).w(px(280.)))),
            )
            .child(
                div()
                    .w(px(400.))
                    .p(px(32.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    .child(
                        div()
                            .text_size(px(24.))
                            .line_height(px(30.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t(FOLLOW_LINES[line])),
                    )
                    .child(
                        div()
                            .mt(px(16.))
                            .mb(px(32.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("2QbSea")),
                    )
                    .child(modal::footer_button(
                        modal::button(
                            "follow-success-ok",
                            dc_intl::t("+IrDzN"),
                            modal::Tone::Primary,
                            |this, _, _| this.follow_success = None,
                        ),
                        cx,
                    )),
            );
        Some(
            deferred(
                div()
                    .id("follow-success-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(Rgba {
                        a: f32::from(0xb8u8) / 255. * pose.scrim,
                        ..rgba(0x000000ff)
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(move |this, _, _, cx| close(this, cx)))
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Module 464035 on the design system modal (`EO`): the header (`header_MXlLC1`) with the server's icon beside
    /// the channel, a centred title and subtitle, two selects, and Follow beside Cancel.
    pub(super) fn follow_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.follow.as_ref()?;
        let source = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == open.channel))?;
        let name = source
            .channels
            .iter()
            .find(|known| known.id == open.channel)
            .map(|known| known.name.clone())
            .unwrap_or_default();
        let nsfw = source
            .channels
            .iter()
            .any(|known| known.id == open.channel && known.nsfw);
        let targets = self.follow_targets();
        let none = targets.is_empty();
        let guild_value = open
            .guild
            .and_then(|id| targets.iter().find(|target| target.guild == id))
            .map(|target| target.name.clone());
        let channel_options: Vec<(Id, String)> = open
            .guild
            .and_then(|id| targets.iter().find(|target| target.guild == id))
            .map(|target| target.channels.clone())
            .unwrap_or_default();
        let channel_value = open
            .destination
            .and_then(|id| channel_options.iter().find(|(known, _)| *known == id))
            .map(|(_, name)| name.clone());
        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).
        let pose = super::motion::legacy_modal_pose("follow-modal", "follow-modal-layer", false);
        let picture = self.images.get(&format!("guild:{}", source.id)).cloned();
        // `header_MXlLC1`: `--background-base-lowest`, the server's icon and `channelContainer_MXlLC1` (16px in, 320
        // at most, `--background-base-lower`, 4px radius) holding the selected-looking channel row. Assumed: 40px
        // icon and a 112px header (the `d.rQ` header's own height is not in the modules read).
        // Measured in stage-ann/ann-follow-modal: the header is 66 high with the 50px icon 16px from its top and the
        // channel box (36 high) 16px right of the icon, both centred together.
        let header = div()
            .flex_none()
            .h(px(66.))
            .pt(px(16.))
            .rounded_t(px(4.))
            .bg(color("--background-base-lowest"))
            .flex()
            .items_center()
            .justify_center()
            .child(match picture {
                Some(picture) => img(picture).size(px(50.)).rounded_full().into_any_element(),
                None => div()
                    .size(px(50.))
                    .rounded_full()
                    .bg(color("--background-base-lower"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(color("--text-default"))
                    .child(
                        source
                            .name
                            .split_whitespace()
                            .filter_map(|word| word.chars().next())
                            .collect::<String>(),
                    )
                    .into_any_element(),
            })
            .child(
                div()
                    .ml(px(16.))
                    .max_w(px(320.))
                    .overflow_hidden()
                    .rounded(px(4.))
                    .bg(color("--background-base-lower"))
                    .child(
                        div()
                            .p(px(8.))
                            .flex()
                            .items_center()
                            .bg(color("--interactive-background-selected"))
                            .child(div().ml(px(6.)).mr(px(8.)).child(icon(
                                "announcement_channel",
                                16.,
                                "--text-muted",
                            )))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(name),
                            ),
                    ),
            );
        let select = |id: &'static str,
                      label: &str,
                      value: Option<String>,
                      shown: Select,
                      disabled: bool,
                      cx: &mut Context<Self>| {
            let focused = !disabled
                && (open.open == Some(shown)
                    || (open.open.is_none() && shown == Select::Guild && open.guild.is_none()));
            div()
                .relative()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        // `label_VhFsFg`: body-md/medium, muted while the select is disabled.
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color(if disabled && value.is_none() {
                            "--text-muted"
                        } else {
                            "--text-strong"
                        }))
                        .child(label.to_owned()),
                )
                .child(
                    div()
                        .id(id)
                        .h(px(40.))
                        .pl(px(11.))
                        .pr(px(9.))
                        .rounded(px(8.))
                        .border_2()
                        // `selectField_vWC4bk`: black at .12; the focused one (the first select when the dialog
                        // opens, or the one open) carries a 2px brand ring.
                        .border_color(if focused {
                            color("--brand-500")
                        } else {
                            Rgba { a: 0., ..rgba(0) }
                        })
                        .bg(Rgba {
                            a: 0.12,
                            ..rgba(0x000000ff)
                        })
                        .flex()
                        .items_center()
                        .justify_between()
                        .when(disabled, |this| this.opacity(0.5))
                        .when(!disabled, |this| this.cursor_pointer())
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color(if value.is_some() {
                            "--text-default"
                        } else {
                            "--text-muted"
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if disabled {
                                return;
                            }
                            if let Some(open) = this.follow.as_mut() {
                                open.open = if open.open == Some(shown) {
                                    None
                                } else {
                                    Some(shown)
                                };
                            }
                            cx.notify();
                        }))
                        .child(
                            div()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(value.unwrap_or_else(|| "Select...".to_owned())),
                        )
                        .child(icon(
                            if open.open == Some(shown) {
                                "discover_select_chevron_up"
                            } else {
                                "chevron_down"
                            },
                            20.,
                            "--interactive-text-default",
                        )),
                )
        };
        // The open list, drawn after both selects so it paints over the one below; `top` is where its select sits in
        // the column (label 16 + gap 8 + field 40, and the 16 gap between the two selects).
        let popup = |top: f32,
                     shown: Select,
                     disabled: bool,
                     options: Vec<(Id, String)>,
                     cx: &mut Context<Self>| {
            (open.open == Some(shown) && !disabled).then(|| {
                deferred(
                    div()
                        .absolute()
                        .top(px(top + 72.))
                        .left(px(0.))
                        .right(px(0.))
                        .max_h(px(200.))
                        .overflow_hidden()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .py(px(4.))
                        .occlude()
                        .children(options.into_iter().enumerate().map(
                            |(index, (picked, label))| {
                                div()
                                    .id(SharedString::from(format!("follow-option-{index}")))
                                    .h(px(40.))
                                    .px(px(12.))
                                    .flex()
                                    .items_center()
                                    .cursor_pointer()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .hover(|style| {
                                        style.bg(color("--interactive-background-hover"))
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.follow_pick(shown, picked);
                                        cx.notify();
                                    }))
                                    // A server's row carries its 16px icon, the acronym when it has no picture
                                    // (stage-ann/ann-follow-select-open: icon at 456, name at 484).
                                    .when(shown == Select::Guild, |row| {
                                        row.child(
                                            div()
                                                .mr(px(12.))
                                                .size(px(16.))
                                                .rounded_full()
                                                .bg(color("--background-base-lowest"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(7.))
                                                .line_height(px(8.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-default"))
                                                .child(
                                                    label
                                                        .split_whitespace()
                                                        .filter_map(|word| word.chars().next())
                                                        .collect::<String>(),
                                                ),
                                        )
                                    })
                                    .child(label)
                            },
                        )),
                )
                .with_priority(2)
            })
        };
        let guild_options: Vec<(Id, String)> = targets
            .iter()
            .map(|target| (target.guild, target.name.clone()))
            .collect();
        let note = open.error.clone().unwrap_or_else(|| {
            if none {
                "Uh oh, you don't have webhook permissions in any server you're in. Ask your server admin to give you the permission or ask them to come follow themselves!".to_owned()
            } else {
                "Only servers and channels you have webhook permissions in are shown.".to_owned()
            }
        });
        let body = div()
            .flex_none()
            .pl(px(24.))
            .pr(px(16.))
            .pt(px(32.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .text_center()
                    .child("Add this channel's updates to your server!"),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .text_center()
                    .child("Select where you want these updates to go. You can remove this at any time in Server Settings > Integrations > Channels Followed."),
            )
            .children(nsfw.then(|| {
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child("Note: This is an age-restricted channel.")
            }))
            .child(
                div()
                    .mt(px(24.))
                    .w_full()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .text_left()
                    .child(select(
                        "follow-guild",
                        "Send to",
                        guild_value,
                        Select::Guild,
                        none,
                        cx,
                    ))
                    .child(select(
                        "follow-channel",
                        "Select channel",
                        channel_value,
                        Select::Channel,
                        channel_options.is_empty(),
                        cx,
                    ))
                    .children(popup(0., Select::Guild, none, guild_options, cx))
                    .children(popup(
                        80.,
                        Select::Channel,
                        channel_options.is_empty(),
                        channel_options.clone(),
                        cx,
                    ))
                    .child(
                        div()
                            .mb(px(16.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color(if none || open.error.is_some() {
                                "--text-feedback-critical"
                            } else {
                                "--text-muted"
                            }))
                            .child(note),
                    ),
            );
        let ready = open.guild.is_some() && open.destination.is_some();
        // Cancel and Follow are 100 wide, 8 apart, 24px from the right edge (stage-ann/ann-follow-modal).
        let footer = div()
            .flex_none()
            .px(px(24.))
            .pt(px(16.))
            .pb(px(16.))
            .flex()
            .flex_row_reverse()
            .gap(px(8.))
            .child(div().w(px(100.)).flex().child(modal::footer_button(
                modal::button(
                    "follow-submit",
                    "Follow",
                    if !ready {
                        modal::Tone::PrimaryDisabled
                    } else if open.busy {
                        modal::Tone::PrimaryLoading
                    } else {
                        modal::Tone::Primary
                    },
                    |this, _, cx| this.submit_follow(cx),
                ),
                cx,
            )))
            .child(div().w(px(100.)).flex().child(modal::footer_button(
                modal::button(
                    "follow-cancel",
                    "Cancel",
                    modal::Tone::Secondary,
                    |this, _, _| {
                        this.follow = None;
                    },
                ),
                cx,
            )));
        let panel = div()
            .id("follow-modal")
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .occlude()
            .w(px(444.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .child(footer);
        Some(
            deferred(
                div()
                    .id("follow-modal-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(Rgba {
                        a: f32::from(0xb8u8) / 255. * pose.scrim,
                        ..rgba(0x000000ff)
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.follow = None;
                        cx.notify();
                    }))
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `handleGuildSelect` and `handleChannelSelect`: choosing a server selects its `general` (else first) channel.
    fn follow_pick(&mut self, which: Select, picked: Id) {
        let targets = self.follow_targets();
        let Some(open) = self.follow.as_mut() else {
            return;
        };
        open.open = None;
        match which {
            Select::Guild => {
                if open.guild != Some(picked) {
                    open.guild = Some(picked);
                    open.destination = targets
                        .iter()
                        .find(|target| target.guild == picked)
                        .and_then(|target| {
                            target
                                .channels
                                .iter()
                                .find(|(_, name)| name == "general")
                                .or_else(|| target.channels.first())
                        })
                        .map(|(id, _)| *id);
                }
            }
            Select::Channel => open.destination = Some(picked),
        }
    }

    /// The hover bar's Publish (module 891522): an announcement channel of a NEWS server, on a plain message that is
    /// this account's own or one it may manage, `Published` and disabled once crossposted (flag 1).
    pub(super) fn can_publish(&self, message: &Message) -> bool {
        let Some(channel) = self.current_channel() else {
            return false;
        };
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == message.author.id);
        self.followable_channel(channel)
            && self.can_in_channel(channel, permissions::SEND_MESSAGES)
            && (own || self.can_in_channel(channel, permissions::MANAGE_MESSAGES))
            // `tJ.A`: a default message or a reply.
            && matches!(message.kind, 0 | 19)
    }

    /// `ni.qv` "publish": `AnnouncementsIcon` before the More button; disabled and labelled `1kWJAr` once published.
    pub(super) fn publish_button(
        &self,
        message: &Message,
        button: &dyn Fn(&str) -> gpui::Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use super::tooltip::{Side, Tip};
        if !self.can_publish(message) {
            return None;
        }
        let published = message.flags & 1 == 1;
        let (channel, id) = (message.channel_id, message.id);
        let element = self.with_tip(
            "toolbar-publish",
            Tip::text(if published { "Published" } else { "Publish" }, Side::Top),
            button("announcement_channel").when(published, |this| this.opacity(0.5)),
            cx,
        );
        Some(if published {
            element.into_any_element()
        } else {
            element
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tip = None;
                    this.open_publish(channel, id, cx);
                }))
                .into_any_element()
        })
    }

    /// `t_.Le`: opens the confirm and asks for the channel's follower numbers.
    fn open_publish(&mut self, channel: Id, message: Id, cx: &mut Context<Self>) {
        self.publish_confirm = Some((channel, message));
        if !self.follower_stats.contains_key(&channel) {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{STATS_KEY}{channel}")),
                method: reqwest::Method::GET,
                route: format!("/channels/{channel}/follower-stats"),
                body: None,
            });
        }
        cx.notify();
    }

    /// `CHANNEL_FOLLOWER_STATS_FETCH_SUCCESS` and `_FAILURE`.
    pub(super) fn follower_stats_answered(
        &mut self,
        channel: Id,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let stats = if body["ok"].as_bool().unwrap_or(false) {
            FollowerStats {
                guilds_following: body["body"]["guilds_following"].as_u64(),
                users_seen: body["body"]["users_seen_ever"].as_u64(),
            }
        } else {
            FollowerStats::default()
        };
        self.follower_stats.insert(channel, stats);
        cx.notify();
    }

    /// The confirm of module 10502 on the confirm dialog (`o.u`): "Are you sure?", the body by how many servers
    /// follow, Confirm beside Cancel. Confirming posts `/crosspost`; the harness never does.
    pub(super) fn publish_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (channel, message) = self.publish_confirm?;
        let following = self
            .follower_stats
            .get(&channel)
            .and_then(|stats| stats.guilds_following)
            .filter(|count| *count > 0);
        // `GCGrNP`, `IMhGZz`, `8FpqOs`; while the numbers load the body shows nothing (`r.y` spinner stands in).
        let loading = !self.follower_stats.contains_key(&channel);
        // The text is the dialog's body (`body-md/normal` 16/20 in `--text-default`, 20px of room under it:
        // stage-ann/ann-publish-confirm, body box 80 for 3 lines), not a subtitle.
        let body = div()
            .flex()
            .flex_col()
            .pb(px(20.))
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .when(loading, |this| this.child(""))
            .when(!loading, |this| match following {
                Some(count) => this
                    .child(
                        div().mb(px(16.)).child(format!(
                            "This message will be sent to {count} {} following this channel. You can edit or delete this message later, and it will update correspondingly in each following server.",
                            if count == 1 { "server" } else { "servers" }
                        )),
                    )
                    .child("Announcement stats can be found in Server Insights."),
                None => this.child(
                    "This message will be sent to all servers following this channel. You can edit or delete this message later, and it will update correspondingly in each following server.",
                ),
            })
            .into_any_element();
        Some(modal::render(
            modal::Spec {
                id: "publish-modal",
                title: "Are you sure?".into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    modal::button(
                        "publish-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| {
                            this.publish_confirm = None;
                        },
                    ),
                    modal::button(
                        "publish-confirm",
                        "Confirm",
                        modal::Tone::Primary,
                        move |this, _, cx| {
                            this.publish_confirm = None;
                            this.publish_message(channel, message);
                            cx.notify();
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.publish_confirm = None),
            },
            cx,
        ))
    }

    /// `crosspostMessage`: `POST /channels/<c>/messages/<m>/crosspost`.
    fn publish_message(&mut self, channel: Id, message: Id) {
        // Under `GRAVECORD_INERT=1` the request is refused and logged as `inert: refused Rest ...`.
        let _ = self.requests.send(Request::Rest {
            key: Some(CROSSPOST_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/messages/{message}/crosspost"),
            body: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_follow_success_lines_are_the_official_ones() {
        for key in FOLLOW_LINES {
            assert!(dc_intl::t(key).starts_with("What a nice addition to your server."));
        }
        assert!(dc_intl::t("2QbSea").starts_with("The most important updates"));
        assert_eq!(FOLLOW_ART_DARK.len(), FOLLOW_ART_LIGHT.len());
    }
}
