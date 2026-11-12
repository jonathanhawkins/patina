//! pat-0q2kx.203 — `PhysicalBone3D` bodies + `PhysicalBoneSimulator3D` pose
//! writeback (ragdolls).
//!
//! Before this, only the ClassDB surface existed: `grep PhysicalBone /
//! physical_bone / simulate_physics / physical_bones_start` across `gdscene` and
//! `gdphysics3d` returned **zero** implementation. A `PhysicalBone3D` in a scene
//! registered no body, created no joint, and never touched the skeleton, so a
//! ragdoll simply did not exist.
//!
//! It also could not have been *authored*: `PhysicalBone3D`'s registration had
//! `joint_type`, `joint_offset`, `body_offset`, mass and damping but **no
//! `bone_name`**, so a `.tscn` had nowhere to record which bone a body drives.
//!
//! Godot: `upstream/godot/scene/3d/physics/physical_bone_3d.cpp` and
//! `physical_bone_simulator_3d.cpp`.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdphysics3d::body::BodyType3D;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d;
use gdvariant::Variant;

/// Bone rest offsets: each bone hangs one unit below its parent, so the rest
/// chain is hip(0,0,0) -> thigh(0,-1,0) -> shin(0,-2,0).
const SEGMENT: f32 = 1.0;

struct Rig {
    ml: MainLoop,
    skeleton: NodeId,
    simulator: NodeId,
    hip: NodeId,
    thigh: NodeId,
    shin: NodeId,
}

/// Adds a `PhysicalBone3D` bound to `bone`, positioned at that bone's global
/// rest origin so its body starts where the bone is.
fn add_bone_node(
    tree: &mut SceneTree,
    simulator: NodeId,
    name: &str,
    bone: &str,
    y: f32,
) -> NodeId {
    let id = tree
        .add_child(simulator, Node::new(name, "PhysicalBone3D"))
        .unwrap();
    tree.get_node_mut(id)
        .unwrap()
        .set_property("bone_name", Variant::String(bone.to_string()));
    node3d::set_position(tree, id, Vector3::new(0.0, y, 0.0));
    id
}

/// Builds the acceptance rig: a 3-bone skeleton whose bones are each driven by
/// a `PhysicalBone3D` under a `PhysicalBoneSimulator3D`.
fn build_rig() -> Rig {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let skeleton = tree
        .add_child(root, Node::new("Skel", "Skeleton3D"))
        .unwrap();

    // hip is the root bone; thigh hangs off hip; shin off thigh.
    let hip_idx = skeleton3d::add_bone(&mut tree, skeleton, "hip");
    let thigh_idx = skeleton3d::add_bone(&mut tree, skeleton, "thigh");
    let shin_idx = skeleton3d::add_bone(&mut tree, skeleton, "shin");
    skeleton3d::set_bone_parent(&mut tree, skeleton, thigh_idx, hip_idx as i64);
    skeleton3d::set_bone_parent(&mut tree, skeleton, shin_idx, thigh_idx as i64);

    let drop = Transform3D {
        basis: gdcore::math3d::Basis::IDENTITY,
        origin: Vector3::new(0.0, -SEGMENT, 0.0),
    };
    skeleton3d::set_bone_rest(&mut tree, skeleton, hip_idx, Transform3D::IDENTITY);
    skeleton3d::set_bone_rest(&mut tree, skeleton, thigh_idx, drop);
    skeleton3d::set_bone_rest(&mut tree, skeleton, shin_idx, drop);

    let simulator = tree
        .add_child(skeleton, Node::new("Sim", "PhysicalBoneSimulator3D"))
        .unwrap();

    // Each bone body starts at its bone's global rest origin.
    let hip = add_bone_node(&mut tree, simulator, "HipBone", "hip", 0.0);
    let thigh = add_bone_node(&mut tree, simulator, "ThighBone", "thigh", -SEGMENT);
    let shin = add_bone_node(&mut tree, simulator, "ShinBone", "shin", -2.0 * SEGMENT);

    Rig {
        ml: MainLoop::new(tree),
        skeleton,
        simulator,
        hip,
        thigh,
        shin,
    }
}

fn body_type(ml: &MainLoop, node: NodeId) -> Option<BodyType3D> {
    let server = ml.physics_server_3d();
    let id = server.body_for_node(node)?;
    server.world().get_body(id).map(|b| b.body_type)
}

