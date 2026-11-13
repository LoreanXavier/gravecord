//! The widgets column of the editor (`container_y4ZEM5`): the Board / Activity / Wishlist tab bar and the Board's
//! empty state (`aQ` of module 927071, `empty_jFcvcH`): the heading and the grid of widgets to add
//! (`widgetGrid_jFcvcH`, tiles of module 297520).
//!
//! The grid lists every addable widget (`a$`, module 116281's order: the featured games' widgets, then Favorite Game,
//! Games I Like, Games in Rotation, Want to Play), or, for an account in
//! `2026-09-profile-widget-empty-state-suggestions`, the `maxWidgetOptions` suggestions of `aw` and Browse All
//! Widgets. The featured games and every picture are the ones of the capture
//! (`design/dom/settings/profile-editor-darker-cozy.json`): `/widget-configs/featured` itself is not fetched.

use super::panel::plus_badge;
use super::{Mode, Tab, text};
use crate::parity::Parity;
use crate::ui::{Root, color, icon};
use gpui::{AnyElement, Context, Div, IntoElement, div, img, prelude::*, px};

/// `2026-09-profile-widget-empty-state-suggestions` (module 927071 `aX`): variation 1 offers 4, variation 2 offers 6.
const SUGGESTIONS_EXPERIMENT: &str = "2026-09-profile-widget-empty-state-suggestions";

/// How a featured game's add tile pictures it (`ADD_WIDGET_PREVIEW` of its widget config, module 297520 `U`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Art {
    /// `heroImage_Ls3EVK`: the picture across the tile's right 40%, at its own aspect ratio, faded.
    Hero(&'static str, f32),
    /// `image_VMx4zg`: a square picture inset 16px in the right 40%.
    Square(&'static str),
    /// `imageSkeleton_VMx4zg`: the square's empty skeleton.
    Skeleton,
}

/// A featured game with a widget (`sk.R` application widgets).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Featured {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    art: Art,
}

const CDN: &str = "https://cdn.discordapp.com";

/// The featured games of the capture, in its order.
const FEATURED: [Featured; 6] = [
    Featured {
        id: "1346069614634864772",
        name: "Marvel Rivals",
        icon: "/app-icons/1346069614634864772/2f7be4ce3fb3261f76e544c56709a6ad.png?size=16&keep_aspect_ratio=false",
        art: Art::Hero(
            "/app-assets/1346069614634864772/1509836387271053342.webp?size=4096",
            1.00314,
        ),
    },
    Featured {
        id: "672188101707169852",
        name: "Wolvesville",
        icon: "/app-icons/672188101707169852/b7a64ea9edb7cb22588dd6530bd66c35.png?size=16&keep_aspect_ratio=false",
        art: Art::Skeleton,
    },
    Featured {
        id: "1323482066758930452",
        name: "Wuthering Waves",
        icon: "/app-icons/1323482066758930452/0a424370271f81fa7399c0938966aadd.png?size=16&keep_aspect_ratio=false",
        art: Art::Hero(
            "/app-assets/1323482066758930452/1506737434887458867.webp?size=300",
            0.75,
        ),
    },
    Featured {
        id: "1463856624455254103",
        name: "Heartopia",
        icon: "/app-icons/1463856624455254103/a7bd13821ede07bf4df788e48609d59e.png?size=16&keep_aspect_ratio=false",
        art: Art::Square("/app-assets/1463856624455254103/1550358418597085287.webp?size=2048"),
    },
    Featured {
        id: "1496037472226906183",
        name: "Arknights Endfield",
        icon: "/app-icons/1496037472226906183/f7877b08efb704adc7f7992af7e3b799.png?size=16&keep_aspect_ratio=false",
        art: Art::Hero(
            "/app-assets/1496037472226906183/1519406898787520646.webp?size=300",
            0.666_667,
        ),
    },
    Featured {
        id: "1481180707966685245",
        name: "Neverness to Everness",
        icon: "/app-icons/1481180707966685245/8751bdb4c480ed61d49f9ff6d0d471f8.png?size=16&keep_aspect_ratio=false",
        art: Art::Square("/app-assets/1481180707966685245/1552590719875551272.webp?size=480"),
    },
];

