//! pat-nzgdu.194 — the 3D gizmo class surface.
//!
//! Covers the bead's "register in class_db.rs" clause. Its **wiring
//! requirement** — a registered gizmo drawing handles in the served 3D viewport,
//! and dragging one mutating the node — is NOT done; that routes through
//! `gdeditor/src/viewport_3d.rs`. See the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_editor_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

#[test]
fn gizmo_chain_is_registered() {
    register_editor_classes();

    assert!(class_exists("Node3DGizmo"));
    assert!(
        is_parent_class("Node3DGizmo", "RefCounted"),
        "the base is a RefCounted a Node3D can hold"
    );
    assert!(class_exists("EditorNode3DGizmo"));
    assert!(
        is_parent_class("EditorNode3DGizmo", "Node3DGizmo"),
        "the concrete gizmo derives the abstract base"
    );
    assert!(is_parent_class("EditorNode3DGizmo", "RefCounted"));
    assert!(is_parent_class("EditorNode3DGizmo", "Object"));

    assert!(class_exists("EditorNode3DGizmoPlugin"));
    // ⚠️ A Resource, NOT a RefCounted — the editor keeps the plugin, unlike the
    // gizmos it mints.
    assert!(
        is_parent_class("EditorNode3DGizmoPlugin", "Resource"),
        "the plugin is a Resource the editor retains"
    );
    assert!(
        !is_parent_class("EditorNode3DGizmoPlugin", "Node3DGizmo"),
        "the plugin is not itself a gizmo — it creates them"
    );
}

/// ⚠️ `Node3DGizmo` is ABSTRACT and declares **nothing at all**.
///
/// It exists only so `Node3D` can hold a gizmo reference without the scene tree
/// depending on the editor. The bead's description attributes `add_lines` /
/// `add_handles` / `redraw` to it — those live on `EditorNode3DGizmo`.
/// Registering them here would let a script call methods Godot's Node3DGizmo
/// does not have.
#[test]
fn the_base_is_abstract_and_declares_nothing() {
    register_editor_classes();

    assert!(
        is_abstract("Node3DGizmo"),
        "Godot reports is_instantiable=false"
    );
    assert!(!can_instantiate("Node3DGizmo"));

    let base_methods: Vec<String> = get_method_list("RefCounted")
        .into_iter()
        .map(|m| m.name)
        .collect();
    let own: Vec<String> = get_method_list("Node3DGizmo")
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect();
    assert!(
        own.is_empty(),
        "Node3DGizmo declares no methods in Godot; found {own:?}"
    );
    assert!(props("Node3DGizmo")
        .iter()
        .all(|p| props("RefCounted").contains(p)));

    // The surface the bead names really is on the SUBCLASS.
    for m in ["add_lines", "add_handles", "_redraw"] {
        assert!(
            class_has_method("EditorNode3DGizmo", m),
            "{m} lives on EditorNode3DGizmo"
        );
    }

    // ...and the subclass really is instantiable, so the contrast is not vacuous.
    assert!(!is_abstract("EditorNode3DGizmo"));
    assert!(can_instantiate("EditorNode3DGizmo"));
    let obj = instantiate("EditorNode3DGizmo").expect("instantiates");
    assert_eq!(obj.get_class(), "EditorNode3DGizmo");
}

/// What a gizmo draws, and — separately — what a click can hit. Godot keeps
/// those apart: collision geometry is not the drawn geometry.
#[test]
fn gizmo_separates_drawn_geometry_from_pickable_geometry() {
    register_editor_classes();

    assert_eq!(arity("EditorNode3DGizmo", "add_lines"), 2);
    assert_eq!(arity("EditorNode3DGizmo", "add_mesh"), 1);
    assert_eq!(arity("EditorNode3DGizmo", "add_unscaled_billboard"), 1);
    // ⚠️ Picking geometry is declared separately — a gizmo you can see is not
    // automatically a gizmo you can click.
    assert_eq!(arity("EditorNode3DGizmo", "add_collision_segments"), 1);
    assert_eq!(arity("EditorNode3DGizmo", "add_collision_triangles"), 1);

    // The draggable dots the bead's proof needs: (handles, material, ids).
    assert_eq!(
        arity("EditorNode3DGizmo", "add_handles"),
        3,
        "add_handles(handles, material, ids)"
    );
    assert_eq!(arity("EditorNode3DGizmo", "set_node_3d"), 1);
    assert_eq!(arity("EditorNode3DGizmo", "get_node_3d"), 0);
    assert_eq!(arity("EditorNode3DGizmo", "get_plugin"), 0);
}

