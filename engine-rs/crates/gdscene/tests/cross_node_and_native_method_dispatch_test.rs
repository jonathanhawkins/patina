//! pat-0q2kx.16 acceptance: GDScript reaches both halves of Godot's
//! `Object::callp` — the target's script instance, then the engine's native
//! method for its class.
//!
//! Patina dispatched node-object methods from a hand-coded list whose fallback
//! threw `UndefinedFunction`, so a `CharacterBody2D` script calling
//! `move_and_slide()` / `is_on_floor()` died mid-frame, and `add_to_group` /
//! `find_child` / `has_signal` on another node were unreachable. Separately the
//! physics server auto-moved *every* kinematic body with a velocity, so a
//! scripted body drifted whether or not it asked to.
//!
//! These drive real `MainLoop::step` frames and assert on **tree state**, not on
//! the interpreter's return values, so a regression to "method resolves but does
//! nothing" fails.

use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn script_var(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn node_vec2(ml: &MainLoop, node: gdscene::node::NodeId, prop: &str) -> gdcore::math::Vector2 {
    match ml.tree().get_node(node).unwrap().get_property(prop) {
        Variant::Vector2(v) => v,
        other => panic!("{prop} should be a Vector2, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Cross-node script dispatch
// ---------------------------------------------------------------------------

/// `$Enemy.take_damage(5)` runs Enemy's own script method and mutates its state.
#[test]
fn cross_node_script_method_call_mutates_the_target() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let enemy = tree.add_child(main, Node::new("Enemy", "Node2D")).unwrap();

    let enemy_src = "\
extends Node2D
var health = 100
func take_damage(amount):
    self.health = self.health - amount
";
    tree.attach_script(
        enemy,
        Box::new(GDScriptNodeInstance::from_source(enemy_src, enemy).unwrap()),
    );

    let main_src = "\
extends Node2D
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        get_node(\"Enemy\").take_damage(30)
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(main_src, main).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, enemy, "health"),
        Some(Variant::Int(70)),
        "Enemy's own script method must run and mutate Enemy's state"
    );
}

// ---------------------------------------------------------------------------
// Native node methods
// ---------------------------------------------------------------------------

/// Groups, `find_child` and `has_signal` round-trip through the native table.
#[test]
fn native_node_methods_round_trip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let enemy = tree.add_child(main, Node::new("Enemy", "Node2D")).unwrap();
    tree.add_child(enemy, Node::new("Hitbox", "Area2D"))
        .unwrap();

    let src = "\
extends Node2D
var in_group_after = false
var found_child = false
var enemy_has_ready = false
var enemy_has_bogus = true
var group_count = 0
var emit_result = -1
func _process(delta):
    var e = get_node(\"Enemy\")
    e.add_to_group(\"enemies\")
    self.in_group_after = e.is_in_group(\"enemies\")
    self.found_child = e.find_child(\"Hitbox\") != null
    self.enemy_has_ready = e.has_method(\"has_method\")
    self.enemy_has_bogus = e.has_method(\"definitely_not_a_method_xyz\")
    self.group_count = e.get_nodes_in_group(\"enemies\").size()
    self.emit_result = e.emit_signal(\"tree_entered\")
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, main, "in_group_after"),
        Some(Variant::Bool(true)),
        "add_to_group then is_in_group must round-trip on another node"
    );
    // `emit_signal` on ANOTHER node: the self form was a script builtin, but the
    // cross-node form reached neither the ObjectId path nor the native table, so
    // `$Enemy.emit_signal(..)` failed while `$Enemy.has_signal(..)` answered.
    // Godot returns an Error code, and OK is 0.
    assert_eq!(
        script_var(&ml, main, "emit_result"),
        Some(Variant::Int(0)),
        "emit_signal must dispatch on another node and report OK"
    );
    assert_eq!(
        script_var(&ml, main, "found_child"),
        Some(Variant::Bool(true)),
        "find_child must resolve a descendant"
    );
    assert_eq!(
        script_var(&ml, main, "group_count"),
        Some(Variant::Int(1)),
        "get_nodes_in_group must see the node added this frame"
    );
    assert_eq!(
        script_var(&ml, main, "enemy_has_bogus"),
        Some(Variant::Bool(false)),
        "has_method must stay false for a method that does not exist"
    );

    // The group is real tree state, not just a script-visible value.
    assert!(
        ml.tree()
            .get_node(enemy)
            .unwrap()
            .groups()
            .contains("enemies"),
        "add_to_group must mutate the SceneTree, not only the script's view"
    );
}

// ---------------------------------------------------------------------------
// CharacterBody2D motion — the acceptance test
// ---------------------------------------------------------------------------

/// Builds `Main -> {Mover (scripted, calls move_and_slide), Idler (scripted,
/// never calls it)}` over a StaticBody2D floor.
fn character_scene(
    mover_src: &str,
    idler_src: &str,
) -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    // Collision geometry comes from a child CollisionShape2D, as the physics
    // server's `find_collision_shape` expects (`size` = rect, `radius` = circle).
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 200.0)),
    );
    let floor_id = tree.add_child(main, floor).unwrap();
    let mut floor_shape = Node::new("CollisionShape", "CollisionShape2D");
    floor_shape.set_property(
        "size",
        Variant::Vector2(gdcore::math::Vector2::new(1000.0, 20.0)),
    );
    tree.add_child(floor_id, floor_shape).unwrap();

    // `velocity` is seeded to (0,0) — Godot's CharacterBody2D default. This is
    // load-bearing for the test, not decoration: the interpreter only routes
    // `self.velocity = ...` to the *node* when that property already exists and
    // is non-Nil (interpreter.rs `is_node_prop`). Without the seed the script's
    // assignment would land in a local and physics would never see it.
    let mut mover = Node::new("Mover", "CharacterBody2D");
    mover.set_property("position", Variant::Vector2(gdcore::math::Vector2::ZERO));
    mover.set_property("velocity", Variant::Vector2(gdcore::math::Vector2::ZERO));
    let mover_id = tree.add_child(main, mover).unwrap();
    let mut mover_shape = Node::new("CollisionShape", "CollisionShape2D");
    mover_shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(mover_id, mover_shape).unwrap();

    // Spawned clear of the Mover, not on top of it. Both bodies are r=10, so a
    // shared origin would make the two overlap and the first thing link 3 would
    // exercise is depenetration between two characters rather than the dispatch
    // this test is about. x=200 is still well inside the 1000-wide floor, and
    // the y assertion below is unchanged.
    let mut idler = Node::new("Idler", "CharacterBody2D");
    idler.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(200.0, 0.0)),
    );
    idler.set_property("velocity", Variant::Vector2(gdcore::math::Vector2::ZERO));
    let idler_id = tree.add_child(main, idler).unwrap();
    let mut idler_shape = Node::new("CollisionShape", "CollisionShape2D");
    idler_shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(idler_id, idler_shape).unwrap();

    tree.attach_script(
        mover_id,
        Box::new(GDScriptNodeInstance::from_source(mover_src, mover_id).unwrap()),
    );
    tree.attach_script(
        idler_id,
        Box::new(GDScriptNodeInstance::from_source(idler_src, idler_id).unwrap()),
    );

    // `MainLoop::new` does NOT put bodies into the physics world, and `step`
    // never does it either — `register_bodies` is the only writer of
    // `node_to_body` (physics_server.rs), and `process_character_movement`
    // iterates exactly that map. Without this call the character pass sees an
    // empty map, so *every* body silently stays at its spawn point no matter
    // how the dispatch behaves — which is precisely the `(0,0) -> (0,0)` this
    // test kept reporting. The host is what registers: `patina-runner`
    // does it at main.rs:369 after loading the scene.
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, mover_id, idler_id)
}

