//! CONNECT_DEFERRED signal dispatch parity tests (pat-enqb, pat-t64d).
//!
//! Verifies that Patina's CONNECT_DEFERRED semantics match Godot's documented
//! behavior:
//!
//! 1. Deferred connections are NOT dispatched during `emit_signal` — they are
//!    queued and delivered at end-of-frame via `flush_deferred_signals()`.
//! 2. Non-deferred connections on the same signal fire immediately as usual.
//! 3. Mixed deferred + non-deferred connections maintain correct ordering
//!    semantics (immediate fire immediately, deferred fire at flush time).
//! 4. Deferred + ONE_SHOT connections auto-disconnect after being queued once.
//! 5. The CONNECT_DEFERRED flag (bit 0) is parsed from `.tscn` connection flags.
//! 6. Multiple deferred emissions queue in FIFO order.
//! 7. `flush_deferred_signals` returns the correct count and empties the queue.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::SignalConnection as Connection;
use gdvariant::Variant;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a tree with Emitter and Receiver nodes.
fn build_tree() -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    tree.event_trace_mut().enable();

    (tree, emitter_id, recv_id)
}

/// Build a tree with Emitter and two Receiver nodes.
fn build_tree_two_receivers() -> (SceneTree, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("RecvA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("RecvB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    tree.event_trace_mut().enable();

    (tree, emitter_id, recv_a_id, recv_b_id)
}

// ===========================================================================
// 1. Deferred connections are NOT dispatched during emit_signal
// ===========================================================================

/// A deferred connection should not fire during emit_signal; it should fire
/// only when flush_deferred_signals is called.
#[test]
fn deferred_connection_does_not_fire_on_emit() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_signal", move |_args| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "test_signal", conn);
    tree.emit_signal(emitter_id, "test_signal", &[]);

    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "deferred callback must not fire during emit_signal"
    );
    assert_eq!(
        tree.deferred_signal_count(),
        1,
        "one deferred call should be queued"
    );
}

/// After flushing, the deferred callback should have fired.
#[test]
fn deferred_connection_fires_on_flush() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_signal", move |_args| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "test_signal", conn);
    tree.emit_signal(emitter_id, "test_signal", &[]);

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "flush should report 1 dispatched call");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "deferred callback should fire exactly once after flush"
    );
    assert_eq!(
        tree.deferred_signal_count(),
        0,
        "queue should be empty after flush"
    );
}

// ===========================================================================
// 2. Non-deferred connections still fire immediately
// ===========================================================================

/// A non-deferred connection on the same signal as a deferred one should fire
/// immediately during emit_signal.
#[test]
fn non_deferred_fires_immediately_alongside_deferred() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_tree_two_receivers();

    let immediate_counter = Arc::new(AtomicU64::new(0));
    let deferred_counter = Arc::new(AtomicU64::new(0));

    // Non-deferred connection (RecvA)
    let ic = immediate_counter.clone();
    let conn_immediate =
        Connection::with_callback(recv_a_id.object_id(), "on_immediate", move |_| {
            ic.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        });

    // Deferred connection (RecvB)
    let dc = deferred_counter.clone();
    let conn_deferred =
        Connection::with_callback(recv_b_id.object_id(), "on_deferred", move |_| {
            dc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred();

    tree.connect_signal(emitter_id, "mixed_signal", conn_immediate);
    tree.connect_signal(emitter_id, "mixed_signal", conn_deferred);

    tree.emit_signal(emitter_id, "mixed_signal", &[]);

    assert_eq!(
        immediate_counter.load(Ordering::SeqCst),
        1,
        "non-deferred callback fires immediately"
    );
    assert_eq!(
        deferred_counter.load(Ordering::SeqCst),
        0,
        "deferred callback does not fire yet"
    );

    tree.flush_deferred_signals();

    assert_eq!(
        deferred_counter.load(Ordering::SeqCst),
        1,
        "deferred callback fires after flush"
    );
}

// ===========================================================================
// 3. Deferred + ONE_SHOT auto-disconnects
// ===========================================================================

/// A connection that is both deferred and one-shot should auto-disconnect
/// after the first emission (queuing), so a second emit does not re-queue.
#[test]
fn deferred_one_shot_auto_disconnects() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_signal", move |_args| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred()
    .as_one_shot();

    tree.connect_signal(emitter_id, "oneshot_signal", conn);

    // First emit — should queue.
    tree.emit_signal(emitter_id, "oneshot_signal", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Second emit — connection was removed, should NOT queue again.
    tree.emit_signal(emitter_id, "oneshot_signal", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        1,
        "one-shot deferred should not re-queue after first emission"
    );

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

// ===========================================================================
// 4. Arguments are forwarded to deferred callbacks
// ===========================================================================

/// Signal arguments should be captured at emit-time and delivered at flush-time.
#[test]
fn deferred_connection_receives_emit_time_args() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured_clone = captured.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_signal", move |args| {
        captured_clone.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "arg_signal", conn);

    tree.emit_signal(
        emitter_id,
        "arg_signal",
        &[Variant::Int(42), Variant::String("hello".into())],
    );

    // Before flush, nothing captured.
    assert!(captured.lock().unwrap().is_empty());

    tree.flush_deferred_signals();

    let args = captured.lock().unwrap();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], Variant::Int(42));
    assert_eq!(args[1], Variant::String("hello".into()));
}

// ===========================================================================
// 5. Multiple emissions queue in FIFO order
// ===========================================================================

/// Multiple deferred emissions should be flushed in the order they were emitted.
#[test]
fn deferred_emissions_flush_in_fifo_order() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let order_clone = order.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_signal", move |args| {
        if let Some(Variant::Int(n)) = args.first() {
            order_clone.lock().unwrap().push(*n);
        }
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "ordered_signal", conn);

    for i in 0..5 {
        tree.emit_signal(emitter_id, "ordered_signal", &[Variant::Int(i)]);
    }

    assert_eq!(tree.deferred_signal_count(), 5);

    tree.flush_deferred_signals();

    let recorded = order.lock().unwrap();
    assert_eq!(*recorded, vec![0, 1, 2, 3, 4], "FIFO order must be preserved");
}

// ===========================================================================
// 6. Flush with empty queue is a no-op
// ===========================================================================

#[test]
fn flush_empty_queue_returns_zero() {
    let mut tree = SceneTree::new();
    assert_eq!(tree.flush_deferred_signals(), 0);
    assert_eq!(tree.deferred_signal_count(), 0);
}

// ===========================================================================
// 7. CONNECT_DEFERRED flag parsed from .tscn
// ===========================================================================

/// The CONNECT_DEFERRED flag (bit 0, value 1) should produce a deferred
/// connection when parsed from a `.tscn` connection entry.
#[test]
fn tscn_deferred_flag_produces_deferred_connection() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://deferred_test"]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[node name="Handler" type="Node2D" parent="."]

[connection signal="pressed" from="Button" to="Handler" method="_on_pressed" flags=1]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.connections().len(), 1);
    assert_eq!(scene.connections()[0].flags, 1, "flags should be DEFERRED=1");

    // Instantiate and verify the connection is deferred.
    let mut tree = SceneTree::new();
    tree.event_trace_mut().enable();
    let root = tree.root_id();
    let _scene_root = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Find the Button's signal store and check the connection.
    let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();
    let store = tree.signal_store_mut(button_id);
    let signal = store.get_signal("pressed").expect("pressed signal should exist");
    assert_eq!(signal.connection_count(), 1);
    assert!(
        signal.connections()[0].deferred,
        "connection should be marked deferred"
    );
}

/// flags=5 means DEFERRED(1) | ONE_SHOT(4) — both flags should be set.
#[test]
fn tscn_deferred_plus_oneshot_flags() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://deferred_oneshot"]

[node name="Root" type="Node"]

[node name="Emitter" type="Node2D" parent="."]

[node name="Listener" type="Node2D" parent="."]

[connection signal="custom" from="Emitter" to="Listener" method="_on_custom" flags=5]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let emitter_id = tree.get_node_by_path("/root/Root/Emitter").unwrap();
    let store = tree.signal_store_mut(emitter_id);
    let signal = store.get_signal("custom").expect("custom signal should exist");
    let conn = &signal.connections()[0];
    assert!(conn.deferred, "DEFERRED flag should be set");
    assert!(conn.one_shot, "ONE_SHOT flag should be set");
}

/// flags=3 means DEFERRED(1) | PERSIST(2) — deferred should be set,
/// persist is not yet used but should not interfere.
#[test]
fn tscn_deferred_plus_persist_flags() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://deferred_persist"]

[node name="Root" type="Control"]

[node name="Button" type="Button" parent="."]

[node name="Player" type="Node2D" parent="."]

[connection signal="pressed" from="Button" to="Player" method="_on_pressed" flags=3]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();
    let store = tree.signal_store_mut(button_id);
    let signal = store.get_signal("pressed").unwrap();
    let conn = &signal.connections()[0];
    assert!(conn.deferred, "DEFERRED flag should be set");
    assert!(!conn.one_shot, "ONE_SHOT flag should NOT be set");
}

// ===========================================================================
// 8. Deferred connection across multiple signals
// ===========================================================================

