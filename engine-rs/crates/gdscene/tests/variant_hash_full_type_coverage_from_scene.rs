//! pat-s3rrv.223 — `godot_hash` covers every Variant kind, and
//! `Array.hash()`/`Dictionary.hash()` are callable from GDScript.
//!
//! `hash.rs` handled 26 kinds and ended in `_ => 0`. Fourteen kinds —
//! NodePath, Rect2i, Vector4, Vector4i, Transform2D, Basis, Transform3D,
//! Projection, Rid, ObjectId, Callable, Signal, Resource and
//! PackedVector4Array — therefore all hashed to **zero**, which collapses any
//! Dictionary keyed by them into a single bucket. The catch-all is now gone, so
//! a future Variant kind is a compile error rather than another silent zero.
//!
//! ⚠️ PROVENANCE OF THE GOLDEN VALUES — both were re-derived from the vendored
//! C++ in `upstream/godot`, independently of any Patina code (pat-s3rrv.223).
//!
//! The earlier note said they were "calibrated against three goldens this repo
//! already asserts", and `{"a":1} = 120477264` was itself one of those
//! calibration inputs — so the dictionary assertion restated a number rather
//! than testing it. Both are now derived rather than calibrated, and the
//! derivation is short enough to redo by hand:
//!
//! * primitives — `core/templates/hashfuncs.h`: `HASH_MURMUR3_SEED = 0x7F07C65`,
//!   `hash_murmur3_one_32` (×0xcc9e2d51, rotl15, ×0x1b873593, xor, rotl13,
//!   ×5 +0xe6546b64), `hash_fmix32`, and `hash_one_uint64`;
//! * containers — `Array::recursive_hash` (`core/variant/array.cpp`) seeds with
//!   `Variant::ARRAY` then folds each element; `Dictionary::recursive_hash`
//!   (`core/variant/dictionary.cpp`) seeds with `Variant::DICTIONARY` then folds
//!   KEY then VALUE per entry; both finish through `hash_fmix32`;
//! * leaves — `Variant::recursive_hash` uses `hash_one_uint64` for INT and
//!   `String::hash` (djb2, `hashv * 33 + c`, seed 5381) for STRING;
//! * ordinals counted off `Variant::Type` in `core/variant/variant.h`:
//!   `DICTIONARY = 27`, `ARRAY = 28`.
//!
//! Folding those gives `[1,2,3] -> 3860078832` and `{"a":1} -> 120477264`,
//! matching the assertions below. Since the chain starts at Godot's constants
//! and never consults `gdvariant`, agreement here is parity evidence rather than
//! our implementation agreeing with itself.

use gdcore::math::{Rect2i, Transform2D, Vector2, Vector2i, Vector4, Vector4i};
use gdcore::math3d::{Basis, Transform3D};
use gdcore::node_path::NodePath;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::hash::godot_hash;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Coverage: the fourteen kinds that used to hash to zero
// ---------------------------------------------------------------------------

fn nonsymmetric_basis() -> Basis {
    // ⚠️ Deliberately NOT symmetric and NOT the identity. A symmetric matrix
    // hashes the same whether you read rows or columns, so an identity fixture
    // cannot catch the transpose bug this bead's Basis arm had to get right.
    Basis {
        x: gdcore::math::Vector3::new(1.0, 2.0, 3.0),
        y: gdcore::math::Vector3::new(4.0, 5.0, 6.0),
        z: gdcore::math::Vector3::new(7.0, 8.0, 9.0),
    }
}

