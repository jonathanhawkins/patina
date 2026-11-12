//! pat-s3rrv.203: `CanvasTexture` is ClassDB-registered with Godot's seven
//! members.
//!
//! ⚠️ An unregistered class has no property list, so a `.tscn` authoring
//! `normal_texture` has nothing to round-trip through and the inspector shows
//! nothing to edit — the resource is unreachable rather than merely undecorated.
//!
//! Every member name, default and accessor below is transcribed from
//! `upstream/godot/doc/classes/CanvasTexture.xml`. That class declares **no
//! `<method>` elements at all** — its entire API is the seven members'
//! `setter=` / `getter=` attributes — so those attributes are the only source
//! of truth for the accessor names, and nothing about the property names
//! recovers them.

use gdobject::class_db::{
    class_exists, get_class_info, get_property_getter, get_property_setter, is_parent_class,
    register_all_classes,
};
use gdvariant::Variant;

/// Godot's seven `CanvasTexture` members, in doc order.
const MEMBERS: [&str; 7] = [
    "diffuse_texture",
    "normal_texture",
    "specular_texture",
    "specular_color",
    "specular_shininess",
    "texture_filter",
    "texture_repeat",
];

fn default_of(name: &str) -> Variant {
    register_all_classes();
    get_class_info("CanvasTexture")
        .expect("CanvasTexture is registered")
        .properties
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("CanvasTexture must declare `{name}`"))
        .default_value
}

/// The acceptance's first half: the class exists and sits under `Texture2D`.
#[test]
fn canvas_texture_is_registered_under_texture_2d() {
    register_all_classes();
    assert!(
        class_exists("CanvasTexture"),
        "CanvasTexture must be registered"
    );
    assert!(
        is_parent_class("CanvasTexture", "Texture2D"),
        "CanvasTexture inherits Texture2D — anything that accepts a Texture2D \
         must accept one, which is the whole point of the class"
    );
}

/// The acceptance's second half: the property list matches Godot's members.
#[test]
fn all_seven_members_are_declared() {
    register_all_classes();
    let names: Vec<String> = get_class_info("CanvasTexture")
        .expect("CanvasTexture is registered")
        .properties
        .into_iter()
        .map(|p| p.name)
        .collect();

    for member in MEMBERS {
        assert!(
            names.iter().any(|n| n == member),
            "CanvasTexture must declare `{member}`; got {names:?}"
        );
    }
}

/// ⚠️ **Assert the accessor NAMES, not just that the property exists.**
///
/// `class_has_property` is satisfied by a duplicate declaration, and ClassDB
/// resolves a duplicate to the LAST one silently rather than erroring — so a
/// second, emptier `CanvasTexture` registration elsewhere in the file would
/// shadow this one and every existence check would still pass. The accessor
/// name is what distinguishes them.
#[test]
fn every_member_records_its_real_accessors() {
    register_all_classes();
    for member in MEMBERS {
        // Godot's accessors are uniformly `get_<member>` / `set_<member>` for
        // this class — but that is an observation from the XML, not a rule the
        // codebase may derive: sibling classes break the pattern
        // (`is_mode_overriding_title`, `get_alpha_cut_mode`).
        let expected_getter = format!("get_{member}");
        let expected_setter = format!("set_{member}");
        assert_eq!(
            get_property_getter("CanvasTexture", member),
            expected_getter,
            "`{member}` must record its documented getter; an empty string here \
             means the property was registered without accessors, and a \
             different name means a duplicate registration shadowed this one"
        );
        assert_eq!(
            get_property_setter("CanvasTexture", member),
            expected_setter,
            "`{member}` must record its documented setter"
        );
    }
}

/// The two scalar defaults, transcribed rather than guessed.
///
/// 🔑 `specular_shininess = 1.0` is the one worth pinning: it is **not** an
/// exponent. The canvas shader turns it into one via
/// `exp2(15 * s + 1) * 0.25` (canvas.glsl:432), so this innocuous-looking 1.0
/// is a Blinn exponent of 16384. Anyone "correcting" it to a plausible
/// exponent like 32 would change every specular pixel.
#[test]
fn the_scalar_defaults_match_the_documentation() {
    match default_of("specular_color") {
        Variant::Color(c) => assert!(
            c.r == 1.0 && c.g == 1.0 && c.b == 1.0 && c.a == 1.0,
            "doc default is Color(1, 1, 1, 1); got {c:?}"
        ),
        other => panic!("specular_color must be a Color, got {other:?}"),
    }
    match default_of("specular_shininess") {
        Variant::Float(f) => assert!((f - 1.0).abs() < 1e-9, "doc default is 1.0; got {f}"),
        other => panic!("specular_shininess must be a Float, got {other:?}"),
    }
}

/// The three texture channels have no documented default, and that absence is
/// load-bearing: a null channel is what makes the normal-mapped path opt-in.
///
/// If these ever gained a non-null default, every CanvasTexture would claim to
/// carry a normal map and the light pass would shade flat surfaces as though
/// they had relief.
#[test]
fn the_texture_channels_default_to_null() {
    for channel in ["diffuse_texture", "normal_texture", "specular_texture"] {
        assert!(
            matches!(default_of(channel), Variant::Nil),
            "`{channel}` has no documented default — an unset channel must stay \
             null so the normal-mapped path remains opt-in"
        );
    }
}

/// `texture_filter` / `texture_repeat` are CanvasItem enums whose default is
/// index 0, and they must arrive as integers so an authored `.tscn` value maps
/// straight onto the enum.
#[test]
fn the_enum_members_default_to_zero_as_ints() {
    for member in ["texture_filter", "texture_repeat"] {
        assert!(
            matches!(default_of(member), Variant::Int(0)),
            "`{member}` is an int-valued enum defaulting to 0; a String or Nil \
             here means an authored value cannot map onto the enum"
        );
    }
}
