//! pat-2ifdx / pat-1unro — the official Godot 3D platformer's level, player and
//! enemies under physics, asserted against numbers measured in real Godot
//! 4.6.1 (Jolt, `games/platformer_3d` unmodified, 120 ticks/s, gravity 22).
//!
//! The level is one `GridMap` (`stage/grid_map.scn`, ~2900 cells) whose
//! MeshLibrary (`stage/tiles.tres`) carries each tile's collision in
//! `item/<id>/shapes`. Before these fixes:
//!
//! * no physics code read a GridMap, so the player fell straight through the
//!   level (its y went -5.7 → -7.5 and on down to the respawn);
//! * cells were placed at their corner (`cell * cell_size`), not their centre;
//! * the character swept its capsule at the body origin, ignoring the
//!   `CollisionShape3D`'s +1.05 offset, so it would have stood 1.05 m high;
//! * every text `Transform3D(...)` was parsed TRANSPOSED (Godot's nine basis
//!   numbers are row-major), so the player — turned 90° about Y — faced
//!   backwards and "forward" walked it the wrong way;
//! * `set_as_top_level(true)` dropped the follow camera's parent transform;
//! * a floor contact applied the slid motion twice (2× walking speed);
//! * enemies (RigidBody3D + `custom_integrator`) fell asleep on their first
//!   quiet tick and never woke, so `_integrate_forces` kept adding gravity to a
//!   velocity the solver no longer touched (linear_velocity.y reached -36), got
//!   9.8 from `get_gravity()` instead of 22, were drawn at twice their parent's
//!   offset, kept most of their downward velocity because a rotation-locked
//!   body's contact impulse was budgeted for rotation, turned back and forth
//!   because `Transform3D.looking_at` built +Z toward the target, and were
//!   kicked 0.1 m up at every tile seam by an EPA depth error on sphere vs
//!   convex hull.
//!
//! Godot reference numbers come from a `SceneTree` probe that instances
//! `game.tscn`, steps it, and presses `move_forward` from tick 240 to 360. The
//! ray oracle is `fixtures/golden/physics/platformer_3d_gridmap_rays.txt`, made
//! by `intersect_ray` over the level with the player, enemies and coins removed.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::{GridCell, GridMap, MeshLibrary, SceneTree};
use gdvariant::Variant;

/// One physics tick at the project's `physics_ticks_per_second = 120`.
const TICK: f64 = 1.0 / 120.0;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .expect("games/platformer_3d fixture")
}

/// Boots `game.tscn` the way the runner does (see `p3d_real_scripts.rs`),
/// optionally dropping top-level nodes first.
fn boot_game(drop: &[&str]) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &dir);
    let tscn = std::fs::read_to_string(dir.join("game.tscn")).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::begin_subscene_session();
    let id = gdscene::packed_scene::instance_packed_scene_detached(
        &mut tree,
        &packed,
        &gdscene::packed_scene::load_scene_from_res,
    )
    .unwrap();
    for name in drop {
        let child = tree
            .get_node(id)
            .unwrap()
            .children()
            .iter()
            .copied()
            .find(|c| tree.get_node(*c).is_some_and(|n| n.name() == *name))
            .unwrap_or_else(|| panic!("game.tscn has a {name}"));
        tree.remove_node(child).unwrap();
    }
    gdscene::scripting::attach_subtree_scripts_from_res(&mut tree, id);
    let node = tree.take_node(id).unwrap();
    tree.add_child(root, node).unwrap();
    gdscene::packed_scene::finish_attached_scene(&mut tree, id);
    let mut ml = MainLoop::new(tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.register_physics_bodies();
    ml
}

fn origin(ml: &MainLoop, path: &str) -> Vector3 {
    let id = ml
        .tree()
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("{path}"));
    gdscene::node3d::get_global_transform(ml.tree(), id).origin
}

fn prop(ml: &MainLoop, path: &str, key: &str) -> Variant {
    let id = ml.tree().get_node_by_path(path).unwrap();
    ml.tree().get_node(id).unwrap().get_property(key)
}

fn vec3(v: Variant, what: &str) -> Vector3 {
    match v {
        Variant::Vector3(v) => v,
        other => panic!("{what} is not a Vector3: {other:?}"),
    }
}

fn assert_near(actual: Vector3, godot: Vector3, tol: f32, what: &str) {
    let d = actual - godot;
    assert!(
        d.x.abs() <= tol && d.y.abs() <= tol && d.z.abs() <= tol,
        "{what}: Patina {actual:?}, Godot {godot:?} (tolerance {tol})"
    );
}

