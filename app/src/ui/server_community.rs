//! Server Settings > Enable Community (module 989433 `c1`, `c$`, `c0`) and its wizard (module 218595, steps module 347704,
//! constants module 124759).
//!
//! The wizard walks through safety settings, channels, default settings and the rules, then asks for everything in one
//! `PATCH /guilds/{id}`, after taking the risky permissions off `@everyone` when any are on.
//! Measured in `design/dom/settings/guild-community-darker-cozy.json`, `modals/community-1..5-darker-cozy.json`.

use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::ui::motion::Posed as _;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, deferred,
    div, img, linear_color_stop, linear_gradient, prelude::*, px,
};
use serde_json::{Value, json};

pub(super) const COMMUNITY_PREFIX: &str = "guild-community:";
const SAVED_PREFIX: &str = "guild-community:saved:";
const ROLE_PREFIX: &str = "guild-community:role:";
pub(super) const OVERVIEW_PREFIX: &str = "guild-overview:";
const OVERVIEW_SAVED_PREFIX: &str = "guild-overview:saved:";
const HOUSE_ART: &str = "https://discord.com/assets/fc0ec9c0404d2552.svg";
const SPARKLES_ART: &str = "https://discord.com/assets/2990c2614b7e7388.svg";
/// `graphic.src` of the intro step.
const CAMPFIRE_ART: &str = "https://cdn.discordapp.com/assets/content/3956ff6cb15230c8f21653fee2a8cea719aebeb9dceda42ac326d0e4bccff159.svg";
/// Module 652215's articles.
const TYPES_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/14078261239831";
const FEATURE_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360047132851";
const GUIDELINES_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360035969312";
/// What `formatOption` calls "Create one for me" on the wire: the server makes the channel.
const CREATE_CHANNEL: &str = "1";

/// `cQ`: name, picture, left and top inside the banner, faded.
const EXAMPLES: [(&str, &str, f32, f32, bool); 6] = [
    ("Pokemon GO Paris", "5e70ed2051446035.png", 100., 18., false),
    ("Python", "e2305dd90ed2d773.png", 428., 18., true),
    ("Learn Latin", "9cd9755971463bde.png", 50., 88., true),
    (
        "r/leagueoflegends",
        "302a27a2e5cc3fb6.svg",
        511.,
        88.,
        false,
    ),
    ("Sneaker Fans", "0923b872f4cfee13.png", 80., 158., false),
    ("Hogwarts School", "3e2d34d476b5a17c.png", 467., 158., true),
];

/// `dR`: the risky permissions, with the wizard's own names for three of them (`n` in `Y`).
const RISKY: [(u64, &str); 14] = [
    (8, "Administrator"),
    (2, "Kick Members"),
    (4, "Ban Members"),
    (32, "Manage Server"),
    (16, "Manage Channels"),
    (268435456, "Manage Roles"),
    (8192, "Manage Messages"),
    (1073741824, "Manage Emoji"),
    (8796093022208, "Create Emoji"),
    (8589934592, "Manage Events"),
    (17592186044416, "Create Events"),
    (1099511627776, "Moderate Members"),
    (131072, "Mention @everyone"),
    (9007199254740992, "Manage Official Messages"),
];

fn risky_mask() -> u64 {
    RISKY.iter().fold(0, |mask, (bit, _)| mask | bit)
}

/// `w`'s steps, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Intro,
    Safety,
    Basics,
    Finish,
    Rules,
}

/// The steps a server sees: those with nothing to change are left out.
fn steps_for(safety_met: bool, finish_met: bool) -> Vec<Step> {
    let mut steps = vec![Step::Intro];
    if !safety_met {
        steps.push(Step::Safety);
    }
    steps.push(Step::Basics);
    if !finish_met {
        steps.push(Step::Finish);
    }
    steps.push(Step::Rules);
    steps
}

/// The bar's share: a step's number over the numbered steps and one more.
fn progress(steps: &[Step], step: Step) -> Option<f32> {
    let numbered = steps.len().checked_sub(1)?;
    let at = steps.iter().position(|known| *known == step)?;
    (at > 0).then(|| at as f32 / (numbered + 1) as f32)
}

/// `onComplete`'s second part: what the guild's own request carries.
fn enable_body(
    features: &[String],
    rules_channel: Option<&str>,
    updates_channel: Option<&str>,
    verification_met: bool,
    content_met: bool,
    notifications_met: bool,
) -> Value {
    let mut all: Vec<&str> = features.iter().map(String::as_str).collect();
    if !all.contains(&"COMMUNITY") {
        all.push("COMMUNITY");
    }
    let mut body = json!({
        "features": all,
        "rules_channel_id": rules_channel,
        "public_updates_channel_id": updates_channel,
    });
    if !verification_met {
        body["verification_level"] = json!(1);
    }
    if !content_met {
        body["explicit_content_filter"] = json!(2);
    }
    if !notifications_met {
        body["default_message_notifications"] = json!(1);
    }
    body
}

/// The wizard while it is open, with what it read when it opened.
pub(crate) struct Wizard {
    step: Step,
    verification_met: bool,
    content_met: bool,
    notifications_met: bool,
    /// `I`: no risky permission is on `@everyone`.
    permissions_met: bool,
    everyone: u64,
    rules_channel: Option<String>,
    updates_channel: Option<String>,
    has_alerts_channel: bool,
    /// Which channel list is open: 0 rules, 1 updates.
    open_select: Option<u8>,
    accepted: bool,
    busy: bool,
    error: Option<String>,
}

impl Wizard {
    fn steps(&self) -> Vec<Step> {
        steps_for(
            self.verification_met && self.content_met,
            self.notifications_met && self.permissions_met,
        )
    }
}

impl Root {
    fn wizard_mut(&mut self) -> Option<&mut Wizard> {
        self.server_settings.as_mut()?.community.as_mut()
    }

    pub(super) fn request_community(&mut self, guild: Id) {
        let community = self
            .stores
            .guild(guild)
            .is_some_and(|known| known.features.iter().any(|feature| feature == "COMMUNITY"));
        if community {
            self.request_overview(guild);
        }
        for url in [HOUSE_ART, SPARKLES_ART, CAMPFIRE_ART] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        for (_, file, ..) in EXAMPLES {
            let url = format!("https://discord.com/assets/{file}");
            self.request_image(url.clone(), url);
        }
    }

