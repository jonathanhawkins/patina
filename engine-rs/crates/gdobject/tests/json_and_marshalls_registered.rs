//! pat-s3rrv.252 — `JSON` and `Marshalls` resolve in ClassDB.
//!
//! Godot ground truth: `core/io/json.h:39` `GDCLASS(JSON, Resource)`,
//! `core/core_bind.h:420` `GDCLASS(Marshalls, Object)`, and
//! `core/register_core_types.cpp` — `GDREGISTER_CLASS` for both, with
//! `add_singleton` (`:373`) for **Marshalls only**.
//!
//! # What was wrong
//!
//! Both were fully dispatched in the interpreter and absent from ClassDB, so
//! `class_exists("JSON")` answered **false** for a name `JSON.new()` already
//! worked on. Same shape as pat-s3rrv.239, .244 and .245: a working call path
//! with no metadata behind it.
//!
//! # 🔑 JSON's parent is `Resource`, and the acceptance says `RefCounted`
//!
//! ⚠️ The bead's acceptance asks for "parent RefCounted (NOT Object/singleton)".
//! Godot's is `GDCLASS(JSON, Resource)`. Because `Resource` derives from
//! `RefCounted`, a transitive `is_parent_class("JSON", "RefCounted")` check
//! **passes either way** — so the wrong parent would look correct while making
//! JSON not-a-Resource, which is what `.tres` handling and `ResourceLoader` key
//! off. `b_json_parent_is_resource_not_refcounted` asserts the DIRECT parent for
//! exactly that reason.
//!
//! # ⚠️ The two have opposite shapes
//!
//! JSON is a class you instantiate and is **not** a singleton; Marshalls **is**
//! a singleton and inherits `Object`. Registering them together without checking
//! would naturally give them the same treatment, and that would be wrong for one
//! of the two whichever way it went.

use gdobject::class_db::{
    get_class_info, get_property_list, register_all_classes, ClassInfo, MethodInfo,
};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

/// ⚠️ A free function with an explicit lifetime, deliberately not a closure.
/// A closure returning `&MethodInfo` borrowed from a captured local leans on
/// return-lifetime inference; the sibling tests that use a closure here return
/// `usize` by copy, so they do not prove the reference case.
fn method<'a>(info: &'a ClassInfo, name: &str) -> &'a MethodInfo {
    info.methods
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("{}.{name} is registered", info.class_name))
}

/// 🔑 **THE ACCEPTANCE CLAUSE.** Both names resolve.
#[test]
fn a_json_and_marshalls_are_registered() {
    register_all_classes();

    assert!(
        class_exists("JSON"),
        "JSON.new() already worked; ClassDB must agree the class exists"
    );
    assert!(class_exists("Marshalls"));
}

/// 🔑 JSON's DIRECT parent is `Resource`.
#[test]
fn b_json_parent_is_resource_not_refcounted() {
    register_all_classes();

    let info = get_class_info("JSON").expect("registered");
    assert_eq!(
        info.parent_class, "Resource",
        "GDCLASS(JSON, Resource) — registering it directly under RefCounted \
         would make JSON not-a-Resource"
    );

    // ⚠️ This transitive check is the one that CANNOT tell the two apart, and
    // it is what the acceptance asks for. Asserted here so the distinction is
    // explicit rather than accidental: it holds under both the right parent and
    // the wrong one.
    assert!(
        is_parent_class("JSON", "RefCounted"),
        "…and RefCounted is still reached, through Resource"
    );
    for ancestor in ["Resource", "RefCounted", "Object"] {
        assert!(
            is_parent_class("JSON", ancestor),
            "chain broken: JSON does not reach {ancestor}"
        );
    }
}

/// Marshalls has the opposite shape: straight off `Object`.
///
/// ⚠️ It must NOT be a Resource. If the two were registered from one template,
/// this is the assertion that catches it.
#[test]
fn c_marshalls_parent_is_object() {
    register_all_classes();

    let info = get_class_info("Marshalls").expect("registered");
    assert_eq!(info.parent_class, "Object");
    assert!(
        !is_parent_class("Marshalls", "Resource"),
        "Marshalls is a singleton Object, not a Resource"
    );
    assert!(
        !is_parent_class("Marshalls", "RefCounted"),
        "…and not RefCounted either"
    );
}

/// The methods the acceptance names, plus their static/const qualifiers.
///
/// ⚠️ `parse_string`, `stringify`, `from_native` and `to_native` are **static**
/// in Godot while `parse` is an instance method — the four statics are how a
/// script uses JSON without ever instantiating it, which is why they were
/// dispatched long before the class was registered.
#[test]
fn d_json_methods_and_qualifiers() {
    register_all_classes();

    let info = get_class_info("JSON").expect("registered");

    for name in ["stringify", "parse_string", "from_native", "to_native"] {
        assert!(
            method(&info, name).is_static,
            "JSON.{name} is static in Godot"
        );
    }
    assert!(
        !method(&info, "parse").is_static,
        "parse is an INSTANCE method — it fills the object's own error state"
    );

    assert_eq!(
        method(&info, "stringify").argument_count,
        4,
        "data, indent, sort_keys, full_precision"
    );
    assert_eq!(
        method(&info, "parse").argument_count,
        2,
        "json_text, keep_text"
    );
    assert_eq!(method(&info, "parse_string").argument_count, 1);

    for name in ["get_error_line", "get_error_message", "get_parsed_text"] {
        assert!(method(&info, name).is_const, "JSON.{name} is const");
        assert_eq!(method(&info, name).argument_count, 0);
    }

    // The single member.
    let data = get_property_list("JSON")
        .into_iter()
        .find(|p| p.name == "data")
        .expect("JSON.data is registered");
    assert_eq!(data.default_value, Variant::Nil, "data defaults to null");
}

/// `Marshalls`' six base64 methods, at their documented arities.
///
/// ⚠️ `variant_to_base64` and `base64_to_variant` take **two** arguments — the
/// second is `allow_objects`, and omitting it is the natural guess.
#[test]
fn e_marshalls_methods_and_arities() {
    register_all_classes();

    let info = get_class_info("Marshalls").expect("registered");
    let arity = |n: &str| method(&info, n).argument_count;

    assert_eq!(arity("variant_to_base64"), 2, "value, allow_objects");
    assert_eq!(arity("base64_to_variant"), 2, "base64_str, allow_objects");
    assert_eq!(arity("raw_to_base64"), 1);
    assert_eq!(arity("base64_to_raw"), 1);
    assert_eq!(arity("utf8_to_base64"), 1);
    assert_eq!(arity("base64_to_utf8"), 1);
}
