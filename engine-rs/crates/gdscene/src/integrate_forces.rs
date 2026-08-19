//! `RigidBody2D._integrate_forces(state)` dispatch (pat-s3rrv.94).
//!
//! Godot hands a body's script a `PhysicsDirectBodyState2D` once per physics
//! tick when `custom_integrator` is on, and whatever the script writes to that
//! state takes effect for the step that follows.
//!
//! ⚠️ **The pass runs BEFORE `sync_to_physics`, not after.** That is what makes
//! the writes land without any new plumbing: `sync_to_physics` already forwards
//! a rigid body's node `linear_velocity` / `angular_velocity` into the solver
//! body (`physics_server.rs:1872-1878`) and already consumes `_pending_force`,
//! `_pending_torque`, `constant_force` and `constant_torque`. Running after the
//! sync would mean the script's writes sat on the node until the NEXT tick — one
//! frame late, which reads as sluggish control rather than as a bug.

use gdcore::math::{Transform2D, Vector2, Vector3};
use gdcore::math3d::Transform3D;
use gdobject::physics_direct_body_state::{
    new_direct_body_state_2d_handle, new_direct_body_state_3d_handle, take_direct_body_state_2d,
    take_direct_body_state_3d, DirectBodyState2D, DirectBodyState3D,
};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::SceneTree;

fn float_of(v: &Variant, default: f32) -> f32 {
    match v {
        Variant::Float(f) => *f as f32,
        Variant::Int(i) => *i as f32,
        _ => default,
    }
}

fn vec2_of(v: &Variant) -> Vector2 {
    match v {
        Variant::Vector2(v) => *v,
        _ => Vector2::new(0.0, 0.0),
    }
}

fn vec3_of(v: &Variant) -> Vector3 {
    match v {
        Variant::Vector3(v) => *v,
        _ => Vector3::new(0.0, 0.0, 0.0),
    }
}

fn int_of(v: &Variant, default: i64) -> i64 {
    match v {
        Variant::Int(i) => *i,
        Variant::Float(f) => *f as i64,
        _ => default,
    }
}

/// Runs `_integrate_forces` for every `RigidBody2D` that opted in.
///
/// `gravity` is the space's gravity before the body's own `gravity_scale`.
pub fn run_integrate_forces_2d(tree: &mut SceneTree, gravity: Vector2, dt: f32) {
    // Collected up front: the callback runs script that can add or free nodes,
    // and walking the tree while that happens would be a borrow of a structure
    // the script is allowed to mutate.
    let targets: Vec<NodeId> = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .filter(|id| {
            tree.get_node(*id).is_some_and(|n| {
                n.class_name() == "RigidBody2D"
                    // ⚠️ `custom_integrator` GATES the callback. Godot only
                    // calls `_integrate_forces` when it is on, so a body that
                    // merely defines the method must not receive it — that is
                    // acceptance criterion 2's negative half.
                    && matches!(n.get_property("custom_integrator"), Variant::Bool(true))
            })
        })
        .collect();

    for id in targets {
        let Some(state) = build_state(tree, id, gravity, dt) else {
            continue;
        };
        let handle = new_direct_body_state_2d_handle(state);
        tree.call_script_with_access(id, "_integrate_forces", &[Variant::ObjectId(handle)]);
        // ⚠️ Taken back unconditionally, even if the script errored or defined
        // no such method. The handle is scoped to this one call; leaving it
        // published would leak an entry per body per frame.
        if let Some(out) = take_direct_body_state_2d(handle) {
            apply_state(tree, id, out);
        }
    }
}

