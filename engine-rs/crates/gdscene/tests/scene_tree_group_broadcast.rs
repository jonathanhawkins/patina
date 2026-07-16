//! pat-nzgdu.136: SceneTree group broadcasts.
//!
//! Godot's `SceneTree.call_group(group, method, ...args)` invokes a method on
//! every scripted member during a frame; `call_group_flags` honors DEFERRED
//! (run at the frame's deferred flush) and REVERSE (reverse tree order); plus
//! `get_first_node_in_group` / `get_node_count_in_group` / `has_group`.
//!
//! This builds a scene with two scripted members of a group, broadcasts to
//! them, and asserts the script method ran (its `hp` member syncs back onto the
//! node) — both immediately and via the deferred queue.

use gdscene::node::Node;
use gdscene::scene_tree::{SceneTree, GROUP_CALL_DEFERRED};
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// A member script: `hp` starts at 100 and `take_damage` subtracts from it.
const ENEMY_SCRIPT: &str = "var hp = 100\nfunc take_damage(amount):\n\thp -= amount\n";

fn hp_of(tree: &SceneTree, id: gdscene::NodeId) -> i64 {
    match tree.get_node(id).map(|n| n.get_property("hp")) {
        Some(Variant::Int(v)) => v,
        Some(Variant::Float(f)) => f as i64,
        other => panic!("hp should be an int property, got {other:?}"),
    }
}

fn add_scripted_enemy(tree: &mut SceneTree, parent: gdscene::NodeId, name: &str) -> gdscene::NodeId {
    let id = tree.add_child(parent, Node::new(name, "Node2D")).unwrap();
    let script = GDScriptNodeInstance::from_source(ENEMY_SCRIPT, id).unwrap();
    tree.attach_script(id, Box::new(script));
    tree.add_to_group(id, "enemies").unwrap();
    id
}

#[test]
fn scene_tree_group_broadcast() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let e1 = add_scripted_enemy(&mut tree, root, "Enemy1");
    let e2 = add_scripted_enemy(&mut tree, root, "Enemy2");
    // A non-member so we know the broadcast is group-scoped, not tree-wide.
    let _ally = tree.add_child(root, Node::new("Ally", "Node2D")).unwrap();

    // --- Group queries ---
    assert!(tree.has_group("enemies"));
    assert!(!tree.has_group("nonexistent"));
    assert_eq!(tree.get_node_count_in_group("enemies"), 2);
    assert_eq!(
        tree.get_first_node_in_group("enemies"),
        Some(e1),
        "first member should be the earliest in tree order"
    );

    // --- call_group (immediate): take_damage(10) on every scripted member ---
    tree.call_group("enemies", "take_damage", &[Variant::Int(10)]);
    assert_eq!(hp_of(&tree, e1), 90, "Enemy1 took 10 damage");
    assert_eq!(hp_of(&tree, e2), 90, "Enemy2 took 10 damage");

    // --- call_group_flags DEFERRED: queued, applied only at the flush ---
    tree.call_group_flags(GROUP_CALL_DEFERRED, "enemies", "take_damage", &[Variant::Int(5)]);
    assert_eq!(
        tree.pending_deferred_count(),
        2,
        "one deferred call queued per member"
    );
    assert_eq!(hp_of(&tree, e1), 90, "deferred call must not apply before the flush");

    tree.process_deferred_calls();
    assert_eq!(hp_of(&tree, e1), 85, "deferred take_damage(5) applied after flush");
    assert_eq!(hp_of(&tree, e2), 85);
}
