//! pat-mgixk.104 — IEEE float division and wrapping int arithmetic, driven
//! from a scene script.
//!
//! # What was wrong
//!
//! Two unrelated defects in the same arithmetic helpers, both of which end a
//! script rather than producing a value:
//!
//! 1. **Division aborted the function instead of yielding infinity.**
//!    `binary_div` returned `RuntimeErrorKind::DivisionByZero` for *every*
//!    combination with a zero divisor, including `Float/Float`, `Int/Float`,
//!    `Float/Int` and `Vector2|Vector3 / scalar`. Godot uses the error-on-zero
//!    `DivNZ` evaluators **only** for `int/int` and the int-vector rows
//!    (`core/variant/variant_op.cpp:353-383`); every float-involving row is a
//!    plain `Div`, so the division happens in IEEE and `1.0 / 0` is `inf`.
//!
//! 2. **Integer overflow panicked.** `binary_add`/`sub`/`mul` used `a + b` on
//!    `i64`, and `UnaryOp::Neg` used `-i`. The workspace keeps
//!    `overflow-checks` on in `[profile.dev]`, so a GDScript hash loop
//!    (`h = h * 31 + c`) **panicked the interpreter** rather than wrapping the
//!    way C++ `int64_t` does.
//!
//! # Why the oracle is a "did the body finish" flag, not just the values
//!
//! 🔑 Both bugs *abort or crash the calling function*, so the statements after
//! the first bad expression never run. A test that only asserted on the
//! numbers could not distinguish "the engine computed the wrong number" from
//! "`_ready` stopped at line 1 and every property is still its declared
//! initialiser" — and the latter is exactly what [[propinfo_is_metadata]]-style
//! declared defaults look like. So `completed` is set as the **last** statement
//! and asserted first: it is the only thing that proves the whole body ran.
//!
//! The int/int case is the inverse — it must *still* abort — so it gets its own
//! node, with a flag set **before** the division and another **after**. Both
//! are needed: the "after" flag alone would also be false if `_ready` had never
//! been called at all.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Every value the acceptance criteria name, computed in one `_ready` body.
///
/// `completed` is deliberately the final statement — see the module comment.
const NUMERIC_SCRIPT: &str = "\
extends Node
var inf_pos = 0.0
var inf_neg = 0.0
var nan_flag = false
var vec_inf = Vector2(0, 0)
var int_max_wrap = 0
var int_min_neg = 0
var completed = false
func _ready():
\tself.inf_pos = 1.0 / 0
\tself.inf_neg = -1 / 0.0
\tself.nan_flag = is_nan(0.0 / 0.0)
\tself.vec_inf = Vector2(1, 1) / 0.0
\tself.int_max_wrap = 9223372036854775807 + 1
\tself.int_min_neg = -(-9223372036854775807 - 1)
\tself.completed = true
";

/// `int / int` by zero must KEEP raising a script error — Godot's `DivNZ`
/// (`variant_op.cpp:353`). Separate node so the abort cannot truncate the
/// script above.
const INT_DIV_ZERO_SCRIPT: &str = "\
extends Node
var reached = false
var survived = false
func _ready():
\tself.reached = true
\tvar x = 1 / 0
\tself.survived = true
";

