//! pat-s3rrv.41 acceptance: browser-shaped input reaches the engine.
//!
//! The browser delivers DOM events in CSS pixels with its own key names, wheel
//! units and gamepad layout; Godot's `InputState` wants `InputEvent` in project
//! pixels with physical keycodes. Nothing bridged the two — `gdrender2d`'s
//! `BrowserInputEvent` covers only `Key` and belongs to the dev HTTP server, and
//! `Key::from_name`, which already understands `KeyboardEvent.code` spellings,
//! had **zero callers** anywhere in the workspace.
//!
//! Each case here is the browser shape the acceptance names.

use gdcore::math::Vector2;
use gdplatform::input::{GamepadAxis, InputEvent, Key, MouseButton};
use patina_web::browser_input::{
    gamepad_axis, key_event, pointer_button, pointer_move, wheel, CanvasMapping, WheelDeltaMode,
};

/// A canvas at (40, 20) CSS on a 2x display — the acceptance's HiDPI case.
fn mapping_2x() -> CanvasMapping {
    CanvasMapping {
        origin_css: Vector2::new(40.0, 20.0),
        device_pixel_ratio: 2.0,
        // A non-stretching project (pat-mgixk.74 added the stretch mapping and
        // left these initializers uncompilable).
        ..Default::default()
    }
}

/// `keydown` with `code = "ArrowRight"` becomes the physical Right key.
#[test]
fn keydown_maps_the_physical_code() {
    match key_event("ArrowRight", true, false, false, false) {
        Some(InputEvent::Key { key, pressed, .. }) => {
            assert_eq!(key, Key::Right, "ArrowRight must map to the Right key");
            assert!(pressed);
        }
        other => panic!("expected a Key event, got {other:?}"),
    }
}

/// Modifier flags survive, and an unknown code is dropped rather than guessed at.
#[test]
fn modifiers_survive_and_unknown_codes_are_dropped() {
    match key_event("KeyA", true, true, false, true) {
        Some(InputEvent::Key {
            shift, ctrl, alt, ..
        }) => {
            assert!(
                shift && alt && !ctrl,
                "modifier flags must be carried through"
            );
        }
        other => panic!("expected a Key event, got {other:?}"),
    }
    assert!(
        key_event("NotARealKey", true, false, false, false).is_none(),
        "an unmappable code must be dropped, not mapped to an arbitrary key"
    );
}

/// **The case that caught a real bug.** The DOM spells letters `"KeyA"` and
/// digits `"Digit1"`, while `Key::from_name` takes the bare `"A"` / `"1"`.
///
/// My first version passed the code through untouched. It looked correct because
/// `"ArrowRight"` IS accepted verbatim — so the arrow test passed while every
/// letter silently produced `None`, i.e. WASD movement would have done nothing
/// in a real browser. Testing only the arrow key hid it.
#[test]
fn prefixed_dom_codes_map_to_their_bare_keys() {
    for (code, expected) in [
        ("KeyA", Key::A),
        ("KeyW", Key::W),
        ("KeyS", Key::S),
        ("KeyD", Key::D),
    ] {
        match key_event(code, true, false, false, false) {
            Some(InputEvent::Key { key, .. }) => {
                assert_eq!(key, expected, "{code} must map to {expected:?}")
            }
            other => panic!("{code} must map to a Key event, got {other:?}"),
        }
    }
    match key_event("Digit1", true, false, false, false) {
        Some(InputEvent::Key { key, .. }) => assert_eq!(key, Key::Num1),
        other => panic!("Digit1 must map to a Key event, got {other:?}"),
    }
    // Named keys are already accepted verbatim and must not be mangled.
    assert!(
        key_event("Space", true, false, false, false).is_some(),
        "a named key must still map after normalization"
    );
}

/// **The HiDPI case.** A pointer at a viewport CSS point lands at the
/// canvas-local project pixel: subtract the canvas origin, then scale by the
/// device pixel ratio. Getting either wrong is why a click lands away from the
/// cursor.
#[test]
fn pointermove_converts_css_to_canvas_local_project_pixels() {
    let event = pointer_move(
        Vector2::new(140.0, 120.0),
        Vector2::new(3.0, -4.0),
        mapping_2x(),
    );
    match event {
        InputEvent::MouseMotion { position, relative } => {
            // (140-40)*2 = 200, (120-20)*2 = 200
            assert!(
                (position.x - 200.0).abs() < 0.001 && (position.y - 200.0).abs() < 0.001,
                "expected (200,200) project pixels, got {position:?}"
            );
            // movementX/Y is already a delta: scaled, not translated.
            assert!(
                (relative.x - 6.0).abs() < 0.001 && (relative.y + 8.0).abs() < 0.001,
                "relative must scale by dpr without the origin offset, got {relative:?}"
            );
        }
        other => panic!("expected MouseMotion, got {other:?}"),
    }
}

