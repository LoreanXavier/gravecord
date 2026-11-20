//! Two confirmations on the dialog of module 189213 with a box in the action bar, kept in the `voice_alert` slot:
//!
//! - "Confirm this isn't spam" (module 693864), opened by `markAsNotSpam` of module 190857 (Mark as Not Spam of a spam
//!   request) while the account has no `privacy.non_spam_retraining_opt_in` yet: "Help improve our spam filter by
//!   forwarding this message to Discord" (on) and "Don't show this again" (on). Confirm keeps the first box as the
//!   setting when the second is ticked, then accepts the request; Cancel only closes.
//! - "Hardware Acceleration will make this better" (module 845187), opened by an Activity's
//!   `ENCOURAGE_HW_ACCELERATION` (module 596339 `DM`) while hardware acceleration is off and the prompt was not turned
//!   off: "Turn it on!" switches it on; a ticked "Don't show again" keeps the prompt off
//!   (`disableActivityHardwareAccelerationPrompt`).

use super::modal;
use super::voice_alerts::VoiceAlert;
use super::{Root, color};
use crate::net::Request;
use dc_proto::settings::NON_SPAM_RETRAINING_OPT_IN;
use dc_state::Id;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use serde_json::json;

const UNSYNCED: &str = "UnsyncedUserSettingsStore";

/// `disableActivityHardwareAccelerationPrompt` of the unsynced settings.
pub(super) fn hardware_prompt_disabled() -> bool {
    crate::storage::get(UNSYNCED)
        .and_then(|store| {
            store
                .get("disableActivityHardwareAccelerationPrompt")?
                .as_bool()
        })
        .unwrap_or(false)
}

fn disable_hardware_prompt() {
    let mut store = crate::storage::get(UNSYNCED)
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| json!({}));
    store["disableActivityHardwareAccelerationPrompt"] = json!(true);
    crate::storage::set(UNSYNCED, store);
}

/// `ENCOURAGE_HW_ACCELERATION`'s condition: the prompt shows unless it was turned off or acceleration is on.
pub(super) fn hardware_prompt_shown(disabled: bool, enabled: bool) -> bool {
    !disabled && !enabled
}

/// `getSetting()` of `privacy.non_spam_retraining_opt_in`: unset, or the account's choice.
pub(super) fn retraining_choice(raw: Option<u64>) -> Option<bool> {
    raw.map(|value| value != 0)
}

/// A box and its label, as the action bar's `Checkbox` of module 150934 (`labelType: secondary`) and the body's of
/// module 143129 draw them.
fn labelled_box(
    id: &'static str,
    on: bool,
    label: &'static str,
    label_token: &'static str,
    toggle: impl Fn(&mut Root) + 'static,
    cx: &mut Context<Root>,
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .items_start()
        .gap(px(12.))
        .cursor_pointer()
        .child(super::server_onboarding::checkbox(on, false))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color(label_token))
                .child(label),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            toggle(this);
            cx.notify();
        }))
        .into_any_element()
}

impl Root {
    /// `markAsNotSpam` of module 190857 for a spam request.
    pub(crate) fn mark_not_spam(&mut self, channel: Id, cx: &mut Context<Self>) {
        let raw = self.stores.proto_varint(
            NON_SPAM_RETRAINING_OPT_IN.path,
            NON_SPAM_RETRAINING_OPT_IN.field,
        );
        match retraining_choice(raw) {
            None => {
                self.voice_alert = Some(VoiceAlert::NotSpam {
                    channel,
                    forward: true,
                    dont_show: true,
                });
                cx.notify();
            }
            Some(forward) => self.not_spam_confirmed(channel, forward, false, cx),
        }
    }

