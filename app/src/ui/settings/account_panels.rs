//! The nested panels of the Account page: Logged-in Devices (`ACCOUNT_SESSIONS_NESTED_PANEL`, module 158935 `N3`)
//! and Multi-Factor Authentication (`ACCOUNT_MFA_NESTED_PANEL`, `bv`). The device list is the `GET /auth/sessions`
//! answer; logging a device out posts `{session_id_hashes}` to `/auth/sessions/logout` (module 200921 `U0`).

use super::{ButtonKind, Row, button, color, mask_icon, mfa, value};
use crate::net::Request;
use crate::ui::Root;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use serde_json::{Value, json};

pub(in crate::ui) const LOGOUT_KEY: &str = "settings-sessions-logout";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Panel {
    Sessions,
    Mfa,
    /// `ACCOUNT_STANDING_NESTED_PANEL` (`account_nested.rs`).
    Standing,
    /// `ACCOUNT_FAMILY_CENTER_NESTED_PANEL` (`account_nested.rs`).
    Family,
}

impl Panel {
    /// The title the nested header's breadcrumb ends with.
    pub(in crate::ui) fn title(self) -> &'static str {
        match self {
            Panel::Sessions => dc_intl::tr("Logged-in Devices"),
            Panel::Mfa => dc_intl::tr("Multi-Factor Authentication"),
            Panel::Standing => dc_intl::tr("Account Standing"),
            Panel::Family => dc_intl::tr("Family Center"),
        }
    }

    /// Whether the content header shows the breadcrumb (`contentHeaderLeft_3Rvb29`) rather than the page's name.
    pub(in crate::ui) fn has_breadcrumb(self) -> bool {
        true
    }
}

