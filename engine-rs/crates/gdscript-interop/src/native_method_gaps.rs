//! Which ClassDB registrations the GDScript interpreter can actually *call*
//! (pat-mgixk.7).
//!
//! # The failure this closes
//!
//! Godot's `Object::callp` (`core/object/object.cpp`) looks a method up in
//! ClassDB and runs its `MethodBind`. Every registered method therefore either
//! does real work or raises a script error; it never answers a silent `null`.
//!
//! `Interpreter::call_object_method` did the opposite. When `node_has_method`
//! said yes (ClassDB knows the name) and no dispatch arm answered, it returned
//! `Ok(Variant::Nil)`. Measured over the 265 Node-derived registered classes,
//! that covered hundreds of names. Of the 562 non-underscore methods registered
//! on the 265 Node-derived classes, only 80 have an unguarded arm in the tables a
//! script call consults — so `has_method()` lied far more often than it told the
//! truth. It is also why the same gap was patched one method at a time at least
//! four times (`is_class` in `interpreter.rs`, `camera3d.rs:1006`, and the
//! Skeleton3D and LineEdit arms in `scripting.rs`), each with a comment
//! confessing it, without anyone measuring the shape of the problem.
//!
//! # How the three tables divide that set
//!
//! The classification is mechanical, and its oracle is Godot's own
//! documentation — all 1065 `*.xml` files under `upstream/godot/doc/classes/`
//! **and** `upstream/godot/**/doc_classes/` (GridMap, the multiplayer nodes and
//! the GDScript classes are documented in their module directories; leaving
//! those out made 34 real Godot methods look like registration bugs). A name
//! counts as documented when it is a `<method>` **or** the `getter=`/`setter=`
//! of a `<member>`: Godot binds property accessors as callable methods but
//! documents them under the property, so matching only `<method>`
//! misclassified every accessor.
//!
//! * [`VOID_NOOP_METHODS`] — the name returns `void` on **every** Godot class
//!   that declares it. A missing implementation cannot be observed through a
//!   return value, so raising would break callers that correctly ignore the
//!   result. These keep answering `Nil`. This is where the remaining silence
//!   lives, and it is the natural target for the per-class beads.
//! * [`KNOWN_MISSING_NATIVE_METHODS`] — registered, no dispatch arm, and
//!   returns a *value* in Godot. A `null` is a wrong answer, so these raise.
//! * [`NON_GODOT_REGISTRATIONS`] — registered on a class whose Godot
//!   documentation has no such method at all. Registration bugs:
//!   `has_method()` answers true for something Godot does not have.
//!
//! # The gap side is deliberately OVER-approximated
//!
//! "Does class C dispatch method m" cannot be answered from the source with
//! certainty, because the node tables are keyed by name with per-class guards
//! (`"get_total_character_count" if is_rich_text_label(..) =>`). A guarded arm
//! covers some classes and not others, and resolving which would mean
//! evaluating the predicate.
//!
//! So the generator counts a name as dispatched only when it has an
//! **unguarded** arm inside the two tables a `obj.<method>()` call on a node
//! actually consults — `call_method_on`'s `Variant::ObjectId` arm and
//! `SceneAccess::call_node_method`. Everything else lands in a gap table.
//!
//! That is sound in exactly one direction, which is the direction the sweep
//! needs: the sweep asserts the observed raisers are a **subset** of these
//! tables, so listing a pair that turns out to dispatch fine costs nothing,
//! while missing one fails the test. Scanning more broadly is what produced a
//! failed verify: `"clear"` has an arm for `Array`, which says nothing about
//! `GridMap.clear`.

//! # These tables may only shrink
//!
//! `gdscript_classdb_dispatch_sweep_test` walks `ClassDB::get_class_list()`,
//! calls every zero-argument registered method on every instantiable
//! Node-derived class *through the interpreter*, and fails if anything raises
//! that is not listed here. Landing a dispatch arm is expected to shorten a
//! list; adding a new undispatched registration cannot pass unnoticed.

