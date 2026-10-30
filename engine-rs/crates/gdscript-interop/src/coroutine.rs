//! GDScript coroutine / `await` support (pat-p0s82.7).
//!
//! When the interpreter hits `await <expr>`, it suspends the current function,
//! stores remaining statements, and resumes later via:
//! - [`CoroutineHost::poll`] — advances timer waits by `delta` seconds
//! - [`CoroutineHost::notify_signal`] — resumes signal waits
//!
//! Awaitable values are dictionaries with key `"__await_kind"`:
//! - `"timer"` + `"seconds"` (f64)
//! - `"signal"` + `"name"` (String)
//! - `"frames"` + `"count"` (i64)

use std::collections::HashMap;

use gdvariant::{Dictionary, Variant};

use crate::parser::{Expr, Stmt};

/// What a suspended coroutine is waiting for.
#[derive(Debug, Clone, PartialEq)]
pub enum AwaitTarget {
    /// Wait until `remaining` seconds of poll-delta accumulate.
    Timer {
        /// Seconds left until resume.
        remaining: f32,
    },
    /// Wait until `notify_signal` is called with this name.
    Signal {
        /// Signal name (e.g. `"timeout"`, `"done"`).
        name: String,
        /// Raw `ObjectId` of the object whose signal is awaited, when the
        /// awaitable was bound to one (`await $ButtonA.pressed`).
        ///
        /// `None` is the legacy name-only form — e.g. a `create_timer`
        /// `timeout`, whose signal name is already unique per timer. Matching
        /// on the name ALONE is why `await $ButtonA.pressed` used to be resumed
        /// by `$ButtonB.pressed` as well (pat-0q2kx.9).
        object_id: Option<u64>,
    },
    /// Wait for N process frames (`poll` once = one frame if delta>0, or
    /// use [`CoroutineHost::tick_frame`]).
    Frames {
        /// Frames remaining.
        remaining: u32,
    },
    /// Wait for another coroutine to finish, then take its return value.
    ///
    /// This is `var v = await helper()` where `helper` itself awaits: Godot
    /// parks the caller on the callee's `GDScriptFunctionState` and delivers its
    /// return value on completion. Without it the call returned `Nil`
    /// immediately and the caller ran on regardless (pat-0q2kx.78).
    Coroutine {
        /// The awaited coroutine's id.
        id: u64,
    },
}

impl AwaitTarget {
    /// Parse an awaitable Variant produced by builtins / expressions.
    pub fn from_variant(v: &Variant) -> Option<Self> {
        match v {
            Variant::Dictionary(d) => {
                let kind = match d.get("__await_kind") {
                    Some(Variant::String(s)) => s,
                    _ => return None,
                };
                match kind.as_str() {
                    "timer" => {
                        let secs = match d.get("seconds") {
                            Some(Variant::Float(f)) => f as f32,
                            Some(Variant::Int(i)) => i as f32,
                            _ => 0.0,
                        };
                        Some(Self::Timer {
                            remaining: secs.max(0.0),
                        })
                    }
                    "signal" => {
                        let name = match d.get("name") {
                            Some(Variant::String(s)) => s.clone(),
                            _ => return None,
                        };
                        // Absent for the legacy name-only awaitables.
                        let object_id = match d.get("object_id") {
                            Some(Variant::Int(i)) if i > 0 => Some(i as u64),
                            _ => None,
                        };
                        Some(Self::Signal { name, object_id })
                    }
                    "frames" => {
                        let n = match d.get("count") {
                            Some(Variant::Int(i)) => i.max(0) as u32,
                            Some(Variant::Float(f)) => f.max(0.0) as u32,
                            _ => 1,
                        };
                        Some(Self::Frames {
                            remaining: n.max(1),
                        })
                    }
                    _ => None,
                }
            }
            // A first-class `Signal` value — `await $B.pressed` (pat-0q2kx.9).
            // It carries its object, so the resulting await is bound and will
            // not be resumed by another node's same-named signal.
            Variant::Signal { object_id, name } => Some(Self::Signal {
                name: name.clone(),
                object_id: Some(*object_id),
            }),
            // Bare float/int → treat as timer seconds (test convenience).
            Variant::Float(f) => Some(Self::Timer {
                remaining: (*f as f32).max(0.0),
            }),
            Variant::Int(i) if *i >= 0 => Some(Self::Timer {
                remaining: *i as f32,
            }),
            // Bare string → signal name.
            // A bare string names a signal with no owning object, so it stays
            // in the legacy name-only form.
            Variant::String(s) if !s.is_empty() => Some(Self::Signal {
                name: s.clone(),
                object_id: None,
            }),
            _ => None,
        }
    }

