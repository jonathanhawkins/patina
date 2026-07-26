//! Script instance traits and supporting types for scripting interop.
//!
//! This module defines the `ScriptInstance` trait that every scripting backend
//! (GDScript, Rust-native, etc.) must implement, along with metadata structs
//! for method and property introspection.

use std::fmt;

use gdvariant::variant::VariantType;
use gdvariant::Variant;

/// Bitflags describing method characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodFlags(u8);

impl MethodFlags {
    /// A regular instance method.
    pub const NORMAL: Self = Self(0b001);
    /// A virtual method (overridable by subclasses).
    pub const VIRTUAL: Self = Self(0b010);
    /// A const method (does not mutate the object).
    pub const CONST: Self = Self(0b100);

    /// Returns `true` if `self` contains all the flags in `other`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for MethodFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl fmt::Display for MethodFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.contains(Self::NORMAL) {
            parts.push("NORMAL");
        }
        if self.contains(Self::VIRTUAL) {
            parts.push("VIRTUAL");
        }
        if self.contains(Self::CONST) {
            parts.push("CONST");
        }
        if parts.is_empty() {
            write!(f, "(none)")
        } else {
            write!(f, "{}", parts.join(" | "))
        }
    }
}

/// Metadata describing a single method on a script.
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// The method name.
    pub name: String,
    /// Names of the method's arguments.
    pub argument_names: Vec<String>,
    /// The return type of the method.
    pub return_type: VariantType,
    /// Method flags.
    pub flags: MethodFlags,
}

impl fmt::Display for MethodInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}({}) -> {} [{}]",
            self.name,
            self.argument_names.join(", "),
            self.return_type,
            self.flags,
        )
    }
}

/// Metadata describing a single property on a script.
#[derive(Debug, Clone)]
pub struct ScriptPropertyInfo {
    /// The property name.
    pub name: String,
    /// The type of the property value.
    pub property_type: VariantType,
    /// The default value for this property.
    pub default_value: Variant,
    /// Godot `PROPERTY_HINT_*` code (0 = `PROPERTY_HINT_NONE`) derived from an
    /// `@export*` annotation; drives the inspector editor (pat-nzgdu.5).
    pub hint: i64,
    /// Hint string paired with [`hint`](Self::hint), e.g. `"0,100,5"` for
    /// `@export_range(0,100,5)` or `"Idle,Run"` for `@export_enum("Idle","Run")`.
    pub hint_string: String,
}

/// Errors that can occur when interacting with a script instance.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScriptError {
    /// The requested method does not exist on the script.
    #[error("method not found: '{0}'")]
    MethodNotFound(String),

    /// The wrong number of arguments were passed to a method.
    #[error("invalid argument count: expected {expected}, got {got}")]
    InvalidArgCount {
        /// Expected number of arguments.
        expected: usize,
        /// Actual number of arguments provided.
        got: usize,
    },

    /// The requested property does not exist on the script.
    #[error("property not found: '{0}'")]
    PropertyNotFound(String),

    /// A type mismatch occurred during a script operation.
    #[error("type error: {0}")]
    TypeError(String),

    /// No script is attached to the given object.
    #[error("no script attached to object")]
    NoScript,
}

/// Trait implemented by all script instances.
///
/// Each scripting backend provides its own concrete type that implements this
/// trait, allowing the engine to call methods, get/set properties, and
/// introspect the script's API uniformly.
pub trait ScriptInstance {
    /// Calls a method by name with the given arguments.
    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError>;

    /// Gets a property value by name, or `None` if it does not exist.
    fn get_property(&self, name: &str) -> Option<Variant>;

    /// Sets a property value by name. Returns `true` if the property existed.
    fn set_property(&mut self, name: &str, value: Variant) -> bool;

    /// Returns metadata for all methods exposed by this script.
    fn list_methods(&self) -> Vec<MethodInfo>;

    /// Returns metadata for all properties exposed by this script.
    fn list_properties(&self) -> Vec<ScriptPropertyInfo>;

    /// Names of the signals this script declares (GDScript `signal foo`).
    ///
    /// Needed to resolve `$Other.my_signal` into a `Signal` value: a declared
    /// signal that has never been connected has no entry in the node's
    /// `SignalStore`, so the store alone reports a false negative in exactly
    /// the connect-before-emit case (pat-0q2kx.9). Default: none.
    /// The script's `@rpc`-annotated methods as `(name, annotation_args)`.
    ///
    /// Defaults to empty so non-GDScript script instances need no change; the
    /// host registers whatever comes back with its MultiplayerAPI
    /// (pat-0q2kx.79).
    fn rpc_methods(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }

    fn list_signals(&self) -> Vec<String> {
        Vec::new()
    }

    /// Returns the human-readable name of this script.
    fn get_script_name(&self) -> &str;

    /// Returns `true` if the script defines a method with the given name.
    fn has_method(&self, name: &str) -> bool {
        self.list_methods().iter().any(|m| m.name == name)
    }

    /// Resolves `@onready` variables by evaluating their default expressions.
    /// Called by the scene tree just before `_ready` fires.
    /// Default implementation is a no-op.
    fn resolve_onready(&mut self) -> Result<(), ScriptError> {
        Ok(())
    }

    /// Inject scene-tree access so the script can call get_node / emit_signal.
    fn set_scene_access(&mut self, _access: Box<dyn SceneAccess>, _node_id: u64) {}

