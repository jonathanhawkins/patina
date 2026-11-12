//! pat-nzgdu.189 — the `EncodedObjectAsID` class surface.
//!
//! Covers the acceptance's `ClassDB.class_exists('EncodedObjectAsID')` clause.
//! The wire round-trip is gdvariant's `encoded_object_as_id_roundtrip`. See the
//! bead notes.
//!
//! Schema diffed against Godot 4.6.1.stable's `extension_api.json`; the default
//! against its `--doctool` XML; behaviour against the vendored
//! `upstream/godot/core/io/marshalls.cpp`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_io_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;
use gdvariant::Variant;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

#[test]
fn encoded_object_as_id_is_registered() {
    register_io_classes();

    assert!(class_exists("EncodedObjectAsID"));
    assert!(
        is_parent_class("EncodedObjectAsID", "RefCounted"),
        "it is a RefCounted the decoder hands back, not a Resource"
    );
    assert!(!is_parent_class("EncodedObjectAsID", "Resource"));
    assert!(is_parent_class("EncodedObjectAsID", "Object"));

    assert!(!is_abstract("EncodedObjectAsID"));
    assert!(can_instantiate("EncodedObjectAsID"));
    let obj = instantiate("EncodedObjectAsID").expect("instantiates");
    assert_eq!(obj.get_class(), "EncodedObjectAsID");
}

/// It carries the id and **nothing else** — deliberately. That minimalism is the
/// security property: decoding an id can never construct anything, which is why
/// Godot permits it even when full-object decoding is refused
/// (`marshalls.cpp:699-715`).
#[test]
fn it_carries_exactly_one_property_the_id() {
    register_io_classes();

    let base = props("RefCounted");
    let mut own: Vec<String> = props("EncodedObjectAsID")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    own.sort();
    assert_eq!(
        own,
        vec!["object_id".to_string()],
        "object_id is the whole surface"
    );

    assert!(class_has_method("EncodedObjectAsID", "set_object_id"));
    assert!(class_has_method("EncodedObjectAsID", "get_object_id"));
    assert_eq!(arity("EncodedObjectAsID", "set_object_id"), 1);
    assert_eq!(arity("EncodedObjectAsID", "get_object_id"), 0);
}

/// ⚠️ The property defaults to 0 — but the decoder can never produce that: a
/// null id decodes to NIL rather than to an object with id 0
/// (`marshalls.cpp:707-708`). So a 0 here means "freshly constructed", never
/// "decoded from the wire".
#[test]
fn object_id_defaults_to_zero_which_the_decoder_never_yields() {
    register_io_classes();
    let default = get_property_list("EncodedObjectAsID")
        .into_iter()
        .find(|p| p.name == "object_id")
        .expect("object_id exists")
        .default_value;
    assert_eq!(default, Variant::Int(0), "Godot's documented default");
}

/// ⚠️ `class_count()` and the property list CANNOT catch a duplicate
/// registration: `classes` is keyed by name, so re-registering overwrites the
/// entry — the count never moves and the fresh `ClassRegistration` replaces the
/// old one wholesale. The thing that actually changes is the **ClassId**:
/// `register_class` mints a new one and leaves a stale `by_id` alias behind.
/// `register_io_classes()` is called ~20 times across the suite, so this matters.
#[test]
fn registration_is_idempotent() {
    register_io_classes();
    let first_id = gdobject::get_class_info("EncodedObjectAsID")
        .expect("registered")
        .class_id;

    register_io_classes();
    let second_id = gdobject::get_class_info("EncodedObjectAsID")
        .expect("still registered")
        .class_id;

    assert_eq!(
        first_id, second_id,
        "re-registering must not mint a new ClassId — the guard is missing if it does"
    );
    assert!(class_exists("EncodedObjectAsID"));
    assert_eq!(
        props("EncodedObjectAsID")
            .iter()
            .filter(|p| *p == "object_id")
            .count(),
        1
    );
}
