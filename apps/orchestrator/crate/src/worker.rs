use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use regex::Regex;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::error::Result;
use crate::tmux;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    Idle,
    Active,
    CompletedWaiting,
    StuckInput,
    Dead,
}

#[derive(Debug, Clone)]
pub struct WorkerInfo {
    pub pane_index: u32,
    pub pane_id: String,
    pub worker_name: String,
    pub state: WorkerState,
    pub completed_waiting: bool,
    pub assigned_bead: Option<String>,
    pub assigned_status: Option<String>,
    pub assignment_age_secs: u64,
}

#[derive(Debug, Clone)]
pub struct SwarmHealth {
    pub worker_panes: usize,
    pub assigned_worker_panes: usize,
    pub unassigned_worker_panes: usize,
    pub idle_assigned_panes: usize,
    pub active_assigned_panes: usize,
    pub missing_worker_assignments: usize,
}

// --- Compiled regex patterns (lazy, thread-safe) ---

fn completed_waiting_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)delivered \(message|completion sent via|sent via \./apps/orchestrator/mail/complete|sent via /skill mail-complete|waiting for next assignment|no reopen\.|pat-[a-z0-9]+ (done|complete)|already completed"
        ).unwrap()
    })
}

fn idle_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)waiting for next (bead|assignment|coordinator assignment)|waiting for coordinator verification|waiting\.$|standing by|done\.$|done\. waiting|complete\.$|complete\. waiting|pat-[a-z0-9]+ is done|human operator:|no further action needed|Press up to edit"
        ).unwrap()
    })
}

fn active_work_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // NOTE: Only match patterns that indicate work is CURRENTLY IN PROGRESS.
        // Do NOT include post-completion summaries like "Worked for", "tests passed",
        // "Finished", or "files changed" — these linger in pane scrollback after
        // the worker finishes and cause false-positive Active detection.
        Regex::new(
            r"(?i)Searching for|Reading [0-9]+ file|Running|Editing|Updated|Applied patch|Tool use|Compiling|Clauding|running stop hook|stop hook|[✢✽✶✻].*(ing|Compacting|running stop hook|stop hook)"
        ).unwrap()
    })
}

/// Check if a tmux pane capture shows active work in progress.
pub fn has_active_work(capture: &str) -> bool {
    active_work_re().is_match(capture)
}

/// Check if the capture contains unsubmitted orchestrator prompt text.
/// Flattens newlines first because `tmux capture-pane -p` splits visual line
/// wraps into separate lines, which can break marker substrings in narrow panes.
///
/// Uses two detection strategies:
/// 1. START markers: `/skill flywheel-worker` + `Work bead` (original)
/// 2. END markers: `mail-complete` (or legacy `complete.sh`) + `freehand completion message`
///    — these stay visible in narrow panes where the beginning of the prompt scrolls off.
///
/// NOTE: `tmux::prompt_text_visible_in_capture` has identical logic for
/// post-submit verification. Keep both in sync if prompt format changes.
pub fn has_stuck_input(capture: &str) -> bool {
    // If Claude shows "Press up to edit", the prompt was already delivered to
    // Claude's queue — it is NOT stuck input.  The coordinator should leave it
    // alone and let Claude process it, rather than clearing and retyping which
    // creates an infinite retry loop.
    if capture.contains("Press up to edit") {
        return false;
    }

    let flat = capture.replace('\n', "");
    // Strategy 1: start-of-prompt markers (works in wide panes)
    let start_markers = (flat.contains("/skill flywheel-worker") || flat.contains("/skill patina-fly-worker")) && flat.contains("Work bead");
    // Strategy 2: end-of-prompt markers (works in narrow panes where start scrolls off).
    // Strip ALL whitespace because tmux wraps mid-word at column boundaries,
    // e.g. "compl\n  ete.sh" → "compl  ete.sh" after newline removal.
    let compact = flat.replace(' ', "");
    // Accept both new ("/skillmail-complete") and legacy ("complete.sh") end markers
    // for transition safety — old prompts may still be in tmux buffers.
    let completion_marker = compact.contains("/skillmail-complete") || compact.contains("complete.sh");
    let end_markers = completion_marker && compact.contains("freehandcompletionmessage");
    start_markers || end_markers
}

/// Classify worker state from tmux pane capture text.
///
/// Priority: CompletedWaiting > Active > StuckInput > Idle.
/// Active work (stop hooks, compiling, etc.) takes priority over idle patterns,
/// but CompletedWaiting patterns (explicit completion messages) take highest
/// priority since they represent a deliberate state transition.
/// StuckInput detects unsubmitted orchestrator prompt text sitting in the input buffer.
///
/// `shell_prompt_char` is the character used to detect an idle shell prompt
/// (defaults to '❯' / U+276F).
pub fn detect_pane_state(capture: &str) -> WorkerState {
    detect_pane_state_with(capture, '\u{276f}')
}

/// Like `detect_pane_state` but with a configurable shell prompt character.
pub fn detect_pane_state_with(capture: &str, shell_prompt_char: char) -> WorkerState {
    // Check CompletedWaiting first — explicit completion signals trump everything
    if completed_waiting_re().is_match(capture) {
        // But if a stop hook is *currently running*, the worker is still active.
        // Stop hooks appear as the last line and indicate Claude is still processing.
        let last_lines = last_n_lines(capture, 3);
        if is_stop_hook_active(&last_lines) {
            return WorkerState::Active;
        }
        return WorkerState::CompletedWaiting;
    }

    // Check active work before idle — prevents classifying busy workers as idle
    // when stale "done." text appears in the capture alongside active indicators.
    if has_active_work(capture) {
        return WorkerState::Active;
    }

    // Check for stuck (unsubmitted) orchestrator prompt text.
    // Must come AFTER active_work — if Claude is processing, the prompt text
    // may still be in scrollback but the worker isn't stuck.
    if has_stuck_input(capture) {
        return WorkerState::StuckInput;
    }

    // Detect usage-limited workers — Claude shows "You've hit your limit" when
    // the account quota is exhausted. These workers are effectively dead until
    // the limit resets, so treat them as Idle to allow bead reclamation.
    if capture.contains("hit your limit") || capture.contains("extra-usage") {
        return WorkerState::Idle;
    }

    // Check idle patterns
    if idle_re().is_match(capture) {
        return WorkerState::Idle;
    }

    // Shell prompt with no active work → Idle
    if capture.contains(shell_prompt_char) {
        return WorkerState::Idle;
    }

    // Default: assume working
    WorkerState::Active
}

/// Extract the last N lines from a capture string.
fn last_n_lines(capture: &str, n: usize) -> String {
    let lines: Vec<&str> = capture.lines().collect();
    let start = if lines.len() > n { lines.len() - n } else { 0 };
    lines[start..].join("\n")
}

/// Check if the tail of the capture shows an active stop hook.
fn is_stop_hook_active(tail: &str) -> bool {
    let lower = tail.to_lowercase();
    lower.contains("clauding") || lower.contains("running stop hook") || lower.contains("stop hook")
}

/// Returns true if the pane looks idle and could accept a new prompt.
pub fn needs_prompt(capture: &str) -> bool {
    let state = detect_pane_state(capture);
    matches!(state, WorkerState::Idle | WorkerState::CompletedWaiting)
}

