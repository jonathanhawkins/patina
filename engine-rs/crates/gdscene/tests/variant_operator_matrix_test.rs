//! pat-mgixk.11 acceptance: the Variant arithmetic operator matrix, driven by a
//! GDScript attached to a node in a loaded scene.
//!
//! Godot registers `+ - * / %` and unary `-` for every builtin pair in
//! `core/variant/variant_op.cpp` `_register_variant_operators`. The interpreter
//! only ever grew the arms a given bead needed, so 186 of the 271 non-equality
//! rows raised a `TypeError` at runtime: `velocity * Vector2(1, 0)`,
//! `cell * 16`, `modulate * 0.5`, `[a] + [b]` and `-Color(...)` all aborted the
//! script.
//!
//! ⚠️ Every expected value here comes from the upstream registration table, not
//! from intuition, because the result **type** is not guessable:
//!
//! * `Vector2i * int` -> `Vector2i` (a grid coordinate stays integral), but
//!   `Vector2i * float` -> `Vector2`. Both are asserted, including the Variant
//!   type, because an implementation that returned the other one would still
//!   produce the right numbers.
//! * `Vector2 * Transform2D` is the INVERSE transform; `Transform2D * Vector2`
//!   is the forward one. Operand order is the whole difference.
//! * int-vector `/` and `%` are `DivNZ`/`ModNZ`, so a zero divisor is a script
//!   error rather than a sentinel value.
//!
//! The script runs through the real scene path — `PackedScene` ->
//! `add_packed_scene_to_tree` -> `attach_script` -> `MainLoop` ->
//! `process_script_ready` -> `Expr::BinaryOp` -> `eval_binary_op` — and the
//! results are read back as script vars. A compound assignment (`*=`) is
//! included deliberately: it reaches `eval_binary_op` through
//! `apply_assign_op`, a different caller, so it would miss a table wired into
//! only one of the two paths.

use gdcore::math::{Color, Vector2, Vector2i, Vector4, Vector4i};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
"#;

/// Assigns through `self.<var>` throughout, which is the spelling the existing
/// script-dispatch tests use and that `list_properties` reads back.
const SCRIPT: &str = r#"
extends Node2D

var v2_mul
var v2_div
var v2i_mul_int
var v2i_mul_float
var v2i_div
var v2i_mod
var col_mul
var col_add
var col_sub
var col_neg
var arr_cat
var packed_cat
var v4_mul
var v4i_mul_int
var v4_add
var xform_inv
var basis_scaled
var compound
var ran = false

func _ready():
    self.v2_mul = Vector2(2, 3) * Vector2(4, 5)
    self.v2_div = Vector2(8, 15) / Vector2(4, 5)
    self.v2i_mul_int = Vector2i(3, 4) * 2
    self.v2i_mul_float = Vector2i(3, 4) * 0.5
    self.v2i_div = Vector2i(7, 9) / 2
    self.v2i_mod = Vector2i(7, 9) % 4
    self.col_mul = Color(1, 0.5, 0, 1) * 0.5
    self.col_add = Color.RED + Color.BLUE
    self.col_sub = Color(1, 1, 1, 1) - Color(0.25, 0.5, 0.75, 1)
    self.col_neg = -Color(1, 1, 1, 1)
    self.arr_cat = [1] + [2, 3]
    self.packed_cat = PackedInt32Array([1]) + PackedInt32Array([2])
    self.v4_mul = Vector4(1, 2, 3, 4) * 2
    self.v4i_mul_int = Vector4i(1, 2, 3, 4) * 2
    self.v4_add = Vector4(1, 2, 3, 4) + Vector4(10, 20, 30, 40)
    self.xform_inv = Vector2(1, 0) * Transform2D.IDENTITY
    self.basis_scaled = Basis.IDENTITY * 2.0
    var acc = Vector2(1, 1)
    acc *= Vector2(2, 3)
    self.compound = acc
    self.ran = true
"#;

/// Reads a script `var` — node properties and script vars are separate stores.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

fn assert_color(got: &Variant, want: Color, what: &str) {
    match got {
        Variant::Color(c) => assert!(
            approx(c.r, want.r)
                && approx(c.g, want.g)
                && approx(c.b, want.b)
                && approx(c.a, want.a),
            "{what}: expected {want:?}, got {c:?}"
        ),
        other => panic!("{what}: expected a Color, got {other:?}"),
    }
}

fn assert_vector2(got: &Variant, want: Vector2, what: &str) {
    match got {
        Variant::Vector2(v) => assert!(
            approx(v.x, want.x) && approx(v.y, want.y),
            "{what}: expected {want:?}, got {v:?}"
        ),
        other => panic!("{what}: expected a Vector2, got {other:?}"),
    }
}

