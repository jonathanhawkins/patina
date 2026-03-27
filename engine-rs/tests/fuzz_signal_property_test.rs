//! Property tests for the signal connection and emission system.
//!
//! Tests cover connection ordering, one-shot semantics, deferred queuing,
//! disconnect cleanup, and bind/unbind argument resolution.

use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdobject::signal::{Connection, Signal, SignalStore};
use gdvariant::Variant;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Connection ordering: callbacks fire in registration order
// ---------------------------------------------------------------------------

proptest! {
    /// Connections fire in the order they were added.
    #[test]
    fn emit_fires_in_registration_order(n in 1usize..20) {
        let log = Arc::new(Mutex::new(Vec::<usize>::new()));
        let mut signal = Signal::new("test_signal");

        for i in 0..n {
            let log_clone = Arc::clone(&log);
            let conn = Connection::with_callback(
                ObjectId::from_raw(i as u64 + 1),
                format!("method_{i}"),
                move |_args| {
                    log_clone.lock().unwrap().push(i);
                    Variant::Nil
                },
            );
            signal.connect(conn);
        }

        signal.emit(&[]);
        let recorded = log.lock().unwrap();
        let expected: Vec<usize> = (0..n).collect();
        prop_assert_eq!(&*recorded, &expected,
            "Callbacks should fire in registration order");
    }
}

// ---------------------------------------------------------------------------
// One-shot: auto-disconnects after first emission
// ---------------------------------------------------------------------------

proptest! {
    /// A one-shot connection fires exactly once across multiple emissions.
    #[test]
    fn one_shot_fires_once(emissions in 2usize..10) {
        let counter = Arc::new(Mutex::new(0usize));
        let mut signal = Signal::new("one_shot_test");

        let counter_clone = Arc::clone(&counter);
        let conn = Connection::with_callback(
            ObjectId::from_raw(1),
            "on_signal",
            move |_args| {
                *counter_clone.lock().unwrap() += 1;
                Variant::Nil
            },
        ).as_one_shot();
        signal.connect(conn);

        for _ in 0..emissions {
            signal.emit(&[]);
        }

        let count = *counter.lock().unwrap();
        prop_assert_eq!(count, 1,
            "One-shot should fire exactly once, fired {} times", count);
    }
}

// ---------------------------------------------------------------------------
// Disconnect: removing a connection prevents future emissions
// ---------------------------------------------------------------------------

proptest! {
    /// After disconnecting, the callback no longer fires.
    #[test]
    fn disconnect_prevents_fire(n in 2usize..10) {
        let counters: Vec<Arc<Mutex<usize>>> = (0..n)
            .map(|_| Arc::new(Mutex::new(0usize)))
            .collect();
        let mut signal = Signal::new("disconnect_test");

        for i in 0..n {
            let counter = Arc::clone(&counters[i]);
            let conn = Connection::with_callback(
                ObjectId::from_raw(i as u64 + 1),
                format!("method_{i}"),
                move |_args| {
                    *counter.lock().unwrap() += 1;
                    Variant::Nil
                },
            );
            signal.connect(conn);
        }

        // Emit once — all should fire
        signal.emit(&[]);
        for (i, c) in counters.iter().enumerate() {
            prop_assert_eq!(*c.lock().unwrap(), 1,
                "Connection {} should have fired once", i);
        }

        // Disconnect the first connection
        signal.disconnect(ObjectId::from_raw(1), "method_0");

        // Emit again — first should not fire, others should
        signal.emit(&[]);
        prop_assert_eq!(*counters[0].lock().unwrap(), 1,
            "Disconnected connection should not fire again");
        for (i, c) in counters.iter().enumerate().skip(1) {
            prop_assert_eq!(*c.lock().unwrap(), 2,
                "Connection {} should have fired twice", i);
        }
    }
}

// ---------------------------------------------------------------------------
// disconnect_all_for: bulk cleanup by target object
// ---------------------------------------------------------------------------