/// The game covers of the placeholders (module 297520 `f`): League of Legends; Valorant; Peak, Battlefield 6, R.E.P.O.;
/// Marvel Rivals, World of Warcraft, Rust (the fourth of each grid is clipped).
const LEAGUE: &str = "https://cdn.discordapp.com/app-icons/1402418696126992445/fae8bbf6a5d14f494db342b5d53eea21.webp?size=128&keep_aspect_ratio=true";
const VALORANT: &str = "https://cdn.discordapp.com/app-icons/700136079562375258/9c55c3dbef3b1621d17ee32dc7765dcc.webp?size=128&keep_aspect_ratio=true";
const LIKED: [&str; 3] = [
    "https://images-ext-1.discordapp.net/external/GTMVuQIs0v5kSO3AeNN1D7YhN5b1E19OkHtBuS9HYMI/https/images.igdb.com/igdb/image/upload/t_cover_big/cocokj.jpg?keep_aspect_ratio=true&format=webp",
    "https://cdn.discordapp.com/app-icons/1402692356343599254/e15fd23de24370b3a882500315de4cf7.webp?size=128&keep_aspect_ratio=true",
    "https://images-ext-1.discordapp.net/external/PXGulRTdhLH3WyICOOLRsfQbfgrDovCiIVtYNyRs8D0/https/images.igdb.com/igdb/image/upload/t_cover_big/co9lxz.jpg?keep_aspect_ratio=true&format=webp",
];
const WANTED: [&str; 3] = [
    "https://cdn.discordapp.com/app-icons/1314395942253756416/ddfdd542275573a097aa9bcddfba7cf5.webp?size=128&keep_aspect_ratio=true",
    "https://cdn.discordapp.com/app-icons/356875762940379136/52a7ab86855bd1f28cb0871daecef1ee.webp?size=128&keep_aspect_ratio=true",
    "https://images-ext-1.discordapp.net/external/Yl44h7wUxiLNpstWKVClimZPFuvh6mY7i0SHN6PhASQ/https/images.igdb.com/igdb/image/upload/t_cover_big/coajjj.jpg?keep_aspect_ratio=true&format=webp",
];

/// The game list widgets' titles: Favorite Game, Games I Like, Games in Rotation, Want to Play.
const LIST_TITLES: [&str; 4] = [
    "Favorite Game",
    "Games I Like",
    "Games in Rotation",
    "Want to Play",
];

/// One addable widget (`WidgetType` of module 540185, the ones `useAddableWidgets` offers here).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Widget {
    Application(Featured),
    FavoriteGames,
    PlayedGames,
    CurrentGames,
    WantToPlayGames,
}

impl Widget {
    /// `Ok` of module 229231, the title under the tile's icon (a game's own name is not a message).
    fn title(self) -> &'static str {
        match self {
            Self::Application(app) => app.name,
            Self::FavoriteGames => LIST_TITLES[0],
            Self::PlayedGames => LIST_TITLES[1],
            Self::CurrentGames => LIST_TITLES[2],
            Self::WantToPlayGames => LIST_TITLES[3],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Application(app) => app.name,
            _ => dc_intl::tr(self.title()),
        }
    }
}

/// `a$`: every addable widget in `Zc`'s type order.
fn addable() -> Vec<Widget> {
    FEATURED
        .iter()
        .map(|app| Widget::Application(*app))
        .chain([
            Widget::FavoriteGames,
            Widget::PlayedGames,
            Widget::CurrentGames,
            Widget::WantToPlayGames,
        ])
        .collect()
}

/// `aw` without the connected or recently played games, the clips gallery and the personal widget (none of them is
/// offered here): Favorite Game, Games I Like, Marvel Rivals, Games in Rotation, Want to Play, Wuthering Waves, cut
/// to `max`.
fn suggested(max: usize) -> Vec<Widget> {
    let all = addable();
    let app = |id: &str| {
        all.iter()
            .find(|widget| matches!(widget, Widget::Application(app) if app.id == id))
            .copied()
    };
    [
        Some(Widget::FavoriteGames),
        Some(Widget::PlayedGames),
        app("1346069614634864772"),
        Some(Widget::CurrentGames),
        Some(Widget::WantToPlayGames),
        app("1323482066758930452"),
    ]
    .into_iter()
    .flatten()
    .take(max)
    .collect()
}

