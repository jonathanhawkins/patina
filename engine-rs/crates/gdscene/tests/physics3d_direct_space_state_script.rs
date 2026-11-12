//! pat-s3rrv.107: `get_world_3d().direct_space_state.intersect_ray(...)` from GDScript.
//!
//! The 3D query library was complete and unreachable: `gdphysics3d/src/query.rs`
//! has the whole surface, and `PhysicsRayQueryParameters3D` was registered and
//! constructible — but nothing connected a script to the 3D world. There was no
//! `get_world_3d`, no space-state handle, and no 3D twin of the script-facing
//! physics-server binding, so the canonical Godot idiom could not be written.
//!
//! ⚠️ The parameters arrive as a `Variant::Resource`, not a handle-registry
//! entry like the 2D path uses. `PhysicsRayQueryParameters3D` is a RefCounted
//! class that already constructs from script and carries its own property map,
//! so a second registry would be duplicate state that could disagree with the
//! object the script holds. Note `create()` is wired for 2D only, hence `.new()`
//! plus property assignment here.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

/// `Root -> {Floor (StaticBody3D box, top face at y = 1), Probe (script)}`.
fn ray_scene(script: &str) -> (MainLoop, NodeId) {
    // ⚠️ REQUIRED: `PhysicsRayQueryParameters3D.new()` resolves through ClassDB,
    // and neither SceneTree nor MainLoop populates it. Without this the
    // constructor answers Nil and the ray has no endpoints — a failure that
    // looks exactly like a broken query.
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    // Box spans y ∈ [-1, 1]; a ray from y=5 down to y=-5 hits its top at y = 1.
    let floor = tree
        .add_child(root, Node::new("Floor", "StaticBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, floor, Vector3::ZERO);
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    shape.set_property("size", Variant::Vector3(Vector3::new(4.0, 2.0, 4.0)));
    tree.add_child(floor, shape).unwrap();

    let probe = tree.add_child(root, Node::new("Probe", "Node3D")).unwrap();
    tree.attach_script(
        probe,
        Box::new(GDScriptNodeInstance::from_source(script, probe).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, probe)
}

/// **The acceptance.** A downward ray from a script finds the floor.
#[test]
fn physics3d_direct_space_state_script() {
    let (mut ml, probe) = ray_scene(
        "\
extends Node3D
var ran = false
var hit_something = false
var hit_y = -999.0

func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var q = PhysicsRayQueryParameters3D.new()
    q.from = Vector3(0, 5, 0)
    q.to = Vector3(0, -5, 0)
    var space = get_world_3d().direct_space_state
    var hit = space.intersect_ray(q)
    if hit:
        self.hit_something = true
        self.hit_y = hit[\"position\"].y
",
    );

    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    // CONTROL FIRST: the callback ran at all. A false here means the fixture
    // never stepped, which would make every assertion below vacuous rather
    // than informative.
    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run — otherwise nothing below is about the query"
    );

    assert_eq!(
        script_var(&ml, probe, "hit_something"),
        Variant::Bool(true),
        "the ray must find the StaticBody3D. An empty Dictionary here means the \
         chain `get_world_3d() -> direct_space_state -> intersect_ray` did not \
         reach the 3D world — the gap this bead exists to close"
    );

    // 🔑 THE VALUE, not just the hit. An implementation that returned a
    // non-empty Dictionary with junk coordinates would satisfy the assertion
    // above; the box's top face is the only y that proves a real intersection.
    match script_var(&ml, probe, "hit_y") {
        Variant::Float(y) => assert!(
            (y - 1.0).abs() < 1e-3,
            "the hit must land on the box's TOP FACE (y = 1); got {y}"
        ),
        other => panic!("hit_y must be a float, got {other:?}"),
    }
}

/// CONTROL: a `collision_mask` the floor is not on returns an empty Dictionary.
///
/// Without this, an implementation that ignored the mask entirely would pass the
/// acceptance above — the mask is only observable when it EXCLUDES something.
#[test]
fn a_collision_mask_excluding_the_body_finds_nothing() {
    let (mut ml, probe) = ray_scene(
        "\
extends Node3D
var ran = false
var hit_something = false

func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var q = PhysicsRayQueryParameters3D.new()
    q.from = Vector3(0, 5, 0)
    q.to = Vector3(0, -5, 0)
    q.collision_mask = 2
    var space = get_world_3d().direct_space_state
    var hit = space.intersect_ray(q)
    if hit:
        self.hit_something = true
",
    );

    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "fixture: the callback must run"
    );
    assert_eq!(
        script_var(&ml, probe, "hit_something"),
        Variant::Bool(false),
        "the floor is on layer 1, so a mask of 2 must see nothing — a hit here \
         means `collision_mask` is being ignored"
    );
}

/// **Acceptance clause (c).** A sphere overlapping the box returns one result.
#[test]
fn intersect_shape_finds_the_overlapping_body() {
    let (mut ml, probe) = ray_scene(
        "\
extends Node3D
var ran = false
var count = -1

func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var sph = SphereShape3D.new()
    sph.radius = 1.0
    var q = PhysicsShapeQueryParameters3D.new()
    q.shape = sph
    var space = get_world_3d().direct_space_state
    var hits = space.intersect_shape(q)
    self.count = hits.size()
",
    );

    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "fixture: the callback must run"
    );
    // ⚠️ -1, not 0, is the sentinel: a query that raised and a query that found
    // nothing are different outcomes, and 0 would conflate them with the very
    // result the control below expects.
    assert_eq!(
        script_var(&ml, probe, "count"),
        Variant::Int(1),
        "a unit sphere at the origin overlaps the box spanning y in [-1, 1], so \
         intersect_shape must report exactly the one body. -1 means the query \
         raised or never returned an Array"
    );
}

/// CONTROL: the same sphere moved clear of the box finds nothing.
///
/// Without this, an implementation that ignored the query POSITION entirely —
/// reporting every body in the world — would pass the clause above.
#[test]
fn intersect_shape_respects_the_query_position() {
    let (mut ml, probe) = ray_scene(
        "\
extends Node3D
var ran = false
var count = -1

func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var sph = SphereShape3D.new()
    sph.radius = 1.0
    var q = PhysicsShapeQueryParameters3D.new()
    q.shape = sph
    q.transform = Vector3(0, 50, 0)
    var space = get_world_3d().direct_space_state
    var hits = space.intersect_shape(q)
    self.count = hits.size()
",
    );

    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "count"),
        Variant::Int(0),
        "a sphere 50 units above the box overlaps nothing — a non-zero count \
         means the query position is being ignored"
    );
}

