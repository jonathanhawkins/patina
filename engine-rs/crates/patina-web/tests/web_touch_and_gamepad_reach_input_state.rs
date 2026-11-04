//! pat-u3l8c.111 acceptance, the halves a host test can reach: touch and
//! gamepad events translate from what the browser reports and arrive in the
//! engine's `InputState`.
//!
//! `browser_input` shipped with keyboard, pointer and wheel translation plus a
//! single gamepad *axis* helper (pat-s3rrv.41). Three of the four input
//! families the bead names were therefore unreachable:
//!
//! - **touch** had no translator at all — `grep -c touch src/` was 0 — even
//!   though `InputEvent::ScreenTouch`/`ScreenDrag` have existed in
//!   `gdplatform` all along, and `InputEventScreenDrag` needs `relative` and
//!   `velocity`, which a `TouchEvent` does not carry;
//! - **gamepad buttons** had no translator, even though
//!   `GamepadButton::from_standard` was already written against the W3C
//!   `buttons[]` indices;
//! - the Gamepad API is **polled**, not evented, so nothing turned a per-frame
//!   sample into the edges a script's `_input` is written against.
//!
//! The browser listeners that call all of this are in
//! `patina_web::browser_runtime`, which is `cfg(target_arch = "wasm32")` and
//! verified by `scripts/check-wasm.sh`; everything it *decides* is here.
//!
//! The frame-rate meter is here too: "60fps" is the bead's acceptance, so the
//! rate has to be *measured* rather than asserted, and
//! `patina_web::FpsMeter` is what the page reads back.

use gdcore::math::Vector2;
use gdplatform::input::{GamepadAxis, GamepadButton, InputEvent};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use patina_web::browser_input::{
    GamepadButtonSample, GamepadSnapshot, GamepadTracker, TouchTracker, GAMEPAD_AXIS_JITTER_FLOOR,
};
use patina_web::engine::WebEngine;
use patina_web::FpsMeter;

/// A canvas at CSS (10, 20) on a 2x display — so a test that forgot either the
/// origin or the ratio cannot accidentally pass.
fn mapping_2x() -> patina_web::browser_input::CanvasMapping {
    patina_web::browser_input::CanvasMapping {
        origin_css: Vector2::new(10.0, 20.0),
        device_pixel_ratio: 2.0,
        // A non-stretching project (pat-mgixk.74 added the stretch mapping and
        // left these initializers uncompilable).
        ..Default::default()
    }
}

fn engine() -> WebEngine {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Node", "Node2D")).unwrap();
    WebEngine::new(tree)
}

/// A pad with `n` released buttons and `axes` sticks, standard-mapped.
fn pad(id: u32, buttons: usize, axes: Vec<f32>) -> GamepadSnapshot {
    GamepadSnapshot {
        id,
        standard_mapping: true,
        buttons: vec![
            GamepadButtonSample {
                pressed: false,
                value: 0.0
            };
            buttons
        ],
        axes,
    }
}

// ---------------------------------------------------------------------------
// Touch
// ---------------------------------------------------------------------------

#[test]
fn a_touch_start_maps_into_canvas_local_project_pixels() {
    let mut touches = TouchTracker::new();
    let event = touches.touch_begin(7, Vector2::new(110.0, 120.0), mapping_2x(), 0.0);
    match event {
        InputEvent::ScreenTouch {
            index,
            position,
            pressed,
            double_tap,
            canceled,
        } => {
            assert_eq!(index, 7, "the DOM Touch.identifier is the finger index");
            // (110 - 10) * 2 = 200, (120 - 20) * 2 = 200.
            assert_eq!((position.x, position.y), (200.0, 200.0));
            assert!(pressed);
            // Godot's InputState decides double_tap; deciding it here would
            // fight it.
            assert!(!double_tap);
            assert!(!canceled);
        }
        other => panic!("expected a ScreenTouch, got {other:?}"),
    }
    assert_eq!(touches.len(), 1);
}

