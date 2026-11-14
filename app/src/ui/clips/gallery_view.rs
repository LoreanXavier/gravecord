//! What the gallery draws (stylesheets AH5XKr the container, ncTrRG the sidebar, yzLwxU the grid, PupK8L the card; modules
//! 657766 `lc`/`tn`/`e1`, 409067 the data, 590146 the card menu).
use super::super::keyboard::{Layer, outside};
use super::super::{Root, color, icon};
use super::gallery::{self, Facet, State};
use super::widgets::{self, Size};
use super::{Modal, actions, editor};
use crate::parity::Parity;
use dc_clips::library::Clip;
use gpui::{
    AnyElement, Context, FontWeight, ObjectFit, SharedString, Window, anchored, deferred, div, img,
    prelude::*, px,
};

/// The sidebar `lc` is 73 wide (stylesheet ncTrRG).
const RAIL: f32 = 73.;
/// `--custom-clips-header-height`.
const HEADER: f32 = 68.;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

impl Root {
    /// `openClipsGallery` (module 271597): the modal opens on the grid, on a game when one is given.
    pub(in crate::ui) fn clips_open_gallery(&mut self, facet: Facet, cx: &mut Context<Self>) {
        self.clips_close_menus();
        self.clips_close_editor(cx);
        self.clips.engine.ui.modal = Some(Modal {
            editor: None,
            direct: false,
            gallery: Some(State::new(facet)),
        });
        cx.notify();
    }

    fn clips_gallery_mut(&mut self) -> Option<&mut State> {
        self.clips
            .engine
            .ui
            .modal
            .as_mut()
            .and_then(|modal| modal.gallery.as_mut())
    }

    /// A card was pressed: the editor opens over the grid and closing it returns to the grid.
    pub(super) fn clips_edit_from_grid(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(clip) = self.clips.engine.clips.get(id).cloned() else {
            return;
        };
        self.clips_close_menus();
        let editor = editor::Editor::open(&clip, cx);
        if let Some(modal) = self.clips.engine.ui.modal.as_mut() {
            modal.editor = Some(editor);
        }
        self.clips_start_polling(cx);
        cx.notify();
    }

    pub(super) fn clips_card_menu_open(&self) -> bool {
        self.clips
            .engine
            .ui
            .modal
            .as_ref()
            .and_then(|modal| modal.gallery.as_ref())
            .is_some_and(|state| state.menu.is_some())
    }

    pub(super) fn clips_close_card_menu(&mut self) {
        if let Some(state) = self.clips_gallery_mut() {
            state.menu = None;
        }
    }

    /// The favourite flag of any clip of the library (`toggleFavorite`, module 590146 `favorite`/`unfavorite`).
    pub(super) fn clips_favourite_clip(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(clip) = self.clips.engine.clips.get_mut(id) else {
            return;
        };
        let on = !clip.meta.is_favorite.unwrap_or(false);
        clip.meta.is_favorite = Some(on);
        let meta = clip.meta.clone();
        let path = clip.filepath.clone();
        self.clips_write(path, move || meta, cx);
        cx.notify();
    }

    /// The harness's view of the grid: the facet and what it lists.
    #[cfg(feature = "capture")]
    pub(super) fn clips_grid_status(&self) -> String {
        match self.clips_view_state() {
            Some(state) => {
                let list = gallery::visible(&self.clips.engine.clips, &state.facet);
                let ids: Vec<&str> = list.iter().map(|clip| clip.meta.id.as_str()).collect();
                format!(
                    "grid: facet {:?} clips {} {:?} menu {}",
                    state.facet,
                    list.len(),
                    ids,
                    state.menu.is_some()
                )
            }
            None => "grid: closed".to_owned(),
        }
    }

    fn clips_view_state(&self) -> Option<&State> {
        self.clips
            .engine
            .ui
            .modal
            .as_ref()
            .and_then(|modal| modal.gallery.as_ref())
    }

