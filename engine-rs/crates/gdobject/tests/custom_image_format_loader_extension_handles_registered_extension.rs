//! pat-nzgdu.218 — `ImageFormatLoader` / `ImageFormatLoaderExtension` are
//! ClassDB-registered under `RefCounted`.
//!
//! **The acceptance's live-loader clause is blocked, and on the epic's oldest
//! blocker.** It asks that "registering a custom ImageFormatLoaderExtension ...
//! causes `Image.load` ... to invoke the extension's `_load_image`". A *custom*
//! `ImageFormatLoaderExtension` is a **script subclassing a RefCounted**, and
//! Patina has no script-backed non-Node objects at all: `attach_script` takes a
//! `NodeId`, so a script cannot back a `RefCounted`. There is also:
//!
//! - **no `Image` class** — `class_db.rs` does not register it, so the bead's own
//!   stated dependency ("Depends on the Image resource class") is unmet; and
//! - **no loader registry** — nothing in the workspace routes file extensions to
//!   loaders, so `add_format_loader` has nowhere to add to.
//!
//! This bead delivers the ClassDB clause. The registry is worth little until a
//! script can implement a loader, which is why the bead itself calls this "low
//! priority: extension surface used mainly by add-ons".
//!
//! Schema diffed against Godot 4.6.1.stable's `extension_api.json` and the
//! `GDREGISTER_*` macros in `register_*_types.cpp`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_get_integer_constant, class_has_method, get_method_list, get_property_list,
    instantiate, is_parent_class, register_io_classes,
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

/// The acceptance's checkable clause: both registered under `RefCounted`.
#[test]
fn custom_image_format_loader_extension_handles_registered_extension() {
    register_io_classes();

    assert!(class_exists("ImageFormatLoader"));
    assert!(class_exists("ImageFormatLoaderExtension"));
    assert!(
        is_parent_class("ImageFormatLoader", "RefCounted"),
        "the acceptance's clause: registered under RefCounted"
    );
    assert!(
        is_parent_class("ImageFormatLoaderExtension", "ImageFormatLoader"),
        "the Extension derives the base"
    );
    assert!(is_parent_class("ImageFormatLoaderExtension", "RefCounted"));
    assert!(is_parent_class("ImageFormatLoaderExtension", "Object"));

    // ⚠️ A loader is RefCounted, NOT a Resource — it is a live hook, not an
    // asset, so it is never saved to disk.
    assert!(!is_parent_class("ImageFormatLoader", "Resource"));
    assert!(!is_parent_class("ImageFormatLoader", "Node"));
}

/// ⚠️ The base is genuinely ABSTRACT — `GDREGISTER_ABSTRACT_CLASS(ImageFormatLoader)`
/// — while the Extension is `GDREGISTER_CLASS`.
///
/// Worth stating because the sibling assumption is wrong elsewhere: pat-nzgdu.215
/// found `Texture`/`TextureLayered`/`Texture3D` marked abstract when they are
/// `GDREGISTER_VIRTUAL_CLASS` and therefore constructible. The macro has to be
/// read; "it's a base class" is not evidence.
#[test]
fn the_base_is_abstract_and_the_extension_is_constructible() {
    register_io_classes();

    assert!(
        is_abstract("ImageFormatLoader"),
        "GDREGISTER_ABSTRACT_CLASS — no creation func"
    );
    assert!(!can_instantiate("ImageFormatLoader"));

    assert!(
        !is_abstract("ImageFormatLoaderExtension"),
        "GDREGISTER_CLASS — an add-on constructs one"
    );
    assert!(can_instantiate("ImageFormatLoaderExtension"));
    let obj = instantiate("ImageFormatLoaderExtension").expect("it instantiates");
    assert_eq!(obj.get_class(), "ImageFormatLoaderExtension");
}

/// ⚠️ The base binds NOTHING — no methods, no properties. It is a pure marker;
/// everything a script touches is on the Extension.
#[test]
fn the_base_is_a_pure_marker() {
    register_io_classes();

    assert!(
        own_methods("ImageFormatLoader", "RefCounted").is_empty(),
        "ImageFormatLoader binds no methods in Godot"
    );

    let base_props: Vec<String> = get_property_list("RefCounted")
        .into_iter()
        .map(|p| p.name)
        .collect();
    let own_props: Vec<String> = get_property_list("ImageFormatLoader")
        .into_iter()
        .map(|p| p.name)
        .filter(|p| !base_props.contains(p))
        .collect();
    assert!(
        own_props.is_empty(),
        "ImageFormatLoader binds no properties; found {own_props:?}"
    );
}

/// ⚠️ FOUR methods, not the two the bead names.
///
/// The bead describes "the overridable `_load_image(...)` and
/// `_get_recognized_extensions()` hooks" — but `add_format_loader` and
/// `remove_format_loader` are **public, non-virtual**: they are how a loader
/// enters and leaves the registry. A surface with only the virtuals would be
/// overridable and unregisterable.
#[test]
fn the_extension_binds_two_hooks_and_two_registry_calls() {
    register_io_classes();

    let mut own = own_methods("ImageFormatLoaderExtension", "ImageFormatLoader");
    own.sort();
    assert_eq!(
        own,
        vec![
            "_get_recognized_extensions",
            "_load_image",
            "add_format_loader",
            "remove_format_loader",
        ],
        "Godot binds exactly these four"
    );

    // The overridable hooks.
    assert_eq!(
        arity("ImageFormatLoaderExtension", "_get_recognized_extensions"),
        0
    );
    assert_eq!(
        arity("ImageFormatLoaderExtension", "_load_image"),
        4,
        "(image, file_access, flags, scale)"
    );

    // ⚠️ The registry calls are NOT virtual — no leading underscore, and taking
    // no arguments: a loader registers *itself*.
    for m in ["add_format_loader", "remove_format_loader"] {
        assert!(class_has_method("ImageFormatLoaderExtension", m));
        assert_eq!(arity("ImageFormatLoaderExtension", m), 0, "{m} takes none");
        assert!(!m.starts_with('_'), "{m} is public, not a virtual hook");
    }
}

/// The `flags` parameter's enum. ⚠️ It is a **bitfield** — the values combine, so
/// they are powers of two and `FLAG_NONE` is 0 rather than a member.
#[test]
fn loader_flags_is_a_bitfield_on_the_base() {
    register_io_classes();

    for (name, value) in [
        ("FLAG_NONE", 0),
        ("FLAG_FORCE_LINEAR", 1),
        ("FLAG_CONVERT_COLORS", 2),
    ] {
        assert_eq!(
            class_get_integer_constant("ImageFormatLoader", name),
            Some(value),
            "{name} must be readable from script"
        );
    }

    // ⚠️ The enum lives on the BASE, not the Extension — `_load_image`'s third
    // argument is `ImageFormatLoader.LoaderFlags`. It is reachable from the
    // Extension by inheritance, which is what a script writing
    // `ImageFormatLoader.FLAG_FORCE_LINEAR` relies on.
    assert!(gdobject::class_has_enum("ImageFormatLoader", "LoaderFlags"));

    // Bitfield: the non-zero values are distinct powers of two, so they compose.
    let linear = class_get_integer_constant("ImageFormatLoader", "FLAG_FORCE_LINEAR").unwrap();
    let convert = class_get_integer_constant("ImageFormatLoader", "FLAG_CONVERT_COLORS").unwrap();
    assert_eq!(linear & convert, 0, "bitfield values must not overlap");
    assert_eq!(linear | convert, 3, "and must combine");
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["ImageFormatLoader", "ImageFormatLoaderExtension"]
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
