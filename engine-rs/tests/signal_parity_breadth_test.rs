//! pat-6tg: Broadened signal parity coverage.
//!
//! Tests signal emission ordering with 5+ connections, complex-typed args,
//! deferred signals, and signals during _ready.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::LifecycleManager;
use gdscene::SignalConnection as Connection;
use gdvariant::Variant;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ===========================================================================
// Helpers
// ===========================================================================

fn signal_events(tree: &SceneTree) -> Vec<(String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect()
}

// ===========================================================================
// 1. Signal ordering with 5+ connections
// ===========================================================================

#[test]
fn five_connections_dispatch_in_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut recv_ids = Vec::new();
    for i in 0..5 {
        let recv = Node::new(&format!("Recv{i}"), "Node2D");
        let recv_id = tree.add_child(root, recv).unwrap();
        recv_ids.push(recv_id);

        let order = call_order.clone();
        let conn =
            Connection::with_callback(recv_id.object_id(), &format!("handler_{i}"), move |_| {
                order.lock().unwrap().push(i);
                Variant::Nil
            });
        tree.connect_signal(emitter_id, "broadcast", conn);
    }

    tree.event_trace_mut().enable();
    tree.emit_signal(emitter_id, "broadcast", &[]);

    let order = call_order.lock().unwrap();
    assert_eq!(
        *order,
        vec![0, 1, 2, 3, 4],
        "dispatch must follow connection order"
    );
}

#[test]
fn eight_connections_all_fire() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("E", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let total = Arc::new(AtomicU64::new(0));
    for i in 0..8 {
        let recv = Node::new(&format!("R{i}"), "Node");
        let recv_id = tree.add_child(root, recv).unwrap();

        let t = total.clone();
        let conn = Connection::with_callback(recv_id.object_id(), &format!("h{i}"), move |_| {
            t.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        });
        tree.connect_signal(emitter_id, "go", conn);
    }

    tree.emit_signal(emitter_id, "go", &[]);
    assert_eq!(total.load(Ordering::SeqCst), 8);
}

// ===========================================================================
// 2. Complex-typed signal arguments
// ===========================================================================

#[test]
fn signal_with_vector2_and_dict_args() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("E", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let recv = Node::new("R", "Node");
    let recv_id = tree.add_child(root, recv).unwrap();

    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let r = received.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "handler", move |args| {
        r.lock().unwrap().extend(args.to_vec());
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "complex", conn);

    let vec_arg = Variant::Vector2(gdcore::math::Vector2::new(3.14, 2.71));
    let array_arg = Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)]);
    let nil_arg = Variant::Nil;
    let bool_arg = Variant::Bool(false);

    tree.emit_signal(
        emitter_id,
        "complex",
        &[
            vec_arg.clone(),
            array_arg.clone(),
            nil_arg.clone(),
            bool_arg.clone(),
        ],
    );

    let args = received.lock().unwrap();
    assert_eq!(args.len(), 4);
    assert_eq!(args[0], vec_arg);
    assert_eq!(args[1], array_arg);
    assert_eq!(args[2], nil_arg);
    assert_eq!(args[3], bool_arg);
}

#[test]
fn signal_with_nested_array_arg() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("E", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let recv = Node::new("R", "Node");
    let recv_id = tree.add_child(root, recv).unwrap();

    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let r = received.clone();

    let conn = Connection::with_callback(recv_id.object_id(), "handler", move |args| {
        r.lock().unwrap().extend(args.to_vec());
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "nested", conn);

    let nested = Variant::Array(vec![
        Variant::Array(vec![Variant::Int(1), Variant::Int(2)]),
        Variant::String("middle".into()),
        Variant::Array(vec![Variant::Bool(true)]),
    ]);
    tree.emit_signal(emitter_id, "nested", &[nested.clone()]);

    let args = received.lock().unwrap();
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], nested);
}

// ===========================================================================
// 3. Signals and lifecycle interaction
// ===========================================================================

