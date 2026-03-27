//! Tests for animation blending and editor preview support.
//!
//! Covers: crossfade transitions, manual blend preview, standalone
//! `blend_animations`, and edge cases (missing animations, zero-duration
//! blends, incompatible track types).

use gdscene::animation::{
    blend_animations, Animation, AnimationPlayer, AnimationTrack, BlendState, KeyFrame, LoopMode,
    TransitionType,
};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_anim(name: &str, length: f64, prop: &str, start: f64, end: f64) -> Animation {
    let mut anim = Animation::new(name, length);
    let mut track = AnimationTrack::new(prop);
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(start)));
    track.add_keyframe(KeyFrame::linear(length, Variant::Float(end)));
    anim.tracks.push(track);
    anim
}

fn float_val(values: &[(String, Variant)], prop: &str) -> f64 {
    for (p, v) in values {
        if p == prop {
            if let Variant::Float(f) = v {
                return *f;
            }
        }
    }
    panic!("property '{}' not found or not Float", prop);
}

// ---------------------------------------------------------------------------
// BlendState unit tests
// ---------------------------------------------------------------------------

#[test]
fn blend_state_factor_zero_duration() {
    let bs = BlendState {
        from_animation: "idle".into(),
        from_position: 0.0,
        blend_duration: 0.0,
        blend_elapsed: 0.0,
    };
    assert_eq!(bs.blend_factor(), 1.0);
    assert!(bs.is_complete());
}

#[test]
fn blend_state_factor_midway() {
    let bs = BlendState {
        from_animation: "idle".into(),
        from_position: 0.0,
        blend_duration: 1.0,
        blend_elapsed: 0.5,
    };
    assert!((bs.blend_factor() - 0.5).abs() < 1e-6);
    assert!(!bs.is_complete());
}

#[test]
fn blend_state_factor_clamped() {
    let bs = BlendState {
        from_animation: "idle".into(),
        from_position: 0.0,
        blend_duration: 1.0,
        blend_elapsed: 2.0,
    };
    assert_eq!(bs.blend_factor(), 1.0);
    assert!(bs.is_complete());
}

// ---------------------------------------------------------------------------
// AnimationPlayer crossfade
// ---------------------------------------------------------------------------

#[test]
fn crossfade_basic_blend() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 2.0, "x", 0.0, 10.0));
    player.add_animation(make_anim("walk", 2.0, "x", 100.0, 200.0));

    // Start playing idle, advance to position 1.0
    player.play("idle");
    player.advance(1.0);
    assert!((player.position() - 1.0).abs() < 1e-10);

    // Crossfade to walk over 0.5 seconds
    player.crossfade_to("walk", 0.5);
    assert_eq!(player.current_animation(), Some("walk"));
    assert!(player.blend_state().is_some());

    // At blend start (elapsed=0), values should be 100% idle
    let vals = player.get_current_values();
    // from_position=1.0 → idle samples 5.0, walk at 0.0 → 100.0, factor=0.0 → 5.0
    let x = float_val(&vals, "x");
    assert!((x - 5.0).abs() < 1e-6, "expected ~5.0 at blend start, got {x}");

    // Advance 0.25s (half the blend)
    player.advance(0.25);
    let vals = player.get_current_values();
    // factor=0.5, from_pos=1.25 → idle=6.25, walk at 0.25 → 112.5
    // blended = lerp(6.25, 112.5, 0.5) = 59.375
    let x = float_val(&vals, "x");
    assert!(
        (x - 59.375).abs() < 1.0,
        "expected ~59.375 at mid-blend, got {x}"
    );

    // Advance past blend end
    player.advance(0.5);
    assert!(player.blend_state().is_none(), "blend should be cleared");
}

#[test]
fn crossfade_to_nonexistent_animation_is_noop() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));
    player.play("idle");
    player.crossfade_to("nonexistent", 0.5);
    // Should still be playing idle
    assert_eq!(player.current_animation(), Some("idle"));
    assert!(player.blend_state().is_none());
}

#[test]
fn crossfade_when_nothing_playing() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("walk", 1.0, "x", 0.0, 10.0));
    // No current animation — crossfade should start walk without a blend
    player.crossfade_to("walk", 0.5);
    assert_eq!(player.current_animation(), Some("walk"));
    // No from animation, so no blend state
    assert!(player.blend_state().is_none());
}

#[test]
fn crossfade_zero_duration_snaps_immediately() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));
    player.add_animation(make_anim("walk", 1.0, "x", 100.0, 200.0));
    player.play("idle");
    player.advance(0.5);

    player.crossfade_to("walk", 0.0);
    // With zero duration, blend_factor() = 1.0 → fully "to"
    let vals = player.get_current_values();
    let x = float_val(&vals, "x");
    assert!((x - 100.0).abs() < 1e-6, "zero-duration blend should snap, got {x}");
}

// ---------------------------------------------------------------------------
// Editor blend preview
// ---------------------------------------------------------------------------

