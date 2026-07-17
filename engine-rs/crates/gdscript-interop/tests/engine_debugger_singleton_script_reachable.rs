//! pat-nzgdu.176 — `EngineDebugger` is script-reachable, backed by the real
//! `gdcore::debugger`.
//!
//! **This does NOT satisfy the bead's acceptance.** That needs the runner→editor
//! protocol carrying a break message for a `.tscn` run
//! (`engine_debugger_remote_protocol_e2e`). This covers the bead's other clause
//! — the singleton dispatch arm over gdcore's debugger — plus the
//! `EngineDebugger.is_active()` half of the acceptance. See the bead notes.
//!
//! Before this, `EngineDebugger` appeared NOWHERE in the workspace: scripts had
//! no debugger at all, while `gdcore::debugger` (536 lines) and
//! `gdcore::debug_protocol` (869 lines) compiled but were referenced only by
//! gdcore's own `lib.rs`. Two healthy islands, no bridge.
//!
//! Signatures diffed against Godot 4.6.1.stable's `extension_api.json`.
//!
//! These are integration tests on purpose: `-p gdscript-interop --lib` cannot
//! pass (a pre-existing `fuzz_deeply_nested_parens` stack overflow cancels the
//! run), so unit tests here would be unverifiable.

use gdscript_interop::interpreter::{
    drain_engine_debugger_messages, engine_debugger_attach, engine_debugger_detach,
    engine_debugger_profiler_frames, engine_debugger_set_skipping_breakpoints,
    with_engine_debugger, Interpreter, RuntimeErrorKind,
};
use gdvariant::Variant;

/// Runs `src` and returns its `return` value.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

/// Runs `src` expecting a runtime error.
fn run_err(src: &str) -> gdscript_interop::interpreter::RuntimeError {
    let mut interp = Interpreter::new();
    interp.run(src).expect_err("script should have failed")
}

/// The acceptance's `is_active()` half: false on a plain run (Godot only
/// activates the debugger under `--remote-debug`), true once a session attaches.
#[test]
fn is_active_reports_the_live_debug_session() {
    engine_debugger_detach();
    assert_eq!(
        run_val("return EngineDebugger.is_active()\n"),
        Variant::Bool(false),
        "a normal run is not being debugged — Godot reports false"
    );

    engine_debugger_attach();
    assert_eq!(
        run_val("return EngineDebugger.is_active()\n"),
        Variant::Bool(true),
        "under a debug session a script must see is_active() == true"
    );

    engine_debugger_detach();
    assert_eq!(
        run_val("return EngineDebugger.is_active()\n"),
        Variant::Bool(false),
        "detaching ends the session"
    );
}

/// The singleton dispatches rather than falling through to the
/// undefined-identifier catch-all. Any missing arm would surface as an Err and
/// panic the harness, so reaching the return proves every call dispatched.
#[test]
fn engine_debugger_singleton_script_reachable() {
    engine_debugger_detach();
    let v = run_val(
        "var calls = 0\n\
         EngineDebugger.insert_breakpoint(12, \"res://player.gd\")\n\
         calls += 1\n\
         EngineDebugger.is_breakpoint(12, \"res://player.gd\")\n\
         calls += 1\n\
         EngineDebugger.remove_breakpoint(12, \"res://player.gd\")\n\
         calls += 1\n\
         EngineDebugger.clear_breakpoints()\n\
         calls += 1\n\
         EngineDebugger.set_depth(3)\n\
         calls += 1\n\
         EngineDebugger.set_lines_left(7)\n\
         calls += 1\n\
         EngineDebugger.register_profiler(\"dispatch_probe\", null)\n\
         calls += 1\n\
         EngineDebugger.profiler_enable(\"dispatch_probe\", true)\n\
         calls += 1\n\
         EngineDebugger.profiler_add_frame_data(\"dispatch_probe\", [1, 2])\n\
         calls += 1\n\
         EngineDebugger.register_message_capture(\"my_cap\", null)\n\
         calls += 1\n\
         EngineDebugger.send_message(\"hello\", [])\n\
         calls += 1\n\
         EngineDebugger.line_poll()\n\
         calls += 1\n\
         return calls\n",
    );
    assert_eq!(v, Variant::Int(12), "all twelve calls dispatched");

    // An unknown method still errors, so this is a real dispatch table and not
    // a catch-all that silently swallows typos.
    let err = run_err("return EngineDebugger.__no_such_method__()\n");
    assert!(
        matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)),
        "got: {err:?}"
    );
}