proptest! {
    /// disconnect_all_for removes all connections for a given target.
    #[test]
    fn disconnect_all_for_target(
        target_conns in 1usize..5,
        other_conns in 1usize..5,
    ) {
        let target_counter = Arc::new(Mutex::new(0usize));
        let other_counter = Arc::new(Mutex::new(0usize));
        let mut signal = Signal::new("bulk_disconnect");

        let target_id = ObjectId::from_raw(100);
        let other_id = ObjectId::from_raw(200);

        for i in 0..target_conns {
            let c = Arc::clone(&target_counter);
            let conn = Connection::with_callback(
                target_id,
                format!("target_method_{i}"),
                move |_| { *c.lock().unwrap() += 1; Variant::Nil },
            );
            signal.connect(conn);
        }
        for i in 0..other_conns {
            let c = Arc::clone(&other_counter);
            let conn = Connection::with_callback(
                other_id,
                format!("other_method_{i}"),
                move |_| { *c.lock().unwrap() += 1; Variant::Nil },
            );
            signal.connect(conn);
        }

        // Disconnect all for the target
        signal.disconnect_all_for(target_id);

        // Emit
        signal.emit(&[]);

        prop_assert_eq!(*target_counter.lock().unwrap(), 0,
            "Target connections should not fire after disconnect_all_for");
        prop_assert_eq!(*other_counter.lock().unwrap(), other_conns,
            "Other connections should still fire");
    }
}

// ---------------------------------------------------------------------------
// Bind/unbind argument resolution
// ---------------------------------------------------------------------------

proptest! {
    /// Bound arguments are appended after signal args.
    #[test]
    fn connection_bind_appends(
        signal_arg_count in 0usize..5,
        bind_count in 0usize..5,
    ) {
        let signal_args: Vec<Variant> = (0..signal_arg_count as i64)
            .map(|i| Variant::Int(i))
            .collect();
        let binds: Vec<Variant> = (100..100 + bind_count as i64)
            .map(|i| Variant::Int(i))
            .collect();

        let conn = Connection::new(ObjectId::from_raw(1), "method")
            .with_binds(binds.clone());
        let resolved = conn.resolve_args(&signal_args);

        prop_assert_eq!(resolved.len(), signal_arg_count + bind_count);
        for (i, arg) in signal_args.iter().enumerate() {
            prop_assert_eq!(&resolved[i], arg);
        }
        for (i, arg) in binds.iter().enumerate() {
            prop_assert_eq!(&resolved[signal_arg_count + i], arg);
        }
    }

    /// Unbind drops trailing signal arguments.
    #[test]
    fn connection_unbind_drops(
        signal_arg_count in 0usize..10,
        unbind_count in 0usize..10,
    ) {
        let signal_args: Vec<Variant> = (0..signal_arg_count as i64)
            .map(|i| Variant::Int(i))
            .collect();

        let conn = Connection::new(ObjectId::from_raw(1), "method")
            .with_unbinds(unbind_count);
        let resolved = conn.resolve_args(&signal_args);

        let expected_len = signal_arg_count.saturating_sub(unbind_count);
        prop_assert_eq!(resolved.len(), expected_len);
        for (i, arg) in resolved.iter().enumerate() {
            prop_assert_eq!(arg, &signal_args[i]);
        }
    }

    /// Unbind then bind: first drop trailing, then append.
    #[test]
    fn connection_unbind_then_bind(
        signal_arg_count in 1usize..5,
        unbind_count in 0usize..3,
        bind_count in 0usize..3,
    ) {
        let signal_args: Vec<Variant> = (0..signal_arg_count as i64)
            .map(|i| Variant::Int(i))
            .collect();
        let binds: Vec<Variant> = (100..100 + bind_count as i64)
            .map(|i| Variant::Int(i))
            .collect();

        let conn = Connection::new(ObjectId::from_raw(1), "method")
            .with_unbinds(unbind_count)
            .with_binds(binds.clone());
        let resolved = conn.resolve_args(&signal_args);

        let truncated_len = signal_arg_count.saturating_sub(unbind_count);
        prop_assert_eq!(resolved.len(), truncated_len + bind_count);
    }
}

// ---------------------------------------------------------------------------
// SignalStore: multi-signal management
// ---------------------------------------------------------------------------

#[test]
fn signal_store_basic_lifecycle() {
    let mut store = SignalStore::new();
    let counter = Arc::new(Mutex::new(0usize));

    // Connect to a signal
    let c = Arc::clone(&counter);
    store.connect(
        "pressed",
        Connection::with_callback(ObjectId::from_raw(1), "on_pressed", move |_| {
            *c.lock().unwrap() += 1;
            Variant::Nil
        }),
    );

    // Emit
    store.emit("pressed", &[]);
    assert_eq!(*counter.lock().unwrap(), 1);

    // Disconnect
    store.disconnect("pressed", ObjectId::from_raw(1), "on_pressed");
    store.emit("pressed", &[]);
    assert_eq!(*counter.lock().unwrap(), 1, "Should not fire after disconnect");
}

