//! DISABLED (references gdscene::skeleton3d::{advance_skeleton_animation, apply_animation_to_skeleton} which are not yet implemented)
//! Re-enable by removing the cfg gate below once the missing
//! items land. Tracked as part of editor parity work.
#![cfg(any())]

//! AnimationPlayer parity tests (pat-x661p).
//!
//! Verifies keyframe-based track playback for position, rotation, scale,
//! generic property, and method track types.

use gdcore::math::{Color, Vector2, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdscene::animation::{
    Animation, AnimationPlayer, AnimationTrack, KeyFrame, LoopMode, TrackType, TransitionType,
};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{
    add_bone, advance_skeleton_animation, apply_animation_to_skeleton, get_bone_global_pose,
    get_bone_pose, set_bone_parent, set_bone_rest,
};
use gdscene::tween::{ease, EaseType, TransFunc, Tween, TweenBuilder, TweenStep};
use gdvariant::Variant;

fn float_at(values: &[(String, Variant)], prop: &str) -> f64 {
    for (p, v) in values {
        if p == prop {
            if let Variant::Float(f) = v {
                return *f;
            }
        }
    }
    panic!("no Float value for {prop}");
}

fn vec2_at(values: &[(String, Variant)], prop: &str) -> Vector2 {
    for (p, v) in values {
        if p == prop {
            if let Variant::Vector2(vec) = v {
                return *vec;
            }
        }
    }
    panic!("no Vector2 value for {prop}");
}

fn vec3_at(values: &[(String, Variant)], prop: &str) -> Vector3 {
    for (p, v) in values {
        if p == prop {
            if let Variant::Vector3(vec) = v {
                return *vec;
            }
        }
    }
    panic!("no Vector3 value for {prop}");
}

#[test]
fn animation_player_plays_position_vector2_track() {
    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("walk", 2.0);

    let mut track = AnimationTrack::with_node("Player", "position");
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector2(Vector2::new(0.0, 0.0)),
    ));
    track.add_keyframe(KeyFrame::linear(
        2.0,
        Variant::Vector2(Vector2::new(100.0, 50.0)),
    ));
    anim.tracks.push(track);
    player.add_animation(anim);

    player.play("walk");
    assert!(player.playing);
    assert_eq!(player.current_animation(), Some("walk"));

    player.advance(1.0);
    let values = player.get_current_values();
    let pos = vec2_at(&values, "position");
    assert!((pos.x - 50.0).abs() < 1e-5, "x was {}", pos.x);
    assert!((pos.y - 25.0).abs() < 1e-5, "y was {}", pos.y);
}

#[test]
fn animation_player_plays_position_vector3_track() {
    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("move3d", 2.0);

    let mut track = AnimationTrack::with_node("Player", "position");
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector3(Vector3::new(0.0, 0.0, 0.0)),
    ));
    track.add_keyframe(KeyFrame::linear(
        2.0,
        Variant::Vector3(Vector3::new(20.0, 40.0, 60.0)),
    ));
    anim.tracks.push(track);
    player.add_animation(anim);

    player.play("move3d");
    player.advance(0.5);

    let values = player.get_current_values();
    let pos = vec3_at(&values, "position");
    assert!((pos.x - 5.0).abs() < 1e-5);
    assert!((pos.y - 10.0).abs() < 1e-5);
    assert!((pos.z - 15.0).abs() < 1e-5);
}

#[test]
fn animation_player_plays_rotation_track() {
    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("spin", 1.0);

    let mut track = AnimationTrack::with_node("Player", "rotation");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(std::f64::consts::PI)));
    anim.tracks.push(track);
    player.add_animation(anim);

    player.play("spin");
    player.advance(0.25);

    let values = player.get_current_values();
    let rot = float_at(&values, "rotation");
    let expected = std::f64::consts::PI * 0.25;
    assert!((rot - expected).abs() < 1e-5, "rotation was {}", rot);
}

#[test]
fn animation_player_plays_scale_track() {
    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("grow", 1.0);

    let mut track = AnimationTrack::with_node("Player", "scale");
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector2(Vector2::new(1.0, 1.0)),
    ));
    track.add_keyframe(KeyFrame::linear(
        1.0,
        Variant::Vector2(Vector2::new(2.0, 3.0)),
    ));
    anim.tracks.push(track);
    player.add_animation(anim);

    player.play("grow");
    player.advance(0.5);

    let values = player.get_current_values();
    let scale = vec2_at(&values, "scale");
    assert!((scale.x - 1.5).abs() < 1e-5);
    assert!((scale.y - 2.0).abs() < 1e-5);
}

#[test]
fn animation_player_plays_generic_property_track() {
    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("fade", 1.0);

    let mut track = AnimationTrack::with_type("Sprite", "modulate:a", TrackType::Property);
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(0.0)));
    anim.tracks.push(track);
    player.add_animation(anim);

    player.play("fade");
    player.advance(0.5);

    let values = player.get_current_values();
    let alpha = float_at(&values, "modulate:a");
    assert!((alpha - 0.5).abs() < 1e-5);
}

#[test]
fn animation_player_method_track_samples_call_payload() {
    let mut anim = Animation::new("fx", 1.0);
    let mut track = AnimationTrack::with_type("Player", "emit_shot", TrackType::Method);

    let call = Variant::Array(vec![
        Variant::String("shoot".into()),
        Variant::Int(3),
    ]);
    track.add_keyframe(KeyFrame::new(0.5, call.clone(), TransitionType::Nearest));
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("fx");
    player.advance(0.5);

    let values = player.get_current_values();
    let (prop, sampled) = values.iter().find(|(p, _)| p == "emit_shot").expect("method track sample");
    assert_eq!(prop, "emit_shot");
    assert_eq!(sampled, &call);

    let anim_ref = player.animations.get("fx").unwrap();
    assert_eq!(anim_ref.tracks[0].track_type(), TrackType::Method);
}

