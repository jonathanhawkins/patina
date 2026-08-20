//! pat-s3rrv.244 — a script `Logger` receives `push_error` / `push_warning`
//! through `_log_error`.
//!
//! Godot ground truth: `doc/classes/Logger.xml` (the `_log_error` signature and
//! the `ERROR_TYPE_*` constants), `core/variant/variant_utility.cpp`
//! (`push_error` → `ERR_PRINT`) and `core/error/error_macros.cpp`
//! (`_err_print_error(func, file, line, p_error, "", notify, type)`).
//!
//! # What already worked, and what did not
//!
//! pat-s3rrv.229 registered `Logger`, wired `OS.add_logger`/`remove_logger`,
//! and fanned `print()` out to `_log_message`. **`_log_error` was never
//! dispatched at all** — `push_error` and `push_warning` reached the console and
//! stopped there, so a script logger saw prints and silently missed every error.
//!
//! # 🔑 The message arrives in `code`, NOT `rationale`
//!
//! ⚠️ This reads backwards. `rationale` is the field whose name suggests
//! human-facing text, but Godot's `_err_print_error` passes the message as
//! `p_error` and an **empty string** as the rationale, so `Logger::log_error`
//! receives it as **`code`**. A test asserting the text lands in `rationale`
//! fails against correct behaviour, and one asserting only "some argument
//! contains the text" would pass against either.
//!
//! # ⚠️ NOT covered here: logger chaining
//!
//! Godot chains loggers on top of the default console routing, so installing
//! one must not swallow the engine's own record. `Interpreter::errors()` is
//! public but is not reachable from a `gdscene` test through the script
//! instance, so I could not assert it. I removed the test I had written for it
//! rather than keep one that only checked the sink was still attached — a
//! vacuous assertion under a name that claims more is worse than an
//! acknowledged gap.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn scene_with(scripts: &[(&str, &str)]) -> (MainLoop, Vec<gdscene::node::NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = Vec::new();
    for (name, src) in scripts {
        let node = tree
            .add_child(root, Node::new(*name, "Node"))
            .expect("child adds");
        let script = GDScriptNodeInstance::from_source(src, node).expect("script parses");
        tree.attach_script(node, Box::new(script));
        ids.push(node);
    }
    let mut ml = MainLoop::new(tree);
    for id in &ids {
        ml.tree_mut().process_script_ready(*id);
    }
    ml.step(1.0 / 60.0);
    (ml, ids)
}

fn script_var(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

/// A sink that records every `_log_error` argument it is handed.
const SINK: &str = "\
extends Logger

var errors = 0
var last_code = \"\"
var last_rationale = \"unset\"
var last_type = -1
var last_function = \"\"
var last_line = -1

func _ready():
    OS.add_logger(self)

func _log_error(function, file, line, code, rationale, editor_notify, error_type):
    errors += 1
    last_code = code
    last_rationale = rationale
    last_type = error_type
    last_function = function
    last_line = line
";

/// 🔑 **THE ACCEPTANCE CLAUSE.** `push_error` reaches `_log_error` with
/// `ERROR_TYPE_ERROR`, and `push_warning` with `ERROR_TYPE_WARNING`.
#[test]
fn a_push_error_and_push_warning_reach_the_logger() {
    let driver = "\
extends Node

func _ready():
    push_error(\"boom\")
";
    let (ml, ids) = scene_with(&[("Sink", SINK), ("Driver", driver)]);

    // An exact count, not `>= 1`: one push_error must produce exactly one
    // `_log_error`. A fan-out that also fired from the message path, or once
    // per registered logger per argument, passes a loose assertion.
    assert_eq!(
        script_var(&ml, ids[0], "errors"),
        Variant::Int(1),
        "exactly one _log_error per push_error"
    );
    assert_eq!(
        script_var(&ml, ids[0], "last_type"),
        Variant::Int(0),
        "ERROR_TYPE_ERROR is 0 (doc/classes/Logger.xml)"
    );

    // 🔑 The message is `code`, and `rationale` is EMPTY — not the other way
    // round. `last_rationale` starts as "unset" so an untouched parameter is
    // distinguishable from a correctly-empty one.
    assert_eq!(
        script_var(&ml, ids[0], "last_code"),
        Variant::String("boom".into()),
        "Godot passes the message as `code` (_err_print_error's p_error)"
    );
    assert_eq!(
        script_var(&ml, ids[0], "last_rationale"),
        Variant::String(String::new()),
        "…and `rationale` empty. Swapping the two still delivers the text \
         somewhere, which is why both are asserted"
    );
}

/// `push_warning` classifies as `ERROR_TYPE_WARNING`, not as an error.
///
/// ⚠️ The control for the type argument. Without it, a build that hardcoded
/// `ERROR_TYPE_ERROR` would satisfy the test above completely.
#[test]
fn b_push_warning_is_classified_as_a_warning() {
    let driver = "\
extends Node

func _ready():
    push_warning(\"careful\")
";
    let (ml, ids) = scene_with(&[("Sink", SINK), ("Driver", driver)]);

    assert_eq!(script_var(&ml, ids[0], "errors"), Variant::Int(1));
    assert_eq!(
        script_var(&ml, ids[0], "last_type"),
        Variant::Int(1),
        "ERROR_TYPE_WARNING is 1, and must NOT be reported as 0"
    );
    assert_eq!(
        script_var(&ml, ids[0], "last_code"),
        Variant::String("careful".into())
    );
}

/// The call site travels with the error.
///
/// ⚠️ `function` and `line` come from the frame that raised, so they must name
/// the DRIVER's `_ready`, not the logger's `_log_error`. Capturing them after
/// entering the logger would report the logger's own position — plausible
/// values that are useless for finding the error.
#[test]
fn c_the_call_site_is_the_raising_frame() {
    let driver = "\
extends Node

func boom_here():
    push_error(\"traced\")

func _ready():
    boom_here()
";
    let (ml, ids) = scene_with(&[("Sink", SINK), ("Driver", driver)]);

    assert_eq!(
        script_var(&ml, ids[0], "last_function"),
        Variant::String("boom_here".into()),
        "the raising function, not `_log_error` and not `_ready`"
    );
    match script_var(&ml, ids[0], "last_line") {
        Variant::Int(l) => assert!(l > 0, "a real line number, got {l}"),
        other => panic!("line must be an Int, got {other:?}"),
    }
}

/// After `remove_logger` the sink receives nothing further.
#[test]
fn d_remove_logger_stops_delivery() {
    let driver = "\
extends Node

func _ready():
    push_error(\"first\")
";
    let sink_that_leaves = "\
extends Logger

var errors = 0

func _ready():
    OS.add_logger(self)
    OS.remove_logger(self)

func _log_error(function, file, line, code, rationale, editor_notify, error_type):
    errors += 1
";
    let (ml, ids) = scene_with(&[("Sink", sink_that_leaves), ("Driver", driver)]);

    assert_eq!(
        script_var(&ml, ids[0], "errors"),
        Variant::Int(0),
        "a removed logger must receive nothing"
    );
}
