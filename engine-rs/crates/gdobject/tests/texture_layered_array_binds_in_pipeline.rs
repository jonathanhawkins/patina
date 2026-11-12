//! pat-nzgdu.215 — the TextureLayered family's ClassDB surface, and the
//! scope decision for the RenderingDevice-backed wrappers.
//!
//! ⚠️ **Registering this family exposed a systemic parity bug: Patina marks
//! VIRTUAL classes as ABSTRACT.** They are not the same thing:
//!
//! - `GDREGISTER_ABSTRACT_CLASS(X)` → `register_abstract_class<X>()` → **no
//!   creation func** → `ClassDB::is_abstract` is **true**.
//! - `GDREGISTER_VIRTUAL_CLASS(X)` → `register_class<X>(/*p_virtual=*/true)` →
//!   **still installs a creation func**, and `ClassDB::is_abstract` returns
//!   `false` the moment one exists (`core/object/class_db.cpp:843`:
//!   `if (ti->creation_func != nullptr) { return false; }`). Virtual means "hidden
//!   from the editor's create dialog and meant to be extended", **not**
//!   "unconstructible". `extension_api.json` agrees: `is_instantiable=true`.
//!
//! `Texture`, `TextureLayered` and `Texture3D` are all
//! `GDREGISTER_VIRTUAL_CLASS`, and all three were registered `.abstract_class()`
//! here — so `Texture.new()` failed where Godot succeeds. Fixed, along with the
//! in-file test that asserted the wrong thing ("Godot marks the bases abstract";
//! it does not).
//!
//! **Twelve more classes have the same bug** and are left for follow-up beads —
//! see this bead's notes. Notably `Material`, which pat-nzgdu.208's test
//! currently *asserts* is abstract.
//!
//! The acceptance's render clause ("loads and binds as a layered texture sampled
//! by a shader") is not covered here: it needs the wgpu pipeline behind the
//! non-default `gpu` feature, and `gdobject` has no renderer. This file covers
//! the ClassDB clause and the RD-wrapper scope decision.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_has_method, get_method_list, inheritance_chain, instantiate,
    is_parent_class, register_viewport_classes,
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

/// The acceptance's ClassDB clause: `TextureLayered <- Texture` plus the
/// concrete layered subclasses.
#[test]
fn texture_layered_array_binds_in_pipeline() {
    register_viewport_classes();

    assert!(class_exists("TextureLayered"));
    assert!(
        is_parent_class("TextureLayered", "Texture"),
        "the acceptance's chain: TextureLayered <- Texture"
    );
    assert!(is_parent_class("TextureLayered", "Resource"));
    assert!(is_parent_class("TextureLayered", "Object"));

    // The three concrete layered resources the bead names.
    for concrete in ["Texture2DArray", "Cubemap", "CubemapArray"] {
        assert!(class_exists(concrete), "{concrete} must be registered");
        assert!(
            is_parent_class(concrete, "TextureLayered"),
            "{concrete} <- .. <- TextureLayered"
        );
        assert!(can_instantiate(concrete));
        let obj = instantiate(concrete).unwrap_or_else(|| panic!("{concrete} instantiates"));
        assert_eq!(obj.get_class(), concrete);
    }

    // ⚠️ Cubemap/CubemapArray are ImageTextureLayereds, NOT TextureLayereds
    // directly — a cubemap is six image layers, so it shares Texture2DArray's
    // image-backed base. Parenting them straight to TextureLayered would skip
    // the layer storage they use.
    assert_eq!(
        inheritance_chain("Cubemap"),
        vec![
            "Cubemap",
            "ImageTextureLayered",
            "TextureLayered",
            "Texture",
            "Resource",
            // Resource is a RefCounted, not a direct Object (pat-0q2kx.7).
            "RefCounted",
            "Object",
        ]
    );
    assert_eq!(inheritance_chain("CubemapArray")[1], "ImageTextureLayered");
}

/// ⚠️ VIRTUAL IS NOT ABSTRACT — the bug this bead found.
///
/// `Texture`/`TextureLayered`/`Texture3D` are `GDREGISTER_VIRTUAL_CLASS`, which
/// keeps a creation func, so Godot's `is_abstract` is false and `Texture.new()`
/// works. They were registered `.abstract_class()` here, which made
/// `can_instantiate` disagree with Godot for three widely-used bases.
#[test]
fn the_virtual_texture_bases_are_constructible_but_the_abstract_ones_are_not() {
    register_viewport_classes();

    // GDREGISTER_VIRTUAL_CLASS -> constructible.
    for virt in ["Texture", "TextureLayered", "Texture3D"] {
        assert!(
            !is_abstract(virt),
            "{virt} is GDREGISTER_VIRTUAL_CLASS — virtual, not abstract"
        );
        assert!(
            can_instantiate(virt),
            "{virt}.new() works in Godot; it must work here"
        );
        let obj = instantiate(virt).unwrap_or_else(|| panic!("{virt} instantiates"));
        assert_eq!(obj.get_class(), virt);
    }

    // GDREGISTER_ABSTRACT_CLASS -> not constructible. Not vacuous: these two
    // sit in the same family and really are abstract, so the split above is a
    // real distinction rather than "nothing is abstract any more".
    for abs in ["ImageTextureLayered", "TextureLayeredRD"] {
        assert!(
            is_abstract(abs),
            "{abs} is GDREGISTER_ABSTRACT_CLASS in Godot"
        );
        assert!(!can_instantiate(abs));
    }
}

