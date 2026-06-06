//! pat-j3rq (originally pat-49m0): NodePath generic NodeId resolver parity coverage.
//!
//! Broadens NodePath parity by systematically verifying that NodeIds obtained
//! from every public SceneTree API work as generic resolver handles across all
//! path resolution methods. Tests focus on:
//!
//! 1. node_path() → get_node_by_path() roundtrip identity
//! 2. collect_subtree_*/all_nodes_in_tree_order IDs as resolver handles
//! 3. get_nodes_in_group IDs → relative/absolute resolution
//! 4. get_index consistency with resolver-returned NodeIds
//! 5. Packed scene instancing: NodeIds are instance-isolated
//! 6. Cross-API matrix: every source × every resolver method
//! 7. Exclusions documented: property subnames, subname-only paths
//! 8. Children-returned NodeIds as resolver handles
//! 9. Parent-returned NodeIds as resolver handles
//! 10. move_child NodeId stability after reordering
//! 11. u64 script-access roundtrip through children/parent APIs
//!
//! Acceptance: focused tests describe supported resolution cases and
//! document any remaining exclusions.

use gdcore::ObjectId;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Builds a rich test tree:
///   root
///   ├── Players (Node)
///   │   ├── Hero (Node2D) [unique, group="chars"]
///   │   │   ├── Sprite (Sprite2D)
///   │   │   └── Collider (CollisionShape2D)
///   │   └── Ally (Node2D) [group="chars"]
///   ├── World (Node)
///   │   ├── Ground (StaticBody2D) [group="env"]
///   │   └── Platform (StaticBody2D) [group="env"]
///   └── UI (Control)
///       └── Score (Label) [unique]
struct TestTree {
    tree: SceneTree,
    root: NodeId,
    players: NodeId,
    hero: NodeId,
    sprite: NodeId,
    collider: NodeId,
    ally: NodeId,
    world: NodeId,
    ground: NodeId,
    platform: NodeId,
    ui: NodeId,
    score: NodeId,
}

fn build_test_tree() -> TestTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Helper: create a node owned by root (mimics add_packed_scene_to_tree).
    let owned_node = |name: &str, type_name: &str| -> Node {
        let mut n = Node::new(name, type_name);
        n.set_owner(Some(root));
        n
    };

    let players = tree.add_child(root, owned_node("Players", "Node")).unwrap();

    let mut hero_node = owned_node("Hero", "Node2D");
    hero_node.set_unique_name(true);
    let hero = tree.add_child(players, hero_node).unwrap();

    let sprite = tree
        .add_child(hero, owned_node("Sprite", "Sprite2D"))
        .unwrap();
    let collider = tree
        .add_child(hero, owned_node("Collider", "CollisionShape2D"))
        .unwrap();

    let ally = tree
        .add_child(players, owned_node("Ally", "Node2D"))
        .unwrap();

    let world = tree.add_child(root, owned_node("World", "Node")).unwrap();
    let ground = tree
        .add_child(world, owned_node("Ground", "StaticBody2D"))
        .unwrap();
    let platform = tree
        .add_child(world, owned_node("Platform", "StaticBody2D"))
        .unwrap();

    let ui = tree.add_child(root, owned_node("UI", "Control")).unwrap();

    let mut score_node = owned_node("Score", "Label");
    score_node.set_unique_name(true);
    let score = tree.add_child(ui, score_node).unwrap();

    // Groups
    tree.add_to_group(hero, "chars").unwrap();
    tree.add_to_group(ally, "chars").unwrap();
    tree.add_to_group(ground, "env").unwrap();
    tree.add_to_group(platform, "env").unwrap();

    TestTree {
        tree,
        root,
        players,
        hero,
        sprite,
        collider,
        ally,
        world,
        ground,
        platform,
        ui,
        score,
    }
}

// ===========================================================================
// 1. node_path() → get_node_by_path() roundtrip identity
// ===========================================================================

#[test]
fn node_path_roundtrip_for_every_node() {
    let t = build_test_tree();

    // For every node in the tree, node_path → get_node_by_path should return the same NodeId.
    let all = t.tree.all_nodes_in_tree_order();
    for &id in &all {
        let path = t.tree.node_path(id).expect("node_path should succeed");
        let resolved = t
            .tree
            .get_node_by_path(&path)
            .unwrap_or_else(|| panic!("get_node_by_path({path}) should resolve"));
        assert_eq!(
            id, resolved,
            "Roundtrip failed for path {path}: original={id:?}, resolved={resolved:?}"
        );
    }
}