    pub(super) fn community_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        if let Some(guild) = key
            .strip_prefix(COMMUNITY_PREFIX)
            .filter(|_| !key.starts_with(SAVED_PREFIX) && !key.starts_with(ROLE_PREFIX))
        {
            // `G`, `d`, `c`, `v`: read once when Enable Community opens.
            let Ok(guild) = guild.parse::<Id>() else {
                return;
            };
            if report["ok"] != true {
                return;
            }
            let body = &report["body"];
            let everyone = body["roles"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|role| role["id"].as_str() == Some(guild.to_string().as_str()))
                .and_then(|role| role["permissions"].as_str())
                .and_then(|bits| bits.parse::<u64>().ok())
                .unwrap_or(0);
            let updates = body["public_updates_channel_id"]
                .as_str()
                .map(str::to_owned);
            let alerts = !body["safety_alerts_channel_id"].is_null();
            let wizard = Wizard {
                step: Step::Intro,
                verification_met: body["verification_level"].as_u64().unwrap_or(0) > 0,
                content_met: body["explicit_content_filter"].as_u64() == Some(2),
                notifications_met: body["default_message_notifications"].as_u64() == Some(1),
                permissions_met: everyone & risky_mask() == 0,
                everyone,
                rules_channel: Some(CREATE_CHANNEL.to_owned()),
                updates_channel: updates,
                has_alerts_channel: alerts,
                open_select: None,
                accepted: false,
                busy: false,
                error: None,
            };
            if let Some(open) = self.server_settings.as_mut() {
                open.community = Some(wizard);
            }
        } else if report["ok"] == true {
            if let Some(guild) = key.strip_prefix(ROLE_PREFIX)
                && let Ok(guild) = guild.parse::<Id>()
            {
                self.send_community_enable(guild);
            } else if let Some(open) = self.server_settings.as_mut() {
                open.community = None;
            }
        } else if let Some(wizard) = self.wizard_mut() {
            wizard.busy = false;
            wizard.error = Some(
                report["body"]["message"]
                    .as_str()
                    .unwrap_or("Something went wrong.")
                    .to_owned(),
            );
        }
        cx.notify();
    }

    /// The landing page's button: the wizard reads the guild when it opens.
    fn open_community_wizard(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{COMMUNITY_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route: format!("/guilds/{guild}"),
            body: None,
        });
    }

    fn send_community_enable(&mut self, guild: Id) {
        let features = self
            .stores
            .guild(guild)
            .map(|known| known.features.clone())
            .unwrap_or_default();
        let Some(wizard) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.community.as_ref())
        else {
            return;
        };
        let body = enable_body(
            &features,
            wizard.rules_channel.as_deref(),
            wizard.updates_channel.as_deref(),
            wizard.verification_met,
            wizard.content_met,
            wizard.notifications_met,
        );
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SAVED_PREFIX}{guild}")),
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}"),
            body: Some(body),
        });
    }

    /// `onComplete`.
    fn finish_community(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(wizard) = self.wizard_mut() else {
            return;
        };
        if !wizard.accepted || wizard.busy {
            return;
        }
        wizard.busy = true;
        wizard.error = None;
        let (met, everyone) = (wizard.permissions_met, wizard.everyone);
        if met {
            self.send_community_enable(guild);
        } else {
            // The role goes first; the guild follows when it is saved.
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{ROLE_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}/roles/{guild}"),
                body: Some(json!({ "permissions": (everyone & !risky_mask()).to_string() })),
            });
        }
        cx.notify();
    }

    fn move_wizard(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some(wizard) = self.wizard_mut() {
            let steps = wizard.steps();
            let at = steps
                .iter()
                .position(|step| *step == wizard.step)
                .unwrap_or(0);
            let next = if forward {
                steps.get(at + 1)
            } else {
                at.checked_sub(1).and_then(|at| steps.get(at))
            };
            if let Some(next) = next {
                wizard.step = *next;
                wizard.open_select = None;
            }
        }
        cx.notify();
    }

    pub(super) fn close_community_wizard(&mut self) -> bool {
        if let Some(open) = self.server_settings.as_mut()
            && open.community.is_some()
        {
            open.community = None;
            return true;
        }
        false
    }

    /// `B`: a tick with what the step will turn on.
    fn wizard_check(title: AnyElement, description: &'static str) -> AnyElement {
        div()
            .flex()
            .items_start()
            .gap(px(8.))
            .child(div().mt(px(2.)).flex_none().child(icon(
                "community_check",
                16.,
                "--icon-feedback-positive",
            )))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child(description),
                    ),
            )
            .into_any_element()
    }

    /// `T`'s selects.
    fn wizard_select(
        &self,
        which: u8,
        (label, description, helper): (&'static str, &'static str, Option<String>),
        value: Option<&str>,
        guild: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let opened = self
            .server_settings
            .as_ref()
            .and_then(|open| open.community.as_ref())
            .is_some_and(|wizard| wizard.open_select == Some(which));
        let key: SharedString = format!("community-select-{which}").into();
        let shown = match value {
            None => "None".to_owned(),
            Some(CREATE_CHANNEL) => "Create one for me".to_owned(),
            Some(id) => id
                .parse::<Id>()
                .ok()
                .and_then(|id| {
                    self.stores
                        .guild(guild)?
                        .channels
                        .iter()
                        .find(|channel| channel.id == id)
                        .map(|channel| format!("#{}", channel.name))
                })
                .unwrap_or_else(|| id.to_owned()),
        };
        div()
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label),
            )
            .child(
                div()
                    .mt(px(4.))
                    .mb(px(8.))
                    // 430 in the capture; GPUI's advance is slightly wider, so the lines break as captured.
                    .w(px(434.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(description),
            )
            .child(
                div()
                    .id(key.clone())
                    .relative()
                    .child(self.anchor_probe(key))
                    .h(px(40.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if opened {
                        color("--text-brand")
                    } else {
                        color("--input-border-default")
                    })
                    .bg(color("--input-background-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(shown),
                    )
                    .child(
                        gpui::svg()
                            .path("icons/select_chevron.svg")
                            .size(px(20.))
                            .text_color(color("--interactive-text-default"))
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                if opened { 0.5 } else { 0. },
                            ))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(wizard) = this.wizard_mut() {
                            wizard.open_select = if wizard.open_select == Some(which) {
                                None
                            } else {
                                Some(which)
                            };
                        }
                        cx.notify();
                    })),
            )
            .children(helper.map(|helper| {
                div()
                    .mt(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-subtle"))
                    .child(helper)
            }))
            .into_any_element()
    }

    /// `G`, `R`: one panel of permissions, two to a row.
    fn wizard_permissions(
        title: &'static str,
        names: Vec<&'static str>,
        warning: bool,
    ) -> AnyElement {
        let rows: Vec<AnyElement> = names
            .into_iter()
            .map(|name| {
                div()
                    .w(px(179.))
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().size(px(6.)).rounded_full().bg(if warning {
                        gpui::rgb(0xfdb833)
                    } else {
                        color("--background-mod-muted")
                    }))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(if warning {
                                color("--text-default")
                            } else {
                                color("--text-muted")
                            })
                            .child(name),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .p(px(20.))
            .pt(px(16.))
            .rounded(px(4.))
            .bg(color("--background-base-lower"))
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_y(px(8.))
                    .gap_x(px(32.))
                    .children(rows),
            )
            .into_any_element()
    }

    /// One step of the wizard.
    pub(super) fn community_wizard_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let wizard = open.community.as_ref()?;
        let guild = open.guild;
        let steps = wizard.steps();
        let step = wizard.step;
        let close = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.close_community_wizard();
            cx.notify();
        });
        let close_scrim = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.close_community_wizard();
            cx.notify();
        });
        let button = |id: &'static str, text: &'static str, primary: bool, enabled: bool| {
            div()
                .id(id)
                .h(px(40.))
                // `min-width` of a medium button; only long text passes it.
                .when(!id.starts_with("community-intro"), |this| {
                    this.min_w(px(100.)).px(px(15.))
                })
                .rounded(px(8.))
                .border_1()
                .border_color(if primary {
                    color("--control-primary-border-default")
                } else {
                    color("--border-muted")
                })
                .bg(if primary {
                    color("--control-primary-background-default")
                } else {
                    color("--control-secondary-background-default")
                })
                .when(!enabled, |this| this.opacity(0.5))
                .when(enabled, |this| this.cursor_pointer())
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(if primary {
                    color("--white")
                } else {
                    color("--control-secondary-text-default")
                })
                .child(text)
        };
        let panel = div()
            .id("community-wizard")
            .occlude()
            .relative()
            .w(px(480.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }]);
        let closer = div()
            .id("community-close")
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .size(px(40.))
            .rounded(px(8.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
            .on_click(close);
        let content: AnyElement = if step == Step::Intro {
            let art = self.images.get(CAMPFIRE_ART).cloned();
            let start = button("community-intro-start", "Get Started", true, true)
                .flex_1()
                .on_click(cx.listener(|this, _, _, cx| this.move_wizard(true, cx)));
            let cancel = button("community-intro-cancel", "Cancel", false, true)
                .flex_1()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_community_wizard();
                    cx.notify();
                }));
            panel
                .pt(px(8.))
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .right(px(0.))
                        .h(px(190.))
                        .rounded_t(px(11.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(color("--expressive-gradient-green-end"), 0.),
                            linear_color_stop(
                                color("--expressive-gradient-green-end").opacity(0.),
                                1.,
                            ),
                        )),
                )
                .child(
                    div()
                        .relative()
                        .pt(px(16.))
                        .px(px(32.))
                        .pb(px(16.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .h(px(144.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(art.map(|art| img(art).w(px(162.)).h(px(108.)))),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(24.))
                                .line_height(px(32.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child("Enable Community"),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .text_center()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-subtle"))
                                .child("Community Servers are larger spaces built around shared interests. Enabling Community requires setting up safety settings, an Updates channel, and moderation settings."),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .px(px(32.))
                        .pt(px(16.))
                        .pb(px(32.))
                        .flex()
                        .gap(px(8.))
                        .child(cancel)
                        .child(start),
                )
                .child(closer)
                .into_any_element()
        } else {
            let (title, subtitle): (&'static str, Option<&'static str>) = match step {
                Step::Safety => (
                    "Keep your community safe",
                    Some(
                        "To ensure user safety, Community Servers must have the following moderation settings enabled.",
                    ),
                ),
                Step::Basics => (
                    "Set up the basics",
                    Some(
                        "Tell us which channel has your server rules and which channel we should send announcements to!",
                    ),
                ),
                Step::Finish => ("Update default settings", None),
                _ => ("One last step", None),
            };
            let share = progress(&steps, step).unwrap_or(0.);
            let body: AnyElement = match step {
                Step::Safety => {
                    let mut list = div().flex().flex_col().gap(px(12.));
                    if !wizard.verification_met {
                        list = list.child(Self::wizard_check(
                            "Verified email required".into_any_element(),
                            "To minimize spam, members of the server must have a verified email before they can send messages. This does not apply to members with roles.",
                        ));
                    }
                    if !wizard.content_met {
                        list = list.child(Self::wizard_check(
                            "Explicit Media Content Filter".into_any_element(),
                            "Discord will automatically scan and delete media sent in this server that contain explicit content (excludes age-restricted channels).",
                        ));
                    }
                    list.into_any_element()
                }
                Step::Basics => {
                    let helper = if wizard.updates_channel.is_none() && !wizard.has_alerts_channel {
                        "Optional Without a moderator channel, raid alerts and lockdown notices won't be delivered."
                    } else {
                        "Optional"
                    };
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(self.wizard_select(
                            0,
                            (
                                "Rules or Guidelines Channel",
                                "Optionally select a channel for your server rules or guidelines, or choose None.",
                                None,
                            ),
                            wizard.rules_channel.as_deref(),
                            guild,
                            cx,
                        ))
                        .child(self.wizard_select(
                            1,
                            (
                                "Community Updates Channel",
                                "This is the channel where Discord will send relevant updates for Community admins and moderators. As some info might be sensitive, we recommend selecting a role-restricted channel.",
                                Some(helper.to_owned()),
                            ),
                            wizard.updates_channel.as_deref(),
                            guild,
                            cx,
                        ))
                        .into_any_element()
                }
                Step::Finish => {
                    let enabled: Vec<&'static str> = RISKY
                        .iter()
                        .filter(|(bit, _)| wizard.everyone & bit == *bit)
                        .map(|(_, name)| *name)
                        .collect();
                    let disabled: Vec<&'static str> = RISKY
                        .iter()
                        .filter(|(bit, _)| wizard.everyone & bit != *bit)
                        .map(|(_, name)| *name)
                        .collect();
                    let all_off = disabled.len() == RISKY.len();
                    let mut list = div().flex().flex_col().gap(px(12.));
                    if !wizard.notifications_met {
                        list = list.child(Self::wizard_check(
                            "Set default settings to Mentions Only".into_any_element(),
                            "Because Community Servers tend to have a lot of people talking at once, defaulting Server Notifications for members to Mentions Only can help reduce members leaving. Members can always override this option.",
                        ));
                    }
                    if !wizard.permissions_met {
                        list = list.child(Self::wizard_check(
                            gpui::StyledText::new("Disable risky permissions for @everyone")
                                .with_highlights([(
                                    30..39,
                                    gpui::HighlightStyle {
                                        font_weight: Some(FontWeight(600.)),
                                        ..Default::default()
                                    },
                                )])
                                .into_any_element(),
                            "This will remove the following permissions from the @everyone role: Administrator, Manage Server, Manage Channels, Manage Roles, Kick Members, Ban Members, Manage Messages, and Mention @everyone, @here, and All Roles.",
                        ));
                        list = list.child(
                            div()
                                .mt(px(12.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .when(!enabled.is_empty(), |this| {
                                    this.child(Self::wizard_permissions(
                                        "Disabling risky permissions...",
                                        enabled,
                                        true,
                                    ))
                                })
                                .child(Self::wizard_permissions(
                                    if all_off {
                                        "All risky permissions disabled!"
                                    } else {
                                        "Currently Disabled"
                                    },
                                    disabled,
                                    false,
                                )),
                        );
                    }
                    list.into_any_element()
                }
                _ => {
                    let text = "By setting your server as a Community Server, you agree that your server complies with our Community Server Guidelines. Learn more about Community Servers.";
                    let accepted = wizard.accepted;
                    let links = gpui::InteractiveText::new(
                        "community-guidelines",
                        gpui::StyledText::new(text).with_highlights([
                            (
                                92..119,
                                gpui::HighlightStyle {
                                    color: Some(color("--text-link").into()),
                                    ..Default::default()
                                },
                            ),
                            (
                                136..153,
                                gpui::HighlightStyle {
                                    color: Some(color("--text-link").into()),
                                    ..Default::default()
                                },
                            ),
                        ]),
                    )
                    .on_click(vec![92..119, 136..153], |at, _, cx| {
                        cx.open_url(if at == 0 {
                            GUIDELINES_ARTICLE
                        } else {
                            TYPES_ARTICLE
                        });
                    });
                    div()
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child("Follow the rules"),
                        )
                        .child(
                            div()
                                .mt(px(4.))
                                .mb(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child(links),
                        )
                        .child(
                            div()
                                .id("community-accept")
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(super::server_games::checkbox(accepted))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child("I agree and understand"),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(wizard) = this.wizard_mut() {
                                        wizard.accepted = !wizard.accepted;
                                    }
                                    cx.notify();
                                })),
                        )
                        .into_any_element()
                }
            };
            let scrolling = step == Step::Finish && !wizard.permissions_met;
            let next_text = match step {
                Step::Basics => "Next",
                Step::Rules => "Finish Setup",
                _ => "Enable",
            };
            let can_go = step != Step::Rules || (wizard.accepted && !wizard.busy);
            let next = button("community-next", next_text, true, can_go).on_click(cx.listener(
                move |this, _, _, cx| {
                    if step == Step::Rules {
                        this.finish_community(guild, cx);
                    } else {
                        this.move_wizard(true, cx);
                    }
                },
            ));
            let error = wizard.error.clone();
            panel
                .pt(px(8.))
                .child(
                    div()
                        .pt(px(16.))
                        .px(px(24.))
                        .pb(px(8.))
                        .child(
                            div()
                                .w(px(120.))
                                .h(px(8.))
                                .rounded(px(4.))
                                .bg(color("--progressbar-track-background"))
                                .child(
                                    div()
                                        .h_full()
                                        .w(px(120. * share))
                                        .rounded(px(4.))
                                        .bg(color("--progressbar-indicator-background")),
                                ),
                        )
                        .child(
                            div()
                                .mt(px(24.))
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .children(subtitle.map(|subtitle| {
                            div()
                                .mt(px(8.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-subtle"))
                                .child(subtitle)
                        })),
                )
                .child(if scrolling {
                    div()
                        .mt(px(8.))
                        .h(px(414.))
                        .border_t_1()
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .child(
                            div()
                                .id("community-finish-scroller")
                                .h(px(412.))
                                .overflow_y_scroll()
                                .flex()
                                .items_start()
                                .child(
                                    div()
                                        .pt(px(12.))
                                        .pl(px(24.))
                                        .pb(px(12.))
                                        .w(px(454.))
                                        .flex_none()
                                        .child(body),
                                ),
                        )
                        .into_any_element()
                } else {
                    div()
                        .mt(px(8.))
                        .pt(px(8.))
                        .px(px(24.))
                        .child(body)
                        .into_any_element()
                })
                .children(error.map(|error| {
                    div()
                        .mt(px(8.))
                        .px(px(24.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-feedback-critical"))
                        .child(error)
                }))
                .child(
                    div()
                        .mt(px(8.))
                        .pt(px(16.))
                        .px(px(24.))
                        .pb(px(24.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .id("community-back")
                                .cursor_pointer()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child("Back")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.move_wizard(false, cx)),
                                ),
                        )
                        .child(next),
                )
                .child(closer)
                .into_any_element()
        };
        let pose = crate::ui::motion::modal_pose("community-wizard");
        Some(
            deferred(
                div()
                    .id("community-wizard-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(gpui::rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(close_scrim)
                    .child(div().posed(pose).child(content)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The Basics step's open list.
    pub(super) fn community_select_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let wizard = open.community.as_ref()?;
        let which = wizard.open_select?;
        let guild = open.guild;
        let field = self
            .tip_anchors
            .borrow()
            .get(format!("community-select-{which}").as_str())
            .copied()?
            .dilate(px(1.));
        let current = if which == 0 {
            wizard.rules_channel.clone()
        } else {
            wizard.updates_channel.clone()
        };
        let known = self.stores.guild(guild)?;
        let mut options: Vec<(Option<String>, String)> = vec![
            (None, "None".to_owned()),
            (
                Some(CREATE_CHANNEL.to_owned()),
                "Create one for me".to_owned(),
            ),
        ];
        options.extend(
            known
                .channels
                .iter()
                .filter(|channel| channel.kind == dc_state::ChannelKind::Text)
                .map(|channel| (Some(channel.id.to_string()), format!("#{}", channel.name))),
        );
        let rows: Vec<AnyElement> = options
            .into_iter()
            .enumerate()
            .map(|(index, (value, label))| {
                let selected = value == current;
                let chosen = value.clone();
                div()
                    .id(SharedString::from(format!(
                        "community-option-{which}-{index}"
                    )))
                    .flex_none()
                    .h(px(40.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .when(selected, |this| {
                        this.child(icon("select_check", 20., "--text-brand"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(wizard) = this.wizard_mut() {
                            if which == 0 {
                                wizard.rules_channel = chosen.clone();
                            } else {
                                wizard.updates_channel = chosen.clone();
                            }
                            wizard.open_select = None;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                div()
                    .id("community-select-layer")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(field.bottom() + px(4.))
                    .w(field.size.width)
                    .max_h(px(210.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(wizard) = this.wizard_mut() {
                            wizard.open_select = None;
                        }
                        cx.notify();
                    }))
                    .child(div().flex().flex_col().children(rows)),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// `c$`.
    fn community_card(
        glyph: &'static str,
        tint: gpui::Rgba,
        header: &'static str,
        body: AnyElement,
    ) -> AnyElement {
        div()
            .w(px(209.))
            .h(px(190.))
            .flex_none()
            .p(px(16.))
            .rounded(px(8.))
            .bg(color("--background-base-lower"))
            .child(
                div()
                    .size(px(40.))
                    .rounded_full()
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path(format!("icons/{glyph}.svg"))
                            .size(px(24.))
                            .text_color(tint),
                    ),
            )
            .child(
                div()
                    .mt(px(16.))
                    .mb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(header),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(body),
            )
            .into_any_element()
    }

    fn community_strong(text: &'static str, range: std::ops::Range<usize>) -> AnyElement {
        gpui::StyledText::new(text)
            .with_highlights([(
                range,
                gpui::HighlightStyle {
                    color: Some(color("--text-strong").into()),
                    font_weight: Some(FontWeight(600.)),
                    ..Default::default()
                },
            )])
            .into_any_element()
    }

    /// The page (`c1`).
    pub(super) fn server_community_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let known = self.stores.guild(guild);
        let admin = known.zip(self.stores.me()).is_some_and(|(known, me)| {
            dc_state::permissions::can_in_guild(known, me, dc_state::permissions::ADMINISTRATOR)
        });
        let picture = |url: &str| self.images.get(url).cloned();
        let examples: Vec<AnyElement> = EXAMPLES
            .iter()
            .map(|(name, file, left, top, faded)| {
                let art = picture(&format!("https://discord.com/assets/{file}"));
                div()
                    .absolute()
                    .left(px(*left))
                    .top(px(*top))
                    .when(*faded, |this| this.opacity(0.4))
                    .h(px(35.))
                    .pl(px(4.))
                    .pr(px(8.))
                    .rounded(px(4.))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(24.))
                            .flex_none()
                            .children(art.map(|art| img(art).size(px(24.)).rounded(px(4.)))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(*name),
                    )
                    .into_any_element()
            })
            .collect();
        let intro = self.text_with_link(
            "community-learn",
            "Convert to a Community Server to access additional administrative tools that help you moderate and grow your server. ",
            "Learn more.",
            "",
            cx,
            |_, cx| cx.open_url(TYPES_ARTICLE),
        );
        let discovery = self.text_with_link(
            "community-discovery",
            "Enabling Community does not make your server visible on Server Discovery. ",
            "Learn more here.",
            "",
            cx,
            |_, cx| cx.open_url(FEATURE_ARTICLE),
        );
        let start = div()
            .id("community-enable")
            .h(px(40.))
            .px(px(15.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-primary-border-default"))
            .bg(color("--control-primary-background-default"))
            .when(!admin, |this| this.opacity(0.5))
            .when(admin, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_community_wizard(guild);
                        cx.notify();
                    }))
            })
            .flex()
            .items_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .child("Enable Community");
        let button: AnyElement = if admin {
            start.into_any_element()
        } else {
            self.with_tip(
                "community-enable-tip",
                super::tooltip::Tip::text(
                    "Only server administrators are allowed to do this",
                    super::tooltip::Side::Top,
                ),
                start,
                cx,
            )
            .into_any_element()
        };
        div()
            .w(px(660.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .relative()
                    .w(px(660.))
                    .h(px(236.))
                    .mb(px(20.))
                    .children(examples)
                    .children(picture(HOUSE_ART).map(|art| {
                        img(art)
                            .absolute()
                            .left(px(202.))
                            .top(px(0.))
                            .w(px(256.))
                            .h(px(233.))
                    }))
                    .children(picture(SPARKLES_ART).map(|art| {
                        img(art)
                            .absolute()
                            .left(px(0.))
                            .top(px(0.))
                            .w(px(660.))
                            .h(px(214.))
                    })),
            )
            .child(
                div()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Are you building a Community?"),
            )
            .child(
                div()
                    .mt(px(8.))
                    .w(px(664.))
                    .text_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(intro),
            )
            .child(div().mt(px(16.)).child(button))
            .child(
                div()
                    .my(px(32.))
                    .w(px(660.))
                    .h(px(1.))
                    .bg(color("--text-strong"))
                    .opacity(0.06),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("Community Servers are larger spaces where people with shared interests can come together."),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(discovery),
            )
            .child(
                div()
                    .mt(px(32.))
                    .w(px(660.))
                    .flex()
                    .justify_between()
                    .child(Self::community_card(
                        "community_analytics",
                        color("--green-360"),
                        "Grow your community",
                        Self::community_strong(
                            "Apply to be in Server Discovery so more people can find your server directly on Discord.",
                            15..31,
                        ),
                    ))
                    .child(Self::community_card(
                        "community_analytics",
                        color("--platform-partner"),
                        "Keep members engaged",
                        Self::community_strong(
                            "Access tools like Server Insights that can better help you moderate and keep your server engaged.",
                            18..33,
                        ),
                    ))
                    .child(Self::community_card(
                        "community_info",
                        color("--red-360"),
                        "Stay informed",
                        "Get direct updates about new features built for communities from Discord."
                            .into_any_element(),
                    )),
            )
            .into_any_element()
    }
}

/// `c8`'s values, as read and as edited.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OverviewValues {
    rules: Option<String>,
    updates: Option<String>,
    alerts: Option<String>,
    locale: String,
    description: String,
    /// Disable Community was pressed: it waits for Save Changes like the rest.
    disabling: bool,
}

/// `c8` while it is open.
pub(crate) struct Overview {
    saved: OverviewValues,
    now: OverviewValues,
    description: Entity<Editor>,
    open_select: Option<OverviewSelect>,
    /// `dismissedCommunityFeaturesUpsell`.
    dismissed: bool,
    confirm_disable: bool,
    error: Option<String>,
    _events: Subscription,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverviewSelect {
    Rules,
    Updates,
    Alerts,
    Locale,
}

impl OverviewSelect {
    fn key(self) -> &'static str {
        match self {
            Self::Rules => "overview-rules",
            Self::Updates => "overview-updates",
            Self::Alerts => "overview-alerts",
            Self::Locale => "overview-locale",
        }
    }
}

/// What Save Changes sends: only what changed, and the features together with the channels they free.
fn overview_body(saved: &OverviewValues, now: &OverviewValues, features: &[String]) -> Value {
    let mut body = json!({});
    if saved.rules != now.rules || now.disabling {
        body["rules_channel_id"] = json!(now.rules);
    }
    if saved.updates != now.updates || now.disabling {
        body["public_updates_channel_id"] = json!(now.updates);
    }
    if saved.alerts != now.alerts {
        body["safety_alerts_channel_id"] = json!(now.alerts);
    }
    if saved.locale != now.locale {
        body["preferred_locale"] = json!(now.locale);
    }
    if saved.description != now.description {
        body["description"] = if now.description.is_empty() {
            Value::Null
        } else {
            json!(now.description)
        };
    }
    if now.disabling {
        let kept: Vec<&String> = features
            .iter()
            .filter(|feature| {
                !matches!(
                    feature.as_str(),
                    "COMMUNITY" | "DISCOVERABLE" | "PREVIEW_ENABLED"
                )
            })
            .collect();
        body["features"] = json!(kept);
    }
    body
}

/// `c5`: the language's own name, and plain English for `en-US`.
pub(super) fn locale_name(value: &str) -> String {
    if value == "en-US" {
        return "English".to_owned();
    }
    super::audit_spec::LOCALES
        .iter()
        .find(|(known, _)| *known == value)
        .map_or_else(|| value.to_owned(), |(_, name)| (*name).to_owned())
}

impl Root {
    fn overview_mut(&mut self) -> Option<&mut Overview> {
        self.server_settings.as_mut()?.overview.as_mut()
    }

    /// Escape: an open select of the Community page or its Disable Community confirmation, then a select of the
    /// wizard, close one at a time before the page does.
    pub(super) fn close_overview_layer(&mut self) -> bool {
        if self
            .wizard_mut()
            .is_some_and(|wizard| wizard.open_select.take().is_some())
        {
            return true;
        }
        self.overview_mut().is_some_and(|overview| {
            overview.open_select.take().is_some() || std::mem::take(&mut overview.confirm_disable)
        })
    }

    fn request_overview(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{OVERVIEW_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route: format!("/guilds/{guild}"),
            body: None,
        });
    }

    pub(super) fn overview_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        if key.starts_with(OVERVIEW_SAVED_PREFIX) {
            if report["ok"] == true {
                if let Some(guild) = key
                    .strip_prefix(OVERVIEW_SAVED_PREFIX)
                    .and_then(|guild| guild.parse::<Id>().ok())
                {
                    self.request_overview(guild);
                }
            } else if let Some(overview) = self.overview_mut() {
                overview.error = report["body"]["message"].as_str().map(str::to_owned);
            }
            cx.notify();
            return;
        }
        if report["ok"] != true {
            return;
        }
        let body = &report["body"];
        let text = |name: &str| body[name].as_str().map(str::to_owned);
        let values = OverviewValues {
            rules: text("rules_channel_id"),
            updates: text("public_updates_channel_id"),
            alerts: text("safety_alerts_channel_id"),
            locale: text("preferred_locale").unwrap_or_default(),
            description: text("description").unwrap_or_default(),
            disabling: false,
        };
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Tell the world a bit about this server.".to_owned());
            editor.set_text(&values.description);
            editor
        });
        let events = cx.subscribe(&editor, |this, editor, event: &EditorEvent, cx| {
            let text = editor.read(cx).text().to_owned();
            match event {
                EditorEvent::Submit(submitted) => {
                    let submitted = submitted.clone();
                    editor.update(cx, |editor, _| editor.set_text(&submitted));
                }
                EditorEvent::Edited => {
                    let cut: String = text.chars().take(300).collect();
                    if cut != text {
                        editor.update(cx, |editor, _| editor.set_text(&cut));
                    }
                    if let Some(overview) = this.overview_mut() {
                        overview.now.description = cut;
                    }
                }
                EditorEvent::Cancel => {}
            }
            cx.notify();
        });
        let dismissed = self
            .overview_mut()
            .is_some_and(|overview| overview.dismissed);
        if let Some(open) = self.server_settings.as_mut() {
            open.overview = Some(Overview {
                saved: values.clone(),
                now: values,
                description: editor,
                open_select: None,
                dismissed,
                confirm_disable: false,
                error: None,
                _events: events,
            });
        }
        cx.notify();
    }

    pub(super) fn overview_dirty(&self) -> bool {
        self.server_settings
            .as_ref()
            .and_then(|open| open.overview.as_ref())
            .is_some_and(|overview| overview.saved != overview.now)
    }

    pub(super) fn reset_overview(&mut self, cx: &mut Context<Self>) {
        let mut text = None;
        if let Some(overview) = self.overview_mut() {
            overview.now = overview.saved.clone();
            overview.error = None;
            text = Some((
                overview.description.clone(),
                overview.saved.description.clone(),
            ));
        }
        if let Some((editor, value)) = text {
            editor.update(cx, |editor, _| editor.set_text(&value));
        }
        cx.notify();
    }

    pub(super) fn save_overview(&mut self, guild: Id, cx: &mut Context<Self>) {
        let features = self
            .stores
            .guild(guild)
            .map(|known| known.features.clone())
            .unwrap_or_default();
        let Some(overview) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.overview.as_ref())
        else {
            return;
        };
        let body = overview_body(&overview.saved, &overview.now, &features);
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{OVERVIEW_SAVED_PREFIX}{guild}")),
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}"),
            body: Some(body),
        });
        cx.notify();
    }

    /// One `e2.Z` in its horizontal layout.
    fn overview_row(
        &self,
        which: OverviewSelect,
        (label, description, helper): (&'static str, &'static str, Option<String>),
        shown: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let opened = self
            .server_settings
            .as_ref()
            .and_then(|open| open.overview.as_ref())
            .is_some_and(|overview| overview.open_select == Some(which));
        let key = which.key();
        div()
            .flex()
            .gap(px(24.))
            .child(
                div()
                    .w(px(388.))
                    .flex_none()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(description),
                    ),
            )
            .child(
                div()
                    .w(px(248.))
                    .flex_none()
                    .child(
                        div()
                            .id(key)
                            .relative()
                            .child(self.anchor_probe(key))
                            .h(px(40.))
                            .pl(px(12.))
                            .pr(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if opened {
                                color("--text-brand")
                            } else {
                                color("--input-border-default")
                            })
                            .bg(color("--input-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(if shown.is_some() {
                                        color("--text-default")
                                    } else {
                                        color("--input-placeholder-text-default")
                                    })
                                    .child(shown.unwrap_or_else(|| "Select...".to_owned())),
                            )
                            .child(
                                gpui::svg()
                                    .path("icons/select_chevron.svg")
                                    .size(px(20.))
                                    .text_color(color("--interactive-text-default"))
                                    .with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(if opened { 0.5 } else { 0. }),
                                    )),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(overview) = this.overview_mut() {
                                    overview.open_select = if overview.open_select == Some(which) {
                                        None
                                    } else {
                                        Some(which)
                                    };
                                }
                                cx.notify();
                            })),
                    )
                    .children(helper.map(|helper| {
                        div()
                            .mt(px(8.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child(helper)
                    })),
            )
            .into_any_element()
    }

    fn overview_channel_name(&self, guild: Id, id: Option<&str>) -> Option<String> {
        let id = id?.parse::<Id>().ok()?;
        self.stores
            .guild(guild)?
            .channels
            .iter()
            .find(|channel| channel.id == id)
            .map(|channel| format!("#{}", channel.name))
    }

    /// `c7`: the notice with where to go next, until it is dismissed.
    fn overview_upsell(&self, guild: Id, cx: &mut Context<Self>) -> Option<AnyElement> {
        let overview = self.server_settings.as_ref()?.overview.as_ref()?;
        if overview.dismissed {
            return None;
        }
        let has = |name: &str| {
            self.stores
                .guild(guild)
                .is_some_and(|known| known.features.iter().any(|feature| feature == name))
        };
        let small = |id: &'static str, text: &'static str, primary: bool| {
            div()
                .id(id)
                .h(px(32.))
                .px(px(11.))
                .rounded(px(8.))
                .border_1()
                .border_color(if primary {
                    color("--control-primary-border-default")
                } else {
                    color("--control-secondary-border-default")
                })
                .bg(if primary {
                    color("--control-primary-background-default")
                } else {
                    color("--control-secondary-background-default")
                })
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(if primary {
                    color("--white")
                } else {
                    color("--control-secondary-text-default")
                })
                .child(text)
        };
        let go = |page: super::server_settings::Page| {
            cx.listener(move |this: &mut Root, _: &gpui::ClickEvent, _, cx| {
                if let Some(open) = this.server_settings.as_mut() {
                    open.page = page;
                }
                if let Some(guild) = this.server_settings.as_ref().map(|open| open.guild) {
                    this.request_server_page(guild, page, cx);
                }
                cx.notify();
            })
        };
        use super::server_settings::Page;
        Some(
            div()
                .relative()
                .p(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .child(
                    div()
                        .pr(px(8.))
                        .child(
                            div()
                                .mb(px(8.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child("Your server is now a Community Server!"),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child("You now have access to additional tools that can help you moderate, run, and grow your server. Check out some of them to get started!"),
                        ),
                )
                .child(
                    div()
                        .id("overview-upsell-close")
                        .absolute()
                        .top(px(22.))
                        .right(px(22.))
                        .cursor_pointer()
                        .child(icon("modal_close", 20., "--interactive-text-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(overview) = this.overview_mut() {
                                overview.dismissed = true;
                            }
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .mt(px(28.))
                        .flex()
                        .gap(px(8.))
                        .when(!has("GUILD_ONBOARDING"), |this| {
                            this.child(
                                small("overview-onboarding", "Set up Onboarding", true)
                                    .on_click(go(Page::Onboarding)),
                            )
                        })
                        .when(!has("DISCOVERABLE"), |this| {
                            this.child(
                                small("overview-discovery", "Apply for Discovery", false)
                                    .on_click(go(Page::Access)),
                            )
                        })
                        .child(
                            small("overview-insights", "Check Server Insights", false)
                                .on_click(go(Page::Insights)),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The page once Community is on (`c8`).
    pub(super) fn server_overview_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(overview) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.overview.as_ref())
        else {
            return div().into_any_element();
        };
        let now = overview.now.clone();
        let description = overview.description.clone();
        let error = overview.error.clone();
        let divider = || div().h(px(1.)).bg(color("--border-subtle"));
        let known = self.stores.guild(guild);
        let me = self.stores.me();
        let admin = known.zip(me).is_some_and(|(known, me)| {
            dc_state::permissions::can_in_guild(known, me, dc_state::permissions::ADMINISTRATOR)
        });
        let updates_helper = if now.updates.is_none() && now.alerts.is_none() {
            "Optional Without a moderator channel, raid alerts and lockdown notices won't be delivered."
        } else {
            "Optional"
        };
        let focus = description.clone();
        let upsell = self.overview_upsell(guild, cx);
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Community Settings"),
            )
            .children(upsell)
            .child(self.overview_row(
                OverviewSelect::Rules,
                (
                    "Rules or guidelines channel",
                    "Please select the channel that hosts your rules. This channel will by default start from the top and will feature a special header.",
                    None,
                ),
                Some(
                    self.overview_channel_name(guild, now.rules.as_deref())
                        .unwrap_or_else(|| "None".to_owned()),
                ),
                cx,
            ))
            .child(divider())
            .child(self.overview_row(
                OverviewSelect::Updates,
                (
                    "Community Updates Channel",
                    "This is the channel where Discord will send relevant updates for Community admins and moderators. As some info might be sensitive, we recommend selecting a role-restricted channel.",
                    Some(updates_helper.to_owned()),
                ),
                Some(
                    self.overview_channel_name(guild, now.updates.as_deref())
                        .unwrap_or_else(|| "None".to_owned()),
                ),
                cx,
            ))
            .child(divider())
            .child(self.overview_row(
                OverviewSelect::Alerts,
                (
                    "Safety Notifications Channel",
                    "This is the channel where Discord will send you important safety updates about your server. As some info might be sensitive, we recommend selecting a role-restricted channel.",
                    None,
                ),
                self.overview_channel_name(guild, now.alerts.as_deref()),
                cx,
            ))
            .child(divider())
            .child(self.overview_row(
                OverviewSelect::Locale,
                (
                    "Server Primary Language",
                    "This helps Discord customize features for you and your members.",
                    None,
                ),
                Some(locale_name(&now.locale)),
                cx,
            ))
            .child(divider())
            .child(
                div()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Server Description"),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .mb(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child("Describe your community. This description will be displayed in external embeds of this server\u{2019}s invite link."),
                    )
                    .child(
                        div()
                            .id("overview-description")
                            .h(px(89.))
                            .pt(px(12.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .cursor_text()
                            .on_click(cx.listener(move |_, _, window, cx| {
                                let handle = gpui::Focusable::focus_handle(focus.read(cx), cx);
                                window.focus(&handle, cx);
                            }))
                            .child(div().w_full().child(description)),
                    ),
            )
            .child(divider())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .w(px(516.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child("Disable Community"),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child("This will remove specific features for Community Servers, like Server Discovery and Server Insights."),
                            ),
                    )
                    .child(
                        div()
                            .id("overview-disable")
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-critical-primary-border-default"))
                            .bg(color("--control-critical-primary-background-default"))
                            .when(!admin, |this| this.opacity(0.5))
                            .when(admin, |this| {
                                this.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                                    let discoverable = this.stores.guild(guild).is_some_and(|known| {
                                        known.features.iter().any(|feature| feature == "DISCOVERABLE")
                                    });
                                    if let Some(overview) = this.overview_mut() {
                                        if discoverable {
                                            overview.confirm_disable = true;
                                        } else {
                                            overview.now.disabling = true;
                                            overview.now.rules = None;
                                            overview.now.updates = None;
                                        }
                                    }
                                    cx.notify();
                                }))
                            })
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child("Disable Community"),
                    ),
            )
            .children(error.map(|error| {
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error)
            }))
            .into_any_element()
    }

    /// The open list of `c8`.
    pub(super) fn overview_select_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if open.page != super::server_settings::Page::Community {
            return None;
        }
        let overview = open.overview.as_ref()?;
        let which = overview.open_select?;
        let field = self
            .tip_anchors
            .borrow()
            .get(which.key())
            .copied()?
            .dilate(px(1.));
        let known = self.stores.guild(open.guild)?;
        let channels = known
            .channels
            .iter()
            .filter(|channel| channel.kind == dc_state::ChannelKind::Text)
            .map(|channel| (channel.id.to_string(), format!("#{}", channel.name)));
        let (options, current): (Vec<(Option<String>, String)>, Option<String>) = match which {
            OverviewSelect::Rules | OverviewSelect::Updates => (
                std::iter::once((None, "None".to_owned()))
                    .chain(channels.map(|(id, name)| (Some(id), name)))
                    .collect(),
                if which == OverviewSelect::Rules {
                    overview.now.rules.clone()
                } else {
                    overview.now.updates.clone()
                },
            ),
            OverviewSelect::Alerts => (
                channels.map(|(id, name)| (Some(id), name)).collect(),
                overview.now.alerts.clone(),
            ),
            OverviewSelect::Locale => (
                super::audit_spec::LOCALES
                    .iter()
                    .map(|(value, _)| (Some((*value).to_owned()), locale_name(value)))
                    .collect(),
                Some(overview.now.locale.clone()),
            ),
        };
        let rows: Vec<AnyElement> = options
            .into_iter()
            .enumerate()
            .map(|(index, (value, label))| {
                let selected = value == current;
                let chosen = value.clone();
                div()
                    .id(SharedString::from(format!(
                        "overview-option-{}-{index}",
                        which.key()
                    )))
                    .flex_none()
                    .h(px(40.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .when(selected, |this| {
                        this.child(icon("select_check", 20., "--text-brand"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(overview) = this.overview_mut() {
                            match which {
                                OverviewSelect::Rules => overview.now.rules = chosen.clone(),
                                OverviewSelect::Updates => overview.now.updates = chosen.clone(),
                                OverviewSelect::Alerts => overview.now.alerts = chosen.clone(),
                                OverviewSelect::Locale => {
                                    overview.now.locale = chosen.clone().unwrap_or_default();
                                }
                            }
                            overview.open_select = None;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                div()
                    .id("overview-select-layer")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(field.bottom() + px(4.))
                    .w(field.size.width)
                    .max_h(px(210.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(overview) = this.overview_mut() {
                            overview.open_select = None;
                        }
                        cx.notify();
                    }))
                    .child(div().flex().flex_col().children(rows)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    pub(super) fn overview_confirm_open(&self) -> bool {
        self.server_settings
            .as_ref()
            .and_then(|open| open.overview.as_ref())
            .is_some_and(|overview| overview.confirm_disable)
    }

    /// The question before a discoverable server stops being a community.
    pub(super) fn overview_confirm_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let overview = self.server_settings.as_ref()?.overview.as_ref()?;
        if !overview.confirm_disable {
            return None;
        }
        let close = |this: &mut Root| {
            if let Some(overview) = this.overview_mut() {
                overview.confirm_disable = false;
            }
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "overview-disable-modal",
                title: "Are you sure?".into(),
                subtitle: Some(
                    "Disabling Community will also remove your server from Server Discovery."
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "overview-disable-cancel",
                        "Never Mind",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "overview-disable-confirm",
                        "Confirm",
                        super::modal::Tone::Critical,
                        |this, _, _| {
                            if let Some(overview) = this.overview_mut() {
                                overview.confirm_disable = false;
                                overview.now.disabling = true;
                                overview.now.rules = None;
                                overview.now.updates = None;
                            }
                        },
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_with_nothing_to_change_are_left_out() {
        assert_eq!(
            steps_for(false, false),
            vec![
                Step::Intro,
                Step::Safety,
                Step::Basics,
                Step::Finish,
                Step::Rules
            ]
        );
        assert_eq!(
            steps_for(true, true),
            vec![Step::Intro, Step::Basics, Step::Rules]
        );
    }

    #[test]
    fn the_bar_counts_the_numbered_steps_and_one_more() {
        let all = steps_for(false, false);
        assert_eq!(progress(&all, Step::Intro), None);
        assert_eq!(progress(&all, Step::Safety), Some(0.2));
        assert_eq!(progress(&all, Step::Basics), Some(0.4));
        assert_eq!(progress(&all, Step::Rules), Some(0.8));
    }

    #[test]
    fn enabling_asks_only_for_what_is_not_met() {
        let features = ["SOUNDBOARD".to_owned()];
        let body = enable_body(&features, Some("1"), None, false, false, false);
        assert_eq!(body["features"], json!(["SOUNDBOARD", "COMMUNITY"]));
        assert_eq!(body["rules_channel_id"], "1");
        assert!(body["public_updates_channel_id"].is_null());
        assert_eq!(body["verification_level"], 1);
        assert_eq!(body["explicit_content_filter"], 2);
        assert_eq!(body["default_message_notifications"], 1);
        let body = enable_body(&features, None, None, true, true, true);
        assert!(body.get("verification_level").is_none());
        assert!(body.get("explicit_content_filter").is_none());
        assert!(body.get("default_message_notifications").is_none());
    }

    #[test]
    fn saving_the_overview_sends_only_what_changed() {
        let saved = OverviewValues {
            rules: Some("1".to_owned()),
            locale: "en-US".to_owned(),
            ..Default::default()
        };
        let mut now = saved.clone();
        assert_eq!(overview_body(&saved, &now, &[]), json!({}));
        now.locale = "tr".to_owned();
        assert_eq!(
            overview_body(&saved, &now, &[]),
            json!({"preferred_locale": "tr"})
        );
        now = saved.clone();
        now.disabling = true;
        now.rules = None;
        let features = [
            "COMMUNITY".to_owned(),
            "NEWS".to_owned(),
            "DISCOVERABLE".to_owned(),
        ];
        let body = overview_body(&saved, &now, &features);
        assert_eq!(body["features"], json!(["NEWS"]));
        assert!(body["rules_channel_id"].is_null());
        assert!(body.get("public_updates_channel_id").is_some());
    }

    #[test]
    fn english_goes_by_its_plain_name() {
        assert_eq!(locale_name("en-US"), "English");
        assert_eq!(locale_name("en-GB"), "English, UK");
    }

    #[test]
    fn the_mask_takes_every_risky_permission() {
        assert_eq!(risky_mask() & (1 << 3), 1 << 3);
        assert_eq!(risky_mask() & (1 << 10), 0);
    }
}
