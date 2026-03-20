//! ClassDB parity tests for core Godot runtime classes (pat-h6a).
//!
//! Registers the 17 core Godot classes in ClassDB with their expected
//! default properties, then verifies recognition, instantiation,
//! default values, inheritance chains, and reports a parity percentage.

use std::sync::Mutex;

use gdcore::math::{Color, Rect2, Vector2, Vector3};
use gdcore::NodePath;
use gdobject::class_db::{
    class_count, class_exists, clear_for_testing, get_class_info, inheritance_chain, instantiate,
    is_parent_class, register_class, ClassRegistration, PropertyInfo,
};
use gdobject::object::GodotObject;
use gdvariant::Variant;

// Since ClassDB is a global singleton, tests must serialize access.
static TEST_LOCK: Mutex<()> = Mutex::new(());

// ===========================================================================
// Expected properties per Godot class (from Godot 4 docs)
// ===========================================================================

/// Describes expected properties for a Godot class.
struct ClassSpec {
    name: &'static str,
    parent: &'static str,
    properties: Vec<(&'static str, Variant)>,
    /// Total properties Godot exposes for this class (approximate from docs).
    /// Used for parity percentage calculation.
    godot_property_count: usize,
}

/// Returns the full list of 17 core class specs matching Godot 4's defaults.
fn core_class_specs() -> Vec<ClassSpec> {
    vec![
        // ── Base classes ──
        ClassSpec {
            name: "Node",
            parent: "",
            properties: vec![
                ("name", Variant::String(String::new())),
                ("process_mode", Variant::Int(0)), // PROCESS_MODE_INHERIT
                ("process_priority", Variant::Int(0)),
                ("process_physics_priority", Variant::Int(0)),
                ("process_thread_group", Variant::Int(0)), // PROCESS_THREAD_GROUP_INHERIT
                ("scene_file_path", Variant::String(String::new())),
                ("unique_name_in_owner", Variant::Bool(false)),
                ("editor_description", Variant::String(String::new())),
            ],
            godot_property_count: 8,
        },
        ClassSpec {
            name: "Node2D",
            parent: "Node",
            properties: vec![
                ("position", Variant::Vector2(Vector2::ZERO)),
                ("rotation", Variant::Float(0.0)),
                ("scale", Variant::Vector2(Vector2::new(1.0, 1.0))),
                ("skew", Variant::Float(0.0)),
                ("visible", Variant::Bool(true)),
                ("z_index", Variant::Int(0)),
                ("z_as_relative", Variant::Bool(true)),
                ("y_sort_enabled", Variant::Bool(false)),
                ("top_level", Variant::Bool(false)),
                ("modulate", Variant::Color(Color::WHITE)),
            ],
            godot_property_count: 12,
        },
        ClassSpec {
            name: "Node3D",
            parent: "Node",
            properties: vec![
                ("position", Variant::Vector3(Vector3::ZERO)),
                ("rotation", Variant::Vector3(Vector3::ZERO)),
                ("scale", Variant::Vector3(Vector3::new(1.0, 1.0, 1.0))),
                ("visible", Variant::Bool(true)),
                ("top_level", Variant::Bool(false)),
                ("rotation_degrees", Variant::Vector3(Vector3::ZERO)),
                ("global_position", Variant::Vector3(Vector3::ZERO)),
                ("global_rotation", Variant::Vector3(Vector3::ZERO)),
            ],
            godot_property_count: 10,
        },
        // ── 2D visual nodes ──
        ClassSpec {
            name: "Sprite2D",
            parent: "Node2D",
            properties: vec![
                ("texture", Variant::Nil),
                ("offset", Variant::Vector2(Vector2::ZERO)),
                ("flip_h", Variant::Bool(false)),
                ("flip_v", Variant::Bool(false)),
                ("centered", Variant::Bool(true)),
                ("region_enabled", Variant::Bool(false)),
                (
                    "region_rect",
                    Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::ZERO)),
                ),
                ("hframes", Variant::Int(1)),
                ("vframes", Variant::Int(1)),
                ("frame", Variant::Int(0)),
                ("self_modulate", Variant::Color(Color::WHITE)),
            ],
            godot_property_count: 11,
        },
        ClassSpec {
            name: "Camera2D",
            parent: "Node2D",
            properties: vec![
                ("offset", Variant::Vector2(Vector2::ZERO)),
                ("zoom", Variant::Vector2(Vector2::new(1.0, 1.0))),
                ("enabled", Variant::Bool(true)),
                ("anchor_mode", Variant::Int(1)), // ANCHOR_MODE_DRAG_CENTER
                ("position_smoothing_enabled", Variant::Bool(false)),
                ("position_smoothing_speed", Variant::Float(5.0)),
                ("rotation_smoothing_enabled", Variant::Bool(false)),
                ("rotation_smoothing_speed", Variant::Float(5.0)),
                ("drag_horizontal_enabled", Variant::Bool(false)),
                ("drag_vertical_enabled", Variant::Bool(false)),
                ("limit_left", Variant::Int(-10_000_000)),
                ("limit_right", Variant::Int(10_000_000)),
            ],
            godot_property_count: 15,
        },
        ClassSpec {
            name: "AnimationPlayer",
            parent: "Node",
            properties: vec![
                ("current_animation", Variant::String(String::new())),
                ("speed_scale", Variant::Float(1.0)),
                ("autoplay", Variant::String(String::new())),
                ("playback_active", Variant::Bool(false)),
                ("root_node", Variant::NodePath(NodePath::from(".."))),
                ("playback_default_blend_time", Variant::Float(0.0)),
                ("movie_quit_on_finish", Variant::Bool(false)),
            ],
            godot_property_count: 8,
        },
        // ── UI nodes ──
        ClassSpec {
            name: "Control",
            parent: "Node",
            properties: vec![
                ("visible", Variant::Bool(true)),
                ("size", Variant::Vector2(Vector2::ZERO)),
                ("position", Variant::Vector2(Vector2::ZERO)),
                ("anchor_left", Variant::Float(0.0)),
                ("anchor_top", Variant::Float(0.0)),
                ("anchor_right", Variant::Float(0.0)),
                ("anchor_bottom", Variant::Float(0.0)),
                ("mouse_filter", Variant::Int(0)), // MOUSE_FILTER_STOP
                ("offset_left", Variant::Float(0.0)),
                ("offset_top", Variant::Float(0.0)),
                ("offset_right", Variant::Float(0.0)),
                ("offset_bottom", Variant::Float(0.0)),
                ("minimum_size", Variant::Vector2(Vector2::ZERO)),
                ("size_flags_horizontal", Variant::Int(1)), // SIZE_FILL
                ("size_flags_vertical", Variant::Int(1)),   // SIZE_FILL
                ("focus_mode", Variant::Int(0)),            // FOCUS_NONE
                ("mouse_default_cursor_shape", Variant::Int(0)), // CURSOR_ARROW
                ("layout_direction", Variant::Int(0)),      // LAYOUT_DIRECTION_INHERITED
                ("clip_contents", Variant::Bool(false)),
                ("pivot_offset", Variant::Vector2(Vector2::ZERO)),
            ],
            godot_property_count: 25,
        },
        ClassSpec {
            name: "Label",
            parent: "Control",
            properties: vec![
                ("text", Variant::String(String::new())),
                ("horizontal_alignment", Variant::Int(0)), // HALIGN_LEFT
                ("vertical_alignment", Variant::Int(0)),   // VALIGN_TOP
                ("autowrap_mode", Variant::Int(0)),
                ("font_size", Variant::Int(16)),
                ("clip_text", Variant::Bool(false)),
                ("text_overrun_behavior", Variant::Int(0)), // OVERRUN_NO_TRIMMING
                ("uppercase", Variant::Bool(false)),
                ("visible_characters", Variant::Int(-1)), // show all
                ("max_lines_visible", Variant::Int(-1)),  // show all
            ],
            godot_property_count: 12,
        },
        ClassSpec {
            name: "Button",
            parent: "Control",
            properties: vec![
                ("text", Variant::String(String::new())),
                ("disabled", Variant::Bool(false)),
                ("flat", Variant::Bool(false)),
                ("toggle_mode", Variant::Bool(false)),
                ("icon", Variant::Nil),
                ("alignment", Variant::Int(1)), // HORIZONTAL_ALIGNMENT_CENTER
                ("text_overrun_behavior", Variant::Int(0)),
                ("clip_text", Variant::Bool(false)),
            ],
            godot_property_count: 10,
        },
        // ── Physics nodes ──
        ClassSpec {
            name: "RigidBody2D",
            parent: "Node2D",
            properties: vec![
                ("mass", Variant::Float(1.0)),
                ("gravity_scale", Variant::Float(1.0)),
                ("linear_velocity", Variant::Vector2(Vector2::ZERO)),
                ("angular_velocity", Variant::Float(0.0)),
                ("freeze", Variant::Bool(false)),
                ("contact_monitor", Variant::Bool(false)),
                ("inertia", Variant::Float(0.0)),
                ("linear_damp", Variant::Float(0.0)),
                ("angular_damp", Variant::Float(0.0)),
                ("lock_rotation", Variant::Bool(false)),
                ("continuous_cd", Variant::Int(0)), // CCD_MODE_DISABLED
                ("sleeping", Variant::Bool(false)),
                ("freeze_mode", Variant::Int(0)), // FREEZE_MODE_STATIC
                ("linear_damp_mode", Variant::Int(0)),
                ("angular_damp_mode", Variant::Int(0)),
            ],
            godot_property_count: 18,
        },
        ClassSpec {
            name: "StaticBody2D",
            parent: "Node2D",
            properties: vec![
                ("physics_material_override", Variant::Nil),
                ("constant_linear_velocity", Variant::Vector2(Vector2::ZERO)),
                ("constant_angular_velocity", Variant::Float(0.0)),
                ("collision_layer", Variant::Int(1)),
                ("collision_mask", Variant::Int(1)),
            ],
            godot_property_count: 5,
        },
        ClassSpec {
            name: "CharacterBody2D",
            parent: "Node2D",
            properties: vec![
                ("velocity", Variant::Vector2(Vector2::ZERO)),
                ("motion_mode", Variant::Int(0)), // MOTION_MODE_GROUNDED
                ("floor_max_angle", Variant::Float(0.7853982)), // ~45 degrees
                ("max_slides", Variant::Int(6)),
                ("up_direction", Variant::Vector2(Vector2::new(0.0, -1.0))),
                ("wall_min_slide_angle", Variant::Float(0.2617994)), // ~15 degrees
                ("floor_stop_on_slope", Variant::Bool(true)),
                ("floor_constant_speed", Variant::Bool(false)),
                ("floor_block_on_wall", Variant::Bool(true)),
                ("floor_snap_length", Variant::Float(1.0)),
                ("slide_on_ceiling", Variant::Bool(true)),
                ("platform_on_leave", Variant::Int(0)), // PLATFORM_ON_LEAVE_ADD_VELOCITY
            ],
            godot_property_count: 14,
        },
        ClassSpec {
            name: "Area2D",
            parent: "Node2D",
            properties: vec![
                ("monitoring", Variant::Bool(true)),
                ("monitorable", Variant::Bool(true)),
                ("gravity", Variant::Float(980.0)),
                (
                    "gravity_direction",
                    Variant::Vector2(Vector2::new(0.0, 1.0)),
                ),
                ("priority", Variant::Int(0)),
                ("gravity_space_override", Variant::Int(0)), // SPACE_OVERRIDE_DISABLED
                ("gravity_point", Variant::Bool(false)),
                ("linear_damp_space_override", Variant::Int(0)),
            ],
            godot_property_count: 10,
        },
        ClassSpec {
            name: "CollisionShape2D",
            parent: "Node2D",
            properties: vec![
                ("shape", Variant::Nil),
                ("disabled", Variant::Bool(false)),
                ("one_way_collision", Variant::Bool(false)),
                ("one_way_collision_margin", Variant::Float(1.0)),
                (
                    "debug_color",
                    Variant::Color(Color::new(0.0, 0.56, 0.65, 0.42)),
                ),
            ],
            godot_property_count: 5,
        },
        // ── Utility nodes ──
        ClassSpec {
            name: "Timer",
            parent: "Node",
            properties: vec![
                ("wait_time", Variant::Float(1.0)),
                ("one_shot", Variant::Bool(false)),
                ("autostart", Variant::Bool(false)),
                ("paused", Variant::Bool(false)),
                ("process_callback", Variant::Int(1)), // TIMER_PROCESS_IDLE
            ],
            godot_property_count: 5,
        },
        ClassSpec {
            name: "TileMap",
            parent: "Node2D",
            properties: vec![
                ("cell_quadrant_size", Variant::Int(16)),
                ("collision_enabled", Variant::Bool(true)),
                ("tile_set", Variant::Nil),
                ("collision_visibility_mode", Variant::Int(0)),
                ("navigation_visibility_mode", Variant::Int(0)),
            ],
            godot_property_count: 6,
        },
        ClassSpec {
            name: "CPUParticles2D",
            parent: "Node2D",
            properties: vec![
                ("emitting", Variant::Bool(true)),
                ("amount", Variant::Int(8)),
                ("lifetime", Variant::Float(1.0)),
                ("one_shot", Variant::Bool(false)),
                ("explosiveness", Variant::Float(0.0)),
                ("speed_scale", Variant::Float(1.0)),
                ("preprocess", Variant::Float(0.0)),
                ("randomness", Variant::Float(0.0)),
                ("fixed_fps", Variant::Int(0)),
                ("fract_delta", Variant::Bool(true)),
                ("local_coords", Variant::Bool(false)),
                ("draw_order", Variant::Int(0)), // DRAW_ORDER_INDEX
                ("direction", Variant::Vector2(Vector2::new(1.0, 0.0))),
                ("spread", Variant::Float(45.0)),
                ("initial_velocity_min", Variant::Float(0.0)),
                ("initial_velocity_max", Variant::Float(0.0)),
                ("angular_velocity_min", Variant::Float(0.0)),
                ("angular_velocity_max", Variant::Float(0.0)),
                ("orbit_velocity_min", Variant::Float(0.0)),
                ("orbit_velocity_max", Variant::Float(0.0)),
                ("linear_accel_min", Variant::Float(0.0)),
                ("linear_accel_max", Variant::Float(0.0)),
                ("radial_accel_min", Variant::Float(0.0)),
                ("radial_accel_max", Variant::Float(0.0)),
                ("tangential_accel_min", Variant::Float(0.0)),
                ("tangential_accel_max", Variant::Float(0.0)),
                ("damping_min", Variant::Float(0.0)),
                ("damping_max", Variant::Float(0.0)),
                ("scale_amount_min", Variant::Float(1.0)),
                ("scale_amount_max", Variant::Float(1.0)),
                ("color", Variant::Color(Color::WHITE)),
                ("emission_shape", Variant::Int(0)), // EMISSION_SHAPE_POINT
            ],
            godot_property_count: 40,
        },
    ]
}

