//! pat-s3rrv.205: visibility-range fade alpha.
//!
//! Ported from `RendererSceneCull::_update_instance`
//! (`renderer_scene_cull.cpp:1483-1490`).
//!
//! ⚠️ The margin extends **both ways**: the band is centred on the range value
//! and spans ±margin. The bead's acceptance says `end = 10, margin = 5` fades
//! "across 5..10 units"; Godot's band is **5..15**. Reading it as one-sided
//! halves the fade distance and pops the instance out while still half-opaque.

use gdserver3d::instance::{GeometryInstanceSettings, VisibilityRangeFadeMode};

fn fading(end: f32, end_margin: f32) -> GeometryInstanceSettings {
    GeometryInstanceSettings {
        visibility_range_end: end,
        visibility_range_end_margin: end_margin,
        visibility_range_fade_mode: VisibilityRangeFadeMode::SelfFade,
        ..Default::default()
    }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// ⚠️ THE band: `end ± margin`, not `[end - margin, end]`.
#[test]
fn the_far_fade_band_is_centred_on_the_range() {
    let s = fading(10.0, 5.0);

    assert!(
        approx(s.visibility_fade_alpha(4.0), 1.0),
        "before the band: opaque"
    );
    assert!(
        approx(s.visibility_fade_alpha(5.0), 1.0),
        "at end-margin: still opaque"
    );
    assert!(
        approx(s.visibility_fade_alpha(10.0), 0.5),
        "at the range value itself the instance is HALF faded, got {}",
        s.visibility_fade_alpha(10.0)
    );
    assert!(
        approx(s.visibility_fade_alpha(15.0), 0.0),
        "fully hidden only at end+margin = 15, got {}",
        s.visibility_fade_alpha(15.0)
    );
    assert!(
        approx(s.visibility_fade_alpha(20.0), 0.0),
        "beyond the band: hidden"
    );
}

/// The ramp is monotonic across the band — no plateau, no reversal.
#[test]
fn the_far_ramp_decreases_monotonically() {
    let s = fading(10.0, 5.0);
    let mut prev = 1.0;
    for i in 0..=20 {
        let d = 5.0 + i as f32 * 0.5; // 5.0 ..= 15.0
        let a = s.visibility_fade_alpha(d);
        assert!(
            a <= prev + 1e-5,
            "alpha rose at distance {d}: {prev} -> {a}"
        );
        prev = a;
    }
    assert!(approx(prev, 0.0));
}

/// The near limit fades IN as the camera pulls back.
#[test]
fn the_near_fade_band_ramps_in() {
    let s = GeometryInstanceSettings {
        visibility_range_begin: 10.0,
        visibility_range_begin_margin: 5.0,
        visibility_range_fade_mode: VisibilityRangeFadeMode::SelfFade,
        ..Default::default()
    };
    assert!(
        approx(s.visibility_fade_alpha(5.0), 0.0),
        "at begin-margin: hidden"
    );
    assert!(
        approx(s.visibility_fade_alpha(10.0), 0.5),
        "at the range value: half"
    );
    assert!(
        approx(s.visibility_fade_alpha(15.0), 1.0),
        "at begin+margin: opaque"
    );
    assert!(
        approx(s.visibility_fade_alpha(30.0), 1.0),
        "well beyond: opaque"
    );
}

/// ⚠️ Only `SelfFade` fades.
///
/// Godot calls `set_fade_range(false, ..)` for every other mode. `Disabled`
/// hard-cuts, and `Dependencies` keeps THIS instance opaque — its children fade
/// instead. Returning a ramp for either would make an instance translucent that
/// Godot draws solid.
#[test]
fn only_self_fade_produces_a_ramp() {
    for mode in [
        VisibilityRangeFadeMode::Disabled,
        VisibilityRangeFadeMode::Dependencies,
    ] {
        let s = GeometryInstanceSettings {
            visibility_range_end: 10.0,
            visibility_range_end_margin: 5.0,
            visibility_range_fade_mode: mode,
            ..Default::default()
        };
        assert!(
            approx(s.visibility_fade_alpha(10.0), 1.0),
            "{mode:?} must not fade; it is a hard cut or a child-fade"
        );
    }
}

/// A zero margin is a hard edge, not a division by zero.
#[test]
fn a_zero_margin_is_a_hard_edge() {
    let s = fading(10.0, 0.0);
    assert!(approx(s.visibility_fade_alpha(9.9), 1.0));
    assert!(
        approx(s.visibility_fade_alpha(10.0), 0.0),
        "band collapses to a step"
    );
    for d in [0.0f32, 5.0, 9.0, 10.0, 11.0, 100.0] {
        let a = s.visibility_fade_alpha(d);
        assert!(a.is_finite(), "alpha went non-finite at {d}");
    }
}

/// An instance with no range set never fades.
#[test]
fn no_range_means_no_fade() {
    let s = GeometryInstanceSettings {
        visibility_range_fade_mode: VisibilityRangeFadeMode::SelfFade,
        ..Default::default()
    };
    for d in [0.0f32, 10.0, 1000.0] {
        assert!(approx(s.visibility_fade_alpha(d), 1.0), "no range at {d}");
    }
}

/// Both ends active: whichever is fading more wins.
#[test]
fn overlapping_bands_take_the_smaller_alpha() {
    let s = GeometryInstanceSettings {
        visibility_range_begin: 5.0,
        visibility_range_begin_margin: 5.0,
        visibility_range_end: 10.0,
        visibility_range_end_margin: 5.0,
        visibility_range_fade_mode: VisibilityRangeFadeMode::SelfFade,
        ..Default::default()
    };
    // At 7.5 the near ramp gives 0.75 and the far ramp 0.75; both agree.
    // At 10 the near ramp is fully in (1.0) but the far ramp is 0.5 — take 0.5.
    assert!(
        approx(s.visibility_fade_alpha(10.0), 0.5),
        "the more restrictive band must win, got {}",
        s.visibility_fade_alpha(10.0)
    );
}