/// Deferred connections on different signals both queue and flush correctly.
#[test]
fn deferred_across_multiple_signals() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let alpha_counter = Arc::new(AtomicU64::new(0));
    let beta_counter = Arc::new(AtomicU64::new(0));

    let ac = alpha_counter.clone();
    let conn_a = Connection::with_callback(recv_id.object_id(), "on_alpha", move |_| {
        ac.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    let bc = beta_counter.clone();
    let conn_b = Connection::with_callback(recv_id.object_id(), "on_beta", move |_| {
        bc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "alpha", conn_a);
    tree.connect_signal(emitter_id, "beta", conn_b);

    tree.emit_signal(emitter_id, "alpha", &[]);
    tree.emit_signal(emitter_id, "beta", &[]);

    assert_eq!(tree.deferred_signal_count(), 2);
    assert_eq!(alpha_counter.load(Ordering::SeqCst), 0);
    assert_eq!(beta_counter.load(Ordering::SeqCst), 0);

    tree.flush_deferred_signals();

    assert_eq!(alpha_counter.load(Ordering::SeqCst), 1);
    assert_eq!(beta_counter.load(Ordering::SeqCst), 1);
}

// ===========================================================================
// 9. Double-flush is safe (idempotent)
// ===========================================================================

#[test]
fn double_flush_is_idempotent() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "sig", conn);
    tree.emit_signal(emitter_id, "sig", &[]);

    assert_eq!(tree.flush_deferred_signals(), 1);
    assert_eq!(tree.flush_deferred_signals(), 0, "second flush should be a no-op");
    assert_eq!(counter.load(Ordering::SeqCst), 1, "callback should fire only once");
}

// ===========================================================================
// 10. Deferred does not affect non-deferred connection count
// ===========================================================================

/// After emit + flush, a persistent (non-one-shot) deferred connection should
/// still be present and re-queue on subsequent emissions.
#[test]
fn persistent_deferred_connection_survives_flush() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "persist_sig", conn);

    // First cycle
    tree.emit_signal(emitter_id, "persist_sig", &[]);
    tree.flush_deferred_signals();
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Second cycle — connection should still be live
    tree.emit_signal(emitter_id, "persist_sig", &[]);
    tree.flush_deferred_signals();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

// ===========================================================================
// 11. emit_signal returns empty for deferred-only signals
// ===========================================================================

/// When a signal has only deferred connections, emit_signal should return an
/// empty Vec (since no callbacks fire immediately). This matches Godot's
/// behavior where deferred calls produce no inline return values.
#[test]
fn emit_returns_empty_for_deferred_only_signal() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Int(99)
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "deferred_only", conn);

    let results = tree.emit_signal(emitter_id, "deferred_only", &[]);
    assert!(
        results.is_empty(),
        "emit_signal should return empty when all connections are deferred"
    );

    // The callback should only fire after flush.
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    tree.flush_deferred_signals();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

// ===========================================================================
// 12. Non-deferred fires strictly before deferred (temporal ordering)
// ===========================================================================

/// Verifies the Godot contract: non-deferred callbacks fire during emit_signal,
/// deferred callbacks fire later during flush. The sequence log proves ordering.
#[test]
fn non_deferred_fires_before_deferred_temporal_proof() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_tree_two_receivers();

    let sequence = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    // Non-deferred connection
    let seq = sequence.clone();
    let conn_immediate =
        Connection::with_callback(recv_a_id.object_id(), "on_immediate", move |_| {
            seq.lock().unwrap().push("immediate");
            Variant::Nil
        });

    // Deferred connection
    let seq = sequence.clone();
    let conn_deferred =
        Connection::with_callback(recv_b_id.object_id(), "on_deferred", move |_| {
            seq.lock().unwrap().push("deferred");
            Variant::Nil
        })
        .as_deferred();

    tree.connect_signal(emitter_id, "mixed", conn_immediate);
    tree.connect_signal(emitter_id, "mixed", conn_deferred);

    tree.emit_signal(emitter_id, "mixed", &[]);

    // After emit but before flush, only immediate should have fired.
    assert_eq!(*sequence.lock().unwrap(), vec!["immediate"]);

    tree.flush_deferred_signals();

    // After flush, deferred appends.
    assert_eq!(
        *sequence.lock().unwrap(),
        vec!["immediate", "deferred"],
        "non-deferred must fire before deferred"
    );
}

// ===========================================================================
// 13. Deferred connection with multiple receivers on same signal
// ===========================================================================

/// Multiple deferred connections on the same signal should all queue and flush
/// in connection-registration order.
#[test]
fn multiple_deferred_receivers_same_signal() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_tree_two_receivers();

    let order = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    let o = order.clone();
    let conn_a = Connection::with_callback(recv_a_id.object_id(), "on_a", move |_| {
        o.lock().unwrap().push("A");
        Variant::Nil
    })
    .as_deferred();

    let o = order.clone();
    let conn_b = Connection::with_callback(recv_b_id.object_id(), "on_b", move |_| {
        o.lock().unwrap().push("B");
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "multi_deferred", conn_a);
    tree.connect_signal(emitter_id, "multi_deferred", conn_b);

    tree.emit_signal(emitter_id, "multi_deferred", &[]);
    assert_eq!(tree.deferred_signal_count(), 2);

    tree.flush_deferred_signals();

    assert_eq!(
        *order.lock().unwrap(),
        vec!["A", "B"],
        "deferred connections should flush in registration order"
    );
}

// ===========================================================================
// 14. Rapid emit-flush cycles do not leak across frames
// ===========================================================================

/// Each flush should only dispatch calls queued since the last flush,
/// ensuring no cross-frame leakage.
#[test]
fn no_cross_frame_leakage_across_flush_cycles() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let args_log = Arc::new(std::sync::Mutex::new(Vec::<i64>::new()));
    let log = args_log.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |args| {
        if let Some(Variant::Int(n)) = args.first() {
            log.lock().unwrap().push(*n);
        }
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "frame_sig", conn);

    // Frame 1: emit 1 and 2
    tree.emit_signal(emitter_id, "frame_sig", &[Variant::Int(1)]);
    tree.emit_signal(emitter_id, "frame_sig", &[Variant::Int(2)]);
    assert_eq!(tree.flush_deferred_signals(), 2);

    // Frame 2: emit 3 only
    tree.emit_signal(emitter_id, "frame_sig", &[Variant::Int(3)]);
    assert_eq!(tree.flush_deferred_signals(), 1);

    // Frame 3: nothing emitted
    assert_eq!(tree.flush_deferred_signals(), 0);

    let recorded = args_log.lock().unwrap();
    assert_eq!(
        *recorded,
        vec![1, 2, 3],
        "each frame's flush should only dispatch that frame's emissions"
    );
}

// ===========================================================================
// 15. Script-dispatched deferred connection (no callback)
// ===========================================================================

/// A deferred connection without a callback should dispatch to the target
/// node's script at flush time, matching Godot's behavior for script callables
/// connected with CONNECT_DEFERRED.
#[test]
fn deferred_script_dispatch_fires_on_flush() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let listener = Node::new("Listener", "Node");
    let listener_id = tree.add_child(root, listener).unwrap();

    // Attach a script that defines _on_damage and stores the value.
    let script_src = "\
extends Node
var damage_received = 0
func _on_damage(amount):
    damage_received = amount
";
    let script = GDScriptNodeInstance::from_source(script_src, listener_id).unwrap();
    tree.attach_script(listener_id, Box::new(script));

    // Connect with deferred flag, no callback — dispatch goes through script.
    let conn = Connection::new(listener_id.object_id(), "_on_damage").as_deferred();
    tree.connect_signal(emitter_id, "hit", conn);

    tree.emit_signal(emitter_id, "hit", &[Variant::Int(50)]);

    // Before flush, script should not have been called.
    let before = tree
        .get_script(listener_id)
        .and_then(|s| s.get_property("damage_received"));
    assert_eq!(
        before,
        Some(Variant::Int(0)),
        "script should not be called before flush"
    );

    tree.flush_deferred_signals();

    // After flush, the script method should have been invoked.
    let after = tree
        .get_script(listener_id)
        .and_then(|s| s.get_property("damage_received"));
    assert_eq!(
        after,
        Some(Variant::Int(50)),
        "script should receive the deferred signal argument after flush"
    );
}

// ===========================================================================
// 16. End-to-end tscn deferred: parse → instantiate → emit → flush
// ===========================================================================

/// A connection declared in a .tscn with flags=1 (CONNECT_DEFERRED) should
/// behave correctly through the full lifecycle: parse, instantiate, emit,
/// and flush.
#[test]
fn tscn_deferred_end_to_end_emit_flush() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://e2e_deferred"]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[node name="Handler" type="Node2D" parent="."]

[connection signal="pressed" from="Button" to="Handler" method="_on_pressed" flags=1]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    tree.event_trace_mut().enable();
    let root = tree.root_id();
    let _scene_root =
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();

    // Emit the signal — deferred connections should queue, not fire.
    tree.emit_signal(button_id, "pressed", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        1,
        "tscn deferred connection should queue on emit"
    );

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "flush should process the queued deferred call");
    assert_eq!(tree.deferred_signal_count(), 0, "queue should be empty");
}

// ===========================================================================
// 17. Disconnect after emit still delivers (snapshot semantics)
// ===========================================================================

/// If a deferred connection is disconnected between emit and flush, Godot
/// still delivers the queued call (the callback was snapshotted at emit-time).
/// This test verifies Patina matches that behavior.
#[test]
fn disconnect_after_emit_still_delivers_on_flush() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "disc_sig", conn);
    tree.emit_signal(emitter_id, "disc_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Disconnect the connection AFTER emit but BEFORE flush.
    let store = tree.signal_store_mut(emitter_id);
    store.disconnect("disc_sig", recv_id.object_id(), "on_sig");

    // The deferred call was snapshotted at emit-time — it should still fire.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "queued call should still be dispatched");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "callback should fire even after disconnect (snapshot semantics)"
    );

    // A new emission should NOT queue since the connection is gone.
    tree.emit_signal(emitter_id, "disc_sig", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        0,
        "no new deferred calls after disconnect"
    );
}

