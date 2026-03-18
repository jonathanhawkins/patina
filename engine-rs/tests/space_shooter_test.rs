//! Integration tests for the space shooter mini-game.
//!
//! Validates that all engine subsystems cooperate correctly in the context
//! of a complete game loop: scene tree, input, physics, collision, rendering,
//! particles, and audio.

#[path = "../examples/space_shooter.rs"]
mod space_shooter;

use space_shooter::run_space_shooter;

#[test]
fn score_greater_than_zero() {
    let result = run_space_shooter();
    assert!(
        result.score > 0,
        "Score should be > 0 (bullets should hit enemies), got {}",
        result.score
    );
}

#[test]
fn player_moved_from_start() {
    let result = run_space_shooter();
    let (x, _y) = result.player_final_pos;
    assert!(
        (x - 320.0).abs() > 1.0,
        "Player should have moved from start position (320, 400), got x={x}"
    );
}

#[test]
fn enemies_spawned_enough() {
    let result = run_space_shooter();
    assert!(
        result.enemies_spawned > 5,
        "Should have spawned > 5 enemies, got {}",
        result.enemies_spawned
    );
}

#[test]
fn bullets_fired_enough() {
    let result = run_space_shooter();
    assert!(
        result.bullets_fired > 10,
        "Should have fired > 10 bullets, got {}",
        result.bullets_fired
    );
}

#[test]
fn frame_rendered_correct_dimensions() {
    let result = run_space_shooter();
    assert_eq!(result.fb_width, 640);
    assert_eq!(result.fb_height, 480);
    assert_eq!(result.pixel_data.len(), (640 * 480) as usize);
}

#[test]
fn frame_has_nonzero_pixels() {
    let result = run_space_shooter();
    let bg = gdcore::math::Color::rgb(0.05, 0.05, 0.15);
    let non_bg_count = result
        .pixel_data
        .iter()
        .filter(|p| {
            (p.r - bg.r).abs() > 0.01 || (p.g - bg.g).abs() > 0.01 || (p.b - bg.b).abs() > 0.01
        })
        .count();
    assert!(
        non_bg_count > 100,
        "Frame should have non-background pixels (game objects), got {}",
        non_bg_count
    );
}

#[test]
fn particles_emitted_on_kills() {
    let result = run_space_shooter();
    assert!(
        result.particles_emitted > 0,
        "Should have emitted explosion particles on kills, got {}",
        result.particles_emitted
    );
    // Each kill emits 8 particles
    assert_eq!(
        result.particles_emitted,
        result.enemies_killed as u64 * 8,
        "Each kill should emit 8 particles"
    );
}

#[test]
fn deterministic_two_runs_identical() {
    let r1 = run_space_shooter();
    let r2 = run_space_shooter();
    assert_eq!(r1.score, r2.score, "Score should be deterministic");
    assert_eq!(
        r1.player_final_pos, r2.player_final_pos,
        "Player position should be deterministic"
    );
    assert_eq!(
        r1.enemies_killed, r2.enemies_killed,
        "Enemies killed should be deterministic"
    );
    assert_eq!(
        r1.bullets_fired, r2.bullets_fired,
        "Bullets fired should be deterministic"
    );
    assert_eq!(
        r1.enemies_spawned, r2.enemies_spawned,
        "Enemies spawned should be deterministic"
    );
    assert_eq!(
        r1.pixel_data, r2.pixel_data,
        "Rendered frame should be deterministic"
    );
}

#[test]
fn offscreen_bullets_cleaned_up() {
    let result = run_space_shooter();
    // After 300 frames, all bullets that were fired should either have hit
    // enemies or gone off screen. No bullet should be below y=0 in the
    // remaining active bullets — but since off-screen means y < 0 and we
    // remove them, the active count should be reasonable.
    // The important thing is that cleanup happened (bullets_fired > active_bullets).
    assert!(
        result.active_bullets < result.bullets_fired as usize,
        "Off-screen cleanup should have removed bullets: active={}, fired={}",
        result.active_bullets,
        result.bullets_fired
    );
}

#[test]
fn enemies_killed_matches_score() {
    let result = run_space_shooter();
    assert_eq!(
        result.score, result.enemies_killed,
        "Score should equal enemies killed"
    );
}

#[test]
fn all_frames_rendered() {
    let result = run_space_shooter();
    assert_eq!(
        result.frames_rendered, 300,
        "Should have rendered exactly 300 frames"
    );
}
