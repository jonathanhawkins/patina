//! Two GDScript features the official 2D Platformer demo
//! (godot-demo-projects/2d/platformer) depends on, both found broken while
//! playing it in the browser:
//!
//! 1. An annotation before a `signal` (`@warning_ignore("unused_signal")`) —
//!    Godot accepts an annotation before ANY statement. It was a parse error,
//!    so `player.gd` never loaded and the player had no script at all.
//! 2. `PhysicsBody2D.get_gravity()` (Godot 4.3+) — the enemies' fall. It was
//!    "method not found" on every physics frame.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

#[test]
fn an_annotation_may_precede_a_signal_a_const_and_a_statement() {
    let src = "extends Node\n\
@warning_ignore(\"unused_signal\")\n\
signal coin_collected()\n\
@warning_ignore(\"unused_private_class_variable\")\n\
const SPEED = 3\n\
var hits := 0\n\
var declared := false\n\
func _ready():\n\
\t@warning_ignore(\"integer_division\")\n\
\thits = SPEED / 2\n\
\tdeclared = has_signal(\"coin_collected\")\n";
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("N", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(src, id).expect("annotated statements parse");
    tree.attach_script(id, Box::new(script));
    gdscene::LifecycleManager::enter_tree(&mut tree, id);
    assert_eq!(
        tree.get_script(id).and_then(|s| s.get_property("hits")),
        Some(Variant::Int(1)),
        "the annotated statement inside _ready ran"
    );
    assert_eq!(
        tree.get_script(id).and_then(|s| s.get_property("declared")),
        Some(Variant::Bool(true)),
        "the annotated signal is declared"
    );
}

#[test]
fn get_gravity_drives_a_character_body_down_and_leaves_area_gravity_alone() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut body = Node::new("Enemy", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let body_id = tree.add_child(root, body).unwrap();
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(8.0));
    tree.add_child(body_id, shape).unwrap();
    let area_id = tree.add_child(root, Node::new("Zone", "Area2D")).unwrap();
    let area_script = GDScriptNodeInstance::from_source(
        "extends Area2D\nvar ag = null\nfunc _ready():\n\tag = get_gravity()\n",
        area_id,
    )
    .unwrap();
    tree.attach_script(area_id, Box::new(area_script));
    gdscene::LifecycleManager::enter_tree(&mut tree, area_id);

    let src = "extends CharacterBody2D\n\
var g := Vector2.ZERO\n\
func _physics_process(delta):\n\
\tg = get_gravity()\n\
\tvelocity += get_gravity() * delta\n\
\tmove_and_slide()\n";
    let script = GDScriptNodeInstance::from_source(src, body_id).unwrap();
    tree.attach_script(body_id, Box::new(script));
    gdscene::LifecycleManager::enter_tree(&mut tree, body_id);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    let out = ml.tree_mut().drain_script_output();
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(
        ml.tree().get_script(body_id).and_then(|s| s.get_property("g")),
        Some(Variant::Vector2(Vector2::new(0.0, 980.0))),
        "Godot's default 2D gravity, (0, 980)"
    );
    let pos = match ml.tree().get_node(body_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        other => panic!("{other:?}"),
    };
    assert!(pos.y > 100.0, "the body falls under get_gravity(): {pos:?}");

    // Area2D.get_gravity() is its float `gravity` property, not a vector.
    let g = ml.tree().get_script(area_id).and_then(|s| s.get_property("ag"));
    assert!(
        matches!(g, Some(Variant::Float(_)) | Some(Variant::Int(_))),
        "Area2D keeps its float gravity getter: {g:?}"
    );
}

