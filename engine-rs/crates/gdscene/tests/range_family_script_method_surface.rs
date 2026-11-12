//! Acceptance test for pat-s3rrv.73: a GDScript can drive a Range control.
//!
//! Godot ground truth: `scene/gui/range.cpp` — `set_value` clamps to
//! `[min, max - page]`, snaps to `step` and emits `value_changed` only on a real
//! change; `set_min`/`set_max` end with `set_value(shared->val)` (range.cpp:217,
//! :234) so moving a bound drags the value with it; `set_value_no_signal`
//! (Range.xml) moves the value without notifying.
//!
//! ⚠️ Before this, a script could not move a slider **at all** — and not because
//! the binding was inert. `SceneTree::node_has_method` defers to
//! `class_db::class_has_method`, which consults declared `MethodInfo` only, and
//! `Range` registers its value model as PROPERTIES. So `set_value` did not
//! resolve; it was not a method that existed and did nothing, it was a method
//! that was not there.
//!
//! ⚠️ Both script spellings are exercised, on purpose. They travel different
//! paths and a binding wired to only one is invisible from the other:
//! - `get_node("Bar").set_value(..)` → `interpreter.rs::call_method_on`, which
//!   falls back to `SceneAccess::call_node_method`;
//! - a **bare** `set_value(..)` in a script on the node itself →
//!   `call_self_func_or_native` → the same `call_node_method`.
//!
//! The bare form is what a script attached to the control writes, and it is the
//! one that historically goes unwired.

use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdscene::control;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct ValueLog(Arc<Mutex<Vec<f64>>>);

impl ValueLog {
    fn values(&self) -> Vec<f64> {
        self.0.lock().unwrap().clone()
    }
}

fn watch(tree: &mut SceneTree, node: NodeId, tag: u64) -> ValueLog {
    let log = ValueLog::default();
    let sink = log.clone();
    tree.connect_signal(
        node,
        "value_changed",
        Connection::with_callback(ObjectId::from_raw(tag), "on_changed", move |args| {
            let v = match args.first() {
                Some(Variant::Float(f)) => *f,
                Some(Variant::Int(i)) => *i as f64,
                other => panic!("value_changed must carry a number, got {other:?}"),
            };
            sink.0.lock().unwrap().push(v);
            Variant::Nil
        }),
    );
    log
}

/// A tree whose root holds one `HSlider` named `Bar`, 0..100 step 1, value 0.
fn slider_tree() -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut bar = Node::new("Bar", "HSlider");
    bar.set_property("size", Variant::Vector2(Vector2::new(110.0, 16.0)));
    for (k, v) in [
        ("min_value", 0.0),
        ("max_value", 100.0),
        ("step", 1.0),
        ("value", 0.0),
    ] {
        bar.set_property(k, Variant::Float(v));
    }
    let bar = tree.add_child(root, bar).expect("add slider");
    (tree, root, bar)
}

fn attach(tree: &mut SceneTree, node: NodeId, src: &str) {
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("script parses")),
    );
}

/// Runs `run()` and asserts the call did not raise.
///
/// `call_script_method` returns `None` when the script raised, so this is what
/// separates "the method ran" from "`UndefinedFunction` aborted the handler at
/// its first statement" — which is how an unwired binding actually presents.
fn run(tree: &mut SceneTree, node: NodeId) {
    assert!(
        tree.call_script_method(node, "run", &[]).is_some(),
        "run() raised — an unresolved method name aborts the handler at that \
         statement, so every later line silently does not execute"
    );
}

// ---------------------------------------------------------------------------
// The two spellings
// ---------------------------------------------------------------------------

/// A **bare** `set_value(50)` from a script on the slider moves it and notifies.
#[test]
fn a_bare_set_value_call_moves_the_slider_and_emits() {
    let (mut tree, _root, bar) = slider_tree();
    let log = watch(&mut tree, bar, 1);
    attach(
        &mut tree,
        bar,
        "extends HSlider\nfunc run():\n    set_value(50)\n",
    );

    run(&mut tree, bar);

    assert_eq!(
        control::get_range_value(&tree, bar),
        50.0,
        "the bare spelling is what a script attached to the control writes, and \
         it must reach the value model"
    );
    assert_eq!(log.values(), vec![50.0], "and notify exactly once");
}

