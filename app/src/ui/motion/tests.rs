use super::*;

fn close(a: f64, b: f64, tolerance: f64) {
    assert!(
        (a - b).abs() <= tolerance,
        "{a} is not {b} (tolerance {tolerance})"
    );
}

#[test]
fn the_css_keywords_match_an_independent_solver() {
    // Values from bisecting the bezier with 200 iterations.
    let cases: [(Ease, [f64; 5]); 5] = [
        (
            Ease::Ease,
            [
                0.094796306,
                0.408510591,
                0.802403388,
                0.960458978,
                0.994316477,
            ],
        ),
        (
            Ease::EaseIn,
            [
                0.017026610,
                0.093464651,
                0.315356813,
                0.621861869,
                0.839427846,
            ],
        ),
        (
            Ease::EaseOut,
            [
                0.160572154,
                0.378138131,
                0.684643187,
                0.906535349,
                0.982973390,
            ],
        ),
        (
            Ease::EaseInOut,
            [
                0.019722454,
                0.129161931,
                0.500000000,
                0.870838069,
                0.980277546,
            ],
        ),
        (
            Ease::Bezier(0.16, 1., 0.3, 1.),
            [
                0.494390617,
                0.825622301,
                0.971779105,
                0.997677067,
                0.999878348,
            ],
        ),
    ];
    for (ease, expected) in cases {
        for (x, want) in [0.1, 0.25, 0.5, 0.75, 0.9].into_iter().zip(expected) {
            close(ease.apply(x), want, 2e-6);
        }
        assert_eq!(ease.apply(0.), 0.);
        close(ease.apply(1.), 1., 1e-9);
    }
    close(Ease::Bezier(0.4, 0., 1., 1.).apply(0.5), 0.324814611, 2e-6);
    close(Ease::Linear.apply(0.3), 0.3, 1e-12);
}

#[test]
fn an_overshooting_bezier_leaves_the_unit_range() {
    // `jumbleWrapper` in the stylesheets: cubic-bezier(0.48, -0.08, 0.06, 1.4).
    let ease = Ease::Bezier(0.48, -0.08, 0.06, 1.4);
    let samples: Vec<f64> = (1..100).map(|i| ease.apply(i as f64 / 100.)).collect();
    assert!(samples.iter().any(|v| *v < 0.));
    assert!(samples.iter().any(|v| *v > 1.));
}

#[test]
fn linear_stops_interpolate_between_their_points() {
    let ease = Ease::Stops(&[(0., 0.), (0.5, 0.8), (1., 1.)]);
    close(ease.apply(0.25), 0.4, 1e-6);
    close(ease.apply(0.75), 0.9, 1e-6);
    close(ease.apply(1.), 1., 1e-6);
}

fn run(config: SpringConfig, from: f64, to: f64, at_ms: &[u32]) -> Vec<f64> {
    let mut spring = Spring::new(config, from, 0.);
    spring.retarget(to, 0.);
    let mut out = Vec::new();
    let mut last = 0;
    for &ms in at_ms {
        spring.advance(ms - last);
        last = ms;
        out.push(spring.position());
    }
    out
}

// The expected numbers come from running the `SpringValue.advance` loop of module 666764, copied as it stands in the
// bundle, in Node (one millisecond per frame, from 0 to 1).
#[test]
fn the_tooltip_spring_follows_the_bundles_loop() {
    let got = run(presets::TOOLTIP, 0., 1., &[10, 25, 50, 100, 200, 400]);
    let want = [
        0.111004216,
        0.472902773,
        0.992431971,
        1.071236245,
        0.999838470,
        1.0,
    ];
    for (got, want) in got.iter().zip(want) {
        close(*got, want, 1e-8);
    }
}

