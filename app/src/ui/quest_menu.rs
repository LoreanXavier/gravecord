//! Quest card context menu (module 270045 `C` / `j`), "Why am I seeing this?" disclosure modal
//! (module 348559), and Quest CTA hooks & helpers (module 201805 `_c`, `ix`, `Lk`, `H1`, `175248`).
//!
//! Mounted from Quest Home tiles (`quests.rs`), the bottom-left Quest Bar (`quest_bar.rs`), and
//! Promoted Quest chat embed cards (`quest_promoted.rs`).

use super::quest_copy::{self, CtaAction};
use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::quests::{
    self, ORBS_INTRO_QUEST_ID, QUEST_HOME_DESKTOP, Quest, QuestRequest, feature, reward, task,
};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, MouseDownEvent, div, prelude::*,
    px,
};
use std::borrow::Cow;
use std::time::{Duration, Instant};

/// `QuestContent` surface codes relevant to module 270045 / 710969 (`vy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum QuestSurface {
    QuestHome,
    QuestBar,
    ActivityPanel,
    QuestEmbed,
    VoiceHeader,
}

impl QuestSurface {
    /// Wire `questContent` id for `POST /quests/{id}/dismissible-content/{c}/dismiss`.
    pub fn wire_id(self) -> u64 {
        match self {
            Self::QuestHome => QUEST_HOME_DESKTOP,
            Self::QuestBar => 2, // QUEST_BAR_V2
            Self::ActivityPanel => 4,
            Self::QuestEmbed => 9,
            Self::VoiceHeader => 8, // QUEST_LIVE_STREAM
        }
    }

    /// `710969.vy`: surfaces that expose the `"Hide This"` (`NN79E9`) menu row.
    pub fn is_dismissible(self) -> bool {
        matches!(
            self,
            Self::QuestBar | Self::ActivityPanel | Self::VoiceHeader
        )
    }
}

/// Open state of the `270045` context menu popout.
#[derive(Clone, Debug)]
pub(super) struct OpenQuestMenu {
    pub quest_id: String,
    pub surface: QuestSurface,
    pub hide_learn_more: bool,
    pub show_share_link: bool,
    pub show_disclosure: bool,
    pub anchor_x: f32,
    pub anchor_y: f32,
}

/// Open state of the `348559` Disclosure Modal ("Why am I seeing this?").
#[derive(Clone, Debug)]
pub(super) struct QuestDisclosureModal {
    pub quest_id: String,
    pub surface: QuestSurface,
    pub contextual_only: bool,
}

/// Open state of the `175248` Unverified Account Reward Prompt or `201805` generic Quest Error alert.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(super) enum QuestAlertModal {
    /// `201805.js`: `R0RpRX` / `OXD41D` enrollment/server error alert.
    Error { title: String, body: String },
    /// `175248.E`: `c8eASM` ("Verify your account to claim rewards!"), `qP5xYc`, `yb7itQ`, or `vjying`.
    Unverified { email: String, send_failed: bool },
}

/// All Quest menu / disclosure / alert UI state kept on `quests::State`.
#[derive(Clone, Debug, Default)]
pub(super) struct QuestMenuState {
    pub menu: Option<OpenQuestMenu>,
    pub disclosure: Option<QuestDisclosureModal>,
    pub alert: Option<QuestAlertModal>,
    /// Leading-only 3000ms throttle timestamp for `201805.Lk` share link toast.
    pub last_share_at: Option<Instant>,
}

/// `61387.F3` progress state consumed by `201805._c` (`useQuestCta`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestProgressState {
    Unaccepted,
    Accepted,
    InProgress,
    Completed,
    Claimed,
}

impl QuestProgressState {
    pub fn from_quest(quest: &Quest) -> Self {
        if quest.is_claimed() {
            Self::Claimed
        } else if quest.is_completed() {
            Self::Completed
        } else if quest.is_enrolled() {
            let details = quest.task_details();
            if details.progress_seconds > 0.0 || details.percent_complete > 0.0 {
                Self::InProgress
            } else {
                Self::Accepted
            }
        } else {
            Self::Unaccepted
        }
    }
}

/// Resolved hook result returned by `resolve_hook_cta` (`201805._c`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestHookCta {
    pub text: String,
    pub tooltip_text: Option<String>,
    pub action: Option<CtaAction>,
}