/// The virtual surface. ⚠️ Being VIRTUAL is *why* these exist: a script overrides
/// `_get_layer_data` etc. to supply the data. Registering the public getters
/// without the virtuals leaves the class introspectable but not implementable.
#[test]
fn the_virtual_bases_expose_the_hooks_a_script_overrides() {
    register_viewport_classes();

    // TextureLayered: 7 public + 7 virtual = 14.
    assert_eq!(
        own_methods("TextureLayered", "Texture").len(),
        14,
        "Godot binds 14 methods on TextureLayered"
    );
    for m in [
        "get_format",
        "get_layered_type",
        "get_width",
        "get_height",
        "get_layers",
        "has_mipmaps",
        "get_layer_data",
    ] {
        assert!(class_has_method("TextureLayered", m), "public {m}");
        assert!(
            class_has_method("TextureLayered", &format!("_{m}")),
            "virtual _{m} — the hook a script overrides"
        );
    }
    assert_eq!(arity("TextureLayered", "get_layer_data"), 1);
    assert_eq!(arity("TextureLayered", "_get_layer_data"), 1);

    // Texture3D: 7 public + 6 virtual = 13. ⚠️ NOT symmetric —
    // `create_placeholder` has no `_create_placeholder` counterpart.
    assert_eq!(
        own_methods("Texture3D", "Texture").len(),
        13,
        "Godot binds 13 methods on Texture3D"
    );
    assert!(class_has_method("Texture3D", "create_placeholder"));
    assert!(
        !class_has_method("Texture3D", "_create_placeholder"),
        "create_placeholder is concrete — inventing a virtual for it would be fiction"
    );
}

/// The RD-wrapper scope decision the bead demands ("either provide a thin
/// RenderingDevice-compatible surface or record an explicit OUT_OF_SCOPE
/// decision ... do not leave them as an undocumented gap").
///
/// They are **registered with Godot's exact surface** — so the gap is closed by
/// registration, not by a scope doc. ⚠️ `TextureLayeredRD` really is abstract
/// (`GDREGISTER_ABSTRACT_CLASS`), unlike its virtual cousins above; its three
/// concrete children are what a script constructs.
#[test]
fn the_rd_wrappers_are_registered_not_silently_missing() {
    register_viewport_classes();

    assert!(class_exists("TextureLayeredRD"));
    assert!(is_parent_class("TextureLayeredRD", "TextureLayered"));
    assert!(is_abstract("TextureLayeredRD"));

    for concrete in [
        "TextureCubemapRD",
        "Texture2DArrayRD",
        "TextureCubemapArrayRD",
    ] {
        assert!(class_exists(concrete), "{concrete} must be registered");
        assert!(
            is_parent_class(concrete, "TextureLayeredRD"),
            "{concrete} <- TextureLayeredRD"
        );
        assert!(can_instantiate(concrete));
        // ⚠️ They bind NOTHING of their own — the surface is TextureLayeredRD's.
        assert!(
            own_methods(concrete, "TextureLayeredRD").is_empty(),
            "{concrete} binds no methods of its own in Godot"
        );
    }

    // ⚠️ UniformSetCacheRD is NOT a texture and NOT a TextureLayeredRD — it is
    // FramebufferCacheRD's sibling: an `Object` helper handing back a cached RID
    // from a STATIC method. The bead lists it beside the texture wrappers, which
    // invites parenting it into the texture family.
    assert!(class_exists("UniformSetCacheRD"));
    assert!(is_parent_class("UniformSetCacheRD", "Object"));
    assert!(!is_parent_class("UniformSetCacheRD", "Texture"));
    assert!(!is_parent_class("UniformSetCacheRD", "RefCounted"));
    assert_eq!(
        arity("UniformSetCacheRD", "get_cache"),
        3,
        "static get_cache(shader, set, uniforms)"
    );
    assert!(!is_abstract("UniformSetCacheRD"));
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        [
            "Texture",
            "TextureLayered",
            "Cubemap",
            "CubemapArray",
            "UniformSetCacheRD",
        ]
        .iter()
        .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
        .collect()
    };

    register_viewport_classes();
    let first = ids();
    register_viewport_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
