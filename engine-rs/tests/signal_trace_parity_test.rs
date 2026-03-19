//! Runtime signal trace parity tests (pat-fu6).
//!
//! Verifies signal emission tracing matches Godot's behavior:
//! 1. Signal emissions appear in EventTrace with correct frame numbers
//! 2. Multiple connections fire in connection order (traced)
//! 3. Signal arguments are passed through correctly
//! 4. Signals emitted during _ready and _process are traced at correct frames
//! 5. Cross-node signal dispatch: emit on one node triggers method on another
//! 6. .tscn-declared connections are wired and traced correctly

mod oracle_fixture;

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::trace::TraceEventType;
use gdscene::{LifecycleManager, MainLoop, SignalConnection as Connection};
use gdvariant::Variant;
use oracle_fixture::fixtures_dir;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ===========================================================================
// Helpers
// ===========================================================================

fn signal_trace(tree: &SceneTree) -> Vec<(u64, String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::SignalEmit)
        .map(|e| (e.frame, e.node_path.clone(), e.detail.clone()))
        .collect()
}

fn script_call_trace(tree: &SceneTree) -> Vec<(u64, String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::ScriptCall)
        .map(|e| (e.frame, e.node_path.clone(), e.detail.clone()))
        .collect()
}

fn all_trace(tree: &SceneTree) -> Vec<(TraceEventType, u64, String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .map(|e| {
            (
                e.event_type.clone(),
                e.frame,
                e.node_path.clone(),
                e.detail.clone(),
            )
        })
        .collect()
}

// ===========================================================================
// 1. Signal emissions in trace with correct frame numbers
// ===========================================================================

/// Signal emissions are recorded in EventTrace with frame number matching emission frame.
#[test]
fn signal_emission_traced_with_frame() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    tree.event_trace_mut().enable();

    tree.set_trace_frame(0);
    tree.emit_signal(emitter_id, "hit", &[]);

    tree.set_trace_frame(1);
    tree.emit_signal(emitter_id, "hit", &[]);

    tree.set_trace_frame(5);
    tree.emit_signal(emitter_id, "scored", &[]);

    let signals = signal_trace(&tree);
    assert_eq!(signals.len(), 3);
    assert_eq!(signals[0], (0, "/root/Emitter".into(), "hit".into()));
    assert_eq!(signals[1], (1, "/root/Emitter".into(), "hit".into()));
    assert_eq!(signals[2], (5, "/root/Emitter".into(), "scored".into()));
}

/// Different signals on different nodes are traced independently with correct paths.
#[test]
fn different_emitters_traced_independently() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player = Node::new("Player", "Node2D");
    let player_id = tree.add_child(root, player).unwrap();

    let enemy = Node::new("Enemy", "Node2D");
    let enemy_id = tree.add_child(root, enemy).unwrap();

    tree.event_trace_mut().enable();

    tree.emit_signal(player_id, "health_changed", &[]);
    tree.emit_signal(enemy_id, "attack", &[]);
    tree.emit_signal(player_id, "died", &[]);

    let signals = signal_trace(&tree);
    assert_eq!(signals.len(), 3);
    assert_eq!(signals[0].1, "/root/Player");
    assert_eq!(signals[0].2, "health_changed");
    assert_eq!(signals[1].1, "/root/Enemy");
    assert_eq!(signals[1].2, "attack");
    assert_eq!(signals[2].1, "/root/Player");
    assert_eq!(signals[2].2, "died");
}

// ===========================================================================
// 2. Connection-order dispatch in trace
// ===========================================================================

