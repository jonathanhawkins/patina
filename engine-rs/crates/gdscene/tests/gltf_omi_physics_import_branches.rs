//! The `OMI_physics_body` / `OMI_physics_shape` branches truck_town does not
//! exercise (pat-l8jg8), each against Godot 4.6.1's
//! `GLTFDocumentExtensionPhysics::generate_scene_node` /
//! `GLTFPhysicsBody::to_node` (modules/gltf/extensions/physics/):
//!
//! - `motion.type` `dynamic` -> `RigidBody3D` (mass, CENTER_OF_MASS_MODE_CUSTOM),
//!   `kinematic` -> `AnimatableBody3D`;
//! - a collider with no body parent gets a generated `StaticBody3D` holding a
//!   `<name>Shape` child (`_generate_shape_node_and_body_if_needed`);
//! - a trigger shape under a solid body becomes an `Area3D` `<name>Trigger`
//!   holding the shape as `<name>Shape`;
//! - a `collider` with no `shape` under no body is a compound collider:
//!   a `StaticBody3D` whose child colliders become plain `CollisionShape3D`s;
//! - `hull` is an alias of `convex`; sphere/capsule/cylinder dimensions;
//! - a file that does not list the extension in `extensionsUsed` is ignored
//!   (`import_preflight` returns ERR_SKIP).
//!
//! Every expected tree was confirmed by importing the identical `.gltf` text
//! in Godot 4.6.1 headless and dumping `instantiate()`'s names and classes.

use gdscene::node::NodeId;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn import(json: &str) -> (SceneTree, NodeId) {
    gdobject::class_db::register_all_classes();
    let doc = gdresource::import_gltf_bytes(json.as_bytes(), std::path::Path::new("omi_test.gltf"))
        .expect("parses");
    let scene = gdscene::gltf_scene::packed_scene_from_gltf(&doc).expect("scene");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = gdscene::packed_scene::add_packed_scene_to_tree_with_subscenes(&mut tree, root, &scene, &|_| None)
        .expect("instances");
    (tree, id)
}

fn describe(tree: &SceneTree, id: NodeId, depth: usize, out: &mut Vec<String>) {
    let node = tree.get_node(id).unwrap();
    let mut line = format!("{}{} [{}]", "  ".repeat(depth), node.name(), node.class_name());
    if let Variant::String(s) = node.get_property("__shape__class") {
        line += &format!(" {s}");
    }
    out.push(line);
    for &c in node.children() {
        describe(tree, c, depth + 1, out);
    }
}

fn tree_of(json: &str) -> (SceneTree, NodeId, String) {
    let (tree, id) = import(json);
    let mut lines = Vec::new();
    describe(&tree, id, 0, &mut lines);
    (tree, id, lines.join("\n"))
}

fn search(tree: &SceneTree, id: NodeId, name: &str) -> Option<NodeId> {
    let node = tree.get_node(id)?;
    if node.name() == name {
        return Some(id);
    }
    node.children().iter().find_map(|&c| search(tree, c, name))
}

fn find(tree: &SceneTree, id: NodeId, name: &str) -> NodeId {
    search(tree, id, name).unwrap_or_else(|| panic!("no {name}"))
}

const SHAPES: &str = r#""extensions": { "OMI_physics_shape": { "shapes": [
    { "type": "box", "box": { "size": [2, 3, 4] } },
    { "type": "sphere", "sphere": { "radius": 0.75 } },
    { "type": "capsule", "capsule": { "radius": 0.25, "height": 1.5 } },
    { "type": "cylinder", "cylinder": { "radius": 1.25, "height": 0.5 } }
] } }"#;

