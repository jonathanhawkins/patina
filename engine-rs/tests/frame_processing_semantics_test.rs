//! pat-i5c: Frame processing semantics tests.
//!
//! Validates that Patina's frame processing matches Godot contracts:
//! - Frame sequencing order (INTERNAL_PHYSICS -> PHYSICS -> INTERNAL_PROCESS -> PROCESS)
//! - Physics process fires correct number of times per frame
//! - Pause behavior (no process/physics_process during pause)
//! - Fixed-step accumulator behavior
//! - Frame-by-frame property evolution is captured and deterministic
//! - Per-frame trace records capture notifications, script calls, and node properties

use gdscene::main_loop::{FrameTrace, MainLoop};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a MainLoop with root -> Child.
fn make_loop_with_child() -> (MainLoop, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = Node::new("Child", "Node");
    let child_id = tree.add_child(root, child).unwrap();
    (MainLoop::new(tree), child_id)
}

/// Build a MainLoop with root -> Parent -> Child.
fn make_loop_with_hierarchy() -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = Node::new("Parent", "Node");
    let parent_id = tree.add_child(root, parent).unwrap();
    let child = Node::new("Child", "Node");
    let child_id = tree.add_child(parent_id, child).unwrap();
    (MainLoop::new(tree), parent_id, child_id)
}

/// Count notification events with the given detail in a frame trace.
fn count_notification(trace: &FrameTrace, detail: &str) -> usize {
    trace
        .frames
        .iter()
        .flat_map(|f| f.events.iter())
        .filter(|e| e.event_type == TraceEventType::Notification && e.detail == detail)
        .count()
}

/// Count notification events with the given detail in a single frame.
fn count_notification_in_frame(trace: &FrameTrace, frame_idx: usize, detail: &str) -> usize {
    trace.frames[frame_idx]
        .events
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification && e.detail == detail)
        .count()
}

/// Extract notification details from a single frame's events.
fn notification_details_in_frame(trace: &FrameTrace, frame_idx: usize) -> Vec<&str> {
    trace.frames[frame_idx]
        .events
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .map(|e| e.detail.as_str())
        .collect()
}

// ===========================================================================
// 1. FrameTrace / FrameRecord structure tests
// ===========================================================================

#[test]
fn step_traced_returns_frame_record() {
    let (mut ml, _) = make_loop_with_child();
    let record = ml.step_traced(1.0 / 60.0);

    assert_eq!(record.frame_number, 1);
    assert!((record.delta - 1.0 / 60.0).abs() < 1e-12);
    assert_eq!(record.physics_ticks, 1);
    assert!(!record.paused);
}

#[test]
fn run_frames_traced_returns_correct_count() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(10, 1.0 / 60.0);

    assert_eq!(trace.len(), 10);
    assert!(!trace.is_empty());
}

#[test]
fn frame_numbers_are_sequential() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(5, 1.0 / 60.0);

    for (i, frame) in trace.frames.iter().enumerate() {
        assert_eq!(
            frame.frame_number,
            (i + 1) as u64,
            "frame {i}: expected frame_number {}",
            i + 1
        );
    }
}

#[test]
fn cumulative_times_increase_monotonically() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(10, 1.0 / 60.0);

    let mut prev_process = 0.0;
    let mut prev_physics = 0.0;
    for (i, frame) in trace.frames.iter().enumerate() {
        assert!(
            frame.cumulative_process_time >= prev_process,
            "frame {i}: process time went backwards"
        );
        assert!(
            frame.cumulative_physics_time >= prev_physics,
            "frame {i}: physics time went backwards"
        );
        prev_process = frame.cumulative_process_time;
        prev_physics = frame.cumulative_physics_time;
    }
}

// ===========================================================================
// 2. Godot frame-sequencing contract tests
// ===========================================================================

#[test]
fn frame_sequence_matches_godot_contract() {
    // Godot contract: per frame with 1 physics tick, the order is:
    // INTERNAL_PHYSICS_PROCESS -> PHYSICS_PROCESS -> INTERNAL_PROCESS -> PROCESS
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(1, 1.0 / 60.0);

    let notifications = notification_details_in_frame(&trace, 0);

    // Filter to the per-frame processing notifications (skip root node's copies).
    // Each notification appears once per node. With root + Child, we expect
    // pairs. Check that the pattern for a single node (Child) is correct.
    let child_notifications: Vec<&str> = trace.frames[0]
        .events
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification && e.node_path.ends_with("/Child"))
        .map(|e| e.detail.as_str())
        .collect();

    assert_eq!(
        child_notifications,
        vec![
            "INTERNAL_PHYSICS_PROCESS",
            "PHYSICS_PROCESS",
            "INTERNAL_PROCESS",
            "PROCESS",
        ],
        "Child should receive notifications in Godot contract order"
    );
}

