//! Voice & Video's Entrance Sounds (`aG` in module 158935): the fieldset with its legend and description, "Choose a
//! Server" over `All Servers` and the guilds, and "Choose a sound" (Nitro). Measured in
//! `design/dom/settings/voice-video-advanced3-darker-cozy.json` (`fieldset_UGw2wN`, `customizationSection_wRbpaT`,
//! `soundButtonSettingContainer_VACzzt`).

use super::voice::SOUNDBOARD_GUIDE;
use super::{Root, color, mask_icon};
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};

/// The key of the generated page's row, and where the chosen server and the open list are kept in `setting_choices`.
pub(super) const ENTRANCE_SOUNDS: &str = "entrance_sounds_setting";
const OPEN: &str = "entrance_sounds_setting/open";
/// `CpEUP/`: the global option, first in the list.
const ALL_SERVERS: &str = "All Servers";

/// The options of "Choose a Server": the global one, then the guilds in the order the client lists them.
pub(super) fn server_options(guilds: impl IntoIterator<Item = String>) -> Vec<SharedString> {
    std::iter::once(dc_intl::tr(ALL_SERVERS).to_owned())
        .chain(guilds)
        .map(SharedString::from)
        .collect()
}

impl Root {
    /// The fieldset: legend, description, then the two sections 24px apart (`stack_TWWX0S`, gap 24).
    pub(super) fn entrance_sounds_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let options = server_options(self.stores.guilds.iter().map(|guild| guild.name.clone()));
        let server =
            self.select_with_list(ENTRANCE_SOUNDS, OPEN, &options, 0, ALL_SERVERS, None, cx);
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
                    .child(section(dc_intl::tr("Choose a Server"), false, server)),
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
}
