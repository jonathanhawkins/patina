//! pat-0q2kx.225 (slice 5) — `RenderingServer.global_shader_parameter_*` from
//! GDScript.
//!
//! Binary deliberately named `rs_globals`: `gdscript-interop` is a long package
//! name, so a descriptive file name pushes the verify command past the 58-char
//! truncation and it silently resolves to a missing target.
//!
//! Godot exposes project-wide shader uniforms through the RenderingServer:
//! `global_shader_parameter_add/set/get/get_type/get_list/remove` plus
//! `set_override` for `ShaderGlobalsOverride`. None of it existed here — the
//! RenderingServer singleton dispatched canvas and physics calls only.
//!
//! The store itself lives in `gdvariant`, which is the one crate both this
//! interpreter and `gdserver3d`'s shader resolver already depend on, so a value
//! set here is the same value a `global uniform` reads while rendering.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `check()` on a fresh instance of `src` through real VM dispatch.
fn call_check(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

#[test]
fn global_shader_parameter_add_set_and_get() {
    gdvariant::shader_globals::clear();

    let src = "\
extends Node

func check():
    RenderingServer.global_shader_parameter_add(\"tint\", \"float\", 1.0)
    RenderingServer.global_shader_parameter_set(\"tint\", 7.5)
    return RenderingServer.global_shader_parameter_get(\"tint\")
";
    assert_eq!(
        call_check(src),
        Variant::Float(7.5),
        "a script must be able to declare and update a global uniform",
    );
}

/// `get_type` reports the declared type as Godot's `GlobalShaderParameterType`
/// int enum, whichever spelling declared it.
///
/// ⚠️ This assertion changed in pat-s3rrv.33. It previously expected
/// `String("color")`, which was the behaviour being fixed rather than the
/// contract: Godot's `global_shader_parameter_get_type` returns
/// `enum::RenderingServer.GlobalShaderParameterType`, so a script comparing the
/// result against `RenderingServer.GLOBAL_VAR_TYPE_COLOR` could never match.
/// Declaring by NAME here (as a `project.godot` `[shader_globals]` entry does)
/// and reading back the INT is exactly the round trip that was broken.
#[test]
fn global_shader_parameter_get_type_reports_the_declared_type() {
    gdvariant::shader_globals::clear();

    let src = "\
extends Node

func check():
    RenderingServer.global_shader_parameter_add(\"sky_tint\", \"color\", 0)
    return RenderingServer.global_shader_parameter_get_type(\"sky_tint\")
";
    assert_eq!(
        call_check(src),
        Variant::Int(17),
        "GLOBAL_VAR_TYPE_COLOR is 17 (doc/classes/RenderingServer.xml)"
    );
}

/// `get_list` returns every declared name, sorted — an unstable order would
/// make the inspector and any script iterating it nondeterministic.
#[test]
fn global_shader_parameter_get_list_is_sorted() {
    gdvariant::shader_globals::clear();

    let src = "\
extends Node

func check():
    RenderingServer.global_shader_parameter_add(\"zulu\", \"float\", 0.0)
    RenderingServer.global_shader_parameter_add(\"alpha\", \"float\", 0.0)
    return RenderingServer.global_shader_parameter_get_list()
";
    match call_check(src) {
        Variant::PackedStringArray(names) => {
            assert_eq!(names, vec!["alpha".to_string(), "zulu".to_string()])
        }
        other => panic!("expected a PackedStringArray, got {other:?}"),
    }
}

/// Setting an undeclared name is refused rather than silently creating a
/// typeless entry that would fail later at shader binding.
#[test]
fn setting_an_undeclared_parameter_does_not_create_it() {
    gdvariant::shader_globals::clear();

    let src = "\
extends Node

func check():
    RenderingServer.global_shader_parameter_set(\"never_declared\", 5.0)
    return RenderingServer.global_shader_parameter_get(\"never_declared\")
";
    assert_eq!(
        call_check(src),
        Variant::Nil,
        "an undeclared global must stay absent, not spring into existence",
    );
}

/// `set_override` wins while active, and passing null reverts to the value the
/// script last set — the behaviour `ShaderGlobalsOverride` depends on.
#[test]
fn override_applies_and_reverts() {
    gdvariant::shader_globals::clear();

    let src = "\
extends Node

func check():
    RenderingServer.global_shader_parameter_add(\"wind\", \"float\", 1.0)
    RenderingServer.global_shader_parameter_set(\"wind\", 3.0)
    RenderingServer.global_shader_parameter_set_override(\"wind\", 9.0)
    var overridden = RenderingServer.global_shader_parameter_get(\"wind\")
    RenderingServer.global_shader_parameter_set_override(\"wind\", null)
    var reverted = RenderingServer.global_shader_parameter_get(\"wind\")
    return [overridden, reverted]
";
    match call_check(src) {
        Variant::Array(vals) => {
            assert_eq!(vals.len(), 2);
            assert_eq!(
                vals.to_vec()[0],
                Variant::Float(9.0),
                "override wins while active"
            );
            assert_eq!(
                vals.to_vec()[1],
                Variant::Float(3.0),
                "reverting restores the last set value, not the original default",
            );
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// `remove` drops it entirely.
#[test]
fn global_shader_parameter_remove() {
    gdvariant::shader_globals::clear();

    let src = "\
extends Node

func check():
    RenderingServer.global_shader_parameter_add(\"temp\", \"float\", 1.0)
    RenderingServer.global_shader_parameter_remove(\"temp\")
    return RenderingServer.global_shader_parameter_get_list()
";
    match call_check(src) {
        Variant::PackedStringArray(names) => assert!(
            names.is_empty(),
            "removed globals must not linger, got {names:?}",
        ),
        other => panic!("expected a PackedStringArray, got {other:?}"),
    }
}
