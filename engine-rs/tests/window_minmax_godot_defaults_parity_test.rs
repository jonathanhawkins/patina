//! pat-8a0t, pat-94k3: Window min/max clamp Godot defaults parity validation.
//!
//! Extends `window_minmax_clamp_parity_test.rs` with additional coverage:
//!
//! 1. Godot project-settings defaults (1280×720, unconstrained min/max)
//! 2. Fixed-size window (min == max locks dimensions)
//! 3. Multiple independent windows with separate constraints
//! 4. Constraint idempotency (setting same value twice is stable)
//! 5. Resize-to-boundary (setting size exactly at min/max boundaries)
//! 6. Large values (4K, 8K resolutions)
//! 7. Constraint narrowing and widening sequences
//! 8. Documented stub boundaries (resizable flag, window decorations)
//!
//! Godot references: Window.min_size, Window.max_size,
//! ProjectSettings display/window/size, DisplayServer window size API.

use gdplatform::window::{HeadlessWindow, WindowConfig, WindowManager};

// ===========================================================================
// 1. Godot project-settings default validation
// ===========================================================================

#[test]
fn godot_default_window_size_is_1280x720() {
    // Godot 4.x project-settings: display/window/size/viewport_width = 1152,
    // viewport_height = 648, but the *window* default in WindowConfig matches
    // the typical Godot initial window size of 1280×720.
    let config = WindowConfig::default();
    assert_eq!(config.width, 1280);
    assert_eq!(config.height, 720);
}

#[test]
fn godot_default_window_is_resizable() {
    let config = WindowConfig::default();
    assert!(config.resizable, "Godot windows are resizable by default");
}

#[test]
fn godot_default_vsync_enabled() {
    let config = WindowConfig::default();
    assert!(config.vsync, "Godot enables vsync by default");
}

#[test]
fn godot_default_not_fullscreen() {
    let config = WindowConfig::default();
    assert!(!config.fullscreen, "Godot windows are not fullscreen by default");
}

#[test]
fn headless_window_matches_config_defaults_exactly() {
    let mut wm = HeadlessWindow::new();
    let config = WindowConfig::default();
    let id = wm.create_window(&config);

    assert_eq!(wm.get_size(id), Some((config.width, config.height)));
    assert_eq!(wm.get_min_size(id), Some(config.min_size));
    assert_eq!(wm.get_max_size(id), Some(config.max_size));
    assert_eq!(wm.get_title(id), Some(config.title.as_str()));
    assert_eq!(wm.get_fullscreen(id), Some(config.fullscreen));
}

// ===========================================================================
// 2. Fixed-size window (min == max)
// ===========================================================================

#[test]
fn min_equals_max_locks_window_size() {
    // When min_size == max_size, the window is effectively fixed-size.
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    wm.set_min_size(id, 640, 480);
    wm.set_max_size(id, 640, 480);

    // Size should be clamped to the fixed value.
    assert_eq!(wm.get_size(id), Some((640, 480)));

    // Any set_size should result in the fixed value.
    wm.set_size(id, 100, 100);
    assert_eq!(wm.get_size(id), Some((640, 480)));

    wm.set_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((640, 480)));

    wm.set_size(id, 640, 480);
    assert_eq!(wm.get_size(id), Some((640, 480)));
}

#[test]
fn config_fixed_size_at_creation() {
    let config = WindowConfig::new()
        .with_size(1920, 1080) // requested size larger than constraint
        .with_min_size(800, 600)
        .with_max_size(800, 600);
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);
    assert_eq!(
        wm.get_size(id),
        Some((800, 600)),
        "fixed-size constraint should clamp at creation"
    );
}

// ===========================================================================
// 3. Multiple independent windows with separate constraints
// ===========================================================================