    /// The gallery inside the modal's box.
    pub(super) fn clips_gallery_view(
        &self,
        state: &State,
        size: (f32, f32),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let clips = &self.clips.engine.clips;
        let list = gallery::visible(clips, &state.facet);
        let content_width = size.0 - RAIL;
        div()
            .parity("clips-gallery")
            .size_full()
            .flex()
            .child(self.clips_rail(state, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .bg(color("--background-surface-high"))
                    .child(self.clips_gallery_header(state, list.len(), cx))
                    .child(if clips.is_empty() || list.is_empty() {
                        self.clips_empty(clips.is_empty())
                    } else {
                        self.clips_grid(state, &list, content_width, cx)
                    }),
            )
            .into_any_element()
    }

    fn clips_rail(&self, state: &State, cx: &mut Context<Self>) -> AnyElement {
        let item = |id: SharedString, selected: bool| {
            div()
                .id(id)
                .flex_none()
                .size(px(40.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(color("--text-muted"))
                .when(selected, |this| {
                    this.bg(color("--background-mod-subtle"))
                        .text_color(color("--text-default"))
                })
                .hover(|style| {
                    style
                        .bg(color("--background-mod-subtle"))
                        .text_color(color("--text-default"))
                })
        };
        let link = |id: &'static str, glyph: &'static str, facet: Facet| {
            let selected = state.facet == facet;
            item(id.into(), selected)
                .child(icon(glyph, 20., "--text-muted"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.clips_gallery_mut() {
                        state.facet = facet.clone();
                        state.menu = None;
                    }
                    cx.notify();
                }))
        };
        let mut games = div().flex().flex_col().items_center().gap(px(4.));
        for game in gallery::games(&self.clips.engine.clips) {
            let selected = state.facet == Facet::Game(game.id.clone());
            let facet = Facet::Game(game.id.clone());
            let letter: String = game.name.chars().take(1).collect::<String>().to_uppercase();
            games = games.child(
                item(
                    SharedString::from(format!("clips-game-{}", game.id)),
                    selected,
                )
                .child(
                    // The game's icon is a 32 px picture with `--radius-xs`; none is stored with a clip, so its
                    // first letter stands in.
                    div()
                        .size(px(32.))
                        .rounded(px(4.))
                        .bg(color("--background-mod-strong"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(letter),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.clips_gallery_mut() {
                        state.facet = facet.clone();
                        state.menu = None;
                    }
                    cx.notify();
                })),
            );
        }
        div()
            .flex_none()
            .w(px(RAIL))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(color("--background-base-lower"))
            .border_r_1()
            .border_color(color("--border-subtle"))
            .child(
                div()
                    .pt(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    // `dPVrEv` All Clips, `9rlCk1` Favorites.
                    .child(link("clips-rail-all", "clips_clips", Facet::All))
                    .child(link(
                        "clips-rail-favourites",
                        "clips_star_outline",
                        Facet::Favorites,
                    )),
            )
            .child(
                div()
                    .w(px(32.))
                    .h(px(1.))
                    .my(px(12.))
                    .bg(color("--border-subtle")),
            )
            .child(
                div()
                    .id("clips-rail-games")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(games),
            )
            .child(
                // `3D5yo/` Settings: the Clips page of user settings.
                div().pb(px(16.)).child(
                    item("clips-rail-settings".into(), false)
                        .child(icon("clips_settings", 20., "--text-muted"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_clips_modal(cx);
                            this.open_settings_page("clips", cx);
                        })),
                ),
            )
            .into_any_element()
    }

    fn clips_gallery_header(
        &self,
        state: &State,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = match &state.facet {
            Facet::All => "All Clips".to_owned(),
            Facet::Favorites => "Favorites".to_owned(),
            Facet::Game(id) => self
                .clips
                .engine
                .clips
                .values()
                .find(|clip| clip.meta.application_id.as_deref() == Some(id))
                .map(|clip| clip.meta.application_name.clone())
                .unwrap_or_default(),
        };
        div()
            .flex_none()
            .h(px(HEADER))
            .px(px(24.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().size(px(3.)).rounded_full().bg(color("--text-muted")))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-muted"))
                                    // `+YIqQM`.
                                    .child(if count == 1 {
                                        "1 clip".to_owned()
                                    } else {
                                        format!("{count} clips")
                                    }),
                            ),
                    ),
            )
            .child(
                widgets::icon_button("clips-gallery-close", "clips_close", Size::Md, false)
                    .on_click(cx.listener(|this, _, _, cx| this.close_clips_modal(cx))),
            )
            .into_any_element()
    }