/// Snapshots the body into a state the script can read.
fn build_state(
    tree: &SceneTree,
    id: NodeId,
    gravity: Vector2,
    dt: f32,
) -> Option<DirectBodyState2D> {
    let node = tree.get_node(id)?;
    let mass = float_of(&node.get_property("mass"), 1.0);
    // Godot stores inverse mass; an explicitly zero or negative mass is treated
    // as infinite rather than producing an infinity here.
    let inverse_mass = if mass > 0.0 { 1.0 / mass } else { 0.0 };
    let inertia = float_of(&node.get_property("inertia"), 0.0);
    // `inertia = 0` is Godot's "compute it from the shapes" sentinel, not a
    // body that cannot rotate, so it maps to an unconstrained inverse rather
    // than to zero.
    let inverse_inertia = if inertia > 0.0 { 1.0 / inertia } else { 1.0 };
    let gravity_scale = float_of(&node.get_property("gravity_scale"), 1.0);
    let position = vec2_of(&node.get_property("position"));
    let rotation = float_of(&node.get_property("rotation"), 0.0);
    let center_of_mass_local = vec2_of(&node.get_property("center_of_mass"));

    Some(DirectBodyState2D {
        step: dt,
        inverse_mass,
        inverse_inertia,
        total_gravity: gravity * gravity_scale,
        total_linear_damp: float_of(&node.get_property("linear_damp"), 0.0),
        total_angular_damp: float_of(&node.get_property("angular_damp"), 0.0),
        center_of_mass: position + center_of_mass_local,
        center_of_mass_local,
        collision_layer: int_of(&node.get_property("collision_layer"), 1),
        collision_mask: int_of(&node.get_property("collision_mask"), 1),
        linear_velocity: vec2_of(&node.get_property("linear_velocity")),
        angular_velocity: float_of(&node.get_property("angular_velocity"), 0.0),
        transform: Transform2D::from_rotation_origin(rotation, position),
        sleeping: matches!(node.get_property("sleeping"), Variant::Bool(true)),
        applied_force: Vector2::new(0.0, 0.0),
        applied_torque: 0.0,
        constant_force: vec2_of(&node.get_property("constant_force")),
        constant_torque: float_of(&node.get_property("constant_torque"), 0.0),
        contacts: contacts_of(node),
    })
}

/// Reads the contacts `PhysicsServer::publish_body_contacts` left on the node
/// (pat-s3rrv.94).
///
/// ⚠️ The list is EMPTY unless the body opted in with `contact_monitor = true`
/// and `max_contacts_reported > 0` — the publisher applies Godot's own gate, so
/// a body that never opted in reports zero contacts even while it is visibly
/// resting on something. That is parity, not a missing read.
fn contacts_of(
    node: &crate::node::Node,
) -> Vec<gdobject::physics_direct_body_state::BodyContact2D> {
    let Variant::Array(items) = node.get_property("_body_contacts") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let Variant::Dictionary(rec) = item else {
                return None;
            };
            let vec2 = |k: &str| match rec.get(k) {
                Some(Variant::Vector2(v)) => *v,
                _ => Vector2::new(0.0, 0.0),
            };
            Some(gdobject::physics_direct_body_state::BodyContact2D {
                local_position: vec2("position"),
                local_normal: vec2("normal"),
                collider_id: match rec.get("collider") {
                    Some(Variant::ObjectId(id)) => id.raw() as i64,
                    _ => 0,
                },
                // The solver does not surface per-contact impulses or shape
                // indices yet; left at their defaults rather than invented, so
                // a reader sees zero instead of a plausible wrong number.
                ..Default::default()
            })
        })
        .collect()
}

/// Writes the script's changes back onto the node, where `sync_to_physics`
/// picks them up on its way into the solver.
fn apply_state(tree: &mut SceneTree, id: NodeId, state: DirectBodyState2D) {
    // Read the pending accumulators first: another system may have queued a
    // force this frame (`apply_force()` on the node), and the callback's forces
    // ADD to those rather than replacing them.
    let (pending_force, pending_torque) = match tree.get_node(id) {
        Some(n) => (
            vec2_of(&n.get_property("_pending_force")),
            float_of(&n.get_property("_pending_torque"), 0.0),
        ),
        None => return,
    };
    let Some(node) = tree.get_node_mut(id) else {
        return;
    };
    node.set_property("linear_velocity", Variant::Vector2(state.linear_velocity));
    node.set_property(
        "angular_velocity",
        Variant::Float(state.angular_velocity as f64),
    );
    node.set_property("position", Variant::Vector2(state.transform.origin));
    node.set_property("sleeping", Variant::Bool(state.sleeping));
    node.set_property(
        "_pending_force",
        Variant::Vector2(pending_force + state.applied_force),
    );
    node.set_property(
        "_pending_torque",
        Variant::Float((pending_torque + state.applied_torque) as f64),
    );
    node.set_property("constant_force", Variant::Vector2(state.constant_force));
    node.set_property(
        "constant_torque",
        Variant::Float(state.constant_torque as f64),
    );
}