#[test]
fn every_preset_follows_the_bundles_loop() {
    let at = [10, 25, 50, 100, 200, 400, 600];
    let cases: [(SpringConfig, [f64; 7]); 4] = [
        (
            presets::DEFAULT,
            [
                0.008644118,
                0.044896038,
                0.142783142,
                0.379148809,
                0.737048520,
                0.966462642,
                0.996419912,
            ],
        ),
        (
            presets::WOBBLY,
            [
                0.009536586,
                0.052744038,
                0.183423947,
                0.551015871,
                1.122335015,
                1.035194184,
                0.973464607,
            ],
        ),
        (
            presets::STIFF,
            [
                0.010863494,
                0.057839458,
                0.189741012,
                0.517766290,
                0.957632632,
                1.023246800,
                1.0,
            ],
        ),
        (
            // The call button's spring in module 105225: mass 2, clamped, precision .01.
            SpringConfig::new(600., 45.)
                .mass(2.)
                .clamp(true)
                .precision(0.01),
            [
                0.015393403,
                0.080730986,
                0.257308715,
                0.655913562,
                1.0,
                1.0,
                1.0,
            ],
        ),
    ];
    for (config, want) in cases {
        let got = run(config, 0., 1., &at);
        for (got, want) in got.iter().zip(want) {
            close(*got, want, 1e-8);
        }
    }
}

#[test]
fn the_underdamped_tooltip_spring_overshoots_and_settles_on_the_target() {
    let mut spring = Spring::new(presets::TOOLTIP, 0., 0.);
    spring.retarget(1., 0.);
    let mut peak: f64 = 0.;
    for _ in 0..400 {
        spring.advance(1);
        peak = peak.max(spring.position());
    }
    assert!(peak > 1.12 && peak < 1.16, "peak {peak}");
    assert!(spring.is_done());
    assert_eq!(spring.position(), 1.);
}

#[test]
fn a_clamped_spring_never_passes_its_target() {
    let config = SpringConfig::new(600., 10.).clamp(true);
    let mut spring = Spring::new(config, 0., 0.);
    spring.retarget(1., 0.);
    for _ in 0..600 {
        spring.advance(1);
        // The clamp acts before each step, so one step past the target is the most it can show.
        assert!(spring.position() <= 1.01, "{}", spring.position());
    }
    assert!(spring.is_done());
}

#[test]
fn a_spring_keeps_its_velocity_when_the_target_moves() {
    let mut spring = Spring::new(presets::DEFAULT, 0., 0.);
    spring.retarget(1., 0.);
    spring.step_to(100.);
    let at_turn = spring.position();
    assert!(at_turn > 0.1 && at_turn < 0.9);
    spring.retarget(0., 100.);
    spring.step_to(101.);
    // Still moving up for a moment: the old velocity was kept, it was not restarted from rest.
    assert!(spring.position() > at_turn);
    spring.step_to(2000.);
    assert!(spring.is_done());
    assert_eq!(spring.position(), 0.);
}

#[test]
fn long_gaps_are_stepped_in_chunks_of_at_most_64_ms() {
    let mut a = Spring::new(presets::DEFAULT, 0., 0.);
    a.retarget(1., 0.);
    a.step_to(500.);
    let mut b = Spring::new(presets::DEFAULT, 0., 0.);
    b.retarget(1., 0.);
    for t in (1..=500).step_by(7) {
        b.step_to(t as f64);
    }
    b.step_to(500.);
    close(a.position(), b.position(), 1e-12);
}

#[test]
fn a_transition_runs_from_the_first_value_to_the_target() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let k = key("row");
    let spec = css(150., Ease::EaseOut);
    assert_eq!(value(k, 0., spec), 0.);
    freeze(Some(10.));
    begin_frame(false);
    assert_eq!(value(k, 1., spec), 0.); // starts at 10 ms, nothing has moved
    freeze(Some(10. + 75.));
    begin_frame(false);
    close(value(k, 1., spec) as f64, Ease::EaseOut.apply(0.5), 1e-6);
    assert!(is_active());
    freeze(Some(10. + 150.));
    begin_frame(false);
    assert_eq!(value(k, 1., spec), 1.);
    assert!(!is_active());
}

