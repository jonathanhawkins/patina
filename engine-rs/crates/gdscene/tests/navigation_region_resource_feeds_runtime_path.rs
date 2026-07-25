//! pat-0q2kx.26 acceptance: a `.tscn`-authored navigation region actually feeds
//! the runtime navigation map.
//!
//! Godot ground truth: `scene/2d/navigation/navigation_region_2d.cpp` — on
//! ENTER_TREE a `NavigationRegion2D` uploads its `NavigationPolygon` (vertices +
//! an array of index polygons) to the navigation map via
//! `region_set_navigation_polygon`; `NavigationRegion3D` does the same with a
//! `NavigationMesh`.
//!
//! Patina had both halves and no seam. `MainLoop::process_navigation` ran every
//! tick, but its region arms read an invented `navmesh_vertices` property that
//! **no loader or node ever produced** — the only writers were `#[cfg(test)]`
//! cases inside `main_loop.rs`. Meanwhile the `.tscn` sub-resource carrying the
//! real geometry was parsed and never read. A region authored in a scene
//! contributed nothing to the map.
//!
//! ⚠️ These tests inject **no** properties. Everything comes from the `.tscn`
//! text through `PackedScene`, because test-side injection of `navmesh_vertices`
//! is precisely what hid this gap for so long.

use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

/// An L-shaped region built from two indexed polygons over shared vertices —
/// the case a single convex blob cannot represent. The agent starts in the
/// lower arm and targets the upper arm, so a path must cross the shared edge.
///
/// Vertices form a 2x2 grid plus the upper extension:
///   0(0,0) 1(20,0) 2(20,20) 3(0,20) 4(20,40) 5(0,40)
/// polygon A = 0,1,2,3 (lower square), polygon B = 3,2,4,5 (upper square).
const NAV_2D_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="NavigationPolygon" id="NavigationPolygon_1"]
vertices = PackedVector2Array(0, 0, 20, 0, 20, 20, 0, 20, 20, 40, 0, 40)
polygons = [PackedInt32Array(0, 1, 2, 3), PackedInt32Array(3, 2, 4, 5)]

[node name="Main" type="Node2D"]

[node name="Region" type="NavigationRegion2D" parent="."]
navigation_polygon = SubResource("NavigationPolygon_1")

[node name="Agent" type="NavigationAgent2D" parent="."]
position = Vector2(10, 5)
target_position = Vector2(10, 35)
"#;

/// Same shape, disabled. Godot's `enabled = false` region contributes nothing.
const NAV_2D_DISABLED_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="NavigationPolygon" id="NavigationPolygon_1"]
vertices = PackedVector2Array(0, 0, 20, 0, 20, 20, 0, 20, 20, 40, 0, 40)
polygons = [PackedInt32Array(0, 1, 2, 3), PackedInt32Array(3, 2, 4, 5)]

[node name="Main" type="Node2D"]

[node name="Region" type="NavigationRegion2D" parent="."]
navigation_polygon = SubResource("NavigationPolygon_1")
enabled = false

[node name="Agent" type="NavigationAgent2D" parent="."]
position = Vector2(10, 5)
target_position = Vector2(10, 35)
"#;

const NAV_3D_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="NavigationMesh" id="NavigationMesh_1"]
vertices = PackedVector3Array(0, 0, 0, 20, 0, 0, 20, 0, 20, 0, 0, 20, 20, 0, 40, 0, 0, 40)
polygons = [PackedInt32Array(0, 1, 2, 3), PackedInt32Array(3, 2, 4, 5)]

[node name="Main" type="Node3D"]

[node name="Region" type="NavigationRegion3D" parent="."]
navigation_mesh = SubResource("NavigationMesh_1")

[node name="Agent" type="NavigationAgent3D" parent="."]
position = Vector3(10, 0, 5)
target_position = Vector3(10, 0, 35)
"#;

fn tree_from(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn node(tree: &SceneTree, path: &str) -> NodeId {
    tree.get_node_by_path(path)
        .unwrap_or_else(|| panic!("node '{path}' not instanced"))
}

/// **The acceptance test.** The scene's own resource feeds the map, and the
/// agent paths across both polygons.
#[test]
fn navigation_region_resource_feeds_runtime_path() {
    let tree = tree_from(NAV_2D_TSCN);
    let region = node(&tree, "/root/Main/Region");
    let agent = node(&tree, "/root/Main/Agent");

    // The `.tscn` sub-resource reached the node — not injected by this test.
    // Without the PackedScene resolution this property is absent and everything
    // below degrades to "no regions", which is exactly the bug.
    assert!(
        tree.get_node(region)
            .map(|n| !n.get_property("__navmesh_vertices__").is_nil())
            .unwrap_or(false),
        "PackedScene must resolve navigation_polygon = SubResource(..) onto the region"
    );

    let mut ml = MainLoop::new(tree);
    ml.step(0.1);

    // ⚠️ The point of the bead: >= 2 waypoints means the path crossed from one
    // indexed polygon into the other. One convex blob over all six vertices
    // would yield a single straight hop and pass a weaker assertion.
    let len = ml.nav_agent_path_len_2d(agent);
    assert!(
        len >= 2,
        "agent must path across both polygons of the region, got {len} waypoint(s)"
    );

    // The published next waypoint stays inside the authored region (x within
    // [0,20], y within [0,40]) rather than cutting outside it.
    let next = match ml.tree().get_node(agent).unwrap().get_property("next_path_position") {
        gdvariant::Variant::Vector2(v) => v,
        other => panic!("next_path_position should be a Vector2, got {other:?}"),
    };
    assert!(
        (0.0..=20.0).contains(&next.x) && (0.0..=40.0).contains(&next.y),
        "next_path_position {next:?} must lie on the authored mesh"
    );
}

/// A disabled region contributes nothing, so there is no mesh to path over.
#[test]
fn disabled_region_contributes_nothing() {
    let tree = tree_from(NAV_2D_DISABLED_TSCN);
    let agent = node(&tree, "/root/Main/Agent");
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    assert_eq!(
        ml.nav_agent_path_len_2d(agent),
        0,
        "an `enabled = false` region must not feed the navigation map"
    );
}

/// The 3D half: `NavigationRegion3D` + `NavigationMesh` behaves the same way.
#[test]
fn navigation_region_3d_resource_feeds_runtime_path() {
    let tree = tree_from(NAV_3D_TSCN);
    let region = node(&tree, "/root/Main/Region");
    let agent = node(&tree, "/root/Main/Agent");

    assert!(
        tree.get_node(region)
            .map(|n| !n.get_property("__navmesh_vertices__").is_nil())
            .unwrap_or(false),
        "PackedScene must resolve navigation_mesh = SubResource(..) onto the region"
    );

    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    let len = ml.nav_agent_path_len_3d(agent);
    assert!(
        len >= 2,
        "3D agent must path across both polygons, got {len} waypoint(s)"
    );
}