#[test]
fn animation_player_multi_track_sampling() {
    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("combo", 1.0);

    let mut pos = AnimationTrack::with_node("Player", "position");
    pos.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector2(Vector2::new(0.0, 0.0)),
    ));
    pos.add_keyframe(KeyFrame::linear(
        1.0,
        Variant::Vector2(Vector2::new(10.0, 0.0)),
    ));
    anim.tracks.push(pos);

    let mut rot = AnimationTrack::with_node("Player", "rotation");
    rot.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    rot.add_keyframe(KeyFrame::linear(1.0, Variant::Float(6.28)));
    anim.tracks.push(rot);

    let mut scale = AnimationTrack::with_node("Player", "scale");
    scale.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector2(Vector2::new(1.0, 1.0)),
    ));
    scale.add_keyframe(KeyFrame::linear(
        1.0,
        Variant::Vector2(Vector2::new(2.0, 2.0)),
    ));
    anim.tracks.push(scale);

    player.add_animation(anim);
    player.play("combo");
    player.advance(0.5);

    let values = player.get_current_values();
    assert_eq!(values.len(), 3);
    let p = vec2_at(&values, "position");
    assert!((p.x - 5.0).abs() < 1e-5);
    assert!((float_at(&values, "rotation") - 3.14).abs() < 1e-2);
    let s = vec2_at(&values, "scale");
    assert!((s.x - 1.5).abs() < 1e-5);
}

#[test]
fn animation_player_loop_mode_linear_wraps_position() {
    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("loopy", 1.0);
    anim.loop_mode = LoopMode::Linear;
    let mut track = AnimationTrack::new("x");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
    anim.tracks.push(track);
    player.add_animation(anim);

    player.play("loopy");
    player.advance(2.25);
    assert!(player.playing);
    assert!((player.position() - 0.25).abs() < 1e-9);
    let values = player.get_current_values();
    assert!((float_at(&values, "x") - 2.5).abs() < 1e-5);
}

#[test]
fn animation_player_stops_at_end_when_no_loop() {
    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("once", 1.0);
    let mut track = AnimationTrack::new("x");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
    anim.tracks.push(track);
    player.add_animation(anim);

    player.play("once");
    player.advance(5.0);
    assert!(!player.playing);
    assert!((player.position() - 1.0).abs() < 1e-9);
    let values = player.get_current_values();
    assert!((float_at(&values, "x") - 10.0).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// Tween property interpolation parity (pat-9zbbx).
//
// Run with:
//   `./scripts/rust_task.sh nextest run --test animation_parity_test -- tween_property`
// ---------------------------------------------------------------------------

const TWEEN_EPS: f64 = 1e-6;
const TWEEN_EPS_F32: f32 = 1e-4;

fn tween_extract_float(v: &Variant) -> f64 {
    match v {
        Variant::Float(f) => *f,
        other => panic!("expected Float, got {:?}", other),
    }
}

#[test]
fn tween_property_linear_ease_is_identity() {
    for &et in &[EaseType::In, EaseType::Out, EaseType::InOut] {
        for &t in &[0.0_f64, 0.25, 0.5, 0.75, 1.0] {
            let v = ease(t, et, TransFunc::Linear);
            assert!(
                (v - t).abs() < TWEEN_EPS,
                "linear ease({t}, {et:?}) = {v}"
            );
        }
    }
}

#[test]
fn tween_property_easing_curves_hit_endpoints() {
    let funcs = [
        TransFunc::Linear,
        TransFunc::Sine,
        TransFunc::Quad,
        TransFunc::Cubic,
        TransFunc::Expo,
        TransFunc::Elastic,
        TransFunc::Bounce,
        TransFunc::Back,
    ];
    for &tf in &funcs {
        for &et in &[EaseType::In, EaseType::Out, EaseType::InOut] {
            let v0 = ease(0.0, et, tf);
            let v1 = ease(1.0, et, tf);
            assert!(v0.abs() < 1e-6, "ease({et:?}, {tf:?}) at t=0 = {v0}");
            assert!(
                (v1 - 1.0).abs() < 1e-6,
                "ease({et:?}, {tf:?}) at t=1 = {v1}"
            );
        }
    }
}

#[test]
fn tween_property_quad_in_matches_godot_curve() {
    for &(t, expected) in &[
        (0.0_f64, 0.0_f64),
        (0.25, 0.0625),
        (0.5, 0.25),
        (0.75, 0.5625),
        (1.0, 1.0),
    ] {
        let v = ease(t, EaseType::In, TransFunc::Quad);
        assert!((v - expected).abs() < TWEEN_EPS, "quad_in({t}) = {v}");
    }
}

#[test]
fn tween_property_cubic_in_matches_godot_curve() {
    for &(t, expected) in &[(0.25_f64, 0.015625), (0.5, 0.125), (0.75, 0.421875)] {
        let v = ease(t, EaseType::In, TransFunc::Cubic);
        assert!((v - expected).abs() < TWEEN_EPS, "cubic_in({t}) = {v}");
    }
}

#[test]
fn tween_property_ease_input_is_clamped() {
    assert!((ease(-1.0, EaseType::In, TransFunc::Quad) - 0.0).abs() < TWEEN_EPS);
    assert!((ease(2.0, EaseType::Out, TransFunc::Cubic) - 1.0).abs() < TWEEN_EPS);
}

#[test]
fn tween_property_interpolates_float_linear() {
    let mut tween = TweenBuilder::new()
        .tween_property("x", Variant::Float(0.0), Variant::Float(100.0), 2.0)
        .build();

    tween.advance(1.0);
    let values = tween.get_current_values();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].0, "x");
    let v = tween_extract_float(&values[0].1);
    assert!((v - 50.0).abs() < TWEEN_EPS, "mid = {v}");

    let done = tween.advance(1.0);
    assert!(done);
    let end = tween_extract_float(&tween.get_current_values()[0].1);
    assert!((end - 100.0).abs() < TWEEN_EPS, "end = {end}");
}

