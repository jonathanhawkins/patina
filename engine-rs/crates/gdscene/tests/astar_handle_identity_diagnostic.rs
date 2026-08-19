//! pat-s3rrv.19 — what identity does `AStar2D.new()` actually hand back?
//!
//! The bead is down to a contradiction. `astar_classes_are_constructible`
//! passes, which proves scene access was installed and the `.new()` arm fired
//! (without access that chain falls through to `eval_expr` and raises
//! `undefined variable: 'AStar2D'`). Yet `probe_1a`, which calls the
//! **zero-argument** `get_point_count()`, fails — so this is not about argument
//! conversion. `astar_call` is missing its own registry entry for a handle it
//! has just created.
//!
//! This test reads the handle back **as the script sees it**. `Variant`'s
//! Display for an ObjectId is `<Object#N>`, so `str(a)` exposes the raw id, and
//! the top byte tells us whether the value carries `ASTAR_HANDLE_TAG`.
//!
//! ⚠️ This only became possible when the interpreter stopped discarding
//! property writes on a raise (same bead, earlier pass). Before that fix every
//! var read its initialiser after `_ready` raised, which is exactly why four
//! passes could not see past this point.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn var_of(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no var `{name}`"))
        .default_value
}

/// The real constant, NOT a mirrored copy (pat-s3rrv.19).
///
/// This test used to hardcode `0xA1`. That is exactly the value that turned out
/// to be the bug — `gdcore::xml_parser::XML_HANDLE_TAG` is the same byte — so a
/// copy here would have kept asserting against the broken tag after the fix.
const ASTAR_TAG: u64 = gdscript_interop::bindings::ASTAR_HANDLE_TAG;

#[test]
fn astar_new_hands_back_a_tagged_astar_handle() {
    // Each step is recorded BEFORE the call that might raise, so the surviving
    // values say exactly how far execution reached.
    let src = "extends Node\n\
               var stage: int = 0\n\
               var handle: String = \"\"\n\
               var count: int = -1\n\
               func _ready():\n\
               \x20   stage = 1\n\
               \x20   var a = AStar2D.new()\n\
               \x20   stage = 2\n\
               \x20   handle = str(a)\n\
               \x20   stage = 3\n\
               \x20   count = a.get_point_count()\n\
               \x20   stage = 4\n";

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Probe", "Node"))
        .expect("child adds");
    let script = GDScriptNodeInstance::from_source(src, node).expect("script parses");
    tree.attach_script(node, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(node);

    let stage = match var_of(ml.tree(), node, "stage") {
        Variant::Int(i) => i,
        other => panic!("stage is not an Int: {other:?}"),
    };
    let handle = match var_of(ml.tree(), node, "handle") {
        Variant::String(s) => s,
        other => panic!("handle is not a String: {other:?}"),
    };

    assert!(
        stage >= 3,
        "`_ready` did not reach the method call. stage={stage}, handle={handle:?} \
         (1 = AStar2D.new() raised, 2 = str(a) raised)",
    );

    // The digits between `#` and `>` are the raw ObjectId.
    let raw: u64 = handle
        .trim_start_matches("<Object#")
        .trim_end_matches('>')
        .parse()
        .unwrap_or_else(|e| panic!("handle {handle:?} is not the expected `<Object#N>` form: {e}"));

    assert_eq!(
        raw & 0xFF00_0000_0000_0000,
        ASTAR_TAG,
        "AStar2D.new() returned handle {raw:#018x} ({handle}), whose tag byte is \
         {:#04x} and not AStar's. The constructor is handing back some \
         OTHER handle family, which is why every AStar method misses the \
         registry while `.new()` itself appears to succeed",
        (raw >> 56) as u8,
    );

    assert_eq!(
        stage, 4,
        "the handle IS tagged as AStar ({raw:#018x}) and yet \
         `get_point_count()` — no arguments, shared dispatch — still failed at \
         stage {stage}. That narrows the fault to the registry lookup inside \
         `SceneTreeAccessor::astar_call`: the entry `astar_new` inserted is not \
         being found again",
    );
}
