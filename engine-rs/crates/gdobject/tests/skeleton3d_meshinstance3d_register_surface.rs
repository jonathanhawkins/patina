//! pat-s3rrv.23 (slice) — `Skeleton3D` and `MeshInstance3D` carry their real
//! ClassDB member surfaces.
//!
//! Both were registered as bare one-liners —
//! `ClassRegistration::new("Skeleton3D").parent("Node3D")` — with zero
//! properties, methods and signals.
//!
//! ⚠️ **That is not cosmetic metadata.** `SceneTree`'s script dispatch gates on
//! `class_has_method`, so an empty method table meant every `get_bone_count` /
//! `set_bone_pose_position` / `set_blend_shape_value` call from GDScript was
//! rejected *before* it reached the `skeleton3d` implementation that already
//! exists and works. The bone and blend-shape APIs were unreachable from script
//! regardless of anything in the render path.
//!
//! Names and arities are diffed against
//! `upstream/godot/scene/3d/skeleton_3d.cpp` / `mesh_instance_3d.cpp`
//! `_bind_methods` and the doc XML — not invented to satisfy the bead.

use gdobject::class_db::{
    class_exists, class_has_method, class_has_signal, get_property_list, inheritance_chain,
    register_3d_classes,
};

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// **The slice's acceptance.** The bone API is reachable through the same gate
/// script dispatch consults.
#[test]
fn skeleton3d_exposes_its_bone_api_to_the_dispatch_gate() {
    register_3d_classes();
    assert!(class_exists("Skeleton3D"), "Skeleton3D is registered");

    // The roster, rest pose and live pose — the three groups a rig needs.
    for method in [
        "get_bone_count",
        "add_bone",
        "find_bone",
        "get_bone_name",
        "get_bone_parent",
        "get_bone_children",
        "get_parentless_bones",
        "get_bone_rest",
        "set_bone_rest",
        "get_bone_pose",
        "get_bone_pose_position",
        "set_bone_pose_position",
        "set_bone_pose_rotation",
        "set_bone_pose_scale",
        "get_bone_global_pose",
        "reset_bone_pose",
        "reset_bone_poses",
        "is_bone_enabled",
        "set_bone_enabled",
        "clear_bones",
        "localize_rests",
        "get_version",
    ] {
        assert!(
            class_has_method("Skeleton3D", method),
            "Skeleton3D must expose `{method}` — script dispatch gates on \
             class_has_method, so an absent entry makes the call unreachable"
        );
    }

    // All six signals from doc/classes/Skeleton3D.xml.
    for signal in [
        "rest_updated",
        "pose_updated",
        "skeleton_updated",
        "bone_enabled_changed",
        "bone_list_changed",
        "show_rest_only_changed",
    ] {
        assert!(
            class_has_signal("Skeleton3D", signal),
            "Skeleton3D must declare the `{signal}` signal"
        );
    }

    let props = props("Skeleton3D");
    for name in [
        "motion_scale",
        "show_rest_only",
        "animate_physical_bones",
        "modifier_callback_mode_process",
    ] {
        assert!(
            props.iter().any(|p| p == name),
            "Skeleton3D must expose `{name}`, got {props:?}"
        );
    }

    // Parent chain intact — an inheritance-aware lookup must still reach Node3D.
    assert!(
        inheritance_chain("Skeleton3D")
            .iter()
            .any(|c| c == "Node3D"),
        "Skeleton3D still descends from Node3D"
    );
}

/// `MeshInstance3D` exposes the mesh/skin/skeleton slots and the blend-shape and
/// mesh-tool methods.
#[test]
fn mesh_instance3d_exposes_mesh_skin_and_blend_shape_surface() {
    register_3d_classes();
    assert!(class_exists("MeshInstance3D"));

    let props = props("MeshInstance3D");
    for name in ["mesh", "skin", "skeleton"] {
        assert!(
            props.iter().any(|p| p == name),
            "MeshInstance3D must expose `{name}` — the slot a .tscn assigns and \
             the render path resolves geometry through; got {props:?}"
        );
    }

    for method in [
        "get_blend_shape_count",
        "find_blend_shape_by_name",
        "get_blend_shape_value",
        "set_blend_shape_value",
        "get_surface_override_material_count",
        "set_surface_override_material",
        "get_surface_override_material",
        "get_active_material",
        "create_trimesh_collision",
        "create_convex_collision",
        "create_multiple_convex_collisions",
        "create_debug_tangents",
        "create_outline",
    ] {
        assert!(
            class_has_method("MeshInstance3D", method),
            "MeshInstance3D must expose `{method}`"
        );
    }

    assert!(
        inheritance_chain("MeshInstance3D")
            .iter()
            .any(|c| c == "GeometryInstance3D"),
        "MeshInstance3D still descends from GeometryInstance3D"
    );
}

/// Guards the regression this slice fixes: a registration with a parent but no
/// members reads as "registered" while every method call is still gated off.
/// `class_exists` alone is therefore not evidence the API is reachable.
#[test]
fn class_exists_alone_does_not_prove_the_api_is_reachable() {
    register_3d_classes();

    // Both classes existed before this bead — that was never the problem.
    assert!(class_exists("Skeleton3D"));
    assert!(class_exists("MeshInstance3D"));

    // What was missing, and what the dispatch gate actually consults:
    assert!(
        class_has_method("Skeleton3D", "get_bone_count"),
        "the gate `class_has_method` is what makes the call reachable, not \
         `class_exists` — this is exactly the distinction the empty \
         registration hid"
    );
    assert!(class_has_method("MeshInstance3D", "set_blend_shape_value"));
}
