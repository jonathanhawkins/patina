//! pat-rappu: AnimationPlayer support for 3D skeletal animation tracks.
//!
//! Validates:
//! 1. TrackType enum variants (Value, BonePosition, BoneRotation, BoneScale)
//! 2. Bone track constructors and properties
//! 3. Bone position/rotation/scale keyframe interpolation
//! 4. sample_bone_tracks aggregates per-bone data
//! 5. apply_bone_tracks drives Skeleton3D bone poses
//! 6. Quaternion slerp interpolation in variant system
//! 7. Full AnimationPlayer → Skeleton3D pipeline

use std::sync::Mutex;

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Quaternion};
use gdobject::class_db;
use gdscene::animation::{
    apply_bone_tracks, sample_bone_tracks, Animation, AnimationPlayer, AnimationTrack,
    BonePoseSample, KeyFrame, LoopMode, TrackType,
};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d;
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn init_classdb() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    class_db::clear_for_testing();
    class_db::register_class(class_db::ClassRegistration::new("Object"));
    class_db::register_class(
        class_db::ClassRegistration::new("Node")
            .parent("Object")
            .property(class_db::PropertyInfo::new(
                "name",
                Variant::String(String::new()),
            )),
    );
    class_db::register_3d_classes();
    guard
}

fn make_tree() -> (SceneTree, std::sync::MutexGuard<'static, ()>) {
    let guard = init_classdb();
    let tree = SceneTree::new();
    (tree, guard)
}

// ── TrackType enum ──────────────────────────────────────────────────

#[test]
fn track_type_default_is_value() {
    assert_eq!(TrackType::default(), TrackType::Value);
}

#[test]
fn track_type_variants_are_distinct() {
    assert_ne!(TrackType::Value, TrackType::BonePosition);
    assert_ne!(TrackType::BonePosition, TrackType::BoneRotation);
    assert_ne!(TrackType::BoneRotation, TrackType::BoneScale);
    assert_ne!(TrackType::BoneScale, TrackType::Value);
}

// ── Bone track constructors ─────────────────────────────────────────

#[test]
fn bone_position_track_has_correct_type_and_idx() {
    let track = AnimationTrack::bone_position("Skeleton3D", 2);
    assert_eq!(track.track_type, TrackType::BonePosition);
    assert_eq!(track.bone_idx, 2);
    assert_eq!(track.node_path, "Skeleton3D");
}

#[test]
fn bone_rotation_track_has_correct_type_and_idx() {
    let track = AnimationTrack::bone_rotation("Skeleton3D", 0);
    assert_eq!(track.track_type, TrackType::BoneRotation);
    assert_eq!(track.bone_idx, 0);
}

#[test]
fn bone_scale_track_has_correct_type_and_idx() {
    let track = AnimationTrack::bone_scale("Skeleton3D", 5);
    assert_eq!(track.track_type, TrackType::BoneScale);
    assert_eq!(track.bone_idx, 5);
}

#[test]
fn value_track_has_default_bone_idx() {
    let track = AnimationTrack::new("position:x");
    assert_eq!(track.track_type, TrackType::Value);
    assert_eq!(track.bone_idx, -1);
}

// ── Bone position keyframe interpolation ────────────────────────────

#[test]
fn bone_position_track_samples_vector3() {
    let mut track = AnimationTrack::bone_position("Skeleton3D", 0);
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Vector3(Vector3::new(10.0, 0.0, 0.0))));

    let val = track.sample(0.5).unwrap();
    if let Variant::Vector3(v) = val {
        assert!((v.x - 5.0).abs() < 0.01);
        assert!(v.y.abs() < 0.01);
    } else {
        panic!("Expected Vector3, got {val:?}");
    }
}

#[test]
fn bone_position_track_clamps_before_first() {
    let mut track = AnimationTrack::bone_position("Skeleton3D", 0);
    track.add_keyframe(KeyFrame::linear(0.5, Variant::Vector3(Vector3::new(1.0, 2.0, 3.0))));

    let val = track.sample(0.0).unwrap();
    if let Variant::Vector3(v) = val {
        assert!((v.x - 1.0).abs() < 0.01);
    } else {
        panic!("Expected Vector3");
    }
}

// ── Quaternion slerp interpolation ──────────────────────────────────

