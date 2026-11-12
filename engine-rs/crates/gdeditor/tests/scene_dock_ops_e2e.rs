//! pat-0q2kx.170 — Scene dock ops: Save Branch as Scene, real NodePath copy,
//! and the script/group data the dock renders from.
//!
//! Godot's scene dock can extract a subtree into its own `.tscn` and replace the
//! branch with an instance of it (`scene_tree_dock.cpp`), copies a real
//! NodePath rather than an internal id, and filters with `g:<group>`.
//!
//! Two of the four audited gaps turned out to be **already closed** — see the
//! notes on each assertion below. The two that were real: there was no
//! save-branch route at all, and Copy Node Path copied the numeric node id.

use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::packed_scene::PackedScene;
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;

/// Root → Enemy → (Sprite2D, CollisionShape2D): a 3-node branch under a root.
fn scene_with_branch() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let enemy = tree.add_child(main, Node::new("Enemy", "Node2D")).unwrap();
    tree.add_child(enemy, Node::new("Sprite2D", "Sprite2D"))
        .unwrap();
    tree.add_child(enemy, Node::new("Hitbox", "CollisionShape2D"))
        .unwrap();
    (tree, main, enemy)
}

/// **The acceptance test.**
#[test]
fn scene_dock_ops_e2e() {
    let (tree, _main, enemy) = scene_with_branch();

    // -- Save Branch as Scene: the 3-node branch serializes on its own --------
    //
    // This is what `POST /api/node/save_branch` writes to disk. The branch root
    // becomes the root of the new scene, so the file stands alone.
    let saved = TscnSaver::save_subtree(&tree, enemy);
    assert!(
        saved.starts_with("[gd_scene"),
        "a branch must serialize as a standalone scene:\n{saved}"
    );
    for name in ["Enemy", "Sprite2D", "Hitbox"] {
        assert!(
            saved.contains(name),
            "the branch scene must contain `{name}`:\n{saved}"
        );
    }

    // -- and it loads back: a valid .tscn, not just plausible text ------------
    let reloaded = PackedScene::from_tscn(&saved).expect("the branch scene must parse");
    assert_eq!(
        reloaded.node_count(),
        3,
        "the round trip recovers all three branch nodes",
    );

    // -- the NodePath the dock copies ----------------------------------------
    //
    // Copy Node Path used to copy `String(selectedNodeId)` — the internal
    // numeric id, useless in a script. The /api/scene payload already carried a
    // real path from `tree.node_path()`; the fix is to copy that.
    let path = tree.node_path(enemy).expect("the branch has a node path");
    assert!(
        path.contains("Enemy"),
        "a Godot-style NodePath names the node, got {path:?}",
    );
    assert!(
        path.parse::<u64>().is_err(),
        "the copied path must not be a bare numeric id, got {path:?}",
    );
    assert!(
        EDITOR_HTML.contains("Copy Node Path"),
        "the dock still offers Copy Node Path",
    );
    assert!(
        EDITOR_HTML.contains("n.path") && EDITOR_HTML.contains("fsCopyToClipboard(np"),
        "Copy Node Path must copy the payload's NodePath, not the node id",
    );

    // -- ALREADY CLOSED, asserted so a regression is caught -------------------
    //
    // The audit said the filter was "plain substring" and the payload lacked
    // group data. Both were stale: `nodeMatchesFilter` already understands the
    // `group:`/`g:` prefix and matches against `node.groups`, which the scene
    // payload already emits.
    assert!(
        EDITOR_HTML.contains("(type|t|group|g):"),
        "the scene filter must understand the g: prefix",
    );
    assert!(
        EDITOR_HTML.contains("node.groups"),
        "the g: filter matches against the payload's groups array",
    );
}

/// The save-branch route is registered and reachable.
#[test]
fn save_branch_route_is_wired() {
    // The route table is the single place a handler becomes reachable; a
    // handler that exists but is unrouted is dead code that still compiles.
    let server_src = include_str!("../src/editor_server.rs");
    assert!(
        server_src.contains(r#"("POST", "/api/node/save_branch")"#),
        "POST /api/node/save_branch must be in the route table",
    );
    assert!(
        server_src.contains("fn api_save_branch_as_scene"),
        "the save_branch handler must exist",
    );
    // It must refuse to write outside the project.
    assert!(
        server_src.contains("path must stay within the project"),
        "save_branch must reject traversal in its target path",
    );
}
