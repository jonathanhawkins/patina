//! pat-nzgdu.225 — the cubemap / curve-texture VisualShader nodes and the
//! `CurveTexture`/`CurveXYZTexture` resources they sample.
//!
//! ⚠️ **The bead's premise is stale and its enum is invented.**
//! - *"ClassDB has no shader classes"* — **49 `VisualShader*` classes are already
//!   registered**, including `VisualShaderNodeCubemapParameter`, and `Cubemap`
//!   landed in pat-nzgdu.215. The real gaps were the six registered here.
//! - *"source enum TEXTURE/PORT/SCREEN"* — **there is no `SOURCE_SCREEN`.** Godot
//!   has `SOURCE_TEXTURE`, `SOURCE_PORT` and a `SOURCE_MAX` sentinel. Registering
//!   a SCREEN would invent a value a script could pass and nothing could honour.
//!   (`texture_type` is likewise `TYPE_NORMAL_MAP`, not "NORMAL".)
//!
//! **`generate_code()` does not exist anywhere in the workspace**, so the
//! acceptance's "emits the cubemap sample and curve texture sample expressions
//! matching a Godot reference fixture" has nothing to call — there is no
//! VisualShader codegen at all, for any node. See the bead notes.
//!
//! Schemas, enums and defaults diffed against Godot 4.6.1.stable's
//! `extension_api.json` + doc XML; abstractness against the `GDREGISTER_*` macros.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, class_has_method, get_method_list,
    get_property_list, instantiate, is_parent_class, register_3d_classes,
    register_viewport_classes,
};
use gdvariant::Variant;

/// The nodes live in the 3D pass with the VisualShader family; the Curve
/// textures live in the viewport pass with `Texture2D`, their parent.
fn register_all() {
    register_3d_classes();
    register_viewport_classes();
}

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// ⚠️ `.rev()`: `get_property_list` is base-first; `size` is shadowed along these
/// chains, so a plain `.find()` would return the wrong one.
fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .rev()
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

fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

/// The acceptance's parent-chain clause, and its round-trip clause.
#[test]
fn visual_shader_texture_nodes_test() {
    register_all();

    // "get_parent_class('VisualShaderNodeCubemap') == 'VisualShaderNode'"
    assert!(class_exists("VisualShaderNodeCubemap"));
    assert_eq!(
        gdobject::get_class_info("VisualShaderNodeCubemap")
            .expect("registered")
            .parent_class,
        "VisualShaderNode",
        "the acceptance's clause, on the immediate parent"
    );

    // "...and ('VisualShaderNodeCurveTexture') == 'VisualShaderNodeResizableBase'"
    for node in [
        "VisualShaderNodeCurveTexture",
        "VisualShaderNodeCurveXYZTexture",
    ] {
        assert!(class_exists(node));
        assert_eq!(
            gdobject::get_class_info(node)
                .expect("registered")
                .parent_class,
            "VisualShaderNodeResizableBase",
            "{node} derives the RESIZABLE base, not VisualShaderNode directly"
        );
        assert!(is_parent_class(node, "VisualShaderNode"));
        assert!(is_parent_class(node, "Resource"), "{node} reaches Resource");
    }

    // ⚠️ The acceptance's "setting Cubemap.source and CurveTexture.texture
    // round-trips". ClassDB has no call dispatch, so the property is the
    // observable: `instantiate` applies the default, set/get round-trips it.
    let mut cubemap = instantiate("VisualShaderNodeCubemap").expect("it instantiates");
    assert_eq!(cubemap.get_class(), "VisualShaderNodeCubemap");
    assert_eq!(
        cubemap.get_property("source"),
        Variant::Int(0),
        "SOURCE_TEXTURE is the default"
    );
    cubemap.set_property("source", Variant::Int(1)); // SOURCE_PORT
    assert_eq!(cubemap.get_property("source"), Variant::Int(1));

    let mut curve = instantiate("VisualShaderNodeCurveTexture").expect("it instantiates");
    assert_eq!(
        curve.get_property("texture"),
        Variant::Nil,
        "no CurveTexture assigned until one is"
    );
    curve.set_property("texture", Variant::Int(42)); // stand-in for a resource ref
    assert_eq!(curve.get_property("texture"), Variant::Int(42));
}

/// ⚠️ THE INVENTED ENUM VALUE. The bead says the source enum is
/// "TEXTURE/PORT/SCREEN". Godot has no SCREEN.
#[test]
fn the_source_enum_has_no_screen_value() {
    register_all();

    assert!(class_has_enum("VisualShaderNodeCubemap", "Source"));
    for (name, value) in [("SOURCE_TEXTURE", 0), ("SOURCE_PORT", 1), ("SOURCE_MAX", 2)] {
        assert_eq!(
            class_get_integer_constant("VisualShaderNodeCubemap", name),
            Some(value),
            "{name} must be readable from script"
        );
    }
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeCubemap", "SOURCE_SCREEN"),
        None,
        "SOURCE_SCREEN does not exist in Godot — the bead invented it"
    );

    // ⚠️ TYPE_NORMAL_MAP, not "NORMAL".
    assert!(class_has_enum("VisualShaderNodeCubemap", "TextureType"));
    for (name, value) in [
        ("TYPE_DATA", 0),
        ("TYPE_COLOR", 1),
        ("TYPE_NORMAL_MAP", 2),
        ("TYPE_MAX", 3),
    ] {
        assert_eq!(
            class_get_integer_constant("VisualShaderNodeCubemap", name),
            Some(value)
        );
    }
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeCubemap", "TYPE_NORMAL"),
        None,
        "the value is TYPE_NORMAL_MAP; TYPE_NORMAL is the bead's spelling"
    );
}

