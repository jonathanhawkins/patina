//! pat-sazee: `var_to_str` / `str_to_var` of a Basis or Transform3D use
//! Godot's ROW-major text order; `str()` prints the COLUMNS.
//!
//! Godot's writer walks `m3.rows[i][j]` (core/variant/variant_parser.cpp:2097,
//! 2110) and its parser fills `rows`. Patina's `Basis` stores columns and wrote
//! them in column order, so a Transform3D printed by `var_to_str` and read by
//! Godot (or a .tscn written by Godot and read by `str_to_var`) came back
//! transposed: every rotation inverted.
//!
//! Expected strings are verbatim Godot 4.6.1 headless output for the same
//! expressions (a non-symmetric matrix, so a transposition always shows):
//!
//! ```text
//! var_to_str(Basis(Vector3(1, 4, 7), Vector3(2, 5, 8), Vector3(3, 6, 9)))
//!   = Basis(1, 2, 3, 4, 5, 6, 7, 8, 9)
//! var_to_str(Transform3D(<that basis>, Vector3(10, 11, 12)))
//!   = Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)
//! str(<that transform>)
//!   = [X: (1.0, 4.0, 7.0), Y: (2.0, 5.0, 8.0), Z: (3.0, 6.0, 9.0), O: (10.0, 11.0, 12.0)]
//! var_to_str(Projection(Vector4(1, 2, 3, 4), ..., Vector4(13, 14, 15, 16)))
//!   = Projection(1, 2, ..., 16)      (Projection is COLUMN-major)
//! str_to_var("Transform3D(1, 2, ..., 12)") * Vector3(1, 0, 0) = (11.0, 15.0, 19.0)
//! ```

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn call(body: &str) -> Variant {
    let src = format!("extends Node\n\nfunc check():\n{body}\n");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}\n{src}"))
}

fn string(body: &str) -> String {
    match call(body) {
        Variant::String(s) => s,
        other => panic!("expected a String, got {other:?}"),
    }
}

const BASIS: &str = "Basis(Vector3(1, 4, 7), Vector3(2, 5, 8), Vector3(3, 6, 9))";

#[test]
fn var_to_str_of_a_basis_is_row_major() {
    assert_eq!(
        string(&format!("    return var_to_str({BASIS})")),
        "Basis(1, 2, 3, 4, 5, 6, 7, 8, 9)"
    );
}

#[test]
fn var_to_str_of_a_transform3d_is_row_major() {
    assert_eq!(
        string(&format!(
            "    return var_to_str(Transform3D({BASIS}, Vector3(10, 11, 12)))"
        )),
        "Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)"
    );
}

/// `str()` is the display form and lists COLUMNS — unchanged by the fix.
#[test]
fn str_of_a_transform3d_lists_columns() {
    assert_eq!(
        string(&format!(
            "    return str(Transform3D({BASIS}, Vector3(10, 11, 12)))"
        )),
        "[X: (1.0, 4.0, 7.0), Y: (2.0, 5.0, 8.0), Z: (3.0, 6.0, 9.0), O: (10.0, 11.0, 12.0)]"
    );
}

/// Godot's text read the other way: the first three numbers are ROW 0.
#[test]
fn str_to_var_of_a_transform3d_reads_rows() {
    let v = call(
        "    var t = str_to_var(\"Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)\")\n    return t * Vector3(1, 0, 0)",
    );
    assert_eq!(v, Variant::Vector3(gdcore::math::Vector3::new(11.0, 15.0, 19.0)));
}

/// Projection is the exception: Godot writes its cells COLUMN-major.
#[test]
fn var_to_str_of_a_projection_is_column_major_and_round_trips() {
    let p = "Projection(Vector4(1, 2, 3, 4), Vector4(5, 6, 7, 8), Vector4(9, 10, 11, 12), Vector4(13, 14, 15, 16))";
    assert_eq!(
        string(&format!("    return var_to_str({p})")),
        "Projection(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16)"
    );
    assert_eq!(
        call(&format!(
            "    var p = {p}\n    return str_to_var(var_to_str(p)) == p"
        )),
        Variant::Bool(true)
    );
}
