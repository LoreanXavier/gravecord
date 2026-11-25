//! Tooltips. Box from `tooltip_ZDdyhE`, caret from module 160733, trigger from 402883, spring from 280336, guild body from 396655.

use super::motion::{Drive, Presence, presets};
use super::{Root, SANS, badge_paths as paths, color, icon, kerning};
use dc_state::{Badge, Guild, Id};
use gpui::{
    AnyElement, App, Bounds, BoxShadow, Context, Display, Element, ElementId, FontWeight,
    GlobalElementId, Image, ImageFormat, InspectorElementId, IntoElement, LayoutId, Pixels,
    Position, Radians, Rgba, SharedString, Stateful, Style, Transformation, Window, WindowId,
    canvas, deferred, div, hsla, img, point, prelude::*, px, svg,
};
use std::cell::RefCell;
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

/// `spacing` default in module 160733.
pub(super) const SPACING: f32 = 11.;
/// `viewportPadding` default in module 508382.
const VIEWPORT_PADDING: f32 = 8.;
const MAX_WIDTH: f32 = 200.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Side {
    Top,
    Bottom,
    /// In the official set; nothing built so far opens to the left.
    #[allow(dead_code)]
    Left,
    Right,
}

#[derive(Clone)]
pub(super) enum Body {
    Text(SharedString),
    /// Plain muted text (`body-sm/normal`, `text-muted`): the voters of a poll answer (module 119591 `L`).
    Muted(SharedString),
    /// The rail's text, in the guild tooltip's `body-md/semibold`.
    Heading(SharedString),
    Guild(Id),
    /// `richTooltipContent`: a medium title over muted text, both `body-sm`.
    Rich(SharedString, SharedString),
    /// A direct message on the server rail (module 396655 `tN`): its name, then who is in its call, streaming or in
    /// an Activity, and whether it is muted.
    DirectMessage(Id),
    /// Module 683063 `u` with a server's 24px icon as its asset and no body (`richTooltipContent_xlDuA-`: 8px by
    /// 12px inside, the icon 8px before a `body-sm/medium` title, up to 272px wide): the voice invite card's channel.
    GuildTitle {
        guild: Id,
        title: SharedString,
    },
    /// A quick reaction of the message hover bar (`np` in module 891522): `:name:` in `body-sm/medium` `text-strong`
    /// over the hint in `body-xs/normal` `text-default`, both centred (`emojiTooltipText_V_VY9Q`).
    Emoji(SharedString, SharedString),
    /// A profile badge (`ed.A` in module 328464): the art over the eyebrow, the title in the display face and a line
    /// of text, centred in a 200px card. `image` is the key of the art in `Root::images`.
    Badge {
        image: Option<String>,
        eyebrow: Option<SharedString>,
        title: SharedString,
        body: Option<SharedString>,
    },
    /// `tooltipWithShortcut_ZDdyhE` (module 866665, `909091.*.css`): `body-sm/medium` text centred 4px over a shortcut
    /// combo (`475358`).
    Shortcut(SharedString, &'static str),
    /// A sticker's name behind `StickerSmallIcon` (module 679636 `rH`): the icon is 16px with a 2px margin, 4px
    /// from the text (`stickerIcon_y33UtT`, `stickerName_y33UtT`).
    Sticker(SharedString),
    /// The popout over a reaction pill (module 30492 `eT`): the emoji, then who reacted, from the reactors fetched
    /// under `fetch_key`. It has no caret and nothing to show until a name is known.
    Reaction {
        emoji: String,
        fetch_key: String,
        label: String,
        count: usize,
        /// A super reaction's pill: its sentences, and the line under them.
        burst: Option<super::reaction_tip::BurstPrompt>,
    },
    /// Green confirmation tooltip (`tooltipGreen_3_v8qp`, modules 664343 & 206845): `--green-360` background and white text.
    Green(SharedString),
}

/// The line under a super reaction's popout (`burstReactionTooltipPrompt` and `burstReactionTooltipSpacer` of
/// module 912842: 12px above, a `--border-subtle` rule and 8px; the Nitro mark is 24px with 5px after it, and the
/// text keeps `reactionTooltipText`'s 12px start). The upsell's "Unlock with Nitro" button is not drawn.
fn burst_prompt(prompt: super::reaction_tip::BurstPrompt) -> AnyElement {
    use super::reaction_tip::BurstPrompt;
    let (mark, text, indented) = match prompt {
        BurstPrompt::Sent { premium } => (premium, dc_intl::t!("ZbNJXn"), true),
        BurstPrompt::Add => (true, dc_intl::t!("kVfuVu"), true),
        BurstPrompt::Upsell => (false, dc_intl::t!("W1bMkq"), false),
    };
    div()
        .w_full()
        .mt(px(12.))
        .pt(px(8.))
        .border_t_1()
        .border_color(color("--border-subtle"))
        .flex()
        .items_center()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(400.))
        .when(mark, |row| {
            row.child(div().mr(px(5.)).flex_none().child(icon(
                "nitro_wheel",
                24.,
                "--text-default",
            )))
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .when(indented, |text| text.ml(px(12.)))
                .child(text),
        )
        .into_any_element()
}

#[derive(Clone)]
pub(super) struct Tip {
    pub body: Body,
    pub side: Side,
    pub spacing: f32,
    /// How long the pointer rests on the target before the tooltip opens, in milliseconds (`delay` in module 280336).
    pub delay: u64,
    /// Optional shake offset in px (`Shaker`, module 568602, used by module 206845 at tiers 10-11).
    pub shake: (f32, f32),
}

impl Tip {
    pub fn text(text: &'static str, side: Side) -> Self {
        Self {
            body: Body::Text(text.into()),
            side,
            spacing: SPACING,
            delay: 0,
            shake: (0., 0.),
        }
    }

