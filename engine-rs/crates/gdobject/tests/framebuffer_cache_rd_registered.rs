//! pat-nzgdu.210 — `FramebufferCacheRD` (and its parameter type
//! `RDFramebufferPass`) are ClassDB-registered.
//!
//! Extends the RD\* schema surface from pat-nzgdu.162, and like that bead this
//! is deliberately the *schema* half.
//!
//! **On the acceptance's second clause — "get_cache_multipass returns a defined
//! result ... without panicking":** ClassDB itself cannot answer it. ClassDB is
//! schema-only — `MethodInfo` is a name plus an arity with no function pointer,
//! and `GodotObject` has no `call`. The call path a script actually takes is the
//! interpreter's static-dispatch chain (`gdscript-interop`), which now has a
//! `FramebufferCacheRD` arm returning the documented unsupported value; before
//! it, the call raised `UndefinedVariable("FramebufferCacheRD")` — the class
//! resolved for reflection while the call did not.
//!
//! ⚠️ The "real cache RID" half stays unimplementable, and not for want of
//! effort: Patina's `Variant` has **no `RID` variant** (`gdvariant::binary`
//! lists `RID` among the types the enum lacks) and there is no RenderingDevice
//! to cache anything (pat-nzgdu.162). `Nil` is the only value it can honestly
//! name. See the bead notes.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_get_integer_constant, class_has_method, get_method_list, get_property_list,
    is_parent_class, register_viewport_classes,
};
use gdvariant::Variant;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// ⚠️ `.rev()`: `get_property_list` is base-first, so a plain `.find()` returns
/// the most-BASE match on a shadowed name while `instantiate` lets the derived
/// default win. Reverse agrees with what an instance actually gets.
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

/// The acceptance's checkable clause.
#[test]
fn framebuffer_cache_rd_registered() {
    register_viewport_classes();

    assert!(class_exists("FramebufferCacheRD"));
    // ⚠️ Object, NOT RefCounted — unlike every RD* class beside it. The RD*
    // types are parameters you fill in and hand to the device; this is a helper
    // that hands a cached RID back. Same prefix family, different base.
    assert!(
        is_parent_class("FramebufferCacheRD", "Object"),
        "FramebufferCacheRD <- Object"
    );
    assert!(
        !is_parent_class("FramebufferCacheRD", "RefCounted"),
        "it is NOT RefCounted, despite sitting among the RD* classes"
    );
    assert!(!is_parent_class("FramebufferCacheRD", "Resource"));

    // `GDREGISTER_CLASS(FramebufferCacheRD)` — register_server_types.cpp:254.
    assert!(!is_abstract("FramebufferCacheRD"));
    assert!(can_instantiate("FramebufferCacheRD"));

    // Not vacuous: RefCounted really is registered and really does derive
    // Object, so the `!is_parent_class(.., "RefCounted")` above discriminates.
    assert!(class_exists("RefCounted"));
    assert!(is_parent_class("RefCounted", "Object"));
}

/// It binds exactly one method, with Godot's arity.
#[test]
fn it_binds_only_get_cache_multipass_with_three_parameters() {
    register_viewport_classes();

    assert!(class_has_method(
        "FramebufferCacheRD",
        "get_cache_multipass"
    ));
    assert_eq!(
        arity("FramebufferCacheRD", "get_cache_multipass"),
        3,
        "(textures: RID[], passes: RDFramebufferPass[], views: int)"
    );

    // Godot binds exactly one method on this class; anything else is invented.
    let base: Vec<String> = get_method_list("Object")
        .into_iter()
        .map(|m| m.name)
        .collect();
    let own: Vec<String> = get_method_list("FramebufferCacheRD")
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base.contains(m))
        .collect();
    assert_eq!(own, vec!["get_cache_multipass"]);

    // ...and no properties of its own: it is a pure static helper.
    let base_props = props("Object");
    let own_props: Vec<String> = props("FramebufferCacheRD")
        .into_iter()
        .filter(|p| !base_props.contains(p))
        .collect();
    assert!(
        own_props.is_empty(),
        "FramebufferCacheRD binds no properties; found {own_props:?}"
    );
}

