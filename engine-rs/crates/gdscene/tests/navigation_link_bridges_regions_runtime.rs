//! pat-0q2kx.98 acceptance: `NavigationLink2D`/`3D` nodes reach the runtime
//! navigation map.
//!
//! Godot ground truth: `scene/2d/navigation/navigation_link_2d.cpp` — a link
//! registers an off-mesh connection between its global start and end positions
//! on the map (`NavigationServer::link_*`); pathfinding may traverse it, both
//! ways when `bidirectional`, and only while `enabled`.
//!
//! Patina had the link-aware search on both servers already —
//! `NavigationServer2D::add_link` / `find_path`'s stitched graph, and
//! `NavigationServer3D::find_path_linked` — but `MainLoop::process_navigation`
//! gathered only regions and agents. `grep NavigationLink main_loop.rs` returned
//! zero, so `server.links` was permanently empty: a scene whose regions are
//! joined *only* by a link could not path at all, however the link was authored.
//!
//! ⚠️ These tests inject **no** properties. Every position, `enabled` and
//! `bidirectional` value comes from the `.tscn` text through `PackedScene`,
//! because test-side injection is exactly what would hide a missing gather.

use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

/// Two disjoint 20x20 regions — one at the origin, one 100 units down — joined
/// only by a link whose endpoints sit on each mesh. The 80-unit gap is far
/// beyond the 0.5 edge-connection margin, so nothing but the link can bridge
/// them.
///
/// `{ENABLED}` / `{BIDIRECTIONAL}` are substituted per test so every variant
/// shares one scene and differs only in the property under test.
const LINKED_2D_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="NavigationPolygon" id="NavigationPolygon_1"]
vertices = PackedVector2Array(0, 0, 20, 0, 20, 20, 0, 20)
polygons = [PackedInt32Array(0, 1, 2, 3)]

[sub_resource type="NavigationPolygon" id="NavigationPolygon_2"]
vertices = PackedVector2Array(0, 0, 20, 0, 20, 20, 0, 20)
polygons = [PackedInt32Array(0, 1, 2, 3)]

[node name="Main" type="Node2D"]

[node name="RegionA" type="NavigationRegion2D" parent="."]
navigation_polygon = SubResource("NavigationPolygon_1")

[node name="RegionB" type="NavigationRegion2D" parent="."]
position = Vector2(0, 100)
navigation_polygon = SubResource("NavigationPolygon_2")

[node name="Link" type="NavigationLink2D" parent="."]
start_position = Vector2(10, 10)
end_position = Vector2(10, 110)
enabled = {ENABLED}
bidirectional = {BIDIRECTIONAL}

[node name="Agent" type="NavigationAgent2D" parent="."]
position = Vector2({AGENT_X}, {AGENT_Y})
target_position = Vector2({TARGET_X}, {TARGET_Y})
"#;

/// The 3D twin. `NavigationServer3D::find_path` searches one region at a time
/// and never consults links, so this also covers the agent picking the
/// flattened `find_path_linked` search when links are present.
const LINKED_3D_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="NavigationMesh" id="NavigationMesh_1"]
vertices = PackedVector3Array(0, 0, 0, 20, 0, 0, 20, 0, 20, 0, 0, 20)
polygons = [PackedInt32Array(0, 1, 2, 3)]

[sub_resource type="NavigationMesh" id="NavigationMesh_2"]
vertices = PackedVector3Array(0, 0, 0, 20, 0, 0, 20, 0, 20, 0, 0, 20)
polygons = [PackedInt32Array(0, 1, 2, 3)]

[node name="Main" type="Node3D"]

[node name="RegionA" type="NavigationRegion3D" parent="."]
navigation_mesh = SubResource("NavigationMesh_1")

[node name="RegionB" type="NavigationRegion3D" parent="."]
position = Vector3(0, 0, 100)
navigation_mesh = SubResource("NavigationMesh_2")

[node name="Link" type="NavigationLink3D" parent="."]
start_position = Vector3(10, 0, 10)
end_position = Vector3(10, 0, 110)
enabled = {ENABLED}

[node name="Agent" type="NavigationAgent3D" parent="."]
position = Vector3(5, 0, 5)
target_position = Vector3(15, 0, 115)
"#;