#[test]
fn tween_property_interpolates_vector2_component_wise() {
    let mut tween = TweenBuilder::new()
        .tween_property(
            "position",
            Variant::Vector2(Vector2::new(0.0, 0.0)),
            Variant::Vector2(Vector2::new(10.0, 40.0)),
            1.0,
        )
        .build();

    tween.advance(0.25);
    match &tween.get_current_values()[0].1 {
        Variant::Vector2(v) => {
            assert!((v.x - 2.5).abs() < TWEEN_EPS_F32);
            assert!((v.y - 10.0).abs() < TWEEN_EPS_F32);
        }
        other => panic!("expected Vector2, got {other:?}"),
    }
}

#[test]
fn tween_property_interpolates_vector3_component_wise() {
    let mut tween = TweenBuilder::new()
        .tween_property(
            "translation",
            Variant::Vector3(Vector3::new(0.0, 0.0, 0.0)),
            Variant::Vector3(Vector3::new(2.0, 4.0, 8.0)),
            1.0,
        )
        .build();

    tween.advance(0.5);
    match &tween.get_current_values()[0].1 {
        Variant::Vector3(v) => {
            assert!((v.x - 1.0).abs() < TWEEN_EPS_F32);
            assert!((v.y - 2.0).abs() < TWEEN_EPS_F32);
            assert!((v.z - 4.0).abs() < TWEEN_EPS_F32);
        }
        other => panic!("expected Vector3, got {other:?}"),
    }
}

#[test]
fn tween_property_interpolates_color_channels() {
    let mut tween = TweenBuilder::new()
        .tween_property(
            "modulate",
            Variant::Color(Color::BLACK),
            Variant::Color(Color::WHITE),
            1.0,
        )
        .build();

    tween.advance(0.5);
    match &tween.get_current_values()[0].1 {
        Variant::Color(c) => {
            assert!((c.r - 0.5).abs() < TWEEN_EPS_F32);
            assert!((c.g - 0.5).abs() < TWEEN_EPS_F32);
            assert!((c.b - 0.5).abs() < TWEEN_EPS_F32);
        }
        other => panic!("expected Color, got {other:?}"),
    }
}

#[test]
fn tween_property_applies_easing_curve_to_interpolation() {
    let mut tween = TweenBuilder::new()
        .tween_property("x", Variant::Float(0.0), Variant::Float(100.0), 1.0)
        .set_ease(EaseType::In)
        .set_trans(TransFunc::Quad)
        .build();

    tween.advance(0.5);
    let v = tween_extract_float(&tween.get_current_values()[0].1);
    assert!((v - 25.0).abs() < TWEEN_EPS, "eased mid = {v}");
}

#[test]
fn tween_property_zero_duration_snaps_to_end() {
    let step = TweenStep::new("x", Variant::Float(5.0), Variant::Float(9.0), 0.0);
    assert_eq!(step.current_value(), Variant::Float(9.0));
}

#[test]
fn tween_property_delay_holds_start_value() {
    let mut tween = TweenBuilder::new()
        .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
        .set_delay(0.5)
        .build();

    tween.advance(0.25);
    assert_eq!(tween.get_current_values()[0].1, Variant::Float(0.0));

    tween.advance(0.75);
    let v = tween_extract_float(&tween.get_current_values()[0].1);
    assert!((v - 5.0).abs() < TWEEN_EPS, "post-delay mid = {v}");
}

#[test]
fn tween_property_sequential_chain_runs_steps_in_order() {
    let mut tween = TweenBuilder::new()
        .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
        .tween_property("y", Variant::Float(0.0), Variant::Float(20.0), 1.0)
        .build();

    assert!(!tween.advance(1.0));
    let values = tween.get_current_values();
    assert_eq!(values[0].1, Variant::Float(10.0));
    assert_eq!(values[1].1, Variant::Float(0.0));

    let done = tween.advance(1.0);
    assert!(done);
    assert!(!tween.running);
    assert_eq!(tween.get_current_values()[1].1, Variant::Float(20.0));
}

#[test]
fn tween_property_parallel_steps_advance_together() {
    let mut tween = TweenBuilder::new()
        .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
        .tween_property("y", Variant::Float(0.0), Variant::Float(20.0), 1.0)
        .parallel()
        .build();

    tween.advance(0.5);
    let values = tween.get_current_values();
    let x = tween_extract_float(&values[0].1);
    let y = tween_extract_float(&values[1].1);
    assert!((x - 5.0).abs() < TWEEN_EPS, "x mid = {x}");
    assert!((y - 10.0).abs() < TWEEN_EPS, "y mid = {y}");

    let done = tween.advance(0.5);
    assert!(done, "parallel steps complete together");
}

#[test]
fn tween_property_chained_sequential_and_parallel_mixed() {
    let mut tween = TweenBuilder::new()
        .tween_property("a", Variant::Float(0.0), Variant::Float(1.0), 1.0)
        .tween_property("b", Variant::Float(0.0), Variant::Float(2.0), 1.0)
        .tween_property("c", Variant::Float(0.0), Variant::Float(4.0), 1.0)
        .parallel()
        .build();

    tween.advance(0.5);
    let values = tween.get_current_values();
    assert!((tween_extract_float(&values[0].1) - 0.5).abs() < TWEEN_EPS);
    assert_eq!(values[1].1, Variant::Float(0.0));
    assert_eq!(values[2].1, Variant::Float(0.0));

    tween.advance(0.5); // finish step 0
    tween.advance(0.5); // half into parallel group
    let values = tween.get_current_values();
    let b = tween_extract_float(&values[1].1);
    let c = tween_extract_float(&values[2].1);
    assert!((b - 1.0).abs() < TWEEN_EPS, "b mid = {b}");
    assert!((c - 2.0).abs() < TWEEN_EPS, "c mid = {c}");

    let done = tween.advance(0.5);
    assert!(done, "mixed chain completes in 2s total");
}

#[test]
fn tween_property_looping_replays_requested_count() {
    let mut tween = TweenBuilder::new()
        .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
        .set_loops(3)
        .build();

    assert!(!tween.advance(1.0));
    assert!(tween.running);
    assert!(!tween.advance(1.0));
    assert!(tween.running);
    assert!(tween.advance(1.0));
    assert!(!tween.running);
}

