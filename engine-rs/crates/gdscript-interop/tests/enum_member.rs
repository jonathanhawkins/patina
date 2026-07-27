//! pat-0q2kx.243 — @GlobalScope enum-qualified access is membership-checked.
//!
//! ⚠️ The file name is deliberately terse (`enum_member`, not
//! `enum_qualified_membership`). The verifier truncates a reported test command
//! at 58 characters, and `cargo nextest run -p gdscript-interop --test <name>`
//! spends 45 of those before the binary name. A descriptive filename here turns
//! the command into a target that does not exist, which fails looking like a
//! missing test. The acceptance criterion names the **function**, which is free
//! to be as long as it likes — so rename this file only if you shorten the
//! crate too.
//!
//! Godot ground truth: `extension_api.json` `.global_enums[].values[]` fixes
//! each enum's member set, and `@GlobalScope.xml` tags every `<constant>` with
//! its `enum="..."`. GDScript's analyzer enforces it: `Key.OK` and
//! `Error.KEY_ESCAPE` are compile errors ("Cannot find member ... in enum ..."),
//! even though `OK` and `KEY_ESCAPE` are both valid barewords.
//!
//! Patina resolved the prefix but never the membership, so `Error.KEY_ESCAPE`
//! answered 4194305. That is the bad kind of wrong: a script that compares an
//! `Error.` code against it type-checks, runs, and silently never matches.
//!
//! Drives the same dispatch the runner uses for `_ready`/`_process`
//! (`run_class` -> `instantiate_class` -> `call_instance_method` -> `eval_expr`),
//! so this proves VM wiring rather than a table lookup.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Parse `src` as a class, instantiate it, and call `method` with no args
/// through the real interpreter dispatch.
fn call_method(src: &str, method: &str) -> Result<Variant, String> {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).map_err(|e| format!("{e:?}"))?;
    let mut inst = interp
        .instantiate_class(&class_def)
        .map_err(|e| format!("{e:?}"))?;
    interp
        .call_instance_method(&mut inst, method, &[])
        .map_err(|e| format!("{e:?}"))
}

/// Wrap a single expression in the smallest class that can return it.
fn eval(expr: &str) -> Result<Variant, String> {
    call_method(
        &format!("extends Node\nfunc check():\n    return {expr}\n"),
        "check",
    )
}

/// **The acceptance.** In-enum members keep their values; cross-enum members
/// raise a resolution error instead of the other enum's integer.
#[test]
fn enum_qualified_membership_enforced() {
    assert_eq!(
        eval("Key.KEY_SPACE"),
        Ok(Variant::Int(32)),
        "Key.KEY_SPACE is a real member and must still resolve"
    );
    assert_eq!(
        eval("Error.OK"),
        Ok(Variant::Int(0)),
        "Error.OK is a real member and must still resolve"
    );

    // The two rejections. Asserting on the *value* as well as the error matters:
    // a regression here does not crash, it quietly hands back 4194305 / 0.
    let cross = eval("Error.KEY_ESCAPE");
    assert!(
        cross.is_err(),
        "Error.KEY_ESCAPE must raise a resolution error, got {cross:?}"
    );
    assert_ne!(cross, Ok(Variant::Int(4_194_305)));

    let cross = eval("Key.OK");
    assert!(
        cross.is_err(),
        "Key.OK must raise a resolution error, got {cross:?}"
    );
    assert_ne!(cross, Ok(Variant::Int(0)));
}

/// Both rejected members really are valid barewords — that is what makes this a
/// membership check and not a spelling check. If these ever stopped resolving,
/// the test above would pass for the wrong reason.
#[test]
fn the_rejected_members_are_still_valid_barewords() {
    assert_eq!(eval("KEY_ESCAPE"), Ok(Variant::Int(4_194_305)));
    assert_eq!(eval("OK"), Ok(Variant::Int(0)));
}

