//! pat-nzgdu.155 — `UndoRedo` registered in ClassDB and script-bound.
//!
//! Drives a real GDScript class through the real interpreter dispatch: the
//! script calls `UndoRedo.new()`, records paired do/undo property operations
//! against a node it reached via `get_node()`, and commits. A recording
//! `SceneAccess` mock is the observation point — `commit_action()` / `undo()` /
//! `redo()` are only "wired" if the property writes actually land on the scene,
//! so every assertion reads the mock's store rather than the UndoRedo history.
//!
//! Note on the binding's location: `UndoRedo` is implemented interpreter-local
//! (the `CameraFeed` handle pattern) and NOT delegated to gdeditor's
//! `UndoRedoManager` — gdeditor depends on gdscript-interop, so reaching the
//! other way is a dependency cycle. See the bead comment.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gdscript_interop::bindings::SceneAccess;
use gdscript_interop::interpreter::{ClassInstance, Interpreter};
use gdvariant::Variant;

/// The node id the mock hands back for `get_node("Target")`.
const TARGET: u64 = 42;

#[derive(Default)]
struct SceneState {
    /// `(node, property)` -> current value.
    props: HashMap<(u64, String), Variant>,
    /// Every `set_node_property` call in order — proves *when* writes happen,
    /// not just where they land.
    writes: Vec<(u64, String, Variant)>,
}

struct SceneMock {
    state: Rc<RefCell<SceneState>>,
}

impl SceneAccess for SceneMock {
    fn get_node(&self, _from: u64, path: &str) -> Option<u64> {
        if path == "Target" {
            Some(TARGET)
        } else {
            None
        }
    }
    fn get_parent(&self, _node: u64) -> Option<u64> {
        None
    }
    fn get_children(&self, _node: u64) -> Vec<u64> {
        Vec::new()
    }
    fn get_node_property(&self, node: u64, prop: &str) -> Variant {
        self.state
            .borrow()
            .props
            .get(&(node, prop.to_string()))
            .cloned()
            .unwrap_or(Variant::Nil)
    }
    fn set_node_property(&mut self, node: u64, prop: &str, value: Variant) {
        let mut s = self.state.borrow_mut();
        s.props.insert((node, prop.to_string()), value.clone());
        s.writes.push((node, prop.to_string(), value));
    }
    fn emit_signal(&mut self, _node: u64, _signal: &str, _args: &[Variant]) {}
    fn connect_signal(&mut self, _source: u64, _signal: &str, _target: u64, _method: &str) {}
    fn get_node_name(&self, _node: u64) -> Option<String> {
        None
    }
}

/// `merge_mode` reaches the script as a plain int via ClassDB reflection —
/// exactly how a GDScript author reads a class enum constant today.
const SCRIPT: &str = "\
extends Node

var ur = null

func begin() -> void:
    ur = UndoRedo.new()

