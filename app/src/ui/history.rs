//! Back and forward through visited routes: the titlebar arrows (`backForwardButtons_nS9Yqn`), Alt+Left/Alt+Right
//! (`NAVIGATE_BACK`/`NAVIGATE_FORWARD`, `alt+left`/`alt+right` on Windows) and the mouse's back/forward buttons. The
//! official client walks the router's history (`rD.OE`/`rD.Qb`); a route here is the selected guild and channel.

use super::tooltip::{Side, Tip};
use super::{Root, color};
use dc_state::Id;
use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Route {
    pub guild: Option<Id>,
    pub channel: Option<Id>,
    /// The Favorites server (`/channels/@favorites/...`).
    pub favorites: bool,
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
    /// The Message Requests page on Home (`/message-requests`).
    pub message_requests: bool,
    /// The Library (`/library`).
    pub library: bool,
}

#[derive(Default)]
pub(crate) struct History {
    back: Vec<Route>,
    forward: Vec<Route>,
    /// Set while a back/forward step applies its route, so the step itself is not recorded.
    pub(super) replaying: bool,
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
            // A message request's chat open beside its page is not a place of its own (`openPrivateChannelAsSidebar`).
            channel: if self.channel_browser_open || self.message_requests_active() {
                None
            } else {
                self.channel
            },
            favorites: self.in_favorites(),
            nitro: self.nitro_open,
            guild_guide: self.guild_guide_open,
            channel_browser: self.channel_browser_open,
            discover: self.discover_place(),
            quests: self.quests_active(),
            shop: self.shop_active(),
            message_requests: self.message_requests_active(),
            library: self.library_active(),
        }
    }

    fn apply_route(&mut self, route: Route, cx: &mut Context<Self>) {
        self.history.replaying = true;
        match route.guild {
            Some(guild) if self.stores.guilds.iter().any(|known| known.id == guild) => {
                self.select(guild, route.channel);
                self.favorites_view = route.favorites && self.favorites_guild_enabled();
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
            None if route.message_requests => self.set_message_requests_open(cx),
            None if route.library => self.set_library_open(self.library.tab, cx),
            None => {
                self.select_home(route.channel);
                self.favorites_view = route.favorites && self.favorites_guild_enabled();
                if route.nitro {
                    self.nitro_open = true;
                    self.request_nitro();
                }
            }
        }
        self.history.replaying = false;
        self.request_images();
        cx.notify();
    }

    /// `976860` `TX`: back/forward is blocked while a covering dialog or settings layer is up, and closes any open
    /// context menu before stepping.
    fn can_navigate_history(&mut self) -> bool {
        if self.covering_dialogs() > 0
            || self.server_settings.is_some()
            || self.channel_settings.is_some()
        {
            return false;
        }
        self.close_all_context_menus();
        true
    }

    pub(super) fn go_back(&mut self, cx: &mut Context<Self>) {
        if !self.can_navigate_history() {
            return;
        }
        let Some(route) = self.history.back.pop() else {
            cx.notify();
            return;
        };
        let here = self.route();
        self.history.forward.push(here);
        self.apply_route(route, cx);
    }

    pub(super) fn go_forward(&mut self, cx: &mut Context<Self>) {
        if !self.can_navigate_history() {
            return;
        }
        let Some(route) = self.history.forward.pop() else {
            cx.notify();
            return;
        };
        let here = self.route();
        self.history.back.push(here);
        self.apply_route(route, cx);
    }

    /// One titlebar arrow: 26 by 24, the 18px icon padded 6px on the outer side and 2px on the inner; disabled at 30%
    /// opacity, hovered on `--background-mod-normal` with `--interactive-text-hover`, active `--interactive-text-active`
    /// (`backForwardButtons_nS9Yqn`, module 596339). Both states `.occlude()` so pressing an arrow never drags the window
    /// (`leading_3zSH5d`: `-webkit-app-region: no-drag`).
    pub(super) fn history_button(&self, back: bool, cx: &mut Context<Self>) -> AnyElement {
        let enabled = if back {
            self.history.can_go_back()
        } else {
            self.history.can_go_forward()
        };
        let name = if back { "title_back" } else { "title_forward" };
        let group = SharedString::from(format!("history-{name}"));
        // A disabled arrow is not a keyboard stop, and carries no tooltip (`disabled` button in 596339).
        let base = div().id(name);
        let stop = if enabled {
            self.title_stop(base, name, 5., cx)
        } else {
            base
        };
        let button = stop
            .group(group.clone())
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
                    .id(SharedString::from(format!("{name}-icon")))
                    .size(px(18.))
                    .flex_none()
                    .text_color(color("--interactive-text-default"))
                    .when(enabled, |this| {
                        this.group_hover(group.clone(), |style| {
                            style.text_color(color("--interactive-text-hover"))
                        })
                        .group_active(group, |style| {
                            style.text_color(color("--interactive-text-active"))
                        })
                    }),
            );
        let left = px(if back { 9. } else { 37. });
        if enabled {
            let tip = Tip::shortcut(
                dc_intl::tr(if back { "Go back" } else { "Go forward" }),
                if back { "alt+left" } else { "alt+right" },
                Side::Bottom,
            );
            self.with_tip(
                if back { "title-back" } else { "title-forward" },
                tip,
                button,
                cx,
            )
            .occlude()
            .absolute()
            .left(left)
            .top(px(4.))
            .into_any_element()
        } else {
            button
                .occlude()
                .absolute()
                .left(left)
                .top(px(4.))
                .into_any_element()
        }
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
            favorites: false,
            nitro: false,
            guild_guide: false,
            channel_browser: false,
            discover: None,
            quests: false,
            shop: false,
            message_requests: false,
            library: false,
        };
        history.visit(a, b);
        assert!(history.can_go_back() && !history.can_go_forward());
        history.forward.push(a);
        history.visit(b, a);
        assert!(!history.can_go_forward());
        history.visit(a, a);
        assert_eq!(history.back.len(), 2);
    }

    #[test]
    fn favorites_route_is_distinct_from_same_guild_channel() {
        let mut history = History::default();
        let guild_route = Route {
            guild: Some(10),
            channel: Some(20),
            ..Default::default()
        };
        let favorites_route = Route {
            favorites: true,
            ..guild_route
        };
        history.visit(guild_route, favorites_route);
        assert_eq!(history.back, vec![guild_route]);
        history.visit(favorites_route, guild_route);
        assert_eq!(history.back, vec![guild_route, favorites_route]);
    }

    #[cfg(feature = "capture")]
    #[gpui::test]
    fn occluded_titlebar_buttons_block_window_drag_area(cx: &mut gpui::TestAppContext) {
        use gpui::{
            Context, Hitbox, HitboxBehavior, IntoElement, Window, WindowControlArea, canvas, div,
            point, prelude::*, px, size,
        };
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        struct DragProbe {
            drag_hitbox: Rc<RefCell<Option<Hitbox>>>,
            tip_hovered: Rc<Cell<bool>>,
        }
        impl Render for DragProbe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let slot = self.drag_hitbox.clone();
                let tip_hovered = self.tip_hovered.clone();
                div()
                    .relative()
                    .w(px(200.))
                    .h(px(32.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        canvas(
                            move |bounds, window, _| {
                                *slot.borrow_mut() =
                                    Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
                            },
                            |_, (), _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .id("tip-title-back")
                            .occlude()
                            .on_hover(move |inside, _, _| tip_hovered.set(*inside))
                            .absolute()
                            .left(px(9.))
                            .top(px(4.))
                            .child(div().id("title_back").w(px(26.)).h(px(24.))),
                    )
                    .child(
                        div()
                            .id("tip-title-inbox")
                            .occlude()
                            .absolute()
                            .left(px(140.))
                            .top(px(4.))
                            .child(div().id("inbox").size(px(24.))),
                    )
            }
        }

        let drag_hitbox = Rc::new(RefCell::new(None));
        let tip_hovered = Rc::new(Cell::new(false));
        let probe_hitbox = drag_hitbox.clone();
        let probe_tip = tip_hovered.clone();
        let window = cx.open_window(size(px(200.), px(32.)), |_, _| DragProbe {
            drag_hitbox: probe_hitbox,
            tip_hovered: probe_tip,
        });
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                let hitbox_id = drag_hitbox.borrow().as_ref().expect("drag hitbox").id;
                window.simulate_mouse_move(point(px(20.), px(16.)), cx);
                assert!(
                    !hitbox_id.should_handle_scroll(window),
                    "occluded back button must block underlying window drag hitbox"
                );
                assert!(
                    tip_hovered.get(),
                    "outer tooltip wrapper must receive hover while occluding drag"
                );
                window.simulate_mouse_move(point(px(150.), px(16.)), cx);
                assert!(
                    !hitbox_id.should_handle_scroll(window),
                    "occluded trailing titlebar icon must block underlying window drag hitbox"
                );
                assert!(!tip_hovered.get());
                window.simulate_mouse_move(point(px(80.), px(16.)), cx);
                assert!(
                    hitbox_id.should_handle_scroll(window),
                    "empty titlebar middle still reaches window drag hitbox"
                );
            })
            .unwrap();
    }
}
