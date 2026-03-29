//! Animation editor parity tests.
//!
//! Verifies that the animation editor, timeline, track management, curve editor,
//! and AnimationPlayer/AnimationTree API surfaces match Godot 4 behavior.

use gdeditor::animation_editor::{
    AnimationEditor, KeyframeRef, KeyframeSelection, PlaybackMode, PlaybackState,
    Timeline, TrackDescriptor,
};
use gdeditor::curve_editor::CurveEditor;
use gdscene::animation::{
    Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType, TransitionType,
};
use gdvariant::Variant;

// -- Timeline parity ----------------------------------------------------------

#[test]
fn timeline_godot4_snap_intervals() {
    // Godot 4 supports common snap intervals: 0.1s, 0.05s, 0.01s
    let mut tl = Timeline::new(10.0);
    tl.snap_enabled = true;

    tl.snap_interval = 0.1;
    assert!((tl.snap_time(0.37) - 0.4).abs() < 0.001);

    tl.snap_interval = 0.05;
    assert!((tl.snap_time(0.37) - 0.35).abs() < 0.001);

    tl.snap_interval = 0.01;
    assert!((tl.snap_time(0.374) - 0.37).abs() < 0.001);
}

#[test]
fn timeline_zoom_range_bounds() {
    let mut tl = Timeline::new(5.0);
    // Godot 4 allows wide zoom range
    tl.zoom_at(0.01, 0.0); // extreme zoom out
    assert!(tl.zoom > 0.0);
    tl.zoom_at(1000.0, 0.0); // extreme zoom in
    assert!(tl.zoom > 0.0);
}

// -- Track types parity -------------------------------------------------------

#[test]
fn all_godot4_track_types_representable() {
    // Godot 4 has: Property, Method, Audio
    let mut editor = AnimationEditor::new(2.0, 0);

    editor.create_property_track("Player", "position").unwrap();
    editor.create_method_track("Player", "play_sound").unwrap();
    editor.create_audio_track("AudioPlayer", "bgm").unwrap();

    assert_eq!(editor.track_count(), 3);
    assert_eq!(editor.track_type(0), Some(TrackType::Property));
    assert_eq!(editor.track_type(1), Some(TrackType::Method));
    assert_eq!(editor.track_type(2), Some(TrackType::Audio));
}

#[test]
fn track_descriptor_converts_to_runtime_track() {
    let desc = TrackDescriptor::property("Player/Sprite2D", "modulate");
    let track = desc.to_animation_track();
    assert_eq!(track.node_path, "Player/Sprite2D");
    assert_eq!(track.property_path, "modulate");
    assert_eq!(track.track_type(), TrackType::Property);
}

// -- Keyframe editing parity --------------------------------------------------

#[test]
fn keyframe_selection_multi_track_range() {
    let mut sel = KeyframeSelection::new();
    let times = vec![
        vec![0.0, 0.5, 1.0, 1.5, 2.0],
        vec![0.25, 0.75, 1.25],
    ];
    let counts = vec![5, 3];
    sel.select_range(2, &counts, &times, 0.4, 1.1);

    // Track 0: kf 1 (0.5), kf 2 (1.0) in range
    // Track 1: kf 1 (0.75) in range
    assert_eq!(sel.count(), 3);
    assert!(sel.is_selected(KeyframeRef::new(0, 1)));
    assert!(sel.is_selected(KeyframeRef::new(0, 2)));
    assert!(sel.is_selected(KeyframeRef::new(1, 1)));
}

// -- Playback modes parity ----------------------------------------------------

#[test]
fn playback_modes_match_godot4() {
    // Godot 4 has: Playing, Paused, Stopped
    let ps = PlaybackState::default();
    assert_eq!(ps.mode, PlaybackMode::Stopped);

    let mut ps = PlaybackState::default();
    ps.play();
    assert_eq!(ps.mode, PlaybackMode::Playing);

    ps.pause();
    assert_eq!(ps.mode, PlaybackMode::Paused);

    ps.stop();
    assert_eq!(ps.mode, PlaybackMode::Stopped);
}

#[test]
fn playback_loop_wrap_matches_godot4() {
    let mut ps = PlaybackState::default();
    ps.play();
    ps.looping = true;
    // At time 1.8 with dt 0.5 on a 2.0 length animation: should wrap to 0.3
    let (new_time, wrapped) = ps.advance(1.8, 0.5, 2.0);
    assert!((new_time - 0.3).abs() < 0.01);
    assert!(wrapped);
}

#[test]
fn playback_reverse_matches_godot4() {
    let mut ps = PlaybackState::default();
    ps.play();
    ps.reverse = true;
    ps.looping = true;
    let (new_time, wrapped) = ps.advance(0.2, 0.5, 2.0);
    assert!((new_time - 1.7).abs() < 0.01);
    assert!(wrapped);
}

#[test]
fn playback_speed_scaling() {
    let mut ps = PlaybackState::default();
    ps.play();
    ps.speed = 2.0;
    assert!((ps.effective_speed() - 2.0).abs() < 0.001);
    ps.reverse = true;
    assert!((ps.effective_speed() - (-2.0)).abs() < 0.001);
}

// -- Track state parity -------------------------------------------------------

