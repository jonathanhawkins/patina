//! pat-s3rrv.60 — NodePath methods and `Callable.call_deferred` reach a script.
//!
//! `gdcore::node_path::NodePath` has been implemented and unit-tested all
//! along, but `Variant::NodePath` had **no arm** in the interpreter's
//! `call_method_on`, so `^"Player:position".get_subname(0)` fell through to the
//! "cannot call method on NodePath" catch-all. Real, tested code with no
//! runtime call path — the implementation was never the gap, the dispatch was.
//!
//! `Callable.call_deferred` was likewise absent from the Callable arm, so the
//! standard "do this at the end of the frame" idiom raised.
//!
//! Ground truth: extension_api.json NodePath (11 methods) and Callable (21,
//! including `call_deferred`).

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reads a script variable off the attached instance.
fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

fn as_int(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        other => panic!("expected Int, got {other:?}"),
    }
}

fn as_string(v: Variant) -> String {
    match v {
        Variant::String(s) => s,
        other => panic!("expected String, got {other:?}"),
    }
}

/// Attaches `src` to a one-node scene, fires `_ready`, and steps one frame.
fn run(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);
    // `MainLoop::step` alone does not fire `_ready` for a script attached after
    // construction; without this every assertion reads its initialiser.
    ml.tree_mut().process_script_ready(node);
    // One full frame — the deferred-call phase (`process_deferred_calls`) runs
    // inside `step`, which is what makes the `call_deferred` assertion real.
    ml.step(1.0 / 60.0);
    (ml, node)
}

/// **The acceptance test.**
#[test]
fn nodepath_and_callable_script_dispatch_from_scene() {
    const SRC: &str = "\
extends Node
var name_count = -1
var subname_count = -1
var subnames = \"unset\"
var concat_names = \"unset\"
var first_subname = \"unset\"
var absolute = true
var bumped = 0
func bump():
    self.bumped += 1
func _ready():
    var p = ^\"Sprite:modulate:r\"
    self.name_count = p.get_name_count()
    self.subname_count = p.get_subname_count()
    self.subnames = p.get_concatenated_subnames()
    self.concat_names = p.get_concatenated_names()
    self.first_subname = p.get_subname(0)
    self.absolute = p.is_absolute()
    Callable(self, \"bump\").call_deferred()
";
    let (ml, node) = run(SRC);

    // -- NodePath, the bead's exact assertions -------------------------------
    assert_eq!(
        as_int(script_prop(ml.tree(), node, "name_count")),
        1,
        "`^\"Sprite:modulate:r\"` has ONE name; -1 means the call never \
         dispatched and the initialiser survived",
    );
    assert_eq!(
        as_int(script_prop(ml.tree(), node, "subname_count")),
        2,
        "…and TWO subnames, `modulate` and `r`",
    );
    assert_eq!(
        as_string(script_prop(ml.tree(), node, "subnames")),
        "modulate:r",
        "get_concatenated_subnames joins with ':'",
    );
    // The names side uses '/' — these are node names, not properties.
    assert_eq!(
        as_string(script_prop(ml.tree(), node, "concat_names")),
        "Sprite",
    );
    assert_eq!(
        as_string(script_prop(ml.tree(), node, "first_subname")),
        "modulate",
        "indexed access must answer too, not just the counts",
    );
    assert_eq!(
        script_prop(ml.tree(), node, "absolute"),
        Variant::Bool(false),
        "a path with no leading slash is relative — asserted because the \
         initialiser is `true`, so a no-op dispatch would leave it true",
    );

    // -- Callable.call_deferred ran by the end of the frame ------------------
    assert_eq!(
        as_int(script_prop(ml.tree(), node, "bumped")),
        1,
        "Callable(self, \"bump\").call_deferred() must have executed during the \
         frame's deferred phase — 0 means it was queued but never flushed, or \
         never queued at all",
    );
}

/// `call_deferred` defers: the call has NOT run when the statement returns
/// (pat-s3rrv.60).
///
/// Without this, the acceptance test above would pass just as well against an
/// implementation that called the method immediately — which is the whole
/// behaviour `call_deferred` exists to avoid.
#[test]
fn call_deferred_does_not_run_immediately() {
    const SRC: &str = "\
extends Node
var during_ready = -1
var after_frame = -1
var bumped = 0
func bump():
    self.bumped += 1
func _ready():
    Callable(self, \"bump\").call_deferred()
    self.during_ready = self.bumped
func _process(delta):
    self.after_frame = self.bumped
";
    let (ml, node) = run(SRC);
    assert_eq!(
        as_int(script_prop(ml.tree(), node, "during_ready")),
        0,
        "the deferred call must NOT have run yet at the point the statement \
         returned — a 1 here means it executed immediately",
    );
    assert_eq!(
        as_int(script_prop(ml.tree(), node, "bumped")),
        1,
        "…but it must have run by the end of the frame",
    );
}

/// `slice` and `get_as_property_path` answer from a script (pat-s3rrv.60).
///
/// Asserted through NodePath's own accessors rather than `str()`: `Variant`'s
/// Display wraps a path as `NodePath("…")`, so a string comparison would be
/// testing the formatter as much as the slice.
#[test]
fn nodepath_slice_and_property_path_dispatch() {
    const SRC: &str = "\
extends Node
var sliced_names = \"unset\"
var sliced_subs = \"unset\"
var sliced_abs = true
var prop_names = \"unset\"
var was_absolute = false
func _ready():
    var p = ^\"/root/Main/Player:position:x\"
    self.was_absolute = p.is_absolute()
    var s = p.slice(1, 3)
    self.sliced_names = s.get_concatenated_names()
    self.sliced_subs = s.get_concatenated_subnames()
    self.sliced_abs = s.is_absolute()
    self.prop_names = p.get_as_property_path().get_concatenated_names()
";
    let (ml, node) = run(SRC);

    assert_eq!(
        script_prop(ml.tree(), node, "was_absolute"),
        Variant::Bool(true),
        "the source path starts with '/' — the initialiser is false, so a \
         no-op dispatch would leave it false and fail here",
    );
    // Names are [root, Main, Player]; slice(1, 3) keeps Main/Player.
    assert_eq!(
        as_string(script_prop(ml.tree(), node, "sliced_names")),
        "Main/Player",
    );
    // The slice reaches the END of the names, so the subnames ride along —
    // they belong to the last name.
    assert_eq!(
        as_string(script_prop(ml.tree(), node, "sliced_subs")),
        "position:x",
    );
    // …and it no longer starts at the root, so it is relative.
    assert_eq!(
        script_prop(ml.tree(), node, "sliced_abs"),
        Variant::Bool(false),
        "a slice that skips the root is relative by construction",
    );
    // get_as_property_path turns the SUBNAMES into the names, so they join
    // with '/' from here on.
    assert_eq!(
        as_string(script_prop(ml.tree(), node, "prop_names")),
        "position/x",
    );
}