#[test]
fn node_path_roundtrip_after_reparent() {
    let mut t = build_test_tree();

    // Reparent Ally under World
    t.tree.reparent(t.ally, t.world).unwrap();

    let path = t.tree.node_path(t.ally).unwrap();
    assert_eq!(path, "/root/World/Ally");
    let resolved = t.tree.get_node_by_path(&path).unwrap();
    assert_eq!(t.ally, resolved);
}

// ===========================================================================
// 2. collect_subtree / all_nodes_in_tree_order IDs as resolver handles
// ===========================================================================

#[test]
fn subtree_ids_resolve_relative_paths() {
    let t = build_test_tree();

    let mut subtree = Vec::new();
    t.tree.collect_subtree_top_down(t.players, &mut subtree);

    // subtree should be [Players, Hero, Sprite, Collider, Ally]
    assert_eq!(subtree.len(), 5);

    // Each ID should work as a from-handle for get_node_relative
    // Players → Hero via "Hero"
    assert_eq!(t.tree.get_node_relative(subtree[0], "Hero"), Some(t.hero));
    // Hero → Sprite via "Sprite"
    assert_eq!(
        t.tree.get_node_relative(subtree[1], "Sprite"),
        Some(t.sprite)
    );
    // Sprite → Hero via ".."
    assert_eq!(t.tree.get_node_relative(subtree[2], ".."), Some(t.hero));
    // Collider → Sprite via "../Sprite"
    assert_eq!(
        t.tree.get_node_relative(subtree[3], "../Sprite"),
        Some(t.sprite)
    );
}

#[test]
fn all_nodes_in_tree_order_ids_resolve_absolute_paths() {
    let t = build_test_tree();
    let all = t.tree.all_nodes_in_tree_order();

    // Every ID from the traversal should be reachable via node_path → get_node_by_path
    for &id in &all {
        let path = t.tree.node_path(id).unwrap();
        assert_eq!(t.tree.get_node_by_path(&path), Some(id));
    }

    // Total should be 11 nodes
    assert_eq!(all.len(), 11);
}

#[test]
fn bottom_up_subtree_ids_match_top_down_set() {
    let t = build_test_tree();

    let mut top_down = Vec::new();
    t.tree.collect_subtree_top_down(t.world, &mut top_down);

    let mut bottom_up = Vec::new();
    t.tree.collect_subtree_bottom_up(t.world, &mut bottom_up);

    // Same set of IDs, different order
    let mut td_sorted = top_down.clone();
    td_sorted.sort_by_key(|id| id.raw());
    let mut bu_sorted = bottom_up.clone();
    bu_sorted.sort_by_key(|id| id.raw());
    assert_eq!(td_sorted, bu_sorted);

    // Each bottom-up ID still resolves correctly
    for &id in &bottom_up {
        let path = t.tree.node_path(id).unwrap();
        assert_eq!(t.tree.get_node_by_path(&path), Some(id));
    }
}

// ===========================================================================
// 3. get_nodes_in_group IDs as resolver handles
// ===========================================================================

#[test]
fn group_ids_resolve_to_correct_nodes() {
    let t = build_test_tree();

    let chars = t.tree.get_nodes_in_group("chars");
    assert_eq!(chars.len(), 2);

    // Both NodeIds from the group should be valid resolver handles
    for &id in &chars {
        let node = t.tree.get_node(id).unwrap();
        let name = node.name();
        assert!(
            name == "Hero" || name == "Ally",
            "Unexpected node in 'chars' group: {name}"
        );

        // Each should resolve its own path roundtrip
        let path = t.tree.node_path(id).unwrap();
        assert_eq!(t.tree.get_node_by_path(&path), Some(id));
    }
}

#[test]
fn group_ids_as_from_handles_for_relative_resolution() {
    let t = build_test_tree();

    let env = t.tree.get_nodes_in_group("env");
    assert_eq!(env.len(), 2);

    // Both Ground and Platform should resolve ".." to World
    for &id in &env {
        assert_eq!(
            t.tree.get_node_relative(id, ".."),
            Some(t.world),
            "env group member should have World as parent"
        );
    }

    // From Ground, resolve "../Platform"
    assert_eq!(
        t.tree.get_node_relative(t.ground, "../Platform"),
        Some(t.platform)
    );
}

