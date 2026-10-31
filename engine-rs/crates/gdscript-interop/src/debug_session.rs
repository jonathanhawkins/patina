//! The process-global debug session: the handshake between a PAUSED script and
//! the editor's HTTP threads (pat-mgixk.14).
//!
//! # Why this is not a condvar
//!
//! The obvious implementation of "pause the game at a breakpoint" is to block
//! the thread running the script until the editor says step or continue. That
//! **cannot work here**, and the reason is worth stating once: the editor's
//! runtime driver calls `advance_running_main_loop` *while holding the
//! `EditorState` mutex* (`gdeditor/src/editor_server.rs`, `run_runtime_driver`
//! — the lock is taken for the whole frame and released only before the
//! inter-frame sleep). Scripts run inside that frame. So a script that blocked
//! would hold the one lock every HTTP route needs, including the
//! `/api/debug/step_over` and `/api/debug/continue` routes that are the only
//! things able to release it. The editor would deadlock on the first
//! breakpoint, and the step routes could never be reached to prove otherwise.
//!
//! So the pause is a SUSPENSION, not a block — which is also closer to what
//! Godot's own `GDScriptFunctionState` is. The interpreter already has a
//! statement-level suspend/resume machine for `await`
//! (`ControlFlow::Suspend` → `park_coroutine` → `CoroutineHost` → `resume_one`,
//! pat-p0s82.7 / pat-0q2kx.78): it captures the statements still to run, the
//! enclosing loops, and the local scope, and resumes from exactly there. A
//! breakpoint parks the frame through that same machinery and this module holds
//! the two things the machinery does not: what the editor should SEE about the
//! pause, and what the editor asks for NEXT.
//!
//! ⚠️ The pause therefore has to travel through a process GLOBAL, not the
//! thread-local `ENGINE_DEBUGGER`. The script runs on the driver thread (or a
//! `/api/runtime/step` connection thread); the step command arrives on a
//! different connection thread entirely. A thread-local is invisible across
//! that boundary — which is exactly why `drain_engine_debugger_into` has to be
//! called "on the same thread that ran the scripts".

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use gdcore::debugger::BreakpointLocation;
use gdvariant::Variant;

/// The signal name a breakpoint parks on.
///
/// 🔑 The pause reuses the `await` machinery, and the cheapest way in is an
/// `AwaitTarget::Signal` nobody else can emit: `CoroutineHost::poll` and
/// `tick_frame` auto-resume only `Timer` and `Frames` waits, so a `Signal` wait
/// stays parked until something explicitly notifies it — which is exactly a
/// breakpoint's semantics. Adding an `AwaitTarget` variant instead would touch
/// every match on that enum for no behavioural gain.
///
/// ⚠️ The leading `@` is load-bearing: a Godot signal name is an identifier, so
/// no script can declare or emit this one and no `await` in user code can be
/// woken by a debugger resume (or vice versa).
pub const BREAK_SIGNAL: &str = "@debug-break";

/// One frame of the paused call stack, in the shape the editor's debugger panel
/// renders: `(function, script, line)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PausedFrame {
    /// The function the frame is executing.
    pub function: String,
    /// The script resource path it belongs to.
    pub script: String,
    /// 1-based source line the frame is stopped on.
    pub line: usize,
}

/// Everything the editor needs to know about a pause.
///
/// Published by the interpreter the moment it decides to break, and read by the
/// `/api/debug/*` routes from whatever thread they land on.
#[derive(Debug, Clone, Default)]
pub struct PauseSnapshot {
    /// The script that broke.
    pub script: String,
    /// The line execution is stopped BEFORE — the statement has not run yet,
    /// which is what makes `x` read 41 rather than 42 at a breakpoint on
    /// `x += 1`.
    pub line: usize,
    /// The real call stack, innermost frame FIRST.
    pub frames: Vec<PausedFrame>,
    /// Locals as the panel shows them: `(name, type, rendered value)`.
    pub locals: Vec<(String, String, String)>,
    /// The same locals as live values, so `/api/debug/evaluate` can compute
    /// with them instead of re-parsing the rendered strings.
    pub values: Vec<(String, Variant)>,
    /// `self`'s properties, which Godot shows as "Members" beside the locals.
    pub members: Vec<(String, Variant)>,
    /// The suspended coroutine this pause parked, if the frame was parked.
    /// `None` for a break the interpreter could not park (see
    /// [`PauseSnapshot::is_resumable`]).
    pub coroutine: Option<u64>,
    /// Source line of each statement still to run, index-aligned with the
    /// parked coroutine's `remaining`.
    ///
    /// 🔑 This is what makes a SECOND break possible after a step. The
    /// `await` machinery carries no line information — it never needed any —
    /// so without this a stepped-to statement could be executed but not
    /// reported, and `step_over` would look like `continue`.
    pub remaining_lines: Vec<usize>,
}

