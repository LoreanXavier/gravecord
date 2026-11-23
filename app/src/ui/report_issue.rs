//! Report Issue for a detected game (module 651930, opened by `game-detection-report-issue` of module 534465 from the
//! "..." of a game activity card). Three steps on the shell of module 189213: "What went wrong?" with two radio
//! options; after one is picked (and 100 ms), either the game search ("Wrong Game Is Shown": the detected game, a
//! divider and a searchable select of `GET /games/autocomplete`) or "Other Feedback" (a text area of 300 characters).
//! Submit only reports the answer to analytics (`K6` of module 409626), which this client does not send, and closes.
//! Measured in `design/dom/report-issue/*-fixture-darker-cozy.json`.

use super::modal::{self, Tone};
use super::{Root, color};
use crate::editor::{Editor, EditorEvent};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window, div,
    img, prelude::*, px, rgba,
};

/// The game the card showed (`detected` of module 651930).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Detected {
    pub name: String,
    pub application_id: Option<String>,
    /// The picture the card showed, when the application has no icon of its own.
    pub art: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    IssueSelection,
    GameSearch,
    OtherFeedback,
}

/// A game the search found (module 518805: id, name, icon).
#[derive(Clone, Debug, PartialEq)]
struct Found {
    id: String,
    name: String,
    icon: Option<String>,
}

pub(super) struct ReportIssue {
    detected: Option<Detected>,
    step: Step,
    /// The radio picked on the first step: 0 the game option, 1 Other Feedback.
    picked_option: Option<usize>,
    query: Entity<Editor>,
    feedback: Entity<Editor>,
    picked: Option<Found>,
    pub(super) list_open: bool,
    _events: [Subscription; 2],
}

/// `maxLength` of the feedback text area.
const FEEDBACK_MAX: usize = 300;

/// The radio options of the first step: with a detected game, "Wrong Game Is Shown" (`TZgkxY`), else "Game Not
/// Detected" (`+ie+wX`); then "Other Feedback" (`tdDpJj`).
fn options(detected: bool) -> [&'static str; 2] {
    [
        if detected {
            "Wrong Game Is Shown"
        } else {
            "Game Not Detected"
        },
        "Other Feedback",
    ]
}

/// Module 518805 results, in the order the server gave them.
fn found_games(body: &serde_json::Value) -> Vec<Found> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|game| {
            let id = game["id"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| game["id"].as_u64().map(|id| id.to_string()))?;
            let icon = game["icon"]["value"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(|value| match game["icon"]["type"].as_str() {
                    Some("url") => value.to_owned(),
                    _ => format!("https://cdn.discordapp.com/app-icons/{id}/{value}.png?size=64"),
                });
            Some(Found {
                name: game["name"].as_str().unwrap_or_default().to_owned(),
                id,
                icon,
            })
        })
        .collect()
}

