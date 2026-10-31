//! pat-mgixk.7: a ClassDB registration that nothing dispatches must fail loudly.
//!
//! **Wiring**: every call below enters through a `.tscn`-loaded scene, a
//! GDScript attached to its root, and `SceneTree::call_script_method` — i.e.
//! `Expr::Call` -> `Interpreter::call_method_on` -> `call_object_method`, the
//! exact path a game takes. Nothing here pokes ClassDB directly to decide
//! whether a method works.
//!
//! **Godot ground truth**: `Object::callp` (`core/object/object.cpp`) resolves
//! the name through ClassDB to a `MethodBind` and runs it. A registered method
//! therefore either does real work or raises a script error. Godot has no state
//! in which `has_method()` is true and the call answers a silent `null`.
//!
//! Patina had exactly that state, for all but 80 of the 562 methods registered on
//! Node-derived classes. `call_object_method` returned `Ok(Variant::Nil)`
//! whenever ClassDB knew the name and no arm answered, which made `has_method()`
//! lie and hid every gap — the reason the same bug was fixed piecemeal at least
//! four times. See `gdscript_interop::native_method_gaps` for the tables and
//! how they were derived.

use gdobject::class_db;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::SceneTree;
use gdscript_interop::native_method_gaps::{
    is_known_gap, is_void_noop, not_implemented_message, KNOWN_MISSING_NATIVE_METHODS,
    NON_GODOT_REGISTRATIONS,
};
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

/// `MeshInstance3D.create_outline` is registered (`class_db.rs`) and has no
/// dispatch arm, and Godot's `MeshInstance3D.create_outline` returns a `Mesh`,
/// so a `null` answer is a wrong answer.
const SCENE: &str = r#"[gd_scene format=3 uid="uid://pat_mgixk_7_dispatch"]

