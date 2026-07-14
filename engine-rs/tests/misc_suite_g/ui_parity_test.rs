//! pat-nri4p: Integration tests for VBoxContainer, HBoxContainer, and
//! GridContainer layout arrangement.
//!
//! These tests verify that `control::arrange_container` distributes
//! children's positions and sizes consistent with Godot's container
//! arrangement rules.
//!
//! pat-yz7rj: Theme resource loading and style property lookup parity.

use gdcore::math::{Color, Vector2};
use gdscene::control::{
    self, apply_anchor_preset, arrange_container, get_position, get_size, resolve_control_layout,
    set_anchor_bottom, set_anchor_left, set_anchor_right, set_anchor_top, set_columns,
    set_custom_minimum_size, set_grow_direction_h, set_grow_direction_v, set_h_size_flags,
    set_offset_bottom, set_offset_left, set_offset_right, set_offset_top, set_separation,
    set_v_size_flags, AnchorPreset, GrowDirection, SizeFlags,
};
use gdscene::node::Node;
use gdscene::node::NodeId;
use gdscene::theme::{Theme, ThemeDB, ThemePropertyType};
use gdscene::SceneTree;
use gdvariant::Variant;

fn add_child(tree: &mut SceneTree, parent: NodeId, name: &str, class: &str) -> NodeId {
    let node = Node::new(name, class);
    tree.add_child(parent, node).unwrap()
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn approx_vec(a: Vector2, b: Vector2) -> bool {
    approx(a.x, b.x) && approx(a.y, b.y)
}

#[test]
fn container_arrangement_vbox_stacks_children_with_separation() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let vbox = add_child(&mut tree, root, "VBox", "VBoxContainer");
    set_separation(&mut tree, vbox, 4);

    let a = add_child(&mut tree, vbox, "A", "Control");
    let b = add_child(&mut tree, vbox, "B", "Control");
    let c = add_child(&mut tree, vbox, "C", "Control");
    set_custom_minimum_size(&mut tree, a, Vector2::new(50.0, 20.0));
    set_custom_minimum_size(&mut tree, b, Vector2::new(50.0, 30.0));
    set_custom_minimum_size(&mut tree, c, Vector2::new(50.0, 40.0));

    arrange_container(&mut tree, vbox, Vector2::new(100.0, 200.0));

    assert!(approx_vec(get_position(&tree, a), Vector2::new(0.0, 0.0)));
    assert!(approx_vec(get_position(&tree, b), Vector2::new(0.0, 24.0)));
    assert!(approx_vec(get_position(&tree, c), Vector2::new(0.0, 58.0)));

    // Every child's width equals the container width.
    assert!(approx(get_size(&tree, a).x, 100.0));
    assert!(approx(get_size(&tree, b).x, 100.0));
    assert!(approx(get_size(&tree, c).x, 100.0));

    // Heights match each child's minimum when no Expand flags are set.
    assert!(approx(get_size(&tree, a).y, 20.0));
    assert!(approx(get_size(&tree, b).y, 30.0));
    assert!(approx(get_size(&tree, c).y, 40.0));
}

#[test]
fn container_arrangement_vbox_expand_distributes_extra_height() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let vbox = add_child(&mut tree, root, "VBox", "VBoxContainer");
    set_separation(&mut tree, vbox, 0);

    let a = add_child(&mut tree, vbox, "A", "Control");
    let b = add_child(&mut tree, vbox, "B", "Control");
    set_custom_minimum_size(&mut tree, a, Vector2::new(10.0, 20.0));
    set_custom_minimum_size(&mut tree, b, Vector2::new(10.0, 30.0));
    set_v_size_flags(&mut tree, b, SizeFlags::Expand);

    arrange_container(&mut tree, vbox, Vector2::new(100.0, 200.0));

    // A keeps its minimum height; B absorbs the 150 extra pixels.
    assert!(approx(get_size(&tree, a).y, 20.0));
    assert!(approx(get_size(&tree, b).y, 180.0));
    assert!(approx_vec(get_position(&tree, a), Vector2::new(0.0, 0.0)));
    assert!(approx_vec(get_position(&tree, b), Vector2::new(0.0, 20.0)));
}

#[test]
fn container_arrangement_hbox_distributes_width() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let hbox = add_child(&mut tree, root, "HBox", "HBoxContainer");
    set_separation(&mut tree, hbox, 6);

    let a = add_child(&mut tree, hbox, "A", "Control");
    let b = add_child(&mut tree, hbox, "B", "Control");
    set_custom_minimum_size(&mut tree, a, Vector2::new(40.0, 25.0));
    set_custom_minimum_size(&mut tree, b, Vector2::new(60.0, 25.0));

    arrange_container(&mut tree, hbox, Vector2::new(500.0, 80.0));

    assert!(approx_vec(get_position(&tree, a), Vector2::new(0.0, 0.0)));
    assert!(approx_vec(get_position(&tree, b), Vector2::new(46.0, 0.0)));

    // Children fill the container height when Fill is used (default).
    assert!(approx(get_size(&tree, a).y, 80.0));
    assert!(approx(get_size(&tree, b).y, 80.0));
    assert!(approx(get_size(&tree, a).x, 40.0));
    assert!(approx(get_size(&tree, b).x, 60.0));
}