impl PauseSnapshot {
    /// Whether the editor can actually step this pause.
    ///
    /// A break with no parked coroutine is informational only: the editor can
    /// show where the game stopped but `step_over` has nothing to resume. That
    /// is reported rather than hidden, because a Step button that silently does
    /// nothing is the failure this bead exists to remove.
    pub fn is_resumable(&self) -> bool {
        self.coroutine.is_some()
    }
}

/// What the editor asked the paused script to do next.
///
/// Mirrors Godot's editor debugger buttons (`script_editor_debugger.cpp`:
/// Continue / Step Into / Step Over / Break).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeCommand {
    /// Run until the next breakpoint.
    Continue,
    /// Run one statement, entering calls.
    StepIn,
    /// Run one statement, not entering calls.
    StepOver,
    /// Run until the current function returns.
    StepOut,
}

impl ResumeCommand {
    /// Parses the command names the editor's routes use.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "continue" => Some(Self::Continue),
            "step_in" | "step_into" => Some(Self::StepIn),
            "step_over" | "next" => Some(Self::StepOver),
            "step_out" | "out" => Some(Self::StepOut),
            _ => None,
        }
    }

    /// The `gdcore` step mode this command resumes with.
    pub fn step_mode(self) -> gdcore::debugger::StepMode {
        match self {
            Self::Continue => gdcore::debugger::StepMode::Continue,
            Self::StepIn => gdcore::debugger::StepMode::StepIn,
            Self::StepOver => gdcore::debugger::StepMode::StepOver,
            Self::StepOut => gdcore::debugger::StepMode::StepOut,
        }
    }
}

/// The session's shared state.
#[derive(Debug, Default)]
struct Session {
    /// Breakpoints the editor has set, which the stepping thread copies into
    /// its thread-local `Debugger` at the start of every frame.
    breakpoints: BTreeSet<BreakpointLocation>,
    /// The live pause, or `None` while the game is running.
    pause: Option<PauseSnapshot>,
    /// The command the editor issued, waiting for the game to pick it up.
    resume: Option<ResumeCommand>,
    /// Set while a session is attached. Breakpoints are kept across a detach,
    /// as Godot does — only the live pause is dropped.
    attached: bool,
    /// The editor pressed Break: stop at the next statement, wherever it is.
    break_requested: bool,
}

fn session() -> &'static Mutex<Session> {
    static SESSION: OnceLock<Mutex<Session>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(Session::default()))
}

/// Lock-free mirror of "a session is attached AND something could break".
///
/// 🔑 This exists for ONE caller: the interpreter's per-statement debugger
/// check. That hook runs before every statement of every script in the game,
/// so it cannot afford to take [`session`]'s mutex — a global lock per
/// statement would be a measurable slowdown on every project, debugger or not.
///
/// ⚠️ It deliberately folds in "something could break", not just "attached".
/// The editor attaches a debugger for EVERY Play session, so a flag that meant
/// only `attached` would leave the full check — a `String` allocation for the
/// breakpoint key plus a set lookup — running before every statement of every
/// normal playtest. With no breakpoints set, nothing paused and no command
/// pending, there is provably nothing to find.
///
/// Kept in step by [`with_session`], which recomputes it after every mutation,
/// so no caller has to remember to.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the per-statement debugger check should do any work at all.
///
/// Call this FIRST in the interpreter hook and return early when it is false.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Runs `f` against the session.
///
/// Poison-tolerant: a panic inside one `/api/debug/*` handler must not make
/// every later debugger request fail with a poisoned mutex, which would look
/// exactly like the hollow routes this bead replaces.
fn with_session<R>(f: impl FnOnce(&mut Session) -> R) -> R {
    let mut guard = session()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let out = f(&mut guard);
    // Recomputed here, after EVERY mutation, rather than at each call site.
    // One place to be wrong instead of nine, and the per-statement gate stays a
    // single relaxed load. See [`ACTIVE`].
    ACTIVE.store(
        guard.attached
            && (!guard.breakpoints.is_empty()
                || guard.pause.is_some()
                || guard.resume.is_some()
                || guard.break_requested),
        Ordering::Relaxed,
    );
    out
}

