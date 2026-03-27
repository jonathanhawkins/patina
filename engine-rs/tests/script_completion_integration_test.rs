//! pat-lbyb1: Script editor code completion with class and method suggestions.
//!
//! Integration tests covering:
//! 1. CompletionItem construction and builder pattern
//! 2. CompletionContext — bare, dot-access, with locals
//! 3. CompletionEngine defaults and configuration
//! 4. Keyword completion — prefix matching, exclusion from dot context
//! 5. Class member completion — methods, properties, inherited
//! 6. Dot-access completion — target class members only
//! 7. Local variable completion and scoring
//! 8. Class name completion from ClassDB
//! 9. Sorting and max_results truncation
//! 10. Inheritance chain resolution
//! 11. Edge cases — empty prefix, unknown class, case-insensitive

use gdeditor::script_completion::{
    CompletionContext, CompletionEngine, CompletionItem, CompletionKind,
};
use gdobject::class_db::{self, ClassRegistration, MethodInfo, PropertyInfo};
use gdvariant::Variant;

/// Register a small class hierarchy with a unique prefix to avoid ClassDB collisions.
fn register_hierarchy(prefix: &str) {
    let base = format!("{prefix}Node");
    let child = format!("{prefix}Sprite");

    class_db::register_class(
        ClassRegistration::new(&base)
            .property(PropertyInfo::new("visible", Variant::Bool(true)))
            .property(PropertyInfo::new("name", Variant::String("".into())))
            .method(MethodInfo::new("_ready", 0))
            .method(MethodInfo::new("_process", 1))
            .method(MethodInfo::new("queue_free", 0))
            .method(MethodInfo::new("add_child", 1)),
    );

    class_db::register_class(
        ClassRegistration::new(&child)
            .parent(&base)
            .property(PropertyInfo::new("texture", Variant::Nil))
            .property(PropertyInfo::new("offset", Variant::Float(0.0)))
            .method(MethodInfo::new("set_texture", 1))
            .method(MethodInfo::new("get_rect", 0)),
    );
}

// ===========================================================================
// 1. CompletionItem construction
// ===========================================================================

#[test]
fn completion_item_new() {
    let item = CompletionItem::new("test_method", CompletionKind::Method);
    assert_eq!(item.label, "test_method");
    assert_eq!(item.kind, CompletionKind::Method);
    assert!(item.detail.is_none());
    assert!(item.documentation.is_none());
    assert_eq!(item.score, 0);
}

#[test]
fn completion_item_builder_chain() {
    let item = CompletionItem::new("position", CompletionKind::Property)
        .with_detail("Vector2")
        .with_docs("The node's position in parent coordinates.")
        .with_score(90);
    assert_eq!(item.detail.as_deref(), Some("Vector2"));
    assert_eq!(
        item.documentation.as_deref(),
        Some("The node's position in parent coordinates.")
    );
    assert_eq!(item.score, 90);
}

