# Engine Input Contract

This document defines the engine-owned input API that all examples, tests,
and future frontends must consume. The contract lives in `gdplatform::input`.

## Core Types

### `InputState`

The runtime-owned input singleton. One instance per engine session.

- Receives raw events via `process_event(InputEvent)`
- Tracks pressed/just-pressed/just-released state for keys, mouse buttons, and gamepad buttons
- Tracks mouse position and gamepad axis values
- Maps physical inputs to named actions via `InputMap`
- Must call `flush_frame()` once per frame to clear just-pressed/just-released edges

### `InputSnapshot`

An immutable, cloneable capture of `InputState` at a point in time.
Created via `InputState::snapshot()`. Used by scripts and tests that need
a frozen view of input without borrowing the mutable singleton.

### `InputMap`

Action-to-binding registry. Loaded from `project.godot` (`load_from_project_godot`)
or JSON (`load_from_json` / `load_from_json_file`). Installed into `InputState`
via `set_input_map()`.

### `InputEvent`

Enum of all input event variants:

- `KeyPressed { key, shift, ctrl, alt }` / `KeyReleased { key }`
- `MouseButtonPressed { button, position }` / `MouseButtonReleased { button, position }`
- `MouseMotion { position, relative }`
- `GamepadButtonPressed { gamepad_id, button }` / `GamepadButtonReleased { gamepad_id, button }`
- `GamepadAxisChanged { gamepad_id, axis, value }`

### `ActionBinding`

Enum mapping an action name to a physical input:

- `Key(Key)`
- `MouseButton(MouseButton)`
- `GamepadButton(GamepadButton)`
- `GamepadAxis { axis, direction: f32 }`

## Query API

All queries are available on both `InputState` and `InputSnapshot`:

| Method | Returns | Description |
|--------|---------|-------------|
| `is_key_pressed(key)` | `bool` | Key is held this frame |
| `is_key_just_pressed(key)` | `bool` | Key was pressed this frame (edge) |
| `is_key_just_released(key)` | `bool` | Key was released this frame (edge) |
| `is_mouse_button_pressed(button)` | `bool` | Mouse button held |
| `is_mouse_button_just_pressed(button)` | `bool` | Mouse button pressed this frame |
| `is_mouse_button_just_released(button)` | `bool` | Mouse button released this frame |
| `get_mouse_position()` | `Vector2` | Current mouse position |
| `is_action_pressed(name)` | `bool` | Named action is held |
| `is_action_just_pressed(name)` | `bool` | Named action pressed this frame |
| `is_action_just_released(name)` | `bool` | Named action released this frame |
| `get_action_strength(name)` | `f32` | 0.0–1.0 strength (1.0 for digital) |
| `get_axis(negative, positive)` | `f32` | -1.0 to 1.0 between two actions |
| `get_vector(neg_x, pos_x, neg_y, pos_y)` | `Vector2` | 2D vector from four actions |

## Frame Lifecycle

1. Backend (winit or test harness) calls `process_event()` for each raw input
2. Engine runs `_process()` / `_physics_process()` — scripts read input via snapshot or direct queries
3. Engine calls `flush_frame()` at end of frame to clear edge state

## Godot Parity Notes

- `is_action_pressed` / `is_action_just_pressed` match Godot's `Input.is_action_pressed()` / `Input.is_action_just_pressed()` semantics
- Action deadzones are per-action, set via `InputMap::add_action(name, deadzone)`
- `InputMap` loading from `project.godot` parses the `[input]` section format
- Key names match GDScript string names (e.g., `"Space"`, `"Enter"`, `"A"`)
