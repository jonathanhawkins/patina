//! pat-nzgdu.213 — the ten CompositorEffect render-data classes are
//! ClassDB-registered with Godot's inheritance and method surface.
//!
//! **Registration only, and the bead says so itself** ("Advanced/exotic —
//! depends on RenderingDevice and CompositorEffect being implemented first.
//! Register the class surface and, *once CompositorEffect exists*, populate
//! these with real per-frame RD buffers"). The acceptance's other clauses — a
//! callback receiving buffers whose `get_color_texture()` returns a valid RID,
//! and an app-level compositor render — are blocked on three things that do not
//! exist, not on effort:
//!
//! 1. **No `RenderingDevice`.** Registered as a bare class; the device is not
//!    implemented and is not reachable from script (pat-nzgdu.162).
//! 2. **No `RID` in `Variant`.** Every texture getter here returns an `RID`, and
//!    `gdvariant::binary` names `RID` among the types the enum lacks
//!    (pat-nzgdu.210). There is no value a callback could hand back.
//! 3. **No `CompositorEffect`, and no script-backed Resources.** The acceptance
//!    needs "a CompositorEffect subclass" — a *script* subclassing a Resource —
//!    and `attach_script` takes a `NodeId`. Scripts cannot back non-Node objects
//!    at all yet, which blocks this epic-wide.
//!
//! Schema **generated** from Godot 4.6.1.stable's `extension_api.json` rather
//! than transcribed: 73 methods with arities from 0 to 10 is where hand-copying
//! invents errors (the pat-nzgdu.204 lesson). Types/defaults for
//! `RenderSceneBuffersConfiguration` come from the doc XML — see the test below
//! for why the api dump cannot be trusted for those.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_viewport_classes,
};
use gdvariant::Variant;

/// The bead's ten classes.
const FAMILY: &[&str] = &[
    "RenderData",
    "RenderDataExtension",
    "RenderDataRD",
    "RenderSceneData",
    "RenderSceneDataExtension",
    "RenderSceneDataRD",
    "RenderSceneBuffers",
    "RenderSceneBuffersExtension",
    "RenderSceneBuffersRD",
    "RenderSceneBuffersConfiguration",
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// ⚠️ `.rev()`: `get_property_list` is base-first, so a plain `.find()` returns
/// the most-BASE match on a shadowed name while `instantiate` lets the derived
/// default win.
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

/// Methods `class` declares that `base` does not.
fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

/// The acceptance's checkable clause: all ten registered, with Godot's chains.
#[test]
fn compositor_effect_render_scene_buffers_test() {
    register_viewport_classes();

    for class in FAMILY {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object — an unregistered intermediate severs the \
             chain silently (pat-nzgdu.98)"
        );
    }

    // ⚠️ The three families do NOT share a base. RenderData/RenderSceneData hang
    // off `Object`; the buffers hang off `RefCounted` — they are shared
    // per-viewport and outlive a callback, where the render data does not.
    for class in ["RenderData", "RenderSceneData"] {
        assert!(
            !is_parent_class(class, "RefCounted"),
            "{class} is a plain Object, not RefCounted"
        );
    }
    for class in [
        "RenderSceneBuffers",
        "RenderSceneBuffersRD",
        "RenderSceneBuffersConfiguration",
    ] {
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} <- RefCounted"
        );
    }

    // Each variant parents to its own family's base.
    assert!(is_parent_class("RenderDataExtension", "RenderData"));
    assert!(is_parent_class("RenderDataRD", "RenderData"));
    assert!(is_parent_class(
        "RenderSceneDataExtension",
        "RenderSceneData"
    ));
    assert!(is_parent_class("RenderSceneDataRD", "RenderSceneData"));
    assert!(is_parent_class(
        "RenderSceneBuffersExtension",
        "RenderSceneBuffers"
    ));
    assert!(is_parent_class(
        "RenderSceneBuffersRD",
        "RenderSceneBuffers"
    ));

    // ⚠️ RenderSceneBuffersConfiguration is NOT a RenderSceneBuffers — it
    // configures one. The shared prefix invites parenting it to the family.
    assert!(
        !is_parent_class("RenderSceneBuffersConfiguration", "RenderSceneBuffers"),
        "the Configuration is a sibling, not a subclass"
    );
}

