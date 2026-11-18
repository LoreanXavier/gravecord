//! Pending-member composer and member verification: 678655, 93895, 408213; P.T. screening DOM captures.
use super::{Root, color, icon};
use crate::ui::motion::Posed as _;
use crate::{net::Request, parity::Parity};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, ScrollHandle, div, prelude::*, px};
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Screening {
    guild: Id,
    form: Option<Value>,
    accepted: BTreeSet<usize>,
    /// Answers to the other field types, by field index: a string for TEXT_INPUT and PARAGRAPH, the chosen index
    /// for MULTIPLE_CHOICE.
    responses: BTreeMap<usize, Value>,
    scroll: ScrollHandle,
    sending: bool,
    error: Option<String>,
    measure_scroll: Cell<bool>,
}

/// Module 709977 `Ge`: whether a field has what it requires. TERMS need acceptance, the text types a non-blank
/// string, MULTIPLE_CHOICE a chosen index; an optional field always passes and an unknown required type does not.
fn answered(
    field: &Value,
    index: usize,
    accepted: &BTreeSet<usize>,
    responses: &BTreeMap<usize, Value>,
) -> bool {
    if !field["required"].as_bool().unwrap_or(false) {
        return true;
    }
    match field["field_type"].as_str() {
        Some("TERMS") => accepted.contains(&index),
        Some("TEXT_INPUT" | "PARAGRAPH") => responses
            .get(&index)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty()),
        Some("MULTIPLE_CHOICE") => responses.get(&index).is_some_and(Value::is_u64),
        _ => false,
    }
}

fn submission(
    form: &Value,
    accepted: &BTreeSet<usize>,
    responses: &BTreeMap<usize, Value>,
) -> Option<Value> {
    let fields = form["form_fields"].as_array()?;
    if !fields
        .iter()
        .enumerate()
        .all(|(index, field)| answered(field, index, accepted, responses))
    {
        return None;
    }
    let fields: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let mut field = field.clone();
            if field["field_type"] == "TERMS" {
                field["response"] = json!(accepted.contains(&index));
            } else if let Some(response) = responses.get(&index) {
                field["response"] = response.clone();
            }
            field
        })
        .collect();
    Some(json!({"version":form.get("version")?, "form_fields":fields}))
}

/// The keys of the account's own join-request calls: `join-request:{remove|reset}:{guild}`.
pub(super) const JOIN_REQUEST_KEY: &str = "join-request:";

/// What the pending member's bar says (module 678655 `render`, its `showMemberVerificationModal` branch, by the
/// account's own request's `applicationStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingBar {
    /// No request yet (or approved): "You must complete a few more steps before you can talk." and Complete.
    Complete,
    /// "Your application to join the server is awaiting review.", its subtitle and Withdraw Application.
    Submitted,
    /// "Your application to join the server has been rejected." and Learn more (module 267).
    Rejected,
}

pub(super) fn pending_bar(status: Option<&str>) -> PendingBar {
    match status {
        Some(dc_state::guild_join_requests::SUBMITTED) => PendingBar::Submitted,
        Some(dc_state::guild_join_requests::REJECTED) => PendingBar::Rejected,
        _ => PendingBar::Complete,
    }
}

/// Module 267 on the dialog of module 189213: "Your application to join {guildName} has been denied." (the
/// server's name unknown: "...the server..."), then "Reason:" (4px from the reason, `body-md/medium`,
/// `text-default`) when one was given; Reapply (`secondary`) and Okay (`critical-primary`).
pub(super) fn application_denied_alert(
    guild: Id,
    name: Option<String>,
    reason: Option<String>,
) -> super::chat_notices::Alert {
    use super::chat_notices::{Alert, AlertButton, Content, Then};
    use super::modal::Tone;
    Alert {
        id: "application-denied",
        title: match name {
            Some(name) => dc_intl::t!("P+/gzA", guildName = name).to_string(),
            None => dc_intl::t("gBPcuP").to_owned(),
        },
        subtitle: None,
        content: reason.map(Content::RejectionReason),
        buttons: vec![
            AlertButton {
                id: "application-denied-reapply",
                label: dc_intl::t("I1LYVk").to_owned(),
                tone: Tone::Secondary,
                then: Then::Reapply { guild },
            },
            AlertButton {
                id: "application-denied-okay",
                label: dc_intl::t("BddRzS").to_owned(),
                tone: Tone::Critical,
                then: Then::Close,
            },
        ],
    }
}