#[test]
fn track_mute_solo_matches_godot4() {
    let mut editor = AnimationEditor::new(2.0, 3);

    // All tracks audible initially
    for i in 0..3 {
        assert!(editor.is_track_audible(i));
    }

    // Muting one track
    editor.set_track_muted(1, true);
    assert!(editor.is_track_audible(0));
    assert!(!editor.is_track_audible(1));
    assert!(editor.is_track_audible(2));

    // Solo overrides mute
    editor.set_track_muted(1, false);
    editor.toggle_solo(0);
    assert!(editor.is_track_audible(0));
    assert!(!editor.is_track_audible(1));
    assert!(!editor.is_track_audible(2));
}

// -- Onion skinning -----------------------------------------------------------

#[test]
fn onion_skinning_available() {
    let editor = AnimationEditor::new(2.0, 1);
    // Onion skinning should be available but disabled by default (Godot 4 behavior)
    assert!(!editor.onion_skin);
    assert_eq!(editor.onion_skin_count, 2); // default 2 frames
}

// -- Bezier/transition types --------------------------------------------------

#[test]
fn transition_types_match_godot4() {
    // Godot 4 supports: Linear, Nearest, CubicBezier
    let _linear = TransitionType::Linear;
    let _nearest = TransitionType::Nearest;
    let _bezier = TransitionType::CubicBezier(0.42, 0.0, 0.58, 1.0);

    // Standard presets
    assert!(TransitionType::EASE_IN.is_bezier());
    assert!(TransitionType::EASE_OUT.is_bezier());
    assert!(TransitionType::EASE_IN_OUT.is_bezier());
    assert!(!TransitionType::Linear.is_bezier());
}

#[test]
fn bezier_keyframe_interpolation() {
    use gdscene::animation::cubic_bezier_y;

    // Linear (identity): y = t
    let y = cubic_bezier_y(0.0, 0.0, 1.0, 1.0, 0.5);
    assert!((y - 0.5).abs() < 0.01);

    // Ease-in-out should be ~0.5 at t=0.5
    let y = cubic_bezier_y(0.42, 0.0, 0.58, 1.0, 0.5);
    assert!((y - 0.5).abs() < 0.1);

    // Endpoints
    let y0 = cubic_bezier_y(0.42, 0.0, 0.58, 1.0, 0.0);
    let y1 = cubic_bezier_y(0.42, 0.0, 0.58, 1.0, 1.0);
    assert!(y0.abs() < 0.01);
    assert!((y1 - 1.0).abs() < 0.01);
}

// -- Curve editor parity ------------------------------------------------------

#[test]
fn curve_editor_exists_and_creates() {
    let curve = CurveEditor::new("walk".to_string(), 0, 0, 0.42, 0.0, 0.58, 1.0);
    assert!((curve.x1 - 0.42).abs() < 0.001);
    assert!((curve.y2 - 1.0).abs() < 0.001);
}

// -- AnimationPlayer parity ---------------------------------------------------

#[test]
fn animation_player_add_and_play() {
    let mut player = AnimationPlayer::new();

    // Create a simple animation with one track
    let mut track = AnimationTrack::with_node("Player", "position:x");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(100.0)));

    let mut anim = Animation::new("walk", 1.0);
    anim.tracks.push(track);
    player.add_animation(anim);
    assert!(player.animations.contains_key("walk"));
    assert!(!player.animations.contains_key("run"));

    player.play("walk");
    assert!(player.playing);
    assert_eq!(player.current_animation(), Some("walk"));
}

#[test]
fn animation_player_list_animations() {
    let mut player = AnimationPlayer::new();
    player.add_animation(Animation::new("idle", 1.0));
    player.add_animation(Animation::new("walk", 2.0));
    player.add_animation(Animation::new("run", 0.5));

    assert_eq!(player.animations.len(), 3);
    assert!(player.animations.contains_key("idle"));
    assert!(player.animations.contains_key("walk"));
    assert!(player.animations.contains_key("run"));
}

#[test]
fn animation_player_stop_resets() {
    let mut player = AnimationPlayer::new();
    player.add_animation(Animation::new("walk", 1.0));
    player.play("walk");
    assert!(player.playing);
    player.stop();
    assert!(!player.playing);
}

// -- AnimationTrack keyframe operations ---------------------------------------

#[test]
fn animation_track_keyframe_insert_sorted() {
    let mut track = AnimationTrack::new("prop");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(100.0)));
    track.add_keyframe(KeyFrame::linear(0.5, Variant::Float(50.0)));

    let keyframes = track.keyframes();
    assert_eq!(keyframes.len(), 3);
    // Should be sorted by time
    assert!(keyframes[0].time <= keyframes[1].time);
    assert!(keyframes[1].time <= keyframes[2].time);
}

#[test]
fn animation_track_sample_linear_interpolation() {
    let mut track = AnimationTrack::new("alpha");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(1.0)));

    // Sample at midpoint should give ~0.5 for linear interpolation
    let value = track.sample(0.5);
    if let Some(Variant::Float(f)) = value {
        assert!((f - 0.5).abs() < 0.01);
    } else {
        panic!("Expected Some(Float) variant, got {:?}", value);
    }
}

// -- ClassDB registrations for animation nodes --------------------------------

#[test]
fn classdb_animation_classes_registered() {
    gdobject::class_db::register_3d_classes();

    assert!(gdobject::class_db::class_exists("AnimationPlayer"));
    assert!(gdobject::class_db::class_exists("AnimationTree"));
}
