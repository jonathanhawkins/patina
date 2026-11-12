//! pat-s3rrv.11 acceptance: script-declared named enums resolve at runtime.
//!
//! `enum State { IDLE, RUN }` then `State.IDLE` is the standard GDScript
//! state-machine idiom (upstream/godot/modules/gdscript/gdscript_parser.cpp
//! `EnumNode`; gdscript_analyzer.cpp resolves `Enum.MEMBER` against the
//! script's own enums).
//!
//! Patina parsed named enums into `ClassDef::enums` and then **read that map
//! from nowhere** — it was a write-only field, with `Stmt::EnumDecl` an
//! explicit no-op in `exec_stmt` and only `class_def.constants` seeded into
//! method scope. So `State.IDLE` fell through to the @GlobalScope
//! enum-qualified resolver, which rejects unknown prefixes, and died with
//! `UndefinedVariable`. Anonymous `enum { A, B }` worked the whole time,
//! because its variants are folded into `constants` at parse time — which is
//! why this gap survived: the feature looked half-present.
//!
//! Drives the same dispatch the runner uses for `_ready`/`_process`
//! (`run_class` -> `instantiate_class` -> `call_instance_method` -> `eval_expr`),
//! so this proves VM wiring rather than a table lookup.

use gdscript_interop::interpreter::{ClassInstance, Interpreter};
use gdvariant::Variant;

/// Parse `src` as a class, instantiate it, and call `method` with no args
/// through the real interpreter dispatch. Returns the instance too, so a test
/// can read back what the method assigned to `self`.
fn call_method(src: &str, method: &str) -> Result<(ClassInstance, Variant), String> {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).map_err(|e| format!("{e:?}"))?;
    let mut inst = interp
        .instantiate_class(&class_def)
        .map_err(|e| format!("{e:?}"))?;
    let ret = interp
        .call_instance_method(&mut inst, method, &[])
        .map_err(|e| format!("{e:?}"))?;
    Ok((inst, ret))
}

/// Wrap a single expression in the smallest class that also declares an enum.
fn eval_with_enum(expr: &str) -> Result<Variant, String> {
    call_method(
        &format!(
            "extends Node\nenum State {{ IDLE, RUN = 5, DONE }}\nfunc check():\n    return {expr}\n"
        ),
        "check",
    )
    .map(|(_, v)| v)
}

/// **The acceptance test.**
///
/// A scene-attached script assigns an enum member to a property from `_ready`
/// and compares against it; the value read back off the instance is the
/// explicit `5`, not `Nil` and not a coincidental `0`.
#[test]
fn gds_named_enums() {
    let src = "\
extends Node
enum State { IDLE, RUN = 5, DONE }
var state = -1
var matched = false
var implicit = -1
var after_explicit = -1
func _ready():
    self.state = State.RUN
    self.matched = self.state == State.RUN
    self.implicit = State.IDLE
    self.after_explicit = State.DONE
";
    let (inst, _) = call_method(src, "_ready").expect("_ready runs");

    // The property survives the call and holds the enum's explicit value.
    assert_eq!(
        inst.properties.get("state"),
        Some(&Variant::Int(5)),
        "self.state = State.RUN must store the enum's declared value 5"
    );
    // ⚠️ Guards the vacuous pass: `-1` would mean `_ready` never assigned, and
    // `Nil` would mean the member resolved to nothing.
    assert_ne!(
        inst.properties.get("state"),
        Some(&Variant::Int(-1)),
        "state is still its initialiser — _ready did not run"
    );
    assert_eq!(
        inst.properties.get("matched"),
        Some(&Variant::Bool(true)),
        "a stored enum value must compare equal to the same member"
    );
    // Implicit numbering starts at 0, and resumes from the last explicit value.
    assert_eq!(
        inst.properties.get("implicit"),
        Some(&Variant::Int(0)),
        "the first variant with no `= N` is 0"
    );
    assert_eq!(
        inst.properties.get("after_explicit"),
        Some(&Variant::Int(6)),
        "a variant after `RUN = 5` continues at 6, matching Godot's numbering"
    );
}

