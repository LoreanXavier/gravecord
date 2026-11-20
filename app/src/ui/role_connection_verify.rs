//! Role Connection Verification modal (module 488358 `default`, `eA`, `eh`, `ex`, `ef`, `em`; requirement formatting
//! module 370480 `RP`; Linked Roles entry module 480900; styles module 219426 `BmAYlI`).
//!
//! Guides a member through connecting external accounts/applications to earn a Linked Role:
//! - Step 0 (`Verify`): shows grouped platform requirements (`GET /guilds/{guild}/roles/{role}/connections/eligibility`),
//!   passed/failed checks, disabled/deprecated/Steam hints, and `Finish` (`POST /guilds/{guild}/roles/{role}/connections`).
//! - Step 1 (`Connected`): shows `{platformName} connected!`, Connection Preview and Privacy switches.
//! - Step 2 (`Granted`): shows `Role granted!` and the channels unlocked by the role.

use super::create_channel::switch;
use super::tooltip::{Body, Side, Tip};
use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, IntoElement, MouseButton, SharedString, Window,
    div, prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) const ROLE_CONN_ELIGIBILITY_PREFIX: &str = "role-conn:eligibility:";
pub(super) const ROLE_CONN_ASSIGN_PREFIX: &str = "role-conn:assign:";

/// `so` of module 783419: comparison operators for role connection metadata rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operator {
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
}

impl Operator {
    pub(crate) fn from_code(code: u64) -> Option<Self> {
        match code {
            1 => Some(Self::LessThan),
            2 => Some(Self::GreaterThan),
            3 => Some(Self::Equal),
            4 => Some(Self::NotEqual),
            _ => None,
        }
    }
}

/// One eligibility check returned by `GET /guilds/{guild}/roles/{role}/connections/eligibility`.
#[derive(Clone, Debug)]
pub(crate) struct EligibilityCheck {
    pub connection_type: String,
    pub application_id: Option<String>,
    pub application_name: Option<String>,
    pub metadata_field: Option<String>,
    pub operator: Option<Operator>,
    pub value: Option<String>,
    pub description: Option<String>,
    pub result: bool,
    pub platform_disabled: bool,
    pub deprecated: bool,
}

/// Three steps of module 488358 (`A`: `0`, `1`, `2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Verify,
    Connected,
    Granted,
}

/// Privacy preview toggles on Step 1 (`ef` of module 488358).
#[derive(Clone, Debug)]
pub(crate) struct ConnectedPrivacy {
    pub platform_type: String,
    pub platform_name: String,
    pub account_name: String,
    pub visibility: bool,
    pub metadata_visibility: bool,
    pub show_activity: bool,
    pub friend_sync: bool,
    pub has_metadata: bool,
    pub supports_activity: bool,
    pub supports_friend_sync: bool,
}

pub(crate) struct RoleConnectionVerify {
    pub guild_id: Id,
    pub role_id: Id,
    pub role_name: String,
    pub role_color: u32,
    pub step: Step,
    pub loading: bool,
    pub assigning: bool,
    pub groups: Vec<Vec<EligibilityCheck>>,
    pub attempted: BTreeMap<String, bool>,
    pub connected: Option<ConnectedPrivacy>,
    focus: FocusHandle,
}

/// Friendly platform display name (matching `573648` / `settings/connections.rs`).
pub(super) fn platform_display_name(kind: &str) -> &'static str {
    match kind {
        "steam" => "Steam",
        "reddit" => "Reddit",
        "bluesky" => "Bluesky",
        "twitter" => "X",
        "paypal" => "PayPal",
        "ebay" => "eBay",
        "tiktok" => "TikTok",
        "github" => "GitHub",
        "twitch" => "Twitch",
        "youtube" => "YouTube",
        "spotify" => "Spotify",
        "xbox" => "Xbox",
        "playstation" => "PlayStation",
        "epicgames" => "Epic Games",
        "riotgames" => "Riot Games",
        "battlenet" => "Battle.net",
        "letterboxd" => "Letterboxd",
        _ => "Application",
    }
}

