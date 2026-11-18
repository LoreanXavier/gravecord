//! The welcome buttons of an app's DM (module 891522 `s9`) and the questions behind them.
//!
//! A bot whose profile names its application, and whose application the account authorized (the newest token of
//! `GET /oauth2/tokens`, `getNewestTokenForApplication` of module 733110), gets:
//! - Mute or Unmute (`s5`, by `isChannelMuted(null, channel)`), which opens "Mute Notifications" / "Unmute
//!   Notifications" (module 259763). Its one button sends `updateChannelOverrideSettings({muted})` for the DM.
//! - Deauthorize (`s6`), unless the account owns the application, which asks "Deauthorize Application" and then
//!   deletes the token (`sX.A.delete`) and leaves the application's running Activity.
//! - Report Spam (`s4`), as every DM welcome has it.
//!
//! Any other bot gets Report Spam alone. GAP: the developer's own applications (`findProjectByApplicationId`, the
//! Edit App button of `s8`) are not known here, so an owned application shows as one that is only authorized.

use super::Root;
use super::chat_notices::{Alert, AlertButton, Then};
use super::dm::{Tone as ButtonTone, small_button};
use super::modal::Tone;
use dc_state::{Id, User};
use gpui::{AnyElement, Context, IntoElement, prelude::*};

/// Module 259763 for a DM that is `muted` now: the title, subtitle and one button that flips it.
pub(super) fn mute_app_alert(channel: Id, muted: bool) -> Alert {
    let (title, subtitle, label) = if muted {
        ("4rg7cA", "5vton6", "YqAjXy")
    } else {
        ("uAmAiL", "mscFJU", "w4m945")
    };
    Alert {
        id: "app-dm-mute",
        title: dc_intl::t(title).to_owned(),
        subtitle: Some(vec![(dc_intl::t(subtitle).to_owned(), false)]),
        content: None,
        buttons: vec![AlertButton {
            id: "app-dm-mute-confirm",
            label: dc_intl::t(label).to_owned(),
            // `variant: g ? "primary" : "critical-primary"`.
            tone: if muted { Tone::Primary } else { Tone::Critical },
            then: Then::MuteAppDm {
                channel,
                muted: !muted,
            },
        }],
    }
}

/// `s6`'s question: "Deauthorize Application", Cancel and the critical Deauthorize.
pub(super) fn deauthorize_alert(application: String, name: &str, token: String) -> Alert {
    Alert {
        id: "app-dm-deauthorize",
        title: dc_intl::t("DT39A+").to_owned(),
        subtitle: Some(vec![(
            dc_intl::format("QWGvxA", &[("applicationName", name.into())]),
            false,
        )]),
        content: None,
        buttons: vec![
            AlertButton {
                id: "app-dm-deauthorize-cancel",
                label: dc_intl::t("ETE/oC").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "app-dm-deauthorize-confirm",
                label: dc_intl::t("xUqheM").to_owned(),
                tone: Tone::Critical,
                then: Then::DeauthorizeApp { application, token },
            },
        ],
    }
}

impl Root {
    /// The application of a bot's profile (`getUserProfile(id).application`), with its id and name.
    fn bot_application(&self, user: Id) -> Option<(String, String)> {
        let application = &self.fetched.get(&format!("profile:{user}"))?["application"];
        let id = application["id"].as_str()?.to_owned();
        let name = application["name"]
            .as_str()
            .or_else(|| {
                self.stores
                    .users
                    .get(&user)
                    .map(|known| known.display_name())
            })
            .unwrap_or_default()
            .to_owned();
        Some((id, name))
    }

