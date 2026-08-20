//! pat-s3rrv.191 acceptance: the `PhysicsServer2D` script singleton reaches
//! beyond the nine staged methods — body params, forces, full state reads, and
//! areas.
//!
//! `call_physics_server_2d_method` had exactly nine arms and everything else
//! raised `UndefinedFunction`. Worse, `body_get_state` answered **Nil for every
//! state except TRANSFORM**, so a script could apply an impulse and had no way
//! to observe that anything had happened.
//!
//! ⚠️ **A constant-table bug fixed here, not merely worked around.** The shipped
//! code mapped `BODY_STATE_LINEAR_VELOCITY` to **2** — in a comment above
//! `body_get_state` and in `server_body_set_state_vec2`'s match. Godot's value
//! is **1**; 2 is ANGULAR_VELOCITY
//! (`servers/physics_2d/physics_server_2d.h`). So `body_set_state(rid, 1, v)`
//! returned false and did nothing, while `(rid, 2, v)` wrote the linear
//! velocity a script had not asked for. Both are corrected, and this file pins
//! the corrected values.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::GDScriptNodeInstance;
use gdscene::SceneTree;
// ⚠️ `list_properties` is a TRAIT method on `ScriptInstance`; without the trait
// in scope `get_script(..).list_properties()` does not resolve. The reference
// test (`physics_server_2d_singleton_script_api.rs`) imports it for the same
// reason.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Creates a body through the server, gives it a capsule, sets its mass, and
/// launches it — then reads the velocity back.
///
/// 🔑 The expected `vel_x` is **50**, not "non-zero". `apply_impulse` is
/// `linear_velocity += impulse * (1 / mass)`, so an impulse of 100 with the
/// authored mass of 2 gives exactly 50 — while the DEFAULT mass of 1 would give
/// 100 and a dropped impulse would give 0. One number separates three distinct
/// failures; the acceptance's own "non-zero" wording would accept the case where
/// `body_set_param` silently did nothing, which is the bug this bead exists to
/// fix.
///
/// ⚠️ `shape_set_data(shape, Vector2(radius, height))` is the real Godot
/// spelling for a capsule and is deliberately used here. It is indistinguishable
/// at the interpreter from a rectangle's half-extents, so the server has to
/// disambiguate on the shape the RID already names — a bespoke capsule setter
/// would have passed this test while the spelling every script uses stayed
/// broken.
const BODY_DRIVER_GD: &str = "\
extends Node2D
var made = false
var mass_read = 0.0
var vel_x = 0.0
var vel_y = 0.0
var layer_ok = false
var ticks = 0
func _physics_process(delta):
    if made == false:
        made = true
        var shape = PhysicsServer2D.capsule_shape_create()
        PhysicsServer2D.shape_set_data(shape, Vector2(8.0, 32.0))
        var body = PhysicsServer2D.body_create()
        PhysicsServer2D.body_set_space(body, 1)
        PhysicsServer2D.body_add_shape(body, shape)
        layer_ok = PhysicsServer2D.body_set_collision_layer(body, 1)
        PhysicsServer2D.body_set_param(body, PhysicsServer2D.BODY_PARAM_MASS, 2.0)
        mass_read = PhysicsServer2D.body_get_param(body, PhysicsServer2D.BODY_PARAM_MASS)
        PhysicsServer2D.body_apply_central_impulse(body, Vector2(100.0, 0.0))
        var v = PhysicsServer2D.body_get_state(body, PhysicsServer2D.BODY_STATE_LINEAR_VELOCITY)
        vel_x = v.x
        vel_y = v.y
    ticks = ticks + 1
";