#[test]
fn container_arrangement_grid_positions_rows_and_columns() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let grid = add_child(&mut tree, root, "Grid", "GridContainer");
    set_columns(&mut tree, grid, 2);
    set_separation(&mut tree, grid, 5);

    // 3 children → 2 rows × 2 cols (last slot empty).
    let a = add_child(&mut tree, grid, "A", "Control");
    let b = add_child(&mut tree, grid, "B", "Control");
    let c = add_child(&mut tree, grid, "C", "Control");
    set_custom_minimum_size(&mut tree, a, Vector2::new(30.0, 40.0));
    set_custom_minimum_size(&mut tree, b, Vector2::new(50.0, 20.0));
    set_custom_minimum_size(&mut tree, c, Vector2::new(10.0, 60.0));

    arrange_container(&mut tree, grid, Vector2::new(200.0, 200.0));

    // Column widths: col0 = max(30,10) = 30, col1 = max(50) = 50.
    // Row heights: row0 = max(40,20) = 40, row1 = max(60) = 60.
    assert!(approx_vec(get_position(&tree, a), Vector2::new(0.0, 0.0)));
    assert!(approx_vec(get_position(&tree, b), Vector2::new(35.0, 0.0)));
    assert!(approx_vec(get_position(&tree, c), Vector2::new(0.0, 45.0)));

    assert!(approx_vec(get_size(&tree, a), Vector2::new(30.0, 40.0)));
    assert!(approx_vec(get_size(&tree, b), Vector2::new(50.0, 40.0)));
    assert!(approx_vec(get_size(&tree, c), Vector2::new(30.0, 60.0)));
}

#[test]
fn container_arrangement_ignores_non_container_class() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = add_child(&mut tree, root, "Panel", "Panel");
    let a = add_child(&mut tree, panel, "A", "Control");
    set_custom_minimum_size(&mut tree, a, Vector2::new(10.0, 10.0));

    arrange_container(&mut tree, panel, Vector2::new(100.0, 100.0));

    // The panel's own size is still written, but no child layout is applied.
    assert_eq!(control::get_size(&tree, panel), Vector2::new(100.0, 100.0));
    assert_eq!(get_position(&tree, a), Vector2::ZERO);
    assert_eq!(get_size(&tree, a), Vector2::ZERO);
}

// ---------------------------------------------------------------------------
// Control anchor / margin / size-flags solver (pat-prbtw)
// ---------------------------------------------------------------------------

/// Oracle: with default anchors (all 0), explicit offsets fully determine the
/// rect. `resolve_control_layout` writes position from top-left offsets and
/// size from offset_right/bottom minus offset_left/top.
#[test]
fn control_layout_default_anchors_use_offsets_directly() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    set_offset_left(&mut tree, ctrl, 10.0);
    set_offset_top(&mut tree, ctrl, 20.0);
    set_offset_right(&mut tree, ctrl, 110.0);
    set_offset_bottom(&mut tree, ctrl, 70.0);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(800.0, 600.0));

    assert!(approx_vec(get_position(&tree, ctrl), Vector2::new(10.0, 20.0)));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(100.0, 50.0)));
}

/// Oracle: the FullRect preset (anchors 0,0,1,1) with zero offsets fills the
/// parent exactly. Mirrors Godot's `PRESET_FULL_RECT`.
#[test]
fn control_layout_full_rect_preset_fills_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    apply_anchor_preset(&mut tree, ctrl, AnchorPreset::FullRect);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(400.0, 300.0));

    assert!(approx_vec(get_position(&tree, ctrl), Vector2::ZERO));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(400.0, 300.0)));
}

/// Oracle: center-anchored rect (all anchors 0.5) with symmetric offsets is
/// positioned around the parent center with size = offset_right - offset_left.
#[test]
fn control_layout_center_preset_with_offsets() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    apply_anchor_preset(&mut tree, ctrl, AnchorPreset::Center);
    set_offset_left(&mut tree, ctrl, -40.0);
    set_offset_top(&mut tree, ctrl, -25.0);
    set_offset_right(&mut tree, ctrl, 40.0);
    set_offset_bottom(&mut tree, ctrl, 25.0);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(200.0, 100.0));

    // Parent center = (100, 50); half-extents (40, 25) → top-left (60, 25).
    assert!(approx_vec(get_position(&tree, ctrl), Vector2::new(60.0, 25.0)));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(80.0, 50.0)));
}