/// Builds the 2D scene with the given link flags and agent/target endpoints.
fn scene_2d(enabled: bool, bidirectional: bool, from: (f32, f32), to: (f32, f32)) -> SceneTree {
    let tscn = LINKED_2D_TSCN
        .replace("{ENABLED}", if enabled { "true" } else { "false" })
        .replace(
            "{BIDIRECTIONAL}",
            if bidirectional { "true" } else { "false" },
        )
        .replace("{AGENT_X}", &from.0.to_string())
        .replace("{AGENT_Y}", &from.1.to_string())
        .replace("{TARGET_X}", &to.0.to_string())
        .replace("{TARGET_Y}", &to.1.to_string());
    tree_from(&tscn)
}

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

/// **The acceptance test.** An agent on region A paths to region B across the
/// link; disabling the link removes the only bridge; a one-way link blocks the
/// reverse direction.
#[test]
fn navigation_link_bridges_regions_runtime() {
    // The `.tscn` endpoints reached the node — not injected here. Without this
    // the gather has nothing to read and every assertion below degrades to
    // "no link", which is the bug itself.
    let tree = scene_2d(true, true, (5.0, 5.0), (15.0, 115.0));
    let link = node(&tree, "/root/Main/Link");
    assert!(
        tree.get_node(link)
            .map(|n| !n.get_property("start_position").is_nil())
            .unwrap_or(false),
        "PackedScene must carry the link's authored start_position onto the node"
    );

    // (1) Enabled + bidirectional: the agent crosses.
    let agent = node(&tree, "/root/Main/Agent");
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    let len = ml.nav_agent_path_len_2d(agent);
    assert!(
        len >= 2,
        "the agent must path from region A to region B across the link, got \
         {len} waypoint(s)"
    );

    // (2) The link is the *only* bridge: switching it off leaves the regions
    // disjoint and the target unreachable.
    let tree = scene_2d(false, true, (5.0, 5.0), (15.0, 115.0));
    let agent = node(&tree, "/root/Main/Agent");
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    assert_eq!(
        ml.nav_agent_path_len_2d(agent),
        0,
        "an `enabled = false` link must not bridge the regions"
    );

    // (3) A one-way link still carries A -> B ...
    let tree = scene_2d(true, false, (5.0, 5.0), (15.0, 115.0));
    let agent = node(&tree, "/root/Main/Agent");
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    assert!(
        ml.nav_agent_path_len_2d(agent) >= 2,
        "a one-way link must still carry its forward direction"
    );

    // ... and blocks B -> A. Same scene, agent and target swapped.
    let tree = scene_2d(true, false, (15.0, 115.0), (5.0, 5.0));
    let agent = node(&tree, "/root/Main/Agent");
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    assert_eq!(
        ml.nav_agent_path_len_2d(agent),
        0,
        "`bidirectional = false` must block the reverse direction"
    );
}

/// The reverse direction *is* available once the link is bidirectional — the
/// companion to the one-way case above, so that assertion cannot pass merely
/// because B -> A never works.
#[test]
fn bidirectional_link_carries_the_reverse_direction() {
    let tree = scene_2d(true, true, (15.0, 115.0), (5.0, 5.0));
    let agent = node(&tree, "/root/Main/Agent");
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    assert!(
        ml.nav_agent_path_len_2d(agent) >= 2,
        "a bidirectional link must carry B -> A as well as A -> B"
    );
}

/// The 3D half. `NavigationServer3D::find_path` never consults links, so this
/// fails unless the agent switches to the flattened search when links exist.
#[test]
fn navigation_link_3d_bridges_regions_runtime() {
    let tree = tree_from(&LINKED_3D_TSCN.replace("{ENABLED}", "true"));
    let agent = node(&tree, "/root/Main/Agent");
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    assert!(
        ml.nav_agent_path_len_3d(agent) >= 2,
        "the 3D agent must path across the link, got {} waypoint(s)",
        ml.nav_agent_path_len_3d(agent)
    );

    let tree = tree_from(&LINKED_3D_TSCN.replace("{ENABLED}", "false"));
    let agent = node(&tree, "/root/Main/Agent");
    let mut ml = MainLoop::new(tree);
    ml.step(0.1);
    assert_eq!(
        ml.nav_agent_path_len_3d(agent),
        0,
        "a disabled 3D link must not bridge the regions"
    );
}