// -- attach / teardown ------------------------------------------------------

/// Marks a debug session as attached. Called when Play starts with the
/// debugger enabled.
pub fn attach() {
    with_session(|s| {
        s.attached = true;
        s.pause = None;
        s.resume = None;
        // A Break pressed while nothing was running must not fire on the next
        // Play's first statement.
        s.break_requested = false;
    });
}

/// Ends the session: drops the live pause and any pending command, and KEEPS
/// the breakpoints, matching `Debugger::detach`.
pub fn detach() {
    with_session(|s| {
        s.attached = false;
        s.pause = None;
        s.resume = None;
        s.break_requested = false;
    });
}

/// Marks the session attached WITHOUT disturbing a live pause or a pending
/// command.
///
/// 🔑 This is the one the per-frame host hook must call, not [`attach`].
/// `bind_engine_debugger` runs at the top of every frame, and the step routes
/// drive a frame themselves — so an `attach` there would clear the pause and
/// the command it is about to pick up, and every Step would silently do
/// nothing. `attach` is for the start of a session; this is for every frame of
/// one.
pub fn ensure_attached() {
    with_session(|s| s.attached = true);
}

/// Whether a debug session is attached.
pub fn is_attached() -> bool {
    with_session(|s| s.attached)
}

/// Clears everything, breakpoints included. For tests and a full editor reset —
/// NOT for detach, which keeps breakpoints.
pub fn reset() {
    with_session(|s| *s = Session::default());
}

// -- breakpoints ------------------------------------------------------------

/// Toggles a breakpoint. Returns `true` when it is now set.
pub fn toggle_breakpoint(script: &str, line: usize) -> bool {
    let loc = BreakpointLocation::new(script, line);
    with_session(|s| {
        if s.breakpoints.contains(&loc) {
            s.breakpoints.remove(&loc);
            false
        } else {
            s.breakpoints.insert(loc);
            true
        }
    })
}

/// Sets a breakpoint, replacing nothing. Returns `false` if it was already set.
pub fn add_breakpoint(script: &str, line: usize) -> bool {
    with_session(|s| s.breakpoints.insert(BreakpointLocation::new(script, line)))
}

/// Removes a breakpoint. Returns `true` if it was present.
pub fn remove_breakpoint(script: &str, line: usize) -> bool {
    with_session(|s| s.breakpoints.remove(&BreakpointLocation::new(script, line)))
}

/// Every breakpoint, sorted by script then line.
pub fn breakpoints() -> Vec<BreakpointLocation> {
    with_session(|s| s.breakpoints.iter().cloned().collect())
}

/// The lines breakpointed in one script — what the script editor's gutter
/// renders.
pub fn breakpoint_lines(script: &str) -> Vec<usize> {
    with_session(|s| {
        s.breakpoints
            .iter()
            .filter(|bp| bp.script == script)
            .map(|bp| bp.line)
            .collect()
    })
}

/// Drops every breakpoint.
pub fn clear_breakpoints() {
    with_session(|s| s.breakpoints.clear());
}

// -- the pause --------------------------------------------------------------

/// Publishes a pause. Called by the interpreter at the moment it breaks.
pub fn publish_pause(snapshot: PauseSnapshot) {
    with_session(|s| s.pause = Some(snapshot));
}

/// The live pause, cloned so the caller does not hold the session lock while
/// it renders a response.
pub fn pause() -> Option<PauseSnapshot> {
    with_session(|s| s.pause.clone())
}

/// Whether the game is stopped at a breakpoint or step.
pub fn is_paused() -> bool {
    with_session(|s| s.pause.is_some())
}

/// Drops the live pause without resuming anything — the teardown half of Stop.
pub fn clear_pause() {
    with_session(|s| s.pause = None);
}

// -- the resume handshake ---------------------------------------------------

/// Records what the editor asked for. A second command before the game has
/// picked the first one up REPLACES it: the editor's buttons are a live
/// intent, not a queue, and replaying a stale Step after the user pressed
/// Continue would step a frame nobody asked to step.
///
/// Returns `false` when there is no pause to resume, which is what the route
/// reports instead of pretending it stepped.
pub fn request_resume(command: ResumeCommand) -> bool {
    with_session(|s| {
        if s.pause.is_none() {
            return false;
        }
        s.resume = Some(command);
        true
    })
}