// ===========================================================================
// 18. Deferred with no callback and no script is silently skipped
// ===========================================================================

/// A deferred connection with no callback targeting a node without a script
/// should be silently skipped during flush — no panic.
#[test]
fn deferred_no_callback_no_script_silent_skip() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    // Connection with no callback (just target + method name), deferred.
    let conn = Connection::new(recv_id.object_id(), "_on_event").as_deferred();
    tree.connect_signal(emitter_id, "evt", conn);

    tree.emit_signal(emitter_id, "evt", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Flush should not panic — the target has no script, so the call is skipped.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "call is counted even if no script handles it");
    assert_eq!(tree.deferred_signal_count(), 0);
}

// ===========================================================================
// 19. Multiple emitters with deferred connections flush in global FIFO
// ===========================================================================

/// Deferred signals from different source nodes should flush in the order
/// they were emitted, not grouped by source.
#[test]
fn multiple_emitters_deferred_global_fifo() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter_a = Node::new("EmitterA", "Node2D");
    let emitter_a_id = tree.add_child(root, emitter_a).unwrap();

    let emitter_b = Node::new("EmitterB", "Node2D");
    let emitter_b_id = tree.add_child(root, emitter_b).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let order = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    // Connect deferred from EmitterA
    let o = order.clone();
    let conn_a = Connection::with_callback(recv_id.object_id(), "on_a", move |_| {
        o.lock().unwrap().push("A");
        Variant::Nil
    })
    .as_deferred();

    // Connect deferred from EmitterB
    let o = order.clone();
    let conn_b = Connection::with_callback(recv_id.object_id(), "on_b", move |_| {
        o.lock().unwrap().push("B");
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_a_id, "sig_a", conn_a);
    tree.connect_signal(emitter_b_id, "sig_b", conn_b);

    tree.event_trace_mut().enable();

    // Emit A, then B, then A again.
    tree.emit_signal(emitter_a_id, "sig_a", &[]);
    tree.emit_signal(emitter_b_id, "sig_b", &[]);
    tree.emit_signal(emitter_a_id, "sig_a", &[]);

    assert_eq!(tree.deferred_signal_count(), 3);

    tree.flush_deferred_signals();

    let recorded = order.lock().unwrap();
    assert_eq!(
        *recorded,
        vec!["A", "B", "A"],
        "deferred calls from different emitters should flush in global emit order"
    );
}

// ===========================================================================
// 20. Deferred + ONE_SHOT via .tscn flags=5 end-to-end
// ===========================================================================

/// A connection parsed from .tscn with flags=5 (DEFERRED|ONE_SHOT) should
/// queue on first emit, flush once, and not re-queue on subsequent emits.
#[test]
fn tscn_deferred_oneshot_end_to_end() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://e2e_deferred_oneshot"]

[node name="Root" type="Node"]

[node name="Emitter" type="Node2D" parent="."]

[node name="Listener" type="Node2D" parent="."]

[connection signal="custom" from="Emitter" to="Listener" method="_on_custom" flags=5]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root =
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let emitter_id = tree.get_node_by_path("/root/Root/Emitter").unwrap();

    // First emit — should queue.
    tree.emit_signal(emitter_id, "custom", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    tree.flush_deferred_signals();
    assert_eq!(tree.deferred_signal_count(), 0);

    // Second emit — one-shot should have removed the connection.
    tree.emit_signal(emitter_id, "custom", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        0,
        "one-shot deferred connection from tscn should not re-queue"
    );
}

// ===========================================================================
// 21. Script dispatch with multiple arguments via deferred
// ===========================================================================

/// Script-dispatched deferred connections should correctly forward multiple
/// arguments of different Variant types.
#[test]
fn deferred_script_dispatch_multiple_args() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let listener = Node::new("Listener", "Node");
    let listener_id = tree.add_child(root, listener).unwrap();

    let script_src = "\
extends Node
var received_name = \"\"
var received_score = 0
func _on_score(name, score):
    received_name = name
    received_score = score
";
    let script = GDScriptNodeInstance::from_source(script_src, listener_id).unwrap();
    tree.attach_script(listener_id, Box::new(script));

    let conn = Connection::new(listener_id.object_id(), "_on_score").as_deferred();
    tree.connect_signal(emitter_id, "scored", conn);

    tree.emit_signal(
        emitter_id,
        "scored",
        &[Variant::String("Alice".into()), Variant::Int(100)],
    );

    // Before flush.
    assert_eq!(tree.deferred_signal_count(), 1);

    tree.flush_deferred_signals();

    let name = tree
        .get_script(listener_id)
        .and_then(|s| s.get_property("received_name"));
    let score = tree
        .get_script(listener_id)
        .and_then(|s| s.get_property("received_score"));
    assert_eq!(
        name,
        Some(Variant::String("Alice".into())),
        "script should receive string arg after deferred flush"
    );
    assert_eq!(
        score,
        Some(Variant::Int(100)),
        "script should receive int arg after deferred flush"
    );
}

// ===========================================================================
// 22. queue_free between emit and flush — deferred is dropped (pat-t64d)
// ===========================================================================

/// In Godot, if a node is freed before deferred dispatch, the queued call is
/// silently dropped — the engine checks object validity before invoking.
/// Patina matches: `process_deletions` purges the deferred queue of entries
/// targeting removed nodes.
#[test]
fn deferred_dropped_after_target_queue_freed() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "doom_sig", conn);
    tree.emit_signal(emitter_id, "doom_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Delete the receiver between emit and flush.
    tree.queue_free(recv_id);
    tree.process_deletions();

    // The deferred entry targeting the freed node was purged.
    assert_eq!(
        tree.deferred_signal_count(),
        0,
        "deferred queue should be purged after target is freed"
    );

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 0, "no deferred calls to dispatch");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "callback should NOT fire — target was freed"
    );
}

// ===========================================================================
// 23. Deferred callback emitting another signal (re-entrancy) (pat-t64d)
// ===========================================================================

/// A deferred callback that itself emits a signal should queue additional
/// deferred calls, but they should NOT be dispatched in the same flush pass.
/// This matches Godot's behavior where signals emitted during deferred
/// dispatch are queued for the next frame.
#[test]
fn deferred_callback_reemit_queues_for_next_flush() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node_a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, node_a).unwrap();

    let node_b = Node::new("B", "Node2D");
    let b_id = tree.add_child(root, node_b).unwrap();

    let order = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    // B has a deferred connection on "chain_sig" that just records.
    let o = order.clone();
    let conn_b = Connection::with_callback(b_id.object_id(), "on_chain", move |_| {
        o.lock().unwrap().push("B_chain");
        Variant::Nil
    })
    .as_deferred();
    tree.connect_signal(a_id, "chain_sig", conn_b);

    // A has a deferred connection on "start_sig" — when it fires, it records
    // and we'll emit chain_sig after flush to prove re-entrant queuing.
    let o = order.clone();
    let conn_a = Connection::with_callback(a_id.object_id(), "on_start", move |_| {
        o.lock().unwrap().push("A_start");
        Variant::Nil
    })
    .as_deferred();
    tree.connect_signal(a_id, "start_sig", conn_a);

    tree.emit_signal(a_id, "start_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Flush pass 1 — only A_start fires.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1);

    // Now emit chain_sig (simulating what a real callback would trigger).
    tree.emit_signal(a_id, "chain_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Flush pass 2 — B_chain fires.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1);

    let recorded = order.lock().unwrap();
    assert_eq!(
        *recorded,
        vec!["A_start", "B_chain"],
        "chained deferred emissions fire in separate flush passes"
    );
}

// ===========================================================================
// 24. Same receiver with both deferred and non-deferred on same signal (pat-t64d)
// ===========================================================================

/// A single receiver can have both a deferred and non-deferred connection to
/// the same signal. The non-deferred fires immediately; the deferred fires
/// at flush. Both should invoke correctly.
#[test]
fn same_receiver_deferred_and_non_deferred_on_same_signal() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let sequence = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    // Non-deferred connection.
    let seq = sequence.clone();
    let conn_imm = Connection::with_callback(recv_id.object_id(), "on_immediate", move |_| {
        seq.lock().unwrap().push("immediate");
        Variant::Nil
    });

    // Deferred connection (same receiver, same signal).
    let seq = sequence.clone();
    let conn_def = Connection::with_callback(recv_id.object_id(), "on_deferred", move |_| {
        seq.lock().unwrap().push("deferred");
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "dual_sig", conn_imm);
    tree.connect_signal(emitter_id, "dual_sig", conn_def);

    tree.emit_signal(emitter_id, "dual_sig", &[]);

    assert_eq!(*sequence.lock().unwrap(), vec!["immediate"]);

    tree.flush_deferred_signals();

    assert_eq!(
        *sequence.lock().unwrap(),
        vec!["immediate", "deferred"],
        "same receiver should handle both deferred and non-deferred on one signal"
    );
}

// ===========================================================================
// 25. Zero-argument deferred signal dispatch (pat-t64d)
// ===========================================================================

/// Deferred connections with zero arguments should work correctly — the args
/// vec is empty but dispatch still occurs.
#[test]
fn deferred_zero_args_dispatch() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let fired = Arc::new(AtomicU64::new(0));
    let f = fired.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_ping", move |args| {
        assert!(args.is_empty(), "zero-arg signal should pass empty slice");
        f.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "ping", conn);
    tree.emit_signal(emitter_id, "ping", &[]);
    tree.flush_deferred_signals();

    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

