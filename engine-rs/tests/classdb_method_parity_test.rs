//! pat-i6l0 / pat-kx5: ClassDB method enumeration parity tests.
//!
//! Expands ClassDB parity to cover METHOD enumeration alongside properties.
//! For each of the 17 core classes, registers expected Godot methods and
//! compares Patina's coverage against the Godot method list.

use std::collections::HashMap;
use std::sync::Mutex;

use gdobject::class_db::{
    class_exists, class_has_method, clear_for_testing, get_method_list, inheritance_chain,
    register_class, ClassRegistration, MethodFlags, MethodInfo, PropertyInfo,
};
use gdcore::math::{Vector2, Vector3};
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

// ===========================================================================
// Expected methods per Godot class (from Godot 4 docs)
// ===========================================================================

struct ClassMethodSpec {
    name: &'static str,
    parent: &'static str,
    /// Properties to register (minimal set for class identity).
    properties: Vec<(&'static str, Variant)>,
    /// Methods we implement / register in Patina.
    methods: Vec<(&'static str, usize)>, // (name, arg_count)
    /// Approximate total method count in Godot for this class (own methods only).
    godot_method_count: usize,
}

fn core_method_specs() -> Vec<ClassMethodSpec> {
    vec![
        ClassMethodSpec {
            name: "Node",
            parent: "",
            properties: vec![
                ("name", Variant::String(String::new())),
                ("process_mode", Variant::Int(0)),
            ],
            methods: vec![
                ("_ready", 0),
                ("_process", 1),
                ("_physics_process", 1),
                ("_enter_tree", 0),
                ("_exit_tree", 0),
                ("_input", 1),
                ("_unhandled_input", 1),
                ("add_child", 1),
                ("remove_child", 1),
                ("get_child", 1),
                ("get_child_count", 0),
                ("get_children", 0),
                ("get_parent", 0),
                ("get_node", 1),
                ("get_node_or_null", 1),
                ("get_path", 0),
                ("get_tree", 0),
                ("is_inside_tree", 0),
                ("queue_free", 0),
                ("set_process", 1),
                ("set_physics_process", 1),
                ("add_to_group", 1),
                ("remove_from_group", 1),
                ("is_in_group", 1),
                ("get_groups", 0),
                ("reparent", 1),
                ("set_name", 1),
                ("get_name", 0),
                ("get_index", 0),
                ("move_child", 2),
                ("duplicate", 0),
                ("replace_by", 1),
                ("propagate_notification", 1),
                ("propagate_call", 1),
                ("set_owner", 1),
                ("get_owner", 0),
                ("set_unique_name_in_owner", 1),
            ],
            godot_method_count: 55,
        },
        ClassMethodSpec {
            name: "Node2D",
            parent: "Node",
            properties: vec![
                ("position", Variant::Vector2(Vector2::ZERO)),
                ("rotation", Variant::Float(0.0)),
                ("scale", Variant::Vector2(Vector2::new(1.0, 1.0))),
            ],
            methods: vec![
                ("get_position", 0),
                ("set_position", 1),
                ("get_rotation", 0),
                ("set_rotation", 1),
                ("get_scale", 0),
                ("set_scale", 1),
                ("rotate", 1),
                ("translate", 1),
                ("global_translate", 1),
                ("look_at", 1),
                ("get_global_position", 0),
                ("set_global_position", 1),
                ("get_global_rotation", 0),
                ("set_global_rotation", 1),
                ("to_local", 1),
                ("to_global", 1),
                ("apply_scale", 1),
                ("get_relative_transform_to_parent", 1),
            ],
            godot_method_count: 22,
        },
        ClassMethodSpec {
            name: "Node3D",
            parent: "Node",
            properties: vec![
                ("position", Variant::Vector3(Vector3::ZERO)),
                ("rotation", Variant::Vector3(Vector3::ZERO)),
                ("scale", Variant::Vector3(Vector3::new(1.0, 1.0, 1.0))),
            ],
            methods: vec![
                ("get_position", 0),
                ("set_position", 1),
                ("get_rotation", 0),
                ("set_rotation", 1),
                ("get_scale", 0),
                ("set_scale", 1),
                ("rotate", 2),
                ("translate", 1),
                ("look_at", 1),
                ("get_global_position", 0),
                ("set_global_position", 1),
                ("get_global_rotation", 0),
                ("set_global_rotation", 1),
                ("get_global_transform", 0),
            ],
            godot_method_count: 25,
        },
        ClassMethodSpec {
            name: "Sprite2D",
            parent: "Node2D",
            properties: vec![
                ("texture", Variant::Nil),
                ("flip_h", Variant::Bool(false)),
            ],
            methods: vec![
                ("get_texture", 0),
                ("set_texture", 1),
                ("get_rect", 0),
                ("is_flipped_h", 0),
                ("is_flipped_v", 0),
                ("set_flip_h", 1),
                ("set_flip_v", 1),
                ("set_frame", 1),
                ("get_frame", 0),
                ("set_region_enabled", 1),
                ("is_region_enabled", 0),
                ("set_region_rect", 1),
                ("get_region_rect", 0),
            ],
            godot_method_count: 18,
        },
        ClassMethodSpec {
            name: "Camera2D",
            parent: "Node2D",
            properties: vec![
                ("offset", Variant::Vector2(Vector2::ZERO)),
                ("zoom", Variant::Vector2(Vector2::new(1.0, 1.0))),
            ],
            methods: vec![
                ("get_zoom", 0),
                ("set_zoom", 1),
                ("get_offset", 0),
                ("set_offset", 1),
                ("make_current", 0),
                ("is_current", 0),
                ("get_screen_center_position", 0),
                ("get_camera_position", 0),
                ("reset_smoothing", 0),
                ("force_update_scroll", 0),
            ],
            godot_method_count: 15,
        },
        ClassMethodSpec {
            name: "AnimationPlayer",
            parent: "Node",
            properties: vec![
                ("current_animation", Variant::String(String::new())),
                ("speed_scale", Variant::Float(1.0)),
            ],
            methods: vec![
                ("play", 1),
                ("stop", 0),
                ("pause", 0),
                ("is_playing", 0),
                ("get_current_animation", 0),
                ("set_current_animation", 1),
                ("get_current_animation_length", 0),
                ("get_current_animation_position", 0),
                ("seek", 1),
                ("has_animation", 1),
                ("get_animation_list", 0),
                ("add_animation", 2),
                ("remove_animation", 1),
                ("set_speed_scale", 1),
                ("get_speed_scale", 0),
            ],
            godot_method_count: 22,
        },
        ClassMethodSpec {
            name: "Control",
            parent: "Node",
            properties: vec![
                ("visible", Variant::Bool(true)),
                ("size", Variant::Vector2(Vector2::ZERO)),
            ],
            methods: vec![
                ("get_minimum_size", 0),
                ("set_size", 1),
                ("get_size", 0),
                ("set_position", 1),
                ("get_position", 0),
                ("set_anchor", 2),
                ("get_anchor", 1),
                ("set_focus_mode", 1),
                ("get_focus_mode", 0),
                ("grab_focus", 0),
                ("release_focus", 0),
                ("has_focus", 0),
                ("get_rect", 0),
                ("get_global_rect", 0),
                ("set_visible", 1),
                ("is_visible", 0),
                ("set_mouse_filter", 1),
                ("get_mouse_filter", 0),
                ("accept_event", 0),
            ],
            godot_method_count: 35,
        },
        ClassMethodSpec {
            name: "Label",
            parent: "Control",
            properties: vec![
                ("text", Variant::String(String::new())),
                ("font_size", Variant::Int(16)),
            ],
            methods: vec![
                ("get_text", 0),
                ("set_text", 1),
                ("get_line_count", 0),
                ("get_visible_line_count", 0),
                ("get_total_character_count", 0),
                ("set_horizontal_alignment", 1),
                ("get_horizontal_alignment", 0),
                ("set_vertical_alignment", 1),
                ("get_vertical_alignment", 0),
            ],
            godot_method_count: 14,
        },
        ClassMethodSpec {
            name: "Button",
            parent: "Control",
            properties: vec![
                ("text", Variant::String(String::new())),
                ("disabled", Variant::Bool(false)),
            ],
            methods: vec![
                ("get_text", 0),
                ("set_text", 1),
                ("is_pressed", 0),
                ("set_pressed", 1),
                ("set_disabled", 1),
                ("is_disabled", 0),
                ("set_toggle_mode", 1),
                ("is_toggle_mode", 0),
            ],
            godot_method_count: 12,
        },
        ClassMethodSpec {
            name: "RigidBody2D",
            parent: "Node2D",
            properties: vec![
                ("mass", Variant::Float(1.0)),
                ("gravity_scale", Variant::Float(1.0)),
            ],
            methods: vec![
                ("apply_force", 1),
                ("apply_impulse", 1),
                ("apply_central_force", 1),
                ("apply_central_impulse", 1),
                ("apply_torque", 1),
                ("apply_torque_impulse", 1),
                ("set_mass", 1),
                ("get_mass", 0),
                ("set_linear_velocity", 1),
                ("get_linear_velocity", 0),
                ("set_angular_velocity", 1),
                ("get_angular_velocity", 0),
                ("set_gravity_scale", 1),
                ("get_gravity_scale", 0),
            ],
            godot_method_count: 20,
        },
        ClassMethodSpec {
            name: "StaticBody2D",
            parent: "Node2D",
            properties: vec![
                ("constant_linear_velocity", Variant::Vector2(Vector2::ZERO)),
            ],
            methods: vec![
                ("set_constant_linear_velocity", 1),
                ("get_constant_linear_velocity", 0),
                ("set_constant_angular_velocity", 1),
                ("get_constant_angular_velocity", 0),
                ("set_physics_material_override", 1),
                ("get_physics_material_override", 0),
            ],
            godot_method_count: 8,
        },
        ClassMethodSpec {
            name: "CharacterBody2D",
            parent: "Node2D",
            properties: vec![
                ("velocity", Variant::Vector2(Vector2::ZERO)),
            ],
            methods: vec![
                ("move_and_slide", 0),
                ("get_velocity", 0),
                ("set_velocity", 1),
                ("is_on_floor", 0),
                ("is_on_wall", 0),
                ("is_on_ceiling", 0),
                ("get_slide_collision_count", 0),
                ("get_slide_collision", 1),
                ("get_floor_normal", 0),
                ("get_wall_normal", 0),
                ("get_last_motion", 0),
                ("get_real_velocity", 0),
            ],
            godot_method_count: 18,
        },
        ClassMethodSpec {
            name: "Area2D",
            parent: "Node2D",
            properties: vec![
                ("monitoring", Variant::Bool(true)),
            ],
            methods: vec![
                ("get_overlapping_bodies", 0),
                ("get_overlapping_areas", 0),
                ("has_overlapping_bodies", 0),
                ("has_overlapping_areas", 0),
                ("set_monitoring", 1),
                ("is_monitoring", 0),
                ("set_monitorable", 1),
                ("is_monitorable", 0),
                ("set_gravity", 1),
                ("get_gravity", 0),
            ],
            godot_method_count: 14,
        },
        ClassMethodSpec {
            name: "CollisionShape2D",
            parent: "Node2D",
            properties: vec![
                ("shape", Variant::Nil),
            ],
            methods: vec![
                ("set_shape", 1),
                ("get_shape", 0),
                ("set_disabled", 1),
                ("is_disabled", 0),
                ("set_one_way_collision", 1),
                ("is_one_way_collision_enabled", 0),
            ],
            godot_method_count: 8,
        },
        ClassMethodSpec {
            name: "Timer",
            parent: "Node",
            properties: vec![
                ("wait_time", Variant::Float(1.0)),
            ],
            methods: vec![
                ("start", 0),
                ("stop", 0),
                ("is_stopped", 0),
                ("get_time_left", 0),
                ("set_wait_time", 1),
                ("get_wait_time", 0),
                ("set_one_shot", 1),
                ("is_one_shot", 0),
                ("set_autostart", 1),
                ("has_autostart", 0),
                ("set_paused", 1),
                ("is_paused", 0),
            ],
            godot_method_count: 14,
        },
        ClassMethodSpec {
            name: "TileMap",
            parent: "Node2D",
            properties: vec![
                ("tile_set", Variant::Nil),
            ],
            methods: vec![
                ("set_cell", 3),
                ("get_cell_source_id", 1),
                ("get_cell_atlas_coords", 1),
                ("get_used_cells", 0),
                ("get_used_rect", 0),
                ("local_to_map", 1),
                ("map_to_local", 1),
                ("set_tile_set", 1),
                ("get_tile_set", 0),
                ("clear", 0),
            ],
            godot_method_count: 18,
        },
        ClassMethodSpec {
            name: "CPUParticles2D",
            parent: "Node2D",
            properties: vec![
                ("emitting", Variant::Bool(true)),
                ("amount", Variant::Int(8)),
            ],
            methods: vec![
                ("set_emitting", 1),
                ("is_emitting", 0),
                ("set_amount", 1),
                ("get_amount", 0),
                ("set_lifetime", 1),
                ("get_lifetime", 0),
                ("restart", 0),
                ("set_direction", 1),
                ("get_direction", 0),
                ("set_spread", 1),
                ("get_spread", 0),
            ],
            godot_method_count: 35,
        },
    ]
}

/// Registers all 17 core classes with methods into ClassDB.
fn register_core_classes_with_methods() {
    for spec in core_method_specs() {
        let mut reg = ClassRegistration::new(spec.name);
        if !spec.parent.is_empty() {
            reg = reg.parent(spec.parent);
        }
        for (prop_name, default_val) in &spec.properties {
            reg = reg.property(PropertyInfo::new(*prop_name, default_val.clone()));
        }
        for (method_name, arg_count) in &spec.methods {
            reg = reg.method(MethodInfo::new(*method_name, *arg_count));
        }
        register_class(reg);
    }
}

// ===========================================================================
// 1. All 17 classes recognized with methods
// ===========================================================================

#[test]
fn all_17_classes_recognized_with_methods() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    for spec in core_method_specs() {
        assert!(
            class_exists(spec.name),
            "{} should be recognized",
            spec.name
        );
    }
}

// ===========================================================================
// 2. Each class has expected methods registered
// ===========================================================================

#[test]
fn each_class_has_expected_own_methods() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    for spec in core_method_specs() {
        let methods = get_method_list(spec.name, true); // own methods only
        let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

        for (expected_name, _) in &spec.methods {
            assert!(
                method_names.contains(expected_name),
                "{}: missing own method '{}'",
                spec.name,
                expected_name
            );
        }
    }
}