/// `/auth/sessions/logout` for the given `id_hash` values.
pub(in crate::ui) fn logout_request(hashes: &[String]) -> Request {
    Request::Rest {
        key: Some(LOGOUT_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: "/auth/sessions/logout".to_owned(),
        body: Some(json!({"session_id_hashes": hashes})),
    }
}

/// `NB` (module 158935): under an hour is "less than an hour ago", else moment's `fromNow`.
pub(in crate::ui) fn last_used(seconds_ago: i64) -> String {
    let minutes = seconds_ago.max(0) as f64 / 60.;
    if minutes < 60. {
        return dc_intl::tr("less than an hour ago").to_owned();
    }
    let hours = minutes / 60.;
    let days = hours / 24.;
    let plural = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    if hours < 22. {
        if hours < 1.5 {
            "an hour ago".to_owned()
        } else {
            plural(hours.round() as i64, "hour")
        }
    } else if hours < 36. {
        "a day ago".to_owned()
    } else if days < 26. {
        plural(days.round() as i64, "day")
    } else if days < 45. {
        "a month ago".to_owned()
    } else if days < 320. {
        plural((days / 30.4).round() as i64, "month")
    } else if days < 548. {
        "a year ago".to_owned()
    } else {
        plural((days / 365.).round() as i64, "year")
    }
}

/// One device as `NK` describes it: the OS as the label, the platform after a bullet, the place and the age under.
pub(in crate::ui) struct Device {
    pub hash: String,
    pub os: String,
    pub platform: String,
    pub detail: String,
    pub mobile: bool,
    pub used: i64,
}

/// Takes this client's own device out of the list: the one whose hash READY gave as `auth_session_id_hash` (module
/// 158935 `NF`), so that "Log Out All Known Devices" never includes it. READY's hash missing, the newest device stands in.
pub(in crate::ui) fn take_current(devices: &mut Vec<Device>, own: Option<&str>) -> Option<Device> {
    let index = match own {
        Some(hash) => devices.iter().position(|device| device.hash == hash),
        None => (!devices.is_empty()).then_some(0),
    }?;
    Some(devices.remove(index))
}

pub(in crate::ui) fn devices_of(list: &[Value], now: i64) -> Vec<Device> {
    let mut devices: Vec<Device> = list
        .iter()
        .map(|session| {
            let info = &session["client_info"];
            let used = chrono::DateTime::parse_from_rfc3339(
                session["approx_last_used_time"].as_str().unwrap_or(""),
            )
            .map_or(0, |time| time.timestamp());
            let os = info["os"].as_str().unwrap_or("").trim().to_owned();
            let place = info["location"]
                .as_str()
                .or_else(|| info["ip"].as_str())
                .unwrap_or("");
            Device {
                hash: session["id_hash"].as_str().unwrap_or("").to_owned(),
                mobile: matches!(os.to_lowercase().as_str(), "ios" | "android"),
                os: if os.is_empty() {
                    dc_intl::tr("Unknown").to_owned()
                } else {
                    os
                },
                platform: info["platform"].as_str().unwrap_or("").to_owned(),
                detail: place.to_owned(),
                used: now - used,
            }
        })
        .collect();
    devices.sort_by_key(|device| device.used);
    devices
}

impl Root {
    pub(in crate::ui) fn account_panel_page(
        &self,
        panel: Panel,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // Every nested panel shares the navigator (`NESTED_PANEL_NAVIGATOR`, module 963935): the header's back
        // button and breadcrumb are drawn by `settings_content`, the body starts right below them.
        let mut out = Vec::new();
        match panel {
            Panel::Sessions => out.extend(self.sessions_body(cx)),
            Panel::Mfa => out.extend(self.mfa_body(cx)),
            // Their bodies come from `account_nested`.
            Panel::Standing | Panel::Family => {}
        }
        out
    }

    fn sessions_body(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let note = |text: &'static str| {
            div()
                .pb(px(24.))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-subtle"))
                .child(text)
                .into_any_element()
        };
        let group = |title: &'static str, rows: Vec<AnyElement>| {
            div()
                .flex()
                .flex_col()
                .gap(px(24.))
                .py(px(8.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child(title),
                )
                .children(rows)
                .into_any_element()
        };
        let Some(list) = self
            .fetched
            .get("sessions")
            .and_then(|body| body["user_sessions"].as_array())
        else {
            return vec![value(dc_intl::tr("Loading...")).into_any_element()];
        };
        let now = chrono::Utc::now().timestamp();
        let mut devices = devices_of(list, now);
        let current = take_current(&mut devices, self.stores.auth_session_id_hash.as_deref());
        let row = |device: &Device, removable: bool, cx: &mut Context<Self>| {
            let hash = device.hash.clone();
            let description = [
                device.detail.clone(),
                if removable {
                    last_used(device.used)
                } else {
                    String::new()
                },
            ]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(mask_icon(
                    if device.mobile {
                        "safety_phone"
                    } else {
                        "golive_tab_screen"
                    },
                    24.,
                    "--icon-default",
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .gap(px(4.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(device.os.clone())
                                .when(!device.platform.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-subtle"))
                                            .child(format!("\u{2022} {}", device.platform)),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-muted"))
                                .child(description),
                        ),
                )
                .when(removable, |this| {
                    this.child(
                        div()
                            .id(SharedString::from(format!("session-logout-{hash}")))
                            .size(px(32.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this
                                    .requests
                                    .send(logout_request(std::slice::from_ref(&hash)));
                                cx.notify();
                            }))
                            .child(mask_icon(
                                "modal_close",
                                16.,
                                "--control-icon-only-icon-default",
                            )),
                    )
                })
                .into_any_element()
        };
        let mut out = vec![note(dc_intl::tr(
            "These are all the devices that are currently logged-in with your Discord account. Log out of devices that you don't recognize.",
        ))];
        if let Some(current) = &current {
            out.push(group("Current Device", vec![row(current, false, cx)]));
        }
        let rows: Vec<AnyElement> = devices.iter().map(|device| row(device, true, cx)).collect();
        if !rows.is_empty() {
            out.push(group("Other Devices", rows));
            let hashes: Vec<String> = devices.iter().map(|device| device.hash.clone()).collect();
            out.push(
                div()
                    .pt(px(24.))
                    .child(
                        Row::new(dc_intl::tr("Log out of all known devices"))
                            .description(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(dc_intl::tr(
                                        "You'll have to log back in on all logged out devices.",
                                    )),
                            )
                            .trailing(
                                div()
                                    .id("session-logout-all")
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let _ = this.requests.send(logout_request(&hashes));
                                        cx.notify();
                                    }))
                                    .child(button(
                                        "settings-logout-all",
                                        dc_intl::tr("Log Out All Known Devices"),
                                        ButtonKind::CriticalPrimary,
                                    )),
                            )
                            .render(),
                    )
                    .into_any_element(),
            );
        }
        out
    }

    /// Module 158935 `by`: Security Keys, Authenticator App, SMS Authentication and Backup Codes, in that order.
    /// Adding a security key needs the platform's WebAuthn dialog, so that button is not drawn.
    fn mfa_body(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let account = &self.stores.account;
        let flags = self
            .stores
            .current_user
            .as_ref()
            .map_or(0, |user| user.flags);
        let sms_on = flags & mfa::SMS_FLAG != 0;
        let opens = |kind: &'static str,
                     id: &'static str,
                     label: &'static str,
                     tone: ButtonKind,
                     cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(format!("mfa-{kind}")))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_account_form(kind, cx);
                    cx.notify();
                }))
                .child(button(id, label, tone))
                .into_any_element()
        };
        let mut out = Vec::new();

        // Security Keys (`bb`): `IBq4Y3` over `yK9edS`, the list of `GET /users/@me/mfa/webauthn/credentials`.
        let keys = match self
            .fetched
            .get(mfa::CREDENTIALS_KEY)
            .and_then(Value::as_array)
        {
            None => vec![value(dc_intl::tr("Loading...")).into_any_element()],
            Some(list) => {
                let now = chrono::Utc::now();
                list.iter()
                    .map(|credential| {
                        let name = credential["name"].as_str().unwrap_or_default().to_owned();
                        let used = credential["last_used"]
                            .as_str()
                            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                            .map(|at| {
                                last_used((now - at.with_timezone(&chrono::Utc)).num_seconds())
                            });
                        div()
                            .flex()
                            .flex_col()
                            .py(px(8.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(name),
                            )
                            .children(used.map(|ago| {
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(format!("Last used {ago}"))
                            }))
                            .into_any_element()
                    })
                    .collect()
            }
        };
        out.push(mfa_category(
            dc_intl::tr("Security Keys"),
            Some(
                div()
                    .child(dc_intl::tr(
                        "Use your fingerprint, face, screenlock, or security key when logging in.",
                    ))
                    .into_any_element(),
            ),
            None,
            keys,
        ));

        // Authenticator App (`bi`, `bn`).
        let mut header = None;
        let mut body = Vec::new();
        if !account.verified {
            body.push(self.warning_notice(dc_intl::tr(
                "You must verify your account before you can enable two-factor authentication.",
            )));
        } else if account.mfa_enabled {
            body.push(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr("Enabled")),
                    )
                    .child(opens(
                        "mfa-remove",
                        "mfa-remove-button",
                        dc_intl::tr("Remove"),
                        ButtonKind::CriticalSecondary,
                        cx,
                    ))
                    .into_any_element(),
            );
        } else {
            header = Some(opens(
                "mfa-enable",
                "mfa-enable-button",
                dc_intl::tr("Enable Authenticator App"),
                ButtonKind::Primary,
                cx,
            ));
        }
        out.push(mfa_category(
            dc_intl::tr("Authenticator App"),
            Some(div().child(dc_intl::tr("Use an app to generate a one-time code when logging in. This adds an extra layer of security to your Discord account, making sure that only you have the ability to log in.")).into_any_element()),
            header,
            body,
        ));

        if account.mfa_enabled && account.verified {
            // SMS Authentication (`bS`, `bh`): `N4` blocks staff, partners and accounts without an email.
            let blocked = self
                .stores
                .current_user
                .as_ref()
                .is_some_and(|user| user.flags & 3 != 0)
                || account.email.is_none();
            let mut header = None;
            let mut body = Vec::new();
            if sms_on {
                let revealed = self.fetched.contains_key(mfa::REVEAL_KEY);
                let phone = account.phone.clone().unwrap_or_default();
                let shown = if revealed {
                    format!("Phone number: {phone}")
                } else {
                    let tail: String = phone
                        .chars()
                        .rev()
                        .take(4)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    format!("Phone number ending in ****{tail}")
                };
                body.push(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::tr("Enabled")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(4.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-subtle"))
                                        .child(shown)
                                        .child(
                                            div()
                                                .id("mfa-sms-reveal")
                                                .cursor_pointer()
                                                .text_color(color("--text-link"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if this
                                                        .fetched
                                                        .remove(mfa::REVEAL_KEY)
                                                        .is_none()
                                                    {
                                                        this.fetched.insert(
                                                            mfa::REVEAL_KEY.to_owned(),
                                                            Value::Bool(true),
                                                        );
                                                    }
                                                    cx.notify();
                                                }))
                                                .child(if revealed {
                                                    dc_intl::tr("Hide")
                                                } else {
                                                    dc_intl::tr("Reveal")
                                                }),
                                        ),
                                ),
                        )
                        .child(opens(
                            "mfa-sms-remove",
                            "mfa-sms-remove-button",
                            dc_intl::tr("Remove"),
                            ButtonKind::CriticalSecondary,
                            cx,
                        ))
                        .into_any_element(),
                );
            } else if !blocked {
                header = Some(
                    div()
                        .id("mfa-sms-enable")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.mfa_enable_sms(cx);
                        }))
                        .child(button(
                            "mfa-sms-enable-button",
                            dc_intl::tr("Enable SMS Authentication"),
                            ButtonKind::Primary,
                        ))
                        .into_any_element(),
                );
            }
            out.push(mfa_category(
                dc_intl::tr("SMS Authentication"),
                None,
                header,
                body,
            ));

            // Backup Codes (`bA`, `bE`).
            let codes = self
                .fetched
                .get(mfa::CODES_KEY)
                .map(mfa::codes_of)
                .unwrap_or_default();
            let mut body = Vec::new();
            if codes.is_empty() {
                // `Jc2myK`: a `sm` secondary button.
                body.push(
                    div()
                        .id("mfa-codes")
                        .flex_none()
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|style| {
                            style
                                .bg(color("--control-secondary-background-hover"))
                                .border_color(color("--control-secondary-border-hover"))
                        })
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_account_form("mfa-codes", cx);
                        }))
                        .child(dc_intl::tr("Show Backup Codes"))
                        .into_any_element(),
                );
            } else {
                // `ul` `codeList_gRSMrg`: a column, `--space-sm` (12) between the rows, `--space-md` (16) below; each
                // `li` (`code_gRSMrg`) a row 12 apart: the indicator checkbox (checked once consumed) and the code in
                // `body-md/normal`. A press copies the code (`mGZ66D`).
                body.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .mb(px(16.))
                        .children(codes.iter().enumerate().map(|(index, (code, consumed))| {
                            let shown = mfa::code_text(code);
                            let copied = shown.clone();
                            div()
                                .id(SharedString::from(format!("mfa-code-{index}")))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        copied.clone(),
                                    ));
                                    this.show_success_toast(
                                        dc_intl::tr("Copied to clipboard.").into(),
                                        cx,
                                    );
                                }))
                                .child(crate::ui::server_onboarding::checkbox(*consumed, false))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(shown),
                                )
                        }))
                        .into_any_element(),
                );
                let email = account.email.clone().unwrap_or_default();
                let contents = mfa::backup_file(&email, &codes);
                // `s5.e` size sm: a row of `sm` secondary buttons with their icons, 8 apart.
                let small = |id: &'static str,
                             label: &'static str,
                             icon: &'static str,
                             run: PanelRun,
                             cx: &mut Context<Self>| {
                    div()
                        .id(id)
                        .flex_none()
                        .h(px(32.))
                        .px(px(11.))
                        .gap(px(4.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|style| {
                            style
                                .bg(color("--control-secondary-background-hover"))
                                .border_color(color("--control-secondary-border-hover"))
                        })
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .whitespace_nowrap()
                        .on_click(cx.listener(move |this, _, _, cx| run(this, cx)))
                        .child(mask_icon(icon, 16., "--control-secondary-text-default"))
                        .child(label)
                        .into_any_element()
                };
                body.push(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(small(
                            "mfa-codes-download",
                            dc_intl::tr("Download Backup Codes"),
                            "lb_download",
                            Box::new(move |this, cx| {
                                this.mfa_save_codes(&contents, cx);
                            }),
                            cx,
                        ))
                        .child(small(
                            "mfa-codes-regenerate",
                            dc_intl::tr("Generate New Backup Codes"),
                            "system_refresh",
                            Box::new(|this, cx| this.mfa_regenerate_codes(cx)),
                            cx,
                        ))
                        .into_any_element(),
                );
            }
            // `tp7zEK` (two paragraphs, the second in bold) with codes, `LoOi4S` (the first alone) without.
            let explain = div()
                .flex()
                .flex_col()
                .child(dc_intl::tr("These codes will allow you to enter your account if you lose your authenticator app. Protect them like you would protect your password. Do not share them with anyone, and keep them somewhere safe."))
                .children((!codes.is_empty()).then(|| {
                    div()
                        .font_weight(FontWeight(600.))
                        .child("Each code can only be used once. Any previously generated backup codes will no longer work.")
                }))
                .into_any_element();
            out.push(mfa_category(
                dc_intl::tr("Backup Codes"),
                Some(explain),
                None,
                body,
            ));
        }
        out
    }

    /// `bT`: an account with a phone number turns SMS authentication on at once; one without adds the number first.
    fn mfa_enable_sms(&mut self, cx: &mut Context<Self>) {
        if self.stores.account.phone.is_none() {
            self.open_account_form("phone", cx);
            return;
        }
        let _ = self
            .requests
            .send(mfa::panel_request(mfa::sms_enable_call()));
        cx.notify();
    }

    /// `bg`: with a verification key at hand the codes are regenerated at once, else the key is asked for first.
    fn mfa_regenerate_codes(&mut self, cx: &mut Context<Self>) {
        let stored = self.fetched.get(mfa::CODES_KEY);
        let key = stored
            .and_then(|body| body["key"].as_str())
            .unwrap_or_default();
        let nonce = stored
            .and_then(|body| body["regenerate_nonce"].as_str())
            .unwrap_or_default();
        if key.is_empty() {
            self.open_account_form("mfa-codes", cx);
        } else {
            let call = mfa::backup_codes_call(key, nonce, true);
            let _ = self.requests.send(mfa::panel_request(call));
        }
        cx.notify();
    }

    /// The answers of the panel's own requests (`MFA_KEY`) and the security keys list.
    pub(in crate::ui) fn mfa_answered(&mut self, key: &str, body: &Value, cx: &mut Context<Self>) {
        if key == mfa::CREDENTIALS_KEY {
            self.fetched.insert(key.to_owned(), body.clone());
            cx.notify();
            return;
        }
        let ok = body["ok"].as_bool() == Some(true);
        let answer = &body["body"];
        if body["refused"].as_bool() == Some(true) {
            return;
        }
        if !ok {
            // `F8FvUy` is the text when the answer has none.
            let message = answer["message"]
                .as_str()
                .unwrap_or("Uh oh, something went wrong. Please try again later.")
                .to_owned();
            self.settings_extra.dialog = Some(super::actions::Dialog::Alert {
                title: dc_intl::tr("Multi-Factor Authentication").to_owned(),
                body: message,
            });
            cx.notify();
            return;
        }
        if answer["backup_codes"].is_array() {
            // A regenerated set: the key and its nonce stay.
            let mut stored = answer.clone();
            if let Some(old) = self.fetched.get(mfa::CODES_KEY) {
                stored["key"] = old["key"].clone();
                stored["regenerate_nonce"] = old["regenerate_nonce"].clone();
            }
            self.fetched.insert(mfa::CODES_KEY.to_owned(), stored);
        } else if let Some(user) = self.stores.current_user.as_mut() {
            // `MFA_SMS_TOGGLE_COMPLETE`: the answer of enabling SMS.
            user.flags |= mfa::SMS_FLAG;
        }
        cx.notify();
    }

    /// The answer of a device logout: the list is read again.
    pub(in crate::ui) fn sessions_logout_answered(&mut self, body: &Value) {
        if body["ok"].as_bool() == Some(true) {
            let _ = self.requests.send(Request::Fetch {
                key: "sessions".to_owned(),
                route: "/auth/sessions".to_owned(),
            });
        }
    }
}

