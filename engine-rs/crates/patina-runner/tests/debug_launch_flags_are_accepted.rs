//! pat-s3rrv.82 — the runner accepts the editor's `--debug-*` launch flags.
//!
//! # The bug this pins
//!
//! `gdeditor`'s Debug menu builds a play-session command line from its checked
//! toggles (`DebugToggles::play_session_flags`). The runner's `parse_args`
//! ended with `other => return Err(format!("unknown argument: {other}"))` and
//! knew none of them, so **checking any item in that menu made Play fail
//! outright** — `unknown argument: --debug-navigation` — instead of starting
//! with the overlay enabled. The only UI that offers the feature could not turn
//! it on.
//!
//! ⚠️ The flag strings are duplicated deliberately in this file rather than
//! imported. `patina-runner` cannot depend on `gdeditor` (wrong direction), so
//! the two lists cannot be compared in one test. Pinning the literals here at
//! least makes a rename on either side a visible, reviewable difference rather
//! than a silent launch failure.
//!
//! The editor's side lives at `gdeditor/src/debug_menu_toggles.rs` in
//! `DebugToggles::flag`.

use patina_runner::debug_flags::DebugFlags;

/// Every flag the editor can emit is recognised, and sets ONLY its own field.
///
/// ⚠️ Asserting the whole struct, not just the one field: a copy-paste slip in
/// the match (`"--debug-paths" => self.navigation = true`) sets a real field and
/// would pass any test that only checked "the flag was accepted".
#[test]
fn each_editor_flag_sets_exactly_its_own_field() {
    let cases: [(&str, DebugFlags); 5] = [
        (
            "--debug-collisions",
            DebugFlags {
                collisions: true,
                ..DebugFlags::default()
            },
        ),
        (
            "--debug-navigation",
            DebugFlags {
                navigation: true,
                ..DebugFlags::default()
            },
        ),
        (
            "--debug-paths",
            DebugFlags {
                paths: true,
                ..DebugFlags::default()
            },
        ),
        (
            "--debug-scene-sync",
            DebugFlags {
                scene_sync: true,
                ..DebugFlags::default()
            },
        ),
        (
            "--debug-script-sync",
            DebugFlags {
                script_sync: true,
                ..DebugFlags::default()
            },
        ),
    ];

    for (arg, expected) in cases {
        let mut flags = DebugFlags::default();
        assert!(
            flags.apply(arg),
            "`{arg}` is emitted by the editor's Debug menu and must be accepted \
             — rejecting it is a hard launch failure, not a missing overlay"
        );
        assert_eq!(flags, expected, "`{arg}` must set only its own field");
    }
}

/// ⚠️ The other half: a genuinely unknown argument is still refused.
///
/// Without this, "accept the debug flags" could be implemented by accepting
/// everything, which would turn a typo into a silently ignored argument.
#[test]
fn an_unknown_argument_is_still_rejected() {
    let mut flags = DebugFlags::default();
    assert!(
        !flags.apply("--debug-nagivation"),
        "a misspelled flag must NOT be consumed, so parse_args can still error"
    );
    assert!(
        !flags.apply("--frames"),
        "a real but non-debug flag is not this parser's business"
    );
    assert_eq!(
        flags,
        DebugFlags::default(),
        "a rejected argument must leave every field untouched"
    );
}

/// `any()` gates the runner's JSON report, so it must stay false until a flag
/// is actually set — otherwise every existing run's output gains new keys.
#[test]
fn any_is_false_until_a_flag_is_set() {
    let mut flags = DebugFlags::default();
    assert!(!flags.any(), "a default session has no debug overlays");
    flags.apply("--debug-navigation");
    assert!(flags.any());
}

/// The reported key for `--debug-navigation` is `debug_navigation`, matching
/// Godot's `SceneTree.debug_navigation_hint` vocabulary minus the suffix.
#[test]
fn the_report_names_match_godots_vocabulary() {
    let mut flags = DebugFlags::default();
    flags.apply("--debug-navigation");
    let enabled: Vec<&str> = flags
        .as_pairs()
        .iter()
        .filter(|(_, on)| *on)
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        enabled,
        vec!["debug_navigation"],
        "only the set flag is reported, under Godot's own name for it"
    );
}
