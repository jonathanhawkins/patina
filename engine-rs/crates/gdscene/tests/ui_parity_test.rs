//! UI parity integration tests.
//!
//! These tests verify that the UI runtime (Theme, Control layout, etc.)
//! behaves in parity with Godot's UI system for the features covered by
//! the V1 gap matrix.
//!
//! Oracle rule: Each test documents the observable behavior it checks.

use gdcore::math::{Color, Vector2};
use gdscene::control::{
    apply_anchor_preset, arrange_container, get_position, get_size, resolve_control_layout,
    set_anchor_bottom, set_anchor_left, set_anchor_right, set_anchor_top, set_columns,
    set_custom_minimum_size, set_grow_direction_h, set_grow_direction_v, set_h_size_flags,
    set_min_size, set_offset_bottom, set_offset_left, set_offset_right, set_offset_top,
    set_separation, set_v_size_flags, AnchorPreset, GrowDirection, SizeFlags,
};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::theme::{Theme, ThemeDB, ThemePropertyType};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Theme property lookup
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
    // A color and a stylebox at the same (control, property) must not alias.
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
    assert!(db.default_theme().get_font_size("Label", "font_size").is_none());
}

// ---------------------------------------------------------------------------
// Control anchor / margin / size-flags layout solver
// ---------------------------------------------------------------------------

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn approx_vec(a: Vector2, b: Vector2) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y)
}

fn add_child(tree: &mut SceneTree, name: &str, class: &str) -> gdscene::node::NodeId {
    let root = tree.root_id();
    let node = Node::new(name, class);
    tree.add_child(root, node).expect("add_child")
}