#[test]
fn a_target_that_changes_mid_flight_starts_from_the_value_on_screen() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let k = key("a");
    let spec = css(100., Ease::Linear);
    value(k, 0., spec);
    value(k, 1., spec);
    freeze(Some(40.));
    begin_frame(false);
    close(value(k, 1., spec) as f64, 0.4, 1e-6);
    // Retarget to 2 at 40 ms: it leaves from 0.4 and takes the full 100 ms.
    close(value(k, 2., spec) as f64, 0.4, 1e-6);
    freeze(Some(90.));
    begin_frame(false);
    close(value(k, 2., spec) as f64, 0.4 + 1.6 * 0.5, 1e-6);
    freeze(Some(140.));
    begin_frame(false);
    close(value(k, 2., spec) as f64, 2., 1e-9);
}

#[test]
fn a_reversal_is_only_as_long_as_the_way_back() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let k = key("hover");
    let spec = css(200., Ease::Linear);
    value(k, 0., spec);
    value(k, 1., spec);
    freeze(Some(50.)); // a quarter of the way
    begin_frame(false);
    close(value(k, 1., spec) as f64, 0.25, 1e-9);
    value(k, 0., spec); // reverses: 0.25 to go, so 50 ms
    freeze(Some(75.));
    begin_frame(false);
    close(value(k, 0., spec) as f64, 0.125, 1e-9);
    freeze(Some(100.));
    begin_frame(false);
    close(value(k, 0., spec) as f64, 0., 1e-9);
    assert!(!is_active());
}

#[test]
fn the_delay_holds_the_first_value() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let k = key("d");
    let spec = css(100., Ease::Linear).delay(100.);
    value(k, 0., spec);
    value(k, 1., spec);
    freeze(Some(99.));
    begin_frame(false);
    assert_eq!(value(k, 1., spec), 0.);
    freeze(Some(150.));
    begin_frame(false);
    close(value(k, 1., spec) as f64, 0.5, 1e-9);
}

#[test]
fn colours_mix_premultiplied() {
    let clear = Rgba {
        r: 0.,
        g: 0.,
        b: 0.,
        a: 0.,
    };
    let white = Rgba {
        r: 1.,
        g: 1.,
        b: 1.,
        a: 1.,
    };
    let half = mix(clear, white, 0.5);
    // Premultiplied, half way between transparent black and white is white at half alpha.
    close(half.r as f64, 1., 1e-6);
    close(half.a as f64, 0.5, 1e-6);
}

#[test]
fn reduced_motion_snaps_gated_transitions_and_keeps_the_others() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(true);
    let gated = key("gated");
    let plain = key("plain");
    let full = css(200., Ease::Linear).full_motion();
    let always = css(200., Ease::Linear);
    value(gated, 0., full);
    value(plain, 0., always);
    freeze(Some(10.));
    begin_frame(true);
    assert_eq!(value(gated, 1., full), 1.);
    close(value(plain, 1., always) as f64, 0., 1e-9);
    freeze(Some(110.));
    begin_frame(true);
    close(value(plain, 1., always) as f64, 0.5, 1e-9);
}

#[test]
fn the_official_reduced_motion_decision() {
    // Module 775602: sync on follows the system, off follows the app's own switch.
    assert!(decide_reduced(true, false, true));
    assert!(!decide_reduced(true, true, false));
    assert!(decide_reduced(false, true, false));
    assert!(!decide_reduced(false, false, true));
}

#[test]
fn a_leaving_element_stays_mounted_until_its_exit_ends() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let mut presence: Presence<&str> = Presence::new("tip", Drive::Css(css(100., Ease::Linear)));
    assert!(presence.frame().is_none());
    presence.set(Some("hello"));
    let (value, progress) = presence.frame().unwrap();
    assert_eq!((value, progress), ("hello", 0.));
    freeze(Some(100.));
    begin_frame(false);
    assert_eq!(presence.frame().unwrap().1, 1.);
    presence.set(None);
    freeze(Some(150.));
    begin_frame(false);
    let (value, progress) = presence.frame().unwrap();
    assert_eq!(value, "hello");
    close(progress as f64, 0.5, 1e-6);
    assert!(presence.is_mounted());
    freeze(Some(250.));
    begin_frame(false);
    assert!(presence.frame().is_none());
    assert!(!presence.is_mounted());
}