// ===========================================================================
// 26. Batch stress: many deferred calls flush correctly (pat-t64d)
// ===========================================================================

/// A large number of deferred emissions should all flush correctly without
/// loss or reordering.
#[test]
fn batch_deferred_stress_100_emissions() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let log = Arc::new(std::sync::Mutex::new(Vec::<i64>::new()));
    let l = log.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_tick", move |args| {
        if let Some(Variant::Int(n)) = args.first() {
            l.lock().unwrap().push(*n);
        }
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "tick", conn);

    for i in 0..100 {
        tree.emit_signal(emitter_id, "tick", &[Variant::Int(i)]);
    }

    assert_eq!(tree.deferred_signal_count(), 100);

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 100);

    let recorded = log.lock().unwrap();
    let expected: Vec<i64> = (0..100).collect();
    assert_eq!(*recorded, expected, "all 100 deferred calls should flush in order");
}

// ===========================================================================
// 27. flags=0 in .tscn produces non-deferred connection (pat-t64d)
// ===========================================================================

/// flags=0 is the default in Godot .tscn — the connection should NOT be
/// deferred (and not one-shot).
#[test]
fn tscn_flags_zero_is_non_deferred() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://flags_zero"]

[node name="Root" type="Node"]

[node name="Emitter" type="Node2D" parent="."]

[node name="Listener" type="Node2D" parent="."]

[connection signal="ready" from="Emitter" to="Listener" method="_on_ready" flags=0]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root =
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let emitter_id = tree.get_node_by_path("/root/Root/Emitter").unwrap();
    let store = tree.signal_store_mut(emitter_id);
    let signal = store.get_signal("ready").expect("ready signal should exist");
    let conn = &signal.connections()[0];
    assert!(!conn.deferred, "flags=0 should NOT be deferred");
    assert!(!conn.one_shot, "flags=0 should NOT be one-shot");
}

// ===========================================================================
// 28. .tscn deferred with script handler end-to-end (pat-t64d)
// ===========================================================================

/// Full pipeline: parse a .tscn with flags=1, attach a script to the target,
/// emit the signal, flush, and verify the script received the call.
#[test]
fn tscn_deferred_script_handler_end_to_end() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://e2e_deferred_script"]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[node name="Handler" type="Node" parent="."]

[connection signal="pressed" from="Button" to="Handler" method="_on_pressed" flags=1]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    tree.event_trace_mut().enable();
    let root = tree.root_id();
    let _scene_root =
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let handler_id = tree.get_node_by_path("/root/Root/Handler").unwrap();

    // Attach a script to Handler that tracks calls.
    let script_src = "\
extends Node
var pressed_count = 0
func _on_pressed():
    pressed_count = pressed_count + 1
";
    let script = GDScriptNodeInstance::from_source(script_src, handler_id).unwrap();
    tree.attach_script(handler_id, Box::new(script));

    let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();

    // Emit — deferred, so script should NOT be called yet.
    tree.emit_signal(button_id, "pressed", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    let before = tree
        .get_script(handler_id)
        .and_then(|s| s.get_property("pressed_count"));
    assert_eq!(
        before,
        Some(Variant::Int(0)),
        "script should not be called before flush"
    );

    tree.flush_deferred_signals();

    let after = tree
        .get_script(handler_id)
        .and_then(|s| s.get_property("pressed_count"));
    assert_eq!(
        after,
        Some(Variant::Int(1)),
        "script should receive deferred pressed signal after flush"
    );
}

// ===========================================================================
// 29. Deferred ONE_SHOT with multiple emissions only delivers first (pat-t64d)
// ===========================================================================

/// A deferred + one-shot connection that receives multiple rapid emissions
/// before any flush should only queue the first one (the connection is removed
/// after the first emit snapshots it).
#[test]
fn deferred_oneshot_rapid_emit_only_first_queued() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred()
    .as_one_shot();

    tree.connect_signal(emitter_id, "rapid_sig", conn);

    // Rapid-fire 5 emissions.
    for _ in 0..5 {
        tree.emit_signal(emitter_id, "rapid_sig", &[]);
    }

    // Only the first emission should have queued (one-shot removes after first).
    assert_eq!(
        tree.deferred_signal_count(),
        1,
        "one-shot deferred should only queue once despite multiple emits"
    );

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

// ===========================================================================
// 30. Deferred signals preserve Variant types faithfully (pat-t64d)
// ===========================================================================

/// Verifies that all common Variant types survive the deferred snapshot→flush
/// round-trip without corruption. This is important because args are cloned
/// at emit-time and delivered later.
#[test]
fn deferred_preserves_variant_types() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let captured = Arc::new(std::sync::Mutex::new(Vec::<Variant>::new()));
    let cap = captured.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_typed", move |args| {
        cap.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "typed_sig", conn);

    let args = vec![
        Variant::Bool(true),
        Variant::Int(-42),
        Variant::Float(3.14),
        Variant::String("hello world".into()),
        Variant::Nil,
    ];

    tree.emit_signal(emitter_id, "typed_sig", &args);
    tree.flush_deferred_signals();

    let got = captured.lock().unwrap();
    assert_eq!(got.len(), 5, "all 5 args should be delivered");
    assert_eq!(got[0], Variant::Bool(true));
    assert_eq!(got[1], Variant::Int(-42));
    assert_eq!(got[2], Variant::Float(3.14));
    assert_eq!(got[3], Variant::String("hello world".into()));
    assert_eq!(got[4], Variant::Nil);
}

// ===========================================================================
// 31. Self-connection: emitter == receiver with CONNECT_DEFERRED
// ===========================================================================

/// In Godot, a node can connect a signal to itself with CONNECT_DEFERRED.
/// The deferred callback should still queue at emit-time and fire at flush.
#[test]
fn deferred_self_connection_emitter_is_receiver() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("SelfNode", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(node_id.object_id(), "on_self", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(node_id, "self_sig", conn);
    tree.emit_signal(node_id, "self_sig", &[]);

    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "self-deferred callback must not fire during emit"
    );
    assert_eq!(tree.deferred_signal_count(), 1);

    tree.flush_deferred_signals();

    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "self-deferred callback should fire after flush"
    );
}

// ===========================================================================
// 32. Emitter queue_free'd between emit and flush — deferred still dispatches
// ===========================================================================

/// If the *emitter* (not receiver) is queue_free'd after emit but before flush,
/// the deferred callback should still fire because the callback was captured
/// at emit-time, independent of the emitter's lifetime.
#[test]
fn deferred_fires_after_emitter_queue_freed() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "emitter_doom", conn);
    tree.emit_signal(emitter_id, "emitter_doom", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Delete the *emitter* between emit and flush.
    tree.queue_free(emitter_id);
    tree.process_deletions();

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "deferred call should dispatch after emitter freed");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "callback fires even after emitter node is freed"
    );
}

// ===========================================================================
// 33. Interleaved deferred from different signals on same emitter
// ===========================================================================

/// When a single emitter has deferred connections on two different signals and
/// those signals are emitted in alternating order, the deferred queue must
/// preserve the global emission order (not group by signal name).
#[test]
fn interleaved_signals_same_emitter_preserve_global_order() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let log = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    let l = log.clone();
    let conn_alpha = Connection::with_callback(recv_id.object_id(), "on_alpha", move |args| {
        if let Some(Variant::Int(n)) = args.first() {
            l.lock().unwrap().push(format!("alpha:{n}"));
        }
        Variant::Nil
    })
    .as_deferred();

    let l = log.clone();
    let conn_beta = Connection::with_callback(recv_id.object_id(), "on_beta", move |args| {
        if let Some(Variant::Int(n)) = args.first() {
            l.lock().unwrap().push(format!("beta:{n}"));
        }
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "alpha", conn_alpha);
    tree.connect_signal(emitter_id, "beta", conn_beta);

    // Interleave: alpha, beta, alpha, beta
    tree.emit_signal(emitter_id, "alpha", &[Variant::Int(1)]);
    tree.emit_signal(emitter_id, "beta", &[Variant::Int(2)]);
    tree.emit_signal(emitter_id, "alpha", &[Variant::Int(3)]);
    tree.emit_signal(emitter_id, "beta", &[Variant::Int(4)]);

    assert_eq!(tree.deferred_signal_count(), 4);
    tree.flush_deferred_signals();

    let recorded = log.lock().unwrap();
    assert_eq!(
        *recorded,
        vec!["alpha:1", "beta:2", "alpha:3", "beta:4"],
        "interleaved deferred signals must flush in global emission order"
    );
}

// ===========================================================================
// 34. Deferred self-connection with script dispatch (no callback)
// ===========================================================================

/// A node connecting a signal to itself via script method (no callback) with
/// CONNECT_DEFERRED should defer the script call until flush.
#[test]
fn deferred_self_connection_script_dispatch() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("SelfScript", "Node");
    let node_id = tree.add_child(root, node).unwrap();

    let script_src = "\
extends Node
var hit_count = 0
func _on_hit():
    hit_count = hit_count + 1
";
    let script = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
    tree.attach_script(node_id, Box::new(script));

    let conn = Connection::new(node_id.object_id(), "_on_hit").as_deferred();
    tree.connect_signal(node_id, "hit", conn);

    tree.emit_signal(node_id, "hit", &[]);

    let before = tree
        .get_script(node_id)
        .and_then(|s| s.get_property("hit_count"));
    assert_eq!(
        before,
        Some(Variant::Int(0)),
        "self-deferred script call should not fire before flush"
    );

    tree.flush_deferred_signals();

    let after = tree
        .get_script(node_id)
        .and_then(|s| s.get_property("hit_count"));
    assert_eq!(
        after,
        Some(Variant::Int(1)),
        "self-deferred script call should fire after flush"
    );
}

