//! Window min/max clamp parity tests (pat-nzdz, pat-licp).
//!
//! Validates that Patina's window min_size / max_size constraint semantics
//! match Godot's documented behavior:
//!
//! - `min_size` and `max_size` default to `(0, 0)` meaning unconstrained
//! - `set_size()` clamps to active min/max constraints
//! - Changing min/max re-clamps the current size immediately
//! - `(0, 0)` sentinel disables the constraint on that axis
//!
//! Godot reference: `Window.min_size`, `Window.max_size`,
//! `DisplayServer.window_set_min_size()`, `DisplayServer.window_set_max_size()`.

use gdplatform::window::{HeadlessWindow, WindowConfig, WindowManager};

// ---------------------------------------------------------------------------
// Godot contract: min_size and max_size default to (0, 0) — unconstrained.
// (Window.min_size = Vector2i(0, 0), Window.max_size = Vector2i(0, 0))
// ---------------------------------------------------------------------------

#[test]
fn default_config_min_max_are_zero_unconstrained() {
    let config = WindowConfig::default();
    assert_eq!(config.min_size, (0, 0), "min_size defaults to (0,0) = unconstrained");
    assert_eq!(config.max_size, (0, 0), "max_size defaults to (0,0) = unconstrained");
}

#[test]
fn headless_window_min_max_default_to_zero() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert_eq!(wm.get_min_size(id), Some((0, 0)));
    assert_eq!(wm.get_max_size(id), Some((0, 0)));
}

// ---------------------------------------------------------------------------
// Godot contract: set_size with no constraints passes through unchanged.
// ---------------------------------------------------------------------------

#[test]
fn set_size_unconstrained_passes_through() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.set_size(id, 400, 300);
    assert_eq!(wm.get_size(id), Some((400, 300)));
}

// ---------------------------------------------------------------------------
// Godot contract: set_size clamps to min_size when below minimum.
// ---------------------------------------------------------------------------

#[test]
fn set_size_clamps_to_min_size() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 640, 480);

    // Try to shrink below minimum.
    wm.set_size(id, 320, 240);
    assert_eq!(
        wm.get_size(id),
        Some((640, 480)),
        "size should be clamped up to min_size"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: set_size clamps to max_size when above maximum.
// ---------------------------------------------------------------------------

#[test]
fn set_size_clamps_to_max_size() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_max_size(id, 1024, 768);

    // Try to grow above maximum.
    wm.set_size(id, 1920, 1080);
    assert_eq!(
        wm.get_size(id),
        Some((1024, 768)),
        "size should be clamped down to max_size"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: set_size clamps independently per axis.
// ---------------------------------------------------------------------------

#[test]
fn set_size_clamps_each_axis_independently() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 400, 300);
    wm.set_max_size(id, 1600, 900);

    // Width below min, height above max.
    wm.set_size(id, 200, 1200);
    assert_eq!(
        wm.get_size(id),
        Some((400, 900)),
        "each axis should be clamped independently"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: Setting min_size re-clamps current size upward.
// (DisplayServer.window_set_min_size triggers immediate resize)
// ---------------------------------------------------------------------------

#[test]
fn setting_min_size_reclamps_current_size() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(400, 300));
    assert_eq!(wm.get_size(id), Some((400, 300)));

    // Set min_size above current size.
    wm.set_min_size(id, 800, 600);
    assert_eq!(
        wm.get_size(id),
        Some((800, 600)),
        "current size must be bumped up to new min_size"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: Setting max_size re-clamps current size downward.
// (DisplayServer.window_set_max_size triggers immediate resize)
// ---------------------------------------------------------------------------

#[test]
fn setting_max_size_reclamps_current_size() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(1920, 1080));
    assert_eq!(wm.get_size(id), Some((1920, 1080)));

    // Set max_size below current size.
    wm.set_max_size(id, 1024, 768);
    assert_eq!(
        wm.get_size(id),
        Some((1024, 768)),
        "current size must be shrunk to new max_size"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: Size within [min, max] passes through unmodified.
// ---------------------------------------------------------------------------

#[test]
fn set_size_within_bounds_is_exact() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 320, 240);
    wm.set_max_size(id, 1920, 1080);

    wm.set_size(id, 1024, 768);
    assert_eq!(wm.get_size(id), Some((1024, 768)));

    // Exact min boundary.
    wm.set_size(id, 320, 240);
    assert_eq!(wm.get_size(id), Some((320, 240)));

    // Exact max boundary.
    wm.set_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((1920, 1080)));
}

