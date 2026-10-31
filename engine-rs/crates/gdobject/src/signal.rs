//! Signal declaration, connection, and emission.
//!
//! Godot's signal system provides a one-to-many observer pattern: an object
//! declares named signals, other objects connect to them, and when a signal
//! is emitted the connected callbacks fire in registration order.
//!
//! This module implements the core signal mechanics. Because we don't yet
//! have a global object registry (that belongs to the scene layer), callbacks
//! are modelled as `Box<dyn Fn>` closures for now. The `Connection` type also
//! records the target ObjectId + method name for serialization and debugging.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use gdcore::id::ObjectId;
use gdvariant::Variant;

/// A boxed signal callback function.
type SignalCallback = Arc<dyn Fn(&[Variant]) -> Variant + Send + Sync>;

/// A deferred signal callback, ready to be invoked later.
#[derive(Clone)]
pub struct DeferredCall {
    connection: Connection,
    args: Vec<Variant>,
}

impl DeferredCall {
    /// Execute the deferred call.
    pub fn call(&self) -> Variant {
        self.connection.call(&self.args)
    }

    /// Returns the resolved arguments that will be passed when the call is dispatched.
    pub fn args(&self) -> &[Variant] {
        &self.args
    }
}

/// A connection between a signal and a target.
///
/// Each connection stores enough data to identify the target (ObjectId +
/// method name) and an optional callback closure for direct dispatch.
pub struct Connection {
    /// The target object's instance ID.
    pub target_id: ObjectId,
    /// The method to call on the target (Godot uses StringName).
    pub method: String,
    /// Optional direct callback. When present, `emit` invokes this instead
    /// of requiring a method-resolution lookup.
    callback: Option<SignalCallback>,
    /// When `true`, the connection auto-disconnects after the first emission
    /// (Godot's `CONNECT_ONE_SHOT` flag).
    pub one_shot: bool,
    /// Extra arguments appended after signal-emitted arguments (Godot's `binds`).
    pub binds: Vec<Variant>,
    /// Number of trailing signal arguments to drop (Godot's `unbinds`).
    pub unbinds: usize,
    /// When `true`, the connection fires in the deferred queue rather than
    /// immediately (Godot's `CONNECT_DEFERRED` flag).
    pub deferred: bool,
    /// The Godot `ConnectFlags` bitmask this connection was made with
    /// (pat-mgixk.98).
    ///
    /// Kept WHOLE as well as decoded into `one_shot`/`deferred` above, because
    /// `Object.get_signal_connection_list` has to report the original bitmask
    /// back to a script — it cannot be reconstructed from two booleans without
    /// losing `PERSIST`, `REFERENCE_COUNTED` and `APPEND_SOURCE_OBJECT`.
    pub flags: u32,
    /// `CONNECT_REFERENCE_COUNTED` reference count; 1 for an ordinary
    /// connection (pat-mgixk.98).
    ///
    /// Godot (`core/object/object.cpp:1646-1666`) refuses a duplicate connect
    /// outright *unless* the caller passes `CONNECT_REFERENCE_COUNTED`, in
    /// which case it increments this instead of adding a second connection —
    /// and `disconnect` only removes the connection once the count falls back
    /// to zero.
    pub ref_count: u32,
}

/// Godot's `Object::ConnectFlags`, from `core/object/object.h:574-578`.
///
/// ⚠️ `gdscene::packed_scene` carries its own `pub const CONNECT_DEFERRED` and
/// `CONNECT_ONE_SHOT` (packed_scene.rs:3868-3871), which predate these. The
/// values agree, so there is no behavioural divergence — but this set is the
/// canonical one, living in the crate that owns `Connection`, and gdscene
/// cannot be the home for it because gdobject sits below gdscene. Consolidating
/// the two is a separate cleanup, deliberately not bundled here.
pub const CONNECT_DEFERRED: u32 = 1;
/// Godot `CONNECT_PERSIST` — the connection is saved with the scene.
pub const CONNECT_PERSIST: u32 = 2;
/// Godot `CONNECT_ONE_SHOT` — disconnects itself after firing once.
pub const CONNECT_ONE_SHOT: u32 = 4;
/// Godot `CONNECT_REFERENCE_COUNTED` — a duplicate connect bumps a refcount
/// instead of being refused.
pub const CONNECT_REFERENCE_COUNTED: u32 = 8;
/// Godot `CONNECT_APPEND_SOURCE_OBJECT` — the emitter is appended to the
/// handler's arguments.
pub const CONNECT_APPEND_SOURCE_OBJECT: u32 = 16;