impl Root {
    pub(super) fn open_report_issue(
        &mut self,
        detected: Option<Detected>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder(
                if detected.is_some() {
                    "Search correct game"
                } else {
                    "Search game"
                }
                .to_owned(),
            );
            editor.set_single_line(true);
            editor.set_keeps_text(true);
            editor.set_reports_all_edits(true);
            editor
        });
        let feedback = cx.new(|cx| {
            let mut style = super::editor_style();
            // `.textArea_uI0xjn` keeps `line-height: normal`: 21px for gg sans at 16px.
            style.line_height = 21.;
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder("Describe issues".to_owned());
            editor.set_enter_inserts_newline(true);
            editor.set_keeps_text(true);
            editor.set_reports_all_edits(true);
            editor.set_max_utf16(FEEDBACK_MAX);
            editor
        });
        let events = [
            cx.subscribe(&query, |this, editor, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Cancel => this.report_issue = None,
                    EditorEvent::Edited | EditorEvent::Submit(_) => {
                        let text = editor.read(cx).text().to_owned();
                        if let Some(open) = this.report_issue.as_mut() {
                            open.list_open = true;
                            // A name typed over the picked game drops it (`U` of module 651930).
                            if open
                                .picked
                                .as_ref()
                                .is_some_and(|picked| picked.name != text)
                            {
                                open.picked = None;
                            }
                        }
                        this.search_games(&text);
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&feedback, |this, _, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Cancel) {
                    this.report_issue = None;
                }
                cx.notify();
            }),
        ];
        // The game's icon is the application's (module 769015): ask for its record when no card has yet.
        if let Some(app) = detected
            .as_ref()
            .and_then(|detected| detected.application_id.clone())
            .filter(|app| app.parse::<u64>().is_ok())
        {
            let key = format!("presence-app:{app}");
            if !self.fetched.contains_key(&key) {
                self.fetched.insert(key.clone(), serde_json::Value::Null);
                let _ = self.requests.send(crate::net::Request::Fetch {
                    key,
                    route: format!("/applications/public?application_ids={app}"),
                });
            }
        }
        self.profile = None;
        self.report_issue = Some(ReportIssue {
            detected,
            step: Step::IssueSelection,
            picked_option: None,
            query,
            feedback,
            picked: None,
            list_open: false,
            _events: events,
        });
        let _ = window;
        cx.notify();
    }

    /// The radio's `onChange`: the pick shows, and 100 ms later the dialog moves to the step it leads to.
    fn report_issue_pick(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.report_issue.as_mut() else {
            return;
        };
        open.picked_option = Some(option);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            let _ = this.update_in(cx, |this, _, cx| {
                if let Some(open) = this.report_issue.as_mut()
                    && open.step == Step::IssueSelection
                {
                    open.step = if option == 0 {
                        Step::GameSearch
                    } else {
                        Step::OtherFeedback
                    };
                    // The new step's field is not focused: the captured dialog shows no caret.
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The results of the query as typed (module 518805 normalizes it the way the games search does).
    fn report_issue_results(&self, query: &str) -> Vec<Found> {
        super::server_games::normalize(query)
            .and_then(|normal| self.fetched.get(&format!("game-search:{normal}")))
            .map(found_games)
            .unwrap_or_default()
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_report_issue(
        &mut self,
        words: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        match words {
            ["open", app, name @ ..] => {
                let detected = (!name.is_empty()).then(|| Detected {
                    name: name.join(" "),
                    application_id: (*app != "-").then(|| (*app).to_owned()),
                    art: None,
                });
                self.open_report_issue(detected, window, cx);
                "report-issue: open".to_owned()
            }
            ["pick", option] => match option.parse::<usize>() {
                Ok(option) if option < 2 => {
                    self.report_issue_pick(option, window, cx);
                    "report-issue: picked".to_owned()
                }
                _ => "report-issue: pick 0 or 1".to_owned(),
            },
            ["state"] => {
                let app = self
                    .report_issue
                    .as_ref()
                    .and_then(|open| open.detected.as_ref())
                    .and_then(|detected| detected.application_id.clone())
                    .unwrap_or_default();
                format!(
                    "report-issue: fetched={:?} icon={}",
                    self.fetched
                        .get(&format!("presence-app:{app}"))
                        .map(|body| body.to_string().chars().take(160).collect::<String>()),
                    self.images.contains_key(&format!("app-icon:{app}"))
                )
            }
            _ => "report-issue: open <app id|-> <name> | pick <n> | state".to_owned(),
        }
    }

    #[cfg(feature = "capture")]
    pub(crate) fn report_issue_editors(&self) -> Vec<Entity<Editor>> {
        self.report_issue
            .iter()
            .flat_map(|open| [open.query.clone(), open.feedback.clone()])
            .collect()
    }

    pub(super) fn report_issue_modal(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.report_issue.as_ref()?;
        let detected = open.detected.clone();
        let step = open.step;
        let query_text = open.query.read(cx).text().to_owned();
        let feedback_text = open.feedback.read(cx).text().to_owned();
        let muted = |text: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(text)
        };
        let (title, body, buttons): (&'static str, AnyElement, Vec<modal::Button>) = match step {
            Step::IssueSelection => {
                let picked = open.picked_option;
                let labels = options(detected.is_some());
                let rows = labels.into_iter().enumerate().map(|(index, label)| {
                    div()
                        .id(SharedString::from(format!("report-issue-option-{index}")))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .cursor_pointer()
                        .child(super::settings::radio_dot(picked == Some(index)))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(label),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.report_issue_pick(index, window, cx)
                        }))
                });
                (
                    "What went wrong?",
                    div()
                        .flex()
                        .flex_col()
                        .child(muted("Please select your issue"))
                        // `radioGroup_QYgmpy` sits 8px below the 16px gap; the group pads 8 above and 4 below, and spaces options 16.
                        .child(
                            div()
                                .mt(px(24.))
                                .pt(px(8.))
                                .pb(px(4.))
                                .flex()
                                .flex_col()
                                .gap(px(16.))
                                .children(rows),
                        )
                        .into_any_element(),
                    Vec::new(),
                )
            }
            Step::GameSearch => {
                let results = if query_text.trim().is_empty() {
                    Vec::new()
                } else {
                    self.report_issue_results(&query_text)
                };
                for icon_url in results.iter().filter_map(|found| found.icon.clone()) {
                    self.request_image(format!("game-icon:{icon_url}"), icon_url);
                }
                let open = self.report_issue.as_ref()?;
                let list_open = open.list_open && !results.is_empty();
                let detected_frame = detected.as_ref().map(|detected| {
                    let picture = detected
                        .application_id
                        .as_ref()
                        .and_then(|id| self.images.get(&format!("app-icon:{id}")).cloned())
                        .or_else(|| {
                            detected
                                .art
                                .as_ref()
                                .and_then(|key| self.images.get(key).cloned())
                        });
                    // `detectedGameFrame_QYgmpy`: white at 4%, 12px inside, 8px corners, 8px above and below.
                    div()
                        .my(px(8.))
                        .p(px(12.))
                        .rounded(px(8.))
                        .bg(rgba(0xffffff0a))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .size(px(48.))
                                .flex_none()
                                .rounded(px(8.))
                                .overflow_hidden()
                                .children(
                                    picture
                                        .map(|picture| img(picture).size(px(48.)).rounded(px(8.))),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(detected.name.clone()),
                        )
                });
                let chevron =
                    super::settings::mask_icon("settings_select_chevron", 20., "--text-strong")
                        .when(list_open, |this| {
                            this.with_transformation(gpui::Transformation::rotate(
                                gpui::percentage(0.5),
                            ))
                        });
                let field = div()
                    .id("report-issue-search")
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if list_open {
                        color("--input-border-active")
                    } else {
                        color("--input-border-default")
                    })
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pl(px(12.))
                            .pr(px(8.))
                            .child(open.query.clone()),
                    )
                    .child(
                        div()
                            .id("report-issue-chevron")
                            .flex_none()
                            .w(px(36.))
                            .h_full()
                            .pr(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(chevron)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.report_issue.as_mut() {
                                    open.list_open = !open.list_open;
                                }
                                cx.notify();
                            })),
                    );
                // `selectDropdown_vWC4bk`: 4px inside, rows of 40 with a 24px icon in a 32px cell, five visible.
                let list = list_open.then(|| {
                    let rows = results.iter().map(|found| {
                        let picture = found
                            .icon
                            .as_ref()
                            .and_then(|url| self.images.get(&format!("game-icon:{url}")).cloned());
                        let pick = found.clone();
                        div()
                            .id(SharedString::from(format!(
                                "report-issue-game-{}",
                                found.id
                            )))
                            .flex_none()
                            .h(px(40.))
                            .pl(px(8.))
                            .pr(px(4.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|style| style.bg(color("--background-mod-normal")))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div().size(px(32.)).flex_none().p(px(4.)).child(
                                    div()
                                        .size(px(24.))
                                        .rounded(px(4.))
                                        .overflow_hidden()
                                        .bg(color("--background-mod-normal"))
                                        .children(picture.map(|picture| {
                                            img(picture).size(px(24.)).rounded(px(4.))
                                        })),
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-strong"))
                                    .child(found.name.clone()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(open) = this.report_issue.as_mut() {
                                    open.picked = Some(pick.clone());
                                    open.list_open = false;
                                    let name = pick.name.clone();
                                    open.query.update(cx, |editor, _| editor.set_text(&name));
                                }
                                cx.notify();
                            }))
                    });
                    gpui::deferred(
                        div()
                            .id("report-issue-list")
                            .occlude()
                            .absolute()
                            .top(px(44.))
                            .left_0()
                            .right_0()
                            .max_h(px(210.))
                            .p(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.16),
                                offset: gpui::point(px(0.), px(2.)),
                                blur_radius: px(8.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .overflow_y_scroll()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.report_issue.as_mut() {
                                    open.list_open = false;
                                    cx.notify();
                                }
                            }))
                            .flex()
                            .flex_col()
                            .children(rows),
                    )
                    .with_priority(3)
                });
                let submit_ready = !query_text.trim().is_empty();
                (
                    if detected.is_some() {
                        "Wrong Game Is Shown"
                    } else {
                        "Game Not Detected"
                    },
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(muted(if detected.is_some() {
                            "Let us know which game this is supposed to be"
                        } else {
                            "Let us know which game you're currently playing"
                        }))
                        .children(detected_frame.map(|frame| {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(16.))
                                .child(frame)
                                .child(div().h(px(1.)).bg(color("--border-subtle")))
                        }))
                        .child(div().relative().child(field).children(list))
                        .into_any_element(),
                    vec![modal::button(
                        "report-issue-submit",
                        "Submit",
                        if submit_ready {
                            Tone::Primary
                        } else {
                            Tone::PrimaryDisabled
                        },
                        |this, _, _| this.report_issue = None,
                    )],
                )
            }
            Step::OtherFeedback => {
                let submit_ready = !feedback_text.trim().is_empty();
                (
                    "Other Feedback",
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(muted(
                            "Please describe any issues you're seeing, or how we can improve game information or game activity.",
                        ))
                        // `wrapper_UcGqzv` around `textArea_uI0xjn` (`rows` 4 of module 103557, no override: 4 * 21 + 24 + 2 = 110).
                        .child(
                            div()
                                .h(px(110.))
                                .px(px(10.))
                                .py(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--input-border-default"))
                                .bg(color("--input-background-default"))
                                .child(open.feedback.clone()),
                        )
                        .into_any_element(),
                    vec![modal::button(
                        "report-issue-submit",
                        "Submit",
                        if submit_ready {
                            Tone::Primary
                        } else {
                            Tone::PrimaryDisabled
                        },
                        |this, _, _| this.report_issue = None,
                    )],
                )
            }
        };
        let no_buttons = buttons.is_empty();
        let spec = modal::Spec {
            id: "report-issue",
            title: title.into(),
            subtitle: None,
            // A dialog without actions keeps the body's 8px bottom spacer (`bodySpacerBottom_dvN-MP`).
            body: Some(
                div()
                    .when(no_buttons, |this| this.pb(px(8.)))
                    .child(body)
                    .into_any_element(),
            ),
            buttons,
            close: Box::new(|this, _, _| this.report_issue = None),
        };
        Some(modal::render(spec, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{found_games, options};
    use serde_json::json;

    #[test]
    fn the_first_step_names_the_game_option_by_whether_a_game_was_detected() {
        assert_eq!(options(true), ["Wrong Game Is Shown", "Other Feedback"]);
        assert_eq!(options(false), ["Game Not Detected", "Other Feedback"]);
    }

    #[test]
    fn search_results_keep_the_server_order_and_their_icons() {
        let body = json!([
            {"id": "1", "name": "Fort Triumph", "icon": {"type": "hash", "value": "abc"}},
            {"id": "2", "name": "Survival Fort", "icon": {"type": "hash", "value": ""}},
        ]);
        let found = found_games(&body);
        assert_eq!(found[0].name, "Fort Triumph");
        assert_eq!(
            found[0].icon.as_deref(),
            Some("https://cdn.discordapp.com/app-icons/1/abc.png?size=64")
        );
        assert_eq!(found[1].icon, None);
    }
}
