//! pat-s3rrv.180 acceptance: a script rebakes a navigation region at runtime and
//! the result is pathable in the same run; obstacles bend a live agent's path.
//!
//! Two subsystems existed in full and never met. `gdserver3d::bake_navigation_mesh`
//! had **zero non-test callers** — nothing walked a scene into source geometry
//! and nothing wrote a baked result where the runtime could see it — and
//! `NavigationServer2D::add_obstacle` had no runtime caller either, leaving the
//! detour logic in `navigation.rs` dead code in a shipped engine.
//!
//! # What this file does NOT re-prove
//!
//! `navmesh_bake_from_geometry_feeds_map.rs` (pat-nzgdu.114) already pins the
//! baker's slope and radius filters *through to a real path*, building
//! `NavigationMesh3D` params directly in Rust. Repeating that here would add a
//! gate without adding information. What that test cannot see — because it never
//! goes through a scene — is whether a parameter the SCENE chooses ever reaches
//! the baker. It did not: `packed_scene` flattened `vertices`/`polygons` off the
//! NavigationMesh sub-resource and nothing else, so `agent_max_slope` stopped at
//! the resource boundary and every bake silently used the hardcoded 45 degrees.
//! That boundary is what the slope tests below cover.
//!
//! ⓘ This file is named for the acceptance. An earlier revision shortened it to
//! `nav_runtime_bake` on the belief that the verifier truncates test commands at
//! 58 characters. **That is false** — `apps/orchestrator/crate/src/verifier.rs`
//! truncates only failure OUTPUT (`tail_chars`), never the command, and commands
//! of 105 and 133 characters run routinely. Shortening the file left this bead's
//! stored `--test` target pointing at nothing, which fails with exit 2 and zero
//! information about the code. Do not shorten acceptance test filenames.
//!
//! ⚠️ Following `navigation_region_resource_feeds_runtime_path.rs`
//! (pat-0q2kx.26): these scenes inject **no** properties. Everything comes from
//! the `.tscn` text, because test-side injection of `__navmesh_*` is precisely
//! what hid the previous gap in this area for so long.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// A script that rebakes the region beside it, exactly as a game would.
///
/// 🔑 Driven through the SCRIPT rather than by calling `nav_bake::bake_region_3d`
/// directly, because the dispatch is half the bead: before this,
/// `bake_navigation_mesh()` raised `UndefinedFunction` and no amount of working
/// baker code was reachable from a game.
const BAKER_GD: &str = "\
extends Node3D
func _ready():
    var region = get_node(\"Region\")
    region.bake_navigation_mesh()
";

/// A flat 40x40 floor under a region whose NavigationMesh carries **no**
/// geometry — the "empty NavigationMesh plus MeshInstance3D floor" the
/// acceptance names.
///
/// The floor is a default `PlaneMesh` (2x2, centred, on the XZ plane) scaled 20x
/// by the node transform, so it spans [-20, 20] on both axes. Big triangles on
/// purpose: the baker drops any triangle whose inradius is under `agent_radius`
/// (default 0.5), and these have an inradius near 11.7.
const FLAT_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="NavigationMesh" id="NavigationMesh_1"]
agent_max_slope = 45.0

[node name="Main" type="Node3D"]

[node name="Region" type="NavigationRegion3D" parent="."]
navigation_mesh = SubResource("NavigationMesh_1")

[node name="Floor" type="MeshInstance3D" parent="Region"]
mesh_type = "PlaneMesh"
transform = Transform3D(20, 0, 0, 0, 20, 0, 0, 0, 20, 0, 0, 0)

[node name="Agent" type="NavigationAgent3D" parent="."]
position = Vector3(-10, 0, -10)
target_position = Vector3(10, 0, 10)
"#;

