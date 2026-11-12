//! pat-s3rrv.17 item (6) — freeing a replication node drops its registry entry.
//!
//! `SceneTree::remove_node` cleaned the node arena, the weak-ref table and the
//! group index, and left the replication maps untouched. A freed
//! `MultiplayerSynchronizer` or `MultiplayerSpawner` kept its entry forever, so
//! a game that spawns and frees units continuously grew `synchronizers` without
//! bound and walked the dead entries on every outbound pass.
//!
//! ⚠️ Worth being precise about what this is and is not. `NodeId` wraps a
//! monotonic `AtomicU64` (`gdcore/src/id.rs:19`), so ids are never recycled and
//! a later node cannot inherit a dead synchronizer's configuration. This is a
//! leak and a growing per-frame cost, not a misconfiguration. Were ids pooled it
//! would be the latter — which is why the note is in the source too.
//!
//! The registry is private, so the observable used here is
//! `synchronizer_add_visibility_filter`, which answers `false` for a node that is
//! not a registered synchronizer. Before the fix it answered **true** for a node
//! that had already been freed — the registry claiming a dead node is a live
//! synchronizer is the leak, stated in the API's own terms.

use gdcore::id::ObjectId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::{CallableRef, Variant};

const WORLD: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Player" type="Node2D" parent="."]
position = Vector2(0, 0)

[node name="Sync" type="MultiplayerSynchronizer" parent="Player"]
root_path = NodePath("..")
replication_interval = 0.0
_sync_properties = ["position"]
"#;

fn a_callable() -> Variant {
    Variant::Callable(Box::new(CallableRef::Method {
        target_id: ObjectId::from_raw(4242).raw(),
        method: "_deny".to_string(),
    }))
}

#[test]
fn freeing_a_synchronizer_drops_its_registry_entry() {
    let scene = PackedScene::from_tscn(WORLD).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let sync = tree
        .get_node_by_path("/root/Main/Player/Sync")
        .expect("the synchronizer is in the tree");

    // A frame so the tree's own scan registers it.
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert!(
        ml.tree_mut()
            .synchronizer_add_visibility_filter(sync, a_callable()),
        "the synchronizer must be registered before removal, or this test \
         proves nothing about the cleanup",
    );

    ml.tree_mut()
        .remove_node(sync)
        .expect("the node is removed");

    assert!(
        !ml.tree_mut()
            .synchronizer_add_visibility_filter(sync, a_callable()),
        "a freed synchronizer must leave no registry entry — `true` here means \
         the registry still claims a node that no longer exists, and the \
         outbound pass keeps walking it every tick",
    );
}

/// Removing the whole subtree cleans the synchronizer nested inside it.
///
/// ⚠️ The node a game frees is the unit (`Player`), not the synchronizer child.
/// Cleaning only the removed node itself would leave every nested synchronizer
/// behind and fix nothing in practice — `remove_node` collects the subtree, and
/// the cleanup has to run for each collected id rather than once for the root.
#[test]
fn freeing_a_parent_cleans_the_synchronizer_nested_inside_it() {
    let scene = PackedScene::from_tscn(WORLD).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let player = tree
        .get_node_by_path("/root/Main/Player")
        .expect("the player is in the tree");
    let sync = tree
        .get_node_by_path("/root/Main/Player/Sync")
        .expect("the synchronizer is in the tree");

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    assert!(ml
        .tree_mut()
        .synchronizer_add_visibility_filter(sync, a_callable()));

    // Free the PARENT — what a game actually does.
    ml.tree_mut()
        .remove_node(player)
        .expect("the player is removed");

    assert!(
        !ml.tree_mut()
            .synchronizer_add_visibility_filter(sync, a_callable()),
        "the nested synchronizer must be cleaned too; cleaning only the removed \
         root would leave every one of them behind",
    );
}
