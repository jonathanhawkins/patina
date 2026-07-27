//! pat-0q2kx.208 (partial): the Performance monitor registry.
//!
//! Godot ground truth: `upstream/godot/main/performance.h` (`Monitor` enum) and
//! `main/performance.cpp` (name table).
//!
//! ⚠️ This is NOT the bead's acceptance test `performance_monitor_registry`,
//! which additionally requires a script calling `Performance.get_monitor(...)`,
//! `MainLoop`/`SceneTree` feeding the counters, and the editor's `/api/monitors`
//! reading the registry. Those all live in files currently carrying other
//! agents' uncommitted work; see the completion report.
//!
//! Tests here use owned registries rather than `performance::global()`, so
//! nothing depends on cross-test ordering.

use gdcore::performance::{Monitor, PerformanceRegistry, ALL_MONITORS, MONITOR_MAX};

/// The discriminants are script-facing API — a script writes
/// `Performance.get_monitor(Performance.OBJECT_NODE_COUNT)` and the integer is
/// what crosses the boundary. Pin the ones the acceptance criterion names plus
/// each block boundary against `performance.h`.
#[test]
fn monitor_indices_match_godots_enum() {
    assert_eq!(MONITOR_MAX, 59, "Godot's MONITOR_MAX with 3D enabled");
    assert_eq!(ALL_MONITORS.len(), MONITOR_MAX);

    assert_eq!(Monitor::TimeFps.to_index(), 0);
    assert_eq!(Monitor::TimeProcess.to_index(), 1);
    assert_eq!(Monitor::TimePhysicsProcess.to_index(), 2);
    assert_eq!(Monitor::MemoryStatic.to_index(), 4);
    assert_eq!(Monitor::ObjectCount.to_index(), 7);
    // The one the acceptance criterion exercises.
    assert_eq!(Monitor::ObjectNodeCount.to_index(), 9);
    assert_eq!(Monitor::ObjectOrphanNodeCount.to_index(), 10);
    assert_eq!(Monitor::RenderTotalDrawCallsInFrame.to_index(), 13);
    assert_eq!(Monitor::Physics2dActiveObjects.to_index(), 17);
    assert_eq!(Monitor::AudioOutputLatency.to_index(), 23);
    // `PIPELINE_*` sits between the deprecated NAVIGATION_* block and the
    // NAVIGATION_2D_* block, which is the easiest place to get the ordering
    // wrong by grouping the navigation entries together.
    assert_eq!(Monitor::PipelineCompilationsCanvas.to_index(), 34);
    assert_eq!(Monitor::Navigation2dActiveMaps.to_index(), 39);
    assert_eq!(Monitor::Navigation3dActiveMaps.to_index(), 49);
    assert_eq!(Monitor::Navigation3dObstacleCount.to_index(), 58);

    // Index and array position must agree, or `from_index` returns the wrong
    // monitor for a correct script call.
    for (i, m) in ALL_MONITORS.iter().enumerate() {
        assert_eq!(
            m.to_index(),
            i,
            "ALL_MONITORS[{i}] has index {}",
            m.to_index()
        );
        assert_eq!(Monitor::from_index(i as i64), Some(*m));
    }
}

/// Out-of-range integers are rejected rather than clamped.
#[test]
fn out_of_range_monitor_indices_are_rejected() {
    assert_eq!(Monitor::from_index(-1), None);
    assert_eq!(Monitor::from_index(MONITOR_MAX as i64), None);
    assert_eq!(Monitor::from_index(9999), None);
    // Clamping would silently answer with `time/fps`, hiding the script bug.
    assert_ne!(Monitor::from_index(9999), Some(Monitor::TimeFps));
}

/// Names come from Godot's table and are unique.
#[test]
fn monitor_names_match_godots_table() {
    assert_eq!(Monitor::TimeFps.name(), "time/fps");
    assert_eq!(Monitor::ObjectNodeCount.name(), "object/nodes");
    assert_eq!(Monitor::ObjectOrphanNodeCount.name(), "object/orphan_nodes");
    // Godot abbreviates this one; "memory/message_buffer_max" is the plausible
    // wrong guess.
    assert_eq!(Monitor::MemoryMessageBufferMax.name(), "memory/msg_buf_max");
    // Draw calls live under `raster/`, while the memory counters live under
    // `video/` — not a shared prefix, despite being adjacent in the enum.
    assert_eq!(
        Monitor::RenderTotalDrawCallsInFrame.name(),
        "raster/total_draw_calls"
    );
    assert_eq!(Monitor::RenderVideoMemUsed.name(), "video/video_mem");
    assert_eq!(
        Monitor::AudioOutputLatency.name(),
        "audio/driver/output_latency"
    );

    let mut names: Vec<&str> = ALL_MONITORS.iter().map(|m| m.name()).collect();
    assert_eq!(names.len(), MONITOR_MAX);
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(
        names.len(),
        before,
        "monitor names must be unique — a duplicate means a row was copied \
         without editing"
    );
}