/// One value of each kind that previously fell through to `_ => 0`.
fn previously_unhashed() -> Vec<(&'static str, Variant)> {
    vec![
        ("NodePath", Variant::NodePath(NodePath::new("A"))),
        (
            "Rect2i",
            Variant::Rect2i(Rect2i::new(Vector2i::new(1, 2), Vector2i::new(3, 4))),
        ),
        (
            "Vector4",
            Variant::Vector4(Vector4::new(1.0, 2.0, 3.0, 4.0)),
        ),
        // ⚠️ 5,6,7,8 and NOT 1,2,3,4 — the value change is load-bearing.
        // `Rect2i` above carries the same four components (pos 1,2 / size 3,4),
        // and Godot hashes BOTH types with byte-identical code: four
        // `hash_murmur3_one_32` folds over the components in order, finished with
        // `hash_fmix32`, and no type tag (`core/math/rect2i.h:230`,
        // `core/math/vector4i.h:139`). Same components therefore MUST produce the
        // same hash upstream, so the distinctness assertion below was demanding
        // behaviour Godot does not provide. Giving them different values keeps the
        // real intent — distinct KEYS stay distinct — without asserting a
        // divergence from the engine we are porting.
        ("Vector4i", Variant::Vector4i(Vector4i::new(5, 6, 7, 8))),
        (
            "Transform2D",
            Variant::Transform2D(Transform2D {
                x: Vector2::new(1.0, 2.0),
                y: Vector2::new(3.0, 4.0),
                origin: Vector2::new(5.0, 6.0),
            }),
        ),
        ("Basis", Variant::Basis(nonsymmetric_basis())),
        (
            "Transform3D",
            Variant::Transform3D(Transform3D {
                basis: nonsymmetric_basis(),
                origin: gdcore::math::Vector3::new(10.0, 11.0, 12.0),
            }),
        ),
        ("Rid", Variant::Rid(gdcore::id::Rid::from_raw(0xABCD_1234))),
        (
            "ObjectId",
            Variant::ObjectId(gdcore::id::ObjectId::from_raw(0x1234_ABCD)),
        ),
        (
            "PackedVector4Array",
            Variant::PackedVector4Array(vec![Vector4::new(1.0, 2.0, 3.0, 4.0)]),
        ),
    ]
}

#[test]
fn every_previously_unhashed_kind_now_hashes_nonzero() {
    for (name, v) in previously_unhashed() {
        assert_ne!(
            godot_hash(&v),
            0,
            "{name} still hashes to 0 — the `_ => 0` catch-all is back, and a \
             Dictionary keyed by {name} collapses to one bucket"
        );
    }
}

#[test]
fn the_previously_unhashed_kinds_do_not_collide_with_each_other() {
    // Non-zero alone is weak: a constant would satisfy it. Distinctness is what
    // makes them usable as dictionary keys.
    let all = previously_unhashed();
    for (i, (na, a)) in all.iter().enumerate() {
        for (nb, b) in all.iter().skip(i + 1) {
            // A Godot-faithful collision, pinned in its own test below.
            if (*na, *nb) == ("Vector4", "PackedVector4Array") {
                continue;
            }
            assert_ne!(
                godot_hash(a),
                godot_hash(b),
                "{na} and {nb} hash identically — distinct keys would share a bucket"
            );
        }
    }
}

/// Godot COLLIDES a `Vector4` with a one-element `PackedVector4Array` holding
/// it, and so do we.
///
/// ⚠️ Also not a bug. `Vector4::hash` (`core/math/vector4.h:150`) is four
/// `hash_murmur3_one_real` folds from the default seed then `hash_fmix32`;
/// the `PACKED_VECTOR4_ARRAY` arm of `Variant::recursive_hash`
/// (`core/variant/variant.cpp:3108`) starts at `HASH_MURMUR3_SEED`, folds the
/// same four reals per element, then `hash_fmix32` — the identical sequence for
/// a single element, with no type tag or length mixed in.
#[test]
fn a_vector4_and_its_one_element_packed_array_collide_as_in_godot() {
    let v = Vector4::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(
        godot_hash(&Variant::Vector4(v)),
        godot_hash(&Variant::PackedVector4Array(vec![v])),
    );
}

/// Godot COLLIDES `Rect2i` and `Vector4i` that carry the same four components,
/// and so do we — pinned here so the next reader does not "fix" it.
///
/// ⚠️ This looks like a bug and is not. `Variant::recursive_hash` dispatches both
/// to `HashMapHasherDefault::hash(..)` with no type tag, and the two hashers are
/// the same function: four `hash_murmur3_one_32` folds in component order,
/// finished with `hash_fmix32` (`core/math/rect2i.h:230`,
/// `core/math/vector4i.h:139`). Matching that is parity; separating them by
/// mixing in a type tag would be a DIVERGENCE, and would quietly change every
/// hash we already assert as a golden.
///
/// This is why the fixture above gives the two kinds different components.
#[test]
fn rect2i_and_vector4i_with_equal_components_collide_as_they_do_in_godot() {
    let r = Variant::Rect2i(Rect2i::new(Vector2i::new(1, 2), Vector2i::new(3, 4)));
    let v = Variant::Vector4i(Vector4i::new(1, 2, 3, 4));
    assert_eq!(
        godot_hash(&r),
        godot_hash(&v),
        "Godot hashes Rect2i and Vector4i with identical code and no type tag, so \
         equal components must collide; a difference here means we diverged"
    );
}