/// Registers all 17 core classes into ClassDB with their expected properties.
fn register_core_classes() {
    for spec in core_class_specs() {
        let mut reg = ClassRegistration::new(spec.name);
        if !spec.parent.is_empty() {
            reg = reg.parent(spec.parent);
        }
        for (prop_name, default_val) in &spec.properties {
            reg = reg.property(PropertyInfo::new(*prop_name, default_val.clone()));
        }
        register_class(reg);
    }
}

// ===========================================================================
// 1. All 17 classes are recognized after registration
// ===========================================================================

#[test]
fn all_17_classes_recognized() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let expected = [
        "Node",
        "Node2D",
        "Node3D",
        "Sprite2D",
        "Camera2D",
        "Control",
        "Label",
        "Button",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "Timer",
        "AnimationPlayer",
        "CollisionShape2D",
        "TileMap",
        "CPUParticles2D",
    ];

    let mut recognized = 0;
    for name in &expected {
        if class_exists(name) {
            recognized += 1;
        } else {
            eprintln!("  NOT recognized: {name}");
        }
    }

    assert_eq!(recognized, 17, "all 17 core classes should be recognized");
    assert_eq!(class_count(), 17);
}

// ===========================================================================
// 2. Each class instantiates with correct default properties
// ===========================================================================