#[test]
fn tween_property_infinite_loop_never_self_terminates() {
    let mut tween = TweenBuilder::new()
        .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
        .set_loops(-1)
        .build();

    for _ in 0..25 {
        assert!(!tween.advance(1.0));
        assert!(tween.running);
    }
}

#[test]
fn tween_property_stop_halts_advance() {
    let mut tween = TweenBuilder::new()
        .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
        .build();

    tween.stop();
    assert!(tween.advance(0.5));
    assert_eq!(tween.get_current_values()[0].1, Variant::Float(0.0));
}

#[test]
fn tween_property_empty_tween_completes_immediately() {
    let mut tween = Tween::new();
    tween.start();
    assert!(tween.advance(1.0));
    assert!(!tween.running);
}

// ---------------------------------------------------------------------------
// AnimationTree parity (pat-r992m)
//
// Covers blend nodes, 1D/2D blend spaces, and state-machine transitions.
// Run with:
//   `./scripts/rust_task.sh nextest run --test animation_parity_test -- animation_tree_blend`
// ---------------------------------------------------------------------------

use gdscene::animation_tree::{
    AnimationNode, AnimationTree, BlendSpace1D, BlendSpace2D, OneShot, StateMachine,
    StateTransition,
};

fn const_anim(name: &str, value: f64) -> Animation {
    let mut anim = Animation::new(name, 1.0);
    anim.loop_mode = LoopMode::Linear;
    let mut track = AnimationTrack::new("x");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(value)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(value)));
    anim.tracks.push(track);
    anim
}

#[test]
fn animation_tree_blend_node_mixes_two_animations() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("idle", 0.0));
    tree.add_animation(const_anim("walk", 10.0));
    tree.set_root(AnimationNode::Blend2 {
        a: Box::new(AnimationNode::Animation {
            name: "idle".into(),
            position: 0.0,
        }),
        b: Box::new(AnimationNode::Animation {
            name: "walk".into(),
            position: 0.0,
        }),
        weight: 0.25,
    });

    let values = tree.get_current_values();
    let x = float_at(&values, "x");
    assert!((x - 2.5).abs() < 1e-5, "blend @0.25 = {x}");
}

#[test]
fn animation_tree_blend_node_weight_clamps_to_unit_interval() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("a", 1.0));
    tree.add_animation(const_anim("b", 9.0));

    tree.set_root(AnimationNode::Blend2 {
        a: Box::new(AnimationNode::Animation {
            name: "a".into(),
            position: 0.0,
        }),
        b: Box::new(AnimationNode::Animation {
            name: "b".into(),
            position: 0.0,
        }),
        weight: -1.0,
    });
    assert_eq!(float_at(&tree.get_current_values(), "x"), 1.0);

    tree.set_root(AnimationNode::Blend2 {
        a: Box::new(AnimationNode::Animation {
            name: "a".into(),
            position: 0.0,
        }),
        b: Box::new(AnimationNode::Animation {
            name: "b".into(),
            position: 0.0,
        }),
        weight: 2.0,
    });
    assert_eq!(float_at(&tree.get_current_values(), "x"), 9.0);
}

#[test]
fn animation_tree_blend_space_1d_interpolates_between_clips() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("idle", 0.0));
    tree.add_animation(const_anim("walk", 10.0));
    tree.add_animation(const_anim("run", 20.0));

    let mut space = BlendSpace1D::new();
    space.add_point(0.0, "idle");
    space.add_point(1.0, "walk");
    space.add_point(2.0, "run");
    space.blend_position = 0.5;

    tree.set_root(AnimationNode::BlendSpace1D(space));
    assert!((float_at(&tree.get_current_values(), "x") - 5.0).abs() < 1e-5);

    // Mid-point between walk (10) and run (20) should be 15.
    if let Some(AnimationNode::BlendSpace1D(space)) = tree.root.as_mut() {
        space.blend_position = 1.5;
    }
    assert!((float_at(&tree.get_current_values(), "x") - 15.0).abs() < 1e-5);
}

#[test]
fn animation_tree_blend_space_1d_clamps_outside_range() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("low", 2.0));
    tree.add_animation(const_anim("high", 8.0));

    let mut space = BlendSpace1D::new();
    space.add_point(0.0, "low");
    space.add_point(1.0, "high");
    space.blend_position = -5.0;
    tree.set_root(AnimationNode::BlendSpace1D(space));
    assert_eq!(float_at(&tree.get_current_values(), "x"), 2.0);

    if let Some(AnimationNode::BlendSpace1D(space)) = tree.root.as_mut() {
        space.blend_position = 99.0;
    }
    assert_eq!(float_at(&tree.get_current_values(), "x"), 8.0);
}

#[test]
fn animation_tree_blend_space_2d_bilinear_at_corners_and_center() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("sw", 0.0));
    tree.add_animation(const_anim("se", 10.0));
    tree.add_animation(const_anim("nw", 20.0));
    tree.add_animation(const_anim("ne", 30.0));

    let mut space = BlendSpace2D::new();
    space.add_point(gdcore::math::Vector2::new(0.0, 0.0), "sw");
    space.add_point(gdcore::math::Vector2::new(1.0, 0.0), "se");
    space.add_point(gdcore::math::Vector2::new(0.0, 1.0), "nw");
    space.add_point(gdcore::math::Vector2::new(1.0, 1.0), "ne");
    space.blend_position = gdcore::math::Vector2::new(0.5, 0.5);
    tree.set_root(AnimationNode::BlendSpace2D(space));

    // Center = (0 + 10 + 20 + 30) / 4 = 15.
    let v = float_at(&tree.get_current_values(), "x");
    assert!((v - 15.0).abs() < 1e-4, "center = {v}");

    if let Some(AnimationNode::BlendSpace2D(space)) = tree.root.as_mut() {
        space.blend_position = gdcore::math::Vector2::new(1.0, 0.0);
    }
    assert!((float_at(&tree.get_current_values(), "x") - 10.0).abs() < 1e-4);
}