#[test]
fn group_ids_work_with_get_node_or_null() {
    let t = build_test_tree();

    let chars = t.tree.get_nodes_in_group("chars");
    for &id in &chars {
        // Absolute path resolution
        assert_eq!(t.tree.get_node_or_null(id, "/root/UI/Score"), Some(t.score));
        // Relative parent
        let parent = t.tree.get_node_or_null(id, "..");
        assert!(parent.is_some());
    }
}

// ===========================================================================
// 4. get_index consistency with resolver-returned NodeIds
// ===========================================================================

#[test]
fn get_index_matches_child_position() {
    let t = build_test_tree();

    // Hero is first child of Players, Ally is second
    assert_eq!(t.tree.get_index(t.hero), Some(0));
    assert_eq!(t.tree.get_index(t.ally), Some(1));

    // Root children: Players=0, World=1, UI=2
    assert_eq!(t.tree.get_index(t.players), Some(0));
    assert_eq!(t.tree.get_index(t.world), Some(1));
    assert_eq!(t.tree.get_index(t.ui), Some(2));
}

#[test]
fn get_index_consistent_after_resolver_lookup() {
    let t = build_test_tree();

    // Get Hero via path, then check its index
    let hero_via_path = t.tree.get_node_by_path("/root/Players/Hero").unwrap();
    assert_eq!(t.tree.get_index(hero_via_path), Some(0));

    // Get via relative
    let hero_via_rel = t.tree.get_node_relative(t.players, "Hero").unwrap();
    assert_eq!(t.tree.get_index(hero_via_rel), Some(0));

    // Get via unique name
    let hero_via_unique = t.tree.get_node_relative(t.root, "%Hero").unwrap();
    assert_eq!(t.tree.get_index(hero_via_unique), Some(0));

    // All three should be the same NodeId
    assert_eq!(hero_via_path, hero_via_rel);
    assert_eq!(hero_via_rel, hero_via_unique);
}

// ===========================================================================
// 5. Packed scene instancing: NodeIds are instance-isolated
// ===========================================================================

#[test]
fn packed_scene_instances_have_distinct_nodeids() {
    let tscn = r#"[gd_scene load_steps=1 format=3]

[node name="Enemy" type="Node2D"]

[node name="AI" type="Node" parent="."]

[node name="Sprite" type="Sprite2D" parent="."]
"#;

    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    // Rename to avoid name collision
    tree.get_node_mut(inst1).unwrap().set_name("Enemy1");

    let inst2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    tree.get_node_mut(inst2).unwrap().set_name("Enemy2");

    // Distinct NodeIds
    assert_ne!(inst1, inst2);

    // Both resolve correctly via their own paths
    assert_eq!(tree.get_node_by_path("/root/Enemy1"), Some(inst1));
    assert_eq!(tree.get_node_by_path("/root/Enemy2"), Some(inst2));

    // Children are also distinct
    let ai1 = tree.get_node_relative(inst1, "AI").unwrap();
    let ai2 = tree.get_node_relative(inst2, "AI").unwrap();
    assert_ne!(ai1, ai2);

    // Each AI resolves ".." to its own parent
    assert_eq!(tree.get_node_relative(ai1, ".."), Some(inst1));
    assert_eq!(tree.get_node_relative(ai2, ".."), Some(inst2));
}

#[test]
fn packed_scene_instance_ids_work_as_generic_handles() {
    let tscn = r#"[gd_scene load_steps=1 format=3]

[node name="Item" type="Node2D"]

[node name="Visual" type="Sprite2D" parent="."]
"#;

    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Get the instance root via path, then use it to resolve children
    let item = tree.get_node_by_path("/root/Item").unwrap();
    let visual = tree.get_node_relative(item, "Visual").unwrap();

    // Visual's path roundtrip
    let visual_path = tree.node_path(visual).unwrap();
    assert_eq!(visual_path, "/root/Item/Visual");
    assert_eq!(tree.get_node_by_path(&visual_path), Some(visual));

    // Use get_node_or_null with instance NodeId as from
    assert_eq!(tree.get_node_or_null(item, "Visual"), Some(visual));
    assert_eq!(tree.get_node_or_null(visual, ".."), Some(item));
}

// ===========================================================================
// 6. Cross-API matrix: every NodeId source × every resolver method
// ===========================================================================

