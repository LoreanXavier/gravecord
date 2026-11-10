//! The collectible pickers of the editor (modules 478733 nameplate, 50976 decoration, 720433 effect, 794561 frame):
//! "Your X" with the None and Shop tiles and the owned items, "Exclusive to Nitro", "See What's in Shop" with
//! the locked suggestions, a live preview on the right and Cancel / Apply. Sizes from the `profile-editor-*`
//! captures: 604 wide for nameplates and decorations, 663 wide for effects and frames.

use super::{Kind, Mode, Open, text};
use crate::parity::Parity;
use crate::ui::modal::{self, Tone};
use crate::ui::{Root, avatar_source, color, icon};
use dc_state::User;
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, ObjectFit, ScrollHandle, SharedString,
    Window, deferred, div, img, prelude::*, px, rgba, svg,
};

pub(super) struct State {
    pub kind: Kind,
    /// The sku the picker has chosen; `None` is the None tile.
    pub choice: Option<u64>,
    pub initial: Option<u64>,
    pub scroll: ScrollHandle,
}

impl State {
    pub(super) fn new(kind: Kind, current: Option<u64>) -> Self {
        Self {
            kind,
            choice: current,
            initial: current,
            scroll: ScrollHandle::new(),
        }
    }
}

/// One tile of a picker: a collectible item and how it is offered.
#[derive(Clone)]
struct Entry {
    sku: u64,
    art: Vec<String>,
    locked: bool,
    nitro: bool,
}

fn cdn(sku: &str) -> String {
    format!("https://cdn.discordapp.com/media/v1/collectibles-shop/{sku}/static")
}

/// The items of a kind in `products`, one entry per item.
fn entries(
    products: &[super::Product],
    kind: Kind,
    locked: bool,
    nitro: Option<bool>,
) -> Vec<Entry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for product in products {
        if let Some(wants_nitro) = nitro {
            // `premiumType: 0 === r ? null : r` (module 510801): a zero tier is no tier.
            if product.premium_type.is_some_and(|tier| tier != 0) != wants_nitro {
                continue;
            }
        }
        let items = product.items.iter().chain(
            product
                .bundled
                .iter()
                .flat_map(|bundled| bundled.items.iter()),
        );
        for item in items.filter(|item| item.kind == kind.item_kind()) {
            let Ok(sku) = item.sku_id.parse::<u64>() else {
                continue;
            };
            if !seen.insert(sku) {
                continue;
            }
            let mut art = Vec::new();
            match kind {
                Kind::Effect => art.extend(item.thumbnail.clone()),
                Kind::Frame => {
                    art.extend(item.layers.iter().map(|layer| {
                        format!(
                            "https://cdn.discordapp.com/media/v1/collectibles-shop/{}/{layer}/static",
                            item.sku_id
                        )
                    }));
                }
                Kind::Nameplate | Kind::Decoration => art.push(cdn(&item.sku_id)),
            }
            out.push(Entry {
                sku,
                art,
                locked,
                nitro: nitro == Some(true),
            });
        }
    }
    out
}

fn sections(open: &Open, kind: Kind, premium: bool) -> (Vec<Entry>, Vec<Entry>, Vec<Entry>) {
    let owned = entries(&open.owned, kind, false, None);
    let have: std::collections::HashSet<u64> = owned.iter().map(|entry| entry.sku).collect();
    let mut nitro = entries(&open.offers, kind, !premium, Some(true));
    nitro.retain(|entry| !have.contains(&entry.sku));
    let mut shop = entries(&open.offers, kind, true, Some(false));
    shop.retain(|entry| !have.contains(&entry.sku));
    (owned, nitro, shop)
}

fn badge(nitro: bool) -> Div {
    div()
        .absolute()
        .top(px(-6.))
        .right(px(-6.))
        .size(px(20.))
        .rounded_full()
        .bg(rgba(0x000000ff))
        .flex()
        .items_center()
        .justify_center()
        .child(
            svg()
                .path(if nitro {
                    "icons/settings_nitro.svg"
                } else {
                    "icons/channel_lock.svg"
                })
                .size(px(12.))
                .text_color(rgba(0xffffffff)),
        )
}