fn scroll_fader(root: &Root, distance: gpui::Pixels, top: bool) -> Option<gpui::Div> {
    if distance <= px(0.) {
        return None;
    }
    // Module 93895's mask stops. Cache a continuous image because GPUI gradients have only two stops.
    let curve: [(f32, f32); 13] = [
        (0., 0.),
        (0.19, 0.262),
        (0.34, 0.459),
        (0.47, 0.618),
        (0.57, 0.722),
        (0.65, 0.806),
        (0.73, 0.874),
        (0.8, 0.925),
        (0.86, 0.958),
        (0.91, 0.979),
        (0.95, 0.992),
        (0.98, 0.998),
        (1., 1.),
    ];
    let strength = (f32::from(distance) / 48.).min(1.);
    let stops: Vec<_> = if top {
        curve.to_vec()
    } else {
        curve
            .into_iter()
            .rev()
            .map(|(position, alpha)| (1. - position, alpha))
            .collect()
    };
    let token = super::theme().color("--background-surface-high");
    let key = format!("screening-fade:{top}:{token}");
    let cached = root.generated_images.borrow().get(&key).cloned();
    let picture = if let Some(picture) = cached {
        picture
    } else {
        let [red, green, blue, _] = token.to_be_bytes();
        let mut pixels = image::RgbaImage::new(534, 180);
        for y in 0..180 {
            let at = (y as f32 + 0.5) / 180.;
            let pair = stops.windows(2).find(|pair| at <= pair[1].0)?;
            let fraction = (at - pair[0].0) / (pair[1].0 - pair[0].0);
            let mask = pair[0].1 + fraction * (pair[1].1 - pair[0].1);
            let alpha = ((1. - mask) * 255.).round() as u8;
            for x in 0..534 {
                pixels.put_pixel(x, y, image::Rgba([red, green, blue, alpha]));
            }
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .ok()?;
        let picture = std::sync::Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            bytes.into_inner(),
        ));
        root.generated_images
            .borrow_mut()
            .insert(key, picture.clone());
        picture
    };
    Some(
        div()
            .absolute()
            .left(px(456.))
            .top(px(if top { 60. } else { 334. }))
            .w(px(534.))
            .h(px(180.))
            .opacity(strength)
            .child(gpui::img(picture).size_full()),
    )
}

impl Root {
    pub(super) fn screening_pending(&self) -> bool {
        self.current_guild().is_some_and(|guild| guild.my_pending)
    }