    /// Remove scene-tree access after a script callback finishes.
    fn clear_scene_access(&mut self) {}

    /// Advances any coroutines suspended on `await` by `delta` seconds, resuming
    /// those whose frame waits have elapsed. Driven once per frame by the
    /// SceneTree tick so `await` actually progresses at runtime. Default no-op
    /// for scripts without coroutine support (pat-nzgdu.6).
    fn poll_coroutines(&mut self, _delta: f32) {}

    /// Notifies suspended coroutines that a signal named `name` fired with
    /// `args`, resuming any `await` parked on it and binding the argument(s) as
    /// the awaited value. Driven by the SceneTree after each `emit_signal` and
    /// on SceneTreeTimer timeouts. Default no-op (pat-nzgdu.6).
    fn notify_await_signal(&mut self, _name: &str, _args: &[Variant]) {}

    /// [`notify_await_signal`](Self::notify_await_signal) with the raw
    /// `ObjectId` of the emitting object, so an `await` bound to a specific
    /// object ignores a same-named signal from a different one (pat-0q2kx.9).
    ///
    /// Defaults to the unattributed form, so existing implementors keep their
    /// current behaviour until they opt in.
    fn notify_await_signal_from(&mut self, _source: u64, name: &str, args: &[Variant]) {
        self.notify_await_signal(name, args);
    }

    /// Delivers an engine notification (`what`, a Godot `NOTIFICATION_*` code)
    /// to the script's `_notification(int what)` virtual, if it overrides it.
    /// This is how PREDELETE, ENTER_TREE/READY/EXIT_TREE, PAUSED/UNPAUSED,
    /// PROCESS/PHYSICS_PROCESS, etc. reach user code in Godot. Default no-op
    /// for scripts without `_notification` support (pat-nzgdu.13).
    fn notification(&mut self, _what: i64) {}
}

/// A physical input event described the way `project.godot` serializes one, so
/// `InputMap` rebinding crosses the host boundary without this crate depending
/// on `gdplatform`'s typed `ActionBinding` (pat-0q2kx.89).
///
/// Godot's `InputMap.action_add_event` takes a real `InputEvent` object; Patina
/// has no `InputEvent` class family yet, so a script names the event with a
/// keycode (`InputMap.action_add_event("dash", KEY_Q)`) or a dictionary
/// (`{"class": "InputEventJoypadMotion", "axis": 0, "axis_value": -1.0}`) —
/// the same fields the `[input]` parser reads.
#[derive(Debug, Clone, PartialEq)]
pub struct InputEventSpec {
    /// Godot event class: `InputEventKey`, `InputEventMouseButton`,
    /// `InputEventJoypadButton` or `InputEventJoypadMotion`.
    pub class: String,
    /// Keycode, mouse button index, joypad button index, or joypad axis index,
    /// per `class`.
    pub code: i64,
    /// For `InputEventJoypadMotion`, the signed deflection the binding presses
    /// on; ignored otherwise.
    pub axis_value: f32,
    /// Shift must be held (`InputEventKey` only).
    pub shift: bool,
    /// Ctrl must be held (`InputEventKey` only).
    pub ctrl: bool,
    /// Alt must be held (`InputEventKey` only).
    pub alt: bool,
}

