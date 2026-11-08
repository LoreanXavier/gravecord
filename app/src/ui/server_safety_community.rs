//! Server Settings > Safety Setup on a Community server (module 989433 `me`, overview `u9`, step list `u5`, rows `u0`,
//! `u3`, `uX`, `uq`, `u$`, `tu`, `td`, `tc`), the Verification Level dialog (module 313003) and Remove risky permissions
//! (module 868952). Measured in `design/dom/settings/guild-community-safety*`, `guild-safetyc-*`.
//!
//! Switches and the channel save with `PATCH /guilds/{id}`, 2FA with `POST /guilds/{id}/mfa`, the risky permissions with
//! `PATCH /guilds/{id}/roles/{id}` (only `permissions`, where the official `updateRole` repeats the role's other fields).
//!
//! Not drawn: `ep` (Report to Mods, needs `REPORT_TO_MOD_PILOT`), `eU` (Verification Role, behind its experiment), the
//! expand animation of Activity Alerts.

use super::modal::{self, Tone};
use super::server_settings::Page;
use super::tooltip::{Side, Tip};
use super::{Root, color, icon};
use crate::net::Request;
use dc_state::{Id, permissions};
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, SharedString, StyledText,
    deferred, div, prelude::*, px, rgba,
};
use serde_json::json;

const RAID_ART: &str = "https://discord.com/assets/22060f02ea963ef3.svg";
const DM_ART: &str = "https://discord.com/assets/d253b51d9c2ac233.svg";
const AUTOMOD_ART: &str = "https://discord.com/assets/afce11491007f19a.svg";
const PERMISSIONS_ART: &str = "https://discord.com/assets/b5d15eff2a0074e3.svg";
const DISABLED_FEATURE: &str = "RAID_ALERTS_DISABLED";
const PRUNE_FEATURE: &str = "PRUNE_REQUIRES_ADMIN";
const ON_BY_DEFAULT: &str = "This feature is on by default";
const VERIFY_INTRO: &str = "Members of the server must meet the following criteria before they can send messages in text channels or initiate a direct message conversation. If a member has an assigned role and server onboarding is not enabled, this does not apply. ";
const VERIFY_BOLD: &str = "We recommend setting a verification level for a Community Server.";
const MFA_TEXT: &str = "Moderators must have two-factor authentication enabled to ban, kick, or timeout members and delete messages. ";

/// `dR`, named as the dialog names them (`C`).
const RISKY: [(u64, &str); 14] = [
    (8, "Administrator"),
    (2, "Kick Members"),
    (4, "Ban Members"),
    (32, "Manage Server"),
    (16, "Manage Channels"),
    (268435456, "Manage Roles"),
    (8192, "Manage Messages"),
    (1073741824, "Manage Expressions"),
    (8796093022208, "Create Expressions"),
    (8589934592, "Manage Events"),
    (17592186044416, "Create Events"),
    (1099511627776, "Moderate Members"),
    (131072, "Mention @everyone"),
    (9007199254740992, "Manage Official Messages"),
];

/// `uW.C`: which page of the step list shows.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) enum SafetyStep {
    #[default]
    Overview,
    Raid,
    Dm,
    AutoMod,
    Permissions,
}

/// Module 313003's state.
pub(super) struct VerifyDialog {
    pub choice: u64,
    pub error: Option<String>,
}

/// `uQ`: the risky permissions `@everyone` has.
fn risky_enabled(everyone: u64) -> Vec<&'static str> {
    RISKY
        .iter()
        .filter(|(bit, _)| everyone & bit != 0)
        .map(|(_, name)| *name)
        .collect()
}

/// `r` in `u$`: taking one of them off `@everyone` would take it from the viewer too.
fn revokes_own(everyone: u64, others: u64, owner: bool) -> bool {
    !owner
        && RISKY.iter().any(|(bit, _)| {
            let total = (everyone & !bit) | others;
            total & permissions::ADMINISTRATOR == 0 && total & bit == 0
        })
}

/// `u5`'s Permissions entries that are on: 2FA, prune, no risky permission left.
fn permissions_enabled(mfa_level: u64, prune: bool, everyone: u64) -> usize {
    usize::from(mfa_level == 1)
        + usize::from(prune)
        + usize::from(risky_enabled(everyone).is_empty())
}