    /// `ef`: nothing captured yet, or nothing in this view.
    fn clips_empty(&self, none_at_all: bool) -> AnyElement {
        let (heading, line) = if none_at_all {
            (
                // `m2GEpP`, `DLzdl7`.
                "Capture and share your best gaming moments, all in one place.",
                "Edit and share your clips anytime from here.",
            )
        } else {
            // `+M2iLf`, `xkY5xS`.
            ("No clips found", "Try searching for something else")
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .px(px(48.))
            .child(
                div()
                    .max_w(px(480.))
                    .text_center()
                    .text_size(px(28.))
                    .line_height(px(34.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(heading),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-muted"))
                    .child(line),
            )
            .into_any_element()
    }

    fn clips_grid(
        &self,
        state: &State,
        list: &[&Clip],
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = chrono::Local::now();
        let columns = gallery::columns(width);
        let tile = gallery::tile_width(width, columns);
        let mut body = div().flex().flex_col().px(px(24.));
        for section in gallery::sections(list, now) {
            let auto = list[section.start..section.end]
                .iter()
                .any(|clip| clip.meta.clip_method == "auto");
            let described = section.title == "Today" && auto;
            body = body.child(
                div()
                    .flex_none()
                    .h(px(if described { 66. } else { 44. }))
                    .py(px(12.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(section.title.clone()),
                    )
                    .children(described.then(|| {
                        // `6AXirz`.
                        div()
                            .mt(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child("Discord has automatically clipped some of these for you.")
                    })),
            );
            for row in list[section.start..section.end].chunks(columns) {
                body = body.child(
                    div()
                        .flex_none()
                        .flex()
                        .gap(px(20.))
                        .mb(px(20.))
                        .w_full()
                        .children(row.iter().map(|clip| self.clips_card(clip, tile, cx))),
                );
            }
        }
        div()
            .id("clips-grid")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&state.scroll)
            .child(body)
            .into_any_element()
    }

    /// `e1`: a clip card (stylesheet PupK8L).
    fn clips_card(&self, clip: &Clip, tile: f32, cx: &mut Context<Self>) -> AnyElement {
        let id = clip.meta.id.clone();
        let group = SharedString::from(format!("clips-card-{id}"));
        let picture = self
            .clips
            .engine
            .ui
            .pictures
            .borrow_mut()
            .entry(id.clone())
            .or_insert_with(|| super::panels::decode(&clip.meta.thumbnail))
            .clone();
        let is_new = self.clips.engine.new_clip_ids.contains(&id);
        let favourite = clip.meta.is_favorite.unwrap_or(false);
        let title = super::model::title(&clip.meta);
        let seconds = gallery::shown_length_ms(clip) / 1000;
        let meta_color = gpui::Rgba {
            a: 0.7,
            ..color("--text-overlay-light")
        };
        let overlay_button = |id: String| {
            div()
                .id(SharedString::from(id))
                .flex_none()
                .h(px(32.))
                .px(px(11.))
                .rounded(px(8.))
                .border_1()
                .border_color(gpui::rgba(0x97979f29))
                .bg(color("--control-overlay-secondary-background-default"))
                .hover(|style| style.bg(color("--control-overlay-secondary-background-hover")))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--control-overlay-secondary-text-default"))
        };
        let open_id = id.clone();
        let edit_id = id.clone();
        let menu_id = id.clone();
        let share_id = id.clone();
        let height = gallery::tile_height(tile);
        div()
            .id(SharedString::from(format!("clips-card-{id}")))
            .group(group.clone())
            .flex_none()
            .w(px(tile))
            .h(px(height))
            .relative()
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .bg(color("--background-mod-subtle"))
            .cursor_pointer()
            .hover(|style| {
                style.shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(4.)),
                    blur_radius: px(8.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
            .on_click(cx.listener(move |this, _, _, cx| this.clips_edit_from_grid(&open_id, cx)))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener({
                    let id = id.clone();
                    move |this, event: &gpui::MouseDownEvent, _, cx| {
                        let at = event.position;
                        this.close_all_context_menus();
                        if let Some(state) = this.clips_gallery_mut() {
                            state.menu = Some((id.clone(), at));
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .children(picture.map(|picture| {
                img(picture)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
            }))
            // The footer's scrim (`footerScrim`: a blur and a tint masked to fade upward, 94 px) as a gradient.
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(94.))
                    .bg(gpui::linear_gradient(
                        0.,
                        gpui::linear_color_stop(gpui::rgba(0x000000b8), 0.4),
                        gpui::linear_color_stop(gpui::rgba(0x00000000), 1.),
                    )),
            )
            // Header: the "new" badge and the star on the left and right, `More` while hovered.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .p(px(12.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(8.))
                    .child(div().children(is_new.then(|| {
                        div()
                            .rounded(px(100.))
                            .px(px(8.))
                            .py(px(4.))
                            .bg(color("--badge-notification-background"))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            // `y2b7CA`, upper-cased by the style.
                            .child("NEW")
                    })))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap(px(4.))
                            .child(
                                overlay_button(format!("clips-card-more-{id}"))
                                    .w(px(32.))
                                    .px(px(0.))
                                    .invisible()
                                    .group_hover(group.clone(), |style| style.visible())
                                    .child(icon(
                                        "clips_more",
                                        16.,
                                        "--control-overlay-secondary-text-default",
                                    ))
                                    .on_click(cx.listener(
                                        move |this, event: &gpui::ClickEvent, _, cx| {
                                            let at = event.position();
                                            this.close_all_context_menus();
                                            if let Some(state) = this.clips_gallery_mut() {
                                                state.menu = Some((menu_id.clone(), at));
                                            }
                                            cx.stop_propagation();
                                            cx.notify();
                                        },
                                    )),
                            )
                            .children(favourite.then(|| icon("clips_star", 16., "--yellow-300"))),
                    ),
            )
            // Footer: title and meta, replaced by the buttons while hovered.
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .px(px(12.))
                    .pb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .overflow_hidden()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-overlay-light"))
                            .child(title),
                    )
                    .child(
                        div()
                            .h(px(24.))
                            .relative()
                            .child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .left_0()
                                    .right_0()
                                    .min_h(px(24.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .group_hover(group.clone(), |style| style.invisible())
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_color(meta_color)
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .child(
                                                // The game's icon, or the screen glyph when there is none.
                                                div()
                                                    .size(px(16.))
                                                    .rounded(px(4.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon(
                                                        "clips_screen",
                                                        16.,
                                                        "--text-overlay-light",
                                                    )),
                                            )
                                            .child(gallery::ago(clip.meta.created_at, now_ms())),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-overlay-light"))
                                            .child(gallery::duration_text(seconds)),
                                    ),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .left_0()
                                    .right_0()
                                    .flex()
                                    .gap(px(8.))
                                    .invisible()
                                    .group_hover(group.clone(), |style| style.visible())
                                    .child(
                                        overlay_button(format!("clips-card-edit-{id}"))
                                            .flex_1()
                                            .child(icon(
                                                "clips_pencil",
                                                16.,
                                                "--control-overlay-secondary-text-default",
                                            ))
                                            // `bt75uw`.
                                            .child("Edit")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.clips_edit_from_grid(&edit_id, cx);
                                            })),
                                    )
                                    .child(
                                        div().flex_1().child(
                                            widgets::button(
                                                format!("clips-card-share-{id}"),
                                                "Share",
                                                Some("clips_share"),
                                                widgets::Variant::Primary,
                                                Size::Sm,
                                                true,
                                            )
                                            .on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.clips_share_clip(&share_id, window, cx);
                                                }),
                                            ),
                                        ),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The card menu (module 590146): Share and Edit, Favorite and Export, Show in Folder, Unfavorite and Delete.
    pub(in crate::ui) fn clips_card_menu_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (id, at) = self.clips_view_state()?.menu.clone()?;
        let clip = self.clips.engine.clips.get(&id)?;
        let favourite = clip.meta.is_favorite.unwrap_or(false);
        let row = |key: &'static str, label: &'static str, glyph: &'static str, danger: bool| {
            div()
                .id(SharedString::from(key))
                .h(px(36.))
                .flex_none()
                .p(px(8.))
                .rounded(px(2.))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(move |style| {
                    style
                        .bg(color(if danger {
                            "--background-feedback-mod-critical"
                        } else {
                            "--interactive-background-hover"
                        }))
                        .rounded(px(4.))
                })
                .child(div().mr(px(8.)).child(icon(
                    glyph,
                    18.,
                    if danger {
                        "--text-feedback-critical"
                    } else {
                        "--interactive-text-default"
                    },
                )))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color(if danger {
                            "--text-feedback-critical"
                        } else {
                            "--text-strong"
                        }))
                        .child(label),
                )
        };
        let separator = || {
            div()
                .flex_none()
                .my(px(8.))
                .mx(px(8.))
                .h(px(1.))
                .bg(color("--border-subtle"))
        };
        let act = |id: String, run: fn(&mut Root, &str, &mut Window, &mut Context<Root>)| {
            cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.clips_close_card_menu();
                run(this, &id, window, cx);
                cx.notify();
            })
        };
        let mut card = div()
            .id("clips-card-menu")
            .occlude()
            .w(px(220.))
            .p(px(8.))
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(color("--background-surface-higher"))
            .border_1()
            .border_color(color("--border-subtle"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(8.)),
                blur_radius: px(16.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                row("clips-card-menu-share", "Share", "clips_share", false)
                    .on_click(act(id.clone(), |this, id, window, cx| {
                        this.clips_share_clip(id, window, cx)
                    })),
            )
            .child(
                row("clips-card-menu-edit", "Edit", "clips_pencil", false)
                    .on_click(act(id.clone(), |this, id, _, cx| {
                        this.clips_edit_from_grid(id, cx)
                    })),
            )
            .child(separator());
        if !favourite {
            card = card.child(
                row(
                    "clips-card-menu-favourite",
                    "Favorite",
                    "clips_star_outline",
                    false,
                )
                .on_click(act(id.clone(), |this, id, _, cx| {
                    this.clips_favourite_clip(id, cx)
                })),
            );
        }
        card = card
            .child(
                row(
                    "clips-card-menu-export",
                    "Export Video File",
                    "clips_download",
                    false,
                )
                .on_click(act(id.clone(), |this, id, _, cx| {
                    this.clips_export_clip(id, cx)
                })),
            )
            .child(separator())
            .child(
                row(
                    "clips-card-menu-folder",
                    "Show in Folder",
                    "clips_folder",
                    false,
                )
                .on_click(act(id.clone(), |this, id, _, cx| {
                    this.clips_show_clip_in_folder(id, cx)
                })),
            )
            .child(separator());
        if favourite {
            card = card.child(
                row(
                    "clips-card-menu-unfavourite",
                    "Remove from Favorites",
                    "clips_x_small",
                    true,
                )
                .on_click(act(id.clone(), |this, id, _, cx| {
                    this.clips_favourite_clip(id, cx)
                })),
            );
        }
        card = card.child(
            row("clips-card-menu-delete", "Delete", "clips_trash", true).on_click(act(
                id.clone(),
                |this, id, _, cx| {
                    this.clips.engine.ui.confirm_delete = Some(id.to_owned());
                    cx.notify();
                },
            )),
        );
        Some(
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(outside(card, Layer::ClipMenu, cx)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// What the Share, Export and Show in Folder of a card do to a clip that is not open in the editor: the same as the
/// editor's, from the clip's stored edit.
impl Root {
    fn clips_stored_job(&self, id: &str) -> Option<actions::ShareClip> {
        let clip = self.clips.engine.clips.get(id)?;
        let (edit, _) = super::model::Edit::of(&clip.meta, clip.meta.length as f64 / 1000.);
        let mut meta = clip.meta.clone();
        meta.name = edit.name.clone();
        let title = super::model::title(&meta);
        Some(actions::ShareClip {
            source: clip.filepath.clone(),
            start_ms: (edit.crop.start * 1000.).round() as u64,
            end_ms: (edit.crop.end * 1000.).round() as u64,
            keep: edit.kept_tracks(),
            stem: actions::file_stem(&title),
            title,
            thumbnail: self
                .clips
                .engine
                .ui
                .pictures
                .borrow()
                .get(id)
                .cloned()
                .flatten(),
        })
    }

    pub(super) fn clips_share_clip(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(job) = self.clips_stored_job(id) {
            self.open_share_clip(job, window, cx);
        }
    }

    pub(super) fn clips_export_clip(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(job) = self.clips_stored_job(id) {
            self.clips_export_job(job, cx);
        }
    }

    pub(super) fn clips_show_clip_in_folder(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(path) = self
            .clips
            .engine
            .clips
            .get(id)
            .map(|clip| clip.filepath.clone())
        else {
            return;
        };
        self.clips_show_path(path, cx);
    }
}