/// Oracle: BottomRight preset pins all four anchors to (1,1), so the rect's
/// origin is the parent's bottom-right corner plus the (negative) offsets.
#[test]
fn control_layout_bottom_right_preset() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    apply_anchor_preset(&mut tree, ctrl, AnchorPreset::BottomRight);
    set_offset_left(&mut tree, ctrl, -80.0);
    set_offset_top(&mut tree, ctrl, -30.0);
    set_offset_right(&mut tree, ctrl, 0.0);
    set_offset_bottom(&mut tree, ctrl, 0.0);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(500.0, 400.0));

    assert!(approx_vec(
        get_position(&tree, ctrl),
        Vector2::new(420.0, 370.0)
    ));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(80.0, 30.0)));
}

/// Oracle: TopWide preset gives anchors (0,0,1,0) — full parent width, height
/// fully driven by offset_bottom.
#[test]
fn control_layout_top_wide_preset() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    apply_anchor_preset(&mut tree, ctrl, AnchorPreset::TopWide);
    set_offset_bottom(&mut tree, ctrl, 48.0);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(640.0, 480.0));

    assert!(approx_vec(get_position(&tree, ctrl), Vector2::ZERO));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(640.0, 48.0)));
}

/// Oracle: when the anchor-derived rect is smaller than `custom_minimum_size`,
/// the rect grows outward on each axis per `grow_horizontal`/`grow_vertical`.
/// `Begin` grows left/up, `End` grows right/down, `Both` splits evenly.
#[test]
fn control_layout_min_size_grow_directions() {
    // Shared setup: TopLeft preset → anchor-rect is zero-size at (0,0),
    // so custom_minimum_size drives the full grow delta (= 40 × 20).
    fn setup(grow_h: GrowDirection, grow_v: GrowDirection) -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
        // Explicit zero anchors (TopLeft preset) so we don't inherit whatever
        // default future versions might adopt.
        set_anchor_left(&mut tree, ctrl, 0.0);
        set_anchor_top(&mut tree, ctrl, 0.0);
        set_anchor_right(&mut tree, ctrl, 0.0);
        set_anchor_bottom(&mut tree, ctrl, 0.0);
        set_custom_minimum_size(&mut tree, ctrl, Vector2::new(40.0, 20.0));
        set_grow_direction_h(&mut tree, ctrl, grow_h);
        set_grow_direction_v(&mut tree, ctrl, grow_v);
        (tree, ctrl)
    }

    // Grow End (default): rect extends right/down from origin.
    let (mut tree, ctrl) = setup(GrowDirection::End, GrowDirection::End);
    resolve_control_layout(&mut tree, ctrl, Vector2::new(200.0, 200.0));
    assert!(approx_vec(get_position(&tree, ctrl), Vector2::ZERO));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(40.0, 20.0)));

    // Grow Begin: rect extends left/up, so origin moves to negative coords.
    let (mut tree, ctrl) = setup(GrowDirection::Begin, GrowDirection::Begin);
    resolve_control_layout(&mut tree, ctrl, Vector2::new(200.0, 200.0));
    assert!(approx_vec(
        get_position(&tree, ctrl),
        Vector2::new(-40.0, -20.0)
    ));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(40.0, 20.0)));

    // Grow Both: rect centered on anchor point.
    let (mut tree, ctrl) = setup(GrowDirection::Both, GrowDirection::Both);
    resolve_control_layout(&mut tree, ctrl, Vector2::new(200.0, 200.0));
    assert!(approx_vec(
        get_position(&tree, ctrl),
        Vector2::new(-20.0, -10.0)
    ));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(40.0, 20.0)));
}

/// Oracle: an asymmetric anchor (left=0, right=0.5) with offsets acts as a
/// margin — the left edge is pinned at `offset_left`, and the right edge is
/// `parent.x * 0.5 + offset_right`. Matches Godot's pair-wise anchor/offset
/// combination rule.
#[test]
fn control_layout_asymmetric_anchors_with_margins() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    set_anchor_left(&mut tree, ctrl, 0.0);
    set_anchor_top(&mut tree, ctrl, 0.25);
    set_anchor_right(&mut tree, ctrl, 0.5);
    set_anchor_bottom(&mut tree, ctrl, 0.75);
    set_offset_left(&mut tree, ctrl, 10.0);
    set_offset_top(&mut tree, ctrl, 5.0);
    set_offset_right(&mut tree, ctrl, -10.0);
    set_offset_bottom(&mut tree, ctrl, -5.0);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(400.0, 200.0));

    // x1 = 400 * 0.0 + 10 = 10,  x2 = 400 * 0.5 - 10 = 190  → pos.x=10, w=180
    // y1 = 200 * 0.25 + 5 = 55, y2 = 200 * 0.75 - 5 = 145   → pos.y=55, h=90
    assert!(approx_vec(get_position(&tree, ctrl), Vector2::new(10.0, 55.0)));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(180.0, 90.0)));
}