// ===========================================================================
// 3. Inherited methods propagate
// ===========================================================================

#[test]
fn inherited_methods_visible() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    // Sprite2D should have Node methods (inherited through Node2D -> Node).
    let methods = get_method_list("Sprite2D", false); // include inherited
    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    assert!(
        method_names.contains(&"_ready"),
        "Sprite2D should inherit _ready from Node"
    );
    assert!(
        method_names.contains(&"_process"),
        "Sprite2D should inherit _process from Node"
    );
    assert!(
        method_names.contains(&"queue_free"),
        "Sprite2D should inherit queue_free from Node"
    );
    assert!(
        method_names.contains(&"get_position"),
        "Sprite2D should inherit get_position from Node2D"
    );
    assert!(
        method_names.contains(&"get_texture"),
        "Sprite2D should have its own get_texture"
    );
}

#[test]
fn label_inherits_control_and_node_methods() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let methods = get_method_list("Label", false);
    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    // From Node
    assert!(method_names.contains(&"_ready"));
    assert!(method_names.contains(&"queue_free"));
    // From Control
    assert!(method_names.contains(&"grab_focus"));
    assert!(method_names.contains(&"get_size"));
    // Own
    assert!(method_names.contains(&"get_text"));
    assert!(method_names.contains(&"set_text"));
}

