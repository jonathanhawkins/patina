//! pat-s3rrv.19 — AStar2D / AStarGrid2D must be reachable from a game script.
//!
//! `gdscene::astar` has carried a complete, unit-tested pathfinder since
//! pat-p0s82 (weighted points, bidirectional links, partial paths, grid
//! diagonal modes, jump-point compression) and ClassDB registers all three
//! classes — but **nothing ever called it**. `grep -rn "astar::"` matched only
//! the `pub mod` line, and the interpreter's `.new()` chain had no arm, so
//! `AStar2D.new()` raised a TypeError. A complete engine, unreachable from
//! every game script, behind a green unit-test suite.
//!
//! **Why this test lives in `gdscene` and not `gdscript-interop`**, which is
//! where the bead's `runnable via` line puts it: the pathfinder is a `gdscene`
//! type, and `gdscene` depends on `gdscript-interop`, not the reverse
//! (gdscene/Cargo.toml:14). So the interpreter cannot own an `AStar` — it
//! reaches one through the `SceneAccess` seam, exactly as it does for
//! `animation_tree_playback_call`. A test in `gdscript-interop` could only
//! supply a *mock* `SceneAccess`, which would have to reimplement AStar itself
//! — proving the mock works while `gdscene::astar` stayed as dead as it started.
//! Driving the real `SceneTreeAccessor` here is the only way the assertions
//! below touch the actual pathfinder. The bead's own acceptance text agrees:
//! it asks for "a GDScript **in a loaded scene**".

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reads a script `var` off the attached instance. Node properties and script
/// vars are separate stores, so `get_property` would answer `Nil` here and fail
/// for the wrong reason.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

fn int_var(tree: &SceneTree, node: NodeId, name: &str) -> i64 {
    match script_var(tree, node, name) {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => panic!("`{name}` is not numeric: {other:?}"),
    }
}

fn float_var(tree: &SceneTree, node: NodeId, name: &str) -> f64 {
    match script_var(tree, node, name) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("`{name}` is not numeric: {other:?}"),
    }
}

