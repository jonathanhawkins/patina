//! pat-s3rrv.103 — `CharacterBody3D`'s velocity getters answer in **3D**.
//!
//! `get_real_velocity()` and `get_platform_velocity()` live in the script table
//! shared by `CharacterBody2D` and `CharacterBody3D`, and that table was 2D
//! only: both getters wrapped their result in `Variant::Vector2`.
//!
//! ⚠️ The failure was worse than a wrong type. The reader they used matched
//! `Variant::Vector2` and fell through to `ZERO` for anything else — and the 3D
//! solver publishes `Vector3`. So a 3D script did not get a type error, it got
//! **zero**: a body riding a moving platform read `(0, 0)`, which reads as "the
//! platform is not moving" rather than as a bug. A wrong type announces itself;
//! a wrong value that is a plausible resting state does not.
//!
//! Two independent defects met here, and the tests keep them distinguishable:
//!
//! 1. `platform_velocity` **was** published as `Vector3` and thrown away by the
//!    `Vector2`-only reader (a getter bug).
//! 2. `_real_velocity` was never published by the 3D solver at all — only
//!    `physics_server.rs` (2D) wrote it — so there was nothing to read even
//!    with a fixed getter (a producer bug).

use gdcore::math::{Vector2, Vector3};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// `TYPE_VECTOR3` / `TYPE_VECTOR2` as GDScript's `typeof()` reports them.
const TYPE_VECTOR2: i64 = 5;
const TYPE_VECTOR3: i64 = 9;