/// ⚠️ The method's parameter TYPE must exist too, or the method is unusable: a
/// script cannot construct a `RDFramebufferPass` to pass, so `get_cache_multipass`
/// would resolve and still be uncallable. Registering the method without it is
/// the "reachable but unusable" trap.
#[test]
fn the_parameter_type_rdframebufferpass_is_registered_too() {
    register_viewport_classes();

    assert!(
        class_exists("RDFramebufferPass"),
        "get_cache_multipass takes RDFramebufferPass[] — the type must exist"
    );
    assert!(is_parent_class("RDFramebufferPass", "RefCounted"));
    assert!(is_parent_class("RDFramebufferPass", "Object"));
    assert!(can_instantiate("RDFramebufferPass"));

    let mut own = props("RDFramebufferPass");
    own.sort();
    assert_eq!(
        own,
        vec![
            "color_attachments",
            "depth_attachment",
            "input_attachments",
            "preserve_attachments",
            "resolve_attachments",
        ],
        "Godot binds exactly these five members"
    );

    for name in [
        "color_attachments",
        "input_attachments",
        "resolve_attachments",
        "preserve_attachments",
    ] {
        assert_eq!(
            default_of("RDFramebufferPass", name),
            Variant::PackedInt32Array(Vec::new()),
            "{name} defaults to an empty PackedInt32Array"
        );
    }
}

/// ⚠️ Godot binds an accessor pair per member — 10 methods.
///
/// The RD\* classes registered alongside this one (pat-nzgdu.162) declare
/// properties only, so `class_has_method("RDVertexAttribute", "set_binding")`
/// answers `false` where Godot answers `true`. That is a gap across that family,
/// not a house convention, so this class follows the ground truth instead of
/// copying it. (`SubViewport` and `Texture3DRD` in the same file already
/// register their accessors, so the file is not consistent either way.)
#[test]
fn every_member_has_its_godot_accessor_pair() {
    register_viewport_classes();

    for member in [
        "color_attachments",
        "input_attachments",
        "resolve_attachments",
        "preserve_attachments",
        "depth_attachment",
    ] {
        let setter = format!("set_{member}");
        let getter = format!("get_{member}");
        assert!(
            class_has_method("RDFramebufferPass", &setter),
            "RDFramebufferPass must declare {setter}"
        );
        assert!(
            class_has_method("RDFramebufferPass", &getter),
            "RDFramebufferPass must declare {getter}"
        );
        assert_eq!(arity("RDFramebufferPass", &setter), 1, "{setter} takes one");
        assert_eq!(
            arity("RDFramebufferPass", &getter),
            0,
            "{getter} takes none"
        );
    }
}

/// ⚠️ `depth_attachment` defaults to **-1**, not 0.
///
/// -1 is `ATTACHMENT_UNUSED`. 0 is a perfectly valid attachment index, so a 0
/// default would silently mean "attachment 0" — binding a real depth attachment
/// nobody asked for — instead of "none". The constant is registered so the
/// magic number is nameable from script rather than transcribed by hand.
#[test]
fn depth_attachment_defaults_to_attachment_unused_not_zero() {
    register_viewport_classes();

    assert_eq!(
        default_of("RDFramebufferPass", "depth_attachment"),
        Variant::Int(-1),
        "depth_attachment defaults to ATTACHMENT_UNUSED (-1)"
    );
    assert_ne!(
        default_of("RDFramebufferPass", "depth_attachment"),
        Variant::Int(0),
        "0 is a VALID attachment index — it must not be the 'none' default"
    );

    assert_eq!(
        class_get_integer_constant("RDFramebufferPass", "ATTACHMENT_UNUSED"),
        Some(-1),
        "the constant must be readable from script"
    );
    // The default and the constant are the same fact; pin them together so they
    // cannot drift apart.
    assert_eq!(
        default_of("RDFramebufferPass", "depth_attachment"),
        Variant::Int(
            class_get_integer_constant("RDFramebufferPass", "ATTACHMENT_UNUSED")
                .expect("ATTACHMENT_UNUSED is registered")
        ),
        "the depth_attachment default IS ATTACHMENT_UNUSED, not a coincidental -1"
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["FramebufferCacheRD", "RDFramebufferPass", "RenderingDevice"]
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
