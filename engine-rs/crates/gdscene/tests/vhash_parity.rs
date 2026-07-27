//! pat-0q2kx.230 acceptance: the global `hash()` returns Godot's values from a
//! scene script, and agrees with `String.hash()`.
//!
//! Godot ground truth read from the vendored source: `core/variant/variant.cpp`
//! `Variant::recursive_hash`, `core/templates/hashfuncs.{h,cpp}`,
//! `core/math/vector2.h`, `core/string/ustring.cpp`, `core/variant/array.cpp`.
//!
//! ⚠️ **The bead's description of the algorithm is wrong for Godot 4.x.** It says
//! "float → hash_djb2_one_float" and "Vector2/3/Color → hash_djb2_buffer". In 4.6
//! those are **murmur3** — `FLOAT` uses `hash_murmur3_one_double` and `Vector2`
//! uses its own `hash()` (murmur3 per component, then `hash_fmix32`). The djb2
//! float helpers still exist in `hashfuncs.h`, which is presumably where the
//! description came from, but `Variant::hash` no longer calls them. Only `INT`
//! (`hash_one_uint64`) and `STRING` (djb2) match the bead's account.
//!
//! The golden constants below are what Godot's own algorithms produce,
//! transcribed from `upstream/godot` and executed step by step. Two of them were
//! additionally re-derived a second way as a check on the transcription itself:
//! `djb2("hello")` computed as `h*33 + c` and as `(h<<5)+h+c` both give
//! 261238937, and Thomas Wang on 42 gives 2136435707.
//!
//! ⚠️ NOT covered by `godot_hash`: NodePath, Transform2D, Basis, Transform3D,
//! ObjectId, Callable, Signal, Resource. They return 0 rather than a
//! plausible-but-wrong number — a hash that disagrees with Godot surfaces as a
//! dictionary lookup that silently misses, so an obviously-wrong value is safer
//! than a convincing one. See the completion report.

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Every clause of the acceptance, computed inside a scene-attached script.
const HASH_GD: &str = "\
extends Node
var h_hello = 0
var h_string_method = 0
var h_int = 0
var h_float = 0
var h_array = 0
var h_vec2 = 0
var h_true = 0
var h_null = 0

func _ready():
    self.h_hello = hash('hello')
    self.h_string_method = 'hello'.hash()
    self.h_int = hash(42)
    self.h_float = hash(1.5)
    self.h_array = hash([1, 2, 3])
    self.h_vec2 = hash(Vector2(1, 2))
    self.h_true = hash(true)
    self.h_null = hash(null)
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn int_var(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match script_var(ml, node, name) {
        Some(Variant::Int(i)) => i,
        other => panic!("`{name}` must be an int, got {other:?}"),
    }
}

/// ⚠️ `_ready` is dispatched by `LifecycleManager::enter_tree`, **not** by
/// `MainLoop::step` — `add_child` alone does not fire it. Without the explicit
/// call every var stays at its declared `0` and the assertions below fail in a
/// way that reads like "hash() returns zero" rather than "the script never ran".
fn run_hash_script() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree
        .add_child(root_id, Node::new("Hasher", "Node"))
        .unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(HASH_GD, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    (ml, node)
}

/// **The acceptance.** Godot-matching values, from a scene script.
#[test]
fn variant_hash_parity_runtime() {
    let (ml, node) = run_hash_script();

    // The clause the bead names first: the global builtin and the String method
    // must agree. They did not before — `hash()` was SipHash while
    // `String.hash()` has always been djb2.
    assert_eq!(
        int_var(&ml, node, "h_hello"),
        int_var(&ml, node, "h_string_method"),
        "hash(\"hello\") must equal \"hello\".hash()"
    );

    // …and both must be Godot's value, not merely equal to each other. Equality
    // alone would pass if both were wrong in the same way.
    assert_eq!(
        int_var(&ml, node, "h_hello"),
        261_238_937,
        "djb2(\"hello\")"
    );
    assert_eq!(
        int_var(&ml, node, "h_int"),
        2_136_435_707,
        "hash(42) is hash_one_uint64, Thomas Wang's 64->32 mix"
    );
    assert_eq!(
        int_var(&ml, node, "h_float"),
        3_990_958_085,
        "hash(1.5) is hash_murmur3_one_double — NOT the djb2 float helper the \
         bead describes"
    );
    assert_eq!(
        int_var(&ml, node, "h_array"),
        3_860_078_832,
        "hash([1,2,3]) folds each element hash from the ARRAY-ordinal seed"
    );
    assert_eq!(
        int_var(&ml, node, "h_vec2"),
        4_195_279_070,
        "hash(Vector2(1,2)) is murmur3 per component then fmix32"
    );

    // Godot returns literal 1/0 for bools and 0 for null.
    assert_eq!(int_var(&ml, node, "h_true"), 1);
    assert_eq!(int_var(&ml, node, "h_null"), 0);
}

/// The values are non-negative: Godot's `Variant::hash` returns `uint32_t`, so
/// widening to the script's int must not sign-extend.
#[test]
fn hashes_are_non_negative() {
    let (ml, node) = run_hash_script();
    for name in ["h_hello", "h_int", "h_float", "h_array", "h_vec2"] {
        let v = int_var(&ml, node, name);
        assert!(v >= 0, "{name} widened to a negative int: {v}");
        assert!(v <= u32::MAX as i64, "{name} exceeds u32: {v}");
    }
}

/// Determinism and distinctness, driven through the script surface — the two
/// properties the old SipHash implementation did satisfy, kept so a future
/// change cannot regress them while chasing the golden values.
#[test]
fn hash_is_deterministic_and_discriminating() {
    const GD: &str = "\
extends Node
var same = false
var differs = false
var nested_differs = false

func _ready():
    self.same = hash('abc') == hash('abc')
    self.differs = hash('abc') != hash('abd')
    self.nested_differs = hash([[1]]) != hash([1])
";
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree.add_child(root_id, Node::new("H", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(GD, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    for name in ["same", "differs", "nested_differs"] {
        assert_eq!(
            script_var(&ml, node, name),
            Some(Variant::Bool(true)),
            "`{name}` must hold"
        );
    }
}