/// Registered-but-undispatched method names that return `void` in Godot on
/// every class declaring them.
///
/// MUST stay sorted — [`is_void_noop`] binary-searches it, and an unsorted
/// table silently answers `false` for names that are in it. A unit test below
/// enforces that.
pub const VOID_NOOP_METHODS: &[&str] = &[
    "add_autoload_singleton",
    "add_command",
    "add_control_to_dock",
    "add_custom_type",
    "add_gizmo",
    "add_preview_generator",
    "add_resource",
    "add_resource_tooltip_plugin",
    "add_spawnable_scene",
    "add_syntax_highlighter",
    "add_text",
    "append_text",
    "cancel_request",
    "check_for_invalidation",
    "clear",
    "clear_baked_meshes",
    "clear_bones",
    "clear_chains",
    "clear_current",
    "clear_docs_from_script",
    "clear_gizmos",
    "clear_selection",
    "clear_setting",
    "clear_settings",
    "clear_spawnable_scenes",
    "clear_subgizmo_selection",
    "config",
    "convert_from_particles",
    "create_convex_collision",
    "create_debug_tangents",
    "create_multiple_convex_collisions",
    "create_trimesh_collision",
    "deselect",
    "emit_particle",
    "force_update_transform",
    "global_rotate",
    "global_scale",
    "global_translate",
    "goto_help",
    "goto_line",
    "insert_text_at_caret",
    "localize_rests",
    "look_at_from_position",
    "make_current",
    "make_visible",
    "navigate_to_path",
    "open_scene_from_path",
    "open_script_create_dialog",
    "orthonormalize",
    "push_toast",
    "queue_edited_resource_preview",
    "queue_resource_preview",
    "register_syntax_highlighter",
    "reimport_files",
    "remove_autoload_singleton",
    "remove_command",
    "remove_control_from_bottom_panel",
    "remove_control_from_docks",
    "remove_custom_type",
    "remove_preview_generator",
    "remove_resource",
    "remove_resource_tooltip_plugin",
    "rename_resource",
    "request_particles_process",
    "reset",
    "reset_bone_pose",
    "reset_bone_poses",
    "restart",
    "rotate_object_local",
    "scale_object_local",
    "scan",
    "scan_sources",
    "select_all",
    "set_active",
    "set_additive",
    "set_amount",
    "set_angular_delta_limit",
    "set_animation_path",
    "set_apply_axis",
    "set_apply_bone",
    "set_apply_bone_name",
    "set_apply_range_max",
    "set_apply_range_min",
    "set_apply_transform_mode",
    "set_avoidance_layer_value",
    "set_avoidance_mask_value",
    "set_axis_flags",
    "set_bake_mask",
    "set_bake_mask_value",
    "set_bake_simplification_distance",
    "set_blend_shape_value",
    "set_bone_enabled",
    "set_bone_name",
    "set_bone_parent",
    "set_bone_pose_position",
    "set_bone_pose_rotation",
    "set_bone_pose_scale",
    "set_bone_rest",
    "set_cast_shadows_setting",
    "set_cell_item",
    "set_cell_scale",
    "set_cell_size",
    "set_chain_count",
    "set_color",
    "set_copy_flags",
    "set_copy_position",
    "set_copy_rotation",
    "set_copy_scale",
    "set_cull_mask_value",
    "set_damping_curve",
    "set_deterministic",
    "set_disable_scale",
    "set_disperse_mode",
    "set_distraction_free_mode",
    "set_end_bone",
    "set_end_bone_direction",
    "set_end_bone_length",
    "set_end_bone_name",
    "set_exclude",
    "set_extend_end_bone",
    "set_forward_axis",
    "set_frustum",
    "set_identity",
    "set_ignore_transform_notification",
    "set_influence",
    "set_invert_flags",
    "set_joint_limitation",
    "set_joint_limitation_right_axis",
    "set_joint_limitation_right_axis_vector",
    "set_joint_limitation_rotation_offset",
    "set_joint_rotation_axis",
    "set_joint_rotation_axis_vector",
    "set_joint_twist_amount",
    "set_layer_mask",
    "set_material",
    "set_max_angular_velocity",
    "set_max_iterations",
    "set_max_redirects",
    "set_mesh",
    "set_mesh_library",
    "set_min_distance",
    "set_mutable_bone_axes",
    "set_name",
    "set_navigation_layer_value",
    "set_navigation_map",
    "set_notify_local_transform",
    "set_notify_transform",
    "set_occluder",
    "set_orthogonal",
    "set_perspective",
    "set_primary_rotation_axis",
    "set_radius",
    "set_reference_axis",
    "set_reference_bone",
    "set_reference_bone_name",
    "set_reference_node",
    "set_reference_range_max",
    "set_reference_range_min",
    "set_reference_transform_mode",
    "set_reference_type",
    "set_relative",
    "set_replication_config",
    "set_replication_interval",
    "set_root_bone",
    "set_root_bone_name",
    "set_root_path",
    "set_selected_palette_item",
    "set_selection",
    "set_setting_count",
    "set_shape",
    "set_size",
    "set_skeleton_path",
    "set_skin",
    "set_spawn_limit",
    "set_spawn_path",
    "set_subgizmo_selection",
    "set_surface_override_material",
    "set_target_node",
    "set_timeout",
    "set_twist_from",
    "set_twist_from_rest",
    "set_use_euler",
    "set_use_secondary_rotation",
    "set_velocity_forced",
    "set_visibility_range_begin",
    "set_visibility_range_begin_margin",
    "set_visibility_range_end",
    "set_visibility_range_end_margin",
    "set_visibility_range_fade_mode",
    "set_weight_position",
    "set_zero_y",
    "translate_object_local",
    "unregister_syntax_highlighter",
    "update_docs_from_script",
    "update_file",
    "update_gizmos",
    "update_visibility",
];

