//! pat-0q2kx.253 — `ScriptBacktrace`, a captured snapshot of a script call stack.
//!
//! Godot's `Engine.capture_script_backtraces(include_variables)` returns one
//! `ScriptBacktrace` per script language: an immutable record of the frames that
//! were live at the moment of capture, optionally with the variables visible in
//! each.
//!
//! The interpreter already maintained `call_stack_frames` for error messages —
//! `get_stack()` exposes it (pat-0q2kx.174) — so nothing new needs to be tracked
//! at call time. This module is the *snapshot*: the interpreter hands over the
//! frames it already has and gets back a handle a script can query.
//!
//! ## Frame order is REVERSED relative to `call_stack_frames`
//!
//! `call_stack_frames` is a push-order `Vec`, so the outermost function is at
//! index 0 and the innermost is last — and `get_stack()` returns it that way.
//! Godot's `ScriptBacktrace` is the opposite: **frame 0 is the innermost**, the
//! function where the capture happened, walking outwards. [`ScriptBacktrace::
//! from_call_stack`] reverses on the way in so the getters match Godot rather
//! than matching the neighbouring builtin, and both orders are pinned by tests.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gdvariant::Variant;

/// One captured frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BacktraceFrame {
    /// The function that frame was executing.
    pub function: String,
    /// Source file, or empty when the interpreter has no path for the script.
    ///
    /// `SourceLocation` carries line and column but **no file**, so this is
    /// empty for a script run from a string rather than loaded from disk.
    pub file: String,
    /// 1-based source line, or 0 when unknown.
    pub line: i64,
    /// Locals visible in that frame — populated only when the capture asked for
    /// variables.
    pub locals: Vec<(String, Variant)>,
    /// Member variables of the object owning that frame.
    pub members: Vec<(String, Variant)>,
}

/// An immutable snapshot of one language's call stack.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptBacktrace {
    language: String,
    frames: Vec<BacktraceFrame>,
    globals: Vec<(String, Variant)>,
}

impl ScriptBacktrace {
    /// Builds a snapshot from the interpreter's push-ordered call stack.
    ///
    /// `frames` arrives outermost-first (the order `call_stack_frames` keeps)
    /// and is **reversed** here, because Godot indexes a backtrace from the
    /// innermost frame outwards.
    pub fn from_call_stack(language: &str, frames: Vec<BacktraceFrame>) -> Self {
        let mut frames = frames;
        frames.reverse();
        Self {
            language: language.to_string(),
            frames,
            globals: Vec::new(),
        }
    }

    /// Attaches the globals visible at capture time (`include_variables`).
    pub fn with_globals(mut self, globals: Vec<(String, Variant)>) -> Self {
        self.globals = globals;
        self
    }

    /// Godot `get_language_name()`.
    pub fn language_name(&self) -> &str {
        &self.language
    }

