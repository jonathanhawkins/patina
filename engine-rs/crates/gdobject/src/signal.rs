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
}

impl Connection {
    /// Creates a connection with just target ID and method name.
    pub fn new(target_id: ObjectId, method: impl Into<String>) -> Self {
        Self {
            target_id,
            method: method.into(),
            callback: None,
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
        }
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

    /// Adds a connection. Duplicates (same target + method) are allowed,
    /// matching Godot's default behavior.
    pub fn connect(&mut self, connection: Connection) {
        self.connections.push(connection);
    }

    /// Removes the first connection matching the given target and method.
    ///
    /// Returns `true` if a connection was removed.
    pub fn disconnect(&mut self, target_id: ObjectId, method: &str) -> bool {
        if let Some(pos) = self
            .connections
            .iter()
            .position(|c| c.target_id == target_id && c.method == method)
        {
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
    pub fn emit(&self, args: &[Variant]) -> Vec<Variant> {
        self.connections.iter().map(|c| c.call(args)).collect()
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

    /// Connects a callback to the named signal, creating the signal if needed.
    pub fn connect(&mut self, signal_name: &str, connection: Connection) {
        self.signals
            .entry(signal_name.to_owned())
            .or_insert_with(|| Signal::new(signal_name))
            .connect(connection);
    }

    /// Disconnects the first matching connection from a signal.
    ///
    /// Returns `true` if a connection was removed.
    pub fn disconnect(
        &mut self,
        signal_name: &str,
        target_id: ObjectId,
        method: &str,
    ) -> bool {
        self.signals
            .get_mut(signal_name)
            .is_some_and(|s| s.disconnect(target_id, method))
    }

    /// Emits a named signal. Returns the collected return values.
    ///
    /// If the signal does not exist, returns an empty vec (matching Godot's
    /// behavior of silently ignoring emission on undeclared signals).
    pub fn emit(&self, signal_name: &str, args: &[Variant]) -> Vec<Variant> {
        self.signals
            .get(signal_name)
            .map_or_else(Vec::new, |s| s.emit(args))
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
    fn disconnect_signal(
        &mut self,
        signal_name: &str,
        target_id: ObjectId,
        method: &str,
    ) -> bool {
        self.signal_store_mut()
            .disconnect(signal_name, target_id, method)
    }

    /// Emits a signal with the given arguments.
    fn emit_signal(&self, signal_name: &str, args: &[Variant]) -> Vec<Variant> {
        self.signal_store().emit(signal_name, args)
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

        signal.connect(Connection::with_callback(
            target_a,
            "on_changed",
            |_| Variant::Int(1),
        ));
        signal.connect(Connection::with_callback(
            target_b,
            "on_changed",
            |_| Variant::Int(2),
        ));

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

        store.connect(
            "new_signal",
            Connection::new(ObjectId::next(), "handler"),
        );

        assert!(store.has_signal("new_signal"));
    }

    #[test]
    fn connection_without_callback() {
        let conn = Connection::new(ObjectId::next(), "some_method");
        assert!(!conn.has_callback());
        assert_eq!(conn.call(&[]), Variant::Nil);
    }
}
