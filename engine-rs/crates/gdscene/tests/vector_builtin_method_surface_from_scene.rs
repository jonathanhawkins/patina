//! pat-s3rrv.9 acceptance: the Vector2/Vector3/Vector2i/Vector3i method and
//! constant surface is reachable from a running scene script.
//!
//! The interpreter's tables implemented 8 Vector2 methods and 7 Vector3
//! methods, and `Variant::Vector2i`/`Vector3i` had **no method arm at all** —
//! they fell through to the generic "cannot call method on" error. So the
//! methods a real game calls every frame — `velocity.move_toward(...)`,
//! `dir.rotated(...)`, `pos.snapped(...)`, `n.slide(...)` — each raised
//! `UndefinedFunction("Vector2.…")`, which **aborts the running script**: a
//! `_physics_process` hitting one stops updating the node entirely.
//!
//! This drives the whole path the bead names — a scene node carrying a script
//! whose `_physics_process` calls these methods and writes the result back onto
//! the node — and asserts the *node's* properties after a frame step, so a pass
//! proves VM wiring plus write-back rather than the presence of a `gdcore`
//! function.
//!
//! Every expected number below was cross-checked against Godot's formulas
//! (`upstream/godot/core/math/vector2.h`) before this test was written, because
//! several are easy to get plausibly wrong: `reflect` is `2 * n * dot(n) - v`
//! (a mirror), `snapped` rounds via `floor(v / step + 0.5)`, and `round` is
//! half-away-from-zero.

use gdcore::math::{Vector2, Vector2i, Vector3, Vector3i};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const EPS: f32 = 1e-4;

/// Builds a `World` Node2D carrying `script`, enters the tree, and steps one
/// physics frame — the "load a scene, step once" shape the bead asks for.
fn scene_stepped_once(script: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut world = Node::new("World", "Node2D");
    // BOTH properties must exist and be non-Nil before the script runs.
    // `exec_assignment` only treats a bare `name = value` as a NODE property
    // write when `get_node_property(node, name)` is already non-Nil; otherwise
    // the assignment silently lands in the script environment instead and the
    // node is never touched. Seeding only `position` would make the `rotation`
    // assertion below read Nil while the math was perfectly correct.
    world.set_property("position", Variant::Vector2(Vector2::ZERO));
    world.set_property("rotation", Variant::Float(0.0));
    let world = tree.add_child(root, world).unwrap();

    // Attached after `add_child` so the premature enter/ready that `add_child`
    // performs cannot run `_ready` before the script exists.
    tree.attach_script(
        world,
        Box::new(GDScriptNodeInstance::from_source(script, world).expect("script parses")),
    );

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(world);
    ml.step(1.0 / 60.0);
    (ml, world)
}

