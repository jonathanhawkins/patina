//! pat-s3rrv.58 acceptance — the `Color` builtin surface works from GDScript.
//!
//! Godot's `Color` exposes 26 methods, 14 members (r/g/b/a, the 8-bit views and
//! the HSV/OKHSL decompositions) and 146 named constants
//! (`core/math/color.cpp`, `core/math/color_names.inc`).
//!
//! Patina had almost none of it reachable from a script. `Variant::Color` had no
//! arm in `call_method_on`, so `col.lightened(0.2)` hit the catch-all and
//! errored "cannot call method on Color"; the methods did not exist in the
//! engine either. The static-member table hardcoded SIX constants inline, and
//! because match arms are exclusive its `None` never reached the fallback that
//! would have found the rest — so `Color.YELLOW`, `Color.CYAN`, `Color.ORANGE`
//! and 140 others resolved to nothing.
//!
//! These drive real GDScript through the interpreter and assert Godot's values.

use gdcore::math::Color;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Evaluates `expr` inside a script and returns its value.
///
/// Mirrors the call sequence of the green `class_enum_constants_from_script`
/// test — `run_class` → `instantiate_class` → `call_instance_method` — rather
/// than inventing one.
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

/// `eval` that expects a failure — used for the invalid-colour case.
fn eval_err(expr: &str) -> bool {
    gdobject::class_db::register_all_classes();
    let src = format!("extends Node\n\nfunc check():\n    return {expr}\n");
    let mut interp = Interpreter::new();
    let Ok(class_def) = interp.run_class(&src) else {
        return true;
    };
    let Ok(mut inst) = interp.instantiate_class(&class_def) else {
        return true;
    };
    interp
        .call_instance_method(&mut inst, "check", &[])
        .is_err()
}

fn color_of(v: Variant) -> Color {
    match v {
        Variant::Color(c) => c,
        other => panic!("expected a Color, got {other:?}"),
    }
}

/// **The acceptance test.** Every clause of the bead's criteria.
#[test]
fn color_builtin_surface_from_scene() {
    // `Color.YELLOW.darkened(0.2)` — a named constant AND an instance method,
    // the two halves that were both unreachable before this bead.
    let dark = color_of(eval("Color.YELLOW.darkened(0.2)"));
    assert!(
        dark.is_equal_approx(Color::new(0.8, 0.8, 0.0, 1.0)),
        "Color.YELLOW.darkened(0.2) must be (0.8, 0.8, 0), got {dark:?}"
    );

    // The single-argument html constructor plus serialisation.
    assert_eq!(
        eval("Color(\"#ff8800\").to_html(false)"),
        Variant::String("ff8800".into()),
        "Color(\"#ff8800\").to_html(false) must be \"ff8800\""
    );

    // `Color.from_hsv(0.5, 1, 1)` is cyan.
    let cyan = color_of(eval("Color.from_hsv(0.5, 1.0, 1.0)"));
    assert!(
        cyan.is_equal_approx(Color::new(0.0, 1.0, 1.0, 1.0)),
        "from_hsv(0.5,1,1) must be cyan, got {cyan:?}"
    );

    let inv = color_of(eval("Color(1.0, 1.0, 0.0).inverted()"));
    assert!(
        inv.is_equal_approx(Color::new(0.0, 0.0, 1.0, 1.0)),
        "{inv:?}"
    );

    // Rec. 709 weights, so yellow is 0.2126 + 0.7152.
    match eval("Color(1.0, 1.0, 0.0).get_luminance()") {
        Variant::Float(l) => assert!((l - 0.9278).abs() < 1e-3, "luminance {l}"),
        other => panic!("expected Float, got {other:?}"),
    }

    // Half-alpha blue over opaque red meets in the middle.
    let blended = color_of(eval(
        "Color(1.0, 0.0, 0.0).blend(Color(0.0, 0.0, 1.0, 0.5))",
    ));
    assert!(
        blended.is_equal_approx(Color::new(0.5, 0.0, 0.5, 1.0)),
        "{blended:?}"
    );

    // ⚠️ `r8` ROUNDS, so 0.5 is 128 rather than 127 — truncating would pull
    // every converted colour one step dark.
    assert_eq!(
        eval("Color(0.5, 0.0, 1.0).r8"),
        Variant::Int(128),
        "r8 must round, not truncate"
    );
    // ⚠️ Hue is 0..1, NOT degrees.
    match eval("Color(0.0, 1.0, 1.0).h") {
        Variant::Float(h) => assert!((h - 0.5).abs() < 1e-4, "cyan hue should be 0.5, got {h}"),
        other => panic!("expected Float hue, got {other:?}"),
    }
}

/// The constants beyond the six that used to be hardcoded — the half that
/// silently resolved to nothing before.
#[test]
fn every_named_colour_constant_resolves() {
    for (name, expect) in [
        ("YELLOW", Color::new(1.0, 1.0, 0.0, 1.0)),
        ("CYAN", Color::new(0.0, 1.0, 1.0, 1.0)),
        ("ORANGE", Color::from_html("ffa500").unwrap()),
        ("REBECCA_PURPLE", Color::from_html("663399").unwrap()),
        // ⚠️ Godot's palette is X11, not CSS: GRAY is bebebe, and the CSS
        // value lives on WEB_GRAY. Taking the CSS one is the easy mistake.
        ("GRAY", Color::from_html("bebebe").unwrap()),
        ("WEB_GRAY", Color::from_html("808080").unwrap()),
    ] {
        let got = color_of(eval(&format!("Color.{name}")));
        assert!(
            got.is_equal_approx(expect),
            "Color.{name} must be {expect:?}, got {got:?}"
        );
    }

    // The six that always worked must keep working.
    assert!(color_of(eval("Color.RED")).is_equal_approx(Color::new(1.0, 0.0, 0.0, 1.0)));
    assert!(color_of(eval("Color.WHITE")).is_equal_approx(Color::WHITE));
}

/// A bad colour string is an error, not a silent black — a wrong colour
/// surfaces as a bad-looking scene long after the mistake, with nothing
/// pointing back to the constructor.
#[test]
fn an_invalid_colour_string_is_an_error() {
    assert!(
        eval_err("Color(\"nonsense\")"),
        "Color(\"nonsense\") must error rather than yielding black"
    );
}
