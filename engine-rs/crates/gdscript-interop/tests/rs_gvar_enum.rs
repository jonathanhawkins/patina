//! pat-s3rrv.33 — `global_shader_parameter_*` speaks the
//! `GlobalShaderParameterType` int enum, not a type-name string.
//!
//! Binary deliberately short (`rs_gvar_enum`): `gdscript-interop` is a long
//! package name, and a descriptive file name pushes the verify command past the
//! truncation point where it silently resolves to a missing target — the same
//! reason `rs_globals.rs` is named the way it is.
//!
//! # The false green this replaces
//!
//! Godot types argument 1 of `global_shader_parameter_add` as
//! `GlobalShaderParameterType`, an int. Patina read it with `arg_str`, which
//! stringifies ANY Variant — so a script written the canonical Godot way, with
//! `RenderingServer.GLOBAL_VAR_TYPE_COLOR`, stored the type name `"17"`. The
//! call succeeded, `get_list` showed the parameter, and every type-dependent
//! lookup afterwards missed. `get_type` compounded it by returning a String, so
//! a script comparing against the enum could never match.
//!
//! `RenderingServer` was not even a recognised static-member holder, so the
//! canonical spelling could not be written at all.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Serializes the tests in this binary that touch the global shader-parameter table (gdvariant::shader_globals) (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

/// The enum constant resolves at all — before this bead `RenderingServer` had no
/// static members, so this expression was `Nil` and everything downstream of it
/// was meaningless.
#[test]
fn global_var_type_constants_resolve() {
    let _guard = serial();
    gdvariant::shader_globals::clear();
    let got = call_check(
        "\
extends Node

func check():
    return RenderingServer.GLOBAL_VAR_TYPE_COLOR
",
    );
    assert_eq!(
        got,
        Variant::Int(17),
        "RenderingServer.GLOBAL_VAR_TYPE_COLOR must be the int 17 \
         (doc/classes/RenderingServer.xml), not Nil"
    );
}

/// **The acceptance test.** Add with the enum, set, then read the value back and
/// the type back — the type as an INT, not the string "color" and certainly not
/// the string "17".
#[test]
fn rendering_server_global_shader_parameter_enum_type() {
    let _guard = serial();
    gdvariant::shader_globals::clear();

    let value = call_check(
        "\
extends Node

func check():
    RenderingServer.global_shader_parameter_add(\"tint\", RenderingServer.GLOBAL_VAR_TYPE_COLOR, Color(1, 0, 0, 1))
    RenderingServer.global_shader_parameter_set(\"tint\", Color(0, 1, 0, 1))
    return RenderingServer.global_shader_parameter_get(\"tint\")
",
    );
    match value {
        Variant::Color(c) => {
            assert!(
                c.g > 0.9 && c.r < 0.1,
                "the set value must win over the declared default, got {c:?}"
            );
        }
        other => panic!("expected the green Color back, got {other:?}"),
    }

    // The type round-trips as Godot's int enum. Reading it as a separate script
    // run proves it survived in the store rather than in a local.
    let ty = call_check(
        "\
extends Node

func check():
    return RenderingServer.global_shader_parameter_get_type(\"tint\")
",
    );
    assert_eq!(
        ty,
        Variant::Int(17),
        "get_type must return the GLOBAL_VAR_TYPE_COLOR int. A String here is \
         the old behaviour; Int(17) stored as the NAME \"17\" would instead \
         surface as Nil, because \"17\" is not a known type name"
    );
}

/// A `project.godot` `[shader_globals]` entry declares its type by NAME, and
/// must report the same int — the two spellings are one table.
#[test]
fn project_declared_type_name_reports_the_same_enum() {
    let _guard = serial();
    gdvariant::shader_globals::clear();
    // Exactly what `project_boot` does for a `[shader_globals]` entry.
    gdvariant::shader_globals::add("sky", "color", Variant::Nil);

    assert_eq!(
        gdvariant::shader_globals::get_type_enum("sky"),
        Some(17),
        "a project-declared \"color\" must report the same enum a script's \
         GLOBAL_VAR_TYPE_COLOR does, or the two declaration routes disagree"
    );

    let ty = call_check(
        "\
extends Node

func check():
    return RenderingServer.global_shader_parameter_get_type(\"sky\")
",
    );
    assert_eq!(ty, Variant::Int(17), "and the script sees the same int");
}

/// An out-of-range enum is a loud error, not a silently mis-declared parameter.
///
/// This is the guard against re-introducing the original bug in a new form: the
/// old code turned anything at all into a type name, so nothing was ever
/// rejected.
#[test]
fn out_of_range_enum_is_rejected() {
    let _guard = serial();
    gdvariant::shader_globals::clear();
    let mut interp = Interpreter::new();
    let class_def = interp
        .run_class(
            "\
extends Node

func check():
    RenderingServer.global_shader_parameter_add(\"bogus\", 999, 1.0)
",
        )
        .expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    let result = interp.call_instance_method(&mut inst, "check", &[]);
    assert!(
        result.is_err(),
        "999 is not a GlobalShaderParameterType and must raise, not declare a \
         parameter typed \"999\""
    );
    assert!(
        !gdvariant::shader_globals::contains("bogus"),
        "the rejected call must not have created the parameter"
    );
}