/// What a button of the panel runs on a press.
type PanelRun = Box<dyn Fn(&mut Root, &mut Context<Root>)>;
/// One category of the MFA panel (`zZ`): the 24px heading with its optional button on the right, the subtitle, then
/// the rows 8px apart. 40px of space between categories.
fn mfa_category(
    title: &'static str,
    subtitle: Option<AnyElement>,
    header: Option<AnyElement>,
    body: Vec<AnyElement>,
) -> AnyElement {
    div()
        .w_full()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .mb(px(40.))
        .child(
            div()
                .pb(px(20.))
                .flex()
                .items_start()
                .justify_between()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(24.))
                                .line_height(px(32.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .children(subtitle.map(|text| {
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-subtle"))
                                .child(text)
                        })),
                )
                .children(header),
        )
        .child(div().flex().flex_col().gap(px(8.)).children(body))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_logout_posts_the_hashes() {
        let Request::Rest { route, body, .. } = logout_request(&["a".to_owned(), "b".to_owned()])
        else {
            panic!("not rest");
        };
        assert_eq!(route, "/auth/sessions/logout");
        assert_eq!(body, Some(json!({"session_id_hashes": ["a", "b"]})));
    }

    #[test]
    fn the_current_device_is_the_one_ready_names() {
        let list = vec![
            json!({"id_hash": "phone", "approx_last_used_time": "2026-11-01T00:00:00+00:00", "client_info": {"os": "iOS"}}),
            json!({"id_hash": "me", "approx_last_used_time": "2026-10-01T00:00:00+00:00", "client_info": {"os": "Windows"}}),
        ];
        let mut devices = devices_of(&list, chrono::Utc::now().timestamp());
        let current = take_current(&mut devices, Some("me")).expect("current");
        assert_eq!(current.hash, "me");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].hash, "phone");
        let mut devices = devices_of(&list, chrono::Utc::now().timestamp());
        assert!(take_current(&mut devices, Some("gone")).is_none());
        assert_eq!(devices.len(), 2);
        assert_eq!(
            take_current(&mut devices, None).expect("fallback").hash,
            "phone"
        );
    }

    #[test]
    fn ages_read_like_moment() {
        assert_eq!(last_used(10), "less than an hour ago");
        assert_eq!(last_used(4000), "an hour ago");
        assert_eq!(last_used(3 * 3600), "3 hours ago");
        assert_eq!(last_used(30 * 3600), "a day ago");
        assert_eq!(last_used(5 * 86400), "5 days ago");
        assert_eq!(last_used(100 * 86400), "3 months ago");
    }

    #[test]
    fn devices_sort_newest_first_and_read_client_info() {
        let list = vec![
            json!({"id_hash": "old", "approx_last_used_time": "2026-01-01T00:00:00+00:00", "client_info": {"os": "iOS", "platform": "Discord iOS", "location": "Paris, France"}}),
            json!({"id_hash": "new", "approx_last_used_time": "2026-11-01T00:00:00+00:00", "client_info": {"os": "", "ip": "10.0.0.1"}}),
        ];
        let devices = devices_of(&list, chrono::Utc::now().timestamp());
        assert_eq!(devices[0].hash, "new");
        assert_eq!(devices[0].os, "Unknown");
        assert_eq!(devices[0].detail, "10.0.0.1");
        assert!(devices[1].mobile);
        assert_eq!(devices[1].platform, "Discord iOS");
    }
}
