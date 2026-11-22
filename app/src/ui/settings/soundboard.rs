//! Voice & Video's Entrance Sounds (`aG` in module 158935): the fieldset with its legend and description, "Choose a
//! Server" over `All Servers` and the guilds, and "Choose a sound" (Nitro). Measured in
//! `design/dom/settings/voice-video-advanced3-darker-cozy.json` (`fieldset_UGw2wN`, `customizationSection_wRbpaT`,
//! `soundButtonSettingContainer_VACzzt`).

use super::model::Choice;
use super::voice::SOUNDBOARD_GUIDE;
use super::{Root, color, mask_icon};
use crate::editor::{Editor, EditorEvent};
use crate::ui::{editor_style, icon};
use dc_state::Id;
use gpui::{AnyElement, Context, Focusable, FontWeight, Window, div, img, prelude::*, px};

/// The key of the generated page's row, and where the chosen server and the open list are kept in `setting_choices`.
pub(super) const ENTRANCE_SOUNDS: &str = "entrance_sounds_setting";
const OPEN: &str = "entrance_sounds_setting/open";

/// The options of "Choose a Server": the global one, then the guilds in the order the client lists them.
#[cfg(test)]
pub(super) fn server_options(guilds: impl IntoIterator<Item = String>) -> Vec<gpui::SharedString> {
    std::iter::once(dc_intl::t("CpEUP/").to_owned())
        .chain(guilds)
        .map(gpui::SharedString::from)
        .collect()
}

/// Module 687021 (`444550`): filters `(option_index, guild_id, name)` entries case-insensitively
/// by `query` (including `"All Servers"` at index 0).
pub(crate) fn filter_server_options(
    entries: &[(usize, Option<Id>, String)],
    query: &str,
) -> Vec<(usize, Option<Id>, String)> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .filter(|(_, _, name)| name.to_lowercase().contains(&needle))
        .cloned()
        .collect()
}

impl Root {
    fn ensure_soundboard_server_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.voice.soundboard_server_search.is_none() {
            let field = cx.new(|cx| {
                let mut ed = Editor::new(editor_style(), cx);
                ed.set_single_line(true);
                ed.set_placeholder(dc_intl::t("kMgj+e").to_owned());
                ed
            });
            let sub = cx.subscribe(&field, |this, ed, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Edited) {
                    this.voice.soundboard_server_query = ed.read(cx).text().to_owned();
                    cx.notify();
                }
            });
            self.voice.soundboard_server_search = Some((field, sub));
        }
        if let Some((field, _)) = &self.voice.soundboard_server_search {
            field.focus_handle(cx).focus(window, cx);
        }
    }

    /// Module 687021 (`444550`): searchable "Choose a Server" select with `"Search servers"` (`kMgj+e`),
    /// `"All Servers"` (`CpEUP/`), 44px rows (`maxVisibleItems = 7` -> `308px`), `4px` rounded guild
    /// icon prefixes, `"No results found"` (`Xe+fJM`) empty state, and `circle_check` mark.
    fn searchable_server_select(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut entries: Vec<(usize, Option<Id>, String)> =
            Vec::with_capacity(self.stores.guilds.len() + 1);
        entries.push((0, None, dc_intl::t("CpEUP/").to_owned()));
        for (i, guild) in self.stores.guilds.iter().enumerate() {
            entries.push((i + 1, Some(guild.id), guild.name.clone()));
        }
        let chosen_index = match self.setting_choices.get(ENTRANCE_SOUNDS) {
            Some(Choice::Index(idx)) if *idx < entries.len() => *idx,
            _ => 0,
        };
        let is_open = matches!(self.setting_choices.get(OPEN), Some(Choice::On(true)));
        let (_, selected_gid, selected_name) = &entries[chosen_index];
        let query = &self.voice.soundboard_server_query;
        let filtered = filter_server_options(&entries, query);

        let guild_badge = |gid: Option<Id>, name: &str| -> AnyElement {
            match gid {
                None => div()
                    .size(px(24.))
                    .flex_none()
                    .rounded(px(4.))
                    .bg(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("access_globe", 16., "--interactive-icon-default"))
                    .into_any_element(),
                Some(id) => {
                    if let Some(pic) = self.images.get(&format!("guild:{id}")).cloned() {
                        img(pic)
                            .size(px(24.))
                            .flex_none()
                            .rounded(px(4.))
                            .object_fit(gpui::ObjectFit::Cover)
                            .into_any_element()
                    } else {
                        let acronym: String = name
                            .split_whitespace()
                            .filter_map(|w| w.chars().next())
                            .take(2)
                            .collect();
                        div()
                            .size(px(24.))
                            .flex_none()
                            .rounded(px(4.))
                            .bg(color("--background-brand"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(acronym)
                            .into_any_element()
                    }
                }
            }
        };

        let trigger = div()
            .id("entrance-sounds-server-trigger")
            .w(px(348.))
            .h(px(40.))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(if is_open {
                "--background-brand"
            } else {
                "--input-border-default"
            }))
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
                    .child(guild_badge(*selected_gid, selected_name))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(selected_name.clone()),
                    ),
            )
            .child(icon(
                if is_open {
                    "chevron_up"
                } else {
                    "chevron_down"
                },
                18.,
                "--interactive-icon-default",
            ))
            .on_click(cx.listener(move |this, _, window, cx| {
                let next = !matches!(this.setting_choices.get(OPEN), Some(Choice::On(true)));
                this.setting_choices.insert(OPEN, Choice::On(next));
                if next {
                    this.ensure_soundboard_server_search(window, cx);
                }
                cx.notify();
            }));

        let dropdown = is_open.then(|| {
            let search_row = div()
                .h(px(40.))
                .px(px(12.))
                .border_b_1()
                .border_color(color("--border-subtle"))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(icon("search", 16., "--text-muted"))
                .child(
                    div().flex_1().min_w(px(0.)).children(
                        self.voice
                            .soundboard_server_search
                            .as_ref()
                            .map(|(ed, _)| ed.clone()),
                    ),
                );

            let list_body = if filtered.is_empty() {
                div()
                    .h(px(64.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::t("Xe+fJM"))
                    .into_any_element()
            } else {
                div()
                    .id("entrance-sounds-server-list")
                    .max_h(px(308.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(filtered.into_iter().map(|(idx, gid, name)| {
                        let is_selected = idx == chosen_index;
                        div()
                            .id(("entrance-sounds-server-row", idx))
                            .h(px(44.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .cursor_pointer()
                            .when(is_selected, |row| row.bg(color("--background-mod-subtle")))
                            .hover(|s| s.bg(color("--interactive-background-hover")))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(guild_badge(gid, &name))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(if is_selected {
                                                600.
                                            } else {
                                                500.
                                            }))
                                            .text_color(color("--text-strong"))
                                            .child(name),
                                    ),
                            )
                            .when(is_selected, |row| {
                                row.child(icon("circle_check", 18., "--background-brand"))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.setting_choices
                                    .insert(ENTRANCE_SOUNDS, Choice::Index(idx));
                                this.setting_choices.insert(OPEN, Choice::On(false));
                                cx.notify();
                            }))
                    }))
                    .into_any_element()
            };

            div()
                .mt(px(4.))
                .w(px(348.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .shadow_lg()
                .overflow_hidden()
                .child(search_row)
                .child(list_body)
        });

        div()
            .flex()
            .flex_col()
            .child(trigger)
            .children(dropdown)
            .into_any_element()
    }

    /// The fieldset: legend, description, then the two sections 24px apart (`stack_TWWX0S`, gap 24).
    pub(super) fn entrance_sounds_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let server = self.searchable_server_select(cx);
        div()
            .py(px(8.))
            .flex()
            .flex_col()
            // `legend_UGw2wN`: 20px/24px medium.
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Entrance Sounds")),
            )
            // The description sits 4px under the legend and 16px above the sections.
            .child(
                div()
                    .mt(px(4.))
                    .mb(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(self.text_with_link(
                        ENTRANCE_SOUNDS,
                        "Choose a Soundboard sound to automatically play whenever you join a voice channel. Right-click on a voice channel to join without playing your entrance sound. ",
                        dc_intl::tr("Learn More"),
                        ".",
                        cx,
                        |_, cx| cx.open_url(SOUNDBOARD_GUIDE),
                    )),
            )
            .child(
                div()
                    .mt(px(16.))
                    .child(section(dc_intl::t("ref7Ki"), false, server)),
            )
            .child(
                div()
                    .mt(px(48.))
                    .child(section("Choose a sound ", true, sound_button())),
            )
            .into_any_element()
    }
}

