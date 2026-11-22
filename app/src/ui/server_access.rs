//! Server Settings > Access (module 989433 `In`, `A7`, `AY`, `AL`, `AS`, save `Il`; store module 306846; requests module
//! 408213; rules module 241499; question rows module 797446; dialogs modules 542717, 520660, 24094).
//!
//! The choice between Invite Only and Apply to Join stays local until Save Changes. Invite Only saves its rules, Apply to
//! Join its questions, both with `PATCH /guilds/{id}/member-verification`; Apply from Profile saves the profile's
//! visibility. Discoverable needs Community, so its settings are not drawn yet, and the warning dialog a server with
//! pending applications gets before its questions change (modules 78903, 990424) is not either.
//! Measured in `design/dom/settings/guild-access-darker-cozy.json`, `-rules-`, `-apply-`, and
//! `modals/form-field-*`, `modals/verification-preview-darker-cozy.json`, `popouts/add-question-menu-darker-cozy.json`.

use super::tooltip::{Body, Side, Tip};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::ui::motion::Posed as _;
use dc_state::Id;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, IntoElement, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use serde_json::{Value, json};

pub(super) const ACCESS_PREFIX: &str = "guild-access:";
const SAVED_PREFIX: &str = "guild-access:saved:";
/// Module 652215's `MEMBER_APPLICATIONS` article.
const LEARN_MORE: &str = "https://support.discord.com/hc/en-us/articles/29729107418519";
const MAX_RULES: usize = 16;
const MAX_QUESTIONS: usize = 5;
const MAX_CHOICES: usize = 8;
/// Module 280513 `n`.
const VISIBILITY_PUBLIC: u64 = 1;
const VISIBILITY_RECRUITING: u64 = 3;
/// `9suSIA`: the rules field's label on the wire.
const TERMS_LABEL: &str = "Read and agree to the server rules";
/// `83ZsRS`: the question Apply to Join starts with.
const FIRST_QUESTION: &str = "Why do you want to join our server?";
const EXAMPLE_RULES: [(&str, &str); 4] = [
    (
        "Be civil and respectful",
        "Treat everyone with respect. Absolutely no harassment, witch hunting, sexism, racism, or hate speech will be tolerated.",
    ),
    (
        "No spam or self-promotion",
        "No spam or self-promotion (server invites, advertisements, etc) without permission from a staff member. This includes DMing fellow members.",
    ),
    (
        "No age-restricted or obscene content",
        "No age-restricted or obscene content. This includes text, images, or links featuring nudity, sex, hard violence, or other graphically disturbing content.",
    ),
    (
        "Help keep things safe",
        "If you see something against the rules or something that makes you feel unsafe, let staff know. We want this server to be a welcoming space!",
    ),
];
const EXAMPLE_QUESTIONS: [&str; 3] = [
    "Do you play any of the same games as us?",
    "How did you find us?",
    "What makes you unique?",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JoinType {
    Invite,
    Apply,
    Discoverable,
}

/// A dragged row; the drop target moves it to its own place.
struct RuleDrag(usize);
struct QuestionDrag(usize);
struct ChoiceDrag(usize);

/// What follows the cursor while dragging: the row itself stays where it is drawn until the drop.
struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Moves one item of a list to another's place, as the rows' `hover` and `drop` do.
fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from < items.len() && to < items.len() && from != to {
        let item = items.remove(from);
        items.insert(to, item);
    }
}

/// `N`: how the server is joined today.
fn current_join(features: &[String]) -> JoinType {
    let has = |name: &str| features.iter().any(|feature| feature == name);
    if has("DISCOVERABLE") {
        JoinType::Discoverable
    } else if has("MEMBER_VERIFICATION_GATE_ENABLED") && has("MEMBER_VERIFICATION_MANUAL_APPROVAL")
    {
        JoinType::Apply
    } else {
        JoinType::Invite
    }
}

fn is_terms(field: &Value) -> bool {
    field["field_type"] == "TERMS"
}

