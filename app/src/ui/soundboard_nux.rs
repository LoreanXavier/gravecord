//! The Soundboard Desktop NUX of the call bar (module 987933, `SoundboardDesktopNuxCoachmark`): when it shows (module
//! 533978) and how it looks (the popover of module 43105 in the `BasePopover` of module 195118, stylesheet
//! `81IzEA` of module 489387, the pink gradient of module 315629 and the caret of module 31188).

use super::reference_layer::{self, Caret, Props, Side};
use super::{Root, color, icon};
use gpui::{
    AnyElement, Context, FontWeight, ObjectFit, Radians, RenderImage, Rgba, SharedString,
    Transformation, div, img, point, prelude::*, px, relative, svg,
};
use std::cell::RefCell;
use std::f32::consts::PI;
use std::sync::Arc;
use std::time::Duration;

/// Module 533978: the tip waits this long after the call bar became eligible (`setTimeout(() => eC(!0), 300)`).
pub(super) const DELAY: Duration = Duration::from_millis(300);

/// Module 401191: the tip's graphic (`graphic: {type: "image", src}`).
const GRAPHIC: &str = "https://cdn.discordapp.com/assets/content/1ace7e5e5388cb95117f01f2d35abf5eba6b9092df1848155b7bf7ffe7242944.svg";

/// Module 195118 as module 987933 opens it: `position: "top"`, `spacing: 14`, `autoInvert: true`, and
/// `nudgeAlignIntoViewport` for a top or bottom popout.
pub(super) const POPOUT: Props = Props {
    position: Side::Top,
    spacing: 14.,
    auto_invert: true,
    nudge_align_into_viewport: true,
};

/// Module 533978's picker `Popout` (module 265872): `position: "top"`, `align: "center"`, `spacing: 16`, and that
/// module's defaults `autoInvert: true`, `nudgeAlignIntoViewport: true`.
pub(super) const PICKER_POPOUT: Props = Props {
    position: Side::Top,
    spacing: 16.,
    auto_invert: true,
    nudge_align_into_viewport: true,
};

/// `--custom-popover-width` of module 195118.
const WIDTH: f32 = 240.;
/// `--space-16`, the popover's padding.
const PADDING: f32 = 16.;
/// `--radius-md`.
const RADIUS: f32 = 12.;
/// The 22 by 14 caret of module 31188 (`--custom-caret-half-width: 11px`, `--custom-caret-half-height: 7px`), 1px
/// into the popover's edge (`--custom-caret-border-overlap`).
const CARET_WIDTH: f32 = 22.;
const CARET_HEIGHT: f32 = 14.;
const CARET_OVERLAP: f32 = 1.;
/// `gradientOffsetBottom` default of module 195118 (`--custom-gradient-offset-bottom: 40%`).
const GRADIENT_OFFSET_BOTTOM: f32 = 0.4;

/// Module 533978 (`ex`): the call bar offers the tip with at least two voice states in the channel, the Soundboard
/// button usable (not muted, suppressed or deafened), no call bar popout open (`whichPopoutIsOpen == null`) and the
/// call not idle.
pub(super) fn eligible(users: usize, disabled: bool, popout_open: bool, idle: bool) -> bool {
    users >= 2 && !disabled && !popout_open && !idle
}

/// Whether the 300ms wait of module 533978 is over for a call bar eligible since `since`.
pub(super) fn waited(since: std::time::Instant, now: std::time::Instant) -> bool {
    now.saturating_duration_since(since) >= DELAY
}

/// `.container_pmhtZL.pink_pmhtZL:before`, drawn once at a fixed size and stretched: its gradient and mask are in
/// percent of its box.
fn gradient() -> Arc<RenderImage> {
    thread_local! {
        static CACHE: RefCell<Option<([u32; 8], Arc<RenderImage>)>> = const { RefCell::new(None) };
    }
    let (start, end) = (
        color("--expressive-gradient-pink-start"),
        color("--expressive-gradient-pink-end"),
    );
    let key = [
        start.r, start.g, start.b, start.a, end.r, end.g, end.b, end.a,
    ]
    .map(f32::to_bits);
    CACHE.with(|cache| {
        if let Some((known, image)) = cache.borrow().as_ref()
            && *known == key
        {
            return image.clone();
        }
        let pixels = super::channel_status::gradient_pixels(start, end, 240, 200);
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
        *cache.borrow_mut() = Some((key, image.clone()));
        image
    })
}

/// The caret pointing down (`caret--bottom`, `rotate(0deg)`) or up (`caret--top`, `rotate(180deg)`): the fill
/// (`--background-surface-high`), the gradient (`--custom-popover-caret-gradient-color`, set for a top caret only:
/// `color-mix(in oklab, end 50%, start)`) and the stroke (`--border-subtle`, masked to the fill).
fn caret(up: bool) -> AnyElement {
    let turn = if up { PI } else { 0. };
    let layer = |path: &'static str, tint: Rgba| {
        svg()
            .absolute()
            .top_0()
            .left_0()
            .w(px(CARET_WIDTH))
            .h(px(CARET_HEIGHT))
            .path(path)
            .text_color(tint)
            .with_transformation(Transformation::rotate(Radians(turn)))
    };
    let gradient = up.then(|| {
        super::profile_theme::mix_oklab_fraction(
            color("--expressive-gradient-pink-end"),
            color("--expressive-gradient-pink-start"),
            0.5,
        )
    });
    div()
        .w(px(CARET_WIDTH))
        .h(px(CARET_HEIGHT))
        .child(layer(
            "icons/popout_caret_down.svg",
            color("--background-surface-high"),
        ))
        .children(gradient.map(|tint| layer("icons/popout_caret_down.svg", tint)))
        .child(layer(
            "icons/popover_caret_stroke.svg",
            color("--border-subtle"),
        ))
        .into_any_element()
}