#[test]
fn a_spring_presence_overshoots_then_leaves() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let mut presence: Presence<u8> = Presence::new("spring", Drive::Spring(presets::TOOLTIP));
    presence.set(Some(1));
    presence.frame();
    freeze(Some(100.));
    begin_frame(false);
    close(presence.frame().unwrap().1 as f64, 1.071236245, 1e-6);
    freeze(Some(300.));
    begin_frame(false);
    assert_eq!(presence.frame().unwrap().1, 1.);
    presence.set(None);
    freeze(Some(300. + 400.));
    begin_frame(false);
    assert!(presence.frame().is_none());
}

#[test]
fn showing_again_during_the_exit_turns_it_around() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let mut presence: Presence<u8> = Presence::new("again", Drive::Css(css(100., Ease::Linear)));
    presence.set(Some(1));
    presence.frame();
    freeze(Some(100.));
    begin_frame(false);
    presence.frame();
    presence.set(None);
    freeze(Some(150.));
    begin_frame(false);
    close(presence.frame().unwrap().1 as f64, 0.5, 1e-6);
    presence.set(Some(2));
    freeze(Some(170.));
    begin_frame(false);
    let (value, progress) = presence.frame().unwrap();
    assert_eq!(value, 2);
    assert!(progress > 0.5);
}

#[test]
fn an_idle_store_asks_for_no_frames() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let k = key("idle");
    value(k, 0.5, css(100., Ease::Linear));
    assert!(!is_active());
    freeze(Some(1000.));
    begin_frame(false);
    value(k, 0.5, css(100., Ease::Linear));
    assert!(!is_active());
}

#[test]
fn hover_is_a_number_through_a_transition() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let k = key("hover-row");
    let spec = css(100., Ease::Linear);
    assert_eq!(hover_value(k, spec), 0.);
    set_hover(k, true);
    hover_value(k, spec);
    freeze(Some(50.));
    begin_frame(false);
    close(hover_value(k, spec) as f64, 0.5, 1e-9);
}

// Reference: the RK4 loop of module 222574 (one millisecond a step) in Node, with the tension 100, friction 10 of
// the layer stack's `S8` config in module 596339.
#[test]
fn the_layer_spring_follows_the_bundles_rk4_loop() {
    let mut spring = RnSpring::new(100., 10., 0., 0.);
    spring.retarget(1., 0.);
    let mut last = 0.;
    for (ms, want) in [
        (10., 0.019881814),
        (50., 0.326468744),
        (100., 0.746303316),
        (200., 1.031306790),
        (300., 1.013116164),
    ] {
        spring.step_to(ms);
        last = ms;
        close(spring.position(), want, 1e-8);
    }
    assert!(last > 0.);
    spring.step_to(2000.);
    assert!(spring.is_done());
    assert_eq!(spring.position(), 1.);
}

#[test]
fn something_that_mounts_enters_again_after_a_gap() {
    reset_for_tests();
    freeze(Some(0.));
    begin_frame(false);
    let k = key("popout");
    let drive = Drive::Css(css(80., Ease::EaseOut));
    assert_eq!(enter(k, drive), 0.);
    freeze(Some(100.));
    begin_frame(false);
    assert_eq!(enter(k, drive), 1.);
    // A frame without it: it left. Drawn again, it enters again.
    freeze(Some(200.));
    begin_frame(false);
    freeze(Some(300.));
    begin_frame(false);
    assert_eq!(enter(k, drive), 0.);
}

/// Draws a frame of a layer at `ms` on the frozen clock.
fn frame_at(ms: f64, reduced: bool, id: &str, layer: bool) -> Pose {
    freeze(Some(ms));
    begin_frame(reduced);
    if layer {
        layer_pose(id)
    } else {
        modal_pose(id)
    }
}