/// Oracle: when an anchor-derived rect is already larger than the minimum
/// size, `resolve_control_layout` leaves it alone — grow direction only
/// kicks in when the rect is *smaller* than the effective minimum.
#[test]
fn control_layout_grow_direction_no_op_when_rect_exceeds_min() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    apply_anchor_preset(&mut tree, ctrl, AnchorPreset::FullRect);
    set_custom_minimum_size(&mut tree, ctrl, Vector2::new(10.0, 10.0));
    // Grow Begin would push pos into negatives if mis-applied.
    set_grow_direction_h(&mut tree, ctrl, GrowDirection::Begin);
    set_grow_direction_v(&mut tree, ctrl, GrowDirection::Begin);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(300.0, 200.0));

    assert!(approx_vec(get_position(&tree, ctrl), Vector2::ZERO));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(300.0, 200.0)));
}

/// Oracle: when only one axis is below its minimum, grow direction is
/// applied on that axis alone — the satisfied axis keeps its anchor-derived
/// position and size.
#[test]
fn control_layout_min_size_per_axis_independent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    // Anchor rect: x from 0..100 (width=100), y collapsed to 0..0.
    apply_anchor_preset(&mut tree, ctrl, AnchorPreset::TopLeft);
    set_offset_right(&mut tree, ctrl, 100.0);
    set_custom_minimum_size(&mut tree, ctrl, Vector2::new(50.0, 30.0));
    set_grow_direction_v(&mut tree, ctrl, GrowDirection::End);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(400.0, 400.0));

    // X axis already has 100 ≥ min 50 → no growth; pos.x=0, size.x=100.
    // Y axis has 0 < 30 → grow End → pos.y=0, size.y=30.
    assert!(approx_vec(get_position(&tree, ctrl), Vector2::ZERO));
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(100.0, 30.0)));
}

// ---------------------------------------------------------------------------
// Theme property lookup (pat-yz7rj)
// ---------------------------------------------------------------------------

/// Oracle: a freshly constructed default Theme exposes Godot's baseline
/// appearance properties for Label, Button, and Panel across every supported
/// property category (colors, font sizes, constants, styleboxes). ThemeDB
/// wraps a Theme and returns fallbacks for unknown keys.
#[test]
fn theme_property_lookup() {
    let theme = Theme::default_theme();

    // --- Label colors / font size / constants ------------------------------
    assert_eq!(
        theme.get_color("Label", "font_color"),
        Some(Color::WHITE),
        "Label font_color should default to white"
    );
    assert_eq!(
        theme.get_font_size("Label", "font_size"),
        Some(16),
        "Label font_size should default to 16"
    );
    assert_eq!(
        theme.get_constant("Label", "shadow_offset_x"),
        Some(1),
        "Label shadow_offset_x should default to 1"
    );
    assert_eq!(
        theme.get_constant("Label", "shadow_offset_y"),
        Some(1),
        "Label shadow_offset_y should default to 1"
    );
    assert_eq!(
        theme.get_constant("Label", "line_spacing"),
        Some(3),
        "Label line_spacing should default to 3"
    );

    // --- Button colors and styleboxes --------------------------------------
    assert!(theme.has_color("Button", "font_color"));
    assert!(theme.has_color("Button", "font_hover_color"));
    assert!(theme.has_color("Button", "font_pressed_color"));
    assert!(theme.has_color("Button", "font_disabled_color"));
    assert_eq!(theme.get_font_size("Button", "font_size"), Some(16));
    assert_eq!(theme.get_constant("Button", "h_separation"), Some(4));

    for state in ["normal", "hover", "pressed"] {
        assert!(
            theme.has_item(ThemePropertyType::Stylebox, "Button", state),
            "Button should have a '{state}' stylebox"
        );
    }

    // --- Panel stylebox + margins ------------------------------------------
    assert!(theme.has_item(ThemePropertyType::Stylebox, "Panel", "panel"));
    for margin in [
        "content_margin_left",
        "content_margin_top",
        "content_margin_right",
        "content_margin_bottom",
    ] {
        assert_eq!(
            theme.get_constant("Panel", margin),
            Some(4),
            "Panel {margin} should default to 4"
        );
    }

    // --- Missing keys return None, not panic -------------------------------
    assert!(theme.get_color("Label", "nonexistent").is_none());
    assert!(theme.get_font_size("Button", "nonexistent").is_none());
    assert!(theme.get_constant("Panel", "nonexistent").is_none());
    assert!(theme
        .get_item(ThemePropertyType::Font, "Label", "nonexistent")
        .is_none());

    // --- Override via mutation propagates to subsequent reads --------------
    let mut mutable = theme.clone();
    let accent = Color::rgb(0.2, 0.6, 0.9);
    mutable.set_color("Label", "font_color", accent);
    assert_eq!(mutable.get_color("Label", "font_color"), Some(accent));
    // Original theme is untouched (Theme is Clone, not shared).
    assert_eq!(theme.get_color("Label", "font_color"), Some(Color::WHITE));

    // --- Generic Variant items roundtrip for Font and Stylebox -------------
    let font_ref = Variant::String("res://fonts/mono.ttf".into());
    mutable.set_item(
        ThemePropertyType::Font,
        "Label",
        "font",
        font_ref.clone(),
    );
    assert_eq!(
        mutable.get_item(ThemePropertyType::Font, "Label", "font"),
        Some(&font_ref)
    );

    let removed = mutable.remove_item(ThemePropertyType::Font, "Label", "font");
    assert_eq!(removed, Some(font_ref));
    assert!(!mutable.has_item(ThemePropertyType::Font, "Label", "font"));

    // --- Category keys are independent -------------------------------------
    mutable.set_color("Label", "paint", Color::rgb(1.0, 0.0, 0.0));
    mutable.set_item(
        ThemePropertyType::Stylebox,
        "Label",
        "paint",
        Variant::Color(Color::rgb(0.0, 1.0, 0.0)),
    );
    assert_eq!(
        mutable.get_color("Label", "paint"),
        Some(Color::rgb(1.0, 0.0, 0.0)),
        "color lookup should not be shadowed by a stylebox with the same name"
    );
    assert_eq!(
        mutable.get_item(ThemePropertyType::Stylebox, "Label", "paint"),
        Some(&Variant::Color(Color::rgb(0.0, 1.0, 0.0))),
        "stylebox lookup should not be shadowed by a color with the same name"
    );

    // --- ThemeDB fallback semantics ----------------------------------------
    let db = ThemeDB::new();
    assert_eq!(
        db.get_color_or("Label", "font_color", Color::BLACK),
        Color::WHITE,
        "known key returns theme value, not fallback"
    );
    assert_eq!(
        db.get_color_or("Unknown", "missing", Color::BLACK),
        Color::BLACK,
        "unknown key returns fallback"
    );
    assert_eq!(db.get_font_size_or("Label", "font_size", 99), 16);
    assert_eq!(db.get_font_size_or("Unknown", "missing", 99), 99);
    assert_eq!(db.get_constant_or("Panel", "content_margin_left", 0), 4);
    assert_eq!(db.get_constant_or("Unknown", "missing", 7), 7);

    // --- ThemeDB allows replacing the default theme entirely ---------------
    let mut db = ThemeDB::new();
    let mut custom = Theme::new("custom");
    custom.set_color("Label", "font_color", Color::rgb(1.0, 0.5, 0.0));
    db.set_default_theme(custom);
    assert_eq!(
        db.default_theme().get_color("Label", "font_color"),
        Some(Color::rgb(1.0, 0.5, 0.0)),
    );
    // After replacement, keys from the previous default are gone.
    assert!(db
        .default_theme()
        .get_font_size("Label", "font_size")
        .is_none());
}