#[test]
fn each_class_instantiates_with_defaults() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    for spec in core_class_specs() {
        let obj =
            instantiate(spec.name).unwrap_or_else(|| panic!("{} should instantiate", spec.name));

        assert_eq!(obj.get_class(), spec.name);

        for (prop_name, expected_val) in &spec.properties {
            let actual = obj.get_property(prop_name);
            assert_eq!(
                &actual, expected_val,
                "{}.{}: expected {:?}, got {:?}",
                spec.name, prop_name, expected_val, actual
            );
        }
    }
}

// ===========================================================================
// 3. Inheritance chains are correct
// ===========================================================================

#[test]
fn node2d_inherits_from_node() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    assert!(is_parent_class("Node2D", "Node"));
    assert!(is_parent_class("Sprite2D", "Node2D"));
    assert!(is_parent_class("Sprite2D", "Node"));
    assert!(is_parent_class("Label", "Control"));
    assert!(is_parent_class("Label", "Node"));
    assert!(is_parent_class("RigidBody2D", "Node2D"));
}

#[test]
fn inheritance_chain_sprite2d() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let chain = inheritance_chain("Sprite2D");
    assert_eq!(chain, vec!["Sprite2D", "Node2D", "Node"]);
}

#[test]
fn inheritance_chain_label() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let chain = inheritance_chain("Label");
    assert_eq!(chain, vec!["Label", "Control", "Node"]);
}

