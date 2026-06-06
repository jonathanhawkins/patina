//! Parity test: signal argument forwarding for typed and untyped callables.
//!
//! Verifies that Patina's signal system matches Godot's behavior for:
//! - Direct argument forwarding (all receivers get identical args)
//! - Callable.bind() appending extra arguments after signal args
//! - Callable.unbind() dropping trailing signal arguments
//! - Nested bind/unbind combinations
//! - Both typed (Method) and untyped (Lambda) callable variants
//!
//! Oracle: Godot 4.6.1-stable behavioral contract + fixtures/golden/traces/signal_arguments_oracle.json

use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdobject::signal::{Connection, Signal, SignalStore};
use gdvariant::{CallableRef, Variant};

/// Helper: collects received arguments from a callback into a shared vec.
fn recording_callback(
    recorder: Arc<Mutex<Vec<Vec<Variant>>>>,
) -> impl Fn(&[Variant]) -> Variant + Send + Sync + 'static {
    move |args: &[Variant]| {
        recorder.lock().unwrap().push(args.to_vec());
        Variant::Nil
    }
}

// ---------------------------------------------------------------------------
// 1. Basic argument forwarding: every receiver gets the same args
// ---------------------------------------------------------------------------

#[test]
fn all_receivers_get_identical_args() {
    let mut signal = Signal::new("health_changed");
    let recorder_a = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));
    let recorder_b = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    signal.connect(Connection::with_callback(
        ObjectId::next(),
        "on_health_a",
        recording_callback(recorder_a.clone()),
    ));
    signal.connect(Connection::with_callback(
        ObjectId::next(),
        "on_health_b",
        recording_callback(recorder_b.clone()),
    ));

    let emit_args = vec![Variant::Int(42), Variant::String("damage".into())];
    signal.emit(&emit_args);

    let a = recorder_a.lock().unwrap();
    let b = recorder_b.lock().unwrap();
    assert_eq!(a.len(), 1, "ReceiverA should fire once");
    assert_eq!(b.len(), 1, "ReceiverB should fire once");
    assert_eq!(a[0], emit_args, "ReceiverA gets exact signal args");
    assert_eq!(b[0], emit_args, "ReceiverB gets exact signal args");
}

// ---------------------------------------------------------------------------
// 2. Oracle parity: multi-type argument forwarding
// ---------------------------------------------------------------------------

#[test]
fn multi_type_args_match_oracle() {
    // Matches fixtures/golden/traces/signal_arguments_oracle.json "item_collected"
    let mut signal = Signal::new("item_collected");
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    signal.connect(Connection::with_callback(
        ObjectId::next(),
        "on_item",
        recording_callback(recorder.clone()),
    ));

    let args = vec![
        Variant::String("gold_coin".into()),
        Variant::Int(5),
        Variant::Bool(true),
    ];
    signal.emit(&args);

    let recorded = recorder.lock().unwrap();
    assert_eq!(recorded[0], args);
}

// ---------------------------------------------------------------------------
// 3. Zero-arg signal emission
// ---------------------------------------------------------------------------

#[test]
fn zero_arg_signal_forwards_empty_slice() {
    let mut signal = Signal::new("no_args_signal");
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    signal.connect(Connection::with_callback(
        ObjectId::next(),
        "on_empty",
        recording_callback(recorder.clone()),
    ));

    signal.emit(&[]);

    let recorded = recorder.lock().unwrap();
    assert_eq!(recorded[0], Vec::<Variant>::new());
}

// ---------------------------------------------------------------------------
// 4. Connection.with_binds() — appends extra args after signal args
// ---------------------------------------------------------------------------

#[test]
fn connection_bind_appends_args() {
    let mut signal = Signal::new("data");
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    let conn = Connection::with_callback(
        ObjectId::next(),
        "on_data",
        recording_callback(recorder.clone()),
    )
    .with_binds(vec![Variant::String("extra".into()), Variant::Int(99)]);

    signal.connect(conn);
    signal.emit(&[Variant::Int(1)]);

    let recorded = recorder.lock().unwrap();
    assert_eq!(
        recorded[0],
        vec![
            Variant::Int(1),
            Variant::String("extra".into()),
            Variant::Int(99),
        ],
        "bind appends extra args after signal args"
    );
}

