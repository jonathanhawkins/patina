//! pat-s3rrv.25 — `CollisionObject2D/3D`'s picking virtuals are dispatched under
//! the names and argument lists Godot documents, so a handler written from the
//! docs actually runs.
//!
//! Godot's signatures (`extension_api.json`, `doc/classes/CollisionObject2D.xml`,
//! `doc/classes/CollisionObject3D.xml`):
//!
//! ```text
//! 2D  _input_event(viewport, event, shape_idx)
//!     _mouse_enter()  _mouse_exit()
//!     _mouse_shape_enter(shape_idx)  _mouse_shape_exit(shape_idx)
//! 3D  _input_event(camera, event, event_position, normal, shape_idx)
//!     _mouse_enter()  _mouse_exit()
//! ```
//!
//! Note the asymmetry: 3D has **no** shape-level mouse virtuals. That is Godot's
//! own API shape, verified against both doc XMLs, not an omission here.
//!
//! Two separate failures are covered. The `_input_event` half was a wrong
//! **argument list**; the `_mouse_*` half was a wrong **name** — Patina emitted
//! only the `mouse_entered`/`mouse_exited` signals, so all four documented mouse
//! virtuals were dead tree-wide. That one was easy to miss because gdscene does
//! dispatch `_mouse_entered`/`_mouse_exited`, but only on Controls in the GUI
//! stage, and those are signal names with an underscore glued on rather than
//! these virtuals. The names the documentation gives were exactly the dead ones.
//!
//! Patina sent **one** argument in 2D and **four** in 3D. That is not a partial
//! delivery that degrades gracefully — the interpreter rejects an arity mismatch
//! outright (`args.len() > func.params.len()` raises a `TypeError`) and
//! `call_script_method` swallows the raise into a `tracing::debug!`. So a script
//! written to the documented signature — the one every Godot doc and tutorial
//! shows — never ran, and nothing anywhere said why.
//!
//! **Why no existing test caught it.** `pick2d_signals.rs` and
//! `physics3d_picking.rs` assert on *signal connections* to `_on_input_event`,
//! and `viewport_input_propagation_gui_stage.rs` asserts on a Rust
//! `RecordingScript` mock that records the call name **regardless of arity**. No
//! test had ever compiled a GDScript with Godot's signature, so the whole
//! failure lived in the one gap the suite did not cover. These tests close it by
//! running real GDScript and reading back what the handler stored.
//!
//! The reads go through the script's own variables rather than a Rust mock on
//! purpose: `call_script_method` discards a script's variable writes when the
//! call raises, so a rejected arity leaves the counter at its initialiser. The
//! counter being non-zero is therefore proof the call was *accepted*, not merely
//! attempted.

use gdcore::math::{Vector2, Vector3};
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
// `get_script()` yields `&dyn ScriptInstance`; the trait must be in scope to
// read the handler's variables back off it.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Godot's documented 2D override, written the way the docs show it.
///
/// One line per statement with explicit `\n`/`\t`: a Rust line-continuation
/// would strip the leading whitespace and hand the parser an unindented body.
const PICKABLE_2D: &str = "extends Area2D\n\
                           var calls = 0\n\
                           var got_shape_idx = -1\n\
                           var got_viewport = null\n\
                           func _input_event(viewport, event, shape_idx):\n\
                           \tcalls = calls + 1\n\
                           \tgot_shape_idx = shape_idx\n\
                           \tgot_viewport = viewport\n";

/// The argument list Patina used to send. Not a Godot override — it exists to
/// prove the dispatch really sends three arguments now, rather than one that
/// happens to satisfy a lenient check.
const ONE_ARG_2D: &str = "extends Area2D\n\
                          var calls = 0\n\
                          func _input_event(event):\n\
                          \tcalls = calls + 1\n";

/// Godot's documented 3D override. `event_position` rather than `position`: the
/// latter is a real Node3D property and shadowing it inside the handler would
/// muddy what the test is measuring.
const PICKABLE_3D: &str = "extends StaticBody3D\n\
                           var calls = 0\n\
                           var got_shape_idx = -1\n\
                           func _input_event(camera, event, event_position, normal, shape_idx):\n\
                           \tcalls = calls + 1\n\
                           \tgot_shape_idx = shape_idx\n";

