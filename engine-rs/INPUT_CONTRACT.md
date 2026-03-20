# Engine Input Contract

This document defines the input contract that the Patina engine exposes to
examples and tests. All examples and integration tests depend on these APIs
behaving consistently. Changes to these contracts require updating dependent
tests.

## Event Flow

```
Platform (OS/HeadlessPlatform)
  → WindowEvent (Key, Mouse, Gamepad, Resize, Close, Focus)
    → PlatformBackend::poll_events() drains to Vec<WindowEvent>
      → MainLoop::run_frame() converts input-compatible events
        → InputState::process_event(InputEvent)
          → Action resolution via InputMap
            → Scripts / tests query InputState or InputSnapshot
```

Non-input window events (`Resized`, `CloseRequested`, `FocusGained`,
`FocusLost`) flow through `poll_events()` but are **not** routed to
`InputState`. They are handled at the platform level (resize updates viewport
size, close triggers quit).

## Core Types

### WindowEvent (`gdplatform::window::WindowEvent`)

Raw events from the platform layer. Only keyboard, mouse, and gamepad variants
convert to `InputEvent` via `to_input_event()`.

| Variant | Converts to InputEvent |
|---------|----------------------|
| `KeyInput { key, pressed, shift, ctrl, alt }` | Yes |
| `MouseInput { button, pressed, position }` | Yes |
| `MouseMotion { position, relative }` | Yes |
| `Resized { width, height }` | No |
| `CloseRequested` | No |
| `FocusGained` | No |
| `FocusLost` | No |

### InputEvent (`gdplatform::input::InputEvent`)

Engine-internal input event consumed by `InputState`.

| Variant | Fields |
|---------|--------|
| `Key` | `key`, `pressed`, `shift`, `ctrl`, `alt` |
| `MouseButton` | `button`, `pressed`, `position` |
| `MouseMotion` | `position`, `relative` |
| `GamepadButton` | `gamepad_id`, `button`, `pressed` |
| `GamepadAxis` | `gamepad_id`, `axis`, `value` |
| `Action` | `name`, `pressed`, `strength` |
| `ScreenTouch` | `index`, `position`, `pressed` |
| `ScreenDrag` | `index`, `position`, `relative`, `velocity` |

### InputState (`gdplatform::input::InputState`)

Singleton-style state tracker. Owns the current frame's input and resolves
action bindings via its `InputMap`.

**Per-frame lifecycle:**
1. `process_event(event)` — called for each input event during `run_frame`
2. Scripts / test code query state (see Query API below)
3. `flush_frame()` — clears `just_pressed` / `just_released` flags

**Query API (available to examples and tests):**

| Method | Returns |
|--------|---------|
| `is_key_pressed(key)` | `bool` — true while held |
| `is_key_just_pressed(key)` | `bool` — true only on the press frame |
| `is_key_just_released(key)` | `bool` — true only on the release frame |
| `is_mouse_button_pressed(button)` | `bool` |
| `is_mouse_button_just_pressed(button)` | `bool` |
| `is_mouse_button_just_released(button)` | `bool` |
| `get_mouse_position()` | `Vector2` |
| `is_action_pressed(name)` | `bool` — resolved via InputMap |
| `is_action_just_pressed(name)` | `bool` |
| `is_action_just_released(name)` | `bool` |
| `get_action_strength(name)` | `f32` — 1.0 for digital, analog for axes |
| `get_axis(negative, positive)` | `f32` — composite axis from two actions |
| `get_vector(neg_x, pos_x, neg_y, pos_y)` | `Vector2` — normalized 2D input |
| `is_gamepad_button_pressed(id, button)` | `bool` |
| `get_gamepad_axis_value(id, axis)` | `f32` |
| `snapshot()` | `InputSnapshot` — frozen copy of current state |

### InputSnapshot (`gdplatform::input::InputSnapshot`)

Frozen copy of `InputState` at snapshot time. Mirrors the query API above.
Immutable after creation — further events do not affect it.

### InputMap (`gdplatform::input::InputMap`)

Maps action names to physical input bindings.

**Setup:**
```rust
let mut map = InputMap::new();
map.add_action("jump", 0.0);        // name, deadzone
map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
```

**Loading from files:**
- `InputMap::load_from_project_godot(content)` — parse `[input]` section from `project.godot`
- `InputMap::load_from_json(json)` / `load_from_json_file(path)` — JSON format

**Binding types (`ActionBinding`):**
- `KeyBinding(Key)` — keyboard key
- `MouseButtonBinding(MouseButton)` — mouse button
- `GamepadButtonBinding(GamepadButton)` — gamepad button
- `GamepadAxisBinding { axis, direction, deadzone }` — analog axis

## HeadlessPlatform Test Contract

Tests inject events via `HeadlessPlatform` and query results through
`InputState` or `MainLoop`.

**Injection:**
```rust
let mut backend = HeadlessPlatform::new(640, 480);
backend.push_event(WindowEvent::KeyInput {
    key: Key::Space, pressed: true,
    shift: false, ctrl: false, alt: false,
});
```

**Frame execution:**
```rust
let mut ml = MainLoop::new(tree);
ml.set_input_map(input_map);
ml.run_frame(&mut backend, 1.0 / 60.0);
assert!(ml.input_state().is_key_pressed(Key::Space));
```

**Platform-level events:**
```rust
// Resize updates viewport size after poll
backend.push_event(WindowEvent::Resized { width: 1920, height: 1080 });
ml.run_frame(&mut backend, 1.0 / 60.0);
assert_eq!(backend.window_size(), (1920, 1080));

// Close triggers quit after poll
backend.push_event(WindowEvent::CloseRequested);
ml.run_frame(&mut backend, 1.0 / 60.0);
assert!(backend.should_quit());
```

## Test Coverage

Input contract compliance is verified by:

| Test File | Scope |
|-----------|-------|
| `tests/platform_backend_test.rs` | PlatformBackend + MainLoop input routing |
| `tests/window_lifecycle_test.rs` | Window events, resize, close, lifecycle |
| `tests/input_map_loading_test.rs` | InputMap loading from project.godot and JSON |
| `tests/input_action_coverage_test.rs` | Action binding resolution |
| `crates/gdplatform/src/input.rs` (unit tests) | InputState, InputMap, InputSnapshot internals |

## Supported Keys

The `Key` enum covers: A-Z, Num0-Num9, F1-F12, Space, Enter, Escape, Tab,
Shift, Ctrl, Alt, arrow keys, Backspace, Delete, Insert, Home, End, PageUp,
PageDown.

## Supported Mouse Buttons

`MouseButton`: Left, Right, Middle, WheelUp, WheelDown.

## Supported Gamepad

`GamepadButton`: South (A), East (B), West (X), North (Y), L1-L3, R1-R3,
DPadUp/Down/Left/Right, Start, Select, Guide.

`GamepadAxis`: LeftX, LeftY, RightX, RightY, TriggerLeft, TriggerRight.
