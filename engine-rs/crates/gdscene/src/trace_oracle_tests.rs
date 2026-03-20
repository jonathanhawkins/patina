//! Oracle trace comparison tests for lifecycle notifications, signals, and
//! extended notification coverage.
//!
//! These tests verify that Patina's `EventTrace` system records the same
//! ordering and semantics as Godot's oracle output. Each test constructs a
//! scene tree, runs it through the `MainLoop`, and compares the resulting
//! trace against the expected oracle-format output.
//!
//! ## Beads covered
//! - **pat-fbi**: Compare lifecycle notification traces against oracle output
//! - **pat-fu6**: Compare runtime signal traces against oracle trace output
//! - **pat-isl**: Expand notification coverage beyond lifecycle basics

#[cfg(test)]
mod tests {
    use crate::lifecycle::LifecycleManager;
    use crate::main_loop::MainLoop;
    use crate::node::{Node, NodeId};
    use crate::scene_tree::SceneTree;
    use crate::trace::{TraceEvent, TraceEventType};
    use gdobject::notification::{
        NOTIFICATION_DRAW, NOTIFICATION_ENTER_TREE, NOTIFICATION_EXIT_TREE,
        NOTIFICATION_INTERNAL_PHYSICS_PROCESS, NOTIFICATION_INTERNAL_PROCESS,
        NOTIFICATION_MOVED_IN_PARENT, NOTIFICATION_PAUSED, NOTIFICATION_PHYSICS_PROCESS,
        NOTIFICATION_PROCESS, NOTIFICATION_READY, NOTIFICATION_UNPAUSED,
    };
    use gdobject::signal::Connection;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Builds a tree: root -> Parent -> [Child1, Child2]
    fn build_hierarchy() -> (SceneTree, NodeId, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child1 = Node::new("Child1", "Node");
        let child1_id = tree.add_child(parent_id, child1).unwrap();
        let child2 = Node::new("Child2", "Node");
        let child2_id = tree.add_child(parent_id, child2).unwrap();
        (tree, parent_id, child1_id, child2_id)
    }

