//! pat-s3rrv.67: lambda parameters honour their default values.
//!
//! `func(a = 5): return a` is ordinary GDScript — `@GDScript.xml` documents
//! lambdas as taking the same parameter list a named function does, defaults
//! included. Patina parsed the default into `FuncParam::default` and then threw
//! it away at lambda construction: `params.iter().map(|p| p.name.clone())` kept
//! the name and nothing else, so an omitted argument bound Nil.
//!
//! ⚠️ THE OLD BEHAVIOUR WAS SILENT. Calling `f.call()` did not raise "missing
//! argument"; it bound `a` to Nil and carried on, so the failure surfaced later
//! as an arithmetic or type error somewhere else entirely — or as a quietly
//! wrong number.
//!
//! 🔑 Defaults are stored as EXPRESSIONS and evaluated at CALL time, inside the
//! scope that already holds the captures. That ordering is what the last test
//! here pins: a default is allowed to read a captured local, which is only
//! possible if evaluation happens per-call rather than at `func(...)` creation.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `body` as a method and returns its value.
fn run(body: &str) -> Variant {
    let src = format!("extends Node\nfunc run():\n{body}");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp.instantiate_class(&class_def).expect("instantiates");
    interp
        .call_instance_method(&mut inst, "run", &[])
        .expect("run() executes")
}

#[test]
fn an_omitted_lambda_argument_takes_its_default() {
    let got = run("\x20   var f = func(a = 5): return a\n\
                   \x20   return f.call()\n");
    assert_eq!(
        got,
        Variant::Int(5),
        "an omitted argument must take the parameter's default; Nil means the \
         default expression was dropped at lambda construction"
    );
}

/// CONTROL: the default must not override an argument that WAS supplied.
///
/// Without this, an implementation that always evaluates the default would
/// pass the test above and be wrong in the more common case.
#[test]
fn a_supplied_argument_beats_the_default() {
    let got = run("\x20   var f = func(a = 5): return a\n\
                   \x20   return f.call(9)\n");
    assert_eq!(
        got,
        Variant::Int(9),
        "a supplied argument must win over the default"
    );
}

/// Mixed arity: one required parameter, one defaulted.
#[test]
fn only_the_missing_parameters_take_defaults() {
    let got = run("\x20   var f = func(a, b = 100): return a + b\n\
                   \x20   var both = f.call(1, 2)\n\
                   \x20   var one = f.call(1)\n\
                   \x20   return [both, one]\n");
    let items = match got {
        Variant::Array(a) => a,
        other => panic!("expected an Array, got {other:?}"),
    };
    assert_eq!(
        items,
        vec![Variant::Int(3), Variant::Int(101)].into(),
        "supplying both args gives 1+2=3; omitting the second must fall back to \
         b=100 for 1+100=101"
    );
}

/// 🔑 Defaults are evaluated at CALL time, in the scope holding the captures —
/// so a default may read an enclosing local.
///
/// This is the assertion that distinguishes storing default EXPRESSIONS from
/// storing pre-evaluated values. Both designs pass every test above; only this
/// one separates them.
#[test]
fn a_default_can_read_a_captured_local() {
    let got = run("\x20   var base = 40\n\
                   \x20   var f = func(n = base + 2): return n\n\
                   \x20   return f.call()\n");
    assert_eq!(
        got,
        Variant::Int(42),
        "the default expression `base + 2` must resolve against the captured \
         `base`; Nil means defaults are not evaluated in the lambda's scope"
    );
}
