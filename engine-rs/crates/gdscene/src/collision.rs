//! Simple distance-based collision detection for the scene tree.
//!
//! Nodes opt into collision by having these properties:
//! - `collision_radius` (Float) — the radius for overlap checks
//! - `collision_group` (String) — what group this node belongs to (e.g. "bullet")
//! - `collision_mask` (String) — what group this node collides WITH (e.g. "enemy")
//!
//! After collision processing, nodes receive:
//! - `_is_colliding` (Bool) — true if colliding with anything
//! - `_colliding_with` (Array of String) — names of nodes this is colliding with
//! - `_off_screen` (Bool) — true if position is far outside the viewport bounds

use crate::node::NodeId;
use crate::scene_tree::SceneTree;
use gdvariant::Variant;

/// Screen bounds for off-screen detection (default viewport size).
const DEFAULT_SCREEN_WIDTH: f32 = 640.0;
const DEFAULT_SCREEN_HEIGHT: f32 = 480.0;
/// Margin beyond the screen where nodes are considered off-screen.
const OFF_SCREEN_MARGIN: f32 = 64.0;

/// A collision-enabled node's extracted data, used to avoid repeated lookups.
struct ColliderInfo {
    id: NodeId,
    name: String,
    x: f32,
    y: f32,
    radius: f64,
    group: String,
    mask: String,
}

/// Runs simple distance-based collision detection on all nodes in the tree
/// that have a `collision_radius` property, and updates collision result
/// properties (`_is_colliding`, `_colliding_with`).
///
/// Also checks screen bounds and sets `_off_screen` on nodes with positions.
pub fn process_collisions(tree: &mut SceneTree) {
    process_collisions_with_bounds(tree, DEFAULT_SCREEN_WIDTH, DEFAULT_SCREEN_HEIGHT);
}

/// Same as [`process_collisions`] but with configurable screen bounds.
pub fn process_collisions_with_bounds(tree: &mut SceneTree, screen_w: f32, screen_h: f32) {
    let all_ids = tree.all_nodes_in_tree_order();

    // Phase 1: Gather all collider data.
    let mut colliders: Vec<ColliderInfo> = Vec::new();

    for &id in &all_ids {
        let node = match tree.get_node(id) {
            Some(n) => n,
            None => continue,
        };

        let radius = match node.get_property("collision_radius") {
            Variant::Float(r) => r,
            Variant::Int(r) => r as f64,
            _ => continue,
        };
        if radius <= 0.0 {
            continue;
        }

        let (x, y) = extract_position(node);

        let group = match node.get_property("collision_group") {
            Variant::String(s) => s,
            _ => String::new(),
        };
        let mask = match node.get_property("collision_mask") {
            Variant::String(s) => s,
            _ => String::new(),
        };

        colliders.push(ColliderInfo {
            id,
            name: node.name().to_string(),
            x,
            y,
            radius,
            group,
            mask,
        });
    }

    // Phase 2: Check all pairs for overlap.
    // Build a map from NodeId -> list of colliding node names.
    let mut collisions: std::collections::HashMap<NodeId, Vec<String>> =
        std::collections::HashMap::new();

    let len = colliders.len();
    for i in 0..len {
        for j in (i + 1)..len {
            let a = &colliders[i];
            let b = &colliders[j];

            // Group/mask filtering: a collision happens if:
            // - a's mask matches b's group, OR
            // - b's mask matches a's group
            // If both group and mask are empty, always collide.
            let a_wants_b = mask_matches(&a.mask, &b.group);
            let b_wants_a = mask_matches(&b.mask, &a.group);
            if !a_wants_b && !b_wants_a {
                continue;
            }

            let dx = a.x - b.x;
            let dy = a.y - b.y;
            let dist_sq = (dx * dx + dy * dy) as f64;
            let combined_radius = a.radius + b.radius;

            if dist_sq <= combined_radius * combined_radius {
                if a_wants_b {
                    collisions.entry(a.id).or_default().push(b.name.clone());
                }
                if b_wants_a {
                    collisions.entry(b.id).or_default().push(a.name.clone());
                }
            }
        }
    }

    // Phase 3: Write collision results back to nodes.
    for collider in &colliders {
        let id = collider.id;
        if let Some(names) = collisions.get(&id) {
            let name_variants: Vec<Variant> =
                names.iter().map(|n| Variant::String(n.clone())).collect();
            if let Some(node) = tree.get_node_mut(id) {
                node.set_property("_is_colliding", Variant::Bool(true));
                node.set_property("_colliding_with", Variant::Array(name_variants));
            }
        } else if let Some(node) = tree.get_node_mut(id) {
            node.set_property("_is_colliding", Variant::Bool(false));
            node.set_property("_colliding_with", Variant::Array(Vec::new()));
        }
    }

    // Phase 4: Screen bounds check for all nodes with a position.
    for &id in &all_ids {
        let node = match tree.get_node(id) {
            Some(n) => n,
            None => continue,
        };

        let has_position = matches!(node.get_property("position"), Variant::Vector2(_));
        if !has_position {
            continue;
        }

        let (x, y) = extract_position(node);
        let off_screen = x < -OFF_SCREEN_MARGIN
            || x > screen_w + OFF_SCREEN_MARGIN
            || y < -OFF_SCREEN_MARGIN
            || y > screen_h + OFF_SCREEN_MARGIN;

        if let Some(node) = tree.get_node_mut(id) {
            node.set_property("_off_screen", Variant::Bool(off_screen));
        }
    }
}