/// `W(quest)` of module 201805: launch/accept label for in-game / play-activity quests.
pub fn activity_cta_label(quest: &Quest) -> (&'static str, CtaAction) {
    if quest.config.tasks.contains(task::PLAY_ACTIVITY) {
        if quest.config.has_feature(feature::CLOUD_GAMING_ACTIVITY) {
            // `+qoymD`: "Try in Discord"
            (dc_intl::t("+qoymD"), CtaAction::LaunchActivity)
        } else {
            // `E4kW5O`: "Play Now"
            (dc_intl::t("E4kW5O"), CtaAction::LaunchActivity)
        }
    } else if quest.config.tasks.contains(task::ACHIEVEMENT_IN_ACTIVITY) {
        // `CkUzLd`: "Launch Quest"
        (dc_intl::t("CkUzLd"), CtaAction::LaunchActivity)
    } else {
        // `l7E81v`: "Accept Quest"
        (dc_intl::t("l7E81v"), CtaAction::Enroll)
    }
}

/// `201805._c` (`useQuestCta`): resolves `{ text, tooltipText, onClick }` across all 5 `QuestProgressState`s.
pub fn resolve_hook_cta(
    quest: &Quest,
    progress_state: QuestProgressState,
    in_gift_inventory: bool,
    needs_console_connection: bool,
) -> QuestHookCta {
    let is_video = quest.config.tasks.contains(task::WATCH_VIDEO)
        || quest.config.tasks.contains(task::WATCH_VIDEO_ON_MOBILE);
    let is_mobile_video = quest.config.tasks.contains(task::WATCH_VIDEO_ON_MOBILE)
        && !quest.config.tasks.contains(task::WATCH_VIDEO);
    let is_play_activity = quest.config.tasks.contains(task::PLAY_ACTIVITY)
        || quest.config.tasks.contains(task::ACHIEVEMENT_IN_ACTIVITY);
    let in_game_quest = quest.config.tasks.contains(task::ACHIEVEMENT_IN_GAME);

    match progress_state {
        QuestProgressState::Unaccepted => {
            if in_game_quest || is_play_activity {
                let (label, action) = activity_cta_label(quest);
                QuestHookCta {
                    text: label.to_owned(),
                    tooltip_text: None,
                    action: Some(action),
                }
            } else if is_video {
                QuestHookCta {
                    // `umdNin`: "Start Video Quest"
                    text: dc_intl::t("umdNin").to_owned(),
                    tooltip_text: None,
                    action: Some(if is_mobile_video {
                        CtaAction::WatchOnMobile
                    } else {
                        CtaAction::WatchVideo
                    }),
                }
            } else {
                QuestHookCta {
                    // `kUQLMJ`: "Accept Quest"
                    text: dc_intl::t("kUQLMJ").to_owned(),
                    tooltip_text: None,
                    action: Some(CtaAction::Enroll),
                }
            }
        }
        QuestProgressState::Accepted | QuestProgressState::InProgress => {
            if needs_console_connection && in_gift_inventory {
                QuestHookCta {
                    // `Cfye4v`: "Connect Console"
                    text: dc_intl::t("Cfye4v").to_owned(),
                    tooltip_text: None,
                    action: Some(CtaAction::ConnectConsole),
                }
            } else if is_mobile_video {
                QuestHookCta {
                    // `/cXIc6`: "Watch on mobile"
                    text: dc_intl::t("/cXIc6").to_owned(),
                    tooltip_text: None,
                    action: Some(CtaAction::WatchOnMobile),
                }
            } else if is_video {
                let details = quest.task_details();
                QuestHookCta {
                    text: quest_copy::video_cta_label(&details),
                    // `hsbwjv`: "Complete the task to unlock the reward"
                    tooltip_text: Some(dc_intl::t("hsbwjv").to_owned()),
                    action: Some(CtaAction::WatchVideo),
                }
            } else if is_play_activity {
                let (label, action) = activity_cta_label(quest);
                QuestHookCta {
                    text: label.to_owned(),
                    tooltip_text: Some(dc_intl::t("hsbwjv").to_owned()),
                    action: Some(action),
                }
            } else {
                QuestHookCta {
                    // `cfY4PE`: "Claim Reward" (disabled until completed)
                    text: dc_intl::t("cfY4PE").to_owned(),
                    tooltip_text: Some(dc_intl::t("hsbwjv").to_owned()),
                    action: None,
                }
            }
        }
        QuestProgressState::Completed => QuestHookCta {
            // `cfY4PE`: "Claim Reward"
            text: dc_intl::t("cfY4PE").to_owned(),
            tooltip_text: None,
            action: Some(CtaAction::ClaimReward),
        },
        QuestProgressState::Claimed => {
            let is_code = quest.config.reward_types.first().copied() == Some(reward::REWARD_CODE);
            QuestHookCta {
                // `bAGFz3`: "See Code" | `vTgCWx`: "View Reward"
                text: dc_intl::t(if is_code { "bAGFz3" } else { "vTgCWx" }).to_owned(),
                tooltip_text: None,
                action: Some(if is_code {
                    CtaAction::SeeCode
                } else {
                    CtaAction::ViewReward
                }),
            }
        }
    }
}