// ---------------------------------------------------------------------------
// 5. Connection.with_unbinds() — drops trailing signal args
// ---------------------------------------------------------------------------

#[test]
fn connection_unbind_drops_trailing_args() {
    let mut signal = Signal::new("verbose");
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    let conn = Connection::with_callback(
        ObjectId::next(),
        "on_verbose",
        recording_callback(recorder.clone()),
    )
    .with_unbinds(2);

    signal.connect(conn);
    signal.emit(&[
        Variant::Int(1),
        Variant::Int(2),
        Variant::Int(3),
        Variant::Int(4),
    ]);

    let recorded = recorder.lock().unwrap();
    assert_eq!(
        recorded[0],
        vec![Variant::Int(1), Variant::Int(2)],
        "unbind(2) drops 2 trailing signal args"
    );
}

// ---------------------------------------------------------------------------
// 6. Combined bind + unbind on a single connection
// ---------------------------------------------------------------------------

#[test]
fn connection_unbind_then_bind() {
    let mut signal = Signal::new("complex");
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    // Unbind 1 trailing arg, then bind an extra arg
    let conn = Connection::with_callback(
        ObjectId::next(),
        "on_complex",
        recording_callback(recorder.clone()),
    )
    .with_unbinds(1)
    .with_binds(vec![Variant::String("bound".into())]);

    signal.connect(conn);
    signal.emit(&[Variant::Int(10), Variant::Int(20)]);

    let recorded = recorder.lock().unwrap();
    // Original [10, 20], unbind 1 → [10], bind "bound" → [10, "bound"]
    assert_eq!(
        recorded[0],
        vec![Variant::Int(10), Variant::String("bound".into())],
    );
}

// ---------------------------------------------------------------------------
// 7. Unbind more than available args saturates to zero
// ---------------------------------------------------------------------------

#[test]
fn unbind_saturates_to_zero_args() {
    let mut signal = Signal::new("short");
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    let conn = Connection::with_callback(
        ObjectId::next(),
        "on_short",
        recording_callback(recorder.clone()),
    )
    .with_unbinds(10);

    signal.connect(conn);
    signal.emit(&[Variant::Int(1)]);

    let recorded = recorder.lock().unwrap();
    assert_eq!(
        recorded[0],
        Vec::<Variant>::new(),
        "over-unbind yields empty args"
    );
}

// ---------------------------------------------------------------------------
// 8. Deferred connection applies bind/unbind when queued
// ---------------------------------------------------------------------------

#[test]
fn deferred_connection_resolves_binds() {
    let mut signal = Signal::new("deferred_data");
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    let conn = Connection::with_callback(
        ObjectId::next(),
        "on_deferred",
        recording_callback(recorder.clone()),
    )
    .as_deferred()
    .with_binds(vec![Variant::Bool(true)]);

    signal.connect(conn);

    let (immediate, deferred) = signal.emit_collecting_deferred(&[Variant::Int(42)]);

    assert!(
        immediate.is_empty(),
        "deferred connections produce no immediate results"
    );
    assert_eq!(deferred.len(), 1);

    // The captured args should already have binds applied
    assert_eq!(
        deferred[0].args(),
        &[Variant::Int(42), Variant::Bool(true)],
        "deferred call captures resolved (bound) args"
    );

    // Calling the deferred callback should also work
    deferred[0].call();
    let recorded = recorder.lock().unwrap();
    assert_eq!(recorded[0], vec![Variant::Int(42), Variant::Bool(true)],);
}

// ---------------------------------------------------------------------------
// 9. SignalStore emit applies bind/unbind through the store API
// ---------------------------------------------------------------------------

#[test]
fn signal_store_emit_with_binds() {
    let mut store = SignalStore::new();
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    let conn = Connection::with_callback(
        ObjectId::next(),
        "on_store",
        recording_callback(recorder.clone()),
    )
    .with_binds(vec![Variant::Float(3.14)]);

    store.connect("my_signal", conn);
    store.emit("my_signal", &[Variant::String("hello".into())]);

    let recorded = recorder.lock().unwrap();
    assert_eq!(
        recorded[0],
        vec![Variant::String("hello".into()), Variant::Float(3.14)],
    );
}