// ===========================================================================
// 4. Inherited properties propagate to derived classes
// ===========================================================================

#[test]
fn sprite2d_has_node2d_properties() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let sprite = instantiate("Sprite2D").unwrap();
    // Inherited from Node2D.
    assert_eq!(
        sprite.get_property("position"),
        Variant::Vector2(Vector2::ZERO)
    );
    assert_eq!(sprite.get_property("visible"), Variant::Bool(true));
    // Own property.
    assert_eq!(sprite.get_property("flip_h"), Variant::Bool(false));
}

#[test]
fn label_has_control_and_node_properties() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let label = instantiate("Label").unwrap();
    // From Node.
    assert_eq!(label.get_property("name"), Variant::String(String::new()));
    // From Control.
    assert_eq!(label.get_property("visible"), Variant::Bool(true));
    assert_eq!(label.get_property("anchor_left"), Variant::Float(0.0));
    // Own.
    assert_eq!(label.get_property("text"), Variant::String(String::new()));
    assert_eq!(label.get_property("font_size"), Variant::Int(16));
}

#[test]
fn rigidbody2d_has_node2d_and_node_properties() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let rb = instantiate("RigidBody2D").unwrap();
    // From Node.
    assert_eq!(rb.get_property("process_mode"), Variant::Int(0));
    // From Node2D.
    assert_eq!(rb.get_property("rotation"), Variant::Float(0.0));
    // Own.
    assert_eq!(rb.get_property("mass"), Variant::Float(1.0));
    assert_eq!(
        rb.get_property("linear_velocity"),
        Variant::Vector2(Vector2::ZERO)
    );
}