    /// Godot `get_frame_count()`.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Godot `is_empty()`.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    fn frame(&self, index: i64) -> Option<&BacktraceFrame> {
        usize::try_from(index).ok().and_then(|i| self.frames.get(i))
    }

    /// Godot `get_frame_function(index)`. Out-of-range yields `""` rather than
    /// an error, matching Godot's tolerance for a stale index.
    pub fn frame_function(&self, index: i64) -> String {
        self.frame(index)
            .map(|f| f.function.clone())
            .unwrap_or_default()
    }

    /// Godot `get_frame_file(index)`.
    pub fn frame_file(&self, index: i64) -> String {
        self.frame(index)
            .map(|f| f.file.clone())
            .unwrap_or_default()
    }

    /// Godot `get_frame_line(index)`. `0` for an unknown or out-of-range frame.
    pub fn frame_line(&self, index: i64) -> i64 {
        self.frame(index).map(|f| f.line).unwrap_or(0)
    }

    /// Godot `get_global_variable_count()`.
    pub fn global_count(&self) -> usize {
        self.globals.len()
    }

    /// Godot `get_global_variable_name(index)`.
    pub fn global_name(&self, index: i64) -> String {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.globals.get(i))
            .map(|(n, _)| n.clone())
            .unwrap_or_default()
    }

    /// Godot `get_global_variable_value(index)`.
    pub fn global_value(&self, index: i64) -> Variant {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.globals.get(i))
            .map(|(_, v)| v.clone())
            .unwrap_or(Variant::Nil)
    }

    /// Godot `get_local_variable_count(frame)`.
    pub fn local_count(&self, frame: i64) -> usize {
        self.frame(frame).map(|f| f.locals.len()).unwrap_or(0)
    }

    /// Godot `get_local_variable_name(frame, index)`.
    pub fn local_name(&self, frame: i64, index: i64) -> String {
        pair_at(self.frame(frame).map(|f| &f.locals), index)
            .map(|(n, _)| n.clone())
            .unwrap_or_default()
    }

    /// Godot `get_local_variable_value(frame, index)`.
    pub fn local_value(&self, frame: i64, index: i64) -> Variant {
        pair_at(self.frame(frame).map(|f| &f.locals), index)
            .map(|(_, v)| v.clone())
            .unwrap_or(Variant::Nil)
    }

    /// Godot `get_member_variable_count(frame)`.
    pub fn member_count(&self, frame: i64) -> usize {
        self.frame(frame).map(|f| f.members.len()).unwrap_or(0)
    }

    /// Godot `get_member_variable_name(frame, index)`.
    pub fn member_name(&self, frame: i64, index: i64) -> String {
        pair_at(self.frame(frame).map(|f| &f.members), index)
            .map(|(n, _)| n.clone())
            .unwrap_or_default()
    }

    /// Godot `get_member_variable_value(frame, index)`.
    pub fn member_value(&self, frame: i64, index: i64) -> Variant {
        pair_at(self.frame(frame).map(|f| &f.members), index)
            .map(|(_, v)| v.clone())
            .unwrap_or(Variant::Nil)
    }

    /// Godot `format(indent_all, indent_frames)` — a human-readable dump.
    ///
    /// Each frame renders as `<function> (<file>:<line>)`, innermost first, so
    /// the top line is where the capture happened.
    pub fn format(&self, indent_all: usize, indent_frames: usize) -> String {
        let outer = " ".repeat(indent_all);
        let inner = " ".repeat(indent_all + indent_frames);
        let mut out = format!("{outer}{} backtrace", self.language);
        for f in &self.frames {
            let file = if f.file.is_empty() {
                "<unknown>"
            } else {
                &f.file
            };
            out.push('\n');
            out.push_str(&format!("{inner}{} ({}:{})", f.function, file, f.line));
        }
        out
    }
}

fn pair_at(list: Option<&Vec<(String, Variant)>>, index: i64) -> Option<&(String, Variant)> {
    let list = list?;
    usize::try_from(index).ok().and_then(|i| list.get(i))
}

// ---------------------------------------------------------------------------
// Handle registry
// ---------------------------------------------------------------------------

/// Captured backtraces, keyed by the raw handle id a script holds.
///
/// Process-global rather than thread_local: a captured backtrace is an inert
/// value, and a debugger or editor reading one on a different thread from the
/// script that captured it is exactly the intended use.
fn store() -> &'static Mutex<HashMap<u64, ScriptBacktrace>> {
    static STORE: OnceLock<Mutex<HashMap<u64, ScriptBacktrace>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Files a captured backtrace and returns its raw handle id.
pub fn insert(raw: u64, backtrace: ScriptBacktrace) {
    store()
        .lock()
        .expect("script backtraces poisoned")
        .insert(raw, backtrace);
}

/// Looks up a captured backtrace.
pub fn get(raw: u64) -> Option<ScriptBacktrace> {
    store()
        .lock()
        .expect("script backtraces poisoned")
        .get(&raw)
        .cloned()
}

/// How many backtraces are currently held.
pub fn count() -> usize {
    store().lock().expect("script backtraces poisoned").len()
}

