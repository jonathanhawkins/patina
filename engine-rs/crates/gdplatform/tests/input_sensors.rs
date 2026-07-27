//! pat-0q2kx.247 (partial) — device sensor vectors on `InputState`.
//!
//! Godot ground truth: `platform/web/js/libs/library_godot_input.js` feeds
//! `devicemotion` / `deviceorientation` into
//! `Input.set_accelerometer` / `set_gravity` / `set_gyroscope`, which
//! `Input.get_*` read back.
//!
//! ⚠️ SCOPE: stage 1 only — the state that the other two stages read and write.
//! NOT delivered: the SceneAccess/interpreter exposure, the client-side
//! devicemotion permission prompt and `/api/runtime/input/sensors` route, and
//! `vibrate_handheld`. So the bead's wiring requirement (a posted reading
//! readable from a scene script the same frame) is not yet met.

use gdcore::math::Vector3;
use gdplatform::input::InputState;

/// All four read ZERO before anything posts a reading — which is also what a
/// desktop build and a sensorless browser report. A script polling them must
/// see Godot's answer, not a panic or a garbage value.
#[test]
fn sensors_start_at_zero() {
    let s = InputState::new();
    assert_eq!(s.accelerometer(), Vector3::ZERO);
    assert_eq!(s.gravity(), Vector3::ZERO);
    assert_eq!(s.gyroscope(), Vector3::ZERO);
    assert_eq!(s.magnetometer(), Vector3::ZERO);
}

/// Each setter feeds its own vector. Distinct values per axis and per sensor,
/// so a copy-paste that wires two sensors to the same field fails.
#[test]
fn each_sensor_is_stored_independently() {
    let mut s = InputState::new();
    s.set_accelerometer(Vector3::new(1.0, 2.0, 3.0));
    s.set_gravity(Vector3::new(4.0, 5.0, 6.0));
    s.set_gyroscope(Vector3::new(7.0, 8.0, 9.0));
    s.set_magnetometer(Vector3::new(10.0, 11.0, 12.0));

    assert_eq!(s.accelerometer(), Vector3::new(1.0, 2.0, 3.0));
    assert_eq!(s.gravity(), Vector3::new(4.0, 5.0, 6.0));
    assert_eq!(s.gyroscope(), Vector3::new(7.0, 8.0, 9.0));
    assert_eq!(
        s.magnetometer(),
        Vector3::new(10.0, 11.0, 12.0),
        "each setter must write its own field — shared storage would make the \
         last write win for every sensor"
    );
}

/// A reading persists across a frame flush. Godot holds the last sensor value
/// until the platform posts a new one; clearing it per frame would make a
/// script that polls in `_process` see zero on most frames.
#[test]
fn readings_survive_a_frame_flush() {
    let mut s = InputState::new();
    s.set_accelerometer(Vector3::new(0.0, -9.8, 0.0));
    s.flush_frame();

    assert_eq!(
        s.accelerometer(),
        Vector3::new(0.0, -9.8, 0.0),
        "a sensor reading is a level, not an edge — it must not be cleared \
         with the per-frame just-pressed sets"
    );

    // And a newer reading replaces it rather than accumulating.
    s.set_accelerometer(Vector3::new(0.0, 0.0, 9.8));
    assert_eq!(s.accelerometer(), Vector3::new(0.0, 0.0, 9.8));
}

/// Sensors are independent of the key/mouse state that shares the struct, so
/// adding them cannot have disturbed existing input handling.
#[test]
fn sensors_do_not_disturb_other_input_state() {
    let mut s = InputState::new();
    s.set_gyroscope(Vector3::new(1.0, 1.0, 1.0));

    // A fresh state's key set is still empty and unaffected.
    assert!(!s.is_key_pressed(gdplatform::input::Key::Space));
    s.flush_frame();
    assert_eq!(s.gyroscope(), Vector3::new(1.0, 1.0, 1.0));
}
