//! pat-s3rrv.76 — the wheel-tilt and thumb buttons Patina's `MouseButton` was
//! missing.
//!
//! The enum stopped at `WheelDown`, so `MOUSE_BUTTON_WHEEL_LEFT` (6),
//! `WHEEL_RIGHT` (7), `XBUTTON1` (8) and `XBUTTON2` (9) could not be
//! represented at all. The editor server's browser-button mapping made the gap
//! visible: DOM `MouseEvent.button` 3 and 4 (the thumb buttons) hit a
//! `_ => return` and were discarded before reaching the running game, so a
//! project binding back/forward saw nothing and had no error to look at.

use gdplatform::input::MouseButton;

/// `@GlobalScope.MouseButton`, from extension_api.json.
const MOUSE_BUTTON_WHEEL_LEFT: i64 = 6;
const MOUSE_BUTTON_WHEEL_RIGHT: i64 = 7;
const MOUSE_BUTTON_XBUTTON1: i64 = 8;
const MOUSE_BUTTON_XBUTTON2: i64 = 9;

/// `@GlobalScope.MouseButtonMask`. Listed separately from the indices on
/// purpose — see the mask test.
const MOUSE_BUTTON_MASK_MB_XBUTTON1: i64 = 128;
const MOUSE_BUTTON_MASK_MB_XBUTTON2: i64 = 256;

#[test]
fn the_new_buttons_carry_godot_indices() {
    assert_eq!(
        MouseButton::WheelLeft.to_godot_index(),
        MOUSE_BUTTON_WHEEL_LEFT
    );
    assert_eq!(
        MouseButton::WheelRight.to_godot_index(),
        MOUSE_BUTTON_WHEEL_RIGHT
    );
    assert_eq!(
        MouseButton::XButton1.to_godot_index(),
        MOUSE_BUTTON_XBUTTON1
    );
    assert_eq!(
        MouseButton::XButton2.to_godot_index(),
        MOUSE_BUTTON_XBUTTON2
    );
}

/// ⚠️ The load-bearing test. `to_godot_mask` is DERIVED as
/// `1 << (to_godot_index() - 1)`, so a wrong index yields a wrong mask
/// silently — and `button_mask` is what a script reads to ask "is the thumb
/// button held".
///
/// Asserting Godot's documented mask constants therefore checks the indices a
/// second, independent way: 128 and 256 only come out if the indices really are
/// 8 and 9. An off-by-one that looked plausible in the index test (say 7 and 8)
/// would produce 64 and 128 here and be caught.
#[test]
fn the_thumb_button_masks_match_godot() {
    assert_eq!(
        MouseButton::XButton1.to_godot_mask(),
        MOUSE_BUTTON_MASK_MB_XBUTTON1,
        "XBUTTON1's mask is 1 << 7"
    );
    assert_eq!(
        MouseButton::XButton2.to_godot_mask(),
        MOUSE_BUTTON_MASK_MB_XBUTTON2,
        "XBUTTON2's mask is 1 << 8"
    );
}

/// No index collides, which is what keeps the existing buttons working — the
/// four additions sit above `WheelDown` rather than renumbering anything.
#[test]
fn every_mouse_button_index_is_distinct() {
    let all = [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::WheelUp,
        MouseButton::WheelDown,
        MouseButton::WheelLeft,
        MouseButton::WheelRight,
        MouseButton::XButton1,
        MouseButton::XButton2,
    ];
    let mut seen: Vec<i64> = all.iter().map(|b| b.to_godot_index()).collect();
    seen.sort_unstable();
    let before = seen.len();
    seen.dedup();
    assert_eq!(
        before,
        seen.len(),
        "two buttons share a Godot index: {seen:?}"
    );
    assert_eq!(
        seen,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
        "the nine buttons must cover Godot's 1..=9 with no gaps"
    );
}
