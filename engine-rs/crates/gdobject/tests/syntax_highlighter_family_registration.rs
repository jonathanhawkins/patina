//! pat-nzgdu.190 — the `SyntaxHighlighter` class chain.
//!
//! Covers the acceptance's "ClassDB reports the three-class chain" clause. The
//! GDScript colour spans and the script editor rendering them are follow-up —
//! see the bead notes.
//!
//! Schema diffed against Godot 4.6.1.stable's `extension_api.json` and its
//! vendored C++ (`upstream/godot/editor/script/script_editor_plugin.h`,
//! `scene/resources/syntax_highlighter.cpp`).

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_editor_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// The acceptance's clause: the three-class chain, link by link.
#[test]
fn syntax_highlighter_chain_is_registered() {
    register_editor_classes();

    assert!(class_exists("SyntaxHighlighter"));
    assert!(
        is_parent_class("SyntaxHighlighter", "Resource"),
        "a highlighter is a Resource — that is what lets it be saved and shared"
    );
    assert!(is_parent_class(
        "EditorSyntaxHighlighter",
        "SyntaxHighlighter"
    ));
    assert!(is_parent_class(
        "GDScriptSyntaxHighlighter",
        "EditorSyntaxHighlighter"
    ));

    // ...and the leaf transitively reaches the root. An unregistered
    // intermediate severs the chain silently (pat-nzgdu.98).
    assert!(is_parent_class(
        "GDScriptSyntaxHighlighter",
        "SyntaxHighlighter"
    ));
    assert!(is_parent_class("GDScriptSyntaxHighlighter", "Resource"));
    assert!(is_parent_class("GDScriptSyntaxHighlighter", "Object"));
}

/// ⚠️ Every link is concrete — unlike most Godot base chains, none of these is
/// abstract. You can instantiate a bare `SyntaxHighlighter`; it simply colours
/// nothing.
#[test]
fn all_three_are_instantiable() {
    register_editor_classes();
    for class in [
        "SyntaxHighlighter",
        "EditorSyntaxHighlighter",
        "GDScriptSyntaxHighlighter",
    ] {
        assert!(!is_abstract(class), "{class} is instantiable in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }
}

/// ⚠️ `get_line_syntax_highlighting` takes a LINE NUMBER, not text.
///
/// The highlighter reads the line from the `TextEdit` it is bound to — and
/// returns an **empty** dictionary when it has none
/// (`syntax_highlighter.cpp:41-43`). An arity of 2 (line + text) would be a
/// different, invented API.
#[test]
fn line_highlighting_takes_a_line_number_not_text() {
    register_editor_classes();
    assert!(class_has_method(
        "SyntaxHighlighter",
        "get_line_syntax_highlighting"
    ));
    assert_eq!(
        arity("SyntaxHighlighter", "get_line_syntax_highlighting"),
        1,
        "it takes only the line index; the text comes from the bound TextEdit"
    );
    // The binding that makes it work, and the cache Godot keeps per line.
    assert_eq!(arity("SyntaxHighlighter", "get_text_edit"), 0);
    assert_eq!(arity("SyntaxHighlighter", "clear_highlighting_cache"), 0);
    assert_eq!(arity("SyntaxHighlighter", "update_cache"), 0);

    // The virtual a custom highlighter overrides mirrors the public arity.
    assert_eq!(
        arity("SyntaxHighlighter", "_get_line_syntax_highlighting"),
        1
    );
}

/// ⚠️ `EditorSyntaxHighlighter` declares EXACTLY three virtuals.
///
/// The bead's description also lists **`_get_supported_extensions`**. Godot has
/// no such virtual — `script_editor_plugin.h:62-64` declares `_get_name`,
/// `_get_supported_languages` and `_create`, and nothing else. Registering it
/// would invent a hook a plugin could override that Godot would never call.
#[test]
fn editor_highlighter_declares_exactly_godots_three_virtuals() {
    register_editor_classes();

    for m in ["_get_name", "_get_supported_languages", "_create"] {
        assert!(
            class_has_method("EditorSyntaxHighlighter", m),
            "missing {m}"
        );
        assert_eq!(arity("EditorSyntaxHighlighter", m), 0, "{m} takes no args");
    }
    assert!(
        !class_has_method("EditorSyntaxHighlighter", "_get_supported_extensions"),
        "the bead's description invents `_get_supported_extensions`; Godot has no such virtual"
    );

    // The three are its OWN; the highlighting virtual is inherited, not
    // redeclared here.
    let base: Vec<String> = get_method_list("SyntaxHighlighter")
        .into_iter()
        .map(|m| m.name)
        .collect();
    let mut own: Vec<String> = get_method_list("EditorSyntaxHighlighter")
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base.contains(m))
        .collect();
    own.sort();
    assert_eq!(
        own,
        vec![
            "_create".to_string(),
            "_get_name".to_string(),
            "_get_supported_languages".to_string(),
        ],
        "these three are the whole editor surface"
    );
}

/// `GDScriptSyntaxHighlighter` declares NOTHING of its own — it is the GDScript
/// *implementation*, and every knob is inherited. Giving it properties would
/// invent schema.
#[test]
fn gdscript_highlighter_declares_nothing_of_its_own() {
    register_editor_classes();

    let base_methods: Vec<String> = get_method_list("EditorSyntaxHighlighter")
        .into_iter()
        .map(|m| m.name)
        .collect();
    let own_methods: Vec<String> = get_method_list("GDScriptSyntaxHighlighter")
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect();
    assert!(
        own_methods.is_empty(),
        "GDScriptSyntaxHighlighter adds no methods in Godot; found {own_methods:?}"
    );

    let base_props = props("EditorSyntaxHighlighter");
    let own_props: Vec<String> = props("GDScriptSyntaxHighlighter")
        .into_iter()
        .filter(|p| !base_props.contains(p))
        .collect();
    assert!(
        own_props.is_empty(),
        "GDScriptSyntaxHighlighter adds no properties; found {own_props:?}"
    );

    // But it inherits the whole surface — that is the point of the chain.
    assert!(class_has_method(
        "GDScriptSyntaxHighlighter",
        "get_line_syntax_highlighting"
    ));
    assert!(class_has_method("GDScriptSyntaxHighlighter", "_create"));
}

/// ⚠️ `class_count()` and the property list cannot catch a duplicate
/// registration (`classes` is name-keyed, so re-registering overwrites). The
/// ClassId is what actually changes — `register_class` mints a new one and
/// leaves a stale `by_id` alias.
#[test]
fn registration_is_idempotent() {
    const CHAIN: [&str; 3] = [
        "SyntaxHighlighter",
        "EditorSyntaxHighlighter",
        "GDScriptSyntaxHighlighter",
    ];
    // No type annotation: `class_id` is a `ClassId`, not a bare integer.
    let ids = || -> Vec<_> {
        CHAIN
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_editor_classes();
    let first = ids();
    register_editor_classes();
    let second = ids();

    assert_eq!(
        first, second,
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