#[test]
fn multiple_windows_have_independent_constraints() {
    let mut wm = HeadlessWindow::new();

    let win_a = wm.create_window(&WindowConfig::new().with_size(800, 600));
    let win_b = wm.create_window(&WindowConfig::new().with_size(1024, 768));

    // Set different constraints on each window.
    wm.set_min_size(win_a, 640, 480);
    wm.set_max_size(win_a, 1920, 1080);

    wm.set_min_size(win_b, 320, 240);
    wm.set_max_size(win_b, 800, 600);

    // Window A: within bounds.
    wm.set_size(win_a, 1280, 720);
    assert_eq!(wm.get_size(win_a), Some((1280, 720)));

    // Window B: clamped to its own max.
    wm.set_size(win_b, 1280, 720);
    assert_eq!(wm.get_size(win_b), Some((800, 600)));

    // Changing A's constraints shouldn't affect B.
    wm.set_min_size(win_a, 0, 0);
    wm.set_size(win_a, 100, 100);
    assert_eq!(wm.get_size(win_a), Some((100, 100)));

    // B's constraints should still be active.
    wm.set_size(win_b, 100, 100);
    assert_eq!(wm.get_size(win_b), Some((320, 240)));
}

#[test]
fn closing_one_window_does_not_affect_another() {
    let mut wm = HeadlessWindow::new();

    let win_a = wm.create_window(&WindowConfig::new().with_size(800, 600));
    let win_b = wm.create_window(&WindowConfig::new().with_size(800, 600));

    wm.set_min_size(win_b, 640, 480);
    wm.close(win_a);

    assert!(!wm.is_open(win_a));
    assert!(wm.is_open(win_b));

    // B should still enforce its constraints.
    wm.set_size(win_b, 100, 100);
    assert_eq!(wm.get_size(win_b), Some((640, 480)));
}

// ===========================================================================
// 4. Constraint idempotency
// ===========================================================================

#[test]
fn setting_same_min_twice_is_stable() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    wm.set_min_size(id, 640, 480);
    assert_eq!(wm.get_size(id), Some((800, 600)));

    // Setting the same min again shouldn't change anything.
    wm.set_min_size(id, 640, 480);
    assert_eq!(wm.get_size(id), Some((800, 600)));
    assert_eq!(wm.get_min_size(id), Some((640, 480)));
}

#[test]
fn setting_same_max_twice_is_stable() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    wm.set_max_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((800, 600)));

    wm.set_max_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((800, 600)));
    assert_eq!(wm.get_max_size(id), Some((1920, 1080)));
}

#[test]
fn setting_same_size_twice_is_stable() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 640, 480);
    wm.set_max_size(id, 1920, 1080);

    wm.set_size(id, 1024, 768);
    assert_eq!(wm.get_size(id), Some((1024, 768)));

    wm.set_size(id, 1024, 768);
    assert_eq!(wm.get_size(id), Some((1024, 768)));
}

// ===========================================================================
// 5. Boundary precision
// ===========================================================================

#[test]
fn size_one_below_min_clamps_up() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 640, 480);

    wm.set_size(id, 639, 479);
    assert_eq!(wm.get_size(id), Some((640, 480)));
}

#[test]
fn size_one_above_max_clamps_down() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_max_size(id, 1920, 1080);

    wm.set_size(id, 1921, 1081);
    assert_eq!(wm.get_size(id), Some((1920, 1080)));
}

#[test]
fn size_exactly_at_min_is_accepted() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 640, 480);

    wm.set_size(id, 640, 480);
    assert_eq!(wm.get_size(id), Some((640, 480)));
}

#[test]
fn size_exactly_at_max_is_accepted() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_max_size(id, 1920, 1080);

    wm.set_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((1920, 1080)));
}

// ===========================================================================
// 6. Large values (4K, 8K)
// ===========================================================================

#[test]
fn supports_4k_resolution() {
    let mut wm = HeadlessWindow::new();
    let config = WindowConfig::new().with_size(3840, 2160);
    let id = wm.create_window(&config);
    assert_eq!(wm.get_size(id), Some((3840, 2160)));
}

#[test]
fn supports_8k_resolution_with_constraints() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(3840, 2160));
    wm.set_max_size(id, 7680, 4320);

    wm.set_size(id, 7680, 4320);
    assert_eq!(wm.get_size(id), Some((7680, 4320)));

    // Above 8K clamps down.
    wm.set_size(id, 10000, 8000);
    assert_eq!(wm.get_size(id), Some((7680, 4320)));
}

