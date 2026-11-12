//! pat-nzgdu.161 — `ProceduralSkyMaterial` renders (gdserver3d::sky) but was
//! invisible to ClassDB. It must now be registered (Material → Resource) with
//! its documented properties + defaults, instantiable/introspectable like any
//! Material, and setting `sky_top_color` via the property path must reach the
//! render struct. (Lives in gdscene because it needs both gdobject's ClassDB and
//! gdserver3d's ProceduralSkyMaterial.)

use gdcore::math::Color;
use gdobject::class_db;
use gdserver3d::ProceduralSkyMaterial;
use gdvariant::Variant;

#[test]
fn procedural_sky_material_classdb_registered() {
    class_db::register_3d_classes();

    // (1) Registered with the Material -> Resource parent chain.
    assert!(
        class_db::class_exists("ProceduralSkyMaterial"),
        "ProceduralSkyMaterial must be registered in ClassDB"
    );
    assert!(class_db::is_parent_class(
        "ProceduralSkyMaterial",
        "Material"
    ));
    assert!(class_db::is_parent_class(
        "ProceduralSkyMaterial",
        "Resource"
    ));

    // (2) Instantiable, and the registered property defaults match the render
    // struct's `default()` (get_property_list exposes the documented surface).
    assert!(
        class_db::instantiate("ProceduralSkyMaterial").is_some(),
        "ClassDB must be able to instantiate ProceduralSkyMaterial"
    );
    let props = class_db::get_property_list("ProceduralSkyMaterial");
    let def = ProceduralSkyMaterial::default();
    let default_of = |name: &str| {
        props
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.default_value.clone())
    };

    match default_of("sky_top_color") {
        Some(Variant::Color(c)) => assert!(
            (c.r - def.sky_top_color.r).abs() < 1e-4 && (c.g - def.sky_top_color.g).abs() < 1e-4,
            "sky_top_color default must match ProceduralSkyMaterial::default(), got {c:?}"
        ),
        other => panic!("sky_top_color property missing/wrong: {other:?}"),
    }
    match default_of("sky_curve") {
        Some(Variant::Float(v)) => {
            assert!(
                (v as f32 - def.sky_curve).abs() < 1e-4,
                "sky_curve default, got {v}"
            )
        }
        other => panic!("sky_curve property missing/wrong: {other:?}"),
    }
    // The rest of the documented property surface is exposed.
    for name in [
        "sky_horizon_color",
        "sky_energy_multiplier",
        "ground_bottom_color",
        "ground_horizon_color",
        "ground_curve",
        "ground_energy_multiplier",
        "sun_angle_max",
        "sun_curve",
        "sky_cover_modulate",
    ] {
        assert!(
            class_db::class_has_property("ProceduralSkyMaterial", name),
            "ClassDB must expose the `{name}` property"
        );
    }

    // (3) Setting sky_top_color via the property path is reflected in the render
    // struct (`from_properties` maps ClassDB props onto the sky material).
    let red = Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0));
    let mat = ProceduralSkyMaterial::from_properties([("sky_top_color", &red)].into_iter());
    assert!(
        mat.sky_top_color.r > 0.9 && mat.sky_top_color.g < 0.1 && mat.sky_top_color.b < 0.1,
        "sky_top_color override must reach the material, got {:?}",
        mat.sky_top_color
    );
}
