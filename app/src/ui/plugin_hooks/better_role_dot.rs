//! BetterRoleDot (Vencord `src/plugins/betterRoleDot`, GPL-3.0-or-later): a click on a role dot copies the role's
//! colour (`#rrggbb`, "Copied to clipboard!"); `bothStyles` turns every `"username" === roleStyle` and `"dot" === roleStyle`
//! test true, so names keep their colour and the dot shows too; `copyRoleColorInProfilePopout` (without `bothStyles`)
//! draws the profile popout's roles with dots, which copy as well. Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use gpui::{
    AnyElement, ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div,
};
use serde_json::Value;

const ID: &str = "BetterRoleDot";

/// `(bothStyles, copyRoleColorInProfilePopout)` while the plugin is on.
fn read(state: &State) -> Option<(bool, bool)> {
    state.is_on(ID).then(|| {
        (
            state.value(ID, "bothStyles") == Value::Bool(true),
            state.value(ID, "copyRoleColorInProfilePopout") == Value::Bool(true),
        )
    })
}

/// `colorString`.
pub(super) fn color_string(rgb: u32) -> String {
    format!("#{rgb:06x}")
}

fn settings() -> Option<(bool, bool)> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(read)
}

impl Root {
    /// "Role Colors" is "Next to names", or `bothStyles` adds the dot to any style.
    pub(in crate::ui) fn role_dots_shown(&self) -> bool {
        self.pref_index("role_style", 0) == 1 || settings().is_some_and(|(both, _)| both)
    }

    /// "Role Colors" is "In names", or `bothStyles` keeps the colour in any style.
    pub(in crate::ui) fn role_names_colored(&self) -> bool {
        self.pref_index("role_style", 0) == 0 || settings().is_some_and(|(both, _)| both)
    }

    /// The profile popout's roles drawn with dots: "Next to names", `bothStyles`, or `copyRoleColorInProfilePopout`.
    pub(in crate::ui) fn popout_role_dots(&self) -> bool {
        self.pref_index("role_style", 0) == 1
            || settings().is_some_and(|(both, popout)| both || popout)
    }

    /// `dot` as it is, or with the plugin on a pointer that copies `rgb` (the press does not reach the row under it).
    pub(in crate::ui) fn role_dot_copy(
        &self,
        key: String,
        rgb: u32,
        dot: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if settings().is_none() {
            return dot.into_any_element();
        }
        div()
            .id(SharedString::from(format!("role-dot-copy-{key}")))
            .cursor_pointer()
            .child(dot)
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                cx.write_to_clipboard(ClipboardItem::new_string(color_string(rgb)));
                this.show_success_toast("Copied to clipboard!".to_owned(), cx);
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn colours_copy_as_color_strings_and_switches_read() {
        assert_eq!(color_string(0xc27c0e), "#c27c0e");
        assert_eq!(color_string(0xff), "#0000ff");
        let mut state = State::default();
        assert_eq!(read(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(read(&state), Some((false, false)));
        state.set_value(ID, "bothStyles", &json!(true));
        state.set_value(ID, "copyRoleColorInProfilePopout", &json!(true));
        assert_eq!(read(&state), Some((true, true)));
    }
}