/// `customizationSection_wRbpaT`: the title (`h5` eyebrow, 16px/20px medium, the Nitro wheel after it when the
/// feature is Nitro's) and the control 8px under it.
fn section(title: &'static str, nitro: bool, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .mb(px(8.))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(title)
                .when(nitro, |this| {
                    this.child(mask_icon("nitro_wheel", 16., "--text-strong"))
                }),
        )
        .child(control)
}

/// `soundButtonSettingContainer_VACzzt`: 348 by 40 with the select's border and fill, "None" and the muted speaker
/// at the start, the change button at the end. Choosing a sound is Nitro's, so for this account the row takes no
/// clicks (the speaker is the `secondaryIconDisabled` one).
fn sound_button() -> impl IntoElement {
    div()
        .w(px(348.))
        .h(px(40.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--input-border-default"))
        .bg(color("--input-background-default"))
        .px(px(12.))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::tr("None")),
                )
                .child(mask_icon("sound_speaker", 20., "--text-muted")),
        )
        // `secondaryButton_N99eHz`: 28 wide (20px icon, 4px each side), in `--text-subtle`.
        .child(
            div()
                .w(px(28.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .child(mask_icon("settings_edit_pencil", 20., "--text-subtle")),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_list_starts_with_all_servers() {
        let options = server_options(["gravecord fixture".to_owned(), "Second".to_owned()]);
        let names: Vec<&str> = options.iter().map(|option| option.as_ref()).collect();
        assert_eq!(names, ["All Servers", "gravecord fixture", "Second"]);
        assert_eq!(server_options(Vec::new()).len(), 1);
    }

    #[test]
    fn searchable_server_select_filters_and_intl_matches_module_687021() {
        let entries = vec![
            (0, None, "All Servers".to_owned()),
            (1, Some(10), "Alpha Guild".to_owned()),
            (2, Some(20), "Beta Builders".to_owned()),
        ];
        assert_eq!(filter_server_options(&entries, "").len(), 3);
        let beta = filter_server_options(&entries, "beta");
        assert_eq!(beta.len(), 1);
        assert_eq!(beta[0].1, Some(20));
        assert!(filter_server_options(&entries, "zzz").is_empty());
        assert_eq!(dc_intl::t("ref7Ki"), "Choose a Server");
        assert_eq!(dc_intl::t("kMgj+e"), "Search servers");
        assert_eq!(dc_intl::t("CpEUP/"), "All Servers");
        assert_eq!(dc_intl::t("Xe+fJM"), "No results found");
    }
}
