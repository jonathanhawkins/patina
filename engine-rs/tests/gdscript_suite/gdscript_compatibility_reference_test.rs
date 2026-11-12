//! Validation tests for the GDScript Compatibility Reference (pat-yvz27).
//!
//! Ensures the reference document exists, has required sections, and
//! its claims about supported features match the actual codebase.

use std::fs;
use std::path::Path;

const DOC_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/GDSCRIPT_COMPATIBILITY.md"
);

fn read_doc() -> String {
    fs::read_to_string(DOC_PATH).expect("docs/GDSCRIPT_COMPATIBILITY.md must exist")
}

// ===========================================================================
// 1. Document structure
// ===========================================================================

#[test]
fn gdscript_compat_doc_exists() {
    assert!(
        Path::new(DOC_PATH).exists(),
        "docs/GDSCRIPT_COMPATIBILITY.md must exist"
    );
}

#[test]
fn doc_has_title() {
    let doc = read_doc();
    assert!(
        doc.starts_with("# GDScript Compatibility Reference"),
        "document must have correct title"
    );
}

#[test]
fn doc_has_all_major_sections() {
    let doc = read_doc();

    let sections = [
        "## Language Features",
        "## Data Types",
        "## Built-in Functions",
        "## Scene Tree Access",
        "## Class & Instance Model",
        "## Integration Architecture",
        "## Deprecated Features",
        "## Performance Notes",
        "## Missing Built-in Functions",
    ];

    for section in &sections {
        assert!(
            doc.contains(section),
            "document must have section: {section}"
        );
    }
}

// ===========================================================================
// 2. Language feature coverage
// ===========================================================================

#[test]
fn doc_covers_supported_language_features() {
    let doc = read_doc();

    let features = [
        "Variable declaration",
        "Functions",
        "Lambda",
        "If/elif/else",
        "While loops",
        "For loops",
        "Match/case",
        "Class declaration",
        "Inheritance",
        "Signals",
        "Enums",
        "@export",
        "@onready",
        "String interpolation",
    ];

    for feat in &features {
        assert!(
            doc.contains(feat),
            "supported features must include: {feat}"
        );
    }
}

#[test]
fn doc_covers_unsupported_language_features() {
    let doc = read_doc();

    let unsupported = [
        "preload",
        "yield",
        "Operator overloading",
        "Abstract methods",
        "remote",
    ];

    for feat in &unsupported {
        assert!(
            doc.contains(feat),
            "unsupported features must mention: {feat}"
        );
    }
}

// ===========================================================================
// 3. Data types
// ===========================================================================

#[test]
fn doc_covers_variant_types() {
    let doc = read_doc();

    let types = [
        "Variant::Nil",
        "Variant::Bool",
        "Variant::Int",
        "Variant::Float",
        "Variant::String",
        "Variant::Vector2",
        "Variant::Vector3",
        "Variant::Color",
        "Variant::Array",
        "Variant::Dictionary",
        "Variant::Transform3D",
        "Variant::Quaternion",
    ];

    for t in &types {
        assert!(doc.contains(t), "variant type mapping must include: {t}");
    }
}

#[test]
fn doc_covers_truthiness_rules() {
    let doc = read_doc();
    assert!(doc.contains("Truthiness"), "must document truthiness rules");
    assert!(doc.contains("false"), "must list false values");
    assert!(
        doc.contains("empty string"),
        "must mention empty string as falsy"
    );
}

// ===========================================================================
// 4. Built-in functions
// ===========================================================================

#[test]
fn doc_covers_math_builtins() {
    let doc = read_doc();

    let builtins = [
        "abs(", "sign(", "floor(", "ceil(", "round(", "sqrt(", "pow(", "sin(", "cos(", "min(",
        "max(", "clamp(", "lerp(",
    ];

    for func in &builtins {
        assert!(doc.contains(func), "math builtins must include: {func}");
    }
}

#[test]
fn doc_covers_random_functions() {
    let doc = read_doc();

    let funcs = ["randi()", "randf()", "randi_range(", "randf_range("];
    for func in &funcs {
        assert!(doc.contains(func), "random functions must include: {func}");
    }
}