#[test]
fn bodies_triggers_and_generated_static_bodies_follow_godot() {
    let json = format!(
        r#"{{ "asset": {{ "version": "2.0" }},
        "extensionsUsed": ["OMI_physics_body", "OMI_physics_shape"],
        {SHAPES},
        "scene": 0, "scenes": [ {{ "name": "OmiTest", "nodes": [0, 2, 3, 5] }} ],
        "nodes": [
          {{ "name": "Crate", "children": [1],
             "extensions": {{ "OMI_physics_body": {{ "motion": {{ "type": "dynamic", "mass": 4.5 }} }} }} }},
          {{ "name": "CrateCol", "extensions": {{ "OMI_physics_body": {{
             "collider": {{ "shape": 0 }}, "trigger": {{ "shape": 1 }} }} }} }},
          {{ "name": "Lone", "extensions": {{ "OMI_physics_body": {{ "collider": {{ "shape": 2 }} }} }} }},
          {{ "name": "Group", "children": [4],
             "extensions": {{ "OMI_physics_body": {{ "collider": {{}} }} }} }},
          {{ "name": "Part", "extensions": {{ "OMI_physics_body": {{ "collider": {{ "shape": 3 }} }} }} }},
          {{ "name": "Lift", "extensions": {{ "OMI_physics_body": {{ "motion": {{ "type": "kinematic" }} }} }} }}
        ] }}"#
    );
    let (tree, root, got) = tree_of(&json);
    // CrateCol under a solid body: the collider is a bare CollisionShape3D
    // (the node itself), and the trigger is added as an Area3D child named
    // `<name>Trigger` holding `<name>Shape` (`_add_physics_node_to_given_node`).
    // Lone: no body parent, so a StaticBody3D is generated and named after the
    // node, its shape `<name>Shape`. Group: compound collider -> StaticBody3D.
    assert_eq!(
        got,
        "\
OmiTest [Node3D]
  Crate [RigidBody3D]
    CrateCol [CollisionShape3D] BoxShape3D
      CrateColTrigger [Area3D]
        CrateColShape [CollisionShape3D] SphereShape3D
  Lone [StaticBody3D]
    LoneShape [CollisionShape3D] CapsuleShape3D
  Group [StaticBody3D]
    Part [CollisionShape3D] CylinderShape3D
  Lift [AnimatableBody3D]",
        "got:\n{got}"
    );
    let crate_id = find(&tree, root, "Crate");
    let crate_node = tree.get_node(crate_id).unwrap();
    assert_eq!(crate_node.get_property("mass"), Variant::Float(4.5));
    assert_eq!(crate_node.get_property("center_of_mass_mode"), Variant::Int(1));
    let col = tree.get_node(find(&tree, root, "CrateCol")).unwrap();
    match col.get_property("__shape__size") {
        Variant::Vector3(v) => assert_eq!((v.x, v.y, v.z), (2.0, 3.0, 4.0)),
        other => panic!("box size {other:?}"),
    }
    let lone = tree.get_node(find(&tree, root, "LoneShape")).unwrap();
    assert_eq!(lone.get_property("__shape__radius"), Variant::Float(0.25));
    assert_eq!(lone.get_property("__shape__height"), Variant::Float(1.5));
}

#[test]
fn physics_extensions_are_ignored_unless_listed_in_extensions_used() {
    let json = format!(
        r#"{{ "asset": {{ "version": "2.0" }},
        {SHAPES},
        "scene": 0, "scenes": [ {{ "name": "OmiTest", "nodes": [0] }} ],
        "nodes": [ {{ "name": "Floor", "extensions": {{ "OMI_physics_body": {{
            "motion": {{ "type": "static" }}, "collider": {{ "shape": 0 }} }} }} }} ] }}"#
    );
    let (_, _, got) = tree_of(&json);
    assert_eq!(got, "OmiTest [Node3D]\n  Floor [Node3D]");
}

/// A node that is both a body and its own collider: Godot builds the body,
/// then (the body being solid) adds the shape as `<name>Shape`. An
/// out-of-range shape index fails that node's extension parse
/// (ERR_FAIL_INDEX_V before `motion` is read), so it stays a plain Node3D.
#[test]
fn a_body_with_its_own_collider_and_a_bad_index() {
    let json = format!(
        r#"{{ "asset": {{ "version": "2.0" }},
        "extensionsUsed": ["OMI_physics_body"],
        {SHAPES},
        "scene": 0, "scenes": [ {{ "name": "OmiTest", "nodes": [0, 1] }} ],
        "nodes": [
          {{ "name": "Ball", "extensions": {{ "OMI_physics_body": {{
             "motion": {{ "type": "static" }}, "collider": {{ "shape": 1 }} }} }} }},
          {{ "name": "Broken", "extensions": {{ "OMI_physics_body": {{
             "motion": {{ "type": "static" }}, "collider": {{ "shape": 99 }} }} }} }}
        ] }}"#
    );
    let (_, _, got) = tree_of(&json);
    assert_eq!(
        got,
        "\
OmiTest [Node3D]
  Ball [StaticBody3D]
    BallShape [CollisionShape3D] SphereShape3D
  Broken [Node3D]",
        "got:\n{got}"
    );
}
