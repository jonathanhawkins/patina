//! pat-s3rrv.113 — Environment brightness/contrast/saturation adjustments.
//!
//! `adjustment_enabled` and its three knobs were registered in ClassDB, so the
//! inspector round-tripped them into the `.tscn`, and the renderer had no field
//! to put them in and discarded every one.
//!
//! 🔑 **The stage straddles the display encode.** Godot multiplies brightness in
//! LINEAR, encodes to sRGB, and only then applies contrast and saturation
//! (`servers/rendering/renderer_rd/shaders/effects/tonemap.glsl:916-947`).
//! Applying all three on one side of the encode is the natural implementation
//! and it produces different pixels — each test below is built so that the
//! one-sided version fails it.
//!
//! With the default `ToneMapper::Linear`, exposure 1.0, and glow and debanding
//! off, `apply_camera_post` is exactly "encode to sRGB, plus this stage", so the
//! expectations are closed-form rather than approximate.

use gdcore::math::{linear_to_srgb_channel, Color};
use gdrender3d::postprocess::apply_camera_post;
use gdserver3d::environment::Environment3D;

/// Runs one pixel through the post chain and returns it.
fn post(pixel: Color, env: &Environment3D) -> Color {
    let mut px = [pixel];
    apply_camera_post(&mut px, 1, 1, env);
    px[0]
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn adjustments_are_off_by_default() {
    // The control. `adjustment_enabled` defaults to false, so the frame is just
    // the sRGB encode — if this drifts, every other assertion here is measuring
    // the wrong thing.
    let env = Environment3D::default();
    assert!(!env.adjustment_enabled);
    assert_eq!(env.adjustment_brightness, 1.0);
    assert_eq!(env.adjustment_contrast, 1.0);
    assert_eq!(env.adjustment_saturation, 1.0);

    let out = post(Color::new(0.25, 0.5, 0.75, 1.0), &env);
    assert!(close(out.r, linear_to_srgb_channel(0.25)));
    assert!(close(out.g, linear_to_srgb_channel(0.5)));
    assert!(close(out.b, linear_to_srgb_channel(0.75)));
    assert_eq!(out.a, 1.0, "alpha is preserved");
}

#[test]
fn saturation_zero_greys_to_the_even_channel_mean_not_luminance() {
    let mut env = Environment3D::default();
    env.adjustment_enabled = true;
    env.adjustment_saturation = 0.0;

    // Pure blue is the discriminating colour: its even mean and its Rec.601
    // luminance are far apart (1/3 versus 0.114), so the two candidate greys
    // cannot be confused. A red or grey fixture would nearly coincide.
    let out = post(Color::new(0.0, 0.0, 1.0, 1.0), &env);

    // 🔑 Godot greys with `dot(vec3(1.0), color.rgb) * (1.0 / 3.0)` — an EVEN
    // average of the three channels. The textbook answer is a luminance
    // weighting, and upstream deliberately does not use one ("maintains
    // compatibility with existing projects").
    let even_mean = 1.0 / 3.0;
    assert!(
        close(out.r, even_mean) && close(out.g, even_mean) && close(out.b, even_mean),
        "expected the even 1/3 mean {even_mean}, got ({}, {}, {})",
        out.r,
        out.g,
        out.b
    );

    // And explicitly not the luma-weighted grey, which is what a
    // "correct-looking" implementation would produce.
    let rec601 = 0.114_f32;
    assert!(
        !close(out.r, rec601),
        "greying to Rec.601 luminance ({rec601}) is the plausible wrong answer"
    );

    // Saturation must still be a no-op on an already-grey pixel.
    let grey_in = post(Color::new(0.5, 0.5, 0.5, 1.0), &env);
    let encoded = linear_to_srgb_channel(0.5);
    assert!(close(grey_in.r, encoded) && close(grey_in.b, encoded));
}

#[test]
fn brightness_multiplies_in_linear_not_after_the_encode() {
    let mut env = Environment3D::default();
    env.adjustment_enabled = true;
    env.adjustment_brightness = 2.0;

    // 0.25 linear doubled is 0.5 linear. Encoded that is ~0.7354; doubling the
    // *encoded* 0.25 instead gives ~1.0744, which clamps to 1.0. The two
    // orderings are nowhere near each other.
    let out = post(Color::new(0.25, 0.25, 0.25, 1.0), &env);

    let correct = linear_to_srgb_channel(0.5);
    let if_applied_after_encode = (2.0 * linear_to_srgb_channel(0.25)).clamp(0.0, 1.0);
    assert!(
        close(out.r, correct),
        "expected srgb(0.25 * 2) = {correct}, got {}",
        out.r
    );
    assert!(
        !close(correct, if_applied_after_encode),
        "fixture is not discriminating: both orderings agree at {correct}"
    );
}

#[test]
fn contrast_zero_collapses_to_the_encoded_midpoint() {
    let mut env = Environment3D::default();
    env.adjustment_enabled = true;
    env.adjustment_contrast = 0.0;

    // 🔑 Contrast pivots on 0.5 *in the sRGB encoding*, so zero contrast lands
    // every channel on exactly 0.5 whatever went in. Pivoting in linear space
    // instead would land on srgb(0.5) ~ 0.7354 — a visibly different grey, and
    // the reason this assertion is worth more than a "contrast changes the
    // image" check.
    for input in [
        Color::new(0.0, 0.0, 0.0, 1.0),
        Color::new(1.0, 1.0, 1.0, 1.0),
        Color::new(0.1, 0.5, 0.9, 1.0),
    ] {
        let out = post(input, &env);
        assert!(
            close(out.r, 0.5) && close(out.g, 0.5) && close(out.b, 0.5),
            "contrast 0 must collapse to the 0.5 pivot, got ({}, {}, {}) for \
             input ({}, {}, {})",
            out.r,
            out.g,
            out.b,
            input.r,
            input.g,
            input.b
        );
    }

    let pivot_if_linear = linear_to_srgb_channel(0.5);
    assert!(
        !close(0.5, pivot_if_linear),
        "fixture is not discriminating: the two pivots agree"
    );
}

#[test]
fn brightness_runs_before_contrast() {
    // Order matters: brightness is linear and contrast is not, so swapping them
    // changes the result. Brightness 2.0 takes 0.25 linear to 0.5 linear,
    // encoded ~0.7354; contrast 0.5 then pulls it halfway to the pivot.
    let mut env = Environment3D::default();
    env.adjustment_enabled = true;
    env.adjustment_brightness = 2.0;
    env.adjustment_contrast = 0.5;

    let out = post(Color::new(0.25, 0.25, 0.25, 1.0), &env);
    let expected = 0.5 + (linear_to_srgb_channel(0.5) - 0.5) * 0.5;
    assert!(
        close(out.r, expected),
        "expected {expected} from brightness-then-contrast, got {}",
        out.r
    );
}

#[test]
fn the_scene_properties_reach_the_render_stage() {
    // `world_environment_from_scene` builds the Environment3D by handing the
    // sub-resource's property bag to `from_properties`, so this is the same
    // entry point a `.tscn` WorldEnvironment goes through — the half that was
    // missing, since ClassDB registered these names but nothing read them back.
    let props: Vec<(&str, gdvariant::Variant)> = vec![
        ("adjustment_enabled", gdvariant::Variant::Bool(true)),
        ("adjustment_saturation", gdvariant::Variant::Float(0.0)),
    ];
    let env = Environment3D::from_properties(props.iter().map(|(k, v)| (*k, v)));

    assert!(env.adjustment_enabled, "the .tscn value must be read back");
    assert_eq!(env.adjustment_saturation, 0.0);
    assert_eq!(
        env.adjustment_brightness, 1.0,
        "an unmentioned knob keeps its Godot default"
    );

    // And it reaches the pixels: an authored saturation of 0 greys the frame.
    let out = post(Color::new(0.0, 0.0, 1.0, 1.0), &env);
    assert!(close(out.r, 1.0 / 3.0) && close(out.g, out.r) && close(out.b, out.r));
}