/// `intersect_point` reports the bodies containing a point.
#[test]
fn intersect_point_finds_the_body_containing_it() {
    let (mut ml, probe) = ray_scene(
        "\
extends Node3D
var ran = false
var inside = -1
var outside = -1

func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var space = get_world_3d().direct_space_state
    var q = PhysicsPointQueryParameters3D.new()
    q.position = Vector3(0, 0, 0)
    self.inside = space.intersect_point(q).size()
    var q2 = PhysicsPointQueryParameters3D.new()
    q2.position = Vector3(0, 50, 0)
    self.outside = space.intersect_point(q2).size()
",
    );

    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "fixture: the callback must run"
    );
    assert_eq!(
        script_var(&ml, probe, "inside"),
        Variant::Int(1),
        "the origin is inside the box spanning y in [-1, 1], so exactly one body \
         must be reported; -1 means the query raised"
    );
    // 🔑 The same query 50 units up must find NOTHING. Without this an
    // implementation that returned every body regardless of the point would
    // satisfy the assertion above.
    assert_eq!(
        script_var(&ml, probe, "outside"),
        Variant::Int(0),
        "a point clear of the box must report no bodies — a non-zero count means \
         the point is being ignored"
    );
}

/// `cast_motion` reports how far the shape can travel before it hits.
#[test]
fn cast_motion_stops_short_of_the_box() {
    let (mut ml, probe) = ray_scene(
        "\
extends Node3D
var ran = false
var safe = -1.0
var clear_safe = -1.0

func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var space = get_world_3d().direct_space_state
    var sph = SphereShape3D.new()
    sph.radius = 0.5
    var q = PhysicsShapeQueryParameters3D.new()
    q.shape = sph
    q.transform = Vector3(0, 5, 0)
    q.motion = Vector3(0, -10, 0)
    self.safe = space.cast_motion(q)[0]
    var q2 = PhysicsShapeQueryParameters3D.new()
    q2.shape = sph
    q2.transform = Vector3(50, 5, 0)
    q2.motion = Vector3(0, -10, 0)
    self.clear_safe = space.cast_motion(q2)[0]
",
    );

    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "fixture: the callback must run"
    );

    // A sphere dropped onto the box must stop PART WAY: strictly between 0
    // (blocked immediately, which would mean it started inside) and 1
    // (unobstructed, which would mean the box was missed entirely).
    match script_var(&ml, probe, "safe") {
        Variant::Float(f) => assert!(
            f > 0.0 && f < 1.0,
            "cast_motion must stop short of the box: safe fraction {f} should be \
             strictly between 0 and 1"
        ),
        other => panic!("safe must be a float, got {other:?}"),
    }

    // 🔑 CONTROL: the same cast 50 units to the side hits nothing and must
    // report a FULL 1.0. Without it, an implementation that always returned a
    // small fraction would satisfy the assertion above.
    match script_var(&ml, probe, "clear_safe") {
        Variant::Float(f) => assert!(
            (f - 1.0).abs() < 1e-6,
            "an unobstructed cast must report the full motion (1.0), got {f}"
        ),
        other => panic!("clear_safe must be a float, got {other:?}"),
    }
}

/// `get_rest_info` answers the nearest contact, and an EMPTY Dictionary on a miss.
#[test]
fn get_rest_info_reports_a_contact_and_is_empty_on_a_miss() {
    let (mut ml, probe) = ray_scene(
        "\
extends Node3D
var ran = false
var touching = false
var missed = true

func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var space = get_world_3d().direct_space_state
    var sph = SphereShape3D.new()
    sph.radius = 1.0
    var q = PhysicsShapeQueryParameters3D.new()
    q.shape = sph
    var info = space.get_rest_info(q)
    if info:
        self.touching = true
    var q2 = PhysicsShapeQueryParameters3D.new()
    q2.shape = sph
    q2.transform = Vector3(0, 50, 0)
    var info2 = space.get_rest_info(q2)
    if info2:
        self.missed = false
",
    );

    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "touching"),
        Variant::Bool(true),
        "a sphere overlapping the box must produce rest info"
    );
    // The empty-on-miss contract is what lets a script write `if info:` without
    // a guard — returning a populated Dictionary with junk would break that.
    assert_eq!(
        script_var(&ml, probe, "missed"),
        Variant::Bool(true),
        "a sphere clear of the box must produce an EMPTY Dictionary, so \
         `if info:` is false"
    );
}