/// Takes the pending command AND clears the pause, so the game resumes exactly
/// once per press.
///
/// ⚠️ One operation, not two. Reading the command and clearing the pause under
/// separate locks would let a second `request_resume` land in between and be
/// silently dropped — the editor's Step would do nothing, which is
/// indistinguishable from the bug this replaces.
pub fn take_resume() -> Option<(ResumeCommand, PauseSnapshot)> {
    with_session(|s| {
        let command = s.resume.take()?;
        let snapshot = s.pause.take()?;
        Some((command, snapshot))
    })
}

/// The pending command without consuming it, for a status route.
pub fn pending_resume() -> Option<ResumeCommand> {
    with_session(|s| s.resume)
}

// -- the Break button ------------------------------------------------------

/// Asks the running game to stop at its next statement (Godot's Break button).
///
/// ⚠️ This cannot arm the step mode itself: the step mode lives in the
/// interpreter's THREAD-LOCAL debugger, and this call arrives on an HTTP
/// connection thread. So the request is parked here and the stepping thread
/// picks it up at the top of its next frame. That is also why Break is
/// observable one frame later rather than instantly — the same frame boundary
/// Godot's own break goes through.
pub fn request_break() {
    with_session(|s| s.break_requested = true);
}

/// Takes the pending Break request, if any. Called by the stepping thread.
pub fn take_break_request() -> bool {
    with_session(|s| std::mem::take(&mut s.break_requested))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The session is a process global and these tests mutate it, so they are
    /// serialized. Poison-tolerant for the same reason `with_session` is.
    static GUARD: Mutex<()> = Mutex::new(());

    fn fresh() -> std::sync::MutexGuard<'static, ()> {
        let guard = GUARD.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        guard
    }

    fn snapshot_at(script: &str, line: usize) -> PauseSnapshot {
        PauseSnapshot {
            script: script.to_string(),
            line,
            frames: vec![PausedFrame {
                function: "_ready".to_string(),
                script: script.to_string(),
                line,
            }],
            locals: vec![("x".to_string(), "int".to_string(), "41".to_string())],
            values: vec![("x".to_string(), Variant::Int(41))],
            members: Vec::new(),
            coroutine: Some(7),
            remaining_lines: vec![line, line + 1],
        }
    }

    #[test]
    fn toggle_sets_then_clears_a_breakpoint() {
        let _g = fresh();
        assert!(toggle_breakpoint("res://main.gd", 3));
        assert_eq!(breakpoint_lines("res://main.gd"), vec![3]);
        assert!(!toggle_breakpoint("res://main.gd", 3));
        assert!(breakpoint_lines("res://main.gd").is_empty());
    }

    #[test]
    fn breakpoint_lines_are_per_script() {
        let _g = fresh();
        add_breakpoint("res://a.gd", 1);
        add_breakpoint("res://b.gd", 2);
        assert_eq!(breakpoint_lines("res://a.gd"), vec![1]);
        assert_eq!(breakpoint_lines("res://b.gd"), vec![2]);
        assert_eq!(breakpoints().len(), 2);
    }

    /// Godot keeps breakpoints across a detach and drops only the live pause;
    /// losing them would silently disarm every gutter marker the user set.
    #[test]
    fn detach_keeps_breakpoints_and_drops_the_pause() {
        let _g = fresh();
        attach();
        add_breakpoint("res://main.gd", 3);
        publish_pause(snapshot_at("res://main.gd", 3));
        assert!(is_paused());

        detach();
        assert!(!is_attached());
        assert!(!is_paused(), "the live pause must not survive a detach");
        assert_eq!(
            breakpoint_lines("res://main.gd"),
            vec![3],
            "breakpoints must survive a detach, as Godot's do"
        );
    }

    /// The regression this guards is the whole reason `ensure_attached` exists:
    /// the host hook runs every frame, and the step routes drive a frame, so an
    /// `attach()` on that path would eat the pause and the command before the
    /// game could pick them up.
    #[test]
    fn ensure_attached_does_not_eat_a_live_pause_or_command() {
        let _g = fresh();
        attach();
        publish_pause(snapshot_at("res://main.gd", 3));
        assert!(request_resume(ResumeCommand::StepOver));

        ensure_attached();

        assert!(is_active());
        assert!(is_paused(), "a per-frame re-attach must keep the pause");
        assert_eq!(
            pending_resume(),
            Some(ResumeCommand::StepOver),
            "...and the command the game has not picked up yet"
        );

        // For contrast: the session-start `attach` deliberately clears both.
        attach();
        assert!(!is_paused());
        assert_eq!(pending_resume(), None);
    }

    #[test]
    fn resume_is_refused_when_nothing_is_paused() {
        let _g = fresh();
        attach();
        assert!(
            !request_resume(ResumeCommand::StepOver),
            "stepping with no pause must be reported, not silently accepted"
        );
        assert!(take_resume().is_none());
    }

    /// The whole point of `take_resume` being one operation.
    #[test]
    fn take_resume_hands_over_the_command_and_the_frame_together() {
        let _g = fresh();
        attach();
        publish_pause(snapshot_at("res://main.gd", 3));
        assert!(request_resume(ResumeCommand::StepOver));

        let (command, snap) = take_resume().expect("a pause and a command were both set");
        assert_eq!(command, ResumeCommand::StepOver);
        assert_eq!(snap.line, 3);
        assert_eq!(snap.coroutine, Some(7));
        assert_eq!(snap.values, vec![("x".to_string(), Variant::Int(41))]);

        // ...and the pause is gone, so a second poll cannot resume twice.
        assert!(!is_paused());
        assert!(
            take_resume().is_none(),
            "one press must resume exactly once"
        );
    }

    /// A later press REPLACES an unconsumed earlier one.
    #[test]
    fn a_second_command_replaces_an_unconsumed_first() {
        let _g = fresh();
        attach();
        publish_pause(snapshot_at("res://main.gd", 3));
        assert!(request_resume(ResumeCommand::StepOver));
        assert!(request_resume(ResumeCommand::Continue));
        assert_eq!(pending_resume(), Some(ResumeCommand::Continue));
        let (command, _) = take_resume().unwrap();
        assert_eq!(
            command,
            ResumeCommand::Continue,
            "the newest press wins; replaying the stale Step would step a \
             frame nobody asked for"
        );
    }

    /// The lock-free flag gates the per-statement hook, so it has to mean
    /// "attached AND something could break" — not merely "attached".
    ///
    /// 🔑 The editor attaches a debugger for EVERY Play session. A flag that
    /// tracked only `attached` would leave the full breakpoint check — a
    /// `String` allocation plus a set lookup — running before every statement
    /// of every ordinary playtest, for a session with provably nothing to find.
    #[test]
    fn the_active_flag_means_something_could_actually_break() {
        let _g = fresh();
        assert!(!is_active(), "a reset session is not active");

        attach();
        assert!(
            !is_active(),
            "attached with no breakpoints, no pause and no command: there is \
             nothing for the per-statement hook to find, so it must stay off"
        );

        add_breakpoint("res://main.gd", 3);
        assert!(is_active(), "one breakpoint arms the hook");

        remove_breakpoint("res://main.gd", 3);
        assert!(
            !is_active(),
            "...and removing the last one disarms it again"
        );

        // A live pause arms it too: that is how the resumed frame's statements
        // get checked after a Step.
        publish_pause(snapshot_at("res://main.gd", 3));
        assert!(is_active(), "a live pause arms the hook");

        detach();
        assert!(
            !is_active(),
            "detach must disarm it whatever else is set, or a finished run \
             keeps taxing the next one"
        );
    }

    /// Break is a one-shot: a single press must stop the game once, not arm
    /// every frame from then on.
    #[test]
    fn a_break_request_is_taken_exactly_once() {
        let _g = fresh();
        attach();
        assert!(!take_break_request(), "nothing is pending before a press");
        request_break();
        assert!(take_break_request(), "the press must be delivered");
        assert!(
            !take_break_request(),
            "and delivered ONCE — a sticky request would break on every \
             statement of every later frame"
        );
    }

    #[test]
    fn command_names_match_the_editor_routes() {
        assert_eq!(
            ResumeCommand::from_name("step_over"),
            Some(ResumeCommand::StepOver)
        );
        assert_eq!(
            ResumeCommand::from_name("next"),
            Some(ResumeCommand::StepOver)
        );
        assert_eq!(
            ResumeCommand::from_name("step_into"),
            Some(ResumeCommand::StepIn)
        );
        assert_eq!(ResumeCommand::from_name("nope"), None);
    }

    /// A break the interpreter could not park is informational only, and says
    /// so rather than offering a Step button that does nothing.
    #[test]
    fn a_pause_with_no_coroutine_is_not_resumable() {
        let _g = fresh();
        let mut snap = snapshot_at("res://main.gd", 3);
        snap.coroutine = None;
        assert!(!snap.is_resumable());
        assert!(snapshot_at("res://main.gd", 3).is_resumable());
    }
}