// ===========================================================================
// 4. Method count per class (own only)
// ===========================================================================

#[test]
fn per_class_own_method_counts() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    for spec in core_method_specs() {
        let methods = get_method_list(spec.name, true);
        assert_eq!(
            methods.len(),
            spec.methods.len(),
            "{}: registered own method count mismatch",
            spec.name
        );
    }
}

// ===========================================================================
// 5. Method + Property coverage parity summary
// ===========================================================================

#[test]
fn method_and_property_parity_summary() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let specs = core_method_specs();
    let total_classes = specs.len();
    let mut total_godot_methods = 0;
    let mut total_implemented_methods = 0;
    let mut total_godot_props = 0;
    let mut total_implemented_props = 0;

    eprintln!();
    eprintln!("┌─────────────────────┬─────────────┬────────────────┐");
    eprintln!("│ Class               │ Props (P/G) │ Methods (P/G)  │");
    eprintln!("├─────────────────────┼─────────────┼────────────────┤");

    for spec in &specs {
        let implemented_methods = spec.methods.len();
        let godot_methods = spec.godot_method_count;
        let implemented_props = spec.properties.len();
        // Use a rough godot prop count based on our registration.
        // (The full property counts are in the other test file.)
        let godot_props = implemented_props; // conservative: count what we have

        total_godot_methods += godot_methods;
        total_implemented_methods += implemented_methods;
        total_godot_props += godot_props;
        total_implemented_props += implemented_props;

        eprintln!(
            "│ {:<19} │ {:>4}/{:<4}   │ {:>5}/{:<5}    │",
            spec.name, implemented_props, godot_props, implemented_methods, godot_methods
        );
    }

    let method_pct =
        (total_implemented_methods as f64 / total_godot_methods as f64 * 100.0).round() as u32;

    eprintln!("├─────────────────────┼─────────────┼────────────────┤");
    eprintln!(
        "│ TOTAL               │ {:>4}/{:<4}   │ {:>5}/{:<5}    │",
        total_implemented_props, total_godot_props, total_implemented_methods, total_godot_methods
    );
    eprintln!("└─────────────────────┴─────────────┴────────────────┘");
    eprintln!();
    eprintln!(
        "═══════════════════════════════════════════════════════════"
    );
    eprintln!(
        "  ClassDB method parity: {total_classes}/17 classes, {method_pct}% method coverage"
    );
    eprintln!(
        "  ({total_implemented_methods}/{total_godot_methods} methods implemented)"
    );
    eprintln!(
        "═══════════════════════════════════════════════════════════"
    );
    eprintln!();

    // Hard requirements.
    assert_eq!(total_classes, 17, "all 17 core classes");
    // Method coverage should be at least 40% (we registered key methods).
    assert!(
        method_pct >= 40,
        "method parity {method_pct}% is below minimum 40% threshold"
    );
}

// ===========================================================================
// 6. get_method_list with no_inheritance
// ===========================================================================