#[test]
fn cross_api_matrix_all_sources_resolve_via_all_methods() {
    let t = build_test_tree();

    // Collect NodeIds from every public API:
    // Source 1: add_child (stored in struct)
    let id_from_add = t.hero;

    // Source 2: get_node_by_path
    let id_from_path = t.tree.get_node_by_path("/root/Players/Hero").unwrap();

    // Source 3: get_node_relative
    let id_from_rel = t.tree.get_node_relative(t.players, "Hero").unwrap();

    // Source 4: get_node_or_null (absolute)
    let id_from_abs_null = t
        .tree
        .get_node_or_null(t.root, "/root/Players/Hero")
        .unwrap();

    // Source 5: get_node_or_null (relative)
    let id_from_rel_null = t.tree.get_node_or_null(t.players, "Hero").unwrap();

    // Source 6: get_node_by_unique_name
    let id_from_unique = t.tree.get_node_by_unique_name(t.root, "Hero").unwrap();

    // Source 7: get_nodes_in_group (find Hero in chars)
    let chars = t.tree.get_nodes_in_group("chars");
    let id_from_group = chars
        .iter()
        .find(|&&id| t.tree.get_node(id).unwrap().name() == "Hero")
        .copied()
        .unwrap();

    // Source 8: all_nodes_in_tree_order (find Hero)
    let all = t.tree.all_nodes_in_tree_order();
    let id_from_traversal = all
        .iter()
        .find(|&&id| t.tree.get_node(id).unwrap().name() == "Hero")
        .copied()
        .unwrap();

    // All sources should yield the same NodeId
    let sources = [
        id_from_add,
        id_from_path,
        id_from_rel,
        id_from_abs_null,
        id_from_rel_null,
        id_from_unique,
        id_from_group,
        id_from_traversal,
    ];
    for (i, &src) in sources.iter().enumerate() {
        assert_eq!(
            src, t.hero,
            "Source {i} yielded different NodeId than expected"
        );
    }

    // Now verify that each source NodeId works with every resolver method:
    for &id in &sources {
        // Method 1: get_node_relative
        assert_eq!(t.tree.get_node_relative(id, "Sprite"), Some(t.sprite));
        assert_eq!(t.tree.get_node_relative(id, ".."), Some(t.players));

        // Method 2: get_node_or_null (absolute)
        assert_eq!(t.tree.get_node_or_null(id, "/root/World"), Some(t.world));

        // Method 3: get_node_or_null (relative)
        assert_eq!(t.tree.get_node_or_null(id, "Collider"), Some(t.collider));

        // Method 4: node_path
        assert_eq!(t.tree.node_path(id), Some("/root/Players/Hero".to_string()));

        // Method 5: get_index
        assert_eq!(t.tree.get_index(id), Some(0));

        // Method 6: get_node (data access)
        assert_eq!(t.tree.get_node(id).unwrap().name(), "Hero");
        assert_eq!(t.tree.get_node(id).unwrap().class_name(), "Node2D");
    }
}

// ===========================================================================
// 7. Unique name resolution from different starting points
// ===========================================================================

#[test]
fn unique_name_resolves_from_any_node_in_same_scene() {
    let t = build_test_tree();

    // %Hero should resolve from any node whose owner scope includes root.
    // Nodes with owner=root can see unique names in the root's subtree.
    // Nodes without an explicit owner use themselves as scope root, so they
    // only see unique names in their own subtree. We test nodes that have
    // root as owner (hero, score) and root itself (which has no owner, so
    // scope = root → covers entire tree).
    let from_nodes = [t.root, t.hero, t.score];

    for &from in &from_nodes {
        let result = t.tree.get_node_relative(from, "%Hero");
        assert_eq!(
            result,
            Some(t.hero),
            "%Hero should resolve from {:?} ({})",
            from,
            t.tree.node_path(from).unwrap_or_default()
        );
    }
}

#[test]
fn unique_name_with_child_path_resolves() {
    let t = build_test_tree();

    // %Hero/Sprite should resolve to the Sprite node
    assert_eq!(
        t.tree.get_node_relative(t.root, "%Hero/Sprite"),
        Some(t.sprite)
    );

    // %Hero/Collider
    assert_eq!(
        t.tree.get_node_relative(t.root, "%Hero/Collider"),
        Some(t.collider)
    );

    // %Score (unique in UI subtree)
    assert_eq!(t.tree.get_node_relative(t.root, "%Score"), Some(t.score));
}