    /// `i` of module 190857's `O`: the setting when "Don't show this again" was ticked, then the accept.
    fn not_spam_confirmed(
        &mut self,
        channel: Id,
        forward: bool,
        dont_show: bool,
        cx: &mut Context<Self>,
    ) {
        if dont_show {
            let patch = self
                .stores
                .set_pref(&NON_SPAM_RETRAINING_OPT_IN, u64::from(forward));
            if !patch.is_empty() {
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::PATCH,
                    route: "/users/@me/settings-proto/1".to_owned(),
                    body: Some(json!({"settings": patch})),
                });
            }
        }
        if forward {
            // GAP: `tJ` of module 928658 sends the first message as a `FIRST_DM` report (variant `_first_dm_ham_v1`)
            // through the reporting flow, which is not built.
            tracing::debug!("not-spam message forward skipped");
        }
        self.mr_accept_now(channel, cx);
    }

    /// Module 693864.
    pub(super) fn not_spam_modal(
        &self,
        channel: Id,
        forward: bool,
        dont_show: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let forward_box = labelled_box(
            "not-spam-forward",
            forward,
            dc_intl::t("zLb+ux"),
            "--text-default",
            |this| {
                if let Some(VoiceAlert::NotSpam { forward, .. }) = this.voice_alert.as_mut() {
                    *forward = !*forward;
                }
            },
            cx,
        );
        let dont_show_box = labelled_box(
            "not-spam-dont-show",
            dont_show,
            dc_intl::t("KX8WJm"),
            "--text-subtle",
            |this| {
                if let Some(VoiceAlert::NotSpam { dont_show, .. }) = this.voice_alert.as_mut() {
                    *dont_show = !*dont_show;
                }
            },
            cx,
        );
        let spec = modal::Spec {
            id: "not-spam-confirm",
            title: dc_intl::t("6rP+Bd").into(),
            subtitle: Some(div().child(dc_intl::t("1LuCnS")).into_any_element()),
            body: Some(forward_box),
            buttons: vec![
                modal::button(
                    "not-spam-cancel",
                    dc_intl::t("ETE/oC"),
                    modal::Tone::Secondary,
                    |this, _, _| this.voice_alert = None,
                ),
                modal::button(
                    "not-spam-confirm-button",
                    dc_intl::t("cY+Oob"),
                    modal::Tone::Primary,
                    move |this, _, cx| {
                        let (forward, dont_show) = match this.voice_alert.take() {
                            Some(VoiceAlert::NotSpam {
                                forward, dont_show, ..
                            }) => (forward, dont_show),
                            _ => (true, true),
                        };
                        this.not_spam_confirmed(channel, forward, dont_show, cx);
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.voice_alert = None),
        };
        modal::render_with_input(spec, dont_show_box, cx)
    }

    /// `ENCOURAGE_HW_ACCELERATION`: the dialog when it is due, and the answer with the setting as it is.
    pub(super) fn encourage_hardware_acceleration(&mut self) -> serde_json::Value {
        let enabled = self.pref_on("hardware_acceleration", true);
        if hardware_prompt_shown(hardware_prompt_disabled(), enabled)
            && let Some(running) = self.activities.running.as_ref()
        {
            self.voice_alert = Some(VoiceAlert::HardwareAcceleration {
                name: running.name.clone(),
                dont_show: false,
            });
        }
        json!({ "enabled": enabled })
    }

    /// `T` of module 845187: a ticked box keeps the prompt off, then the dialog closes.
    pub(super) fn close_hardware_prompt(&mut self) {
        if let Some(VoiceAlert::HardwareAcceleration {
            dont_show: true, ..
        }) = self.voice_alert
        {
            disable_hardware_prompt();
        }
        self.voice_alert = None;
    }

    /// Module 845187.
    pub(super) fn hardware_acceleration_modal(
        &self,
        name: &str,
        dont_show: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dont_show_box = labelled_box(
            "hardware-acceleration-dont-show",
            dont_show,
            dc_intl::t("5E9SB9"),
            "--text-subtle",
            |this| {
                if let Some(VoiceAlert::HardwareAcceleration { dont_show, .. }) =
                    this.voice_alert.as_mut()
                {
                    *dont_show = !*dont_show;
                }
            },
            cx,
        );
        // `p?.name ?? "This Activity"`.
        let name = if name.is_empty() {
            "This Activity"
        } else {
            name
        };
        let body = div()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(dc_intl::format(
                "B9eiaK",
                &[("applicationName", name.into())],
            ));
        let spec = modal::Spec {
            id: "activity-hardware-acceleration-modal",
            title: dc_intl::t("NQkK4l").into(),
            subtitle: None,
            body: Some(body.into_any_element()),
            buttons: vec![
                modal::button(
                    "hardware-acceleration-no",
                    dc_intl::t("Ibf5/h"),
                    modal::Tone::Secondary,
                    |this, _, _| this.close_hardware_prompt(),
                ),
                modal::button(
                    "hardware-acceleration-on",
                    dc_intl::t("/wlDqi"),
                    modal::Tone::Primary,
                    |this, _, _| {
                        // `setEnableHardwareAcceleration(true)` (the official client relaunches; the window here
                        // always draws on the GPU).
                        if !this.pref_on("hardware_acceleration", true) {
                            this.toggle_pref("hardware_acceleration", true);
                        }
                        this.close_hardware_prompt();
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.close_hardware_prompt()),
        };
        modal::render_with_input(spec, dont_show_box, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spam_question_asks_only_while_the_setting_is_unset() {
        assert_eq!(retraining_choice(None), None);
        assert_eq!(retraining_choice(Some(0)), Some(false));
        assert_eq!(retraining_choice(Some(1)), Some(true));
    }

    #[test]
    fn the_hardware_prompt_needs_acceleration_off_and_the_prompt_on() {
        assert!(hardware_prompt_shown(false, false));
        assert!(!hardware_prompt_shown(true, false));
        assert!(!hardware_prompt_shown(false, true));
    }
}
