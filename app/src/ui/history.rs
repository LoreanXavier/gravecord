//! Back and forward through visited routes: the titlebar arrows (`backForwardButtons_nS9Yqn`), Alt+Left/Alt+Right
//! (`NAVIGATE_BACK`/`NAVIGATE_FORWARD`, `alt+left`/`alt+right` on Windows) and the mouse's back/forward buttons. The
//! official client walks the router's history (`rD.OE`/`rD.Qb`); a route here is the selected guild and channel.

use super::{Root, color};
use dc_state::Id;
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Route {
    pub guild: Option<Id>,
    pub channel: Option<Id>,
    /// The Nitro page on Home (`/store`).
    pub nitro: bool,
    pub guild_guide: bool,
    pub channel_browser: bool,
    /// The Discover page and the part of it shown.
    pub discover: Option<super::discover::Place>,
    /// Quest Home (`/quest-home`).
    pub quests: bool,
    /// The Shop page on Home (`/shop`).
    pub shop: bool,
}

#[derive(Default)]
pub(crate) struct History {
    back: Vec<Route>,
    forward: Vec<Route>,
    /// Set while a back/forward step applies its route, so the step itself is not recorded.
    replaying: bool,
}

impl History {
    /// Called with the route being left whenever a new one is entered.
    pub fn visit(&mut self, from: Route, to: Route) {
        if self.replaying || from == to {
            return;
        }
        self.back.push(from);
        self.forward.clear();
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

impl Root {
    pub(super) fn route(&self) -> Route {
        Route {
            guild: self.guild,
            channel: if self.channel_browser_open {
                None
            } else {
                self.channel
            },
            nitro: self.nitro_open,
            guild_guide: self.guild_guide_open,
            channel_browser: self.channel_browser_open,
            discover: self.discover_place(),
            quests: self.quests_active(),
            shop: self.shop_active(),
        }
    }

    fn apply_route(&mut self, route: Route, cx: &mut Context<Self>) {
        self.history.replaying = true;
        match route.guild {
            Some(guild) if self.stores.guilds.iter().any(|known| known.id == guild) => {
                self.select(guild, route.channel);
                self.guild_guide_open = route.guild_guide && self.has_guild_guide();
                self.channel_browser_open = route.channel_browser && self.has_channel_browser();
            }
            Some(_) => {}
            None if route.discover.is_some() => {
                if let Some(place) = route.discover {
                    self.set_discover_place(place, cx);
                }
            }
            None if route.quests => self.set_quests_open(cx),
            None if route.shop => self.set_shop_open(cx),
            None => {
                self.select_home(route.channel);
                if route.nitro {
                    self.nitro_open = true;
                    self.request_nitro();
                }
            }
        }
        self.history.replaying = false;
        cx.notify();
    }

    pub(super) fn go_back(&mut self, cx: &mut Context<Self>) {
        let Some(route) = self.history.back.pop() else {
            return;
        };
        let here = self.route();
        self.history.forward.push(here);
        self.apply_route(route, cx);
    }

    pub(super) fn go_forward(&mut self, cx: &mut Context<Self>) {
        let Some(route) = self.history.forward.pop() else {
            return;
        };
        let here = self.route();
        self.history.back.push(here);
        self.apply_route(route, cx);
    }

    /// One titlebar arrow: 26 by 24, the 18px icon padded 6px on the outer side and 2px on the inner; disabled at 30%
    /// opacity, hovered on `--background-mod-normal` with `--interactive-text-hover`.
    pub(super) fn history_button(&self, back: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = if back {
            self.history.can_go_back()
        } else {
            self.history.can_go_forward()
        };
        let name = if back { "title_back" } else { "title_forward" };
        let group = SharedString::from(format!("history-{name}"));
        div()
            .id(name)
            .group(group.clone())
            .absolute()
            .left(px(if back { 9. } else { 37. }))
            .top(px(4.))
            .w(px(26.))
            .h(px(24.))
            .flex()
            .items_center()
            .when(back, |this| this.pl(px(6.)).pr(px(2.)).rounded_l(px(5.)))
            .when(!back, |this| this.pl(px(2.)).pr(px(6.)).rounded_r(px(5.)))
            .when(!enabled, |this| this.opacity(0.3))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .active(|style| style.opacity(0.7))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if back {
                            this.go_back(cx);
                        } else {
                            this.go_forward(cx);
                        }
                    }))
            })
            .child(
                gpui::svg()
                    .path(format!("icons/{name}.svg"))
                    .size(px(18.))
                    .flex_none()
                    .text_color(color("--interactive-text-default"))
                    .when(enabled, |this| {
                        this.group_hover(group, |style| {
                            style.text_color(color("--interactive-text-hover"))
                        })
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{History, Route};

    #[test]
    fn a_new_visit_clears_forward() {
        let mut history = History::default();
        let a = Route::default();
        let b = Route {
            guild: Some(1),
            channel: Some(2),
            nitro: false,
            guild_guide: false,
            channel_browser: false,
            discover: None,
            quests: false,
            shop: false,
        };
        history.visit(a, b);
        assert!(history.can_go_back() && !history.can_go_forward());
        history.forward.push(a);
        history.visit(b, a);
        assert!(!history.can_go_forward());
        history.visit(a, a);
        assert_eq!(history.back.len(), 2);
    }
}