[node name="Root" type="Node2D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
"#;

const SCRIPT: &str = r#"extends Node2D

var classdb_claims_create_outline = false
var classdb_claims_bogus = true
var void_noop_is_null = false
var dispatched_method_answered = ""
var reached_end_of_ready = false

func _ready():
    # ClassDB says the method exists...
    classdb_claims_create_outline = $Mesh.has_method("create_outline")
    # ...and says this one does not, so "it raised" is not the whole story:
    # an unknown name raises too. The pair of answers is what makes the
    # assertion specific.
    classdb_claims_bogus = $Mesh.has_method("patina_not_a_godot_method")

    # `orthonormalize()` is Godot `void` on every class that declares it, so a
    # missing implementation is unobservable and it stays a silent no-op
    # instead of raising. It must NOT abort _ready.
    void_noop_is_null = $Mesh.orthonormalize() == null

    # A method with a real arm still answers its real value.
    dispatched_method_answered = $Mesh.get_name()

    reached_end_of_ready = true

func call_unimplemented_native():
    return $Mesh.create_outline(1.0)

func call_unregistered_native():
    return $Mesh.patina_not_a_godot_method()

func call_void_noop():
    return $Mesh.orthonormalize()
"#;

/// Loads the fixture scene, attaches `SCRIPT` to its root, runs the lifecycle
/// and steps a MainLoop. The loop is returned rather than unwrapped because the
/// tests keep calling script methods through `MainLoop::tree_mut`.
fn run_fixture() -> (MainLoop, gdscene::NodeId) {
    class_db::register_all_classes();

    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene should parse");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let scene_root =
        add_packed_scene_to_tree(&mut tree, tree_root, &packed).expect("scene should instance");

    let instance = GDScriptNodeInstance::from_source(SCRIPT, scene_root)
        .expect("acceptance script should parse");
    tree.attach_script(scene_root, Box::new(instance));

    for nid in tree.all_nodes_in_tree_order() {
        tree.process_script_enter_tree(nid);
    }
    for nid in tree.all_nodes_in_tree_order() {
        if tree.has_script(nid) {
            tree.process_script_ready(nid);
        }
    }

    let mut main_loop = MainLoop::new(tree);
    for _ in 0..3 {
        main_loop.step(DT);
    }
    (main_loop, scene_root)
}

fn script_var(tree: &SceneTree, node: gdscene::NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script should be attached")
        .get_property(name)
        .unwrap_or_else(|| panic!("script should declare `{name}`"))
}

// ===========================================================================
// 1. The acceptance: a registered method with no implementation raises
// ===========================================================================

#[test]
fn registered_but_unimplemented_native_call_raises_instead_of_answering_null() {
    let (mut main_loop, root) = run_fixture();

    // Preconditions, so the raise below cannot be read as "the name is unknown".
    assert_eq!(
        script_var(main_loop.tree(), root, "classdb_claims_create_outline"),
        Variant::Bool(true),
        "precondition: ClassDB must register MeshInstance3D.create_outline — \
         without that, call_object_method never reaches the branch under test"
    );
    assert_eq!(
        script_var(main_loop.tree(), root, "classdb_claims_bogus"),
        Variant::Bool(false),
        "precondition: has_method() must still say no for a name Godot has not got"
    );
    assert!(
        is_known_gap("MeshInstance3D", "create_outline"),
        "precondition: create_outline is the recorded gap this test leans on"
    );

    // The behaviour change: ClassDB claims the method, so Godot would run a
    // MethodBind. Patina has no arm, so it must raise rather than hand back the
    // Nil placeholder. `call_script_method` answers None when the body raised.
    assert!(
        main_loop
            .tree_mut()
            .call_script_method(root, "call_unimplemented_native", &[])
            .is_none(),
        "$Mesh.create_outline(1.0) is registered with no dispatch arm and returns a \
         Mesh in Godot, so it must raise. Answering Some(Nil) here is the old silent \
         placeholder: has_method() says yes and the call says nothing."
    );
}

#[test]
fn a_void_method_with_no_arm_stays_a_silent_no_op() {
    let (mut main_loop, root) = run_fixture();

    // The allow-list exists so that flipping the placeholder to an error does
    // not break callers that correctly ignore a void result.
    assert!(
        is_void_noop("orthonormalize"),
        "precondition: orthonormalize is on the void allow-list"
    );
    assert_eq!(
        script_var(main_loop.tree(), root, "void_noop_is_null"),
        Variant::Bool(true),
        "a void no-op answers null, and crucially does NOT abort _ready"
    );
    assert_eq!(
        script_var(main_loop.tree(), root, "reached_end_of_ready"),
        Variant::Bool(true),
        "_ready must run past the void no-op to its last statement"
    );
    assert!(
        main_loop
            .tree_mut()
            .call_script_method(root, "call_void_noop", &[])
            .is_some(),
        "calling an allow-listed void no-op directly must not raise either"
    );
}

#[test]
fn a_dispatched_method_still_answers_and_an_unregistered_one_still_raises() {
    let (mut main_loop, root) = run_fixture();

    // The control in the other direction: the new error must not swallow the
    // methods that DO work.
    assert_eq!(
        script_var(main_loop.tree(), root, "dispatched_method_answered"),
        Variant::String("Mesh".into()),
        "get_name() has a real dispatch arm and must keep answering"
    );
    assert!(
        main_loop
            .tree_mut()
            .call_script_method(root, "call_unregistered_native", &[])
            .is_none(),
        "a name ClassDB does not know raised before this change and must still raise"
    );
}

#[test]
fn the_raised_message_names_the_class_and_the_method() {
    // `SceneTree::call_script_method` logs and swallows the script error, and no
    // public API hands it back, so the text is asserted against the single
    // formatter `call_object_method` actually calls rather than against a copy
    // captured from the running script.
    let msg = not_implemented_message("MeshInstance3D", "create_outline");
    assert!(
        msg.contains("MeshInstance3D") && msg.contains("create_outline"),
        "the error must name the class and the method so the gap is actionable: {msg}"
    );
}

// ===========================================================================
// 2. The sweep: every raiser must be a recorded gap
// ===========================================================================

/// What calling a zero-argument registered method through the interpreter did.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The call produced a value (including a legitimate null).
    Answered,
    /// The call raised — after this change, that means "registered but not
    /// implemented", or a real arm rejecting the call.
    Raised,
}

