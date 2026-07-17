//! pat-nzgdu.221 — the eight `Placeholder*` resources are ClassDB-registered
//! with Godot's parents and surfaces.
//!
//! These are what Godot substitutes when a texture/material/mesh is stripped
//! from an export or not yet imported: they carry dimensions and draw nothing.
//!
//! ⚠️ **The bead's "give each a get_size/get_width/get_height or surface-count
//! getter" is wrong three ways**, and following it would have invented ClassDB
//! entries Godot lacks:
//! 1. **Four of the eight bind nothing at all** (`PlaceholderCubemap`,
//!    `PlaceholderCubemapArray`, `PlaceholderTexture2DArray`, `PlaceholderMaterial`).
//! 2. **Only the SETTERS are bound.** `PlaceholderTexture2D` binds `set_size`
//!    and not `get_size`; `PlaceholderTextureLayered` binds `set_layers` and not
//!    `get_layers`; `PlaceholderMesh` binds `set_aabb` and not `get_aabb`. The
//!    getters are the bases' — reachable by inheritance, which is the point.
//! 3. **No "surface count" exists.** `PlaceholderMesh`'s member is an `aabb`.
//!
//! The acceptance's third clause — "a resource load that resolves to a
//! placeholder yields a Placeholder\* instance rather than an error" — needs a
//! loader substitution path that does not exist (nothing in `gdresource` resolves
//! to placeholders). See the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`, defaults
//! against its doc XML, abstractness against the `GDREGISTER_*` macros.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_3d_classes, register_viewport_classes,
};
use gdvariant::Variant;

/// All eight, with the base Godot gives each.
const PLACEHOLDERS: &[(&str, &str)] = &[
    ("PlaceholderTexture2D", "Texture2D"),
    ("PlaceholderTexture3D", "Texture3D"),
    ("PlaceholderTextureLayered", "TextureLayered"),
    ("PlaceholderTexture2DArray", "PlaceholderTextureLayered"),
    ("PlaceholderCubemap", "PlaceholderTextureLayered"),
    ("PlaceholderCubemapArray", "PlaceholderTextureLayered"),
    ("PlaceholderMaterial", "Material"),
    ("PlaceholderMesh", "Mesh"),
];

/// The placeholders span two registration passes because their parents do —
/// `Material`/`Mesh` are 3D, the textures are viewport.
fn register_all() {
    register_viewport_classes();
    register_3d_classes();
}

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// ⚠️ `.rev()`: `get_property_list` is base-first, so a plain `.find()` returns
/// the most-BASE match on a shadowed name while `instantiate` lets the derived
/// default win. `size` IS shadowed here — every layered base has one.
fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .rev()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

/// The acceptance's clauses: all eight registered with correct chains, and
/// setting a PlaceholderTexture2D's size reads back.
#[test]
fn placeholder_resources_registered_with_size() {
    register_all();

    for (class, parent) in PLACEHOLDERS {
        assert!(class_exists(class), "{class} must be registered");
        assert!(is_parent_class(class, parent), "{class} <- {parent}");
        // An unregistered intermediate severs the chain silently (pat-nzgdu.98).
        assert!(
            is_parent_class(class, "Resource"),
            "{class} must reach Resource — every placeholder is an asset"
        );
        assert!(is_parent_class(class, "Object"));
    }

    // The acceptance's size clause. `get_size` is Texture2D's method and ClassDB
    // has no call dispatch, so the property is the observable: `instantiate`
    // applies the default, and set/get round-trips it.
    let mut tex = instantiate("PlaceholderTexture2D").expect("it instantiates");
    assert_eq!(tex.get_class(), "PlaceholderTexture2D");
    assert_eq!(
        tex.get_property("size"),
        Variant::Vector2(gdcore::math::Vector2::new(1.0, 1.0)),
        "a fresh placeholder is 1x1, not 0x0"
    );

    tex.set_property(
        "size",
        Variant::Vector2(gdcore::math::Vector2::new(64.0, 32.0)),
    );
    assert_eq!(
        tex.get_property("size"),
        Variant::Vector2(gdcore::math::Vector2::new(64.0, 32.0)),
        "setting size must read back"
    );
}

