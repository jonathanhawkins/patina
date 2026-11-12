//! pat-s3rrv.105 (merged findings) — `PhysicsBody3D.axis_lock_linear_*` and
//! `axis_lock_angular_*` on a `RigidBody3D`.
//!
//! Godot ground truth: `doc/classes/PhysicsBody3D.xml` — six independent
//! booleans freezing motion along or about each world axis.
//!
//! ## What was missing
//!
//! `grep axis_lock_linear crates/*/src` returned **zero** hits: the properties
//! existed only in the editor's generated inspector schema, so authoring one had
//! no effect at all. `gdphysics3d::body` had only the all-or-nothing
//! `lock_rotation`, which is a *different* Godot property.
//!
//! ## ⚠️ Why the lock is applied to velocity, before integration
//!
//! Zeroing a locked component *after* the position update leaves the body having
//! already travelled that tick, so a "locked" axis creeps one step per frame —
//! about 2.7 mm/frame under default gravity, which looks like solver tolerance
//! rather than a bug. The test below asserts the axis is EXACTLY unmoved for
//! that reason: a near-zero tolerance would accept the broken ordering.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// A `RigidBody3D` falling in free space, with the given locks authored.
///
/// No floor: this is about integration, not contact, and a floor would stop the
/// control case for a reason unrelated to the flag under test.
fn falling_body(locks: &[(&str, bool)]) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let mut body = Node::new("Faller", "RigidBody3D");
    for (key, value) in locks {
        body.set_property(key, Variant::Bool(*value));
    }
    let body_id = tree.add_child(root, body).unwrap();
    // Set through `node3d` for the same reason the reader above does.
    gdscene::node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 5.0, 0.0));
    let mut bs = Node::new("Shape", "CollisionShape3D");
    bs.set_property("shape", Variant::String("SphereShape3D".to_string()));
    bs.set_property("radius", Variant::Float(0.5));
    tree.add_child(body_id, bs).unwrap();

    (MainLoop::new(tree), body_id)
}

/// ⚠️ Read through `node3d::get_position`, NOT `get_property("position")`.
/// A Node3D carries a transform, and `node3d` reads that when one exists,
/// ignoring the raw `position` property (`node3d.rs:69-72`). The solver
/// publishes back with `node3d::set_position`, so the raw property can be stale
/// — a test reading it would see the authored value and report "the body never
/// moved" against a perfectly working engine.
fn position(ml: &MainLoop, id: NodeId) -> Vector3 {
    gdscene::node3d::get_position(ml.tree(), id)
}

fn run(locks: &[(&str, bool)], ticks: usize) -> Vector3 {
    let (mut ml, id) = falling_body(locks);
    for _ in 0..ticks {
        ml.step(1.0 / 60.0);
    }
    position(&ml, id)
}

/// **The acceptance.** A Y-locked body does not fall; an unlocked one does.
#[test]
fn rigidbody3d_axis_locks() {
    const TICKS: usize = 60;

    // The control comes FIRST and is asserted first: if gravity were not being
    // applied at all, the locked case below would also be motionless and would
    // "pass" while proving nothing.
    let free = run(&[], TICKS);
    assert!(
        free.y < 4.0,
        "the control body must actually fall under gravity, or the lock case \
         proves nothing; it ended at y = {}",
        free.y
    );

    let locked = run(&[("axis_lock_linear_y", true)], TICKS);
    // ⚠️ EXACTLY 5.0, not approximately. Applying the lock after the position
    // update instead of before would leave a per-frame creep of a few
    // millimetres — small enough to slip through a tolerant comparison and to
    // read as solver noise rather than a mis-ordered lock.
    assert_eq!(
        locked.y, 5.0,
        "axis_lock_linear_y must freeze Y exactly; the body started at 5.0 and \
         ended at {}",
        locked.y
    );
}

