//! pat-nzgdu.208 — the FogMaterial / FogVolume ClassDB surface.
//!
//! Covers the acceptance's third clause (`class_exists('FogMaterial')` and it
//! inherits `Material`), and fixes a parity bug the bead did not ask about:
//! `FogVolume` was parented to `Node3D` where Godot has `VisualInstance3D`.
//!
//! The property binding itself lives in `gdserver3d`
//! (`FogMaterial::from_properties` / `to_properties`): ClassDB and the render
//! structs deliberately do not depend on each other, so they meet at
//! `gdvariant::Variant`, exactly as `ProceduralSkyMaterial` does (pat-nzgdu.161).
//! `gdeditor`'s `fog_volume_from_node_properties` (viewport_3d.rs) parses a
//! `FogVolume` node out of a scene and now drives that binding, so a custom
//! `density`/`albedo` reaches `sample_density` and the rendered frame.
//!
//! Still missing, and NOT claimed here: the runtime path. `gdscene` has no
//! `FogVolume` node type of its own, and `gdrender3d::Renderer::add_fog_volume`
//! has no non-test caller — the parse-to-render path exists only through the
//! editor viewport. See the bead notes.
//!
//! Schemas and defaults diffed against the vendored Godot source:
//! `upstream/godot/doc/classes/FogMaterial.xml` (defaults),
//! `scene/resources/3d/fog_material.cpp` and `scene/3d/fog_volume.cpp`
//! (behaviour), `scene/register_scene_types.cpp` (abstract vs concrete).

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_3d_classes,
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

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// The acceptance's clause: `class_exists('FogMaterial')` and it inherits
/// `Material`.
#[test]
fn fog_material_classdb_binding() {
    register_3d_classes();

    assert!(class_exists("FogMaterial"));
    assert!(
        is_parent_class("FogMaterial", "Material"),
        "the acceptance's chain: FogMaterial <- Material"
    );
    // An unregistered intermediate severs the chain silently (pat-nzgdu.98), so
    // check the whole way up rather than just the immediate parent.
    assert!(
        is_parent_class("FogMaterial", "Resource"),
        "FogMaterial must reach Resource through Material"
    );
    assert!(is_parent_class("FogMaterial", "Object"));

    // ⚠️ Material is VIRTUAL but FogMaterial is CONCRETE:
    // `GDREGISTER_VIRTUAL_CLASS(Material)` vs `GDREGISTER_CLASS(FogMaterial)`
    // (register_scene_types.cpp:439, 742). Marking FogMaterial abstract would
    // make `FogMaterial.new()` fail where Godot allows it.
    assert!(is_abstract("Material"), "Material is virtual in Godot");
    assert!(!can_instantiate("Material"));
    assert!(
        !is_abstract("FogMaterial"),
        "GDREGISTER_CLASS(FogMaterial) — it is instantiable"
    );
    assert!(can_instantiate("FogMaterial"));
    let obj = instantiate("FogMaterial").expect("FogMaterial instantiates");
    assert_eq!(obj.get_class(), "FogMaterial");
}

/// Defaults mirror `FogMaterial::FogMaterial()` (fog_material.cpp:170-179) and
/// the doc XML, which is the only source of property defaults.
#[test]
fn fog_material_defaults_match_godots_constructor() {
    register_3d_classes();

    assert_eq!(default_of("FogMaterial", "density"), Variant::Float(1.0));
    assert_eq!(
        default_of("FogMaterial", "albedo"),
        Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
        "albedo defaults to opaque white"
    );

    // ⚠️ emission is opaque BLACK (0,0,0,1) — NOT transparent (0,0,0,0). Fog
    // emits no light by default, but `Color::TRANSPARENT` is a different colour
    // and the two are easy to conflate when the intent reads as "nothing".
    assert_eq!(
        default_of("FogMaterial", "emission"),
        Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0))
    );
    assert_ne!(
        default_of("FogMaterial", "emission"),
        Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 0.0)),
        "emission must be opaque black, not transparent black"
    );

    assert_eq!(
        default_of("FogMaterial", "height_falloff"),
        Variant::Float(0.0),
        "0.0 = uniform-density fog"
    );
    assert_eq!(default_of("FogMaterial", "edge_fade"), Variant::Float(0.1));
    assert_eq!(
        default_of("FogMaterial", "density_texture"),
        Variant::Nil,
        "the Texture3D is null until assigned"
    );

    // The doc XML lists exactly these six members; an extra own property would
    // be schema fiction.
    let base = props("Material");
    let mut own: Vec<String> = props("FogMaterial")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    own.sort();
    assert_eq!(
        own,
        vec![
            "albedo",
            "density",
            "density_texture",
            "edge_fade",
            "emission",
            "height_falloff"
        ],
        "FogMaterial binds exactly Godot's six members"
    );
}