// ---------------------------------------------------------------------------
// pat-4h6s8: Control layout solver (anchors + margins + size-flags)
// ---------------------------------------------------------------------------

/// End-to-end acceptance for pat-4h6s8: a single test that drives the Control
/// layout solver through each of its three responsibilities — anchor
/// resolution, margin offsets, and per-child size-flag distribution —
/// against Godot's documented layout rules.
#[test]
fn control_layout() {
    // ---- 1. Anchors + margins resolve into a final rect ------------------
    // FullRect preset (all anchors at 1.0 on the right/bottom, 0.0 on the
    // left/top) with a symmetric 10px inset must produce a rect inset from
    // the parent on every side.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    apply_anchor_preset(&mut tree, ctrl, AnchorPreset::FullRect);
    set_offset_left(&mut tree, ctrl, 10.0);
    set_offset_top(&mut tree, ctrl, 10.0);
    set_offset_right(&mut tree, ctrl, -10.0);
    set_offset_bottom(&mut tree, ctrl, -10.0);

    resolve_control_layout(&mut tree, ctrl, Vector2::new(400.0, 300.0));
    assert!(
        approx_vec(get_position(&tree, ctrl), Vector2::new(10.0, 10.0)),
        "FullRect preset with 10px insets must position at (10,10)"
    );
    assert!(
        approx_vec(get_size(&tree, ctrl), Vector2::new(380.0, 280.0)),
        "FullRect preset with 10px insets must shrink the rect by 20px on each axis"
    );

    // ---- 2. Arbitrary anchors combine parent-fraction and offset ---------
    // Anchors (0.25, 0.50, 0.75, 1.0) + zero offsets yield a rect spanning
    // from 25%..75% horizontally and 50%..100% vertically of the parent.
    let ctrl2 = add_child(&mut tree, root, "Ctrl2", "Control");
    set_anchor_left(&mut tree, ctrl2, 0.25);
    set_anchor_right(&mut tree, ctrl2, 0.75);
    set_anchor_top(&mut tree, ctrl2, 0.5);
    set_anchor_bottom(&mut tree, ctrl2, 1.0);

    resolve_control_layout(&mut tree, ctrl2, Vector2::new(400.0, 300.0));
    assert!(approx_vec(
        get_position(&tree, ctrl2),
        Vector2::new(100.0, 150.0)
    ));
    assert!(approx_vec(get_size(&tree, ctrl2), Vector2::new(200.0, 150.0)));

    // ---- 3. Minimum-size grow direction takes effect when offsets imply
    //        a rect smaller than `custom_minimum_size` --------------------
    // Center-anchored (all anchors 0.5) + zero offsets produces a zero-sized
    // rect at the parent center (100,100) for a 200x200 parent. With
    // custom_minimum_size 40x20 and GrowDirection::Both, the rect must grow
    // symmetrically around that anchor point.
    let ctrl3 = add_child(&mut tree, root, "Ctrl3", "Control");
    apply_anchor_preset(&mut tree, ctrl3, AnchorPreset::Center);
    set_custom_minimum_size(&mut tree, ctrl3, Vector2::new(40.0, 20.0));
    set_grow_direction_h(&mut tree, ctrl3, GrowDirection::Both);
    set_grow_direction_v(&mut tree, ctrl3, GrowDirection::Both);

    resolve_control_layout(&mut tree, ctrl3, Vector2::new(200.0, 200.0));
    assert!(
        approx_vec(get_position(&tree, ctrl3), Vector2::new(80.0, 90.0)),
        "Center-anchored rect with Both-grow must be centered on (100,100)"
    );
    assert!(approx_vec(get_size(&tree, ctrl3), Vector2::new(40.0, 20.0)));

    // ---- 4. Size flags in a container: Fill keeps min size, Expand
    //        distributes extra space -------------------------------------
    // VBox of height 200, two children with min-heights 20 and 30; the
    // Expand child must absorb the remaining 150px, the Fill child (default)
    // must keep its min-height exactly.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let vbox = add_child(&mut tree, root, "VBox", "VBoxContainer");
    set_separation(&mut tree, vbox, 0);

    let filler = add_child(&mut tree, vbox, "Fill", "Control");
    let grower = add_child(&mut tree, vbox, "Expand", "Control");
    set_custom_minimum_size(&mut tree, filler, Vector2::new(10.0, 20.0));
    set_custom_minimum_size(&mut tree, grower, Vector2::new(10.0, 30.0));
    set_v_size_flags(&mut tree, filler, SizeFlags::Fill);
    set_v_size_flags(&mut tree, grower, SizeFlags::Expand);

    arrange_container(&mut tree, vbox, Vector2::new(100.0, 200.0));

    assert!(
        approx(get_size(&tree, filler).y, 20.0),
        "Fill child must keep its minimum height"
    );
    assert!(
        approx(get_size(&tree, grower).y, 180.0),
        "Expand child must absorb the 150px remainder on top of its 30px min"
    );
    // Fill children still stretch to the cross-axis width of the container.
    assert!(approx(get_size(&tree, filler).x, 100.0));
    assert!(approx(get_size(&tree, grower).x, 100.0));

    // ---- 5. Size flags split extra space evenly between multiple Expand
    //        children in an HBox -----------------------------------------
    let hbox = add_child(&mut tree, root, "HBox", "HBoxContainer");
    set_separation(&mut tree, hbox, 0);

    let a = add_child(&mut tree, hbox, "A", "Control");
    let b = add_child(&mut tree, hbox, "B", "Control");
    let c = add_child(&mut tree, hbox, "C", "Control");
    set_custom_minimum_size(&mut tree, a, Vector2::new(20.0, 10.0));
    set_custom_minimum_size(&mut tree, b, Vector2::new(20.0, 10.0));
    set_custom_minimum_size(&mut tree, c, Vector2::new(20.0, 10.0));
    set_h_size_flags(&mut tree, a, SizeFlags::Expand);
    set_h_size_flags(&mut tree, b, SizeFlags::Fill);
    set_h_size_flags(&mut tree, c, SizeFlags::Expand);

    arrange_container(&mut tree, hbox, Vector2::new(300.0, 40.0));

    // Total min = 60; remaining 240 distributed between the 2 Expand
    // children → each gets +120. B (Fill) keeps its 20px min.
    assert!(approx(get_size(&tree, a).x, 140.0));
    assert!(approx(get_size(&tree, b).x, 20.0));
    assert!(approx(get_size(&tree, c).x, 140.0));
    assert!(approx_vec(get_position(&tree, a), Vector2::new(0.0, 0.0)));
    assert!(approx_vec(get_position(&tree, b), Vector2::new(140.0, 0.0)));
    assert!(approx_vec(get_position(&tree, c), Vector2::new(160.0, 0.0)));
}

