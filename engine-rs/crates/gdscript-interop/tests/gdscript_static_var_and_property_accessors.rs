//! pat-u3l8c.18 — `static var` declarations and inline property accessor
//! bodies (`set(value):` / `get:`).
//!
//! Before this bead a `static var` was a hard parse error and the only
//! property accessor form accepted was the single-line `set = _m, get = _m`
//! (a method *name*). Godot 4 also supports a `static var` and the inline
//! block form:
//!
//! ```gdscript
//! var value = 0:
//!     set(v):
//!         _backing = v * 2
//!     get:
//!         return _backing + 1
//! ```
//!
//! These tests drive the public interpreter API end to end: the scripts must
//! parse *and* the declarations must be honored at runtime.

use gdscript_interop::interpreter::{GDScriptInstance, Interpreter};
use gdscript_interop::ScriptInstance;
use gdvariant::Variant;

/// `static var` must parse (it used to be a parse error) and hold a value that
/// can be read and written through the top-level script instance.
#[test]
fn static_var_parses_and_is_honored() {
    let mut inst = GDScriptInstance::from_source("Counter", "static var count = 5\n")
        .expect("`static var` should parse and initialize");

    assert_eq!(
        inst.get_property("count"),
        Some(Variant::Int(5)),
        "static var should initialize to its declared value"
    );
    assert!(
        inst.set_property("count", Variant::Int(10)),
        "static var should be writable"
    );
    assert_eq!(
        inst.get_property("count"),
        Some(Variant::Int(10)),
        "static var should retain the written value"
    );
}

/// A `static var` declared in a class body is initialized on the instance and
/// is readable/writable from the class's own methods.
#[test]
fn static_var_usable_from_class_method() {
    let src = "\
static var tally = 3\n\
func bump():\n\
\ttally = tally + 2\n\
func read():\n\
\treturn tally\n";

    let mut interp = Interpreter::new();
    let class_def = interp
        .run_class(src)
        .expect("a class with a `static var` should parse");
    let mut instance = interp
        .instantiate_class(&class_def)
        .expect("instantiate the class");

    assert_eq!(
        interp
            .call_instance_method(&mut instance, "read", &[])
            .expect("read()"),
        Variant::Int(3),
        "static var should initialize to 3"
    );
    interp
        .call_instance_method(&mut instance, "bump", &[])
        .expect("bump()");
    assert_eq!(
        interp
            .call_instance_method(&mut instance, "read", &[])
            .expect("read() after bump"),
        Variant::Int(5),
        "static var should honor writes from a method (3 + 2)"
    );
}

/// Inline `set(param): <block>` and `get: <block>` bodies must execute through
/// the normal property-access dispatch: assigning the property runs the setter
/// block, reading it runs the getter block.
#[test]
fn inline_property_setter_and_getter_run() {
    // `value`'s setter doubles the assigned number into a backing field; its
    // getter returns the backing field + 1.
    let src = "\
var _backing = 0\n\
var value = 0:\n\
\tset(v):\n\
\t\t_backing = v * 2\n\
\tget:\n\
\t\treturn _backing + 1\n\
func poke(n):\n\
\tvalue = n\n\
func read_value():\n\
\treturn self.value\n\
func read_backing():\n\
\treturn _backing\n";

    let mut interp = Interpreter::new();
    let class_def = interp
        .run_class(src)
        .expect("a script with inline property accessors should parse");
    let mut instance = interp
        .instantiate_class(&class_def)
        .expect("instantiate the class");

    // Assigning `value = 7` runs the inline setter: _backing = 7 * 2 = 14.
    interp
        .call_instance_method(&mut instance, "poke", &[Variant::Int(7)])
        .expect("poke()");
    assert_eq!(
        interp
            .call_instance_method(&mut instance, "read_backing", &[])
            .expect("read_backing()"),
        Variant::Int(14),
        "the inline setter block should have doubled the value into the backing field"
    );

    // Reading `value` runs the inline getter: _backing + 1 = 15.
    assert_eq!(
        interp
            .call_instance_method(&mut instance, "read_value", &[])
            .expect("read_value()"),
        Variant::Int(15),
        "the inline getter block should have returned backing + 1"
    );
}