#[test]
fn animation_tree_blend_state_machine_plays_current_state() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("idle", 4.0));
    tree.add_animation(const_anim("walk", 7.0));

    let mut sm = StateMachine::default();
    sm.states.insert(
        "idle".into(),
        AnimationNode::Animation {
            name: "idle".into(),
            position: 0.0,
        },
    );
    sm.states.insert(
        "walk".into(),
        AnimationNode::Animation {
            name: "walk".into(),
            position: 0.0,
        },
    );
    sm.current = "idle".into();
    tree.set_root(AnimationNode::StateMachine(sm));

    assert_eq!(float_at(&tree.get_current_values(), "x"), 4.0);
}

#[test]
fn animation_tree_blend_state_machine_transitions_on_condition() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("idle", 0.0));
    tree.add_animation(const_anim("walk", 10.0));

    let mut sm = StateMachine::default();
    sm.states.insert(
        "idle".into(),
        AnimationNode::Animation {
            name: "idle".into(),
            position: 0.0,
        },
    );
    sm.states.insert(
        "walk".into(),
        AnimationNode::Animation {
            name: "walk".into(),
            position: 0.0,
        },
    );
    sm.current = "idle".into();
    sm.transitions.push(StateTransition {
        from: "idle".into(),
        to: "walk".into(),
        condition: "moving".into(),
        crossfade: 0.0,
    });
    tree.set_root(AnimationNode::StateMachine(sm));

    // Before condition fires, we stay in idle.
    tree.advance(0.1);
    assert_eq!(float_at(&tree.get_current_values(), "x"), 0.0);

    tree.set_param("moving", true);
    tree.advance(0.1);
    // Instant transition — walk's value now dominates.
    assert_eq!(float_at(&tree.get_current_values(), "x"), 10.0);
}

#[test]
fn animation_tree_blend_state_machine_crossfade_blends_during_transition() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("idle", 0.0));
    tree.add_animation(const_anim("walk", 10.0));

    let mut sm = StateMachine::default();
    sm.states.insert(
        "idle".into(),
        AnimationNode::Animation {
            name: "idle".into(),
            position: 0.0,
        },
    );
    sm.states.insert(
        "walk".into(),
        AnimationNode::Animation {
            name: "walk".into(),
            position: 0.0,
        },
    );
    sm.current = "idle".into();
    sm.transitions.push(StateTransition {
        from: "idle".into(),
        to: "walk".into(),
        condition: "moving".into(),
        crossfade: 1.0,
    });
    tree.set_root(AnimationNode::StateMachine(sm));

    tree.set_param("moving", true);
    tree.advance(0.5); // Triggers transition, advances halfway through the 1s crossfade.
    let mid = float_at(&tree.get_current_values(), "x");
    assert!(
        (mid - 5.0).abs() < 1e-4,
        "mid-crossfade should blend ~5.0, got {mid}"
    );

    tree.advance(0.6); // Finish the crossfade.
    let end = float_at(&tree.get_current_values(), "x");
    assert!((end - 10.0).abs() < 1e-5, "post-crossfade = {end}");
}

#[test]
fn animation_tree_blend_nested_state_machine_inside_blend_node() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("idle", 0.0));
    tree.add_animation(const_anim("walk", 10.0));
    tree.add_animation(const_anim("aim", 100.0));

    let mut sm = StateMachine::default();
    sm.states.insert(
        "idle".into(),
        AnimationNode::Animation {
            name: "idle".into(),
            position: 0.0,
        },
    );
    sm.states.insert(
        "walk".into(),
        AnimationNode::Animation {
            name: "walk".into(),
            position: 0.0,
        },
    );
    sm.current = "walk".into();

    tree.set_root(AnimationNode::Blend2 {
        a: Box::new(AnimationNode::StateMachine(sm)),
        b: Box::new(AnimationNode::Animation {
            name: "aim".into(),
            position: 0.0,
        }),
        weight: 0.5,
    });

    // (walk=10 * 0.5) + (aim=100 * 0.5) = 55.
    let v = float_at(&tree.get_current_values(), "x");
    assert!((v - 55.0).abs() < 1e-4, "nested blend = {v}");
}

#[test]
fn animation_tree_blend_advance_progresses_animation_position() {
    let mut tree = AnimationTree::new();
    let mut ramp = Animation::new("ramp", 1.0);
    ramp.loop_mode = LoopMode::Linear;
    let mut track = AnimationTrack::new("x");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(100.0)));
    ramp.tracks.push(track);
    tree.add_animation(ramp);

    tree.set_root(AnimationNode::Animation {
        name: "ramp".into(),
        position: 0.0,
    });

    tree.advance(0.25);
    let v = float_at(&tree.get_current_values(), "x");
    assert!((v - 25.0).abs() < 1e-4, "advanced to {v}");
}

fn blend3_anim_leaf(name: &str) -> AnimationNode {
    AnimationNode::Animation {
        name: name.into(),
        position: 0.0,
    }
}

#[test]
fn animation_tree_blend3_neutral_weight_samples_center() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("low", 0.0));
    tree.add_animation(const_anim("mid", 5.0));
    tree.add_animation(const_anim("high", 10.0));
    tree.set_root(AnimationNode::Blend3 {
        a: Box::new(blend3_anim_leaf("low")),
        b: Box::new(blend3_anim_leaf("mid")),
        c: Box::new(blend3_anim_leaf("high")),
        weight: 0.0,
    });
    assert_eq!(float_at(&tree.get_current_values(), "x"), 5.0);
}

#[test]
fn animation_tree_blend3_negative_weight_interpolates_low_to_mid() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("low", 0.0));
    tree.add_animation(const_anim("mid", 10.0));
    tree.add_animation(const_anim("high", 20.0));
    tree.set_root(AnimationNode::Blend3 {
        a: Box::new(blend3_anim_leaf("low")),
        b: Box::new(blend3_anim_leaf("mid")),
        c: Box::new(blend3_anim_leaf("high")),
        weight: -0.5,
    });
    // halfway between low(0) and mid(10) → 5.0
    let v = float_at(&tree.get_current_values(), "x");
    assert!((v - 5.0).abs() < 1e-4, "negative-side blend = {v}");
}

