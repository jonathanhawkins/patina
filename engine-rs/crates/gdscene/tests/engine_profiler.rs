//! pat-0q2kx.251 — `EngineProfiler` exists, and a subclass registered with
//! `EngineDebugger` actually receives its virtuals.
//!
//! Godot ground truth: `core/debugger/engine_profiler.{h,cpp}`,
//! `core/register_core_types.cpp:296`, `doc/classes/EngineProfiler.xml`.
//!
//! Before this, `EngineDebugger.register_profiler(name, obj)` parsed and
//! returned Nil but **dropped the object on the floor** — the arm was
//! `entry(name).or_default();`, keeping only an enabled flag and a frame log.
//! So the whole point of the class (a script that reacts to profiling) was
//! unreachable: you could register a profiler and nothing would ever call it.
//!
//! ⚠️ TWO CORRECTIONS TO THE BEAD, both read from the vendored source rather
//! than inferred, and both asserted below so a future edit cannot quietly
//! reintroduce them:
//!   * The bead says EngineProfiler "inherits Object". It inherits
//!     **RefCounted** (`engine_profiler.h:36`, and the doc XML's `inherits=`).
//!   * The bead says it is "abstract to subclass". `register_core_types.cpp`
//!     uses `GDREGISTER_CLASS`, not `GDREGISTER_ABSTRACT_CLASS`, so it is
//!     **instantiable**. Virtual is not abstract.
//!
//! ⚠️ SCOPE: `_toggle` and `_add_frame` are delivered — the two the acceptance
//! names. `_tick` is registered as a virtual but has NO caller: it fires once
//! per engine iteration while profiling, and the debugger has no per-frame
//! pump to fire it from. Registering the method without the frame loop would
//! be wired-but-inert, so it is called out here rather than claimed.
//!
//! ⚠️ The profiler object must be a NODE, because a node is the only thing in
//! this engine that a script can hold as an `ObjectId` — the handle
//! `call_script_method` dispatches on. A real EngineProfiler is a bare
//! RefCounted. That gap is in the object model, not in this dispatch.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A profiler subclass that records what it was handed, plus a driver script on
/// the root that registers it and drives `EngineDebugger`.
fn tree_with_profiler(driver: &str) -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    gdobject::class_db::register_3d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, Node::new("Main", "Node"))
        .expect("main added");
    let prof = tree
        .add_child(main, Node::new("Prof", "Node"))
        .expect("profiler added");

    // Each virtual records separately, so a dispatch that fires the wrong one
    // cannot satisfy the other's assertion.
    let profiler_src = "extends Node\n\
         var toggles = 0\n\
         var enabled = false\n\
         var options_len = -1\n\
         var frames = 0\n\
         var last_frame = []\n\
         func _toggle(enable, options):\n\
         \x20   self.toggles += 1\n\
         \x20   self.enabled = enable\n\
         \x20   self.options_len = options.size()\n\
         func _add_frame(data):\n\
         \x20   self.frames += 1\n\
         \x20   self.last_frame = data\n";
    let p = GDScriptNodeInstance::from_source(profiler_src, prof).expect("profiler parses");
    tree.attach_script(prof, Box::new(p));

    let d = GDScriptNodeInstance::from_source(driver, main).expect("driver parses");
    tree.attach_script(main, Box::new(d));

    (tree, main, prof)
}

/// Reads a script `var`. These live on the SCRIPT instance, not in the node's
/// property store — `get_node(id).get_property(..)` returns Nil for them, which
/// would make every assertion below fail for the wrong reason.
fn prop(tree: &SceneTree, id: gdscene::node::NodeId, name: &str) -> Variant {
    tree.get_script(id)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("the script declares no var `{name}`"))
        .default_value
}

/// Drives the script through the TREE, not through `get_script_mut(..)`.
///
/// Only `SceneTree::call_script_method` builds the `SceneTreeAccessor` and
/// installs it as the interpreter's scene access; calling `call_method` on the
/// script handle directly leaves the interpreter with none, and every
/// `get_node(..)` inside raises `get_node() requires scene access`.
///
/// It also takes the running script OUT of the arena for the duration of the
/// call, which is exactly why the profiler object here is a SEPARATE node from
/// the driver — dispatching a virtual back into the driver's own script would
/// find it checked out.
fn run(tree: &mut SceneTree, id: gdscene::node::NodeId) {
    let out = tree.call_script_method(id, "run", &[]);
    assert!(
        out.is_some(),
        "the driver script must define `run` and must not raise"
    );
}

/// THE ACCEPTANCE. A script class registered via `register_profiler` has
/// `_toggle(true)` called on `profiler_enable` and `_add_frame` called on
/// `profiler_add_frame_data`, and `EngineProfiler` exists in the ClassDB.
#[test]
fn engine_profiler_subclass_receives_toggle_and_frame() {
    gdobject::class_db::register_3d_classes();
    assert!(
        gdobject::class_db::class_exists("EngineProfiler"),
        "the class a custom profiler extends must exist"
    );

    let (mut tree, main, prof) = tree_with_profiler(
        "extends Node\n\
         func run():\n\
         \x20   var p = get_node(\"Prof\")\n\
         \x20   EngineDebugger.register_profiler(\"mine\", p)\n\
         \x20   EngineDebugger.profiler_enable(\"mine\", true)\n\
         \x20   EngineDebugger.profiler_add_frame_data(\"mine\", [7, 8])\n",
    );
    run(&mut tree, main);

    assert_eq!(
        prop(&tree, prof, "toggles"),
        Variant::Int(1),
        "profiler_enable must invoke _toggle exactly once — 0 means the \
         registered object was dropped and nothing was ever dispatched"
    );
    assert_eq!(
        prop(&tree, prof, "enabled"),
        Variant::Bool(true),
        "_toggle receives the enable flag, not a placeholder"
    );
    assert_eq!(
        prop(&tree, prof, "frames"),
        Variant::Int(1),
        "profiler_add_frame_data must invoke _add_frame"
    );
    assert_eq!(
        prop(&tree, prof, "last_frame"),
        Variant::Array(vec![Variant::Int(7), Variant::Int(8)].into()),
        "_add_frame receives the DATA array — dispatching with the profiler \
         name, or an empty array, would still increment the counter above"
    );
}