    pub fn body(body: Body, side: Side) -> Self {
        Self {
            body,
            side,
            spacing: SPACING,
            delay: 0,
            shake: (0., 0.),
        }
    }

    pub fn shortcut(text: impl Into<SharedString>, bind: &'static str, side: Side) -> Self {
        Self::body(Body::Shortcut(text.into(), bind), side)
    }

    /// The same tip, opening after the pointer has rested for `milliseconds`.
    pub fn after(mut self, milliseconds: u64) -> Self {
        self.delay = milliseconds;
        self
    }

    /// The same tip, `spacing` pixels from its target.
    pub fn spaced(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// The same tip, offset by `shake` pixels (`568602.b`).
    pub fn shaken(mut self, shake: (f32, f32)) -> Self {
        self.shake = shake;
        self
    }

    /// The server rail keeps its tooltips 12px from the icon.
    pub fn rail(body: Body) -> Self {
        Self {
            body,
            side: Side::Right,
            spacing: 12.,
            delay: 0,
            shake: (0., 0.),
        }
    }
}

#[derive(Clone)]
pub(super) struct Active {
    key: SharedString,
    tip: Tip,
    anchor: Bounds<Pixels>,
    /// The window whose element it belongs to (`anchor` is in that window's coordinates); `None` is the main window.
    window: Option<WindowId>,
}

impl Active {
    /// A tooltip placed by its owner rather than by hover, like a slider's value bubble, in the main window.
    pub fn new(key: SharedString, tip: Tip, anchor: Bounds<Pixels>) -> Self {
        Self {
            key,
            tip,
            anchor,
            window: None,
        }
    }