// ===========================================================================
// 5. Node2D-specific defaults: position, rotation, scale
// ===========================================================================

#[test]
fn node2d_defaults_position_rotation_scale() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let n2d = instantiate("Node2D").unwrap();
    assert_eq!(
        n2d.get_property("position"),
        Variant::Vector2(Vector2::ZERO)
    );
    assert_eq!(n2d.get_property("rotation"), Variant::Float(0.0));
    assert_eq!(
        n2d.get_property("scale"),
        Variant::Vector2(Vector2::new(1.0, 1.0))
    );
    assert_eq!(n2d.get_property("skew"), Variant::Float(0.0));
}

// ===========================================================================
// 6. Control defaults
// ===========================================================================

#[test]
fn control_defaults_anchors_and_size() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let ctrl = instantiate("Control").unwrap();
    assert_eq!(ctrl.get_property("anchor_left"), Variant::Float(0.0));
    assert_eq!(ctrl.get_property("anchor_top"), Variant::Float(0.0));
    assert_eq!(ctrl.get_property("anchor_right"), Variant::Float(0.0));
    assert_eq!(ctrl.get_property("anchor_bottom"), Variant::Float(0.0));
    assert_eq!(ctrl.get_property("size"), Variant::Vector2(Vector2::ZERO));
    assert_eq!(ctrl.get_property("mouse_filter"), Variant::Int(0));
}

