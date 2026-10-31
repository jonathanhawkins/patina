//! Script debugger state machine.
//!
//! Provides breakpoint management, execution stepping (step-in, step-over,
//! step-out), call stack tracking, and variable inspection for the Patina
//! Engine's GDScript-compatible runtime.
//!
//! This module implements the core debugger logic — the editor panel and
//! remote protocol are layered on top.

use std::collections::{BTreeSet, HashMap};

// ---------------------------------------------------------------------------
// BreakpointLocation
// ---------------------------------------------------------------------------

/// A breakpoint location: script path + line number.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BreakpointLocation {
    /// Script resource path (e.g. `"res://player.gd"`).
    pub script: String,
    /// 1-based line number.
    pub line: usize,
}

impl BreakpointLocation {
    /// Creates a new breakpoint location.
    pub fn new(script: impl Into<String>, line: usize) -> Self {
        Self {
            script: script.into(),
            line,
        }
    }
}

// ---------------------------------------------------------------------------
// StepMode
// ---------------------------------------------------------------------------

/// How the debugger should proceed after hitting a breakpoint or pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    /// Continue execution until the next breakpoint.
    Continue,
    /// Execute one statement then pause (step into function calls).
    StepIn,
    /// Execute one statement then pause (step over function calls).
    StepOver,
    /// Continue until the current function returns, then pause.
    StepOut,
}

// ---------------------------------------------------------------------------
// DebuggerState
// ---------------------------------------------------------------------------

/// The current state of the debugger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebuggerState {
    /// Not attached / not debugging.
    Detached,
    /// Running normally (will pause on breakpoints).
    Running,
    /// Paused at a breakpoint or step.
    Paused,
}

// ---------------------------------------------------------------------------
// StackFrame
// ---------------------------------------------------------------------------

/// A single frame in the call stack.
#[derive(Debug, Clone, PartialEq)]
pub struct StackFrame {
    /// The script resource path.
    pub script: String,
    /// The function/method name.
    pub function: String,
    /// 1-based line number where execution is paused.
    pub line: usize,
    /// Local variables visible in this frame.
    pub locals: HashMap<String, VariableValue>,
}