/// Matches `physics3d_picking.rs`, whose ray through the screen centre is known
/// to hit a 2x2x2 box at the origin from a camera at z=+10.
/// Records the `shape_idx` a click reports, for the multi-collider case.
const SHAPE_IDX_2D: &str = "extends Area2D\n\
                            var got_shape_idx = -1\n\
                            var calls = 0\n\
                            func _input_event(viewport, event, shape_idx):\n\
                            \tcalls = calls + 1\n\
                            \tgot_shape_idx = shape_idx\n";

/// Reads the shape-owner API back through real GDScript.
const SHAPE_OWNERS_2D: &str = "extends Area2D\n\
                               var owner_count = -1\n\
                               var second_owner_x = -1.0\n\
                               var shapes_on_first = -1\n\
                               func _input_event(viewport, event, shape_idx):\n\
                               \tvar owners = get_shape_owners()\n\
                               \towner_count = owners.size()\n\
                               \tshapes_on_first = shape_owner_get_shape_count(0)\n\
                               \tsecond_owner_x = shape_owner_get_transform(1).origin.x\n";

const VIEW_W: u32 = 200;
const VIEW_H: u32 = 200;

/// Reads one of the handler's script variables back.
///
/// Script variables are not node properties — they live on the `ScriptInstance`
/// and are reachable only through `list_properties`.
fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .unwrap_or_else(|| panic!("node has no script attached"))
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no variable named `{name}`"))
        .default_value
}