/// **The acceptance test.** A script that assigns `velocity` and calls
/// `move_and_slide()` moves; an otherwise identical body whose script never
/// calls it stays put. `is_on_floor()` is reachable from GDScript.
#[test]
fn cross_node_script_and_native_method_dispatch() {
    // Both scripts set the same velocity. Only one calls move_and_slide().
    let mover = "\
extends CharacterBody2D
var landed = false
func _physics_process(delta):
    self.velocity = Vector2(0, 400)
    move_and_slide()
    if is_on_floor():
        self.landed = true
";
    let idler = "\
extends CharacterBody2D
func _physics_process(delta):
    self.velocity = Vector2(0, 400)
";
    let (mut ml, mover_id, idler_id) = character_scene(mover, idler);

    let start = node_vec2(&ml, mover_id, "position");

    // Step once and check each link of the chain separately, so a failure names
    // the broken one instead of only reporting "it did not move".
    ml.step(1.0 / 60.0);

    // Link 1 — the script's `self.velocity = ...` reached the *node*. (The
    // physics phase runs before scripts, so nothing has overwritten it yet.)
    let vel = node_vec2(&ml, mover_id, "velocity");
    assert!(
        vel.y > 0.0,
        "link 1: `self.velocity = Vector2(0, 400)` must reach the node property, got {vel:?}"
    );

    // Link 2 — the bare `move_and_slide()` call reached the native dispatch and
    // recorded its one-shot request (the physics phase has not consumed it yet).
    assert_eq!(
        ml.tree()
            .get_node(mover_id)
            .unwrap()
            .get_property("_move_and_slide_requested"),
        Variant::Bool(true),
        "link 2: a bare move_and_slide() must reach SceneAccess::call_node_method \
         and set the request flag"
    );

    // Link 3 — the physics phase honors the request and actually moves the body.
    for _ in 0..29 {
        ml.step(1.0 / 60.0);
    }

    // The scripted caller moved ...
    let moved = node_vec2(&ml, mover_id, "position");
    assert!(
        moved.y > start.y + 1.0,
        "link 3: the physics phase must honor the move_and_slide request, \
         start {start:?} -> {moved:?}"
    );

    // ... and the one that never called it did not. This is the assertion the
    // old unconditional auto-move failed.
    let idle = node_vec2(&ml, idler_id, "position");
    assert!(
        (idle.y - start.y).abs() < 1e-3,
        "a body whose script never calls move_and_slide() must not move, got {idle:?}"
    );

    // `is_on_floor()` resolves from GDScript rather than throwing
    // UndefinedFunction — and reports true once the body lands.
    assert_eq!(
        script_var(&ml, mover_id, "landed"),
        Some(Variant::Bool(true)),
        "is_on_floor() must be callable from a script and be true after landing"
    );
}