/// The same floor tilted 50 degrees about X, with a navmesh that rejects it.
///
/// A plane rotated by 50 degrees has `|dot(normal, up)| = cos 50 = 0.6428`.
/// Against `agent_max_slope = 45` the baker's test is `0.6428 >= cos 45 = 0.7071`
/// — false, so every triangle is dropped. The rotation's SIGN does not matter:
/// the slope test takes the absolute value, so a basis transcribed with the
/// opposite winding still measures 50 degrees.
const RAMP_45_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="NavigationMesh" id="NavigationMesh_1"]
agent_max_slope = 45.0

[node name="Main" type="Node3D"]

[node name="Region" type="NavigationRegion3D" parent="."]
navigation_mesh = SubResource("NavigationMesh_1")

[node name="Ramp" type="MeshInstance3D" parent="Region"]
mesh_type = "PlaneMesh"
transform = Transform3D(20, 0, 0, 0, 12.8558, 15.3209, 0, -15.3209, 12.8558, 0, 0, 0)
"#;

/// Identical geometry, with the ONE value that decides the outcome raised to 60.
///
/// `0.6428 >= cos 60 = 0.5` — true, so the same ramp bakes. Nothing else in this
/// scene differs from `RAMP_45_TSCN`, which is what makes the pair a test of the
/// parameter rather than of the geometry.
const RAMP_60_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="NavigationMesh" id="NavigationMesh_1"]
agent_max_slope = 60.0

[node name="Main" type="Node3D"]

[node name="Region" type="NavigationRegion3D" parent="."]
navigation_mesh = SubResource("NavigationMesh_1")

[node name="Ramp" type="MeshInstance3D" parent="Region"]
mesh_type = "PlaneMesh"
transform = Transform3D(20, 0, 0, 0, 12.8558, 15.3209, 0, -15.3209, 12.8558, 0, 0, 0)
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

/// How many polygons the region currently publishes.
fn polygon_count(tree: &SceneTree, region: NodeId) -> usize {
    match tree
        .get_node(region)
        .map(|n| n.get_property("__navmesh_polygons__"))
    {
        Some(Variant::Array(entries)) => entries.len(),
        _ => 0,
    }
}

/// **The acceptance test.** Bake from a script, then path across the result.
#[test]
fn nav_runtime_bake_and_obstacle_carving() {
    let mut tree = tree_from(FLAT_TSCN);
    let region = node(&tree, "/root/Main/Region");
    let agent = node(&tree, "/root/Main/Agent");
    let main = node(&tree, "/root/Main");

    // 🔑 The starting state is genuinely empty, or "it baked" proves nothing.
    // The sub-resource declares a parameter and no geometry, so the region
    // contributes nothing until something bakes it.
    assert_eq!(
        polygon_count(&tree, region),
        0,
        "the authored NavigationMesh carries no geometry"
    );
    // And the parameter DID cross the resource boundary — the half that was
    // missing. Without the flatten this is Nil and the bake silently uses 45.
    assert!(
        !gdscene::nav_bake::bake_param(&tree, region, "agent_max_slope", -1.0).eq(&-1.0),
        "packed_scene must flatten agent_max_slope onto the region node"
    );

    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(BAKER_GD, main).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    // ⚠️ `_ready` is NOT run by `step` — the frame loop drives `_process` /
    // `_physics_process` only, and every script test in this crate triggers
    // `_ready` explicitly. Relying on `step` alone means the bake never happens
    // and every assertion below fails for a reason that has nothing to do with
    // navigation.
    ml.tree_mut().process_script_ready(main);
    // The step AFTER the bake: `process_navigation` rebuilds the servers from
    // the region nodes, which is what makes the fresh bake pathable in this same
    // run rather than the next one.
    ml.step(0.1);

    // The script's `bake_navigation_mesh()` resolved and produced geometry.
    // Before this bead that call raised `UndefinedFunction` and the script died
    // on that line.
    let baked = polygon_count(ml.tree(), region);
    assert!(baked >= 2, "the floor's triangles must bake, got {baked}");

    // ⚠️ Pathable IN THE SAME RUN. `process_navigation` rebuilds the servers from
    // the region nodes every step, so writing the companion properties is what
    // makes the bake take effect without a reload — that is the wiring
    // requirement, and a bake that only showed up next session would fail here.
    let len = ml.nav_agent_path_len_3d(agent);
    assert!(
        len >= 1,
        "the agent must path across the freshly baked region, got {len} waypoint(s)"
    );
}