fn script_int(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match script_var(ml, node, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` should be an Int, got {other:?}"),
    }
}

fn click(position: Vector2) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position,
    }
}

/// **The 2D acceptance.** A handler with Godot's three-parameter signature runs,
/// and the two arguments Patina used to withhold carry real values.
#[test]
fn collision_object_2d_input_event_uses_godot_signature() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // The viewport a node under the root resolves to is the root `Window`
    // itself — Godot's `Node.get_viewport()`. Captured before the tree moves.
    let root_object_id = root.object_id();

    let area = tree.add_child(root, Node::new("Coin", "Area2D")).unwrap();
    tree.get_node_mut(area)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    tree.add_child(area, shape).unwrap();
    tree.attach_script(
        area,
        Box::new(GDScriptNodeInstance::from_source(PICKABLE_2D, area).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    // Picking runs in the input stage, which precedes physics, so the collider
    // has to be in the world before the first step or the click hits nothing
    // and this test would pass vacuously in the negative direction.
    ml.register_physics_bodies();

    ml.push_event(click(Vector2::new(50.0, 50.0)));
    ml.step(1.0 / 60.0);

    // Link 0 — the handler ran at all. Before this bead the arity was rejected
    // and the raise was swallowed, so this counter stayed at its initialiser
    // with nothing logged anywhere to explain it.
    assert_eq!(
        script_int(&ml, area, "calls"),
        1,
        "a `func _input_event(viewport, event, shape_idx)` written straight from \
         Godot's docs must be called exactly once for a click inside the collider",
    );

    // Link 1 — `shape_idx` is a real value. Every Patina body holds exactly one
    // shape (`PhysicsBody2D.shape`), so 0 is the only correct index; when
    // multi-shape bodies land this assertion is what will need revisiting.
    assert_eq!(
        script_int(&ml, area, "got_shape_idx"),
        0,
        "`shape_idx` must arrive as 0 — the single shape every Patina body has",
    );

    // Link 2 — `viewport` is the real viewport, not a null filler. For a node
    // under the root that is the root `Window`, which is what Godot's
    // `get_viewport()` returns.
    assert_eq!(
        script_var(&ml, area, "got_viewport"),
        Variant::ObjectId(root_object_id),
        "`viewport` must be the collider's own viewport (the root Window here), \
         which is what `Node.get_viewport()` resolves to",
    );
}

/// The dispatch really sends three arguments — a one-parameter handler, which is
/// what Patina used to accommodate, is now correctly rejected.
///
/// This is the negative half of the acceptance: without it, a dispatch that
/// still sent one argument would satisfy nothing above except by coincidence.
#[test]
fn collision_object_2d_input_event_rejects_the_old_one_arg_form() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let area = tree.add_child(root, Node::new("Coin", "Area2D")).unwrap();
    tree.get_node_mut(area)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    tree.add_child(area, shape).unwrap();
    tree.attach_script(
        area,
        Box::new(GDScriptNodeInstance::from_source(ONE_ARG_2D, area).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.push_event(click(Vector2::new(50.0, 50.0)));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_int(&ml, area, "calls"),
        0,
        "a one-parameter `_input_event` is not Godot's override and must not be \
         called with three arguments — if this fires, the dispatch is still \
         sending the old single-argument list",
    );
}

/// **The 3D acceptance.** The same fix on the 3D twin, which was sending four of
/// its five documented arguments.
#[test]
fn collision_object_3d_input_event_uses_godot_signature() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    // A default Camera3D looks down its own -Z, so placing it at +Z aims it at
    // the origin with no rotation to get wrong.
    let camera = tree
        .add_child(root, Node::new("Camera", "Camera3D"))
        .unwrap();
    node3d::set_position(&mut tree, camera, Vector3::new(0.0, 0.0, 10.0));
    tree.get_node_mut(camera)
        .unwrap()
        .set_property("current", Variant::Bool(true));

    let body = tree
        .add_child(root, Node::new("Box", "StaticBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, body, Vector3::ZERO);
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    shape.set_property("size", Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)));
    tree.add_child(body, shape).unwrap();
    tree.attach_script(
        body,
        Box::new(GDScriptNodeInstance::from_source(PICKABLE_3D, body).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.set_viewport_size(VIEW_W, VIEW_H);
    // One warm-up frame: the 3D server registers bodies lazily inside
    // `sync_to_physics`, and picking runs before physics, so on a fresh scene
    // the world is still empty on frame 1 and the ray would hit nothing.
    ml.step(1.0 / 60.0);

    // Screen centre — the ray through it hits the 2x2x2 box dead on.
    let centre = Vector2::new(VIEW_W as f32 / 2.0, VIEW_H as f32 / 2.0);
    ml.push_event(click(centre));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_int(&ml, body, "calls"),
        1,
        "a `func _input_event(camera, event, event_position, normal, shape_idx)` \
         written from Godot's docs must be called exactly once for a click on the box",
    );
    assert_eq!(
        script_int(&ml, body, "got_shape_idx"),
        0,
        "`shape_idx` must arrive as 0 — the single shape every Patina 3D body has",
    );
}

/// Godot's four documented 2D mouse virtuals, written the way the docs show them.
const MOUSE_2D: &str = "extends Area2D\n\
                        var enters = 0\n\
                        var exits = 0\n\
                        var shape_enters = 0\n\
                        var shape_exits = 0\n\
                        var last_shape_idx = -1\n\
                        func _mouse_enter():\n\
                        \tenters = enters + 1\n\
                        func _mouse_exit():\n\
                        \texits = exits + 1\n\
                        func _mouse_shape_enter(shape_idx):\n\
                        \tshape_enters = shape_enters + 1\n\
                        \tlast_shape_idx = shape_idx\n\
                        func _mouse_shape_exit(shape_idx):\n\
                        \tshape_exits = shape_exits + 1\n";

/// The 3D pair, plus a shape-level handler that must NEVER be called — Godot
/// gives `CollisionObject3D` only two mouse virtuals.
const MOUSE_3D: &str = "extends StaticBody3D\n\
                        var enters = 0\n\
                        var exits = 0\n\
                        var shape_enters = 0\n\
                        func _mouse_enter():\n\
                        \tenters = enters + 1\n\
                        func _mouse_exit():\n\
                        \texits = exits + 1\n\
                        func _mouse_shape_enter(shape_idx):\n\
                        \tshape_enters = shape_enters + 1\n";

fn motion(position: Vector2) -> InputEvent {
    InputEvent::MouseMotion {
        position,
        relative: Vector2::new(0.0, 0.0),
    }
}

/// **The 2D mouse acceptance.** All four documented virtuals fire on the hover
/// transitions, with `shape_idx` carrying a real value.
#[test]
fn collision_object_2d_mouse_virtuals_use_godot_names() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let area = tree.add_child(root, Node::new("Coin", "Area2D")).unwrap();
    tree.get_node_mut(area)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    tree.add_child(area, shape).unwrap();
    tree.attach_script(
        area,
        Box::new(GDScriptNodeInstance::from_source(MOUSE_2D, area).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // -- pointer moves onto the collider ------------------------------------
    ml.push_event(motion(Vector2::new(50.0, 50.0)));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_int(&ml, area, "enters"),
        1,
        "`func _mouse_enter()` is the name CollisionObject2D.xml documents and \
         must fire once when the pointer enters the collider — before this bead \
         Patina emitted only the `mouse_entered` SIGNAL and this virtual was dead",
    );
    assert_eq!(
        script_int(&ml, area, "shape_enters"),
        1,
        "`_mouse_shape_enter(shape_idx)` must fire alongside `_mouse_enter`",
    );
    assert_eq!(
        script_int(&ml, area, "last_shape_idx"),
        0,
        "`shape_idx` must arrive as 0 — the single shape every Patina body has",
    );
    // Entering must not also report an exit; the two are separate transitions.
    assert_eq!(
        script_int(&ml, area, "exits"),
        0,
        "entering a collider must not fire `_mouse_exit`",
    );

    // -- pointer moves off the collider --------------------------------------
    ml.push_event(motion(Vector2::new(500.0, 500.0)));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_int(&ml, area, "exits"),
        1,
        "`func _mouse_exit()` must fire once when the pointer leaves the collider",
    );
    assert_eq!(
        script_int(&ml, area, "shape_exits"),
        1,
        "`_mouse_shape_exit(shape_idx)` must fire alongside `_mouse_exit`",
    );
    // The enter must not repeat: hover is a transition, not a per-frame state.
    assert_eq!(
        script_int(&ml, area, "enters"),
        1,
        "`_mouse_enter` must fire on the transition only, not once per frame \
         while the pointer rests inside",
    );
}

/// **The 3D mouse acceptance**, including the asymmetry: `CollisionObject3D` has
/// only `_mouse_enter`/`_mouse_exit`, so a shape-level handler must stay silent.
#[test]
fn collision_object_3d_mouse_virtuals_use_godot_names() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let camera = tree
        .add_child(root, Node::new("Camera", "Camera3D"))
        .unwrap();
    node3d::set_position(&mut tree, camera, Vector3::new(0.0, 0.0, 10.0));
    tree.get_node_mut(camera)
        .unwrap()
        .set_property("current", Variant::Bool(true));

    let body = tree
        .add_child(root, Node::new("Box", "StaticBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, body, Vector3::ZERO);
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    shape.set_property("size", Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)));
    tree.add_child(body, shape).unwrap();
    tree.attach_script(
        body,
        Box::new(GDScriptNodeInstance::from_source(MOUSE_3D, body).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.set_viewport_size(VIEW_W, VIEW_H);
    // Warm-up frame: the 3D server registers bodies lazily inside
    // `sync_to_physics`, and picking runs before physics.
    ml.step(1.0 / 60.0);

    let centre = Vector2::new(VIEW_W as f32 / 2.0, VIEW_H as f32 / 2.0);
    ml.push_event(motion(centre));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_int(&ml, body, "enters"),
        1,
        "`func _mouse_enter()` must fire once when the pointer enters the body",
    );

    // The asymmetry, asserted rather than assumed: Godot documents no
    // shape-level mouse virtual on CollisionObject3D, so dispatching one here
    // would be inventing API that a real Godot project would never receive.
    assert_eq!(
        script_int(&ml, body, "shape_enters"),
        0,
        "CollisionObject3D has no `_mouse_shape_enter` in Godot — dispatching \
         one would invent API the engine is not supposed to have",
    );

    // A corner ray misses the 2x2x2 box at the origin entirely.
    ml.push_event(motion(Vector2::new(4.0, 4.0)));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_int(&ml, body, "exits"),
        1,
        "`func _mouse_exit()` must fire once when the pointer leaves the body",
    );
}

/// `shape_idx` reports **which** collider was clicked, not a constant `0`
/// (pat-s3rrv.25).
///
/// Picking sent `0` for every hit. That was correct only while a
/// `CollisionObject2D` could own one shape; once bodies and areas gained a
/// collider list it became silently wrong for every click on a second collider —
/// a fighting-game hitbox or a multi-part button could not tell which part was
/// hit.
///
/// The two colliders are far apart so the click lands unambiguously in the
/// second: a reported `0` can only mean the index is not being resolved.
#[test]
fn input_event_reports_which_collider_was_clicked() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let area = tree.add_child(root, Node::new("Coin", "Area2D")).unwrap();
    tree.get_node_mut(area)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));

    let mut first = Node::new("ShapeA", "CollisionShape2D");
    first.set_property("size", Variant::Vector2(Vector2::new(20.0, 20.0)));
    tree.add_child(area, first).unwrap();

    let mut second = Node::new("ShapeB", "CollisionShape2D");
    second.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    second.set_property("position", Variant::Vector2(Vector2::new(300.0, 0.0)));
    tree.add_child(area, second).unwrap();

    tree.attach_script(
        area,
        Box::new(GDScriptNodeInstance::from_source(SHAPE_IDX_2D, area).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.push_event(click(Vector2::new(300.0, 0.0)));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_int(&ml, area, "calls"),
        1,
        "the click must reach `_input_event` on the second collider at all",
    );
    assert_eq!(
        script_int(&ml, area, "got_shape_idx"),
        1,
        "`shape_idx` must be 1 for a click on the SECOND collider — a 0 means \
         picking is still sending the constant it used when a body could only \
         own one shape",
    );
}

/// The read-only shape-owner API answers from the scene (pat-s3rrv.25).
///
/// Godot's `CollisionObject2D` exposes its colliders as shape owners. Patina
/// models one collider child as one owner holding one shape, so the owner id is
/// exactly the `shape_idx` picking reports — the two have to agree or a script
/// that picks a shape and then queries it gets a different collider back.
///
/// The second collider is authored at x=300 so its transform is unmistakable.
#[test]
fn shape_owner_api_reports_the_collider_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let area = tree.add_child(root, Node::new("Coin", "Area2D")).unwrap();
    let mut first = Node::new("ShapeA", "CollisionShape2D");
    first.set_property("size", Variant::Vector2(Vector2::new(20.0, 20.0)));
    tree.add_child(area, first).unwrap();
    let mut second = Node::new("ShapeB", "CollisionShape2D");
    second.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    second.set_property("position", Variant::Vector2(Vector2::new(300.0, 0.0)));
    tree.add_child(area, second).unwrap();

    tree.attach_script(
        area,
        Box::new(GDScriptNodeInstance::from_source(SHAPE_OWNERS_2D, area).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    // Driven from `_input_event` rather than `_ready`: every passing test in
    // this file reaches its script through a click, and `_ready` timing under a
    // bare `step` is an assumption this fixture should not be making.
    ml.register_physics_bodies();
    ml.push_event(click(Vector2::ZERO));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_int(&ml, area, "owner_count"),
        2,
        "`get_shape_owners()` must enumerate both collider children",
    );
    assert_eq!(
        script_int(&ml, area, "shapes_on_first"),
        1,
        "Patina models one collider child as one owner holding ONE shape, so \
         `shape_owner_get_shape_count(0)` is 1 — not the object's total",
    );
    match script_var(&ml, area, "second_owner_x") {
        Variant::Float(x) => assert!(
            (x - 300.0).abs() < 0.001,
            "`shape_owner_get_transform(1)` must return the SECOND collider's \
             own transform, origin.x=300 — got {x}",
        ),
        other => panic!("expected a Float, got {other:?}"),
    }
}