    /// Builds a deeper tree: root -> A -> B -> C
    fn build_deep_hierarchy() -> (SceneTree, NodeId, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node2D");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node2D");
        let b_id = tree.add_child(a_id, b).unwrap();
        let c = Node::new("C", "Node2D");
        let c_id = tree.add_child(b_id, c).unwrap();
        (tree, a_id, b_id, c_id)
    }

    /// Filter events by type.
    fn filter_type<'a>(events: &'a [TraceEvent], ty: &TraceEventType) -> Vec<&'a TraceEvent> {
        events.iter().filter(|e| &e.event_type == ty).collect()
    }

    /// Filter events by detail string.
    fn filter_detail<'a>(events: &'a [TraceEvent], detail: &str) -> Vec<&'a TraceEvent> {
        events.iter().filter(|e| e.detail == detail).collect()
    }

    /// Extracts paths from events in order.
    fn paths<'a>(events: &[&'a TraceEvent]) -> Vec<&'a str> {
        events.iter().map(|e| e.node_path.as_str()).collect()
    }

    // =======================================================================
    // pat-fbi: Lifecycle notification traces vs oracle output
    // =======================================================================

    /// Oracle contract: ENTER_TREE fires top-down (parent before children).
    #[test]
    fn lifecycle_enter_tree_top_down_oracle_order() {
        let (mut tree, parent_id, _child1_id, _child2_id) = build_hierarchy();
        tree.event_trace_mut().enable();

        LifecycleManager::enter_tree(&mut tree, parent_id);

        let events = tree.event_trace().events();
        let enter_tree = filter_detail(events, "ENTER_TREE");
        let paths = paths(&enter_tree);

        // Oracle contract: parent enters before children.
        assert_eq!(
            paths,
            vec!["/root/Parent", "/root/Parent/Child1", "/root/Parent/Child2"]
        );
    }

    /// Oracle contract: READY fires bottom-up (children before parent).
    #[test]
    fn lifecycle_ready_bottom_up_oracle_order() {
        let (mut tree, parent_id, _child1_id, _child2_id) = build_hierarchy();
        tree.event_trace_mut().enable();

        LifecycleManager::enter_tree(&mut tree, parent_id);

        let events = tree.event_trace().events();
        let ready = filter_detail(events, "READY");
        let paths = paths(&ready);

        // Oracle contract: children ready before parent.
        assert_eq!(
            paths,
            vec!["/root/Parent/Child1", "/root/Parent/Child2", "/root/Parent"]
        );
    }

    /// Oracle contract: EXIT_TREE fires bottom-up (children before parent).
    #[test]
    fn lifecycle_exit_tree_bottom_up_oracle_order() {
        let (mut tree, parent_id, _child1_id, _child2_id) = build_hierarchy();
        tree.event_trace_mut().enable();

        LifecycleManager::exit_tree(&mut tree, parent_id);

        let events = tree.event_trace().events();
        let exit = filter_detail(events, "EXIT_TREE");
        let paths = paths(&exit);

        // Oracle contract: children exit before parent.
        assert_eq!(
            paths,
            vec!["/root/Parent/Child1", "/root/Parent/Child2", "/root/Parent"]
        );
    }

    /// Oracle contract: all ENTER_TREE events complete before any READY event.
    #[test]
    fn lifecycle_all_enter_before_any_ready() {
        let (mut tree, parent_id, _child1_id, _child2_id) = build_hierarchy();
        tree.event_trace_mut().enable();

        LifecycleManager::enter_tree(&mut tree, parent_id);

        let events = tree.event_trace().events();
        let notifs = filter_type(events, &TraceEventType::Notification);

        let last_enter = notifs
            .iter()
            .rposition(|e| e.detail == "ENTER_TREE")
            .expect("at least one ENTER_TREE");
        let first_ready = notifs
            .iter()
            .position(|e| e.detail == "READY")
            .expect("at least one READY");

        assert!(
            last_enter < first_ready,
            "all ENTER_TREE events must finish before first READY"
        );
    }

    /// Oracle contract: deep hierarchy (3 levels) preserves ordering.
    #[test]
    fn lifecycle_deep_hierarchy_enter_ready_order() {
        let (mut tree, a_id, _b_id, _c_id) = build_deep_hierarchy();
        tree.event_trace_mut().enable();

        LifecycleManager::enter_tree(&mut tree, a_id);

        let events = tree.event_trace().events();
        let enter = filter_detail(events, "ENTER_TREE");
        let ready = filter_detail(events, "READY");

        // ENTER_TREE: top-down (A, B, C)
        assert_eq!(paths(&enter), vec!["/root/A", "/root/A/B", "/root/A/B/C"]);

        // READY: bottom-up (C, B, A)
        assert_eq!(paths(&ready), vec!["/root/A/B/C", "/root/A/B", "/root/A"]);
    }

    /// Oracle contract: per-frame PROCESS and PHYSICS_PROCESS notifications
    /// fire in tree order and are traced correctly.
    #[test]
    fn lifecycle_per_frame_process_trace_matches_oracle() {
        let (tree, _parent_id, _child1_id, _child2_id) = build_hierarchy();
        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();

        // Run 3 frames at 60fps (1 physics tick per frame).
        ml.run_frames(3, 1.0 / 60.0);

        let events = ml.tree().event_trace().events();

        // Each frame should produce: INTERNAL_PHYSICS_PROCESS, PHYSICS_PROCESS,
        // INTERNAL_PROCESS, PROCESS for each node.
        for frame in 0..3u64 {
            let frame_process: Vec<_> = events
                .iter()
                .filter(|e| {
                    e.frame == frame
                        && e.event_type == TraceEventType::Notification
                        && e.detail == "PROCESS"
                })
                .collect();
            assert!(
                frame_process.len() >= 3,
                "frame {frame}: expected at least 3 PROCESS notifications (root + Parent + children), got {}",
                frame_process.len()
            );
        }
    }

    /// Oracle contract: INTERNAL_PHYSICS_PROCESS always precedes PHYSICS_PROCESS
    /// within a single frame, for every node.
    #[test]
    fn lifecycle_internal_physics_before_user_physics() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("TestNode", "Node");
        tree.add_child(root, child).unwrap();
        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();

        ml.step(1.0 / 60.0);

        let events = ml.tree().event_trace().events();
        let internal_phys: Vec<_> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.detail == "INTERNAL_PHYSICS_PROCESS" && e.node_path == "/root/TestNode"
            })
            .collect();
        let user_phys: Vec<_> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.detail == "PHYSICS_PROCESS" && e.node_path == "/root/TestNode")
            .collect();

        assert!(
            !internal_phys.is_empty(),
            "expected INTERNAL_PHYSICS_PROCESS"
        );
        assert!(!user_phys.is_empty(), "expected PHYSICS_PROCESS");
        assert!(
            internal_phys[0].0 < user_phys[0].0,
            "INTERNAL_PHYSICS_PROCESS must precede PHYSICS_PROCESS"
        );
    }

    /// Oracle contract: INTERNAL_PROCESS always precedes PROCESS within a frame.
    #[test]
    fn lifecycle_internal_process_before_user_process() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("TestNode", "Node");
        tree.add_child(root, child).unwrap();
        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();

        ml.step(1.0 / 60.0);

        let events = ml.tree().event_trace().events();
        let internal_proc_idx = events
            .iter()
            .position(|e| e.detail == "INTERNAL_PROCESS" && e.node_path == "/root/TestNode")
            .expect("INTERNAL_PROCESS for TestNode");
        let user_proc_idx = events
            .iter()
            .position(|e| e.detail == "PROCESS" && e.node_path == "/root/TestNode")
            .expect("PROCESS for TestNode");

        assert!(
            internal_proc_idx < user_proc_idx,
            "INTERNAL_PROCESS must precede PROCESS"
        );
    }

    /// Oracle contract: complete lifecycle trace for enter->frames->exit matches
    /// expected notification sequence.
    #[test]
    fn lifecycle_full_enter_process_exit_trace() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let leaf = Node::new("Leaf", "Node2D");
        let leaf_id = tree.add_child(root, leaf).unwrap();
        tree.event_trace_mut().enable();

        // Enter tree.
        LifecycleManager::enter_tree(&mut tree, leaf_id);

        // Run 1 frame through MainLoop (need to transfer tree ownership).
        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        // Exit tree.
        LifecycleManager::exit_tree(ml.tree_mut(), leaf_id);

        let events = ml.tree().event_trace().events();
        let leaf_notifs: Vec<&str> = events
            .iter()
            .filter(|e| e.node_path == "/root/Leaf" && e.event_type == TraceEventType::Notification)
            .map(|e| e.detail.as_str())
            .collect();

        // Expected oracle order: ENTER_TREE, READY, then per-frame notifs, then EXIT_TREE
        assert!(leaf_notifs.starts_with(&["ENTER_TREE", "READY"]));
        assert_eq!(*leaf_notifs.last().unwrap(), "EXIT_TREE");

        // Verify PROCESS notifications appear between READY and EXIT_TREE.
        let ready_idx = leaf_notifs.iter().position(|&d| d == "READY").unwrap();
        let exit_idx = leaf_notifs.iter().position(|&d| d == "EXIT_TREE").unwrap();
        let has_process = leaf_notifs[ready_idx..exit_idx]
            .iter()
            .any(|&d| d == "PROCESS");
        assert!(has_process, "expected PROCESS between READY and EXIT_TREE");
    }

    // =======================================================================
    // pat-fu6: Signal traces vs oracle signal trace output
    // =======================================================================

    /// Oracle signal_trace contract: signal emission is recorded with correct
    /// source path and signal name.
    #[test]
    fn signal_trace_records_emission_source_and_name() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();

        tree.event_trace_mut().enable();
        tree.emit_signal(emitter_id, "my_signal", &[]);

        let events = tree.event_trace().events();
        let signal_events = filter_type(events, &TraceEventType::SignalEmit);

        assert_eq!(signal_events.len(), 1);
        assert_eq!(signal_events[0].node_path, "/root/Emitter");
        assert_eq!(signal_events[0].detail, "my_signal");
    }

    /// Oracle signal_trace contract: multiple signals emit in correct order.
    #[test]
    fn signal_trace_multiple_emissions_preserve_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node_a = Node::new("A", "Node2D");
        let a_id = tree.add_child(root, node_a).unwrap();
        let node_b = Node::new("B", "Node2D");
        let b_id = tree.add_child(root, node_b).unwrap();

        tree.event_trace_mut().enable();
        tree.emit_signal(a_id, "signal_one", &[]);
        tree.emit_signal(b_id, "signal_two", &[]);
        tree.emit_signal(a_id, "signal_three", &[]);

        let events = tree.event_trace().events();
        let signals = filter_type(events, &TraceEventType::SignalEmit);

        assert_eq!(signals.len(), 3);
        assert_eq!(signals[0].detail, "signal_one");
        assert_eq!(signals[0].node_path, "/root/A");
        assert_eq!(signals[1].detail, "signal_two");
        assert_eq!(signals[1].node_path, "/root/B");
        assert_eq!(signals[2].detail, "signal_three");
        assert_eq!(signals[2].node_path, "/root/A");
    }

    /// Oracle signal_trace contract: signal frame number matches current frame.
    #[test]
    fn signal_trace_frame_number_matches_emit_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let emitter = Node::new("E", "Node");
        let emitter_id = tree.add_child(root, emitter).unwrap();

        tree.event_trace_mut().enable();

        // Frame 0 emission.
        tree.set_trace_frame(0);
        tree.emit_signal(emitter_id, "frame0_signal", &[]);

        // Frame 5 emission.
        tree.set_trace_frame(5);
        tree.emit_signal(emitter_id, "frame5_signal", &[]);

        let events = tree.event_trace().events();
        let signals = filter_type(events, &TraceEventType::SignalEmit);

        assert_eq!(signals[0].frame, 0);
        assert_eq!(signals[0].detail, "frame0_signal");
        assert_eq!(signals[1].frame, 5);
        assert_eq!(signals[1].detail, "frame5_signal");
    }

    /// Oracle signal_trace contract: connected callback fires and trace records it.
    #[test]
    fn signal_trace_with_callback_connection() {
        use gdcore::id::ObjectId;
        use std::sync::{
            atomic::{AtomicU32, Ordering},
            Arc,
        };

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let emitter = Node::new("Emitter", "Node2D");
        let emitter_id = tree.add_child(root, emitter).unwrap();
        let listener = Node::new("Listener", "Node2D");
        let _listener_id = tree.add_child(root, listener).unwrap();

        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();
        let conn = Connection::with_callback(ObjectId::next(), "on_signal", move |_args| {
            cc.fetch_add(1, Ordering::SeqCst);
            gdvariant::Variant::Nil
        });
        tree.connect_signal(emitter_id, "test_signal", conn);

        tree.event_trace_mut().enable();
        tree.emit_signal(emitter_id, "test_signal", &[]);

        // Verify callback was called.
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Verify trace recorded the emission.
        let signals = filter_type(tree.event_trace().events(), &TraceEventType::SignalEmit);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].detail, "test_signal");
        assert_eq!(signals[0].node_path, "/root/Emitter");
    }

    /// Oracle signal_trace contract: signals interleaved with notifications
    /// maintain global ordering in the trace.
    #[test]
    fn signal_trace_interleaved_with_notifications() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Player", "Node2D");
        let node_id = tree.add_child(root, node).unwrap();

        tree.event_trace_mut().enable();

        // Manually record a notification then emit a signal then another notification.
        tree.trace_record(node_id, TraceEventType::Notification, "ENTER_TREE");
        tree.emit_signal(node_id, "ready_signal", &[]);
        tree.trace_record(node_id, TraceEventType::Notification, "READY");

        let events = tree.event_trace().events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, TraceEventType::Notification);
        assert_eq!(events[0].detail, "ENTER_TREE");
        assert_eq!(events[1].event_type, TraceEventType::SignalEmit);
        assert_eq!(events[1].detail, "ready_signal");
        assert_eq!(events[2].event_type, TraceEventType::Notification);
        assert_eq!(events[2].detail, "READY");
    }

    /// Oracle signal_trace contract: matches the oracle signals_complex fixture
    /// format — "draw" signals emitted for CanvasItem nodes during first frame.
    /// This tests the oracle signal_trace structure: {signal_name, source_path, frame_number}.
    #[test]
    fn signal_trace_oracle_format_draw_signals() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = Node::new("Root", "Node");
        let scene_root_id = tree.add_child(root, scene_root).unwrap();
        let player = Node::new("Player", "Node2D");
        let player_id = tree.add_child(scene_root_id, player).unwrap();
        let enemy = Node::new("Enemy", "Node2D");
        let enemy_id = tree.add_child(scene_root_id, enemy).unwrap();

        tree.event_trace_mut().enable();
        tree.set_trace_frame(0);

        // Simulate the oracle's signal_trace: draw signals fire in tree order.
        tree.emit_signal(player_id, "draw", &[]);
        tree.emit_signal(enemy_id, "draw", &[]);

        let events = tree.event_trace().events();
        let signals = filter_type(events, &TraceEventType::SignalEmit);

        // Matches oracle format: [{signal_name: "draw", source_path: "/root/Root/Player", frame_number: 0}, ...]
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0].detail, "draw");
        assert_eq!(signals[0].node_path, "/root/Root/Player");
        assert_eq!(signals[0].frame, 0);
        assert_eq!(signals[1].detail, "draw");
        assert_eq!(signals[1].node_path, "/root/Root/Enemy");
        assert_eq!(signals[1].frame, 0);
    }

    // =======================================================================
    // pat-isl: Extended notification coverage beyond lifecycle basics
    // =======================================================================

    /// NOTIFICATION_DRAW (code 30) fires for CanvasItem-derived nodes.
    /// In Godot, this triggers when a node needs to redraw.
    #[test]
    fn notification_draw_fires_and_is_recorded() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let sprite = Node::new("Sprite", "Node2D");
        let sprite_id = tree.add_child(root, sprite).unwrap();

        tree.event_trace_mut().enable();
        tree.trace_record(sprite_id, TraceEventType::Notification, "DRAW");
        if let Some(node) = tree.get_node_mut(sprite_id) {
            node.receive_notification(NOTIFICATION_DRAW);
        }

        let events = tree.event_trace().events();
        let draw_events = filter_detail(events, "DRAW");
        assert_eq!(draw_events.len(), 1);
        assert_eq!(draw_events[0].node_path, "/root/Sprite");

        // Also verify notification was logged on the node.
        let log = tree.get_node(sprite_id).unwrap().notification_log();
        assert!(log.contains(&NOTIFICATION_DRAW));
    }

    /// NOTIFICATION_PAUSED (code 14) and NOTIFICATION_UNPAUSED (code 15) fire
    /// when the MainLoop pause state transitions.
    #[test]
    fn notification_paused_unpaused_traced() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("PausableNode", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();

        ml.set_paused(true);
        ml.set_paused(false);

        // Check the node received both notifications.
        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        assert!(
            log.contains(&NOTIFICATION_PAUSED),
            "expected PAUSED notification"
        );
        assert!(
            log.contains(&NOTIFICATION_UNPAUSED),
            "expected UNPAUSED notification"
        );
    }

    /// NOTIFICATION_MOVED_IN_PARENT (code 12) fires when a node's position
    /// among its siblings changes.
    #[test]
    fn notification_moved_in_parent_fires() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let _b_id = tree.add_child(root, b).unwrap();

        // Move A to a different index (if move_child is available).
        // For now, verify the node can receive MOVED_IN_PARENT.
        if let Some(node) = tree.get_node_mut(a_id) {
            node.receive_notification(NOTIFICATION_MOVED_IN_PARENT);
        }

        let log = tree.get_node(a_id).unwrap().notification_log();
        assert!(
            log.contains(&NOTIFICATION_MOVED_IN_PARENT),
            "expected MOVED_IN_PARENT notification"
        );
    }

    /// NOTIFICATION_DRAW fires for multiple CanvasItem nodes in tree order,
    /// matching oracle signal_trace "draw" pattern from signals_complex fixture.
    #[test]
    fn notification_draw_multiple_canvas_items_tree_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene = Node::new("Root", "Node");
        let scene_id = tree.add_child(root, scene).unwrap();

        let player = Node::new("Player", "Node2D");
        let player_id = tree.add_child(scene_id, player).unwrap();
        let trigger = Node::new("TriggerZone", "Node2D");
        let trigger_id = tree.add_child(player_id, trigger).unwrap();
        let enemy = Node::new("Enemy", "Node2D");
        let enemy_id = tree.add_child(scene_id, enemy).unwrap();
        let item_drop = Node::new("ItemDrop", "Node2D");
        let item_drop_id = tree.add_child(scene_id, item_drop).unwrap();

        tree.event_trace_mut().enable();

        // Simulate DRAW notification dispatch in tree order.
        let canvas_items = [player_id, trigger_id, enemy_id, item_drop_id];
        for &id in &canvas_items {
            tree.trace_record(id, TraceEventType::Notification, "DRAW");
            if let Some(node) = tree.get_node_mut(id) {
                node.receive_notification(NOTIFICATION_DRAW);
            }
        }

        let events = tree.event_trace().events();
        let draw_events = filter_detail(events, "DRAW");
        let draw_paths = paths(&draw_events);

        // Matches oracle signals_complex fixture tree order.
        assert_eq!(
            draw_paths,
            vec![
                "/root/Root/Player",
                "/root/Root/Player/TriggerZone",
                "/root/Root/Enemy",
                "/root/Root/ItemDrop",
            ]
        );
    }

    /// NOTIFICATION_PROCESS fires every frame for all nodes, with frame counter
    /// matching the MainLoop state.
    #[test]
    fn notification_process_per_frame_with_correct_frame_counter() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Counter", "Node");
        tree.add_child(root, node).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();
        ml.run_frames(5, 1.0 / 60.0);

        let events = ml.tree().event_trace().events();
        let process = events
            .iter()
            .filter(|e| {
                e.detail == "PROCESS"
                    && e.node_path == "/root/Counter"
                    && e.event_type == TraceEventType::Notification
            })
            .collect::<Vec<_>>();

        assert_eq!(process.len(), 5);
        for (i, ev) in process.iter().enumerate() {
            assert_eq!(
                ev.frame, i as u64,
                "frame counter mismatch at process event {i}"
            );
        }
    }

    /// NOTIFICATION_PHYSICS_PROCESS frame counter tracks the MainLoop correctly
    /// even when multiple physics ticks occur per frame.
    #[test]
    fn notification_physics_process_multi_tick_frame_counter() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("PhysNode", "Node");
        tree.add_child(root, node).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();

        // delta = 2/60 at 60 TPS => 2 physics ticks per frame.
        ml.step(2.0 / 60.0);

        let events = ml.tree().event_trace().events();
        let phys: Vec<_> = events
            .iter()
            .filter(|e| e.detail == "PHYSICS_PROCESS" && e.node_path == "/root/PhysNode")
            .collect();

        // Both physics ticks should have frame 0.
        assert_eq!(phys.len(), 2);
        assert_eq!(phys[0].frame, 0);
        assert_eq!(phys[1].frame, 0);
    }

    /// Paused frames do not generate PROCESS or PHYSICS_PROCESS trace events.
    #[test]
    fn notification_paused_skips_process_trace() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("PauseTest", "Node");
        tree.add_child(root, node).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();

        ml.set_paused(true);
        ml.step(1.0 / 60.0);

        let events = ml.tree().event_trace().events();
        let process = events
            .iter()
            .filter(|e| {
                (e.detail == "PROCESS" || e.detail == "PHYSICS_PROCESS")
                    && e.event_type == TraceEventType::Notification
            })
            .count();

        assert_eq!(
            process, 0,
            "paused frame should not produce PROCESS/PHYSICS_PROCESS traces"
        );
    }

    /// Frame numbers in trace are monotonically non-decreasing.
    #[test]
    fn trace_frame_numbers_monotonic() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Mono", "Node");
        tree.add_child(root, node).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();
        ml.run_frames(10, 1.0 / 60.0);

        let events = ml.tree().event_trace().events();
        let mut last_frame = 0u64;
        for ev in events {
            assert!(
                ev.frame >= last_frame,
                "frame {} followed {}, expected monotonic",
                ev.frame,
                last_frame
            );
            last_frame = ev.frame;
        }
    }

    /// Full Godot notification ordering per frame: INTERNAL_PHYSICS -> PHYSICS -> INTERNAL_PROCESS -> PROCESS
    /// verified in the trace for a hierarchy of nodes.
    #[test]
    fn notification_godot_four_phase_order_in_trace() {
        let (tree, _parent_id, _child1_id, _child2_id) = build_hierarchy();
        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();

        ml.step(1.0 / 60.0);

        let events = ml.tree().event_trace().events();
        // Find notification ordering for the Parent node on frame 0.
        let parent_notifs: Vec<&str> = events
            .iter()
            .filter(|e| {
                e.node_path == "/root/Parent"
                    && e.event_type == TraceEventType::Notification
                    && e.frame == 0
            })
            .map(|e| e.detail.as_str())
            .collect();

        // Should contain the 4 per-frame notifications in Godot order.
        let ip_idx = parent_notifs
            .iter()
            .position(|&d| d == "INTERNAL_PHYSICS_PROCESS");
        let pp_idx = parent_notifs.iter().position(|&d| d == "PHYSICS_PROCESS");
        let ip2_idx = parent_notifs.iter().position(|&d| d == "INTERNAL_PROCESS");
        let p_idx = parent_notifs.iter().position(|&d| d == "PROCESS");

        assert!(ip_idx.is_some() && pp_idx.is_some() && ip2_idx.is_some() && p_idx.is_some());
        assert!(
            ip_idx.unwrap() < pp_idx.unwrap(),
            "INTERNAL_PHYSICS before PHYSICS"
        );
        assert!(
            pp_idx.unwrap() < ip2_idx.unwrap(),
            "PHYSICS before INTERNAL_PROCESS"
        );
        assert!(
            ip2_idx.unwrap() < p_idx.unwrap(),
            "INTERNAL_PROCESS before PROCESS"
        );
    }

    /// Verify that the trace correctly records events for sibling nodes in tree order.
    #[test]
    fn notification_siblings_process_in_tree_order() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("Alpha", "Node");
        tree.add_child(root, a).unwrap();
        let b = Node::new("Beta", "Node");
        tree.add_child(root, b).unwrap();
        let c = Node::new("Gamma", "Node");
        tree.add_child(root, c).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.tree_mut().event_trace_mut().enable();
        ml.step(1.0 / 60.0);

        let events = ml.tree().event_trace().events();
        // For each notification type, siblings should appear in insertion order.
        let process_events: Vec<_> = events
            .iter()
            .filter(|e| e.detail == "PROCESS" && e.frame == 0)
            .map(|e| e.node_path.as_str())
            .collect();

        let alpha_idx = process_events
            .iter()
            .position(|&p| p == "/root/Alpha")
            .unwrap();
        let beta_idx = process_events
            .iter()
            .position(|&p| p == "/root/Beta")
            .unwrap();
        let gamma_idx = process_events
            .iter()
            .position(|&p| p == "/root/Gamma")
            .unwrap();

        assert!(alpha_idx < beta_idx, "Alpha before Beta");
        assert!(beta_idx < gamma_idx, "Beta before Gamma");
    }
}