/// Built-in counters read back what was written, and start at zero.
#[test]
fn built_in_monitors_store_and_accumulate() {
    let mut reg = PerformanceRegistry::new();
    assert_eq!(reg.get(Monitor::ObjectNodeCount), 0.0);

    reg.set(Monitor::ObjectNodeCount, 12.0);
    assert_eq!(reg.get(Monitor::ObjectNodeCount), 12.0);

    // The acceptance criterion's shape: sample, add five nodes, re-sample.
    reg.add(Monitor::ObjectNodeCount, 5.0);
    assert_eq!(reg.get(Monitor::ObjectNodeCount), 17.0);

    reg.add(Monitor::ObjectNodeCount, -3.0);
    assert_eq!(
        reg.get(Monitor::ObjectNodeCount),
        14.0,
        "removals must decrement the same counter"
    );

    // Monitors are independent slots, not one shared cell.
    assert_eq!(reg.get(Monitor::TimeFps), 0.0);
    reg.set(Monitor::TimeFps, 60.0);
    assert_eq!(reg.get(Monitor::ObjectNodeCount), 14.0);
    assert_eq!(reg.get(Monitor::TimeFps), 60.0);
}

/// Custom monitors are evaluated on read, as in Godot — not cached at
/// registration.
#[test]
fn custom_monitors_are_evaluated_on_read() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let mut reg = PerformanceRegistry::new();
    let coins = Arc::new(AtomicU64::new(3));

    let handle = Arc::clone(&coins);
    assert!(reg.add_custom_monitor(
        "game/coins",
        Box::new(move || handle.load(Ordering::Relaxed) as f64),
    ));

    assert!(reg.has_custom_monitor("game/coins"));
    assert_eq!(reg.get_custom_monitor("game/coins"), Some(3.0));

    // The value the game reports changes; the monitor must follow it. A
    // registration-time snapshot would still say 3.
    coins.store(11, Ordering::Relaxed);
    assert_eq!(
        reg.get_custom_monitor("game/coins"),
        Some(11.0),
        "a custom monitor must be called at read time, not cached"
    );

    // Absent is distinguishable from a monitor that returns 0.0.
    assert_eq!(reg.get_custom_monitor("game/absent"), None);
    assert!(!reg.has_custom_monitor("game/absent"));
    assert!(reg.add_custom_monitor("game/zero", Box::new(|| 0.0)));
    assert_eq!(reg.get_custom_monitor("game/zero"), Some(0.0));
}

/// Duplicate registration is refused, and names report in registration order.
#[test]
fn custom_monitor_names_are_ordered_and_unique() {
    let mut reg = PerformanceRegistry::new();
    assert!(reg.add_custom_monitor("game/c", Box::new(|| 1.0)));
    assert!(reg.add_custom_monitor("game/a", Box::new(|| 2.0)));
    assert!(reg.add_custom_monitor("game/b", Box::new(|| 3.0)));

    // Registration order, NOT sorted and not HashMap order — the latter varies
    // between runs and would make editor monitor lists jump around.
    assert_eq!(
        reg.custom_monitor_names(),
        vec!["game/c", "game/a", "game/b"]
    );

    // Godot errors on a duplicate rather than replacing the callback.
    assert!(
        !reg.add_custom_monitor("game/a", Box::new(|| 99.0)),
        "re-registering an existing name must be refused"
    );
    assert_eq!(
        reg.get_custom_monitor("game/a"),
        Some(2.0),
        "the original callback must survive a refused duplicate"
    );
    assert_eq!(reg.custom_monitor_names().len(), 3);

    assert!(reg.remove_custom_monitor("game/a"));
    assert!(!reg.has_custom_monitor("game/a"));
    assert_eq!(reg.custom_monitor_names(), vec!["game/c", "game/b"]);
    assert!(
        !reg.remove_custom_monitor("game/a"),
        "removing an absent monitor reports false"
    );

    // The name is reusable once removed.
    assert!(reg.add_custom_monitor("game/a", Box::new(|| 42.0)));
    assert_eq!(reg.get_custom_monitor("game/a"), Some(42.0));
    assert_eq!(
        reg.custom_monitor_names(),
        vec!["game/c", "game/b", "game/a"]
    );
}

/// `reset` clears both halves, so a new play session cannot inherit numbers.
#[test]
fn reset_clears_counters_and_custom_monitors() {
    let mut reg = PerformanceRegistry::new();
    reg.set(Monitor::ObjectNodeCount, 25.0);
    reg.set(Monitor::TimeFps, 60.0);
    reg.add_custom_monitor("game/coins", Box::new(|| 7.0));

    reg.reset();

    assert_eq!(reg.get(Monitor::ObjectNodeCount), 0.0);
    assert_eq!(reg.get(Monitor::TimeFps), 0.0);
    assert!(!reg.has_custom_monitor("game/coins"));
    assert!(
        reg.custom_monitor_names().is_empty(),
        "reset must clear the name order too, not just the map — a stale name \
         would report a monitor that no longer resolves"
    );
}

