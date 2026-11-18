//! ShowConnections (Vencord `src/plugins/showConnections`, GPL-3.0-or-later): a person's connected accounts as a row
//! of platform icons in the profile popout and the DM sidebar (and with `showOnModal` in the full profile), each with
//! its name as the tooltip; a click opens the account's page, or copies the name where there is none. Not part of the
//! pinned client.

use super::super::tooltip::{Body, Side, Tip};
use super::super::{Root, color};
use crate::plugins::State;
use dc_state::Id;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, img, px,
};
use serde_json::Value;

const ID: &str = "ShowConnections";

/// `getSpacingPx`: Compact 4, Cozy 6, Roomy 8.
pub(super) fn spacing_px(spacing: i64) -> f32 {
    (spacing.clamp(0, 2) * 2 + 4) as f32
}

/// The settings that shape the row: icon size, spacing, and whether the full profile shows it.
fn read(state: &State) -> Option<(f32, f32, bool)> {
    state.is_on(ID).then(|| {
        (
            state.value(ID, "iconSize").as_f64().unwrap_or(32.) as f32,
            spacing_px(state.value(ID, "iconSpacing").as_i64().unwrap_or(1)),
            state.value(ID, "showOnModal") == Value::Bool(true),
        )
    })
}

impl Root {
    /// The row for `user`, or nothing. `modal`: the full profile, shown only with `showOnModal`.
    pub(in crate::ui) fn connections_row(
        &self,
        user: Id,
        modal: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if crate::plugins::bypassed() {
            return None;
        }
        let (size, gap, on_modal) = crate::plugins::with(read)?;
        if modal && !on_modal {
            return None;
        }
        let dark = !matches!(super::super::theme(), dc_tokens::Theme::Light);
        let accounts = self.connected_account_icons(user, dark, cx);
        if accounts.is_empty() {
            return None;
        }
        let icons = accounts.into_iter().enumerate().map(
            |(index, (kind, name, verified, url, picture))| {
                let label = if verified {
                    format!("{name} \u{2713}")
                } else {
                    name.clone()
                };
                let loading = picture.is_none();
                let target = div()
                    .id(SharedString::from(format!("connection-{user}-{index}")))
                    .size(px(size))
                    .cursor_pointer()
                    // A placeholder square while the platform icon loads.
                    .when(loading, |this| {
                        this.rounded(px(4.)).bg(color("--background-mod-subtle"))
                    })
                    .children(picture.map(|picture| img(picture).size(px(size))))
                    .on_click(move |_, _, cx| match (&url, kind.as_str()) {
                        (Some(url), _) => cx.open_url(url),
                        (None, "xbox") => cx.open_url(&format!(
                            "https://www.xbox.com/en-US/play/user/{}",
                            dc_http::percent_encode(&name)
                        )),
                        (None, _) => cx.write_to_clipboard(ClipboardItem::new_string(name.clone())),
                    });
                self.with_tip(
                    format!("connection-tip-{user}-{index}"),
                    Tip::body(Body::Text(label.into()), Side::Top),
                    target,
                    cx,
                )
                .into_any_element()
            },
        );
        Some(
            div()
                .flex()
                .flex_wrap()
                .gap(px(gap))
                .children(icons)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spacing_and_settings_read_as_the_original() {
        assert_eq!((spacing_px(0), spacing_px(1), spacing_px(2)), (4., 6., 8.));
        assert_eq!(read(&State::default()), None);
        let mut state = State::default();
        state.set_enabled(ID, true);
        assert_eq!(read(&state), Some((32., 6., true)));
        state.set_value(ID, "iconSize", &json!(20));
        state.set_value(ID, "showOnModal", &json!(false));
        assert_eq!(read(&state), Some((20., 6., false)));
    }
}