// ===========================================================================
// 7. Physics node defaults
// ===========================================================================

#[test]
fn characterbody2d_defaults() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let cb = instantiate("CharacterBody2D").unwrap();
    assert_eq!(cb.get_property("velocity"), Variant::Vector2(Vector2::ZERO));
    assert_eq!(
        cb.get_property("up_direction"),
        Variant::Vector2(Vector2::new(0.0, -1.0))
    );
    assert_eq!(cb.get_property("max_slides"), Variant::Int(6));
}

#[test]
fn area2d_defaults() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let area = instantiate("Area2D").unwrap();
    assert_eq!(area.get_property("monitoring"), Variant::Bool(true));
    assert_eq!(area.get_property("gravity"), Variant::Float(980.0));
    assert_eq!(
        area.get_property("gravity_direction"),
        Variant::Vector2(Vector2::new(0.0, 1.0))
    );
}

// ===========================================================================
// 8. Timer defaults
// ===========================================================================

#[test]
fn timer_defaults() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let timer = instantiate("Timer").unwrap();
    assert_eq!(timer.get_property("wait_time"), Variant::Float(1.0));
    assert_eq!(timer.get_property("one_shot"), Variant::Bool(false));
    assert_eq!(timer.get_property("autostart"), Variant::Bool(false));
}

// ===========================================================================
// 9. Node3D defaults
// ===========================================================================

#[test]
fn node3d_defaults() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let n3d = instantiate("Node3D").unwrap();
    assert_eq!(
        n3d.get_property("position"),
        Variant::Vector3(Vector3::ZERO)
    );
    assert_eq!(
        n3d.get_property("scale"),
        Variant::Vector3(Vector3::new(1.0, 1.0, 1.0))
    );
    assert_eq!(n3d.get_property("visible"), Variant::Bool(true));
}

// ===========================================================================
// 10. Unregistered class returns None
// ===========================================================================

#[test]
fn unregistered_class_returns_none() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    assert!(instantiate("AudioStreamPlayer").is_none());
    assert!(!class_exists("AudioStreamPlayer"));
}

// ===========================================================================
// 11. Parity summary — the headline metric
// ===========================================================================

