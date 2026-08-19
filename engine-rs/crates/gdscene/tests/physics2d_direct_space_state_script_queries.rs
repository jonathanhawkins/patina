//! pat-s3rrv.26 — `get_world_2d().direct_space_state` queries run from GDScript.
//!
//! Godot's space-query surface is a chain of *objects*:
//!
//! ```gdscript
//! var space = get_world_2d().direct_space_state
//! var hit = space.intersect_ray(PhysicsRayQueryParameters2D.create(from, to))
//! ```
//!
//! The registration half of this bead put all eight query classes into ClassDB,
//! which makes a query **describable**. It could not make one **runnable**:
//! `get_world_2d`, `direct_space_state` and `intersect_ray` had 0 dispatch hits
//! between `interpreter.rs` and `gdscene/src/scripting.rs`, while
//! `PhysicsWorld2D::intersect_ray` had been correct and unit-tested the whole
//! time. Every layer existed and none of them met.
//!
//! ⚠️ Driven through `MainLoop` and a script — never by calling
//! `world.intersect_ray` directly. That function was green in isolation
//! throughout the entire period the feature was unreachable, so testing it
//! proves nothing about the chain.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A wall at x=200 and a scripted probe at the origin.
///
/// Collision geometry comes from a child `CollisionShape2D` (`size` = rect),
/// which is what the physics server's `find_collision_shape` reads.
fn wall_scene(script: &str) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(200.0, 0.0)));
    let wall_id = tree.add_child(main, wall).unwrap();
    let mut wall_shape = Node::new("CollisionShape", "CollisionShape2D");
    wall_shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 400.0)));
    tree.add_child(wall_id, wall_shape).unwrap();

    let probe_id = tree.add_child(main, Node::new("Probe", "Node2D")).unwrap();
    tree.attach_script(
        probe_id,
        Box::new(GDScriptNodeInstance::from_source(script, probe_id).unwrap()),
    );

    // ⚠️ Without this the bodies are never registered and every query answers
    // empty — which reads exactly like "the dispatch does not work".
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, probe_id, wall_id)
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

