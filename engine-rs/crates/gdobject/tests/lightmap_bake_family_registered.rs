//! pat-s3rrv.249 clause 1 — the lightmap bake family resolves in ClassDB.
//!
//! Godot ground truth: `doc/classes/LightmapGIData.xml`, and the registration
//! macros in `scene/register_scene_types.cpp:649,651` and
//! `modules/lightmapper_rd/register_types.cpp:65`.
//!
//! # What was wrong
//!
//! `LightmapGI` was registered and carries a `light_data` property — but
//! `LightmapGIData`, the type that property holds, did not exist. A `.tscn`
//! pointing `light_data` at a `LightmapGIData` resource had nothing to resolve
//! it to. `Lightmapper` and `LightmapperRD` were absent for the same reason.
//!
//! # 🔑 Only ONE of the three is abstract
//!
//! ⚠️ Taken from Godot's own macros, not inferred from the names:
//! `GDREGISTER_CLASS(LightmapGIData)`, `GDREGISTER_ABSTRACT_CLASS(Lightmapper)`,
//! `GDREGISTER_CLASS(LightmapperRD)`. Marking `LightmapperRD` abstract too — the
//! reflex, since it reads as "just the RD backend" — would make the one member
//! of the family a project actually instantiates unconstructible, and
//! `class_exists` would still answer true for it.

use gdobject::class_db::{get_class_info, get_property_list, instantiate, register_all_classes};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

/// 🔑 **THE ACCEPTANCE CLAUSE.** All three resolve, with Godot's parents.
#[test]
fn a_the_family_is_registered_with_godots_parents() {
    register_all_classes();

    for class in ["LightmapGIData", "Lightmapper", "LightmapperRD"] {
        assert!(class_exists(class), "{class} must resolve");
    }

    assert!(is_parent_class("LightmapGIData", "Resource"));
    assert!(is_parent_class("Lightmapper", "RefCounted"));

    // ⚠️ LightmapperRD inherits `Lightmapper`, not `RefCounted` directly. A
    // flattened parent still reaches `Object`, so only the intermediate link
    // catches it.
    assert!(
        is_parent_class("LightmapperRD", "Lightmapper"),
        "LightmapperRD inherits Lightmapper"
    );
    for ancestor in ["Lightmapper", "RefCounted", "Object"] {
        assert!(
            is_parent_class("LightmapperRD", ancestor),
            "chain broken: LightmapperRD does not reach {ancestor}"
        );
    }
}

/// 🔑 `Lightmapper` is abstract; the other two are not.
///
/// ⚠️ Asserting only "Lightmapper is abstract" would pass against a build that
/// marked all three, which is the likelier mistake — so the concrete pair is
/// asserted in the same test.
#[test]
fn b_only_lightmapper_is_abstract() {
    register_all_classes();

    let abstractness = |c: &str| get_class_info(c).expect("registered").is_abstract;

    assert!(abstractness("Lightmapper"), "Lightmapper is abstract");
    assert!(
        instantiate("Lightmapper").is_none(),
        "an abstract class must not instantiate"
    );

    assert!(
        !abstractness("LightmapperRD"),
        "LightmapperRD is GDREGISTER_CLASS — concrete"
    );
    assert!(
        instantiate("LightmapperRD").is_some(),
        "LightmapperRD is what a project actually constructs"
    );
    assert!(!abstractness("LightmapGIData"));
    assert!(
        instantiate("LightmapGIData").is_some(),
        "a .tscn's light_data resource has to be constructible"
    );
}

/// `LightmapGIData`'s members, from the doc XML.
///
/// ⚠️ The bead also lists `uses_spherical_harmonics` as a member. The XML has
/// **no such member** — only the `is_using_spherical_harmonics` /
/// `set_uses_spherical_harmonics` method pair — so registering it as a property
/// would invent surface Godot does not have. This test pins its absence.
#[test]
fn c_lightmapgidata_members_match_the_doc_xml() {
    register_all_classes();

    let names: Vec<String> = get_property_list("LightmapGIData")
        .into_iter()
        .map(|p| p.name)
        .collect();

    for expected in ["light_texture", "lightmap_textures", "shadowmask_textures"] {
        assert!(
            names.contains(&expected.to_string()),
            "{expected}, got {names:?}"
        );
    }
    assert!(
        !names.contains(&"uses_spherical_harmonics".to_string()),
        "that is a METHOD pair in Godot, not a member — got {names:?}"
    );

    let prop = |n: &str| {
        get_property_list("LightmapGIData")
            .into_iter()
            .find(|p| p.name == n)
            .unwrap_or_else(|| panic!("{n} registered"))
    };
    // The two texture arrays default to empty; the single texture is null.
    assert_eq!(
        prop("lightmap_textures").default_value,
        Variant::Array(Default::default())
    );
    assert_eq!(
        prop("shadowmask_textures").default_value,
        Variant::Array(Default::default())
    );
    assert_eq!(
        prop("light_texture").default_value,
        Variant::Nil,
        "a TextureLayered ref with no documented default is null"
    );
}

/// The six methods `LightmapGIData` documents, at their doc-XML arities.
#[test]
fn d_lightmapgidata_methods_and_arities() {
    register_all_classes();

    let info = get_class_info("LightmapGIData").expect("registered");
    let arity = |n: &str| {
        info.methods
            .iter()
            .find(|m| m.name == n)
            .unwrap_or_else(|| panic!("{n} registered"))
            .argument_count
    };

    // ⚠️ `add_user` takes FOUR arguments (path, uv_scale, slice_index,
    // sub_instance) — the obvious guess is one path.
    assert_eq!(arity("add_user"), 4);
    assert_eq!(arity("clear_users"), 0);
    assert_eq!(arity("get_user_count"), 0);
    assert_eq!(arity("get_user_path"), 1);
    assert_eq!(arity("is_using_spherical_harmonics"), 0);
    assert_eq!(arity("set_uses_spherical_harmonics"), 1);
}

/// `LightmapGI.light_data` now has a type that exists.
///
/// ⚠️ The property was already registered; what was missing was anything for it
/// to point at. This is the link the bead is really about.
#[test]
fn e_lightmapgi_light_data_has_a_resolvable_type() {
    register_all_classes();

    assert!(class_exists("LightmapGI"));
    let light_data = get_property_list("LightmapGI")
        .into_iter()
        .find(|p| p.name == "light_data")
        .expect("LightmapGI.light_data was already registered");
    assert_eq!(light_data.getter, "get_light_data");

    assert!(
        class_exists("LightmapGIData"),
        "…and the type it holds now resolves too"
    );
}
