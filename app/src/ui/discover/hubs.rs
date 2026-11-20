//! The Student Hubs tab of the Servers page and its "Join Hub" dialog (module 300290 `eU` and `eD`, the dialog of
//! module 500126 built on the shell of module 189213). Only the first step of the dialog (`VERIFY_EMAIL`) and the
//! viewer-has-no-hub state of the tab were captured; the later steps (school list, school request, PIN) are not built.
//!
//! The dialog's request is `sendVerificationEmail` of module 154672: `POST /guilds/automations/email-domain-lookup`
//! with the email, an optional guild and the two flags. It mails the viewer a code, so it is an account action and a
//! harness run refuses it centrally (`account_route` in `net.rs`).
use super::{Root, text};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use crate::ui::{color, editor_style, icon};
use gpui::{
    AnyElement, Context, Entity, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    SharedString, StyledText, Subscription, Window, deferred, div, prelude::*, px, rgba,
};
use serde_json::{Value, json};

/// The key the lookup's answer comes back under.
pub(super) const LOOKUP_KEY: &str = "discover:hub:lookup";
const LOOKUP_ROUTE: &str = "/guilds/automations/email-domain-lookup";
const TERMS: &str = "https://discord.com/terms";
const PRIVACY: &str = "https://discord.com/privacy";

/// The open dialog: the email field, the error under it and whether the request is out.
pub(in crate::ui) struct HubDialog {
    pub email: Entity<Editor>,
    pub error: Option<String>,
    pub busy: bool,
    _events: Subscription,
}