impl StackFrame {
    /// Creates a new stack frame.
    pub fn new(script: impl Into<String>, function: impl Into<String>, line: usize) -> Self {
        Self {
            script: script.into(),
            function: function.into(),
            line,
            locals: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// VariableValue
// ---------------------------------------------------------------------------

/// A debugger-visible variable value (simplified representation).
#[derive(Debug, Clone, PartialEq)]
pub enum VariableValue {
    /// Null / nil.
    Nil,
    /// Boolean value.
    Bool(bool),
    /// Integer value.
    Int(i64),
    /// Float value.
    Float(f64),
    /// String value.
    String(String),
    /// Object reference (class name + instance ID).
    Object(String, u64),
    /// Array of values.
    Array(Vec<VariableValue>),
    /// Dictionary of key-value pairs.
    Dictionary(Vec<(String, VariableValue)>),
}

impl std::fmt::Display for VariableValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VariableValue::Nil => write!(f, "null"),
            VariableValue::Bool(b) => write!(f, "{b}"),
            VariableValue::Int(n) => write!(f, "{n}"),
            VariableValue::Float(n) => write!(f, "{n}"),
            VariableValue::String(s) => write!(f, "\"{s}\""),
            VariableValue::Object(class, id) => write!(f, "<{class}#{id}>"),
            VariableValue::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            VariableValue::Dictionary(entries) => {
                write!(f, "{{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Debugger
// ---------------------------------------------------------------------------

/// The core debugger state machine.
///
/// Manages breakpoints, execution state, call stack, and variable inspection.
/// The debugger is designed to be driven by the engine's script executor:
///
/// 1. Before executing each statement, call [`should_break`] to check if the
///    debugger wants to pause.
/// 2. When paused, the editor can inspect the call stack and variables.
/// 3. The editor issues a step command via [`step`] or [`continue_running`].
#[derive(Debug)]
pub struct Debugger {
    state: DebuggerState,
    breakpoints: BTreeSet<BreakpointLocation>,
    /// Conditional breakpoints: location -> expression string.
    conditions: HashMap<BreakpointLocation, String>,
    /// Hit counts for breakpoints.
    hit_counts: HashMap<BreakpointLocation, u64>,
    call_stack: Vec<StackFrame>,
    step_mode: Option<StepMode>,
    /// Call stack depth when step-over/step-out was initiated.
    step_depth: usize,
    /// The script runtime's own live call depth, when the host reports one
    /// (pat-mgixk.14).
    ///
    /// 🔑 [`call_stack`](Self::call_stack) is the debugger's OWN frame list, and
    /// a host that never calls [`push_frame`](Self::push_frame) leaves it empty
    /// — which makes `StepOver`'s `len() <= step_depth` and `StepOut`'s
    /// `len() < step_depth` compare `0` against `0`. `StepOver` then behaves
    /// like `StepIn` and `StepOut` like `Continue`: both buttons appear wired
    /// and neither does what it says. The GDScript interpreter already tracks
    /// its depth in `call_depth`, so it reports that here rather than
    /// duplicating the frame list on the hot path.
    runtime_depth: Option<usize>,
    /// Swallows the ONE break check that fires on the statement a resume
    /// re-enters.
    ///
    /// 🔑 A breakpoint pauses BEFORE its statement, so the line it stopped on
    /// has not run yet and the resume re-enters that same statement. Without
    /// this, [`should_break`](Self::should_break) sees the armed step on the
    /// very line the user asked to step OFF and pauses again immediately —
    /// Step Over kept reporting the same line, which is what pat-mgixk.14's
    /// acceptance caught (`left: 5, right: 6`).
    ///
    /// ⚠️ It must swallow the BREAKPOINT check too, not only the step check.
    /// Stepping off a line that carries a breakpoint would otherwise stop on it
    /// again; that breakpoint has already been honoured once.
    resume_skips_one_check: bool,
    /// Global/autoload variables visible across all frames.
    globals: HashMap<String, VariableValue>,
}

impl Debugger {
    /// Creates a new debugger in the detached state.
    pub fn new() -> Self {
        Self {
            state: DebuggerState::Detached,
            breakpoints: BTreeSet::new(),
            conditions: HashMap::new(),
            hit_counts: HashMap::new(),
            call_stack: Vec::new(),
            step_mode: None,
            resume_skips_one_check: false,
            step_depth: 0,
            runtime_depth: None,
            globals: HashMap::new(),
        }
    }

    // -- State management ---------------------------------------------------

    /// Returns the current debugger state.
    pub fn state(&self) -> DebuggerState {
        self.state
    }

    /// Attaches the debugger (transitions to Running).
    pub fn attach(&mut self) {
        self.state = DebuggerState::Running;
        self.step_mode = None;
    }

    /// Detaches the debugger, clearing all runtime state (breakpoints are kept).
    pub fn detach(&mut self) {
        self.state = DebuggerState::Detached;
        self.call_stack.clear();
        self.step_mode = None;
        self.step_depth = 0;
        self.runtime_depth = None;
        self.hit_counts.clear();
        self.globals.clear();
    }

    /// Returns `true` if the debugger is attached (Running or Paused).
    pub fn is_attached(&self) -> bool {
        self.state != DebuggerState::Detached
    }

    // -- Breakpoints --------------------------------------------------------

    /// Adds a breakpoint. Returns `false` if it was already set.
    pub fn add_breakpoint(&mut self, loc: BreakpointLocation) -> bool {
        self.breakpoints.insert(loc)
    }

    /// Removes a breakpoint. Returns `true` if it was present.
    pub fn remove_breakpoint(&mut self, loc: &BreakpointLocation) -> bool {
        self.conditions.remove(loc);
        self.hit_counts.remove(loc);
        self.breakpoints.remove(loc)
    }

    /// Toggles a breakpoint. Returns `true` if now set, `false` if removed.
    pub fn toggle_breakpoint(&mut self, loc: BreakpointLocation) -> bool {
        if self.breakpoints.contains(&loc) {
            self.remove_breakpoint(&loc);
            false
        } else {
            self.add_breakpoint(loc)
        }
    }

    /// Returns whether a breakpoint is set at the given location.
    pub fn has_breakpoint(&self, loc: &BreakpointLocation) -> bool {
        self.breakpoints.contains(loc)
    }

    /// Returns all breakpoints sorted by script path then line.
    pub fn breakpoints(&self) -> Vec<&BreakpointLocation> {
        self.breakpoints.iter().collect()
    }

    /// Returns all breakpoints for a specific script.
    pub fn breakpoints_for_script(&self, script: &str) -> Vec<usize> {
        self.breakpoints
            .iter()
            .filter(|bp| bp.script == script)
            .map(|bp| bp.line)
            .collect()
    }

    /// Clears all breakpoints.
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
        self.conditions.clear();
        self.hit_counts.clear();
    }

    /// Sets a condition expression for a breakpoint.
    pub fn set_condition(&mut self, loc: BreakpointLocation, expr: impl Into<String>) {
        self.conditions.insert(loc, expr.into());
    }

    /// Returns the condition for a breakpoint, if any.
    pub fn get_condition(&self, loc: &BreakpointLocation) -> Option<&str> {
        self.conditions.get(loc).map(|s| s.as_str())
    }

    /// Returns the hit count for a breakpoint.
    pub fn hit_count(&self, loc: &BreakpointLocation) -> u64 {
        self.hit_counts.get(loc).copied().unwrap_or(0)
    }

    // -- Execution control --------------------------------------------------

    /// The depth the step comparisons use: the host's reported runtime depth
    /// when it has one, otherwise this debugger's own frame list.
    ///
    /// See [`runtime_depth`](Self::runtime_depth) for why the fallback alone is
    /// not enough.
    fn effective_depth(&self) -> usize {
        self.runtime_depth.unwrap_or(self.call_stack.len())
    }

    /// Reports the script runtime's live call depth, so `StepOver`/`StepOut`
    /// compare against something real (pat-mgixk.14).
    pub fn set_runtime_depth(&mut self, depth: usize) {
        self.runtime_depth = Some(depth);
    }

    /// Arms a step mode from the HOST's resume path, in whatever state the
    /// debugger happens to be.
    ///
    /// ⚠️ Deliberately distinct from [`step`](Self::step), which requires
    /// `Paused` and is the editor-facing transition. In a real session the
    /// editor's press and the game's pickup happen on DIFFERENT threads and a
    /// frame apart, and the host re-`attach()`es every frame in between — which
    /// resets the state to `Running` and clears `step_mode`. A resume that
    /// insisted on `Paused` would therefore arm nothing, and every Step button
    /// would behave like Continue (pat-mgixk.14).
    pub fn arm_step(&mut self, mode: StepMode, depth: usize) {
        self.step_mode = Some(mode);
        self.step_depth = depth;
        self.runtime_depth = Some(depth);
        self.state = DebuggerState::Running;
        // The statement being resumed into is the one this step RUNS, so its
        // check is already spent. Set for EVERY mode, Continue included: a
        // Continue off a breakpointed line must not stop on it again either.
        self.resume_skips_one_check = true;
    }

    /// Checks whether the debugger should pause at the given location.
    ///
    /// Call this before executing each statement. Returns `true` if the
    /// debugger wants to pause (breakpoint hit or step completed).
    pub fn should_break(&mut self, script: &str, line: usize) -> bool {
        if self.state != DebuggerState::Running {
            return false;
        }

        // The statement a resume re-enters has already had its break honoured —
        // see `resume_skips_one_check`. This returns BEFORE both the step and
        // the breakpoint checks, which is what lets a step actually advance.
        if self.resume_skips_one_check {
            self.resume_skips_one_check = false;
            return false;
        }

        let loc = BreakpointLocation::new(script, line);

        // Check step mode first.
        if let Some(mode) = self.step_mode {
            let depth = self.effective_depth();
            let should_pause = match mode {
                StepMode::Continue => false,
                StepMode::StepIn => true,
                StepMode::StepOver => depth <= self.step_depth,
                StepMode::StepOut => depth < self.step_depth,
            };
            if should_pause {
                self.pause();
                return true;
            }
        }

        // Check breakpoints.
        if self.breakpoints.contains(&loc) {
            *self.hit_counts.entry(loc).or_insert(0) += 1;
            self.pause();
            return true;
        }

        false
    }

    /// Pauses execution.
    pub fn pause(&mut self) {
        if self.state == DebuggerState::Running {
            self.state = DebuggerState::Paused;
            self.step_mode = None;
        }
    }

    /// Resumes execution with the given step mode.
    pub fn step(&mut self, mode: StepMode) {
        if self.state != DebuggerState::Paused {
            return;
        }
        self.step_mode = Some(mode);
        self.step_depth = self.effective_depth();
        self.state = DebuggerState::Running;
    }

    /// Continues execution until the next breakpoint.
    pub fn continue_running(&mut self) {
        self.step(StepMode::Continue);
    }

    /// Steps into the next statement.
    pub fn step_in(&mut self) {
        self.step(StepMode::StepIn);
    }

    /// Steps over the next statement (doesn't enter function calls).
    pub fn step_over(&mut self) {
        self.step(StepMode::StepOver);
    }

    /// Steps out of the current function.
    pub fn step_out(&mut self) {
        self.step(StepMode::StepOut);
    }

    // -- Call stack ----------------------------------------------------------

    /// Pushes a new frame onto the call stack (called when entering a function).
    pub fn push_frame(&mut self, frame: StackFrame) {
        self.call_stack.push(frame);
    }

    /// Pops the top frame from the call stack (called when leaving a function).
    pub fn pop_frame(&mut self) -> Option<StackFrame> {
        self.call_stack.pop()
    }

    /// Returns the current call stack (top frame last).
    pub fn call_stack(&self) -> &[StackFrame] {
        &self.call_stack
    }

    /// Returns the current (topmost) stack frame.
    pub fn current_frame(&self) -> Option<&StackFrame> {
        self.call_stack.last()
    }

    /// Returns a mutable reference to the current stack frame.
    pub fn current_frame_mut(&mut self) -> Option<&mut StackFrame> {
        self.call_stack.last_mut()
    }

    /// Returns the call stack depth.
    pub fn stack_depth(&self) -> usize {
        self.call_stack.len()
    }

    /// Updates the line number of the current frame.
    pub fn set_current_line(&mut self, line: usize) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.line = line;
        }
    }

    // -- Variable inspection ------------------------------------------------

    /// Sets a local variable in the current frame.
    pub fn set_local(&mut self, name: impl Into<String>, value: VariableValue) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.locals.insert(name.into(), value);
        }
    }