/// Runs `_integrate_forces` for every `RigidBody3D` that opted in.
///
/// The 3D twin of [`run_integrate_forces_2d`], and placed at the same point in
/// the tick for the same reason: `physics_server_3d::sync_to_physics` forwards a
/// rigid body's node `linear_velocity` / `angular_velocity` into the solver
/// (`physics_server_3d.rs:1117-1120`), so writing the state back onto the node
/// BEFORE that sync is what makes the script's override take effect this step
/// rather than next.
pub fn run_integrate_forces_3d(tree: &mut SceneTree, gravity: Vector3, dt: f32) {
    let targets: Vec<NodeId> = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .filter(|id| {
            tree.get_node(*id).is_some_and(|n| {
                n.class_name() == "RigidBody3D"
                    && matches!(n.get_property("custom_integrator"), Variant::Bool(true))
            })
        })
        .collect();

    for id in targets {
        let Some(state) = build_state_3d(tree, id, gravity, dt) else {
            continue;
        };
        let handle = new_direct_body_state_3d_handle(state);
        tree.call_script_with_access(id, "_integrate_forces", &[Variant::ObjectId(handle)]);
        if let Some(out) = take_direct_body_state_3d(handle) {
            apply_state_3d(tree, id, out);
        }
    }
}

/// Snapshots a 3D body into a state the script can read.
fn build_state_3d(
    tree: &SceneTree,
    id: NodeId,
    gravity: Vector3,
    dt: f32,
) -> Option<DirectBodyState3D> {
    let node = tree.get_node(id)?;
    let mass = float_of(&node.get_property("mass"), 1.0);
    let inverse_mass = if mass > 0.0 { 1.0 / mass } else { 0.0 };
    let gravity_scale = float_of(&node.get_property("gravity_scale"), 1.0);
    let center_of_mass_local = vec3_of(&node.get_property("center_of_mass"));
    // ⚠️ The transform is read from the node's own `transform` property when it
    // has one. `node3d::local_transform_of` prefers that key over `position`,
    // so composing one from `position` here would report a stale origin for any
    // body whose transform has been written (pat-s3rrv.93 hit the same rule
    // from the writing side).
    let transform = gdscene_local_transform(node);

    Some(DirectBodyState3D {
        step: dt,
        inverse_mass,
        total_gravity: gravity * gravity_scale,
        total_linear_damp: float_of(&node.get_property("linear_damp"), 0.0),
        total_angular_damp: float_of(&node.get_property("angular_damp"), 0.0),
        center_of_mass: transform.origin + center_of_mass_local,
        center_of_mass_local,
        collision_layer: int_of(&node.get_property("collision_layer"), 1),
        collision_mask: int_of(&node.get_property("collision_mask"), 1),
        linear_velocity: vec3_of(&node.get_property("linear_velocity")),
        angular_velocity: vec3_of(&node.get_property("angular_velocity")),
        transform,
        sleeping: matches!(node.get_property("sleeping"), Variant::Bool(true)),
        ..Default::default()
    })
}

/// The node's local transform, preferring an explicit `transform` property.
fn gdscene_local_transform(node: &crate::node::Node) -> Transform3D {
    if let Variant::Transform3D(t) = node.get_property("transform") {
        return t;
    }
    let mut t = Transform3D::IDENTITY;
    t.origin = vec3_of(&node.get_property("position"));
    t
}

/// Writes the script's 3D changes back onto the node.
fn apply_state_3d(tree: &mut SceneTree, id: NodeId, state: DirectBodyState3D) {
    let (pending_force, pending_torque) = match tree.get_node(id) {
        Some(n) => (
            vec3_of(&n.get_property("_pending_force")),
            vec3_of(&n.get_property("_pending_torque")),
        ),
        None => return,
    };
    // ⚠️ Position goes through `node3d::set_position`, NOT a raw `position`
    // write: once a node carries a `transform`, `local_transform_of` ignores
    // `position` entirely and the write is silently discarded.
    crate::node3d::set_position(tree, id, state.transform.origin);
    let Some(node) = tree.get_node_mut(id) else {
        return;
    };
    node.set_property("linear_velocity", Variant::Vector3(state.linear_velocity));
    node.set_property("angular_velocity", Variant::Vector3(state.angular_velocity));
    node.set_property("sleeping", Variant::Bool(state.sleeping));
    node.set_property(
        "_pending_force",
        Variant::Vector3(pending_force + state.applied_force),
    );
    node.set_property(
        "_pending_torque",
        Variant::Vector3(pending_torque + state.applied_torque),
    );
    node.set_property("constant_force", Variant::Vector3(state.constant_force));
    node.set_property("constant_torque", Variant::Vector3(state.constant_torque));
}
