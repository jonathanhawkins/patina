//! truck_town's `town/model/town_model.gltf` declares its colliders through the
//! `OMI_physics_body` / `OMI_physics_shape` glTF extensions, and Godot's import
//! turns them into `StaticBody3D` + `CollisionShape3D` nodes
//! (`modules/gltf/extensions/physics/`). Patina ignored both extensions
//! (pat-l8jg8): every body imported as a plain `Node3D` with no collider, so
//! the whole town had no floor and the car free-fell (y -49.6 at frame 300).
//!
//! Every expected value below is real Godot 4.6.1's, from a headless dump of
//! the imported scene (`load("res://town/model/town_model.gltf").instantiate()`,
//! node name/class/origin, `shape.get_class()`, `shape.get_faces().size()`) and
//! from a drop test on that scene at the project's 120 physics ticks/s.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

/// Instances the imported town under the tree root, as
/// `add_child(load("res://town/model/town_model.gltf").instantiate())` does.
fn town_tree() -> (SceneTree, NodeId) {
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(game_dir()), None);
    let scene = gdscene::gltf_scene::packed_scene_from_gltf_path("res://town/model/town_model.gltf")
        .expect("town_model.gltf imports");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let town = gdscene::packed_scene::add_packed_scene_to_tree_with_subscenes(&mut tree, root, &scene, &|_| None)
        .expect("town instances");
    (tree, town)
}

/// `name [Class] shape=<ShapeClass> faces=<n>` per node, depth-first, the same
/// fields the Godot dump printed.
fn describe(tree: &SceneTree, id: NodeId, depth: usize, out: &mut Vec<String>) {
    let node = tree.get_node(id).unwrap();
    let mut line = format!("{}{} [{}]", "  ".repeat(depth), node.name(), node.class_name());
    if node.class_name() == "CollisionShape3D" {
        let class = match node.get_property("__shape__class") {
            Variant::String(s) => s,
            other => format!("{other:?}"),
        };
        line += &format!(" shape={class}");
        match node.get_property("__shape__data") {
            Variant::PackedVector3Array(v) => line += &format!(" faces={}", v.len()),
            Variant::Nil => {}
            other => line += &format!(" data={other:?}"),
        }
        if let Variant::Vector3(s) = node.get_property("__shape__size") {
            line += &format!(" size=({}, {}, {})", s.x, s.y, s.z);
        }
    }
    out.push(line);
    for &c in node.children() {
        describe(tree, c, depth + 1, out);
    }
}

/// Godot 4.6.1's imported tree, verbatim apart from the dropped origins/layers.
const GODOT_TREE: &str = "\
TownModel [Node3D]
  Terrain [StaticBody3D]
    GrassMesh [MeshInstance3D]
    GrassCollider [CollisionShape3D] shape=ConcavePolygonShape3D faces=22359
    RoadMesh [MeshInstance3D]
    RoadCollider [CollisionShape3D] shape=ConcavePolygonShape3D faces=1197
    OuterGroundMesh [MeshInstance3D]
    OuterGroundCollider [CollisionShape3D] shape=BoxShape3D size=(2048, 10, 2048)
  InstancePos [Node3D]
  Racetrack [Node3D]
    RacetrackMain [StaticBody3D]
      RacetrackMesh [MeshInstance3D]
      RacetrackCollider [CollisionShape3D] shape=ConcavePolygonShape3D faces=38292
    HugeTire [StaticBody3D]
      HugeTireMesh [MeshInstance3D]
      HugeTireCollider [CollisionShape3D] shape=ConcavePolygonShape3D faces=12288
    Ramp1 [StaticBody3D]
      Ramp1Mesh [MeshInstance3D]
      Ramp1Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=48
    Ramp2 [StaticBody3D]
      Ramp2Mesh [MeshInstance3D]
      Ramp2Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=48
    Ramp3 [StaticBody3D]
      Ramp3Mesh [MeshInstance3D]
      Ramp3Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=48
    Ramp4 [StaticBody3D]
      Ramp4Mesh [MeshInstance3D]
      Ramp4Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=48
    Ramp5 [StaticBody3D]
      Ramp5Mesh [MeshInstance3D]
      Ramp5Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=48
  Houses [Node3D]
    House01 [StaticBody3D]
      House01Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=3309
      House01Mesh [MeshInstance3D]
    House02 [StaticBody3D]
      House02Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=1962
      House02Mesh [MeshInstance3D]
    House03 [StaticBody3D]
      House03Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=1848
      House03Mesh [MeshInstance3D]
    House04 [StaticBody3D]
      House04Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=882
      House04Mesh [MeshInstance3D]
    House05 [StaticBody3D]
      House05Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=2367
      House05Mesh [MeshInstance3D]
    House06 [StaticBody3D]
      House06Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=1110
      House06Mesh [MeshInstance3D]
    House07 [StaticBody3D]
      House07Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=1290
      House07Mesh [MeshInstance3D]
    House08 [StaticBody3D]
      House08Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=1431
      House08Mesh [MeshInstance3D]
    House09 [StaticBody3D]
      House09Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=1128
      House09Mesh [MeshInstance3D]
    House10 [StaticBody3D]
      House10Collider [CollisionShape3D] shape=ConcavePolygonShape3D faces=1848
      House10Mesh [MeshInstance3D]";

