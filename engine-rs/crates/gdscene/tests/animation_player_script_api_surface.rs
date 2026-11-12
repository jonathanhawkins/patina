//! pat-s3rrv.43 (slice 1): the AnimationPlayer query/speed script surface.
//!
//! `AnimationPlayer` already had `speed_scale` as a real field, and
//! `AnimationPlayer::advance` already scaled its delta by
//! `speed_scale * custom_speed` — the ENGINE half of the acceptance was live.
//! What was missing was the script surface: `has_animation`,
//! `get_animation_list` and `speed_scale` had no `SceneAccess` method and no
//! interpreter dispatch arm, so a script calling them raised UndefinedFunction.
//!
//! These drive the player through the same accessors the script path uses.

use gdscene::animation::{Animation, AnimationPlayer};

fn player_with(names: &[&str]) -> AnimationPlayer {
    let mut p = AnimationPlayer::new();
    for n in names {
        // `Animation::new(name, length)` — a non-zero length matters: a
        // zero-length clip could not advance and the speed test would be vacuous.
        p.animations
            .insert((*n).to_string(), Animation::new(*n, 10.0));
    }
    p
}

/// `has_animation` answers for a clip the library holds, and denies one it does
/// not — the second half matters because a `contains_key` typo'd into `true`
/// would pass the first alone.
#[test]
fn has_animation_answers_both_ways() {
    let p = player_with(&["walk", "run"]);
    assert!(p.has_animation("walk"));
    assert!(p.has_animation("run"));
    assert!(
        !p.has_animation("fly"),
        "a clip the library does not hold must report false"
    );
}

/// `get_animation_list` returns every clip, SORTED.
///
/// The backing store is a `HashMap`, which iterates arbitrarily; Godot returns a
/// stable list. Without the sort a script iterating the result — and any test
/// asserting on it — would be nondeterministic run to run.
#[test]
fn get_animation_list_is_complete_and_sorted() {
    let p = player_with(&["zulu", "alpha", "mike"]);
    assert_eq!(
        p.get_animation_list(),
        vec!["alpha".to_string(), "mike".to_string(), "zulu".to_string()],
        "the list must be sorted, not in HashMap order"
    );
}

/// An empty player lists nothing rather than panicking.
#[test]
fn an_empty_player_lists_nothing() {
    let p = AnimationPlayer::new();
    assert!(p.get_animation_list().is_empty());
    assert!(!p.has_animation("anything"));
}

/// **The headline acceptance behaviour.** `speed_scale = 2.0` advances the
/// playhead twice as far over the same delta.
///
/// This is what the script-side setter now reaches: the scaling itself was
/// already implemented in `advance`, so the bug was purely that no script could
/// set the field.
#[test]
fn speed_scale_doubles_the_playhead_advance() {
    let step = 0.1;

    let mut normal = player_with(&["walk"]);
    normal.play("walk");
    normal.advance(step);
    let at_1x = normal.position();

    let mut fast = player_with(&["walk"]);
    fast.play("walk");
    fast.speed_scale = 2.0;
    fast.advance(step);
    let at_2x = fast.position();

    assert!(
        at_1x > 0.0,
        "the 1x player must advance at all, got {at_1x}"
    );
    assert!(
        (at_2x - at_1x * 2.0).abs() < 1e-6,
        "speed_scale 2.0 must advance twice as far: 1x={at_1x} 2x={at_2x}"
    );
}

/// A default player runs at 1.0 — the setter must not be required for normal
/// playback, and a default of 0 would freeze every animation in the project.
#[test]
fn the_default_speed_scale_is_one() {
    assert_eq!(AnimationPlayer::new().speed_scale, 1.0);
}

/// `play_section` restricts the playhead to a range (pat-s3rrv.43).
///
/// The engine method already existed with a 7-parameter signature; the script
/// path supplies Godot's defaults for the three a 3-argument call omits.
#[test]
fn play_section_installs_the_range() {
    let mut p = player_with(&["walk"]);
    p.play_section("walk", 0.2, 0.5, -1.0, 1.0, false);

    assert!(p.has_section(), "play_section must install a section");
    assert!(
        (p.get_section_start_time() - 0.2).abs() < 1e-6,
        "section start must be 0.2, got {}",
        p.get_section_start_time()
    );
    assert!(
        (p.get_section_end_time() - 0.5).abs() < 1e-6,
        "section end must be 0.5, got {}",
        p.get_section_end_time()
    );
}

/// An omitted range (the -1 sentinel a 1-argument script call produces) leaves
/// the player UNSECTIONED rather than installing an inverted one.
///
/// `set_section` applies a range only when `end > start`, so -1/-1 is a no-op.
/// Asserting this is what makes the sentinel choice safe rather than incidental.
#[test]
fn an_omitted_section_range_is_a_no_op() {
    let mut p = player_with(&["walk"]);
    p.play_section("walk", -1.0, -1.0, -1.0, 1.0, false);
    assert!(
        !p.has_section(),
        "an inverted/omitted range must not install a section"
    );
}
