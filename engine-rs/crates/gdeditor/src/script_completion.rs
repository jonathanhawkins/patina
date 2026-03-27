//! Script editor code completion with class and method suggestions.
//!
//! Provides a headless completion engine for the GDScript editor. Given a
//! cursor context (class name, partial identifier, scope), the engine
//! queries the [`ClassDB`](gdobject::class_db) to produce ranked suggestions
//! for classes, methods, properties, signals, and built-in keywords.
//!
//! # Design
//!
//! - [`CompletionContext`] describes what the user is typing and where.
//! - [`CompletionItem`] is a single suggestion with label, kind, and detail.
//! - [`CompletionEngine`] holds configuration and produces completions.
//! - Completions are filtered by prefix match and sorted by relevance.

use gdobject::class_db;

// ---------------------------------------------------------------------------
// CompletionKind
// ---------------------------------------------------------------------------

/// The kind of a completion item, used for icon selection and sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    /// A class name (e.g. `Node2D`, `Sprite2D`).
    Class,
    /// A method on the current or dot-accessed class.
    Method,
    /// A property on the current or dot-accessed class.
    Property,
    /// A signal on the current or dot-accessed class.
    Signal,
    /// A GDScript keyword (`var`, `func`, `if`, `for`, etc.).
    Keyword,
    /// A local variable in scope.
    Variable,
    /// A constant value.
    Constant,
}

// ---------------------------------------------------------------------------
// CompletionItem
// ---------------------------------------------------------------------------

/// A single code-completion suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// The text to insert when accepted.
    pub label: String,
    /// What kind of symbol this represents.
    pub kind: CompletionKind,
    /// Optional short description shown beside the label.
    pub detail: Option<String>,
    /// Optional longer documentation string.
    pub documentation: Option<String>,
    /// Relevance score (higher = more relevant). Used for sorting.
    pub score: u32,
}

impl CompletionItem {
    /// Creates a new completion item.
    pub fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        Self {
            label: label.into(),
            kind,
            detail: None,
            documentation: None,
            score: 0,
        }
    }

    /// Sets the detail text.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets the documentation text.
    pub fn with_docs(mut self, docs: impl Into<String>) -> Self {
        self.documentation = Some(docs.into());
        self
    }

    /// Sets the relevance score.
    pub fn with_score(mut self, score: u32) -> Self {
        self.score = score;
        self
    }
}

// ---------------------------------------------------------------------------
// CompletionContext
// ---------------------------------------------------------------------------

/// Describes the cursor context when requesting completions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionContext {
    /// The class the script is attached to (e.g. `"CharacterBody2D"`).
    pub current_class: String,
    /// The partial text the user has typed (prefix to match against).
    pub prefix: String,
    /// If the user typed `expr.`, this is the resolved class of `expr`.
    pub dot_class: Option<String>,
    /// Local variable names currently in scope.
    pub local_variables: Vec<String>,
}

impl CompletionContext {
    /// Creates a context for completing a bare identifier.
    pub fn bare(current_class: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self {
            current_class: current_class.into(),
            prefix: prefix.into(),
            dot_class: None,
            local_variables: Vec::new(),
        }
    }

    /// Creates a context for completing after a dot.
    pub fn dot_access(
        current_class: impl Into<String>,
        dot_class: impl Into<String>,
        prefix: impl Into<String>,
    ) -> Self {
        Self {
            current_class: current_class.into(),
            prefix: prefix.into(),
            dot_class: Some(dot_class.into()),
            local_variables: Vec::new(),
        }
    }

    /// Adds local variables to the context.
    pub fn with_locals(mut self, locals: Vec<String>) -> Self {
        self.local_variables = locals;
        self
    }
}

// ---------------------------------------------------------------------------
// GDScript keywords
// ---------------------------------------------------------------------------

/// Built-in GDScript keywords for completion.
const GDSCRIPT_KEYWORDS: &[&str] = &[
    "var", "const", "func", "class", "extends", "class_name",
    "if", "elif", "else", "for", "while", "match", "break", "continue",
    "pass", "return", "signal", "enum", "static", "onready",
    "export", "preload", "await", "yield", "self", "super",
    "true", "false", "null", "not", "and", "or", "in", "is", "as",
    "void", "int", "float", "bool", "String",
];