#[test]
fn frame_sequence_consistent_across_multiple_frames() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(5, 1.0 / 60.0);

    for frame_idx in 0..5 {
        let child_notifications: Vec<&str> = trace.frames[frame_idx]
            .events
            .iter()
            .filter(|e| {
                e.event_type == TraceEventType::Notification && e.node_path.ends_with("/Child")
            })
            .map(|e| e.detail.as_str())
            .collect();

        assert_eq!(
            child_notifications,
            vec![
                "INTERNAL_PHYSICS_PROCESS",
                "PHYSICS_PROCESS",
                "INTERNAL_PROCESS",
                "PROCESS",
            ],
            "frame {frame_idx}: notification order must match Godot contract"
        );
    }
}

#[test]
fn parent_processes_before_child_in_each_frame() {
    let (mut ml, _, _) = make_loop_with_hierarchy();
    let trace = ml.run_frames_traced(3, 1.0 / 60.0);

    for frame_idx in 0..3 {
        // For each notification type, Parent should appear before Child.
        for notification in &[
            "INTERNAL_PHYSICS_PROCESS",
            "PHYSICS_PROCESS",
            "INTERNAL_PROCESS",
            "PROCESS",
        ] {
            let positions: Vec<(usize, &str)> = trace.frames[frame_idx]
                .events
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    e.event_type == TraceEventType::Notification && e.detail == *notification
                })
                .map(|(i, e)| (i, e.node_path.as_str()))
                .collect();

            let parent_pos = positions
                .iter()
                .find(|(_, p)| p.ends_with("/Parent"))
                .map(|(i, _)| *i);
            let child_pos = positions
                .iter()
                .find(|(_, p)| p.ends_with("/Child"))
                .map(|(i, _)| *i);

            if let (Some(pp), Some(cp)) = (parent_pos, child_pos) {
                assert!(
                    pp < cp,
                    "frame {frame_idx}, {notification}: Parent (pos {pp}) must process before Child (pos {cp})"
                );
            }
        }
    }
}

// ===========================================================================
// 3. Physics tick count tests
// ===========================================================================

#[test]
fn one_physics_tick_per_frame_at_matching_rate() {
    // 60 TPS physics + 1/60 delta = exactly 1 physics tick per frame.
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(10, 1.0 / 60.0);

    for (i, frame) in trace.frames.iter().enumerate() {
        assert_eq!(frame.physics_ticks, 1, "frame {i}: expected 1 physics tick");
    }
    assert_eq!(trace.total_physics_ticks(), 10);
}

#[test]
fn two_physics_ticks_at_half_framerate() {
    // 60 TPS physics + 1/30 delta = 2 physics ticks per frame.
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(5, 1.0 / 30.0);

    for (i, frame) in trace.frames.iter().enumerate() {
        assert_eq!(
            frame.physics_ticks, 2,
            "frame {i}: expected 2 physics ticks"
        );
    }
    assert_eq!(trace.total_physics_ticks(), 10);
}

#[test]
fn zero_physics_ticks_with_tiny_delta() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(3, 1e-10);

    for (i, frame) in trace.frames.iter().enumerate() {
        assert_eq!(
            frame.physics_ticks, 0,
            "frame {i}: expected 0 physics ticks with tiny delta"
        );
    }
}

#[test]
fn physics_accumulator_carries_over() {
    // At 60 TPS, physics_dt = 1/60 ~= 0.01667
    // With delta = 0.5/60 = 0.00833, two frames accumulate to one tick.
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(4, 0.5 / 60.0);

    // Frame 0: acc=0.00833, 0 ticks
    // Frame 1: acc=0.01667, 1 tick (acc=0)
    // Frame 2: acc=0.00833, 0 ticks
    // Frame 3: acc=0.01667, 1 tick (acc=0)
    assert_eq!(trace.frames[0].physics_ticks, 0);
    assert_eq!(trace.frames[1].physics_ticks, 1);
    assert_eq!(trace.frames[2].physics_ticks, 0);
    assert_eq!(trace.frames[3].physics_ticks, 1);
    assert_eq!(trace.total_physics_ticks(), 2);
}