#[test]
fn a_modal_enters_after_64ms_and_leaves_on_the_stiffer_spring() {
    reset_for_tests();
    let first = frame_at(0., false, "dialog", false);
    assert_eq!((first.opacity, first.scale), (0., 0.9));
    // The scrim runs from the first frame, the dialog only after its delay.
    let early = frame_at(40., false, "dialog", false);
    assert!(early.scrim > 0.3, "scrim {}", early.scrim);
    assert_eq!(early.opacity, 0.);
    frame_at(70., false, "dialog", false);
    let later = frame_at(110., false, "dialog", false);
    assert!(later.opacity > 0. && later.scale > 0.9 && later.scale < 1.1);
    let rest = frame_at(1500., false, "dialog", false);
    assert_eq!((rest.scrim, rest.opacity), (1., 1.));
    close(rest.scale as f64, 1., 1e-3);

    // Leaving: no delay, back to opacity 0 and scale .9, and it settles.
    assert!(begin_leave("dialog"));
    assert!(!begin_leave("dialog"));
    let start = frame_at(1500., false, "dialog", false);
    assert!(start.opacity > 0.99);
    let mid = frame_at(1530., false, "dialog", false);
    assert!(mid.opacity < start.opacity && mid.opacity > 0.);
    let gone = frame_at(3000., false, "dialog", false);
    assert_eq!(gone.opacity, 0.);
    close(gone.scale as f64, 0.9, 1e-3);
    assert!(settled(&pose_keys("dialog")));
    end_leave("dialog");
    assert!(!is_leaving("dialog"));
}

#[test]
fn reduced_motion_keeps_layers_at_scale_one() {
    reset_for_tests();
    let modal = frame_at(0., true, "reduced-modal", false);
    assert_eq!(modal.scale, 1.);
    let layer = frame_at(0., true, "reduced-layer", true);
    assert_eq!(layer.scale, 1.);
    let rest = frame_at(5000., true, "reduced-layer", true);
    assert_eq!((rest.opacity, rest.scale), (1., 1.));
}

#[test]
fn a_layer_enters_from_scale_1_1_and_leaves_back_to_it() {
    reset_for_tests();
    let first = frame_at(0., false, "layer", true);
    assert_eq!((first.opacity, first.scale), (0., 1.1));
    let mid = frame_at(100., false, "layer", true);
    assert!(mid.opacity > 0. && mid.scale < 1.1);
    let rest = frame_at(6000., false, "layer", true);
    assert_eq!(rest.opacity, 1.);
    close(rest.scale as f64, 1., 1e-3);
    begin_leave("layer");
    frame_at(6000., false, "layer", true);
    let out = frame_at(12000., false, "layer", true);
    assert_eq!(out.opacity, 0.);
    close(out.scale as f64, 1.1, 1e-3);
    assert!(settled(&pose_keys("layer")));
}

#[test]
fn a_pose_dims_the_scrim_by_its_factor_and_keeps_the_colour() {
    let pose = Pose {
        scrim: 0.5,
        opacity: 0.,
        scale: 0.9,
    };
    let base = gpui::rgba(0x000000b8);
    let dim = pose.dim(base);
    assert_eq!((dim.r, dim.g, dim.b), (base.r, base.g, base.b));
    close(dim.a as f64, base.a as f64 / 2., 1e-6);
    let open = Pose {
        scrim: 1.,
        opacity: 1.,
        scale: 1.,
    };
    assert_eq!(open.dim(base), base);
}

#[test]
fn only_the_media_viewer_leaves_without_a_scale() {
    assert!(subtle_layer("Name(\"lightbox\")"));
    assert!(!subtle_layer("Name(\"lightbox-more-menu\")"));
    assert!(!subtle_layer("Name(\"forward-layer\")"));
}

fn pop_at(ms: f64, reduced: bool, id: &str, kind: PopKind, side: PopSide) -> PopPose {
    freeze(Some(ms));
    begin_frame(reduced);
    popout_pose(id, kind, side)
}

