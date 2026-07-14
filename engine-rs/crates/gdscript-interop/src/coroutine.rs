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

use crate::parser::Stmt;

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
    },
    /// Wait for N process frames (`poll` once = one frame if delta>0, or
    /// use [`CoroutineHost::tick_frame`]).
    Frames {
        /// Frames remaining.
        remaining: u32,
    },
}

impl AwaitTarget {
    /// Parse an awaitable Variant produced by builtins / expressions.
    pub fn from_variant(v: &Variant) -> Option<Self> {
        match v {
            Variant::Dictionary(d) => {
                let kind = match d.get("__await_kind") {
                    Some(Variant::String(s)) => s.as_str(),
                    _ => return None,
                };
                match kind {
                    "timer" => {
                        let secs = match d.get("seconds") {
                            Some(Variant::Float(f)) => *f as f32,
                            Some(Variant::Int(i)) => *i as f32,
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
                        Some(Self::Signal { name })
                    }
                    "frames" => {
                        let n = match d.get("count") {
                            Some(Variant::Int(i)) => (*i).max(0) as u32,
                            Some(Variant::Float(f)) => (*f).max(0.0) as u32,
                            _ => 1,
                        };
                        Some(Self::Frames { remaining: n.max(1) })
                    }
                    _ => None,
                }
            }
            // Bare float/int → treat as timer seconds (test convenience).
            Variant::Float(f) => Some(Self::Timer {
                remaining: (*f as f32).max(0.0),
            }),
            Variant::Int(i) if *i >= 0 => Some(Self::Timer {
                remaining: *i as f32,
            }),
            // Bare string → signal name.
            Variant::String(s) if !s.is_empty() => Some(Self::Signal { name: s.clone() }),
            _ => None,
        }
    }

    /// Build the dictionary form of a timer awaitable.
    pub fn timer_variant(seconds: f64) -> Variant {
        let mut d = Dictionary::new();
        d.insert("__await_kind", Variant::String("timer".into()));
        d.insert("seconds", Variant::Float(seconds));
        Variant::Dictionary(d)
    }

    /// Build the dictionary form of a signal awaitable.
    pub fn signal_variant(name: &str) -> Variant {
        let mut d = Dictionary::new();
        d.insert("__await_kind", Variant::String("signal".into()));
        d.insert("name", Variant::String(name.into()));
        Variant::Dictionary(d)
    }

    /// Build the dictionary form of a frames awaitable.
    pub fn frames_variant(count: i64) -> Variant {
        let mut d = Dictionary::new();
        d.insert("__await_kind", Variant::String("frames".into()));
        d.insert("count", Variant::Int(count));
        Variant::Dictionary(d)
    }
}

/// Identity for a suspended coroutine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoroutineId(pub u64);

/// A function suspended at an `await` with remaining body statements.
#[derive(Debug, Clone)]
pub struct SuspendedCoroutine {
    /// Unique id.
    pub id: CoroutineId,
    /// Statements still to execute after resume.
    pub remaining: Vec<Stmt>,
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
        let mut still = Vec::new();
        for mut co in self.suspended.drain(..) {
            let matches = matches!(&co.wait, AwaitTarget::Signal { name } if name == signal_name);
            if matches {
                co.resume_value = value.clone();
                self.ready.push(co);
            } else {
                still.push(co);
            }
        }
        self.suspended = still;
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
            id: CoroutineId(0),
            remaining: vec![],
            locals: HashMap::new(),
            instance_props: HashMap::new(),
            function_name: "f".into(),
            is_instance_method: false,
            wait: AwaitTarget::Signal {
                name: "done".into(),
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

    #[test]
    fn concurrent_timers() {
        let mut host = CoroutineHost::new();
        for secs in [0.1_f32, 0.3, 0.2] {
            host.suspend(SuspendedCoroutine {
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