/// **Diagnostic for pat-s3rrv.105's failing `custom_integrator` clause.**
///
/// `physics3d_integrate_forces_callback` fails with the body falling **exactly
/// `g·dt²` per tick** — one tick of gravity, every tick — even though
/// `sync_rigid_body` zeroes `gravity_scale` whenever `custom_integrator` is set,
/// and that override is correctly ordered after the assignment it overrides.
///
/// This authors `gravity_scale = 0` **directly**, bypassing `custom_integrator`
/// altogether, and so separates the two remaining explanations:
///
/// * it **passes** ⇒ `gravity_scale = 0` genuinely suppresses gravity, and the
///   defect is that the `custom_integrator` override never fires;
/// * it **fails** ⇒ `gravity_scale` itself is not reaching the solver per tick,
///   and the override is irrelevant.
///
/// Those need entirely different fixes, and no amount of reading the source
/// distinguishes them — every site involved is correct as written.
#[test]
fn rigidbody3d_axis_locks_gravity_scale_zero_suppresses_gravity() {
    let (mut ml, id) = falling_body(&[]);
    if let Some(n) = ml.tree_mut().get_node_mut(id) {
        n.set_property("gravity_scale", Variant::Float(0.0));
    }
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    let y = position(&ml, id).y;
    assert_eq!(
        y, 5.0,
        "gravity_scale = 0 must suppress gravity entirely; the body started at \
         5.0 and ended at {y}. A drop of about 0.0027 per tick is one tick of \
         gravity per tick, the same signature as the custom_integrator failure \
         in physics3d_integrate_forces_callback."
    );
}

/// A lock on one axis leaves the others free.
///
/// Locking every axis would pass against an implementation that ignored the
/// index and froze the whole vector — which is the obvious way to get this
/// wrong, since the body already has an all-or-nothing `lock_rotation`.
#[test]
fn rigidbody3d_axis_locks_are_per_axis() {
    const TICKS: usize = 60;

    // X is locked; gravity acts on Y, which must still move.
    let x_locked = run(&[("axis_lock_linear_x", true)], TICKS);
    assert_eq!(
        x_locked.x, 0.0,
        "the locked X axis must not move, got {}",
        x_locked.x
    );
    assert!(
        x_locked.y < 4.0,
        "…while the UNLOCKED Y axis still falls: a per-axis lock must not \
         freeze the whole body, got y = {}",
        x_locked.y
    );
}

/// The angular locks are independent of `lock_rotation`.
///
/// `lock_rotation` is Godot's separate all-or-nothing switch, and the two are
/// distinct properties. A body with only `axis_lock_angular_y` set must not be
/// treated as fully rotation-locked.
#[test]
fn rigidbody3d_axis_locks_angular_is_not_lock_rotation() {
    let (mut ml, id) = falling_body(&[("axis_lock_angular_y", true)]);
    // Spin the body about all three axes.
    if let Some(n) = ml.tree_mut().get_node_mut(id) {
        n.set_property(
            "angular_velocity",
            Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)),
        );
    }
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }

    // ⚠️ Asserted on `angular_velocity`, NOT on `rotation`. The 3D bridge
    // publishes the solved angular velocity back onto the node
    // (`physics_server_3d.rs:2388`) but does **not** publish `rotation` — so a
    // rotation read would hand back the authored value unchanged and the test
    // would fail against a perfectly correct engine.
    let ang = match ml
        .tree()
        .get_node(id)
        .map(|n| n.get_property("angular_velocity"))
    {
        Some(Variant::Vector3(v)) => v,
        other => panic!("angular_velocity must be a Vector3, got {other:?}"),
    };
    assert_eq!(
        ang.y, 0.0,
        "axis_lock_angular_y must freeze spin about Y, got {}",
        ang.y
    );
    assert!(
        ang.x.abs() > 1e-4 || ang.z.abs() > 1e-4,
        "…and must NOT behave as lock_rotation: the unlocked axes must still \
         spin, got {ang:?}"
    );
}
