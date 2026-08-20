//! pat-s3rrv.246 — ClassDB's `Time` surface must match what the interpreter
//! actually dispatches.
//!
//! # The gap
//!
//! `Time` was registered — as a bare class with **zero methods**. So
//! `ClassDB.class_exists("Time")` answered true while
//! `ClassDB.get_method_list("Time")` returned nothing, and a script doing
//! reflection over the singleton found an empty API for a subsystem that has
//! been fully dispatchable all along.
//!
//! The bead's own framing is the right one: `call_time_static` in
//! `gdscript-interop` stays the single execution path. This adds only the
//! description.
//!
//! # Why the drift assertion is the point
//!
//! A registration and a dispatch table are two hand-maintained lists of the same
//! names, and nothing makes them agree. The interesting failure is not "is
//! `Time` registered" but "does the registration promise a method the
//! interpreter will refuse". So the test does not check its own table against
//! ClassDB and stop — it takes **every name ClassDB reports** and calls it
//! through the real VM.
//!
//! ⚠️ The dispatch check asserts only that the call is not an **unknown
//! method**. Arguments are passed as zeros, so a method wanting a Dictionary
//! may well raise a type error — that is a correct engine refusing bad input,
//! not a missing method, and conflating the two would make this test either
//! unwritable or dishonest.

use gdobject::class_db::{class_exists, get_class_info, get_method_list, register_all_classes};
use gdscript_interop::interpreter::{Interpreter, RuntimeErrorKind};

/// `(method, argument count)` — transcribed from
/// `upstream/godot/doc/classes/Time.xml`, which is the oracle here because
/// `extension_api.json` (the bead's stated source) is not vendored.
const TIME_METHODS: &[(&str, usize)] = &[
    ("get_date_dict_from_system", 1),
    ("get_date_dict_from_unix_time", 1),
    ("get_date_string_from_system", 1),
    ("get_date_string_from_unix_time", 1),
    ("get_datetime_dict_from_datetime_string", 2),
    ("get_datetime_dict_from_system", 1),
    ("get_datetime_dict_from_unix_time", 1),
    ("get_datetime_string_from_datetime_dict", 2),
    ("get_datetime_string_from_system", 2),
    ("get_datetime_string_from_unix_time", 2),
    ("get_offset_string_from_offset_minutes", 1),
    ("get_ticks_msec", 0),
    ("get_ticks_usec", 0),
    ("get_time_dict_from_system", 1),
    ("get_time_dict_from_unix_time", 1),
    ("get_time_string_from_system", 1),
    ("get_time_string_from_unix_time", 1),
    ("get_time_zone_from_system", 0),
    ("get_unix_time_from_datetime_dict", 1),
    ("get_unix_time_from_datetime_string", 1),
    ("get_unix_time_from_system", 0),
];

/// Calls `Time.<method>(0, 0, …)` through the real VM and returns the error
/// kind, if any.
fn dispatch(method: &str, argc: usize) -> Option<RuntimeErrorKind> {
    let args = vec!["0"; argc].join(", ");
    let src = format!(
        "extends Node\n\
         func run():\n\
         \x20   return Time.{method}({args})\n"
    );
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "run", &[])
        .err()
        .map(|e| e.kind)
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** `Time` exists under `Object`, reports all 21
/// methods, and every one of them dispatches.
#[test]
fn time_singleton_classdb_matches_dispatch() {
    register_all_classes();

    assert!(class_exists("Time"), "Time must be registered");
    assert_eq!(
        get_class_info("Time")
            .expect("registered class has info")
            .parent_class,
        "Object",
        "Time is an Object singleton in Godot"
    );

    // A miscount in the table would weaken every assertion below without
    // failing one of them.
    assert_eq!(
        TIME_METHODS.len(),
        21,
        "Time.xml declares 21 methods; the bead says 21"
    );

    // `get_method_list` walks the inheritance chain, so this also carries
    // Object's methods. That is correct for the acceptance's containment
    // clause — a script doing reflection sees the inherited surface too.
    let reported: Vec<String> = get_method_list("Time")
        .into_iter()
        .map(|m| m.name)
        .collect();

    // ⚠️ Time's OWN methods, for the two checks below. Using the inherited list
    // there would report `get_class`, `is_class` and friends as methods Godot's
    // Time.xml does not declare, and would then demand `call_time_static`
    // dispatch them — neither of which is true or desirable.
    let own: Vec<String> = get_class_info("Time")
        .expect("registered class has info")
        .methods
        .into_iter()
        .map(|m| m.name)
        .collect();

    for (name, _) in TIME_METHODS {
        assert!(
            reported.iter().any(|r| r == name),
            "ClassDB must report Time.{name} — before this bead the list was \
             empty, so reflection over the singleton found no API at all"
        );
    }

    // ⚠️ The reverse direction too. Containment one way would pass on a
    // registration that ALSO declared methods Godot does not have, which is the
    // shape that promises a script something the interpreter will refuse.
    for name in &own {
        assert!(
            TIME_METHODS.iter().any(|(n, _)| n == name),
            "Time declares {name}, which is not in Godot's Time.xml"
        );
    }

    // -- the drift guard -----------------------------------------------------
    // Driven from what ClassDB REPORTS, not from the table above: that is what
    // makes this an agreement check between two independently maintained lists
    // rather than a restatement of one of them.
    for name in &own {
        let argc = TIME_METHODS
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, a)| *a)
            .expect("checked present above");
        if let Some(kind) = dispatch(name, argc) {
            assert!(
                !matches!(kind, RuntimeErrorKind::UndefinedFunction(_)),
                "ClassDB reports Time.{name} but the interpreter does not \
                 dispatch it — registration and dispatch have drifted apart, \
                 which is exactly what this bead exists to prevent"
            );
        }
    }
}

/// The control for the drift guard.
///
/// Without it, a `dispatch` helper that silently returned `None` for everything
/// — a typo in the script template, say — would make the loop above vacuous and
/// pass no matter what the interpreter did.
#[test]
fn time_singleton_classdb_matches_dispatch_detects_an_unknown_method() {
    let kind = dispatch("get_definitely_not_a_time_method", 0);
    assert!(
        matches!(kind, Some(RuntimeErrorKind::UndefinedFunction(_))),
        "the dispatch probe must actually detect an unknown method, or the \
         drift assertion is measuring nothing; got {kind:?}"
    );
}