/// 3. `@onready` initializers run in DECLARATION ORDER, each seeing the ones
///    before it — `@onready var gun = sprite.get_node(^"Gun")` after
///    `@onready var sprite := $Sprite2D` (the demo's player). The later one
///    read the earlier one as its pre-ready Nil, so the first shot raised
///    "cannot call method on Nil".
#[test]
fn an_onready_initializer_sees_the_onready_vars_declared_before_it() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player = tree.add_child(root, Node::new("Player", "Node2D")).unwrap();
    let sprite = tree.add_child(player, Node::new("Sprite2D", "Sprite2D")).unwrap();
    let gun = tree.add_child(sprite, Node::new("Gun", "Marker2D")).unwrap();
    let src = "extends Node2D\n\
@onready var sprite := $Sprite2D as Sprite2D\n\
@onready var gun = sprite.get_node(^\"Gun\")\n\
@onready var gun_name = gun.name\n";
    let script = GDScriptNodeInstance::from_source(src, player).unwrap();
    tree.attach_script(player, Box::new(script));
    gdscene::LifecycleManager::enter_tree(&mut tree, player);
    let get = |k: &str| tree.get_script(player).and_then(|s| s.get_property(k));
    assert_eq!(get("gun"), Some(Variant::ObjectId(gun.object_id())));
    assert_eq!(get("gun_name"), Some(Variant::String("Gun".into())), "a chain of three");
}

/// 4. An action named by a StringName literal (`&"toggle_fullscreen"`, as the
///    demo's `game.gd` writes it) is an action name. `InputEvent` accepted only
///    a String and raised a type error on every unhandled input event.
#[test]
fn input_event_is_action_pressed_accepts_a_string_name() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("N", "Node")).unwrap();
    let src = "extends Node\nvar r = null\nfunc _ready():\n\
\tvar ev := InputEventKey.new()\n\
\tr = ev.is_action_pressed(&\"toggle_fullscreen\")\n";
    let script = GDScriptNodeInstance::from_source(src, id).unwrap();
    tree.attach_script(id, Box::new(script));
    gdscene::LifecycleManager::enter_tree(&mut tree, id);
    let out = tree.drain_script_output();
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(
        tree.get_script(id).and_then(|s| s.get_property("r")),
        Some(Variant::Bool(false)),
        "an unmapped action on a blank event is simply not pressed"
    );
}

/// 5. `is` / `as` with a SCRIPT `class_name`: `(body as Player).coin_collected`
///    and `instantiate() as Bullet` in the demo. Only the native class chain
///    was consulted, so every such cast produced null.
#[test]
fn is_and_as_understand_script_class_names() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let hero = tree.add_child(root, Node::new("Hero", "CharacterBody2D")).unwrap();
    let checker = tree.add_child(root, Node::new("Checker", "Node")).unwrap();
    let hero_src = "class_name Hero\nextends CharacterBody2D\nsignal coin_collected\n";
    tree.attach_script(hero, Box::new(GDScriptNodeInstance::from_source(hero_src, hero).unwrap()));
    let src = "class_name Checker\nextends Node\n\
var is_hero = null\nvar cast_ok = null\nvar native_still = null\nvar self_is = null\nvar wrong = null\n\
func _ready():\n\
\tvar h = get_node(\"../Hero\")\n\
\tis_hero = h is Hero\n\
\tcast_ok = (h as Hero) != null\n\
\tnative_still = h is CharacterBody2D\n\
\tself_is = self is Checker\n\
\twrong = h is Checker\n";
    tree.attach_script(checker, Box::new(GDScriptNodeInstance::from_source(src, checker).unwrap()));
    gdscene::LifecycleManager::enter_tree(&mut tree, checker);
    let get = |k: &str| tree.get_script(checker).and_then(|s| s.get_property(k));
    assert_eq!(get("is_hero"), Some(Variant::Bool(true)), "h is Hero");
    assert_eq!(get("cast_ok"), Some(Variant::Bool(true)), "h as Hero is not null");
    assert_eq!(get("native_still"), Some(Variant::Bool(true)), "native classes still match");
    assert_eq!(get("self_is"), Some(Variant::Bool(true)), "a running script knows its own class");
    assert_eq!(get("wrong"), Some(Variant::Bool(false)), "a different class_name does not match");
}