#[test]
fn a_touch_move_carries_relative_and_velocity() {
    let mut touches = TouchTracker::new();
    touches.touch_begin(0, Vector2::new(10.0, 20.0), mapping_2x(), 1000.0);
    // 20 CSS px right over 100ms, on a 2x display => 40 project px, 400 px/s.
    let event = touches
        .touch_move(0, Vector2::new(30.0, 20.0), 0.5, mapping_2x(), 1100.0)
        .expect("a finger that is down produces a drag");
    match event {
        InputEvent::ScreenDrag {
            index,
            position,
            relative,
            velocity,
            pressure,
            tilt,
        } => {
            assert_eq!(index, 0);
            assert_eq!((position.x, position.y), (40.0, 0.0));
            assert_eq!((relative.x, relative.y), (40.0, 0.0));
            assert_eq!((velocity.x, velocity.y), (400.0, 0.0));
            assert_eq!(pressure, 0.5, "DOM Touch.force is Godot's pressure");
            // The Touch API reports no tilt; a stylus arrives as a PointerEvent.
            assert_eq!((tilt.x, tilt.y), (0.0, 0.0));
        }
        other => panic!("expected a ScreenDrag, got {other:?}"),
    }
}

#[test]
fn a_touch_move_without_a_start_is_dropped() {
    let mut touches = TouchTracker::new();
    // No previous sample exists, so `relative` would be the whole screen.
    assert!(touches
        .touch_move(3, Vector2::new(500.0, 500.0), 1.0, mapping_2x(), 0.0)
        .is_none());
    assert!(touches.is_empty());
}

#[test]
fn two_fingers_are_differenced_independently() {
    let mut touches = TouchTracker::new();
    touches.touch_begin(0, Vector2::new(10.0, 20.0), mapping_2x(), 0.0);
    touches.touch_begin(1, Vector2::new(110.0, 20.0), mapping_2x(), 0.0);
    assert_eq!(touches.len(), 2);

    // Finger 0 moves; finger 1 has not moved. Sharing one "last position"
    // would make each finger report a jump to the other's place.
    let drag = touches
        .touch_move(0, Vector2::new(20.0, 20.0), 0.0, mapping_2x(), 100.0)
        .expect("finger 0 is down");
    match drag {
        InputEvent::ScreenDrag {
            index, relative, ..
        } => {
            assert_eq!(index, 0);
            assert_eq!((relative.x, relative.y), (20.0, 0.0));
        }
        other => panic!("expected a ScreenDrag, got {other:?}"),
    }
}

#[test]
fn a_zero_or_backwards_interval_yields_no_velocity() {
    let mut touches = TouchTracker::new();
    touches.touch_begin(0, Vector2::ZERO, mapping_2x(), 500.0);
    for stamp in [500.0, 400.0] {
        let drag = touches
            .touch_move(0, Vector2::new(100.0, 0.0), 0.0, mapping_2x(), stamp)
            .expect("finger 0 is down");
        match drag {
            InputEvent::ScreenDrag { velocity, .. } => {
                assert_eq!(
                    (velocity.x, velocity.y),
                    (0.0, 0.0),
                    "timestamp {stamp} must not divide by a non-positive interval"
                );
            }
            other => panic!("expected a ScreenDrag, got {other:?}"),
        }
    }
}

#[test]
fn a_touch_end_releases_and_a_cancel_is_distinguishable() {
    let mut touches = TouchTracker::new();
    touches.touch_begin(0, Vector2::ZERO, mapping_2x(), 0.0);
    touches.touch_begin(1, Vector2::ZERO, mapping_2x(), 0.0);

    match touches.touch_end(0, Vector2::new(10.0, 20.0), false, mapping_2x()) {
        InputEvent::ScreenTouch {
            pressed, canceled, ..
        } => {
            assert!(!pressed);
            assert!(!canceled, "a touchend is a lift, not a cancel");
        }
        other => panic!("expected a ScreenTouch, got {other:?}"),
    }
    match touches.touch_end(1, Vector2::new(10.0, 20.0), true, mapping_2x()) {
        InputEvent::ScreenTouch { canceled, .. } => {
            assert!(canceled, "a touchcancel must not read as a tap");
        }
        other => panic!("expected a ScreenTouch, got {other:?}"),
    }
    assert!(touches.is_empty());
}

