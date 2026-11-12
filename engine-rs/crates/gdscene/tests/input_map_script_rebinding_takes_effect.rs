//! pat-0q2kx.89 — the acceptance clause no existing test covered: a **GDScript**
//! script rebinding an action through `InputMap` changes what a later event
//! fires, in the same run.
//!
//! The bead's other clauses are pinned elsewhere:
//! `gdplatform/tests/input_map_action_semantics_parity.rs` (axis direction, the
//! deadzone strength remap, modifier bindings) and
//! `tests/input_suite/input_map_action_semantics_parity.rs` (the same plus
//! `project.godot` joypad parsing, the builtin `ui_*` table, and the **Rust**
//! rebinding API). What neither reaches is the clause as written:
//!
//! > a script calling `InputMap.add_action("dash")` + `InputMap.action_add_event`
//! > binding `KEY_Q` makes `Input.is_action_pressed("dash")` true after a pushed
//! > Q key event
//!
//! That path is longer than the Rust API and can break on its own: the script
//! phase only *queues* an `InputCommand` on the tree (`scripting.rs`
//! `input_map_add_action` / `input_map_action_add_event`), and
//! `MainLoop::apply_script_input_commands` drains it into the engine
//! `InputState` at the end of the frame. A regression anywhere in
//! interpreter -> SceneAccess -> queue -> drain leaves the script silently
//! binding nothing, with every Rust-level test still green.
//!
//! Lives in `gdscene` because it needs the real `MainLoop`-owned `InputState`:
//! `gdplatform` cannot see the interpreter, and the interpreter crate cannot
//! see `MainLoop`.

use gdplatform::input::{InputEvent, InputMap, Key};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reads the action *before* binding it, so frame 1 is an honest control: at
/// that point `dash` does not exist and a held Q must not fire it.
const REBINDER: &str = "\
extends Node
var bound = false
var dash = false
func _process(delta):
    if Input.is_action_pressed(\"dash\"):
        self.dash = true
    if not self.bound:
        InputMap.add_action(\"dash\")
        InputMap.action_add_event(\"dash\", KEY_Q)
        self.bound = true
";

fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

fn key_event(key: Key, pressed: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

/// A scene whose only node rebinds its own action from GDScript, over an input
/// map that starts with **no** `dash` action.
fn rebinder_scene() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Rebinder", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(REBINDER, node).expect("rebinder parses")),
    );

    let mut ml = MainLoop::new(tree);
    ml.set_input_map(InputMap::new());
    (ml, node)
}

/// **The missing acceptance clause.** `InputMap.add_action` +
/// `action_add_event(KEY_Q)` from a script make a later Q press fire the action.
#[test]
fn script_bound_action_fires_from_a_later_key_event() {
    let (mut ml, node) = rebinder_scene();

    // Frame 1 — Q is already held, but `dash` is not bound yet (the script binds
    // it later in this same frame). This is the control: without it, a test that
    // only ever asserts `true` cannot tell a working binding from an action that
    // was somehow pressed all along.
    ml.push_event(key_event(Key::Q, true));
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), node, "dash"),
        Variant::Bool(false),
        "Q must not fire `dash` before the script binds it — otherwise the \
         assertion below proves nothing"
    );
    assert_eq!(
        script_prop(ml.tree(), node, "bound"),
        Variant::Bool(true),
        "the script's InputMap calls must have run at all"
    );

    // Frame 2 — a fresh press of the same key, now that the binding has been
    // drained into the engine InputState.
    ml.push_event(key_event(Key::Q, false));
    ml.push_event(key_event(Key::Q, true));
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), node, "dash"),
        Variant::Bool(true),
        "InputMap.add_action + action_add_event(KEY_Q) from GDScript must make \
         Input.is_action_pressed(\"dash\") true for a later Q press — the whole \
         interpreter -> SceneAccess -> queued InputCommand -> drain path"
    );
}

/// The engine map really was mutated, not just the script's own view: the host
/// can see the action the script created.
#[test]
fn the_script_binding_lands_on_the_engine_input_map() {
    let (mut ml, _node) = rebinder_scene();
    ml.step(1.0 / 60.0);

    let map = ml
        .input_state()
        .input_map()
        .expect("the loop was given an input map at construction");
    assert!(
        map.has_action("dash"),
        "the action a script created must exist on the engine map, not only in \
         the tree's read mirror"
    );
    assert_eq!(
        map.get_bindings("dash").map(|b| b.len()),
        Some(1),
        "…carrying exactly the one binding the script added"
    );
}
