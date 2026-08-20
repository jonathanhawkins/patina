//! pat-s3rrv.213 (clause 1) — the `Translation` family resolves in ClassDB with
//! an unbroken inheritance chain.
//!
//! Ground truth: `upstream/godot/doc/classes/{Translation,OptimizedTranslation}.xml`.
//!
//! # Scope
//!
//! `Translation`, `TranslationDomain` and `TranslationServer` were registered by
//! a sibling bead; this adds `OptimizedTranslation`, the class the CSV/PO
//! importer emits when `compress` is on. The remaining clauses of the bead —
//! loading a real `.translation` and reaching `tr()` — are tracked separately;
//! the payload decoder for them lives in `gdcore::optimized_translation`.
//!
//! ⚠️ Test names carry explicit ordering prefixes so the ACCEPTANCE runs first.
//! A runner stopping at the first failure goes alphabetically, and a control
//! sorting ahead of the acceptance would consume that budget and leave the
//! clause itself unrun.

use gdobject::class_db::{class_exists, instantiate, register_all_classes};
use gdobject::is_parent_class;

/// 🔑 **THE ACCEPTANCE (clause 1).** Both classes resolve, and the chain walks
/// `OptimizedTranslation -> Translation -> Resource -> RefCounted -> Object`
/// unbroken.
#[test]
fn a_optimized_translation_registered_with_an_unbroken_chain() {
    register_all_classes();

    assert!(class_exists("Translation"), "the base must be registered");
    assert!(
        class_exists("OptimizedTranslation"),
        "the compressed catalogue class the importer actually emits"
    );

    // ⚠️ Parent is `Translation`, NOT `Resource`. A wrong parent still satisfies
    // `class_exists`, so only the chain walk catches it.
    assert!(
        is_parent_class("OptimizedTranslation", "Translation"),
        "OptimizedTranslation inherits Translation"
    );

    // The full walk. Each step is asserted separately so a break reports WHERE.
    for ancestor in ["Translation", "Resource", "RefCounted", "Object"] {
        assert!(
            is_parent_class("OptimizedTranslation", ancestor),
            "chain broken: OptimizedTranslation does not reach {ancestor}"
        );
    }
    for ancestor in ["Resource", "RefCounted", "Object"] {
        assert!(
            is_parent_class("Translation", ancestor),
            "chain broken: Translation does not reach {ancestor}"
        );
    }
}

/// ⚠️ The control that gives the chain assertion its teeth.
///
/// `is_parent_class` walking to `Object` would also succeed if
/// `OptimizedTranslation` had been registered directly under `Resource` — the
/// chain reaches `Object` either way. This pins the SHAPE: the intermediate
/// `Translation` link must be present, and the direction must not be reversed.
#[test]
fn b_the_chain_shape_not_merely_its_endpoint() {
    register_all_classes();

    assert!(
        is_parent_class("OptimizedTranslation", "Translation"),
        "the intermediate link is the whole point"
    );
    assert!(
        !is_parent_class("Translation", "OptimizedTranslation"),
        "inheritance is not symmetric — the base must NOT report the derived \
         class as its ancestor"
    );
    // A class outside the family must not appear in the chain.
    assert!(
        !is_parent_class("OptimizedTranslation", "Node"),
        "a Resource-derived class must not report Node as an ancestor"
    );
}

/// `OptimizedTranslation.xml` declares `is_instantiable=true`.
///
/// ⚠️ `instantiate` returns `None` for an abstract or unregistered class, so
/// this is what separates "registered as a real class" from "registered as a
/// name". It is also what `load()` needs in order to hand back an instance.
#[test]
fn c_both_classes_are_instantiable() {
    register_all_classes();

    assert!(
        instantiate("Translation").is_some(),
        "Translation must be constructible"
    );
    assert!(
        instantiate("OptimizedTranslation").is_some(),
        "OptimizedTranslation must be constructible — the importer's artifact \
         is loaded as an instance of it"
    );
}