    /// Gets a local variable from a specific frame (0 = top).
    pub fn get_local(&self, frame_index: usize, name: &str) -> Option<&VariableValue> {
        let idx = self.call_stack.len().checked_sub(1 + frame_index)?;
        self.call_stack.get(idx)?.locals.get(name)
    }

    /// Returns all locals in the current frame.
    pub fn current_locals(&self) -> Option<&HashMap<String, VariableValue>> {
        self.call_stack.last().map(|f| &f.locals)
    }

    /// Sets a global variable.
    pub fn set_global(&mut self, name: impl Into<String>, value: VariableValue) {
        self.globals.insert(name.into(), value);
    }

    /// Gets a global variable.
    pub fn get_global(&self, name: &str) -> Option<&VariableValue> {
        self.globals.get(name)
    }

    /// Returns all globals.
    pub fn globals(&self) -> &HashMap<String, VariableValue> {
        &self.globals
    }
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_debugger_is_detached() {
        let dbg = Debugger::new();
        assert_eq!(dbg.state(), DebuggerState::Detached);
        assert!(!dbg.is_attached());
    }

    #[test]
    fn attach_and_detach() {
        let mut dbg = Debugger::new();
        dbg.attach();
        assert_eq!(dbg.state(), DebuggerState::Running);
        assert!(dbg.is_attached());
        dbg.detach();
        assert_eq!(dbg.state(), DebuggerState::Detached);
    }

