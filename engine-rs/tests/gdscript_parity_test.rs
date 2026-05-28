//! pat-83zgq: GDScript @export var serialization parity.
//!
//! A script's `@export` vars must round-trip through TSCN save/load so the
//! scene oracle reflects the same property values that the interpreter
//! computes for freshly instantiated class defaults.

use gdeditor::InspectorPanel;
use gdscene::node::Node;
use gdscene::packed_scene::PackedScene;
use gdscene::scene_tree::SceneTree;
use gdscene::TscnSaver;
use gdscript_interop::interpreter::Interpreter;
use gdscript_interop::type_checker::{type_check, TypeErrorKind};
use gdvariant::Variant;

/// Parses `src`, instantiates the class, and returns the (name, Variant)
/// pairs for every `@export` var, resolved from the instance's default
/// evaluation. This is the hook the scene oracle uses to materialize
/// exported properties into the scene tree.
fn export_defaults(src: &str) -> Vec<(String, Variant)> {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let instance = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");
    class_def
        .exports
        .iter()
        .map(|e| {
            let v = instance
                .properties
                .get(&e.name)
                .cloned()
                .unwrap_or(Variant::Nil);
            (e.name.clone(), v)
        })
        .collect()
}

#[test]
fn export_var_round_trip() {
    // A script with several typed @export defaults — scalars, strings,
    // and a Vector2 — covering the Variant shapes the saver must emit.
    let src = "\
extends Node2D

@export var speed: float = 100.0
@export var health: int = 75
@export var player_name: String = \"Hero\"
@export var alive: bool = true
";

    let exports = export_defaults(src);

    // Sanity: the interpreter must expose these four exports.
    let names: Vec<&str> = exports.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["speed", "health", "player_name", "alive"]);

    // Materialize exports as Node properties (the oracle-side step).
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Player", "Node2D");
    for (name, value) in &exports {
        node.set_property(name, value.clone());
    }
    let node_id = tree.add_child(root, node).unwrap();

    // Serialize — the oracle output.
    let saved = TscnSaver::save_tree(&tree, node_id);

    // The saved TSCN must contain every @export key with its default.
    assert!(
        saved.contains("speed = 100.0"),
        "saved output missing speed:\n{saved}"
    );
    assert!(
        saved.contains("health = 75"),
        "saved output missing health:\n{saved}"
    );
    assert!(
        saved.contains("player_name = \"Hero\""),
        "saved output missing player_name:\n{saved}"
    );
    assert!(
        saved.contains("alive = true"),
        "saved output missing alive:\n{saved}"
    );

    // Re-parse — the round-trip back into the oracle.
    let scene = PackedScene::from_tscn(&saved).expect("re-parse saved tscn");
    let nodes = scene.instance().expect("instance re-parsed scene");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name(), "Player");
    assert_eq!(nodes[0].class_name(), "Node2D");

    assert_eq!(nodes[0].get_property("speed"), Variant::Float(100.0));
    assert_eq!(nodes[0].get_property("health"), Variant::Int(75));
    assert_eq!(
        nodes[0].get_property("player_name"),
        Variant::String("Hero".to_string())
    );
    assert_eq!(nodes[0].get_property("alive"), Variant::Bool(true));
}

/// pat-yamee: static type checking for GDScript.
///
/// Exercises the new `type_check` API across three failure modes —
/// `class_name` duplicates, typed-argument mismatches, and return-type
/// mismatches — and confirms that a well-typed script produces no errors.
#[test]
fn type_checker_errors() {
    // Clean, well-typed script: no errors expected.
    //
    // NOTE: avoid Rust's `"\<newline>` line-continuation escape here — it
    // also strips leading whitespace on the next physical line, which
    // silently removes the 4-space indents that GDScript's parser needs.
    let clean = concat!(
        "class_name Hero\n",
        "\n",
        "func add(a: int, b: int) -> int:\n",
        "    return a + b\n",
        "\n",
        "var total: int = 5\n",
    );
    let errors = type_check(clean).expect("clean script should parse");
    assert!(
        errors.is_empty(),
        "expected no type errors on clean script, got {errors:?}"
    );

    // Typed-argument mismatch: greet(42) against `name: String`.
    let bad_arg = concat!(
        "func greet(name: String) -> void:\n",
        "    pass\n",
        "\n",
        "func run() -> void:\n",
        "    greet(42)\n",
    );
    let errors = type_check(bad_arg).expect("parse bad_arg");
    assert!(
        errors.iter().any(|e| matches!(
            &e.kind,
            TypeErrorKind::ArgTypeMismatch { func, expected, found, .. }
                if func == "greet" && expected == "String" && found == "int"
        )),
        "expected ArgTypeMismatch for greet(42), got {errors:?}"
    );

    // Return-type mismatch: func `-> int` returns a string literal.
    let bad_return = concat!(
        "func number() -> int:\n",
        "    return \"forty-two\"\n",
    );
    let errors = type_check(bad_return).expect("parse bad_return");
    assert!(
        errors.iter().any(|e| matches!(
            &e.kind,
            TypeErrorKind::ReturnTypeMismatch { func, expected, found }
                if func == "number" && expected == "int" && found == "String"
        )),
        "expected ReturnTypeMismatch for number(), got {errors:?}"
    );

    // Duplicate class_name declaration.
    let dup_class = concat!(
        "class_name Alpha\n",
        "class_name Beta\n",
    );
    let errors = type_check(dup_class).expect("parse dup_class");
    assert!(
        errors
            .iter()
            .any(|e| matches!(&e.kind, TypeErrorKind::DuplicateClassName)),
        "expected DuplicateClassName, got {errors:?}"
    );

    // int → float widening must still succeed.
    let widening = concat!(
        "func speed(v: float) -> float:\n",
        "    return v\n",
        "\n",
        "func run() -> void:\n",
        "    speed(5)\n",
    );
    let errors = type_check(widening).expect("parse widening");
    assert!(
        errors.is_empty(),
        "expected int→float widening to be accepted, got {errors:?}"
    );
}

