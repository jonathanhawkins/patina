//! pat-s3rrv.245 — `DisplayServer.Feature` constants resolve from script.
//!
//! Godot ground truth: `doc/classes/DisplayServer.xml`, the `Feature` enum.
//!
//! # Why this exists
//!
//! `NativeMenu.has_feature(NativeMenu.FEATURE_GLOBAL_MENU)` already worked —
//! `NativeMenu` carries its own constants. But the bead's acceptance also asks
//! that `DisplayServer.has_feature(DisplayServer.FEATURE_GLOBAL_MENU)` return
//! the same answer, and **no `DisplayServer.FEATURE_*` constant existed
//! anywhere**. The method answered `false` correctly; the *constant* could not
//! be named, so the expression died on `UndefinedVariable` before reaching it.
//!
//! # 🔑 The enum is NOT contiguous
//!
//! ⚠️ There are 34 constants but the highest value is **34** — value **17 is
//! absent**, so the enum jumps 16 → 18. Generating these as `0..n` from the
//! declaration order, which is the obvious shortcut, shifts
//! `FEATURE_CLIPBOARD_PRIMARY` and the sixteen constants after it down by one.
//! That is a uniform, silent drift no single-constant test would catch, which
//! is why `c_the_enum_is_not_contiguous` pins both sides of the gap.
//!
//! ⚠️ They were added **whole**, not just the one the acceptance needs: a
//! half-filled enum looks functional, because the present names work while the
//! missing ones fall through to `UndefinedVariable`.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    // ClassDB is consulted before the singleton constant table, so seed it —
    // an unseeded ClassDB is what makes a `Class.CONSTANT` lookup fail in a way
    // that reads as a missing registration (pat-s3rrv.220).
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

/// 🔑 **THE ACCEPTANCE CLAUSE.** Both singletons answer, and they agree.
#[test]
fn a_displayserver_and_nativemenu_agree_on_global_menu() {
    assert_eq!(
        run_val("return DisplayServer.FEATURE_GLOBAL_MENU\n"),
        Variant::Int(0),
        "the constant must resolve at all — it did not exist before"
    );

    let ds = run_val("return DisplayServer.has_feature(DisplayServer.FEATURE_GLOBAL_MENU)\n");
    let nm = run_val("return NativeMenu.has_feature(NativeMenu.FEATURE_GLOBAL_MENU)\n");

    assert_eq!(
        ds,
        Variant::Bool(false),
        "no global menu on this target, and it must not error"
    );
    assert_eq!(
        ds, nm,
        "DisplayServer and NativeMenu must give the SAME answer — Godot's web \
         DisplayServer delegates FEATURE_GLOBAL_MENU straight to its NativeMenu"
    );
}

/// The two enums share the value, which is what makes the agreement meaningful.
///
/// ⚠️ If `DisplayServer.FEATURE_GLOBAL_MENU` and `NativeMenu.FEATURE_GLOBAL_MENU`
/// were different numbers, the test above would still pass — both calls return
/// false regardless of the argument — while the two singletons were in fact
/// being asked different questions.
#[test]
fn b_both_singletons_name_the_same_value() {
    assert_eq!(
        run_val("return NativeMenu.FEATURE_GLOBAL_MENU\n"),
        Variant::Int(0)
    );
    assert_eq!(
        run_val("return DisplayServer.FEATURE_GLOBAL_MENU\n"),
        Variant::Int(0)
    );
}

/// 🔑 The gap at 17.
///
/// ⚠️ This is the assertion a derived `0..n` table fails. Godot's `Feature`
/// enum skips **17**, so `FEATURE_CLIPBOARD_PRIMARY` is **18**, and the highest
/// value is 34 across 34 constants.
#[test]
fn c_the_enum_is_not_contiguous() {
    // Either side of the hole.
    assert_eq!(
        run_val("return DisplayServer.FEATURE_SWAP_BUFFERS\n"),
        Variant::Int(16),
        "the last value before the gap"
    );
    assert_eq!(
        run_val("return DisplayServer.FEATURE_CLIPBOARD_PRIMARY\n"),
        Variant::Int(18),
        "18, NOT 17 — value 17 is absent from Godot's enum, so a sequential \
         mapping shifts this and everything after it down by one"
    );

    // And the far end, which a shifted table would report as 33.
    assert_eq!(
        run_val("return DisplayServer.FEATURE_ACCESSIBILITY_SCREEN_READER\n"),
        Variant::Int(34),
        "34 constants but the highest value is 34, because of the gap"
    );
}

/// A spread of other members, so the table is not just its first entry.
#[test]
fn d_the_rest_of_the_enum_resolves_too() {
    for (name, value) in [
        ("FEATURE_SUBWINDOWS", 1),
        ("FEATURE_TOUCHSCREEN", 2),
        ("FEATURE_MOUSE", 3),
        ("FEATURE_CLIPBOARD", 5),
        ("FEATURE_TEXT_TO_SPEECH", 19),
        ("FEATURE_SCREEN_CAPTURE", 21),
    ] {
        assert_eq!(
            run_val(&format!("return DisplayServer.{name}\n")),
            Variant::Int(value),
            "DisplayServer.{name}"
        );
    }
}