#[test]
fn completion_item_kind_variants() {
    // Ensure all kind variants are distinct
    let kinds = [
        CompletionKind::Class,
        CompletionKind::Method,
        CompletionKind::Property,
        CompletionKind::Signal,
        CompletionKind::Keyword,
        CompletionKind::Variable,
        CompletionKind::Constant,
    ];
    for (i, a) in kinds.iter().enumerate() {
        for (j, b) in kinds.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

// ===========================================================================
// 2. CompletionContext
// ===========================================================================

#[test]
fn context_bare_construction() {
    let ctx = CompletionContext::bare("CharacterBody2D", "mov");
    assert_eq!(ctx.current_class, "CharacterBody2D");
    assert_eq!(ctx.prefix, "mov");
    assert!(ctx.dot_class.is_none());
    assert!(ctx.local_variables.is_empty());
}

#[test]
fn context_dot_access_construction() {
    let ctx = CompletionContext::dot_access("MyScript", "Sprite2D", "set");
    assert_eq!(ctx.current_class, "MyScript");
    assert_eq!(ctx.dot_class.as_deref(), Some("Sprite2D"));
    assert_eq!(ctx.prefix, "set");
}

#[test]
fn context_with_locals() {
    let ctx = CompletionContext::bare("Node", "pl")
        .with_locals(vec!["player".into(), "platform".into(), "score".into()]);
    assert_eq!(ctx.local_variables.len(), 3);
    assert!(ctx.local_variables.contains(&"player".to_string()));
}

// ===========================================================================
// 3. CompletionEngine defaults and config
// ===========================================================================

#[test]
fn engine_default_config() {
    let engine = CompletionEngine::new();
    assert_eq!(engine.max_results, 50);
    assert!(engine.include_keywords);
    assert!(engine.include_inherited);
}

#[test]
fn engine_custom_max_results() {
    let engine = CompletionEngine::new().with_max_results(10);
    assert_eq!(engine.max_results, 10);
}

#[test]
fn engine_default_trait() {
    let engine = CompletionEngine::default();
    assert_eq!(engine.max_results, 50);
}

// ===========================================================================
// 4. Keyword completion
// ===========================================================================

#[test]
fn keywords_var_prefix() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("Unregistered_KW1", "va");
    let items = engine.complete(&ctx);
    assert!(items.iter().any(|i| i.label == "var" && i.kind == CompletionKind::Keyword));
}

#[test]
fn keywords_func_prefix() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("Unregistered_KW2", "fu");
    let items = engine.complete(&ctx);
    assert!(items.iter().any(|i| i.label == "func" && i.kind == CompletionKind::Keyword));
}

#[test]
fn keywords_return_prefix() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("Unregistered_KW3", "ret");
    let items = engine.complete(&ctx);
    assert!(items.iter().any(|i| i.label == "return" && i.kind == CompletionKind::Keyword));
}

#[test]
fn keywords_excluded_from_dot_completion() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::dot_access("X_KWDOT", "X_KWDOT", "va");
    let items = engine.complete(&ctx);
    assert!(
        !items.iter().any(|i| i.kind == CompletionKind::Keyword),
        "dot completion must not include keywords"
    );
}

#[test]
fn keywords_disabled_via_config() {
    let mut engine = CompletionEngine::new();
    engine.include_keywords = false;
    let ctx = CompletionContext::bare("Unregistered_KWOFF", "va");
    let items = engine.complete(&ctx);
    assert!(
        !items.iter().any(|i| i.kind == CompletionKind::Keyword),
        "keywords should be excluded when include_keywords=false"
    );
}

#[test]
fn keywords_score_is_30() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("Unregistered_KWS", "var");
    let items = engine.complete(&ctx);
    let kw = items.iter().find(|i| i.label == "var").unwrap();
    assert_eq!(kw.score, 30);
}

// ===========================================================================
// 5. Class member completion
// ===========================================================================

#[test]
fn methods_from_own_class() {
    register_hierarchy("IT_OWN_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_OWN_Sprite", "set");
    let items = engine.complete(&ctx);
    assert!(items.iter().any(|i| i.label == "set_texture" && i.kind == CompletionKind::Method));
}

#[test]
fn properties_from_own_class() {
    register_hierarchy("IT_PROP_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_PROP_Sprite", "tex");
    let items = engine.complete(&ctx);
    assert!(items.iter().any(|i| i.label == "texture" && i.kind == CompletionKind::Property));
}

#[test]
fn inherited_methods_included() {
    register_hierarchy("IT_INH_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_INH_Sprite", "queue");
    let items = engine.complete(&ctx);
    assert!(
        items.iter().any(|i| i.label == "queue_free"),
        "should include inherited queue_free from base"
    );
}

#[test]
fn inherited_properties_included() {
    register_hierarchy("IT_INHP_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_INHP_Sprite", "vis");
    let items = engine.complete(&ctx);
    assert!(items.iter().any(|i| i.label == "visible" && i.kind == CompletionKind::Property));
}

#[test]
fn inherited_methods_excluded_when_disabled() {
    register_hierarchy("IT_NOINH_");
    let mut engine = CompletionEngine::new();
    engine.include_inherited = false;
    let ctx = CompletionContext::bare("IT_NOINH_Sprite", "queue");
    let items = engine.complete(&ctx);
    assert!(
        !items.iter().any(|i| i.label == "queue_free"),
        "inherited methods should be excluded when include_inherited=false"
    );
}