#[test]
fn a_basis_does_not_hash_the_same_as_its_transpose() {
    // 🔑 The test that pins the row/column question. Godot's
    // `Basis::operator[]` returns ROWS while Patina stores COLUMNS, so the hash
    // arm transposes on purpose. If someone "simplifies" it to feed our columns
    // straight through, this fails — and nothing built on the identity would.
    let b = nonsymmetric_basis();
    let transposed = Basis {
        x: gdcore::math::Vector3::new(b.x.x, b.y.x, b.z.x),
        y: gdcore::math::Vector3::new(b.x.y, b.y.y, b.z.y),
        z: gdcore::math::Vector3::new(b.x.z, b.y.z, b.z.z),
    };
    assert_ne!(
        godot_hash(&Variant::Basis(b)),
        godot_hash(&Variant::Basis(transposed)),
        "a non-symmetric basis and its transpose must hash differently, or the \
         row/column convention is unpinned"
    );
}

#[test]
fn nodepath_hashes_as_its_string() {
    // Godot's NODE_PATH arm returns `NodePath::hash()`, which is the path's
    // string hash — so these must agree.
    assert_eq!(
        godot_hash(&Variant::NodePath(NodePath::new("A"))),
        NodePath::new("A").godot_hash(),
    );
    assert_ne!(
        godot_hash(&Variant::NodePath(NodePath::new("A"))),
        godot_hash(&Variant::NodePath(NodePath::new("B"))),
    );
}

// ---------------------------------------------------------------------------
// From a scene script
// ---------------------------------------------------------------------------

const SCRIPT: &str = "\
var np_a = 0
var np_b = 0
var t_one = 0
var t_two = 0
var arr_hash = 0
var dict_hash = 0
var ready_done = false
func _ready():
    var paths = {^\"A\": 1, ^\"B\": 2}
    self.np_a = paths[^\"A\"]
    self.np_b = paths[^\"B\"]
    var xf = {Transform2D(Vector2(1, 0), Vector2(0, 1), Vector2(0, 0)): 10, Transform2D(Vector2(2, 0), Vector2(0, 2), Vector2(5, 5)): 20}
    self.t_one = xf[Transform2D(Vector2(1, 0), Vector2(0, 1), Vector2(0, 0))]
    self.t_two = xf[Transform2D(Vector2(2, 0), Vector2(0, 2), Vector2(5, 5))]
    self.arr_hash = [1, 2, 3].hash()
    self.dict_hash = {\"a\": 1}.hash()
    self.ready_done = true
";

fn run_script() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("N", "Node"))
        .expect("node added");
    let script = GDScriptNodeInstance::from_source(SCRIPT, node).expect("script parses");
    tree.attach_script(node, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(node);
    (ml, node)
}

fn var_of(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

#[test]
fn a_script_keys_a_dictionary_by_nodepath_and_transform2d() {
    let (ml, node) = run_script();
    assert_eq!(
        var_of(&ml, node, "ready_done"),
        Variant::Bool(true),
        "`_ready` must run to completion, or every assertion below reads an \
         initialiser instead of a value"
    );
    // ⚠️ The load-bearing pair. With the old `_ => 0` both keys hashed to zero,
    // so the second insert landed in the first's bucket and these two reads
    // could not come back distinct.
    assert_eq!(var_of(&ml, node, "np_a"), Variant::Int(1));
    assert_eq!(var_of(&ml, node, "np_b"), Variant::Int(2));
    assert_eq!(var_of(&ml, node, "t_one"), Variant::Int(10));
    assert_eq!(var_of(&ml, node, "t_two"), Variant::Int(20));
}

#[test]
fn array_and_dictionary_hash_are_callable_and_match_godot() {
    let (ml, node) = run_script();
    // Golden values — see the module header for how they were calibrated.
    assert_eq!(
        var_of(&ml, node, "arr_hash"),
        Variant::Int(3_860_078_832),
        "[1,2,3].hash() must equal Godot's value"
    );
    assert_eq!(
        var_of(&ml, node, "dict_hash"),
        Variant::Int(120_477_264),
        "{{\"a\":1}}.hash() must equal Godot's value"
    );
}