/// **The acceptance.** A ray fired from the script hits the wall and reports it.
#[test]
fn physics2d_direct_space_state_script_queries() {
    // `_physics_process` is where Godot documents space queries as valid, and
    // it is the phase in which the script's physics-server handle is installed.
    let script = "\
extends Node2D
var ran = false
var hit_something = false
var hit_x = -1.0
var collider_name = \"\"
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var space = get_world_2d().direct_space_state
    var params = PhysicsRayQueryParameters2D.create(Vector2(0, 0), Vector2(400, 0))
    var hit = space.intersect_ray(params)
    if hit:
        self.hit_something = true
        self.hit_x = hit[\"position\"].x
        self.collider_name = hit[\"collider\"].get_name()
";
    let (mut ml, probe, _wall) = wall_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run — a false here means the fixture never \
         stepped, not that the query failed"
    );
    assert_eq!(
        script_var(&ml, probe, "hit_something"),
        Variant::Bool(true),
        "the ray must hit the wall: get_world_2d().direct_space_state\
         .intersect_ray(...) has to resolve through the whole object chain"
    );

    // The wall spans x = 180..220, so a ray from the origin hits its near face.
    let hit_x = match script_var(&ml, probe, "hit_x") {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("hit_x should be numeric, got {other:?}"),
    };
    assert!(
        (150.0..=225.0).contains(&hit_x),
        "the hit position must land on the wall's near face (~180), got {hit_x}"
    );

    // ⚠️ The assertion that proves `collider` is the real NODE rather than a
    // raw body id — that is what a script actually uses
    // (`hit.collider.queue_free()`).
    //
    // Read via `get_name()`, not `.name`: Patina exposes no `name` *property*
    // (neither `Node::get_property` nor `SceneAccess::get_node_property` handles
    // it), so `.name` yields Nil regardless of whether the collider resolved —
    // which is exactly how this assertion first failed, pointing at the query
    // when the query was fine. Godot does support `.name`; that gap is real but
    // belongs to another bead.
    assert_eq!(
        script_var(&ml, probe, "collider_name"),
        Variant::String("Wall".to_string()),
        "`collider` must be the StaticBody2D node itself"
    );
}

/// Two walls on the +X axis — `NearWall` at x=200 and `FarWall` at x=400 —
/// plus a scripted probe at the origin.
///
/// ⚠️ `exclude` is only provable with **two** bodies in line. With a single
/// wall, "exclude worked" and "exclude was silently ignored" are
/// indistinguishable from a miss, so the test would pass while the filter did
/// nothing. Here the excluded ray must still hit *something*, just further away.
fn two_wall_scene(script: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    for (name, x) in [("NearWall", 200.0), ("FarWall", 400.0)] {
        let mut wall = Node::new(name, "StaticBody2D");
        wall.set_property("position", Variant::Vector2(Vector2::new(x, 0.0)));
        let wall_id = tree.add_child(main, wall).unwrap();
        let mut wall_shape = Node::new("CollisionShape", "CollisionShape2D");
        wall_shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 400.0)));
        tree.add_child(wall_id, wall_shape).unwrap();
    }

    let probe_id = tree.add_child(main, Node::new("Probe", "Node2D")).unwrap();
    tree.attach_script(
        probe_id,
        Box::new(GDScriptNodeInstance::from_source(script, probe_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, probe_id)
}

fn num(v: Variant) -> f64 {
    match v {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// `params.exclude = [node]` actually filters the query (pat-s3rrv.26).
///
/// Godot's `exclude` takes RIDs; Patina has no RID, so a script passes the node
/// objects themselves. Two rays are fired down the same axis in the same frame:
/// the unfiltered one must stop at `NearWall`, the filtered one must pass
/// through it and stop at `FarWall`.
#[test]
fn ray_exclude_skips_the_named_body() {
    let script = "\
extends Node2D
var ran = false
var plain_x = -1.0
var excluded_x = -1.0
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var space = get_world_2d().direct_space_state
    var p1 = PhysicsRayQueryParameters2D.create(Vector2(0, 0), Vector2(600, 0))
    var h1 = space.intersect_ray(p1)
    if h1:
        self.plain_x = h1[\"position\"].x
    var near = get_node(\"../NearWall\")
    var p2 = PhysicsRayQueryParameters2D.create(Vector2(0, 0), Vector2(600, 0))
    p2.exclude = [near]
    var h2 = space.intersect_ray(p2)
    if h2:
        self.excluded_x = h2[\"position\"].x
";
    let (mut ml, probe) = two_wall_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run"
    );

    // The control. NearWall spans x = 180..220, so an unfiltered ray from the
    // origin stops at its near face. If THIS fails the fixture is wrong and the
    // exclude assertion below would be meaningless.
    let plain = num(script_var(&ml, probe, "plain_x"));
    assert!(
        (150.0..=225.0).contains(&plain),
        "control: the unfiltered ray must stop at NearWall (~180), got {plain}"
    );

    // The acceptance. FarWall spans x = 380..420.
    let excluded = num(script_var(&ml, probe, "excluded_x"));
    assert!(
        (350.0..=425.0).contains(&excluded),
        "excluding NearWall must let the ray reach FarWall (~380), got {excluded} \
         — a value near {plain} means `exclude` was accepted and then dropped"
    );
    assert!(
        excluded > plain + 100.0,
        "the two rays must resolve to different walls: plain={plain}, excluded={excluded}"
    );
}

/// `intersect_shape` finds the bodies a shape overlaps (pat-s3rrv.26).
///
/// Godot's spelling, end to end:
/// ```gdscript
/// var s = CircleShape2D.new()
/// s.radius = 30.0
/// var q = PhysicsShapeQueryParameters2D.new()
/// q.shape = s
/// q.transform = Transform2D(0.0, Vector2(200, 0))
/// space.intersect_shape(q)
/// ```
///
/// ⚠️ The two walls are 200 apart and the probe circle has radius 30 placed on
/// `NearWall`, so it can only reach that one. A query that ignored the
/// transform, or that returned every body regardless of overlap, would report
/// 2 — so the count is what makes this test meaningful, not merely non-empty.
#[test]
fn intersect_shape_reports_the_overlapped_body_only() {
    let script = "\
extends Node2D
var ran = false
var hit_count = -1
var first_name = \"\"
var far_count = -1
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var space = get_world_2d().direct_space_state
    var s = CircleShape2D.new()
    s.radius = 30.0
    var q = PhysicsShapeQueryParameters2D.new()
    q.shape = s
    q.transform = Transform2D(0.0, Vector2(200, 0))
    var hits = space.intersect_shape(q)
    self.hit_count = hits.size()
    if hits.size() > 0:
        self.first_name = hits[0][\"collider\"].get_name()
    var q2 = PhysicsShapeQueryParameters2D.new()
    q2.shape = s
    q2.transform = Transform2D(0.0, Vector2(1000, 1000))
    self.far_count = space.intersect_shape(q2).size()
";
    let (mut ml, probe) = two_wall_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run"
    );

    // ⚠️ Asserted before the count: a -1 here means the whole
    // CircleShape2D.new() -> q.shape -> intersect_shape chain raised, and every
    // assertion below would then be reading an initialiser.
    let count = script_var(&ml, probe, "hit_count");
    assert_ne!(
        count,
        Variant::Int(-1),
        "the shape-query chain must not raise — -1 is the initialiser, meaning \
         CircleShape2D.new(), PhysicsShapeQueryParameters2D.new() or \
         intersect_shape() failed to resolve"
    );
    assert_eq!(
        count,
        Variant::Int(1),
        "a radius-30 circle on NearWall must overlap exactly that body — 2 would \
         mean the transform was ignored, 0 that the overlap never ran"
    );
    assert_eq!(
        script_var(&ml, probe, "first_name"),
        Variant::String("NearWall".to_string()),
        "`collider` must be the overlapped StaticBody2D node itself"
    );

    // The same shape somewhere empty overlaps nothing — proving the non-empty
    // result above came from the geometry rather than from always answering.
    assert_eq!(
        script_var(&ml, probe, "far_count"),
        Variant::Int(0),
        "a shape in empty space must overlap nothing"
    );
}

/// The remaining three `PhysicsDirectSpaceState2D` methods (pat-s3rrv.26):
/// `cast_motion`, `collide_shape` and `get_rest_info`.
///
/// Contracts from `doc/classes/PhysicsDirectSpaceState2D.xml`:
/// - `cast_motion` → `[safe, unsafe]` fractions, `[1, 1]` when the whole motion
///   is clear;
/// - `collide_shape` → contact points in PAIRS, so the array length is even and
///   twice the number of overlaps;
/// - `get_rest_info` → the nearest contact, empty Dictionary when nothing
///   overlaps.
#[test]
fn cast_motion_collide_shape_and_rest_info_answer_from_script() {
    let script = "\
extends Node2D
var ran = false
var clear_safe = -1.0
var blocked_safe = -1.0
var pair_count = -1
var rest_name = \"\"
var rest_empty = false
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var space = get_world_2d().direct_space_state
    var s = CircleShape2D.new()
    s.radius = 10.0
    var q = PhysicsShapeQueryParameters2D.new()
    q.shape = s
    q.transform = Transform2D(0.0, Vector2(0, -300))
    q.motion = Vector2(0, -50)
    self.clear_safe = space.cast_motion(q)[0]
    var q2 = PhysicsShapeQueryParameters2D.new()
    q2.shape = s
    q2.transform = Transform2D(0.0, Vector2(0, 0))
    q2.motion = Vector2(400, 0)
    self.blocked_safe = space.cast_motion(q2)[0]
    var q3 = PhysicsShapeQueryParameters2D.new()
    q3.shape = s
    q3.transform = Transform2D(0.0, Vector2(200, 0))
    self.pair_count = space.collide_shape(q3).size()
    var info = space.get_rest_info(q3)
    if info:
        self.rest_name = info[\"collider\"].get_name()
    var q4 = PhysicsShapeQueryParameters2D.new()
    q4.shape = s
    q4.transform = Transform2D(0.0, Vector2(0, -900))
    self.rest_empty = space.get_rest_info(q4).is_empty()
";
    let (mut ml, probe) = two_wall_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run"
    );

    // Motion through empty space is entirely safe — Godot's [1, 1].
    let clear = num(script_var(&ml, probe, "clear_safe"));
    assert!(
        (clear - 1.0).abs() < 1e-3,
        "an unobstructed motion must report safe = 1.0, got {clear} \
         (-1 means the cast_motion chain raised)"
    );

    // Moving +400 in X from the origin runs into NearWall, whose near face is
    // at x=180 and which the radius-10 circle meets around x=170 — so the safe
    // fraction must be a real value strictly inside (0, 1), not a clamp.
    let blocked = num(script_var(&ml, probe, "blocked_safe"));
    assert!(
        blocked > 0.2 && blocked < 0.6,
        "a motion into NearWall must stop partway (~0.42), got {blocked} — \
         1.0 would mean the sweep never detected the wall, 0.0 that it \
         reported an overlap at rest"
    );

    // Pairs: one overlapped body, two points.
    assert_eq!(
        script_var(&ml, probe, "pair_count"),
        Variant::Int(2),
        "collide_shape must return contact points in PAIRS — 1 would mean the \
         pair contract was dropped, 0 that the overlap never ran"
    );

    assert_eq!(
        script_var(&ml, probe, "rest_name"),
        Variant::String("NearWall".to_string()),
        "get_rest_info must name the nearest collider"
    );
    assert_eq!(
        script_var(&ml, probe, "rest_empty"),
        Variant::Bool(true),
        "get_rest_info must answer an EMPTY Dictionary when nothing overlaps, \
         never null — so `if info:` works without a guard"
    );
}

/// A ray that reaches nothing answers an EMPTY Dictionary, never null, so a
/// script can write `if hit:` without a null guard — Godot's contract.
#[test]
fn missing_ray_hit_is_an_empty_dictionary() {
    let script = "\
extends Node2D
var ran = false
var got_hit = true
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var space = get_world_2d().direct_space_state
    var params = PhysicsRayQueryParameters2D.create(Vector2(0, -300), Vector2(0, -280))
    var hit = space.intersect_ray(params)
    self.got_hit = false
    if hit:
        self.got_hit = true
";
    let (mut ml, probe, _wall) = wall_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&ml, probe, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run"
    );
    assert_eq!(
        script_var(&ml, probe, "got_hit"),
        Variant::Bool(false),
        "a ray into empty space must yield a falsy (empty) Dictionary"
    );
}
