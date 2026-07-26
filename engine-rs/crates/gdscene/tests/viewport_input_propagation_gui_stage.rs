//! pat-0q2kx.138 acceptance: `push_input`'s full Godot stage order, including
//! the per-node `set_process_input` gate and physics object picking.
//!
//! Godot ground truth: `upstream/godot/scene/main/viewport.cpp:3464-3580`
//! (`_input` group → GUI stage → shortcut → unhandled_key → unhandled →
//! physics picking) and `scene_tree.cpp::_call_input_pause`, which walks the
//! input group in **reverse** and gates each node on `can_process` *and*
//! `is_processing_input()`.
//!
//! Earlier beads landed the GUI stage (pat-0q2kx.18 / .84) and the
//! reverse-order, pause-gated walk (pat-0q2kx.91). Two pieces were still
//! missing and are what this file drives:
//!
//! - **`set_process_input`.** `Node` had `set_process` and
//!   `set_physics_process` but no input toggle at all, so a node that opted out
//!   of input still received every event. `should_process_node` only answers
//!   the *pause* question.
//! - **Physics picking.** `PhysicsWorld2D::point_query` and the
//!   `input_pickable` property both existed with nothing joining them, so a
//!   click never reached an `Area2D`'s `_input_event`.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscript_interop::bindings::{MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo};
use gdvariant::Variant;
use std::cell::RefCell;
use std::rc::Rc;

type CallLog = Rc<RefCell<Vec<(String, Vec<Variant>)>>>;

/// Records every method the engine dispatches, tagged with the node's name so
/// one shared log proves *ordering* between nodes, not just arrival.
struct RecordingScript {
    who: String,
    calls: CallLog,
}

impl ScriptInstance for RecordingScript {
    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError> {
        self.calls
            .borrow_mut()
            .push((format!("{}:{}", self.who, name), args.to_vec()));
        Ok(Variant::Nil)
    }
    fn get_property(&self, _name: &str) -> Option<Variant> {
        None
    }
    fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
        false
    }
    fn list_methods(&self) -> Vec<MethodInfo> {
        Vec::new()
    }
    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        Vec::new()
    }
    fn get_script_name(&self) -> &str {
        "RecordingScript"
    }
    fn has_method(&self, _name: &str) -> bool {
        true
    }
}

fn attach(tree: &mut SceneTree, node: NodeId, who: &str, log: &CallLog) {
    tree.attach_script(
        node,
        Box::new(RecordingScript {
            who: who.to_string(),
            calls: log.clone(),
        }),
    );
}

fn click(position: Vector2) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position,
    }
}

fn names(log: &CallLog) -> Vec<String> {
    log.borrow().iter().map(|(n, _)| n.clone()).collect()
}

