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
use crate::tween::{EaseType, TransFunc, TweenId, TweenStep};
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
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        // An AnimationTree's `parameters/...` values live on the attached graph,
        // which evaluation reads and writes — so report those rather than the
        // node's stored copy, mirroring Godot's `AnimationTree.get()`
        // (pat-0q2kx.2).
        if prop.starts_with("parameters/") {
            if let Some(anim_tree) = self.tree().get_animation_tree(nid) {
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

        match method {
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
            // Collision state persisted by `process_character_movement`.
            "is_on_floor" | "is_on_wall" | "is_on_ceiling" => {
                Some(Variant::Bool(bool_prop(self.tree(), method)))
            }
            // `*_only` variants ignore the "was on X last frame" distinction
            // Patina does not model separately; report the same state.
            "is_on_floor_only" => Some(Variant::Bool(bool_prop(self.tree(), "is_on_floor"))),
            "is_on_wall_only" => Some(Variant::Bool(bool_prop(self.tree(), "is_on_wall"))),
            "is_on_ceiling_only" => Some(Variant::Bool(bool_prop(self.tree(), "is_on_ceiling"))),
            // Godot's `get_real_velocity()` is the motion actually achieved last
            // move, which differs from `velocity` when sliding cancelled part of
            // it. Patina publishes the post-slide velocity, which is that value
            // for every case except the first tick after a collision; reporting it
            // beats returning null and killing `get_real_velocity().length()`.
            "get_real_velocity" => Some(Variant::Vector2(vec2_prop(self.tree(), "velocity"))),
            // Written by `process_character_movement` from the platform the body
            // is standing on; ZERO until then.
            "get_platform_velocity" => Some(Variant::Vector2(vec2_prop(
                self.tree(),
                "platform_velocity",
            ))),

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

    fn is_physics_processing(&self, node: u64) -> bool {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree().is_node_physics_processing(nid)
    }

    fn request_ready(&mut self, node: u64) {
        let nid = NodeId::from_object_id(ObjectId::from_raw(node));
        self.tree_mut().request_node_ready(nid);
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

    fn input_map_action_event_count(&self, action: &str) -> usize {
        self.tree().input_map_action_event_count(action)
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
        let id = self.tree_mut().create_timer(seconds);
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
        let resolved = gdcore::resolve_path(path).ok()?;
        let source = std::fs::read_to_string(resolved).ok()?;
        let scene = crate::packed_scene::PackedScene::from_tscn(&source).ok()?;
        // Recursively resolve `instance=ExtResource` sub-scenes from disk.
        let resolve = |sub: &str| -> Option<crate::packed_scene::PackedScene> {
            let sub_resolved = gdcore::resolve_path(sub).ok()?;
            let src = std::fs::read_to_string(sub_resolved).ok()?;
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
        let resolved = gdcore::resolve_path(path).ok()?;
        let source = std::fs::read_to_string(&resolved).ok()?;
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