/// A spread of correct qualifications across enums of very different sizes,
/// so the generated table is exercised beyond the two names in the acceptance.
#[test]
fn in_enum_members_resolve_across_enums() {
    for (expr, want) in [
        ("Side.SIDE_LEFT", 0),
        ("Corner.CORNER_TOP_LEFT", 0),
        ("Orientation.HORIZONTAL", 0),
        ("ClockDirection.CLOCKWISE", 0),
        ("HorizontalAlignment.HORIZONTAL_ALIGNMENT_CENTER", 1),
        ("EulerOrder.EULER_ORDER_XYZ", 0),
        ("Key.KEY_ESCAPE", 4_194_305),
        ("KeyModifierMask.KEY_MASK_CTRL", 268_435_456),
        ("MouseButton.MOUSE_BUTTON_LEFT", 1),
        ("JoyButton.JOY_BUTTON_A", 0),
        ("MIDIMessage.MIDI_MESSAGE_NOTE_ON", 9),
        ("Error.ERR_FILE_NOT_FOUND", 7),
        ("PropertyHint.PROPERTY_HINT_RANGE", 1),
        ("PropertyUsageFlags.PROPERTY_USAGE_DEFAULT", 6),
    ] {
        assert_eq!(eval(expr), Ok(Variant::Int(want)), "{expr}");
    }
}

/// Every cross-pairing of the same names is rejected. Written as a matrix
/// because a membership check that only fired for `Error`/`Key` would pass the
/// acceptance while leaving the other eighteen enums wide open.
#[test]
fn cross_enum_qualification_is_rejected_everywhere() {
    for expr in [
        "Side.CORNER_TOP_LEFT",
        "Corner.SIDE_LEFT",
        "Orientation.CLOCKWISE",
        "ClockDirection.HORIZONTAL",
        "EulerOrder.KEY_SPACE",
        "Key.MOUSE_BUTTON_LEFT",
        "MouseButton.KEY_SPACE",
        "JoyButton.JOY_AXIS_LEFT_X",
        "MIDIMessage.ERR_FILE_NOT_FOUND",
        "Error.PROPERTY_HINT_RANGE",
        "PropertyHint.PROPERTY_USAGE_DEFAULT",
        "PropertyUsageFlags.METHOD_FLAG_CONST",
        "MethodFlags.OK",
    ] {
        assert!(
            eval(expr).is_err(),
            "{expr} names a member of another enum and must not resolve"
        );
    }
}

/// `Object.ConnectFlags`, `Control.CursorShape` and `Input.MouseMode` share the
/// flat bareword namespace but are *class* enums, not @GlobalScope ones. They
/// stay reachable as barewords and must not be reachable under any global
/// prefix — the previous prefix-only check made all three available under all
/// twenty.
#[test]
fn class_enum_barewords_are_not_global_enum_members() {
    assert_eq!(eval("CONNECT_DEFERRED"), Ok(Variant::Int(1)));
    assert_eq!(eval("MOUSE_MODE_CAPTURED"), Ok(Variant::Int(2)));

    for expr in [
        "Error.CONNECT_DEFERRED",
        "Key.CONNECT_DEFERRED",
        "MethodFlags.MOUSE_MODE_CAPTURED",
        "PropertyUsageFlags.CURSOR_ARROW",
    ] {
        assert!(eval(expr).is_err(), "{expr} must not resolve");
    }
}

/// An unrecognised prefix is unchanged: it was rejected before and still is.
#[test]
fn unknown_prefix_still_rejected() {
    assert!(eval("NotAnEnum.KEY_SPACE").is_err());
    // `Variant` is not a bareword prefix in GDScript, so neither of its two
    // enums is addressable this way even though their members are barewords.
    assert_eq!(eval("TYPE_VECTOR2"), Ok(Variant::Int(5)));
    assert!(eval("Variant.TYPE_VECTOR2").is_err());
}