/// `maxWidgetOptions` of the account's variation.
fn suggestion_count(variant: Option<i64>) -> Option<usize> {
    match variant {
        Some(1) => Some(4),
        Some(2) => Some(6),
        _ => None,
    }
}

fn skeleton(width: f32, height: f32, radius: f32) -> Div {
    div()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .rounded(px(radius))
        .bg(color("--background-mod-subtle"))
}

/// `coverContainer_R0hw0B placeholder`: a 56px game cover at 20%.
fn cover(image: Option<std::sync::Arc<gpui::Image>>) -> Div {
    div()
        .flex_none()
        .w(px(56.))
        .h(px(74.66))
        .rounded(px(8.))
        .overflow_hidden()
        .opacity(0.2)
        .bg(color("--background-base-low"))
        .children(image.map(|image| {
            img(image)
                .w(px(56.))
                .h(px(74.66))
                .object_fit(gpui::ObjectFit::Cover)
        }))
}

impl Root {
    fn widget_image(&self, url: &str) -> Option<std::sync::Arc<gpui::Image>> {
        self.images.get(url).cloned()
    }

    fn request_widget_art(&mut self, widgets: &[Widget]) {
        let mut urls: Vec<String> = Vec::new();
        for widget in widgets {
            match widget {
                Widget::Application(app) => {
                    urls.push(format!("{CDN}{}", app.icon));
                    if let Art::Hero(url, _) | Art::Square(url) = app.art {
                        urls.push(format!("{CDN}{url}"));
                    }
                }
                Widget::FavoriteGames => urls.push(LEAGUE.to_owned()),
                Widget::CurrentGames => urls.push(VALORANT.to_owned()),
                Widget::PlayedGames => urls.extend(LIKED.iter().map(|url| (*url).to_owned())),
                Widget::WantToPlayGames => urls.extend(WANTED.iter().map(|url| (*url).to_owned())),
            }
        }
        for url in urls {
            self.request_image(url.clone(), url);
        }
    }