#[test]
fn a_scale_popout_starts_10ms_late_at_scale_01_and_follows_ease_out_over_120ms() {
    reset_for_tests();
    let first = pop_at(0., false, "menu", PopKind::Scale, PopSide::Bottom);
    assert_eq!((first.opacity, first.scale), (0., 0.01));
    assert_eq!(first.origin, (0.5, 0.));
    let still = pop_at(10., false, "menu", PopKind::Scale, PopSide::Bottom);
    assert_eq!(still.scale, 0.01);
    // 70 ms in: progress is ease-out(60 / 120), then the end of it.
    let mid = pop_at(70., false, "menu", PopKind::Scale, PopSide::Bottom);
    let expected = Ease::EaseOut.apply(0.5) as f32;
    close(mid.opacity as f64, expected as f64, 1e-3);
    close(mid.scale as f64, (0.01 + 0.99 * expected) as f64, 1e-3);
    let end = pop_at(200., false, "menu", PopKind::Scale, PopSide::Bottom);
    assert_eq!((end.opacity, end.scale), (1., 1.));
}

#[test]
fn a_fade_popout_takes_80ms_and_never_scales() {
    reset_for_tests();
    pop_at(0., false, "picker", PopKind::Fade, PopSide::Top);
    let end = pop_at(100., false, "picker", PopKind::Fade, PopSide::Top);
    assert_eq!((end.opacity, end.scale), (1., 1.));
    let mid = pop_at(50., false, "other", PopKind::Fade, PopSide::Top);
    assert_eq!(mid.scale, 1.);
}

#[test]
fn reduced_motion_turns_every_popout_into_a_fade() {
    reset_for_tests();
    pop_at(0., true, "menu", PopKind::Scale, PopSide::Bottom);
    let mid = pop_at(60., true, "menu", PopKind::Scale, PopSide::Bottom);
    assert_eq!(mid.scale, 1.);
}

fn frozen(ms: f64) {
    freeze(Some(ms));
    begin_frame(false);
}

#[test]
fn a_right_scale_popout_grows_about_its_top_centre() {
    reset_for_tests();
    let first = pop_at(0., false, "menu", PopKind::Scale, PopSide::Right);
    assert_eq!(first.origin, (0.5, 0.));
}

#[test]
fn the_typing_dots_step_through_the_official_keyframes() {
    reset_for_tests();
    set_window_active(true);
    // The first dot is at its 0% keyframe at t = 0 (delay -1.2 s on a 1.2 s loop): opacity .32, scale .8.
    frozen(0.);
    assert_eq!(typing_dot(0, true), (0.32, 0.8));
    // A quarter of the loop is 300 ms: the end of the first interval, opacity 1 and scale 1, but steps(12)
    // jumps only at the end of each twelfth, so just before it the value is 11/12 of the way.
    frozen(299.);
    let (opacity, scale) = typing_dot(0, true);
    close(opacity as f64, 0.32 + 0.68 * 11. / 12., 1e-4);
    close(scale as f64, 0.8 + 0.2 * 11. / 12., 1e-4);
    frozen(300.);
    assert_eq!(typing_dot(0, true), (1., 1.));
    // The second dot (delay -1.05 s) is 150 ms behind the first: it peaks at 450 ms.
    frozen(450.);
    assert_eq!(typing_dot(1, true), (1., 1.));
    // Without focus every dot is at full opacity.
    assert_eq!(typing_dot(2, false), (1., 1.));
}

#[test]
fn a_count_rolls_up_when_it_grows_and_down_when_it_shrinks() {
    reset_for_tests();
    set_window_active(true);
    frozen(0.);
    let at_rest = number_roll(1, 3);
    assert_eq!((at_rest.current, at_rest.previous), (3, None));
    frozen(10.);
    let grew = number_roll(1, 4);
    assert_eq!(grew.current, 4);
    assert!(grew.current_y > 0.9, "the new number starts below");
    assert_eq!(grew.previous.map(|(value, _)| value), Some(3));
    frozen(400.);
    let done = number_roll(1, 4);
    assert_eq!((done.current_y, done.previous), (0., None));
    frozen(410.);
    let shrank = number_roll(1, 2);
    assert!(shrank.current_y < -0.9, "the new number starts above");
    // Unfocused changes show at once.
    set_window_active(false);
    frozen(1000.);
    number_roll(2, 1);
    frozen(1010.);
    let quiet = number_roll(2, 9);
    assert_eq!((quiet.current, quiet.previous), (9, None));
    set_window_active(true);
}