// ---------------------------------------------------------------------------
// The GridMap itself
// ---------------------------------------------------------------------------

fn stage_grid() -> (SceneTree, gdscene::NodeId) {
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(game_dir()), None);
    let tscn = std::fs::read_to_string(game_dir().join("stage/stage.tscn")).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree_from_res(&mut tree, root, &packed).unwrap();
    let id = tree.get_node_by_path("/root/Stage/GridMap").unwrap();
    (tree, id)
}

/// Godot: `map_to_local(Vector3i(0, 0, 0)) == (1, 1, 1)` for this 2 m grid with
/// the default `cell_center_*`, and the MeshLibrary item a cell draws sits at
/// `cell_xform * mesh_transform` — the cell's CENTRE, rotated by its
/// orientation. The render list used to put every tile at its corner.
#[test]
fn gridmap_tiles_draw_at_the_cell_centre_with_their_orientation() {
    let (tree, id) = stage_grid();
    let mut grid = GridMap::from_node(&tree, id).unwrap();
    grid.mesh_library = MeshLibrary::load("res://stage/tiles.tres").unwrap();
    assert_eq!(grid.cell_center, [true; 3], "Godot's default centring");
    assert_eq!(
        grid.map_to_local(GridCell { x: 0, y: 0, z: 0 }),
        Vector3::new(1.0, 1.0, 1.0),
        "Godot's map_to_local(0,0,0)"
    );

    let instances = grid.mesh_instances_oriented();
    assert_eq!(instances.len(), grid.cell_count());
    let placed: std::collections::HashSet<[u32; 3]> = instances
        .iter()
        .map(|(p, _, _)| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()])
        .collect();
    for cell in grid.used_cells() {
        let want = Vector3::new(
            cell.x as f32 * 2.0 + 1.0,
            cell.y as f32 * 2.0 + 1.0,
            cell.z as f32 * 2.0 + 1.0,
        );
        assert!(
            placed.contains(&[want.x.to_bits(), want.y.to_bits(), want.z.to_bits()]),
            "cell {cell:?} must draw at its centre {want:?}"
        );
    }
    // An oriented cell keeps its rotation on the drawn tile.
    let oriented = grid
        .used_cells()
        .into_iter()
        .find(|c| grid.cell_orientation(*c) != 0)
        .expect("the level has rotated tiles");
    let centre = grid.map_to_local(oriented);
    let (_, basis, _) = instances.iter().find(|(p, _, _)| *p == centre).unwrap();
    assert_eq!(
        *basis,
        gdscene::grid_map::orientation_basis(grid.cell_orientation(oriented))
    );
}

/// Every item's `item/<id>/shapes` is read, paired with its transform: the
/// library's CornerFloor (item 4) has a corner hull at the centre AND a 2 m box
/// one cell below (`Transform3D(.., 0, -2, 0)`), exactly as tiles.tres writes.
#[test]
fn mesh_library_reads_item_shapes_and_mesh_transform() {
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(game_dir()), None);
    let lib = MeshLibrary::load("res://stage/tiles.tres").unwrap();
    let corner_floor = lib.get_item(4).unwrap();
    assert_eq!(corner_floor.name, "CornerFloor");
    let shapes: Vec<(&str, Vector3)> = corner_floor
        .shapes
        .iter()
        .map(|(p, t)| (p.as_str(), t.origin))
        .collect();
    assert_eq!(
        shapes,
        vec![
            ("res://stage/collision/corner.res", Vector3::ZERO),
            (
                "res://stage/collision/box.tres",
                Vector3::new(0.0, -2.0, 0.0)
            ),
        ]
    );
    assert_eq!(
        corner_floor.mesh_transform,
        gdcore::math3d::Transform3D::IDENTITY
    );
    // Ramp (item 15) is a concave trimesh; TrunkFloor (10) has two shapes.
    assert_eq!(lib.get_item(15).unwrap().shapes.len(), 1);
    assert_eq!(lib.get_item(10).unwrap().shapes.len(), 2);
}