// ---------------------------------------------------------------------------
// pat-sk37j: VBoxContainer / HBoxContainer / GridContainer end-to-end
// ---------------------------------------------------------------------------

/// End-to-end acceptance for pat-sk37j: a single test that drives all three
/// container nodes through `arrange_container`, verifying separation,
/// per-child size flags, and (for the grid) row/column sizing.
#[test]
fn container_nodes() {
    // ---- VBoxContainer: stacks children with separation ------------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let vbox = add_child(&mut tree, root, "VBox", "VBoxContainer");
    set_separation(&mut tree, vbox, 4);

    let v_a = add_child(&mut tree, vbox, "VA", "Control");
    let v_b = add_child(&mut tree, vbox, "VB", "Control");
    let v_c = add_child(&mut tree, vbox, "VC", "Control");
    set_custom_minimum_size(&mut tree, v_a, Vector2::new(40.0, 20.0));
    set_custom_minimum_size(&mut tree, v_b, Vector2::new(40.0, 30.0));
    set_custom_minimum_size(&mut tree, v_c, Vector2::new(40.0, 40.0));

    arrange_container(&mut tree, vbox, Vector2::new(100.0, 200.0));

    // Children stack vertically with 4px separation.
    assert!(approx_vec(get_position(&tree, v_a), Vector2::new(0.0, 0.0)));
    assert!(approx_vec(get_position(&tree, v_b), Vector2::new(0.0, 24.0)));
    assert!(approx_vec(get_position(&tree, v_c), Vector2::new(0.0, 58.0)));
    // Cross-axis fills the container width.
    assert!(approx(get_size(&tree, v_a).x, 100.0));
    assert!(approx(get_size(&tree, v_b).x, 100.0));
    assert!(approx(get_size(&tree, v_c).x, 100.0));
    // Heights match each child's minimum since no Expand flags are set.
    assert!(approx(get_size(&tree, v_a).y, 20.0));
    assert!(approx(get_size(&tree, v_b).y, 30.0));
    assert!(approx(get_size(&tree, v_c).y, 40.0));

    // ---- HBoxContainer: Expand children share leftover width -------------
    let hbox = add_child(&mut tree, root, "HBox", "HBoxContainer");
    set_separation(&mut tree, hbox, 0);

    let h_a = add_child(&mut tree, hbox, "HA", "Control");
    let h_b = add_child(&mut tree, hbox, "HB", "Control");
    set_custom_minimum_size(&mut tree, h_a, Vector2::new(50.0, 40.0));
    set_custom_minimum_size(&mut tree, h_b, Vector2::new(30.0, 40.0));
    set_h_size_flags(&mut tree, h_b, SizeFlags::Expand);

    arrange_container(&mut tree, hbox, Vector2::new(200.0, 80.0));

    // Total min width = 80; HB (Expand) absorbs the remaining 120.
    assert!(approx(get_size(&tree, h_a).x, 50.0));
    assert!(approx(get_size(&tree, h_b).x, 150.0));
    assert!(approx_vec(get_position(&tree, h_a), Vector2::new(0.0, 0.0)));
    assert!(approx_vec(get_position(&tree, h_b), Vector2::new(50.0, 0.0)));
    // Cross-axis (height) fills the container.
    assert!(approx(get_size(&tree, h_a).y, 80.0));
    assert!(approx(get_size(&tree, h_b).y, 80.0));

    // ---- GridContainer: cells take the max of their column / row ---------
    let grid = add_child(&mut tree, root, "Grid", "GridContainer");
    set_columns(&mut tree, grid, 2);
    set_separation(&mut tree, grid, 5);

    // 3 children → 2 rows × 2 cols; row 1 col 1 stays empty.
    let g_a = add_child(&mut tree, grid, "GA", "Control");
    let g_b = add_child(&mut tree, grid, "GB", "Control");
    let g_c = add_child(&mut tree, grid, "GC", "Control");
    set_custom_minimum_size(&mut tree, g_a, Vector2::new(30.0, 40.0));
    set_custom_minimum_size(&mut tree, g_b, Vector2::new(50.0, 20.0));
    set_custom_minimum_size(&mut tree, g_c, Vector2::new(10.0, 60.0));

    arrange_container(&mut tree, grid, Vector2::new(200.0, 200.0));

    // Column widths: col0 = max(30, 10) = 30, col1 = max(50) = 50.
    // Row heights:   row0 = max(40, 20) = 40, row1 = max(60)    = 60.
    // Separation 5px between columns and rows.
    assert!(approx_vec(get_position(&tree, g_a), Vector2::new(0.0, 0.0)));
    assert!(approx_vec(get_position(&tree, g_b), Vector2::new(35.0, 0.0)));
    assert!(approx_vec(get_position(&tree, g_c), Vector2::new(0.0, 45.0)));
    // Each cell sizes to its column width × row height.
    assert!(approx_vec(get_size(&tree, g_a), Vector2::new(30.0, 40.0)));
    assert!(approx_vec(get_size(&tree, g_b), Vector2::new(50.0, 40.0)));
    assert!(approx_vec(get_size(&tree, g_c), Vector2::new(30.0, 60.0)));
}