/// A script calling `move_and_slide()` on *another* body moves that body —
/// the receiver form of the same native dispatch.
#[test]
fn move_and_slide_is_callable_on_another_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut body = Node::new("Body", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(gdcore::math::Vector2::ZERO));
    body.set_property(
        "velocity",
        Variant::Vector2(gdcore::math::Vector2::new(120.0, 0.0)),
    );
    let body_id = tree.add_child(main, body).unwrap();
    let mut body_shape = Node::new("CollisionShape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(body_id, body_shape).unwrap();
    // The body itself is scripted (so the gate applies) but its script never
    // calls move_and_slide — the *driver* does, through a node reference.
    tree.attach_script(
        body_id,
        Box::new(
            GDScriptNodeInstance::from_source("extends CharacterBody2D\nvar idle = 0\n", body_id)
                .unwrap(),
        ),
    );

    let driver_src = "\
extends Node2D
func _physics_process(delta):
    get_node(\"Body\").move_and_slide()
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(driver_src, main).unwrap()),
    );

    // See `character_scene`: the character pass only walks registered bodies.
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    for _ in 0..20 {
        ml.step(1.0 / 60.0);
    }

    let pos = node_vec2(&ml, body_id, "position");
    assert!(
        pos.x > 1.0,
        "move_and_slide() called on another node must move that node, got {pos:?}"
    );
}

/// A CharacterBody2D with **no** script keeps the legacy auto-move, so existing
/// scenes and tests that drive `velocity` directly are unaffected by the gate.
#[test]
fn unscripted_character_body_still_auto_moves() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut body = Node::new("Body", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(gdcore::math::Vector2::ZERO));
    body.set_property(
        "velocity",
        Variant::Vector2(gdcore::math::Vector2::new(120.0, 0.0)),
    );
    let body_id = tree.add_child(main, body).unwrap();
    let mut body_shape = Node::new("CollisionShape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(body_id, body_shape).unwrap();

    // See `character_scene`: the character pass only walks registered bodies.
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    for _ in 0..20 {
        ml.step(1.0 / 60.0);
    }

    let pos = node_vec2(&ml, body_id, "position");
    assert!(
        pos.x > 1.0,
        "an unscripted body must keep auto-moving (the gate is scripted-only), got {pos:?}"
    );
}
