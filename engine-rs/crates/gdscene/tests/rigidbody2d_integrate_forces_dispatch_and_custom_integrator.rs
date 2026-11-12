//! pat-mgixk.38 acceptance: `_integrate_forces` is dispatched on OVERRIDE, and
//! `custom_integrator` is what omits the server's gravity and damping.
//!
//! # What was wrong
//!
//! Two separate things were fused into one flag.
//!
//! Godot calls `_integrate_forces` whenever a script overrides it —
//! `RigidBody2D::_body_state_changed` (`rigid_body_2d.cpp:163`) is guarded by
//! `GDVIRTUAL_IS_OVERRIDDEN` and never consults `custom_integrator`. The flag
//! instead reaches the server as `body_set_omit_force_integration`, where
//! `GodotBody2D::integrate_forces` uses it to skip ITS OWN gravity and damping
//! while still integrating forces the script applied.
//!
//! Patina had it backwards and incomplete at the same time:
//!
//! * `run_integrate_forces_2d` dispatched only when `custom_integrator == true`,
//!   so the ordinary idiom — override the method, leave the flag alone — was
//!   never called. Nothing errored; the body simply was not a target.
//! * Nothing in `gdphysics2d` read `custom_integrator` at all, so turning it on
//!   omitted nothing and the body kept falling.
//!
//! Each bug hid the other: a body with the flag on got its callback, and a body
//! with the flag off kept falling, so both halves looked plausible as long as
//! nobody wrote the combination a real game writes.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> PackedScene -> SceneTree -> attached GDScript
//!   -> MainLoop::step -> run_integrate_forces_2d -> _integrate_forces
//!     -> apply_state -> sync_to_physics -> PhysicsWorld2D::step
//!       -> the body's position, as a game observes it
//! ```
//!
//! Both bodies are authored in a `.tscn` the way a scene does it, including
//! `mass`, `inertia` and `custom_integrator`, and every assertion reads the
//! node's own `position` after the loop has run.

use gdcore::math::Vector2;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// 60 Hz, one physics tick per frame.
const PHYSICS_DT: f64 = 1.0 / 60.0;
/// A full second of simulation, as the bead's acceptance asks for.
const TICKS: usize = 60;
/// The authored inertia. `get_inverse_inertia()` must answer its reciprocal.
const INERTIA: f32 = 4.0;

/// A body that overrides `_integrate_forces` and leaves `custom_integrator` at
/// its default — the shape of nearly every real RigidBody2D script.
const SCENE_DEFAULT: &str = r#"[gd_scene format=3]

[node name="Rig" type="Node2D"]

[node name="Body" type="RigidBody2D" parent="."]
position = Vector2(0, 0)
mass = 1.0
inertia = 4.0

[node name="Shape" type="CollisionShape2D" parent="Body"]
radius = 8.0
"#;

/// The same body with `custom_integrator` switched on.
const SCENE_CUSTOM: &str = r#"[gd_scene format=3]

[node name="Rig" type="Node2D"]

[node name="Body" type="RigidBody2D" parent="."]
position = Vector2(0, 0)
mass = 1.0
inertia = 4.0
custom_integrator = true

[node name="Shape" type="CollisionShape2D" parent="Body"]
radius = 8.0
"#;

/// Steers the body sideways while leaving the vertical velocity alone, so
/// gravity can still accumulate into it.
///
/// ⚠️ The y component is READ BACK and rewritten rather than zeroed. Writing
/// `Vector2(100, 0)` would clobber the gravity the server had just added, and
/// the body would then barely fall whether or not integration was omitted —
/// which is precisely the distinction this file exists to measure.
///
/// ⚠️ The read goes through a local `v` rather than chaining
/// `state.linear_velocity.y`. Component access directly off the state object is
/// used nowhere else in the suite, and a harness that fails on an unproven
/// construct looks exactly like the engine bug under test. Reading `.y` off a
/// plain local is the form every script already relies on.
const STEER_GD: &str = "\
extends RigidBody2D
var ticks = 0
var inv_inertia = -1.0

func _integrate_forces(state):
    self.ticks = self.ticks + 1
    self.inv_inertia = state.get_inverse_inertia()
    var v = state.linear_velocity
    state.linear_velocity = Vector2(100, v.y)
";

/// Overrides the callback but writes nothing: with `custom_integrator` on, a
/// body like this must simply hang where it is.
///
/// ⚠️ It still counts its calls. A truly empty body would leave "the body did
/// not fall" ambiguous between "integration was omitted" and "the callback
/// never ran and something else held it still".
const INERT_GD: &str = "\
extends RigidBody2D
var ticks = 0

func _integrate_forces(state):
    self.ticks = self.ticks + 1
";