/// ⚠️ Godot spells it `insert_breakpoint(line: int, source: StringName)` —
/// **line FIRST**, unlike `BreakpointLocation::new(script, line)`. Both come out
/// of `&[Variant]`, so a swap is not a compile error: it would silently register
/// a breakpoint on a script named "12" at the line named "res://player.gd".
/// This asserts against the real gdcore debugger, not the interpreter's echo.
#[test]
fn insert_breakpoint_takes_line_first_source_second() {
    with_engine_debugger(|d| d.clear_breakpoints());
    run_val(
        "EngineDebugger.insert_breakpoint(12, \"res://player.gd\")\n\
         return 0\n",
    );

    let lines = with_engine_debugger(|d| d.breakpoints_for_script("res://player.gd"));
    assert_eq!(
        lines,
        vec![12],
        "the breakpoint must land on line 12 of res://player.gd — if the args \
         were swapped, the script name would be \"12\""
    );
    // The swapped reading would have produced a script literally named "12".
    assert!(
        with_engine_debugger(|d| d.breakpoints_for_script("12")).is_empty(),
        "no breakpoint may be registered against a script named \"12\""
    );

    // And the script's own view agrees with gdcore's.
    assert_eq!(
        run_val("return EngineDebugger.is_breakpoint(12, \"res://player.gd\")\n"),
        Variant::Bool(true)
    );
    assert_eq!(
        run_val("return EngineDebugger.is_breakpoint(99, \"res://player.gd\")\n"),
        Variant::Bool(false),
        "a line with no breakpoint reports false"
    );
}

/// Breakpoints set from a script must reach the same gdcore debugger the runner
/// drives — otherwise the singleton is a private mock and the editor would never
/// see them.
#[test]
fn breakpoints_round_trip_through_the_real_gdcore_debugger() {
    with_engine_debugger(|d| d.clear_breakpoints());
    run_val(
        "EngineDebugger.insert_breakpoint(3, \"res://a.gd\")\n\
         EngineDebugger.insert_breakpoint(9, \"res://a.gd\")\n\
         EngineDebugger.insert_breakpoint(4, \"res://b.gd\")\n\
         return 0\n",
    );
    assert_eq!(
        with_engine_debugger(|d| d.breakpoints_for_script("res://a.gd")),
        vec![3, 9]
    );
    assert_eq!(
        with_engine_debugger(|d| d.breakpoints_for_script("res://b.gd")),
        vec![4]
    );

    // should_break is the hook the interpreter will call per line — proving the
    // scripted breakpoint actually governs execution, not just bookkeeping.
    //
    // Order matters: should_break() only fires while Running and calls pause()
    // on a hit. Asserting the miss AFTER the hit would pass merely because the
    // debugger was already Paused — vacuously true.
    engine_debugger_attach();
    assert!(
        !with_engine_debugger(|d| d.should_break("res://a.gd", 5)),
        "a line with no breakpoint must not stop the runner"
    );
    assert!(
        with_engine_debugger(|d| d.should_break("res://a.gd", 3)),
        "a breakpoint set from a script must stop the runner"
    );
    // ...and that hit really did pause the run.
    assert!(
        !with_engine_debugger(|d| d.should_break("res://a.gd", 3)),
        "already Paused — the runner does not re-break until it resumes"
    );

    run_val("EngineDebugger.remove_breakpoint(3, \"res://a.gd\")\nreturn 0\n");
    assert_eq!(
        with_engine_debugger(|d| d.breakpoints_for_script("res://a.gd")),
        vec![9],
        "removing line 3 must leave line 9 alone"
    );

    run_val("EngineDebugger.clear_breakpoints()\nreturn 0\n");
    assert!(with_engine_debugger(|d| d.breakpoints_for_script("res://a.gd")).is_empty());
    assert!(with_engine_debugger(|d| d.breakpoints_for_script("res://b.gd")).is_empty());
    engine_debugger_detach();
}