/// Why [`Signal::try_connect`] refused a connection (pat-mgixk.98).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalConnectError {
    /// This `(target, method)` pair is already connected and the call did not
    /// pass `CONNECT_REFERENCE_COUNTED`. Godot returns `ERR_INVALID_PARAMETER`.
    AlreadyConnected,
}

impl Connection {
    /// Creates a connection with just target ID and method name.
    pub fn new(target_id: ObjectId, method: impl Into<String>) -> Self {
        Self {
            target_id,
            method: method.into(),
            callback: None,
            one_shot: false,
            binds: Vec::new(),
            unbinds: 0,
            deferred: false,
            flags: 0,
            // 1, not 0: an ordinary connection is held once, so the
            // refcount-aware `disconnect` removes it on the first call.
            ref_count: 1,
        }
    }

    /// Creates a connection with a direct callback closure.
    pub fn with_callback(
        target_id: ObjectId,
        method: impl Into<String>,
        callback: impl Fn(&[Variant]) -> Variant + Send + Sync + 'static,
    ) -> Self {
        Self {
            target_id,
            method: method.into(),
            callback: Some(Arc::new(callback)),
            one_shot: false,
            binds: Vec::new(),
            unbinds: 0,
            deferred: false,
            flags: 0,
            ref_count: 1,
        }
    }

    /// Returns a copy of this connection with the one-shot flag set.
    pub fn as_one_shot(mut self) -> Self {
        self.one_shot = true;
        self
    }

    /// Returns a copy of this connection with the deferred flag set.
    pub fn as_deferred(mut self) -> Self {
        self.deferred = true;
        self
    }

    /// Returns a copy carrying Godot `ConnectFlags`, decoding the two the
    /// dispatcher reads directly (pat-mgixk.98).
    ///
    /// ⚠️ Decodes `one_shot`/`deferred` as well as storing the mask, so a
    /// caller cannot set flags and silently leave the dispatcher's own booleans
    /// disagreeing with them.
    pub fn with_flags(mut self, flags: u32) -> Self {
        self.flags = flags;
        self.deferred = flags & CONNECT_DEFERRED != 0;
        self.one_shot = flags & CONNECT_ONE_SHOT != 0;
        self
    }

    /// Returns a copy with extra bound arguments appended after signal args.
    pub fn with_binds(mut self, binds: Vec<Variant>) -> Self {
        self.binds = binds;
        self
    }

    /// Returns a copy that drops `n` trailing signal arguments.
    pub fn with_unbinds(mut self, n: usize) -> Self {
        self.unbinds = n;
        self
    }

    /// Resolve the final argument list: drop `unbinds` trailing signal args,
    /// then append `binds`.
    pub fn resolve_args(&self, signal_args: &[Variant]) -> Vec<Variant> {
        let keep = signal_args.len().saturating_sub(self.unbinds);
        let mut resolved = signal_args[..keep].to_vec();
        resolved.extend(self.binds.iter().cloned());
        resolved
    }

    /// Invokes this connection's callback (if present) with the given arguments.
    ///
    /// Returns `Variant::Nil` if no callback is attached.
    pub fn call(&self, args: &[Variant]) -> Variant {
        match &self.callback {
            Some(cb) => cb(args),
            None => Variant::Nil,
        }
    }