    /// One add tile (`addButtonContainer_ECBSq8`, size small): the widget's placeholder picture behind the add (or
    /// link, for a game whose account is not linked) icon and the title.
    fn widget_tile(&self, widget: Widget, width: f32) -> Div {
        let picture = match widget {
            Widget::Application(app) => {
                // `container_5TW5x6`: the skeleton widget on the left 60%, the art on the right 40%, 4px apart.
                let art_width = (width - 4.) * 0.4;
                let content_width = (width - 4.) * 0.6 - 32.;
                let stat = (content_width - 16.) / 3.;
                let art: Div = match app.art {
                    Art::Hero(url, ratio) => div()
                        .w(px(art_width))
                        .h(px(99.))
                        .overflow_hidden()
                        .children(self.widget_image(&format!("{CDN}{url}")).map(|image| {
                            img(image)
                                .w(px(art_width))
                                .h(px(art_width / ratio))
                                .opacity(0.2)
                        })),
                    Art::Square(url) => div().w(px(art_width)).p(px(16.)).child(
                        div()
                            .size(px(art_width - 32.))
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(color("--background-mod-subtle"))
                            .children(
                                self.widget_image(&format!("{CDN}{url}"))
                                    .map(|image| img(image).size(px(art_width - 32.)).opacity(0.2)),
                            ),
                    ),
                    Art::Skeleton => div().w(px(art_width)).p(px(16.)).child(skeleton(
                        art_width - 32.,
                        art_width - 32.,
                        8.,
                    )),
                };
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .gap(px(4.))
                    .child(
                        div()
                            .w(px(content_width + 32.))
                            .p(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(skeleton(content_width / 2., 12., 8.))
                            .child(
                                div()
                                    .w(px(content_width))
                                    .h(px(1.))
                                    .bg(color("--background-mod-subtle")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .w(px(content_width))
                                    .children((0..6).map(|_| skeleton(stat, 15., 12.))),
                            ),
                    )
                    .child(art)
            }
            // `placeholderDetailsCard_6BQISv`: one cover and two text bars.
            Widget::FavoriteGames | Widget::CurrentGames => {
                let url = if widget == Widget::FavoriteGames {
                    LEAGUE
                } else {
                    VALORANT
                };
                div()
                    .absolute()
                    .inset_0()
                    .p(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(cover(self.widget_image(url)))
                    .child(
                        div()
                            .w(px(120.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(skeleton(72., 12., 8.))
                            .child(skeleton(114., 12., 8.)),
                    )
            }
            // `placeholderCoverGrid_6BQISv`: a row of covers, clipped at 184px.
            Widget::PlayedGames | Widget::WantToPlayGames => {
                let urls = if widget == Widget::PlayedGames {
                    LIKED
                } else {
                    WANTED
                };
                div().absolute().inset_0().p(px(12.)).child(
                    div()
                        .w(px(184.))
                        .overflow_hidden()
                        .flex()
                        .gap(px(8.))
                        .children(urls.iter().map(|url| cover(self.widget_image(url)))),
                )
            }
        };
        let (badge, app_icon) = match widget {
            Widget::Application(app) => (
                icon("discover_link", 24., "--text-default").into_any_element(),
                Some(app.icon),
            ),
            _ => (plus_badge().into_any_element(), None),
        };
        let title = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .children(app_icon.map(|path| {
                div()
                    .size(px(16.))
                    .rounded(px(2.))
                    .overflow_hidden()
                    .children(
                        self.widget_image(&format!("{CDN}{path}"))
                            .map(|image| img(image).size(px(16.))),
                    )
            }))
            .child(text(widget.label(), 16., 20., 500., "--text-strong"));
        div()
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(if matches!(widget, Widget::Application(_)) {
                99.
            } else {
                98.66
            }))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-mod-subtle"))
            .child(picture)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(badge)
                    .child(title),
            )
    }

    /// The Board's empty state: heading, the grid of tiles (two to a row, an odd last one centred) and, with the
    /// suggestions, Browse All Widgets; centred in the panel's height when it fits.
    fn profile_editor_board(&mut self) -> Div {
        let open = self.profile_editor.as_ref().expect("editor is open");
        let geometry = open.geometry;
        #[cfg(feature = "capture")]
        let forced = open.suggestions;
        #[cfg(not(feature = "capture"))]
        let forced: Option<usize> = None;
        let count = forced
            .or_else(|| suggestion_count(self.stores.experiments.variant(SUGGESTIONS_EXPERIMENT)));
        let widgets = match count {
            Some(max) => suggested(max),
            None => addable(),
        };
        self.request_widget_art(&widgets);
        let tile_width = geometry.tile_width();
        let rows: Vec<Div> = widgets
            .chunks(2)
            .map(|pair| {
                div()
                    .flex()
                    .gap(px(16.))
                    .when(pair.len() == 1, |this| this.justify_center())
                    .children(
                        pair.iter()
                            .map(|widget| self.widget_tile(*widget, tile_width)),
                    )
            })
            .collect();
        // `tabPanelScroller_y4ZEM5` is the tab panel (the card's height less the 48px tab bar) less 20px above and
        // 16px below; `empty_jFcvcH` (`flex: 1`, centred) fills it.
        let fill = (geometry.card_height() - 48. - 36.).max(0.);
        div()
            .w(px(geometry.board_width()))
            .min_h(px(fill))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(24.))
            .child(
                div()
                    .w(px(320.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(text(
                        dc_intl::tr("Customize your profile with Widgets"),
                        16.,
                        20.,
                        500.,
                        "--text-strong",
                    ))
                    .child(
                        div()
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::tr(
                                "Choose from our library of Widgets to share more about yourself and your interests",
                            )),
                    ),
            )
            .child(
                div()
                    .w(px(geometry.board_width()))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .children(rows),
            )
            .children(count.map(|_| {
                // `na.$` size sm, secondary: Browse All Widgets (the widget library, module 414904, is not built).
                div()
                    .id("profile-editor-browse-widgets")
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--control-secondary-background-default"))
                    .hover(|style| style.bg(color("--control-secondary-background-hover")))
                    .flex()
                    .items_center()
                    .child(text(
                        dc_intl::tr("Browse All Widgets"),
                        14.,
                        18.,
                        500.,
                        "--control-secondary-text-default",
                    ))
            }))
    }

    pub(super) fn profile_editor_widgets(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.profile_editor.as_ref().expect("editor is open");
        let (tab, nitro) = (open.tab, open.mode == Mode::Nitro);
        let geometry = open.geometry;
        let tabs: &[(Tab, &'static str)] = if nitro {
            &[
                (Tab::Activity, dc_intl::tr("Activity")),
                (Tab::Wishlist, dc_intl::tr("Wishlist")),
            ]
        } else {
            &[
                (Tab::Board, dc_intl::tr("Board")),
                (Tab::Activity, dc_intl::tr("Activity")),
                (Tab::Wishlist, dc_intl::tr("Wishlist")),
            ]
        };
        let bar = div()
            .flex()
            .border_b_1()
            .border_color(color("--border-subtle"))
            .children(tabs.iter().enumerate().map(|(index, (kind, name))| {
                let selected = *kind == tab;
                let kind = *kind;
                div()
                    .id(*name)
                    .h(px(30.))
                    .when(index > 0, |this| this.ml(px(24.)))
                    .cursor_pointer()
                    .border_b_2()
                    .border_color(if selected {
                        color("--text-strong")
                    } else {
                        gpui::rgba(0)
                    })
                    .child(text(
                        *name,
                        14.,
                        18.,
                        if selected { 600. } else { 500. },
                        if selected {
                            "--text-strong"
                        } else {
                            "--text-muted"
                        },
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.tab = kind;
                        }
                        cx.notify();
                    }))
            }));
        let content: AnyElement = match tab {
            Tab::Board => self.profile_editor_board().into_any_element(),
            Tab::Activity => self.profile_editor_activity_tab().into_any_element(),
            Tab::Wishlist => self
                .profile_editor_wishlist_tab(window, cx)
                .into_any_element(),
        };
        let board = geometry.board_width();
        div()
            .parity("profile-editor-widgets")
            .w(px(geometry.widgets_width() + 8.))
            .h_full()
            .flex_none()
            .pl(px(36.))
            .pt(px(17.))
            .ml(px(-4.))
            .flex()
            .flex_col()
            .child(div().w(px(board)).child(bar))
            .child(
                div()
                    .id("profile-editor-widgets-scroll")
                    .w(px(board))
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pt(px(19.))
                    .pb(px(16.))
                    .child(content),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capture's grid: the six featured games, then the four game list widgets.
    #[test]
    fn every_addable_widget_in_the_captured_order() {
        let titles: Vec<&str> = addable().iter().map(|widget| widget.title()).collect();
        assert_eq!(
            titles,
            [
                "Marvel Rivals",
                "Wolvesville",
                "Wuthering Waves",
                "Heartopia",
                "Arknights Endfield",
                "Neverness to Everness",
                "Favorite Game",
                "Games I Like",
                "Games in Rotation",
                "Want to Play",
            ]
        );
    }

    /// `aw` with six options gives the owner's board; variation 1 keeps four.
    #[test]
    fn the_suggestions_follow_aw() {
        let titles =
            |max| -> Vec<&str> { suggested(max).iter().map(|widget| widget.title()).collect() };
        assert_eq!(
            titles(6),
            [
                "Favorite Game",
                "Games I Like",
                "Marvel Rivals",
                "Games in Rotation",
                "Want to Play",
                "Wuthering Waves",
            ]
        );
        assert_eq!(titles(4).len(), 4);
        assert_eq!(suggestion_count(Some(2)), Some(6));
        assert_eq!(suggestion_count(Some(1)), Some(4));
        assert_eq!(suggestion_count(Some(0)), None);
        assert_eq!(suggestion_count(None), None);
    }

    /// Every tile title is an en-US message.
    #[test]
    fn the_titles_are_messages() {
        for widget in addable() {
            if !matches!(widget, Widget::Application(_)) {
                assert!(dc_intl::keys_of(widget.title()).next().is_some());
            }
        }
    }
}
