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
use gdscene::physics_server::PhysicsServer;
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

// ---------------------------------------------------------------------------
// The `PhysicsServer2D` server forms (pat-s3rrv.100, last open clause)
// ---------------------------------------------------------------------------
//
// `PhysicsBody2D.add_collision_exception_with` above is the NODE form. Godot
// also exposes `PhysicsServer2D.body_add_collision_exception(body, excepted)`
// addressed by RID, which is how a script drives physics with no node of its
// own (`upstream/godot/servers/physics_server_2d.cpp`). Patina had the node form
// and not this one: `grep -rn '"body_add_collision_exception"'` over
// gdscript-interop returned zero.
//
// Both layers are covered, because a break in either is a different bug: the
// SERVER level (the RID registry over the shared world) and the SCRIPT level
// (the singleton dispatch arm reaching it from GDScript).

/// Half-extent of the server-created boxes.
///
/// ⚠️ Boxes rather than circles on purpose: two circles stacked at the same `x`
/// balance on a point, and any drift would make the control flaky. The node
/// tests above use boxes for the same reason, and a half-extent of 8 keeps the
/// resting geometry identical either way.
const R: f32 = 8.0;
/// A floor spanning `y ∈ [190, 210]`, so its top face is at 190.
const SRV_FLOOR_Y: f32 = 200.0;
const SRV_FLOOR_HALF: f32 = 10.0;
/// Where a single circle comes to rest on that floor.
const SRV_REST_Y: f32 = SRV_FLOOR_Y - SRV_FLOOR_HALF - R;
const DT: f32 = 1.0 / 60.0;

/// A `StaticBody2D` floor registered the ordinary (node) way, so the server
/// bodies below have something to land on that is NOT one of themselves — the
/// same control the node tests use.
fn server_floor_tree() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, SRV_FLOOR_Y)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property(
        "size",
        Variant::Vector2(Vector2::new(1000.0, SRV_FLOOR_HALF * 2.0)),
    );
    tree.add_child(floor_id, shape).unwrap();
    tree
}

/// Two RID-addressed circle bodies above the node floor, `upper` 40px above
/// `lower`, mirroring the node scene's geometry.
fn server_stack() -> (PhysicsServer, u64, u64) {
    let tree = server_floor_tree();
    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let shape = server.server_rectangle_shape_create();
    assert!(server.server_shape_set_data(shape, R, Some(Vector2::new(R, R))));

    let mut make = |y: f32| {
        let body = server.server_body_create();
        assert!(server.server_body_set_space(body, 1));
        assert!(server.server_body_add_shape(body, shape));
        assert!(server.server_body_set_state_vec2(body, 0, Vector2::new(0.0, y)));
        body
    };
    let lower = make(100.0);
    let upper = make(60.0);
    (server, upper, lower)
}

fn srv_y(server: &PhysicsServer, body: u64) -> f32 {
    server
        .server_body_position(body)
        .unwrap_or_else(|| panic!("RID {body} must be a live body in the world"))
        .y
}

fn srv_settle(server: &mut PhysicsServer) {
    for _ in 0..180 {
        server.step_physics(DT);
    }
}

/// ⚠️ The control. Two RID bodies with no exception STACK — without this, every
/// "they passed through" assertion below would pass against a world that never
/// made them collide.
#[test]
fn without_an_exception_two_server_bodies_stack() {
    let (mut server, upper, lower) = server_stack();
    srv_settle(&mut server);

    let (uy, ly) = (srv_y(&server, upper), srv_y(&server, lower));
    assert!(
        (ly - SRV_REST_Y).abs() < 3.0,
        "the lower RID body must rest on the node floor at ~{SRV_REST_Y}, got {ly}"
    );
    assert!(
        ly - uy > R,
        "the upper RID body must settle ON the lower one — separation {} is too \
         small for a stack, so they already pass through each other",
        ly - uy
    );
}

/// **The server-level acceptance.** `body_add_collision_exception` makes two RID
/// bodies interpenetrate while BOTH still land on the node floor.
#[test]
fn the_server_exception_form_lets_two_rid_bodies_pass_through() {
    let (mut server, upper, lower) = server_stack();
    assert!(
        server.server_body_add_collision_exception(upper, lower),
        "both RIDs are live bodies in a space, so the call must report applied"
    );
    srv_settle(&mut server);

    let (uy, ly) = (srv_y(&server, upper), srv_y(&server, lower));
    assert!(
        (ly - uy).abs() < R,
        "the excepted RID bodies must occupy the same space; still {} apart",
        ly - uy
    );
    // 🔑 Both still collide with everything else. An exception that disabled the
    // bodies outright would satisfy the assertion above with both of them far
    // below the floor.
    assert!(
        (ly - SRV_REST_Y).abs() < 3.0,
        "the lower body must still rest on the floor at ~{SRV_REST_Y}, got {ly}"
    );
    assert!(
        (uy - SRV_REST_Y).abs() < 3.0,
        "the upper body must ALSO rest on the floor at ~{SRV_REST_Y}, got {uy} — \
         falling past it means the exception leaked into every pair"
    );
}