// ---------------------------------------------------------------------------
// 10. Mixed connections: some with binds, some without
// ---------------------------------------------------------------------------

#[test]
fn mixed_bound_and_unbound_connections() {
    let mut signal = Signal::new("mixed");
    let rec_plain = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));
    let rec_bound = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    signal.connect(Connection::with_callback(
        ObjectId::next(),
        "plain",
        recording_callback(rec_plain.clone()),
    ));
    signal.connect(
        Connection::with_callback(
            ObjectId::next(),
            "bound",
            recording_callback(rec_bound.clone()),
        )
        .with_binds(vec![Variant::Int(99)]),
    );

    signal.emit(&[Variant::Int(1)]);

    let plain = rec_plain.lock().unwrap();
    let bound = rec_bound.lock().unwrap();
    assert_eq!(plain[0], vec![Variant::Int(1)], "plain gets original args");
    assert_eq!(
        bound[0],
        vec![Variant::Int(1), Variant::Int(99)],
        "bound gets args + binds"
    );
}

// ---------------------------------------------------------------------------
// 11. CallableRef::Bound — resolve_args
// ---------------------------------------------------------------------------

#[test]
fn callable_ref_bound_resolve_args() {
    let base = CallableRef::Method {
        target_id: 1,
        method: "test".into(),
    };
    let bound = CallableRef::Bound {
        inner: Box::new(base),
        bound_args: vec![Variant::String("extra".into())],
    };

    let resolved = bound.resolve_args(&[Variant::Int(10)]);
    assert_eq!(
        resolved,
        vec![Variant::Int(10), Variant::String("extra".into())]
    );
}

// ---------------------------------------------------------------------------
// 12. CallableRef::Unbound — resolve_args
// ---------------------------------------------------------------------------

#[test]
fn callable_ref_unbound_resolve_args() {
    let base = CallableRef::Method {
        target_id: 1,
        method: "test".into(),
    };
    let unbound = CallableRef::Unbound {
        inner: Box::new(base),
        unbind_count: 1,
    };

    let resolved = unbound.resolve_args(&[Variant::Int(1), Variant::Int(2), Variant::Int(3)]);
    assert_eq!(resolved, vec![Variant::Int(1), Variant::Int(2)]);
}

// ---------------------------------------------------------------------------
// 13. CallableRef::inner_callable unwraps nested layers
// ---------------------------------------------------------------------------

#[test]
fn callable_ref_inner_callable_unwraps() {
    let method = CallableRef::Method {
        target_id: 42,
        method: "deep".into(),
    };
    let bound = CallableRef::Bound {
        inner: Box::new(method.clone()),
        bound_args: vec![],
    };
    let unbound = CallableRef::Unbound {
        inner: Box::new(bound),
        unbind_count: 1,
    };

    let inner = unbound.inner_callable();
    assert_eq!(
        inner,
        &CallableRef::Method {
            target_id: 42,
            method: "deep".into(),
        }
    );
}

// ---------------------------------------------------------------------------
// 14. CallableRef::Bound equality
// ---------------------------------------------------------------------------