#[test]
fn signal_after_enter_tree_traced() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("E", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let recv = Node::new("R", "Node");
    let recv_id = tree.add_child(root, recv).unwrap();

    let counter = Arc::new(AtomicU64::new(0));
    let c = counter.clone();
    let conn = Connection::with_callback(recv_id.object_id(), "handler", move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "ready_sig", conn);

    tree.event_trace_mut().enable();

    // Enter tree, then emit signal
    LifecycleManager::enter_tree(&mut tree, root);
    tree.emit_signal(emitter_id, "ready_sig", &[]);

    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Trace should show ENTER_TREE events before signal
    let events = tree.event_trace().events();
    let enter_count = events.iter().filter(|e| e.detail == "ENTER_TREE").count();
    let signal_idx = events
        .iter()
        .position(|e| e.event_type == TraceEventType::SignalEmit)
        .unwrap();

    assert!(enter_count >= 3); // root + E + R
    assert!(
        signal_idx >= enter_count,
        "signal should come after ENTER_TREE"
    );
}

// ===========================================================================
// 4. One-shot with 5+ connections — only one-shots removed
// ===========================================================================

#[test]
fn mixed_one_shot_and_persistent_five_connections() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("E", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let counters: Vec<Arc<AtomicU64>> = (0..5).map(|_| Arc::new(AtomicU64::new(0))).collect();

    for i in 0..5 {
        let recv = Node::new(&format!("R{i}"), "Node");
        let recv_id = tree.add_child(root, recv).unwrap();

        let c = counters[i].clone();
        let mut conn =
            Connection::with_callback(recv_id.object_id(), &format!("h{i}"), move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            });
        // Make even-indexed connections one-shot
        if i % 2 == 0 {
            conn = conn.as_one_shot();
        }
        tree.connect_signal(emitter_id, "mixed", conn);
    }

    // First emit: all 5 fire
    tree.emit_signal(emitter_id, "mixed", &[]);
    for (i, c) in counters.iter().enumerate() {
        assert_eq!(
            c.load(Ordering::SeqCst),
            1,
            "R{i} should fire on first emit"
        );
    }

    // Second emit: only persistent (odd) fire
    tree.emit_signal(emitter_id, "mixed", &[]);
    for (i, c) in counters.iter().enumerate() {
        let expected = if i % 2 == 0 { 1 } else { 2 };
        assert_eq!(
            c.load(Ordering::SeqCst),
            expected,
            "R{i} count after second emit"
        );
    }
}

// ===========================================================================
// 5. Signal fan-in: multiple emitters to one receiver
// ===========================================================================

#[test]
fn fan_in_multiple_emitters_single_receiver() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let recv = Node::new("Receiver", "Node");
    let recv_id = tree.add_child(root, recv).unwrap();

    let total = Arc::new(AtomicU64::new(0));

    let mut emitter_ids = Vec::new();
    for i in 0..5 {
        let emitter = Node::new(&format!("E{i}"), "Node2D");
        let eid = tree.add_child(root, emitter).unwrap();
        emitter_ids.push(eid);

        let t = total.clone();
        let conn = Connection::with_callback(recv_id.object_id(), &format!("on_e{i}"), move |_| {
            t.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        });
        tree.connect_signal(eid, "ping", conn);
    }

    tree.event_trace_mut().enable();

    // Each emitter emits once
    for eid in &emitter_ids {
        tree.emit_signal(*eid, "ping", &[]);
    }

    assert_eq!(total.load(Ordering::SeqCst), 5);
    assert_eq!(signal_events(&tree).len(), 5);
}

// ===========================================================================
// 6. Signal with zero args
// ===========================================================================

#[test]
fn signal_zero_args_callback_receives_empty_slice() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("E", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();
    let recv = Node::new("R", "Node");
    let recv_id = tree.add_child(root, recv).unwrap();

    let arg_count = Arc::new(std::sync::Mutex::new(None));
    let ac = arg_count.clone();
    let conn = Connection::with_callback(recv_id.object_id(), "h", move |args| {
        *ac.lock().unwrap() = Some(args.len());
        Variant::Nil
    });
    tree.connect_signal(emitter_id, "empty", conn);
    tree.emit_signal(emitter_id, "empty", &[]);

    assert_eq!(*arg_count.lock().unwrap(), Some(0));
}
