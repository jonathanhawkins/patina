//! pat-0q2kx.125 acceptance: the builtin `InputMap` seeds Godot's *whole*
//! `ui_*` table, and its key bindings are modifier-aware.
//!
//! Godot seeds 72 default actions in `core/input/input_map.cpp` — the names at
//! `:341-409` and their event bindings at `:436-853` — including modifier combos
//! (`ui_copy` = Ctrl+C) alongside bare-key navigation (`ui_home` = Home). A
//! partial table is not a cosmetic gap: a `Control` asking for `ui_menu` or
//! `ui_swap_input_direction` simply never fires, and an action bound to a bare
//! letter would swallow its Ctrl-modified shortcut.
//!
//! These assert the count against upstream and then the matching *semantics*,
//! because a table of the right size with the wrong modifier rules would pass a
//! count check alone.

use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key, KeyCombo};

/// Every `ui_*` action Godot's `input_map.cpp` defines, in its own order. Kept
/// as a literal so this test fails when the table drifts from upstream rather
/// than agreeing with whatever the implementation happens to seed.
const GODOT_UI_ACTIONS: [&str; 72] = [
    "ui_accept",
    "ui_accessibility_drag_and_drop",
    "ui_cancel",
    "ui_close_dialog",
    "ui_colorpicker_delete_preset",
    "ui_copy",
    "ui_cut",
    "ui_down",
    "ui_end",
    "ui_filedialog_delete",
    "ui_filedialog_find",
    "ui_filedialog_focus_path",
    "ui_filedialog_refresh",
    "ui_filedialog_show_hidden",
    "ui_filedialog_up_one_level",
    "ui_focus_mode",
    "ui_focus_next",
    "ui_focus_prev",
    "ui_graph_delete",
    "ui_graph_duplicate",
    "ui_graph_follow_left",
    "ui_graph_follow_right",
    "ui_home",
    "ui_left",
    "ui_menu",
    "ui_page_down",
    "ui_page_up",
    "ui_paste",
    "ui_redo",
    "ui_right",
    "ui_select",
    "ui_swap_input_direction",
    "ui_text_add_selection_for_next_occurrence",
    "ui_text_backspace",
    "ui_text_backspace_all_to_left",
    "ui_text_backspace_word",
    "ui_text_caret_add_above",
    "ui_text_caret_add_below",
    "ui_text_caret_document_end",
    "ui_text_caret_document_start",
    "ui_text_caret_down",
    "ui_text_caret_left",
    "ui_text_caret_line_end",
    "ui_text_caret_line_start",
    "ui_text_caret_page_down",
    "ui_text_caret_page_up",
    "ui_text_caret_right",
    "ui_text_caret_up",
    "ui_text_caret_word_left",
    "ui_text_caret_word_right",
    "ui_text_clear_carets_and_selection",
    "ui_text_completion_accept",
    "ui_text_completion_query",
    "ui_text_completion_replace",
    "ui_text_dedent",
    "ui_text_delete",
    "ui_text_delete_all_to_right",
    "ui_text_delete_word",
    "ui_text_indent",
    "ui_text_newline",
    "ui_text_newline_above",
    "ui_text_newline_blank",
    "ui_text_scroll_down",
    "ui_text_scroll_up",
    "ui_text_select_all",
    "ui_text_select_word_under_caret",
    "ui_text_skip_selection_for_next_occurrence",
    "ui_text_submit",
    "ui_text_toggle_insert_mode",
    "ui_undo",
    "ui_unicode_start",
    "ui_up",
];

fn booted() -> InputMap {
    let mut map = InputMap::new();
    map.install_builtin_ui_actions();
    map
}

/// A key press carrying an explicit modifier state.
fn key(k: Key, shift: bool, ctrl: bool, alt: bool) -> InputEvent {
    InputEvent::Key {
        key: k,
        pressed: true,
        shift,
        ctrl,
        alt,
    }
}