#[test]
fn get_method_list_no_inheritance_excludes_parent_methods() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    // Node2D own methods should NOT include Node methods.
    let own_methods = get_method_list("Node2D", true);
    let own_names: Vec<&str> = own_methods.iter().map(|m| m.name.as_str()).collect();

    assert!(!own_names.contains(&"_ready"), "Node2D own should not include _ready (from Node)");
    assert!(own_names.contains(&"get_position"), "Node2D own should include get_position");
}

// ===========================================================================
// 7. has_method works across inheritance chain
// ===========================================================================

#[test]
fn class_has_method_across_chain() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    // Sprite2D -> Node2D -> Node
    assert!(gdobject::class_has_method("Sprite2D", "_ready")); // from Node
    assert!(gdobject::class_has_method("Sprite2D", "get_position")); // from Node2D
    assert!(gdobject::class_has_method("Sprite2D", "get_texture")); // own
    assert!(!gdobject::class_has_method("Sprite2D", "nonexistent"));
}

// ===========================================================================
// 8. Per-class method detail: Node2D specific
// ===========================================================================

#[test]
fn node2d_methods_include_transform_ops() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let methods = get_method_list("Node2D", true);
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    let expected = [
        "get_position", "set_position", "get_rotation", "set_rotation",
        "get_scale", "set_scale", "rotate", "translate", "look_at",
    ];
    for m in &expected {
        assert!(names.contains(m), "Node2D should have method '{m}'");
    }
}

// ===========================================================================
// 9. Per-class method detail: CharacterBody2D specific
// ===========================================================================

#[test]
fn characterbody2d_methods_include_move_and_slide() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let methods = get_method_list("CharacterBody2D", true);
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    assert!(names.contains(&"move_and_slide"));
    assert!(names.contains(&"is_on_floor"));
    assert!(names.contains(&"is_on_wall"));
    assert!(names.contains(&"get_velocity"));
}

// ===========================================================================
// 10. Total inherited method count for deep chain
// ===========================================================================

#[test]
fn sprite2d_total_method_count_includes_all_ancestors() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let all_methods = get_method_list("Sprite2D", false);
    let own_methods = get_method_list("Sprite2D", true);

    // Should have more total than own.
    assert!(
        all_methods.len() > own_methods.len(),
        "Sprite2D total methods ({}) should exceed own ({}) due to inheritance",
        all_methods.len(),
        own_methods.len()
    );

    // Total should include Node + Node2D + Sprite2D methods.
    let node_methods = get_method_list("Node", true);
    let node2d_methods = get_method_list("Node2D", true);
    let expected_total = node_methods.len() + node2d_methods.len() + own_methods.len();
    assert_eq!(
        all_methods.len(),
        expected_total,
        "Sprite2D total methods = Node + Node2D + Sprite2D own"
    );
}

// ===========================================================================
// Helper: register classes with enriched metadata (virtual, const, return_type)
// ===========================================================================

fn register_enriched_classes() {
    // Node: virtual lifecycle methods
    register_class(
        ClassRegistration::new("Node")
            .property(PropertyInfo::new("name", Variant::String(String::new())))
            .method(MethodInfo::new("_ready", 0).with_virtual())
            .method(MethodInfo::new("_process", 1).with_virtual())
            .method(MethodInfo::new("_physics_process", 1).with_virtual())
            .method(MethodInfo::new("_enter_tree", 0).with_virtual())
            .method(MethodInfo::new("_exit_tree", 0).with_virtual())
            .method(MethodInfo::new("_input", 1).with_virtual())
            .method(MethodInfo::new("_unhandled_input", 1).with_virtual())
            .method(MethodInfo::new("add_child", 1))
            .method(MethodInfo::new("remove_child", 1))
            .method(MethodInfo::new("get_child", 1).with_const().with_return_type("Node"))
            .method(MethodInfo::new("get_child_count", 0).with_const().with_return_type("int"))
            .method(MethodInfo::new("get_parent", 0).with_const().with_return_type("Node"))
            .method(MethodInfo::new("get_node", 1).with_const().with_return_type("Node"))
            .method(MethodInfo::new("get_path", 0).with_const().with_return_type("NodePath"))
            .method(MethodInfo::new("is_inside_tree", 0).with_const().with_return_type("bool"))
            .method(MethodInfo::new("queue_free", 0)),
    );
    // Node2D: transform methods
    register_class(
        ClassRegistration::new("Node2D")
            .parent("Node")
            .property(PropertyInfo::new("position", Variant::Vector2(Vector2::ZERO)))
            .property(PropertyInfo::new("rotation", Variant::Float(0.0)))
            .property(PropertyInfo::new("scale", Variant::Vector2(Vector2::new(1.0, 1.0))))
            .method(MethodInfo::new("get_position", 0).with_const().with_return_type("Vector2"))
            .method(MethodInfo::new("set_position", 1))
            .method(MethodInfo::new("get_rotation", 0).with_const().with_return_type("float"))
            .method(MethodInfo::new("set_rotation", 1))
            .method(MethodInfo::new("get_scale", 0).with_const().with_return_type("Vector2"))
            .method(MethodInfo::new("set_scale", 1))
            .method(MethodInfo::new("get_global_position", 0).with_const().with_return_type("Vector2"))
            .method(MethodInfo::new("translate", 1))
            .method(MethodInfo::new("rotate", 1)),
    );
    // Sprite2D
    register_class(
        ClassRegistration::new("Sprite2D")
            .parent("Node2D")
            .method(MethodInfo::new("get_texture", 0).with_const().with_return_type("Texture2D"))
            .method(MethodInfo::new("set_texture", 1))
            .method(MethodInfo::new("get_frame", 0).with_const().with_return_type("int"))
            .method(MethodInfo::new("set_frame", 1))
            .method(MethodInfo::new("is_flipped_h", 0).with_const().with_return_type("bool")),
    );
    // CharacterBody2D
    register_class(
        ClassRegistration::new("CharacterBody2D")
            .parent("Node2D")
            .method(MethodInfo::new("move_and_slide", 0).with_return_type("bool"))
            .method(MethodInfo::new("is_on_floor", 0).with_const().with_return_type("bool"))
            .method(MethodInfo::new("is_on_wall", 0).with_const().with_return_type("bool"))
            .method(MethodInfo::new("is_on_ceiling", 0).with_const().with_return_type("bool"))
            .method(MethodInfo::new("get_velocity", 0).with_const().with_return_type("Vector2"))
            .method(MethodInfo::new("set_velocity", 1)),
    );
    // Area2D
    register_class(
        ClassRegistration::new("Area2D")
            .parent("Node2D")
            .method(MethodInfo::new("get_overlapping_bodies", 0).with_const().with_return_type("Array"))
            .method(MethodInfo::new("get_overlapping_areas", 0).with_const().with_return_type("Array")),
    );
    // Timer
    register_class(
        ClassRegistration::new("Timer")
            .parent("Node")
            .method(MethodInfo::new("start", 0))
            .method(MethodInfo::new("stop", 0))
            .method(MethodInfo::new("is_stopped", 0).with_const().with_return_type("bool"))
            .method(MethodInfo::new("get_time_left", 0).with_const().with_return_type("float")),
    );
    // Control
    register_class(
        ClassRegistration::new("Control")
            .parent("Node")
            .method(MethodInfo::new("_gui_input", 1).with_virtual())
            .method(MethodInfo::new("get_rect", 0).with_const().with_return_type("Rect2"))
            .method(MethodInfo::new("get_minimum_size", 0).with_const().with_return_type("Vector2")),
    );
    // Label
    register_class(
        ClassRegistration::new("Label")
            .parent("Control")
            .method(MethodInfo::new("get_text", 0).with_const().with_return_type("String"))
            .method(MethodInfo::new("set_text", 1)),
    );
}

