//! pat-s3rrv.207 — a sub-resource whose class the engine does not know survives
//! a load/save round trip with its data intact.
//!
//! This is what makes a project safe to open in an editor that lacks a plugin's
//! custom `Resource` types, which is exactly Patina's situation against Godot.
//!
//! 🔑 **The failure it guards is silent and destructive.** `__resref__` already
//! records `class::id`, so the saver always wrote the correct
//! `[sub_resource type="MyPluginResource"]` header — it looked right. The BODY
//! was recovered through `companion_prefix_for`, an allowlist of four property
//! names, so anything outside it was written as a header with NO properties.
//! The scene still loaded, the section was still there, and the plugin's data
//! was gone. Opening and saving a project would quietly strip it.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// ⚠️ Property order is ALPHABETICAL within the sub-resource body on purpose.
/// The saver sorts keys so that re-saving an unchanged scene is a no-op, which
/// is pre-existing behaviour for every sub-resource, not something this bead
/// introduces. Authoring the fixture in the saver's order is what lets the
/// round-trip be compared as text at all.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="MyPluginResource" id="MyPluginResource_1"]
alpha = 3
beta = "hello"

[node name="Root" type="Node2D"]

[node name="Holder" type="Node2D" parent="."]
custom = SubResource("MyPluginResource_1")
"#;

fn load(src: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// The scene's OWN root, not the tree's.
///
/// ⚠️ `add_packed_scene_to_tree` attaches the scene under the tree's synthetic
/// root, so saving from `tree.root_id()` would emit that wrapper and add a
/// nesting level on every pass — which would fail the fixed-point test below
/// for a reason that has nothing to do with the resource body.
fn scene_root(tree: &SceneTree) -> gdscene::node::NodeId {
    named(tree, "Root")
}

fn named(tree: &SceneTree, want: &str) -> gdscene::node::NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == want)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no node named {want}"))
}

fn holder(tree: &SceneTree) -> gdscene::node::NodeId {
    named(tree, "Holder")
}

/// The unknown resource's body reaches the node verbatim.
///
/// Acceptance (2): the load records `original_class` and both property values.
#[test]
fn an_unknown_sub_resource_body_is_preserved_on_the_node() {
    let tree = load(SCENE);
    let node = tree.get_node(holder(&tree)).expect("holder exists");

    // `__resref__` carries the original class — this is what lets the saver
    // write `type="MyPluginResource"` for a class it cannot construct.
    assert!(
        matches!(
            node.get_property("__resref__custom"),
            Variant::String(ref s) if s.starts_with("MyPluginResource::")
        ),
        "the original class must be recorded; got {:?}",
        node.get_property("__resref__custom")
    );

    assert!(
        matches!(
            node.get_property("__subres__custom__alpha"),
            Variant::Int(3)
        ),
        "the body must be preserved verbatim; got {:?}",
        node.get_property("__subres__custom__alpha")
    );
    assert!(
        matches!(
            node.get_property("__subres__custom__beta"),
            Variant::String(ref s) if s == "hello"
        ),
        "every property, not just the first; got {:?}",
        node.get_property("__subres__custom__beta")
    );
}

/// Acceptance (5): the property that referenced the unknown resource is not
/// dropped, and the node is still there.
#[test]
fn the_referencing_property_survives_the_load() {
    let tree = load(SCENE);
    let node = tree.get_node(holder(&tree)).expect("holder exists");
    assert!(
        matches!(
            node.get_property("custom"),
            Variant::String(ref s) if s.contains("MyPluginResource_1")
        ),
        "the reference itself must be left intact, or the node loses its link \
         to the resource even once the body is preserved; got {:?}",
        node.get_property("custom")
    );
}

/// **Acceptance (3): the round trip.** Saving writes the original class back
/// with both properties.
#[test]
fn saving_writes_the_unknown_class_and_its_properties_back() {
    let tree = load(SCENE);
    let out = TscnSaver::save_tree(&tree, scene_root(&tree));

    assert!(
        out.contains(r#"[sub_resource type="MyPluginResource" id="MyPluginResource_1"]"#),
        "the section must come back under its ORIGINAL class name; got:\n{out}"
    );
    assert!(
        out.contains("alpha = 3"),
        "the first property must round-trip; got:\n{out}"
    );
    assert!(
        out.contains(r#"beta = "hello""#),
        "and the second — a header with an empty body is the exact bug this \
         bead names, and it is invisible without checking the body; got:\n{out}"
    );
}

/// ⚠️ **The control that makes the test above mean something.** The companion
/// keys are engine plumbing and must NOT be written into the `[node]` section.
///
/// 🔑 This is not hypothetical: `is_internal_prop`'s own comments record this
/// exact corruption happening twice before (`__tile_physics_*`,
/// `__tile_occluder_*`), and an audit for this bead found five more prefixes
/// unfiltered. A leaked companion produces a `.tscn` Godot cannot parse, so the
/// round trip is worse than lossy — it is destructive.
#[test]
fn companion_keys_never_leak_into_the_saved_scene() {
    let tree = load(SCENE);
    let out = TscnSaver::save_tree(&tree, scene_root(&tree));

    for leaked in ["__subres__", "__resref__"] {
        assert!(
            !out.contains(leaked),
            "`{leaked}` is engine plumbing and must not appear in the saved \
             scene — Godot cannot parse it as a property. got:\n{out}"
        );
    }
}

/// A second load of the saved text must produce the same body, which is the
/// property "round trip" actually means: stable under repetition, not merely
/// non-empty once.
#[test]
fn the_round_trip_is_stable_across_a_second_pass() {
    let first = load(SCENE);
    let text = TscnSaver::save_tree(&first, scene_root(&first));
    // Re-load what we just wrote and save it again.
    let second = load(&text);
    let again = TscnSaver::save_tree(&second, scene_root(&second));

    assert_eq!(
        text, again,
        "saving, re-loading and saving again must be a fixed point; a \
         difference means each pass mutates the scene and the data degrades \
         with every open/save cycle"
    );
}