/// `m`: the rules of the saved form, or one empty rule.
fn rules_of(form: &[Value]) -> Vec<String> {
    match form.iter().find(|field| is_terms(field)) {
        Some(terms) => terms["values"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        None => vec![String::new()],
    }
}

/// `C`: the saved questions, or the first question when there are none but the rules.
fn questions_of(form: &[Value]) -> Vec<Value> {
    let terms = form.iter().find(|field| is_terms(field));
    if form.is_empty() || (form.len() == 1 && terms.is_some()) {
        let first = json!({"field_type": "TEXT_INPUT", "label": FIRST_QUESTION, "required": true});
        return terms.cloned().into_iter().chain([first]).collect();
    }
    form.to_vec()
}

/// `N` in the save: what Invite Only sends, nothing when there are no rules.
fn terms_fields(rules: &[String]) -> Vec<Value> {
    if rules.is_empty() {
        Vec::new()
    } else {
        vec![json!({
            "field_type": "TERMS",
            "label": TERMS_LABEL,
            "values": rules,
            "required": true,
        })]
    }
}

fn trimmed(rules: &[String]) -> Vec<String> {
    rules
        .iter()
        .map(|rule| rule.trim().to_owned())
        .filter(|rule| !rule.is_empty())
        .collect()
}

/// `L`: whether Invite Only has anything to save in the form.
fn invite_form_dirty(
    features: &[String],
    form: &[Value],
    require_terms: bool,
    rules: &[String],
) -> bool {
    let has = |name: &str| features.iter().any(|feature| feature == name);
    if has("MEMBER_VERIFICATION_MANUAL_APPROVAL")
        || has("MEMBER_VERIFICATION_GATE_ENABLED") != require_terms
        || form.len() > 1
    {
        return true;
    }
    let saved: Vec<String> = form
        .iter()
        .find(|field| is_terms(field))
        .map(|field| trimmed(&rules_of(std::slice::from_ref(field))))
        .unwrap_or_default();
    saved != trimmed(rules)
}

/// `R`'s Apply to Join case for the form.
fn apply_form_dirty(features: &[String], form: &[Value], questions: &[Value]) -> bool {
    let has = |name: &str| features.iter().any(|feature| feature == name);
    !has("MEMBER_VERIFICATION_GATE_ENABLED")
        || !has("MEMBER_VERIFICATION_MANUAL_APPROVAL")
        || form != questions
}

/// One rule's field.
pub(crate) struct Rule {
    pub(super) editor: Entity<Editor>,
    _events: Subscription,
}

/// One of the dialogs that add or edit a question.
pub(crate) struct FieldDialog {
    /// `TEXT_INPUT`, `PARAGRAPH`, `MULTIPLE_CHOICE` or `TERMS`.
    kind: &'static str,
    /// The question being edited.
    index: Option<usize>,
    pub(super) label: Option<Entity<Editor>>,
    pub(super) choices: Vec<Entity<Editor>>,
    rules: Vec<Rule>,
    error: Option<String>,
    _events: Vec<Subscription>,
}

/// The page's pending state (`GuildSettingsJoinRulesStore`).
pub(crate) struct AccessState {
    /// The saved form, once it has been read.
    form: Option<Vec<Value>>,
    join: JoinType,
    require_terms: bool,
    rules: Vec<Rule>,
    questions: Vec<Value>,
    /// The profile's visibility, saved and chosen.
    visibility: Option<(u64, u64)>,
    error: Option<String>,
    menu: Option<gpui::Point<gpui::Pixels>>,
    dialog: Option<FieldDialog>,
    preview: bool,
}

impl AccessState {
    /// The fields open now, for typing in tests.
    #[cfg(feature = "capture")]
    pub(super) fn editors(&self) -> Vec<Entity<Editor>> {
        let mut list: Vec<Entity<Editor>> =
            self.rules.iter().map(|rule| rule.editor.clone()).collect();
        if let Some(dialog) = self.dialog.as_ref() {
            list.extend(dialog.label.clone());
            list.extend(dialog.choices.iter().cloned());
            list.extend(dialog.rules.iter().map(|rule| rule.editor.clone()));
        }
        list
    }
}

/// What the unsaved-changes bar says on the Access page, and what its save button reads (`tv.A`'s props in `Is`/`Il`).
pub(super) struct AccessBar {
    pub message: String,
    /// The save button's label and whether it is disabled; none when the bar can only be reset.
    pub save: Option<(&'static str, bool)>,
}

impl Root {
    /// The bar's words for Discoverable, which differ from the default ones.
    pub(super) fn access_bar(&self, guild: Id, cx: &Context<Self>) -> Option<AccessBar> {
        let state = self.server_settings.as_ref()?.access.as_ref()?;
        if state.join != JoinType::Discoverable {
            return None;
        }
        match self.discovery_view(guild)? {
            super::server_discovery::View::Ineligible => Some(AccessBar {
                message: "You cannot set your server to Discoverable because it does not meet requirements."
                    .to_owned(),
                save: None,
            }),
            super::server_discovery::View::EligibleDisabled => Some(AccessBar {
                message: "Fill in the required information to list on Discovery.".to_owned(),
                save: Some(("List on Discovery", !self.discovery_listable(cx))),
            }),
            _ => None,
        }
    }

    /// What keeps Save Changes from going through, in the bar's words.
    pub(super) fn access_error_text(&self) -> Option<String> {
        let open = self.server_settings.as_ref()?;
        let state = open.access.as_ref()?;
        state.error.clone().or_else(|| {
            (state.join == JoinType::Discoverable)
                .then(|| open.discovery.error.clone())
                .flatten()
        })
    }

    fn access_mut(&mut self) -> Option<&mut AccessState> {
        self.server_settings.as_mut()?.access.as_mut()
    }

    fn make_rule(&mut self, text: &str, cx: &mut Context<Self>) -> Rule {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Enter a rule".to_owned());
            editor.set_text(text);
            editor
        });
        let events = cx.subscribe(&editor, |_, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                EditorEvent::Edited => {
                    // `maxCharacterCount: 300`.
                    let text = editor.read(cx).text().to_owned();
                    if text.chars().count() > 300 {
                        let cut: String = text.chars().take(300).collect();
                        editor.update(cx, |editor, _| editor.set_text(&cut));
                    }
                }
                EditorEvent::Cancel => {}
            }
            cx.notify();
        });
        Rule {
            editor,
            _events: events,
        }
    }

    /// Opening the page: the choice follows the server, then the form is read (`fetchVerificationForm`).
    pub(super) fn request_access(&mut self, guild: Id, cx: &mut Context<Self>) {
        let features = self
            .stores
            .guild(guild)
            .map(|known| known.features.clone())
            .unwrap_or_default();
        let visibility = self
            .server_settings
            .as_ref()
            .map(|open| open.visibility_now());
        if let Some(open) = self.server_settings.as_mut() {
            open.access = Some(AccessState {
                form: None,
                join: current_join(&features),
                require_terms: features
                    .iter()
                    .any(|feature| feature == "MEMBER_VERIFICATION_GATE_ENABLED"),
                rules: Vec::new(),
                questions: Vec::new(),
                visibility: visibility.map(|visibility| (visibility, visibility)),
                error: None,
                menu: None,
                dialog: None,
                preview: false,
            });
        }
        self.request_access_form(guild);
        self.request_discovery_art();
        if current_join(&features) == JoinType::Discoverable {
            self.request_discovery(guild);
        }
        cx.notify();
    }

    pub(super) fn request_access_form(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{ACCESS_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route: format!("/guilds/{guild}/member-verification?with_guild=false"),
            body: None,
        });
    }

    pub(super) fn access_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        if let Some(guild) = key.strip_prefix(SAVED_PREFIX) {
            if report["ok"] == true {
                if let Ok(guild) = guild.parse::<Id>() {
                    self.request_access_form(guild);
                }
            } else if let Some(state) = self.access_mut() {
                state.error = Some(
                    report["body"]["message"]
                        .as_str()
                        .unwrap_or("Oops! You've caught an ultra rare error. This is probably our fault, so please try again or check our status page.")
                        .to_owned(),
                );
            }
        } else if key.strip_prefix(ACCESS_PREFIX).is_some() {
            // No form yet is a 404: `MEMBER_VERIFICATION_FORM_FETCH_FAIL` leaves the fields empty.
            let form: Vec<Value> = report["body"]["form_fields"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            self.apply_access_form(form, cx);
        }
        cx.notify();
    }

    /// `y`: the pending rules and questions follow the saved form; the choice stays.
    fn apply_access_form(&mut self, form: Vec<Value>, cx: &mut Context<Self>) {
        let rules: Vec<Rule> = rules_of(&form)
            .iter()
            .map(|rule| self.make_rule(rule, cx))
            .collect();
        if let Some(state) = self.access_mut() {
            state.rules = rules;
            state.questions = questions_of(&form);
            state.form = Some(form);
            state.error = None;
        }
    }

    fn access_rule_texts(&self, cx: &Context<Self>) -> Vec<String> {
        self.server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
            .map(|state| {
                state
                    .rules
                    .iter()
                    .map(|rule| rule.editor.read(cx).text().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn access_dirty(&self, guild: Id, cx: &Context<Self>) -> bool {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
        else {
            return false;
        };
        let Some(form) = state.form.as_ref() else {
            return false;
        };
        let features = self
            .stores
            .guild(guild)
            .map(|known| known.features.clone())
            .unwrap_or_default();
        let joined_differently = current_join(&features) != state.join;
        joined_differently
            || match state.join {
                JoinType::Invite => invite_form_dirty(
                    &features,
                    form,
                    state.require_terms,
                    &self.access_rule_texts(cx),
                ),
                JoinType::Discoverable => {
                    invite_form_dirty(
                        &features,
                        form,
                        state.require_terms,
                        &self.access_rule_texts(cx),
                    ) || self.discovery_dirty()
                }
                JoinType::Apply => {
                    apply_form_dirty(&features, form, &state.questions)
                        || state
                            .visibility
                            .is_some_and(|(saved, chosen)| saved != chosen)
                }
            }
    }

    /// Reset: `ex.A.init` runs the page's opening again.
    pub(super) fn reset_access(&mut self, guild: Id, cx: &mut Context<Self>) {
        self.request_access(guild, cx);
    }

    /// `I`, then `A`: the checks, then what each way of joining saves.
    pub(super) fn save_access(&mut self, guild: Id, cx: &mut Context<Self>) {
        let rules = trimmed(&self.access_rule_texts(cx));
        let features = self
            .stores
            .guild(guild)
            .map(|known| known.features.clone())
            .unwrap_or_default();
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
        else {
            return;
        };
        let (join, require_terms, questions, visibility, form) = (
            state.join,
            state.require_terms,
            state.questions.clone(),
            state.visibility,
            state.form.clone().unwrap_or_default(),
        );
        let refused = match join {
            JoinType::Invite | JoinType::Discoverable if require_terms && rules.is_empty() => {
                Some("You need to add one or more rules.")
            }
            JoinType::Apply if !questions.iter().any(|field| !is_terms(field)) => {
                Some("Add a non-server rules question to enable Apply to Join.")
            }
            _ => None,
        };
        if let Some(message) = refused {
            if let Some(state) = self.access_mut() {
                state.error = Some(message.to_owned());
            }
            cx.notify();
            return;
        }
        if let Some(state) = self.access_mut() {
            state.error = None;
        }
        // `Il`: listing a server first asks for the three promises (module 883494).
        if join == JoinType::Discoverable
            && self.discovery_view(guild) == Some(super::server_discovery::View::EligibleDisabled)
            && !self.discovery_agreed(guild)
        {
            self.discovery_ask_agreement();
            cx.notify();
            return;
        }
        if features.iter().any(|feature| feature == "DISCOVERABLE")
            && join != JoinType::Discoverable
        {
            let kept: Vec<&String> = features
                .iter()
                .filter(|feature| *feature != "DISCOVERABLE")
                .collect();
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{SAVED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}"),
                body: Some(json!({ "features": kept })),
            });
        }
        let body = match join {
            JoinType::Invite | JoinType::Discoverable => {
                invite_form_dirty(&features, &form, require_terms, &rules)
                    .then(|| json!({"form_fields": terms_fields(&rules), "enabled": require_terms}))
            }
            JoinType::Apply => apply_form_dirty(&features, &form, &questions)
                .then(|| json!({"form_fields": questions, "enabled": true})),
        };
        if let Some(body) = body {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{SAVED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}/member-verification"),
                body: Some(body),
            });
        }
        if join == JoinType::Discoverable {
            self.save_discovery(guild);
        }
        if join == JoinType::Apply
            && let Some((saved, chosen)) = visibility
            && saved != chosen
        {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{SAVED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}/profile"),
                body: Some(json!({ "visibility": chosen })),
            });
            if let Some(open) = self.server_settings.as_mut() {
                open.set_saved_visibility(chosen);
            }
            if let Some(state) = self.access_mut() {
                state.visibility = Some((chosen, chosen));
            }
        }
        cx.notify();
    }

    fn pick_join(&mut self, join: JoinType, cx: &mut Context<Self>) {
        if let Some(state) = self.access_mut() {
            state.join = join;
            state.error = None;
        }
        if join == JoinType::Discoverable
            && let Some(guild) = self.server_settings.as_ref().map(|open| open.guild)
        {
            self.request_discovery(guild);
        }
        cx.notify();
    }

    /// `A7`: the three ways of joining in their row.
    fn access_join_cards(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let current = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
            .map_or(JoinType::Invite, |state| state.join);
        let known = self.stores.guild(guild);
        let community = known.is_some_and(|known| known.features.iter().any(|f| f == "COMMUNITY"));
        let admin = known.zip(self.stores.me()).is_some_and(|(known, me)| {
            dc_state::permissions::can_in_guild(known, me, dc_state::permissions::ADMINISTRATOR)
        });
        let locked_in = current == JoinType::Discoverable && !admin;
        let cards: [(JoinType, &str, &str, &str, bool, &str); 3] = [
            (
                JoinType::Invite,
                "Invite Only",
                "People can join your server directly with an invite",
                "access_lock",
                !locked_in,
                "Only server administrators can turn off Server Discovery",
            ),
            (
                JoinType::Apply,
                "Apply to Join",
                "People must submit an application and be approved to join",
                "access_envelope",
                !locked_in,
                "Only server administrators can turn off Server Discovery",
            ),
            (
                JoinType::Discoverable,
                "Discoverable",
                "Anyone can join your server directly through Server Discovery",
                "access_globe",
                community && (current == JoinType::Discoverable || admin),
                if community {
                    "Only server administrators can turn on Server Discovery"
                } else {
                    "Enable Community to access Server Discovery"
                },
            ),
        ];
        let mut row = div()
            .h(px(152.))
            .p(px(8.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-muted"))
            .bg(color("--background-base-lower"))
            .flex()
            .gap(px(8.));
        for (index, (join, title, body, glyph, enabled, why)) in cards.into_iter().enumerate() {
            let selected = join == current;
            let tint = if selected {
                "--interactive-text-active"
            } else {
                "--interactive-text-default"
            };
            let card = div()
                .id(SharedString::from(format!("access-join-{index}")))
                // The row's 642 between its 8 gaps.
                .w(px(208.67))
                .h(px(134.))
                .pt(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(if selected {
                    color("--border-normal")
                } else {
                    gpui::rgba(0)
                })
                .when(selected, |this| this.bg(color("--background-mod-normal")))
                .when(!selected && enabled, |this| {
                    this.hover(|style| style.bg(color("--background-mod-subtle")))
                })
                .when(!enabled, |this| this.opacity(0.5))
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| this.pick_join(join, cx)))
                })
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .child(icon(glyph, 24., tint))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color(tint))
                        .child(title),
                )
                .child(
                    div()
                        // Between the capture's widest line (173) and the next word's break.
                        .w(px(176.))
                        .text_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color(tint))
                        .child(body),
                );
            row = row.child(if enabled {
                card.into_any_element()
            } else {
                self.with_tip(
                    format!("access-join-tip-{index}"),
                    Tip::body(Body::Text(why.into()), Side::Bottom),
                    card,
                    cx,
                )
                .into_any_element()
            });
        }
        row.into_any_element()
    }

    /// `Ay`: opens the applicant's view of the form.
    fn access_preview_link(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("access-preview")
            .flex()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-brand"))
                    .child("Preview"),
            )
            .child(icon("access_launch", 16., "--text-brand"))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.access_mut() {
                    state.preview = true;
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn access_heading(text: &'static str) -> gpui::Div {
        div()
            .text_size(px(14.))
            .line_height(px(18.67))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-muted"))
            .child(text)
    }

    fn access_add_row(id: &'static str, text: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(52.))
            .rounded(px(8.))
            .border_2()
            .border_dashed()
            .border_color(color("--border-subtle"))
            .hover(|style| style.border_color(color("--border-strong")))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .child(icon("access_plus", 24., "--interactive-text-active"))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(text),
            )
    }

    fn access_pill(id: SharedString, text: &'static str, rules: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(36.))
            .px(px(12.))
            .rounded_full()
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(if rules {
                color("--background-base-lowest")
            } else {
                color("--background-base-lower")
            })
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .cursor_pointer()
            .flex()
            .flex_none()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-subtle"))
            .child(text)
    }

    /// Module 241499: the rules, Add a rule and the examples, on the page or in the dialog.
    fn access_rules_editor(
        &self,
        in_dialog: bool,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref());
        let rules: &[Rule] = state
            .map(|state| {
                if in_dialog {
                    state
                        .dialog
                        .as_ref()
                        .map_or(&[][..], |dialog| &dialog.rules[..])
                } else {
                    &state.rules[..]
                }
            })
            .unwrap_or(&[]);
        let many = rules.len() > 1;
        let count = rules.len();
        let group_name = if in_dialog { "dialog" } else { "page" };
        let mut list = div().flex().flex_col().gap(px(12.)).my(px(12.));
        for (index, rule) in rules.iter().enumerate() {
            let group = SharedString::from(format!("access-rule-{group_name}-{index}"));
            let editor = rule.editor.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!(
                        "access-rule-row-{group_name}-{index}"
                    )))
                    .group(group.clone())
                    .relative()
                    .min_h(px(58.))
                    .py(px(17.))
                    .pl(px(27.))
                    .pr(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--input-background-default"))
                    .when(enabled, |this| {
                        this.cursor_text()
                            .on_click(cx.listener(move |_, _, window, cx| {
                                let handle = gpui::Focusable::focus_handle(editor.read(cx), cx);
                                window.focus(&handle, cx);
                            }))
                    })
                    .when(many && enabled, |this| {
                        this.on_drag(RuleDrag(index), |_, _, _, cx| cx.new(|_| DragGhost))
                            .on_drop(cx.listener(move |this, drag: &RuleDrag, _, cx| {
                                this.move_rule(in_dialog, drag.0, index, cx);
                            }))
                    })
                    .child(div().w_full().child(rule.editor.clone()))
                    .when(many && enabled, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(px(4.))
                                .top(px(20.))
                                .cursor_grab()
                                .child(icon("role_drag", 16., "--icon-muted")),
                        )
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "access-rule-clear-{group_name}-{index}"
                            )))
                            .absolute()
                            .right(px(8.))
                            .top(px(12.))
                            .size(px(32.))
                            .rounded(px(8.))
                            .opacity(0.)
                            .group_hover(group, |style| style.opacity(1.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("circle_x_small", 16., "--interactive-text-default"))
                            .when(enabled, |this| {
                                this.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.clear_rule(in_dialog, index, cx);
                                }))
                            }),
                    ),
            );
        }
        let full = count == MAX_RULES;
        let mut pills = div().flex().flex_wrap().gap(px(8.));
        for (index, (label, text)) in EXAMPLE_RULES.into_iter().enumerate() {
            let pill = Self::access_pill(
                SharedString::from(format!("access-rule-pill-{group_name}-{index}")),
                label,
                true,
            )
            .when(full, |this| this.opacity(0.8))
            .when(enabled && !full, |this| {
                this.on_click(cx.listener(move |this, _, _, cx| {
                    this.add_rule(in_dialog, Some(text), cx);
                }))
            });
            pills = pills.child(if full {
                self.with_tip(
                    format!("access-rule-pill-tip-{group_name}-{index}"),
                    Tip::text("You can only add up to 16 rules", Side::Top),
                    pill,
                    cx,
                )
                .into_any_element()
            } else {
                pill.into_any_element()
            });
        }
        div()
            .child(list)
            .when(!full, |this| {
                this.child(
                    Self::access_add_row(
                        if in_dialog {
                            "access-add-rule-dialog"
                        } else {
                            "access-add-rule"
                        },
                        "Add a rule",
                    )
                    .when(enabled, |this| {
                        this.on_click(cx.listener(move |this, _, _, cx| {
                            this.add_rule(in_dialog, None, cx);
                        }))
                    }),
                )
            })
            .child(Self::access_heading("Example rules").mt(px(24.)).mb(px(8.)))
            .child(pills)
            .into_any_element()
    }

    fn with_rules(&mut self, in_dialog: bool, run: impl FnOnce(&mut Vec<Rule>)) {
        if let Some(state) = self.access_mut() {
            if in_dialog {
                if let Some(dialog) = state.dialog.as_mut() {
                    run(&mut dialog.rules);
                }
            } else {
                run(&mut state.rules);
            }
        }
    }

    fn move_rule(&mut self, in_dialog: bool, from: usize, to: usize, cx: &mut Context<Self>) {
        self.with_rules(in_dialog, |rules| move_item(rules, from, to));
        cx.notify();
    }

    /// The clear button: the last rule stays as an empty one.
    fn clear_rule(&mut self, in_dialog: bool, index: usize, cx: &mut Context<Self>) {
        let empty = self.make_rule("", cx);
        self.with_rules(in_dialog, move |rules| {
            if index < rules.len() {
                rules.remove(index);
            }
            if rules.is_empty() {
                rules.push(empty);
            }
        });
        cx.notify();
    }

    /// `C`: an example fills an empty last rule, or goes after it.
    fn add_rule(&mut self, in_dialog: bool, text: Option<&str>, cx: &mut Context<Self>) {
        let state = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref());
        let rules = state.map(|state| {
            if in_dialog {
                state
                    .dialog
                    .as_ref()
                    .map_or(&[][..], |dialog| &dialog.rules[..])
            } else {
                &state.rules[..]
            }
        });
        let last = rules
            .and_then(|rules| rules.last())
            .map(|rule| rule.editor.clone());
        if let (Some(text), Some(last)) = (text, last.as_ref())
            && last.read(cx).text().is_empty()
        {
            last.update(cx, |editor, _| editor.set_text(text));
            cx.notify();
            return;
        }
        let rule = self.make_rule(text.unwrap_or(""), cx);
        self.with_rules(in_dialog, move |rules| {
            if rules.len() < MAX_RULES {
                rules.push(rule);
            }
        });
        cx.notify();
    }

    /// `AY`: Invite Only's section.
    pub(super) fn access_invite_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
            .is_some_and(|state| state.require_terms);
        div()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .w(px(516.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child("Server Rules"),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child("Members must agree to rules before they can chat or interact in the server."),
                            ),
                    )
                    .child(
                        div()
                            .id("access-rules-switch")
                            .cursor_pointer()
                            .child(super::create_channel::switch(on))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(state) = this.access_mut() {
                                    state.require_terms = !state.require_terms;
                                    state.error = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .when(on, |this| this.child(self.access_preview_link(cx)))
            .child(
                div()
                    .mt(px(24.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .when(!on, |this| this.opacity(0.5))
                    .child(Self::access_heading("Rules"))
                    .child(self.access_rules_editor(false, on, cx)),
            )
            .into_any_element()
    }

    /// One question (`A`, `compactContainer`): the title with its kind, the grip left of it, the tools right of it.
    fn access_question_row(
        &self,
        index: usize,
        field: &Value,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind = field["field_type"].as_str().unwrap_or_default();
        let (glyph, tag) = match kind {
            "MULTIPLE_CHOICE" => ("access_list", "Multiple Choice"),
            "PARAGRAPH" => ("access_paragraph", "Paragraph"),
            "TERMS" => ("access_rules", "Server Rules"),
            _ => ("access_short", "Short Answer"),
        };
        let title = if kind == "TERMS" {
            "Read and Agree to Server Rules".to_owned()
        } else {
            field["label"].as_str().unwrap_or_default().to_owned()
        };
        let can_remove = count > 1;
        let group = SharedString::from(format!("access-question-{index}"));
        let tool = |id: String, glyph: &'static str| {
            div()
                .id(SharedString::from(id))
                .size(px(32.))
                .rounded_full()
                .bg(color("--background-base-lowest"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(glyph, 16., "--interactive-text-default"))
        };
        let edited = field.clone();
        div()
            .id(SharedString::from(format!("access-question-row-{index}")))
            .group(group.clone())
            .relative()
            .mt(px(8.))
            .when(count > 1, |this| {
                this.on_drag(QuestionDrag(index), |_, _, _, cx| cx.new(|_| DragGhost))
                    .on_drop(cx.listener(move |this, drag: &QuestionDrag, _, cx| {
                        if let Some(state) = this.access_mut() {
                            move_item(&mut state.questions, drag.0, index);
                        }
                        cx.notify();
                    }))
            })
            .child(
                div()
                    .h(px(52.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(4.))
                            .child(icon(glyph, 16., "--interactive-text-default"))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(tag),
                            ),
                    ),
            )
            .when(count > 1, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(-21.))
                        .top(px(18.))
                        .opacity(0.)
                        .group_hover(group.clone(), |style| style.opacity(1.))
                        .cursor_grab()
                        .child(icon("role_drag", 16., "--primary-400")),
                )
            })
            .child(
                div()
                    .absolute()
                    .right(px(if can_remove { -88. } else { -48. }))
                    .top(px(2.))
                    .p(px(8.))
                    .flex()
                    .gap(px(8.))
                    .opacity(0.)
                    .group_hover(group, |style| style.opacity(1.))
                    .child(
                        tool(format!("access-question-edit-{index}"), "sound_edit").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open_field_dialog(
                                    Some((index, edited.clone())),
                                    None,
                                    window,
                                    cx,
                                );
                            }),
                        ),
                    )
                    .when(can_remove, |this| {
                        this.child(
                            tool(
                                format!("access-question-remove-{index}"),
                                "channel_settings_trash",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(state) = this.access_mut()
                                        && index < state.questions.len()
                                    {
                                        state.questions.remove(index);
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                    }),
            )
            .into_any_element()
    }

    /// `AL`: Apply to Join's section.
    fn access_apply_section(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
        else {
            return div().into_any_element();
        };
        let questions = state.questions.clone();
        let (_, visibility) = state.visibility.unwrap_or((0, 0));
        let count = questions.len();
        let full = count == MAX_QUESTIONS;
        let rows: Vec<AnyElement> = questions
            .iter()
            .enumerate()
            .map(|(index, field)| self.access_question_row(index, field, count, cx))
            .collect();
        let mut form = div()
            .my(px(24.))
            .child(Self::access_heading_owned(format!(
                "Questions ({count}/{MAX_QUESTIONS})"
            )))
            .children(rows);
        if !full {
            let mut pills = div().flex().flex_wrap().gap(px(8.));
            for (index, label) in EXAMPLE_QUESTIONS.into_iter().enumerate() {
                pills = pills.child(
                    Self::access_pill(
                        SharedString::from(format!("access-question-pill-{index}")),
                        label,
                        false,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.access_mut() {
                            state.questions.push(json!({
                                "field_type": "TEXT_INPUT",
                                "label": label,
                                "required": true,
                            }));
                        }
                        cx.notify();
                    })),
                );
            }
            let add = Self::access_add_row("access-add-question", "Add a question").on_click(
                cx.listener(|this, event: &ClickEvent, _, cx| {
                    if let Some(state) = this.access_mut() {
                        state.menu = Some(event.position());
                    }
                    cx.notify();
                }),
            );
            form = form
                .child(div().mt(px(8.)).child(self.with_tip(
                    "access-add-question-tip",
                    Tip::text("You can add up to 5 questions.", Side::Bottom),
                    add,
                    cx,
                )))
                .child(
                    Self::access_heading("Example questions")
                        .mt(px(24.))
                        .mb(px(8.)),
                )
                .child(pills);
        }
        // `a`: Apply from Profile needs a visible profile.
        let visible = matches!(visibility, VISIBILITY_PUBLIC | VISIBILITY_RECRUITING);
        let recruiting = visibility == VISIBILITY_RECRUITING;
        // `buttonContainer`: 16 at its sides and below.
        let button = div()
            .mt(px(16.))
            .px(px(16.))
            .pb(px(16.))
            .child(
                div()
                    .h(px(32.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-connected-border-default"))
                    .bg(color("--control-connected-background-default"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child("Apply to Join"),
            )
            .into_any_element();
        let card = self.invite_preview_sized(guild, None, None, Some(button), 314., cx);
        let profile_tab = self.text_with_link(
            "access-profile-tab",
            "You can\u{2019}t enable Apply from Profile if your server has \u{201c}Private Profile\u{201d} enabled. You can change this setting in the ",
            "Profile tab",
            ".",
            cx,
            |this, _| {
                if let Some(open) = this.server_settings.as_mut() {
                    open.page = super::server_settings::Page::Profile;
                }
            },
        );
        div()
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Join Application"),
            )
            .child(
                div()
                    .my(px(8.))
                    // 660 in the capture; GPUI's advance is slightly wider, so the lines break as captured.
                    .w(px(666.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-subtle"))
                    .child("Add questions to your application. People don't join your server until you approve their application. You'll have the option to DM applicants before approving or denying them."),
            )
            .child(self.access_preview_link(cx))
            .child(form)
            .child(
                div()
                    .my(px(32.))
                    .h(px(1.))
                    .bg(color("--border-strong")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(32.))
                    .child(
                        div()
                            .w(px(314.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .when(!visible, |this| this.opacity(0.5))
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child("Apply from Profile"),
                                    )
                                    .child(
                                        div()
                                            .id("access-apply-profile")
                                            .when(!visible, |this| this.opacity(0.5))
                                            .when(visible, |this| {
                                                this.cursor_pointer().on_click(cx.listener(
                                                    |this, _, _, cx| {
                                                        if let Some(state) = this.access_mut()
                                                            && let Some((_, chosen)) =
                                                                state.visibility.as_mut()
                                                        {
                                                            *chosen = if *chosen
                                                                == VISIBILITY_RECRUITING
                                                            {
                                                                VISIBILITY_PUBLIC
                                                            } else {
                                                                VISIBILITY_RECRUITING
                                                            };
                                                        }
                                                        cx.notify();
                                                    },
                                                ))
                                            })
                                            .child(super::create_channel::switch(recruiting)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child("Allow people who see your server profile to apply."),
                            )
                            .when(!visible, |this| {
                                this.child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(profile_tab),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w(px(314.))
                            .flex_none()
                            .relative()
                            .h(px(100.))
                            .overflow_hidden()
                            .children(card.map(|card| {
                                div()
                                    .absolute()
                                    .left(px(0.))
                                    .bottom(px(0.))
                                    .w(px(314.))
                                    .child(card)
                            }))
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .bg(gpui::linear_gradient(
                                        180.,
                                        gpui::linear_color_stop(color("--background-base-low"), 0.),
                                        gpui::linear_color_stop(
                                            color("--background-base-low").opacity(0.),
                                            0.7,
                                        ),
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn access_heading_owned(text: String) -> gpui::Div {
        div()
            .text_size(px(14.))
            .line_height(px(18.67))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-muted"))
            .child(text)
    }

    /// The page (`In`).
    pub(super) fn server_access_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
        else {
            return div().into_any_element();
        };
        let loaded = state.form.is_some();
        let join = state.join;
        let cards = self.access_join_cards(guild, cx);
        let description = self.text_with_link(
            "access-learn-more",
            "Keep your server private, or open it up for more people to join. ",
            "Learn More.",
            "",
            cx,
            |_, cx| cx.open_url(LEARN_MORE),
        );
        let section: Option<AnyElement> = loaded.then(|| match join {
            JoinType::Invite => self.access_invite_section(cx),
            JoinType::Discoverable => self.access_discovery_section(guild, cx),
            JoinType::Apply => self.access_apply_section(guild, cx),
        });
        div()
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Access"),
            )
            .child(
                div()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("How can people join your server?"),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .mb(px(16.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-subtle"))
                            .child(description),
                    )
                    .child(cards),
            )
            .child(div().h(px(1.)).bg(color("--border-subtle")))
            .children(section)
            .into_any_element()
    }

    /// The menu "Add a question" opens at the click, above it and centred (`jA` with `top`, `center`).
    pub(super) fn access_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let at = self.server_settings.as_ref()?.access.as_ref()?.menu?;
        let allow_terms = !self
            .server_settings
            .as_ref()?
            .access
            .as_ref()?
            .questions
            .iter()
            .any(is_terms);
        let items: [(&str, &str, &'static str, &'static str, bool); 4] = [
            (
                "access-menu-text",
                "Short Answer",
                "access_short",
                "TEXT_INPUT",
                true,
            ),
            (
                "access-menu-paragraph",
                "Paragraph",
                "access_paragraph",
                "PARAGRAPH",
                true,
            ),
            (
                "access-menu-choice",
                "Multiple Choice",
                "access_list",
                "MULTIPLE_CHOICE",
                true,
            ),
            (
                "access-menu-terms",
                "Server Rules",
                "access_rules",
                "TERMS",
                allow_terms,
            ),
        ];
        let height = 18. + 36. * items.iter().filter(|item| item.4).count() as f32;
        let mut menu = div()
            .id("access-question-menu")
            .occlude()
            .absolute()
            .left(at.x - px(78.5))
            .top(at.y - px(height))
            .w(px(157.))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.access_mut() {
                    state.menu = None;
                }
                cx.notify();
            }));
        for (id, label, glyph, kind, shown) in items {
            if !shown {
                continue;
            }
            menu = menu.child(
                div()
                    .id(id)
                    .h(px(36.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(glyph, 20., "--text-default"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(label),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(state) = this.access_mut() {
                            state.menu = None;
                        }
                        this.open_field_dialog(None, Some(kind), window, cx);
                    })),
            );
        }
        Some(gpui::deferred(menu).with_priority(2).into_any_element())
    }

    fn field_editor(
        &self,
        placeholder: String,
        text: &str,
        limit: usize,
        submits: bool,
        cx: &mut Context<Self>,
    ) -> (Entity<Editor>, Subscription) {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder(placeholder);
            editor.set_text(text);
            editor
        });
        let events = cx.subscribe(&editor, move |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Cancel => {
                    if let Some(state) = this.access_mut() {
                        state.dialog = None;
                    }
                }
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                    if submits {
                        this.save_field_dialog(cx);
                    }
                }
                EditorEvent::Edited => {
                    let text = editor.read(cx).text().to_owned();
                    if text.chars().count() > limit {
                        let cut: String = text.chars().take(limit).collect();
                        editor.update(cx, |editor, _| editor.set_text(&cut));
                    }
                    if let Some(dialog) = this.access_mut().and_then(|state| state.dialog.as_mut())
                    {
                        dialog.error = None;
                    }
                }
            }
            cx.notify();
        });
        (editor, events)
    }

    /// `_` to add and `k` to edit.
    fn open_field_dialog(
        &mut self,
        existing: Option<(usize, Value)>,
        kind: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (index, field) = match existing {
            Some((index, field)) => (Some(index), field),
            None => (None, Value::Null),
        };
        let kind: &'static str = match kind.or(field["field_type"].as_str()) {
            Some("PARAGRAPH") => "PARAGRAPH",
            Some("MULTIPLE_CHOICE") => "MULTIPLE_CHOICE",
            Some("TERMS") => "TERMS",
            _ => "TEXT_INPUT",
        };
        let mut events = Vec::new();
        let label = (kind != "TERMS").then(|| {
            let (editor, subscription) = self.field_editor(
                "Enter your question".to_owned(),
                field["label"].as_str().unwrap_or_default(),
                300,
                kind != "MULTIPLE_CHOICE",
                cx,
            );
            events.push(subscription);
            editor
        });
        let mut choices = Vec::new();
        if kind == "MULTIPLE_CHOICE" {
            let saved: Vec<String> = field["choices"]
                .as_array()
                .map(|choices| {
                    choices
                        .iter()
                        .filter_map(|choice| choice.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_else(|| vec![String::new()]);
            for (at, choice) in saved.iter().enumerate() {
                let (editor, subscription) =
                    self.field_editor(format!("Option {}", at + 1), choice, 150, false, cx);
                events.push(subscription);
                choices.push(editor);
            }
        }
        let rules: Vec<Rule> = if kind == "TERMS" {
            let values = field["values"].as_array().cloned().unwrap_or_default();
            let texts: Vec<String> = if values.is_empty() {
                vec![String::new()]
            } else {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            };
            texts.iter().map(|text| self.make_rule(text, cx)).collect()
        } else {
            Vec::new()
        };
        if let Some(editor) = label.as_ref() {
            let handle = gpui::Focusable::focus_handle(editor.read(cx), cx);
            window.focus(&handle, cx);
        }
        if let Some(state) = self.access_mut() {
            state.dialog = Some(FieldDialog {
                kind,
                index,
                label,
                choices,
                rules,
                error: None,
                _events: events,
            });
        }
        cx.notify();
    }

    /// `w`, and the choice and rules dialogs' `onConfirm`.
    fn save_field_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
            .and_then(|state| state.dialog.as_ref())
        else {
            return;
        };
        let label = dialog
            .label
            .as_ref()
            .map(|editor| editor.read(cx).text().trim().to_owned());
        let index = dialog.index;
        let built = match dialog.kind {
            "TERMS" => {
                let rules: Vec<String> = dialog
                    .rules
                    .iter()
                    .map(|rule| rule.editor.read(cx).text().to_owned())
                    .collect();
                let rules = trimmed(&rules);
                if rules.is_empty() {
                    Err("You need to add one or more rules.")
                } else {
                    Ok(json!({
                        "field_type": "TERMS",
                        "label": TERMS_LABEL,
                        "values": rules,
                        "required": true,
                    }))
                }
            }
            _ if label.as_deref() == Some("") => Err("You must enter a question."),
            "MULTIPLE_CHOICE" => {
                let choices: Vec<String> = dialog
                    .choices
                    .iter()
                    .map(|editor| editor.read(cx).text().to_owned())
                    .collect();
                if trimmed(&choices).is_empty() {
                    Err("You need at least two or more choices.")
                } else {
                    Ok(json!({
                        "field_type": "MULTIPLE_CHOICE",
                        "label": label,
                        "choices": choices,
                        "required": true,
                    }))
                }
            }
            kind => Ok(json!({"field_type": kind, "label": label, "required": true})),
        };
        if let Some(state) = self.access_mut() {
            match built {
                Ok(field) => {
                    match index {
                        Some(index) if index < state.questions.len() => {
                            state.questions[index] = field;
                        }
                        _ => state.questions.push(field),
                    }
                    state.dialog = None;
                }
                Err(message) => {
                    if let Some(dialog) = state.dialog.as_mut() {
                        dialog.error = Some(message.to_owned());
                    }
                }
            }
        }
        cx.notify();
    }

    fn add_choice(&mut self, cx: &mut Context<Self>) {
        let at = self
            .server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
            .and_then(|state| state.dialog.as_ref())
            .map_or(0, |dialog| dialog.choices.len());
        if at >= MAX_CHOICES {
            return;
        }
        let (editor, subscription) =
            self.field_editor(format!("Option {}", at + 1), "", 150, false, cx);
        if let Some(dialog) = self.access_mut().and_then(|state| state.dialog.as_mut()) {
            dialog.choices.push(editor);
            dialog._events.push(subscription);
        }
        cx.notify();
    }

    /// The dialog's fields: the question, then the options; or the rules editor.
    pub(super) fn access_dialog_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.server_settings.as_ref()?.access.as_ref()?;
        let dialog = state.dialog.as_ref()?;
        let (title, subtitle): (&str, Option<&str>) = match dialog.kind {
            "PARAGRAPH" => (
                "Paragraph",
                Some("A free-form response with a 1,000 character limit. Let them go wild."),
            ),
            "MULTIPLE_CHOICE" => ("Multiple Choice", None),
            "TERMS" => ("Server Rules", None),
            _ => (
                "Short Answer",
                Some("A free-form response with a 150 character limit."),
            ),
        };
        let rules_body = (dialog.kind == "TERMS").then(|| self.access_rules_editor(true, true, cx));
        let input = |editor: &Entity<Editor>, id: SharedString| {
            let focused = editor.clone();
            div()
                .id(id)
                .h(px(40.))
                .px(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .cursor_text()
                .flex()
                .items_center()
                .on_click(cx.listener(move |_, _, window, cx| {
                    let handle = gpui::Focusable::focus_handle(focused.read(cx), cx);
                    window.focus(&handle, cx);
                }))
                .child(div().w_full().child(editor.clone()))
        };
        let mut body = div().flex().flex_col();
        body = body.children(rules_body);
        if let Some(label) = dialog.label.as_ref() {
            body = body.child(input(label, "access-field-label".into()));
        }
        if dialog.kind == "MULTIPLE_CHOICE" {
            body = body.child(
                div()
                    .mt(px(24.))
                    .mb(px(24.))
                    .h(px(1.))
                    .bg(color("--border-subtle")),
            );
            let count = dialog.choices.len();
            for (index, choice) in dialog.choices.iter().enumerate() {
                let group = SharedString::from(format!("access-choice-{index}"));
                body = body.child(
                    div()
                        .id(SharedString::from(format!("access-choice-row-{index}")))
                        .group(group.clone())
                        .h(px(52.))
                        .flex()
                        .gap(px(0.))
                        .on_drag(ChoiceDrag(index), |_, _, _, cx| cx.new(|_| DragGhost))
                        .on_drop(cx.listener(move |this, drag: &ChoiceDrag, _, cx| {
                            if let Some(dialog) =
                                this.access_mut().and_then(|state| state.dialog.as_mut())
                            {
                                move_item(&mut dialog.choices, drag.0, index);
                            }
                            cx.notify();
                        }))
                        .child(div().w(px(30.)).pt(px(10.)).flex_none().child(icon(
                            "access_radio",
                            20.,
                            "--interactive-text-default",
                        )))
                        .child(
                            div()
                                .relative()
                                .w(px(376.))
                                .flex_none()
                                .child(input(choice, format!("access-choice-{index}-field").into()))
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "access-choice-clear-{index}"
                                        )))
                                        .absolute()
                                        .right(px(8.))
                                        .top(px(6.))
                                        .size(px(32.))
                                        .rounded(px(8.))
                                        .opacity(0.)
                                        .group_hover(group, |style| style.opacity(1.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "circle_x_small",
                                            16.,
                                            "--interactive-text-default",
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.clear_choice(index, cx);
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .w(px(24.))
                                .pt(px(12.))
                                .pl(px(8.))
                                .flex_none()
                                .cursor_grab()
                                .child(icon("role_drag", 16., "--icon-muted")),
                        ),
                );
            }
            if count != MAX_CHOICES {
                body = body.child(
                    div()
                        .id("access-add-choice")
                        .mt(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .child(icon("access_circle_plus", 17., "--text-link"))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-link"))
                                .child("Add an option"),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.add_choice(cx))),
                );
            }
        }
        if let Some(error) = dialog.error.clone() {
            body = body.child(
                div()
                    .mt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error),
            );
        }
        let close = |this: &mut Root| {
            if let Some(state) = this.access_mut() {
                state.dialog = None;
            }
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "access-field-modal",
                title: title.to_owned().into(),
                subtitle: subtitle.map(|text| text.into_any_element()),
                body: Some(body.into_any_element()),
                buttons: vec![
                    super::modal::button(
                        "access-field-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "access-field-save",
                        "Save",
                        super::modal::Tone::Primary,
                        |this, _, cx| this.save_field_dialog(cx),
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }

    /// The X on an option: the last one stays as an empty one.
    fn clear_choice(&mut self, index: usize, cx: &mut Context<Self>) {
        let (editor, subscription) = self.field_editor("Option 1".to_owned(), "", 150, false, cx);
        if let Some(dialog) = self.access_mut().and_then(|state| state.dialog.as_mut()) {
            if index < dialog.choices.len() {
                dialog.choices.remove(index);
            }
            if dialog.choices.is_empty() {
                dialog.choices.push(editor);
                dialog._events.push(subscription);
            }
        }
        cx.notify();
    }

    /// Whether the applicant's view is open.
    pub(super) fn access_preview_open(&self) -> bool {
        self.server_settings
            .as_ref()
            .and_then(|open| open.access.as_ref())
            .is_some_and(|state| state.preview)
    }

    pub(super) fn close_access_overlay(&mut self) -> bool {
        let Some(state) = self.access_mut() else {
            return false;
        };
        if state.preview {
            state.preview = false;
            true
        } else if state.dialog.is_some() {
            state.dialog = None;
            true
        } else if state.menu.is_some() {
            state.menu = None;
            true
        } else {
            false
        }
    }

    /// `verification-preview`: the applicant's view of the questions, for looking only; Submit stays off.
    pub(super) fn access_preview_modal(
        &self,
        guild: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.access_preview_open() {
            return None;
        }
        let state = self.server_settings.as_ref()?.access.as_ref()?;
        let name = self.stores.guild(guild)?.name.clone();
        let fields: Vec<Value> = match state.join {
            JoinType::Apply => state.questions.clone(),
            _ => terms_fields(&trimmed(&self.access_rule_texts(cx))),
        };
        let card = self.invite_preview_sized(guild, None, None, None, 282., cx);
        let mut questions = div().flex().flex_col().gap(px(16.));
        for field in &fields {
            let label = if is_terms(field) {
                "Read and Agree to Server Rules".to_owned()
            } else {
                field["label"].as_str().unwrap_or_default().to_owned()
            };
            let kind = field["field_type"].as_str().unwrap_or_default();
            let mut block = div().child(
                div()
                    .mb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label),
            );
            block = match kind {
                "MULTIPLE_CHOICE" => block.children(
                    field["choices"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|choice| choice.as_str().map(str::to_owned))
                        .map(|choice| {
                            div()
                                .h(px(32.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(icon("access_radio", 20., "--interactive-text-default"))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(choice),
                                )
                        }),
                ),
                "TERMS" => block.children(
                    field["values"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|value| value.as_str().map(str::to_owned))
                        .enumerate()
                        .map(|(at, rule)| {
                            div()
                                .mt(px(4.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(format!("{}. {rule}", at + 1))
                        }),
                ),
                _ => block.child(
                    div()
                        .h(px(40.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--input-placeholder-text-default"))
                        .child("Enter your answer"),
                ),
            };
            questions = questions.child(block);
        }
        let close = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.close_access_overlay();
            cx.notify();
        });
        let close_scrim = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.close_access_overlay();
            cx.notify();
        });
        let pose = crate::ui::motion::modal_pose("access-preview");
        Some(
            gpui::deferred(
                div()
                    .id("access-preview-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(color("--background-scrim")))
                    .px(px(24.))
                    .py(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(close_scrim)
                    .child(
                        div()
                            .posed(pose)
                            .id("access-preview-modal")
                            .occlude()
                            .relative()
                            .w_full()
                            .max_w(px(1024.))
                            .min_h(px(430.))
                            .max_h_full()
                            .overflow_hidden()
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .flex()
                            .items_center()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .ml(px(48.))
                                    .w(px(282.))
                                    .flex_none()
                                    .children(card),
                            )
                            .child(
                                div()
                                    .ml(px(48.))
                                    .mr(px(32.))
                                    .flex_1()
                                    .min_w(px(0.))
                                    .max_w(px(534.))
                                    .py(px(48.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(32.))
                                            .line_height(px(40.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(format!("Answer a few questions to join {name}")),
                                    )
                                    .child(
                                        div()
                                            .mt(px(8.))
                                            .mb(px(32.))
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child("The server admin will get back to you shortly after you submit."),
                                    )
                                    .child(questions)
                                    .child(
                                        div()
                                            .mt(px(32.))
                                            .flex()
                                            .items_center()
                                            .gap(px(32.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-default"))
                                                    .child("Make sure you have not entered your password or any other sensitive information.")
                                                    .child(div().mt(px(4.)).child("By applying, you give the server admins permission to message you regarding your application.")),
                                            )
                                            .child(
                                                div()
                                                    .h(px(40.))
                                                    .px(px(24.))
                                                    .rounded(px(8.))
                                                    .opacity(0.5)
                                                    .bg(color("--control-connected-background-default"))
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(16.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--white"))
                                                    .child("Submit"),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .id("access-preview-close")
                                    .absolute()
                                    .top(px(32.))
                                    .right(px(32.))
                                    .size(px(40.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                                    .on_click(close),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_way_of_joining_follows_the_features() {
        assert_eq!(current_join(&[]), JoinType::Invite);
        assert_eq!(
            current_join(&strings(&["MEMBER_VERIFICATION_GATE_ENABLED"])),
            JoinType::Invite
        );
        assert_eq!(
            current_join(&strings(&[
                "MEMBER_VERIFICATION_GATE_ENABLED",
                "MEMBER_VERIFICATION_MANUAL_APPROVAL"
            ])),
            JoinType::Apply
        );
        assert_eq!(
            current_join(&strings(&["DISCOVERABLE"])),
            JoinType::Discoverable
        );
    }

    #[test]
    fn apply_starts_with_the_first_question_after_any_rules() {
        let questions = questions_of(&[]);
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0]["label"], FIRST_QUESTION);
        let terms = terms_fields(&strings(&["be kind"]));
        let questions = questions_of(&terms);
        assert_eq!(questions.len(), 2);
        assert!(is_terms(&questions[0]));
        assert_eq!(questions[1]["field_type"], "TEXT_INPUT");
    }

    #[test]
    fn invite_is_dirty_only_when_its_form_would_change() {
        assert!(!invite_form_dirty(&[], &[], false, &strings(&[""])));
        assert!(invite_form_dirty(&[], &[], true, &strings(&[""])));
        let form = terms_fields(&strings(&["be kind"]));
        let gate = strings(&["MEMBER_VERIFICATION_GATE_ENABLED"]);
        assert!(!invite_form_dirty(
            &gate,
            &form,
            true,
            &strings(&[" be kind "])
        ));
        assert!(invite_form_dirty(
            &gate,
            &form,
            true,
            &strings(&["be nice"])
        ));
        assert!(invite_form_dirty(
            &strings(&["MEMBER_VERIFICATION_MANUAL_APPROVAL"]),
            &form,
            false,
            &strings(&["be kind"])
        ));
    }

    #[test]
    fn apply_is_dirty_until_the_gate_and_its_questions_are_saved() {
        let questions = questions_of(&[]);
        assert!(apply_form_dirty(&[], &[], &questions));
        let both = strings(&[
            "MEMBER_VERIFICATION_GATE_ENABLED",
            "MEMBER_VERIFICATION_MANUAL_APPROVAL",
        ]);
        assert!(!apply_form_dirty(&both, &questions, &questions));
        assert!(apply_form_dirty(&both, &[], &questions));
    }

    #[test]
    fn terms_are_sent_only_with_rules() {
        assert!(terms_fields(&[]).is_empty());
        assert_eq!(terms_fields(&strings(&["x"]))[0]["label"], TERMS_LABEL);
        assert_eq!(trimmed(&strings(&[" a ", "", "  "])), strings(&["a"]));
    }

    #[test]
    fn rows_move_to_the_place_they_are_dropped_on() {
        let mut items = vec![1, 2, 3];
        move_item(&mut items, 0, 2);
        assert_eq!(items, vec![2, 3, 1]);
        move_item(&mut items, 2, 0);
        assert_eq!(items, vec![1, 2, 3]);
        move_item(&mut items, 5, 0);
        assert_eq!(items, vec![1, 2, 3]);
    }
}