/// The list in the dialog's two content-width columns, read row by row.
fn two_columns<T: Copy>(items: &[T]) -> (Vec<T>, Vec<T>) {
    (
        items.iter().step_by(2).copied().collect(),
        items.iter().skip(1).step_by(2).copied().collect(),
    )
}

impl Root {
    pub(super) fn request_community_safety(&mut self, guild: Id, cx: &mut Context<Self>) {
        for url in [RAID_ART, DM_ART, AUTOMOD_ART, PERMISSIONS_ART] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        self.request_automod(guild, cx);
    }

    fn safety_step(&self) -> SafetyStep {
        self.server_settings
            .as_ref()
            .map_or(SafetyStep::Overview, |open| open.safety.step)
    }

    fn go_safety_step(&mut self, step: SafetyStep, cx: &mut Context<Self>) {
        if let Some(state) = self.safety_mut() {
            state.step = step;
            state.select_open = false;
        }
        cx.notify();
    }

    /// `@everyone`'s permissions as the state store knows them.
    fn everyone_permissions(&self, guild: Id) -> u64 {
        self.stores
            .guild(guild)
            .and_then(|known| known.role_permissions.get(&guild).copied())
            .unwrap_or(0)
    }

    /// `me`.
    pub(super) fn community_safety_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let body = match self.safety_step() {
            SafetyStep::Overview => return self.safety_overview(guild, cx),
            SafetyStep::Raid => self.safety_raid_step(guild, cx),
            SafetyStep::Dm => self.safety_dm_step(cx),
            SafetyStep::AutoMod => self.server_automod_page(guild, cx),
            SafetyStep::Permissions => self.safety_permissions_step(guild, cx),
        };
        div()
            .flex()
            .flex_col()
            .child(
                div().mb(px(32.)).child(
                    div()
                        .id("safety-back")
                        .h(px(24.))
                        .w_full()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--interactive-text-default"))
                        .child(
                            gpui::svg()
                                .path("icons/safety_back.svg")
                                .size(px(24.))
                                .text_color(color("--interactive-text-default"))
                                .with_transformation(gpui::Transformation::rotate(
                                    gpui::percentage(0.75),
                                )),
                        )
                        .child("Back")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.go_safety_step(SafetyStep::Overview, cx);
                        })),
                ),
            )
            .child(body)
            .into_any_element()
    }

    /// `u9`.
    fn safety_overview(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let known = self.stores.guild(guild);
        let has = |name: &str| {
            known.is_some_and(|known| known.features.iter().any(|feature| feature == name))
        };
        let raid_on = !has(DISABLED_FEATURE);
        let prune = has(PRUNE_FEATURE);
        let (enabled, total) = self.automod_counts();
        let can_manage = known.zip(self.stores.me()).is_some_and(|(known, me)| {
            permissions::can_in_guild(known, me, permissions::MANAGE_GUILD)
        });
        let perms = permissions_enabled(
            known.map_or(0, |known| known.mfa_level),
            prune,
            self.everyone_permissions(guild),
        );
        let count = |on: usize, all: usize| format!("{on} of {all} enabled");
        let mut steps: Vec<(&'static str, &'static str, String, SafetyStep)> = vec![
            (
                RAID_ART,
                "Raid Protection and CAPTCHA",
                count(2 + usize::from(raid_on), 3),
                SafetyStep::Raid,
            ),
            (
                DM_ART,
                "DM and Spam Protection",
                count(5, 5),
                SafetyStep::Dm,
            ),
        ];
        if can_manage {
            steps.push((
                AUTOMOD_ART,
                "AutoMod",
                count(enabled, total),
                SafetyStep::AutoMod,
            ));
        }
        steps.push((
            PERMISSIONS_ART,
            "Permissions",
            count(perms, 3),
            SafetyStep::Permissions,
        ));
        let mut list = div()
            .mt(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--card-background-default"));
        let last = steps.len() - 1;
        for (index, (art, title, subtitle, step)) in steps.into_iter().enumerate() {
            list = list.child(self.safety_step_row(index, (art, title, subtitle), step, cx));
            if index != last {
                list = list.child(div().mx(px(16.)).h(px(1.)).bg(color("--border-subtle")));
            }
        }
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Safety Setup"),
            )
            .child(list)
            .into_any_element()
    }

    /// `u4`.
    fn safety_step_row(
        &self,
        index: usize,
        (art, title, subtitle): (&'static str, &'static str, String),
        step: SafetyStep,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        div()
            .p(px(16.))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(40.))
                            .flex_none()
                            .rounded_full()
                            .overflow_hidden()
                            .children(
                                self.images
                                    .get(art)
                                    .cloned()
                                    .map(|image| gpui::img(image).size(px(40.))),
                            ),
                    )
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(subtitle),
                            ),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("safety-edit-{index}")))
                    .h(px(32.))
                    .min_w(px(60.))
                    .px(px(12.))
                    .justify_center()
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
                    .child("Edit")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.go_safety_step(step, cx);
                    })),
            )
    }

    fn safety_step_title(title: &'static str) -> gpui::Div {
        div()
            .mb(px(8.))
            .text_size(px(20.))
            .line_height(px(24.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child(title)
    }

    /// `simpleItemWrapper_NKrv-e`.
    fn safety_item() -> gpui::Div {
        div()
            .mb(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--card-background-default"))
            .p(px(16.))
            .flex()
            .gap(px(16.))
    }

    fn safety_item_title(title: &'static str) -> gpui::Div {
        div()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child(title)
    }

    fn safety_item_text() -> gpui::Div {
        div()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-default"))
    }

    fn safety_item_column() -> gpui::Div {
        div().flex_1().min_w(px(0.)).flex().flex_col().gap(px(4.))
    }

    /// `to`: the switch in its 72 wide column.
    fn safety_item_switch(on: bool, disabled: bool) -> gpui::Div {
        div()
            .w(px(72.))
            .flex_none()
            .flex()
            .justify_end()
            .child(if disabled {
                super::create_channel::switch_dimmed(on, "--card-background-default")
            } else {
                super::create_channel::switch(on)
            })
    }

    /// `u0`: a protection Discord keeps on.
    fn safety_default_item(
        &self,
        id: &'static str,
        title: &'static str,
        details: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        Self::safety_item()
            .child(
                Self::safety_item_column()
                    .child(Self::safety_item_title(title))
                    .children(details.map(|details| Self::safety_item_text().child(details))),
            )
            .child(self.with_tip(
                id,
                Tip::text(ON_BY_DEFAULT, Side::Top),
                Self::safety_item_switch(true, true),
                cx,
            ))
    }

    /// `tu`, `td`, `tc`.
    fn safety_raid_step(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.server_settings.as_ref().map(|open| &open.safety) else {
            return div().into_any_element();
        };
        let known = self.stores.guild(guild);
        let on = !known.is_some_and(|known| {
            known
                .features
                .iter()
                .any(|feature| feature == DISABLED_FEATURE)
        });
        let can_manage = known.zip(self.stores.me()).is_some_and(|(known, me)| {
            permissions::can_in_guild(known, me, permissions::MANAGE_GUILD)
        });
        let selected = state
            .alerts_channel
            .clone()
            .or_else(|| state.updates_channel.clone());
        let name = selected
            .as_deref()
            .and_then(|channel| channel.parse::<Id>().ok())
            .and_then(|channel| {
                known?
                    .channels
                    .iter()
                    .find(|known| known.id == channel)
                    .map(|known| known.name.clone())
            });
        let expanded = state.raid_open;
        let select_open = state.select_open;
        let disabled = !can_manage || (!on && selected.is_none());
        let header = div()
            .id("safety-raid-header")
            .p(px(16.))
            .cursor_pointer()
            .flex()
            .gap(px(16.))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.safety_mut() {
                    state.raid_open = !state.raid_open;
                    state.select_open = false;
                }
                cx.notify();
            }))
            .child(
                Self::safety_item_column()
                    .child(Self::safety_item_title("Activity Alerts"))
                    .child(Self::safety_item_text().child("Receive notifications for DM or join activity that exceeds usual numbers for your server. Each notification will contain information about the activity, including time period and approximate number of joins or DMs."))
                    .when(selected.is_none(), |this| {
                        this.child(
                            Self::safety_item_text()
                                .text_color(color("--text-subtle"))
                                .child("Select a channel to turn on Activity Alerts."),
                        )
                    })
                    .when(!expanded && selected.is_some(), |this| {
                        this.child(div().flex().child(Self::safety_pill(
                            "automod_channel_text",
                            12.,
                            name.clone().unwrap_or_default(),
                        )))
                    }),
            )
            .child(
                div()
                    .id("safety-raid-switch")
                    .when(!disabled, |this| this.cursor_pointer())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !disabled {
                            this.toggle_safety_feature(guild, DISABLED_FEATURE, on);
                        }
                        cx.notify();
                    }))
                    .child(Self::safety_item_switch(on, disabled)),
            );
        let body = expanded.then(|| {
            div().px(px(16.)).pb(px(16.)).child(
                div()
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .child(
                        div()
                            .mb(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Safety Notifications Channel"),
                    )
                    .child(
                        div()
                            .id("safety-channel")
                            .relative()
                            .child(self.anchor_probe("safety-channel"))
                            .h(px(40.))
                            .pl(px(12.))
                            .pr(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if select_open {
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
                                    .text_color(if name.is_some() {
                                        color("--text-default")
                                    } else {
                                        color("--input-placeholder-text-default")
                                    })
                                    .child(name.as_ref().map_or_else(
                                        || "Select an option".to_owned(),
                                        |name| format!("#{name}"),
                                    )),
                            )
                            .child(
                                gpui::svg()
                                    .path("icons/select_chevron.svg")
                                    .size(px(24.))
                                    .text_color(color("--interactive-text-default"))
                                    .with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(if select_open { 0.5 } else { 0. }),
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
                            .mt(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child("Anyone with access to this text channel will be able to see the notifications."),
                    ),
            )
        });
        div()
            .flex()
            .flex_col()
            .child(Self::safety_step_title("Raid Protection and CAPTCHA"))
            .child(
                div()
                    .mb(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--card-background-default"))
                    .child(header)
                    .children(body),
            )
            .child(self.safety_default_item(
                "safety-captcha-suspicious",
                "CAPTCHA suspicious accounts before they are able to join",
                None,
                cx,
            ))
            .child(self.safety_default_item(
                "safety-captcha-raid",
                "CAPTCHA all accounts before they can join during a suspected raid",
                None,
                cx,
            ))
            .into_any_element()
    }

    /// `valuePill_NKrv-e`.
    fn safety_pill(glyph: &'static str, size: f32, text: String) -> gpui::Div {
        div()
            .mt(px(4.))
            .py(px(4.))
            .px(px(8.))
            .rounded(px(16.))
            .bg(color("--background-mod-normal"))
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--interactive-text-default"))
            .child(icon(glyph, size, "--interactive-text-default"))
            .child(text)
    }

    /// `u3` and the rest of `u5`'s DM list.
    fn safety_dm_step(&self, cx: &mut Context<Self>) -> AnyElement {
        let level = self
            .server_settings
            .as_ref()
            .map_or(0, |open| open.safety.verification);
        let label = match level {
            1 => "Low",
            2 => "Medium",
            3 => "High",
            4 => "Highest",
            _ => "",
        };
        let glyph = if level == 4 {
            "safety_phone"
        } else {
            "access_envelope"
        };
        div()
            .flex()
            .flex_col()
            .child(Self::safety_step_title("DM and Spam Protection"))
            .child(
                Self::safety_item()
                    .id("safety-verification")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.safety_mut() {
                            state.verify_dialog = Some(VerifyDialog {
                                choice: level,
                                error: None,
                            });
                        }
                        cx.notify();
                    }))
                    .child(
                        Self::safety_item_column()
                            .child(Self::safety_item_title("Verification Level"))
                            .child(Self::safety_item_text().child("Members of the server must meet the following criteria before they can send messages. If a member has an assigned role and server onboarding is not enabled, this does not apply."))
                            .child(div().flex().child(Self::safety_pill(glyph, 16., label.to_owned()))),
                    )
                    .child(
                        gpui::svg()
                            .path("icons/chevron_right_20.svg")
                            .flex_none()
                            .size(px(24.))
                            .text_color(color("--interactive-text-default"))
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                0.75,
                            ))),
                    ),
            )
            .child(self.safety_default_item(
                "safety-hide-dms",
                "Hide DMs from suspicious users",
                Some("Send suspicious DMs to a separate Spam Inbox. Members can send a scam report or move it back to Inbox."),
                cx,
            ))
            .child(self.safety_default_item(
                "safety-filter-dms",
                "Filter DMs from unknown users",
                Some("Filter DMs from unknown users to Message Requests Inbox. Members must approve the request before they are able to respond."),
                cx,
            ))
            .child(self.safety_default_item(
                "safety-outbound",
                "Warn members before they visit outbound links",
                None,
                cx,
            ))
            .child(self.safety_default_item(
                "safety-spammers",
                "Hide all messages from and delete suspected spammers",
                None,
                cx,
            ))
            .into_any_element()
    }

    /// `uX`, `uq`, `u$`.
    fn safety_permissions_step(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
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
        let has = |name: &str| {
            known.is_some_and(|known| known.features.iter().any(|feature| feature == name))
        };
        let discoverable = has("DISCOVERABLE");
        let prune = has(PRUNE_FEATURE);
        let everyone = self.everyone_permissions(guild);
        let others = known.map_or(0, |known| {
            known.my_roles.iter().fold(0, |total, role| {
                total | known.role_permissions.get(role).copied().unwrap_or(0)
            })
        });
        let mut page = div()
            .flex()
            .flex_col()
            .child(Self::safety_step_title("Permissions"));
        if can_manage {
            let changeable = owner && mfa_account;
            let locked = !changeable || (mfa_level == 1 && discoverable);
            let text: AnyElement = if changeable {
                Self::safety_item_text().child(MFA_TEXT).into_any_element()
            } else if owner {
                Self::safety_item_text()
                    .child(self.text_with_styled_link(
                        "safety-community-mfa-link",
                        (
                            &format!(
                                "{MFA_TEXT}To change this setting, first enable 2FA on your account "
                            ),
                            "here",
                            ".",
                        ),
                        HighlightStyle {
                            color: Some(color("--text-link").into()),
                            ..Default::default()
                        },
                        cx,
                        |this, cx| this.open_settings(cx),
                    ))
                    .into_any_element()
            } else {
                Self::safety_item_text()
                    .child(format!(
                        "{MFA_TEXT}Only the server owner can change this setting if they have 2FA enabled."
                    ))
                    .into_any_element()
            };
            let reason = if discoverable {
                "2FA is required while this server is in Server Discovery"
            } else if owner {
                "To change this setting, first enable 2FA on your account."
            } else {
                "This setting can only be changed by a server owner with 2FA enabled on their account."
            };
            let switch = div()
                .id("safety-community-mfa")
                .when(!locked, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.send_guild_mfa(guild, u64::from(mfa_level != 1));
                            cx.notify();
                        }))
                })
                .child(Self::safety_item_switch(mfa_level == 1, locked));
            let switch: AnyElement = if locked {
                self.with_tip(
                    "safety-community-mfa-tip",
                    Tip::text(reason, Side::Top),
                    switch,
                    cx,
                )
                .into_any_element()
            } else {
                switch.into_any_element()
            };
            page = page.child(
                Self::safety_item()
                    .child(
                        Self::safety_item_column()
                            .child(Self::safety_item_title("Require 2FA for moderator actions"))
                            .child(text),
                    )
                    .child(switch),
            );
            page = page.child(
                Self::safety_item()
                    .child(
                        Self::safety_item_column()
                            .child(Self::safety_item_title("Restrict member prune to admins"))
                            .child(Self::safety_item_text().child("Only administrators and the server owner can prune inactive members when this setting is enabled.")),
                    )
                    .child(
                        div()
                            .id("safety-community-prune")
                            .when(owner, |this| {
                                this.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_safety_feature(guild, PRUNE_FEATURE, !prune);
                                    cx.notify();
                                }))
                            })
                            .child(Self::safety_item_switch(prune, !owner)),
                    ),
            );
        }
        let count = risky_enabled(everyone).len();
        let blocked = count > 0 && revokes_own(everyone, others, owner);
        let row = Self::safety_item()
            .id("safety-risky")
            .child(
                Self::safety_item_column()
                    .child(Self::safety_item_title(
                        "Remove risky permissions from @everyone",
                    ))
                    .child(Self::safety_item_text().child(format!(
                        "@everyone currently has {count} risky permissions enabled"
                    ))),
            )
            .child(if count > 0 {
                icon("safety_warning", 24., "--status-warning")
            } else {
                icon("circle_check", 24., "--status-positive")
            })
            .when(count > 0 && !blocked, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.safety_mut() {
                            state.risky_open = true;
                        }
                        cx.notify();
                    }))
            });
        page = page.child(if blocked {
            self.with_tip(
                "safety-risky-tip",
                Tip::text(
                    "Cannot disable risky permissions because this would revoke your own permissions.",
                    Side::Top,
                ),
                row,
                cx,
            )
            .into_any_element()
        } else {
            row.into_any_element()
        });
        page.into_any_element()
    }

    /// Module 313003.
    pub(super) fn safety_verify_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if open.page != Page::SafetySetup {
            return None;
        }
        let dialog = open.safety.verify_dialog.as_ref()?;
        let guild = open.guild;
        let choice = dialog.choice;
        let mut options = div().flex().flex_col().gap(px(16.)).mt(px(8.)).mb(px(4.));
        for (value, title, description) in super::server_safety::verification_choices() {
            options = options.child(
                div()
                    .id(SharedString::from(format!("safety-verify-{value}")))
                    .relative()
                    .pl(px(32.))
                    .cursor_pointer()
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .child(super::create_channel::radio(value == choice)),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(title),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(description),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = this
                            .safety_mut()
                            .and_then(|state| state.verify_dialog.as_mut())
                        {
                            dialog.choice = value;
                        }
                        cx.notify();
                    })),
            );
        }
        let text = format!("{VERIFY_INTRO}{VERIFY_BOLD}");
        let bold = VERIFY_INTRO.len()..text.len();
        let subtitle = StyledText::new(text).with_highlights(vec![(
            bold,
            HighlightStyle {
                font_weight: Some(FontWeight(600.)),
                ..Default::default()
            },
        )]);
        let error = dialog.error.clone();
        Some(modal::render(
            modal::Spec {
                id: "safety-verify-dialog",
                title: "Verification Level".into(),
                subtitle: Some(subtitle.into_any_element()),
                body: Some(
                    div()
                        .children(error.map(|error| {
                            div()
                                .mb(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-feedback-critical"))
                                .child(error)
                        }))
                        .child(options)
                        .into_any_element(),
                ),
                buttons: vec![
                    modal::button(
                        "safety-verify-cancel",
                        "Cancel",
                        Tone::Secondary,
                        |this, _, cx| {
                            if let Some(state) = this.safety_mut() {
                                state.verify_dialog = None;
                            }
                            cx.notify();
                        },
                    ),
                    modal::button(
                        "safety-verify-save",
                        "Save",
                        Tone::Primary,
                        move |this, _, cx| {
                            let choice = this
                                .safety_mut()
                                .and_then(|state| state.verify_dialog.as_ref())
                                .map_or(choice, |dialog| dialog.choice);
                            this.save_safety(guild, json!({ "verification_level": choice }));
                            cx.notify();
                        },
                    ),
                ],
                close: Box::new(|this, _, cx| {
                    if let Some(state) = this.safety_mut() {
                        state.verify_dialog = None;
                    }
                    cx.notify();
                }),
            },
            cx,
        ))
    }

    /// Module 868952.
    pub(super) fn safety_risky_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if open.page != Page::SafetySetup || !open.safety.risky_open {
            return None;
        }
        let guild = open.guild;
        let known = self.stores.guild(guild)?;
        let everyone = self.everyone_permissions(guild);
        let icon_image = known.icon.as_ref().and_then(|hash| {
            self.images
                .get(&format!(
                    "https://cdn.discordapp.com/icons/{guild}/{hash}.webp?size=240"
                ))
                .cloned()
        });
        let enabled = risky_enabled(everyone);
        let disabled: Vec<&'static str> = RISKY
            .iter()
            .filter(|(bit, _)| everyone & bit == 0)
            .map(|(_, name)| *name)
            .collect();
        let acronym: String = known
            .name
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .collect();
        let modal = div()
            .id("safety-risky-dialog")
            .occlude()
            .relative()
            .w(px(444.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-normal"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .absolute()
                    .top(px(-16.))
                    .left(px(161.))
                    .size(px(120.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(color("--background-mod-normal"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(40.))
                    .line_height(px(40.))
                    .text_color(color("--text-default"))
                    .when_some(icon_image.clone(), |this, image| {
                        this.child(gpui::img(image).size(px(120.)))
                    })
                    .when(icon_image.is_none(), |this| this.child(acronym)),
            )
            .child(
                div()
                    .mt(px(128.))
                    .px(px(46.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .items_center()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Remove risky permissions"),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child("With one easy click you can disable risky permissions for the @everyone role"),
                    ),
            )
            .child(
                div()
                    .mt(px(8.))
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(risky_table("Currently Enabled", &enabled, true))
                    .child(risky_table("Currently Disabled", &disabled, false)),
            )
            .child(
                div()
                    .py(px(16.))
                    .px(px(24.))
                    .flex()
                    .gap(px(8.))
                    .child(modal::footer_button(
                        modal::button(
                            "safety-risky-no",
                            "No, thanks",
                            Tone::Secondary,
                            |this, _, cx| {
                                if let Some(state) = this.safety_mut() {
                                    state.risky_open = false;
                                }
                                cx.notify();
                            },
                        ),
                        cx,
                    ))
                    .child(modal::footer_button(
                        modal::button(
                            "safety-risky-yes",
                            "Yes, change the default",
                            Tone::Primary,
                            move |this, _, cx| {
                                let now = this.everyone_permissions(guild);
                                let mask = RISKY.iter().fold(0, |mask, (bit, _)| mask | bit);
                                let _ = this.requests.send(Request::Rest {
                                    key: Some(format!(
                                        "{}{guild}",
                                        super::server_safety::RISKY_PREFIX
                                    )),
                                    method: reqwest::Method::PATCH,
                                    route: format!("/guilds/{guild}/roles/{guild}"),
                                    body: Some(json!({ "permissions": (now & !mask).to_string() })),
                                });
                                cx.notify();
                            },
                        ),
                        cx,
                    )),
            );
        Some(
            deferred(
                div()
                    .id("safety-risky-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.safety_mut() {
                            state.risky_open = false;
                        }
                        cx.notify();
                    }))
                    .child(modal),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Escape: the dialogs first.
    pub(super) fn close_safety_overlay(&mut self) -> bool {
        let Some(state) = self.safety_mut() else {
            return false;
        };
        if state.verify_dialog.take().is_some() {
            return true;
        }
        std::mem::take(&mut state.risky_open)
    }
}

/// `tableContainer_PaxA8v`: a heading and the names under it.
fn risky_table(header: &'static str, names: &[&'static str], warning: bool) -> gpui::Div {
    let (first, second) = two_columns(names);
    let column = |names: Vec<&'static str>| {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(names.into_iter().map(|name| {
                div()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().size(px(6.)).rounded_full().bg(if warning {
                        color("--status-warning")
                    } else {
                        color("--background-mod-normal")
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
            }))
    };
    div()
        .rounded(px(4.))
        .bg(color("--background-base-lower"))
        .px(px(21.))
        .py(px(17.))
        .child(
            div()
                .text_size(px(14.))
                .line_height(px(18.67))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(header),
        )
        .child(
            div()
                .mt(px(8.))
                .flex()
                .gap(px(32.))
                .child(column(first))
                .child(column(second)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENTION: u64 = 131072;
    const CREATE_EVENTS: u64 = 17592186044416;

    #[test]
    fn risky_lists_only_what_everyone_has() {
        assert_eq!(
            risky_enabled(MENTION | CREATE_EVENTS | 1024),
            ["Create Events", "Mention @everyone"]
        );
        assert_eq!(permissions_enabled(0, false, MENTION), 0);
        assert_eq!(permissions_enabled(1, true, 1024), 3);
    }

    #[test]
    fn taking_a_permission_only_blocks_when_it_is_the_viewers_too() {
        // The owner keeps everything.
        assert!(!revokes_own(MENTION, 0, true));
        // Without another source of the permission, the viewer would lose it.
        assert!(revokes_own(MENTION, 0, false));
        // An administrator by another role loses nothing.
        assert!(!revokes_own(MENTION, permissions::ADMINISTRATOR, false));
    }

    #[test]
    fn the_columns_read_row_by_row() {
        let (first, second) = two_columns(&["a", "b", "c", "d", "e"]);
        assert_eq!(first, ["a", "c", "e"]);
        assert_eq!(second, ["b", "d"]);
    }
}