/// **The acceptance test.** Reverse dispatch order, the `set_process_input`
/// gate, and physics picking reaching an `input_pickable` collider.
#[test]
fn viewport_input_propagation_gui_stage() {
    // -- (1) two overlapping scripted nodes receive `_input` in REVERSE order --
    //
    // Godot's `_call_input_pause` walks the input group backwards, so the node
    // added last sees the event first and can consume it before its ancestors.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let first = tree.add_child(root, Node::new("First", "Node2D")).unwrap();
    let second = tree.add_child(root, Node::new("Second", "Node2D")).unwrap();
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    attach(&mut tree, first, "first", &log);
    attach(&mut tree, second, "second", &log);

    tree.push_input(&click(Vector2::new(10.0, 10.0)));
    let order: Vec<String> = names(&log)
        .into_iter()
        .filter(|n| n.ends_with(":_input"))
        .collect();
    assert_eq!(
        order,
        vec!["second:_input".to_string(), "first:_input".to_string()],
        "`_input` must reach the LAST-added node first — Godot iterates the \
         input group in reverse so a child can consume before its ancestors"
    );

    // -- (2) a node after set_process_input(false) receives nothing -----------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let listening = tree.add_child(root, Node::new("On", "Node2D")).unwrap();
    let muted = tree.add_child(root, Node::new("Off", "Node2D")).unwrap();
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    attach(&mut tree, listening, "on", &log);
    attach(&mut tree, muted, "off", &log);

    assert!(
        tree.is_node_processing_input(muted),
        "nodes process input by default, as in Godot"
    );
    assert!(tree.set_node_process_input(muted, false));

    tree.push_input(&click(Vector2::new(5.0, 5.0)));
    let got = names(&log);
    assert!(
        got.iter().any(|n| n.starts_with("on:")),
        "the still-listening node must receive the event, got {got:?}"
    );
    assert!(
        !got.iter().any(|n| n.starts_with("off:")),
        "a node that called set_process_input(false) must receive NOTHING, got {got:?}"
    );

    // -- (3) physics picking reaches an input_pickable collider ---------------
    //
    // Clicking empty space is not consumed by the GUI stage, so it falls all
    // the way through to picking — Godot's last input stage.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let area = tree
        .add_child(root, Node::new("Pickable", "Area2D"))
        .unwrap();
    {
        let node = tree.get_node_mut(area).unwrap();
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    }
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    tree.add_child(area, shape).unwrap();

    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    attach(&mut tree, area, "area", &log);

    let mut ml = MainLoop::new(tree);
    // Picking runs in the INPUT phase, which precedes the physics phase — so on
    // the first step the collider must already be registered. This is the
    // documented "call after loading a scene" entry point (`register_bodies`
    // registers Area2D into the area store, physics_server.rs:689), and it is
    // what the runner and editor Play both do at boot.
    ml.register_physics_bodies();
    ml.push_event(click(Vector2::new(50.0, 50.0)));
    ml.step(1.0 / 60.0);

    let got = names(&log);
    assert!(
        got.iter().any(|n| n == "area:_unhandled_input"),
        "a click on empty GUI space must fall through to _unhandled_input, got {got:?}"
    );
    assert!(
        got.iter().any(|n| n == "area:_input_event"),
        "physics picking must call _input_event on an input_pickable collider \
         under the cursor — point_query and `input_pickable` both existed but \
         nothing joined them, got {got:?}"
    );
    // Picking is the LAST stage: it must come after _unhandled_input.
    let unhandled = got.iter().position(|n| n == "area:_unhandled_input");
    let picked = got.iter().position(|n| n == "area:_input_event");
    assert!(
        unhandled < picked,
        "picking is Godot's final input stage, so _input_event must follow \
         _unhandled_input: {got:?}"
    );
}

/// `input_pickable = false` opts a collider out of picking entirely (Godot's
/// `CollisionObject2D.input_pickable`), while leaving the script's other input
/// callbacks alone.
#[test]
fn input_pickable_false_is_not_picked() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let area = tree.add_child(root, Node::new("Deaf", "Area2D")).unwrap();
    {
        let node = tree.get_node_mut(area).unwrap();
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        node.set_property("input_pickable", Variant::Bool(false));
    }
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_string()));
    shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    tree.add_child(area, shape).unwrap();

    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    attach(&mut tree, area, "area", &log);

    let mut ml = MainLoop::new(tree);
    // Picking runs in the INPUT phase, which precedes the physics phase — so on
    // the first step the collider must already be registered. This is the
    // documented "call after loading a scene" entry point (`register_bodies`
    // registers Area2D into the area store, physics_server.rs:689), and it is
    // what the runner and editor Play both do at boot.
    ml.register_physics_bodies();
    ml.push_event(click(Vector2::new(50.0, 50.0)));
    ml.step(1.0 / 60.0);

    let got = names(&log);
    assert!(
        !got.iter().any(|n| n == "area:_input_event"),
        "input_pickable = false must take the collider out of picking, got {got:?}"
    );
    assert!(
        got.iter().any(|n| n == "area:_unhandled_input"),
        "opting out of *picking* must not stop the ordinary input stages, got {got:?}"
    );
}
