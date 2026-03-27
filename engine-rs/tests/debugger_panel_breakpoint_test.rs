//! Tests for the debugger state machine: breakpoints, stepping, call stack,
//! and variable inspection.

use gdcore::debugger::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn bp(script: &str, line: usize) -> BreakpointLocation {
    BreakpointLocation::new(script, line)
}

fn attached_debugger() -> Debugger {
    let mut dbg = Debugger::new();
    dbg.attach();
    dbg
}

// ---------------------------------------------------------------------------
// Breakpoint management
// ---------------------------------------------------------------------------

#[test]
fn add_multiple_breakpoints_across_scripts() {
    let mut dbg = Debugger::new();
    dbg.add_breakpoint(bp("res://player.gd", 10));
    dbg.add_breakpoint(bp("res://player.gd", 20));
    dbg.add_breakpoint(bp("res://enemy.gd", 5));

    assert_eq!(dbg.breakpoints().len(), 3);
    assert_eq!(dbg.breakpoints_for_script("res://player.gd"), vec![10, 20]);
    assert_eq!(dbg.breakpoints_for_script("res://enemy.gd"), vec![5]);
    assert!(dbg.breakpoints_for_script("res://none.gd").is_empty());
}

#[test]
fn clear_breakpoints() {
    let mut dbg = Debugger::new();
    dbg.add_breakpoint(bp("res://a.gd", 1));
    dbg.add_breakpoint(bp("res://b.gd", 2));
    dbg.clear_breakpoints();
    assert!(dbg.breakpoints().is_empty());
}

#[test]
fn conditional_breakpoint() {
    let mut dbg = Debugger::new();
    let loc = bp("res://test.gd", 15);
    dbg.add_breakpoint(loc.clone());
    dbg.set_condition(loc.clone(), "x > 10");
    assert_eq!(dbg.get_condition(&loc), Some("x > 10"));

    // Removing breakpoint also removes condition
    dbg.remove_breakpoint(&loc);
    assert_eq!(dbg.get_condition(&loc), None);
}

#[test]
fn hit_count_tracking() {
    let mut dbg = attached_debugger();
    let loc = bp("res://test.gd", 10);
    dbg.add_breakpoint(loc.clone());

    assert_eq!(dbg.hit_count(&loc), 0);
    dbg.should_break("res://test.gd", 10);
    assert_eq!(dbg.hit_count(&loc), 1);

    dbg.continue_running();
    dbg.should_break("res://test.gd", 10);
    assert_eq!(dbg.hit_count(&loc), 2);

    // Detach clears hit counts
    dbg.detach();
    assert_eq!(dbg.hit_count(&loc), 0);
}

// ---------------------------------------------------------------------------
// Execution control: should_break
// ---------------------------------------------------------------------------

#[test]
fn should_break_only_when_running() {
    let mut dbg = Debugger::new();
    dbg.add_breakpoint(bp("res://test.gd", 10));
    // Detached — should not break
    assert!(!dbg.should_break("res://test.gd", 10));
}

#[test]
fn should_break_pauses_on_hit() {
    let mut dbg = attached_debugger();
    dbg.add_breakpoint(bp("res://test.gd", 10));

    assert!(!dbg.should_break("res://test.gd", 5)); // miss
    assert_eq!(dbg.state(), DebuggerState::Running);

    assert!(dbg.should_break("res://test.gd", 10)); // hit
    assert_eq!(dbg.state(), DebuggerState::Paused);
}

#[test]
fn no_break_without_breakpoints() {
    let mut dbg = attached_debugger();
    assert!(!dbg.should_break("res://test.gd", 1));
    assert!(!dbg.should_break("res://test.gd", 100));
    assert_eq!(dbg.state(), DebuggerState::Running);
}

// ---------------------------------------------------------------------------
// Step modes
// ---------------------------------------------------------------------------

#[test]
fn step_in_pauses_on_next_statement() {
    let mut dbg = attached_debugger();
    dbg.add_breakpoint(bp("res://test.gd", 10));
    dbg.should_break("res://test.gd", 10); // paused
    dbg.step_in();
    assert_eq!(dbg.state(), DebuggerState::Running);
    // Next statement should pause
    assert!(dbg.should_break("res://test.gd", 11));
    assert_eq!(dbg.state(), DebuggerState::Paused);
}

