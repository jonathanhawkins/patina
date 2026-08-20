//! pat-s3rrv.122: shader `TIME` is fed by the frame loop.
//!
//! 🔑 A THIRD MISSING PRODUCER on this bead. `CustomFragmentShader::set_time`
//! existed and worked, and `grep -rn "set_time("` across the whole workspace
//! returned exactly two hits — both in that file's own test module. Nothing in
//! production ever called it, so `TIME` evaluated to 0.0 in every real render on
//! every backend and an animated shader rendered static.
//!
//! The chain is: `MainLoop::step` accumulates elapsed seconds and publishes them
//! onto the tree; the render adapter reads them each frame and hands them to the
//! renderer; the renderer gives them to the fragment shader. This pins the first
//! link, which is the one that did not exist.

use gdscene::main_loop::MainLoop;
use gdscene::scene_tree::SceneTree;

/// **The producer.** Stepping the loop advances the published clock.
#[test]
fn stepping_the_loop_publishes_elapsed_seconds() {
    let mut ml = MainLoop::new(SceneTree::new());

    // CONTROL: a fresh loop has published nothing. Without this, a clock that
    // was simply initialised to some non-zero constant would satisfy the
    // assertions below without ever advancing.
    assert_eq!(
        ml.tree().shader_time(),
        0.0,
        "a loop that has not stepped must publish 0"
    );

    ml.run_frames(30, 0.1);

    let t = ml.tree().shader_time();
    assert!(
        (t - 3.0).abs() < 1e-3,
        "30 frames at 0.1s must publish ~3.0 seconds, got {t}"
    );
}

/// The clock ADVANCES between frames rather than being set once.
///
/// A producer that published only on the first frame would pass the test above
/// if the total happened to match, and would leave every shader frozen at that
/// first value — which is the exact bug this bead is about, one layer up.
#[test]
fn the_clock_advances_every_frame() {
    let mut ml = MainLoop::new(SceneTree::new());

    ml.run_frames(5, 0.1);
    let early = ml.tree().shader_time();
    ml.run_frames(5, 0.1);
    let later = ml.tree().shader_time();

    assert!(
        later > early,
        "the published clock must keep advancing: {early} then {later}"
    );
    assert!(
        (later - early - 0.5).abs() < 1e-3,
        "five more 0.1s frames must add ~0.5s, got {}",
        later - early
    );
}
