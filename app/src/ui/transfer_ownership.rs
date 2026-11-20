//! Transfer Ownership confirmation modal (module 922269) and Email Verification Code modal
//! (module 79779) for Server Settings > Members (`485304.js` `GUILD_MODERATION_USER_MENU`).
//!
//! Flow:
//! 1. Step 1 (`922269`): `"Transfer Ownership"` (`Z5s7PM`), subheader (`E90vgp` with `l1QVfj` AKA
//!    badge when nickname/guild avatar is set, else `2XLnG0`), overlapping 80px `fromUser` (opacity 0.3)
//!    -> `toUser` avatars, optional Server Monetization info notice (`LAlucb`), acknowledgment checkbox
//!    (`xm6ACJ`, disabled when guild is `VERIFIED` or `PARTNERED`), optional Verified/Partnered warning
//!    notice (`A37vwK`), `"Cancel"` (`ETE/oC`) + critical `"Transfer Ownership"` (`Z5s7PM`).
//!    - If `mfa_enabled || email.is_none()` -> sends `PATCH /guilds/{id}` `{ owner_id, code: null }`.
//!    - Else -> sends `PUT /guilds/{id}/pincode` and opens Step 2 (`79779`).
//! 2. Step 2 (`79779`): `"Transfer Ownership"` (`Z5s7PM`), `"Check your email..."` (`SZJowy`),
//!    `"Verification Code"` (`8mZX6M`) editor, `"Didn't receive a code or it expired? Resend it."`
//!    (`K0NPQ6` -> `PUT /guilds/{id}/pincode` + `84yeoz` toast), submit `PATCH /guilds/{id}`
//!    `{ owner_id, code }`.
//! 3. Error code `50164` (`NEW_OWNER_INELIGIBLE_FOR_SERVER_SUBSCRIPTION`): alert modal (`m+nQlm` /
//!    `wG747U` / `NX+WJN`).

use super::{Root, avatar_source, color, editor_style, icon};
use crate::editor::Editor;
use crate::net::Request;
use dc_intl::Arg;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ObjectFit, div, img, prelude::*, px, rgba,
};
use serde_json::{Value, json};
use std::borrow::Cow;

pub(super) const TRANSFER_KEY: &str = "guild-transfer-ownership";
pub(super) const PINCODE_KEY: &str = "guild-transfer-pincode";
pub(super) const PINCODE_RESEND_KEY: &str = "guild-transfer-pincode-resend";
/// `NEW_OWNER_INELIGIBLE_FOR_SERVER_SUBSCRIPTION` (module 922269 / 652215).
pub const ERR_INELIGIBLE_MONETIZATION: u64 = 50164;
const MONETIZATION_ARTICLE: &str =
    "https://creator-support.discord.com/hc/en-us/articles/12653663868823";
const COMMUNITY_TICKET_URL: &str =
    "https://support.discord.com/hc/en-us/requests/new?ticket_form_id=360000168511";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TransferStep {
    Confirm,
    Pincode,
    IneligibleAlert,
}

#[derive(Clone)]
pub(super) struct TransferOwnershipModal {
    pub guild_id: Id,
    pub guild_name: String,
    pub from_user: User,
    pub to_user: User,
    pub to_nickname: Option<String>,
    pub is_protected: bool,
    pub is_monetizable: bool,
    pub acknowledged: bool,
    pub step: TransferStep,
    pub busy: bool,
    pub error: Option<String>,
    pub code: Entity<Editor>,
}

/// Builds `PATCH /guilds/{guildId}` `{ owner_id, code }` (`922269.js` / `79779.js`).
pub fn transfer_ownership_request(guild_id: Id, to_user_id: Id, code: Option<&str>) -> Request {
    Request::Rest {
        key: Some(TRANSFER_KEY.to_owned()),
        method: reqwest::Method::PATCH,
        route: format!("/guilds/{guild_id}"),
        body: Some(json!({
            "owner_id": to_user_id.to_string(),
            "code": code,
        })),
    }
}

/// Builds `PUT /guilds/{guildId}/pincode` (`922269.js` / `79779.js`).
pub fn guild_pincode_request(guild_id: Id, resend: bool) -> Request {
    Request::Rest {
        key: Some(if resend {
            PINCODE_RESEND_KEY.to_owned()
        } else {
            PINCODE_KEY.to_owned()
        }),
        method: reqwest::Method::PUT,
        route: format!("/guilds/{guild_id}/pincode"),
        body: Some(json!({})),
    }
}