#[test]
fn set_blend_preview_at_zero() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));
    player.add_animation(make_anim("walk", 1.0, "x", 100.0, 200.0));
    player.play("idle");
    player.advance(0.5); // idle position = 0.5

    player.set_blend_preview("idle", "walk", 0.0);
    let vals = player.get_current_values();
    let x = float_val(&vals, "x");
    // weight=0.0 → fully idle at position 0.5 → x=5.0
    assert!((x - 5.0).abs() < 1e-6, "blend preview at 0.0 failed, got {x}");
}

#[test]
fn set_blend_preview_at_one() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));
    player.add_animation(make_anim("walk", 1.0, "x", 100.0, 200.0));
    player.play("idle");
    player.advance(0.5);

    player.set_blend_preview("idle", "walk", 1.0);
    let vals = player.get_current_values();
    let x = float_val(&vals, "x");
    // weight=1.0 → fully walk at position 0.5 → x=150.0
    assert!((x - 150.0).abs() < 1e-6, "blend preview at 1.0 failed, got {x}");
}

#[test]
fn set_blend_preview_at_half() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));
    player.add_animation(make_anim("walk", 1.0, "x", 100.0, 200.0));
    player.play("idle");
    player.advance(0.5);

    player.set_blend_preview("idle", "walk", 0.5);
    let vals = player.get_current_values();
    let x = float_val(&vals, "x");
    // from=5.0 (idle@0.5), to=150.0 (walk@0.5), factor=0.5 → 77.5
    assert!((x - 77.5).abs() < 1e-6, "blend preview at 0.5 failed, got {x}");
}

#[test]
fn set_blend_preview_clamps_weight() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));
    player.add_animation(make_anim("walk", 1.0, "x", 100.0, 200.0));
    player.play("idle");

    player.set_blend_preview("idle", "walk", 2.0);
    let bs = player.blend_state().unwrap();
    assert_eq!(bs.blend_factor(), 1.0);

    player.set_blend_preview("idle", "walk", -1.0);
    let bs = player.blend_state().unwrap();
    assert!((bs.blend_factor()).abs() < 1e-6);
}

#[test]
fn set_blend_preview_nonexistent_from_is_noop() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("walk", 1.0, "x", 0.0, 10.0));
    player.set_blend_preview("ghost", "walk", 0.5);
    assert!(player.blend_state().is_none());
}

#[test]
fn set_blend_preview_nonexistent_to_is_noop() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));
    player.set_blend_preview("idle", "ghost", 0.5);
    assert!(player.blend_state().is_none());
}

#[test]
fn clear_blend_removes_state() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));
    player.add_animation(make_anim("walk", 1.0, "x", 100.0, 200.0));
    player.play("idle");
    player.crossfade_to("walk", 1.0);
    assert!(player.blend_state().is_some());
    player.clear_blend();
    assert!(player.blend_state().is_none());
}

// ---------------------------------------------------------------------------
// Standalone blend_animations
// ---------------------------------------------------------------------------

#[test]
fn blend_animations_factor_zero() {
    let a = make_anim("a", 1.0, "x", 0.0, 10.0);
    let b = make_anim("b", 1.0, "x", 100.0, 200.0);
    let vals = blend_animations(&a, &b, 0.5, 0.5, 0.0);
    let x = float_val(&vals, "x");
    assert!((x - 5.0).abs() < 1e-6, "factor 0.0 should be fully A, got {x}");
}

#[test]
fn blend_animations_factor_one() {
    let a = make_anim("a", 1.0, "x", 0.0, 10.0);
    let b = make_anim("b", 1.0, "x", 100.0, 200.0);
    let vals = blend_animations(&a, &b, 0.5, 0.5, 1.0);
    let x = float_val(&vals, "x");
    assert!(
        (x - 150.0).abs() < 1e-6,
        "factor 1.0 should be fully B, got {x}"
    );
}

#[test]
fn blend_animations_factor_half() {
    let a = make_anim("a", 1.0, "x", 0.0, 10.0);
    let b = make_anim("b", 1.0, "x", 100.0, 200.0);
    let vals = blend_animations(&a, &b, 0.5, 0.5, 0.5);
    let x = float_val(&vals, "x");
    // A@0.5 = 5.0, B@0.5 = 150.0, lerp(5, 150, 0.5) = 77.5
    assert!((x - 77.5).abs() < 1e-6, "factor 0.5 blend failed, got {x}");
}

#[test]
fn blend_animations_disjoint_tracks() {
    let a = make_anim("a", 1.0, "x", 0.0, 10.0);
    let b = make_anim("b", 1.0, "y", 100.0, 200.0);

    let vals = blend_animations(&a, &b, 0.5, 0.5, 0.5);
    // Both tracks should appear since they're disjoint
    assert_eq!(vals.len(), 2);
    let x = float_val(&vals, "x");
    let y = float_val(&vals, "y");
    assert!((x - 5.0).abs() < 1e-6, "disjoint x wrong: {x}");
    assert!((y - 150.0).abs() < 1e-6, "disjoint y wrong: {y}");
}