#[test]
fn signal_store_deferred_queue_fifo() {
    let mut store = SignalStore::new();
    let log = Arc::new(Mutex::new(Vec::<i64>::new()));

    // Connect three deferred callbacks
    for i in 0..3 {
        let log_clone = Arc::clone(&log);
        store.connect(
            "tick",
            Connection::with_callback(
                ObjectId::from_raw(i as u64 + 1),
                format!("on_tick_{i}"),
                move |args| {
                    if let Some(Variant::Int(val)) = args.first() {
                        log_clone.lock().unwrap().push(*val);
                    }
                    Variant::Nil
                },
            ).as_deferred(),
        );
    }

    // Emit using emit_collecting_deferred — deferred connections should NOT fire immediately
    let (immediate, deferred) = store.emit_collecting_deferred("tick", &[Variant::Int(42)]);
    assert!(immediate.is_empty(), "No immediate results from deferred-only connections");

    // Invoke the deferred calls
    for call in &deferred {
        call.call();
    }

    let recorded = log.lock().unwrap();
    assert_eq!(recorded.len(), 3, "All 3 deferred callbacks should fire on flush");
    for val in recorded.iter() {
        assert_eq!(*val, 42);
    }
}

// ---------------------------------------------------------------------------
// Deferred connection + one-shot interaction
// ---------------------------------------------------------------------------

#[test]
fn deferred_one_shot_fires_once() {
    let mut store = SignalStore::new();
    let counter = Arc::new(Mutex::new(0usize));

    let c = Arc::clone(&counter);
    store.connect(
        "event",
        Connection::with_callback(ObjectId::from_raw(1), "handler", move |_| {
            *c.lock().unwrap() += 1;
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    // First emit + invoke deferred
    let (_, deferred1) = store.emit_collecting_deferred("event", &[]);
    for call in &deferred1 {
        call.call();
    }

    // Second emit + invoke deferred — one-shot should have been removed
    let (_, deferred2) = store.emit_collecting_deferred("event", &[]);
    for call in &deferred2 {
        call.call();
    }

    assert_eq!(*counter.lock().unwrap(), 1,
        "Deferred one-shot should fire exactly once");
}

// ---------------------------------------------------------------------------
// Emitting an undeclared signal is a no-op
// ---------------------------------------------------------------------------

#[test]
fn emit_undeclared_signal_is_noop() {
    let mut store = SignalStore::new();
    let results = store.emit("nonexistent", &[Variant::Int(1)]);
    assert!(results.is_empty());
}

// ---------------------------------------------------------------------------
// Multiple connects with same target+method (duplicates allowed)
// ---------------------------------------------------------------------------

#[test]
fn duplicate_connections_both_fire() {
    let counter = Arc::new(Mutex::new(0usize));
    let mut signal = Signal::new("dup_test");

    for _ in 0..3 {
        let c = Arc::clone(&counter);
        let conn = Connection::with_callback(
            ObjectId::from_raw(1),
            "on_event",
            move |_| { *c.lock().unwrap() += 1; Variant::Nil },
        );
        signal.connect(conn);
    }

    signal.emit(&[]);
    assert_eq!(*counter.lock().unwrap(), 3,
        "All duplicate connections should fire");
}

// ---------------------------------------------------------------------------
// Disconnect removes only the first matching connection
// ---------------------------------------------------------------------------

#[test]
fn disconnect_removes_first_only() {
    let counter = Arc::new(Mutex::new(0usize));
    let mut signal = Signal::new("disconnect_first");

    for _ in 0..3 {
        let c = Arc::clone(&counter);
        let conn = Connection::with_callback(
            ObjectId::from_raw(1),
            "handler",
            move |_| { *c.lock().unwrap() += 1; Variant::Nil },
        );
        signal.connect(conn);
    }

    // Disconnect one
    signal.disconnect(ObjectId::from_raw(1), "handler");

    signal.emit(&[]);
    assert_eq!(*counter.lock().unwrap(), 2,
        "After disconnecting one of three dups, two should remain");
}

// ---------------------------------------------------------------------------
// Signal argument forwarding
// ---------------------------------------------------------------------------

proptest! {
    /// Signal args are forwarded to callbacks intact.
    #[test]
    fn args_forwarded_intact(n in 0usize..10) {
        let args: Vec<Variant> = (0..n as i64).map(Variant::Int).collect();
        let received = Arc::new(Mutex::new(Vec::<Variant>::new()));

        let mut signal = Signal::new("arg_test");
        let recv = Arc::clone(&received);
        signal.connect(Connection::with_callback(
            ObjectId::from_raw(1),
            "handler",
            move |a| {
                *recv.lock().unwrap() = a.to_vec();
                Variant::Nil
            },
        ));

        signal.emit(&args);
        let got = received.lock().unwrap();
        prop_assert_eq!(&*got, &args);
    }
}