#[test]
fn own_methods_score_higher_than_inherited() {
    register_hierarchy("IT_SCORE_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_SCORE_Sprite", "");
    let items = engine.complete(&ctx);
    let own = items.iter().find(|i| i.label == "set_texture");
    let inherited = items.iter().find(|i| i.label == "queue_free");
    if let (Some(o), Some(i)) = (own, inherited) {
        assert!(o.score > i.score, "own methods should score higher than inherited");
    }
}

// ===========================================================================
// 6. Dot-access completion
// ===========================================================================

#[test]
fn dot_access_shows_target_members() {
    register_hierarchy("IT_DOT_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::dot_access("SomeScript", "IT_DOT_Sprite", "get");
    let items = engine.complete(&ctx);
    assert!(items.iter().any(|i| i.label == "get_rect"));
}

#[test]
fn dot_access_excludes_current_class_members() {
    register_hierarchy("IT_DOT2_");
    let engine = CompletionEngine::new();
    // Dotting into base — should NOT show child-only methods
    let ctx = CompletionContext::dot_access("IT_DOT2_Sprite", "IT_DOT2_Node", "set_tex");
    let items = engine.complete(&ctx);
    assert!(
        !items.iter().any(|i| i.label == "set_texture"),
        "dot into base should not show child's set_texture"
    );
}

#[test]
fn dot_access_empty_prefix_shows_all() {
    register_hierarchy("IT_DOTALL_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::dot_access("X", "IT_DOTALL_Node", "");
    let items = engine.complete(&ctx);
    assert!(items.iter().any(|i| i.label == "_ready"));
    assert!(items.iter().any(|i| i.label == "_process"));
    assert!(items.iter().any(|i| i.label == "queue_free"));
    assert!(items.iter().any(|i| i.label == "add_child"));
    assert!(items.iter().any(|i| i.label == "visible"));
}

// ===========================================================================
// 7. Local variable completion
// ===========================================================================

#[test]
fn local_variables_match_prefix() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("Unregistered_LOC", "pl")
        .with_locals(vec!["player".into(), "platform".into(), "enemy".into()]);
    let items = engine.complete(&ctx);
    let vars: Vec<_> = items
        .iter()
        .filter(|i| i.kind == CompletionKind::Variable)
        .collect();
    assert_eq!(vars.len(), 2);
    assert!(vars.iter().any(|i| i.label == "player"));
    assert!(vars.iter().any(|i| i.label == "platform"));
}

#[test]
fn local_variables_score_highest() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("Unregistered_LOCS", "p")
        .with_locals(vec!["player".into()]);
    let items = engine.complete(&ctx);
    let local = items.iter().find(|i| i.label == "player").unwrap();
    assert_eq!(local.score, 110);
    // Should be first in results (highest score)
    assert_eq!(items[0].label, "player");
}

#[test]
fn local_variables_not_in_dot_context() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::dot_access("X_LOCDOT", "X_LOCDOT", "pl")
        .with_locals(vec!["player".into()]);
    let items = engine.complete(&ctx);
    assert!(
        !items.iter().any(|i| i.kind == CompletionKind::Variable),
        "local variables should not appear in dot completion"
    );
}

// ===========================================================================
// 8. Class name completion
// ===========================================================================

#[test]
fn class_names_suggested_with_prefix() {
    register_hierarchy("IT_CN_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("X_CN", "it_cn_");
    let items = engine.complete(&ctx);
    let class_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == CompletionKind::Class)
        .collect();
    assert!(
        class_items.len() >= 2,
        "should suggest IT_CN_Node and IT_CN_Sprite"
    );
}

#[test]
fn class_names_suppressed_with_empty_prefix() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("Unregistered_NOCLS", "");
    let items = engine.complete(&ctx);
    assert!(
        !items.iter().any(|i| i.kind == CompletionKind::Class),
        "empty prefix should not suggest class names"
    );
}

// ===========================================================================
// 9. Sorting and truncation
// ===========================================================================

