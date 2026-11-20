//! The collectible pickers of the editor (modules 478733 nameplate, 50976 decoration, 720433 effect, 794561 frame):
//! "Your X" with the None and Shop tiles and the owned items, "Exclusive to Nitro", "See What's in Shop" with
//! the locked suggestions, a live preview on the right and Cancel / Apply. Sizes from the `profile-editor-*`
//! captures: 604 wide for nameplates and decorations, 663 wide for effects and frames.

use super::{Kind, Mode, Open, text};
use crate::parity::Parity;
use crate::ui::modal::{self, Tone};
use crate::ui::motion::Posed as _;
use crate::ui::{Root, color, icon};
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
    /// The item's `label`, the tile's accessible name.
    label: String,
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
                label: item.label.clone(),
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

/// Harness only: the first "Exclusive to Nitro" item of a picker.
#[cfg(feature = "capture")]
pub(super) fn first_nitro_sku(open: &Open, kind: Kind, premium: bool) -> Option<u64> {
    sections(open, kind, premium)
        .1
        .first()
        .map(|entry| entry.sku)
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

/// `presetEffectBackground_bYuL-p` of the capture: the profile mock behind an effect's or a frame's tile art.
const PRESET_BACKGROUND: &str = "https://cdn.discordapp.com/assets/content/a859611882903f1102a796c08c68278917821af3e87eb9191df74bf78426971d.png";

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
        // A frame tile holds module 665411's preview (`role: img`, "Profile Frame Preview: {label}"); a nameplate
        // tile is named after the plate (`s.label ?? "Nameplate"`, module 478733) over module 872200's preview.
        let label: Option<String> = match kind {
            Kind::Frame => Some(crate::ui::shop::cards::frame_preview_label(&entry.label)),
            Kind::Nameplate => Some(if entry.label.is_empty() {
                dc_intl::t("x5CoXR").to_owned()
            } else {
                entry.label.clone()
            }),
            _ => None,
        };
        let mut tile = div()
            .id(SharedString::from(format!("picker-item-{sku}")))
            .when_some(label, |tile, label| tile.aria_label(label))
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
        // `presetEffectBackground_bYuL-p`: the profile mock the effect or frame art is drawn over.
        let mock = matches!(kind, Kind::Effect | Kind::Frame)
            .then(|| self.picker_image(PRESET_BACKGROUND))
            .flatten();
        if let Some(mock) = mock {
            tile = tile.child(
                img(mock)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(4.))
                    .object_fit(ObjectFit::Cover),
            );
        } else if matches!(kind, Kind::Effect | Kind::Frame) {
            // Skeleton profile card the art is drawn over while the mock loads.
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
        let locked_entry = choice.and_then(|sku| {
            [&nitro, &shop]
                .into_iter()
                .flatten()
                .find(|entry| entry.sku == sku && entry.locked)
                .cloned()
        });
        let locked_choice = locked_entry.is_some();
        let locked_nitro = locked_entry.is_some_and(|entry| entry.nitro);
        let apply_label = if locked_nitro {
            "Get Nitro"
        } else if locked_choice {
            "Go to Shop"
        } else {
            "Apply"
        };
        let apply = modal::button(
            "picker-apply",
            apply_label,
            if changed || locked_choice {
                Tone::Primary
            } else {
                Tone::PrimaryDisabled
            },
            move |this, _, cx| {
                if locked_nitro {
                    this.open_purchase(false, cx);
                } else if locked_choice {
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
        // 50976 and its siblings: without `canUseCollectibles`, a Nitro item puts the upsell of 571827 where Cancel is.
        let secondary = if locked_nitro {
            self.picker_nitro_upsell(kind, cx)
        } else {
            Some(
                div()
                    .w(px(100.))
                    .flex()
                    .child(modal::footer_button(cancel, cx))
                    .into_any_element(),
            )
        };
        let item_upsell = self
            .profile_editor
            .as_ref()
            .and_then(|open| open.item_upsell)
            .and_then(|kind| self.picker_item_upsell(kind, cx));
        let (width, height) = match kind {
            Kind::Nameplate => (606., 524.),
            Kind::Decoration => (606., 544.),
            _ => (665., 674.),
        };
        let modal = div()
            .parity("profile-editor-picker")
            .id("profile-editor-picker")
            .occlude()
            .on_click(|_, _, cx| cx.stop_propagation())
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
                    .children(secondary)
                    .child(
                        div()
                            .w(px(100.))
                            .flex()
                            .child(modal::footer_button(apply, cx)),
                    ),
            );
        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).
        let pose = crate::ui::motion::legacy_modal_pose(
            "profile-editor-picker",
            "profile-editor-picker-layer",
            false,
        );
        Some(
            deferred(
                div()
                    .id("profile-editor-picker-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.picker = None;
                        }
                        cx.notify();
                    }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(modal.posed(pose))
                    .children(item_upsell),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 571827: "Access exclusive Avatar Decorations with Nitro" (`aFR/EI`, Profile Effects `eWNONM`,
    /// Nameplates `G6b8TF`) in `body-md/medium`, after the 24px Nitro wheel in the tier 2 colour, 6px apart
    /// (`nitroUpsellContainer_zj5gK9`, pushed to the start of the footer). Decorations and effects leave the wheel out
    /// in the `2026-10-shop-sku-nitro-upsell-refresh` experiment; frames have no upsell. Its "Nitro" opens 393027.
    fn picker_nitro_upsell(&self, kind: Kind, cx: &mut Context<Self>) -> Option<AnyElement> {
        let key = match kind {
            Kind::Decoration => "aFR/EI",
            Kind::Effect => "eWNONM",
            Kind::Nameplate => "G6b8TF",
            Kind::Frame => return None,
        };
        let refresh = matches!(kind, Kind::Decoration | Kind::Effect)
            && self
                .stores
                .experiments
                .variant("2026-10-shop-sku-nitro-upsell-refresh")
                == Some(1);
        let runs = dc_intl::format_runs(key, &[]);
        let lead: String = runs
            .iter()
            .filter(|(_, tag)| tag.as_deref() != Some("$link"))
            .map(|(text, _)| text.as_str())
            .collect();
        let link: String = runs
            .iter()
            .filter(|(_, tag)| tag.as_deref() == Some("$link"))
            .map(|(text, _)| text.as_str())
            .collect();
        Some(
            div()
                .mr_auto()
                .flex()
                .items_center()
                .when(!refresh, |this| {
                    this.child(
                        div().mr(px(6.)).child(
                            img(crate::ui::go_live::nitro_ribbon_icon())
                                .flex_none()
                                .size(px(24.)),
                        ),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(lead)
                        .child(
                            div()
                                .id("picker-nitro-upsell-link")
                                .cursor_pointer()
                                .text_color(color("--text-link"))
                                .hover(|style| style.underline())
                                .child(link)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(open) = this.profile_editor.as_mut() {
                                        open.item_upsell = Some(kind);
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Module 393027 on the premium upsell dialog (831318) for a picker's kind: the kind's title and body, its
    /// graphic (a looping video, not drawn), View all Nitro perks closing the picker and opening the Nitro page.
    fn picker_item_upsell(&self, kind: Kind, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (title, body) = match kind {
            Kind::Decoration => ("JI7uhc", "5XvsdZ"),
            Kind::Effect => ("sYm15X", "Hza+Bu"),
            Kind::Nameplate => ("k7UN4k", "0/QPxO"),
            Kind::Frame => return None,
        };
        fn close(this: &mut Root) {
            if let Some(open) = this.profile_editor.as_mut() {
                open.item_upsell = None;
            }
        }
        Some(self.premium_upsell(
            crate::ui::premium_upsell::Spec {
                id: "picker-item-upsell",
                ids: [
                    "picker-item-upsell-close",
                    "picker-item-upsell-perks",
                    "picker-item-upsell-get",
                ],
                // GAP: 49792's graphic is a looping video (a still picture with reduced motion); neither is drawn.
                header: crate::ui::premium_upsell::Header::Video,
                title: dc_intl::t(title).into(),
                subtitle: dc_intl::t(body).into(),
                secondary: dc_intl::t("PcTCB7").into(),
                on_secondary: Box::new(|this, _, cx| {
                    close(this);
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.picker = None;
                    }
                    this.open_nitro(cx);
                }),
                on_primary: Box::new(|this, _, cx| {
                    close(this);
                    this.open_purchase(false, cx);
                }),
                close: Box::new(|this, _, _| close(this)),
                priority: 4,
            },
            cx,
        ))
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
                let mine = self.nameplate_preview(&user, 276., false);
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
                let avatar = self.profile_editor_avatar_image(&user);
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
                let avatar = self.profile_editor_avatar_image(&user);
                let name = user.display_name().to_owned();
                let handle = user.username.clone();
                let effect = (kind == Kind::Effect).then_some(choice).flatten();
                let frame = (kind == Kind::Frame).then_some(choice).flatten();
                let layer = effect.and_then(|sku| {
                    self.profile_effect_layer(
                        "profile-editor-picker",
                        sku,
                        crate::ui::profile_effects::Surface::profile(300., false),
                        window,
                        cx,
                    )
                });
                let frame_layer = self.profile_editor_art(frame, Kind::Frame, 300., 274.);
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
                    .children(frame_layer)
                    .children(layer)
                    .into_any_element()
            }
        }
    }

    /// A nameplate row (`nameplatePreview_PaSpLC`): the plate's art behind the avatar and the name (or the
    /// placeholder bar of the panel tile). `placeholder` draws the placeholder user of module 887988
    /// (`showPlaceholderUser` while the tile is not hovered): the gray default avatar instead of the account's.
    pub(super) fn nameplate_preview(
        &mut self,
        user: &User,
        width: f32,
        placeholder: bool,
    ) -> AnyElement {
        let avatar = (!placeholder)
            .then(|| self.profile_editor_avatar_image(user))
            .flatten();
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
                    .when(placeholder, |this| {
                        // Measured in the capture's golden: the gray avatar and its lighter mark.
                        this.bg(rgba(0x717177ff))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui::svg()
                                    .path("icons/sb_discord.svg")
                                    .size(px(20.))
                                    .text_color(rgba(0xb1b1b4ff)),
                            )
                    })
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
