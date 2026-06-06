//! Script editor **breakpoints + debugger integration** (pat-o1odk).
//!
//! Toggling a breakpoint marks a line and registers it with the debugger. When
//! the running project reaches a line that has a breakpoint, execution halts and
//! the debugger exposes the current call stack and local variables. Resuming —
//! or clearing the breakpoint at the paused line — lets execution continue.
//!
//! This is a headless model: the "running project" calls [`ScriptDebugger::reach_line`]
//! as it executes, and the debugger decides whether to pause.

use std::collections::{BTreeMap, BTreeSet};

/// One call-stack frame exposed while paused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The function executing in this frame.
    pub function: String,
    /// The current line within the function.
    pub line: usize,
}

/// The state exposed while the debugger is paused at a breakpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauseState {
    /// The line execution halted on.
    pub line: usize,
    /// The call stack, innermost frame first.
    pub stack: Vec<Frame>,
    /// The local variables in scope, by name.
    pub locals: BTreeMap<String, String>,
}

/// Tracks breakpoints and the paused state of a debugging session.
#[derive(Debug, Clone, Default)]
pub struct ScriptDebugger {
    breakpoints: BTreeSet<usize>,
    paused: Option<PauseState>,
}

impl ScriptDebugger {
    /// Creates a debugger with no breakpoints, not paused.
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggles the breakpoint on `line`, returning whether it is now set. If a
    /// breakpoint is cleared while the debugger is paused on that same line,
    /// execution resumes.
    pub fn toggle_breakpoint(&mut self, line: usize) -> bool {
        let now_set = if self.breakpoints.remove(&line) {
            false
        } else {
            self.breakpoints.insert(line);
            true
        };
        if !now_set && self.paused.as_ref().is_some_and(|p| p.line == line) {
            self.paused = None;
        }
        now_set
    }

    /// Whether `line` has a breakpoint.
    pub fn has_breakpoint(&self, line: usize) -> bool {
        self.breakpoints.contains(&line)
    }

    /// All breakpoint lines, ascending.
    pub fn breakpoints(&self) -> Vec<usize> {
        self.breakpoints.iter().copied().collect()
    }

    /// Whether execution is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    /// The pause state (stack/locals), if paused.
    pub fn paused(&self) -> Option<&PauseState> {
        self.paused.as_ref()
    }

    /// Called by the running project as it reaches `line` with the given call
    /// stack and locals. If a breakpoint is set there (and we're not already
    /// paused), execution halts: the state is captured and `true` is returned.
    /// Otherwise execution continues and `false` is returned.
    pub fn reach_line(
        &mut self,
        line: usize,
        stack: Vec<Frame>,
        locals: BTreeMap<String, String>,
    ) -> bool {
        if self.paused.is_some() || !self.breakpoints.contains(&line) {
            return false;
        }
        self.paused = Some(PauseState {
            line,
            stack,
            locals,
        });
        true
    }

    /// Resumes execution, clearing the pause state. Returns whether it had been
    /// paused.
    pub fn resume(&mut self) -> bool {
        self.paused.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(func: &str, line: usize) -> Frame {
        Frame {
            function: func.to_string(),
            line,
        }
    }

    fn locals(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Acceptance (pat-o1odk): toggling a breakpoint registers it so a running
    /// project halts at that line exposing the stack/locals; clearing it resumes
    /// normal execution.
    #[test]
    fn script_nav_breakpoints_debugger() {
        let mut dbg = ScriptDebugger::new();

        // Toggle a breakpoint on line 10; it's registered.
        assert!(dbg.toggle_breakpoint(10));
        assert!(dbg.has_breakpoint(10));
        assert_eq!(dbg.breakpoints(), vec![10]);

        // Running past a non-breakpoint line continues without pausing.
        assert!(!dbg.reach_line(5, vec![frame("_process", 5)], locals(&[])));
        assert!(!dbg.is_paused());

        // Reaching the breakpoint halts and exposes stack + locals.
        let halted = dbg.reach_line(
            10,
            vec![frame("_process", 10), frame("_ready", 3)],
            locals(&[("hp", "3"), ("name", "hero")]),
        );
        assert!(halted);
        assert!(dbg.is_paused());
        let state = dbg.paused().unwrap();
        assert_eq!(state.line, 10);
        assert_eq!(state.stack[0], frame("_process", 10));
        assert_eq!(state.stack.len(), 2);
        assert_eq!(state.locals.get("hp").map(String::as_str), Some("3"));
        assert_eq!(state.locals.get("name").map(String::as_str), Some("hero"));

        // While paused, further line reaches are ignored (still paused).
        assert!(!dbg.reach_line(11, vec![frame("_process", 11)], locals(&[])));
        assert!(dbg.is_paused());

        // Resuming clears the pause; execution continues.
        assert!(dbg.resume());
        assert!(!dbg.is_paused());
        assert!(!dbg.resume()); // already running

        // The breakpoint still halts on the next hit.
        assert!(dbg.reach_line(10, vec![frame("_process", 10)], locals(&[("hp", "1")])));
        assert!(dbg.is_paused());

        // Clearing the breakpoint at the paused line resumes execution…
        assert!(!dbg.toggle_breakpoint(10));
        assert!(!dbg.has_breakpoint(10));
        assert!(!dbg.is_paused());

        // …and execution no longer halts there.
        assert!(!dbg.reach_line(10, vec![frame("_process", 10)], locals(&[])));
        assert!(!dbg.is_paused());
    }

    /// Multiple breakpoints are tracked sorted; toggling clears them.
    #[test]
    fn multiple_breakpoints() {
        let mut dbg = ScriptDebugger::new();
        dbg.toggle_breakpoint(20);
        dbg.toggle_breakpoint(5);
        dbg.toggle_breakpoint(12);
        assert_eq!(dbg.breakpoints(), vec![5, 12, 20]);
        assert!(!dbg.toggle_breakpoint(12));
        assert_eq!(dbg.breakpoints(), vec![5, 20]);
    }
}