/// `Class.method` pairs that are registered, have no dispatch arm, and return a
/// value in Godot. These raise rather than answer a wrong `null`.
pub const KNOWN_MISSING_NATIVE_METHODS: &[&str] = &[
    "AimModifier3D.get_forward_axis",
    "AimModifier3D.get_primary_rotation_axis",
    "AimModifier3D.is_relative",
    "AimModifier3D.is_using_euler",
    "AimModifier3D.is_using_secondary_rotation",
    "AudioListener2D.is_current",
    "AudioListener3D.is_current",
    "BoneConstraint3D.get_amount",
    "BoneConstraint3D.get_apply_bone",
    "BoneConstraint3D.get_apply_bone_name",
    "BoneConstraint3D.get_reference_bone",
    "BoneConstraint3D.get_reference_bone_name",
    "BoneConstraint3D.get_reference_node",
    "BoneConstraint3D.get_reference_type",
    "BoneConstraint3D.get_setting_count",
    "BoneTwistDisperser3D.are_bone_axes_mutable",
    "BoneTwistDisperser3D.get_damping_curve",
    "BoneTwistDisperser3D.get_disperse_mode",
    "BoneTwistDisperser3D.get_end_bone",
    "BoneTwistDisperser3D.get_end_bone_direction",
    "BoneTwistDisperser3D.get_end_bone_name",
    "BoneTwistDisperser3D.get_joint_bone",
    "BoneTwistDisperser3D.get_joint_bone_name",
    "BoneTwistDisperser3D.get_joint_count",
    "BoneTwistDisperser3D.get_joint_twist_amount",
    "BoneTwistDisperser3D.get_reference_bone",
    "BoneTwistDisperser3D.get_reference_bone_name",
    "BoneTwistDisperser3D.get_root_bone",
    "BoneTwistDisperser3D.get_root_bone_name",
    "BoneTwistDisperser3D.get_setting_count",
    "BoneTwistDisperser3D.get_twist_from",
    "BoneTwistDisperser3D.get_weight_position",
    "BoneTwistDisperser3D.is_end_bone_extended",
    "BoneTwistDisperser3D.is_twist_from_rest",
    "Camera3D.get_camera_projection",
    "Camera3D.get_camera_rid",
    "Camera3D.get_camera_transform",
    "Camera3D.get_cull_mask_value",
    "Camera3D.get_frustum",
    "Camera3D.get_pyramid_shape_rid",
    "Camera3D.is_position_behind",
    "Camera3D.is_position_in_frustum",
    "Camera3D.project_local_ray_normal",
    "Camera3D.project_position",
    "Camera3D.project_ray_normal",
    "Camera3D.project_ray_origin",
    "Camera3D.unproject_position",
    "ChainIK3D.get_end_bone",
    "ChainIK3D.get_end_bone_direction",
    "ChainIK3D.get_end_bone_length",
    "ChainIK3D.get_end_bone_name",
    "ChainIK3D.get_joint_bone",
    "ChainIK3D.get_joint_bone_name",
    "ChainIK3D.get_joint_count",
    "ChainIK3D.get_root_bone",
    "ChainIK3D.get_root_bone_name",
    "ChainIK3D.is_end_bone_extended",
    "ConvertTransformModifier3D.get_apply_axis",
    "ConvertTransformModifier3D.get_apply_range_max",
    "ConvertTransformModifier3D.get_apply_range_min",
    "ConvertTransformModifier3D.get_apply_transform_mode",
    "ConvertTransformModifier3D.get_reference_axis",
    "ConvertTransformModifier3D.get_reference_range_max",
    "ConvertTransformModifier3D.get_reference_range_min",
    "ConvertTransformModifier3D.get_reference_transform_mode",
    "ConvertTransformModifier3D.is_additive",
    "ConvertTransformModifier3D.is_relative",
    "CopyTransformModifier3D.get_axis_flags",
    "CopyTransformModifier3D.get_copy_flags",
    "CopyTransformModifier3D.get_invert_flags",
    "CopyTransformModifier3D.is_additive",
    "CopyTransformModifier3D.is_position_copying",
    "CopyTransformModifier3D.is_relative",
    "CopyTransformModifier3D.is_rotation_copying",
    "CopyTransformModifier3D.is_scale_copying",
    "EditorDock.close",
    "EditorDock.open",
    "EditorFileSystem.get_file_type",
    "EditorFileSystem.get_filesystem",
    "EditorFileSystem.get_filesystem_path",
    "EditorFileSystem.get_scanning_progress",
    "EditorFileSystem.is_scanning",
    "EditorInterface.get_edited_scene_root",
    "EditorInterface.get_editor_settings",
    "EditorInterface.get_file_system_dock",
    "EditorInterface.get_inspector",
    "EditorInterface.get_selection",
    "EditorInterface.is_distraction_free_mode_enabled",
    "EditorInterface.save_scene",
    "EditorPlugin.add_control_to_bottom_panel",
    "EditorPlugin.get_editor_interface",
    "FogVolume.get_material",
    "FogVolume.get_shape",
    "FogVolume.get_size",
    "GPUParticles2D.capture_rect",
    "GridMap.get_cell_item",
    "GridMap.get_cell_item_basis",
    "GridMap.get_cell_item_orientation",
    "GridMap.get_cell_scale",
    "GridMap.get_cell_size",
    "GridMap.get_mesh_library",
    "GridMap.get_used_cells",
    "GridMap.get_used_cells_by_item",
    "GridMap.local_to_map",
    "GridMap.map_to_local",
    "GridMapEditorPlugin.get_current_grid_map",
    "GridMapEditorPlugin.get_selected_cells",
    "GridMapEditorPlugin.get_selected_palette_item",
    "GridMapEditorPlugin.get_selection",
    "GridMapEditorPlugin.has_selection",
    "HTTPRequest.get_http_client_status",
    "HTTPRequest.get_max_redirects",
    "HTTPRequest.get_timeout",
    "HTTPRequest.request",
    "HTTPRequest.request_raw",
    "ImporterMeshInstance3D.get_cast_shadows_setting",
    "ImporterMeshInstance3D.get_layer_mask",
    "ImporterMeshInstance3D.get_mesh",
    "ImporterMeshInstance3D.get_skeleton_path",
    "ImporterMeshInstance3D.get_skin",
    "ImporterMeshInstance3D.get_visibility_range_begin",
    "ImporterMeshInstance3D.get_visibility_range_begin_margin",
    "ImporterMeshInstance3D.get_visibility_range_end",
    "ImporterMeshInstance3D.get_visibility_range_end_margin",
    "ImporterMeshInstance3D.get_visibility_range_fade_mode",
    "InstancePlaceholder.create_instance",
    "InstancePlaceholder.get_instance_path",
    "InstancePlaceholder.get_stored_values",
    "IterateIK3D.get_angular_delta_limit",
    "IterateIK3D.get_joint_limitation",
    "IterateIK3D.get_joint_limitation_right_axis",
    "IterateIK3D.get_joint_limitation_right_axis_vector",
    "IterateIK3D.get_joint_limitation_rotation_offset",
    "IterateIK3D.get_joint_rotation_axis",
    "IterateIK3D.get_joint_rotation_axis_vector",
    "IterateIK3D.get_max_iterations",
    "IterateIK3D.get_min_distance",
    "IterateIK3D.get_target_node",
    "IterateIK3D.is_deterministic",
    "LimitAngularVelocityModifier3D.get_chain_count",
    "LimitAngularVelocityModifier3D.get_end_bone",
    "LimitAngularVelocityModifier3D.get_end_bone_name",
    "LimitAngularVelocityModifier3D.get_max_angular_velocity",
    "LimitAngularVelocityModifier3D.get_root_bone",
    "LimitAngularVelocityModifier3D.get_root_bone_name",
    "LimitAngularVelocityModifier3D.is_exclude",
    "LineEdit.get_selected_text",
    "LineEdit.has_selection",
    "MeshInstance3D.find_blend_shape_by_name",
    "MeshInstance3D.get_active_material",
    "MeshInstance3D.get_blend_shape_count",
    "MeshInstance3D.get_blend_shape_value",
    "MeshInstance3D.get_surface_override_material",
    "MeshInstance3D.get_surface_override_material_count",
    "MultiplayerSpawner.get_spawn_limit",
    "MultiplayerSpawner.get_spawn_path",
    "MultiplayerSpawner.get_spawnable_scene",
    "MultiplayerSpawner.get_spawnable_scene_count",
    "MultiplayerSpawner.spawn",
    "MultiplayerSynchronizer.get_replication_config",
    "MultiplayerSynchronizer.get_replication_interval",
    "MultiplayerSynchronizer.get_root_path",
    "MultiplayerSynchronizer.get_visibility_for",
    "NavigationAgent2D.distance_to_target",
    "NavigationAgent2D.get_avoidance_layer_value",
    "NavigationAgent2D.get_avoidance_mask_value",
    "NavigationAgent2D.get_current_navigation_path",
    "NavigationAgent2D.get_current_navigation_path_index",
    "NavigationAgent2D.get_current_navigation_result",
    "NavigationAgent2D.get_final_position",
    "NavigationAgent2D.get_navigation_layer_value",
    "NavigationAgent2D.get_navigation_map",
    "NavigationAgent2D.get_next_path_position",
    "NavigationAgent2D.get_path_length",
    "NavigationAgent2D.get_rid",
    "NavigationAgent2D.is_navigation_finished",
    "NavigationAgent2D.is_target_reachable",
    "NavigationAgent2D.is_target_reached",
    "NavigationAgent3D.distance_to_target",
    "NavigationAgent3D.get_avoidance_layer_value",
    "NavigationAgent3D.get_avoidance_mask_value",
    "NavigationAgent3D.get_current_navigation_path",
    "NavigationAgent3D.get_current_navigation_path_index",
    "NavigationAgent3D.get_current_navigation_result",
    "NavigationAgent3D.get_final_position",
    "NavigationAgent3D.get_navigation_layer_value",
    "NavigationAgent3D.get_navigation_map",
    "NavigationAgent3D.get_next_path_position",
    "NavigationAgent3D.get_path_length",
    "NavigationAgent3D.get_rid",
    "NavigationAgent3D.is_navigation_finished",
    "NavigationAgent3D.is_target_reachable",
    "NavigationAgent3D.is_target_reached",
    "Node.duplicate",
    "Node.get_path",
    "Node.get_tree",
    "Node3D.get_gizmos",
    "Node3D.get_global_transform_interpolated",
    "Node3D.get_parent_node_3d",
    "Node3D.is_local_transform_notification_enabled",
    "Node3D.is_scale_disabled",
    "Node3D.is_transform_notification_enabled",
    "Node3D.is_visible_in_tree",
    "Node3D.translate",
    "OccluderInstance3D.get_bake_mask",
    "OccluderInstance3D.get_bake_mask_value",
    "OccluderInstance3D.get_bake_simplification_distance",
    "OccluderInstance3D.get_occluder",
    "RichTextLabel.get_parsed_text",
    "RichTextLabel.get_total_character_count",
    "RootMotionView.get_animation_path",
    "RootMotionView.get_cell_size",
    "RootMotionView.get_color",
    "RootMotionView.get_radius",
    "RootMotionView.get_zero_y",
    "ScriptEditor.get_breakpoints",
    "ScriptEditor.get_current_editor",
    "ScriptEditor.get_current_script",
    "ScriptEditor.get_open_script_editors",
    "ScriptEditor.get_open_scripts",
    "ScriptEditorBase.get_base_editor",
    "Skeleton3D.add_bone",
    "Skeleton3D.find_bone",
    "Skeleton3D.get_bone_children",
    "Skeleton3D.get_bone_count",
    "Skeleton3D.get_bone_global_pose",
    "Skeleton3D.get_bone_name",
    "Skeleton3D.get_bone_parent",
    "Skeleton3D.get_bone_pose",
    "Skeleton3D.get_bone_pose_position",
    "Skeleton3D.get_bone_pose_rotation",
    "Skeleton3D.get_bone_pose_scale",
    "Skeleton3D.get_bone_rest",
    "Skeleton3D.get_parentless_bones",
    "Skeleton3D.get_version",
    "Skeleton3D.is_bone_enabled",
    "SkeletonModifier3D.get_influence",
    "SkeletonModifier3D.get_skeleton",
    "SkeletonModifier3D.is_active",
    "SubViewport.get_size",
    "SubViewport.get_texture",
    "TextEdit.get_selected_text",
    "TextEdit.has_selection",
    "Viewport.get_texture",
    "Window.get_viewport",
    "Window.get_window",
];