    /// Build the dictionary form of a timer awaitable.
    pub fn timer_variant(seconds: f64) -> Variant {
        let mut d = Dictionary::new();
        d.insert("__await_kind", Variant::String("timer".into()));
        d.insert("seconds", Variant::Float(seconds));
        Variant::Dictionary(d.into())
    }

    /// Build the dictionary form of a signal awaitable.
    pub fn signal_variant(name: &str) -> Variant {
        let mut d = Dictionary::new();
        d.insert("__await_kind", Variant::String("signal".into()));
        d.insert("name", Variant::String(name.into()));
        Variant::Dictionary(d.into())
    }

    /// Like [`signal_variant`](Self::signal_variant) but bound to the object
    /// that owns the signal, so only *that* object's emit resumes the await
    /// (pat-0q2kx.9).
    pub fn signal_variant_from(object_id: u64, name: &str) -> Variant {
        let mut d = Dictionary::new();
        d.insert("__await_kind", Variant::String("signal".into()));
        d.insert("name", Variant::String(name.into()));
        d.insert("object_id", Variant::Int(object_id as i64));
        Variant::Dictionary(d.into())
    }

    /// Build the dictionary form of a frames awaitable.
    pub fn frames_variant(count: i64) -> Variant {
        let mut d = Dictionary::new();
        d.insert("__await_kind", Variant::String("frames".into()));
        d.insert("count", Variant::Int(count));
        Variant::Dictionary(d.into())
    }
}

/// One enclosing construct a suspended function still has to finish.
///
/// Godot's VM resumes at the exact instruction, so a `for` that awaits mid-body
/// carries on iterating (`gdscript_vm.cpp` `GDScriptFunctionState`). A tree-walk
/// interpreter has no instruction pointer, so the equivalent is an explicit
/// stack of "what is left to do" frames, captured as the suspension unwinds and
/// replayed innermost-first on resume.
///
/// ⚠️ Without this, `While`/`For` propagated `Suspend` outward exactly like
/// `Return`: `for e in list: spawn(e); await timer` spawned ONE enemy and
/// silently abandoned the rest, with every top-level-await test still green
/// (pat-0q2kx.78).
#[derive(Debug, Clone)]
pub enum ResumeFrame {
    /// Statements left in a plain block (an `if` body, a loop body, a function
    /// body) after the one that suspended.
    Block {
        /// The not-yet-executed statements, in order.
        remaining: Vec<Stmt>,
    },
    /// A `for` that suspended inside its body: finish the current iteration
    /// (via the `Block` frame pushed beneath this one), then iterate the items
    /// that were never reached.
    For {
        /// The loop variable to rebind per item.
        var: String,
        /// The loop body, re-executed per remaining item.
        body: Vec<Stmt>,
        /// Items after the one being processed when the suspension happened.
        pending_items: Vec<Variant>,
    },
    /// A `while` that suspended inside its body: after the current iteration
    /// finishes, re-test the condition and keep looping.
    While {
        /// The loop condition, re-evaluated each iteration.
        condition: Expr,
        /// The loop body.
        body: Vec<Stmt>,
    },
}

/// Identity for a suspended coroutine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoroutineId(pub u64);

/// A function suspended at an `await` with remaining body statements.
#[derive(Debug, Clone)]
pub struct SuspendedCoroutine {
    /// Unique id.
    pub id: CoroutineId,
    /// Statements still to execute after resume, from the OUTERMOST block (the
    /// function body). Inner constructs are carried by [`frames`](Self::frames).
    pub remaining: Vec<Stmt>,
    /// The enclosing constructs still to finish, innermost first.
    ///
    /// Empty for a top-level `await`, which is why the old flat `remaining`
    /// looked sufficient for so long (pat-0q2kx.78).
    pub frames: Vec<ResumeFrame>,
    /// Local scope snapshot (parameter + local bindings as name→value).
    pub locals: HashMap<String, Variant>,
    /// Instance property snapshot (for class methods). Empty for free funcs.
    pub instance_props: HashMap<String, Variant>,
    /// Method/function name for diagnostics.
    pub function_name: String,
    /// Whether this was a class instance method.
    pub is_instance_method: bool,
    /// What we are waiting for.
    pub wait: AwaitTarget,
    /// Variable name to bind the resumed value to (`var v = await sig`), or
    /// `None` for a bare `await` whose result is discarded (pat-nzgdu.6).
    pub bind_target: Option<String>,
    /// Value delivered on resume — the awaited signal's first emitted argument,
    /// bound to [`bind_target`](Self::bind_target) before the remaining
    /// statements run (pat-nzgdu.6). `Nil` for timer/frame waits.
    pub resume_value: Variant,
    /// print() lines produced before suspend (optional accumulation).
    pub prior_output: Vec<String>,
}