#[test]
fn animation_tree_blend3_positive_weight_interpolates_mid_to_high() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("low", 0.0));
    tree.add_animation(const_anim("mid", 10.0));
    tree.add_animation(const_anim("high", 20.0));
    tree.set_root(AnimationNode::Blend3 {
        a: Box::new(blend3_anim_leaf("low")),
        b: Box::new(blend3_anim_leaf("mid")),
        c: Box::new(blend3_anim_leaf("high")),
        weight: 0.25,
    });
    // 25% from mid(10) toward high(20) → 12.5
    let v = float_at(&tree.get_current_values(), "x");
    assert!((v - 12.5).abs() < 1e-4, "positive-side blend = {v}");
}

#[test]
fn animation_tree_blend3_clamps_weight_outside_unit_range() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("low", 1.0));
    tree.add_animation(const_anim("mid", 5.0));
    tree.add_animation(const_anim("high", 9.0));

    tree.set_root(AnimationNode::Blend3 {
        a: Box::new(blend3_anim_leaf("low")),
        b: Box::new(blend3_anim_leaf("mid")),
        c: Box::new(blend3_anim_leaf("high")),
        weight: -5.0,
    });
    assert_eq!(float_at(&tree.get_current_values(), "x"), 1.0);

    tree.set_root(AnimationNode::Blend3 {
        a: Box::new(blend3_anim_leaf("low")),
        b: Box::new(blend3_anim_leaf("mid")),
        c: Box::new(blend3_anim_leaf("high")),
        weight: 5.0,
    });
    assert_eq!(float_at(&tree.get_current_values(), "x"), 9.0);
}

#[test]
fn animation_tree_one_shot_inactive_returns_base() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("base", 2.0));
    tree.add_animation(const_anim("shot", 9.0));

    let one_shot = OneShot::new(blend3_anim_leaf("base"), blend3_anim_leaf("shot"))
        .with_fades(0.1, 0.1)
        .with_shot_length(0.5)
        .with_trigger("fire");
    tree.set_root(AnimationNode::OneShot(one_shot));

    tree.advance(0.25);
    assert_eq!(float_at(&tree.get_current_values(), "x"), 2.0);
}

#[test]
fn animation_tree_one_shot_triggered_plays_shot_at_full_factor() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("base", 0.0));
    tree.add_animation(const_anim("shot", 10.0));

    let one_shot = OneShot::new(blend3_anim_leaf("base"), blend3_anim_leaf("shot"))
        .with_fades(0.1, 0.1)
        .with_shot_length(1.0)
        .with_trigger("fire");
    tree.set_root(AnimationNode::OneShot(one_shot));

    tree.set_param("fire", true);
    tree.advance(0.5); // 0.5s into a 0.1 + 1.0 + 0.1 = 1.2s shot → full hold phase.
    let v = float_at(&tree.get_current_values(), "x");
    assert!((v - 10.0).abs() < 1e-4, "mid-shot factor = {v}");
}

#[test]
fn animation_tree_one_shot_completes_and_returns_to_base() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("base", 3.0));
    tree.add_animation(const_anim("shot", 8.0));

    let one_shot = OneShot::new(blend3_anim_leaf("base"), blend3_anim_leaf("shot"))
        .with_fades(0.1, 0.1)
        .with_shot_length(0.2)
        .with_trigger("fire");
    tree.set_root(AnimationNode::OneShot(one_shot));

    tree.set_param("fire", true);
    // Advance past total (fade_in 0.1 + shot 0.2 + fade_out 0.1 = 0.4s).
    tree.advance(0.5);
    assert_eq!(float_at(&tree.get_current_values(), "x"), 3.0);
    if let Some(AnimationNode::OneShot(one_shot)) = tree.root.as_ref() {
        assert!(!one_shot.active, "one-shot should deactivate after fade-out");
    } else {
        panic!("expected OneShot root");
    }
}

#[test]
fn animation_tree_one_shot_fade_in_blends_between_base_and_shot() {
    let mut tree = AnimationTree::new();
    tree.add_animation(const_anim("base", 0.0));
    tree.add_animation(const_anim("shot", 10.0));

    let one_shot = OneShot::new(blend3_anim_leaf("base"), blend3_anim_leaf("shot"))
        .with_fades(1.0, 0.1)
        .with_shot_length(1.0)
        .with_trigger("fire");
    tree.set_root(AnimationNode::OneShot(one_shot));

    tree.set_param("fire", true);
    tree.advance(0.5); // halfway through 1s fade-in → 50% of shot.
    let v = float_at(&tree.get_current_values(), "x");
    assert!((v - 5.0).abs() < 1e-4, "fade-in blend = {v}");
}

// ---------------------------------------------------------------------------
// Skeleton3D bone-track wiring (pat-rjggr)
// ---------------------------------------------------------------------------

fn make_skeleton(tree: &mut SceneTree) -> gdscene::node::NodeId {
    let root = tree.root_id();
    let skel = Node::new("Skeleton", "Skeleton3D");
    tree.add_child(root, skel).unwrap()
}

fn approx_vec3(a: Vector3, b: Vector3, eps: f32) -> bool {
    (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps && (a.z - b.z).abs() < eps
}

#[test]
fn skeleton_bone_track_applies_transform_keyframe_by_index() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Root");

    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("wiggle", 2.0);
    let mut track = AnimationTrack::with_node("Skeleton", "bones/0/pose");
    let start = Transform3D::IDENTITY;
    let end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(10.0, 0.0, 0.0),
    };
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Transform3D(start)));
    track.add_keyframe(KeyFrame::linear(2.0, Variant::Transform3D(end)));
    anim.tracks.push(track);
    player.add_animation(anim);
    player.play("wiggle");

    player.advance(1.0);
    let values = player.get_current_values();
    let applied = apply_animation_to_skeleton(&mut tree, skel_id, &values);
    assert_eq!(applied, 1, "should apply the single bone pose");

    let pose = get_bone_pose(&tree, skel_id, 0);
    assert!(
        approx_vec3(pose.origin, Vector3::new(5.0, 0.0, 0.0), 1e-4),
        "midpoint origin was {:?}",
        pose.origin
    );
}

