//! Per-server profiles in the profile editor. The select at the top of the panel (`ld` of module 927071, styled by
//! module 216384) lists "Main Profile" and the account's servers; choosing one edits that server's profile with the
//! guild routes of `save.rs`. Without Nitro a server profile only shows the upsell and its groups are locked
//! (`lP`: `h = m && !p`); with Nitro it shows the dismissible notice instead (`lA`).

use super::{label, text};
use crate::net::Request;
use crate::ui::{Root, color, icon};
use dc_state::Id;
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, SharedString, deferred, div, img,
    prelude::*, px,
};

/// The local-storage key of the dismissed per-server notice (`per-server-profile-editing-notice-dismissed`).
const NOTICE_KEY: &str = "per-server-profile-editing-notice-dismissed";

/// The art of the per-server profile upsell (`artURL` of module 63473).
const GUILD_UPSELL_ART: &str = "https://cdn.discordapp.com/assets/content/cebaa07ffa781bd4cb680a3dcfeb8fe4a6efc48bfb7fc4c80068e875049e2961.png";
const GUILD_UPSELL_ART_KEY: &str = "profile-editor:guild-upsell-art";

/// Whether a click on the preview's avatar opens the per-server profile upsell (module 63473): `E` of module 641318
/// sends it to `onUpsellClick` when a server profile is edited (`guild != null`) by an account that cannot use premium
/// profile customization; the main profile, or an account with Nitro, gets the avatar menu instead.
pub(super) fn guild_upsell_opens(guild: Option<Id>, premium: bool) -> bool {
    guild.is_some() && !premium
}

/// A row of the select: `None` is "Main Profile".
pub(super) struct Choice {
    pub guild: Option<Id>,
    pub label: String,
    /// The account's nickname there (`description` of the option).
    pub nick: Option<String>,
}

/// The select's options (`f` in `ld`): Main Profile, the server being edited, then every server in the rail's order.
pub(super) fn choices(
    guilds: &[(Id, String)],
    current: Option<Id>,
    nick: impl Fn(Id) -> Option<String>,
) -> Vec<Choice> {
    let mut out = vec![Choice {
        guild: None,
        label: dc_intl::tr("Main Profile").to_owned(),
        nick: None,
    }];
    let rest = guilds.iter().filter(|(id, _)| Some(*id) != current);
    let first = guilds.iter().find(|(id, _)| Some(*id) == current);
    for (id, name) in first.into_iter().chain(rest) {
        out.push(Choice {
            guild: Some(*id),
            label: name.clone(),
            nick: nick(*id),
        });
    }
    out
}

/// A group of the panel with its locked look: the tiles take no input while it is locked.
pub(super) fn locked(content: Div, disabled: bool) -> Div {
    if disabled {
        div()
            .relative()
            .child(content)
            .child(div().absolute().inset_0().occlude())
    } else {
        div().child(content)
    }
}

impl Root {
    /// Switches the editor to another profile: the main one (`None`) or a server's. Each profile keeps its own
    /// pending changes, as the settings store keeps them per guild.
    pub(super) fn profile_editor_select_guild(
        &mut self,
        guild: Option<Id>,
        cx: &mut Context<Self>,
    ) {
        let Some(me) = self.stores.current_user.clone() else {
            return;
        };
        let nick = guild
            .and_then(|id| self.stores.members.get(id, me.id))
            .and_then(|member| member.nick.clone());
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        open.select_open = false;
        if open.guild == guild {
            cx.notify();
            return;
        }
        let leaving = std::mem::replace(&mut open.pending, super::save::Pending::new(guild));
        open.stashed.insert(open.guild, leaving);
        if let Some(back) = open.stashed.remove(&guild) {
            open.pending = back;
        }
        open.guild = guild;
        let kept_widgets = std::mem::take(&mut open.saved.widgets);
        open.saved = super::Saved::default();
        open.saved.widgets = kept_widgets;
        open.avatar_image = None;
        open.banner_image = None;
        open.picker = None;
        open.banner_color = None;
        open.styles = None;
        open.tag_select_open = false;
        open.game_picker = None;
        open.widget_modal = None;
        open.widget_confirm_remove = None;
        open.error = None;
        let shown = match (&open.pending.global_name, &open.pending.nickname, guild) {
            (_, Some(Some(pending)), Some(_)) => pending.clone(),
            (Some(Some(pending)), _, None) => pending.clone(),
            (_, _, Some(_)) => nick.unwrap_or_else(|| me.display_name().to_owned()),
            _ => me.display_name().to_owned(),
        };
        let bio_text = open.pending.bio.as_deref().unwrap_or("").to_owned();
        let pronouns_text = open.pending.pronouns.as_deref().unwrap_or("").to_owned();
        let (name, bio, pronouns) = (open.name.clone(), open.bio.clone(), open.pronouns.clone());
        name.update(cx, |editor, _| editor.set_text(&shown));
        bio.update(cx, |editor, _| editor.set_text(&bio_text));
        pronouns.update(cx, |editor, _| editor.set_text(&pronouns_text));
        let route = super::profile_route(me.id, guild);
        let _ = self.requests.send(Request::Fetch {
            key: format!("{}profile", super::FETCH_PREFIX),
            route,
        });
        cx.notify();
    }