/// Calls `obj.<method>()` on a node of `class_name` from a GDScript attached to
/// a sibling node, and reports whether the call answered or raised.
///
/// ⚠️ Uses the real `obj.<method>()` syntax, NOT `obj.call("<method>")`. In
/// Patina those are different dispatch surfaces: `Object.call` routes straight
/// into `call_object_method`, which consults only
/// `SceneAccess::call_node_method`, while `obj.<method>()` goes through
/// `call_method_on`'s own `Variant::ObjectId` method table first and falls
/// through to `call_object_method` after. Probing with `call()` therefore
/// reports methods as missing that `$Node.method()` resolves perfectly well —
/// it cost this test a verify. (Godot has no such split: `Object::callp` is the
/// one path. Worth its own bead.) The price of the real syntax is one generated
/// script per pair, which is a few hundred three-line parses.
///
/// A fresh `SceneTree` per call is deliberate: `call_script_method` lifts the
/// script out of the arena for the duration of the call and re-inserts it
/// afterwards, so a panic inside a dispatch arm would leave the probe script
/// detached and make every later call in that tree answer `None` for the wrong
/// reason.
fn probe(class_name: &str, method: &str) -> Outcome {
    let source = format!("extends Node\nfunc probe(obj):\n    return obj.{method}()\n");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe_id = tree
        .add_child(root, Node::new("Probe", "Node"))
        .expect("probe node should attach");
    let target_id = tree
        .add_child(root, Node::new("Target", class_name))
        .expect("target node should attach");
    let script = GDScriptNodeInstance::from_source(&source, probe_id)
        .unwrap_or_else(|e| panic!("probe script for {class_name}.{method} should parse: {e}"));
    tree.attach_script(probe_id, Box::new(script));

    let args = [Variant::ObjectId(target_id.object_id())];
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.call_script_method(probe_id, "probe", &args)
    }));
    match called {
        Ok(Some(_)) => Outcome::Answered,
        // A panic inside a dispatch arm is not an answer either; it must be
        // recorded like any other gap rather than silently counted as working.
        Ok(None) | Err(_) => Outcome::Raised,
    }
}

#[test]
fn classdb_registered_node_methods_have_dispatch_arms() {
    class_db::register_all_classes();

    let mut unlisted: Vec<String> = Vec::new();
    let (mut answered, mut raised, mut classes) = (0usize, 0usize, 0usize);

    for class_name in class_db::get_class_list() {
        if class_name == "Node" {
            // `Node` itself is the base; it is swept through its subclasses.
            continue;
        }
        if !class_db::is_parent_class(&class_name, "Node")
            || !class_db::can_instantiate(&class_name)
        {
            continue;
        }
        // The class's OWN registrations, not the inherited ones: that is the
        // key the gap tables use, and it keeps each pair swept once instead of
        // once per subclass.
        let Some(info) = class_db::get_class_info(&class_name) else {
            continue;
        };
        let own: Vec<String> = info
            .methods
            .iter()
            .filter(|m| {
                !m.is_virtual
                    && !m.is_static
                    && m.argument_count == 0
                    && !m.name.starts_with('_')
                    // A void no-op is allow-listed by design, so calling it
                    // proves nothing about dispatch.
                    && !is_void_noop(&m.name)
            })
            .map(|m| m.name.clone())
            .collect();
        if own.is_empty() {
            continue;
        }
        classes += 1;
        for method in own {
            match probe(&class_name, &method) {
                Outcome::Answered => answered += 1,
                Outcome::Raised => {
                    raised += 1;
                    if !is_known_gap(&class_name, &method) {
                        unlisted.push(format!("{class_name}.{method}"));
                    }
                }
            }
        }
    }

    assert!(
        classes > 0 && answered + raised > 0,
        "the sweep found nothing to call — ClassDB is probably empty, which makes \
         this test vacuous (classes={classes}, answered={answered}, raised={raised})"
    );
    // Proves the error path is reachable at all: if nothing raised, either every
    // gap is now wired (delete the tables) or the branch under test is dead.
    assert!(
        raised > 0,
        "nothing raised across {classes} classes, so the registered-but-unimplemented \
         branch is unreachable and this sweep guards nothing"
    );
    unlisted.sort();
    assert!(
        unlisted.is_empty(),
        "{} registered zero-arg method(s) raise through the interpreter but are not \
         recorded in native_method_gaps. Either wire them up or add them to \
         KNOWN_MISSING_NATIVE_METHODS / NON_GODOT_REGISTRATIONS:\n  {}",
        unlisted.len(),
        unlisted.join("\n  ")
    );
}