/// Formats a single role-connection requirement line (`ex` of module 488358 + `RP` of module 370480).
pub(crate) fn format_requirement(check: &EligibilityCheck) -> Option<String> {
    let raw_num = check
        .value
        .as_deref()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.)
        .round() as i64;
    if let Some(desc) = check.description.as_deref() {
        return Some(match check.operator {
            Some(Operator::LessThan) => dc_intl::format(
                "2p7dA3",
                &[
                    ("description", desc.into()),
                    ("count", (raw_num - 1).max(0).into()),
                ],
            ),
            Some(Operator::GreaterThan) => dc_intl::format(
                "2p7dA3",
                &[
                    ("description", desc.into()),
                    ("count", (raw_num + 1).max(0).into()),
                ],
            ),
            _ => desc.to_owned(),
        });
    }
    let op = check.operator?;
    let field = check.metadata_field.as_deref()?;
    let (op_key, adjusted) = match op {
        Operator::Equal => {
            let k = if check.connection_type == "paypal" && field == "verified" {
                "dcSDhW"
            } else {
                "0BlpbA"
            };
            (k, raw_num)
        }
        Operator::NotEqual => ("otcpTN", raw_num),
        Operator::LessThan => ("Ef35xs", (raw_num - 1).max(0)),
        Operator::GreaterThan => ("8W9OXU", (raw_num + 1).max(0)),
    };
    let quantity = match (check.connection_type.as_str(), field) {
        (_, "created_at") => dc_intl::format("TPbtEu", &[("days", adjusted.into())]),
        ("reddit", "total_karma") => dc_intl::format("P2JAEc", &[("karma", adjusted.into())]),
        ("reddit", "gold") => dc_intl::t("+/5TCx").to_owned(),
        ("reddit", "mod") => dc_intl::t("9rPbEs").to_owned(),
        ("steam", "game_count") => dc_intl::format("H9eLoe", &[("count", adjusted.into())]),
        ("steam", "item_count_tf2") => dc_intl::format("MceZJ6", &[("count", adjusted.into())]),
        ("steam", "item_count_dota2") => dc_intl::format("dMnRar", &[("count", adjusted.into())]),
        ("bluesky", "followers_count") => dc_intl::format("xIdKU8", &[("count", adjusted.into())]),
        ("bluesky", "statuses_count") => dc_intl::format("dy3+NR", &[("count", adjusted.into())]),
        ("letterboxd", "watches") => dc_intl::format("szM3Fw", &[("count", adjusted.into())]),
        ("letterboxd", "films_in_diary_this_year") => {
            dc_intl::format("6SIEpt", &[("count", adjusted.into())])
        }
        ("twitter", "verified") => dc_intl::t("xRygZL").to_owned(),
        ("twitter", "followers_count") => dc_intl::format("bkajar", &[("count", adjusted.into())]),
        ("twitter", "statuses_count") => dc_intl::format("MI7NKi", &[("count", adjusted.into())]),
        ("paypal", "verified") => dc_intl::t("slSQuB").to_owned(),
        ("ebay", "top_rated_seller") => dc_intl::t("TEEYwa").to_owned(),
        ("ebay", "positive_feedback_percentage") => {
            dc_intl::format("rl9Vgy", &[("value", adjusted.into())])
        }
        ("ebay", "unique_positive_feedback_count") => {
            dc_intl::format("QP5W1R", &[("count", adjusted.into())])
        }
        ("ebay", "unique_negative_feedback_count") => {
            dc_intl::format("6ZFYdK", &[("count", adjusted.into())])
        }
        ("tiktok", "verified") => dc_intl::t("uv7ety").to_owned(),
        ("tiktok", "follower_count") => dc_intl::format("qIPDRy", &[("count", adjusted.into())]),
        ("tiktok", "following_count") => dc_intl::format("zRta4X", &[("count", adjusted.into())]),
        ("tiktok", "likes_count") => dc_intl::format("ar0WW+", &[("count", adjusted.into())]),
        _ => return None,
    };
    let formatted: String = dc_intl::format_runs(
        op_key,
        &[("platformQuantityHook", quantity.as_str().into())],
    )
    .into_iter()
    .map(|(text, _)| text)
    .collect();
    Some(formatted)
}

