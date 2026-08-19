//! pat-s3rrv.40 acceptance: the web runtime advances its MainLoop from
//! animation-frame timestamps.
//!
//! `gdscene::frame_driver::FrameDriver` already implemented callback
//! coalescing, delta carry, stall clamping and hidden-tab throttling — and had
//! **zero production callers**. `patina-web` was a 157-line stub with a start
//! hook, a version string and a smoke check: nothing in the shipped web runtime
//! advanced a frame at all.
//!
//! These drive `WebEngine`, which takes a millisecond timestamp and nothing
//! else, so the whole frame pipeline is exercised on the host with synthetic
//! timestamps rather than only in a browser. `requestAnimationFrame` hands its
//! callback exactly that value.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use patina_web::engine::WebEngine;

/// One second of frames, in milliseconds, starting at an arbitrary origin (the
/// browser's clock does not start at zero).
const ORIGIN: f64 = 1234.5;

fn engine() -> WebEngine {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Node", "Node2D")).unwrap();
    WebEngine::new(tree)
}

/// Feeds `stamps` and returns the number of frames that actually stepped.
fn feed(engine: &mut WebEngine, stamps: &[f64]) -> usize {
    stamps
        .iter()
        .filter(|t| engine.frame(**t).is_some())
        .count()
}

/// One second delivered as 60 even callbacks simulates one second.
#[test]
fn web_frame_driver_drives_main_loop_from_raf() {
    let mut e = engine();
    let stamps: Vec<f64> = (0..=60)
        .map(|i| ORIGIN + i as f64 * (1000.0 / 60.0))
        .collect();
    let stepped = feed(&mut e, &stamps);

    assert!(stepped > 0, "callbacks must actually step the loop");
    assert!(
        (e.elapsed() - 1.0).abs() < 0.02,
        "one second of timestamps must simulate ~1.0s, got {}",
        e.elapsed()
    );
}

/// **The core claim.** The SAME wall-clock second delivered as 5 uneven
/// callbacks simulates the same amount of time as 60 even ones.
///
/// This is what makes the loop frame-rate independent: a browser that drops
/// frames must not run the game in slow motion.
#[test]
fn uneven_callbacks_simulate_the_same_second() {
    let mut even = engine();
    let even_stamps: Vec<f64> = (0..=60)
        .map(|i| ORIGIN + i as f64 * (1000.0 / 60.0))
        .collect();
    feed(&mut even, &even_stamps);

    let mut uneven = engine();
    // Same span, five ragged callbacks — deltas of 200/240/180/220/160 ms.
    //
    // Every gap is deliberately kept UNDER `DEFAULT_MAX_FRAME_DELTA` (0.25 s).
    // My first version used 130/280/95/375/120 and measured 0.845 s instead of
    // 1.0: the 280 ms and 375 ms gaps were each clamped to 250 ms, which is the
    // stall clamp working correctly, not a bug. Straddling that threshold here
    // would make this test assert the opposite of
    // `a_long_stall_is_clamped_not_simulated` below. Keep every gap < 250 ms.
    let uneven_stamps = [
        ORIGIN,
        ORIGIN + 200.0,
        ORIGIN + 440.0,
        ORIGIN + 620.0,
        ORIGIN + 840.0,
        ORIGIN + 1000.0,
    ];
    feed(&mut uneven, &uneven_stamps);

    assert!(
        (even.elapsed() - uneven.elapsed()).abs() < 0.02,
        "the same second must simulate the same time however it is delivered; \
         even={} uneven={}",
        even.elapsed(),
        uneven.elapsed()
    );
}

/// The first callback establishes the time origin and simulates nothing — there
/// is no previous frame to measure against. Without this the engine would treat
/// the browser's clock value as an elapsed duration and jump forward wildly.
#[test]
fn the_first_callback_only_sets_the_origin() {
    let mut e = engine();
    assert!(
        e.frame(ORIGIN).is_none(),
        "the first callback must not step"
    );
    assert_eq!(e.elapsed(), 0.0, "and must simulate no time");
}

/// A long stall is clamped rather than simulated in one enormous step: a tab
/// backgrounded for a minute must not run a minute of physics on return.
#[test]
fn a_long_stall_is_clamped_not_simulated() {
    let mut e = engine();
    e.frame(ORIGIN);
    e.frame(ORIGIN + 60_000.0);

    assert!(
        e.elapsed() < 1.0,
        "a 60s gap must be clamped well below its wall-clock span, got {}",
        e.elapsed()
    );
}

/// A hidden tab is throttled but does not lose time outright.
#[test]
fn a_hidden_tab_is_throttled_without_freezing() {
    let mut visible = engine();
    let mut hidden = engine();
    hidden.set_hidden(true);
    assert!(hidden.is_hidden());

    let stamps: Vec<f64> = (0..=60)
        .map(|i| ORIGIN + i as f64 * (1000.0 / 60.0))
        .collect();
    let visible_steps = feed(&mut visible, &stamps);
    let hidden_steps = feed(&mut hidden, &stamps);

    assert!(
        hidden_steps < visible_steps,
        "a hidden tab must step less often than a visible one; hidden={hidden_steps} \
         visible={visible_steps}"
    );
    assert!(
        hidden.elapsed() > 0.0,
        "…but must not freeze entirely — throttled is not stopped"
    );
}

/// A clock that goes backwards contributes nothing rather than rewinding the
/// simulation.
#[test]
fn a_backwards_clock_does_not_rewind() {
    let mut e = engine();
    e.frame(ORIGIN);
    e.frame(ORIGIN + 500.0);
    let after = e.elapsed();
    e.frame(ORIGIN + 100.0);

    assert!(
        e.elapsed() >= after,
        "elapsed time must never decrease; was {after}, now {}",
        e.elapsed()
    );
}