#[test]
fn back_easing_overshoots_at_both_ends() {
    close(back_in_out(0.), 0., 1e-9);
    close(back_in_out(1.), 1., 1e-9);
    assert!(back_in_out(0.1) < 0., "pulls back before it goes");
    assert!(back_in_out(0.9) > 1., "and overshoots before it settles");
}

#[test]
fn a_radio_fill_follows_the_fill_in_keyframes() {
    use super::controls::{radio_dot, radio_fill};
    reset_for_tests();
    frozen(0.);
    // First sight is the rest state.
    assert_eq!(radio_fill(7, false).opacity, 0.);
    frozen(100.);
    // Chosen at 100 ms; at 135 ms (35 ms into 225) the scale is on its way to .33 at 35%.
    let begin = radio_fill(7, true);
    assert!(begin.scale < 0.05 && begin.opacity == 0.);
    frozen(100. + 225. * 0.4);
    let overshoot = radio_fill(7, true);
    close(overshoot.scale as f64, 1.15, 1e-3);
    frozen(100. + 400.);
    assert_eq!(radio_fill(7, true).scale, 1.);
    assert_eq!(radio_dot(8, true).scale, 1.);
}

#[test]
fn a_checkmark_draws_in_over_300ms_from_a_scale_of_point_seven() {
    use super::controls::check_stroke;
    reset_for_tests();
    frozen(0.);
    check_stroke(3, false);
    frozen(50.);
    let start = check_stroke(3, true);
    assert_eq!((start.opacity, start.scale), (0., 0.7));
    frozen(50. + 300.);
    let end = check_stroke(3, true);
    assert_eq!((end.opacity, end.scale), (1., 1.));
}

#[test]
fn the_picker_category_rule_is_100ms_ease_in_out_on_the_background() {
    let (props, hovering, resting) = Rule::PickerCategory.parts();
    assert_eq!(props, Props::BACKGROUND_COLOR);
    assert_eq!((hovering.duration_ms, resting.duration_ms), (100., 100.));
}

#[test]
fn an_animation_in_a_region_marks_only_that_region_active() {
    let (root, region) = (gpui::EntityId::from(1), gpui::EntityId::from(2));
    with(|motion| *motion = Motion::default());
    begin_view(root);
    begin_view(region);
    with(|motion| motion.mark_active());
    leave_view();
    with(|motion| {
        assert!(motion.active_views.contains(&region));
        assert!(!motion.active_views.contains(&root));
        assert!(!motion.active);
    });
    // Outside any view the whole window is marked.
    with(|motion| motion.mark_active());
    with(|motion| assert!(motion.active));
    // A new render of the region forgets what it asked for last time.
    begin_view(region);
    with(|motion| assert!(!motion.active_views.contains(&region)));
    leave_view();
}

#[test]
fn the_gap_floor_follows_the_view_being_built() {
    let (a, b) = (gpui::EntityId::from(3), gpui::EntityId::from(4));
    with(|motion| *motion = Motion::default());
    with(|motion| motion.frame = 10);
    begin_view(a);
    with(|motion| motion.frame = 20);
    begin_view(b);
    with(|motion| motion.frame = 30);
    begin_view(a);
    // `a` last rendered in draw 10 (current was 10 before this render), so 10 is the floor; `b` has no earlier render.
    with(|motion| assert_eq!(motion.gap_floor(), 10));
    enter_view(b);
    with(|motion| assert_eq!(motion.gap_floor(), 0));
    leave_view();
    with(|motion| assert_eq!(motion.gap_floor(), 29));
}