/// `bake_finished` reaches a script handler.
#[test]
fn baking_emits_bake_finished() {
    let mut tree = tree_from(FLAT_TSCN);
    let main = node(&tree, "/root/Main");
    let region = node(&tree, "/root/Main/Region");

    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n = seen.clone();
    tree.connect_signal(
        region,
        "bake_finished",
        gdobject::signal::Connection::with_callback(
            gdcore::id::ObjectId::from_raw(9500),
            "on_baked",
            move |_| {
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Variant::Nil
            },
        ),
    );
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(BAKER_GD, main).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    // ⚠️ `_ready` is NOT run by `step` — the frame loop drives `_process` /
    // `_physics_process` only, and every script test in this crate triggers
    // `_ready` explicitly. Relying on `step` alone means the bake never happens
    // and every assertion below fails for a reason that has nothing to do with
    // navigation.
    ml.tree_mut().process_script_ready(main);
    // The step AFTER the bake: `process_navigation` rebuilds the servers from
    // the region nodes, which is what makes the fresh bake pathable in this same
    // run rather than the next one.
    ml.step(0.1);
    assert_eq!(
        seen.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "one bake emits `bake_finished` exactly once"
    );
    // The signal is not the whole story: it must fire AFTER the properties are
    // written, or a handler that reads the region back sees the previous mesh.
    assert!(polygon_count(ml.tree(), region) >= 2);
}

/// A 50-degree ramp is rejected at `agent_max_slope = 45` and admitted at 60.
///
/// 🔑 The two scenes differ in exactly one number. That is what makes this a test
/// of the parameter crossing the resource boundary rather than of the slope
/// filter, which `navmesh_bake_from_geometry_feeds_map.rs` already pins.
#[test]
fn agent_max_slope_from_the_scene_decides_the_bake() {
    for (tscn, expect_polygons, label) in [
        (RAMP_45_TSCN, false, "45 rejects a 50-degree ramp"),
        (RAMP_60_TSCN, true, "60 admits the same ramp"),
    ] {
        let mut tree = tree_from(tscn);
        let region = node(&tree, "/root/Main/Region");
        let main = node(&tree, "/root/Main");
        tree.attach_script(
            main,
            Box::new(GDScriptNodeInstance::from_source(BAKER_GD, main).unwrap()),
        );
        let mut ml = MainLoop::new(tree);
        ml.tree_mut().process_script_ready(main);
        ml.step(0.1);
        let baked = polygon_count(ml.tree(), region);
        assert_eq!(baked > 0, expect_polygons, "{label} (baked {baked})");
    }
}

// ---------------------------------------------------------------------------
// Obstacles
// ---------------------------------------------------------------------------

/// A straight 200x60 corridor with an agent crossing it end to end.
///
/// `{OBSTACLE}` is replaced with the obstacle node under test, or nothing.
const CORRIDOR_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="NavigationPolygon" id="NavigationPolygon_1"]
vertices = PackedVector2Array(0, 0, 200, 0, 200, 60, 0, 60)
polygons = [PackedInt32Array(0, 1, 2, 3)]

[node name="Main" type="Node2D"]

[node name="Region" type="NavigationRegion2D" parent="."]
navigation_polygon = SubResource("NavigationPolygon_1")

[node name="Agent" type="NavigationAgent2D" parent="."]
position = Vector2(10, 30)
target_position = Vector2(190, 30)
{OBSTACLE}
"#;

/// The agent's published path, as points.
fn path_of(ml: &MainLoop, agent: NodeId) -> Vec<Vector2> {
    match ml
        .tree()
        .get_node(agent)
        .map(|n| n.get_property("__nav_path__"))
    {
        Some(Variant::PackedVector2Array(p)) => p,
        _ => Vec::new(),
    }
}

