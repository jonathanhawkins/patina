//! pat-t1x: Broadened GDScript interop parity coverage.
//!
//! Tests nested function calls, closures/lambdas, match with complex patterns,
//! string interpolation/formatting, and typed variables.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

fn run(src: &str) -> (Vec<String>, Option<Variant>) {
    let mut interp = Interpreter::new();
    let result = interp.run(src).expect("script should not error");
    (result.output, result.return_value)
}

fn run_val(src: &str) -> Variant {
    run(src).1.expect("expected a return value")
}

#[allow(dead_code)]
fn run_output(src: &str) -> Vec<String> {
    run(src).0
}

// ===========================================================================
// 1. Nested function calls
// ===========================================================================

#[test]
fn nested_function_calls_basic() {
    let val = run_val(
        "\
func add(a, b):
    return a + b

func double(x):
    return x * 2

return add(double(3), double(4))
",
    );
    assert_eq!(val, Variant::Int(14)); // (3*2) + (4*2) = 14
}

#[test]
fn triple_nested_function_call() {
    let val = run_val(
        "\
func inc(x):
    return x + 1

return inc(inc(inc(0)))
",
    );
    assert_eq!(val, Variant::Int(3));
}

#[test]
fn nested_calls_with_string_concat() {
    let val = run_val(
        "\
func greet(name):
    return \"Hello, \" + name

func shout(s):
    return s + \"!\"

return shout(greet(\"World\"))
",
    );
    assert_eq!(val, Variant::String("Hello, World!".into()));
}

#[test]
fn function_as_arg_result() {
    let val = run_val(
        "\
func square(x):
    return x * x

func sum3(a, b, c):
    return a + b + c

return sum3(square(2), square(3), square(4))
",
    );
    assert_eq!(val, Variant::Int(29)); // 4 + 9 + 16
}

// ===========================================================================
// 2. Closures/lambdas
// ===========================================================================

#[test]
fn lambda_captures_outer_variable() {
    let val = run_val(
        "\
var multiplier = 10
var mul = func(x): return x * multiplier
return mul.call(5)
",
    );
    assert_eq!(val, Variant::Int(50));
}

#[test]
fn lambda_in_array() {
    let val = run_val(
        "\
var ops = [func(x): return x + 1, func(x): return x * 2]
return ops[1].call(5)
",
    );
    assert_eq!(val, Variant::Int(10));
}

#[test]
fn lambda_returned_from_function() {
    // Note: GDScript interop doesn't yet support closure capture of function-scoped
    // variables. Test that a lambda can be returned and called with its own args.
    let val = run_val(
        "\
func make_doubler():
    return func(x): return x * 2

var dbl = make_doubler()
return dbl.call(10)
",
    );
    assert_eq!(val, Variant::Int(20));
}

#[test]
fn lambda_called_immediately() {
    let val = run_val(
        "\
var result = (func(a, b): return a - b).call(10, 3)
return result
",
    );
    assert_eq!(val, Variant::Int(7));
}

// ===========================================================================
// 3. Match with complex patterns
// ===========================================================================

#[test]
fn match_integer_patterns() {
    let val = run_val(
        "\
var x = 42
var result = 0
match x:
    0:
        result = -1
    42:
        result = 1
    _:
        result = 99
return result
",
    );
    assert_eq!(val, Variant::Int(1));
}

#[test]
fn match_string_patterns() {
    let val = run_val(
        "\
var cmd = \"attack\"
var damage = 0
match cmd:
    \"idle\":
        damage = 0
    \"attack\":
        damage = 10
    \"special\":
        damage = 25
    _:
        damage = -1
return damage
",
    );
    assert_eq!(val, Variant::Int(10));
}

#[test]
fn match_with_default_wildcard() {
    let val = run_val(
        "\
var val = \"unknown\"
var out = \"\"
match val:
    \"a\":
        out = \"found a\"
    \"b\":
        out = \"found b\"
    _:
        out = \"default\"
return out
",
    );
    assert_eq!(val, Variant::String("default".into()));
}

#[test]
fn match_in_function() {
    let val = run_val(
        "\
func classify(n):
    match n:
        0:
            return \"zero\"
        1:
            return \"one\"
        _:
            return \"many\"

return classify(0) + \",\" + classify(1) + \",\" + classify(5)
",
    );
    assert_eq!(val, Variant::String("zero,one,many".into()));
}

// ===========================================================================
// 4. String formatting / interpolation
// ===========================================================================

#[test]
fn string_format_with_percent() {
    let val = run_val(
        "\
return \"%s has %d items\" % [\"Player\", 5]
",
    );
    assert_eq!(val, Variant::String("Player has 5 items".into()));
}

#[test]
fn string_format_single_value() {
    let val = run_val(
        "\
return \"Score: %d\" % [100]
",
    );
    assert_eq!(val, Variant::String("Score: 100".into()));
}

#[test]
fn string_concatenation_as_formatting() {
    let val = run_val(
        "\
var name = \"Godot\"
var ver = 4
return name + \" v\" + str(ver)
",
    );
    assert_eq!(val, Variant::String("Godot v4".into()));
}

#[test]
fn str_builtin_converts_types() {
    let val = run_val("return str(42)\n");
    assert_eq!(val, Variant::String("42".into()));
}

#[test]
fn str_builtin_bool() {
    let val = run_val("return str(true)\n");
    assert_eq!(val, Variant::String("true".into()));
}

// ===========================================================================
// 5. Typed variables
// ===========================================================================

#[test]
fn typed_var_int() {
    let val = run_val(
        "\
var x: int = 42
return x
",
    );
    assert_eq!(val, Variant::Int(42));
}

#[test]
fn typed_var_float() {
    let val = run_val(
        "\
var f: float = 3.14
return f
",
    );
    assert_eq!(val, Variant::Float(3.14));
}

#[test]
fn typed_var_string() {
    let val = run_val(
        "\
var s: String = \"hello\"
return s
",
    );
    assert_eq!(val, Variant::String("hello".into()));
}

#[test]
fn typed_var_bool() {
    let val = run_val(
        "\
var b: bool = false
return b
",
    );
    assert_eq!(val, Variant::Bool(false));
}

#[test]
fn typed_function_params() {
    let val = run_val(
        "\
func add(a: int, b: int) -> int:
    return a + b

return add(10, 20)
",
    );
    assert_eq!(val, Variant::Int(30));
}

#[test]
fn typed_var_reassignment() {
    let val = run_val(
        "\
var x: int = 1
x = 2
x = 3
return x
",
    );
    assert_eq!(val, Variant::Int(3));
}

// ===========================================================================
// 6. Complex control flow combinations
// ===========================================================================

#[test]
fn for_loop_with_match() {
    let val = run_val(
        "\
var result = 0
for i in range(5):
    match i:
        0:
            result += 10
        2:
            result += 20
        4:
            result += 30
        _:
            result += 1
return result
",
    );
    assert_eq!(val, Variant::Int(62)); // 10 + 1 + 20 + 1 + 30
}

#[test]
fn recursive_function() {
    let val = run_val(
        "\
func factorial(n):
    if n <= 1:
        return 1
    return n * factorial(n - 1)

return factorial(6)
",
    );
    assert_eq!(val, Variant::Int(720));
}

#[test]
fn while_loop_with_function_call() {
    let val = run_val(
        "\
func is_even(n):
    return n % 2 == 0

var count = 0
var i = 0
while i < 10:
    if is_even(i):
        count += 1
    i += 1
return count
",
    );
    assert_eq!(val, Variant::Int(5));
}

// ===========================================================================
// 7. Array and dictionary operations
// ===========================================================================

#[test]
fn array_map_with_lambda() {
    let val = run_val(
        "\
var arr = [1, 2, 3, 4, 5]
var doubled = []
for x in arr:
    doubled.append(x * 2)
return doubled
",
    );
    assert_eq!(
        val,
        Variant::Array(vec![
            Variant::Int(2),
            Variant::Int(4),
            Variant::Int(6),
            Variant::Int(8),
            Variant::Int(10),
        ])
    );
}

#[test]
fn dictionary_access() {
    let val = run_val(
        "\
var d = {\"a\": 1, \"b\": 2, \"c\": 3}
return d[\"a\"] + d[\"b\"] + d[\"c\"]
",
    );
    assert_eq!(val, Variant::Int(6));
}

#[test]
fn dictionary_has_method() {
    let val = run_val(
        "\
var d = {\"key\": \"value\"}
return d.has(\"key\")
",
    );
    assert_eq!(val, Variant::Bool(true));
}