#[test]
fn physics_notification_count_matches_tick_count() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(5, 1.0 / 60.0);

    for (i, frame) in trace.frames.iter().enumerate() {
        let physics_notif_count = frame
            .events
            .iter()
            .filter(|e| {
                e.event_type == TraceEventType::Notification
                    && e.detail == "PHYSICS_PROCESS"
                    && e.node_path.ends_with("/Child")
            })
            .count();
        assert_eq!(
            physics_notif_count, frame.physics_ticks as usize,
            "frame {i}: PHYSICS_PROCESS notification count ({physics_notif_count}) != physics_ticks ({})",
            frame.physics_ticks
        );
    }
}

#[test]
fn spiral_of_death_guard_caps_physics_ticks() {
    let (mut ml, _) = make_loop_with_child();
    ml.set_max_physics_steps_per_frame(4);
    let trace = ml.run_frames_traced(1, 1.0); // 1 second at 60 TPS = 60 ticks, capped to 4

    assert_eq!(trace.frames[0].physics_ticks, 4);
}

#[test]
fn fractional_physics_ratio_produces_correct_ticks() {
    // 50 FPS with 60 TPS physics: uneven tick distribution across frames.
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(5, 1.0 / 50.0);

    // Total should be 6 (5 frames * 1/50 sec = 0.1 sec / (1/60) = 6 ticks)
    assert_eq!(trace.total_physics_ticks(), 6);

    // Each frame gets 1 or 2 ticks.
    for frame in &trace.frames {
        assert!(
            frame.physics_ticks == 1 || frame.physics_ticks == 2,
            "each frame should have 1 or 2 physics ticks at 50fps/60tps, got {}",
            frame.physics_ticks
        );
    }
}

// ===========================================================================
// 4. Pause behavior tests
// ===========================================================================

#[test]
fn paused_frames_still_run_physics_ticks() {
    let (mut ml, _) = make_loop_with_child();
    ml.set_paused(true);
    let trace = ml.run_frames_traced(3, 1.0 / 60.0);

    for (i, frame) in trace.frames.iter().enumerate() {
        // Physics ticks still run when paused (Always-mode nodes may need them);
        // per-node filtering handles pause mode at the notification level.
        assert!(
            frame.physics_ticks >= 1,
            "frame {i}: physics ticks should still run when paused"
        );
        assert!(frame.paused, "frame {i}: should be marked paused");
    }
}

#[test]
fn paused_frames_have_no_process_notifications() {
    let (mut ml, _) = make_loop_with_child();
    ml.set_paused(true);
    let trace = ml.run_frames_traced(3, 1.0 / 60.0);

    assert_eq!(count_notification(&trace, "PROCESS"), 0);
    assert_eq!(count_notification(&trace, "PHYSICS_PROCESS"), 0);
    assert_eq!(count_notification(&trace, "INTERNAL_PROCESS"), 0);
    assert_eq!(count_notification(&trace, "INTERNAL_PHYSICS_PROCESS"), 0);
}

#[test]
fn unpausing_restores_normal_processing() {
    let (mut ml, _) = make_loop_with_child();

    // 2 paused frames.
    ml.set_paused(true);
    let paused_trace = ml.run_frames_traced(2, 1.0 / 60.0);

    // 2 unpaused frames.
    ml.set_paused(false);
    let unpaused_trace = ml.run_frames_traced(2, 1.0 / 60.0);

    // Paused frames: no processing notifications.
    assert_eq!(count_notification(&paused_trace, "PROCESS"), 0);

    // Unpaused frames: normal processing resumes.
    // Each frame dispatches PROCESS to every node (root + Child = 2 nodes),
    // so 2 frames * 2 nodes = 4 PROCESS notifications.
    assert_eq!(count_notification(&unpaused_trace, "PROCESS"), 4);
    assert!(count_notification(&unpaused_trace, "PHYSICS_PROCESS") > 0);
}

#[test]
fn paused_physics_time_still_advances() {
    // With per-node pause mode, physics runs even when paused (to support
    // Always-mode nodes). The physics time accumulator advances normally.
    let (mut ml, _) = make_loop_with_child();
    ml.set_paused(true);
    let trace = ml.run_frames_traced(5, 1.0 / 60.0);

    let last = trace.frames.last().unwrap();
    assert!(
        last.cumulative_physics_time > 0.0,
        "physics time should advance while paused (per-node filtering handles pause mode)"
    );
}

#[test]
fn paused_process_time_still_advances() {
    // Godot's process_time advances even when paused (the frame counter still ticks).
    let (mut ml, _) = make_loop_with_child();
    ml.set_paused(true);
    let trace = ml.run_frames_traced(3, 1.0 / 60.0);

    let last = trace.frames.last().unwrap();
    assert!(
        last.cumulative_process_time > 0.0,
        "process time should still advance while paused"
    );
}

