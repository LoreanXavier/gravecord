//! The nested panels of the Account page: Logged-in Devices (`ACCOUNT_SESSIONS_NESTED_PANEL`, module 158935 `N3`)
//! and Multi-Factor Authentication (`ACCOUNT_MFA_NESTED_PANEL`, `bv`). The device list is the `GET /auth/sessions`
//! answer; logging a device out posts `{session_id_hashes}` to `/auth/sessions/logout` (module 200921 `U0`).

use super::{ButtonKind, Row, button, category, color, mask_icon, value};
use crate::net::Request;
use crate::ui::Root;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use serde_json::{Value, json};

pub(in crate::ui) const LOGOUT_KEY: &str = "settings-sessions-logout";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Panel {
    Sessions,
    Mfa,
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
        return "less than an hour ago".to_owned();
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
                    "Unknown".to_owned()
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
        let title = match panel {
            Panel::Sessions => "Logged-in Devices",
            Panel::Mfa => "Multi-Factor Authentication",
        };
        let back = div()
            .id("account-panel-back")
            .flex()
            .items_center()
            .gap(px(8.))
            .pb(px(16.))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_extra.panel = None;
                cx.notify();
            }))
            .child(mask_icon("role_back", 16., "--interactive-text-default"))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--interactive-text-default"))
                    .child("Back"),
            )
            .into_any_element();
        let heading = div()
            .pb(px(20.))
            .text_size(px(24.))
            .line_height(px(32.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(title)
            .into_any_element();
        let mut out = vec![back, heading];
        match panel {
            Panel::Sessions => out.extend(self.sessions_body(cx)),
            Panel::Mfa => out.extend(self.mfa_body()),
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
            return vec![value("Loading...").into_any_element()];
        };
        let now = chrono::Utc::now().timestamp();
        let mut devices = devices_of(list, now);
        let current = (!devices.is_empty()).then(|| devices.remove(0));
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
        let mut out = vec![note(
            "These are all the devices that are currently logged-in with your Discord account. Log out of devices that you don't recognize.",
        )];
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
                        Row::new("Log out of all known devices")
                            .description(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child("You'll have to log back in on all logged out devices."),
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
                                        "Log Out All Known Devices",
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

    /// Module 158935 `by`: Security Keys, Authenticator App, SMS Authentication and Backup Codes. The setup flows of
    /// the app, the key and the codes need the account's second factor and are not built; only what the account
    /// already holds is shown.
    fn mfa_body(&self) -> Vec<AnyElement> {
        let account = &self.stores.account;
        let status = if account.mfa_enabled {
            "Enabled"
        } else {
            "Set up"
        };
        vec![category(
            Some("Authenticator App"),
            vec![Row::new("Status").trailing(value(status))],
        )]
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