    /// Returns `true` if this connection has a callable callback.
    pub fn has_callback(&self) -> bool {
        self.callback.is_some()
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("target_id", &self.target_id)
            .field("method", &self.method)
            .field("has_callback", &self.callback.is_some())
            .finish()
    }
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        Self {
            target_id: self.target_id,
            method: self.method.clone(),
            callback: self.callback.clone(),
            one_shot: self.one_shot,
            binds: self.binds.clone(),
            unbinds: self.unbinds,
            deferred: self.deferred,
            flags: self.flags,
            ref_count: self.ref_count,
        }
    }
}

/// A named signal with its list of connections.
///
/// Signals maintain insertion order — connections fire in the order they
/// were added, matching Godot's behavior.
#[derive(Debug, Clone)]
pub struct Signal {
    /// The signal's name (e.g., `"pressed"`, `"body_entered"`).
    name: String,
    /// Ordered list of connections.
    connections: Vec<Connection>,
}

impl Signal {
    /// Creates a new signal with the given name and no connections.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            connections: Vec::new(),
        }
    }

    /// Returns the signal name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Adds a connection, applying Godot's duplicate rules.
    ///
    /// ⚠️ This used to push unconditionally, under a doc comment claiming
    /// "Duplicates (same target + method) are allowed, matching Godot's default
    /// behavior". That was backwards: `Object::connect`
    /// (`core/object/object.cpp:1646-1666`) ERRORS on an already-connected
    /// `(target, method)` pair and leaves the single existing connection in
    /// place. The old behaviour meant a re-entered `_ready` that connected
    /// twice fired its handler twice on every emit (pat-mgixk.98).
    ///
    /// The outcome is discarded here so the existing call sites — scene
    /// wiring, the editor, the deferred queue — keep their signature. The
    /// script bridge calls [`try_connect`](Self::try_connect) instead, because
    /// it has to hand an `Error` code back to GDScript.
    pub fn connect(&mut self, connection: Connection) {
        let _ = self.try_connect(connection);
    }

    /// As [`connect`](Self::connect), reporting what Godot would report.
    ///
    /// Returns `Err(SignalConnectError::AlreadyConnected)` for a duplicate
    /// `(target, method)` unless the incoming call carries
    /// `CONNECT_REFERENCE_COUNTED`, in which case the existing connection's
    /// `ref_count` is incremented and no second connection is added.
    pub fn try_connect(&mut self, connection: Connection) -> Result<(), SignalConnectError> {
        if let Some(existing) = self
            .connections
            .iter_mut()
            .find(|c| c.target_id == connection.target_id && c.method == connection.method)
        {
            // 🔑 The flag is read off the INCOMING call, not off the stored
            // connection: Godot decides per `connect()` whether *this*
            // duplicate is counted or refused, so a first connect without the
            // flag followed by one with it still increments.
            if connection.flags & CONNECT_REFERENCE_COUNTED != 0 {
                existing.ref_count = existing.ref_count.saturating_add(1);
                return Ok(());
            }
            return Err(SignalConnectError::AlreadyConnected);
        }
        self.connections.push(connection);
        Ok(())
    }

    /// Removes the first connection matching the given target and method.
    ///
    /// Returns `true` if the disconnect applied — which for a
    /// `CONNECT_REFERENCE_COUNTED` connection held more than once means its
    /// count was decremented rather than the connection removed
    /// (`core/object/object.cpp:1646-1666`). An ordinary connection has
    /// `ref_count == 1` and is removed on the first call, exactly as before.
    pub fn disconnect(&mut self, target_id: ObjectId, method: &str) -> bool {
        if let Some(pos) = self
            .connections
            .iter()
            .position(|c| c.target_id == target_id && c.method == method)
        {
            if self.connections[pos].ref_count > 1 {
                self.connections[pos].ref_count -= 1;
                return true;
            }
            self.connections.remove(pos);
            true
        } else {
            false
        }
    }

    /// Removes all connections targeting the given object.
    pub fn disconnect_all_for(&mut self, target_id: ObjectId) {
        self.connections.retain(|c| c.target_id != target_id);
    }

    /// Emits this signal, calling all connected callbacks in order.
    ///
    /// Returns a `Vec` of return values from each connection. Connections
    /// without a callback produce `Variant::Nil`.
    ///
    /// One-shot connections are automatically removed after firing.
    pub fn emit(&mut self, args: &[Variant]) -> Vec<Variant> {
        let results: Vec<Variant> = self.connections.iter().map(|c| c.call(args)).collect();
        self.connections.retain(|c| !c.one_shot);
        results
    }

    /// Emits this signal, separating immediate and deferred connections.
    ///
    /// Returns `(immediate_results, deferred_connections)`. Immediate
    /// connections fire now; deferred connections are returned for later
    /// dispatch by the caller.
    pub fn emit_collecting_deferred(
        &mut self,
        args: &[Variant],
    ) -> (Vec<Variant>, Vec<DeferredCall>) {
        let mut immediate = Vec::new();
        let mut deferred = Vec::new();

        for c in &self.connections {
            let resolved = c.resolve_args(args);
            if c.deferred {
                deferred.push(DeferredCall {
                    connection: c.clone(),
                    args: resolved,
                });
            } else {
                immediate.push(c.call(&resolved));
            }
        }

        self.connections.retain(|c| !c.one_shot);
        (immediate, deferred)
    }

    /// Returns the number of active connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Returns `true` if any connections exist.
    pub fn is_connected(&self) -> bool {
        !self.connections.is_empty()
    }

    /// Returns a slice of all connections (for inspection/testing).
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }
}