#[test]
fn variant_operator_matrix_test() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let script = GDScriptNodeInstance::from_source(SCRIPT, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(scene_root);

    // ⚠️ First, and for a reason: a TypeError on ANY line above aborts `_ready`
    // silently, which from here is indistinguishable from "the var was never
    // declared". Without this guard every assertion below would report a
    // missing var instead of the operator that is actually unimplemented.
    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_ready` must run to completion — an unimplemented operator raises a \
         TypeError and aborts it part-way"
    );

    let tree = ml.tree();
    let var = |name: &str| script_var(tree, scene_root, name);

    // -- componentwise float vectors ------------------------------------
    assert_vector2(&var("v2_mul"), Vector2::new(8.0, 15.0), "Vector2 * Vector2");
    assert_vector2(&var("v2_div"), Vector2::new(2.0, 3.0), "Vector2 / Vector2");

    // -- int vectors: the result TYPE is the point ----------------------
    assert_eq!(
        var("v2i_mul_int"),
        Variant::Vector2i(Vector2i::new(6, 8)),
        "Vector2i * int must stay a Vector2i — that is what makes grid maths work"
    );
    // Same operand values, a float scalar, and a DIFFERENT result type.
    assert_vector2(
        &var("v2i_mul_float"),
        Vector2::new(1.5, 2.0),
        "Vector2i * float widens to Vector2 (upstream row VECTOR2I FLOAT -> Vector2)",
    );
    assert_eq!(
        var("v2i_div"),
        Variant::Vector2i(Vector2i::new(3, 4)),
        "Vector2i / int truncates toward zero and stays integral"
    );
    assert_eq!(
        var("v2i_mod"),
        Variant::Vector2i(Vector2i::new(3, 1)),
        "Vector2i % int is componentwise ModNZ"
    );

    // -- Color (gdcore has NO arithmetic impls for it) -------------------
    assert_color(
        &var("col_mul"),
        Color::new(0.5, 0.25, 0.0, 0.5),
        "Color * float scales every channel INCLUDING alpha",
    );
    assert_color(
        &var("col_add"),
        Color::new(1.0, 0.0, 1.0, 2.0),
        "Color.RED + Color.BLUE adds alpha too, so a == 2.0 (no clamping)",
    );
    assert_color(
        &var("col_sub"),
        Color::new(0.75, 0.5, 0.25, 0.0),
        "Color - Color",
    );
    assert_color(
        &var("col_neg"),
        Color::new(-1.0, -1.0, -1.0, -1.0),
        "-Color negates all four channels",
    );

    // -- containers: a NEW array, never an alias ------------------------
    assert_eq!(
        var("arr_cat"),
        Variant::Array(gdvariant::SharedArray::from_vec(vec![
            Variant::Int(1),
            Variant::Int(2),
            Variant::Int(3),
        ])),
        "Array + Array concatenates"
    );
    assert_eq!(
        var("packed_cat"),
        Variant::PackedInt32Array(vec![1, 2]),
        "PackedInt32Array + PackedInt32Array concatenates and stays packed"
    );

    // -- Vector4 / Vector4i ---------------------------------------------
    assert_eq!(
        var("v4_mul"),
        Variant::Vector4(Vector4::new(2.0, 4.0, 6.0, 8.0)),
        "Vector4 * int"
    );
    assert_eq!(
        var("v4i_mul_int"),
        Variant::Vector4i(Vector4i::new(2, 4, 6, 8)),
        "Vector4i * int stays integral"
    );
    assert_eq!(
        var("v4_add"),
        Variant::Vector4(Vector4::new(11.0, 22.0, 33.0, 44.0)),
        "Vector4 + Vector4"
    );

    // -- the inverse-transform row (vector on the LEFT) ------------------
    assert_vector2(
        &var("xform_inv"),
        Vector2::new(1.0, 0.0),
        "Vector2 * Transform2D.IDENTITY is the inverse transform, which the \
         identity leaves unchanged",
    );
    match var("basis_scaled") {
        Variant::Basis(b) => {
            assert!(
                approx(b.x.x, 2.0) && approx(b.y.y, 2.0) && approx(b.z.z, 2.0),
                "Basis * float scales every column, got {b:?}"
            );
            assert!(
                approx(b.x.y, 0.0) && approx(b.y.x, 0.0),
                "the off-diagonal entries must stay zero, got {b:?}"
            );
        }
        other => panic!("Basis.IDENTITY * 2.0 should be a Basis, got {other:?}"),
    }

    // -- the OTHER caller of eval_binary_op ------------------------------
    assert_vector2(
        &var("compound"),
        Vector2::new(2.0, 3.0),
        "`*=` reaches eval_binary_op through apply_assign_op, not Expr::BinaryOp",
    );
}
