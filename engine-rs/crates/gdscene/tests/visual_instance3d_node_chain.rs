//! pat-0q2kx.63 acceptance: the 3D visual nodes derive from `VisualInstance3D` /
//! `GeometryInstance3D`, not straight from `Node3D`.
//!
//! Godot: `Light3D`, `Decal`, `ReflectionProbe`, `VoxelGI`, `LightmapGI` and
//! `VisibleOnScreenNotifier3D` derive from `VisualInstance3D`; `Label3D` and
//! `CPUParticles3D` from `GeometryInstance3D` (`scene/3d/visual_instance_3d.h`).
//! Those bases own the cull-`layers` and `material_override`/`cast_shadow`
//! surface, so a light that is not a `VisualInstance3D` cannot inherit the cull
//! layers the renderer and the editor gizmos read.
//!
//! Patina had all eight parented directly to `Node3D` while both bases were
//! already registered with the right chain.
//!
//! ⚠️ The dangerous half of a re-parent is not the `.parent()` call, it is the
//! chain SEVERING: `inheritance_chain` resolves parents by name at query time and
//! stops at the first unregistered link. `Label3D` and `VisibleOnScreenNotifier3D`
//! are registered by the **2D** table while their new bases live in the 3D one,
//! so `register_2d_classes` now bootstraps those bases — and the second test here
//! registers ONE entry point at a time to prove it, because asserting only
//! against a fully-populated ClassDB hides exactly this bug.

use gdscene::node::Node;
use gdscene::SceneTree;

/// Every class the bead moves, with the base it must now derive from.
const REPARENTED: &[(&str, &str)] = &[
    ("Light3D", "VisualInstance3D"),
    ("Decal", "VisualInstance3D"),
    ("ReflectionProbe", "VisualInstance3D"),
    ("VoxelGI", "VisualInstance3D"),
    ("LightmapGI", "VisualInstance3D"),
    ("VisibleOnScreenNotifier3D", "VisualInstance3D"),
    ("Label3D", "GeometryInstance3D"),
    ("CPUParticles3D", "GeometryInstance3D"),
];

/// **The acceptance test.** With the engine's classes registered, every moved
/// class reaches its new base AND still reaches `Node3D`.
#[test]
fn visual_instance3d_node_chain() {
    gdobject::class_db::clear_for_testing();
    gdobject::class_db::register_2d_classes();
    gdobject::class_db::register_3d_classes();

    for (class, base) in REPARENTED {
        assert!(
            gdobject::class_db::is_parent_class(class, base),
            "{class} must derive from {base}"
        );
        // ⚠️ The re-parent must not sever the tail: everything here is still a
        // Node3D, and still a Node, through the inserted base.
        assert!(
            gdobject::class_db::is_parent_class(class, "Node3D"),
            "{class} must still be a Node3D through {base}"
        );
        assert!(
            gdobject::class_db::is_parent_class(class, "Node"),
            "{class} must still be a Node through {base}"
        );
    }

    // Concrete light subclasses inherit the new base through Light3D.
    for light in ["DirectionalLight3D", "OmniLight3D", "SpotLight3D"] {
        assert!(
            gdobject::class_db::is_parent_class(light, "VisualInstance3D"),
            "{light} inherits VisualInstance3D through Light3D"
        );
    }

    // A `GeometryInstance3D` is a `VisualInstance3D`, so the two-step chain holds.
    assert!(
        gdobject::class_db::is_parent_class("Label3D", "VisualInstance3D"),
        "GeometryInstance3D derives from VisualInstance3D, so Label3D reaches it too"
    );
    // Negative: the two bases are not interchangeable.
    assert!(
        !gdobject::class_db::is_parent_class("Decal", "GeometryInstance3D"),
        "a Decal is a VisualInstance3D but NOT a GeometryInstance3D"
    );

    // The part a game observes: a live node answers the `is` check.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let light = tree
        .add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    let node = tree.get_node(light).expect("light node in the tree");
    assert!(
        node.is_class("VisualInstance3D"),
        "a scene DirectionalLight3D must satisfy `is VisualInstance3D`"
    );
    assert!(
        node.is_class("Node3D"),
        "…and still satisfy `is Node3D` after the re-parent"
    );
}

/// The 2D table registers `Label3D`/`VisibleOnScreenNotifier3D`, so on its own it
/// must also register the bases they now point at — otherwise their chains sever
/// at the first unregistered name for anyone who calls only `register_2d_classes`.
#[test]
fn two_d_entry_point_bootstraps_the_visual_bases() {
    gdobject::class_db::clear_for_testing();
    gdobject::class_db::register_2d_classes();

    assert!(
        gdobject::class_db::class_exists("VisualInstance3D"),
        "register_2d_classes must bootstrap VisualInstance3D — it registers \
         VisibleOnScreenNotifier3D, which now derives from it"
    );
    assert!(
        gdobject::class_db::class_exists("GeometryInstance3D"),
        "register_2d_classes must bootstrap GeometryInstance3D for Label3D"
    );
    assert!(
        gdobject::class_db::is_parent_class("Label3D", "GeometryInstance3D"),
        "Label3D's chain resolves with only the 2D table registered"
    );
    assert!(
        gdobject::class_db::is_parent_class("VisibleOnScreenNotifier3D", "VisualInstance3D"),
        "VisibleOnScreenNotifier3D's chain resolves with only the 2D table registered"
    );

    // Registering the 3D table afterwards must not lose the full definitions:
    // the bootstraps above are bare, and the 3D registrations carry the real
    // property sets.
    gdobject::class_db::register_3d_classes();
    assert!(
        gdobject::class_db::class_has_property("VisualInstance3D", "layers"),
        "the 3D table's full VisualInstance3D (with `layers`) must win over the \
         bare 2D bootstrap"
    );
    assert!(
        gdobject::class_db::class_has_property("Label3D", "material_override"),
        "Label3D inherits GeometryInstance3D's material_override once 3D is registered"
    );
}