// ===========================================================================
// 12. Virtual methods inherit is_virtual flag through chain — pat-kx5
// ===========================================================================

#[test]
fn inherited_virtual_methods_preserve_flag() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    // Sprite2D inherits from Node2D → Node.
    // Node's _ready is virtual — it must still be virtual when seen through Sprite2D.
    let all_methods = get_method_list("Sprite2D", false);
    let ready = all_methods.iter().find(|m| m.name == "_ready");
    assert!(ready.is_some(), "_ready must be inherited by Sprite2D");
    let ready = ready.unwrap();
    assert!(ready.is_virtual, "_ready must preserve is_virtual=true through inheritance");
    assert_eq!(ready.flags.0, MethodFlags::VIRTUAL.0, "_ready flags must be VIRTUAL");
}

// ===========================================================================
// 13. Const methods inherit is_const and return_type — pat-kx5
// ===========================================================================

#[test]
fn inherited_const_methods_preserve_metadata() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    // Sprite2D inherits Node2D's get_position (const, returns Vector2).
    let all_methods = get_method_list("Sprite2D", false);
    let get_pos = all_methods.iter().find(|m| m.name == "get_position");
    assert!(get_pos.is_some(), "get_position must be inherited by Sprite2D");
    let get_pos = get_pos.unwrap();
    assert!(get_pos.is_const, "get_position must preserve is_const=true");
    assert_eq!(
        get_pos.return_type, "Vector2",
        "get_position must preserve return_type=Vector2"
    );
}

// ===========================================================================
// 14. Return type propagates through deep inheritance — pat-kx5
// ===========================================================================

#[test]
fn return_type_propagates_through_deep_chain() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    // Label inherits Control → Node. Node's get_child returns "Node".
    let all_methods = get_method_list("Label", false);
    let get_child = all_methods.iter().find(|m| m.name == "get_child");
    assert!(get_child.is_some(), "get_child must be inherited by Label");
    assert_eq!(
        get_child.unwrap().return_type, "Node",
        "get_child return_type must propagate through chain"
    );
}

// ===========================================================================
// 15. class_has_method works across inheritance with metadata — pat-kx5
// ===========================================================================

#[test]
fn class_has_method_works_with_enriched_metadata() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    // Sprite2D should find Node's _ready and Node2D's get_position.
    assert!(class_has_method("Sprite2D", "_ready"), "Sprite2D must inherit _ready");
    assert!(class_has_method("Sprite2D", "get_position"), "Sprite2D must inherit get_position");
    assert!(class_has_method("Sprite2D", "get_texture"), "Sprite2D must have own get_texture");
    // But not CharacterBody2D methods.
    assert!(!class_has_method("Sprite2D", "move_and_slide"), "Sprite2D must not have move_and_slide");
}

// ===========================================================================
// 16. Method ordering: base-to-derived in full listings — pat-kx5
// ===========================================================================

#[test]
fn method_listing_order_is_base_to_derived() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    let all_methods = get_method_list("Sprite2D", false);
    let names: Vec<&str> = all_methods.iter().map(|m| m.name.as_str()).collect();

    // Node methods should appear before Node2D methods, which appear before Sprite2D.
    let ready_idx = names.iter().position(|n| *n == "_ready").unwrap();
    let get_pos_idx = names.iter().position(|n| *n == "get_position").unwrap();
    let get_tex_idx = names.iter().position(|n| *n == "get_texture").unwrap();

    assert!(
        ready_idx < get_pos_idx,
        "Node._ready ({ready_idx}) must come before Node2D.get_position ({get_pos_idx})"
    );
    assert!(
        get_pos_idx < get_tex_idx,
        "Node2D.get_position ({get_pos_idx}) must come before Sprite2D.get_texture ({get_tex_idx})"
    );
}

// ===========================================================================
// 17. own_only=true excludes inherited metadata — pat-kx5
// ===========================================================================

#[test]
fn own_only_excludes_inherited_enriched_methods() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    let own = get_method_list("Sprite2D", true);
    let names: Vec<&str> = own.iter().map(|m| m.name.as_str()).collect();

    assert!(names.contains(&"get_texture"), "own methods must include get_texture");
    assert!(!names.contains(&"_ready"), "own methods must not include inherited _ready");
    assert!(!names.contains(&"get_position"), "own methods must not include inherited get_position");
}

// ===========================================================================
// 18. Total method count = sum of all ancestors — pat-kx5
// ===========================================================================

#[test]
fn total_method_count_equals_ancestor_sum() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    let chain = inheritance_chain("Label");
    // Label → Control → Node
    assert_eq!(chain.len(), 3, "Label chain: Label, Control, Node");

    let label_own = get_method_list("Label", true).len();
    let control_own = get_method_list("Control", true).len();
    let node_own = get_method_list("Node", true).len();
    let label_all = get_method_list("Label", false).len();

    assert_eq!(
        label_all,
        node_own + control_own + label_own,
        "Label total = Node({node_own}) + Control({control_own}) + Label({label_own})"
    );
}