/// A bare key press — no modifiers held.
fn bare(k: Key) -> InputEvent {
    key(k, false, false, false)
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** All 72 actions are present, and matching is
/// modifier-sensitive in both directions.
#[test]
fn input_map_default_ui_actions_complete() {
    let map = booted();

    // -- (a) the whole upstream table is seeded ------------------------------
    let mut seeded: Vec<&str> = map.actions().map(|s| s.as_str()).collect();
    seeded.sort_unstable();
    let mut expected: Vec<&str> = GODOT_UI_ACTIONS.to_vec();
    expected.sort_unstable();

    let missing: Vec<&&str> = expected.iter().filter(|a| !seeded.contains(a)).collect();
    assert!(
        missing.is_empty(),
        "these Godot ui_* actions are not seeded: {missing:?}"
    );
    let extra: Vec<&&str> = seeded.iter().filter(|a| !expected.contains(a)).collect();
    assert!(
        extra.is_empty(),
        "these actions are seeded but are not in Godot's table: {extra:?}"
    );
    assert_eq!(
        seeded.len(),
        72,
        "Godot's input_map.cpp defines exactly 72 ui_* actions",
    );

    // -- (b) a modifier combo fires its own action and nothing else ----------
    let ctrl_c = key(Key::C, false, true, false);
    assert!(
        map.event_matches_action(&ctrl_c, "ui_copy"),
        "Ctrl+C must fire ui_copy",
    );
    assert!(
        !map.event_matches_action(&ctrl_c, "ui_cancel"),
        "Ctrl+C must not fire ui_cancel — that is bound to Escape",
    );

    // The decisive half: a bare C must NOT fire the Ctrl-bound action. Without
    // the modifier requirement every letter shortcut would fire on the bare
    // letter, which is what makes a text field unusable.
    assert!(
        !map.event_matches_action(&bare(Key::C), "ui_copy"),
        "a bare C must not fire ui_copy — ui_copy requires Ctrl",
    );

    // -- (c) bare-key navigation still works ---------------------------------
    assert!(
        map.event_matches_action(&bare(Key::Home), "ui_home"),
        "a bare Home must fire ui_home",
    );

    // One physical key legitimately drives two actions; Godot resolves the
    // conflict by context (focus owner), not by binding.
    assert!(
        map.event_matches_action(&bare(Key::Left), "ui_left"),
        "a bare Left must fire ui_left",
    );
    assert!(
        map.event_matches_action(&bare(Key::Left), "ui_text_caret_left"),
        "a bare Left must also fire ui_text_caret_left",
    );

    // -- (d) the two actions this bead added ---------------------------------
    assert!(
        map.event_matches_action(&bare(Key::Menu), "ui_menu"),
        "the context-menu key must fire ui_menu",
    );
    assert!(
        map.event_matches_action(
            &key(Key::QuoteLeft, false, true, false),
            "ui_swap_input_direction"
        ),
        "Ctrl+Backquote must fire ui_swap_input_direction",
    );
    assert!(
        !map.event_matches_action(&bare(Key::QuoteLeft), "ui_swap_input_direction"),
        "a bare backquote must not fire ui_swap_input_direction",
    );
}

/// A Ctrl-modified arrow fires the word-wise caret action *and* the plain
/// navigation one, because a binding with no modifier requirement is satisfied
/// whatever is held. That is Godot's behaviour: `InputMap::event_get_action_status`
/// calls `is_match(event, false)` — non-exact — so the subset rule is the port,
/// not a shortcut.
#[test]
fn ctrl_arrow_satisfies_both_bare_and_modified_bindings() {
    let map = booted();
    let ctrl_left = key(Key::Left, false, true, false);

    assert!(
        map.event_matches_action(&ctrl_left, "ui_text_caret_word_left"),
        "Ctrl+Left must fire the word-wise caret action",
    );
    assert!(
        map.event_matches_action(&ctrl_left, "ui_left"),
        "Ctrl+Left also satisfies the bare Left binding (Godot's non-exact match)",
    );
    // But a modifier the binding does not ask for is still not a free pass in
    // the other direction: Shift+Left must not reach the Ctrl-bound action.
    assert!(
        !map.event_matches_action(
            &key(Key::Left, true, false, false),
            "ui_text_caret_word_left"
        ),
        "Shift+Left must not fire the Ctrl-bound word-wise action",
    );
}

/// Shift-modified defaults: `ui_focus_prev` is Shift+Tab, and a bare Tab must
/// go to `ui_focus_next` only. A table that dropped modifiers would make the
/// two indistinguishable and break every focus chain.
#[test]
fn shift_tab_and_tab_select_different_actions() {
    let map = booted();

    let tab = bare(Key::Tab);
    assert!(map.event_matches_action(&tab, "ui_focus_next"));
    assert!(
        !map.event_matches_action(&tab, "ui_focus_prev"),
        "a bare Tab must not fire ui_focus_prev — that needs Shift",
    );

    let shift_tab = key(Key::Tab, true, false, false);
    assert!(map.event_matches_action(&shift_tab, "ui_focus_prev"));
}

/// The two keys this bead added round-trip through the name table, so a routed
/// browser event ("Backquote"/"ContextMenu") resolves to them. Both spellings
/// the browser can report are accepted.
#[test]
fn added_keys_round_trip_through_the_name_table() {
    assert_eq!(Key::from_name("`"), Some(Key::QuoteLeft));
    assert_eq!(Key::from_name("Backquote"), Some(Key::QuoteLeft));
    assert_eq!(Key::from_name("Menu"), Some(Key::Menu));
    assert_eq!(Key::from_name("ContextMenu"), Some(Key::Menu));

    // `name()` is exhaustive over Key, so the new variants must be spelled there
    // too; round-tripping proves they are.
    assert_eq!(Key::from_name(Key::QuoteLeft.name()), Some(Key::QuoteLeft));
    assert_eq!(Key::from_name(Key::Menu.name()), Some(Key::Menu));

    // The backquote is NOT the apostrophe — Godot keeps QUOTELEFT and
    // APOSTROPHE distinct, and collapsing them would mis-route text input.
    assert_ne!(Key::from_name("`"), Key::from_name("'"));
}

/// Every seeded action carries Godot's 0.5 deadzone, including the two added
/// here — a zero deadzone would make stick drift fire UI navigation.
#[test]
fn every_builtin_action_uses_godots_default_deadzone() {
    let map = booted();
    for action in GODOT_UI_ACTIONS {
        assert!(map.has_action(action), "{action} must be seeded");
    }
    // `ui_accessibility_drag_and_drop` is seeded with no events upstream, so
    // presence — not bindings — is the right assertion for it.
    assert!(
        map.get_bindings("ui_accessibility_drag_and_drop")
            .is_some_and(|b| b.is_empty()),
        "ui_accessibility_drag_and_drop has no default events upstream",
    );
}

/// A sanity check that the binding *shape* survived: `ui_copy` keeps both of
/// Godot's events (Ctrl+C and Ctrl+Insert), not just the first.
#[test]
fn ui_copy_keeps_both_upstream_bindings() {
    let map = booted();
    let bindings = map.get_bindings("ui_copy").expect("ui_copy is seeded");
    assert!(
        bindings.contains(&ActionBinding::KeyWithModifiers(
            KeyCombo::new(Key::C).ctrl()
        )),
        "ui_copy must bind Ctrl+C, got {bindings:?}",
    );
    assert!(
        bindings.contains(&ActionBinding::KeyWithModifiers(
            KeyCombo::new(Key::Insert).ctrl()
        )),
        "ui_copy must also bind Ctrl+Insert, got {bindings:?}",
    );
}
