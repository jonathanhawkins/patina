//! pat-s3rrv.95 acceptance: `get_slide_collision_count()` / `get_slide_collision(i)`
//! are callable from GDScript, and the object they return names the collider.
//!
//! Godot ground truth: `doc/classes/KinematicCollision2D.xml`,
//! `CharacterBody2D.get_slide_collision`.
//!
//! This is the standard CharacterBody idiom for wall/enemy/damage detection:
//!
//! ```gdscript
//! for i in get_slide_collision_count():
//!     var c = get_slide_collision(i)
//!     if c.get_collider().is_in_group("hazard"): die()
//! ```
//!
//! ⚠️ The contacts were ALREADY being computed every physics tick and thrown
//! away — `CharacterBody2D` fills a `Vec<KinematicCollision2D>` during its slide
//! loop and nothing ever published it. So the failure this guards against is not
//! "the physics is wrong", it is "the physics is invisible to script".
//!
//! ⚠️ The script calls the BARE form (`get_slide_collision_count()`, not
//! `$Player.get_slide_collision_count()`) on purpose. A bare self-call resolves
//! through `SceneAccess::call_node_method`, a different table from the
//! interpreter's `call_method_on`; wiring only the latter ships a feature that
//! works in the spelling nobody uses and fails in the one Godot's own docs show.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// Walks right into the wall, then reads the contact back through the script
/// surface. Every step is stored so a failure names the link that broke.
///
/// ⚠️ `grouped` starts as an INT, not `false`. `false` is also what
/// `is_in_group` returns when the answer is genuinely no, so a `false`
/// initialiser cannot tell that case apart from the statement never running at
/// all — and an aborted handler leaves exactly the initialiser behind. Starting
/// at `0` makes the three outcomes distinct: `Int(0)` never assigned,
/// `Bool(false)` ran and said no, `Bool(true)` ran and said yes.
const READER: &str = "
extends CharacterBody2D
var hits = 0
var nx = 0.0
var reached = 0
var who = 0
var grouped = 0
func _physics_process(delta):
    velocity = Vector2(200, 0)
    move_and_slide()
    hits = get_slide_collision_count()
    if hits > 0:
        var c = get_slide_collision(0)
        var n = c.get_normal()
        nx = n.x
        reached = reached + 1
        var other = c.get_collider()
        who = other
        grouped = other.is_in_group(\"wall\")
";

/// The control: identical script, no wall to hit.
const LONELY: &str = "
extends CharacterBody2D
var hits = 0
func _physics_process(delta):
    velocity = Vector2(200, 0)
    move_and_slide()
    hits = get_slide_collision_count()
";

fn scene(src: &str, with_wall: bool) -> (MainLoop, NodeId, Option<NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut wall_node = None;
    if with_wall {
        let mut wall = Node::new("Wall", "StaticBody2D");
        wall.set_property("position", Variant::Vector2(Vector2::new(60.0, 0.0)));
        let wall_id = tree.add_child(main, wall).unwrap();
        let mut shape = Node::new("CollisionShape", "CollisionShape2D");
        shape.set_property("size", Variant::Vector2(Vector2::new(20.0, 400.0)));
        tree.add_child(wall_id, shape).unwrap();
        // `get_collider().is_in_group("wall")` is the assertion this exists for.
        tree.add_to_group(wall_id, "wall").unwrap();
        wall_node = Some(wall_id);
    }

    let mut body = Node::new("Player", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    let body_id = tree.add_child(main, body).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(body_id, shape).unwrap();
    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(src, body_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, body_id, wall_node)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn int_var(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match script_var(ml, node, name) {
        Some(Variant::Int(i)) => i,
        other => panic!("{name} should be an Int, got {other:?}"),
    }
}

/// Player starts at x=0 with radius 10; the wall's near face is x=50. At
/// 200px/s that is ~12 ticks, so 40 gives the contact room to happen and
/// persist without depending on the exact tick it lands on.
fn run(src: &str, with_wall: bool) -> (MainLoop, NodeId, Option<NodeId>) {
    let (mut ml, body, wall) = scene(src, with_wall);
    for _ in 0..40 {
        ml.step(1.0 / 60.0);
    }
    (ml, body, wall)
}

/// The collider on the first published contact, read straight off the node.
///
/// ⚠️ Deliberately bypasses the script layer. `_slide_collisions` is the
/// contract between the physics phase and the script phase, so checking it
/// separately makes a failure name WHICH half broke — a publish that never
/// resolved the body to a node, or a script surface that did not hand the node
/// back. Asserting only through the script conflates the two.
fn published_collider(ml: &MainLoop, body: NodeId) -> Variant {
    match ml
        .tree()
        .get_node(body)
        .map(|n| n.get_property("_slide_collisions"))
    {
        Some(Variant::Array(items)) => match items.to_vec().first() {
            Some(Variant::Dictionary(d)) => d.get("collider").unwrap_or(Variant::Nil),
            _ => Variant::Nil,
        },
        _ => Variant::Nil,
    }
}

/// **The acceptance test.**
#[test]
fn slide_collisions_reach_gdscript() {
    let (ml, body, wall) = run(READER, true);

    // (a) The count is readable at all — the bare call resolved.
    let hits = int_var(&ml, body, "hits");
    assert!(
        hits >= 1,
        "a CharacterBody2D walking into a wall must report at least one slide \
         collision; got {hits}. Zero means the contacts are still being computed \
         and discarded, or the bare call never resolved"
    );

    // (b) The object answers its own getters. ⚠️ `reached` guards the two
    //     assertions below from passing vacuously: if the `if hits > 0` branch
    //     never ran, `nx` and `grouped` would hold their initialisers and
    //     `grouped == false` would look like a real answer.
    assert!(
        int_var(&ml, body, "reached") >= 1,
        "the script never entered the collision branch, so the assertions below \
         would be reading initialisers rather than results"
    );

    let nx = match script_var(&ml, body, "nx") {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("nx should be a number, got {other:?}"),
    };
    assert!(
        nx < -0.9,
        "walking RIGHT into a wall gives a surface normal pointing LEFT \
         (about -1, 0); got x = {nx}"
    );

    // (c) The PUBLISH layer resolved the body to the wall node. Checked before
    //     the script assertion below so the two failures are distinguishable.
    let wall = wall.expect("the walled fixture has a wall");
    assert_eq!(
        published_collider(&ml, body),
        Variant::ObjectId(wall.object_id()),
        "the physics phase must publish the collider as the WALL's node id; a \
         Nil here means `body_to_node` never resolved it and the script layer \
         is not at fault"
    );

    // (d) The SCRIPT receives that same node id from `get_collider()`.
    //
    // ⚠️ Split from the `is_in_group` assertion below on purpose. Those are two
    // different mechanisms — the handle getter handing a value back, and node
    // dispatch on the value it handed — and collapsing them means a failure
    // cannot say which one broke. (c) already proved the publish is right, so
    // this pins the remaining half.
    assert_eq!(
        script_var(&ml, body, "who"),
        Some(Variant::ObjectId(wall.object_id())),
        "get_collider() must hand the script the wall's node id; anything else \
         means the handle getter, not node dispatch, is at fault"
    );

    // (e) ...and that node is callable like any other node. This is what makes
    //     the damage idiom writable.
    assert_eq!(
        script_var(&ml, body, "grouped"),
        Some(Variant::Bool(true)),
        "get_collider() must return the wall node itself, callable like any \
         node: `is_in_group(\"wall\")` is how the standard idiom identifies what \
         it hit"
    );
}

/// The control that makes the test above meaningful: the same script with
/// nothing to run into reports no collisions. Without it, a `get_slide_collision_count()`
/// that returned a constant would pass every assertion above.
#[test]
fn a_character_that_hits_nothing_reports_no_slide_collisions() {
    let (ml, body, _) = run(LONELY, false);
    assert_eq!(
        int_var(&ml, body, "hits"),
        0,
        "nothing was in the way, so the count must be 0 — a non-zero answer \
         means the count is not actually reading this tick's contacts"
    );
}
