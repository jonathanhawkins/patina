//! pat-s3rrv.98 — the whole `Shape2D` family is constructible from GDScript.
//!
//! All eight were already in ClassDB and already representable by
//! `gdphysics2d::shape::Shape2D`, and the editor's own resource picker offers
//! all eight — but only `CircleShape2D` and `RectangleShape2D` had a `.new()`,
//! so `var s = CapsuleShape2D.new()` failed on a class every other layer knew.
//!
//! ⚠️ `.new()` needs BOTH halves: a name in the interpreter's construction gate
//! and an arm in `physics_query_static_call`. A name without an arm returns
//! nothing; an arm without a name is never reached.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A small 10x10 body at the origin, plus a scripted probe.
///
/// ⚠️ The target is deliberately SMALL. The capsule test below distinguishes a
/// capsule from a circle by vertical reach, which a tall wall would hide by
/// overlapping both.
fn small_target_scene(script: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let target = tree
        .add_child(main, Node::new("Target", "StaticBody2D"))
        .unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(Vector2::new(10.0, 10.0)));
    tree.add_child(target, shape).unwrap();

    let probe = tree.add_child(main, Node::new("Probe", "Node2D")).unwrap();
    tree.attach_script(
        probe,
        Box::new(GDScriptNodeInstance::from_source(script, probe).unwrap()),
    );

    // Without this the bodies are never registered and every query answers
    // empty — which reads exactly like "the dispatch does not work".
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, probe)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

/// **The acceptance.** All six previously-unconstructible shapes now build.
#[test]
fn shape2d_family_constructible_from_gdscript() {
    let script = "\
extends Node2D
var ran = false
var made = 0
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    if CapsuleShape2D.new() != null:
        self.made = self.made + 1
    if SegmentShape2D.new() != null:
        self.made = self.made + 1
    if ConvexPolygonShape2D.new() != null:
        self.made = self.made + 1
    if ConcavePolygonShape2D.new() != null:
        self.made = self.made + 1
    if WorldBoundaryShape2D.new() != null:
        self.made = self.made + 1
    if SeparationRayShape2D.new() != null:
        self.made = self.made + 1
";
    let (mut ml, probe) = small_target_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run, or `made` below is just its initialiser"
    );
    assert_eq!(
        script_var(&ml, probe, "made"),
        Variant::Int(6),
        "all six shapes must construct; a lower count means a class is listed \
         in the interpreter gate without an arm in physics_query_static_call, \
         or the reverse"
    );
}

/// 🔑 Setting `radius` on a capsule must leave it a CAPSULE.
///
/// ⚠️ The `radius` setter used to rebuild the shape as a `Circle`
/// unconditionally — harmless while circles were the only radius-bearing shape,
/// and silently destructive the moment capsules became constructible. The two
/// are distinguished here by VERTICAL REACH, which is the only thing that
/// differs: a capsule of radius 20 and total height 60 reaches 30 above and
/// below its centre, where a circle of radius 20 reaches only 20.
///
/// Placed at y = -32, the capsule's lower cap reaches y = -2 and overlaps the
/// 10x10 target spanning [-5, 5]; a circle would stop at y = -12 and miss. So
/// the hit count is 1 for a capsule and 0 for a silently-converted circle.
#[test]
fn setting_radius_on_a_capsule_does_not_turn_it_into_a_circle() {
    let script = "\
extends Node2D
var ran = false
var hits = -1
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var c = CapsuleShape2D.new()
    c.radius = 20.0
    c.height = 60.0
    var q = PhysicsShapeQueryParameters2D.new()
    q.shape = c
    q.transform = Transform2D(0.0, Vector2(0, -32))
    self.hits = get_world_2d().direct_space_state.intersect_shape(q).size()
";
    let (mut ml, probe) = small_target_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run"
    );
    // Asserted before the count: -1 means the construct/assign/query chain
    // raised, and the count assertion below would be reading an initialiser.
    assert_ne!(
        script_var(&ml, probe, "hits"),
        Variant::Int(-1),
        "the CapsuleShape2D.new() -> radius -> height -> intersect_shape chain \
         must complete without raising"
    );
    assert_eq!(
        script_var(&ml, probe, "hits"),
        Variant::Int(1),
        "a capsule of radius 20 / height 60 at y = -32 reaches the target at \
         y = -5; 0 hits means `radius` rebuilt it as a circle, which stops \
         12px short"
    );
}

/// Extracts a number from a script var, so a failure prints the value rather
/// than a `Variant` debug blob.
fn fnum(ml: &MainLoop, node: NodeId, name: &str) -> f64 {
    match script_var(ml, node, name) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("`{name}` is not a number: {other:?}"),
    }
}

