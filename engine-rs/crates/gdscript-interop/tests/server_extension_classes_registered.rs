//! pat-s3rrv.231 — the server `*Extension` surfaces resolve as base classes.
//!
//! Godot's `*Extension` classes are the seam for replacing a whole server:
//! `TextServerExtension` backs a custom text shaper, `PhysicsServer2DExtension`
//! and `PhysicsServer3DExtension` a custom physics backend, the two
//! `PhysicsDirectSpaceState*Extension` classes the query interfaces those hand
//! out, and `PhysicsServer3DRenderingServerHandler` the soft-body writeback.
//! **The virtuals are the entire API** — a class registered without them exists
//! in name only, with nothing for a script author or the editor to discover.
//!
//! None of the six existed in ClassDB, so `extends TextServerExtension` failed
//! on an unknown base class. Two of their PARENTS were missing too — `TextServer`
//! and `PhysicsServer3D` — and a child registered against an unregistered base
//! leaves an inheritance chain that stops there, so those are registered as well.
//!
//! This bead is deliberately the registration surface only. Routing Patina's
//! text shaping or physics through a script-provided server is a separate and
//! much larger effort; the contract here is that the class resolves, reports the
//! right parent, and lists its virtuals.
//!
//! ## Where the table comes from
//!
//! Each class's own doc XML — the `<method>` entries whose names begin with
//! `_`. The bead says to generate these from `extension_api.json` with jq; that
//! file is not vendored, so the XML is the oracle. Nothing is derived from a
//! name.

use gdobject::class_db::{class_exists, get_class_info, register_all_classes};
use gdscript_interop::interpreter::Interpreter;

/// `(class, parent, [virtual names])`.
type Surface = (&'static str, &'static str, &'static [&'static str]);