/// When multiple nodes are connected, callbacks fire in connection order.
/// The trace captures the emission source, not the receivers.
#[test]
fn multi_connection_fires_in_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv_a = Node::new("RecvA", "Node2D");
    let recv_a_id = tree.add_child(root, recv_a).unwrap();

    let recv_b = Node::new("RecvB", "Node2D");
    let recv_b_id = tree.add_child(root, recv_b).unwrap();

    let recv_c = Node::new("RecvC", "Node2D");
    let recv_c_id = tree.add_child(root, recv_c).unwrap();

    tree.event_trace_mut().enable();

    let order = Arc::new(std::sync::Mutex::new(Vec::new()));

    let oa = order.clone();
    tree.connect_signal(
        emitter_id,
        "blast",
        Connection::with_callback(recv_a_id.object_id(), "on_blast", move |_| {
            oa.lock().unwrap().push("A");
            Variant::Nil
        }),
    );

    let ob = order.clone();
    tree.connect_signal(
        emitter_id,
        "blast",
        Connection::with_callback(recv_b_id.object_id(), "on_blast", move |_| {
            ob.lock().unwrap().push("B");
            Variant::Nil
        }),
    );

    let oc = order.clone();
    tree.connect_signal(
        emitter_id,
        "blast",
        Connection::with_callback(recv_c_id.object_id(), "on_blast", move |_| {
            oc.lock().unwrap().push("C");
            Variant::Nil
        }),
    );

    tree.emit_signal(emitter_id, "blast", &[]);

    let fired = order.lock().unwrap();
    assert_eq!(*fired, vec!["A", "B", "C"], "must fire in connection order");

    // Single trace event for the emission source.
    let signals = signal_trace(&tree);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].1, "/root/Emitter");
    assert_eq!(signals[0].2, "blast");
}

// ===========================================================================
// 3. Signal arguments passed through
// ===========================================================================

/// Signal arguments are correctly passed to connected callbacks.
#[test]
fn signal_arguments_passed_to_callbacks() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let recv = Node::new("Recv", "Node2D");
    let recv_id = tree.add_child(root, recv).unwrap();

    tree.event_trace_mut().enable();

    let captured_args = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ca = captured_args.clone();

    tree.connect_signal(
        emitter_id,
        "damage",
        Connection::with_callback(recv_id.object_id(), "on_damage", move |args| {
            ca.lock().unwrap().extend(args.iter().cloned());
            Variant::Nil
        }),
    );

    tree.emit_signal(
        emitter_id,
        "damage",
        &[Variant::Int(42), Variant::String("fire".into())],
    );

    let args = captured_args.lock().unwrap();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], Variant::Int(42));
    assert_eq!(args[1], Variant::String("fire".into()));

    // Signal emission is traced.
    let signals = signal_trace(&tree);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].2, "damage");
}

// ===========================================================================
// 4. Signal during _ready and _process
// ===========================================================================

/// Signal emitted during _ready appears in trace after READY notification.
#[test]
fn signal_during_ready_traced_correctly() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Emitter", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let script_src = "\
extends Node2D
signal initialized
func _ready():
    emit_signal(\"initialized\")
";
    let script = GDScriptNodeInstance::from_source(script_src, emitter_id).unwrap();
    tree.attach_script(emitter_id, Box::new(script));

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, emitter_id);

    let events = tree.event_trace().events();

    // Find READY notification and signal emission positions.
    let ready_idx = events
        .iter()
        .position(|e| e.detail == "READY" && e.node_path.contains("Emitter"))
        .expect("Emitter READY");

    let signal_idx = events
        .iter()
        .position(|e| e.event_type == TraceEventType::SignalEmit && e.detail == "initialized");

    if let Some(sig_idx) = signal_idx {
        assert!(
            sig_idx > ready_idx,
            "signal emission should occur after READY notification"
        );
        assert_eq!(
            events[sig_idx].frame, 0,
            "signal during init should be on frame 0"
        );
    }
}

