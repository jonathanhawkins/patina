//! pat-s3rrv.94 — `RigidBody2D._integrate_forces(state)` runs once per physics
//! tick when `custom_integrator` is on, and what the script writes to the state
//! steers the body.
//!
//! ⚠️ The velocity override is asserted through **motion along +X**. Gravity
//! pulls along +Y, so no amount of default integration moves the body sideways:
//! a positive X displacement can only come from the velocity the script wrote.
//! Asserting "the position changed" instead would pass on gravity alone and
//! prove nothing about the callback.
//!
//! ⚠️ **Corrected by pat-mgixk.38.** This file used to state that Godot gates
//! the callback on `custom_integrator` rather than on whether the method is
//! overridden, and the 2D control asserted that a body defining
//! `_integrate_forces` without opting in is never called. That is not Godot's
//! behaviour: `RigidBody2D::_body_state_changed` (rigid_body_2d.cpp:163) calls
//! `_integrate_forces` whenever `GDVIRTUAL_IS_OVERRIDDEN`, and
//! `custom_integrator` only tells the SERVER to omit its own gravity and
//! damping (`body_set_omit_force_integration`). The 2D control below now
//! asserts the real rule; the dispatch still shares one script with the
//! opted-in case, which is what tells "dispatch works" apart from "the script
//! never loaded".
//!
//! ⚠️ The **3D** half of this file still asserts the old gate, because
//! `run_integrate_forces_3d` still carries the same mistake
//! (`integrate_forces.rs:245`). pat-mgixk.38 is scoped to 2D
//! (`subsys:physics-2d`), so the 3D tests are deliberately left alone rather
//! than changed ahead of the fix they describe.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
// `get_property` is a `ScriptInstance` trait method, so the trait has to be in
// scope to call it — the same import the sibling script tests carry.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Counts its callbacks and drives the body sideways at a fixed speed.
const SCRIPT: &str = "\
extends RigidBody2D
var ticks = 0

func _integrate_forces(state):
    self.ticks = self.ticks + 1
    state.linear_velocity = Vector2(120, 0)
";

const PHYSICS_DT: f64 = 1.0 / 60.0;

fn scene_with(custom_integrator: bool) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("custom_integrator", Variant::Bool(custom_integrator));
    let id = tree.add_child(root, body).expect("the body attaches");
    let script = GDScriptNodeInstance::from_source(SCRIPT, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    (MainLoop::new(tree), id)
}

fn ticks(ml: &MainLoop, id: NodeId) -> i64 {
    match ml
        .tree()
        .get_script(id)
        .and_then(|s| s.get_property("ticks"))
    {
        Some(Variant::Int(n)) => n,
        other => panic!("the script must expose a `ticks` counter, got {other:?}"),
    }
}

fn position(ml: &MainLoop, id: NodeId) -> gdcore::math::Vector2 {
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v,
        other => panic!("the body should carry a Vector2 position, got {other:?}"),
    }
}

/// **The acceptance test.** Callback cadence, the gate, and the velocity write.
#[test]
fn rigidbody_custom_integrator_direct_body_state() {
    let (mut ml, id) = scene_with(true);

    assert_eq!(
        ticks(&ml, id),
        0,
        "nothing should have run before the first frame",
    );

    // One frame of exactly one physics step.
    ml.step(PHYSICS_DT);
    assert_eq!(
        ticks(&ml, id),
        1,
        "_integrate_forces must run exactly once per physics tick; 0 means the \
         callback is never dispatched, more than 1 means it runs per frame or \
         per body-shape instead",
    );

    for _ in 0..4 {
        ml.step(PHYSICS_DT);
    }
    assert_eq!(
        ticks(&ml, id),
        5,
        "five physics ticks must produce five callbacks",
    );

    // 🔑 Only the overridden velocity can move the body along +X — gravity is
    // along +Y. Five ticks at 120 px/s is ~10px.
    let p = position(&ml, id);
    assert!(
        p.x > 5.0,
        "the body must move along +X at the velocity the script wrote to the \
         state; got {p:?}. An x of 0 means the write never reached the body — \
         the state was a dead copy",
    );
}