#[test]
fn the_gap_ledgers_can_only_shrink() {
    // The bound is the measured size at the time the tables were generated.
    // Wiring a method up should shorten a list; a new undispatched registration
    // must not slip in unnoticed.
    assert!(
        KNOWN_MISSING_NATIVE_METHODS.len() <= 246,
        "KNOWN_MISSING_NATIVE_METHODS grew to {} (budget 246). A new registration with \
         no dispatch arm is a regression, not new debt to record.",
        KNOWN_MISSING_NATIVE_METHODS.len()
    );
    assert!(
        NON_GODOT_REGISTRATIONS.len() <= 4,
        "NON_GODOT_REGISTRATIONS grew to {} (budget 4). Registering a method Godot does \
         not have makes has_method() lie in the other direction.",
        NON_GODOT_REGISTRATIONS.len()
    );
}

#[test]
fn every_non_godot_registration_really_is_absent_from_godots_docs() {
    // Re-derives the NON_GODOT_REGISTRATIONS claim from the oracle itself rather
    // than trusting the generator. Read at runtime, so it stays valid without a
    // rebuild — and the red-check snapshot symlinks `upstream/`, so it runs
    // there too.
    //
    // Two details the first version of this check got wrong, and the reason it
    // is worth re-deriving at all:
    //
    // 1. Godot documents module classes under `modules/<name>/doc_classes/`,
    //    not `doc/classes/`. GridMap, MultiplayerSpawner/Synchronizer and the
    //    GDScript classes all live there; searching only `doc/classes` made 34
    //    real Godot methods look like registration bugs.
    // 2. A property accessor is documented as the `getter=`/`setter=` of a
    //    `<member>`, never as a `<method>` — but ClassDB binds it as a callable
    //    method. Matching only `<method name=...>` misclassified every accessor.
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine-rs has a parent")
        .to_path_buf();
    let upstream = repo.join("upstream/godot");
    if !upstream.is_dir() {
        // Shallow checkouts do not vendor Godot. Skipping is the honest
        // outcome; the sweep above does not depend on this check.
        eprintln!("skipping: {} is not present", upstream.display());
        return;
    }

    /// Every `<class>.xml` under `doc/classes` and any `doc_classes`, keyed by
    /// class name.
    fn doc_files(upstream: &std::path::Path) -> std::collections::HashMap<String, String> {
        let mut out = std::collections::HashMap::new();
        let mut stack = vec![upstream.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_doc_dir = name == "classes" || name == "doc_classes";
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Only walk far enough to reach doc/classes and
                    // modules/*/doc_classes; the whole engine tree is huge.
                    stack.push(path);
                } else if is_doc_dir && path.extension().is_some_and(|e| e == "xml") {
                    if let (Some(stem), Ok(text)) = (
                        path.file_stem().and_then(|s| s.to_str()),
                        std::fs::read_to_string(&path),
                    ) {
                        out.insert(stem.to_string(), text);
                    }
                }
            }
        }
        out
    }

    let docs = doc_files(&upstream);
    assert!(
        docs.len() > 900,
        "expected ~1065 Godot doc XML files, found {} — the oracle is incomplete and          this check would pass for the wrong reason",
        docs.len()
    );

    let mut wrong: Vec<String> = Vec::new();
    for entry in NON_GODOT_REGISTRATIONS {
        let (class_name, method) = entry.split_once('.').expect("entries are Class.method");
        let Some(text) = docs.get(class_name) else {
            // No doc file at all means Godot has no such class, a stronger
            // version of the same claim.
            continue;
        };
        let documented = text.contains(&format!("<method name=\"{method}\""))
            || text.contains(&format!("getter=\"{method}\""))
            || text.contains(&format!("setter=\"{method}\""));
        if documented {
            wrong.push((*entry).to_string());
        }
    }
    assert!(
        wrong.is_empty(),
        "these are listed as non-Godot registrations but Godot's own docs declare them \
         (as a method or as a property accessor), so they belong in \
         KNOWN_MISSING_NATIVE_METHODS instead:\n  {}",
        wrong.join("\n  ")
    );
}