fn parse_eligibility_groups(body: &Value) -> Vec<Vec<EligibilityCheck>> {
    body.as_array()
        .into_iter()
        .flatten()
        .map(|group| {
            group
                .as_array()
                .into_iter()
                .flatten()
                .map(|item| EligibilityCheck {
                    connection_type: item["connection_type"]
                        .as_str()
                        .unwrap_or("application")
                        .to_owned(),
                    application_id: item["application_id"].as_str().map(str::to_owned),
                    application_name: item["application"]["name"].as_str().map(str::to_owned),
                    metadata_field: item["connection_metadata_field"]
                        .as_str()
                        .map(str::to_owned),
                    operator: item["operator"].as_u64().and_then(Operator::from_code),
                    value: item["value"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| item["value"].as_i64().map(|n| n.to_string())),
                    description: item["description"].as_str().map(str::to_owned),
                    result: item["result"].as_bool().unwrap_or(false),
                    platform_disabled: item["disabled"].as_bool().unwrap_or(false),
                    deprecated: item["deprecated"].as_bool().unwrap_or(false),
                })
                .collect()
        })
        .collect()
}

impl Root {
    pub(super) fn role_verify_open(&self) -> bool {
        self.role_verify.is_some()
    }

    /// Opens the Role Connection Verification modal (`488358`) and fetches eligibility.
    pub(super) fn open_role_connection_verify(
        &mut self,
        guild_id: Id,
        role_id: Id,
        role_name: String,
        role_color: u32,
        cx: &mut Context<Self>,
    ) {
        self.role_verify = Some(RoleConnectionVerify {
            guild_id,
            role_id,
            role_name,
            role_color,
            step: Step::Verify,
            loading: true,
            assigning: false,
            groups: Vec::new(),
            attempted: BTreeMap::new(),
            connected: None,
            focus: cx.focus_handle(),
        });
        let _ = self.requests.send(Request::Fetch {
            key: format!("{ROLE_CONN_ELIGIBILITY_PREFIX}{guild_id}:{role_id}"),
            route: format!("/guilds/{guild_id}/roles/{role_id}/connections/eligibility"),
        });
        cx.notify();
    }

    pub(super) fn close_role_connection_verify(&mut self, cx: &mut Context<Self>) {
        self.role_verify = None;
        cx.notify();
    }

    pub(super) fn role_verify_key(
        &mut self,
        key: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.role_verify.is_none() {
            return false;
        }
        if key.key == "escape" {
            self.close_role_connection_verify(cx);
            return true;
        }
        false
    }