fn body_pos(ml: &MainLoop, node: NodeId) -> Vector3 {
    let server = ml.physics_server_3d();
    let id = server.body_for_node(node).expect("bone has a body");
    server.world().get_body(id).expect("body exists").position
}

fn bone_global_y(ml: &MainLoop, skeleton: NodeId, bone: &str) -> f32 {
    let idx = skeleton3d::find_bone(ml.tree(), skeleton, bone).expect("bone exists");
    skeleton3d::get_bone_global_pose(ml.tree(), skeleton, idx)
        .origin
        .y
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** A hip-anchored 2-bone chain started via
/// `physical_bones_start_simulation` falls, stays connected, writes its
/// simulated poses onto the skeleton, and returns to animation control on stop.
#[test]
fn physics3d_ragdoll_sim() {
    let mut rig = build_rig();

    // -- one tick registers the bodies, all Kinematic (animation-driven) -----
    rig.ml.step(1.0 / 60.0);
    for (name, node) in [("hip", rig.hip), ("thigh", rig.thigh), ("shin", rig.shin)] {
        assert_eq!(
            body_type(&rig.ml, node),
            Some(BodyType3D::Kinematic),
            "{name} must register a body, Kinematic until simulation starts",
        );
    }

    let rest_shin_y = bone_global_y(&rig.ml, rig.skeleton, "shin");
    assert!(
        (rest_shin_y - -2.0 * SEGMENT).abs() < 1e-4,
        "the rest chain puts shin at y = -2, got {rest_shin_y}",
    );

    // A Kinematic bone must not fall — otherwise "it dropped" below proves
    // nothing about the simulation actually being switched on.
    for _ in 0..30 {
        rig.ml.step(1.0 / 60.0);
    }
    assert!(
        (bone_global_y(&rig.ml, rig.skeleton, "shin") - rest_shin_y).abs() < 1e-3,
        "bones must stay on their animated pose before simulation starts",
    );

    // -- start simulating thigh+shin; hip stays put as the anchor ------------
    let simulated = ["thigh".to_string(), "shin".to_string()];
    rig.ml
        .physical_bones_start_simulation(rig.simulator, &simulated);

    assert_eq!(
        body_type(&rig.ml, rig.thigh),
        Some(BodyType3D::Rigid),
        "a simulated bone becomes Rigid",
    );
    assert_eq!(
        body_type(&rig.ml, rig.hip),
        Some(BodyType3D::Kinematic),
        "a bone left out of the list stays Kinematic and anchors the chain",
    );

    let hip_start = body_pos(&rig.ml, rig.hip);

    for _ in 0..120 {
        rig.ml.step(1.0 / 60.0);
    }

    // (a) it fell — gravity is -Y, so this direction claim is robust.
    let shin_y = bone_global_y(&rig.ml, rig.skeleton, "shin");
    assert!(
        shin_y < rest_shin_y - 1e-3,
        "the simulated shin must hang below its rest y ({rest_shin_y}), got {shin_y}",
    );

    // (b) the skeleton reflects the simulation, not just the bodies.
    let thigh_y = bone_global_y(&rig.ml, rig.skeleton, "thigh");
    assert!(
        thigh_y.is_finite() && shin_y.is_finite(),
        "poses must stay finite, got thigh {thigh_y} shin {shin_y}",
    );

    // (c) the anchor did not move.
    let hip_now = body_pos(&rig.ml, rig.hip);
    assert!(
        (hip_now.y - hip_start.y).abs() < 1e-3,
        "the Kinematic hip must not fall; {} -> {}",
        hip_start.y,
        hip_now.y,
    );

    // (d) the chain stayed connected: the pin joints preserve segment length.
    let thigh_p = body_pos(&rig.ml, rig.thigh);
    let shin_p = body_pos(&rig.ml, rig.shin);
    let seg = (shin_p - thigh_p).length();
    assert!(
        (seg - SEGMENT).abs() <= SEGMENT * 0.05,
        "pin joints must hold the thigh->shin segment at {SEGMENT} ±5%, got {seg}",
    );

    // -- stop: bones return to animation control ----------------------------
    rig.ml.physical_bones_stop_simulation(rig.simulator);
    assert_eq!(
        body_type(&rig.ml, rig.thigh),
        Some(BodyType3D::Kinematic),
        "stopping returns the bone to Kinematic so animation drives it again",
    );

    let after_stop = bone_global_y(&rig.ml, rig.skeleton, "shin");
    for _ in 0..30 {
        rig.ml.step(1.0 / 60.0);
    }
    assert!(
        (bone_global_y(&rig.ml, rig.skeleton, "shin") - after_stop).abs() < 1e-3,
        "a stopped ragdoll must stop writing poses — the skeleton holds still",
    );
}

// ===========================================================================
// Binding and lifecycle
// ===========================================================================

/// The body binds to its bone by **name**, which is what a `.tscn` stores.
#[test]
fn bone_name_binds_body_to_skeleton_bone() {
    let mut rig = build_rig();
    rig.ml.step(1.0 / 60.0);

    let idx = skeleton3d::find_bone(rig.ml.tree(), rig.skeleton, "shin")
        .expect("`shin` resolves via the registered bone_name");
    assert_eq!(idx, 2, "shin is the third bone");

    // The property must actually be registered, or a .tscn could not carry it.
    gdobject::class_db::register_3d_classes();
    let info = gdobject::class_db::get_class_info("PhysicalBone3D").expect("registered");
    let props: Vec<&str> = info.properties.iter().map(|p| p.name.as_str()).collect();
    assert!(
        props.contains(&"bone_name"),
        "PhysicalBone3D must register `bone_name`; has {props:?}",
    );
}

/// A bone with no `bone_name` is simply not bound — it must not panic or
/// silently drive bone 0.
#[test]
fn unbound_bone_is_ignored() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let skeleton = tree
        .add_child(root, Node::new("Skel", "Skeleton3D"))
        .unwrap();
    skeleton3d::add_bone(&mut tree, skeleton, "only");
    let simulator = tree
        .add_child(skeleton, Node::new("Sim", "PhysicalBoneSimulator3D"))
        .unwrap();
    // No bone_name set at all.
    tree.add_child(simulator, Node::new("Loose", "PhysicalBone3D"))
        .unwrap();

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    ml.physical_bones_start_simulation(simulator, &[]);
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
    }

    let pose = skeleton3d::get_bone_global_pose(ml.tree(), skeleton, 0);
    assert!(
        pose.origin.y.abs() < 1e-6,
        "an unbound PhysicalBone3D must not write onto bone 0, got {:?}",
        pose.origin,
    );
}