    #[test]
    fn add_remove_breakpoint() {
        let mut dbg = Debugger::new();
        let loc = BreakpointLocation::new("res://test.gd", 10);
        assert!(dbg.add_breakpoint(loc.clone()));
        assert!(!dbg.add_breakpoint(loc.clone())); // duplicate
        assert!(dbg.has_breakpoint(&loc));
        assert!(dbg.remove_breakpoint(&loc));
        assert!(!dbg.has_breakpoint(&loc));
    }

    #[test]
    fn toggle_breakpoint() {
        let mut dbg = Debugger::new();
        let loc = BreakpointLocation::new("res://test.gd", 5);
        assert!(dbg.toggle_breakpoint(loc.clone())); // added
        assert!(dbg.has_breakpoint(&loc));
        assert!(!dbg.toggle_breakpoint(loc.clone())); // removed
        assert!(!dbg.has_breakpoint(&loc));
    }

    #[test]
    fn breakpoints_for_script() {
        let mut dbg = Debugger::new();
        dbg.add_breakpoint(BreakpointLocation::new("res://a.gd", 1));
        dbg.add_breakpoint(BreakpointLocation::new("res://a.gd", 10));
        dbg.add_breakpoint(BreakpointLocation::new("res://b.gd", 5));
        let lines = dbg.breakpoints_for_script("res://a.gd");
        assert_eq!(lines, vec![1, 10]);
    }

