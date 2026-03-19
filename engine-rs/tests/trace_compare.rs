//! Frame trace comparison library — aligns Patina and upstream event traces
//! and reports parity differences.
//!
//! Used by pat-9j5 to compare Patina frame traces against upstream (Godot) goldens.

use serde_json::Value;
use std::fmt;

/// A single trace event extracted from JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEvent {
    pub event_type: String,
    pub node_path: String,
    pub detail: String,
    pub frame: u64,
}

impl fmt::Display for TraceEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[frame={} {} {} {}]",
            self.frame, self.event_type, self.detail, self.node_path
        )
    }
}

/// A difference found between two traces.
#[derive(Debug)]
pub enum TraceDiff {
    /// Event exists in upstream but not in Patina.
    MissingInPatina { index: usize, event: TraceEvent },
    /// Event exists in Patina but not in upstream.
    ExtraInPatina { index: usize, event: TraceEvent },
    /// Events at the same logical position differ.
    Mismatch {
        index: usize,
        upstream: TraceEvent,
        patina: TraceEvent,
        field: String,
    },
    /// Per-frame event ordering difference (same events, different order).
    OrderingDiff {
        frame: u64,
        detail: String,
        upstream_order: Vec<String>,
        patina_order: Vec<String>,
    },
}

impl fmt::Display for TraceDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceDiff::MissingInPatina { index, event } => {
                write!(f, "MISSING in Patina [index={index}]: {event}")
            }
            TraceDiff::ExtraInPatina { index, event } => {
                write!(f, "EXTRA in Patina [index={index}]: {event}")
            }
            TraceDiff::Mismatch {
                index,
                upstream,
                patina,
                field,
            } => {
                write!(
                    f,
                    "MISMATCH at [index={index}] field={field}: upstream={upstream} patina={patina}"
                )
            }
            TraceDiff::OrderingDiff {
                frame,
                detail,
                upstream_order,
                patina_order,
            } => {
                write!(
                    f,
                    "ORDERING at frame={frame} for {detail}:\n  upstream: {:?}\n  patina:   {:?}",
                    upstream_order, patina_order
                )
            }
        }
    }
}

/// Parse event trace JSON array into structured events.
pub fn parse_events(trace: &Value) -> Vec<TraceEvent> {
    trace
        .as_array()
        .expect("event_trace should be an array")
        .iter()
        .map(|ev| TraceEvent {
            event_type: ev["event_type"].as_str().unwrap_or_default().to_string(),
            node_path: ev["node_path"].as_str().unwrap_or_default().to_string(),
            detail: ev["detail"].as_str().unwrap_or_default().to_string(),
            frame: ev["frame"].as_u64().unwrap_or(0),
        })
        .collect()
}

/// Compare two event traces and return all differences.
///
/// This performs two levels of comparison:
/// 1. **Sequential diff**: walks both traces in order and reports mismatches,
///    extras, and missing events.
/// 2. **Per-frame ordering diff**: for each frame, groups events by detail type
///    and compares the node ordering within each group.
pub fn compare_traces(upstream: &[TraceEvent], patina: &[TraceEvent]) -> Vec<TraceDiff> {
    let mut diffs = Vec::new();

    // Sequential comparison using longest-common-subsequence alignment.
    sequential_diff(upstream, patina, &mut diffs);

    // Per-frame ordering comparison.
    ordering_diff(upstream, patina, &mut diffs);

    diffs
}