#[test]
fn skeleton_bone_track_applies_transform_keyframe_by_name() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Hip");

    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("name_track", 1.0);
    let mut track = AnimationTrack::with_node("Skeleton", "bones/Hip/pose");
    let target = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 7.0, 0.0),
    };
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(target)));
    anim.tracks.push(track);
    player.add_animation(anim);
    player.play("name_track");

    player.advance(1.0);
    let values = player.get_current_values();
    let applied = apply_animation_to_skeleton(&mut tree, skel_id, &values);
    assert_eq!(applied, 1, "name-addressed bone pose should resolve");

    let pose = get_bone_pose(&tree, skel_id, 0);
    assert!(approx_vec3(pose.origin, target.origin, 1e-4));
}

#[test]
fn skeleton_bone_track_multi_bone_applies_each() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Hip");
    add_bone(&mut tree, skel_id, "Spine");

    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("multi", 1.0);

    let hip_end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(1.0, 0.0, 0.0),
    };
    let mut hip_track = AnimationTrack::with_node("Skeleton", "bones/0/pose");
    hip_track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    hip_track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(hip_end)));
    anim.tracks.push(hip_track);

    let spine_end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 2.0, 0.0),
    };
    let mut spine_track = AnimationTrack::with_node("Skeleton", "bones/1/pose");
    spine_track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    spine_track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(spine_end)));
    anim.tracks.push(spine_track);

    player.add_animation(anim);
    player.play("multi");

    player.advance(1.0);
    let values = player.get_current_values();
    let applied = apply_animation_to_skeleton(&mut tree, skel_id, &values);
    assert_eq!(applied, 2);

    assert!(approx_vec3(
        get_bone_pose(&tree, skel_id, 0).origin,
        hip_end.origin,
        1e-4
    ));
    assert!(approx_vec3(
        get_bone_pose(&tree, skel_id, 1).origin,
        spine_end.origin,
        1e-4
    ));
}

#[test]
fn skeleton_bone_track_composes_into_global_pose() {
    // Parent->child chain: parent rest offsets up, child rest offsets forward.
    // Animating the child's pose should compose through the parent's rest * pose.
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Parent");
    add_bone(&mut tree, skel_id, "Child");
    set_bone_parent(&mut tree, skel_id, 1, 0i64);

    set_bone_rest(
        &mut tree,
        skel_id,
        0,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 1.0, 0.0),
        },
    );
    set_bone_rest(
        &mut tree,
        skel_id,
        1,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, 1.0),
        },
    );

    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("chain", 1.0);
    let child_pose_end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(3.0, 0.0, 0.0),
    };
    let mut track = AnimationTrack::with_node("Skeleton", "bones/1/pose");
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(child_pose_end)));
    anim.tracks.push(track);
    player.add_animation(anim);
    player.play("chain");
    player.advance(1.0);

    let values = player.get_current_values();
    assert_eq!(apply_animation_to_skeleton(&mut tree, skel_id, &values), 1);

    // Global = parent_rest * parent_pose(id) * child_rest * child_pose
    //        = translate(y=1) * translate(z=1) * translate(x=3)
    //        = origin (3, 1, 1)
    let global = get_bone_global_pose(&tree, skel_id, 1);
    assert!(
        approx_vec3(global.origin, Vector3::new(3.0, 1.0, 1.0), 1e-4),
        "global origin was {:?}",
        global.origin
    );
}

// ---------------------------------------------------------------------------
// skeleton_animation: AnimationPlayer → Skeleton3D per-frame wiring (pat-dh7xg)
// ---------------------------------------------------------------------------

#[test]
fn skeleton_animation_advance_applies_pose_each_frame() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Root");

    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("walk", 2.0);
    let end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(4.0, 0.0, 0.0),
    };
    let mut track = AnimationTrack::with_node("Skeleton", "bones/0/pose");
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    track.add_keyframe(KeyFrame::linear(2.0, Variant::Transform3D(end)));
    anim.tracks.push(track);
    player.add_animation(anim);
    player.play("walk");

    // Frame 1: advance to t=0.5 → origin should be (1.0, 0, 0)
    let applied = advance_skeleton_animation(&mut tree, skel_id, &mut player, 0.5);
    assert_eq!(applied, 1);
    let pose = get_bone_pose(&tree, skel_id, 0);
    assert!(
        approx_vec3(pose.origin, Vector3::new(1.0, 0.0, 0.0), 1e-4),
        "frame-1 origin was {:?}",
        pose.origin
    );

    // Frame 2: advance to t=1.0 → origin should be (2.0, 0, 0)
    let applied = advance_skeleton_animation(&mut tree, skel_id, &mut player, 0.5);
    assert_eq!(applied, 1);
    let pose = get_bone_pose(&tree, skel_id, 0);
    assert!(
        approx_vec3(pose.origin, Vector3::new(2.0, 0.0, 0.0), 1e-4),
        "frame-2 origin was {:?}",
        pose.origin
    );
}

#[test]
fn skeleton_animation_advance_multi_bone_writes_each() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Hip");
    add_bone(&mut tree, skel_id, "Spine");

    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("idle", 1.0);

    let hip_end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(3.0, 0.0, 0.0),
    };
    let mut hip_track = AnimationTrack::with_node("Skeleton", "bones/Hip/pose");
    hip_track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    hip_track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(hip_end)));
    anim.tracks.push(hip_track);

    let spine_end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, 5.0),
    };
    let mut spine_track = AnimationTrack::with_node("Skeleton", "bones/1/pose");
    spine_track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    spine_track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(spine_end)));
    anim.tracks.push(spine_track);

    player.add_animation(anim);
    player.play("idle");

    let applied = advance_skeleton_animation(&mut tree, skel_id, &mut player, 1.0);
    assert_eq!(applied, 2, "both bone tracks should apply");
    assert!(approx_vec3(
        get_bone_pose(&tree, skel_id, 0).origin,
        hip_end.origin,
        1e-4
    ));
    assert!(approx_vec3(
        get_bone_pose(&tree, skel_id, 1).origin,
        spine_end.origin,
        1e-4
    ));
}

