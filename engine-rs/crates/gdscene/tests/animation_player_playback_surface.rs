//! pat-0q2kx.52 acceptance: `AnimationPlayer`'s playback surface matches Godot —
//! the stop/pause split, `play`'s full argument list, `play_backwards`, `queue`,
//! and section-restricted playback.
//!
//! Patina's `play` always restarted at 0 and took no extra arguments, its `stop`
//! was really Godot's `pause` (it left the playhead where it was), and
//! `pause`/`queue`/`play_backwards`/sections did not exist at all. A game
//! calling `play("walk")` every frame therefore froze on frame 0, and `stop()`
//! followed by `play()` resumed mid-clip instead of restarting.
//!
//! These live in an integration test rather than `animation.rs`'s unit-test
//! module so the bead can be verified without building every other `#[cfg(test)]`
//! module in `gdscene` — an unrelated broken unit test elsewhere in the crate
//! would otherwise fail this bead's verification.

use gdscene::animation::{Animation, AnimationPlayer, AnimationPlayerSignal};

/// pat-0q2kx.52 — Godot's playback surface: queue chaining, section
/// clamping, and the stop/pause split.
#[test]
fn animation_player_queue_sections_stop_semantics() {
    fn player() -> AnimationPlayer {
        let mut p = AnimationPlayer::new();
        p.add_animation(Animation::new("a", 1.0));
        p.add_animation(Animation::new("b", 1.0));
        p
    }

    // -- queue(): the queued clip auto-plays when the first finishes ------
    let mut p = player();
    p.play("a");
    p.queue("b");
    assert_eq!(p.get_queue(), vec!["b".to_string()]);
    p.advance(1.0); // "a" reaches its end
    assert_eq!(
        p.current_animation(),
        Some("b"),
        "a queued clip must start when the current one finishes"
    );
    assert!(p.playing, "the queued clip is playing, not merely selected");
    assert!(p.get_queue().is_empty(), "the queue entry was consumed");

    // -- stop() rewinds and clears the queue; pause() preserves ----------
    let mut p = player();
    p.play("a");
    p.advance(0.5);
    p.pause();
    assert!(!p.playing);
    assert!(
        (p.position() - 0.5).abs() < 1e-9,
        "pause() must preserve the playhead, got {}",
        p.position()
    );

    p.play("a"); // same clip → resumes rather than snapping back to 0
    assert!(
        (p.position() - 0.5).abs() < 1e-9,
        "play() on the already-current clip must not rewind, got {}",
        p.position()
    );

    p.queue("b");
    p.stop();
    assert!(!p.playing);
    assert!(
        (p.position() - 0.0).abs() < 1e-9,
        "stop() must reset the playhead, got {}",
        p.position()
    );
    assert!(p.get_queue().is_empty(), "stop() must cancel the queue");

    // -- play_backwards runs from the end down to 0 and finishes ---------
    let mut p = player();
    p.play_backwards("a", -1.0);
    assert!(
        (p.position() - 1.0).abs() < 1e-9,
        "play_backwards starts at the clip end, got {}",
        p.position()
    );
    assert!(
        p.get_playing_speed() < 0.0,
        "backwards playback is negative"
    );
    p.advance(1.0);
    assert!(
        (p.position() - 0.0).abs() < 1e-9,
        "backwards playback reaches 0, got {}",
        p.position()
    );
    assert!(!p.playing, "reaching the start finishes the clip");
    assert!(
        p.drain_signals()
            .iter()
            .any(|s| matches!(s, AnimationPlayerSignal::AnimationFinished(n) if n == "a")),
        "animation_finished fires when backwards playback reaches the start"
    );

    // -- sections clamp the travelled range ------------------------------
    let mut p = player();
    p.play_section("a", 0.25, 0.75, -1.0, 1.0, false);
    assert!(p.has_section());
    assert_eq!(p.get_section_start_time(), 0.25);
    assert_eq!(p.get_section_end_time(), 0.75);
    assert!(
        (p.position() - 0.25).abs() < 1e-9,
        "playback starts at the section start, got {}",
        p.position()
    );
    p.advance(10.0); // far past the section end
    assert!(
        (p.position() - 0.75).abs() < 1e-9,
        "a non-looping section stops at its end, not the clip length, got {}",
        p.position()
    );
    p.reset_section();
    assert!(!p.has_section());
    assert_eq!(p.get_section_end_time(), 1.0, "falls back to clip length");
}

/// `get_playing_speed` folds `speed_scale` with the per-play `custom_speed`,
/// and reads 0 while stopped.
#[test]
fn animation_player_playing_speed_composes() {
    let mut p = AnimationPlayer::new();
    p.add_animation(Animation::new("a", 1.0));
    assert_eq!(p.get_playing_speed(), 0.0, "nothing playing yet");

    p.speed_scale = 2.0;
    p.play_ex("a", -1.0, 1.5, false);
    assert!((p.get_playing_speed() - 3.0).abs() < 1e-9);

    p.pause();
    assert_eq!(p.get_playing_speed(), 0.0, "a paused player reports 0");
}