/// The level's collision, ray for ray, against Godot's own `intersect_ray`
/// over the same GridMap: 564 rays (down onto every floor, ramp and corner;
/// along X and Z into walls at four heights). Any misplaced, unrotated or
/// uncentred cell shape — or a missing GridMap body — moves or loses hits.
#[test]
fn gridmap_collision_matches_godot_ray_for_ray() {
    let mut ml = boot_game(&["Player", "Enemies", "Coins"]);
    ml.step(TICK);
    let grid = ml
        .tree()
        .get_node_by_path("/root/Game/Stage/GridMap")
        .unwrap();
    assert!(
        ml.physics_server_3d().gridmap_bodies(grid).len() > 2000,
        "the GridMap must register its cells' shapes"
    );

    let oracle = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/golden/physics/platformer_3d_gridmap_rays.txt"),
    )
    .unwrap();
    let bodies: Vec<&gdphysics3d::body::PhysicsBody3D> =
        ml.physics_server_3d().world().bodies().collect();
    let (mut rays, mut hits, mut bad) = (0, 0, Vec::new());
    for line in oracle.lines().filter(|l| l.starts_with("RAY ")) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let n = |i: usize| f[i].parse::<f32>().unwrap();
        let from = Vector3::new(n(1), n(2), n(3));
        let to = from + Vector3::new(n(4), n(5), n(6));
        let got =
            gdphysics3d::query::PhysicsRayQuery3D::new(from, to).intersect(bodies.iter().copied());
        rays += 1;
        match (f[7], got) {
            ("MISS", None) => {}
            ("HIT", Some(hit)) => {
                hits += 1;
                let pos = Vector3::new(n(8), n(9), n(10));
                let normal = Vector3::new(n(11), n(12), n(13));
                if (hit.point - pos).length() > 0.01 || hit.normal.dot(normal) < 0.999 {
                    bad.push(format!(
                        "{line}\n    Patina HIT {:?} n {:?}",
                        hit.point, hit.normal
                    ));
                }
            }
            (want, got) => bad.push(format!("{line}\n    Patina {got:?} (Godot {want})")),
        }
    }
    assert_eq!(rays, 564);
    assert!(hits > 250, "only {hits} hits");
    assert!(
        bad.is_empty(),
        "{} of {rays} rays disagree with Godot:\n{}",
        bad.len(),
        bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n")
    );
}

// ---------------------------------------------------------------------------
// The player
// ---------------------------------------------------------------------------

/// The player lands on the spawn platform where Godot stands it, and walks
/// "forward" the way Godot walks it.
///
/// Godot 4.6.1 (Jolt): at rest `position.y == -3.998966` (floor top -4 plus
/// `safe_margin`), `is_on_floor()`. Holding `move_forward` from tick 240:
/// tick 300 → (-7.754435, -3.999599, 3.9334); tick 360 → (-4.721112,
/// -3.999599, 3.933393). The follow camera, after its first tick, sits at
/// (-10.75505, 0.11055, 3.9334) — BEHIND the player, which faces +X.
#[test]
fn player_stands_on_the_level_and_walks_forward_like_godot() {
    let mut ml = boot_game(&[]);
    for tick in 1..=360 {
        if tick == 240 {
            ml.input_state_mut().action_press("move_forward", 1.0);
        }
        ml.step(TICK);
        if tick == 2 {
            assert_near(
                origin(&ml, "/root/Game/Player/Target/Camera3D"),
                Vector3::new(-10.75505, 0.11055, 3.9334),
                0.01,
                "follow camera after set_as_top_level(true)",
            );
        }
        if tick == 120 {
            assert_near(
                origin(&ml, "/root/Game/Player"),
                Vector3::new(-9.49833, -3.998966, 3.9334),
                0.002,
                "player at rest on the spawn platform",
            );
            assert_eq!(
                prop(&ml, "/root/Game/Player", "is_on_floor"),
                Variant::Bool(true)
            );
        }
        // ⚠️ The Godot probe sampled from its own `_physics_process`, which
        // runs BEFORE the player's in the same tick, so its "tick 300" is the
        // state after 299 ticks. This test used to read after 300 and passed
        // only because Patina deferred `move_and_slide` to the next tick, so
        // the position it reported was one tick stale (pat-hwhbj).
        if tick == 299 {
            assert_near(
                origin(&ml, "/root/Game/Player"),
                Vector3::new(-7.754435, -3.999599, 3.9334),
                0.002,
                "player after 0.5 s of move_forward",
            );
        }
        if tick == 359 {
            assert_near(
                origin(&ml, "/root/Game/Player"),
                Vector3::new(-4.721112, -3.999599, 3.933393),
                0.002,
                "player after 1 s of move_forward",
            );
        }
    }
    let errors = ml.tree_mut().drain_script_errors();
    assert!(errors.is_empty(), "script errors: {errors:?}");
}