/// Disabling dispatches too, with `false`. A `_toggle` wired only to the
/// enabling edge would leave a profiler running forever.
#[test]
fn disabling_dispatches_toggle_with_false() {
    let (mut tree, main, prof) = tree_with_profiler(
        "extends Node\n\
         func run():\n\
         \x20   var p = get_node(\"Prof\")\n\
         \x20   EngineDebugger.register_profiler(\"mine\", p)\n\
         \x20   EngineDebugger.profiler_enable(\"mine\", true)\n\
         \x20   EngineDebugger.profiler_enable(\"mine\", false)\n",
    );
    run(&mut tree, main);

    assert_eq!(prop(&tree, prof, "toggles"), Variant::Int(2));
    assert_eq!(
        prop(&tree, prof, "enabled"),
        Variant::Bool(false),
        "the second call must dispatch with false, not repeat the first"
    );
}

/// `profiler_enable(name, enable, arguments)` passes `arguments` through as
/// `_toggle`'s `options`, and an omitted third argument arrives as an EMPTY
/// ARRAY — `_toggle` is typed `(bool, Array)`, so handing it Nil would break a
/// body that calls `options.size()`.
#[test]
fn the_options_array_reaches_toggle() {
    let (mut tree, main, prof) = tree_with_profiler(
        "extends Node\n\
         func run():\n\
         \x20   var p = get_node(\"Prof\")\n\
         \x20   EngineDebugger.register_profiler(\"mine\", p)\n\
         \x20   EngineDebugger.profiler_enable(\"mine\", true)\n",
    );
    run(&mut tree, main);
    assert_eq!(
        prop(&tree, prof, "options_len"),
        Variant::Int(0),
        "an omitted options argument must arrive as [], not Nil"
    );

    let (mut tree2, main2, prof2) = tree_with_profiler(
        "extends Node\n\
         func run():\n\
         \x20   var p = get_node(\"Prof\")\n\
         \x20   EngineDebugger.register_profiler(\"mine\", p)\n\
         \x20   EngineDebugger.profiler_enable(\"mine\", true, [1, 2, 3])\n",
    );
    run(&mut tree2, main2);
    assert_eq!(
        prop(&tree2, prof2, "options_len"),
        Variant::Int(3),
        "the caller's options array must reach _toggle"
    );
}

/// A profiler name with no registered object is not an error. Godot's
/// `profiler_enable` on an unbound name is a no-op, and the pre-existing
/// state-only behaviour (`is_profiling`) must survive the dispatch change.
#[test]
fn an_unregistered_profiler_still_only_tracks_state() {
    let (mut tree, main, prof) = tree_with_profiler(
        "extends Node\n\
         var profiling = false\n\
         func run():\n\
         \x20   EngineDebugger.profiler_enable(\"nobody\", true)\n\
         \x20   EngineDebugger.profiler_add_frame_data(\"nobody\", [1])\n\
         \x20   self.profiling = EngineDebugger.is_profiling(\"nobody\")\n",
    );
    run(&mut tree, main);

    assert_eq!(
        prop(&tree, main, "profiling"),
        Variant::Bool(true),
        "the enabled flag must still be tracked with no object bound"
    );
    assert_eq!(
        prop(&tree, prof, "toggles"),
        Variant::Int(0),
        "a profiler registered under a DIFFERENT name must not be dispatched to"
    );
}

/// The two corrections to the bead's spec, pinned. Registering this under
/// Object would sever it from the ref-counted chain, and registering it
/// abstract would make `EngineProfiler.new()` fail — which would defeat a class
/// whose documented purpose is to be constructed and extended.
#[test]
fn engine_profiler_is_a_concrete_refcounted() {
    gdobject::class_db::register_3d_classes();

    let chain = gdobject::class_db::inheritance_chain("EngineProfiler");
    assert!(
        chain.iter().any(|c| c == "RefCounted"),
        "EngineProfiler inherits RefCounted, NOT Object as the bead states \
         (engine_profiler.h:36) — got {chain:?}"
    );
    assert!(
        !gdobject::class_db::is_abstract("EngineProfiler"),
        "register_core_types.cpp uses GDREGISTER_CLASS, not \
         GDREGISTER_ABSTRACT_CLASS — virtual is not abstract"
    );

    // The three virtuals a subclass may override.
    for m in ["_toggle", "_add_frame", "_tick"] {
        assert!(
            gdobject::class_db::class_has_method("EngineProfiler", m),
            "{m} must be declared so a subclass overriding it is recognised"
        );
    }
}