/// `Class.method` pairs registered on a class whose Godot documentation has no
/// such method.
///
/// ⚠️ Deliberately **not** deleted from `class_db.rs` in this change: several of
/// these names are asserted to *be* registered by existing `*_class_surface`
/// tests, so removing a registration is a separate change with its own blast
/// radius. This table is the enforced ledger until then.
pub const NON_GODOT_REGISTRATIONS: &[&str] = &[
    "EditorInterface.reload_scene_from_disk",
    "MeshInstance3D.create_outline",
    "MultiplayerSynchronizer.get_net_id",
    "MultiplayerSynchronizer.set_net_id",
];

/// Length of [`VOID_NOOP_METHODS`] when this table was generated.
pub const VOID_NOOP_COUNT: usize = 197;
/// Length of [`KNOWN_MISSING_NATIVE_METHODS`] when this table was generated.
pub const KNOWN_MISSING_COUNT: usize = 246;
/// Length of [`NON_GODOT_REGISTRATIONS`] when this table was generated.
pub const NON_GODOT_COUNT: usize = 4;

/// Whether `method` is an allow-listed void no-op: a registered method with no
/// dispatch arm whose Godot return type is `void`.
///
/// Called from `call_object_method`'s miss path, so it is a binary search over a
/// sorted `&[&str]` rather than a set build per call.
pub fn is_void_noop(method: &str) -> bool {
    VOID_NOOP_METHODS.binary_search(&method).is_ok()
}