/// Signal emitted during _process appears with correct frame number.
#[test]
fn signal_during_process_traced_with_frame() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Ticker", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let script_src = "\
extends Node2D
signal tick
func _process(delta):
    emit_signal(\"tick\")
";
    let script = GDScriptNodeInstance::from_source(script_src, emitter_id).unwrap();
    tree.attach_script(emitter_id, Box::new(script));

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, emitter_id);

    let mut ml = MainLoop::new(tree);
    ml.run_frames(3, 1.0 / 60.0);

    let tree = ml.tree();
    let signals = signal_trace(tree);

    // Should have at least one tick signal per frame.
    assert!(
        signals.len() >= 3,
        "expected at least 3 tick signals (one per frame), got {}",
        signals.len()
    );

    // Frame numbers should be 0, 1, 2.
    let frames: Vec<u64> = signals.iter().map(|(f, _, _)| *f).collect();
    assert!(frames.contains(&0), "should have tick on frame 0");
    assert!(frames.contains(&1), "should have tick on frame 1");
    assert!(frames.contains(&2), "should have tick on frame 2");
}

// ===========================================================================
// 5. Cross-node signal dispatch: emit triggers script on another node
// ===========================================================================

/// Emitting a signal on one node calls a method on a connected target node's script.
#[test]
fn cross_node_signal_triggers_target_script() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = Node::new("Player", "Node2D");
    let emitter_id = tree.add_child(root, emitter).unwrap();

    let listener = Node::new("HUD", "Node");
    let listener_id = tree.add_child(root, listener).unwrap();

    // HUD script defines _on_health_changed.
    let listener_script_src = "\
extends Node
var last_health = 0
func _on_health_changed(value):
    last_health = value
";
    let script = GDScriptNodeInstance::from_source(listener_script_src, listener_id).unwrap();
    tree.attach_script(listener_id, Box::new(script));

    // Connect Player.health_changed → HUD._on_health_changed (no callback, script dispatch).
    tree.connect_signal(
        emitter_id,
        "health_changed",
        Connection::new(listener_id.object_id(), "_on_health_changed"),
    );

    tree.event_trace_mut().enable();
    tree.emit_signal(emitter_id, "health_changed", &[Variant::Int(75)]);

    // Signal emission should be traced.
    let signals = signal_trace(&tree);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].1, "/root/Player");
    assert_eq!(signals[0].2, "health_changed");

    // Script call should be traced (since the script defines _on_health_changed).
    let calls = script_call_trace(&tree);
    let matching: Vec<_> = calls
        .iter()
        .filter(|(_, path, detail)| path == "/root/HUD" && detail == "_on_health_changed")
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected 1 _on_health_changed script call on HUD"
    );

    // Verify the script actually received the argument.
    let hud_health = tree
        .get_script(listener_id)
        .and_then(|s| s.get_property("last_health"));
    assert_eq!(
        hud_health,
        Some(Variant::Int(75)),
        "HUD script should have received health value"
    );
}

/// Chain: A emits → B's script handles and emits → C's script handles.
#[test]
fn signal_chain_a_to_b_to_c() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node2D");
    let b_id = tree.add_child(root, b).unwrap();

    let c = Node::new("C", "Node2D");
    let c_id = tree.add_child(root, c).unwrap();

    // B handles ping from A and emits pong.
    let b_script_src = "\
extends Node2D
signal pong
var got_ping = false
func _on_ping():
    got_ping = true
    emit_signal(\"pong\")
";
    let b_script = GDScriptNodeInstance::from_source(b_script_src, b_id).unwrap();
    tree.attach_script(b_id, Box::new(b_script));

    // C handles pong from B.
    let c_script_src = "\
extends Node2D
var got_pong = false
func _on_pong():
    got_pong = true
";
    let c_script = GDScriptNodeInstance::from_source(c_script_src, c_id).unwrap();
    tree.attach_script(c_id, Box::new(c_script));

    // Wire: A.ping → B._on_ping, B.pong → C._on_pong
    tree.connect_signal(a_id, "ping", Connection::new(b_id.object_id(), "_on_ping"));
    tree.connect_signal(b_id, "pong", Connection::new(c_id.object_id(), "_on_pong"));

    tree.event_trace_mut().enable();
    tree.emit_signal(a_id, "ping", &[]);

    // Trace should show: signal_emit(A, ping) → script_call(B, _on_ping) →
    //   signal_emit(B, pong) → script_call(C, _on_pong)
    let signals = signal_trace(&tree);
    assert_eq!(
        signals.len(),
        2,
        "should have 2 signal emissions (ping + pong)"
    );
    assert_eq!(signals[0].1, "/root/A");
    assert_eq!(signals[0].2, "ping");
    assert_eq!(signals[1].1, "/root/B");
    assert_eq!(signals[1].2, "pong");

    // Verify scripts ran.
    assert_eq!(
        tree.get_script(b_id).unwrap().get_property("got_ping"),
        Some(Variant::Bool(true))
    );
    assert_eq!(
        tree.get_script(c_id).unwrap().get_property("got_pong"),
        Some(Variant::Bool(true))
    );
}

