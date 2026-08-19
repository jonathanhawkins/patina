//! pat-s3rrv.100 — per-body collision exceptions:
//! `PhysicsBody2D.add_collision_exception_with(body)`.
//!
//! Two bodies listed as exceptions pass THROUGH each other while both keep
//! colliding with everything else. It is how a bullet stops hitting its
//! shooter and a rider its vehicle, and Patina had none of it: `grep -rn
//! "exception" gdphysics2d/src/` returned exactly one doc comment, about the
//! unrelated `RayCast2D.add_exception`.
//!
//! ⚠️ Every test here asserts the floor as well as the pair. "They passed
//! through each other" and "collision stopped working for these bodies" produce
//! the same y-separation, and only the floor tells them apart — an exception
//! that leaked into the rest of the world would look exactly like a success.

use gdcore::math::Vector2;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Box half-extent, so a resting stack separates by one full box height.
const BOX: f32 = 10.0;
/// Floor centre; its top face is at `FLOOR_Y - BOX`.
const FLOOR_Y: f32 = 300.0;
/// Where a single box comes to rest on the floor: floor top minus its own half.
const REST_Y: f32 = FLOOR_Y - BOX - BOX;

/// `Root -> {Floor, Lower, Upper}`. `Upper` starts 40px above `Lower`, so with
/// ordinary collision it lands on top of it and the two settle a box apart.
fn stack_scene(upper_script: Option<&str>) -> (MainLoop, NodeId, NodeId) {
    build_stack(upper_script.map(|src| ("Upper", src)))
}