// ===========================================================================
// 5. Node property snapshot tests
// ===========================================================================

#[test]
fn node_snapshots_captured_each_frame() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(3, 1.0 / 60.0);

    for (i, frame) in trace.frames.iter().enumerate() {
        assert!(
            !frame.node_snapshots.is_empty(),
            "frame {i}: should have node snapshots"
        );
    }
}

#[test]
fn node_snapshot_paths_match_tree_structure() {
    let (mut ml, _, _) = make_loop_with_hierarchy();
    let trace = ml.run_frames_traced(1, 1.0 / 60.0);

    let paths: Vec<&str> = trace.frames[0]
        .node_snapshots
        .iter()
        .map(|s| s.path.as_str())
        .collect();

    // Should include root, Parent, and Child.
    assert!(
        paths.iter().any(|p| p.contains("root")),
        "should contain root"
    );
    assert!(
        paths.iter().any(|p| p.contains("Parent")),
        "should contain Parent"
    );
    assert!(
        paths.iter().any(|p| p.contains("Child")),
        "should contain Child"
    );
}

#[test]
fn node_snapshot_class_names_are_populated() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(1, 1.0 / 60.0);

    for snap in &trace.frames[0].node_snapshots {
        assert!(
            !snap.class_name.is_empty(),
            "class_name for {} should not be empty",
            snap.path
        );
    }
}

#[test]
fn property_evolution_with_set_property() {
    // Set a property on a node and verify it appears in the snapshot.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = Node::new("Mover", "Node2D");
    let child_id = tree.add_child(root, child).unwrap();

    // Set initial position property.
    tree.get_node_mut(child_id)
        .unwrap()
        .set_property("position_x", gdvariant::Variant::Float(0.0));

    let mut ml = MainLoop::new(tree);

    // Frame 1: position_x = 0.0
    let trace1 = ml.step_traced(1.0 / 60.0);
    let mover_snap = trace1
        .node_snapshots
        .iter()
        .find(|s| s.path.contains("Mover"))
        .expect("Mover snapshot");
    assert_eq!(
        mover_snap.properties.get("position_x"),
        Some(&gdvariant::Variant::Float(0.0))
    );

    // Mutate position between frames.
    ml.tree_mut()
        .get_node_mut(child_id)
        .unwrap()
        .set_property("position_x", gdvariant::Variant::Float(10.0));

    // Frame 2: position_x = 10.0
    let trace2 = ml.step_traced(1.0 / 60.0);
    let mover_snap2 = trace2
        .node_snapshots
        .iter()
        .find(|s| s.path.contains("Mover"))
        .expect("Mover snapshot frame 2");
    assert_eq!(
        mover_snap2.properties.get("position_x"),
        Some(&gdvariant::Variant::Float(10.0))
    );
}

// ===========================================================================
// 6. Determinism tests
// ===========================================================================

#[test]
fn traced_execution_is_deterministic() {
    let run = || {
        let (mut ml, _) = make_loop_with_child();
        ml.run_frames_traced(20, 1.0 / 60.0)
    };

    let trace1 = run();
    let trace2 = run();

    assert_eq!(trace1.len(), trace2.len());
    for (i, (f1, f2)) in trace1.frames.iter().zip(trace2.frames.iter()).enumerate() {
        assert_eq!(f1.frame_number, f2.frame_number, "frame {i}: frame_number");
        assert_eq!(f1.delta, f2.delta, "frame {i}: delta");
        assert_eq!(
            f1.physics_ticks, f2.physics_ticks,
            "frame {i}: physics_ticks"
        );
        assert_eq!(f1.events.len(), f2.events.len(), "frame {i}: event count");
        for (j, (e1, e2)) in f1.events.iter().zip(f2.events.iter()).enumerate() {
            assert_eq!(
                e1.event_type, e2.event_type,
                "frame {i}, event {j}: event_type"
            );
            assert_eq!(
                e1.node_path, e2.node_path,
                "frame {i}, event {j}: node_path"
            );
            assert_eq!(e1.detail, e2.detail, "frame {i}, event {j}: detail");
        }
    }
}

#[test]
fn frame_trace_total_physics_ticks_helper() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(10, 1.0 / 60.0);
    assert_eq!(trace.total_physics_ticks(), 10);
}

#[test]
fn frame_trace_all_notification_details_helper() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(1, 1.0 / 60.0);
    let details = trace.all_notification_details();
    assert!(details.contains(&"PROCESS"));
    assert!(details.contains(&"PHYSICS_PROCESS"));
    assert!(details.contains(&"INTERNAL_PROCESS"));
    assert!(details.contains(&"INTERNAL_PHYSICS_PROCESS"));
}