func record(action_name, merge_mode, before, after) -> void:
    var target = get_node(\"Target\")
    ur.create_action(action_name, merge_mode)
    ur.add_do_property(target, \"value\", after)
    ur.add_undo_property(target, \"value\", before)
    ur.commit_action()

func do_undo():
    return ur.undo()

func do_redo():
    return ur.redo()

func history_count():
    return ur.get_history_count()

func current_action_name():
    return ur.get_current_action_name()

func version():
    return ur.get_version()

func has_undo():
    return ur.has_undo()

func has_redo():
    return ur.has_redo()

func merge_ends_constant():
    return ClassDB.class_get_integer_constant(\"UndoRedo\", \"MERGE_ENDS\")
";

/// A live script instance plus the scene it writes through.
struct Harness {
    interp: Interpreter,
    inst: ClassInstance,
    state: Rc<RefCell<SceneState>>,
}

impl Harness {
    fn new() -> Self {
        gdobject::register_io_classes();
        let state = Rc::new(RefCell::new(SceneState::default()));
        let mut interp = Interpreter::new();
        let class_def = interp.run_class(SCRIPT).expect("script parses");
        let inst = interp.instantiate_class(&class_def).expect("script instantiates");
        interp.set_scene_access(
            Box::new(SceneMock {
                state: state.clone(),
            }),
            1,
        );
        let mut h = Harness {
            interp,
            inst,
            state,
        };
        h.call("begin", &[]);
        h
    }

    fn call(&mut self, method: &str, args: &[Variant]) -> Variant {
        self.interp
            .call_instance_method(&mut self.inst, method, args)
            .unwrap_or_else(|e| panic!("{method}() dispatches: {e:?}"))
    }

    /// Records one action: `value` goes `before` -> `after`.
    fn record(&mut self, name: &str, merge_mode: i64, before: i64, after: i64) {
        self.call(
            "record",
            &[
                Variant::String(name.to_string()),
                Variant::Int(merge_mode),
                Variant::Int(before),
                Variant::Int(after),
            ],
        );
    }

    /// The value the scene actually holds — the only honest observation point.
    fn value(&self) -> Variant {
        self.state
            .borrow()
            .props
            .get(&(TARGET, "value".to_string()))
            .cloned()
            .unwrap_or(Variant::Nil)
    }

    fn write_count(&self) -> usize {
        self.state.borrow().writes.len()
    }

    fn int(&mut self, method: &str) -> i64 {
        match self.call(method, &[]) {
            Variant::Int(i) => i,
            o => panic!("{method}() must return an int, got {o:?}"),
        }
    }

    fn boolean(&mut self, method: &str) -> bool {
        match self.call(method, &[]) {
            Variant::Bool(b) => b,
            o => panic!("{method}() must return a bool, got {o:?}"),
        }
    }
}

/// The bead's acceptance test: create -> record -> commit -> undo -> redo,
/// plus the ClassDB inheritance and MERGE_ENDS coalescing clauses.
#[test]
fn undoredo_class_commit_undo_redo_property() {
    // -- ClassDB reports UndoRedo <- Object ---------------------------------
    gdobject::register_io_classes();
    assert!(
        gdobject::class_exists("UndoRedo"),
        "register_io_classes() must register UndoRedo"
    );
    assert!(
        gdobject::is_parent_class("UndoRedo", "Object"),
        "UndoRedo derives Object (it is not a Resource or RefCounted)"
    );
    assert!(
        gdobject::class_has_enum("UndoRedo", "MergeMode"),
        "UndoRedo exposes the MergeMode enum"
    );

    // -- A script records a do/undo property change -------------------------
    let mut h = Harness::new();
    assert_eq!(h.value(), Variant::Nil, "nothing written before commit");
    assert!(!h.boolean("has_undo"), "a fresh UndoRedo has nothing to undo");

    h.record("Set Value", 0, 1, 2);

    // commit_action() applies the do-half.
    assert_eq!(
        h.value(),
        Variant::Int(2),
        "commit_action() applies the do-property through SceneAccess"
    );
    assert_eq!(h.write_count(), 1, "commit writes exactly once");
    assert_eq!(h.int("history_count"), 1);
    assert!(h.boolean("has_undo"));
    assert!(!h.boolean("has_redo"), "nothing to redo at the history tip");
    assert_eq!(
        h.call("current_action_name", &[]),
        Variant::String("Set Value".to_string())
    );

    // undo() reverts it.
    assert!(h.boolean("do_undo"), "undo() reports it did something");
    assert_eq!(
        h.value(),
        Variant::Int(1),
        "undo() applies the undo-property, restoring the old value"
    );
    assert!(!h.boolean("has_undo"), "fully undone");
    assert!(h.boolean("has_redo"), "the undone action is redoable");

    // redo() re-applies it.
    assert!(h.boolean("do_redo"), "redo() reports it did something");
    assert_eq!(
        h.value(),
        Variant::Int(2),
        "redo() re-applies the do-property"
    );

    // Exhausted in both directions: no write, and a false return.
    let writes = h.write_count();
    assert!(!h.boolean("do_redo"), "redo() past the tip is a no-op");
    assert_eq!(h.write_count(), writes, "a no-op redo writes nothing");
}

/// MERGE_ENDS coalesces two same-named actions into one history entry, keeping
/// the *first* action's undo-ops and the *last* action's do-ops — so undoing the
/// merged pair jumps all the way back past both.
#[test]
fn undoredo_merge_ends_coalesces_same_named_actions() {
    let mut h = Harness::new();
    let merge_ends = h.int("merge_ends_constant");
    assert_eq!(
        merge_ends, 1,
        "UndoRedo.MERGE_ENDS == 1, and is reachable via ClassDB reflection"
    );

    h.record("Move", merge_ends, 0, 10);
    assert_eq!(h.int("history_count"), 1);

    // Same name + MERGE_ENDS -> coalesced into the previous action.
    h.record("Move", merge_ends, 10, 20);
    assert_eq!(
        h.value(),
        Variant::Int(20),
        "the merged action still applies its do-half"
    );
    assert_eq!(
        h.int("history_count"),
        1,
        "MERGE_ENDS coalesces: two commits, one history entry"
    );

    // The "ends" semantic: the ORIGINAL undo op survived the merge.
    assert!(h.boolean("do_undo"));
    assert_eq!(
        h.value(),
        Variant::Int(0),
        "undo after MERGE_ENDS returns to the state before the FIRST action, \
         not the intermediate value — the first action's undo-ops are kept"
    );
    assert!(!h.boolean("has_undo"), "one merged entry, one undo");
}

/// A differently-named action must never merge, even under MERGE_ENDS — and
/// MERGE_DISABLE must never merge at all. This is the contrast that proves the
/// merge above was the merge mode doing the work, not a coincidence.
#[test]
fn undoredo_merge_only_coalesces_matching_names() {
    let mut h = Harness::new();
    let merge_ends = h.int("merge_ends_constant");

    h.record("Move", merge_ends, 0, 10);
    h.record("Rotate", merge_ends, 10, 20);
    assert_eq!(
        h.int("history_count"),
        2,
        "MERGE_ENDS only merges an action with the SAME name as the previous"
    );

    let mut h = Harness::new();
    h.record("Move", 0, 0, 10);
    h.record("Move", 0, 10, 20);
    assert_eq!(
        h.int("history_count"),
        2,
        "MERGE_DISABLE never merges, even for identical names"
    );
    assert!(h.boolean("do_undo"));
    assert_eq!(
        h.value(),
        Variant::Int(10),
        "unmerged, undo steps back one action to the intermediate value"
    );
}

/// Every commit/undo/redo bumps the version; a no-op does not.
#[test]
fn undoredo_version_tracks_committed_mutations() {
    let mut h = Harness::new();
    assert_eq!(h.int("version"), 0, "a fresh UndoRedo is at version 0");

    h.record("Set Value", 0, 1, 2);
    let after_commit = h.int("version");
    assert_eq!(after_commit, 1, "commit_action() bumps the version");

    h.boolean("do_undo");
    assert_eq!(h.int("version"), 2, "undo() bumps the version");

    h.boolean("do_redo");
    assert_eq!(h.int("version"), 3, "redo() bumps the version");

    // Exhausted redo changes nothing.
    h.boolean("do_redo");
    assert_eq!(h.int("version"), 3, "a no-op redo does not bump the version");
}

/// A new action taken after an undo forks the history: the redoable tail is
/// discarded rather than left dangling.
#[test]
fn undoredo_new_action_after_undo_discards_the_redo_tail() {
    let mut h = Harness::new();
    h.record("First", 0, 0, 10);
    h.record("Second", 0, 10, 20);
    assert_eq!(h.int("history_count"), 2);

    h.boolean("do_undo");
    assert_eq!(h.value(), Variant::Int(10));
    assert!(h.boolean("has_redo"), "Second is redoable");

    // Committing here forks: Second is gone.
    h.record("Third", 0, 10, 30);
    assert_eq!(h.value(), Variant::Int(30));
    assert!(
        !h.boolean("has_redo"),
        "committing after an undo discards the redo tail"
    );
    assert_eq!(
        h.int("history_count"),
        2,
        "First + Third — Second was truncated away"
    );
    assert_eq!(
        h.call("current_action_name", &[]),
        Variant::String("Third".to_string())
    );
}