#[test]
fn doc_covers_array_methods() {
    let doc = read_doc();

    let methods = [
        ".append(",
        ".push_back(",
        ".size()",
        ".sort()",
        ".find(",
        ".has(",
        ".slice(",
    ];

    for m in &methods {
        assert!(doc.contains(m), "array methods must include: {m}");
    }
}

#[test]
fn doc_covers_dictionary_methods() {
    let doc = read_doc();

    let methods = [
        ".keys()",
        ".values()",
        ".has(",
        ".get(",
        ".erase(",
        ".merge(",
    ];
    for m in &methods {
        assert!(doc.contains(m), "dictionary methods must include: {m}");
    }
}

#[test]
fn doc_covers_string_methods() {
    let doc = read_doc();

    let methods = [
        ".to_lower()",
        ".to_upper()",
        ".split(",
        ".begins_with(",
        ".ends_with(",
        ".find(",
    ];

    for m in &methods {
        assert!(doc.contains(m), "string methods must include: {m}");
    }
}

// ===========================================================================
// 5. Scene tree access
// ===========================================================================

#[test]
fn doc_covers_scene_access_functions() {
    let doc = read_doc();

    let functions = [
        "get_node(",
        "get_parent()",
        "get_children()",
        "emit_signal(",
    ];

    for func in &functions {
        assert!(
            doc.contains(func),
            "scene access functions must include: {func}"
        );
    }
}

#[test]
fn doc_covers_input_functions() {
    let doc = read_doc();

    let functions = [
        "is_action_pressed",
        "is_action_just_pressed",
        "is_key_pressed",
        "get_global_mouse_position",
        "get_vector",
    ];

    for func in &functions {
        assert!(doc.contains(func), "input functions must include: {func}");
    }
}

// ===========================================================================
// 6. Class model
// ===========================================================================

#[test]
fn doc_covers_lifecycle_methods() {
    let doc = read_doc();

    let methods = [
        "_ready()",
        "_process(delta)",
        "_physics_process(delta)",
        "_enter_tree()",
        "_exit_tree()",
        "_input(",
    ];

    for m in &methods {
        assert!(doc.contains(m), "lifecycle methods must include: {m}");
    }
}

#[test]
fn doc_covers_class_features() {
    let doc = read_doc();
    assert!(doc.contains("class_name"), "must document class_name");
    assert!(doc.contains("extends"), "must document extends");
    assert!(doc.contains("Inner class"), "must document inner classes");
    assert!(doc.contains("enum"), "must document enums");
}

// ===========================================================================
// 7. Integration architecture
// ===========================================================================

#[test]
fn doc_covers_architecture_components() {
    let doc = read_doc();

    let components = [
        "Tokenizer",
        "Parser",
        "Interpreter",
        "SceneAccess",
        "ScriptBridge",
    ];

    for comp in &components {
        assert!(
            doc.contains(comp),
            "architecture section must reference: {comp}"
        );
    }
}

// ===========================================================================
// 8. Crate references exist on disk
// ===========================================================================

#[test]
fn referenced_crates_exist() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let crates_dir = Path::new(manifest_dir).join("crates");

    let crates = ["gdscript-interop", "gdvariant", "gdobject", "gdscene"];

    for c in &crates {
        let p = crates_dir.join(c);
        assert!(p.exists(), "referenced crate '{c}' must exist at {p:?}");
    }
}

// ===========================================================================
// 9. Missing builtins documented
// ===========================================================================

#[test]
fn doc_lists_missing_builtins() {
    let doc = read_doc();

    let missing = ["preload()", "load()", "weakref()", "instance_from_id()"];

    for func in &missing {
        assert!(
            doc.contains(func),
            "missing builtins section must list: {func}"
        );
    }
}

// ===========================================================================
// 10. Cross-references
// ===========================================================================

#[test]
fn doc_has_see_also_links() {
    let doc = read_doc();
    assert!(doc.contains("## See Also"), "must have See Also section");
    assert!(
        doc.contains("migration-guide.md"),
        "must link to migration guide"
    );
}

// ===========================================================================
// 11. VisualScript deprecation
// ===========================================================================

#[test]
fn doc_covers_visual_script_deprecation() {
    let doc = read_doc();
    assert!(
        doc.contains("VisualScript"),
        "must document VisualScript deprecation"
    );
    assert!(
        doc.contains("VisualScriptStub"),
        "must mention the stub implementation"
    );
}
