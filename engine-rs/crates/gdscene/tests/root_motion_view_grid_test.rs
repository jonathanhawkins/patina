//! pat-nzgdu.184 — a `RootMotionView` grid whose offset advances with the root
//! motion a real `AnimationPlayer` accumulates.
//!
//! The gizmo's whole purpose is to make root motion *visible*: a walk cycle that
//! carries the character forward must scroll the grid, and one that slides in
//! place must not. So these tests drive the real
//! `AnimationPlayer::consume_root_motion` — the same call the runtime makes —
//! rather than feeding the view hand-made deltas. A grid that scrolled on
//! invented numbers would look identical and prove nothing.
//!
//! **This does NOT satisfy the whole acceptance:** the editor-viewport drawing
//! and the `.tscn` + `animation_path` resolution are follow-up. See the bead
//! notes.

use gdcore::math::Vector3;
use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType};
use gdscene::root_motion_view::RootMotionView;
use gdvariant::Variant;

/// A 2-second "walk" whose root track travels (4, 0, 8) — so each half-second
/// of play is (1, 0, 2) of motion.
fn walking_player() -> AnimationPlayer {
    let mut anim = Animation::new("walk", 2.0);
    let mut track = AnimationTrack::with_type("Root", "position", TrackType::Property);
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    track.add_keyframe(KeyFrame::linear(
        2.0,
        Variant::Vector3(Vector3::new(4.0, 0.0, 8.0)),
    ));
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("walk");
    player.root_motion_track = Some("position".to_string());
    player
}

/// The acceptance's core: the grid offset advances with root-motion
/// accumulation, frame by frame.
#[test]
fn root_motion_view_grid_test() {
    let player = walking_player();
    // This test measures ACCUMULATION, not wrapping (`offset_wraps_at_one_cell`
    // covers that), so the cell must genuinely contain the whole walk. Max
    // accumulated travel below is (2, 0, 4) — so 8, not 4: at cell_size 4 the z
    // travel would land exactly ON the boundary and wrap to 0.0.
    //
    // Note the field assignment rather than `..RootMotionView::new()`: struct
    // update syntax needs every field visible, and `accumulated` is private, so
    // FRU is an E0451 from another crate (it only compiles inside the module).
    let mut view = RootMotionView::new();
    view.cell_size = 8.0;

    assert_eq!(
        view.grid_offset(),
        Vector3::ZERO,
        "an untouched gizmo sits at the origin"
    );

    // Frame 1: play 0.0 -> 0.5 -> the animation drives (1, 0, 2).
    view.accumulate(&player.consume_root_motion(0.0, 0.5));
    let after_one = view.grid_offset();
    assert!(
        (after_one.x - 1.0).abs() < 1e-4 && (after_one.z - 2.0).abs() < 1e-4,
        "the grid must scroll by the motion the animation drove, got {after_one:?}"
    );

    // Frame 2: play 0.5 -> 1.0 -> another (1, 0, 2). The offset ADVANCES; it is
    // not recomputed from the current frame alone.
    view.accumulate(&player.consume_root_motion(0.5, 1.0));
    let after_two = view.grid_offset();
    assert!(
        (after_two.x - 2.0).abs() < 1e-4 && (after_two.z - 4.0).abs() < 1e-4,
        "root motion must ACCUMULATE across frames, got {after_two:?}"
    );
    assert!(
        after_two.z > after_one.z,
        "the grid keeps scrolling while the animation plays"
    );

    // The unwrapped travel is the real distance walked.
    let travelled = view.accumulated();
    assert!(
        (travelled - Vector3::new(2.0, 0.0, 4.0)).length() < 1e-4,
        "half a 2s walk of (4,0,8) is (2,0,4), got {travelled:?}"
    );
}

/// ⚠️ An animation with no root-motion track drives nothing — a character
/// sliding in place must leave the grid still. If the grid scrolled here, the
/// gizmo would be reporting motion that does not exist.
#[test]
fn animation_without_root_track_leaves_the_grid_still() {
    let mut player = walking_player();
    player.root_motion_track = None;

    let mut view = RootMotionView::new();
    view.accumulate(&player.consume_root_motion(0.0, 0.5));
    view.accumulate(&player.consume_root_motion(0.5, 1.0));

    assert_eq!(
        view.accumulated(),
        Vector3::ZERO,
        "no root-motion track means no travel"
    );
    assert_eq!(
        view.grid_offset(),
        Vector3::ZERO,
        "so the grid must not move"
    );
}

/// With Godot's default 1-unit cells the same walk wraps — which is exactly what
/// makes the grid read as infinite ground rather than a moving object.
#[test]
fn default_cell_size_wraps_the_walk_within_one_cell() {
    let player = walking_player();
    let mut view = RootMotionView::new(); // cell_size 1.0
    assert_eq!(view.cell_size, 1.0);

    // 0.0 -> 0.25 drives (0.5, 0, 1.0): x stays inside the cell, z crosses one
    // whole cell and wraps back to 0.
    view.accumulate(&player.consume_root_motion(0.0, 0.25));
    let off = view.grid_offset();
    assert!(
        (off.x - 0.5).abs() < 1e-4,
        "half a cell of x travel shows as half a cell, got {}",
        off.x
    );
    assert!(
        off.z.abs() < 1e-4,
        "exactly one cell of z travel is indistinguishable from none, got {}",
        off.z
    );
    // Every drawn offset must stay inside one cell, or the grid tears.
    assert!(off.x >= 0.0 && off.x < 1.0, "x escaped its cell: {}", off.x);
    assert!(off.z >= 0.0 && off.z < 1.0, "z escaped its cell: {}", off.z);

    // ...while the honest travel keeps counting past the cell.
    assert!((view.accumulated().z - 1.0).abs() < 1e-4);
}

/// Playing the animation backwards must scroll the grid backwards, and the
/// offset must stay inside its cell rather than going negative.
#[test]
fn reversed_playback_scrolls_the_grid_backwards() {
    let player = walking_player();
    let mut view = RootMotionView::new();

    // Consuming 0.25 -> 0.0 is the walk in reverse: (-0.5, 0, -1.0). Half a
    // cell, deliberately: a whole cell would wrap to exactly 0.0, which is
    // indistinguishable from "no motion" and would prove nothing.
    view.accumulate(&player.consume_root_motion(0.25, 0.0));
    let travelled = view.accumulated();
    assert!(
        (travelled.x + 0.5).abs() < 1e-4 && (travelled.z + 1.0).abs() < 1e-4,
        "reverse playback must drive negative travel, got {travelled:?}"
    );

    let off = view.grid_offset();
    assert!(
        off.x >= 0.0 && off.x < 1.0 && off.z >= 0.0 && off.z < 1.0,
        "a backwards offset must wrap into its cell, not go negative: {off:?}"
    );
    // -0.5 wraps to 0.5. Plain `%` would give -0.5 and drag the grid out of its
    // own cell.
    assert!(
        (off.x - 0.5).abs() < 1e-4,
        "-0.5 must wrap to 0.5, got {}",
        off.x
    );
}