/// A scripted `CharacterBody3D` standing on a wide static floor.
///
/// ⚠️ The `CollisionShape3D` child is not decoration: `process_character_movement`
/// resolves the body's shape with a `find_map` over its children and `continue`s
/// when there is none, so a body without one is silently skipped and every
/// assertion below would read an initialiser.
fn walker_scene(script: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let mut floor = Node::new("Floor", "StaticBody3D");
    // ⚠️ Placed at -2.0, not -1.0. A 1.0-tall box at -1.0 has its top face at
    // exactly -0.5, which is exactly where the player sphere's bottom sits — a
    // tangency, the coincidence point where "resting on" and "not touching"
    // agree. The floor is not load-bearing for anything asserted here (the
    // script holds velocity.y at 0, so the body never falls); it is scene
    // furniture, and it should not sit on a degenerate contact.
    floor.set_property("position", Variant::Vector3(Vector3::new(0.0, -2.0, 0.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut floor_shape = Node::new("FloorShape", "CollisionShape3D");
    floor_shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    floor_shape.set_property("size", Variant::Vector3(Vector3::new(200.0, 1.0, 200.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    let player = tree
        .add_child(root, Node::new("Player", "CharacterBody3D"))
        .unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_string()));
    shape.set_property("radius", Variant::Float(0.5));
    tree.add_child(player, shape).unwrap();
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(script, player).unwrap()),
    );

    (MainLoop::new(tree), player)
}

fn prop(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_node(node)
        .expect("player node")
        .get_property(name)
}

fn int_prop(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match prop(ml, node, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` must be an int, got {other:?}"),
    }
}

/// A scripted `CharacterBody3D` RESTING on a floor with a wall to its right.
///
/// ⚠️ Different from [`walker_scene`] in the one way that matters here: the body
/// genuinely rests on the floor rather than floating above it, because
/// `is_on_floor_only()` is meaningless unless `is_on_floor()` is true. The
/// sphere has radius 0.5 and the floor's top face is y = 0, so the body sits
/// centred at y = 0.5 and the script lets gravity hold it there.
fn floor_and_wall_scene(script: &str, slide_on_ceiling: Option<bool>) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    // Floor: 1.0 tall, centred at -0.5, so its top face is exactly y = 0.
    let mut floor = Node::new("Floor", "StaticBody3D");
    floor.set_property("position", Variant::Vector3(Vector3::new(0.0, -0.5, 0.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut floor_shape = Node::new("FloorShape", "CollisionShape3D");
    floor_shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    floor_shape.set_property("size", Variant::Vector3(Vector3::new(200.0, 1.0, 200.0)));
    tree.add_child(floor_id, floor_shape).unwrap();

    // Wall: a slab at x = 2.5, so its -X face is at x = 2.0. A sphere of radius
    // 0.5 walking right first touches it centred at x = 1.5.
    let mut wall = Node::new("Wall", "StaticBody3D");
    wall.set_property("position", Variant::Vector3(Vector3::new(2.5, 2.0, 0.0)));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut wall_shape = Node::new("WallShape", "CollisionShape3D");
    wall_shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    wall_shape.set_property("size", Variant::Vector3(Vector3::new(1.0, 8.0, 200.0)));
    tree.add_child(wall_id, wall_shape).unwrap();

    let player = tree
        .add_child(root, Node::new("Player", "CharacterBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, player, Vector3::new(0.0, 0.5, 0.0));
    if let Some(flag) = slide_on_ceiling {
        if let Some(n) = tree.get_node_mut(player) {
            n.set_property("slide_on_ceiling", Variant::Bool(flag));
        }
    }
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_string()));
    shape.set_property("radius", Variant::Float(0.5));
    tree.add_child(player, shape).unwrap();
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(script, player).unwrap()),
    );

    (MainLoop::new(tree), player)
}

fn bool_prop_of(ml: &MainLoop, node: NodeId, name: &str) -> bool {
    match prop(ml, node, name) {
        Variant::Bool(b) => b,
        other => panic!("`{name}` must be a bool, got {other:?}"),
    }
}

fn vec3_prop_of(ml: &MainLoop, node: NodeId, name: &str) -> Vector3 {
    match prop(ml, node, name) {
        Variant::Vector3(v) => v,
        other => panic!("`{name}` must be a Vector3, got {other:?}"),
    }
}

/// **The bead's named acceptance.** Clauses (a)-(c): 3D-typed getters, the
/// `_only` predicates distinguishing floor-and-wall from floor-alone, and a
/// slide-collision report carrying the wall normal.
#[test]
fn character_body_3d_script_surface() {
    // Walk right into the wall while gravity holds the body on the floor.
    let script = "\
extends CharacterBody3D
var ticks = 0
var rv_type = -1
var pv_type = -1
var rv = Vector3(-1, -1, -1)
var on_floor = false
var on_floor_only = true
var on_wall = false
var hits = -1
var first_normal_x = 0.0
func _physics_process(delta):
    velocity = Vector3(6, -4, 0)
    move_and_slide()
    self.ticks = self.ticks + 1
    self.rv_type = typeof(get_real_velocity())
    self.pv_type = typeof(get_platform_velocity())
    self.rv = get_real_velocity()
    self.on_floor = is_on_floor()
    self.on_floor_only = is_on_floor_only()
    self.on_wall = is_on_wall()
    self.hits = get_slide_collision_count()
    if self.hits > 0:
        var c0 = get_slide_collision(0)
        var n0 = c0.get_normal()
        self.first_normal_x = n0.x
";
    let (mut ml, player) = floor_and_wall_scene(script, None);
    for _ in 0..90 {
        ml.step(1.0 / 60.0);
    }

    // Guard the guard: with no ticks every value below is still its
    // initialiser, and each assertion would fail for the wrong reason.
    assert!(
        int_prop(&ml, player, "ticks") > 0,
        "`_physics_process` never ran, so nothing below is evidence"
    );

    // -- (a) the getters answer in 3D ---------------------------------------
    assert_eq!(
        int_prop(&ml, player, "rv_type"),
        TYPE_VECTOR3,
        "get_real_velocity() must be a Vector3 on a CharacterBody3D"
    );
    assert_eq!(
        int_prop(&ml, player, "pv_type"),
        TYPE_VECTOR3,
        "get_platform_velocity() must be a Vector3 on a CharacterBody3D"
    );

    // -- (b) floor AND wall, so `_only` must be FALSE ------------------------
    //
    // ⚠️ The pair is asserted together. `is_on_floor_only()` returning false is
    // only meaningful if the body is actually on the floor — otherwise an
    // implementation that always answers false would pass.
    assert!(
        bool_prop_of(&ml, player, "on_floor"),
        "the body must be resting on the floor"
    );
    assert!(
        bool_prop_of(&ml, player, "on_wall"),
        "…and pressed against the wall, or the _only predicate proves nothing"
    );
    assert!(
        !bool_prop_of(&ml, player, "on_floor_only"),
        "is_on_floor_only() must be FALSE while also touching a wall — it is a \
         conjunction, not an alias for is_on_floor()"
    );

    // -- (c) the slide-collision report --------------------------------------
    let hits = int_prop(&ml, player, "hits");
    assert!(
        hits >= 1,
        "walking into a wall must record at least one slide collision, got {hits}"
    );
    // The wall's -X face points back toward the body, i.e. -X.
    //
    // ⚠️ Read as a scalar, and bound to a local in the script before the field
    // access: `get_slide_collision(0).get_normal()` chained in one expression
    // has no precedent in any passing test, whereas the two-step form is what
    // `characterbody2d_slide_collision_script_api` already proves parses.
    let nx = match prop(&ml, player, "first_normal_x") {
        Variant::Float(f) => f,
        other => panic!("first_normal_x must be a float, got {other:?}"),
    };
    assert!(
        nx < -0.5,
        "get_slide_collision(0).get_normal() must be the wall's surface normal, \
         pointing back at the body along -X; got x = {nx}"
    );
    // Real velocity must reflect being stopped by the wall, not the input.
    let rv = vec3_prop_of(&ml, player, "rv");
    assert!(
        rv.x < 1.0,
        "the body is against a wall, so its REAL x velocity must be far below \
         the requested 6; got {rv:?}"
    );
}

/// **Clause (d).** `slide_on_ceiling = false` stops a body dead on ceiling
/// contact instead of letting it slide along.
///
/// ⚠️ Run as a PAIR with the `true` case in the same test. The flag's whole
/// meaning is the difference between the two, and asserting only the `false`
/// side would pass against an engine that never slides on ceilings at all.
#[test]
fn character_body_3d_script_surface_slide_on_ceiling_stops_dead() {
    // Jump straight up into the wall slab's underside. The slab spans
    // y = -2..6 at x = 2.5, so a body launched upward at x = 2.2 meets it.
    // ⚠️ The jump is IMPULSIVE — velocity is set once, on the first tick only.
    //
    // My first version re-assigned `velocity = Vector3(0, 8, 0)` at the top of
    // every tick and then read `velocity.y` back after `move_and_slide()`. That
    // can never observe the cancellation: the solver's post-move velocity is
    // published by `process_character_movement`, which runs AFTER the script's
    // tick, so the script's own assignment on the following tick overwrites the
    // zero before anything can read it. It reported 8 forever — not because the
    // engine failed to cancel, but because the fixture kept re-arming the jump.
    //
    // A real jump is an impulse, so this models one and lets the engine own the
    // velocity from then on.
    let script = "\
extends CharacterBody3D
var ticks = 0
var hit_ceiling = false
func _physics_process(delta):
    if self.ticks == 0:
        velocity = Vector3(0, 8, 0)
    move_and_slide()
    self.ticks = self.ticks + 1
    if is_on_ceiling():
        self.hit_ceiling = true
";
    // ⚠️ A DEDICATED ceiling, not the wall slab. My first attempt reused the
    // wall by standing the body at x = 2.4 — which is INSIDE that slab
    // (it spans x 2.0..3.0, y -2..6), so the body started overlapping it and
    // never "rose into" anything. The guard below caught that; without it the
    // `velocity.y == 0` assertion would have passed vacuously on a body that
    // touched nothing.
    let ceiling_scene = |flag: bool| {
        let (mut ml, player) = floor_and_wall_scene(script, Some(flag));
        let root = ml.tree().root_id();
        let host = ml.tree().find_child(root, "Root", true).expect("Root");
        let mut ceiling = Node::new("Ceiling", "StaticBody3D");
        // 1.0 tall centred at y = 3.0, so its underside is exactly y = 2.5.
        // The body's top is at y = 1.0, leaving 1.5 of clear rise.
        ceiling.set_property("position", Variant::Vector3(Vector3::new(0.0, 3.0, 0.0)));
        let cid = ml.tree_mut().add_child(host, ceiling).unwrap();
        let mut cshape = Node::new("CeilingShape", "CollisionShape3D");
        cshape.set_property("shape", Variant::String("BoxShape3D".to_string()));
        cshape.set_property("size", Variant::Vector3(Vector3::new(200.0, 1.0, 200.0)));
        ml.tree_mut().add_child(cid, cshape).unwrap();
        // Well clear of the wall slab at x = 2.0..3.0.
        node3d::set_position(ml.tree_mut(), player, Vector3::new(0.0, 0.5, 0.0));
        for _ in 0..60 {
            ml.step(1.0 / 60.0);
        }
        (ml, player)
    };

    let (ml_off, p_off) = ceiling_scene(false);
    assert!(
        int_prop(&ml_off, p_off, "ticks") > 0,
        "`_physics_process` never ran"
    );
    assert!(
        bool_prop_of(&ml_off, p_off, "hit_ceiling"),
        "the body must actually reach the ceiling, or clause (d) is vacuous"
    );
    // ⚠️ Read from the NODE, not from a script variable. The solver publishes
    // the post-move velocity in `process_character_movement`, which runs after
    // the script's tick — so the node property is the only place the
    // cancellation is observable at all.
    let vy = vec3_prop_of(&ml_off, p_off, "velocity").y;
    assert!(
        vy <= 0.0,
        "with slide_on_ceiling = false a jump into the ceiling must be \
         cancelled — the body's upward velocity must not survive contact; got \
         y = {vy}"
    );

    // The control: with the flag ON the body still reaches the ceiling. Without
    // this, an engine that simply never moved anything upward would satisfy the
    // assertion above.
    let (ml_on, p_on) = ceiling_scene(true);
    assert!(
        bool_prop_of(&ml_on, p_on, "hit_ceiling"),
        "the slide_on_ceiling = true body must reach the ceiling too, or the \
         two cases are not comparable"
    );
}

/// **The acceptance.** Both getters answer `Vector3` on a `CharacterBody3D`.
#[test]
fn character_body_3d_velocity_getters_are_vector3() {
    // `velocity` is assigned bare rather than via `self.`, the way an exported
    // Godot scene drives a body — the interpreter routes it to the node
    // property because `CharacterBody3D.velocity` has a non-Nil ClassDB default.
    let script = "\
extends CharacterBody3D
var ticks = 0
var rv = Vector3(-1, -1, -1)
var pv = Vector3(-1, -1, -1)
var rv_type = -1
var pv_type = -1
func _physics_process(delta):
    velocity = Vector3(4, 0, 0)
    move_and_slide()
    self.ticks = self.ticks + 1
    self.rv = get_real_velocity()
    self.pv = get_platform_velocity()
    self.rv_type = typeof(get_real_velocity())
    self.pv_type = typeof(get_platform_velocity())
";
    let (mut ml, player) = walker_scene(script);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }

    // Asserted first: with no ticks, every value below is still its
    // initialiser, and `rv_type == -1` would read as "not a Vector3" for the
    // wrong reason entirely.
    assert!(
        int_prop(&ml, player, "ticks") > 0,
        "`_physics_process` never ran, so nothing below is evidence of anything"
    );

    // 🔑 The acceptance line: `typeof()` as the script sees it.
    assert_eq!(
        int_prop(&ml, player, "rv_type"),
        TYPE_VECTOR3,
        "get_real_velocity() must be a Vector3 on a CharacterBody3D; {TYPE_VECTOR2} \
         (TYPE_VECTOR2) means the shared script table is still answering in 2D"
    );
    assert_eq!(
        int_prop(&ml, player, "pv_type"),
        TYPE_VECTOR3,
        "get_platform_velocity() must be a Vector3 on a CharacterBody3D"
    );

    // ⚠️ And the VALUE, not just the type. The old reader returned
    // `Vector2::ZERO`, so a test that only checked "is it 2D?" would still pass
    // against a getter that answered `Vector3::ZERO` for every body forever.
    // The walker is driven at 4 units/s along +x and nothing obstructs it, so
    // the motion it actually achieved must show up on x.
    match prop(&ml, player, "rv") {
        Variant::Vector3(v) => assert!(
            (v.x - 4.0).abs() < 0.5,
            "get_real_velocity().x must reflect the ~4/s the body actually \
             travelled, got {v:?} — a zero here means the getter answers the \
             right TYPE with a dead value"
        ),
        other => panic!("`rv` must be a Vector3, got {other:?}"),
    }

    // Positive control on the producer: the body really did move, so a
    // `_real_velocity` of zero would be the solver's silence, not a true rest.
    let end = node3d::get_position(ml.tree(), player);
    assert!(
        end.x > 1.0,
        "the walker must have travelled along +x for the velocity above to mean \
         anything, ended at {end:?}"
    );
}

/// 🔑 The 2D body must keep answering in 2D.
///
/// ⚠️ The obvious fix — make the getters return `Vector3` — passes every 3D
/// assertion above and breaks every 2D script in the engine. The getters are
/// one shared table, so widening it is only correct if the 2D half is pinned.
#[test]
fn character_body_2d_velocity_getters_stay_vector2() {
    let script = "\
extends CharacterBody2D
var ticks = 0
var rv_type = -1
var pv_type = -1
func _physics_process(delta):
    velocity = Vector2(4, 0)
    move_and_slide()
    self.ticks = self.ticks + 1
    self.rv_type = typeof(get_real_velocity())
    self.pv_type = typeof(get_platform_velocity())
";
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    let body = tree
        .add_child(root, Node::new("Body", "CharacterBody2D"))
        .unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(Vector2::new(10.0, 10.0)));
    tree.add_child(body, shape).unwrap();
    tree.attach_script(
        body,
        Box::new(GDScriptNodeInstance::from_source(script, body).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        int_prop(&ml, body, "ticks") > 0,
        "the 2D `_physics_process` never ran"
    );
    assert_eq!(
        int_prop(&ml, body, "rv_type"),
        TYPE_VECTOR2,
        "get_real_velocity() must stay a Vector2 on a CharacterBody2D"
    );
    assert_eq!(
        int_prop(&ml, body, "pv_type"),
        TYPE_VECTOR2,
        "get_platform_velocity() must stay a Vector2 on a CharacterBody2D"
    );
}

/// Before the first physics tick nothing has been published, and Godot still
/// answers a zero vector rather than null so a script may chain `.length()`.
///
/// ⚠️ It has to be the zero of the RIGHT dimension. A single shared
/// `Vector2::ZERO` fallback makes the very first read of a 3D body a `Vector2`
/// again — the same bug, surviving in the one frame most likely to be read by
/// an `_ready`-adjacent script.
#[test]
fn unpublished_velocity_reads_as_zero_in_the_bodys_own_dimension() {
    let script = "\
extends CharacterBody3D
var ran = false
var rv_type = -1
var rv_len = -1.0
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    self.rv_type = typeof(get_real_velocity())
    self.rv_len = get_real_velocity().length()
";
    // Note: no `velocity` is ever assigned, so the solver skips this body
    // entirely and never publishes `_real_velocity`.
    let (mut ml, player) = walker_scene(script);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        prop(&ml, player, "ran"),
        Variant::Bool(true),
        "`_physics_process` must run"
    );
    assert_eq!(
        int_prop(&ml, player, "rv_type"),
        TYPE_VECTOR3,
        "an unpublished get_real_velocity() must still be a Vector3 on a 3D body"
    );
    assert_eq!(
        prop(&ml, player, "rv_len"),
        Variant::Float(0.0),
        "and it must be the ZERO vector, chainable with .length() the way Godot \
         guarantees"
    );
}

/// `get_position_delta()`, `get_last_motion()` and `get_platform_angular_velocity()`
/// (pat-s3rrv.103).
///
/// ⚠️ `position_delta` and `last_motion` are DIFFERENT quantities that happen to
/// coincide here: Godot splits a movement into several motions when sliding
/// occurs and `get_last_motion` reports the final segment, so the two agree
/// exactly when nothing was hit. This walker is unobstructed, which is why the
/// equality below is the correct assertion — and why it is deliberately paired
/// with the wall test underneath, which is the case that can tell them apart.
#[test]
fn position_delta_and_last_motion_are_reported_in_3d() {
    let script = "\
extends CharacterBody3D
var ticks = 0
var delta_v = Vector3(-1, -1, -1)
var last_v = Vector3(-1, -1, -1)
var ang_type = -1
func _physics_process(delta):
    velocity = Vector3(4, 0, 0)
    move_and_slide()
    self.ticks = self.ticks + 1
    self.delta_v = get_position_delta()
    self.last_v = get_last_motion()
    self.ang_type = typeof(get_platform_angular_velocity())
";
    let (mut ml, player) = walker_scene(script);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        int_prop(&ml, player, "ticks") > 0,
        "`_physics_process` never ran"
    );

    // 4 units/s over a 1/60 tick ≈ 0.0667 of travel along +x.
    let expected = 4.0 / 60.0;
    match prop(&ml, player, "delta_v") {
        Variant::Vector3(v) => assert!(
            (v.x - expected).abs() < 0.02 && v.y.abs() < 0.02,
            "get_position_delta() must be the travel over ONE move_and_slide \
             (~{expected} along +x), got {v:?} — a value ~60x larger means it \
             is reporting a per-second velocity instead of a delta"
        ),
        other => panic!("`delta_v` must be a Vector3, got {other:?}"),
    }
    match prop(&ml, player, "last_v") {
        Variant::Vector3(v) => assert!(
            (v.x - expected).abs() < 0.02,
            "with nothing obstructing the walker the slide is a single segment, \
             so get_last_motion() must equal the travel; got {v:?}"
        ),
        other => panic!("`last_v` must be a Vector3, got {other:?}"),
    }
    // 3D-only in Godot — CharacterBody2D has no such method — so it must answer
    // a Vector3 rather than going through the 2D/3D dimension fallback.
    assert_eq!(
        int_prop(&ml, player, "ang_type"),
        TYPE_VECTOR3,
        "get_platform_angular_velocity() must be a Vector3"
    );
}

/// 🔑 The case that separates the two: a body sliding along a wall.
///
/// ⚠️ Godot documents `get_last_motion` as *"useful to retrieve the current
/// direction of the movement"* — the final segment, after the slide has removed
/// the component pointing into the surface. So a character shoved hard into a
/// wall must NOT report a last motion that still points into it, however large
/// the commanded velocity along that axis was. An implementation that reported
/// the raw requested motion, or that measured the outer anti-tunnelling
/// sub-step rather than the slide iteration, fails exactly here.
#[test]
fn last_motion_follows_the_wall_not_the_input() {
    let script = "\
extends CharacterBody3D
var ticks = 0
var last_v = Vector3(-1, -1, -1)
func _physics_process(delta):
    velocity = Vector3(20, 0, 4)
    move_and_slide()
    self.ticks = self.ticks + 1
    self.last_v = get_last_motion()
";
    // A wall standing at x = +2, thin in x and tall/wide in y/z.
    let (mut ml, player) = walker_scene(script);
    {
        let tree = ml.tree_mut();
        let root = tree
            .get_node(player)
            .and_then(|n| n.parent())
            .expect("player has a parent");
        let mut wall = Node::new("Wall", "StaticBody3D");
        wall.set_property("position", Variant::Vector3(Vector3::new(2.0, 0.0, 0.0)));
        let wall_id = tree.add_child(root, wall).unwrap();
        let mut ws = Node::new("WallShape", "CollisionShape3D");
        ws.set_property("shape", Variant::String("BoxShape3D".to_string()));
        ws.set_property("size", Variant::Vector3(Vector3::new(1.0, 40.0, 40.0)));
        tree.add_child(wall_id, ws).unwrap();
    }
    for _ in 0..90 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        int_prop(&ml, player, "ticks") > 0,
        "`_physics_process` never ran"
    );
    let end = node3d::get_position(ml.tree(), player);
    assert!(
        end.x < 2.0,
        "the walker must be stopped by the wall at x = 2 rather than passing \
         through it, ended at {end:?} — without contact this test proves nothing"
    );
    match prop(&ml, player, "last_v") {
        Variant::Vector3(v) => {
            // The commanded x velocity is 5x the z one, so a last motion that
            // still leads with x would mean the slide projection never applied.
            assert!(
                v.x.abs() <= v.z.abs() + 0.01,
                "pressed against a wall on +x, get_last_motion() must follow the \
                 wall (mostly +z), not the input that points into it; got {v:?}"
            );
            assert!(
                v.z > 0.0,
                "and it must still carry the along-wall component the body is \
                 actually travelling, got {v:?}"
            );
        }
        other => panic!("`last_v` must be a Vector3, got {other:?}"),
    }
}