/// A click carries the same converted position, and DOM button numbering maps
/// to Godot's buttons.
#[test]
fn pointer_buttons_map_and_carry_position() {
    match pointer_button(2, true, Vector2::new(140.0, 120.0), mapping_2x()) {
        Some(InputEvent::MouseButton {
            button, position, ..
        }) => {
            assert_eq!(
                button,
                MouseButton::Right,
                "DOM button 2 is the right button"
            );
            assert!((position.x - 200.0).abs() < 0.001, "got {position:?}");
        }
        other => panic!("expected MouseButton, got {other:?}"),
    }
    assert!(
        pointer_button(9, true, Vector2::ZERO, mapping_2x()).is_none(),
        "an unmapped DOM button must be dropped"
    );
}

/// **The `deltaMode` case.** The same physical scroll arrives as ~48 in PIXEL
/// mode and 3 in LINE mode; normalizing through `deltaMode` makes them
/// comparable. Ignoring it makes a line-mode mouse ~16x weaker than a trackpad.
#[test]
fn wheel_normalizes_delta_mode() {
    let line = wheel(
        3.0,
        WheelDeltaMode::Line,
        Vector2::ZERO,
        CanvasMapping::default(),
    );
    let pixel = wheel(
        48.0,
        WheelDeltaMode::Pixel,
        Vector2::ZERO,
        CanvasMapping::default(),
    );

    assert_eq!(line.len(), 2, "a wheel notch is a press and a release");
    assert_eq!(pixel.len(), 2);

    let dir = |evs: &[InputEvent]| match &evs[0] {
        InputEvent::MouseButton { button, .. } => *button,
        other => panic!("expected MouseButton, got {other:?}"),
    };
    assert_eq!(
        dir(&line),
        dir(&pixel),
        "3 lines and 48 pixels are the same scroll and must agree in direction"
    );
    assert_eq!(
        dir(&line),
        MouseButton::WheelDown,
        "the DOM's +deltaY is downward"
    );
}

/// A wheel notch is press THEN release — Godot models scrolling as a button, and
/// a press with no release leaves the button stuck down forever.
#[test]
fn a_wheel_notch_presses_and_releases() {
    let evs = wheel(
        1.0,
        WheelDeltaMode::Line,
        Vector2::ZERO,
        CanvasMapping::default(),
    );
    match (&evs[0], &evs[1]) {
        (
            InputEvent::MouseButton { pressed: true, .. },
            InputEvent::MouseButton { pressed: false, .. },
        ) => {}
        other => panic!("expected a press then a release, got {other:?}"),
    }
    assert!(
        wheel(
            0.0,
            WheelDeltaMode::Pixel,
            Vector2::ZERO,
            CanvasMapping::default()
        )
        .is_empty(),
        "a zero delta must not emit a spurious click"
    );
}

/// A gamepad axis passes through, and the deadzone silences a resting stick —
/// which the browser does not do for us.
#[test]
fn gamepad_axis_passes_through_with_a_deadzone() {
    match gamepad_axis(0, 1.0, 0, 0.2) {
        Some(InputEvent::GamepadAxis { axis, value, .. }) => {
            assert_eq!(axis, GamepadAxis::LeftStickX, "index 0 is the left stick X");
            assert!((value - 1.0).abs() < 0.001);
        }
        other => panic!("expected GamepadAxis, got {other:?}"),
    }
    match gamepad_axis(0, 0.05, 0, 0.2) {
        Some(InputEvent::GamepadAxis { value, .. }) => assert_eq!(
            value, 0.0,
            "a resting stick inside the deadzone must report exactly 0"
        ),
        other => panic!("expected GamepadAxis, got {other:?}"),
    }
    assert!(
        gamepad_axis(99, 1.0, 0, 0.2).is_none(),
        "an unmapped axis index must be dropped"
    );
}

/// A 1x display is the identity case — the conversion must not distort a
/// non-HiDPI canvas at the origin.
#[test]
fn a_1x_canvas_at_the_origin_is_the_identity() {
    let m = CanvasMapping::default();
    let p = m.to_project(Vector2::new(123.0, 45.0));
    assert!(
        (p.x - 123.0).abs() < 0.001 && (p.y - 45.0).abs() < 0.001,
        "got {p:?}"
    );
}
