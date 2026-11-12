//! pat-s3rrv.60 — the `NodePath()` / `StringName()` CALL-form constructors.
//!
//! Both types had a `Variant` arm, a `gdcore` implementation and a script-facing
//! method surface — and no way for a script to construct one **by name**. The
//! builtin-constructor dispatch carried arms for `Rect2`, `Transform2D`,
//! `Color` and the rest, but not for these two, so `NodePath("a/b")` raised
//! instead of building a path.
//!
//! The literal forms `^"a/b"` and `&"foo"` are the sibling half and are covered
//! by the parser tests; these are the call forms, which is what code that builds
//! a path from a variable must use.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::{CallableRef, Variant};

fn eval(expr: &str) -> Variant {
    gdobject::class_db::register_all_classes();
    let src = format!("extends Node\n\nfunc check():\n    return {expr}\n");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("`{expr}` failed: {e:?}"))
}

#[test]
fn nodepath_constructor_builds_a_path_from_a_string() {
    match eval("NodePath(\"Sprite:modulate:r\")") {
        Variant::NodePath(p) => {
            assert_eq!(p.get_name_count(), 1, "one node name: Sprite");
            assert_eq!(p.get_subname_count(), 2, "two subnames: modulate, r");
            assert_eq!(p.get_concatenated_subnames(), "modulate:r");
        }
        other => panic!("expected a NodePath, got {other:?}"),
    }
}

/// ⚠️ Godot's constructors are also COPY constructors, and this is not
/// decorative: normalising an argument of unknown origin with
/// `NodePath(maybe_a_path)` is the idiomatic use, and rejecting an already-built
/// path would make the constructor useless for exactly that.
#[test]
fn nodepath_constructor_accepts_a_nodepath() {
    match eval("NodePath(NodePath(\"A/B\"))") {
        Variant::NodePath(p) => assert_eq!(p.get_name_count(), 2, "A and B survive the round trip"),
        other => panic!("expected a NodePath, got {other:?}"),
    }
}

#[test]
fn nodepath_constructor_with_no_argument_is_empty() {
    match eval("NodePath()") {
        Variant::NodePath(p) => assert!(p.is_empty(), "NodePath() is the empty path"),
        other => panic!("expected a NodePath, got {other:?}"),
    }
}

#[test]
fn stringname_constructor_builds_from_a_string() {
    match eval("StringName(\"some_name\")") {
        Variant::StringName(s) => assert_eq!(s.as_str(), "some_name"),
        other => panic!("expected a StringName, got {other:?}"),
    }
}

#[test]
fn stringname_constructor_accepts_a_stringname() {
    match eval("StringName(StringName(\"n\"))") {
        Variant::StringName(s) => assert_eq!(s.as_str(), "n"),
        other => panic!("expected a StringName, got {other:?}"),
    }
}

/// The constructors must produce their OWN types, not the String they were
/// built from — otherwise `is`/`as` checks and any overload keyed on the type
/// would silently take the String path.
#[test]
fn the_constructors_produce_their_own_variant_types() {
    assert!(
        matches!(eval("NodePath(\"A\")"), Variant::NodePath(_)),
        "NodePath(..) must not decay to a String"
    );
    assert!(
        matches!(eval("StringName(\"A\")"), Variant::StringName(_)),
        "StringName(..) must not decay to a String"
    );
}

// ---------------------------------------------------------------------------
// Callable.create — the static spelling
// ---------------------------------------------------------------------------

/// `Callable.create(object, "method")` is Godot's STATIC spelling of
/// `Callable(object, "method")` (pat-s3rrv.60).
///
/// The instance form was already dispatched; the static one was not, and it is
/// the spelling used when the target is decided at runtime.
#[test]
fn callable_create_builds_a_method_callable() {
    match eval("Callable.create(null, \"bump\")") {
        Variant::Callable(c) => match *c {
            CallableRef::Method {
                target_id,
                ref method,
            } => {
                assert_eq!(target_id, 0, "`null` means self, which is target 0");
                assert_eq!(method, "bump");
            }
            ref other => panic!("expected a Method callable, got {other:?}"),
        },
        other => panic!("expected a Callable, got {other:?}"),
    }
}

/// ⚠️ The two spellings must agree. They are separate code paths — the
/// constructor lives in `try_builtin`, the static in the type-name call
/// dispatch — so this pins that they cannot drift apart.
#[test]
fn callable_create_matches_the_constructor_spelling() {
    let via_static = eval("Callable.create(null, \"bump\")");
    let via_ctor = eval("Callable(null, \"bump\")");
    let parts = |v: Variant| match v {
        Variant::Callable(c) => match *c {
            CallableRef::Method {
                target_id,
                ref method,
            } => (target_id, method.clone()),
            ref other => panic!("expected a Method callable, got {other:?}"),
        },
        other => panic!("expected a Callable, got {other:?}"),
    };
    assert_eq!(
        parts(via_static),
        parts(via_ctor),
        "Callable.create(..) and Callable(..) must build the same callable"
    );
}

/// A StringName method name is accepted, because `Callable.create(self, &\"bump\")`
/// is the spelling a Godot project actually writes.
#[test]
fn callable_create_accepts_a_stringname_method() {
    match eval("Callable.create(null, &\"bump\")") {
        Variant::Callable(c) => match *c {
            CallableRef::Method { ref method, .. } => assert_eq!(method, "bump"),
            ref other => panic!("expected a Method callable, got {other:?}"),
        },
        other => panic!("expected a Callable, got {other:?}"),
    }
}