/// Every member's accessor pair, which is what a script or `.tscn` setter path
/// would call.
#[test]
fn every_member_has_its_godot_accessor_pair() {
    register_3d_classes();

    for member in [
        "density",
        "albedo",
        "emission",
        "height_falloff",
        "edge_fade",
        "density_texture",
    ] {
        let setter = format!("set_{member}");
        let getter = format!("get_{member}");
        assert!(
            class_has_method("FogMaterial", &setter),
            "FogMaterial must declare {setter}"
        );
        assert!(
            class_has_method("FogMaterial", &getter),
            "FogMaterial must declare {getter}"
        );
        assert_eq!(arity("FogMaterial", &setter), 1, "{setter} takes one value");
        assert_eq!(arity("FogMaterial", &getter), 0, "{getter} takes none");
    }
}

/// ⚠️ The parity bug this bead exposed: `FogVolume` was registered bare as a
/// `Node3D`. Godot has `FogVolume <- VisualInstance3D`
/// (doc/classes/FogVolume.xml) — that base is what marks a node as taking part
/// in rendering, so anything walking visual instances silently skipped fog.
#[test]
fn fog_volume_is_a_visual_instance_not_a_bare_node3d() {
    register_3d_classes();

    assert!(class_exists("FogVolume"));
    assert!(
        is_parent_class("FogVolume", "VisualInstance3D"),
        "FogVolume <- VisualInstance3D in Godot; it was parented to Node3D"
    );
    assert!(is_parent_class("FogVolume", "Node3D"));
    assert!(is_parent_class("FogVolume", "Node"));
    assert!(!is_abstract("FogVolume"), "GDREGISTER_CLASS(FogVolume)");

    // Not vacuous: VisualInstance3D really is registered and really does derive
    // Node3D, so the assertion above discriminates rather than passing trivially.
    assert!(class_exists("VisualInstance3D"));
    assert!(is_parent_class("VisualInstance3D", "Node3D"));

    // ⚠️ The material is a RESOURCE and the volume is the NODE — the same split
    // as Mesh vs MeshInstance3D. The volume references one; it is not one.
    assert!(!is_parent_class("FogVolume", "Resource"));
    assert!(!is_parent_class("FogMaterial", "Node"));
    assert!(props("FogVolume").iter().any(|p| p == "material"));
    assert_eq!(
        default_of("FogVolume", "material"),
        Variant::Nil,
        "a fresh FogVolume holds no material until assigned"
    );
}

/// ⚠️ `shape` defaults to **3 (BOX)**, not 0 (Ellipsoid). `FogVolume::FogVolume()`
/// calls `fog_volume_set_shape(volume, RS::FOG_VOLUME_SHAPE_BOX)`
/// (fog_volume.cpp:139) and the doc XML records `default="3"`.
///
/// This is where gdserver3d's backend used to diverge: `FogVolume::default()`
/// took `FogVolumeShape::default()` = Ellipsoid(0), so a stock `.tscn` fog
/// volume (which carries no `shape` key) rendered an ellipsoid. Both sides now
/// say Box; gdscene's `the_node_default_shape_is_box_on_both_sides` is what
/// keeps them in step, since the two crates cannot see each other. This test
/// pins only the ClassDB side.
#[test]
fn fog_volume_defaults_match_godot_including_the_box_shape() {
    register_3d_classes();

    assert_eq!(
        default_of("FogVolume", "shape"),
        Variant::Int(3),
        "Godot's FogVolume constructor sets FOG_VOLUME_SHAPE_BOX (3)"
    );
    assert_ne!(
        default_of("FogVolume", "shape"),
        Variant::Int(0),
        "0 is Ellipsoid — the enum's zero value, which is NOT the node default"
    );
    match default_of("FogVolume", "size") {
        Variant::Vector3(v) => assert!(
            (v.x - 2.0).abs() < 1e-6 && (v.y - 2.0).abs() < 1e-6 && (v.z - 2.0).abs() < 1e-6,
            "FogVolume.size defaults to Vector3(2,2,2), got {v:?}"
        ),
        other => panic!("FogVolume.size must be a Vector3, got {other:?}"),
    }

    for (m, n) in [
        ("set_material", 1),
        ("get_material", 0),
        ("set_shape", 1),
        ("get_shape", 0),
        ("set_size", 1),
        ("get_size", 0),
    ] {
        assert_eq!(arity("FogVolume", m), n, "{m} arity");
    }
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes — and FogVolume's registration was previously
    // unguarded, so a second pass minted it a fresh one.
    let ids = || -> Vec<_> {
        ["FogMaterial", "FogVolume", "Material"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_3d_classes();
    let first = ids();
    register_3d_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