/// Extract the bead ID from a pane capture that shows completion text.
/// Returns `Some("pat-XXXX")` if the capture contains patterns like
/// "pat-XXX done", "pat-XXX complete", "pat-XXX is done",
/// "complete.sh pat-XXX", or "mail-complete pat-XXX".
/// Returns `None` if no bead ID is found.
pub fn extract_completed_bead_id(capture: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:complete\.sh\s+(pat-[a-z0-9]+)|mail-complete\s+(pat-[a-z0-9]+)|(pat-[a-z0-9]+)\s+(?:done|complete|is done))"
        ).unwrap()
    });
    re.captures(capture).map(|c| {
        c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).unwrap().as_str().to_string()
    })
}

/// Check if the capture shows a queued prompt for a specific bead.
pub fn has_assignment_prompt(capture: &str, _worker: &str, bead_id: &str) -> bool {
    has_assignment_prompt_with(capture, "Press up to edit", bead_id)
}

/// Like `has_assignment_prompt` but with a configurable queue prompt marker.
pub fn has_assignment_prompt_with(capture: &str, queue_prompt_marker: &str, bead_id: &str) -> bool {
    capture.contains(queue_prompt_marker) && capture.contains(bead_id)
}

/// Shell out to identity-resolve.sh to get the worker name for a pane.
pub fn resolve_worker_identity(project_root: &Path, pane_id: &str) -> Option<String> {
    let script = project_root.join("mcp_agent_mail/scripts/identity-resolve.sh");
    if !script.exists() {
        return None;
    }

    let output = Command::new(&script)
        .arg(project_root)
        .arg(pane_id)
        .output()
        .ok()?;

    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    } else {
        None
    }
}

/// Parse an ISO datetime string and return seconds since then.
/// Returns 0 on parse failure or if the datetime is in the future.
pub fn assignment_age_secs(updated_at: Option<&str>) -> u64 {
    let value = match updated_at {
        Some(v) if !v.trim().is_empty() => v.trim(),
        _ => return 0,
    };

    // Try parsing with timezone info, then without (assuming UTC)
    let dt = value
        .replace("Z", "+00:00")
        .parse::<chrono::DateTime<chrono::FixedOffset>>()
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
                .ok()
                .map(|ndt| ndt.and_utc())
        });

    match dt {
        Some(dt) => {
            let age = (chrono::Utc::now() - dt).num_seconds();
            if age > 0 { age as u64 } else { 0 }
        }
        None => 0,
    }
}

/// List all worker panes with their state, identity, and assignment info.
pub fn worker_info_list(
    session: &str,
    window: u32,
    db: &Connection,
    project_root: &Path,
) -> Result<Vec<WorkerInfo>> {
    worker_info_list_with(session, window, db, project_root, 3, "claude", '\u{276f}')
}

/// Like `worker_info_list` but with configurable pane filter parameters.
pub fn worker_info_list_with_config(
    session: &str,
    window: u32,
    db: &Connection,
    config: &Config,
) -> Result<Vec<WorkerInfo>> {
    worker_info_list_with(
        session,
        window,
        db,
        &config.project_root,
        config.min_worker_pane_index,
        &config.worker_command,
        config.shell_prompt_char,
    )
}

/// Core implementation with all configurable parameters.
pub fn worker_info_list_with(
    session: &str,
    window: u32,
    db: &Connection,
    project_root: &Path,
    min_worker_pane_index: u32,
    worker_command: &str,
    shell_prompt_char: char,
) -> Result<Vec<WorkerInfo>> {
    let panes = tmux::list_panes(session, window)?;
    let mut workers = Vec::new();

    for pane in &panes {
        // Skip non-worker panes based on configurable thresholds
        if pane.index < min_worker_pane_index || pane.dead || pane.current_command != worker_command {
            continue;
        }

        let worker_name = resolve_worker_identity(project_root, &pane.id)
            .unwrap_or_default();

        let capture = tmux::capture_pane(session, window, pane.index, 80)
            .unwrap_or_default();

        let state = detect_pane_state_with(&capture, shell_prompt_char);
        let cw = completed_waiting_re().is_match(&capture);

        let (assigned_bead, assigned_status, age) = if !worker_name.is_empty() {
            match db::assigned_bead_for_worker(db, &worker_name) {
                Ok(Some(bead)) => {
                    let age = assignment_age_secs(bead.updated_at.as_deref());
                    (
                        Some(bead.id),
                        Some(bead.status.as_str().to_string()),
                        age,
                    )
                }
                _ => (None, None, 0),
            }
        } else {
            (None, None, 0)
        };

        workers.push(WorkerInfo {
            pane_index: pane.index,
            pane_id: pane.id.clone(),
            worker_name,
            state,
            completed_waiting: cw,
            assigned_bead,
            assigned_status,
            assignment_age_secs: age,
        });
    }

    Ok(workers)
}