/// 🔑 Dispatch follows the OVERRIDE, not `custom_integrator`.
///
/// This test asserted the opposite until pat-mgixk.38 — that the same script
/// without `custom_integrator` is never called. Conflating the two silently
/// broke the most common idiom there is: an `_integrate_forces` override left
/// at the default `custom_integrator = false` was never dispatched at all, and
/// nothing reported an error because the body simply was not a target.
#[test]
fn the_callback_runs_without_custom_integrator() {
    let (mut ml, id) = scene_with(false);
    for _ in 0..5 {
        ml.step(PHYSICS_DT);
    }

    assert_eq!(
        ticks(&ml, id),
        5,
        "a script that OVERRIDES `_integrate_forces` must be called every \
         physics tick even with `custom_integrator` off; 0 means dispatch is \
         still gated on the property",
    );
    let p = position(&ml, id);
    assert!(
        p.x > 5.0,
        "…and the velocity the script wrote must still reach the body; got {p:?}",
    );
    // 🔑 The property's REAL job, asserted from the other side: with it off the
    // server still integrates its own gravity, so the body falls while the
    // script steers it sideways. The fall is small because the script rewrites
    // the whole velocity every tick, so only one tick of gravity is ever
    // carried — but it must not be zero.
    assert!(
        p.y > 0.0,
        "with `custom_integrator` off the server's gravity must still apply, so \
         the body falls as it moves; got {p:?}. A y of exactly 0 means gravity \
         was omitted for a body that never asked for it",
    );
}

// ---------------------------------------------------------------------------
// 3D (acceptance 5)
// ---------------------------------------------------------------------------

/// The 3D twin of `SCRIPT`. Gravity in 3D pulls along -Y, so driving the body
/// along +X again keeps "only the override can produce this" true.
const SCRIPT_3D: &str = "\
extends RigidBody3D
var ticks = 0

func _integrate_forces(state):
    self.ticks = self.ticks + 1
    state.linear_velocity = Vector3(120, 0, 0)
";

fn scene_3d_with(custom_integrator: bool) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut body = Node::new("Body3D", "RigidBody3D");
    body.set_property("custom_integrator", Variant::Bool(custom_integrator));
    let id = tree.add_child(root, body).expect("the body attaches");
    let script = GDScriptNodeInstance::from_source(SCRIPT_3D, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
    (MainLoop::new(tree), id)
}

/// ⚠️ Read through `node3d::get_position`, not the raw `position` property.
/// Once a node carries a `transform`, `local_transform_of` prefers it and the
/// bare `position` key goes stale — the same rule that made a raw positional
/// write a silent no-op in pat-s3rrv.93.
fn position_3d(ml: &MainLoop, id: NodeId) -> gdcore::math::Vector3 {
    gdscene::node3d::get_position(ml.tree(), id)
}

/// 🔑 The 3D sibling: cadence, the gate, and the velocity write.
#[test]
fn rigidbody_3d_custom_integrator_direct_body_state() {
    let (mut ml, id) = scene_3d_with(true);

    ml.step(PHYSICS_DT);
    assert_eq!(
        ticks(&ml, id),
        1,
        "_integrate_forces must reach a RigidBody3D once per physics tick too; \
         0 means only the 2D pass is wired",
    );

    for _ in 0..4 {
        ml.step(PHYSICS_DT);
    }
    assert_eq!(ticks(&ml, id), 5, "five ticks, five callbacks");

    // Gravity is along -Y, so +X travel can only come from the script's write.
    let p = position_3d(&ml, id);
    assert!(
        p.x > 5.0,
        "the body must move along +X at the velocity written to the 3D state; \
         got {p:?}. An x of 0 means the write never reached the body",
    );
}

/// 🔑 The 3D gate, using the same script.
#[test]
fn without_custom_integrator_the_3d_callback_never_runs() {
    let (mut ml, id) = scene_3d_with(false);
    for _ in 0..5 {
        ml.step(PHYSICS_DT);
    }

    assert_eq!(
        ticks(&ml, id),
        0,
        "the 3D path still gates the callback on `custom_integrator`. \
         ⚠️ This is the OLD, wrong rule — Godot dispatches on the override \
         (`rigid_body_3d.cpp` `_body_state_changed`) — and it is asserted here \
         only because pat-mgixk.38 is scoped to 2D. When the 3D twin of that \
         bead lands, this becomes `ticks == 5` like its 2D counterpart",
    );
    let p = position_3d(&ml, id);
    assert!(
        p.x.abs() < 1e-3,
        "…and with no callback there is no sideways velocity; got {p:?}",
    );
}