    /// The same tooltip, in the window `window` (the one its element was drawn in).
    pub fn in_window(mut self, window: WindowId) -> Self {
        self.window = Some(window);
        self
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn anchor(&self) -> Bounds<Pixels> {
        self.anchor
    }
}

/// Whether a window's tooltip layer draws a tooltip that belongs to the window `tip`. `layer` is the window drawing
/// (`None` for the main window) and `popout` the popped-out call window, if open: that window draws only its own
/// tooltips, and the main window every other one, so a tooltip shows in the window that was hovered and nowhere else.
/// (A tooltip of the popped-out window is dropped when the window closes.)
pub(super) fn drawn_in(
    tip: Option<WindowId>,
    layer: Option<WindowId>,
    popout: Option<WindowId>,
) -> bool {
    let popped = popout.is_some() && tip == popout;
    if layer.is_some() && layer == popout {
        popped
    } else {
        !popped
    }
}

/// Where the box goes around its target, pushed along the target's edge to stay in the viewport.
fn place(
    anchor: Bounds<Pixels>,
    body: gpui::Size<Pixels>,
    side: Side,
    spacing: Pixels,
    viewport: gpui::Size<Pixels>,
) -> gpui::Point<Pixels> {
    let center = anchor.center();
    let origin = match side {
        Side::Top => point(
            center.x - body.width / 2.,
            anchor.top() - spacing - body.height,
        ),
        Side::Bottom => point(center.x - body.width / 2., anchor.bottom() + spacing),
        Side::Left => point(
            anchor.left() - spacing - body.width,
            center.y - body.height / 2.,
        ),
        Side::Right => point(anchor.right() + spacing, center.y - body.height / 2.),
    };
    let padding = px(VIEWPORT_PADDING);
    let clamp = |value: Pixels, length: Pixels, limit: Pixels| {
        value.min(limit - padding - length).max(padding)
    };
    match side {
        Side::Top | Side::Bottom => point(clamp(origin.x, body.width, viewport.width), origin.y),
        Side::Left | Side::Right => point(origin.x, clamp(origin.y, body.height, viewport.height)),
    }
}

/// Top left of the 16x10 caret, before rotation. Its 16px box sits 5px over the edge and it rotates about that box's center.
fn caret_origin(anchor: Bounds<Pixels>, body: Bounds<Pixels>, side: Side) -> gpui::Point<Pixels> {
    let target = anchor.center();
    // Limits from `A` in module 160733.
    let reach_x = body.size.width / 2. - px(8.);
    let reach_y = body.size.height / 2. - px(8.);
    let center = body.center();
    let along_x = (target.x - center.x).max(-reach_x).min(reach_x);
    let along_y = (target.y - center.y).max(-reach_y).min(reach_y);
    let box_center = match side {
        // The box is above its target, so the caret hangs under the box.
        Side::Top => point(center.x + along_x, body.bottom() + px(3.)),
        Side::Bottom => point(center.x + along_x, body.top() - px(3.)),
        Side::Left => point(body.right() + px(3.), center.y + along_y + px(1.)),
        Side::Right => point(body.left() - px(3.), center.y + along_y + px(1.)),
    };
    point(box_center.x - px(8.), box_center.y - px(5.))
}

struct Placed {
    anchor: Bounds<Pixels>,
    side: Side,
    spacing: Pixels,
    body: AnyElement,
    caret: AnyElement,
    /// The scale the tooltip draws at (`.95` to 1 with its opacity, module 280336), about its body's centre.
    scale: f32,
    /// Optional shake offset (`568602.b`).
    shake: (f32, f32),
}

impl IntoElement for Placed {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Placed {
    type RequestLayoutState = (LayoutId, LayoutId);
    type PrepaintState = gpui::Point<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let body = self.body.request_layout(window, cx);
        let caret = self.caret.request_layout(window, cx);
        let style = Style {
            position: Position::Absolute,
            display: Display::Flex,
            ..Style::default()
        };
        let layout = window.request_layout(style, [body, caret], cx);
        (layout, (body, caret))
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (body, caret): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let laid_body = window.layout_bounds(*body);
        let laid_caret = window.layout_bounds(*caret);
        let origin = place(
            self.anchor,
            laid_body.size,
            self.side,
            self.spacing,
            window.viewport_size(),
        );
        let shake = point(px(self.shake.0), px(self.shake.1));
        let origin = point(origin.x + shake.x, origin.y + shake.y);
        let placed = Bounds::new(origin, laid_body.size);
        let caret_at = caret_origin(self.anchor, placed, self.side);
        window.with_element_offset(origin - laid_body.origin, |window| {
            self.body.prepaint(window, cx);
        });
        window.with_element_offset(caret_at - laid_caret.origin, |window| {
            self.caret.prepaint(window, cx);
        });
        placed.center()
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        centre: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_paint_scale(self.scale, *centre, |window| {
            self.body.paint(window, cx);
            self.caret.paint(window, cx);
        });
    }
}

/// Not in the pinned client: a tooltip over `anchor` whose content its caller draws (BetterActivities' activity card,
/// `plugin_hooks/better_activities.rs`), in the tooltip's box and caret, opening at once without the spring.
pub(super) fn placed_content(anchor: Bounds<Pixels>, content: AnyElement) -> AnyElement {
    deferred(Placed {
        anchor,
        side: Side::Top,
        spacing: px(SPACING),
        body: frame().child(content).into_any_element(),
        caret: caret(Side::Top).into_any_element(),
        scale: 1.,
        shake: (0., 0.),
    })
    .with_priority(10)
    .into_any_element()
}

fn caret(side: Side) -> impl IntoElement {
    caret_colored(side, "--background-surface-high", Some("--border-subtle"))
}

fn caret_colored(side: Side, fill: &'static str, stroke: Option<&'static str>) -> impl IntoElement {
    // The drawing points down; turn it to point at the target.
    let turn = match side {
        Side::Top => 0.,
        Side::Bottom => PI,
        Side::Left => -FRAC_PI_2,
        Side::Right => FRAC_PI_2,
    };
    let layer = |path: &'static str, tint: &str| {
        svg()
            .absolute()
            .w(px(16.))
            .h(px(10.))
            .path(path)
            .text_color(color(tint))
            .with_transformation(Transformation::rotate(Radians(turn)))
    };
    div()
        .relative()
        .flex_none()
        .w(px(16.))
        .h(px(10.))
        .child(layer("icons/tooltip_caret_fill.svg", fill))
        .children(stroke.map(|s| layer("icons/tooltip_caret_stroke.svg", s)))
}

fn frame() -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .max_w(px(MAX_WIDTH))
        // A 1px inset shadow in the original, so the border comes out of the padding.
        .px(px(11.))
        .py(px(7.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-high"))
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.24),
            offset: point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        }])
        .font_family(SANS)
        .font_features(kerning())
        .text_color(color("--text-default"))
}

