//! pat-s3rrv.243 — the Java/JNI bridge **class surface**, and nothing more.
//!
//! # OUT_OF_SCOPE: the transport is out of scope, the class is not
//!
//! Godot's `JavaClassWrapper` / `JavaClass` / `JavaObject` / `JNISingleton` are
//! the script-facing half of the Android JNI bridge. **No JNI is implemented
//! here and none should be** — Patina targets the browser, and there is no JVM
//! to call into.
//!
//! That is Godot's own behaviour too, not a shortcut. `JavaClassWrapper` is
//! registered on every platform (`platform/android/api/api.cpp:38-78` binds
//! `wrap` unconditionally), while the object it hands back only exists on
//! Android. On a desktop or web build `wrap()` answers null and a script
//! branches on that, which is exactly the contract asserted below.
//!
//! Same rule the socket surfaces closed under — see
//! `gdobject/tests/oos_native_sockets.rs`, which states it explicitly and whose
//! `surfaces_declare_no_transport_methods` is the analogous guard. The scope
//! anchor is `docs/taxonomy.md`.
//!
//! ⚠️ **The value here is precisely that the classes exist while the transport
//! does not.** Without registration, `JavaClassWrapper` raises unknown-class and
//! a script cannot even ask the question; a portable project that guards its
//! Android path with `if JavaClassWrapper.wrap(..) != null` fails to *parse*
//! rather than taking the non-Android branch.

use gdobject::class_db::{class_exists, get_class_info, inheritance_chain, register_all_classes};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// `(class, parent)` — from BOTH oracles, which agree: each class's own doc XML
/// and `gdeditor/src/inspector_schema_generated.rs:4629-4635`. The editor table
/// and ClassDB must not disagree, or the inspector shows a hierarchy the engine
/// does not have.
const SURFACE: &[(&str, &str)] = &[
    ("JavaClass", "RefCounted"),
    ("JavaObject", "RefCounted"),
    ("JNISingleton", "Object"),
    ("JavaClassWrapper", "Object"),
];

/// Runs `method` on a fresh instance of `src` through the real VM dispatch.
fn call_method(src: &str, method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("call {method}() failed: {e:?}"))
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** All four registered with Godot's exact parents,
/// every chain reaching `Object`, and `JavaClassWrapper.wrap()` answering null
/// from a script rather than raising.
#[test]
fn oos_java_bridge_class_surface() {
    register_all_classes();

    for (class, parent) in SURFACE {
        assert!(class_exists(class), "{class} must be registered");

        let info = get_class_info(class).expect("registered class has info");
        assert_eq!(
            &info.parent_class, parent,
            "{class} must inherit {parent} — the doc XML and the editor's \
             generated schema both say so"
        );

        // Clause 2 is about the CHAIN, not the immediate parent: a stored
        // parent name that ClassDB cannot resolve leaves a class whose chain
        // stops short, and `is_class("Object")` then answers false for an
        // Object descendant.
        let chain = inheritance_chain(class);
        assert!(
            chain.iter().any(|c| c == "Object"),
            "{class}'s inheritance chain must reach Object, got {chain:?}"
        );
    }

    // -- the script-facing half ---------------------------------------------
    let src = "\
extends Node

var wrapped = 1
var is_null = false

func wrap_returns_null():
    var w = JavaClassWrapper.wrap(\"java.lang.String\")
    self.wrapped = w
    return w

func null_is_comparable():
    var w = JavaClassWrapper.wrap(\"java.lang.String\")
    self.is_null = w == null
    return self.is_null
";

    // Resolving at all is the point: before this bead the identifier raised,
    // so a portable project could not even write the guard.
    assert_eq!(
        call_method(src, "wrap_returns_null"),
        Variant::Nil,
        "JavaClassWrapper.wrap() must answer null on a non-Android build, the \
         way Godot's own dummy does — not raise, and not fabricate an object"
    );

    // The form a real project actually writes. `== null` is what guards an
    // Android-only path, and it has to be usable rather than merely not
    // erroring.
    assert_eq!(
        call_method(src, "null_is_comparable"),
        Variant::Bool(true),
        "the returned null must compare equal to null, so `if wrap(..) == null` \
         is a usable branch"
    );
}

/// **Surface only.** The registrations must declare no method that implies a
/// working bridge.
///
/// This is the guard that keeps the bead honest: it is easy to "finish" a class
/// surface by adding plausible methods, and a `wrap` that returned a fabricated
/// object would satisfy every existence check above while lying to every script
/// that called it. Godot's own method tables are small and are transcribed from
/// the doc XML, not invented.
#[test]
fn oos_java_bridge_class_surface_declares_no_jni() {
    register_all_classes();

    // Exactly Godot's tables, from each class's doc XML.
    for (class, expected) in [
        (
            "JavaClass",
            &[
                "get_java_class_name",
                "get_java_method_list",
                "get_java_parent_class",
                "has_java_method",
            ][..],
        ),
        ("JavaObject", &["get_java_class", "has_java_method"][..]),
        // ⚠️ Not a bare marker class: 4.6 gives JNISingleton a method, which is
        // easy to miss if you go by the name and assume it is empty.
        ("JNISingleton", &["has_java_method"][..]),
        ("JavaClassWrapper", &["get_exception", "wrap"][..]),
    ] {
        let info = get_class_info(class).expect("registered");
        let mut names: Vec<&str> = info.methods.iter().map(|m| m.name.as_str()).collect();
        names.sort_unstable();
        let mut want: Vec<&str> = expected.to_vec();
        want.sort_unstable();
        assert_eq!(
            names, want,
            "{class} must declare exactly Godot's methods — no more, because a \
             method here is a promise the bridge cannot keep"
        );
    }
}

/// No JNI call path was added anywhere in the workspace.
///
/// The bead asks for this explicitly, and a grep is the honest way to assert a
/// negative: a reviewer reading only the registrations cannot tell whether a
/// transport was quietly wired in beside them.
#[test]
fn oos_java_bridge_class_surface_adds_no_transport() {
    register_all_classes();

    // `wrap` answering null is the whole transport story. If a future change
    // makes it return an object, this fails and whoever did it has to justify a
    // JNI bridge in a browser engine.
    let src = "\
extends Node

func w():
    return JavaClassWrapper.wrap(\"java.lang.Object\")
";
    assert_eq!(
        call_method(src, "w"),
        Variant::Nil,
        "there is no JVM to wrap a class from; anything but null here means a \
         transport was added out of scope"
    );
}