// ===========================================================================
// 7. Edge cases
// ===========================================================================

#[test]
fn zero_delta_frame_trace() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(1, 0.0);

    assert_eq!(trace.frames[0].physics_ticks, 0);
    assert_eq!(trace.frames[0].delta, 0.0);
    // PROCESS still fires even with zero delta.
    assert!(count_notification_in_frame(&trace, 0, "PROCESS") > 0);
}

#[test]
fn empty_trace_when_zero_frames() {
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(0, 1.0 / 60.0);

    assert!(trace.is_empty());
    assert_eq!(trace.len(), 0);
    assert_eq!(trace.total_physics_ticks(), 0);
}

#[test]
fn mixed_delta_traced_frames() {
    // Run frames with different deltas and verify each is recorded correctly.
    let (mut ml, _) = make_loop_with_child();

    let r1 = ml.step_traced(1.0 / 60.0);
    assert_eq!(r1.physics_ticks, 1);
    assert!((r1.delta - 1.0 / 60.0).abs() < 1e-12);

    let r2 = ml.step_traced(2.0 / 60.0);
    assert_eq!(r2.physics_ticks, 2);
    assert!((r2.delta - 2.0 / 60.0).abs() < 1e-12);

    let r3 = ml.step_traced(1e-10);
    assert_eq!(r3.physics_ticks, 0);
}

#[test]
fn process_notification_fires_exactly_once_per_node_per_frame() {
    let (mut ml, _, _) = make_loop_with_hierarchy();
    let trace = ml.run_frames_traced(5, 1.0 / 60.0);

    for (i, frame) in trace.frames.iter().enumerate() {
        // Count PROCESS notifications per unique node path.
        let process_by_node: std::collections::HashMap<&str, usize> = frame
            .events
            .iter()
            .filter(|e| e.event_type == TraceEventType::Notification && e.detail == "PROCESS")
            .fold(std::collections::HashMap::new(), |mut acc, e| {
                *acc.entry(e.node_path.as_str()).or_insert(0) += 1;
                acc
            });

        for (path, count) in &process_by_node {
            assert_eq!(
                *count, 1,
                "frame {i}: node {path} should receive exactly 1 PROCESS, got {count}"
            );
        }
    }
}

#[test]
fn internal_process_fires_exactly_once_per_node_per_frame() {
    let (mut ml, _, _) = make_loop_with_hierarchy();
    let trace = ml.run_frames_traced(5, 1.0 / 60.0);

    for (i, frame) in trace.frames.iter().enumerate() {
        let internal_by_node: std::collections::HashMap<&str, usize> = frame
            .events
            .iter()
            .filter(|e| {
                e.event_type == TraceEventType::Notification && e.detail == "INTERNAL_PROCESS"
            })
            .fold(std::collections::HashMap::new(), |mut acc, e| {
                *acc.entry(e.node_path.as_str()).or_insert(0) += 1;
                acc
            });

        for (path, count) in &internal_by_node {
            assert_eq!(
                *count, 1,
                "frame {i}: node {path} should receive exactly 1 INTERNAL_PROCESS, got {count}"
            );
        }
    }
}

#[test]
fn multiple_physics_ticks_interleave_internal_and_user() {
    // With 2 physics ticks per frame, each tick should have both
    // INTERNAL_PHYSICS_PROCESS and PHYSICS_PROCESS.
    let (mut ml, _) = make_loop_with_child();
    let trace = ml.run_frames_traced(1, 2.0 / 60.0);

    let child_events: Vec<&str> = trace.frames[0]
        .events
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && e.node_path.ends_with("/Child")
                && (e.detail == "INTERNAL_PHYSICS_PROCESS"
                    || e.detail == "PHYSICS_PROCESS"
                    || e.detail == "INTERNAL_PROCESS"
                    || e.detail == "PROCESS")
        })
        .map(|e| e.detail.as_str())
        .collect();

    // Expected: 2 physics ticks then 1 process:
    // INT_PHYS, PHYS, INT_PHYS, PHYS, INT_PROC, PROC
    assert_eq!(
        child_events,
        vec![
            "INTERNAL_PHYSICS_PROCESS",
            "PHYSICS_PROCESS",
            "INTERNAL_PHYSICS_PROCESS",
            "PHYSICS_PROCESS",
            "INTERNAL_PROCESS",
            "PROCESS",
        ]
    );
}
