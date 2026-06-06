//! pat-8wqm: Signal declaration and emit_signal callable from script.

use gdcore::id::ObjectId;
use gdobject::signal::{Connection, SignalStore};
use gdvariant::Variant;

#[test]
fn signal_declaration() {
    let mut store = SignalStore::new();
    store.add_signal("health_changed");
    assert!(store.has_signal("health_changed"));
}

#[test]
fn signal_not_declared_returns_false() {
    let store = SignalStore::new();
    assert!(!store.has_signal("nonexistent"));
}

#[test]
fn signal_connect_and_emit() {
    let mut store = SignalStore::new();
    store.add_signal("hit");

    let target_id = ObjectId::next();
    let conn = Connection::new(target_id, "on_hit");
    store.connect("hit", conn);

    // Emit returns collected results (empty for basic connections without callbacks)
    let results = store.emit("hit", &[Variant::Int(10)]);
    // Just verify it doesn't panic — callback-based verification needs a callback
    let _ = results;
}

#[test]
fn signal_with_callback_invoked() {
    use std::sync::{Arc, Mutex};

    let mut store = SignalStore::new();
    store.add_signal("scored");

    let called = Arc::new(Mutex::new(false));
    let called_clone = called.clone();

    let target_id = ObjectId::next();
    let conn = Connection::with_callback(target_id, "on_scored", move |_args| {
        *called_clone.lock().unwrap() = true;
        Variant::Nil
    });
    store.connect("scored", conn);

    store.emit("scored", &[Variant::Int(100)]);
    assert!(*called.lock().unwrap(), "callback should have been invoked");
}

#[test]
fn signal_emit_unregistered_is_safe() {
    let mut store = SignalStore::new();
    let results = store.emit("nonexistent", &[]);
    assert!(results.is_empty());
}

#[test]
fn signal_multiple_connections_all_fire() {
    use std::sync::{Arc, Mutex};

    let mut store = SignalStore::new();
    store.add_signal("tick");

    let count = Arc::new(Mutex::new(0u32));

    for _ in 0..3 {
        let count_clone = count.clone();
        let conn = Connection::with_callback(ObjectId::next(), "on_tick", move |_| {
            *count_clone.lock().unwrap() += 1;
            Variant::Nil
        });
        store.connect("tick", conn);
    }

    store.emit("tick", &[]);
    assert_eq!(*count.lock().unwrap(), 3);
}