impl InputEventSpec {
    /// A bare key event for `keycode`, the form
    /// `InputMap.action_add_event(action, KEY_Q)` produces.
    pub fn key(keycode: i64) -> Self {
        Self {
            class: "InputEventKey".to_string(),
            code: keycode,
            axis_value: 0.0,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    /// Reads the dictionary form, defaulting `class` to `InputEventKey` and
    /// accepting either `keycode`/`button_index`/`axis` for the code.
    pub fn from_dictionary(dict: &gdvariant::Dictionary) -> Option<Self> {
        let get = |k: &str| dict.get(Variant::String(k.to_string())).cloned();
        let as_int = |v: Option<Variant>| match v {
            Some(Variant::Int(i)) => Some(i),
            Some(Variant::Float(f)) => Some(f as i64),
            _ => None,
        };
        let as_bool = |v: Option<Variant>| matches!(v, Some(Variant::Bool(true)));
        let class = match get("class") {
            Some(Variant::String(s)) => s,
            _ => "InputEventKey".to_string(),
        };
        let code = as_int(get("keycode"))
            .or_else(|| as_int(get("button_index")))
            .or_else(|| as_int(get("axis")))?;
        let axis_value = match get("axis_value") {
            Some(Variant::Float(f)) => f as f32,
            Some(Variant::Int(i)) => i as f32,
            _ => 1.0,
        };
        Some(Self {
            class,
            code,
            axis_value,
            shift: as_bool(get("shift_pressed")),
            ctrl: as_bool(get("ctrl_pressed")) || as_bool(get("meta_pressed")),
            alt: as_bool(get("alt_pressed")),
        })
    }
}

/// Trait providing scene-tree operations to a running script.
///
/// The engine crate (`gdscene`) implements this trait on an accessor that wraps
/// a raw pointer to the `SceneTree`, breaking the circular borrow that would
/// otherwise prevent a script from calling back into the tree during execution.
pub trait SceneAccess {
    /// Resolve a node by path relative to `from`.
    fn get_node(&self, from: u64, path: &str) -> Option<u64>;
    /// Return the parent of `node`, if any.
    fn get_parent(&self, node: u64) -> Option<u64>;
    /// Return the children of `node`.
    fn get_children(&self, node: u64) -> Vec<u64>;
    /// Read a property from `node`.
    fn get_node_property(&self, node: u64, prop: &str) -> Variant;
    /// Write a property on `node`.
    fn set_node_property(&mut self, node: u64, prop: &str, value: Variant);
    /// Emit a signal on `node`.
    fn emit_signal(&mut self, node: u64, signal: &str, args: &[Variant]);
    /// Connect a signal on `source` to `method` on `target`.
    fn connect_signal(&mut self, source: u64, signal: &str, target: u64, method: &str);
    /// Connect a signal honoring Godot `ConnectFlags`
    /// (`DEFERRED=1`, `PERSIST=2`, `ONE_SHOT=4`, `REFERENCE_COUNTED=8`) plus any
    /// `Callable.bind` arguments to append. The default implementation drops the
    /// flags/binds and calls [`connect_signal`](Self::connect_signal); scene
    /// backends override it to record them (pat-nzgdu.31).
    fn connect_signal_flags(
        &mut self,
        source: u64,
        signal: &str,
        target: u64,
        method: &str,
        flags: i64,
        binds: &[Variant],
    ) {
        let _ = (flags, binds);
        self.connect_signal(source, signal, target, method);
    }
    /// Disconnect `target.method` from `source.signal`. Returns `true` if a
    /// connection was removed. Default: no-op returning `false`.
    fn disconnect_signal(&mut self, source: u64, signal: &str, target: u64, method: &str) -> bool {
        let _ = (source, signal, target, method);
        false
    }
    /// Whether `target.method` is connected to `source.signal`. Default `false`.
    fn is_signal_connected(&self, source: u64, signal: &str, target: u64, method: &str) -> bool {
        let _ = (source, signal, target, method);
        false
    }

    /// Every `(target_object, method)` currently connected to `source.signal`,
    /// backing Godot's `Signal.get_connections()` / `has_connections()`
    /// (pat-0q2kx.9). Default: none.
    fn signal_connections(&self, _source: u64, _signal: &str) -> Vec<(u64, String)> {
        Vec::new()
    }

    /// Whether `node` declares a signal called `name`, from any of the three
    /// places one can come from: the attached script's `signal` declarations,
    /// the node class's ClassDB signal set, or an existing entry in the node's
    /// `SignalStore`. Lets `$Other.my_signal` resolve to a `Signal` value
    /// rather than a Nil property read (pat-0q2kx.9). Default: `false`.
    fn node_has_signal(&self, _node: u64, _name: &str) -> bool {
        false
    }
    /// Return the name of `node`, if it exists.
    fn get_node_name(&self, node: u64) -> Option<String>;

    /// The text `node`'s script returns from `_to_string()`, if it overrides it
    /// (Godot `Object::to_string`, core/object/object.cpp) — what `str(obj)`
    /// and `print(obj)` must show instead of the engine's default
    /// `<Class#id>` form (pat-0q2kx.82).
    ///
    /// `None` means "no override, use the default", which is also what a
    /// backend that does not model scripts reports. Godot ignores an override
    /// that returns a non-String and falls back to the default, so implementors
    /// should return `None` in that case rather than stringifying the value.
    fn object_to_string(&mut self, _node: u64) -> Option<String> {
        None
    }

    /// The value `node`'s script returns from its `_get(property)` virtual —
    /// Godot's dynamic-property read fallback, consulted only after the real
    /// property lookup misses (core/object/object.cpp `Object::get`).
    ///
    /// `None` (or a `Nil` return from the script) means "not handled", which is
    /// how Godot distinguishes an unimplemented property from one whose value
    /// genuinely is null.
    fn object_get_virtual(&mut self, _node: u64, _property: &str) -> Option<Variant> {
        None
    }

    /// Offers `property = value` to `node`'s `_set(property, value)` virtual.
    /// Returns `true` when the script reports it consumed the write, which
    /// stops the engine from also storing the value (Godot `Object::set`).
    fn object_set_virtual(&mut self, _node: u64, _property: &str, _value: &Variant) -> bool {
        false
    }

    /// The extra property entries `node`'s script contributes through
    /// `_get_property_list()`, appended to the ones the engine knows about
    /// (Godot `Object::get_property_list`).
    fn object_property_list_virtual(&mut self, _node: u64) -> Vec<Variant> {
        Vec::new()
    }

    // -- Runtime node creation/deletion ------------------------------------

    /// Create a new node of the given class (not yet in the tree).
    /// Returns the raw ObjectId of the new node.
    fn create_node(&mut self, _class_name: &str, _name: &str) -> Option<u64> {
        None
    }

    /// Add `child_id` as a child of `parent_id` in the scene tree.
    fn add_child(&mut self, _parent_id: u64, _child_id: u64) -> bool {
        false
    }

    /// Mark a node for deferred deletion (removed at end of frame).
    fn queue_free(&mut self, _node_id: u64) {}

    /// Sends `method` to every peer via the tree's MultiplayerAPI (Godot
    /// `Node.rpc`), returning the engine's error text on failure.
    ///
    /// Defaults to an error rather than a silent no-op: a script calling `rpc()`
    /// with no multiplayer configured should hear about it, exactly as Godot
    /// reports an unregistered RPC (pat-0q2kx.79).
    fn node_rpc(&mut self, _node: u64, _method: &str, _args: &[Variant]) -> Result<(), String> {
        Err("no multiplayer available".to_string())
    }

    /// [`node_rpc`](Self::node_rpc) addressed to a single peer (Godot
    /// `Node.rpc_id`).
    fn node_rpc_id(
        &mut self,
        _node: u64,
        _peer: i64,
        _method: &str,
        _args: &[Variant],
    ) -> Result<(), String> {
        Err("no multiplayer available".to_string())
    }

    /// Return `true` if the object with the given raw id is a live node in the
    /// scene tree (Godot `is_instance_valid`). A freed / never-existed object
    /// returns `false`.
    ///
    /// The default returns `true` so mock accessors without a real object arena
    /// preserve the legacy "an ObjectId is always valid" behavior; the
    /// production `SceneTreeAccessor` overrides this to consult the live arena.
    fn is_object_alive(&self, _id: u64) -> bool {
        true
    }

    /// Resolve a raw object id back to a live object (Godot `instance_from_id`).
    /// Returns the id if an object with that id is currently alive, else `None`.
    fn instance_from_id(&self, _id: u64) -> Option<u64> {
        None
    }

    /// Immediately free the object (Godot `Object.free()`): remove it and its
    /// descendants from the tree *now*, unlike [`queue_free`](Self::queue_free)
    /// which defers removal to end-of-frame.
    fn free_object(&mut self, _id: u64) {}

    /// Queue a method call on `node_id` for end-of-frame (Godot `call_deferred`).
    fn call_deferred(&mut self, _node_id: u64, _method: &str, _args: &[Variant]) {}

    /// Create a SceneTreeTimer (Godot `SceneTree.create_timer`).
    ///
    /// Returns `(timer_id, signal_name)` used with `await` / `notify_await_signal`.
    fn create_timer(&mut self, _seconds: f64) -> Option<(u64, String)> {
        None
    }

    /// Advance scene timers by `delta` and return fired signal names.
    fn process_timers(&mut self, _delta: f64) -> Vec<String> {
        Vec::new()
    }

    /// Return the class name of a node.
    fn get_class(&self, _node: u64) -> Option<String> {
        None
    }

    // -- cross-object script calls (pat-0q2kx.8) ----------------------------
    //
    // Godot's `Object::call` dispatches into the *target's* script
    // (upstream/godot/core/object/object.cpp `Object::callv`), so
    // `$Enemy.take_damage(5)`, `obj.call("take_damage", 5)` and
    // `Callable(obj, "take_damage").call(5)` all run on `obj`. These hooks let
    // the interpreter ask the host to run a method on another object and hand
    // back its return value.

    /// Call `method` on the script attached to `node`, synchronously, returning
    /// its value. `None` means the node has no script, the script does not define
    /// `method`, or the call raised — the caller then reports "no such method".
    fn call_script_method(
        &mut self,
        _node: u64,
        _method: &str,
        _args: &[Variant],
    ) -> Option<Variant> {
        None
    }

    /// Whether `node` responds to `method` (script methods and native/class
    /// methods alike), for `Object.has_method`.
    fn node_has_method(&self, _node: u64, _method: &str) -> bool {
        false
    }

    /// Declared parameter count of `node`'s `method`, for
    /// `Object.get_method_argument_count`. `None` when the method is unknown.
    fn node_method_argument_count(&self, _node: u64, _method: &str) -> Option<i64> {
        None
    }

    /// Run a **native** (engine-implemented) method on `node` — the second half
    /// of Godot's `Object::callp`, which tries the script instance first and then
    /// the ClassDB method bind (upstream/godot/core/object/object.cpp).
    ///
    /// `None` means "not a native method I implement", so the caller falls
    /// through to its own error. This is deliberately **one** dispatch hook
    /// rather than a trait method per engine call: `SceneAccess` has several
    /// implementors (mostly test mocks), and every added method needs a default
    /// body or they all break. Keeping the semantics host-side also puts them
    /// where `SceneTree` is actually reachable (pat-0q2kx.16).
    fn call_node_method(
        &mut self,
        _node: u64,
        _method: &str,
        _args: &[Variant],
    ) -> Option<Variant> {
        None
    }

    // -- AnimationPlayer playback (pat-0q2kx.1) -----------------------------
    //
    // `AnimationPlayer.play("walk")` names an animation clip, whereas
    // `AudioStreamPlayer.play(0.5)` names a seek position — and both arrive at
    // the interpreter as `ObjectId.play`. These hooks let the interpreter ask
    // the host whether the node actually has an animation player before it
    // falls back to the audio semantics. Each returns whether the node had one,
    // so a `false` means "not an animation player, handle it some other way".

    /// Play `name` on the AnimationPlayer attached to `node` (an empty `name`
    /// resumes the current clip). Returns `false` if `node` has no player.
    fn animation_player_play(&mut self, _node: u64, _name: &str) -> bool {
        false
    }

    /// Stop the AnimationPlayer attached to `node`. Returns `false` if absent.
    fn animation_player_stop(&mut self, _node: u64) -> bool {
        false
    }

    /// Seek the AnimationPlayer attached to `node` to `time` seconds. Returns
    /// `false` if absent.
    fn animation_player_seek(&mut self, _node: u64, _time: f64) -> bool {
        false
    }

    /// Pause the AnimationPlayer attached to `node`, preserving its playhead
    /// (Godot `pause()`, unlike `stop()` which rewinds). Returns `false` if
    /// `node` has no player (pat-0q2kx.52).
    fn animation_player_pause(&mut self, _node: u64) -> bool {
        false
    }

    /// Queue `name` to play after the current clip finishes (Godot `queue()`).
    /// Returns `false` if `node` has no player.
    fn animation_player_queue(&mut self, _node: u64, _name: &str) -> bool {
        false
    }

    /// Play `name` backwards from its end (Godot `play_backwards`). Returns
    /// `false` if `node` has no player.
    fn animation_player_play_backwards(&mut self, _node: u64, _name: &str) -> bool {
        false
    }

    /// The effective playback speed (`speed_scale * custom_speed`, `0.0` when
    /// stopped), or `None` when `node` has no player.
    fn animation_player_playing_speed(&self, _node: u64) -> Option<f64> {
        None
    }

    // -- Object introspection over the signal graph (pat-0q2kx.64) ----------
    //
    // Godot's `get_signal_connection_list` / `get_incoming_connections` /
    // `has_connections` let a script (or a plugin, or a save system) walk the
    // live connection graph. The interpreter cannot see the tree's signal
    // stores, so the host answers.

    /// Outgoing connections on `node`'s `signal_name`, as
    /// `(target_object_id, method, one_shot)`.
    fn signal_connection_list(&self, _node: u64, _signal_name: &str) -> Vec<(u64, String, bool)> {
        Vec::new()
    }

    /// Connections anywhere in the tree whose TARGET is `node`, as
    /// `(source_object_id, signal_name, method)` — Godot's
    /// `get_incoming_connections`.
    fn incoming_connections(&self, _node: u64) -> Vec<(u64, String, String)> {
        Vec::new()
    }

    /// Whether `node` has any outgoing connection on any of its signals.
    fn has_any_connections(&self, _node: u64) -> bool {
        false
    }

    /// Emits `property_list_changed` on `node` (Godot
    /// `notify_property_list_changed`). Returns `false` if `node` is unknown.
    fn notify_property_list_changed(&mut self, _node: u64) -> bool {
        false
    }

    /// Whether the AnimationPlayer attached to `node` is playing, or `None`
    /// when `node` has no attached player.
    fn animation_player_is_playing(&self, _node: u64) -> Option<bool> {
        None
    }

    /// The clip the AnimationPlayer attached to `node` is playing (empty when
    /// stopped), or `None` when `node` has no attached player.
    fn animation_player_current(&self, _node: u64) -> Option<String> {
        None
    }

    /// The playback position of the AnimationPlayer attached to `node`, or
    /// `None` when `node` has no attached player.
    fn animation_player_position(&self, _node: u64) -> Option<f64> {
        None
    }

    // -- Tween (pat-0q2kx.4) -----------------------------------------------
    //
    // Godot: `node.create_tween()` hands back a `Tween`, and the tweeners chain
    // fluently off it (`tween_property(...).set_ease(...)`). The whole surface
    // routes through these two hooks rather than one trait method per Tween
    // method, so the host owns the tween semantics and this trait stays small.

    /// Create a tween owned by `node` and return its raw id, usable as a
    /// `Variant::ObjectId` handle (Godot `Node.create_tween`). The tween starts
    /// running immediately, as in Godot. `None` when the host has no tween
    /// store.
    fn create_tween(&mut self, _node: u64) -> Option<u64> {
        None
    }

    /// Dispatch a `Tween`/`Tweener` method against the tween handle `tween`.
    ///
    /// Returns `None` when `tween` is **not** a live tween, so the caller falls
    /// through to ordinary node semantics — necessary because a tween handle
    /// and a node id are both `Variant::ObjectId`, and `Tween` shares the
    /// method names `play` / `stop` / `pause` / `kill` / `set_loops` with
    /// `AudioStreamPlayer` and `AnimationPlayer`.
    ///
    /// Chainable setters return the tween handle itself so GDScript's fluent
    /// `create_tween().tween_property(...).set_trans(...)` works.
    fn tween_call(&mut self, _tween: u64, _method: &str, _args: &[Variant]) -> Option<Variant> {
        None
    }

    // -- SceneTree control surface (pat-0q2kx.47) --------------------------
    //
    // How a Godot script drives scene flow: `get_tree().change_scene_to_file()`,
    // `.quit()`, `.set_pause()`, and the group broadcasts. Every one of these is
    // already implemented and unit-tested engine-side, but nothing on this trait
    // exposed them, so the interpreter's `get_tree()` path fell through to
    // `UndefinedFunction` and a script could not change scene, quit, pause or
    // broadcast at all. Defaults keep non-tree implementors (test mocks)
    // compiling; each answers as "no tree to control".

    /// Queues Godot's `SceneTree.change_scene_to_file`. `false` when the path
    /// cannot be loaded or parsed.
    ///
    /// The swap is deferred to the next frame boundary, as in Godot, so the
    /// calling frame finishes in the old scene and the caller is not freed
    /// underneath itself mid-callback.
    fn change_scene_to_file(&mut self, _path: &str) -> bool {
        false
    }

    /// `SceneTree.reload_current_scene`. `false` when there is no current scene
    /// or it cannot be re-instantiated.
    fn reload_current_scene(&mut self) -> bool {
        false
    }

    /// `SceneTree.quit(exit_code)` — records the quit request. It does not stop
    /// execution itself; the host loop polls it at the frame boundary, so the
    /// calling frame still completes.
    fn quit(&mut self, _exit_code: i64) {}

    /// `SceneTree.set_pause` — the `paused` property's setter.
    fn set_tree_paused(&mut self, _paused: bool) {}

    /// `SceneTree.is_paused`.
    fn is_tree_paused(&self) -> bool {
        false
    }

    /// `SceneTree.get_root` — the `/root` node as an object handle.
    fn tree_root(&self) -> Option<u64> {
        None
    }

    /// `SceneTree.get_node_count`.
    fn tree_node_count(&self) -> i64 {
        0
    }

    /// `SceneTree.get_nodes_in_group`, in tree order.
    fn nodes_in_group(&self, _group: &str) -> Vec<u64> {
        Vec::new()
    }

    /// `SceneTree.get_first_node_in_group`.
    fn first_node_in_group(&self, _group: &str) -> Option<u64> {
        None
    }

    /// `SceneTree.has_group` — whether any node belongs to `group`.
    fn tree_has_group(&self, _group: &str) -> bool {
        false
    }

    /// `SceneTree.call_group_flags`. `flags` of 0 is plain `call_group`;
    /// GROUP_CALL_REVERSE/GROUP_CALL_DEFERRED are honored by the host.
    fn call_group_flags(&mut self, _flags: i64, _group: &str, _method: &str, _args: &[Variant]) {}

    /// `SceneTree.set_group` — set `property` on every member of `group`.
    fn set_group(&mut self, _group: &str, _property: &str, _value: Variant) {}

    /// `SceneTree.notify_group` — deliver notification code `what` to every
    /// member of `group`.
    fn notify_group(&mut self, _group: &str, _what: i64) {}

    // -- Object metadata (pat-nzgdu.30) ------------------------------------
    //
    // Godot's `Object` exposes a meta namespace distinct from properties,
    // used by the editor, plugins, and generic tooling. These route through
    // the Node's meta bag. Defaults are no-ops so non-tree implementors
    // (test mocks) need not override them.

    /// Set a meta entry on `node` (Godot `Object.set_meta`).
    fn set_meta(&mut self, _node: u64, _name: &str, _value: Variant) {}

    /// Get a meta entry from `node`, or `default` if absent
    /// (Godot `Object.get_meta`).
    fn get_meta(&self, _node: u64, _name: &str, default: Variant) -> Variant {
        default
    }

    /// Returns `true` if `node` has a meta entry named `name`
    /// (Godot `Object.has_meta`).
    fn has_meta(&self, _node: u64, _name: &str) -> bool {
        false
    }

    /// Remove a meta entry from `node` (Godot `Object.remove_meta`).
    fn remove_meta(&mut self, _node: u64, _name: &str) {}

    /// Return the sorted list of meta entry names on `node`
    /// (Godot `Object.get_meta_list`).
    fn get_meta_list(&self, _node: u64) -> Vec<String> {
        Vec::new()
    }

    /// Instantiate a packed scene (`.tscn`/`.scn`) referenced by `path`,
    /// building its node subtree and returning the root node's raw ObjectId.
    ///
    /// The returned subtree lives in the tree but is not yet parented to the
    /// caller — the script is expected to follow up with `add_child`, mirroring
    /// Godot's `preload("x.tscn").instantiate()` spawn pattern. Returns `None`
    /// if the scene cannot be loaded (Godot: `load` yields `null`).
    fn instantiate_packed_scene(&mut self, _path: &str) -> Option<u64> {
        None
    }

    /// Instantiate a GDScript class loaded from `path` (a `.gd` file), building
    /// a scripted instance and returning its root node's raw ObjectId.
    ///
    /// Mirrors [`instantiate_packed_scene`] for scenes: the host reads and
    /// parses the script, creates a node of the script's base class with the
    /// script attached, and returns its id (the script follows up with
    /// `add_child` if it wants the instance parented). Powers GDScript's
    /// `load("x.gd").new()`. Returns `None` if the script cannot be loaded
    /// (Godot: `new()` on a bad script yields `null`).
    fn instantiate_script(&mut self, _path: &str) -> Option<u64> {
        None
    }

    // -- Per-node process toggles (pat-nzgdu.137) ---------------------------

    /// Enables/disables `_process` on `node` (Godot's `Node.set_process`).
    ///
    /// Defaults to a no-op for hosts without a scene tree.
    fn set_process(&mut self, _node: u64, _enabled: bool) {}

    /// Whether `_process` runs on `node` (`Node.is_processing`).
    fn is_processing(&self, _node: u64) -> bool {
        false
    }

    /// Enables/disables `_physics_process` on `node`
    /// (`Node.set_physics_process`).
    fn set_physics_process(&mut self, _node: u64, _enabled: bool) {}

    /// Whether `_physics_process` runs on `node`
    /// (`Node.is_physics_processing`).
    fn is_physics_processing(&self, _node: u64) -> bool {
        false
    }

    /// Re-arms `_ready` on `node` so it runs again on the next tree-enter
    /// (`Node.request_ready`).
    fn request_ready(&mut self, _node: u64) {}

    // -- Input methods (used by the `Input` singleton in GDScript) ----------

    /// Marks the input event currently being dispatched as handled, stopping
    /// it from reaching any later propagation stage (Godot:
    /// `Viewport.set_input_as_handled()`).
    ///
    /// Defaults to a no-op for hosts that do not dispatch input events.
    fn set_input_as_handled(&mut self) {}

    /// Whether the in-flight input event has already been marked handled
    /// (Godot: `Viewport.is_input_handled()`).
    ///
    /// Lets a later stage check whether an earlier one consumed the event
    /// instead of only being able to set the flag. Defaults to `false` for
    /// hosts that do not dispatch input events (pat-0q2kx.91).
    fn is_input_handled(&self) -> bool {
        false
    }

    /// Returns `true` if any key mapped to `action` is currently held.
    fn is_input_action_pressed(&self, _action: &str) -> bool {
        false
    }

    /// Returns `true` if any key mapped to `action` was just pressed this frame.
    fn is_input_action_just_pressed(&self, _action: &str) -> bool {
        false
    }

    /// Returns `true` if the raw key `key` is currently held.
    /// Whether the key with Godot keycode `keycode` (`@GlobalScope.KEY_*`) is
    /// held — `Input.is_key_pressed(KEY_A)` (pat-0q2kx.92).
    ///
    /// Separate from [`is_input_key_pressed`](Self::is_input_key_pressed)
    /// because the host owns the keycode↔key mapping; this crate has no
    /// `gdplatform` dependency to do the lookup itself.
    fn is_input_keycode_pressed(&self, _keycode: i64) -> bool {
        false
    }

    /// Whether any key, mouse button or action is currently held
    /// (`Input.is_anything_pressed`).
    fn is_anything_pressed(&self) -> bool {
        false
    }

    /// The last mouse motion's velocity in pixels/second, as `(x, y)`
    /// (`Input.get_last_mouse_velocity`).
    fn get_last_mouse_velocity(&self) -> (f32, f32) {
        (0.0, 0.0)
    }

    fn is_input_key_pressed(&self, _key: &str) -> bool {
        false
    }

    /// Returns the global mouse position as (x, y).
    fn get_global_mouse_position(&self) -> (f32, f32) {
        (0.0, 0.0)
    }

    /// Returns `true` if the given mouse button index is pressed.
    fn is_mouse_button_pressed(&self, _button_index: i64) -> bool {
        false
    }

    /// Returns a direction Vector2 from four input actions.
    fn get_input_vector(&self, neg_x: &str, pos_x: &str, neg_y: &str, pos_y: &str) -> (f32, f32) {
        let mut x: f32 = 0.0;
        let mut y: f32 = 0.0;
        if self.is_input_action_pressed(neg_x) {
            x -= 1.0;
        }
        if self.is_input_action_pressed(pos_x) {
            x += 1.0;
        }
        if self.is_input_action_pressed(neg_y) {
            y -= 1.0;
        }
        if self.is_input_action_pressed(pos_y) {
            y += 1.0;
        }
        // Normalize if diagonal
        let len = (x * x + y * y).sqrt();
        if len > 0.0 {
            (x / len, y / len)
        } else {
            (0.0, 0.0)
        }
    }

    /// Analog strength of `action` in `[0, 1]` (pat-nzgdu.9).
    ///
    /// The default treats an action as digital: `1.0` while any mapped input is
    /// pressed, else `0.0`. The runtime accessor overrides this with real
    /// analog/deadzone-scaled magnitudes for gamepad axes.
    fn get_action_strength(&self, action: &str) -> f32 {
        if self.is_input_action_pressed(action) {
            1.0
        } else {
            0.0
        }
    }

    /// Godot `Input.get_axis(neg, pos)` == `strength(pos) - strength(neg)`
    /// (pat-nzgdu.9). Derives from [`Self::get_action_strength`], so digital
    /// actions yield `-1.0` / `0.0` / `1.0`.
    fn get_input_axis(&self, neg: &str, pos: &str) -> f32 {
        self.get_action_strength(pos) - self.get_action_strength(neg)
    }

    /// Returns `true` if any input mapped to `action` was released this frame
    /// (mirror of [`Self::is_input_action_just_pressed`]). The default is
    /// `false`; the runtime accessor overrides it from the frame's
    /// just-released set. (pat-nzgdu.9)
    fn is_input_action_just_released(&self, _action: &str) -> bool {
        false
    }

    // -- Mouse mode / programmatic actions / joypad (pat-u3l8c.22) ----------

    /// Sets the mouse capture/visibility mode (`Input.set_mouse_mode`).
    /// `mode` uses Godot's `MOUSE_MODE_*` integer values.
    fn set_mouse_mode(&mut self, _mode: i64) {}

    /// Returns the current mouse mode as a Godot `MOUSE_MODE_*` integer
    /// (`Input.get_mouse_mode`); defaults to `MOUSE_MODE_VISIBLE` (0).
    fn get_mouse_mode(&self) -> i64 {
        0
    }

    // -- cursor shape (pat-0q2kx.90) ----------------------------------------

    /// Sets the default cursor shape (`Input.set_default_cursor_shape`).
    /// `shape` uses Godot's `CURSOR_*` integer values.
    fn set_default_cursor_shape(&mut self, _shape: i64) {}

    /// The active cursor shape as a Godot `CURSOR_*` integer
    /// (`Input.get_current_cursor_shape`); defaults to `CURSOR_ARROW` (0).
    fn get_current_cursor_shape(&self) -> i64 {
        0
    }

    /// Overrides the image drawn for `shape` (`Input.set_custom_mouse_cursor`).
    /// An empty `image_path` clears the override, as passing `null` does in
    /// Godot.
    fn set_custom_mouse_cursor(
        &mut self,
        _image_path: &str,
        _shape: i64,
        _hotspot_x: f32,
        _hotspot_y: f32,
    ) {
    }

    // -- SceneMultiplayer surface (pat-0q2kx.94) ----------------------------
    //
    // Reached from a script as `multiplayer.*`. The defaults are Godot's
    // no-peer answers, not errors: asking before a peer exists is ordinary.

    /// `multiplayer.is_server()`.
    fn multiplayer_is_server(&self) -> bool {
        false
    }

    /// `multiplayer.get_unique_id()`; `0` when there is no peer.
    fn multiplayer_unique_id(&self) -> i64 {
        0
    }

    /// `multiplayer.has_multiplayer_peer()`.
    fn multiplayer_has_peer(&self) -> bool {
        false
    }

    /// `multiplayer.get_peers()`.
    fn multiplayer_peers(&self) -> Vec<i64> {
        Vec::new()
    }

    /// `multiplayer.get_remote_sender_id()` — valid inside an RPC handler.
    fn multiplayer_remote_sender_id(&self) -> i64 {
        0
    }

    /// `multiplayer.get_authenticating_peers()`.
    fn multiplayer_authenticating_peers(&self) -> Vec<i64> {
        Vec::new()
    }

    /// `multiplayer.send_bytes(bytes, id, mode, channel)`; `true` when sent.
    fn multiplayer_send_bytes(&mut self, _bytes: &[u8], _target: i64, _channel: i64) -> bool {
        false
    }

    /// `multiplayer.complete_auth(id)`; `true` when the peer was awaiting auth.
    fn multiplayer_complete_auth(&mut self, _id: i64, _accepted: bool) -> bool {
        false
    }

    /// `multiplayer.refuse_new_connections = value`.
    fn multiplayer_set_refuse_new_connections(&mut self, _refuse: bool) {}

    /// Reads back `multiplayer.refuse_new_connections`.
    fn multiplayer_refuses_new_connections(&self) -> bool {
        false
    }

    /// Warps the mouse cursor to `(x, y)` in window coordinates
    /// (`Input.warp_mouse`).
    fn warp_mouse(&mut self, _x: f32, _y: f32) {}

    /// Programmatically presses `action` with the given strength
    /// (`Input.action_press`).
    fn action_press(&mut self, _action: &str, _strength: f32) {}

    /// Programmatically releases `action` (`Input.action_release`).
    fn action_release(&mut self, _action: &str) {}

    // -- InputMap singleton (pat-0q2kx.89) ---------------------------------
    //
    // Godot's `InputMap` is a script-mutable singleton, so a game can add
    // actions and rebind keys at runtime. The map itself lives in the host (the
    // frame loop owns it, like `InputState`), so these are hooks rather than
    // logic. Mutations must be visible to the *next* `Input.is_action_pressed`
    // query, which is what makes a runtime rebind actually take effect.

    /// Whether the map has `action` (`InputMap.has_action`).
    fn input_map_has_action(&self, _action: &str) -> bool {
        false
    }

    /// Every action name, sorted (`InputMap.get_actions`).
    fn input_map_get_actions(&self) -> Vec<String> {
        Vec::new()
    }

    /// Adds an action with `deadzone` (`InputMap.add_action`).
    fn input_map_add_action(&mut self, _action: &str, _deadzone: f32) {}

    /// Removes an action and its events (`InputMap.erase_action`).
    fn input_map_erase_action(&mut self, _action: &str) {}

    /// Sets an existing action's deadzone (`InputMap.action_set_deadzone`).
    fn input_map_action_set_deadzone(&mut self, _action: &str, _deadzone: f32) {}

    /// Binds `event` to `action` (`InputMap.action_add_event`).
    fn input_map_action_add_event(&mut self, _action: &str, _event: InputEventSpec) {}

    /// Unbinds `event` from `action` (`InputMap.action_erase_event`).
    fn input_map_action_erase_event(&mut self, _action: &str, _event: InputEventSpec) {}

    /// Unbinds every event from `action` (`InputMap.action_erase_events`).
    fn input_map_action_erase_events(&mut self, _action: &str) {}

    /// How many events are bound to `action`. Backs `InputMap.action_get_events`,
    /// which cannot return real `InputEvent` objects until Patina has that class
    /// family; the count is what a script can meaningfully act on today.
    fn input_map_action_event_count(&self, _action: &str) -> usize {
        0
    }

    /// Returns the device ids of connected joypads
    /// (`Input.get_connected_joypads`).
    fn get_connected_joypads(&self) -> Vec<i64> {
        Vec::new()
    }

    /// Whether the Godot `JOY_BUTTON_*` `button` is held on joypad `device`
    /// (`Input.is_joy_button_pressed`, pat-0q2kx.88).
    fn is_joy_button_pressed(&self, _device: i64, _button: i64) -> bool {
        false
    }

    /// The value of the Godot `JOY_AXIS_*` `axis` on joypad `device`, `0.0`
    /// when untouched (`Input.get_joy_axis`, pat-0q2kx.88).
    fn get_joy_axis(&self, _device: i64, _axis: i64) -> f32 {
        0.0
    }

    /// Starts a vibration on joypad `device` with `weak`/`strong` motor
    /// magnitudes for `duration` seconds (`Input.start_joy_vibration`).
    fn start_joy_vibration(&mut self, _device: i64, _weak: f32, _strong: f32, _duration: f32) {}

    /// Stops any vibration on joypad `device` (`Input.stop_joy_vibration`).
    fn stop_joy_vibration(&mut self, _device: i64) {}
}