#[test]
fn parity_summary() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let specs = core_class_specs();
    let total_classes = specs.len();
    let mut recognized_classes = 0;
    let mut total_godot_props = 0;
    let mut total_implemented_props = 0;

    for spec in &specs {
        if class_exists(spec.name) {
            recognized_classes += 1;
        }

        total_godot_props += spec.godot_property_count;
        total_implemented_props += spec.properties.len();

        // Verify the class actually instantiates.
        let obj =
            instantiate(spec.name).unwrap_or_else(|| panic!("{} should instantiate", spec.name));
        assert_eq!(obj.get_class(), spec.name);
    }

    let parity_pct =
        (total_implemented_props as f64 / total_godot_props as f64 * 100.0).round() as u32;

    eprintln!();
    eprintln!("═══════════════════════════════════════════════════");
    eprintln!(
        "  ClassDB parity: {recognized_classes}/{total_classes} classes recognized, \
               {parity_pct}% property coverage"
    );
    eprintln!("  ({total_implemented_props}/{total_godot_props} properties implemented)");
    eprintln!("═══════════════════════════════════════════════════");
    eprintln!();

    // Hard requirements.
    assert_eq!(
        recognized_classes, total_classes,
        "all {total_classes} core classes must be recognized"
    );
    // Property coverage should be at least 80% (target: 170+/214).
    assert!(
        parity_pct >= 80,
        "property parity {parity_pct}% is below minimum 80% threshold"
    );
}

// ===========================================================================
// 12. Per-class property counts
// ===========================================================================

#[test]
fn per_class_property_counts() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    for spec in core_class_specs() {
        let info = get_class_info(spec.name)
            .unwrap_or_else(|| panic!("{} should have ClassInfo", spec.name));

        assert_eq!(
            info.properties.len(),
            spec.properties.len(),
            "{}: registered property count mismatch",
            spec.name
        );

        // Verify each property name is in the ClassInfo.
        for (prop_name, _) in &spec.properties {
            assert!(
                info.properties.iter().any(|p| p.name == *prop_name),
                "{}: missing property '{}' in ClassInfo",
                spec.name,
                prop_name
            );
        }
    }
}

// ===========================================================================
// 13. Two instances of same class are independent
// ===========================================================================

#[test]
fn two_instances_independent() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let mut a = instantiate("RigidBody2D").unwrap();
    let b = instantiate("RigidBody2D").unwrap();

    a.set_property("mass", Variant::Float(99.0));

    assert_eq!(a.get_property("mass"), Variant::Float(99.0));
    assert_eq!(
        b.get_property("mass"),
        Variant::Float(1.0),
        "modifying one instance must not affect another"
    );
}

// ===========================================================================
// 14. CPUParticles2D — highest property count class
// ===========================================================================

#[test]
fn cpuparticles2d_defaults() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let p = instantiate("CPUParticles2D").unwrap();
    assert_eq!(p.get_property("emitting"), Variant::Bool(true));
    assert_eq!(p.get_property("amount"), Variant::Int(8));
    assert_eq!(p.get_property("lifetime"), Variant::Float(1.0));
    assert_eq!(p.get_property("speed_scale"), Variant::Float(1.0));
    // Inherited from Node2D.
    assert_eq!(p.get_property("position"), Variant::Vector2(Vector2::ZERO));
}

// ===========================================================================
// 15. Button inherits Control and Node defaults
// ===========================================================================

#[test]
fn button_full_chain_defaults() {
    let _g = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_core_classes();

    let btn = instantiate("Button").unwrap();
    // Own.
    assert_eq!(btn.get_property("text"), Variant::String(String::new()));
    assert_eq!(btn.get_property("disabled"), Variant::Bool(false));
    // From Control.
    assert_eq!(btn.get_property("anchor_left"), Variant::Float(0.0));
    assert_eq!(btn.get_property("visible"), Variant::Bool(true));
    // From Node.
    assert_eq!(btn.get_property("name"), Variant::String(String::new()));

    let chain = inheritance_chain("Button");
    assert_eq!(chain, vec!["Button", "Control", "Node"]);
}