fn run(script: &str) -> (MainLoop, NodeId) {
    let (mut ml, probe) = small_target_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run, or every assertion below reads an initialiser"
    );
    (ml, probe)
}

/// 🔑 A shape was **write-only** from script until now: the setters landed, but
/// every read answered `Nil`.
///
/// ⚠️ `size` is the arm that catches a plausible getter: Godot's
/// `RectangleShape2D.size` is the FULL size while the solver stores half
/// extents, so a getter that returns the stored field reads back HALF of every
/// assignment. `Vector2(20, 10)` in and `Vector2(10, 5)` out is a silent
/// halving that only a round-trip notices — reading the field alone looks
/// perfectly correct.
#[test]
fn shape_properties_round_trip_through_script() {
    let script = "\
extends Node2D
var ran = false
var circle_r = -1.0
var rect_size = Vector2(-1, -1)
var seg_a = Vector2(-1, -1)
var seg_b = Vector2(-1, -1)
var wb_dist = -1.0
var wb_normal = Vector2(-1, -1)
var ray_len = -1.0
var poly_points = -1
var concave_segments = -1
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var c = CircleShape2D.new()
    c.radius = 7.0
    self.circle_r = c.radius
    var r = RectangleShape2D.new()
    r.size = Vector2(20, 10)
    self.rect_size = r.size
    var s = SegmentShape2D.new()
    s.a = Vector2(1, 2)
    s.b = Vector2(3, 4)
    self.seg_a = s.a
    self.seg_b = s.b
    var w = WorldBoundaryShape2D.new()
    w.normal = Vector2(2, 0)
    w.distance = 12.0
    self.wb_normal = w.normal
    self.wb_dist = w.distance
    var sr = SeparationRayShape2D.new()
    sr.length = 33.0
    self.ray_len = sr.length
    var cx = ConvexPolygonShape2D.new()
    cx.points = PackedVector2Array([Vector2(0, 0), Vector2(4, 0), Vector2(0, 4)])
    self.poly_points = cx.points.size()
    var cc = ConcavePolygonShape2D.new()
    cc.segments = PackedVector2Array([Vector2(0, 0), Vector2(5, 5)])
    self.concave_segments = cc.segments.size()
";
    let (ml, probe) = run(script);

    assert_eq!(fnum(&ml, probe, "circle_r"), 7.0, "CircleShape2D.radius");
    // 🔑 The halving trap: full size in, full size out.
    assert_eq!(
        script_var(&ml, probe, "rect_size"),
        Variant::Vector2(Vector2::new(20.0, 10.0)),
        "RectangleShape2D.size is the FULL size both ways; (10, 5) here means \
         the getter returned the stored half-extents"
    );
    assert_eq!(
        script_var(&ml, probe, "seg_a"),
        Variant::Vector2(Vector2::new(1.0, 2.0))
    );
    assert_eq!(
        script_var(&ml, probe, "seg_b"),
        Variant::Vector2(Vector2::new(3.0, 4.0)),
        "`a` and `b` share one setter arm; reading `b` back as (1, 2) means it \
         wrote whichever end the arm was entered with"
    );
    // ⚠️ **Divergence from Godot, pinned deliberately.** Godot stores this
    // normal exactly as written — `set_normal` only rejects zero, with the
    // comment "Can be non-unit but prevent zero" — whereas Patina's
    // `Shape2D::world_boundary` normalizes on the way in, so `(2, 0)` comes
    // back as `(1, 0)`.
    //
    // The fixture is non-unit on purpose: a unit normal is the coincidence
    // point where normalizing and not normalizing agree, so it would pass
    // whichever behaviour is in force and pin nothing. `(2, 0)` also differs
    // from the `(0, -1)` default, so this still proves the setter ran.
    //
    // Not "fixed" here: the collision solver re-normalizes at use
    // (`collision.rs`), so nothing is currently wrong, and the stored
    // representation is shared with the scene loader — moving it belongs with
    // the solver, not with a constructibility change.
    assert_eq!(
        script_var(&ml, probe, "wb_normal"),
        Variant::Vector2(Vector2::new(1.0, 0.0)),
        "Patina normalizes the boundary normal on write; (2, 0) must read back          as the unit (1, 0), not as (2, 0)"
    );
    assert_eq!(fnum(&ml, probe, "wb_dist"), 12.0);
    assert_eq!(fnum(&ml, probe, "ray_len"), 33.0);
    assert_eq!(
        script_var(&ml, probe, "poly_points"),
        Variant::Int(3),
        "ConvexPolygonShape2D.points must read back the three vertices written"
    );
    assert_eq!(
        script_var(&ml, probe, "concave_segments"),
        Variant::Int(2),
        "ConcavePolygonShape2D.segments is a flat endpoint list, so two points \
         is one segment"
    );
}