    /// `s9`: the buttons of a bot's DM welcome.
    pub(super) fn app_dm_buttons(
        &self,
        channel: Id,
        user: &User,
        report: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut list = Vec::new();
        let authorized = self
            .bot_application(user.id)
            .and_then(|(application, name)| {
                let token = self.newest_app_token(&application)?["id"]
                    .as_str()?
                    .to_owned();
                Some((application, name, token))
            });
        if let Some((application, name, token)) = authorized {
            let muted = self
                .stores
                .guild_settings
                .channel_muted(0, channel, chrono::Utc::now());
            list.push(
                small_button(
                    "welcome-app-mute",
                    dc_intl::t(if muted { "YqAjXy" } else { "w4m945" }),
                    if muted {
                        ButtonTone::Secondary
                    } else {
                        ButtonTone::CriticalPrimary
                    },
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.chat_extra.alert = Some(mute_app_alert(channel, muted));
                    cx.notify();
                }))
                .into_any_element(),
            );
            list.push(
                small_button(
                    "welcome-app-deauthorize",
                    dc_intl::t("xUqheM"),
                    ButtonTone::Secondary,
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.chat_extra.alert =
                        Some(deauthorize_alert(application.clone(), &name, token.clone()));
                    cx.notify();
                }))
                .into_any_element(),
            );
        }
        list.extend(report);
        list
    }

    /// Module 259763's button: the DM's override takes `muted` (`updateChannelOverrideSettings`, no guild).
    pub(super) fn mute_app_dm(&mut self, channel: Id, muted: bool) {
        self.patch_channel_override(0, channel, serde_json::json!({ "muted": muted }));
    }

    /// `s6`'s confirm: the token goes (`DELETE /oauth2/tokens/{id}`) and the application's Activity of this session
    /// is left (`sz.d`).
    pub(super) fn deauthorize_app(
        &mut self,
        application: &str,
        token: &str,
        cx: &mut Context<Self>,
    ) {
        let _ = self.requests.send(crate::net::Request::Rest {
            key: None,
            method: reqwest::Method::DELETE,
            route: format!("/oauth2/tokens/{token}"),
            body: None,
        });
        if let Some(tokens) = self.settings_extra.tokens.as_mut() {
            tokens.retain(|known| known["id"].as_str() != Some(token));
        }
        if self
            .activities
            .running
            .as_ref()
            .is_some_and(|running| running.application_id == application)
        {
            self.leave_activity(cx);
        }
    }

    /// Capture only: `app-dm <user id> <application id> <name>` gives a bot's DM a profile application and an
    /// authorized token for it, as `GET /users/{id}/profile` and `GET /oauth2/tokens` would; nothing is sent.
    #[cfg(feature = "capture")]
    pub(crate) fn script_app_dm(&mut self, user: Id, application: &str, name: &str) -> String {
        // `0`: the open DM's recipient.
        let user = if user == 0 {
            self.current_dm()
                .and_then(|dm| self.stores.dm_recipient(dm))
                .map_or(0, |known| known.id)
        } else {
            user
        };
        let mut profile = self
            .fetched
            .get(&format!("profile:{user}"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        profile["application"] = serde_json::json!({ "id": application, "name": name });
        self.fetched.insert(format!("profile:{user}"), profile);
        let token = serde_json::json!({
            "id": "1", "application": { "id": application, "name": name }, "scopes": ["bot"],
        });
        self.settings_extra
            .tokens
            .get_or_insert_with(Vec::new)
            .push(token);
        if let Some(known) = self.stores.users.get_mut(&user) {
            known.bot = true;
        }
        format!("app dm: {user} has {application}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mute_question_flips_with_the_dm() {
        let mute = mute_app_alert(7, false);
        assert_eq!(mute.title, "Mute Notifications");
        assert_eq!(mute.buttons[0].label, "Mute");
        assert_eq!(mute.buttons[0].tone, Tone::Critical);
        assert!(matches!(
            mute.buttons[0].then,
            Then::MuteAppDm {
                channel: 7,
                muted: true
            }
        ));
        let unmute = mute_app_alert(7, true);
        assert_eq!(unmute.title, "Unmute Notifications");
        assert_eq!(
            unmute.subtitle.as_ref().unwrap()[0].0,
            "This will unmute all message notifications from this App."
        );
        assert_eq!(unmute.buttons[0].label, "Unmute");
        assert_eq!(unmute.buttons[0].tone, Tone::Primary);
    }

    #[test]
    fn deauthorizing_names_the_application() {
        let alert = deauthorize_alert("1".into(), "Some Bot", "9".into());
        assert_eq!(alert.title, "Deauthorize Application");
        assert_eq!(
            alert.subtitle.unwrap()[0].0,
            "This action will remove the link between your Discord account and Some Bot"
        );
        assert_eq!(alert.buttons[1].label, "Deauthorize");
    }
}