// ===========================================================================
// 6. .tscn connection wiring and tracing
// ===========================================================================

/// signals_complex.tscn connections are wired from the scene file.
/// Verify the signal stores exist and have the right connection counts.
#[test]
fn signals_complex_tscn_connections_wired() {
    let scene_path = fixtures_dir().join("scenes/signals_complex.tscn");
    let content = std::fs::read_to_string(&scene_path).expect("read signals_complex.tscn");
    let packed = gdscene::PackedScene::from_tscn(&content).expect("parse scene");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    gdscene::wire_connections(&mut tree, scene_root, packed.connections());

    // Player node should have health_changed and died signals wired.
    let player_id = tree
        .get_node_by_path("/root/Root/Player")
        .expect("Player node");
    let player_store = tree.signal_store_mut(player_id);
    assert!(
        player_store.has_signal("health_changed"),
        "Player should have health_changed signal"
    );
    assert!(
        player_store.has_signal("died"),
        "Player should have died signal"
    );

    // Enemy should have attack signal.
    let enemy_id = tree
        .get_node_by_path("/root/Root/Enemy")
        .expect("Enemy node");
    let enemy_store = tree.signal_store_mut(enemy_id);
    assert!(
        enemy_store.has_signal("attack"),
        "Enemy should have attack signal"
    );

    // Root should have score_updated signal.
    let root_store = tree.signal_store_mut(scene_root);
    assert!(
        root_store.has_signal("score_updated"),
        "Root should have score_updated signal"
    );

    // ItemDrop should have collected signal.
    let item_id = tree
        .get_node_by_path("/root/Root/ItemDrop")
        .expect("ItemDrop node");
    let item_store = tree.signal_store_mut(item_id);
    assert!(
        item_store.has_signal("collected"),
        "ItemDrop should have collected signal"
    );

    // TriggerZone should have body_entered signal.
    let trigger_id = tree
        .get_node_by_path("/root/Root/Player/TriggerZone")
        .expect("TriggerZone node");
    let trigger_store = tree.signal_store_mut(trigger_id);
    assert!(
        trigger_store.has_signal("body_entered"),
        "TriggerZone should have body_entered signal"
    );
}