#[test]
fn quaternion_interpolation_works() {
    use gdscene::animation::interpolate_variant;

    let q_a = Quaternion::IDENTITY;
    let q_b = Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), std::f32::consts::FRAC_PI_2);

    let result = interpolate_variant(
        &Variant::Quaternion(q_a),
        &Variant::Quaternion(q_b),
        0.5,
    );
    assert!(result.is_some());
    if let Some(Variant::Quaternion(q)) = result {
        // Halfway rotation around Y should give ~45 degrees
        let expected = Quaternion::from_axis_angle(
            Vector3::new(0.0, 1.0, 0.0),
            std::f32::consts::FRAC_PI_4,
        );
        assert!((q.x - expected.x).abs() < 0.01);
        assert!((q.y - expected.y).abs() < 0.01);
        assert!((q.z - expected.z).abs() < 0.01);
        assert!((q.w - expected.w).abs() < 0.01);
    } else {
        panic!("Expected Quaternion");
    }
}

#[test]
fn bone_rotation_track_slerp_interpolates() {
    let mut track = AnimationTrack::bone_rotation("Skeleton3D", 1);
    let q0 = Quaternion::IDENTITY;
    let q1 = Quaternion::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), std::f32::consts::PI);

    track.add_keyframe(KeyFrame::linear(0.0, Variant::Quaternion(q0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Quaternion(q1)));

    let val = track.sample(0.5).unwrap();
    assert!(matches!(val, Variant::Quaternion(_)));
}

// ── Bone scale keyframe interpolation ───────────────────────────────

#[test]
fn bone_scale_track_interpolates_vector3() {
    let mut track = AnimationTrack::bone_scale("Skeleton3D", 0);
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::new(1.0, 1.0, 1.0))));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Vector3(Vector3::new(2.0, 2.0, 2.0))));

    let val = track.sample(0.5).unwrap();
    if let Variant::Vector3(v) = val {
        assert!((v.x - 1.5).abs() < 0.01);
        assert!((v.y - 1.5).abs() < 0.01);
        assert!((v.z - 1.5).abs() < 0.01);
    } else {
        panic!("Expected Vector3");
    }
}

// ── sample_bone_tracks ──────────────────────────────────────────────

#[test]
fn sample_bone_tracks_aggregates_per_bone() {
    let mut anim = Animation::new("walk", 1.0);

    let mut pos_track = AnimationTrack::bone_position("Skel", 0);
    pos_track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    pos_track.add_keyframe(KeyFrame::linear(1.0, Variant::Vector3(Vector3::new(0.0, 5.0, 0.0))));

    let mut rot_track = AnimationTrack::bone_rotation("Skel", 0);
    let q = Quaternion::from_axis_angle(Vector3::new(1.0, 0.0, 0.0), std::f32::consts::FRAC_PI_2);
    rot_track.add_keyframe(KeyFrame::linear(0.0, Variant::Quaternion(Quaternion::IDENTITY)));
    rot_track.add_keyframe(KeyFrame::linear(1.0, Variant::Quaternion(q)));

    anim.tracks.push(pos_track);
    anim.tracks.push(rot_track);

    let samples = sample_bone_tracks(&anim, 0.5);
    assert_eq!(samples.len(), 1);
    let s = &samples[0];
    assert_eq!(s.bone_idx, 0);
    assert!(s.position.is_some());
    assert!(s.rotation.is_some());
    assert!(s.scale.is_none());

    let pos = s.position.unwrap();
    assert!((pos.y - 2.5).abs() < 0.01);
}

#[test]
fn sample_bone_tracks_multiple_bones() {
    let mut anim = Animation::new("run", 1.0);

    let mut t0 = AnimationTrack::bone_position("Skel", 0);
    t0.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    let mut t1 = AnimationTrack::bone_position("Skel", 1);
    t1.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::new(1.0, 0.0, 0.0))));
    let mut t2 = AnimationTrack::bone_position("Skel", 2);
    t2.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::new(2.0, 0.0, 0.0))));

    anim.tracks.push(t0);
    anim.tracks.push(t1);
    anim.tracks.push(t2);

    let samples = sample_bone_tracks(&anim, 0.0);
    assert_eq!(samples.len(), 3);
}