/// ⚠️ Seven construct; `PlaceholderTextureLayered` is ABSTRACT.
///
/// `GDREGISTER_ABSTRACT_CLASS(PlaceholderTextureLayered)` — a loader substitutes
/// one of its three concrete kinds, never the base. (Checked against the macro,
/// not guessed: pat-nzgdu.215 found `TextureLayered` itself is
/// `GDREGISTER_VIRTUAL_CLASS` and therefore *not* abstract, so the two sit one
/// inheritance step apart with opposite answers.)
#[test]
fn only_the_layered_base_is_abstract() {
    register_all();

    assert!(
        is_abstract("PlaceholderTextureLayered"),
        "GDREGISTER_ABSTRACT_CLASS — no creation func"
    );
    assert!(!can_instantiate("PlaceholderTextureLayered"));

    // ...while its parent, one step up, is NOT abstract (pat-nzgdu.215).
    assert!(
        !is_abstract("TextureLayered"),
        "TextureLayered is GDREGISTER_VIRTUAL_CLASS — virtual, not abstract"
    );

    for (class, _) in PLACEHOLDERS
        .iter()
        .filter(|(c, _)| *c != "PlaceholderTextureLayered")
    {
        assert!(!is_abstract(class), "{class} is GDREGISTER_CLASS in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), *class);
    }
}

/// ⚠️ Four of the eight bind NOTHING — the bead's "give each a getter" is wrong.
#[test]
fn four_of_them_bind_nothing_at_all() {
    register_all();

    for (class, base) in [
        ("PlaceholderTexture2DArray", "PlaceholderTextureLayered"),
        ("PlaceholderCubemap", "PlaceholderTextureLayered"),
        ("PlaceholderCubemapArray", "PlaceholderTextureLayered"),
        ("PlaceholderMaterial", "Material"),
    ] {
        assert!(
            own_methods(class, base).is_empty(),
            "{class} binds no methods of its own in Godot"
        );
        let base_props = props(base);
        let own: Vec<String> = props(class)
            .into_iter()
            .filter(|p| !base_props.contains(p))
            .collect();
        assert!(
            own.is_empty(),
            "{class} binds no properties of its own; found {own:?}"
        );
    }

    // ...yet the layered kinds still reach the inherited surface, which is why
    // they are useful at all.
    for class in [
        "PlaceholderTexture2DArray",
        "PlaceholderCubemap",
        "PlaceholderCubemapArray",
    ] {
        assert!(class_has_method(class, "set_size"));
        assert!(props(class).iter().any(|p| p == "layers"));
    }
}

/// ⚠️ Only the SETTERS are bound — every getter is the base's.
#[test]
fn the_placeholders_bind_setters_and_inherit_the_getters() {
    register_all();

    // PlaceholderTexture2D: set_size only.
    assert_eq!(
        own_methods("PlaceholderTexture2D", "Texture2D"),
        vec!["set_size"]
    );
    assert!(
        class_has_method("PlaceholderTexture2D", "get_size"),
        "get_size is reachable — through Texture2D"
    );

    // PlaceholderMesh: set_aabb only. ⚠️ Its member is an aabb, NOT a surface
    // count — the bead's "surface-count getter" does not exist.
    assert_eq!(own_methods("PlaceholderMesh", "Mesh"), vec!["set_aabb"]);
    for invented in ["get_surface_count", "surface_get_arrays", "get_size"] {
        assert!(
            !own_methods("PlaceholderMesh", "Mesh").contains(&invented.to_string()),
            "PlaceholderMesh does not bind {invented}"
        );
    }

    // PlaceholderTextureLayered: set_size, get_size, set_layers — and NOT
    // get_layers, which is TextureLayered's.
    let mut own = own_methods("PlaceholderTextureLayered", "TextureLayered");
    own.sort();
    assert_eq!(own, vec!["get_size", "set_layers", "set_size"]);
    assert!(
        class_has_method("PlaceholderTextureLayered", "get_layers"),
        "get_layers is reachable — through TextureLayered"
    );
}

/// The defaults, from the doc XML. ⚠️ `1`, not `0` — a zero-sized or zero-layer
/// placeholder is not a texture at all, and a loader substituting one would
/// produce something that fails differently from Godot.
#[test]
fn the_defaults_are_one_not_zero() {
    register_all();

    assert_eq!(
        default_of("PlaceholderTexture2D", "size"),
        Variant::Vector2(gdcore::math::Vector2::new(1.0, 1.0)),
        "Vector2(1, 1)"
    );
    assert_eq!(
        default_of("PlaceholderTextureLayered", "layers"),
        Variant::Int(1),
        "one layer, not zero"
    );
    assert_eq!(
        default_of("PlaceholderMesh", "aabb"),
        Variant::Aabb(gdcore::math3d::Aabb::new(
            gdcore::math::Vector3::ZERO,
            gdcore::math::Vector3::ZERO
        )),
        "AABB(0, 0, 0, 0, 0, 0) — a mesh placeholder really is empty, unlike the \
         textures"
    );
}

/// ⚠️ A KNOWN GAP: two `size` members have no representable default.
///
/// `PlaceholderTextureLayered.size` is a **Vector2i** and
/// `PlaceholderTexture3D.size` a **Vector3i**; `Variant` carries neither
/// (`gdvariant::binary` names `Vector2i` among the types the enum lacks). They
/// register as `Nil`: the member exists so introspection matches Godot, but the
/// value is absent rather than a `Vector2`/`Vector3` standing in for an integer
/// vector, which would make `typeof()` lie forever.
///
/// ⚠️ Note `PlaceholderTexture2D.size` is a plain **Vector2** and IS
/// representable — the three are not the same type despite the shared name.
#[test]
fn known_gap_the_integer_vector_sizes_have_no_representable_default() {
    register_all();

    for class in ["PlaceholderTextureLayered", "PlaceholderTexture3D"] {
        assert!(
            props(class).iter().any(|p| p == "size"),
            "{class} must register `size` for introspection"
        );
        assert_eq!(
            default_of(class, "size"),
            Variant::Nil,
            "{class}.size is an integer vector, which Variant cannot represent"
        );
    }

    // Not a Vector2 masquerading as a Vector2i.
    assert_ne!(
        default_of("PlaceholderTextureLayered", "size"),
        Variant::Vector2(gdcore::math::Vector2::new(1.0, 1.0)),
        "a Vector2 must not stand in for a Vector2i"
    );

    // ...but the float-typed one IS representable, which is the contrast.
    assert_ne!(
        default_of("PlaceholderTexture2D", "size"),
        Variant::Nil,
        "PlaceholderTexture2D.size is a Vector2 and must carry a real default"
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        PLACEHOLDERS
            .iter()
            .map(|(c, _)| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_all();
    let first = ids();
    register_all();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