fn hex(token: &str) -> String {
    let rgba: Rgba = color(token);
    let byte = |value: f32| (value * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(rgba.r),
        byte(rgba.g),
        byte(rgba.b)
    )
}

fn paths_svg(paths: &[&str], fill: &str) -> String {
    paths
        .iter()
        .map(|path| format!(r#"<path fill="{fill}" fill-rule="evenodd" d="{path}"/>"#))
        .collect()
}

/// The table `V` in module 554783, dark theme: icon, foreground, background, size adjustment.
fn badge_svg(badge: Badge, premium: bool, side: u32) -> Option<String> {
    let open = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}" viewBox="0 0 {side} {side}">"#
    );
    if badge == Badge::Discoverable {
        let scale = side as f32 / 24.;
        return Some(format!(
            r#"{open}<g transform="scale({scale})"><path fill="{}" d="{}"/>{}</g></svg>"#,
            hex("--badge-inverted-background"),
            paths::WEB_BACKGROUND,
            paths_svg(paths::WEB_FOREGROUND, &hex("--badge-inverted-text")),
        ));
    }
    let white = "#ffffff".to_owned();
    let ((view_box, icon), foreground, background, adjust) = match badge {
        Badge::Staff => (paths::STAFF, white, hex("--green-360"), 0),
        Badge::Verified | Badge::VerifiedAndPartnered => {
            (paths::CHECK, white, hex("--green-360"), 0)
        }
        Badge::Partnered => (paths::PARTNER, white, hex("--brand-500"), 0),
        Badge::Community if premium => (paths::HOME, white, "url(#premium)".to_owned(), 2),
        Badge::Community => (paths::HOME, hex("--primary-500"), white, 2),
        Badge::Discoverable | Badge::None => return None,
    };
    let inner = (side as f32 * 0.75).floor() as u32 - adjust;
    let inset = (side as f32 - inner as f32) / 2.;
    let scale = side as f32 / 16.;
    Some(format!(
        r#"{open}<defs><linearGradient id="premium" gradientTransform="rotate(45)"><stop offset="0" stop-color="{}"/><stop offset="1" stop-color="{}"/></linearGradient></defs><g transform="scale({scale})"><path fill="{background}" fill-rule="evenodd" d="{}"/></g><svg x="{inset}" y="{inset}" width="{inner}" height="{inner}" viewBox="{view_box}">{}</svg></svg>"#,
        hex("--premium-tier-2-purple"),
        hex("--premium-tier-2-pink"),
        paths::FLOWER,
        paths_svg(icon, &foreground),
    ))
}