// ---------------------------------------------------------------------------
// CompletionEngine
// ---------------------------------------------------------------------------

/// Configuration and state for the code completion engine.
#[derive(Debug, Clone)]
pub struct CompletionEngine {
    /// Maximum number of suggestions to return.
    pub max_results: usize,
    /// Whether to include keywords in bare completions.
    pub include_keywords: bool,
    /// Whether to include inherited members.
    pub include_inherited: bool,
}

impl Default for CompletionEngine {
    fn default() -> Self {
        Self {
            max_results: 50,
            include_keywords: true,
            include_inherited: true,
        }
    }
}

impl CompletionEngine {
    /// Creates a new engine with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the max results limit.
    pub fn with_max_results(mut self, max: usize) -> Self {
        self.max_results = max;
        self
    }

    /// Produces completion items for the given context.
    pub fn complete(&self, ctx: &CompletionContext) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        let prefix_lower = ctx.prefix.to_lowercase();

        if let Some(ref dot_class) = ctx.dot_class {
            // Dot completion: show members of the dot class.
            self.add_class_members(dot_class, &prefix_lower, &mut items);
        } else {
            // Bare completion: keywords + current class members + class names + locals.
            if self.include_keywords {
                self.add_keywords(&prefix_lower, &mut items);
            }
            self.add_class_members(&ctx.current_class, &prefix_lower, &mut items);
            self.add_class_names(&prefix_lower, &mut items);
            self.add_local_variables(&ctx.local_variables, &prefix_lower, &mut items);
        }