fn script_var(ml: &MainLoop, node: NodeId, prop: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("node has a script")
        // `get_property`, never `list_properties().default_value`: the latter
        // is the parse-time initialiser and never changes, which would make
        // every assertion below read its own fixture.
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

fn float_of(v: Variant, what: &str) -> f64 {
    match v {
        Variant::Float(f) => f,
        other => panic!("{what} should be a Float, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------

/// **The acceptance test.** A scene script divides by zero in every
/// float-involving combination and overflows `i64` in both directions, without
/// aborting; `int / 0` still errors.
#[test]
fn variant_numeric_edge_semantics_test() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node")).unwrap();
    let int_div = tree
        .add_child(world, Node::new("IntDivZero", "Node"))
        .unwrap();
    tree.attach_script(
        world,
        Box::new(GDScriptNodeInstance::from_source(NUMERIC_SCRIPT, world).expect("script parses")),
    );
    tree.attach_script(
        int_div,
        Box::new(
            GDScriptNodeInstance::from_source(INT_DIV_ZERO_SCRIPT, int_div).expect("script parses"),
        ),
    );

    let mut ml = MainLoop::new(tree);
    // `enter_tree` walks the whole subtree, so both scripts get `_ready`.
    ml.enter_tree_with_scripts(world);
    ml.step(1.0 / 60.0);

    // -- (0) the body ran to the end ----------------------------------------
    // Asserted FIRST because it is the precondition for reading any value
    // below: a `DivisionByZero` on line 1 aborts `_ready`, leaving every
    // property at its declared initialiser.
    assert_eq!(
        script_var(&ml, world, "completed"),
        Variant::Bool(true),
        "`_ready` must run to completion. False means an arithmetic expression \
         raised a script error (or panicked) and aborted the function — which \
         is the whole defect: Godot only errors on int/int and int-vector \
         division (`variant_op.cpp:353-383`), and `int64_t` arithmetic wraps \
         rather than trapping. Every assertion below is meaningless until this \
         one holds"
    );

    // -- (1) IEEE division ---------------------------------------------------
    let inf_pos = float_of(script_var(&ml, world, "inf_pos"), "inf_pos");
    assert!(
        inf_pos.is_infinite() && inf_pos.is_sign_positive(),
        "`1.0 / 0` must be +inf (Float/Int is a plain `Div` row, \
         variant_op.cpp:354); got {inf_pos}"
    );
    let inf_neg = float_of(script_var(&ml, world, "inf_neg"), "inf_neg");
    assert!(
        inf_neg.is_infinite() && inf_neg.is_sign_negative(),
        "`-1 / 0.0` must be -inf, not +inf: the sign of the zero divisor's \
         numerator has to survive. Got {inf_neg}"
    );
    assert_eq!(
        script_var(&ml, world, "nan_flag"),
        Variant::Bool(true),
        "`is_nan(0.0 / 0.0)` must be true — 0/0 is NaN, not inf and not an \
         error. A false here with `completed` true would mean the division \
         produced a finite number"
    );
    match script_var(&ml, world, "vec_inf") {
        Variant::Vector2(v) => assert!(
            v.x.is_infinite()
                && v.x.is_sign_positive()
                && v.y.is_infinite()
                && v.y.is_sign_positive(),
            "`Vector2(1, 1) / 0.0` must be Vector2(inf, inf) — Vector2/Float is \
             a plain `Div` row (variant_op.cpp:359), so BOTH components divide \
             in IEEE. Got ({}, {})",
            v.x,
            v.y
        ),
        other => panic!("`vec_inf` should be a Vector2, got {other:?}"),
    }

    // -- (2) wrapping integer arithmetic -------------------------------------
    // `i64::MIN`, not the literal `-9223372036854775808`: in Rust that lexes
    // as negation of an out-of-range `i64`, so it does not compile.
    assert_eq!(
        script_var(&ml, world, "int_max_wrap"),
        Variant::Int(i64::MIN),
        "`9223372036854775807 + 1` must WRAP to i64::MIN (-9223372036854775808) \
         like C++ `int64_t`, \
         not panic. The workspace keeps `overflow-checks` on in \
         `[profile.dev]`, so plain `a + b` aborts the whole interpreter here — \
         a GDScript hash loop `h = h * 31 + c` is enough to hit it"
    );
    assert_eq!(
        script_var(&ml, world, "int_min_neg"),
        Variant::Int(i64::MIN),
        "negating i64::MIN must wrap back to itself: there is no +2^63, so \
         `-i` panics on this input while C++ wraps. (The inner \
         `-9223372036854775807 - 1` is how i64::MIN is written without an \
         out-of-range literal.)"
    );

    // -- (3) int / int by zero STILL errors ----------------------------------
    // Both flags are required. `survived == false` alone would also hold if
    // `_ready` had never run on this node.
    assert_eq!(
        script_var(&ml, int_div, "reached"),
        Variant::Bool(true),
        "precondition: `IntDivZero._ready` must actually have run, or the \
         `survived` assertion below is vacuous"
    );
    assert_eq!(
        script_var(&ml, int_div, "survived"),
        Variant::Bool(false),
        "`1 / 0` must STILL raise a script error and abort the function — \
         Godot keeps the error-on-zero `DivNZ` evaluator for int/int \
         (`variant_op.cpp:353`). A true here means the fix went too far and \
         made integer division by zero silently produce a value"
    );
}