/// A member the enum does not declare must RAISE.
///
/// This is the assertion that constrains the implementation: the enum name is
/// also bound as a Dictionary, so a "just look it up and fall through" design
/// would read the missing key back as `Nil` and `State.WALK` would silently
/// compare equal to nothing. It must also not fall through to @GlobalScope and
/// answer an unrelated constant.
#[test]
fn unknown_enum_member_raises() {
    let err = eval_with_enum("State.WALK").expect_err("State.WALK must not resolve");
    assert!(
        err.contains("WALK") && err.contains("State"),
        "the error should name the member and the enum, got: {err}"
    );
}

/// A name that is genuinely a @GlobalScope enum still resolves — the new arm
/// returns "not mine" for anything the script did not declare rather than
/// swallowing every `Ident.MEMBER`.
#[test]
fn global_scope_enums_still_resolve_alongside_a_script_enum() {
    // Resolved through the existing global table; its exact value is pinned by
    // tests/enum_member.rs, so here only "a script enum did not shadow it"
    // matters.
    assert!(
        matches!(
            eval_with_enum("Key.KEY_SPACE"),
            Ok(Variant::Int(_)) | Ok(Variant::Float(_))
        ),
        "declaring a script enum must not shadow the @GlobalScope resolver, got {:?}",
        eval_with_enum("Key.KEY_SPACE")
    );
    assert!(
        eval_with_enum("NotAnEnum.KEY_SPACE").is_err(),
        "an unknown prefix is still rejected"
    );
}

/// The enum name used *as a value* is a Dictionary, which is what makes the
/// `values()` / `keys()` / `size()` access in Godot's enum idiom work.
#[test]
fn enum_name_is_a_dictionary_of_its_variants() {
    let size = eval_with_enum("State.size()").expect("State.size()");
    assert_eq!(size, Variant::Int(3), "three declared variants");

    let values = eval_with_enum("State.values()").expect("State.values()");
    match values {
        Variant::Array(items) => assert_eq!(
            items,
            vec![Variant::Int(0), Variant::Int(5), Variant::Int(6)].into(),
            "values() yields the variant values in declaration order"
        ),
        other => panic!("values() should return an Array, got {other:?}"),
    }

    let keys = eval_with_enum("State.keys()").expect("State.keys()");
    match keys {
        Variant::Array(items) => assert_eq!(
            items,
            vec![
                Variant::String("IDLE".to_string()),
                Variant::String("RUN".to_string()),
                Variant::String("DONE".to_string()),
            ]
            .into(),
            "keys() yields the variant names in declaration order"
        ),
        other => panic!("keys() should return an Array, got {other:?}"),
    }
}

/// Resolution works in any function reached from a method call, not just the
/// one the interpreter happened to enter — the enum stays visible through a
/// nested `call_user_func`, which seeds no class members of its own.
#[test]
fn enum_resolves_inside_a_nested_call() {
    let src = "\
extends Node
enum State { IDLE, RUN = 5 }
var seen = -1
func pick():
    return State.RUN
func _ready():
    self.seen = pick()
";
    let (inst, _) = call_method(src, "_ready").expect("_ready runs");
    assert_eq!(
        inst.properties.get("seen"),
        Some(&Variant::Int(5)),
        "an enum member must resolve inside a function called from _ready"
    );
}

/// Regression guard: anonymous enums already worked by folding their variants
/// into class constants, and must keep working unchanged.
#[test]
fn anonymous_enum_variants_are_still_bare_constants() {
    let src = "\
extends Node
enum { ALPHA, BETA = 7 }
var a = -1
var b = -1
func _ready():
    self.a = ALPHA
    self.b = BETA
";
    let (inst, _) = call_method(src, "_ready").expect("_ready runs");
    assert_eq!(
        inst.properties.get("a"),
        Some(&Variant::Int(0)),
        "anonymous enum variants stay reachable as bare constants"
    );
    assert_eq!(inst.properties.get("b"), Some(&Variant::Int(7)));
}