/// 🔑 Godot repairs the capsule invariant `height >= 2 * radius`, and the two
/// setters repair it in **opposite directions**.
///
/// ⚠️ `set_radius` GROWS the height; `set_height` SHRINKS the radius
/// (`capsule_shape_2d.cpp`). Clamping both the same way passes whichever half
/// the test writes first and disagrees with Godot on the other — so both
/// directions are exercised here.
#[test]
fn capsule_repairs_the_height_radius_invariant_like_godot() {
    let script = "\
extends Node2D
var ran = false
var grew_r = -1.0
var grew_h = -1.0
var shrunk_r = -1.0
var shrunk_h = -1.0
var untouched_h = -1.0
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var a = CapsuleShape2D.new()
    a.radius = 40.0
    self.grew_r = a.radius
    self.grew_h = a.height
    var b = CapsuleShape2D.new()
    b.height = 10.0
    self.shrunk_r = b.radius
    self.shrunk_h = b.height
    var c = CapsuleShape2D.new()
    c.radius = 12.0
    self.untouched_h = c.height
";
    let (ml, probe) = run(script);

    // Defaults are radius 10 / height 30, so a radius of 40 violates the
    // invariant and Godot grows the height to 80 rather than refusing.
    assert_eq!(fnum(&ml, probe, "grew_r"), 40.0, "the radius written wins");
    assert_eq!(
        fnum(&ml, probe, "grew_h"),
        80.0,
        "setting radius past height/2 must GROW height to 2*radius; 30.0 here \
         means the invariant is unenforced and the capsule is degenerate"
    );

    // The opposite repair: a height of 10 cannot hold the default radius 10.
    assert_eq!(
        fnum(&ml, probe, "shrunk_h"),
        10.0,
        "the height written wins"
    );
    assert_eq!(
        fnum(&ml, probe, "shrunk_r"),
        5.0,
        "setting height below 2*radius must SHRINK radius to height/2; 10.0 \
         here means the clamp was copied from the radius setter and pushes the \
         wrong field"
    );

    // ⚠️ Positive control: the repair must fire ONLY when the invariant is
    // actually violated. Radius 12 still fits inside height 30, so a height
    // that moved to 24 would mean the setter clamps unconditionally — which
    // both assertions above would happily pass.
    assert_eq!(
        fnum(&ml, probe, "untouched_h"),
        30.0,
        "radius 12 fits within height 30, so height must not move"
    );
}

/// `mid_height` is the straight middle section, excluding both caps — a
/// derived wrapper over `height`, which is why the docs give it no default.
#[test]
fn capsule_mid_height_wraps_height() {
    let script = "\
extends Node2D
var ran = false
var default_mid = -1.0
var set_mid_h = -1.0
var set_mid_back = -1.0
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var a = CapsuleShape2D.new()
    self.default_mid = a.mid_height
    var b = CapsuleShape2D.new()
    b.mid_height = 40.0
    self.set_mid_h = b.height
    self.set_mid_back = b.mid_height
";
    let (ml, probe) = run(script);

    // Defaults radius 10 / height 30 leave 30 - 2*10 = 10 of straight section.
    assert_eq!(
        fnum(&ml, probe, "default_mid"),
        10.0,
        "mid_height = height - 2*radius; 30.0 here means it aliased `height`"
    );
    assert_eq!(
        fnum(&ml, probe, "set_mid_h"),
        60.0,
        "writing mid_height 40 must set height to 40 + 2*radius"
    );
    assert_eq!(
        fnum(&ml, probe, "set_mid_back"),
        40.0,
        "and read back as 40"
    );
}

/// 🔑 The read gap's real cost was not the read — it was that `+=` needs one.
///
/// ⚠️ A compound assignment desugars to a read, a combine, and a write. While
/// the read answered `Nil` the interpreter's "is this a property?" gate said
/// no, so `c.radius += 5` died mid-frame exactly the way an unset
/// `CharacterBody2D.velocity` used to — the most ordinary line a script can
/// write against a shape.
#[test]
fn compound_assignment_on_a_shape_property_works() {
    let script = "\
extends Node2D
var ran = false
var r = -1.0
var survived = false
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var c = CircleShape2D.new()
    c.radius = 10.0
    c.radius += 5.0
    self.r = c.radius
    self.survived = true
";
    let (ml, probe) = run(script);

    assert_eq!(
        script_var(&ml, probe, "survived"),
        Variant::Bool(true),
        "the frame must reach the end; a false here means `+=` raised"
    );
    assert_eq!(
        fnum(&ml, probe, "r"),
        15.0,
        "`c.radius += 5` must read 10, add, and store 15"
    );
}