/// Per-object store of named signals.
///
/// Every `ObjectBase` embeds one of these. Signals are created lazily on
/// first connect or explicitly via `add_signal`.
#[derive(Debug, Clone, Default)]
pub struct SignalStore {
    signals: HashMap<String, Signal>,
}

impl SignalStore {
    /// Creates an empty signal store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a signal by name. If the signal already exists this is a no-op.
    pub fn add_signal(&mut self, name: impl Into<String>) {
        let name = name.into();
        self.signals
            .entry(name.clone())
            .or_insert_with(|| Signal::new(name));
    }

    /// Undeclares a signal, dropping it and every connection made to it.
    /// Returns `true` if a signal was removed (Godot `Object.remove_user_signal`).
    ///
    /// ⚠️ Removing the signal is the whole point: leaving the entry behind and
    /// merely forgetting that it was user-added would keep `has_signal` true and
    /// keep `emit_signal` delivering to its connections, which is the
    /// registered-but-dead shape this engine's ClassDB sweeps exist to remove.
    pub fn remove_signal(&mut self, name: &str) -> bool {
        self.signals.remove(name).is_some()
    }

    /// Connects a callback to the named signal, creating the signal if needed.
    pub fn connect(&mut self, signal_name: &str, connection: Connection) {
        let _ = self.try_connect(signal_name, connection);
    }

    /// As [`connect`](Self::connect), reporting Godot's duplicate outcome so a
    /// caller that has to answer a script can (pat-mgixk.98).
    pub fn try_connect(
        &mut self,
        signal_name: &str,
        connection: Connection,
    ) -> Result<(), SignalConnectError> {
        self.signals
            .entry(signal_name.to_owned())
            .or_insert_with(|| Signal::new(signal_name))
            .try_connect(connection)
    }

    /// Disconnects the first matching connection from a signal.
    ///
    /// Returns `true` if a connection was removed.
    pub fn disconnect(&mut self, signal_name: &str, target_id: ObjectId, method: &str) -> bool {
        self.signals
            .get_mut(signal_name)
            .is_some_and(|s| s.disconnect(target_id, method))
    }

