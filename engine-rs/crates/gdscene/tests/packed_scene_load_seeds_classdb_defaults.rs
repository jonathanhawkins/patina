//! pat-0q2kx.210 — loading a `.tscn` seeds ClassDB class defaults before the
//! scene's stored overrides.
//!
//! Godot ground truth: instantiating a PackedScene creates each node through
//! ClassDB, which pre-fills every member with its class default; the scene's
//! stored values then override a subset. A member the scene text omits still
//! reads its class default, never null
//! (`ClassDB.class_get_property_default_value`).
//!
//! Patina built scene nodes with `Node::new` + stored-only `set_property`, so an
//! omitted member read back `Nil`. `SceneTree::create_node` already seeded
//! defaults, so the same class loaded from a file and created at runtime
//! disagreed — `scale` was `(1, 1)` on one and `Nil` on the other.
//!
//! ⚠️ These tests call `register_2d_classes()` first on purpose. ClassDB is only
//! populated by an explicit `register_*_classes()`, and the seeding is a no-op
//! against an empty DB — without the registration these tests would pass
//! vacuously no matter what the loader does.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn find(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no node named {name}"))
}

/// A Node2D that stores **only** `position`. `scale` and `visible` are absent
/// from the text, which is the whole point — an exported Godot scene omits
/// every member still at its default.
const SCENE: &str = r#"[gd_scene load_steps=1 format=3]

[node name="Root" type="Node2D"]
position = Vector2(10, 20)

[node name="Child" type="Node2D" parent="."]
position = Vector2(30, 40)
"#;

/// **The acceptance test.** Unset members resolve to their ClassDB defaults and
/// stored members override them, on the plain `instance()` path.
#[test]
fn packed_scene_load_seeds_classdb_defaults() {
    gdobject::class_db::register_2d_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let nodes = scene.instance().expect("scene instances");
    assert_eq!(nodes.len(), 2, "fixture has a root and one child");

    for node in &nodes {
        let who = node.name();

        // The clause: `scale` is absent from the scene text, so it must come
        // from Node2D's ClassDB default.
        let scale = node.get_property("scale");
        assert!(
            !scale.is_nil(),
            "{who}: `scale` is omitted by the scene, so it must be seeded from \
             ClassDB — got Nil, which means the defaults were never applied"
        );
        assert_eq!(
            scale,
            Variant::Vector2(Vector2::ONE),
            "{who}: Node2D's registered `scale` default is Vector2(1, 1)"
        );

        // `visible` comes from CanvasItem, one step up the chain, so this also
        // proves the walk covers ancestors and not just the leaf class.
        assert_eq!(
            node.get_property("visible"),
            Variant::Bool(true),
            "{who}: CanvasItem's registered `visible` default is true — an \
             ancestor's defaults must be seeded too"
        );
    }

    // Stored values still win over the seeded defaults. Node2D registers a
    // `position` default of its own, so this is the override half of the bead:
    // if seeding ran *after* the stored props, these would be the default.
    assert_eq!(
        nodes[0].get_property("position"),
        Variant::Vector2(Vector2::new(10.0, 20.0)),
        "the root's stored `position` must override the seeded default"
    );
    assert_eq!(
        nodes[1].get_property("position"),
        Variant::Vector2(Vector2::new(30.0, 40.0)),
        "the child's stored `position` must override the seeded default"
    );
}

/// The same guarantee through `add_packed_scene_to_tree` — the path the runtime
/// actually uses. It copies the property bag out of `instance()`, so this is the
/// end-to-end wiring requirement rather than a second copy of the logic.
#[test]
fn scene_added_to_tree_carries_classdb_defaults() {
    gdobject::class_db::register_2d_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let scene_root = find(&tree, "Root");
    let node = tree.get_node(scene_root).expect("node exists");

    assert_eq!(
        node.get_property("scale"),
        Variant::Vector2(Vector2::ONE),
        "a node loaded into the tree must carry Node2D's `scale` default"
    );
    assert_eq!(
        node.get_property("visible"),
        Variant::Bool(true),
        "a node loaded into the tree must carry CanvasItem's `visible` default"
    );
    assert_eq!(
        node.get_property("position"),
        Variant::Vector2(Vector2::new(10.0, 20.0)),
        "the stored `position` must survive the trip into the tree"
    );
}

/// The two build paths must agree: a node created at runtime through
/// `SceneTree::create_node` and the same class loaded from a scene file should
/// carry the same defaults. This is the regression that motivated the bead, so
/// it is asserted directly rather than inferred.
#[test]
fn loaded_and_runtime_created_nodes_agree_on_defaults() {
    gdobject::class_db::register_2d_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let loaded = scene.instance().expect("scene instances");

    let mut tree = SceneTree::new();
    let runtime = tree.create_node("Node2D", "Runtime");
    let runtime_node = tree.get_node(runtime).expect("created node exists");

    for prop in ["scale", "visible", "rotation"] {
        assert_eq!(
            loaded[0].get_property(prop),
            runtime_node.get_property(prop),
            "`{prop}` must match between a scene-loaded node and one from \
             create_node — the two paths seed from the same ClassDB walk"
        );
    }
}

/// A class ClassDB does not know must still load: seeding an unregistered class
/// contributes nothing rather than failing, and the stored values are untouched.
#[test]
fn unregistered_class_still_loads_stored_values() {
    let source = r#"[gd_scene load_steps=1 format=3]

[node name="Odd" type="TotallyNotAGodotClass"]
custom_thing = 7
"#;
    let scene = PackedScene::from_tscn(source).expect("scene parses");
    let nodes = scene.instance().expect("scene instances");

    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0].get_property("custom_thing"),
        Variant::Int(7),
        "an unregistered class seeds nothing but must still take its stored values"
    );
}

/// Guards the negative half of `Node::new` staying available: a bare node built
/// outside the scene loader is still unseeded, so the seeding is a property of
/// the *load path* and not something that silently moved into `Node::new`.
#[test]
fn bare_node_new_is_still_unseeded() {
    gdobject::class_db::register_2d_classes();

    let bare = Node::new("Bare", "Node2D");
    assert!(
        bare.get_property("scale").is_nil(),
        "Node::new must stay a plain constructor — if it began seeding defaults \
         itself, the loader tests above would pass for the wrong reason"
    );
}