/// Builds an area with a gravity override and drops a body into it.
///
/// ⚠️ The body's RID is kept in `body_rid` rather than assumed. RIDs come from
/// one counter in creation order, so the literal `1` here is the AREA's shape,
/// not the body — `body_get_state` then answers Nil and `v.y` raises, killing
/// the script mid-tick. That is what the `ticks` guard caught on the first run.
///
/// ⚠️ `AREA_PARAM_GRAVITY` alone changes nothing: the override MODE defaults to
/// `DISABLED` on both Godot's side and ours, so an area that merely names a
/// gravity is not overriding anything yet. Setting the mode explicitly is the
/// difference between a test that proves the feature and one that proves the
/// default.
const AREA_DRIVER_GD: &str = "\
extends Node2D
var made = false
var area_ok = false
var param_ok = false
var body_vy = 0.0
var body_rid = 0
var ticks = 0
func _physics_process(delta):
    if made == false:
        made = true
        var ashape = PhysicsServer2D.rectangle_shape_create()
        PhysicsServer2D.shape_set_data(ashape, Vector2(400.0, 400.0))
        var area = PhysicsServer2D.area_create()
        area_ok = PhysicsServer2D.area_set_space(area, 1)
        PhysicsServer2D.area_add_shape(area, ashape)
        PhysicsServer2D.area_set_transform(area, Vector2(0.0, 0.0))
        PhysicsServer2D.area_set_param(area, PhysicsServer2D.AREA_PARAM_GRAVITY_OVERRIDE_MODE, PhysicsServer2D.AREA_SPACE_OVERRIDE_REPLACE)
        param_ok = PhysicsServer2D.area_set_param(area, PhysicsServer2D.AREA_PARAM_GRAVITY, 0.0)
        var bshape = PhysicsServer2D.circle_shape_create()
        PhysicsServer2D.shape_set_data(bshape, 8.0)
        body_rid = PhysicsServer2D.body_create()
        PhysicsServer2D.body_set_space(body_rid, 1)
        PhysicsServer2D.body_add_shape(body_rid, bshape)
        PhysicsServer2D.body_set_state(body_rid, PhysicsServer2D.BODY_STATE_TRANSFORM, Vector2(0.0, 0.0))
    else:
        var v = PhysicsServer2D.body_get_state(body_rid, PhysicsServer2D.BODY_STATE_LINEAR_VELOCITY)
        body_vy = v.y
    ticks = ticks + 1
";

fn scene_with(script: &str) -> (MainLoop, NodeId) {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Driver", "Node2D")).unwrap();
    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(script, id).unwrap()),
    );
    (MainLoop::new(tree), id)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn script_float(ml: &MainLoop, node: NodeId, name: &str) -> f64 {
    match script_var(ml, node, name) {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("{name} should be a number, got {other:?}"),
    }
}

/// **The acceptance test.** Body params, an impulse, and a readable velocity.
#[test]
fn physics_server_2d_singleton_area_and_joint_api() {
    // ClassDB reports the singleton — the acceptance names this explicitly.
    gdobject::class_db::register_all_classes();
    assert!(
        gdobject::class_db::class_exists("PhysicsServer2D"),
        "PhysicsServer2D must be a registered class"
    );

    let (mut ml, id) = scene_with(BODY_DRIVER_GD);
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);

    // 🔑 The script ran to completion. Every statement after an
    // `UndefinedFunction` is silently skipped, so a partial run looks exactly
    // like a wrong value — `ticks` rising is what separates the two.
    assert!(
        script_float(&ml, id, "ticks") >= 2.0,
        "the driver script must survive both ticks without a RuntimeError"
    );

    // `body_set_collision_layer` reported success against a real RID.
    assert!(
        matches!(script_var(&ml, id, "layer_ok"), Some(Variant::Bool(true))),
        "body_set_collision_layer must resolve the RID"
    );
    // `body_get_param` reads back what `body_set_param` wrote.
    assert!(
        (script_float(&ml, id, "mass_read") - 2.0).abs() < 1e-4,
        "body_get_param(MASS) must read back the authored 2.0, got {}",
        script_float(&ml, id, "mass_read")
    );

    // The derived value: impulse 100 / mass 2.
    let vel_x = script_float(&ml, id, "vel_x");
    assert!(
        (vel_x - 50.0).abs() < 1e-3,
        "expected 50 (impulse 100 / mass 2); 100 would mean body_set_param was \
         ignored and 0 that the impulse never landed — got {vel_x}"
    );
}

/// An area's gravity override reaches a server-created body.
#[test]
fn an_area_gravity_override_reaches_a_server_body() {
    let (mut ml, id) = scene_with(AREA_DRIVER_GD);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        script_float(&ml, id, "ticks") >= 4.0,
        "the area driver must survive its ticks"
    );
    assert!(
        matches!(script_var(&ml, id, "area_ok"), Some(Variant::Bool(true))),
        "area_set_space must admit the area to the scene's space"
    );
    assert!(
        matches!(script_var(&ml, id, "param_ok"), Some(Variant::Bool(true))),
        "area_set_param must resolve the area RID"
    );

    // ⚠️ The override sets gravity to ZERO and REPLACE, so the body must NOT
    // accumulate downward velocity. A body outside any area falls at the world
    // default, so "stayed at rest" is only achievable through the override —
    // the reverse assertion (expecting motion) would pass on a default fall.
    let vy = script_float(&ml, id, "body_vy");
    assert!(
        vy.abs() < 1.0,
        "an area with REPLACE + gravity 0 must cancel the world gravity, got vy = {vy}"
    );
}
