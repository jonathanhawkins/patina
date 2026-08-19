//! pat-s3rrv.97 acceptance: CharacterBody2D's slide-collision and state surface
//! is correct and reachable from GDScript.
//!
//! Godot ground truth: `upstream/godot/scene/2d/physics/character_body_2d.cpp:456-466`
//! for the `_only` predicates, and `get_real_velocity()` for the motion actually
//! achieved by the last move.
//!
//! ⚠️ Two of these were WRONG rather than missing, which is why the fixture
//! wedges the body into a CORNER. Both bugs are invisible to a body touching
//! only one surface:
//!
//!   * `is_on_floor_only()` returned plain `is_on_floor()`, so it answered
//!     `true` for a body that was also against a wall — exactly the case it
//!     exists to rule out.
//!   * `get_real_velocity()` returned the node's `velocity`, which still
//!     carries the slide velocity for the next tick while the body went nowhere
//!     along x. It reported "still moving" for the one case worth asking about.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Drives into the corner, then records the whole surface each tick.
///
/// ⚠️ Every result starts at a value the engine cannot produce (`-1` / `-999`),
/// never `0` or `false`. A handler that raises leaves the initialiser behind,
/// and `false` is also a legitimate answer here — so a falsy initialiser would
/// make "never ran" and "ran and said no" the same observation.
const SCRIPT: &str = "
extends CharacterBody2D
var floor_only = -1
var on_wall = -1
var slides = -1
var nx0 = -999.0
var nx1 = -999.0
var realx = -999.0
var velx = -999.0
var px = -999.0
var py = -999.0

func _physics_process(delta):
    velocity = Vector2(200, 200)
    move_and_slide()
    floor_only = is_on_floor_only()
    on_wall = is_on_wall()
    slides = get_slide_collision_count()
    velx = velocity.x
    var rv = get_real_velocity()
    realx = rv.x
    px = position.x
    py = position.y
    if slides > 0:
        var c0 = get_slide_collision(0)
        var n0 = c0.get_normal()
        nx0 = n0.x
    if slides > 1:
        var c1 = get_slide_collision(1)
        var n1 = c1.get_normal()
        nx1 = n1.x
";

/// Sweeps into the wall with `move_and_collide` and records whether it returned
/// an object. Godot returns null on a clean move, so `hit` distinguishes the two.
const COLLIDE_SCRIPT: &str = "
extends CharacterBody2D
var hit = -1
var ran = 0

func _physics_process(delta):
    ran = ran + 1
    if ran == 1:
        var c = move_and_collide(Vector2(50, 0))
        if c == null:
            hit = 0
        else:
            hit = 1
";

/// A floor at y = 100 (top surface y = 90) and a wall at x = 60 (near face
/// x = 50), so a radius-10 body driven down-and-right wedges into the corner
/// touching BOTH.
fn corner_scene(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
    let floor_id = tree.add_child(root, floor).expect("floor attaches");
    let mut floor_shape = Node::new("FloorShape", "CollisionShape2D");
    floor_shape.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(floor_id, floor_shape).expect("floor shape");

    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(60.0, 0.0)));
    let wall_id = tree.add_child(root, wall).expect("wall attaches");
    let mut wall_shape = Node::new("WallShape", "CollisionShape2D");
    wall_shape.set_property("size", Variant::Vector2(Vector2::new(20.0, 400.0)));
    tree.add_child(wall_id, wall_shape).expect("wall shape");

    let mut body = Node::new("Player", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    let id = tree.add_child(root, body).expect("body attaches");
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(id, shape).expect("body shape");
    let script = GDScriptNodeInstance::from_source(src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, id)
}

fn var(ml: &MainLoop, id: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(id)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

fn num(ml: &MainLoop, id: NodeId, name: &str) -> f64 {
    match var(ml, id, name) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{name} should be a number, got {other:?}"),
    }
}

fn run(src: &str, ticks: usize) -> (MainLoop, NodeId) {
    let (mut ml, id) = corner_scene(src);
    for _ in 0..ticks {
        ml.step(1.0 / 60.0);
    }
    (ml, id)
}

/// **The acceptance test.**
#[test]
fn characterbody2d_slide_collision_script_api() {
    let (ml, id) = run(SCRIPT, 60);

    // The fixture only means anything if the body really is in the corner.
    // ⚠️ Reported WITH the body's final position. "on_wall was false" alone
    // cannot distinguish a wall-detection bug from a body that never travelled
    // far enough to touch the wall, and those live in different subsystems.
    assert_eq!(
        var(&ml, id, "on_wall"),
        Variant::Bool(true),
        "the body must reach the wall; it ended at ({}, {}) and the wall's near \
         face is x = 50 with a radius-10 body, so a body that arrived should sit \
         near x = 40. Without contact every assertion below is about a body \
         touching only the floor, where the bugs are invisible",
        num(&ml, id, "px"),
        num(&ml, id, "py")
    );

    // ⚠️ THE BUG. `is_on_floor_only()` used to return `is_on_floor()`, so this
    // was `true` for a body wedged against a wall.
    assert_eq!(
        var(&ml, id, "floor_only"),
        Variant::Bool(false),
        "on the floor AND against a wall is not `floor only` — Godot defines it \
         as on_floor && !on_wall && !on_ceiling"
    );

    // Slide collisions reach the script (delivered under pat-s3rrv.95).
    assert!(
        num(&ml, id, "slides") >= 1.0,
        "a body pressed into a corner must report slide collisions"
    );
    // ⚠️ Checked across the contacts rather than at index 0. A body wedged in
    // a corner touches TWO surfaces, and nothing promises which one the solver
    // recorded first — it recorded the FLOOR first here, whose normal is
    // (0, -1), so an index-0 assertion read x = 0 and looked like the wall
    // normal was wrong when the wall contact was simply the second entry.
    // Ordering is not part of Godot's contract, so the test must not depend on
    // it.
    let (nx0, nx1) = (num(&ml, id, "nx0"), num(&ml, id, "nx1"));
    assert!(
        nx0 < -0.5 || nx1 < -0.5,
        "one of the contacts must be the WALL, whose normal points back along \
         -X; got x = {nx0} and {nx1}. Two contacts near zero would mean both \
         were floor-like and the wall never registered as a slide collision"
    );

    // ⚠️ THE OTHER BUG, and the pair that proves it: the REQUESTED velocity is
    // still non-zero along x while the ACHIEVED motion is zero, because the
    // wall cancelled it. Returning `velocity` here answered "still moving".
    assert!(
        num(&ml, id, "velx").abs() > 1.0,
        "the script keeps asking to move right, so `velocity.x` stays non-zero"
    );
    assert!(
        num(&ml, id, "realx").abs() < 1.0,
        "the wall cancelled the horizontal motion, so get_real_velocity().x is \
         about 0; got {} (equal to velocity.x means it is still reporting the \
         requested value)",
        num(&ml, id, "realx")
    );
}

/// `move_and_collide` sweeps into the wall and returns a collision object.
#[test]
fn move_and_collide_reports_the_wall() {
    let (ml, id) = run(COLLIDE_SCRIPT, 30);
    assert!(
        num(&ml, id, "ran") >= 1.0,
        "the callback must have run at all"
    );
    assert_eq!(
        num(&ml, id, "hit"),
        1.0,
        "sweeping 50px right from the origin reaches the wall at x=50, so \
         move_and_collide must return a collision rather than null"
    );
}
