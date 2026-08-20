//! Script-to-node integration for the scene tree.
//!
//! This module provides [`GDScriptNodeInstance`], a [`ScriptInstance`]
//! implementation that wraps a parsed GDScript class and connects its
//! lifecycle methods (`_ready`, `_process`, `_physics_process`, etc.) to
//! scene tree nodes.
//!
//! The [`SceneTree`] stores attached scripts in a side map
//! (`HashMap<NodeId, Box<dyn ScriptInstance>>`) and exposes methods to
//! dispatch lifecycle callbacks to them.

use std::collections::HashMap;

use gdcore::math::{Color, Rect2, Vector2};
use gdscript_interop::bindings::{
    MethodFlags, MethodInfo, SceneAccess, ScriptError, ScriptInstance, ScriptPropertyInfo,
};
use gdscript_interop::interpreter::{ClassInstance, Interpreter, RuntimeError};
use gdserver2d::canvas::DrawCommand;
use gdvariant::variant::VariantType;
use gdvariant::Variant;

use crate::node::NodeId;

// -- `draw_*` argument coercion (pat-0q2kx.82) ------------------------------
//
// GDScript is dynamically typed, so a literal `2` reaches a float parameter as
// `Variant::Int`. These accept either numeric form the way Godot's Variant
// argument conversion does, and return `None` for a genuinely wrong type so the
// caller falls through to "not a native method" instead of drawing garbage.

fn arg_f32(args: &[Variant], index: usize) -> Option<f32> {
    match args.get(index)? {
        Variant::Float(f) => Some(*f as f32),
        Variant::Int(i) => Some(*i as f32),
        _ => None,
    }
}

fn arg_vec2(args: &[Variant], index: usize) -> Option<Vector2> {
    match args.get(index)? {
        Variant::Vector2(v) => Some(*v),
        Variant::Vector2i(v) => Some(Vector2::new(v.x as f32, v.y as f32)),
        _ => None,
    }
}

fn arg_color(args: &[Variant], index: usize) -> Option<Color> {
    match args.get(index)? {
        Variant::Color(c) => Some(*c),
        _ => None,
    }
}

fn arg_rect2(args: &[Variant], index: usize) -> Option<Rect2> {
    match args.get(index)? {
        Variant::Rect2(r) => Some(*r),
        _ => None,
    }
}

/// Reads a point list, accepting either a `PackedVector2Array` or a plain
/// `Array` of `Vector2` — a GDScript literal `[Vector2(0,0), ...]` parses as
/// the latter, which is exactly how these are written by hand (pat-0q2kx.129).
fn arg_vec2_array(args: &[Variant], index: usize) -> Option<Vec<Vector2>> {
    match args.get(index)? {
        Variant::PackedVector2Array(pts) => Some(pts.clone()),
        Variant::Array(items) => Some(
            items
                .iter()
                .filter_map(|v| match v {
                    Variant::Vector2(p) => Some(*p),
                    Variant::Vector2i(p) => Some(Vector2::new(p.x as f32, p.y as f32)),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// Reads a color list (`PackedColorArray` or `Array` of `Color`).
fn arg_color_array(args: &[Variant], index: usize) -> Vec<Color> {
    match args.get(index) {
        Some(Variant::PackedColorArray(cs)) => cs.clone(),
        Some(Variant::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Variant::Color(c) => Some(*c),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// First color of a color-array argument, for the `*_colors` draw variants.
fn arg_first_color(args: &[Variant], index: usize) -> Option<Color> {
    arg_color_array(args, index).first().copied()
}

/// A stroke width in pixels. Godot's `-1` (and an absent argument) means
/// "thinnest visible line", which the rasterizer wants as 1.
fn positive_width(args: &[Variant], index: usize) -> f32 {
    match arg_f32(args, index) {
        Some(w) if w > 0.0 => w,
        _ => 1.0,
    }
}

/// Per-node script storage.
pub type ScriptStore = HashMap<NodeId, Box<dyn ScriptInstance>>;

// ---------------------------------------------------------------------------
// GDScriptNodeInstance
// ---------------------------------------------------------------------------

/// A [`ScriptInstance`] backed by a parsed GDScript class.
///
/// Wraps an [`Interpreter`], [`ClassDef`], and [`ClassInstance`] so that
/// lifecycle methods like `_ready` and `_process` execute the corresponding
/// GDScript methods defined in the source file.
pub struct GDScriptNodeInstance {
    interpreter: Interpreter,
    instance: ClassInstance,
    node_id: NodeId,
}

impl std::fmt::Debug for GDScriptNodeInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GDScriptNodeInstance")
            .field("node_id", &self.node_id)
            .field("class_name", &self.instance.class_def.name)
            .finish()
    }
}

impl GDScriptNodeInstance {
    /// Creates a new script instance by parsing GDScript source.
    ///
    /// The source is expected to be a class-style script (with `extends`,
    /// `var` declarations, and `func` definitions).
    pub fn from_source(source: &str, node_id: NodeId) -> Result<Self, RuntimeError> {
        let mut interpreter = Interpreter::new();
        let class_def = interpreter.run_class(source)?;
        let instance = interpreter.instantiate_class(&class_def)?;
        Ok(Self {
            interpreter,
            instance,
            node_id,
        })
    }

    /// Returns the node ID this script is attached to.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Returns true if the class defines a method with the given name.
    pub fn has_method(&self, name: &str) -> bool {
        self.instance.class_def.methods.contains_key(name)
    }

    /// The `@rpc`-annotated methods, as `(method_name, annotation_args)`.
    ///
    /// The parser has always recorded these; `FuncDef` used to discard them, so
    /// nothing downstream could see them (pat-0q2kx.79). Returned as raw
    /// argument strings because the RPC config type lives in `gdplatform`, which
    /// the interpreter crate does not depend on.
    fn rpc_methods_impl(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = self
            .instance
            .class_def
            .methods
            .iter()
            .filter_map(|(name, def)| {
                def.rpc_annotation()
                    .map(|args| (name.clone(), args.to_vec()))
            })
            .collect();
        // Deterministic order: a HashMap walk would register in a different
        // order each run, which matters the moment two methods disagree.
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Whether the script carries `@tool`, i.e. it is meant to run in the
    /// editor as well as at runtime (pat-0q2kx.76).
    ///
    /// The parser already records the annotation; nothing outside the
    /// interpreter could read it, so the editor had no way to tell a tool
    /// script from an ordinary one.
    pub fn is_tool(&self) -> bool {
        self.instance.class_def.is_tool
    }

    /// Drains anything the script printed since the last call.
    ///
    /// `print` accumulates into the interpreter's output buffer; draining it is
    /// how a host surfaces script output — the editor routes it to the Output
    /// panel, so a `@tool` script's prints are visible where a user expects.
    pub fn take_output(&mut self) -> Vec<String> {
        self.interpreter.take_output()
    }
}

impl ScriptInstance for GDScriptNodeInstance {
    fn rpc_methods(&self) -> Vec<(String, Vec<String>)> {
        self.rpc_methods_impl()
    }

    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError> {
        self.interpreter
            .call_instance_method(&mut self.instance, name, args)
            .map_err(|e| match e.kind {
                gdscript_interop::interpreter::RuntimeErrorKind::UndefinedFunction(n) => {
                    ScriptError::MethodNotFound(n)
                }
                gdscript_interop::interpreter::RuntimeErrorKind::TypeError(msg) => {
                    ScriptError::TypeError(msg)
                }
                other => ScriptError::TypeError(other.to_string()),
            })
    }

    fn get_property(&self, name: &str) -> Option<Variant> {
        self.instance.properties.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, value: Variant) -> bool {
        // A property declared with `set = fn` must be written *through* its
        // setter: Godot's `Object::set` routes into `GDScriptInstance::set`, which
        // runs the setter (gdscript.cpp), so a scene override or an engine-side
        // write that bypassed it would leave whatever the setter maintains —
        // derived state, clamping, a dirty flag — unapplied (pat-0q2kx.17).
        if let Some(setter) = self.instance.class_def.setters.get(name).cloned() {
            // Must go through `call_property_setter`, not `call_instance_method`:
            // the latter leaves `current_function` unset, so the setter body's own
            // `self.prop = value` does not recognise that it is already inside the
            // setter and fires it a second time.
            return self
                .interpreter
                .call_property_setter(&mut self.instance, &setter, value)
                .is_ok();
        }
        if self.instance.properties.contains_key(name) {
            self.instance.properties.insert(name.to_string(), value);
            true
        } else {
            false
        }
    }

    fn list_methods(&self) -> Vec<MethodInfo> {
        self.instance
            .class_def
            .methods
            .iter()
            .map(|(name, func)| MethodInfo {
                name: name.clone(),
                argument_names: func.params.clone(),
                return_type: VariantType::Nil,
                flags: MethodFlags::NORMAL,
            })
            .collect()
    }

    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        self.instance
            .properties
            .iter()
            .map(|(name, val)| ScriptPropertyInfo {
                name: name.clone(),
                property_type: val.variant_type(),
                default_value: val.clone(),
                hint: 0,
                hint_string: String::new(),
            })
            .collect()
    }

    fn get_script_name(&self) -> &str {
        self.instance
            .class_def
            .name
            .as_deref()
            .unwrap_or("GDScript")
    }

    fn has_method(&self, name: &str) -> bool {
        self.instance.class_def.methods.contains_key(name)
    }

    fn resolve_onready(&mut self) -> Result<(), ScriptError> {
        self.interpreter
            .resolve_onready_vars(&mut self.instance)
            .map_err(|e| ScriptError::TypeError(e.to_string()))
    }

    fn set_scene_access(&mut self, access: Box<dyn SceneAccess>, node_id: u64) {
        self.interpreter.set_scene_access(access, node_id);
    }

    fn clear_scene_access(&mut self) {
        self.interpreter.clear_scene_access();
    }

    fn poll_coroutines(&mut self, _delta: f32) {
        // Advance frame-based awaits (e.g. `await RenderingServer.frame_post_draw`)
        // by one frame and resume any that are ready. Timer-based awaits are
        // driven centrally by the SceneTree once per frame (via
        // `notify_await_signal`), so we deliberately do NOT re-process the shared
        // SceneTree timers here — doing so per attached script would advance them
        // N times per frame (pat-nzgdu.6). The instance is threaded in as `self`
        // so resumed `self.x = ...` writes land on this script's properties.
        let _ = self
            .interpreter
            .tick_instance_coroutine_frames(&mut self.instance);
    }

    fn pump_xr_signals(&mut self) {
        // Errors here are a script's own fault (a bad signal callback) and must
        // not abort the frame, matching how `call_script_method` swallows a
        // failed script call.
        if let Err(err) = self
            .interpreter
            .pump_instance_xr_signals(&mut self.instance)
        {
            tracing::debug!("XR signal pump failed: {err}");
        }
    }

    fn list_signals(&self) -> Vec<String> {
        self.instance.class_def.signals.clone()
    }

    fn notify_await_signal(&mut self, name: &str, args: &[Variant]) {
        // Wake `await`s parked on this signal name; the interpreter's coroutine
        // host resumes the matching suspended frames, binding the first emitted
        // argument as the awaited value for `var v = await sig` (pat-nzgdu.6).
        // The instance is threaded in as `self` so resumed property writes and
        // declared-signal lookups resolve against this script.
        let _ = self
            .interpreter
            .notify_instance_await_signal(&mut self.instance, name, args);
    }

    fn notify_await_signal_from(&mut self, source: u64, name: &str, args: &[Variant]) {
        // Same as above, but carrying the emitter so an object-bound `await`
        // (`await $ButtonA.pressed`) ignores another node's same-named signal
        // (pat-0q2kx.9).
        let _ = self.interpreter.notify_instance_await_signal_from(
            &mut self.instance,
            Some(source),
            name,
            args,
        );
    }

    fn notification(&mut self, what: i64) {
        // Deliver the engine notification to the script's `_notification(what)`
        // virtual, matching Godot's GDScriptInstance::notification. Only scripts
        // that override `_notification` receive it (pat-nzgdu.13). The caller
        // (SceneTree::notify_script) has already installed scene access, so a
        // `_notification` body can call get_node/emit_signal like any callback.
        if self
            .instance
            .class_def
            .methods
            .contains_key("_notification")
        {
            let _ = self.interpreter.call_instance_method(
                &mut self.instance,
                "_notification",
                &[Variant::Int(what)],
            );
        }
    }
}

// ---------------------------------------------------------------------------
// InputSnapshot — a frozen copy of input state for one frame
// ---------------------------------------------------------------------------

use std::collections::HashSet;

/// An `Input` mutation a script asked for, queued on the [`crate::SceneTree`] and
/// applied to the engine `InputState` by the frame owner (pat-0q2kx.24).
///
/// Godot's `Input` methods mutate live engine state directly
/// (upstream/godot/core/input/input.cpp). A running script here reaches only the
/// tree, never the `InputState` that `MainLoop` owns, so intent is recorded and
/// drained once per frame instead of being silently dropped.
#[derive(Debug, Clone, PartialEq)]
pub enum InputCommand {
    /// `Input.set_mouse_mode(mode)` — Godot `MOUSE_MODE_*` as an int.
    SetMouseMode(i64),
    /// `Input.warp_mouse(position)`.
    WarpMouse {
        /// Target x in window coordinates.
        x: f32,
        /// Target y in window coordinates.
        y: f32,
    },
    /// `Input.action_press(action, strength)`.
    ActionPress {
        /// Action name.
        action: String,
        /// Press strength, 0..1.
        strength: f32,
    },
    /// `Input.action_release(action)`.
    ActionRelease {
        /// Action name.
        action: String,
    },
    /// `Input.start_joy_vibration(device, weak, strong, duration)`.
    StartJoyVibration {
        /// Joypad device id.
        device: i64,
        /// Weak motor amplitude.
        weak: f32,
        /// Strong motor amplitude.
        strong: f32,
        /// Duration in seconds (0 = until stopped).
        duration: f32,
    },
    /// `Input.stop_joy_vibration(device)`.
    StopJoyVibration {
        /// Joypad device id.
        device: i64,
    },
    // -- cursor shape (pat-0q2kx.90) --------------------------------------
    //
    // The `CursorManager` lives on the frame loop beside `InputState`, so a
    // script's cursor change is queued here and applied in the same drain. The
    // browser game view then reads the resulting shape out of the input-state
    // payload and applies it as a CSS `cursor`.
    /// `Input.set_default_cursor_shape(shape)` — Godot `CURSOR_*` as an int.
    SetDefaultCursorShape(i64),
    /// `Input.set_custom_mouse_cursor(image, shape, hotspot)`.
    SetCustomMouseCursor {
        /// Resource path of the cursor image, or empty to clear the override.
        image_path: String,
        /// Which `CURSOR_*` shape this image replaces.
        shape: i64,
        /// Hotspot x within the image.
        hotspot_x: f32,
        /// Hotspot y within the image.
        hotspot_y: f32,
    },
    // -- InputMap singleton (pat-0q2kx.89) --------------------------------
    //
    // Runtime rebinding. The engine `InputMap` lives beside `InputState` in the
    // frame loop, so a script's mutation is queued here and applied in the same
    // drain — which is what makes the rebind live before the next event is
    // resolved against the map.
    /// `InputMap.add_action(action, deadzone)`.
    InputMapAddAction {
        /// Action name.
        action: String,
        /// Deadzone for analog bindings.
        deadzone: f32,
    },
    /// `InputMap.erase_action(action)`.
    InputMapEraseAction {
        /// Action name.
        action: String,
    },
    /// `InputMap.action_set_deadzone(action, deadzone)`.
    InputMapSetDeadzone {
        /// Action name.
        action: String,
        /// New deadzone.
        deadzone: f32,
    },
    /// `InputMap.action_add_event(action, event)`.
    InputMapAddEvent {
        /// Action name.
        action: String,
        /// The binding to add.
        binding: gdplatform::input::ActionBinding,
    },
    /// `InputMap.action_erase_event(action, event)`.
    InputMapEraseEvent {
        /// Action name.
        action: String,
        /// The binding to remove.
        binding: gdplatform::input::ActionBinding,
    },
    /// `InputMap.action_erase_events(action)`.
    InputMapEraseEvents {
        /// Action name.
        action: String,
    },
    /// `InputMap.load_from_project_settings()` — restore the project-declared
    /// map, discarding every runtime change.
    InputMapLoadFromProjectSettings,
}

/// Converts a script-described input event into the engine's typed binding
/// (pat-0q2kx.89).
///
/// `None` for a class or code the engine does not model, so an unrecognized
/// rebind is dropped rather than silently bound to the wrong control.
pub fn binding_from_event_spec(
    spec: &gdscript_interop::bindings::InputEventSpec,
) -> Option<gdplatform::input::ActionBinding> {
    use gdplatform::input::{ActionBinding, KeyCombo};

    match spec.class.as_str() {
        "InputEventKey" => {
            let key = gdplatform::input::key_from_godot_keycode(spec.code)?;
            if spec.shift || spec.ctrl || spec.alt {
                Some(ActionBinding::KeyWithModifiers(KeyCombo {
                    key,
                    shift: spec.shift,
                    ctrl: spec.ctrl,
                    alt: spec.alt,
                }))
            } else {
                Some(ActionBinding::KeyBinding(key))
            }
        }
        "InputEventMouseButton" => Some(ActionBinding::MouseBinding(
            gdplatform::input::mouse_button_from_godot(spec.code)?,
        )),
        "InputEventJoypadButton" => Some(ActionBinding::GamepadButtonBinding(
            gdplatform::input::gamepad_button_from_godot(spec.code)?,
        )),
        "InputEventJoypadMotion" => Some(ActionBinding::GamepadAxisDirection {
            axis: gdplatform::input::gamepad_axis_from_godot(spec.code)?,
            positive: spec.axis_value >= 0.0,
        }),
        _ => None,
    }
}

/// A snapshot of input state at a single frame, passed to scripts via
/// [`SceneTreeAccessor`] so they can query `Input.is_action_pressed()` etc.
#[derive(Debug, Clone, Default)]
pub struct InputSnapshot {
    /// Keys currently held down (browser key names like "ArrowLeft", "a").
    pub pressed_keys: HashSet<String>,
    /// Keys that were first pressed this frame.
    pub just_pressed_keys: HashSet<String>,
    /// Keys that were released this frame (pat-nzgdu.9).
    pub just_released_keys: HashSet<String>,
    /// Action name → list of key names.
    pub input_map: std::collections::HashMap<String, Vec<String>>,
    /// Current mouse position in screen coordinates.
    pub mouse_position: gdcore::math::Vector2,
    /// Motion of the last frame in pixels per second — Godot
    /// `Input.get_last_mouse_velocity` (pat-0q2kx.92).
    ///
    /// Velocity rather than raw displacement because that is what Godot
    /// reports and what drag-inertia code integrates; the frame owner divides
    /// the accumulated motion by the frame delta, which is where the delta is
    /// known.
    pub mouse_velocity: gdcore::math::Vector2,
    /// Mouse buttons currently held (button index strings: "1"=Left, "2"=Right, "3"=Middle).
    pub mouse_buttons_pressed: HashSet<String>,
    /// Actions currently pressed, as the engine resolved them (pat-0q2kx.88).
    ///
    /// [`input_map`](Self::input_map) only relates actions to **key names**, so
    /// an action bound to a gamepad button or a mouse button could never report
    /// pressed through it. These carry the engine's own action state, which
    /// `InputState::process_event` resolves for every device.
    pub actions_pressed: HashSet<String>,
    /// Actions that became pressed this frame (pat-0q2kx.88).
    pub actions_just_pressed: HashSet<String>,
    /// Actions released this frame (pat-0q2kx.88).
    pub actions_just_released: HashSet<String>,
    /// Gamepad buttons held, keyed by `(device, Godot JoyButton index)`
    /// (pat-0q2kx.88). Godot's numbering, not the browser's — see
    /// `GamepadButton::to_godot_index`.
    pub gamepad_buttons_pressed: HashSet<(i64, i64)>,
    /// Latest value per `(device, Godot JoyAxis index)` (pat-0q2kx.88).
    pub gamepad_axis_values: std::collections::HashMap<(i64, i64), f32>,
    /// Per-action `(strength, raw_strength)` for actions held this frame
    /// (pat-s3rrv.14).
    ///
    /// ⚠️ The pair is NOT redundant. They differ only for ANALOG bindings and
    /// only once a deadzone applies: a stick at 0.6 under a 0.1 deadzone reads
    /// **0.56** as strength and **0.6** as raw. A digital binding reports 1.0
    /// from both — which is exactly why an implementation that aliases them
    /// passes every keyboard-shaped test and is wrong on every gamepad.
    ///
    /// Absent means "not held": both accessors fall back to the digital
    /// default, matching `gdplatform`'s own accessors, which gate on
    /// `actions_pressed` before consulting either map.
    pub action_strengths: std::collections::HashMap<String, (f32, f32)>,
}

impl InputSnapshot {
    /// Returns `true` if `action` is held.
    ///
    /// Consults the engine-resolved action set first so a gamepad- or
    /// mouse-bound action reports correctly, then falls back to the key-name
    /// map for snapshots built by hand from keys alone (pat-0q2kx.88).
    pub fn is_action_pressed(&self, action: &str) -> bool {
        if self.actions_pressed.contains(action) {
            return true;
        }
        if let Some(keys) = self.input_map.get(action) {
            keys.iter().any(|k| self.pressed_keys.contains(k))
        } else {
            false
        }
    }

    /// Returns `true` if `action` became pressed this frame — see
    /// [`is_action_pressed`](Self::is_action_pressed) for how it resolves.
    pub fn is_action_just_pressed(&self, action: &str) -> bool {
        if self.actions_just_pressed.contains(action) {
            return true;
        }
        if let Some(keys) = self.input_map.get(action) {
            keys.iter().any(|k| self.just_pressed_keys.contains(k))
        } else {
            false
        }
    }

    /// Returns `true` if `action` was released this frame (pat-nzgdu.9).
    pub fn is_action_just_released(&self, action: &str) -> bool {
        if self.actions_just_released.contains(action) {
            return true;
        }
        if let Some(keys) = self.input_map.get(action) {
            keys.iter().any(|k| self.just_released_keys.contains(k))
        } else {
            false
        }
    }

    /// Returns `true` if the Godot `JOY_BUTTON_*` `button` is held on joypad
    /// `device` (`Input.is_joy_button_pressed`, pat-0q2kx.88).
    pub fn is_joy_button_pressed(&self, device: i64, button: i64) -> bool {
        self.gamepad_buttons_pressed.contains(&(device, button))
    }

    /// The value of the Godot `JOY_AXIS_*` `axis` on joypad `device`, `0.0`
    /// when untouched (`Input.get_joy_axis`, pat-0q2kx.88).
    pub fn get_joy_axis(&self, device: i64, axis: i64) -> f32 {
        self.gamepad_axis_values
            .get(&(device, axis))
            .copied()
            .unwrap_or(0.0)
    }

    /// Returns `true` if the raw key is held.
    pub fn is_key_pressed(&self, key: &str) -> bool {
        self.pressed_keys.contains(key)
    }

    /// The reserved key name representing a script-injected action press.
    ///
    /// Godot's `Input.action_press` sets action state directly, with no physical
    /// key involved. This snapshot resolves actions through `input_map`, so an
    /// injected press is modelled as a synthetic binding whose name no real device
    /// can produce (pat-0q2kx.24).
    fn injected_action_key(action: &str) -> String {
        format!("__script_action__{action}")
    }

    /// Marks `action` pressed as if a script had called `Input.action_press`, so
    /// `is_action_pressed` and `is_action_just_pressed` report it for the rest of
    /// this frame — Godot makes the state visible immediately.
    pub fn inject_action_press(&mut self, action: &str) {
        let key = Self::injected_action_key(action);
        let keys = self.input_map.entry(action.to_string()).or_default();
        if !keys.iter().any(|k| k == &key) {
            keys.push(key.clone());
        }
        self.just_released_keys.remove(&key);
        self.just_pressed_keys.insert(key.clone());
        self.pressed_keys.insert(key);
    }

    /// The `Input.action_release` counterpart of
    /// [`inject_action_press`](Self::inject_action_press).
    pub fn inject_action_release(&mut self, action: &str) {
        let key = Self::injected_action_key(action);
        // The binding stays so `is_action_just_released` can resolve it.
        let keys = self.input_map.entry(action.to_string()).or_default();
        if !keys.iter().any(|k| k == &key) {
            keys.push(key.clone());
        }
        self.pressed_keys.remove(&key);
        self.just_pressed_keys.remove(&key);
        self.just_released_keys.insert(key);
    }

    /// Returns the current mouse position.
    pub fn get_mouse_position(&self) -> gdcore::math::Vector2 {
        self.mouse_position
    }

    /// Returns `true` if the given mouse button index is pressed.
    ///
    /// Button indices match Godot: 1=Left, 2=Right, 3=Middle.
    pub fn is_mouse_button_pressed(&self, button_index: i64) -> bool {
        self.mouse_buttons_pressed
            .contains(&button_index.to_string())
    }
}

// ---------------------------------------------------------------------------
// SceneTreeAccessor
// ---------------------------------------------------------------------------

use crate::scene_tree::SceneTree;
use crate::tween::{
    EaseType, TransFunc, Tween, TweenId, TweenPauseMode, TweenProcessMode, TweenStep,
};
use gdcore::id::ObjectId;
use gdobject::signal::Connection;

/// Wraps a raw pointer to [`SceneTree`] so that a running script can call
/// back into the tree (e.g. `get_node`, `emit_signal`) during execution.
///
/// # Safety
///
/// The pointer is valid for the duration of a single `call_script_with_access`
/// call. The script is temporarily removed from the tree's script map before
/// the accessor is created, so there is no aliasing of the script itself.
pub(crate) struct SceneTreeAccessor {
    tree: *mut SceneTree,
    input: Option<InputSnapshot>,
}

impl SceneTreeAccessor {
    /// Creates a new accessor. Caller must ensure the pointer is valid.
    pub(crate) unsafe fn new(tree: *mut SceneTree) -> Self {
        Self { tree, input: None }
    }

    /// Creates a new accessor with an input snapshot.
    pub(crate) unsafe fn with_input(tree: *mut SceneTree, input: InputSnapshot) -> Self {
        Self {
            tree,
            input: Some(input),
        }
    }

    fn tree(&self) -> &SceneTree {
        // SAFETY: The raw pointer is valid for the lifetime of the SceneTreeAccessor.
        // Callers of `new`/`with_input` (both `unsafe`) guarantee the pointer is valid
        // and that no mutable alias exists while this shared reference is live.
        unsafe { &*self.tree }
    }

    /// The absolute-path hash used to address `nid` over the network, or `None`
    /// when the node is not in the tree.
    fn rpc_hash_for(&self, nid: NodeId) -> Option<u64> {
        self.tree()
            .node_path(nid)
            .map(|p| crate::scene_tree::rpc_path_hash(&p))
    }

    fn tree_mut(&mut self) -> &mut SceneTree {
        // SAFETY: We hold `&mut self`, so no other accessor can alias this pointer.
        // The raw pointer validity is guaranteed by the caller of `new`/`with_input`.
        unsafe { &mut *self.tree }
    }

    /// The physics server bound to the tree for this script phase, or `None`
    /// when nothing is driving physics (pat-0q2kx.200).
    ///
    /// The address is read back off the tree rather than carried in the
    /// accessor so that every existing `SceneTreeAccessor` construction site
    /// keeps working unchanged — the accessor already holds the tree pointer.
    fn physics(&self) -> Option<&crate::physics_server::PhysicsServer> {
        let addr = self.tree().script_physics_addr();
        if addr == 0 {
            return None;
        }
        // SAFETY: `set_script_physics_server` stored the address of a live
        // `PhysicsServer` and its caller (`MainLoop`) clears it before that
        // server can move or drop, so a non-zero address points at a live
        // server. The server is a sibling of the tree, never inside it, so this
        // borrow cannot alias the tree pointer.
        Some(unsafe { &*(addr as *const crate::physics_server::PhysicsServer) })
    }

    /// The 3D twin of [`physics`](Self::physics) (pat-s3rrv.107).
    fn physics_3d(&self) -> Option<&crate::physics_server_3d::PhysicsServer3D> {
        let addr = self.tree().script_physics_3d_addr();
        if addr == 0 {
            return None;
        }
        // SAFETY: identical contract to `physics()`. `set_script_physics_server_3d`
        // stored the address of a live `PhysicsServer3D` and `MainLoop` clears it
        // before that server can move or drop, at every one of the paired
        // set/clear brackets. A zero address means "not bound" and is handled
        // above, so a non-zero one points at a live server. The server is a
        // sibling of the tree, never inside it, so this borrow cannot alias the
        // tree pointer.
        Some(unsafe { &*(addr as *const crate::physics_server_3d::PhysicsServer3D) })
    }

    /// Maps a shape query's `exclude` list from NODE ids to `BodyId`s
    /// (pat-s3rrv.26).
    ///
    /// Godot's `exclude` takes RIDs; Patina has none, so a script passes the
    /// node objects. A node with no registered body drops out of the list rather
    /// than becoming body 0 and excluding an unrelated body.
    fn shape_query_exclude(
        &self,
        params: &ShapeParams2D,
    ) -> Option<Vec<gdphysics2d::body::BodyId>> {
        let phys = self.physics()?;
        Some(
            params
                .exclude
                .iter()
                .filter_map(|raw| {
                    phys.body_for_node(NodeId::from_object_id(ObjectId::from_raw(*raw)))
                })
                .collect(),
        )
    }

    /// Mutable counterpart of [`Self::physics`].
    fn physics_mut(&mut self) -> Option<&mut crate::physics_server::PhysicsServer> {
        let addr = self.tree().script_physics_addr();
        if addr == 0 {
            return None;
        }
        // SAFETY: As in `physics`, plus: we hold `&mut self`, so no other
        // accessor can hand out a second reference to the same server.
        Some(unsafe { &mut *(addr as *mut crate::physics_server::PhysicsServer) })
    }

    /// The audio server bound to the tree for this script phase, or `None` when
    /// nothing is driving audio (pat-s3rrv.6).
    ///
    /// Read off the tree for the same reason as [`Self::physics`]: the accessor
    /// already holds the tree pointer, so every existing construction site keeps
    /// working unchanged.
    fn audio(&self) -> Option<&gdaudio::AudioServer> {
        let addr = self.tree().script_audio_addr();
        if addr == 0 {
            return None;
        }
        // SAFETY: `set_script_audio_server` stored the address of a live
        // `AudioServer` owned by `MainLoop`'s `AudioServerBridge`, and its
        // caller clears it before that server can move or drop. The server is a
        // sibling of the tree, never inside it, so this borrow cannot alias the
        // tree pointer.
        Some(unsafe { &*(addr as *const gdaudio::AudioServer) })
    }

    /// Mutable counterpart of [`Self::audio`].
    fn audio_mut(&mut self) -> Option<&mut gdaudio::AudioServer> {
        let addr = self.tree().script_audio_addr();
        if addr == 0 {
            return None;
        }
        // SAFETY: As in `audio`, plus: we hold `&mut self`, so no other accessor
        // can hand out a second reference to the same server.
        Some(unsafe { &mut *(addr as *mut gdaudio::AudioServer) })
    }

    // -- Tween step plumbing (pat-0q2kx.4) ----------------------------------

    /// Appends a step to the tween, returning `None` if it no longer exists.
    fn push_tween_step(&mut self, tid: TweenId, mut step: TweenStep) -> Option<()> {
        let tween = self.tree_mut().get_tween_mut(tid)?;
        // `set_parallel(true)` makes every SUBSEQUENT tweener run alongside the
        // previous one, so the default is stamped onto each step as it is added
        // (Godot `Tween.set_parallel`, distinct from the one-shot `parallel()`).
        if tween.default_parallel {
            step.parallel = true;
        }
        tween.steps.push(step);
        // Chaining a tweener onto a tween that already ran dry restarts it, so
        // `create_tween()` followed by tweeners in the same frame animates even
        // though the tween was momentarily empty.
        tween.running = true;
        Some(())
    }

    /// Applies `f` to the most recently added step — the one Godot's per-tweener
    /// setters (`set_ease`/`set_trans`/`set_delay`) target in a fluent chain.
    ///
    /// `None` means the *tween* is gone. Having no steps yet is NOT an error:
    /// `create_tween().set_trans(TRANS_SINE).tween_property(...)` is a common
    /// Godot idiom where the setter records a tween-wide default for subsequent
    /// tweeners. Patina's `Tween` has no default slot, so that setting is
    /// currently dropped — but it must still report success, or the call would
    /// fall through to node dispatch and raise `UndefinedFunction` on a handle
    /// that is not a node. **Known gap:** setters placed *before* the first
    /// tweener are ignored rather than applied as defaults.
    fn with_last_step(&mut self, tid: TweenId, f: impl FnOnce(&mut TweenStep)) -> Option<()> {
        let tween = self.tree_mut().get_tween_mut(tid)?;
        if let Some(step) = tween.steps.last_mut() {
            f(step);
        }
        Some(())
    }
}

// -- Tween argument coercion (pat-0q2kx.4) ----------------------------------

/// The raw object id behind a `Variant`, for `tween_property`'s target.
fn variant_object_raw(v: &Variant) -> Option<u64> {
    match v {
        Variant::ObjectId(id) => Some(id.raw()),
        // A script may hand back a bare instance id from `get_instance_id()`.
        Variant::Int(i) if *i > 0 => Some(*i as u64),
        _ => None,
    }
}

/// String-ish `Variant` as an owned `String` (property paths, method names).
fn variant_string(v: &Variant) -> Option<String> {
    match v {
        Variant::String(s) => Some(s.clone()),
        Variant::StringName(s) => Some(s.as_str().to_string()),
        Variant::NodePath(p) => Some(p.to_string()),
        _ => None,
    }
}

/// Numeric `Variant` as `f64` — Godot accepts int or float for durations.
fn variant_number(v: &Variant) -> Option<f64> {
    match v {
        Variant::Float(f) => Some(*f),
        Variant::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// The method name a `tween_callback` / `tween_method` argument names.
///
/// Godot passes a `Callable`; Patina's tween steps carry a plain method name
/// dispatched against the tween's **owning node**, so a Callable bound to some
/// other object is invoked on the owner instead. That is the pre-existing
/// `TweenStep::method` contract, not something this bridge introduces.
fn variant_callable_method(v: &Variant) -> Option<String> {
    match v {
        Variant::Callable(c) => match c.inner_callable() {
            gdvariant::variant::CallableRef::Method { method, .. } => Some(method.clone()),
            _ => None,
        },
        _ => variant_string(v),
    }
}

/// Godot `Tween.EaseType`: EASE_IN=0, EASE_OUT=1, EASE_IN_OUT=2, EASE_OUT_IN=3.
///
/// All four are now real curves (pat-0q2kx.55); `OUT_IN` used to collapse onto
/// `InOut`, which is a visibly different shape (fast-slow-fast vs
/// slow-fast-slow).
fn ease_type_from_i64(v: i64) -> EaseType {
    match v {
        0 => EaseType::In,
        1 => EaseType::Out,
        3 => EaseType::OutIn,
        _ => EaseType::InOut,
    }
}

/// Godot `Tween.TransitionType`: LINEAR=0, SINE=1, QUINT=2, QUART=3, QUAD=4,
/// EXPO=5, ELASTIC=6, CUBIC=7, CIRC=8, BOUNCE=9, BACK=10, SPRING=11.
///
/// ⚠️ All twelve now map to their own curve (pat-0q2kx.55). This mapping used to
/// substitute approximations — QUINT and QUART both became Cubic, CIRC became
/// Sine, and SPRING became Elastic — so a script asking for `TRANS_SPRING` got a
/// visibly wrong motion with nothing reporting an error. Do not reintroduce a
/// "nearest shape" fallback here: a silently different curve is harder to spot
/// than a missing one.
fn trans_func_from_i64(v: i64) -> TransFunc {
    match v {
        0 => TransFunc::Linear,
        1 => TransFunc::Sine,
        2 => TransFunc::Quint,
        3 => TransFunc::Quart,
        4 => TransFunc::Quad,
        5 => TransFunc::Expo,
        6 => TransFunc::Elastic,
        7 => TransFunc::Cubic,
        8 => TransFunc::Circ,
        9 => TransFunc::Bounce,
        10 => TransFunc::Back,
        11 => TransFunc::Spring,
        _ => TransFunc::Linear,
    }
}

/// Reads a `Vector3` argument, or `None` when the argument is absent or is a
/// different type — which is how the 3D force arms decline a 2D call.
fn vec3_arg(args: &[Variant], index: usize) -> Option<gdcore::math::Vector3> {
    match args.get(index) {
        Some(Variant::Vector3(v)) => Some(*v),
        _ => None,
    }
}

/// Adds `delta` to a node's `Vector3` property, treating an absent value as zero
/// — the accumulate step of the `_pending_*` force bridge (pat-0q2kx.37).
fn accumulate_vec3_prop(
    tree: &mut SceneTree,
    nid: NodeId,
    key: &str,
    delta: gdcore::math::Vector3,
) {
    let current = match tree.get_node(nid).map(|n| n.get_property(key)) {
        Some(Variant::Vector3(v)) => v,
        _ => gdcore::math::Vector3::ZERO,
    };
    if let Some(node) = tree.get_node_mut(nid) {
        node.set_property(key, Variant::Vector3(current + delta));
    }
}

/// The built-in properties Godot guarantees exist on a class, with their
/// documented defaults — reported when the node itself was never given one
/// (pat-0q2kx.33).
///
/// A static table rather than a ClassDB lookup: ClassDB is only populated by an
/// explicit `register_*_classes()` call, so a ClassDB-derived fallback silently
/// vanishes in exactly the contexts that need it most (bare tests, partially
/// registered servers) — the same reasoning that keeps the transform routing in
/// [`SceneAccess::set_node_property`] off ClassDB.
///
/// Deliberately narrow: the `CharacterBody*` runtime state whose absence breaks a
/// verbatim Godot script, and only values that agree with what
/// [`crate::physics_server::PhysicsServer::process_character_movement`] assumes
/// for the same key, so the script and the simulation cannot disagree.
fn builtin_property_default(class_name: &str, prop: &str) -> Option<Variant> {
    match (class_name, prop) {
        // doc/classes/CharacterBody2D.xml: velocity Vector2(0, 0),
        // up_direction Vector2(0, -1), floor_max_angle 0.785398 (45°).
        ("CharacterBody2D", "velocity") => Some(Variant::Vector2(gdcore::math::Vector2::ZERO)),
        ("CharacterBody2D", "up_direction") => {
            Some(Variant::Vector2(gdcore::math::Vector2::new(0.0, -1.0)))
        }
        ("CharacterBody2D", "floor_max_angle") => Some(Variant::Float(std::f64::consts::FRAC_PI_4)),
        // doc/classes/CharacterBody3D.xml: velocity Vector3(0, 0, 0). The other 3D
        // knobs are left out — no 3D character step reads them yet, and inventing
        // a value the simulation ignores would be worse than reporting nothing.
        ("CharacterBody3D", "velocity") => Some(Variant::Vector3(gdcore::math::Vector3::ZERO)),
        // doc/classes/RigidBody3D.xml: all four are Vector3(0, 0, 0). Without
        // these, `linear_velocity.y += lift * delta` on a body the scene never
        // gave a velocity dies exactly as CharacterBody2D's did (pat-0q2kx.37).
        ("RigidBody3D", "linear_velocity")
        | ("RigidBody3D", "angular_velocity")
        | ("RigidBody3D", "constant_force")
        | ("RigidBody3D", "constant_torque") => Some(Variant::Vector3(gdcore::math::Vector3::ZERO)),
        _ => None,
    }
}

// -- AnimationNodeStateMachinePlayback handles (pat-s3rrv.1) ----------------
//
// Godot hands a script an *object* for `parameters/playback`, which it then
// calls methods on: `$AnimationTree.get("parameters/playback").travel("run")`.
// Patina has no engine-side Object for that, so the read mints a tagged
// `ObjectId` naming (AnimationTree node, graph-node name) and
// `animation_tree_playback_call` resolves it back on every call. The tag byte
// keeps a playback id from ever colliding with a node id, and the table is
// deduped by address so re-reading `parameters/playback` returns the same
// handle each frame — as it does in Godot, where the playback is one object.

/// Tag byte marking an `ObjectId` as a state-machine playback handle. Distinct
/// from every other handle family's tag (see the `*_HANDLE_TAG` constants in
/// `gdscript-interop`).
const PLAYBACK_HANDLE_TAG: u64 = 0xA5_00_0000_0000_0000;

thread_local! {
    /// Live playback handles: raw id -> (AnimationTree node id, graph-node name).
    /// The name is empty for the tree root's own machine.
    static PLAYBACK_HANDLES: std::cell::RefCell<HashMap<u64, (u64, String)>> =
        std::cell::RefCell::new(HashMap::new());
    static NEXT_PLAYBACK_HANDLE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// Returns `true` when `raw` was issued by [`playback_handle_for`].
fn is_playback_handle(raw: u64) -> bool {
    raw & 0xFF00_0000_0000_0000 == PLAYBACK_HANDLE_TAG
}

// -- KinematicCollision2D handles (pat-s3rrv.95) ----------------------------
//
// Godot hands `get_slide_collision(i)` and `move_and_collide()` back an OBJECT
// whose getters the script then calls:
//
//     for i in get_slide_collision_count():
//         var c = get_slide_collision(i)
//         if c.get_collider().is_in_group("hazard"): die()
//
// The contacts were already computed every physics tick and discarded;
// `physics_server` now publishes them onto the character node as
// `_slide_collisions`. These handles are what makes one addressable.
//
// ⚠️ The handle stores (character node, index) and RE-READS the record on every
// getter, exactly as the playback handle re-resolves itself. Copying the values
// in at mint time would freeze a contact that the next physics tick has already
// replaced, so a script holding one across frames would read stale geometry.

/// Tag byte marking an `ObjectId` as a slide-collision handle. Distinct from
/// every other family's tag — 0x71 was free in the workspace-wide sweep.
const KINEMATIC_COLLISION_TAG: u64 = 0x71_00_0000_0000_0000;

thread_local! {
    /// Live collision handles: raw id -> (character node raw id, contact index).
    static COLLISION_HANDLES: std::cell::RefCell<HashMap<u64, (u64, usize)>> =
        std::cell::RefCell::new(HashMap::new());
    static NEXT_COLLISION_HANDLE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// Index marking a handle as addressing the single `move_and_collide` result
/// rather than an entry in the slide list (pat-s3rrv.97).
///
/// ⚠️ A sentinel rather than a second handle family: both kinds are a
/// `KinematicCollision2D` with identical getters, so splitting them would mean
/// two tags and two dispatch paths answering the same method names — and tag
/// bytes are a workspace-wide namespace where a collision misroutes silently.
const MOVE_AND_COLLIDE_INDEX: usize = usize::MAX;

/// Returns `true` when `raw` was issued by [`kinematic_collision_handle_for`].
fn is_kinematic_collision_handle(raw: u64) -> bool {
    raw & 0xFF00_0000_0000_0000 == KINEMATIC_COLLISION_TAG
}

/// A handle addressing contact `index` on character `node`, deduped so reading
/// the same contact twice yields the same object — as it does in Godot.
fn kinematic_collision_handle_for(node: u64, index: usize) -> u64 {
    COLLISION_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some((raw, _)) = handles.iter().find(|(_, v)| **v == (node, index)) {
            return *raw;
        }
        let seq = NEXT_COLLISION_HANDLE.with(|n| {
            let v = n.get();
            n.set(v + 1);
            v
        });
        let raw = KINEMATIC_COLLISION_TAG | seq;
        handles.insert(raw, (node, index));
        raw
    })
}

/// The published record behind a handle, re-read from the node every call.
fn collision_record(tree: &SceneTree, raw: u64) -> Option<gdvariant::Dictionary> {
    let (node, index) = COLLISION_HANDLES.with(|h| h.borrow().get(&raw).copied())?;
    let nid = NodeId::from_object_id(ObjectId::from_raw(node));
    if index == MOVE_AND_COLLIDE_INDEX {
        return match tree
            .get_node(nid)
            .map(|n| n.get_property("_move_and_collide"))
        {
            Some(Variant::Dictionary(d)) => Some(d),
            _ => None,
        };
    }
    match tree
        .get_node(nid)
        .map(|n| n.get_property("_slide_collisions"))
    {
        Some(Variant::Array(items)) => match items.get(index) {
            Some(Variant::Dictionary(d)) => Some(d.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// How many contacts the last physics tick published on `nid`.
fn slide_collision_count(tree: &SceneTree, nid: NodeId) -> usize {
    match tree
        .get_node(nid)
        .map(|n| n.get_property("_slide_collisions"))
    {
        Some(Variant::Array(items)) => items.len(),
        _ => 0,
    }
}

/// `KinematicCollision2D`'s getter surface, answered for handle receivers only.
///
/// ⚠️ Must run BEFORE the generic method table. `get_collider`, `get_normal`
/// and `get_position` all already exist there reading RayCast2D's `_hit_*`
/// keys, and a handle reaching those arms would read a raycast's empty results
/// and report nothing — the same shadowing ShapeCast2D had to be lifted above.
/// Declines every non-handle receiver, so the table stays reachable.
/// `Timer.start()` / `stop()` / `is_stopped()` from GDScript (pat-s3rrv.121).
///
/// `SceneTree::process_timer_nodes` drives Timers from the stashed
/// `__timer_running` / `time_left` properties, and **nothing outside that
/// function ever wrote them**, so `$Timer.start()` was a silent no-op: a Timer
/// without `autostart = true` could never run at all.
///
/// Declines every non-Timer node, leaving the dispatch table below reachable.
fn timer_node_call(
    tree: &mut SceneTree,
    nid: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if !matches!(method, "start" | "stop" | "is_stopped") {
        return None;
    }
    if tree.get_node(nid)?.class_name() != "Timer" {
        return None;
    }

    let num = |v: Option<&Variant>| -> Option<f64> {
        match v {
            Some(Variant::Float(f)) => Some(*f),
            Some(Variant::Int(i)) => Some(*i as f64),
            _ => None,
        }
    };
    let read = |tree: &SceneTree, key: &str, default: f64| -> f64 {
        tree.get_node(nid)
            .map(|n| n.get_property(key))
            .and_then(|v| num(Some(&v)))
            .unwrap_or(default)
    };

    match method {
        "start" => {
            // 🔑 `start(time_sec)` changes `wait_time` PERMANENTLY — Godot calls
            // `set_wait_time(p_time)` (`timer.cpp:121-123`), it does not
            // override the duration for one run. A repeating timer restarted
            // with `start(0.25)` keeps firing every 0.25s afterwards.
            if let Some(t) = num(args.first()) {
                if t > 0.0 {
                    tree.get_node_mut(nid)?
                        .set_property("wait_time", Variant::Float(t));
                }
            }
            let wait = read(tree, "wait_time", 1.0).max(1e-4);
            let node = tree.get_node_mut(nid)?;
            node.set_property("time_left", Variant::Float(wait));
            node.set_property("__timer_running", Variant::Bool(true));
            // Mark it started so the first-frame init pass in
            // `process_timer_nodes` cannot re-seed the countdown underneath us.
            node.set_property("__timer_started", Variant::Bool(true));
            Some(Variant::Nil)
        }
        "stop" => {
            let node = tree.get_node_mut(nid)?;
            node.set_property("time_left", Variant::Float(0.0));
            node.set_property("__timer_running", Variant::Bool(false));
            node.set_property("__timer_started", Variant::Bool(true));
            // ⚠️ `stop()` also CLEARS `autostart` (`timer.cpp:131`). Without
            // this a stopped Timer whose scene set `autostart = true` would be
            // restarted by the next init pass — stop would not stick.
            node.set_property("autostart", Variant::Bool(false));
            Some(Variant::Nil)
        }
        // ⚠️ Godot derives this from the clock alone: `get_time_left() <= 0`,
        // where `get_time_left()` clamps negatives to 0 (`timer.cpp:155-161`).
        // It is NOT a separate "running" flag — a *paused* Timer with time
        // remaining reports `false`, i.e. not stopped.
        "is_stopped" => Some(Variant::Bool(read(tree, "time_left", 0.0) <= 0.0)),
        _ => None,
    }
}

fn kinematic_collision_call(
    tree: &SceneTree,
    node: u64,
    method: &str,
    _args: &[Variant],
) -> Option<Variant> {
    if !is_kinematic_collision_handle(node) {
        return None;
    }
    let rec = collision_record(tree, node)?;
    let field = |k: &str| rec.get(k).cloned().unwrap_or(Variant::Nil);
    let value = match method {
        "get_normal" => field("normal"),
        "get_position" => field("position"),
        "get_remainder" => field("remainder"),
        "get_depth" => field("depth"),
        // The collider is published as a node ObjectId, so the value handed
        // back is an ordinary node reference — `is_in_group()` and every other
        // node method then work on it through the normal dispatch below.
        "get_collider" => field("collider"),
        "get_collider_id" => match field("collider") {
            Variant::ObjectId(id) => Variant::Int(id.raw() as i64),
            _ => Variant::Nil,
        },
        // `get_travel` is Godot's motion-applied-before-contact. The slide loop
        // does not record it yet, so it is deliberately NOT answered here: a
        // getter returning a plausible zero would read as "no travel" rather
        // than "not implemented".
        _ => return None,
    };
    Some(value)
}

// -- Physics 2D space-query handles (pat-s3rrv.26) --------------------------
//
// Godot's query surface is a chain of *objects*:
//
//     get_world_2d().direct_space_state.intersect_ray(
//         PhysicsRayQueryParameters2D.create(from, to))
//
// ClassDB knows all of these (registered by this bead's other half), which
// makes a query *describable*; these handles make it *runnable*. The interpreter
// routes any `ObjectId` receiver through `SceneAccess::call_node_method`
// (interpreter.rs:11863), so a tagged id dispatches here without a new trait
// method.
//
// `World2D` and the space state are **fixed singletons**: Patina has one
// `PhysicsWorld2D` per tree (`PhysicsServer::world`), so there is nothing to key
// a registry on. Only the query parameters carry per-call state and need one.
const PHYS_QUERY_TAG: u64 = 0xA6_00_0000_0000_0000;
/// `get_world_2d()`'s return value.
const WORLD_2D_HANDLE: u64 = PHYS_QUERY_TAG | 0x01;
/// `World2D.direct_space_state`'s value.
const SPACE_STATE_2D_HANDLE: u64 = PHYS_QUERY_TAG | 0x02;
/// Builds a `PhysicsShapeQuery3D` from a script's
/// `PhysicsShapeQueryParameters3D` resource (pat-s3rrv.107).
///
/// Shared by every shape-based 3D query — `intersect_shape`, `cast_motion`,
/// `collide_shape`, `get_rest_info` — so the shape decoding, the extent
/// conversion and the mask handling cannot drift apart between them.
///
/// ⚠️ Godot's `BoxShape3D.size` and the capsule/cylinder `height` are FULL
/// extents; `Shape3D` stores HALF-extents and a total height. Passing `size`
/// straight through yields a box twice the authored size, which silently
/// over-reports overlaps.
///
/// Returns `None` when the params carry no usable shape — the callers turn that
/// into Godot's empty answer rather than guessing geometry, because a
/// substituted shape reports contacts that do not exist.
fn shape_query_3d_from_params(
    params: &gdvariant::ResourceRef,
) -> Option<gdphysics3d::query::PhysicsShapeQuery3D> {
    let Some(Variant::Resource(shape_res)) = params.properties.get("shape") else {
        return None;
    };
    let num = |k: &str, d: f32| match shape_res.properties.get(k) {
        Some(Variant::Float(f)) => *f as f32,
        Some(Variant::Int(i)) => *i as f32,
        _ => d,
    };
    let shape = match shape_res.class_name.as_str() {
        "SphereShape3D" => gdphysics3d::shape::Shape3D::Sphere {
            radius: num("radius", 0.5),
        },
        "BoxShape3D" => {
            let size = match shape_res.properties.get("size") {
                Some(Variant::Vector3(v)) => *v,
                _ => gdcore::math::Vector3::new(1.0, 1.0, 1.0),
            };
            gdphysics3d::shape::Shape3D::BoxShape {
                half_extents: gdcore::math::Vector3::new(size.x / 2.0, size.y / 2.0, size.z / 2.0),
            }
        }
        "CapsuleShape3D" => gdphysics3d::shape::Shape3D::CapsuleShape {
            radius: num("radius", 0.5),
            height: num("height", 2.0),
        },
        "CylinderShape3D" => gdphysics3d::shape::Shape3D::CylinderShape {
            radius: num("radius", 0.5),
            height: num("height", 2.0),
        },
        _ => return None,
    };
    // Godot types `transform` as a Transform3D and uses its ORIGIN; a bare
    // Vector3 is accepted too, since the query consumes only a position.
    let position = match params.properties.get("transform") {
        Some(Variant::Transform3D(t)) => t.origin,
        Some(Variant::Vector3(v)) => *v,
        _ => gdcore::math::Vector3::ZERO,
    };
    let mut query = gdphysics3d::query::PhysicsShapeQuery3D::new(shape, position);
    if let Some(Variant::Int(m)) = params.properties.get("collision_mask") {
        query.collision_mask = *m as u32;
    }
    if let Some(Variant::Vector3(v)) = params.properties.get("motion") {
        query.motion = *v;
    }
    Some(query)
}

/// `get_world_3d()`'s return value (pat-s3rrv.107).
const WORLD_3D_HANDLE: u64 = PHYS_QUERY_TAG | 0x11;
/// `World3D.direct_space_state`'s value.
const SPACE_STATE_3D_HANDLE: u64 = PHYS_QUERY_TAG | 0x12;
/// Marks a ray-parameters handle; the low bits are its registry key.
const RAY_PARAMS_BASE: u64 = PHYS_QUERY_TAG | 0x1000;

/// One `PhysicsRayQueryParameters2D`.
#[derive(Debug, Clone)]
struct RayParams2D {
    from: gdcore::math::Vector2,
    to: gdcore::math::Vector2,
    /// Node ids to skip — Godot's `exclude`, which takes RIDs but which scripts
    /// populate from `get_rid()`/node references.
    exclude: Vec<u64>,
    /// Godot defaults this to all layers. ⚠️ A mask of 0 hits **nothing** here,
    /// matching `RayQuery2D` (and Godot) rather than `raycast_filtered`, which
    /// reads 0 as "every layer".
    collision_mask: u32,
}

thread_local! {
    static RAY_PARAMS: std::cell::RefCell<HashMap<u64, RayParams2D>> =
        std::cell::RefCell::new(HashMap::new());
    static NEXT_RAY_PARAMS: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// `true` when `raw` names anything in the physics-query family.
fn is_phys_query_handle(raw: u64) -> bool {
    raw & 0xFF00_0000_0000_0000 == PHYS_QUERY_TAG
}

/// Mints a ray-parameters handle (`PhysicsRayQueryParameters2D.create`).
fn new_ray_params_handle(from: gdcore::math::Vector2, to: gdcore::math::Vector2) -> u64 {
    let seq = NEXT_RAY_PARAMS.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    let raw = RAY_PARAMS_BASE | seq;
    RAY_PARAMS.with(|m| {
        m.borrow_mut().insert(
            raw,
            RayParams2D {
                from,
                to,
                exclude: Vec::new(),
                collision_mask: u32::MAX,
            },
        )
    });
    raw
}

/// Reads back a ray-parameters handle's state.
fn ray_params(raw: u64) -> Option<RayParams2D> {
    RAY_PARAMS.with(|m| m.borrow().get(&raw).cloned())
}

/// Marks a shape-resource handle (`CircleShape2D` / `RectangleShape2D`).
const SHAPE_BASE: u64 = PHYS_QUERY_TAG | 0x2000;
/// Marks a `PhysicsShapeQueryParameters2D` handle.
const SHAPE_PARAMS_BASE: u64 = PHYS_QUERY_TAG | 0x3000;

/// One `PhysicsShapeQueryParameters2D`.
///
/// `shape` is the *handle* rather than the shape itself: Godot's script assigns
/// a shape resource (`params.shape = circle`) and may keep mutating it
/// afterwards, so the query has to read the shape's state at query time rather
/// than snapshot it at assignment.
#[derive(Debug, Clone)]
struct ShapeParams2D {
    shape: u64,
    transform: gdcore::math::Transform2D,
    /// Godot's `motion`, read only by `cast_motion`.
    motion: gdcore::math::Vector2,
    exclude: Vec<u64>,
    collision_mask: u32,
}

thread_local! {
    static SHAPES: std::cell::RefCell<HashMap<u64, gdphysics2d::shape::Shape2D>> =
        std::cell::RefCell::new(HashMap::new());
    static NEXT_SHAPE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static SHAPE_PARAMS: std::cell::RefCell<HashMap<u64, ShapeParams2D>> =
        std::cell::RefCell::new(HashMap::new());
    static NEXT_SHAPE_PARAMS: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// Mints a shape-resource handle (`CircleShape2D.new()` / `RectangleShape2D.new()`).
fn new_shape_handle(shape: gdphysics2d::shape::Shape2D) -> u64 {
    let seq = NEXT_SHAPE.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    let raw = SHAPE_BASE | seq;
    SHAPES.with(|m| m.borrow_mut().insert(raw, shape));
    raw
}

/// Mints a `PhysicsShapeQueryParameters2D` handle with Godot's defaults.
fn new_shape_params_handle() -> u64 {
    let seq = NEXT_SHAPE_PARAMS.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    let raw = SHAPE_PARAMS_BASE | seq;
    SHAPE_PARAMS.with(|m| {
        m.borrow_mut().insert(
            raw,
            ShapeParams2D {
                shape: 0,
                transform: gdcore::math::Transform2D::IDENTITY,
                motion: gdcore::math::Vector2::ZERO,
                exclude: Vec::new(),
                collision_mask: u32::MAX,
            },
        )
    });
    raw
}

fn shape_for(raw: u64) -> Option<gdphysics2d::shape::Shape2D> {
    SHAPES.with(|m| m.borrow().get(&raw).copied())
}

/// Reads one script-visible property off a shape resource.
///
/// ⚠️ Every arm here MUST agree with the matching setter in
/// `set_node_property`, because a script round-trips through both:
/// `r.size = Vector2(20, 10)` followed by `r.size` has to give back what was
/// written. The `size` pair is where that is easy to get wrong — Godot's
/// `RectangleShape2D.size` is the FULL size while the solver stores half
/// extents, so the setter halves and this doubles. Returning the stored field
/// unchanged would read back half of every assignment.
fn shape_property(shape: &gdphysics2d::shape::Shape2D, prop: &str) -> Option<Variant> {
    use gdphysics2d::shape::Shape2D as S;
    let pack = |points: &[gdcore::math::Vector2; gdphysics2d::shape::MAX_POLY_VERTS], count: u8| {
        Variant::PackedVector2Array(points[..count as usize].to_vec())
    };
    Some(match (shape, prop) {
        (S::Circle { radius }, "radius") => Variant::Float(*radius as f64),
        (S::Rectangle { half_extents }, "size") => Variant::Vector2(gdcore::math::Vector2::new(
            half_extents.x * 2.0,
            half_extents.y * 2.0,
        )),
        (S::Segment { a, .. }, "a") => Variant::Vector2(*a),
        (S::Segment { b, .. }, "b") => Variant::Vector2(*b),
        (S::Capsule { radius, .. }, "radius") => Variant::Float(*radius as f64),
        (S::Capsule { height, .. }, "height") => Variant::Float(*height as f64),
        // Derived, never stored — which is why the docs give it no default.
        (S::Capsule { radius, height }, "mid_height") => {
            Variant::Float((height - radius * 2.0) as f64)
        }
        (S::ConvexPolygon { points, count }, "points") => pack(points, *count),
        (S::ConcavePolygon { points, count }, "segments") => pack(points, *count),
        (S::WorldBoundary { normal, .. }, "normal") => Variant::Vector2(*normal),
        (S::WorldBoundary { distance, .. }, "distance") => Variant::Float(*distance as f64),
        (S::SeparationRay { length }, "length") => Variant::Float(*length as f64),
        _ => return None,
    })
}

fn shape_mut<F: FnOnce(&mut gdphysics2d::shape::Shape2D)>(raw: u64, f: F) -> bool {
    SHAPES.with(|m| match m.borrow_mut().get_mut(&raw) {
        Some(s) => {
            f(s);
            true
        }
        None => false,
    })
}

/// The per-tick contact list `PhysicsServer::publish_body_contacts` leaves on a
/// monitoring body (pat-s3rrv.100).
///
/// ⚠️ EMPTY unless the body opted in with `contact_monitor = true` AND
/// `max_contacts_reported > 0` — the publisher applies Godot's own gate, so a
/// body visibly resting on the floor still reports nothing until it asks to
/// monitor. That is parity, not a missing read.
fn body_contacts(tree: &SceneTree, nid: NodeId) -> Vec<Variant> {
    match tree.get_node(nid).map(|n| n.get_property("_body_contacts")) {
        Some(Variant::Array(items)) => items,
        _ => Vec::new(),
    }
}

fn shape_params(raw: u64) -> Option<ShapeParams2D> {
    SHAPE_PARAMS.with(|m| m.borrow().get(&raw).cloned())
}

fn shape_params_mut<F: FnOnce(&mut ShapeParams2D)>(raw: u64, f: F) -> bool {
    SHAPE_PARAMS.with(|m| match m.borrow_mut().get_mut(&raw) {
        Some(p) => {
            f(p);
            true
        }
        None => false,
    })
}

/// Mutates a ray-parameters handle in place (the `exclude` / `collision_mask`
/// setters a script assigns before running the query).
fn ray_params_mut<F: FnOnce(&mut RayParams2D)>(raw: u64, f: F) -> bool {
    RAY_PARAMS.with(|m| match m.borrow_mut().get_mut(&raw) {
        Some(p) => {
            f(p);
            true
        }
        None => false,
    })
}

/// The handle naming `(node, sm_name)`, minting one on first use.
fn playback_handle_for(node: u64, sm_name: &str) -> u64 {
    PLAYBACK_HANDLES.with(|m| {
        let mut map = m.borrow_mut();
        if let Some((raw, _)) = map
            .iter()
            .find(|(_, (n, name))| *n == node && name == sm_name)
        {
            return *raw;
        }
        let seq = NEXT_PLAYBACK_HANDLE.with(|n| {
            let v = n.get();
            n.set(v + 1);
            v
        });
        let raw = PLAYBACK_HANDLE_TAG | seq;
        map.insert(raw, (node, sm_name.to_string()));
        raw
    })
}

/// The `(node, sm_name)` a handle names, or `None` when it names no playback.
fn playback_handle_target(raw: u64) -> Option<(u64, String)> {
    if !is_playback_handle(raw) {
        return None;
    }
    PLAYBACK_HANDLES.with(|m| m.borrow().get(&raw).cloned())
}

/// Splits a Godot playback parameter path into the graph-node name it
/// addresses: `parameters/playback` -> `Some("")` (the graph root) and
/// `parameters/Foo/playback` -> `Some("Foo")` (a nested machine). `None` when
/// `path` is not a playback path at all.
fn playback_param_name(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("parameters/")?;
    match rest {
        "playback" => Some(""),
        _ => rest.strip_suffix("/playback"),
    }
}

/// The HTTPRequest settings as authored on the NODE (pat-s3rrv.78).
///
/// ⚠️ Read from the node properties, not from a copy on `HttpRequestNode`.
/// ClassDB registers `timeout` and `max_redirects` on HTTPRequest with Godot's
/// defaults, so a `.tscn` writing `timeout = 5.0` and a script writing
/// `r.timeout = 5.0` both land here — and Godot treats `r.timeout = 5.0` and
/// `r.set_timeout(5.0)` as the same operation. A second copy on the node struct
/// would make those two disagree, with whichever one the transport happened to
/// read winning silently.
fn http_settings(tree: &SceneTree, node_id: NodeId) -> crate::http_request_node::RequestSettings {
    let node = tree.get_node(node_id);
    crate::http_request_node::RequestSettings {
        timeout: match node.map(|n| n.get_property("timeout")) {
            Some(Variant::Float(f)) => f,
            Some(Variant::Int(i)) => i as f64,
            _ => crate::http_request_node::DEFAULT_TIMEOUT,
        },
        max_redirects: match node.map(|n| n.get_property("max_redirects")) {
            Some(Variant::Int(i)) => i,
            _ => crate::http_request_node::DEFAULT_MAX_REDIRECTS,
        },
        // ⚠️ Absent means `true`, the Godot default — not `false`.
        accept_gzip: match node.map(|n| n.get_property("accept_gzip")) {
            Some(Variant::Bool(b)) => b,
            _ => crate::http_request_node::DEFAULT_ACCEPT_GZIP,
        },
    }
}

/// Whether `node_id` is an `HTTPRequest` (pat-s3rrv.78).
/// The HTTPRequest DELIVERY settings as authored on the node (pat-s3rrv.78).
///
/// Delegates rather than re-reading the properties, so the getters here and the
/// emit path in `SceneTree::process_http_requests` cannot drift apart.
fn http_delivery(tree: &SceneTree, node_id: NodeId) -> crate::http_request_node::DeliverySettings {
    tree.get_node(node_id)
        .map(crate::http_request_node::delivery_settings)
        .unwrap_or_default()
}

/// The HTTPRequest PROXY settings as authored on the node (pat-s3rrv.78).
///
/// Delegates like `http_delivery` does, so the setter arms below and the
/// request arms cannot drift apart on the "unset" rule.
fn http_proxies(tree: &SceneTree, node_id: NodeId) -> crate::http_request_node::ProxySettings {
    tree.get_node(node_id)
        .map(crate::http_request_node::proxy_settings)
        .unwrap_or_default()
}

/// Godot's `custom_headers` argument: a `PackedStringArray` of `"Name: Value"`.
///
/// ⚠️ An untyped `Array` of strings is accepted too — GDScript array literals
/// produce one, so `r.request(url, ["X: 1"])` is what a project actually writes
/// and a PackedStringArray-only reader would silently drop every header.
fn custom_headers_arg(args: &[Variant]) -> Vec<String> {
    match args.get(1) {
        Some(Variant::PackedStringArray(h)) => h.clone(),
        Some(Variant::Array(a)) => a
            .iter()
            .filter_map(|v| match v {
                Variant::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether `node_id` is an `HTTPRequest` (pat-s3rrv.78).
fn is_http_request(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.class_name() == "HTTPRequest")
        .unwrap_or(false)
}

/// Whether `node_id` is a `RichTextLabel` (pat-s3rrv.74).
fn is_rich_text_label(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.class_name() == "RichTextLabel")
        .unwrap_or(false)
}

/// The label's current BBCode source, matching what the draw arm reads.
///
/// `bbcode_text` wins when set, else plain `text` — so a script appending to a
/// label authored with plain `text` extends that text rather than silently
/// starting a second, empty string beside it.
fn richtext_src(tree: &SceneTree, node_id: NodeId) -> String {
    let string_prop = |key: &str| match tree.get_node(node_id).map(|n| n.get_property(key)) {
        Some(Variant::String(s)) => Some(s),
        _ => None,
    };
    // Same precedence, expressed the same way, as `control_render::richtext_source`.
    string_prop("bbcode_text")
        .filter(|s| !s.is_empty())
        .or_else(|| string_prop("text"))
        .unwrap_or_default()
}

/// Writes the label's source back.
///
/// Both properties are set: `bbcode_text` is what the draw arm prefers, and
/// leaving a stale `text` behind would resurrect the old string the moment
/// something cleared the bbcode one.
/// The node property holding a `RichTextLabel`'s open tag stack.
///
/// Godot's push/pop API is a STACK of open tags — `push_color(); push_bold();
/// pop()` closes the bold, not the colour — so `pop` has to know what the last
/// `push` opened. The label itself carries that state, which keeps it alive
/// across frames and dies with the node.
const RICHTEXT_TAG_STACK: &str = "__richtext_tag_stack__";

/// Reads the open-tag stack (outermost first).
///
/// Stored newline-joined because a BBCode close tag never contains a newline,
/// so the round trip is lossless without a second escaping scheme.
fn richtext_tag_stack(tree: &SceneTree, node_id: NodeId) -> Vec<String> {
    match tree
        .get_node(node_id)
        .map(|n| n.get_property(RICHTEXT_TAG_STACK))
    {
        Some(Variant::String(s)) if !s.is_empty() => s.split('\n').map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

fn set_richtext_tag_stack(tree: &mut SceneTree, node_id: NodeId, stack: &[String]) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(RICHTEXT_TAG_STACK, Variant::String(stack.join("\n")));
    }
}

/// Godot's `push_*`: append `open` to the BBCode source and remember `close`.
///
/// ⚠️ ONLY emit tags `parse_bbcode` actually understands. A tag it does not
/// recognise is stripped, so an unsupported push would appear to work — the
/// call succeeds, the text still renders — while silently doing nothing. That
/// is why there is no `push_bold`/`push_italics` here: `[b]`/`[i]` are not
/// parsed, and the built-in 5x7 bitmap font has no weight or slant variants to
/// render them with even if they were.
fn richtext_push(tree: &mut SceneTree, node_id: NodeId, open: &str, close: &str) {
    let joined = format!("{}{}", richtext_src(tree, node_id), open);
    set_richtext(tree, node_id, joined);
    let mut stack = richtext_tag_stack(tree, node_id);
    stack.push(close.to_string());
    set_richtext_tag_stack(tree, node_id, &stack);
}

/// Godot's `pop`: close the most recently pushed tag. `false` when nothing is open.
fn richtext_pop(tree: &mut SceneTree, node_id: NodeId) -> bool {
    let mut stack = richtext_tag_stack(tree, node_id);
    let Some(close) = stack.pop() else {
        return false;
    };
    let joined = format!("{}{}", richtext_src(tree, node_id), close);
    set_richtext(tree, node_id, joined);
    set_richtext_tag_stack(tree, node_id, &stack);
    true
}

/// A `Color` as the `#rrggbbaa` BBCode `parse_bbcode_color` reads back.
fn richtext_color_hex(c: Color) -> String {
    let byte = |f: f32| (f.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(c.r),
        byte(c.g),
        byte(c.b),
        byte(c.a)
    )
}

fn set_richtext(tree: &mut SceneTree, node_id: NodeId, value: String) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("bbcode_text", Variant::String(value.clone()));
        node.set_property("text", Variant::String(value));
    }
}

impl SceneAccess for SceneTreeAccessor {
    fn get_node(&self, from: u64, path: &str) -> Option<u64> {
        let from_id = NodeId::from_object_id(ObjectId::from_raw(from));
        let tree = self.tree();
        // Use the tree's built-in relative path lookup first
        if let Some(found) = tree.get_node_or_null(from_id, path) {
            return Some(found.raw());
        }
        // Fallback: search children by name
        if let Some(from_node) = tree.get_node(from_id) {
            for &child_id in from_node.children() {
                if let Some(child_node) = tree.get_node(child_id) {
                    if child_node.name() == path {
                        return Some(child_id.raw());
                    }
                }
            }
        }
        // Search siblings
        if let Some(from_node) = tree.get_node(from_id) {
            if let Some(parent_id) = from_node.parent() {
                if let Some(parent_node) = tree.get_node(parent_id) {
                    for &sib_id in parent_node.children() {
                        if let Some(sib_node) = tree.get_node(sib_id) {
                            if sib_node.name() == path {
                                return Some(sib_id.raw());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn get_parent(&self, node: u64) -> Option<u64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_node(nid)
            .and_then(|n| n.parent())
            .map(|pid| pid.raw())
    }

    fn get_children(&self, node: u64) -> Vec<u64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_node(nid)
            .map(|n| n.children().iter().map(|id| id.raw()).collect())
            .unwrap_or_default()
    }

    fn get_node_property(&self, node: u64, prop: &str) -> Variant {
        // `World2D.direct_space_state` (pat-s3rrv.26). Answered before the node
        // lookup below because the receiver is a physics-query handle, not a
        // node — `NodeId::from_object_id` on it would name nothing and the
        // property would read back `Nil`, which is what made the documented
        // `get_world_2d().direct_space_state.intersect_ray(..)` chain
        // unreachable even once `World2D` had the property registered.
        if is_phys_query_handle(node) {
            if node == WORLD_3D_HANDLE && prop == "direct_space_state" {
                return Variant::ObjectId(ObjectId::from_raw(SPACE_STATE_3D_HANDLE));
            }
            if node == WORLD_2D_HANDLE && prop == "direct_space_state" {
                return Variant::ObjectId(ObjectId::from_raw(SPACE_STATE_2D_HANDLE));
            }
            // Shape resources answer their own properties (pat-s3rrv.98).
            //
            // ⚠️ Until this existed every one of these read back `Nil`, so a
            // shape was write-only from script: `c.radius = 20` stuck, but
            // `c.radius` gave nothing back. The damage is not just the read —
            // it is that `c.radius += 5` desugars to a read, and the
            // interpreter's "is this a property?" gate treats a `Nil` read as
            // "no such property", so the compound assignment died mid-frame
            // exactly the way an unset `CharacterBody2D.velocity` used to.
            if let Some(shape) = shape_for(node) {
                if let Some(v) = shape_property(&shape, prop) {
                    return v;
                }
                return Variant::Nil;
            }
            // `PhysicsShapeQueryParameters2D` reads back what was assigned, so
            // a script can hand the same params object around and inspect it.
            if let Some(params) = shape_params(node) {
                return match prop {
                    "shape" => {
                        if params.shape == 0 {
                            Variant::Nil
                        } else {
                            Variant::ObjectId(ObjectId::from_raw(params.shape))
                        }
                    }
                    "transform" => Variant::Transform2D(params.transform),
                    "motion" => Variant::Vector2(params.motion),
                    "collision_mask" => Variant::Int(params.collision_mask as i64),
                    "exclude" => Variant::Array(
                        params
                            .exclude
                            .iter()
                            .map(|raw| Variant::ObjectId(ObjectId::from_raw(*raw)))
                            .collect(),
                    ),
                    _ => Variant::Nil,
                };
            }
            return Variant::Nil;
        }
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        // An AnimationTree's `parameters/...` values live on the attached graph,
        // which evaluation reads and writes — so report those rather than the
        // node's stored copy, mirroring Godot's `AnimationTree.get()`
        // (pat-0q2kx.2).
        if prop.starts_with("parameters/") {
            if let Some(anim_tree) = self.tree().get_animation_tree(nid) {
                // `parameters/playback` is an object, not a stored value: it is
                // how every Godot state-machine game steers the graph
                // (`get("parameters/playback").travel("run")`). Answer with a
                // handle the interpreter routes back through
                // `animation_tree_playback_call` (pat-s3rrv.1). Checked before
                // the params map so a scene that happens to have written that
                // key cannot shadow the object.
                if let Some(sm_name) = playback_param_name(prop) {
                    if anim_tree.state_machine(sm_name).is_some() {
                        return Variant::ObjectId(ObjectId::from_raw(playback_handle_for(
                            node, sm_name,
                        )));
                    }
                }
                if let Some(value) = anim_tree.get_parameter(prop) {
                    return value;
                }
            }
        }
        // Try script property first
        if let Some(script) = self.tree().get_script(nid) {
            if let Some(val) = script.get_property(prop) {
                return val;
            }
        }
        // Then try node property
        if let Some(n) = self.tree().get_node(nid) {
            let value = n.get_property(prop);
            if !value.is_nil() {
                return value;
            }
            // A Godot class property exists with its default even when nothing
            // ever assigned it; `Node` stores only what was set, so an unset
            // built-in read back `Nil`. That killed the most ordinary script in
            // Godot: `velocity.y += gravity * delta` on a fresh CharacterBody2D
            // found `Nil`, so the interpreter's "is this a node property?" gate
            // said no and the read-modify-write died with "cannot assign member
            // 'y' on Nil" mid-frame — the body never fell (pat-0q2kx.33).
            return builtin_property_default(n.class_name(), prop).unwrap_or(Variant::Nil);
        }
        Variant::Nil
    }

    fn set_node_property(&mut self, node: u64, prop: &str, value: Variant) {
        // pat-s3rrv.26 — assignments onto a `PhysicsRayQueryParameters2D`
        // handle: `params.exclude = [body]`, `params.collision_mask = 2`.
        //
        // ⚠️ Checked before anything resolves `node` as a NodeId. These handles
        // are tagged ids, not nodes, so the lookup below would miss (or land on
        // an unrelated node) and the assignment would vanish silently — the
        // query would then run with the DEFAULT filter and look like the filter
        // was ignored, rather than like the setter was never wired.
        if is_phys_query_handle(node) {
            match prop {
                // Godot's `exclude` takes RIDs. Patina has no RID, so a script
                // passes the node objects themselves. Non-object entries are
                // dropped rather than silently excluding body 0.
                "exclude" => {
                    let ids: Vec<u64> = match &value {
                        Variant::Array(items) => items
                            .iter()
                            .filter_map(|v| match v {
                                Variant::ObjectId(id) => Some(id.raw()),
                                _ => None,
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    // The same list serves both parameter kinds; only the one
                    // whose registry holds this handle actually takes it.
                    ray_params_mut(node, |p| p.exclude = ids.clone());
                    shape_params_mut(node, |p| p.exclude = ids);
                }
                "collision_mask" => {
                    if let Variant::Int(m) = &value {
                        let mask = *m as u32;
                        ray_params_mut(node, |p| p.collision_mask = mask);
                        shape_params_mut(node, |p| p.collision_mask = mask);
                    }
                }
                // -- shape resources: `circle.radius = 16`, `rect.size = v` ---
                //
                // Godot's `RectangleShape2D.size` is the FULL extent while
                // `Shape2D::Rectangle` stores half-extents, so this halves it.
                // Getting that backwards doubles every rectangle query and the
                // overlap still "works", just against the wrong box.
                // ⚠️ `radius` must PRESERVE the shape kind, not rebuild a
                // circle. `CapsuleShape2D` also has a radius, so the old
                // unconditional `*s = Circle { .. }` silently turned a capsule
                // into a circle the moment a script set its radius — a shape
                // that still collides, just as the wrong one (pat-s3rrv.98,
                // introduced the moment capsules became constructible).
                "radius" => {
                    if let Some(r) = variant_number(&value) {
                        let r = r as f32;
                        // Godot rejects a negative radius outright rather than
                        // storing it (`ERR_FAIL_COND_MSG`), leaving the shape
                        // untouched.
                        if r < 0.0 {
                            return;
                        }
                        shape_mut(node, |s| match *s {
                            gdphysics2d::shape::Shape2D::Capsule { height, .. } => {
                                // ⚠️ A capsule must satisfy `height >= 2 * radius`
                                // or it is no longer a capsule. Godot repairs the
                                // pair rather than refusing, and the two setters
                                // repair it in OPPOSITE directions: setting the
                                // radius GROWS the height, while setting the
                                // height SHRINKS the radius. Clamping both the
                                // same way would silently disagree with Godot on
                                // whichever half was written second.
                                *s = gdphysics2d::shape::Shape2D::Capsule {
                                    radius: r,
                                    height: height.max(r * 2.0),
                                }
                            }
                            _ => *s = gdphysics2d::shape::Shape2D::Circle { radius: r },
                        });
                    }
                }
                // `CapsuleShape2D.height` is the TOTAL height in Godot; the
                // solver stores the same, so no mid-height conversion here.
                "height" => {
                    if let Some(h) = variant_number(&value) {
                        let h = h as f32;
                        if h < 0.0 {
                            return;
                        }
                        shape_mut(node, |s| {
                            if let gdphysics2d::shape::Shape2D::Capsule { radius, .. } = *s {
                                // The opposite repair from `radius` above: a
                                // height too small for the current radius shrinks
                                // the radius to `height / 2`.
                                *s = gdphysics2d::shape::Shape2D::Capsule {
                                    radius: radius.min(h * 0.5),
                                    height: h,
                                }
                            }
                        });
                    }
                }
                // `mid_height` is the straight middle section only, excluding
                // the two semicircular caps — a wrapper over `height`, which is
                // why the docs give it no default. It needs no invariant repair:
                // `height = mid_height + 2 * radius` already satisfies
                // `height >= 2 * radius` for any non-negative mid-height.
                "mid_height" => {
                    if let Some(m) = variant_number(&value) {
                        let m = m as f32;
                        if m < 0.0 {
                            return;
                        }
                        shape_mut(node, |s| {
                            if let gdphysics2d::shape::Shape2D::Capsule { radius, .. } = *s {
                                *s = gdphysics2d::shape::Shape2D::Capsule {
                                    radius,
                                    height: m + radius * 2.0,
                                }
                            }
                        });
                    }
                }
                // `SegmentShape2D.a` / `.b`, each preserving the other end.
                "a" | "b" => {
                    if let Variant::Vector2(v) = &value {
                        let (v, is_a) = (*v, prop == "a");
                        shape_mut(node, |s| {
                            if let gdphysics2d::shape::Shape2D::Segment { a, b } = *s {
                                *s = if is_a {
                                    gdphysics2d::shape::Shape2D::Segment { a: v, b }
                                } else {
                                    gdphysics2d::shape::Shape2D::Segment { a, b: v }
                                }
                            }
                        });
                    }
                }
                // ⚠️ `points` builds a CONVEX hull and `segments` a segment
                // soup — Godot's two polygon shapes take different array
                // meanings under different property names, so they are not one
                // arm with a shared body.
                "points" => {
                    if let Variant::PackedVector2Array(pts) = &value {
                        let pts = pts.clone();
                        shape_mut(node, |s| {
                            *s = gdphysics2d::shape::Shape2D::convex_polygon(&pts)
                        });
                    }
                }
                "segments" => {
                    if let Variant::PackedVector2Array(pts) = &value {
                        let pts = pts.clone();
                        shape_mut(node, |s| {
                            *s = gdphysics2d::shape::Shape2D::concave_polygon(&pts)
                        });
                    }
                }
                "normal" => {
                    if let Variant::Vector2(v) = &value {
                        let v = *v;
                        shape_mut(node, |s| {
                            if let gdphysics2d::shape::Shape2D::WorldBoundary { distance, .. } = *s
                            {
                                *s = gdphysics2d::shape::Shape2D::world_boundary(v, distance)
                            }
                        });
                    }
                }
                "distance" => {
                    if let Some(d) = variant_number(&value) {
                        let d = d as f32;
                        shape_mut(node, |s| {
                            if let gdphysics2d::shape::Shape2D::WorldBoundary { normal, .. } = *s {
                                *s = gdphysics2d::shape::Shape2D::world_boundary(normal, d)
                            }
                        });
                    }
                }
                "length" => {
                    if let Some(l) = variant_number(&value) {
                        let l = l as f32;
                        shape_mut(node, |s| {
                            if matches!(*s, gdphysics2d::shape::Shape2D::SeparationRay { .. }) {
                                *s = gdphysics2d::shape::Shape2D::SeparationRay { length: l }
                            }
                        });
                    }
                }
                "size" => {
                    if let Variant::Vector2(v) = &value {
                        let half = gdcore::math::Vector2::new(v.x * 0.5, v.y * 0.5);
                        shape_mut(node, |s| {
                            *s = gdphysics2d::shape::Shape2D::Rectangle { half_extents: half }
                        });
                    }
                }
                // -- PhysicsShapeQueryParameters2D --------------------------
                "shape" => {
                    if let Variant::ObjectId(id) = &value {
                        let raw = id.raw();
                        shape_params_mut(node, |p| p.shape = raw);
                    }
                }
                "transform" => {
                    if let Variant::Transform2D(t) = &value {
                        let t = *t;
                        shape_params_mut(node, |p| p.transform = t);
                    }
                }
                "motion" => {
                    if let Variant::Vector2(v) = &value {
                        let v = *v;
                        shape_params_mut(node, |p| p.motion = v);
                    }
                }
                _ => {}
            }
            return;
        }
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        // Route an AnimationTree's own properties into the attached graph so a
        // script driving it (`tree.set("parameters/conditions/run", true)`,
        // `tree.active = false`) affects the next frame's evaluation, as Godot's
        // `AnimationTree.set()` does. The value is mirrored onto the node so the
        // inspector and a scene save still see it (pat-0q2kx.2).
        if prop.starts_with("parameters/") || prop == "active" {
            let routed = match self.tree_mut().get_animation_tree_mut(nid) {
                Some(anim_tree) => {
                    if prop == "active" {
                        anim_tree.active = match &value {
                            Variant::Bool(b) => *b,
                            Variant::Int(i) => *i != 0,
                            _ => anim_tree.active,
                        };
                    } else {
                        anim_tree.set_parameter(prop, value.clone());
                    }
                    true
                }
                None => false,
            };
            if routed {
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property(prop, value);
                }
                return;
            }
        }
        // Route a Node3D transform component through the canonical
        // local-transform setters so a script write is visible to rendering and
        // physics (pat-0q2kx.29). A plain `set_property` here writes only the
        // component key, and `get_local_transform` prefers an explicit
        // `transform` — so on any node loaded from a `.tscn` with a
        // `transform =` line, `self.position = ...` silently changed nothing.
        //
        // Checked before the script-property path because these are engine
        // properties of the node, not script variables.
        // The `Vector3` payload is itself the 2D/3D discriminator: a Node2D
        // writes `position` as a `Vector2`, so it falls through untouched. That
        // avoids consulting ClassDB here, which is not guaranteed to be
        // populated in every runtime/test context and would silently disable
        // this routing when it isn't.
        match (prop, &value) {
            ("position", Variant::Vector3(v)) => {
                crate::node3d::set_position(self.tree_mut(), nid, *v);
                return;
            }
            ("rotation", Variant::Vector3(v)) => {
                crate::node3d::set_rotation(self.tree_mut(), nid, *v);
                return;
            }
            ("scale", Variant::Vector3(v)) => {
                crate::node3d::set_scale(self.tree_mut(), nid, *v);
                return;
            }
            _ => {}
        }
        // Try script property first
        if let Some(script) = self.tree_mut().get_script_mut(nid) {
            if script.set_property(prop, value.clone()) {
                return;
            }
        }
        // Then try node property
        if let Some(n) = self.tree_mut().get_node_mut(nid) {
            n.set_property(prop, value);
        }
    }

    // -- Object virtual chain (pat-0q2kx.82) --------------------------------

    fn object_to_string(&mut self, node: u64) -> Option<String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        match self.tree_mut().call_script_method(nid, "_to_string", &[])? {
            Variant::String(s) => Some(s),
            Variant::StringName(s) => Some(s.as_str().to_string()),
            // Godot logs "Wrong type for _to_string, must be a String" and uses
            // the default representation; silently falling back matches the
            // observable behaviour.
            _ => None,
        }
    }

    fn object_get_virtual(&mut self, node: u64, property: &str) -> Option<Variant> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        match self.tree_mut().call_script_method(
            nid,
            "_get",
            &[Variant::String(property.to_string())],
        )? {
            // Godot treats a `null` return as "this property is not mine",
            // which is why a script must `return` nothing for names it does not
            // handle rather than falling off the end with a value.
            Variant::Nil => None,
            value => Some(value),
        }
    }

    fn object_set_virtual(&mut self, node: u64, property: &str, value: &Variant) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let args = [Variant::String(property.to_string()), value.clone()];
        // `_set` reports whether it consumed the write; only an explicit `true`
        // stops the engine from storing the value itself.
        matches!(
            self.tree_mut().call_script_method(nid, "_set", &args),
            Some(Variant::Bool(true))
        )
    }

    fn object_property_list_virtual(&mut self, node: u64) -> Vec<Variant> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        match self
            .tree_mut()
            .call_script_method(nid, "_get_property_list", &[])
        {
            Some(Variant::Array(entries)) => entries,
            _ => Vec::new(),
        }
    }

    // -- Cross-object script calls (pat-0q2kx.8) ----------------------------

    fn call_script_method(&mut self, node: u64, method: &str, args: &[Variant]) -> Option<Variant> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().call_script_method(nid, method, args)
    }

    fn node_has_method(&self, node: u64, method: &str) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().node_has_method(nid, method)
    }

    fn node_method_argument_count(&self, node: u64, method: &str) -> Option<i64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().node_method_argument_count(nid, method)
    }

    // -- Native node method dispatch (pat-0q2kx.16) -------------------------
    //
    // The second half of Godot's `Object::callp`: after the script instance
    // declines, the engine's own method for the class runs. Before this the
    // interpreter's ObjectId match was a hand-coded list whose fallback threw
    // `UndefinedFunction`, so `body.move_and_slide()` and `other.add_to_group()`
    // errored out of a running script.
    //
    // `None` = "not a native method", so the caller keeps its own error path.

    fn call_node_method(&mut self, node: u64, method: &str, args: &[Variant]) -> Option<Variant> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        // NavigationAgent2D/3D answer their own method surface (pat-s3rrv.20).
        // First, so an agent getter cannot be shadowed by a generic node arm;
        // it returns `None` for every non-agent node, so the table below is
        // reached unchanged.
        if let Some(value) =
            crate::navigation_agents::call_agent_method(self.tree_mut(), nid, method, args)
        {
            return Some(value);
        }
        // ShapeCast2D answers its own surface (pat-s3rrv.87), and must do so
        // BEFORE the generic table below: that table's `is_colliding` /
        // `get_collider` / `get_collision_point` arms read RayCast2D's SINGULAR
        // `_hit_point` / `_hit_collider` keys, which a shape cast never writes —
        // it publishes indexed arrays. Left to the table, a ShapeCast2D would
        // read a raycast's empty results and report `false` forever. The handler
        // declines every non-ShapeCast2D node, so the table stays reachable.
        // A slide-collision handle answers its own getters, and must do so
        // BEFORE the table below for the same reason ShapeCast2D does: that
        // table's `get_collider` / `get_normal` / `get_position` arms read
        // RayCast2D's `_hit_*` keys, which a collision handle never writes.
        // Left to the table it would read a raycast's empty results and report
        // nothing (pat-s3rrv.95). Declines every non-handle receiver.
        if let Some(value) = kinematic_collision_call(self.tree(), node, method, args) {
            return Some(value);
        }
        // XRNode3D and its subclasses answer their own surface (pat-s3rrv.219).
        // Declines every non-XR node, so the table below stays reachable.
        if let Some(value) = crate::xr_nodes::call_xr_node_method(self.tree(), nid, method, args) {
            return Some(value);
        }
        // Timer's own surface (pat-s3rrv.121); declines every non-Timer node.
        if let Some(value) = timer_node_call(self.tree_mut(), nid, method, args) {
            return Some(value);
        }

        // VehicleWheel3D's script surface (pat-s3rrv.194); declines every other
        // node.
        //
        // ⚠️ Wired HERE and nowhere else on purpose. `call_method_on` in the
        // interpreter falls back to this table for names it does not know
        // (`interpreter.rs:14281`), so one registration serves BOTH
        // `$Wheel.is_in_contact()` and the bare `is_in_contact()` a script on
        // the wheel itself would write. Adding it to the interpreter instead
        // would cover only the first spelling.
        //
        // The values are read off node properties the physics server published
        // in `sync_from_physics` — by the time a script runs it has the tree and
        // nothing else, so the simulated state has to travel on the node.
        let is_wheel = self
            .tree()
            .get_node(nid)
            .map(|n| n.class_name() == "VehicleWheel3D")
            .unwrap_or(false);
        if is_wheel {
            match method {
                "is_in_contact" => {
                    let touching = matches!(
                        self.tree()
                            .get_node(nid)
                            .map(|n| n.get_property("_wheel_in_contact")),
                        Some(Variant::Bool(true))
                    );
                    return Some(Variant::Bool(touching));
                }
                "get_skidinfo" => {
                    // Godot returns 1.0 for full grip and falls toward 0 as the
                    // tire slips; an airborne wheel reports full grip because it
                    // is not slipping against anything.
                    let v = match self
                        .tree()
                        .get_node(nid)
                        .map(|n| n.get_property("_wheel_compression"))
                    {
                        Some(Variant::Float(f)) => f,
                        Some(Variant::Int(i)) => i as f64,
                        _ => 1.0,
                    };
                    return Some(Variant::Float(v));
                }
                "get_contact_point" => {
                    let p = match self
                        .tree()
                        .get_node(nid)
                        .map(|n| n.get_property("_wheel_contact_point"))
                    {
                        Some(Variant::Vector3(v)) => v,
                        _ => gdcore::math::Vector3::ZERO,
                    };
                    return Some(Variant::Vector3(p));
                }
                _ => {}
            }
        }

        // AnimatedSprite2D/3D's playback surface (pat-s3rrv.182); declines every
        // other node.
        //
        // ⚠️ These MUST be wired here as well as in
        // `interpreter.rs::call_method_on`. The two tables serve different
        // spellings: `$Sprite.play_backwards("walk")` goes through
        // `call_method_on`, but a **bare** `play_backwards("walk")` — the form a
        // script attached to the sprite itself uses, and the form Godot's own
        // documentation shows — resolves through `call_self_func_or_native` into
        // THIS table. Wiring only the interpreter side ships a surface that
        // works from one spelling and raises `UndefinedFunction` from the one
        // people actually write.
        //
        // The failure is also silent in a deceptive way: `UndefinedFunction`
        // aborts the whole handler at the first unknown name, so every later
        // statement in `_ready` quietly does not run and the symptom shows up on
        // some unrelated property being left unset.
        let is_animated_sprite = self
            .tree()
            .get_node(nid)
            .map(|n| matches!(n.class_name(), "AnimatedSprite2D" | "AnimatedSprite3D"))
            .unwrap_or(false);
        if is_animated_sprite {
            let clip = match args.first() {
                Some(Variant::String(s)) => s.clone(),
                Some(Variant::StringName(s)) => s.as_str().to_string(),
                _ => String::new(),
            };
            match method {
                "play" => {
                    // Godot's default `custom_speed` is 1.0, and its SIGN is what
                    // runs the clip backwards; `from_end` only picks the starting
                    // frame. They are independent arguments.
                    let custom_speed = match args.get(1) {
                        Some(Variant::Float(f)) => *f,
                        Some(Variant::Int(i)) => *i as f64,
                        _ => 1.0,
                    };
                    let from_end = matches!(args.get(2), Some(Variant::Bool(true)));
                    self.animated_sprite_play(node, &clip, custom_speed, from_end);
                    return Some(Variant::Nil);
                }
                "play_backwards" => {
                    self.animated_sprite_play_backwards(node, &clip);
                    return Some(Variant::Nil);
                }
                "pause" => {
                    self.animated_sprite_pause(node);
                    return Some(Variant::Nil);
                }
                "stop" => {
                    self.animated_sprite_stop(node);
                    return Some(Variant::Nil);
                }
                "set_frame_and_progress" => {
                    let frame = match args.first() {
                        Some(Variant::Int(i)) => *i,
                        Some(Variant::Float(f)) => *f as i64,
                        _ => 0,
                    };
                    let progress = match args.get(1) {
                        Some(Variant::Float(f)) => *f,
                        Some(Variant::Int(i)) => *i as f64,
                        _ => 0.0,
                    };
                    self.animated_sprite_set_frame_and_progress(node, frame, progress);
                    return Some(Variant::Nil);
                }
                "get_playing_speed" => {
                    return Some(Variant::Float(self.animated_sprite_playing_speed(node)));
                }
                _ => {}
            }
        }

        // Node2D's global-transform surface (pat-s3rrv.184); declines every
        // other node. The 2D side had no script method surface at all — this
        // file grew a Node3D block (`look_at`, `rotate`, `to_global`) and
        // nothing equivalent for 2D — so `sprite.get_global_position()` raised
        // UndefinedFunction while `node2d::get_global_position` was computing
        // the right answer one layer down.
        if let Some(value) = crate::node2d::call_node2d_method(self.tree_mut(), nid, method, args) {
            return Some(value);
        }

        // NavigationRegion3D's runtime bake (pat-s3rrv.180); declines every
        // other node. A 3D baker has existed in `gdserver3d` since the port
        // began with no caller able to reach it — this arm is what lets a script
        // rebake a region and have the next `process_navigation` path over it.
        if let Some(value) =
            crate::nav_bake::call_nav_region_method(self.tree_mut(), nid, method, args)
        {
            return Some(value);
        }

        // ColorPicker's preset surface (pat-s3rrv.172). Declines every other
        // node, INCLUDING ColorPickerButton: upstream gives the button only
        // `get_picker`/`get_popup` (`doc/classes/ColorPickerButton.xml`), so
        // forwarding the preset methods to it would invent API a real project
        // cannot call. Answered here rather than by a generic property arm,
        // which would hand back the raw `presets` Variant — Nil for a picker
        // that has never had one set, instead of an empty PackedColorArray.
        if let Some(value) =
            crate::color_picker::call_color_picker_method(self.tree_mut(), nid, method, args)
        {
            return Some(value);
        }

        if method == "force_shapecast_update" {
            // Inlined rather than factored into a helper method: this block is
            // inside `impl SceneAccess`, which may only contain that trait's own
            // members, so a helper here would mean widening the trait in
            // gdscript-interop for one caller.
            //
            // Godot re-runs the sweep immediately so the SAME frame observes the
            // result; the per-tick pass runs before the script phase, so this is
            // what lets a script re-query after moving the cast mid-frame.
            let addr = self.tree().script_physics_addr();
            if addr != 0 {
                // SAFETY: identical contract to `force_raycast_update_2d` —
                // `set_script_physics_server` stored the address of a live
                // `PhysicsServer` that `MainLoop` clears before the server can
                // move or drop, and the server is a sibling of the tree, never
                // stored inside it, so this shared borrow cannot alias the
                // `&mut SceneTree` taken on the next line.
                let physics = unsafe { &*(addr as *const crate::physics_server::PhysicsServer) };
                crate::shapecast2d::force_shapecast_update(self.tree_mut(), physics, nid);
            }
            return Some(Variant::Nil);
        }
        if let Some(value) =
            crate::shapecast2d::call_shapecast_method(self.tree_mut(), nid, method, args)
        {
            return Some(value);
        }
        // TileMapLayer's cell/coordinate API (pat-s3rrv.88). Every operation had
        // a Rust implementation and no binding at all, so a running game could
        // not touch its tilemap — procedural generation, the node's dominant
        // use, was impossible. Declines every non-tilemap node.
        if let Some(value) = crate::tilemap::call_tilemap_method(self.tree_mut(), nid, method, args)
        {
            return Some(value);
        }
        // Skeleton3D's bone API (pat-s3rrv.23). Same shape and same reason as
        // the agent arm above: it answers `None` for every non-skeleton node,
        // so the generic table below is reached unchanged.
        //
        // ClassDB had already been taught these method names, which opened the
        // `class_has_method` dispatch gate onto nothing — the 38 working
        // functions in `skeleton3d` still had no script-facing binding.
        if let Some(value) =
            crate::skeleton3d::call_skeleton_method(self.tree_mut(), nid, method, args)
        {
            return Some(value);
        }
        // The `Range` value model (pat-s3rrv.73). Same shape and same reason as
        // the two arms above: it answers `None` for every non-range node and for
        // every method it does not implement, so the generic table below is
        // reached unchanged.
        //
        // ⚠️ Without this a script cannot move a slider at all. `class_has_method`
        // consults declared `MethodInfo` only, and `Range` registers its value
        // model as PROPERTIES — so `set_value` is not merely inert, it does not
        // resolve. Both spellings land here: `$Bar.set_value(50)` through
        // `interpreter.rs::call_method_on`'s fallback, and a bare `set_value(50)`
        // through `call_self_func_or_native`.
        if let Some(value) =
            crate::range_controls::call_range_method(self.tree_mut(), nid, method, args)
        {
            return Some(value);
        }
        // String-ish first argument, shared by the group/child/signal calls.
        let arg_str = |i: usize| -> Option<String> {
            match args.get(i) {
                Some(Variant::String(s)) => Some(s.clone()),
                Some(Variant::StringName(s)) => Some(s.as_str().to_string()),
                Some(Variant::NodePath(p)) => Some(p.to_string()),
                _ => None,
            }
        };
        let bool_prop = |tree: &SceneTree, key: &str| -> bool {
            tree.get_node(nid)
                .map(|n| matches!(n.get_property(key), Variant::Bool(true)))
                .unwrap_or(false)
        };
        // Godot's Vector2 getters return `Vector2.ZERO`, never null, so a script
        // can chain `.length()` on the result before the first physics tick has
        // published anything (pat-0q2kx.33).
        let vec2_prop = |tree: &SceneTree, key: &str| -> gdcore::math::Vector2 {
            match tree.get_node(nid).map(|n| n.get_property(key)) {
                Some(Variant::Vector2(v)) => v,
                _ => gdcore::math::Vector2::ZERO,
            }
        };
        // The velocity getters are shared by CharacterBody2D and
        // CharacterBody3D, so they must answer in the BODY'S OWN dimension
        // (pat-s3rrv.103).
        //
        // ⚠️ `vec2_prop` above matches `Variant::Vector2` and falls through to
        // ZERO for anything else — so on a CharacterBody3D, whose solver
        // publishes `Vector3`, it did not merely return the wrong TYPE, it
        // returned a zero VALUE. A 3D script polling `get_platform_velocity()`
        // read (0, 0) off a body riding a moving platform, which reads as "the
        // platform is not moving" rather than as a type error.
        let vel_prop = |tree: &SceneTree, key: &str| -> Variant {
            match tree.get_node(nid).map(|n| n.get_property(key)) {
                Some(v @ (Variant::Vector2(_) | Variant::Vector3(_))) => v,
                // Nothing published yet — before the first physics tick, or on
                // a body that never moved. Godot still answers a zero vector
                // rather than null, so a script may chain `.length()` on it;
                // it has to be the zero of the RIGHT dimension, or the very
                // first read of a 3D body is a Vector2 again.
                _ => {
                    if tree
                        .get_node(nid)
                        .is_some_and(|n| n.class_name() == "CharacterBody3D")
                    {
                        Variant::Vector3(gdcore::math::Vector3::ZERO)
                    } else {
                        Variant::Vector2(gdcore::math::Vector2::ZERO)
                    }
                }
            }
        };

        match method {
            // -- Multiplayer authority (pat-s3rrv.17) ------------------------
            //
            // Answered for ANY node: in Godot authority is a `Node` property,
            // not something only the replication nodes carry. Placed first so a
            // node class that happens to declare a same-named method cannot
            // shadow the engine rule.
            "get_multiplayer_authority" => {
                Some(Variant::Int(self.tree().get_multiplayer_authority(nid)))
            }
            "is_multiplayer_authority" => {
                Some(Variant::Bool(self.tree().is_multiplayer_authority(nid)))
            }
            "set_multiplayer_authority" => {
                let Some(authority) = (match args.first() {
                    Some(Variant::Int(i)) => Some(*i),
                    Some(Variant::Float(f)) => Some(*f as i64),
                    _ => None,
                }) else {
                    // Godot requires an int id; a bad argument leaves authority
                    // untouched rather than resetting it to the server, which
                    // would silently hand the node back to the host.
                    return Some(Variant::Nil);
                };
                // ⚠️ Godot's second argument DEFAULTS TO TRUE. Reading a missing
                // argument as `false` would set authority on the node alone and
                // leave every child inheriting the server — the subtle half-
                // transfer this API exists to avoid.
                let recursive = !matches!(args.get(1), Some(Variant::Bool(false)));
                self.tree_mut()
                    .set_multiplayer_authority(nid, authority, recursive);
                Some(Variant::Nil)
            }
            // -- MultiplayerSynchronizer visibility (pat-s3rrv.17) -----------
            //
            // Both halves of this feature already existed and could not meet:
            // the per-peer state lives on the synchronizer and `poll_outbound`
            // already refuses to send to a peer it is hidden from, but no script
            // could reach either one.
            "set_visibility_for" => {
                let peer = match args.first() {
                    Some(Variant::Int(i)) => *i,
                    Some(Variant::Float(f)) => *f as i64,
                    _ => return Some(Variant::Nil),
                };
                // ⚠️ `visible` is required. Reading a missing or non-bool second
                // argument as `false` would HIDE the node — the opposite of the
                // no-op a malformed call should be.
                let Some(Variant::Bool(visible)) = args.get(1) else {
                    return Some(Variant::Nil);
                };
                self.tree_mut()
                    .synchronizer_set_visibility_for(nid, peer, *visible);
                Some(Variant::Nil)
            }
            "is_visible_to" => {
                let peer = match args.first() {
                    Some(Variant::Int(i)) => *i,
                    Some(Variant::Float(f)) => *f as i64,
                    _ => return Some(Variant::Nil),
                };
                // Reads the cached filter verdict plus the stored per-peer
                // state; it never runs a filter, so `tree()` suffices.
                Some(Variant::Bool(
                    self.tree().synchronizer_is_visible_to(nid, peer),
                ))
            }
            "add_visibility_filter" => {
                // The Callable is stored, not called here: it is evaluated once
                // per peer during the outbound replication pass, which is the
                // only place the peer list exists.
                let Some(filter) = args.first().cloned() else {
                    return Some(Variant::Nil);
                };
                self.tree_mut()
                    .synchronizer_add_visibility_filter(nid, filter);
                Some(Variant::Nil)
            }
            // Godot `remove_visibility_filter(callable)` — void, and silent
            // about a filter that was never added (`HashSet::erase`,
            // `multiplayer_synchronizer.cpp:216`). Without this arm a script
            // could install a filter and never take it back: the only way to
            // stop filtering was to free the synchronizer.
            "remove_visibility_filter" => {
                let Some(filter) = args.first().cloned() else {
                    return Some(Variant::Nil);
                };
                self.tree_mut()
                    .synchronizer_remove_visibility_filter(nid, filter);
                Some(Variant::Nil)
            }
            // -- Physics 2D space queries (pat-s3rrv.26) ---------------------
            //
            // `get_world_2d()` is answered for any node: Patina has one
            // `PhysicsWorld2D` per tree, so the handle is a singleton rather
            // than something keyed on the caller.
            // -- CharacterBody2D slide contacts (pat-s3rrv.95) ------------
            //
            // `physics_server` publishes each tick's contacts as
            // `_slide_collisions`; these turn them into the objects Godot's
            // idiom indexes. Names are specific enough not to shadow anything,
            // so they sit in the table rather than needing to precede it.
            "get_slide_collision_count" => {
                Some(Variant::Int(slide_collision_count(self.tree(), nid) as i64))
            }
            // Godot returns null for an out-of-range index rather than raising,
            // so a `for i in count` loop that races a tick degrades quietly.
            "get_slide_collision" => {
                let idx = match args.first() {
                    Some(Variant::Int(i)) if *i >= 0 => *i as usize,
                    _ => return Some(Variant::Nil),
                };
                if idx >= slide_collision_count(self.tree(), nid) {
                    return Some(Variant::Nil);
                }
                Some(Variant::ObjectId(ObjectId::from_raw(
                    kinematic_collision_handle_for(node, idx),
                )))
            }
            // Godot's `move_and_collide(motion)`: sweep and STOP at the first
            // contact, returning a `KinematicCollision2D` or null
            // (pat-s3rrv.97). The sweep itself lives on the physics server,
            // which owns the world; this arm only bridges to it.
            "move_and_collide" => {
                let motion = match args.first() {
                    Some(Variant::Vector2(v)) => *v,
                    _ => return Some(Variant::Nil),
                };
                let addr = self.tree().script_physics_addr();
                if addr == 0 {
                    return Some(Variant::Nil);
                }
                // SAFETY: identical contract to `force_shapecast_update` above —
                // the address is of a live `PhysicsServer` that outlives this
                // call and is a sibling of the tree, never inside it, so this
                // shared borrow cannot alias the `&mut` tree borrow below.
                let physics = unsafe { &*(addr as *const crate::physics_server::PhysicsServer) };
                if !physics.script_move_and_collide(self.tree_mut(), nid, motion) {
                    // Godot returns null when nothing was hit, and scripts test
                    // exactly that; handing back an object with zeroed fields
                    // would make `if collision:` true after a clean move.
                    return Some(Variant::Nil);
                }
                Some(Variant::ObjectId(ObjectId::from_raw(
                    kinematic_collision_handle_for(node, MOVE_AND_COLLIDE_INDEX),
                )))
            }
            "get_last_slide_collision" => {
                match slide_collision_count(self.tree(), nid).checked_sub(1) {
                    Some(last) => Some(Variant::ObjectId(ObjectId::from_raw(
                        kinematic_collision_handle_for(node, last),
                    ))),
                    // No contacts this tick — Godot returns null, not an object
                    // describing a collision that did not happen.
                    None => Some(Variant::Nil),
                }
            }
            "get_world_2d" => Some(Variant::ObjectId(ObjectId::from_raw(WORLD_2D_HANDLE))),
            // The 3D twin (pat-s3rrv.107). Like `World2D`, `World3D` and its
            // space state are fixed singletons — one `PhysicsWorld3D` per tree —
            // so they are constant handles rather than registry entries.
            "get_world_3d" => Some(Variant::ObjectId(ObjectId::from_raw(WORLD_3D_HANDLE))),
            // Queries on the space-state handle. Guarded on the receiver being
            // that handle so these names cannot shadow a same-named script or
            // node method on an ordinary node.
            // 3D ray query (pat-s3rrv.107).
            //
            // ⚠️ Reads the parameters off a `Variant::Resource` rather than a
            // handle registry, which is where the 2D path keeps them. That is
            // deliberate: `PhysicsRayQueryParameters3D` is a RefCounted class
            // that already constructs from script and carries its own property
            // map, so a second registry would be duplicate state that could
            // disagree with the object the script is holding.
            // 3D point query (pat-s3rrv.107): every body containing a point.
            //
            // ⚠️ `collision_mask` is set ON THE QUERY rather than pre-filtering
            // the body iterator, because `intersect_bodies` already honours the
            // mask, `exclude` and `max_results` itself. Filtering here as well
            // would apply the mask twice — harmless today, but it would silently
            // diverge the moment the library's own rule changes.
            "intersect_point" if node == SPACE_STATE_3D_HANDLE => {
                let params = match args.first() {
                    Some(Variant::Resource(r)) => r,
                    _ => return None,
                };
                let position = match params.properties.get("position") {
                    Some(Variant::Vector3(v)) => *v,
                    // No point to test: Godot answers an empty Array.
                    _ => return Some(Variant::Array(Vec::new())),
                };
                let mut query = gdphysics3d::query::PhysicsPointQuery3D::new(position);
                if let Some(Variant::Int(m)) = params.properties.get("collision_mask") {
                    query.collision_mask = *m as u32;
                }
                let server = self.physics_3d()?;
                let ids = query.intersect_bodies(server.world().bodies());
                let out: Vec<Variant> = ids
                    .into_iter()
                    .map(|body_id| {
                        let mut d = gdvariant::Dictionary::new();
                        if let Some(nid) = server.node_for_body(body_id) {
                            d.insert("collider", Variant::ObjectId(nid.object_id()));
                        }
                        Variant::Dictionary(d)
                    })
                    .collect();
                Some(Variant::Array(out))
            }
            // The shape-based 3D queries (pat-s3rrv.107). All four decode
            // their parameters through `shape_query_3d_from_params`, so the
            // extent conversion and mask handling cannot drift between them.
            //
            // A params object with no usable shape yields Godot's empty answer
            // for each — an Array, a full-travel `[1, 1]`, or an empty
            // Dictionary — rather than a guessed shape, which would report
            // contacts that do not exist.
            "intersect_shape" if node == SPACE_STATE_3D_HANDLE => {
                let Some(Variant::Resource(params)) = args.first() else {
                    return None;
                };
                let Some(query) = shape_query_3d_from_params(params) else {
                    return Some(Variant::Array(Vec::new()));
                };
                let server = self.physics_3d()?;
                let hits = query.intersect(server.world().bodies());
                let out: Vec<Variant> = hits
                    .into_iter()
                    .map(|hit| {
                        let mut d = gdvariant::Dictionary::new();
                        if let Some(nid) = server.node_for_body(hit.body_id) {
                            d.insert("collider", Variant::ObjectId(nid.object_id()));
                        }
                        Variant::Dictionary(d)
                    })
                    .collect();
                Some(Variant::Array(out))
            }
            // `cast_motion(params)` — `[safe, unsafe]` fractions of the
            // params' `motion`, which is Godot's return shape.
            "cast_motion" if node == SPACE_STATE_3D_HANDLE => {
                let Some(Variant::Resource(params)) = args.first() else {
                    return None;
                };
                let unobstructed = || {
                    // Godot answers `[1, 1]` when nothing blocks the motion —
                    // NOT an empty array, so `result[0]` is always indexable.
                    Some(Variant::Array(vec![
                        Variant::Float(1.0),
                        Variant::Float(1.0),
                    ]))
                };
                let Some(query) = shape_query_3d_from_params(params) else {
                    return unobstructed();
                };
                let server = self.physics_3d()?;
                let (safe, unsafe_) = query.cast_motion(server.world().bodies());
                Some(Variant::Array(vec![
                    Variant::Float(safe as f64),
                    Variant::Float(unsafe_ as f64),
                ]))
            }
            // `collide_shape(params)` — contact points as PAIRS, flattened into
            // one Array exactly as Godot returns them (own point, then the
            // other body's, per contact).
            "collide_shape" if node == SPACE_STATE_3D_HANDLE => {
                let Some(Variant::Resource(params)) = args.first() else {
                    return None;
                };
                let Some(query) = shape_query_3d_from_params(params) else {
                    return Some(Variant::Array(Vec::new()));
                };
                let server = self.physics_3d()?;
                let pairs = query.collide_shape(server.world().bodies());
                let mut out: Vec<Variant> = Vec::with_capacity(pairs.len() * 2);
                for [a, b] in pairs {
                    out.push(Variant::Vector3(a));
                    out.push(Variant::Vector3(b));
                }
                Some(Variant::Array(out))
            }
            // `get_rest_info(params)` — the NEAREST contact as a Dictionary,
            // empty on a miss so `if info:` works unguarded.
            "get_rest_info" if node == SPACE_STATE_3D_HANDLE => {
                let Some(Variant::Resource(params)) = args.first() else {
                    return None;
                };
                let empty = || Some(Variant::Dictionary(gdvariant::Dictionary::new()));
                let Some(query) = shape_query_3d_from_params(params) else {
                    return empty();
                };
                let server = self.physics_3d()?;
                let Some(info) = query.get_rest_info(server.world().bodies()) else {
                    return empty();
                };
                let mut d = gdvariant::Dictionary::new();
                d.insert("point", Variant::Vector3(info.point));
                d.insert("normal", Variant::Vector3(info.normal));
                d.insert("linear_velocity", Variant::Vector3(info.linear_velocity));
                if let Some(nid) = server.node_for_body(info.collider) {
                    d.insert("collider", Variant::ObjectId(nid.object_id()));
                }
                Some(Variant::Dictionary(d))
            }
            "intersect_ray" if node == SPACE_STATE_3D_HANDLE => {
                let params = match args.first() {
                    Some(Variant::Resource(r)) => r,
                    // Godot only accepts the parameters object.
                    _ => return None,
                };
                let v3 = |key: &str| match params.properties.get(key) {
                    Some(Variant::Vector3(v)) => Some(*v),
                    _ => None,
                };
                let (from, to) = match (v3("from"), v3("to")) {
                    (Some(f), Some(t)) => (f, t),
                    // Without both endpoints there is no ray. Godot answers an
                    // empty Dictionary rather than erroring, so `if result:`
                    // works unguarded.
                    _ => return Some(Variant::Dictionary(gdvariant::Dictionary::new())),
                };
                let mask = match params.properties.get("collision_mask") {
                    Some(Variant::Int(m)) => *m as u32,
                    // Godot's default mask is all layers.
                    _ => u32::MAX,
                };
                let world = self.physics_3d()?.world();
                let query = gdphysics3d::query::PhysicsRayQuery3D::new(from, to);
                let hit = match query.intersect(world.bodies().filter(|b| {
                    // `collision_mask` selects which layers the ray sees.
                    (b.collision_layer & mask) != 0
                })) {
                    Some(hit) => hit,
                    None => return Some(Variant::Dictionary(gdvariant::Dictionary::new())),
                };
                let mut d = gdvariant::Dictionary::new();
                d.insert("position", Variant::Vector3(hit.point));
                d.insert("normal", Variant::Vector3(hit.normal));
                if let Some(nid) = self.physics_3d()?.node_for_body(hit.body_id) {
                    d.insert("collider", Variant::ObjectId(nid.object_id()));
                }
                Some(Variant::Dictionary(d))
            }
            "intersect_ray" if node == SPACE_STATE_2D_HANDLE => {
                let params = match args.first() {
                    Some(Variant::ObjectId(id)) => ray_params(id.raw())?,
                    // Godot only accepts the parameters object.
                    _ => return None,
                };
                // `exclude` carries NODE ids — that is what a script holds —
                // while the world filters by `BodyId`, so map through the same
                // registry `collider` uses in reverse. A node with no
                // registered body drops out of the list instead of becoming
                // body 0 and excluding an unrelated body.
                //
                // Bound before `world` so the two immutable borrows of `self`
                // do not have to overlap a closure capture.
                let exclude: Vec<_> = {
                    let phys = self.physics()?;
                    params
                        .exclude
                        .iter()
                        .filter_map(|raw| {
                            phys.body_for_node(NodeId::from_object_id(ObjectId::from_raw(*raw)))
                        })
                        .collect()
                };
                let world = self.physics()?.world();
                // ⚠️ Built through the builder, not a struct literal:
                // `RayQuery2D` derives no `Default`, and its
                // `collide_with_bodies` / `hit_from_inside` defaults are part of
                // Godot's semantics — `new` sets them, a literal would force me
                // to restate them and drift.
                let query = gdphysics2d::world::RayQuery2D::new(params.from, params.to)
                    .with_collision_mask(params.collision_mask)
                    .with_exclude(exclude);
                // Godot answers an EMPTY Dictionary on a miss, never null, so a
                // script can test `if result:` and index it without a guard.
                let hit = match world.intersect_ray(&query) {
                    Some(hit) => hit,
                    None => return Some(Variant::Dictionary(gdvariant::Dictionary::new())),
                };
                // `collider` is the hit NODE, which is what a script actually
                // uses (`result.collider.queue_free()`). `node_for_body` is the
                // same reverse map `RayCast2D.get_collider` already relies on
                // (physics_server.rs:895), so the two agree by construction.
                let collider = self.physics().and_then(|p| p.node_for_body(hit.body_id));
                let mut dict = gdvariant::Dictionary::new();
                dict.insert(
                    Variant::String("position".to_string()),
                    Variant::Vector2(hit.point),
                );
                dict.insert(
                    Variant::String("normal".to_string()),
                    Variant::Vector2(hit.normal),
                );
                if let Some(node_id) = collider {
                    dict.insert(
                        Variant::String("collider".to_string()),
                        Variant::ObjectId(node_id.object_id()),
                    );
                }
                Some(Variant::Dictionary(dict))
            }
            // `intersect_shape(params, max_results)` — every body overlapping
            // the parameters' shape at its transform, as an ARRAY of
            // Dictionaries (empty when nothing overlaps).
            //
            // The narrowphase behind this (`PhysicsWorld2D::shape_cast`) was
            // already correct and unit-tested; what was missing was any way for
            // a script to name a shape at all, which is why the shape-resource
            // constructors land in the same bead.
            "intersect_shape" if node == SPACE_STATE_2D_HANDLE => {
                let params = match args.first() {
                    Some(Variant::ObjectId(id)) => shape_params(id.raw())?,
                    _ => return None,
                };
                // A parameters object with no shape assigned overlaps nothing,
                // rather than defaulting to some shape the script never chose.
                let shape = shape_for(params.shape)?;
                let max_results = match args.get(1) {
                    Some(Variant::Int(n)) if *n >= 0 => *n as usize,
                    _ => 32,
                };
                let exclude: Vec<_> = {
                    let phys = self.physics()?;
                    params
                        .exclude
                        .iter()
                        .filter_map(|raw| {
                            phys.body_for_node(NodeId::from_object_id(ObjectId::from_raw(*raw)))
                        })
                        .collect()
                };
                let hits = self.physics()?.world().shape_cast(
                    &shape,
                    params.transform,
                    &exclude,
                    params.collision_mask,
                );
                let mut out: Vec<Variant> = Vec::new();
                for hit in hits.into_iter().take(max_results) {
                    let Some(node_id) = self.physics().and_then(|p| p.node_for_body(hit.body_id))
                    else {
                        continue;
                    };
                    let mut entry = gdvariant::Dictionary::new();
                    entry.insert(
                        Variant::String("collider".to_string()),
                        Variant::ObjectId(node_id.object_id()),
                    );
                    // Godot reports which shape OF THE COLLIDER was hit; Patina
                    // registers one shape per body, so it is always index 0.
                    entry.insert(Variant::String("shape".to_string()), Variant::Int(0));
                    out.push(Variant::Dictionary(entry));
                }
                Some(Variant::Array(out))
            }
            // `collide_shape(params, max_results)` — contact points as PAIRS
            // (first on the query shape, second on the collider), per
            // doc/classes/PhysicsDirectSpaceState2D.xml. `max_results` counts
            // Godot's *results*, i.e. pairs, so the returned array is twice it.
            "collide_shape" if node == SPACE_STATE_2D_HANDLE => {
                let params = match args.first() {
                    Some(Variant::ObjectId(id)) => shape_params(id.raw())?,
                    _ => return None,
                };
                let shape = shape_for(params.shape)?;
                let max_results = match args.get(1) {
                    Some(Variant::Int(n)) if *n >= 0 => *n as usize,
                    _ => 32,
                };
                let exclude = self.shape_query_exclude(&params)?;
                let points = self.physics()?.world().collide_shape(
                    &shape,
                    params.transform,
                    &exclude,
                    params.collision_mask,
                    max_results,
                );
                Some(Variant::Array(
                    points.into_iter().map(Variant::Vector2).collect(),
                ))
            }
            // `cast_motion(params)` — `[safe, unsafe]` fractions of
            // `params.motion`. Godot returns a PackedFloat32Array; Patina has no
            // packed-float Variant, so it surfaces as a 2-element Array, which
            // indexes identically from script (`result[0]`).
            "cast_motion" if node == SPACE_STATE_2D_HANDLE => {
                let params = match args.first() {
                    Some(Variant::ObjectId(id)) => shape_params(id.raw())?,
                    _ => return None,
                };
                let shape = shape_for(params.shape)?;
                let exclude = self.shape_query_exclude(&params)?;
                let fractions = self.physics()?.world().cast_motion(
                    &shape,
                    params.transform,
                    params.motion,
                    &exclude,
                    params.collision_mask,
                );
                Some(Variant::Array(vec![
                    Variant::Float(fractions[0] as f64),
                    Variant::Float(fractions[1] as f64),
                ]))
            }
            // `get_rest_info(params)` — the NEAREST contact, as a Dictionary.
            // Godot answers an EMPTY Dictionary when nothing overlaps, the same
            // contract `intersect_ray` uses, so `if info:` works unguarded.
            "get_rest_info" if node == SPACE_STATE_2D_HANDLE => {
                let params = match args.first() {
                    Some(Variant::ObjectId(id)) => shape_params(id.raw())?,
                    _ => return None,
                };
                let shape = shape_for(params.shape)?;
                let exclude = self.shape_query_exclude(&params)?;
                let hit = match self.physics()?.world().shape_rest_info(
                    &shape,
                    params.transform,
                    &exclude,
                    params.collision_mask,
                ) {
                    Some(hit) => hit,
                    None => return Some(Variant::Dictionary(gdvariant::Dictionary::new())),
                };
                let mut dict = gdvariant::Dictionary::new();
                dict.insert(
                    Variant::String("point".to_string()),
                    Variant::Vector2(hit.point),
                );
                dict.insert(
                    Variant::String("normal".to_string()),
                    Variant::Vector2(hit.normal),
                );
                // Godot documents `linear_velocity` as the colliding object's
                // velocity — (0,0) for a static body, which falls out naturally.
                let velocity = self
                    .physics()
                    .and_then(|p| p.world().get_body(hit.body_id).map(|b| b.linear_velocity))
                    .unwrap_or(gdcore::math::Vector2::ZERO);
                dict.insert(
                    Variant::String("linear_velocity".to_string()),
                    Variant::Vector2(velocity),
                );
                if let Some(node_id) = self.physics().and_then(|p| p.node_for_body(hit.body_id)) {
                    dict.insert(
                        Variant::String("collider".to_string()),
                        Variant::ObjectId(node_id.object_id()),
                    );
                }
                dict.insert(Variant::String("shape".to_string()), Variant::Int(0));
                Some(Variant::Dictionary(dict))
            }
            // `intersect_point(params, max_results)` — Godot answers an ARRAY of
            // Dictionaries (one per overlapping body), not a single hit, and an
            // empty array when nothing is under the point.
            "intersect_point" if node == SPACE_STATE_2D_HANDLE => {
                let point = match args.first() {
                    // Godot takes a PhysicsPointQueryParameters2D; a bare
                    // Vector2 is accepted too so a script can query a position
                    // without constructing a parameters object for it.
                    Some(Variant::Vector2(v)) => *v,
                    Some(Variant::ObjectId(id)) => ray_params(id.raw())?.from,
                    _ => return None,
                };
                // Godot's `max_results` defaults to 32.
                let max_results = match args.get(1) {
                    Some(Variant::Int(n)) if *n >= 0 => *n as usize,
                    _ => 32,
                };
                let bodies = self.physics()?.world().point_query(point);
                let mut out: Vec<Variant> = Vec::new();
                for body_id in bodies.into_iter().take(max_results) {
                    let Some(node_id) = self.physics().and_then(|p| p.node_for_body(body_id))
                    else {
                        continue;
                    };
                    let mut entry = gdvariant::Dictionary::new();
                    entry.insert(
                        Variant::String("collider".to_string()),
                        Variant::ObjectId(node_id.object_id()),
                    );
                    out.push(Variant::Dictionary(entry));
                }
                Some(Variant::Array(out))
            }
            // -- Node3D transform API (pat-s3rrv.24) -------------------------
            //
            // `gdscene::node3d` already implements all of this correctly; it was
            // simply unreachable. `class_has_method` now answers true for these
            // (the registration half of this bead), but registration only makes
            // a method *reflectable* — a script call still has to land on an
            // arm here, and there was none: `grep '"look_at"'` over both
            // interpreter.rs and this file returned 0.
            //
            // Godot signatures from doc/classes/Node3D.xml. A malformed
            // argument yields `None` (= "not handled") rather than a panic,
            // matching how the 2D/3D force arms decline a call meant for the
            // other dimension.
            "look_at" => {
                let target = vec3_arg(args, 0)?;
                // `up` is optional in Godot and defaults to Vector3.UP.
                match vec3_arg(args, 1) {
                    Some(up) => crate::node3d::look_at(self.tree_mut(), nid, target, up),
                    None => crate::node3d::look_at_default_up(self.tree_mut(), nid, target),
                }
                Some(Variant::Nil)
            }
            "translate" if !args.is_empty() && vec3_arg(args, 0).is_some() => {
                // ⚠️ Guarded on a Vector3 first argument: `translate` is also
                // Godot's *i18n* call (`Object.tr`-adjacent), which takes a
                // String. Without the guard this arm would swallow every
                // localisation call made on a node.
                let offset = vec3_arg(args, 0)?;
                crate::node3d::translate(self.tree_mut(), nid, offset);
                Some(Variant::Nil)
            }
            "rotate" => {
                let axis = vec3_arg(args, 0)?;
                let angle = arg_f32(args, 1)?;
                crate::node3d::rotate(self.tree_mut(), nid, axis, angle);
                Some(Variant::Nil)
            }
            "rotate_x" | "rotate_y" | "rotate_z" => {
                let angle = arg_f32(args, 0)?;
                let axis = match method {
                    "rotate_x" => gdcore::math::Vector3::new(1.0, 0.0, 0.0),
                    "rotate_y" => gdcore::math::Vector3::new(0.0, 1.0, 0.0),
                    _ => gdcore::math::Vector3::new(0.0, 0.0, 1.0),
                };
                crate::node3d::rotate(self.tree_mut(), nid, axis, angle);
                Some(Variant::Nil)
            }
            "to_global" => {
                let local = vec3_arg(args, 0)?;
                Some(Variant::Vector3(crate::node3d::to_global(
                    self.tree(),
                    nid,
                    local,
                )))
            }
            "to_local" => {
                let global = vec3_arg(args, 0)?;
                Some(Variant::Vector3(crate::node3d::to_local(
                    self.tree(),
                    nid,
                    global,
                )))
            }
            // -- CharacterBody2D motion --------------------------------------
            //
            // Godot's `move_and_slide()` moves the body inside the call. Patina
            // runs the character step in the physics phase, which is *before*
            // the script phase in the same tick (main_loop.rs), so the request
            // is recorded here and applied on the next physics tick. The
            // observable contract — "a body whose script calls move_and_slide
            // moves, one that never calls it does not" — holds either way; only
            // the timing differs by one tick.
            // -- GraphEdit connection model (pat-0q2kx.244) --
            //
            // `graph_edit.rs` is a complete 732-line model — port typing,
            // duplicate rejection, validity rules — that had NO owner, so a
            // script could not reach any of it. These three route to the
            // per-node model now stored on the tree.
            //
            // Godot returns an Error code (OK == 0) from connect_node, not a
            // bool, so a script can `if err != OK`. Mapping the model's
            // Result onto that is the whole translation.
            "connect_node" | "disconnect_node" => {
                let from = arg_str(0)?;
                let from_port = match args.get(1) {
                    Some(Variant::Int(i)) if *i >= 0 => *i as u32,
                    // ERR_INVALID_PARAMETER (core/error/error_list.h).
                    _ => return Some(Variant::Int(31)),
                };
                let to = arg_str(2)?;
                let to_port = match args.get(3) {
                    Some(Variant::Int(i)) if *i >= 0 => *i as u32,
                    // ERR_INVALID_PARAMETER (core/error/error_list.h).
                    _ => return Some(Variant::Int(31)),
                };
                let graph = self.tree_mut().graph_edit_mut(nid);
                let res = if method == "connect_node" {
                    graph.connect_node(&from, from_port, &to, to_port)
                } else {
                    graph.disconnect_node(&from, from_port, &to, to_port)
                };
                // OK == 0, ERR_INVALID_PARAMETER == 31 (core/error/error_list.h).
                Some(Variant::Int(if res.is_ok() { 0 } else { 31 }))
            }
            "get_connection_list" => {
                // Godot yields an Array of Dictionaries keyed
                // from_node/from_port/to_node/to_port. Reading through
                // `get_graph_edit` rather than `graph_edit_mut` so a pure query
                // does not allocate a model for a node that has none.
                let list = match self.tree().get_graph_edit(nid) {
                    Some(g) => g
                        .get_connection_list()
                        .iter()
                        .map(|c| {
                            let mut d = gdvariant::Dictionary::new();
                            d.insert(
                                Variant::String("from_node".into()),
                                Variant::String(c.from_node.clone()),
                            );
                            d.insert(
                                Variant::String("from_port".into()),
                                Variant::Int(c.from_port as i64),
                            );
                            d.insert(
                                Variant::String("to_node".into()),
                                Variant::String(c.to_node.clone()),
                            );
                            d.insert(
                                Variant::String("to_port".into()),
                                Variant::Int(c.to_port as i64),
                            );
                            Variant::Dictionary(d)
                        })
                        .collect(),
                    None => Vec::new(),
                };
                Some(Variant::Array(list))
            }
            "move_and_slide" => {
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property("_move_and_slide_requested", Variant::Bool(true));
                }
                // Godot returns whether the body collided during the motion.
                let tree = self.tree();
                let collided = bool_prop(tree, "is_on_floor")
                    || bool_prop(tree, "is_on_wall")
                    || bool_prop(tree, "is_on_ceiling");
                Some(Variant::Bool(collided))
            }
            // `ResourcePreloader` accessors (pat-0q2kx.218). The preloaded set
            // lives in the node's `resources` Dictionary, which is how a `.tscn`
            // authors it; `gdscene::resource_preloader::ResourcePreloader` models
            // the same mapping but nothing instantiated one per node, so these
            // read the authored property directly rather than duplicating state
            // that could drift from it.
            //
            // Gated on the class name rather than `is_class`, because ClassDB is
            // only populated by an explicit `register_*_classes()` and these must
            // work in a bare tree — and ungated they would answer `get_resource`
            // for every node in the scene.
            "get_resource" | "has_resource" | "get_resource_list"
                if self
                    .tree()
                    .get_node(nid)
                    .is_some_and(|n| n.class_name() == "ResourcePreloader") =>
            {
                let dict = match self
                    .tree()
                    .get_node(nid)
                    .map(|n| n.get_property("resources"))
                {
                    Some(Variant::Dictionary(d)) => d,
                    // No authored `resources`: an empty preloader, not an error.
                    _ => Default::default(),
                };
                match method {
                    "get_resource" => {
                        let name = arg_str(0)?;
                        Some(
                            dict.get(Variant::String(name))
                                .cloned()
                                .unwrap_or(Variant::Nil),
                        )
                    }
                    "has_resource" => {
                        let name = arg_str(0)?;
                        Some(Variant::Bool(dict.contains_key(Variant::String(name))))
                    }
                    // Godot returns a PackedStringArray of the resource names.
                    _ => Some(Variant::PackedStringArray(
                        dict.keys()
                            .map(|k| match k {
                                Variant::String(s) => s.clone(),
                                other => format!("{other:?}"),
                            })
                            .collect(),
                    )),
                }
            }
            // `CanvasItem.get_canvas_item()` — the RID standing for this node's
            // canvas item, which is what a script parents a RenderingServer
            // canvas item to (pat-0q2kx.213). Allocated on first ask and stable
            // afterwards, as Godot's is.
            "get_canvas_item" => Some(Variant::Int(
                gdscript_interop::canvas_server::node_canvas_item(node) as i64,
            )),
            // `PhysicsBody2D.test_move(motion)` — whether the motion would be
            // blocked, without moving the body (pat-0q2kx.200). Godot's real
            // signature leads with a transform; the motion is the argument that
            // decides the answer, so it is read from whichever slot holds a
            // Vector2 and a bodyless/serverless call reports "not blocked"
            // rather than a false positive.
            "test_move" => {
                let motion = args
                    .iter()
                    .find_map(|a| match a {
                        Variant::Vector2(v) => Some(*v),
                        _ => None,
                    })
                    .unwrap_or(Vector2::ZERO);
                Some(Variant::Bool(
                    self.physics2d_node_test_move(node, (motion.x, motion.y)),
                ))
            }
            // `PhysicsBody2D.add_collision_exception_with(body)` /
            // `remove_collision_exception_with(body)` (pat-s3rrv.100) — how a
            // bullet stops hitting its shooter and a rider its vehicle.
            //
            // ⚠️ Both return **void** in Godot, so both answer `Nil` here even
            // when nothing was applied. The bool the accessor returns is
            // deliberately discarded rather than surfaced: inventing a return
            // value Godot does not have would make `if a.add_collision_exception_with(b):`
            // a script that works in Patina and breaks in Godot.
            //
            // `method` is the match's own binding, so comparing it inside a
            // shared arm is sound — the two names differ only in direction.
            "add_collision_exception_with" | "remove_collision_exception_with" => {
                if let Some(other) = args.iter().find_map(|a| match a {
                    Variant::ObjectId(id) => Some(id.raw()),
                    _ => None,
                }) {
                    if method == "add_collision_exception_with" {
                        self.physics2d_node_add_collision_exception(node, other);
                    } else {
                        self.physics2d_node_remove_collision_exception(node, other);
                    }
                }
                Some(Variant::Nil)
            }
            // `RigidBody2D.get_contact_count()` — the number of CONTACTS, not
            // of bodies (pat-s3rrv.100).
            //
            // ⚠️ Godot is explicit that the two differ: "collisions between
            // parallel edges will result in two contacts (one at each end)"
            // (RigidBody2D.xml). So this is the raw list length, while
            // `get_colliding_bodies` below deduplicates — returning the same
            // number from both would be wrong for the flat-on-flat resting case
            // that is the single most common thing a body does.
            "get_contact_count" => Some(Variant::Int(body_contacts(self.tree(), nid).len() as i64)),
            // `RigidBody2D.get_colliding_bodies()` — Godot returns `Node2D[]`,
            // one entry per BODY however many points it touches at.
            "get_colliding_bodies" => {
                let mut seen: Vec<u64> = Vec::new();
                let mut out: Vec<Variant> = Vec::new();
                for rec in body_contacts(self.tree(), nid) {
                    let Variant::Dictionary(d) = rec else {
                        continue;
                    };
                    if let Some(Variant::ObjectId(id)) = d.get("collider") {
                        // Insertion order is preserved rather than sorted: it
                        // mirrors contact order, and a script indexing [0] gets
                        // a stable answer across runs.
                        if !seen.contains(&id.raw()) {
                            seen.push(id.raw());
                            out.push(Variant::ObjectId(*id));
                        }
                    }
                }
                Some(Variant::Array(out))
            }
            // Godot answers an Array of the excepted bodies — empty, never
            // null, so a script can `for b in ...` it without a guard.
            "get_collision_exceptions" => Some(Variant::Array(
                self.physics2d_node_get_collision_exceptions(node)
                    .into_iter()
                    .map(|raw| Variant::ObjectId(ObjectId::from_raw(raw)))
                    .collect(),
            )),
            // Collision state persisted by `process_character_movement`.
            "is_on_floor" | "is_on_wall" | "is_on_ceiling" => {
                Some(Variant::Bool(bool_prop(self.tree(), method)))
            }
            // `*_only` variants ignore the "was on X last frame" distinction
            // Patina does not model separately; report the same state.
            // ⚠️ The `_only` variants are NOT aliases for the plain ones —
            // that is the bug they had (pat-s3rrv.97). Godot defines each as
            // "this surface AND NEITHER of the other two"
            // (`character_body_2d.cpp:456-466`):
            //
            //     is_on_floor_only() { return on_floor && !on_wall && !on_ceiling; }
            //
            // Returning the plain flag made a body that ran into a wall while
            // standing on the ground report `is_on_floor_only() == true`, which
            // is exactly the case the method exists to rule out — so the
            // "am I cleanly on the ground?" test every platformer writes was
            // always true once grounded.
            "is_on_floor_only" | "is_on_wall_only" | "is_on_ceiling_only" => {
                let floor = bool_prop(self.tree(), "is_on_floor");
                let wall = bool_prop(self.tree(), "is_on_wall");
                let ceiling = bool_prop(self.tree(), "is_on_ceiling");
                let only = match method {
                    "is_on_floor_only" => floor && !wall && !ceiling,
                    "is_on_wall_only" => wall && !floor && !ceiling,
                    _ => ceiling && !floor && !wall,
                };
                Some(Variant::Bool(only))
            }
            // Godot's `get_real_velocity()` is the motion the last move ACHIEVED.
            //
            // ⚠️ NOT the node's `velocity`, which is what this used to return.
            // The two agree only while nothing obstructs the body: run into a
            // wall and `velocity` still carries the slide velocity for the next
            // tick while the body went nowhere along x. Reporting `velocity`
            // therefore answered "still moving" for exactly the case the method
            // exists to detect (pat-s3rrv.97).
            //
            // `process_character_movement` publishes the position delta over the
            // tick as `_real_velocity`; ZERO before the first move, which is
            // also what Godot reports for a body that has not moved yet.
            "get_real_velocity" => Some(vel_prop(self.tree(), "_real_velocity")),
            // Written by `process_character_movement` from the platform the body
            // is standing on; ZERO until then.
            "get_platform_velocity" => Some(vel_prop(self.tree(), "platform_velocity")),
            // Godot's `get_position_delta` (travel over the last
            // `move_and_slide`) and `get_last_motion` (the LAST of the several
            // motions a slide is split into) — pat-s3rrv.103.
            //
            // ⚠️ Deliberately two separate published properties rather than one
            // reused value: they coincide only when the body hit nothing, so
            // serving both from the same source reads as correct in any fixture
            // that never collides, which is most of them.
            //
            // Both exist on CharacterBody2D *and* 3D (Vector2 / Vector3), so
            // they go through the dimension-preserving reader.
            "get_position_delta" => Some(vel_prop(self.tree(), "_position_delta")),
            "get_last_motion" => Some(vel_prop(self.tree(), "_last_motion")),
            // ⚠️ 3D ONLY. Godot gives `CharacterBody3D` this method and
            // `CharacterBody2D` none at all — 2D angular velocity is a scalar,
            // so there is no Vector2 form to fall back to and this must not go
            // through `vel_prop`.
            "get_platform_angular_velocity" => Some(Variant::Vector3(
                match self
                    .tree()
                    .get_node(nid)
                    .map(|n| n.get_property("_platform_angular_velocity"))
                {
                    Some(Variant::Vector3(v)) => v,
                    _ => gdcore::math::Vector3::ZERO,
                },
            )),

            // -- RayCast2D script API (pat-0q2kx.32) --------------------------
            //
            // The interpreter already carries these in `call_method_on`, but
            // that table is only reached for a call *on another object*
            // (`$Ray.is_colliding()`). A bare `is_colliding()` — how a script on
            // the RayCast2D itself writes it, and what Godot's own docs show —
            // resolves through `call_self_func_or_native`, which tries user
            // funcs and then lands **here**. Missing from this table, those
            // names raised `UndefinedFunction` and aborted `_physics_process` at
            // the first getter, so nothing after it ran.
            //
            // Keys match what `raycast2d::store_hit` publishes.
            "is_colliding" => Some(Variant::Bool(bool_prop(self.tree(), "_hit_colliding"))),
            "get_collision_point" => Some(Variant::Vector2(vec2_prop(self.tree(), "_hit_point"))),
            "get_collision_normal" => Some(Variant::Vector2(vec2_prop(self.tree(), "_hit_normal"))),
            "get_collider" => Some(
                self.tree()
                    .get_node(nid)
                    .map(|n| n.get_property("_hit_collider"))
                    .unwrap_or(Variant::Nil),
            ),
            // Godot re-runs the cast immediately so the *same* frame sees the
            // result; the per-tick pass runs after the script phase, so without
            // this a script could never observe a hit during `_physics_process`.
            "force_raycast_update" => {
                self.force_raycast_update_2d(node);
                Some(Variant::Nil)
            }
            // -- RayCast3D / ShapeCast3D exception list (pat-s3rrv.106) -------
            //
            // `add_exception(node)` excludes a body from the cast — the standard
            // way a character stops its own raycast hitting itself. The Rust
            // modules have carried `add_exception` / `remove_exception` /
            // `clear_exceptions` since they landed; no script could reach them.
            //
            // ⚠️ Dispatched on the node's CLASS, unlike the property-backed arms
            // above (`is_colliding` and friends read `_hit_colliding`, which
            // both dimensions publish, so those are deliberately class-blind).
            // The exception list is real per-module state, so sending a
            // RayCast2D to the 3D module would silently write a list nothing
            // reads. An unrecognised class falls through to `Nil` rather than
            // guessing a module.
            "add_exception" | "remove_exception" => {
                let Some(Variant::ObjectId(oid)) = args.first() else {
                    return Some(Variant::Nil);
                };
                let excluded = NodeId::from_object_id(*oid);
                let adding = method == "add_exception";
                // `node` is a raw object id here, as `force_raycast_update_2d`
                // above also has to convert.
                let nid = NodeId::from_object_id(ObjectId::from_raw(node));
                let class = self
                    .tree()
                    .get_node(nid)
                    .map(|n| n.class_name().to_string())
                    .unwrap_or_default();
                match class.as_str() {
                    "RayCast3D" => {
                        if adding {
                            crate::raycast3d::add_exception(self.tree_mut(), nid, excluded);
                        } else {
                            crate::raycast3d::remove_exception(self.tree_mut(), nid, excluded);
                        }
                    }
                    "ShapeCast3D" => {
                        if adding {
                            crate::shapecast3d::add_exception(self.tree_mut(), nid, excluded);
                        } else {
                            crate::shapecast3d::remove_exception(self.tree_mut(), nid, excluded);
                        }
                    }
                    _ => {}
                }
                Some(Variant::Nil)
            }
            "clear_exceptions" => {
                let nid = NodeId::from_object_id(ObjectId::from_raw(node));
                let class = self
                    .tree()
                    .get_node(nid)
                    .map(|n| n.class_name().to_string())
                    .unwrap_or_default();
                match class.as_str() {
                    "RayCast3D" => crate::raycast3d::clear_exceptions(self.tree_mut(), nid),
                    "ShapeCast3D" => crate::shapecast3d::clear_exceptions(self.tree_mut(), nid),
                    _ => {}
                }
                Some(Variant::Nil)
            }

            // -- RigidBody3D force / impulse script API (pat-0q2kx.37) --------
            //
            // A body holds no handle on the PhysicsServer, so these accumulate
            // onto `_pending_*` node properties that
            // `PhysicsServer3D::sync_to_physics` drains into the world body every
            // physics tick — the Vector3 twin of the 2D `_pending_*` bridge.
            //
            // The interpreter has its own arms for the `$Body.apply_*()` form,
            // but they coerce every argument through `vec2_arg`, so a `Vector3`
            // arrived as ZERO and every 3D force call was a silent no-op. These
            // are reached by the bare / `self.` call form and discriminate on the
            // argument's own type: a non-Vector3 call declines, leaving the 2D
            // path exactly as it was.
            "apply_central_impulse"
            | "apply_impulse"
            | "apply_central_force"
            | "apply_force"
            | "apply_torque"
            | "apply_torque_impulse"
            | "add_constant_central_force"
            | "add_constant_force"
            | "add_constant_torque" => {
                let value = vec3_arg(args, 0)?;
                // `linear_key` empty means the argument *is* a torque.
                let (linear_key, torque_key): (&str, &str) = match method {
                    "apply_central_impulse" | "apply_impulse" => {
                        ("_pending_impulse", "_pending_torque_impulse")
                    }
                    "apply_central_force" | "apply_force" => ("_pending_force", "_pending_torque"),
                    "apply_torque" => ("", "_pending_torque"),
                    "apply_torque_impulse" => ("", "_pending_torque_impulse"),
                    // Godot's add_constant_* accumulate onto the real
                    // `constant_force` / `constant_torque` properties, which the
                    // server re-reads each tick rather than draining.
                    "add_constant_central_force" | "add_constant_force" => {
                        ("constant_force", "constant_torque")
                    }
                    _ => ("", "constant_torque"),
                };
                if linear_key.is_empty() {
                    accumulate_vec3_prop(self.tree_mut(), nid, torque_key, value);
                } else {
                    accumulate_vec3_prop(self.tree_mut(), nid, linear_key, value);
                    // A positioned force/impulse also induces torque about the
                    // centre of mass: τ = r × F (Godot takes `position` in global
                    // space relative to the body's centre of mass).
                    if let Some(offset) = vec3_arg(args, 1) {
                        accumulate_vec3_prop(self.tree_mut(), nid, torque_key, offset.cross(value));
                    }
                }
                Some(Variant::Nil)
            }

            // -- Area3D / Area2D overlap queries (pat-0q2kx.35) ---------------
            //
            // The physics phase leaves the current overlap set on the node as
            // `_overlapping_bodies` (see
            // `PhysicsServer3D::publish_overlapping_bodies`) because the script
            // phase holds no handle on the physics server — the same bridge
            // RayCast2D uses for `_hit_normal`. Godot returns an Array of nodes,
            // empty (never null) when nothing overlaps.
            // -- shape owners (pat-s3rrv.25) --
            //
            // Godot's `CollisionObject2D` owns a list of shape owners, each with
            // its own transform. Patina models one collider child as one owner
            // holding one shape, so owner id == the index picking reports as
            // `shape_idx` and `shape_owner_get_shape_count` is always 1.
            //
            // Answered from the SCENE rather than the physics world: shape owners
            // are the collider children, so this is correct even for a node that
            // has not been registered with the server yet — and the script phase
            // holds no physics handle anyway.
            "get_shape_owners" => {
                let count =
                    crate::physics_server::collider_local_transforms(self.tree(), nid).len();
                Some(Variant::Array(
                    (0..count).map(|i| Variant::Int(i as i64)).collect(),
                ))
            }
            "shape_owner_get_shape_count" => {
                // One shape per owner in Patina's model. Godot allows several,
                // so this returns 1 rather than the body's total — a script
                // iterating `0..shape_owner_get_shape_count(id)` must see one
                // shape per owner, not every shape on the object.
                let owners = crate::physics_server::collider_local_transforms(self.tree(), nid);
                let idx = arg_f32(args, 0)?.max(0.0) as usize;
                Some(Variant::Int(if idx < owners.len() { 1 } else { 0 }))
            }
            "shape_owner_get_transform" => {
                let owners = crate::physics_server::collider_local_transforms(self.tree(), nid);
                let idx = arg_f32(args, 0)?.max(0.0) as usize;
                Some(match owners.get(idx) {
                    Some(tf) => Variant::Transform2D(*tf),
                    None => Variant::Transform2D(gdcore::math::Transform2D::IDENTITY),
                })
            }
            "get_overlapping_bodies" => Some(match self.tree().get_node(nid) {
                Some(n) => match n.get_property("_overlapping_bodies") {
                    Variant::Array(items) => Variant::Array(items),
                    _ => Variant::Array(Vec::new()),
                },
                None => Variant::Array(Vec::new()),
            }),
            "has_overlapping_bodies" => {
                let count = match self.tree().get_node(nid) {
                    Some(n) => match n.get_property("_overlapping_bodies") {
                        Variant::Array(items) => items.len(),
                        _ => 0,
                    },
                    None => 0,
                };
                Some(Variant::Bool(count > 0))
            }

            // The area-vs-area half of the same surface (pat-0q2kx.112). Only
            // the bodies half existed, so `get_overlapping_areas()` — Godot's
            // usual way to ask "which triggers am I inside?" — was unreachable
            // from script. `_overlapping_areas` is published by
            // `PhysicsServer::publish_overlapping_areas` alongside the bodies
            // set, so both answer from the same frame.
            "get_overlapping_areas" => Some(match self.tree().get_node(nid) {
                Some(n) => match n.get_property("_overlapping_areas") {
                    Variant::Array(items) => Variant::Array(items),
                    _ => Variant::Array(Vec::new()),
                },
                None => Variant::Array(Vec::new()),
            }),
            "has_overlapping_areas" => {
                let count = match self.tree().get_node(nid) {
                    Some(n) => match n.get_property("_overlapping_areas") {
                        Variant::Array(items) => items.len(),
                        _ => 0,
                    },
                    None => 0,
                };
                Some(Variant::Bool(count > 0))
            }

            // Godot's `overlaps_body(node)` / `overlaps_area(node)` answer from
            // the same live set rather than recomputing, so these are membership
            // tests over the published arrays. A missing or non-Object argument
            // is `false`, as Godot returns for a node that is not overlapping.
            "overlaps_body" | "overlaps_area" => {
                let key = if method == "overlaps_body" {
                    "_overlapping_bodies"
                } else {
                    "_overlapping_areas"
                };
                let target = match args.first() {
                    Some(Variant::ObjectId(id)) => *id,
                    _ => return Some(Variant::Bool(false)),
                };
                let found = match self.tree().get_node(nid) {
                    Some(n) => match n.get_property(key) {
                        Variant::Array(items) => items
                            .iter()
                            .any(|v| matches!(v, Variant::ObjectId(id) if *id == target)),
                        _ => false,
                    },
                    None => false,
                };
                Some(Variant::Bool(found))
            }

            // -- Control keyboard focus (pat-0q2kx.84) ------------------------
            // The viewport owns focus, so these are the only way a script can
            // ask "am I focused?" or move focus itself — a UI script that calls
            // `grab_focus()` in `_ready` is the standard Godot idiom.
            "has_focus" => Some(Variant::Bool(self.tree().has_focus(nid))),
            "grab_focus" => {
                self.tree_mut().grab_focus(nid);
                Some(Variant::Nil)
            }
            // Godot's `Control.release_focus` only drops focus when the caller
            // is the one holding it; releasing from an unfocused control must
            // not steal focus away from whoever has it.
            "release_focus" => {
                if self.tree().has_focus(nid) {
                    self.tree_mut().release_focus();
                }
                Some(Variant::Nil)
            }

            // -- Node groups --------------------------------------------------
            "add_to_group" => {
                let group = arg_str(0)?;
                let _ = self.tree_mut().add_to_group(nid, &group);
                Some(Variant::Nil)
            }
            "remove_from_group" => {
                let group = arg_str(0)?;
                let _ = self.tree_mut().remove_from_group(nid, &group);
                Some(Variant::Nil)
            }
            "is_in_group" => {
                let group = arg_str(0)?;
                Some(Variant::Bool(
                    self.tree()
                        .get_node(nid)
                        .map(|n| n.groups().contains(&group))
                        .unwrap_or(false),
                ))
            }
            // Godot puts this on SceneTree; accepting it on a node too costs
            // nothing and matches how scripts reach for it.
            "get_nodes_in_group" => {
                let group = arg_str(0)?;
                let ids: Vec<Variant> = self
                    .tree()
                    .get_nodes_in_group(&group)
                    .into_iter()
                    .map(|n| Variant::ObjectId(n.object_id()))
                    .collect();
                Some(Variant::Array(ids))
            }

            // -- Node tree queries --------------------------------------------
            "find_child" => {
                let pattern = arg_str(0)?;
                // Godot's signature is find_child(pattern, recursive = true, ...).
                let recursive = !matches!(args.get(1), Some(Variant::Bool(false)));
                Some(match self.tree().find_child(nid, &pattern, recursive) {
                    Some(found) => Variant::ObjectId(found.object_id()),
                    None => Variant::Nil,
                })
            }
            "has_signal" => {
                let name = arg_str(0)?;
                let declared = self
                    .tree()
                    .signal_store(nid)
                    .map(|s| s.has_signal(&name))
                    .unwrap_or(false);
                // A signal the class declares counts even with nothing connected.
                let on_class = self
                    .tree()
                    .get_node(nid)
                    .map(|n| gdobject::class_db::class_has_signal(n.class_name(), &name))
                    .unwrap_or(false);
                Some(Variant::Bool(declared || on_class))
            }
            // `other.emit_signal("died", 5)` — emitting on ANOTHER node.
            //
            // The self form is a script builtin (`try_builtin`), but nothing
            // dispatched the cross-node form: it is not on the ObjectId method
            // path and was not in this native table, so `$Enemy.emit_signal(..)`
            // failed even though `$Enemy.has_signal(..)` answered (pat-0q2kx.16,
            // whose implement list names `emit_signal via SceneTree`).
            "emit_signal" => {
                let name = arg_str(0)?;
                // Godot passes everything after the signal name as arguments.
                let signal_args: Vec<Variant> = args.iter().skip(1).cloned().collect();
                self.tree_mut().emit_signal(nid, &name, &signal_args);
                // Godot returns an Error code; OK is 0.
                Some(Variant::Int(0))
            }

            // -- CanvasItem custom drawing (pat-0q2kx.82) --------------------
            //
            // Godot's `_draw` contract: `queue_redraw()` marks the item dirty,
            // and the `draw_*` calls are only meaningful inside the `_draw` body
            // that the resulting redraw invokes (canvas_item.cpp). The commands
            // land in the tree's capture buffer, which
            // `SceneTree::flush_pending_redraws` opens around each `_draw` — a
            // `draw_rect` from `_process` is dropped, exactly as Godot drops it.
            "queue_redraw" => {
                self.tree_mut().queue_redraw(nid);
                Some(Variant::Nil)
            }
            "is_queued_for_redraw" => Some(Variant::Bool(self.tree().is_queued_for_redraw(nid))),
            // draw_rect(rect, color, filled = true, width = -1.0)
            // draw_style_box(stylebox, rect) — CanvasItem's themed-box draw
            // (pat-s3rrv.215).
            //
            // 🔑 It emits through the SAME `StyleBox::draw` the themed control
            // path calls, so a script-drawn box and a theme-drawn one cannot
            // diverge. Reimplementing the geometry here is the obvious shortcut
            // and it is how the two silently drift apart.
            //
            // The argument arrives as a `Variant::Resource`, which is what
            // `load("res://box.tres")` returns — it carries the file's real
            // class name and property bag, which is exactly what
            // `stylebox_from_props` consumes.
            "draw_style_box" => {
                let rect = arg_rect2(args, 1)?;
                let commands = match args.first() {
                    Some(Variant::Resource(res)) => {
                        crate::stylebox::stylebox_from_props(&res.class_name, &res.properties)
                            .map(|sb| sb.draw(rect))
                    }
                    _ => None,
                };
                // ⚠️ An unrecognised or absent resource draws NOTHING rather
                // than falling back to a default box. Godot's `draw_style_box`
                // with a null StyleBox is a no-op, and inventing a grey panel
                // here would make a broken resource reference look like a
                // deliberate one.
                for command in commands.unwrap_or_default() {
                    self.tree_mut().push_draw_command(command);
                }
                Some(Variant::Nil)
            }
            "draw_rect" => {
                let rect = arg_rect2(args, 0)?;
                let color = arg_color(args, 1).unwrap_or(Color::WHITE);
                let filled = !matches!(args.get(2), Some(Variant::Bool(false)));
                self.tree_mut().push_draw_command(DrawCommand::DrawRect {
                    rect,
                    color,
                    filled,
                });
                Some(Variant::Nil)
            }
            // draw_line(from, to, color, width = -1.0)
            "draw_line" => {
                let from = arg_vec2(args, 0)?;
                let to = arg_vec2(args, 1)?;
                let color = arg_color(args, 2).unwrap_or(Color::WHITE);
                // Godot's -1 width means "thinnest line"; the rasterizer wants a
                // positive pixel width, so a negative/absent width becomes 1.
                let width = match arg_f32(args, 3) {
                    Some(w) if w > 0.0 => w,
                    _ => 1.0,
                };
                self.tree_mut().push_draw_command(DrawCommand::DrawLine {
                    from,
                    to,
                    color,
                    width,
                });
                Some(Variant::Nil)
            }
            // draw_circle(position, radius, color, filled = true)
            "draw_circle" => {
                let center = arg_vec2(args, 0)?;
                let radius = arg_f32(args, 1)?;
                let color = arg_color(args, 2).unwrap_or(Color::WHITE);
                self.tree_mut().push_draw_command(DrawCommand::DrawCircle {
                    center,
                    radius,
                    color,
                });
                Some(Variant::Nil)
            }
            // -- stroke / fill family (pat-0q2kx.129) ------------------------
            //
            // These reach `polygon_raster`, which had the scanline fill and the
            // capped/jointed stroke implemented and unit-tested but no caller
            // anywhere — so a script could only ever draw rects, circles and
            // single lines.
            //
            // draw_polyline(points, color, width = -1.0, antialiased = false)
            "draw_polyline" | "draw_polyline_colors" => {
                let points = arg_vec2_array(args, 0)?;
                // `draw_polyline_colors` takes a PackedColorArray in slot 1;
                // the software path has no per-vertex interpolation, so the
                // first color applies to the whole stroke.
                let color = arg_color(args, 1)
                    .or_else(|| arg_first_color(args, 1))
                    .unwrap_or(Color::WHITE);
                self.tree_mut()
                    .push_draw_command(DrawCommand::DrawPolyline {
                        points,
                        color,
                        width: positive_width(args, 2),
                        closed: false,
                    });
                Some(Variant::Nil)
            }
            // draw_multiline(points, color, width = -1.0)
            "draw_multiline" | "draw_multiline_colors" => {
                let points = arg_vec2_array(args, 0)?;
                let color = arg_color(args, 1)
                    .or_else(|| arg_first_color(args, 1))
                    .unwrap_or(Color::WHITE);
                self.tree_mut()
                    .push_draw_command(DrawCommand::DrawMultiline {
                        points,
                        color,
                        width: positive_width(args, 2),
                    });
                Some(Variant::Nil)
            }
            // draw_dashed_line(from, to, color, width = -1.0, dash = 2.0)
            "draw_dashed_line" => {
                let from = arg_vec2(args, 0)?;
                let to = arg_vec2(args, 1)?;
                let color = arg_color(args, 2).unwrap_or(Color::WHITE);
                let dash = match arg_f32(args, 4) {
                    Some(d) if d > 0.0 => d,
                    _ => 2.0,
                };
                self.tree_mut()
                    .push_draw_command(DrawCommand::DrawDashedLine {
                        from,
                        to,
                        color,
                        width: positive_width(args, 3),
                        dash,
                    });
                Some(Variant::Nil)
            }
            // draw_arc(center, radius, start_angle, end_angle, point_count,
            //          color, width = -1.0, antialiased = false)
            "draw_arc" => {
                let center = arg_vec2(args, 0)?;
                let radius = arg_f32(args, 1)?;
                let start_angle = arg_f32(args, 2)?;
                let end_angle = arg_f32(args, 3)?;
                let point_count = match args.get(4) {
                    Some(Variant::Int(i)) if *i > 1 => *i as u32,
                    Some(Variant::Float(f)) if *f > 1.0 => *f as u32,
                    _ => 24,
                };
                let color = arg_color(args, 5).unwrap_or(Color::WHITE);
                self.tree_mut().push_draw_command(DrawCommand::DrawArc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                    point_count,
                    color,
                    width: positive_width(args, 6),
                });
                Some(Variant::Nil)
            }
            // draw_polygon(points, colors, uvs = [], texture = null)
            // draw_colored_polygon(points, color, uvs = [], texture = null)
            "draw_polygon" | "draw_colored_polygon" => {
                let points = arg_vec2_array(args, 0)?;
                // `draw_polygon` takes an array of colors, `draw_colored_polygon`
                // a single one — accept either shape in slot 1.
                let colors = match arg_color(args, 1) {
                    Some(c) => vec![c],
                    None => {
                        let list = arg_color_array(args, 1);
                        if list.is_empty() {
                            vec![Color::WHITE]
                        } else {
                            list
                        }
                    }
                };
                self.tree_mut()
                    .push_draw_command(DrawCommand::DrawPolygon { points, colors });
                Some(Variant::Nil)
            }
            // draw_texture(texture, position, modulate = WHITE). Patina's
            // DrawCommand addresses textures by path, so the `texture` argument
            // is the resource path a script would `preload`; the destination
            // rect is the texture's own size, which the renderer resolves, so a
            // zero-size rect at `position` stands for "natural size".
            "draw_texture" => {
                let path = arg_str(0)?;
                let position = arg_vec2(args, 1)?;
                let modulate = arg_color(args, 2).unwrap_or(Color::WHITE);
                self.tree_mut()
                    .push_draw_command(DrawCommand::DrawTextureRect {
                        texture_path: path,
                        rect: Rect2::new(position, Vector2::ZERO),
                        modulate,
                    });
                Some(Variant::Nil)
            }
            // draw_texture_rect(texture, rect, tile, modulate = WHITE)
            "draw_texture_rect" => {
                let path = arg_str(0)?;
                let rect = arg_rect2(args, 1)?;
                let modulate = arg_color(args, 3).unwrap_or(Color::WHITE);
                self.tree_mut()
                    .push_draw_command(DrawCommand::DrawTextureRect {
                        texture_path: path,
                        rect,
                        modulate,
                    });
                Some(Variant::Nil)
            }
            // draw_string(font, pos, text, alignment, width, font_size, modulate)
            // — the positional signature from Godot's CanvasItem. `font` and the
            // alignment/width layout arguments have no representation in
            // `DrawCommand::DrawString`, so they are accepted and ignored rather
            // than making the call fail.
            "draw_string" => {
                let position = arg_vec2(args, 1)?;
                let text = match args.get(2) {
                    Some(Variant::String(s)) => s.clone(),
                    Some(Variant::StringName(s)) => s.as_str().to_string(),
                    _ => return Some(Variant::Nil),
                };
                let font_size = match args.get(5) {
                    Some(Variant::Int(n)) if *n > 0 => *n as u32,
                    // Godot's default `font_size` is 16.
                    _ => 16,
                };
                let color = arg_color(args, 6).unwrap_or(Color::WHITE);
                self.tree_mut().push_draw_command(DrawCommand::DrawString {
                    text,
                    position,
                    color,
                    font_size,
                });
                Some(Variant::Nil)
            }

            // -- HTTPRequest (pat-s3rrv.78) ----------------------------------
            //
            // ⚠️ `HTTPRequest` was registered in ClassDB with a 602-line
            // transport behind it and NO route from a script to either. A
            // `.tscn` node of that type loaded, answered `get_class()`
            // correctly, and did nothing — which is worse than being absent,
            // because everything except the behaviour says it works.
            "request" if is_http_request(self.tree(), nid) => {
                let url = arg_str(0)?;
                // Godot: request(url, custom_headers, method, request_data).
                // The method ordinal is argument 2 (`http_client.h:118-126`).
                let method_ord = match args.get(2) {
                    Some(Variant::Int(m)) => *m,
                    _ => 0,
                };
                let Some(method) = crate::http_request_node::method_from_int(method_ord) else {
                    // ERR_UNAVAILABLE. OPTIONS/TRACE/CONNECT have no transport
                    // variant, and silently substituting GET would look like a
                    // successful request while doing something else entirely.
                    return Some(Variant::Int(2));
                };
                let body = match args.get(3) {
                    Some(Variant::String(s)) => s.clone().into_bytes(),
                    Some(Variant::PackedByteArray(b)) => b.clone(),
                    _ => Vec::new(),
                };
                // ⚠️ Resolved BEFORE the mutable borrow below; reading the
                // tree inside the `http_request_for` call is E0502. The proxies
                // are read here for exactly the same reason.
                let settings = http_settings(self.tree(), nid);
                let proxies = http_proxies(self.tree(), nid);
                let headers = custom_headers_arg(args);
                let code = self
                    .tree_mut()
                    .http_request_for(nid)
                    .request_with_proxies(&url, method, body, &headers, settings, &proxies);
                Some(Variant::Int(code as i64))
            }
            // `request_raw` differs only in taking the body as bytes, which the
            // arm above already accepts.
            "request_raw" if is_http_request(self.tree(), nid) => {
                let url = arg_str(0)?;
                let method_ord = match args.get(2) {
                    Some(Variant::Int(m)) => *m,
                    _ => 0,
                };
                let Some(method) = crate::http_request_node::method_from_int(method_ord) else {
                    return Some(Variant::Int(2));
                };
                let body = match args.get(3) {
                    Some(Variant::PackedByteArray(b)) => b.clone(),
                    Some(Variant::String(s)) => s.clone().into_bytes(),
                    _ => Vec::new(),
                };
                // ⚠️ Resolved BEFORE the mutable borrow below; reading the
                // tree inside the `http_request_for` call is E0502. The proxies
                // are read here for exactly the same reason.
                let settings = http_settings(self.tree(), nid);
                let proxies = http_proxies(self.tree(), nid);
                let headers = custom_headers_arg(args);
                let code = self
                    .tree_mut()
                    .http_request_for(nid)
                    .request_with_proxies(&url, method, body, &headers, settings, &proxies);
                Some(Variant::Int(code as i64))
            }
            // ⚠️ `cancel_request` returns VOID and emits nothing. The
            // capability has existed on the node the whole time
            // (`HttpRequestNode::cancel`, which consumes the transport's
            // recorded failure so no late `request_completed` fires, matching
            // `http_request.cpp:186-210`) with no route from a script to reach
            // it — a cancel button in a game could not cancel anything.
            // 🔑 Registered in ClassDB but, until now, with no arm here — so
            // `has_method` reported true and the call returned `Nil`. A script
            // comparing the result against `HTTPClient.STATUS_DISCONNECTED` got
            // `false` even while idle, which is the failure mode this bead exists
            // to remove: it looks supported and quietly is not.
            //
            // ⚠️ Only two of the ten statuses are reachable, and that is the
            // transport's shape rather than a shortcut here — see
            // `http_request_node::STATUS_REQUESTING`. Both values reported are
            // ones Godot really produces, and both are correct at their endpoint.
            "get_http_client_status" if is_http_request(self.tree(), nid) => {
                // ⚠️ Guarded on `has_http_request` so a GETTER does not lazily
                // create per-node transport state. `http_request_for` is
                // `entry().or_default()`, so calling it unguarded would allocate
                // an HttpRequestNode for every node a script merely polls.
                let requesting = if self.tree().has_http_request(nid) {
                    self.tree_mut().http_request_for(nid).is_requesting()
                } else {
                    false
                };
                Some(Variant::Int(if requesting {
                    crate::http_request_node::STATUS_REQUESTING
                } else {
                    crate::http_request_node::STATUS_DISCONNECTED
                }))
            }
            "cancel_request" if is_http_request(self.tree(), nid) => {
                self.tree_mut().http_request_for(nid).cancel();
                Some(Variant::Nil)
            }
            // Godot exposes `timeout` and `max_redirects` as properties; these
            // are the bound setter/getter names, and they read and write the
            // same node properties the `.tscn` does, so `r.timeout = 5.0` and
            // `r.set_timeout(5.0)` cannot disagree.
            "set_timeout" if is_http_request(self.tree(), nid) => {
                let requested = match args.first() {
                    Some(Variant::Float(f)) => *f,
                    Some(Variant::Int(i)) => *i as f64,
                    _ => return Some(Variant::Nil),
                };
                let current = http_settings(self.tree(), nid).timeout;
                // Negative is rejected, not clamped — see `accepted_timeout`.
                let value = crate::http_request_node::accepted_timeout(current, requested);
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property("timeout", Variant::Float(value));
                }
                Some(Variant::Nil)
            }
            "get_timeout" if is_http_request(self.tree(), nid) => {
                Some(Variant::Float(http_settings(self.tree(), nid).timeout))
            }
            // 🔑 Setters only — Godot exposes NO getter and no property for
            // either proxy (`doc/classes/HTTPRequest.xml:209-225`). Every other
            // pair on this node has a `get_*` twin, so adding one here by
            // symmetry would invent API upstream does not have.
            //
            // ⚠️ The first HTTPRequest arms that take a SECOND argument, which
            // makes the missing-arg default load-bearing: a bare
            // `set_http_proxy("host")` must fall back to `-1` (= unset), never
            // to `0`. Port 0 is a real value the "unset" rule does not cover
            // (`HTTPRequest.xml:215`, `http_client_tcp.cpp:771` — the test is
            // `port == -1` exactly), so defaulting to 0 would leave the node
            // holding a live proxy the script never asked for.
            "set_http_proxy" | "set_https_proxy" if is_http_request(self.tree(), nid) => {
                let host = match args.first() {
                    Some(Variant::String(s)) => s.clone(),
                    _ => String::new(),
                };
                let port = match args.get(1) {
                    Some(Variant::Int(p)) => *p,
                    _ => -1,
                };
                // Matched by name rather than `if/else`, following the
                // `apply_*`/`add_constant_*` arm at :3541. ⚠️ Not cosmetic: an
                // `else` would silently classify any third name later added to
                // the pattern above as the HTTPS proxy.
                let (host_key, port_key): (&str, &str) = match method {
                    "set_http_proxy" => ("http_proxy_host", "http_proxy_port"),
                    "set_https_proxy" => ("https_proxy_host", "https_proxy_port"),
                    _ => return Some(Variant::Nil),
                };
                // Stored verbatim; the "unset" rule is applied once, on read,
                // by `http_request_node::proxy_settings`. Normalising here as
                // well would give two places to disagree.
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property(host_key, Variant::String(host));
                    n.set_property(port_key, Variant::Int(port));
                }
                Some(Variant::Nil)
            }
            "set_max_redirects" if is_http_request(self.tree(), nid) => {
                let Some(Variant::Int(amount)) = args.first() else {
                    return Some(Variant::Nil);
                };
                // ⚠️ Stored verbatim, negatives included: a negative means
                // UNLIMITED (`http_request.cpp:255`), not "none".
                let amount = *amount;
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property("max_redirects", Variant::Int(amount));
                }
                Some(Variant::Nil)
            }
            "get_max_redirects" if is_http_request(self.tree(), nid) => {
                Some(Variant::Int(http_settings(self.tree(), nid).max_redirects))
            }
            // `body_size_limit` and `download_file` shape what the completion
            // DELIVERS rather than what the request sends, so they are read at
            // emit time by `SceneTree::process_http_requests`. These arms only
            // have to store them where that reader looks — the node itself.
            "set_body_size_limit" if is_http_request(self.tree(), nid) => {
                let Some(Variant::Int(bytes)) = args.first() else {
                    return Some(Variant::Nil);
                };
                // ⚠️ Stored verbatim, negatives included: negative is NO LIMIT
                // (`http_request.cpp:408`), and the default is -1.
                let bytes = *bytes;
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property("body_size_limit", Variant::Int(bytes));
                }
                Some(Variant::Nil)
            }
            "get_body_size_limit" if is_http_request(self.tree(), nid) => Some(Variant::Int(
                http_delivery(self.tree(), nid).body_size_limit,
            )),
            // pat-s3rrv.237. Godot: `set_tls_options(client_options)`, void.
            //
            // Stored rather than acted on, and that is the correct behaviour
            // here rather than a stub: on web the browser owns the TLS
            // handshake through fetch, so these are configuration a script sets
            // and reads back, not something this layer applies. Accepting it is
            // the whole point — `TLSOptions` is a real class as of this bead,
            // and a script passing one to an HTTPRequest must not error.
            //
            // The argument arrives as a `Variant::ObjectId` (TLSOptions is a
            // handle-table object in gdscript-interop), so it is stored
            // verbatim; unwrapping it here would just lose the identity.
            "set_tls_options" if is_http_request(self.tree(), nid) => {
                let opts = args.first().cloned().unwrap_or(Variant::Nil);
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property("tls_options", opts);
                }
                Some(Variant::Nil)
            }
            // ⚠️ No `get_tls_options` arm on purpose. Godot's HTTPRequest has
            // `set_tls_options` and no getter — no method, no `tls_options`
            // member — so adding one would invent API in a parity port. The
            // stored property is engine-side state a test can read, not a script
            // surface.
            "set_download_file" if is_http_request(self.tree(), nid) => {
                let Some(Variant::String(path)) = args.first() else {
                    return Some(Variant::Nil);
                };
                let path = path.clone();
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property("download_file", Variant::String(path));
                }
                Some(Variant::Nil)
            }
            "get_download_file" if is_http_request(self.tree(), nid) => Some(Variant::String(
                http_delivery(self.tree(), nid).download_file,
            )),
            "set_accept_gzip" if is_http_request(self.tree(), nid) => {
                let Some(Variant::Bool(enable)) = args.first() else {
                    return Some(Variant::Nil);
                };
                let enable = *enable;
                if let Some(n) = self.tree_mut().get_node_mut(nid) {
                    n.set_property("accept_gzip", Variant::Bool(enable));
                }
                Some(Variant::Nil)
            }
            // ⚠️ The getter is `is_accepting_gzip`, not `get_accept_gzip`
            // (`HTTPRequest.xml:236` names it as the property's getter). A
            // script calling the name the setter implies would miss.
            "is_accepting_gzip" if is_http_request(self.tree(), nid) => {
                Some(Variant::Bool(http_delivery(self.tree(), nid).accept_gzip))
            }
            // -- RichTextLabel text API (pat-s3rrv.74) -----------------------
            //
            // Godot builds rich text imperatively as well as declaratively;
            // without these a script could only ever set the whole `bbcode_text`
            // string, so nothing could append a line to a log or a dialogue box —
            // the two things a RichTextLabel is usually for.
            //
            // All of them work on the same source string the draw arm reads, so
            // a script append shows up in the very next frame with no separate
            // model to fall out of sync.
            // Godot's push/pop TAG STACK (pat-s3rrv.74). `push_color(c)` opens a
            // run, `pop()` closes the most recent one — the API a log or chat
            // box uses to build a line without hand-writing BBCode.
            //
            // 🔑 These emit into the SAME source string `append_text` appends
            // to, which is the whole reason the stack needs no parallel
            // document model: the draw arm re-parses that string every frame,
            // so a push shows up in the next frame like any other edit.
            "push_color" if is_rich_text_label(self.tree(), nid) => {
                let c = match args.first() {
                    Some(Variant::Color(c)) => *c,
                    _ => return Some(Variant::Nil),
                };
                let open = format!("[color={}]", richtext_color_hex(c));
                richtext_push(self.tree_mut(), nid, &open, "[/color]");
                Some(Variant::Nil)
            }
            "push_bgcolor" if is_rich_text_label(self.tree(), nid) => {
                let c = match args.first() {
                    Some(Variant::Color(c)) => *c,
                    _ => return Some(Variant::Nil),
                };
                let open = format!("[bgcolor={}]", richtext_color_hex(c));
                richtext_push(self.tree_mut(), nid, &open, "[/bgcolor]");
                Some(Variant::Nil)
            }
            "push_fgcolor" if is_rich_text_label(self.tree(), nid) => {
                let c = match args.first() {
                    Some(Variant::Color(c)) => *c,
                    _ => return Some(Variant::Nil),
                };
                let open = format!("[fgcolor={}]", richtext_color_hex(c));
                richtext_push(self.tree_mut(), nid, &open, "[/fgcolor]");
                Some(Variant::Nil)
            }
            "push_underline" if is_rich_text_label(self.tree(), nid) => {
                richtext_push(self.tree_mut(), nid, "[u]", "[/u]");
                Some(Variant::Nil)
            }
            "push_strikethrough" if is_rich_text_label(self.tree(), nid) => {
                richtext_push(self.tree_mut(), nid, "[s]", "[/s]");
                Some(Variant::Nil)
            }
            // `push_meta(v)` is what makes a run clickable — the payload
            // `meta_clicked` carries. The parser models it as `[url=...]`.
            "push_meta" if is_rich_text_label(self.tree(), nid) => {
                let Some(meta) = args.first().and_then(variant_string) else {
                    return Some(Variant::Nil);
                };
                let open = format!("[url={meta}]");
                richtext_push(self.tree_mut(), nid, &open, "[/url]");
                Some(Variant::Nil)
            }
            // Godot's `pop()` returns nothing and is a no-op on an empty stack,
            // so an unbalanced script misbehaves rather than crashing — the
            // same forgiveness the engine gives.
            "pop" if is_rich_text_label(self.tree(), nid) => {
                richtext_pop(self.tree_mut(), nid);
                Some(Variant::Nil)
            }
            "pop_all" if is_rich_text_label(self.tree(), nid) => {
                while richtext_pop(self.tree_mut(), nid) {}
                Some(Variant::Nil)
            }
            "append_text" if is_rich_text_label(self.tree(), nid) => {
                let extra = arg_str(0)?;
                let joined = format!("{}{}", richtext_src(self.tree(), nid), extra);
                set_richtext(self.tree_mut(), nid, joined);
                Some(Variant::Nil)
            }
            // ⚠️ `add_text` adds RAW text: "Adds raw non-BBCode-parsed text to the
            // tag stack" (RichTextLabel.xml). Appending it unescaped would let a
            // stray `[` in user data — a chat line, a filename — be read as a
            // tag and silently swallow the rest of the string. The brackets are
            // escaped with Godot's own `[lb]`/`[rb]`, which the parser now
            // understands, so the text round-trips through `get_parsed_text`.
            "add_text" if is_rich_text_label(self.tree(), nid) => {
                let raw = arg_str(0)?;
                // ⚠️ ONE pass, not two chained `replace`s. Escaping `[` emits a
                // `]` of its own, so a following `replace(']', "[rb]")` mangles
                // the escape just written — `[lb]` becomes `[lb[rb]`, an unknown
                // tag that is then stripped, and the bracket the user typed
                // disappears entirely. No ordering of two replaces avoids it,
                // because each one emits both characters the other rewrites.
                let escaped: String = raw.chars().fold(String::new(), |mut acc, c| {
                    match c {
                        '[' => acc.push_str("[lb]"),
                        ']' => acc.push_str("[rb]"),
                        other => acc.push(other),
                    }
                    acc
                });
                let joined = format!("{}{}", richtext_src(self.tree(), nid), escaped);
                set_richtext(self.tree_mut(), nid, joined);
                Some(Variant::Nil)
            }
            "clear" if is_rich_text_label(self.tree(), nid) => {
                set_richtext(self.tree_mut(), nid, String::new());
                Some(Variant::Nil)
            }
            "get_parsed_text" if is_rich_text_label(self.tree(), nid) => Some(Variant::String(
                crate::control_render::richtext_parsed_text(self.tree(), nid),
            )),
            "get_total_character_count" if is_rich_text_label(self.tree(), nid) => {
                // Counts PARSED characters, so markup never inflates the total —
                // this is the denominator a typewriter effect drives
                // `visible_characters` against.
                let n = crate::control_render::richtext_parsed_text(self.tree(), nid)
                    .chars()
                    .count();
                Some(Variant::Int(n as i64))
            }

            // -- LineEdit / TextEdit editing (pat-0q2kx.87) ------------------
            //
            // Registered in ClassDB, so without these arms `node_has_method`
            // would answer true and the call would return the Nil placeholder —
            // a method that exists and does nothing.
            "select_all" if crate::line_edit::is_text_input(self.tree(), nid) => {
                crate::line_edit::select_all(self.tree_mut(), nid);
                Some(Variant::Nil)
            }
            "deselect" if crate::line_edit::is_text_input(self.tree(), nid) => {
                crate::line_edit::deselect(self.tree_mut(), nid);
                Some(Variant::Nil)
            }
            "has_selection" if crate::line_edit::is_text_input(self.tree(), nid) => Some(
                Variant::Bool(crate::line_edit::has_selection(self.tree(), nid)),
            ),
            "get_selected_text" if crate::line_edit::is_text_input(self.tree(), nid) => Some(
                Variant::String(crate::line_edit::selected_text(self.tree(), nid)),
            ),
            "insert_text_at_caret" if crate::line_edit::is_text_input(self.tree(), nid) => {
                let insert = arg_str(0)?;
                crate::line_edit::insert_text_at_caret(self.tree_mut(), nid, &insert);
                Some(Variant::Nil)
            }

            // `Control.accept_event()` — Godot's shorthand for
            // `get_viewport().set_input_as_handled()` (scene/gui/control.cpp),
            // and the documented way a `_gui_input` handler stops an event from
            // bubbling to ancestor Controls and the later input stages.
            "accept_event" => {
                self.tree_mut().set_input_as_handled();
                Some(Variant::Nil)
            }

            // The item-view Controls (pat-0q2kx.85). Their methods are
            // class-specific — `select` means one thing on an ItemList and
            // another on an OptionButton — so they dispatch on the node's class
            // inside `item_views` rather than adding a dozen arms here. Last,
            // so a method named above keeps its meaning for every class.
            _ => crate::item_views::call_item_view_method(self.tree_mut(), nid, method, args),
        }
    }

    // -- Input propagation (pat-nzgdu.83) -----------------------------------

    fn set_input_as_handled(&mut self) {
        self.tree_mut().set_input_as_handled();
    }

    fn is_input_handled(&self) -> bool {
        self.tree().is_input_handled()
    }

    // -- Per-node process toggles (pat-nzgdu.137) ---------------------------

    fn set_process(&mut self, node: u64, enabled: bool) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().set_node_process(nid, enabled);
    }

    fn is_processing(&self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().is_node_processing(nid)
    }

    fn set_physics_process(&mut self, node: u64, enabled: bool) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().set_node_physics_process(nid, enabled);
    }

    fn set_process_input(&mut self, node: u64, enabled: bool) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().set_node_process_input(nid, enabled);
    }

    fn is_processing_input(&self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().is_node_processing_input(nid)
    }

    // -- PhysicsServer2D (pat-0q2kx.200) ------------------------------------
    //
    // Each of these routes to the `PhysicsServer` bound for this script phase.
    // With no server bound they fall through to the trait defaults' behaviour
    // (invalid RID / no result) rather than inventing a value.

    fn physics2d_circle_shape_create(&mut self) -> u64 {
        self.physics_mut()
            .map_or(0, |p| p.server_circle_shape_create())
    }

    fn physics2d_rectangle_shape_create(&mut self) -> u64 {
        self.physics_mut()
            .map_or(0, |p| p.server_rectangle_shape_create())
    }

    fn physics2d_shape_set_data_radius(&mut self, shape: u64, radius: f32) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_shape_set_data(shape, radius, None))
    }

    fn physics2d_shape_set_data_extents(&mut self, shape: u64, extents: (f32, f32)) -> bool {
        let extents = Vector2::new(extents.0, extents.1);
        self.physics_mut()
            .is_some_and(|p| p.server_shape_set_data(shape, 0.0, Some(extents)))
    }

    fn physics2d_body_create(&mut self) -> u64 {
        self.physics_mut().map_or(0, |p| p.server_body_create())
    }

    fn physics2d_body_set_space(&mut self, body: u64, space: u64) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_body_set_space(body, space))
    }

    fn physics2d_body_add_shape(&mut self, body: u64, shape: u64) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_body_add_shape(body, shape))
    }

    fn physics2d_body_set_state_vec2(&mut self, body: u64, state: i64, value: (f32, f32)) -> bool {
        let value = Vector2::new(value.0, value.1);
        self.physics_mut()
            .is_some_and(|p| p.server_body_set_state_vec2(body, state, value))
    }

    // pat-s3rrv.191: the tier-1 PhysicsServer2D surface. Each of these is the
    // thin half — the RID bookkeeping and the world mutation live in
    // `physics_server.rs`, so this file stays a routing table.
    fn physics2d_capsule_shape_create(&mut self) -> u64 {
        self.physics_mut()
            .map_or(0, |p| p.server_capsule_shape_create())
    }

    fn physics2d_shape_set_data_capsule(&mut self, shape: u64, radius: f32, height: f32) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_shape_set_data_capsule(shape, radius, height))
    }

    fn physics2d_body_set_param(&mut self, body: u64, param: i64, value: f32) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_body_set_param(body, param, value))
    }

    fn physics2d_body_get_param(&self, body: u64, param: i64) -> Option<f32> {
        self.physics()
            .and_then(|p| p.server_body_get_param(body, param))
    }

    fn physics2d_body_set_collision_layers(
        &mut self,
        body: u64,
        layers: u32,
        is_mask: bool,
    ) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_body_set_collision_layers(body, layers, is_mask))
    }

    fn physics2d_body_apply_impulse(
        &mut self,
        body: u64,
        impulse: (f32, f32),
        at_point: Option<(f32, f32)>,
    ) -> bool {
        let impulse = Vector2::new(impulse.0, impulse.1);
        let at = at_point.map(|(x, y)| Vector2::new(x, y));
        self.physics_mut()
            .is_some_and(|p| p.server_body_apply_impulse(body, impulse, at))
    }

    fn physics2d_body_apply_force(
        &mut self,
        body: u64,
        force: (f32, f32),
        at_point: Option<(f32, f32)>,
    ) -> bool {
        let force = Vector2::new(force.0, force.1);
        let at = at_point.map(|(x, y)| Vector2::new(x, y));
        self.physics_mut()
            .is_some_and(|p| p.server_body_apply_force(body, force, at))
    }

    fn physics2d_body_apply_torque_impulse(&mut self, body: u64, torque: f32) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_body_apply_torque_impulse(body, torque))
    }

    fn physics2d_body_get_linear_velocity(&self, body: u64) -> Option<(f32, f32)> {
        self.physics()
            .and_then(|p| p.server_body_linear_velocity(body))
            .map(|v| (v.x, v.y))
    }

    fn physics2d_body_get_angular_velocity(&self, body: u64) -> Option<f32> {
        self.physics()
            .and_then(|p| p.server_body_angular_velocity(body))
    }

    fn physics2d_body_get_sleep_state(&self, body: u64, can_sleep: bool) -> Option<bool> {
        self.physics()
            .and_then(|p| p.server_body_sleep_state(body, can_sleep))
    }

    fn physics2d_area_create(&mut self) -> u64 {
        self.physics_mut().map_or(0, |p| p.server_area_create())
    }

    fn physics2d_area_set_space(&mut self, area: u64, space: u64) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_area_set_space(area, space))
    }

    fn physics2d_area_add_shape(&mut self, area: u64, shape: u64) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_area_add_shape(area, shape))
    }

    fn physics2d_area_set_position(&mut self, area: u64, position: (f32, f32)) -> bool {
        let position = Vector2::new(position.0, position.1);
        self.physics_mut()
            .is_some_and(|p| p.server_area_set_position(area, position))
    }

    fn physics2d_area_set_param(
        &mut self,
        area: u64,
        param: i64,
        scalar: f32,
        vector: (f32, f32),
    ) -> bool {
        let vector = Vector2::new(vector.0, vector.1);
        self.physics_mut()
            .is_some_and(|p| p.server_area_set_param(area, param, scalar, vector))
    }

    fn physics2d_area_set_monitorable(&mut self, area: u64, monitorable: bool) -> bool {
        self.physics_mut()
            .is_some_and(|p| p.server_area_set_monitorable(area, monitorable))
    }

    fn physics2d_body_get_position(&self, body: u64) -> Option<(f32, f32)> {
        let pos = self.physics()?.server_body_position(body)?;
        Some((pos.x, pos.y))
    }

    fn physics2d_body_test_motion(
        &self,
        body: u64,
        motion: (f32, f32),
    ) -> Option<(bool, (f32, f32))> {
        let physics = self.physics()?;
        // An RID with no space has no world position to sweep from; reporting
        // `None` keeps that distinguishable from "swept and hit nothing".
        physics.server_body_position(body)?;
        let (collided, travel) =
            physics.server_body_test_motion(body, Vector2::new(motion.0, motion.1));
        Some((collided, (travel.x, travel.y)))
    }

    fn physics2d_node_test_move(&self, node: u64, motion: (f32, f32)) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.physics()
            .is_some_and(|p| p.node_test_move(nid, Vector2::new(motion.0, motion.1)))
    }

    /// `PhysicsBody2D.add_collision_exception_with(body)` (pat-s3rrv.100).
    ///
    /// ⚠️ Both nodes must already be REGISTERED bodies. A script calling this
    /// in `_ready` — before the first `sync_to_physics` has run — resolves no
    /// `BodyId` and the exception is silently dropped, which is why this
    /// answers a bool the node-method arm can surface rather than returning
    /// unit and pretending it worked.
    fn physics2d_node_add_collision_exception(&mut self, node: u64, other: u64) -> bool {
        let a_nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let b_nid = NodeId::from_object_id(ObjectId::from_raw(other));
        let Some(phys) = self.physics_mut() else {
            return false;
        };
        let (Some(a), Some(b)) = (phys.body_for_node(a_nid), phys.body_for_node(b_nid)) else {
            return false;
        };
        match phys.world_mut().get_body_mut(a) {
            Some(body) => {
                body.add_collision_exception(b);
                true
            }
            None => false,
        }
    }

    fn physics2d_node_remove_collision_exception(&mut self, node: u64, other: u64) -> bool {
        let a_nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let b_nid = NodeId::from_object_id(ObjectId::from_raw(other));
        let Some(phys) = self.physics_mut() else {
            return false;
        };
        let (Some(a), Some(b)) = (phys.body_for_node(a_nid), phys.body_for_node(b_nid)) else {
            return false;
        };
        match phys.world_mut().get_body_mut(a) {
            Some(body) => {
                body.remove_collision_exception(b);
                true
            }
            None => false,
        }
    }

    /// The excepted bodies as node object ids, in the order they were added.
    ///
    /// A `BodyId` with no node behind it is dropped rather than reported as a
    /// zero id: Godot hands back an Array of real nodes, and a script doing
    /// `for b in get_collision_exceptions(): b.queue_free()` must not be given
    /// something it cannot call methods on.
    fn physics2d_node_get_collision_exceptions(&self, node: u64) -> Vec<u64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(phys) = self.physics() else {
            return Vec::new();
        };
        let Some(id) = phys.body_for_node(nid) else {
            return Vec::new();
        };
        let Some(body) = phys.world().get_body(id) else {
            return Vec::new();
        };
        body.collision_exceptions
            .iter()
            .filter_map(|bid| phys.node_for_body(*bid))
            .map(|n| n.object_id().raw())
            .collect()
    }

    /// `RayCast2D.force_raycast_update()` (pat-0q2kx.32) — runs the very same
    /// `raycast2d::force_raycast_update` the physics tick runs
    /// (`main_loop.rs::process_raycasts`), so a scripted update and a ticked one
    /// cannot drift apart.
    ///
    /// The physics server is resolved from its stored *address* rather than via
    /// [`Self::physics`]: that helper borrows `&self`, which would collide with
    /// the `&mut SceneTree` this call also needs. Reading the address out as a
    /// `usize` first keeps the two borrows independent — which is the reason the
    /// server is held as an address in the first place.
    fn force_raycast_update_2d(&mut self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let addr = self.tree().script_physics_addr();
        if addr == 0 {
            return false;
        }
        // SAFETY: identical contract to `physics()` — `set_script_physics_server`
        // stored the address of a live `PhysicsServer` that `MainLoop` clears
        // before the server can move or drop. The server is a sibling of the
        // tree, never stored inside it, so this shared borrow cannot alias the
        // `&mut SceneTree` taken on the next line.
        let physics = unsafe { &*(addr as *const crate::physics_server::PhysicsServer) };
        crate::raycast2d::force_raycast_update(self.tree_mut(), physics, nid)
    }

    fn is_physics_processing(&self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().is_node_physics_processing(nid)
    }

    fn request_ready(&mut self, node: u64) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().request_node_ready(nid);
    }

    // -- AudioServer singleton (pat-s3rrv.6) --------------------------------
    //
    // Every one of these addresses the live `AudioServer` that `MainLoop`'s
    // `AudioServerBridge` mixes with each frame, bound by address for the
    // duration of the script phase. Nothing bound (a bare accessor, or a script
    // running outside a `MainLoop` phase) leaves the trait defaults in force.
    //
    // Godot indexes buses with a signed int and tolerates a bad index by doing
    // nothing, so every method funnels through `usize::try_from` and reports
    // failure instead of panicking on a negative or out-of-range bus.

    fn audio_get_bus_index(&self, name: &str) -> i64 {
        self.audio()
            .and_then(|a| a.mixer().get_bus_by_name(name))
            .map_or(-1, |i| i as i64)
    }

    fn audio_get_bus_count(&self) -> i64 {
        self.audio().map_or(0, |a| a.mixer().bus_count() as i64)
    }

    fn audio_get_bus_name(&self, idx: i64) -> Option<String> {
        let i = usize::try_from(idx).ok()?;
        self.audio()?
            .mixer()
            .get_bus(i)
            .map(|b| b.name().to_string())
    }

    fn audio_set_bus_name(&mut self, idx: i64, name: &str) -> bool {
        let Ok(i) = usize::try_from(idx) else {
            return false;
        };
        let name = name.to_string();
        let Some(audio) = self.audio_mut() else {
            return false;
        };
        match audio.mixer_mut().get_bus_mut(i) {
            Some(bus) => {
                bus.set_name(name);
                true
            }
            None => false,
        }
    }

    fn audio_set_bus_volume_db(&mut self, idx: i64, db: f32) -> bool {
        let Ok(i) = usize::try_from(idx) else {
            return false;
        };
        let Some(audio) = self.audio_mut() else {
            return false;
        };
        match audio.mixer_mut().get_bus_mut(i) {
            Some(bus) => {
                bus.set_volume_db(db);
                true
            }
            None => false,
        }
    }

    fn audio_get_bus_volume_db(&self, idx: i64) -> Option<f32> {
        let i = usize::try_from(idx).ok()?;
        self.audio()?.mixer().get_bus(i).map(|b| b.volume_db())
    }

    fn audio_set_bus_mute(&mut self, idx: i64, mute: bool) -> bool {
        let Ok(i) = usize::try_from(idx) else {
            return false;
        };
        let Some(audio) = self.audio_mut() else {
            return false;
        };
        match audio.mixer_mut().get_bus_mut(i) {
            Some(bus) => {
                bus.set_mute(mute);
                true
            }
            None => false,
        }
    }

    fn audio_is_bus_mute(&self, idx: i64) -> bool {
        usize::try_from(idx)
            .ok()
            .and_then(|i| self.audio()?.mixer().get_bus(i).map(|b| b.is_mute()))
            .unwrap_or(false)
    }

    fn audio_set_bus_solo(&mut self, idx: i64, solo: bool) -> bool {
        let Ok(i) = usize::try_from(idx) else {
            return false;
        };
        let Some(audio) = self.audio_mut() else {
            return false;
        };
        match audio.mixer_mut().get_bus_mut(i) {
            Some(bus) => {
                bus.set_solo(solo);
                true
            }
            None => false,
        }
    }

    fn audio_is_bus_solo(&self, idx: i64) -> bool {
        usize::try_from(idx)
            .ok()
            .and_then(|i| self.audio()?.mixer().get_bus(i).map(|b| b.is_solo()))
            .unwrap_or(false)
    }

    fn audio_set_bus_bypass_effects(&mut self, idx: i64, bypass: bool) -> bool {
        let Ok(i) = usize::try_from(idx) else {
            return false;
        };
        let Some(audio) = self.audio_mut() else {
            return false;
        };
        match audio.mixer_mut().get_bus_mut(i) {
            Some(bus) => {
                bus.set_bypass(bypass);
                true
            }
            None => false,
        }
    }

    fn audio_is_bus_bypassing_effects(&self, idx: i64) -> bool {
        usize::try_from(idx)
            .ok()
            .and_then(|i| self.audio()?.mixer().get_bus(i).map(|b| b.is_bypass()))
            .unwrap_or(false)
    }

    fn audio_get_bus_peak_volume_left_db(&self, idx: i64, channel: i64) -> f32 {
        let (Ok(i), Ok(c)) = (usize::try_from(idx), usize::try_from(channel)) else {
            return -200.0;
        };
        self.audio()
            .map_or(-200.0, |a| a.get_bus_peak_volume_left_db(i, c))
    }

    fn audio_get_bus_peak_volume_right_db(&self, idx: i64, channel: i64) -> f32 {
        let (Ok(i), Ok(c)) = (usize::try_from(idx), usize::try_from(channel)) else {
            return -200.0;
        };
        self.audio()
            .map_or(-200.0, |a| a.get_bus_peak_volume_right_db(i, c))
    }

    fn audio_set_playback_speed_scale(&mut self, scale: f32) -> bool {
        self.audio_mut()
            .map(|a| a.set_playback_speed_scale(scale))
            .is_some()
    }

    fn audio_get_playback_speed_scale(&self) -> f32 {
        self.audio().map_or(1.0, |a| a.playback_speed_scale())
    }

    fn audio_get_mix_rate(&self) -> f32 {
        self.audio()
            .map_or(44_100.0, |a| a.output_sample_rate() as f32)
    }

    /// Godot's `add_bus(at_position)` inserts a bus named "New Bus"; a negative
    /// or out-of-range position appends. The mixer only appends, so a positional
    /// insert is an append followed by a move.
    ///
    /// `AudioMixer::move_bus` *panics* on index 0 (the master bus cannot be
    /// displaced), so position 0 is clamped to an append rather than allowed to
    /// take down the engine from a script.
    fn audio_add_bus(&mut self, at_position: i64) -> bool {
        let Some(audio) = self.audio_mut() else {
            return false;
        };
        let mixer = audio.mixer_mut();
        let added = mixer.add_bus("New Bus");
        if let Ok(target) = usize::try_from(at_position) {
            if target >= 1 && target < added {
                mixer.move_bus(added, target);
            }
        }
        true
    }

    /// `AudioMixer::remove_bus` *panics* on index 0 or out of range. Godot
    /// refuses to remove the master bus and ignores a bad index, so both are
    /// reported as failure here instead of reaching the assert.
    fn audio_remove_bus(&mut self, idx: i64) -> bool {
        let Ok(i) = usize::try_from(idx) else {
            return false;
        };
        let Some(audio) = self.audio_mut() else {
            return false;
        };
        let mixer = audio.mixer_mut();
        if i == 0 || i >= mixer.bus_count() {
            return false;
        }
        mixer.remove_bus(i);
        true
    }

    // -- bus effect instances (pat-s3rrv.141) -------------------------------
    // `AudioEffectCapture` and `AudioEffectRecord` are the two effects a script
    // DRIVES rather than configures, so they need a call path down to the live
    // mixer. gdscript-interop cannot depend on gdaudio, which is why the
    // dispatch lands here where both are in scope.

    fn audio_effect_class(&self, bus_idx: i64, effect_idx: i64) -> Option<String> {
        let bus = usize::try_from(bus_idx).ok()?;
        let slot = usize::try_from(effect_idx).ok()?;
        Some(self.audio()?.bus_effect_class(bus, slot)?.to_string())
    }

    fn audio_effect_call(
        &mut self,
        bus_idx: i64,
        effect_idx: i64,
        method: &str,
        args: &[Variant],
    ) -> Option<Variant> {
        let bus = usize::try_from(bus_idx).ok()?;
        let slot = usize::try_from(effect_idx).ok()?;
        // Frame counts arrive as `Int`, but a float is tolerated because
        // arithmetic on one yields a float (`get_frames_available() / 2`).
        let int_arg = |i: usize| -> i64 {
            match args.get(i) {
                Some(Variant::Int(v)) => *v,
                Some(Variant::Float(f)) => *f as i64,
                _ => 0,
            }
        };
        let audio = self.audio_mut()?;

        match method {
            // -- AudioEffectCapture ---------------------------------------
            "get_buffer" => {
                let frames = audio.bus_capture_mut(bus, slot)?.get_buffer(int_arg(0));
                // ⚠️ One `Vector2` per FRAME, `(left, right)` — not a flat
                // float array. A caller reading it as mono would take every
                // other sample and hear the signal an octave up.
                Some(Variant::PackedVector2Array(
                    frames
                        .into_iter()
                        .map(|(l, r)| Vector2::new(l, r))
                        .collect(),
                ))
            }
            "can_get_buffer" => Some(Variant::Bool(
                audio.bus_capture(bus, slot)?.can_get_buffer(int_arg(0)),
            )),
            "clear_buffer" => {
                audio.bus_capture_mut(bus, slot)?.clear_buffer();
                Some(Variant::Nil)
            }
            "get_frames_available" => Some(Variant::Int(
                audio.bus_capture(bus, slot)?.get_frames_available(),
            )),
            "get_buffer_length_frames" => Some(Variant::Int(
                audio.bus_capture(bus, slot)?.get_buffer_length_frames(),
            )),
            "get_pushed_frames" => Some(Variant::Int(
                audio.bus_capture(bus, slot)?.get_pushed_frames(),
            )),
            "get_discarded_frames" => Some(Variant::Int(
                audio.bus_capture(bus, slot)?.get_discarded_frames(),
            )),
            "get_buffer_length" => Some(Variant::Float(
                audio.bus_capture(bus, slot)?.get_buffer_length() as f64,
            )),
            "set_buffer_length" => {
                let seconds = match args.first() {
                    Some(Variant::Float(f)) => *f as f32,
                    Some(Variant::Int(i)) => *i as f32,
                    _ => return Some(Variant::Nil),
                };
                audio.bus_capture_mut(bus, slot)?.set_buffer_length(seconds);
                Some(Variant::Nil)
            }

            // -- AudioEffectRecord ----------------------------------------
            "set_recording_active" => {
                let active = matches!(args.first(), Some(Variant::Bool(true)));
                audio
                    .bus_record_mut(bus, slot)?
                    .set_recording_active(active);
                Some(Variant::Nil)
            }
            "is_recording_active" => Some(Variant::Bool(
                audio.bus_record(bus, slot)?.is_recording_active(),
            )),
            "set_format" => {
                audio.bus_record_mut(bus, slot)?.set_format(int_arg(0));
                Some(Variant::Nil)
            }
            "get_format" => Some(Variant::Int(audio.bus_record(bus, slot)?.get_format())),
            "get_recording" => {
                let recording = audio.bus_record(bus, slot)?.get_recording();
                // ⚠️ Godot returns a NULL for an empty take rather than a
                // zero-length stream, so a script cannot mistake "the bus was
                // never recorded" for "silence was recorded". Same for a format
                // this port cannot encode.
                let Ok(wav) = recording else {
                    return Some(Variant::Nil);
                };
                let mut res = gdvariant::ResourceRef {
                    path: String::new(),
                    class_name: "AudioStreamWAV".to_string(),
                    properties: HashMap::new(),
                };
                // The four setters Godot calls on the sample it builds
                // (`audio_effect_record.cpp:264-269`), under their real Godot
                // property names so a script reads them the usual way.
                res.properties
                    .insert("data".to_string(), Variant::PackedByteArray(wav.data));
                res.properties
                    .insert("format".to_string(), Variant::Int(wav.format));
                res.properties
                    .insert("mix_rate".to_string(), Variant::Int(wav.mix_rate as i64));
                res.properties
                    .insert("stereo".to_string(), Variant::Bool(wav.stereo));
                res.properties
                    .insert("loop_mode".to_string(), Variant::Int(wav.loop_mode));
                Some(Variant::Resource(Box::new(res)))
            }

            _ => None,
        }
    }

    // -- Object metadata (pat-nzgdu.30) -------------------------------------
    // Meta is Object-level state stored in the Node's own meta bag (distinct
    // from the script/property namespace used above).

    fn set_meta(&mut self, node: u64, name: &str, value: Variant) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        if let Some(n) = self.tree_mut().get_node_mut(nid) {
            n.set_meta(name, value);
        }
    }

    fn get_meta(&self, node: u64, name: &str, default: Variant) -> Variant {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        match self.tree().get_node(nid) {
            Some(n) if n.has_meta(name) => n.get_meta(name),
            _ => default,
        }
    }

    fn has_meta(&self, node: u64, name: &str) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_node(nid)
            .map(|n| n.has_meta(name))
            .unwrap_or(false)
    }

    fn remove_meta(&mut self, node: u64, name: &str) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        if let Some(n) = self.tree_mut().get_node_mut(nid) {
            n.remove_meta(name);
        }
    }

    fn get_meta_list(&self, node: u64) -> Vec<String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let mut names: Vec<String> = self
            .tree()
            .get_node(nid)
            .map(|n| {
                n.get_meta_list()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn emit_signal(&mut self, node: u64, signal: &str, args: &[Variant]) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().emit_signal(nid, signal, args);
    }

    fn connect_signal(&mut self, source: u64, signal: &str, target: u64, method: &str) {
        let source_id = NodeId::from_object_id(ObjectId::from_raw(source));
        let target_oid = ObjectId::from_raw(target);
        let conn = Connection::new(target_oid, method);
        self.tree_mut().connect_signal(source_id, signal, conn);
    }

    fn connect_signal_flags(
        &mut self,
        source: u64,
        signal: &str,
        target: u64,
        method: &str,
        flags: i64,
        binds: &[Variant],
    ) {
        let source_id = NodeId::from_object_id(ObjectId::from_raw(source));
        let target_oid = ObjectId::from_raw(target);
        let mut conn = Connection::new(target_oid, method);
        // Godot ConnectFlags: DEFERRED=1, ONE_SHOT=4 (PERSIST/REFERENCE_COUNTED
        // do not affect dispatch here).
        conn.deferred = flags & 1 != 0;
        conn.one_shot = flags & 4 != 0;
        if !binds.is_empty() {
            conn.binds = binds.to_vec();
        }
        self.tree_mut().connect_signal(source_id, signal, conn);
    }

    fn disconnect_signal(&mut self, source: u64, signal: &str, target: u64, method: &str) -> bool {
        let source_id = NodeId::from_object_id(ObjectId::from_raw(source));
        let target_oid = ObjectId::from_raw(target);
        self.tree_mut()
            .signal_store_mut(source_id)
            .disconnect(signal, target_oid, method)
    }

    fn is_signal_connected(&self, source: u64, signal: &str, target: u64, method: &str) -> bool {
        let source_id = NodeId::from_object_id(ObjectId::from_raw(source));
        let target_oid = ObjectId::from_raw(target);
        self.tree()
            .signal_store(source_id)
            .and_then(|store| store.get_signal(signal))
            .map(|sig| {
                sig.connections()
                    .iter()
                    .any(|c| c.target_id == target_oid && c.method == method)
            })
            .unwrap_or(false)
    }

    fn node_has_signal(&self, node: u64, name: &str) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let tree = self.tree();
        // 1. The attached script's `signal` declarations — the case the
        //    SignalStore misses, because a declared-but-unconnected signal has
        //    no store entry yet (pat-0q2kx.9).
        if let Some(script) = tree.get_script(nid) {
            if script.list_signals().iter().any(|s| s == name) {
                return true;
            }
        }
        // 2. A built-in signal on the node's class (`Button.pressed`).
        if let Some(node_ref) = tree.get_node(nid) {
            if gdobject::class_db::class_has_signal(node_ref.class_name(), name) {
                return true;
            }
        }
        // 3. Anything already connected or explicitly added at runtime.
        tree.signal_store(nid)
            .map(|store| store.get_signal(name).is_some())
            .unwrap_or(false)
    }

    /// Backs Godot's `Signal.get_connections()` / `has_connections()`
    /// (pat-0q2kx.9).
    fn signal_connections(&self, source: u64, signal: &str) -> Vec<(u64, String)> {
        let source_id = NodeId::from_object_id(ObjectId::from_raw(source));
        self.tree()
            .signal_store(source_id)
            .and_then(|store| store.get_signal(signal))
            .map(|sig| {
                sig.connections()
                    .iter()
                    .map(|c| (c.target_id.raw(), c.method.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn get_node_name(&self, node: u64) -> Option<String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().get_node(nid).map(|n| n.name().to_string())
    }

    fn is_input_action_pressed(&self, action: &str) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_action_pressed(action))
            .unwrap_or(false)
    }

    fn is_input_action_just_pressed(&self, action: &str) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_action_just_pressed(action))
            .unwrap_or(false)
    }

    // Analog action strength (pat-s3rrv.14). The `SceneAccess` DEFAULTS treat
    // every action as digital (1.0 / 0.0), and the real analog values live in
    // `gdplatform::input::InputState`, which belongs to `MainLoop` and is
    // unreachable from a running script — so without these overrides no script
    // could observe an analog or deadzoned value from EITHER accessor, however
    // correct `InputState`'s own implementation was.
    //
    // The frame owner mirrors the pair into the snapshot; here we only read it.
    // Falling through to the digital default when the action is absent is what
    // keeps a keyboard-bound action reporting 1.0 while held.
    fn get_action_strength(&self, action: &str) -> f32 {
        if let Some(strength) = self
            .input
            .as_ref()
            .and_then(|i| i.action_strengths.get(action))
            .map(|(s, _)| *s)
        {
            return strength;
        }
        if self.is_input_action_pressed(action) {
            1.0
        } else {
            0.0
        }
    }

    fn get_action_raw_strength(&self, action: &str) -> f32 {
        if let Some(raw) = self
            .input
            .as_ref()
            .and_then(|i| i.action_strengths.get(action))
            .map(|(_, raw)| *raw)
        {
            return raw;
        }
        if self.is_input_action_pressed(action) {
            1.0
        } else {
            0.0
        }
    }

    // -- Input singleton mutators (pat-0q2kx.24) ----------------------------
    //
    // Godot's `Input` mutators change live engine state
    // (upstream/godot/core/input/input.cpp). The engine `InputState` belongs to
    // `MainLoop`, which a running script cannot reach, so each call is queued on
    // the tree and applied after the script phase. Reads stay answerable within
    // the frame: mouse mode and joypads come from the tree's mirrors, and an
    // injected action press lands on the frame snapshot so a later script in the
    // same frame observes it — matching Godot, where `action_press` is visible to
    // `is_action_pressed` immediately.

    fn set_mouse_mode(&mut self, mode: i64) {
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::SetMouseMode(mode));
        tree.set_input_mouse_mode_mirror(mode);
    }

    fn get_mouse_mode(&self) -> i64 {
        self.tree().input_mouse_mode()
    }

    // -- SceneMultiplayer surface (pat-0q2kx.94) ----------------------------

    fn multiplayer_is_server(&self) -> bool {
        self.tree().get_multiplayer().is_server()
    }

    fn multiplayer_unique_id(&self) -> i64 {
        self.tree()
            .get_multiplayer()
            .unique_id()
            .map(|id| id.0 as i64)
            .unwrap_or(0)
    }

    fn multiplayer_has_peer(&self) -> bool {
        self.tree().get_multiplayer().unique_id().is_some()
    }

    fn multiplayer_peers(&self) -> Vec<i64> {
        self.tree()
            .get_multiplayer()
            .get_peers()
            .into_iter()
            .map(|id| id as i64)
            .collect()
    }

    fn multiplayer_remote_sender_id(&self) -> i64 {
        self.tree().get_multiplayer().get_remote_sender_id().0 as i64
    }

    fn multiplayer_authenticating_peers(&self) -> Vec<i64> {
        self.tree()
            .get_multiplayer()
            .pending_auth_peers()
            .into_iter()
            .map(|id| id as i64)
            .collect()
    }

    fn multiplayer_send_bytes(&mut self, bytes: &[u8], target: i64, channel: i64) -> bool {
        self.tree_mut()
            .get_multiplayer_mut()
            .send_bytes(
                bytes,
                target as i32,
                gdplatform::network::TransferMode::Reliable,
                channel.clamp(0, u8::MAX as i64) as u8,
            )
            .is_ok()
    }

    fn multiplayer_complete_auth(&mut self, id: i64, accepted: bool) -> bool {
        self.tree_mut()
            .get_multiplayer_mut()
            .complete_auth(id as i32, accepted)
    }

    fn multiplayer_set_refuse_new_connections(&mut self, refuse: bool) {
        self.tree_mut()
            .get_multiplayer_mut()
            .set_refuse_new_connections(refuse);
    }

    fn multiplayer_refuses_new_connections(&self) -> bool {
        self.tree().get_multiplayer().refuse_new_connections()
    }

    /// `SceneMultiplayer.disconnect_peer(id)` (pat-s3rrv.18) — routed through
    /// the peer state the transports share, so the peer really leaves
    /// `get_peers()` rather than only being marked.
    fn multiplayer_disconnect_peer(&mut self, id: i64) -> bool {
        self.tree_mut()
            .get_multiplayer_mut()
            .peer_state_mut()
            .disconnect_peer(id as i32)
    }

    /// `SceneMultiplayer.clear()` — forgets the connected peers and any queued
    /// events. The assigned transport stays, matching Godot: `clear` resets the
    /// API's bookkeeping, it does not unassign the peer.
    fn multiplayer_clear(&mut self) {
        let api = self.tree_mut().get_multiplayer_mut();
        for id in api.get_peers() {
            api.peer_state_mut().disconnect_peer(id);
        }
        let _ = api.take_events();
    }

    fn multiplayer_set_auth_callback(&mut self, callback: Variant) {
        self.tree_mut().set_multiplayer_auth_callback(callback);
    }

    fn multiplayer_get_auth_callback(&self) -> Variant {
        self.tree().multiplayer_auth_callback()
    }

    /// `SceneMultiplayer.send_auth(id, data)` — the payload goes out on the
    /// live transport, so it only succeeds while a peer is assigned.
    fn multiplayer_send_auth(&mut self, id: i64, data: Vec<u8>) -> bool {
        let api = self.tree_mut().get_multiplayer_mut();
        let Some(peer) = api.peer_mut() else {
            return false;
        };
        peer.send_packet(
            gdplatform::network::PeerId(id.max(0) as u32),
            gdplatform::network::Packet {
                sender: gdplatform::network::PeerId(0),
                channel: 0,
                data,
                transfer_mode: gdplatform::network::TransferMode::Reliable,
            },
        )
        .is_ok()
    }

    /// The node the per-frame multiplayer drain emits its signals on
    /// (pat-s3rrv.18).
    ///
    /// `poll_multiplayer_with_delta` emits all eight connection signals on the
    /// scene root (`scene_tree.rs`, the `emit_signal(root, …)` at the end of the
    /// event drain), so binding `multiplayer.peer_connected` to that same object
    /// makes `.connect(cb)` land on the store the emission already reaches.
    ///
    /// Godot hangs these off the `MultiplayerAPI` object rather than the root;
    /// the only observable difference is `signal.get_object()`, and routing to
    /// the real emitter is what makes the callback actually fire.
    fn multiplayer_signal_object(&self) -> u64 {
        self.tree().root_id().raw()
    }

    /// `multiplayer.multiplayer_peer = peer` (pat-s3rrv.16).
    ///
    /// Takes the peer OUT of the process-wide handle table and hands ownership
    /// to the tree's `MultiplayerAPI`, which is what makes the existing
    /// per-frame `poll_multiplayer_with_delta` start driving it. Assigning the
    /// same handle twice fails the second time — by then the tree owns the peer
    /// and the slot is empty, which is the honest answer rather than silently
    /// installing a second peer.
    fn multiplayer_set_peer(&mut self, handle: u64) -> bool {
        let id = ObjectId::from_raw(handle);
        if !gdplatform::multiplayer_peer_handles::is_multiplayer_peer_handle(id) {
            // pat-s3rrv.230: not a built-in transport handle — it may be a node
            // whose script `extends MultiplayerPeerExtension`, which is how Godot
            // lets a project supply its own transport. This used to `return false`
            // unconditionally, so `multiplayer.multiplayer_peer = self` was
            // silently dropped and the script's `_poll` was never called.
            //
            // The identity is the node because a bare `ClassInstance` has no
            // Variant form: `self` is the only script identity the interpreter can
            // hand across this boundary.
            let node = crate::node::NodeId::from_object_id(id);
            return self.tree_mut().set_script_multiplayer_peer(node);
        }
        let Some(peer) = gdplatform::multiplayer_peer_handles::take_multiplayer_peer(id) else {
            return false;
        };
        self.tree_mut()
            .get_multiplayer_mut()
            .set_multiplayer_peer(peer);
        true
    }

    // -- cursor shape (pat-0q2kx.90) ----------------------------------------
    //
    // Mirrored on the tree as well as queued, for the same reason
    // `set_mouse_mode` is: the command is applied by the frame loop, but Godot's
    // `Input.get_current_cursor_shape()` answers immediately, so a script that
    // sets and reads back within one frame must see its own write.

    fn set_default_cursor_shape(&mut self, shape: i64) {
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::SetDefaultCursorShape(shape));
        tree.set_cursor_shape_mirror(shape);
    }

    fn get_current_cursor_shape(&self) -> i64 {
        self.tree().cursor_shape()
    }

    fn set_custom_mouse_cursor(
        &mut self,
        image_path: &str,
        shape: i64,
        hotspot_x: f32,
        hotspot_y: f32,
    ) {
        self.tree_mut()
            .queue_input_command(InputCommand::SetCustomMouseCursor {
                image_path: image_path.to_string(),
                shape,
                hotspot_x,
                hotspot_y,
            });
    }

    fn warp_mouse(&mut self, x: f32, y: f32) {
        self.tree_mut()
            .queue_input_command(InputCommand::WarpMouse { x, y });
    }

    fn action_press(&mut self, action: &str, strength: f32) {
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::ActionPress {
            action: action.to_string(),
            strength,
        });
        tree.inject_action_press(action);
    }

    fn action_release(&mut self, action: &str) {
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::ActionRelease {
            action: action.to_string(),
        });
        tree.inject_action_release(action);
    }

    // -- InputMap singleton (pat-0q2kx.89) ---------------------------------
    //
    // Mutations are queued for the frame loop (which owns the engine map) and
    // ALSO applied to the tree's mirror right away, so a script that does
    // `add_action` then `has_action` in the same frame sees its own write —
    // the same eager-mirror rule `set_mouse_mode` follows.

    fn input_map_has_action(&self, action: &str) -> bool {
        self.tree().input_map_has_action(action)
    }

    fn input_map_get_actions(&self) -> Vec<String> {
        self.tree().input_map_actions()
    }

    fn input_map_add_action(&mut self, action: &str, deadzone: f32) {
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::InputMapAddAction {
            action: action.to_string(),
            deadzone,
        });
        tree.input_map_mirror_add_action(action);
    }

    fn input_map_erase_action(&mut self, action: &str) {
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::InputMapEraseAction {
            action: action.to_string(),
        });
        tree.input_map_mirror_erase_action(action);
    }

    fn input_map_action_set_deadzone(&mut self, action: &str, deadzone: f32) {
        self.tree_mut()
            .queue_input_command(InputCommand::InputMapSetDeadzone {
                action: action.to_string(),
                deadzone,
            });
    }

    fn input_map_action_add_event(
        &mut self,
        action: &str,
        event: gdscript_interop::bindings::InputEventSpec,
    ) {
        // An event the engine cannot model is dropped here rather than queued,
        // so the mirror's event count stays truthful about what will bind.
        let Some(binding) = binding_from_event_spec(&event) else {
            return;
        };
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::InputMapAddEvent {
            action: action.to_string(),
            binding,
        });
        tree.input_map_mirror_add_event(action);
    }

    fn input_map_action_erase_event(
        &mut self,
        action: &str,
        event: gdscript_interop::bindings::InputEventSpec,
    ) {
        let Some(binding) = binding_from_event_spec(&event) else {
            return;
        };
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::InputMapEraseEvent {
            action: action.to_string(),
            binding,
        });
        tree.input_map_mirror_erase_event(action);
    }

    fn input_map_action_erase_events(&mut self, action: &str) {
        let tree = self.tree_mut();
        tree.queue_input_command(InputCommand::InputMapEraseEvents {
            action: action.to_string(),
        });
        tree.input_map_mirror_erase_events(action);
    }

    fn input_map_load_from_project_settings(&mut self) {
        // No mirror call to pair with this one: a reload replaces the whole map
        // rather than touching one action, and the tree has no copy of the
        // project's declarations to rebuild from. It does not need one — the
        // command applier rebuilds BOTH input-map mirrors from engine truth
        // immediately after draining, so the restored map is visible from the
        // next read onward, exactly as for any other queued mutator.
        self.tree_mut()
            .queue_input_command(InputCommand::InputMapLoadFromProjectSettings);
    }

    fn input_map_action_event_count(&self, action: &str) -> usize {
        self.tree().input_map_action_event_count(action)
    }

    /// pat-s3rrv.14: the engine's real bindings, translated into the spec shape
    /// the interpreter matches events against.
    ///
    /// A `GamepadButton` with no Godot `JoyButton` (the analog triggers, which
    /// are axes there) is DROPPED rather than reported under a wrong index — a
    /// binding that cannot be named in Godot's terms must not masquerade as one
    /// that can.
    fn input_map_action_events(
        &self,
        action: &str,
    ) -> Vec<gdscript_interop::bindings::InputEventSpec> {
        use gdplatform::input::ActionBinding as B;
        use gdscript_interop::bindings::InputEventSpec;
        self.tree()
            .input_map_action_bindings(action)
            .iter()
            .filter_map(|binding| match binding {
                B::KeyBinding(key) => Some(InputEventSpec::key(key.to_godot_keycode())),
                B::KeyWithModifiers(combo) => Some(InputEventSpec {
                    class: "InputEventKey".to_string(),
                    code: combo.key.to_godot_keycode(),
                    axis_value: 0.0,
                    shift: combo.shift,
                    ctrl: combo.ctrl,
                    alt: combo.alt,
                }),
                B::MouseBinding(button) => Some(InputEventSpec {
                    class: "InputEventMouseButton".to_string(),
                    code: button.to_godot_index(),
                    axis_value: 0.0,
                    shift: false,
                    ctrl: false,
                    alt: false,
                }),
                B::GamepadButtonBinding(button) => {
                    button.to_godot_index().map(|code| InputEventSpec {
                        class: "InputEventJoypadButton".to_string(),
                        code,
                        axis_value: 0.0,
                        shift: false,
                        ctrl: false,
                        alt: false,
                    })
                }
                // A bare axis binding presses on POSITIVE deflection, which is
                // what `ActionBinding::axis_positive` means; the directional
                // form below carries its own sign.
                B::GamepadAxisBinding(axis) => Some(InputEventSpec {
                    class: "InputEventJoypadMotion".to_string(),
                    code: axis.to_godot_index(),
                    axis_value: 1.0,
                    shift: false,
                    ctrl: false,
                    alt: false,
                }),
                B::GamepadAxisDirection { axis, positive } => Some(InputEventSpec {
                    class: "InputEventJoypadMotion".to_string(),
                    code: axis.to_godot_index(),
                    axis_value: if *positive { 1.0 } else { -1.0 },
                    shift: false,
                    ctrl: false,
                    alt: false,
                }),
            })
            .collect()
    }

    fn get_connected_joypads(&self) -> Vec<i64> {
        self.tree().input_connected_joypads().to_vec()
    }

    fn is_joy_button_pressed(&self, device: i64, button: i64) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_joy_button_pressed(device, button))
            .unwrap_or(false)
    }

    fn get_joy_axis(&self, device: i64, axis: i64) -> f32 {
        self.input
            .as_ref()
            .map(|i| i.get_joy_axis(device, axis))
            .unwrap_or(0.0)
    }

    fn start_joy_vibration(&mut self, device: i64, weak: f32, strong: f32, duration: f32) {
        self.tree_mut()
            .queue_input_command(InputCommand::StartJoyVibration {
                device,
                weak,
                strong,
                duration,
            });
    }

    fn stop_joy_vibration(&mut self, device: i64) {
        self.tree_mut()
            .queue_input_command(InputCommand::StopJoyVibration { device });
    }

    fn is_input_action_just_released(&self, action: &str) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_action_just_released(action))
            .unwrap_or(false)
    }

    fn is_input_key_pressed(&self, key: &str) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_key_pressed(key))
            .unwrap_or(false)
    }

    /// `Input.is_key_pressed(KEY_A)` (pat-0q2kx.92). The snapshot keys pressed
    /// keys by Patina's own key names, so the Godot keycode is resolved through
    /// the platform mapping and then asked by name — the same state the string
    /// form reads, not a parallel one that could disagree.
    fn is_input_keycode_pressed(&self, keycode: i64) -> bool {
        let Some(key) = gdplatform::input::key_from_godot_keycode(keycode) else {
            return false;
        };
        self.is_input_key_pressed(key.name())
    }

    fn is_anything_pressed(&self) -> bool {
        self.input
            .as_ref()
            .map(|i| {
                !i.pressed_keys.is_empty()
                    || !i.mouse_buttons_pressed.is_empty()
                    || !i.actions_pressed.is_empty()
            })
            .unwrap_or(false)
    }

    fn get_last_mouse_velocity(&self) -> (f32, f32) {
        self.input
            .as_ref()
            .map(|i| (i.mouse_velocity.x, i.mouse_velocity.y))
            .unwrap_or((0.0, 0.0))
    }

    fn get_global_mouse_position(&self) -> (f32, f32) {
        self.input
            .as_ref()
            .map(|i| (i.mouse_position.x, i.mouse_position.y))
            .unwrap_or((0.0, 0.0))
    }

    fn is_mouse_button_pressed(&self, button_index: i64) -> bool {
        self.input
            .as_ref()
            .map(|i| i.is_mouse_button_pressed(button_index))
            .unwrap_or(false)
    }

    fn create_node(&mut self, class_name: &str, name: &str) -> Option<u64> {
        let id = self.tree_mut().create_node(class_name, name);
        Some(id.raw())
    }

    fn add_child(&mut self, parent_id: u64, child_id: u64) -> bool {
        let pid = NodeId::from_object_id(ObjectId::from_raw(parent_id));
        let cid = NodeId::from_object_id(ObjectId::from_raw(child_id));

        let tree = self.tree_mut();
        // A node produced by `instantiate_packed_scene` is already in the tree
        // (parented under the root), so move its whole subtree with `reparent`.
        // A freshly `create_node`d node is detached (no parent) and is inserted
        // via `take_node` + `add_child`, which wires up the parent-child
        // relationship and lifecycle events.
        let already_parented = tree
            .get_node(cid)
            .map(|n| n.parent().is_some())
            .unwrap_or(false);
        if already_parented {
            tree.reparent(cid, pid).is_ok()
        } else if let Some(node) = tree.take_node(cid) {
            tree.add_child(pid, node).is_ok()
        } else {
            false
        }
    }

    fn queue_free(&mut self, node_id: u64) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node_id));
        self.tree_mut().queue_free(nid);
    }

    /// Godot `Node.rpc`: send through the tree's MultiplayerAPI, addressed by
    /// the node's absolute path hash — the same hash `poll_multiplayer` resolves
    /// inbound packets with, so a call lands on the matching node on the peer.
    /// Godot `Node.rpc`: send through the tree's MultiplayerAPI, addressed by
    /// the node's absolute path hash — the same hash `poll_multiplayer` resolves
    /// inbound packets with, so a call lands on the matching node on the peer.
    fn node_rpc(&mut self, node: u64, method: &str, args: &[Variant]) -> Result<(), String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        // pat-s3rrv.230: a script `extends MultiplayerAPIExtension` REPLACES the
        // built-in stack, so it is consulted before the engine's own RPC registry
        // — Godot hands the call to `_rpc(peer, object, method, args)` and the
        // extension owns delivery from there. Returning early is the point: the
        // built-in path must not also run, or the call would go out twice.
        //
        // `0` is Godot's broadcast peer, which is what a plain `rpc()` means; the
        // addressed form is `node_rpc_id` below.
        if self.tree_mut().rpc_through_script_api(0, nid, method, args) {
            return Ok(());
        }
        let Some(hash) = self.rpc_hash_for(nid) else {
            return Err(format!("rpc(): node {node} is not in the tree"));
        };
        let sent = self
            .tree_mut()
            .get_multiplayer_mut()
            .rpc(hash, method, args.to_vec())
            .map_err(|e| format!("{e:?}"));
        // ⚠️ `call_local` runs the method on the SENDER too, and Godot does it
        // whether or not the packet went anywhere — a peerless send is not a
        // reason to skip local execution.
        //
        // Queued rather than called inline: `rpc()` is invoked FROM inside the
        // node's own running script, and a running script is checked out of
        // `SceneTree.scripts`, so a re-entrant `call_script_method` would find
        // no script and silently do nothing. The deferred queue runs it once the
        // current method has returned and the script is back in the arena.
        if self.tree().rpc_is_call_local(method) {
            self.tree_mut().call_deferred(nid, method, args);
        }
        sent
    }

    /// [`node_rpc`](Self::node_rpc) addressed to a single peer.
    fn node_rpc_id(
        &mut self,
        node: u64,
        peer: i64,
        method: &str,
        args: &[Variant],
    ) -> Result<(), String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(hash) = self.rpc_hash_for(nid) else {
            return Err(format!("rpc_id(): node {node} is not in the tree"));
        };
        let sent = self
            .tree_mut()
            .get_multiplayer_mut()
            .rpc_id(hash, peer, method, args.to_vec())
            .map_err(|e| format!("{e:?}"));
        if self.tree().rpc_is_call_local(method) {
            self.tree_mut().call_deferred(nid, method, args);
        }
        sent
    }

    fn is_object_alive(&self, id: u64) -> bool {
        // The scene-tree node arena is the authoritative alive registry: a node
        // is present until it is immediately freed (`remove_node`) or reaped
        // from the deferred-deletion queue (`process_deletions`). A `queue_free`d
        // node stays alive until that reap, matching Godot semantics.
        let nid = NodeId::from_object_id(ObjectId::from_raw(id));
        self.tree().get_node(nid).is_some()
    }

    fn instance_from_id(&self, id: u64) -> Option<u64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(id));
        self.tree().get_node(nid).map(|_| id)
    }

    fn free_object(&mut self, id: u64) {
        // Godot `Object.free()` is immediate; `remove_node` fires EXIT_TREE /
        // UNPARENTED, drops the subtree from the arena, and unregisters each
        // node from the weak-ref registry. The root cannot be freed this way.
        let nid = NodeId::from_object_id(ObjectId::from_raw(id));
        let _ = self.tree_mut().remove_node(nid);
    }

    fn call_deferred(&mut self, node_id: u64, method: &str, args: &[Variant]) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node_id));
        self.tree_mut().call_deferred(nid, method, args);
    }

    fn create_timer(&mut self, seconds: f64) -> Option<(u64, String)> {
        self.create_timer_with_flags(seconds, true, false, false)
    }

    fn create_timer_with_flags(
        &mut self,
        seconds: f64,
        process_always: bool,
        process_in_physics: bool,
        ignore_time_scale: bool,
    ) -> Option<(u64, String)> {
        let id = self.tree_mut().create_timer_with_flags(
            seconds,
            process_always,
            process_in_physics,
            ignore_time_scale,
        );
        let signal = self
            .tree()
            .get_timer(id)
            .map(|t| t.signal_name.clone())
            .unwrap_or_else(|| format!("timeout_{id}"));
        Some((id, signal))
    }

    fn process_timers(&mut self, delta: f64) -> Vec<String> {
        self.tree_mut().process_timers(delta);
        self.tree_mut().take_timeout_signals()
    }

    fn get_class(&self, node: u64) -> Option<String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_node(nid)
            .map(|n| n.class_name().to_string())
    }

    // -- AnimationPlayer playback (pat-0q2kx.1) -----------------------------

    fn animation_player_play(&mut self, node: u64, name: &str) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(player) = self.tree_mut().get_animation_player_mut(nid) else {
            return false;
        };
        if name.is_empty() {
            // Godot's `play()` with no argument resumes the current clip, or
            // starts the autoplay clip when nothing has played yet.
            let current = player.current_animation().map(str::to_owned);
            match current {
                Some(current) => player.play(&current),
                None => {
                    player.play_autoplay();
                }
            }
        } else {
            player.play(name);
        }
        true
    }

    fn animation_player_stop(&mut self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        match self.tree_mut().get_animation_player_mut(nid) {
            Some(player) => {
                player.stop();
                true
            }
            None => false,
        }
    }

    fn animation_player_pause(&mut self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        match self.tree_mut().get_animation_player_mut(nid) {
            Some(player) => {
                player.pause();
                true
            }
            None => false,
        }
    }

    fn animation_player_queue(&mut self, node: u64, name: &str) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        match self.tree_mut().get_animation_player_mut(nid) {
            Some(player) => {
                player.queue(name);
                true
            }
            None => false,
        }
    }

    /// `AnimatedSprite2D.play_backwards(name)` — Godot's `play(name, -1, true)`
    /// (`animated_sprite_2d.cpp:520-522`).
    ///
    /// ⚠️ Writes NODE PROPERTIES, not a playback struct.
    /// `process_animated_sprites` rebuilds its `AnimatedSpritePlayback` from the
    /// node every tick, so anything set on a struct here would be discarded
    /// before the next frame.
    /// `AnimatedSprite2D.play(name, custom_speed, from_end)`.
    ///
    /// ⚠️ The direction and the starting frame are INDEPENDENT arguments: the
    /// sign of `custom_speed` decides which way the clip runs, `from_end`
    /// decides where the playhead starts. Godot's `play_backwards(name)` is
    /// exactly `play(name, -1, true)` (`animated_sprite_2d.cpp:520-522`) — both
    /// at once — so folding them into a single flag would make
    /// `play("walk", -1, false)`, a reverse run from wherever the clip is,
    /// impossible to express.
    fn animated_sprite_play(
        &mut self,
        node: u64,
        name: &str,
        custom_speed: f64,
        from_end: bool,
    ) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(n) = self.tree().get_node(nid) else {
            return false;
        };
        let clip = if name.is_empty() {
            match n.get_property("animation") {
                Variant::String(s) if !s.is_empty() => s,
                _ => "default".to_string(),
            }
        } else {
            name.to_string()
        };
        let last = crate::scene_tree::sprite_frames_from_node(n)
            .map(|f| f.frame_count(&clip).saturating_sub(1))
            .unwrap_or(0);
        let Some(n) = self.tree_mut().get_node_mut(nid) else {
            return false;
        };
        n.set_property("animation", Variant::String(clip));
        n.set_property(
            "frame",
            Variant::Int(if from_end { last as i64 } else { 0 }),
        );
        // ⚠️ A `__sf_time` of 0 is the START of the frame going forwards and the
        // END of it going backwards, because a reversed clip counts down through
        // `(-hold, 0]`. That is exactly Godot's `set_frame_and_progress(last,
        // 1.0)` / `(0, 0.0)` pair, expressed once instead of twice.
        n.set_property("__sf_time", Variant::Float(0.0));
        // Godot keeps `speed_scale` and `custom_speed` separate and multiplies
        // them; the node carries only `speed_scale`, so the custom speed lands
        // there — the same place `play_backwards` negates.
        let speed = if custom_speed == 0.0 {
            1.0
        } else {
            custom_speed
        };
        n.set_property("speed_scale", Variant::Float(speed));
        n.set_property("playing", Variant::Bool(true));
        true
    }

    fn animated_sprite_play_backwards(&mut self, node: u64, name: &str) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(n) = self.tree().get_node(nid) else {
            return false;
        };
        let clip = if name.is_empty() {
            match n.get_property("animation") {
                Variant::String(s) if !s.is_empty() => s,
                _ => "default".to_string(),
            }
        } else {
            name.to_string()
        };
        // `from_end`: start at the last frame. Without it a non-looping clip
        // begins already at its reverse end and finishes without drawing.
        let last = crate::scene_tree::sprite_frames_from_node(n)
            .map(|f| f.frame_count(&clip).saturating_sub(1))
            .unwrap_or(0);
        let speed = match n.get_property("speed_scale") {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            _ => 1.0,
        };
        let Some(n) = self.tree_mut().get_node_mut(nid) else {
            return false;
        };
        n.set_property("animation", Variant::String(clip));
        n.set_property("frame", Variant::Int(last as i64));
        n.set_property("__sf_time", Variant::Float(0.0));
        n.set_property(
            "speed_scale",
            Variant::Float(-(speed.abs().max(0.01)) as f64),
        );
        n.set_property("playing", Variant::Bool(true));
        true
    }

    /// `AnimatedSprite2D.pause()` — freeze without resetting.
    ///
    /// ⚠️ Godot spells `pause` and `stop` as `_stop_internal(reset)`
    /// (`animated_sprite_2d.cpp:524-540`); the ONLY difference is whether the
    /// frame and progress are cleared. Collapsing them loses mid-clip resume.
    fn animated_sprite_pause(&mut self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(n) = self.tree_mut().get_node_mut(nid) else {
            return false;
        };
        n.set_property("playing", Variant::Bool(false));
        true
    }

    /// `AnimatedSprite2D.stop()` — freeze AND reset to the clip start.
    fn animated_sprite_stop(&mut self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(n) = self.tree_mut().get_node_mut(nid) else {
            return false;
        };
        n.set_property("playing", Variant::Bool(false));
        n.set_property("frame", Variant::Int(0));
        n.set_property("__sf_time", Variant::Float(0.0));
        // ⚠️ `frame_progress` has to be cleared HERE, not left to the next tick.
        // `process_animated_sprites` skips a node whose `playing` is false, so
        // nothing recomputes it after a stop — the sprite would sit rewound to
        // frame 0 while still advertising the sub-frame offset it stopped at.
        n.set_property("frame_progress", Variant::Float(0.0));
        true
    }

    /// `AnimatedSprite2D.set_frame_and_progress(frame, progress)` — both at once.
    ///
    /// ⚠️ `progress` is a 0..1 FRACTION of the current frame, while the tick
    /// stores raw seconds in `__sf_time` and derives progress as
    /// `time / frame_seconds(frame)`. Storing the fraction directly would make a
    /// progress of 0.5 mean half a second, which on a 10 fps clip is five frames.
    fn animated_sprite_set_frame_and_progress(
        &mut self,
        node: u64,
        frame: i64,
        progress: f64,
    ) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(n) = self.tree().get_node(nid) else {
            return false;
        };
        let anim = match n.get_property("animation") {
            Variant::String(s) if !s.is_empty() => s,
            _ => "default".to_string(),
        };
        let idx = frame.max(0) as usize;
        let hold = crate::scene_tree::sprite_frames_from_node(n)
            .and_then(|f| f.animation(&anim).map(|a| a.frame_seconds(idx)))
            .unwrap_or(0.0);
        // ⚠️ Read BEFORE the mutable re-borrow, and read it at all: the
        // fraction -> seconds conversion is direction-dependent, because a
        // reversed clip stores `__sf_time` in `(-hold, 0]`. Writing
        // `progress * hold` on a backwards sprite would put the playhead a
        // whole frame ahead of where the caller asked for, and the reader in
        // `process_animated_sprites` would clamp it straight back to 1.0.
        // The two conversions have to agree; this is the inverse of that one.
        let backwards = match n.get_property("speed_scale") {
            Variant::Float(f) => f < 0.0,
            Variant::Int(i) => i < 0,
            _ => false,
        };
        let Some(n) = self.tree_mut().get_node_mut(nid) else {
            return false;
        };
        let seconds = if backwards {
            (progress - 1.0) * hold as f64
        } else {
            progress * hold as f64
        };
        n.set_property("frame", Variant::Int(idx as i64));
        n.set_property("__sf_time", Variant::Float(seconds));
        n.set_property("frame_progress", Variant::Float(progress));
        true
    }

    /// `AnimatedSprite2D.get_playing_speed()` — the SIGNED effective speed, or
    /// 0 while stopped. Negative means the clip is running backwards.
    fn animated_sprite_playing_speed(&self, node: u64) -> f64 {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(n) = self.tree().get_node(nid) else {
            return 0.0;
        };
        if !matches!(n.get_property("playing"), Variant::Bool(true)) {
            return 0.0;
        }
        match n.get_property("speed_scale") {
            Variant::Float(f) => f,
            Variant::Int(i) => i as f64,
            _ => 1.0,
        }
    }

    fn animation_player_play_backwards(&mut self, node: u64, name: &str) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(player) = self.tree_mut().get_animation_player_mut(nid) else {
            return false;
        };
        // An empty name means "the current clip", matching `play()`'s handling.
        let clip = if name.is_empty() {
            player.current_animation().map(str::to_owned)
        } else {
            Some(name.to_owned())
        };
        if let Some(clip) = clip {
            player.play_backwards(&clip, -1.0);
        }
        true
    }

    fn animation_player_playing_speed(&self, node: u64) -> Option<f64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_animation_player(nid)
            .map(|p| p.get_playing_speed())
    }

    fn animation_player_has_animation(&self, node: u64, name: &str) -> Option<bool> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_animation_player(nid)
            .map(|p| p.has_animation(name))
    }

    fn animation_player_animation_list(&self, node: u64) -> Option<Vec<String>> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_animation_player(nid)
            .map(|p| p.get_animation_list())
    }

    fn animation_player_speed_scale(&self, node: u64) -> Option<f64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().get_animation_player(nid).map(|p| p.speed_scale)
    }

    fn animation_player_play_section(&mut self, node: u64, name: &str, start: f64, end: f64) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        if let Some(p) = self.tree_mut().get_animation_player_mut(nid) {
            // Godot's defaults for the three arguments a 3-arg call omits.
            p.play_section(name, start, end, -1.0, 1.0, false);
        }
    }

    fn animation_player_set_speed_scale(&mut self, node: u64, scale: f64) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        if let Some(p) = self.tree_mut().get_animation_player_mut(nid) {
            p.speed_scale = scale;
        }
    }

    // -- Object introspection over the signal graph (pat-0q2kx.64) ----------

    fn signal_connection_list(&self, node: u64, signal_name: &str) -> Vec<(u64, String, bool)> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(store) = self.tree().signal_store(nid) else {
            return Vec::new();
        };
        let Some(signal) = store.get_signal(signal_name) else {
            return Vec::new();
        };
        signal
            .connections()
            .iter()
            .map(|c| (c.target_id.raw(), c.method.clone(), c.one_shot))
            .collect()
    }

    fn incoming_connections(&self, node: u64) -> Vec<(u64, String, String)> {
        // Godot answers this from a per-object back-reference list; this tree
        // keeps connections only on the SOURCE, so the whole graph is scanned.
        // Linear in nodes×signals — fine for an introspection call a script
        // makes rarely, and it cannot go stale the way a cached back-list can.
        let target_raw = node;
        let mut out = Vec::new();
        for id in self.tree().all_nodes_in_tree_order() {
            let Some(store) = self.tree().signal_store(id) else {
                continue;
            };
            for signal_name in store.signal_names() {
                let Some(signal) = store.get_signal(signal_name) else {
                    continue;
                };
                for conn in signal.connections() {
                    if conn.target_id.raw() == target_raw {
                        out.push((id.raw(), signal_name.to_string(), conn.method.clone()));
                    }
                }
            }
        }
        out
    }

    fn has_any_connections(&self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let Some(store) = self.tree().signal_store(nid) else {
            return false;
        };
        store.signal_names().into_iter().any(|name| {
            store
                .get_signal(name)
                .is_some_and(|s| s.connection_count() > 0)
        })
    }

    fn notify_property_list_changed(&mut self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        if self.tree().get_node(nid).is_none() {
            return false;
        }
        self.tree_mut()
            .emit_signal(nid, "property_list_changed", &[]);
        true
    }

    fn animation_player_seek(&mut self, node: u64, time: f64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        match self.tree_mut().get_animation_player_mut(nid) {
            Some(player) => {
                player.seek(time, true);
                true
            }
            None => false,
        }
    }

    fn animation_player_is_playing(&self, node: u64) -> Option<bool> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().get_animation_player(nid).map(|p| p.playing)
    }

    fn animation_player_current(&self, node: u64) -> Option<String> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree()
            .get_animation_player(nid)
            .map(|p| p.current_animation().unwrap_or("").to_string())
    }

    fn animation_player_position(&self, node: u64) -> Option<f64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().get_animation_player(nid).map(|p| p.position())
    }

    // -- SceneTree control surface (pat-0q2kx.47) --------------------------
    //
    // Thin routing onto SceneTree's own implementations, which already existed
    // and were unit-tested; what was missing was any way for a script to reach
    // them. `EngineResult` collapses to a bool here because GDScript's
    // `change_scene_to_file` returns an Error code the idiomatic call discards.

    fn change_scene_to_file(&mut self, path: &str) -> bool {
        self.tree_mut().change_scene_to_file(path).is_ok()
    }

    fn reload_current_scene(&mut self) -> bool {
        self.tree_mut().reload_current_scene().is_ok()
    }

    fn quit(&mut self, exit_code: i64) {
        self.tree_mut().quit(exit_code as i32);
    }

    fn set_tree_paused(&mut self, paused: bool) {
        self.tree_mut().set_paused(paused);
    }

    /// `get_tree().set_multiplayer(api)` from a script (pat-s3rrv.230).
    ///
    /// The capability already existed and only Rust could reach it, so this is
    /// the spelling rather than the mechanism.
    fn set_script_multiplayer_api(&mut self, node: u64) -> bool {
        // Same identity story as `multiplayer_set_peer` above: the script's
        // identity is the node it is attached to.
        let node = crate::node::NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().set_script_multiplayer_api(node)
    }

    fn is_tree_paused(&self) -> bool {
        self.tree().paused()
    }

    fn tree_root(&self) -> Option<u64> {
        Some(self.tree().root_id().raw())
    }

    fn tree_node_count(&self) -> i64 {
        self.tree().node_count() as i64
    }

    fn nodes_in_group(&self, group: &str) -> Vec<u64> {
        self.tree()
            .get_nodes_in_group(group)
            .into_iter()
            .map(|id| id.raw())
            .collect()
    }

    fn first_node_in_group(&self, group: &str) -> Option<u64> {
        self.tree()
            .get_first_node_in_group(group)
            .map(|id| id.raw())
    }

    fn tree_has_group(&self, group: &str) -> bool {
        self.tree().has_group(group)
    }

    fn call_group_flags(&mut self, flags: i64, group: &str, method: &str, args: &[Variant]) {
        // A negative flag word cannot name any GROUP_CALL_* bit; clamp rather
        // than wrap it into a mask that would silently mean "reverse + deferred".
        self.tree_mut()
            .call_group_flags(flags.max(0) as u32, group, method, args);
    }

    fn set_group(&mut self, group: &str, property: &str, value: Variant) {
        self.tree_mut().set_group(group, property, value);
    }

    fn notify_group(&mut self, group: &str, what: i64) {
        self.tree_mut().notify_group(
            group,
            gdobject::notification::Notification::new(what as i32),
        );
    }

    // -- Physics query parameter constructors (pat-s3rrv.26) ---------------

    fn physics_query_static_call(
        &mut self,
        type_name: &str,
        method: &str,
        args: &[Variant],
    ) -> Option<Variant> {
        // Godot: `PhysicsRayQueryParameters2D.create(from, to)` — a static
        // factory returning a configured parameters object. `intersect_ray`
        // accepts nothing else, so without this the whole query surface is
        // unreachable from script no matter what ClassDB knows about it.
        match (type_name, method) {
            ("PhysicsRayQueryParameters2D", "create") => {
                let from = match args.first() {
                    Some(Variant::Vector2(v)) => *v,
                    _ => return None,
                };
                let to = match args.get(1) {
                    Some(Variant::Vector2(v)) => *v,
                    _ => return None,
                };
                Some(Variant::ObjectId(ObjectId::from_raw(
                    new_ray_params_handle(from, to),
                )))
            }
            // `CircleShape2D.new()` / `RectangleShape2D.new()` — the shape
            // resources `intersect_shape` takes. `gdphysics2d::Shape2D` has had
            // these forever and `shape_cast` has been correct and unit-tested,
            // but no script could build the argument, so the whole shape-query
            // half of PhysicsDirectSpaceState2D was unreachable (pat-s3rrv.26).
            //
            // Defaults are Godot's: radius 10, size 20x20 (= half-extents 10).
            ("CircleShape2D", "new") => Some(Variant::ObjectId(ObjectId::from_raw(
                new_shape_handle(gdphysics2d::shape::Shape2D::Circle { radius: 10.0 }),
            ))),
            ("RectangleShape2D", "new") => Some(Variant::ObjectId(ObjectId::from_raw(
                new_shape_handle(gdphysics2d::shape::Shape2D::Rectangle {
                    half_extents: gdcore::math::Vector2::new(10.0, 10.0),
                }),
            ))),
            // The remaining six Shape2D subclasses (pat-s3rrv.98). All were
            // already in ClassDB and already representable by
            // `gdphysics2d::shape::Shape2D`; only `.new()` was missing, so
            // `var s = CapsuleShape2D.new()` failed on a class the editor's own
            // resource picker offers.
            //
            // Defaults are Godot's, from each class's doc XML rather than
            // copied from the circle above:
            ("CapsuleShape2D", "new") => Some(Variant::ObjectId(ObjectId::from_raw(
                // ⚠️ height is the TOTAL height (30.0), not Godot's derived
                // `mid_height` (= height - 2 * radius). Storing mid_height here
                // would give a capsule 20px shorter than the authored one.
                new_shape_handle(gdphysics2d::shape::Shape2D::Capsule {
                    radius: 10.0,
                    height: 30.0,
                }),
            ))),
            ("SegmentShape2D", "new") => Some(Variant::ObjectId(ObjectId::from_raw(
                new_shape_handle(gdphysics2d::shape::Shape2D::Segment {
                    a: gdcore::math::Vector2::new(0.0, 0.0),
                    b: gdcore::math::Vector2::new(0.0, 10.0),
                }),
            ))),
            // ⚠️ Both polygon kinds default to NO points, which is Godot's
            // default and is a legal shape — geometry arrives later through the
            // `points` / `segments` setter. Seeding a placeholder quad instead
            // would make an unconfigured shape silently collide.
            ("ConvexPolygonShape2D", "new") => Some(Variant::ObjectId(ObjectId::from_raw(
                new_shape_handle(gdphysics2d::shape::Shape2D::convex_polygon(&[])),
            ))),
            ("ConcavePolygonShape2D", "new") => Some(Variant::ObjectId(ObjectId::from_raw(
                new_shape_handle(gdphysics2d::shape::Shape2D::concave_polygon(&[])),
            ))),
            ("WorldBoundaryShape2D", "new") => Some(Variant::ObjectId(ObjectId::from_raw(
                // ⚠️ The default normal is (0, -1) — pointing UP in Godot's
                // y-down 2D space, so the solid half-plane is everything below
                // the line. Flipping the sign leaves a "floor" that bodies fall
                // through while the shape still exists.
                new_shape_handle(gdphysics2d::shape::Shape2D::world_boundary(
                    gdcore::math::Vector2::new(0.0, -1.0),
                    0.0,
                )),
            ))),
            ("SeparationRayShape2D", "new") => Some(Variant::ObjectId(ObjectId::from_raw(
                new_shape_handle(gdphysics2d::shape::Shape2D::SeparationRay { length: 20.0 }),
            ))),
            ("PhysicsShapeQueryParameters2D", "new") => Some(Variant::ObjectId(
                ObjectId::from_raw(new_shape_params_handle()),
            )),
            _ => None,
        }
    }

    // -- AnimationNodeStateMachinePlayback (pat-s3rrv.1) --------------------

    fn animation_tree_playback_call(
        &mut self,
        handle: u64,
        method: &str,
        args: &[Variant],
    ) -> Option<Variant> {
        let (node, sm_name) = playback_handle_target(handle)?;
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));

        // `get_current_length` is the one method that needs the clip library
        // rather than the machine, so read it before the mutable borrow.
        if method == "get_current_length" {
            let tree = self.tree().get_animation_tree(nid)?;
            let sm = tree.state_machine(&sm_name)?;
            return Some(Variant::Float(sm.current_length(&tree.animations)));
        }

        let sm = self
            .tree_mut()
            .get_animation_tree_mut(nid)?
            .state_machine_mut(&sm_name)?;

        match method {
            // Godot: travel(to_node, reset_on_teleport = true). The path is
            // walked by `advance` over the following frames, using each hop's
            // declared crossfade — a travel is not an instant jump.
            "travel" => {
                let target = variant_string(args.first()?)?;
                sm.travel(&target);
                Some(Variant::Nil)
            }
            "start" => {
                let target = variant_string(args.first()?)?;
                let reset = match args.get(1) {
                    Some(Variant::Bool(b)) => *b,
                    Some(Variant::Int(i)) => *i != 0,
                    // Godot's default.
                    _ => true,
                };
                sm.start(&target, reset);
                Some(Variant::Nil)
            }
            "next" | "travel_to_next_blend" => {
                sm.next_state();
                Some(Variant::Nil)
            }
            "stop" => {
                sm.stop();
                Some(Variant::Nil)
            }
            "is_playing" => Some(Variant::Bool(sm.is_playing())),
            "get_current_node" => Some(Variant::String(sm.current_node().to_string())),
            "get_travel_path" => Some(Variant::Array(
                sm.travel_path_remaining()
                    .iter()
                    .map(|s| Variant::String(s.clone()))
                    .collect(),
            )),
            // Godot answers an empty StringName when no fade is in flight.
            "get_fading_from_node" => Some(Variant::String(
                sm.fading_from_node().unwrap_or_default().to_string(),
            )),
            "get_current_play_position" => Some(Variant::Float(sm.current_play_position())),
            // Not a playback method: fall through so the interpreter reports it
            // rather than swallowing a typo as a no-op.
            _ => None,
        }
    }

    // -- Tween (pat-0q2kx.4) -----------------------------------------------

    fn create_tween(&mut self, node: u64) -> Option<u64> {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        let tree = self.tree_mut();
        tree.get_node(nid)?;
        let id = tree.create_tween(nid);
        // Godot's `create_tween()` hands back an already-running tween: the
        // script chains tweeners onto it and it advances on the next frame
        // without an explicit `play()`.
        if let Some(t) = tree.get_tween_mut(id) {
            t.start();
        }
        Some(id.raw())
    }

    fn tween_call(&mut self, tween: u64, method: &str, args: &[Variant]) -> Option<Variant> {
        let tid = TweenId::from_raw(tween);
        // The identity check that makes this safe to try before node dispatch:
        // an id that names no live tween returns `None` and the interpreter
        // falls through to ordinary node semantics.
        self.tree_mut().get_tween_mut(tid)?;

        // `self` handle, so the fluent chain keeps flowing:
        // `create_tween().tween_property(...).set_trans(...).set_ease(...)`.
        let handle = Variant::ObjectId(ObjectId::from_raw(tween));

        match method {
            // -- tweeners ---------------------------------------------------
            // Godot: tween_property(object, property, final_val, duration)
            "tween_property" => {
                let target = variant_object_raw(args.first()?)?;
                let property = variant_string(args.get(1)?)?;
                let final_value = args.get(2)?.clone();
                let duration = variant_number(args.get(3)?)?;
                // Godot samples the start value when the tweener *starts*; the
                // tween is created and stepped in the same frame here, so the
                // node's present value is that same sample.
                let start = {
                    let nid = NodeId::from_object_id(ObjectId::from_raw(target));
                    let (prop, sub) = crate::tween::split_property_path(&property);
                    let base = self
                        .tree()
                        .get_node(nid)
                        .map(|n| n.get_property(prop))
                        .unwrap_or(Variant::Nil);
                    match sub {
                        // `"position:x"` interpolates a float component, so the
                        // start value must be that component — not the whole
                        // Vector2, which would make `interpolate_variant` bail
                        // on the type mismatch against the float end value.
                        Some(s) => {
                            crate::tween::read_subname(&base, s).unwrap_or(Variant::Float(0.0))
                        }
                        None => base,
                    }
                };
                let step =
                    TweenStep::new(property, start, final_value, duration).with_target(target);
                self.push_tween_step(tid, step)?;
                Some(handle)
            }
            // Godot: tween_method(callable, from, to, duration). Patina carries
            // the callable as a plain method name on the owning node's script.
            "tween_method" => {
                let m = variant_callable_method(args.first()?)?;
                let from = args.get(1)?.clone();
                let to = args.get(2)?.clone();
                let duration = variant_number(args.get(3)?)?;
                self.push_tween_step(tid, TweenStep::method(m, from, to, duration))?;
                Some(handle)
            }
            "tween_callback" => {
                let m = variant_callable_method(args.first()?)?;
                self.push_tween_step(tid, TweenStep::callback(m))?;
                Some(handle)
            }
            "tween_interval" => {
                let secs = variant_number(args.first()?)?;
                self.push_tween_step(tid, TweenStep::interval(secs))?;
                Some(handle)
            }

            // -- per-tweener setters ----------------------------------------
            //
            // Godot returns the *Tweener* from `tween_property`, and these
            // setters apply to it. Patina returns the tween handle, so they
            // apply to the most recently added step — which is the same step
            // in every fluent chain.
            "set_ease" => {
                let v = variant_number(args.first()?)? as i64;
                self.with_last_step(tid, |s| s.ease_type = ease_type_from_i64(v))?;
                Some(handle)
            }
            "set_trans" => {
                let v = variant_number(args.first()?)? as i64;
                self.with_last_step(tid, |s| s.trans_func = trans_func_from_i64(v))?;
                Some(handle)
            }
            "set_delay" => {
                let v = variant_number(args.first()?)?;
                self.with_last_step(tid, |s| s.delay = v)?;
                Some(handle)
            }

            // -- sequencing ---------------------------------------------------
            //
            // `parallel()` makes the *next* tweener run alongside the previous
            // one; Godot also accepts it retroactively on the last one, which is
            // how the fluent form `.tween_property(..).parallel()` reads.
            "parallel" => {
                self.with_last_step(tid, |s| s.parallel = true)?;
                Some(handle)
            }
            "chain" => {
                self.with_last_step(tid, |s| s.parallel = false)?;
                Some(handle)
            }
            // Godot's `set_parallel(enabled = true)`: a MODE, not a one-shot.
            // Every tweener added afterwards runs alongside the previous one
            // until it is turned off — `parallel()` above only affects the next.
            "set_parallel" => {
                let enabled = !matches!(args.first(), Some(Variant::Bool(false)));
                let tween = self.tree_mut().get_tween_mut(tid)?;
                tween.default_parallel = enabled;
                Some(handle)
            }

            // -- tween-wide controls ------------------------------------------
            "set_loops" => {
                let n = args
                    .first()
                    .and_then(variant_number)
                    .map(|v| v as i32)
                    // Godot: `set_loops()` with no argument loops forever.
                    .unwrap_or(-1);
                self.tree_mut().get_tween_mut(tid)?.set_loops(n);
                Some(handle)
            }
            "set_speed_scale" => {
                let v = variant_number(args.first()?)?;
                self.tree_mut().get_tween_mut(tid)?.set_speed_scale(v);
                Some(handle)
            }
            "play" => {
                self.tree_mut().get_tween_mut(tid)?.play();
                Some(Variant::Nil)
            }
            "pause" => {
                self.tree_mut().get_tween_mut(tid)?.pause();
                Some(Variant::Nil)
            }
            "stop" => {
                self.tree_mut().get_tween_mut(tid)?.stop();
                Some(Variant::Nil)
            }
            "kill" => {
                self.tree_mut().get_tween_mut(tid)?.kill();
                Some(Variant::Nil)
            }
            "is_running" => {
                let running = self.tree_mut().get_tween_mut(tid)?.running;
                Some(Variant::Bool(running))
            }
            "is_valid" => {
                let valid = self.tree_mut().get_tween_mut(tid)?.is_valid();
                Some(Variant::Bool(valid))
            }
            // -- PropertyTweener start-value verbs (pat-s3rrv.46) -----------
            //
            // Godot's `tween_property(...).from(v)` / `.from_current()` /
            // `.as_relative()` all rewrite the tweener's START value, so they
            // land on the most recently added step exactly like the setters
            // above. ClassDB advertised all three and a call raised
            // `UndefinedFunction` until this bead.
            "from" => {
                let v = args.first()?.clone();
                self.with_last_step(tid, |s| s.start_value = v)?;
                Some(handle)
            }
            "from_current" => {
                // Re-sample the target's property NOW. `tween_property` already
                // sampled at creation, so this only differs when the chain moved
                // the node in between — which is precisely why Godot has it.
                let target = {
                    let tween = self.tree_mut().get_tween_mut(tid)?;
                    let step = tween.steps.last()?;
                    (step.target, step.property_path.clone())
                };
                let (raw, path) = target;
                let owner = self.tree().tween_owner(tid)?;
                let nid = match raw {
                    Some(r) => NodeId::from_object_id(ObjectId::from_raw(r)),
                    None => owner,
                };
                let (prop, sub) = crate::tween::split_property_path(&path);
                let base = self
                    .tree()
                    .get_node(nid)
                    .map(|n| n.get_property(prop))
                    .unwrap_or(Variant::Nil);
                let current = match sub {
                    Some(sn) => {
                        crate::tween::read_subname(&base, sn).unwrap_or(Variant::Float(0.0))
                    }
                    None => base,
                };
                self.with_last_step(tid, |s| s.start_value = current)?;
                Some(handle)
            }
            "as_relative" => {
                // The final value becomes an OFFSET from the start value, so
                // `tween_property(n, "position", Vector2(10, 0), 1).as_relative()`
                // ends at start + (10, 0) instead of at (10, 0).
                self.with_last_step(tid, |s| {
                    if let Some(sum) = crate::tween::add_variants(&s.start_value, &s.end_value) {
                        s.end_value = sum;
                    }
                })?;
                Some(handle)
            }

            // -- subtween (pat-s3rrv.46) ------------------------------------
            //
            // `TweenStep::subtween` existed and nothing could reach it: the
            // nested tween arrives as another live tween handle, which is taken
            // out of the tree so the parent owns it — Godot's
            // `tween_subtween(subtween)` does the same.
            "tween_subtween" => {
                let raw = variant_object_raw(args.first()?)?;
                let sub_id = TweenId::from_raw(raw);
                // A handle that names no live tween is not a subtween.
                let nested = self.tree_mut().take_tween(sub_id)?;
                self.push_tween_step(tid, TweenStep::subtween(nested))?;
                Some(handle)
            }

            // -- process / pause modes (pat-s3rrv.46) ------------------------
            "set_process_mode" => {
                let v = variant_number(args.first()?)? as i64;
                self.tree_mut()
                    .get_tween_mut(tid)?
                    .set_process_mode(TweenProcessMode::from_i64(v));
                Some(handle)
            }
            "set_pause_mode" => {
                let v = variant_number(args.first()?)? as i64;
                self.tree_mut()
                    .get_tween_mut(tid)?
                    .set_pause_mode(TweenPauseMode::from_i64(v));
                Some(handle)
            }
            // Godot's `set_ignore_time_scale(ignore = true)`.
            "set_ignore_time_scale" => {
                let ignore = !matches!(args.first(), Some(Variant::Bool(false)));
                self.tree_mut()
                    .get_tween_mut(tid)?
                    .set_ignore_time_scale(ignore);
                Some(handle)
            }
            "bind_node" => {
                let raw = variant_object_raw(args.first()?)?;
                self.tree_mut().get_tween_mut(tid)?.bind_node(raw);
                Some(handle)
            }

            // -- introspection / manual stepping (pat-s3rrv.46) --------------
            "get_total_elapsed_time" => {
                let t = self.tree_mut().get_tween_mut(tid)?.get_total_elapsed_time();
                Some(Variant::Float(t))
            }
            "get_loops_left" => {
                let n = self.tree_mut().get_tween_mut(tid)?.get_loops_left();
                Some(Variant::Int(n as i64))
            }
            // `custom_step(delta)` advances the tween by hand and reports
            // whether it is still running. The property writes land on the next
            // `process_tweens` pass, which is the same frame's phase.
            "custom_step" => {
                let delta = variant_number(args.first()?)?;
                let running = self.tree_mut().get_tween_mut(tid)?.custom_step(delta);
                Some(Variant::Bool(running))
            }
            // Godot's static `Tween.interpolate_value(initial, delta, elapsed,
            // duration, trans_type, ease_type)`. Reached through a live handle
            // here because Patina has no static-call path onto Tween.
            "interpolate_value" => {
                let initial = args.first()?.clone();
                let delta = args.get(1)?.clone();
                let elapsed = variant_number(args.get(2)?)?;
                let duration = variant_number(args.get(3)?)?;
                let trans = trans_func_from_i64(variant_number(args.get(4)?)? as i64);
                let ease_type = ease_type_from_i64(variant_number(args.get(5)?)? as i64);
                Some(Tween::interpolate_value(
                    &initial, &delta, elapsed, duration, trans, ease_type,
                ))
            }

            // A live tween asked for a method Tween does not have: fall through
            // so the interpreter reports `UndefinedFunction` as it would for any
            // other object, rather than silently returning Nil.
            _ => None,
        }
    }

    fn instantiate_packed_scene(&mut self, path: &str) -> Option<u64> {
        // `res://` resolves against the configured project root, exactly like
        // `SceneTree::change_scene_to_file`. Hand-rolling
        // `strip_prefix("res://")` here instead mapped `res://X` to the
        // CWD-relative `X`, so `preload("res://bullet.tscn").instantiate()` —
        // the way every real game spawns — only worked when the process
        // happened to be running from the project root (pat-u3l8c.12).
        // An unconfigured root is an Err here, which becomes `null` at the
        // call site, matching the "missing/unloadable scene → null" contract.
        // Read through `FileAccess`, which serves a mounted `.pck` before disk
        // (pat-s3rrv.34). The raw `std::fs::read_to_string` this replaces made
        // `preload("res://bullet.tscn").instantiate()` fail outright in an
        // exported game, where the scene exists only inside the pack.
        let source = gdcore::file_access::FileAccess::read_file_string(path).ok()?;
        let scene = crate::packed_scene::PackedScene::from_tscn(&source).ok()?;
        // Recursively resolve `instance=ExtResource` sub-scenes the same way.
        let resolve = |sub: &str| -> Option<crate::packed_scene::PackedScene> {
            let src = gdcore::file_access::FileAccess::read_file_string(sub).ok()?;
            crate::packed_scene::PackedScene::from_tscn(&src).ok()
        };
        let tree = self.tree_mut();
        let root = tree.root_id();
        // The subtree is attached under the root here; the script's follow-up
        // `add_child` reparents it to the desired parent (see `add_child`).
        crate::packed_scene::add_packed_scene_to_tree_with_subscenes(tree, root, &scene, &resolve)
            .ok()
            .map(|id| id.raw())
    }

    fn instantiate_script(&mut self, path: &str) -> Option<u64> {
        // `res://` resolves against the project root (mirrors
        // `instantiate_packed_scene`); see the note there on why the previous
        // CWD-relative mapping broke `load("res://enemy.gd").new()`.
        // `resolved` is still needed for the instance name below, but the bytes
        // come from `FileAccess` so a packed script loads too (pat-s3rrv.34).
        let resolved = gdcore::resolve_path(path).ok()?;
        let source = gdcore::file_access::FileAccess::read_file_string(path).ok()?;
        let base_class = script_base_class(&source);
        // Name the instance after the script file stem, matching how a
        // developer thinks of `load("enemy.gd").new()`.
        let name = resolved
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Script")
            .to_string();
        let tree = self.tree_mut();
        let root = tree.root_id();
        // Attach under the root; the script's follow-up `add_child` reparents
        // it to the desired parent (see `add_child`), mirroring scenes.
        let node_id = tree
            .add_child(root, crate::node::Node::new(name, base_class))
            .ok()?;
        match GDScriptNodeInstance::from_source(&source, node_id) {
            Ok(instance) => tree.attach_script(node_id, Box::new(instance)),
            // Non-fatal: the node exists without a live script (Godot logs and
            // still yields an object); behaviour tests use a parseable script.
            Err(_) => {}
        }
        Some(node_id.raw())
    }

    // -- AStar2D / AStar3D / AStarGrid2D (pat-s3rrv.19) --------------------
    //
    // `crate::astar` has been a complete, unit-tested pathfinder since
    // pat-p0s82, with ClassDB metadata registered — and zero callers. The
    // instances live here rather than in the interpreter's handle tables
    // because `gdscript-interop` cannot name a `gdscene` type (this crate
    // depends on it, not the reverse), so the trait is the inversion seam.

    fn polygon_path_finder_new(&mut self) -> Option<u64> {
        POLYGON_PATH_FINDER_REGISTRY.with(|r| {
            let mut reg = r.borrow_mut();
            let seq = reg.next_seq;
            reg.next_seq += 1;
            let raw = gdscript_interop::bindings::POLYGON_PATH_FINDER_HANDLE_TAG | seq;
            reg.finders
                .insert(raw, crate::polygon_path_finder::PolygonPathFinder::new());
            Some(raw)
        })
    }

    fn polygon_path_finder_call(
        &mut self,
        handle: u64,
        method: &str,
        args: &[Variant],
    ) -> Option<Variant> {
        let vec2 = |i: usize| match args.get(i) {
            Some(Variant::Vector2(v)) => *v,
            _ => gdcore::math::Vector2::ZERO,
        };
        POLYGON_PATH_FINDER_REGISTRY.with(|r| {
            let mut reg = r.borrow_mut();
            let f = reg.finders.get_mut(&handle)?;
            Some(match method {
                // Godot: `setup(points: PackedVector2Array, connections:
                // PackedInt32Array)`. The connections are a **flat** list of
                // index pairs, while the module takes tuples — adapting that is
                // the whole reason this lives on the scene side.
                //
                // An odd trailing index is dropped rather than paired with
                // nothing: `chunks_exact(2)` ignores the remainder, which
                // matches Godot treating the array as pairs.
                "setup" => {
                    let points = match args.first() {
                        Some(Variant::PackedVector2Array(p)) => p.clone(),
                        _ => Vec::new(),
                    };
                    let flat = match args.get(1) {
                        Some(Variant::PackedInt32Array(c)) => c.clone(),
                        _ => Vec::new(),
                    };
                    let pairs: Vec<(usize, usize)> = flat
                        .chunks_exact(2)
                        .filter(|c| c[0] >= 0 && c[1] >= 0)
                        .map(|c| (c[0] as usize, c[1] as usize))
                        .collect();
                    f.setup(points, &pairs);
                    Variant::Nil
                }
                "find_path" => Variant::PackedVector2Array(f.find_path(vec2(0), vec2(1))),
                "get_intersections" => {
                    Variant::PackedVector2Array(f.get_intersections(vec2(0), vec2(1)))
                }
                "is_point_inside" => Variant::Bool(f.is_point_inside(vec2(0))),
                // Godot returns a zero Vector2 when there are no points at all,
                // rather than failing — a script asking an empty finder gets a
                // usable value.
                "get_closest_point" => Variant::Vector2(
                    f.get_closest_point(vec2(0))
                        .unwrap_or(gdcore::math::Vector2::ZERO),
                ),
                // Godot's `get_bounds` is a `Rect2`; the module reports the
                // min/max extent, so this converts rather than reshaping the
                // module. An empty finder yields a zero rect, matching Godot's
                // default-constructed `bounds`.
                "get_bounds" => {
                    let (pos, size) = match f.bounds() {
                        Some((min, max)) => (min, max - min),
                        None => (gdcore::math::Vector2::ZERO, gdcore::math::Vector2::ZERO),
                    };
                    Variant::Rect2(gdcore::math::Rect2 {
                        position: pos,
                        size,
                    })
                }
                "set_point_penalty" => {
                    let idx = match args.first() {
                        Some(Variant::Int(i)) if *i >= 0 => *i as usize,
                        _ => return None,
                    };
                    let penalty = match args.get(1) {
                        Some(Variant::Float(v)) => *v as f32,
                        Some(Variant::Int(v)) => *v as f32,
                        _ => 0.0,
                    };
                    f.set_point_penalty(idx, penalty);
                    Variant::Nil
                }
                "get_point_penalty" => {
                    let idx = match args.first() {
                        Some(Variant::Int(i)) if *i >= 0 => *i as usize,
                        _ => return None,
                    };
                    Variant::Float(f64::from(f.get_point_penalty(idx)))
                }
                _ => return None,
            })
        })
    }

    fn astar_new(&mut self, class_name: &str) -> Option<u64> {
        ASTAR_REGISTRY.with(|r| {
            let mut reg = r.borrow_mut();
            let seq = reg.next_seq;
            reg.next_seq += 1;
            let raw = gdscript_interop::bindings::ASTAR_HANDLE_TAG | seq;
            match class_name {
                "AStar2D" => {
                    reg.astar2d.insert(raw, crate::astar::AStar2D::new());
                }
                "AStar3D" => {
                    reg.astar3d.insert(raw, crate::astar::AStar3D::new());
                }
                "AStarGrid2D" => {
                    reg.grid.insert(raw, crate::astar::AStarGrid2D::new());
                }
                _ => return None,
            }
            Some(raw)
        })
    }

    fn mesh_builder_new(&mut self, class_name: &str) -> Option<u64> {
        MESH_BUILDER_REGISTRY.with(|r| {
            let mut reg = r.borrow_mut();
            let seq = reg.next_seq;
            reg.next_seq += 1;
            let raw = gdscript_interop::bindings::MESH_BUILDER_HANDLE_TAG | seq;
            match class_name {
                "ImmediateMesh" => {
                    reg.immediate
                        .insert(raw, gdserver3d::immediate_mesh::ImmediateMesh::new());
                }
                "SurfaceTool" => {
                    reg.surface
                        .insert(raw, gdserver3d::mesh::SurfaceTool::new());
                }
                _ => return None,
            }
            Some(raw)
        })
    }

    fn mesh_builder_call(
        &mut self,
        handle: u64,
        method: &str,
        args: &[Variant],
    ) -> Option<Variant> {
        MESH_BUILDER_REGISTRY.with(|r| {
            let mut reg = r.borrow_mut();
            if let Some(m) = reg.immediate.get_mut(&handle) {
                return immediate_mesh_call(m, method, args);
            }
            if let Some(t) = reg.surface.get_mut(&handle) {
                // ⚠️ `commit` is answered HERE, not in `surface_tool_call`,
                // because it must return the handle and only this scope has it
                // (pat-s3rrv.201).
                //
                // Godot's `commit()` returns a fresh ArrayMesh. Here the tool's
                // own handle IS the mesh reference: `resolve_mesh` reads the
                // geometry back through `surface_tool_geometry`. The visible
                // difference is that later edits to the tool keep affecting an
                // already-committed mesh, where Godot's snapshot would not.
                if method == "commit" {
                    if t.vertex_count() == 0 {
                        // Nothing built: Godot returns null, and handing back a
                        // handle here would render an empty instance instead.
                        return Some(Variant::Nil);
                    }
                    return Some(Variant::ObjectId(ObjectId::from_raw(handle)));
                }
                return surface_tool_call(t, method, args);
            }
            None
        })
    }

    fn astar_call(&mut self, handle: u64, method: &str, args: &[Variant]) -> Option<Variant> {
        ASTAR_REGISTRY.with(|r| {
            let mut reg = r.borrow_mut();
            if let Some(a) = reg.astar2d.get_mut(&handle) {
                return astar_call_2d(a, method, args);
            }
            if let Some(a) = reg.astar3d.get_mut(&handle) {
                return astar_call_3d(a, method, args);
            }
            if let Some(g) = reg.grid.get_mut(&handle) {
                return astar_call_grid(g, method, args);
            }
            None
        })
    }
}

/// Live AStar objects, keyed by the handle the interpreter carries.
/// `ImmediateMesh` and `SurfaceTool` instances owned by the scene layer
/// (pat-s3rrv.197), keyed by the tagged handle scripts hold.
///
/// Mirrors [`AStarRegistry`]: both classes are RefCounted in Godot and both live
/// in `gdserver3d`, which `gdscript-interop` cannot name, so the scene layer
/// owns them and the interpreter routes on the tag alone.
#[derive(Default)]
struct MeshBuilderRegistry {
    immediate: std::collections::HashMap<u64, gdserver3d::immediate_mesh::ImmediateMesh>,
    surface: std::collections::HashMap<u64, gdserver3d::mesh::SurfaceTool>,
    next_seq: u64,
}

thread_local! {
    static MESH_BUILDER_REGISTRY: std::cell::RefCell<MeshBuilderRegistry> =
        std::cell::RefCell::new(MeshBuilderRegistry {
            next_seq: 1,
            ..Default::default()
        });
}

#[derive(Default)]
struct AStarRegistry {
    astar2d: std::collections::HashMap<u64, crate::astar::AStar2D>,
    astar3d: std::collections::HashMap<u64, crate::astar::AStar3D>,
    grid: std::collections::HashMap<u64, crate::astar::AStarGrid2D>,
    next_seq: u64,
}

/// Live `PolygonPathFinder` instances behind their handles (pat-s3rrv.240).
///
/// Owned here rather than in the interpreter for the reason AStar records: the
/// type lives in `gdscene`, and `gdscene` depends on `gdscript-interop`, not the
/// reverse.
#[derive(Default)]
struct PolygonPathFinderRegistry {
    finders: std::collections::HashMap<u64, crate::polygon_path_finder::PolygonPathFinder>,
    next_seq: u64,
}

thread_local! {
    static POLYGON_PATH_FINDER_REGISTRY: std::cell::RefCell<PolygonPathFinderRegistry> =
        std::cell::RefCell::new(PolygonPathFinderRegistry {
            next_seq: 1,
            ..Default::default()
        });
}

thread_local! {
    static ASTAR_REGISTRY: std::cell::RefCell<AStarRegistry> =
        std::cell::RefCell::new(AStarRegistry {
            next_seq: 1,
            ..Default::default()
        });
}

/// The renderable geometry an `ImmediateMesh` handle currently holds
/// (pat-s3rrv.197), or `None` when it has no finished surfaces.
///
/// ⚠️ This accessor exists because `MESH_BUILDER_REGISTRY` is a PRIVATE
/// thread-local in this module, while the code that needs it —
/// `RenderServer3DAdapter::resolve_mesh` — lives in `render_server_3d.rs`.
/// Without it that resolver cannot see the geometry at all: `get_mesh_path`
/// matches only `Variant::String`, so an `ObjectId` handle assigned to
/// `MeshInstance3D.mesh` returns `None` and the node renders a DEFAULT CUBE.
///
/// That failure is deceptive rather than loud — the instance exists and carries
/// exactly one surface, so a check that only counts surfaces passes against
/// geometry that ignored the script entirely. Only the vertex count betrays it.
///
/// `None` for a cleared mesh is deliberate and is what makes
/// `clear_surfaces()` mean "renders nothing" instead of handing the renderer an
/// empty husk that still counts as a live instance.
pub(crate) fn immediate_mesh_geometry(handle: u64) -> Option<gdserver3d::mesh::Mesh3D> {
    MESH_BUILDER_REGISTRY.with(|r| r.borrow().immediate.get(&handle).and_then(|m| m.to_mesh()))
}

/// The mesh a `SurfaceTool` handle has accumulated so far (pat-s3rrv.197).
///
/// Godot's `SurfaceTool.commit()` returns an ArrayMesh; this is the same data
/// for the same reason as [`immediate_mesh_geometry`] — the registry is private
/// to this module. Returns `None` for a tool with no vertices, so an
/// uncommitted builder does not render an empty instance.
pub(crate) fn surface_tool_geometry(handle: u64) -> Option<gdserver3d::mesh::Mesh3D> {
    MESH_BUILDER_REGISTRY.with(|r| {
        let reg = r.borrow();
        let tool = reg.surface.get(&handle)?;
        if tool.vertex_count() == 0 {
            return None;
        }
        Some(tool.commit())
    })
}

/// Dispatches an `ImmediateMesh` method (pat-s3rrv.197).
///
/// ⚠️ Errors are reported as `Variant::Bool(false)`, never by silently doing
/// nothing. Godot ERRORS when `surface_add_vertex` is called outside a
/// begin/end pair, and a silent no-op would let a script build geometry it
/// believes exists — the acceptance calls this out explicitly.
fn immediate_mesh_call(
    mesh: &mut gdserver3d::immediate_mesh::ImmediateMesh,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let vec3 = |i: usize| match args.get(i) {
        Some(Variant::Vector3(v)) => *v,
        _ => gdcore::math::Vector3::ZERO,
    };
    let ok = |r: Result<(), gdserver3d::immediate_mesh::ImmediateMeshError>| {
        Some(Variant::Bool(r.is_ok()))
    };
    match method {
        "surface_begin" => {
            let primitive = match args.first() {
                Some(Variant::Int(i)) => *i,
                Some(Variant::Float(f)) => *f as i64,
                // Godot's own default for `surface_begin` is PRIMITIVE_TRIANGLES
                // (3), NOT 0 — 0 is PRIMITIVE_POINTS, so defaulting to zero
                // would draw a point cloud for a call that omitted the argument.
                _ => 3,
            };
            ok(mesh.surface_begin(primitive))
        }
        "surface_set_normal" => ok(mesh.surface_set_normal(vec3(0))),
        "surface_set_uv" => {
            let uv = match args.first() {
                Some(Variant::Vector2(v)) => [v.x, v.y],
                _ => [0.0, 0.0],
            };
            ok(mesh.surface_set_uv(uv))
        }
        "surface_set_color" => {
            let c = match args.first() {
                Some(Variant::Color(c)) => *c,
                _ => gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0),
            };
            ok(mesh.surface_set_color(c))
        }
        "surface_add_vertex" => ok(mesh.surface_add_vertex(vec3(0))),
        "surface_end" => ok(mesh.surface_end()),
        "clear_surfaces" => {
            mesh.clear_surfaces();
            Some(Variant::Nil)
        }
        "get_surface_count" => Some(Variant::Int(mesh.surface_count() as i64)),
        _ => None,
    }
}

/// Dispatches a `SurfaceTool` method (pat-s3rrv.197).
fn surface_tool_call(
    tool: &mut gdserver3d::mesh::SurfaceTool,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let vec3 = |i: usize| match args.get(i) {
        Some(Variant::Vector3(v)) => *v,
        _ => gdcore::math::Vector3::ZERO,
    };
    match method {
        "begin" => {
            let primitive = match args.first() {
                Some(Variant::Int(i)) => *i,
                _ => 3,
            };
            // Same ordinal table as ImmediateMesh: TRIANGLES is 3, not 2.
            match gdserver3d::immediate_mesh::primitive_from_godot_int(primitive) {
                Some(p) => {
                    tool.begin(p);
                    Some(Variant::Bool(true))
                }
                None => Some(Variant::Bool(false)),
            }
        }
        "set_normal" => {
            tool.set_normal(vec3(0));
            Some(Variant::Nil)
        }
        "set_uv" => {
            let uv = match args.first() {
                Some(Variant::Vector2(v)) => [v.x, v.y],
                _ => [0.0, 0.0],
            };
            tool.set_uv(uv);
            Some(Variant::Nil)
        }
        "add_vertex" => {
            tool.add_vertex(vec3(0));
            Some(Variant::Nil)
        }
        "add_index" => {
            let i = match args.first() {
                Some(Variant::Int(i)) => *i.max(&0) as u32,
                _ => 0,
            };
            tool.add_index(i);
            Some(Variant::Nil)
        }
        // ⚠️ `set_color` and `clear` were ClassDB-REGISTERED with no dispatch
        // arm (pat-s3rrv.201). This dispatcher errors loudly on an unknown
        // method rather than returning Nil, so they failed noisily — but they
        // failed, and `has_method` answered true for both.
        "set_color" => {
            let Some(Variant::Color(c)) = args.first() else {
                return Some(Variant::Bool(false));
            };
            tool.set_color(*c);
            Some(Variant::Nil)
        }
        "clear" => {
            tool.clear();
            Some(Variant::Nil)
        }
        "commit_to_arrays" => {
            // Packing lives in `script_mesh` beside its inverse
            // (`mesh_from_mesh_array`) so the two can be round-trip tested
            // against each other rather than only through a script.
            Some(Variant::Array(crate::script_mesh::mesh_to_mesh_array(
                &tool.commit(),
                tool.colors(),
            )))
        }
        "set_smooth_group" => {
            let group = match args.first() {
                Some(Variant::Int(i)) => *i as u32,
                _ => return Some(Variant::Bool(false)),
            };
            tool.set_smooth_group(group);
            Some(Variant::Nil)
        }
        "set_bones" => {
            let bones: Vec<i32> = match args.first() {
                Some(Variant::PackedInt32Array(v)) => v.clone(),
                _ => return Some(Variant::Bool(false)),
            };
            tool.set_bones(&bones);
            Some(Variant::Nil)
        }
        "set_weights" => {
            let weights: Vec<f32> = match args.first() {
                Some(Variant::PackedFloat32Array(v)) => v.clone(),
                _ => return Some(Variant::Bool(false)),
            };
            tool.set_weights(&weights);
            Some(Variant::Nil)
        }
        "add_triangle_fan" => {
            // Godot's signature is (vertices, uvs, colors, uv2s, normals,
            // tangents); uv2 and tangent have no channel in Mesh3D, so those
            // two positions are accepted and ignored rather than rejected.
            let verts = match args.first() {
                Some(Variant::PackedVector3Array(v)) => v.clone(),
                _ => return Some(Variant::Bool(false)),
            };
            let uvs: Vec<[f32; 2]> = match args.get(1) {
                Some(Variant::PackedVector2Array(v)) => v.iter().map(|p| [p.x, p.y]).collect(),
                _ => Vec::new(),
            };
            let colors: Vec<gdcore::math::Color> = match args.get(2) {
                Some(Variant::PackedColorArray(v)) => v.clone(),
                _ => Vec::new(),
            };
            let normals: Vec<gdcore::math::Vector3> = match args.get(4) {
                Some(Variant::PackedVector3Array(v)) => v.clone(),
                _ => Vec::new(),
            };
            Some(Variant::Bool(
                tool.add_triangle_fan(&verts, &uvs, &colors, &normals),
            ))
        }
        "index" => {
            tool.index();
            Some(Variant::Nil)
        }
        "deindex" => {
            tool.deindex();
            Some(Variant::Nil)
        }
        "generate_normals" => {
            // Godot's parameter is `flip`, defaulting to false.
            let flip = matches!(args.first(), Some(Variant::Bool(true)));
            tool.generate_normals(flip);
            Some(Variant::Nil)
        }
        "get_vertex_count" => Some(Variant::Int(tool.vertex_count() as i64)),
        _ => None,
    }
}

fn astar_int(args: &[Variant], i: usize) -> i64 {
    match args.get(i) {
        Some(Variant::Int(v)) => *v,
        Some(Variant::Float(f)) => *f as i64,
        _ => 0,
    }
}

fn astar_f32(args: &[Variant], i: usize, default: f32) -> f32 {
    match args.get(i) {
        Some(Variant::Float(f)) => *f as f32,
        Some(Variant::Int(v)) => *v as f32,
        _ => default,
    }
}

/// Godot defaults `bidirectional` to `true` on connect/disconnect/are_connected.
fn astar_bool(args: &[Variant], i: usize, default: bool) -> bool {
    match args.get(i) {
        Some(Variant::Bool(b)) => *b,
        _ => default,
    }
}

fn astar_v2(args: &[Variant], i: usize) -> gdcore::math::Vector2 {
    match args.get(i) {
        Some(Variant::Vector2(v)) => *v,
        _ => gdcore::math::Vector2::new(0.0, 0.0),
    }
}

fn astar_v3(args: &[Variant], i: usize) -> gdcore::math::Vector3 {
    match args.get(i) {
        Some(Variant::Vector3(v)) => *v,
        _ => gdcore::math::Vector3::new(0.0, 0.0, 0.0),
    }
}

/// A `Vector2i` cell id. Scripts also spell these as integer-valued `Vector2`s.
fn astar_v2i(args: &[Variant], i: usize) -> gdcore::math::Vector2i {
    match args.get(i) {
        Some(Variant::Vector2i(v)) => *v,
        Some(Variant::Vector2(v)) => gdcore::math::Vector2i::new(v.x as i32, v.y as i32),
        _ => gdcore::math::Vector2i::new(0, 0),
    }
}

/// The half of the AStar surface that does not mention the vector type.
/// Returns `None` for methods handled by the per-dimension wrappers.
fn astar_call_shared<V: crate::astar::AStarVector>(
    a: &mut crate::astar::AStar<V>,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    match method {
        "has_point" => Some(Variant::Bool(a.has_point(astar_int(args, 0)))),
        "remove_point" => {
            a.remove_point(astar_int(args, 0));
            Some(Variant::Nil)
        }
        "get_point_count" => Some(Variant::Int(a.get_point_count() as i64)),
        // pat-s3rrv.19: the id/capacity surface. `get_available_point_id` is
        // the one scripts reach for constantly — `a.add_point(a.get_available_point_id(), pos)`.
        "get_available_point_id" => Some(Variant::Int(a.get_available_point_id())),
        "get_point_capacity" => Some(Variant::Int(a.get_point_capacity() as i64)),
        "reserve_space" => {
            let n = astar_int(args, 0).max(0) as usize;
            a.reserve_space(n);
            Some(Variant::Nil)
        }
        "get_point_ids" => Some(Variant::PackedInt64Array(a.get_point_ids())),
        "get_point_weight_scale" => Some(Variant::Float(
            a.get_point_weight_scale(astar_int(args, 0)).unwrap_or(0.0) as f64,
        )),
        "set_point_weight_scale" => {
            a.set_point_weight_scale(astar_int(args, 0), astar_f32(args, 1, 1.0));
            Some(Variant::Nil)
        }
        "set_point_disabled" => {
            a.set_point_disabled(astar_int(args, 0), astar_bool(args, 1, true));
            Some(Variant::Nil)
        }
        "is_point_disabled" => Some(Variant::Bool(a.is_point_disabled(astar_int(args, 0)))),
        "connect_points" => {
            a.connect_points(
                astar_int(args, 0),
                astar_int(args, 1),
                astar_bool(args, 2, true),
            );
            Some(Variant::Nil)
        }
        "disconnect_points" => {
            a.disconnect_points(
                astar_int(args, 0),
                astar_int(args, 1),
                astar_bool(args, 2, true),
            );
            Some(Variant::Nil)
        }
        "are_points_connected" => Some(Variant::Bool(a.are_points_connected(
            astar_int(args, 0),
            astar_int(args, 1),
            astar_bool(args, 2, true),
        ))),
        "get_point_connections" => Some(Variant::PackedInt64Array(
            a.get_point_connections(astar_int(args, 0)),
        )),
        "get_id_path" => Some(Variant::PackedInt64Array(
            a.get_id_path(astar_int(args, 0), astar_int(args, 1)),
        )),
        "clear" => {
            a.clear();
            Some(Variant::Nil)
        }
        _ => None,
    }
}

fn astar_call_2d(a: &mut crate::astar::AStar2D, method: &str, args: &[Variant]) -> Option<Variant> {
    match method {
        "add_point" => {
            a.add_point(
                astar_int(args, 0),
                astar_v2(args, 1),
                astar_f32(args, 2, 1.0),
            );
            Some(Variant::Nil)
        }
        "get_point_position" => Some(
            a.get_point_position(astar_int(args, 0))
                .map(Variant::Vector2)
                .unwrap_or(Variant::Nil),
        ),
        "set_point_position" => {
            a.set_point_position(astar_int(args, 0), astar_v2(args, 1));
            Some(Variant::Nil)
        }
        "get_point_path" => Some(Variant::PackedVector2Array(
            a.get_point_path(astar_int(args, 0), astar_int(args, 1)),
        )),
        "get_closest_point" => Some(Variant::Int(
            a.get_closest_point(astar_v2(args, 0), astar_bool(args, 1, false)),
        )),
        // pat-s3rrv.19: the fallback is the dimension's zero — `AStarVector`
        // cannot name one generically, and Godot returns a default-constructed
        // vector when no segment qualifies.
        "get_closest_position_in_segment" => Some(Variant::Vector2(
            a.get_closest_position_in_segment(astar_v2(args, 0), gdcore::math::Vector2::ZERO),
        )),
        _ => astar_call_shared(a, method, args),
    }
}

fn astar_call_3d(a: &mut crate::astar::AStar3D, method: &str, args: &[Variant]) -> Option<Variant> {
    match method {
        "add_point" => {
            a.add_point(
                astar_int(args, 0),
                astar_v3(args, 1),
                astar_f32(args, 2, 1.0),
            );
            Some(Variant::Nil)
        }
        "get_point_position" => Some(
            a.get_point_position(astar_int(args, 0))
                .map(Variant::Vector3)
                .unwrap_or(Variant::Nil),
        ),
        "set_point_position" => {
            a.set_point_position(astar_int(args, 0), astar_v3(args, 1));
            Some(Variant::Nil)
        }
        "get_point_path" => Some(Variant::PackedVector3Array(
            a.get_point_path(astar_int(args, 0), astar_int(args, 1)),
        )),
        "get_closest_point" => Some(Variant::Int(
            a.get_closest_point(astar_v3(args, 0), astar_bool(args, 1, false)),
        )),
        "get_closest_position_in_segment" => Some(Variant::Vector3(
            a.get_closest_position_in_segment(astar_v3(args, 0), gdcore::math::Vector3::ZERO),
        )),
        _ => astar_call_shared(a, method, args),
    }
}

/// Godot's `AStarGrid2D.DiagonalMode` enum values (doc/classes/AStarGrid2D.xml).
fn astar_diagonal_mode(v: i64) -> crate::astar::DiagonalMode {
    match v {
        1 => crate::astar::DiagonalMode::Never,
        2 => crate::astar::DiagonalMode::AtLeastOneWalkable,
        3 => crate::astar::DiagonalMode::OnlyIfNoObstacles,
        _ => crate::astar::DiagonalMode::Always,
    }
}

/// Godot's `AStarGrid2D.Heuristic` enum values.
fn astar_heuristic(v: i64) -> crate::astar::Heuristic {
    match v {
        1 => crate::astar::Heuristic::Manhattan,
        2 => crate::astar::Heuristic::Octile,
        3 => crate::astar::Heuristic::Chebyshev,
        _ => crate::astar::Heuristic::Euclidean,
    }
}

fn astar_call_grid(
    g: &mut crate::astar::AStarGrid2D,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    match method {
        // `set_region(position, size)`; Godot's `region` property is a Rect2i,
        // which Patina's Variant does not model, so the two-vector spelling is
        // what a script uses here.
        "set_region" => {
            g.set_region(astar_v2i(args, 0), astar_v2i(args, 1));
            Some(Variant::Nil)
        }
        "get_size" => Some(Variant::Vector2i(g.get_region().1)),
        "set_size" => {
            let (pos, _) = g.get_region();
            g.set_region(pos, astar_v2i(args, 0));
            Some(Variant::Nil)
        }
        "set_cell_size" => {
            g.set_cell_size(astar_v2(args, 0));
            Some(Variant::Nil)
        }
        "set_offset" => {
            g.set_offset(astar_v2(args, 0));
            Some(Variant::Nil)
        }
        "set_diagonal_mode" => {
            g.set_diagonal_mode(astar_diagonal_mode(astar_int(args, 0)));
            Some(Variant::Nil)
        }
        "set_default_compute_heuristic" => {
            g.set_default_compute_heuristic(astar_heuristic(astar_int(args, 0)));
            Some(Variant::Nil)
        }
        "set_default_estimate_heuristic" => {
            g.set_default_estimate_heuristic(astar_heuristic(astar_int(args, 0)));
            Some(Variant::Nil)
        }
        "set_jumping_enabled" => {
            g.set_jumping_enabled(astar_bool(args, 0, true));
            Some(Variant::Nil)
        }
        // pat-s3rrv.19: `update()` used to be a no-op, so this had nothing to
        // report; the flag it clears is what a script branches on.
        "is_dirty" => Some(Variant::Bool(g.is_dirty())),
        "update" => {
            g.update();
            Some(Variant::Nil)
        }
        "is_in_bounds" => Some(Variant::Bool(
            g.is_in_bounds(astar_int(args, 0) as i32, astar_int(args, 1) as i32),
        )),
        "is_in_boundsv" => Some(Variant::Bool(g.is_in_boundsv(astar_v2i(args, 0)))),
        "set_point_solid" => {
            g.set_point_solid(astar_v2i(args, 0), astar_bool(args, 1, true));
            Some(Variant::Nil)
        }
        "is_point_solid" => Some(Variant::Bool(g.is_point_solid(astar_v2i(args, 0)))),
        "fill_solid_region" => {
            g.fill_solid_region(
                astar_v2i(args, 0),
                astar_v2i(args, 1),
                astar_bool(args, 2, true),
            );
            Some(Variant::Nil)
        }
        "set_point_weight_scale" => {
            g.set_point_weight_scale(astar_v2i(args, 0), astar_f32(args, 1, 1.0));
            Some(Variant::Nil)
        }
        "get_point_weight_scale" => Some(Variant::Float(
            g.get_point_weight_scale(astar_v2i(args, 0)) as f64,
        )),
        "get_point_position" => Some(Variant::Vector2(g.get_point_position(astar_v2i(args, 0)))),
        // Godot returns a typed Vector2i array; Patina has no PackedVector2iArray,
        // so the cells come back as a plain Array of Vector2i.
        "get_id_path" => Some(Variant::Array(
            g.get_id_path(astar_v2i(args, 0), astar_v2i(args, 1))
                .into_iter()
                .map(Variant::Vector2i)
                .collect(),
        )),
        "get_point_path" => Some(Variant::PackedVector2Array(
            g.get_point_path(astar_v2i(args, 0), astar_v2i(args, 1)),
        )),
        _ => None,
    }
}

/// Extracts the base class from a GDScript's leading `extends` declaration,
/// defaulting to `Node`. A path/quoted base (`extends "res://x.gd"`) resolves
/// to a Node subtype we cannot name statically, so it falls back to `Node` —
/// matching the autoload boot path's `base_class_from_source`.
fn script_base_class(source: &str) -> String {
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("extends") {
            // Require a separator so we don't match `extends_foo`.
            let rest = match rest.strip_prefix([' ', '\t']) {
                Some(r) => r.trim(),
                None => return "Node".to_string(),
            };
            if rest.is_empty() || rest.starts_with('"') {
                return "Node".to_string();
            }
            return rest.split_whitespace().next().unwrap_or("Node").to_string();
        }
        // First meaningful line isn't `extends` → default base class.
        break;
    }
    "Node".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::LifecycleManager;
    use crate::main_loop::MainLoop;
    use crate::node::Node;
    use crate::scene_tree::SceneTree;

    /// pat-s3rrv.19: does the AStar registry round-trip AT ALL?
    ///
    /// The script-level diagnostic proved `AStar2D.new()` hands back a
    /// correctly tagged handle (`0xa100000000000001`) and that *every* method on
    /// it then fails — including `get_point_count()`, which takes no arguments.
    /// So `astar_call` returns `None` for an entry `astar_new` has just
    /// inserted, which should be impossible: both use the same file-level
    /// `thread_local!` and the same key.
    ///
    /// This calls the two trait methods back to back with NOTHING in between —
    /// no interpreter, no script, no `stacker::maybe_grow` stack switch. It
    /// splits the last fork in that bead:
    ///
    /// * FAILS → the registry itself is broken, and the fault is in these ~30
    ///   lines.
    /// * PASSES → the registry is fine and something on the interpreter path
    ///   between `.new()` and the method call is losing it.
    #[test]
    fn astar_registry_round_trips_a_handle() {
        use gdscript_interop::bindings::SceneAccess;

        let mut tree = SceneTree::new();
        // SAFETY: `tree` outlives the accessor, which never escapes this test.
        let mut access = unsafe { SceneTreeAccessor::new(&mut tree as *mut SceneTree) };

        let raw = access
            .astar_new("AStar2D")
            .expect("AStar2D.new() must mint a handle");
        assert_eq!(
            raw & 0xFF00_0000_0000_0000,
            gdscript_interop::bindings::ASTAR_HANDLE_TAG,
            "the minted handle must carry the AStar tag",
        );

        let answer = access.astar_call(raw, "get_point_count", &[]);
        assert_eq!(
            answer,
            Some(Variant::Int(0)),
            "`astar_call` must find the entry `astar_new` just inserted. `None` \
             here means the registry lookup is the bug — the handle is right \
             (verified above) and no interpreter was involved at all",
        );

        // …and it must still be there on a second call, i.e. the lookup is not
        // consuming the entry.
        assert_eq!(
            access.astar_call(raw, "get_point_count", &[]),
            Some(Variant::Int(0)),
            "the entry must survive being looked up",
        );
    }

    /// pat-s3rrv.19: an AStar handle must be claimed by NO other handle family.
    ///
    /// This is the bug the whole bead turned on. `ASTAR_HANDLE_TAG` was `0xA1`,
    /// which is byte-for-byte `gdcore::xml_parser`'s `XML_HANDLE_TAG` — the same
    /// tag in a different crate. `call_method_on` checks `is_xml_parser_handle`
    /// **before** the AStar branch, so every AStar handle was claimed by
    /// XMLParser and every method on it died as
    /// `MethodNotFound("XMLParser.<method>")`.
    ///
    /// It hid for eight passes because the symptom points at the wrong layer:
    /// `AStar2D.new()` works (the constructor never consults the predicates),
    /// the registry round-trips in isolation, and the failure is identical for
    /// zero-argument methods — so the constructor, the registry and the argument
    /// conversion all look guilty in turn and all check out clean.
    ///
    /// ⚠️ A tag byte is a WORKSPACE-wide namespace that nothing enforces. A
    /// sweep of `gdscript-interop` alone — which is where `ASTAR_HANDLE_TAG`
    /// lives — misses `gdcore`, `gdresource` and `gdobject` entirely. That is
    /// precisely the sweep I ran, twice, and it came back clean both times.
    #[test]
    fn an_astar_handle_is_not_claimed_by_another_handle_family() {
        use gdscript_interop::bindings::SceneAccess;

        let mut tree = SceneTree::new();
        // SAFETY: `tree` outlives the accessor, which never escapes this test.
        let mut access = unsafe { SceneTreeAccessor::new(&mut tree as *mut SceneTree) };

        for class in ["AStar2D", "AStar3D", "AStarGrid2D"] {
            let raw = access.astar_new(class).expect("handle minted");
            let id = gdcore::id::ObjectId::from_raw(raw);

            assert!(
                gdscript_interop::bindings::is_astar_handle(raw),
                "{class} must mint an AStar-tagged handle",
            );
            // The collision that caused this bead. `is_xml_parser_handle` is
            // checked earlier in `call_method_on`, so a shared tag silently
            // steals every AStar method call.
            assert!(
                !gdcore::xml_parser::is_xml_parser_handle(id),
                "{class}'s handle {raw:#018x} is being claimed by XMLParser — \
                 the two tags have collided again. `call_method_on` checks \
                 XMLParser first, so every AStar method will die as \
                 MethodNotFound(\"XMLParser.<method>\")",
            );
            assert!(
                !gdcore::is_file_access_handle(id) && !gdcore::is_dir_access_handle(id),
                "{class}'s handle collides with a gdcore file handle",
            );
            assert!(
                !gdresource::is_config_file_handle(id)
                    && !gdresource::is_zip_reader_handle(id)
                    && !gdresource::is_zip_packer_handle(id)
                    && !gdresource::pck::is_pck_packer_handle(id),
                "{class}'s handle collides with a gdresource handle",
            );
            assert!(
                !gdobject::refcounted_handle::is_refcounted_handle(id),
                "{class}'s handle collides with the RefCounted family",
            );
        }
    }

    /// pat-s3rrv.19: the next layer down — a script WITH access installed.
    ///
    /// `astar_registry_round_trips_a_handle` proved the registry is sound:
    /// `astar_new` inserts and `astar_call` finds it again. So the handle is
    /// lost somewhere on the interpreter path instead.
    ///
    /// This runs the real thing one step below `SceneTree::call_script_method`:
    /// a `GDScriptNodeInstance` with a genuine `SceneTreeAccessor` installed,
    /// invoked directly so the `Result` — and with it the **error text** — comes
    /// back instead of being swallowed. `call_script_method` discards the error
    /// (it returns `Option`), which is why every previous run could only say
    /// *that* it failed and never *why*.
    #[test]
    fn astar_reaches_a_script_with_access_installed() {
        use gdscript_interop::bindings::ScriptInstance;

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = tree
            .add_child(root, Node::new("Pathfinder", "Node"))
            .expect("child adds");
        let src = "extends Node\n\
                   func run():\n\
                   \x20   var a = AStar2D.new()\n\
                   \x20   return a.get_point_count()\n";
        let mut script =
            crate::GDScriptNodeInstance::from_source(src, node).expect("script parses");

        // SAFETY: `tree` outlives `script` in this test and is never moved
        // afterwards; the accessor does not escape.
        let accessor = unsafe { SceneTreeAccessor::new(&mut tree as *mut SceneTree) };
        script.set_scene_access(Box::new(accessor), node.raw());

        let out = script.call_method("run", &[]);
        assert_eq!(
            out.as_ref().ok(),
            Some(&Variant::Int(0)),
            "an empty AStar2D has 0 points. The error text is the deliverable \
             here — it names the layer that loses a handle the registry \
             demonstrably round-trips: {out:?}",
        );
    }

    // pat-u3l8c.12: `load("x.gd").new()` — the accessor loads the script, makes
    // a node of its base class with the script attached, and returns its id.
    #[test]
    fn script_base_class_reads_extends_or_defaults_to_node() {
        assert_eq!(script_base_class("extends Node2D\nvar x = 1\n"), "Node2D");
        assert_eq!(script_base_class("# hi\nextends Control\n"), "Control");
        assert_eq!(script_base_class("var x = 1\n"), "Node");
        assert_eq!(script_base_class(""), "Node");
        // A path/quoted base can't be named statically → Node.
        assert_eq!(script_base_class("extends \"res://base.gd\"\n"), "Node");
    }

    #[test]
    fn accessor_instantiate_script_attaches_script_node() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("patina_u3l8c12_{}_{}.gd", std::process::id(), n));
        std::fs::write(&path, "extends Node2D\nvar hp = 7\n").unwrap();
        // Absolute path (no `res://`) is read directly, avoiding CWD coupling.
        let path_str = path.to_str().unwrap().to_string();

        let mut tree = SceneTree::new();
        let tree_ptr = &mut tree as *mut SceneTree;
        let raw = {
            let mut acc = unsafe { SceneTreeAccessor::new(tree_ptr) };
            acc.instantiate_script(&path_str)
        };
        let raw = raw.expect("script instantiated");

        let node_id = NodeId::from_object_id(ObjectId::from_raw(raw));
        assert!(
            tree.get_script(node_id).is_some(),
            "the scripted instance has its script attached"
        );
        assert_eq!(
            tree.get_node(node_id).unwrap().class_name(),
            "Node2D",
            "node uses the script's base class"
        );

        let _ = std::fs::remove_file(&path);
    }

    const PLAYER_SCRIPT: &str = "\
extends Node2D
var speed = 100.0
var moved = false
func _ready():
    self.moved = true
func _process(delta):
    self.speed = self.speed + delta
";

    const PHYSICS_SCRIPT: &str = "\
extends Node2D
var velocity = 0.0
func _physics_process(delta):
    self.velocity = self.velocity + delta * 10.0
";

    const ENTER_EXIT_SCRIPT: &str = "\
extends Node
var entered = false
var exited = false
func _enter_tree():
    self.entered = true
func _exit_tree():
    self.exited = true
";

    const MINIMAL_SCRIPT: &str = "\
extends Node
var alive = true
";

    const MULTI_LIFECYCLE_SCRIPT: &str = "\
extends Node2D
var log_count = 0
func _ready():
    self.log_count = self.log_count + 1
func _process(delta):
    self.log_count = self.log_count + 1
func _physics_process(delta):
    self.log_count = self.log_count + 1
";

    // -- GDScriptNodeInstance unit tests ------------------------------------

    #[test]
    fn create_instance_from_source() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.node_id(), node_id);
        assert!(inst.has_method("_ready"));
        assert!(inst.has_method("_process"));
    }

    #[test]
    fn call_ready_modifies_property() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.get_property("moved"), Some(Variant::Bool(false)));
        inst.call_method("_ready", &[]).unwrap();
        assert_eq!(inst.get_property("moved"), Some(Variant::Bool(true)));
    }

    #[test]
    fn call_process_with_delta() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.get_property("speed"), Some(Variant::Float(100.0)));
        inst.call_method("_process", &[Variant::Float(0.5)])
            .unwrap();
        assert_eq!(inst.get_property("speed"), Some(Variant::Float(100.5)));
    }

    #[test]
    fn list_methods_includes_lifecycle() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        let methods: Vec<String> = inst.list_methods().iter().map(|m| m.name.clone()).collect();
        assert!(methods.contains(&"_ready".to_string()));
        assert!(methods.contains(&"_process".to_string()));
    }

    #[test]
    fn list_properties_includes_vars() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        let props: Vec<String> = inst
            .list_properties()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert!(props.contains(&"speed".to_string()));
        assert!(props.contains(&"moved".to_string()));
    }

    #[test]
    fn set_property_updates_value() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert!(inst.set_property("speed", Variant::Float(200.0)));
        assert_eq!(inst.get_property("speed"), Some(Variant::Float(200.0)));
    }

    #[test]
    fn set_nonexistent_property_returns_false() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert!(!inst.set_property("nonexistent", Variant::Int(1)));
    }

    #[test]
    fn get_script_name_returns_gdscript() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.get_script_name(), "GDScript");
    }

    #[test]
    fn call_nonexistent_method_returns_error() {
        let node_id = NodeId::next();
        let mut inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        let result = inst.call_method("_nonexistent", &[]);
        assert!(result.is_err());
    }

    // -- SceneTree script store tests --------------------------------------

    #[test]
    fn attach_and_has_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));
        assert!(tree.has_script(child_id));
        assert!(!tree.has_script(root));
    }

    #[test]
    fn detach_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));
        assert!(tree.has_script(child_id));

        tree.detach_script(child_id);
        assert!(!tree.has_script(child_id));
    }

    #[test]
    fn get_script_returns_reference() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let s = tree.get_script(child_id);
        assert!(s.is_some());
        assert_eq!(s.unwrap().get_script_name(), "GDScript");
    }

    // -- Lifecycle integration tests ---------------------------------------

    #[test]
    fn ready_fires_script_after_enter_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // Before lifecycle, moved=false
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("moved"),
            Some(Variant::Bool(false))
        );

        LifecycleManager::enter_tree(&mut tree, child_id);

        // After lifecycle, _ready was called so moved=true
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn enter_tree_fires_script_enter_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Obj", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(ENTER_EXIT_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("entered"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn exit_tree_fires_script_exit_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Obj", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(ENTER_EXIT_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::exit_tree(&mut tree, child_id);

        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("exited"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn process_fires_script_each_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        ml.run_frames(3, 1.0 / 60.0);

        let delta = 1.0 / 60.0;
        let expected = 100.0 + delta * 3.0;
        let speed = ml
            .tree()
            .get_script(child_id)
            .unwrap()
            .get_property("speed")
            .unwrap();
        match speed {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-9,
                "expected speed ~{expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn physics_process_fires_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Body", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PHYSICS_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        // 1 frame at 1/60 → exactly 1 physics tick at dt=1/60
        ml.step(1.0 / 60.0);

        let vel = ml
            .tree()
            .get_script(child_id)
            .unwrap()
            .get_property("velocity")
            .unwrap();
        let physics_dt = 1.0 / 60.0;
        let expected = physics_dt * 10.0;
        match vel {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-9,
                "expected velocity ~{expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn script_with_no_lifecycle_methods_no_error() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Passive", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(MINIMAL_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // This should not panic or error — script has no _ready/_process
        LifecycleManager::enter_tree(&mut tree, child_id);

        let mut ml = MainLoop::new(tree);
        ml.run_frames(5, 1.0 / 60.0);

        assert_eq!(
            ml.tree()
                .get_script(child_id)
                .unwrap()
                .get_property("alive"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn multiple_scripted_nodes() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let n1 = Node::new("P1", "Node2D");
        let id1 = tree.add_child(root, n1).unwrap();
        let n2 = Node::new("P2", "Node2D");
        let id2 = tree.add_child(root, n2).unwrap();

        let s1 = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, id1).unwrap();
        let s2 = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, id2).unwrap();
        tree.attach_script(id1, Box::new(s1));
        tree.attach_script(id2, Box::new(s2));

        LifecycleManager::enter_tree(&mut tree, id1);
        LifecycleManager::enter_tree(&mut tree, id2);

        // Both should have _ready called
        assert_eq!(
            tree.get_script(id1).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
        assert_eq!(
            tree.get_script(id2).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn detach_script_stops_callbacks() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        ml.run_frames(2, 1.0 / 60.0);

        let delta = 1.0 / 60.0;
        let speed_after_2 = 100.0 + delta * 2.0;

        // Detach the script
        ml.tree_mut().detach_script(child_id);

        // Run more frames — speed should NOT change
        ml.run_frames(5, 1.0 / 60.0);

        // Script is gone, so get_script returns None
        assert!(ml.tree().get_script(child_id).is_none());
        assert!(!ml.tree().has_script(child_id));

        // Verify the speed was what we expected before detach
        assert!((speed_after_2 - (100.0_f64 + delta * 2.0)).abs() < 1e-9);
    }

    #[test]
    fn multi_lifecycle_all_callbacks_fire() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Multi", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(MULTI_LIFECYCLE_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        // After enter_tree: _ready fires (+1) → log_count = 1
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("log_count"),
            Some(Variant::Int(1))
        );

        let mut ml = MainLoop::new(tree);
        // 1 frame at 1/60: 1 physics tick (+1) + 1 process (+1) → log_count = 3
        ml.step(1.0 / 60.0);

        assert_eq!(
            ml.tree()
                .get_script(child_id)
                .unwrap()
                .get_property("log_count"),
            Some(Variant::Int(3))
        );
    }

    #[test]
    fn script_process_accumulates_over_many_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        let delta = 1.0 / 60.0;
        ml.run_frames(100, delta);

        let expected = 100.0 + delta * 100.0;
        let speed = ml
            .tree()
            .get_script(child_id)
            .unwrap()
            .get_property("speed")
            .unwrap();
        match speed {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-6,
                "expected speed ~{expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn script_node_with_initial_property_values() {
        let node_id = NodeId::next();
        let inst = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, node_id).unwrap();
        assert_eq!(inst.get_property("speed"), Some(Variant::Float(100.0)));
        assert_eq!(inst.get_property("moved"), Some(Variant::Bool(false)));
    }

    #[test]
    fn enter_tree_with_children_scripts_fire_in_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let parent_script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, parent_id).unwrap();
        let child_script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(parent_id, Box::new(parent_script));
        tree.attach_script(child_id, Box::new(child_script));

        LifecycleManager::enter_tree(&mut tree, parent_id);

        // Both should have _ready called (child first in bottom-up, then parent)
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("moved"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_script_mut_allows_mutation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let s = tree.get_script_mut(child_id).unwrap();
        s.set_property("speed", Variant::Float(999.0));

        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("speed"),
            Some(Variant::Float(999.0))
        );
    }

    #[test]
    fn ext_resource_script_path_stored_on_node() {
        let tscn = "\
[gd_scene format=3]

[ext_resource type=\"Script\" path=\"res://scripts/player.gd\" id=\"1_abc\"]

[node name=\"Player\" type=\"Node2D\"]
script = ExtResource(\"1_abc\")
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let player = &nodes[0];
        assert_eq!(
            player.get_property("_script_path"),
            Variant::String("res://scripts/player.gd".into())
        );
    }

    #[test]
    fn node_without_script_no_script_path() {
        let tscn = "\
[gd_scene format=3]

[node name=\"Empty\" type=\"Node2D\"]
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes[0].get_property("_script_path"), Variant::Nil);
    }

    // -- Scene access / signal tests ----------------------------------------

    /// Helper: build a tree with root → parent → child1, child2
    fn build_tree_with_children() -> (SceneTree, NodeId, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let c1 = Node::new("Child1", "Node2D");
        let c1_id = tree.add_child(parent_id, c1).unwrap();
        let c2 = Node::new("Child2", "Node2D");
        let c2_id = tree.add_child(parent_id, c2).unwrap();
        (tree, parent_id, c1_id, c2_id)
    }

    #[test]
    fn get_node_from_script() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var found = false
func _ready():
    var c = get_node(\"Child1\")
    self.found = true
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("found"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn dollar_syntax_parses() {
        // Verify the tokenizer and parser handle $NodeName
        let tokens = gdscript_interop::tokenize("$Player\n").unwrap();
        let has_dollar = tokens
            .iter()
            .any(|t| matches!(t.token, gdscript_interop::Token::Dollar));
        assert!(has_dollar, "Dollar token not found");
    }

    #[test]
    fn dollar_syntax_in_script() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var found_name = \"\"
func _ready():
    var c = $Child1
    self.found_name = c.get_name()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("found_name"),
            Some(Variant::String("Child1".into()))
        );
    }

    #[test]
    fn dollar_string_syntax() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var ok = false
func _ready():
    var c = $\"Child2\"
    self.ok = true
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("ok"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_parent_from_script() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var has_parent = false
func _ready():
    var p = get_parent()
    self.has_parent = true
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("has_parent"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_children_from_script() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _ready():
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn emit_signal_from_script() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

        // Set up signal handler
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let conn = gdobject::signal::Connection::with_callback(
            ObjectId::from_raw(parent_id.raw()),
            "on_hit",
            move |_args| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            },
        );
        tree.connect_signal(parent_id, "hit", conn);

        let script_src = "\
extends Node2D
func _ready():
    emit_signal(\"hit\")
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn emit_signal_with_args() {
        use std::sync::{Arc, Mutex};
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();
        let conn = gdobject::signal::Connection::with_callback(
            ObjectId::from_raw(parent_id.raw()),
            "on_damage",
            move |args| {
                received_clone.lock().unwrap().extend_from_slice(args);
                Variant::Nil
            },
        );
        tree.connect_signal(parent_id, "damage_taken", conn);

        let script_src = "\
extends Node2D
func _ready():
    emit_signal(\"damage_taken\", 42)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);

        let args = received.lock().unwrap();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], Variant::Int(42));
    }

    #[test]
    fn connect_signal_from_script() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();

        // Child1 has a script that connects parent's "test_sig" to itself
        // We can't fully test cross-node method dispatch yet, but we can
        // verify the connection is registered.
        let script_src = "\
extends Node2D
var connected = false
func _ready():
    var p = get_parent()
    p.connect(\"test_sig\", $Child2, \"on_test\")
    self.connected = true
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("connected"),
            Some(Variant::Bool(true))
        );
        // Verify signal was registered on the parent
        let store = tree.signal_store(parent_id);
        assert!(store.is_some());
        assert!(store.unwrap().has_signal("test_sig"));
    }

    #[test]
    fn signal_deferred_one_shot_bind_dispatch() {
        // A deferred, one-shot connection with a bound arg: emitting twice in a
        // frame must fire the target method exactly once, only at the deferred
        // flush, with the bound value; and the connection is gone afterward
        // (one-shot). Covers the scene-runtime half of pat-nzgdu.31.
        let (mut tree, parent_id, _c1_id, c2_id) = build_tree_with_children();

        let target_src = "\
extends Node2D
var hits = 0
var last = 0
func on_sig(v):
    self.hits += 1
    self.last = v
";
        let script = GDScriptNodeInstance::from_source(target_src, c2_id).unwrap();
        tree.attach_script(c2_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c2_id);

        // Connect parent."sig" -> c2.on_sig, deferred + one-shot + bind(42).
        let conn = Connection::new(c2_id.object_id(), "on_sig")
            .as_deferred()
            .as_one_shot()
            .with_binds(vec![Variant::Int(42)]);
        tree.connect_signal(parent_id, "sig", conn);

        // Emit twice in the same frame.
        tree.emit_signal(parent_id, "sig", &[]);
        tree.emit_signal(parent_id, "sig", &[]);

        // Deferred: nothing has fired yet.
        assert_eq!(
            tree.get_script(c2_id).unwrap().get_property("hits"),
            Some(Variant::Int(0)),
            "deferred connection must not fire before the flush"
        );

        // Flush the deferred queue (end-of-frame MessageQueue).
        tree.process_deferred_calls();

        // Fired exactly once (one-shot removed after first emit), bind arrived.
        assert_eq!(
            tree.get_script(c2_id).unwrap().get_property("hits"),
            Some(Variant::Int(1)),
            "one-shot deferred connection fires exactly once at the flush"
        );
        assert_eq!(
            tree.get_script(c2_id).unwrap().get_property("last"),
            Some(Variant::Int(42)),
            "bound argument arrives in the callback"
        );
        // One-shot connection removed.
        assert!(!tree
            .signal_store(parent_id)
            .and_then(|s| s.get_signal("sig"))
            .map(|sig| sig.is_connected())
            .unwrap_or(false));
    }

    #[test]
    fn signal_disconnect_and_is_connected() {
        // Exercises the SceneTree signal store logic that the SceneAccess
        // disconnect_signal / is_signal_connected methods delegate to.
        let (mut tree, parent_id, _c1_id, c2_id) = build_tree_with_children();
        let target = c2_id.object_id();

        let is_conn = |tree: &SceneTree| {
            tree.signal_store(parent_id)
                .and_then(|s| s.get_signal("sig"))
                .map(|sig| {
                    sig.connections()
                        .iter()
                        .any(|c| c.target_id == target && c.method == "on_sig")
                })
                .unwrap_or(false)
        };

        assert!(!is_conn(&tree), "not connected initially");
        tree.connect_signal(parent_id, "sig", Connection::new(target, "on_sig"));
        assert!(is_conn(&tree), "connected after connect");
        assert!(
            tree.signal_store_mut(parent_id)
                .disconnect("sig", target, "on_sig"),
            "disconnect removes the connection"
        );
        assert!(!is_conn(&tree), "not connected after disconnect");
        assert!(
            !tree
                .signal_store_mut(parent_id)
                .disconnect("sig", target, "on_sig"),
            "disconnecting a missing connection returns false"
        );
    }

    #[test]
    fn get_node_property_via_dot_access() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();

        // Attach a script to Child1 with a property
        let child_script = "\
extends Node2D
var health = 100
";
        let cs = GDScriptNodeInstance::from_source(child_script, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(cs));

        // Parent script reads Child1's property
        let parent_script = "\
extends Node2D
var child_health = 0
func _ready():
    var c = get_node(\"Child1\")
    self.child_health = c.health
";
        let ps = GDScriptNodeInstance::from_source(parent_script, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(ps));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_health"),
            Some(Variant::Int(100))
        );
    }

    #[test]
    fn set_node_property_via_dot_access() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();

        let child_script = "\
extends Node2D
var health = 100
";
        let cs = GDScriptNodeInstance::from_source(child_script, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(cs));

        // Parent sets Child1's health to 50
        let parent_script = "\
extends Node2D
func _ready():
    var c = get_node(\"Child1\")
    c.health = 50
";
        let ps = GDScriptNodeInstance::from_source(parent_script, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(ps));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("health"),
            Some(Variant::Int(50))
        );
    }

    #[test]
    fn get_node_name_method() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_name = \"\"
func _ready():
    var c = get_node(\"Child1\")
    self.child_name = c.get_name()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_name"),
            Some(Variant::String("Child1".into()))
        );
    }

    #[test]
    fn get_node_not_found_error() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var ok = true
func _ready():
    var c = get_node(\"NonExistent\")
    self.ok = false
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        // Script should have errored on get_node, so ok stays true
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("ok"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_children_on_object_id() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var grandchild_count = 0
func _ready():
    var p = get_parent()
    var kids = p.get_children()
    self.grandchild_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id)
                .unwrap()
                .get_property("grandchild_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn get_parent_on_object_id() {
        let (mut tree, _parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var ok = false
func _ready():
    var p = get_parent()
    var gp = p.get_parent()
    self.ok = true
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("ok"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn get_node_on_object_id() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var sibling_name = \"\"
func _ready():
    var p = get_parent()
    var sib = p.get_node(\"Child2\")
    self.sibling_name = sib.get_name()
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("sibling_name"),
            Some(Variant::String("Child2".into()))
        );
    }

    #[test]
    fn scene_access_during_process() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _process(delta):
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_process(parent_id, 0.016);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn scene_access_during_physics_process() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _physics_process(delta):
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_physics_process(parent_id, 0.016);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn get_child_count_builtin() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var count = 0
func _ready():
    self.count = get_child_count()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn get_child_count_on_leaf_node() {
        let (mut tree, _parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var count = -1
func _ready():
    self.count = get_child_count()
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("count"),
            Some(Variant::Int(0))
        );
    }

    #[test]
    fn get_child_count_on_object_id() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var parent_child_count = 0
func _ready():
    var p = get_parent()
    self.parent_child_count = p.get_child_count()
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id)
                .unwrap()
                .get_property("parent_child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn get_child_count_during_process() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var count = 0
func _process(delta):
    self.count = get_child_count()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_process(parent_id, 0.016);
        assert_eq!(
            tree.get_script(parent_id).unwrap().get_property("count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn scene_access_during_enter_tree() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _enter_tree():
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_enter_tree(parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn scene_access_during_exit_tree() {
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
        let script_src = "\
extends Node2D
var child_count = 0
func _exit_tree():
    var kids = get_children()
    self.child_count = len(kids)
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_exit_tree(parent_id);
        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_count"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn multiple_emit_signals() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

        let counter = Arc::new(AtomicUsize::new(0));
        let cc = counter.clone();
        let conn = gdobject::signal::Connection::with_callback(
            ObjectId::from_raw(parent_id.raw()),
            "on_tick",
            move |_| {
                cc.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            },
        );
        tree.connect_signal(parent_id, "tick", conn);

        let script_src = "\
extends Node2D
func _ready():
    emit_signal(\"tick\")
    emit_signal(\"tick\")
    emit_signal(\"tick\")
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, parent_id);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn get_node_finds_sibling() {
        let (mut tree, parent_id, c1_id, _c2_id) = build_tree_with_children();
        // Child1's script looks for sibling Child2
        let script_src = "\
extends Node2D
var found = false
func _ready():
    var sib = get_node(\"Child2\")
    self.found = true
";
        let script = GDScriptNodeInstance::from_source(script_src, c1_id).unwrap();
        tree.attach_script(c1_id, Box::new(script));
        LifecycleManager::enter_tree(&mut tree, c1_id);
        assert_eq!(
            tree.get_script(c1_id).unwrap().get_property("found"),
            Some(Variant::Bool(true))
        );
    }

    #[test]
    fn tokenizer_dollar_token() {
        let tokens = gdscript_interop::tokenize("$Foo\n").unwrap();
        assert!(tokens
            .iter()
            .any(|t| matches!(t.token, gdscript_interop::Token::Dollar)));
        assert!(tokens
            .iter()
            .any(|t| matches!(&t.token, gdscript_interop::Token::Ident(n) if n == "Foo")));
    }

    #[test]
    fn parser_get_node_expr() {
        let tokens = gdscript_interop::tokenize("$Player\n").unwrap();
        let mut parser = gdscript_interop::Parser::new(tokens, "$Player\n");
        let stmts = parser.parse_script().unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            gdscript_interop::Stmt::ExprStmt(expr) => {
                assert!(matches!(expr, gdscript_interop::Expr::GetNode(_)));
            }
            _ => panic!("expected ExprStmt with GetNode"),
        }
    }

    // -- Script-to-node property sync tests ---------------------------------

    /// After _ready sets self.speed = 300, the node should have speed=300
    /// as a node property (synced from script).
    #[test]
    fn script_vars_sync_to_node_after_ready() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 200
func _ready():
    self.speed = 300
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        // Script var should be 300
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("speed"),
            Some(Variant::Int(300))
        );
        // Node property should also be 300 (synced)
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("speed"),
            Variant::Int(300)
        );
    }

    /// Script vars sync to node after _process calls too.
    #[test]
    fn script_vars_sync_to_node_after_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 100.0
func _process(delta):
    self.speed = self.speed + delta
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        tree.process_script_process(child_id, 0.5);

        // Node property should reflect the updated value
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("speed"),
            Variant::Float(100.5)
        );
    }

    /// Script vars sync to node after _physics_process.
    #[test]
    fn script_vars_sync_to_node_after_physics_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Body", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PHYSICS_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let dt = 1.0 / 60.0;
        tree.process_script_physics_process(child_id, dt);

        let expected = dt * 10.0;
        match tree.get_node(child_id).unwrap().get_property("velocity") {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-9,
                "expected velocity ~{expected} on node, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// Script vars sync to node after _enter_tree.
    #[test]
    fn script_vars_sync_to_node_after_enter_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Obj", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(ENTER_EXIT_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        tree.process_script_enter_tree(child_id);

        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("entered"),
            Variant::Bool(true)
        );
    }

    /// Script vars sync to node after _exit_tree.
    #[test]
    fn script_vars_sync_to_node_after_exit_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Obj", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(ENTER_EXIT_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        tree.process_script_exit_tree(child_id);

        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("exited"),
            Variant::Bool(true)
        );
    }

    /// Initial script variable values are synced even without calling methods,
    /// when the first lifecycle callback fires.
    #[test]
    fn initial_script_vars_sync_on_first_callback() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 200
var health = 100
func _ready():
    pass
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // Before any lifecycle, node has no speed/health
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("speed"),
            Variant::Nil
        );

        LifecycleManager::enter_tree(&mut tree, child_id);

        // After lifecycle, initial values are synced
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("speed"),
            Variant::Int(200)
        );
        assert_eq!(
            tree.get_node(child_id).unwrap().get_property("health"),
            Variant::Int(100)
        );
    }

    /// Multiple script vars are all synced correctly after method call.
    #[test]
    fn multiple_script_vars_all_synced() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var x = 0
var y = 0
var z = 0
func _ready():
    self.x = 10
    self.y = 20
    self.z = 30
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        let node = tree.get_node(child_id).unwrap();
        assert_eq!(node.get_property("x"), Variant::Int(10));
        assert_eq!(node.get_property("y"), Variant::Int(20));
        assert_eq!(node.get_property("z"), Variant::Int(30));
    }

    /// Script vars accumulate over multiple frames via MainLoop.
    #[test]
    fn script_vars_sync_across_multiple_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(PLAYER_SCRIPT, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        let delta = 1.0 / 60.0;
        ml.run_frames(5, delta);

        let expected = 100.0 + delta * 5.0;
        match ml.tree().get_node(child_id).unwrap().get_property("speed") {
            Variant::Float(v) => assert!(
                (v - expected).abs() < 1e-9,
                "expected speed ~{expected} on node, got {v}"
            ),
            other => panic!("expected Float on node, got {other:?}"),
        }
    }

    /// Two nodes with scripts: both have their vars synced independently.
    #[test]
    fn multiple_nodes_script_vars_sync_independently() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let n1 = Node::new("P1", "Node2D");
        let id1 = tree.add_child(root, n1).unwrap();
        let n2 = Node::new("P2", "Node2D");
        let id2 = tree.add_child(root, n2).unwrap();

        let script1_src = "\
extends Node2D
var score = 0
func _ready():
    self.score = 42
";
        let script2_src = "\
extends Node2D
var score = 0
func _ready():
    self.score = 99
";
        let s1 = GDScriptNodeInstance::from_source(script1_src, id1).unwrap();
        let s2 = GDScriptNodeInstance::from_source(script2_src, id2).unwrap();
        tree.attach_script(id1, Box::new(s1));
        tree.attach_script(id2, Box::new(s2));

        LifecycleManager::enter_tree(&mut tree, id1);
        LifecycleManager::enter_tree(&mut tree, id2);

        assert_eq!(
            tree.get_node(id1).unwrap().get_property("score"),
            Variant::Int(42)
        );
        assert_eq!(
            tree.get_node(id2).unwrap().get_property("score"),
            Variant::Int(99)
        );
    }

    /// Verify script vars and node properties both appear after lifecycle,
    /// and they agree on the value.
    #[test]
    fn script_and_node_properties_agree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 200
func _ready():
    self.speed = 300
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        let script_val = tree
            .get_script(child_id)
            .unwrap()
            .get_property("speed")
            .unwrap();
        let node_val = tree.get_node(child_id).unwrap().get_property("speed");

        assert_eq!(script_val, node_val);
        assert_eq!(script_val, Variant::Int(300));
    }

    /// Script vars of different types (int, float, bool, string) all sync.
    #[test]
    fn script_vars_different_types_sync() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Mixed", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node
var count = 0
var ratio = 0.0
var flag = false
var label = \"hello\"
func _ready():
    self.count = 42
    self.ratio = 3.14
    self.flag = true
    self.label = \"world\"
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        LifecycleManager::enter_tree(&mut tree, child_id);

        let node = tree.get_node(child_id).unwrap();
        assert_eq!(node.get_property("count"), Variant::Int(42));
        assert_eq!(node.get_property("ratio"), Variant::Float(3.14));
        assert_eq!(node.get_property("flag"), Variant::Bool(true));
        assert_eq!(node.get_property("label"), Variant::String("world".into()));
    }

    // -----------------------------------------------------------------------
    // Script execution integration tests (Play button wiring)
    // -----------------------------------------------------------------------

    /// Test that a script with _process modifying position works end-to-end.
    /// This simulates what happens when the editor Play button is pressed:
    /// parse script -> attach to node -> call _ready -> call _process per frame.
    #[test]
    fn script_position_x_moves_over_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("Mover", "Node2D");
        child.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 100.0
func _process(delta):
    self.position.x = self.position.x + speed * delta
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // Run 10 frames at 1/60
        let delta = 1.0 / 60.0;
        for _ in 0..10 {
            tree.process_script_process(child_id, delta);
        }

        // Position.x should have moved by speed * delta * 10
        let expected_x = 100.0 * delta as f32 * 10.0;
        match tree.get_node(child_id).unwrap().get_property("position") {
            Variant::Vector2(v) => {
                assert!(
                    (v.x - expected_x).abs() < 0.01,
                    "expected position.x ~{expected_x}, got {}",
                    v.x
                );
            }
            other => panic!("expected Vector2 position, got {other:?}"),
        }
    }

    /// Bare node property component writes should behave like `self.position.x`
    /// during per-frame script processing.
    #[test]
    fn bare_script_position_x_moves_over_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("Mover", "Node2D");
        child.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 100.0
func _process(delta):
    position.x += speed * delta
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        let delta = 1.0 / 60.0;
        for _ in 0..10 {
            tree.process_script_process(child_id, delta);
        }

        let expected_x = 100.0 * delta as f32 * 10.0;
        match tree.get_node(child_id).unwrap().get_property("position") {
            Variant::Vector2(v) => {
                assert!(
                    (v.x - expected_x).abs() < 0.01,
                    "expected position.x ~{expected_x}, got {}",
                    v.x
                );
            }
            other => panic!("expected Vector2 position, got {other:?}"),
        }
    }

    /// Test that _ready is called when we manually invoke it, simulating
    /// what the Play button does.
    #[test]
    fn play_button_calls_ready_then_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var ready_called = false
var frame_count = 0
func _ready():
    self.ready_called = true
func _process(delta):
    self.frame_count = self.frame_count + 1
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // Simulate Play: call _ready first
        tree.process_script_ready(child_id);

        assert_eq!(
            tree.get_script(child_id)
                .unwrap()
                .get_property("ready_called"),
            Some(Variant::Bool(true))
        );

        // Then run 5 frames
        for _ in 0..5 {
            tree.process_script_process(child_id, 1.0 / 60.0);
        }

        assert_eq!(
            tree.get_script(child_id)
                .unwrap()
                .get_property("frame_count"),
            Some(Variant::Int(5))
        );
    }

    /// Test that Vector2 constructor works in scripts.
    #[test]
    fn script_creates_vector2() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("Setter", "Node2D");
        child.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
func _ready():
    self.position = Vector2(100.0, 200.0)
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));
        tree.process_script_ready(child_id);

        match tree.get_node(child_id).unwrap().get_property("position") {
            Variant::Vector2(v) => {
                assert!(
                    (v.x - 100.0).abs() < 0.01 && (v.y - 200.0).abs() < 0.01,
                    "expected (100, 200), got ({}, {})",
                    v.x,
                    v.y
                );
            }
            other => panic!("expected Vector2, got {other:?}"),
        }
    }

    /// Test that print() doesn't crash (output goes to interpreter's output buffer).
    #[test]
    fn script_print_does_not_crash() {
        let node_id = NodeId::next();
        let script_src = "\
extends Node
var done = false
func _ready():
    print(\"hello from script\")
    self.done = true
";
        let mut inst = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
        inst.call_method("_ready", &[]).unwrap();
        assert_eq!(inst.get_property("done"), Some(Variant::Bool(true)));
    }

    /// Test that math builtins (abs, min, max, clamp) work in scripts.
    #[test]
    fn script_math_builtins() {
        let node_id = NodeId::next();
        let script_src = "\
extends Node
var abs_val = 0
var min_val = 0
var max_val = 0
var clamped = 0
func _ready():
    self.abs_val = abs(-42)
    self.min_val = min(3, 7)
    self.max_val = max(3, 7)
    self.clamped = clamp(15, 0, 10)
";
        let mut inst = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
        inst.call_method("_ready", &[]).unwrap();
        assert_eq!(inst.get_property("abs_val"), Some(Variant::Int(42)));
        assert_eq!(inst.get_property("min_val"), Some(Variant::Int(3)));
        assert_eq!(inst.get_property("max_val"), Some(Variant::Int(7)));
        assert_eq!(inst.get_property("clamped"), Some(Variant::Int(10)));
    }

    /// Test deg_to_rad and rad_to_deg builtins.
    #[test]
    fn script_deg_rad_builtins() {
        let node_id = NodeId::next();
        let script_src = "\
extends Node
var radians = 0.0
var degrees = 0.0
func _ready():
    self.radians = deg_to_rad(180.0)
    self.degrees = rad_to_deg(3.14159265358979)
";
        let mut inst = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
        inst.call_method("_ready", &[]).unwrap();

        match inst.get_property("radians") {
            Some(Variant::Float(v)) => assert!(
                (v - std::f64::consts::PI).abs() < 1e-6,
                "expected PI, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
        match inst.get_property("degrees") {
            Some(Variant::Float(v)) => assert!((v - 180.0).abs() < 0.01, "expected 180, got {v}"),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// Test that randf() and randi() return valid values.
    #[test]
    fn script_random_builtins() {
        let node_id = NodeId::next();
        let script_src = "\
extends Node
var rf = 0.0
var ri = 0
func _ready():
    self.rf = randf()
    self.ri = randi()
";
        let mut inst = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
        inst.call_method("_ready", &[]).unwrap();
        // randf should return a float (could be any value)
        assert!(matches!(inst.get_property("rf"), Some(Variant::Float(_))));
        assert!(matches!(inst.get_property("ri"), Some(Variant::Int(_))));
    }

    /// Test that get_node works during _process (simulating runtime).
    #[test]
    fn script_get_node_during_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let script_src = "\
extends Node2D
var found_child = false
func _process(delta):
    var c = get_node(\"Child\")
    self.found_child = true
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));
        tree.process_script_process(parent_id, 0.016);

        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("found_child"),
            Some(Variant::Bool(true))
        );
    }

    /// Test that script errors during _process don't crash; the method
    /// returns an error but the tree remains intact.
    #[test]
    fn script_runtime_error_does_not_crash() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Buggy", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        // This script references an undefined variable
        let script_src = "\
extends Node2D
var ok = true
func _process(delta):
    var x = undefined_var
    self.ok = false
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        // _process should fail but not panic
        tree.process_script_process(child_id, 0.016);

        // ok should still be true because the error happens before self.ok = false
        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("ok"),
            Some(Variant::Bool(true))
        );

        // Tree is still functional
        assert!(tree.has_script(child_id));
    }

    /// Test the fixture script test_move.gd can be loaded from disk.
    #[test]
    fn load_test_move_fixture_script() {
        let script_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/scripts/test_move.gd"
        );
        let source = std::fs::read_to_string(script_path)
            .unwrap_or_else(|e| panic!("Failed to read {script_path}: {e}"));

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Mover", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script = GDScriptNodeInstance::from_source(&source, child_id).unwrap();
        assert!(script.has_method("_process"));

        tree.attach_script(child_id, Box::new(script));

        // Run a few frames and verify the speed property changes
        // (test_move.gd does: self.speed = self.speed + delta)
        let delta = 1.0 / 60.0;
        for _ in 0..5 {
            tree.process_script_process(child_id, delta);
        }

        let expected = 100.0 + delta * 5.0;
        match tree.get_script(child_id).unwrap().get_property("speed") {
            Some(Variant::Float(v)) => assert!(
                (v - expected).abs() < 1e-9,
                "expected speed ~{expected}, got {v}"
            ),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// Test that multiple scripts all get _ready and _process called
    /// during a simulated play session.
    #[test]
    fn multi_script_play_simulation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let n1 = Node::new("A", "Node2D");
        let id1 = tree.add_child(root, n1).unwrap();
        let n2 = Node::new("B", "Node2D");
        let id2 = tree.add_child(root, n2).unwrap();

        let script_src = "\
extends Node2D
var count = 0
func _ready():
    self.count = self.count + 1
func _process(delta):
    self.count = self.count + 1
";
        let s1 = GDScriptNodeInstance::from_source(script_src, id1).unwrap();
        let s2 = GDScriptNodeInstance::from_source(script_src, id2).unwrap();
        tree.attach_script(id1, Box::new(s1));
        tree.attach_script(id2, Box::new(s2));

        // Simulate Play: call _ready on all
        tree.process_script_ready(id1);
        tree.process_script_ready(id2);

        // Run 3 frames
        for _ in 0..3 {
            tree.process_all_scripts_process(1.0 / 60.0);
        }

        // Each script: 1 (_ready) + 3 (_process) = 4
        assert_eq!(
            tree.get_script(id1).unwrap().get_property("count"),
            Some(Variant::Int(4))
        );
        assert_eq!(
            tree.get_script(id2).unwrap().get_property("count"),
            Some(Variant::Int(4))
        );
    }

    #[test]
    fn process_callbacks_follow_tree_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root)
            .unwrap()
            .set_property("counter", Variant::Int(0));

        let first = Node::new("First", "Node2D");
        let first_id = tree.add_child(root, first).unwrap();
        let second = Node::new("Second", "Node2D");
        let second_id = tree.add_child(root, second).unwrap();

        let script_src = "\
extends Node2D
var seen_order = 0
func _process(delta):
    var parent = get_parent()
    parent.counter = parent.counter + 1
    self.seen_order = parent.counter
";
        let first_script = GDScriptNodeInstance::from_source(script_src, first_id).unwrap();
        let second_script = GDScriptNodeInstance::from_source(script_src, second_id).unwrap();
        tree.attach_script(first_id, Box::new(first_script));
        tree.attach_script(second_id, Box::new(second_script));

        tree.process_all_scripts_process(1.0 / 60.0);

        assert_eq!(
            tree.get_script(first_id)
                .unwrap()
                .get_property("seen_order"),
            Some(Variant::Int(1))
        );
        assert_eq!(
            tree.get_script(second_id)
                .unwrap()
                .get_property("seen_order"),
            Some(Variant::Int(2))
        );
    }

    #[test]
    fn physics_process_callbacks_follow_tree_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root)
            .unwrap()
            .set_property("counter", Variant::Int(0));

        let first = Node::new("First", "Node2D");
        let first_id = tree.add_child(root, first).unwrap();
        let second = Node::new("Second", "Node2D");
        let second_id = tree.add_child(root, second).unwrap();

        let script_src = "\
extends Node2D
var seen_order = 0
func _physics_process(delta):
    var parent = get_parent()
    parent.counter = parent.counter + 1
    self.seen_order = parent.counter
";
        let first_script = GDScriptNodeInstance::from_source(script_src, first_id).unwrap();
        let second_script = GDScriptNodeInstance::from_source(script_src, second_id).unwrap();
        tree.attach_script(first_id, Box::new(first_script));
        tree.attach_script(second_id, Box::new(second_script));

        tree.process_all_scripts_physics_process(1.0 / 60.0);

        assert_eq!(
            tree.get_script(first_id)
                .unwrap()
                .get_property("seen_order"),
            Some(Variant::Int(1))
        );
        assert_eq!(
            tree.get_script(second_id)
                .unwrap()
                .get_property("seen_order"),
            Some(Variant::Int(2))
        );
    }

    // -----------------------------------------------------------------------
    // Scene-aware signal dispatch tests (Bead B005)
    // -----------------------------------------------------------------------

    /// Signal connect and emit across two nodes: Emitter emits, Listener
    /// receives via script method dispatch.
    #[test]
    fn signal_cross_node_emit_dispatches_to_target_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        // Listener has a script with an _on_hit method.
        let listener_script = "\
extends Node2D
var got_hit = false
func _on_hit():
    self.got_hit = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Wire: Emitter.hit -> Listener._on_hit (no callback, just target+method).
        tree.connect_signal(
            emitter_id,
            "hit",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_hit"),
        );

        // Emit the signal.
        tree.emit_signal(emitter_id, "hit", &[]);

        // Listener's script should have been called.
        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("got_hit"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal from .tscn [connection] fires on instancing.
    #[test]
    fn tscn_connection_dispatches_to_script_on_emit() {
        let tscn = "\
[gd_scene format=3]

[node name=\"Root\" type=\"Node\"]

[node name=\"Emitter\" type=\"Node2D\" parent=\".\"]

[node name=\"Listener\" type=\"Node2D\" parent=\".\"]

[connection signal=\"fired\" from=\"Emitter\" to=\"Listener\" method=\"_on_fired\"]
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root =
            crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let emitter_id = tree.get_node_by_path("/root/Root/Emitter").unwrap();
        let listener_id = tree.get_node_by_path("/root/Root/Listener").unwrap();

        // Attach a script to the listener.
        let listener_script = "\
extends Node2D
var received = false
func _on_fired():
    self.received = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Emit from Emitter.
        tree.emit_signal(emitter_id, "fired", &[]);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("received"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal with arguments passed correctly to target script method.
    #[test]
    fn signal_args_passed_to_target_script() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var damage_amount = 0
func _on_damage(amount):
    self.damage_amount = amount
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "damage",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_damage"),
        );

        tree.emit_signal(emitter_id, "damage", &[Variant::Int(42)]);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("damage_amount"),
            Some(Variant::Int(42))
        );
    }

    /// Multiple connections on the same signal all fire.
    #[test]
    fn signal_multiple_connections_all_fire() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener_a = Node::new("A", "Node2D");
        let a_id = tree.add_child(root, listener_a).unwrap();
        let listener_b = Node::new("B", "Node2D");
        let b_id = tree.add_child(root, listener_b).unwrap();

        let script_a = "\
extends Node2D
var heard = false
func _on_ping():
    self.heard = true
";
        let script_b = "\
extends Node2D
var heard = false
func _on_ping():
    self.heard = true
";
        let sa = GDScriptNodeInstance::from_source(script_a, a_id).unwrap();
        let sb = GDScriptNodeInstance::from_source(script_b, b_id).unwrap();
        tree.attach_script(a_id, Box::new(sa));
        tree.attach_script(b_id, Box::new(sb));

        tree.connect_signal(
            emitter_id,
            "ping",
            gdobject::signal::Connection::new(a_id.object_id(), "_on_ping"),
        );
        tree.connect_signal(
            emitter_id,
            "ping",
            gdobject::signal::Connection::new(b_id.object_id(), "_on_ping"),
        );

        tree.emit_signal(emitter_id, "ping", &[]);

        assert_eq!(
            tree.get_script(a_id).unwrap().get_property("heard"),
            Some(Variant::Bool(true))
        );
        assert_eq!(
            tree.get_script(b_id).unwrap().get_property("heard"),
            Some(Variant::Bool(true))
        );
    }

    /// Disconnect removes the connection so it no longer fires.
    #[test]
    fn signal_disconnect_stops_dispatch() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var count = 0
func _on_tick():
    self.count = self.count + 1
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "tick",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_tick"),
        );

        // Emit once — should increment.
        tree.emit_signal(emitter_id, "tick", &[]);
        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("count"),
            Some(Variant::Int(1))
        );

        // Disconnect.
        tree.signal_store_mut(emitter_id)
            .disconnect("tick", listener_id.object_id(), "_on_tick");

        // Emit again — should NOT increment.
        tree.emit_signal(emitter_id, "tick", &[]);
        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("count"),
            Some(Variant::Int(1))
        );
    }

    /// Signal from parent to child dispatches correctly.
    #[test]
    fn signal_parent_to_child() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let child_script = "\
extends Node2D
var notified = false
func _on_parent_event():
    self.notified = true
";
        let cs = GDScriptNodeInstance::from_source(child_script, child_id).unwrap();
        tree.attach_script(child_id, Box::new(cs));

        tree.connect_signal(
            parent_id,
            "parent_event",
            gdobject::signal::Connection::new(child_id.object_id(), "_on_parent_event"),
        );

        tree.emit_signal(parent_id, "parent_event", &[]);

        assert_eq!(
            tree.get_script(child_id).unwrap().get_property("notified"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal from child to parent dispatches correctly.
    #[test]
    fn signal_child_to_parent() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let parent_script = "\
extends Node2D
var child_done = false
func _on_child_done():
    self.child_done = true
";
        let ps = GDScriptNodeInstance::from_source(parent_script, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(ps));

        tree.connect_signal(
            child_id,
            "done",
            gdobject::signal::Connection::new(parent_id.object_id(), "_on_child_done"),
        );

        tree.emit_signal(child_id, "done", &[]);

        assert_eq!(
            tree.get_script(parent_id)
                .unwrap()
                .get_property("child_done"),
            Some(Variant::Bool(true))
        );
    }

    /// emit_signal from GDScript fires cross-node.
    #[test]
    fn emit_signal_from_gdscript_fires_cross_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        // Listener script receives the signal.
        let listener_script = "\
extends Node2D
var received = false
func _on_boom():
    self.received = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Wire the connection (no callback, script dispatch).
        tree.connect_signal(
            emitter_id,
            "boom",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_boom"),
        );

        // Emitter script calls emit_signal("boom").
        let emitter_script = "\
extends Node2D
func _ready():
    emit_signal(\"boom\")
";
        let es = GDScriptNodeInstance::from_source(emitter_script, emitter_id).unwrap();
        tree.attach_script(emitter_id, Box::new(es));
        LifecycleManager::enter_tree(&mut tree, emitter_id);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("received"),
            Some(Variant::Bool(true))
        );
    }

    /// connect() from GDScript _ready wires a working connection that
    /// dispatches to the target script when emitted.
    #[test]
    fn connect_from_gdscript_ready_then_emit() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();
        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(parent_id, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(parent_id, listener).unwrap();

        // Listener has the target method.
        let listener_script = "\
extends Node2D
var received = false
func _on_signal_from_emitter():
    self.received = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Emitter's script connects signal via obj.connect() in _ready.
        // In GDScript: `self_node.connect(signal, target, method)`.
        // Since bare connect() is a method on ObjectId, we use
        // `get_parent().get_node("Emitter").connect(...)`.
        let emitter_script = "\
extends Node2D
func _ready():
    var p = get_parent()
    var me = p.get_node(\"Emitter\")
    var listener = p.get_node(\"Listener\")
    me.connect(\"my_signal\", listener, \"_on_signal_from_emitter\")
func _process(delta):
    emit_signal(\"my_signal\")
";
        let es = GDScriptNodeInstance::from_source(emitter_script, emitter_id).unwrap();
        tree.attach_script(emitter_id, Box::new(es));

        // _ready wires the connection.
        LifecycleManager::enter_tree(&mut tree, emitter_id);

        // Verify connection is registered.
        let store = tree.signal_store(emitter_id).unwrap();
        assert!(store.has_signal("my_signal"));

        // _process emits the signal.
        tree.process_script_process(emitter_id, 0.016);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("received"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal from .tscn [connection] between siblings.
    #[test]
    fn tscn_connection_sibling_signal() {
        let tscn = "\
[gd_scene format=3]

[node name=\"Root\" type=\"Node\"]

[node name=\"Button\" type=\"Node2D\" parent=\".\"]

[node name=\"Display\" type=\"Node2D\" parent=\".\"]

[connection signal=\"pressed\" from=\"Button\" to=\"Display\" method=\"_on_button_pressed\"]
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();
        let display_id = tree.get_node_by_path("/root/Root/Display").unwrap();

        let display_script = "\
extends Node2D
var pressed_count = 0
func _on_button_pressed():
    self.pressed_count = self.pressed_count + 1
";
        let ds = GDScriptNodeInstance::from_source(display_script, display_id).unwrap();
        tree.attach_script(display_id, Box::new(ds));

        // Emit twice.
        tree.emit_signal(button_id, "pressed", &[]);
        tree.emit_signal(button_id, "pressed", &[]);

        assert_eq!(
            tree.get_script(display_id)
                .unwrap()
                .get_property("pressed_count"),
            Some(Variant::Int(2))
        );
    }

    /// Both callback and script connections fire for the same signal.
    #[test]
    fn signal_callback_and_script_both_fire() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var script_fired = false
func _on_event():
    self.script_fired = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Script-dispatched connection (no callback).
        tree.connect_signal(
            emitter_id,
            "event",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_event"),
        );

        // Callback-based connection.
        let callback_count = Arc::new(AtomicUsize::new(0));
        let cc = callback_count.clone();
        tree.connect_signal(
            emitter_id,
            "event",
            gdobject::signal::Connection::with_callback(
                listener_id.object_id(),
                "_callback",
                move |_| {
                    cc.fetch_add(1, Ordering::SeqCst);
                    Variant::Nil
                },
            ),
        );

        tree.emit_signal(emitter_id, "event", &[]);

        // Both should have fired.
        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("script_fired"),
            Some(Variant::Bool(true))
        );
        assert_eq!(callback_count.load(Ordering::SeqCst), 1);
    }

    /// Emit signal with no connections is a no-op (no crash).
    #[test]
    fn signal_emit_no_connections_no_crash() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Lonely", "Node2D");
        let node_id = tree.add_child(root, node).unwrap();

        let results = tree.emit_signal(node_id, "nonexistent_signal", &[]);
        assert!(results.is_empty());
    }

    /// Signal with multiple arguments dispatches all args.
    #[test]
    fn signal_multiple_args_dispatched() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var total = 0
func _on_data(a, b):
    self.total = a + b
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "data",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_data"),
        );

        tree.emit_signal(emitter_id, "data", &[Variant::Int(10), Variant::Int(32)]);

        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("total"),
            Some(Variant::Int(42))
        );
    }

    /// Signal dispatches to a node 3 levels deep in the tree.
    #[test]
    fn signal_deep_hierarchy_dispatch() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();
        let c = Node::new("C", "Node2D");
        let c_id = tree.add_child(b_id, c).unwrap();

        let c_script = "\
extends Node2D
var pinged = false
func _on_ping():
    self.pinged = true
";
        let cs = GDScriptNodeInstance::from_source(c_script, c_id).unwrap();
        tree.attach_script(c_id, Box::new(cs));

        // Signal on root dispatches to deeply nested node C.
        tree.connect_signal(
            root,
            "ping",
            gdobject::signal::Connection::new(c_id.object_id(), "_on_ping"),
        );
        tree.emit_signal(root, "ping", &[]);

        assert_eq!(
            tree.get_script(c_id).unwrap().get_property("pinged"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal fires multiple times, accumulating state in target script.
    #[test]
    fn signal_fires_multiple_times_accumulates() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var count = 0
func _on_tick():
    self.count = self.count + 1
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "tick",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_tick"),
        );

        for _ in 0..5 {
            tree.emit_signal(emitter_id, "tick", &[]);
        }

        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("count"),
            Some(Variant::Int(5))
        );
    }

    /// Signal to node without script (no crash, graceful no-op).
    #[test]
    fn signal_to_node_without_script_no_crash() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let target = Node::new("Target", "Node2D");
        let target_id = tree.add_child(root, target).unwrap();

        // Connect but target has no script.
        tree.connect_signal(
            emitter_id,
            "boom",
            gdobject::signal::Connection::new(target_id.object_id(), "_on_boom"),
        );

        // Should not crash. The callback-less connection returns Nil.
        let results = tree.emit_signal(emitter_id, "boom", &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], Variant::Nil);
    }

    /// Full .tscn signal test: Emitter emits signal, Listener receives.
    #[test]
    fn tscn_full_signal_test_emitter_listener() {
        let tscn = "\
[gd_scene format=3]

[node name=\"Game\" type=\"Node\"]

[node name=\"Emitter\" type=\"Node2D\" parent=\".\"]

[node name=\"Listener\" type=\"Node2D\" parent=\".\"]

[connection signal=\"game_over\" from=\"Emitter\" to=\"Listener\" method=\"_on_game_over\"]
";
        let scene = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let emitter_id = tree.get_node_by_path("/root/Game/Emitter").unwrap();
        let listener_id = tree.get_node_by_path("/root/Game/Listener").unwrap();

        // Emitter script emits in _ready.
        let emitter_script = "\
extends Node2D
func _ready():
    emit_signal(\"game_over\")
";
        let es = GDScriptNodeInstance::from_source(emitter_script, emitter_id).unwrap();
        tree.attach_script(emitter_id, Box::new(es));

        // Listener script handles the signal.
        let listener_script = "\
extends Node2D
var game_ended = false
func _on_game_over():
    self.game_ended = true
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        // Fire _ready on emitter — this calls emit_signal("game_over")
        // which should dispatch to listener's script.
        LifecycleManager::enter_tree(&mut tree, emitter_id);

        assert_eq!(
            tree.get_script(listener_id)
                .unwrap()
                .get_property("game_ended"),
            Some(Variant::Bool(true))
        );
    }

    /// Signal with string argument dispatched correctly.
    #[test]
    fn signal_string_arg_dispatched() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let listener_id = tree.add_child(root, listener).unwrap();

        let listener_script = "\
extends Node2D
var msg = \"\"
func _on_message(text):
    self.msg = text
";
        let ls = GDScriptNodeInstance::from_source(listener_script, listener_id).unwrap();
        tree.attach_script(listener_id, Box::new(ls));

        tree.connect_signal(
            emitter_id,
            "message",
            gdobject::signal::Connection::new(listener_id.object_id(), "_on_message"),
        );

        tree.emit_signal(emitter_id, "message", &[Variant::String("hello".into())]);

        assert_eq!(
            tree.get_script(listener_id).unwrap().get_property("msg"),
            Some(Variant::String("hello".into()))
        );
    }

    // -- Runtime node creation / deletion tests ----------------------------

    #[test]
    fn script_creates_node_with_new_and_add_child() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let spawner = Node::new("Spawner", "Node2D");
        let spawner_id = tree.add_child(root, spawner).unwrap();

        let script_src = "\
extends Node2D
var spawned = false
func _process(delta):
    if not self.spawned:
        var bullet = Node2D.new()
        add_child(bullet)
        self.spawned = true
";
        let script = GDScriptNodeInstance::from_source(script_src, spawner_id).unwrap();
        tree.attach_script(spawner_id, Box::new(script));

        assert_eq!(tree.get_node(spawner_id).unwrap().children().len(), 0);

        tree.process_all_scripts_process(1.0 / 60.0);

        let children = tree.get_node(spawner_id).unwrap().children();
        assert_eq!(
            children.len(),
            1,
            "spawner should have 1 child after _process"
        );

        let child_id = children[0];
        let child = tree.get_node(child_id).unwrap();
        assert_eq!(child.class_name(), "Node2D");
    }

    #[test]
    fn script_queue_free_removes_node_after_process_deletions() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let enemy = Node::new("Enemy", "Node2D");
        let enemy_id = tree.add_child(root, enemy).unwrap();

        let script_src = "\
extends Node2D
func _process(delta):
    queue_free()
";
        let script = GDScriptNodeInstance::from_source(script_src, enemy_id).unwrap();
        tree.attach_script(enemy_id, Box::new(script));

        assert!(tree.get_node(enemy_id).is_some());

        tree.process_all_scripts_process(1.0 / 60.0);

        // Enemy still exists (queue_free is deferred).
        assert!(tree.get_node(enemy_id).is_some());
        assert_eq!(tree.pending_deletion_count(), 1);

        tree.process_deletions();
        assert!(tree.get_node(enemy_id).is_none());
        assert_eq!(tree.pending_deletion_count(), 0);
    }

    #[test]
    fn queue_free_on_object_id_removes_child() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();

        let child = Node::new("Child", "Node2D");
        let _child_id = tree.add_child(parent_id, child).unwrap();

        let script_src = "\
extends Node2D
func _process(delta):
    var kids = get_children()
    for kid in kids:
        kid.queue_free()
";
        let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
        tree.attach_script(parent_id, Box::new(script));

        tree.process_all_scripts_process(1.0 / 60.0);
        tree.process_deletions();

        assert_eq!(tree.get_node(parent_id).unwrap().children().len(), 0);
    }

    #[test]
    fn object_instance_validity_lifecycle() {
        // pat-nzgdu.32: is_instance_valid / instance_from_id / weakref().get_ref()
        // must all consult the live-object registry (the scene-tree arena) rather
        // than a bare non-nil check. A child freed via `Object.free()` (immediate)
        // must report invalid the moment it is freed.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let owner = Node::new("Owner", "Node");
        let owner_id = tree.add_child(root, owner).unwrap();

        let victim = Node::new("Victim", "Node");
        let victim_id = tree.add_child(owner_id, victim).unwrap();

        let script_src = "\
extends Node
var did_run = false
var cid = 0
var valid_before = false
var resolved_before = 0
var valid_after = true
var weak_after = 0
var resolved_after = 0
func _process(delta):
    if did_run:
        return
    did_run = true
    var child = get_node(\"Victim\")
    cid = child.get_instance_id()
    var w = weakref(child)
    valid_before = is_instance_valid(child)
    resolved_before = instance_from_id(cid)
    child.free()
    valid_after = is_instance_valid(child)
    weak_after = w.get_ref()
    resolved_after = instance_from_id(cid)
";
        let script = GDScriptNodeInstance::from_source(script_src, owner_id).unwrap();
        tree.attach_script(owner_id, Box::new(script));

        tree.process_all_scripts_process(1.0 / 60.0);

        // The immediate `free()` removed the victim from the arena.
        assert!(tree.get_node(victim_id).is_none());

        let s = tree.get_script(owner_id).unwrap();
        // While alive: valid, and resolvable by id.
        assert_eq!(s.get_property("valid_before"), Some(Variant::Bool(true)));
        assert!(
            matches!(
                s.get_property("resolved_before"),
                Some(Variant::ObjectId(_))
            ),
            "instance_from_id should resolve a live object, got {:?}",
            s.get_property("resolved_before")
        );
        // After free(): invalid, weakref lost, id no longer resolvable.
        assert_eq!(s.get_property("valid_after"), Some(Variant::Bool(false)));
        assert_eq!(s.get_property("weak_after"), Some(Variant::Nil));
        assert_eq!(s.get_property("resolved_after"), Some(Variant::Nil));
    }

    #[test]
    fn queue_free_defers_instance_validity_until_reap() {
        // A `queue_free`d node stays *valid* until the end-of-frame reap
        // (`process_deletions`), matching Godot: the deferred deletion has not
        // happened yet, so `is_instance_valid` still reports true in-frame.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let owner = Node::new("Owner", "Node");
        let owner_id = tree.add_child(root, owner).unwrap();

        let victim = Node::new("Victim", "Node");
        let victim_id = tree.add_child(owner_id, victim).unwrap();

        let script_src = "\
extends Node
var did_run = false
var valid_after_queue_free = false
func _process(delta):
    if did_run:
        return
    did_run = true
    var child = get_node(\"Victim\")
    child.queue_free()
    valid_after_queue_free = is_instance_valid(child)
";
        let script = GDScriptNodeInstance::from_source(script_src, owner_id).unwrap();
        tree.attach_script(owner_id, Box::new(script));

        tree.process_all_scripts_process(1.0 / 60.0);

        // Deferred: still valid and still in the arena this frame.
        assert_eq!(
            tree.get_script(owner_id)
                .unwrap()
                .get_property("valid_after_queue_free"),
            Some(Variant::Bool(true))
        );
        assert!(tree.get_node(victim_id).is_some());
        assert_eq!(tree.pending_deletion_count(), 1);

        // After the reap the node is gone from the arena -> future
        // is_instance_valid checks (via the registry) would report false.
        tree.process_deletions();
        assert!(tree.get_node(victim_id).is_none());
    }

    #[test]
    fn spawned_nodes_get_process_called_on_next_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let spawner = Node::new("Spawner", "Node2D");
        let spawner_id = tree.add_child(root, spawner).unwrap();

        let spawner_script = "\
extends Node2D
var did_spawn = false
func _process(delta):
    if not self.did_spawn:
        var bullet = Node2D.new()
        add_child(bullet)
        self.did_spawn = true
";
        let script = GDScriptNodeInstance::from_source(spawner_script, spawner_id).unwrap();
        tree.attach_script(spawner_id, Box::new(script));

        // Frame 1: spawner creates bullet.
        tree.process_all_scripts_process(1.0 / 60.0);
        tree.process_deletions();

        let children = tree.get_node(spawner_id).unwrap().children();
        assert_eq!(children.len(), 1);
        let bullet_id = children[0];

        // Attach a script to the bullet that records it was processed.
        let bullet_script_src = "\
extends Node2D
var processed = false
func _process(delta):
    self.processed = true
";
        let bullet_script =
            GDScriptNodeInstance::from_source(bullet_script_src, bullet_id).unwrap();
        tree.attach_script(bullet_id, Box::new(bullet_script));

        // Frame 2: both spawner and bullet get _process.
        tree.process_all_scripts_process(1.0 / 60.0);
        tree.process_deletions();

        let processed = tree
            .get_script(bullet_id)
            .unwrap()
            .get_property("processed");
        assert_eq!(processed, Some(Variant::Bool(true)));
    }

    #[test]
    fn get_parent_add_child_on_parent_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let parent = Node::new("Parent", "Node2D");
        let parent_id = tree.add_child(root, parent).unwrap();

        let child = Node::new("Child", "Node2D");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let script_src = "\
extends Node2D
var did_spawn = false
func _process(delta):
    if not self.did_spawn:
        var bullet = Node2D.new()
        get_parent().add_child(bullet)
        self.did_spawn = true
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        tree.process_all_scripts_process(1.0 / 60.0);

        let parent_children = tree.get_node(parent_id).unwrap().children();
        assert_eq!(parent_children.len(), 2);
    }

    #[test]
    fn create_node_sets_correct_class_name() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let spawner = Node::new("Spawner", "Node2D");
        let spawner_id = tree.add_child(root, spawner).unwrap();

        let script_src = "\
extends Node2D
var child_class = \"\"
func _process(delta):
    var area = Area2D.new()
    add_child(area)
    self.child_class = area.get_class()
";
        let script = GDScriptNodeInstance::from_source(script_src, spawner_id).unwrap();
        tree.attach_script(spawner_id, Box::new(script));

        tree.process_all_scripts_process(1.0 / 60.0);

        let cls = tree
            .get_script(spawner_id)
            .unwrap()
            .get_property("child_class");
        assert_eq!(cls, Some(Variant::String("Area2D".into())));
    }

    #[test]
    fn queue_free_idempotent() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let node = Node::new("N", "Node2D");
        let nid = tree.add_child(root, node).unwrap();

        tree.queue_free(nid);
        tree.queue_free(nid);
        assert_eq!(tree.pending_deletion_count(), 1);

        tree.process_deletions();
        assert!(tree.get_node(nid).is_none());
    }

    #[test]
    fn mainloop_processes_deletions_each_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let enemy = Node::new("Enemy", "Node2D");
        let enemy_id = tree.add_child(root, enemy).unwrap();

        let script_src = "\
extends Node2D
func _process(delta):
    queue_free()
";
        let script = GDScriptNodeInstance::from_source(script_src, enemy_id).unwrap();
        tree.attach_script(enemy_id, Box::new(script));

        let mut ml = MainLoop::new(tree);
        ml.run_frames(1, 1.0 / 60.0);

        assert!(ml.tree().get_node(enemy_id).is_none());
    }

    // -- End-to-end Input tests through scene tree ----------------------------

    const INPUT_MOVEMENT_SCRIPT: &str = "\
extends Node2D
var speed = 200.0
var moved_right = false
func _process(delta):
    if Input.is_action_pressed(\"ui_right\"):
        self.moved_right = true
";

    #[test]
    fn input_action_pressed_through_scene_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let player = Node::new("Player", "Node2D");
        let player_id = player.id();
        tree.add_child(root, player).unwrap();

        let script = GDScriptNodeInstance::from_source(INPUT_MOVEMENT_SCRIPT, player_id).unwrap();
        tree.attach_script(player_id, Box::new(script));

        // Set up input: ui_right is pressed
        let mut snapshot = InputSnapshot::default();
        snapshot.pressed_keys.insert("ArrowRight".to_string());
        snapshot
            .input_map
            .insert("ui_right".to_string(), vec!["ArrowRight".to_string()]);
        tree.set_input_snapshot(snapshot);

        // Run _ready + one _process frame
        tree.process_script_ready(player_id);
        tree.process_script_process(player_id, 1.0 / 60.0);

        // Verify the script saw Input.is_action_pressed("ui_right") == true
        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("moved_right"),
            Some(Variant::Bool(true)),
            "script should detect ui_right is pressed via Input singleton"
        );
    }

    #[test]
    fn input_action_not_pressed_through_scene_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let player = Node::new("Player", "Node2D");
        let player_id = player.id();
        tree.add_child(root, player).unwrap();

        let script = GDScriptNodeInstance::from_source(INPUT_MOVEMENT_SCRIPT, player_id).unwrap();
        tree.attach_script(player_id, Box::new(script));

        // Set up input: nothing pressed
        let snapshot = InputSnapshot::default();
        tree.set_input_snapshot(snapshot);

        tree.process_script_ready(player_id);
        tree.process_script_process(player_id, 1.0 / 60.0);

        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("moved_right"),
            Some(Variant::Bool(false)),
            "script should detect no input when nothing is pressed"
        );
    }

    #[test]
    fn input_just_pressed_only_on_first_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let player = Node::new("Player", "Node2D");
        let player_id = player.id();
        tree.add_child(root, player).unwrap();

        let script_src = "\
extends Node2D
var fire_count = 0
func _process(delta):
    if Input.is_action_just_pressed(\"shoot\"):
        self.fire_count = self.fire_count + 1
";
        let script = GDScriptNodeInstance::from_source(script_src, player_id).unwrap();
        tree.attach_script(player_id, Box::new(script));

        // Frame 1: shoot just pressed
        let mut snap1 = InputSnapshot::default();
        snap1.pressed_keys.insert("x".to_string());
        snap1.just_pressed_keys.insert("x".to_string());
        snap1
            .input_map
            .insert("shoot".to_string(), vec!["x".to_string()]);
        tree.set_input_snapshot(snap1);
        tree.process_script_process(player_id, 1.0 / 60.0);

        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("fire_count"),
            Some(Variant::Int(1)),
            "should fire on first frame (just_pressed)"
        );

        // Frame 2: shoot still held but NOT just pressed
        let mut snap2 = InputSnapshot::default();
        snap2.pressed_keys.insert("x".to_string());
        // just_pressed_keys is empty — key was already held
        snap2
            .input_map
            .insert("shoot".to_string(), vec!["x".to_string()]);
        tree.set_input_snapshot(snap2);
        tree.process_script_process(player_id, 1.0 / 60.0);

        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("fire_count"),
            Some(Variant::Int(1)),
            "should NOT fire on second frame (not just_pressed)"
        );
    }

    #[test]
    fn input_get_vector_through_scene_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let player = Node::new("Player", "Node2D");
        let player_id = player.id();
        tree.add_child(root, player).unwrap();

        let script_src = "\
extends Node2D
var dir_x = 0.0
var dir_y = 0.0
func _process(delta):
    var dir = Input.get_vector(\"ui_left\", \"ui_right\", \"ui_up\", \"ui_down\")
    self.dir_x = dir.x
    self.dir_y = dir.y
";
        let script = GDScriptNodeInstance::from_source(script_src, player_id).unwrap();
        tree.attach_script(player_id, Box::new(script));

        let mut snapshot = InputSnapshot::default();
        snapshot.pressed_keys.insert("ArrowRight".to_string());
        snapshot
            .input_map
            .insert("ui_left".to_string(), vec!["ArrowLeft".to_string()]);
        snapshot
            .input_map
            .insert("ui_right".to_string(), vec!["ArrowRight".to_string()]);
        snapshot
            .input_map
            .insert("ui_up".to_string(), vec!["ArrowUp".to_string()]);
        snapshot
            .input_map
            .insert("ui_down".to_string(), vec!["ArrowDown".to_string()]);
        tree.set_input_snapshot(snapshot);

        tree.process_script_process(player_id, 1.0 / 60.0);

        let script = tree.get_script(player_id).unwrap();
        assert_eq!(
            script.get_property("dir_x"),
            Some(Variant::Float(1.0)),
            "get_vector should return x=1.0 when ui_right pressed"
        );
        assert_eq!(
            script.get_property("dir_y"),
            Some(Variant::Float(0.0)),
            "get_vector should return y=0.0 when only ui_right pressed"
        );
    }

    // ── B006: Script call/return trace tests ────────────────────────────

    mod trace_script_tests {
        use super::*;
        use crate::trace::TraceEventType;

        #[test]
        fn trace_script_call_return_on_ready() {
            let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
            tree.event_trace_mut().enable();

            let script_src = "\
extends Node2D
var x = 0
func _ready():
    self.x = 42
";
            let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
            tree.attach_script(parent_id, Box::new(script));
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let script_events: Vec<_> = events
                .iter()
                .filter(|e| {
                    e.event_type == TraceEventType::ScriptCall
                        || e.event_type == TraceEventType::ScriptReturn
                })
                .collect();

            // Should have call/return pairs for _enter_tree and _ready
            // (at least _ready since the script defines it)
            let ready_calls: Vec<_> = script_events
                .iter()
                .filter(|e| e.detail == "_ready" && e.event_type == TraceEventType::ScriptCall)
                .collect();
            let ready_returns: Vec<_> = script_events
                .iter()
                .filter(|e| e.detail == "_ready" && e.event_type == TraceEventType::ScriptReturn)
                .collect();
            assert_eq!(ready_calls.len(), 1);
            assert_eq!(ready_returns.len(), 1);
        }

        #[test]
        fn trace_script_call_return_on_process() {
            let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();
            let script_src = "\
extends Node2D
var counter = 0
func _process(delta):
    self.counter = self.counter + 1
";
            let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
            tree.attach_script(parent_id, Box::new(script));

            tree.event_trace_mut().enable();
            tree.process_all_scripts_process(1.0 / 60.0);

            let events = tree.event_trace().events();
            let call_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::ScriptCall && e.detail == "_process")
                .collect();
            let return_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::ScriptReturn && e.detail == "_process")
                .collect();

            assert_eq!(call_events.len(), 1);
            assert_eq!(return_events.len(), 1);
        }

        #[test]
        fn trace_signal_emit_from_script() {
            use std::sync::{
                atomic::{AtomicUsize, Ordering},
                Arc,
            };
            let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

            let counter = Arc::new(AtomicUsize::new(0));
            let counter_clone = counter.clone();
            let conn = gdobject::signal::Connection::with_callback(
                ObjectId::from_raw(parent_id.raw()),
                "on_hit",
                move |_args| {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                    Variant::Nil
                },
            );
            tree.connect_signal(parent_id, "hit", conn);

            tree.event_trace_mut().enable();

            let script_src = "\
extends Node2D
func _ready():
    emit_signal(\"hit\")
";
            let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
            tree.attach_script(parent_id, Box::new(script));
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let signal_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::SignalEmit)
                .collect();

            assert!(
                !signal_events.is_empty(),
                "Signal emission should be traced"
            );
            // The script's `emit_signal("hit")` must be traced. It is NOT
            // necessarily the FIRST signal: `enter_tree` legitimately emits
            // `tree_entered` first (lifecycle.rs:50, matching Godot), so
            // asserting `signal_events[0]` pinned an ordering the engine never
            // promised. Assert the intent — that "hit" was traced — instead.
            assert!(
                signal_events.iter().any(|e| e.detail == "hit"),
                "the script's emit_signal(\"hit\") must be traced; got {:?}",
                signal_events.iter().map(|e| &e.detail).collect::<Vec<_>>()
            );
        }
    }

    // ── B008: Cross-node property reads and script var sync tests ───────

    mod b008_sync_tests {
        use super::*;

        #[test]
        fn cross_node_read_position_via_get_node() {
            // nodeA reads nodeB.position via get_node("../NodeB").position
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let parent = Node::new("Parent", "Node2D");
            let parent_id = tree.add_child(root, parent).unwrap();

            let mut node_a = Node::new("NodeA", "Node2D");
            let node_a_id = tree.add_child(parent_id, node_a).unwrap();

            let mut node_b = Node::new("NodeB", "Node2D");
            let node_b_id = tree.add_child(parent_id, node_b).unwrap();

            // Set NodeB's position as a node property
            tree.get_node_mut(node_b_id)
                .unwrap()
                .set_property("position_x", Variant::Float(42.5));

            // NodeA reads NodeB's position_x
            let script_src = "\
extends Node2D
var read_value = 0.0
func _ready():
    var b = get_node(\"../NodeB\")
    self.read_value = b.position_x
";
            let script = GDScriptNodeInstance::from_source(script_src, node_a_id).unwrap();
            tree.attach_script(node_a_id, Box::new(script));
            LifecycleManager::enter_tree(&mut tree, node_a_id);

            assert_eq!(
                tree.get_script(node_a_id)
                    .unwrap()
                    .get_property("read_value"),
                Some(Variant::Float(42.5)),
                "NodeA should read NodeB.position_x = 42.5"
            );
        }

        #[test]
        fn cross_node_read_script_var_via_scene_access() {
            // nodeA reads nodeB's script variable
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let parent = Node::new("Parent", "Node2D");
            let parent_id = tree.add_child(root, parent).unwrap();

            let node_a = Node::new("NodeA", "Node2D");
            let node_a_id = tree.add_child(parent_id, node_a).unwrap();

            let node_b = Node::new("NodeB", "Node2D");
            let node_b_id = tree.add_child(parent_id, node_b).unwrap();

            // NodeB has a script var
            let b_script = "\
extends Node2D
var health = 75
";
            let bs = GDScriptNodeInstance::from_source(b_script, node_b_id).unwrap();
            tree.attach_script(node_b_id, Box::new(bs));

            // NodeA reads it
            let a_script = "\
extends Node2D
var other_health = 0
func _ready():
    var b = get_node(\"../NodeB\")
    self.other_health = b.health
";
            let a_s = GDScriptNodeInstance::from_source(a_script, node_a_id).unwrap();
            tree.attach_script(node_a_id, Box::new(a_s));
            LifecycleManager::enter_tree(&mut tree, parent_id);

            assert_eq!(
                tree.get_script(node_a_id)
                    .unwrap()
                    .get_property("other_health"),
                Some(Variant::Int(75)),
                "NodeA should read NodeB's script var health = 75"
            );
        }

        #[test]
        fn script_var_sync_after_ready() {
            let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

            let script_src = "\
extends Node2D
var score = 0
func _ready():
    self.score = 999
";
            let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
            tree.attach_script(parent_id, Box::new(script));
            LifecycleManager::enter_tree(&mut tree, parent_id);

            // After _ready, script var should be synced to node property
            let node_val = tree.get_node(parent_id).unwrap().get_property("score");
            assert_eq!(
                node_val,
                Variant::Int(999),
                "Script var should sync to node property after _ready"
            );
        }

        #[test]
        fn script_var_sync_after_process() {
            let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

            let script_src = "\
extends Node2D
var ticks = 0
func _process(delta):
    self.ticks = self.ticks + 1
";
            let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
            tree.attach_script(parent_id, Box::new(script));

            tree.process_all_scripts_process(1.0 / 60.0);

            // Check sync to node property
            let node_val = tree.get_node(parent_id).unwrap().get_property("ticks");
            assert_eq!(
                node_val,
                Variant::Int(1),
                "Script var should sync to node property after _process"
            );

            tree.process_all_scripts_process(1.0 / 60.0);
            let node_val = tree.get_node(parent_id).unwrap().get_property("ticks");
            assert_eq!(
                node_val,
                Variant::Int(2),
                "Script var should sync after second _process"
            );
        }

        #[test]
        fn script_var_sync_after_physics_process() {
            let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

            let script_src = "\
extends Node2D
var physics_ticks = 0
func _physics_process(delta):
    self.physics_ticks = self.physics_ticks + 1
";
            let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
            tree.attach_script(parent_id, Box::new(script));

            tree.process_all_scripts_physics_process(1.0 / 60.0);

            let node_val = tree
                .get_node(parent_id)
                .unwrap()
                .get_property("physics_ticks");
            assert_eq!(
                node_val,
                Variant::Int(1),
                "Script var should sync to node property after _physics_process"
            );
        }

        #[test]
        fn script_var_sync_after_enter_tree() {
            let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

            let script_src = "\
extends Node2D
var entered = false
func _enter_tree():
    self.entered = true
";
            let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
            tree.attach_script(parent_id, Box::new(script));
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let node_val = tree.get_node(parent_id).unwrap().get_property("entered");
            assert_eq!(
                node_val,
                Variant::Bool(true),
                "Script var should sync to node property after _enter_tree"
            );
        }

        #[test]
        fn script_var_sync_consistent_across_callbacks() {
            // Verify sync happens after EVERY callback type within a single node.
            let (mut tree, parent_id, _c1_id, _c2_id) = build_tree_with_children();

            let script_src = "\
extends Node2D
var stage = 0
func _enter_tree():
    self.stage = 1
func _ready():
    self.stage = 2
func _process(delta):
    self.stage = self.stage + 10
func _physics_process(delta):
    self.stage = self.stage + 100
";
            let script = GDScriptNodeInstance::from_source(script_src, parent_id).unwrap();
            tree.attach_script(parent_id, Box::new(script));

            // _enter_tree sets stage=1, then _ready sets stage=2
            LifecycleManager::enter_tree(&mut tree, parent_id);
            assert_eq!(
                tree.get_node(parent_id).unwrap().get_property("stage"),
                Variant::Int(2),
                "stage should be 2 after _ready"
            );

            // _process adds 10 -> 12
            tree.process_all_scripts_process(1.0 / 60.0);
            assert_eq!(
                tree.get_node(parent_id).unwrap().get_property("stage"),
                Variant::Int(12),
                "stage should be 12 after _process"
            );

            // _physics_process adds 100 -> 112
            tree.process_all_scripts_physics_process(1.0 / 60.0);
            assert_eq!(
                tree.get_node(parent_id).unwrap().get_property("stage"),
                Variant::Int(112),
                "stage should be 112 after _physics_process"
            );
        }

        #[test]
        fn cross_node_read_synced_property() {
            // After nodeB's script sets a var in _ready, nodeA can read
            // the synced value as a node property.
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let parent = Node::new("Parent", "Node2D");
            let parent_id = tree.add_child(root, parent).unwrap();

            let node_b = Node::new("NodeB", "Node2D");
            let node_b_id = tree.add_child(parent_id, node_b).unwrap();

            let node_a = Node::new("NodeA", "Node2D");
            let node_a_id = tree.add_child(parent_id, node_a).unwrap();

            // NodeB sets speed in _ready
            let b_script = "\
extends Node2D
var speed = 0
func _ready():
    self.speed = 200
";
            let bs = GDScriptNodeInstance::from_source(b_script, node_b_id).unwrap();
            tree.attach_script(node_b_id, Box::new(bs));

            // NodeA reads NodeB.speed in _ready (NodeB is ready first since bottom-up)
            let a_script = "\
extends Node2D
var b_speed = 0
func _ready():
    var b = get_node(\"../NodeB\")
    self.b_speed = b.speed
";
            let a_s = GDScriptNodeInstance::from_source(a_script, node_a_id).unwrap();
            tree.attach_script(node_a_id, Box::new(a_s));

            LifecycleManager::enter_tree(&mut tree, parent_id);

            // NodeB's _ready fires before NodeA's (bottom-up, and NodeB is listed before NodeA)
            assert_eq!(
                tree.get_script(node_a_id).unwrap().get_property("b_speed"),
                Some(Variant::Int(200)),
                "NodeA should read synced value from NodeB"
            );
        }
    }
}