/// ⚠️ `VisualShaderNodeCubemap` is NOT a `VisualShaderNodeCubemapParameter`.
///
/// A *parameter* declares a uniform the material exposes; this *samples* a
/// cubemap the node owns. Same noun, unrelated jobs — they share only
/// `VisualShaderNode`. The pre-existing CubemapParameter registration makes this
/// an easy mis-parent.
#[test]
fn the_sampler_is_not_the_parameter() {
    register_all();

    assert!(!is_parent_class(
        "VisualShaderNodeCubemap",
        "VisualShaderNodeCubemapParameter"
    ));
    assert!(!is_parent_class(
        "VisualShaderNodeCubemap",
        "VisualShaderNodeParameter"
    ));

    // Not vacuous: the parameter really is registered and really does derive the
    // parameter base, so the split above is a real distinction.
    assert!(class_exists("VisualShaderNodeCubemapParameter"));
    assert!(is_parent_class(
        "VisualShaderNodeCubemapParameter",
        "VisualShaderNodeParameter"
    ));
}

/// ⚠️ The resizable base is ABSTRACT; the three nodes are concrete.
#[test]
fn the_resizable_base_is_abstract() {
    register_all();

    assert!(
        is_abstract("VisualShaderNodeResizableBase"),
        "GDREGISTER_ABSTRACT_CLASS — it exists to give resizable nodes a size"
    );
    assert!(!can_instantiate("VisualShaderNodeResizableBase"));
    assert_eq!(
        default_of("VisualShaderNodeResizableBase", "size"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        "Vector2(0, 0) — the editor sizes it on placement"
    );

    for node in [
        "VisualShaderNodeCubemap",
        "VisualShaderNodeCurveTexture",
        "VisualShaderNodeCurveXYZTexture",
    ] {
        assert!(!is_abstract(node), "{node} is GDREGISTER_CLASS");
        assert!(can_instantiate(node));
    }
}

/// The Curve texture resources. ⚠️ `width` is **256**, not 0 — they bake a curve
/// into a texture of that many samples, and a zero-width lookup samples nothing.
#[test]
fn the_curve_textures_bake_at_256_samples_by_default() {
    register_all();

    for tex in ["CurveTexture", "CurveXYZTexture"] {
        assert!(class_exists(tex));
        assert!(
            is_parent_class(tex, "Texture2D"),
            "{tex} <- Texture2D — it IS a texture, not a curve"
        );
        assert!(can_instantiate(tex));
        assert_eq!(
            default_of(tex, "width"),
            Variant::Int(256),
            "{tex}.width bakes 256 samples by default, not 0"
        );
        // ⚠️ set_width is bound but get_width is NOT — the getter is Texture2D's.
        assert!(class_has_method(tex, "set_width"));
        assert!(
            class_has_method(tex, "get_width"),
            "{tex} reaches get_width through Texture2D"
        );
        assert_eq!(arity(tex, "set_width"), 1);
    }

    // ⚠️ CurveXYZTexture has THREE curves, one per axis — and no texture_mode at
    // all, unlike its sibling.
    for p in ["curve_x", "curve_y", "curve_z"] {
        assert!(props("CurveXYZTexture").iter().any(|x| x == p));
    }
    assert!(
        !props("CurveXYZTexture").iter().any(|x| x == "texture_mode"),
        "CurveXYZTexture has no texture_mode — only CurveTexture does"
    );
    assert!(props("CurveTexture").iter().any(|x| x == "texture_mode"));
    assert_eq!(
        default_of("CurveTexture", "texture_mode"),
        Variant::Int(0),
        "TEXTURE_MODE_RGB"
    );
    for (name, value) in [("TEXTURE_MODE_RGB", 0), ("TEXTURE_MODE_RED", 1)] {
        assert_eq!(
            class_get_integer_constant("CurveTexture", name),
            Some(value)
        );
    }
}

/// Each node binds only its own accessor pair; the rest is inherited.
#[test]
fn the_nodes_bind_only_their_own_accessors() {
    register_all();

    let mut cube = own_methods("VisualShaderNodeCubemap", "VisualShaderNode");
    cube.sort();
    assert_eq!(
        cube,
        vec![
            "get_cube_map",
            "get_source",
            "get_texture_type",
            "set_cube_map",
            "set_source",
            "set_texture_type",
        ]
    );

    for node in [
        "VisualShaderNodeCurveTexture",
        "VisualShaderNodeCurveXYZTexture",
    ] {
        let mut own = own_methods(node, "VisualShaderNodeResizableBase");
        own.sort();
        assert_eq!(
            own,
            vec!["get_texture", "set_texture"],
            "{node} binds only its texture accessors"
        );
        // ...and reaches the resizable base's size, which is why it derives it.
        assert!(class_has_method(node, "set_size"));
        assert!(props(node).iter().any(|p| p == "size"));
    }
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        [
            "VisualShaderNodeCubemap",
            "VisualShaderNodeResizableBase",
            "VisualShaderNodeCurveTexture",
            "VisualShaderNodeCurveXYZTexture",
            "CurveTexture",
            "CurveXYZTexture",
        ]
        .iter()
        .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
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
