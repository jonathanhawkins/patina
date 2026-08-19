//! pat-s3rrv.57 (merged finding 7): `class_get_method_list`'s `flags` really
//! distinguish a virtual method from a concrete one.
//!
//! The key was always emitted, but almost nothing set it: of 1901 registered
//! methods only 48 carried any flag, and **zero** were marked static. So a
//! caller could read `flags` and learn nothing — the editor's
//! Override-Virtual-Method dialog, which upstream builds by filtering this list
//! on `METHOD_FLAG_VIRTUAL`, would have offered an empty list for `Node`.
//!
//! Flag values are Godot's own (`upstream/godot/core/object/object.h`):
//!
//! ```text
//! METHOD_FLAG_NORMAL = 1   METHOD_FLAG_VIRTUAL = 8    METHOD_FLAG_STATIC = 32
//! METHOD_FLAG_CONST  = 4   METHOD_FLAG_VARARG  = 16   METHOD_FLAG_VIRTUAL_REQUIRED = 128
//! ```
//!
//! ⚠️ Which method carries which flag comes from the `qualifiers=` attribute on
//! `<method>` in `upstream/godot/doc/classes/*.xml` — never from name shape.
//! `Tween.interpolate_value` has no underscore and IS static;
//! `Object.get_class` has no underscore and is `const`, not virtual. A
//! leading-underscore rule gets both wrong, which is why static sat at zero.
//!
//! # Why this lives in gdscript-interop rather than beside the scene probes
//!
//! `ClassDB.class_get_method_list` is answered by `call_classdb_static`, a free
//! function needing no scene access, so the interpreter harness is sufficient —
//! and `gdscene`'s lib is currently red from unrelated in-flight work, which
//! would make a test there unverifiable for reasons having nothing to do with
//! ClassDB.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Godot's `MethodFlags` (`core/object/object.h`).
const METHOD_FLAG_VIRTUAL: i64 = 8;
const METHOD_FLAG_STATIC: i64 = 32;

/// Each probe returns -1 when it never found the method, so "the list did not
/// contain it" stays distinguishable from "it carried no flags".
const SRC: &str = "extends Node\n\
                   \n\
                   func ready_flags():\n\
                   \x20   for m in ClassDB.class_get_method_list(\"Node\"):\n\
                   \x20       if m[\"name\"] == \"_ready\":\n\
                   \x20           return m[\"flags\"]\n\
                   \x20   return -1\n\
                   \n\
                   func add_child_flags():\n\
                   \x20   for m in ClassDB.class_get_method_list(\"Node\"):\n\
                   \x20       if m[\"name\"] == \"add_child\":\n\
                   \x20           return m[\"flags\"]\n\
                   \x20   return -1\n\
                   \n\
                   func static_flags():\n\
                   \x20   for m in ClassDB.class_get_method_list(\"Tween\"):\n\
                   \x20       if m[\"name\"] == \"interpolate_value\":\n\
                   \x20           return m[\"flags\"]\n\
                   \x20   return -1\n\
                   \n\
                   func virtual_count():\n\
                   \x20   var n = 0\n\
                   \x20   for m in ClassDB.class_get_method_list(\"Node\"):\n\
                   \x20       if (m[\"flags\"] & 8) != 0:\n\
                   \x20           n += 1\n\
                   \x20   return n\n";

/// Runs one probe method.
///
/// Mirrors the call sequence of the green
/// `classdb_instantiate_builds_non_node_classes` test — `run_class` →
/// `instantiate_class` → `call_instance_method` — rather than inventing one.
///
/// ⚠️ `register_all_classes`, not the 2D/3D pair: ClassDB registers itself in
/// `register_root_window_chain`, which only the full registrar runs.
fn probe(method: &str) -> i64 {
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SRC).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    match interp.call_instance_method(&mut inst, method, &[]) {
        Ok(Variant::Int(i)) => i,
        Ok(other) => panic!("`{method}` should answer an Int, got {other:?}"),
        Err(e) => panic!("`{method}` failed: {e:?}"),
    }
}

/// **The acceptance case**: a virtual is flagged, a concrete method is not.
///
/// Both halves matter. Asserting only that `_ready` is virtual would pass
/// against an implementation that flagged *everything*, which is exactly as
/// useless to the dialog consuming this as flagging nothing.
#[test]
fn a_virtual_is_flagged_and_a_concrete_method_is_not() {
    let ready = probe("ready_flags");
    assert!(
        ready >= 0,
        "Node must declare _ready in its method list at all; -1 means the probe \
         never found it"
    );
    assert!(
        ready & METHOD_FLAG_VIRTUAL != 0,
        "Node._ready must carry METHOD_FLAG_VIRTUAL (8); flags were {ready}"
    );

    let add_child = probe("add_child_flags");
    assert!(add_child >= 0, "Node must declare add_child");
    assert!(
        add_child & METHOD_FLAG_VIRTUAL == 0,
        "Node.add_child is concrete and must NOT be flagged virtual — a list \
         that flags everything is as unusable as one that flags nothing; flags \
         were {add_child}"
    );
}

/// 🔑 The flags follow the documented qualifiers, not the method's name.
///
/// `Tween.interpolate_value` carries no underscore and is `qualifiers="static"`.
/// Any rule keyed on name shape misses it — and static was the flag NOTHING in
/// the registry set before this.
#[test]
fn a_static_method_is_flagged_static() {
    let flags = probe("static_flags");
    assert!(
        flags >= 0,
        "Tween must declare interpolate_value in its method list"
    );
    assert!(
        flags & METHOD_FLAG_STATIC != 0,
        "Tween.interpolate_value is documented `qualifiers=\"static\"` and must \
         carry METHOD_FLAG_STATIC (32); flags were {flags}"
    );
    assert!(
        flags & METHOD_FLAG_VIRTUAL == 0,
        "…and it is not virtual; flags were {flags}"
    );
}

/// The point of the flag: filtering the way the Override-Virtual-Method dialog
/// does selects Node's real virtuals.
///
/// ⚠️ A LOWER BOUND, not an exact count — pinning the number would break every
/// time a virtual is added to the registration, which is expected and
/// desirable. What must hold is that the filter finds the dozen Node documents
/// rather than nothing.
#[test]
fn filtering_on_the_virtual_flag_selects_the_overridables() {
    let virtuals = probe("virtual_count");
    assert!(
        virtuals >= 10,
        "Node documents a dozen virtuals (_ready, _process, _input, …); \
         filtering class_get_method_list on METHOD_FLAG_VIRTUAL found only \
         {virtuals}"
    );
}