#[test]
fn step_over_skips_deeper_frames() {
    let mut dbg = attached_debugger();
    dbg.push_frame(StackFrame::new("res://test.gd", "_ready", 10));
    dbg.add_breakpoint(bp("res://test.gd", 10));
    dbg.should_break("res://test.gd", 10); // paused at depth 1

    dbg.step_over();
    // Enter a function call (push frame) — depth becomes 2
    dbg.push_frame(StackFrame::new("res://util.gd", "helper", 1));
    // Should NOT break inside deeper frame
    assert!(!dbg.should_break("res://util.gd", 1));
    assert!(!dbg.should_break("res://util.gd", 2));

    // Pop back to original depth
    dbg.pop_frame();
    // Should break at original depth
    assert!(dbg.should_break("res://test.gd", 11));
    assert_eq!(dbg.state(), DebuggerState::Paused);
}

#[test]
fn step_out_pauses_when_frame_pops() {
    let mut dbg = attached_debugger();
    dbg.push_frame(StackFrame::new("res://main.gd", "_ready", 5));
    dbg.push_frame(StackFrame::new("res://test.gd", "helper", 10));
    dbg.add_breakpoint(bp("res://test.gd", 10));
    dbg.should_break("res://test.gd", 10); // paused at depth 2

    dbg.step_out();
    // Still inside helper — should not break
    assert!(!dbg.should_break("res://test.gd", 11));
    assert!(!dbg.should_break("res://test.gd", 12));

    // Pop helper frame — back to depth 1 (< step_depth of 2)
    dbg.pop_frame();
    // Should break in caller
    assert!(dbg.should_break("res://main.gd", 6));
    assert_eq!(dbg.state(), DebuggerState::Paused);
}

#[test]
fn continue_running_ignores_steps() {
    let mut dbg = attached_debugger();
    dbg.add_breakpoint(bp("res://test.gd", 10));
    dbg.should_break("res://test.gd", 10);
    dbg.continue_running();
    // Should not break on arbitrary lines
    assert!(!dbg.should_break("res://test.gd", 11));
    assert!(!dbg.should_break("res://test.gd", 12));
    // Should break on another breakpoint
    dbg.add_breakpoint(bp("res://test.gd", 20));
    assert!(dbg.should_break("res://test.gd", 20));
}

#[test]
fn step_commands_ignored_when_not_paused() {
    let mut dbg = attached_debugger();
    dbg.step_in(); // should be ignored (not paused)
    assert_eq!(dbg.state(), DebuggerState::Running);
}

// ---------------------------------------------------------------------------
// Call stack
// ---------------------------------------------------------------------------

#[test]
fn push_pop_frames() {
    let mut dbg = attached_debugger();
    assert_eq!(dbg.stack_depth(), 0);
    assert!(dbg.current_frame().is_none());

    dbg.push_frame(StackFrame::new("res://main.gd", "_ready", 1));
    assert_eq!(dbg.stack_depth(), 1);
    assert_eq!(dbg.current_frame().unwrap().function, "_ready");

    dbg.push_frame(StackFrame::new("res://util.gd", "helper", 10));
    assert_eq!(dbg.stack_depth(), 2);
    assert_eq!(dbg.current_frame().unwrap().function, "helper");

    let popped = dbg.pop_frame().unwrap();
    assert_eq!(popped.function, "helper");
    assert_eq!(dbg.stack_depth(), 1);
    assert_eq!(dbg.current_frame().unwrap().function, "_ready");
}

#[test]
fn set_current_line() {
    let mut dbg = attached_debugger();
    dbg.push_frame(StackFrame::new("res://test.gd", "run", 1));
    dbg.set_current_line(42);
    assert_eq!(dbg.current_frame().unwrap().line, 42);
}

#[test]
fn detach_clears_stack() {
    let mut dbg = attached_debugger();
    dbg.push_frame(StackFrame::new("res://test.gd", "run", 1));
    dbg.detach();
    assert_eq!(dbg.stack_depth(), 0);
}

// ---------------------------------------------------------------------------
// Variable inspection
// ---------------------------------------------------------------------------

#[test]
fn set_and_get_local() {
    let mut dbg = attached_debugger();
    dbg.push_frame(StackFrame::new("res://test.gd", "run", 1));
    dbg.set_local("x", VariableValue::Int(42));
    assert_eq!(dbg.get_local(0, "x"), Some(&VariableValue::Int(42)));
    assert_eq!(dbg.get_local(0, "y"), None);
}

#[test]
fn get_local_from_deeper_frame() {
    let mut dbg = attached_debugger();
    let mut f1 = StackFrame::new("res://main.gd", "_ready", 1);
    f1.locals.insert("a".into(), VariableValue::Int(10));
    dbg.push_frame(f1);

    let mut f2 = StackFrame::new("res://util.gd", "helper", 5);
    f2.locals.insert("b".into(), VariableValue::Int(20));
    dbg.push_frame(f2);

    // Frame 0 = top (helper), frame 1 = _ready
    assert_eq!(dbg.get_local(0, "b"), Some(&VariableValue::Int(20)));
    assert_eq!(dbg.get_local(1, "a"), Some(&VariableValue::Int(10)));
    assert_eq!(dbg.get_local(0, "a"), None); // not in helper
}