#[test]
fn results_sorted_score_descending() {
    register_hierarchy("IT_SORT_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_SORT_Sprite", "")
        .with_locals(vec!["alpha".into()]);
    let items = engine.complete(&ctx);
    for window in items.windows(2) {
        assert!(
            window[0].score >= window[1].score
                || (window[0].score == window[1].score && window[0].label <= window[1].label),
            "items should be sorted by score desc then label asc: {:?} vs {:?}",
            window[0],
            window[1]
        );
    }
}

#[test]
fn max_results_truncation() {
    let engine = CompletionEngine::new().with_max_results(3);
    let ctx = CompletionContext::bare("Unregistered_TRUNC", "")
        .with_locals(vec![
            "a".into(),
            "b".into(),
            "c".into(),
            "d".into(),
            "e".into(),
        ]);
    let items = engine.complete(&ctx);
    assert!(items.len() <= 3);
}

// ===========================================================================
// 10. Inheritance chain
// ===========================================================================

#[test]
fn inheritance_chain_walks_upward() {
    register_hierarchy("IT_CHAIN_");
    let chain = CompletionEngine::get_inheritance_chain("IT_CHAIN_Sprite");
    assert_eq!(chain[0], "IT_CHAIN_Sprite");
    assert!(chain.contains(&"IT_CHAIN_Node".to_string()));
}

#[test]
fn inheritance_chain_unknown_class() {
    let chain = CompletionEngine::get_inheritance_chain("NonexistentClass_XYZ");
    assert_eq!(chain, vec!["NonexistentClass_XYZ"]);
}

#[test]
fn inheritance_chain_base_class() {
    register_hierarchy("IT_CHAINB_");
    let chain = CompletionEngine::get_inheritance_chain("IT_CHAINB_Node");
    assert_eq!(chain[0], "IT_CHAINB_Node");
    // Base has no parent, so chain length should be 1
    assert_eq!(chain.len(), 1);
}

// ===========================================================================
// 11. Edge cases
// ===========================================================================

#[test]
fn empty_prefix_returns_keywords_at_minimum() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("Unregistered_EMPTY", "");
    let items = engine.complete(&ctx);
    assert!(!items.is_empty(), "empty prefix should still return keywords");
}

#[test]
fn unknown_class_does_not_panic() {
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("CompletelyFakeClass_99999", "xyz");
    let _items = engine.complete(&ctx);
}

#[test]
fn case_insensitive_prefix() {
    register_hierarchy("IT_CI_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_CI_Sprite", "Set_T");
    let items = engine.complete(&ctx);
    assert!(
        items.iter().any(|i| i.label == "set_texture"),
        "prefix matching should be case-insensitive"
    );
}

#[test]
fn no_duplicate_items() {
    register_hierarchy("IT_DUP_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_DUP_Sprite", "");
    let items = engine.complete(&ctx);
    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    let unique: std::collections::HashSet<_> = labels.iter().collect();
    // Methods and properties may share a name with keywords in rare cases,
    // but same-kind duplicates should not exist
    for kind in [
        CompletionKind::Method,
        CompletionKind::Property,
        CompletionKind::Keyword,
    ] {
        let kind_labels: Vec<_> = items
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| &i.label)
            .collect();
        let kind_unique: std::collections::HashSet<_> = kind_labels.iter().collect();
        assert_eq!(
            kind_labels.len(),
            kind_unique.len(),
            "duplicate items found for kind {:?}",
            kind
        );
    }
}

#[test]
fn method_detail_includes_arg_count() {
    register_hierarchy("IT_DET_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_DET_Node", "add_ch");
    let items = engine.complete(&ctx);
    let item = items.iter().find(|i| i.label == "add_child").unwrap();
    assert!(
        item.detail.as_ref().unwrap().contains("1 args"),
        "method detail should show argument count"
    );
}

#[test]
fn property_detail_includes_class_name() {
    register_hierarchy("IT_PDET_");
    let engine = CompletionEngine::new();
    let ctx = CompletionContext::bare("IT_PDET_Sprite", "vis");
    let items = engine.complete(&ctx);
    let item = items.iter().find(|i| i.label == "visible").unwrap();
    assert!(
        item.detail.as_ref().unwrap().contains("IT_PDET_Node"),
        "inherited property detail should show source class"
    );
}
