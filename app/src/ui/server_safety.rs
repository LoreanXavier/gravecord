//! Server Settings > Safety Setup, for a server without Community (module 989433 `tN`, `tE`, `tg`, `tf`, `tj`, `th`, `tm`;
//! option lists module 809505; pickers `eJ`, `eW`, `eX`).
//!
//! Every switch and option saves at once: most with `PATCH /guilds/{id}`, 2FA with `POST /guilds/{id}/mfa`. Not drawn:
//! Verification Role and moderator reports, which their experiments keep off here.
//! Measured in `design/dom/settings/guild-safety-darker-cozy.json`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::{Id, permissions};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Stateful, deferred, div,
    prelude::*, px,
};
use serde_json::{Value, json};

pub(super) const SAFETY_PREFIX: &str = "guild-safety:";
const SAVED_PREFIX: &str = "guild-safety:saved:";
pub(super) const RISKY_PREFIX: &str = "guild-safety:risky:";
/// Module 652215's `SAFE_DIRECT_MESSAGING` article.
pub(super) const CONTENT_ARTICLE: &str =
    "https://support.discord.com/hc/en-us/articles/115000068672";
const MEMBERS_FEATURE: &str = "ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY";
const PRUNE_FEATURE: &str = "PRUNE_REQUIRES_ADMIN";
const RAID_FEATURE: &str = "NON_COMMUNITY_RAID_ALERTS";
const COMMUNITY_ONLY: &str = "This option is not available for Community Servers.";
/// `iuRk2j`, its last sentence in `$b`.
const VERIFICATION_TEXT: &str = "Members of the server must meet the following criteria before they can send messages in text channels or initiate a direct message conversation. If a member has an assigned role and server onboarding is not enabled, this does not apply. We recommend setting a verification level for a Community Server.";

fn recommendation_range() -> std::ops::Range<usize> {
    VERIFICATION_TEXT.find("We recommend").unwrap_or(0)..VERIFICATION_TEXT.len()
}

/// One option of a picker (`w3`): its value, words and the mark at its left.
pub(super) struct Option3 {
    value: u64,
    title: &'static str,
    description: String,
    mark: Option<&'static str>,
    disabled: bool,
}

/// `vd` with its disabled options left out, as module 313003 asks for them.
pub(super) fn verification_choices() -> Vec<(u64, &'static str, String)> {
    verification_options(true)
        .into_iter()
        .filter(|option| !option.disabled)
        .map(|option| (option.value, option.title, option.description))
        .collect()
}

/// `vd`.
fn verification_options(community: bool) -> Vec<Option3> {
    vec![
        Option3 {
            value: 0,
            title: "None",
            description: if community {
                COMMUNITY_ONLY.to_owned()
            } else {
                "Unrestricted".to_owned()
            },
            mark: None,
            disabled: community,
        },
        Option3 {
            value: 1,
            title: "Low",
            description: "Must have a verified email on their Discord account.".to_owned(),
            mark: Some("--status-positive"),
            disabled: false,
        },
        Option3 {
            value: 2,
            title: "Medium",
            description: "Must also be registered on Discord for longer than 5 minutes.".to_owned(),
            mark: Some("--status-warning"),
            disabled: false,
        },
        Option3 {
            value: 3,
            title: "High",
            description: "Must also be a member of this server for longer than 10 minutes."
                .to_owned(),
            mark: Some("--orange-360"),
            disabled: false,
        },
        Option3 {
            value: 4,
            title: "Highest",
            description: "Must have a verified phone on their Discord account.".to_owned(),
            mark: Some("--border-feedback-critical"),
            disabled: false,
        },
    ]
}

/// `QL`.
pub(super) fn content_options(community: bool) -> Vec<Option3> {
    vec![
        Option3 {
            value: 2,
            title: "Filter messages from all members",
            description: "All messages will be filtered for sensitive image-based media."
                .to_owned(),
            mark: Some("--border-feedback-critical"),
            disabled: false,
        },
        Option3 {
            value: 1,
            title: "Filter messages from server members without roles",
            description: if community {
                COMMUNITY_ONLY.to_owned()
            } else {
                "Messages from server members without a role will be filtered for sensitive image-based media."
                    .to_owned()
            },
            mark: Some("--status-warning"),
            disabled: community,
        },
        Option3 {
            value: 0,
            title: "Do not filter",
            description: if community {
                COMMUNITY_ONLY.to_owned()
            } else {
                "Messages will not be filtered for sensitive image-based media.".to_owned()
            },
            mark: Some("--border-muted"),
            disabled: community,
        },
    ]
}