/// Aggregate swarm health metrics from a worker info list.
pub fn swarm_health(workers: &[WorkerInfo], db: &Connection) -> Result<SwarmHealth> {
    let worker_panes = workers.len();
    let mut assigned_worker_panes = 0;
    let mut idle_assigned_panes = 0;
    let mut active_assigned_panes = 0;

    for w in workers {
        if w.assigned_bead.is_some() {
            assigned_worker_panes += 1;
            match w.state {
                WorkerState::Idle | WorkerState::CompletedWaiting | WorkerState::StuckInput => {
                    idle_assigned_panes += 1;
                }
                WorkerState::Active => {
                    active_assigned_panes += 1;
                }
                WorkerState::Dead => {}
            }
        }
    }

    let unassigned_worker_panes = worker_panes - assigned_worker_panes;

    // Count in_progress beads assigned to workers NOT in our active list
    let active_worker_names: Vec<String> = workers
        .iter()
        .filter(|w| !w.worker_name.is_empty())
        .map(|w| w.worker_name.clone())
        .collect();

    let stale = db::stale_assignments(db, &active_worker_names)?;
    let missing_worker_assignments = stale.len();

    Ok(SwarmHealth {
        worker_panes,
        assigned_worker_panes,
        unassigned_worker_panes,
        idle_assigned_panes,
        active_assigned_panes,
        missing_worker_assignments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_idle_prompt() {
        // Shell prompt with no active work → Idle
        let capture = "Some output\n❯ ";
        assert_eq!(detect_pane_state(capture), WorkerState::Idle);
    }

    #[test]
    fn test_detect_active_compilation() {
        let capture = "Compiling patina-orchestrator v0.1.0\n❯ ";
        assert_eq!(detect_pane_state(capture), WorkerState::Active);
    }

    #[test]
    fn test_detect_completed_waiting() {
        let capture = "All done. completion sent via ./apps/orchestrator/mail/complete\nWaiting.";
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_detect_idle_waiting() {
        let capture = "Finished work.\nWaiting for next assignment.";
        // "waiting for next assignment" matches completed_waiting_re first
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_detect_idle_standing_by() {
        let capture = "No more work. Standing by";
        assert_eq!(detect_pane_state(capture), WorkerState::Idle);
    }

    #[test]
    fn test_has_active_work_true() {
        assert!(has_active_work("Searching for files..."));
        assert!(has_active_work("Reading 5 files"));
        assert!(has_active_work("Compiling crate"));
        assert!(has_active_work("✢ running stop hook"));
    }

    #[test]
    fn test_post_completion_not_active() {
        // Post-completion summaries should NOT be detected as active work —
        // they linger in scrollback and cause false-positive Active detection.
        assert!(!has_active_work("Worked for 2m30s"));
        assert!(!has_active_work("42 unit tests passed"));
        assert!(!has_active_work("Finished dev profile"));
        assert!(!has_active_work("3 files changed"));
    }

    #[test]
    fn test_has_active_work_false() {
        assert!(!has_active_work("❯ "));
        assert!(!has_active_work("Waiting for next bead"));
        assert!(!has_active_work("some random text"));
    }

    #[test]
    fn test_has_assignment_prompt() {
        let capture = "Press up to edit\nWork bead pat-123 as WorkerA, check inbox\n";

        assert!(has_assignment_prompt(capture, "WorkerA", "pat-123"));
        // Worker name doesn't matter — just bead ID + queue marker
        assert!(has_assignment_prompt(capture, "WorkerB", "pat-123"));
        // Different bead ID — should not match
        assert!(!has_assignment_prompt(capture, "WorkerA", "pat-999"));
        // No queue marker — should not match
        assert!(!has_assignment_prompt("Work bead pat-123 as WorkerA", "WorkerA", "pat-123"));
    }

    #[test]
    fn test_needs_prompt() {
        assert!(needs_prompt("❯ "));
        assert!(needs_prompt("completion sent via ./apps/orchestrator/mail/complete"));
        assert!(!needs_prompt("Compiling patina-orchestrator v0.1.0"));
    }

    #[test]
    fn test_assignment_age_secs() {
        // None and empty
        assert_eq!(assignment_age_secs(None), 0);
        assert_eq!(assignment_age_secs(Some("")), 0);
        assert_eq!(assignment_age_secs(Some("   ")), 0);

        // Garbage
        assert_eq!(assignment_age_secs(Some("not-a-date")), 0);

        // A datetime far in the past should return a large positive number
        let age = assignment_age_secs(Some("2020-01-01T00:00:00Z"));
        assert!(age > 100_000_000); // more than ~3 years in seconds

        // A datetime very recently should return a small number
        let now = chrono::Utc::now();
        let recent = (now - chrono::Duration::seconds(30)).to_rfc3339();
        let age = assignment_age_secs(Some(&recent));
        assert!(age >= 28 && age <= 35, "age was {age}");
    }

    #[test]
    fn test_assignment_age_secs_naive_format() {
        // Test the "YYYY-MM-DD HH:MM:SS" format (no timezone)
        let age = assignment_age_secs(Some("2020-01-01 00:00:00"));
        assert!(age > 100_000_000);
    }

    #[test]
    fn test_detect_stop_hook_as_active() {
        // A worker running a stop hook must be Active, never Idle
        let capture = "✻ Clauding… (running stop hook · 8m 10s)";
        assert_eq!(detect_pane_state(capture), WorkerState::Active);

        // Even if the capture also contains idle-looking text from earlier output
        let capture_with_done = "pat-abc done.\n✻ Clauding… (running stop hook · 8m 10s)";
        assert_eq!(detect_pane_state(capture_with_done), WorkerState::Active);
    }

    #[test]
    fn test_detect_clauding_as_active() {
        // Plain "Clauding" text (no special prefix) should be Active
        let capture = "Clauding...";
        assert_eq!(detect_pane_state(capture), WorkerState::Active);
    }

    #[test]
    fn test_detect_waiting_for_assignment_as_completed_waiting() {
        let capture =
            "pat-abc complete. Sent to GrayMountain. Waiting for next assignment.";
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_detect_pat_done_as_completed_waiting() {
        let capture = "pat-xyz done";
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_detect_already_completed_as_completed_waiting() {
        let capture = "Bead pat-abc already completed, skipping.";
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_has_active_work_stop_hook() {
        assert!(has_active_work("Clauding"));
        assert!(has_active_work("running stop hook"));
        assert!(has_active_work("stop hook executing"));
        assert!(has_active_work("✻ Clauding… (running stop hook · 8m 10s)"));
    }

    // --- Tests for configurable variants ---

    #[test]
    fn test_detect_pane_state_custom_prompt_char() {
        // Default ❯ detected as idle
        assert_eq!(detect_pane_state("Some output\n❯ "), WorkerState::Idle);

        // Custom '$' prompt char
        assert_eq!(detect_pane_state_with("Some output\n$ ", '$'), WorkerState::Idle);
        // '❯' not recognized when looking for '$'
        assert_eq!(detect_pane_state_with("Some output\n❯ ", '$'), WorkerState::Active);

        // Custom '>' prompt char
        assert_eq!(detect_pane_state_with("Some output\n> ", '>'), WorkerState::Idle);
    }

    #[test]
    fn test_detect_pane_state_with_preserves_priority() {
        // CompletedWaiting still wins over custom prompt char
        let capture = "completion sent via ./apps/orchestrator/mail/complete\n$ ";
        assert_eq!(detect_pane_state_with(capture, '$'), WorkerState::CompletedWaiting);

        // Active still wins over custom prompt char
        let capture = "Compiling crate\n$ ";
        assert_eq!(detect_pane_state_with(capture, '$'), WorkerState::Active);
    }

    #[test]
    fn test_has_assignment_prompt_custom_marker() {
        let capture = "Queued for edit\nWork bead pat-abc as W, check inbox\n";
        // Default marker doesn't match
        assert!(!has_assignment_prompt(capture, "W", "pat-abc"));
        // Custom marker matches
        assert!(has_assignment_prompt_with(capture, "Queued for edit", "pat-abc"));
        // Absent marker doesn't match
        assert!(!has_assignment_prompt_with(capture, "Press up to edit", "pat-abc"));
    }

    #[test]
    fn test_has_assignment_prompt_with_requires_both() {
        // Only marker, no bead ID
        assert!(!has_assignment_prompt_with("Press up to edit\nsome text", "Press up to edit", "pat-123"));
        // Only bead ID, no marker
        assert!(!has_assignment_prompt_with("Work bead pat-123", "Press up to edit", "pat-123"));
        // Both present
        assert!(has_assignment_prompt_with(
            "Press up to edit\nWork bead pat-123 as Worker, check inbox",
            "Press up to edit",
            "pat-123",
        ));
    }

    // --- Stuck input detection tests ---

    #[test]
    fn test_detect_stuck_input() {
        let capture = "❯ Use /skill flywheel-worker. Work bead pat-123: Fix bug. Your Agent Mail identity is WorkerA.";
        assert_eq!(detect_pane_state(capture), WorkerState::StuckInput);
    }

    #[test]
    fn test_stuck_input_not_triggered_when_active() {
        // If Claude is actively processing, Active wins over stuck detection
        let capture = "✢ Searching for files...\nUse /skill flywheel-worker. Work bead pat-123: Fix bug.";
        assert_eq!(detect_pane_state(capture), WorkerState::Active);
    }

    #[test]
    fn test_stuck_input_with_garbled_text() {
        // Realistic stuck pane: partial prompt text with tmux rendering artifacts
        let capture = "+loa\n+t\" 2069 +\nUse /skill flywheel-worker. Work bead pat-456: Add feature.";
        assert_eq!(detect_pane_state(capture), WorkerState::StuckInput);
    }

    #[test]
    fn test_stuck_input_with_narrow_pane_wrapping() {
        // In narrow panes, tmux wraps the prompt across visual lines.
        // capture-pane -p returns visual lines, so the two marker substrings
        // end up on different lines separated by \n.
        let capture = "❯ Use /skill flywheel-w\norker. Work bead pat-12\n3: Fix bug.";
        assert_eq!(detect_pane_state(capture), WorkerState::StuckInput);
    }

    #[test]
    fn test_has_stuck_input_false_for_normal_output() {
        assert!(!has_stuck_input("Compiling patina v0.1"));
        assert!(!has_stuck_input("❯ "));
        assert!(!has_stuck_input("completion sent via ./apps/orchestrator/mail/complete"));
    }

    /// Regression: "Press up to edit" means Claude queued the prompt — it is NOT
    /// stuck input. Before this fix, the coordinator classified it as StuckInput,
    /// cleared the line, and resubmitted, creating an infinite retry loop that
    /// prevented all queued workers from ever processing their assignments.
    #[test]
    fn test_queued_prompt_is_not_stuck_input() {
        // Prompt text visible + "Press up to edit" = queued, not stuck
        let capture = "Use /skill flywheel-worker. Work bead pat-123: Fix bug.\n❯ Press up to edit queued message";
        assert!(
            !has_stuck_input(capture),
            "Queued prompt (Press up to edit) must NOT be classified as stuck input"
        );
        // Should classify as Idle (not StuckInput) so coordinator leaves it alone
        assert_eq!(detect_pane_state(capture), WorkerState::Idle);
    }

    #[test]
    fn test_queued_prompt_with_end_markers_is_not_stuck() {
        // End markers visible + "Press up to edit" = still queued, not stuck
        let capture = "mail-complete, not a freehand completion message.\n❯ Press up to edit queued message";
        assert!(!has_stuck_input(capture));
    }

    #[test]
    fn test_true_stuck_input_without_press_up() {
        // Raw prompt text without "Press up to edit" IS stuck input
        let capture = "❯ Use /skill flywheel-worker. Work bead pat-123: Fix bug.";
        assert!(has_stuck_input(capture));
        assert_eq!(detect_pane_state(capture), WorkerState::StuckInput);
    }

    // --- Queued prompt regression: boundary tests ---

    /// "Press up to edit" at start of capture (before prompt text)
    #[test]
    fn test_queued_prompt_press_up_before_prompt_text() {
        let capture = "Press up to edit queued message\nUse /skill flywheel-worker. Work bead pat-abc: Fix.\n❯ ";
        assert!(!has_stuck_input(capture));
    }

    /// "Press up to edit" buried in the middle of scrollback
    #[test]
    fn test_queued_prompt_press_up_in_middle() {
        let capture = "some output\nPress up to edit queued message\nmore output\n/skill flywheel-worker. Work bead pat-xyz: Task.";
        assert!(!has_stuck_input(capture));
    }

    /// Partial "Press up" should NOT suppress stuck input detection
    #[test]
    fn test_partial_press_up_does_not_suppress() {
        let capture = "Press up\nUse /skill flywheel-worker. Work bead pat-123: Fix.";
        // "Press up" alone (without " to edit") should NOT suppress
        assert!(has_stuck_input(capture), "Partial 'Press up' must not suppress stuck input");
    }

    /// Case sensitivity: "press up to edit" in different casing should still suppress
    #[test]
    fn test_queued_prompt_exact_case_only() {
        // The actual Claude output is always "Press up to edit" — exact match
        let capture = "Use /skill flywheel-worker. Work bead pat-123: Fix.\nPress up to edit queued message";
        assert!(!has_stuck_input(capture));
        // Lowercase variant — Claude never produces this, so stuck input should trigger
        let capture_lower = "Use /skill flywheel-worker. Work bead pat-123: Fix.\npress up to edit queued message";
        assert!(has_stuck_input(capture_lower), "Only exact 'Press up to edit' should suppress");
    }

    // --- Queued prompt regression: realistic captures from production ---

    /// Realistic 5x5 pane: narrow width causes wrapping, "Press up to edit" on prompt line
    #[test]
    fn test_queued_prompt_narrow_pane_realistic() {
        let capture = "\
⏺ All 63 importer\n\
  tests pass\n\
❯ Use /skill\n\
flywheel-worker. Work\n\
 bead pat-h45pa:\n\
WorkerThreadPool\n\
───────────────────────\n\
❯ Press up to edit que…\n\
───────────────────────\n\
  1 shell · ⏵⏵ bypas…";
        assert!(!has_stuck_input(capture), "Narrow pane queued prompt must not be stuck input");
        // Should NOT be StuckInput
        assert_ne!(detect_pane_state(capture), WorkerState::StuckInput);
    }

    /// Realistic pane: worker finished, new prompt queued, previous output visible
    #[test]
    fn test_queued_prompt_with_previous_work_output() {
        let capture = "\
⏺ Bash(cd /Users/bone/dev/games/patina/engine-rs && cargo test)\n\
  ⎿ Running in\n\
  ⎿  (timeout 2m)\n\
\n\
❯ Use /skill flywheel-worker. Work bead pat-ao5fs: AudioStream import.\n\
Your Agent Mail identity is WhiteBeaver.\n\
───────────────────────\n\
❯ Press up to edit queued message\n\
───────────────────────";
        assert!(!has_stuck_input(capture));
        assert_ne!(detect_pane_state(capture), WorkerState::StuckInput);
    }

    // --- Queued prompt regression: stress test ---

    /// Verify consistent behavior across many different bead IDs and prompt formats
    #[test]
    fn test_queued_prompt_stress_many_beads() {
        for i in 0..100 {
            let bead = format!("pat-{i:05x}");
            let capture = format!(
                "Use /skill flywheel-worker. Work bead {bead}: Task {i}.\n\
                 Close out with /skill mail-complete, not a freehand completion message.\n\
                 ❯ Press up to edit queued message"
            );
            assert!(
                !has_stuck_input(&capture),
                "Queued prompt for bead {bead} incorrectly classified as stuck"
            );
        }
    }

    /// Verify real stuck input is still detected across many bead IDs
    #[test]
    fn test_real_stuck_input_stress_many_beads() {
        for i in 0..100 {
            let bead = format!("pat-{i:05x}");
            let capture = format!(
                "❯ Use /skill flywheel-worker. Work bead {bead}: Task {i}."
            );
            assert!(
                has_stuck_input(&capture),
                "Real stuck input for bead {bead} must be detected"
            );
        }
    }

    // --- Queued prompt regression: interaction with other states ---

    /// CompletedWaiting + queued prompt: CompletedWaiting should still win
    #[test]
    fn test_completed_waiting_wins_over_queued_prompt() {
        let capture = "waiting for next assignment\nUse /skill flywheel-worker. Work bead pat-123: Fix.\n❯ Press up to edit";
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    /// Active work + queued prompt: Active should win
    #[test]
    fn test_active_work_wins_over_queued_prompt() {
        let capture = "✢ Searching for files...\nUse /skill flywheel-worker. Work bead pat-123: Fix.\nPress up to edit";
        assert_eq!(detect_pane_state(capture), WorkerState::Active);
    }

    /// Only "Press up to edit" without prompt markers → plain Idle
    #[test]
    fn test_press_up_without_prompt_markers_is_idle() {
        let capture = "❯ Press up to edit queued message";
        assert!(!has_stuck_input(capture));
        assert_eq!(detect_pane_state(capture), WorkerState::Idle);
    }

    // --- Negative tests ---

    /// Empty capture should never be stuck input
    #[test]
    fn test_empty_capture_not_stuck() {
        assert!(!has_stuck_input(""));
        assert!(!has_stuck_input("Press up to edit"));
    }

    /// Capture with ONLY "Press up to edit" and nothing else
    #[test]
    fn test_only_press_up_no_prompt_not_stuck() {
        let capture = "Press up to edit queued message\n❯ ";
        assert!(!has_stuck_input(capture));
        assert_eq!(detect_pane_state(capture), WorkerState::Idle);
    }

    #[test]
    fn test_completed_waiting_wins_over_stuck_input() {
        // CompletedWaiting has higher priority than StuckInput
        let capture = "completion sent via ./apps/orchestrator/mail/complete\nUse /skill flywheel-worker. Work bead pat-123: Fix.";
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    // ========================================================================
    // Integration tests: exhaustive state machine priority, realistic captures,
    // column-width wrapping, detection consistency, and coordinator contract.
    // ========================================================================

    /// Helper: generate a realistic orchestrator prompt (same format as
    /// coordinator::build_prompt_text) for arbitrary bead/worker combos.
    fn fake_prompt(bead_id: &str, title: &str, worker: &str) -> String {
        format!(
            "Use /skill flywheel-worker. Work bead {bead_id}: {title}. \
             Your Agent Mail identity is {worker}. Read your inbox for full \
             assignment details. Close out with /skill mail-complete, \
             not a freehand completion message."
        )
    }

    /// Helper: simulate tmux visual line wrapping at a given column width.
    /// Splits a long string into lines of at most `cols` characters, joining
    /// with '\n' — exactly what `tmux capture-pane -p` returns for wrapped text.
    fn wrap_at_cols(text: &str, cols: usize) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut lines = Vec::new();
        for chunk in chars.chunks(cols) {
            lines.push(chunk.iter().collect::<String>());
        }
        lines.join("\n")
    }

    // --- Exhaustive state priority tests ---
    //
    // The documented priority is:
    //   CompletedWaiting > Active > StuckInput > Idle > shell_prompt > default Active
    //
    // These tests verify every pairwise combination where two signals co-occur.

    #[test]
    fn test_priority_completed_waiting_beats_active() {
        let capture = "completion sent via ./apps/orchestrator/mail/complete\nCompiling patina v0.1";
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_priority_completed_waiting_beats_stuck_input() {
        let prompt = fake_prompt("pat-x1", "Fix", "W1");
        let capture = format!("waiting for next assignment\n{prompt}");
        assert_eq!(detect_pane_state(&capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_priority_completed_waiting_beats_idle() {
        let capture = "completion sent via ./apps/orchestrator/mail/complete\n❯ ";
        assert_eq!(detect_pane_state(capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_priority_active_beats_stuck_input() {
        let prompt = fake_prompt("pat-x2", "Add feature", "W2");
        // Every active_work_re pattern should override stuck input
        for active_indicator in &[
            "Searching for files...",
            "Reading 5 files",
            "Running cargo test",
            "Editing src/main.rs",
            "Updated src/lib.rs",
            "Applied patch to file",
            "Tool use: Bash",
            "Compiling patina-runner",
            "Clauding...",
            "running stop hook",
            "✢ Compacting database",
        ] {
            let capture = format!("{active_indicator}\n{prompt}");
            assert_eq!(
                detect_pane_state(&capture),
                WorkerState::Active,
                "active indicator '{active_indicator}' should beat stuck input"
            );
        }
    }

    #[test]
    fn test_priority_active_beats_idle() {
        let capture = "Compiling crate\nwaiting.$";
        assert_eq!(detect_pane_state(capture), WorkerState::Active);
    }

    #[test]
    fn test_priority_stuck_input_beats_idle() {
        let prompt = fake_prompt("pat-x3", "Refactor", "W3");
        let capture = format!("{prompt}\n❯ ");
        assert_eq!(detect_pane_state(&capture), WorkerState::StuckInput);
    }

    #[test]
    fn test_priority_stuck_input_beats_default_active() {
        // Without stuck input, unrecognized text defaults to Active.
        // With stuck input markers, it should be StuckInput instead.
        let prompt = fake_prompt("pat-x4", "Debug", "W4");
        let capture = format!("some random unrecognized text\n{prompt}");
        assert_eq!(detect_pane_state(&capture), WorkerState::StuckInput);
    }

    #[test]
    fn test_default_active_when_nothing_matches() {
        // No known patterns → default Active (assumes work in progress)
        let capture = "abcdef ghijkl 12345 random bytes";
        assert_eq!(detect_pane_state(capture), WorkerState::Active);
    }

    // --- Stop-hook edge case with stuck input ---

    #[test]
    fn test_stop_hook_overrides_completed_waiting_even_with_stuck_input() {
        // CompletedWaiting matched, but stop hook active → Active (not StuckInput)
        let prompt = fake_prompt("pat-x5", "Fix", "W5");
        let capture = format!(
            "completion sent via ./apps/orchestrator/mail/complete\n{prompt}\n✻ Clauding… (running stop hook · 2m)"
        );
        assert_eq!(detect_pane_state(&capture), WorkerState::Active);
    }

    // --- Column-width wrapping robustness ---
    //
    // The original bug: regex `.*` and plain `.contains()` can't match
    // substrings that are split by \n from visual line wrapping.
    // These tests verify detection works at every realistic pane width.

    #[test]
    fn test_stuck_input_detected_at_all_realistic_pane_widths() {
        let prompt = fake_prompt("pat-wrap1", "Implement physics solver", "PearlFox");

        // Test every width from very narrow (20 cols) to wide (200 cols)
        for cols in (20..=200).step_by(5) {
            let wrapped = wrap_at_cols(&prompt, cols);
            assert!(
                has_stuck_input(&wrapped),
                "has_stuck_input failed at {cols} cols. Wrapped text:\n{wrapped}"
            );
            // Also verify detect_pane_state classifies it as StuckInput
            // (not default Active, which was the original bug)
            assert_eq!(
                detect_pane_state(&wrapped),
                WorkerState::StuckInput,
                "detect_pane_state returned wrong state at {cols} cols"
            );
        }
    }

    #[test]
    fn test_stuck_input_with_shell_prompt_prefix_at_all_widths() {
        // Realistic: shell prompt + typed text
        let prompt = fake_prompt("pat-wrap2", "Add caching layer", "GrayBay");
        let full_text = format!("❯ {prompt}");

        for cols in (20..=200).step_by(5) {
            let wrapped = wrap_at_cols(&full_text, cols);
            assert!(
                has_stuck_input(&wrapped),
                "failed with shell prompt prefix at {cols} cols"
            );
        }
    }

    #[test]
    fn test_stuck_input_with_preceding_scrollback_at_all_widths() {
        // Realistic: previous output in scrollback + stuck prompt at bottom
        let prompt = fake_prompt("pat-wrap3", "Fix rendering bug", "TopazOtter");
        let scrollback = "test foo::bar ... ok\ntest foo::baz ... ok\ntest result: ok. 42 passed\n";
        let full_text = format!("{scrollback}❯ {prompt}");

        for cols in (25..=150).step_by(10) {
            let wrapped = wrap_at_cols(&full_text, cols);
            assert!(
                has_stuck_input(&wrapped),
                "failed with scrollback + prompt at {cols} cols"
            );
        }
    }

    // --- Detection consistency: has_stuck_input vs tmux detection ---
    //
    // worker::has_stuck_input and tmux::prompt_text_visible_in_capture
    // must always agree. We can't call the tmux function directly (it's
    // private), but we can replicate its logic to verify consistency.

    #[test]
    fn test_detection_functions_agree_on_all_prompt_variants() {
        // Replicate tmux::prompt_text_visible_in_capture logic
        fn tmux_check(capture: &str) -> bool {
            let flat = capture.replace('\n', "");
            flat.contains("/skill flywheel-worker") && flat.contains("Work bead")
        }

        let prompts = vec![
            fake_prompt("pat-c1", "Fix bug", "WorkerA"),
            fake_prompt("pat-c2", "Add feature with spaces and 'quotes'", "Worker-With-Dashes"),
            fake_prompt("pat-c3", "A very long title that goes on and on to stress-test wrapping behavior", "ExtremelyLongWorkerNameThatShouldStillWork"),
            fake_prompt("pat-c4", "", "W"), // empty title
        ];

        for prompt in &prompts {
            for cols in &[20, 25, 30, 40, 60, 80, 120, 200] {
                let wrapped = wrap_at_cols(prompt, *cols);

                let worker_detects = has_stuck_input(&wrapped);
                let tmux_detects = tmux_check(&wrapped);

                assert_eq!(
                    worker_detects, tmux_detects,
                    "Detection disagreement at {cols} cols for prompt starting with '{}'",
                    &prompt[..prompt.len().min(50)]
                );
                // Both should detect it (prompt always contains both markers)
                assert!(
                    worker_detects,
                    "Both should detect stuck input at {cols} cols"
                );
            }
        }
    }

    #[test]
    fn test_detection_functions_agree_on_non_prompts() {
        fn tmux_check(capture: &str) -> bool {
            let flat = capture.replace('\n', "");
            flat.contains("/skill flywheel-worker") && flat.contains("Work bead")
        }

        let non_prompts = vec![
            "Compiling patina v0.1",
            "❯ ",
            "✢ Searching for files...",
            "completion sent via ./apps/orchestrator/mail/complete",
            "test result: ok. 42 passed; 0 failed",
            "/skill flywheel-worker but no work bead here",
            "Work bead pat-123 but no flywheel-worker here",
            "",
        ];

        for text in &non_prompts {
            assert_eq!(
                has_stuck_input(text),
                tmux_check(text),
                "Detection disagreement on non-prompt: '{text}'"
            );
            assert!(
                !has_stuck_input(text),
                "False positive on non-prompt: '{text}'"
            );
        }
    }

    // --- Realistic multi-state pane capture simulations ---
    //
    // These simulate actual pane captures from a running swarm, combining
    // multiple signals that might co-occur in real scrollback.

    #[test]
    fn test_realistic_capture_worker_actively_compiling_with_old_prompt_in_scrollback() {
        // Worker received prompt, Claude started, now compiling.
        // The prompt text may still be in the 80-line capture window.
        let prompt = fake_prompt("pat-r1", "Implement physics", "Worker1");
        let capture = format!(
            "{prompt}\n\
             ✢ Reading 3 files...\n\
             Applied patch to src/physics.rs\n\
             Compiling patina-runner v0.1.0\n\
             Compiling patina-orchestrator v0.1.0"
        );
        // Active should win because Compiling is in the capture
        assert_eq!(detect_pane_state(&capture), WorkerState::Active);
    }

    #[test]
    fn test_realistic_capture_worker_completed_with_old_prompt_in_scrollback() {
        let prompt = fake_prompt("pat-r2", "Add signals", "Worker2");
        let capture = format!(
            "{prompt}\n\
             ✢ Running cargo test...\n\
             test result: ok. 12 passed\n\
             completion sent via ./apps/orchestrator/mail/complete\n\
             Waiting for next assignment."
        );
        // CompletedWaiting should win
        assert_eq!(detect_pane_state(&capture), WorkerState::CompletedWaiting);
    }

    #[test]
    fn test_realistic_capture_stuck_prompt_with_previous_test_output() {
        // Previous test output scrolled through, then a new prompt was typed
        // but Enter didn't go through.
        let prompt = fake_prompt("pat-r3", "Fix collision", "Worker3");
        let capture = format!(
            "test physics::body ... ok\n\
             test physics::world ... ok\n\
             test result: ok. 24 passed; 0 failed\n\
             ❯ {prompt}"
        );
        // StuckInput should be detected (no active work indicators)
        assert_eq!(detect_pane_state(&capture), WorkerState::StuckInput);
    }

    #[test]
    fn test_realistic_capture_idle_shell_no_stuck_markers() {
        // Clean idle state — no prompt text, just shell
        let capture = "test result: ok. 24 passed\n\
                        Worked for 2m30s\n\
                        3 files changed\n\
                        ❯ ";
        assert_eq!(detect_pane_state(capture), WorkerState::Idle);
    }

    #[test]
    fn test_realistic_capture_garbled_diff_paste_with_no_prompt_markers() {
        // Someone pasted a diff into the pane (no orchestrator prompt markers)
        let capture = "+loa\n+t\"\n2069 +\n+val\n+ue\"\n+: 0\n+.";
        // No known patterns → default Active
        assert_eq!(detect_pane_state(capture), WorkerState::Active);
    }

    #[test]
    fn test_realistic_capture_garbled_diff_plus_stuck_prompt() {
        // Garbled diff text followed by stuck orchestrator prompt
        let prompt = fake_prompt("pat-r4", "Fix bug", "Worker4");
        let capture = format!("+loa\n+t\"\n2069 +\n+val\n+ue\"\n{prompt}");
        assert_eq!(detect_pane_state(&capture), WorkerState::StuckInput);
    }

    // --- SwarmHealth categorization ---

    #[test]
    fn test_swarm_health_stuck_input_counted_as_idle_assigned() {
        // Verify StuckInput workers count toward idle_assigned_panes,
        // not active_assigned_panes, so stall detection works correctly.
        let workers = vec![
            WorkerInfo {
                pane_index: 3,
                pane_id: "%3".into(),
                worker_name: "W1".into(),
                state: WorkerState::StuckInput,
                completed_waiting: false,
                assigned_bead: Some("pat-h1".into()),
                assigned_status: Some("in_progress".into()),
                assignment_age_secs: 10,
            },
            WorkerInfo {
                pane_index: 4,
                pane_id: "%4".into(),
                worker_name: "W2".into(),
                state: WorkerState::Active,
                completed_waiting: false,
                assigned_bead: Some("pat-h2".into()),
                assigned_status: Some("in_progress".into()),
                assignment_age_secs: 30,
            },
            WorkerInfo {
                pane_index: 5,
                pane_id: "%5".into(),
                worker_name: "W3".into(),
                state: WorkerState::Idle,
                completed_waiting: false,
                assigned_bead: None,
                assigned_status: None,
                assignment_age_secs: 0,
            },
        ];

        // swarm_health requires a DB connection for stale_assignments.
        // We use an in-memory DB with the minimal schema it needs.
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE issues (
                id TEXT PRIMARY KEY,
                status TEXT DEFAULT 'open',
                assignee TEXT,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );"
        ).unwrap();

        let health = swarm_health(&workers, &db).unwrap();
        assert_eq!(health.worker_panes, 3);
        assert_eq!(health.assigned_worker_panes, 2); // W1 (stuck) + W2 (active)
        assert_eq!(health.idle_assigned_panes, 1);    // W1 (stuck → idle bucket)
        assert_eq!(health.active_assigned_panes, 1);   // W2
        assert_eq!(health.unassigned_worker_panes, 1);  // W3
    }

    // --- needs_prompt correctly excludes StuckInput ---

    #[test]
    fn test_needs_prompt_excludes_stuck_input() {
        let prompt = fake_prompt("pat-np1", "Fix", "W");
        // StuckInput panes need C-u before prompting — needs_prompt should be false
        assert!(!needs_prompt(&prompt));

        // But idle and completed-waiting should return true
        assert!(needs_prompt("❯ "));
        assert!(needs_prompt("completion sent via ./apps/orchestrator/mail/complete"));
    }

    // --- Ensure newline flattening doesn't create false positives ---

    #[test]
    fn test_newline_flattening_no_false_positive_from_line_concatenation() {
        // Ensure that joining lines doesn't accidentally create the marker strings.
        // E.g., "flywheel-worke" + "\n" + "r" → "flywheel-worker" after flattening.
        // This IS the desired behavior (it means the prompt was wrapped).
        // But verify that UNRELATED text doesn't false-positive.
        let capture = "/skill something-else\nWork on another bead\n";
        assert!(!has_stuck_input(capture), "unrelated /skill + Work should not match");

        let capture2 = "flywheel-worker status: OK\nWork bead-counter: 5\n";
        // This contains "flywheel-worker" and "Work bead" but NOT "/skill flywheel-worker",
        // so it should NOT match. The function requires the full "/skill flywheel-worker" marker.
        assert!(
            !has_stuck_input(capture2),
            "partial 'flywheel-worker' without '/skill' prefix should not match"
        );
    }

    // --- Prompt format contract: if build_prompt_text changes, detection must still work ---

    #[test]
    fn test_stuck_input_detection_requires_both_markers_present() {
        // Verify the contract: detection requires BOTH "/skill flywheel-worker"
        // AND "Work bead". If the prompt format changes to drop either marker,
        // this test will catch it.
        assert!(!has_stuck_input("/skill flywheel-worker. Do something else."));
        assert!(!has_stuck_input("Work bead pat-123: Fix bug."));
        assert!(has_stuck_input("/skill flywheel-worker. Work bead pat-123: Fix bug."));
    }

    // ========================================================================
    // Regression tests: narrow-pane end-marker detection and mid-word wrapping.
    //
    // Bug: In narrow panes (23 cols), the START of the prompt ("/skill
    // flywheel-worker" + "Work bead") scrolls off the visible capture, so
    // has_stuck_input returned false for genuinely stuck workers. Fixed by
    // adding END-of-prompt markers ("mail-complete" / legacy "complete.sh" + "freehand completion
    // message") with all whitespace stripped to handle mid-word wrapping.
    // ========================================================================

    /// Regression: prompt tail only visible (start scrolled off in narrow pane).
    /// Before fix, this returned false — the worker sat stuck forever.
    #[test]
    fn test_stuck_input_detected_with_only_end_markers_visible() {
        // Simulate what a 23-col pane shows: only the tail of the prompt (new skill format)
        let tail_only = "\
  your inbox for full\n\
  assignment\n\
  details. Close out\n\
  with /skill\n\
  mail-complete,\n\
  not a freehand\n\
  completion message.";
        assert!(
            has_stuck_input(tail_only),
            "end-of-prompt markers must detect stuck input when start is scrolled off"
        );
    }

    /// Regression: legacy complete.sh end markers still detected (transition safety).
    #[test]
    fn test_stuck_input_detected_with_legacy_end_markers() {
        let tail_only = "\
  your inbox for full\n\
  assignment\n\
  details. Close out\n\
  with\n\
  ./apps/orchestrator\n\
  /mail/complete.sh,\n\
  not a freehand\n\
  completion message.";
        assert!(
            has_stuck_input(tail_only),
            "legacy complete.sh end markers must still be detected during transition"
        );
    }

    /// Regression: mid-word wrapping breaks marker detection.
    /// tmux wraps "mail-complete" across lines at column boundaries.
    #[test]
    fn test_stuck_input_mid_word_wrap_mail_complete() {
        // "mail-complete" split across lines with indentation
        let wrapped = "/skill mail-com\n  plete, not a\n  freehand\n  completion\n  message.";
        assert!(
            has_stuck_input(wrapped),
            "mid-word wrap of 'mail-complete' must still be detected"
        );
    }

    /// Regression: mid-word wrapping of "freehand" and "completion" and "message".
    #[test]
    fn test_stuck_input_mid_word_wrap_freehand_completion() {
        // Various mid-word break points (using new skill format)
        let cases = vec![
            "/skill mail-complete, not a free\nhand completion message.",
            "/skill mail-complete, not a freehand comp\nletion message.",
            "/skill mail-complete, not a freehand completion mes\nsage.",
            "/skill mail-com\nplete, not a freeh\nand comple\ntion messa\nge.",
        ];
        for (i, text) in cases.iter().enumerate() {
            assert!(
                has_stuck_input(text),
                "case {i} failed: mid-word wrap not handled: {text}"
            );
        }
    }

    /// Boundary: end markers present but incomplete — should NOT match.
    #[test]
    fn test_stuck_input_partial_end_markers_no_false_positive() {
        // Only mail-complete, no freehand message
        assert!(!has_stuck_input("/skill mail-complete"));
        // Only freehand message, no mail-complete
        assert!(!has_stuck_input("not a freehand completion message."));
        // Neither
        assert!(!has_stuck_input("Compiling patina v0.1"));
    }

    /// End markers must not trigger on normal Claude output that happens
    /// to mention completion.
    #[test]
    fn test_stuck_input_no_false_positive_on_claude_output() {
        // Claude Code activity output — should never match
        let claude_output = vec![
            "✢ Searching for files...\nReading 3 files\nRunning cargo test",
            "✻ Clauding… (2m 30s)\n  Running cargo test\n  Applied patch",
            "Completion sent via /skill mail-complete.\nWaiting for next assignment.",
            "test result: ok. 42 passed; 0 failed\ncomplete",
        ];
        for capture in &claude_output {
            assert!(
                !has_stuck_input(capture),
                "false positive on Claude output: '{capture}'"
            );
        }
    }

    /// Stress: end markers detected at ALL realistic narrow pane widths.
    /// Tests widths from 15 to 30 cols where mid-word wrapping is most likely.
    #[test]
    fn test_stuck_input_end_markers_at_all_narrow_widths() {
        // Use only the TAIL of the prompt (simulating start scrolled off)
        let tail = "details. Close out with /skill mail-complete, \
                    not a freehand completion message.";

        for cols in 15..=40 {
            let wrapped = wrap_at_cols(tail, cols);
            assert!(
                has_stuck_input(&wrapped),
                "end markers failed at {cols} cols. Wrapped:\n{wrapped}"
            );
        }
    }

    /// Stress: full prompt detected at ALL widths via EITHER start or end markers.
    #[test]
    fn test_stuck_input_full_prompt_detected_at_all_widths_via_either_strategy() {
        let prompt = fake_prompt("pat-reg1", "Implement physics solver", "PearlFox");

        for cols in 15..=200 {
            let wrapped = wrap_at_cols(&prompt, cols);
            assert!(
                has_stuck_input(&wrapped),
                "neither start nor end markers detected at {cols} cols"
            );
        }
    }

    /// Variant: prompt tail with preceding scrollback (realistic pane capture).
    #[test]
    fn test_stuck_input_end_markers_with_scrollback_prefix() {
        let scrollback = "test foo::bar ... ok\ntest foo::baz ... ok\n\
                          test result: ok. 42 passed\n❯ ";
        let tail = "your inbox for full assignment details. Close out with \
                    /skill mail-complete, not a freehand completion message.";

        for cols in 20..=35 {
            let full = format!("{scrollback}{tail}");
            let wrapped = wrap_at_cols(&full, cols);
            assert!(
                has_stuck_input(&wrapped),
                "end markers with scrollback failed at {cols} cols"
            );
        }
    }

    /// Variant: prompt with tmux indentation spaces (Claude Code adds 2-space indent).
    #[test]
    fn test_stuck_input_end_markers_with_claude_indentation() {
        // Claude Code indents continuation lines with 2 spaces
        let indented = "\
  /skill mail-compl\n\
  ete, not a freeha\n\
  nd completion mess\n\
  age.";
        assert!(
            has_stuck_input(indented),
            "end markers with Claude Code indentation must be detected"
        );
    }

    /// Cross-module contract: has_stuck_input (worker) is BROADER than
    /// prompt_text_visible_in_capture (tmux). The worker function uses both
    /// start AND end markers for stuck detection. The tmux function uses
    /// ONLY start markers for post-submit verification (end markers cause
    /// false positives in conversation history).
    ///
    /// Invariant: if tmux detects it, worker must also detect it.
    /// Worker may detect cases tmux doesn't (end-marker-only captures).
    #[test]
    fn test_worker_detects_superset_of_tmux() {
        let cases = vec![
            // Full prompt — both detect
            fake_prompt("pat-xm1", "Fix bug", "W1"),
            // Tail only — worker detects, tmux does NOT
            "details. Close out with /skill mail-complete, \
             not a freehand completion message.".to_string(),
        ];

        for (i, text) in cases.iter().enumerate() {
            for cols in &[20, 23, 30, 60, 120] {
                let wrapped = wrap_at_cols(text, *cols);
                let worker_sees = has_stuck_input(&wrapped);
                let tmux_sees = crate::tmux::tests::prompt_text_visible_for_test(&wrapped);

                // Invariant: tmux detection implies worker detection
                if tmux_sees {
                    assert!(worker_sees,
                        "case {i}: tmux detected at {cols} cols but worker didn't"
                    );
                }
            }
        }

        // Verify worker detects tail-only but tmux does NOT
        let tail = "details. Close out with /skill mail-complete, \
                    not a freehand completion message.";
        assert!(has_stuck_input(tail), "worker must detect end markers");
        assert!(
            !crate::tmux::tests::prompt_text_visible_for_test(tail),
            "tmux must NOT detect end markers (false positive risk)"
        );
    }

    // --- extract_completed_bead_id tests ---

    #[test]
    fn test_extract_completed_bead_id_done() {
        assert_eq!(
            extract_completed_bead_id("pat-abc123 done"),
            Some("pat-abc123".to_string()),
        );
    }

    #[test]
    fn test_extract_completed_bead_id_complete() {
        assert_eq!(
            extract_completed_bead_id("pat-xyz99 complete. Sent to coordinator."),
            Some("pat-xyz99".to_string()),
        );
    }

    #[test]
    fn test_extract_completed_bead_id_is_done() {
        assert_eq!(
            extract_completed_bead_id("Bead pat-foo1 is done"),
            Some("pat-foo1".to_string()),
        );
    }

    #[test]
    fn test_extract_completed_bead_id_complete_sh() {
        assert_eq!(
            extract_completed_bead_id("sent via ./apps/orchestrator/mail/complete.sh pat-bar2 --to Coord"),
            Some("pat-bar2".to_string()),
        );
    }

    #[test]
    fn test_extract_completed_bead_id_mail_complete_skill() {
        assert_eq!(
            extract_completed_bead_id("sent via /skill mail-complete pat-baz3 --to Coord"),
            Some("pat-baz3".to_string()),
        );
    }

    #[test]
    fn test_extract_completed_bead_id_none() {
        assert_eq!(extract_completed_bead_id("waiting for next assignment"), None);
        assert_eq!(extract_completed_bead_id(""), None);
        assert_eq!(extract_completed_bead_id("Compiling crate"), None);
    }

    #[test]
    fn test_extract_completed_bead_id_realistic_capture() {
        let capture = "Bead pat-birr9 complete. Collision layers/masks implemented — 37 tests pass. Standing by.";
        assert_eq!(
            extract_completed_bead_id(capture),
            Some("pat-birr9".to_string()),
        );
    }

    #[test]
    fn test_stuck_input_detects_codex_worker_skill() {
        let capture = "❯ Use /skill patina-fly-worker. Work bead pat-123: Fix bug.";
        assert!(has_stuck_input(capture));
    }

    #[test]
    fn test_stuck_input_detects_both_claude_and_codex() {
        assert!(has_stuck_input("❯ Use /skill flywheel-worker. Work bead pat-123: Fix."));
        assert!(has_stuck_input("❯ Use /skill patina-fly-worker. Work bead pat-123: Fix."));
    }

    // --- Usage limit detection ---

    #[test]
    fn test_usage_limit_detected_as_idle() {
        // When Claude hits its usage limit, the pane shows this message.
        // The orchestrator must classify it as Idle so the bead gets reclaimed.
        let capture = "⏺ Background command completed\n\
                        You've hit your limit · resets 4am (America/Los_Angeles)\n\
                        /extra-usage to finish what you're working on.";
        assert_eq!(detect_pane_state(capture), WorkerState::Idle);
    }

    #[test]
    fn test_extra_usage_prompt_detected_as_idle() {
        let capture = "Some output\n/extra-usage to finish what you're working on.\n❯ ";
        assert_eq!(detect_pane_state(capture), WorkerState::Idle);
    }
}