/// Godot's `Node3D.set_as_top_level(true)` keeps the node where it is: the
/// global transform is stored as the new local one (`node_3d.cpp`). The
/// platformer's follow camera calls it in `_ready`; dropping the parent offset
/// moved the camera to near the world origin.
#[test]
fn set_as_top_level_keeps_the_global_transform() {
    gdobject::class_db::register_all_classes();
    let tscn = r#"[gd_scene format=3]
[node name="Root" type="Node3D"]
[node name="Parent" type="Node3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 10, 2, -3)
[node name="Child" type="Node3D" parent="Parent"]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1)
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let child = tree.get_node_by_path("/root/Root/Parent/Child").unwrap();
    let before = gdscene::node3d::get_global_transform(&tree, child).origin;
    assert_eq!(before, Vector3::new(11.0, 3.0, -2.0));
    gdscene::node3d::call_node3d_method(
        &mut tree,
        child,
        "set_as_top_level",
        &[Variant::Bool(true)],
    );
    assert!(gdscene::node3d::is_top_level(&tree, child));
    assert_eq!(
        gdscene::node3d::get_global_transform(&tree, child).origin,
        before,
        "becoming top-level must not move the node"
    );
    gdscene::node3d::call_node3d_method(
        &mut tree,
        child,
        "set_as_top_level",
        &[Variant::Bool(false)],
    );
    assert_eq!(
        gdscene::node3d::get_global_transform(&tree, child).origin,
        before
    );
    assert_eq!(
        gdscene::node3d::get_position(&tree, child),
        Vector3::new(1.0, 1.0, 1.0)
    );
}

// ---------------------------------------------------------------------------
// The enemies (RigidBody3D + custom_integrator)
// ---------------------------------------------------------------------------

/// Enemy1 falls with the project's gravity, lands on the level, rests with a
/// bounded `linear_velocity`, and walks off in Godot's direction.
///
/// Godot 4.6.1 (Jolt), Enemy1 (spawned at (2.3062, -0.59173, -6.03062)):
/// tick 60 → y -3.205759, linear_velocity.y -10.81667 (22 m/s² — 9.8 would be
/// about -4.9); tick 120 → (2.736485, -4.072498, -6.30194), lv (1.69346,
/// -0.162192, -1.067708); tick 180 → (3.582413, -4.072286, -6.835248). At rest
/// its `linear_velocity.y` stays at -0.1833 (one tick of gravity) — never the
/// unbounded growth of a body asleep under its own integrator.
#[test]
fn enemy_lands_rests_and_walks_like_godot() {
    let mut ml = boot_game(&[]);
    const E1: &str = "/root/Game/Enemies/Enemy1";
    for tick in 1..=240 {
        ml.step(TICK);
        let lv = vec3(prop(&ml, E1, "linear_velocity"), "linear_velocity");
        if tick == 60 {
            assert!(
                (lv.y - -10.81667).abs() < 0.25,
                "falling at the project's gravity: lv.y {} (Godot -10.81667)",
                lv.y
            );
        }
        if tick == 120 {
            assert_near(
                origin(&ml, E1),
                Vector3::new(2.736485, -4.072498, -6.30194),
                0.1,
                "Enemy1 landed and walking",
            );
            let speed = Vector3::new(lv.x, 0.0, lv.z).length();
            assert!(
                (speed - 2.0).abs() < 0.05 && lv.x > 0.0 && lv.z < 0.0,
                "walking at MAX_SPEED 2 toward +X/-Z like Godot (1.69, -1.07): {lv:?}"
            );
        }
        if tick == 180 {
            assert_near(
                origin(&ml, E1),
                Vector3::new(3.582413, -4.072286, -6.835248),
                0.15,
                "Enemy1 still walking its line",
            );
        }
        if tick >= 100 {
            assert!(
                lv.y.abs() < 0.5,
                "tick {tick}: a resting enemy's linear_velocity.y must stay bounded \
                 (Godot -0.1833), got {}",
                lv.y
            );
        }
    }
    // Drawn where it simulates: the node's global position is the body's.
    let id = ml.tree().get_node_by_path(E1).unwrap();
    let body = ml.physics_server_3d().body_for_node(id).unwrap();
    let body_pos = ml
        .physics_server_3d()
        .world()
        .get_body(body)
        .unwrap()
        .position;
    assert_near(origin(&ml, E1), body_pos, 1e-4, "node vs its physics body");
}