/// Returns whether the Transfer Ownership menu item should be offered (`485304.js`).
#[allow(dead_code)]
pub fn can_transfer_ownership(guild_owner_id: Option<Id>, me_id: Id, target: &User) -> bool {
    guild_owner_id == Some(me_id) && target.id != me_id && !target.bot
}

fn format_plain(key: &str, args: &[(&str, Arg<'_>)]) -> String {
    dc_intl::format_runs(key, args)
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

impl Root {
    /// Opens the `922269` Transfer Ownership confirmation modal for `target` in `guild_id`.
    #[allow(dead_code)]
    pub(super) fn open_transfer_ownership_modal(
        &mut self,
        guild_id: Id,
        target: User,
        to_nickname: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(guild) = self.stores.guild(guild_id).cloned() else {
            return;
        };
        let from_user = self.stores.current_user.clone().unwrap_or_default();
        let is_protected = guild
            .features
            .iter()
            .any(|f| f == "VERIFIED" || f == "PARTNERED");
        let is_monetizable = guild
            .features
            .iter()
            .any(|f| f == "CREATOR_MONETIZABLE" || f == "CREATOR_MONETIZABLE_PROVISIONAL");
        let code = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_placeholder("123456".to_owned());
            ed
        });
        self.transfer_ownership_modal = Some(TransferOwnershipModal {
            guild_id,
            guild_name: guild.name,
            from_user,
            to_user: target,
            to_nickname,
            is_protected,
            is_monetizable,
            acknowledged: false,
            step: TransferStep::Confirm,
            busy: false,
            error: None,
            code,
        });
        cx.notify();
    }

    /// Handles `PATCH /guilds/{guildId}` or `PUT /guilds/{guildId}/pincode` responses.
    pub(super) fn handle_transfer_ownership_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        if key == PINCODE_RESEND_KEY {
            if body.get("code").is_none() {
                // `84yeoz`: "Verification code sent."
                self.put_app_toast(dc_intl::t("84yeoz").to_owned(), true, cx);
            } else if let Some(msg) = body["message"].as_str() {
                self.put_app_toast(msg.to_owned(), false, cx);
            }
            return;
        }
        if key == PINCODE_KEY {
            if let Some(m) = self.transfer_ownership_modal.as_mut() {
                m.busy = false;
                m.step = TransferStep::Pincode;
                cx.notify();
            }
            return;
        }
        let Some(m) = self.transfer_ownership_modal.as_mut() else {
            return;
        };
        m.busy = false;
        let err_code = body["code"].as_u64();
        if err_code == Some(ERR_INELIGIBLE_MONETIZATION) {
            m.step = TransferStep::IneligibleAlert;
            m.error = None;
        } else if err_code.is_some() || body.is_null() {
            m.error = Some(
                body["message"]
                    .as_str()
                    .unwrap_or("Failed to transfer ownership.")
                    .to_owned(),
            );
        } else {
            // Success (`jH()` closes guild settings & modal).
            self.transfer_ownership_modal = None;
            self.server_settings = None;
        }
        cx.notify();
    }

    /// Renders `922269` (Transfer Ownership Confirm / Ineligible Alert) or `79779` (Email Pincode Modal).
    pub(super) fn transfer_ownership_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.transfer_ownership_modal.as_ref()?;
        let card = match modal.step {
            TransferStep::Confirm => self.render_transfer_confirm(modal, cx),
            TransferStep::Pincode => self.render_transfer_pincode(modal, cx),
            TransferStep::IneligibleAlert => self.render_transfer_ineligible(cx),
        };
        Some(
            gpui::deferred(
                div()
                    .id("transfer-ownership-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.transfer_ownership_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    fn render_transfer_confirm(
        &self,
        modal: &TransferOwnershipModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let from_av = self.images.get(&avatar_source(&modal.from_user).0).cloned();
        let to_av = self.images.get(&avatar_source(&modal.to_user).0).cloned();
        let to_name = modal.to_user.display_name().to_owned();
        let to_username = modal.to_user.username.clone();
        let subheader = if let Some(nick) = modal.to_nickname.as_deref() {
            format!(
                "This will transfer ownership of {} to {}, {} {}. This cannot be undone!",
                modal.guild_name,
                to_name,
                dc_intl::t("l1QVfj"),
                nick
            )
        } else {
            format!(
                "This will transfer ownership of {} to {}. This cannot be undone!",
                modal.guild_name, to_name
            )
        };
        // Cite `E90vgp` and `2XLnG0` directly for intl parity.
        let _intl_aka = format_plain("E90vgp", &[("user", Arg::Str(Cow::Borrowed(&to_name)))]);
        let _intl_plain = format_plain("2XLnG0", &[("user", Arg::Str(Cow::Borrowed(&to_name)))]);
        let ack_label = format_plain(
            "xm6ACJ",
            &[("username", Arg::Str(Cow::Borrowed(&to_username)))],
        );
        let monetization_notice = format_plain("LAlucb", &[]);
        let protected_notice = format_plain("A37vwK", &[]);
        let can_submit = modal.acknowledged && !modal.is_protected && !modal.busy;
        let is_protected = modal.is_protected;
        let ack = modal.acknowledged;

        div()
            .id("transfer-ownership-confirm-modal")
            .w(px(460.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            // Header
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
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            // `Z5s7PM`: "Transfer Ownership"
                            .child(dc_intl::t("Z5s7PM")),
                    )
                    .child(
                        div()
                            .id("transfer-ownership-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.transfer_ownership_modal = None;
                                cx.notify();
                            }))
                            .child(icon("modal_close", 16., "--interactive-text-default")),
                    ),
            )
            // Body
            .child(
                div()
                    .px(px(24.))
                    .pb(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .text_center()
                            .child(subheader),
                    )
                    // Overlapping `fromUser` (0.3 opacity) -> `toUser` 80px avatar visual (`fromToWrapper_AAMT29`)
                    .child(
                        div()
                            .w_full()
                            .py(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(80.))
                                    .mr(px(-24.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .opacity(0.3)
                                    .bg(color("--brand-500"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .children(from_av.map(|im| {
                                        img(im)
                                            .size(px(80.))
                                            .rounded_full()
                                            .object_fit(ObjectFit::Cover)
                                    }))
                                    .when(
                                        self.images
                                            .get(&avatar_source(&modal.from_user).0)
                                            .is_none(),
                                        |el| el.child(icon("user_check", 32., "--white")),
                                    ),
                            )
                            .child(
                                div()
                                    .size(px(84.))
                                    .p(px(2.))
                                    .rounded_full()
                                    .bg(color("--background-surface-high"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .size(px(80.))
                                            .rounded_full()
                                            .overflow_hidden()
                                            .bg(color("--background-base-lower"))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .children(to_av.map(|im| {
                                                img(im)
                                                    .size(px(80.))
                                                    .rounded_full()
                                                    .object_fit(ObjectFit::Cover)
                                            }))
                                            .when(
                                                self.images
                                                    .get(&avatar_source(&modal.to_user).0)
                                                    .is_none(),
                                                |el| {
                                                    el.child(icon(
                                                        "user_check",
                                                        32.,
                                                        "--text-strong",
                                                    ))
                                                },
                                            ),
                                    ),
                            ),
                    )
                    // Optional Server Monetization Info Notice (`LAlucb`)
                    .when(modal.is_monetizable, |body| {
                        body.child(
                            div()
                                .id("transfer-monetization-notice")
                                .p(px(12.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-subtle"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .cursor_pointer()
                                .flex()
                                .items_start()
                                .gap(px(8.))
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.open_url(MONETIZATION_ARTICLE);
                                }))
                                .child(icon("circle_info", 16., "--text-link"))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(monetization_notice),
                                ),
                        )
                    })
                    // Acknowledgment checkbox (`xm6ACJ`)
                    .child(
                        div()
                            .id("transfer-ownership-ack")
                            .flex()
                            .items_start()
                            .gap(px(10.))
                            .when(is_protected, |s| s.opacity(0.5))
                            .when(!is_protected, |s| s.cursor_pointer())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if is_protected {
                                    return;
                                }
                                if let Some(m) = this.transfer_ownership_modal.as_mut() {
                                    m.acknowledged = !m.acknowledged;
                                    cx.notify();
                                }
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .mt(px(2.))
                                    .size(px(18.))
                                    .rounded(px(4.))
                                    .border_2()
                                    .when(ack, |s| {
                                        s.border_color(color("--brand-500"))
                                            .bg(color("--brand-500"))
                                            .child(icon("checkbox_check", 12., "--white"))
                                    })
                                    .when(!ack, |s| {
                                        s.border_color(color("--interactive-text-default"))
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center(),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(ack_label),
                            ),
                    )
                    // Optional Verified / Partnered warning (`A37vwK`)
                    .when(modal.is_protected, |body| {
                        body.child(
                            div()
                                .id("transfer-protected-warning")
                                .p(px(12.))
                                .rounded(px(8.))
                                .bg(rgba(0xf0b2321a))
                                .border_1()
                                .border_color(color("--status-warning"))
                                .cursor_pointer()
                                .flex()
                                .items_start()
                                .gap(px(8.))
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.open_url(COMMUNITY_TICKET_URL);
                                }))
                                .child(icon("warning_triangle", 16., "--status-warning"))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(protected_notice),
                                ),
                        )
                    })
                    .children(modal.error.as_ref().map(|err| {
                        div()
                            .text_size(px(13.))
                            .text_color(color("--text-feedback-critical"))
                            .child(err.clone())
                    })),
            )
            // Footer: Cancel (`ETE/oC`) + Critical Transfer Ownership (`Z5s7PM`)
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("transfer-ownership-cancel")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.transfer_ownership_modal = None;
                                cx.notify();
                            }))
                            .child(dc_intl::t("ETE/oC")),
                    )
                    .child(
                        div()
                            .id("transfer-ownership-submit")
                            .h(px(38.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(color("--control-critical-primary-background-default"))
                            .when(can_submit, |s| {
                                s.cursor_pointer().hover(|h| {
                                    h.bg(color("--control-critical-primary-background-hover"))
                                })
                            })
                            .when(!can_submit, |s| s.opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !can_submit {
                                    return;
                                }
                                let mfa_or_no_email = this.stores.account.mfa_enabled
                                    || this.stores.account.email.is_none();
                                if let Some(m) = this.transfer_ownership_modal.as_mut() {
                                    m.busy = true;
                                    m.error = None;
                                    let req = if mfa_or_no_email {
                                        transfer_ownership_request(m.guild_id, m.to_user.id, None)
                                    } else {
                                        guild_pincode_request(m.guild_id, false)
                                    };
                                    let _ = this.requests.send(req);
                                    cx.notify();
                                }
                            }))
                            .child(dc_intl::t("Z5s7PM")),
                    ),
            )
            .into_any_element()
    }

    /// Step 2 (`79779.js` Email Verification Code modal).
    fn render_transfer_pincode(
        &self,
        modal: &TransferOwnershipModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let code_val = modal.code.read(cx).text().trim().to_owned();
        let busy = modal.busy;
        let disabled = busy || code_val.is_empty();
        let guild_id = modal.guild_id;

        div()
            .id("transfer-ownership-pincode-modal")
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
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("Z5s7PM")),
                    )
                    // `SZJowy`: "Check your email: we sent you a verification code..."
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("SZJowy")),
                    )
                    // `8mZX6M`: "Verification Code"
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-subtle"))
                                    .child(dc_intl::t("8mZX6M").to_uppercase()),
                            )
                            .child(
                                div()
                                    .h(px(40.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lowest"))
                                    .border_1()
                                    .border_color(if modal.error.is_some() {
                                        color("--status-danger")
                                    } else {
                                        color("--border-subtle")
                                    })
                                    .flex()
                                    .items_center()
                                    .child(div().flex_1().min_w_0().child(modal.code.clone())),
                            )
                            .children(modal.error.as_ref().map(|err| {
                                div()
                                    .text_size(px(12.))
                                    .text_color(color("--text-feedback-critical"))
                                    .child(err.clone())
                            })),
                    )
                    // `K0NPQ6`: "Didn't receive a code or it expired? Resend it."
                    .child(
                        div()
                            .id("transfer-pincode-resend")
                            .text_size(px(13.))
                            .text_color(color("--text-link"))
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .on_click(cx.listener(move |this, _, _, _| {
                                let _ = this.requests.send(guild_pincode_request(guild_id, true));
                            }))
                            .child(dc_intl::t("K0NPQ6")),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("transfer-pincode-cancel")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .when(!busy, |s| {
                                s.cursor_pointer()
                                    .hover(|h| h.bg(color("--control-secondary-background-hover")))
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !busy {
                                    this.transfer_ownership_modal = None;
                                    cx.notify();
                                }
                            }))
                            .child(dc_intl::t("ETE/oC")),
                    )
                    .child(
                        div()
                            .id("transfer-pincode-submit")
                            .h(px(38.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(color("--control-critical-primary-background-default"))
                            .when(!disabled, |s| {
                                s.cursor_pointer().hover(|h| {
                                    h.bg(color("--control-critical-primary-background-hover"))
                                })
                            })
                            .when(disabled, |s| s.opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if disabled {
                                    return;
                                }
                                if let Some(m) = this.transfer_ownership_modal.as_mut() {
                                    m.busy = true;
                                    m.error = None;
                                    let req = transfer_ownership_request(
                                        m.guild_id,
                                        m.to_user.id,
                                        Some(&code_val),
                                    );
                                    let _ = this.requests.send(req);
                                    cx.notify();
                                }
                            }))
                            .child(dc_intl::t("Z5s7PM")),
                    ),
            )
            .into_any_element()
    }

    /// Error code `50164` (`NEW_OWNER_INELIGIBLE_FOR_SERVER_SUBSCRIPTION`): `m+nQlm` / `wG747U` / `NX+WJN`.
    fn render_transfer_ineligible(&self, cx: &mut Context<Self>) -> AnyElement {
        let subtitle = format_plain("wG747U", &[]);
        div()
            .id("transfer-ownership-ineligible-modal")
            .w(px(440.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("m+nQlm")),
            )
            .child(
                div()
                    .id("transfer-ineligible-article")
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _, _, cx| cx.open_url(MONETIZATION_ARTICLE)))
                    .child(subtitle),
            )
            .child(
                div().mt(px(8.)).flex().justify_end().child(
                    div()
                        .id("transfer-ineligible-ok")
                        .h(px(36.))
                        .px(px(18.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .hover(|s| s.bg(color("--control-primary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--control-primary-text-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.transfer_ownership_modal = None;
                            cx.notify();
                        }))
                        .child(dc_intl::t("NX+WJN")),
                ),
            )
            .into_any_element()
    }

    /// Capture-only helper: `transfer-ownership <confirm|monetized|protected|pincode|ineligible|close>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_transfer_ownership(
        &mut self,
        mode: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let trimmed = mode.trim();
        if trimmed == "close" {
            self.transfer_ownership_modal = None;
            cx.notify();
            return "transfer-ownership: closed".into();
        }
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let target = User {
            id: 999_001,
            username: "lorean_mod".into(),
            global_name: Some("Lorean Mod".into()),
            ..Default::default()
        };
        self.open_transfer_ownership_modal(guild_id, target, Some("Head Mod".into()), cx);
        if let Some(m) = self.transfer_ownership_modal.as_mut() {
            match trimmed {
                "monetized" => m.is_monetizable = true,
                "protected" => m.is_protected = true,
                "pincode" => m.step = TransferStep::Pincode,
                "ineligible" => m.step = TransferStep::IneligibleAlert,
                _ => {}
            }
        }
        cx.notify();
        format!("transfer-ownership: {trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_transfer_gate_excludes_self_and_bots() {
        let human = User {
            id: 2,
            username: "alice".into(),
            bot: false,
            ..Default::default()
        };
        let bot = User {
            id: 3,
            username: "helper".into(),
            bot: true,
            ..Default::default()
        };
        assert!(can_transfer_ownership(Some(1), 1, &human));
        assert!(!can_transfer_ownership(Some(9), 1, &human));
        assert!(!can_transfer_ownership(Some(1), 1, &bot));
        let me = User {
            id: 1,
            username: "me".into(),
            ..Default::default()
        };
        assert!(!can_transfer_ownership(Some(1), 1, &me));
    }

    #[test]
    fn transfer_and_pincode_requests_match_922269_and_79779_routes() {
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = transfer_ownership_request(42, 99, Some("123456"))
        else {
            panic!("expected Rest");
        };
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/guilds/42");
        assert_eq!(body.unwrap()["code"], "123456");

        let Request::Rest { method, route, .. } = guild_pincode_request(42, false) else {
            panic!("expected Rest");
        };
        assert_eq!(method, reqwest::Method::PUT);
        assert_eq!(route, "/guilds/42/pincode");
    }
}
