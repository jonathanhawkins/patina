//! pat-0q2kx.237 (partial) — `AnimationNodeExtension` in the ClassDB.
//!
//! Godot ground truth: `register_scene_types.cpp`
//! (`GDREGISTER_VIRTUAL_CLASS(AnimationNodeExtension)`) and
//! `doc/classes/AnimationNodeExtension.xml`.
//!
//! ⚠️ SCOPE: registration only. The bead also asks that
//! `_process_animation_node` be INVOKED during AnimationTree blend-tree
//! evaluation with its return value influencing the pose — NOT delivered, so
//! the acceptance test `animation_node_extension_virtual_runs_in_blend_tree`
//! does not exist. See the completion report.

use gdobject::class_db::{
    class_exists, get_class_info, is_abstract, is_parent_class, register_3d_classes,
};

#[test]
fn animation_node_extension_is_registered_under_animation_node() {
    register_3d_classes();

    assert!(class_exists("AnimationNodeExtension"));
    assert!(
        is_parent_class("AnimationNodeExtension", "AnimationNode"),
        "AnimationNodeExtension.xml declares inherits=\"AnimationNode\""
    );
    // Through AnimationNode to Resource — the chain, not just the direct link.
    assert!(
        is_parent_class("AnimationNodeExtension", "Resource"),
        "AnimationNode is a Resource, so the extension must reach it"
    );
}

/// VIRTUAL is not ABSTRACT. Its own parent IS abstract, so copying the parent's
/// flag — the natural move — would make the class un-constructible and defeat
/// the point of an "Extension" base.
#[test]
fn it_is_virtual_not_abstract_unlike_its_parent() {
    register_3d_classes();

    assert!(
        !is_abstract("AnimationNodeExtension"),
        "GDREGISTER_VIRTUAL_CLASS keeps a creation func, so Godot reports \
         is_abstract == false; a script must be able to construct one"
    );
    assert!(
        is_abstract("AnimationNode"),
        "the parent IS abstract — this pair is exactly why the flag cannot be \
         inherited by eye"
    );
}

/// The three declared methods, with the arities the doc XML gives.
#[test]
fn the_virtual_surface_has_the_documented_arities() {
    register_3d_classes();
    let info = get_class_info("AnimationNodeExtension").expect("registered");

    for (name, arity) in [
        // (playback_info: PackedFloat64Array, test_only: bool)
        // -> PackedFloat32Array. TWO params — the bead's
        // (time, seek, is_external_seeking) is an older, wrong shape.
        ("_process_animation_node", 2usize),
        // (node_info: PackedFloat32Array, break_loop: bool) -> float
        ("get_remaining_time", 2),
        // (node_info: PackedFloat32Array) -> bool
        ("is_looping", 1),
    ] {
        let m = info
            .methods
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("AnimationNodeExtension must declare `{name}`"));
        assert_eq!(
            m.argument_count, arity,
            "`{name}` takes {arity} argument(s) per AnimationNodeExtension.xml"
        );
    }

    assert_eq!(
        info.methods.len(),
        3,
        "exactly the three the doc declares — no invented extras"
    );
}