/// `sendVerificationEmail(email, true, guild)`: the body carries `guild_id` only when there is one (an undefined field
/// is dropped by `JSON.stringify`).
pub(super) fn lookup_request(email: &str, guild: Option<u64>) -> Request {
    let mut body = json!({
        "email": email,
        "allow_multiple_guilds": true,
        "use_verification_code": true,
    });
    if let Some(guild) = guild {
        body["guild_id"] = json!(guild.to_string());
    }
    Request::Rest {
        key: Some(LOOKUP_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: LOOKUP_ROUTE.to_owned(),
        body: Some(body),
    }
}

/// Where the lookup's answer sends the dialog (the `onSubmit` of module 500126).
#[derive(Debug, PartialEq)]
pub(super) enum Next {
    /// `has_matching_guild`: the code was sent for the guild already in play.
    VerifyPin,
    /// No school matches the address.
    SubmitSchool,
    /// One school matches: the code is sent again for that guild, then the PIN step.
    SendAgain(u64),
    /// Several schools match: the viewer picks one.
    SelectSchool(Vec<Value>),
}

pub(super) fn next_step(body: &Value) -> Next {
    let schools = body["guilds_info"].as_array().cloned().unwrap_or_default();
    if body["has_matching_guild"] == true {
        return Next::VerifyPin;
    }
    match schools.len() {
        0 => Next::SubmitSchool,
        1 => match super::data::id_of(&schools[0]["id"]) {
            Some(id) => Next::SendAgain(id),
            None => Next::SubmitSchool,
        },
        _ => Next::SelectSchool(schools),
    }
}

impl Root {
    /// The Join Hub button was pressed: the dialog opens with an empty field.
    pub(in crate::ui) fn hub_dialog_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let email = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("name@school.edu".to_owned());
            editor.set_single_line(true);
            editor
        });
        let events = cx.subscribe_in(&email, window, |this, _, event: &EditorEvent, _, cx| {
            match event {
                EditorEvent::Submit(_) => this.hub_dialog_submit(cx),
                EditorEvent::Cancel => {
                    this.hub_dialog_close();
                }
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        self.discover.servers.hub = Some(HubDialog {
            email,
            error: None,
            busy: false,
            _events: events,
        });
        cx.notify();
    }

    pub(in crate::ui) fn hub_dialog_close(&mut self) -> bool {
        self.discover.servers.hub.take().is_some()
    }

    /// The form's submit: the lookup goes out and the button shows its loading state until the answer.
    fn hub_dialog_submit(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.discover.servers.hub.as_mut() else {
            return;
        };
        if dialog.busy {
            return;
        }
        let email = dialog.email.read(cx).text().to_owned();
        dialog.busy = true;
        dialog.error = None;
        let _ = self.requests.send(lookup_request(&email, None));
    }

    /// The lookup answered: an error shows under the field; a match moves on to a step this client does not build yet.
    pub(super) fn hub_fetched(&mut self, key: &str) {
        let body = self.fetched.remove(key).unwrap_or_default();
        let Some(dialog) = self.discover.servers.hub.as_mut() else {
            return;
        };
        dialog.busy = false;
        if body["ok"] == true {
            // The school, PIN and waitlist steps were not captured (BLOCKED.md); the dialog stays on its field.
            let _ = next_step(&body["body"]);
        } else {
            dialog.error = Some(crate::ui::server_automod::any_error_message(&body["body"]));
        }
    }

    /// The Student Hubs section under the hero (`contentSection_YXXVAo`): the join card, then the notice about schools.
    pub(super) fn hubs_section(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let columns = Self::grid_columns(width - 40.);
        let card = div()
            .parity("discover-hub-card")
            .flex()
            .flex_col()
            .h(px(320.))
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .child(
                div()
                    .flex_none()
                    .size(px(48.))
                    .rounded(px(48.))
                    .bg(color("--green-360"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("discover_hub", 24., "--white")),
            )
            .child(
                text(20., 24., 600., "--text-strong")
                    .mt(px(16.))
                    .h(px(48.))
                    .child("Find study groups, clubs, and friends in the Student Hub for your school!"),
            )
            .child(
                text(14., 18., 400., "--text-default")
                    .mt(px(16.))
                    .h(px(36.))
                    .child(
                        "Meet classmates from your school, discover communities, and share your servers, all in one place.",
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("discover-hub-join")
                    .flex_none()
                    .h(px(40.))
                    .w_full()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-connected-border-default"))
                    .bg(color("--control-connected-background-default"))
                    .hover(|style| style.bg(color("--control-connected-background-hover")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--control-connected-text-default"))
                    .child("Join Hub")
                    .on_click(cx.listener(|this, _, window, cx| this.hub_dialog_open(window, cx))),
            );
        let hub_cards: Vec<AnyElement> = self
            .stores
            .guilds
            .iter()
            .filter(|g| g.features.iter().any(|f| f == "HUB"))
            .map(|hub| {
                let dir_ch = hub.channels.first().map(|c| c.id).unwrap_or(hub.id);
                let hub_id = hub.id;
                let hub_name = hub.name.clone();
                let hub_name_click = hub_name.clone();
                div()
                    .flex()
                    .flex_col()
                    .h(px(320.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .child(
                        div()
                            .flex_none()
                            .size(px(48.))
                            .rounded(px(48.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("discover_hub", 24., "--white")),
                    )
                    .child(
                        text(20., 24., 600., "--text-strong")
                            .mt(px(16.))
                            .child(hub_name),
                    )
                    .child(
                        text(14., 18., 400., "--text-default")
                            .mt(px(12.))
                            .child(dc_intl::t("pYFZ9p")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id(SharedString::from(format!("discover-hub-add-{hub_id}")))
                            .flex_none()
                            .h(px(40.))
                            .w_full()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-primary-border-default"))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("H9jxS1"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_hub_add_server_modal(
                                    dir_ch,
                                    hub_id,
                                    hub_name_click.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .pl(px(20.))
                    .pr(px(20.))
                    .pt(px(16.))
                    .pb(px(16.))
                    .grid()
                    .grid_cols(columns as u16)
                    .gap(px(16.))
                    .children(hub_cards)
                    .child(card),
            )
            .child(div().w_full().h(px(1.)).bg(color("--border-subtle")))
            .child(
                div()
                    .w_full()
                    .pl(px(32.))
                    .pt(px(24.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        text(14., 18., 500., "--text-default")
                            .w(px(400.))
                            .child("Hubs are not affiliated with or managed by schools."),
                    )
                    .child(
                        text(14., 18., 500., "--text-default").w(px(400.)).child(
                            "Servers listed in the school\u{2019}s Student Hub are run by students. Users do not need to be a member of a Student Hub to be invited to join a server listed there.",
                        ),
                    ),
            )
            .into_any_element()
    }

    /// The Join Hub dialog (`container_dvN-MP`, 480 wide).
    pub(in crate::ui) fn hub_dialog_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.discover.servers.hub.as_ref()?;
        let error = dialog.error.clone();
        let busy = dialog.busy;
        let field = div()
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(if error.is_some() {
                color("--input-border-error-default")
            } else {
                color("--input-border-default")
            })
            .bg(color("--input-background-default"))
            .pl(px(12.))
            .pr(px(11.))
            .flex()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(dialog.email.clone()));
        let terms = terms_text();
        let modal = div()
            .id("discover-hub-dialog")
            .parity_with_border("discover-hub-dialog", 1.)
            .occlude()
            .w(px(480.))
            .pt(px(8.))
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
            }])
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .h(px(72.))
                    .pl(px(24.))
                    .pt(px(16.))
                    .pr(px(24.))
                    .flex()
                    .child(
                        text(20., 24., 600., "--text-strong")
                            .flex_none()
                            .w(px(394.))
                            .child(
                                "Find study groups, clubs, and friends in the Student Hub for your school!",
                            ),
                    )
                    .child(
                        div()
                            .id("discover-hub-dialog-close")
                            .flex_none()
                            .mt(px(-16.))
                            .ml(px(12.))
                            .size(px(40.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.hub_dialog_close();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .pt(px(16.))
                    .pl(px(24.))
                    .pr(px(24.))
                    .flex()
                    .flex_col()
                    .child(
                        text(16., 20., 500., "--text-strong")
                            .h(px(20.))
                            .mb(px(8.))
                            .child("Your Student Email"),
                    )
                    .child(field)
                    .children(error.map(|message| {
                        text(12., 16., 400., "--text-feedback-critical")
                            .mt(px(8.))
                            .child(message)
                    }))
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(terms),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .mt(px(8.))
                    .pt(px(16.))
                    .pl(px(24.))
                    .pr(px(24.))
                    .pb(px(24.))
                    .child(
                        div()
                            .id("discover-hub-submit")
                            .h(px(40.))
                            .w_full()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-primary-border-default"))
                            .bg(color("--control-primary-background-default"))
                            .when(busy, |this| this.opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child("Join Hub")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.hub_dialog_submit(cx);
                                cx.notify();
                            })),
                    ),
            );
        let pose = crate::ui::motion::modal_pose("discover-hub");
        Some(
            deferred(
                div()
                    .id("discover-hub-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.hub_dialog_close();
                        cx.notify();
                    }))
                    .child(modal.posed(pose)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The editors the dialog owns, for the harness's `type` step.
    #[cfg(feature = "capture")]
    pub(in crate::ui) fn discover_editors(&self) -> Vec<Entity<Editor>> {
        self.discover
            .servers
            .hub
            .iter()
            .map(|dialog| dialog.email.clone())
            .collect()
    }
}

/// `cgT481`: the data notice with its two links.
fn terms_text() -> AnyElement {
    let parts: [(&str, Option<&'static str>); 5] = [
        ("Review Discord's ", None),
        ("Terms of Service", Some(TERMS)),
        (" and ", None),
        ("Privacy Policy", Some(PRIVACY)),
        (
            " to learn more about how we use your data. Hubs are not affiliated with or managed by schools. Servers listed in the school's Student Hub are run by students. Users do not need to be a member of a Student Hub to be invited to join a server listed there.",
            None,
        ),
    ];
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut links: Vec<(std::ops::Range<usize>, &'static str)> = Vec::new();
    for (part, link) in parts {
        let start = text.len();
        text.push_str(part);
        if let Some(url) = link {
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    color: Some(color("--text-link").into()),
                    ..Default::default()
                },
            ));
            links.push((start..text.len(), url));
        }
    }
    let ranges: Vec<_> = links.iter().map(|(range, _)| range.clone()).collect();
    InteractiveText::new(
        SharedString::from("discover-hub-terms"),
        StyledText::new(text).with_highlights(highlights),
    )
    .on_click(ranges, move |at, _, cx| {
        if let Some((_, url)) = links.get(at) {
            cx.open_url(url);
        }
    })
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lookup_is_the_documented_post() {
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = lookup_request("a@school.edu", None)
        else {
            panic!("a rest request");
        };
        assert_eq!(key.as_deref(), Some("discover:hub:lookup"));
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/guilds/automations/email-domain-lookup");
        assert_eq!(
            body,
            Some(
                json!({"email": "a@school.edu", "allow_multiple_guilds": true, "use_verification_code": true})
            )
        );
        let Request::Rest { body, .. } = lookup_request("a@school.edu", Some(42)) else {
            panic!("a rest request");
        };
        assert_eq!(body.unwrap()["guild_id"], json!("42"));
    }

    #[test]
    fn the_answer_picks_the_next_step() {
        assert_eq!(
            next_step(&json!({"has_matching_guild": true, "guilds_info": []})),
            Next::VerifyPin
        );
        assert_eq!(next_step(&json!({"guilds_info": []})), Next::SubmitSchool);
        assert_eq!(
            next_step(&json!({"guilds_info": [{"id": "7"}]})),
            Next::SendAgain(7)
        );
        assert!(matches!(
            next_step(&json!({"guilds_info": [{"id": "7"}, {"id": "8"}]})),
            Next::SelectSchool(schools) if schools.len() == 2
        ));
    }

    #[test]
    fn a_harness_run_refuses_the_hub_requests() {
        assert!(crate::net::refuses_in_capture(
            &reqwest::Method::POST,
            LOOKUP_ROUTE
        ));
        assert!(crate::net::refuses_in_capture(
            &reqwest::Method::POST,
            "/hub-waitlist/signup"
        ));
        assert!(!crate::net::refuses_in_capture(
            &reqwest::Method::GET,
            "/guilds/1/profile"
        ));
    }
}