/// `201805.H1` / `61387.UX`: console setup help string (`beN4DG` for both Xbox + PSN, `HVS7nh` for one).
#[allow(dead_code)]
pub fn console_help_copy(both_consoles: bool) -> String {
    let key = if both_consoles { "beN4DG" } else { "HVS7nh" };
    format_plain(key, &[])
}

/// `343559` / `201805` console heartbeat error message (`Whhv4w` for 429, `xSCvBf` generic).
#[allow(dead_code)]
pub fn console_error_copy(status_code: u16) -> &'static str {
    if status_code == 429 {
        dc_intl::t("Whhv4w")
    } else {
        dc_intl::t("xSCvBf")
    }
}

/// `348559.js` / `651892.JM`: disclosure modal body paragraphs for a quest & surface.
pub fn disclosure_copy(quest: &Quest, contextual_only: bool) -> (String, String, String) {
    let pub_name = quest.config.game_publisher.as_str();
    let intro = if let Some(cosponsor) = quest.config.cosponsor_name.as_deref() {
        // `DV47Gy`: "{gamePublisher} and {cosponsorName} are trying to reach certain kinds of Discord users."
        dc_intl::format(
            "DV47Gy",
            &[
                ("gamePublisher", Arg::Str(Cow::Borrowed(pub_name))),
                ("cosponsorName", Arg::Str(Cow::Borrowed(cosponsor))),
            ],
        )
    } else {
        // `Piihy1`: "{gamePublisher} is trying to reach certain kinds of Discord users."
        dc_intl::format(
            "Piihy1",
            &[("gamePublisher", Arg::Str(Cow::Borrowed(pub_name)))],
        )
    };
    let promo = if quest.config.game_title.is_empty() {
        // `zDHY3s`
        dc_intl::format(
            "zDHY3s",
            &[("gamePublisher", Arg::Str(Cow::Borrowed(pub_name)))],
        )
    } else {
        // `euizJY`
        dc_intl::format(
            "euizJY",
            &[
                ("gamePublisher", Arg::Str(Cow::Borrowed(pub_name))),
                (
                    "gameTitle",
                    Arg::Str(Cow::Borrowed(quest.config.game_title.as_str())),
                ),
            ],
        )
    };
    // `fEbrT8` (personalized) vs `nPg6f1` (contextual signals)
    let reason = dc_intl::t(if contextual_only { "nPg6f1" } else { "fEbrT8" }).to_owned();
    (intro, promo, reason)
}

/// Converts a `dc_state::quests::QuestRequest` into a `crate::net::Request::Rest`.
pub(super) fn to_net_request(key: String, req: QuestRequest) -> Request {
    let method = match req.method {
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        "PUT" => reqwest::Method::PUT,
        "GET" => reqwest::Method::GET,
        _ => reqwest::Method::POST,
    };
    Request::Rest {
        key: Some(key),
        method,
        route: req.route,
        body: req.body,
    }
}