/// ⚠️ `Debugger::detach()` deliberately KEEPS breakpoints while clearing runtime
/// state — an editor that drops its session must not lose the user's
/// breakpoints. Easy to "fix" into a full reset.
#[test]
fn detach_keeps_breakpoints_but_ends_the_session() {
    with_engine_debugger(|d| d.clear_breakpoints());
    engine_debugger_attach();
    run_val("EngineDebugger.insert_breakpoint(5, \"res://keep.gd\")\nreturn 0\n");

    engine_debugger_detach();
    assert_eq!(
        run_val("return EngineDebugger.is_active()\n"),
        Variant::Bool(false)
    );
    assert_eq!(
        with_engine_debugger(|d| d.breakpoints_for_script("res://keep.gd")),
        vec![5],
        "detaching must not throw away the user's breakpoints"
    );
    with_engine_debugger(|d| d.clear_breakpoints());
}

/// Registered and enabled are different states: Godot registers a profiler up
/// front and only streams frames once `profiler_enable` turns it on.
#[test]
fn profiler_registry_tracks_registration_and_enablement_separately() {
    assert_eq!(
        run_val(
            "EngineDebugger.register_profiler(\"servers\", null)\n\
             return EngineDebugger.has_profiler(\"servers\")\n"
        ),
        Variant::Bool(true)
    );
    assert_eq!(
        run_val("return EngineDebugger.is_profiling(\"servers\")\n"),
        Variant::Bool(false),
        "registering does not start profiling"
    );
    assert_eq!(
        run_val(
            "EngineDebugger.profiler_enable(\"servers\", true)\n\
             return EngineDebugger.is_profiling(\"servers\")\n"
        ),
        Variant::Bool(true)
    );
    assert_eq!(
        run_val(
            "EngineDebugger.profiler_enable(\"servers\", false)\n\
             return EngineDebugger.is_profiling(\"servers\")\n"
        ),
        Variant::Bool(false),
        "disabling stops profiling without unregistering"
    );
    assert_eq!(
        run_val("return EngineDebugger.has_profiler(\"servers\")\n"),
        Variant::Bool(true),
        "...and the profiler stays registered"
    );

    assert_eq!(
        run_val(
            "EngineDebugger.unregister_profiler(\"servers\")\n\
             return EngineDebugger.has_profiler(\"servers\")\n"
        ),
        Variant::Bool(false)
    );
    assert_eq!(
        run_val("return EngineDebugger.is_profiling(\"nope\")\n"),
        Variant::Bool(false),
        "an unknown profiler is not profiling — it must not panic or error"
    );
}

/// Frame data a script hands to a profiler must survive for the transport to
/// stream to the editor, in arrival order.
#[test]
fn profiler_frame_data_is_queued_in_order() {
    run_val(
        "EngineDebugger.register_profiler(\"frames\", null)\n\
         EngineDebugger.profiler_add_frame_data(\"frames\", [1, 2])\n\
         EngineDebugger.profiler_add_frame_data(\"frames\", [3])\n\
         return 0\n",
    );
    let frames = engine_debugger_profiler_frames("frames");
    assert_eq!(frames.len(), 2, "both frames queued");
    assert_eq!(frames[0], vec![Variant::Int(1), Variant::Int(2)]);
    assert_eq!(frames[1], vec![Variant::Int(3)], "arrival order preserved");
    assert!(
        engine_debugger_profiler_frames("never_registered").is_empty(),
        "an unknown profiler has no frames rather than panicking"
    );
}

/// `send_message` is what a script uses to talk to the editor; the transport
/// drains the queue. Draining must empty it, or every message would be re-sent
/// on the next frame.
#[test]
fn send_message_queues_for_the_transport_and_drains_once() {
    let _ = drain_engine_debugger_messages();
    run_val(
        "EngineDebugger.send_message(\"scene:tree\", [1])\n\
         EngineDebugger.send_message(\"perf\", [])\n\
         return 0\n",
    );
    let drained = drain_engine_debugger_messages();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].0, "scene:tree");
    assert_eq!(drained[0].1, vec![Variant::Int(1)]);
    assert_eq!(drained[1].0, "perf");
    assert!(drained[1].1.is_empty(), "an empty data array stays empty");

    assert!(
        drain_engine_debugger_messages().is_empty(),
        "draining consumes the queue — otherwise messages would be re-sent"
    );
}