/// Holds suspended coroutines and advances them.
#[derive(Debug, Default)]
pub struct CoroutineHost {
    next_id: u64,
    /// Active suspensions.
    pub suspended: Vec<SuspendedCoroutine>,
    /// Coroutines that became ready this tick (waiting to be run by interpreter).
    pub ready: Vec<SuspendedCoroutine>,
}

impl CoroutineHost {
    /// Create an empty host.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of currently suspended (not yet ready) coroutines.
    pub fn suspended_count(&self) -> usize {
        self.suspended.len()
    }

    /// Number of coroutines ready to resume.
    pub fn ready_count(&self) -> usize {
        self.ready.len()
    }

    /// Register a new suspension. Returns its id.
    pub fn suspend(&mut self, mut co: SuspendedCoroutine) -> CoroutineId {
        self.next_id += 1;
        let id = CoroutineId(self.next_id);
        co.id = id;
        // Zero-duration timer / zero frames become immediately ready.
        match &co.wait {
            AwaitTarget::Timer { remaining } if *remaining <= 0.0 => {
                self.ready.push(co);
            }
            AwaitTarget::Frames { remaining } if *remaining == 0 => {
                self.ready.push(co);
            }
            _ => {
                self.suspended.push(co);
            }
        }
        id
    }

    /// Advance timer waits by `delta` seconds. Moves ready coroutines to
    /// [`Self::ready`]. Does **not** decrement frame waits (use [`tick_frame`]).
    pub fn poll(&mut self, delta: f32) {
        let mut still = Vec::new();
        for mut co in self.suspended.drain(..) {
            let mut done = false;
            if let AwaitTarget::Timer { remaining } = &mut co.wait {
                *remaining -= delta;
                if *remaining <= 0.0 {
                    done = true;
                }
            }
            if done {
                self.ready.push(co);
            } else {
                still.push(co);
            }
        }
        self.suspended = still;
    }