pub(super) fn badge_image(guild: &Guild) -> Option<Arc<Image>> {
    badge_image_sized(guild, 18)
}

/// The guild badge at `side` pixels (module 714991 `tr.A` takes a size).
pub(super) fn badge_image_sized(guild: &Guild, side: u32) -> Option<Arc<Image>> {
    let svg = badge_svg(guild.badge(), guild.premium(), side)?;
    Some(Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        svg.into_bytes(),
    )))
}

impl Root {
    /// Records the bounds of its parent (it fills it) under `key` in `tip_anchors`, for popouts that open beside it.
    pub(super) fn anchor_probe(&self, key: impl Into<SharedString>) -> impl IntoElement {
        let anchors = self.tip_anchors.clone();
        let key: SharedString = key.into();
        canvas(
            move |bounds, _, _| {
                anchors.borrow_mut().insert(key.clone(), bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Puts a tooltip on `target`: it opens on hover and closes on leave or any press, as module 402883 does.
    pub(super) fn with_tip(
        &self,
        key: impl Into<SharedString>,
        tip: Tip,
        target: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let key: SharedString = key.into();
        let anchors = self.tip_anchors.clone();
        let (stored, hovered) = (key.clone(), key.clone());
        div()
            .id(ElementId::Name(format!("tip-{key}").into()))
            .relative()
            .flex_none()
            .child(target)
            .child(
                canvas(
                    move |bounds, _, _| {
                        anchors.borrow_mut().insert(stored.clone(), bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .on_hover(cx.listener(move |this, inside: &bool, window, cx| {
                let window = window.window_handle().window_id();
                let menu_open = this.menu.is_some()
                    || this.context.open.is_some()
                    || this.rail_folders.menu.is_some();
                if *inside && !menu_open && tip.delay > 0 {
                    // The tooltip opens once the pointer has stayed; leaving or pressing first cancels it.
                    this.tip_pending = Some(hovered.clone());
                    let (key, tip, wait) = (hovered.clone(), tip.clone(), tip.delay);
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(wait))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            if this.tip_pending.as_ref() != Some(&key)
                                || this.menu.is_some()
                                || this.context.open.is_some()
                                || this.rail_folders.menu.is_some()
                            {
                                this.tip_pending = None;
                                return;
                            }
                            this.tip_pending = None;
                            let anchor = this.tip_anchors.borrow().get(&key).copied();
                            this.tip = anchor.map(|anchor| {
                                Active::new(key.clone(), tip.clone(), anchor).in_window(window)
                            });
                            cx.notify();
                        });
                    })
                    .detach();
                } else if *inside && !menu_open {
                    let anchor = this.tip_anchors.borrow().get(&hovered).copied();
                    this.tip = anchor.map(|anchor| {
                        Active::new(hovered.clone(), tip.clone(), anchor).in_window(window)
                    });
                } else {
                    if this.tip_pending.as_ref() == Some(&hovered) {
                        this.tip_pending = None;
                    }
                    if this.tip.as_ref().is_some_and(|tip| tip.key == hovered) {
                        this.tip = None;
                    }
                }
                cx.notify();
            }))
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.tip = None;
                this.tip_pending = None;
                cx.notify();
            }))
    }