// ===========================================================================
// 8. u64 roundtrip preserves resolver capability
// ===========================================================================

#[test]
fn nodeid_u64_roundtrip_preserves_resolution() {
    let t = build_test_tree();

    // Roundtrip Hero's NodeId through u64
    let raw = t.hero.raw();
    let reconstructed = NodeId::from_object_id(ObjectId::from_raw(raw));
    assert_eq!(reconstructed, t.hero);

    // Reconstructed ID works in all resolver methods
    assert_eq!(
        t.tree.get_node_relative(reconstructed, "Sprite"),
        Some(t.sprite)
    );
    assert_eq!(
        t.tree.node_path(reconstructed),
        Some("/root/Players/Hero".to_string())
    );
    assert_eq!(t.tree.get_index(reconstructed), Some(0));
}

#[test]
fn stale_nodeid_returns_none_gracefully() {
    let mut t = build_test_tree();

    // Remove Ground from tree
    t.tree.remove_node(t.ground).unwrap();

    // Stale NodeId should return None from node-data and relative resolvers.
    // Note: get_node_or_null with an *absolute* path delegates to get_node_by_path
    // which walks from root — it ignores the `from` argument, so it still works.
    assert_eq!(t.tree.node_path(t.ground), None);
    assert_eq!(t.tree.get_node_relative(t.ground, ".."), None);
    assert_eq!(t.tree.get_node_or_null(t.ground, ".."), None);
    assert_eq!(t.tree.get_index(t.ground), None);
    assert!(t.tree.get_node(t.ground).is_none());
}

// ===========================================================================
// 9. Edge cases & exclusions
// ===========================================================================

#[test]
fn empty_path_returns_self() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_relative(t.hero, ""), Some(t.hero));
    assert_eq!(t.tree.get_node_relative(t.root, ""), Some(t.root));
}

#[test]
fn dot_path_returns_self() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_relative(t.sprite, "."), Some(t.sprite));
}

#[test]
fn parent_of_root_returns_none() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_relative(t.root, ".."), None);
}

#[test]
fn nonexistent_child_returns_none() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_relative(t.hero, "Ghost"), None);
    assert_eq!(t.tree.get_node_by_path("/root/Ghost"), None);
}

#[test]
fn nonexistent_unique_name_returns_none() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_relative(t.root, "%Ghost"), None);
}

#[test]
fn deep_parent_traversal_stops_at_root() {
    let t = build_test_tree();
    // Sprite → Hero → Players → root → None (no parent)
    assert_eq!(t.tree.get_node_relative(t.sprite, "../../.."), Some(t.root));
    assert_eq!(t.tree.get_node_relative(t.sprite, "../../../.."), None);
}

#[test]
fn absolute_path_ignores_from_node() {
    let t = build_test_tree();
    // get_node_or_null with absolute path ignores `from`
    assert_eq!(
        t.tree.get_node_or_null(t.sprite, "/root/UI/Score"),
        Some(t.score)
    );
    assert_eq!(
        t.tree.get_node_or_null(t.ground, "/root/Players/Hero"),
        Some(t.hero)
    );
}

// ===========================================================================
// 10. Documented exclusions
// ===========================================================================

/// Documents behaviors that are NOT supported and should return None or be
/// handled at a higher layer (e.g., script interpreter).
///
/// These exclusions are intentional parity gaps documented here for reference:
///
/// - **Property subnames** (`Player:position:x`): The NodePath struct parses
///   these, but the SceneTree resolver does not interpret subnames. Property
///   access is handled by the script interpreter layer, not the path resolver.
///
/// - **Subname-only paths** (`:prop:sub`): Parsed as having no node names
///   and subnames only. The resolver returns the `from` node (empty path).
///
/// - **Trailing slashes** (`/root/Players/`): The resolver splits on `/`,
///   producing an empty final segment which fails to match any child name.
///   Godot also does not support trailing slashes.
///
/// - **Double slashes** (`/root//Players`): Produces empty intermediate
///   segments that fail to match. Not supported.
#[test]
fn exclusion_trailing_slash_fails() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_by_path("/root/Players/"), None);
}

#[test]
fn exclusion_double_slash_fails() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_by_path("/root//Players"), None);
}

#[test]
fn exclusion_relative_path_in_get_node_by_path_fails() {
    let t = build_test_tree();
    // get_node_by_path requires leading '/'
    assert_eq!(t.tree.get_node_by_path("Players/Hero"), None);
}