/// Loads `scene`, attaches `script` to its `Body`, and returns the running loop.
fn run(scene: &str, script: &str) -> (MainLoop, NodeId) {
    let packed = PackedScene::from_tscn(scene).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("the scene instances");
    let body = tree
        .get_node_by_path("/root/Rig/Body")
        .expect("the body is in the tree");

    let instance = GDScriptNodeInstance::from_source(script, body).expect("the script parses");
    tree.attach_script(body, Box::new(instance));

    let mut ml = MainLoop::new(tree);
    for _ in 0..TICKS {
        ml.step(PHYSICS_DT);
    }
    (ml, body)
}

fn position(ml: &MainLoop, id: NodeId) -> Vector2 {
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v,
        other => panic!("the body should carry a Vector2 position, got {other:?}"),
    }
}

fn script_float(ml: &MainLoop, id: NodeId, name: &str) -> f32 {
    match ml.tree().get_script(id).and_then(|s| s.get_property(name)) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        other => panic!("`{name}` should be a number, got {other:?}"),
    }
}

fn script_int(ml: &MainLoop, id: NodeId, name: &str) -> i64 {
    match ml.tree().get_script(id).and_then(|s| s.get_property(name)) {
        Some(Variant::Int(n)) => n,
        other => panic!("`{name}` should be an Int, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn rigidbody2d_integrate_forces_dispatch_and_custom_integrator() {
    // -- (1) the default idiom: overridden, flag untouched ------------------
    let (ml, body) = run(SCENE_DEFAULT, STEER_GD);

    assert_eq!(
        script_int(&ml, body, "ticks"),
        TICKS as i64,
        "an overridden `_integrate_forces` must be called once per physics \
         tick with `custom_integrator` at its default `false`. 0 means dispatch \
         is still gated on the flag, which skipped the single most common way \
         this callback is written."
    );

    let p = position(&ml, body);
    assert!(
        p.x > 50.0,
        "the body must move along +X at the velocity the script wrote — only \
         the callback can produce sideways motion, since gravity is along +Y. \
         Got {p:?}"
    );
    assert!(
        p.y > 50.0,
        "…and it must STILL FALL, because `custom_integrator` is off and the \
         server's own gravity therefore still applies. Got {p:?}. A y near 0 \
         would mean integration was omitted for a body that never asked."
    );

    // -- the state's inertia, which had no accessor at all ------------------
    //
    // `get_inverse_inertia()` was registered in ClassDB but fell through to
    // `Nil` in the interpreter, so this read answered nothing while the
    // property form `state.inverse_inertia` answered the number.
    let inv = script_float(&ml, body, "inv_inertia");
    assert!(
        (inv - 1.0 / INERTIA).abs() < 1e-4,
        "`state.get_inverse_inertia()` must be 1/inertia = {} for the authored \
         inertia of {INERTIA}; got {inv}. -1 means the script never ran, and 0 \
         means the method answered Nil and the default stuck.",
        1.0 / INERTIA
    );

    // -- (2) custom_integrator on: the callback runs, gravity does not ------
    let (ml, body) = run(SCENE_CUSTOM, INERT_GD);

    assert_eq!(
        script_int(&ml, body, "ticks"),
        TICKS as i64,
        "the callback must still be dispatched with `custom_integrator` on — \
         the flag was never about dispatch"
    );

    let p = position(&ml, body);
    assert!(
        p.y.abs() < 1e-3,
        "`custom_integrator` must omit the server's gravity, so a body whose \
         callback writes nothing stays exactly where it was authored. Got \
         {p:?}. Before this bead nothing in gdphysics2d read the flag at all, \
         so the body fell the full second — about 490px."
    );
    assert!(
        p.x.abs() < 1e-3,
        "…and nothing moved it sideways either; got {p:?}"
    );
}

/// The flag must not disable the forces a script applies — only the server's
/// own gravity and damping.
///
/// ⚠️ This is the half that a "skip the whole integrate phase when omitting"
/// shortcut would break, and it would break it silently: `apply_central_impulse`
/// is exactly what a custom integrator is written to call, so a body would stop
/// responding to the only API it uses.
#[test]
fn custom_integrator_still_integrates_applied_forces() {
    const PUSH_GD: &str = "\
extends RigidBody2D
var ticks = 0

func _integrate_forces(state):
    self.ticks = self.ticks + 1
    if self.ticks == 1:
        state.apply_central_impulse(Vector2(60, 0))
";

    let (ml, body) = run(SCENE_CUSTOM, PUSH_GD);

    assert_eq!(
        script_int(&ml, body, "ticks"),
        TICKS as i64,
        "the callback must run every tick"
    );
    let p = position(&ml, body);
    assert!(
        p.x > 10.0,
        "a single impulse applied through the state must still move the body \
         under `custom_integrator`; got {p:?}. An x of 0 means omitting the \
         default forces also dropped the applied ones."
    );
    assert!(
        p.y.abs() < 1e-3,
        "…while gravity stays omitted, so the body travels flat; got {p:?}"
    );
}