// ---------------------------------------------------------------------------
// pat-n8jtk: Inspector → Control layout binding
// ---------------------------------------------------------------------------

/// End-to-end acceptance for pat-n8jtk: edits to a Control node's anchor /
/// margin / size-flags / minimum-size properties made through `InspectorPanel`
/// must flow into the `gdscene::control` layout solver immediately, mirroring
/// Godot's behavior where dragging an anchor in the inspector retargets the
/// rect on screen.
#[test]
fn control_property_binding() {
    use gdeditor::control_binding::set_control_property_with_layout;
    use gdeditor::InspectorPanel;

    let parent_size = Vector2::new(400.0, 300.0);

    // ---- 1. Anchor edits propagate through the inspector ----------------
    // Start with TopLeft preset and offset_right=100, offset_bottom=50 →
    // a 100×50 rect at (0,0). Setting anchor_right=1.0 via the inspector
    // must re-solve the layout to a 500×50 rect (offset_right is added on
    // top of `parent.x * 1.0`).
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ctrl = add_child(&mut tree, root, "Ctrl", "Control");
    apply_anchor_preset(&mut tree, ctrl, AnchorPreset::TopLeft);
    set_offset_right(&mut tree, ctrl, 100.0);
    set_offset_bottom(&mut tree, ctrl, 50.0);
    resolve_control_layout(&mut tree, ctrl, parent_size);
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(100.0, 50.0)));

    let mut inspector = InspectorPanel::new();
    inspector.inspect(ctrl);
    set_control_property_with_layout(
        &inspector,
        &mut tree,
        "anchor_right",
        Variant::Float(1.0),
        parent_size,
    );
    // After the binding fires, x2 = 400*1.0 + 100 = 500, x1 = 0 → 500×50.
    assert!(approx_vec(get_size(&tree, ctrl), Vector2::new(500.0, 50.0)));

    // ---- 2. custom_minimum_size triggers grow-direction expansion -------
    // Center anchor with no offsets collapses the rect to a single point at
    // the parent center. Setting custom_minimum_size to 40×20 via the
    // inspector must grow the rect to that size (Grow End is the default,
    // so the origin stays anchored at parent-center and size becomes 40×20).
    let ctrl2 = add_child(&mut tree, root, "Ctrl2", "Control");
    apply_anchor_preset(&mut tree, ctrl2, AnchorPreset::Center);
    resolve_control_layout(&mut tree, ctrl2, parent_size);
    assert!(approx_vec(get_size(&tree, ctrl2), Vector2::ZERO));

    inspector.inspect(ctrl2);
    set_control_property_with_layout(
        &inspector,
        &mut tree,
        "custom_minimum_size",
        Variant::Vector2(Vector2::new(40.0, 20.0)),
        parent_size,
    );
    assert!(approx_vec(get_size(&tree, ctrl2), Vector2::new(40.0, 20.0)));

    // ---- 3. size_flags edit re-arranges the parent container ------------
    // VBox with two min-sized children (20px + 30px). With no Expand flags
    // set, the remaining 150px is unused and B keeps its 30px minimum.
    // Switching B's size_flags_vertical to Expand via the inspector must
    // trigger a re-arrange of the VBox — B should absorb the remaining
    // 150px and grow to 180px.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let vbox = add_child(&mut tree, root, "VBox", "VBoxContainer");
    set_separation(&mut tree, vbox, 0);
    let a = add_child(&mut tree, vbox, "A", "Control");
    let b = add_child(&mut tree, vbox, "B", "Control");
    set_custom_minimum_size(&mut tree, a, Vector2::new(10.0, 20.0));
    set_custom_minimum_size(&mut tree, b, Vector2::new(10.0, 30.0));
    arrange_container(&mut tree, vbox, Vector2::new(100.0, 200.0));
    assert!(approx(get_size(&tree, a).y, 20.0));
    assert!(approx(get_size(&tree, b).y, 30.0));

    let mut inspector = InspectorPanel::new();
    inspector.inspect(b);
    // SizeFlags::Expand serializes to int 3 (FILL | EXPAND) per the
    // representation gdscene::control uses internally.
    set_control_property_with_layout(
        &inspector,
        &mut tree,
        "size_flags_vertical",
        Variant::Int(3),
        Vector2::new(100.0, 200.0),
    );
    // B now expands; A keeps its minimum.
    assert!(approx(get_size(&tree, a).y, 20.0));
    assert!(approx(get_size(&tree, b).y, 180.0));
    assert!(approx_vec(get_position(&tree, b), Vector2::new(0.0, 20.0)));

    // ---- 4. Non-layout property edits are passthroughs ------------------
    // Setting a property the binding doesn't recognise (e.g. an arbitrary
    // user-defined name) must NOT trigger a re-solve, but must still write
    // the value through to the node so other inspector consumers see it.
    let ctrl3 = add_child(&mut tree, root, "Ctrl3", "Control");
    apply_anchor_preset(&mut tree, ctrl3, AnchorPreset::TopLeft);
    set_offset_right(&mut tree, ctrl3, 100.0);
    set_offset_bottom(&mut tree, ctrl3, 50.0);
    resolve_control_layout(&mut tree, ctrl3, parent_size);
    let pre_size = get_size(&tree, ctrl3);

    inspector.inspect(ctrl3);
    set_control_property_with_layout(
        &inspector,
        &mut tree,
        "my_custom_prop",
        Variant::Int(7),
        parent_size,
    );
    // Layout untouched.
    assert!(approx_vec(get_size(&tree, ctrl3), pre_size));
    // But the property write went through.
    let written = inspector.get_property(&tree, "my_custom_prop");
    assert_eq!(written, Variant::Int(7));
}