#[test]
fn a_release_for_an_untracked_finger_is_still_emitted() {
    let mut touches = TouchTracker::new();
    // Dropping this would leave a script believing the touch is still down.
    match touches.touch_end(9, Vector2::new(10.0, 20.0), false, mapping_2x()) {
        InputEvent::ScreenTouch { index, pressed, .. } => {
            assert_eq!(index, 9);
            assert!(!pressed);
        }
        other => panic!("expected a ScreenTouch, got {other:?}"),
    }
}

#[test]
fn release_all_cancels_every_live_finger() {
    let mut touches = TouchTracker::new();
    touches.touch_begin(0, Vector2::ZERO, mapping_2x(), 0.0);
    touches.touch_begin(4, Vector2::ZERO, mapping_2x(), 0.0);
    let released = touches.release_all();
    assert_eq!(released.len(), 2);
    for event in &released {
        match event {
            InputEvent::ScreenTouch {
                pressed, canceled, ..
            } => {
                assert!(!pressed);
                // The browser stopped reporting the gesture; no touchend will
                // ever arrive, so this is a cancel rather than a lift.
                assert!(canceled);
            }
            other => panic!("expected a ScreenTouch, got {other:?}"),
        }
    }
    assert!(touches.is_empty());
}

// ---------------------------------------------------------------------------
// Gamepad
// ---------------------------------------------------------------------------

#[test]
fn a_button_press_is_emitted_once_not_every_poll() {
    let mut pads = GamepadTracker::new();
    let mut snapshot = pad(0, 17, vec![0.0; 4]);
    snapshot.buttons[0].pressed = true;

    let first = pads.poll(std::slice::from_ref(&snapshot));
    assert_eq!(
        first,
        vec![InputEvent::GamepadButton {
            button: GamepadButton::FaceA,
            pressed: true,
            gamepad_id: 0,
        }]
    );

    // The Gamepad API re-reports a held button on every poll. An `== 1` here
    // rather than `>= 1`: a level-triggered implementation passes the first
    // assertion and fails this one.
    assert!(
        pads.poll(std::slice::from_ref(&snapshot)).is_empty(),
        "a held button must not re-emit every frame"
    );

    snapshot.buttons[0].pressed = false;
    assert_eq!(
        pads.poll(std::slice::from_ref(&snapshot)),
        vec![InputEvent::GamepadButton {
            button: GamepadButton::FaceA,
            pressed: false,
            gamepad_id: 0,
        }]
    );
}

#[test]
fn the_standard_triggers_become_analog_axes_not_buttons() {
    let mut pads = GamepadTracker::new();
    let mut snapshot = pad(0, 17, vec![0.0; 4]);
    // W3C standard mapping puts LT/RT at buttons[6]/[7]; Godot models them as
    // JOY_AXIS_TRIGGER_LEFT/_RIGHT, and `GamepadButton::to_godot_index`
    // returns None for them — so a button press here would be dropped on the
    // way to a script.
    snapshot.buttons[6] = GamepadButtonSample {
        pressed: true,
        value: 0.75,
    };
    let events = pads.poll(std::slice::from_ref(&snapshot));
    assert_eq!(
        events,
        vec![InputEvent::GamepadAxis {
            axis: GamepadAxis::LeftTriggerAnalog,
            value: 0.75,
            gamepad_id: 0,
        }]
    );
    for event in &events {
        assert!(
            !matches!(event, InputEvent::GamepadButton { .. }),
            "a trigger must not arrive as a JoyButton"
        );
    }
}

#[test]
fn stick_axes_pass_through_above_the_jitter_floor() {
    let mut pads = GamepadTracker::new();
    let snapshot = pad(0, 17, vec![0.5, -0.25, 0.0, 0.0]);
    let events = pads.poll(std::slice::from_ref(&snapshot));
    assert_eq!(
        events,
        vec![
            InputEvent::GamepadAxis {
                axis: GamepadAxis::LeftStickX,
                value: 0.5,
                gamepad_id: 0,
            },
            InputEvent::GamepadAxis {
                axis: GamepadAxis::LeftStickY,
                value: -0.25,
                gamepad_id: 0,
            },
        ],
        "the two resting axes must not announce themselves"
    );
}