impl Root {
    fn picker_title(kind: Kind) -> (&'static str, &'static str) {
        match kind {
            Kind::Nameplate => ("Change Nameplate", "Your Nameplates"),
            Kind::Decoration => ("Change Avatar Decoration", "Your Decorations"),
            Kind::Effect => ("Change Profile Effect", "Your Effects"),
            Kind::Frame => ("Change Profile Frame", "Your Frames"),
        }
    }

    /// An image of the shop CDN, or nothing while it loads.
    fn picker_image(&mut self, url: &str) -> Option<std::sync::Arc<gpui::Image>> {
        self.shop_img(url)
    }

    fn picker_tile(
        &mut self,
        kind: Kind,
        entry: &Entry,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sku = entry.sku;
        let width = match kind {
            Kind::Decoration => 75.328,
            _ => 83.328,
        };
        let mut art = Vec::new();
        for url in &entry.art {
            art.extend(self.picker_image(url));
        }
        let wide = kind == Kind::Nameplate;
        let mut tile = div()
            .id(SharedString::from(format!("picker-item-{sku}")))
            .relative()
            .flex_none()
            .w(px(if wide { 224. } else { width }))
            .h(px(if wide { 42. } else { 80. }))
            .rounded(px(if wide { 8. } else { 4. }))
            .when(!wide, |this| this.bg(color("--background-base-lowest")))
            .when(selected, |this| {
                this.border_2().border_color(color("--border-focus"))
            })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(picker) = this
                    .profile_editor
                    .as_mut()
                    .and_then(|open| open.picker.as_mut())
                {
                    picker.choice = Some(sku);
                }
                cx.notify();
            }));
        if matches!(kind, Kind::Effect | Kind::Frame) {
            // Skeleton profile card the art is drawn over.
            tile = tile.child(
                div()
                    .absolute()
                    .left(px(16.))
                    .top(px(20.))
                    .w(px(50.))
                    .h(px(46.))
                    .rounded(px(4.))
                    .bg(color("--background-mod-subtle"))
                    .child(
                        div()
                            .absolute()
                            .left(px(4.))
                            .top(px(12.))
                            .size(px(8.))
                            .rounded_full()
                            .bg(color("--background-mod-strong")),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(4.))
                            .top(px(26.))
                            .w(px(30.))
                            .h(px(3.))
                            .rounded(px(2.))
                            .bg(color("--background-mod-strong")),
                    ),
            );
        }
        for image in art {
            tile = tile.child(
                img(image)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(if wide { 8. } else { 4. }))
                    .object_fit(ObjectFit::Cover),
            );
        }
        if wide {
            tile = tile
                .child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .top(px(5.))
                        .size(px(32.))
                        .rounded_full()
                        .bg(color("--background-mod-strong")),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(52.))
                        .top(px(14.))
                        .w(px(121.))
                        .h(px(14.))
                        .rounded(px(8.))
                        .bg(rgba(0x706f74ff)),
                );
        }
        if entry.locked || entry.nitro {
            tile = tile.child(badge(entry.nitro));
        }
        tile.into_any_element()
    }

    fn picker_control(
        id: &'static str,
        glyph: &'static str,
        label: &'static str,
        selected: bool,
        width: f32,
    ) -> gpui::Stateful<Div> {
        div()
            .id(id)
            .w(px(width))
            .h(px(80.))
            .flex_none()
            .rounded(px(4.))
            .bg(color("--background-base-lowest"))
            .when(selected, |this| {
                this.border_2().border_color(color("--border-focus"))
            })
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .cursor_pointer()
            .child(icon(glyph, 24., "--text-strong"))
            .child(text(label, 12., 16., 400., "--text-default"))
    }

    pub(in crate::ui) fn profile_editor_picker_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let premium = self.profile_editor_premium();
        let (kind, choice, initial, scroll) = {
            let picker = self.profile_editor.as_ref()?.picker.as_ref()?;
            (
                picker.kind,
                picker.choice,
                picker.initial,
                picker.scroll.clone(),
            )
        };
        let (owned, nitro, shop) = sections(self.profile_editor.as_ref()?, kind, premium);
        let (title, own_heading) = Self::picker_title(kind);
        let wide = matches!(kind, Kind::Effect | Kind::Frame);
        let tile_width = if kind == Kind::Decoration {
            75.328
        } else {
            83.328
        };
        let control_width = if kind == Kind::Nameplate {
            106.
        } else {
            tile_width
        };

        let mut first_row = div().flex().flex_wrap().gap(px(12.));
        first_row = first_row
            .child(
                Self::picker_control(
                    "picker-none",
                    "deny_circle",
                    "None",
                    choice.is_none(),
                    control_width,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(picker) = this
                        .profile_editor
                        .as_mut()
                        .and_then(|open| open.picker.as_mut())
                    {
                        picker.choice = None;
                    }
                    cx.notify();
                })),
            )
            .child(
                Self::picker_control("picker-shop", "shop", "Shop", false, control_width).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.profile_editor = None;
                        this.close_settings(cx);
                        this.open_shop(cx);
                    }),
                ),
            );
        for entry in &owned {
            let tile = self.picker_tile(kind, entry, choice == Some(entry.sku), cx);
            first_row = first_row.child(tile);
        }
        let mut list = div().flex().flex_col().gap(px(4.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(text(own_heading, 16., 20., 500., "--text-strong"))
                .child(first_row),
        );
        for (heading, group) in [
            ("Exclusive to Nitro", &nitro),
            ("See What's in Shop", &shop),
        ] {
            if group.is_empty() {
                continue;
            }
            let mut row = div()
                .flex()
                .flex_wrap()
                .gap(px(12.))
                .mt(px(if kind == Kind::Nameplate { 12. } else { 0. }));
            for entry in group {
                let tile = self.picker_tile(kind, entry, choice == Some(entry.sku), cx);
                row = row.child(tile);
            }
            list = list.child(
                div()
                    .mt(px(match kind {
                        Kind::Nameplate => 28.,
                        Kind::Decoration => 12.,
                        _ => 16.,
                    }))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(text(heading, 16., 20., 500., "--text-strong"))
                    .child(row),
            );
        }
        let inventory = div()
            .id("picker-scroll")
            .track_scroll(&scroll)
            .overflow_y_scroll()
            .w(px(match kind {
                Kind::Nameplate => 282.,
                Kind::Decoration => 274.,
                _ => 298.,
            }))
            .flex_none()
            .h_full()
            .ml(px(-4.))
            .mt(px(if wide { -2. } else { -4. }))
            .pl(px(4.))
            .pt(px(4.))
            .child(list);

        let preview = self.picker_preview(kind, choice, window, cx);
        let changed = choice != initial;
        let locked_choice = choice.is_some_and(|sku| {
            [&nitro, &shop]
                .into_iter()
                .flatten()
                .any(|entry| entry.sku == sku && entry.locked)
        });
        let apply = modal::button(
            "picker-apply",
            "Apply",
            if changed {
                Tone::Primary
            } else {
                Tone::PrimaryDisabled
            },
            move |this, _, cx| {
                if locked_choice {
                    // Not owned and not included in the account's Nitro: the Shop sells it.
                    this.profile_editor = None;
                    this.close_settings(cx);
                    this.open_shop(cx);
                } else {
                    this.profile_editor_apply_choice(cx);
                }
            },
        );
        let cancel = modal::button("picker-cancel", "Cancel", Tone::Secondary, |this, _, cx| {
            if let Some(open) = this.profile_editor.as_mut() {
                open.picker = None;
            }
            cx.notify();
        });
        let (width, height) = match kind {
            Kind::Nameplate => (606., 524.),
            Kind::Decoration => (606., 544.),
            _ => (665., 674.),
        };
        let modal = div()
            .parity("profile-editor-picker")
            .id("profile-editor-picker")
            .occlude()
            .w(px(width))
            .h(px(height))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .h(px(40.))
                    .pl(px(24.))
                    .pt(px(16.))
                    .pr(px(16.))
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .id("picker-close")
                            .size(px(24.))
                            .cursor_pointer()
                            .child(icon("modal_close", 24., "--text-strong"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.profile_editor.as_mut() {
                                    open.picker = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .pl(px(24.))
                    .pt(px(8.))
                    .flex()
                    .child(inventory)
                    .child(preview),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(64.))
                    .px(px(24.))
                    .flex()
                    .justify_end()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(100.))
                            .flex()
                            .child(modal::footer_button(cancel, cx)),
                    )
                    .child(
                        div()
                            .w(px(100.))
                            .flex()
                            .child(modal::footer_button(apply, cx)),
                    ),
            );
        Some(
            deferred(
                div()
                    .id("profile-editor-picker-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(modal),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// The right side of a picker: the choice as it would look.
    fn picker_preview(
        &mut self,
        kind: Kind,
        choice: Option<u64>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut user: User = self.profile_editor_user();
        match kind {
            Kind::Nameplate => {
                user.nameplate = choice.map(|sku_id| dc_state::collectibles::Nameplate {
                    sku_id,
                    label: String::new(),
                    palette: String::new(),
                });
                let row = |index: u32| {
                    div()
                        .h(px(42.))
                        .flex_none()
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .size(px(32.))
                                .rounded_full()
                                .bg(color("--background-mod-strong")),
                        )
                        .child(
                            div()
                                .w(px(120.))
                                .h(px(14.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-strong"))
                                .when(index == 99, |this| this),
                        )
                };
                let mine = self.nameplate_preview(&user, 276., choice);
                div()
                    .ml(px(-2.))
                    .mt(px(2.))
                    .w(px(276.))
                    .h(px(190.))
                    .flex_none()
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(row(0))
                    .child(row(1))
                    .child(mine)
                    .child(row(3))
                    .child(row(4))
                    .into_any_element()
            }
            Kind::Decoration => {
                user.avatar_decoration = choice.map(|sku_id| {
                    Box::new(dc_state::collectibles::AvatarDecoration {
                        sku_id,
                        asset: None,
                        expires_at: None,
                    })
                });
                if let Some(decoration) = user.avatar_decoration.as_deref() {
                    self.request_image(
                        format!("avatar-decoration:{}", decoration.sku_id),
                        decoration.static_url(),
                    );
                }
                let avatar = self.images.get(&avatar_source(&user).0).cloned();
                let deco = user.avatar_decoration.as_deref().and_then(|d| {
                    self.images
                        .get(&format!("avatar-decoration:{}", d.sku_id))
                        .cloned()
                });
                let draw = move |size: f32, status: bool| {
                    div()
                        .relative()
                        .size(px(size))
                        .flex_none()
                        .child(
                            div()
                                .size(px(size))
                                .rounded_full()
                                .overflow_hidden()
                                .children(
                                    avatar
                                        .clone()
                                        .map(|image| img(image).size(px(size)).rounded_full()),
                                ),
                        )
                        .children(deco.clone().map(|image| {
                            div()
                                .absolute()
                                .left(px(-size * 0.1))
                                .top(px(-size * 0.1))
                                .child(img(image).size(px(size * 1.2)))
                        }))
                        .when(status, |this| {
                            // Do Not Disturb badge of the small previews.
                            this.child(
                                div()
                                    .absolute()
                                    .right(px(-4.))
                                    .bottom(px(-4.))
                                    .size(px(14.))
                                    .rounded_full()
                                    .bg(color("--status-danger"))
                                    .border_2()
                                    .border_color(color("--background-base-lowest")),
                            )
                        })
                };
                div()
                    .ml(px(7.))
                    .mt(px(27.))
                    .flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .w(px(208.))
                            .h(px(208.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(draw(120., false)),
                    )
                    .child(
                        div()
                            .w(px(56.))
                            .h(px(208.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(16.))
                            .child(draw(40., false))
                            .child(draw(32., false))
                            .child(draw(40., true))
                            .child(draw(32., true)),
                    )
                    .into_any_element()
            }
            Kind::Effect | Kind::Frame => {
                let avatar = self.images.get(&avatar_source(&user).0).cloned();
                let name = user.display_name().to_owned();
                let handle = user.username.clone();
                let effect = (kind == Kind::Effect).then_some(choice).flatten();
                let layer = effect.and_then(|sku| {
                    self.profile_effect_layer(
                        "profile-editor-picker",
                        sku,
                        crate::ui::profile_effects::Surface::profile(300., false),
                        window,
                        cx,
                    )
                });
                div()
                    .relative()
                    .ml(px(15.))
                    .mt(px(6.5))
                    .w(px(300.))
                    .h(px(274.))
                    .flex_none()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-low"))
                    .child(div().h(px(105.)).bg(rgba(0x5c64f4ff)))
                    .child(
                        div()
                            .absolute()
                            .left(px(10.))
                            .top(px(55.))
                            .size(px(92.))
                            .rounded_full()
                            .bg(color("--background-base-low"))
                            .p(px(6.))
                            .child(
                                div()
                                    .size(px(80.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .children(
                                        avatar.map(|image| img(image).size(px(80.)).rounded_full()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(165.))
                            .flex()
                            .flex_col()
                            .child(text(name, 20., 24., 600., "--text-strong"))
                            .child(text(handle, 14., 18., 400., "--text-strong")),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(226.))
                            .w(px(268.))
                            .h(px(32.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(text(
                                "Example Button",
                                14.,
                                18.,
                                500.,
                                "--control-primary-text-default",
                            )),
                    )
                    .children(layer)
                    .into_any_element()
            }
        }
    }

    /// A nameplate row (`nameplatePreview_PaSpLC`): the plate's art behind the avatar and the name (or the
    /// placeholder bar of the panel tile).
    pub(super) fn nameplate_preview(
        &mut self,
        user: &User,
        width: f32,
        _choice: Option<u64>,
    ) -> AnyElement {
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        let art = user
            .nameplate
            .as_ref()
            .and_then(|plate| self.shop_img(&cdn(&plate.sku_id.to_string())));
        let named = width > 250.;
        div()
            .relative()
            .w(px(width))
            .h(px(42.))
            .flex_none()
            .rounded(px(8.))
            .overflow_hidden()
            .children(art.map(|image| {
                img(image)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
            }))
            .child(
                div()
                    .absolute()
                    .left(px(8.))
                    .top(px(5.))
                    .size(px(32.))
                    .rounded_full()
                    .overflow_hidden()
                    .children(avatar.map(|image| img(image).size(px(32.)).rounded_full())),
            )
            .child(if named {
                div().absolute().left(px(52.)).top(px(11.)).child(text(
                    user.display_name().to_owned(),
                    16.,
                    20.,
                    500.,
                    "--text-strong",
                ))
            } else {
                div()
                    .absolute()
                    .left(px(52.))
                    .top(px(14.))
                    .w(px(110.))
                    .h(px(14.))
                    .rounded(px(8.))
                    .bg(rgba(0x706f74ff))
            })
            .into_any_element()
    }

    /// Art of an equipped collectible for the panel tiles: an effect's still or a frame's layers.
    pub(super) fn profile_editor_art(
        &mut self,
        sku: Option<u64>,
        kind: Kind,
        width: f32,
        height: f32,
    ) -> Vec<AnyElement> {
        let Some(sku) = sku else {
            return Vec::new();
        };
        let key = sku;
        let open = self.profile_editor.as_ref();
        let products: Vec<super::Product> = open
            .map(|open| {
                open.owned
                    .iter()
                    .chain(open.offers.iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let found = entries(&products, kind, false, None)
            .into_iter()
            .find(|entry| entry.sku == key);
        let mut out = Vec::new();
        for url in found.map(|entry| entry.art).unwrap_or_default() {
            if let Some(image) = self.shop_img(&url) {
                out.push(
                    img(image)
                        .w(px(width))
                        .h(px(height))
                        .object_fit(ObjectFit::Cover)
                        .into_any_element(),
                );
            }
        }
        out
    }

    /// Writes the picker's choice into the pending changes and closes it. A choice equal to the saved value
    /// clears the pending change, as the store's setters do.
    pub(super) fn profile_editor_apply_choice(&mut self, cx: &mut Context<Self>) {
        let me = self.stores.current_user.clone().unwrap_or_default();
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        let Some(picker) = open.picker.take() else {
            return;
        };
        let choice = picker.choice;
        match picker.kind {
            Kind::Nameplate => {
                let saved = me.nameplate.as_ref().map(|plate| plate.sku_id);
                open.pending.nameplate = (choice != saved).then_some(choice);
            }
            Kind::Decoration => {
                let saved = me.avatar_decoration.as_deref().map(|d| d.sku_id);
                open.pending.avatar_decoration = (choice != saved).then_some(choice);
            }
            Kind::Effect => {
                open.pending.profile_effect =
                    (choice != open.saved.equipped.profile_effect).then_some(choice);
            }
            Kind::Frame => {
                open.pending.profile_frame =
                    (choice != open.saved.equipped.profile_frame).then_some(choice);
            }
        }
        let _ = Mode::Main;
        cx.notify();
    }
}
