//! Event tracing system for lifecycle, signal, and script call ordering.
//!
//! The [`EventTrace`] records a global, ordered log of events that occur
//! during scene tree execution. This enables verification of Godot-compatible
//! dispatch ordering (e.g. ENTER_TREE top-down, READY bottom-up, signal
//! emission order).
//!
//! Each [`TraceEvent`] captures what happened (notification, signal, script
//! call/return), where it happened (node path), and when (frame number).

/// The type of event being traced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEventType {
    /// A notification was dispatched to a node.
    Notification,
    /// A signal was emitted from a node.
    SignalEmit,
    /// A script method was called on a node.
    ScriptCall,
    /// A script method returned on a node.
    ScriptReturn,
}

/// A single recorded event in the trace log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEvent {
    /// What kind of event occurred.
    pub event_type: TraceEventType,
    /// The path (or name) of the node involved.
    pub node_path: String,
    /// A human-readable description of the event
    /// (e.g. "NOTIFICATION_ENTER_TREE", "signal:body_entered", "_ready").
    pub detail: String,
    /// The frame number when this event was recorded.
    pub frame: u64,
}

/// A global event trace that records ordered events during scene execution.
///
/// Enable the trace before running frames, then inspect [`events()`] to
/// verify ordering. Disabled by default to avoid overhead in production.
#[derive(Debug, Clone, Default)]
pub struct EventTrace {
    events: Vec<TraceEvent>,
    enabled: bool,
}

impl EventTrace {
    /// Creates a new, disabled trace.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            enabled: false,
        }
    }

    /// Records an event if tracing is enabled.
    pub fn record(&mut self, event: TraceEvent) {
        if self.enabled {
            self.events.push(event);
        }
    }

    /// Clears all recorded events.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Returns a slice of all recorded events.
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Enables event recording.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disables event recording.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Returns whether tracing is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_disabled_by_default() {
        let trace = EventTrace::new();
        assert!(!trace.is_enabled());
        assert!(trace.events().is_empty());
    }

    #[test]
    fn trace_does_not_record_when_disabled() {
        let mut trace = EventTrace::new();
        trace.record(TraceEvent {
            event_type: TraceEventType::Notification,
            node_path: "/root/A".into(),
            detail: "ENTER_TREE".into(),
            frame: 0,
        });
        assert!(trace.events().is_empty());
    }

    #[test]
    fn trace_records_when_enabled() {
        let mut trace = EventTrace::new();
        trace.enable();
        trace.record(TraceEvent {
            event_type: TraceEventType::Notification,
            node_path: "/root/A".into(),
            detail: "ENTER_TREE".into(),
            frame: 0,
        });
        assert_eq!(trace.events().len(), 1);
        assert_eq!(trace.events()[0].event_type, TraceEventType::Notification);
        assert_eq!(trace.events()[0].node_path, "/root/A");
    }

    #[test]
    fn trace_clear() {
        let mut trace = EventTrace::new();
        trace.enable();
        trace.record(TraceEvent {
            event_type: TraceEventType::ScriptCall,
            node_path: "/root/B".into(),
            detail: "_ready".into(),
            frame: 1,
        });
        assert_eq!(trace.events().len(), 1);
        trace.clear();
        assert!(trace.events().is_empty());
    }

    #[test]
    fn trace_enable_disable_toggle() {
        let mut trace = EventTrace::new();
        trace.enable();
        assert!(trace.is_enabled());
        trace.disable();
        assert!(!trace.is_enabled());
        trace.record(TraceEvent {
            event_type: TraceEventType::SignalEmit,
            node_path: "/root".into(),
            detail: "pressed".into(),
            frame: 0,
        });
        assert!(trace.events().is_empty());
    }

    #[test]
    fn trace_preserves_insertion_order() {
        let mut trace = EventTrace::new();
        trace.enable();
        for i in 0..5 {
            trace.record(TraceEvent {
                event_type: TraceEventType::Notification,
                node_path: format!("/root/N{i}"),
                detail: "ENTER_TREE".into(),
                frame: i,
            });
        }
        assert_eq!(trace.events().len(), 5);
        for (i, ev) in trace.events().iter().enumerate() {
            assert_eq!(ev.frame, i as u64);
            assert_eq!(ev.node_path, format!("/root/N{i}"));
        }
    }
}
