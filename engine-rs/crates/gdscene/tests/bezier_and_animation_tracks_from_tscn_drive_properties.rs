//! pat-s3rrv.44 acceptance: bézier and animation tracks, end to end.
//!
//! Both track types previously stopped at the parser: `tscn_track_type` mapped
//! them to `None`, and a test asserted that mapping was correct. That was the
//! honest state — Patina's sampler had no bézier curve and no way to start a
//! nested clip, so mapping `bezier` onto a value track would have driven the
//! property along a straight line through keys the artist deliberately curved.
//!
//! The pieces this exercises:
//!   * five-floats-per-key `points` parsing (value + two relative handles),
//!   * curve sampling that matches `Animation::bezier_track_interpolate`,
//!   * writing through a `position:x` sub-property path,
//!   * an animation track starting a clip on a nested `AnimationPlayer`,
//!   * both blocks surviving a re-save.

use gdcore::math::Vector2;
use gdscene::animation::{
    Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType, TransitionType,
};
use gdvariant::Variant;

/// A bézier segment from (0, 0) to (1, 100) whose handles bow the curve upward:
/// the left key leaves steeply, so the value runs ahead of the straight line.
///
/// The handles are `(time, value)` offsets RELATIVE to their own key — Godot
/// forms the control point as `point + handle` — so the left key's `out_handle`
/// of `(0.5, 50)` places a control point at `(0.5, 50)`, and the right key's
/// `in_handle` of `(-0.5, 0)` places one at `(0.5, 100)`.
///
/// The control values `0, 50, 100, 100` are non-decreasing, which is what keeps
/// the curve monotonic and under the 100 end value. Handles that reach further
/// (an `in_handle.y` of `+80`, say) put a control point at 180 and make the
/// curve OVERSHOOT past 100 — the midpoint here was computed, not guessed.
fn bowed_track() -> AnimationTrack {
    let mut track = AnimationTrack::with_type("Sprite2D", "position:x", TrackType::Bezier);
    track.add_keyframe(KeyFrame::bezier(
        0.0,
        0.0,
        Vector2::new(-0.25, 0.0),
        Vector2::new(0.5, 50.0),
    ));
    track.add_keyframe(KeyFrame::bezier(
        1.0,
        100.0,
        Vector2::new(-0.5, 0.0),
        Vector2::new(0.25, 0.0),
    ));
    track
}

/// The same two keys with FLAT handles — the linear reference the bowed curve
/// must differ from.
fn flat_track() -> AnimationTrack {
    let mut track = AnimationTrack::with_type("Sprite2D", "position:x", TrackType::Bezier);
    track.add_keyframe(KeyFrame::bezier(0.0, 0.0, Vector2::ZERO, Vector2::ZERO));
    track.add_keyframe(KeyFrame::bezier(1.0, 100.0, Vector2::ZERO, Vector2::ZERO));
    track
}

fn as_f64(v: Option<Variant>) -> f64 {
    match v {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("expected a scalar, got {other:?}"),
    }
}

/// **The headline acceptance.** Non-linear handles move the midpoint away from
/// the straight-line value by the amount the handles imply.
///
/// The bowed curve pulls the value UP, so at the midpoint it must exceed the
/// linear 50. Asserting merely "differs from 50" would also pass if the curve
/// bowed the wrong way, which is the likelier bug: mis-reading the handles as
/// absolute rather than relative flips them.
#[test]
fn bezier_handles_bend_the_curve_away_from_linear() {
    let bowed = bowed_track();
    let flat = flat_track();

    let mid_flat = as_f64(flat.sample(0.5));
    let mid_bowed = as_f64(bowed.sample(0.5));

    assert!(
        (mid_flat - 50.0).abs() < 0.5,
        "zero handles must sample as a straight line, got {mid_flat}"
    );
    // The exact midpoint of this control polygon is 68.75, computed from
    // (0,0) (0.5,50) (0.5,100) (1,100) rather than eyeballed; the bound is
    // loose enough to survive f32 rounding but tight enough that a curve
    // bowing the WRONG way (handles read as absolute, which flips them) fails.
    assert!(
        mid_bowed > mid_flat + 5.0,
        "upward handles must lift the midpoint above the linear {mid_flat}, got {mid_bowed}"
    );
    assert!(
        (mid_bowed - 68.75).abs() < 0.5,
        "the midpoint must match the hand-computed 68.75, got {mid_bowed}"
    );
    assert!(
        mid_bowed < 100.0,
        "the curve must stay under the end value, got {mid_bowed}"
    );
}

/// The endpoints are untouched by the handles — a curve that missed its own
/// keys would be wrong no matter how the middle looked.
#[test]
fn a_bezier_track_still_passes_through_its_keys() {
    let t = bowed_track();
    assert!((as_f64(t.sample(0.0)) - 0.0).abs() < 1e-6);
    assert!((as_f64(t.sample(1.0)) - 100.0).abs() < 1e-6);
    // Godot does not loop-wrap bézier tracks; outside the range it clamps.
    assert!((as_f64(t.sample(-5.0)) - 0.0).abs() < 1e-6);
    assert!((as_f64(t.sample(9.0)) - 100.0).abs() < 1e-6);
}