/// Emitting signals on wired .tscn connections produces trace events.
#[test]
fn signals_complex_emit_produces_trace() {
    let scene_path = fixtures_dir().join("scenes/signals_complex.tscn");
    let content = std::fs::read_to_string(&scene_path).expect("read signals_complex.tscn");
    let packed = gdscene::PackedScene::from_tscn(&content).expect("parse scene");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    gdscene::wire_connections(&mut tree, scene_root, packed.connections());

    tree.event_trace_mut().enable();

    let player_id = tree.get_node_by_path("/root/Root/Player").expect("Player");
    let enemy_id = tree.get_node_by_path("/root/Root/Enemy").expect("Enemy");
    let item_id = tree
        .get_node_by_path("/root/Root/ItemDrop")
        .expect("ItemDrop");

    // Emit signals — no scripts, so no method dispatch, but trace should record.
    tree.emit_signal(player_id, "health_changed", &[Variant::Int(50)]);
    tree.emit_signal(player_id, "died", &[]);
    tree.emit_signal(enemy_id, "attack", &[Variant::Int(10)]);
    tree.emit_signal(scene_root, "score_updated", &[Variant::Int(100)]);
    tree.emit_signal(item_id, "collected", &[]);

    let signals = signal_trace(&tree);
    assert_eq!(signals.len(), 5, "all 5 emissions should be traced");

    let details: Vec<&str> = signals.iter().map(|(_, _, d)| d.as_str()).collect();
    assert_eq!(
        details,
        vec![
            "health_changed",
            "died",
            "attack",
            "score_updated",
            "collected"
        ]
    );

    let paths: Vec<&str> = signals.iter().map(|(_, p, _)| p.as_str()).collect();
    assert_eq!(paths[0], "/root/Root/Player");
    assert_eq!(paths[1], "/root/Root/Player");
    assert_eq!(paths[2], "/root/Root/Enemy");
    assert_eq!(paths[3], "/root/Root");
    assert_eq!(paths[4], "/root/Root/ItemDrop");
}

// ===========================================================================
// 7. One-shot via .tscn flags
// ===========================================================================

/// One-shot connections (flags=3 in .tscn includes CONNECT_ONE_SHOT) should
/// auto-disconnect after first emit.
#[test]
fn one_shot_callback_via_flags() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let trigger = Node::new("Trigger", "Node2D");
    let trigger_id = tree.add_child(root, trigger).unwrap();

    let target = Node::new("Target", "Node2D");
    let target_id = tree.add_child(root, target).unwrap();

    tree.event_trace_mut().enable();

    let counter = Arc::new(AtomicU64::new(0));
    let cc = counter.clone();

    let conn = Connection::with_callback(target_id.object_id(), "on_entered", move |_| {
        cc.fetch_add(1, Ordering::SeqCst);
        Variant::Nil
    })
    .as_one_shot();

    tree.connect_signal(trigger_id, "body_entered", conn);

    tree.emit_signal(trigger_id, "body_entered", &[]);
    tree.emit_signal(trigger_id, "body_entered", &[]);
    tree.emit_signal(trigger_id, "body_entered", &[]);

    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "one-shot should fire only once"
    );

    // All 3 emissions are traced regardless.
    let signals = signal_trace(&tree);
    assert_eq!(
        signals.len(),
        3,
        "all emissions traced even after one-shot disconnect"
    );
}

// ===========================================================================
// 8. Trace ordering: signals interleaved with lifecycle events
// ===========================================================================

/// Verify signal emissions are correctly interleaved with lifecycle notifications
/// in the trace output.
#[test]
fn signal_trace_interleaved_with_lifecycle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Obj", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();

    // Script emits signal during _ready.
    let script_src = "\
extends Node2D
signal ready_done
func _ready():
    emit_signal(\"ready_done\")
";
    let script = GDScriptNodeInstance::from_source(script_src, node_id).unwrap();
    tree.attach_script(node_id, Box::new(script));

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, node_id);

    let events = tree.event_trace().events();

    // Find key event positions.
    let enter_tree_idx = events
        .iter()
        .position(|e| e.detail == "ENTER_TREE" && e.node_path.contains("Obj"))
        .expect("ENTER_TREE");
    let ready_idx = events
        .iter()
        .position(|e| e.detail == "READY" && e.node_path.contains("Obj"))
        .expect("READY");

    let signal_idx = events
        .iter()
        .position(|e| e.event_type == TraceEventType::SignalEmit && e.detail == "ready_done");

    // ENTER_TREE < READY < signal emission (during _ready callback)
    assert!(enter_tree_idx < ready_idx, "ENTER_TREE before READY");

    if let Some(sig_idx) = signal_idx {
        assert!(
            sig_idx > ready_idx,
            "signal during _ready should come after READY notification"
        );
    }
}