#[test]
fn sample_bone_tracks_ignores_value_tracks() {
    let mut anim = Animation::new("idle", 1.0);

    let mut value_track = AnimationTrack::new("opacity");
    value_track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0)));
    anim.tracks.push(value_track);

    let mut bone_track = AnimationTrack::bone_position("Skel", 0);
    bone_track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    anim.tracks.push(bone_track);

    let samples = sample_bone_tracks(&anim, 0.0);
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].bone_idx, 0);
}

#[test]
fn sample_bone_tracks_empty_animation_returns_empty() {
    let anim = Animation::new("empty", 1.0);
    let samples = sample_bone_tracks(&anim, 0.5);
    assert!(samples.is_empty());
}

// ── apply_bone_tracks (Skeleton3D integration) ──────────────────────

#[test]
fn apply_bone_tracks_sets_bone_position() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();
    let skel = Node::new("Skel", "Skeleton3D");
    let skel_id = tree.add_child(root, skel).unwrap();

    skeleton3d::add_bone(&mut tree, skel_id, "Hip");
    skeleton3d::add_bone(&mut tree, skel_id, "Spine");

    let samples = vec![BonePoseSample {
        bone_idx: 0,
        position: Some(Vector3::new(0.0, 1.0, 0.0)),
        rotation: None,
        scale: None,
    }];

    apply_bone_tracks(&mut tree, skel_id, &samples);

    let pose = skeleton3d::get_bone_pose(&tree, skel_id, 0);
    assert!((pose.origin.y - 1.0).abs() < 0.01);
}

#[test]
fn apply_bone_tracks_sets_bone_rotation() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();
    let skel = Node::new("Skel", "Skeleton3D");
    let skel_id = tree.add_child(root, skel).unwrap();

    skeleton3d::add_bone(&mut tree, skel_id, "Root");

    let q = Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), std::f32::consts::FRAC_PI_2);
    let samples = vec![BonePoseSample {
        bone_idx: 0,
        position: None,
        rotation: Some(q),
        scale: None,
    }];

    apply_bone_tracks(&mut tree, skel_id, &samples);

    let pose = skeleton3d::get_bone_pose(&tree, skel_id, 0);
    let expected_basis = Basis::from_quaternion(q);
    assert!((pose.basis.x.x - expected_basis.x.x).abs() < 0.01);
    assert!((pose.basis.z.z - expected_basis.z.z).abs() < 0.01);
}

#[test]
fn apply_bone_tracks_sets_bone_scale() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();
    let skel = Node::new("Skel", "Skeleton3D");
    let skel_id = tree.add_child(root, skel).unwrap();

    skeleton3d::add_bone(&mut tree, skel_id, "Root");

    let samples = vec![BonePoseSample {
        bone_idx: 0,
        position: None,
        rotation: None,
        scale: Some(Vector3::new(2.0, 2.0, 2.0)),
    }];

    apply_bone_tracks(&mut tree, skel_id, &samples);

    let pose = skeleton3d::get_bone_pose(&tree, skel_id, 0);
    // Scaled identity basis: diagonal should be 2.0
    assert!((pose.basis.x.x - 2.0).abs() < 0.01);
    assert!((pose.basis.y.y - 2.0).abs() < 0.01);
    assert!((pose.basis.z.z - 2.0).abs() < 0.01);
}

#[test]
fn apply_bone_tracks_combined_position_rotation_scale() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();
    let skel = Node::new("Skel", "Skeleton3D");
    let skel_id = tree.add_child(root, skel).unwrap();

    skeleton3d::add_bone(&mut tree, skel_id, "Root");

    let q = Quaternion::IDENTITY;
    let samples = vec![BonePoseSample {
        bone_idx: 0,
        position: Some(Vector3::new(1.0, 2.0, 3.0)),
        rotation: Some(q),
        scale: Some(Vector3::new(0.5, 0.5, 0.5)),
    }];

    apply_bone_tracks(&mut tree, skel_id, &samples);

    let pose = skeleton3d::get_bone_pose(&tree, skel_id, 0);
    assert!((pose.origin.x - 1.0).abs() < 0.01);
    assert!((pose.origin.y - 2.0).abs() < 0.01);
    assert!((pose.origin.z - 3.0).abs() < 0.01);
    assert!((pose.basis.x.x - 0.5).abs() < 0.01);
}

// ── Full AnimationPlayer → Skeleton3D pipeline ──────────────────────