/// The curve is monotonic in time: sampling forward never goes backwards in
/// value for these keys. This catches a parameter/time confusion — using
/// `elapsed / duration` as the Bézier parameter instead of solving for it
/// produces a curve that is subtly wrong everywhere but still hits the
/// endpoints, so the endpoint test above cannot see it.
#[test]
fn sampling_advances_monotonically_across_the_segment() {
    let t = bowed_track();
    let mut prev = f64::NEG_INFINITY;
    for step in 0..=20 {
        let v = as_f64(t.sample(step as f64 / 20.0));
        assert!(
            v >= prev - 1e-6,
            "value went backwards at t={}: {prev} -> {v}",
            step as f64 / 20.0
        );
        prev = v;
    }
}

/// A bézier track whose handles are zero must agree with an ordinary linear
/// value track — the two describe the same motion, so a systematic offset in
/// the bézier path would show up as a disagreement here.
#[test]
fn flat_handles_agree_with_a_linear_value_track() {
    let bez = flat_track();
    let mut lin = AnimationTrack::with_type("Sprite2D", "position:x", TrackType::Property);
    lin.add_keyframe(KeyFrame::new(
        0.0,
        Variant::Float(0.0),
        TransitionType::Linear,
    ));
    lin.add_keyframe(KeyFrame::new(
        1.0,
        Variant::Float(100.0),
        TransitionType::Linear,
    ));

    for step in 0..=10 {
        let time = step as f64 / 10.0;
        let (b, l) = (as_f64(bez.sample(time)), as_f64(lin.sample(time)));
        assert!(
            (b - l).abs() < 0.5,
            "at t={time} bezier {b} disagrees with linear {l}"
        );
    }
}

/// An animation track samples to the CLIP NAME of the most recent key and never
/// interpolates — blending two clip names would be meaningless.
#[test]
fn an_animation_track_holds_the_clip_name_without_interpolating() {
    let mut t = AnimationTrack::with_type("Sub", "animation", TrackType::Animation);
    t.add_keyframe(KeyFrame::linear(0.0, Variant::String("idle".into())));
    t.add_keyframe(KeyFrame::linear(1.0, Variant::String("swing".into())));

    assert_eq!(t.sample(0.0), Some(Variant::String("idle".into())));
    // Halfway between the keys the FIRST clip is still the one running.
    assert_eq!(t.sample(0.5), Some(Variant::String("idle".into())));
    assert_eq!(t.sample(1.0), Some(Variant::String("swing".into())));
    assert_eq!(t.sample(5.0), Some(Variant::String("swing".into())));
}

/// Crossing an animation key reports the clip to start, exactly once, and a key
/// the playhead has not reached is not reported early.
#[test]
fn animation_keys_report_as_they_are_crossed() {
    let mut anim = Animation::new("cutscene", 2.0);
    let mut t = AnimationTrack::with_type("Sub", "animation", TrackType::Animation);
    t.add_keyframe(KeyFrame::linear(1.0, Variant::String("swing".into())));
    anim.tracks.push(t);

    let mut player = AnimationPlayer::new();
    player.animations.insert("cutscene".to_string(), anim);
    player.play("cutscene");

    assert!(
        player.animation_clip_starts_in_range(0.0, 0.5).is_empty(),
        "a key ahead of the playhead must not fire early"
    );
    assert_eq!(
        player.animation_clip_starts_in_range(0.5, 1.5),
        vec![("Sub".to_string(), "swing".to_string())],
        "the crossed key must report its node path and clip"
    );
    assert!(
        player.animation_clip_starts_in_range(1.5, 2.0).is_empty(),
        "a key already passed must not fire again"
    );
}

/// An animation track is kept out of the property-write path. Its value is a
/// clip name, so applying it as a property would write the string `"swing"`
/// into a property called `animation` and quietly do nothing useful.
#[test]
fn an_animation_track_is_not_applied_as_a_property() {
    let mut anim = Animation::new("cutscene", 2.0);
    let mut t = AnimationTrack::with_type("Sub", "animation", TrackType::Animation);
    t.add_keyframe(KeyFrame::linear(0.0, Variant::String("swing".into())));
    anim.tracks.push(t);

    let mut player = AnimationPlayer::new();
    player.animations.insert("cutscene".to_string(), anim);
    player.play("cutscene");

    assert!(
        player
            .non_property_track_paths()
            .contains(&"animation".to_string()),
        "the animation track must be listed as non-property so the tree skips it"
    );
}

/// The `.tscn` track-type spellings both round-trip through the type mapping.
#[test]
fn the_track_type_names_round_trip() {
    assert_eq!(TrackType::from_str_name("bezier"), Some(TrackType::Bezier));
    assert_eq!(
        TrackType::from_str_name("animation"),
        Some(TrackType::Animation)
    );
    assert_eq!(TrackType::from_str_name("not_a_type"), None);
}