/// Adds or removes one feature in a copy of the list (`eZ`, `tx`, `jB`).
fn with_feature(features: &[String], feature: &str, on: bool) -> Vec<String> {
    let mut next: Vec<String> = features
        .iter()
        .filter(|known| *known != feature)
        .cloned()
        .collect();
    if on {
        next.push(feature.to_owned());
    }
    next
}

/// `e9`: whether Activity Alerts are on.
fn alerts_on(features: &[String]) -> bool {
    let has = |name: &str| features.iter().any(|feature| feature == name);
    if has("COMMUNITY") {
        !has("RAID_ALERTS_DISABLED")
    } else {
        has(RAID_FEATURE)
    }
}

/// What the page reads from the guild, which the state store does not keep.
#[derive(Default)]
pub(crate) struct SafetyState {
    pub(super) verification: u64,
    content: u64,
    pub(super) alerts_channel: Option<String>,
    pub(super) updates_channel: Option<String>,
    /// Which picker is open: 0 verification, 1 content.
    open_picker: Option<u8>,
    pub(super) select_open: bool,
    error: Option<String>,
    /// The Community page: which step, Activity Alerts opened, the two dialogs.
    pub(super) step: super::server_safety_community::SafetyStep,
    pub(super) raid_open: bool,
    pub(super) verify_dialog: Option<super::server_safety_community::VerifyDialog>,
    pub(super) risky_open: bool,
}

impl Root {
    pub(super) fn safety_mut(&mut self) -> Option<&mut SafetyState> {
        self.server_settings.as_mut().map(|open| &mut open.safety)
    }

    /// The filter level the guild has, for the AutoMod page's `th`.
    pub(super) fn safety_content(&self) -> u64 {
        self.server_settings
            .as_ref()
            .map_or(0, |open| open.safety.content)
    }