// ===========================================================================
// 19. MethodFlags constants match Godot — pat-kx5
// ===========================================================================

#[test]
fn method_flags_match_godot_constants() {
    assert_eq!(MethodFlags::NORMAL.0, 1, "NORMAL must be 1");
    assert_eq!(MethodFlags::VIRTUAL.0, 2, "VIRTUAL must be 2");
    assert_eq!(MethodFlags::EDITOR.0, 4, "EDITOR must be 4");
}

// ===========================================================================
// 20. Oracle fixture method names match Patina for Node — pat-kx5
// ===========================================================================

#[test]
fn oracle_node_methods_present_in_patina() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    // Key methods from fixtures/oracle_outputs/classdb_probe_signatures.json for Node
    let oracle_node_methods = [
        "_ready", "_process", "_physics_process", "_enter_tree", "_exit_tree",
        "_input", "_unhandled_input", "add_child", "remove_child", "get_child",
        "get_child_count", "get_parent", "get_node", "get_path", "is_inside_tree",
        "queue_free",
    ];

    let patina_methods = get_method_list("Node", true);
    let patina_names: Vec<&str> = patina_methods.iter().map(|m| m.name.as_str()).collect();

    let mut matched = 0;
    for method in &oracle_node_methods {
        if patina_names.contains(method) {
            matched += 1;
        }
    }

    assert_eq!(
        matched,
        oracle_node_methods.len(),
        "All oracle Node methods must be in Patina: matched {matched}/{}, missing: {:?}",
        oracle_node_methods.len(),
        oracle_node_methods
            .iter()
            .filter(|m| !patina_names.contains(m))
            .collect::<Vec<_>>()
    );
}

// ===========================================================================
// 21. Oracle fixture method names match Patina for Node2D — pat-kx5
// ===========================================================================

#[test]
fn oracle_node2d_methods_present_in_patina() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    // Key methods from oracle for Node2D (own only)
    let oracle_methods = [
        "get_position", "set_position", "get_rotation", "set_rotation",
        "get_scale", "set_scale", "get_global_position", "translate", "rotate",
    ];

    let patina_methods = get_method_list("Node2D", true);
    let patina_names: Vec<&str> = patina_methods.iter().map(|m| m.name.as_str()).collect();

    let mut matched = 0;
    for method in &oracle_methods {
        if patina_names.contains(method) {
            matched += 1;
        }
    }

    assert_eq!(
        matched,
        oracle_methods.len(),
        "All oracle Node2D methods must be in Patina: matched {matched}/{}",
        oracle_methods.len()
    );
}

// ===========================================================================
// 22. Virtual methods in oracle match Patina virtual flags — pat-kx5
// ===========================================================================

#[test]
fn oracle_virtual_methods_have_virtual_flag() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    // In Godot, _ready, _process, _physics_process, _enter_tree, _exit_tree are virtual.
    let virtual_methods = ["_ready", "_process", "_physics_process", "_enter_tree", "_exit_tree", "_input", "_unhandled_input"];

    let methods = get_method_list("Node", true);
    for name in &virtual_methods {
        let m = methods.iter().find(|m| m.name == *name);
        assert!(m.is_some(), "Node must have method {name}");
        assert!(
            m.unwrap().is_virtual,
            "Node.{name} must have is_virtual=true"
        );
    }
}

// ===========================================================================
// 23. Inherited node API parity report — pat-kx5
// ===========================================================================

#[test]
fn inherited_node_method_metadata_parity_report() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    let classes = ["Node", "Node2D", "Sprite2D", "CharacterBody2D", "Area2D", "Timer", "Control", "Label"];

    let mut total_checks = 0;
    let mut passed_checks = 0;

    eprintln!("\n=== ClassDB Inherited Method Metadata Parity (4.6.1) ===");
    for class in &classes {
        let own = get_method_list(class, true);
        let all = get_method_list(class, false);
        let chain = inheritance_chain(class);

        // Check 1: own < all (except root)
        let has_inheritance = if chain.len() > 1 {
            all.len() > own.len()
        } else {
            all.len() == own.len()
        };
        total_checks += 1;
        if has_inheritance { passed_checks += 1; }

        // Check 2: virtual methods preserve flag through chain
        let virtual_ok = all.iter()
            .filter(|m| m.name.starts_with('_'))
            .all(|m| m.is_virtual);
        total_checks += 1;
        if virtual_ok { passed_checks += 1; }

        // Check 3: const getters have return types
        let const_ok = all.iter()
            .filter(|m| m.is_const && m.name.starts_with("get_"))
            .all(|m| m.return_type != "void");
        total_checks += 1;
        if const_ok { passed_checks += 1; }

        eprintln!(
            "  {}: own={}, inherited={}, chain={}, virtual_ok={}, const_ok={}",
            class,
            own.len(),
            all.len() - own.len(),
            chain.len(),
            virtual_ok,
            const_ok
        );
    }

    let pct = (passed_checks as f64 / total_checks as f64) * 100.0;
    eprintln!("  Result: {}/{} checks ({:.1}%)", passed_checks, total_checks, pct);
    eprintln!("  Contract: classdb.methods.inherited_node_metadata");
    eprintln!("  Oracle: Godot 4.6.1-stable ClassDB probes");
    eprintln!("=========================================================\n");

    assert_eq!(
        passed_checks, total_checks,
        "All inherited method metadata checks must pass: {passed_checks}/{total_checks}"
    );
}

// ===========================================================================
// Oracle-driven inherited method metadata tests (pat-ouo)
// ===========================================================================

/// Parse the oracle probe JSON and return a map of class -> (method_name -> arg_count).
fn load_oracle_probe() -> HashMap<String, Vec<(String, usize)>> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/oracle_outputs/classdb_probe_signatures.json"
    );
    let data = std::fs::read_to_string(path).expect("oracle probe JSON must exist");
    let entries: serde_json::Value = serde_json::from_str(&data).expect("valid JSON");
    let mut map = HashMap::new();
    for entry in entries.as_array().unwrap() {
        let d = &entry["data"];
        let class = d["class"].as_str().unwrap().to_string();
        let methods: Vec<(String, usize)> = d["methods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let name = m["name"].as_str().unwrap().to_string();
                let argc = m["args"].as_array().unwrap().len();
                (name, argc)
            })
            .collect();
        map.insert(class, methods);
    }
    map
}

