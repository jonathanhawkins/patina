//! pat-nzgdu.86: Node query surface — wildcard find_child/find_children,
//! add_sibling ordering, find_parent, is_ancestor_of, and propagate_call.
//!
//! Godot's Node exposes these over the scene tree (node.cpp); Patina previously
//! had only exact-path resolution. This exercises the SceneTree-level API (the
//! GDScript bridge wraps these; that exposure is a separate follow-up).

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use std::collections::HashSet;

fn add(tree: &mut SceneTree, parent: NodeId, name: &str, class: &str) -> NodeId {
    tree.add_child(parent, Node::new(name, class)).unwrap()
}

#[test]
fn node_find_and_sibling_queries() {
    // root
    //  ├─ Player (Node2D)
    //  │    ├─ Sprite (Sprite2D)
    //  │    └─ Enemy1 (Node2D)
    //  │         └─ Body (Sprite2D)
    //  └─ Enemy2 (Node2D)
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player = add(&mut tree, root, "Player", "Node2D");
    let sprite = add(&mut tree, player, "Sprite", "Sprite2D");
    let enemy1 = add(&mut tree, player, "Enemy1", "Node2D");
    let body = add(&mut tree, enemy1, "Body", "Sprite2D");
    let enemy2 = add(&mut tree, root, "Enemy2", "Node2D");

    // find_children('*', 'Sprite2D', recursive) → every Sprite2D descendant.
    let sprites = tree.find_children(root, "*", Some("Sprite2D"), true);
    assert_eq!(sprites.len(), 2, "two Sprite2D descendants: {sprites:?}");
    assert!(sprites.contains(&sprite) && sprites.contains(&body));

    // The type filter excludes non-matching classes.
    assert!(
        tree.find_children(root, "*", Some("Camera3D"), true).is_empty(),
        "no Camera3D descendants"
    );

    // find_child wildcard: 'Ene*' matches an Enemy node (pre-order → Enemy1).
    let ene = tree.find_child(root, "Ene*", true).expect("wildcard match");
    assert!(
        tree.get_node(ene).unwrap().name().starts_with("Ene"),
        "matched a node whose name starts with Ene"
    );

    // '?' matches exactly one character.
    assert_eq!(
        tree.find_child(root, "Enemy?", true).map(|id| id),
        Some(enemy1),
        "Enemy? matches Enemy1 (single trailing char), pre-order first"
    );

    // Non-recursive find_child only inspects direct children.
    assert!(
        tree.find_child(root, "Sprite", false).is_none(),
        "Sprite is not a direct child of root"
    );
    assert!(
        tree.find_child(player, "Sprite", false).is_some(),
        "Sprite is a direct child of Player"
    );

    // find_parent walks up the ancestor chain.
    assert_eq!(tree.find_parent(body, "Play*"), Some(player));
    assert_eq!(tree.find_parent(body, "Nope*"), None);

    // is_ancestor_of.
    assert!(tree.is_ancestor_of(player, body));
    assert!(!tree.is_ancestor_of(enemy1, sprite));

    // add_sibling inserts immediately after the sibling in tree order.
    let shadow = tree
        .add_sibling(sprite, Node::new("Shadow", "Sprite2D"))
        .unwrap();
    let player_children = tree.get_node(player).unwrap().children().to_vec();
    let sprite_pos = player_children
        .iter()
        .position(|&c| c == sprite)
        .expect("Sprite still under Player");
    assert_eq!(
        player_children.get(sprite_pos + 1),
        Some(&shadow),
        "Shadow inserted immediately after Sprite: {player_children:?}"
    );

    // propagate_call (parent_first) reaches root and every descendant.
    let mut visited: HashSet<NodeId> = HashSet::new();
    tree.propagate_call(root, true, |n| {
        visited.insert(n.id());
    });
    for id in [root, player, sprite, shadow, enemy1, body, enemy2] {
        assert!(visited.contains(&id), "propagate_call must reach {id:?}");
    }
}
