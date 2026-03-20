//! Tests for v1-critical GDScript features:
//! - pat-500: await and coroutines
//! - pat-a6f: Callable type and lambda expressions
//! - pat-7e6: setters and getters
//! - pat-eb3: preload() returning real Resource

use gdscript_interop::interpreter::Interpreter;
use gdvariant::{CallableRef, ResourceRef, Variant};

// ---- Helpers ---------------------------------------------------------------

fn run(src: &str) -> (Vec<String>, Option<Variant>) {
    let mut interp = Interpreter::new();
    let result = interp.run(src).expect("script should not error");
    (result.output, result.return_value)
}

fn run_val(src: &str) -> Variant {
    run(src).1.expect("expected a return value")
}

fn run_output(src: &str) -> Vec<String> {
    run(src).0
}

fn run_class_val(src: &str, method: &str, args: &[Variant]) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");
    interp
        .call_instance_method(&mut inst, method, args)
        .expect("method call failed")
}

// ===========================================================================
// pat-500: await and coroutines
// ===========================================================================

#[test]
fn await_keyword_tokenizes() {
    let tokens = gdscript_interop::tokenize("await foo()\n").unwrap();
    assert!(tokens
        .iter()
        .any(|t| matches!(t.token, gdscript_interop::Token::Await)));
}

#[test]
fn await_parses_as_stmt() {
    let tokens = gdscript_interop::tokenize("await get_tree().process_frame\n").unwrap();
    let mut parser = gdscript_interop::Parser::new(tokens, "await get_tree().process_frame\n");
    let stmts = parser.parse_script().unwrap();
    assert_eq!(stmts.len(), 1);
    assert!(matches!(stmts[0], gdscript_interop::Stmt::Await(_)));
}

#[test]
fn await_emits_warning() {
    let output = run_output("await null\n");
    assert_eq!(output.len(), 1);
    assert!(output[0].contains("coroutines not fully supported"));
}

#[test]
fn await_evaluates_expression() {
    // await evaluates its expression, just emits a warning
    let output = run_output("await print(\"hello\")\n");
    assert_eq!(output.len(), 2);
    assert_eq!(output[0], "hello"); // print was called
    assert!(output[1].contains("await")); // warning
}

#[test]
fn await_with_member_access() {
    // Should parse and execute without panic
    let output = run_output("var x = null\nawait x\n");
    assert!(output[0].contains("coroutines not fully supported"));
}

#[test]
fn await_in_function_body() {
    let src = "\
func do_stuff():
    await null
    return 42
return do_stuff()
";
    let (output, val) = run(src);
    assert!(output[0].contains("await"));
    assert_eq!(val.unwrap(), Variant::Int(42));
}

// ===========================================================================
// pat-a6f: Callable type and lambda expressions
// ===========================================================================

#[test]
fn callable_constructor_method_ref() {
    let val = run_val("return Callable(null, \"my_method\")\n");
    match val {
        Variant::Callable(c) => match c.as_ref() {
            CallableRef::Method { target_id, method } => {
                assert_eq!(*target_id, 0);
                assert_eq!(method, "my_method");
            }
            _ => panic!("expected Method callable"),
        },
        _ => panic!("expected Callable, got {val:?}"),
    }
}

#[test]
fn callable_empty() {
    let val = run_val("return Callable()\n");
    assert!(matches!(val, Variant::Callable(_)));
}

#[test]
fn lambda_basic() {
    let val = run_val(
        "\
var add = func(a, b): return a + b
return add.call(3, 4)
",
    );
    assert_eq!(val, Variant::Int(7));
}

#[test]
fn lambda_no_args() {
    let val = run_val(
        "\
var greet = func(): return \"hello\"
return greet.call()
",
    );
    assert_eq!(val, Variant::String("hello".into()));
}

#[test]
fn lambda_assigned_to_var_and_called_directly() {
    // Call a lambda like a regular function via variable name
    let val = run_val(
        "\
var double = func(x): return x * 2
return double(5)
",
    );
    assert_eq!(val, Variant::Int(10));
}