    /// Emits a named signal. Returns the collected return values.
    ///
    /// If the signal does not exist, returns an empty vec (matching Godot's
    /// behavior of silently ignoring emission on undeclared signals).
    ///
    /// One-shot connections are automatically removed after firing.
    pub fn emit(&mut self, signal_name: &str, args: &[Variant]) -> Vec<Variant> {
        self.signals
            .get_mut(signal_name)
            .map_or_else(Vec::new, |s| s.emit(args))
    }

    /// Emits a named signal, separating immediate and deferred results.
    pub fn emit_collecting_deferred(
        &mut self,
        signal_name: &str,
        args: &[Variant],
    ) -> (Vec<Variant>, Vec<DeferredCall>) {
        self.signals.get_mut(signal_name).map_or_else(
            || (Vec::new(), Vec::new()),
            |s| s.emit_collecting_deferred(args),
        )
    }

    /// Returns `true` if the named signal exists.
    pub fn has_signal(&self, name: &str) -> bool {
        self.signals.contains_key(name)
    }

    /// Returns a reference to a signal by name.
    pub fn get_signal(&self, name: &str) -> Option<&Signal> {
        self.signals.get(name)
    }

    /// Returns the names of all declared signals.
    pub fn signal_names(&self) -> Vec<&str> {
        self.signals.keys().map(String::as_str).collect()
    }

    /// Removes all connections targeting the given object from every signal.
    pub fn disconnect_all_for(&mut self, target_id: ObjectId) {
        for signal in self.signals.values_mut() {
            signal.disconnect_all_for(target_id);
        }
    }
}

/// Trait providing signal operations on an object.
///
/// Types that embed a `SignalStore` (via `ObjectBase`) can implement this
/// to expose a uniform signal API.
pub trait SignalEmitter {
    /// Returns a reference to the underlying signal store.
    fn signal_store(&self) -> &SignalStore;

    /// Returns a mutable reference to the underlying signal store.
    fn signal_store_mut(&mut self) -> &mut SignalStore;

    /// Declares a signal by name.
    fn add_signal(&mut self, name: impl Into<String>) {
        self.signal_store_mut().add_signal(name);
    }

    /// Connects a callback to a signal.
    fn connect_signal(&mut self, signal_name: &str, connection: Connection) {
        self.signal_store_mut().connect(signal_name, connection);
    }

    /// Disconnects a callback from a signal.
    fn disconnect_signal(&mut self, signal_name: &str, target_id: ObjectId, method: &str) -> bool {
        self.signal_store_mut()
            .disconnect(signal_name, target_id, method)
    }