// ===========================================================================
// 11. Children-returned NodeIds as resolver handles
// ===========================================================================

#[test]
fn children_ids_resolve_paths_correctly() {
    let t = build_test_tree();

    // Get Hero's children via the node API
    let hero_node = t.tree.get_node(t.hero).unwrap();
    let children: Vec<NodeId> = hero_node.children().to_vec();

    assert_eq!(children.len(), 2);

    // Each child ID should work as a resolver handle
    for &child_id in &children {
        let path = t.tree.node_path(child_id).unwrap();
        assert_eq!(t.tree.get_node_by_path(&path), Some(child_id));
        // Each child should resolve ".." to Hero
        assert_eq!(t.tree.get_node_relative(child_id, ".."), Some(t.hero));
    }

    // First child is Sprite, second is Collider
    assert_eq!(children[0], t.sprite);
    assert_eq!(children[1], t.collider);
}

#[test]
fn children_ids_cross_resolve_siblings() {
    let t = build_test_tree();

    let hero_node = t.tree.get_node(t.hero).unwrap();
    let children: Vec<NodeId> = hero_node.children().to_vec();

    // Sprite → ../Collider
    assert_eq!(
        t.tree.get_node_relative(children[0], "../Collider"),
        Some(t.collider)
    );
    // Collider → ../Sprite
    assert_eq!(
        t.tree.get_node_relative(children[1], "../Sprite"),
        Some(t.sprite)
    );
}

// ===========================================================================
// 12. Parent-returned NodeIds as resolver handles
// ===========================================================================

#[test]
fn parent_id_resolves_as_handle() {
    let t = build_test_tree();

    // Get parent of Sprite (should be Hero)
    let sprite_node = t.tree.get_node(t.sprite).unwrap();
    let parent_id = sprite_node.parent().unwrap();
    assert_eq!(parent_id, t.hero);

    // Use parent ID as a resolver handle
    assert_eq!(
        t.tree.get_node_relative(parent_id, "Collider"),
        Some(t.collider)
    );
    assert_eq!(
        t.tree.node_path(parent_id),
        Some("/root/Players/Hero".to_string())
    );
}

#[test]
fn parent_chain_ids_all_resolve() {
    let t = build_test_tree();

    // Walk parent chain from Sprite up to root
    let mut current = t.sprite;
    let expected_chain = [t.hero, t.players, t.root];

    for &expected_parent in &expected_chain {
        let node = t.tree.get_node(current).unwrap();
        let parent = node.parent().unwrap();
        assert_eq!(parent, expected_parent);

        // Parent ID works as resolver handle
        let path = t.tree.node_path(parent).unwrap();
        assert_eq!(t.tree.get_node_by_path(&path), Some(parent));

        current = parent;
    }

    // Root has no parent
    assert_eq!(t.tree.get_node(t.root).unwrap().parent(), None);
}

// ===========================================================================
// 13. move_child — NodeId stability after reordering
// ===========================================================================

#[test]
fn move_child_preserves_nodeid_identity() {
    let mut t = build_test_tree();

    // Hero is at index 0, Ally at index 1 under Players
    assert_eq!(t.tree.get_index(t.hero), Some(0));
    assert_eq!(t.tree.get_index(t.ally), Some(1));

    // Move Hero to index 1 (swap order)
    t.tree.move_child(t.players, t.hero, 1).unwrap();

    // NodeIds should be unchanged
    assert_eq!(t.tree.get_node(t.hero).unwrap().name(), "Hero");
    assert_eq!(t.tree.get_node(t.ally).unwrap().name(), "Ally");

    // Indices should be swapped
    assert_eq!(t.tree.get_index(t.ally), Some(0));
    assert_eq!(t.tree.get_index(t.hero), Some(1));

    // Path resolution still works
    assert_eq!(t.tree.get_node_by_path("/root/Players/Hero"), Some(t.hero));
    assert_eq!(t.tree.get_node_by_path("/root/Players/Ally"), Some(t.ally));
}