impl Root {
    /// The tip in its layer over the call bar's Soundboard button.
    pub(super) fn soundboard_nux_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        reference_layer::reference_layer(
            POPOUT,
            2,
            self.soundboard_nux_card(cx),
            Some(Caret {
                down: caret(false),
                up: caret(true),
                size: gpui::Size {
                    width: CARET_WIDTH,
                    height: CARET_HEIGHT,
                },
                overlap: CARET_OVERLAP,
            }),
        )
    }

    /// Module 43105 with `graphic`, `title` `UaGc2V`, `body` `lyhz2x`, one action (`SoundboardIcon`, `6EJvHt`) and
    /// `gradientColor: "pink"`:
    /// - `popoverGradientWrapper_81IzEA`: `--background-surface-high`, `--radius-md`, a 1px `--border-subtle` ring
    ///   inside and `--shadow-high`, under the pink gradient over its top 60%.
    /// - `popoverContentWithGradient_81IzEA`: 240 wide, 16 padding, centred text. Its one block child keeps its
    ///   margins: the graphic (at most 144 wide, 16:9) 16 under the top and 20 over the header, the header
    ///   (`heading-sm/semibold` title, `body-sm/normal` body in `--text-subtle`, 4 apart) 20 over the action bar.
    /// - The close button (`sm`, `color-mix`) 4px in from the top end; the action an `sm` primary button, full
    ///   width.
    fn soundboard_nux_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let art = self.images.get(GRAPHIC).cloned();
        if art.is_none() {
            self.want_image(GRAPHIC.to_owned(), GRAPHIC.to_owned(), cx);
        }
        let close = div()
            .id("soundboard-nux-close")
            .group("soundboard-nux-close")
            .absolute()
            .top(px(4.))
            .right(px(4.))
            // `sm`: a 30px box inside a 1px border.
            .size(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            // `color-mix`: the `:before` fills on hover and press, with a `--border-subtle` edge.
            .hover(|style| {
                style
                    .bg(color("--control-icon-only-background-hover"))
                    .border_color(color("--border-subtle"))
            })
            .active(|style| {
                style
                    .bg(color("--control-icon-only-background-active"))
                    .border_color(color("--border-subtle"))
            })
            .aria_label(SharedString::from(dc_intl::t("cpT0Cq")))
            .child(
                icon("modal_close", 16., "--icon-strong")
                    .group_hover("soundboard-nux-close", |style| {
                        style.text_color(color("--control-icon-only-icon-hover"))
                    }),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| {
                this.dismiss_soundboard_desktop_nux();
                cx.notify();
            }));
        let graphic = div()
            .mt(px(16.))
            .mb(px(20.))
            .w_full()
            .flex()
            .justify_center()
            .child(
                // `container_sYj23M aspect-ratio-16/9`, at most 144 wide (`graphic--md`).
                div()
                    .w(px(144.))
                    .h(px(81.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .children(art.map(|art| {
                        img(art)
                            .w(px(144.))
                            .h(px(81.))
                            .object_fit(ObjectFit::Contain)
                    })),
            );
        let header = div()
            .mb(px(20.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("UaGc2V")),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(dc_intl::t("lyhz2x")),
            );
        let action = div()
            .id("soundboard-nux-open-cta")
            .w_full()
            .h(px(32.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-primary-border-default"))
            .bg(color("--control-primary-background-default"))
            .hover(|style| {
                style
                    .bg(color("--control-primary-background-hover"))
                    .border_color(color("--control-primary-border-hover"))
            })
            .active(|style| {
                style
                    .bg(color("--control-primary-background-active"))
                    .border_color(color("--control-primary-border-active"))
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-primary-text-default"))
            .child(icon(
                "voice_soundboard",
                16.,
                "--control-primary-text-default",
            ))
            .child(dc_intl::t("6EJvHt"))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                // `onClick: () => { t(); g(TAKE_ACTION) }`: the call bar's picker opens, the tip is dismissed.
                this.dismiss_soundboard_desktop_nux();
                this.open_call_bar_soundboard(window, cx);
                cx.notify();
            }));
        div()
            .id("soundboard-desktop-nux-card")
            .occlude()
            .relative()
            .flex_none()
            .w(px(WIDTH))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // The gradient's box is the wrapper's border box; the ring is an inset shadow there, not a border.
            .child(
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(-1.))
                    .right(px(-1.))
                    .bottom(relative(GRADIENT_OFFSET_BOTTOM))
                    .child(
                        img(gradient())
                            .size_full()
                            .rounded(px(RADIUS))
                            .object_fit(ObjectFit::Fill),
                    ),
            )
            .child(
                div()
                    // The 16px padding, less the 1px ring drawn as a border here.
                    .p(px(PADDING - 1.))
                    .flex()
                    .flex_col()
                    .text_center()
                    .child(graphic)
                    .child(header)
                    .child(action),
            )
            .child(close)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn the_tip_needs_two_people_a_usable_button_no_popout_and_a_busy_call() {
        assert!(eligible(2, false, false, false));
        assert!(!eligible(1, false, false, false));
        assert!(!eligible(2, true, false, false));
        assert!(!eligible(2, false, true, false));
        assert!(!eligible(2, false, false, true));
    }

    #[test]
    fn the_tip_waits_300ms() {
        let since = Instant::now();
        assert!(!waited(since, since + Duration::from_millis(299)));
        assert!(waited(since, since + Duration::from_millis(300)));
    }
}