    pub(super) fn request_safety(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SAFETY_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route: format!("/guilds/{guild}"),
            body: None,
        });
    }

    /// `updateMFALevel`.
    pub(super) fn send_guild_mfa(&mut self, guild: Id, level: u64) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SAVED_PREFIX}{guild}")),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/mfa"),
            body: Some(json!({ "level": level })),
        });
    }

    pub(super) fn safety_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        if key.starts_with(RISKY_PREFIX) {
            // Module 868952 closes once the role is saved; the gateway updates the count.
            if report["ok"] == true {
                if let Some(state) = self.safety_mut() {
                    state.risky_open = false;
                }
            } else {
                self.put_app_toast(
                    "Something went wrong when enabling alerts. Please try again later.".to_owned(),
                    false,
                    cx,
                );
            }
            cx.notify();
            return;
        }
        let saved = key.starts_with(SAVED_PREFIX);
        // Module 313003: a saved level closes the dialog, a refused one says why in it.
        if saved {
            let ok = report["ok"] == true;
            let dialog = self.safety_mut().and_then(|state| {
                if ok {
                    state.verify_dialog = None;
                    return None;
                }
                state.verify_dialog.as_mut()
            });
            if let Some(dialog) = dialog {
                dialog.error = report["body"]["message"].as_str().map(str::to_owned);
                cx.notify();
                return;
            }
            // `tc`, `td`, `uq` on a Community server: a toast, as that page has no place for the words.
            let community = self.server_settings.as_ref().is_some_and(|open| {
                self.stores.guild(open.guild).is_some_and(|known| {
                    known.features.iter().any(|feature| feature == "COMMUNITY")
                })
            });
            if !ok && community {
                self.put_app_toast(
                    "Something went wrong when enabling alerts. Please try again later.".to_owned(),
                    false,
                    cx,
                );
            }
        }
        if let Some(state) = self.safety_mut() {
            if report["ok"] == true {
                let body = &report["body"];
                state.error = None;
                if !saved || body["verification_level"].is_u64() {
                    state.verification = body["verification_level"].as_u64().unwrap_or(0);
                    state.content = body["explicit_content_filter"].as_u64().unwrap_or(0);
                    state.alerts_channel =
                        body["safety_alerts_channel_id"].as_str().map(str::to_owned);
                    state.updates_channel = body["public_updates_channel_id"]
                        .as_str()
                        .map(str::to_owned);
                }
            } else if saved {
                state.error = Some(
                    report["body"]["message"]
                        .as_str()
                        .unwrap_or(
                            "Something went wrong when enabling alerts. Please try again later.",
                        )
                        .to_owned(),
                );
            }
        }
        cx.notify();
    }

    /// `saveGuild`.
    pub(super) fn save_safety(&mut self, guild: Id, body: Value) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SAVED_PREFIX}{guild}")),
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}"),
            body: Some(body),
        });
    }

    fn safety_features(&self, guild: Id) -> Vec<String> {
        self.stores
            .guild(guild)
            .map(|known| known.features.clone())
            .unwrap_or_default()
    }

    pub(super) fn toggle_safety_feature(&mut self, guild: Id, feature: &str, on: bool) {
        let features = with_feature(&self.safety_features(guild), feature, on);
        self.save_safety(guild, json!({ "features": features }));
    }

    /// A blue `inline-notice`.
    pub(super) fn safety_notice(body: AnyElement) -> AnyElement {
        div()
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--inlinenotice-border-info"))
            .bg(color("--background-feedback-mod-info"))
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .flex_none()
                    .child(icon("safety_info", 16., "--icon-feedback-info")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-strong"))
                    .child(body),
            )
            .into_any_element()
    }

    /// `el.d`: a label with its description, and the switch.
    fn safety_switch(
        id: &'static str,
        label: gpui::Div,
        description: &'static str,
        on: bool,
        disabled: bool,
    ) -> (gpui::Div, Stateful<gpui::Div>) {
        let row = div().flex().justify_between().child(
            div().w(px(580.)).child(label).child(
                div()
                    .mt(px(4.))
                    .w(px(572.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(description),
            ),
        );
        let switch = div()
            .id(id)
            .when(!disabled, |this| this.cursor_pointer())
            .child(if disabled {
                super::create_channel::switch_dimmed(on, "--background-base-lower")
            } else {
                super::create_channel::switch(on)
            });
        (row, switch)
    }

    fn safety_label(text: &'static str) -> gpui::Div {
        div()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(text)
    }

    fn safety_divider() -> gpui::Div {
        div().my(px(24.)).h(px(1.)).bg(color("--border-subtle"))
    }

    /// `eW`: one row of a picker, its mark down its left edge.
    fn safety_row(
        title: String,
        description: String,
        mark: Option<&'static str>,
        selected: bool,
        disabled: bool,
    ) -> gpui::Div {
        div()
            .h(px(62.))
            .relative()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(if selected {
                color("--background-surface-highest")
            } else {
                color("--background-surface-high")
            })
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| {
                this.hover(|style| style.bg(color("--background-surface-higher")))
            })
            .pl(px(14.))
            .pr(px(12.))
            .flex()
            .items_center()
            .children(mark.map(|mark| {
                div()
                    .absolute()
                    .left(px(-1.))
                    .top(px(-1.))
                    .bottom(px(-1.))
                    .w(px(3.))
                    .rounded_l(px(8.))
                    .bg(color(mark))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--interactive-text-active"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--interactive-text-default"))
                            .child(description),
                    ),
            )
    }

    /// `eJ`: the choice in a row with Change; open, every option under it.
    pub(super) fn safety_picker(
        &self,
        guild: Id,
        which: u8,
        change_title: &'static str,
        value: u64,
        options: Vec<Option3>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self
            .server_settings
            .as_ref()
            .is_some_and(|open| open.safety.open_picker == Some(which));
        let chosen = options.iter().find(|option| option.value == value);
        let (title, description, mark) = if open {
            (
                change_title.to_owned(),
                format!("({})", chosen.map_or("None", |option| option.title)),
                None,
            )
        } else {
            (
                chosen
                    .map_or(change_title, |option| option.title)
                    .to_owned(),
                chosen.map_or(String::new(), |option| option.description.clone()),
                chosen.and_then(|option| option.mark),
            )
        };
        let header = div()
            .id(SharedString::from(format!("safety-picker-{which}")))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(state) = this.safety_mut() {
                    state.open_picker = if state.open_picker == Some(which) {
                        None
                    } else {
                        Some(which)
                    };
                }
                cx.notify();
            }))
            .child(
                Self::safety_row(title, description, mark, false, false).child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-brand"))
                        .child("Change"),
                ),
            );
        let mut box_ = div()
            .rounded(px(4.))
            .bg(color("--background-mod-subtle"))
            .child(header);
        if open {
            let mut list = div().p(px(12.)).flex().flex_col().gap(px(8.));
            for option in options {
                let selected = option.value == value;
                let picked = option.value;
                let disabled = option.disabled;
                list = list.child(
                    div()
                        .id(SharedString::from(format!(
                            "safety-option-{which}-{picked}"
                        )))
                        .when(!disabled, |this| this.cursor_pointer())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if disabled || selected {
                                return;
                            }
                            if let Some(state) = this.safety_mut() {
                                if which == 0 {
                                    state.verification = picked;
                                } else {
                                    state.content = picked;
                                }
                                state.open_picker = None;
                            }
                            let body = if which == 0 {
                                json!({ "verification_level": picked })
                            } else {
                                json!({ "explicit_content_filter": picked })
                            };
                            this.save_safety(guild, body);
                            cx.notify();
                        }))
                        .child(
                            Self::safety_row(
                                option.title.to_owned(),
                                option.description,
                                option.mark,
                                selected,
                                disabled,
                            )
                            .child(icon(
                                if selected {
                                    "safety_radio_on"
                                } else {
                                    "safety_radio_off"
                                },
                                24.,
                                if selected {
                                    "--interactive-text-active"
                                } else {
                                    "--interactive-text-default"
                                },
                            )),
                        ),
                );
            }
            box_ = box_.child(list);
        }
        box_.into_any_element()
    }

    /// `tm`, `td`, `tc`: the switch and the channel it reports to.
    fn safety_alerts(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.server_settings.as_ref().map(|open| &open.safety) else {
            return div().into_any_element();
        };
        let selected = state
            .alerts_channel
            .clone()
            .or_else(|| state.updates_channel.clone());
        let features = self.safety_features(guild);
        let on = alerts_on(&features);
        let shown = selected
            .as_deref()
            .and_then(|channel| channel.parse::<Id>().ok())
            .and_then(|channel| {
                self.stores
                    .guild(guild)?
                    .channels
                    .iter()
                    .find(|known| known.id == channel)
                    .map(|known| format!("#{}", known.name))
            });
        let can_switch = on || selected.is_some();
        let opened = state.select_open;
        div()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .w(px(580.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child("Activity Alerts"),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child("Receive notifications for DM or join activity that exceeds usual numbers for your server. Each notification will contain information about the activity, including time period and approximate number of affected accounts."),
                            )
                            .when(selected.is_none(), |this| {
                                this.child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-subtle"))
                                        .child("Select a channel to turn on Activity Alerts."),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("safety-alerts-switch")
                            .when(!can_switch, |this| this.opacity(0.5))
                            .when(can_switch, |this| {
                                this.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_safety_feature(guild, RAID_FEATURE, !on);
                                    cx.notify();
                                }))
                            })
                            .child(super::create_channel::switch(on)),
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .pl(px(13.))
                    .child(Self::safety_label("Safety Notifications Channel").mb(px(8.)))
                    .child(
                        div()
                            .id("safety-channel")
                            .relative()
                            .child(self.anchor_probe("safety-channel"))
                            .w(px(384.))
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
                                    .child(shown.unwrap_or_else(|| "Select an option".to_owned())),
                            )
                            .child(
                                gpui::svg()
                                    .path("icons/select_chevron.svg")
                                    .size(px(24.))
                                    .text_color(color("--interactive-text-default"))
                                    .with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(if opened { 0.5 } else { 0. }),
                                    )),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(state) = this.safety_mut() {
                                    state.select_open = !state.select_open;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .w(px(384.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child("Anyone with access to this text channel will be able to see the notifications."),
                    ),
            )
            .into_any_element()
    }

    /// The open channel list (`maxOptionsVisible: 5`).
    pub(super) fn safety_select_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if open.page != super::server_settings::Page::SafetySetup || !open.safety.select_open {
            return None;
        }
        let guild = open.guild;
        let field = self
            .tip_anchors
            .borrow()
            .get("safety-channel")
            .copied()?
            .dilate(px(1.));
        let known = self.stores.guild(guild)?;
        let me = self.stores.me()?;
        let current = open.safety.alerts_channel.clone();
        let rows: Vec<AnyElement> = known
            .channels
            .iter()
            .filter(|channel| {
                channel.kind == dc_state::ChannelKind::Text
                    && permissions::can_in_channel(known, channel, me, permissions::SEND_MESSAGES)
            })
            .map(|channel| {
                let id = channel.id.to_string();
                let chosen = id.clone();
                let selected = current.as_deref() == Some(id.as_str());
                div()
                    .id(SharedString::from(format!("safety-channel-{id}")))
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
                            .child(format!("#{}", channel.name)),
                    )
                    .when(selected, |this| {
                        this.child(icon("select_check", 20., "--text-brand"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.safety_mut() {
                            state.alerts_channel = Some(chosen.clone());
                            state.select_open = false;
                        }
                        this.save_safety(guild, json!({ "safety_alerts_channel_id": chosen }));
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                div()
                    .id("safety-channel-layer")
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
                        if let Some(state) = this.safety_mut() {
                            state.select_open = false;
                        }
                        cx.notify();
                    }))
                    .child(div().flex().flex_col().children(rows)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The page (`tN`).
    pub(super) fn server_safety_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.server_settings.as_ref() else {
            return div().into_any_element();
        };
        let state = &open.safety;
        let (verification, content, error) =
            (state.verification, state.content, state.error.clone());
        let features = self.safety_features(guild);
        let has = |name: &str| features.iter().any(|feature| feature == name);
        let community = has("COMMUNITY");
        let members_on = has(MEMBERS_FEATURE);
        let prune_on = has(PRUNE_FEATURE);
        let known = self.stores.guild(guild);
        let me = self.stores.me();
        let can_manage = known.zip(me).is_some_and(|(known, me)| {
            permissions::can_in_guild(known, me, permissions::MANAGE_GUILD)
        });
        let owner = known
            .zip(me)
            .is_some_and(|(known, me)| known.owner_id == Some(me.id));
        let mfa_account = me.is_some_and(|me| me.mfa_enabled);
        let mfa_level = known.map_or(0, |known| known.mfa_level);
        let mut page = div().flex().flex_col().child(
            div()
                .mb(px(24.))
                .text_size(px(20.))
                .line_height(px(24.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child("Safety Setup"),
        );
        if !community {
            // `tE`.
            let badge = div()
                .flex()
                .items_center()
                .child(Self::safety_label("Show Members In Channel List"))
                .child(
                    div()
                        .ml(px(4.))
                        .h(px(16.))
                        .px(px(8.))
                        .rounded_full()
                        .bg(color("--control-primary-background-default"))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child("BETA"),
                );
            let (row, switch) = Self::safety_switch(
                "safety-members",
                badge,
                "Enabling this will show the members page in the channel list, allowing you to quickly see who's recently joined your server, and find any users flagged for unusual activity.",
                members_on,
                !can_manage,
            );
            page = page
                .child(row.child(switch.when(can_manage, |this| {
                    this.on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_safety_feature(guild, MEMBERS_FEATURE, !members_on);
                        cx.notify();
                    }))
                })))
                .child(
                    div().mt(px(12.)).child(Self::safety_notice(
                        "Becoming a community will automatically enable this experience for you."
                            .into_any_element(),
                    )),
                )
                .child(Self::safety_divider());
        }
        // `tg`.
        page = page
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Verification Level"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .mb(px(12.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(gpui::StyledText::new(VERIFICATION_TEXT).with_highlights([(
                        recommendation_range(),
                        gpui::HighlightStyle {
                            font_weight: Some(FontWeight(600.)),
                            ..Default::default()
                        },
                    )])),
            )
            .child(self.safety_picker(
                guild,
                0,
                "Change Verification Level",
                verification,
                verification_options(community),
                cx,
            ));
        // `tf`.
        if can_manage {
            // `disabled: !s` only marks a non-owner; an owner without 2FA gets the notice and a switch that does nothing.
            let changeable = owner && mfa_account;
            let (row, switch) = Self::safety_switch(
                "safety-mfa",
                Self::safety_label("Require 2FA for moderator actions"),
                "Moderators must have two-factor authentication enabled to ban, kick, or timeout members and delete messages. Only the server owner can change this setting if they have 2FA enabled.",
                mfa_level == 1,
                !owner,
            );
            page = page
                .child(Self::safety_divider())
                .child(row.child(switch.when(changeable, |this| {
                    this.on_click(cx.listener(move |this, _, _, cx| {
                        let level = u64::from(mfa_level != 1);
                        let _ = this.requests.send(Request::Rest {
                            key: Some(format!("{SAVED_PREFIX}{guild}")),
                            method: reqwest::Method::POST,
                            route: format!("/guilds/{guild}/mfa"),
                            body: Some(json!({ "level": level })),
                        });
                        cx.notify();
                    }))
                })));
            if owner && !mfa_account {
                let notice = self.text_with_styled_link(
                    "safety-mfa-link",
                    (
                        "In order to change this setting, you must have 2FA enabled on your account. ",
                        "Enable it here.",
                        "",
                    ),
                    gpui::HighlightStyle {
                        underline: Some(gpui::UnderlineStyle {
                            thickness: px(1.),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    cx,
                    |this, cx| this.open_settings(cx),
                );
                page = page.child(div().mt(px(12.)).child(Self::safety_notice(notice)));
            }
        }
        // `tj`.
        if !community && can_manage {
            let (row, switch) = Self::safety_switch(
                "safety-prune",
                Self::safety_label("Restrict member prune to admins"),
                "Only administrators and the server owner can prune inactive members when this setting is enabled.",
                prune_on,
                !owner,
            );
            page = page
                .child(Self::safety_divider())
                .child(row.child(switch.when(owner, |this| {
                    this.on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_safety_feature(guild, PRUNE_FEATURE, !prune_on);
                        cx.notify();
                    }))
                })));
        }
        // `th`.
        let learn_more = self.text_with_link(
            "safety-content-link",
            "Choose if server members can share image-based media detected by Discord\u{2019}s sensitive content filters. This setting will apply to channels that are not age-restricted. ",
            "Learn more",
            "",
            cx,
            |_, cx| cx.open_url(CONTENT_ARTICLE),
        );
        page = page
            .child(Self::safety_divider())
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Sensitive content filters"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .mb(px(12.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(learn_more),
            )
            .child(self.safety_picker(
                guild,
                1,
                "Change Explicit Content Filter Level",
                content,
                content_options(community),
                cx,
            ))
            .child(Self::safety_divider())
            .child(self.safety_alerts(guild, cx));
        if let Some(error) = error {
            page = page.child(
                div()
                    .mt(px(12.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error),
            );
        }
        page.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_toggle_in_a_copy() {
        let features = vec!["A".to_owned(), PRUNE_FEATURE.to_owned()];
        assert_eq!(with_feature(&features, PRUNE_FEATURE, false), ["A"]);
        assert_eq!(
            with_feature(&features, "B", true),
            ["A", PRUNE_FEATURE, "B"]
        );
        assert_eq!(with_feature(&features, PRUNE_FEATURE, true).len(), 2);
    }

    #[test]
    fn alerts_follow_the_feature_for_each_kind_of_server() {
        assert!(!alerts_on(&[]));
        assert!(alerts_on(&[RAID_FEATURE.to_owned()]));
        assert!(alerts_on(&["COMMUNITY".to_owned()]));
        assert!(!alerts_on(&[
            "COMMUNITY".to_owned(),
            "RAID_ALERTS_DISABLED".to_owned()
        ]));
    }

    #[test]
    fn community_servers_lose_the_loose_options() {
        assert!(verification_options(true)[0].disabled);
        assert!(!verification_options(false)[0].disabled);
        assert!(content_options(true)[2].disabled);
        assert_eq!(content_options(false)[1].value, 1);
    }

    #[test]
    fn the_strong_sentence_is_the_recommendation() {
        assert_eq!(
            &VERIFICATION_TEXT[recommendation_range()],
            "We recommend setting a verification level for a Community Server."
        );
    }
}