/// `body_remove_collision_exception` restores blocking.
///
/// ⚠️ Removed while the bodies are still far apart and falling, not after they
/// have interpenetrated: un-excepting overlapping bodies would test the
/// solver's ability to push them apart, which is a different mechanism.
#[test]
fn removing_the_server_exception_restores_blocking() {
    let (mut server, upper, lower) = server_stack();
    assert!(server.server_body_add_collision_exception(upper, lower));
    for _ in 0..5 {
        server.step_physics(DT);
    }
    assert!(
        server.server_body_remove_collision_exception(upper, lower),
        "removing a live exception must report applied"
    );
    srv_settle(&mut server);

    let (uy, ly) = (srv_y(&server, upper), srv_y(&server, lower));
    assert!(
        ly - uy > R,
        "with the exception removed the bodies must stack again — separation \
         {} means the pair filter never went back to reporting contacts",
        ly - uy
    );
    assert!(
        (ly - SRV_REST_Y).abs() < 3.0,
        "the lower body must still rest on the floor at ~{SRV_REST_Y}, got {ly}"
    );
}

/// A RID that is not a live body reports "not applied" rather than silently
/// dropping the exception.
///
/// Both halves matter: `body_create` hands back a RID whose body does not exist
/// until `body_set_space`, so "the RID is real" and "the body is in the world"
/// are different questions, and only the second one can carry an exception.
#[test]
fn the_server_exception_form_rejects_a_rid_with_no_body() {
    let (mut server, upper, lower) = server_stack();

    let spaceless = server.server_body_create();
    assert!(
        server.server_body_id(spaceless).is_none(),
        "a body with no space is not in the world yet"
    );
    assert!(
        !server.server_body_add_collision_exception(upper, spaceless),
        "excepting a spaceless body must report not-applied, not pretend to work"
    );
    assert!(
        !server.server_body_add_collision_exception(spaceless, lower),
        "the same must hold with the spaceless RID named first"
    );
    assert!(
        !server.server_body_add_collision_exception(upper, 999_999),
        "a RID the server never minted must report not-applied"
    );

    // The real pair is untouched by any of that: the bodies still stack.
    srv_settle(&mut server);
    assert!(
        srv_y(&server, lower) - srv_y(&server, upper) > R,
        "a rejected call must not have excepted the pair anyway"
    );
}

/// **The script-level acceptance.** A GDScript reaches the exception through the
/// `PhysicsServer2D` singleton, with no node of its own for either body.
///
/// The resting positions are read from the SERVER, not relayed through script
/// variables: the engine is what decides where the bodies end up, and a test
/// that asserted on a value the script had written could not see the engine
/// disagree.
#[test]
fn a_script_excepts_two_bodies_through_the_physics_server_singleton() {
    const DRIVER_GD: &str = "\
extends Node2D
var upper_rid = 0
var lower_rid = 0
var made = false
var ticks = 0
func _physics_process(delta):
    if self.made == false:
        self.made = true
        var shape = PhysicsServer2D.rectangle_shape_create()
        PhysicsServer2D.shape_set_data(shape, Vector2(8, 8))
        self.lower_rid = PhysicsServer2D.body_create()
        PhysicsServer2D.body_set_space(self.lower_rid, 1)
        PhysicsServer2D.body_add_shape(self.lower_rid, shape)
        PhysicsServer2D.body_set_state(self.lower_rid, 0, Vector2(0, 100))
        self.upper_rid = PhysicsServer2D.body_create()
        PhysicsServer2D.body_set_space(self.upper_rid, 1)
        PhysicsServer2D.body_add_shape(self.upper_rid, shape)
        PhysicsServer2D.body_set_state(self.upper_rid, 0, Vector2(0, 60))
    PhysicsServer2D.body_add_collision_exception(self.upper_rid, self.lower_rid)
    self.ticks = self.ticks + 1
";

    let mut tree = server_floor_tree();
    let root = tree.root_id();
    let driver = tree.add_child(root, Node::new("Driver", "Node2D")).unwrap();
    tree.attach_script(
        driver,
        Box::new(GDScriptNodeInstance::from_source(DRIVER_GD, driver).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    // Only this populates `node_to_body`; without it the floor is not in the
    // world and nothing would stop the falling bodies.
    ml.register_physics_bodies();
    settle(&mut ml);

    let var_of = |name: &str| ml.tree().get_node(driver).map(|n| n.get_property(name));
    let rid = |name: &str| -> u64 {
        match var_of(name) {
            Some(Variant::Int(i)) if i > 0 => i as u64,
            other => panic!("{name} must be a live RID, got {other:?}"),
        }
    };
    // A handler that throws part-way stops incrementing `ticks`, so this is the
    // "the dispatch arm exists and did not raise" assertion. An unwired
    // `PhysicsServer2D` method is a RuntimeError, not a silent no-op.
    let ticks = match var_of("ticks") {
        Some(Variant::Int(i)) => i,
        other => panic!("ticks must be an int, got {other:?}"),
    };
    assert!(
        ticks > 100,
        "every physics tick must have run the handler to completion, got \
         {ticks} — a RuntimeError from an unwired \
         PhysicsServer2D.body_add_collision_exception stops the count"
    );

    let (upper, lower) = (rid("upper_rid"), rid("lower_rid"));
    let server = ml.physics_server();
    let (uy, ly) = (srv_y(server, upper), srv_y(server, lower));
    assert!(
        (ly - uy).abs() < R,
        "the script's exception must make the two RID bodies interpenetrate; \
         still {} apart",
        ly - uy
    );
    assert!(
        (ly - SRV_REST_Y).abs() < 3.0,
        "the lower body must rest on the node floor at ~{SRV_REST_Y}, got {ly}"
    );
    assert!(
        (uy - SRV_REST_Y).abs() < 3.0,
        "the upper body must ALSO rest on the floor at ~{SRV_REST_Y}, got {uy}"
    );
}
