//! pat-s3rrv.212 — `Translation`, `TranslationServer` and `TranslationDomain`
//! are ClassDB-registered.
//!
//! The catalog layer in `gdcore::translation` has worked for a long time and
//! the interpreter already routes `TranslationServer.*` to it — but none of the
//! three classes was registered, so `[sub_resource type="Translation"]` in a
//! `.tres`, `ClassDB.instantiate("Translation")` and the inspector all failed
//! on a subsystem that otherwise ran.
//!
//! Members, defaults and accessors are transcribed from
//! `upstream/godot/doc/classes/{Translation,TranslationServer,TranslationDomain}.xml`.

use gdobject::class_db::{
    get_class_info, get_property_getter, instantiate, is_parent_class, register_all_classes,
};
use gdvariant::Variant;

fn default_of(class: &str, name: &str) -> Variant {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`"))
        .default_value
}

/// The acceptance's first clause: both instantiate.
#[test]
fn translation_and_translation_domain_instantiate() {
    register_all_classes();
    assert!(
        instantiate("Translation").is_some(),
        "ClassDB.instantiate(\"Translation\") must succeed — a .tres carrying \
         `[sub_resource type=\"Translation\"]` has nothing to construct otherwise"
    );
    assert!(
        instantiate("TranslationDomain").is_some(),
        "ClassDB.instantiate(\"TranslationDomain\") must succeed"
    );
}

/// ⚠️ **Three different bases, and none of them is guessable from the names.**
/// `Translation` is a Resource (it is authored into a .tres),
/// `TranslationServer` is a bare Object singleton, and `TranslationDomain` is
/// **RefCounted** — a runtime namespace that never appears in a scene file.
#[test]
fn each_class_sits_under_its_documented_base() {
    register_all_classes();
    assert!(
        is_parent_class("Translation", "Resource"),
        "Translation is a Resource — this is what lets it load from a .tres"
    );
    assert!(
        is_parent_class("TranslationServer", "Object"),
        "TranslationServer is a plain Object, not a Resource"
    );
    assert!(
        is_parent_class("TranslationDomain", "RefCounted"),
        "TranslationDomain is RefCounted — a runtime namespace, never authored"
    );
}

/// `Translation.locale` defaults to `"en"`, matching the server's own default.
#[test]
fn translation_declares_its_documented_members() {
    match default_of("Translation", "locale") {
        Variant::String(s) => assert_eq!(s, "en", "doc default is \"en\""),
        other => panic!("locale must be a String, got {other:?}"),
    }
    match default_of("Translation", "plural_rules_override") {
        Variant::String(s) => assert!(s.is_empty(), "doc default is the empty string"),
        other => panic!("plural_rules_override must be a String, got {other:?}"),
    }
}

/// 🔑 **TranslationServer declares exactly ONE member.**
///
/// Every pseudolocalization knob lives on TranslationDomain; the server holds
/// only `pseudolocalization_enabled` and forwards the rest to its default
/// domain. Registering the knobs here would invent a surface Godot does not
/// have — and the inspector would then offer properties that go nowhere.
#[test]
fn the_server_declares_only_the_one_documented_member() {
    register_all_classes();
    let own: Vec<String> = get_class_info("TranslationServer")
        .expect("TranslationServer is registered")
        .properties
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(
        own,
        vec!["pseudolocalization_enabled".to_string()],
        "TranslationServer's doc XML lists exactly one member; the other ten \
         pseudolocalization knobs belong to TranslationDomain"
    );
    assert!(matches!(
        default_of("TranslationServer", "pseudolocalization_enabled"),
        Variant::Bool(false)
    ));
}

/// The domain's ten members carry the whole pseudolocalization surface, at the
/// defaults from `PseudolocalizationConfig`.
///
/// ⚠️ `accents_enabled` is the ONE that defaults true, and `expansion_ratio` is
/// 0.0 — so switching the mode on accents and brackets the text without
/// lengthening it. Anyone "fixing" the ratio to a non-zero default would change
/// every pseudolocalized string in the engine.
#[test]
fn the_domain_carries_the_pseudolocalization_defaults() {
    assert!(matches!(
        default_of("TranslationDomain", "enabled"),
        Variant::Bool(true)
    ));
    assert!(matches!(
        default_of("TranslationDomain", "pseudolocalization_accents_enabled"),
        Variant::Bool(true)
    ));
    for off in [
        "pseudolocalization_enabled",
        "pseudolocalization_double_vowels_enabled",
        "pseudolocalization_fake_bidi_enabled",
        "pseudolocalization_override_enabled",
    ] {
        assert!(
            matches!(default_of("TranslationDomain", off), Variant::Bool(false)),
            "{off} defaults to false"
        );
    }
    assert!(matches!(
        default_of(
            "TranslationDomain",
            "pseudolocalization_skip_placeholders_enabled"
        ),
        Variant::Bool(true)
    ));
    match default_of("TranslationDomain", "pseudolocalization_expansion_ratio") {
        Variant::Float(f) => assert!(f == 0.0, "doc default is 0.0, got {f}"),
        other => panic!("expansion_ratio must be a Float, got {other:?}"),
    }
    match default_of("TranslationDomain", "pseudolocalization_prefix") {
        Variant::String(s) => assert_eq!(s, "["),
        other => panic!("prefix must be a String, got {other:?}"),
    }
    match default_of("TranslationDomain", "pseudolocalization_suffix") {
        Variant::String(s) => assert_eq!(s, "]"),
        other => panic!("suffix must be a String, got {other:?}"),
    }
}

/// ⚠️ The boolean accessors are `is_…`, not `get_…`, and `enabled` reads through
/// the bare `is_enabled` rather than anything derived from a prefix. Asserted
/// because no rule recovers them.
#[test]
fn the_documented_accessors_are_recorded() {
    register_all_classes();
    assert_eq!(
        get_property_getter("TranslationDomain", "enabled"),
        "is_enabled"
    );
    assert_eq!(
        get_property_getter("TranslationDomain", "pseudolocalization_accents_enabled"),
        "is_pseudolocalization_accents_enabled"
    );
    assert_eq!(
        get_property_getter("TranslationDomain", "pseudolocalization_expansion_ratio"),
        "get_pseudolocalization_expansion_ratio",
        "the float reads through `get_`, unlike its boolean siblings"
    );
    assert_eq!(
        get_property_getter("TranslationServer", "pseudolocalization_enabled"),
        "is_pseudolocalization_enabled"
    );
    assert_eq!(get_property_getter("Translation", "locale"), "get_locale");
}