/// The timing feed converts cumulative clocks into per-frame monitors. This is
/// the conversion stage (2) must not skip: assigning the totals directly gives
/// a monitor that climbs forever.
#[test]
fn frame_feed_reports_last_frame_time_not_the_running_total() {
    use gdcore::performance::FrameMonitorFeed;

    let mut reg = PerformanceRegistry::new();
    let mut feed = FrameMonitorFeed::new();
    let dt = 1.0 / 60.0;

    // Ten frames, exactly as MainLoop would report them: totals, not deltas.
    let mut process_total = 0.0;
    let mut physics_total = 0.0;
    for _ in 0..10 {
        process_total += dt;
        physics_total += dt;
        feed.record_frame(&mut reg, process_total, physics_total);
    }

    // The monitor is one frame, ~0.0167 s -- NOT the 0.167 s total.
    assert!(
        (reg.get(Monitor::TimeProcess) - dt).abs() < 1e-9,
        "TIME_PROCESS must be the last frame's time, got {} (the running \
         total would be {process_total})",
        reg.get(Monitor::TimeProcess)
    );
    assert!(
        reg.get(Monitor::TimeProcess) < process_total / 2.0,
        "a monitor holding the cumulative total would pass a naive \
         'greater than zero' check; it must not"
    );
    assert!((reg.get(Monitor::TimePhysicsProcess) - dt).abs() < 1e-9);

    // A frame with a different cost is reported at its own cost, not smoothed.
    process_total += 0.5;
    feed.record_frame(&mut reg, process_total, physics_total);
    assert!(
        (reg.get(Monitor::TimeProcess) - 0.5).abs() < 1e-9,
        "a slow frame must report its own duration, got {}",
        reg.get(Monitor::TimeProcess)
    );
    assert_eq!(
        reg.get(Monitor::TimePhysicsProcess),
        0.0,
        "physics did not advance on that frame, so its monitor reads zero"
    );
}

/// FPS is a trailing-second frame count, not `1.0 / delta`.
#[test]
fn frame_feed_counts_frames_in_the_trailing_second() {
    use gdcore::performance::FrameMonitorFeed;

    let mut reg = PerformanceRegistry::new();
    let mut feed = FrameMonitorFeed::new();
    let dt = 1.0 / 60.0;

    // Three full seconds so the window is saturated and well clear of its
    // boundary -- a count taken exactly at 1.0 s is a float coin-flip.
    let mut total = 0.0;
    for _ in 0..180 {
        total += dt;
        feed.record_frame(&mut reg, total, 0.0);
    }
    let fps = reg.get(Monitor::TimeFps);
    // Range, not equality: the window edge is compared against an accumulated
    // float, so the count can land one frame either side of 61.
    assert!(
        (55.0..=63.0).contains(&fps),
        "a steady 60 Hz loop must report ~60 fps, got {fps}"
    );
    assert_eq!(fps, feed.fps());

    // Halving the frame rate must halve the count, which `1.0 / delta` would
    // also do -- so pair it with the window check below.
    let mut slow_reg = PerformanceRegistry::new();
    let mut slow = FrameMonitorFeed::new();
    let mut t = 0.0;
    for _ in 0..90 {
        t += 1.0 / 30.0;
        slow.record_frame(&mut slow_reg, t, 0.0);
    }
    let slow_fps = slow_reg.get(Monitor::TimeFps);
    assert!(
        (27.0..=33.0).contains(&slow_fps),
        "a 30 Hz loop must report ~30 fps, got {slow_fps}"
    );

    // One long stall: `1.0 / delta` would say 2 fps, but only a single frame
    // occurred in the trailing second, so the count is 1.
    let mut stall_reg = PerformanceRegistry::new();
    let mut stall = FrameMonitorFeed::new();
    stall.record_frame(&mut stall_reg, 10.0, 0.0);
    assert_eq!(
        stall_reg.get(Monitor::TimeFps),
        1.0,
        "the window must drop frames older than a second"
    );
}

/// A restarted loop whose totals go backwards must not report a negative
/// duration.
#[test]
fn frame_feed_survives_totals_going_backwards() {
    use gdcore::performance::FrameMonitorFeed;

    let mut reg = PerformanceRegistry::new();
    let mut feed = FrameMonitorFeed::new();
    feed.record_frame(&mut reg, 5.0, 5.0);
    feed.record_frame(&mut reg, 1.0, 1.0);

    assert!(
        reg.get(Monitor::TimeProcess) >= 0.0,
        "a rewound clock must clamp to zero, not report negative time (got {})",
        reg.get(Monitor::TimeProcess)
    );
    assert!(reg.get(Monitor::TimePhysicsProcess) >= 0.0);

    feed.reset();
    assert_eq!(feed.fps(), 0.0, "reset clears the frame window");
}

/// The process-wide registry is reachable and shared between lookups.
#[test]
fn global_registry_is_shared() {
    use gdcore::performance::global;

    // A monitor no other test in this binary touches, so this stays true even
    // if the harness runs tests in threads rather than processes.
    global()
        .lock()
        .unwrap()
        .set(Monitor::Navigation3dObstacleCount, 4.0);
    assert_eq!(
        global()
            .lock()
            .unwrap()
            .get(Monitor::Navigation3dObstacleCount),
        4.0,
        "global() must return the same registry on each call"
    );
}