    pub(super) fn open_screening(&mut self, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else { return };
        self.screening = Some(Screening {
            guild,
            form: None,
            accepted: BTreeSet::new(),
            responses: BTreeMap::new(),
            scroll: ScrollHandle::new(),
            sending: false,
            error: None,
            measure_scroll: Cell::new(true),
        });
        for (prefix, route) in [
            (
                "screening-form",
                format!("/guilds/{guild}/member-verification?with_guild=false"),
            ),
            ("screening-preview", format!("/guilds/{guild}/preview")),
        ] {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{prefix}:{guild}")),
                method: reqwest::Method::GET,
                route,
                body: None,
            });
        }
        cx.notify();
    }

    pub(super) fn screening_fetched(
        &mut self,
        key: &str,
        report: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if !key.starts_with("screening-") {
            return false;
        }
        if key.starts_with("screening-preview:") {
            // A failed preview is not a preview: the card keeps what the guild list already knows.
            if report["ok"].as_bool() == Some(true) {
                self.fetched.insert(key.to_owned(), report.clone());
            }
        } else if let Some(open) = self.screening.as_mut()
            && key.rsplit(':').next().and_then(|id| id.parse::<Id>().ok()) == Some(open.guild)
        {
            open.sending = false;
            if report["ok"].as_bool() == Some(true) {
                if key.starts_with("screening-submit:") {
                    self.screening = None;
                } else {
                    open.form = Some(report["body"].clone());
                    open.measure_scroll.set(true);
                }
            } else {
                open.error = Some(
                    report["body"]["message"]
                        .as_str()
                        .unwrap_or("Something went wrong")
                        .to_owned(),
                );
            }
        }
        cx.notify();
        true
    }

    /// The account's own join request in the open server, if any (`UserGuildJoinRequestStore.getRequest`).
    fn own_join_status(&self) -> Option<String> {
        let guild = self.guild?;
        self.stores
            .join_requests
            .get_request(guild)
            .and_then(|request| request.application_status.clone())
    }

    /// Withdraw Application (`handleCancelApplication` of module 678655): "Are you sure?" on the alert of module
    /// 732159, Cancel and Confirm.
    fn ask_withdraw_application(&mut self, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else { return };
        self.chat_extra.alert = Some(super::chat_notices::Alert::show(
            "withdraw-application",
            dc_intl::t("aIz1oV"),
            Some(dc_intl::t("13tjTU").to_owned()),
            Some(dc_intl::t("ETE/oC").to_owned()),
            Some(dc_intl::t("cY+Oob").to_owned()),
            super::chat_notices::Then::WithdrawApplication { guild },
        ));
        cx.notify();
    }

    /// `removeGuildJoinRequest` (module 624458): `DELETE /guilds/{guild}/requests/@me`, then
    /// `USER_GUILD_JOIN_REQUEST_UPDATE {request: null}`.
    pub(super) fn remove_join_request(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{JOIN_REQUEST_KEY}remove:{guild}")),
            method: reqwest::Method::DELETE,
            route: format!("/guilds/{guild}/requests/@me"),
            body: None,
        });
    }

    /// `resetGuildJoinRequest` (module 624458): `POST /guilds/{guild}/requests/@me`; the answer is the new request.
    pub(super) fn reset_join_request(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{JOIN_REQUEST_KEY}reset:{guild}")),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/requests/@me"),
            body: None,
        });
    }

    /// The answers of [`Self::remove_join_request`] and [`Self::reset_join_request`]: on success the store takes
    /// `USER_GUILD_JOIN_REQUEST_UPDATE`. Reapply of a pending member opens the verification form once the old request
    /// is gone (module 267's `Ze`).
    pub(super) fn join_request_answered(
        &mut self,
        key: &str,
        report: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = key.strip_prefix(JOIN_REQUEST_KEY) else {
            return false;
        };
        let (kind, guild) = rest.split_once(':').unwrap_or((rest, ""));
        let Ok(guild) = guild.parse::<Id>() else {
            return true;
        };
        if report["ok"].as_bool() != Some(true) {
            return true;
        }
        let request = if kind == "reset" {
            report["body"].clone()
        } else {
            Value::Null
        };
        self.stores.dispatch(
            "USER_GUILD_JOIN_REQUEST_UPDATE",
            &serde_json::json!({"guildId": guild.to_string(), "request": request}),
        );
        if kind == "remove"
            && self.reapply_after_remove.take() == Some(guild)
            && self.guild == Some(guild)
        {
            self.open_screening(cx);
        }
        cx.notify();
        true
    }

    /// "Your application to join {guild} has been denied." (module 267): the reason under it when the moderators
    /// gave one; Reapply and Okay.
    pub(super) fn show_application_denied(&mut self, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else { return };
        let name = self.stores.guild(guild).map(|known| known.name.clone());
        let reason = self
            .stores
            .join_requests
            .get_request(guild)
            .and_then(|request| request.rejection_reason.clone())
            .filter(|reason| !reason.is_empty());
        self.chat_extra.alert = Some(application_denied_alert(guild, name, reason));
        cx.notify();
    }

    /// Reapply of module 267: a pending member's request is removed and the form opens again; anyone else resets it.
    pub(super) fn reapply(&mut self, guild: Id) {
        let pending = self
            .stores
            .guild(guild)
            .is_some_and(|known| known.my_pending);
        if pending {
            self.reapply_after_remove = Some(guild);
            self.remove_join_request(guild);
        } else {
            self.reset_join_request(guild);
        }
    }

    /// Capture only: the open server becomes one this account is a pending member of, with its own request in the
    /// given status (`none` drops it) and an optional rejection reason.
    pub(crate) fn script_join_request(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let Some(guild) = self.guild else {
            return "no server open".to_owned();
        };
        if let Some(known) = self
            .stores
            .guilds
            .iter_mut()
            .find(|known| known.id == guild)
        {
            known.my_pending = true;
        }
        let status = words.first().copied().unwrap_or("none");
        let reason = words
            .get(1..)
            .map(|rest| rest.join(" "))
            .unwrap_or_default();
        let request = if status == "none" {
            Value::Null
        } else {
            json!({
                "join_request_id": "1", "guild_id": guild.to_string(), "user_id": null,
                "application_status": status, "form_responses": [],
                "rejection_reason": if reason.is_empty() { Value::Null } else { json!(reason) },
            })
        };
        self.stores.dispatch(
            "USER_GUILD_JOIN_REQUEST_UPDATE",
            &json!({"guildId": guild.to_string(), "request": request}),
        );
        cx.notify();
        format!("{status} in {guild}")
    }

    pub(super) fn screening_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let status = self.own_join_status();
        let bar = pending_bar(status.as_deref());
        let (message, subtitle, label) = match bar {
            PendingBar::Complete => (dc_intl::t("rEBKvg"), None, dc_intl::t("r8/DT+")),
            PendingBar::Submitted => (
                dc_intl::t("5iLvSx"),
                Some(dc_intl::t("FdsK4h")),
                dc_intl::t("mqtdmQ"),
            ),
            PendingBar::Rejected => (dc_intl::t("lk30cY"), None, dc_intl::t("8RrsHr")),
        };
        let primary = bar == PendingBar::Complete;
        div()
            .relative()
            .flex_none()
            .mt(px(-16.))
            .px(px(8.))
            .pb(px(8.))
            .children(self.typing_bar(cx))
            .children(self.slowmode_notice(cx))
            .child(
                div()
                    .parity_with_border("screening-composer", 1.)
                    .min_h(px(58.))
                    .py(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-low"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .ml(px(16.))
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(message),
                            )
                            // `body-xs/normal` in `text-muted` (module 786051).
                            .children(subtitle.map(|subtitle| {
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(subtitle)
                            })),
                    )
                    .child({
                        // Complete is `primary`; the application states keep module 786051's `secondary`.
                        let (border, background, text) = if primary {
                            (
                                "--control-primary-border-default",
                                "--control-primary-background-default",
                                "--control-primary-text-default",
                            )
                        } else {
                            (
                                "--control-secondary-border-default",
                                "--control-secondary-background-default",
                                "--control-secondary-text-default",
                            )
                        };
                        div()
                            .id("screening-open")
                            .flex_none()
                            .ml(px(12.))
                            .mr(px(12.))
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color(border))
                            .bg(color(background))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color(text))
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| match bar {
                                PendingBar::Complete => this.open_screening(cx),
                                PendingBar::Submitted => this.ask_withdraw_application(cx),
                                PendingBar::Rejected => this.show_application_denied(cx),
                            }))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn screening_layer(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.screening.as_ref()?;
        if open.form.is_some() && open.measure_scroll.replace(false) {
            window.request_animation_frame();
        }
        let guild = open.guild;
        let card = self.invite_preview_sized(guild, None, None, None, 282., cx);
        let mut palette = self
            .images
            .get(&format!("guild:{guild}"))
            .and_then(|picture| image::load_from_memory(picture.bytes()).ok())
            .map(|picture| {
                crate::palette::avatar_palette(
                    picture
                        .resize_exact(40, 40, image::imageops::FilterType::Triangle)
                        .to_rgba8()
                        .as_raw(),
                )
            })
            .unwrap_or_default();
        if let Some(primary) = palette.first().copied() {
            palette.truncate(2);
            palette.extend(crate::palette::complementary_colors(primary));
        }
        let palette_color = |at, fallback| {
            palette.get(at).map_or_else(
                || color(fallback),
                |rgb: &[u8; 3]| {
                    gpui::rgb(
                        (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2]),
                    )
                },
            )
        };
        let sidebar_gradient = gpui::linear_gradient(
            -45.,
            gpui::linear_color_stop(palette_color(0, "--background-base-lower"), 0.),
            gpui::linear_color_stop(palette_color(2, "--background-base-lowest"), 1.),
        );
        let form = open.form.as_ref();
        let fields = form.and_then(|form| form["form_fields"].as_array());
        let enabled = !open.sending
            && form
                .and_then(|form| submission(form, &open.accepted, &open.responses))
                .is_some();
        let questions = fields
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(index, field)| {
                let terms = field["field_type"] == "TERMS";
                let checked = open.accepted.contains(&index);
                let rows = field["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter_map(|(at, value)| {
                        let text = value.as_str()?;
                        Some(
                            div()
                                .flex()
                                .pt(px(if at == 0 { 8. } else { 16. }))
                                .pb(px(16.))
                                .px(px(4.))
                                .when(at > 0, |row| {
                                    row.border_t_1().border_color(color("--border-subtle"))
                                })
                                .child(
                                    div()
                                        .text_color(color("--text-muted"))
                                        .child(format!("{}.", at + 1)),
                                )
                                .child(
                                    div().pl(px(12.)).flex_1().min_w(px(0.)).child(
                                        crate::inline::inline(
                                            super::first_spans(text)
                                                .iter()
                                                .map(|span| {
                                                    self.piece(
                                                        span,
                                                        super::hsla("--text-default"),
                                                        16.,
                                                        400.,
                                                        16.,
                                                    )
                                                })
                                                .collect(),
                                            px(20.),
                                        ),
                                    ),
                                ),
                        )
                    });
                div()
                    .mt(px(40.))
                    .p(px(1.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(div().mb(px(12.)).child(if terms {
                        "Agree to rules".to_owned()
                    } else {
                        field["label"].as_str().unwrap_or_default().to_owned()
                    }))
                    .child(
                        div()
                            .px(px(17.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .children(rows),
                    )
                    .when(terms, |row| {
                        row.child(
                            div()
                                .id(gpui::SharedString::from(format!("screening-term-{index}")))
                                .mt(px(16.))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(open) = this.screening.as_mut()
                                        && !open.sending
                                        && !open.accepted.remove(&index)
                                    {
                                        open.accepted.insert(index);
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .size(px(20.))
                                        .flex_none()
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(color("--checkbox-border-default"))
                                        .bg(color(if checked {
                                            "--checkbox-background-selected-default"
                                        } else {
                                            "--checkbox-background-default"
                                        }))
                                        .when(checked, |box_| {
                                            box_.child(icon("checkbox_check", 18., "--white"))
                                        }),
                                )
                                .child("I have read and agree to the rules"),
                        )
                    })
            });
        let pose = crate::ui::motion::modal_pose("screening");
        Some(gpui::deferred(div().id("screening-layer").absolute().inset_0().occlude()
            .bg(pose.dim(color("--background-scrim"))).flex().items_center().justify_center()
            .on_click(cx.listener(|this, _, _, cx| { this.screening = None; cx.notify(); }))
            .child(div().id("screening-modal").posed(pose).parity_with_border("screening-modal", 1.)
                .relative().w(px(1024.)).h(px(608.)).rounded(px(16.)).border_1()
                .border_color(color("--border-subtle")).bg(color("--background-surface-high"))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(div().absolute().left(px(16.)).top(px(8.)).w(px(400.)).h(px(590.))
                    .rounded(px(8.)).flex().items_center().justify_center()
                    .child(div().absolute().inset_0().rounded(px(8.)).opacity(0.33).bg(sidebar_gradient))
                    .child(div().parity("screening-guild-card").children(card)))
                .child(div().parity("screening-form-scroll")
                    .absolute().left(px(456.)).top(px(60.)).w(px(534.)).h(px(454.))
                    .child(div().id("screening-form-scroll").size_full().overflow_y_scroll().track_scroll(&open.scroll)
                    .child(div().pt(px(8.)).pb(px(20.))
                        .child(div().text_size(px(32.)).line_height(px(40.)).font_weight(FontWeight(500.))
                            .text_color(color("--text-strong")).child("Before you can talk here..."))
                        .child(div().mt(px(8.)).text_size(px(14.)).line_height(px(18.))
                            .text_color(color("--text-default")).child("You’ll need to complete the required steps below."))
                        .children(questions))))
                .children(scroll_fader(self, -open.scroll.offset().y, true))
                .children(scroll_fader(self, open.scroll.max_offset().y + open.scroll.offset().y, false))
                .children(open.error.as_ref().map(|error| div().absolute().left(px(456.)).bottom(px(76.))
                    .text_size(px(14.)).text_color(color("--text-feedback-critical")).child(error.clone())))
                .child(div().id("screening-submit").parity("screening-submit").absolute().right(px(32.)).bottom(px(44.))
                    .h(px(40.)).w(px(100.)).rounded(px(8.)).bg(color("--control-connected-background-default"))
                    .when(!enabled, |button| button.opacity(0.5)).when(enabled, |button| button.cursor_pointer())
                    .flex().items_center().justify_center().text_size(px(16.)).line_height(px(20.))
                    .font_weight(FontWeight(500.)).text_color(color("--white")).child("Submit")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !enabled { return }
                        if let Some(open) = this.screening.as_mut()
                            && !open.sending
                            && let Some(body) = open.form.as_ref().and_then(|form| submission(form, &open.accepted, &open.responses))
                        {
                            open.sending = true;
                            open.error = None;
                            let _ = this.requests.send(Request::Rest { key:Some(format!("screening-submit:{guild}")),
                                method:reqwest::Method::PUT, route:format!("/guilds/{guild}/requests/@me"), body:Some(body) });
                        }
                        cx.notify();
                    })))
                .child(div().id("screening-close").parity("screening-close").absolute().top(px(32.)).right(px(32.)).size(px(40.))
                    .cursor_pointer().flex().items_center().justify_center().child(icon("screening_close", 20., "--interactive-text-default"))
                    .on_click(cx.listener(|this, _, _, cx| { this.screening = None; cx.notify(); }))))
        ).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_pending_bar_follows_the_accounts_own_request() {
        assert_eq!(pending_bar(None), PendingBar::Complete);
        assert_eq!(pending_bar(Some("APPROVED")), PendingBar::Complete);
        assert_eq!(pending_bar(Some("SUBMITTED")), PendingBar::Submitted);
        assert_eq!(pending_bar(Some("REJECTED")), PendingBar::Rejected);
        let denied = application_denied_alert(5, Some("Fixture".into()), Some("No".into()));
        assert_eq!(
            denied.title,
            "Your application to join Fixture has been denied."
        );
        assert_eq!(denied.buttons.len(), 2);
        assert_eq!(denied.buttons[1].tone, super::super::modal::Tone::Critical);
        let unnamed = application_denied_alert(5, None, None);
        assert!(unnamed.content.is_none());
        assert_eq!(
            unnamed.title,
            "Your application to join the server has been denied."
        );
    }

    #[test]
    fn required_terms_gate_submission_and_preserve_the_form_version() {
        let form = json!({"version":"v1", "form_fields":[
            {"field_type":"TERMS", "required":true,"values":["Rule one"]},
            {"field_type":"TERMS", "required":true,"values":["Rule two"]}
        ]});
        let none = BTreeMap::new();
        assert!(submission(&form, &BTreeSet::new(), &none).is_none());
        assert!(submission(&form, &BTreeSet::from([0]), &none).is_none());
        let sent = submission(&form, &BTreeSet::from([0, 1]), &none).unwrap();
        assert_eq!(sent["version"], "v1");
        assert_eq!(sent["form_fields"][0]["response"], true);
        assert_eq!(sent["form_fields"][1]["values"][0], "Rule two");
    }
    #[test]
    fn every_field_type_follows_the_official_answer_rule() {
        let none = BTreeMap::new();
        let accepted = BTreeSet::new();
        let form = |field: Value| json!({"version":"v1","form_fields":[field]});
        // TEXT_INPUT and PARAGRAPH need a non-blank string.
        for kind in ["TEXT_INPUT", "PARAGRAPH"] {
            let form = form(json!({"field_type":kind,"required":true}));
            assert!(submission(&form, &accepted, &none).is_none());
            assert!(submission(&form, &accepted, &BTreeMap::from([(0, json!("   "))])).is_none());
            let sent =
                submission(&form, &accepted, &BTreeMap::from([(0, json!("hello"))])).unwrap();
            assert_eq!(sent["form_fields"][0]["response"], "hello");
        }
        // MULTIPLE_CHOICE needs a chosen index, zero included.
        let choice =
            form(json!({"field_type":"MULTIPLE_CHOICE","required":true,"values":["a","b"]}));
        assert!(submission(&choice, &accepted, &none).is_none());
        assert!(submission(&choice, &accepted, &BTreeMap::from([(0, json!("0"))])).is_none());
        let sent = submission(&choice, &accepted, &BTreeMap::from([(0, json!(0))])).unwrap();
        assert_eq!(sent["form_fields"][0]["response"], 0);
        // Optional fields may stay empty and then carry no response.
        let optional = form(json!({"field_type":"PARAGRAPH","required":false}));
        let sent = submission(&optional, &accepted, &none).unwrap();
        assert!(sent["form_fields"][0].get("response").is_none());
        // A required type the client does not know cannot be answered.
        let unknown = form(json!({"field_type":"SOMETHING_NEW","required":true}));
        assert!(submission(&unknown, &accepted, &none).is_none());
        // Mixed with terms, every required field must be answered.
        let mixed = json!({"version":"v2","form_fields":[
            {"field_type":"TERMS","required":true,"values":["Rule"]},
            {"field_type":"TEXT_INPUT","required":true}
        ]});
        assert!(submission(&mixed, &BTreeSet::from([0]), &none).is_none());
        let sent = submission(
            &mixed,
            &BTreeSet::from([0]),
            &BTreeMap::from([(1, json!("x"))]),
        )
        .unwrap();
        assert_eq!(sent["form_fields"][0]["response"], true);
        assert_eq!(sent["form_fields"][1]["response"], "x");
    }
}