fn format_plain(key: &str, args: &[(&str, Arg<'_>)]) -> String {
    dc_intl::format_runs(key, args)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

impl Root {
    /// Opens the `270045` Quest context menu popout at `(anchor_x, anchor_y)`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_quest_menu(
        &mut self,
        quest_id: String,
        surface: QuestSurface,
        hide_learn_more: bool,
        show_share_link: bool,
        show_disclosure: bool,
        anchor_x: f32,
        anchor_y: f32,
        cx: &mut Context<Self>,
    ) {
        if self
            .quests
            .menus
            .menu
            .as_ref()
            .is_some_and(|m| m.quest_id == quest_id && m.surface == surface)
        {
            self.quests.menus.menu = None;
        } else {
            self.quests.menus.menu = Some(OpenQuestMenu {
                quest_id,
                surface,
                hide_learn_more,
                show_share_link,
                show_disclosure,
                anchor_x,
                anchor_y,
            });
        }
        cx.notify();
    }

    /// `201805.Lk`: copies `https://discord.com/quests/{id}` and shows success toast `+5kSoW` ("Link Copied!"),
    /// throttled leading-only for 3000 ms.
    pub(super) fn share_quest_link(&mut self, quest_id: &str, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self
            .quests
            .menus
            .last_share_at
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(3000))
        {
            return;
        }
        self.quests.menus.last_share_at = Some(now);
        let url = format!("https://discord.com/quests/{quest_id}");
        cx.write_to_clipboard(ClipboardItem::new_string(url));
        // `+5kSoW`: "Link Copied!"
        self.put_app_toast(dc_intl::t("+5kSoW").to_owned(), true, cx);
    }

    /// `201805.ix` (`G`): verifies email/phone requirement (`175248.E`) before sending `POST /quests/{id}/claim-reward`.
    pub(super) fn trigger_quest_claim(&mut self, quest_id: &str, cx: &mut Context<Self>) {
        let verified = self
            .stores
            .current_user
            .as_ref()
            .is_none_or(|_| self.stores.account.verified);
        let is_orbs = self
            .quests
            .store
            .quests
            .iter()
            .find(|q| q.id == quest_id)
            .is_some_and(|q| q.config.has_virtual_currency_reward());
        if !verified && is_orbs {
            self.quests.menus.alert = Some(QuestAlertModal::Unverified {
                email: self
                    .stores
                    .account
                    .email
                    .clone()
                    .unwrap_or_else(|| "your email".to_owned()),
                send_failed: false,
            });
            cx.notify();
            return;
        }
        let req = quests::claim_reward_request(quest_id, 0, QUEST_HOME_DESKTOP);
        let _ = self
            .requests
            .send(to_net_request(format!("quests:claim:{quest_id}"), req));
        // Optimistically reflect claimed_at locally so the card updates cleanly in offline replay too.
        if let Some(quest) = self
            .quests
            .store
            .quests
            .iter_mut()
            .find(|q| q.id == quest_id)
        {
            let status = quest
                .user_status
                .get_or_insert_with(|| dc_state::quests::UserStatus {
                    quest_id: quest_id.to_owned(),
                    ..Default::default()
                });
            status.claimed_at = Some(chrono::Utc::now().to_rfc3339());
        }
        cx.notify();
    }

    /// Sends `POST /quests/{id}/enroll` (`343559` / `201805`) and updates local status.
    pub(super) fn send_quest_enroll(
        &mut self,
        quest_id: &str,
        surface: QuestSurface,
        cx: &mut Context<Self>,
    ) {
        let req = quests::enroll_request(quest_id, surface.wire_id());
        let _ = self
            .requests
            .send(to_net_request(format!("quests:enroll:{quest_id}"), req));
        self.quest_enroll_local(quest_id, cx);
    }

    /// Sends `POST /quests/{id}/dismissible-content/{c}/dismiss` (`270045` / `343559.g5`) and hides surface locally.
    pub(super) fn dismiss_quest_surface(
        &mut self,
        quest_id: &str,
        surface: QuestSurface,
        cx: &mut Context<Self>,
    ) {
        let req = quests::dismiss_content_request(quest_id, surface.wire_id());
        let _ = self
            .requests
            .send(to_net_request(format!("quests:dismiss:{quest_id}"), req));
        if !self
            .quests
            .bar
            .dismissed_ids
            .iter()
            .any(|id| id == quest_id)
        {
            self.quests.bar.dismissed_ids.push(quest_id.to_owned());
        }
        self.quests.menus.menu = None;
        // Module 321538 (`en`): offer the "Not interested in this Quest?" dismiss survey (`798899`).
        let (entity_name, is_video) = self
            .quests
            .store
            .quests
            .iter()
            .find(|q| q.id == quest_id)
            .map(|q| {
                let video = q.config.tasks.iter().any(|t| t.contains("VIDEO"));
                let name = if q.config.game_title.is_empty() {
                    q.config.quest_name.clone()
                } else {
                    q.config.game_title.clone()
                };
                (name, video)
            })
            .unwrap_or_else(|| (String::from("this Quest"), false));
        self.open_quest_survey_modal(quest_id.to_owned(), entity_name, is_video, cx);
        cx.notify();
    }

    /// Renders the `270045` context menu popout layer when open.
    pub(super) fn quest_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.quests.menus.menu.as_ref()?;
        let quest = self
            .quests
            .store
            .quests
            .iter()
            .find(|q| q.id == open.quest_id)
            .cloned()?;
        let qid = quest.id.clone();
        let surface = open.surface;
        let is_orbs_intro = qid == ORBS_INTRO_QUEST_ID;
        let play_label = if quest.config.cta_button_label.is_empty() {
            "Play Game".to_owned()
        } else {
            quest.config.cta_button_label.clone()
        };
        let play_link = if is_orbs_intro {
            "https://support.discord.com/hc/en-us/articles/30593690165783".to_owned()
        } else {
            quest.config.cta_link.clone()
        };
        let show_share = open.show_share_link && !is_orbs_intro;
        let show_disclosure = open.show_disclosure && !is_orbs_intro;
        let show_hide = surface.is_dismissible();
        let is_preview = quest.preview;

        let menu_w = 240.0_f32;
        let left = open
            .anchor_x
            .min((self.viewport.0 - menu_w - 12.).max(8.))
            .max(8.);
        let top = open.anchor_y.min((self.viewport.1 - 280.).max(8.)).max(8.);

        let row = |id: &'static str,
                   label: String,
                   subtext: Option<&str>,
                   trailing_icon: Option<&'static str>| {
            div()
                .id(id)
                .w_full()
                .px(px(8.))
                .py(px(7.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(color("--brand-500")).text_color(color("--white")))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .text_color(color("--interactive-text-default"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .child(label),
                        )
                        .children(subtext.map(|sub| {
                            div()
                                .text_size(px(12.))
                                .line_height(px(15.))
                                .text_color(color("--text-muted"))
                                .child(sub.to_owned())
                        })),
                )
                .children(trailing_icon.map(|ic| icon(ic, 16., "--interactive-text-default")))
        };

        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            // Group "major-actions": Play Game + Share
            .child(
                row(
                    "quest-menu-play-game",
                    play_label,
                    None,
                    Some("open_external"),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.quests.menus.menu = None;
                    if !play_link.is_empty() {
                        cx.open_url(&play_link);
                    }
                    cx.notify();
                })),
            );

        if show_share {
            let share_id = qid.clone();
            list = list.child(
                row(
                    "quest-menu-share-link",
                    // `RDE0Sc`: "Share"
                    dc_intl::t("RDE0Sc").to_owned(),
                    None,
                    Some("share_link"),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.quests.menus.menu = None;
                    this.share_quest_link(&share_id, cx);
                })),
            );
        }

        // Minor actions separator
        let has_minor = !open.hide_learn_more || show_disclosure || show_hide;
        if has_minor {
            list = list.child(div().my(px(4.)).h(px(1.)).bg(color("--border-subtle")));
        }

        if !open.hide_learn_more {
            list = list.child(
                row(
                    "quest-menu-learn-more",
                    // `Ws2Bl+`: "View More Quests"
                    dc_intl::t("Ws2Bl+").to_owned(),
                    None,
                    Some("home_quests"),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.quests.menus.menu = None;
                    this.open_quests(cx);
                })),
            );
        }

        if show_disclosure {
            let disc_id = qid.clone();
            list = list.child(
                row(
                    "quest-menu-disclosure",
                    // `GcsZKJ`: "Why am I seeing this?"
                    dc_intl::t("GcsZKJ").to_owned(),
                    None,
                    Some("circle_info"),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.quests.menus.menu = None;
                    this.quests.menus.disclosure = Some(QuestDisclosureModal {
                        quest_id: disc_id.clone(),
                        surface,
                        contextual_only: false,
                    });
                    cx.notify();
                })),
            );
        }

        if show_hide {
            let hide_id = qid.clone();
            list = list.child(
                row(
                    "quest-menu-hide",
                    // `NN79E9`: "Hide This" | `RK9gxo`: subtext
                    dc_intl::t("NN79E9").to_owned(),
                    Some(dc_intl::t("RK9gxo")),
                    None,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.dismiss_quest_surface(&hide_id, surface, cx);
                })),
            );
        }

        // Preview controls (`270045.js`, `Ape+mm`) when `quest.preview == true`
        if is_preview {
            list = list
                .child(div().my(px(4.)).h(px(1.)).bg(color("--border-subtle")))
                .child(
                    div()
                        .px(px(8.))
                        .py(px(4.))
                        .text_size(px(11.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("Ape+mm")),
                );
            let id_d = qid.clone();
            let id_r = qid.clone();
            let id_p = qid.clone();
            let id_c = qid.clone();
            let id_copy = qid.clone();
            let id_tool = qid.clone();
            list = list
                .child(
                    row(
                        "quest-menu-prev-dismiss",
                        dc_intl::t("JF6W66").to_owned(),
                        None,
                        None,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quests.menus.menu = None;
                        let req = quests::preview_reset_dismissibility_request(&id_d);
                        let _ = this
                            .requests
                            .send(to_net_request(format!("quests:prev-dismiss:{id_d}"), req));
                        cx.notify();
                    })),
                )
                .child(
                    row(
                        "quest-menu-prev-reset",
                        dc_intl::t("taqkwK").to_owned(),
                        None,
                        None,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quests.menus.menu = None;
                        let req = quests::preview_reset_status_request(&id_r);
                        let _ = this
                            .requests
                            .send(to_net_request(format!("quests:prev-reset:{id_r}"), req));
                        let stop = quests::console_stop_request(&id_r);
                        let _ = this
                            .requests
                            .send(to_net_request(format!("quests:console-stop:{id_r}"), stop));
                        cx.notify();
                    })),
                )
                .child(
                    row(
                        "quest-menu-prev-progress",
                        dc_intl::t("cKSLr4").to_owned(),
                        None,
                        None,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quests.menus.menu = None;
                        let req = quests::preview_complete_request(&id_p, 0.5);
                        let _ = this
                            .requests
                            .send(to_net_request(format!("quests:prev-prog:{id_p}"), req));
                        cx.notify();
                    })),
                )
                .child(
                    row(
                        "quest-menu-prev-complete",
                        dc_intl::t("jQEfRT").to_owned(),
                        None,
                        None,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quests.menus.menu = None;
                        let req = quests::preview_complete_request(&id_c, 1.0);
                        let _ = this
                            .requests
                            .send(to_net_request(format!("quests:prev-comp:{id_c}"), req));
                        cx.notify();
                    })),
                )
                .child(
                    row(
                        "quest-menu-prev-copy-id",
                        dc_intl::t("oisrFi").to_owned(),
                        None,
                        None,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quests.menus.menu = None;
                        cx.write_to_clipboard(ClipboardItem::new_string(id_copy.clone()));
                        cx.notify();
                    })),
                )
                .child(
                    row(
                        "quest-menu-prev-tool",
                        dc_intl::t("tx5Ax5").to_owned(),
                        None,
                        None,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quests.menus.menu = None;
                        cx.open_url(&format!("https://discord.com/quest-preview/{id_tool}"));
                        cx.notify();
                    })),
                );
        }

        Some(
            gpui::deferred(
                div()
                    .id("quest-context-menu-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .child(
                        div()
                            .id("quests-entry")
                            .occlude()
                            .absolute()
                            .left(px(left))
                            .top(px(top))
                            .w(px(menu_w))
                            .p(px(6.))
                            .rounded(px(8.))
                            .bg(color("--background-surface-high"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.quests.menus.menu = None;
                                cx.notify();
                            }))
                            .when(false, |el| el.child(dc_intl::t("ogxXGq")))
                            .child(list),
                    ),
            )
            .with_priority(7)
            .into_any_element(),
        )
    }

    /// Renders the `348559` Disclosure Modal ("Why am I seeing this?") or `201805`/`175248` Quest alert modal.
    pub(super) fn quest_disclosure_and_alert_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(disc) = self.quests.menus.disclosure.as_ref() {
            let quest = self
                .quests
                .store
                .quests
                .iter()
                .find(|q| q.id == disc.quest_id)
                .cloned()?;
            let (intro, promo, reason) = disclosure_copy(&quest, disc.contextual_only);
            let traits: Vec<(&'static str, &'static str)> = if disc.contextual_only {
                vec![("discover_header_servers", dc_intl::t("2bL0wT"))]
            } else if disc.surface == QuestSurface::QuestBar {
                vec![
                    ("access_globe", dc_intl::t("xQSdPv")),
                    ("user_check", dc_intl::t("mYt7hQ")),
                    ("bot_game_controller", dc_intl::t("XAsWxQ")),
                ]
            } else {
                vec![("bot_game_controller", dc_intl::t("XAsWxQ"))]
            };
            let privacy_line = format_plain("tzq9Wa", &[]);
            return Some(
                gpui::deferred(
                    div()
                        .id("quest-disclosure-backdrop")
                        .occlude()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(gpui::rgba(0x000000b3))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.quests.menus.disclosure = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .id("quest-disclosure-modal")
                                .w(px(440.))
                                .rounded(px(12.))
                                .bg(color("--background-surface-high"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(
                                    div()
                                        .px(px(20.))
                                        .pt(px(20.))
                                        .pb(px(12.))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(18.))
                                                .line_height(px(22.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("GcsZKJ")),
                                        )
                                        .child(
                                            div()
                                                .id("quest-disclosure-close-x")
                                                .size(px(24.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.quests.menus.disclosure = None;
                                                    cx.notify();
                                                }))
                                                .child(icon("modal_close", 16., "--icon-subtle")),
                                        ),
                                )
                                .child(
                                    div()
                                        .px(px(20.))
                                        .pb(px(16.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(10.))
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .line_height(px(20.))
                                                .text_color(color("--text-default"))
                                                .child(intro),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .line_height(px(20.))
                                                .text_color(color("--text-default"))
                                                .child(reason),
                                        )
                                        .child(
                                            div()
                                                .p(px(10.))
                                                .rounded(px(8.))
                                                .bg(color("--background-base-lower"))
                                                .flex()
                                                .flex_col()
                                                .gap(px(8.))
                                                .children(traits.into_iter().map(|(ic, label)| {
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(8.))
                                                        .child(icon(ic, 16., "--icon-subtle"))
                                                        .child(
                                                            div()
                                                                .text_size(px(14.))
                                                                .font_weight(FontWeight(500.))
                                                                .text_color(color("--text-strong"))
                                                                .child(label),
                                                        )
                                                })),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .text_color(color("--text-muted"))
                                                .child(promo),
                                        )
                                        .child(
                                            div()
                                                .id("quest-disclosure-privacy")
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .text_color(color("--text-link"))
                                                .cursor_pointer()
                                                .hover(|s| s.underline())
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.quests.menus.disclosure = None;
                                                    this.open_settings_page("privacy", cx);
                                                }))
                                                .child(privacy_line),
                                        ),
                                )
                                .child(
                                    div()
                                        .px(px(20.))
                                        .py(px(14.))
                                        .bg(color("--background-base-lower"))
                                        .border_t_1()
                                        .border_color(color("--border-subtle"))
                                        .flex()
                                        .justify_end()
                                        .child(
                                            div()
                                                .id("quest-disclosure-close-btn")
                                                .h(px(36.))
                                                .px(px(18.))
                                                .rounded(px(8.))
                                                .bg(color("--control-primary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-primary-background-hover",
                                                    ))
                                                })
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--control-primary-text-default"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.quests.menus.disclosure = None;
                                                    cx.notify();
                                                }))
                                                .child(dc_intl::t("cpT0Cq")),
                                        ),
                                ),
                        ),
                )
                .with_priority(7)
                .into_any_element(),
            );
        }

        let alert = self.quests.menus.alert.as_ref()?;
        let (title, body, sub) = match alert {
            QuestAlertModal::Error { title, body } => (title.clone(), body.clone(), None),
            QuestAlertModal::Unverified { email, send_failed } => {
                let title = dc_intl::t("c8eASM").to_owned();
                if *send_failed {
                    (title, dc_intl::t("vjying").to_owned(), None)
                } else {
                    let b = format_plain(
                        "qP5xYc",
                        &[("emailAddress", Arg::Str(Cow::Borrowed(email.as_str())))],
                    );
                    (title, b, Some(dc_intl::t("yb7itQ").to_owned()))
                }
            }
        };
        Some(
            gpui::deferred(
                div()
                    .id("quest-alert-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(gpui::rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quests.menus.alert = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("quest-alert-modal")
                            .w(px(420.))
                            .rounded(px(12.))
                            .bg(color("--background-surface-high"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(body),
                            )
                            .children(sub.map(|s| {
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(s)
                            }))
                            .child(
                                div().mt(px(4.)).flex().justify_end().child(
                                    div()
                                        .id("quest-alert-close")
                                        .h(px(36.))
                                        .px(px(18.))
                                        .rounded(px(8.))
                                        .bg(color("--control-primary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--control-primary-text-default"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.quests.menus.alert = None;
                                            cx.notify();
                                        }))
                                        .child(dc_intl::t("cpT0Cq")),
                                ),
                            ),
                    ),
            )
            .with_priority(7)
            .into_any_element(),
        )
    }

    /// Capture-only helper: `quest-menu <open|disclosure|error|unverified|close>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_menu(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        let qid = self
            .quests
            .store
            .quests
            .first()
            .map(|q| q.id.clone())
            .unwrap_or_default();
        match mode.trim() {
            "open" => {
                if qid.is_empty() {
                    return "quest-menu: no quest in store".into();
                }
                self.quests.menus.menu = Some(OpenQuestMenu {
                    quest_id: qid.clone(),
                    surface: QuestSurface::QuestBar,
                    hide_learn_more: false,
                    show_share_link: true,
                    show_disclosure: true,
                    anchor_x: 240.0,
                    anchor_y: 360.0,
                });
                self.quests.menus.disclosure = None;
                self.quests.menus.alert = None;
                cx.notify();
                format!("quest-menu: open for {qid}")
            }
            "disclosure" => {
                if qid.is_empty() {
                    return "quest-menu: no quest in store".into();
                }
                self.quests.menus.menu = None;
                self.quests.menus.disclosure = Some(QuestDisclosureModal {
                    quest_id: qid.clone(),
                    surface: QuestSurface::QuestBar,
                    contextual_only: false,
                });
                cx.notify();
                format!("quest-menu: disclosure for {qid}")
            }
            "error" => {
                self.quests.menus.menu = None;
                self.quests.menus.alert = Some(QuestAlertModal::Error {
                    title: dc_intl::t("R0RpRX").to_owned(),
                    body: dc_intl::t("OXD41D").to_owned(),
                });
                cx.notify();
                "quest-menu: error alert".into()
            }
            "unverified" => {
                self.quests.menus.menu = None;
                self.quests.menus.alert = Some(QuestAlertModal::Unverified {
                    email: "user@example.com".into(),
                    send_failed: false,
                });
                cx.notify();
                "quest-menu: unverified alert".into()
            }
            "close" => {
                self.quests.menus.menu = None;
                self.quests.menus.disclosure = None;
                self.quests.menus.alert = None;
                cx.notify();
                "quest-menu: closed".into()
            }
            other => format!("quest-menu: unknown mode {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_quest(task_type: &str, reward_type: u64) -> Quest {
        Quest::from_value(&json!({
            "id": "9001",
            "preview": false,
            "config": {
                "config_version": 2,
                "id": "9001",
                "starts_at": "2026-01-01T00:00:00Z",
                "expires_at": "2027-01-01T00:00:00Z",
                "messages": {
                    "quest_name": "Starlight",
                    "game_title": "Starfield",
                    "game_publisher": "Bethesda"
                },
                "task_config_v2": {
                    "tasks": {
                        task_type: { "type": task_type, "target": 900 }
                    }
                },
                "rewards_config": {
                    "rewards": [{
                        "type": reward_type,
                        "orb_quantity": 500,
                        "messages": { "name": "500 Orbs", "name_with_article": "500 Orbs" }
                    }],
                    "platforms": [0]
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn hook_cta_matrix_covers_all_201805_states() {
        let play = sample_quest(task::PLAY_ON_DESKTOP, reward::VIRTUAL_CURRENCY);
        let unacc = resolve_hook_cta(&play, QuestProgressState::Unaccepted, false, false);
        assert_eq!(unacc.text, "Accept Quest");
        assert_eq!(unacc.action, Some(CtaAction::Enroll));

        let video = sample_quest(task::WATCH_VIDEO, reward::VIRTUAL_CURRENCY);
        let v_unacc = resolve_hook_cta(&video, QuestProgressState::Unaccepted, false, false);
        assert_eq!(v_unacc.text, "Start Video Quest");

        let act = sample_quest(task::PLAY_ACTIVITY, reward::VIRTUAL_CURRENCY);
        let a_unacc = resolve_hook_cta(&act, QuestProgressState::Unaccepted, false, false);
        assert_eq!(a_unacc.text, "Play Now");

        let con = resolve_hook_cta(&play, QuestProgressState::Accepted, true, true);
        assert_eq!(con.text, "Connect Console");

        let comp = resolve_hook_cta(&play, QuestProgressState::Completed, false, false);
        assert_eq!(comp.text, "Claim Reward");
        assert_eq!(comp.action, Some(CtaAction::ClaimReward));

        let code_q = sample_quest(task::PLAY_ON_DESKTOP, reward::REWARD_CODE);
        let cl = resolve_hook_cta(&code_q, QuestProgressState::Claimed, false, false);
        assert_eq!(cl.text, "See Code");

        let orb_cl = resolve_hook_cta(&play, QuestProgressState::Claimed, false, false);
        assert_eq!(orb_cl.text, "View Reward");
    }

    #[test]
    fn disclosure_and_console_helpers_resolve_intl() {
        let q = sample_quest(task::PLAY_ON_DESKTOP, reward::VIRTUAL_CURRENCY);
        let (intro, promo, reason) = disclosure_copy(&q, false);
        assert!(intro.contains("Bethesda"));
        assert!(promo.contains("Starfield"));
        assert!(!reason.is_empty());
        assert!(!console_help_copy(true).is_empty());
        assert!(!console_help_copy(false).is_empty());
        assert_eq!(console_error_copy(429), "You are being rate limited.");
    }
}
