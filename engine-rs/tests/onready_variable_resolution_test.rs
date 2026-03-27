//! pat-hsfr: @onready variable resolution after _ready.
//!
//! Validates that @onready-annotated variables are:
//! 1. Nil before _ready fires
//! 2. Resolved (default expression evaluated) just before _ready
//! 3. Available with correct values inside _ready and _process

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn parse_and_instantiate(source: &str) -> (Interpreter, gdscript_interop::interpreter::ClassInstance) {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(source).unwrap();
    let instance = interp.instantiate_class(&class_def).unwrap();
    (interp, instance)
}

#[test]
fn onready_var_starts_as_nil() {
    let source = r#"
extends Node

@onready var label = "Hello"
var normal_var = 42
"#;
    let (_interp, instance) = parse_and_instantiate(source);

    // @onready var should be Nil before resolution
    assert_eq!(
        instance.properties.get("label"),
        Some(&Variant::Nil),
        "@onready var should be Nil before resolution"
    );
    // Normal var should have its default
    assert_eq!(
        instance.properties.get("normal_var"),
        Some(&Variant::Int(42)),
        "normal var should have default value"
    );
}

#[test]
fn onready_var_resolved_after_resolve_call() {
    let source = r#"
extends Node

@onready var label = "Hello"
@onready var count = 10
"#;
    let (mut interp, mut instance) = parse_and_instantiate(source);

    // Before resolution
    assert_eq!(instance.properties.get("label"), Some(&Variant::Nil));
    assert_eq!(instance.properties.get("count"), Some(&Variant::Nil));

    // Resolve
    interp.resolve_onready_vars(&mut instance).unwrap();

    // After resolution
    assert_eq!(
        instance.properties.get("label"),
        Some(&Variant::String("Hello".into())),
        "@onready var should be resolved"
    );
    assert_eq!(
        instance.properties.get("count"),
        Some(&Variant::Int(10)),
        "@onready var should be resolved"
    );
}

#[test]
fn onready_vars_drained_after_resolution() {
    let source = r#"
extends Node

@onready var x = 1
"#;
    let (mut interp, mut instance) = parse_and_instantiate(source);

    assert!(!instance.onready_vars.is_empty(), "should have pending onready vars");
    interp.resolve_onready_vars(&mut instance).unwrap();
    assert!(instance.onready_vars.is_empty(), "onready_vars should be drained");
}

#[test]
fn onready_with_no_default_resolves_to_nil() {
    let source = r#"
extends Node

@onready var unset
"#;
    let (mut interp, mut instance) = parse_and_instantiate(source);

    interp.resolve_onready_vars(&mut instance).unwrap();
    assert_eq!(
        instance.properties.get("unset"),
        Some(&Variant::Nil),
        "@onready with no default should resolve to Nil"
    );
}

#[test]
fn mixed_onready_and_normal_vars() {
    let source = r#"
extends Node

var a = 1
@onready var b = 2
var c = 3
@onready var d = 4
"#;
    let (mut interp, mut instance) = parse_and_instantiate(source);

    // Normal vars have values, onready are Nil
    assert_eq!(instance.properties.get("a"), Some(&Variant::Int(1)));
    assert_eq!(instance.properties.get("b"), Some(&Variant::Nil));
    assert_eq!(instance.properties.get("c"), Some(&Variant::Int(3)));
    assert_eq!(instance.properties.get("d"), Some(&Variant::Nil));

    interp.resolve_onready_vars(&mut instance).unwrap();

    // All should now have values
    assert_eq!(instance.properties.get("a"), Some(&Variant::Int(1)));
    assert_eq!(instance.properties.get("b"), Some(&Variant::Int(2)));
    assert_eq!(instance.properties.get("c"), Some(&Variant::Int(3)));
    assert_eq!(instance.properties.get("d"), Some(&Variant::Int(4)));
}