    pub(super) fn role_connection_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        if key.starts_with(ROLE_CONN_ELIGIBILITY_PREFIX) {
            let body = self.fetched.get(key).cloned().unwrap_or(Value::Null);
            let groups = parse_eligibility_groups(&body);
            if let Some(modal) = self.role_verify.as_mut() {
                let newly_connected = modal.connected.as_ref().is_some_and(|c| {
                    groups
                        .iter()
                        .flatten()
                        .any(|check| check.connection_type == c.platform_type && check.result)
                });
                modal.groups = groups;
                modal.loading = false;
                if newly_connected && modal.step == Step::Verify {
                    modal.step = Step::Connected;
                }
            }
            cx.notify();
            return;
        }
        if key.starts_with(ROLE_CONN_ASSIGN_PREFIX) {
            let answer = self.fetched.get(key).cloned().unwrap_or(Value::Null);
            if let Some(modal) = self.role_verify.as_mut() {
                modal.assigning = false;
                if answer["ok"].as_bool() == Some(true) {
                    modal.step = Step::Granted;
                }
            }
            cx.notify();
        }
    }

    /// Capture helper: `role-verify [step0|step1|step2]` opens the modal with sample requirements.
    #[cfg(feature = "capture")]
    pub(crate) fn script_role_connection_verify(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let step = if arg.contains("step1") {
            Step::Connected
        } else if arg.contains("step2") {
            Step::Granted
        } else {
            Step::Verify
        };
        let mut attempted = BTreeMap::new();
        attempted.insert("steam".to_owned(), true);
        self.role_verify = Some(RoleConnectionVerify {
            guild_id,
            role_id: 999123,
            role_name: "Verified Gamer".to_owned(),
            role_color: 0x57f287,
            step,
            loading: false,
            assigning: false,
            groups: vec![vec![
                EligibilityCheck {
                    connection_type: "steam".to_owned(),
                    application_id: None,
                    application_name: None,
                    metadata_field: Some("game_count".to_owned()),
                    operator: Some(Operator::GreaterThan),
                    value: Some("9".to_owned()),
                    description: None,
                    result: false,
                    platform_disabled: false,
                    deprecated: false,
                },
                EligibilityCheck {
                    connection_type: "github".to_owned(),
                    application_id: None,
                    application_name: None,
                    metadata_field: Some("created_at".to_owned()),
                    operator: Some(Operator::GreaterThan),
                    value: Some("29".to_owned()),
                    description: None,
                    result: true,
                    platform_disabled: false,
                    deprecated: false,
                },
            ]],
            attempted,
            connected: Some(ConnectedPrivacy {
                platform_type: "steam".to_owned(),
                platform_name: "Steam".to_owned(),
                account_name: "PlayerOne".to_owned(),
                visibility: true,
                metadata_visibility: true,
                show_activity: true,
                friend_sync: false,
                has_metadata: true,
                supports_activity: true,
                supports_friend_sync: true,
            }),
            focus: cx.focus_handle(),
        });
        cx.notify();
        format!("role-verify: opened {step:?}")
    }

    /// Renders the Role Connection Verification modal (`488358`).
    pub(super) fn role_verify_layer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.role_verify.as_ref()?;
        let viewport = window.viewport_size();
        let modal_w = (f32::from(viewport.width) - 64.).clamp(440., 560.);
        let guild_id = modal.guild_id;
        let role_id = modal.role_id;
        let eligible = !modal.groups.is_empty()
            && modal
                .groups
                .iter()
                .any(|g| !g.is_empty() && g.iter().all(|c| c.result));

        let (title, body, primary_label, primary_enabled, show_manage): (
            String,
            AnyElement,
            String,
            bool,
            bool,
        ) = match modal.step {
            Step::Verify => {
                let is_or = modal.groups.len() > 1;
                let is_single = modal.groups.len() == 1 && modal.groups[0].len() == 1;
                let intro_key = if is_single {
                    "jHfRvZ"
                } else if is_or {
                    "mOQ8k+"
                } else {
                    "U0olLg"
                };
                let intro =
                    dc_intl::format(intro_key, &[("roleName", modal.role_name.as_str().into())]);

                // Group flat checks by `connection_type[:application_id]` (`eA`).
                let mut grouped: BTreeMap<String, Vec<&EligibilityCheck>> = BTreeMap::new();
                for check in modal.groups.iter().flatten() {
                    let key = match &check.application_id {
                        Some(app_id) => format!("{}:{app_id}", check.connection_type),
                        None => check.connection_type.clone(),
                    };
                    grouped.entry(key).or_default().push(check);
                }
                let mut entries: Vec<(String, Vec<&EligibilityCheck>)> =
                    grouped.into_iter().collect();
                entries.sort_by_key(|(_, checks)| checks.iter().all(|c| c.result));

                let mut cards = div().mt(px(16.)).flex().flex_col().gap(px(12.));
                for (gkey, checks) in entries {
                    let first = checks[0];
                    let passed = checks.iter().all(|c| c.result);
                    let disabled = first.platform_disabled || first.deprecated;
                    let retried = !passed
                        && modal
                            .attempted
                            .get(&first.connection_type)
                            .copied()
                            .unwrap_or(false);
                    let name = first.application_name.clone().unwrap_or_else(|| {
                        platform_display_name(&first.connection_type).to_owned()
                    });
                    let info_tip = if first.deprecated {
                        Some(dc_intl::t!("Rzz1Xs", platformName = name.as_str()))
                    } else if first.connection_type == "steam" {
                        Some(dc_intl::t("NcZh6K").to_owned())
                    } else {
                        None
                    };
                    let conn_type = first.connection_type.clone();
                    let conn_name = name.clone();

                    let mut req_lines = div().flex().flex_col().gap(px(4.));
                    for c in &checks {
                        if let Some(line) = format_requirement(c) {
                            req_lines = req_lines.child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(if c.result {
                                        color("--text-default")
                                    } else {
                                        color("--text-feedback-critical")
                                    })
                                    .child(line),
                            );
                        }
                    }
                    if disabled && !passed && first.application_id.is_some() {
                        req_lines = req_lines.child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("+z5dYe")),
                        );
                    }

                    let trailing: AnyElement = if passed {
                        icon("checkmark_large", 20., "--text-feedback-positive").into_any_element()
                    } else if retried {
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-link"))
                            .child(dc_intl::t("5911Lb"))
                            .into_any_element()
                    } else if disabled {
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("cEts68"))
                            .into_any_element()
                    } else {
                        icon("chevron_right_20", 20., "--interactive-text-default")
                            .into_any_element()
                    };

                    let info_badge = info_tip.map(|tip_text| {
                        self.with_tip(
                            format!("role-verify-info-{gkey}"),
                            Tip::body(Body::Text(tip_text.into()), Side::Top),
                            icon("circle_info", 14., "--text-feedback-warning"),
                            cx,
                        )
                    });

                    cards = cards.child(
                        div()
                            .id(SharedString::from(format!("role-verify-card-{gkey}")))
                            .relative()
                            .px(px(16.))
                            .py(px(16.))
                            .rounded(px(12.))
                            .border_2()
                            .border_color(if passed {
                                color("--background-base-lowest")
                            } else {
                                color("--border-subtle")
                            })
                            .when(passed, |d| d.bg(color("--background-base-lowest")))
                            .when(disabled, |d| d.opacity(0.7))
                            .when(!passed && !disabled, |d| {
                                d.cursor_pointer()
                                    .hover(|s| s.border_color(color("--border-muted")))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let _ = this.requests.send(Request::Fetch {
                                            key: format!(
                                                "settings-connection-authorize-{conn_type}"
                                            ),
                                            route: format!("/connections/{conn_type}/authorize"),
                                        });
                                        if let Some(m) = this.role_verify.as_mut() {
                                            m.attempted.insert(conn_type.clone(), true);
                                            m.connected = Some(ConnectedPrivacy {
                                                platform_type: conn_type.clone(),
                                                platform_name: conn_name.clone(),
                                                account_name: conn_name.clone(),
                                                visibility: true,
                                                metadata_visibility: true,
                                                show_activity: true,
                                                friend_sync: false,
                                                has_metadata: true,
                                                supports_activity: true,
                                                supports_friend_sync: conn_type == "steam",
                                            });
                                        }
                                        cx.notify();
                                    }))
                            })
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .when(retried, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top(px(-9.))
                                        .right(px(16.))
                                        .px(px(8.))
                                        .py(px(2.))
                                        .rounded(px(12.))
                                        .bg(color("--background-feedback-critical"))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(gpui::rgb(0xffffff))
                                        .child(dc_intl::t("UB3hKo")),
                                )
                            })
                            .child(
                                div()
                                    .size(px(32.))
                                    .flex_none()
                                    .rounded(px(8.))
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(name.chars().next().unwrap_or('?').to_string()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(15.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-strong"))
                                                    .child(name),
                                            )
                                            .children(info_badge),
                                    )
                                    .child(req_lines),
                            )
                            .child(trailing),
                    );
                }

                let privacy_note = dc_intl::format(
                    "gsgvxh",
                    &[
                        ("privacyPolicyUrl", "https://discord.com/privacy".into()),
                        ("onAuthorizedApplicationsClick", "".into()),
                        ("onConnectionsClick", "".into()),
                    ],
                );
                let content = div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(intro),
                    )
                    .child(cards)
                    .child(
                        div()
                            .mt(px(14.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(privacy_note),
                    )
                    .into_any_element();

                (
                    dc_intl::t("zOZh3R").to_owned(),
                    content,
                    dc_intl::t("8SuVoE").to_owned(),
                    eligible && !modal.loading && !modal.assigning,
                    true,
                )
            }
            Step::Connected => {
                let conn = modal.connected.clone().unwrap_or(ConnectedPrivacy {
                    platform_type: "steam".to_owned(),
                    platform_name: "Steam".to_owned(),
                    account_name: "Account".to_owned(),
                    visibility: true,
                    metadata_visibility: true,
                    show_activity: true,
                    friend_sync: false,
                    has_metadata: true,
                    supports_activity: true,
                    supports_friend_sync: true,
                });
                let title = dc_intl::t!("yQvgBO", platformName = conn.platform_name.as_str());
                let privacy_note = dc_intl::format(
                    "gsgvxh",
                    &[
                        ("privacyPolicyUrl", "https://discord.com/privacy".into()),
                        ("onAuthorizedApplicationsClick", "".into()),
                        ("onConnectionsClick", "".into()),
                    ],
                );
                let mut toggles = div()
                    .mt(px(8.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("role-verify-toggle-visibility")
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("f7yOAX")),
                            )
                            .child(switch(conn.visibility))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(c) =
                                    this.role_verify.as_mut().and_then(|m| m.connected.as_mut())
                                {
                                    c.visibility = !c.visibility;
                                }
                                cx.notify();
                            })),
                    );
                if conn.has_metadata {
                    toggles = toggles.child(
                        div()
                            .id("role-verify-toggle-meta")
                            .cursor_pointer()
                            .when(!conn.visibility, |d| d.opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("FYKGsL")),
                            )
                            .child(switch(conn.metadata_visibility))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(c) =
                                    this.role_verify.as_mut().and_then(|m| m.connected.as_mut())
                                    && c.visibility
                                {
                                    c.metadata_visibility = !c.metadata_visibility;
                                }
                                cx.notify();
                            })),
                    );
                }
                if conn.supports_activity {
                    toggles = toggles.child(
                        div()
                            .id("role-verify-toggle-activity")
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t!(
                                        "6u6J0q",
                                        platform = conn.platform_name.as_str()
                                    )),
                            )
                            .child(switch(conn.show_activity))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(c) =
                                    this.role_verify.as_mut().and_then(|m| m.connected.as_mut())
                                {
                                    c.show_activity = !c.show_activity;
                                }
                                cx.notify();
                            })),
                    );
                }
                if conn.supports_friend_sync {
                    toggles = toggles.child(
                        div()
                            .id("role-verify-toggle-friends")
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("+KCMSi")),
                            )
                            .child(switch(conn.friend_sync))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(c) =
                                    this.role_verify.as_mut().and_then(|m| m.connected.as_mut())
                                {
                                    c.friend_sync = !c.friend_sync;
                                }
                                cx.notify();
                            })),
                    );
                }

                let preview_info = self.with_tip(
                    "role-verify-preview-info".to_owned(),
                    Tip::body(Body::Text(dc_intl::t("x3svVc").into()), Side::Top),
                    icon("circle_info", 14., "--icon-muted"),
                    cx,
                );

                let content = div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(privacy_note),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(20.))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("TOjkEg"))
                                            .child(preview_info),
                                    )
                                    .child(
                                        div()
                                            .mt(px(8.))
                                            .p(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-base-lowest"))
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(conn.account_name),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .child(if conn.platform_name.is_empty() {
                                                        conn.platform_type.clone()
                                                    } else {
                                                        conn.platform_name
                                                    }),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("jndPhX")),
                                    )
                                    .child(toggles),
                            ),
                    )
                    .into_any_element();

                (title, content, dc_intl::t("i4jeWR").to_owned(), true, false)
            }
            Step::Granted => {
                let channels: Vec<String> = self
                    .stores
                    .guild(guild_id)
                    .map(|g| {
                        g.channels
                            .iter()
                            .filter(|c| matches!(c.kind, dc_state::ChannelKind::Text))
                            .take(5)
                            .map(|c| c.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let mut ch_list = div()
                    .p(px(14.))
                    .rounded_b(px(6.))
                    .bg(color("--background-mod-normal"))
                    .flex()
                    .flex_col()
                    .gap(px(6.));
                for ch_name in channels {
                    ch_list = ch_list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-muted"))
                                    .child("#"),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child(ch_name),
                            ),
                    );
                }

                let content = div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .p(px(12.))
                            .rounded_t(px(6.))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().size(px(16.)).rounded_full().bg(gpui::rgb(
                                if modal.role_color == 0 {
                                    0x5865f2
                                } else {
                                    modal.role_color
                                },
                            )))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(modal.role_name.clone()),
                            ),
                    )
                    .child(ch_list)
                    .into_any_element();

                (
                    dc_intl::t("najNdz").to_owned(),
                    content,
                    dc_intl::t("cpT0Cq").to_owned(),
                    true,
                    true,
                )
            }
        };

        let step = modal.step;
        Some(
            div()
                .id("role-verify-backdrop")
                .track_focus(&modal.focus)
                .absolute()
                .inset_0()
                .bg(gpui::rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_role_connection_verify(cx)),
                )
                .child(
                    div()
                        .id("role-verify-modal")
                        .w(px(modal_w))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow_2xl()
                        .flex()
                        .flex_col()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px(px(24.))
                                .pt(px(20.))
                                .pb(px(12.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .line_height(px(24.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .id("role-verify-close")
                                        .size(px(32.))
                                        .rounded(px(8.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("modal_close", 18., "--interactive-icon-default"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_role_connection_verify(cx)
                                        })),
                                ),
                        )
                        .child(div().px(px(24.)).pb(px(20.)).child(body))
                        .child(
                            div()
                                .px(px(24.))
                                .py(px(16.))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-higher"))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(if show_manage {
                                    div()
                                        .id("role-verify-manage-connections")
                                        .cursor_pointer()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-link"))
                                        .child(dc_intl::t("VXV55P"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_role_connection_verify(cx);
                                            this.open_settings_page("connected-apps", cx);
                                        }))
                                        .into_any_element()
                                } else {
                                    div().into_any_element()
                                })
                                .child(
                                    div()
                                        .id("role-verify-primary")
                                        .h(px(38.))
                                        .px(px(18.))
                                        .rounded(px(8.))
                                        .bg(color("--brand-500"))
                                        .when(primary_enabled, |d| {
                                            d.cursor_pointer()
                                                .hover(|s| s.bg(color("--brand-560")))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    match step {
                                                        Step::Verify => {
                                                            if let Some(m) = this.role_verify.as_mut() {
                                                                m.assigning = true;
                                                            }
                                                            let _ = this.requests.send(Request::Rest {
                                                                key: Some(format!(
                                                                    "{ROLE_CONN_ASSIGN_PREFIX}{guild_id}:{role_id}"
                                                                )),
                                                                method: reqwest::Method::POST,
                                                                route: format!(
                                                                    "/guilds/{guild_id}/roles/{role_id}/connections"
                                                                ),
                                                                body: Some(json!({})),
                                                            });
                                                            cx.notify();
                                                        }
                                                        Step::Connected => {
                                                            if let Some(m) = this.role_verify.as_mut() {
                                                                m.step = Step::Verify;
                                                            }
                                                            cx.notify();
                                                        }
                                                        Step::Granted => {
                                                            this.close_role_connection_verify(cx);
                                                        }
                                                    }
                                                }))
                                        })
                                        .when(!primary_enabled, |d| d.opacity(0.5))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(gpui::rgb(0xffffff))
                                        .child(primary_label),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirement_formatter_handles_all_operators_and_platforms() {
        let steam = EligibilityCheck {
            connection_type: "steam".into(),
            application_id: None,
            application_name: None,
            metadata_field: Some("game_count".into()),
            operator: Some(Operator::GreaterThan),
            value: Some("9".into()),
            description: None,
            result: false,
            platform_disabled: false,
            deprecated: false,
        };
        assert_eq!(
            format_requirement(&steam).as_deref(),
            Some("Must have at least 10 games in library")
        );

        let paypal = EligibilityCheck {
            connection_type: "paypal".into(),
            application_id: None,
            application_name: None,
            metadata_field: Some("verified".into()),
            operator: Some(Operator::Equal),
            value: Some("1".into()),
            description: None,
            result: true,
            platform_disabled: false,
            deprecated: false,
        };
        assert_eq!(
            format_requirement(&paypal).as_deref(),
            Some("Must have a verified bank account")
        );

        let custom = EligibilityCheck {
            connection_type: "application".into(),
            application_id: Some("123".into()),
            application_name: Some("Bot".into()),
            metadata_field: Some("level".into()),
            operator: Some(Operator::LessThan),
            value: Some("10".into()),
            description: Some("Max rank".into()),
            result: true,
            platform_disabled: false,
            deprecated: false,
        };
        assert_eq!(format_requirement(&custom).as_deref(), Some("Max rank - 9"));
    }

    #[test]
    fn all_role_connection_verify_strings_resolve() {
        for key in [
            "zOZh3R", "8SuVoE", "VXV55P", "TOjkEg", "jndPhX", "x3svVc", "i4jeWR", "najNdz",
            "cpT0Cq", "cEts68", "+z5dYe", "5911Lb", "UB3hKo", "NcZh6K", "+KCMSi", "FYKGsL",
            "f7yOAX",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "key {key} empty");
        }
        assert!(dc_intl::t!("yQvgBO", platformName = "Steam").contains("Steam"));
        assert!(dc_intl::t!("Rzz1Xs", platformName = "X").contains("X"));
        assert!(dc_intl::t!("6u6J0q", platform = "Steam").contains("Steam"));
    }
}