const EXPECTED: &[Surface] = &[
    (
        "TextServerExtension",
        "TextServer",
        &[
            "_cleanup",
            "_create_font",
            "_create_font_linked_variation",
            "_create_shaped_text",
            "_draw_hex_code_box",
            "_font_clear_glyphs",
            "_font_clear_kerning_map",
            "_font_clear_size_cache",
            "_font_clear_system_fallback_cache",
            "_font_clear_textures",
            "_font_draw_glyph",
            "_font_draw_glyph_outline",
            "_font_get_antialiasing",
            "_font_get_ascent",
            "_font_get_baseline_offset",
            "_font_get_char_from_glyph_index",
            "_font_get_descent",
            "_font_get_disable_embedded_bitmaps",
            "_font_get_embolden",
            "_font_get_face_count",
            "_font_get_face_index",
            "_font_get_fixed_size",
            "_font_get_fixed_size_scale_mode",
            "_font_get_generate_mipmaps",
            "_font_get_global_oversampling",
            "_font_get_glyph_advance",
            "_font_get_glyph_contours",
            "_font_get_glyph_index",
            "_font_get_glyph_list",
            "_font_get_glyph_offset",
            "_font_get_glyph_size",
            "_font_get_glyph_texture_idx",
            "_font_get_glyph_texture_rid",
            "_font_get_glyph_texture_size",
            "_font_get_glyph_uv_rect",
            "_font_get_hinting",
            "_font_get_keep_rounding_remainders",
            "_font_get_kerning",
            "_font_get_kerning_list",
            "_font_get_language_support_override",
            "_font_get_language_support_overrides",
            "_font_get_msdf_pixel_range",
            "_font_get_msdf_size",
            "_font_get_name",
            "_font_get_opentype_feature_overrides",
            "_font_get_ot_name_strings",
            "_font_get_oversampling",
            "_font_get_scale",
            "_font_get_script_support_override",
            "_font_get_script_support_overrides",
            "_font_get_size_cache_info",
            "_font_get_size_cache_list",
            "_font_get_spacing",
            "_font_get_stretch",
            "_font_get_style",
            "_font_get_style_name",
            "_font_get_subpixel_positioning",
            "_font_get_supported_chars",
            "_font_get_supported_glyphs",
            "_font_get_texture_count",
            "_font_get_texture_image",
            "_font_get_texture_offsets",
            "_font_get_transform",
            "_font_get_underline_position",
            "_font_get_underline_thickness",
            "_font_get_variation_coordinates",
            "_font_get_weight",
            "_font_has_char",
            "_font_is_allow_system_fallback",
            "_font_is_force_autohinter",
            "_font_is_language_supported",
            "_font_is_modulate_color_glyphs",
            "_font_is_multichannel_signed_distance_field",
            "_font_is_script_supported",
            "_font_remove_glyph",
            "_font_remove_kerning",
            "_font_remove_language_support_override",
            "_font_remove_script_support_override",
            "_font_remove_size_cache",
            "_font_remove_texture",
            "_font_render_glyph",
            "_font_render_range",
            "_font_set_allow_system_fallback",
            "_font_set_antialiasing",
            "_font_set_ascent",
            "_font_set_baseline_offset",
            "_font_set_data",
            "_font_set_data_ptr",
            "_font_set_descent",
            "_font_set_disable_embedded_bitmaps",
            "_font_set_embolden",
            "_font_set_face_index",
            "_font_set_fixed_size",
            "_font_set_fixed_size_scale_mode",
            "_font_set_force_autohinter",
            "_font_set_generate_mipmaps",
            "_font_set_global_oversampling",
            "_font_set_glyph_advance",
            "_font_set_glyph_offset",
            "_font_set_glyph_size",
            "_font_set_glyph_texture_idx",
            "_font_set_glyph_uv_rect",
            "_font_set_hinting",
            "_font_set_keep_rounding_remainders",
            "_font_set_kerning",
            "_font_set_language_support_override",
            "_font_set_modulate_color_glyphs",
            "_font_set_msdf_pixel_range",
            "_font_set_msdf_size",
            "_font_set_multichannel_signed_distance_field",
            "_font_set_name",
            "_font_set_opentype_feature_overrides",
            "_font_set_oversampling",
            "_font_set_scale",
            "_font_set_script_support_override",
            "_font_set_spacing",
            "_font_set_stretch",
            "_font_set_style",
            "_font_set_style_name",
            "_font_set_subpixel_positioning",
            "_font_set_texture_image",
            "_font_set_texture_offsets",
            "_font_set_transform",
            "_font_set_underline_position",
            "_font_set_underline_thickness",
            "_font_set_variation_coordinates",
            "_font_set_weight",
            "_font_supported_feature_list",
            "_font_supported_variation_list",
            "_format_number",
            "_free_rid",
            "_get_features",
            "_get_hex_code_box_size",
            "_get_name",
            "_get_support_data",
            "_get_support_data_filename",
            "_get_support_data_info",
            "_has",
            "_has_feature",
            "_is_confusable",
            "_is_locale_right_to_left",
            "_is_locale_using_support_data",
            "_is_valid_identifier",
            "_is_valid_letter",
            "_load_support_data",
            "_name_to_tag",
            "_parse_number",
            "_parse_structured_text",
            "_percent_sign",
            "_reference_oversampling_level",
            "_save_support_data",
            "_shaped_get_run_count",
            "_shaped_get_run_direction",
            "_shaped_get_run_font_rid",
            "_shaped_get_run_font_size",
            "_shaped_get_run_language",
            "_shaped_get_run_object",
            "_shaped_get_run_range",
            "_shaped_get_run_text",
            "_shaped_get_span_count",
            "_shaped_get_span_embedded_object",
            "_shaped_get_span_meta",
            "_shaped_get_span_object",
            "_shaped_get_span_text",
            "_shaped_get_text",
            "_shaped_set_span_update_font",
            "_shaped_text_add_object",
            "_shaped_text_add_string",
            "_shaped_text_clear",
            "_shaped_text_closest_character_pos",
            "_shaped_text_draw",
            "_shaped_text_draw_outline",
            "_shaped_text_duplicate",
            "_shaped_text_fit_to_width",
            "_shaped_text_get_ascent",
            "_shaped_text_get_carets",
            "_shaped_text_get_character_breaks",
            "_shaped_text_get_custom_ellipsis",
            "_shaped_text_get_custom_punctuation",
            "_shaped_text_get_descent",
            "_shaped_text_get_direction",
            "_shaped_text_get_dominant_direction_in_range",
            "_shaped_text_get_ellipsis_glyph_count",
            "_shaped_text_get_ellipsis_glyphs",
            "_shaped_text_get_ellipsis_pos",
            "_shaped_text_get_glyph_count",
            "_shaped_text_get_glyphs",
            "_shaped_text_get_grapheme_bounds",
            "_shaped_text_get_inferred_direction",
            "_shaped_text_get_line_breaks",
            "_shaped_text_get_line_breaks_adv",
            "_shaped_text_get_object_glyph",
            "_shaped_text_get_object_range",
            "_shaped_text_get_object_rect",
            "_shaped_text_get_objects",
            "_shaped_text_get_orientation",
            "_shaped_text_get_parent",
            "_shaped_text_get_preserve_control",
            "_shaped_text_get_preserve_invalid",
            "_shaped_text_get_range",
            "_shaped_text_get_selection",
            "_shaped_text_get_size",
            "_shaped_text_get_spacing",
            "_shaped_text_get_trim_pos",
            "_shaped_text_get_underline_position",
            "_shaped_text_get_underline_thickness",
            "_shaped_text_get_width",
            "_shaped_text_get_word_breaks",
            "_shaped_text_has_object",
            "_shaped_text_hit_test_grapheme",
            "_shaped_text_hit_test_position",
            "_shaped_text_is_ready",
            "_shaped_text_next_character_pos",
            "_shaped_text_next_grapheme_pos",
            "_shaped_text_overrun_trim_to_width",
            "_shaped_text_prev_character_pos",
            "_shaped_text_prev_grapheme_pos",
            "_shaped_text_resize_object",
            "_shaped_text_set_bidi_override",
            "_shaped_text_set_custom_ellipsis",
            "_shaped_text_set_custom_punctuation",
            "_shaped_text_set_direction",
            "_shaped_text_set_orientation",
            "_shaped_text_set_preserve_control",
            "_shaped_text_set_preserve_invalid",
            "_shaped_text_set_spacing",
            "_shaped_text_shape",
            "_shaped_text_sort_logical",
            "_shaped_text_substr",
            "_shaped_text_tab_align",
            "_shaped_text_update_breaks",
            "_shaped_text_update_justification_ops",
            "_spoof_check",
            "_string_get_character_breaks",
            "_string_get_word_breaks",
            "_string_to_lower",
            "_string_to_title",
            "_string_to_upper",
            "_strip_diacritics",
            "_tag_to_name",
            "_unreference_oversampling_level",
        ],
    ),
    (
        "PhysicsServer2DExtension",
        "PhysicsServer2D",
        &[
            "_area_add_shape",
            "_area_attach_canvas_instance_id",
            "_area_attach_object_instance_id",
            "_area_clear_shapes",
            "_area_create",
            "_area_get_canvas_instance_id",
            "_area_get_collision_layer",
            "_area_get_collision_mask",
            "_area_get_object_instance_id",
            "_area_get_param",
            "_area_get_shape",
            "_area_get_shape_count",
            "_area_get_shape_transform",
            "_area_get_space",
            "_area_get_transform",
            "_area_remove_shape",
            "_area_set_area_monitor_callback",
            "_area_set_collision_layer",
            "_area_set_collision_mask",
            "_area_set_monitor_callback",
            "_area_set_monitorable",
            "_area_set_param",
            "_area_set_pickable",
            "_area_set_shape",
            "_area_set_shape_disabled",
            "_area_set_shape_transform",
            "_area_set_space",
            "_area_set_transform",
            "_body_add_collision_exception",
            "_body_add_constant_central_force",
            "_body_add_constant_force",
            "_body_add_constant_torque",
            "_body_add_shape",
            "_body_apply_central_force",
            "_body_apply_central_impulse",
            "_body_apply_force",
            "_body_apply_impulse",
            "_body_apply_torque",
            "_body_apply_torque_impulse",
            "_body_attach_canvas_instance_id",
            "_body_attach_object_instance_id",
            "_body_clear_shapes",
            "_body_collide_shape",
            "_body_create",
            "_body_get_canvas_instance_id",
            "_body_get_collision_exceptions",
            "_body_get_collision_layer",
            "_body_get_collision_mask",
            "_body_get_collision_priority",
            "_body_get_constant_force",
            "_body_get_constant_torque",
            "_body_get_contacts_reported_depth_threshold",
            "_body_get_continuous_collision_detection_mode",
            "_body_get_direct_state",
            "_body_get_max_contacts_reported",
            "_body_get_mode",
            "_body_get_object_instance_id",
            "_body_get_param",
            "_body_get_shape",
            "_body_get_shape_count",
            "_body_get_shape_transform",
            "_body_get_space",
            "_body_get_state",
            "_body_is_omitting_force_integration",
            "_body_remove_collision_exception",
            "_body_remove_shape",
            "_body_reset_mass_properties",
            "_body_set_axis_velocity",
            "_body_set_collision_layer",
            "_body_set_collision_mask",
            "_body_set_collision_priority",
            "_body_set_constant_force",
            "_body_set_constant_torque",
            "_body_set_contacts_reported_depth_threshold",
            "_body_set_continuous_collision_detection_mode",
            "_body_set_force_integration_callback",
            "_body_set_max_contacts_reported",
            "_body_set_mode",
            "_body_set_omit_force_integration",
            "_body_set_param",
            "_body_set_pickable",
            "_body_set_shape",
            "_body_set_shape_as_one_way_collision",
            "_body_set_shape_disabled",
            "_body_set_shape_transform",
            "_body_set_space",
            "_body_set_state",
            "_body_set_state_sync_callback",
            "_body_test_motion",
            "_capsule_shape_create",
            "_circle_shape_create",
            "_concave_polygon_shape_create",
            "_convex_polygon_shape_create",
            "_damped_spring_joint_get_param",
            "_damped_spring_joint_set_param",
            "_end_sync",
            "_finish",
            "_flush_queries",
            "_free_rid",
            "_get_process_info",
            "_init",
            "_is_flushing_queries",
            "_joint_clear",
            "_joint_create",
            "_joint_disable_collisions_between_bodies",
            "_joint_get_param",
            "_joint_get_type",
            "_joint_is_disabled_collisions_between_bodies",
            "_joint_make_damped_spring",
            "_joint_make_groove",
            "_joint_make_pin",
            "_joint_set_param",
            "_pin_joint_get_flag",
            "_pin_joint_get_param",
            "_pin_joint_set_flag",
            "_pin_joint_set_param",
            "_rectangle_shape_create",
            "_segment_shape_create",
            "_separation_ray_shape_create",
            "_set_active",
            "_shape_collide",
            "_shape_get_custom_solver_bias",
            "_shape_get_data",
            "_shape_get_type",
            "_shape_set_custom_solver_bias",
            "_shape_set_data",
            "_space_create",
            "_space_get_contact_count",
            "_space_get_contacts",
            "_space_get_direct_state",
            "_space_get_param",
            "_space_is_active",
            "_space_set_active",
            "_space_set_debug_contacts",
            "_space_set_param",
            "_step",
            "_sync",
            "_world_boundary_shape_create",
        ],
    ),
    (
        "PhysicsServer3DExtension",
        "PhysicsServer3D",
        &[
            "_area_add_shape",
            "_area_attach_object_instance_id",
            "_area_clear_shapes",
            "_area_create",
            "_area_get_collision_layer",
            "_area_get_collision_mask",
            "_area_get_object_instance_id",
            "_area_get_param",
            "_area_get_shape",
            "_area_get_shape_count",
            "_area_get_shape_transform",
            "_area_get_space",
            "_area_get_transform",
            "_area_remove_shape",
            "_area_set_area_monitor_callback",
            "_area_set_collision_layer",
            "_area_set_collision_mask",
            "_area_set_monitor_callback",
            "_area_set_monitorable",
            "_area_set_param",
            "_area_set_ray_pickable",
            "_area_set_shape",
            "_area_set_shape_disabled",
            "_area_set_shape_transform",
            "_area_set_space",
            "_area_set_transform",
            "_body_add_collision_exception",
            "_body_add_constant_central_force",
            "_body_add_constant_force",
            "_body_add_constant_torque",
            "_body_add_shape",
            "_body_apply_central_force",
            "_body_apply_central_impulse",
            "_body_apply_force",
            "_body_apply_impulse",
            "_body_apply_torque",
            "_body_apply_torque_impulse",
            "_body_attach_object_instance_id",
            "_body_clear_shapes",
            "_body_create",
            "_body_get_collision_exceptions",
            "_body_get_collision_layer",
            "_body_get_collision_mask",
            "_body_get_collision_priority",
            "_body_get_constant_force",
            "_body_get_constant_torque",
            "_body_get_contacts_reported_depth_threshold",
            "_body_get_direct_state",
            "_body_get_max_contacts_reported",
            "_body_get_mode",
            "_body_get_object_instance_id",
            "_body_get_param",
            "_body_get_shape",
            "_body_get_shape_count",
            "_body_get_shape_transform",
            "_body_get_space",
            "_body_get_state",
            "_body_get_user_flags",
            "_body_is_axis_locked",
            "_body_is_continuous_collision_detection_enabled",
            "_body_is_omitting_force_integration",
            "_body_remove_collision_exception",
            "_body_remove_shape",
            "_body_reset_mass_properties",
            "_body_set_axis_lock",
            "_body_set_axis_velocity",
            "_body_set_collision_layer",
            "_body_set_collision_mask",
            "_body_set_collision_priority",
            "_body_set_constant_force",
            "_body_set_constant_torque",
            "_body_set_contacts_reported_depth_threshold",
            "_body_set_enable_continuous_collision_detection",
            "_body_set_force_integration_callback",
            "_body_set_max_contacts_reported",
            "_body_set_mode",
            "_body_set_omit_force_integration",
            "_body_set_param",
            "_body_set_ray_pickable",
            "_body_set_shape",
            "_body_set_shape_disabled",
            "_body_set_shape_transform",
            "_body_set_space",
            "_body_set_state",
            "_body_set_state_sync_callback",
            "_body_set_user_flags",
            "_body_test_motion",
            "_box_shape_create",
            "_capsule_shape_create",
            "_concave_polygon_shape_create",
            "_cone_twist_joint_get_param",
            "_cone_twist_joint_set_param",
            "_convex_polygon_shape_create",
            "_custom_shape_create",
            "_cylinder_shape_create",
            "_end_sync",
            "_finish",
            "_flush_queries",
            "_free_rid",
            "_generic_6dof_joint_get_flag",
            "_generic_6dof_joint_get_param",
            "_generic_6dof_joint_set_flag",
            "_generic_6dof_joint_set_param",
            "_get_process_info",
            "_heightmap_shape_create",
            "_hinge_joint_get_flag",
            "_hinge_joint_get_param",
            "_hinge_joint_set_flag",
            "_hinge_joint_set_param",
            "_init",
            "_is_flushing_queries",
            "_joint_clear",
            "_joint_create",
            "_joint_disable_collisions_between_bodies",
            "_joint_get_solver_priority",
            "_joint_get_type",
            "_joint_is_disabled_collisions_between_bodies",
            "_joint_make_cone_twist",
            "_joint_make_generic_6dof",
            "_joint_make_hinge",
            "_joint_make_hinge_simple",
            "_joint_make_pin",
            "_joint_make_slider",
            "_joint_set_solver_priority",
            "_pin_joint_get_local_a",
            "_pin_joint_get_local_b",
            "_pin_joint_get_param",
            "_pin_joint_set_local_a",
            "_pin_joint_set_local_b",
            "_pin_joint_set_param",
            "_separation_ray_shape_create",
            "_set_active",
            "_shape_get_custom_solver_bias",
            "_shape_get_data",
            "_shape_get_margin",
            "_shape_get_type",
            "_shape_set_custom_solver_bias",
            "_shape_set_data",
            "_shape_set_margin",
            "_slider_joint_get_param",
            "_slider_joint_set_param",
            "_soft_body_add_collision_exception",
            "_soft_body_apply_central_force",
            "_soft_body_apply_central_impulse",
            "_soft_body_apply_point_force",
            "_soft_body_apply_point_impulse",
            "_soft_body_create",
            "_soft_body_get_bounds",
            "_soft_body_get_collision_exceptions",
            "_soft_body_get_collision_layer",
            "_soft_body_get_collision_mask",
            "_soft_body_get_damping_coefficient",
            "_soft_body_get_drag_coefficient",
            "_soft_body_get_linear_stiffness",
            "_soft_body_get_point_global_position",
            "_soft_body_get_pressure_coefficient",
            "_soft_body_get_shrinking_factor",
            "_soft_body_get_simulation_precision",
            "_soft_body_get_space",
            "_soft_body_get_state",
            "_soft_body_get_total_mass",
            "_soft_body_is_point_pinned",
            "_soft_body_move_point",
            "_soft_body_pin_point",
            "_soft_body_remove_all_pinned_points",
            "_soft_body_remove_collision_exception",
            "_soft_body_set_collision_layer",
            "_soft_body_set_collision_mask",
            "_soft_body_set_damping_coefficient",
            "_soft_body_set_drag_coefficient",
            "_soft_body_set_linear_stiffness",
            "_soft_body_set_mesh",
            "_soft_body_set_pressure_coefficient",
            "_soft_body_set_ray_pickable",
            "_soft_body_set_shrinking_factor",
            "_soft_body_set_simulation_precision",
            "_soft_body_set_space",
            "_soft_body_set_state",
            "_soft_body_set_total_mass",
            "_soft_body_set_transform",
            "_soft_body_update_rendering_server",
            "_space_create",
            "_space_get_contact_count",
            "_space_get_contacts",
            "_space_get_direct_state",
            "_space_get_param",
            "_space_is_active",
            "_space_set_active",
            "_space_set_debug_contacts",
            "_space_set_param",
            "_sphere_shape_create",
            "_step",
            "_sync",
            "_world_boundary_shape_create",
        ],
    ),
    (
        "PhysicsDirectSpaceState2DExtension",
        "PhysicsDirectSpaceState2D",
        &[
            "_cast_motion",
            "_collide_shape",
            "_intersect_point",
            "_intersect_ray",
            "_intersect_shape",
            "_rest_info",
        ],
    ),
    (
        "PhysicsDirectSpaceState3DExtension",
        "PhysicsDirectSpaceState3D",
        &[
            "_cast_motion",
            "_collide_shape",
            "_get_closest_point_to_object_volume",
            "_intersect_point",
            "_intersect_ray",
            "_intersect_shape",
            "_rest_info",
        ],
    ),
    (
        "PhysicsServer3DRenderingServerHandler",
        "Object",
        &["_set_aabb", "_set_normal", "_set_vertex"],
    ),
];