/// The same scene with the script on whichever box is named.
///
/// 🔑 Which box runs the script is not cosmetic — see
/// [`an_exception_is_symmetric_whichever_body_declares_it`]. `Lower` is created
/// FIRST, so it holds the smaller `NodeId` and therefore the smaller `BodyId`,
/// and the solver's pair loop visits each unordered pair once as
/// `(a = lower id, b = higher id)`. Scripting `Upper` exercises only one operand
/// of the symmetric test; scripting `Lower` exercises the other.
fn build_stack(script: Option<(&str, &str)>) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, FLOOR_Y)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut floor_shape = Node::new("FloorShape", "CollisionShape2D");
    floor_shape.set_property("size", Variant::Vector2(Vector2::new(400.0, BOX * 2.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    let mut make_box = |name: &str, y: f32| {
        let mut b = Node::new(name, "RigidBody2D");
        b.set_property("position", Variant::Vector2(Vector2::new(0.0, y)));
        let id = tree.add_child(root, b).unwrap();
        let mut sh = Node::new("Shape", "CollisionShape2D");
        sh.set_property("size", Variant::Vector2(Vector2::new(BOX * 2.0, BOX * 2.0)));
        tree.add_child(id, sh).unwrap();
        id
    };
    let lower = make_box("Lower", 0.0);
    let upper = make_box("Upper", -40.0);

    if let Some((target, src)) = script {
        let id = if target == "Lower" { lower } else { upper };
        tree.attach_script(
            id,
            Box::new(GDScriptNodeInstance::from_source(src, id).unwrap()),
        );
    }

    (MainLoop::new(tree), upper, lower)
}

fn y_of(ml: &MainLoop, id: NodeId) -> f32 {
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v.y,
        other => panic!("body must carry a Vector2 position, got {other:?}"),
    }
}

fn settle(ml: &mut MainLoop) {
    for _ in 0..180 {
        ml.step(1.0 / 60.0);
    }
}

/// ⚠️ The control. Without an exception the two boxes STACK — if they did not,
/// every "they passed through" assertion below would pass against a physics
/// world that never made them collide in the first place.
#[test]
fn without_an_exception_the_boxes_stack() {
    let (mut ml, upper, lower) = stack_scene(None);
    settle(&mut ml);

    let (uy, ly) = (y_of(&ml, upper), y_of(&ml, lower));
    assert!(
        (ly - REST_Y).abs() < 3.0,
        "the lower box must come to rest on the floor at ~{REST_Y}, got {ly}"
    );
    assert!(
        ly - uy > BOX,
        "the upper box must settle ON the lower one — separation {} is too \
         small for a stack, so they are already passing through each other \
         and the exception tests below would prove nothing",
        ly - uy
    );
}

/// **The acceptance.** With the exception the boxes interpenetrate, and BOTH
/// still land on the floor.
#[test]
fn an_exception_lets_two_bodies_pass_through_each_other() {
    // Called every tick on purpose. `add_collision_exception` is idempotent, and
    // a script cannot tell when the body became registered — bodies enter the
    // world on the first `sync_to_physics`, which a `_ready` call would precede.
    // Re-asserting it each frame is both the safe idiom and the one games use.
    let script = "\
extends RigidBody2D
var ticks = 0
func _physics_process(delta):
    self.ticks = self.ticks + 1
    add_collision_exception_with(get_node(\"../Lower\"))
";
    let (mut ml, upper, lower) = stack_scene(Some(script));
    settle(&mut ml);

    let (uy, ly) = (y_of(&ml, upper), y_of(&ml, lower));
    assert!(
        (ly - uy).abs() < BOX,
        "the boxes must occupy the same space once excepted; they are still {} \
         apart, i.e. still stacked",
        ly - uy
    );
    // 🔑 Both still collide with everything else. Without this, an exception
    // that disabled the bodies entirely would satisfy the assertion above and
    // both boxes would be somewhere far below the floor.
    assert!(
        (ly - REST_Y).abs() < 3.0,
        "the lower box must still rest on the floor at ~{REST_Y}, got {ly}"
    );
    assert!(
        (uy - REST_Y).abs() < 3.0,
        "the upper box must ALSO rest on the floor at ~{REST_Y}, got {uy} — a \
         box that kept falling means the exception leaked into every pair, not \
         just this one"
    );
}

/// `remove_collision_exception_with` restores blocking on a later tick.
///
/// ⚠️ The exception is removed while the boxes are still far apart and falling,
/// NOT after they have interpenetrated. Removing it on overlapping bodies would
/// test the solver's ability to push them apart, which is a different mechanism
/// — this asserts the pair filter itself went back to reporting contacts.
#[test]
fn removing_an_exception_restores_blocking() {
    let script = "\
extends RigidBody2D
var ticks = 0
func _physics_process(delta):
    self.ticks = self.ticks + 1
    if self.ticks < 10:
        add_collision_exception_with(get_node(\"../Lower\"))
    elif self.ticks == 10:
        remove_collision_exception_with(get_node(\"../Lower\"))
";
    let (mut ml, upper, lower) = stack_scene(Some(script));
    settle(&mut ml);

    let (uy, ly) = (y_of(&ml, upper), y_of(&ml, lower));
    assert!(
        ly - uy > BOX,
        "with the exception removed before the boxes met, they must stack \
         again — separation {} says they still pass through",
        ly - uy
    );
    assert!(
        (ly - REST_Y).abs() < 3.0,
        "and the lower box still rests on the floor, got {ly}"
    );
}

/// 🔑 The exception holds whichever body declares it.
///
/// ⚠️ **This is the test the other four cannot substitute for.** Godot stores
/// the exception on the body the script called the method on, but its broadphase
/// skips the pair in EITHER direction, so `excludes` asks both sides:
/// `a.lists(b.id) || b.lists(a.id)`.
///
/// Every other test here scripts `Upper`, which is created second and so holds
/// the higher `BodyId`. The pair loop visits the pair as `(a = Lower, b = Upper)`,
/// meaning those tests only ever exercise the **second** operand — an
/// implementation that checked just `b.lists(a.id)` would pass all four of them
/// and still break the moment the shooter happened to be spawned before the
/// bullet. This scripts `Lower` instead, so the FIRST operand is what has to
/// fire.
#[test]
fn an_exception_is_symmetric_whichever_body_declares_it() {
    let script = "\
extends RigidBody2D
var ticks = 0
func _physics_process(delta):
    self.ticks = self.ticks + 1
    add_collision_exception_with(get_node(\"../Upper\"))
";
    let (mut ml, upper, lower) = build_stack(Some(("Lower", script)));
    settle(&mut ml);

    let (uy, ly) = (y_of(&ml, upper), y_of(&ml, lower));
    assert!(
        (ly - uy).abs() < BOX,
        "declaring the exception on the LOWER body must work exactly as well as \
         declaring it on the upper one; the boxes are still {} apart, so only \
         one direction of the pair test is wired",
        ly - uy
    );
    assert!(
        (ly - REST_Y).abs() < 3.0,
        "and both must still land on the floor, got lower {ly}"
    );
    assert!((uy - REST_Y).abs() < 3.0, "and upper {uy}");
}

/// `get_collision_exceptions()` reports what was added, as real nodes.
#[test]
fn get_collision_exceptions_lists_the_excepted_nodes() {
    let script = "\
extends RigidBody2D
var ticks = 0
var count = -1
var first = null
func _physics_process(delta):
    self.ticks = self.ticks + 1
    add_collision_exception_with(get_node(\"../Lower\"))
    var ex = get_collision_exceptions()
    self.count = ex.size()
    if ex.size() > 0:
        self.first = ex[0]
";
    let (mut ml, upper, lower) = stack_scene(Some(script));
    for _ in 0..20 {
        ml.step(1.0 / 60.0);
    }

    let count = match ml.tree().get_node(upper).map(|n| n.get_property("count")) {
        Some(Variant::Int(n)) => n,
        other => panic!("`count` must be an int, got {other:?}"),
    };
    assert_eq!(
        count, 1,
        "exactly one exception must be listed; 0 means the add never reached a \
         registered body, and >1 means repeated calls are not idempotent"
    );
    // 🔑 The entry must be the Lower NODE, not a bare body id. The accessor
    // maps `BodyId` back through the server's reverse table, and getting that
    // wrong yields a plausible-looking id that names nothing — so this compares
    // against the real node rather than merely checking the list is non-empty.
    assert_eq!(
        ml.tree().get_node(upper).map(|n| n.get_property("first")),
        Some(Variant::ObjectId(lower.object_id())),
        "the listed entry must be the Lower node itself"
    );
}

// -- CharacterBody2D --------------------------------------------------------
//
// ⚠️ A kinematic body does NOT go through the solver's pair loop: it is swept
// against an obstacle list built by `PhysicsServer::obstacle_bodies`, so the
// exception has to be honoured there as well. Gating only the rigid-body pair
// filter leaves a character walking straight into a wall it was excepted from —
// which is the half of this feature games actually use for one-way doors and
// pass-through platforms.

const WALL_X: f32 = 100.0;

/// `Root -> {Wall, Walker}`, the walker driven at the wall every tick.
fn walker_scene(except_the_wall: bool) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();

    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(WALL_X, 0.0)));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut ws = Node::new("WallShape", "CollisionShape2D");
    ws.set_property("size", Variant::Vector2(Vector2::new(20.0, 400.0)));
    tree.add_child(wall_id, ws).unwrap();

    let walker = tree
        .add_child(root, Node::new("Walker", "CharacterBody2D"))
        .unwrap();
    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("size", Variant::Vector2(Vector2::new(20.0, 20.0)));
    tree.add_child(walker, cs).unwrap();

    // The exception is re-asserted every tick: `add_collision_exception` is
    // idempotent, and a script cannot tell when the body became registered.
    let except_line = if except_the_wall {
        "    add_collision_exception_with(get_node(\"../Wall\"))\n"
    } else {
        ""
    };
    let script = format!(
        "\
extends CharacterBody2D
var ticks = 0
func _physics_process(delta):
    self.ticks = self.ticks + 1
{except_line}    velocity = Vector2(200, 0)
    move_and_slide()
"
    );
    tree.attach_script(
        walker,
        Box::new(GDScriptNodeInstance::from_source(&script, walker).unwrap()),
    );
    (MainLoop::new(tree), walker)
}

fn x_of(ml: &MainLoop, id: NodeId) -> f32 {
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v.x,
        other => panic!("walker must carry a Vector2 position, got {other:?}"),
    }
}

/// ⚠️ The control: without an exception the wall stops the walker.
#[test]
fn without_an_exception_a_character_is_stopped_by_the_wall() {
    let (mut ml, walker) = walker_scene(false);
    settle(&mut ml);
    let x = x_of(&ml, walker);
    assert!(
        x < WALL_X,
        "the walker must be blocked by the wall at x = {WALL_X}, but reached \
         {x} — if it passes through unconditionally, the exception test below \
         proves nothing"
    );
}

/// **The last acceptance clause.** A CharacterBody2D excepted from the wall
/// walks through it.
#[test]
fn a_character_with_an_exception_walks_through_the_wall() {
    let (mut ml, walker) = walker_scene(true);
    settle(&mut ml);
    let x = x_of(&ml, walker);
    assert!(
        x > WALL_X + 20.0,
        "with the wall excepted the walker must pass clean through it, but \
         stopped at {x} (wall at {WALL_X}) — the kinematic sweep builds its own \
         obstacle list, so the solver's pair filter alone does not cover it"
    );
}