#[test]
fn full_pipeline_animation_player_drives_skeleton() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();
    let skel = Node::new("Skel", "Skeleton3D");
    let skel_id = tree.add_child(root, skel).unwrap();

    skeleton3d::add_bone(&mut tree, skel_id, "Hip");
    skeleton3d::add_bone(&mut tree, skel_id, "Spine");
    skeleton3d::set_bone_parent(&mut tree, skel_id, 1, 0);

    // Create animation with bone tracks
    let mut anim = Animation::new("walk", 2.0);
    anim.loop_mode = LoopMode::Linear;

    let mut hip_pos = AnimationTrack::bone_position("Skel", 0);
    hip_pos.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    hip_pos.add_keyframe(KeyFrame::linear(2.0, Variant::Vector3(Vector3::new(0.0, 0.0, 10.0))));

    let mut spine_rot = AnimationTrack::bone_rotation("Skel", 1);
    spine_rot.add_keyframe(KeyFrame::linear(0.0, Variant::Quaternion(Quaternion::IDENTITY)));
    spine_rot.add_keyframe(KeyFrame::linear(2.0, Variant::Quaternion(
        Quaternion::from_axis_angle(Vector3::new(1.0, 0.0, 0.0), std::f32::consts::FRAC_PI_4),
    )));

    anim.tracks.push(hip_pos);
    anim.tracks.push(spine_rot);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("walk");

    // Advance to midpoint
    player.advance(1.0);
    assert!((player.position() - 1.0).abs() < 0.01);

    // Sample and apply
    let anim_ref = player.animations.get("walk").unwrap();
    let samples = sample_bone_tracks(anim_ref, player.position());
    assert_eq!(samples.len(), 2);

    apply_bone_tracks(&mut tree, skel_id, &samples);

    // Check hip moved halfway
    let hip_pose = skeleton3d::get_bone_pose(&tree, skel_id, 0);
    assert!((hip_pose.origin.z - 5.0).abs() < 0.01);

    // Check spine rotated halfway
    let spine_pose = skeleton3d::get_bone_pose(&tree, skel_id, 1);
    assert!(spine_pose.basis.x.x > 0.9); // Still mostly facing forward

    // Global pose should compose rest * pose up the chain
    let global = skeleton3d::get_bone_global_pose(&tree, skel_id, 1);
    // Spine's global pose includes hip's position
    assert!((global.origin.z - 5.0).abs() < 0.5);
}

#[test]
fn pipeline_looping_animation_wraps_correctly() {
    let mut anim = Animation::new("bob", 1.0);
    anim.loop_mode = LoopMode::Linear;

    let mut track = AnimationTrack::bone_position("Skel", 0);
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Vector3(Vector3::new(0.0, 10.0, 0.0))));
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("bob");

    // Advance past the end — should loop
    player.advance(1.5);
    assert!(player.playing);
    let pos = player.position();
    assert!((pos - 0.5).abs() < 0.01);

    let samples = sample_bone_tracks(player.animations.get("bob").unwrap(), pos);
    assert_eq!(samples.len(), 1);
    let p = samples[0].position.unwrap();
    assert!((p.y - 5.0).abs() < 0.01);
}

#[test]
fn multiple_bones_animated_simultaneously() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();
    let skel = Node::new("Skel", "Skeleton3D");
    let skel_id = tree.add_child(root, skel).unwrap();

    for name in &["Hip", "Spine", "Head", "LeftArm", "RightArm"] {
        skeleton3d::add_bone(&mut tree, skel_id, name);
    }

    let mut anim = Animation::new("idle", 1.0);
    for i in 0..5i32 {
        let mut track = AnimationTrack::bone_position("Skel", i);
        track.add_keyframe(KeyFrame::linear(
            0.0,
            Variant::Vector3(Vector3::new(i as f32, 0.0, 0.0)),
        ));
        anim.tracks.push(track);
    }

    let samples = sample_bone_tracks(&anim, 0.0);
    assert_eq!(samples.len(), 5);

    apply_bone_tracks(&mut tree, skel_id, &samples);

    for i in 0..5 {
        let pose = skeleton3d::get_bone_pose(&tree, skel_id, i);
        assert!((pose.origin.x - i as f32).abs() < 0.01);
    }
}