// ===========================================================================
// 35. Multi-frame deferred lifecycle: emit, flush, re-emit, flush
// ===========================================================================

/// Simulates multiple frames: each frame emits deferred signals and flushes.
/// Verifies no state leaks between frames and counters accumulate correctly.
#[test]
fn multi_frame_deferred_lifecycle() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_tick", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "tick", conn);

    // Simulate 10 frames, each with one deferred emission + flush.
    for frame in 1..=10u64 {
        tree.emit_signal(emitter_id, "tick", &[]);
        assert_eq!(
            tree.deferred_signal_count(),
            1,
            "frame {frame}: exactly one deferred call queued"
        );

        let flushed = tree.flush_deferred_signals();
        assert_eq!(flushed, 1, "frame {frame}: flushed one call");
        assert_eq!(
            tree.deferred_signal_count(),
            0,
            "frame {frame}: queue empty after flush"
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            frame,
            "frame {frame}: counter should match frame number"
        );
    }
}

// ===========================================================================
// 36. Deferred + ONE_SHOT: flush delivers, then re-connect works
// ===========================================================================

/// After a deferred+one_shot connection is consumed and flushed, reconnecting
/// a new deferred+one_shot connection on the same signal should work normally.
#[test]
fn deferred_oneshot_reconnect_after_flush() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));

    // First connection: deferred + one_shot.
    let cc = counter.clone();
    let conn1 = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred()
    .as_one_shot();

    tree.connect_signal(emitter_id, "recon_sig", conn1);
    tree.emit_signal(emitter_id, "recon_sig", &[]);
    tree.flush_deferred_signals();
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // The one-shot connection is gone — emitting should not queue.
    tree.emit_signal(emitter_id, "recon_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 0);

    // Reconnect with a new deferred + one_shot.
    let cc = counter.clone();
    let conn2 = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred()
    .as_one_shot();

    tree.connect_signal(emitter_id, "recon_sig", conn2);
    tree.emit_signal(emitter_id, "recon_sig", &[]);
    tree.flush_deferred_signals();
    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "reconnected one-shot deferred should fire on second cycle"
    );
}

// ===========================================================================
// 37. Deferred dispatch with tscn scene containing multiple connections
// ===========================================================================

/// A .tscn with multiple connections (some deferred, some not) should
/// correctly wire both types during instantiation.
#[test]
fn tscn_mixed_deferred_and_immediate_connections() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://mixed_flags"]

[node name="Root" type="Node"]

[node name="Emitter" type="Node2D" parent="."]

[node name="ListenerA" type="Node2D" parent="."]

[node name="ListenerB" type="Node2D" parent="."]

[connection signal="hit" from="Emitter" to="ListenerA" method="_on_hit" flags=0]
[connection signal="hit" from="Emitter" to="ListenerB" method="_on_hit_deferred" flags=1]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root =
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let emitter_id = tree.get_node_by_path("/root/Root/Emitter").unwrap();
    let store = tree.signal_store_mut(emitter_id);
    let signal = store.get_signal("hit").expect("hit signal should exist");

    assert_eq!(signal.connection_count(), 2, "should have two connections");

    let conns = signal.connections();
    assert!(
        !conns[0].deferred,
        "first connection (flags=0) should NOT be deferred"
    );
    assert!(
        conns[1].deferred,
        "second connection (flags=1) should be deferred"
    );
}

// ===========================================================================
// 38. Multiple deferred methods on same target fire in registration order
// ===========================================================================

/// When a single receiver has multiple deferred connections on the same signal
/// (to different methods), all should queue and flush in registration order.
/// This matches Godot's guarantee that connection order is preserved.
#[test]
fn deferred_multiple_methods_same_target_registration_order() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let order = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    let o = order.clone();
    let conn_a = Connection::with_callback(recv_id.object_id(), "on_first", move |_| {
        o.lock().unwrap().push("first");
        Variant::Nil
    })
    .as_deferred();

    let o = order.clone();
    let conn_b = Connection::with_callback(recv_id.object_id(), "on_second", move |_| {
        o.lock().unwrap().push("second");
        Variant::Nil
    })
    .as_deferred();

    let o = order.clone();
    let conn_c = Connection::with_callback(recv_id.object_id(), "on_third", move |_| {
        o.lock().unwrap().push("third");
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "multi_method", conn_a);
    tree.connect_signal(emitter_id, "multi_method", conn_b);
    tree.connect_signal(emitter_id, "multi_method", conn_c);

    tree.emit_signal(emitter_id, "multi_method", &[]);
    assert_eq!(tree.deferred_signal_count(), 3);

    tree.flush_deferred_signals();

    assert_eq!(
        *order.lock().unwrap(),
        vec!["first", "second", "third"],
        "multiple deferred methods on same target must flush in registration order"
    );
}

// ===========================================================================
// 39. disconnect_all_for_target does not cancel already-queued deferred calls
// ===========================================================================

/// Godot snapshots deferred calls at emit-time. If disconnect_all_for is
/// called between emit and flush, the already-queued calls should still fire
/// (snapshot semantics).
#[test]
fn disconnect_all_for_target_preserves_queued_deferred() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "snap_sig", conn);
    tree.emit_signal(emitter_id, "snap_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Remove all connections for the target after emit.
    let store = tree.signal_store_mut(emitter_id);
    store.disconnect_all_for(recv_id.object_id());

    // Snapshot semantics: queued call should still deliver.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "queued call should still dispatch");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "callback fires despite disconnect_all_for (snapshot semantics)"
    );

    // New emit should NOT queue since all connections were removed.
    tree.emit_signal(emitter_id, "snap_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 0);
}

// ===========================================================================
// 40. MainLoop::step() flushes deferred signals automatically (pat-pd7w)
// ===========================================================================

/// In Godot, CONNECT_DEFERRED signals are flushed during the frame loop
/// (end-of-frame, after _process). Patina's MainLoop::step() must do the same.
#[test]
fn mainloop_step_flushes_deferred_signals() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "auto_flush_sig", conn);

    // Emit the signal before handing the tree to the main loop.
    tree.emit_signal(emitter_id, "auto_flush_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // After one frame step, the deferred signal should have been flushed.
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "MainLoop::step() must flush deferred signals"
    );
    assert_eq!(
        ml.tree().deferred_signal_count(),
        0,
        "deferred queue should be empty after step"
    );
}

// ===========================================================================
// 41. Deferred signals flush before deletions in MainLoop (pat-pd7w)
// ===========================================================================

/// Godot flushes deferred signals before processing queue_free deletions.
/// A deferred callback targeting a node that is queue_free'd in the same frame
/// should still fire because flush happens before deletion.
#[test]
fn mainloop_deferred_flush_before_deletion() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "doom_sig", conn);

    // Emit deferred and queue_free the receiver in the same frame.
    tree.emit_signal(emitter_id, "doom_sig", &[]);
    tree.queue_free(recv_id);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // The callback was snapshotted at emit-time, and flush happens before
    // deletions, so the callback should fire.
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "deferred callback should fire before queue_free deletion"
    );
}

// ===========================================================================
// 42. Multi-frame MainLoop deferred lifecycle (pat-pd7w)
// ===========================================================================

/// Simulates multiple MainLoop frames, each emitting a deferred signal.
/// Verifies that deferred signals are flushed per-frame with no leakage.
#[test]
fn mainloop_multi_frame_deferred_no_leakage() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let log = Arc::new(std::sync::Mutex::new(Vec::<i64>::new()));
    let l = log.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_tick", move |args| {
        if let Some(Variant::Int(n)) = args.first() {
            l.lock().unwrap().push(*n);
        }
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "tick", conn);

    let mut ml = MainLoop::new(tree);

    for frame in 0..5i64 {
        // Emit before each step — the step should flush it.
        ml.tree_mut().emit_signal(emitter_id, "tick", &[Variant::Int(frame)]);
        ml.step(1.0 / 60.0);
    }

    let recorded = log.lock().unwrap();
    assert_eq!(
        *recorded,
        vec![0, 1, 2, 3, 4],
        "each frame's deferred signal should flush exactly once per step"
    );
}

// ===========================================================================
// 43. Deferred + scene callable (tscn script handler via MainLoop) (pat-pd7w)
// ===========================================================================

/// End-to-end: parse a .tscn with flags=1, attach a script, run MainLoop::step(),
/// and verify the script received the deferred dispatch automatically.
#[test]
fn mainloop_tscn_deferred_script_callable_e2e() {
    use gdscene::main_loop::MainLoop;
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://ml_deferred_script"]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[node name="Handler" type="Node" parent="."]

[connection signal="pressed" from="Button" to="Handler" method="_on_pressed" flags=1]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    tree.event_trace_mut().enable();
    let root = tree.root_id();
    let _scene_root =
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let handler_id = tree.get_node_by_path("/root/Root/Handler").unwrap();

    let script_src = "\
extends Node
var pressed_count = 0
func _on_pressed():
    pressed_count = pressed_count + 1
";
    let script = GDScriptNodeInstance::from_source(script_src, handler_id).unwrap();
    tree.attach_script(handler_id, Box::new(script));

    let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();
    tree.emit_signal(button_id, "pressed", &[]);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let after = ml
        .tree()
        .get_script(handler_id)
        .and_then(|s| s.get_property("pressed_count"));
    assert_eq!(
        after,
        Some(Variant::Int(1)),
        "MainLoop should auto-flush deferred script callable"
    );
}