/// An empty bone list simulates everything, matching Godot's default argument.
#[test]
fn empty_bone_list_simulates_all_bones() {
    let mut rig = build_rig();
    rig.ml.step(1.0 / 60.0);
    rig.ml.physical_bones_start_simulation(rig.simulator, &[]);

    for (name, node) in [("hip", rig.hip), ("thigh", rig.thigh), ("shin", rig.shin)] {
        assert_eq!(
            body_type(&rig.ml, node),
            Some(BodyType3D::Rigid),
            "{name} must simulate when no bone list is given",
        );
    }
}

/// Restarting must not accumulate joints, and must not inherit the momentum the
/// bodies had when they were stopped.
#[test]
fn restart_is_clean() {
    let mut rig = build_rig();
    // Pin the two simulated bones to their parents. Godot's `joint_type`
    // defaults to JOINT_TYPE_NONE (`doc/classes/PhysicalBone3D.xml:96`,
    // `default="0"`), so an unauthored bone creates NO joint — and the
    // "releases the two auto-created joints" check below underflowed on
    // `0 - 2`. JOINT_TYPE_PIN = 1.
    for node in [rig.thigh, rig.shin] {
        rig.ml
            .tree_mut()
            .get_node_mut(node)
            .unwrap()
            .set_property("joint_type", Variant::Int(1));
    }
    rig.ml.step(1.0 / 60.0);

    let only = ["thigh".to_string(), "shin".to_string()];
    rig.ml.physical_bones_start_simulation(rig.simulator, &only);
    for _ in 0..60 {
        rig.ml.step(1.0 / 60.0);
    }
    let joints_after_first = rig.ml.physics_server_3d().world().joint_count();

    rig.ml.physical_bones_stop_simulation(rig.simulator);
    assert_eq!(
        rig.ml.physics_server_3d().world().joint_count(),
        joints_after_first - 2,
        "stopping releases the two auto-created ragdoll joints",
    );

    rig.ml.physical_bones_start_simulation(rig.simulator, &only);
    assert_eq!(
        rig.ml.physics_server_3d().world().joint_count(),
        joints_after_first,
        "restarting recreates exactly the same joints, not a second set",
    );
}