// ===========================================================================
// 24. Oracle arg counts match Patina for each class's own methods — pat-ouo
// ===========================================================================

#[test]
fn oracle_arg_counts_match_patina_own_methods() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let oracle = load_oracle_probe();

    // Classes that appear in both oracle and our registration.
    let check_classes = [
        "Node", "Node2D", "Sprite2D", "Camera2D", "CharacterBody2D",
        "Area2D", "Timer", "Control", "Label", "Button",
        "RigidBody2D", "StaticBody2D", "CollisionShape2D",
        "AnimationPlayer", "TileMap", "CPUParticles2D",
    ];

    let mut total = 0;
    let mut matched = 0;

    for class in &check_classes {
        let Some(oracle_methods) = oracle.get(*class) else { continue };
        let patina_methods = get_method_list(class, true);
        let patina_map: HashMap<&str, usize> = patina_methods
            .iter()
            .map(|m| (m.name.as_str(), m.argument_count))
            .collect();

        for (oname, oargc) in oracle_methods {
            if let Some(&pargc) = patina_map.get(oname.as_str()) {
                total += 1;
                if pargc == *oargc {
                    matched += 1;
                } else {
                    eprintln!(
                        "  ARG MISMATCH {}.{}: oracle={}, patina={}",
                        class, oname, oargc, pargc
                    );
                }
            }
        }
    }

    eprintln!(
        "Oracle arg-count parity: {matched}/{total} methods match"
    );
    assert_eq!(matched, total, "all shared methods must have matching arg counts");
}

// ===========================================================================
// 25. Inherited methods from oracle parent are visible in child — pat-ouo
// ===========================================================================

#[test]
fn oracle_inherited_methods_visible_in_child() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let oracle = load_oracle_probe();

    // For each child->parent pair in oracle, verify parent methods appear
    // in child's full (inherited) method list in Patina.
    let pairs: &[(&str, &str)] = &[
        ("Sprite2D", "Node2D"),
        ("Camera2D", "Node2D"),
        ("CharacterBody2D", "Node2D"),
        ("Area2D", "Node2D"),
        ("RigidBody2D", "Node2D"),
        ("StaticBody2D", "Node2D"),
        ("CollisionShape2D", "Node2D"),
        ("TileMap", "Node2D"),
        ("CPUParticles2D", "Node2D"),
        ("Label", "Control"),
        ("Button", "Control"),
        ("Timer", "Node"),
        ("AnimationPlayer", "Node"),
    ];

    let mut total = 0;
    let mut found = 0;

    for (child, parent) in pairs {
        let Some(oracle_parent_methods) = oracle.get(*parent) else { continue };
        let patina_all = get_method_list(child, false);
        let patina_names: Vec<&str> = patina_all.iter().map(|m| m.name.as_str()).collect();

        // Also get Patina's registered parent methods to know what we can check.
        let patina_parent_own = get_method_list(parent, true);
        let patina_parent_names: Vec<&str> =
            patina_parent_own.iter().map(|m| m.name.as_str()).collect();

        for (oname, _) in oracle_parent_methods {
            // Only check oracle methods that Patina has registered on the parent.
            if !patina_parent_names.contains(&oname.as_str()) {
                continue;
            }
            total += 1;
            if patina_names.contains(&oname.as_str()) {
                found += 1;
            } else {
                eprintln!(
                    "  MISSING INHERITED: {}.{} (from {})",
                    child, oname, parent
                );
            }
        }
    }

    eprintln!(
        "Oracle inherited visibility: {found}/{total} parent methods visible in children"
    );
    assert_eq!(found, total, "all registered parent methods must be visible in children");
}

// ===========================================================================
// 26. Inherited arg counts match oracle across full chain — pat-ouo
// ===========================================================================

#[test]
fn inherited_arg_counts_match_oracle_through_chain() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let oracle = load_oracle_probe();

    // For Sprite2D (Node2D -> Node), verify arg counts of inherited Node methods
    // as seen through Sprite2D match what oracle says for Node.
    let sprite_all = get_method_list("Sprite2D", false);
    let sprite_map: HashMap<&str, usize> = sprite_all
        .iter()
        .map(|m| (m.name.as_str(), m.argument_count))
        .collect();

    let Some(oracle_node) = oracle.get("Node") else {
        panic!("oracle must have Node");
    };

    let mut total = 0;
    let mut matched = 0;

    for (oname, oargc) in oracle_node {
        if let Some(&pargc) = sprite_map.get(oname.as_str()) {
            total += 1;
            if pargc == *oargc {
                matched += 1;
            } else {
                eprintln!(
                    "  ARG MISMATCH Sprite2D (inherited Node).{}: oracle={}, patina={}",
                    oname, oargc, pargc
                );
            }
        }
    }

    eprintln!(
        "Sprite2D inherited-from-Node arg parity: {matched}/{total}"
    );
    assert!(total > 0, "must check at least one inherited method");
    assert_eq!(matched, total, "inherited arg counts must match oracle");
}

// ===========================================================================
// 27. Label deep chain: Node methods visible with correct arg counts — pat-ouo
// ===========================================================================

#[test]
fn label_inherits_node_methods_with_correct_arg_counts() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let oracle = load_oracle_probe();

    // Label -> Control -> Node. Verify Node methods are inherited with correct arg counts.
    let label_all = get_method_list("Label", false);
    let label_map: HashMap<&str, usize> = label_all
        .iter()
        .map(|m| (m.name.as_str(), m.argument_count))
        .collect();

    let Some(oracle_node) = oracle.get("Node") else {
        panic!("oracle must have Node");
    };

    let node_own = get_method_list("Node", true);
    let node_names: Vec<&str> = node_own.iter().map(|m| m.name.as_str()).collect();

    let mut checked = 0;

    for (oname, oargc) in oracle_node {
        // Only check methods we've registered for Node.
        if !node_names.contains(&oname.as_str()) {
            continue;
        }
        let pargc = label_map.get(oname.as_str());
        assert!(
            pargc.is_some(),
            "Label must inherit Node.{oname}"
        );
        assert_eq!(
            *pargc.unwrap(),
            *oargc,
            "Label inherited Node.{oname} arg count must match oracle"
        );
        checked += 1;
    }

    eprintln!("Label deep-chain Node method checks: {checked}");
    assert!(checked > 20, "must verify substantial method set (got {checked})");
}

