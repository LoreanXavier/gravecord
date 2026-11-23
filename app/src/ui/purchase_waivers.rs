//! Application Installation & Purchase Legal Waivers modal (module 357465):
//! Install Location picker (`F`), EULA & EU Right to Withdrawal checkboxes (`L`),
//! Trial/Discount recurring disclosures (`j`), and Install Game dialog (`V`).

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct InstallDirOption {
    pub path: String,
    pub available_label: String,
    pub has_permission: bool,
    pub has_enough_space: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum RecurringDisclosure {
    None,
    Trial,
    Discount,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct PurchaseWaiversModal {
    pub app_name: String,
    pub install_size_label: String,
    pub directories: Vec<InstallDirOption>,
    pub selected_dir_idx: usize,
    pub picker_open: bool,
    pub show_eula: bool,
    pub accepted_eula: bool,
    pub show_withdrawal_waiver: bool,
    pub accepted_withdrawal: bool,
    pub show_pricing_link: bool,
    pub disclosure: RecurringDisclosure,
    pub interval_label: String,
    pub button_text: String,
}

fn render_link_runs(
    id_prefix: &'static str,
    prefix: &str,
    runs: Vec<(String, Option<Box<str>>)>,
    link_url: &'static str,
) -> AnyElement {
    let mut text = String::from(prefix);
    let mut highlights = Vec::new();
    let mut ranges = Vec::new();
    let link_color = color("--text-link");
    for (chunk, hook) in runs {
        let start = text.len();
        text.push_str(&chunk);
        let end = text.len();
        if hook.as_deref() == Some("$link") && end > start {
            highlights.push((
                start..end,
                gpui::HighlightStyle {
                    color: Some(link_color.into()),
                    ..Default::default()
                },
            ));
            ranges.push(start..end);
        }
    }
    let styled = gpui::StyledText::new(text).with_highlights(highlights);
    if ranges.is_empty() {
        div().child(styled).into_any_element()
    } else {
        gpui::InteractiveText::new(SharedString::from(id_prefix), styled)
            .on_click(ranges, move |_, _, cx| {
                cx.open_url(link_url);
            })
            .into_any_element()
    }
}

impl Root {
    pub(crate) fn close_purchase_waivers(&mut self) -> bool {
        self.purchase_waivers_modal.take().is_some()
    }

    pub(in crate::ui) fn purchase_waivers_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.purchase_waivers_modal.as_ref()?;
        let cur_dir = modal.directories.get(modal.selected_dir_idx);
        let dir_label = cur_dir
            .map(|d| {
                dc_intl::format(
                    "Y+M3rp",
                    &[
                        ("path", Arg::Str(Cow::Borrowed(&d.path))),
                        ("size", Arg::Str(Cow::Borrowed(&d.available_label))),
                    ],
                )
            })
            .unwrap_or_else(|| dc_intl::t("cL/rrq").to_owned());

        let dir_error: Option<&'static str> = cur_dir.and_then(|d| {
            if !d.has_permission {
                Some(dc_intl::t("cEUT8B"))
            } else if !d.has_enough_space {
                Some(dc_intl::t("duXbE/"))
            } else {
                None
            }
        });

        let terms_ok = (!modal.show_eula || modal.accepted_eula)
            && (!modal.show_withdrawal_waiver || modal.accepted_withdrawal);
        let can_install = dir_error.is_none() && terms_ok;

        let eula_runs = dc_intl::format_runs(
            "IodJKT",
            &[("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name)))],
        );
        let pricing_runs = dc_intl::format_runs(
            "5zmY3F",
            &[(
                "documentationLink",
                Arg::Str(Cow::Borrowed(
                    "https://support.discord.com/hc/en-us/articles/4407269525911",
                )),
            )],
        );

        let disclosure_el: Option<AnyElement> = match modal.disclosure {
            RecurringDisclosure::None => None,
            RecurringDisclosure::Trial => {
                let runs = dc_intl::format_runs(
                    "Hvo/Z5",
                    &[
                        ("buttonText", Arg::Str(Cow::Borrowed(&modal.button_text))),
                        ("interval", Arg::Str(Cow::Borrowed(&modal.interval_label))),
                        (
                            "paidServiceTermsArticle",
                            Arg::Str(Cow::Borrowed("https://discord.com/terms")),
                        ),
                        (
                            "cancelSubscriptionArticle",
                            Arg::Str(Cow::Borrowed(
                                "https://support.discord.com/hc/en-us/articles/19580873036695",
                            )),
                        ),
                    ],
                );
                Some(
                    div()
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .text_color(color("--interactive-text-default"))
                        .child(render_link_runs(
                            "waiver-trial",
                            "",
                            runs,
                            "https://discord.com/terms",
                        ))
                        .into_any_element(),
                )
            }
            RecurringDisclosure::Discount => {
                let runs = dc_intl::format_runs(
                    "Z2c+aV",
                    &[
                        ("buttonText", Arg::Str(Cow::Borrowed(&modal.button_text))),
                        ("interval", Arg::Str(Cow::Borrowed(&modal.interval_label))),
                        (
                            "paidServiceTermsArticle",
                            Arg::Str(Cow::Borrowed("https://discord.com/terms")),
                        ),
                        (
                            "cancelSubscriptionArticle",
                            Arg::Str(Cow::Borrowed(
                                "https://support.discord.com/hc/en-us/articles/19580873036695",
                            )),
                        ),
                    ],
                );
                Some(
                    div()
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .text_color(color("--interactive-text-default"))
                        .child(render_link_runs(
                            "waiver-disc",
                            "",
                            runs,
                            "https://discord.com/terms",
                        ))
                        .into_any_element(),
                )
            }
        };

        let dialog = div()
            .id("purchase-waivers-modal")
            .occlude()
            .w(px(480.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.35),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            // Header
            .child(
                div()
                    .px(px(20.))
                    .pt(px(18.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("PphjtJ")),
                    )
                    .child(
                        div()
                            .id("waivers-close")
                            .flex_shrink_0()
                            .size(px(30.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_purchase_waivers();
                                cx.notify();
                            })),
                    ),
            )
            // Body
            .child(
                div()
                    .px(px(20.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    // Game row (`gameIcon_Jr2nWU` + `gameName_Jr2nWU` + `installSize_Jr2nWU`)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        "bot_game_controller",
                                        22.,
                                        "--interactive-icon-default",
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(modal.app_name.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(8.))
                                    .py(px(4.))
                                    .rounded(px(4.))
                                    .bg(color("--background-mod-strong"))
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(modal.install_size_label.clone()),
                            ),
                    )
                    .child(div().h(px(1.)).bg(color("--border-subtle")))
                    // Install Location selector (`WTJuNr`)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("WTJuNr")),
                            )
                            .child(
                                div()
                                    .id("waivers-dir-select")
                                    .relative()
                                    .h(px(38.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon(
                                                "clips_folder",
                                                16.,
                                                "--interactive-icon-default",
                                            ))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .text_size(px(13.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dir_label),
                                            ),
                                    )
                                    .child(icon("chevron_down", 16., "--interactive-icon-default"))
                                    .child(self.anchor_probe("waivers-dir-select"))
                                    .when(modal.picker_open, |col| {
col.child(gpui::deferred(div()
                                        .id("waivers-dir-list")
.occlude()
.absolute()
.left(px(-1.))
.right(px(-1.))
.top(px(41.))
                                        // `maxOptionsVisible` 5 (module 691885): five 34px rows, 2px gaps, 4px padding.
                                        .max_h(px(186.))
                                        .overflow_y_scroll()
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lower"))
.shadow(vec![super::shadow_high()])
                                        .p(px(4.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .on_click(|_, _, cx| cx.stop_propagation())
.on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
let on_trigger = this
.tip_anchors
.borrow()
.get("waivers-dir-select")
.is_some_and(|b| b.contains(&event.position));
if !on_trigger
&& let Some(m) = this.purchase_waivers_modal.as_mut()
&& m.picker_open
{
m.picker_open = false;
cx.notify();
}
}))
                                        .children(modal.directories.iter().enumerate().map(
                                            |(i, d)| {
                                                let active = i == modal.selected_dir_idx;
                                                let lbl = dc_intl::format(
                                                    "Y+M3rp",
                                                    &[
                                                        ("path", Arg::Str(Cow::Borrowed(&d.path))),
                                                        (
                                                            "size",
                                                            Arg::Str(Cow::Borrowed(
                                                                &d.available_label,
                                                            )),
                                                        ),
                                                    ],
                                                );
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "waiver-dir-{i}"
                                                    )))
                                                    .flex_none()
                                                    .px(px(10.))
                                                    .py(px(6.))
                                                    .rounded(px(6.))
                                                    .when(active, |el| {
                                                        el.bg(color("--background-mod-strong"))
                                                    })
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .text_color(color("--text-strong"))
                                                            .child(lbl),
                                                    )
                                                    .when(active, |el| {
                                                        el.child(icon(
                                                            "select_check",
                                                            14.,
                                                            "--control-primary-background-default",
                                                        ))
                                                    })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Some(m) =
                                                            this.purchase_waivers_modal.as_mut()
                                                        {
                                                            m.selected_dir_idx = i;
                                                            m.picker_open = false;
                                                        }
                                                        cx.notify();
                                                    }))
                                            },
                                        ))
                                        .child(
                                            div()
                                                .flex_none()
                                                .px(px(10.))
                                                .py(px(6.))
                                                .text_size(px(13.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("cL/rrq")),
                                        )).with_priority(99))
})
.on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.purchase_waivers_modal.as_mut() {
                                            m.picker_open = !m.picker_open;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .when_some(dir_error, |col, err| {
                                col.child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(err),
                                )
                            }),
                    )
                    // Legal Waivers (`Ogvn5o`, `IodJKT`, `DFCVN+`, `jhu6zm`, `5zmY3F`, `Hvo/Z5`/`Z2c+aV`)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .when(modal.show_withdrawal_waiver, |col| {
                                col.child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("Ogvn5o")),
                                )
                            })
                            .when(modal.show_eula, |col| {
                                let checked = modal.accepted_eula;
                                col.child(
                                    div()
                                        .id("waiver-eula-check")
                                        .cursor_pointer()
                                        .flex()
                                        .items_start()
                                        .gap(px(10.))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .mt(px(1.))
                                                .size(px(18.))
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(if checked {
                                                    color("--control-primary-background-default")
                                                } else {
                                                    color("--border-strong")
                                                })
                                                .bg(if checked {
                                                    color("--control-primary-background-default")
                                                } else {
                                                    gpui::hsla(0., 0., 0., 0.).into()
                                                })
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .when(checked, |b| {
                                                    b.child(icon("checkbox_check", 12., "--white"))
                                                }),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .text_color(color("--interactive-text-default"))
                                                .child(render_link_runs(
                                                    "waiver-eula-link",
                                                    "",
                                                    eula_runs,
                                                    "https://discord.com/terms",
                                                )),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(m) = this.purchase_waivers_modal.as_mut() {
                                                m.accepted_eula = !m.accepted_eula;
                                            }
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(modal.show_withdrawal_waiver, |col| {
                                let checked = modal.accepted_withdrawal;
                                col.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .id("waiver-eu-check")
                                                .cursor_pointer()
                                                .flex()
                                                .items_start()
                                                .gap(px(10.))
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .mt(px(1.))
                                                        .size(px(18.))
                                                        .rounded(px(4.))
                                                        .border_1()
                                                        .border_color(if checked {
                                                            color(
                                                                "--control-primary-background-default",
                                                            )
                                                        } else {
                                                            color("--border-strong")
                                                        })
                                                        .bg(if checked {
                                                            color(
                                                                "--control-primary-background-default",
                                                            )
                                                        } else {
                                                            gpui::hsla(0., 0., 0., 0.).into()
                                                        })
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .when(checked, |b| {
                                                            b.child(icon(
                                                                "checkbox_check",
                                                                12.,
                                                                "--white",
                                                            ))
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .text_size(px(13.))
                                                        .line_height(px(18.))
                                                        .text_color(color(
                                                            "--interactive-text-default",
                                                        ))
                                                        .child(dc_intl::t("DFCVN+")),
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.purchase_waivers_modal.as_mut()
                                                    {
                                                        m.accepted_withdrawal =
                                                            !m.accepted_withdrawal;
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("jhu6zm")),
                                        ),
                                )
                            })
                            .when(modal.show_pricing_link, |col| {
                                col.child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(render_link_runs(
                                            "waiver-pricing",
                                            "*",
                                            pricing_runs,
                                            "https://support.discord.com/hc/en-us/articles/4407269525911",
                                        )),
                                )
                            })
                            .when_some(disclosure_el, |col, el| col.child(el)),
                    ),
            )
            // Footer
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        div()
                            .id("waivers-install-btn")
                            .h(px(38.))
                            .px(px(20.))
                            .rounded(px(8.))
                            .bg(color("--status-positive"))
                            .when(can_install, |b| b.cursor_pointer())
                            .when(!can_install, |b| b.opacity(0.5))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("nL0WvC"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if can_install {
                                    this.close_purchase_waivers();
                                    cx.notify();
                                }
                            })),
                    ),
            );

        Some(
            deferred(
                div()
                    .id("purchase-waivers-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_purchase_waivers();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_purchase_waivers(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let disclosure = match arg {
            "discount" => RecurringDisclosure::Discount,
            "none" => RecurringDisclosure::None,
            _ => RecurringDisclosure::Trial,
        };
        self.purchase_waivers_modal = Some(PurchaseWaiversModal {
            app_name: "Starlight Odyssey".to_owned(),
            install_size_label: "4.2 GB".to_owned(),
            directories: vec![
                InstallDirOption {
                    path: "C:\\Games\\Discord".to_owned(),
                    available_label: "54.2 GB".to_owned(),
                    has_permission: true,
                    has_enough_space: true,
                },
                InstallDirOption {
                    path: "D:\\ReadOnly".to_owned(),
                    available_label: "1.1 GB".to_owned(),
                    has_permission: false,
                    has_enough_space: false,
                },
            ],
            selected_dir_idx: if arg == "err" { 1 } else { 0 },
            picker_open: false,
            show_eula: true,
            accepted_eula: true,
            show_withdrawal_waiver: true,
            accepted_withdrawal: arg != "unchecked",
            show_pricing_link: true,
            disclosure,
            interval_label: "month".to_owned(),
            button_text: "Get Nitro".to_owned(),
        });
        cx.notify();
        format!("purchase-waivers: {arg}")
    }
}

impl Root {
    pub(super) fn close_waivers_dir_select(&mut self) -> bool {
        if let Some(m) = self.purchase_waivers_modal.as_mut()
            && m.picker_open
        {
            m.picker_open = false;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_14_keys_of_purchase_waivers_357465_resolve() {
        for key in [
            "5zmY3F", "DFCVN+", "Hvo/Z5", "IodJKT", "Ogvn5o", "PphjtJ", "WTJuNr", "Y+M3rp",
            "Z2c+aV", "cEUT8B", "cL/rrq", "duXbE/", "jhu6zm", "nL0WvC",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