/// The two base classes the six above inherit from that ClassDB did not have.
const MISSING_BASES: &[(&str, &str)] =
    &[("TextServer", "RefCounted"), ("PhysicsServer3D", "Object")];

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** `class_exists`, the parent, and the full virtual
/// list answer correctly for all six.
#[test]
fn server_extension_classes_are_registered() {
    register_all_classes();

    // A miscount in the table would weaken every assertion below without
    // failing one of them.
    let declared: usize = EXPECTED.iter().map(|(_, _, v)| v.len()).sum();
    assert_eq!(
        declared, 589,
        "the doc XML defines 589 virtuals across these six classes"
    );

    for (class, parent, virtuals) in EXPECTED {
        assert!(class_exists(class), "{class} must be registered");

        let info = get_class_info(class).expect("registered class has info");
        assert_eq!(
            &info.parent_class, parent,
            "{class} must inherit {parent}, per its doc XML"
        );

        for name in *virtuals {
            let m = info
                .methods
                .iter()
                .find(|m| m.name == *name)
                .unwrap_or_else(|| panic!("{class} must declare {name}"));
            assert!(m.is_virtual, "{class}.{name} must be marked virtual");
        }
    }
}

/// A base that is not itself registered breaks the chain below it, so the two
/// missing parents are part of this bead rather than someone else's.
#[test]
fn the_parents_resolve_too() {
    register_all_classes();

    for (class, parent) in MISSING_BASES {
        assert!(class_exists(class), "{class} must be registered");
        let info = get_class_info(class).expect("registered class has info");
        assert_eq!(
            &info.parent_class, parent,
            "{class} must inherit {parent}, per its doc XML"
        );
    }

    // The chain is only useful if it actually walks: every Extension's parent
    // has to be a class ClassDB knows, not just a string it stored.
    for (class, parent, _) in EXPECTED {
        assert!(
            class_exists(parent),
            "{class}'s parent {parent} must itself be registered, or the chain stops there"
        );
    }
}