#[test]
fn move_child_nodeid_still_resolves_children() {
    let mut t = build_test_tree();

    // Move Hero to end
    t.tree.move_child(t.players, t.hero, 1).unwrap();

    // Hero's children should still be reachable
    assert_eq!(t.tree.get_node_relative(t.hero, "Sprite"), Some(t.sprite));
    assert_eq!(
        t.tree.get_node_relative(t.hero, "Collider"),
        Some(t.collider)
    );

    // And roundtrip through path
    let sprite_path = t.tree.node_path(t.sprite).unwrap();
    assert_eq!(sprite_path, "/root/Players/Hero/Sprite");
    assert_eq!(t.tree.get_node_by_path(&sprite_path), Some(t.sprite));
}

// ===========================================================================
// 14. u64 script-access roundtrip through children/parent APIs
// ===========================================================================

#[test]
fn u64_roundtrip_through_children_ids() {
    let t = build_test_tree();

    // Simulate script access: get children as u64, reconstruct NodeId, resolve
    let hero_node = t.tree.get_node(t.hero).unwrap();
    let child_raws: Vec<u64> = hero_node.children().iter().map(|id| id.raw()).collect();

    for raw in child_raws {
        let reconstructed = NodeId::from_object_id(ObjectId::from_raw(raw));
        let path = t.tree.node_path(reconstructed).unwrap();
        assert_eq!(t.tree.get_node_by_path(&path), Some(reconstructed));
        // Parent resolution
        assert_eq!(t.tree.get_node_relative(reconstructed, ".."), Some(t.hero));
    }
}

#[test]
fn u64_roundtrip_through_parent_id() {
    let t = build_test_tree();

    // Simulate script access: get parent as u64, reconstruct, resolve
    let sprite_parent_raw = t.tree.get_node(t.sprite).unwrap().parent().unwrap().raw();
    let reconstructed = NodeId::from_object_id(ObjectId::from_raw(sprite_parent_raw));

    assert_eq!(reconstructed, t.hero);
    assert_eq!(
        t.tree.get_node_relative(reconstructed, "Sprite"),
        Some(t.sprite)
    );
    assert_eq!(
        t.tree.node_path(reconstructed),
        Some("/root/Players/Hero".to_string())
    );
}

#[test]
fn u64_roundtrip_through_group_ids() {
    let t = build_test_tree();

    // Get group IDs, convert to u64 and back
    let env_ids = t.tree.get_nodes_in_group("env");
    for &id in &env_ids {
        let raw = id.raw();
        let reconstructed = NodeId::from_object_id(ObjectId::from_raw(raw));
        assert_eq!(reconstructed, id);

        // Use reconstructed ID for resolution
        assert_eq!(t.tree.get_node_relative(reconstructed, ".."), Some(t.world));
    }
}

// ===========================================================================
// 15. Combined source: packed scene children + parent as resolver handles
// ===========================================================================

#[test]
fn packed_scene_children_and_parent_ids_as_handles() {
    let tscn = r#"[gd_scene load_steps=1 format=3]

[node name="Container" type="Node2D"]

[node name="Child1" type="Sprite2D" parent="."]

[node name="Child2" type="Node2D" parent="."]

[node name="Nested" type="Label" parent="Child2"]
"#;

    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let container = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Get children of container via node API
    let container_node = tree.get_node(container).unwrap();
    let children: Vec<NodeId> = container_node.children().to_vec();
    assert_eq!(children.len(), 2);

    // Each child ID should resolve back to itself
    for &child_id in &children {
        let path = tree.node_path(child_id).unwrap();
        assert_eq!(tree.get_node_by_path(&path), Some(child_id));
        // Parent should be container
        assert_eq!(tree.get_node(child_id).unwrap().parent(), Some(container));
    }

    // Child2's child (Nested) — get via children API then resolve
    let child2 = children[1];
    let child2_node = tree.get_node(child2).unwrap();
    let nested_ids: Vec<NodeId> = child2_node.children().to_vec();
    assert_eq!(nested_ids.len(), 1);

    let nested = nested_ids[0];
    assert_eq!(tree.get_node(nested).unwrap().name(), "Nested");
    assert_eq!(
        tree.node_path(nested),
        Some("/root/Container/Child2/Nested".to_string())
    );

    // Nested's parent chain roundtrip
    let nested_parent_raw = tree.get_node(nested).unwrap().parent().unwrap().raw();
    let reconstructed_parent = NodeId::from_object_id(ObjectId::from_raw(nested_parent_raw));
    assert_eq!(reconstructed_parent, child2);
    assert_eq!(
        tree.get_node_relative(reconstructed_parent, "Nested"),
        Some(nested)
    );
}