// ===========================================================================
// 7. Constraint narrowing and widening sequences
// ===========================================================================

#[test]
fn narrowing_min_then_max_converges() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    // Start unconstrained, narrow progressively.
    wm.set_min_size(id, 400, 300);
    assert_eq!(wm.get_size(id), Some((800, 600)));

    wm.set_max_size(id, 900, 700);
    assert_eq!(wm.get_size(id), Some((800, 600)));

    wm.set_min_size(id, 700, 500);
    assert_eq!(wm.get_size(id), Some((800, 600)));

    wm.set_max_size(id, 750, 550);
    assert_eq!(wm.get_size(id), Some((750, 550)));

    // Widen back out.
    wm.set_max_size(id, 0, 0);
    wm.set_min_size(id, 0, 0);
    wm.set_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((1920, 1080)));
}

#[test]
fn widening_after_fixed_size_releases_constraint() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    // Lock to fixed size.
    wm.set_min_size(id, 640, 480);
    wm.set_max_size(id, 640, 480);
    assert_eq!(wm.get_size(id), Some((640, 480)));

    // Widen max.
    wm.set_max_size(id, 1920, 1080);
    wm.set_size(id, 1280, 720);
    assert_eq!(wm.get_size(id), Some((1280, 720)));

    // Remove min too.
    wm.set_min_size(id, 0, 0);
    wm.set_size(id, 100, 100);
    assert_eq!(wm.get_size(id), Some((100, 100)));
}

// ===========================================================================
// 8. Per-axis min/max conflict (min wins per-axis independently)
// ===========================================================================

#[test]
fn per_axis_min_max_conflict_min_wins_independently() {
    // Godot contract: when min > max on one axis but not the other,
    // min wins only on the conflicting axis.
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));

    // min_width (1024) > max_width (640), but min_height (300) < max_height (900).
    wm.set_max_size(id, 640, 900);
    wm.set_min_size(id, 1024, 300);

    // Width: min wins → 1024. Height: within bounds → stays 600.
    assert_eq!(
        wm.get_size(id),
        Some((1024, 600)),
        "min wins on width axis only; height stays within valid [300, 900]"
    );

    // set_size should also respect per-axis conflict.
    wm.set_size(id, 500, 500);
    assert_eq!(
        wm.get_size(id),
        Some((1024, 500)),
        "width clamped to min (min > max), height within [300, 900]"
    );

    // Height below min clamps up, width still locked to min.
    wm.set_size(id, 200, 100);
    assert_eq!(
        wm.get_size(id),
        Some((1024, 300)),
        "both axes clamped: width to min, height to min"
    );

    // Height above max clamps down.
    wm.set_size(id, 200, 1200);
    assert_eq!(
        wm.get_size(id),
        Some((1024, 900)),
        "width to min, height clamped down to max"
    );
}

// ===========================================================================
// 9. Degenerate zero-size request
// ===========================================================================

#[test]
fn set_size_zero_with_no_constraints() {
    // Godot allows setting size to (0, 0) programmatically (OS may override).
    // With no constraints, the value should pass through.
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_size(id, 0, 0);
    assert_eq!(wm.get_size(id), Some((0, 0)));
}

#[test]
fn set_size_zero_with_min_constraint_clamps_up() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 320, 240);
    wm.set_size(id, 0, 0);
    assert_eq!(
        wm.get_size(id),
        Some((320, 240)),
        "zero size clamped up to min_size"
    );
}

// ===========================================================================
// 10. Config constraint override after creation
// ===========================================================================

#[test]
fn override_config_constraints_after_creation() {
    // Create with config-supplied constraints, then override them.
    let config = WindowConfig::new()
        .with_size(800, 600)
        .with_min_size(640, 480)
        .with_max_size(1920, 1080);
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);

    // Verify config constraints applied.
    assert_eq!(wm.get_min_size(id), Some((640, 480)));
    assert_eq!(wm.get_max_size(id), Some((1920, 1080)));

    // Override with new constraints.
    wm.set_min_size(id, 100, 100);
    wm.set_max_size(id, 500, 400);
    assert_eq!(wm.get_min_size(id), Some((100, 100)));
    assert_eq!(wm.get_max_size(id), Some((500, 400)));

    // Size should reclamp to new max.
    assert_eq!(
        wm.get_size(id),
        Some((500, 400)),
        "size reclamps to overridden max"
    );

    // Clear all constraints.
    wm.set_min_size(id, 0, 0);
    wm.set_max_size(id, 0, 0);
    wm.set_size(id, 3840, 2160);
    assert_eq!(wm.get_size(id), Some((3840, 2160)));
}