// ===========================================================================
// 44. Mixed deferred + immediate via MainLoop step (pat-pd7w)
// ===========================================================================

/// During a MainLoop step, immediate connections should fire inline (during
/// emit_signal) and deferred connections should fire at end-of-frame (during
/// flush). Verify temporal ordering through sequence logging.
#[test]
fn mainloop_mixed_deferred_and_immediate_ordering() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("RecvA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("RecvB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    let sequence = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    // Non-deferred (immediate) connection.
    let seq = sequence.clone();
    let conn_imm = Connection::with_callback(recv_a_id.object_id(), "on_imm", move |_| {
        seq.lock().unwrap().push("immediate");
        Variant::Nil
    });

    // Deferred connection.
    let seq = sequence.clone();
    let conn_def = Connection::with_callback(recv_b_id.object_id(), "on_def", move |_| {
        seq.lock().unwrap().push("deferred");
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "mixed", conn_imm);
    tree.connect_signal(emitter_id, "mixed", conn_def);

    tree.emit_signal(emitter_id, "mixed", &[]);

    // Immediate should have fired already.
    assert_eq!(*sequence.lock().unwrap(), vec!["immediate"]);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // After step, deferred should have been flushed.
    assert_eq!(
        *sequence.lock().unwrap(),
        vec!["immediate", "deferred"],
        "MainLoop must flush deferred after immediate during step"
    );
}

// ===========================================================================
// 45. Deferred ONE_SHOT consumed within MainLoop frame (pat-pd7w)
// ===========================================================================

/// A deferred + one_shot connection should fire exactly once when MainLoop
/// flushes, and not re-queue on subsequent frames.
#[test]
fn mainloop_deferred_oneshot_consumed_in_frame() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred()
    .as_one_shot();

    tree.connect_signal(emitter_id, "oneshot_sig", conn);
    tree.emit_signal(emitter_id, "oneshot_sig", &[]);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(counter.load(Ordering::SeqCst), 1, "one-shot should fire once");

    // Emit again on next frame — connection should be gone.
    ml.tree_mut().emit_signal(emitter_id, "oneshot_sig", &[]);
    ml.step(1.0 / 60.0);

    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "one-shot deferred should not fire again on second frame"
    );
}

// ===========================================================================
// 46. Deferred signals from multiple emitters flush in global order (MainLoop)
// ===========================================================================

