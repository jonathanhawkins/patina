//! The Debugger panel's **stack frames + local variables** view.
//!
//! When the running game hits a breakpoint, the debugger pauses and reports the
//! call stack. The Debugger panel lists those stack frames (innermost first);
//! selecting a frame shows that frame's local variables in the inspector. The
//! innermost frame is selected by default the moment execution pauses. Resuming
//! clears the stack and variables until the next breakpoint.

/// A single local variable in a stack frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVariable {
    /// Variable name (e.g. `"velocity"`).
    pub name: String,
    /// Rendered value (e.g. `"Vector2(0, 0)"`).
    pub value: String,
}

impl LocalVariable {
    /// Convenience constructor.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// One frame of the paused call stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    /// Function / method name for the frame.
    pub function: String,
    /// Source file the frame is executing in.
    pub file: String,
    /// 1-based line number within `file`.
    pub line: u32,
    /// Local variables visible in this frame.
    pub locals: Vec<LocalVariable>,
}

/// The Debugger panel's stack view: the paused call stack plus which frame is
/// selected.
#[derive(Debug, Clone, Default)]
pub struct DebuggerStackPanel {
    frames: Vec<StackFrame>,
    selected: Option<usize>,
    paused: bool,
}

impl DebuggerStackPanel {
    /// Creates an empty, running (not paused) panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether execution is paused at a breakpoint.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Handles hitting a breakpoint: records the reported `frames` (innermost
    /// first), pauses, and selects the innermost frame by default. A breakpoint
    /// with no frames leaves the panel paused with nothing selected.
    pub fn hit_breakpoint(&mut self, frames: Vec<StackFrame>) {
        self.selected = if frames.is_empty() { None } else { Some(0) };
        self.frames = frames;
        self.paused = true;
    }

    /// The paused call stack, innermost frame first.
    pub fn frames(&self) -> &[StackFrame] {
        &self.frames
    }

    /// Selects the stack frame at `index`. Returns whether the index was valid.
    pub fn select_frame(&mut self, index: usize) -> bool {
        if index < self.frames.len() {
            self.selected = Some(index);
            true
        } else {
            false
        }
    }

    /// The index of the selected frame, if any.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The currently selected stack frame, if any.
    pub fn selected_frame(&self) -> Option<&StackFrame> {
        self.selected.and_then(|i| self.frames.get(i))
    }

    /// The local variables shown in the inspector: the selected frame's locals.
    /// Empty when nothing is selected.
    pub fn variables(&self) -> &[LocalVariable] {
        match self.selected_frame() {
            Some(frame) => &frame.locals,
            None => &[],
        }
    }

    /// Resumes execution: clears the stack and selection until the next
    /// breakpoint.
    pub fn resume(&mut self) {
        self.frames.clear();
        self.selected = None;
        self.paused = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(function: &str, line: u32, locals: &[(&str, &str)]) -> StackFrame {
        StackFrame {
            function: function.to_string(),
            file: "res://player.gd".to_string(),
            line,
            locals: locals
                .iter()
                .map(|(n, v)| LocalVariable::new(*n, *v))
                .collect(),
        }
    }

    /// Acceptance (pat-7khnw): hitting a breakpoint populates the stack frames,
    /// and selecting a frame shows its local variables in the inspector.
    #[test]
    fn bottom_debugger_stack_and_variables_populate() {
        let mut panel = DebuggerStackPanel::new();

        // Nothing while running.
        assert!(!panel.is_paused());
        assert!(panel.frames().is_empty());
        assert!(panel.variables().is_empty());

        // Hitting a breakpoint populates the stack (innermost first).
        panel.hit_breakpoint(vec![
            frame("_physics_process", 42, &[("velocity", "Vector2(0, 0)"), ("speed", "300")]),
            frame("_run_step", 17, &[("dt", "0.016")]),
            frame("_ready", 5, &[("self", "Player")]),
        ]);

        assert!(panel.is_paused());
        assert_eq!(panel.frames().len(), 3);

        // The innermost frame is selected by default; its locals show in the
        // inspector.
        assert_eq!(panel.selected_index(), Some(0));
        assert_eq!(panel.selected_frame().unwrap().function, "_physics_process");
        assert_eq!(
            panel.variables(),
            &[
                LocalVariable::new("velocity", "Vector2(0, 0)"),
                LocalVariable::new("speed", "300"),
            ]
        );

        // Selecting another frame swaps the inspector to that frame's locals.
        assert!(panel.select_frame(1));
        assert_eq!(panel.selected_frame().unwrap().function, "_run_step");
        assert_eq!(panel.variables(), &[LocalVariable::new("dt", "0.016")]);

        assert!(panel.select_frame(2));
        assert_eq!(panel.variables(), &[LocalVariable::new("self", "Player")]);

        // Out-of-range selection is rejected and leaves the selection intact.
        assert!(!panel.select_frame(9));
        assert_eq!(panel.selected_index(), Some(2));

        // Resuming clears the stack and variables until the next breakpoint.
        panel.resume();
        assert!(!panel.is_paused());
        assert!(panel.frames().is_empty());
        assert!(panel.variables().is_empty());
        assert_eq!(panel.selected_index(), None);
    }

    /// A breakpoint that reports no frames pauses with nothing selected.
    #[test]
    fn breakpoint_with_no_frames_selects_nothing() {
        let mut panel = DebuggerStackPanel::new();
        panel.hit_breakpoint(vec![]);
        assert!(panel.is_paused());
        assert!(panel.frames().is_empty());
        assert_eq!(panel.selected_index(), None);
        assert!(panel.variables().is_empty());
    }
}