/// Whether `class_name.method` is a recorded gap — either a Godot method Patina
/// has not wired yet, or a registration Godot does not have.
pub fn is_known_gap(class_name: &str, method: &str) -> bool {
    let pair = format!("{class_name}.{method}");
    KNOWN_MISSING_NATIVE_METHODS
        .binary_search(&pair.as_str())
        .is_ok()
        || NON_GODOT_REGISTRATIONS
            .binary_search(&pair.as_str())
            .is_ok()
}

/// The message `call_object_method` raises for a registered method with no
/// implementation.
///
/// Lives here so the text has exactly one definition: the tests assert on this
/// function rather than on a copy of the string.
pub fn not_implemented_message(class_name: &str, method: &str) -> String {
    format!(
        "Native method '{class_name}.{method}' is registered in ClassDB but not implemented in Patina"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn void_noop_table_is_sorted_and_deduped() {
        let mut sorted = VOID_NOOP_METHODS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.as_slice(),
            VOID_NOOP_METHODS,
            "VOID_NOOP_METHODS must stay sorted and deduplicated, or is_void_noop's \
             binary search answers false for names that are in it"
        );
    }

    #[test]
    fn gap_tables_are_sorted_deduped_and_class_qualified() {
        for (label, table) in [
            ("KNOWN_MISSING_NATIVE_METHODS", KNOWN_MISSING_NATIVE_METHODS),
            ("NON_GODOT_REGISTRATIONS", NON_GODOT_REGISTRATIONS),
        ] {
            let mut sorted = table.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                sorted.as_slice(),
                table,
                "{label} must stay sorted and deduplicated — is_known_gap binary-searches it"
            );
            for entry in table {
                let parts: Vec<&str> = entry.split('.').collect();
                assert!(
                    parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty(),
                    "{label} entries are `Class.method`, got {entry:?}"
                );
            }
        }
    }

    #[test]
    fn counts_match_the_tables() {
        assert_eq!(VOID_NOOP_METHODS.len(), VOID_NOOP_COUNT);
        assert_eq!(KNOWN_MISSING_NATIVE_METHODS.len(), KNOWN_MISSING_COUNT);
        assert_eq!(NON_GODOT_REGISTRATIONS.len(), NON_GODOT_COUNT);
    }

    #[test]
    fn the_two_gap_tables_do_not_overlap() {
        for entry in KNOWN_MISSING_NATIVE_METHODS {
            assert!(
                !NON_GODOT_REGISTRATIONS.contains(entry),
                "{entry} is in both gap tables, so the generator double-counted it"
            );
        }
    }

    #[test]
    fn a_gap_entrys_method_is_never_an_allow_listed_void_noop() {
        // Otherwise the runtime answers Nil for a pair the sweep expects to
        // raise, and the sweep is vacuous for it.
        for entry in KNOWN_MISSING_NATIVE_METHODS
            .iter()
            .chain(NON_GODOT_REGISTRATIONS.iter())
        {
            let method = entry.split('.').nth(1).unwrap_or("");
            assert!(
                !is_void_noop(method),
                "{entry} is listed as a gap, but {method} is an allow-listed void no-op, \
                 so calling it can never raise"
            );
        }
    }

    #[test]
    fn is_known_gap_finds_entries_from_both_tables() {
        let (kc, km) = KNOWN_MISSING_NATIVE_METHODS[0].split_once('.').unwrap();
        assert!(is_known_gap(kc, km), "{kc}.{km} should be a known gap");
        let (nc, nm) = NON_GODOT_REGISTRATIONS[0].split_once('.').unwrap();
        assert!(is_known_gap(nc, nm), "{nc}.{nm} should be a known gap");
        assert!(
            !is_known_gap("Node", "get_name"),
            "get_name has a real dispatch arm and must not read as a gap"
        );
    }

    #[test]
    fn the_message_names_both_the_class_and_the_method() {
        let msg = not_implemented_message("MeshInstance3D", "create_outline");
        assert!(msg.contains("MeshInstance3D"), "{msg}");
        assert!(msg.contains("create_outline"), "{msg}");
        assert!(msg.contains("not implemented"), "{msg}");
    }

    #[test]
    fn create_outline_is_the_recorded_gap_the_acceptance_uses() {
        // The acceptance test calls `$Mesh.create_outline(1.0)` and expects a
        // raise; if this pair ever leaves the ledger, that test is asserting
        // something else.
        assert!(is_known_gap("MeshInstance3D", "create_outline"));
    }
}