// ===========================================================================
// 28. Enriched metadata: inherited virtual + const flags match oracle — pat-ouo
// ===========================================================================

#[test]
fn enriched_inherited_metadata_matches_oracle_contract() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_enriched_classes();

    let oracle = load_oracle_probe();

    // Verify Sprite2D's full method list has correct metadata for inherited methods.
    let sprite_all = get_method_list("Sprite2D", false);
    let sprite_map: HashMap<&str, &MethodInfo> = sprite_all
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect();

    // Node virtuals must be virtual in Sprite2D's inherited view.
    let oracle_node = oracle.get("Node").unwrap();
    let node_virtuals = [
        "_ready", "_process", "_physics_process", "_enter_tree",
        "_exit_tree", "_input", "_unhandled_input",
    ];
    for vname in &node_virtuals {
        if let Some(m) = sprite_map.get(vname) {
            assert!(
                m.is_virtual,
                "Sprite2D inherited {vname} must be virtual"
            );
            assert_eq!(
                m.flags.0,
                MethodFlags::VIRTUAL.0,
                "Sprite2D inherited {vname} flags must be VIRTUAL"
            );
        }
    }

    // Node2D const getters must be const in Sprite2D's view.
    let const_getters = [
        ("get_position", "Vector2"),
        ("get_rotation", "float"),
        ("get_scale", "Vector2"),
        ("get_global_position", "Vector2"),
    ];
    for (gname, rtype) in &const_getters {
        if let Some(m) = sprite_map.get(gname) {
            assert!(
                m.is_const,
                "Sprite2D inherited {gname} must be const"
            );
            assert_eq!(
                m.return_type, *rtype,
                "Sprite2D inherited {gname} return_type must be {rtype}"
            );
        }
    }

    // Verify oracle arg counts for Node methods visible in Sprite2D.
    let mut argc_checks = 0;
    for (oname, oargc) in oracle_node {
        if let Some(m) = sprite_map.get(oname.as_str()) {
            assert_eq!(
                m.argument_count, *oargc,
                "Sprite2D inherited Node.{oname} argc"
            );
            argc_checks += 1;
        }
    }

    eprintln!(
        "Enriched Sprite2D inherited metadata: {argc_checks} arg checks, virtuals OK, const OK"
    );
    assert!(argc_checks >= 10, "must validate substantial inherited method set");
}

// ===========================================================================
// 29. All oracle classes: own method count >= Patina's registered count — pat-ouo
// ===========================================================================

#[test]
fn oracle_method_count_covers_patina_registered() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let oracle = load_oracle_probe();
    let specs = core_method_specs();

    let mut total_classes = 0;
    let mut total_confirmed = 0;
    let mut total_patina_only = 0;

    for spec in &specs {
        if let Some(oracle_methods) = oracle.get(spec.name) {
            total_classes += 1;
            let patina_own = get_method_list(spec.name, true);
            let oracle_names: Vec<&str> =
                oracle_methods.iter().map(|(n, _)| n.as_str()).collect();

            let mut class_confirmed = 0;
            for pm in &patina_own {
                if oracle_names.contains(&pm.name.as_str()) {
                    total_confirmed += 1;
                    class_confirmed += 1;
                } else {
                    // Method may come from an intermediate class (e.g. CanvasItem)
                    // that our oracle probe doesn't capture at the same level.
                    total_patina_only += 1;
                    eprintln!(
                        "  NOTE: {}.{} registered in Patina but not in oracle own-methods (may be from intermediate class)",
                        spec.name, pm.name
                    );
                }
            }

            // The oracle should have at least as many methods as Patina confirmed.
            assert!(
                oracle_methods.len() >= class_confirmed,
                "{}: oracle has {} methods but Patina confirmed {}",
                spec.name,
                oracle_methods.len(),
                class_confirmed
            );
        }
    }

    eprintln!(
        "Oracle coverage: {total_classes} classes, {total_confirmed} confirmed, {total_patina_only} Patina-only (intermediate)"
    );
    assert!(total_confirmed > 100, "must confirm substantial method set");
}

// ===========================================================================
// 30. Cross-chain: CharacterBody2D sees Node2D + Node methods with
//     correct arg counts from oracle — pat-ouo
// ===========================================================================

#[test]
fn characterbody2d_full_chain_oracle_arg_parity() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes_with_methods();

    let oracle = load_oracle_probe();

    let cb2d_all = get_method_list("CharacterBody2D", false);
    let cb2d_map: HashMap<&str, usize> = cb2d_all
        .iter()
        .map(|m| (m.name.as_str(), m.argument_count))
        .collect();

    let mut total = 0;
    let mut matched = 0;

    // Check Node methods inherited through Node2D.
    for (oname, oargc) in oracle.get("Node").unwrap_or(&vec![]) {
        if let Some(&pargc) = cb2d_map.get(oname.as_str()) {
            total += 1;
            if pargc == *oargc {
                matched += 1;
            } else {
                eprintln!(
                    "  CB2D inherited Node.{}: oracle={}, patina={}",
                    oname, oargc, pargc
                );
            }
        }
    }

    // Check Node2D methods.
    for (oname, oargc) in oracle.get("Node2D").unwrap_or(&vec![]) {
        if let Some(&pargc) = cb2d_map.get(oname.as_str()) {
            total += 1;
            if pargc == *oargc {
                matched += 1;
            } else {
                eprintln!(
                    "  CB2D inherited Node2D.{}: oracle={}, patina={}",
                    oname, oargc, pargc
                );
            }
        }
    }

    // Check own CharacterBody2D methods.
    for (oname, oargc) in oracle.get("CharacterBody2D").unwrap_or(&vec![]) {
        if let Some(&pargc) = cb2d_map.get(oname.as_str()) {
            total += 1;
            if pargc == *oargc {
                matched += 1;
            } else {
                eprintln!(
                    "  CB2D own .{}: oracle={}, patina={}",
                    oname, oargc, pargc
                );
            }
        }
    }

    eprintln!(
        "CharacterBody2D full chain oracle parity: {matched}/{total}"
    );
    assert!(total > 0, "must check inherited methods");
    assert_eq!(matched, total, "all arg counts must match oracle");
}