/// Drops every captured backtrace.
pub fn clear() {
    store().lock().expect("script backtraces poisoned").clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(function: &str, line: i64) -> BacktraceFrame {
        BacktraceFrame {
            function: function.to_string(),
            file: String::new(),
            line,
            locals: Vec::new(),
            members: Vec::new(),
        }
    }

    /// The reversal is the whole point: `call_stack_frames` is outermost-first,
    /// Godot indexes from the innermost.
    #[test]
    fn frame_zero_is_the_innermost_call() {
        let bt = ScriptBacktrace::from_call_stack(
            "GDScript",
            vec![frame("outer", 3), frame("middle", 7), frame("inner", 11)],
        );
        assert_eq!(bt.frame_count(), 3);
        assert_eq!(bt.frame_function(0), "inner", "frame 0 must be innermost");
        assert_eq!(bt.frame_function(1), "middle");
        assert_eq!(bt.frame_function(2), "outer");
        assert_eq!(bt.frame_line(0), 11);
    }

    /// Out-of-range and negative indices answer with empty values rather than
    /// panicking — a script holding a stale index must not take the VM down.
    #[test]
    fn out_of_range_indices_are_inert() {
        let bt = ScriptBacktrace::from_call_stack("GDScript", vec![frame("only", 1)]);
        assert_eq!(bt.frame_function(9), "");
        assert_eq!(bt.frame_function(-1), "");
        assert_eq!(bt.frame_line(9), 0);
        assert_eq!(bt.frame_file(-4), "");
        assert_eq!(bt.local_count(9), 0);
        assert_eq!(bt.local_value(9, 0), Variant::Nil);
        assert_eq!(bt.member_name(-1, 0), "");
    }

    #[test]
    fn an_empty_capture_reports_empty() {
        let bt = ScriptBacktrace::from_call_stack("GDScript", Vec::new());
        assert!(bt.is_empty());
        assert_eq!(bt.frame_count(), 0);
        assert_eq!(bt.language_name(), "GDScript");
    }

    #[test]
    fn locals_and_members_are_per_frame() {
        let mut inner = frame("inner", 2);
        inner.locals = vec![("x".into(), Variant::Int(1))];
        inner.members = vec![("hp".into(), Variant::Int(50))];
        let outer = frame("outer", 1);
        // Push order: outer first.
        let bt = ScriptBacktrace::from_call_stack("GDScript", vec![outer, inner]);

        // `inner` is frame 0 after the reversal, and its variables travel with
        // it — a naive implementation that indexed variables by the original
        // order would report the outer frame's (empty) set here.
        assert_eq!(bt.local_count(0), 1);
        assert_eq!(bt.local_name(0, 0), "x");
        assert_eq!(bt.local_value(0, 0), Variant::Int(1));
        assert_eq!(bt.member_name(0, 0), "hp");
        assert_eq!(bt.member_value(0, 0), Variant::Int(50));
        assert_eq!(bt.local_count(1), 0, "the outer frame has no locals");
    }

    #[test]
    fn globals_are_attached_separately() {
        let bt = ScriptBacktrace::from_call_stack("GDScript", vec![frame("f", 1)])
            .with_globals(vec![("Engine".into(), Variant::Int(7))]);
        assert_eq!(bt.global_count(), 1);
        assert_eq!(bt.global_name(0), "Engine");
        assert_eq!(bt.global_value(0), Variant::Int(7));
        assert_eq!(bt.global_value(5), Variant::Nil);
    }

    #[test]
    fn format_lists_frames_innermost_first() {
        let bt = ScriptBacktrace::from_call_stack(
            "GDScript",
            vec![frame("outer", 3), frame("inner", 9)],
        );
        let text = bt.format(0, 2);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].contains("GDScript"));
        assert!(lines[1].contains("inner"), "got {text:?}");
        assert!(lines[2].contains("outer"), "got {text:?}");
        assert!(
            lines[1].starts_with("  "),
            "frames must honour indent_frames, got {:?}",
            lines[1],
        );
    }

    #[test]
    fn the_registry_round_trips_a_capture() {
        clear();
        let bt = ScriptBacktrace::from_call_stack("GDScript", vec![frame("f", 1)]);
        insert(42, bt.clone());
        assert_eq!(get(42), Some(bt));
        assert_eq!(count(), 1);
        assert_eq!(get(99), None, "an unknown handle must not invent a value");
    }
}