    /// Emits a signal with the given arguments.
    fn emit_signal(&mut self, signal_name: &str, args: &[Variant]) -> Vec<Variant> {
        self.signal_store_mut().emit(signal_name, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn signal_connect_and_emit() {
        let mut signal = Signal::new("pressed");
        assert_eq!(signal.connection_count(), 0);

        let target_id = ObjectId::next();
        let call_count = Arc::new(AtomicUsize::new(0));
        let counter = call_count.clone();

        signal.connect(Connection::with_callback(
            target_id,
            "on_pressed",
            move |_args| {
                counter.fetch_add(1, Ordering::SeqCst);
                Variant::Bool(true)
            },
        ));

        assert_eq!(signal.connection_count(), 1);
        assert!(signal.is_connected());

        let results = signal.emit(&[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], Variant::Bool(true));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn signal_emit_order() {
        let mut signal = Signal::new("tick");
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        for i in 0..3 {
            let order_clone = order.clone();
            let target_id = ObjectId::next();
            signal.connect(Connection::with_callback(
                target_id,
                &format!("handler_{i}"),
                move |_args| {
                    order_clone.lock().unwrap().push(i);
                    Variant::Int(i as i64)
                },
            ));
        }

        let results = signal.emit(&[Variant::String("hello".into())]);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], Variant::Int(0));
        assert_eq!(results[1], Variant::Int(1));
        assert_eq!(results[2], Variant::Int(2));

        let recorded = order.lock().unwrap();
        assert_eq!(*recorded, vec![0, 1, 2]);
    }

    #[test]
    fn signal_disconnect() {
        let mut signal = Signal::new("changed");
        let target_a = ObjectId::next();
        let target_b = ObjectId::next();

        signal.connect(Connection::with_callback(target_a, "on_changed", |_| {
            Variant::Int(1)
        }));
        signal.connect(Connection::with_callback(target_b, "on_changed", |_| {
            Variant::Int(2)
        }));

        assert_eq!(signal.connection_count(), 2);

        // Disconnect target_a.
        assert!(signal.disconnect(target_a, "on_changed"));
        assert_eq!(signal.connection_count(), 1);

        let results = signal.emit(&[]);
        assert_eq!(results, vec![Variant::Int(2)]);

        // Disconnecting a non-existent connection returns false.
        assert!(!signal.disconnect(target_a, "on_changed"));
    }

    #[test]
    fn signal_disconnect_all_for_target() {
        let mut signal = Signal::new("multi");
        let target = ObjectId::next();

        signal.connect(Connection::new(target, "method_a"));
        signal.connect(Connection::new(target, "method_b"));
        signal.connect(Connection::new(ObjectId::next(), "method_c"));

        assert_eq!(signal.connection_count(), 3);
        signal.disconnect_all_for(target);
        assert_eq!(signal.connection_count(), 1);
    }

    #[test]
    fn signal_store_basic() {
        let mut store = SignalStore::new();

        store.add_signal("ready");
        assert!(store.has_signal("ready"));
        assert!(!store.has_signal("process"));

        let target = ObjectId::next();
        let call_count = Arc::new(AtomicUsize::new(0));
        let counter = call_count.clone();

        store.connect(
            "ready",
            Connection::with_callback(target, "on_ready", move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );

        let results = store.emit("ready", &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Emit on non-existent signal is silent.
        let results = store.emit("nonexistent", &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn signal_store_auto_creates_on_connect() {
        let mut store = SignalStore::new();
        assert!(!store.has_signal("new_signal"));

        store.connect("new_signal", Connection::new(ObjectId::next(), "handler"));

        assert!(store.has_signal("new_signal"));
    }

    #[test]
    fn connection_without_callback() {
        let conn = Connection::new(ObjectId::next(), "some_method");
        assert!(!conn.has_callback());
        assert_eq!(conn.call(&[]), Variant::Nil);
    }

    #[test]
    fn emit_with_no_connections_returns_empty() {
        let mut signal = Signal::new("empty_signal");
        let results = signal.emit(&[Variant::Int(42)]);
        assert!(results.is_empty());
    }

    #[test]
    fn emit_nonexistent_signal_on_store_returns_empty() {
        let mut store = SignalStore::new();
        let results = store.emit("nonexistent", &[Variant::Bool(true)]);
        assert!(results.is_empty());
    }

    /// ⚠️ This test previously asserted the OPPOSITE — it was called
    /// `connect_same_callback_twice_fires_twice` and pinned
    /// `connection_count() == 2`. That encoded a parity bug, not a
    /// requirement: `Object::connect` (`core/object/object.cpp:1646-1666`)
    /// refuses an already-connected `(target, method)` pair. Rewritten with the
    /// bead's ground truth rather than deleted, so the old claim stays visible
    /// (pat-mgixk.98).
    #[test]
    fn connect_same_callback_twice_is_refused() {
        let mut signal = Signal::new("doubled");
        let target_id = ObjectId::next();

        let call_count = Arc::new(AtomicUsize::new(0));
        let mut outcomes = Vec::new();
        for _ in 0..2 {
            let counter = call_count.clone();
            outcomes.push(signal.try_connect(Connection::with_callback(
                target_id,
                "on_doubled",
                move |_| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Variant::Nil
                },
            )));
        }

        assert_eq!(outcomes[0], Ok(()));
        assert_eq!(
            outcomes[1],
            Err(SignalConnectError::AlreadyConnected),
            "the second connect must be refused, as Godot's is"
        );
        assert_eq!(signal.connection_count(), 1);
        let results = signal.emit(&[]);
        assert_eq!(results.len(), 1, "one connection, so one dispatch");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    /// The `CONNECT_REFERENCE_COUNTED` escape hatch: a duplicate is counted
    /// rather than refused, still as ONE connection, and survives exactly as
    /// many disconnects as it had connects.
    #[test]
    fn reference_counted_duplicate_connect_counts_instead_of_refusing() {
        let mut signal = Signal::new("counted");
        let target_id = ObjectId::next();
        let conn =
            || Connection::new(target_id, "on_counted").with_flags(CONNECT_REFERENCE_COUNTED);

        assert_eq!(signal.try_connect(conn()), Ok(()));
        assert_eq!(
            signal.try_connect(conn()),
            Ok(()),
            "with REFERENCE_COUNTED a duplicate is accepted, not refused"
        );
        assert_eq!(
            signal.connection_count(),
            1,
            "counted twice, but still ONE connection — so it dispatches once"
        );

        assert!(signal.disconnect(target_id, "on_counted"));
        assert_eq!(
            signal.connection_count(),
            1,
            "the first disconnect only drops the count to 1"
        );
        assert!(signal.disconnect(target_id, "on_counted"));
        assert_eq!(
            signal.connection_count(),
            0,
            "the second disconnect removes it"
        );
    }

    #[test]
    fn disconnect_nonexistent_returns_false() {
        let mut signal = Signal::new("test");
        assert!(!signal.disconnect(ObjectId::next(), "no_method"));
    }

    #[test]
    fn signal_name_accessor() {
        let signal = Signal::new("my_signal");
        assert_eq!(signal.name(), "my_signal");
    }

    #[test]
    fn signal_store_add_signal_is_idempotent() {
        let mut store = SignalStore::new();
        store.add_signal("ready");
        store.add_signal("ready"); // should not panic or create duplicate
        assert!(store.has_signal("ready"));
        assert_eq!(store.get_signal("ready").unwrap().connection_count(), 0);
    }

    #[test]
    fn signal_store_disconnect_nonexistent_signal_returns_false() {
        let mut store = SignalStore::new();
        assert!(!store.disconnect("missing", ObjectId::next(), "method"));
    }

    #[test]
    fn signal_store_get_signal_returns_none_for_missing() {
        let store = SignalStore::new();
        assert!(store.get_signal("missing").is_none());
    }

    #[test]
    fn signal_store_signal_names() {
        let mut store = SignalStore::new();
        store.add_signal("a");
        store.add_signal("b");
        let mut names = store.signal_names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn signal_store_disconnect_all_for_target() {
        let mut store = SignalStore::new();
        let target = ObjectId::next();
        store.connect("sig_a", Connection::new(target, "method_a"));
        store.connect("sig_b", Connection::new(target, "method_b"));
        store.connect("sig_a", Connection::new(ObjectId::next(), "other"));

        store.disconnect_all_for(target);
        assert_eq!(store.get_signal("sig_a").unwrap().connection_count(), 1);
        assert_eq!(store.get_signal("sig_b").unwrap().connection_count(), 0);
    }

    #[test]
    fn connection_clone() {
        let conn = Connection::with_callback(ObjectId::next(), "method", |_| Variant::Int(42));
        let cloned = conn.clone();
        assert_eq!(cloned.method, "method");
        assert_eq!(cloned.call(&[]), Variant::Int(42));
    }

    #[test]
    fn connection_debug_format() {
        let conn = Connection::new(ObjectId::from_raw(1), "test_method");
        let debug = format!("{conn:?}");
        assert!(debug.contains("Connection"));
        assert!(debug.contains("test_method"));
    }
}