// ---------------------------------------------------------------------------
// Godot contract: (0,0) sentinel removes constraint; axis becomes unbounded.
// ---------------------------------------------------------------------------

#[test]
fn zero_sentinel_removes_constraint() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    wm.set_min_size(id, 640, 480);
    wm.set_size(id, 100, 100);
    assert_eq!(wm.get_size(id), Some((640, 480)), "min active");

    // Remove min constraint.
    wm.set_min_size(id, 0, 0);
    wm.set_size(id, 100, 100);
    assert_eq!(
        wm.get_size(id),
        Some((100, 100)),
        "with min=(0,0) any size should be allowed"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: WindowConfig min/max are applied on create_window.
// ---------------------------------------------------------------------------

#[test]
fn config_min_size_clamps_initial_size() {
    let config = WindowConfig::new()
        .with_size(200, 150)
        .with_min_size(640, 480);
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);
    assert_eq!(
        wm.get_size(id),
        Some((640, 480)),
        "initial size should be clamped to config min_size"
    );
}

#[test]
fn config_max_size_clamps_initial_size() {
    let config = WindowConfig::new()
        .with_size(3840, 2160)
        .with_max_size(1920, 1080);
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);
    assert_eq!(
        wm.get_size(id),
        Some((1920, 1080)),
        "initial size should be clamped to config max_size"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: min/max on closed window are no-ops and return None.
// ---------------------------------------------------------------------------

#[test]
fn closed_window_min_max_are_inert() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.close(id);

    // Mutations should not panic.
    wm.set_min_size(id, 100, 100);
    wm.set_max_size(id, 200, 200);

    // Queries on closed window return None.
    assert_eq!(wm.get_min_size(id), None);
    assert_eq!(wm.get_max_size(id), None);
}

// ---------------------------------------------------------------------------
// Godot contract: min_size only constrains the specified axis when one
// component is 0. E.g. min_size=(640, 0) constrains only width.
// ---------------------------------------------------------------------------

#[test]
fn partial_min_size_constrains_one_axis() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 640, 0);

    wm.set_size(id, 320, 100);
    assert_eq!(
        wm.get_size(id),
        Some((640, 100)),
        "only width should be clamped; height unconstrained"
    );
}