#[test]
fn imported_town_has_godots_physics_node_tree() {
    let (tree, town) = town_tree();
    let mut lines = Vec::new();
    describe(&tree, town, 0, &mut lines);
    let got = lines.join("\n");
    assert_eq!(got, GODOT_TREE, "imported town tree differs from Godot's:\n{got}");
}

fn child(tree: &SceneTree, id: NodeId, path: &str) -> NodeId {
    path.split('/').fold(id, |at, name| {
        *tree
            .get_node(at)
            .unwrap()
            .children()
            .iter()
            .find(|&&c| tree.get_node(c).unwrap().name() == name)
            .unwrap_or_else(|| panic!("no node {name} on the way to {path}"))
    })
}

fn vec3(tree: &SceneTree, id: NodeId, key: &str) -> Vec<Vector3> {
    match tree.get_node(id).unwrap().get_property(key) {
        Variant::PackedVector3Array(v) => v,
        other => panic!("{key}: {other:?}"),
    }
}

/// The triangle soup must keep Godot's CLOCKWISE winding (the importer swaps
/// glTF's CCW), since a one-sided concave shape collides by face direction.
/// Godot's first GrassCollider and RoadCollider faces, and the body/shape
/// transforms that place them.
#[test]
fn trimesh_faces_and_transforms_match_godot() {
    let (tree, town) = town_tree();
    let close = |a: Vector3, b: (f32, f32, f32)| {
        (a.x - b.0).abs() < 1e-3 && (a.y - b.1).abs() < 1e-3 && (a.z - b.2).abs() < 1e-3
    };
    let grass = vec3(&tree, child(&tree, town, "Terrain/GrassCollider"), "__shape__data");
    let want = [(11.5579, 9.4812, -0.6458), (16.2098, 9.4812, -0.6458), (16.2098, 10.259, -0.6458)];
    for (i, w) in want.iter().enumerate() {
        assert!(close(grass[i], *w), "GrassCollider face 0 vertex {i}: {:?} vs Godot {w:?}", grass[i]);
    }
    let road = vec3(&tree, child(&tree, town, "Terrain/RoadCollider"), "__shape__data");
    let want = [(16.2099, 10.8385, 3.9653), (16.2099, 10.9669, 5.4373), (11.5577, 10.8385, 3.9653)];
    for (i, w) in want.iter().enumerate() {
        assert!(close(road[i], *w), "RoadCollider face 0 vertex {i}: {:?} vs Godot {w:?}", road[i]);
    }
    let origin = |path: &str| match tree.get_node(child(&tree, town, path)).unwrap().get_property("transform") {
        Variant::Transform3D(t) => t.origin,
        other => panic!("{path} transform {other:?}"),
    };
    assert!(close(origin("Terrain/OuterGroundCollider"), (0.0, -5.0, 0.0)));
    assert!(close(origin("Houses/House01"), (15.3681, 9.17, -12.336)));
    assert!(close(origin("Racetrack/Ramp1"), (21.48, 0.8, -6.465)));
    // The five ramps share ONE shape (`shapes[5]`), as in Godot.
    assert_eq!(
        vec3(&tree, child(&tree, town, "Racetrack/Ramp1/Ramp1Collider"), "__shape__data"),
        vec3(&tree, child(&tree, town, "Racetrack/Ramp5/Ramp5Collider"), "__shape__data"),
    );
}

fn rigid_body(tree: &mut SceneTree, name: &str, shape: &str, key: &str, value: Variant, at: Vector3) -> NodeId {
    let root = tree.root_id();
    let body = tree.add_child(root, Node::new(name, "RigidBody3D")).unwrap();
    let mut cs = Node::new("CollisionShape3D", "CollisionShape3D");
    cs.set_property("shape", Variant::String(shape.into()));
    cs.set_property(key, value);
    tree.add_child(body, cs).unwrap();
    gdscene::node3d::set_position(tree, body, at);
    body
}