/// 🔑 The two handle families do not cross-dispatch.
///
/// ⚠️ A 2D body's state must answer as a Vector2 and a 3D body's as a Vector3.
/// If the tag check were wrong, the router would fall through to the other
/// family, find no such handle, and quietly report zero — which looks exactly
/// like "the callback did not run" rather than like a routing bug.
#[test]
fn two_d_and_three_d_states_do_not_cross_dispatch() {
    let (mut ml2, id2) = scene_with(true);
    ml2.step(PHYSICS_DT);
    let p2 = position(&ml2, id2);

    let (mut ml3, id3) = scene_3d_with(true);
    ml3.step(PHYSICS_DT);
    let p3 = position_3d(&ml3, id3);

    assert!(
        p2.x > 0.0,
        "the 2D body moved, so its state routed to the 2D reader; got {p2:?}",
    );
    assert!(
        p3.x > 0.0,
        "the 3D body moved, so its state routed to the 3D reader; got {p3:?}",
    );
}

// ---------------------------------------------------------------------------
// Acceptance item 4: contact reporting through the state
// ---------------------------------------------------------------------------

/// Drives itself down onto the floor and reports what the state says it touched.
///
/// ⚠️ `contacts` starts at -1, NOT 0. Zero is also the honest answer for a body
/// touching nothing, so a 0 initialiser cannot tell "the callback never ran"
/// from "it ran and saw nothing" — and a handler that raises leaves exactly the
/// initialiser behind.
///
/// ⚠️ The body drives ITSELF downward rather than relying on gravity, because
/// `custom_integrator = true` is precisely the flag that turns the built-in
/// force integration off. A fixture that waited for gravity here would hang in
/// mid-air and report zero contacts for a reason unrelated to contact
/// reporting.
const CONTACT_SCRIPT: &str = "\
extends RigidBody2D
var contacts = -1
var ny = 0.0

func _integrate_forces(state):
    state.linear_velocity = Vector2(0, 200)
    contacts = state.get_contact_count()
    if contacts > 0:
        var n = state.get_contact_local_normal(0)
        ny = n.y
";

/// A floor at y = 100 (half-height 10, so its top surface is y = 90) with a
/// monitoring body driven onto it.
fn contact_scene(contact_monitor: bool) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 100.0)),
    );
    let floor_id = tree.add_child(root, floor).expect("floor attaches");
    let mut floor_shape = Node::new("FloorShape", "CollisionShape2D");
    floor_shape.set_property(
        "size",
        Variant::Vector2(gdcore::math::Vector2::new(400.0, 20.0)),
    );
    tree.add_child(floor_id, floor_shape).expect("floor shape");

    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("custom_integrator", Variant::Bool(true));
    // Godot gates contact reporting on BOTH flags; the control below flips the
    // first one only.
    body.set_property("contact_monitor", Variant::Bool(contact_monitor));
    body.set_property("max_contacts_reported", Variant::Int(4));
    let id = tree.add_child(root, body).expect("the body attaches");
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(id, shape).expect("body shape");

    let script = GDScriptNodeInstance::from_source(CONTACT_SCRIPT, id).expect("script parses");
    tree.attach_script(id, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, id)
}

fn script_int(ml: &MainLoop, id: NodeId, name: &str) -> i64 {
    match ml.tree().get_script(id).and_then(|s| s.get_property(name)) {
        Some(Variant::Int(i)) => i,
        other => panic!("{name} should be an Int, got {other:?}"),
    }
}

/// **Acceptance item 4.** A monitoring body resting on a floor reports its
/// contacts through the state, with a normal pointing away from the floor.
#[test]
fn the_state_reports_contacts_for_a_monitoring_body() {
    let (mut ml, id) = contact_scene(true);
    for _ in 0..60 {
        ml.step(PHYSICS_DT);
    }

    let contacts = script_int(&ml, id, "contacts");
    assert!(
        contacts >= 1,
        "a body driven onto a floor with contact_monitor on must report at \
         least one contact; got {contacts} (-1 means the callback never ran)"
    );

    // ⚠️ Up is -Y in Godot's 2D convention, so a normal pointing AWAY from a
    // floor underneath the body is negative. Publishing the raw collision
    // normal would report +Y here and silently invert the standard floor test.
    let ny = match ml.tree().get_script(id).and_then(|s| s.get_property("ny")) {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("ny should be a number, got {other:?}"),
    };
    assert!(
        ny < -0.5,
        "the contact normal must point away from the floor (up, i.e. -Y); got \
         y = {ny}"
    );
}

/// The control: Godot reports nothing unless the body opted in. Without this a
/// `get_contact_count()` that ignored the flag would pass the test above.
#[test]
fn a_body_without_contact_monitor_reports_no_contacts() {
    let (mut ml, id) = contact_scene(false);
    for _ in 0..60 {
        ml.step(PHYSICS_DT);
    }
    assert_eq!(
        script_int(&ml, id, "contacts"),
        0,
        "contact_monitor is off, so the state must report zero contacts even \
         though the body is visibly resting on the floor"
    );
}