#[test]
fn a_resting_stick_emits_nothing_at_all() {
    let mut pads = GamepadTracker::new();
    // A real stick at rest reports small drifting values forever. Without a
    // floor this is a fresh event every frame for a pad nobody is touching.
    let drift = GAMEPAD_AXIS_JITTER_FLOOR / 2.0;
    let snapshot = pad(0, 17, vec![drift, -drift, drift, -drift]);
    assert!(pads.poll(std::slice::from_ref(&snapshot)).is_empty());
    assert!(pads.poll(std::slice::from_ref(&snapshot)).is_empty());
}

#[test]
fn a_non_standard_pad_is_ignored_entirely() {
    let mut pads = GamepadTracker::new();
    let mut snapshot = pad(0, 17, vec![1.0; 4]);
    snapshot.standard_mapping = false;
    snapshot.buttons[0].pressed = true;
    // On a non-standard pad the indices carry no meaning, so binding index 0
    // to "A" is worse than ignoring the pad — which is the rule Godot's web
    // build follows.
    assert!(pads.poll(std::slice::from_ref(&snapshot)).is_empty());
    assert_eq!(pads.len(), 0);
}

#[test]
fn a_disconnect_releases_what_the_pad_was_holding() {
    let mut pads = GamepadTracker::new();
    let mut snapshot = pad(0, 17, vec![0.9, 0.0, 0.0, 0.0]);
    snapshot.buttons[0].pressed = true;
    snapshot.buttons[12].pressed = true;
    let pressed = pads.poll(std::slice::from_ref(&snapshot));
    assert_eq!(pressed.len(), 3, "two buttons and one axis: {pressed:?}");

    // The pad is gone from the next poll. A controller unplugged mid-jump must
    // not leave the jump button pressed forever.
    let released = pads.poll(&[]);
    assert!(released.contains(&InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: false,
        gamepad_id: 0,
    }));
    assert!(released.contains(&InputEvent::GamepadButton {
        button: GamepadButton::DPadUp,
        pressed: false,
        gamepad_id: 0,
    }));
    assert!(released.contains(&InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.0,
        gamepad_id: 0,
    }));
    assert_eq!(pads.len(), 0);
}

#[test]
fn two_pads_are_tracked_separately() {
    let mut pads = GamepadTracker::new();
    let mut first = pad(0, 17, vec![0.0; 4]);
    let second = pad(1, 17, vec![0.0; 4]);
    first.buttons[1].pressed = true;
    let events = pads.poll(&[first.clone(), second.clone()]);
    assert_eq!(
        events,
        vec![InputEvent::GamepadButton {
            button: GamepadButton::FaceB,
            pressed: true,
            gamepad_id: 0,
        }],
        "only pad 0 pressed anything"
    );
    assert_eq!(pads.len(), 2);
}

#[test]
fn release_all_drops_every_pad() {
    let mut pads = GamepadTracker::new();
    let mut snapshot = pad(0, 17, vec![0.0; 4]);
    snapshot.buttons[3].pressed = true;
    pads.poll(std::slice::from_ref(&snapshot));
    let released = pads.release_all();
    assert_eq!(
        released,
        vec![InputEvent::GamepadButton {
            button: GamepadButton::FaceY,
            pressed: false,
            gamepad_id: 0,
        }]
    );
    assert!(pads.is_empty());
}

// ---------------------------------------------------------------------------
// End to end: the translated events reach the engine's InputState
// ---------------------------------------------------------------------------