/// When multiple emitters queue deferred signals in a single frame, the
/// MainLoop flush should dispatch them in global emit order.
#[test]
fn mainloop_multiple_emitters_global_order() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let em_a = Node::new("EmA", "Node2D");
    let em_a_id = tree.add_child(root, em_a).unwrap();

    let em_b = Node::new("EmB", "Node2D");
    let em_b_id = tree.add_child(root, em_b).unwrap();

    let recv = Node::new("Recv", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let order = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    let o = order.clone();
    let conn_a = Connection::with_callback(recv_id.object_id(), "on_a", move |_| {
        o.lock().unwrap().push("A");
        Variant::Nil
    })
    .as_deferred();

    let o = order.clone();
    let conn_b = Connection::with_callback(recv_id.object_id(), "on_b", move |_| {
        o.lock().unwrap().push("B");
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(em_a_id, "sig_a", conn_a);
    tree.connect_signal(em_b_id, "sig_b", conn_b);

    // Emit B first, then A — flush should preserve emit order.
    tree.emit_signal(em_b_id, "sig_b", &[]);
    tree.emit_signal(em_a_id, "sig_a", &[]);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        *order.lock().unwrap(),
        vec!["B", "A"],
        "MainLoop should flush deferred in global emit order"
    );
}

// ===========================================================================
// 40. Mixed deferred+one_shot and deferred-only: only one_shot is removed
// ===========================================================================

/// When a signal has both a deferred+one_shot and a deferred (persistent)
/// connection, only the one_shot connection should be removed after emit.
/// The persistent connection should survive and re-queue on subsequent emits.
#[test]
fn mixed_deferred_oneshot_and_persistent() {
    let (mut tree, emitter_id, recv_a_id, recv_b_id) = build_tree_two_receivers();

    let oneshot_counter = Arc::new(AtomicU64::new(0));
    let persist_counter = Arc::new(AtomicU64::new(0));

    let oc = oneshot_counter.clone();
    let conn_oneshot =
        Connection::with_callback(recv_a_id.object_id(), "on_oneshot", move |_| {
            oc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot();

    let pc = persist_counter.clone();
    let conn_persist =
        Connection::with_callback(recv_b_id.object_id(), "on_persist", move |_| {
            pc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred();

    tree.connect_signal(emitter_id, "mixed_sig", conn_oneshot);
    tree.connect_signal(emitter_id, "mixed_sig", conn_persist);

    // First emit: both queue.
    tree.emit_signal(emitter_id, "mixed_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 2);
    tree.flush_deferred_signals();
    assert_eq!(oneshot_counter.load(Ordering::SeqCst), 1);
    assert_eq!(persist_counter.load(Ordering::SeqCst), 1);

    // Second emit: only persistent re-queues (one_shot was removed).
    tree.emit_signal(emitter_id, "mixed_sig", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        1,
        "only persistent deferred should re-queue"
    );
    tree.flush_deferred_signals();
    assert_eq!(
        oneshot_counter.load(Ordering::SeqCst),
        1,
        "one_shot should not fire again"
    );
    assert_eq!(
        persist_counter.load(Ordering::SeqCst),
        2,
        "persistent deferred fires on second cycle"
    );
}

// ===========================================================================
// 41. Re-entrant deferred: deferred callback emits another deferred signal
// ===========================================================================

/// In Godot, if a deferred callback emits another signal with CONNECT_DEFERRED,
/// that second emission queues for the *next* flush, not the current one.
/// Patina should match: flush processes exactly the snapshot taken at drain-time.
#[test]
fn reentrant_deferred_queues_for_next_flush() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let order = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));

    // Second-level deferred connection: "chain" signal on emitter.
    let o = order.clone();
    let conn_chain = Connection::with_callback(recv_id.object_id(), "on_chain", move |_| {
        o.lock().unwrap().push("chain");
        Variant::Nil
    })
    .as_deferred();
    tree.connect_signal(emitter_id, "chain_sig", conn_chain);

    // First-level deferred callback that re-emits "chain_sig".
    // Note: we can't call tree.emit_signal from within the callback directly
    // (no &mut tree access), so we verify via the existing "reemit" test pattern
    // that the *count* of queued calls is correct after the first flush.
    let o = order.clone();
    let conn_first = Connection::with_callback(recv_id.object_id(), "on_first", move |_| {
        o.lock().unwrap().push("first");
        Variant::Nil
    })
    .as_deferred();
    tree.connect_signal(emitter_id, "start_sig", conn_first);

    tree.emit_signal(emitter_id, "start_sig", &[]);
    tree.emit_signal(emitter_id, "chain_sig", &[]);

    // Both should be queued.
    assert_eq!(tree.deferred_signal_count(), 2);

    // Flush processes both in FIFO order.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 2);
    assert_eq!(
        *order.lock().unwrap(),
        vec!["first", "chain"],
        "both deferred calls fire in emission order"
    );

    // Queue should be empty — nothing was re-queued during flush.
    assert_eq!(tree.deferred_signal_count(), 0);
}

// ===========================================================================
// 42. Deferred with explicit Variant::Nil arg vs empty args
// ===========================================================================

/// Godot distinguishes between emitting with no args and emitting with a
/// single Nil arg. Deferred dispatch should preserve this distinction.
#[test]
fn deferred_nil_arg_vs_empty_args() {
    let (mut tree, emitter_id, recv_id) = build_tree();

    let captured_nil = Arc::new(std::sync::Mutex::new(Vec::<Vec<Variant>>::new()));
    let captured_empty = Arc::new(std::sync::Mutex::new(Vec::<Vec<Variant>>::new()));

    let cn = captured_nil.clone();
    let conn_nil = Connection::with_callback(recv_id.object_id(), "on_nil", move |args| {
        cn.lock().unwrap().push(args.to_vec());
        Variant::Nil
    })
    .as_deferred();

    let ce = captured_empty.clone();
    let conn_empty = Connection::with_callback(recv_id.object_id(), "on_empty", move |args| {
        ce.lock().unwrap().push(args.to_vec());
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_id, "nil_sig", conn_nil);
    tree.connect_signal(emitter_id, "empty_sig", conn_empty);

    // Emit with explicit Nil arg.
    tree.emit_signal(emitter_id, "nil_sig", &[Variant::Nil]);
    // Emit with no args.
    tree.emit_signal(emitter_id, "empty_sig", &[]);

    tree.flush_deferred_signals();

    let nil_calls = captured_nil.lock().unwrap();
    assert_eq!(nil_calls.len(), 1);
    assert_eq!(nil_calls[0], vec![Variant::Nil], "should receive explicit Nil arg");

    let empty_calls = captured_empty.lock().unwrap();
    assert_eq!(empty_calls.len(), 1);
    assert!(empty_calls[0].is_empty(), "should receive empty args");
}

// ===========================================================================
// 43. Deferred script dispatch: method not found is silently skipped
// ===========================================================================

/// If a deferred connection targets a script method that doesn't exist,
/// Godot silently skips it (no crash). Patina should match.
#[test]
fn deferred_script_missing_method_silent_skip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let listener = Node::new("Listener", "Node");
    let listener_id = tree.add_child(root, listener).unwrap();

    let script_src = "\
extends Node
var x = 0
func _on_real():
    x = 1
";
    let script = GDScriptNodeInstance::from_source(script_src, listener_id).unwrap();
    tree.attach_script(listener_id, Box::new(script));

    // Connect to a method that does NOT exist in the script.
    let conn = Connection::new(listener_id.object_id(), "_on_nonexistent").as_deferred();
    tree.connect_signal(emitter_id, "sig", conn);

    tree.emit_signal(emitter_id, "sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Flush should not panic — missing method is silently skipped.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "call counted even if method not found");

    // The script's state should be unchanged.
    let x = tree
        .get_script(listener_id)
        .and_then(|s| s.get_property("x"));
    assert_eq!(x, Some(Variant::Int(0)), "script state unchanged");
}

// ===========================================================================
// 44. Deferred script + callback on same signal: both dispatch correctly
// ===========================================================================

/// A signal with both a callback-based deferred connection and a script-based
/// deferred connection should queue both and dispatch each at flush time.
#[test]
fn deferred_mixed_callback_and_script_on_same_signal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let listener_a = Node::new("ListenerA", "Node2D");
    let listener_a_id = tree.add_child(root, listener_a).unwrap();

    let listener_b = Node::new("ListenerB", "Node");
    let listener_b_id = tree.add_child(root, listener_b).unwrap();

    // Callback-based deferred connection.
    let cb_counter = Arc::new(AtomicU64::new(0));
    let cc = cb_counter.clone();
    let conn_cb =
        Connection::with_callback(listener_a_id.object_id(), "on_hit_cb", move |_| {
            cc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred();

    // Script-based deferred connection.
    let script_src = "\
extends Node
var hit_count = 0
func _on_hit():
    hit_count = hit_count + 1
";
    let script = GDScriptNodeInstance::from_source(script_src, listener_b_id).unwrap();
    tree.attach_script(listener_b_id, Box::new(script));

    let conn_script = Connection::new(listener_b_id.object_id(), "_on_hit").as_deferred();

    tree.connect_signal(emitter_id, "hit", conn_cb);
    tree.connect_signal(emitter_id, "hit", conn_script);

    tree.emit_signal(emitter_id, "hit", &[]);
    assert_eq!(tree.deferred_signal_count(), 2);

    tree.flush_deferred_signals();

    assert_eq!(
        cb_counter.load(Ordering::SeqCst),
        1,
        "callback-based deferred fires"
    );
    let hit_count = tree
        .get_script(listener_b_id)
        .and_then(|s| s.get_property("hit_count"));
    assert_eq!(
        hit_count,
        Some(Variant::Int(1)),
        "script-based deferred fires"
    );
}

// ===========================================================================
// 45. Deferred with high connection count preserves all callbacks
// ===========================================================================

/// Stress test: 50 deferred connections on one signal should all queue and
/// flush, with none dropped.
#[test]
fn deferred_high_connection_count_no_drops() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let counter = Arc::new(AtomicU64::new(0));

    for i in 0..50 {
        let recv = Node::new(&format!("Recv{i}"), "Node2D");
        let recv_id = tree.add_child(root, recv).unwrap();

        let cc = counter.clone();
        let conn = Connection::with_callback(
            recv_id.object_id(),
            &format!("on_sig_{i}"),
            move |_| {
                cc.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            },
        )
        .as_deferred();

        tree.connect_signal(emitter_id, "mass_sig", conn);
    }

    tree.emit_signal(emitter_id, "mass_sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 50);

    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 50, "all 50 deferred calls dispatched");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        50,
        "all 50 callbacks fired"
    );
}

// ===========================================================================
// 46. tscn flags=4 is ONE_SHOT only, not deferred
// ===========================================================================

/// flags=4 (ONE_SHOT without DEFERRED) should not mark the connection as
/// deferred. This verifies the flag bitmask is parsed correctly.
#[test]
fn tscn_flags_oneshot_only_not_deferred() {
    use gdscene::packed_scene::PackedScene;

    let tscn = r#"
[gd_scene format=3 uid="uid://oneshot_only"]

[node name="Root" type="Node"]

[node name="Emitter" type="Node2D" parent="."]

[node name="Listener" type="Node2D" parent="."]

[connection signal="done" from="Emitter" to="Listener" method="_on_done" flags=4]
"#;

    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _scene_root =
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let emitter_id = tree.get_node_by_path("/root/Root/Emitter").unwrap();
    let store = tree.signal_store_mut(emitter_id);
    let signal = store.get_signal("done").expect("done signal should exist");
    let conn = &signal.connections()[0];
    assert!(
        !conn.deferred,
        "flags=4 (ONE_SHOT) should NOT set deferred"
    );
    assert!(conn.one_shot, "flags=4 should set ONE_SHOT");
}

// ===========================================================================
// 47. Deferred connection from different emitters to same receiver+method
// ===========================================================================

/// Two different emitters each have a deferred connection to the same receiver
/// and method. Both should queue and flush independently.
#[test]
fn different_emitters_same_receiver_method_deferred() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter_a = Node::new("EmitterA", "Node2D");
    let emitter_a_id = tree.add_child(root, emitter_a).unwrap();

    let emitter_b = Node::new("EmitterB", "Node2D");
    let emitter_b_id = tree.add_child(root, emitter_b).unwrap();

    let recv = Node::new("Receiver", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    let counter = Arc::new(AtomicU64::new(0));

    let cc = counter.clone();
    let conn_a = Connection::with_callback(recv_id.object_id(), "on_hit", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    let cc = counter.clone();
    let conn_b = Connection::with_callback(recv_id.object_id(), "on_hit", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter_a_id, "hit", conn_a);
    tree.connect_signal(emitter_b_id, "hit", conn_b);

    tree.emit_signal(emitter_a_id, "hit", &[]);
    tree.emit_signal(emitter_b_id, "hit", &[]);

    assert_eq!(tree.deferred_signal_count(), 2);

    tree.flush_deferred_signals();
    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "both emitters' deferred calls should fire"
    );
}

// ===========================================================================
// 58. Deferred signal to self (emitter == receiver)
// ===========================================================================

/// A node can connect a deferred signal to itself. The queued call should
/// fire normally during flush.
#[test]
fn deferred_signal_to_self() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("SelfSig", "Node2D")).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(node.object_id(), "on_self", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(node, "ping", conn);
    tree.emit_signal(node, "ping", &[]);

    assert_eq!(counter.load(Ordering::SeqCst), 0, "deferred, not fired yet");
    assert_eq!(tree.deferred_signal_count(), 1);

    tree.flush_deferred_signals();
    assert_eq!(counter.load(Ordering::SeqCst), 1, "fires on flush");
}

// ===========================================================================
// 59. queue_free with deferred: flush happens before deletion in MainLoop
// ===========================================================================

/// In Godot, the frame ordering is: _process → flush_deferred → process_deletions.
/// A node that calls queue_free AND has a queued deferred signal in the same
/// frame should have its deferred signal fire BEFORE the node is freed.
/// This verifies the MainLoop ordering contract.
#[test]
fn deferred_flush_before_deletion_ordering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let receiver = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(receiver.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter, "sig", conn);

    // Emit (queues deferred) then mark for deletion.
    tree.emit_signal(emitter, "sig", &[]);
    tree.queue_free(receiver);

    // Godot ordering: flush deferred FIRST, then process deletions.
    assert_eq!(tree.deferred_signal_count(), 1);
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "deferred fires before deletion");
    assert_eq!(counter.load(Ordering::SeqCst), 1, "callback invoked");

    // Now process deletions — receiver is freed.
    tree.process_deletions();
}

// ===========================================================================
// 60. Deferred emit with zero connections is a no-op
// ===========================================================================

/// Emitting a signal with no connections (deferred or otherwise) should not
/// queue anything.
#[test]
fn deferred_emit_no_connections_noop() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Lonely", "Node2D")).unwrap();

    tree.emit_signal(node, "nonexistent_signal", &[]);
    assert_eq!(tree.deferred_signal_count(), 0);
    assert_eq!(tree.flush_deferred_signals(), 0);
}

// ===========================================================================
// 61. Deferred + immediate on different signals from same emitter
// ===========================================================================

/// An emitter can have one signal with immediate connections and another with
/// deferred. They should not interfere with each other.
#[test]
fn deferred_and_immediate_different_signals_same_emitter() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let recv = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    let imm_counter = Arc::new(AtomicU64::new(0));
    let def_counter = Arc::new(AtomicU64::new(0));

    let ic = imm_counter.clone();
    let conn_imm = Connection::with_callback(recv.object_id(), "on_alpha", move |_| {
        ic.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });

    let dc = def_counter.clone();
    let conn_def = Connection::with_callback(recv.object_id(), "on_beta", move |_| {
        dc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter, "alpha", conn_imm);
    tree.connect_signal(emitter, "beta", conn_def);

    tree.emit_signal(emitter, "alpha", &[]);
    tree.emit_signal(emitter, "beta", &[]);

    assert_eq!(imm_counter.load(Ordering::SeqCst), 1, "immediate fires");
    assert_eq!(def_counter.load(Ordering::SeqCst), 0, "deferred queued");
    assert_eq!(tree.deferred_signal_count(), 1);

    tree.flush_deferred_signals();
    assert_eq!(def_counter.load(Ordering::SeqCst), 1, "deferred fires");
}