    #[test]
    fn should_break_on_breakpoint() {
        let mut dbg = Debugger::new();
        dbg.attach();
        dbg.add_breakpoint(BreakpointLocation::new("res://test.gd", 10));
        assert!(!dbg.should_break("res://test.gd", 5));
        assert!(dbg.should_break("res://test.gd", 10));
        assert_eq!(dbg.state(), DebuggerState::Paused);
    }

    #[test]
    fn hit_count_increments() {
        let mut dbg = Debugger::new();
        dbg.attach();
        let loc = BreakpointLocation::new("res://test.gd", 10);
        dbg.add_breakpoint(loc.clone());
        dbg.should_break("res://test.gd", 10);
        assert_eq!(dbg.hit_count(&loc), 1);
        dbg.continue_running();
        dbg.should_break("res://test.gd", 10);
        assert_eq!(dbg.hit_count(&loc), 2);
    }

    #[test]
    fn variable_value_display() {
        assert_eq!(format!("{}", VariableValue::Nil), "null");
        assert_eq!(format!("{}", VariableValue::Int(42)), "42");
        assert_eq!(format!("{}", VariableValue::String("hi".into())), "\"hi\"");
        assert_eq!(
            format!("{}", VariableValue::Object("Node2D".into(), 123)),
            "<Node2D#123>"
        );
    }

    // -- pat-mgixk.14: stepping for a host that keeps its own call depth -----

    /// A host that never pushes frames gets `StepOver` == `StepIn` and
    /// `StepOut` == `Continue` from the frame-list fallback. Reporting the
    /// runtime depth is what separates them.
    #[test]
    fn step_over_does_not_pause_inside_a_deeper_call() {
        let mut dbg = Debugger::new();
        dbg.attach();
        // Stepping over from depth 1.
        dbg.arm_step(StepMode::StepOver, 1);
        // ⚠️ Consume the one-shot resume skip first, or the depth assertion
        // below is VACUOUS: `arm_step` swallows the check for the statement the
        // resume re-enters (see `resume_skips_one_check`), so a bare negative
        // would pass whatever the depth. This stands in for that statement.
        dbg.should_break("res://main.gd", 1);

        // A statement one level DEEPER is the body of the call being stepped
        // over, so it must run without pausing.
        dbg.set_runtime_depth(2);
        assert!(
            !dbg.should_break("res://main.gd", 10),
            "StepOver must run the callee's body, not stop inside it"
        );

        // Back at the original depth: this is the next statement of the frame
        // the user stepped in, so it pauses.
        dbg.set_runtime_depth(1);
        assert!(
            dbg.should_break("res://main.gd", 4),
            "StepOver must pause on the next statement of the stepped frame"
        );
        assert_eq!(dbg.state(), DebuggerState::Paused);
    }

    /// `StepOut` is the strict version: it must not stop until the frame is
    /// actually gone.
    #[test]
    fn step_out_pauses_only_after_the_frame_returns() {
        let mut dbg = Debugger::new();
        dbg.attach();
        dbg.arm_step(StepMode::StepOut, 2);
        // ⚠️ Consume the one-shot resume skip first, or the depth assertion
        // below is VACUOUS: `arm_step` swallows the check for the statement the
        // resume re-enters (see `resume_skips_one_check`), so a bare negative
        // would pass whatever the depth. This stands in for that statement.
        dbg.should_break("res://main.gd", 1);

        dbg.set_runtime_depth(2);
        assert!(
            !dbg.should_break("res://main.gd", 11),
            "StepOut must not stop in the frame it is leaving"
        );
        dbg.set_runtime_depth(1);
        assert!(
            dbg.should_break("res://main.gd", 5),
            "StepOut must pause once the frame has returned"
        );
    }