/// `get_node("Bar").set_value(75)` from a parent script does the same.
///
/// Different resolution path from the test above — this one goes through the
/// interpreter's builtin match and its fallback into the scene bridge.
#[test]
fn an_object_spelling_set_value_call_also_reaches_the_value_model() {
    let (mut tree, root, bar) = slider_tree();
    let log = watch(&mut tree, bar, 2);
    attach(
        &mut tree,
        root,
        "extends Node\nfunc run():\n    get_node(\"Bar\").set_value(75)\n",
    );

    run(&mut tree, root);

    assert_eq!(control::get_range_value(&tree, bar), 75.0);
    assert_eq!(log.values(), vec![75.0]);
}

// ---------------------------------------------------------------------------
// Semantics
// ---------------------------------------------------------------------------

/// `set_value_no_signal` moves the value and stays silent.
///
/// This is the documented way to sync a control TO a value it just reported,
/// without the handler that produced it hearing its own echo — so the silence
/// is the whole feature, not an optimisation.
#[test]
fn set_value_no_signal_moves_the_value_without_notifying() {
    let (mut tree, _root, bar) = slider_tree();
    let log = watch(&mut tree, bar, 3);
    attach(
        &mut tree,
        bar,
        "extends HSlider\nfunc run():\n    set_value_no_signal(30)\n",
    );

    run(&mut tree, bar);

    assert_eq!(
        control::get_range_value(&tree, bar),
        30.0,
        "the value must still move"
    );
    assert!(
        log.values().is_empty(),
        "but nothing may be emitted — got {:?}",
        log.values()
    );
}

/// Raising `min` above the current value drags the value up with it.
///
/// ⚠️ Godot's `set_min` ends with `set_value(shared->val)` (range.cpp:217)
/// precisely for this. Without the re-clamp a control keeps a value outside its
/// own range, and every ratio derived from it — the drawn grabber included — is
/// out of bounds while every individual getter still looks reasonable.
#[test]
fn moving_a_bound_reclamps_the_value() {
    let (mut tree, _root, bar) = slider_tree();
    attach(
        &mut tree,
        bar,
        "extends HSlider\nfunc run():\n    set_value(20)\n    set_min(60)\n",
    );
    let log = watch(&mut tree, bar, 4);

    run(&mut tree, bar);

    assert_eq!(
        control::get_range_min(&tree, bar),
        60.0,
        "the floor must move"
    );
    assert_eq!(
        control::get_range_value(&tree, bar),
        60.0,
        "and drag the value up with it, rather than leaving 20 below the floor"
    );
    assert_eq!(
        log.values(),
        vec![20.0, 60.0],
        "both moves are real changes, so both notify"
    );
}

/// `page` narrows the effective maximum to `max - page`, and the current value
/// is re-clamped into it.
#[test]
fn setting_a_page_reclamps_against_the_reduced_maximum() {
    let (mut tree, _root, bar) = slider_tree();
    attach(
        &mut tree,
        bar,
        "extends HSlider\nfunc run():\n    set_value(100)\n    set_page(25)\n",
    );

    run(&mut tree, bar);

    assert_eq!(
        control::get_range_value(&tree, bar),
        75.0,
        "with page 25 the effective maximum is max - page = 75"
    );
}

/// The ratio accessors round-trip through the value model.
#[test]
fn the_ratio_accessors_agree_with_the_value() {
    let (mut tree, _root, bar) = slider_tree();
    attach(
        &mut tree,
        bar,
        "extends HSlider\nvar r = -1.0\nfunc run():\n    set_as_ratio(0.4)\n    self.r = get_as_ratio()\n",
    );

    run(&mut tree, bar);

    assert_eq!(
        control::get_range_value(&tree, bar),
        40.0,
        "ratio 0.4 of 0..100 is 40"
    );
    assert!(
        (control::get_range_ratio(&tree, bar) - 0.4).abs() < 1e-6,
        "and reading the ratio back returns what was set, got {}",
        control::get_range_ratio(&tree, bar)
    );
}

/// A control outside the family is untouched, which is what keeps this arm from
/// shadowing the generic node-method table.
#[test]
fn a_non_range_node_does_not_answer_the_range_methods() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = tree
        .add_child(root, Node::new("P", "Panel"))
        .expect("add panel");

    assert_eq!(
        gdscene::range_controls::call_range_method(
            &mut tree,
            panel,
            "set_value",
            &[Variant::Float(5.0)]
        ),
        None,
        "a Panel has no value model; answering here would shadow the generic \
         table below it in the dispatch chain"
    );
}