/// Simple sequential walk comparing events at each position.
fn sequential_diff(upstream: &[TraceEvent], patina: &[TraceEvent], diffs: &mut Vec<TraceDiff>) {
    let max_len = upstream.len().max(patina.len());

    let mut ui = 0;
    let mut pi = 0;

    while ui < upstream.len() && pi < patina.len() {
        let u = &upstream[ui];
        let p = &patina[pi];

        if u == p {
            ui += 1;
            pi += 1;
            continue;
        }

        // Check if it's a field mismatch at same logical position.
        if u.frame == p.frame && u.detail == p.detail && u.event_type == p.event_type {
            if u.node_path != p.node_path {
                diffs.push(TraceDiff::Mismatch {
                    index: ui,
                    upstream: u.clone(),
                    patina: p.clone(),
                    field: "node_path".to_string(),
                });
            }
            ui += 1;
            pi += 1;
        } else if u.frame == p.frame {
            // Same frame but different event — try to find match ahead.
            if let Some(ahead) = patina[pi + 1..].iter().position(|e| e == u) {
                // Events in patina before the match are extras.
                for j in pi..pi + ahead + 1 {
                    if j < patina.len() && &patina[j] != u {
                        diffs.push(TraceDiff::ExtraInPatina {
                            index: j,
                            event: patina[j].clone(),
                        });
                    }
                }
                pi += ahead + 1;
                ui += 1;
            } else if let Some(ahead) = upstream[ui + 1..].iter().position(|e| e == p) {
                for j in ui..ui + ahead + 1 {
                    if j < upstream.len() && &upstream[j] != p {
                        diffs.push(TraceDiff::MissingInPatina {
                            index: j,
                            event: upstream[j].clone(),
                        });
                    }
                }
                ui += ahead + 1;
                pi += 1;
            } else {
                diffs.push(TraceDiff::Mismatch {
                    index: ui,
                    upstream: u.clone(),
                    patina: p.clone(),
                    field: "event".to_string(),
                });
                ui += 1;
                pi += 1;
            }
        } else {
            // Different frames — one trace is ahead.
            if u.frame < p.frame {
                diffs.push(TraceDiff::MissingInPatina {
                    index: ui,
                    event: u.clone(),
                });
                ui += 1;
            } else {
                diffs.push(TraceDiff::ExtraInPatina {
                    index: pi,
                    event: p.clone(),
                });
                pi += 1;
            }
        }
    }

    // Remaining events.
    for i in ui..upstream.len().min(max_len) {
        if i < upstream.len() {
            diffs.push(TraceDiff::MissingInPatina {
                index: i,
                event: upstream[i].clone(),
            });
        }
    }
    for i in pi..patina.len().min(max_len) {
        if i < patina.len() {
            diffs.push(TraceDiff::ExtraInPatina {
                index: i,
                event: patina[i].clone(),
            });
        }
    }
}

/// Compare per-frame event ordering for each detail type.
fn ordering_diff(upstream: &[TraceEvent], patina: &[TraceEvent], diffs: &mut Vec<TraceDiff>) {
    let max_frame = upstream
        .iter()
        .chain(patina.iter())
        .map(|e| e.frame)
        .max()
        .unwrap_or(0);

    for frame in 0..=max_frame {
        let upstream_frame: Vec<_> = upstream.iter().filter(|e| e.frame == frame).collect();
        let patina_frame: Vec<_> = patina.iter().filter(|e| e.frame == frame).collect();

        // Group by (event_type, detail) and compare node_path ordering.
        let mut seen_groups = std::collections::HashSet::new();
        for ev in upstream_frame.iter().chain(patina_frame.iter()) {
            seen_groups.insert((ev.event_type.clone(), ev.detail.clone()));
        }

        for (event_type, detail) in &seen_groups {
            let u_order: Vec<String> = upstream_frame
                .iter()
                .filter(|e| &e.event_type == event_type && &e.detail == detail)
                .map(|e| e.node_path.clone())
                .collect();
            let p_order: Vec<String> = patina_frame
                .iter()
                .filter(|e| &e.event_type == event_type && &e.detail == detail)
                .map(|e| e.node_path.clone())
                .collect();

            if u_order != p_order && !u_order.is_empty() && !p_order.is_empty() {
                diffs.push(TraceDiff::OrderingDiff {
                    frame,
                    detail: format!("{event_type}:{detail}"),
                    upstream_order: u_order,
                    patina_order: p_order,
                });
            }
        }
    }
}

/// Produce a human-readable summary report of trace comparison results.
pub fn format_report(
    upstream_name: &str,
    patina_name: &str,
    upstream: &[TraceEvent],
    patina: &[TraceEvent],
    diffs: &[TraceDiff],
) -> String {
    let mut report = String::new();
    report.push_str(&format!(
        "=== Frame Trace Comparison Report ===\n\
         Upstream: {upstream_name} ({} events)\n\
         Patina:   {patina_name} ({} events)\n\n",
        upstream.len(),
        patina.len()
    ));

    if diffs.is_empty() {
        report.push_str("RESULT: FULL PARITY — no differences found.\n");
        return report;
    }

    let missing_count = diffs
        .iter()
        .filter(|d| matches!(d, TraceDiff::MissingInPatina { .. }))
        .count();
    let extra_count = diffs
        .iter()
        .filter(|d| matches!(d, TraceDiff::ExtraInPatina { .. }))
        .count();
    let mismatch_count = diffs
        .iter()
        .filter(|d| matches!(d, TraceDiff::Mismatch { .. }))
        .count();
    let ordering_count = diffs
        .iter()
        .filter(|d| matches!(d, TraceDiff::OrderingDiff { .. }))
        .count();

    report.push_str(&format!(
        "RESULT: {} differences found\n\
         - Missing in Patina: {missing_count}\n\
         - Extra in Patina:   {extra_count}\n\
         - Mismatches:        {mismatch_count}\n\
         - Ordering diffs:    {ordering_count}\n\n",
        diffs.len()
    ));

    for (i, diff) in diffs.iter().enumerate() {
        report.push_str(&format!("{}: {diff}\n", i + 1));
    }

    report
}