    /// `arm_step` exists because the host's resume runs a frame after the
    /// editor's press, with a re-`attach()` in between that leaves the state
    /// `Running`. `step` refuses that; `arm_step` must not.
    #[test]
    fn arm_step_works_from_the_running_state_where_step_refuses() {
        let mut dbg = Debugger::new();
        dbg.attach();
        assert_eq!(dbg.state(), DebuggerState::Running);

        // The editor-facing transition declines — it is only valid from Paused.
        dbg.step(StepMode::StepIn);
        assert!(
            !dbg.should_break("res://main.gd", 1),
            "`step` from Running must arm nothing"
        );

        // The host-facing one arms regardless, which is what keeps a Step from
        // silently behaving like Continue.
        dbg.attach();
        dbg.arm_step(StepMode::StepIn, 0);
        // ⚠️ The FIRST check after arming is swallowed by design: it is the
        // statement the resume re-enters, which a breakpoint already stopped
        // before. This assertion used to read `should_break(..) == true` here,
        // and that expectation WAS the bug — if the first check pauses, a Step
        // never advances, which is exactly what pat-mgixk.14's acceptance saw
        // (`left: 5, right: 6`). The intent of this test is unchanged: arming
        // from `Running` works, observed one statement later.
        assert!(
            !dbg.should_break("res://main.gd", 1),
            "the resumed statement's check is spent, so this one must not pause"
        );
        assert!(
            dbg.should_break("res://main.gd", 2),
            "`arm_step` must arm from Running too — StepIn stops on the next \
             statement"
        );
    }

    /// A resume must not stop again on the statement it resumes INTO.
    ///
    /// 🔑 This is the whole of pat-mgixk.14's Step Over defect. A breakpoint
    /// pauses BEFORE its statement, so that statement has not run and the
    /// resume re-enters it. Re-evaluating the armed step there pauses on the
    /// very line the user asked to step off, and `/api/debug/state` kept
    /// reporting the same line however many times Step Over was pressed.
    #[test]
    fn a_resume_does_not_rebreak_on_the_statement_it_re_enters() {
        let mut dbg = Debugger::new();
        dbg.attach();
        dbg.add_breakpoint(BreakpointLocation::new("res://main.gd", 5));

        // The breakpoint stops BEFORE line 5 runs.
        assert!(
            dbg.should_break("res://main.gd", 5),
            "precondition: the breakpoint must fire"
        );
        assert_eq!(dbg.state(), DebuggerState::Paused);

        // Step Over resumes into line 5 — which must now RUN, not re-break.
        dbg.arm_step(StepMode::StepOver, 0);
        assert!(
            !dbg.should_break("res://main.gd", 5),
            "the statement being resumed into must run; re-breaking here is \
             what made Step Over report the same line forever"
        );
        // ...and the step stops on the next statement.
        assert!(
            dbg.should_break("res://main.gd", 6),
            "StepOver must pause on the statement AFTER the one it ran"
        );

        // 🔑 The skip is ONE-SHOT, not a standing suppression: a loop that
        // comes back around to line 5 must stop there again.
        dbg.arm_step(StepMode::Continue, 0);
        assert!(
            !dbg.should_break("res://main.gd", 5),
            "the resumed statement is still spent, Continue included — \
             otherwise Continue off a breakpointed line never leaves it"
        );
        assert!(
            !dbg.should_break("res://main.gd", 7),
            "Continue does not stop on an unbreakpointed line"
        );
        assert!(
            dbg.should_break("res://main.gd", 5),
            "but the breakpoint must fire again when execution returns to it"
        );
    }

    /// A reported depth must not leak across a detach, or the next session
    /// steps against a stale frame count.
    ///
    /// Observed through `step`, which captures `effective_depth()` as its
    /// `step_depth`: with the stale 5 still in place it would capture 5 and
    /// then pause at depth 1; with it cleared it captures the empty frame
    /// list's 0 and correctly does not.
    #[test]
    fn detach_forgets_the_reported_runtime_depth() {
        let mut dbg = Debugger::new();
        dbg.attach();
        dbg.set_runtime_depth(5);
        dbg.detach();

        dbg.attach();
        dbg.pause();
        dbg.step(StepMode::StepOver);
        dbg.set_runtime_depth(1);
        assert!(
            !dbg.should_break("res://main.gd", 1),
            "a stale depth of 5 would have made `step_depth` 5, and depth 1 \
             would then satisfy `1 <= 5` and pause one level too deep"
        );
    }
}