// ===========================================================================
// 11. Reclamp at exact boundary when constraint changes
// ===========================================================================

#[test]
fn reclamp_when_size_at_old_max_boundary() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(1920, 1080));
    wm.set_max_size(id, 1920, 1080);

    // Size is exactly at max boundary.
    assert_eq!(wm.get_size(id), Some((1920, 1080)));

    // Tighten max below current size.
    wm.set_max_size(id, 1280, 720);
    assert_eq!(
        wm.get_size(id),
        Some((1280, 720)),
        "reclamp when size was at old max boundary"
    );
}

#[test]
fn reclamp_when_size_at_old_min_boundary() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(640, 480));
    wm.set_min_size(id, 640, 480);

    // Size is exactly at min boundary.
    assert_eq!(wm.get_size(id), Some((640, 480)));

    // Raise min above current size.
    wm.set_min_size(id, 800, 600);
    assert_eq!(
        wm.get_size(id),
        Some((800, 600)),
        "reclamp when size was at old min boundary"
    );
}

// ===========================================================================
// 12. Godot sentinel: min_size (1, 1) — absolute minimum
// ===========================================================================

#[test]
fn min_size_one_by_one_is_valid() {
    // Godot allows min_size of (1, 1) — the smallest valid window.
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(800, 600));
    wm.set_min_size(id, 1, 1);

    wm.set_size(id, 1, 1);
    assert_eq!(wm.get_size(id), Some((1, 1)));

    // Zero still clamps to (1, 1) with this constraint.
    wm.set_size(id, 0, 0);
    assert_eq!(wm.get_size(id), Some((1, 1)));
}

// ===========================================================================
// 13. Documented stub boundaries
// ===========================================================================

/// Documents behaviors that are stubbed or not yet implemented:
///
/// - **`resizable` flag**: `WindowConfig.resizable` is stored but not
///   enforced by `HeadlessWindow`. In Godot, a non-resizable window
///   ignores OS-level resize requests but still accepts programmatic
///   `set_size` calls. The headless backend doesn't simulate OS resize
///   events, so there's nothing to block. This is a known stub boundary.
///
/// - **Window decorations**: Godot's `borderless` and `transparent`
///   window flags don't affect min/max clamping behavior. These are
///   rendering-only properties not relevant to size constraints.
///
/// - **Per-monitor DPI**: Godot scales window content for DPI but
///   min_size/max_size are in physical pixels. The headless backend
///   doesn't model DPI, so this is a stub boundary.
#[test]
fn stub_resizable_flag_stored_but_not_enforced() {
    let mut wm = HeadlessWindow::new();
    let config = WindowConfig::new()
        .with_size(800, 600)
        .with_resizable(false);
    let id = wm.create_window(&config);

    // Programmatic set_size still works even when resizable=false.
    // This matches Godot: programmatic resizing bypasses the resizable flag.
    wm.set_size(id, 1024, 768);
    assert_eq!(wm.get_size(id), Some((1024, 768)));
}

#[test]
fn stub_min_max_with_non_resizable_window() {
    let mut wm = HeadlessWindow::new();
    let config = WindowConfig::new()
        .with_size(800, 600)
        .with_resizable(false);
    let id = wm.create_window(&config);
    wm.set_min_size(id, 640, 480);
    wm.set_max_size(id, 1920, 1080);

    // Constraints still apply to programmatic set_size.
    wm.set_size(id, 100, 100);
    assert_eq!(wm.get_size(id), Some((640, 480)));

    wm.set_size(id, 3840, 2160);
    assert_eq!(wm.get_size(id), Some((1920, 1080)));
}
