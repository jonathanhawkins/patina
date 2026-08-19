//! pat-s3rrv.108 — `OS.is_keycode_unicode(code)`.
//!
//! Text-input handling branches on this to decide whether a key event should
//! insert a character. It was absent, so the call raised `UndefinedFunction`.
//!
//! 🔑 **The implementation is an explicit list, not a bitmask**, and that is the
//! whole subtlety. `KEY_ESCAPE` (4194305) and `KEY_KP_0` (4194438) both carry
//! Godot's `KEY_SPECIAL` bit (1 << 22), but only the first is a non-typing key:
//! the numpad digits produce characters. `code & KEY_SPECIAL` is the obvious
//! implementation and it reports every numpad key as non-typing, which is why
//! Godot enumerates 104 exceptions in `keycode_has_unicode`
//! (`core/os/keyboard.cpp:248`) instead.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = "extends Node\n\
                   \n\
                   func letter():\n\
                   \x20   return OS.is_keycode_unicode(KEY_A)\n\
                   \n\
                   func escape():\n\
                   \x20   return OS.is_keycode_unicode(KEY_ESCAPE)\n\
                   \n\
                   func f_key():\n\
                   \x20   return OS.is_keycode_unicode(KEY_F1)\n\
                   \n\
                   func numpad():\n\
                   \x20   return OS.is_keycode_unicode(KEY_KP_0)\n\
                   \n\
                   func space():\n\
                   \x20   return OS.is_keycode_unicode(KEY_SPACE)\n\
                   \n\
                   func shift():\n\
                   \x20   return OS.is_keycode_unicode(KEY_SHIFT)\n";

fn call(method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SRC).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("OS.is_keycode_unicode raised for {method}: {e:?}"))
}

#[test]
fn plain_characters_type_and_control_keys_do_not() {
    // An ordinary letter (65) is nowhere near the special range.
    assert_eq!(
        call("letter"),
        Variant::Bool(true),
        "KEY_A must be a typing key"
    );
    assert_eq!(
        call("space"),
        Variant::Bool(true),
        "KEY_SPACE types a character"
    );

    // Listed exceptions.
    assert_eq!(
        call("escape"),
        Variant::Bool(false),
        "KEY_ESCAPE is in Godot's non-Unicode list"
    );
    assert_eq!(
        call("f_key"),
        Variant::Bool(false),
        "KEY_F1 is in Godot's non-Unicode list"
    );
    assert_eq!(
        call("shift"),
        Variant::Bool(false),
        "KEY_SHIFT is a modifier, not a character"
    );
}

/// 🔑 The case that separates the real implementation from the tempting one.
///
/// ⚠️ `KEY_KP_0` is 4194438 — it has the `KEY_SPECIAL` bit set, exactly like
/// `KEY_ESCAPE`. A `code & KEY_SPECIAL` implementation therefore calls it a
/// non-typing key and every numpad digit stops inserting text, while passing
/// every other assertion in this file. Godot's list omits the keypad on
/// purpose.
#[test]
fn numpad_digits_type_even_though_they_carry_the_special_bit() {
    assert_eq!(
        call("numpad"),
        Variant::Bool(true),
        "KEY_KP_0 carries KEY_SPECIAL but DOES produce a character; a false \
         here means the implementation is masking the special bit instead of \
         using Godot's explicit exception list"
    );
}