/// pat-f5xm4: @export vars flow from compile → InspectorPanel → TSCN.
///
/// Exercises the full pipeline:
///   1. Interpreter captures `@export` annotations during compile and
///      materializes them as instance properties on class instantiation.
///   2. `InspectorPanel` surfaces those properties for inspector editing;
///      edits go through the panel's `set_property` API.
///   3. An inspector-edited value round-trips through `TscnSaver` +
///      `PackedScene::from_tscn` back into a fresh node.
#[test]
fn export_vars_inspector_and_tscn_round_trip() {
    let src = "\
extends Node2D

@export var speed: float = 100.0
@export var health: int = 75
@export var player_name: String = \"Hero\"
@export var alive: bool = true
";

    // -- 1. Compile: @export annotations captured by the interpreter.
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let export_names: Vec<&str> = class_def
        .exports
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(
        export_names,
        vec!["speed", "health", "player_name", "alive"],
        "compile step must capture all @export vars in source order"
    );

    // Instantiate — default expressions become live property values.
    let instance = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");
    assert_eq!(instance.properties.get("speed"), Some(&Variant::Float(100.0)));
    assert_eq!(instance.properties.get("health"), Some(&Variant::Int(75)));
    assert_eq!(
        instance.properties.get("player_name"),
        Some(&Variant::String("Hero".to_string()))
    );
    assert_eq!(instance.properties.get("alive"), Some(&Variant::Bool(true)));

    // Populate a Node with the instance's exported defaults so the
    // scene tree + inspector see the same values the script produced.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Player", "Node2D");
    for export in &class_def.exports {
        let value = instance
            .properties
            .get(&export.name)
            .cloned()
            .unwrap_or(Variant::Nil);
        node.set_property(&export.name, value);
    }
    let node_id = tree.add_child(root, node).expect("add node to tree");

    // -- 2. InspectorPanel surfaces the exported properties and accepts edits.
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);
    let listed: Vec<String> = panel
        .list_properties(&tree)
        .into_iter()
        .map(|e| e.name)
        .collect();
    for name in ["speed", "health", "player_name", "alive"] {
        assert!(
            listed.iter().any(|n| n == name),
            "inspector must list @export var `{name}`: got {listed:?}"
        );
    }
    assert_eq!(panel.get_property(&tree, "health"), Variant::Int(75));

    // Inspector edit: user bumps `health` from 75 to 90.
    let old = panel.set_property(&mut tree, "health", Variant::Int(90));
    assert_eq!(old, Variant::Int(75), "set_property must return old value");
    assert_eq!(
        panel.get_property(&tree, "health"),
        Variant::Int(90),
        "edit must persist to the underlying node"
    );

    // -- 3. TSCN round-trip preserves the inspector-edited value.
    let saved = TscnSaver::save_tree(&tree, node_id);
    assert!(
        saved.contains("health = 90"),
        "saved tscn must reflect inspector edit:\n{saved}"
    );
    assert!(
        saved.contains("speed = 100.0"),
        "saved tscn must preserve untouched @export defaults:\n{saved}"
    );

    let scene = PackedScene::from_tscn(&saved).expect("re-parse saved tscn");
    let nodes = scene.instance().expect("instance re-parsed scene");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name(), "Player");
    assert_eq!(nodes[0].get_property("health"), Variant::Int(90));
    assert_eq!(nodes[0].get_property("speed"), Variant::Float(100.0));
    assert_eq!(
        nodes[0].get_property("player_name"),
        Variant::String("Hero".to_string())
    );
    assert_eq!(nodes[0].get_property("alive"), Variant::Bool(true));
}