#[test]
fn current_locals() {
    let mut dbg = attached_debugger();
    dbg.push_frame(StackFrame::new("res://test.gd", "run", 1));
    dbg.set_local("speed", VariableValue::Float(3.14));
    dbg.set_local("name", VariableValue::String("Player".into()));
    let locals = dbg.current_locals().unwrap();
    assert_eq!(locals.len(), 2);
    assert_eq!(locals.get("speed"), Some(&VariableValue::Float(3.14)));
}

#[test]
fn globals() {
    let mut dbg = attached_debugger();
    dbg.set_global("game_state", VariableValue::String("playing".into()));
    assert_eq!(
        dbg.get_global("game_state"),
        Some(&VariableValue::String("playing".into()))
    );
    assert_eq!(dbg.globals().len(), 1);
}

#[test]
fn no_locals_without_frame() {
    let dbg = attached_debugger();
    assert!(dbg.current_locals().is_none());
    assert_eq!(dbg.get_local(0, "x"), None);
}

// ---------------------------------------------------------------------------
// Variable display
// ---------------------------------------------------------------------------

#[test]
fn variable_value_display_array() {
    let arr = VariableValue::Array(vec![
        VariableValue::Int(1),
        VariableValue::Int(2),
        VariableValue::Int(3),
    ]);
    assert_eq!(format!("{arr}"), "[1, 2, 3]");
}

#[test]
fn variable_value_display_dictionary() {
    let dict = VariableValue::Dictionary(vec![
        ("hp".into(), VariableValue::Int(100)),
        ("name".into(), VariableValue::String("Hero".into())),
    ]);
    assert_eq!(format!("{dict}"), "{hp: 100, name: \"Hero\"}");
}

#[test]
fn variable_value_display_nested() {
    let val = VariableValue::Array(vec![
        VariableValue::Dictionary(vec![("x".into(), VariableValue::Float(1.5))]),
    ]);
    assert_eq!(format!("{val}"), "[{x: 1.5}]");
}

// ---------------------------------------------------------------------------
// Pause / resume cycle
// ---------------------------------------------------------------------------

#[test]
fn manual_pause_and_resume() {
    let mut dbg = attached_debugger();
    dbg.pause();
    assert_eq!(dbg.state(), DebuggerState::Paused);
    dbg.continue_running();
    assert_eq!(dbg.state(), DebuggerState::Running);
}

#[test]
fn pause_when_detached_is_noop() {
    let mut dbg = Debugger::new();
    dbg.pause();
    assert_eq!(dbg.state(), DebuggerState::Detached);
}

// ---------------------------------------------------------------------------
// Full debugging session scenario
// ---------------------------------------------------------------------------

#[test]
fn full_debugging_session() {
    let mut dbg = Debugger::new();

    // Set breakpoints before attaching
    dbg.add_breakpoint(bp("res://player.gd", 15));
    dbg.add_breakpoint(bp("res://player.gd", 30));

    // Attach
    dbg.attach();
    assert_eq!(dbg.state(), DebuggerState::Running);

    // Simulate execution — enter _ready function
    dbg.push_frame(StackFrame::new("res://player.gd", "_ready", 10));

    // Execute lines 10-14, no breakpoint
    for line in 10..15 {
        dbg.set_current_line(line);
        assert!(!dbg.should_break("res://player.gd", line));
    }

    // Hit breakpoint at line 15
    dbg.set_current_line(15);
    assert!(dbg.should_break("res://player.gd", 15));
    assert_eq!(dbg.state(), DebuggerState::Paused);

    // Inspect locals
    dbg.set_local("health", VariableValue::Int(100));
    dbg.set_local("position", VariableValue::String("Vector2(0, 0)".into()));
    assert_eq!(dbg.current_locals().unwrap().len(), 2);

    // Step over once
    dbg.step_over();
    dbg.set_current_line(16);
    assert!(dbg.should_break("res://player.gd", 16));

    // Continue to next breakpoint
    dbg.continue_running();
    for line in 17..30 {
        dbg.set_current_line(line);
        assert!(!dbg.should_break("res://player.gd", line));
    }
    dbg.set_current_line(30);
    assert!(dbg.should_break("res://player.gd", 30));

    // Detach
    dbg.detach();
    assert_eq!(dbg.state(), DebuggerState::Detached);
    assert_eq!(dbg.stack_depth(), 0);
    // Breakpoints survive detach
    assert_eq!(dbg.breakpoints().len(), 2);
}