#[test]
fn touch_events_reach_the_engines_input_state() {
    let mut engine = engine();
    let mut touches = TouchTracker::new();

    let begin = touches.touch_begin(0, Vector2::new(110.0, 120.0), mapping_2x(), 0.0);
    engine.push_input(begin);
    assert!(
        engine.main_loop().input_state().is_touch_pressed(0),
        "the finger must be pollable, not only queued for _input"
    );
    assert_eq!(
        engine.main_loop().input_state().touch_position(0),
        Some(Vector2::new(200.0, 200.0))
    );

    // A frame still runs with input queued; the loop and the input path are
    // not alternatives.
    assert!(
        engine.frame(0.0).is_none(),
        "the first callback sets the origin"
    );
    assert!(engine.frame(16.0).is_some());

    let end = touches.touch_end(0, Vector2::new(110.0, 120.0), false, mapping_2x());
    engine.push_input(end);
    assert!(!engine.main_loop().input_state().is_touch_pressed(0));
}

#[test]
fn gamepad_polls_reach_the_engines_input_state() {
    let mut engine = engine();
    let mut pads = GamepadTracker::new();

    let mut snapshot = pad(2, 17, vec![0.0, 0.0, 0.6, 0.0]);
    snapshot.buttons[0].pressed = true;
    engine.push_inputs(pads.poll(std::slice::from_ref(&snapshot)));

    let state = engine.main_loop().input_state();
    assert!(
        state.is_gamepad_button_pressed(2, GamepadButton::FaceA),
        "the press must be pollable by Input.is_joy_button_pressed"
    );
    assert_eq!(
        state.get_gamepad_axis_value(2, GamepadAxis::RightStickX),
        0.6
    );

    // Unplugging the pad clears both.
    engine.push_inputs(pads.poll(&[]));
    let state = engine.main_loop().input_state();
    assert!(!state.is_gamepad_button_pressed(2, GamepadButton::FaceA));
    assert_eq!(
        state.get_gamepad_axis_value(2, GamepadAxis::RightStickX),
        0.0
    );
}

// ---------------------------------------------------------------------------
// Frame rate
// ---------------------------------------------------------------------------

#[test]
fn fps_meter_reports_sixty_for_sixty_even_frames() {
    let mut meter = FpsMeter::new(1000.0);
    // The browser clock does not start at zero.
    let origin = 1234.5;
    // The first callback only opens the window, so 61 timestamps bound 60
    // frames across exactly one second.
    for i in 0..=60 {
        meter.frame(origin + f64::from(i) * (1000.0 / 60.0));
    }
    assert!(
        (meter.fps() - 60.0).abs() < 0.01,
        "expected ~60 fps, got {}",
        meter.fps()
    );
}

#[test]
fn fps_meter_is_zero_before_the_first_window_closes() {
    let mut meter = FpsMeter::new(1000.0);
    for i in 0..30 {
        assert_eq!(meter.frame(f64::from(i) * 16.0), 0.0);
    }
}

#[test]
fn fps_meter_halves_when_the_frame_rate_halves() {
    let mut meter = FpsMeter::new(1000.0);
    for i in 0..=30 {
        meter.frame(f64::from(i) * (1000.0 / 30.0));
    }
    assert!(
        (meter.fps() - 30.0).abs() < 0.01,
        "expected ~30 fps, got {}",
        meter.fps()
    );
}

#[test]
fn fps_meter_survives_a_backwards_clock() {
    let mut meter = FpsMeter::new(1000.0);
    for i in 0..=60 {
        meter.frame(f64::from(i) * (1000.0 / 60.0));
    }
    let before = meter.fps();
    // A stepped system clock hands back a smaller timestamp.
    meter.frame(0.0);
    assert_eq!(meter.fps(), before, "a rewind must not produce a new rate");
    assert!(meter.fps() > 0.0);
}

#[test]
fn fps_meter_rejects_a_nonsensical_window() {
    for window in [0.0, -1000.0, f64::NAN] {
        let mut meter = FpsMeter::new(window);
        for i in 0..=60 {
            meter.frame(f64::from(i) * (1000.0 / 60.0));
        }
        assert!(
            (meter.fps() - 60.0).abs() < 0.01,
            "window {window} should fall back to 1s, got {}",
            meter.fps()
        );
    }
}
