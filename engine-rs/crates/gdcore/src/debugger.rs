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
            step_depth: 0,
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

    /// Checks whether the debugger should pause at the given location.
    ///
    /// Call this before executing each statement. Returns `true` if the
    /// debugger wants to pause (breakpoint hit or step completed).
    pub fn should_break(&mut self, script: &str, line: usize) -> bool {
        if self.state != DebuggerState::Running {
            return false;
        }

        let loc = BreakpointLocation::new(script, line);

        // Check step mode first.
        if let Some(mode) = self.step_mode {
            let should_pause = match mode {
                StepMode::Continue => false,
                StepMode::StepIn => true,
                StepMode::StepOver => self.call_stack.len() <= self.step_depth,
                StepMode::StepOut => self.call_stack.len() < self.step_depth,
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
        self.step_depth = self.call_stack.len();
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
}