#[test]
fn callable_ref_bound_equality() {
    let a = CallableRef::Bound {
        inner: Box::new(CallableRef::Method {
            target_id: 1,
            method: "m".into(),
        }),
        bound_args: vec![Variant::Int(1)],
    };
    let b = CallableRef::Bound {
        inner: Box::new(CallableRef::Method {
            target_id: 1,
            method: "m".into(),
        }),
        bound_args: vec![Variant::Int(1)],
    };
    let c = CallableRef::Bound {
        inner: Box::new(CallableRef::Method {
            target_id: 1,
            method: "m".into(),
        }),
        bound_args: vec![Variant::Int(2)],
    };
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// ---------------------------------------------------------------------------
// 15. One-shot with binds: fires once with resolved args, then disconnects
// ---------------------------------------------------------------------------

#[test]
fn one_shot_with_binds_fires_once() {
    let mut signal = Signal::new("oneshot_bound");
    let recorder = Arc::new(Mutex::new(Vec::<Vec<Variant>>::new()));

    let conn = Connection::with_callback(
        ObjectId::next(),
        "handler",
        recording_callback(recorder.clone()),
    )
    .as_one_shot()
    .with_binds(vec![Variant::String("tag".into())]);

    signal.connect(conn);

    signal.emit(&[Variant::Int(1)]);
    signal.emit(&[Variant::Int(2)]);

    let recorded = recorder.lock().unwrap();
    assert_eq!(recorded.len(), 1, "one-shot fires exactly once");
    assert_eq!(
        recorded[0],
        vec![Variant::Int(1), Variant::String("tag".into())],
    );
    assert_eq!(
        signal.connection_count(),
        0,
        "one-shot removed after firing"
    );
}

// ===========================================================================
// pat-zud: Typed vs untyped callable argument forwarding parity
// ===========================================================================

// ---------------------------------------------------------------------------
// 16. CallableRef::Lambda resolve_args passes through unchanged
// ---------------------------------------------------------------------------

#[test]
fn lambda_callable_resolve_args_passthrough() {
    let lambda = CallableRef::Lambda {
        params: vec!["x".into(), "y".into()],
        body: std::sync::Arc::new(()),
    };

    let args = vec![Variant::Int(10), Variant::String("hello".into())];
    let resolved = lambda.resolve_args(&args);
    assert_eq!(
        resolved, args,
        "Lambda resolve_args should pass through unchanged"
    );
}

// ---------------------------------------------------------------------------
// 17. Lambda wrapped in Bound — appends args
// ---------------------------------------------------------------------------

#[test]
fn lambda_bound_resolve_args() {
    let lambda = CallableRef::Lambda {
        params: vec!["a".into()],
        body: std::sync::Arc::new(()),
    };
    let bound = CallableRef::Bound {
        inner: Box::new(lambda),
        bound_args: vec![Variant::Bool(true), Variant::Float(2.5)],
    };

    let resolved = bound.resolve_args(&[Variant::Int(1)]);
    assert_eq!(
        resolved,
        vec![Variant::Int(1), Variant::Bool(true), Variant::Float(2.5)],
        "Lambda.bind() should append bound args"
    );
}

// ---------------------------------------------------------------------------
// 18. Lambda wrapped in Unbound — drops trailing args
// ---------------------------------------------------------------------------

#[test]
fn lambda_unbound_resolve_args() {
    let lambda = CallableRef::Lambda {
        params: vec!["a".into(), "b".into(), "c".into()],
        body: std::sync::Arc::new(()),
    };
    let unbound = CallableRef::Unbound {
        inner: Box::new(lambda),
        unbind_count: 2,
    };

    let resolved = unbound.resolve_args(&[Variant::Int(1), Variant::Int(2), Variant::Int(3)]);
    assert_eq!(
        resolved,
        vec![Variant::Int(1)],
        "Lambda.unbind(2) should drop 2 trailing args"
    );
}

// ---------------------------------------------------------------------------
// 19. Nested bind(bind()) — double wrapping stacks args
// ---------------------------------------------------------------------------

#[test]
fn nested_bind_stacks_args() {
    let method = CallableRef::Method {
        target_id: 1,
        method: "handler".into(),
    };
    let bind1 = CallableRef::Bound {
        inner: Box::new(method),
        bound_args: vec![Variant::Int(100)],
    };
    let bind2 = CallableRef::Bound {
        inner: Box::new(bind1),
        bound_args: vec![Variant::Int(200)],
    };

    // Outer bind resolves: [signal_args..., 200]
    // But bind only looks at its own level, not recursively.
    // Godot's bind() is the same: bind().bind() creates two layers.
    let resolved = bind2.resolve_args(&[Variant::Int(0)]);
    assert_eq!(
        resolved,
        vec![Variant::Int(0), Variant::Int(200)],
        "outer bind applies its own bound_args"
    );

    // inner_callable should unwrap to the Method.
    let inner = bind2.inner_callable();
    match inner {
        CallableRef::Method { method, .. } => assert_eq!(method, "handler"),
        _ => panic!("inner_callable should be Method"),
    }
}

// ---------------------------------------------------------------------------
// 20. Nested unbind(bind()) — unbind then bind
// ---------------------------------------------------------------------------

#[test]
fn nested_unbind_wrapping_bind() {
    let method = CallableRef::Method {
        target_id: 1,
        method: "m".into(),
    };
    let bound = CallableRef::Bound {
        inner: Box::new(method),
        bound_args: vec![Variant::String("extra".into())],
    };
    let unbound = CallableRef::Unbound {
        inner: Box::new(bound),
        unbind_count: 1,
    };

    // Unbound resolves first: drops 1 trailing arg from call_args
    let resolved = unbound.resolve_args(&[Variant::Int(1), Variant::Int(2), Variant::Int(3)]);
    assert_eq!(
        resolved,
        vec![Variant::Int(1), Variant::Int(2)],
        "outer unbind drops trailing arg, inner bind not applied at this level"
    );
}

// ---------------------------------------------------------------------------
// 21. Method vs Lambda produce same forwarded args
// ---------------------------------------------------------------------------

#[test]
fn method_and_lambda_forward_identically() {
    let method = CallableRef::Method {
        target_id: 1,
        method: "handle".into(),
    };
    let lambda = CallableRef::Lambda {
        params: vec!["a".into(), "b".into()],
        body: std::sync::Arc::new(()),
    };

    let args = vec![
        Variant::Int(42),
        Variant::String("test".into()),
        Variant::Bool(false),
    ];

    let method_resolved = method.resolve_args(&args);
    let lambda_resolved = lambda.resolve_args(&args);

    assert_eq!(
        method_resolved, lambda_resolved,
        "Method and Lambda should forward args identically when no bind/unbind"
    );
}

// ---------------------------------------------------------------------------
// 22. Method.bind() vs Lambda.bind() produce same results
// ---------------------------------------------------------------------------

#[test]
fn method_bind_vs_lambda_bind_identical() {
    let bound_args = vec![Variant::String("appended".into())];

    let method_bound = CallableRef::Bound {
        inner: Box::new(CallableRef::Method {
            target_id: 1,
            method: "m".into(),
        }),
        bound_args: bound_args.clone(),
    };
    let lambda_bound = CallableRef::Bound {
        inner: Box::new(CallableRef::Lambda {
            params: vec![],
            body: std::sync::Arc::new(()),
        }),
        bound_args,
    };

    let call_args = vec![Variant::Int(5)];
    assert_eq!(
        method_bound.resolve_args(&call_args),
        lambda_bound.resolve_args(&call_args),
        "Method.bind() and Lambda.bind() should produce identical resolved args"
    );
}

// ---------------------------------------------------------------------------
// 23. Variant type preservation through bind/unbind chain
// ---------------------------------------------------------------------------

#[test]
fn variant_types_preserved_through_bind_unbind() {
    use gdcore::math::Vector2;

    let method = CallableRef::Method {
        target_id: 1,
        method: "on_hit".into(),
    };
    let bound = CallableRef::Bound {
        inner: Box::new(method),
        bound_args: vec![Variant::Vector2(Vector2::new(1.0, 2.0)), Variant::Nil],
    };

    let call_args = vec![
        Variant::Int(42),
        Variant::Float(3.14),
        Variant::Bool(true),
        Variant::String("damage".into()),
    ];
    let resolved = bound.resolve_args(&call_args);

    // All original args + bound args, types preserved.
    assert_eq!(resolved.len(), 6);
    assert_eq!(resolved[0], Variant::Int(42));
    assert_eq!(resolved[1], Variant::Float(3.14));
    assert_eq!(resolved[2], Variant::Bool(true));
    assert_eq!(resolved[3], Variant::String("damage".into()));
    assert_eq!(resolved[4], Variant::Vector2(Vector2::new(1.0, 2.0)));
    assert_eq!(resolved[5], Variant::Nil);
}