/// Attaches `src` to a fresh tree and runs its `_ready()` against the real
/// `SceneTreeAccessor`, then advances one frame.
///
/// `process_script_ready` is explicit: `MainLoop::step` alone does not fire
/// `_ready` for a script attached after construction, so without it every
/// assertion below would read its initialiser and fail for the wrong reason.
fn run_script(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Pathfinder", "Node"))
        .expect("child adds");
    let script = GDScriptNodeInstance::from_source(src, node).expect("script parses");
    tree.attach_script(node, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(node);
    ml.step(1.0 / 60.0);
    (ml, node)
}

// ---------------------------------------------------------------------------
// The bead's acceptance
// ---------------------------------------------------------------------------

/// **The acceptance test.** A script constructs `AStar2D.new()`, adds and
/// connects weighted points, and `get_point_path()` hands back the real route;
/// then `AStarGrid2D.new()` sets a region, marks solid cells, and
/// `get_id_path()` routes *around* them. Both round-trip through the
/// interpreter with no `UndefinedFunction` / `TypeError`.
#[test]
fn astar_classes_reachable_from_gdscript() {
    // A 3-point chain 1 -> 2 -> 3 laid out along +X, so the returned path is
    // checkable by coordinate and not just by length.
    //
    // The grid is 5x5 with a wall at column 1 spanning rows 0..2, and diagonals
    // disabled (DiagonalMode::Never = 1). A straight run (0,0) -> (2,0) is
    // blocked, so a correct search must detour down and back up — strictly more
    // than the 3 cells the unobstructed route would take.
    let src = "extends Node\n\
               var path_len: int = -1\n\
               var first_x: float = -1.0\n\
               var last_x: float = -1.0\n\
               var grid_len: int = -1\n\
               var solid_marked: bool = false\n\
               var stage: int = 0\n\
               func _ready():\n\
               \x20   stage = 1\n\
               \x20   var a = AStar2D.new()\n\
               \x20   stage = 2\n\
               \x20   var p1 = Vector2(0, 0)\n\
               \x20   a.add_point(1, p1, 1.0)\n\
               \x20   stage = 3\n\
               \x20   var p2 = Vector2(10, 0)\n\
               \x20   a.add_point(2, p2, 1.0)\n\
               \x20   stage = 4\n\
               \x20   var p3 = Vector2(20, 0)\n\
               \x20   a.add_point(3, p3, 1.0)\n\
               \x20   stage = 5\n\
               \x20   a.connect_points(1, 2, true)\n\
               \x20   stage = 6\n\
               \x20   a.connect_points(2, 3, true)\n\
               \x20   stage = 7\n\
               \x20   var p = a.get_point_path(1, 3)\n\
               \x20   stage = 8\n\
               \x20   path_len = p.size()\n\
               \x20   stage = 9\n\
               \x20   first_x = p[0].x\n\
               \x20   stage = 10\n\
               \x20   last_x = p[2].x\n\
               \x20   stage = 11\n\
               \x20   var g = AStarGrid2D.new()\n\
               \x20   stage = 12\n\
               \x20   var r0 = Vector2i(0, 0)\n\
               \x20   var rs = Vector2i(5, 5)\n\
               \x20   g.set_region(r0, rs)\n\
               \x20   stage = 13\n\
               \x20   g.update()\n\
               \x20   stage = 14\n\
               \x20   g.set_diagonal_mode(1)\n\
               \x20   stage = 15\n\
               \x20   var s0 = Vector2i(1, 0)\n\
               \x20   g.set_point_solid(s0, true)\n\
               \x20   stage = 16\n\
               \x20   var s1 = Vector2i(1, 1)\n\
               \x20   g.set_point_solid(s1, true)\n\
               \x20   stage = 17\n\
               \x20   solid_marked = g.is_point_solid(s0)\n\
               \x20   stage = 18\n\
               \x20   var gz = Vector2i(2, 0)\n\
               \x20   var q = g.get_id_path(r0, gz)\n\
               \x20   stage = 19\n\
               \x20   grid_len = q.size()\n\
               \x20   stage = 20\n";
    let (mut ml, node) = run_script(src);

    // `stage 0` with the bump as statement 1 means `_ready` never executed at
    // all. Distinguish "the body was mangled at parse time" from "it ran and
    // threw": Patina's parser can produce garbage statements rather than an
    // error (a mis-parsed construct leaves tokens unconsumed), in which case
    // `_ready` may not survive as a method at all.
    // Run `_ready` through the tree path, which installs `SceneTreeAccessor`.
    // Calling `ScriptInstance::call_method` directly does NOT: it leaves
    // `scene_access` as `None`, so the `.new()` arm cannot fire and the script
    // dies with `undefined variable: 'AStar2D'` — an artefact of the harness,
    // not the bug. `call_script_method` returns `None` when the call raised
    // (scene_tree.rs:3381), which with `has_method` already asserted below is
    // an unambiguous "it raised".
    let ran = ml.tree_mut().call_script_method(node, "_ready", &[]);
    assert!(
        ran.is_some(),
        "_ready raised even with SceneTreeAccessor installed — the fault is in \
         the AStar call path, not the harness"
    );

    let script = ml.tree().get_script(node).expect("script attached");
    assert!(
        script.has_method("_ready"),
        "the script parsed but produced no `_ready` method — the body was \
         mangled at parse time, so no AStar call ever ran. Methods present: {:?}",
        script
            .list_methods()
            .iter()
            .map(|m| m.name.clone())
            .collect::<Vec<_>>()
    );

    assert_eq!(
        int_var(ml.tree(), node, "stage"),
        20,
        "_ready aborted early. stage 0 = _ready never ran at all (harness \
         problem, not AStar); 1 = AStar2D.new() threw; thereafter stage N means \
         statement N-1 completed, in order: add_point x3/connect x2/\
         get_point_path/size/[0].x/[2].x/grid new/set_region/update/\
         set_diagonal_mode/set_point_solid x2/is_point_solid/get_id_path/size"
    );

    // --- AStar2D: the real graph, not a stub ---
    assert_eq!(
        int_var(ml.tree(), node, "path_len"),
        3,
        "the 1->2->3 chain is a 3-point path; -1 means `_ready` never got past \
         `AStar2D.new()`"
    );
    assert_eq!(
        float_var(ml.tree(), node, "first_x"),
        0.0,
        "path starts at point 1"
    );
    assert_eq!(
        float_var(ml.tree(), node, "last_x"),
        20.0,
        "path ends at point 3"
    );

    // --- AStarGrid2D: solid cells are honoured and routed around ---
    assert!(
        script_var(ml.tree(), node, "solid_marked") == Variant::Bool(true),
        "set_point_solid must be observable through is_point_solid"
    );
    let grid_len = int_var(ml.tree(), node, "grid_len");
    assert!(
        grid_len > 3,
        "with column 1 walled off and diagonals disabled the route must detour \
         (>3 cells); got {grid_len} — 3 would mean the wall was ignored, -1 that \
         AStarGrid2D never constructed"
    );
}

/// The constructors are the part that did not exist: before this, `.new()` fell
/// through to the unknown-class error. Asserting a non-`Nil` object separately
/// keeps a future regression there from being reported as a pathfinding bug.
#[test]
fn astar_classes_are_constructible() {
    let src = "extends Node\n\
               var made_2d: bool = false\n\
               var made_3d: bool = false\n\
               var made_grid: bool = false\n\
               func _ready():\n\
               \x20   made_2d = AStar2D.new() != null\n\
               \x20   made_3d = AStar3D.new() != null\n\
               \x20   made_grid = AStarGrid2D.new() != null\n";
    let (ml, node) = run_script(src);
    for v in ["made_2d", "made_3d", "made_grid"] {
        assert_eq!(
            script_var(ml.tree(), node, v),
            Variant::Bool(true),
            "`{v}`: the .new() arm is missing for this class"
        );
    }
}

/// `AStar3D` shares the graph code but not the vector width — the dispatcher
/// must hand back `Vector3` positions rather than silently truncating.
#[test]
fn astar3d_round_trips_vector3_positions() {
    // `stage` advances past each step so a failure names the statement that
    // aborted `_ready` rather than only the variable it never reached — an
    // interpreter error kills the handler silently, so without this every
    // failure looks identical.
    let src = "extends Node\n\
               var stage: int = 0\n\
               var z: float = -1.0\n\
               var count: int = -1\n\
               func _ready():\n\
               \x20   stage = 1\n\
               \x20   var a = AStar3D.new()\n\
               \x20   stage = 2\n\
               \x20   var p7 = Vector3(1, 2, 3)\n\
               \x20   a.add_point(7, p7, 1.0)\n\
               \x20   stage = 3\n\
               \x20   count = a.get_point_count()\n\
               \x20   stage = 4\n\
               \x20   z = a.get_point_position(7).z\n\
               \x20   stage = 5\n";
    let (ml, node) = run_script(src);
    assert_eq!(
        int_var(ml.tree(), node, "stage"),
        5,
        "_ready aborted early: 0=never ran, 1=AStar3D.new() failed, \
         2=add_point failed, 3=get_point_count failed, 4=get_point_position failed"
    );
    assert_eq!(int_var(ml.tree(), node, "count"), 1);
    assert_eq!(
        float_var(ml.tree(), node, "z"),
        3.0,
        "a Vector3 position must survive the Variant round trip"
    );
}

// ---------------------------------------------------------------------------
// Bisect: which statement raises?
// ---------------------------------------------------------------------------
//
// `stage` counters were useless here: `call_script_method` discards the script
// variable writes when the call raises, so every var reads its initialiser no
// matter how far `_ready` actually got. Progress must therefore be measured
// from OUTSIDE the script — one probe per prefix, each asserting only that the
// call did not raise. With `--no-fail-fast` a single run names the first
// prefix that fails, and that prefix's last line is the culprit.

/// Runs `stmts` as the body of `_ready` and reports whether the call completed
/// without raising.
///
/// Takes statements as a slice and applies the indent **in Rust**, not inside
/// the string literal: a `\`-continuation strips the leading whitespace of the
/// next line, which silently un-indents GDScript and produces a parse failure
/// that looks like a feature bug. That trap has cost this bead two cycles
/// already.
///
/// Deliberately ignores script variables — `call_script_method` discards their
/// writes when the call raises, so they read their initialisers regardless of
/// how far execution actually got.
fn ready_survives(stmts: &[&str]) -> bool {
    let body: String = stmts.iter().map(|s| format!("    {s}\n")).collect();
    let src = format!("extends Node\nfunc _ready():\n{body}");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Probe", "Node"))
        .expect("child adds");
    let script = GDScriptNodeInstance::from_source(&src, node).expect("script parses");
    tree.attach_script(node, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut()
        .call_script_method(node, "_ready", &[])
        .is_some()
}

const MK2D: &str = "var a = AStar2D.new()";
const P0: &str = "var p = Vector2(0, 0)";
const ADD1: &str = "a.add_point(1, p, 1.0)";
const Q0: &str = "var q = Vector2(10, 0)";
const ADD2: &str = "a.add_point(2, q, 1.0)";
const CONN: &str = "a.connect_points(1, 2, true)";
const MKGRID: &str = "var g = AStarGrid2D.new()";
const O0: &str = "var o = Vector2i(0, 0)";
const Z5: &str = "var z = Vector2i(5, 5)";
const REGION: &str = "g.set_region(o, z)";
const UPDATE: &str = "g.update()";

#[test]
fn probe_1_construct_astar2d() {
    assert!(ready_survives(&[MK2D]));
}

#[test]
fn probe_2_add_point() {
    assert!(ready_survives(&[MK2D, P0, ADD1]));
}

#[test]
fn probe_3_connect_points() {
    assert!(ready_survives(&[MK2D, P0, ADD1, Q0, ADD2, CONN]));
}

#[test]
fn probe_4_get_point_path() {
    assert!(ready_survives(&[
        MK2D,
        P0,
        ADD1,
        Q0,
        ADD2,
        CONN,
        "var r = a.get_point_path(1, 2)",
    ]));
}

#[test]
fn probe_5_grid_construct_and_region() {
    assert!(ready_survives(&[MKGRID, O0, Z5, REGION, UPDATE]));
}

#[test]
fn probe_6_grid_solid_and_path() {
    assert!(ready_survives(&[
        MKGRID,
        O0,
        Z5,
        REGION,
        UPDATE,
        "var c = Vector2i(1, 0)",
        "g.set_point_solid(c, true)",
        "var t = Vector2i(2, 0)",
        "var path = g.get_id_path(o, t)",
    ]));
}