/// The control for the test above: a virtual is reported because it was
/// declared, not because the lookup says yes to anything.
#[test]
fn an_undeclared_virtual_is_absent() {
    register_all_classes();

    for (class, _, _) in EXPECTED {
        let info = get_class_info(class).expect("registered class has info");
        assert!(
            !info.methods.iter().any(|m| m.name == "_no_such_virtual"),
            "{class} must not report a virtual it never declared"
        );
    }
}

/// A duplicate declaration does not error — the later one silently shadows the
/// earlier — so the only way to catch one is to count.
#[test]
fn no_virtual_is_declared_twice() {
    register_all_classes();

    for (class, _, virtuals) in EXPECTED {
        let info = get_class_info(class).expect("registered class has info");
        for name in *virtuals {
            let hits = info.methods.iter().filter(|m| m.name == *name).count();
            assert_eq!(hits, 1, "{class} declares {name} {hits} times");
        }
    }
}

/// The other half of the acceptance: a script can actually name one of these as
/// its base and be constructed.
#[test]
fn a_script_extends_a_server_extension_and_instantiates() {
    register_all_classes();

    let src = "\
extends PhysicsServer2DExtension

var built = true

func ready_marker():
    return built
";
    let mut interp = Interpreter::new();
    let class_def = interp
        .run_class(src)
        .expect("`extends PhysicsServer2DExtension` must resolve its base class");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("the class must instantiate");
    assert_eq!(
        interp
            .call_instance_method(&mut inst, "ready_marker", &[])
            .expect("method call"),
        gdvariant::Variant::Bool(true),
        "the instance must be usable, not merely constructible"
    );
}