fn path_length(points: &[Vector2]) -> f32 {
    points.windows(2).map(|w| (w[1] - w[0]).length()).sum()
}

fn corridor_with(obstacle: &str) -> (MainLoop, NodeId) {
    let tree = tree_from(&CORRIDOR_TSCN.replace("{OBSTACLE}", obstacle));
    let agent = node(&tree, "/root/Main/Agent");
    (MainLoop::new(tree), agent)
}

/// **Clause 3.** An obstacle on the corridor bends the runtime path around it.
#[test]
fn an_obstacle_bends_the_runtime_path() {
    // Baseline: nothing in the way, so the path is the straight shot.
    let (mut ml, agent) = corridor_with("");
    ml.step(0.1);
    let clear = path_of(&ml, agent);
    assert!(
        !clear.is_empty(),
        "the corridor must be pathable before an obstacle is added"
    );
    let clear_len = path_length(&clear);

    // The same corridor with a 20-radius obstacle sitting on the straight line.
    let (mut ml, agent) = corridor_with(
        "\n[node name=\"Rock\" type=\"NavigationObstacle2D\" parent=\".\"]\nposition = Vector2(100, 30)\nradius = 20.0\n",
    );
    ml.step(0.1);
    let bent = path_of(&ml, agent);
    let bent_len = path_length(&bent);

    // 🔑 Before this bead `add_obstacle` had no runtime caller, so
    // `route_around_obstacles` returned its input untouched and this length was
    // identical to the baseline.
    assert!(
        bent_len > clear_len + 1.0,
        "the detour must be measurably longer: {bent_len} vs {clear_len}"
    );
    // ⚠️ Length alone is not enough — a path that wandered and still clipped the
    // rock would pass it. Every waypoint must clear the radius.
    let centre = Vector2::new(100.0, 30.0);
    for p in &bent {
        assert!(
            (*p - centre).length() >= 20.0,
            "waypoint {p:?} lies inside the obstacle"
        );
    }
}

/// `avoidance_enabled = false` puts the obstacle back to sleep.
///
/// 🔑 The gate is `avoidance_enabled` (default TRUE), not `affect_navigation_mesh`
/// (default false). They belong to different Godot subsystems: this detour is
/// driven by `radius`, which is the avoidance shape, while
/// `affect_navigation_mesh` discards source geometry inside the obstacle's
/// `vertices` during BAKING (`doc/classes/NavigationObstacle2D.xml`). Gating on
/// the wrong one would leave every ordinary obstacle inert.
#[test]
fn a_disabled_obstacle_does_not_bend_the_path() {
    let (mut ml, agent) = corridor_with("");
    ml.step(0.1);
    let clear_len = path_length(&path_of(&ml, agent));

    let (mut ml, agent) = corridor_with(
        "\n[node name=\"Rock\" type=\"NavigationObstacle2D\" parent=\".\"]\nposition = Vector2(100, 30)\nradius = 20.0\navoidance_enabled = false\n",
    );
    ml.step(0.1);
    let len = path_length(&path_of(&ml, agent));
    assert!(
        (len - clear_len).abs() < 0.001,
        "a disabled obstacle must leave the path exactly as it was: {len} vs {clear_len}"
    );
}

/// A zero-radius obstacle blocks nothing.
///
/// Godot's default `radius` is 0.0, so an obstacle dropped into a scene and left
/// alone must not alter any path — otherwise adding the node at all would move
/// agents.
#[test]
fn a_zero_radius_obstacle_blocks_nothing() {
    let (mut ml, agent) = corridor_with("");
    ml.step(0.1);
    let clear_len = path_length(&path_of(&ml, agent));

    let (mut ml, agent) = corridor_with(
        "\n[node name=\"Rock\" type=\"NavigationObstacle2D\" parent=\".\"]\nposition = Vector2(100, 30)\n",
    );
    ml.step(0.1);
    let len = path_length(&path_of(&ml, agent));
    assert!((len - clear_len).abs() < 0.001, "{len} vs {clear_len}");
}
