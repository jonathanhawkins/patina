//! pat-0q2kx.199 acceptance: `CollisionObject2D` picking **signals**.
//!
//! Godot ground truth: `upstream/godot/scene/main/viewport.cpp`
//! (`_process_picking`) and `scene/2d/physics/collision_object_2d.cpp`.
//!
//! pat-0q2kx.138 already joined `point_query` to picking and landed the
//! `input_pickable` gate, so the bead's "UNWIRED / no pick pass" description is
//! stale. Two halves of the Godot behaviour were still missing, and they are
//! what this file drives:
//!
//! - **`mouse_entered` / `mouse_exited`.** Absent outright — no hover state
//!   existed anywhere in the runtime, so a collider could never tell that the
//!   pointer had arrived or left. This is the half that needs *state*: hover is
//!   a transition, underivable from a single frame's hit set.
//! - **The `input_event` signal.** Picking called the virtual `_input_event`
//!   method but never emitted the signal, so the ordinary authoring case — a
//!   scene wiring `input_event` in the editor, with no script on the collider
//!   at all — received nothing.
//!
//! These are integration tests rather than in-module because `gdscene`'s
//! `#[cfg(test)]` build has been intermittently broken by unrelated files; every
//! API used here is public, so the coverage is unaffected.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscript_interop::bindings::{MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo};
use gdvariant::Variant;
use std::cell::RefCell;
use std::rc::Rc;

type CallLog = Rc<RefCell<Vec<String>>>;

/// Records the picking-signal callbacks dispatched to it. Signal connections
/// without a closure resolve to a script method on the target node, so one
/// recorder proves both signal delivery and ordering.
///
/// Only `_on_*` names are kept, and that filter is load-bearing. A listener is
/// a real scripted node, so the engine also dispatches `_input`,
/// `_unhandled_input`, `_process`, `_physics_process` and `_draw` to it every
/// frame. Recording those made a "this collider must receive nothing" check
/// written as `starts_with("far:")` match `far:_process` and fail even though
/// picking had behaved correctly — the assertion has to see picking traffic
/// only.
struct RecordingScript {
    who: String,
    calls: CallLog,
}

impl ScriptInstance for RecordingScript {
    fn call_method(&mut self, name: &str, _args: &[Variant]) -> Result<Variant, ScriptError> {
        if name.starts_with("_on_") {
            self.calls
                .borrow_mut()
                .push(format!("{}:{}", self.who, name));
        }
        Ok(Variant::Nil)
    }
    fn get_property(&self, _name: &str) -> Option<Variant> {
        None
    }
    fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
        false
    }
    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        Vec::new()
    }
    fn list_methods(&self) -> Vec<MethodInfo> {
        Vec::new()
    }
    fn get_script_name(&self) -> &str {
        "RecordingScript"
    }
    // Must be overridden: the trait's default answers from `list_methods`, which
    // is empty here, so signal dispatch would skip every call. That would make
    // this file's *negative* assertions ("must receive nothing") pass
    // vacuously — the failure mode a recorder must never have.
    fn has_method(&self, _name: &str) -> bool {
        true
    }
}

/// Adds an `Area2D` with a 40x40 box at `at`, plus a listener script bound to
/// its three picking signals. Returns the area's id.
fn pickable(tree: &mut SceneTree, name: &str, at: Vector2, log: &CallLog) -> NodeId {
    let root = tree.root_id();
    let area = tree.add_child(root, Node::new(name, "Area2D")).unwrap();
    tree.get_node_mut(area)
        .unwrap()
        .set_property("position", Variant::Vector2(at));

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    tree.add_child(area, shape).unwrap();

    // The listener is a separate node with no collider of its own, so nothing it
    // records can come from picking dispatching to it directly — only from a
    // signal connection actually firing.
    let listener = tree
        .add_child(root, Node::new(&format!("{name}Listener"), "Node"))
        .unwrap();
    tree.attach_script(
        listener,
        Box::new(RecordingScript {
            who: name.to_string(),
            calls: Rc::clone(log),
        }),
    );
    for (signal, method) in [
        ("input_event", "_on_input_event"),
        ("mouse_entered", "_on_mouse_entered"),
        ("mouse_exited", "_on_mouse_exited"),
    ] {
        tree.connect_signal(
            area,
            signal,
            gdobject::signal::Connection::new(listener.object_id(), method),
        );
    }
    area
}

fn click(position: Vector2) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position,
    }
}

fn motion(position: Vector2) -> InputEvent {
    InputEvent::MouseMotion {
        position,
        relative: Vector2::new(0.0, 0.0),
    }
}