/// ⚠️ The three bases are ABSTRACT; every variant is CONCRETE. You never
/// construct a bare `RenderData` — the renderer hands you an `RD` one, or a
/// GDExtension supplies an `Extension` one.
#[test]
fn the_bases_are_abstract_and_the_variants_are_not() {
    register_viewport_classes();

    for base in ["RenderData", "RenderSceneData", "RenderSceneBuffers"] {
        assert!(is_abstract(base), "{base} is abstract in Godot");
        assert!(!can_instantiate(base));
    }

    for class in [
        "RenderDataExtension",
        "RenderDataRD",
        "RenderSceneDataExtension",
        "RenderSceneDataRD",
        "RenderSceneBuffersExtension",
        "RenderSceneBuffersRD",
        "RenderSceneBuffersConfiguration",
    ] {
        assert!(!is_abstract(class), "{class} IS instantiable in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }
}

/// ⚠️ `RenderDataRD` and `RenderSceneDataRD` bind **NOTHING** — their entire
/// surface is inherited. Adding a method to either would invent a ClassDB entry
/// Godot does not have; the temptation is real, because they are the classes an
/// effect actually receives.
#[test]
fn the_rd_variants_bind_nothing_and_inherit_everything() {
    register_viewport_classes();

    assert!(
        own_methods("RenderDataRD", "RenderData").is_empty(),
        "RenderDataRD declares no methods of its own"
    );
    assert!(
        own_methods("RenderSceneDataRD", "RenderSceneData").is_empty(),
        "RenderSceneDataRD declares no methods of its own"
    );

    // ...yet the inherited surface is reachable, which is the whole point: the
    // camera matrices the acceptance asks about come from the BASE.
    for m in ["get_cam_transform", "get_cam_projection", "get_view_count"] {
        assert!(
            class_has_method("RenderSceneDataRD", m),
            "RenderSceneDataRD must reach {m} through RenderSceneData"
        );
    }
    for m in ["get_render_scene_buffers", "get_render_scene_data"] {
        assert!(class_has_method("RenderDataRD", m));
    }
}

/// ⚠️ `RenderSceneBuffersExtension` is NOT a 1:1 mirror of its base, unlike the
/// other two Extensions.
///
/// `RenderData`/`RenderSceneData` each expose N public getters and their
/// Extension mirrors exactly N `_`-prefixed virtuals. The buffers break the
/// pattern: the base binds **one** method (`configure`), the Extension binds
/// **five** — four `_set_*` hooks have no public counterpart at all. Assuming
/// the mirror would silently drop them.
#[test]
fn the_extension_mirror_pattern_holds_except_for_the_buffers() {
    register_viewport_classes();

    // The mirror holds here: N public getters -> N underscore virtuals.
    for (base, ext) in [
        ("RenderData", "RenderDataExtension"),
        ("RenderSceneData", "RenderSceneDataExtension"),
    ] {
        let base_own = own_methods(base, "Object");
        let ext_own = own_methods(ext, base);
        assert_eq!(
            base_own.len(),
            ext_own.len(),
            "{ext} mirrors {base} one-for-one"
        );
        for m in &base_own {
            assert!(
                ext_own.contains(&format!("_{m}")),
                "{ext} must declare _{m} for {base}::{m}"
            );
        }
        // Every Extension method is a virtual — nothing callable directly.
        for m in &ext_own {
            assert!(m.starts_with('_'), "{ext}::{m} must be a virtual");
        }
    }

    // ...and breaks here.
    let mut buf_ext = own_methods("RenderSceneBuffersExtension", "RenderSceneBuffers");
    buf_ext.sort();
    assert_eq!(
        buf_ext,
        vec![
            "_configure",
            "_set_anisotropic_filtering_level",
            "_set_fsr_sharpness",
            "_set_texture_mipmap_bias",
            "_set_use_debanding",
        ],
        "the buffers Extension binds five virtuals over a one-method base"
    );
    assert_eq!(
        own_methods("RenderSceneBuffers", "RefCounted"),
        vec!["configure"],
        "the base itself binds only configure"
    );
}

/// The class the acceptance names. ⚠️ `get_color_texture`/`get_depth_texture`
/// take an ARGUMENT (the MSAA flag) — they are not the zero-arg getters the
/// acceptance's `get_color_texture()` spelling implies.
#[test]
fn the_buffers_expose_godots_texture_surface_with_real_arities() {
    register_viewport_classes();

    assert_eq!(
        own_methods("RenderSceneBuffersRD", "RenderSceneBuffers").len(),
        27,
        "Godot binds 27 methods on RenderSceneBuffersRD"
    );

    // The acceptance's two, with their real arities.
    assert_eq!(arity("RenderSceneBuffersRD", "get_color_texture"), 1);
    assert_eq!(arity("RenderSceneBuffersRD", "get_depth_texture"), 1);
    assert_eq!(arity("RenderSceneBuffersRD", "get_velocity_texture"), 1);

    // Spot-check the arities a transcription would most plausibly get wrong.
    assert_eq!(arity("RenderSceneBuffersRD", "create_texture"), 10);
    assert_eq!(arity("RenderSceneBuffersRD", "get_texture_slice_view"), 7);
    assert_eq!(arity("RenderSceneBuffersRD", "get_texture_slice"), 6);
    assert_eq!(arity("RenderSceneBuffersRD", "get_render_target"), 0);
}

/// ⚠️ The api dump LIES about two of these property types, so the doc XML is the
/// oracle here.
///
/// `extension_api.json` reports `fsr_sharpness` and `texture_mipmap_bias` as
/// `bool`, while its own `set_fsr_sharpness(float)` signature — and the XML's
/// `type="float" default="0.0"` — say float. The dump is internally
/// inconsistent; trusting its `properties[].type` blindly would have registered
/// two booleans.
#[test]
fn the_configuration_defaults_come_from_the_doc_xml_not_the_dump() {
    register_viewport_classes();

    // The two the dump gets wrong.
    assert_eq!(
        default_of("RenderSceneBuffersConfiguration", "fsr_sharpness"),
        Variant::Float(0.0),
        "fsr_sharpness is a float, whatever the api dump's property type says"
    );
    assert_eq!(
        default_of("RenderSceneBuffersConfiguration", "texture_mipmap_bias"),
        Variant::Float(0.0)
    );

    // ⚠️ Non-zero defaults: each of these would be a real, wrong setting if
    // defaulted to 0.
    assert_eq!(
        default_of("RenderSceneBuffersConfiguration", "view_count"),
        Variant::Int(1),
        "0 views would allocate nothing"
    );
    assert_eq!(
        default_of("RenderSceneBuffersConfiguration", "scaling_3d_mode"),
        Variant::Int(255),
        "255 is the unset sentinel; 0 is BILINEAR — a real mode nobody chose"
    );
    assert_eq!(
        default_of(
            "RenderSceneBuffersConfiguration",
            "anisotropic_filtering_level"
        ),
        Variant::Int(2),
        "Godot defaults to ANISOTROPY_4X"
    );

    // The genuinely-zero ones, so the assertions above are not just "non-zero".
    assert_eq!(
        default_of("RenderSceneBuffersConfiguration", "msaa_3d"),
        Variant::Int(0)
    );
    assert_eq!(
        default_of("RenderSceneBuffersConfiguration", "screen_space_aa"),
        Variant::Int(0)
    );
}

/// ⚠️ A KNOWN GAP, pinned so it is not mistaken for a correct value.
///
/// `render_target` is an `RID` and `internal_size`/`target_size` are `Vector2i`.
/// `Variant` carries **neither** — `gdvariant::binary` names both among the
/// types the enum lacks. They register as `Nil`: the property exists, so
/// introspection matches Godot, but the value is absent rather than a `Vector2`
/// standing in for a `Vector2i`, which would make `typeof()` report the wrong
/// type forever.
///
/// When `Variant` grows `Vector2i`/`RID`, this test should fail and be replaced
/// with the real defaults (`Vector2i(0, 0)` and an invalid RID).
#[test]
fn known_gap_rid_and_vector2i_members_have_no_representable_default() {
    register_viewport_classes();

    // The members exist — introspection must agree with Godot even though the
    // values cannot.
    let own = props("RenderSceneBuffersConfiguration");
    for name in ["render_target", "internal_size", "target_size"] {
        assert!(own.iter().any(|p| p == name), "{name} must be registered");
        assert_eq!(
            default_of("RenderSceneBuffersConfiguration", name),
            Variant::Nil,
            "{name}'s type (RID/Vector2i) is not representable in Variant — Nil \
             is the documented gap, not a value"
        );
    }

    // ⚠️ Not a Vector2 masquerading as a Vector2i.
    assert_ne!(
        default_of("RenderSceneBuffersConfiguration", "internal_size"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        "a Vector2 must NOT stand in for a Vector2i — typeof() would lie"
    );

    // All ten members are registered, gap or not.
    assert_eq!(
        props("RenderSceneBuffersConfiguration")
            .into_iter()
            .filter(|p| !props("RefCounted").contains(p))
            .count(),
        10,
        "Godot binds ten members on RenderSceneBuffersConfiguration"
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        FAMILY
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