#[test]
fn message_captures_register_and_unregister() {
    assert_eq!(
        run_val("return EngineDebugger.has_capture(\"cap\")\n"),
        Variant::Bool(false)
    );
    assert_eq!(
        run_val(
            "EngineDebugger.register_message_capture(\"cap\", null)\n\
             return EngineDebugger.has_capture(\"cap\")\n"
        ),
        Variant::Bool(true)
    );
    assert_eq!(
        run_val(
            "EngineDebugger.unregister_message_capture(\"cap\")\n\
             return EngineDebugger.has_capture(\"cap\")\n"
        ),
        Variant::Bool(false)
    );
}

/// ⚠️ Godot's `depth` is the *stepping* counter, not the call-stack depth.
/// Backing `get_depth()` with `Debugger::stack_depth()` would conflate them.
#[test]
fn depth_and_lines_left_round_trip_and_are_not_the_call_stack() {
    assert_eq!(
        run_val("EngineDebugger.set_depth(4)\nreturn EngineDebugger.get_depth()\n"),
        Variant::Int(4)
    );
    assert_eq!(
        run_val("EngineDebugger.set_lines_left(9)\nreturn EngineDebugger.get_lines_left()\n"),
        Variant::Int(9)
    );
    // The call stack is empty here, so a conflated implementation would read 0.
    assert_eq!(with_engine_debugger(|d| d.stack_depth()), 0);
    assert_eq!(
        run_val("return EngineDebugger.get_depth()\n"),
        Variant::Int(4),
        "depth is the stepping counter, independent of the call stack"
    );
}

/// Skipping is off unless the editor asks for it — and a script must observe the
/// editor's choice. Asserting only the `false` default would pin a constant.
#[test]
fn is_skipping_breakpoints_reflects_the_editors_choice() {
    engine_debugger_set_skipping_breakpoints(false);
    assert_eq!(
        run_val("return EngineDebugger.is_skipping_breakpoints()\n"),
        Variant::Bool(false),
        "breakpoints are honoured by default"
    );

    engine_debugger_set_skipping_breakpoints(true);
    assert_eq!(
        run_val("return EngineDebugger.is_skipping_breakpoints()\n"),
        Variant::Bool(true),
        "the editor can ignore breakpoints without removing them"
    );

    // ...and skipping must NOT delete the user's breakpoints.
    with_engine_debugger(|d| d.clear_breakpoints());
    run_val("EngineDebugger.insert_breakpoint(8, \"res://skip.gd\")\nreturn 0\n");
    assert_eq!(
        with_engine_debugger(|d| d.breakpoints_for_script("res://skip.gd")),
        vec![8],
        "skipping is a runtime choice, not a removal"
    );
    engine_debugger_set_skipping_breakpoints(false);
    with_engine_debugger(|d| d.clear_breakpoints());
}

/// Bad arguments must produce a clear error, not a wrong breakpoint. A swapped
/// call is the realistic mistake, and it must not silently succeed.
#[test]
fn malformed_breakpoint_arguments_are_rejected() {
    let err = run_err("EngineDebugger.insert_breakpoint(\"res://a.gd\", 12)\nreturn 0\n");
    assert!(
        matches!(err.kind, RuntimeErrorKind::TypeError(_)),
        "swapped (source, line) must be rejected, got: {err:?}"
    );
    let err = run_err("EngineDebugger.insert_breakpoint(-3, \"res://a.gd\")\nreturn 0\n");
    assert!(
        matches!(err.kind, RuntimeErrorKind::TypeError(_)),
        "a negative line is not a line, got: {err:?}"
    );
    let err = run_err("EngineDebugger.profiler_add_frame_data(\"p\", 5)\nreturn 0\n");
    assert!(
        matches!(err.kind, RuntimeErrorKind::TypeError(_)),
        "frame data must be an Array, got: {err:?}"
    );

    // ⚠️ A truthy non-bool must not be read as `false` and silently DISABLE the
    // profiler — that would look like the profiler simply never firing.
    let err = run_err("EngineDebugger.profiler_enable(\"p\", 1)\nreturn 0\n");
    assert!(
        matches!(err.kind, RuntimeErrorKind::TypeError(_)),
        "profiler_enable takes a real bool, got: {err:?}"
    );
}