#[test]
fn callable_call_method() {
    let val = run_val(
        "\
func add(a, b):
    return a + b
var cb = Callable(null, \"add\")
return cb.call(10, 20)
",
    );
    assert_eq!(val, Variant::Int(30));
}

#[test]
fn callable_callv() {
    let val = run_val(
        "\
var mul = func(a, b): return a * b
return mul.callv([6, 7])
",
    );
    assert_eq!(val, Variant::Int(42));
}

#[test]
fn callable_is_valid() {
    let val = run_val(
        "\
var cb = Callable(null, \"foo\")
return cb.is_valid()
",
    );
    assert_eq!(val, Variant::Bool(true));
}

#[test]
fn callable_is_valid_empty() {
    let val = run_val(
        "\
var cb = Callable()
return cb.is_valid()
",
    );
    assert_eq!(val, Variant::Bool(false));
}

#[test]
fn callable_get_method() {
    let val = run_val(
        "\
var cb = Callable(null, \"my_func\")
return cb.get_method()
",
    );
    assert_eq!(val, Variant::String("my_func".into()));
}

#[test]
fn lambda_get_method() {
    let val = run_val(
        "\
var cb = func(): return 1
return cb.get_method()
",
    );
    assert_eq!(val, Variant::String("<lambda>".into()));
}

#[test]
fn lambda_is_valid() {
    let val = run_val(
        "\
var cb = func(): return 1
return cb.is_valid()
",
    );
    assert_eq!(val, Variant::Bool(true));
}

// ===========================================================================
// pat-7e6: setters and getters
// ===========================================================================

#[test]
fn setter_getter_parsed() {
    let tokens =
        gdscript_interop::tokenize("var health: int = 100: set = _set_health, get = _get_health\n")
            .unwrap();
    let mut parser = gdscript_interop::Parser::new(
        tokens,
        "var health: int = 100: set = _set_health, get = _get_health\n",
    );
    let stmts = parser.parse_script().unwrap();
    assert_eq!(stmts.len(), 1);
    match &stmts[0] {
        gdscript_interop::Stmt::VarDecl {
            name,
            setter,
            getter,
            ..
        } => {
            assert_eq!(name, "health");
            assert_eq!(setter.as_deref(), Some("_set_health"));
            assert_eq!(getter.as_deref(), Some("_get_health"));
        }
        other => panic!("expected VarDecl, got {other:?}"),
    }
}

#[test]
fn setter_getter_only_set() {
    let tokens = gdscript_interop::tokenize("var x = 0: set = _set_x\n").unwrap();
    let mut parser = gdscript_interop::Parser::new(tokens, "var x = 0: set = _set_x\n");
    let stmts = parser.parse_script().unwrap();
    match &stmts[0] {
        gdscript_interop::Stmt::VarDecl { setter, getter, .. } => {
            assert_eq!(setter.as_deref(), Some("_set_x"));
            assert!(getter.is_none());
        }
        other => panic!("expected VarDecl, got {other:?}"),
    }
}

#[test]
fn setter_getter_only_get() {
    let tokens = gdscript_interop::tokenize("var x = 0: get = _get_x\n").unwrap();
    let mut parser = gdscript_interop::Parser::new(tokens, "var x = 0: get = _get_x\n");
    let stmts = parser.parse_script().unwrap();
    match &stmts[0] {
        gdscript_interop::Stmt::VarDecl { setter, getter, .. } => {
            assert!(setter.is_none());
            assert_eq!(getter.as_deref(), Some("_get_x"));
        }
        other => panic!("expected VarDecl, got {other:?}"),
    }
}

#[test]
fn setter_called_on_write() {
    let src = "\
extends Node
var health: int = 100: set = _set_health
var log_msg: String = \"\"

func _set_health(v):
    health = v
    log_msg = \"set:\" + str(v)

func set_and_read(val):
    self.health = val
    return self.log_msg
";
    let val = run_class_val(src, "set_and_read", &[Variant::Int(50)]);
    assert_eq!(val, Variant::String("set:50".into()));
}

#[test]
fn getter_called_on_read() {
    let src = "\
extends Node
var health: int = 100: get = _get_health

func _get_health():
    return health * 2

func read_health():
    return self.health
";
    let val = run_class_val(src, "read_health", &[]);
    assert_eq!(val, Variant::Int(200));
}

#[test]
fn setter_and_getter_together() {
    let src = "\
extends Node
var score: int = 0: set = _set_score, get = _get_score
var _raw_score: int = 0

func _set_score(v):
    _raw_score = v

func _get_score():
    return _raw_score + 1000

func test():
    self.score = 42
    return self.score
";
    let val = run_class_val(src, "test", &[]);
    // _set_score stores 42 in _raw_score, _get_score returns 42+1000=1042
    assert_eq!(val, Variant::Int(1042));
}

#[test]
fn setter_bare_name_assignment() {
    // When writing `health = x` inside a method, the setter should be called
    let src = "\
extends Node
var health: int = 100: set = _set_health
var clamped: bool = false

func _set_health(v):
    if v < 0:
        health = 0
        clamped = true
    else:
        health = v

func damage():
    health = -10
    return clamped
";
    let val = run_class_val(src, "damage", &[]);
    assert_eq!(val, Variant::Bool(true));
}

#[test]
fn getter_no_infinite_recursion() {
    // Inside the getter, reading the property should NOT re-trigger the getter
    let src = "\
extends Node
var x: int = 42: get = _get_x

func _get_x():
    return x

func read():
    return self.x
";
    let val = run_class_val(src, "read", &[]);
    assert_eq!(val, Variant::Int(42));
}

// ===========================================================================
// pat-eb3: preload() returning real Resource
// ===========================================================================

#[test]
fn preload_returns_resource_variant() {
    let val = run_val("return preload(\"res://icon.png\")\n");
    match val {
        Variant::Resource(r) => {
            assert_eq!(r.path, "res://icon.png");
            assert_eq!(r.class_name, "Texture2D");
        }
        other => panic!("expected Resource, got {other:?}"),
    }
}

#[test]
fn preload_tres_returns_resource() {
    let val = run_val("return preload(\"res://theme.tres\")\n");
    match val {
        Variant::Resource(r) => {
            assert_eq!(r.path, "res://theme.tres");
            assert_eq!(r.class_name, "Resource");
        }
        other => panic!("expected Resource, got {other:?}"),
    }
}

#[test]
fn preload_tscn_returns_packed_scene() {
    let val = run_val("return preload(\"res://level.tscn\")\n");
    match val {
        Variant::Resource(r) => {
            assert_eq!(r.path, "res://level.tscn");
            assert_eq!(r.class_name, "PackedScene");
        }
        other => panic!("expected Resource, got {other:?}"),
    }
}

#[test]
fn load_returns_resource() {
    let val = run_val("return load(\"res://audio.wav\")\n");
    match val {
        Variant::Resource(r) => {
            assert_eq!(r.path, "res://audio.wav");
            assert_eq!(r.class_name, "AudioStream");
        }
        other => panic!("expected Resource, got {other:?}"),
    }
}

#[test]
fn preload_gd_returns_gdscript() {
    let val = run_val("return preload(\"res://player.gd\")\n");
    match val {
        Variant::Resource(r) => {
            assert_eq!(r.path, "res://player.gd");
            assert_eq!(r.class_name, "GDScript");
        }
        other => panic!("expected Resource, got {other:?}"),
    }
}

#[test]
fn resource_get_path() {
    let val = run_val(
        "\
var r = preload(\"res://icon.png\")
return r.get_path()
",
    );
    assert_eq!(val, Variant::String("res://icon.png".into()));
}

#[test]
fn resource_get_class() {
    let val = run_val(
        "\
var r = preload(\"res://icon.png\")
return r.get_class()
",
    );
    assert_eq!(val, Variant::String("Texture2D".into()));
}

#[test]
fn resource_is_truthy() {
    let val = run_val(
        "\
var r = preload(\"res://icon.png\")
if r:
    return true
return false
",
    );
    assert_eq!(val, Variant::Bool(true));
}

// ===========================================================================
// pat-t1x: GDScript interop parity — match, ternary, string formatting
// ===========================================================================

/// match statement with literal patterns.
#[test]
fn match_with_literal_patterns() {
    let val = run_val(
        "\
var x = 2
match x:
    1:
        return \"one\"
    2:
        return \"two\"
    3:
        return \"three\"
return \"unknown\"
",
    );
    assert_eq!(val, Variant::String("two".into()));
}

/// match with default/wildcard pattern.
#[test]
fn match_with_wildcard_pattern() {
    let val = run_val(
        "\
var x = 99
match x:
    1:
        return \"one\"
    _:
        return \"default\"
",
    );
    assert_eq!(val, Variant::String("default".into()));
}

/// match with string patterns.
#[test]
fn match_with_string_patterns() {
    let val = run_val(
        "\
var cmd = \"attack\"
match cmd:
    \"move\":
        return 1
    \"attack\":
        return 2
    \"defend\":
        return 3
return 0
",
    );
    assert_eq!(val, Variant::Int(2));
}

/// Ternary (conditional) expression.
#[test]
fn ternary_expression_true_branch() {
    let val = run_val("return 10 if true else 20\n");
    assert_eq!(val, Variant::Int(10));
}

#[test]
fn ternary_expression_false_branch() {
    let val = run_val("return 10 if false else 20\n");
    assert_eq!(val, Variant::Int(20));
}

#[test]
fn ternary_in_variable_assignment() {
    let val = run_val(
        "\
var x = 5
var result = \"big\" if x > 3 else \"small\"
return result
",
    );
    assert_eq!(val, Variant::String("big".into()));
}

/// String formatting with % operator.
#[test]
fn string_format_percent_single() {
    let val = run_val("return \"Hello %s\" % \"world\"\n");
    assert_eq!(val, Variant::String("Hello world".into()));
}

#[test]
fn string_format_percent_array() {
    let val = run_val("return \"%s has %s items\" % [\"bag\", \"3\"]\n");
    assert_eq!(val, Variant::String("bag has 3 items".into()));
}

#[test]
fn preload_font_returns_resource() {
    let val = run_val("return preload(\"res://font.ttf\")\n");
    match val {
        Variant::Resource(r) => {
            assert_eq!(r.path, "res://font.ttf");
            assert_eq!(r.class_name, "Font");
        }
        other => panic!("expected Resource, got {other:?}"),
    }
}
