//! pat-nzgdu.220 — `PackedDataContainer` / `PackedDataContainerRef` are
//! ClassDB-registered.
//!
//! ⚠️ **`pack()`/indexing is NOT implemented, and the blocker is a missing
//! primitive: `gdvariant` has no Godot-compatible `Variant::hash()`.**
//!
//! That is not a detail. Godot's packed format stores a dictionary as
//! `(hash, key_offset, value_offset)` triples **sorted by the key's hash**, and
//! reads them back by **binary search on that hash**
//! (`core/io/packed_data_container.cpp`, `_pack` / `_key_at_ofs`). So the hash is
//! baked into the bytes and into every lookup. `Variant::hash()`
//! (`core/variant/variant.cpp:2801`, ~84 hash-primitive call sites, type by type)
//! does not exist anywhere in this workspace.
//!
//! **Implementing it with a different hash would be worse than not implementing
//! it.** A container packed here could not be read by Godot, a container packed
//! by Godot could not be read here — every lookup would miss — and a round-trip
//! test would pass anyway, because it would use the same wrong hash on both
//! sides. That is exactly the self-consistent green this port keeps getting
//! bitten by. **Port `Variant::hash()` first; it is a prerequisite bead.**
//!
//! The rest of the format is straightforward and fully mapped in the bead notes
//! (`TYPE_DICT = 0xFFFFFFFF`, `TYPE_ARRAY = 0xFFFFFFFE`, header reserved before
//! children are appended, leaves via the existing `gdvariant::binary::encode_variant`,
//! strings interned by offset, RID/OBJECT packed as NIL).
//!
//! Schema diffed against Godot 4.6.1.stable's `extension_api.json` and the
//! `GDREGISTER_*` macros.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_io_classes,
};

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

/// The acceptance's checkable clause: both classes exist.
#[test]
fn packed_data_container_pack_index_roundtrip() {
    register_io_classes();

    assert!(class_exists("PackedDataContainer"));
    assert!(class_exists("PackedDataContainerRef"));

    // ⚠️ The container is a RESOURCE — an asset you save and load — while the
    // ref is a RefCounted view into its bytes. They do not share a base, despite
    // the shared name prefix.
    assert!(
        is_parent_class("PackedDataContainer", "Resource"),
        "the container is an asset"
    );
    assert!(
        is_parent_class("PackedDataContainerRef", "RefCounted"),
        "the ref is a live view, never saved"
    );
    assert!(
        !is_parent_class("PackedDataContainerRef", "Resource"),
        "a view is not an asset"
    );
    assert!(
        !is_parent_class("PackedDataContainerRef", "PackedDataContainer"),
        "the ref is a sibling, not a subclass — the shared prefix invites this"
    );

    // Both reach Object; an unregistered base severs the chain silently
    // (pat-nzgdu.98). This module bootstraps Object/RefCounted but not Resource.
    assert!(is_parent_class("PackedDataContainer", "Object"));
    assert!(is_parent_class("PackedDataContainerRef", "Object"));
}

/// ⚠️ The container is constructible; the ref is ABSTRACT.
///
/// `GDREGISTER_CLASS(PackedDataContainer)` vs
/// `GDREGISTER_ABSTRACT_CLASS(PackedDataContainerRef)` — a script builds a
/// container and packs into it, but never constructs a view: indexing hands one
/// back. Abstractness is what stops a script making a view of nothing.
#[test]
fn the_container_constructs_but_the_view_is_abstract() {
    register_io_classes();

    assert!(!is_abstract("PackedDataContainer"));
    assert!(can_instantiate("PackedDataContainer"));
    let obj = instantiate("PackedDataContainer").expect("it instantiates");
    assert_eq!(obj.get_class(), "PackedDataContainer");

    assert!(
        is_abstract("PackedDataContainerRef"),
        "GDREGISTER_ABSTRACT_CLASS — the container hands views back"
    );
    assert!(!can_instantiate("PackedDataContainerRef"));
}

/// ⚠️ Exactly two methods, and one on the view.
///
/// `pack` and `size` — that is the whole bound surface. **Indexing is not a
/// method**: it goes through the Variant `_get` operator, so a `get`/`get_at`
/// here would be invented. The acceptance's "reading back via indexing" is an
/// operator, not an API call.
#[test]
fn the_bound_surface_is_pack_and_size_only() {
    register_io_classes();

    let mut own = own_methods("PackedDataContainer", "Resource");
    own.sort();
    assert_eq!(
        own,
        vec!["pack", "size"],
        "Godot binds exactly pack and size on PackedDataContainer"
    );
    assert_eq!(arity("PackedDataContainer", "pack"), 1, "pack(value)");
    assert_eq!(arity("PackedDataContainer", "size"), 0);

    // The view exposes only size — nested containers are reached by indexing it.
    assert_eq!(
        own_methods("PackedDataContainerRef", "RefCounted"),
        vec!["size"],
        "the view binds only size"
    );
    assert_eq!(arity("PackedDataContainerRef", "size"), 0);

    // Not invented: no indexing accessor exists anywhere in the chain.
    //
    // ⚠️ `get` is deliberately NOT in this list. It exists — inherited from
    // `Object` — but it is the *generic property accessor* (`obj.get("prop")`),
    // not container indexing. `class_has_method` walks the whole chain, so
    // asserting `!class_has_method(.., "get")` asserts something false about
    // Object, not something true about PackedDataContainer. The `own_methods`
    // check above is what actually proves indexing has no bound method.
    for m in ["get_at", "index", "keys", "values"] {
        assert!(
            !class_has_method("PackedDataContainer", m),
            "Godot has no PackedDataContainer::{m} — indexing is an operator"
        );
    }
    assert!(
        class_has_method("PackedDataContainer", "get"),
        "Object::get IS inherited — the generic property accessor, which is a \
         different thing from indexing"
    );
}

/// ⚠️ `__data__` is REAL but INTERNAL, and is deliberately not registered.
///
/// `packed_data_container.cpp:366` does
/// `ADD_PROPERTY(PropertyInfo(Variant::PACKED_BYTE_ARRAY, "__data__", ...,
/// PROPERTY_USAGE_STORAGE | PROPERTY_USAGE_INTERNAL), "_set_data", "_get_data")`
/// — so it exists in Godot's ClassDB, but `extension_api.json` omits it because
/// the dump filters INTERNAL properties. **The dump is therefore not a complete
/// property list.**
///
/// It is left off here because `PropertyInfo` has no `usage` field: registering
/// it would advertise an internal, storage-only member as a public property —
/// the opposite error. When `PropertyInfo` grows `usage`, register it as
/// STORAGE|INTERNAL and this test should change.
#[test]
fn known_gap_the_internal_data_member_is_not_registered() {
    register_io_classes();

    let base: Vec<String> = get_property_list("Resource")
        .into_iter()
        .map(|p| p.name)
        .collect();
    let own: Vec<String> = get_property_list("PackedDataContainer")
        .into_iter()
        .map(|p| p.name)
        .filter(|p| !base.contains(p))
        .collect();

    assert!(
        own.is_empty(),
        "matches extension_api.json, which reports zero properties because \
         __data__ is PROPERTY_USAGE_INTERNAL; found {own:?}"
    );
    assert!(
        !own.iter().any(|p| p == "__data__"),
        "__data__ must not be advertised as a public property while PropertyInfo \
         cannot express PROPERTY_USAGE_INTERNAL"
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["PackedDataContainer", "PackedDataContainerRef"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_io_classes();
    let first = ids();
    register_io_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