/// **The acceptance test.** Two overlapping coins: clicking inside one fires its
/// `input_event` exactly once, hovering on then off fires `mouse_entered` then
/// `mouse_exited`, and an `input_pickable = false` body receives nothing.
#[test]
fn physics2d_picking_input_event_signals() {
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    let mut tree = SceneTree::new();

    // Two coins that overlap: "near" covers 30..70, "far" covers 60..100, so
    // x=65 is inside both and x=35 is inside only "near".
    pickable(&mut tree, "near", Vector2::new(50.0, 50.0), &log);
    pickable(&mut tree, "far", Vector2::new(80.0, 50.0), &log);

    // A third collider opted out of picking, sitting exactly under "near" so a
    // click that reaches one geometrically reaches the other too. Anything it
    // records is a gate failure, not a miss.
    //
    // It gets its OWN log because the shared one is cleared between stages: on
    // the shared log a final "deaf received nothing" check would only cover the
    // last motion, silently exempting every earlier event from the gate.
    let deaf_log: CallLog = Rc::new(RefCell::new(Vec::new()));
    let deaf = pickable(&mut tree, "deaf", Vector2::new(50.0, 50.0), &deaf_log);
    tree.get_node_mut(deaf)
        .unwrap()
        .set_property("input_pickable", Variant::Bool(false));

    let mut ml = MainLoop::new(tree);
    // Picking runs in the input phase, which precedes physics — the colliders
    // must already be registered on the first step (pat-0q2kx.138).
    ml.register_physics_bodies();

    // -- (1) a click fires input_event exactly once on the collider hit -------
    ml.push_event(click(Vector2::new(35.0, 50.0)));
    ml.step(1.0 / 60.0);

    let clicks = log
        .borrow()
        .iter()
        .filter(|e| *e == "near:_on_input_event")
        .count();
    assert_eq!(
        clicks,
        1,
        "a click inside a collider must emit `input_event` exactly once — the \
         signal was never emitted at all before this bead; a count assertion \
         also catches the opposite bug of emitting per shape, got {:?}",
        log.borrow()
    );
    assert!(
        !log.borrow().iter().any(|e| e.starts_with("far:")),
        "x=35 is outside `far`, which must receive nothing, got {:?}",
        log.borrow()
    );

    // -- (2) hover transitions ------------------------------------------------
    // Entering `near` only. The click above already put the pointer inside it,
    // so this also pins that enter is not re-emitted while the pointer stays.
    log.borrow_mut().clear();
    ml.push_event(motion(Vector2::new(35.0, 50.0)));
    ml.step(1.0 / 60.0);
    assert!(
        !log.borrow().iter().any(|e| e == "near:_on_mouse_entered"),
        "the pointer was already inside `near`, so no second `mouse_entered` \
         may fire — hover is a transition, not a per-frame state, got {:?}",
        log.borrow()
    );

    // Slide into the overlap: `far` is newly entered, `near` still holds.
    log.borrow_mut().clear();
    ml.push_event(motion(Vector2::new(65.0, 50.0)));
    ml.step(1.0 / 60.0);
    assert!(
        log.borrow().iter().any(|e| e == "far:_on_mouse_entered"),
        "moving into `far` must emit `mouse_entered`, got {:?}",
        log.borrow()
    );
    assert!(
        !log.borrow().iter().any(|e| e == "near:_on_mouse_exited"),
        "x=65 is still inside `near`, which must NOT report an exit, got {:?}",
        log.borrow()
    );

    // Slide out of `near` into `far` alone.
    log.borrow_mut().clear();
    ml.push_event(motion(Vector2::new(95.0, 50.0)));
    ml.step(1.0 / 60.0);
    assert!(
        log.borrow().iter().any(|e| e == "near:_on_mouse_exited"),
        "leaving `near` must emit `mouse_exited`, got {:?}",
        log.borrow()
    );
    assert!(
        !log.borrow().iter().any(|e| e == "far:_on_mouse_exited"),
        "`far` was never left and must not report an exit, got {:?}",
        log.borrow()
    );

    // Off every collider: the last hovered one releases.
    log.borrow_mut().clear();
    ml.push_event(motion(Vector2::new(500.0, 500.0)));
    ml.step(1.0 / 60.0);
    assert!(
        log.borrow().iter().any(|e| e == "far:_on_mouse_exited"),
        "moving off all colliders must release the last hover — an empty hit \
         set is the case a naive `for hit in hits` implementation skips \
         entirely, got {:?}",
        log.borrow()
    );

    // -- (3) input_pickable = false receives nothing --------------------------
    // `deaf_log` spans every event above, not just the last one, and holds only
    // `_on_*` callbacks — so an empty log here means the gate held for the whole
    // run rather than merely for the final frame.
    assert!(
        deaf_log.borrow().is_empty(),
        "input_pickable = false opts out of picking entirely, including the \
         hover signals — `deaf` sits directly under `near`, so every click and \
         hover that reached `near` reached it geometrically too, got {:?}",
        deaf_log.borrow()
    );
}

/// Exits precede entries when the pointer crosses between two colliders in one
/// motion, so a script tracking "what am I over" never briefly holds both.
#[test]
fn exit_is_emitted_before_enter_when_crossing() {
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    let mut tree = SceneTree::new();
    // Disjoint: 30..70 and 130..170, so one motion leaves the first and enters
    // the second with no overlap frame in between.
    pickable(&mut tree, "a", Vector2::new(50.0, 50.0), &log);
    pickable(&mut tree, "b", Vector2::new(150.0, 50.0), &log);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.push_event(motion(Vector2::new(50.0, 50.0)));
    ml.step(1.0 / 60.0);
    log.borrow_mut().clear();

    ml.push_event(motion(Vector2::new(150.0, 50.0)));
    ml.step(1.0 / 60.0);

    let got = log.borrow().clone();
    let exit = got.iter().position(|e| e == "a:_on_mouse_exited");
    let enter = got.iter().position(|e| e == "b:_on_mouse_entered");
    assert!(exit.is_some(), "`a` must report an exit, got {got:?}");
    assert!(enter.is_some(), "`b` must report an enter, got {got:?}");
    assert!(
        exit < enter,
        "Godot releases the old collider before the new one takes over, got {got:?}"
    );
}