#[test]
fn blend_animations_multi_track() {
    let mut a = Animation::new("a", 1.0);
    let mut tx = AnimationTrack::new("x");
    tx.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    tx.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
    a.tracks.push(tx);
    let mut ty = AnimationTrack::new("y");
    ty.add_keyframe(KeyFrame::linear(0.0, Variant::Float(20.0)));
    ty.add_keyframe(KeyFrame::linear(1.0, Variant::Float(40.0)));
    a.tracks.push(ty);

    let mut b = Animation::new("b", 1.0);
    let mut tx2 = AnimationTrack::new("x");
    tx2.add_keyframe(KeyFrame::linear(0.0, Variant::Float(100.0)));
    tx2.add_keyframe(KeyFrame::linear(1.0, Variant::Float(200.0)));
    b.tracks.push(tx2);
    let mut ty2 = AnimationTrack::new("y");
    ty2.add_keyframe(KeyFrame::linear(0.0, Variant::Float(300.0)));
    ty2.add_keyframe(KeyFrame::linear(1.0, Variant::Float(400.0)));
    b.tracks.push(ty2);

    let vals = blend_animations(&a, &b, 0.5, 0.5, 0.5);
    // A: x=5, y=30; B: x=150, y=350; blend: x=77.5, y=190
    let x = float_val(&vals, "x");
    let y = float_val(&vals, "y");
    assert!((x - 77.5).abs() < 1e-6, "multi-track x: {x}");
    assert!((y - 190.0).abs() < 1e-6, "multi-track y: {y}");
}

#[test]
fn blend_animations_different_lengths() {
    let a = make_anim("a", 1.0, "x", 0.0, 10.0);
    let b = make_anim("b", 2.0, "x", 100.0, 200.0);
    // Sample A at 0.5 (x=5), B at 1.0 (x=150), blend at 0.5 → 77.5
    let vals = blend_animations(&a, &b, 0.5, 1.0, 0.5);
    let x = float_val(&vals, "x");
    assert!((x - 77.5).abs() < 1e-6);
}

#[test]
fn blend_animations_clamps_factor() {
    let a = make_anim("a", 1.0, "x", 0.0, 10.0);
    let b = make_anim("b", 1.0, "x", 100.0, 200.0);

    let vals = blend_animations(&a, &b, 0.5, 0.5, -0.5);
    let x = float_val(&vals, "x");
    assert!((x - 5.0).abs() < 1e-6, "negative factor should clamp to 0");

    let vals = blend_animations(&a, &b, 0.5, 0.5, 1.5);
    let x = float_val(&vals, "x");
    assert!((x - 150.0).abs() < 1e-6, "factor >1 should clamp to 1");
}

// ---------------------------------------------------------------------------
// Crossfade advances from_position
// ---------------------------------------------------------------------------

#[test]
fn crossfade_advances_from_position() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 2.0, "x", 0.0, 20.0));
    player.add_animation(make_anim("walk", 2.0, "x", 100.0, 200.0));

    player.play("idle");
    player.advance(0.5); // idle at 0.5
    player.crossfade_to("walk", 1.0);

    let from_pos_before = player.blend_state().unwrap().from_position;
    assert!((from_pos_before - 0.5).abs() < 1e-10);

    player.advance(0.3);
    let from_pos_after = player.blend_state().unwrap().from_position;
    assert!(
        (from_pos_after - 0.8).abs() < 1e-10,
        "from_position should advance, got {from_pos_after}"
    );
}

// ---------------------------------------------------------------------------
// Blend completes and clears
// ---------------------------------------------------------------------------

#[test]
fn crossfade_completes_and_clears() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 2.0, "x", 0.0, 20.0));
    player.add_animation(make_anim("walk", 2.0, "x", 100.0, 200.0));

    player.play("idle");
    player.crossfade_to("walk", 0.5);
    player.advance(0.6); // > 0.5, blend should be done

    assert!(player.blend_state().is_none());
    assert_eq!(player.current_animation(), Some("walk"));
    assert!(player.playing);
}

// ---------------------------------------------------------------------------
// Looping animation during crossfade
// ---------------------------------------------------------------------------

#[test]
fn crossfade_with_looping_target() {
    let mut player = AnimationPlayer::new();
    player.add_animation(make_anim("idle", 1.0, "x", 0.0, 10.0));

    let mut walk = make_anim("walk", 0.5, "x", 100.0, 200.0);
    walk.loop_mode = LoopMode::Linear;
    player.add_animation(walk);

    player.play("idle");
    player.advance(0.5);
    player.crossfade_to("walk", 1.0);

    // Advance past walk's length — it should loop
    player.advance(0.7);
    assert!(player.playing);
    // Walk should have looped: position was 0.7, length 0.5, so position wraps to 0.2
    assert!(
        player.position() < 0.5,
        "walk should have looped, pos={}",
        player.position()
    );
}