/// The two virtuals that make a plugin a plugin: which nodes it claims, and the
/// gizmo it builds for one.
#[test]
fn plugin_declares_the_claim_and_create_hooks() {
    register_editor_classes();

    assert_eq!(arity("EditorNode3DGizmoPlugin", "_has_gizmo"), 1);
    assert_eq!(arity("EditorNode3DGizmoPlugin", "_create_gizmo"), 1);
    assert_eq!(arity("EditorNode3DGizmoPlugin", "_get_gizmo_name"), 0);
    // Priority breaks ties when two plugins claim the same node type.
    assert_eq!(arity("EditorNode3DGizmoPlugin", "_get_priority"), 0);
    assert_eq!(arity("EditorNode3DGizmoPlugin", "_can_be_hidden"), 0);
    assert_eq!(
        arity("EditorNode3DGizmoPlugin", "_is_selectable_when_hidden"),
        0
    );

    // Materials a gizmo draws with come from the plugin, not the gizmo.
    assert_eq!(arity("EditorNode3DGizmoPlugin", "create_material"), 2);
    assert_eq!(
        arity("EditorNode3DGizmoPlugin", "create_handle_material"),
        1
    );
    assert_eq!(arity("EditorNode3DGizmoPlugin", "add_material"), 2);
}

/// ⚠️ THE ARITY TRAP: every plugin hook takes the GIZMO as its first argument,
/// so each is exactly one higher than the gizmo's same-named virtual.
///
/// A plugin serves many gizmos; a gizmo serves one node. Copying the arities
/// across would compile and then be handed the wrong argument at runtime.
#[test]
fn plugin_hooks_take_one_more_argument_than_the_gizmos() {
    register_editor_classes();

    for (method, gizmo_arity, plugin_arity) in [
        ("_redraw", 0, 1),
        ("_get_handle_name", 2, 3),
        ("_is_handle_highlighted", 2, 3),
        ("_get_handle_value", 2, 3),
        ("_begin_handle_action", 2, 3),
        ("_set_handle", 4, 5),
        ("_commit_handle", 4, 5),
        ("_subgizmos_intersect_ray", 2, 3),
        ("_subgizmos_intersect_frustum", 2, 3),
        ("_set_subgizmo_transform", 2, 3),
        ("_commit_subgizmos", 3, 4),
    ] {
        assert_eq!(
            arity("EditorNode3DGizmo", method),
            gizmo_arity,
            "EditorNode3DGizmo.{method}"
        );
        assert_eq!(
            arity("EditorNode3DGizmoPlugin", method),
            plugin_arity,
            "EditorNode3DGizmoPlugin.{method} takes the gizmo first, so it is one higher"
        );
        assert_eq!(
            arity("EditorNode3DGizmoPlugin", method),
            arity("EditorNode3DGizmo", method) + 1,
            "{method}: the plugin hook must be exactly one arity higher"
        );
    }

    // The +1 rule holds for every hook, with no exceptions. This pair is pinned
    // separately only because it looks lopsided — get is 1/2 while set is 2/3 —
    // which invites "correcting" one to match the other.
    assert_eq!(arity("EditorNode3DGizmo", "_get_subgizmo_transform"), 1);
    assert_eq!(
        arity("EditorNode3DGizmoPlugin", "_get_subgizmo_transform"),
        2
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        [
            "Node3DGizmo",
            "EditorNode3DGizmo",
            "EditorNode3DGizmoPlugin",
        ]
        .iter()
        .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
        .collect()
    };

    register_editor_classes();
    let first = ids();
    register_editor_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