#[test]
fn partial_max_size_constrains_one_axis() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_max_size(id, 0, 480);

    wm.set_size(id, 3840, 2160);
    assert_eq!(
        wm.get_size(id),
        Some((3840, 480)),
        "only height should be clamped; width unconstrained"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: builder methods round-trip correctly.
// ---------------------------------------------------------------------------

#[test]
fn config_builder_min_max_round_trip() {
    let config = WindowConfig::new()
        .with_min_size(320, 240)
        .with_max_size(1920, 1080);
    assert_eq!(config.min_size, (320, 240));
    assert_eq!(config.max_size, (1920, 1080));
}

// ---------------------------------------------------------------------------
// Godot contract: When min_size > max_size, min_size takes priority.
// (Godot guarantees the window is never smaller than min_size.)
// ---------------------------------------------------------------------------

#[test]
fn min_size_takes_priority_over_max_size() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    // Set max first, then min larger than max.
    wm.set_max_size(id, 500, 400);
    wm.set_min_size(id, 1024, 768);

    // min > max: min wins, so size should be clamped up to min.
    assert_eq!(
        wm.get_size(id),
        Some((1024, 768)),
        "min_size must take priority when min > max"
    );

    // Attempting to set_size between min and max should still yield min.
    wm.set_size(id, 600, 500);
    assert_eq!(
        wm.get_size(id),
        Some((1024, 768)),
        "set_size below min_size must clamp up even when max < min"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: Config with both min and max applied at creation.
// ---------------------------------------------------------------------------

#[test]
fn config_both_min_and_max_clamp_initial_size() {
    // Size between min and max — should pass through.
    let config = WindowConfig::new()
        .with_size(800, 600)
        .with_min_size(640, 480)
        .with_max_size(1920, 1080);
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);
    assert_eq!(
        wm.get_size(id),
        Some((800, 600)),
        "size within [min, max] should be unchanged"
    );
}

#[test]
fn config_min_max_conflict_min_wins_at_creation() {
    // min > max in config: min should win.
    let config = WindowConfig::new()
        .with_size(500, 400)
        .with_min_size(1024, 768)
        .with_max_size(800, 600);
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);
    assert_eq!(
        wm.get_size(id),
        Some((1024, 768)),
        "min wins over max at window creation"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: Removing max_size constraint (reset to 0,0) while
// min_size remains active — min still enforced, max no longer limits.
// ---------------------------------------------------------------------------

#[test]
fn remove_max_constraint_while_min_remains() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 640, 480);
    wm.set_max_size(id, 1024, 768);

    // Remove max constraint.
    wm.set_max_size(id, 0, 0);

    // Should be able to exceed the old max.
    wm.set_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((1920, 1080)), "max removed, no upper limit");

    // Min should still be enforced.
    wm.set_size(id, 100, 100);
    assert_eq!(wm.get_size(id), Some((640, 480)), "min still enforced after max removed");
}

// ---------------------------------------------------------------------------
// Godot contract: Removing min_size constraint (reset to 0,0) while
// max_size remains active — max still enforced, min no longer limits.
// ---------------------------------------------------------------------------

#[test]
fn remove_min_constraint_while_max_remains() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 640, 480);
    wm.set_max_size(id, 1024, 768);

    // Remove min constraint.
    wm.set_min_size(id, 0, 0);

    // Should be able to go below the old min.
    wm.set_size(id, 100, 100);
    assert_eq!(wm.get_size(id), Some((100, 100)), "min removed, no lower limit");

    // Max should still be enforced.
    wm.set_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((1024, 768)), "max still enforced after min removed");
}

// ---------------------------------------------------------------------------
// Godot contract: Multiple sequential constraint updates converge correctly.
// ---------------------------------------------------------------------------

#[test]
fn sequential_constraint_updates_converge() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    // Phase 1: Tight constraints.
    wm.set_min_size(id, 700, 500);
    wm.set_max_size(id, 900, 700);
    assert_eq!(wm.get_size(id), Some((800, 600)));

    // Phase 2: Tighten max below current size.
    wm.set_max_size(id, 750, 550);
    assert_eq!(
        wm.get_size(id),
        Some((750, 550)),
        "reclamp should shrink to new max"
    );

    // Phase 3: Raise min above current size.
    wm.set_min_size(id, 800, 600);
    assert_eq!(
        wm.get_size(id),
        Some((800, 600)),
        "reclamp should grow to new min (min > max, min wins)"
    );

    // Phase 4: Relax both constraints entirely.
    wm.set_min_size(id, 0, 0);
    wm.set_max_size(id, 0, 0);
    wm.set_size(id, 3840, 2160);
    assert_eq!(
        wm.get_size(id),
        Some((3840, 2160)),
        "fully unconstrained after resetting both to (0,0)"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: Partial axis constraints interact correctly.
// E.g. min_size=(640, 0) + max_size=(0, 480) constrains width-min and
// height-max independently.
// ---------------------------------------------------------------------------

#[test]
fn mixed_partial_axis_constraints() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    // Constrain only width-min and only height-max.
    wm.set_min_size(id, 640, 0);
    wm.set_max_size(id, 0, 480);

    wm.set_size(id, 100, 1000);
    assert_eq!(
        wm.get_size(id),
        Some((640, 480)),
        "width clamped up to min, height clamped down to max"
    );

    wm.set_size(id, 1920, 200);
    assert_eq!(
        wm.get_size(id),
        Some((1920, 200)),
        "width above min passes through, height below max passes through"
    );
}