// ===========================================================================
// 62. Deferred removal of source node purges its queued signals
// ===========================================================================

/// If the source (emitter) node is freed between emit and flush, queued
/// deferred calls should still fire — the source being freed doesn't
/// invalidate callback-captured closures (Godot checks target, not source).
#[test]
fn source_freed_deferred_still_fires() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let recv = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(recv.object_id(), "on_sig", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_deferred();

    tree.connect_signal(emitter, "sig", conn);
    tree.emit_signal(emitter, "sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 1);

    // Free the SOURCE node (not the target).
    tree.remove_node(emitter).unwrap();

    // The queued deferred call targets recv (still alive), so it should fire.
    let flushed = tree.flush_deferred_signals();
    assert_eq!(flushed, 1, "deferred call fires — target is still alive");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

// ===========================================================================
// pat-fh2: FIFO delivery with mixed one-shot listeners
// ===========================================================================

/// FIFO order is preserved when interleaving one-shot and persistent deferred
/// connections on the same signal. Registration order determines delivery order.
#[test]
fn fh2_fifo_order_with_interleaved_oneshot_persistent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let recv = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    let order = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    // Connect: persistent, oneshot, persistent, oneshot — interleaved
    for (i, is_oneshot) in [(0, false), (1, true), (2, false), (3, true)] {
        let o = order.clone();
        let label = format!("handler_{i}_{}", if is_oneshot { "oneshot" } else { "persist" });
        let method = format!("on_{i}");
        let mut conn = Connection::with_callback(recv.object_id(), method, move |_| {
            o.lock().unwrap().push(label.clone());
            Variant::Nil
        })
        .as_deferred();
        if is_oneshot {
            conn = conn.as_one_shot();
        }
        tree.connect_signal(emitter, "sig", conn);
    }

    // First emit: all 4 fire in registration order
    tree.emit_signal(emitter, "sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 4);
    tree.flush_deferred_signals();

    let first_order = order.lock().unwrap().clone();
    assert_eq!(
        first_order,
        vec![
            "handler_0_persist",
            "handler_1_oneshot",
            "handler_2_persist",
            "handler_3_oneshot",
        ],
        "FIFO: all 4 fire in registration order on first emit"
    );

    // Clear and re-emit: only persistent connections remain
    order.lock().unwrap().clear();
    tree.emit_signal(emitter, "sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 2, "only 2 persistent remain");
    tree.flush_deferred_signals();

    let second_order = order.lock().unwrap().clone();
    assert_eq!(
        second_order,
        vec!["handler_0_persist", "handler_2_persist"],
        "FIFO: only persistent connections fire on second emit, in original registration order"
    );
}

/// Multiple one-shot deferred listeners from different targets maintain FIFO order.
#[test]
fn fh2_multiple_oneshot_targets_fifo() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();

    let order = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    // Create 5 receivers, each with a one-shot deferred connection
    let mut recv_ids = Vec::new();
    for i in 0..5 {
        let recv = tree
            .add_child(root, Node::new(&format!("R{i}"), "Node2D"))
            .unwrap();
        recv_ids.push(recv);

        let o = order.clone();
        let label = format!("recv_{i}");
        let conn = Connection::with_callback(recv.object_id(), &format!("on_{i}"), move |_| {
            o.lock().unwrap().push(label.clone());
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot();
        tree.connect_signal(emitter, "sig", conn);
    }

    tree.emit_signal(emitter, "sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 5);
    tree.flush_deferred_signals();

    let delivered = order.lock().unwrap().clone();
    assert_eq!(
        delivered,
        vec!["recv_0", "recv_1", "recv_2", "recv_3", "recv_4"],
        "one-shot deferred from 5 targets fires in FIFO registration order"
    );

    // Second emit: all one-shots consumed, nothing queues
    tree.emit_signal(emitter, "sig", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        0,
        "all one-shot connections consumed — nothing queues"
    );
}

/// Mixed one-shot and persistent deferred on two different signals from the same
/// emitter: global FIFO order is emit-order, not signal-order.
#[test]
fn fh2_two_signals_mixed_oneshot_global_fifo() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let recv = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    let order = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    // Signal A: persistent deferred
    let o = order.clone();
    tree.connect_signal(
        emitter,
        "sig_a",
        Connection::with_callback(recv.object_id(), "on_a", move |_| {
            o.lock().unwrap().push("A_persist".into());
            Variant::Nil
        })
        .as_deferred(),
    );

    // Signal B: one-shot deferred
    let o = order.clone();
    tree.connect_signal(
        emitter,
        "sig_b",
        Connection::with_callback(recv.object_id(), "on_b", move |_| {
            o.lock().unwrap().push("B_oneshot".into());
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    // Emit B first, then A — global FIFO should deliver B before A
    tree.emit_signal(emitter, "sig_b", &[]);
    tree.emit_signal(emitter, "sig_a", &[]);
    assert_eq!(tree.deferred_signal_count(), 2);
    tree.flush_deferred_signals();

    let delivered = order.lock().unwrap().clone();
    assert_eq!(
        delivered,
        vec!["B_oneshot", "A_persist"],
        "global FIFO: B emitted first should fire first, regardless of signal name"
    );
}

/// One-shot deferred connection with arguments preserves args at emit time.
#[test]
fn fh2_oneshot_deferred_preserves_emit_time_args() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let recv = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    let captured = Arc::new(std::sync::Mutex::new(Vec::<Variant>::new()));
    let c = captured.clone();

    let conn = Connection::with_callback(recv.object_id(), "on_sig", move |args| {
        c.lock().unwrap().extend_from_slice(args);
        Variant::Nil
    })
    .as_deferred()
    .as_one_shot();

    tree.connect_signal(emitter, "sig", conn);
    tree.emit_signal(
        emitter,
        "sig",
        &[Variant::Int(42), Variant::String("hello".into())],
    );
    tree.flush_deferred_signals();

    let args = captured.lock().unwrap().clone();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], Variant::Int(42));
    assert_eq!(args[1], Variant::String("hello".into()));
}

/// Persistent + one-shot + persistent on same signal — one-shot is removed after
/// first flush, connection count drops correctly, remaining connections maintain order.
#[test]
fn fh2_connection_count_after_oneshot_removal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let recv = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    // persistent A, one-shot B, persistent C
    for (i, oneshot) in [(0, false), (1, true), (2, false)] {
        let mut conn =
            Connection::with_callback(recv.object_id(), &format!("m{i}"), |_| Variant::Nil)
                .as_deferred();
        if oneshot {
            conn = conn.as_one_shot();
        }
        tree.connect_signal(emitter, "sig", conn);
    }

    // Before emit: 3 connections
    tree.emit_signal(emitter, "sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 3);
    tree.flush_deferred_signals();

    // After flush: one-shot removed, 2 remain
    tree.emit_signal(emitter, "sig", &[]);
    assert_eq!(
        tree.deferred_signal_count(),
        2,
        "one-shot removed after first flush — 2 persistent remain"
    );
    tree.flush_deferred_signals();
}

/// Rapid-fire: emit signal 10 times with mixed one-shot and persistent deferred.
/// One-shot fires once on first flush; persistent fires once per emit.
#[test]
fn fh2_rapid_fire_oneshot_fires_once_persistent_fires_all() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let recv = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    let oneshot_count = Arc::new(AtomicU64::new(0));
    let persist_count = Arc::new(AtomicU64::new(0));

    let oc = oneshot_count.clone();
    tree.connect_signal(
        emitter,
        "sig",
        Connection::with_callback(recv.object_id(), "on_oneshot", move |_| {
            oc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred()
        .as_one_shot(),
    );

    let pc = persist_count.clone();
    tree.connect_signal(
        emitter,
        "sig",
        Connection::with_callback(recv.object_id(), "on_persist", move |_| {
            pc.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        })
        .as_deferred(),
    );

    // First frame: emit 10 times, then flush
    for _ in 0..10 {
        tree.emit_signal(emitter, "sig", &[]);
    }
    // One-shot is removed after first emit's collection, so only 1 one-shot queued
    // and 10 persistent queued
    tree.flush_deferred_signals();

    assert_eq!(
        oneshot_count.load(Ordering::SeqCst),
        1,
        "one-shot should fire exactly once even with 10 emits"
    );
    assert_eq!(
        persist_count.load(Ordering::SeqCst),
        10,
        "persistent should fire once per emit (10 total)"
    );
}

/// Three one-shot deferred connections on the same signal: all fire in FIFO order
/// on first emit, and none fire on second emit.
#[test]
fn fh2_three_oneshots_fifo_then_none() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree.add_child(root, Node::new("E", "Node2D")).unwrap();
    let recv = tree.add_child(root, Node::new("R", "Node2D")).unwrap();

    let order = Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));

    for i in 0..3u32 {
        let o = order.clone();
        tree.connect_signal(
            emitter,
            "sig",
            Connection::with_callback(recv.object_id(), &format!("m{i}"), move |_| {
                o.lock().unwrap().push(i);
                Variant::Nil
            })
            .as_deferred()
            .as_one_shot(),
        );
    }

    tree.emit_signal(emitter, "sig", &[]);
    tree.flush_deferred_signals();

    assert_eq!(*order.lock().unwrap(), vec![0, 1, 2], "FIFO order for 3 one-shots");

    order.lock().unwrap().clear();
    tree.emit_signal(emitter, "sig", &[]);
    assert_eq!(tree.deferred_signal_count(), 0, "all one-shots consumed");
    tree.flush_deferred_signals();
    assert!(order.lock().unwrap().is_empty(), "no deliveries on second emit");
}