        // Sort by score descending, then alphabetically.
        items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.label.cmp(&b.label)));
        items.truncate(self.max_results);
        items
    }

    /// Adds methods and properties from a class (and optionally its ancestors).
    fn add_class_members(
        &self,
        class_name: &str,
        prefix: &str,
        items: &mut Vec<CompletionItem>,
    ) {
        let classes_to_check = if self.include_inherited {
            Self::inheritance_chain(class_name)
        } else {
            vec![class_name.to_string()]
        };

        for cls in &classes_to_check {
            if let Some(info) = class_db::get_class_info(cls) {
                // Methods.
                for method in &info.methods {
                    if prefix.is_empty() || method.name.to_lowercase().starts_with(prefix) {
                        let detail = format!("{}({} args)", method.name, method.argument_count);
                        let score = if cls == class_name { 100 } else { 50 };
                        items.push(
                            CompletionItem::new(&method.name, CompletionKind::Method)
                                .with_detail(detail)
                                .with_score(score),
                        );
                    }
                }

                // Properties.
                for prop in &info.properties {
                    if prefix.is_empty() || prop.name.to_lowercase().starts_with(prefix) {
                        let score = if cls == class_name { 90 } else { 45 };
                        items.push(
                            CompletionItem::new(&prop.name, CompletionKind::Property)
                                .with_detail(format!("from {}", cls))
                                .with_score(score),
                        );
                    }
                }
            }
        }
    }

    /// Adds matching GDScript keywords.
    fn add_keywords(&self, prefix: &str, items: &mut Vec<CompletionItem>) {
        for &kw in GDSCRIPT_KEYWORDS {
            if prefix.is_empty() || kw.starts_with(prefix) {
                items.push(
                    CompletionItem::new(kw, CompletionKind::Keyword).with_score(30),
                );
            }
        }
    }

    /// Adds matching class names from the global registry.
    fn add_class_names(&self, prefix: &str, items: &mut Vec<CompletionItem>) {
        if prefix.is_empty() {
            return; // Don't dump all classes with no prefix.
        }
        let all_classes = class_db::get_class_list();
        for name in &all_classes {
            if name.to_lowercase().starts_with(prefix) {
                items.push(
                    CompletionItem::new(name, CompletionKind::Class)
                        .with_detail("class")
                        .with_score(40),
                );
            }
        }
    }

    /// Adds local variables that match the prefix.
    fn add_local_variables(
        &self,
        locals: &[String],
        prefix: &str,
        items: &mut Vec<CompletionItem>,
    ) {
        for var in locals {
            if prefix.is_empty() || var.to_lowercase().starts_with(prefix) {
                items.push(
                    CompletionItem::new(var, CompletionKind::Variable).with_score(110),
                );
            }
        }
    }

    /// Walks the ClassDB inheritance chain from `class_name` up to the root.
    fn inheritance_chain(class_name: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut current = class_name.to_string();
        // Safety bound to prevent infinite loops.
        for _ in 0..32 {
            chain.push(current.clone());
            match class_db::get_class_info(&current) {
                Some(info) if !info.parent_class.is_empty() => {
                    current = info.parent_class.clone();
                }
                _ => break,
            }
        }
        chain
    }

    /// Returns the inheritance chain for a class (exposed for testing).
    pub fn get_inheritance_chain(class_name: &str) -> Vec<String> {
        Self::inheritance_chain(class_name)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gdobject::class_db::{register_class, ClassRegistration, get_class_info};

    // Helper: register a small class hierarchy for testing.
    // NOTE: ClassDB is global+static, so tests share state.
    // Use unique class names per test to avoid collisions.
    fn register_test_classes(prefix: &str) {
        let base = format!("{prefix}Base");
        let child = format!("{prefix}Child");

        register_class(
            ClassRegistration::new(&base)
                .property(gdobject::class_db::PropertyInfo::new("visible", gdvariant::Variant::Bool(true)))
                .method(gdobject::class_db::MethodInfo::new("_ready", 0))
                .method(gdobject::class_db::MethodInfo::new("_process", 1))
                .method(gdobject::class_db::MethodInfo::new("queue_free", 0)),
        );

        register_class(
            ClassRegistration::new(&child)
                .parent(&base)
                .property(gdobject::class_db::PropertyInfo::new("speed", gdvariant::Variant::Float(100.0)))
                .method(gdobject::class_db::MethodInfo::new("move_and_slide", 0))
                .method(gdobject::class_db::MethodInfo::new("get_velocity", 0)),
        );
    }

    // -- CompletionItem --

    #[test]
    fn item_new() {
        let item = CompletionItem::new("test", CompletionKind::Method);
        assert_eq!(item.label, "test");
        assert_eq!(item.kind, CompletionKind::Method);
        assert!(item.detail.is_none());
        assert_eq!(item.score, 0);
    }

    #[test]
    fn item_with_detail_and_score() {
        let item = CompletionItem::new("foo", CompletionKind::Property)
            .with_detail("bar")
            .with_score(42);
        assert_eq!(item.detail.as_deref(), Some("bar"));
        assert_eq!(item.score, 42);
    }

    #[test]
    fn item_with_docs() {
        let item = CompletionItem::new("x", CompletionKind::Class)
            .with_docs("A class.");
        assert_eq!(item.documentation.as_deref(), Some("A class."));
    }

    // -- CompletionContext --

    #[test]
    fn context_bare() {
        let ctx = CompletionContext::bare("Node2D", "qu");
        assert_eq!(ctx.current_class, "Node2D");
        assert_eq!(ctx.prefix, "qu");
        assert!(ctx.dot_class.is_none());
    }

    #[test]
    fn context_dot_access() {
        let ctx = CompletionContext::dot_access("Script", "Sprite2D", "se");
        assert_eq!(ctx.dot_class.as_deref(), Some("Sprite2D"));
        assert_eq!(ctx.prefix, "se");
    }

    #[test]
    fn context_with_locals() {
        let ctx = CompletionContext::bare("Node", "")
            .with_locals(vec!["player".into(), "enemy".into()]);
        assert_eq!(ctx.local_variables.len(), 2);
    }

    // -- CompletionEngine basics --

    #[test]
    fn engine_default() {
        let engine = CompletionEngine::new();
        assert_eq!(engine.max_results, 50);
        assert!(engine.include_keywords);
        assert!(engine.include_inherited);
    }

    #[test]
    fn engine_with_max_results() {
        let engine = CompletionEngine::new().with_max_results(10);
        assert_eq!(engine.max_results, 10);
    }

    // -- Keyword completion --

    #[test]
    fn keywords_match_prefix() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("UnknownClass_KW", "va");
        let items = engine.complete(&ctx);
        let kw_items: Vec<_> = items.iter().filter(|i| i.kind == CompletionKind::Keyword).collect();
        assert!(kw_items.iter().any(|i| i.label == "var"), "should suggest 'var'");
        assert!(!kw_items.iter().any(|i| i.label == "func"), "'func' doesn't start with 'va'");
    }

    #[test]
    fn keywords_for_prefix() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("X_KW2", "re");
        let items = engine.complete(&ctx);
        assert!(items.iter().any(|i| i.label == "return" && i.kind == CompletionKind::Keyword));
    }

    #[test]
    fn no_keywords_in_dot_completion() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::dot_access("X_DOT", "X_DOT", "va");
        let items = engine.complete(&ctx);
        let kw_items: Vec<_> = items.iter().filter(|i| i.kind == CompletionKind::Keyword).collect();
        assert!(kw_items.is_empty(), "dot completion should not include keywords");
    }

    // -- Class member completion --

    #[test]
    fn methods_from_registered_class() {
        register_test_classes("MC_");
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("MC_Child", "mov");
        let items = engine.complete(&ctx);
        assert!(
            items.iter().any(|i| i.label == "move_and_slide" && i.kind == CompletionKind::Method),
            "should suggest move_and_slide"
        );
    }

    #[test]
    fn inherited_methods_included() {
        register_test_classes("INH_");
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("INH_Child", "_re");
        let items = engine.complete(&ctx);
        assert!(
            items.iter().any(|i| i.label == "_ready"),
            "should include inherited _ready from INH_Base"
        );
    }

    #[test]
    fn inherited_methods_excluded_when_disabled() {
        register_test_classes("NINH_");
        let mut engine = CompletionEngine::new();
        engine.include_inherited = false;
        let ctx = CompletionContext::bare("NINH_Child", "_re");
        let items = engine.complete(&ctx);
        assert!(
            !items.iter().any(|i| i.label == "_ready"),
            "should NOT include inherited _ready when include_inherited=false"
        );
    }

    #[test]
    fn properties_from_class() {
        register_test_classes("PROP_");
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("PROP_Child", "sp");
        let items = engine.complete(&ctx);
        assert!(
            items.iter().any(|i| i.label == "speed" && i.kind == CompletionKind::Property),
            "should suggest 'speed' property"
        );
    }

    #[test]
    fn inherited_properties() {
        register_test_classes("IP_");
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("IP_Child", "vis");
        let items = engine.complete(&ctx);
        assert!(
            items.iter().any(|i| i.label == "visible" && i.kind == CompletionKind::Property),
            "should suggest inherited 'visible'"
        );
    }

    // -- Dot completion --

    #[test]
    fn dot_completion_shows_target_class_members() {
        register_test_classes("DOT_");
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::dot_access("SomeScript", "DOT_Child", "get");
        let items = engine.complete(&ctx);
        assert!(items.iter().any(|i| i.label == "get_velocity"));
    }

    #[test]
    fn dot_completion_no_current_class_members() {
        register_test_classes("DOT2_");
        let engine = CompletionEngine::new();
        // Current class is DOT2_Child but dotting into DOT2_Base
        let ctx = CompletionContext::dot_access("DOT2_Child", "DOT2_Base", "queue");
        let items = engine.complete(&ctx);
        assert!(items.iter().any(|i| i.label == "queue_free"));
        // Should NOT show move_and_slide (from DOT2_Child, not DOT2_Base)
        assert!(!items.iter().any(|i| i.label == "move_and_slide"));
    }

    // -- Local variables --

    #[test]
    fn local_variables_appear() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("X_LOC", "pl")
            .with_locals(vec!["player".into(), "platform".into(), "enemy".into()]);
        let items = engine.complete(&ctx);
        let var_items: Vec<_> = items.iter().filter(|i| i.kind == CompletionKind::Variable).collect();
        assert_eq!(var_items.len(), 2);
        assert!(var_items.iter().any(|i| i.label == "player"));
        assert!(var_items.iter().any(|i| i.label == "platform"));
    }

    #[test]
    fn local_variables_have_highest_score() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("X_SCORE", "p")
            .with_locals(vec!["player".into()]);
        let items = engine.complete(&ctx);
        // Locals should score 110, above methods (100) and keywords (30).
        let local = items.iter().find(|i| i.label == "player").unwrap();
        assert_eq!(local.score, 110);
    }

    // -- Sorting and limits --

    #[test]
    fn results_sorted_by_score_then_alpha() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("X_SORT", "")
            .with_locals(vec!["alpha".into(), "beta".into()]);
        let items = engine.complete(&ctx);
        // All locals (score 110) should come before keywords (score 30).
        let first_keyword_idx = items.iter().position(|i| i.kind == CompletionKind::Keyword);
        let last_local_idx = items.iter().rposition(|i| i.kind == CompletionKind::Variable);
        if let (Some(kw), Some(loc)) = (first_keyword_idx, last_local_idx) {
            assert!(loc < kw, "locals should appear before keywords");
        }
    }

    #[test]
    fn max_results_respected() {
        let engine = CompletionEngine::new().with_max_results(5);
        let ctx = CompletionContext::bare("X_MAX", ""); // empty prefix = many results
        let items = engine.complete(&ctx);
        assert!(items.len() <= 5);
    }

    // -- Class name completion --

    #[test]
    fn class_names_suggested() {
        register_test_classes("CN_");
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("X_CN", "cn_");
        let items = engine.complete(&ctx);
        let class_items: Vec<_> = items.iter().filter(|i| i.kind == CompletionKind::Class).collect();
        assert!(class_items.len() >= 2, "should suggest CN_Base and CN_Child");
    }

    #[test]
    fn class_names_not_suggested_with_empty_prefix() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("X_NOCLSS", "");
        let items = engine.complete(&ctx);
        // With empty prefix, class names are suppressed to avoid noise.
        let class_items: Vec<_> = items.iter().filter(|i| i.kind == CompletionKind::Class).collect();
        assert!(class_items.is_empty());
    }

    // -- Inheritance chain --

    #[test]
    fn inheritance_chain_walks_up() {
        register_test_classes("CHAIN_");
        let chain = CompletionEngine::get_inheritance_chain("CHAIN_Child");
        assert_eq!(chain[0], "CHAIN_Child");
        assert!(chain.contains(&"CHAIN_Base".to_string()));
    }

    #[test]
    fn inheritance_chain_unknown_class() {
        let chain = CompletionEngine::get_inheritance_chain("NoSuchClass_12345");
        assert_eq!(chain, vec!["NoSuchClass_12345"]);
    }

    // -- Edge cases --

    #[test]
    fn empty_prefix_no_crash() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("X_EMPTY", "");
        let items = engine.complete(&ctx);
        // Should return keywords at minimum.
        assert!(!items.is_empty());
    }

    #[test]
    fn unknown_class_no_crash() {
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("TotallyFakeClass_999", "foo");
        let _items = engine.complete(&ctx);
        // Should not panic.
    }

    #[test]
    fn case_insensitive_prefix_match() {
        register_test_classes("CI_");
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::bare("CI_Child", "Move");
        let items = engine.complete(&ctx);
        assert!(
            items.iter().any(|i| i.label == "move_and_slide"),
            "prefix matching should be case-insensitive"
        );
    }

    #[test]
    fn dot_access_with_empty_prefix_shows_all_members() {
        register_test_classes("DALL_");
        let engine = CompletionEngine::new();
        let ctx = CompletionContext::dot_access("X", "DALL_Base", "");
        let items = engine.complete(&ctx);
        // Should show all methods and properties of DALL_Base.
        assert!(items.iter().any(|i| i.label == "_ready"));
        assert!(items.iter().any(|i| i.label == "queue_free"));
        assert!(items.iter().any(|i| i.label == "visible"));
    }
}