    fn tip_body(&self, body: &Body) -> Option<AnyElement> {
        Some(match body {
            Body::Text(text) => frame()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_center()
                .child(div().max_w(px(MAX_WIDTH - 24.)).child(text.clone()))
                .into_any_element(),
            Body::Green(text) => frame()
                .bg(color("--green-360"))
                .border_color(color("--green-360"))
                .text_color(color("--white"))
                .text_size(px(14.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_center()
                .child(div().max_w(px(MAX_WIDTH - 24.)).child(text.clone()))
                .into_any_element(),
            Body::Muted(text) => frame()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(div().max_w(px(MAX_WIDTH - 24.)).child(text.clone()))
                .into_any_element(),
            Body::Heading(text) => frame()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(600.))
                .child(div().max_w(px(MAX_WIDTH - 24.)).child(text.clone()))
                .into_any_element(),
            // Module 683063 `u` without an asset: `richTooltipContent_xlDuA-` (up to 272px, 8px by 12px inside) holds
            // `textContent_xlDuA-` (a column, centred under `noAsset_xlDuA-`): the `body-sm/medium` title over the
            // `body-sm/normal` body in `text-subtle`.
            Body::Rich(title, body) => frame()
                .max_w(px(272.))
                .flex_col()
                .text_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .child(
                    div()
                        .max_w(px(272. - 24.))
                        .font_weight(FontWeight(500.))
                        .child(title.clone()),
                )
                .child(
                    div()
                        .max_w(px(272. - 24.))
                        .text_color(color("--text-subtle"))
                        .child(body.clone()),
                )
                .into_any_element(),
            Body::DirectMessage(channel) => frame()
                .child(self.dm_tooltip_body(*channel))
                .into_any_element(),
            Body::GuildTitle { guild, title } => {
                let picture = self.images.get(&format!("guild:{guild}")).cloned();
                let acronym = self
                    .stores
                    .guild(*guild)
                    .map(|known| Guild::acronym_of(&known.name))
                    .unwrap_or_default();
                frame()
                    .max_w(px(272.))
                    .gap(px(8.))
                    .child(
                        // `guildTooltipIcon_OC_CcB`: the icon with `--radius-sm` corners.
                        div()
                            .size(px(24.))
                            .flex_none()
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .font_weight(FontWeight(500.))
                            .when_some(picture.clone(), |this, picture| {
                                this.child(img(picture).size(px(24.)))
                            })
                            .when(picture.is_none(), |this| this.child(acronym)),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .child(title.clone()),
                    )
                    .into_any_element()
            }
            Body::Emoji(name, hint) => frame()
                .flex_col()
                .text_center()
                .child(
                    div()
                        .max_w(px(MAX_WIDTH - 24.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(name.clone()),
                )
                .child(
                    div()
                        .max_w(px(MAX_WIDTH - 24.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-default"))
                        .child(hint.clone()),
                )
                .into_any_element(),
            Body::Badge {
                image,
                eyebrow,
                title,
                body,
            } => {
                let art = image.as_ref().and_then(|key| self.images.get(key)).cloned();
                // `content_2dzrtJ`: 12px at the sides, 16 above the art and below the text; the 1px ring is the
                // card's border here, so each is one less.
                div()
                    .flex()
                    .flex_none()
                    .flex_col()
                    .items_center()
                    .w(px(200.))
                    .px(px(11.))
                    .pt(px(15.))
                    .pb(px(15.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.24),
                        offset: point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .font_family(SANS)
                    .font_features(kerning())
                    .text_color(color("--text-default"))
                    // `graphic_2dzrtJ`: 64 high at the catalog's 25:16 shape.
                    .child(
                        div()
                            .h(px(64.))
                            .w(px(99.84))
                            .flex_none()
                            .children(art.map(|art| img(art).w(px(99.84)).h(px(64.)))),
                    )
                    // `titleGroup_2dzrtJ`: 16 below the art, 4 between lines.
                    .child(
                        div()
                            .mt(px(16.))
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(4.))
                            .text_center()
                            .children(eyebrow.clone().map(|eyebrow| {
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-subtle"))
                                    .child(eyebrow)
                            }))
                            .child(
                                div()
                                    .font_family(super::nitro::NITRO_FONT)
                                    .italic()
                                    .text_size(px(16.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(900.))
                                    .text_color(color("--text-strong"))
                                    .child(title.to_uppercase()),
                            )
                            .children(body.clone().map(|body| {
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-subtle"))
                                    .child(body)
                            })),
                    )
                    .into_any_element()
            }
            Body::Shortcut(text, bind) => frame()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .text_center()
                        .child(
                            div()
                                .max_w(px(MAX_WIDTH - 24.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .child(text.clone()),
                        )
                        .child(super::keyboard::help::combo(bind)),
                )
                .into_any_element(),
            Body::Sticker(name) => frame()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .child(
                    div()
                        .max_w(px(MAX_WIDTH - 24.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .mr(px(2.))
                                .child(icon("sticker_small", 16., "--text-default")),
                        )
                        .child(name.clone()),
                )
                .into_any_element(),
            Body::Reaction {
                emoji,
                fetch_key,
                label,
                count,
                burst,
            } => {
                let names = super::reaction_tip::names(self.fetched.get(fetch_key)?);
                let text = super::reaction_tip::text(label, &names, *count, burst.is_some())?;
                let image = self.images.get(emoji).cloned();
                // `reactionTooltipInner_5l5ajC` (or `burstReactionTooltipMessage_5l5ajC`): the emoji and the sentence.
                let message = div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .children(image.map(|image| gpui::img(image).size(px(32.)))),
                    )
                    .child(
                        div()
                            .ml(px(12.))
                            .min_w(px(0.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(400.))
                            .child(text),
                    );
                // `reactionTooltip_5l5ajC`.
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .max_w(px(288.))
                    .p(px(16.))
                    .overflow_hidden()
                    .rounded(px(4.))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![
                        BoxShadow {
                            color: hsla(0., 0., 1., 0.08),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(0.),
                            spread_radius: px(1.),
                            inset: false,
                        },
                        BoxShadow {
                            color: hsla(0., 0., 0., 0.24),
                            offset: point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        },
                    ])
                    .font_family(SANS)
                    .font_features(kerning())
                    .text_color(color("--text-default"))
                    .child(message)
                    .children(burst.map(burst_prompt))
                    .into_any_element()
            }
            Body::Guild(id) => {
                let guild = self.stores.guild(*id)?;
                // Not in the pinned client: MemberCount's counts under the name (`plugin_hooks/member_count.rs`).
                let counts = self.member_count_tooltip(*id);
                // Module 396655 `ih`: "Invites paused for this server" under the name (`u = nq.Ig(guild)`, module
                // 347951: paused, and the account may manage the server).
                let paused = match self.invites_pause_state(*id) {
                    (true, true) => Some(
                        div()
                            .mt(px(2.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("2n0/Fk")),
                    ),
                    _ => None,
                };
                let framed = || match (&counts, &paused) {
                    (None, None) => frame(),
                    _ => frame().flex_col().items_start(),
                };
                framed()
                    .child(
                        div()
                            .max_w(px(MAX_WIDTH - 24.))
                            .flex()
                            .items_start()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .children(badge_image(guild).map(|badge| {
                                img(badge).flex_none().mt(px(2.)).mr(px(8.)).size(px(18.))
                            }))
                            .child(div().min_w(px(0.)).line_clamp(2).child(guild.name.clone())),
                    )
                    .children(paused)
                    .children(counts)
                    .into_any_element()
            }
        })
    }

    /// The tooltips on screen: the one under the pointer and any that is still leaving. Module 280336 runs
    /// `useTransition` with `{tension: 2400, friction: 52}`, from `{scale: .95, opacity: 0}` to `{1, 1}`; the opacity
    /// of that spring fades the body and caret, and its scale is painted about the body's centre (`Placed`). Under
    /// reduced motion the
    /// official `from` and `leave` keep scale 1 and still fade, which is what this does.
    pub(super) fn tip_layer(&self) -> Vec<AnyElement> {
        self.tip_layer_in(None)
    }

    /// The window the call is popped out to, if it is.
    fn popout_window(&self) -> Option<WindowId> {
        self.vv
            .popout
            .map(|handle| gpui::AnyWindowHandle::from(handle).window_id())
    }

    /// A window is closing: its tooltip goes with it.
    pub(super) fn drop_tips_of(&mut self, window: WindowId) {
        if self
            .tip
            .as_ref()
            .is_some_and(|tip| tip.window == Some(window))
        {
            self.tip = None;
            self.tip_pending = None;
        }
    }

    /// The tooltips of the window `layer` (`None` for the main window): those whose element was hovered there.
    pub(super) fn tip_layer_in(&self, layer: Option<WindowId>) -> Vec<AnyElement> {
        let popout = self.popout_window();
        let frames: Vec<(Active, f32)> = TIPS.with(|tips| {
            let mut tips = tips.borrow_mut();
            let wanted = self.tip.as_ref();
            for (key, presence) in tips.iter_mut() {
                if wanted.is_none_or(|active| active.key != *key) {
                    presence.set(None);
                }
            }
            if let Some(active) = wanted {
                match tips.iter_mut().find(|(key, _)| *key == active.key) {
                    Some((_, presence)) => presence.set(Some(active.clone())),
                    None => {
                        let mut presence = Presence::new(
                            &("tooltip", active.key.clone()),
                            Drive::Spring(presets::TOOLTIP),
                        );
                        presence.set(Some(active.clone()));
                        tips.push((active.key.clone(), presence));
                    }
                }
            }
            let mut frames = Vec::new();
            tips.retain_mut(|(_, presence)| match presence.frame() {
                Some(frame) => {
                    frames.push(frame);
                    true
                }
                None => false,
            });
            frames
        });
        frames
            .into_iter()
            .filter(|(active, _)| drawn_in(active.window, layer, popout))
            .filter_map(|(active, progress)| {
                let body = self.tip_body(&active.tip.body)?;
                let opacity = progress.clamp(0., 1.);
                let body = div().child(body).opacity(opacity).into_any_element();
                let has_caret = !matches!(active.tip.body, Body::Reaction { .. });
                let is_green = matches!(active.tip.body, Body::Green(_));
                let caret = div()
                    .when(has_caret, |this| {
                        if is_green {
                            this.child(caret_colored(active.tip.side, "--green-360", None))
                        } else {
                            this.child(caret(active.tip.side))
                        }
                    })
                    .opacity(opacity)
                    .into_any_element();
                Some(
                    deferred(Placed {
                        anchor: active.anchor,
                        side: active.tip.side,
                        spacing: px(active.tip.spacing),
                        body,
                        caret,
                        scale: if super::motion::reduced() {
                            1.
                        } else {
                            0.95 + 0.05 * progress
                        },
                        shake: active.tip.shake,
                    })
                    .with_priority(10)
                    .into_any_element(),
                )
            })
            .collect()
    }
}

thread_local! {
    /// One presence per tooltip key, so a tooltip that is leaving fades while the next one comes in.
    static TIPS: RefCell<Vec<(SharedString, Presence<Active>)>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use super::{Side, caret_origin, drawn_in, place};
    use gpui::{Bounds, WindowId, point, px, size};

    #[test]
    fn a_tooltip_shows_in_the_window_that_was_hovered_and_nowhere_else() {
        let main = Some(WindowId::from(1));
        let popout = Some(WindowId::from(2));
        // Hovered in the popped-out call window: only that window's layer draws it.
        assert!(drawn_in(popout, popout, popout));
        assert!(!drawn_in(popout, None, popout));
        // Hovered in the main window, or placed by its owner there: only the main layer.
        for tip in [main, None] {
            assert!(drawn_in(tip, None, popout));
            assert!(!drawn_in(tip, popout, popout));
        }
        // Without a popped-out window everything is the main window's.
        assert!(drawn_in(main, None, None));
        assert!(drawn_in(None, None, None));
    }

    fn anchor() -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(16.), px(137.)), size(px(40.), px(40.)))
    }

    #[test]
    fn the_guild_tooltip_lands_where_the_capture_has_it() {
        let viewport = size(px(1280.), px(720.));
        let body = size(px(200.), px(56.));
        let origin = place(anchor(), body, Side::Right, px(12.), viewport);
        assert_eq!(origin, point(px(68.), px(129.)));
        let caret = caret_origin(anchor(), Bounds::new(origin, body), Side::Right);
        // Unrotated box of the capture's caret, which turns into 60..70 by 150..166.
        assert_eq!(caret, point(px(57.), px(153.)));
    }

    #[test]
    fn a_tooltip_at_the_window_edge_slides_in() {
        let viewport = size(px(300.), px(720.));
        let target = Bounds::new(point(px(280.), px(10.)), size(px(20.), px(20.)));
        let origin = place(
            target,
            size(px(100.), px(34.)),
            Side::Bottom,
            px(11.),
            viewport,
        );
        assert_eq!(origin.x, px(192.));
    }
}
