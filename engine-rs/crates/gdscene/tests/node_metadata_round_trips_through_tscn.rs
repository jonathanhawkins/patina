//! pat-s3rrv.151: `metadata/<name>` must survive in BOTH directions.
//!
//! A node keeps metadata in `meta`, a different map from `properties`. A .tscn
//! spells metadata as an ordinary `metadata/foo = 42` property row, so the two
//! stores never met:
//!
//! - **reading**: a row from the file landed in `properties`, and `get_meta`
//!   reads `meta` — so `get_meta("foo")` answered Nil for metadata the scene
//!   plainly declared;
//! - **writing**: `set_meta` wrote `meta`, and the saver serialises
//!   `properties` — so editor- or script-authored metadata was dropped on save
//!   with no error.
//!
//! Each direction fails silently and independently, which is why this pins both
//! and then a full round trip.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]
metadata/from_file = 42
"#;

/// `add_packed_scene_to_tree` returns the instanced scene's OWN root, which is
/// the level the assertions want — the tree root is its parent.
fn instance(tscn: &str) -> (SceneTree, gdscene::node::NodeId) {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    (tree, scene_root)
}

/// Read direction.
#[test]
fn metadata_declared_in_a_tscn_is_readable_via_get_meta() {
    let (tree, scene_root) = instance(SCENE);
    let node = tree.get_node(scene_root).expect("scene root exists");

    assert_eq!(
        node.get_meta("from_file"),
        Variant::Int(42),
        "a `metadata/foo` row in the .tscn must be readable as get_meta(\"foo\") — \
         this is what a script's get_meta() call sees"
    );

    // The property row must ALSO still be there: existing callers and tests read
    // metadata back through `get_property`, so the mirror must add, not move.
    assert_eq!(
        node.get_property("metadata/from_file"),
        Variant::Int(42),
        "the property row must survive the mirror"
    );
}

/// Write direction.
#[test]
fn set_meta_reaches_the_saved_tscn() {
    let (mut tree, scene_root) = instance(SCENE);
    tree.get_node_mut(scene_root)
        .expect("scene root exists")
        .set_meta("runtime_tag", Variant::Int(7));

    let saved = TscnSaver::save_tree(&tree, scene_root);
    assert!(
        saved.contains("metadata/runtime_tag = 7"),
        "set_meta must be serialised as a metadata row; saved scene was:\n{saved}"
    );
}

/// ⚠️ The row that came from the file must appear EXACTLY once. The saver walks
/// properties and then meta, and both now hold `from_file` — a naive second pass
/// would emit it twice and the reloaded scene would still look correct, so only
/// counting catches it.
#[test]
fn a_file_declared_metadata_row_is_not_written_twice() {
    let (tree, scene_root) = instance(SCENE);
    let saved = TscnSaver::save_tree(&tree, scene_root);

    let count = saved.matches("metadata/from_file").count();
    assert_eq!(count, 1, "expected one row, found {count}:\n{saved}");
}

/// Both directions composed: save, reload, and the metadata is still readable
/// through the meta API rather than only as a property.
#[test]
fn metadata_survives_a_full_save_and_reload() {
    let (mut tree, scene_root) = instance(SCENE);
    tree.get_node_mut(scene_root)
        .expect("scene root exists")
        .set_meta("runtime_tag", Variant::Int(7));
    let saved = TscnSaver::save_tree(&tree, scene_root);

    let (tree2, root2) = instance(&saved);
    let node = tree2.get_node(root2).expect("reloaded root exists");

    assert_eq!(
        node.get_meta("from_file"),
        Variant::Int(42),
        "file-declared metadata must survive the round trip"
    );
    assert_eq!(
        node.get_meta("runtime_tag"),
        Variant::Int(7),
        "runtime-set metadata must survive the round trip — this is the clause \
         that was impossible before, since the value never reached the file"
    );
}