/// Checks if a mask string matches a group string.
///
/// Rules:
/// - If both mask and group are empty, they match (no filtering).
/// - If mask is empty but group is non-empty, no match (mask must opt in).
/// - If mask is non-empty, it matches if group equals mask.
/// - Mask can contain comma-separated values for matching multiple groups.
fn mask_matches(mask: &str, group: &str) -> bool {
    if mask.is_empty() && group.is_empty() {
        return true;
    }
    if mask.is_empty() {
        return false;
    }
    // Support comma-separated masks like "enemy,obstacle"
    mask.split(',').any(|m| m.trim() == group)
}

/// Extracts (x, y) position from a node, checking the `position` property.
fn extract_position(node: &crate::node::Node) -> (f32, f32) {
    match node.get_property("position") {
        Variant::Vector2(v) => (v.x, v.y),
        _ => (0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use gdcore::math::Vector2;

    /// Helper: create a tree with two nodes at given positions with given radii.
    fn setup_two_nodes(
        pos_a: (f32, f32),
        radius_a: f64,
        pos_b: (f32, f32),
        radius_b: f64,
    ) -> (SceneTree, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut a = Node::new("NodeA", "Node2D");
        a.set_property("position", Variant::Vector2(Vector2::new(pos_a.0, pos_a.1)));
        a.set_property("collision_radius", Variant::Float(radius_a));
        let a_id = tree.add_child(root, a).unwrap();

        let mut b = Node::new("NodeB", "Node2D");
        b.set_property("position", Variant::Vector2(Vector2::new(pos_b.0, pos_b.1)));
        b.set_property("collision_radius", Variant::Float(radius_b));
        let b_id = tree.add_child(root, b).unwrap();

        (tree, a_id, b_id)
    }

    // ── Basic overlap tests ──────────────────────────────────────────────

    #[test]
    fn nodes_within_radius_are_colliding() {
        let (mut tree, a_id, b_id) = setup_two_nodes((100.0, 100.0), 20.0, (110.0, 100.0), 20.0);
        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(true)
        );
        assert_eq!(
            tree.get_node(b_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn nodes_outside_radius_are_not_colliding() {
        let (mut tree, a_id, b_id) = setup_two_nodes((100.0, 100.0), 5.0, (200.0, 100.0), 5.0);
        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(false)
        );
        assert_eq!(
            tree.get_node(b_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn nodes_exactly_touching_are_colliding() {
        // Distance = 40, combined radius = 20 + 20 = 40 -> touching
        let (mut tree, a_id, _b_id) = setup_two_nodes((100.0, 100.0), 20.0, (140.0, 100.0), 20.0);
        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn nodes_just_outside_radius_not_colliding() {
        // Distance = 41, combined radius = 20 + 20 = 40 -> not colliding
        let (mut tree, a_id, _b_id) = setup_two_nodes((100.0, 100.0), 20.0, (141.0, 100.0), 20.0);
        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn colliding_with_contains_other_node_name() {
        let (mut tree, a_id, _b_id) = setup_two_nodes((100.0, 100.0), 20.0, (110.0, 100.0), 20.0);
        process_collisions(&mut tree);

        let colliding_with = tree.get_node(a_id).unwrap().get_property("_colliding_with");
        if let Variant::Array(arr) = colliding_with {
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0], Variant::String("NodeB".to_string()));
        } else {
            panic!("expected Array, got {colliding_with:?}");
        }
    }

    #[test]
    fn node_without_collision_radius_is_ignored() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut a = Node::new("WithRadius", "Node2D");
        a.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        a.set_property("collision_radius", Variant::Float(20.0));
        let a_id = tree.add_child(root, a).unwrap();

        let mut b = Node::new("NoRadius", "Node2D");
        b.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        // No collision_radius property
        let b_id = tree.add_child(root, b).unwrap();

        process_collisions(&mut tree);

        // a should not be colliding (b has no radius)
        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(false)
        );
        // b should have no collision properties at all
        assert_eq!(
            tree.get_node(b_id).unwrap().get_property("_is_colliding"),
            Variant::Nil
        );
    }

    #[test]
    fn zero_radius_is_ignored() {
        let (mut tree, a_id, _b_id) = setup_two_nodes((100.0, 100.0), 0.0, (100.0, 100.0), 20.0);
        process_collisions(&mut tree);

        // a has zero radius, should not be a collider
        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Nil
        );
    }

    // ── Collision group/mask tests ───────────────────────────────────────

    #[test]
    fn collision_groups_filter_correctly() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Bullet: group="bullet", mask="enemy"
        let mut bullet = Node::new("Bullet", "Node2D");
        bullet.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        bullet.set_property("collision_radius", Variant::Float(10.0));
        bullet.set_property("collision_group", Variant::String("bullet".to_string()));
        bullet.set_property("collision_mask", Variant::String("enemy".to_string()));
        let bullet_id = tree.add_child(root, bullet).unwrap();

        // Enemy: group="enemy", mask="bullet"
        let mut enemy = Node::new("Enemy", "Node2D");
        enemy.set_property("position", Variant::Vector2(Vector2::new(105.0, 100.0)));
        enemy.set_property("collision_radius", Variant::Float(15.0));
        enemy.set_property("collision_group", Variant::String("enemy".to_string()));
        enemy.set_property("collision_mask", Variant::String("bullet".to_string()));
        let enemy_id = tree.add_child(root, enemy).unwrap();

        // Player: group="player", mask="enemy"
        let mut player = Node::new("Player", "Node2D");
        player.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        player.set_property("collision_radius", Variant::Float(10.0));
        player.set_property("collision_group", Variant::String("player".to_string()));
        player.set_property("collision_mask", Variant::String("enemy".to_string()));
        let player_id = tree.add_child(root, player).unwrap();

        process_collisions(&mut tree);

        // Bullet should be colliding with Enemy (mask="enemy" matches group="enemy")
        assert_eq!(
            tree.get_node(bullet_id)
                .unwrap()
                .get_property("_is_colliding"),
            Variant::Bool(true)
        );
        let bullet_colliders = tree
            .get_node(bullet_id)
            .unwrap()
            .get_property("_colliding_with");
        if let Variant::Array(arr) = &bullet_colliders {
            assert!(arr.contains(&Variant::String("Enemy".to_string())));
            // Bullet should NOT collide with Player (mask="enemy" doesn't match group="player")
            assert!(!arr.contains(&Variant::String("Player".to_string())));
        } else {
            panic!("expected Array");
        }

        // Enemy should be colliding with Bullet (mask="bullet" matches group="bullet")
        assert_eq!(
            tree.get_node(enemy_id)
                .unwrap()
                .get_property("_is_colliding"),
            Variant::Bool(true)
        );

        // Player should be colliding with Enemy (mask="enemy" matches group="enemy")
        assert_eq!(
            tree.get_node(player_id)
                .unwrap()
                .get_property("_is_colliding"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn same_group_nodes_dont_collide_unless_mask_matches() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Two bullets near each other: group="bullet", mask="enemy"
        let mut b1 = Node::new("Bullet1", "Node2D");
        b1.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        b1.set_property("collision_radius", Variant::Float(10.0));
        b1.set_property("collision_group", Variant::String("bullet".to_string()));
        b1.set_property("collision_mask", Variant::String("enemy".to_string()));
        let b1_id = tree.add_child(root, b1).unwrap();

        let mut b2 = Node::new("Bullet2", "Node2D");
        b2.set_property("position", Variant::Vector2(Vector2::new(105.0, 100.0)));
        b2.set_property("collision_radius", Variant::Float(10.0));
        b2.set_property("collision_group", Variant::String("bullet".to_string()));
        b2.set_property("collision_mask", Variant::String("enemy".to_string()));
        let b2_id = tree.add_child(root, b2).unwrap();

        process_collisions(&mut tree);

        // Neither bullet should be colliding (mask="enemy" doesn't match group="bullet")
        assert_eq!(
            tree.get_node(b1_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(false)
        );
        assert_eq!(
            tree.get_node(b2_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn comma_separated_mask_matches_multiple_groups() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut a = Node::new("A", "Node2D");
        a.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        a.set_property("collision_radius", Variant::Float(20.0));
        a.set_property("collision_group", Variant::String("player".to_string()));
        a.set_property(
            "collision_mask",
            Variant::String("enemy, obstacle".to_string()),
        );
        let a_id = tree.add_child(root, a).unwrap();

        let mut b = Node::new("B", "Node2D");
        b.set_property("position", Variant::Vector2(Vector2::new(105.0, 100.0)));
        b.set_property("collision_radius", Variant::Float(20.0));
        b.set_property("collision_group", Variant::String("obstacle".to_string()));
        b.set_property("collision_mask", Variant::String("player".to_string()));
        let _b_id = tree.add_child(root, b).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(true)
        );
    }

    // ── Collision properties update each frame ──────────────────────────

    #[test]
    fn collision_properties_update_each_frame() {
        let (mut tree, a_id, b_id) = setup_two_nodes((100.0, 100.0), 20.0, (110.0, 100.0), 20.0);

        // Frame 1: colliding
        process_collisions(&mut tree);
        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(true)
        );

        // Move b far away
        tree.get_node_mut(b_id)
            .unwrap()
            .set_property("position", Variant::Vector2(Vector2::new(500.0, 500.0)));

        // Frame 2: no longer colliding
        process_collisions(&mut tree);
        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(false)
        );
        let arr = tree.get_node(a_id).unwrap().get_property("_colliding_with");
        if let Variant::Array(v) = arr {
            assert!(v.is_empty());
        } else {
            panic!("expected Array");
        }
    }

    // ── Screen bounds tests ─────────────────────────────────────────────

    #[test]
    fn node_inside_screen_is_not_off_screen() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut node = Node::new("Ship", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(320.0, 240.0)));
        let id = tree.add_child(root, node).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn node_far_left_is_off_screen() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut node = Node::new("Bullet", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(-100.0, 240.0)));
        let id = tree.add_child(root, node).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn node_far_right_is_off_screen() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut node = Node::new("Bullet", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(800.0, 240.0)));
        let id = tree.add_child(root, node).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn node_far_below_is_off_screen() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut node = Node::new("Bullet", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(320.0, 600.0)));
        let id = tree.add_child(root, node).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn node_far_above_is_off_screen() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut node = Node::new("Bullet", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(320.0, -100.0)));
        let id = tree.add_child(root, node).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn node_within_margin_is_not_off_screen() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Just inside the margin: -64 < -60 < 0
        let mut node = Node::new("Bullet", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(-60.0, 240.0)));
        let id = tree.add_child(root, node).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn node_without_position_gets_no_off_screen() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let node = Node::new("NoPos", "Node");
        let id = tree.add_child(root, node).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Nil
        );
    }

    #[test]
    fn off_screen_updates_when_node_moves_back() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut node = Node::new("Ship", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(-200.0, 240.0)));
        let id = tree.add_child(root, node).unwrap();

        process_collisions(&mut tree);
        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(true)
        );

        // Move back on screen
        tree.get_node_mut(id)
            .unwrap()
            .set_property("position", Variant::Vector2(Vector2::new(320.0, 240.0)));
        process_collisions(&mut tree);
        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(false)
        );
    }

    // ── Multi-node collision scenario ───────────────────────────────────

    #[test]
    fn three_way_collision() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let positions = [(100.0, 100.0), (110.0, 100.0), (105.0, 105.0)];
        let mut ids = Vec::new();
        for (i, &(x, y)) in positions.iter().enumerate() {
            let mut node = Node::new(format!("N{i}"), "Node2D");
            node.set_property("position", Variant::Vector2(Vector2::new(x, y)));
            node.set_property("collision_radius", Variant::Float(20.0));
            ids.push(tree.add_child(root, node).unwrap());
        }

        process_collisions(&mut tree);

        // All three should be colliding with at least one other
        for &id in &ids {
            assert_eq!(
                tree.get_node(id).unwrap().get_property("_is_colliding"),
                Variant::Bool(true)
            );
        }

        // N0 should be colliding with N1 and N2
        let arr = tree
            .get_node(ids[0])
            .unwrap()
            .get_property("_colliding_with");
        if let Variant::Array(v) = arr {
            assert_eq!(v.len(), 2);
        } else {
            panic!("expected Array");
        }
    }

    // ── mask_matches unit tests ─────────────────────────────────────────

    #[test]
    fn mask_matches_both_empty() {
        assert!(mask_matches("", ""));
    }

    #[test]
    fn mask_matches_mask_empty_group_set() {
        assert!(!mask_matches("", "enemy"));
    }

    #[test]
    fn mask_matches_exact() {
        assert!(mask_matches("enemy", "enemy"));
    }

    #[test]
    fn mask_matches_no_match() {
        assert!(!mask_matches("player", "enemy"));
    }

    #[test]
    fn mask_matches_comma_separated() {
        assert!(mask_matches("enemy,obstacle", "obstacle"));
        assert!(mask_matches("enemy, obstacle", "obstacle"));
        assert!(!mask_matches("enemy,obstacle", "player"));
    }

    // ── Integration with SceneTree (process_collisions on tree) ─────────

    #[test]
    fn collision_with_int_radius() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut a = Node::new("A", "Node2D");
        a.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        a.set_property("collision_radius", Variant::Int(20));
        let a_id = tree.add_child(root, a).unwrap();

        let mut b = Node::new("B", "Node2D");
        b.set_property("position", Variant::Vector2(Vector2::new(110.0, 100.0)));
        b.set_property("collision_radius", Variant::Int(20));
        let _b_id = tree.add_child(root, b).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn negative_radius_is_ignored() {
        let (mut tree, a_id, _b_id) = setup_two_nodes((100.0, 100.0), -5.0, (100.0, 100.0), 20.0);
        process_collisions(&mut tree);

        // a has negative radius, treated as no collider
        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Nil
        );
    }

    #[test]
    fn custom_screen_bounds() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut node = Node::new("Ship", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(500.0, 240.0)));
        let id = tree.add_child(root, node).unwrap();

        // With default 640x480, 500 is on screen
        process_collisions(&mut tree);
        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(false)
        );

        // With smaller bounds 400x300, 500 is off screen
        process_collisions_with_bounds(&mut tree, 400.0, 300.0);
        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn no_nodes_with_collisions_is_harmless() {
        let mut tree = SceneTree::new();
        // Just root, no collision nodes
        process_collisions(&mut tree);
        // Should not panic
    }

    #[test]
    fn single_collider_is_not_colliding() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut a = Node::new("Alone", "Node2D");
        a.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        a.set_property("collision_radius", Variant::Float(20.0));
        let a_id = tree.add_child(root, a).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(a_id).unwrap().get_property("_is_colliding"),
            Variant::Bool(false)
        );
    }

    // ── Space shooter scenario test ─────────────────────────────────────

    #[test]
    fn space_shooter_bullet_hits_enemy() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Player at bottom center
        let mut player = Node::new("Player", "Node2D");
        player.set_property("position", Variant::Vector2(Vector2::new(320.0, 400.0)));
        player.set_property("collision_radius", Variant::Float(16.0));
        player.set_property("collision_group", Variant::String("player".to_string()));
        player.set_property("collision_mask", Variant::String("enemy".to_string()));
        let player_id = tree.add_child(root, player).unwrap();

        // Enemy at top
        let mut enemy = Node::new("Enemy1", "Node2D");
        enemy.set_property("position", Variant::Vector2(Vector2::new(320.0, 100.0)));
        enemy.set_property("collision_radius", Variant::Float(20.0));
        enemy.set_property("collision_group", Variant::String("enemy".to_string()));
        enemy.set_property("collision_mask", Variant::String("bullet".to_string()));
        let enemy_id = tree.add_child(root, enemy).unwrap();

        // Bullet traveling upward, near the enemy
        let mut bullet = Node::new("Bullet1", "Node2D");
        bullet.set_property("position", Variant::Vector2(Vector2::new(320.0, 110.0)));
        bullet.set_property("collision_radius", Variant::Float(5.0));
        bullet.set_property("collision_group", Variant::String("bullet".to_string()));
        bullet.set_property("collision_mask", Variant::String("enemy".to_string()));
        let bullet_id = tree.add_child(root, bullet).unwrap();

        process_collisions(&mut tree);

        // Bullet should be colliding with Enemy (distance=10, radii=5+20=25)
        assert_eq!(
            tree.get_node(bullet_id)
                .unwrap()
                .get_property("_is_colliding"),
            Variant::Bool(true)
        );
        let bullet_with = tree
            .get_node(bullet_id)
            .unwrap()
            .get_property("_colliding_with");
        if let Variant::Array(arr) = &bullet_with {
            assert!(arr.contains(&Variant::String("Enemy1".to_string())));
        } else {
            panic!("expected Array");
        }

        // Enemy should be colliding with Bullet (mask="bullet" matches group="bullet")
        assert_eq!(
            tree.get_node(enemy_id)
                .unwrap()
                .get_property("_is_colliding"),
            Variant::Bool(true)
        );

        // Player should NOT be colliding (distance to enemy = 300, radii = 16+20 = 36)
        assert_eq!(
            tree.get_node(player_id)
                .unwrap()
                .get_property("_is_colliding"),
            Variant::Bool(false)
        );

        // All nodes should be on-screen
        assert_eq!(
            tree.get_node(player_id)
                .unwrap()
                .get_property("_off_screen"),
            Variant::Bool(false)
        );
        assert_eq!(
            tree.get_node(bullet_id)
                .unwrap()
                .get_property("_off_screen"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn space_shooter_bullet_flies_off_screen() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut bullet = Node::new("Bullet", "Node2D");
        bullet.set_property("position", Variant::Vector2(Vector2::new(320.0, -200.0)));
        bullet.set_property("collision_radius", Variant::Float(5.0));
        let id = tree.add_child(root, bullet).unwrap();

        process_collisions(&mut tree);

        assert_eq!(
            tree.get_node(id).unwrap().get_property("_off_screen"),
            Variant::Bool(true)
        );
    }
}