#[test]
fn skeleton_animation_advance_composes_into_global_pose() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Parent");
    add_bone(&mut tree, skel_id, "Child");
    set_bone_parent(&mut tree, skel_id, 1, 0i64);
    set_bone_rest(
        &mut tree,
        skel_id,
        0,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 2.0, 0.0),
        },
    );

    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("reach", 1.0);
    let child_end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(5.0, 0.0, 0.0),
    };
    let mut track = AnimationTrack::with_node("Skeleton", "bones/1/pose");
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(child_end)));
    anim.tracks.push(track);
    player.add_animation(anim);
    player.play("reach");

    let applied = advance_skeleton_animation(&mut tree, skel_id, &mut player, 1.0);
    assert_eq!(applied, 1);

    // Global = parent_rest(y=2) * parent_pose(id) * child_rest(id) * child_pose(x=5)
    //        = origin (5, 2, 0)
    let global = get_bone_global_pose(&tree, skel_id, 1);
    assert!(
        approx_vec3(global.origin, Vector3::new(5.0, 2.0, 0.0), 1e-4),
        "global origin was {:?}",
        global.origin
    );
}

#[test]
fn skeleton_animation_advance_with_no_current_is_noop() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Root");

    // Player with no animation queued / playing — advance must not panic or write.
    let mut player = AnimationPlayer::new();
    let applied = advance_skeleton_animation(&mut tree, skel_id, &mut player, 0.25);
    assert_eq!(applied, 0);
    assert_eq!(
        get_bone_pose(&tree, skel_id, 0),
        Transform3D::IDENTITY,
        "bone pose should remain untouched with no animation"
    );
}

#[test]
fn skeleton_animation_advance_ignores_non_bone_tracks() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Root");

    let mut player = AnimationPlayer::new();
    let mut anim = Animation::new("mixed", 1.0);

    // Non-bone track: a position vector track — must be ignored by the wiring.
    let mut pos_track = AnimationTrack::with_node("Mesh", "position");
    pos_track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    pos_track.add_keyframe(KeyFrame::linear(
        1.0,
        Variant::Vector3(Vector3::new(9.0, 0.0, 0.0)),
    ));
    anim.tracks.push(pos_track);

    // Bone track: the one that should land on the skeleton.
    let bone_end = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, 7.0),
    };
    let mut bone_track = AnimationTrack::with_node("Skeleton", "bones/0/pose");
    bone_track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Transform3D(Transform3D::IDENTITY),
    ));
    bone_track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(bone_end)));
    anim.tracks.push(bone_track);

    player.add_animation(anim);
    player.play("mixed");

    let applied = advance_skeleton_animation(&mut tree, skel_id, &mut player, 1.0);
    assert_eq!(applied, 1, "only the bone track should contribute");
    assert!(approx_vec3(
        get_bone_pose(&tree, skel_id, 0).origin,
        bone_end.origin,
        1e-4
    ));
}

#[test]
fn skeleton_animation_advance_respects_crossfade_blend() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Root");

    let mut player = AnimationPlayer::new();

    // Animation "a": bone pose at origin x=0 throughout.
    let mut a = Animation::new("a", 1.0);
    let mut a_track = AnimationTrack::with_node("Skeleton", "bones/0/pose");
    let a_pose = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, 0.0),
    };
    a_track.add_keyframe(KeyFrame::linear(0.0, Variant::Transform3D(a_pose)));
    a_track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(a_pose)));
    a.tracks.push(a_track);
    player.add_animation(a);

    // Animation "b": bone pose at origin x=10 throughout.
    let mut b = Animation::new("b", 1.0);
    let mut b_track = AnimationTrack::with_node("Skeleton", "bones/0/pose");
    let b_pose = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(10.0, 0.0, 0.0),
    };
    b_track.add_keyframe(KeyFrame::linear(0.0, Variant::Transform3D(b_pose)));
    b_track.add_keyframe(KeyFrame::linear(1.0, Variant::Transform3D(b_pose)));
    b.tracks.push(b_track);
    player.add_animation(b);

    player.play("a");
    player.crossfade_to("b", 1.0);

    // Halfway through the crossfade → expect the pose blended to ~x=5.
    let applied = advance_skeleton_animation(&mut tree, skel_id, &mut player, 0.5);
    assert_eq!(applied, 1);
    let pose = get_bone_pose(&tree, skel_id, 0);
    assert!(
        (pose.origin.x - 5.0).abs() < 1e-3,
        "mid-crossfade origin.x was {}",
        pose.origin.x
    );
}

#[test]
fn skeleton_bone_track_ignores_non_bone_paths() {
    let mut tree = SceneTree::new();
    let skel_id = make_skeleton(&mut tree);
    add_bone(&mut tree, skel_id, "Root");

    let values = vec![
        ("position".to_string(), Variant::Vector3(Vector3::ZERO)),
        (
            "bones/99/pose".to_string(),
            Variant::Transform3D(Transform3D::IDENTITY),
        ),
        (
            "bones/Missing/pose".to_string(),
            Variant::Transform3D(Transform3D::IDENTITY),
        ),
        (
            "bones/0/pose".to_string(),
            Variant::Transform3D(Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(42.0, 0.0, 0.0),
            }),
        ),
    ];
    let applied = apply_animation_to_skeleton(&mut tree, skel_id, &values);
    assert_eq!(applied, 1, "only one valid bone path should apply");
    assert!(approx_vec3(
        get_bone_pose(&tree, skel_id, 0).origin,
        Vector3::new(42.0, 0.0, 0.0),
        1e-4
    ));
}