/// Oracle: a Control with a `FullRect` preset and zero offsets fills its
/// parent exactly. With positive offsets, the resolved rect is inset; with
/// negative offsets, the rect is expanded outward. Anchor/offset arithmetic
/// mirrors Godot's `Control::_compute_anchors_rect` rules:
///   x1 = parent.x * anchor_left + offset_left
///   x2 = parent.x * anchor_right + offset_right
///   (and likewise for y). The resolved width/height must never drop below
/// the effective minimum size (max of `custom_minimum_size` and `min_size`);
/// when it would, the rect grows outward per `grow_horizontal`/`grow_vertical`.
/// Containers (VBox/HBox/Grid) distribute their children using `size_flags`
/// and `separation`. This integration test exercises each of those rules.
#[test]
fn control_anchor_layout() {
    let parent = Vector2::new(800.0, 600.0);

    // --- FullRect preset fills the parent exactly --------------------------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "Full", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);
        resolve_control_layout(&mut tree, id, parent);

        assert!(
            approx_vec(get_position(&tree, id), Vector2::ZERO),
            "FullRect preset should anchor at (0, 0)"
        );
        assert!(
            approx_vec(get_size(&tree, id), parent),
            "FullRect preset should fill parent"
        );
    }

    // --- FullRect with insets (positive offsets) ---------------------------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "Inset", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);
        set_offset_left(&mut tree, id, 10.0);
        set_offset_top(&mut tree, id, 20.0);
        set_offset_right(&mut tree, id, -30.0);
        set_offset_bottom(&mut tree, id, -40.0);
        resolve_control_layout(&mut tree, id, parent);

        assert!(
            approx_vec(get_position(&tree, id), Vector2::new(10.0, 20.0)),
            "Positive start offsets move the origin inward"
        );
        assert!(
            approx_vec(
                get_size(&tree, id),
                Vector2::new(parent.x - 40.0, parent.y - 60.0),
            ),
            "Negative end offsets shrink width/height"
        );
    }

    // --- TopLeft preset + explicit offsets positions a fixed-size rect -----
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "Fixed", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        set_offset_left(&mut tree, id, 50.0);
        set_offset_top(&mut tree, id, 60.0);
        set_offset_right(&mut tree, id, 250.0);
        set_offset_bottom(&mut tree, id, 160.0);
        resolve_control_layout(&mut tree, id, parent);

        assert!(approx_vec(get_position(&tree, id), Vector2::new(50.0, 60.0)));
        assert!(approx_vec(
            get_size(&tree, id),
            Vector2::new(200.0, 100.0)
        ));
    }

    // --- Center preset with symmetric offsets -> centered panel -----------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "Center", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::Center);
        set_offset_left(&mut tree, id, -100.0);
        set_offset_top(&mut tree, id, -50.0);
        set_offset_right(&mut tree, id, 100.0);
        set_offset_bottom(&mut tree, id, 50.0);
        resolve_control_layout(&mut tree, id, parent);

        // parent/2 = (400, 300); offsets give a 200x100 rect centered on it.
        assert!(approx_vec(
            get_position(&tree, id),
            Vector2::new(300.0, 250.0)
        ));
        assert!(approx_vec(
            get_size(&tree, id),
            Vector2::new(200.0, 100.0)
        ));
    }

    // --- Partial anchors evaluate as mixed fractions of parent ------------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "Partial", "Control");
        set_anchor_left(&mut tree, id, 0.25);
        set_anchor_top(&mut tree, id, 0.1);
        set_anchor_right(&mut tree, id, 0.75);
        set_anchor_bottom(&mut tree, id, 0.9);
        resolve_control_layout(&mut tree, id, parent);

        // left  = 800 * 0.25 = 200  / top    = 600 * 0.1 = 60
        // right = 800 * 0.75 = 600  / bottom = 600 * 0.9 = 540
        assert!(approx_vec(
            get_position(&tree, id),
            Vector2::new(200.0, 60.0)
        ));
        assert!(approx_vec(
            get_size(&tree, id),
            Vector2::new(400.0, 480.0)
        ));
    }

    // --- Minimum size enforcement with GrowDirection::End ------------------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "Min", "Control");
        // Anchor rect would be zero-sized at (100, 80).
        apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        set_offset_left(&mut tree, id, 100.0);
        set_offset_top(&mut tree, id, 80.0);
        set_offset_right(&mut tree, id, 100.0);
        set_offset_bottom(&mut tree, id, 80.0);
        set_custom_minimum_size(&mut tree, id, Vector2::new(120.0, 40.0));
        set_grow_direction_h(&mut tree, id, GrowDirection::End);
        set_grow_direction_v(&mut tree, id, GrowDirection::End);
        resolve_control_layout(&mut tree, id, parent);

        assert!(
            approx_vec(get_position(&tree, id), Vector2::new(100.0, 80.0)),
            "Grow::End should keep the start position fixed"
        );
        assert!(
            approx_vec(get_size(&tree, id), Vector2::new(120.0, 40.0)),
            "Rect should expand to the effective minimum size"
        );
    }

    // --- GrowDirection::Begin grows backward from the end edge ------------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "GrowBegin", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        set_offset_left(&mut tree, id, 200.0);
        set_offset_top(&mut tree, id, 150.0);
        set_offset_right(&mut tree, id, 200.0);
        set_offset_bottom(&mut tree, id, 150.0);
        set_min_size(&mut tree, id, Vector2::new(80.0, 30.0));
        set_grow_direction_h(&mut tree, id, GrowDirection::Begin);
        set_grow_direction_v(&mut tree, id, GrowDirection::Begin);
        resolve_control_layout(&mut tree, id, parent);

        // End edge stays at (200, 150); the rect grows backward by the min size.
        assert!(approx_vec(
            get_position(&tree, id),
            Vector2::new(120.0, 120.0)
        ));
        assert!(approx_vec(get_size(&tree, id), Vector2::new(80.0, 30.0)));
    }

    // --- GrowDirection::Both splits the delta evenly ----------------------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "GrowBoth", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        set_offset_left(&mut tree, id, 400.0);
        set_offset_top(&mut tree, id, 300.0);
        set_offset_right(&mut tree, id, 400.0);
        set_offset_bottom(&mut tree, id, 300.0);
        set_custom_minimum_size(&mut tree, id, Vector2::new(100.0, 40.0));
        set_grow_direction_h(&mut tree, id, GrowDirection::Both);
        set_grow_direction_v(&mut tree, id, GrowDirection::Both);
        resolve_control_layout(&mut tree, id, parent);

        // Centered on (400, 300) with half the min size on each side.
        assert!(approx_vec(
            get_position(&tree, id),
            Vector2::new(350.0, 280.0)
        ));
        assert!(approx_vec(
            get_size(&tree, id),
            Vector2::new(100.0, 40.0)
        ));
    }

    // --- Effective minimum size uses the max of custom and inner min ------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "MinMax", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        set_min_size(&mut tree, id, Vector2::new(50.0, 100.0));
        set_custom_minimum_size(&mut tree, id, Vector2::new(200.0, 10.0));
        resolve_control_layout(&mut tree, id, parent);

        assert!(
            approx_vec(get_size(&tree, id), Vector2::new(200.0, 100.0)),
            "Effective min = component-wise max(custom_minimum_size, min_size)"
        );
    }

    // --- VBoxContainer arranges children vertically with Expand + separation
    {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let vbox = tree
            .add_child(root, Node::new("VBox", "VBoxContainer"))
            .expect("vbox");
        let a = tree
            .add_child(vbox, Node::new("A", "Control"))
            .expect("a");
        let b = tree
            .add_child(vbox, Node::new("B", "Control"))
            .expect("b");
        let c = tree
            .add_child(vbox, Node::new("C", "Control"))
            .expect("c");

        set_custom_minimum_size(&mut tree, a, Vector2::new(0.0, 20.0));
        set_custom_minimum_size(&mut tree, b, Vector2::new(0.0, 30.0));
        set_custom_minimum_size(&mut tree, c, Vector2::new(0.0, 40.0));
        // B takes the remaining space.
        set_v_size_flags(&mut tree, b, SizeFlags::Expand);
        set_separation(&mut tree, vbox, 10);

        let total = Vector2::new(200.0, 200.0);
        arrange_container(&mut tree, vbox, total);

        // mins = 20+30+40 = 90; separation = 10*2 = 20; extra = 200-90-20 = 90.
        // B grows by 90 -> height = 120.
        assert!(approx_vec(get_position(&tree, a), Vector2::new(0.0, 0.0)));
        assert!(approx_vec(get_size(&tree, a), Vector2::new(200.0, 20.0)));

        assert!(approx_vec(get_position(&tree, b), Vector2::new(0.0, 30.0)));
        assert!(approx_vec(get_size(&tree, b), Vector2::new(200.0, 120.0)));

        // C starts after B plus separation: 30 + 120 + 10 = 160.
        assert!(approx_vec(
            get_position(&tree, c),
            Vector2::new(0.0, 160.0)
        ));
        assert!(approx_vec(get_size(&tree, c), Vector2::new(200.0, 40.0)));
    }

    // --- HBoxContainer with two Expand children shares remaining width ----
    {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let hbox = tree
            .add_child(root, Node::new("HBox", "HBoxContainer"))
            .expect("hbox");
        let a = tree
            .add_child(hbox, Node::new("A", "Control"))
            .expect("a");
        let b = tree
            .add_child(hbox, Node::new("B", "Control"))
            .expect("b");
        let c = tree
            .add_child(hbox, Node::new("C", "Control"))
            .expect("c");

        set_custom_minimum_size(&mut tree, a, Vector2::new(40.0, 0.0));
        set_custom_minimum_size(&mut tree, b, Vector2::new(40.0, 0.0));
        set_custom_minimum_size(&mut tree, c, Vector2::new(40.0, 0.0));
        set_h_size_flags(&mut tree, a, SizeFlags::Expand);
        set_h_size_flags(&mut tree, c, SizeFlags::Expand);
        set_separation(&mut tree, hbox, 8);

        let total = Vector2::new(300.0, 50.0);
        arrange_container(&mut tree, hbox, total);

        // mins = 120; gaps = 16; extra = 164; per_expand = 82.
        // a: 40 + 82 = 122, b: 40, c: 40 + 82 = 122.
        assert!(approx_vec(get_position(&tree, a), Vector2::new(0.0, 0.0)));
        assert!(approx_vec(get_size(&tree, a), Vector2::new(122.0, 50.0)));

        // b follows a + separation.
        assert!(approx_vec(
            get_position(&tree, b),
            Vector2::new(130.0, 0.0)
        ));
        assert!(approx_vec(get_size(&tree, b), Vector2::new(40.0, 50.0)));

        // c follows b + separation.
        assert!(approx_vec(
            get_position(&tree, c),
            Vector2::new(178.0, 0.0)
        ));
        assert!(approx_vec(get_size(&tree, c), Vector2::new(122.0, 50.0)));
    }

    // --- GridContainer arranges children by per-column / per-row maxima ----
    {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let grid = tree
            .add_child(root, Node::new("Grid", "GridContainer"))
            .expect("grid");
        set_columns(&mut tree, grid, 2);
        set_separation(&mut tree, grid, 5);

        let cells: Vec<_> = (0..4)
            .map(|i| {
                tree.add_child(grid, Node::new(&format!("Cell{i}"), "Control"))
                    .expect("cell")
            })
            .collect();

        // Column 0 max width = max(50, 70) = 70.
        // Column 1 max width = max(60, 40) = 60.
        // Row 0 max height   = max(20, 25) = 25.
        // Row 1 max height   = max(30, 35) = 35.
        set_custom_minimum_size(&mut tree, cells[0], Vector2::new(50.0, 20.0));
        set_custom_minimum_size(&mut tree, cells[1], Vector2::new(60.0, 25.0));
        set_custom_minimum_size(&mut tree, cells[2], Vector2::new(70.0, 30.0));
        set_custom_minimum_size(&mut tree, cells[3], Vector2::new(40.0, 35.0));

        arrange_container(&mut tree, grid, Vector2::new(200.0, 100.0));

        // Row 0
        assert!(approx_vec(
            get_position(&tree, cells[0]),
            Vector2::new(0.0, 0.0)
        ));
        assert!(approx_vec(
            get_size(&tree, cells[0]),
            Vector2::new(70.0, 25.0)
        ));
        assert!(approx_vec(
            get_position(&tree, cells[1]),
            Vector2::new(75.0, 0.0)
        ));
        assert!(approx_vec(
            get_size(&tree, cells[1]),
            Vector2::new(60.0, 25.0)
        ));

        // Row 1 starts after row-0 height + separation = 30.
        assert!(approx_vec(
            get_position(&tree, cells[2]),
            Vector2::new(0.0, 30.0)
        ));
        assert!(approx_vec(
            get_size(&tree, cells[2]),
            Vector2::new(70.0, 35.0)
        ));
        assert!(approx_vec(
            get_position(&tree, cells[3]),
            Vector2::new(75.0, 30.0)
        ));
        assert!(approx_vec(
            get_size(&tree, cells[3]),
            Vector2::new(60.0, 35.0)
        ));
    }

    // --- Anchor + grow rules compose: center-anchored with min size -------
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "CenterMin", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::Center);
        set_offset_left(&mut tree, id, 0.0);
        set_offset_top(&mut tree, id, 0.0);
        set_offset_right(&mut tree, id, 0.0);
        set_offset_bottom(&mut tree, id, 0.0);
        // Anchor rect is a zero-area point at parent/2.
        set_custom_minimum_size(&mut tree, id, Vector2::new(60.0, 40.0));
        set_grow_direction_h(&mut tree, id, GrowDirection::Both);
        set_grow_direction_v(&mut tree, id, GrowDirection::Both);
        resolve_control_layout(&mut tree, id, parent);

        // Centered: origin = (parent/2) - (min/2).
        assert!(approx_vec(
            get_position(&tree, id),
            Vector2::new(370.0, 280.0)
        ));
        assert!(approx_vec(get_size(&tree, id), Vector2::new(60.0, 40.0)));
    }

    // --- BottomRight preset with offsets places rect above-left of corner -
    {
        let mut tree = SceneTree::new();
        let id = add_child(&mut tree, "BR", "Control");
        apply_anchor_preset(&mut tree, id, AnchorPreset::BottomRight);
        set_offset_left(&mut tree, id, -120.0);
        set_offset_top(&mut tree, id, -60.0);
        set_offset_right(&mut tree, id, -20.0);
        set_offset_bottom(&mut tree, id, -10.0);
        resolve_control_layout(&mut tree, id, parent);

        // x1 = 800 - 120 = 680, x2 = 800 - 20 = 780 -> width 100.
        // y1 = 600 - 60  = 540, y2 = 600 - 10 = 590 -> height 50.
        assert!(approx_vec(
            get_position(&tree, id),
            Vector2::new(680.0, 540.0)
        ));
        assert!(approx_vec(
            get_size(&tree, id),
            Vector2::new(100.0, 50.0)
        ));
    }
}