/// Bodies dropped on the imported town come to rest where Godot's do. Godot
/// 4.6.1 (Jolt, the project's 120 ticks/s), positions after 600 ticks:
///
/// | body | dropped at | Godot rests at |
/// |---|---|---|
/// | unit box | (-15.957, 8, 12.389), flat road at y 5.182 | (-15.957, 5.6807, 12.389) |
/// | r=0.5 sphere | (-20.085, 8, 17.12), flat road | (-20.085, 5.6807, 17.12) |
/// | r=0.5 sphere | (0, 20, 0), a hole in the grass mesh | clips the hole's rim at tick ~170 and bounces off: (-0.1840, 10.2228, 1.1113) at tick 200, rolling on OuterGroundCollider (box, top y 0) at (-1.0434, 0.4999, 8.3874) at tick 500 |
/// | unit box | (35.48, 12, 10), the 6 degree road by the car spawn | (35.4948, 9.2836, 10.0241) |
///
/// Without the import every one of them free-falls past y -100.
#[test]
fn bodies_dropped_on_the_town_rest_where_godots_do() {
    let (mut tree, _town) = town_tree();
    let unit_box = || Variant::Vector3(Vector3::new(1.0, 1.0, 1.0));
    let flat_box = rigid_body(&mut tree, "BoxOnFlatRoad", "BoxShape3D", "size", unit_box(), Vector3::new(-15.957, 8.0, 12.389));
    let flat_sphere = rigid_body(&mut tree, "SphereOnFlatRoad", "SphereShape3D", "radius", Variant::Float(0.5), Vector3::new(-20.085, 8.0, 17.12));
    let hole_sphere = rigid_body(&mut tree, "SphereOnGrass", "SphereShape3D", "radius", Variant::Float(0.5), Vector3::new(0.0, 20.0, 0.0));
    let slope_box = rigid_body(&mut tree, "BoxOnRoad", "BoxShape3D", "size", unit_box(), Vector3::new(35.48, 12.0, 10.0));
    let mut ml = MainLoop::new(tree);
    ml.set_physics_ticks_per_second(120);
    ml.register_physics_bodies();
    // The hole sphere is checked along its path: it is still rolling at tick
    // 600 in Godot (0.39 m/s), so its end point is not a rest pose.
    let mut hole_at = Vec::new();
    for tick in 1..=600 {
        ml.step(1.0 / 120.0);
        if tick == 200 || tick == 500 {
            hole_at.push(gdscene::node3d::get_global_transform(ml.tree(), hole_sphere).origin);
        }
    }
    let pos = |id| gdscene::node3d::get_global_transform(ml.tree(), id).origin;
    for (name, id) in [("flat box", flat_box), ("flat sphere", flat_sphere), ("hole sphere", hole_sphere), ("slope box", slope_box)] {
        eprintln!("{name}: {:?}", pos(id));
    }
    let near = |p: Vector3, w: (f32, f32, f32), tol: f32| {
        (p.x - w.0).abs() < tol && (p.y - w.1).abs() < tol && (p.z - w.2).abs() < tol
    };
    let p = pos(flat_box);
    assert!(near(p, (-15.957, 5.6807, 12.389), 0.01), "box on the flat road at {p:?}; Godot (-15.957, 5.6807, 12.389)");
    let p = pos(flat_sphere);
    assert!(near(p, (-20.085, 5.6807, 17.12), 0.01), "sphere on the flat road at {p:?}; Godot (-20.085, 5.6807, 17.12)");
    // pat-qg3nu: this used to assert only "y 0.4999 at tick 600", which
    // Patina met by falling straight through the hole; with the 3D space's
    // default damping (0.1) it clips the rim and bounces like Godot's.
    let p = hole_at[0];
    assert!(near(p, (-0.1840, 10.2228, 1.1113), 0.05), "sphere bouncing off the hole rim at tick 200: {p:?}; Godot (-0.1840, 10.2228, 1.1113)");
    let p = hole_at[1];
    assert!(near(p, (-1.0434, 0.4999, 8.3874), 0.1), "sphere on the outer ground at tick 500: {p:?}; Godot (-1.0434, 0.4999, 8.3874)");
    // The sloped road: Patina's single-contact solver lets the box settle a
    // few centimetres off Godot's pose, but it must REST, not slide away (it
    // slid 4 m while the trimesh contact point sat at a phantom support).
    let p = pos(slope_box);
    assert!((p.y - 9.2836).abs() < 0.05, "box on the sloped road at y {} (Godot 9.2836); at {p:?}", p.y);
    let drift = ((p.x - 35.4948).powi(2) + (p.z - 10.0241).powi(2)).sqrt();
    assert!(drift < 0.75, "box on the sloped road slid {drift} m to {p:?} (Godot 0.03 m)");
}