    /// The select's list under its button (`listbox_VrghH6`: 264 wide, on `--background-surface-higher`).
    pub(super) fn profile_editor_server_list(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.profile_editor.as_ref()?;
        if !open.select_open {
            return None;
        }
        let me = self.stores.current_user.as_ref()?.id;
        let current = open.guild;
        let guilds: Vec<(Id, String)> = self
            .stores
            .guilds
            .iter()
            .map(|guild| (guild.id, guild.name.clone()))
            .collect();
        let options = choices(&guilds, current, |guild| {
            self.stores
                .members
                .get(guild, me)
                .and_then(|member| member.nick.clone())
        });
        let rows: Vec<_> = options
            .into_iter()
            .map(|choice| {
                let guild = choice.guild;
                let leading = match guild {
                    None => icon("sb_discord", 20., "--icon-default").into_any_element(),
                    Some(id) => {
                        let image = self.images.get(&format!("guild:{id}")).cloned();
                        let acronym = self
                            .stores
                            .guilds
                            .iter()
                            .find(|known| known.id == id)
                            .map(dc_state::Guild::acronym)
                            .unwrap_or_default();
                        let tile = div()
                            .size(px(24.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color("--background-surface-high"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.))
                            .text_color(color("--text-default"));
                        match image {
                            Some(image) => tile.child(img(image).size(px(24.))),
                            None => tile.child(acronym),
                        }
                        .into_any_element()
                    }
                };
                let name = div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(choice.label);
                let nick = choice.nick.filter(|nick| !nick.is_empty()).map(|nick| {
                    text(nick, 12., 16., 400., "--text-subtle")
                        .overflow_hidden()
                        .whitespace_nowrap()
                });
                div()
                    .id(SharedString::from(format!(
                        "profile-editor-server-{}",
                        guild.map_or("main".to_owned(), |id| id.to_string())
                    )))
                    .flex_none()
                    .min_h(px(40.))
                    .p(px(4.))
                    .pl(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(color("--text-strong"))
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_none()
                            .size(px(32.))
                            .ml(px(-8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(leading),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(name)
                            .children(nick),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profile_editor_select_guild(guild, cx);
                    }))
            })
            .collect();
        let open = self.profile_editor.as_ref()?;
        let left = if open.geometry.panel_over {
            open.geometry.width - open.geometry.panel_width + 9.
        } else {
            9.
        };
        let top = open.geometry.system_bar + 44.;
        Some(
            deferred(
                div()
                    .id("profile-editor-server-list")
                    .occlude()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(264.))
                    .max_h(px(5. * 40. + 10.))
                    .overflow_y_scroll()
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.select_open = false;
                        }
                        if event.button != gpui::MouseButton::Right {
                            cx.stop_propagation();
                        }
                        cx.notify();
                    }))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Opens the per-server profile upsell (module 63473) when [`guild_upsell_opens`] says so.
    pub(super) fn profile_editor_open_guild_upsell(&mut self, cx: &mut Context<Self>) {
        let premium = self.profile_editor_premium();
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        if !guild_upsell_opens(open.guild, premium) {
            return;
        }
        open.guild_upsell = true;
        self.request_image(GUILD_UPSELL_ART_KEY.to_owned(), GUILD_UPSELL_ART.to_owned());
        cx.notify();
    }

    /// Closes the per-server profile upsell. True when it was open.
    pub(super) fn profile_editor_close_guild_upsell(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.profile_editor.as_mut() else {
            return false;
        };
        if !std::mem::take(&mut open.guild_upsell) {
            return false;
        }
        cx.notify();
        true
    }

    /// Module 63473: the premium upsell modal of module 945240 (`PREMIUM_GUILD_IDENTITY_MODAL`, classes of module
    /// 777133) with the art (`artContainer_PwiJiV`: 225 wide, 8px corners, 48px above the box and 24px over the text,
    /// `artContainerBoxShadow_PwiJiV`), "Express yourself fully with per-server profiles" (`kOEBKP`,
    /// `heading-xl/semibold`, 24px under it), the `WMO9po` body whose "Discord Nitro!" link (`onAndMore`) closes the
    /// modal and opens User Settings on the Nitro page (module 87719 `e`), then the tier 2 subscribe button (green,
    /// small) and Back (`13/7kX`).
    /// GAP: the trial / discount variant of 945240 (module 811611, `nh`) is not drawn.
    pub(super) fn profile_editor_guild_upsell_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.profile_editor.as_ref()?.guild_upsell {
            return None;
        }
        let art = self
            .images
            .get(GUILD_UPSELL_ART_KEY)
            .cloned()
            .map(|image| img(image).w(px(225.)).into_any_element());
        let runs = dc_intl::format_runs("WMO9po", &[("onAndMore", dc_intl::Arg::from(true))]);
        // `root_PwiJiV` is `text-align: center`: one centred paragraph with the `$link` run in the link colour.
        let mut body_text = String::new();
        let mut link = None;
        for (run, tag) in runs {
            let start = body_text.len();
            body_text.push_str(&run);
            if tag.as_deref().is_some_and(|tag| tag.starts_with('$')) {
                link = Some(start..body_text.len());
            }
        }
        let highlights: Vec<_> = link
            .iter()
            .map(|range| {
                (
                    range.clone(),
                    gpui::HighlightStyle {
                        color: Some(color("--text-link").into()),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let entity = cx.entity().downgrade();
        // `subHeader_PwiJiV`: 32px either side.
        let body = div()
            .w_full()
            .px(px(32.))
            .text_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(
                gpui::InteractiveText::new(
                    "profile-editor-guild-upsell-body",
                    gpui::StyledText::new(body_text).with_highlights(highlights),
                )
                .on_click(link.into_iter().collect(), move |_, _, cx| {
                    // `onAndMore`: `c()` closes the modal, `(0, n.e)()` opens settings on the Nitro panel. The editor
                    // is drawn over settings, so it closes too.
                    let _ = entity.update(cx, |this, cx| {
                        this.profile_editor = None;
                        this.open_settings_page("nitro", cx);
                    });
                }),
            );
        let cta = self.subscribe_cta(Some(dc_state::premium::sku::TIER_2));
        let disabled = cta.disabled;
        let subscribe = div()
            .id("profile-editor-guild-upsell-subscribe")
            .h(px(32.))
            .px(px(16.))
            .rounded(px(8.))
            // `Colors.GREEN` (module 725807 for tier 2 without the gradient).
            .bg(color("--control-connected-background-default"))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-connected-text-default"))
            .when(!disabled, |this| this.cursor_pointer())
            .when(disabled, |this| this.opacity(0.5))
            .child(crate::ui::settings::mask_icon(
                "nitro_wheel",
                16.,
                "--control-connected-text-default",
            ))
            .child(dc_intl::tr(cta.text))
            .on_click(cx.listener(move |this, _, _, cx| {
                if disabled {
                    return;
                }
                this.profile_editor_close_guild_upsell(cx);
                this.open_purchase(false, cx);
            }));
        let back = div()
            .id("profile-editor-guild-upsell-back")
            .h(px(32.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--control-secondary-background-default"))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-secondary-text-default"))
            .child(dc_intl::t("13/7kX"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.profile_editor_close_guild_upsell(cx);
            }));
        Some(
            deferred(
                div()
                    .id("profile-editor-guild-upsell-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.profile_editor_close_guild_upsell(cx);
                    }))
                    .child(
                        div()
                            .id("profile-editor-guild-upsell")
                            .occlude()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .relative()
                            .w(px(440.))
                            .mt(px(48.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                div()
                                    .mt(px(-48.))
                                    .mb(px(24.))
                                    .w(px(225.))
                                    .rounded(px(8.))
                                    // `--elevation-medium`: 0 4px 4px `--opacity-black-8` on light, `--opacity-black-16` on dark.
                                    .shadow(vec![gpui::BoxShadow {
                                        color: gpui::hsla(
                                            0.,
                                            0.,
                                            0.,
                                            if matches!(
                                                super::super::theme(),
                                                super::super::Theme::Light
                                            ) {
                                                0.08
                                            } else {
                                                0.16
                                            },
                                        ),
                                        offset: gpui::point(px(0.), px(4.)),
                                        blur_radius: px(4.),
                                        spread_radius: px(0.),
                                        inset: false,
                                    }])
                                    .children(art),
                            )
                            .child(
                                div()
                                    .px(px(16.))
                                    .mb(px(32.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(
                                        div()
                                            .mb(px(24.))
                                            .text_center()
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("kOEBKP")),
                                    )
                                    .child(body),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .p(px(16.))
                                    .flex()
                                    .flex_row_reverse()
                                    .items_center()
                                    .justify_between()
                                    .child(subscribe)
                                    .child(back),
                            ),
                    ),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }

    /// The top of a server profile's panel: the dismissible notice with Nitro (`lA`), the upsell without
    /// (`lI`, with the lock divider below it).
    pub(super) fn profile_editor_server_intro(
        &self,
        premium: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if premium {
            if crate::storage::get(NOTICE_KEY).and_then(|value| value.as_bool()) == Some(true) {
                return div().into_any_element();
            }
            return div()
                .flex()
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--icon-feedback-info"))
                .bg(color("--background-feedback-mod-info"))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(
                            dc_intl::tr("You can customize styles, display name, pronouns, and bio for each server."),
                        ),
                )
                .child(
                    div()
                        .id("profile-editor-notice-dismiss")
                        .size(px(24.))
                        .mt(px(-2.))
                        .mr(px(-2.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(icon("modal_close", 16., "--icon-subtle"))
                        .on_click(cx.listener(|_, _, _, cx| {
                            crate::storage::set(NOTICE_KEY, serde_json::json!(true));
                            cx.notify();
                        })),
                )
                .into_any_element();
        }
        let pitch = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(text(
                dc_intl::tr("Customize more with Nitro"),
                16.,
                20.,
                500.,
                "--text-default",
            ))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(
                        "Go beyond name and pronouns. Give each server unique banners, themes, decorations, and more with ",
                    )
                    .child(
                        div()
                            .id("profile-editor-server-nitro-link")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .child(dc_intl::tr("Nitro"))
                            .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx))),
                    ),
            );
        let unlock = div()
            .id("profile-editor-server-unlock")
            .h(px(32.))
            .rounded(px(8.))
            .bg(color("--control-primary-background-default"))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .cursor_pointer()
            .child(icon(
                "settings_nitro",
                16.,
                "--control-primary-text-default",
            ))
            .child(text(
                dc_intl::tr("Unlock with Nitro"),
                14.,
                18.,
                500.,
                "--control-primary-text-default",
            ))
            .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx)));
        // `lockDivider`: a rule across the panel with the lock on it.
        let divider = div()
            .relative()
            .mt(px(12.))
            .mx(px(-16.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(14.))
                    .h(px(1.))
                    .bg(color("--border-normal")),
            )
            .child(
                div()
                    .size(px(28.))
                    .rounded_full()
                    .border_1()
                    .border_color(color("--border-normal"))
                    .bg(color("--background-surface-highest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("channel_lock", 16., "--icon-subtle")),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(pitch)
            .child(unlock)
            .child(divider)
            .into_any_element()
    }
}

/// The group label with the Nitro mark (`showNitroIcon`), muted while the group is locked.
pub(super) fn nitro_label(title: &'static str, disabled: bool) -> Div {
    let (tint, mark) = if disabled {
        ("--text-muted", "--icon-muted")
    } else {
        ("--text-strong", "--icon-default")
    };
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(text(title, 14., 18., 500., tint))
        .child(icon("settings_nitro", 16., mark))
}

/// The plain group label, muted while the group is locked.
pub(super) fn group_label(title: &'static str, disabled: bool) -> Div {
    if disabled {
        text(title, 14., 18., 500., "--text-muted")
    } else {
        label(title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(id: Id, name: &str) -> (Id, String) {
        (id, name.to_owned())
    }

    #[test]
    fn the_select_lists_main_first_then_the_open_server_then_the_rest() {
        let guilds = vec![guild(1, "One"), guild(2, "Two"), guild(3, "Three")];
        let list = choices(&guilds, Some(2), |id| (id == 3).then(|| "Nick".to_owned()));
        let names: Vec<_> = list.iter().map(|choice| choice.label.as_str()).collect();
        assert_eq!(names, ["Main Profile", "Two", "One", "Three"]);
        assert_eq!(list[0].guild, None);
        assert_eq!(list[3].nick.as_deref(), Some("Nick"));
    }

    #[test]
    fn the_guild_upsell_opens_only_for_a_server_profile_without_nitro() {
        assert!(guild_upsell_opens(Some(1), false));
        assert!(!guild_upsell_opens(Some(1), true));
        assert!(!guild_upsell_opens(None, false));
        assert!(!guild_upsell_opens(None, true));
    }

    #[test]
    fn the_guild_upsell_strings_are_the_official_ones() {
        assert_eq!(
            dc_intl::t("kOEBKP"),
            "Express yourself fully with per-server profiles"
        );
        let runs = dc_intl::format_runs("WMO9po", &[("onAndMore", dc_intl::Arg::from(true))]);
        let link: Vec<_> = runs
            .iter()
            .filter(|(_, tag)| tag.as_deref().is_some_and(|tag| tag.starts_with('$')))
            .map(|(run, _)| run.as_str())
            .collect();
        assert_eq!(link, ["Discord Nitro!"]);
    }
}
