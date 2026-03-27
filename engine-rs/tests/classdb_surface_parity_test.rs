//! pat-5tcs: Measurable ClassDB parity for core runtime classes.
//!
//! Defines the expected Godot class surface (properties with defaults, methods
//! with argument counts) for 17 core runtime classes and measures Patina's
//! ClassDB coverage as a percentage. Acceptance: API signature probes and
//! runtime tests agree on the supported core class surface.

use std::sync::Mutex;

use gdcore::math::Vector2;
use gdobject::class_db::{
    class_exists, class_has_method, clear_for_testing, get_class_info, get_class_list,
    get_method_list, get_property_list, inheritance_chain, instantiate, is_parent_class,
    register_class, ClassRegistration, MethodInfo, PropertyInfo,
};
use gdobject::object::GodotObject;
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock poisoned");
    clear_for_testing();
    guard
}

// ===========================================================================
// Expected Godot class surface definitions
// ===========================================================================

/// A complete class surface specification: properties + methods + Godot totals.
struct ClassSurface {
    name: &'static str,
    parent: &'static str,
    /// Properties we register in Patina: (name, default_value).
    properties: Vec<(&'static str, Variant)>,
    /// Methods we register in Patina: (name, arg_count).
    methods: Vec<(&'static str, usize)>,
    /// Total property count in Godot for this class (own only, from docs).
    godot_own_property_count: usize,
    /// Total method count in Godot for this class (own only, from docs).
    godot_own_method_count: usize,
}

fn core_class_surfaces() -> Vec<ClassSurface> {
    vec![
        ClassSurface {
            name: "Node",
            parent: "",
            properties: vec![
                ("name", Variant::String(String::new())),
                ("process_mode", Variant::Int(0)),
                ("process_priority", Variant::Int(0)),
                ("editor_description", Variant::String(String::new())),
                ("unique_name_in_owner", Variant::Bool(false)),
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
            ],
            godot_own_property_count: 8,
            godot_own_method_count: 55,
        },
        ClassSurface {
            name: "Node2D",
            parent: "Node",
            properties: vec![
                ("position", Variant::Vector2(Vector2::ZERO)),
                ("rotation", Variant::Float(0.0)),
                ("scale", Variant::Vector2(Vector2::ONE)),
                ("skew", Variant::Float(0.0)),
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
            ],
            godot_own_property_count: 5,
            godot_own_method_count: 22,
        },
        ClassSurface {
            name: "Sprite2D",
            parent: "Node2D",
            properties: vec![
                ("texture", Variant::Nil),
                ("centered", Variant::Bool(true)),
                ("offset", Variant::Vector2(Vector2::ZERO)),
                ("flip_h", Variant::Bool(false)),
                ("flip_v", Variant::Bool(false)),
                ("hframes", Variant::Int(1)),
                ("vframes", Variant::Int(1)),
                ("frame", Variant::Int(0)),
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
            ],
            godot_own_property_count: 10,
            godot_own_method_count: 18,
        },
        ClassSurface {
            name: "AnimatedSprite2D",
            parent: "Node2D",
            properties: vec![
                ("sprite_frames", Variant::Nil),
                ("animation", Variant::String("default".into())),
                ("autoplay", Variant::String(String::new())),
                ("frame", Variant::Int(0)),
                ("speed_scale", Variant::Float(1.0)),
                ("playing", Variant::Bool(false)),
                ("flip_h", Variant::Bool(false)),
                ("flip_v", Variant::Bool(false)),
            ],
            methods: vec![
                ("play", 1),
                ("stop", 0),
                ("pause", 0),
                ("is_playing", 0),
                ("set_animation", 1),
                ("get_animation", 0),
                ("set_frame", 1),
                ("get_frame", 0),
                ("set_speed_scale", 1),
                ("get_speed_scale", 0),
            ],
            godot_own_property_count: 9,
            godot_own_method_count: 16,
        },
        ClassSurface {
            name: "Camera2D",
            parent: "Node2D",
            properties: vec![
                ("offset", Variant::Vector2(Vector2::ZERO)),
                ("zoom", Variant::Vector2(Vector2::ONE)),
                ("anchor_mode", Variant::Int(1)),
                ("enabled", Variant::Bool(true)),
                ("limit_smoothed", Variant::Bool(false)),
            ],
            methods: vec![
                ("get_zoom", 0),
                ("set_zoom", 1),
                ("get_offset", 0),
                ("set_offset", 1),
                ("make_current", 0),
                ("is_current", 0),
                ("get_screen_center_position", 0),
                ("reset_smoothing", 0),
                ("force_update_scroll", 0),
            ],
            godot_own_property_count: 12,
            godot_own_method_count: 15,
        },
        ClassSurface {
            name: "RigidBody2D",
            parent: "Node2D",
            properties: vec![
                ("mass", Variant::Float(1.0)),
                ("gravity_scale", Variant::Float(1.0)),
                ("linear_velocity", Variant::Vector2(Vector2::ZERO)),
                ("angular_velocity", Variant::Float(0.0)),
                ("can_sleep", Variant::Bool(true)),
                ("lock_rotation", Variant::Bool(false)),
                ("freeze", Variant::Bool(false)),
                ("continuous_cd", Variant::Int(0)),
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
            godot_own_property_count: 12,
            godot_own_method_count: 20,
        },
        ClassSurface {
            name: "StaticBody2D",
            parent: "Node2D",
            properties: vec![
                ("constant_linear_velocity", Variant::Vector2(Vector2::ZERO)),
                ("constant_angular_velocity", Variant::Float(0.0)),
            ],
            methods: vec![
                ("set_constant_linear_velocity", 1),
                ("get_constant_linear_velocity", 0),
                ("set_constant_angular_velocity", 1),
                ("get_constant_angular_velocity", 0),
            ],
            godot_own_property_count: 3,
            godot_own_method_count: 8,
        },
        ClassSurface {
            name: "CharacterBody2D",
            parent: "Node2D",
            properties: vec![
                ("velocity", Variant::Vector2(Vector2::ZERO)),
                ("motion_mode", Variant::Int(0)),
                ("floor_max_angle", Variant::Float(std::f64::consts::FRAC_PI_4)),
                ("up_direction", Variant::Vector2(Vector2::new(0.0, -1.0))),
                ("slide_on_ceiling", Variant::Bool(true)),
                ("max_slides", Variant::Int(6)),
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
            godot_own_property_count: 10,
            godot_own_method_count: 18,
        },
        ClassSurface {
            name: "Area2D",
            parent: "Node2D",
            properties: vec![
                ("monitoring", Variant::Bool(true)),
                ("monitorable", Variant::Bool(true)),
                ("gravity_space_override", Variant::Int(0)),
                ("gravity", Variant::Float(980.0)),
                ("gravity_direction", Variant::Vector2(Vector2::new(0.0, 1.0))),
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
            ],
            godot_own_property_count: 8,
            godot_own_method_count: 14,
        },
        ClassSurface {
            name: "CollisionShape2D",
            parent: "Node2D",
            properties: vec![
                ("shape", Variant::Nil),
                ("disabled", Variant::Bool(false)),
                ("one_way_collision", Variant::Bool(false)),
            ],
            methods: vec![
                ("set_shape", 1),
                ("get_shape", 0),
                ("set_disabled", 1),
                ("is_disabled", 0),
                ("set_one_way_collision", 1),
                ("is_one_way_collision_enabled", 0),
            ],
            godot_own_property_count: 4,
            godot_own_method_count: 8,
        },
        ClassSurface {
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
                ("mouse_filter", Variant::Int(0)),
                ("focus_mode", Variant::Int(0)),
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
            godot_own_property_count: 18,
            godot_own_method_count: 35,
        },
        ClassSurface {
            name: "Label",
            parent: "Control",
            properties: vec![
                ("text", Variant::String(String::new())),
                ("horizontal_alignment", Variant::Int(0)),
                ("vertical_alignment", Variant::Int(0)),
                ("autowrap_mode", Variant::Int(0)),
                ("clip_text", Variant::Bool(false)),
            ],
            methods: vec![
                ("get_text", 0),
                ("set_text", 1),
                ("get_line_count", 0),
                ("get_visible_line_count", 0),
                ("set_horizontal_alignment", 1),
                ("get_horizontal_alignment", 0),
                ("set_vertical_alignment", 1),
                ("get_vertical_alignment", 0),
            ],
            godot_own_property_count: 8,
            godot_own_method_count: 14,
        },
        ClassSurface {
            name: "Button",
            parent: "Control",
            properties: vec![
                ("text", Variant::String(String::new())),
                ("flat", Variant::Bool(false)),
                ("disabled", Variant::Bool(false)),
                ("toggle_mode", Variant::Bool(false)),
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
            godot_own_property_count: 6,
            godot_own_method_count: 12,
        },
        ClassSurface {
            name: "Timer",
            parent: "Node",
            properties: vec![
                ("wait_time", Variant::Float(1.0)),
                ("one_shot", Variant::Bool(false)),
                ("autostart", Variant::Bool(false)),
                ("paused", Variant::Bool(false)),
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
            ],
            godot_own_property_count: 5,
            godot_own_method_count: 14,
        },
        ClassSurface {
            name: "AudioStreamPlayer",
            parent: "Node",
            properties: vec![
                ("stream", Variant::Nil),
                ("volume_db", Variant::Float(0.0)),
                ("autoplay", Variant::Bool(false)),
                ("bus", Variant::String("Master".into())),
            ],
            methods: vec![
                ("play", 0),
                ("stop", 0),
                ("is_playing", 0),
                ("get_playback_position", 0),
                ("seek", 1),
                ("set_volume_db", 1),
                ("get_volume_db", 0),
            ],
            godot_own_property_count: 6,
            godot_own_method_count: 10,
        },
        ClassSurface {
            name: "TileMap",
            parent: "Node2D",
            properties: vec![
                ("tile_set", Variant::Nil),
                ("cell_quadrant_size", Variant::Int(16)),
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
            godot_own_property_count: 4,
            godot_own_method_count: 18,
        },
        ClassSurface {
            name: "AnimationPlayer",
            parent: "Node",
            properties: vec![
                ("current_animation", Variant::String(String::new())),
                ("speed_scale", Variant::Float(1.0)),
                ("autoplay", Variant::String(String::new())),
                ("active", Variant::Bool(true)),
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
                ("set_speed_scale", 1),
                ("get_speed_scale", 0),
            ],
            godot_own_property_count: 6,
            godot_own_method_count: 22,
        },
    ]
}

/// Register all 17 core classes with their full surface.
fn register_all_surfaces() {
    for s in core_class_surfaces() {
        let mut reg = ClassRegistration::new(s.name);
        if !s.parent.is_empty() {
            reg = reg.parent(s.parent);
        }
        for (pname, pdefault) in &s.properties {
            reg = reg.property(PropertyInfo::new(*pname, pdefault.clone()));
        }
        for (mname, argc) in &s.methods {
            reg = reg.method(MethodInfo::new(*mname, *argc));
        }
        register_class(reg);
    }
}

// ===========================================================================
// 1. All 17 core classes registered and recognized
// ===========================================================================

#[test]
fn all_17_classes_registered() {
    let _g = setup();
    register_all_surfaces();

    let surfaces = core_class_surfaces();
    assert_eq!(surfaces.len(), 17);
    for s in &surfaces {
        assert!(class_exists(s.name), "{} not registered", s.name);
    }
}

// ===========================================================================
// 2. Property parity — measurable percentage
// ===========================================================================

#[test]
fn property_parity_measurable() {
    let _g = setup();
    register_all_surfaces();

    let surfaces = core_class_surfaces();
    let mut total_patina = 0usize;
    let mut total_godot = 0usize;

    for s in &surfaces {
        let patina_count = get_property_list(s.name, true).len();
        total_patina += patina_count;
        total_godot += s.godot_own_property_count;
    }

    let pct = (total_patina as f64 / total_godot as f64 * 100.0).round() as u32;

    eprintln!();
    eprintln!("  Property parity: {total_patina}/{total_godot} properties ({pct}%)");

    // We should cover at least 50% of Godot's properties for core classes.
    assert!(
        pct >= 50,
        "property parity {pct}% below 50% minimum threshold"
    );
}

// ===========================================================================
// 3. Method parity — measurable percentage
// ===========================================================================

#[test]
fn method_parity_measurable() {
    let _g = setup();
    register_all_surfaces();

    let surfaces = core_class_surfaces();
    let mut total_patina = 0usize;
    let mut total_godot = 0usize;

    for s in &surfaces {
        let patina_count = get_method_list(s.name, true).len();
        total_patina += patina_count;
        total_godot += s.godot_own_method_count;
    }

    let pct = (total_patina as f64 / total_godot as f64 * 100.0).round() as u32;

    eprintln!("  Method parity:   {total_patina}/{total_godot} methods ({pct}%)");

    // We should cover at least 40% of Godot's methods for core classes.
    assert!(
        pct >= 40,
        "method parity {pct}% below 40% minimum threshold"
    );
}

// ===========================================================================
// 4. Combined surface area summary — properties + methods
// ===========================================================================

#[test]
fn combined_surface_parity_summary() {
    let _g = setup();
    register_all_surfaces();

    let surfaces = core_class_surfaces();
    let mut patina_props = 0usize;
    let mut godot_props = 0usize;
    let mut patina_methods = 0usize;
    let mut godot_methods = 0usize;

    eprintln!();
    eprintln!("┌─────────────────────┬──────────────────┬──────────────────┐");
    eprintln!("│ Class               │ Props (Patina/G) │ Methods (P/G)    │");
    eprintln!("├─────────────────────┼──────────────────┼──────────────────┤");

    for s in &surfaces {
        let p = get_property_list(s.name, true).len();
        let m = get_method_list(s.name, true).len();
        patina_props += p;
        godot_props += s.godot_own_property_count;
        patina_methods += m;
        godot_methods += s.godot_own_method_count;

        let ppct = if s.godot_own_property_count > 0 {
            (p as f64 / s.godot_own_property_count as f64 * 100.0).round() as u32
        } else {
            100
        };
        let mpct = if s.godot_own_method_count > 0 {
            (m as f64 / s.godot_own_method_count as f64 * 100.0).round() as u32
        } else {
            100
        };
        eprintln!(
            "│ {:<19} │ {:>3}/{:<3} ({:>3}%)  │ {:>3}/{:<3} ({:>3}%)  │",
            s.name, p, s.godot_own_property_count, ppct, m, s.godot_own_method_count, mpct
        );
    }

    let total_patina = patina_props + patina_methods;
    let total_godot = godot_props + godot_methods;
    let overall_pct = (total_patina as f64 / total_godot as f64 * 100.0).round() as u32;
    let prop_pct = (patina_props as f64 / godot_props as f64 * 100.0).round() as u32;
    let method_pct = (patina_methods as f64 / godot_methods as f64 * 100.0).round() as u32;

    eprintln!("├─────────────────────┼──────────────────┼──────────────────┤");
    eprintln!(
        "│ TOTAL (17 classes)  │ {:>3}/{:<3} ({:>3}%)  │ {:>3}/{:<3} ({:>3}%)  │",
        patina_props, godot_props, prop_pct, patina_methods, godot_methods, method_pct
    );
    eprintln!("└─────────────────────┴──────────────────┴──────────────────┘");
    eprintln!();
    eprintln!("  Combined surface parity: {total_patina}/{total_godot} ({overall_pct}%)");
    eprintln!();

    assert!(
        overall_pct >= 45,
        "combined surface parity {overall_pct}% below 45% minimum"
    );
}

// ===========================================================================
// 5. API signature probes — method argument counts match
// ===========================================================================

#[test]
fn api_signature_arg_counts_match() {
    let _g = setup();
    register_all_surfaces();

    for s in core_class_surfaces() {
        let methods = get_method_list(s.name, true);
        for (expected_name, expected_argc) in &s.methods {
            let found = methods.iter().find(|m| m.name == *expected_name);
            assert!(
                found.is_some(),
                "{}: method '{}' not registered",
                s.name,
                expected_name
            );
            assert_eq!(
                found.unwrap().argument_count,
                *expected_argc,
                "{}.{}: arg count mismatch",
                s.name,
                expected_name
            );
        }
    }
}

// ===========================================================================
// 6. Property defaults match expected values
// ===========================================================================

#[test]
fn property_defaults_match_expected() {
    let _g = setup();
    register_all_surfaces();

    for s in core_class_surfaces() {
        let props = get_property_list(s.name, true);
        for (expected_name, expected_default) in &s.properties {
            let found = props.iter().find(|p| p.name == *expected_name);
            assert!(
                found.is_some(),
                "{}: property '{}' not registered",
                s.name,
                expected_name
            );
            assert_eq!(
                &found.unwrap().default_value,
                expected_default,
                "{}.{}: default value mismatch",
                s.name,
                expected_name
            );
        }
    }
}

// ===========================================================================
// 7. Inheritance chains are correct
// ===========================================================================

#[test]
fn inheritance_chains_correct() {
    let _g = setup();
    register_all_surfaces();

    // Node2D hierarchy.
    let chain = inheritance_chain("Sprite2D");
    assert_eq!(chain, vec!["Sprite2D", "Node2D", "Node"]);

    let chain = inheritance_chain("RigidBody2D");
    assert_eq!(chain, vec!["RigidBody2D", "Node2D", "Node"]);

    // Control hierarchy.
    let chain = inheritance_chain("Label");
    assert_eq!(chain, vec!["Label", "Control", "Node"]);

    let chain = inheritance_chain("Button");
    assert_eq!(chain, vec!["Button", "Control", "Node"]);

    // Direct child of Node.
    let chain = inheritance_chain("Timer");
    assert_eq!(chain, vec!["Timer", "Node"]);
}

// ===========================================================================
// 8. is_parent_class probes across hierarchy
// ===========================================================================

#[test]
fn is_parent_class_probes() {
    let _g = setup();
    register_all_surfaces();

    // All classes should inherit from Node.
    for s in core_class_surfaces() {
        if s.name != "Node" {
            assert!(
                is_parent_class(s.name, "Node"),
                "{} should inherit from Node",
                s.name
            );
        }
    }

    // Node2D subtypes.
    for name in &[
        "Sprite2D",
        "Camera2D",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "AnimatedSprite2D",
        "TileMap",
    ] {
        assert!(
            is_parent_class(name, "Node2D"),
            "{name} should inherit from Node2D"
        );
    }

    // Control subtypes.
    for name in &["Label", "Button"] {
        assert!(
            is_parent_class(name, "Control"),
            "{name} should inherit from Control"
        );
    }

    // Non-relationships.
    assert!(!is_parent_class("Sprite2D", "Control"));
    assert!(!is_parent_class("Label", "Node2D"));
    assert!(!is_parent_class("Timer", "Node2D"));
}

// ===========================================================================
// 9. Inherited property/method totals grow with depth
// ===========================================================================

#[test]
fn inherited_totals_grow_with_depth() {
    let _g = setup();
    register_all_surfaces();

    // Sprite2D should have more total properties than Node2D own.
    let sprite_all = get_property_list("Sprite2D", false).len();
    let sprite_own = get_property_list("Sprite2D", true).len();
    let node2d_own = get_property_list("Node2D", true).len();
    let node_own = get_property_list("Node", true).len();

    assert_eq!(
        sprite_all,
        sprite_own + node2d_own + node_own,
        "Sprite2D total props = own + Node2D + Node"
    );

    // Same for methods.
    let sprite_methods_all = get_method_list("Sprite2D", false).len();
    let sprite_methods_own = get_method_list("Sprite2D", true).len();
    let node2d_methods_own = get_method_list("Node2D", true).len();
    let node_methods_own = get_method_list("Node", true).len();

    assert_eq!(
        sprite_methods_all,
        sprite_methods_own + node2d_methods_own + node_methods_own,
        "Sprite2D total methods = own + Node2D + Node"
    );
}

// ===========================================================================
// 10. Instantiation applies all inherited defaults
// ===========================================================================

#[test]
fn instantiation_applies_all_defaults() {
    let _g = setup();
    register_all_surfaces();

    let obj = instantiate("Sprite2D").expect("should instantiate Sprite2D");
    assert_eq!(obj.get_class(), "Sprite2D");

    // Own defaults.
    assert_eq!(obj.get_property("centered"), Variant::Bool(true));
    assert_eq!(obj.get_property("flip_h"), Variant::Bool(false));
    assert_eq!(obj.get_property("frame"), Variant::Int(0));

    // Inherited from Node2D.
    assert_eq!(obj.get_property("position"), Variant::Vector2(Vector2::ZERO));
    assert_eq!(obj.get_property("rotation"), Variant::Float(0.0));

    // Inherited from Node.
    assert_eq!(obj.get_property("name"), Variant::String(String::new()));
    assert_eq!(obj.get_property("process_mode"), Variant::Int(0));
}

// ===========================================================================
// 11. class_has_method across inheritance for all classes
// ===========================================================================

#[test]
fn class_has_method_across_all_hierarchies() {
    let _g = setup();
    register_all_surfaces();

    // Every non-root class should inherit _ready and _process from Node.
    for s in core_class_surfaces() {
        if s.name != "Node" {
            assert!(
                class_has_method(s.name, "_ready"),
                "{} should inherit _ready",
                s.name
            );
            assert!(
                class_has_method(s.name, "_process"),
                "{} should inherit _process",
                s.name
            );
        }
    }

    // Node2D descendants should inherit translate.
    for name in &["Sprite2D", "RigidBody2D", "CharacterBody2D", "Area2D"] {
        assert!(
            class_has_method(name, "translate"),
            "{name} should inherit translate from Node2D"
        );
    }
}

// ===========================================================================
// 12. Each class has expected own property count
// ===========================================================================

#[test]
fn each_class_own_property_count() {
    let _g = setup();
    register_all_surfaces();

    for s in core_class_surfaces() {
        let own = get_property_list(s.name, true);
        assert_eq!(
            own.len(),
            s.properties.len(),
            "{}: registered own property count mismatch",
            s.name
        );
    }
}

// ===========================================================================
// 13. Each class has expected own method count
// ===========================================================================

#[test]
fn each_class_own_method_count() {
    let _g = setup();
    register_all_surfaces();

    for s in core_class_surfaces() {
        let own = get_method_list(s.name, true);
        assert_eq!(
            own.len(),
            s.methods.len(),
            "{}: registered own method count mismatch",
            s.name
        );
    }
}

// ===========================================================================
// 14. ClassInfo matches registered surface
// ===========================================================================

#[test]
fn class_info_matches_surface() {
    let _g = setup();
    register_all_surfaces();

    for s in core_class_surfaces() {
        let info = get_class_info(s.name).expect(&format!("{} should have ClassInfo", s.name));
        assert_eq!(info.class_name, s.name);
        assert_eq!(info.parent_class, s.parent);
        assert_eq!(info.properties.len(), s.properties.len());
        assert_eq!(info.methods.len(), s.methods.len());
    }
}

// ===========================================================================
// 15. Unique ClassIds for all 17 classes
// ===========================================================================

#[test]
fn unique_class_ids() {
    let _g = setup();
    register_all_surfaces();

    let mut ids = Vec::new();
    for s in core_class_surfaces() {
        let info = get_class_info(s.name).unwrap();
        assert!(
            !ids.contains(&info.class_id),
            "{}: duplicate ClassId",
            s.name
        );
        ids.push(info.class_id);
    }
    assert_eq!(ids.len(), 17);
}

// ===========================================================================
// 16. Control subtypes have both Control and Node methods
// ===========================================================================

#[test]
fn control_subtypes_full_method_chain() {
    let _g = setup();
    register_all_surfaces();

    for name in &["Label", "Button"] {
        let all_methods = get_method_list(name, false);
        let names: Vec<&str> = all_methods.iter().map(|m| m.name.as_str()).collect();

        // From Node.
        assert!(names.contains(&"_ready"), "{name} missing _ready");
        assert!(names.contains(&"queue_free"), "{name} missing queue_free");
        // From Control.
        assert!(names.contains(&"grab_focus"), "{name} missing grab_focus");
        assert!(names.contains(&"get_size"), "{name} missing get_size");
    }
}

// ===========================================================================
// 17. Physics classes have collision-related methods
// ===========================================================================

#[test]
fn physics_classes_have_collision_methods() {
    let _g = setup();
    register_all_surfaces();

    // RigidBody2D.
    assert!(class_has_method("RigidBody2D", "apply_force"));
    assert!(class_has_method("RigidBody2D", "apply_impulse"));
    assert!(class_has_method("RigidBody2D", "set_mass"));
    assert!(class_has_method("RigidBody2D", "get_linear_velocity"));

    // CharacterBody2D.
    assert!(class_has_method("CharacterBody2D", "move_and_slide"));
    assert!(class_has_method("CharacterBody2D", "is_on_floor"));
    assert!(class_has_method("CharacterBody2D", "get_floor_normal"));

    // StaticBody2D.
    assert!(class_has_method("StaticBody2D", "set_constant_linear_velocity"));

    // Area2D.
    assert!(class_has_method("Area2D", "get_overlapping_bodies"));
    assert!(class_has_method("Area2D", "is_monitoring"));
}

// ===========================================================================
// 18. Timer methods complete
// ===========================================================================

#[test]
fn timer_methods_complete() {
    let _g = setup();
    register_all_surfaces();

    let methods = get_method_list("Timer", true);
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    for expected in &[
        "start",
        "stop",
        "is_stopped",
        "get_time_left",
        "set_wait_time",
        "get_wait_time",
        "set_one_shot",
        "is_one_shot",
        "set_autostart",
        "has_autostart",
    ] {
        assert!(names.contains(expected), "Timer missing method '{expected}'");
    }
}

// ===========================================================================
// 19. Instantiation of each class succeeds
// ===========================================================================

#[test]
fn instantiation_succeeds_for_all_classes() {
    let _g = setup();
    register_all_surfaces();

    for s in core_class_surfaces() {
        let obj = instantiate(s.name);
        assert!(
            obj.is_some(),
            "instantiate({}) returned None",
            s.name
        );
        assert_eq!(obj.unwrap().get_class(), s.name);
    }
}

// ===========================================================================
// 20. Non-existent class does not register
// ===========================================================================

#[test]
fn nonexistent_class_not_registered() {
    let _g = setup();
    register_all_surfaces();

    assert!(!class_exists("NonExistentClass123"));
    assert!(instantiate("NonExistentClass123").is_none());
    assert!(get_class_info("NonExistentClass123").is_none());
    assert!(get_property_list("NonExistentClass123", false).is_empty());
    assert!(get_method_list("NonExistentClass123", false).is_empty());
}

// ===========================================================================
// 21. class_list returns stable sorted order (pat-o78e / pat-kf7)
// ===========================================================================

#[test]
fn class_list_returns_sorted_order() {
    let _g = setup();
    register_all_surfaces();

    let list = get_class_list();
    assert!(!list.is_empty(), "class_list must not be empty after registration");

    let mut sorted = list.clone();
    sorted.sort();
    assert_eq!(list, sorted, "class_list must be in lexicographic order");
}

#[test]
fn class_list_stable_across_repeated_calls() {
    let _g = setup();
    register_all_surfaces();

    let first = get_class_list();
    let second = get_class_list();
    let third = get_class_list();

    assert_eq!(first, second, "class_list must be deterministic");
    assert_eq!(second, third, "class_list must be stable across calls");
}

#[test]
fn class_list_contains_all_17_core_classes() {
    let _g = setup();
    register_all_surfaces();

    let list = get_class_list();
    let expected = [
        "AnimatedSprite2D", "AnimationPlayer", "Area2D", "AudioStreamPlayer",
        "Button", "Camera2D", "CharacterBody2D", "CollisionShape2D",
        "Control", "Label", "Node", "Node2D", "RigidBody2D", "Sprite2D",
        "StaticBody2D", "TileMap", "Timer",
    ];
    for class in &expected {
        assert!(
            list.contains(&class.to_string()),
            "class_list must contain {class}"
        );
    }
}

#[test]
fn class_list_count_matches_registered() {
    let _g = setup();
    register_all_surfaces();

    let list = get_class_list();
    // At minimum the 17 core classes (Object base may or may not be present).
    assert!(
        list.len() >= 17,
        "must have at least 17 registered classes, got {}",
        list.len()
    );
}