    /// Advance frame-based waits by one frame.
    pub fn tick_frame(&mut self) {
        let mut still = Vec::new();
        for mut co in self.suspended.drain(..) {
            let mut done = false;
            if let AwaitTarget::Frames { remaining } = &mut co.wait {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    done = true;
                }
            }
            if done {
                self.ready.push(co);
            } else {
                still.push(co);
            }
        }
        self.suspended = still;
    }

    /// Resume all coroutines waiting on `signal_name`, binding `value` (the
    /// signal's first emitted argument) as each resumed coroutine's awaited
    /// value (pat-nzgdu.6).
    pub fn notify_signal(&mut self, signal_name: &str, value: Variant) {
        self.notify_signal_from(None, signal_name, value);
    }

    /// Resume coroutines awaiting `signal_name`, emitted by `source` (the raw
    /// `ObjectId` of the emitting object, or `None` when the caller cannot say).
    ///
    /// Matching rules (pat-0q2kx.9):
    /// * an await with no bound object resumes on any matching name — the
    ///   legacy behaviour, which `create_timer`'s per-timer signal names rely on;
    /// * an await bound to an object resumes only when `source` is that object,
    ///   so `await $ButtonA.pressed` ignores `$ButtonB.pressed`;
    /// * a bound await never resumes on an unattributed emit, since we cannot
    ///   confirm it came from the right object.
    pub fn notify_signal_from(&mut self, source: Option<u64>, signal_name: &str, value: Variant) {
        let mut still = Vec::new();
        for mut co in self.suspended.drain(..) {
            let matches = match &co.wait {
                AwaitTarget::Signal { name, object_id } if name == signal_name => {
                    match (object_id, source) {
                        (None, _) => true,
                        (Some(want), Some(got)) => *want == got,
                        (Some(_), None) => false,
                    }
                }
                _ => false,
            };
            if matches {
                co.resume_value = value.clone();
                self.ready.push(co);
            } else {
                still.push(co);
            }
        }
        self.suspended = still;
    }

    /// Wakes anything awaiting coroutine `id`, delivering its return `value`.
    ///
    /// Called when a coroutine's body finally runs to completion, which may be
    /// many resumes after the caller parked on it.
    pub fn notify_coroutine_done(&mut self, id: CoroutineId, value: Variant) {
        let mut i = 0;
        while i < self.suspended.len() {
            let hit = matches!(
                self.suspended[i].wait,
                AwaitTarget::Coroutine { id: waited } if waited == id.0
            );
            if hit {
                let mut co = self.suspended.remove(i);
                co.resume_value = value.clone();
                self.ready.push(co);
            } else {
                i += 1;
            }
        }
    }

    /// Pop all ready coroutines (caller will execute them).
    pub fn take_ready(&mut self) -> Vec<SuspendedCoroutine> {
        std::mem::take(&mut self.ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_variant_parses() {
        let v = AwaitTarget::timer_variant(1.5);
        match AwaitTarget::from_variant(&v) {
            Some(AwaitTarget::Timer { remaining }) => {
                assert!((remaining - 1.5).abs() < 1e-5);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn poll_moves_timer_to_ready() {
        let mut host = CoroutineHost::new();
        host.suspend(SuspendedCoroutine {
            frames: Vec::new(),
            id: CoroutineId(0),
            remaining: vec![],
            locals: HashMap::new(),
            instance_props: HashMap::new(),
            function_name: "f".into(),
            is_instance_method: false,
            wait: AwaitTarget::Timer { remaining: 0.25 },
            bind_target: None,
            resume_value: Variant::Nil,
            prior_output: vec![],
        });
        assert_eq!(host.suspended_count(), 1);
        host.poll(0.1);
        assert_eq!(host.suspended_count(), 1);
        host.poll(0.2);
        assert_eq!(host.suspended_count(), 0);
        assert_eq!(host.ready_count(), 1);
    }

    #[test]
    fn signal_notify_resumes() {
        let mut host = CoroutineHost::new();
        host.suspend(SuspendedCoroutine {
            frames: Vec::new(),
            id: CoroutineId(0),
            remaining: vec![],
            locals: HashMap::new(),
            instance_props: HashMap::new(),
            function_name: "f".into(),
            is_instance_method: false,
            wait: AwaitTarget::Signal {
                name: "done".into(),
                object_id: None,
            },
            bind_target: None,
            resume_value: Variant::Nil,
            prior_output: vec![],
        });
        host.notify_signal("other", Variant::Nil);
        assert_eq!(host.suspended_count(), 1);
        host.notify_signal("done", Variant::Nil);
        assert_eq!(host.ready_count(), 1);
    }

    /// pat-0q2kx.9: an `await` bound to a specific object must ignore a
    /// same-named signal emitted by a *different* object.
    ///
    /// Before this, `AwaitTarget::Signal` carried only a name and
    /// `notify_signal` matched on it globally, so two buttons' `pressed` awaits
    /// cross-resumed each other.
    #[test]
    fn object_bound_signal_await_ignores_other_emitters() {
        fn park(host: &mut CoroutineHost, name: &str, object_id: Option<u64>) {
            host.suspend(SuspendedCoroutine {
                frames: Vec::new(),
                id: CoroutineId(0),
                remaining: vec![],
                locals: HashMap::new(),
                instance_props: HashMap::new(),
                function_name: "f".into(),
                is_instance_method: false,
                wait: AwaitTarget::Signal {
                    name: name.into(),
                    object_id,
                },
                bind_target: None,
                resume_value: Variant::Nil,
                prior_output: vec![],
            });
        }

        // Awaiting object #7's `pressed`.
        let mut host = CoroutineHost::new();
        park(&mut host, "pressed", Some(7));

        // Object #9 emits the SAME signal name — must not resume.
        host.notify_signal_from(Some(9), "pressed", Variant::Nil);
        assert_eq!(
            host.ready_count(),
            0,
            "another object's emit must not resume"
        );
        assert_eq!(host.suspended_count(), 1);

        // An unattributed emit must not resume a bound await either: we cannot
        // confirm it came from #7.
        host.notify_signal("pressed", Variant::Nil);
        assert_eq!(
            host.ready_count(),
            0,
            "unattributed emit must not resume a bound await"
        );
        assert_eq!(host.suspended_count(), 1);

        // The right object does resume it.
        host.notify_signal_from(Some(7), "pressed", Variant::Nil);
        assert_eq!(host.ready_count(), 1);
        assert_eq!(host.suspended_count(), 0);

        // Not vacuous: the legacy name-only await still resumes on any emitter,
        // which is what `create_timer`'s `timeout` relies on.
        let mut legacy = CoroutineHost::new();
        park(&mut legacy, "timeout", None);
        legacy.notify_signal_from(Some(123), "timeout", Variant::Nil);
        assert_eq!(
            legacy.ready_count(),
            1,
            "name-only awaits keep resuming on any source"
        );
    }

    #[test]
    fn concurrent_timers() {
        let mut host = CoroutineHost::new();
        for secs in [0.1_f32, 0.3, 0.2] {
            host.suspend(SuspendedCoroutine {
                frames: Vec::new(),
                id: CoroutineId(0),
                remaining: vec![],
                locals: HashMap::new(),
                instance_props: HashMap::new(),
                function_name: format!("t{secs}"),
                is_instance_method: false,
                wait: AwaitTarget::Timer { remaining: secs },
                bind_target: None,
                resume_value: Variant::Nil,
                prior_output: vec![],
            });
        }
        host.poll(0.15);
        assert_eq!(host.ready_count(), 1); // only 0.1
        host.poll(0.1);
        assert_eq!(host.ready_count(), 2); // +0.2
        host.poll(0.1);
        assert_eq!(host.ready_count(), 3);
    }
}