/// A script variable, read back off the live script instance.
fn script_var(ml: &MainLoop, node: NodeId, prop: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

fn node_vec2(ml: &MainLoop, node: NodeId, prop: &str) -> Vector2 {
    match ml.tree().get_node(node).unwrap().get_property(prop) {
        Variant::Vector2(v) => v,
        other => panic!("{prop} should be a Vector2, got {other:?}"),
    }
}

fn node_float(ml: &MainLoop, node: NodeId, prop: &str) -> f32 {
    match ml.tree().get_node(node).unwrap().get_property(prop) {
        Variant::Float(f) => f as f32,
        Variant::Int(i) => i as f32,
        other => panic!("{prop} should be a number, got {other:?}"),
    }
}

#[track_caller]
fn assert_vec2(got: &Variant, x: f32, y: f32, what: &str) {
    match got {
        Variant::Vector2(v) => assert!(
            (v.x - x).abs() < EPS && (v.y - y).abs() < EPS,
            "{what}: expected ({x}, {y}), got ({}, {})",
            v.x,
            v.y,
        ),
        other => panic!("{what}: expected a Vector2, got {other:?}"),
    }
}

#[track_caller]
fn assert_vec3(got: &Variant, x: f32, y: f32, z: f32, what: &str) {
    match got {
        Variant::Vector3(v) => assert!(
            (v.x - x).abs() < EPS && (v.y - y).abs() < EPS && (v.z - z).abs() < EPS,
            "{what}: expected ({x}, {y}, {z}), got ({}, {}, {})",
            v.x,
            v.y,
            v.z,
        ),
        other => panic!("{what}: expected a Vector3, got {other:?}"),
    }
}

/// Exercises every method the bead's acceptance names, then writes the result
/// onto the node so the frame step has something observable to prove.
///
/// `_physics_process` is deliberate: it is the handler the bead's wiring
/// requirement describes, and it is where an `UndefinedFunction` would silently
/// freeze a real game's movement.
const MOVER: &str = "\
extends Node2D
var moved = Vector2.ZERO
var rotated_v = Vector2.ZERO
var slid = Vector2.ZERO
var reflected = Vector2.ZERO
var limited = Vector2.ZERO
var direction = Vector2.ZERO
var snapped_v = Vector2.ZERO
var absed = Vector2.ZERO
var floored = Vector2.ZERO
var rounded = Vector2.ZERO
var angle = 0.0
var from_angle_v = Vector2.ZERO
func _physics_process(delta):
    self.moved = Vector2(10, 0).move_toward(Vector2.ZERO, 4.0)
    self.rotated_v = Vector2(1, 0).rotated(1.5707963267948966)
    self.slid = Vector2(1, 1).slide(Vector2(0, 1))
    self.reflected = Vector2(1, 1).reflect(Vector2(0, 1))
    self.limited = Vector2(10, 0).limit_length(3.0)
    self.direction = Vector2(0, 0).direction_to(Vector2(0, 5))
    self.snapped_v = Vector2(17, 33).snapped(Vector2(16, 16))
    self.absed = Vector2(-3.5, 2.2).abs()
    self.floored = Vector2(-3.5, 2.7).floor()
    self.rounded = Vector2(-3.5, 2.7).round()
    self.angle = Vector2(1, 0).angle_to(Vector2(0, 1))
    self.from_angle_v = Vector2.from_angle(0.0)
    position = self.moved + self.snapped_v
    rotation = self.angle
";

/// **The bead's acceptance.** A scene script using the full method surface runs
/// a frame with no RuntimeError and lands the documented numbers on the node.
#[test]
fn vector_builtin_method_surface_from_scene() {
    let (ml, world) = scene_stepped_once(MOVER);

    // -- the node's own properties, written back from the script -------------
    // (10,0) move_toward ZERO by 4 = (6,0); (17,33) snapped to a 16 grid =
    // (16,32); their sum is what `position` must hold.
    let pos = node_vec2(&ml, world, "position");
    assert!(
        (pos.x - 22.0).abs() < EPS && (pos.y - 32.0).abs() < EPS,
        "position must be (22, 32) after the frame, got ({}, {})",
        pos.x,
        pos.y,
    );
    assert!(
        (node_float(&ml, world, "rotation") - std::f32::consts::FRAC_PI_2).abs() < EPS,
        "rotation must be the angle_to result (PI/2), got {}",
        node_float(&ml, world, "rotation"),
    );

    // -- each method individually, so a failure names the culprit ------------
    assert_vec2(&script_var(&ml, world, "moved"), 6.0, 0.0, "move_toward");
    assert_vec2(&script_var(&ml, world, "rotated_v"), 0.0, 1.0, "rotated");
    assert_vec2(&script_var(&ml, world, "slid"), 1.0, 0.0, "slide");
    // 2 * (0,1) * dot((1,1),(0,1)) - (1,1) = (0,2) - (1,1) = (-1,1).
    assert_vec2(&script_var(&ml, world, "reflected"), -1.0, 1.0, "reflect");
    assert_vec2(&script_var(&ml, world, "limited"), 3.0, 0.0, "limit_length");
    assert_vec2(
        &script_var(&ml, world, "direction"),
        0.0,
        1.0,
        "direction_to",
    );
    assert_vec2(&script_var(&ml, world, "snapped_v"), 16.0, 32.0, "snapped");
    assert_vec2(&script_var(&ml, world, "absed"), 3.5, 2.2, "abs");
    assert_vec2(&script_var(&ml, world, "floored"), -4.0, 2.0, "floor");
    // Half away from zero, matching Godot's Math::round — NOT banker's rounding.
    assert_vec2(&script_var(&ml, world, "rounded"), -4.0, 3.0, "round");
    assert_vec2(
        &script_var(&ml, world, "from_angle_v"),
        1.0,
        0.0,
        "from_angle",
    );
    match script_var(&ml, world, "angle") {
        Variant::Float(a) => assert!(
            (a as f32 - std::f32::consts::FRAC_PI_2).abs() < EPS,
            "angle_to must be PI/2, got {a}",
        ),
        other => panic!("angle should be a Float, got {other:?}"),
    }
}

/// `Vector3.BACK`/`RIGHT`/`LEFT` (and `INF`) simply did not exist — the float
/// `Vector3` type itself defined only ZERO/ONE/UP/DOWN/FORWARD, so these were
/// unresolved identifiers rather than wrong values.
const V3_CONSTS: &str = "\
extends Node2D
var back = Vector3.ZERO
var right = Vector3.ZERO
var left = Vector3.ZERO
var model_front = Vector3.ZERO
var slid3 = Vector3.ZERO
var moved3 = Vector3.ZERO
var infinite = false
func _physics_process(delta):
    self.back = Vector3.BACK
    self.right = Vector3.RIGHT
    self.left = Vector3.LEFT
    self.model_front = Vector3.MODEL_FRONT
    self.slid3 = Vector3(1, 1, 0).slide(Vector3(0, 1, 0))
    self.moved3 = Vector3(0, 0, 10).move_toward(Vector3.ZERO, 4.0)
    self.infinite = Vector3(1, 1, 1).is_finite()
";

#[test]
fn vector3_direction_constants_and_methods_resolve() {
    let (ml, world) = scene_stepped_once(V3_CONSTS);

    assert_vec3(
        &script_var(&ml, world, "back"),
        0.0,
        0.0,
        1.0,
        "Vector3.BACK",
    );
    assert_vec3(
        &script_var(&ml, world, "right"),
        1.0,
        0.0,
        0.0,
        "Vector3.RIGHT",
    );
    assert_vec3(
        &script_var(&ml, world, "left"),
        -1.0,
        0.0,
        0.0,
        "Vector3.LEFT",
    );
    // MODEL_FRONT is +Z: an imported model faces +Z, which is the OPPOSITE of
    // the world-space FORWARD (-Z). Conflating them is the classic bug here.
    assert_vec3(
        &script_var(&ml, world, "model_front"),
        0.0,
        0.0,
        1.0,
        "Vector3.MODEL_FRONT",
    );
    assert_vec3(
        &script_var(&ml, world, "slid3"),
        1.0,
        0.0,
        0.0,
        "Vector3.slide",
    );
    assert_vec3(
        &script_var(&ml, world, "moved3"),
        0.0,
        0.0,
        6.0,
        "Vector3.move_toward",
    );
    assert_eq!(
        script_var(&ml, world, "infinite"),
        Variant::Bool(true),
        "is_finite must answer true for a finite vector",
    );
}

/// The int vectors had no method arm and no constants at all, so every one of
/// these was a runtime abort rather than a wrong number.
const INT_VECS: &str = "\
extends Node2D
var sum2 = Vector2i.ZERO
var diff2 = Vector2i.ZERO
var abs2 = Vector2i.ZERO
var sign2 = Vector2i.ZERO
var len2 = 0.0
var sum3 = Vector3i.ZERO
var abs3 = Vector3i.ZERO
var clamped = Vector2i.ZERO
var neg2 = Vector2i.ZERO
func _physics_process(delta):
    self.sum2 = Vector2i(3, 4) + Vector2i(1, 1)
    self.diff2 = Vector2i(10, 10) - Vector2i(4, 3)
    self.abs2 = Vector2i(4, -9).abs()
    self.sign2 = Vector2i(4, -9).sign()
    self.len2 = Vector2i(3, 4).length()
    self.sum3 = Vector3i(1, 2, 3) + Vector3i(1, 1, 1)
    self.abs3 = Vector3i(2, -3, 4).abs()
    self.clamped = Vector2i(50, -50).clamp(Vector2i(0, 0), Vector2i(10, 10))
    self.neg2 = -Vector2i(5, -6)
";

#[test]
fn int_vector_methods_and_arithmetic_stay_integral() {
    let (ml, world) = scene_stepped_once(INT_VECS);

    let v2i = |name: &str| match script_var(&ml, world, name) {
        Variant::Vector2i(v) => v,
        other => panic!("{name} should be a Vector2i, got {other:?}"),
    };
    let v3i = |name: &str| match script_var(&ml, world, name) {
        Variant::Vector3i(v) => v,
        other => panic!("{name} should be a Vector3i, got {other:?}"),
    };

    // Staying a Vector2i (rather than promoting to a float Vector2) is the
    // whole point of a grid coordinate — the match arms above are the assertion.
    assert_eq!(v2i("sum2"), Vector2i::new(4, 5), "Vector2i + Vector2i");
    assert_eq!(v2i("diff2"), Vector2i::new(6, 7), "Vector2i - Vector2i");
    assert_eq!(v2i("abs2"), Vector2i::new(4, 9), "Vector2i.abs");
    assert_eq!(v2i("sign2"), Vector2i::new(1, -1), "Vector2i.sign");
    assert_eq!(v2i("clamped"), Vector2i::new(10, 0), "Vector2i.clamp");
    assert_eq!(v2i("neg2"), Vector2i::new(-5, 6), "unary -Vector2i");
    assert_eq!(v3i("sum3"), Vector3i::new(2, 3, 4), "Vector3i + Vector3i");
    assert_eq!(v3i("abs3"), Vector3i::new(2, 3, 4), "Vector3i.abs");

    match script_var(&ml, world, "len2") {
        Variant::Float(l) => assert!(
            (l - 5.0).abs() < 1e-6,
            "Vector2i(3,4).length() must be exactly 5, got {l}",
        ),
        other => panic!("len2 should be a Float, got {other:?}"),
    }
}

/// A method name that genuinely does not exist must still report
/// `UndefinedFunction` — the fallback table must not swallow typos into a
/// silently wrong value.
#[test]
fn an_unknown_vector_method_is_still_an_error() {
    let mut interp = gdscript_interop::interpreter::Interpreter::new();
    let class_def = interp
        .run_class("extends Node\nfunc check():\n    return Vector2(1, 2).no_such_method()\n")
        .expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    let err = interp
        .call_instance_method(&mut inst, "check", &[])
        .expect_err("an unknown vector method must not succeed");
    let text = format!("{err:?}");
    assert!(
        text.contains("no_such_method"),
        "the error must name the missing method, got: {text}",
    );
}
